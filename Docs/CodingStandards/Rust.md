# Rust

Use rustfmt and Clippy. Prefer concrete types and small modules; extract shared behavior instead of adding generic frameworks. Return errors at I/O boundaries; reserve panics for impossible invariants and explain them. Keep application data independent of native resource handles where practical.

Document functions with `///`, modules with `//!`. State purpose, return semantics, relevant errors and side effects. Add `# Safety` for unsafe callers and a safety explanation at unsafe blocks. Keep FFI buffers alive for the entire native borrow. Use RAII for cleanup and weak widget references in callbacks. [Rust API documentation guidelines](https://rust-lang.github.io/api-guidelines/documentation.html).

GTK objects belong to their main thread. Perform blocking I/O on workers and apply only necessary results on GTK. Avoid holding RefCell borrows or mutex guards across callbacks or awaits. Cancel owned tasks on teardown; use bounded queues or latest-value delivery according to semantics. [GTK threading](https://docs.gtk.org/gtk4/section-threading.html).

Tests exercise public behavior and failure boundaries, including production helpers. Run them in CI. Keep unsafe native adapters small enough to inspect separately from workspace policy.
