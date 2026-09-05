//! Divider recovery scoped to mapped widgets owned by the affected pane subtree.

use super::{ffi, surface_for_area};
use gtk4::prelude::*;
use std::cell::Cell;
use std::rc::Rc;

/// Attach one drag-end observer; use a coalesced position notification if GTK exposes no gesture.
pub(super) fn install(paned: &gtk4::Paned) {
    let gesture = find_drag(paned.upcast_ref()).or_else(|| {
        let mut child = paned.first_child();
        while let Some(widget) = child {
            if let Some(gesture) = find_drag(&widget) {
                return Some(gesture);
            }
            child = widget.next_sibling();
        }
        None
    });
    if let Some(gesture) = gesture {
        let weak = paned.downgrade();
        gesture.connect_drag_end(move |_, _, _| {
            let weak = weak.clone();
            gtk4::glib::idle_add_local_once(move || {
                if let Some(paned) = weak.upgrade() {
                    recover(&paned);
                }
            });
        });
    } else {
        let pending = Rc::new(Cell::new(false));
        paned.connect_notify_local(Some("position"), move |paned, _| {
            if pending.replace(true) {
                return;
            }
            let pending = pending.clone();
            let weak = paned.downgrade();
            gtk4::glib::idle_add_local_once(move || {
                pending.set(false);
                if let Some(paned) = weak.upgrade() {
                    recover(&paned);
                }
            });
        });
    }
}

/// Find the first GTK drag controller attached directly to a widget.
fn find_drag(widget: &gtk4::Widget) -> Option<gtk4::GestureDrag> {
    let controllers = widget.observe_controllers();
    (0..controllers.n_items()).find_map(|index| {
        controllers
            .item(index)?
            .downcast::<gtk4::GestureDrag>()
            .ok()
    })
}

/// Collect only mapped terminal widgets; hidden notebook pages and workspaces are excluded.
fn mapped_terminals(widget: &gtk4::Widget, terminals: &mut Vec<gtk4::GLArea>) {
    if !widget.is_mapped() {
        return;
    }
    if let Some(area) = widget.downcast_ref::<gtk4::GLArea>() {
        terminals.push(area.clone());
        return;
    }
    let mut child = widget.first_child();
    while let Some(widget) = child {
        mapped_terminals(&widget, terminals);
        child = widget.next_sibling();
    }
}

/// Resynchronize mapped native sizes and restore selected terminal focus after a divider gesture.
/// Native focus is not explicitly bounced: GTK focus events own that transition.
fn recover(paned: &gtk4::Paned) {
    if !paned.is_mapped() {
        return;
    }
    let mut terminals = Vec::new();
    mapped_terminals(paned.upcast_ref(), &mut terminals);
    for area in &terminals {
        if let Some(surface) = surface_for_area(area) {
            let scale = area.scale_factor();
            let (width, height) = (area.width() * scale, area.height() * scale);
            if width > 0 && height > 0 {
                // SAFETY: this GTK-thread widget owns a live native surface. Registry
                // lookup releases its lock before these potentially reentrant calls.
                unsafe {
                    ffi::ghostty_surface_set_size(surface, width as u32, height as u32);
                    ffi::ghostty_surface_refresh(surface);
                }
            }
        }
        area.queue_render();
        area.queue_draw();
    }
    if let Some(area) = terminals
        .iter()
        .find(|area| area.has_css_class("active-pane"))
    {
        area.grab_focus();
    }
    tick();
    // Native resize crosses IO/render threads. Retry painting only while the owner
    // is still mapped, without retaining a closed subtree or changing focus later.
    for delay in [50, 150, 300] {
        let weak = paned.downgrade();
        gtk4::glib::timeout_add_local_once(std::time::Duration::from_millis(delay), move || {
            let Some(paned) = weak.upgrade().filter(|paned| paned.is_mapped()) else {
                return;
            };
            tick();
            let mut terminals = Vec::new();
            mapped_terminals(paned.upcast_ref(), &mut terminals);
            for area in terminals {
                area.queue_render();
            }
        });
    }
}

/// Process native mailbox work on GTK after checking that the application handle exists.
fn tick() {
    let app = crate::ghostty::callbacks::APP_PTR.load(std::sync::atomic::Ordering::SeqCst);
    if app != 0 {
        // SAFETY: callbacks run on the owning GTK thread during application lifetime.
        unsafe { ffi::ghostty_app_tick(app as ffi::ghostty_app_t) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Drive real GTK mapping with an elapsed-time deadline; never wait forever for a widget.
    fn wait_until(mut ready: impl FnMut() -> bool) {
        let deadline = std::time::Instant::now() + std::time::Duration::from_secs(3);
        while !ready() {
            assert!(
                std::time::Instant::now() < deadline,
                "GTK condition did not converge"
            );
            for _ in 0..8 {
                gtk4::glib::MainContext::default().iteration(false);
            }
            std::thread::sleep(std::time::Duration::from_millis(5));
        }
    }

    /// Hidden tabs are excluded and delayed divider recovery does not retain a detached owner.
    #[test]
    #[ignore = "requires GTK display; run in GitHub Actions under Xvfb"]
    fn divider_recovery_mapping_and_lifetime() {
        gtk4::init().expect("GTK display");
        let window = gtk4::Window::new();
        window.set_default_size(640, 400);
        let paned = gtk4::Paned::new(gtk4::Orientation::Horizontal);
        let notebook = gtk4::Notebook::new();
        let first = gtk4::GLArea::new();
        let second = gtk4::GLArea::new();
        first.set_focusable(true);
        second.set_focusable(true);
        notebook.append_page(&first, Some(&gtk4::Label::new(Some("first"))));
        notebook.append_page(&second, Some(&gtk4::Label::new(Some("second"))));
        notebook.set_current_page(Some(0));
        paned.set_start_child(Some(&notebook));
        paned.set_end_child(Some(&gtk4::Label::new(Some("other pane"))));
        install(&paned);
        window.set_child(Some(&paned));
        window.present();
        wait_until(|| first.is_mapped());
        let mut mapped = Vec::new();
        mapped_terminals(paned.upcast_ref(), &mut mapped);
        assert_eq!(mapped, vec![first.clone()]);
        notebook.set_current_page(Some(1));
        wait_until(|| second.is_mapped() && !first.is_mapped());
        mapped.clear();
        mapped_terminals(paned.upcast_ref(), &mut mapped);
        assert_eq!(mapped, vec![second.clone()]);
        second.add_css_class("active-pane");
        recover(&paned);
        assert_eq!(
            gtk4::prelude::GtkWindowExt::focus(&window),
            Some(second.upcast())
        );
        let weak = paned.downgrade();
        window.set_child(None::<&gtk4::Widget>);
        drop(paned);
        assert!(
            weak.upgrade().is_none(),
            "recovery retained detached divider"
        );
        window.close();
    }
}
