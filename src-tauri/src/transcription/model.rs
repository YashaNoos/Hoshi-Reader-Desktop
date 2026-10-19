// Speech model files (parakeet or ReazonSpeech), downloaded on first use and verified
// with SHA-256.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use sha2::{Digest, Sha256};

/// The two models this build can use (Transcription → Model in Match).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Model {
    /// NVIDIA parakeet-tdt_ctc-0.6b-ja (CC-BY-4.0), TDT branch exported to sherpa-onnx and
    /// quantized to int8, from this project's GitHub release. Needs long pieces (context).
    Parakeet,
    /// ReazonSpeech k2 v2 (Apache-2.0): the same pinned revision and checksums as Hoshi
    /// Reader Android. Cuts at every short pause, like the other platforms.
    ReazonSpeech,
}

impl Model {
    /// The id the frontend sends and the transcript stores. Unknown ids use parakeet.
    pub fn from_id(id: &str) -> Model {
        match id {
            "reazonspeech" => Model::ReazonSpeech,
            _ => Model::Parakeet,
        }
    }

    /// Directory name under the cache's Models folder.
    pub fn dir_name(self) -> &'static str {
        match self {
            Model::Parakeet => "parakeet-tdt_ctc-0.6b-ja-int8",
            Model::ReazonSpeech => "reazonspeech-k2-v2-int8",
        }
    }

    /// sherpa-onnx's model_type for the transducer.
    pub fn model_type(self) -> &'static str {
        match self {
            Model::Parakeet => "nemo_transducer",
            Model::ReazonSpeech => "transducer",
        }
    }

    fn base(self) -> &'static str {
        match self {
            Model::Parakeet => "https://github.com/YashaNoos/Hoshi-Reader-for-Linux-unofficial-/releases/download/model-parakeet-ja-1/",
            Model::ReazonSpeech => "https://huggingface.co/reazon-research/reazonspeech-k2-v2/resolve/291488c8151be24d7da4bf7af26e533fad96e407/",
        }
    }

    pub fn files(self) -> &'static [ModelFile; 4] {
        match self {
            Model::Parakeet => &PARAKEET_FILES,
            Model::ReazonSpeech => &REAZONSPEECH_FILES,
        }
    }
}

pub struct ModelFile {
    pub name: &'static str,
    remote: &'static str,
    bytes: u64,
    sha256: &'static str,
}

const PARAKEET_FILES: [ModelFile; 4] = [
    ModelFile { name: "encoder.int8.onnx", remote: "encoder.int8.onnx", bytes: 652_467_728,
        sha256: "2780b59886febf5b00bfb411a7ed9e6ef569b871739fcc58e7c7b42398f38277" },
    ModelFile { name: "decoder.int8.onnx", remote: "decoder.int8.onnx", bytes: 8_568_472,
        sha256: "396758f94454d0f2491a1414747d4670b33c543d50efd1565bd28657b387a308" },
    ModelFile { name: "joiner.int8.onnx", remote: "joiner.int8.onnx", bytes: 3_057_995,
        sha256: "d2e2f86466d3d1bdee245c791131a403f6061cb3e588b60dea23648878b8e0af" },
    ModelFile { name: "tokens.txt", remote: "tokens.txt", bytes: 28_557,
        sha256: "732f64c53909f2620c713f4106b487d92e6f54a6915b3cd3d1dbd32f9f4f392a" },
];

const REAZONSPEECH_FILES: [ModelFile; 4] = [
    ModelFile { name: "encoder.int8.onnx", remote: "encoder-epoch-99-avg-1.int8.onnx", bytes: 154_670_139,
        sha256: "2c7bd08a8a99f9ddd0d9e458456577b1f6279214e51426f114f9eced44c54e1d" },
    ModelFile { name: "decoder.int8.onnx", remote: "decoder-epoch-99-avg-1.int8.onnx", bytes: 2_959_337,
        sha256: "8f0bff94d38797b03b762634ed03211a8e303d06cc4603cdd0cf4199d6eb1485" },
    ModelFile { name: "joiner.int8.onnx", remote: "joiner-epoch-99-avg-1.int8.onnx", bytes: 2_696_970,
        sha256: "49cc7ea1d3d35a40a27442db5e89996da64bf0e683a903dce76e99e57a12e4de" },
    ModelFile { name: "tokens.txt", remote: "tokens.txt", bytes: 45_754,
        sha256: "2c3ac659818a48a0c04010e0593bbc4d7c8a24a054340b01131499c05fd52def" },
];

