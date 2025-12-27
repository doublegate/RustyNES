# Rust GUI and rendering frameworks for NES emulator development

**The ideal replacement for Iced + WGPU depends on your priority: use egui for emulator-centric development, pixels crate for rendering, and consider SDL2 for simplicity.** The Rust emulator ecosystem has matured significantly, with clear patterns emerging from successful projects like TetaNES (wgpu + egui), Pinky (SDL2 + libretro), and RustBoyAdvance-NG (SDL2). For a cycle-accurate NES emulator requiring modern UI, excellent performance, and broad platform reach, the **recommended stack is egui + pixels crate + winit + cpal** for a pure-Rust solution, or **SDL2** for a proven, batteries-included alternative.

## GUI frameworks: egui leads for emulator-specific needs

The Rust GUI landscape in 2025 offers several mature options, but **egui** emerges as the pragmatic choice for emulators specifically. Despite being immediate-mode (rebuilt each frame), this paradigm actually aligns perfectly with emulator architecture where debug views (registers, memory, disassembly) update every frame anyway.

**egui** (~25,000 GitHub stars, very active development sponsored by Rerun) integrates seamlessly with game loops. Rendering an NES frame buffer is trivial—load it as a texture and display in a panel. The framework excels at rapidly-changing debug information and requires no state synchronization between UI and emulator state. Current version **0.32.2** offers pure Rust with no external dependencies, multiple rendering backends (wgpu, glow), and proven WASM support with **1-2ms frame overhead**.

**Slint** (formerly SixtyFPS, ~18,700 stars, production-ready since v1.0 in April 2023) represents the best option if polished, retained-mode UI is the priority over debug flexibility. Its declarative DSL with live preview tooling produces professional interfaces with a **sub-300KB RAM footprint**.  The licensing model requires attention: GPLv3 for open source, royalty-free for desktop/mobile/web, commercial licenses for embedded with support.  Slint draws its own widgets rather than using native OS controls, which produces consistent cross-platform appearance but non-native feel.

**Dioxus** (~32,800 stars, v0.6.3) offers a React-like paradigm with RSX macros similar to JSX.  Its unique strength is genuine mobile support—`dx serve --platform ios` and Android targets work as of December 2024, though Android remains “quite experimental.” The desktop renderer uses system WebView (WebView2/WKWebView/WebKitGTK), which introduces platform inconsistencies but enables hot-reload and familiar web development patterns.  For teams with React experience wanting native mobile apps, Dioxus provides the most viable path.

|Framework|Maturity |Game Display Integration|Debug Views      |WASM|Mobile      |
|---------|---------|------------------------|-----------------|----|------------|
|egui     |Good     |⭐⭐⭐⭐⭐ Trivial texture   |⭐⭐⭐⭐⭐ Perfect fit|✅   |❌ (via WASM)|
|Slint    |Excellent|⭐⭐⭐ Requires integration|⭐⭐⭐ Possible     |✅   |Experimental|
|Dioxus   |Medium   |⭐⭐ WebView workarounds  |⭐⭐⭐ Workable     |✅   |✅ (new)     |
|Tauri    |Excellent|⭐⭐ WebGL canvas/IPC     |⭐⭐⭐ Web tooling  |❌   |✅           |

**Tauri 2.0** (~86,000 stars) deserves mention for teams with strong web skills. It produces **2.5MB bundles** versus Electron’s ~85MB, with mobile support added in October 2024. However, game display requires WebGL canvas or IPC workarounds that add latency—not ideal for real-time emulation.

Frameworks to avoid: **Druid is officially deprecated**; its successor **Xilem** remains “perma-experimental since May 2022” and explicitly not production-ready. **Makepad** (~5,000 stars) shows impressive GPU-first performance but suffers from lacking documentation and small community. **gtk-rs** works excellently on Linux but requires heavy system dependencies and feels non-native on Windows/macOS.

## Rendering backends: pixels crate simplifies emulator development

For NES emulator framebuffer presentation, the **pixels crate** represents the sweet spot between wgpu’s power and simpler abstractions. Built specifically for emulators and pixel-perfect rendering, it handles the common case—uploading a 256×240 frame buffer and scaling it to display resolution—with a drastically simplified API while maintaining full shader support for CRT effects.

**pixels** (2,000+ stars, actively maintained with January 2025 release) provides hardware-accelerated scaling on perfect pixel boundaries, non-square pixel aspect ratio support (useful for NES’s 8:7 pixel ratio), and works with winit, tao, or fltk window managers.  Shader support via WGSL enables CRT-Royale, scanlines, NTSC filters, and xBR scaling—essential for authentic retro presentation.  Cross-platform support covers Windows, macOS, Linux, and WebGL2 (WebGPU work in progress).

