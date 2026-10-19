// Speech segmentation, ordered parallel recognition and stitching.
// Ported from Hoshi Reader Android (HuangAntimony, GPL-3.0): SasayakiEnergyVad,
// SpeechSegmenter, SasayakiSpeechPipeline, SasayakiRecognitionStitcher.

use std::collections::VecDeque;
use std::sync::mpsc;

use super::Token;

pub const SAMPLE_RATE: i64 = 16_000;
const FRAME: usize = 512;
const SPEECH_LEAD_SAMPLES: i64 = 16_000;
const SPEECH_CONTINUATION_LEAD_SAMPLES: i64 = 8_000;
const SPEECH_TAIL_SAMPLES: i64 = 4_000;

/// How speech is cut into pieces for the model. A pause of at least 0.5 s ends a piece
/// once the piece is `min_piece` long; a pause of `long_pause` always ends it. Pieces are
/// cut at most at `max_piece`, at a pause when there is one.
#[derive(Clone, Copy)]
pub struct Cutting {
    min_piece: i64,
    long_pause: i64,
    max_piece: i64,
}

impl Cutting {
    /// parakeet uses context (15-30 s pieces). Given a long silence inside a piece, it
    /// tends to transcribe only one side of it, so 1.25 s of silence always ends one.
    pub const PARAKEET: Cutting = Cutting { min_piece: 240_000, long_pause: 20_000, max_piece: 480_000 };
    /// ReazonSpeech, as on the other platforms: every pause ends a piece, at most 20 s.
    pub const REAZONSPEECH: Cutting = Cutting { min_piece: 0, long_pause: 8_000, max_piece: 320_000 };
}

impl Default for Cutting {
    fn default() -> Self {
        Cutting::PARAKEET
    }
}

/// Clean audiobook speech is separated from silence by energy. A bounded 30-second
/// histogram follows the recording's noise floor.
pub struct EnergyVad {
    history: Vec<usize>,
    histogram: [usize; 101],
    cursor: usize,
}

impl EnergyVad {
    pub fn new() -> Self {
        let history = vec![0; 938];
        let mut histogram = [0; 101];
        histogram[0] = history.len();
        EnergyVad { history, histogram, cursor: 0 }
    }

    fn probability(&mut self, samples: &[f32]) -> f32 {
        let energy: f64 = samples.iter().map(|&s| s as f64 * s as f64).sum();
        let db = if energy > 0.0 {
            (10.0 * (energy / samples.len() as f64).log10()).clamp(-100.0, 0.0)
        } else {
            -100.0
        };
        let bin = (db.round() as i64 + 100) as usize;
        self.histogram[self.history[self.cursor]] -= 1;
        self.history[self.cursor] = bin;
        self.histogram[bin] += 1;
        self.cursor = (self.cursor + 1) % self.history.len();
        // 10th percentile + 12 dB, clamped so quiet voices and digital silence behave.
        let mut count = 0;
        let mut floor = 0;
        while floor < self.histogram.len() - 1 {
            count += self.histogram[floor];
            if count > self.history.len() / 10 {
                break;
            }
            floor += 1;
        }
        let threshold = (floor as i64 - 100 + 12).clamp(-55, -30) as f64;
        (1.0 / (1.0 + (-(db - threshold) / 2.0).exp())) as f32
    }
}

#[derive(Clone, Copy)]
struct SpeechBounds {
    start: i64,
    end: i64,
    hard_cut: bool,
}

impl SpeechBounds {
    fn padded_start(&self) -> i64 {
        (self.start - SPEECH_LEAD_SAMPLES).max(0)
    }
    fn padded_end(&self) -> i64 {
        self.end + SPEECH_TAIL_SAMPLES
    }
}

/// Keeps short pauses within speech; rejects brief sounds only after a segment ends.
#[derive(Default)]
struct SpeechSegmenter {
    position: i64,
    candidate_start: Option<i64>,
    speech: bool,
    silence_start: Option<i64>,
    continuation: bool,
    cutting: Cutting,
}

