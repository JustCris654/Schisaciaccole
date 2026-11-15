# AGENTS.md

This file provides guidance to Codex when working with code in this repository.

Schisaciaccole is a Slint desktop countdown for time-boxed Scrum ceremonies. Workspace crates: `crates/core` (`schisaciaccole-core`), `crates/app` (`schisaciaccole`), `crates/server` (`schisaciaccole-server`, axum). `deploy/` holds the server Dockerfile and Compose (PostgreSQL 17). `README.org` is the local source for behavior, setup, packaging and deployment.

## Project agent context

Read [.agents/project.md](.agents/project.md) and [.agents/rules/common.md](.agents/rules/common.md) before project work. Load matching domain rules using the routing in project.md; [.agents/rules/model-routing.md](.agents/rules/model-routing.md) follows the model choices below. Domain and generic profiles are in `.codex/agents/`; their permissions inherit the session.

The `.agents/rules/` files use `paths` frontmatter to describe applicability; Codex does not load them automatically. `.agents/memory/MEMORY.md` is a local Markdown index, not automatic runtime memory. Read pertinent notes when present and verify historical facts against current files.

## Read only when the work needs it

Do not open these for unrelated work. Read the matching domain rules before relevant work:

- Server: [.agents/rules/api.md](.agents/rules/api.md)
- Core: [.agents/rules/core.md](.agents/rules/core.md)
- Desktop: [.agents/rules/desktop.md](.agents/rules/desktop.md)
- Database: [.agents/rules/database.md](.agents/rules/database.md)
- Deployment: [.agents/rules/deploy.md](.agents/rules/deploy.md)
- CI, packaging and versioning: [.agents/rules/ci-versioning.md](.agents/rules/ci-versioning.md)
- Rust (`*.rs`, `Cargo.toml`): [.agents/rules/rust.md](.agents/rules/rust.md). Its complete Rust coding guidelines apply when writing or reviewing Rust.
- A change that alters a documented behavior, route, env var, config key, Compose service or volume, workflow, deploy step, or operational procedure: [.agents/rules/docs.md](.agents/rules/docs.md)

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

**Planning** is done by the currently selected main Codex model.
During planning:
- ask the developer whenever something is unclear or underspecified — do not guess
- push back when something looks wrong (bad approach, conflict with the architecture or with the
  rules in this file, missing edge case) and explain why, before writing the plan

**Implementation** is always delegated to subagents (`spawn_agent` with explicit `model` and
`reasoning_effort` parameters). The planning model picks the model and reasoning effort per task,
based on the task's complexity, and states the choice and reason in the plan. The model names below
are this repository's workflow mapping, not universal model equivalences:
- `gpt-6.1-sol` for complex or risky work: multi-file changes, Rust code, router/CI/
  Docker Compose logic, anything needing judgment. Use medium or high reasoning effort; use high for
  complex or risky work.
- `gpt-6.1-terra` for intermediate implementation work: bounded multi-file changes or
  implementation tasks that need moderate technical judgment but are not especially complex or
  risky. Use medium or high reasoning effort.
- `gpt-6-luna` for simple, mechanical, low-risk work: single-file edits, renames,
  typo fixes, boilerplate, text changes with a precise spec. Use low reasoning effort.
- when unsure, use `gpt-6.1-sol` with medium reasoning effort
- the developer can override the choice at any time

The planning model acts as **orchestrator**: it gives each subagent a self-contained brief, then
always reviews and validates the subagent's output (read the diff, check it against the plan and the
rules in this file, run the checks the developer allows). If something is wrong or incomplete,
spawn an agent again with a brief describing the exact error to fix, and validate again. Never report
a task as done before its output has been validated. The orchestrator commits after validation.

## Git: atomic commits

- One commit = one feature or fix, and the repo stays working. Every commit, the commands in [Test](#test) pass.
- Never mix unrelated changes (refactor, feature, CI/packaging, docs, license, assets). A refactor a feature needs is its own commit, before the feature.
- Plan commits with the tasks. Each validated task is its own commit, made by the orchestrator after validation. If a task touches unrelated areas, split with `git add -p` or path-scoped `git add` before committing.
- Conventional Commits (`feat:`, `fix:`, `refactor:`, `ci:`, `docs:`, `chore:`, optional scope). Subject ≤ 72 characters. Body says what changes and why.
- Commit messages, code, comments, and branch names never reference plan milestones or task ids (`M0`, `M1`, `T3`, …): describe the change itself.
- Push only when the developer asks.

## Exploring the code

The graphify skill is available and is how to explore this codebase: architecture, how files relate, and where a behavior lives. Follow that skill before searching or reading source to answer those questions. When `graphify-out/graph.json` exists, query the graph first, then verify what it cites against the current files. The graph can be stale; rebuild it only when the task is about the graph.