const VERIFIED_MARKER: &str = ".verified";

pub struct ModelPaths {
    pub encoder: PathBuf,
    pub decoder: PathBuf,
    pub joiner: PathBuf,
    pub tokens: PathBuf,
}

impl ModelPaths {
    fn in_dir(dir: &Path, files: &[ModelFile; 4]) -> Self {
        let find = |file: &ModelFile| {
            let local = dir.join(file.name);
            if local.is_file() { local } else { dir.join(file.remote) }
        };
        ModelPaths { encoder: find(&files[0]), decoder: find(&files[1]), joiner: find(&files[2]), tokens: find(&files[3]) }
    }
}

fn hex(digest: &[u8]) -> String {
    digest.iter().map(|byte| format!("{byte:02x}")).collect()
}

fn sha256(path: &Path) -> std::io::Result<String> {
    use std::io::Read;
    let mut hasher = Sha256::new();
    let mut file = fs::File::open(path)?;
    let mut buffer = vec![0u8; 1 << 20];
    loop {
        let read = file.read(&mut buffer)?;
        if read == 0 {
            break;
        }
        hasher.update(&buffer[..read]);
    }
    Ok(hex(&hasher.finalize()))
}

/// Returns ready model paths, downloading (with `progress` in 0..=1) when needed.
/// `HOSHI_SASAYAKI_MODEL_DIR` points at a directory with the files already present (or
/// with one subdirectory per model, named like the cache's).
pub async fn ensure(
    model: Model,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> Result<Option<ModelPaths>, String> {
    if let Some(custom) = std::env::var_os("HOSHI_SASAYAKI_MODEL_DIR") {
        let custom = Path::new(&custom);
        let dir = Some(custom.join(model.dir_name())).filter(|dir| dir.is_dir());
        return Ok(Some(ModelPaths::in_dir(dir.as_deref().unwrap_or(custom), model.files())));
    }
    fetch(model.base(), model.files(), dir, cancel, progress).await
}

const CONNECT_TIMEOUT: Duration = Duration::from_secs(15);
/// A download that receives nothing for this long has stalled: it ends with an error, so
/// Match can be pressed again. It resets whenever data arrives (no limit on the total).
const READ_TIMEOUT: Duration = Duration::from_secs(if cfg!(test) { 2 } else { 45 });

/// Resolves once Pause sets `cancel`, so a download waiting on the network can stop.
async fn cancelled(cancel: &AtomicBool) {
    while !cancel.load(Ordering::Relaxed) {
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
}

fn download_error(error: reqwest::Error) -> String {
    if error.is_timeout() {
        format!("Model download stalled (no data for {} s). Press Match to try again.", READ_TIMEOUT.as_secs())
    } else {
        format!("Model download failed: {error}")
    }
}

/// Deletes a partly downloaded file on every way out, unless it was completed.
struct Partial<'a> {
    path: &'a Path,
    complete: bool,
}

impl Drop for Partial<'_> {
    fn drop(&mut self) {
        if !self.complete {
            fs::remove_file(self.path).ok();
        }
    }
}