impl SpeechSegmenter {
    fn safe_through_sample(&self) -> i64 {
        (self.candidate_start.unwrap_or(self.position) - SPEECH_LEAD_SAMPLES).max(0)
    }

    fn accept(&mut self, probability: f32, count: usize) -> Option<SpeechBounds> {
        let frame_start = self.position;
        self.position += count as i64;
        if !self.speech {
            if probability >= 0.5 {
                self.candidate_start.get_or_insert(frame_start);
                self.speech = true;
            } else {
                self.candidate_start = None;
            }
        }
        if !self.speech {
            return None;
        }
        if probability < 0.35 {
            let silence = *self.silence_start.get_or_insert(frame_start);
            let long_enough = silence - self.candidate_start.unwrap_or(silence) >= self.cutting.min_piece;
            if self.position - silence >= 8_000 && (long_enough || self.position - silence >= self.cutting.long_pause) {
                let result = self.bounds(silence);
                self.candidate_start = None;
                self.silence_start = None;
                self.speech = false;
                self.continuation = false;
                return result;
            }
        } else {
            self.silence_start = None;
        }
        let candidate = self.candidate_start.unwrap();
        if self.position - candidate >= self.cutting.max_piece {
            let end = self.silence_start.unwrap_or(self.position);
            let result = SpeechBounds { start: candidate, end, hard_cut: self.silence_start.is_none() };
            self.speech = self.silence_start.is_none();
            self.continuation = self.speech;
            self.candidate_start = self.speech.then_some(self.position);
            self.silence_start = None;
            return Some(result);
        }
        None
    }

    fn finish(&mut self) -> Option<SpeechBounds> {
        let result = if self.speech { self.bounds(self.silence_start.unwrap_or(self.position)) } else { None };
        self.candidate_start = None;
        self.silence_start = None;
        self.speech = false;
        self.continuation = false;
        result
    }

    fn bounds(&self, end: i64) -> Option<SpeechBounds> {
        let start = self.candidate_start?;
        // Even an immediate silence after a hard cut owns its trailing context.
        (end >= start && (self.continuation || end - start >= 4_000))
            .then_some(SpeechBounds { start, end, hard_cut: false })
    }
}

/// Fixed-size PCM history supporting leading and trailing recognition context.
struct AudioHistory {
    samples: Vec<f32>,
    end_sample: i64,
}

impl AudioHistory {
    fn new(capacity: usize) -> Self {
        AudioHistory { samples: vec![0.0; capacity], end_sample: 0 }
    }
    fn start_sample(&self) -> i64 {
        (self.end_sample - self.samples.len() as i64).max(0)
    }
    fn append(&mut self, input: &[f32]) {
        let capacity = self.samples.len() as i64;
        for &sample in input {
            self.samples[(self.end_sample % capacity) as usize] = sample;
            self.end_sample += 1;
        }
    }
    fn read(&self, from: i64, through: i64) -> Vec<f32> {
        let from = from.max(self.start_sample());
        let capacity = self.samples.len() as i64;
        (from..through).map(|i| self.samples[(i % capacity) as usize]).collect()
    }
}

pub struct Batch {
    pub tokens: Vec<Token>,
    pub through: f64,
    pub segment_start: f64,
}

pub struct Recognition {
    pub samples: Vec<f32>,
    pub segment_start: f64,
    pub segment_end: f64,
    pub through: f64,
}

pub enum Job {
    Ready(Batch),
    Recognize(Recognition),
}

