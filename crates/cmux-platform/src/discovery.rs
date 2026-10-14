//! Control endpoint discovery with shared environment override precedence.

#[cfg_attr(target_os = "linux", path = "linux/discovery.rs")]
#[cfg_attr(windows, path = "windows/discovery.rs")]
mod native;

/// Discover the cmux socket path using the standard search chain.
///
/// Nonempty environment overrides win even when absent on disk. Otherwise try
/// native discovery: Linux tries existing XDG, marker and debug paths, skipping
/// filesystem failures; Windows returns its current-user named pipe. Connecting
/// to the selected endpoint validates its transport behavior.
pub fn discover_socket() -> Option<String> {
    // 1. CMUX_SOCKET env var
    if let Ok(val) = std::env::var("CMUX_SOCKET") {
        if !val.is_empty() {
            return Some(val);
        }
    }

    // Also check CMUX_SOCKET_PATH for backwards compat with Python client
    if let Ok(val) = std::env::var("CMUX_SOCKET_PATH") {
        if !val.is_empty() {
            return Some(val);
        }
    }

    native::discover_socket()
}
