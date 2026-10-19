// Streaming audiobook decode to 16 kHz mono for speech recognition.

use std::fs::File;
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};

use sherpa_onnx::LinearResampler;
use symphonia::core::audio::SampleBuffer;
use symphonia::core::codecs::{CODEC_TYPE_NULL, DecoderOptions};
use symphonia::core::errors::Error;
use symphonia::core::formats::{FormatOptions, FormatReader, SeekMode, SeekTo, Track};
use symphonia::core::io::MediaSourceStream;
use symphonia::core::meta::MetadataOptions;
use symphonia::core::probe::Hint;
use symphonia::core::units::Time;

use super::pipeline::SAMPLE_RATE;

fn open(path: &Path) -> Result<Box<dyn FormatReader>, String> {
    let file = File::open(path).map_err(|e| format!("Could not open {}: {e}", path.display()))?;
    let stream = MediaSourceStream::new(Box::new(file), Default::default());
    let mut hint = Hint::new();
    if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
        hint.with_extension(ext);
    }
    symphonia::default::get_probe()
        .format(&hint, stream, &FormatOptions::default(), &MetadataOptions::default())
        .map(|probed| probed.format)
        .map_err(|e| format!("Unsupported audio file: {e}"))
}

fn audio_track(format: &dyn FormatReader) -> Result<Track, String> {
    format
        .tracks()
        .iter()
        .find(|t| t.codec_params.codec != CODEC_TYPE_NULL)
        .cloned()
        .ok_or_else(|| "No audio track".to_string())
}

/// Duration in seconds, from the container when it knows it, else by scanning packets.
pub fn duration(path: &Path) -> Option<f64> {
    let mut format = open(path).ok()?;
    let track = audio_track(format.as_ref()).ok()?;
    let rate = track.codec_params.sample_rate? as f64;
    if let Some(frames) = track.codec_params.n_frames {
        return Some(frames as f64 / rate);
    }
    let base = track.codec_params.time_base?;
    let mut end = 0u64;
    while let Ok(packet) = format.next_packet() {
        if packet.track_id() == track.id {
            end = end.max(packet.ts() + packet.dur());
        }
    }
    let time = base.calc_time(end);
    Some(time.seconds as f64 + time.frac)
}

/// Decodes from `from` seconds, calling `sink` with 16 kHz mono chunks.
/// Stops early (Ok) when `cancel` is set.
pub fn decode(path: &Path, from: f64, cancel: &AtomicBool, mut sink: impl FnMut(&[f32])) -> Result<(), String> {
    let mut format = open(path)?;
    let track = audio_track(format.as_ref())?;
    let rate = track.codec_params.sample_rate.ok_or("Unknown sample rate")?;
    let mut decoder = symphonia::default::get_codecs()
        .make(&track.codec_params, &DecoderOptions::default())
        .map_err(|e| format!("Unsupported audio codec: {e}"))?;

    // Position (in source frames) of the next decoded frame; frames before `skip_to` are dropped.
    let skip_to = (from * rate as f64).round() as u64;
    let mut position = 0u64;
    if from > 0.0 {
        if let Ok(seeked) = format.seek(SeekMode::Accurate, SeekTo::Time { time: Time::from(from), track_id: Some(track.id) }) {
            decoder.reset();
            position = track
                .codec_params
                .time_base
                .map(|base| {
                    let t = base.calc_time(seeked.actual_ts);
                    ((t.seconds as f64 + t.frac) * rate as f64).round() as u64
                })
                .unwrap_or(0);
        }
    }

    let resampler = if rate as i64 != SAMPLE_RATE {
        Some(LinearResampler::create(rate as i32, SAMPLE_RATE as i32).ok_or("Could not create resampler")?)
    } else {
        None
    };
    let mut mono = Vec::new();
    loop {
        if cancel.load(Ordering::Relaxed) {
            return Ok(());
        }
        let packet = match format.next_packet() {
            Ok(packet) => packet,
            Err(Error::IoError(e)) if e.kind() == std::io::ErrorKind::UnexpectedEof => break,
            Err(Error::ResetRequired) => break,
            Err(e) => return Err(format!("Audio read error: {e}")),
        };
        if packet.track_id() != track.id {
            continue;
        }
        let decoded = match decoder.decode(&packet) {
            Ok(decoded) => decoded,
            Err(Error::DecodeError(_)) => continue,
            Err(e) => return Err(format!("Audio decode error: {e}")),
        };
        let spec = *decoded.spec();
        let channels = spec.channels.count().max(1);
        let mut buffer = SampleBuffer::<f32>::new(decoded.capacity() as u64, spec);
        buffer.copy_interleaved_ref(decoded);
        let frames = buffer.samples().len() / channels;
        let skip = skip_to.saturating_sub(position).min(frames as u64) as usize;
        position += frames as u64;
        mono.clear();
        mono.extend(
            buffer.samples()[skip * channels..]
                .chunks_exact(channels)
                .map(|frame| frame.iter().sum::<f32>() / channels as f32),
        );
        if mono.is_empty() {
            continue;
        }
        match &resampler {
            Some(r) => {
                let out = r.resample(&mono, false);
                if !out.is_empty() {
                    sink(&out);
                }
            }
            None => sink(&mono),
        }
    }
    if let Some(r) = &resampler {
        let out = r.resample(&[], true);
        if !out.is_empty() {
            sink(&out);
        }
    }
    Ok(())
}
