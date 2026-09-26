# Release checklist

Bloom ships auto-updates to every install, so a bad release reaches users
silently. Run this checklist for every release, every time.

## 1. Before tagging (on `main`)

- [ ] CI green on `main` (Build, CodeQL) and `bun run build` passes locally.
- [ ] In `src-tauri`: `cargo check --locked`, `cargo clippy --locked --all-targets`, `cargo test --locked`.
- [ ] Run `bun run tauri dev` and exercise what the release touches. Always cover:
  - [ ] notch modes (fixed / smart / peek) and hover expansion
  - [ ] dock modes, pinned apps, Start menu
  - [ ] volume/brightness overlays and per-app mixer
  - [ ] Settings → About: "Check for Updates" reaches the network without erroring
  - [ ] tray → Quit restores the native taskbar (and no crash flag is left behind)
- [ ] Updater channel health: `bun scripts/verify-updater.mjs v<current>` passes.
  This fetches `latest.json` and every installer the way the app does, within
  the app's own `CHECK_TIMEOUT_SECS` budget (`src-tauri/src/updater.rs`). If it
  fails on the release machine, users on similar networks cannot update —
  fix the manifest/network path before tagging.
- [ ] Any network/timeout change was tested against a degraded connection, not
  just a fast one. Update paths must tolerate slow GitHub routes (SYN retries
  alone can take ~21 s).

## 2. Tag and publish

- [ ] `bun run release x.y.z` (must be on `main`).
- [ ] Watch the Release workflow to completion; do not walk away from a red run.

## 3. After the workflow

- [ ] `bun scripts/verify-updater.mjs vX.Y.Z` passes.
- [ ] **N‑1 install smoke test**: on a machine still running the previous
  version, click Settings → About → Check for Updates. It must find the new
  version. Auto-install waits ~24 h by design (`MIN_AUTO_INSTALL_AGE_SECS`),
  but the badge/label must appear immediately.
- [ ] Fresh install: download `bloom_X.Y.Z_x64-setup.exe`, install, confirm it
  launches and About shows the new version.
- [ ] Quit from the tray on the fresh install; confirm the taskbar returns.
- [ ] If users must be told something (workarounds, known issues), publish
  `website/public/announcements.json` and pin a GitHub issue with direct
  installer links.

## 4. If something is broken

- [ ] Rollback lever: edit the GitHub release and mark it **pre-release**.
  `releases/latest` then points at the previous stable release and update
  checks fall back to it.
- [ ] The 24 h age gate before auto-install gives a window to pull a bad
  release before it rolls out widely.
- [ ] Reach users on 3.9.1+ via `website/public/announcements.json`; reaching
  older builds requires a pinned issue / website notice, because a broken
  updater is exactly what they cannot use.
