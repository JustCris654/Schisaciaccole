//! Backend of Schisaciaccole: HTTP API consumed by the desktop app.
//!
//! The tests of the [`store`] module run against a real PostgreSQL through
//! `#[sqlx::test]`: `DATABASE_URL` must be set in the environment when
//! running `cargo test`. The `query!` macros are checked against the
//! committed `.sqlx/` data, so building needs no database.

pub mod app;
pub mod config;
pub mod db;
pub mod store;
