<div align="center">

<img 
  src="https://github.com/user-attachments/assets/962887ec-636b-4e0f-90eb-0862c0feefca" 
  width="72"
/>

# Bloom

### Your Windows, alive.

*A desktop companion that moves like thought — fluid, responsive, and unapologetically beautiful.*

<!-- HERO SHOWCASE — full-screen hero image or video goes here -->
<!-- ![Hero](your-hero-image-or-video-url) -->
<img width="1920" height="1080" alt="Screenshot (170)" src="https://github.com/user-attachments/assets/22041f25-a69e-457d-80d9-7dfbfbed2d29" />

</div>

---

Bloom is not a skin. It's not a theme pack. It's a ground-up reimagining of how you interact with your Windows desktop — built on a high-performance Rust engine, rendered through physics-driven React animations, and deeply integrated into the Windows shell at a level most apps never touch.

It feels alive because every pixel is in motion. It feels premium because every transition is intentional. It feels like *yours* because it adapts to how you actually work.

---

## The Island

<!-- SHOWCASE: GIF or short clip of the notch expanding, music mode, command center — 3-5 seconds -->

<p align="center">
  <img width="430" height="70" alt="Bloom Island" src="https://github.com/user-attachments/assets/0d723558-9df4-4214-b20e-4a1f97eb1f22" />
</p>

At the top of your screen lives a context-aware hub that morphs based on what you're doing. Swipe or scroll left and right to cycle through four modes:

- **Music** — A reactive audio visualizer powered by real-time WASAPI loopback capture and DFT analysis. Album art, track info, playback controls — all from a single glance. The visualizer bars respond to 5 frequency bands with individually tuned spring physics.

- **Command Center** — WiFi, Bluetooth, dock/notch mode toggles, Do Not Disturb, system tray access, volume and brightness sliders. A minimal control surface that replaces digging through settings menus.

- **Status** — Battery status, weather, and more — all at a glance. More coming soon.

- **Calendar** — A month view with an integrated Pomodoro timer. Focus without leaving your workspace.

Every mode transition is a spring-loaded animation — no easing curves, no linear interpolations. Width, height, border-radius, and position each animate with independent spring parameters, creating a staggered, mechanical fluidity that feels physical.

---

## The Dock

<!-- SHOWCASE: GIF or clip of dock appearing, drag-reorder, window previews — 3-5 seconds -->

<p align="center">
  <img width="576" height="102" alt="Bloom Dock" src="https://github.com/user-attachments/assets/96229f0e-1246-4baf-b8ad-3e8f77142a12" />
</p>

A taskbar that breathes. Bloom's dock floats above your desktop in auto-hide mode by default — appearing when you need it, vanishing when you don't. The native Windows taskbar is suppressed entirely — Bloom *is* your taskbar.

- **Pinned apps** with persistent ordering via drag-and-drop (powered by `framer-motion` Reorder)
- **Running apps** detected through real-time `EnumWindows` polling
- **Window previews** captured via `PrintWindow` with a disk-persisted thumbnail cache
- **Click-through intelligence** — a Rust-side cursor monitor running at 32ms intervals determines when to intercept clicks and when to pass them through to the desktop below
- **Context menus** for pin/unpin, app search, and Bloom options

---

## Under the Hood

<!-- SHOWCASE: Optional — architecture diagram or a visual showing the 5-window system -->

Bloom runs **5 independent Tauri webview windows** — each a separate React application — coordinated through a Rust backend that speaks directly to the Windows shell.

| Window | Purpose |
|---|---|
| `main` | The Island (notch) |
| `dock` | The Dock (taskbar replacement) |
| `settings` | Mica-backed settings panel |
| `volume-overlay` | Left-edge volume HUD |
| `brightness-overlay` | Right-edge brightness HUD |

### The Engine

- **Rust core** (`Tauri v2`) — Zero-jank execution. Global keyboard hooks intercept volume/brightness keys before Windows sees them. WASAPI loopback captures system audio for real-time visualization. COM interfaces control media sessions, audio endpoints, and display brightness. WMI monitors hardware state. `SetWinEventHook` tracks window focus, minimize, and foreground changes.

- **React frontend** (`framer-motion`) — Every animation is a physics simulation. Spring stiffness, damping, and mass are tuned per-property. The result is motion that feels weighty and deliberate, not computed.

- **Deep system integration** — AppBar registration via `SHAppBarMessage`. Taskbar suppression via `ShowWindow(SW_HIDE)` with a hook to prevent re-show. Cursor tracking with `GetCursorInfo` for pixel-accurate click-through. Window thumbnails via `PrintWindow` with a capped image cache. App enumeration through Shell API's `FOLDERID_AppsFolder` and filesystem scanning.

### Performance Philosophy

Bloom is obsessive about idle cost. The audio visualizer skips processing when no media is playing. The cursor monitor sleeps when the dock isn't visible. Window thumbnails are cached and invalidated on focus events. Icon extraction uses a persistent disk cache with in-memory lookup. The entire application is designed to be *there* without being *heavy*.

---

## Quick Start

### Prerequisites

- [Rust](https://rustup.rs/) (stable)
- [Bun](https://bun.sh/)

```bash
# Clone
git clone https://github.com/SehajveerSingh2005/bloom.git
cd bloom

# Install
bun install

# Develop
bun run tauri dev

# Build for production
bun run tauri build
```

---

## Built With

<table>
  <tr>
    <td align="center"><strong>Backend</strong></td>
    <td align="center"><strong>Frontend</strong></td>
    <td align="center"><strong>System</strong></td>
  </tr>
  <tr>
    <td>Rust · Tauri v2</td>
    <td>React 19 · TypeScript · Vite 7</td>
    <td>windows-rs · WASAPI · WMI · COM</td>
  </tr>
  <tr>
    <td>tokio · serde</td>
    <td>framer-motion · Tailwind CSS 4</td>
    <td>SHAppBarMessage · DWM · GDI</td>
  </tr>
  <tr>
    <td>window-vibrancy</td>
    <td>Rollup (multi-entry)</td>
    <td>Shell API · Global Hooks</td>
  </tr>
</table>

---

<div align="center">

**Bloom is an open-source project.**
If it makes your desktop a little more alive, consider giving it a star.

</div>
