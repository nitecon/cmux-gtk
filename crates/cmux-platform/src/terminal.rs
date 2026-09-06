//! Bounded terminal identity discovery for command-line callers.

/// Return the first terminal attached to stdin, stdout or stderr, without spawning a helper.
/// Pipes and closed descriptors are skipped. No controlling terminal means no TTY evidence.
pub fn caller_tty() -> Option<String> {
    for fd in [libc::STDIN_FILENO, libc::STDOUT_FILENO, libc::STDERR_FILENO] {
        let mut buffer = [0_u8; 256];
        // SAFETY: ttyname_r borrows this writable buffer only for the call; invalid or non-terminal
        // standard descriptors produce an error. No descriptor ownership is transferred.
        if unsafe { libc::ttyname_r(fd, buffer.as_mut_ptr().cast(), buffer.len()) } != 0 {
            continue;
        }
        let end = buffer.iter().position(|byte| *byte == 0)?;
        let tty = std::str::from_utf8(&buffer[..end]).ok()?;
        if !tty.is_empty() {
            return Some(tty.to_owned());
        }
    }
    None
}
