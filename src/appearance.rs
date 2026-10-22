//! Shared desktop typography and palette, independent of native service backends.

thread_local! {
    /// GTK owns the display registration; this main-thread handle reloads it on Apply.
    static PROVIDER: gtk4::CssProvider = gtk4::CssProvider::new();
}

/// Reload application typography on GTK's main thread, including existing dialogs and readers.
/// Point units follow GTK's display DPI, matching the units of the terminal preference.
pub(crate) fn apply_font_size(size: f32) {
    load(Some(size));
}

/// Keep the platform baseline until a user selects a shared application font size.
fn load(size: Option<f32>) {
    let mut css = include_str!("appearance.css").to_owned();
    #[cfg(target_os = "windows")]
    css.push_str(include_str!("appearance-windows.css"));
    if let Some(size) = size {
        css.push_str(&format!("\nwindow, popover {{ font-size: {size}pt; }}"));
    }
    PROVIDER.with(|provider| provider.load_from_data(&css));
}

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

    load(crate::preferences::saved_font_size());
    PROVIDER.with(|provider| {
        gtk4::style_context_add_provider_for_display(
            &display,
            provider,
            gtk4::STYLE_PROVIDER_PRIORITY_APPLICATION,
        );
    });
}