**Direct wgpu** use (v23.0.0, October 2024) is typically overkill for simple framebuffer blitting. While necessary for complex shader pipelines or WebGPU browser targets, the abstraction layer adds unnecessary complexity when pixels crate handles the common emulator patterns. Reports indicate **23% performance regression** in some workloads between v20 and v22,  though resource tracking optimizations have improved (~40% gains with Arc-based tracking).

**SDL2** (rust-sdl2 crate) remains the battle-tested alternative, particularly valuable when unified audio/input/video is desired. Multiple successful Rust emulators (Pinky, RustBoyAdvance-NG, sprocketnes) use SDL2. The `bundled` feature eliminates runtime dependency concerns. Trade-off: C dependency adds build complexity versus pure-Rust alternatives.

For VSync and frame pacing critical to emulation:

- **PresentMode::Fifo**: Traditional VSync, ~16.67ms frame time, adds 1-2 frames latency
- **PresentMode::Mailbox**: Lower latency, discards old frames, may tear on some systems
- **Recommended approach**: Fifo with dynamic audio rate control (used by higan, RetroArch, Mesen)

Avoid **minifb** for production—it lacks hardware acceleration (except non-configurable macOS Metal)   and shader support, limiting quality.  **softbuffer** serves as fallback for GPU-less systems but requires CPU-side scaling/filtering.  **vulkano** and **ash** (Vulkan bindings) are vastly overkill for 2D pixel pushing.

## Real-world Rust emulators reveal proven patterns

Examining successful Rust emulator projects reveals clear architectural patterns and technology choices:

**TetaNES** (219 stars, 1,281 commits, very active) represents the modern pure-Rust approach: wgpu + egui + winit + cpal. Split into `tetanes-core` (emulation library) and `tetanes` (UI binary), it achieves cross-platform desktop plus **working WebAssembly deployment**. The developer explicitly migrated from SDL2 to wgpu specifically for web support. Includes built-in NTSC filter and CRT shader, demonstrates 4-player gamepad support.

**Pinky** (802 stars) takes the libretro approach: compile as a libretro core, delegate all UI to frontends like RetroArch.  Primary distribution is `pinky-libretro` with a minimal SDL2-based dev UI.  This pattern provides save states, netplay, and shader support “for free” through the frontend ecosystem. The `libretro-backend` Rust crate (by Pinky’s author) simplifies this approach.

**RustBoyAdvance-NG** (639 stars) demonstrates multiple frontends from a single core: SDL2 desktop, WebAssembly, Android via JNI, even terminal frontend using crossterm + viuer. This architecture pattern—strict core/frontend separation—appears in nearly all successful projects and enables maximum portability.

**Key architectural pattern**: Separate emulation core from platform layer. Every successful project isolates CPU/PPU/APU/Memory/Input emulation in a library crate, with frontends handling rendering, audio output, and input. This enables:

- Multiple simultaneous frontends (desktop, web, mobile, libretro)
- Testing without graphics
- Cleaner code organization
- Easier porting

## Cross-platform viability and WASM performance

**Desktop support** is mature across all major frameworks. egui, Dioxus, Slint, and Tauri all function well on Windows, Linux (X11/Wayland), and macOS including Apple Silicon. Build complexity varies: egui requires only `cargo add egui eframe` plus Linux display libraries; gtk-rs demands full GTK4 development packages.

**WebAssembly deployment is proven viable for 60fps emulation.** Multiple Rust NES and Game Boy emulators run in browsers: TetaNES (lukeworks.tech/tetanes-web), nes-rust, wasm-gb.  Performance keys:

- Avoid excessive JS-WASM boundary crossings
- Use typed arrays for frame buffer transfer
- Web Audio API with AudioWorklet for low-latency sound
- requestAnimationFrame for vsync-aligned frame pacing

Canvas-based frameworks (egui, Slint) struggle with mobile virtual keyboards in WASM—acceptable for emulators where keyboard input isn’t typical.

**Mobile support** remains the weakest area:

- **Dioxus v0.6** (December 2024): Native iOS/Android via WebView, hot-reload works, Android “quite experimental”
- **Tauri 2.0** (October 2024): iOS/Android officially supported, mobile-specific plugins (NFC, haptics, geolocation)
- **Slint**: Android experimental, iOS planned
- Most Rust emulators simply don’t target mobile natively—WASM serves as the mobile path

For CI/CD friendliness, pure-Rust stacks (egui, Dioxus, Slint, pixels) enable reproducible builds without system dependencies. gtk-rs complicates pipelines significantly.

## Framework deep dives for emulator requirements

