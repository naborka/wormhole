# Rust

Repository's own stack, lints and style win over picks and style here. Benchmark rules always apply.

- Gates before done, plus repository's own: `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D warnings`, `cargo test --workspace`. Deny warnings by flag or CI, never `#![deny(warnings)]` in source.
- Follow Rust API Guidelines. Make invalid states unrepresentable: newtypes, enums, `Option` over sentinel values.
- No `.unwrap()` outside tests. `.expect("<why it holds>")` only for invariants.
- Borrow over own: `&str` over `String`, `Cow<'_, str>` when ownership is conditional. Clone on purpose; no hidden clones in closures or iterators. `Vec::with_capacity` when size is known.
- Exhaustive `match`; avoid `_` catch-all where possible.
- Types: private fields, accessors when needed. Builder for complex construction. Composition, not `Deref` inheritance. Derive `Debug`, `Clone`, `PartialEq`, `Default` where sensible.
- No wildcard imports, except preludes and `use super::*` in tests. Import order: std, external, local.
- `unsafe` only when no safe way exists.
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

## Benchmarks

- Run benchmarks alone: no parallel benchmarks, builds or tests. No `target-cpu=native` or other `RUSTFLAGS`.
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
