//! PostgreSQL connection pool and schema migrations.

use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use thiserror::Error;

const MAX_CONNECTIONS: u32 = 10;

/// Error returned when connecting to or migrating the database fails.
#[derive(Debug, Error)]
pub enum DbError {
    /// The pool could not be created or the server is unreachable.
    #[error("cannot connect to the database: {0}")]
    Connect(#[source] sqlx::Error),

    /// A migration could not be applied.
    #[error("cannot run database migrations: {0}")]
    Migrate(#[source] sqlx::migrate::MigrateError),
}

/// Opens a connection pool of at most 10 connections.
///
/// # Errors
///
/// Returns `Err` when `database_url` is malformed or the database does not
/// accept a first connection.
pub async fn connect(database_url: &str) -> Result<PgPool, DbError> {
    PgPoolOptions::new()
        .max_connections(MAX_CONNECTIONS)
        .connect(database_url)
        .await
        .map_err(DbError::Connect)
}

/// Applies the pending migrations from `migrations/`, embedded at build time.
///
/// Already applied migrations are skipped. A migration whose file changed
/// after being applied is reported as an error, not repaired.
///
/// # Errors
///
/// Returns `Err` when a migration fails or the database is unreachable.
pub async fn migrate(pool: &PgPool) -> Result<(), DbError> {
    sqlx::migrate!("./migrations")
        .run(pool)
        .await
        .map_err(DbError::Migrate)
}
