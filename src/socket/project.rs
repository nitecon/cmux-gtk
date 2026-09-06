//! GTK capture and bounded asynchronous project-action resolution shared by agent commands.
use crate::socket::{
    commands::RespTx,
    response::{err, ok},
};
use serde_json::{json, Value};
static READS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
    std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)));

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
        let task = runtime.spawn(async move {
            let worker = tokio::task::spawn_blocking(move || {
                let _permit = permit;
                let global = crate::project_config::global_path();
                crate::project_config::resolve(&captured, global.as_deref())
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
        let mut resolved=match result {
            Ok(Ok(resolved))=>resolved,
            _=>{let _=response.send(err(req_id,"config_error","project configuration could not be resolved"));return;}
        };
        let Some(action)=resolved.actions.remove(&action_id) else {let _=response.send(err(req_id,"not_found","project action not found"));return;};
        if action.fingerprint!=fingerprint {let _=response.send(err(req_id,"changed","project action changed since inspection"));return;}
        use crate::project_config::project_action::{Builtin,Intent,Target};
        let Some(owner)=owner.upgrade() else {return;};
        let mut state=owner.borrow_mut();
        let Some(index)=state.workspaces.iter().position(|w|w.uuid==workspace_id) else {let _=response.send(err(req_id,"not_found","workspace closed"));return;};
        if state.local_workspace_directory(index).as_ref()!=Some(&directory) || state.split_engines[index].active_pane_uuid()!=pane {
            let _=response.send(err(req_id,"changed","workspace execution context changed"));return;
        }
        if response.is_closed(){return;}
        let surface=match action.intent {
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
        crate::diagnostics::record("project.actions.run",json!({"trace_id":trace_id,"workspace_id":workspace_id,"surface_id":surface,"duration_us":started.elapsed().as_micros() as u64,"outcome":"submitted"}));
        let _=response.send(ok(req_id,json!({"workspace_id":workspace_id,"surface_id":surface,"status":"submitted"})));
    });
}
