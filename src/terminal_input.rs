//! CMUX owns unfinished human text. Complete human/event messages share one GTK FIFO per terminal.
use crate::{gateway::model::Session, ghostty::ffi};
use gtk4::{glib, prelude::*};
use std::{
    cell::{Cell, RefCell},
    collections::VecDeque,
    rc::Rc,
};
use tokio::sync::oneshot;

const MAX_MESSAGES: usize = 64;
const MAX_TEXT: usize = 256 * 1024;

struct Message {
    target: Session,
    text: String,
    reply: oneshot::Sender<Result<(), String>>,
    allowed: Option<Box<dyn Fn() -> bool>>,
}

/// GTK owns editor state and queue ordering; platform workers inspect processes without GTK handles.
struct Composer {
    area: glib::WeakRef<gtk4::GLArea>,
    surface: Rc<RefCell<Option<ffi::ghostty_surface_t>>>,
    editor: gtk4::TextView,
    container: gtk4::Box,
    status: gtk4::Label,
    target: RefCell<Option<Session>>,
    runtime: RefCell<Option<tokio::runtime::Handle>>,
    queue: RefCell<VecDeque<Message>>,
    draining: Cell<bool>,
    forwarding_control: Cell<bool>,
    history: RefCell<VecDeque<String>>,
    history_index: Cell<Option<usize>>,
    history_draft: RefCell<String>,
}

/// Install one local composer without taking ownership of the native surface or its output.
pub fn attach(area: &gtk4::GLArea, surface: Rc<RefCell<Option<ffi::ghostty_surface_t>>>) {
    let editor = gtk4::TextView::new();
    editor.set_wrap_mode(gtk4::WrapMode::WordChar);
    editor.set_monospace(true);
    editor.set_top_margin(6);
    editor.set_bottom_margin(6);
    editor.set_left_margin(8);
    editor.set_right_margin(8);
    editor.set_tooltip_text(Some("Enter to send; modified Enter adds a newline"));
    let scroll = gtk4::ScrolledWindow::builder()
        .child(&editor)
        .min_content_height(48)
        .max_content_height(160)
        .propagate_natural_height(true)
        .hscrollbar_policy(gtk4::PolicyType::Never)
        .build();
    let status = gtk4::Label::new(Some("Enter to send · Shift+Enter for a newline"));
    status.set_xalign(0.0);
    status.add_css_class("dim-label");
    let container = gtk4::Box::new(gtk4::Orientation::Vertical, 2);
    container.add_css_class("cmux-composer");
    container.append(&scroll);
    container.append(&status);
    container.set_visible(false);
    let composer = Rc::new(Composer {
        area: area.downgrade(),
        surface,
        editor: editor.clone(),
        container,
        status,
        target: RefCell::new(None),
        runtime: RefCell::new(None),
        queue: RefCell::new(VecDeque::new()),
        draining: Cell::new(false),
        forwarding_control: Cell::new(false),
        history: RefCell::new(VecDeque::new()),
        history_index: Cell::new(None),
        history_draft: RefCell::new(String::new()),
    });
    let keys = gtk4::EventControllerKey::new();
    keys.set_propagation_phase(gtk4::PropagationPhase::Capture);
    let weak = Rc::downgrade(&composer);
    keys.connect_key_pressed(move |controller, key, _, modifiers| {
        let Some(composer) = weak.upgrade() else {
            return glib::Propagation::Proceed;
        };
        if matches!(key, gtk4::gdk::Key::Return | gtk4::gdk::Key::KP_Enter) {
            let modifiers = modifiers
                & (gtk4::gdk::ModifierType::SHIFT_MASK
                    | gtk4::gdk::ModifierType::CONTROL_MASK
                    | gtk4::gdk::ModifierType::ALT_MASK
                    | gtk4::gdk::ModifierType::SUPER_MASK
                    | gtk4::gdk::ModifierType::META_MASK
                    | gtk4::gdk::ModifierType::HYPER_MASK);
            if modifiers.is_empty() {
                composer.submit_draft();
            } else {
                composer.editor.buffer().insert_at_cursor("\n");
            }
            return glib::Propagation::Stop;
        }
        // History and all text editing remain local; only explicit non-text controls reach the app.
        let buffer = composer.editor.buffer();
        if modifiers.is_empty()
            && matches!(key, gtk4::gdk::Key::Up | gtk4::gdk::Key::Down)
            && (composer.history_index.get().is_some() || buffer.char_count() == 0)
        {
            composer.recall(key == gtk4::gdk::Key::Up);
            return glib::Propagation::Stop;
        }
        let control = modifiers.intersects(gtk4::gdk::ModifierType::CONTROL_MASK)
            && !modifiers.intersects(gtk4::gdk::ModifierType::ALT_MASK);
        if control && modifiers.intersects(gtk4::gdk::ModifierType::SHIFT_MASK) {
            match key.to_lower() {
                gtk4::gdk::Key::v => {
                    composer.editor.emit_by_name::<()>("paste-clipboard", &[]);
                    return glib::Propagation::Stop;
                }
                gtk4::gdk::Key::c => {
                    composer.editor.emit_by_name::<()>("copy-clipboard", &[]);
                    return glib::Propagation::Stop;
                }
                _ => {}
            }
        }
        if process_control(key, modifiers) {
            if let Some(area) = composer.area.upgrade() {
                composer.forwarding_control.set(true);
                controller.forward(&area);
                composer.forwarding_control.set(false);
                return glib::Propagation::Stop;
            }
        }
        glib::Propagation::Proceed
    });
    let weak = Rc::downgrade(&composer);
    keys.connect_key_released(move |controller, key, _, modifiers| {
        if process_control(key, modifiers) {
            if let Some(composer) = weak.upgrade() {
                if let Some(area) = composer.area.upgrade() {
                    composer.forwarding_control.set(true);
                    controller.forward(&area);
                    composer.forwarding_control.set(false);
                }
            }
        }
    });
    editor.add_controller(keys);
    // SAFETY: this key always owns Rc<Composer>; callbacks and its destructor run on GTK.
    unsafe {
        area.set_data("cmux-composer", composer);
    }
}

