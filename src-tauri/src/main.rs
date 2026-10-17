#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Route GTK's native file chooser through xdg-desktop-portal so the desktop's own
    // dialog is used (KDE's on Plasma). Only when a FileChooser portal answers: with
    // GTK_USE_PORTAL=1 and no portal, GTK shows no dialog at all.
    #[cfg(target_os = "linux")]
    if std::env::var_os("GTK_USE_PORTAL").is_none() && file_chooser_portal_available() {
        // SAFETY: called first thing in main; no other thread exists yet.
        unsafe { std::env::set_var("GTK_USE_PORTAL", "1") };
    }
    hoshi_reader_lib::run();
}

#[cfg(target_os = "linux")]
fn file_chooser_portal_available() -> bool {
    use std::process::{Command, Stdio};
    use std::time::{Duration, Instant};

    let Ok(mut child) = Command::new("busctl")
        .args([
            "--user",
            "get-property",
            "org.freedesktop.portal.Desktop",
            "/org/freedesktop/portal/desktop",
            "org.freedesktop.portal.FileChooser",
            "version",
        ])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .spawn()
    else {
        return false;
    };
    let deadline = Instant::now() + Duration::from_secs(2);
    loop {
        match child.try_wait() {
            Ok(Some(status)) => return status.success(),
            Ok(None) if Instant::now() < deadline => std::thread::sleep(Duration::from_millis(20)),
            _ => {
                let _ = child.kill();
                let _ = child.wait();
                return false;
            }
        }
    }
}
