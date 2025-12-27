# rustynes-desktop

Desktop frontend for the RustyNES NES emulator using egui and eframe.

**Version:** 0.7.0
**Part of:** [RustyNES](https://github.com/doublegate/RustyNES) workspace
**License:** MIT OR Apache-2.0

---

## Table of Contents

- [Architecture Overview](#architecture-overview)
- [Technology Stack](#technology-stack)
- [Architecture Decisions](#architecture-decisions)
- [Module Structure](#module-structure)
- [Key Implementation Details](#key-implementation-details)
- [Debug Windows](#debug-windows)
- [Building and Running](#building-and-running)
- [Command Line Arguments](#command-line-arguments)
- [Keyboard Controls](#keyboard-controls)
- [Configuration File](#configuration-file)
- [Future Improvements](#future-improvements)
- [Contributing](#contributing)

---

## Architecture Overview

```
                     +------------------+
                     |     main.rs      |
                     |  (CLI + eframe)  |
                     +--------+---------+
                              |
                              v
+---------------+    +--------+---------+    +---------------+
|   config.rs   |<-->|     app.rs       |<-->|   audio.rs    |
| (RON config)  |    |  (eframe::App)   |    | (cpal output) |
+---------------+    +--------+---------+    +---------------+
                              |
            +-----------------+------------------+
            |                 |                  |
            v                 v                  v
    +---------------+  +-------------+   +---------------+
    |   input.rs    |  |  gui/mod.rs |   | rustynes-core |
    | (kbd+gamepad) |  |  (egui UI)  |   |  (emulator)   |
    +---------------+  +------+------+   +---------------+
                              |
         +--------------------+--------------------+
         |          |         |         |          |
         v          v         v         v          v
     +------+  +--------+  +------+  +------+  +--------+
     | menu |  |settings|  | cpu  |  | ppu  |  | memory |
     +------+  +--------+  +------+  +------+  +--------+
                               debug windows
```

### Data Flow

1. **Input**: Keyboard/gamepad events are captured by `input.rs` and converted to NES controller state
2. **Emulation**: The `rustynes-core` Console runs one frame at a time (29,780 CPU cycles)
3. **Video**: The NES framebuffer (256x240 RGBA) is copied to an egui texture for display
4. **Audio**: Audio samples from the APU are queued to a ring buffer and consumed by cpal

---

## Technology Stack

| Component | Crate | Version | Purpose |
|-----------|-------|---------|---------|
| **GUI Framework** | `eframe` | 0.29 | Window management, event loop, rendering context |
| **Immediate Mode UI** | `egui` | 0.29 | Menus, debug windows, overlays |
| **Audio Output** | `cpal` | 0.15 | Cross-platform low-latency audio I/O |
| **Gamepad Support** | `gilrs` | 0.11 | Cross-platform gamepad input |
| **File Dialogs** | `rfd` | 0.15 | Native open/save dialogs |
| **Configuration** | `ron` | 0.8 | Rusty Object Notation for config files |
| **Platform Paths** | `directories` | 5.0 | Platform-specific config/data directories |
| **CLI Parsing** | `clap` | 4.5 | Command-line argument parsing with derive macros |
| **Error Handling** | `anyhow`/`thiserror` | 1.0 | Error propagation and custom error types |
| **Logging** | `log`/`env_logger` | 0.4/0.11 | Configurable logging infrastructure |

### Dependency Rationale

**eframe/egui** - Chosen for its immediate mode architecture which naturally fits emulator GUIs. Unlike retained-mode frameworks, egui redraws the entire UI each frame, which aligns perfectly with emulator refresh patterns. The eframe wrapper provides cross-platform window management with OpenGL (glow) backend support for Linux (X11/Wayland), macOS, and Windows.

**cpal** - Selected over SDL2 audio or rodio for several reasons:
- Pure Rust implementation (no C dependencies)
- Direct device access with minimal latency
- Supports custom sample rates and buffer sizes
- Cross-platform without runtime dependencies

**gilrs** - Provides a unified gamepad API across platforms with automatic controller detection, hotplug support, and button/axis mapping.

**rfd** - Native file dialogs that integrate with the system's look and feel, supporting filters and modern dialog features.

**ron** - Rust's native serialization format offers type safety, human readability, and seamless serde integration. Preferred over TOML for complex nested structures.

**directories** - Handles platform-specific configuration paths automatically:
- Linux: `~/.config/rustynes/`
- macOS: `~/Library/Application Support/rustynes/`
- Windows: `%APPDATA%\rustynes\`

---

## Architecture Decisions

### Why eframe over pixels+egui-wgpu+winit

The initial v0.5.0-v0.6.0 implementation used Iced with wgpu. Version 0.7.0 switched to eframe+egui for the following reasons:

1. **Version Compatibility**: wgpu ecosystem had significant version conflicts between Iced 0.13, egui-wgpu, and winit. eframe bundles compatible versions internally.

2. **Simpler Integration**: eframe provides a complete solution (window + rendering + egui) versus manually integrating pixels, winit, and egui-wgpu.

3. **Maintenance Burden**: Fewer moving parts means fewer breaking changes across dependency updates.

4. **glow Backend**: The OpenGL backend (glow) is mature and widely supported, avoiding wgpu's occasional driver-specific issues.

### Why Immediate Mode GUI

Immediate mode GUI (egui) is particularly well-suited for emulator frontends:

1. **Frame Synchronization**: The emulator already redraws every frame; egui's model matches this naturally
2. **Debug Windows**: Rapidly changing debug data (registers, memory) renders efficiently without state management
3. **Simple State**: No complex widget state management or event callbacks
4. **Hot Reload**: UI changes are immediate; no need to track "dirty" state

### Frame Timing Approach

The application uses an accumulator-based frame timing system:

```rust
const TARGET_FPS: f64 = 60.0988;  // NTSC refresh rate
const FRAME_DURATION: Duration = Duration::from_nanos(16_639_266);  // ~16.64ms

while self.accumulator >= FRAME_DURATION {
    self.accumulator -= FRAME_DURATION;
    self.run_frame();
}
```

This approach:
- Maintains accurate NTSC timing (60.0988 Hz)
- Handles variable host frame rates gracefully
- Allows frame skipping under heavy load
- Prevents audio buffer underruns by keeping emulation in sync

### Audio Buffer Strategy

Audio uses a lock-free ring buffer design:

```
           8192 samples
    +------------------------+
    |  Ring Buffer (Mono)    |
    +------------------------+
         ^              |
         |              v
    [APU Writes]    [cpal Reads]
```

- **Ring Buffer Size**: 8192 mono samples (~185ms at 44.1kHz)
- **Lock-Free Design**: Atomic read/write positions for minimal contention
- **Underrun Handling**: Silence is output when buffer is empty (no audio glitches)
- **Channel Duplication**: Mono samples are duplicated to stereo/multi-channel on output

### Thread Model

RustyNES desktop uses a single-threaded model:

- **Emulation**: Runs on the main thread within the eframe update loop
- **Audio**: cpal callback runs on a separate audio thread, reading from the shared ring buffer
- **GUI**: egui rendering occurs on the main thread

This simplifies synchronization and debugging while maintaining acceptable performance. A multi-threaded model (separate emulation thread) is planned for Phase 2.

---

## Module Structure

### Core Modules

| Module | File | Responsibility |
|--------|------|----------------|
| **Entry Point** | `main.rs` | CLI parsing with clap, eframe initialization, configuration loading |
| **Library Root** | `lib.rs` | Public exports (`NesApp`, `Config`) |
| **Application** | `app.rs` | `eframe::App` implementation, frame loop, texture management |
| **Configuration** | `config.rs` | RON-based settings, platform paths, persistence |
| **Audio** | `audio.rs` | cpal stream setup, ring buffer, volume/mute control |
| **Input** | `input.rs` | Keyboard/gamepad mapping, controller state management |

### GUI Modules

| Module | File | Responsibility |
|--------|------|----------------|
| **GUI State** | `gui/mod.rs` | Window visibility state, FPS counter, render coordination |
| **Menu Bar** | `gui/menu.rs` | File/Emulation/Options/Debug/Help menus |
| **Settings** | `gui/settings.rs` | Configuration dialog with video/audio/input/debug sections |

### Debug Modules

| Module | File | Responsibility |
|--------|------|----------------|
| **CPU Debug** | `gui/debug/cpu.rs` | Register display, status flags, cycle counter |
| **PPU Debug** | `gui/debug/ppu.rs` | Frame info, PPU state (placeholder for pattern tables) |
| **APU Debug** | `gui/debug/apu.rs` | Audio info, sample buffer status, channel overview |
| **Memory Viewer** | `gui/debug/memory.rs` | Hex editor with navigation, ASCII display |

---

## Key Implementation Details

### Frame Rendering

The NES framebuffer is rendered as an egui texture:

```rust
// Constants
pub const NES_WIDTH: usize = 256;
pub const NES_HEIGHT: usize = 240;

// Texture creation/update
let image = ColorImage::from_rgba_unmultiplied(
    [NES_WIDTH, NES_HEIGHT],
    &self.framebuffer,
);

// Use nearest-neighbor filtering for crisp pixels
texture.set(image, TextureOptions::NEAREST);
```

**Scaling Modes**:
- Default: Fit window while maintaining aspect ratio
- 8:7 PAR: Corrects for NES pixel aspect ratio (256 * 8/7 / 240 = ~1.14)
- Integer scaling: Not yet implemented (planned)

**Aspect Ratio Calculation**:
```rust
let nes_aspect = if config.video.pixel_aspect_correction {
    256.0 * (8.0 / 7.0) / 240.0  // ~1.14 (8:7 PAR)
} else {
    256.0 / 240.0  // 1.067 (square pixels)
};
```

### Audio Pipeline

```
Console APU          AudioOutput              cpal Device
    |                     |                        |
    +-- audio_samples() ->+                        |
    |                     +-- write() ------------->
    |                     |   (ring buffer)        |
    +-- clear_samples() ->+                        |
                          |<-- read() -------------+
                          |   (callback thread)    |
                          +-- volume/mute -------->+
```

**Sample Flow**:
1. APU generates samples during `step_frame()`
2. Samples are queued to the ring buffer via `queue_samples()`
3. cpal callback reads samples at device sample rate
4. Volume/mute applied during output
5. Mono samples duplicated to all output channels

**Ring Buffer Implementation**:
```rust
const RING_BUFFER_SIZE: usize = 8192;

struct RingBuffer {
    buffer: Box<[f32; RING_BUFFER_SIZE]>,
    read_pos: AtomicU32,
    write_pos: AtomicU32,
}
```

### Input System

**Button Mapping**:
```rust
pub enum NesButton {
    A      = 0x01,
    B      = 0x02,
    Select = 0x04,
    Start  = 0x08,
    Up     = 0x10,
    Down   = 0x20,
    Left   = 0x40,
    Right  = 0x80,
}
```

**Gamepad Mapping** (gilrs):
- South button (A/Cross) -> NES A
- East button (B/Circle) -> NES B
- Select -> NES Select
- Start -> NES Start
- D-pad/Left stick -> NES D-pad

**Axis Handling**: Analog stick is converted to digital with 0.5 threshold.

### Configuration

Configuration is stored in RON format:

```rust
pub struct Config {
    pub video: VideoConfig,    // scale, fullscreen, vsync, aspect, fps
    pub audio: AudioConfig,    // volume, muted, sample_rate, buffer_size
    pub input: InputConfig,    // player1/player2 keyboard bindings
    pub debug: DebugConfig,    // enabled, show_cpu, show_ppu, etc.
    pub recent_roms: RecentRoms,  // recently opened ROM paths
}
```

**Platform Paths**:
- Linux: `~/.config/rustynes/config.ron`
- macOS: `~/Library/Application Support/rustynes/config.ron`
- Windows: `%APPDATA%\rustynes\config.ron`

---

## Debug Windows

### CPU Debug Window

Displays 6502 CPU state:
- **Registers**: PC, A, X, Y, SP (hex and decimal)
- **Status Register**: P register value
- **Status Flags**: N, V, B, D, I, Z, C with color indicators (green=set, gray=clear)
- **Timing**: Cycle counter, frame number

### PPU Debug Window

Displays PPU information:
- **Frame Info**: Current frame number, total cycles
- Note: Pattern table and nametable visualization requires additional core API support (planned)

### APU Debug Window

Displays audio information:
- **Frame Info**: Frame number, total cycles
- **Sample Buffer**: Number of samples currently buffered
- **Channels**: Overview of the 5 APU channels (Pulse 1, Pulse 2, Triangle, Noise, DMC)
- Note: Detailed channel state requires additional core API support (planned)

### Memory Viewer

Hex editor for CPU address space:
- **Address Navigation**: Jump to address, page up/down buttons
- **Quick Jump**: Preset buttons for $0000, $2000, $8000, $C000
- **Hex Display**: 16 bytes per row with address labels
- **ASCII Display**: Printable character representation
- Note: PPU/OAM memory viewer planned for future release

---

## Building and Running

### Prerequisites

- Rust 1.75 or later
- Platform-specific audio libraries (usually pre-installed):
  - Linux: ALSA development libraries (`libasound2-dev` on Debian/Ubuntu)
  - macOS: Core Audio (built-in)
  - Windows: WASAPI (built-in)

### Build Commands

```bash
# Debug build
cargo build -p rustynes-desktop

# Release build (recommended for playing)
cargo build -p rustynes-desktop --release

# Run directly
cargo run -p rustynes-desktop -- [OPTIONS] [ROM]

# Install to ~/.cargo/bin
cargo install --path crates/rustynes-desktop
```

### Running

```bash
# Launch without ROM (opens file dialog)
cargo run -p rustynes-desktop --release

# Launch with specific ROM
cargo run -p rustynes-desktop --release -- path/to/game.nes

# Launch with debug mode
cargo run -p rustynes-desktop --release -- --debug game.nes

# Launch fullscreen at 4x scale
cargo run -p rustynes-desktop --release -- -f -s 4 game.nes
```

---

## Command Line Arguments

```
rustynes [OPTIONS] [ROM]

Arguments:
  [ROM]  Path to a NES ROM file (.nes)

Options:
  -f, --fullscreen       Start in fullscreen mode
  -s, --scale <SCALE>    Window scale factor (1-8) [default: 3]
  -d, --debug            Enable debug mode (shows debug windows)
  -m, --mute             Mute audio on startup
  -h, --help             Print help
  -V, --version          Print version
```

### Examples

```bash
# Basic usage
rustynes super_mario_bros.nes

# 4x scale, fullscreen, muted
rustynes -f -s 4 -m game.nes

# Debug mode for development
rustynes --debug nestest.nes
```

---

## Keyboard Controls

### NES Controller (Player 1)

| NES Button | Keyboard Key |
|------------|--------------|
| A | Z |
| B | X |
| Select | Right Shift |
| Start | Enter |
| Up | Arrow Up |
| Down | Arrow Down |
| Left | Arrow Left |
| Right | Arrow Right |

### NES Controller (Player 2)

| NES Button | Keyboard Key |
|------------|--------------|
| A | G |
| B | F |
| Select | T |
| Start | Y |
| Up | W |
| Down | S |
| Left | A |
| Right | D |

### Emulator Controls

| Function | Key |
|----------|-----|
| Toggle Debug Mode | F1 |
| Reset Console | F2 |
| Pause/Resume | F3 |
| Toggle Menu | Escape |
| Toggle Mute | M |

### Gamepad Mapping

| Gamepad Button | NES Button |
|----------------|------------|
| South (A/Cross) | A |
| East (B/Circle) | B |
| Select/Back | Select |
| Start | Start |
| D-pad/Left Stick | D-pad |

Gamepads are automatically detected on connection. The first connected gamepad is assigned to Player 1, the second to Player 2.

---

## Configuration File

Configuration is stored in RON format. Default location varies by platform (see [Configuration](#configuration) section).

### Example Configuration

```ron
(
    video: (
        scale: 3,
        fullscreen: false,
        vsync: true,
        pixel_aspect_correction: true,
        show_fps: false,
    ),
    audio: (
        volume: 0.8,
        muted: false,
        sample_rate: 44100,
        buffer_size: 2048,
    ),
    input: (
        player1_keyboard: (
            a: "KeyX",
            b: "KeyZ",
            select: "ShiftRight",
            start: "Enter",
            up: "ArrowUp",
            down: "ArrowDown",
            left: "ArrowLeft",
            right: "ArrowRight",
        ),
        player2_keyboard: (
            a: "KeyG",
            b: "KeyF",
            select: "KeyT",
            start: "KeyY",
            up: "KeyW",
            down: "KeyS",
            left: "KeyA",
            right: "KeyD",
        ),
    ),
    debug: (
        enabled: false,
        show_cpu: false,
        show_ppu: false,
        show_apu: false,
        show_memory: false,
    ),
    recent_roms: (
        paths: [],
        max_entries: 10,
    ),
)
```

### Configuration Options

**Video**:
- `scale`: Window scale factor (1-8)
- `fullscreen`: Start in fullscreen mode
- `vsync`: Enable vertical sync
- `pixel_aspect_correction`: Apply 8:7 pixel aspect ratio
- `show_fps`: Display FPS counter overlay

**Audio**:
- `volume`: Master volume (0.0-1.0)
- `muted`: Mute audio output
- `sample_rate`: Audio sample rate (44100, 48000, 96000)
- `buffer_size`: Audio buffer size in samples (512, 1024, 2048, 4096)

**Input**:
- Keyboard bindings use JavaScript KeyboardEvent.code format
- See [MDN KeyboardEvent.code](https://developer.mozilla.org/en-US/docs/Web/API/KeyboardEvent/code) for valid key names

**Debug**:
- `enabled`: Master toggle for debug mode
- `show_*`: Auto-open specific debug windows on startup

---

## Future Improvements

### Planned Features

- **Save States**: Quick save/load with multiple slots
- **Integer Scaling**: Pixel-perfect scaling for CRT-accurate display
- **Shader Support**: CRT filters, scanlines, curvature effects
- **Recording**: Video/audio capture to common formats
- **Rewind**: Frame-by-frame rewind capability
- **Cheats**: Game Genie code support
- **Netplay**: Rollback-based online multiplayer (using backroll)
- **TAS Tools**: Input recording and playback (FM2 format)
- **RetroAchievements**: Integration with rcheevos

### Known Limitations

- **Audio Latency**: Fixed buffer size; dynamic latency adjustment not implemented
- **No Resampling**: Output device must support 44.1kHz or configured sample rate
- **Single Thread**: Emulation runs on UI thread; heavy games may cause UI lag
- **Debug Windows**: PPU/APU debug windows show limited information pending core API expansion
- **No WASM**: WebAssembly build not yet implemented

### WebAssembly Roadmap

The crate is designed with WASM compatibility in mind:
1. Core emulation (`rustynes-core`) is `no_std` compatible
2. Audio will use Web Audio API
3. Rendering will use WebGL/WebGPU via wgpu
4. Input will use browser gamepad API
5. Configuration will use localStorage

A separate `rustynes-web` crate will provide the WASM frontend.

---

## Contributing

Contributions to rustynes-desktop are welcome. Please follow these guidelines:

### Code Style

- Run `cargo fmt` before committing
- Run `cargo clippy --workspace -- -D warnings` and fix all warnings
- Follow existing code patterns and module organization
- Document public APIs with doc comments

### Testing

```bash
# Run all tests
cargo test -p rustynes-desktop

# Run with verbose output
cargo test -p rustynes-desktop -- --nocapture
```

### Pull Request Guidelines

1. Create a feature branch from `main`
2. Keep commits focused and atomic
3. Use conventional commit messages (`feat:`, `fix:`, `docs:`, etc.)
4. Update documentation for user-facing changes
5. Ensure CI passes before requesting review

### Areas Needing Help

- **PPU Debug Visualization**: Pattern table and nametable rendering
- **Shader Effects**: CRT filters and post-processing
- **Testing**: UI test automation
- **Documentation**: User guide and tutorials
- **Accessibility**: Screen reader support, high contrast themes

---

## License

This crate is dual-licensed under MIT and Apache-2.0. See the repository root for license files.

---

## Related Crates

- [rustynes-core](../rustynes-core/) - Core emulation engine
- [rustynes-cpu](../rustynes-cpu/) - 6502 CPU implementation
- [rustynes-ppu](../rustynes-ppu/) - 2C02 PPU implementation
- [rustynes-apu](../rustynes-apu/) - 2A03 APU implementation
- [rustynes-mappers](../rustynes-mappers/) - Mapper implementations
