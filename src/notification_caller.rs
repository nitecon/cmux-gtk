//! Resolve caller identity from stable UUIDs and unique live local PTYs, never borrowed pane focus.
use crate::{
    app_state::AppState,
    inbox::{Caller, Scope},
};
use uuid::Uuid;

type Error = (&'static str, &'static str);

/// Normalize a caller's conventional terminal name without filesystem access or remote path matching.
fn normalize(tty: &str) -> Option<&str> {
    let tty = tty.trim();
    if tty.is_empty() || tty == "not a tty" {
        return None;
    }
    Some(tty.strip_prefix("/dev/").unwrap_or(tty))
}

/// Resolve a unique current local PTY; multiple matches are attribution failure, never first-match wins.
fn tty_target(state: &AppState, tty: &str) -> Option<Scope> {
    let tty = normalize(tty)?;
    let mut found = None;
    for (index, engine) in state.split_engines.iter().enumerate() {
        let workspace = state.workspaces.get(index)?;
        if workspace.remote_target.is_some() || engine.remote_launch.is_some() {
            continue;
        }
        for (id, _, _) in engine.all_panes() {
            let Some(surface) = engine.find_surface_by_uuid(&id.to_string()) else {
                continue;
            };
            // SAFETY: GTK owns the engine and surface for this synchronous non-callback getter.
            if unsafe { crate::ghostty::tty::name(surface) }
                .as_deref()
                .and_then(normalize)
                == Some(tty)
            {
                if found.is_some() {
                    return None;
                }
                found = Some(Scope {
                    workspace_id: Some(workspace.uuid),
                    surface_id: Some(id),
                });
            }
        }
    }
    found
}

/// Use stable terminal identity before live TTY evidence; explicit workspace scope is a hard boundary.
/// A proven workspace with no proven terminal remains workspace-only, including selector-free callers.
pub(crate) fn resolve(state: &AppState, caller: &Caller) -> Result<Scope, Error> {
    let workspace = caller
        .preferred_workspace_id
        .filter(|id| state.workspaces.iter().any(|row| row.uuid == *id));
    let surface = caller.preferred_surface_id.and_then(|surface| {
        state
            .split_engines
            .iter()
            .enumerate()
            .find_map(|(index, engine)| {
                engine.find_pane_id_by_uuid(&surface.to_string())?;
                Some(Scope {
                    workspace_id: Some(state.workspaces.get(index)?.uuid),
                    surface_id: Some(surface),
                })
            })
    });
    if let Some(workspace) = workspace {
        if let Some(surface) = surface.filter(|surface| {
            !caller.preferred_workspace_is_explicit || surface.workspace_id == Some(workspace)
        }) {
            return Ok(surface);
        }
        if let Some(target) = caller
            .caller_tty
            .as_deref()
            .and_then(|tty| tty_target(state, tty))
            .filter(|target| target.workspace_id == Some(workspace))
        {
            return Ok(target);
        }
        return Ok(Scope {
            workspace_id: Some(workspace),
            surface_id: None,
        });
    }
    if caller.preferred_workspace_is_explicit && caller.preferred_workspace_id.is_some() {
        return Err(("not_found", "caller workspace not found"));
    }
    if let Some(surface) = surface {
        return Ok(surface);
    }
    // Upstream prefer_tty cannot override a stronger stable UUID or explicit workspace scope.
    let _prefer_tty = caller.prefer_tty;
    if let Some(target) = caller
        .caller_tty
        .as_deref()
        .and_then(|tty| tty_target(state, tty))
    {
        return Ok(target);
    }
    if caller.preferred_workspace_id.is_some()
        || caller.preferred_surface_id.is_some()
        || caller.caller_tty.as_deref().and_then(normalize).is_some()
    {
        return Err(("not_found", "caller notification target not found"));
    }
    let workspace: Uuid = state
        .workspaces
        .get(state.active_index)
        .ok_or(("not_found", "caller workspace not found"))?
        .uuid;
    Ok(Scope {
        workspace_id: Some(workspace),
        surface_id: None,
    })
}
