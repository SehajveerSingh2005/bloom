//! `bloom-ai.exe --wake-score <wake dir> [wav...]`: how well recordings match
//! a trained wake word. Each WAV (default: the folder's `sample-*.wav`) gets
//! quiet room noise before and after and goes through the same detector
//! settings and frame feeding as the live listener, under a few variations.
//! It only reads files and prints a table.

use crate::voice::read_wav;
use crate::wake;
use rustpotter::{RustpotterConfig, VADMode, WakewordLoad, WakewordRef, WakewordSave};
use std::fmt::Write;
use std::path::{Path, PathBuf};

const LEAD_MS: usize = 1000;
/// Rustpotter confirms a detection up to about half the model's length after
/// the phrase, so the tail is longer than the lead.
const TAIL_MS: usize = 3000;
/// The quiet before and after: noise about as loud as a quiet room on a
/// laptop mic (RMS ~170, -45 dBFS), not digital silence.
const ROOM_NOISE: i32 = 300;
const THRESHOLDS: [f32; 4] = [0.5, 0.45, 0.4, 0.35];
const AVG_THRESHOLDS: [f32; 2] = [0.2, 0.1];
const VADS: [(&str, Option<VADMode>); 3] = [
    ("off", None),
    ("easy", Some(VADMode::Easy)),
    ("hard", Some(VADMode::Hard)),
];

/// `clip` with quiet noise before and after, as the live mic would hear it.
pub(crate) fn padded(clip: &[i16], rate: u32) -> Vec<i16> {
    let mut seed = 1u32;
    let mut noise = |ms: usize| -> Vec<i16> {
        (0..rate as usize * ms / 1000)
            .map(|_| {
                seed = seed.wrapping_mul(1_103_515_245).wrapping_add(12_345);
                ((seed >> 16) as i32 % (2 * ROOM_NOISE) - ROOM_NOISE) as i16
            })
            .collect()
    };
    [noise(LEAD_MS), clip.to_vec(), noise(TAIL_MS)].concat()
}

/// Runs `audio` through a detector in 30 ms chunks like the microphone
/// delivers. Returns the detections' scores and the best partial score
/// (with its averaged-template score) seen at any point.
pub(crate) fn run(model: &[u8], audio: &[i16], config: &RustpotterConfig) -> Result<Run, String> {
    let model = WakewordRef::load_from_buffer(model)?;
    let mut detector = wake::detector_with(model, config)?;
    let mut frame = Vec::new();
    let mut out = Run::default();
    for chunk in audio.chunks(config.fmt.sample_rate * 30 / 1000) {
        if let Some(d) = wake::feed(&mut detector, &mut frame, chunk) {
            out.detections.push(d.score);
            out.best = out.best.max(d.score);
        }
        if let Some(d) = detector.get_partial_detection() {
            if d.score > out.best {
                (out.best, out.best_avg) = (d.score, d.avg_score);
            }
        }
    }
    Ok(out)
}

#[derive(Default)]
pub(crate) struct Run {
    pub detections: Vec<f32>,
    pub best: f32,
    pub best_avg: f32,
}

/// The best score anywhere in `audio`, with any threshold or VAD.
pub(crate) fn best(model: &[u8], audio: &[i16], rate: u32) -> Result<Run, String> {
    let mut raw = wake::settings(rate);
    raw.detector.vad_mode = None;
    // Every frame scores, so the partial detection holds the best score.
    raw.detector.threshold = 0.0;
    raw.detector.avg_threshold = f32::MIN_POSITIVE;
    run(model, audio, &raw)
}

