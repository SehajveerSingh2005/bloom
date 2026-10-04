//! "Hey <name>": an optional wake word trained on the user's own voice.
//! The user records a few samples (enroll_sample), Rustpotter turns them into
//! `wake/wake.rpw` (build, with the trained name in `wake/name.txt` for Bloom to compare), and while the toggle is on a Listener keeps
//! the microphone open and runs Rustpotter on it, all locally. After a
//! detection the Listener records the request until the user stops talking.

use crate::voice;
use rustpotter::{
    Rustpotter, RustpotterConfig, RustpotterDetection, SampleFormat, WakewordLoad, WakewordRef,
    WakewordRefBuildFromBuffers, WakewordSave,
};
use std::collections::{HashMap, VecDeque};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicBool, AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::mpsc::UnboundedSender;

/// Task ids for wake requests start here, far above Bloom's own counter.
pub const FIRST_TASK: u64 = 1_000_000_000;
/// Rustpotter's label for the model; the spoken name lives in name.txt.
const NAME: &str = "wake-2";
/// The label of models built before `trim` was fixed; `load` rebuilds them.
const OLD_NAME: &str = "wake";
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
/// A pause this short inside the wake phrase does not split it.
const GAP_MS: usize = 300;
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
    samples_in(&dir.join("wake"))
}

/// The `sample-*.wav` files in a wake folder, sorted.
pub(crate) fn samples_in(wake_dir: &Path) -> Vec<PathBuf> {
    let mut found: Vec<PathBuf> = std::fs::read_dir(wake_dir)
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
    let files = samples(dir);
    if files.len() < MIN_SAMPLES {
        return Err(format!(
            "Record at least {MIN_SAMPLES} samples of \"Hey {name}\" first."
        ));
    }
    let wakeword = train(&files.iter().collect::<Vec<_>>(), false)?;
    wakeword.save_to_file(&model_path(dir).to_string_lossy())?;
    std::fs::write(name_path(dir), name).map_err(|e| e.to_string())
}

/// A wake word from sample files, each trimmed first if `retrim`.
pub(crate) fn train(files: &[&PathBuf], retrim: bool) -> Result<WakewordRef, String> {
    let mut buffers = HashMap::new();
    for path in files {
        let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
        let (clip, rate) = voice::read_wav(&bytes)?;
        let clip = match retrim {
            true => trim(&clip, rate).unwrap_or(&clip),
            false => &clip,
        };
        let key = path.file_name().unwrap_or_default().to_string_lossy();
        buffers.insert(key.into_owned(), voice::wav(clip, rate));
    }
    WakewordRef::new_from_sample_buffers(NAME.into(), None, None, buffers, MFCC_SIZE)
}

