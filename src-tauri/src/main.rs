#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

fn main() {
    // Fullscreen reader on KDE Plasma (AMD RX 7900 XTX, 144 Hz monitor next to a 75 Hz one):
    // with WebKitGTK's DMA-BUF renderer, KWin shows the page's GPU buffers directly on the
    // monitor ("direct scanout"). Switching between that and normal compositing, e.g. when
    // the pointer moves onto that monitor, made it lose and re-acquire the signal: black
    // flashes, tearing, the monitor's "display on" box. Other apps were not affected.
    // Frames are shared through shared memory instead, which a reader does not notice.
    // WEBKIT_DISABLE_DMABUF_RENDERER=0 restores WebKit's default.
    #[cfg(target_os = "linux")]
    match std::env::var_os("WEBKIT_DISABLE_DMABUF_RENDERER") {
        // SAFETY (both): called first thing in main; no other thread exists yet.
        None => unsafe { std::env::set_var("WEBKIT_DISABLE_DMABUF_RENDERER", "1") },
        Some(value) if value == "0" => unsafe { std::env::remove_var("WEBKIT_DISABLE_DMABUF_RENDERER") },
        Some(_) => {}
    }
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
