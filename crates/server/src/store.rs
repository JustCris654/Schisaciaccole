//! Database access for users, Google credentials, app sessions and meeting preferences.
//!
//! Most operations take a [`PgPool`]; credential operations also accept a
//! transaction connection so refresh and login can share an advisory lock. The tests need
//! a PostgreSQL reachable through `DATABASE_URL`: `#[sqlx::test]` creates a
//! throwaway database per test and applies the migrations.

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use schisaciaccole_core::api::{MeetingPreference, PersonKind, PreferencePerson};
use sqlx::{Executor, PgPool, Postgres, Transaction};
use uuid::Uuid;

/// A user, identified by the stable Google account id (`sub`).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct User {
    /// Internal id.
    pub id: Uuid,
    /// Google account id, unique per user.
    pub google_sub: String,
    /// Email address from the last login.
    pub email: String,
    /// Name from the last login, if Google provided one.
    pub display_name: Option<String>,
}

/// Encrypted Google refresh token of a user.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GoogleCredentials {
    /// Refresh token, already encrypted by the caller.
    pub refresh_token_enc: Vec<u8>,
    /// Space-separated OAuth scopes granted with the token.
    pub scopes: String,
}

/// An active app session, as found by its refresh token hash.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Session {
    /// Session id.
    pub id: Uuid,
    /// Owner of the session.
    pub user_id: Uuid,
    /// Instant after which the refresh token is no longer valid.
    pub expires_at: DateTime<Utc>,
}

/// Creates the user for `google_sub`, or updates its email and display name.
///
/// The id is assigned on creation and never changes afterwards. A `None`
/// display name overwrites a previously stored one.
///
/// # Errors
///
/// Returns `Err` when the query fails.
pub async fn upsert_user(
    pool: &PgPool,
    google_sub: &str,
    email: &str,
    display_name: Option<&str>,
) -> Result<User, sqlx::Error> {
    sqlx::query_as!(
        User,
        "INSERT INTO users (id, google_sub, email, display_name)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (google_sub)
         DO UPDATE SET email = EXCLUDED.email, display_name = EXCLUDED.display_name
         RETURNING id, google_sub, email, display_name",
        Uuid::new_v4(),
        google_sub,
        email,
        display_name,
    )
    .fetch_one(pool)
    .await
}

/// Looks a user up by internal id.
///
/// Returns `Ok(None)` when no such user exists.
///
/// # Errors
///
/// Returns `Err` when the query fails.
pub async fn find_user(pool: &PgPool, id: Uuid) -> Result<Option<User>, sqlx::Error> {
    sqlx::query_as!(
        User,
        "SELECT id, google_sub, email, display_name FROM users WHERE id = $1",
        id,
    )
    .fetch_optional(pool)
    .await
}

/// Serializes credential changes and access-token refreshes for one user.
///
/// The lock lasts until commit or rollback. Callers must run credential queries
/// on the returned transaction to avoid acquiring another pool connection.
/// Different users normally proceed independently; a hash collision only adds
/// harmless serialization. Google HTTP calls keep their existing timeout.
///
/// # Errors
///
/// Returns an error when acquiring the connection or advisory lock fails.
pub(crate) async fn lock_google_credentials(
    pool: &PgPool,
    user_id: Uuid,
) -> Result<Transaction<'_, Postgres>, sqlx::Error> {
    let mut transaction = pool.begin().await?;
    sqlx::query("SELECT pg_advisory_xact_lock(hashtextextended($1, 0))")
        .bind(user_id.to_string())
        .execute(&mut *transaction)
        .await?;
    Ok(transaction)
}

