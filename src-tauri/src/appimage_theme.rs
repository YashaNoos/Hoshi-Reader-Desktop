//! AppImage only. linuxdeploy's GTK hook (apprun-hooks/linuxdeploy-plugin-gtk.sh) exports
//! `GTK_THEME=Adwaita:<variant>` with the variant taken from the desktop's color scheme.
//! A variant in GTK_THEME overrides `gtk-application-prefer-dark-theme`, which is what
//! `Window::set_theme` changes, so the app's own Light/Dark setting never reaches WebKit:
//! on a dark desktop, a book set to Light is drawn white on black (and the reverse).
//!
//! Keep the theme name, drop the variant, and remember the desktop's variant so the
//! "System" setting can still follow the desktop (see `desktop_color_scheme`).

use std::sync::OnceLock;

static DESKTOP_DARK: OnceLock<bool> = OnceLock::new();

/// Must run before GTK is initialised and before any other thread exists.
pub fn init() {
    if std::env::var_os("APPIMAGE").is_none() {
        return;
    }
    let Ok(theme) = std::env::var("GTK_THEME") else {
        return;
    };
    let Some((name, variant)) = theme.split_once(':') else {
        return;
    };
    let _ = DESKTOP_DARK.set(variant == "dark");
    // SAFETY: called at the start of run(), before GTK starts and before any thread is spawned.
    unsafe { std::env::set_var("GTK_THEME", name) };
}

/// The desktop's color scheme as the AppImage hook saw it at launch, or `None` outside an
/// AppImage (there GTK follows the desktop theme by itself).
#[tauri::command]
pub fn desktop_color_scheme() -> Option<&'static str> {
    DESKTOP_DARK.get().map(|dark| if *dark { "dark" } else { "light" })
}
