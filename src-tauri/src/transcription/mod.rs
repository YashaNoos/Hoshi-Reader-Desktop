// On-device Sasayaki transcription for Linux: sherpa-onnx + parakeet-tdt_ctc-0.6b-ja,
// following the pipeline of Hoshi Reader Android. Used by transcriber/linux.rs; the
// transcript is aligned to the book by the shared aligner (aligner.rs).

pub mod audio;
pub mod model;
mod pipeline;

use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, mpsc};

use sherpa_onnx::{OfflineModelConfig, OfflineRecognizer, OfflineRecognizerConfig, OfflineTransducerModelConfig};

pub use crate::sasayaki::SasayakiToken as Token;
use pipeline::{Batch, OrderedScheduler, Pipeline, Recognition, Stitcher, project_tokens};

/// Concurrent recognitions and ONNX threads per recognition, from the CPU count.
fn parallelism() -> (usize, i32) {
    let cores = std::thread::available_parallelism().map_or(2, |n| n.get());
    let workers = (cores / 2).clamp(1, 4);
    let threads = (cores / workers).clamp(1, 4) as i32;
    (workers, threads)
}

fn recognizer(paths: &model::ModelPaths, threads: i32) -> Result<OfflineRecognizer, String> {
    let path = |p: &Path| Some(p.to_string_lossy().into_owned());
    let mut config = OfflineRecognizerConfig::default();
    config.model_config = OfflineModelConfig {
        transducer: OfflineTransducerModelConfig {
            encoder: path(&paths.encoder),
            decoder: path(&paths.decoder),
            joiner: path(&paths.joiner),
        },
        tokens: path(&paths.tokens),
        num_threads: threads,
        provider: Some("cpu".into()),
        model_type: Some("nemo_transducer".into()),
        ..Default::default()
    };
    config.decoding_method = Some("greedy_search".into());
    OfflineRecognizer::create(&config).ok_or_else(|| "Could not load the speech recognition model".to_string())
}

fn recognize(recognizer: &OfflineRecognizer, job: &Recognition) -> Result<Batch, String> {
    let stream = recognizer.create_stream();
    stream.accept_waveform(pipeline::SAMPLE_RATE as i32, &job.samples);
    recognizer.decode(&stream);
    let result = stream.get_result().ok_or("Speech recognition failed")?;
    let timestamps = result.timestamps.unwrap_or_default();
    if timestamps.len() != result.tokens.len() {
        return Err("Speech recognition returned no token timestamps".into());
    }
    Ok(Batch {
        tokens: project_tokens(&result.tokens, &timestamps, job.segment_start, job.segment_end),
        through: job.through,
        segment_start: job.segment_start,
    })
}

/// Transcribes `path` from `from` seconds. Blocking; returns Ok(()) when cancelled.
/// `previous` is the already saved transcript tail, used to drop repeated words on resume.
pub fn transcribe(
    model_dir: &Path,
    path: &Path,
    from: f64,
    previous: &[Token],
    cancel: &AtomicBool,
    on_download: &mut dyn FnMut(f64),
    on_tokens: &mut dyn FnMut(Vec<Token>, f64),
) -> Result<(), String> {
    let duration = audio::duration(path).ok_or_else(|| format!("Could not read {}", path.display()))?;
    if from >= duration {
        on_tokens(Vec::new(), duration);
        return Ok(());
    }
    let Some(paths) = tauri::async_runtime::block_on(model::ensure(model_dir, cancel, on_download))? else {
        return Ok(());
    };
    let (workers, threads) = parallelism();
    let recognizer = Arc::new(recognizer(&paths, threads)?);

    type Work = (Recognition, mpsc::SyncSender<Result<Batch, String>>);
    let (jobs, queue) = mpsc::channel::<Work>();
    let queue = Arc::new(Mutex::new(queue));
    let handles: Vec<_> = (0..workers)
        .map(|_| {
            let (queue, recognizer) = (queue.clone(), recognizer.clone());
            std::thread::spawn(move || loop {
                let next = queue.lock().unwrap().recv();
                let Ok((job, reply)) = next else { break };
                reply.send(recognize(&recognizer, &job)).ok();
            })
        })
        .collect();

    // Half a second of leading context; the stitcher drops what was already saved.
    let audio_from = (from - 0.5).max(0.0);
    let mut pipeline = Pipeline::new(from, duration, audio_from);
    let mut scheduler = OrderedScheduler::new(jobs, workers + 1, Stitcher::new(from, previous), from, on_tokens);
    let stop = AtomicBool::new(false);
    let decoded = audio::decode(path, audio_from, &stop, |chunk| {
        pipeline.accept(chunk, &mut |job| scheduler.schedule(job));
        if cancel.load(Ordering::Relaxed) || scheduler.error().is_some() {
            stop.store(true, Ordering::Relaxed);
        }
    });
    // The scheduler owns the job queue's sender: it must be finished or dropped on
    // every path before joining, so idle workers see the queue close and exit.
    let result = if cancel.load(Ordering::Relaxed) {
        drop(scheduler);
        Ok(())
    } else {
        let decoded = decoded.and_then(|_| match scheduler.error() {
            Some(error) => Err(error.clone()),
            None => pipeline.finish(&mut |job| scheduler.schedule(job)),
        });
        match decoded {
            Ok(()) => scheduler.finish(),
            Err(error) => {
                drop(scheduler);
                Err(error)
            }
        }
    };
    for handle in handles {
        handle.join().ok();
    }
    result
}
