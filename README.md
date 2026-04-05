<h1 align="left">
  <img 
    src="https://github.com/user-attachments/assets/962887ec-636b-4e0f-90eb-0862c0feefca" 
    width="24" 
    style="vertical-align: -4px;"
  />
  Bloom
</h1>

### Refined Desktop Utilities for Windows

Bloom translates the concept of the Dynamic Island into a high-performance, native Windows experience. Built on Tauri v2 and Rust, it provides a persistent, context-aware interface that integrates system status, media controls, and utility overlays with fluid motion and professional aesthetics.

---

### Showcase

---

### Core Module

#### The Dynamic Island
A centralized island that adapts to your workflow.
- **Audio Visualizer**: High-frequency reactive bars with liquid physics.
- **Media Engine**: Real-time album art extraction and marquee typography.
- **Status Dashboard**: Glanceable monitoring for battery, network, and temperature.
- **Productivity View**: Integrated Pomodoro timer and calendar transition.

#### Native Volume HUD
Bloom intercepts system volume events to provide a minimalist replacement for the Windows OSD.
- **Aggressive Suppression**: Automatically hides the native Microsoft volume indicator.
- **Edge-Anchored**: Smoothly slides from the screen boundary with spring-loaded physics.

#### Passive Screen Corners
Modernize your display with rounded screen boundaries.
- **Mica Integration**: Uses Windows 11 backdrop effects for a seamless blend.
- **Non-Intrusive**: Operates as a separate transparent layer that respects fullscreen applications.

#### Floating Settings Hub
A glassmorphic control center for real-time personalization.
- **Instant Sync**: Changes propagate across all Bloom windows without restarts.
- **Module Control**: Toggle visualizers, artwork, or secondary HUDs on the fly.

---

### Architecture

Bloom is engineered for zero-jank performance and minimal system impact.
- **Backend**: High-efficiency Rust core utilizing `tauri` and `windows-rs`.
- **Frontend**: React-based UI powered by `framer-motion` for fluid state transitions.
- **Aesthetics**: Native Windows 11 Mica/Acrylic effects via `window-vibrancy`.

---

### Installation

Bloom is built on Tauri v2. To run a local instance:

1. **Prerequisites**: [Rust](https://rustup.rs/) and [Bun](https://bun.sh/)
2. **Setup**:
   ```bash
   git clone https://github.com/SehajveerSingh2005/bloom.git
   cd bloom
   bun install
   ```
3. **Execution**:
   ```bash
   bun run tauri dev
   ```

To generate a production executable:
```bash
bun run tauri build
```

---
