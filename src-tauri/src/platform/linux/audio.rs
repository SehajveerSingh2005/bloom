//! Audio output control and capture for the visualizer.
//!
//! Mint ships PipeWire (with WirePlumber) or PulseAudio. Neither libpipewire nor
//! libpulse development headers can be assumed, so PipeWire is reached through
//! `wpctl` and the PulseAudio compatibility layer through `pactl`. These are
//! short-lived, one-shot calls made on user actions or on a slow poll, never in
//! the visualizer's hot path.
//!
//! Capture for the visualizer uses `pw-record` streaming from the default sink's
//! monitor. `parec` is kept as a fallback, but it is known to deliver no audio on
//! some PipeWire setups, so it is only tried when `pw-record` is missing.

use crate::{
    platform::linux::find_in_path,
    state::ANY_MEDIA_PLAYING,
    types::AudioVisualizationData,
};
use std::{
    io::{BufRead, BufReader, Read},
    path::PathBuf,
    process::{Child, Command, Stdio},
    sync::{atomic::Ordering, mpsc, OnceLock},
    time::{Duration, Instant},
};
use tauri::{AppHandle, Emitter};

const DEFAULT_SINK: &str = "@DEFAULT_AUDIO_SINK@";
const DEFAULT_SINK_PULSE: &str = "@DEFAULT_SINK@";

const SAMPLE_RATE: u32 = 44_100;
const FFT_SIZE: usize = 512;
const BAND_COUNT: usize = 5;
/// Frequency buckets in FFT bins, matching the Windows backend so the bars look
/// identical on both platforms.
const BAND_RANGES: [(usize, usize); BAND_COUNT] = [(1, 2), (2, 6), (6, 18), (18, 60), (60, 200)];
const BAND_WEIGHTING: [f32; BAND_COUNT] = [1.2, 1.2, 1.5, 2.8, 5.0];

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SinkState {
    /// The sink's real level, where 1.0 is unity gain.
    ///
    /// PipeWire and PulseAudio both allow amplification above 1.0 and that is
    /// common on laptops, so this is deliberately not clamped. Clamping here
    /// would make every adjustment above 100% look like no change at all, and
    /// the watcher below would then never report it.
    pub volume: f32,
    pub muted: bool,
}

/// The level Bloom's interface expresses, which is the 0-1 range the Windows
/// backend uses and the range the frontend's volume slider offers.
///
/// Levels above unity gain are still *reported* through this, so the interface
/// stays within the contract it was written for, while change detection runs on
/// the unclamped value.
pub fn ui_level(volume: f32) -> f32 {
    volume.clamp(0.0, 1.0)
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Backend {
    WirePlumber,
    PulseAudio,
}

fn backend() -> Option<Backend> {
    static BACKEND: OnceLock<Option<Backend>> = OnceLock::new();
    *BACKEND.get_or_init(|| {
        if find_in_path("wpctl").is_some() {
            Some(Backend::WirePlumber)
        } else if find_in_path("pactl").is_some() {
            Some(Backend::PulseAudio)
        } else {
            None
        }
    })
}

fn run(program: &PathBuf, args: &[&str]) -> Result<String, String> {
    let output = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .output()
        .map_err(|e| format!("Could not run {}: {e}", program.display()))?;
    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_owned();
        return Err(if stderr.is_empty() {
            format!("{} failed", program.display())
        } else {
            stderr
        });
    }
    Ok(String::from_utf8_lossy(&output.stdout).into_owned())
}

/// `wpctl get-volume` prints `Volume: 0.42` with `[MUTED]` appended when muted.
/// The value can exceed 1.0 when overamplification is enabled.
fn parse_wpctl_volume(text: &str) -> Option<(f32, bool)> {
    let rest = text.split_once("Volume:")?.1.trim();
    let value = rest.split_whitespace().next()?.parse::<f32>().ok()?;
    (value.is_finite() && value >= 0.0).then(|| (value, text.contains("[MUTED]")))
}

/// `pactl get-sink-volume` prints several channels; the first percentage wins.
fn parse_pactl_volume(text: &str) -> Option<f32> {
    let after_label = text.split_once("Volume:")?.1;
    let percent = after_label.split('%').next()?;
    let digits = percent.rsplit(|c: char| !c.is_ascii_digit()).next()?;
    let value = digits.parse::<f32>().ok()? / 100.0;
    value.is_finite().then_some(value)
}

