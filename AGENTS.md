# AGENTS.md

This file provides guidance to Codex when working with code in this repository.

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
- `opus` maps to `gpt-6-astra` for particularly complex work: cross-crate or architectural changes,
  subtle concurrency/async or error-handling logic, delicate CI/release or Docker Compose deploy
  flows where a mistake ships, anything where the risk of a wrong result is high. Use high or xhigh
  reasoning effort, especially for the most complex work.
- `sonnet` maps to `gpt-6.1-sol` for complex or risky work: multi-file changes, Rust code, router/CI/
  Docker Compose logic, anything needing judgment. Use medium or high reasoning effort; use high for
  complex or risky work.
- `haiku` maps to `gpt-6-luna` for simple, mechanical, low-risk work: single-file edits, renames,
  typo fixes, boilerplate, text changes with a precise spec. Use low reasoning effort.
- when unsure, use `gpt-6.1-sol` with medium reasoning effort
- the developer can override the choice at any time

The planning model acts as **orchestrator**: it gives each subagent a self-contained brief, then
always reviews and validates the subagent's output (read the diff, check it against the plan and the
rules in this file, run the checks the developer allows). If something is wrong or incomplete,
spawn an agent again with a brief describing the exact error to fix, and validate again. Never report
a task as done before its output has been validated. The orchestrator commits after validation.

## Git: atomic commits

- One commit = one change that introduces a single feature or fix and leaves the repo working: at every
  commit `cargo fmt --all --check`, `cargo build --workspace`, `cargo test --workspace` and
  `cargo clippy --workspace --all-targets -- -D warnings` pass.
- Never mix unrelated changes in one commit (refactor, feature, CI/packaging, docs, license, assets).
  A refactor that a feature needs goes in its own commit, before the feature.
- Plan the commits together with the tasks: each validated task ends in its own commit, made by the
  orchestrator after validation. If a task turns out to touch unrelated areas, split it with
  `git add -p` / path-scoped `git add` before committing.
- Conventional Commits messages (`feat:`, `fix:`, `refactor:`, `ci:`, `docs:`, `chore:`, optional scope),
  subject ≤ 72 characters, body saying what changes and why.
- Commit messages, code, comments and branch names never reference plan milestones or task ids
  (`M0`, `M1`, `T3`, …): describe the change itself.
- Push only when the developer asks.

## Rust glossaries

Each crate has a glossary file at `glossary/<crate>.md`; `glossary/Glossario.md` is the index. Each
glossary file has one `## <file>` table per source file (`Nome | Tipo | Riga | Descrizione`) listing
every fn, method, struct, enum, trait, type alias, const, macro and trait impl of the crate.
Excluded: tests, inherent `impl` blocks (their methods are listed), code generated inside `quote!`.
When a change adds, removes, renames or moves a Rust item in a documented crate — line shifts from
edits above an item included — update the affected rows and the commit named in the file's lead.
Check every `Riga` against the source (`sed -n '<line>p' <file>`). A new Rust crate gets its own
glossary file in the same format and an entry in `glossary/Glossario.md`.

## Rust coding guidelines

**Style**
- Simple structure, idiomatic Rust, plain constructs. Readable code beats clever code: no long combinator chains, macro tricks or generics where a loop, a `match` or a concrete type says it more clearly.
- Extract a function when the code is reused, when it isolates a complex or non-obvious part, or when it needs its own unit test. Do not wrap trivial one-liners used once.
- Macros only when a function or a generic cannot do the job; macro input should look like Rust.
- `rustfmt` always. `snake_case` for fn/var, `CamelCase` for types, `SCREAMING_SNAKE_CASE` for const/static, with no prefixes such as `G_`.

**Comments and docs**
- A comment exists only when it helps a future developer working on the code: it explains a complex function or a non-obvious section. No comments that justify design choices or narrate why something was done — with too many comments, none gets read.
- `///` on public items, `//!` for module docs.
- Every function doc states its edge cases and the errors or conditions it does not handle.
- A function returning `Result` always has a `# Errors` section listing when it returns `Err`. A function that can panic has a `# Panics` section.

**Errors and panics**
- Propagate with `?` and add context (`anyhow::Context` in binaries, a dedicated error enum variant in libraries/services) instead of panicking.
- No `unwrap`, `expect`, `panic!`, `assert!` or indexing in non-test code. When a panic is truly acceptable (startup config), use `expect` with a clear message and document it under `# Panics`.
- Check invariants by returning an error, not with `assert!`.

**Tests**
- Unit tests go in a `#[cfg(test)] mod tests` at the bottom of the file; `unwrap` and `expect` are fine there.
- One behavior per test, a name that says it (`rejects_tenant_with_leading_dash`), setup / call / check readable top to bottom. A one-line comment above the test when the case it covers is not obvious from its name.
- Cover the edge cases listed in the function's doc.

**Naming**
- No `get_` prefix on accessors: `fn name()`, not `fn get_name()`. Functions that do I/O use a verb that says so (`fetch_`, `load_`, `write_`).
- Conversions: `as_` (cheap, borrowed), `to_` (expensive), `into_` (takes ownership). Iterators: `iter()` / `iter_mut()` / `into_iter()`.
- Named lifetimes when there is more than one (`'src`, `'ctx`); a single `'a` is fine.

**Data and strings**
- Newtypes for domain values when they prevent mix-ups (`struct Tenant(String)`).
- Slice patterns (`if let [first, .., last] = slice`) and `.get()` instead of indexing.
- `Vec::with_capacity` / `String::with_capacity` when the size is known; arrays for fixed sizes.
- `format!` instead of `+` concatenation; `bytes()` over `chars()` for ASCII-only input; `Cow<str>` when a borrowed value is only sometimes modified.
- Shadowing for transformations: `let port = port.parse()?;`.

**Concurrency and async**
- Async is for I/O; CPU-bound work stays sync (`tokio::task::spawn_blocking` when it is heavy).
- In async code use `tokio::sync` (`Mutex`, `RwLock`, `mpsc`); `std::sync` locks only for short sections. Never hold a lock guard across `.await`.
- Atomics for flags and counters (`AtomicBool`, `AtomicUsize`), not a `Mutex`; choose the memory ordering on purpose.
- When more than one lock is taken, take them always in the same order.
- `std::sync::LazyLock` / `OnceLock` instead of `lazy_static!` / `once_cell`; `thiserror` for library/service errors and `anyhow` for binaries.
