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

/// The desktop's color scheme, for the "System" theme setting.
///
/// Needed on every Linux build, not only the AppImage: for "System" the app calls
/// `Window::set_theme(None)`, which tao turns into `gtk-application-prefer-dark-theme =
/// false`, so WebKit reports light even on a dark desktop (KDE's Breeze is one theme with
/// a dark variant selected by that setting). The frontend passes this value instead.
///
/// Read from the XDG desktop portal (`org.freedesktop.appearance color-scheme`, what KDE
/// and GNOME set), falling back to what the AppImage hook saw at launch. `None` when the
/// desktop states no preference; "System" then keeps the old behaviour.
/// Async: busctl may take a moment and sync commands run on the GTK main thread.
#[tauri::command(async)]
pub fn desktop_color_scheme() -> Option<&'static str> {
    let dark = portal_color_scheme().or_else(|| DESKTOP_DARK.get().copied())?;
    Some(if dark { "dark" } else { "light" })
}

/// `Some(true)` = prefer dark, `Some(false)` = prefer light, `None` = no preference or no portal.
fn portal_color_scheme() -> Option<bool> {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let mut child = Command::new("busctl")
        .args([
            "--user",
            "call",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.Settings",
            "Read",
            "ss",
            "org.freedesktop.appearance",
            "color-scheme",
        ])
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .ok()?;
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(_)) => break,
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return None;
            }
        }
    }
    let output = child.wait_with_output().ok()?;
    if !output.status.success() {
        return None;
    }
    // "v v u 1" (Read wraps the value in a second variant) or "v u 1"
    let text = String::from_utf8_lossy(&output.stdout);
    match text.split_whitespace().last()? {
        "1" => Some(true),
        "2" => Some(false),
        _ => None,
    }
}
