//! Reconcile page-driven navigation with saved browser locations without taking keyboard focus.
use gtk4::prelude::*;
use std::{rc::Rc, time::Duration};

/// GTK Entry delegates editing to a child; protect both direct and descendant keyboard focus.
pub(crate) fn is_editing(entry: &gtk4::Entry) -> bool {
    entry
        .root()
        .and_then(|root| root.focus())
        .is_some_and(|focus| focus == *entry || focus.is_ancestor(entry))
}

/// Refresh one live surface per second, round-robin; never initialize suspended pages.
/// One owned CLI worker runs at a time. Window destruction aborts both the loop and its worker.
pub fn start(state: &crate::app_state::AppStateRef, window: &gtk4::ApplicationWindow) {
    let Some(runtime) = state.borrow().runtime_handle.clone() else {
        return;
    };
    let weak = Rc::downgrade(state);
    let task = glib::MainContext::default().spawn_local(async move {
        let mut cursor = 0usize;
        loop {
            glib::timeout_future(Duration::from_secs(1)).await;
            let selected = {
                let Some(state) = weak.upgrade() else {
                    break;
                };
                let state = state.borrow();
                let tabs: Vec<_> = state
                    .split_engines
                    .iter()
                    .flat_map(|engine| engine.browser_tabs())
                    .filter(|widgets| {
                        state
                            .browser_sessions
                            .get(&widgets.uuid)
                            .is_some_and(|browser| browser.binary_path.is_some())
                    })
                    .collect();
                if tabs.is_empty() {
                    continue;
                }
                let widgets = &tabs[cursor % tabs.len()];
                cursor = cursor.wrapping_add(1);
                if is_editing(&widgets.url_entry) {
                    continue;
                }
                let browser = &state.browser_sessions[&widgets.uuid];
                let trace = uuid::Uuid::new_v4();
                (
                    widgets.uuid,
                    browser.session_identity(),
                    widgets.url_entry.text().to_string(),
                    runtime.spawn(browser.current_url_async(trace)),
                )
            };
            let (id, session, original, worker) = selected;
            let _cancel = crate::task::AbortOnDrop(worker.abort_handle());
            let Ok(Ok(Some(url))) = worker.await else {
                continue;
            };
            if url.len() > 8192 {
                continue;
            }
            let Some(state) = weak.upgrade() else {
                break;
            };
            let state = state.borrow();
            if state
                .browser_sessions
                .get(&id)
                .is_none_or(|browser| browser.session_identity() != session)
            {
                continue;
            }
            let widgets = state
                .split_engines
                .iter()
                .flat_map(|engine| engine.browser_tabs())
                .find(|widgets| widgets.uuid == id);
            if let Some(widgets) = widgets {
                if !is_editing(&widgets.url_entry)
                    && widgets.url_entry.text() == original
                    && url != original
                {
                    widgets.url_entry.set_text(&url);
                    state.trigger_session_save();
                }
            }
        }
    });
    window.connect_destroy(move |_| task.abort());
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Real GTK entry delegation must be treated as editing; moving focus releases that protection.
    #[test]
    #[ignore = "requires GTK display; run in headless Linux CI"]
    fn browser_address_editing_tracks_delegated_focus() {
        gtk4::init().unwrap();
        let window = gtk4::Window::new();
        let layout = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        let entry = gtk4::Entry::new();
        let other = gtk4::Entry::new();
        layout.append(&entry);
        layout.append(&other);
        window.set_child(Some(&layout));
        window.present();
        assert!(entry.grab_focus());
        assert!(is_editing(&entry));
        assert!(!is_editing(&other));
        assert!(other.grab_focus());
        assert!(!is_editing(&entry));
        assert!(is_editing(&other));
        window.destroy();
    }
}
