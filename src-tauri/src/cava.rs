//! The audio visualizer's analysis, after cava (github.com/karlstav/cava),
//! the engine behind Kurve (plasma-audio-visualizer): a windowed FFT split
//! into log-spaced bands from 50 Hz to 10 kHz, then cava's smoothing, a
//! gravity fall-off with an integral filter ("noise reduction"), and its
//! automatic sensitivity, which keeps the loudest band just under 1.

use rustfft::num_complex::Complex;
use rustfft::{Fft, FftPlanner};
use std::sync::Arc;

pub const BANDS: usize = 24;
const FFT_SIZE: usize = 2048;
const LOW_HZ: f32 = 50.0;
const HIGH_HZ: f32 = 10_000.0;
/// cava's default.
const NOISE_REDUCTION: f32 = 0.77;

pub struct Cava {
    fft: Arc<dyn Fft<f32>>,
    window: Vec<f32>,
    /// The last FFT_SIZE mono samples, oldest first.
    history: Vec<f32>,
    buf: Vec<Complex<f32>>,
    /// Each band's FFT bins, [lo, hi).
    bins: Vec<(usize, usize)>,
    /// Per-band weight that evens out music's falling high end.
    eq: Vec<f32>,
    sens: f32,
    /// Frames the sensitivity has been starting up (it climbs fast at first).
    warming: bool,
    prev: Vec<f32>,
    peak: Vec<f32>,
    fall: Vec<f32>,
    mem: Vec<f32>,
    gravity: f32,
}

impl Cava {
    /// `rate`: the capture's sample rate; `fps`: how often frame() runs.
    pub fn new(rate: u32, fps: f32) -> Self {
        let fft = FftPlanner::new().plan_fft_forward(FFT_SIZE);
        let window = (0..FFT_SIZE)
            .map(|i| 0.5 * (1.0 - (2.0 * std::f32::consts::PI * i as f32 / (FFT_SIZE - 1) as f32).cos()))
            .collect();
        let hz_per_bin = rate as f32 / FFT_SIZE as f32;
        let top = FFT_SIZE / 2;
        let mut bins = Vec::with_capacity(BANDS);
        let mut eq = Vec::with_capacity(BANDS);
        let mut lo = ((LOW_HZ / hz_per_bin) as usize).max(1);
        for n in 0..BANDS {
            let edge = LOW_HZ * (HIGH_HZ / LOW_HZ).powf((n + 1) as f32 / BANDS as f32);
            // Every band gets at least one bin of its own, like cava's
            // cut-off adjustment for the crowded low end.
            let hi = ((edge / hz_per_bin) as usize).max(lo + 1).min(top);
            let lo_now = lo.min(hi - 1);
            bins.push((lo_now, hi));
            let centre = (lo_now + hi) as f32 / 2.0 * hz_per_bin;
            eq.push(centre.powf(0.6) / (hi - lo_now) as f32);
            lo = hi;
        }
        Self {
            fft,
            window,
            history: vec![0.0; FFT_SIZE],
            buf: vec![Complex::new(0.0, 0.0); FFT_SIZE],
            bins,
            eq,
            sens: 1.0,
            warming: true,
            prev: vec![0.0; BANDS],
            peak: vec![0.0; BANDS],
            fall: vec![0.0; BANDS],
            mem: vec![0.0; BANDS],
            gravity: (60.0 / fps).powf(2.5) * 1.54 / NOISE_REDUCTION,
        }
    }

    /// Adds newly captured mono samples (any amount).
    pub fn push(&mut self, samples: &[f32]) {
        if samples.len() >= FFT_SIZE {
            self.history.copy_from_slice(&samples[samples.len() - FFT_SIZE..]);
        } else {
            self.history.drain(..samples.len());
            self.history.extend_from_slice(samples);
        }
    }

    /// One frame of BANDS values in 0..=1.
    pub fn frame(&mut self) -> Vec<f32> {
        for (i, c) in self.buf.iter_mut().enumerate() {
            *c = Complex::new(self.history[i] * self.window[i], 0.0);
        }
        self.fft.process(&mut self.buf);
        let silent = self.history.iter().all(|s| s.abs() < 1e-5);

        let mut out = vec![0.0f32; BANDS];
        let mut overshoot = false;
        for n in 0..BANDS {
            let (lo, hi) = self.bins[n];
            let sum: f32 = self.buf[lo..hi].iter().map(|c| c.norm()).sum();
            let mut v = sum * self.eq[n] * self.sens / 2000.0;
            // Gravity: a falling band drops along a parabola from its peak.
            if v < self.prev[n] {
                v = (self.peak[n] * (1.0 - self.fall[n] * self.fall[n] * self.gravity)).max(0.0);
                self.fall[n] += 0.028;
            } else {
                self.peak[n] = v;
                self.fall[n] = 0.0;
            }
            self.prev[n] = v;
            // Integral filter: steadies the bands.
            v += self.mem[n] * NOISE_REDUCTION;
            self.mem[n] = v;
            if v > 1.0 {
                overshoot = true;
                v = 1.0;
            }
            out[n] = v;
        }
        // Autosens: back off on overshoot, creep up otherwise (fast at start).
        if overshoot {
            self.sens *= 0.98;
            self.warming = false;
        } else if !silent {
            self.sens *= if self.warming { 1.1 } else { 1.001 };
        }
        self.sens = self.sens.clamp(1e-4, 1e4);
        out
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tone(hz: f32, rate: u32, len: usize) -> Vec<f32> {
        (0..len)
            .map(|i| (2.0 * std::f32::consts::PI * hz * i as f32 / rate as f32).sin() * 0.5)
            .collect()
    }

    #[test]
    fn bands_are_ordered_and_non_empty() {
        let c = Cava::new(48_000, 40.0);
        assert_eq!(c.bins.len(), BANDS);
        for w in c.bins.windows(2) {
            assert!(w[0].0 < w[0].1 && w[0].1 <= w[1].0);
        }
    }

    #[test]
    fn a_tone_lights_its_own_band_most() {
        let mut c = Cava::new(48_000, 40.0);
        let samples = tone(1000.0, 48_000, FFT_SIZE);
        let mut out = vec![];
        // Long enough for autosens to settle the loudest band just under 1.
        for _ in 0..300 {
            c.push(&samples);
            out = c.frame();
        }
        let loudest = out.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        let (lo, hi) = c.bins[loudest];
        let hz = 48_000.0 / FFT_SIZE as f32;
        assert!(lo as f32 * hz <= 1050.0 && hi as f32 * hz >= 950.0, "band {loudest}");
        assert!(out[loudest] > 0.5);
    }

    #[test]
    fn silence_falls_to_zero() {
        let mut c = Cava::new(48_000, 40.0);
        let samples = tone(200.0, 48_000, FFT_SIZE);
        for _ in 0..30 {
            c.push(&samples);
            c.frame();
        }
        let quiet = vec![0.0; FFT_SIZE];
        let mut out = vec![];
        for _ in 0..120 {
            c.push(&quiet);
            out = c.frame();
        }
        assert!(out.iter().all(|v| *v < 0.01), "{out:?}");
    }
}
