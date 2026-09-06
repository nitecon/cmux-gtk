//! GTK-thread notification targeting, state mutation and focus-intent navigation.
use crate::{
    app_state::AppState,
    inbox::{Action, Record, Scope},
};
use gtk4::prelude::*;
use serde_json::{json, Value};
use uuid::Uuid;

type Error = (&'static str, &'static str);

/// Resolve explicit surface identity across workspaces and reject conflicting scope selectors.
fn target(state: &AppState, scope: &Scope) -> Result<(usize, Uuid), Error> {
    let index = if let Some(surface) = scope.surface_id {
        state
            .split_engines
            .iter()
            .position(|engine| engine.find_pane_id_by_uuid(&surface.to_string()).is_some())
            .ok_or(("not_found", "notification surface not found"))?
    } else if let Some(workspace) = scope.workspace_id {
        state
            .workspaces
            .iter()
            .position(|row| row.uuid == workspace)
            .ok_or(("not_found", "notification workspace not found"))?
    } else {
        state.active_index
    };
    let workspace = state
        .workspaces
        .get(index)
        .ok_or(("not_found", "notification workspace not found"))?;
    if scope.workspace_id.is_some_and(|id| id != workspace.uuid) {
        return Err(("not_found", "surface is not in the selected workspace"));
    }
    let surface = scope
        .surface_id
        .or_else(|| {
            state
                .split_engines
                .get(index)?
                .active_pane_uuid()?
                .parse()
                .ok()
        })
        .ok_or(("not_found", "notification surface not found"))?;
    Ok((index, surface))
}

/// Match historical routing identities without redirecting messages whose surfaces have closed.
fn matches(record: &Record, scope: &Scope) -> bool {
    scope
        .workspace_id
        .is_none_or(|id| id == record.workspace_id)
        && scope.surface_id.is_none_or(|id| id == record.surface_id)
}

/// Apply one admitted operation; only Open and JumpToUnread are allowed to focus the target.
pub fn handle(state: &mut AppState, action: Action) -> Result<Value, Error> {
    let result = match action {
        Action::Create { scope, content } => {
            let (index, surface) = target(state, &scope)?;
            let focused = state.active_index == index
                && state.split_engines[index].active_pane_uuid().as_deref()
                    == Some(&surface.to_string())
                && state
                    .gtk_app
                    .active_window()
                    .is_some_and(|window| window.is_active());
            let id = Uuid::new_v4();
            let workspace = state.workspaces[index].uuid;
            let created_at = glib::DateTime::now_utc()
                .and_then(|date| date.format_iso8601())
                .map(|value| value.to_string())
                .unwrap_or_default();
            let evicted = state.inbox.push(Record {
                id,
                workspace_id: workspace,
                surface_id: surface,
                content,
                created_at,
                is_read: focused,
            });
            crate::diagnostics::record(
                "notification.inbox.create",
                json!({"id":id,"workspace":workspace,"surface":surface,"focused":focused,"evicted":evicted}),
            );
            json!({"id": id, "workspace_id": workspace, "surface_id": surface})
        }
        Action::Clear(scope) => {
            if scope.workspace_id.is_some() || scope.surface_id.is_some() {
                target(state, &scope)?;
            }
            state
                .inbox
                .records
                .retain(|record| !matches(record, &scope));
            for index in 0..state.workspaces.len() {
                if scope
                    .workspace_id
                    .is_none_or(|id| state.workspaces[index].uuid == id)
                    && scope.surface_id.is_none()
                {
                    state.clear_workspace_attention(index);
                }
            }
            json!({"cleared": true, "workspace_id": scope.workspace_id, "surface_id": scope.surface_id})
        }
        Action::MarkRead { id, scope, all } => {
            if id.is_some_and(|id| !state.inbox.records.iter().any(|record| record.id == id)) {
                return Err(("not_found", "notification not found"));
            }
            let mut marked = 0;
            for record in &mut state.inbox.records {
                if (all || id == Some(record.id) || (id.is_none() && matches(record, &scope)))
                    && !record.is_read
                {
                    record.is_read = true;
                    marked += 1;
                }
            }
            json!({"marked_read": marked})
        }
        Action::Dismiss { id, all_read } => {
            let before = state.inbox.records.len();
            state
                .inbox
                .records
                .retain(|record| !(id == Some(record.id) || (all_read && record.is_read)));
            let dismissed = before - state.inbox.records.len();
            if id.is_some() && dismissed == 0 {
                return Err(("not_found", "notification not found"));
            }
            json!({"dismissed": dismissed, "all_read": all_read})
        }
        Action::Open(id) => open(state, id)?,
        Action::JumpToUnread => {
            let id = state
                .inbox
                .records
                .iter()
                .rev()
                .find(|record| !record.is_read)
                .map(|record| record.id);
            match id {
                Some(id) => open(state, id)?,
                None => json!({"opened": false}),
            }
        }
    };
    refresh(state);
    if let Some(sender) = &state.inbox_updates {
        sender.send_replace(());
    }
    state.trigger_session_save();
    Ok(result)
}

/// Select the exact saved terminal tab and mark only this message read after successful routing.
fn open(state: &mut AppState, id: Uuid) -> Result<Value, Error> {
    let record = state
        .inbox
        .records
        .iter()
        .find(|record| record.id == id)
        .cloned()
        .ok_or(("not_found", "notification not found"))?;
    let (index, surface) = target(
        state,
        &Scope {
            workspace_id: Some(record.workspace_id),
            surface_id: Some(record.surface_id),
        },
    )?;
    state.switch_to_index(index);
    if !state.split_engines[index].focus_surface(&surface.to_string()) {
        return Err(("not_found", "notification surface not found"));
    }
    let record = state
        .inbox
        .records
        .iter_mut()
        .find(|record| record.id == id)
        .expect("focus does not remove notification records");
    record.is_read = true;
    let mut result = serde_json::to_value(record).expect("notification record serializes");
    result["opened"] = json!(true);
    Ok(result)
}

/// Reconcile unread rings and sidebar dots without disturbing independent terminal BEL attention.
pub fn refresh(state: &AppState) {
    for (index, engine) in state.split_engines.iter().enumerate() {
        let unread_panes: std::collections::HashSet<_> = state
            .inbox
            .records
            .iter()
            .filter(|record| !record.is_read && record.workspace_id == state.workspaces[index].uuid)
            .filter_map(|record| engine.find_pane_id_by_uuid(&record.surface_id.to_string()))
            .collect();
        for (_, pane, _) in engine.all_panes() {
            if let Some(node) = engine.root.find_node(pane) {
                let unread = unread_panes.contains(&pane);
                let widget = node.widget();
                if unread {
                    widget.add_css_class("notification-unread");
                } else {
                    widget.remove_css_class("notification-unread");
                }
            }
        }
        state.update_sidebar_attention(index);
    }
}
