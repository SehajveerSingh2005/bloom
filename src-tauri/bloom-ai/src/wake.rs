//! "Hey <name>": an optional wake word trained on the user's own voice.
//! The user records a few samples (enroll_sample), Rustpotter turns them into
//! `wake/wake.rpw` (build, with the trained name in `wake/name.txt` for Bloom to compare), and while the toggle is on a Listener keeps
//! the microphone open and runs Rustpotter on it, all locally. After a
//! detection the Listener records the request until the user stops talking.

use crate::voice;
use rustpotter::{
    Rustpotter, RustpotterConfig, SampleFormat, VADMode, WakewordLoad, WakewordRef,
    WakewordRefBuildFromFiles, WakewordSave,
};
use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

/// Task ids for wake requests start here, far above Bloom's own counter.
pub const FIRST_TASK: u64 = 1_000_000_000;
/// Rustpotter's label for the model; the spoken name lives in name.txt.
const NAME: &str = "wake";
/// MFCC coefficients per frame, Rustpotter's usual value.
const MFCC_SIZE: u16 = 16;
const SAMPLE_MS: usize = 2500;
const MIN_SAMPLES: usize = 3;
/// A second detection this soon after one is the same "Hey <name>".
const COOLDOWN: Duration = Duration::from_secs(2);
/// Audio kept from before a detection, so words said right after the wake
/// word (while Rustpotter is still confirming it) are not lost.
const PRE_ROLL_MS: usize = 500;

const WINDOW_MS: usize = 30;
/// Windows (300 ms) the noise floor is learned from.
const LEARN_WINDOWS: usize = 10;
/// Speech is louder than this whatever the floor (about -40 dBFS).
const MIN_RMS: f32 = 330.0;
const END_SILENCE_MS: usize = 900;
const MAX_REQUEST_MS: usize = 12_000;
const NO_SPEECH_MS: usize = 4_000;

pub fn model_path(dir: &Path) -> PathBuf {
    dir.join("wake").join("wake.rpw")
}

fn name_path(dir: &Path) -> PathBuf {
    dir.join("wake").join("name.txt")
}

fn sample_path(dir: &Path, index: u32) -> PathBuf {
    dir.join("wake").join(format!("sample-{index}.wav"))
}

fn samples(dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(dir.join("wake"))
        .into_iter()
        .flatten()
        .flatten()
        .map(|e| e.path())
        .filter(|p| {
            let name = p.file_name().unwrap_or_default().to_string_lossy();
            name.starts_with("sample-") && name.ends_with(".wav")
        })
        .collect();
    found.sort();
    found
}

/// Records one "Hey <name>" (blocking, ~2.5 s) and saves it without the
/// silence around it. Sample 1 starts a new set, so a retrain never mixes in
/// samples from an earlier one.
pub fn enroll_sample(dir: &Path, index: u32) -> Result<PathBuf, String> {
    let mut clip: Vec<i16> = Vec::new();
    let rate = voice::stream(|rate, chunk| {
        clip.extend_from_slice(chunk);
        clip.len() < rate as usize * SAMPLE_MS / 1000
    })?;
    let speech = trim(&clip, rate).ok_or("Didn't hear anything. Try again a little louder.")?;
    if index == 1 {
        for old in samples(dir) {
            let _ = std::fs::remove_file(old);
        }
    }
    let path = sample_path(dir, index);
    std::fs::create_dir_all(dir.join("wake")).map_err(|e| e.to_string())?;
    std::fs::write(&path, voice::wav(speech, rate)).map_err(|e| e.to_string())?;
    Ok(path)
}

/// Turns the saved samples into the wake word file.
pub fn build(dir: &Path, name: &str) -> Result<(), String> {
    let files: Vec<String> = samples(dir)
        .iter()
        .map(|p| p.to_string_lossy().into_owned())
        .collect();
    if files.len() < MIN_SAMPLES {
        return Err(format!(
            "Record at least {MIN_SAMPLES} samples of \"Hey {name}\" first."
        ));
    }
    let wakeword = WakewordRef::new_from_sample_files(NAME.into(), None, None, files, MFCC_SIZE)?;
    wakeword.save_to_file(&model_path(dir).to_string_lossy())?;
    std::fs::write(name_path(dir), name).map_err(|e| e.to_string())
}

/// Counts running or recording requests while alive. The Listener ignores
/// the wake word while any exist.
pub struct Busy(Arc<AtomicUsize>);

impl Busy {
    pub fn new(count: &Arc<AtomicUsize>) -> Busy {
        count.fetch_add(1, Ordering::Relaxed);
        Busy(count.clone())
    }
}

