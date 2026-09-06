//! GTK capture and bounded asynchronous project-action resolution shared by agent commands.
use crate::socket::{
    commands::RespTx,
    response::{err, ok},
};
use serde_json::{json, Value};

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
    static READS: std::sync::LazyLock<std::sync::Arc<tokio::sync::Semaphore>> =
        std::sync::LazyLock::new(|| std::sync::Arc::new(tokio::sync::Semaphore::new(2)));
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
