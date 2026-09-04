use std::backtrace::Backtrace;
use std::fmt;
use std::fs::{self, OpenOptions};
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;
use std::time::{SystemTime, UNIX_EPOCH};

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

pub fn install_panic_hook() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |panic_info| {
        event(format_args!(
            "PANIC version={} pid={} thread={} {panic_info}\n{}",
            env!("CMUX_VERSION"),
            std::process::id(),
            std::thread::current().name().unwrap_or("unnamed"),
            Backtrace::force_capture(),
        ));
        previous(panic_info);
    }));
}

pub fn log_path() -> &'static Path {
    LOG_PATH.get_or_init(resolve_log_path).as_path()
}

pub fn event(args: fmt::Arguments<'_>) {
    let timestamp_ms = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|duration| duration.as_millis())
        .unwrap_or_default();
    let message = format!("[{timestamp_ms}] {args}");
    eprintln!("cmux: {message}");

    let path = log_path();
    if let Some(parent) = path.parent() {
        let _ = fs::create_dir_all(parent);
    }
    if let Ok(mut file) = OpenOptions::new().create(true).append(true).open(path) {
        let _ = writeln!(file, "{message}");
    }
}

fn resolve_log_path() -> PathBuf {
    if let Some(path) = std::env::var_os("CMUX_LOG").filter(|value| !value.is_empty()) {
        return PathBuf::from(path);
    }
    if let Some(state_home) = std::env::var_os("XDG_STATE_HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(state_home).join("cmux/cmux.log");
    }
    if let Some(home) = std::env::var_os("HOME").filter(|value| !value.is_empty()) {
        return PathBuf::from(home).join(".local/state/cmux/cmux.log");
    }
    std::env::temp_dir().join(format!("cmux-{}.log", std::process::id()))
}