impl Drop for Busy {
    fn drop(&mut self) {
        self.0.fetch_sub(1, Ordering::Relaxed);
    }
}

/// What the Listener tells the message loop.
pub enum Event {
    /// "Hey <name>" was heard; the request is being recorded.
    Wake,
    /// The request ended. `Err` means nobody spoke. `busy` keeps the wake
    /// word paused until the request is done with it.
    Clip {
        audio: Result<(Vec<i16>, u32), String>,
        busy: Busy,
    },
}

/// The microphone stays open while this lives; dropping it closes it.
pub struct Listener {
    stop: Arc<AtomicBool>,
    thread: Option<std::thread::JoinHandle<()>>,
}

impl Listener {
    pub fn start(
        dir: &Path,
        busy: Arc<AtomicUsize>,
        events: UnboundedSender<Event>,
    ) -> Result<Listener, String> {
        let model = WakewordRef::load_from_file(&model_path(dir).to_string_lossy())
            .map_err(|_| "Teach Janice your voice first in Settings > AI.".to_string())?;
        let stop = Arc::new(AtomicBool::new(false));
        let flag = stop.clone();
        let thread = std::thread::spawn(move || {
            if let Err(message) = listen(model, &flag, &busy, &events) {
                crate::protocol::emit(&crate::protocol::Out::Error {
                    task: None,
                    message,
                });
            }
        });
        Ok(Listener {
            stop,
            thread: Some(thread),
        })
    }
}

impl Drop for Listener {
    fn drop(&mut self) {
        self.stop.store(true, Ordering::Relaxed);
        if let Some(thread) = self.thread.take() {
            let _ = thread.join();
        }
    }
}

fn detector(model: WakewordRef, rate: u32) -> Result<Rustpotter, String> {
    let mut config = RustpotterConfig::default();
    config.fmt.sample_rate = rate as usize;
    config.fmt.sample_format = SampleFormat::I16;
    // Scores only while there is sound well above the room's quiet level:
    // about 1-2% of a core in a noisy room instead of ~3%.
    config.detector.vad_mode = Some(VADMode::Hard);
    let mut detector = Rustpotter::new(&config)?;
    detector.add_wakeword_ref(NAME, model)?;
    Ok(detector)
}

fn listen(
    model: WakewordRef,
    stop: &AtomicBool,
    busy: &Arc<AtomicUsize>,
    events: &UnboundedSender<Event>,
) -> Result<(), String> {
    // Built on the first callback, once the microphone's rate is known.
    let mut model = Some(model);
    let mut rustpotter: Option<Rustpotter> = None;
    let mut failed: Option<String> = None;
    let mut frame: Vec<i16> = Vec::new();
    let mut recent: VecDeque<i16> = VecDeque::new();
    let mut request: Option<(Endpoint, Vec<i16>, Busy)> = None;
    let mut paused = false;
    let mut last_wake: Option<Instant> = None;
    voice::stream(|rate, chunk| {
        if stop.load(Ordering::Relaxed) {
            return false;
        }
        if let Some((endpoint, clip, _)) = request.as_mut() {
            clip.extend_from_slice(chunk);
            let step = endpoint.feed(chunk);
            if step != Step::Listening {
                let (_, clip, busy) = request.take().expect("checked above");
                let audio = match step {
                    Step::Done => Ok((clip, rate)),
                    _ => Err("Didn't catch that.".to_string()),
                };
                let _ = events.send(Event::Clip { audio, busy });
            }
            return true;
        }
        let detector = match rustpotter.as_mut() {
            Some(d) => d,
            None => match detector(model.take().expect("built once"), rate) {
                Ok(d) => rustpotter.insert(d),
                Err(e) => {
                    failed = Some(format!("The wake word couldn't start: {e}"));
                    return false;
                }
            },
        };
        if busy.load(Ordering::Relaxed) > 0 {
            if !paused {
                paused = true;
                frame.clear();
                recent.clear();
                detector.reset();
            }
            return true;
        }
        paused = false;
        recent.extend(chunk);
        let keep = rate as usize * PRE_ROLL_MS / 1000;
        if recent.len() > keep {
            recent.drain(..recent.len() - keep);
        }
        frame.extend_from_slice(chunk);
        let size = detector.get_samples_per_frame();
        let mut heard = false;
        while frame.len() >= size {
            let samples: Vec<i16> = frame.drain(..size).collect();
            heard |= detector.process_samples(samples).is_some();
        }
        if heard && last_wake.is_none_or(|t| t.elapsed() >= COOLDOWN) {
            last_wake = Some(Instant::now());
            request = Some((
                Endpoint::new(rate),
                recent.drain(..).collect(),
                Busy::new(busy),
            ));
            frame.clear();
            let _ = events.send(Event::Wake);
        }
        true
    })?;
    failed.map_or(Ok(()), Err)
}

