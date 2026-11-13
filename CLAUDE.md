# CLAUDE.md

Guidance for Claude Code (claude.ai/code) in this repository.

Schisaciaccole is a Slint desktop countdown for time-boxed Scrum ceremonies. Workspace crates: `crates/core` (`schisaciaccole-core`), `crates/app` (`schisaciaccole`), `crates/server` (`schisaciaccole-server`, axum). `deploy/` holds the server Dockerfile and Compose (PostgreSQL 17). Packaging and releases are in `README.org`.

## Read only when the work needs it

Do not open these for unrelated work. Claude Code also loads them from `.claude/rules/` only when a matching file is in play.

- Shared agent instructions: [.claude/rules/common.md](.claude/rules/common.md). Domain profiles live in `.claude/agents/`.
- Server/core/desktop/database/deployment/CI: read the matching `api.md`, `core.md`, `desktop.md`, `database.md`, `deploy.md` or `ci-versioning.md` in `.claude/rules/` when those paths are in play.
- Rust (`*.rs`, `Cargo.toml`): [.claude/rules/rust.md](.claude/rules/rust.md)
- A change that alters a documented behavior, route, env var, config key, Compose service or volume, workflow, deploy step, or operational procedure: [.claude/rules/docs.md](.claude/rules/docs.md)

## Launch

Desktop app, local timer (no backend):

```sh
cargo run -p schisaciaccole
```

Release build: `cargo run -p schisaciaccole --release`. Verbosity: `RUST_LOG=info` (default `warn`). `RUST_LOG=debug` also logs the option refresh.

App built against a backend. The URL is fixed at compile time. Unset means the local-only timer:

```sh
SCHISACIACCOLE_BACKEND_URL=https://schisaciaccole.example.com cargo run --release -p schisaciaccole
```

Server. Copy `.env.example` to `.env` and fill it. `TOKEN_ENC_KEY` is base64 of exactly 32 bytes. `JWT_SIGNING_KEY` is at least 32 characters. Generate both with `openssl rand -base64 32`. Google OAuth setup is in `README.org` under "Backend (local development)". Then:

```sh
docker compose -f deploy/docker-compose.yml up -d db
cargo run -p schisaciaccole-server
```

The server listens on `BIND_ADDR` (default `0.0.0.0:8080`), loads `.env` from the repo root, and runs migrations at startup.

## Test

Server tests need PostgreSQL and a role that can create databases (`#[sqlx::test]`). Start `db` as above, then:

```sh
DATABASE_URL=postgres://schisaciaccole:schisaciaccole@localhost:5432/schisaciaccole cargo test --workspace
```

Every commit, `cargo fmt --all --check`, `cargo build --workspace`, the test command above, and `cargo clippy --workspace --all-targets -- -D warnings` pass.

CI also passes `--locked` and `SQLX_OFFLINE=true`. After a query or migration change, with `DATABASE_URL` set:

```sh
sqlx migrate run --source crates/server/migrations
cargo sqlx prepare --workspace
```

Commit `.sqlx/`. Install sqlx-cli once with `cargo install sqlx-cli --version 0.8.6 --no-default-features --features rustls,postgres --locked`.

## IMPORTANT: workflow — plan, delegate, validate

**Planning** is the main session model: Opus or Fable, whichever is selected.
- Unclear or underspecified: ask the developer. Do not guess.
- Bad approach, conflict with the architecture or these rules, or a missing edge case: push back and explain why, before writing the plan.

**Implementation** is always delegated to subagents (Agent tool, `model` parameter). The planning model picks the model per task by complexity and states the choice and reason in the plan:
- `opus` — particularly complex only, and only when really necessary: cross-crate or architectural changes, subtle concurrency/async or error-handling.
- `sonnet` — complex or risky: multi-file changes, Rust, router/CI/Docker Compose, anything needing judgment.
- `haiku` — simple, mechanical, low-risk: single-file edits, renames, typo fixes, boilerplate, text with a precise spec.
- Unsure: `sonnet`. The developer can override at any time.

The planning model is the **orchestrator**: a self-contained brief per agent, then review the output (diff vs plan and these rules, plus the checks the developer allows). Wrong or incomplete: re-spawn with a brief of the exact error and validate again. Never report a task done before its output is validated.

## Git: atomic commits

- One commit = one feature or fix, and the repo stays working. Every commit, the commands in [Test](#test) pass.
- Never mix unrelated changes (refactor, feature, CI/packaging, docs, license, assets). A refactor a feature needs is its own commit, before the feature.
- Plan commits with the tasks. Each validated task is its own commit, made by the orchestrator after validation. If a task touches unrelated areas, split with `git add -p` or path-scoped `git add` before committing.
- Conventional Commits (`feat:`, `fix:`, `refactor:`, `ci:`, `docs:`, `chore:`, optional scope). Subject ≤ 72 characters. Body says what changes and why.
- Commit messages, code, comments, and branch names never reference plan milestones or task ids (`M0`, `M1`, `T3`, …): describe the change itself.
- Push only when the developer asks.