/// Turns raw recognizer output (characters + start offsets) into timed tokens.
pub fn project_tokens(texts: &[String], timestamps: &[f32], segment_start: f64, segment_end: f64) -> Vec<Token> {
    let mut tokens = Vec::new();
    for (index, raw) in texts.iter().enumerate() {
        let text = raw.replace('\u{2581}', " ");
        let offset = timestamps[index] as f64;
        let start = segment_start + offset;
        // RNN-T can emit several characters on one frame: give them the interval
        // up to the next distinct timestamp.
        let next = timestamps[index + 1..]
            .iter()
            .find(|&&t| t as f64 > offset)
            .map_or(segment_end, |&t| segment_start + t as f64);
        if text.trim().is_empty() || text == "<blk>" || text == "<unk>" || !start.is_finite()
            || !next.is_finite() || start >= segment_end || offset < 0.0
        {
            continue;
        }
        // Sentence pieces can hold several characters (っている): one token per character,
        // spread over the piece's interval. Spaces and punctuation are dropped.
        let end = next.max(start).min(segment_end);
        let characters: Vec<char> = text.chars().filter(|c| c.is_alphanumeric()).collect();
        let step = (end - start) / characters.len().max(1) as f64;
        for (position, character) in characters.iter().enumerate() {
            tokens.push(Token {
                text: character.to_string(),
                start: start + step * position as f64,
                end: start + step * (position + 1) as f64,
            });
        }
    }
    tokens
}

pub struct Pipeline {
    origin_sample: i64,
    duration: f64,
    history: AudioHistory,
    segmenter: SpeechSegmenter,
    vad: EnergyVad,
    pending: VecDeque<SpeechBounds>,
    scheduled_through: f64,
    frame: Vec<f32>,
    frame_size: usize,
    received: i64,
}

impl Pipeline {
    pub fn new(from: f64, duration: f64, audio_from: f64, cutting: Cutting) -> Self {
        Pipeline {
            origin_sample: (audio_from * SAMPLE_RATE as f64 - 1e-8).ceil() as i64,
            duration,
            history: AudioHistory::new(48 * SAMPLE_RATE as usize),
            segmenter: SpeechSegmenter { cutting, ..Default::default() },
            vad: EnergyVad::new(),
            pending: VecDeque::new(),
            scheduled_through: from,
            frame: vec![0.0; FRAME],
            frame_size: 0,
            received: 0,
        }
    }

    pub fn accept(&mut self, samples: &[f32], schedule: &mut impl FnMut(Job)) {
        self.received += samples.len() as i64;
        let mut offset = 0;
        while offset < samples.len() {
            let count = (FRAME - self.frame_size).min(samples.len() - offset);
            self.frame[self.frame_size..self.frame_size + count].copy_from_slice(&samples[offset..offset + count]);
            self.frame_size += count;
            offset += count;
            if self.frame_size == FRAME {
                self.process_frame(FRAME, schedule);
            }
        }
    }

    pub fn finish(&mut self, schedule: &mut impl FnMut(Job)) -> Result<(), String> {
        if self.frame_size > 0 {
            let size = self.frame_size;
            self.frame[size..].fill(0.0);
            self.process_frame(size, schedule);
        }
        if let Some(bounds) = self.segmenter.finish() {
            self.pending.push_back(bounds);
        }
        self.drain(true, schedule);
        let decoded_end = (self.origin_sample + self.received) as f64 / SAMPLE_RATE as f64;
        if self.received == 0 || decoded_end < self.duration - 1.5 {
            return Err("Audio ended before its declared duration".into());
        }
        self.emit(self.duration, schedule);
        Ok(())
    }

    fn process_frame(&mut self, count: usize, schedule: &mut impl FnMut(Job)) {
        self.history.append(&self.frame[..count]);
        let probability = self.vad.probability(&self.frame);
        if let Some(bounds) = self.segmenter.accept(probability, count) {
            self.pending.push_back(bounds);
        }
        self.frame_size = 0;
        self.drain(false, schedule);
        let safe = self
            .segmenter
            .safe_through_sample()
            .min(self.pending.front().map_or(i64::MAX, |b| b.padded_start()));
        let through = self.duration.min((self.origin_sample + safe) as f64 / SAMPLE_RATE as f64);
        if through - self.scheduled_through >= 5.0 {
            self.emit(through, schedule);
        }
    }

