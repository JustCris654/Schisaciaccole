---
paths:
  - "crates/server/**"
  - "crates/core/src/api.rs"
---

# Server API

Read `README.org`, `crates/server/src/app.rs`, core `api.rs` and the affected handler/store/tests before changing routes or contracts. The axum server owns Google OAuth/OIDC login, application access/refresh sessions, Google Calendar meetings and slot templates/plans. The desktop currently does not consume the template/plan endpoints.

Keep shared request/response types and route constants in core aligned with server routes and desktop callers. Preserve authentication/ownership checks, one-time login codes, refresh handling, token encryption and sanitized errors/logs. Request query strings can contain login states and codes: do not log them.

Use mocked Google endpoints in integration tests. Read `database.md` for SQLx changes and `docs.md` for behavior/config updates. Verify with the workspace checks required by the root instructions; server tests need development PostgreSQL.