/// The trained wake word. One built before `trim` was fixed is rebuilt once
/// from its saved samples, trimmed now: those samples are ~2.2 s with only
/// ~0.7 s of voice, so the old model scored the user's voice at about the
/// threshold and quiet room noise above it.
fn load(dir: &Path, name: &str) -> Result<WakewordRef, String> {
    let path = model_path(dir).to_string_lossy().into_owned();
    let untrained = || format!("Teach {name} your voice first in Settings > AI.");
    let model = WakewordRef::load_from_file(&path).map_err(|_| untrained())?;
    if model.name != OLD_NAME {
        return Ok(model);
    }
    let files = samples(dir);
    if files.len() < MIN_SAMPLES {
        return Err(untrained());
    }
    let model = train(&files.iter().collect::<Vec<_>>(), true)?;
    model.save_to_file(&path)?;
    Ok(model)
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
    /// `name` is the assistant's configured name, for the error message.
    pub fn start(
        dir: &Path,
        name: &str,
        busy: Arc<AtomicUsize>,
        events: UnboundedSender<Event>,
    ) -> Result<Listener, String> {
        let model = load(dir, name)?;
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

/// The live detector settings for a microphone at `rate`.
pub(crate) fn settings(rate: u32) -> RustpotterConfig {
    let mut config = RustpotterConfig::default();
    config.fmt.sample_rate = rate as usize;
    config.fmt.sample_format = SampleFormat::I16;
    // No VAD: Rustpotter's VAD (a cepstrum level against the last 0.5 s)
    // took steady background noise and quieter voices for silence and
    // skipped scoring, so the phrase often went unheard. --wake-score shows it.
    config.detector.vad_mode = None;
    config
}

pub(crate) fn detector_with(
    model: WakewordRef,
    config: &RustpotterConfig,
) -> Result<Rustpotter, String> {
    let mut detector = Rustpotter::new(config)?;
    detector.add_wakeword_ref(NAME, model)?;
    Ok(detector)
}

fn detector(model: WakewordRef, rate: u32) -> Result<Rustpotter, String> {
    detector_with(model, &settings(rate))
}

/// Hands the detector the microphone's samples in the frame size it wants;
/// `frame` keeps the leftover between calls. The live listener and
/// `--wake-score` both feed audio through here.
pub(crate) fn feed(
    detector: &mut Rustpotter,
    frame: &mut Vec<i16>,
    chunk: &[i16],
) -> Option<RustpotterDetection> {
    frame.extend_from_slice(chunk);
    let size = detector.get_samples_per_frame();
    let mut heard = None;
    while frame.len() >= size {
        let samples: Vec<i16> = frame.drain(..size).collect();
        heard = detector.process_samples(samples).or(heard);
    }
    heard
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
        let heard = feed(detector, &mut frame, chunk).is_some();
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

/// The loudest stretch of speech in an enrollment clip (pauses up to
/// GAP_MS bridged), with 90 ms either side. None if nobody spoke.
///
/// Speech is judged against the clip's own quiet level (its 20th percentile
/// window), not the first 300 ms: mics ramp up after opening and rooms get
/// louder, and with a first-300-ms floor that later noise counted as speech,
/// so samples kept 1-1.5 s of noise around a 0.7 s phrase. Taking the
/// loudest stretch also drops a click or a cough away from the phrase.
pub(crate) fn trim(samples: &[i16], rate: u32) -> Option<&[i16]> {
    let win = window_len(rate);
    let levels: Vec<f32> = samples.chunks(win).map(rms).collect();
    let mut sorted = levels.clone();
    sorted.sort_by(f32::total_cmp);
    let loud = (sorted.get(sorted.len() / 5)? * 3.0).max(MIN_RMS);
    let gap = GAP_MS / WINDOW_MS;
    // (first, last, energy) of each stretch.
    let mut stretches: Vec<(usize, usize, f32)> = Vec::new();
    for (i, &level) in levels.iter().enumerate() {
        if level <= loud {
            continue;
        }
        match stretches.last_mut() {
            Some((_, last, energy)) if i - *last <= gap => {
                *last = i;
                *energy += level * level;
            }
            _ => stretches.push((i, i, level * level)),
        }
    }
    let (first, last, _) = stretches.into_iter().max_by(|a, b| a.2.total_cmp(&b.2))?;
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
    use rustpotter::VADMode;

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

    fn ms_of(clip: &[i16]) -> usize {
        clip.len() * 1000 / RATE as usize
    }

    #[test]
    fn trim_keeps_the_phrase_not_the_room() {
        // The mic starts near silent, the room is louder once it has warmed
        // up, and a click follows the phrase: only the phrase is kept. (The
        // old trim learned the floor from the first 300 ms and kept it all.)
        let scale = |clip: Vec<i16>, by: i16| -> Vec<i16> { clip.iter().map(|s| s * by).collect() };
        let ramp: Vec<i16> = quiet(300).iter().map(|s| s / 4).collect();
        let room = |ms| scale(quiet(ms), 15);
        let audio = [ramp, room(200), loud(700), room(600), loud(60), room(500)].concat();
        let phrase = ms_of(trim(&audio, RATE).unwrap());
        assert!((700..=900).contains(&phrase), "{phrase}");
    }

    #[test]
    fn trim_keeps_a_short_pause_inside_the_phrase() {
        let room = || -> Vec<i16> { quiet(200).iter().map(|s| s * 15).collect() };
        let audio = [
            room(),
            room(),
            loud(300),
            room(),
            loud(300),
            room(),
            room(),
            room(),
        ]
        .concat();
        let phrase = ms_of(trim(&audio, RATE).unwrap());
        assert!((800..=1000).contains(&phrase), "{phrase}");
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

    /// Steady hiss at about `level` RMS, like a fan or a laptop's mic floor.
    fn hiss(ms: usize, level: f32, seed: u32) -> Vec<i16> {
        let mut noise = seed;
        (0..RATE as usize * ms / 1000)
            .map(|_| {
                noise = noise.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                (((noise >> 16) as f32 / 65_535.0 - 0.5) * level * 3.46) as i16
            })
            .collect()
    }

    fn with_hiss(clip: &[i16], level: f32) -> Vec<i16> {
        let mix = clip
            .iter()
            .zip(hiss(clip.len() * 1000 / RATE as usize + 1, level, 9));
        mix.map(|(s, n)| s.saturating_add(n)).collect()
    }

    /// A wake word trained (trimmed, like enrollment) on `fake_voice` seeds 1-3.
    fn fake_model() -> Vec<u8> {
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("wake")).unwrap();
        let files: Vec<PathBuf> = (1..=3)
            .map(|i| {
                let take = [quiet(400), fake_voice(i), quiet(800)].concat();
                std::fs::write(sample_path(&dir, i), voice::wav(&take, RATE)).unwrap();
                sample_path(&dir, i)
            })
            .collect();
        crate::wake_score::build(&files.iter().collect::<Vec<_>>(), true).unwrap()
    }

    #[test]
    fn live_settings_hear_the_phrase_over_steady_noise() {
        // Rustpotter's VAD (Hard, and Easy too) judged steady background
        // noise "no voice" and never scored the phrase; live has it off.
        let model = fake_model();
        let room = |ms| hiss(ms, 700.0, 5);
        let audio = [room(1000), with_hiss(&fake_voice(7), 700.0), room(3000)].concat();
        let run = |config| crate::wake_score::run(&model, &audio, &config).unwrap();
        assert!(!run(settings(RATE)).detections.is_empty());
        let mut hard = settings(RATE);
        hard.detector.vad_mode = Some(VADMode::Hard);
        assert!(
            run(hard).detections.is_empty(),
            "VAD case no longer reproduces"
        );
        // The noise alone never wakes her.
        let noise = crate::wake_score::run(&model, &room(8000), &settings(RATE)).unwrap();
        assert!(noise.detections.is_empty());
    }

    /// The user's own "Hey <name>" recordings, when this PC has them (read
    /// only): each one, in quiet room noise, wakes a wake word trained on the
    /// others, with margin over the threshold.
    #[test]
    fn own_samples_wake_a_model_trained_on_the_others() {
        let Some(local) = dirs::data_local_dir() else {
            return;
        };
        let samples = samples_in(&local.join("com.sehaz.bloom").join("ai").join("wake"));
        if samples.len() <= MIN_SAMPLES {
            return;
        }
        for sample in &samples {
            let others: Vec<&PathBuf> = samples.iter().filter(|s| *s != sample).collect();
            let model = crate::wake_score::build(&others, true).unwrap();
            let bytes = std::fs::read(sample).unwrap();
            let (clip, rate) = voice::read_wav(&bytes).unwrap();
            let audio = crate::wake_score::padded(&clip, rate);
            let run = crate::wake_score::run(&model, &audio, &settings(rate)).unwrap();
            assert!(
                !run.detections.is_empty(),
                "{} did not wake",
                sample.display()
            );
            let best = crate::wake_score::best(&model, &audio, rate).unwrap().best;
            let threshold = settings(rate).detector.threshold;
            assert!(best >= threshold + 0.05, "{}: {best}", sample.display());
        }
    }

    #[test]
    fn old_models_are_rebuilt_from_trimmed_samples_once() {
        use rustpotter::WakewordRefBuildFromFiles;
        let dir = temp_dir();
        std::fs::create_dir_all(dir.join("wake")).unwrap();
        let model_file = model_path(&dir).to_string_lossy().into_owned();
        // Saved by the old enrollment: the phrase with ~1.3 s of room after it.
        let room = |ms| -> Vec<i16> { quiet(ms).iter().map(|s| s * 15).collect() };
        let files: Vec<String> = (1..=3)
            .map(|i| {
                let take = [room(150), fake_voice(i), room(1300)].concat();
                std::fs::write(sample_path(&dir, i), voice::wav(&take, RATE)).unwrap();
                sample_path(&dir, i).to_string_lossy().into_owned()
            })
            .collect();
        let old = WakewordRef::new_from_sample_files(OLD_NAME.into(), None, None, files, MFCC_SIZE)
            .unwrap();
        let old_frames = old.samples_features["sample-1.wav"].len();
        old.save_to_file(&model_file).unwrap();

        let model = load(&dir, "Mina").unwrap();
        assert_eq!(model.name, NAME);
        let frames = model.samples_features["sample-1.wav"].len();
        assert!(frames * 2 < old_frames, "{frames} vs {old_frames}");
        // Saved, so the next start loads it as it is.
        let mut saved = WakewordRef::load_from_file(&model_file).unwrap();
        assert_eq!(saved.name, NAME);

        // An old model with no samples left to rebuild from: train again.
        saved.name = OLD_NAME.into();
        saved.save_to_file(&model_file).unwrap();
        for i in 1..=3 {
            std::fs::remove_file(sample_path(&dir, i)).unwrap();
        }
        assert!(load(&dir, "Mina").is_err_and(|e| e.contains("Teach Mina")));
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
