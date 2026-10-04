# Janice Round 3 Implementation Plan

> **For agentic workers:** REQUIRED SUB-SKILL: Use superpowers:subagent-driven-development. Steps use checkbox (`- [ ]`) syntax.

**Goal:** Show saved keys as saved (they were never wiped; the UI just never showed them), let the user rename the assistant (wake phrase "Hey <name>", default "Hey Janice", retrain required after a rename), make the merged-dock panel expand exactly like the dock's media/control panels, and replace the full window on a wake-word request with a floating Siri-style 3D aura orb.

**Builds on:** `2026-10-04-bloom-ai-sidecar.md` and `2026-10-04-janice-round2.md` (sidecar `src-tauri/bloom-ai/`, Bloom `src-tauri/src/ai.rs`, panel `src/ai/`, dock `src/Dock.tsx` + `src/InfoCentre.tsx`, overlay window `src/Overlay.tsx`).

## Global Constraints

- All earlier Global Constraints hold (no em dashes; windows 0.61; Coucou untouched; commits on `feat/bloom-ai`, conventional, no attribution lines).
- **Saved API keys and passwords must never be lost.** Only "Delete AI altogether" may remove Credential Manager entries. Never change the `bloom-ai` service name, the secret names (`llm-key`, `stt-key`, `email-password`, `outlook-refresh`) or the keyring target format. Never send a secret value to a webview; only booleans saying whether one is stored.
- Default name "Janice"; wake phrase is "Hey <name>". Wake word stays off by default.
- **Cohesion is the top UI priority:** Janice's surfaces must reuse the exact motion, container and visual language of the surface she appears in (notch command centre / dock info panel). Copy values from the existing components; do not invent new ones.
- Animations run only while something is happening (CPU idle stays ~0%); respect `prefers-reduced-motion`.

## Tasks

### Task 1: Saved keys show as saved
- Bloom `ai_status` gains `secrets: { llm_key: bool, stt_key: bool, email_password: bool, outlook: bool }`, true when that credential exists. The sidecar does the lookup (Bloom has no keyring dep): add an In message `secret_status` answered by Out `secret_status {llm_key, stt_key, email_password, outlook}` (booleans only), or read Credential Manager from Bloom with the existing `windows` crate's `CredReadW` for existence only, never returning the blob. Pick the smaller safe option and justify it.
- `src/settings/AiTab.tsx` `SecretField`: when stored, show a "Saved" state (placeholder like "Saved in Windows Credential Manager", a small check) and keep "Save" for replacing it; an explicit "Remove" action is out of scope.
- Regression guard: a sidecar test asserting that handling every `In` message except none calls `secrets::wipe` (e.g. wipe only reachable from the `--wipe` CLI path), and a comment in `secrets.rs` stating the never-lose rule.

### Task 2: Custom name and "Hey <name>"
- New setting `bloom-ai-name` (default "Janice", trimmed, 1-24 chars, letters/spaces/hyphens/apostrophes).
- Sidecar system prompt uses the configured name ("You are <name>, the assistant built into Bloom...").
- Wake model: building writes the trained name next to the model (e.g. `wake/model.json` `{ "name": "<name>" }` or `wake/name.txt`); `ai_status.wake_trained` is true only when the model exists AND its trained name equals the current name (case-insensitive). Existing installs: the current `hello-janice.rpw` has no trained-name file, so it counts as untrained (the user must retrain with "Hey Janice"). Keep reading the old file name only if needed for cleanup; new builds may use a stable file name (e.g. `wake/wake.rpw`).
- When the name changes or the model is untrained, Bloom does not turn the listener on (send `wake_off` if it was on) even if `bloom-ai-wake` is "true"; the toggle shows as needing retraining.
- Settings: a "Name" field in the Bloom AI group (saves on blur); changing it shows "Retrain the wake word: say 'Hey <name>'". Enrollment copy uses "Hey <name>". Panel/orb labels and placeholder use the name ("Ask <name>").
- Tests: name validation; trained-name match logic; system prompt includes the name.

### Task 3: Dock panel cohesion (merged mode)
In merged mode the dock's media/calendar/timer/controls panel (`InfoPanel` in `src/InfoCentre.tsx`, anchored in `src/Dock.tsx`) unrolls from the bar with a specific motion (clipPath inset from 100% to 0% with `round 18px 18px 0px 0px`, opacity 0.4 to 1, spring stiffness 300 damping 32, same exit) and a fixed size/style (`.ic-panel` in `src/Dock.css`). Janice's dock panel currently appears without that motion and with a different box. Make it indistinguishable in motion, size behaviour, padding, background/glass, radius and typography from the InfoPanel. Strongly consider making Janice a tab/view of the same panel shell (same tabs row style, same slide transition) rather than a separate box; keep `onHeight`/click-rect and glass behaviour working. Re-check the notch panel against the command centre once more for anything still off (spacing, input/pill sizes, header row). Reuse; do not invent.

### Task 4: Floating orb for wake-word requests (3D aura)
- A wake-word request ("Hey <name>") must NOT open the notch/dock panel. Instead show a floating Siri-style orb on screen, using the existing full-screen click-through `overlay` window (`src/Overlay.tsx`, used for the volume/brightness HUDs) so no new WebView is created. Position: just under the notch in notch mode, just above the dock in merged mode (respect scale and multi-monitor the way the HUDs do).
- The orb: a 3D aura (layered blurred gradient shells rotating on different 3D axes with perspective, glow, soft breathing; colour/speed reflect phase: listening brighter/faster, transcribing/working swirling, done settling, error red). Pure CSS 3D transforms/opacity (compositor), no JS animation loop; reduced-motion = static glow. Reuse the same orb component (smaller) in the notch/dock panel's status line so the brand is consistent.
- Under the orb, a small caption shows what it heard and then the short reply, then the orb fades out after ~5 s of done/error. If the request needs a confirm (email/script), open the normal panel on the current surface with the confirm card (approving needs a real panel). Hotkey push-to-talk keeps opening the panel as today.
- Backend: Bloom routes a wake request to the overlay (e.g. emit `ai-orb {show: true}` to `overlay` on `wake`, and the overlay renders from `ai-event`s for that task); the overlay stays click-through; a confirm event for the orb's task emits `ai-open` to the surface.
- `bun run build`, `bun test scripts/ai-state.test.ts`, and Rust checks pass.

### Task 5: Rebuild and swap the test Bloom
Controller task: stop `bloom-ai-test.exe` and `bloom-ai.exe` (restore taskbar), build standalone debug Bloom + sidecar from HEAD in the snapshot worktree, install the sidecar to `%LOCALAPPDATA%\com.sehaz.bloom\ai\` (keep the `wake` folder), launch with logging. Never touch Credential Manager or `settings.json`.
