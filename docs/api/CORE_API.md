# RustyNES Core API

**Table of Contents**
- [Overview](#overview)
- [Core Library](#core-library)
- [Console API](#console-api)
- [Embedding RustyNES](#embedding-rustynes)
- [Frontend Integration](#frontend-integration)
- [Examples](#examples)
- [Thread Safety](#thread-safety)

---

## Overview

RustyNES is designed as a **library-first** emulator, with the core emulation logic separated from frontend concerns. This allows embedding RustyNES in various applications.

### Crate Structure

```
rustynes/
├── rustynes-core/       # Core emulation library (no dependencies on UI)
├── rustynes-cpu/        # Standalone 6502 CPU
├── rustynes-ppu/        # Standalone PPU
├── rustynes-apu/        # Standalone APU
├── rustynes-mappers/    # Mapper implementations
├── rustynes-desktop/    # Desktop frontend (egui)
├── rustynes-web/        # WebAssembly frontend
└── rustynes-headless/   # Headless CLI (testing, TAS)
```

---

## Core Library

### Adding as Dependency

**Cargo.toml**:
```toml
[dependencies]
rustynes-core = "0.1.0"
```

### Core Types

```rust
use rustynes_core::{Console, Rom, Config, Button};

// Load ROM
let rom_data = std::fs::read("game.nes")?;
let rom = Rom::from_bytes(&rom_data)?;

// Create console
let mut console = Console::new(rom, Config::default());

// Run one frame
console.step_frame();

// Get video output (256x240 RGB pixels)
let framebuffer = console.framebuffer();

// Get audio samples
let audio_samples = console.audio_buffer();
```

---

## Console API

### Console Structure

```rust
pub struct Console {
    cpu: Cpu,
    ppu: Ppu,
    apu: Apu,
    bus: Bus,
    cartridge: Box<dyn Mapper>,

    // State
    master_clock: u64,
    frame_count: u64,
    config: Config,
}
```

### Core Methods

#### Initialization

```rust
impl Console {
    /// Create new console with ROM and configuration
    pub fn new(rom: Rom, config: Config) -> Self {
        // ...
    }

    /// Reset console (power cycle)
    pub fn reset(&mut self) {
        // ...
    }
}
```

#### Execution

```rust
impl Console {
    /// Execute one CPU instruction
    pub fn step(&mut self) -> u8 {
        // Returns: Number of CPU cycles executed
    }

    /// Execute until frame complete (29780 CPU cycles)
    pub fn step_frame(&mut self) {
        // Runs until PPU completes one frame (262 scanlines)
    }

    /// Execute for specific number of CPU cycles
    pub fn step_cycles(&mut self, cycles: u64) {
        // Useful for precise timing control
    }
}
```

#### Input Handling

```rust
impl Console {
    /// Set controller button state
    pub fn set_button(&mut self, controller: Controller, button: Button, pressed: bool) {
        // controller: Controller1 or Controller2
        // button: A, B, Select, Start, Up, Down, Left, Right
    }

    /// Set all button states at once
    pub fn set_controller_state(&mut self, controller: Controller, state: u8) {
        // state: 8-bit button mask
    }
}
```

#### Output Access

```rust
impl Console {
    /// Get current framebuffer (256×240 RGB888)
    pub fn framebuffer(&self) -> &[u8; 256 * 240 * 3] {
        &self.ppu.framebuffer
    }

    /// Get audio samples since last call (cleared after retrieval)
    pub fn audio_buffer(&mut self) -> Vec<f32> {
        std::mem::take(&mut self.apu.sample_buffer)
    }

    /// Get current frame number
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }
}
```

---

## Embedding RustyNES

### Minimal Example

```rust
use rustynes_core::{Console, Rom};
use std::fs;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    // Load ROM
    let rom_data = fs::read("super_mario_bros.nes")?;
    let rom = Rom::from_bytes(&rom_data)?;

    // Create console
    let mut console = Console::new(rom, Default::default());

    // Main loop
    loop {
        // Run one frame (60 FPS NTSC)
        console.step_frame();

        // Get framebuffer and display
        let framebuffer = console.framebuffer();
        display_frame(framebuffer)?;

        // Get audio and play
        let audio = console.audio_buffer();
        play_audio(&audio)?;

        // Handle input (example: keyboard)
        handle_input(&mut console)?;

        // Sleep to maintain 60 FPS
        std::thread::sleep(std::time::Duration::from_millis(16));
    }
}
```

### Advanced Example with Save States

```rust
use rustynes_core::{Console, Rom, SaveState};

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let rom = Rom::from_file("game.nes")?;
    let mut console = Console::new(rom, Default::default());

    // Save state
    let save_state = console.save_state()?;
    fs::write("save.state", &save_state)?;

    // Run for a while
    for _ in 0..600 {
        console.step_frame();
    }

    // Load state (rewind)
    let loaded_state = fs::read("save.state")?;
    console.load_state(&loaded_state)?;

    Ok(())
}
```

---

## Frontend Integration

### Framebuffer Format

**Size**: 256 × 240 pixels
**Format**: RGB888 (3 bytes per pixel)
**Layout**: Row-major, top-to-bottom, left-to-right

```rust
// Access pixel at (x, y)
fn get_pixel(framebuffer: &[u8], x: usize, y: usize) -> (u8, u8, u8) {
    let offset = (y * 256 + x) * 3;
    (framebuffer[offset], framebuffer[offset + 1], framebuffer[offset + 2])
}
```

### Audio Buffer Format

**Sample Rate**: 44100 Hz (configurable)
**Format**: f32 mono
**Range**: -1.0 to 1.0

**Samples per Frame** (60 FPS): ~735 samples

```rust
// Play audio samples
fn play_audio(samples: &[f32]) {
    for sample in samples {
        // Output to audio device
        audio_device.write_sample(*sample);
    }
}
```

### Input Handling

```rust
use rustynes_core::{Console, Controller, Button};

fn handle_keyboard(console: &mut Console, key: Key, pressed: bool) {
    let button = match key {
        Key::Z => Button::A,
        Key::X => Button::B,
        Key::Return => Button::Start,
        Key::RShift => Button::Select,
        Key::Up => Button::Up,
        Key::Down => Button::Down,
        Key::Left => Button::Left,
        Key::Right => Button::Right,
        _ => return,
    };

    console.set_button(Controller::Controller1, button, pressed);
}
```

---

## Examples

### Custom Frontend

```rust
struct MyEmulator {
    console: Console,
    display: DisplayDevice,
    audio: AudioDevice,
}

impl MyEmulator {
    pub fn new(rom_path: &str) -> Result<Self, Error> {
        let rom = Rom::from_file(rom_path)?;
        let console = Console::new(rom, Config::default());

        Ok(Self {
            console,
            display: DisplayDevice::new()?,
            audio: AudioDevice::new()?,
        })
    }

    pub fn run_frame(&mut self) {
        self.console.step_frame();

        // Update display
        let framebuffer = self.console.framebuffer();
        self.display.update(framebuffer);

        // Output audio
        let audio = self.console.audio_buffer();
        self.audio.write(&audio);
    }
}
```

### Headless Testing

```rust
use rustynes_core::Console;

fn test_rom(rom_path: &str) -> Result<(), Error> {
    let rom = Rom::from_file(rom_path)?;
    let mut console = Console::new(rom, Config::default());

    // Run for 10 seconds (600 frames at 60 FPS)
    for frame in 0..600 {
        console.step_frame();

        // Check test status (game-specific)
        let test_result = console.read_cpu(0x6000);
        if test_result == 0x00 {
            println!("Test passed at frame {}", frame);
            return Ok(());
        }
    }

    Err("Test timed out".into())
}
```

---

## Thread Safety

**Important**: `Console` is **NOT** thread-safe by default.

### Single-Threaded Usage (Recommended)

```rust
let mut console = Console::new(rom, config);

// Main loop (single thread)
loop {
    console.step_frame();
    // ...
}
```

### Multi-Threaded Usage (Advanced)

**Wrap in Mutex**:
```rust
use std::sync::{Arc, Mutex};

let console = Arc::new(Mutex::new(Console::new(rom, config)));

// Emulation thread
let console_clone = console.clone();
std::thread::spawn(move || {
    loop {
        let mut console = console_clone.lock().unwrap();
        console.step_frame();
    }
});

// Input thread
std::thread::spawn(move || {
    loop {
        let mut console = console.lock().unwrap();
        console.set_button(Controller::Controller1, Button::A, true);
    }
});
```

**Note**: High contention on mutex may impact performance.

---

## References

- [SAVE_STATES.md](SAVE_STATES.md) - Save state API
- [CONFIGURATION.md](CONFIGURATION.md) - Configuration options
- [ARCHITECTURE.md](../ARCHITECTURE.md) - System architecture

---

**Related Documents**:
- [BUILD.md](../dev/BUILD.md) - Building the library
- [TESTING.md](../dev/TESTING.md) - Testing guidelines