    fn drain(&mut self, finishing: bool, schedule: &mut impl FnMut(Job)) {
        let rate = SAMPLE_RATE as f64;
        while let Some(&bounds) = self.pending.front() {
            if !finishing && bounds.padded_end() > self.history.end_sample {
                return;
            }
            let end = bounds.padded_end().min(self.history.end_sample);
            let scheduled_sample = (self.scheduled_through * rate - 1e-8).ceil() as i64 - self.origin_sample;
            // Trim leading context only before new speech; hard cuts and resumes keep
            // half a second of context, and the stitcher removes repeated text.
            let start = if bounds.start > scheduled_sample {
                bounds.padded_start().max(scheduled_sample)
            } else {
                (bounds.start - SPEECH_CONTINUATION_LEAD_SAMPLES).max(0)
            };
            let samples = self.history.read(start, end);
            let owned_end = if bounds.hard_cut { bounds.end } else { end };
            let through = self.duration.min((self.origin_sample + owned_end) as f64 / rate);
            if through > self.scheduled_through {
                schedule(Job::Recognize(Recognition {
                    samples,
                    segment_start: (self.origin_sample + start) as f64 / rate,
                    segment_end: (self.origin_sample + end) as f64 / rate,
                    through,
                }));
                self.scheduled_through = through;
            }
            self.pending.pop_front();
        }
    }

    fn emit(&mut self, through: f64, schedule: &mut impl FnMut(Job)) {
        if through <= self.scheduled_through {
            return;
        }
        schedule(Job::Ready(Batch { tokens: Vec::new(), through, segment_start: through }));
        self.scheduled_through = through;
    }
}

/// Consumed in audio order after parallel recognition; removes the overlap that
/// leading context produces between neighbouring segments.
pub struct Stitcher {
    committed: f64,
    previous: Vec<Token>,
    recognized: bool,
    emitted_through: f64,
}

impl Stitcher {
    pub fn new(from: f64, previous_tokens: &[Token]) -> Self {
        let previous: Vec<Token> = previous_tokens.iter().filter(|t| t.end >= from - 1.0).cloned().collect();
        let emitted_through = from.max(previous.iter().map(|t| t.end).fold(from, f64::max));
        Stitcher { committed: from, recognized: !previous.is_empty(), previous, emitted_through }
    }

    pub fn accept(&mut self, batch: Batch) -> (Vec<Token>, f64) {
        if batch.through <= self.committed {
            return (Vec::new(), self.committed);
        }
        let raw = batch.tokens;
        let overlap: Vec<&Token> = self.previous.iter().filter(|t| t.end >= batch.segment_start).collect();
        let duplicate = (1..=overlap.len().min(raw.len()))
            .rev()
            .find(|&count| {
                let old = &overlap[overlap.len() - count..];
                let next = &raw[..count];
                let same_text = old.iter().zip(next).all(|(a, b)| a.text == b.text);
                let joined: String = next.iter().map(|t| t.text.as_str()).collect();
                let mut distinct: Vec<char> = joined.chars().collect();
                distinct.sort_unstable();
                distinct.dedup();
                let phrase = count > 1 && distinct.len() > 1;
                let drift = (old[0].start - next[0].start).abs();
                same_text
                    && next[0].start < self.emitted_through
                    && if phrase { drift <= 0.75 } else { drift <= 0.25 }
            })
            .unwrap_or(0);
        let retained: Vec<Token> = if !self.recognized {
            raw.into_iter().filter(|t| t.start >= self.committed).collect()
        } else {
            raw.into_iter().skip(duplicate).collect()
        };
        let early = if retained.first().is_some_and(|t| t.start < self.emitted_through) {
            retained.iter().position(|t| t.start > self.emitted_through).unwrap_or(retained.len())
        } else {
            0
        };
        let end = retained
            .get(early)
            .map(|t| t.start)
            .or(retained.last().map(|t| t.end))
            .unwrap_or(batch.through);
        let emitted = self.emitted_through;
        let tokens: Vec<Token> = retained
            .iter()
            .enumerate()
            .map(|(index, token)| {
                if index >= early {
                    token.clone()
                } else {
                    Token {
                        text: token.text.clone(),
                        start: emitted + (end - emitted) * index as f64 / early as f64,
                        end: emitted + (end - emitted) * (index + 1) as f64 / early as f64,
                    }
                }
            })
            .collect();
        let mut previous = std::mem::take(&mut self.previous);
        previous.extend(retained.iter().enumerate().filter(|(i, _)| tokens[*i].end > tokens[*i].start).map(|(_, t)| t.clone()));
        self.previous = previous.into_iter().filter(|t| t.end >= batch.through - 1.0).collect();
        self.recognized = true;
        let published: Vec<Token> = tokens.into_iter().filter(|t| t.end > t.start).collect();
        self.emitted_through = published.iter().map(|t| t.end).fold(self.emitted_through, f64::max);
        self.committed = batch.through;
        (published, self.committed)
    }
}

