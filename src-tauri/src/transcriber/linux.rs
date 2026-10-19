//! Linux transcription backend: sherpa-onnx with the ReazonSpeech model (see
//! `crate::transcription`). The model (~160 MB) is downloaded on first use into the app's
//! cache directory and checked with SHA-256; after that everything runs offline.
//!
//! Built without the `transcription` feature, transcription reports "unsupported".

use tauri::AppHandle;
use tokio_util::sync::CancellationToken;

use super::Events;
use crate::sasayaki::SasayakiToken;

pub struct Audio {
    #[cfg_attr(not(feature = "transcription"), allow(dead_code))]
    path: std::path::PathBuf,
    pub duration: f64,
    /// Already saved tokens near the resume point, to drop repeated words.
    #[cfg_attr(not(feature = "transcription"), allow(dead_code))]
    previous: Vec<SasayakiToken>,
}

impl Audio {
    pub fn resuming_after(mut self, tokens: &[SasayakiToken], through: f64) -> Self {
        let tail = tokens.partition_point(|token| token.end < through - 1.0);
        self.previous = tokens[tail..].to_vec();
        self
    }
}

#[cfg(feature = "transcription")]
pub fn prepare(path: &str) -> Result<Audio, String> {
    let path = std::path::PathBuf::from(path);
    let duration = crate::transcription::audio::duration(&path)
        .ok_or_else(|| format!("Could not read {}", path.display()))?;
    Ok(Audio {
        path,
        duration,
        previous: Vec::new(),
    })
}

#[cfg(feature = "transcription")]
pub fn start(
    app: &AppHandle,
    audio: Audio,
    from: f64,
    _performance: i32,
    events: Events,
    cancel: CancellationToken,
) -> Result<(), String> {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};
    use tauri::Manager;

    use super::Event;

    let model_dir = app
        .path()
        .app_cache_dir()
        .unwrap_or_else(|_| std::env::temp_dir())
        .join("Models")
        .join("reazonspeech-k2-v2-int8");
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    tauri::async_runtime::spawn(async move {
        cancel.cancelled().await;
        flag.store(true, Ordering::Relaxed);
    });
    std::thread::spawn(move || {
        let result = crate::transcription::transcribe(
            &model_dir,
            &audio.path,
            from,
            &audio.previous,
            &stop,
            &mut |fraction| {
                events.send(Event::Download(fraction)).ok();
            },
            &mut |tokens, through| {
                events.send(Event::Tokens(tokens, through)).ok();
            },
        );
        events.send(Event::Finish(result.err())).ok();
    });
    Ok(())
}

#[cfg(feature = "transcription")]
pub async fn status(_app: AppHandle) -> &'static str {
    "available"
}

#[cfg(not(feature = "transcription"))]
pub fn prepare(_path: &str) -> Result<Audio, String> {
    Err("This build has no transcription support".into())
}

#[cfg(not(feature = "transcription"))]
pub fn start(
    _app: &AppHandle,
    _audio: Audio,
    _from: f64,
    _performance: i32,
    _events: Events,
    _cancel: CancellationToken,
) -> Result<(), String> {
    Err("This build has no transcription support".into())
}

#[cfg(not(feature = "transcription"))]
pub async fn status(_app: AppHandle) -> &'static str {
    "unsupported"
}
