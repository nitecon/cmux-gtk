# C

Keep owned C limited to narrow native bridges. Include authoritative library headers; do not hand-copy ABI declarations. Document functions with adjacent `/** ... */` comments describing pointer validity, ownership, output parameters, errors and thread/context requirements. Validate unsupported native backends before using backend handles.

Prefer explicit return codes and caller-owned output buffers. Release resources exactly once; avoid hidden global ownership. Do not allocate when a stack value suffices. Keep Rust safety contracts consistent with the C implementation. Use warnings during compilation and behavior tests through the Rust-facing adapter in CI. GTK objects must obey [GTK threading rules](https://docs.gtk.org/gtk4/section-threading.html).
