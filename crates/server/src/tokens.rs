//! Access tokens issued to the desktop app: HS256 JWTs bound to a session.

use chrono::{DateTime, Utc};
use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

/// Lifetime of an access token, in seconds.
pub const ACCESS_TOKEN_TTL_SECS: u64 = 900;

/// Clock skew tolerated when checking the expiry, in seconds.
const LEEWAY_SECS: u64 = 30;

/// Error returned when issuing or verifying an access token fails.
#[derive(Debug, Error)]
pub enum TokenError {
    /// The token could not be signed.
    #[error("cannot sign access token: {0}")]
    Sign(#[source] jsonwebtoken::errors::Error),

    /// The token is malformed, badly signed or expired.
    #[error("invalid access token: {0}")]
    Invalid(#[source] jsonwebtoken::errors::Error),
}

/// Claims carried by an access token.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AccessClaims {
    /// Id of the user.
    pub sub: Uuid,
    /// Id of the session the token was issued for.
    pub sid: Uuid,
    /// Issue time, seconds since the Unix epoch.
    pub iat: i64,
    /// Expiry time, seconds since the Unix epoch.
    pub exp: i64,
}

/// Issues an access token for `user_id` and `session_id`, valid for
/// [`ACCESS_TOKEN_TTL_SECS`] seconds from `now`.
///
/// # Errors
///
/// Returns `Err(TokenError::Sign)` when signing fails, which in practice
/// only happens with an unusable key.
pub fn issue_access_token(
    key: &[u8],
    user_id: Uuid,
    session_id: Uuid,
    now: DateTime<Utc>,
) -> Result<String, TokenError> {
    let iat = now.timestamp();
    let claims = AccessClaims {
        sub: user_id,
        sid: session_id,
        iat,
        exp: iat.saturating_add_unsigned(ACCESS_TOKEN_TTL_SECS),
    };
    encode(
        &Header::new(Algorithm::HS256),
        &claims,
        &EncodingKey::from_secret(key),
    )
    .map_err(TokenError::Sign)
}

/// Verifies an access token and returns its claims.
///
/// Only HS256 is accepted. The expiry is checked against the current system
/// time with 30 seconds of leeway; `iat` is not checked. Whether the session
/// is still active is not checked here.
///
/// # Errors
///
/// Returns `Err(TokenError::Invalid)` when the token is malformed, uses
/// another algorithm, has a bad signature, lacks `exp`/`sub`/`sid`, or is
/// expired.
pub fn verify_access_token(key: &[u8], token: &str) -> Result<AccessClaims, TokenError> {
    let mut validation = Validation::new(Algorithm::HS256);
    validation.leeway = LEEWAY_SECS;
    validation.set_required_spec_claims(&["exp", "sub"]);
    decode::<AccessClaims>(token, &DecodingKey::from_secret(key), &validation)
        .map(|data| data.claims)
        .map_err(TokenError::Invalid)
}

#[cfg(test)]
mod tests {
    use chrono::Duration;

    use super::*;

    const KEY: &[u8] = b"0123456789abcdef0123456789abcdef";

    #[test]
    fn verify_returns_issued_claims() {
        let user = Uuid::new_v4();
        let session = Uuid::new_v4();
        let now = Utc::now();

        let token = issue_access_token(KEY, user, session, now).unwrap();
        let claims = verify_access_token(KEY, &token).unwrap();

        assert_eq!(claims.sub, user);
        assert_eq!(claims.sid, session);
        assert_eq!(claims.iat, now.timestamp());
        assert_eq!(claims.exp, now.timestamp() + 900);
    }

    #[test]
    fn verify_rejects_other_key() {
        let token = issue_access_token(KEY, Uuid::new_v4(), Uuid::new_v4(), Utc::now()).unwrap();

        let result = verify_access_token(b"another-key-another-key-another-k", &token);

        assert!(matches!(result, Err(TokenError::Invalid(_))));
    }

    #[test]
    fn verify_rejects_expired_token() {
        let issued = Utc::now() - Duration::seconds(900 + 31);
        let token = issue_access_token(KEY, Uuid::new_v4(), Uuid::new_v4(), issued).unwrap();

        let result = verify_access_token(KEY, &token);

        assert!(matches!(result, Err(TokenError::Invalid(_))));
    }

    #[test]
    fn verify_accepts_token_expired_within_leeway() {
        let issued = Utc::now() - Duration::seconds(900 + 10);
        let token = issue_access_token(KEY, Uuid::new_v4(), Uuid::new_v4(), issued).unwrap();

        assert!(verify_access_token(KEY, &token).is_ok());
    }

    #[test]
    fn verify_rejects_garbage() {
        let result = verify_access_token(KEY, "not-a-jwt");

        assert!(matches!(result, Err(TokenError::Invalid(_))));
    }

    #[test]
    fn verify_rejects_tampered_payload() {
        let token = issue_access_token(KEY, Uuid::new_v4(), Uuid::new_v4(), Utc::now()).unwrap();
        let other = issue_access_token(KEY, Uuid::new_v4(), Uuid::new_v4(), Utc::now()).unwrap();
        let parts: Vec<&str> = token.split('.').collect();
        let other_parts: Vec<&str> = other.split('.').collect();
        let forged = format!("{}.{}.{}", parts[0], other_parts[1], parts[2]);

        let result = verify_access_token(KEY, &forged);

        assert!(matches!(result, Err(TokenError::Invalid(_))));
    }
}
