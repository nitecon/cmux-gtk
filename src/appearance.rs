//! Shared desktop typography and palette, independent of native service backends.

/// Install the application palette and request dark native controls before building the UI.
/// Call on GTK's main thread after initialization. The display retains the CSS provider;
/// dialogs and menus share it, while Ghostty retains ownership of terminal font settings.
pub(crate) fn install() {
    let display = gtk4::gdk::Display::default().expect("no display");
    // A portable Windows executable must find its icons even without the launcher script.
    #[cfg(target_os = "windows")]
    if let Ok(executable) = std::env::current_exe() {
        if let Some(directory) = executable.parent() {
            gtk4::IconTheme::for_display(&display).add_search_path(directory.join("share/icons"));
        }
    }
    gtk4::Window::set_default_icon_name(crate::APP_ID);
    let settings = gtk4::Settings::for_display(&display);
    // Match cmux's fixed dark panels with the native controls not fully restyled by our CSS.
    // This property supports the GTK 4.8 baseline; CssProvider's color-scheme API needs 4.20.
    settings.set_gtk_application_prefer_dark_theme(true);

    let provider = gtk4::CssProvider::new();
    #[cfg(not(target_os = "windows"))]
    provider.load_from_data(include_str!("appearance.css"));
    #[cfg(target_os = "windows")]
    provider.load_from_data(&format!(
        "{}\n{}",
        include_str!("appearance.css"),
        include_str!("appearance-windows.css")
    ));
    gtk4::style_context_add_provider_for_display(
        &display,
        &provider,
        gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
    );
}
