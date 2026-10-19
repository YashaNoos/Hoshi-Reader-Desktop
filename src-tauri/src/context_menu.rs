// Linux: a context menu shown without a position is anchored by muda to the X11
// root window. Wayland has no root window, so GTK cannot parent the popup
// ("Couldn't map as window ... as popup because it doesn't have a parent") and
// places it at the wrong spot. Read the pointer position relative to our own
// window instead and anchor the menu there; this also works on X11.

use std::sync::mpsc;

use gtk::prelude::*;
use tauri::menu::{ContextMenu, Menu};
use tauri::{LogicalPosition, Manager, ResourceId, Runtime, Webview, Window};

fn pointer_position<R: Runtime>(window: &Window<R>) -> Option<(i32, i32)> {
    let gdk_window = window.gtk_window().ok()?.window()?;
    let pointer = gdk_window.display().default_seat()?.pointer()?;
    let (_, x, y, _) = gdk_window.device_position(&pointer);
    Some((x, y))
}

#[tauri::command]
pub async fn popup_menu_at_pointer<R: Runtime>(
    webview: Webview<R>,
    window: Window<R>,
    rid: ResourceId,
) -> Result<(), String> {
    let menu = webview
        .resources_table()
        .get::<Menu<R>>(rid)
        .map_err(|error| error.to_string())?;
    let (sender, receiver) = mpsc::channel();
    let target = window.clone();
    window
        .run_on_main_thread(move || {
            sender.send(pointer_position(&target)).ok();
        })
        .map_err(|error| error.to_string())?;
    let position = receiver.recv().ok().flatten();
    match position {
        Some((x, y)) => menu.popup_at(window, LogicalPosition::new(x as f64, y as f64)),
        None => menu.popup(window),
    }
    .map_err(|error| error.to_string())
}
