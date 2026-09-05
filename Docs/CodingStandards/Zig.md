# Zig

Zig is maintained inside the complete Ghostty dependency. Use the toolchain required by that revision, its formatter and its own contribution rules. Do not apply repository-wide rewrites to vendored sources. Parent integration builds ReleaseFast with baseline CPU compatibility.

When a Ghostty change is required, document function contracts with native doc comments, make allocator ownership explicit, pair allocation with cleanup, and propagate recoverable errors. Keep exported ABI behavior compatible with generated Rust bindings. Consult the [Zig language reference](https://ziglang.org/documentation/) for the selected version. Push dependency commits to a reachable remote before updating the parent submodule pointer.
