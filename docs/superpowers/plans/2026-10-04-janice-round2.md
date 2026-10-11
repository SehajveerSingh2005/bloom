# Janice Round 2 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Name the assistant Janice, add an optional "Hello Janice" wake word trained on the user's voice, make the notch panel open like the control centre, add a Siri-style orb while Janice works, and stop Janice from closing apps it opened.

**Architecture:** Builds on `2026-10-04-bloom-ai-sidecar.md` (same sidecar `src-tauri/bloom-ai/`, Bloom `src-tauri/src/ai.rs`, panel `src/ai/`). The wake word runs inside the sidecar with Rustpotter (pure Rust, Apache-2.0), fed by a continuous WASAPI capture that only exists while the wake toggle is on. Detection starts an auto-stopping recording (energy VAD) and reuses the voice request path.

**Tech Stack:** as before, plus `rustpotter` 3.x.

## Global Constraints

- Everything in the first plan's Global Constraints still holds (no em dashes; windows 0.61; secrets only in Credential Manager; Coucou untouched; commits on `feat/bloom-ai`, no attribution lines).
- **API keys and passwords the user already saved must survive every update, rebuild and reinstall.** Only "Delete AI altogether" may wipe them. No task may add code that deletes, renames or re-namespaces Credential Manager entries, or changes the `bloom-ai` service name or secret names.
- Wake word is **off by default** (`bloom-ai-wake` = `"false"`). With it off, nothing listens and idle CPU stays 0%. With it on, a few MB of RAM is fine; target under ~2% of one core while listening (measure and report).
- Audio is processed locally. Only the request recorded after "Hello Janice" goes to the speech endpoint.
- The assistant's name is **Janice** in UI copy and the system prompt.

## Tasks

### Task 1: Janice no longer closes apps it opened
Problem: the kill-on-close Job object in Bloom's `ai.rs` (from the final fix wave) contains the whole sidecar tree, so apps launched via the `open` tool (ShellExecute) or `Start-Process` die when AI turns off, Delete AI runs, or Bloom exits.
- Remove the Job object from Bloom's `src-tauri/src/ai.rs` (keep `stop()` killing/waiting `bloom-ai.exe`; remove the job handle field and the added `windows` features if nothing else uses them).
- In the sidecar `src-tauri/bloom-ai/src/powershell.rs`: create a kill-on-close Job per script run and assign the spawned `powershell.exe` to it right after spawn; the job handle is owned by the running future, so dropping it (Stop, timeout, sidecar exit, which closes all its handles) kills the script and its children. Processes the script deliberately detaches (Start-Process) still die with it; that is accepted for scripts.
- `open` (ShellExecute) launches must not be in any job.
- Tests: a sidecar test that runs a script starting a child `powershell -Command Start-Sleep 30` in the background, cancels (drops the future), and asserts the child is gone (query by a unique marker in its command line via `Get-CimInstance Win32_Process`). Keep it under ~10 s.

