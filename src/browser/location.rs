//! Reconcile page-driven navigation with saved browser locations without taking keyboard focus.
use gtk4::prelude::*;
use std::{rc::Rc, time::Duration};

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
                if widgets.url_entry.has_focus() {
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
                if !widgets.url_entry.has_focus()
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
