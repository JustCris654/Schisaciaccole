# Schisaciaccole — project context

Read `AGENTS.md` first: its planning, delegation, validation, commit and Rust policies govern Codex work. Use `.agents/rules/model-routing.md` for profile selection. The main session reviews delegated output and commits after validation; push only on explicit developer request.

## Architecture

`README.org` is the local source for behavior, setup, packaging and deployment.

- `crates/core` (`schisaciaccole-core`): pure quarter-hour/countdown, speaking turn, Daily, attendee, editor-row and meeting preference logic, plus shared API types and route constants. Time is passed into domain functions.
- `crates/app` (`schisaciaccole`): Rust/Slint desktop app, UI in `ui/`, build script, assets, backend client, browser/loopback login and OS keychain sessions. The default build is a local timer. `SCHISACIACCOLE_BACKEND_URL` is fixed at compile time. The app uses meeting timers/turns/Daily and the server's meeting preferences (attendee renames and people added by hand); the editor is local, on a per-event draft.
- `crates/server` (`schisaciaccole-server`): axum Google OAuth/OIDC login, application sessions, Google Calendar meetings, meeting preferences, SQLx PostgreSQL storage and startup migrations. Routes are assembled in `src/app.rs`; shared paths live in core `api.rs`.
- `deploy/`: server Dockerfile and Docker Compose with PostgreSQL 17 and persistent `pgdata`. `SERVER_TAG` defaults to `latest`; use the intended release version for deployment.
- `.github/workflows/ci.yml`, `release.yml`: workspace checks and Linux/macOS packaging, server image publication to GHCR. Packaging also uses `Dockerfile.deb`, `packaging/arch/` and app bundle metadata.

The workspace uses Rust edition 2024 and a shared version in root `Cargo.toml`.

## Commands (repository root unless stated)

```sh
cargo run -p schisaciaccole
SCHISACIACCOLE_BACKEND_URL=https://schisaciaccole.example.com cargo run --release -p schisaciaccole
docker compose -f deploy/docker-compose.yml up -d db
cargo run -p schisaciaccole-server
cargo fmt --all --check
cargo build --workspace
DATABASE_URL=postgres://schisaciaccole:schisaciaccole@localhost:5432/schisaciaccole cargo test --workspace
cargo clippy --workspace --all-targets -- -D warnings
```

Server setup uses root `.env` from `.env.example`; consult `README.org` for OAuth configuration, keys, allowed domain and proxy settings. Do not expose real secrets. Server SQLx tests need a PostgreSQL role allowed to create databases. Run the required checks within the current conversation's scope; record failures or unavailable dependencies explicitly.

CI uses `--locked` and `SQLX_OFFLINE=true`. After a SQL query or migration change, with a development `DATABASE_URL`:

```sh
sqlx migrate run --source crates/server/migrations
cargo sqlx prepare --workspace
```

Commit regenerated `.sqlx/`; CI verifies it with `cargo sqlx prepare --workspace --check`. CLI installation is documented in `README.org`.

Run `cargo bundle --release --format deb` or `--format osx` from `crates/app`, and `makepkg -f` from `packaging/arch`. Consult the README before packaging or deployment; release tags must match the workspace version. Do not publish images, releases or git pushes without the requested scope.

## Rules and memory

Read the matching `.agents/rules/` files before relevant work: `api.md`, `core.md`, `desktop.md`, `deploy.md`, `ci-versioning.md`, `database.md`, `rust.md`, `docs.md`; `common.md` applies to every task. Their `paths` frontmatter describes applicability and is not an automatic Codex loader.

If `graphify-out/graph.json` exists, use the available graphify skill/query workflow to locate relevant areas before source exploration, then verify facts against current files. Graph output may be stale; do not rewrite it for an unrelated task.

`.agents/memory/MEMORY.md` is a local Markdown index, not automatic runtime memory. Read pertinent notes when present; verify historical facts against current sources. `.agents/agent-memory/` documents optional role notes. Project-local skills have been removed; do not reference copied skills.

For documentation updates, read `docs.md`. Local glossaries follow `AGENTS.md`; access to the linked team document must be verified using available tools before claiming an external update.