pub(crate) async fn fetch(
    base: &str,
    files: &'static [ModelFile; 4],
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> Result<Option<ModelPaths>, String> {
    let present = |file: &ModelFile| fs::metadata(dir.join(file.name)).is_ok_and(|m| m.len() == file.bytes);
    if dir.join(VERIFIED_MARKER).is_file() && files.iter().all(present) {
        return Ok(Some(ModelPaths::in_dir(dir, files)));
    }
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create model directory: {e}"))?;
    fs::remove_file(dir.join(VERIFIED_MARKER)).ok();

    let total: u64 = files.iter().map(|f| f.bytes).sum();
    let mut done: u64 = files.iter().filter(|f| present(f)).map(|f| f.bytes).sum();
    progress(done as f64 / total as f64);
    let client = reqwest::Client::builder()
        .connect_timeout(CONNECT_TIMEOUT)
        .read_timeout(READ_TIMEOUT)
        .build()
        .map_err(download_error)?;
    for file in files {
        let target = dir.join(file.name);
        if present(file) && sha256(&target).is_ok_and(|h| h == file.sha256) {
            continue;
        }
        if present(file) {
            done -= file.bytes;
        }
        let partial = dir.join(format!("{}.part", file.name));
        let request = client.get(format!("{base}{}", file.remote)).send();
        let mut response = tokio::select! {
            response = request => response.and_then(|r| r.error_for_status()).map_err(download_error)?,
            _ = cancelled(cancel) => return Ok(None),
        };
        let mut out = fs::File::create(&partial).map_err(|e| format!("Cannot write model: {e}"))?;
        let mut cleanup = Partial { path: &partial, complete: false };
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        loop {
            let chunk = tokio::select! {
                chunk = response.chunk() => chunk.map_err(download_error)?,
                _ = cancelled(cancel) => return Ok(None),
            };
            let Some(chunk) = chunk else { break };
            received += chunk.len() as u64;
            if received > file.bytes {
                return Err("Model download is larger than expected".into());
            }
            hasher.update(&chunk);
            out.write_all(&chunk).map_err(|e| format!("Cannot write model: {e}"))?;
            progress((done + received) as f64 / total as f64);
        }
        out.flush().ok();
        drop(out);
        if received != file.bytes || hex(&hasher.finalize()) != file.sha256 {
            return Err("Model integrity verification failed".into());
        }
        fs::rename(&partial, &target).map_err(|e| format!("Cannot save model: {e}"))?;
        cleanup.complete = true;
        done += file.bytes;
    }
    fs::write(dir.join(VERIFIED_MARKER), b"").ok();
    progress(1.0);
    Ok(Some(ModelPaths::in_dir(dir, files)))
}

#[cfg(test)]
mod tests {
    use std::io::{Read, Write};
    use std::net::TcpListener;
    use std::sync::Arc;
    use std::time::{Duration, Instant};

    use super::*;

    /// A server that accepts one download, optionally starts the reply, then goes silent
    /// without closing the connection (like a frozen Wi-Fi or hotel network).
    fn stalled_server(start_reply: bool) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").unwrap();
        let address = listener.local_addr().unwrap();
        std::thread::spawn(move || {
            let (mut socket, _) = listener.accept().unwrap();
            let mut request = [0u8; 1024];
            socket.read(&mut request).ok();
            if start_reply {
                socket.write_all(b"HTTP/1.1 200 OK\r\nContent-Length: 1000000\r\n\r\n0123456789").ok();
            }
            std::thread::sleep(Duration::from_secs(30));
        });
        format!("http://{address}/")
    }

    fn download(base: &str, dir: &Path, cancel: &AtomicBool) -> (Result<Option<ModelPaths>, String>, Duration) {
        let started = Instant::now();
        let result = tauri::async_runtime::block_on(fetch(base, Model::Parakeet.files(), dir, cancel, &mut |_| {}));
        (result, started.elapsed())
    }

    fn leftovers(dir: &Path) -> Vec<String> {
        fs::read_dir(dir)
            .map(|entries| entries.flatten().map(|e| e.file_name().to_string_lossy().into_owned()).collect())
            .unwrap_or_default()
    }

    #[test]
    fn pause_stops_a_stalled_download() {
        for start_reply in [false, true] {
            let dir = std::env::temp_dir().join(format!("hoshi-model-pause-{start_reply}-{}", std::process::id()));
            let cancel = Arc::new(AtomicBool::new(false));
            let flag = cancel.clone();
            std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(300));
                flag.store(true, Ordering::Relaxed);
            });
            let (result, elapsed) = download(&stalled_server(start_reply), &dir, &cancel);
            assert!(matches!(result, Ok(None)), "paused download returns Ok(None), got {:?}", result.err());
            assert!(elapsed < Duration::from_millis(1500), "pause took {elapsed:?}");
            assert!(!leftovers(&dir).iter().any(|name| name.ends_with(".part")), "{:?}", leftovers(&dir));
            fs::remove_dir_all(&dir).ok();
        }
    }

    #[test]
    fn a_stalled_download_ends_with_an_error() {
        for start_reply in [false, true] {
            let dir = std::env::temp_dir().join(format!("hoshi-model-stall-{start_reply}-{}", std::process::id()));
            let cancel = AtomicBool::new(false);
            let (result, elapsed) = download(&stalled_server(start_reply), &dir, &cancel);
            let error = result.err().expect("a stalled download fails");
            assert!(error.contains("stalled"), "{error}");
            assert!(elapsed < READ_TIMEOUT + Duration::from_secs(2), "timeout took {elapsed:?}");
            assert!(!leftovers(&dir).iter().any(|name| name.ends_with(".part")), "{:?}", leftovers(&dir));
            fs::remove_dir_all(&dir).ok();
        }
    }
}
