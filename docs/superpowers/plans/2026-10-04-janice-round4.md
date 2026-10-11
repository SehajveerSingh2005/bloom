# Janice Round 4: Reliability Fixes

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development and superpowers:systematic-debugging (root cause before fixes).

**Goal:** Fix the user's reports: "Hey Janice" does not trigger reliably; asking for the weather opened a weather website instead of answering; asking about Neha Aggarwal said it didn't know her although she is saved in `contacts.json` ("Neha aggarwal": neha.aggarwal2004@gmail.com).

## Global Constraints
- All earlier constraints hold (no em dashes; never touch Credential Manager entries except via Delete AI; Coucou untouched; commits on `feat/bloom-ai`, conventional, no attribution).
- Read-only access to the user's real wake data in `%LOCALAPPDATA%\com.sehaz.bloom\ai\wake\` is allowed for diagnosis; never modify or delete it; never read secrets.
- Idle CPU with wake off stays 0%; with wake on, report the new CPU cost if detection settings change.

## Evidence so far
- Wake model retrained 18:20 with 4 samples (`sample-1..4.wav`, ~2.1-2.3 s each at the mic rate, about 48 kHz), `wake.rpw` + `name.txt` = "Janice". Settings: wake on, name Janice, merged dock mode.
- Detector: `RustpotterConfig::default()` (default thresholds) with `vad_mode = Some(VADMode::Hard)` (`bloom-ai/src/wake.rs` `detector`). Model built with `WakewordRef::new_from_sample_files(NAME, None, None, files, MFCC_SIZE)`.
- No logging exists for what the agent did (tool calls) or wake scores, so behaviour can't be inspected after the fact.
- System prompt (`agent.rs` `system_prompt`) only mentions `find_contact` for email, tells the model to use `open` for web links, and has no rule to answer questions directly. There is no weather tool. Each request is independent (no conversation memory).

## Tasks

### Task 1: Wake word diagnosis and fix
1. Add a diagnostic CLI to the sidecar, `bloom-ai.exe --wake-score <wake dir> [wav...]`, that loads `wake.rpw` and runs each given WAV (default: the dir's `sample-*.wav`), padded with ~1 s of silence before/after and resampled exactly the way the live listener feeds audio, through the detector with several configurations (VAD Hard / Easy / none; a few thresholds and avg thresholds), printing per file the best score, whether it detected, and which config. No other side effects.
2. Run it against the user's real samples (read-only) and against a recording with background noise if you can synthesise one (e.g. sample + generated noise at a few levels). Record the numbers in the report.
3. Find the root cause from the data (e.g. Hard VAD suppressing normal speech, threshold too high for a 4-sample reference, a format/rate mismatch between training and live audio, frames fed in the wrong size). Fix it with the smallest change that makes the user's own samples detect reliably with margin and keeps false triggers unlikely (do not just set the threshold to near 0). If the evidence says enrollment itself is flawed (e.g. trimming cuts the phrase, too-short samples), fix enrollment and say the user must retrain.
4. Add an opt-in `bloom-ai-debug` setting ("false" default): when "true", the sidecar appends to `ai\debug.log` (rotating, capped ~1 MB): wake detections with scores, request text, tool calls with arguments (redact any `value` of `set_secret`, never log secrets), tool results truncated to ~300 chars, and replies. Off = no file writes.
5. Report the CPU while listening if detection settings change.

### Task 2: Answer questions, weather, and people
1. System prompt: answer questions directly in the reply (general knowledge, facts, the user's own data via tools). Use `open` only when the user explicitly asks to open/launch/show something. For any mention of a person by name (who is X, email X, call X, X's address), call `find_contact` first and use what it returns.
2. Add a `get_weather` tool: current conditions and today's forecast from Open-Meteo (free, no key; Bloom already uses it), location from the settings Bloom already stores (`bloom-weather-lat`/`bloom-weather-lon`, else `bloom-weather-city` via Open-Meteo geocoding, else IP lookup via `ipapi.co` as Bloom does), units from `bloom-temp-unit`. Returns a compact text summary the model turns into a sentence. Tests with the mock HTTP server.
3. Short conversation memory: keep the last ~6 user/assistant turns (text only, no tool results) for 10 minutes so follow-ups ("email her", "what about tomorrow") work; cleared on Stop/close, AI off, and after the timeout. Memory lives only in the sidecar process (RAM), never on disk.
4. Tests: prompt contains the new rules; weather tool parsing/units/location fallback; memory window/expiry.