### Task 2: Wake word engine (sidecar + Bloom)
Sidecar (`src-tauri/bloom-ai/`):
- Add `rustpotter = "3"` (check the crate's docs on docs.rs for the exact API: `Rustpotter`, `RustpotterConfig`, `WakewordRef`, `WakewordRefBuildFromFiles`/`...FromBuffers`, `WakewordSave`/`WakewordLoad`, `process_*` returning `Option<RustpotterDetection>`, samples per frame, audio format config). Feed it the mic's native format or 16 kHz mono i16/f32 as its config requires.
- New module `wake.rs`:
  - `enroll_sample(dir, index) -> Result<PathBuf>`: record ~2.5 s from the default mic, trim leading/trailing silence with the same energy VAD, save `wake/sample-{index}.wav` (16 kHz mono 16-bit is fine; resample by simple decimation/averaging if the mix rate is 48 kHz).
  - `build(dir) -> Result<()>`: build a `WakewordRef` named `hello janice` from all `wake/sample-*.wav` (need at least 3), save `wake/hello-janice.rpw`.
  - `Listener::start(dir, on_detect) / stop()`: a thread doing continuous WASAPI capture (reuse `voice.rs` capture code: refactor `capture` so its loop can feed a callback instead of only buffering), feeding Rustpotter; on detection call `on_detect`, then ignore detections for ~2 s. Sleep between reads like `voice.rs` (30 ms). Pause detection while a request is recording or running.
- VAD-stopped recording for wake requests: after detection, record until ~900 ms of silence following speech, or 12 s max, or 4 s with no speech at all (then emit `error{task, "Didn't catch that."}`). Energy VAD: RMS over 30 ms windows vs an adaptive noise floor (e.g. floor = 10th percentile of the first 300 ms, speech = RMS > floor*3 and > a small absolute minimum). Pure function with tests.
- Protocol additions (update `protocol.rs`, the plan's Wire Protocol table and Bloom's relay as needed):
  - In: `wake_on`, `wake_off`, `enroll_sample { index }`, `enroll_build`.
  - Out: `wake` (detected; carries `task` the sidecar assigns, taken from a separate range starting at 1_000_000_000 so it never collides with Bloom's counter), `enroll_saved { index }`, `enroll_done`, and errors via the existing `error`.
  - After `wake`, the sidecar itself runs: `recording{on:true}` then VAD stop, `recording{on:false}`, `transcript{task}`, agent run, `reply{task}` (same as the hotkey path).
- Name: system prompt starts "You are Janice, the assistant built into Bloom...".
Bloom (`src-tauri/src/ai.rs`):
- New setting `bloom-ai-wake` (`"false"` default). In `sync_from_settings`: when AI is enabled and wake is `"true"`, ensure the sidecar is running and send `wake_on` (once per change); when wake turns off (or AI off), send `wake_off` if running. On startup with both on, start the sidecar and send `wake_on`.
- Relay: on `wake` from the sidecar, emit `ai-open {recording: true}` to the current surface and also forward the event as `ai-event`.
- New commands `ai_enroll_sample(index: u32)`, `ai_enroll_build()` that forward to the sidecar (starting it if needed); `ai_status` gains `wake_trained: bool` (= `ai/wake/hello-janice.rpw` exists).
- Wake data lives in the `ai` folder, so Delete AI removes it; it never touches Credential Manager beyond the existing `--wipe`.
- Tests: VAD pure-function tests; protocol parse/serialize tests for the new messages; `cargo test -p bloom-ai`, `cargo test --locked ai::`, clippy for both.
- Report CPU while listening: run the sidecar with `wake_on` for 60 s and report `bloom-ai.exe` CPU time delta and working set (PowerShell `Get-Process`).

### Task 3: Settings: "Hello Janice"
`src/settings/AiTab.tsx`:
- New "Voice" rows: "Teach Janice your voice": shows "Say 'Hello Janice' after you click. Sample N of 5"; a Record button calls `ai_enroll_sample(N)`, waits for `enroll_saved`, advances; after 3+ samples a "Finish" button calls `ai_enroll_build`, waits for `enroll_done`. A "Retrain" action restarts at sample 1.
- Toggle "Hello Janice" (`bloom-ai-wake`), disabled until `wake_trained`; description notes the microphone listens locally while on.
- Errors from these flows show in the existing message line. No em dashes.

### Task 4: Panel: control-centre cohesion, Siri orb, Janice name
`src/ai/AiPanel.tsx`, `src/ai/ai.css`, `src/App.tsx` (notch block), `src/Dock.tsx` only if needed.
- Make the notch AI panel open the way the command centre does: same container conventions as `.command-center-content-minimal` (`src/App.css` ~1352: absolute below the 36 px row, `padding: 12px 14px`, `border-radius: 0 0 18px 18px`, `color: var(--bloom-text)`), the same framer-motion transition (`type: "spring", stiffness: 400, damping: 30`, exit with `filter: blur(4px)` 0.1 s), and stable heights per phase so the notch's own spring animates between sizes instead of jumping with every content change (e.g. compact ~120 px for listening/working, taller only for a confirm card or a long reply, capped). Buttons and the input should use the same visual language as the command centre pills (`cc-pills-grid` styles) so it reads as part of the same island.
- Siri-style orb: a round, softly blurred, multi-colour gradient (conic/radial) that slowly rotates and breathes while the phase is `recording`, `transcribing` or `working` (faster/brighter while recording), static and dim when idle or done, red-tinted on error. Pure CSS (transform/opacity/filter animations only, so the compositor does the work); no JS timers. It replaces the red recording dot and sits at the left of the status line; also shown in the dock panel.
- Title/idle label "Janice" instead of "Bloom AI"; placeholder "Ask Janice".
- `bun run build`, `bun test scripts/ai-state.test.ts` pass.

### Task 5: Rebuild and swap the test Bloom
Controller task (no implementer): stop the running `bloom-ai-test.exe` (restore taskbar), build a standalone debug Bloom from this branch's HEAD (`tauri build --debug --no-bundle`), install the latest sidecar (`bun run ai:dev`), copy the exe out of `target` and launch it. Credential Manager entries and `settings.json` are left as they are.
