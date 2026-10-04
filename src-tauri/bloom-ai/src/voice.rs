//! Push-to-talk: record the default microphone while the hotkey is held, then
//! send the clip to the transcription endpoint. The mic is open only between
//! record_start and record_stop; nothing runs otherwise.

use crate::agent::Shared;
use crate::config::Config;
use crate::secrets;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

/// Longest clip: a backstop if the key-up never arrives.
const MAX_SECONDS: usize = 120;

pub struct Recorder {
    stop: Arc<AtomicBool>,
    thread: std::thread::JoinHandle<Result<(Vec<i16>, u32), String>>,
}

pub fn start() -> Recorder {
    let stop = Arc::new(AtomicBool::new(false));
    let flag = stop.clone();
    Recorder {
        stop,
        thread: std::thread::spawn(move || capture(&flag)),
    }
}

impl Recorder {
    /// Blocking: stops recording and returns mono 16-bit samples and their rate.
    pub fn finish(self) -> Result<(Vec<i16>, u32), String> {
        self.stop.store(true, Ordering::Relaxed);
        self.thread
            .join()
            .map_err(|_| "The recorder crashed.".to_string())?
    }
}

/// Stops the recorder, transcribes the clip and returns the text.
pub async fn listen(recorder: Recorder, shared: &Shared) -> Result<String, String> {
    let (samples, rate) = tokio::task::spawn_blocking(move || recorder.finish())
        .await
        .map_err(|e| e.to_string())??;
    if samples.len() < rate as usize * 3 / 10 {
        return Err("Didn't catch that. Hold the key while you speak.".into());
    }
    to_text(&samples, rate, shared).await
}

/// Sends a clip to the transcription endpoint.
pub async fn to_text(samples: &[i16], rate: u32, shared: &Shared) -> Result<String, String> {
    let cfg = Config::load(&shared.settings_path);
    let key = secrets::get("stt-key")
        .or_else(|| secrets::get("llm-key"))
        .unwrap_or_default();
    let text = transcribe(
        &shared.http,
        &cfg.stt_url,
        &cfg.stt_model,
        &key,
        wav(samples, rate),
    )
    .await?;
    if text.is_empty() {
        Err("Didn't catch that.".into())
    } else {
        Ok(text)
    }
}

fn capture(stop: &AtomicBool) -> Result<(Vec<i16>, u32), String> {
    let mut mono: Vec<i16> = Vec::new();
    let rate = stream(|rate, chunk| {
        mono.extend_from_slice(chunk);
        !stop.load(Ordering::Relaxed) && mono.len() < rate as usize * MAX_SECONDS
    })?;
    Ok((mono, rate))
}

/// Opens the default microphone and hands `feed` the mono 16-bit samples
/// captured since the last call (often none), about every 30 ms, until it
/// returns false. Returns the sample rate.
#[cfg(windows)]
pub fn stream(mut feed: impl FnMut(u32, &[i16]) -> bool) -> Result<u32, String> {
    use std::time::Duration;
    use windows::Win32::Media::Audio::{
        eCapture, eConsole, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator,
        MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED,
    };
    use windows::Win32::System::Com::{
        CoCreateInstance, CoInitializeEx, CoTaskMemFree, CLSCTX_ALL, COINIT_MULTITHREADED,
    };

    let err = |what: &str, e: windows::core::Error| format!("Microphone error ({what}): {e}");
    unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let enumerator: IMMDeviceEnumerator =
            CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)
                .map_err(|e| err("devices", e))?;
        let device = enumerator
            .GetDefaultAudioEndpoint(eCapture, eConsole)
            .map_err(|_| "No microphone found.".to_string())?;
        let client: IAudioClient = device
            .Activate(CLSCTX_ALL, None)
            .map_err(|e| err("activate", e))?;
        let format = client.GetMixFormat().map_err(|e| err("format", e))?;
        let channels = (*format).nChannels as usize;
        let rate = (*format).nSamplesPerSec;
        let bits = (*format).wBitsPerSample;
        let init = client.Initialize(
            AUDCLNT_SHAREMODE_SHARED,
            0,
            10_000_000,
            0,
            format,
            Some(std::ptr::null()),
        );
        CoTaskMemFree(Some(format as *const _));
        init.map_err(|e| err("init", e))?;
        let capture: IAudioCaptureClient = client.GetService().map_err(|e| err("service", e))?;
        client.Start().map_err(|e| err("start", e))?;

        let frame_bytes = channels * (bits as usize / 8);
        let mut mono: Vec<i16> = Vec::new();
        while feed(rate, &mono) {
            mono.clear();
            std::thread::sleep(Duration::from_millis(30));
            while capture.GetNextPacketSize().map_err(|e| err("read", e))? > 0 {
                let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                capture
                    .GetBuffer(&mut data, &mut frames, &mut flags, None, None)
                    .map_err(|e| err("read", e))?;
                if flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0 || data.is_null() {
                    mono.resize(mono.len() + frames as usize, 0);
                } else {
                    let bytes = std::slice::from_raw_parts(data, frames as usize * frame_bytes);
                    mono.extend(to_mono(bytes, channels, bits));
                }
                capture.ReleaseBuffer(frames).map_err(|e| err("read", e))?;
            }
        }
        let _ = client.Stop();
        Ok(rate)
    }
}

