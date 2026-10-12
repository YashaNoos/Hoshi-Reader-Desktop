// ReazonSpeech k2 v2 (Apache-2.0) model files: the same pinned revision and checksums
// as Hoshi Reader Android, downloaded on first use and verified with SHA-256.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

use sha2::{Digest, Sha256};

const BASE: &str = "https://huggingface.co/reazon-research/reazonspeech-k2-v2/resolve/291488c8151be24d7da4bf7af26e533fad96e407/";

pub struct ModelFile {
    pub name: &'static str,
    remote: &'static str,
    bytes: u64,
    sha256: &'static str,
}

pub const FILES: [ModelFile; 4] = [
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
    fn in_dir(dir: &Path) -> Self {
        let find = |file: &ModelFile| {
            let local = dir.join(file.name);
            if local.is_file() { local } else { dir.join(file.remote) }
        };
        ModelPaths { encoder: find(&FILES[0]), decoder: find(&FILES[1]), joiner: find(&FILES[2]), tokens: find(&FILES[3]) }
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
/// `HOSHI_SASAYAKI_MODEL_DIR` points at a directory with the files already present.
pub async fn ensure(dir: &Path, cancel: &AtomicBool, progress: &mut dyn FnMut(f64)) -> Result<Option<ModelPaths>, String> {
    if let Some(custom) = std::env::var_os("HOSHI_SASAYAKI_MODEL_DIR") {
        return Ok(Some(ModelPaths::in_dir(Path::new(&custom))));
    }
    fetch(BASE, dir, cancel, progress).await
}

pub(crate) async fn fetch(
    base: &str,
    dir: &Path,
    cancel: &AtomicBool,
    progress: &mut dyn FnMut(f64),
) -> Result<Option<ModelPaths>, String> {
    let present = |file: &ModelFile| fs::metadata(dir.join(file.name)).is_ok_and(|m| m.len() == file.bytes);
    if dir.join(VERIFIED_MARKER).is_file() && FILES.iter().all(present) {
        return Ok(Some(ModelPaths::in_dir(dir)));
    }
    fs::create_dir_all(dir).map_err(|e| format!("Cannot create model directory: {e}"))?;
    fs::remove_file(dir.join(VERIFIED_MARKER)).ok();

    let total: u64 = FILES.iter().map(|f| f.bytes).sum();
    let mut done: u64 = FILES.iter().filter(|f| present(f)).map(|f| f.bytes).sum();
    progress(done as f64 / total as f64);
    let client = reqwest::Client::new();
    for file in &FILES {
        let target = dir.join(file.name);
        if present(file) && sha256(&target).is_ok_and(|h| h == file.sha256) {
            continue;
        }
        if present(file) {
            done -= file.bytes;
        }
        let partial = dir.join(format!("{}.part", file.name));
        let mut response = client
            .get(format!("{base}{}", file.remote))
            .send()
            .await
            .and_then(|r| r.error_for_status())
            .map_err(|e| format!("Model download failed: {e}"))?;
        let mut out = fs::File::create(&partial).map_err(|e| format!("Cannot write model: {e}"))?;
        let mut hasher = Sha256::new();
        let mut received = 0u64;
        while let Some(chunk) = response.chunk().await.map_err(|e| format!("Model download failed: {e}"))? {
            if cancel.load(Ordering::Relaxed) {
                drop(out);
                fs::remove_file(&partial).ok();
                return Ok(None);
            }
            received += chunk.len() as u64;
            if received > file.bytes {
                fs::remove_file(&partial).ok();
                return Err("Model download is larger than expected".into());
            }
            hasher.update(&chunk);
            out.write_all(&chunk).map_err(|e| format!("Cannot write model: {e}"))?;
            progress((done + received) as f64 / total as f64);
        }
        out.flush().ok();
        drop(out);
        if received != file.bytes || hex(&hasher.finalize()) != file.sha256 {
            fs::remove_file(&partial).ok();
            return Err("Model integrity verification failed".into());
        }
        fs::rename(&partial, &target).map_err(|e| format!("Cannot save model: {e}"))?;
        done += file.bytes;
    }
    fs::write(dir.join(VERIFIED_MARKER), b"").ok();
    progress(1.0);
    Ok(Some(ModelPaths::in_dir(dir)))
}