**Slint for emulator UIs**: The .slint markup language and live preview tooling excel at building preference dialogs, settings screens, and “player” interfaces.  Performance is solid with multiple renderer options (Femtovg/OpenGL, Skia, software).  For a polished frontend where debug views aren’t primary, Slint provides the most professional result. However, game display integration requires more work than egui’s trivial texture approach. The royalty-free license covers desktop/mobile/web; commercial license needed for embedded with support.

**egui achieving polish**: While immediate-mode, egui supports custom styling via the `Style` system and custom painting through epaint. The ecosystem includes egui_tiles for tiling layouts, egui_plot for visualization. Projects like Rerun Viewer demonstrate complex, professional UIs. For emulators, the “non-native” appearance matters less than functional debug views and smooth game display.

**Dioxus renderer options**: Desktop uses system WebView by default (~15MB binaries). A WGPU-based native renderer using Vello/Taffy is in development for true native rendering. The WebView approach means CSS styling works, enabling rapid polished UI development—but introduces platform inconsistencies and potential latency for game display integration.

**Makepad status**: Despite impressive performance claims and usage in Lapce code editor and Robrix Matrix client, documentation gaps make Makepad difficult to recommend. The `live_design!` macro DSL and shader-based styling are novel but poorly documented. Watch for maturity improvements, but not production-ready for new projects.

## Recommended framework combinations

### Best overall: egui + pixels + winit + cpal

This pure-Rust stack provides the most emulator-appropriate architecture:

```
Emulator Core (platform-agnostic library)
    ↓ Frame buffer + audio samples
pixels crate (GPU-accelerated framebuffer)
    ↓ Rendered texture
egui (debug views, menus, settings)
    ↓ Window events
winit (cross-platform windowing)
    + cpal (audio output)
```

**Strengths**: Immediate-mode suits debug views perfectly. Game display integrates as simple texture. Pure `cargo build` on all platforms. WASM deployment proven. Active development with corporate backing (Rerun sponsors egui). Shader support for CRT effects via pixels’ WGSL pipeline.

**Weaknesses**: Non-native look requires custom styling. IME quirks on Windows. No native mobile (WASM serves mobile browsers). Immediate-mode paradigm may feel unfamiliar.

### Alternative: SDL2 unified stack

For simpler setup prioritizing proven reliability:

```
Emulator Core
    ↓
SDL2 (graphics + audio + input)
    + egui_sdl2_gl (optional GUI overlay)
```

**Strengths**: Battle-tested in emulator scene (Pinky, RustBoyAdvance-NG, sprocketnes). Excellent gamepad support. Unified library eliminates integration complexity. `bundled` feature packages dependencies. Extensive documentation and community knowledge.

**Weaknesses**: C dependency complicates pure-Rust builds. No WASM support (must maintain separate web frontend). Heavier binaries. OpenGL-based (requires glow/glutin for modern GL usage).

### For maximum reach: Dioxus + emulator core

When mobile apps and web deployment from single codebase matter:

```
Emulator Core (Rust library)
    ↓
Dioxus (desktop WebView + mobile + web)
    + Canvas/WebGL for game display
```

**Strengths**: Single codebase for desktop, iOS, Android, web.  React-familiar patterns. Hot-reload accelerates development. CSS styling enables polished UI rapidly.

**Weaknesses**: WebView introduces platform inconsistencies. Game display requires WebGL canvas workarounds. Mobile support still maturing. Pre-1.0 API stability concerns.

## Conclusion: prioritize egui for emulator-specific needs

For a cycle-accurate NES emulator replacing Iced + WGPU, the **egui + pixels + winit + cpal** stack resolves the core pain points: pixels crate eliminates raw WGPU complexity while retaining shader support; egui’s immediate-mode paradigm naturally fits emulator architecture where debug views update every frame; pure Rust enables WASM deployment without maintaining separate frontends.

The “retained-mode desktop feel” preference suggests evaluating **Slint** for settings/preferences screens, potentially in a hybrid approach where Slint handles configuration while egui/pixels handles actual emulation display. However, starting with egui provides the pragmatic foundation that works—polish can be added incrementally.

**Concrete next steps**:

1. Examine TetaNES source code as architecture reference (github.com/lukexor/tetanes)
1. Prototype with pixels crate for framebuffer rendering
1. Add egui for debug views (memory viewer, pattern tables, disassembly)
1. Implement CRT shaders via pixels’ WGSL pipeline
1. Test WASM build early to catch compatibility issues

The Rust emulator ecosystem has matured past the experimental phase. Multiple production-quality NES emulators demonstrate that these frameworks deliver on their promises for cross-platform, performant emulator development.
