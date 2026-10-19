// Speech model files (parakeet or ReazonSpeech), downloaded on first use and verified
// with SHA-256.

use std::fs;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, Ordering};

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
    let client = reqwest::Client::new();
    for file in files {
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
    Ok(Some(ModelPaths::in_dir(dir, files)))
}
