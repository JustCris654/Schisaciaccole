---
paths:
  - "crates/server/src/**"
  - "crates/server/migrations/**"
  - "crates/server/tests/**"
  - ".sqlx/**"
  - "deploy/docker-compose.yml"
---

# PostgreSQL and SQLx

The server uses SQLx with PostgreSQL 17. Read the affected store/query, numbered migrations and integration tests before changing schema or persistence. Preserve per-user ownership and application session/token handling. Review destructive statements, locks and compatibility with existing data; do not rewrite an applied migration silently.

Development tests use `#[sqlx::test]` and require a role that can create temporary databases. Use only the intended development/test database for validation. Never connect to or mutate a live database without scope authorizing it.

After query/migration changes, apply migrations to the development database with `sqlx migrate run --source crates/server/migrations`, run `cargo sqlx prepare --workspace`, and include `.sqlx/`. CI uses offline builds and `cargo sqlx prepare --workspace --check`. Use the CLI version/install command documented in `README.org`.
