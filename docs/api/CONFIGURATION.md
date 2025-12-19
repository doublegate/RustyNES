# Configuration

**Table of Contents**

- [Overview](#overview)
- [Configuration Structure](#configuration-structure)
- [Emulation Settings](#emulation-settings)
- [Video Settings](#video-settings)
- [Audio Settings](#audio-settings)
- [Input Settings](#input-settings)
- [Advanced Settings](#advanced-settings)
- [Configuration File](#configuration-file)

---

## Overview

RustyNES provides extensive configuration options to balance accuracy, performance, and user experience.

---

## Configuration Structure

### Config Object

```rust
pub struct Config {
    pub emulation: EmulationConfig,
    pub video: VideoConfig,
    pub audio: AudioConfig,
    pub input: InputConfig,
    pub advanced: AdvancedConfig,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            emulation: EmulationConfig::default(),
            video: VideoConfig::default(),
            audio: AudioConfig::default(),
            input: InputConfig::default(),
            advanced: AdvancedConfig::default(),
        }
    }
}
```

### Creating Console with Config

```rust
use rustynes_core::{Console, Rom, Config};

let rom = Rom::from_file("game.nes")?;
let config = Config {
    video: VideoConfig {
        scale: 3,
        filter: Filter::Nearest,
        ..Default::default()
    },
    ..Default::default()
};

let console = Console::new(rom, config);
```

---

## Emulation Settings

```rust
pub struct EmulationConfig {
    /// Emulation speed multiplier (1.0 = 60 FPS NTSC, 0.5 = 30 FPS, 2.0 = 120 FPS)
    pub speed: f32,

    /// Pause emulation
    pub paused: bool,

    /// Region (NTSC 60Hz or PAL 50Hz)
    pub region: Region,

    /// Accurate cycle timing vs. performance
    pub accuracy: AccuracyLevel,

    /// Emulate DPCM/controller conflicts
    pub dmc_conflicts: bool,
}

pub enum Region {
    NTSC, // 60.0988 Hz, 1.789773 MHz
    PAL,  // 50.0070 Hz, 1.662607 MHz
    Dendy, // 50 Hz, NTSC timings
}

pub enum AccuracyLevel {
    /// Cycle-accurate (slowest, most accurate)
    CycleAccurate,

    /// Scanline-accurate (faster, slightly less accurate)
    ScanlineAccurate,

    /// Frame-accurate (fastest, least accurate)
    FrameAccurate,
}

impl Default for EmulationConfig {
    fn default() -> Self {
        Self {
            speed: 1.0,
            paused: false,
            region: Region::NTSC,
            accuracy: AccuracyLevel::CycleAccurate,
            dmc_conflicts: true,
        }
    }
}
```

**Example**:

```rust
config.emulation.speed = 2.0; // 2x speed (120 FPS)
config.emulation.accuracy = AccuracyLevel::ScanlineAccurate; // Performance mode
```

---

## Video Settings

```rust
pub struct VideoConfig {
    /// Display scale (1-10)
    pub scale: u32,

    /// Scaling filter
    pub filter: Filter,

    /// NTSC filter (composite video emulation)
    pub ntsc_filter: Option<NtscConfig>,

    /// Overscan cropping (top, bottom, left, right pixels)
    pub overscan: Overscan,

    /// Palette
    pub palette: Palette,

    /// Sprite limit (8 per scanline)
    pub sprite_limit: bool,
}

pub enum Filter {
    Nearest,     // Sharp pixels (default)
    Linear,      // Smooth scaling
    CrtShader,   // CRT scanline effect
}

pub struct Overscan {
    pub top: u32,
    pub bottom: u32,
    pub left: u32,
    pub right: u32,
}

pub enum Palette {
    Default,     // NES classic palette
    FCEUX,       // FCEUX palette
    Smooth,      // Smooth FBX palette
    Custom(Box<[u8; 192]>), // Custom 64-color palette (RGB888)
}

impl Default for VideoConfig {
    fn default() -> Self {
        Self {
            scale: 3,
            filter: Filter::Nearest,
            ntsc_filter: None,
            overscan: Overscan { top: 0, bottom: 0, left: 0, right: 0 },
            palette: Palette::Default,
            sprite_limit: true,
        }
    }
}
```

**Example**:

```rust
config.video.scale = 4; // 1024x960 output (256*4 x 240*4)
config.video.filter = Filter::CrtShader;
config.video.overscan = Overscan { top: 8, bottom: 8, left: 0, right: 0 };
```

---

## Audio Settings

```rust
pub struct AudioConfig {
    /// Enable audio output
    pub enabled: bool,

    /// Sample rate (Hz)
    pub sample_rate: u32,

    /// Buffer size (samples)
    pub buffer_size: usize,

    /// Master volume (0.0 - 1.0)
    pub volume: f32,

    /// Channel volumes
    pub channel_volumes: ChannelVolumes,

    /// Low-pass filter cutoff (Hz)
    pub lowpass_filter: Option<f32>,
}

pub struct ChannelVolumes {
    pub pulse1: f32,
    pub pulse2: f32,
    pub triangle: f32,
    pub noise: f32,
    pub dmc: f32,
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            sample_rate: 44100,
            buffer_size: 2048,
            volume: 0.5,
            channel_volumes: ChannelVolumes::default(),
            lowpass_filter: Some(14000.0),
        }
    }
}

impl Default for ChannelVolumes {
    fn default() -> Self {
        Self {
            pulse1: 1.0,
            pulse2: 1.0,
            triangle: 1.0,
            noise: 1.0,
            dmc: 1.0,
        }
    }
}
```

**Example**:

```rust
config.audio.volume = 0.8;
config.audio.channel_volumes.dmc = 0.5; // Reduce DMC volume
config.audio.sample_rate = 48000; // Higher quality
```

---

## Input Settings

```rust
pub struct InputConfig {
    /// Controller 1 mapping
    pub controller1: ControllerMapping,

    /// Controller 2 mapping
    pub controller2: ControllerMapping,

    /// Input polling rate (Hz, 0 = per-frame)
    pub poll_rate: u32,
}

pub struct ControllerMapping {
    pub a: KeyCode,
    pub b: KeyCode,
    pub select: KeyCode,
    pub start: KeyCode,
    pub up: KeyCode,
    pub down: KeyCode,
    pub left: KeyCode,
    pub right: KeyCode,
}

impl Default for ControllerMapping {
    fn default() -> Self {
        Self {
            a: KeyCode::Z,
            b: KeyCode::X,
            select: KeyCode::RShift,
            start: KeyCode::Return,
            up: KeyCode::Up,
            down: KeyCode::Down,
            left: KeyCode::Left,
            right: KeyCode::Right,
        }
    }
}
```

**Example**:

```rust
config.input.controller1.a = KeyCode::A;
config.input.controller1.b = KeyCode::S;
```

---

## Advanced Settings

```rust
pub struct AdvancedConfig {
    /// Enable rewind (performance impact)
    pub rewind_enabled: bool,

    /// Rewind buffer size (frames)
    pub rewind_buffer: usize,

    /// Save state compression
    pub compress_savestates: bool,

    /// Overclocking (CPU speed multiplier)
    pub overclock: f32,

    /// Remove sprite flicker (non-authentic)
    pub no_sprite_flicker: bool,

    /// Fast-forward speed cap (0 = unlimited)
    pub fast_forward_cap: u32,
}

impl Default for AdvancedConfig {
    fn default() -> Self {
        Self {
            rewind_enabled: false,
            rewind_buffer: 600, // 10 seconds at 60 FPS
            compress_savestates: true,
            overclock: 1.0,
            no_sprite_flicker: false,
            fast_forward_cap: 300, // 5x speed
        }
    }
}
```

**Example**:

```rust
config.advanced.rewind_enabled = true;
config.advanced.overclock = 2.0; // 2x CPU speed
config.advanced.no_sprite_flicker = true; // Show all sprites
```

---

## Configuration File

### TOML Format

**config.toml**:

```toml
[emulation]
speed = 1.0
region = "NTSC"
accuracy = "CycleAccurate"
dmc_conflicts = true

[video]
scale = 3
filter = "Nearest"
sprite_limit = true

[video.overscan]
top = 8
bottom = 8
left = 0
right = 0

[audio]
enabled = true
sample_rate = 44100
volume = 0.5

[input.controller1]
a = "Z"
b = "X"
select = "RShift"
start = "Return"
up = "Up"
down = "Down"
left = "Left"
right = "Right"

[advanced]
rewind_enabled = false
compress_savestates = true
overclock = 1.0
```

### Loading Configuration

```rust
use serde::{Deserialize, Serialize};

#[derive(Serialize, Deserialize)]
pub struct Config {
    // ... (same as above, with Serialize/Deserialize derives)
}

impl Config {
    pub fn load_from_file(path: &Path) -> Result<Self, ConfigError> {
        let contents = std::fs::read_to_string(path)?;
        let config: Config = toml::from_str(&contents)?;
        Ok(config)
    }

    pub fn save_to_file(&self, path: &Path) -> Result<(), ConfigError> {
        let toml = toml::to_string_pretty(self)?;
        std::fs::write(path, toml)?;
        Ok(())
    }
}
```

**Usage**:

```rust
// Load
let config = Config::load_from_file("config.toml")?;
let console = Console::new(rom, config);

// Modify and save
config.video.scale = 4;
config.save_to_file("config.toml")?;
```

---

## References

- [CORE_API.md](CORE_API.md) - Console API
- [SAVE_STATES.md](SAVE_STATES.md) - Save state management

---

**Related Documents**:

- [BUILD.md](../dev/BUILD.md) - Feature flags
- [ARCHITECTURE.md](../ARCHITECTURE.md) - System design
