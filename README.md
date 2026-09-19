<div align="center">

<img 
  src="https://github.com/user-attachments/assets/962887ec-636b-4e0f-90eb-0862c0feefca" 
  width="72"
/>

# Bloom

<br/>

<!-- HERO SHOWCASE — full-width cinematic shot or video -->
<!-- ![Hero](your-hero-url) -->
<img width="1920" height="1080" alt="Bloom Hero" src="https://github.com/user-attachments/assets/22041f25-a69e-457d-80d9-7dfbfbed2d29" />

</div>

---

Bloom makes your Windows desktop feel alive.

Every transition is a physics simulation.
Every element responds to touch.
Every pixel is in motion.

Your desktop has been asleep for years.
This wakes it up.

---

## The Island

<!-- SHOWCASE: GIF or short clip — Island expanding, cycling through modes (3-5s) -->
<!-- ![Island Demo](your-island-gif-url) -->

<p align="center">
  <img width="430" height="70" alt="Bloom Island" src="https://github.com/user-attachments/assets/0d723558-9df4-4214-b20e-4a1f97eb1f22" />
</p>

A notch at the top of your screen that adapts to what you're doing.

Scroll or swipe to switch modes.
Watch it transform.

**Music** — album art, track info, playback controls. A visualizer that reacts to five frequency bands with spring physics. It moves when the music plays.

<!-- SHOWCASE: GIF — music mode reacting to a song -->
<!-- ![Music Visualizer](your-music-gif-url) -->

**Command Center** — WiFi, Bluetooth, Do Not Disturb, volume, brightness. Everything you usually dig through settings for.

<!-- SHOWCASE: GIF — command center toggling controls -->
<!-- ![Command Center](your-command-center-gif-url) -->

**Status** — Battery, weather. Your desktop, summarized.

**Calendar** — A month view with a Pomodoro timer. Focus without switching apps.

Each transition is spring-loaded.
Width, height, border-radius, position — all animate independently.
It feels mechanical. In a good way.

---

## The Dock

<!-- SHOWCASE: GIF — dock appearing on hover, drag-reorder, window previews -->
<!-- ![Dock Demo](your-dock-gif-url) -->

<p align="center">
  <img width="576" height="102" alt="Bloom Dock" src="https://github.com/user-attachments/assets/96229f0e-1246-4baf-b8ad-3e8f77142a12" />
</p>

A taskbar that actually moves.

Bloom replaces your native Windows taskbar.
It sits at the bottom of your screen, always there when you need it.

Drag to reorder.
Hover for window previews.
Right-click for context menus.

It's not an overlay.
It *is* your taskbar.

<!-- SHOWCASE: GIF — dock hover previews in action -->
<!-- ![Window Previews](your-preview-gif-url) -->

---

## Under the Hood

<!-- SHOWCASE: Optional — architecture diagram or visual of the 5-window system -->
<!-- ![Architecture](your-arch-url) -->

A Rust backend that speaks directly to the Windows shell.

Global hooks intercepting keys before Windows sees them.
WASAPI capturing system audio in real-time.
COM controlling your media sessions.
WMI monitoring your hardware.

The whole thing sleeps when you don't need it.
The audio visualizer pauses when nothing's playing.
The cursor monitor hides when the dock is gone.
Thumbnails only refresh on focus.

It's fast because it has to be.

---

<!-- SHOWCASE: Full-width cinematic video or GIF montage -->
<!-- ![Bloom Montage](your-montage-url) -->

---

## 🛡️ Microsoft Defender

Some users may see a Microsoft Defender warning when installing Bloom.

The executable was submitted directly to Microsoft for analysis. Microsoft reviewed the file and confirmed that it **does not meet their criteria for malware or potentially unwanted applications**, and **the detection has been removed**.

<p align="center">
  <img width="800" alt="Microsoft Security Intelligence" src="https://github.com/user-attachments/assets/67ddddde-9c68-4338-a0f2-c5645e416514" />
</p>

<details>
<summary>Still seeing the detection?</summary>

Your system may still have the previous Defender signature cached. Microsoft recommends updating your Defender security intelligence.

Open **Command Prompt as Administrator** and run:

```cmd
cd "C:\Program Files\Windows Defender"
MpCmdRun.exe -removedefinitions -dynamicsignatures
MpCmdRun.exe -SignatureUpdate
```
</details>

## Get It Running

**Download** the latest build from [Releases](https://github.com/SehajveerSingh2005/bloom/releases/latest).

Or build from source:

```bash
git clone https://github.com/SehajveerSingh2005/bloom.git
cd bloom
bun install
bun run tauri dev
```

You'll need [Rust](https://rustup.rs/) and [Bun](https://bun.sh/). That's it.

## Linux (early support)

The first Linux target is **Linux Mint 22.x, Cinnamon, and X11**. Bloom launches,
shows its notch and dock, discovers and launches installed XDG applications,
resolves their icons from your icon theme (and lets you set your own per app),
tracks running windows through X11/EWMH (including focus, close and real hover
previews), reports CPU, memory, disk, network, brightness and battery, controls
volume and mute, drives any MPRIS-capable media player (track, artist,
transport, progress) with the audio visualizer reacting to your system audio,
reads and toggles Wi-Fi and Bluetooth, and reacts to the screen edges and to
windows covering it, so the dock's smart mode and the notch's fullscreen hiding
work the way they do on Windows. Both default to `smart` on Linux, so they move
out of the way as soon as a window covers them until you choose another mode.
The volume and brightness displays appear on real changes (your keyboard keys or
Bloom's own controls) rather than when the pointer touches a side edge, which
can be changed back in Settings.

It deliberately coexists with Cinnamon's panel rather than replacing it, and
relies on Cinnamon for media keys. Wayland is not supported yet, and a few
smaller desktop quirks are listed in the
[Linux port audit and roadmap](docs/linux-port.md), along with the architecture
and runtime tool expectations.

### Installing

```bash
bun install
bun run tauri build --bundles deb
sudo apt install ./src-tauri/target/release/bundle/deb/*.deb
```

This writes `bloom_<version>_amd64.deb`, which installs `/usr/bin/bloom` plus a
`Utility` menu entry with a hicolor icon set. The build then stops while signing
the updater artifacts, which requires the maintainer's
`TAURI_SIGNING_PRIVATE_KEY`; the package itself is already written at that point.

To run from source instead, `bun install && bun run tauri dev` works on Linux
Mint without extra packages. Runtime helpers `wpctl` (or `pactl`), `pw-record`,
`gsettings` and `cinnamon-settings` are used where available; Bloom explains
itself rather than failing when one is missing.

---

## Contributing

Bloom is open source.
Found a bug? Open an issue.
Have an idea? Send a PR.
Want to just say it's cool? A star goes a long way.

Licensed under [GPLv3](LICENSE).

---

<div align="center">

**Your desktop is waiting.**

</div>