/// Stores the encrypted refresh token of `user_id`, replacing any previous one.
///
/// # Errors
///
/// Returns `Err` when the query fails, including when `user_id` does not
/// exist (foreign key violation).
pub async fn save_google_credentials<'executor>(
    executor: impl Executor<'executor, Database = Postgres>,
    user_id: Uuid,
    refresh_token_enc: &[u8],
    scopes: &str,
) -> Result<(), sqlx::Error> {
    sqlx::query!(
        "INSERT INTO google_credentials (user_id, refresh_token_enc, scopes)
         VALUES ($1, $2, $3)
         ON CONFLICT (user_id)
         DO UPDATE SET refresh_token_enc = EXCLUDED.refresh_token_enc,
                       scopes = EXCLUDED.scopes,
                       updated_at = now()",
        user_id,
        refresh_token_enc,
        scopes,
    )
    .execute(executor)
    .await?;
    Ok(())
}

/// Loads the Google credentials of `user_id`.
///
/// Returns `Ok(None)` when none are stored. The token is returned still
/// encrypted.
///
/// # Errors
///
/// Returns `Err` when the query fails.
pub async fn google_credentials<'executor>(
    executor: impl Executor<'executor, Database = Postgres>,
    user_id: Uuid,
) -> Result<Option<GoogleCredentials>, sqlx::Error> {
    sqlx::query_as!(
        GoogleCredentials,
        "SELECT refresh_token_enc, scopes FROM google_credentials WHERE user_id = $1",
        user_id,
    )
    .fetch_optional(executor)
    .await
}

/// Deletes the Google credentials of `user_id`.
///
/// Deleting credentials that do not exist is not an error.
///
/// # Errors
///
/// Returns `Err` when the query fails.
pub async fn delete_google_credentials<'executor>(
    executor: impl Executor<'executor, Database = Postgres>,
    user_id: Uuid,
) -> Result<(), sqlx::Error> {
    sqlx::query!("DELETE FROM google_credentials WHERE user_id = $1", user_id)
        .execute(executor)
        .await?;
    Ok(())
}

/// Creates a session and returns its id.
///
/// `refresh_token_hash` must be unique across sessions.
///
/// # Errors
///
/// Returns `Err` when the query fails, including when `user_id` does not
/// exist or the hash is already used by another session.
pub async fn create_session(
    pool: &PgPool,
    user_id: Uuid,
    refresh_token_hash: &[u8],
    device_label: Option<&str>,
    expires_at: DateTime<Utc>,
) -> Result<Uuid, sqlx::Error> {
    let id = Uuid::new_v4();
    sqlx::query!(
        "INSERT INTO app_sessions (id, user_id, refresh_token_hash, device_label, expires_at)
         VALUES ($1, $2, $3, $4, $5)",
        id,
        user_id,
        refresh_token_hash,
        device_label,
        expires_at,
    )
    .execute(pool)
    .await?;
    Ok(id)
}

/// Finds the session with this refresh token hash that is still usable.
///
/// Returns `Ok(None)` when the hash is unknown, the session is revoked, or
/// `expires_at` is not after `now`.
///
/// # Errors
///
/// Returns `Err` when the query fails.
pub async fn find_active_session_by_hash(
    pool: &PgPool,
    refresh_token_hash: &[u8],
    now: DateTime<Utc>,
) -> Result<Option<Session>, sqlx::Error> {
    sqlx::query_as!(
        Session,
        "SELECT id, user_id, expires_at FROM app_sessions
         WHERE refresh_token_hash = $1 AND revoked_at IS NULL AND expires_at > $2",
        refresh_token_hash,
        now,
    )
    .fetch_optional(pool)
    .await
}

/// Finds the session with this id if it is still usable.
///
/// Returns `Ok(None)` when the id is unknown, the session is revoked, or
/// `expires_at` is not after `now`.
///
/// # Errors
///
/// Returns `Err` when the query fails.
pub async fn find_active_session_by_id(
    pool: &PgPool,
    session_id: Uuid,
    now: DateTime<Utc>,
) -> Result<Option<Session>, sqlx::Error> {
    sqlx::query_as!(
        Session,
        "SELECT id, user_id, expires_at FROM app_sessions
         WHERE id = $1 AND revoked_at IS NULL AND expires_at > $2",
        session_id,
        now,
    )
    .fetch_optional(pool)
    .await
}