fn window_len(rate: u32) -> usize {
    (rate as usize * WINDOW_MS / 1000).max(1)
}

fn rms(window: &[i16]) -> f32 {
    if window.is_empty() {
        return 0.0;
    }
    let sum: f64 = window.iter().map(|&s| (s as f64) * (s as f64)).sum();
    (sum / window.len() as f64).sqrt() as f32
}

/// Energy voice detection over 30 ms windows: speech is well above a noise
/// floor learned from the first 300 ms (its 10th percentile).
#[derive(Default)]
struct Vad {
    learned: Vec<f32>,
    floor: f32,
}

impl Vad {
    fn is_speech(&mut self, window: &[i16]) -> bool {
        let level = rms(window);
        if self.learned.len() < LEARN_WINDOWS {
            self.learned.push(level);
            let mut sorted = self.learned.clone();
            sorted.sort_by(f32::total_cmp);
            self.floor = sorted[sorted.len() / 10];
        }
        level > MIN_RMS && level > self.floor * 3.0
    }
}

/// The part of a clip from its first to its last speech window, with 90 ms
/// either side. None if nobody spoke.
fn trim(samples: &[i16], rate: u32) -> Option<&[i16]> {
    let win = window_len(rate);
    let mut vad = Vad::default();
    let speech: Vec<bool> = samples.chunks(win).map(|w| vad.is_speech(w)).collect();
    let first = speech.iter().position(|&s| s)?;
    let last = speech.iter().rposition(|&s| s)?;
    let pad = 3;
    Some(&samples[first.saturating_sub(pad) * win..((last + 1 + pad) * win).min(samples.len())])
}

#[derive(Debug, PartialEq)]
enum Step {
    Listening,
    /// Speech, then a pause (or the time limit).
    Done,
    /// Nothing said at all.
    NoSpeech,
}

/// Decides when a wake request is over.
struct Endpoint {
    win: usize,
    pending: Vec<i16>,
    vad: Vad,
    windows: usize,
    heard: bool,
    quiet: usize,
}

impl Endpoint {
    fn new(rate: u32) -> Endpoint {
        Endpoint {
            win: window_len(rate),
            pending: Vec::new(),
            vad: Vad::default(),
            windows: 0,
            heard: false,
            quiet: 0,
        }
    }