/// Return the terminal page with the local editor below its unchanged process-output viewport.
pub fn widget(area: &gtk4::GLArea) -> gtk4::Widget {
    // SAFETY: both private data keys have one concrete type and are GTK-thread confined.
    unsafe {
        if let Some(weak) = area.data::<glib::WeakRef<gtk4::Box>>("cmux-terminal-page") {
            if let Some(page) = weak.as_ref().upgrade() {
                return page.upcast();
            }
        }
        let Some(composer) = composer(area) else {
            return area.clone().upcast();
        };
        let page = gtk4::Box::new(gtk4::Orientation::Vertical, 0);
        page.set_hexpand(true);
        page.set_vexpand(true);
        page.append(area);
        page.append(&composer.container);
        area.set_data("cmux-terminal-page", page.downgrade());
        page.upcast()
    }
}

/// Apply verified process metadata independently of gateway enablement or downstream screen contents.
pub fn synchronize(area: &gtk4::GLArea, target: Option<Session>, runtime: &tokio::runtime::Handle) {
    let Some(composer) = composer(area) else {
        return;
    };
    let enabled = target.is_some();
    let changed = composer.target.borrow().as_ref().map(|s| &s.process)
        != target.as_ref().map(|s| &s.process);
    *composer.target.borrow_mut() = target;
    *composer.runtime.borrow_mut() = Some(runtime.clone());
    composer
        .container
        .set_visible(enabled || composer.editor.buffer().char_count() > 0);
    composer.editor.set_sensitive(enabled);
    if changed && enabled && area.has_focus() {
        composer.editor.grab_focus();
    }
    if !enabled && composer.editor.buffer().char_count() > 0 {
        composer.status.set_text("Process stopped · draft retained");
    }
}

/// Redirect native-area typing to the local editor before Ghostty can forward it to the process.
pub fn redirect(area: &gtk4::GLArea, controller: &gtk4::EventControllerKey) -> bool {
    let Some(composer) = composer(area) else {
        return false;
    };
    if composer.target.borrow().is_none() {
        return false;
    }
    // Events explicitly forwarded from the empty composer are controls, not human composition.
    if composer.forwarding_control.get() {
        return false;
    }
    composer.editor.grab_focus();
    controller.forward(&composer.editor)
}

