//! GTK capture and bounded asynchronous project-action resolution shared by agent commands.
use crate::socket::{
    commands::RespTx,
    response::{err, ok},
};
use serde_json::{json, Value};
static READS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)));

/// Worker-validated default-layout workspace inputs, ready for GTK allocation.
struct WorkspaceLaunch {
    name: String,
    directory: std::path::PathBuf,
    environment: std::collections::BTreeMap<String, String>,
    color: Option<String>,
}

/// Resolve inline or named workspace intent and validate all supported fields before GTK mutation.
fn prepare_workspace(
    resolved: &crate::project_config::Resolved,
    id: &str,
) -> Result<Option<WorkspaceLaunch>, String> {
    use crate::project_config::project_action::Intent;
    let Some(action) = resolved.actions.get(id) else {
        return Ok(None);
    };
    let intent = match &action.intent {
        Intent::WorkspaceCommand { name } => {
            let command = resolved
                .commands
                .get(name)
                .ok_or("named workspace command not found")?;
            if command
                .definition
                .get("restart")
                .filter(|value| !value.is_null())
                .is_some_and(|value| value.as_str() != Some("new"))
            {
                return Err("named workspace restart policy is not implemented yet".into());
            }
            &command.intent
        }
        intent => intent,
    };
    let Intent::Workspace { workspace } = intent else {
        if matches!(action.intent, Intent::WorkspaceCommand { .. }) {
            return Err("named command does not define a workspace".into());
        }
        return Ok(None);
    };
    if workspace.layout.is_some() || workspace.setup.is_some() {
        return Err("project workspace layout and setup execution are not implemented yet".into());
    }
    if workspace
        .color
        .as_deref()
        .is_some_and(|color| !crate::workspace::valid_workspace_color(color))
    {
        return Err("workspace color requires six-digit RGB hex".into());
    }
    let directory = workspace
        .cwd
        .as_deref()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| resolved.directory.clone());
    let directory = if directory.is_absolute() {
        directory
    } else {
        resolved.directory.join(directory)
    };
    let (name, directory) = crate::workspace::prepare_local_workspace(
        workspace.name.as_deref().unwrap_or_default(),
        &directory,
    )?;
    Ok(Some(WorkspaceLaunch {
        name,
        directory,
        environment: workspace.env.clone(),
        color: workspace.color.clone(),
    }))
}

/// Capture exact workspace/CWD on GTK, then read configuration off-thread without retaining widgets.
/// Started filesystem reads retain their admission permit if the request times out or is cancelled.
pub fn list(
    state: &crate::app_state::AppStateRef,
    workspace: Option<uuid::Uuid>,
    req_id: Value,
    mut response: RespTx,
    trace_id: Option<String>,
) {
    let state = state.borrow();
    let index = workspace
        .and_then(|id| {
            state
                .workspaces
                .iter()
                .position(|workspace| workspace.uuid == id)
        })
        .or_else(|| workspace.is_none().then_some(state.active_index));
    let Some(index) = index.filter(|index| *index < state.workspaces.len()) else {
        let _ = response.send(err(req_id, "not_found", "workspace not found"));
        return;
    };
    let workspace = &state.workspaces[index];
    if workspace.remote_target.is_some() {
        let _ = response.send(err(
            req_id,
            "unsupported",
            "remote project configuration reads are not implemented yet",
        ));
        return;
    }
    let Some(directory) = state.local_workspace_directory(index) else {
        let _ = response.send(err(
            req_id,
            "unavailable",
            "workspace directory unavailable",
        ));
        return;
    };
    let Some(runtime) = state.runtime_handle.clone() else {
        let _ = response.send(err(req_id, "not_running", "async runtime unavailable"));
        return;
    };
    let Ok(permit) = READS.clone().try_acquire_owned() else {
        let _ = response.send(err(req_id, "busy", "project configuration readers busy"));
        return;
    };
    let workspace_id = workspace.uuid;
    runtime.spawn(async move {
        let started=std::time::Instant::now();
        let worker=tokio::task::spawn_blocking(move|| {
            let _permit=permit;
            let global=crate::project_config::global_path();
            crate::project_config::resolve(&directory,global.as_deref())
        });
        let result=tokio::select! {
            _=response.closed()=>{return;},
            result=tokio::time::timeout(std::time::Duration::from_secs(5),worker)=>result,
        };
        let result=match result {
            Ok(Ok(Ok(resolved)))=>Ok(resolved),
            Ok(Ok(Err(message)))=>Err(message),
            Ok(Err(_))=>Err("project configuration worker failed".into()),
            Err(_)=>Err("project configuration read timed out".into()),
        };
        crate::diagnostics::record("project.actions.resolve",json!({"trace_id":trace_id,"workspace_id":workspace_id,"duration_us":started.elapsed().as_micros() as u64,"outcome":if result.is_ok(){"success"}else{"error"}}));
        let reply=match result {Ok(resolved)=>ok(req_id,json!({"workspace_id":workspace_id,"config":resolved})),Err(message)=>err(req_id,"config_error",&message)};
        let _=response.send(reply);
    });
}

