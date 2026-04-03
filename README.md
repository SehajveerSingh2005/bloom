# Bloom 🌸
**The Modern Desktop Experience for Windows**

Bloom is a minimalist, glassmorphic dynamic platform for Windows designed to seamlessly integrate essential utilities into your workflow. Built as a sleek "island" that stays neatly at the top of your screen, it provides immediate access to system data, media controls, and utility overlays with fluid, liquid-like animations—all while maintaining a clean, distraction-free desktop.

## ✨ Current Features
- **🎨 Universal Dynamic Island**: A centralized, context-aware pill that intelligently adapts to your needs.
- **🎵 Unified Media Experience**: Features a real-time reactive frequency visualizer and marquee text for song/artist details—automatically activating when music is detected.
- **⚡ System Dashboard**: Glance-able status monitoring for battery life, system temperature, and Wi-Fi connectivity.
- **🔊 Edge Volume Overlay**: A custom, "Apple-style" volume notch that pops out from the left edge of your screen. To keep your experience clean, **Bloom automatically hides the native Windows volume OSD**.
- **💎 Premium Aesthetics**: 20px frosted glass with high-fidelity concave "bridge" corners, designed to feel like a native extension of the Windows interface.
- **🚀 Engineered for Performance**: 1.0.0 is built on a high-performance Rust core (Tauri) and React (Framer Motion) to ensure zero-jank animations with minimal CPU footprint.

## 🔮 Future Roadmap
Bloom is evolving from a system-status tool into a complete desktop companion. Future updates aim to include:
- **Notifications Proxy**: A tighter, more refined view for system notifications.
- **Quick Actions**: One-click toggles for Focus mode, Night light, and more.
- **Customizable Widgets**: Embed tiny productivity tools directly into the island.
- **Theming Engine**: Support for multiple glass styles and accent color syncing.

## 🛠️ Getting Started
Bloom is built on **Tauri v2**. To run a local dev instance or build a binary:

1. **Prerequisites**: Ensure you have [Rust](https://rustup.rs/) and [Bun/Node](https://bun.sh/) installed.
2. **Clone & Install**:
   ```bash
   git clone https://github.com/SehajveerSingh2005/bloom.git
   cd bloom
   bun install
   ```
3. **Run Dev**: `bun run tauri dev`
4. **Build Windows Binary**: `bun run tauri build` (Builds located in `src-tauri/target/release/bundle`)

## ⚖️ License
MIT License - Developed by Sehajveer Singh.