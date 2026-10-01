# Rust

Repository's own stack, lints and style win over picks and style here. Benchmark rules always apply.

- Gates before done, plus repository's own: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo nextest run --workspace`, `cargo test --workspace --doc`. Deny warnings by flag or CI, never `#![deny(warnings)]` in source.
- Tests run with `cargo nextest run`: faster, one process per test, failures only in the report. nextest runs no doctests; `cargo test --doc` does.
- Claim about the code (every `.unwrap()`, every caller of X): `ast-grep run -p '<pattern>' -l rust`, `--json=compact` to count. Regex finds text; this finds syntax.
- Follow Rust API Guidelines. Make invalid states unrepresentable: newtypes, enums, `Option` over sentinel values.
- No `.unwrap()` outside tests. `.expect("<why it holds>")` only for invariants.
- Borrow over own: `&str` over `String`, `Cow<'_, str>` when ownership is conditional. Clone on purpose; no hidden clones in closures or iterators. `Vec::with_capacity` when size is known.
- Exhaustive `match`; avoid `_` catch-all where possible.
- Types: private fields, accessors when needed. Builder for complex construction. Composition, not `Deref` inheritance. Derive `Debug`, `Clone`, `PartialEq`, `Default` where sensible.
- No wildcard imports, except preludes and `use super::*` in tests. Import order: std, external, local.
- `unsafe` only when no safe way exists. Smallest block that covers the operation, never the safe code around it. Any crate carrying `unsafe` runs `cargo miri test` in CI. Rust 2024 spellings: `unsafe extern { }`, `#[unsafe(no_mangle)]`.
- Tests: Arrange-Act-Assert. Fake external APIs and databases behind trait. Real temp directories for file tests.
- Small, light crate that replaces much new code at optimal performance: use it.
- Picks:
  - Errors: `thiserror` in libraries, `anyhow` with `.context()` in apps.
  - Error output: `tracing::error!` or `log::error!`, with subscriber or logger installed in binary. Not `println!`.
  - Concurrency: `tokio` for async, `rayon` for CPU parallelism.
  - Progress: `indicatif` bars for long operations, message fits context.
  - TUI: `ratatui` + `crossterm`, with mouse support. Click position includes scroll offset.
  - HTTP: `axum`. Async handlers return `Result<Response, AppError>`. Layered extractors and shared state struct over global mutable data. `tower-http` layers: timeout, trace, compression. CPU work in `tokio::task::spawn_blocking` or background service.
  - Tables: `polars`. Inspect max 10 rows at once. Print frame alone, not with row count or schema.
  - Secrets: `.env` (in `.gitignore`) via `dotenvy` or `std::env`, held in `secrecy` types.
- Read `Cargo.lock` only when extremely relevant.
- Shipped release profile: `lto = true`, `codegen-units = 1`. Never `target-cpu`: it trades portability for speed, and the benchmark rules below forbid it.
- `#[inline]` on small hot functions, `#[inline(always)]` only where profiling proved it, `#[cold]` on error paths.

## Numbers

- Integer overflow is a decision, never a default: pick `checked_`, `saturating_`, `wrapping_` or `overflowing_` on purpose.
- No `as` for narrowing. `From` widens, `TryFrom` narrows and reports.
- Never compare floats with `==`. Tolerance for equality, `total_cmp` for ordering.
- `NonZero*` where zero is invalid: the compiler forbids it and the niche pays for itself.

## Async

- Never hold a `Mutex` or `RwLock` across `.await`.
- `tokio::fs`, not `std::fs`. CPU work to `spawn_blocking`.
- Independent futures: `join!`, or `try_join!` when they can fail.
- Every future in a `select!` branch must be cancellation-safe.
- Bounded channels, so a slow consumer pushes back instead of growing memory. `oneshot` for request-response, `watch` for latest value, `broadcast` for pub/sub, `JoinSet` for a changing set of tasks.
- `async fn` in traits directly; `async_trait` only for dyn-safety it cannot give.

## Memory

- `Box` a large enum variant: every value of the enum pays for the biggest one.
- `mem::take` or `mem::replace` to move out of a `&mut` instead of cloning.
- `write!` into a buffer over `format!` in a loop. `clone_from` to reuse an allocation. Clear and refill a collection rather than building a new one each pass.
- `Box<[T]>` over `Vec<T>` once the length is fixed.
- Drop order is load-bearing wherever a guard is: struct fields top to bottom, locals in reverse.

## Concurrency

- Weakest correct `Ordering` on every atomic, with a comment saying why when it is not `SeqCst`.
- `std::thread::scope` to borrow stack data across threads.
- `thread_local!` with `Cell` or `RefCell`, never `static mut`.

## Benchmarks

- Run benchmarks alone: no parallel benchmarks, builds or tests. No `target-cpu=native` or other `RUSTFLAGS`.
- Time commands with `hyperfine -N -w 3 -r 10 --export-markdown <file> '<cmd>'`, never bare `time`: warmup, repeats, mean and deviation.
- Never game benchmarks to meet target. Compare apples to apples. Keep each benchmark independent; disable caching that couples them.

## Web front end

- All deep computation in Rust (WASM binary or `dioxus` process), never JavaScript.
- Pico CSS, vanilla JavaScript, own CSS or SCSS file. No jQuery, React or other JavaScript frameworks. No raw Pico defaults.
- Fast UX per common Human Interface Guidelines. Adaptive light and dark theme with toggle. Modern, distinct header and body fonts (Google Fonts OK). Design fits app purpose.
- After last Rust change: rebuild with `wasm-pack build --target web --out-dir web/pkg`.

## Python bindings (PyO3, `maturin`)

- After last Rust change: rebuild with `source .venv/bin/activate && maturin develop --uv --release --features python`. `cargo build --features python` always fails.
- `uv` manages packages and `.venv` (in `.gitignore`), never system Python. `ipykernel` and `ipywidgets` in `.venv` only, not in package requirements.
- Python code: type hints on every signature, `Any` only when unavoidable, `mypy` clean, no mutable default arguments.