/// Add complete messages in arrival order. Draft contents are never inspected or changed by events.
pub fn execute(
    area: &gtk4::GLArea,
    target: Session,
    text: String,
    allowed: Box<dyn Fn() -> bool>,
) -> Result<oneshot::Receiver<Result<(), String>>, String> {
    composer(area)
        .ok_or("Terminal input is unavailable")?
        .enqueue(target, text, Some(allowed))
}

/// Inspect attachment without copying a potentially large user draft into periodic snapshots.
pub fn is_active(area: &gtk4::GLArea) -> bool {
    composer(area).is_some_and(|composer| composer.target.borrow().is_some())
}

/// Use the same local editing path for public human-input RPCs as for keyboard/clipboard input.
pub fn insert(area: &gtk4::GLArea, text: &str) -> bool {
    let Some(composer) = composer(area) else {
        return false;
    };
    if composer.target.borrow().is_none() {
        return false;
    }
    composer.editor.buffer().insert_at_cursor(text);
    true
}

/// Enter is a message boundary; other literal text edits locally, with interrupt/clear controls retained.
pub fn character(area: &gtk4::GLArea, character: char) -> Result<bool, String> {
    let Some(composer) = composer(area) else {
        return Ok(false);
    };
    if composer.target.borrow().is_none() {
        return Ok(false);
    }
    match character {
        '\r' => composer.submit_draft(),
        '\u{15}' => composer.editor.buffer().set_text(""),
        character if !character.is_control() || character == '\n' || character == '\t' => {
            composer
                .editor
                .buffer()
                .insert_at_cursor(&character.to_string());
        }
        _ => return Ok(false),
    }
    Ok(true)
}

/// Credential-free local editor diagnostics; no downstream prompt or screen interpretation.
pub fn context(area: &gtk4::GLArea) -> serde_json::Value {
    let Some(composer) = composer(area) else {
        return serde_json::json!({"active":false});
    };
    let buffer = composer.editor.buffer();
    serde_json::json!({"active":composer.target.borrow().is_some(),
        "draft":buffer.text(&buffer.start_iter(), &buffer.end_iter(), true).as_str(),
        "cursor":buffer.cursor_position(), "queued":composer.queue.borrow().len(),
        "executing":composer.draining.get()})
}

fn composer(area: &gtk4::GLArea) -> Option<Rc<Composer>> {
    // SAFETY: attach is the only writer and always stores Rc<Composer> on GTK.
    unsafe {
        area.data::<Rc<Composer>>("cmux-composer")
            .map(|value| value.as_ref().clone())
    }
}

impl Composer {
    /// Recall only CMUX-submitted human messages, never a downstream application's editable line.
    fn recall(&self, previous: bool) {
        let history = self.history.borrow();
        if history.is_empty() {
            return;
        }
        let index = if previous {
            self.history_index
                .get()
                .unwrap_or(history.len())
                .saturating_sub(1)
        } else {
            self.history_index
                .get()
                .map_or(history.len(), |index| index + 1)
        };
        if self.history_index.get().is_none() {
            let buffer = self.editor.buffer();
            *self.history_draft.borrow_mut() = buffer
                .text(&buffer.start_iter(), &buffer.end_iter(), true)
                .to_string();
        }
        self.history_index
            .set((index < history.len()).then_some(index));
        self.editor.buffer().set_text(
            history
                .get(index)
                .map(String::as_str)
                .unwrap_or(&self.history_draft.borrow()),
        );
        let buffer = self.editor.buffer();
        buffer.place_cursor(&buffer.end_iter());
    }

    /// Clear a draft only after FIFO admission; a failed submission remains visible for explicit retry.
    fn submit_draft(self: &Rc<Self>) {
        let Some(target) = self.target.borrow().clone() else {
            return;
        };
        let buffer = self.editor.buffer();
        let text = buffer
            .text(&buffer.start_iter(), &buffer.end_iter(), true)
            .to_string();
        match self.enqueue(target, text.clone(), None) {
            Ok(result) => {
                if !text.is_empty() {
                    self.history.borrow_mut().push_back(text.clone());
                    if self.history.borrow().len() > MAX_MESSAGES {
                        self.history.borrow_mut().pop_front();
                    }
                }
                self.history_index.set(None);
                buffer.set_text("");
                // Keep the completion receiver alive; recovery is performed in FIFO order by the executor.
                glib::MainContext::default().spawn_local(async move {
                    let _ = result.await;
                });
            }
            Err(error) => self.status.set_text(&format!("Not sent: {error}")),
        }
    }

