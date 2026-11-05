//! Backend of Schisaciaccole: HTTP API consumed by the desktop app.
//!
//! The tests of the [`store`] module and the login flow tests in
//! `tests/auth_flow.rs` run against a real PostgreSQL through
//! `#[sqlx::test]`: `DATABASE_URL` must be set in the environment when
//! running `cargo test`. The login flow tests mock Google with `wiremock`
//! and never reach the network. The `query!` macros are checked against the
//! committed `.sqlx/` data, so building needs no database.

pub mod app;
pub mod auth;
pub mod config;
pub mod crypto;
pub mod db;
pub mod error;
pub mod google;
pub mod login_store;
pub mod store;
pub mod tokens;
