---
paths:
  - "**/*.rs"
  - "**/Cargo.toml"
---

# Rust coding guidelines

Read this when writing or reviewing Rust. It is not always-on context.

**Style**
- Simple, idiomatic Rust, plain constructs. Readable over clever: no long combinator chains, macro tricks, or generics where a loop, a `match`, or a concrete type is clearer.
- Extract a function when it is reused, isolates a complex or non-obvious part, or needs its own unit test. Do not wrap a trivial one-liner used once.
- Macros only when a function or a generic cannot do the job. Macro input looks like Rust.
- `rustfmt` always. `snake_case` fn/var, `CamelCase` types, `SCREAMING_SNAKE_CASE` const/static. No prefixes such as `G_`.

**Comments and docs**
- A comment only when it helps a later developer: a complex function or a non-obvious section. No comments that justify a design or narrate why something was done.
- `///` on public items, `//!` for module docs.
- Every function doc states its edge cases and the errors or conditions it does not handle.
- A function returning `Result` has a `# Errors` section listing when it returns `Err`. A function that can panic has a `# Panics` section.

**Errors and panics**
- Propagate with `?` and add context (`anyhow::Context` in binaries, a dedicated error enum variant in libraries/services). Do not panic.
- No `unwrap`, `expect`, `panic!`, `assert!`, or indexing in non-test code. A panic is acceptable only for startup config: `expect` with a clear message, documented under `# Panics`.
- Check invariants by returning an error, not with `assert!`.

**Tests**
- Unit tests in `#[cfg(test)] mod tests` at the bottom of the file. `unwrap` and `expect` are fine there.
- One behavior per test, name that says it (`rejects_tenant_with_leading_dash`). Setup / call / check, top to bottom. A one-line comment above the test when the case is not obvious from the name.
- Cover the edge cases listed in the function doc.

**Naming**
- No `get_` prefix: `fn name()`, not `fn get_name()`. I/O functions use a verb that says so (`fetch_`, `load_`, `write_`).
- Conversions: `as_` (cheap, borrowed), `to_` (expensive), `into_` (ownership). Iterators: `iter()` / `iter_mut()` / `into_iter()`.
- Named lifetimes when there is more than one (`'src`, `'ctx`). A single `'a` is fine.

**Data and strings**
- Newtypes when they prevent mix-ups (`struct Tenant(String)`).
- Slice patterns (`if let [first, .., last] = slice`) and `.get()` instead of indexing.
- `Vec::with_capacity` / `String::with_capacity` when the size is known. Arrays for fixed sizes.
- `format!` instead of `+`. `bytes()` over `chars()` for ASCII-only input. `Cow<str>` when a borrowed value is only sometimes modified.
- Shadow transformations: `let port = port.parse()?;`.

**Concurrency and async**
- Async is for I/O. CPU-bound work stays sync (`tokio::task::spawn_blocking` when it is heavy).
- In async code use `tokio::sync` (`Mutex`, `RwLock`, `mpsc`). `std::sync` locks only for short sections. Never hold a lock guard across `.await`.
- Flags and counters: `AtomicBool`, `AtomicUsize`, not a `Mutex`. Choose the memory ordering on purpose.
- More than one lock: always take them in the same order.
- `std::sync::LazyLock` / `OnceLock` instead of `lazy_static!` / `once_cell`. `thiserror` for library/service errors, `anyhow` for binaries.