/// Runs `recognize` for scheduled segments on `workers` threads while publishing
/// results strictly in audio order (bounded lookahead, like the Android pipeline).
pub struct OrderedScheduler<'a> {
    jobs: Option<mpsc::Sender<(Recognition, mpsc::SyncSender<Result<Batch, String>>)>>,
    pending: VecDeque<mpsc::Receiver<Result<Batch, String>>>,
    lookahead: usize,
    stitcher: Stitcher,
    published_through: f64,
    on_batch: &'a mut dyn FnMut(Vec<Token>, f64),
    error: Option<String>,
}

impl<'a> OrderedScheduler<'a> {
    pub fn new(
        jobs: mpsc::Sender<(Recognition, mpsc::SyncSender<Result<Batch, String>>)>,
        lookahead: usize,
        stitcher: Stitcher,
        from: f64,
        on_batch: &'a mut dyn FnMut(Vec<Token>, f64),
    ) -> Self {
        OrderedScheduler { jobs: Some(jobs), pending: VecDeque::new(), lookahead, stitcher, published_through: from, on_batch, error: None }
    }

    pub fn schedule(&mut self, job: Job) {
        let (sender, receiver) = mpsc::sync_channel(1);
        match job {
            Job::Ready(batch) => {
                sender.send(Ok(batch)).ok();
            }
            Job::Recognize(recognition) => {
                if let Some(jobs) = &self.jobs {
                    jobs.send((recognition, sender)).ok();
                }
            }
        }
        self.pending.push_back(receiver);
        while self.pending.len() > self.lookahead {
            self.consume_front();
        }
        while let Some(front) = self.pending.front() {
            match front.try_recv() {
                Ok(result) => {
                    self.pending.pop_front();
                    self.publish(result);
                }
                Err(_) => break,
            }
        }
    }

    fn consume_front(&mut self) {
        if let Some(front) = self.pending.pop_front() {
            let result = front.recv().unwrap_or_else(|_| Err("Recognition worker stopped".into()));
            self.publish(result);
        }
    }

    fn publish(&mut self, result: Result<Batch, String>) {
        match result {
            Ok(batch) if self.error.is_none() && batch.through > self.published_through => {
                self.published_through = batch.through;
                let (tokens, through) = self.stitcher.accept(batch);
                (self.on_batch)(tokens, through);
            }
            Ok(_) => {}
            Err(error) => {
                self.error.get_or_insert(error);
            }
        }
    }

    pub fn finish(mut self) -> Result<(), String> {
        while !self.pending.is_empty() {
            self.consume_front();
        }
        self.jobs.take();
        self.error.map_or(Ok(()), Err)
    }

    pub fn error(&self) -> Option<&String> {
        self.error.as_ref()
    }
}
