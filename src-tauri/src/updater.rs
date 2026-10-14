// The unofficial Linux build is not published through upstream's update feed (and
// would replace a package-managed binary): updates come from the package manager or
// the Linux releases page, so the updater commands are inert on Linux.
#[cfg(not(target_os = "linux"))]
use serde_json::json;
#[cfg(not(target_os = "linux"))]
use std::time::Duration;
use tauri::AppHandle;
#[cfg(not(target_os = "linux"))]
use tauri::Emitter;
#[cfg(not(target_os = "linux"))]
use tauri_plugin_updater::{Update, UpdaterExt};

#[cfg(not(target_os = "linux"))]
async fn available(app: &AppHandle) -> Result<Option<Update>, String> {
    app.updater()
        .map_err(|error| error.to_string())?
        .check()
        .await
        .map_err(|error| error.to_string())
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn check_update(_app: AppHandle) -> Result<Option<String>, String> {
    Ok(None)
}

#[cfg(target_os = "linux")]
#[tauri::command]
pub async fn install_update(_app: AppHandle) -> Result<(), String> {
    Err("Updates are installed through your package manager".into())
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub async fn check_update(app: AppHandle) -> Result<Option<String>, String> {
    Ok(available(&app).await?.map(|update| update.version))
}

#[cfg(not(target_os = "linux"))]
#[tauri::command]
pub async fn install_update(app: AppHandle) -> Result<(), String> {
    let Some(update) = available(&app).await? else {
        return Ok(());
    };
    let mut downloaded = 0;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk;
                app.emit(
                    "update://progress",
                    json!({ "downloaded": downloaded, "total": total }),
                )
                .ok();
            },
            || {},
        )
        .await
        .map_err(|error| error.to_string())?;
    tokio::time::timeout(
        Duration::from_secs(10),
        crate::sync::gdrive::manager::pause(),
    )
    .await
    .ok();
    update.install(bytes).map_err(|error| error.to_string())?;
    #[cfg(target_os = "macos")]
    if let Some(bundle) = std::env::current_exe()
        .ok()
        .and_then(|exe| exe.ancestors().nth(3).map(|path| path.to_path_buf()))
    {
        std::process::Command::new("open")
            .arg("-n")
            .arg(bundle)
            .spawn()
            .map_err(|error| error.to_string())?;
        app.exit(0);
        return Ok(());
    }
    app.request_restart();
    Ok(())
}
