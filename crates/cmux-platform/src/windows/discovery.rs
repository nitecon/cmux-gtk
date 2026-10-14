//! Windows control endpoint discovery without filesystem socket assumptions.

/// Return the current user's pipe; connection attempts determine server availability.
pub(super) fn discover_socket() -> Option<String> {
    Some(crate::paths::socket_path().to_string_lossy().into_owned())
}