/// Replaces the refresh token hash and expiry of a session, atomically.
///
/// The update applies only if the session is not revoked and still stores
/// `old_hash`, so of two concurrent refreshes with the same token only one
/// returns `true`. Expiry is not checked: the caller does it through
/// [`find_active_session_by_hash`].
///
/// # Errors
///
/// Returns `Err` when the query fails, including when `new_hash` is already
/// used by another session. Returns `Ok(false)` when the session does not
/// exist, is revoked, or no longer has `old_hash`.
pub async fn rotate_session(
    pool: &PgPool,
    session_id: Uuid,
    old_hash: &[u8],
    new_hash: &[u8],
    new_expires_at: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE app_sessions SET refresh_token_hash = $3, expires_at = $4
         WHERE id = $1 AND refresh_token_hash = $2 AND revoked_at IS NULL",
        session_id,
        old_hash,
        new_hash,
        new_expires_at,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Revokes a session by id, keeping the first revocation time.
///
/// Returns `true` when the session was active and is now revoked, `false`
/// when it does not exist or was already revoked.
///
/// # Errors
///
/// Returns `Err` when the query fails.
pub async fn revoke_session(
    pool: &PgPool,
    session_id: Uuid,
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE app_sessions SET revoked_at = $2 WHERE id = $1 AND revoked_at IS NULL",
        session_id,
        now,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Revokes the session with this refresh token hash.
///
/// Returns `true` when an active session was revoked, `false` when the hash
/// is unknown or the session was already revoked. Expired sessions are
/// revoked like any other.
///
/// # Errors
///
/// Returns `Err` when the query fails.
pub async fn revoke_session_by_hash(
    pool: &PgPool,
    refresh_token_hash: &[u8],
    now: DateTime<Utc>,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "UPDATE app_sessions SET revoked_at = $2
         WHERE refresh_token_hash = $1 AND revoked_at IS NULL",
        refresh_token_hash,
        now,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

/// Database value of a person kind.
fn kind_to_str(kind: PersonKind) -> &'static str {
    match kind {
        PersonKind::Attendee => "attendee",
        PersonKind::Manual => "manual",
    }
}

/// Person kind of a database value, or `None` when it is unknown.
fn kind_from_str(value: &str) -> Option<PersonKind> {
    match value {
        "attendee" => Some(PersonKind::Attendee),
        "manual" => Some(PersonKind::Manual),
        _ => None,
    }
}

/// Creates the preference of `user_id` for `meeting_key`, or replaces the
/// existing one, and returns its id.
///
/// Everything happens in one transaction. When a preference exists for the
/// key, its id is kept, its title and `updated_at` are updated and all its
/// people are replaced; otherwise a new id is assigned. People are stored at
/// positions `0..n` in the given order.
///
/// The content of `people` is not validated beyond what the database
/// enforces.
///
/// # Errors
///
/// Returns `Err` when a query fails, including when `user_id` does not
/// exist or a person breaks a table constraint (attendee kind without email
/// or the reverse). Nothing is stored in that case.
pub async fn upsert_preference(
    pool: &PgPool,
    user_id: Uuid,
    meeting_key: &str,
    meeting_title: &str,
    people: &[PreferencePerson],
) -> Result<Uuid, sqlx::Error> {
    let mut positions = Vec::with_capacity(people.len());
    let mut kinds = Vec::with_capacity(people.len());
    let mut emails = Vec::with_capacity(people.len());
    let mut labels = Vec::with_capacity(people.len());
    for (position, person) in people.iter().enumerate() {
        let position = i32::try_from(position).map_err(|e| sqlx::Error::Encode(Box::new(e)))?;
        positions.push(position);
        kinds.push(kind_to_str(person.kind).to_string());
        emails.push(person.email.clone());
        labels.push(person.label.clone());
    }

    let mut tx = pool.begin().await?;
    let preference_id = sqlx::query_scalar!(
        "INSERT INTO meeting_preferences (id, user_id, meeting_key, meeting_title)
         VALUES ($1, $2, $3, $4)
         ON CONFLICT (user_id, meeting_key)
         DO UPDATE SET meeting_title = EXCLUDED.meeting_title, updated_at = now()
         RETURNING id",
        Uuid::new_v4(),
        user_id,
        meeting_key,
        meeting_title,
    )
    .fetch_one(&mut *tx)
    .await?;
    sqlx::query!(
        "DELETE FROM meeting_preference_people WHERE preference_id = $1",
        preference_id,
    )
    .execute(&mut *tx)
    .await?;
    sqlx::query!(
        "INSERT INTO meeting_preference_people
             (preference_id, position, kind, attendee_email, label)
         SELECT $1, position, kind, attendee_email, label
         FROM UNNEST($2::int[], $3::text[], $4::text[], $5::text[])
             AS p(position, kind, attendee_email, label)",
        preference_id,
        &positions,
        &kinds,
        &emails as &[Option<String>],
        &labels,
    )
    .execute(&mut *tx)
    .await?;
    tx.commit().await?;
    Ok(preference_id)
}

/// Loads the preferences of `user_id` whose meeting key is in `keys`, by
/// meeting key. People are ordered by position.
///
/// Keys without a preference are absent from the map. An empty `keys`
/// returns an empty map without querying.
///
/// # Errors
///
/// Returns `Err` when a query fails, or with `sqlx::Error::Decode` when a
/// stored person has an unknown kind: the whole load is then refused
/// instead of silently losing people.
pub async fn preferences_by_keys(
    pool: &PgPool,
    user_id: Uuid,
    keys: &[String],
) -> Result<HashMap<String, MeetingPreference>, sqlx::Error> {
    if keys.is_empty() {
        return Ok(HashMap::new());
    }
    let preferences = sqlx::query!(
        "SELECT id, meeting_key, meeting_title FROM meeting_preferences
         WHERE user_id = $1 AND meeting_key = ANY($2)",
        user_id,
        keys,
    )
    .fetch_all(pool)
    .await?;
    let ids: Vec<Uuid> = preferences.iter().map(|row| row.id).collect();
    let rows = sqlx::query!(
        "SELECT p.preference_id, p.kind, p.attendee_email, p.label
         FROM meeting_preference_people p
         JOIN meeting_preferences m ON m.id = p.preference_id
         WHERE m.user_id = $1 AND p.preference_id = ANY($2)
         ORDER BY p.preference_id, p.position",
        user_id,
        &ids,
    )
    .fetch_all(pool)
    .await?;

    let mut people_by_id: HashMap<Uuid, Vec<PreferencePerson>> = HashMap::new();
    for row in rows {
        let kind = kind_from_str(&row.kind).ok_or_else(|| {
            sqlx::Error::Decode(format!("unknown preference person kind {:?}", row.kind).into())
        })?;
        people_by_id
            .entry(row.preference_id)
            .or_default()
            .push(PreferencePerson {
                kind,
                email: row.attendee_email,
                label: row.label,
            });
    }

    let mut result = HashMap::with_capacity(preferences.len());
    for row in preferences {
        let people = people_by_id.remove(&row.id).unwrap_or_default();
        result.insert(
            row.meeting_key,
            MeetingPreference {
                id: row.id.to_string(),
                meeting_title: row.meeting_title,
                people,
            },
        );
    }
    Ok(result)
}

/// Deletes the preference `id` if it belongs to `user_id`; its people go
/// with it.
///
/// Returns `true` when a preference was deleted, `false` when the id does
/// not exist or belongs to another user.
///
/// # Errors
///
/// Returns `Err` when the query fails.
pub async fn delete_preference(
    pool: &PgPool,
    user_id: Uuid,
    id: Uuid,
) -> Result<bool, sqlx::Error> {
    let result = sqlx::query!(
        "DELETE FROM meeting_preferences WHERE id = $1 AND user_id = $2",
        id,
        user_id,
    )
    .execute(pool)
    .await?;
    Ok(result.rows_affected() == 1)
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    async fn new_user(pool: &PgPool) -> User {
        upsert_user(pool, "sub-1", "a@example.com", Some("Ada"))
            .await
            .unwrap()
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn upsert_user_creates_then_updates_same_user(pool: PgPool) {
        let created = upsert_user(&pool, "sub-1", "a@example.com", Some("Ada"))
            .await
            .unwrap();

        let updated = upsert_user(&pool, "sub-1", "new@example.com", None)
            .await
            .unwrap();

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.email, "new@example.com");
        assert_eq!(updated.display_name, None);
        assert_eq!(created.display_name.as_deref(), Some("Ada"));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_user_returns_stored_user(pool: PgPool) {
        let user = new_user(&pool).await;

        let found = find_user(&pool, user.id).await.unwrap();

        assert_eq!(found, Some(user));
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn find_user_returns_none_for_missing_id(pool: PgPool) {
        let found = find_user(&pool, Uuid::new_v4()).await.unwrap();

        assert_eq!(found, None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn credentials_are_saved_and_read_back(pool: PgPool) {
        let user = new_user(&pool).await;

        save_google_credentials(&pool, user.id, b"enc-1", "calendar")
            .await
            .unwrap();

        let creds = google_credentials(&pool, user.id).await.unwrap().unwrap();
        assert_eq!(creds.refresh_token_enc, b"enc-1");
        assert_eq!(creds.scopes, "calendar");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn saving_credentials_again_overwrites_them(pool: PgPool) {
        let user = new_user(&pool).await;
        save_google_credentials(&pool, user.id, b"enc-1", "calendar")
            .await
            .unwrap();

        save_google_credentials(&pool, user.id, b"enc-2", "calendar openid")
            .await
            .unwrap();

        let creds = google_credentials(&pool, user.id).await.unwrap().unwrap();
        assert_eq!(creds.refresh_token_enc, b"enc-2");
        assert_eq!(creds.scopes, "calendar openid");
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn credentials_are_none_when_not_saved(pool: PgPool) {
        let user = new_user(&pool).await;

        let creds = google_credentials(&pool, user.id).await.unwrap();

        assert_eq!(creds, None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn deleted_credentials_are_gone(pool: PgPool) {
        let user = new_user(&pool).await;
        save_google_credentials(&pool, user.id, b"enc-1", "calendar")
            .await
            .unwrap();

        delete_google_credentials(&pool, user.id).await.unwrap();

        assert_eq!(google_credentials(&pool, user.id).await.unwrap(), None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn deleting_missing_credentials_is_not_an_error(pool: PgPool) {
        let user = new_user(&pool).await;

        delete_google_credentials(&pool, user.id).await.unwrap();
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn created_session_is_found_by_hash(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        let expires_at = now + Duration::days(30);

        let id = create_session(&pool, user.id, b"hash-1", Some("laptop"), expires_at)
            .await
            .unwrap();

        let session = find_active_session_by_hash(&pool, b"hash-1", now)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.id, id);
        assert_eq!(session.user_id, user.id);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_hash_finds_no_session(pool: PgPool) {
        let found = find_active_session_by_hash(&pool, b"nope", Utc::now())
            .await
            .unwrap();

        assert_eq!(found, None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn expired_session_is_not_found(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        create_session(&pool, user.id, b"hash-1", None, now - Duration::seconds(1))
            .await
            .unwrap();

        let found = find_active_session_by_hash(&pool, b"hash-1", now)
            .await
            .unwrap();

        assert_eq!(found, None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revoked_session_is_not_found(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        let id = create_session(&pool, user.id, b"hash-1", None, now + Duration::days(1))
            .await
            .unwrap();

        assert!(revoke_session(&pool, id, now).await.unwrap());

        let found = find_active_session_by_hash(&pool, b"hash-1", now)
            .await
            .unwrap();
        assert_eq!(found, None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revoking_twice_returns_false_the_second_time(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        let id = create_session(&pool, user.id, b"hash-1", None, now + Duration::days(1))
            .await
            .unwrap();
        revoke_session(&pool, id, now).await.unwrap();

        let again = revoke_session(&pool, id, now).await.unwrap();

        assert!(!again);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn active_session_is_found_by_id(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        let id = create_session(&pool, user.id, b"hash-1", None, now + Duration::days(1))
            .await
            .unwrap();

        let session = find_active_session_by_id(&pool, id, now)
            .await
            .unwrap()
            .unwrap();

        assert_eq!(session.id, id);
        assert_eq!(session.user_id, user.id);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn unknown_id_finds_no_session(pool: PgPool) {
        let found = find_active_session_by_id(&pool, Uuid::new_v4(), Utc::now())
            .await
            .unwrap();

        assert_eq!(found, None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revoked_session_is_not_found_by_id(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        let id = create_session(&pool, user.id, b"hash-1", None, now + Duration::days(1))
            .await
            .unwrap();
        revoke_session(&pool, id, now).await.unwrap();

        let found = find_active_session_by_id(&pool, id, now).await.unwrap();

        assert_eq!(found, None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn expired_session_is_not_found_by_id(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        let id = create_session(&pool, user.id, b"hash-1", None, now - Duration::seconds(1))
            .await
            .unwrap();

        let found = find_active_session_by_id(&pool, id, now).await.unwrap();

        assert_eq!(found, None);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn rotate_session_replaces_hash_and_expiry(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        let id = create_session(&pool, user.id, b"old", None, now + Duration::days(1))
            .await
            .unwrap();
        let new_expiry = now + Duration::days(60);

        let rotated = rotate_session(&pool, id, b"old", b"new", new_expiry)
            .await
            .unwrap();

        assert!(rotated);
        assert_eq!(
            find_active_session_by_hash(&pool, b"old", now)
                .await
                .unwrap(),
            None
        );
        let session = find_active_session_by_hash(&pool, b"new", now)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(session.id, id);
        assert_eq!(session.expires_at.timestamp(), new_expiry.timestamp());
    }

    // A refresh replayed with the already rotated token must lose.
    #[sqlx::test(migrations = "./migrations")]
    async fn rotate_session_succeeds_only_once_per_token(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        let id = create_session(&pool, user.id, b"old", None, now + Duration::days(1))
            .await
            .unwrap();
        let expiry = now + Duration::days(60);

        let first = rotate_session(&pool, id, b"old", b"new-1", expiry)
            .await
            .unwrap();
        let second = rotate_session(&pool, id, b"old", b"new-2", expiry)
            .await
            .unwrap();

        assert!(first);
        assert!(!second);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn rotate_session_fails_after_revoke(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        let id = create_session(&pool, user.id, b"old", None, now + Duration::days(1))
            .await
            .unwrap();
        revoke_session(&pool, id, now).await.unwrap();

        let rotated = rotate_session(&pool, id, b"old", b"new", now + Duration::days(60))
            .await
            .unwrap();

        assert!(!rotated);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revoke_session_by_hash_revokes_known_hash(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        create_session(&pool, user.id, b"hash-1", None, now + Duration::days(1))
            .await
            .unwrap();

        let revoked = revoke_session_by_hash(&pool, b"hash-1", now).await.unwrap();

        assert!(revoked);
        assert_eq!(
            find_active_session_by_hash(&pool, b"hash-1", now)
                .await
                .unwrap(),
            None
        );
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn revoke_session_by_hash_returns_false_for_unknown_hash(pool: PgPool) {
        let revoked = revoke_session_by_hash(&pool, b"nope", Utc::now())
            .await
            .unwrap();

        assert!(!revoked);
    }

    #[sqlx::test(migrations = "./migrations")]
    async fn deleting_user_removes_sessions_and_credentials(pool: PgPool) {
        let user = new_user(&pool).await;
        let now = Utc::now();
        create_session(&pool, user.id, b"hash-1", None, now + Duration::days(1))
            .await
            .unwrap();
        save_google_credentials(&pool, user.id, b"enc", "calendar")
            .await
            .unwrap();

        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user.id)
            .execute(&pool)
            .await
            .unwrap();

        assert_eq!(
            find_active_session_by_hash(&pool, b"hash-1", now)
                .await
                .unwrap(),
            None
        );
        assert_eq!(google_credentials(&pool, user.id).await.unwrap(), None);
    }
}