/// Interleaved shared-mode samples (32-bit float or 16-bit int) to mono
/// 16-bit, averaging the channels.
pub fn to_mono(bytes: &[u8], channels: usize, bits: u16) -> Vec<i16> {
    let width = bits as usize / 8;
    if channels == 0 || !(width == 2 || width == 4) {
        return Vec::new();
    }
    bytes
        .chunks_exact(channels * width)
        .map(|frame| {
            let sum: f32 = frame
                .chunks_exact(width)
                .map(|s| {
                    if width == 4 {
                        f32::from_le_bytes([s[0], s[1], s[2], s[3]])
                    } else {
                        i16::from_le_bytes([s[0], s[1]]) as f32 / 32768.0
                    }
                })
                .sum();
            ((sum / channels as f32).clamp(-1.0, 1.0) * 32767.0) as i16
        })
        .collect()
}

/// A 16-bit mono PCM WAV file.
pub fn wav(samples: &[i16], rate: u32) -> Vec<u8> {
    let data_len = (samples.len() * 2) as u32;
    let mut out = Vec::with_capacity(44 + data_len as usize);
    out.extend_from_slice(b"RIFF");
    out.extend_from_slice(&(36 + data_len).to_le_bytes());
    out.extend_from_slice(b"WAVEfmt ");
    out.extend_from_slice(&16u32.to_le_bytes()); // fmt chunk size
    out.extend_from_slice(&1u16.to_le_bytes()); // PCM
    out.extend_from_slice(&1u16.to_le_bytes()); // mono
    out.extend_from_slice(&rate.to_le_bytes());
    out.extend_from_slice(&(rate * 2).to_le_bytes()); // bytes per second
    out.extend_from_slice(&2u16.to_le_bytes()); // bytes per frame
    out.extend_from_slice(&16u16.to_le_bytes()); // bits per sample
    out.extend_from_slice(b"data");
    out.extend_from_slice(&data_len.to_le_bytes());
    for s in samples {
        out.extend_from_slice(&s.to_le_bytes());
    }
    out
}

/// OpenAI-compatible `/audio/transcriptions` (OpenAI, Groq, local Whisper servers).
pub async fn transcribe(
    http: &reqwest::Client,
    url: &str,
    model: &str,
    key: &str,
    wav: Vec<u8>,
) -> Result<String, String> {
    let file = reqwest::multipart::Part::bytes(wav)
        .file_name("speech.wav")
        .mime_str("audio/wav")
        .map_err(|e| e.to_string())?;
    let form = reqwest::multipart::Form::new()
        .text("model", model.to_string())
        .part("file", file);
    let res = http
        .post(format!("{url}/audio/transcriptions"))
        .bearer_auth(key)
        .multipart(form)
        .send()
        .await
        .map_err(|e| format!("Can't reach the speech service: {e}"))?;
    let status = res.status();
    let reply: serde_json::Value = res
        .json()
        .await
        .map_err(|e| format!("The speech service sent something unreadable: {e}"))?;
    if !status.is_success() {
        let detail = reply["error"]["message"]
            .as_str()
            .unwrap_or("request failed");
        return Err(format!("Speech error ({status}): {detail}"));
    }
    Ok(reply["text"]
        .as_str()
        .unwrap_or_default()
        .trim()
        .to_string())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::testutil::{http, mock_server};

    fn f32s(values: &[f32]) -> Vec<u8> {
        values.iter().flat_map(|v| v.to_le_bytes()).collect()
    }

    #[test]
    fn float_stereo_to_mono() {
        assert_eq!(
            to_mono(&f32s(&[0.5, -0.5, 1.0, 1.0]), 2, 32),
            vec![0, 32767]
        );
    }

    #[test]
    fn int16_mono_passes_through() {
        let bytes: Vec<u8> = [16384i16, -16384]
            .iter()
            .flat_map(|v| v.to_le_bytes())
            .collect();
        assert_eq!(to_mono(&bytes, 1, 16), vec![16383, -16383]);
    }

    #[test]
    fn unknown_formats_give_nothing() {
        assert!(to_mono(&[0; 6], 1, 24).is_empty());
    }

    #[test]
    fn wav_header() {
        let file = wav(&[1, 2, 3], 48000);
        assert_eq!(file.len(), 44 + 6);
        assert_eq!(&file[0..4], b"RIFF");
        assert_eq!(&file[8..12], b"WAVE");
        assert_eq!(u32::from_le_bytes(file[24..28].try_into().unwrap()), 48000);
        assert_eq!(u32::from_le_bytes(file[40..44].try_into().unwrap()), 6);
    }

    #[tokio::test]
    async fn transcribe_posts_the_clip() {
        let (url, requests) = mock_server(vec![r#"{"text":"  make a grocery list "}"#.into()]);
        let text = transcribe(&http(), &url, "whisper-1", "k", wav(&[0; 10], 16000)).await;
        assert_eq!(text, Ok("make a grocery list".into()));
        let body = requests.recv().unwrap();
        assert!(body.contains("speech.wav") && body.contains("whisper-1"));
    }
}