fn parse_pactl_mute(text: &str) -> Option<bool> {
    let rest = text.split_once("Mute:")?.1.trim().to_ascii_lowercase();
    Some(rest.starts_with("yes"))
}

pub fn state() -> Result<SinkState, String> {
    match backend() {
        Some(Backend::WirePlumber) => {
            let program = find_in_path("wpctl").ok_or("wpctl is not available")?;
            let volume = run(&program, &["get-volume", DEFAULT_SINK])?;
            let (volume, muted) =
                parse_wpctl_volume(&volume).ok_or("Could not read the WirePlumber volume")?;
            Ok(SinkState { volume, muted })
        }
        Some(Backend::PulseAudio) => {
            let program = find_in_path("pactl").ok_or("pactl is not available")?;
            let volume = run(&program, &["get-sink-volume", DEFAULT_SINK_PULSE])?;
            let muted = run(&program, &["get-sink-mute", DEFAULT_SINK_PULSE])?;
            Ok(SinkState {
                volume: parse_pactl_volume(&volume).ok_or("Could not read the sink volume")?,
                muted: parse_pactl_mute(&muted).unwrap_or(false),
            })
        }
        None => Err("No supported audio controller is available".into()),
    }
}

pub fn set_volume(volume: f32) -> Result<(), String> {
    // Bloom's slider offers 0-1, and nothing here should amplify past unity
    // gain on the user's behalf.
    let volume = ui_level(volume);
    match backend() {
        Some(Backend::WirePlumber) => {
            let program = find_in_path("wpctl").ok_or("wpctl is not available")?;
            run(&program, &["set-volume", DEFAULT_SINK, &format!("{volume:.2}")])?;
            // Raising the volume is expected to un-mute, like the Windows backend.
            if volume > 0.0 {
                let _ = run(&program, &["set-mute", DEFAULT_SINK, "0"]);
            }
            Ok(())
        }
        Some(Backend::PulseAudio) => {
            let program = find_in_path("pactl").ok_or("pactl is not available")?;
            let percent = (volume * 100.0).round() as u32;
            run(&program, &["set-sink-volume", DEFAULT_SINK_PULSE, &format!("{percent}%")])?;
            if volume > 0.0 {
                let _ = run(&program, &["set-sink-mute", DEFAULT_SINK_PULSE, "0"]);
            }
            Ok(())
        }
        None => Err("No supported audio controller is available".into()),
    }
}

/// How long to wait between checks when no event source is available. Without
/// `pactl subscribe` this is the only way to notice a change, and it is why the
/// on-screen display used to lag behind a volume key by up to a second.
const IDLE_POLL: Duration = Duration::from_millis(1000);

/// Re-reads the level shortly after an event, because the event can arrive
/// marginally before the new value is readable. Without this the update would
/// not appear until the next idle poll.
const SETTLE_DELAY: Duration = Duration::from_millis(120);

/// Report the current sink state when it differs from the last one reported.
fn publish(app: &AppHandle, previous: &mut Option<SinkState>) {
    let Ok(current) = state() else {
        return;
    };
    crate::state::CURRENT_VOLUME.store((ui_level(current.volume) * 100.0) as u32, Ordering::Relaxed);
    // Compared unclamped, so an adjustment that stays above 100% is still
    // reported to the interface.
    if *previous == Some(current) {
        return;
    }
    *previous = Some(current);
    let _ = app.emit(
        "volume-change",
        crate::types::VolumeChangeEvent {
            volume: current.volume,
            is_muted: current.muted,
        },
    );
}

