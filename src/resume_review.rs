//! GTK-only approval controls; the public socket cannot grant execution authority.
use crate::app_state::AppStateRef;
use crate::resume::{ResumeAction, ResumeBinding};
use gtk4::prelude::*;

/// Snapshot the active local terminal's identity and binding without changing focus.
fn selected(state: &AppStateRef) -> Option<(String, ResumeBinding)> {
    let state = state.borrow();
    if state
        .workspaces
        .get(state.active_index)?
        .remote_target
        .is_some()
    {
        return None;
    }
    let engine = state.split_engines.get(state.active_index)?;
    let id = engine.active_pane_uuid()?;
    Some((
        id.clone(),
        engine.resume_action(&id, &ResumeAction::Show).ok()??,
    ))
}

/// Append exact-command review and revocation controls to Preferences on the GTK thread.
/// Approval compares against the current binding again so a stale dialog cannot approve an update.
pub fn append(content: &gtk4::Box, state: &AppStateRef) {
    let selection = selected(state);
    let count = state.borrow().resume_policy.len();
    let title = gtk4::Label::new(Some(&format!("Automatic resume approvals: {count}")));
    title.set_xalign(0.0);
    content.append(&title);
    let review = gtk4::TextView::builder()
        .editable(false)
        .cursor_visible(false)
        .monospace(true)
        .wrap_mode(gtk4::WrapMode::WordChar)
        .build();
    review.buffer().set_text(
        &selection
            .as_ref()
            .map(|(_, binding)| {
                format!(
                    "Command:\n{}\n\nDirectory:\n{}\n\nEnvironment overrides:\n{}",
                    binding.command,
                    binding.cwd.as_deref().unwrap_or("(not set)"),
                    serde_json::to_string_pretty(&binding.environment).unwrap_or_default()
                )
            })
            .unwrap_or_else(|| {
                "Select a local terminal with a resume binding to review its command.".into()
            }),
    );
    let scroll = gtk4::ScrolledWindow::builder()
        .min_content_height(140)
        .max_content_height(220)
        .child(&review)
        .build();
    content.append(&scroll);
    let buttons = gtk4::Box::new(gtk4::Orientation::Horizontal, 8);
    let approve = gtk4::Button::with_mnemonic("_Approve exact command");
    approve.set_sensitive(selection.is_some());
    let revoke = gtk4::Button::with_mnemonic("_Revoke all approvals");
    buttons.append(&approve);
    buttons.append(&revoke);
    content.append(&buttons);
    let result = gtk4::Label::new(Some("Approval allows this exact command, directory and environment to restart in future sessions."));
    result.set_wrap(true);
    result.set_xalign(0.0);
    content.append(&result);
    approve.connect_clicked({
        let state = state.clone();
        let result = result.clone();
        let title = title.clone();
        move |_| {
            if selected(&state) != selection {
                result.set_text("The selected terminal or its binding changed. Reopen Preferences to review it.");
                return;
            }
            let Some((_, binding)) = &selection else { return; };
            let mut state = state.borrow_mut();
            match state.resume_policy.approve(binding) {
                Ok(()) => {
                    state.trigger_session_save();
                    title.set_text(&format!("Automatic resume approvals: {}", state.resume_policy.len()));
                    result.set_text("Approved. Changes to the command, directory or environment require fresh approval.");
                    crate::diagnostics::event(format_args!("resume.approval action=approve outcome=success"));
                }
                Err(error) => result.set_text(error),
            }
        }
    });
    revoke.connect_clicked({
        let state = state.clone();
        move |_| {
            let mut state = state.borrow_mut();
            state.resume_policy.revoke_all();
            state.trigger_session_save();
            title.set_text("Automatic resume approvals: 0");
            result
                .set_text("All automatic approvals revoked. Manual resume bindings are preserved.");
            crate::diagnostics::event(format_args!(
                "resume.approval action=revoke_all outcome=success"
            ));
        }
    });
}
