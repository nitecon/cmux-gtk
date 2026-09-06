//! Local terminal listener observations, independent of persisted agent metadata.
use cmux_platform::listeners;
use gtk4::prelude::*;
use std::{path::PathBuf, rc::Rc, time::Duration};
use uuid::Uuid;

/// A listener attributed to an existing local terminal surface.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize)]
pub struct Port {
    surface_uuid: Uuid,
    address: std::net::IpAddr,
    port: u16,
    pid: u32,
    provenance: &'static str,
}

/// Copy native TTY metadata synchronously while GTK guarantees surface lifetime.
fn terminals(state: &crate::app_state::AppState, index: usize) -> Vec<(Uuid, PathBuf)> {
    let Some(engine) = state.split_engines.get(index) else {
        return Vec::new();
    };
    if state.workspaces[index].remote_target.is_some() || engine.remote_launch.is_some() {
        return Vec::new();
    }
    engine
        .all_panes()
        .into_iter()
        .filter_map(|(id, _, _)| {
            let surface = engine.find_surface_by_uuid(&id.to_string())?;
            // SAFETY: GTK owns this engine and native surface for the synchronous metadata getter.
            let tty = unsafe { crate::ghostty::tty::name(surface) }?;
            Some((id, PathBuf::from(tty)))
        })
        .collect()
}

/// Scan only current application descendants and attribute their controlling TTY to supplied native terminals.
fn scan(terminals: &[(Uuid, PathBuf)]) -> std::io::Result<Vec<Port>> {
    let devices: Vec<_> = terminals
        .iter()
        .map(|(id, tty)| listeners::terminal_device(tty).map(|device| (*id, device)))
        .collect::<Result<_, _>>()?;
    let tree = listeners::process_tree(listeners::identity(std::process::id())?)?;
    let mut ports = Vec::new();
    for process in tree {
        let Some(device) = listeners::controlling_terminal(process)? else {
            continue;
        };
        let matches: Vec<_> = devices
            .iter()
            .filter(|(_, candidate)| *candidate == device)
            .collect();
        if matches.len() != 1 {
            continue;
        }
        for listener in listeners::listening_tcp(&[process])? {
            if ports.len() >= 256 {
                return Err(std::io::Error::other("workspace listener limit exceeded"));
            }
            ports.push(Port {
                surface_uuid: matches[0].0,
                address: listener.address,
                port: listener.port,
                pid: process.pid,
                provenance: "local",
            });
        }
    }
    ports.sort_by_key(|port| (port.port, port.surface_uuid, port.pid, port.address));
    ports.dedup();
    Ok(ports)
}

/// Render unique local port numbers without replacing agent metadata or taking focus.
pub fn render(label: &gtk4::Label, ports: Option<&[Port]>) {
    let numbers: std::collections::BTreeSet<_> = ports
        .unwrap_or_default()
        .iter()
        .map(|port| port.port)
        .collect();
    let text = numbers
        .iter()
        .take(8)
        .map(|port| format!(":{port}"))
        .collect::<Vec<_>>()
        .join("  ");
    label.set_text(&text);
    label.set_visible(!numbers.is_empty());
}

/// Observe one workspace per second with one blocking worker; stale terminal topology discards results.
/// Await the blocking worker before scheduling another, even during slow filesystem access.
pub fn start(state: &crate::app_state::AppStateRef, window: &gtk4::ApplicationWindow) {
    let Some(runtime) = state.borrow().runtime_handle.clone() else {
        return;
    };
    let weak = Rc::downgrade(state);
    let task = glib::MainContext::default().spawn_local(async move {
        let mut cursor = 0usize;
        loop {
            glib::timeout_future(Duration::from_secs(1)).await;
            let (id, tty) = {
                let Some(state) = weak.upgrade() else {
                    break;
                };
                let state = state.borrow();
                if state.workspaces.is_empty() {
                    continue;
                }
                let index = cursor % state.workspaces.len();
                cursor = cursor.wrapping_add(1);
                if state.workspaces[index].remote_target.is_some() {
                    continue;
                }
                (state.workspaces[index].uuid, terminals(&state, index))
            };
            let requested = tty.clone();
            let worker = runtime.spawn_blocking(move || {
                let started = std::time::Instant::now();
                let result = scan(&requested);
                crate::diagnostics::record(
                    "workspace.ports.scan",
                    serde_json::json!({
                        "workspace_uuid":id,"duration_us":started.elapsed().as_micros(),
                        "outcome":if result.is_ok() { "success" } else { "error" },
                        "count":result.as_ref().ok().map(Vec::len),
                    }),
                );
                result.ok()
            });
            let value = worker.await.unwrap_or_default();
            let Some(state) = weak.upgrade() else {
                break;
            };
            let mut state = state.borrow_mut();
            let Some(index) = state
                .workspaces
                .iter()
                .position(|workspace| workspace.uuid == id)
            else {
                continue;
            };
            if terminals(&state, index) == tty && state.workspaces[index].ports != value {
                state.workspaces[index].ports = value;
                if let Some(container) = state
                    .sidebar_list
                    .row_at_index(index as i32)
                    .and_then(|row| row.child())
                    .and_then(|row| row.first_child())
                {
                    let mut child = container.first_child();
                    while let Some(widget) = child {
                        child = widget.next_sibling();
                        if widget.has_css_class("workspace-ports") {
                            if let Ok(label) = widget.downcast::<gtk4::Label>() {
                                render(&label, state.workspaces[index].ports.as_deref());
                            }
                            break;
                        }
                    }
                }
            }
        }
    });
    window.connect_destroy(move |_| task.abort());
}