/// Watch for changes to the default sink and report them.
///
/// The trigger is `pactl subscribe`, which streams an event the moment a sink
/// changes, so Bloom's on-screen display reacts to a volume key immediately
/// rather than on the next poll. Where the PulseAudio compatibility layer is
/// unavailable, a one-second poll is used instead.
pub fn start_volume_watcher(app: AppHandle) {
    std::thread::spawn(move || {
        let mut previous: Option<SinkState> = None;
        let subscription = start_sink_subscription();
        match subscription {
            Some(events) => loop {
                match events.recv_timeout(IDLE_POLL) {
                    Ok(()) => {
                        publish(&app, &mut previous);
                        std::thread::sleep(SETTLE_DELAY);
                        publish(&app, &mut previous);
                    }
                    Err(mpsc::RecvTimeoutError::Timeout) => publish(&app, &mut previous),
                    // The event source went away, so finish out the process with
                    // polling rather than going blind.
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            },
            None => {}
        }
        loop {
            publish(&app, &mut previous);
            std::thread::sleep(IDLE_POLL);
        }
    });
}

/// Stream sink changes from `pactl subscribe`, or `None` when that is not
/// available on this system.
fn start_sink_subscription() -> Option<mpsc::Receiver<()>> {
    let program = find_in_path("pactl")?;
    let (sender, receiver) = mpsc::channel();
    std::thread::spawn(move || {
        let mut quick_failures = 0;
        loop {
            let Ok(mut child) = Command::new(&program)
                .arg("subscribe")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
            else {
                // Could not start it at all: dropping the sender makes the
                // watcher fall back to polling.
                return;
            };
            let started = Instant::now();
            if let Some(stdout) = child.stdout.take() {
                for line in BufReader::new(stdout).lines().map_while(Result::ok) {
                    if line.contains("sink") || line.contains("server") {
                        // A closed receiver means the watcher has stopped.
                        if sender.send(()).is_err() {
                            let _ = child.kill();
                            return;
                        }
                    }
                }
            }
            let _ = child.wait();
            // An immediate exit means this system cannot subscribe (for example
            // PipeWire without its PulseAudio layer), so stop trying.
            if started.elapsed() < Duration::from_secs(1) {
                quick_failures += 1;
                if quick_failures >= 2 {
                    return;
                }
            } else {
                quick_failures = 0;
            }
            // Avoid a tight respawn loop after a PulseAudio restart.
            std::thread::sleep(Duration::from_secs(2));
        }
    });
    Some(receiver)
}

/// Sliding-window FFT that turns a stream of mono samples into the five bands
/// the visualizer renders. Kept free of I/O so it can be tested directly.
struct BandAnalyzer {
    fft: std::sync::Arc<dyn rustfft::Fft<f32>>,
    spectrum: Vec<rustfft::num_complex::Complex<f32>>,
    windowed: Vec<f32>,
    position: usize,
    max_energies: [f32; BAND_COUNT],
    previous: [f32; BAND_COUNT],
}

impl BandAnalyzer {
    fn new() -> Self {
        let mut planner = rustfft::FftPlanner::<f32>::new();
        Self {
            fft: planner.plan_fft_forward(FFT_SIZE),
            spectrum: vec![rustfft::num_complex::Complex::new(0.0, 0.0); FFT_SIZE],
            windowed: vec![0.0; FFT_SIZE],
            position: 0,
            max_energies: [0.01; BAND_COUNT],
            previous: [0.1; BAND_COUNT],
        }
    }

    /// Feed one normalized sample. Returns band magnitudes once a full window
    /// has been collected.
    fn push(&mut self, sample: f32) -> Option<[f32; BAND_COUNT]> {
        // Hann window, matching the Windows implementation.
        let window = 0.5
            * (1.0
                - (2.0 * std::f32::consts::PI * self.position as f32 / FFT_SIZE as f32).cos());
        self.windowed[self.position] = sample * window;
        self.position += 1;
        if self.position < FFT_SIZE {
            return None;
        }
        self.position = 0;
        Some(self.analyze())
    }

    fn analyze(&mut self) -> [f32; BAND_COUNT] {
        for (index, sample) in self.windowed.iter().enumerate() {
            self.spectrum[index] = rustfft::num_complex::Complex::new(*sample, 0.0);
        }
        self.fft.process(&mut self.spectrum);

        let mut output = [0.0f32; BAND_COUNT];
        for (band, (start, end)) in BAND_RANGES.iter().enumerate() {
            let mut total = 0.0f32;
            let mut count = 0u32;
            for bin in *start..*end {
                if bin >= FFT_SIZE / 2 {
                    break;
                }
                total += self.spectrum[bin].norm();
                count += 1;
            }
            let average = total / count.max(1) as f32 * BAND_WEIGHTING[band];

            // Auto-gain: track the loudest this band has recently been, and let
            // it decay so quiet passages still render.
            if average > self.max_energies[band] {
                self.max_energies[band] = average;
            } else {
                self.max_energies[band] *= 0.99;
            }
            let target = (average / self.max_energies[band].max(0.12))
                .min(1.0)
                .powf(0.75);
            let smooth = if target > self.previous[band] { 0.10 } else { 0.20 };
            let value =
                (self.previous[band] * smooth + target * (1.0 - smooth)).clamp(0.18, 1.0);
            self.previous[band] = value;
            output[band] = value;
        }
        output
    }
}

/// Kills the capture process even if the reader loop exits early.
struct Capture(Child);

impl Drop for Capture {
    fn drop(&mut self) {
        let _ = self.0.kill();
        let _ = self.0.wait();
    }
}

/// The argument list for the best available capture tool.
fn capture_command() -> Option<(PathBuf, Vec<String>)> {
    if let Some(program) = find_in_path("pw-record") {
        return Some((
            program,
            vec![
                "--format=s16".into(),
                format!("--rate={SAMPLE_RATE}"),
                "--channels=1".into(),
                // Captures the default sink's monitor without naming a device.
                // Kept as a separate argument pair, which is the verified form.
                "-P".into(),
                "stream.capture.sink=true".into(),
                "-".into(),
            ],
        ));
    }
    if let Some(program) = find_in_path("parec") {
        return Some((
            program,
            vec![
                "--format=s16le".into(),
                format!("--rate={SAMPLE_RATE}"),
                "--channels=1".into(),
                "--device=@DEFAULT_MONITOR@".into(),
            ],
        ));
    }
    None
}

/// Stream the default sink's monitor and emit the visualizer bands until the
/// capture ends or playback stops.
fn capture_once(app: &AppHandle, program: &PathBuf, args: &[String]) -> Result<(), String> {
    let mut child = Command::new(program)
        .args(args)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .map_err(|e| format!("Could not start {}: {e}", program.display()))?;
    let stdout = child.stdout.take().ok_or("Capture produced no output stream")?;
    let _guard = Capture(child);

    let mut analyzer = BandAnalyzer::new();
    let mut reader = std::io::BufReader::with_capacity(64 * 1024, stdout);
    let mut frame = [0u8; 1024]; // 512 mono s16 samples
    loop {
        if !ANY_MEDIA_PLAYING.load(Ordering::Relaxed) {
            return Ok(());
        }
        // `read_exact` gathers a whole window, so partial reads cannot desync the
        // sample stream.
        reader
            .read_exact(&mut frame)
            .map_err(|e| format!("Audio capture ended: {e}"))?;
        for chunk in frame.chunks_exact(2) {
            let sample = i16::from_ne_bytes([chunk[0], chunk[1]]) as f32 / 32768.0;
            if let Some(bands) = analyzer.push(sample) {
                let _ = app.emit(
                    "audio-visualization",
                    AudioVisualizationData {
                        frequencies: bands.to_vec(),
                    },
                );
            }
        }
    }
}

/// Only capture while something is playing, so an idle desktop costs nothing.
pub fn start_visualizer(app: AppHandle) {
    std::thread::spawn(move || {
        let Some((program, args)) = capture_command() else {
            eprintln!(
                "Bloom's audio visualizer is unavailable: install pw-record (PipeWire) or parec (PulseAudio)"
            );
            return;
        };
        loop {
            if !ANY_MEDIA_PLAYING.load(Ordering::Relaxed) {
                std::thread::sleep(Duration::from_millis(500));
                continue;
            }
            if let Err(error) = capture_once(&app, &program, &args) {
                eprintln!("Bloom audio capture stopped: {error}");
                std::thread::sleep(Duration::from_secs(2));
            }
        }
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Laptops commonly run above unity gain, and the watcher only reports a
    /// change when this differs. Clamping here is what previously made every
    /// adjustment above 100% invisible, so the interface never reacted.
    #[test]
    fn adjustments_above_unity_gain_are_distinguishable() {
        let quiet = parse_wpctl_volume("Volume: 1.20").unwrap().0;
        let louder = parse_wpctl_volume("Volume: 1.25").unwrap().0;
        assert_ne!(quiet, louder);
        assert!(louder > quiet);
    }

    #[test]
    fn the_reported_level_stays_within_the_interfaces_range() {
        assert_eq!(ui_level(1.2), 1.0);
        assert_eq!(ui_level(1.0), 1.0);
        assert_eq!(ui_level(0.5), 0.5);
        assert_eq!(ui_level(-1.0), 0.0);
    }

    #[test]
    fn pulse_volumes_above_full_scale_survive_parsing() {
        assert_eq!(parse_pactl_volume("Volume: Front Left: 120%").unwrap(), 1.2);
    }

    #[test]
    fn parses_wireplumber_volume_output() {
        assert_eq!(parse_wpctl_volume("Volume: 0.42\n"), Some((0.42, false)));
        // Amplification is reported as-is; the interface's own range is applied
        // when the level is handed to the frontend, not here.
        assert_eq!(
            parse_wpctl_volume("Volume: 1.50 [MUTED]\n"),
            Some((1.5, true))
        );
        assert_eq!(parse_wpctl_volume("Volume: 1.20"), Some((1.2, false)));
        assert_eq!(parse_wpctl_volume("garbage"), None);
        assert_eq!(parse_wpctl_volume("Volume: nan"), None);
    }

    #[test]
    fn parses_pulseaudio_volume_output() {
        let text = "Volume: front-left: 65536 / 100% / 0.00 dB,   front-right: 32768 / 50% / -18.06 dB\n";
        assert_eq!(parse_pactl_volume(text), Some(1.0));
        let half = "Volume: front-left: 32768 /  50% / -18.06 dB,   front-right: 32768 /  50% / -18.06 dB\n";
        assert_eq!(parse_pactl_volume(half), Some(0.5));
        assert_eq!(parse_pactl_volume("no volume here"), None);
    }

    #[test]
    fn parses_pulseaudio_mute_output() {
        assert_eq!(parse_pactl_mute("Mute: yes\n"), Some(true));
        assert_eq!(parse_pactl_mute("Mute: no\n"), Some(false));
        assert_eq!(parse_pactl_mute("nope"), None);
    }

    /// Run a steady tone through the analyzer and return the settled bands.
    fn bands_for_tone(frequency: f32) -> [f32; BAND_COUNT] {
        let mut analyzer = BandAnalyzer::new();
        let mut bands = [0.0f32; BAND_COUNT];
        for index in 0..(FFT_SIZE * 12) {
            let phase =
                2.0 * std::f32::consts::PI * frequency * index as f32 / SAMPLE_RATE as f32;
            if let Some(output) = analyzer.push(phase.sin()) {
                bands = output;
            }
        }
        bands
    }

    #[test]
    fn tone_energy_lands_in_the_expected_bands() {
        // 440 Hz sits around bin 5, so the three low bands carry energy and the
        // two high bands (1.5 kHz and up) must stay flat. Each band normalizes
        // against its own recent maximum, which is why active bands all read
        // near full rather than scaling with absolute loudness.
        let low = bands_for_tone(440.0);
        assert!(low[1] > 0.5, "expected mid-low energy, got {low:?}");
        assert_eq!(low[3], 0.18, "no energy should reach 1.5-5 kHz: {low:?}");
        assert_eq!(low[4], 0.18, "no energy should reach 5 kHz+: {low:?}");

        // 10 kHz sits around bin 116, inside the highest band.
        let high = bands_for_tone(10_000.0);
        assert!(high[4] > 0.5, "expected high energy, got {high:?}");
        assert_eq!(high[0], 0.18, "no energy should reach 86-172 Hz: {high:?}");

        for bands in [low, high] {
            assert!(bands.iter().all(|value| (0.18..=1.0).contains(value)));
        }
    }

    #[test]
    fn silence_keeps_the_bars_at_their_floor() {
        let mut analyzer = BandAnalyzer::new();
        let mut bands = [0.0f32; BAND_COUNT];
        for _ in 0..(FFT_SIZE * 8) {
            if let Some(output) = analyzer.push(0.0) {
                bands = output;
            }
        }
        assert!(bands.iter().all(|value| *value <= 0.19), "bands were {bands:?}");
    }

    #[test]
    fn capture_never_panics_when_audio_tools_are_absent() {
        // Either a tool is found or the visualizer reports itself unavailable;
        // in both cases this must not panic.
        let _ = capture_command();
    }
}