    fn enqueue(
        self: &Rc<Self>,
        target: Session,
        text: String,
        allowed: Option<Box<dyn Fn() -> bool>>,
    ) -> Result<oneshot::Receiver<Result<(), String>>, String> {
        if text.len() > MAX_TEXT || text.contains('\0') {
            return Err("Input exceeds the text limit or contains NUL".into());
        }
        if self.queue.borrow().len() >= MAX_MESSAGES {
            return Err("Terminal input queue is full".into());
        }
        let runtime = self
            .runtime
            .borrow()
            .clone()
            .ok_or("Terminal process is not tracked")?;
        let (reply, receiver) = oneshot::channel();
        self.queue.borrow_mut().push_back(Message {
            target,
            text,
            reply,
            allowed,
        });
        if !self.draining.replace(true) {
            let weak = Rc::downgrade(self);
            glib::MainContext::default().spawn_local(async move {
                loop {
                    let Some(composer) = weak.upgrade() else {
                        return;
                    };
                    let Some(message) = composer.queue.borrow_mut().pop_front() else {
                        composer.draining.set(false);
                        return;
                    };
                    if message.reply.is_closed() {
                        continue;
                    }
                    let process = message.target.process.clone();
                    let root = message.target.terminal.foreground_pid;
                    let verified = runtime
                        .spawn_blocking(move || {
                            cmux_platform::process::input_identity(root).as_ref() == Some(&process)
                        })
                        .await
                        .unwrap_or(false);
                    let result = if message.reply.is_closed() {
                        continue;
                    } else if message.allowed.as_ref().is_some_and(|allowed| !allowed()) {
                        Err("Submission canceled before execution".into())
                    } else if !verified
                        || composer
                            .target
                            .borrow()
                            .as_ref()
                            .is_none_or(|current| !current.same_attachment(&message.target))
                    {
                        Err("Running process changed".into())
                    } else {
                        composer.write(&message)
                    };
                    if message.allowed.is_none() {
                        if let Err(error) = &result {
                            let buffer = composer.editor.buffer();
                            let mut end = buffer.end_iter();
                            if buffer.char_count() > 0 && !message.text.is_empty() {
                                buffer.insert(&mut end, "\n");
                            }
                            buffer.insert(&mut end, &message.text);
                            composer.container.set_visible(true);
                            composer
                                .status
                                .set_text(&format!("Not sent: {error} · draft retained"));
                        }
                    }
                    let _ = message.reply.send(result);
                }
            });
        }
        Ok(receiver)
    }

    /// One GTK operation writes a whole message and its Enter, with no event-loop yield between them.
    fn write(&self, message: &Message) -> Result<(), String> {
        let surface = self.surface.borrow().ok_or("Terminal closed")?;
        // SAFETY: the weak widget and shared surface cell keep lifetime checks on GTK; no yield occurs here.
        unsafe {
            if self.area.upgrade().is_none()
                || crate::ghostty::tty::root_pid(surface) != message.target.terminal.foreground_pid
            {
                return Err("Terminal process changed".into());
            }
            crate::ghostty::text::send_literal(surface, &message.text).map_err(str::to_owned)?;
            crate::ghostty::text::send_character(surface, '\r').map_err(str::to_owned)?;
        }
        self.status
            .set_text("Enter to send · Shift+Enter for a newline");
        Ok(())
    }
}

/// These controls interrupt or inspect the process without creating an unfinished downstream draft.
fn process_control(key: gtk4::gdk::Key, modifiers: gtk4::gdk::ModifierType) -> bool {
    key == gtk4::gdk::Key::Escape
        || (gtk4::gdk::Key::F1..=gtk4::gdk::Key::F12).contains(&key)
        || modifiers == gtk4::gdk::ModifierType::CONTROL_MASK
            && matches!(key.to_lower(), gtk4::gdk::Key::c | gtk4::gdk::Key::d)
}
