//! Bounded diagnostic delivery and size-limited local log retention.

use std::fs::{self, File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

/// Commands owned by the diagnostic writer; records arrive already serialized.
enum Message {
    Record(Vec<u8>),
    Stop,
}

/// Nonblocking producer handle; a full queue increments a visible drop counter.
pub(super) struct Sender {
    channel: mpsc::SyncSender<Message>,
    pub dropped: Arc<AtomicU64>,
    pub failures: Arc<AtomicU64>,
}

/// Process-lifetime guard that drains queued records before joining the writer.
pub struct Guard {
    channel: mpsc::SyncSender<Message>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Drop for Guard {
    /// Finish pending writes at normal application shutdown.
    fn drop(&mut self) {
        let _ = self.channel.send(Message::Stop);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

impl Sender {
    /// Queue one record without blocking a UI or transport producer.
    pub fn record(&self, record: Vec<u8>) {
        if self.channel.try_send(Message::Record(record)).is_err() {
            self.dropped.fetch_add(1, Ordering::Relaxed);
        }
    }
}

/// Start a writer with a bounded record queue and one retained backup file.
///
/// Errors opening the file or starting the worker are returned to the caller.
pub(super) fn start(path: &Path, capacity: usize, max_bytes: u64) -> io::Result<(Sender, Guard)> {
    let mut file = RotatingFile::open(path, max_bytes)?;
    let (channel, receiver) = mpsc::sync_channel(capacity);
    let failures = Arc::new(AtomicU64::new(0));
    let worker_failures = failures.clone();
    let thread = std::thread::Builder::new()
        .name("cmux-diagnostics".into())
        .spawn(move || {
            while let Ok(Message::Record(record)) = receiver.recv() {
                if file.write(&record).is_err() {
                    worker_failures.fetch_add(1, Ordering::Relaxed);
                }
            }
        })?;
    Ok((
        Sender {
            channel: channel.clone(),
            dropped: Arc::new(AtomicU64::new(0)),
            failures,
        },
        Guard {
            channel,
            thread: Some(thread),
        },
    ))
}

/// Own the active file and byte accounting; rotate before exceeding its limit.
struct RotatingFile {
    path: PathBuf,
    file: Option<File>,
    bytes: u64,
    limit: u64,
}

impl RotatingFile {
    /// Open append output and account for pre-existing bytes before the first write.
    fn open(path: &Path, limit: u64) -> io::Result<Self> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            fs::create_dir_all(parent)?;
        }
        let file = OpenOptions::new().create(true).append(true).open(path)?;
        let bytes = file.metadata()?.len();
        Ok(Self {
            path: path.into(),
            file: Some(file),
            bytes,
            limit,
        })
    }

    /// Append a complete record, preserving the preceding bounded file as `.1`.
    fn write(&mut self, record: &[u8]) -> io::Result<()> {
        if record.len() as u64 > self.limit {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "diagnostic record exceeds file limit",
            ));
        }
        if self.bytes.saturating_add(record.len() as u64) > self.limit {
            self.file.take();
            let mut backup = self.path.as_os_str().to_os_string();
            backup.push(".1");
            fs::rename(&self.path, PathBuf::from(backup))?;
            self.bytes = 0;
        }
        if self.file.is_none() {
            self.file = Some(
                OpenOptions::new()
                    .create(true)
                    .append(true)
                    .open(&self.path)?,
            );
        }
        self.file
            .as_mut()
            .ok_or_else(|| io::Error::other("diagnostic file unavailable"))?
            .write_all(record)?;
        self.bytes += record.len() as u64;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Check rotation and guard draining through the production worker.
    #[test]
    fn drains_and_bounds_retention() {
        let directory = std::env::temp_dir().join(format!("cmux-log-{}", uuid::Uuid::new_v4()));
        let path = directory.join("events.jsonl");
        let (sender, guard) = start(&path, 32, 32).unwrap();
        for _ in 0..10 {
            sender.record(b"{\"ok\":true}\n".to_vec());
        }
        drop(guard);
        assert_eq!(sender.dropped.load(Ordering::Relaxed), 0);
        assert_eq!(sender.failures.load(Ordering::Relaxed), 0);
        assert_eq!(fs::read_dir(&directory).unwrap().count(), 2);
        for entry in fs::read_dir(&directory).unwrap() {
            let path = entry.unwrap().path();
            assert!(fs::metadata(&path).unwrap().len() <= 32);
            for line in fs::read_to_string(path).unwrap().lines() {
                assert!(serde_json::from_str::<serde_json::Value>(line).is_ok());
            }
        }
        fs::remove_dir_all(directory).unwrap();
    }

    /// A full producer queue drops records without waiting for a consumer.
    #[test]
    fn records_backpressure() {
        let (channel, _receiver) = mpsc::sync_channel(1);
        let sender = Sender {
            channel,
            dropped: Arc::new(AtomicU64::new(0)),
            failures: Arc::new(AtomicU64::new(0)),
        };
        sender.record(vec![1]);
        sender.record(vec![2]);
        assert_eq!(sender.dropped.load(Ordering::Relaxed), 1);
    }
}