/// Re-read an explicitly requested action, then apply only if its reviewed identity and GTK context survive.
/// Successful execution selects the target workspace/tab; listing or possessing a fingerprint never executes.
pub fn run(
    state: &crate::app_state::AppStateRef,
    workspace: Option<uuid::Uuid>,
    action_id: String,
    fingerprint: String,
    req_id: Value,
    mut response: RespTx,
    trace_id: Option<String>,
) {
    let owner = std::rc::Rc::downgrade(state);
    let (workspace_id, directory, pane, task) = {
        let state = state.borrow();
        let index = workspace
            .and_then(|id| state.workspaces.iter().position(|w| w.uuid == id))
            .or_else(|| workspace.is_none().then_some(state.active_index));
        let Some(index) = index.filter(|i| *i < state.workspaces.len()) else {
            let _ = response.send(err(req_id, "not_found", "workspace not found"));
            return;
        };
        let Some(directory) = state.local_workspace_directory(index) else {
            let _ = response.send(err(
                req_id,
                "unsupported",
                "local workspace directory required",
            ));
            return;
        };
        let Some(runtime) = state.runtime_handle.clone() else {
            let _ = response.send(err(req_id, "not_running", "async runtime unavailable"));
            return;
        };
        let Ok(permit) = READS.clone().try_acquire_owned() else {
            let _ = response.send(err(req_id, "busy", "project configuration readers busy"));
            return;
        };
        let pane = state.split_engines[index].active_pane_uuid();
        let captured = directory.clone();
        let selected_action = action_id.clone();
        let task = runtime.spawn(async move {
            let worker = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let global = crate::project_config::global_path();
                let resolved = crate::project_config::resolve(&captured, global.as_deref())?;
                let workspace = prepare_workspace(&resolved, &selected_action)?;
                Ok((resolved, workspace))
            });
            match tokio::time::timeout(std::time::Duration::from_secs(5), worker).await {
                Ok(Ok(result)) => result,
                _ => Err("project configuration could not be resolved".to_string()),
            }
        });
        (state.workspaces[index].uuid, directory, pane, task)
    };
    let cancellation = crate::task::AbortOnDrop(task.abort_handle());
    glib::MainContext::default().spawn_local(async move {
        let _cancellation = cancellation;
        let started=std::time::Instant::now();
        let result=tokio::select!{
            _=response.closed()=>{return;},
            result=task=>result,
        };
        let (mut resolved, workspace_launch)=match result {
            Ok(Ok(resolved))=>resolved,
            Ok(Err(message))=>{let _=response.send(err(req_id,"config_error",&message));return;}
            _=>{let _=response.send(err(req_id,"config_error","project configuration could not be resolved"));return;}
        };
        let Some(action)=resolved.actions.remove(&action_id) else {let _=response.send(err(req_id,"not_found","project action not found"));return;};
        if action.fingerprint!=fingerprint {let _=response.send(err(req_id,"changed","project action changed since inspection"));return;}
        use crate::project_config::project_action::{Builtin,Intent,Target};
        let intent = match action.intent {
            Intent::Agent { agent, args } => Intent::Command { command: crate::project_config::project_action::agent_command(&agent, args.as_deref()) },
            intent => intent,
        };
        let Some(owner)=owner.upgrade() else {return;};
        let mut state=owner.borrow_mut();
        let Some(index)=state.workspaces.iter().position(|w|w.uuid==workspace_id) else {let _=response.send(err(req_id,"not_found","workspace closed"));return;};
        if state.local_workspace_directory(index).as_ref()!=Some(&directory) || state.split_engines[index].active_pane_uuid()!=pane {
            let _=response.send(err(req_id,"changed","workspace execution context changed"));return;
        }
        if response.is_closed(){return;}
        if matches!(intent, Intent::Builtin { builtin: Builtin::NewBrowser }) {
            let Some(pane) = pane else {let _=response.send(err(req_id,"not_found","project browser target missing"));return;};
            drop(state);
            super::handlers::start_browser_lifecycle(&owner, crate::browser::StartupRequest::Open(json!({"url":"about:blank","workspace":workspace_id})), req_id, response, trace_id, Some(pane));
            return;
        }
        let mut result_workspace_id = workspace_id;
        let mut wire_row = false;
        let surface=match intent {
            Intent::Workspace { .. } | Intent::WorkspaceCommand { .. } => {
                let Some(launch) = workspace_launch else {let _=response.send(err(req_id,"config_error","workspace launch unavailable"));return;};
                let created_id = state.create_workspace_configured(launch.name, launch.directory, launch.environment);
                let created = state.active_index;
                if let Some(color) = launch.color { state.set_workspace_color(created_id, Some(color)); }
                result_workspace_id = state.workspaces[created].uuid;
                wire_row = true;
                state.split_engines[created].active_pane_uuid().and_then(|id| uuid::Uuid::parse_str(&id).ok())
            }
            Intent::Builtin { builtin: Builtin::NewWorkspace } => {
                state.create_workspace_bound(String::new(), resolved.directory.clone());
                let created = state.active_index;
                result_workspace_id = state.workspaces[created].uuid;
                wire_row = true;
                state.split_engines[created].active_pane_uuid().and_then(|id| uuid::Uuid::parse_str(&id).ok())
            }
            Intent::Builtin { builtin: Builtin::NewTerminal } => {
                state.switch_to_index(index);
                state.split_engines[index].new_terminal_tab()
            }
            Intent::Builtin { builtin: builtin @ (Builtin::SplitRight | Builtin::SplitDown) } => {
                state.switch_to_index(index);
                let orientation = if builtin == Builtin::SplitRight { gtk4::Orientation::Horizontal } else { gtk4::Orientation::Vertical };
                let engine = &mut state.split_engines[index];
                engine.split_active(orientation).and_then(|new_pane| {
                    engine.all_panes().into_iter().find(|(_, id, _)| *id == new_pane).map(|(uuid, _, _)| uuid)
                })
            }
            Intent::Command { command } => match action.target {
            Target::NewTabInCurrentPane=>{
                state.switch_to_index(index);
                state.split_engines[index].new_project_command(&command,resolved.directory)
            },
            Target::CurrentTerminal=>{
                let native=pane.as_ref().and_then(|id|state.split_engines[index].find_surface_by_uuid(id));
                let Some(native)=native else {let _=response.send(err(req_id,"not_found","current surface is not a terminal"));return;};
                state.switch_to_index(index);
                drop(state);
                // SAFETY: native is live on GTK; borrows are released and no event loop is entered between inputs.
                let delivered=unsafe{crate::ghostty::text::send_literal(native,&command).and_then(|_|crate::ghostty::text::send_character(native,'\r'))};
                if delivered.is_err(){let _=response.send(err(req_id,"input_error","command input rejected"));return;}
                state=owner.borrow_mut();
                pane.as_ref().and_then(|id|uuid::Uuid::parse_str(id).ok())
            }
            },
            _ => {let _=response.send(err(req_id,"unsupported","this action family is not executable yet"));return;}
        };
        let Some(surface)=surface else {let _=response.send(err(req_id,"launch_failed","project terminal could not be created"));return;};
        state.trigger_session_save();
        if wire_row {
            let list = state.sidebar_list.clone();
            let app = state.gtk_app.clone();
            drop(state);
            crate::sidebar::wire_latest_row(&list, owner.clone(), &app);
        }
        crate::diagnostics::record("project.actions.run",json!({"trace_id":trace_id,"workspace_id":result_workspace_id,"source_workspace_id":workspace_id,"surface_id":surface,"duration_us":started.elapsed().as_micros() as u64,"outcome":"submitted"}));
        let _=response.send(ok(req_id,json!({"workspace_id":result_workspace_id,"source_workspace_id":workspace_id,"surface_id":surface,"status":"submitted"})));
    });
}
