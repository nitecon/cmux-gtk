//! Latest-destination navigation for the one browser session shared by visible tabs.
use std::path::PathBuf;
use std::sync::{
    atomic::{AtomicBool, Ordering},
    Arc, Weak,
};
use tokio::sync::{watch, Semaphore};

#[derive(Clone)]
struct Destination {
    url: String,
    visible: Weak<AtomicBool>,
}

/// Retain one pending destination and one serialized worker; teardown aborts its active CLI child.
pub(super) struct MappedNavigation {
    latest: watch::Sender<Option<Destination>>,
    _worker: crate::task::AbortOnDrop,
}

impl MappedNavigation {
    /// Wait for navigation admission, then read the newest still-visible destination before spawning CLI.
    pub(super) fn new(
        runtime: &tokio::runtime::Handle,
        binary: PathBuf,
        session: String,
        gate: Arc<Semaphore>,
    ) -> Self {
        let (latest, mut requests) = watch::channel::<Option<Destination>>(None);
        let worker = runtime.spawn(async move {
            while requests.changed().await.is_ok() {
                let Ok(_permit) = gate.acquire().await else {
                    break;
                };
                let Some(destination) = requests.borrow_and_update().clone() else {
                    continue;
                };
                if !destination
                    .visible
                    .upgrade()
                    .is_some_and(|visible| visible.load(Ordering::Acquire))
                {
                    continue;
                }
                let mut activity = super::metrics::Activity::begin("mapped_navigation", None);
                let result =
                    super::cli::run(&binary, &session, &["open", &destination.url], activity.id)
                        .await;
                activity.finish(if result.is_ok() { "success" } else { "error" });
            }
        });
        Self {
            latest,
            _worker: crate::task::AbortOnDrop(worker.abort_handle()),
        }
    }

    /// Replace the pending destination without queuing each intermediate tab selection.
    pub(super) fn navigate(&self, url: String, visible: &Arc<AtomicBool>) {
        self.latest.send_replace(Some(Destination {
            url,
            visible: Arc::downgrade(visible),
        }));
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Wait for the fixture's complete line without relying on child scheduling order.
    async fn wait_line(path: &std::path::Path) -> String {
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                if let Ok(text) = tokio::fs::read_to_string(path).await {
                    if text.ends_with('\n') {
                        return text;
                    }
                }
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap()
    }

    /// A held navigation slot coalesces destinations; invisible tabs are skipped and teardown kills a live CLI.
    #[tokio::test]
    async fn latest_visible_destination_and_teardown() {
        let directory = std::env::temp_dir().join(format!("cmux-mapped-{}", uuid::Uuid::new_v4()));
        cmux_platform::filesystem::create_private_directory(&directory).unwrap();
        let binary = directory.join("browser");
        std::fs::write(
            &binary,
            br#"#!/bin/sh
[ "$4" = 'open' ] || exit 2
printf '%s\n' "$5" >> "$0.calls"
if [ "$5" = 'hang' ]; then
    printf '%s\n' $$ > "$0.pid"
    exec sleep 60
fi
printf '%s\n' '{"success":true,"data":{}}'
"#,
        )
        .unwrap();
        cmux_platform::filesystem::set_executable_permissions(&binary).unwrap();
        let gate = Arc::new(Semaphore::new(1));
        let permit = gate.clone().acquire_owned().await.unwrap();
        let mapped = MappedNavigation::new(
            &tokio::runtime::Handle::current(),
            binary.clone(),
            "fixture".into(),
            gate,
        );
        let visible = Arc::new(AtomicBool::new(true));
        mapped.navigate("old".into(), &visible);
        tokio::task::yield_now().await;
        mapped.navigate("https://example.test/a b?x=$(false)".into(), &visible);
        drop(permit);
        let calls = directory.join("browser.calls");
        assert_eq!(
            wait_line(&calls).await,
            "https://example.test/a b?x=$(false)\n"
        );
        visible.store(false, Ordering::Release);
        mapped.navigate("hidden".into(), &visible);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert_eq!(
            std::fs::read_to_string(&calls).unwrap(),
            "https://example.test/a b?x=$(false)\n"
        );
        visible.store(true, Ordering::Release);
        mapped.navigate("hang".into(), &visible);
        let pid: u32 = wait_line(&directory.join("browser.pid"))
            .await
            .trim()
            .parse()
            .unwrap();
        drop(mapped);
        tokio::time::timeout(std::time::Duration::from_secs(3), async {
            while PathBuf::from(format!("/proc/{pid}")).exists() {
                tokio::time::sleep(std::time::Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        assert_eq!(
            std::fs::read_to_string(calls).unwrap(),
            "https://example.test/a b?x=$(false)\nhang\n"
        );
        std::fs::remove_dir_all(directory).unwrap();
    }
}
