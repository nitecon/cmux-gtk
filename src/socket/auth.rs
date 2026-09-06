//! Local connection authentication delegates to the Linux platform boundary.

/// Accept peers owned by the current user; kernel lookup errors reject access.
pub fn validate_peer_uid(stream: &cmux_platform::local_socket::Stream) -> std::io::Result<bool> {
    cmux_platform::peer::same_user(stream)
}
