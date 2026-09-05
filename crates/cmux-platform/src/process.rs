//! Linux process resource inspection without terminal content or environment data.

use std::io;

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

/// Read this process's Linux resources; return an error if procfs is unavailable.
///
/// Performs blocking filesystem I/O; call on a worker, never on GTK's main thread.
pub fn resources() -> io::Result<Resources> {
    let mut sample = parse_status(&std::fs::read_to_string("/proc/self/status")?);
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
