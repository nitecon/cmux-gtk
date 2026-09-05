//! GTK header controls backed by the same window actions as menus and shortcuts.

use gtk4::prelude::*;

/// Build workspace and pane controls on the GTK thread, unless the header is hidden.
pub fn build_header_bar(config: &crate::config::Config) -> Option<gtk4::HeaderBar> {
    if config.ui.header_bar.style == "none" {
        return None;
    }
    let header = gtk4::HeaderBar::new();
    header.add_css_class("cmux-headerbar");
    for (icon, tooltip, action) in [
        (
            "tab-new-symbolic",
            "New Workspace (Ctrl+N)",
            "win.new-workspace",
        ),
        (
            "web-browser-symbolic",
            "New Tab (Browser) (Ctrl+Shift+L)",
            "win.new-browser-tab",
        ),
    ] {
        header.pack_start(&action_button(icon, tooltip, action));
    }
    let menu = gtk4::MenuButton::new();
    menu.set_icon_name("open-menu-symbolic");
    menu.set_tooltip_text(Some("Menu"));
    menu.set_menu_model(Some(&crate::menus::build_hamburger_menu()));
    menu.add_css_class("headerbar-btn");
    // GTK packs end children from right to left, starting with the menu.
    header.pack_end(&menu);
    for (icon, tooltip, action) in [
        (
            "sidebar-show-symbolic",
            "Toggle Sidebar (Ctrl+B)",
            "win.toggle-sidebar",
        ),
        (
            "object-flip-vertical-symbolic",
            "Split Down (Ctrl+Shift+D)",
            "win.split-down",
        ),
        (
            "view-dual-symbolic",
            "Split Right (Ctrl+D)",
            "win.split-right",
        ),
    ] {
        header.pack_end(&action_button(icon, tooltip, action));
    }
    Some(header)
}

/// Create a consistently styled header button bound to an existing GIO action.
fn action_button(icon: &str, tooltip: &str, action: &str) -> gtk4::Button {
    let button = gtk4::Button::from_icon_name(icon);
    button.set_tooltip_text(Some(tooltip));
    button.set_action_name(Some(action));
    button.add_css_class("headerbar-btn");
    button
}
