<h1 align="left">
  <img 
    src="https://github.com/user-attachments/assets/962887ec-636b-4e0f-90eb-0862c0feefca" 
    width="24" 
    style="vertical-align: -4px;"
  />
  Bloom
</h1>

### Your Windows, Alive.

**Bloom** is a desktop companion that reimagines how you interact with Windows. It moves away from static, rigid interfaces and introduces a workspace that feels natural, fluid, and responsive. 

By blending high-performance engineering with "mechanical fluidity," Bloom transforms your desktop into a living environment that adapts to your workflow in real-time.

---

### Showcase

<img width="1920" height="1080" alt="image" src="https://github.com/user-attachments/assets/44acd0f8-77e6-4d10-b623-10523466687e" />

---

### The Bloom Experience

#### The Bloom Island
A smart, adaptive notch at the top of your screen. It’s a context-aware hub that knows what you need before you do.
- **Music in Motion**: A reactive visualizer and media engine that expands when you're listening and tucks away when you're done.
- **Glanceable Status**: Real-time monitoring for your battery, weather, and connection—delivered through fluid "Power Pulses."
- **Integrated Focus**: Switch to a calendar view or a Pomodoro timer with a simple click to stay in your flow.

#### The High-Fidelity Dock
A taskbar that breathes. Inspired by modern design but engineered for Windows, the Dock features intelligent auto-hide, physics-based reordering, and native app integration. 

#### Pure Fluidity
Every interaction in Bloom is powered by a custom physics engine. Transitions aren't just "animations"—they are smooth, spring-loaded movements that respond instantly to your touch.

#### Professional Aesthetics
- **Mica Integration**: Deeply honors Windows 11 design language with native translucency.
- **Minimalist HUD**: Replaces the bulky system volume indicator with a sleek, edge-anchored overlay.
- **Rounded Harmony**: Adds subtle rounded corners to your screen for a softer, more modern display boundary.

---

### Engineering

Bloom is built for those who demand premium aesthetics without sacrificing performance.
- **Core**: High-efficiency Rust backend (Tauri v2) for zero-jank execution and minimal CPU impact.
- **UI**: React-powered frontend utilizing `framer-motion` for buttery-smooth state changes.
- **Native**: Direct integration with `windows-rs` for deep system-level control.

---

### Installation

Bloom requires [Rust](https://rustup.rs/) and [Bun](https://bun.sh/) for development.

1. **Clone the repository**:
   ```bash
   git clone https://github.com/SehajveerSingh2005/bloom.git
   cd bloom
   ```
2. **Install dependencies**:
   ```bash
   bun install
   ```
3. **Run in development mode**:
   ```bash
   bun run tauri dev
   ```

To generate a optimized production executable:
```bash
bun run tauri build
```

---

<p align="center">
  Built with ❤️ for a more beautiful desktop.
</p>