    fn feed(&mut self, chunk: &[i16]) -> Step {
        self.pending.extend_from_slice(chunk);
        while self.pending.len() >= self.win {
            let speech = self.vad.is_speech(&self.pending[..self.win]);
            self.pending.drain(..self.win);
            self.windows += 1;
            if speech {
                self.heard = true;
                self.quiet = 0;
            } else {
                self.quiet += 1;
            }
            let elapsed = self.windows * WINDOW_MS;
            if self.heard && (self.quiet * WINDOW_MS >= END_SILENCE_MS || elapsed >= MAX_REQUEST_MS)
            {
                return Step::Done;
            }
            if !self.heard && elapsed >= NO_SPEECH_MS {
                return Step::NoSpeech;
            }
        }
        Step::Listening
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::temp_dir;

    const RATE: u32 = 16_000;

    fn quiet(ms: usize) -> Vec<i16> {
        // Low noise, not digital silence.
        (0..RATE as usize * ms / 1000)
            .map(|i| if i % 2 == 0 { 40 } else { -40 })
            .collect()
    }

    fn loud(ms: usize) -> Vec<i16> {
        (0..RATE as usize * ms / 1000)
            .map(|i| ((i as f32 * 0.07).sin() * 6000.0) as i16)
            .collect()
    }

    fn run(endpoint: &mut Endpoint, audio: &[i16]) -> (Step, usize) {
        // 10 ms chunks, like WASAPI packets; returns when it stopped (ms).
        for (i, chunk) in audio.chunks(RATE as usize / 100).enumerate() {
            let step = endpoint.feed(chunk);
            if step != Step::Listening {
                return (step, (i + 1) * 10);
            }
        }
        (Step::Listening, audio.len() * 1000 / RATE as usize)
    }

    #[test]
    fn rms_of_a_square_wave() {
        assert_eq!(rms(&[100, -100, 100, -100]), 100.0);
        assert_eq!(rms(&[]), 0.0);
    }

    #[test]
    fn stops_after_speech_and_a_pause() {
        let audio = [quiet(300), loud(1500), quiet(2000)].concat();
        let (step, at) = run(&mut Endpoint::new(RATE), &audio);
        assert_eq!(step, Step::Done);
        // 300 + 1500 + 900 ms of silence, give or take a window.
        assert!((2690..=2760).contains(&at), "{at}");
    }

    #[test]
    fn short_pauses_do_not_end_the_request() {
        let audio = [quiet(300), loud(800), quiet(500), loud(800), quiet(1500)].concat();
        let (step, at) = run(&mut Endpoint::new(RATE), &audio);
        assert_eq!(step, Step::Done);
        assert!(at > 300 + 800 + 500 + 800, "{at}");
    }

    #[test]
    fn gives_up_without_speech() {
        let (step, at) = run(&mut Endpoint::new(RATE), &quiet(6000));
        assert_eq!(step, Step::NoSpeech);
        assert!((3990..=4020).contains(&at), "{at}");
    }

    #[test]
    fn caps_a_long_request() {
        let audio = [quiet(300), loud(20_000)].concat();
        let (step, at) = run(&mut Endpoint::new(RATE), &audio);
        assert_eq!(step, Step::Done);
        assert!((11_990..=12_020).contains(&at), "{at}");
    }

    #[test]
    fn noise_floor_adapts_to_a_noisy_room() {
        // Steady fan noise at speech-like volume is not speech.
        let fan: Vec<i16> = loud(6000).iter().map(|s| s / 4).collect();
        assert_eq!(run(&mut Endpoint::new(RATE), &fan).0, Step::NoSpeech);
    }

    #[test]
    fn trims_silence_around_speech() {
        let audio = [quiet(600), loud(900), quiet(1000)].concat();
        let speech = trim(&audio, RATE).unwrap();
        let ms = speech.len() * 1000 / RATE as usize;
        assert!((900..=1100).contains(&ms), "{ms}");
        assert!(trim(&quiet(2500), RATE).is_none());
    }

    #[test]
    fn busy_counts_live_guards() {
        let count = Arc::new(AtomicUsize::new(0));
        let a = Busy::new(&count);
        let b = Busy::new(&count);
        drop(a);
        assert_eq!(count.load(Ordering::Relaxed), 1);
        drop(b);
        assert_eq!(count.load(Ordering::Relaxed), 0);
    }

    #[test]
    fn sample_paths_and_listing() {
        let dir = temp_dir();
        assert!(model_path(&dir).ends_with("wake/wake.rpw"));
        std::fs::create_dir_all(dir.join("wake")).unwrap();
        for name in ["sample-2.wav", "sample-1.wav", "wake.rpw", "notes.txt"] {
            std::fs::write(dir.join("wake").join(name), b"").unwrap();
        }
        let names: Vec<String> = samples(&dir)
            .iter()
            .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
            .collect();
        assert_eq!(names, ["sample-1.wav", "sample-2.wav"]);
    }

    /// A synthetic "voice": a few vowel-like tones with a bit of jitter.
    fn fake_voice(seed: u32) -> Vec<i16> {
        let mut noise = seed;
        (0..RATE as usize * 9 / 10)
            .map(|i| {
                noise = noise.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                let t = i as f32 / RATE as f32;
                let pitch = if t < 0.45 { 220.0 } else { 330.0 };
                let tone = (t * pitch * std::f32::consts::TAU).sin()
                    + 0.5 * (t * pitch * 2.0 * std::f32::consts::TAU).sin();
                (tone * 6000.0 + (noise >> 20) as f32 - 2048.0) as i16
            })
            .collect()
    }

    #[test]
    fn builds_saves_and_loads_a_wake_word() {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("wake")).unwrap();
        std::fs::write(sample_path(&dir, 1), voice::wav(&fake_voice(1), RATE)).unwrap();
        assert!(build(&dir, "Mina").unwrap_err().contains("at least 3"));
        for i in 2..=3 {
            std::fs::write(sample_path(&dir, i), voice::wav(&fake_voice(i), RATE)).unwrap();
        }
        build(&dir, "Mina").unwrap();
        assert_eq!(std::fs::read_to_string(name_path(&dir)).unwrap(), "Mina");
        let model = WakewordRef::load_from_file(&model_path(&dir).to_string_lossy()).unwrap();
        assert_eq!(model.name, NAME);
        assert_eq!(model.samples_features.len(), 3);
        // A detector at a 48 kHz mic rate takes it and wants 30 ms frames.
        let mut d = detector(model, 48_000).unwrap();
        assert_eq!(d.get_samples_per_frame(), 1440);
        assert!(d.process_samples(vec![0i16; 1440]).is_none());
    }
}