/// One line per model: the best raw score, then D (detected) or . for each
/// VAD mode x avg threshold x threshold.
fn score_line(model: &[u8], audio: &[i16], rate: u32) -> Result<String, String> {
    let best = best(model, audio, rate)?;
    // One core's share spent on the live settings (meaningful in a release build).
    let started = std::time::Instant::now();
    run(model, audio, &wake::settings(rate))?;
    let cpu = started.elapsed().as_secs_f32() * rate as f32 / audio.len() as f32 * 100.0;
    let mut line = format!(
        "best {:.3} (avg {:.3}) cpu {cpu:.1}% |",
        best.best, best.best_avg
    );
    for (vad_name, vad) in VADS {
        let _ = write!(line, " {vad_name:>4} ");
        for avg in AVG_THRESHOLDS {
            for threshold in THRESHOLDS {
                let mut config = wake::settings(rate);
                config.detector.vad_mode = vad;
                config.detector.threshold = threshold;
                config.detector.avg_threshold = avg;
                let hit = !run(model, audio, &config)?.detections.is_empty();
                line.push(if hit { 'D' } else { '.' });
            }
            line.push(' ');
        }
        line.push('|');
    }
    Ok(line)
}

/// A wake word built in memory from `files`, each trimmed first if `retrim`.
pub(crate) fn build(files: &[&PathBuf], retrim: bool) -> Result<Vec<u8>, String> {
    wake::train(files, retrim)?.save_to_buffer()
}

/// The report for `wavs` (or the folder's samples) against the folder's
/// `wake.rpw`, and against models rebuilt from the other samples in the
/// folder (a recording never scores against itself there).
pub fn report(wake_dir: &Path, wavs: &[PathBuf]) -> Result<String, String> {
    let model = std::fs::read(wake_dir.join("wake.rpw"))
        .map_err(|e| format!("{}: {e}", wake_dir.join("wake.rpw").display()))?;
    let samples = wake::samples_in(wake_dir);
    let wavs = if wavs.is_empty() { &samples } else { wavs };
    let mut out = String::new();
    let marks = THRESHOLDS.map(|t| format!("{t}")).join(",");
    let live = wake::settings(16_000).detector;
    let _ = writeln!(
        out,
        "Columns per VAD mode: avg threshold {AVG_THRESHOLDS:?}, each with thresholds {marks}. D = detected.\n\
         Live settings: VAD {:?}, avg threshold {}, threshold {}. cpu = one core's share on live settings.\n\
         held out = built from the folder's other samples; retrim = trimmed first, as enrollment and the old-model rebuild do now.",
        live.vad_mode, live.avg_threshold, live.threshold
    );
    for wav in wavs {
        let bytes = std::fs::read(wav).map_err(|e| format!("{}: {e}", wav.display()))?;
        let (clip, rate) = read_wav(&bytes).map_err(|e| format!("{}: {e}", wav.display()))?;
        let audio = padded(&clip, rate);
        let trimmed = wake::trim(&clip, rate).map_or(0, <[i16]>::len);
        let _ = writeln!(
            out,
            "\n{} ({rate} Hz, {:.2} s, trims to {:.2} s)",
            wav.display(),
            clip.len() as f32 / rate as f32,
            trimmed as f32 / rate as f32
        );
        let _ = writeln!(
            out,
            "  wake.rpw            {}",
            score_line(&model, &audio, rate)?
        );
        let others: Vec<&PathBuf> = samples
            .iter()
            .filter(|s| s.file_name() != wav.file_name())
            .collect();
        if others.len() >= 2 {
            for (label, retrim) in [("held out", false), ("held out, retrim", true)] {
                let held = build(&others, retrim)?;
                let _ = writeln!(out, "  {label:<19} {}", score_line(&held, &audio, rate)?);
            }
        }
    }
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pads_with_quiet_noise() {
        let audio = padded(&[9000; 480], 48_000);
        assert_eq!(audio.len(), 48 * (LEAD_MS + TAIL_MS) + 480);
        let lead = &audio[..48_000];
        assert!(lead.iter().all(|s| s.abs() <= ROOM_NOISE as i16));
        assert!(lead.iter().any(|&s| s != 0));
    }
}
