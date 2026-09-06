//! Linux process resource inspection without terminal content or environment data.

use std::io;
use std::io::Read;

/// Replace the calling process with a prepared command, retaining its terminal descriptors.
/// Success never returns; an exec failure returns the OS error without spawning a second process.
pub fn replace_current(command: &mut std::process::Command) -> io::Error {
    use std::os::unix::process::CommandExt;
    command.exec()
}

/// Read the first kernel CPU model name from at most 64 KiB of procfs data.
/// Blocking worker-only I/O; None means unavailable or unsupported, not generic hardware.
/// This identifies one reported model, not every CPU in a heterogeneous system.
pub fn cpu_model() -> Option<String> {
    let mut bytes = Vec::new();
    std::fs::File::open("/proc/cpuinfo")
        .ok()?
        .take(64 * 1024)
        .read_to_end(&mut bytes)
        .ok()?;
    parse_cpu_model(std::str::from_utf8(&bytes).ok()?)
}

/// Select a complete, nonempty model-name line; reject oversized or control-bearing labels.
fn parse_cpu_model(cpuinfo: &str) -> Option<String> {
    cpuinfo.split_inclusive('\n').find_map(|line| {
        if !line.ends_with('\n') {
            return None;
        }
        let (key, value) = line.split_once(':')?;
        let value = value.trim();
        (key.trim() == "model name"
            && !value.is_empty()
            && value.len() <= 256
            && !value.chars().any(char::is_control))
        .then(|| value.to_owned())
    })
}

/// A point-in-time resource sample for this process, in kernel-reported units.
#[derive(Debug, Default)]
pub struct Resources {
    /// Cumulative user-mode CPU microseconds across this process's threads, excluding children.
    pub cpu_user_us: Option<u64>,
    /// Cumulative kernel-mode CPU microseconds across this process's threads, excluding children.
    pub cpu_system_us: Option<u64>,
    /// Resident memory in KiB, or None when unavailable.
    pub rss_kib: Option<u64>,
    /// Peak resident memory in KiB, or None when unavailable.
    pub peak_rss_kib: Option<u64>,
    /// Number of live kernel threads, or None when unavailable.
    pub threads: Option<u64>,
    /// Open file descriptors, excluding the directory used for this sample.
    pub file_descriptors: Option<usize>,
}

/// Read this process's Linux resources; reject unavailable, invalid or over-64-KiB status data.
///
/// Performs blocking filesystem I/O; call on a worker, never on GTK's main thread.
pub fn resources() -> io::Result<Resources> {
    let status =
        crate::filesystem::read_text_bounded(std::path::Path::new("/proc/self/status"), 64 * 1024)?;
    let mut sample = parse_status(&status);
    sample.file_descriptors = std::fs::read_dir("/proc/self/fd")
        .ok()
        .map(|entries| entries.filter_map(Result::ok).count().saturating_sub(1));
    if let Some((user, system)) = cpu_times() {
        sample.cpu_user_us = Some(user);
        sample.cpu_system_us = Some(system);
    }
    Ok(sample)
}

/// Sample cumulative CPU use for all threads, keeping syscall failure distinct from zero.
fn cpu_times() -> Option<(u64, u64)> {
    let mut usage = std::mem::MaybeUninit::<libc::rusage>::uninit();
    // SAFETY: usage points to writable storage of the required size; RUSAGE_SELF
    // needs no external handle. Read the initialized structure only after success.
    if unsafe { libc::getrusage(libc::RUSAGE_SELF, usage.as_mut_ptr()) } != 0 {
        return None;
    }
    // SAFETY: successful getrusage initialized the structure above.
    let usage = unsafe { usage.assume_init() };
    Some((timeval_us(usage.ru_utime)?, timeval_us(usage.ru_stime)?))
}

/// Convert a normalized kernel timeval to microseconds without signed casts or overflow.
fn timeval_us(value: libc::timeval) -> Option<u64> {
    let seconds = u64::try_from(value.tv_sec).ok()?;
    let micros = u64::try_from(value.tv_usec).ok()?;
    if micros >= 1_000_000 {
        return None;
    }
    seconds.checked_mul(1_000_000)?.checked_add(micros)
}

/// Extract known numeric status fields, leaving missing or malformed values absent.
fn parse_status(status: &str) -> Resources {
    let mut sample = Resources::default();
    for line in status.lines() {
        let Some((key, value)) = line.split_once(':') else {
            continue;
        };
        let number = value
            .split_whitespace()
            .next()
            .and_then(|value| value.parse().ok());
        match key {
            "VmRSS" => sample.rss_kib = number,
            "VmHWM" => sample.peak_rss_kib = number,
            "Threads" => sample.threads = number,
            _ => {}
        }
    }
    sample
}

#[cfg(test)]
mod tests {
    use super::*;

    /// CPU indices and unrelated fields are not model identities; truncated or invalid labels stay absent.
    #[test]
    fn cpu_model_uses_complete_kernel_labels() {
        assert_eq!(
            parse_cpu_model(
                "processor : 0\nmodel name\t: Example CPU 12\nmodel name: Second CPU\n"
            ),
            Some("Example CPU 12".into())
        );
        assert_eq!(parse_cpu_model("processor: 0\nHardware: board\n"), None);
        assert_eq!(parse_cpu_model("model name: truncated"), None);
        assert_eq!(
            parse_cpu_model("model name: \nmodel name: valid\n"),
            Some("valid".into())
        );
        assert_eq!(parse_cpu_model("model name: bad\0label\n"), None);
        assert_eq!(
            parse_cpu_model(&format!("model name: {}\n", "x".repeat(257))),
            None
        );
    }

    /// Missing and malformed values remain distinguishable from measured zero.
    #[test]
    fn handles_partial_status() {
        let sample = parse_status("Name:\tcmux\nVmRSS:\t1024 kB\nThreads:\tinvalid\n");
        assert_eq!(sample.rss_kib, Some(1024));
        assert_eq!(sample.threads, None);
        assert_eq!(sample.peak_rss_kib, None);
    }

    /// Reject malformed kernel time values and preserve exact fractional microseconds.
    #[test]
    fn cpu_time_conversion() {
        assert_eq!(
            timeval_us(libc::timeval {
                tv_sec: 2,
                tv_usec: 3
            }),
            Some(2_000_003)
        );
        assert_eq!(
            timeval_us(libc::timeval {
                tv_sec: -1,
                tv_usec: 0
            }),
            None
        );
        assert_eq!(
            timeval_us(libc::timeval {
                tv_sec: 0,
                tv_usec: 1_000_000
            }),
            None
        );
    }

    /// Exercise live procfs access, including the descriptor-count path.
    #[test]
    fn samples_current_process() {
        let sample = resources().unwrap();
        assert!(sample.rss_kib.unwrap() > 0);
        assert!(sample.threads.unwrap() > 0);
        assert!(sample.file_descriptors.unwrap() >= 3);
        let again = resources().unwrap();
        assert!(again.cpu_user_us.unwrap() >= sample.cpu_user_us.unwrap());
        assert!(again.cpu_system_us.unwrap() >= sample.cpu_system_us.unwrap());
    }
}
