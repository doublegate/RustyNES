# TAS Movie Support Implementation Guide

Complete reference for implementing Tool-Assisted Speedrun (TAS) movie recording and playback in RustyNES, with FM2 format compatibility.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [FM2 Format Implementation](#fm2-format-implementation)
4. [Input Recording](#input-recording)
5. [Playback System](#playback-system)
6. [Re-recording](#re-recording)
7. [TAS Editor Integration](#tas-editor-integration)
8. [Verification](#verification)
9. [Performance Considerations](#performance-considerations)
10. [References](#references)

---

## Overview

TAS (Tool-Assisted Speedrun/Superplay) support enables frame-perfect input recording, playback, and editing. RustyNES implements FCEUX-compatible FM2 format for maximum compatibility with the TAS community.

### Key Features

1. **FM2 Compatibility**: Full support for FCEUX movie format
2. **Re-recording**: Save state integration for input modification
3. **Input Display**: On-screen visualization of recorded inputs
4. **Verification**: Playback validation and desync detection
5. **TAS Editor**: Piano-roll style input editing interface

### Design Goals

- Frame-perfect determinism
- Minimal performance overhead
- Seamless save state integration
- Accurate timing model

---

## Architecture

### Component Overview

```
┌─────────────────────────────────────────────────────────────┐
│                      TAS System                              │
│                                                              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   Movie     │    │   Input     │    │   TAS Editor    │  │
│  │   Manager   │◄──►│   Buffer    │◄──►│   Interface     │  │
│  └──────┬──────┘    └──────┬──────┘    └────────┬────────┘  │
│         │                  │                    │           │
│         ▼                  ▼                    ▼           │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │    FM2      │    │  Controller │    │   Save State    │  │
│  │   Parser    │    │   Handler   │    │   Integration   │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
└─────────────────────────────────────────────────────────────┘
          │                  │                    │
          ▼                  ▼                    ▼
┌─────────────────────────────────────────────────────────────┐
│                    Emulator Core                             │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │    Input    │    │   Frame     │    │   Determinism   │  │
│  │   Polling   │    │   Advance   │    │     Engine      │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Core Types

```rust
/// TAS movie state
pub struct TasMovie {
    /// Movie metadata
    pub header: MovieHeader,

    /// Frame input data
    pub frames: Vec<FrameInput>,

    /// Current playback position
    pub current_frame: usize,

    /// Movie mode
    pub mode: MovieMode,

    /// Re-record count
    pub rerecord_count: u32,

    /// Greenzone (save states for each frame)
    pub greenzone: Option<Greenzone>,
}

/// Movie operation mode
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MovieMode {
    /// No movie active
    Inactive,

    /// Recording new inputs
    Recording,

    /// Playing back recorded inputs
    Playback,

    /// Finished playback
    Finished,
}

/// Single frame input state
#[derive(Clone, Copy, Debug, Default)]
pub struct FrameInput {
    /// Controller 1 input
    pub controller1: ControllerState,

    /// Controller 2 input
    pub controller2: ControllerState,

    /// Controller 3 input (Four Score)
    pub controller3: Option<ControllerState>,

    /// Controller 4 input (Four Score)
    pub controller4: Option<ControllerState>,

    /// Reset pressed this frame
    pub reset: bool,

    /// Power cycle this frame
    pub power: bool,

    /// FDS disk side change
    pub fds_insert: Option<u8>,

    /// VS System coin insert
    pub vs_coin: bool,

    /// Frame-specific commands
    pub commands: Vec<FrameCommand>,
}

/// Controller state for one frame
#[derive(Clone, Copy, Debug, Default)]
pub struct ControllerState {
    pub a: bool,
    pub b: bool,
    pub select: bool,
    pub start: bool,
    pub up: bool,
    pub down: bool,
    pub left: bool,
    pub right: bool,
}

impl ControllerState {
    /// Convert to byte representation
    pub fn to_byte(&self) -> u8 {
        let mut byte = 0u8;
        if self.right  { byte |= 0x01; }
        if self.left   { byte |= 0x02; }
        if self.down   { byte |= 0x04; }
        if self.up     { byte |= 0x08; }
        if self.start  { byte |= 0x10; }
        if self.select { byte |= 0x20; }
        if self.b      { byte |= 0x40; }
        if self.a      { byte |= 0x80; }
        byte
    }

    /// Create from byte representation
    pub fn from_byte(byte: u8) -> Self {
        Self {
            right:  (byte & 0x01) != 0,
            left:   (byte & 0x02) != 0,
            down:   (byte & 0x04) != 0,
            up:     (byte & 0x08) != 0,
            start:  (byte & 0x10) != 0,
            select: (byte & 0x20) != 0,
            b:      (byte & 0x40) != 0,
            a:      (byte & 0x80) != 0,
        }
    }

    /// Convert to FM2 string format
    pub fn to_fm2_string(&self) -> String {
        let mut s = String::with_capacity(8);
        s.push(if self.right  { 'R' } else { '.' });
        s.push(if self.left   { 'L' } else { '.' });
        s.push(if self.down   { 'D' } else { '.' });
        s.push(if self.up     { 'U' } else { '.' });
        s.push(if self.start  { 'T' } else { '.' });
        s.push(if self.select { 'S' } else { '.' });
        s.push(if self.b      { 'B' } else { '.' });
        s.push(if self.a      { 'A' } else { '.' });
        s
    }

    /// Parse from FM2 string format
    pub fn from_fm2_string(s: &str) -> Option<Self> {
        if s.len() < 8 {
            return None;
        }

        let chars: Vec<char> = s.chars().collect();
        Some(Self {
            right:  chars[0] != '.',
            left:   chars[1] != '.',
            down:   chars[2] != '.',
            up:     chars[3] != '.',
            start:  chars[4] != '.',
            select: chars[5] != '.',
            b:      chars[6] != '.',
            a:      chars[7] != '.',
        })
    }
}
```

---

## FM2 Format Implementation

### Header Parsing

```rust
/// FM2 movie header
#[derive(Clone, Debug)]
pub struct MovieHeader {
    /// Format version
    pub version: u32,

    /// Emulator version that created movie
    pub emulator_version: String,

    /// Re-record count
    pub rerecord_count: u32,

    /// PAL mode flag
    pub pal: bool,

    /// New PPU flag
    pub new_ppu: bool,

    /// FDS flag
    pub fds: bool,

    /// Four Score enabled
    pub four_score: bool,

    /// Port 0 device
    pub port0: PortDevice,

    /// Port 1 device
    pub port1: PortDevice,

    /// Port 2 device (expansion)
    pub port2: PortDevice,

    /// ROM filename
    pub rom_filename: String,

    /// ROM checksum
    pub rom_checksum: String,

    /// Movie title
    pub title: Option<String>,

    /// Author
    pub author: Option<String>,

    /// Comments
    pub comments: Vec<String>,

    /// Subtitle messages
    pub subtitles: Vec<Subtitle>,

    /// Binary savestate (for movies starting from state)
    pub savestate: Option<Vec<u8>>,

    /// SRAM data
    pub sram: Option<Vec<u8>>,

    /// Movie start type
    pub start_type: MovieStartType,

    /// Guid for identification
    pub guid: String,
}

/// Movie start type
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MovieStartType {
    /// Start from power-on
    PowerOn,

    /// Start from reset
    Reset,

    /// Start from savestate
    Savestate,
}

/// Controller port device
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum PortDevice {
    None,
    Gamepad,
    Zapper,
    Arkanoid,
    PowerPad,
}

/// Subtitle entry
#[derive(Clone, Debug)]
pub struct Subtitle {
    pub frame: u32,
    pub text: String,
}
```

### FM2 Parser

```rust
pub struct Fm2Parser;

impl Fm2Parser {
    /// Parse FM2 file
    pub fn parse(content: &str) -> Result<TasMovie, Fm2Error> {
        let mut header = MovieHeader::default();
        let mut frames = Vec::new();
        let mut in_header = true;

        for line in content.lines() {
            let line = line.trim();

            if line.is_empty() {
                continue;
            }

            if line.starts_with('|') {
                // Frame input line
                in_header = false;
                let frame = Self::parse_frame_line(line)?;
                frames.push(frame);
            } else if in_header {
                // Header key-value pair
                Self::parse_header_line(line, &mut header)?;
            }
        }

        Ok(TasMovie {
            header,
            frames,
            current_frame: 0,
            mode: MovieMode::Inactive,
            rerecord_count: 0,
            greenzone: None,
        })
    }

    /// Parse header line (key value)
    fn parse_header_line(line: &str, header: &mut MovieHeader) -> Result<(), Fm2Error> {
        let parts: Vec<&str> = line.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return Ok(()); // Skip malformed lines
        }

        let key = parts[0];
        let value = parts[1];

        match key {
            "version" => header.version = value.parse().unwrap_or(3),
            "emuVersion" => header.emulator_version = value.to_string(),
            "rerecordCount" => header.rerecord_count = value.parse().unwrap_or(0),
            "palFlag" => header.pal = value == "1",
            "NewPPU" => header.new_ppu = value == "1",
            "FDS" => header.fds = value == "1",
            "fourscore" => header.four_score = value == "1",
            "port0" => header.port0 = Self::parse_port_device(value),
            "port1" => header.port1 = Self::parse_port_device(value),
            "port2" => header.port2 = Self::parse_port_device(value),
            "romFilename" => header.rom_filename = value.to_string(),
            "romChecksum" => header.rom_checksum = value.to_string(),
            "comment" => header.comments.push(value.to_string()),
            "subtitle" => {
                if let Some(sub) = Self::parse_subtitle(value) {
                    header.subtitles.push(sub);
                }
            }
            "guid" => header.guid = value.to_string(),
            "savestate" => {
                // Base64 encoded savestate
                if let Ok(data) = base64::decode(value) {
                    header.savestate = Some(data);
                    header.start_type = MovieStartType::Savestate;
                }
            }
            _ => {} // Ignore unknown keys
        }

        Ok(())
    }

    /// Parse frame input line
    fn parse_frame_line(line: &str) -> Result<FrameInput, Fm2Error> {
        // Format: |commands|port0|port1|port2|
        let parts: Vec<&str> = line.split('|').collect();

        if parts.len() < 4 {
            return Err(Fm2Error::InvalidFrameLine);
        }

        let mut frame = FrameInput::default();

        // Parse commands section
        let commands = parts[1];
        for c in commands.chars() {
            match c {
                'R' => frame.reset = true,
                'P' => frame.power = true,
                _ => {}
            }
        }

        // Parse controller 1
        if parts.len() > 2 && !parts[2].is_empty() {
            if let Some(state) = ControllerState::from_fm2_string(parts[2]) {
                frame.controller1 = state;
            }
        }

        // Parse controller 2
        if parts.len() > 3 && !parts[3].is_empty() {
            if let Some(state) = ControllerState::from_fm2_string(parts[3]) {
                frame.controller2 = state;
            }
        }

        Ok(frame)
    }

    fn parse_port_device(value: &str) -> PortDevice {
        match value.parse::<u32>().unwrap_or(0) {
            0 => PortDevice::None,
            1 => PortDevice::Gamepad,
            2 => PortDevice::Zapper,
            _ => PortDevice::None,
        }
    }

    fn parse_subtitle(value: &str) -> Option<Subtitle> {
        let parts: Vec<&str> = value.splitn(2, ' ').collect();
        if parts.len() < 2 {
            return None;
        }

        let frame = parts[0].parse().ok()?;
        let text = parts[1].to_string();

        Some(Subtitle { frame, text })
    }
}
```

### FM2 Writer

```rust
pub struct Fm2Writer;

impl Fm2Writer {
    /// Write movie to FM2 format
    pub fn write(movie: &TasMovie) -> String {
        let mut output = String::new();

        // Write header
        Self::write_header(&movie.header, &mut output);

        // Write frames
        for frame in &movie.frames {
            Self::write_frame(frame, &mut output);
        }

        output
    }

    fn write_header(header: &MovieHeader, output: &mut String) {
        output.push_str(&format!("version {}\n", header.version));
        output.push_str(&format!("emuVersion {}\n", header.emulator_version));
        output.push_str(&format!("rerecordCount {}\n", header.rerecord_count));
        output.push_str(&format!("palFlag {}\n", if header.pal { "1" } else { "0" }));
        output.push_str(&format!("romFilename {}\n", header.rom_filename));
        output.push_str(&format!("romChecksum {}\n", header.rom_checksum));
        output.push_str(&format!("guid {}\n", header.guid));
        output.push_str(&format!("fourscore {}\n", if header.four_score { "1" } else { "0" }));
        output.push_str(&format!("port0 {}\n", Self::port_device_num(&header.port0)));
        output.push_str(&format!("port1 {}\n", Self::port_device_num(&header.port1)));
        output.push_str(&format!("port2 {}\n", Self::port_device_num(&header.port2)));

        if let Some(ref title) = header.title {
            output.push_str(&format!("comment title {}\n", title));
        }

        if let Some(ref author) = header.author {
            output.push_str(&format!("comment author {}\n", author));
        }

        for comment in &header.comments {
            output.push_str(&format!("comment {}\n", comment));
        }

        for subtitle in &header.subtitles {
            output.push_str(&format!("subtitle {} {}\n", subtitle.frame, subtitle.text));
        }

        output.push('\n');
    }

    fn write_frame(frame: &FrameInput, output: &mut String) {
        output.push('|');

        // Commands
        if frame.reset { output.push('R'); }
        if frame.power { output.push('P'); }
        output.push('|');

        // Controller 1
        output.push_str(&frame.controller1.to_fm2_string());
        output.push('|');

        // Controller 2
        output.push_str(&frame.controller2.to_fm2_string());
        output.push('|');

        output.push('\n');
    }

    fn port_device_num(device: &PortDevice) -> u32 {
        match device {
            PortDevice::None => 0,
            PortDevice::Gamepad => 1,
            PortDevice::Zapper => 2,
            PortDevice::Arkanoid => 3,
            PortDevice::PowerPad => 4,
        }
    }
}
```

---

## Input Recording

### Recording System

```rust
pub struct InputRecorder {
    /// Active movie being recorded
    movie: TasMovie,

    /// Frame counter
    frame_count: u32,

    /// Recording start frame
    start_frame: u32,

    /// Save state at recording start
    start_state: Option<SaveState>,
}

impl InputRecorder {
    pub fn new(header: MovieHeader) -> Self {
        Self {
            movie: TasMovie::new(header),
            frame_count: 0,
            start_frame: 0,
            start_state: None,
        }
    }

    /// Start recording from current state
    pub fn start_recording(&mut self, emulator: &Emulator) {
        self.frame_count = emulator.frame_count();
        self.start_frame = self.frame_count;
        self.start_state = Some(emulator.save_state());
        self.movie.mode = MovieMode::Recording;
        self.movie.frames.clear();
    }

    /// Record a frame's input
    pub fn record_frame(&mut self, input: FrameInput) {
        if self.movie.mode != MovieMode::Recording {
            return;
        }

        self.movie.frames.push(input);
        self.frame_count += 1;
    }

    /// Stop recording
    pub fn stop_recording(&mut self) -> &TasMovie {
        self.movie.mode = MovieMode::Inactive;
        &self.movie
    }

    /// Get current recording length
    pub fn length(&self) -> usize {
        self.movie.frames.len()
    }

    /// Truncate recording at current position (for re-recording)
    pub fn truncate_at(&mut self, frame: usize) {
        if frame < self.movie.frames.len() {
            self.movie.frames.truncate(frame);
            self.movie.rerecord_count += 1;
        }
    }
}
```

### Input Capture

```rust
/// Captures input from various sources
pub struct InputCapture {
    /// Current controller state
    controller1: ControllerState,
    controller2: ControllerState,

    /// Input source
    source: InputSource,
}

pub enum InputSource {
    /// Physical controller input
    Hardware,

    /// Movie playback
    Movie,

    /// Lua script
    Script,

    /// Network (netplay)
    Network,
}

impl InputCapture {
    /// Poll current input state
    pub fn poll(&mut self, hardware: &dyn InputDevice) -> FrameInput {
        match self.source {
            InputSource::Hardware => {
                self.controller1 = hardware.read_controller(0);
                self.controller2 = hardware.read_controller(1);
            }
            _ => {} // Other sources handled elsewhere
        }

        FrameInput {
            controller1: self.controller1,
            controller2: self.controller2,
            ..Default::default()
        }
    }

    /// Override input (for TAS editor, scripts)
    pub fn set_input(&mut self, input: FrameInput) {
        self.controller1 = input.controller1;
        self.controller2 = input.controller2;
    }
}
```

---

## Playback System

### Movie Playback

```rust
pub struct MoviePlayer {
    /// Movie being played
    movie: TasMovie,

    /// Current playback frame
    current_frame: usize,

    /// Playback speed (1.0 = normal)
    speed: f32,

    /// Read-only mode (no modifications allowed)
    read_only: bool,

    /// Loop playback
    loop_enabled: bool,
}

impl MoviePlayer {
    pub fn new(movie: TasMovie) -> Self {
        Self {
            movie,
            current_frame: 0,
            speed: 1.0,
            read_only: true,
            loop_enabled: false,
        }
    }

    /// Start playback from beginning
    pub fn start(&mut self, emulator: &mut Emulator) {
        self.current_frame = 0;
        self.movie.mode = MovieMode::Playback;

        // Load start state if present
        if let Some(ref state_data) = self.movie.header.savestate {
            emulator.load_state_data(state_data);
        } else {
            // Power on or reset based on start type
            match self.movie.header.start_type {
                MovieStartType::PowerOn => emulator.power_on(),
                MovieStartType::Reset => emulator.reset(),
                MovieStartType::Savestate => {} // Already handled
            }
        }
    }

    /// Get input for current frame
    pub fn get_current_input(&self) -> Option<FrameInput> {
        self.movie.frames.get(self.current_frame).copied()
    }

    /// Advance to next frame
    pub fn advance(&mut self) -> PlaybackResult {
        if self.current_frame >= self.movie.frames.len() {
            if self.loop_enabled {
                self.current_frame = 0;
                return PlaybackResult::Looped;
            } else {
                self.movie.mode = MovieMode::Finished;
                return PlaybackResult::Finished;
            }
        }

        self.current_frame += 1;
        PlaybackResult::Continue
    }

    /// Seek to specific frame
    pub fn seek(&mut self, frame: usize, emulator: &mut Emulator, greenzone: &Greenzone) {
        if frame >= self.movie.frames.len() {
            return;
        }

        // Find nearest greenzone state
        let state_frame = greenzone.find_nearest_state(frame);

        if let Some(state) = greenzone.get_state(state_frame) {
            emulator.load_state(state);
            self.current_frame = state_frame;

            // Fast-forward to target frame
            while self.current_frame < frame {
                if let Some(input) = self.get_current_input() {
                    emulator.set_input(input);
                    emulator.run_frame();
                }
                self.current_frame += 1;
            }
        }
    }

    /// Check if playback is finished
    pub fn is_finished(&self) -> bool {
        self.movie.mode == MovieMode::Finished
    }

    /// Get playback progress (0.0 - 1.0)
    pub fn progress(&self) -> f32 {
        if self.movie.frames.is_empty() {
            0.0
        } else {
            self.current_frame as f32 / self.movie.frames.len() as f32
        }
    }
}

pub enum PlaybackResult {
    Continue,
    Finished,
    Looped,
}
```

---

## Re-recording

### Greenzone System

The greenzone maintains save states at regular intervals for fast seeking and re-recording.

```rust
/// Greenzone: save states at every N frames for fast seeking
pub struct Greenzone {
    /// Save states indexed by frame
    states: BTreeMap<u32, SaveState>,

    /// Interval between automatic states
    interval: u32,

    /// Maximum number of states to keep
    max_states: usize,

    /// Memory usage estimate (bytes)
    memory_usage: usize,

    /// Compressed storage
    compressed: bool,
}

impl Greenzone {
    pub fn new(interval: u32, max_states: usize) -> Self {
        Self {
            states: BTreeMap::new(),
            interval,
            max_states,
            memory_usage: 0,
            compressed: true,
        }
    }

    /// Capture state at current frame
    pub fn capture(&mut self, frame: u32, state: SaveState) {
        // Only capture at intervals or for specific frames
        if frame % self.interval != 0 {
            return;
        }

        let state_size = state.data.len();

        // Remove oldest states if at capacity
        while self.states.len() >= self.max_states {
            if let Some((&oldest_frame, _)) = self.states.iter().next() {
                if let Some(removed) = self.states.remove(&oldest_frame) {
                    self.memory_usage -= removed.data.len();
                }
            }
        }

        self.memory_usage += state_size;
        self.states.insert(frame, state);
    }

    /// Get state at exact frame
    pub fn get_state(&self, frame: u32) -> Option<&SaveState> {
        self.states.get(&frame)
    }

    /// Find nearest state at or before target frame
    pub fn find_nearest_state(&self, target_frame: usize) -> u32 {
        self.states
            .range(..=(target_frame as u32))
            .next_back()
            .map(|(&frame, _)| frame)
            .unwrap_or(0)
    }

    /// Invalidate states after a given frame (for re-recording)
    pub fn invalidate_after(&mut self, frame: u32) {
        let to_remove: Vec<u32> = self.states
            .range((frame + 1)..)
            .map(|(&f, _)| f)
            .collect();

        for f in to_remove {
            if let Some(removed) = self.states.remove(&f) {
                self.memory_usage -= removed.data.len();
            }
        }
    }

    /// Clear all states
    pub fn clear(&mut self) {
        self.states.clear();
        self.memory_usage = 0;
    }

    /// Get memory usage
    pub fn memory_usage(&self) -> usize {
        self.memory_usage
    }
}
```

### Re-recording Controller

```rust
pub struct RerecordController {
    /// Movie being edited
    movie: TasMovie,

    /// Greenzone for fast seeking
    greenzone: Greenzone,

    /// Current frame position
    current_frame: usize,

    /// Is currently recording?
    recording: bool,

    /// Re-record count
    rerecord_count: u32,
}

impl RerecordController {
    pub fn new(movie: TasMovie) -> Self {
        Self {
            greenzone: Greenzone::new(60, 1000), // State every 60 frames, max 1000
            movie,
            current_frame: 0,
            recording: false,
            rerecord_count: 0,
        }
    }

    /// Start re-recording at current frame
    pub fn start_rerecord(&mut self, frame: usize) {
        if frame < self.movie.frames.len() {
            self.current_frame = frame;
            self.recording = true;
            self.rerecord_count += 1;

            // Truncate movie at this point
            self.movie.frames.truncate(frame);

            // Invalidate greenzone after this frame
            self.greenzone.invalidate_after(frame as u32);
        }
    }

    /// Record input during re-recording
    pub fn record(&mut self, input: FrameInput) {
        if self.recording {
            self.movie.frames.push(input);
            self.current_frame += 1;
        }
    }

    /// Stop re-recording
    pub fn stop_rerecord(&mut self) {
        self.recording = false;
    }

    /// Seek to frame for re-recording
    pub fn seek_to(&mut self, frame: usize, emulator: &mut Emulator) {
        // Find nearest greenzone state
        let state_frame = self.greenzone.find_nearest_state(frame);

        if let Some(state) = self.greenzone.get_state(state_frame) {
            emulator.load_state(state);

            // Replay from state to target frame
            for f in state_frame as usize..frame {
                if let Some(input) = self.movie.frames.get(f) {
                    emulator.set_input(*input);
                    emulator.run_frame();

                    // Capture greenzone state if at interval
                    if f as u32 % self.greenzone.interval == 0 {
                        self.greenzone.capture(f as u32, emulator.save_state());
                    }
                }
            }

            self.current_frame = frame;
        }
    }

    /// Get re-record count
    pub fn rerecord_count(&self) -> u32 {
        self.rerecord_count
    }
}
```

---

## TAS Editor Integration

### Piano Roll Interface

```rust
/// TAS Editor piano roll data
pub struct PianoRoll {
    /// Visible frame range
    visible_start: usize,
    visible_end: usize,

    /// Selected frames
    selection: Selection,

    /// Column visibility
    columns: ColumnConfig,

    /// Markers
    markers: Vec<Marker>,

    /// Branches
    branches: Vec<Branch>,
}

pub struct Selection {
    /// Selected frame range
    frames: std::ops::Range<usize>,

    /// Selected columns (buttons)
    buttons: Vec<Button>,
}

pub struct ColumnConfig {
    pub frame_number: bool,
    pub controller1: bool,
    pub controller2: bool,
    pub lag_indicator: bool,
    pub markers: bool,
}

#[derive(Clone, Debug)]
pub struct Marker {
    pub frame: usize,
    pub note: String,
}

#[derive(Clone)]
pub struct Branch {
    pub name: String,
    pub frames: Vec<FrameInput>,
    pub parent_frame: usize,
}

impl PianoRoll {
    /// Get display data for visible range
    pub fn get_display_data(&self, movie: &TasMovie) -> Vec<RowData> {
        let mut rows = Vec::new();

        for frame in self.visible_start..self.visible_end.min(movie.frames.len()) {
            let input = &movie.frames[frame];

            rows.push(RowData {
                frame,
                controller1: input.controller1,
                controller2: input.controller2,
                selected: self.selection.frames.contains(&frame),
                marker: self.markers.iter().find(|m| m.frame == frame),
            });
        }

        rows
    }

    /// Toggle button at frame
    pub fn toggle_button(&mut self, frame: usize, button: Button, movie: &mut TasMovie) {
        if let Some(input) = movie.frames.get_mut(frame) {
            match button {
                Button::P1A => input.controller1.a = !input.controller1.a,
                Button::P1B => input.controller1.b = !input.controller1.b,
                Button::P1Select => input.controller1.select = !input.controller1.select,
                Button::P1Start => input.controller1.start = !input.controller1.start,
                Button::P1Up => input.controller1.up = !input.controller1.up,
                Button::P1Down => input.controller1.down = !input.controller1.down,
                Button::P1Left => input.controller1.left = !input.controller1.left,
                Button::P1Right => input.controller1.right = !input.controller1.right,
                // ... P2 buttons
                _ => {}
            }
        }
    }

    /// Insert frames at position
    pub fn insert_frames(&mut self, position: usize, count: usize, movie: &mut TasMovie) {
        let new_frames = vec![FrameInput::default(); count];
        movie.frames.splice(position..position, new_frames);
    }

    /// Delete selected frames
    pub fn delete_selection(&mut self, movie: &mut TasMovie) {
        movie.frames.drain(self.selection.frames.clone());
        self.selection.frames = 0..0;
    }

    /// Clone selection
    pub fn clone_selection(&self, movie: &TasMovie) -> Vec<FrameInput> {
        movie.frames[self.selection.frames.clone()].to_vec()
    }

    /// Paste at position
    pub fn paste(&mut self, position: usize, data: Vec<FrameInput>, movie: &mut TasMovie) {
        movie.frames.splice(position..position, data);
    }
}

pub struct RowData<'a> {
    pub frame: usize,
    pub controller1: ControllerState,
    pub controller2: ControllerState,
    pub selected: bool,
    pub marker: Option<&'a Marker>,
}

#[derive(Clone, Copy, Debug)]
pub enum Button {
    P1A, P1B, P1Select, P1Start, P1Up, P1Down, P1Left, P1Right,
    P2A, P2B, P2Select, P2Start, P2Up, P2Down, P2Left, P2Right,
}
```

---

## Verification

### Playback Verification

```rust
pub struct MovieVerifier {
    /// Expected ROM checksum
    expected_checksum: String,

    /// Frame checksums for verification
    frame_checksums: Vec<u64>,

    /// Verification interval
    checksum_interval: u32,
}

impl MovieVerifier {
    pub fn new(movie: &TasMovie) -> Self {
        Self {
            expected_checksum: movie.header.rom_checksum.clone(),
            frame_checksums: Vec::new(),
            checksum_interval: 60, // Checksum every 60 frames
        }
    }

    /// Verify ROM matches movie
    pub fn verify_rom(&self, emulator: &Emulator) -> VerifyResult {
        let actual = emulator.rom_checksum();

        if actual == self.expected_checksum {
            VerifyResult::Ok
        } else {
            VerifyResult::RomMismatch {
                expected: self.expected_checksum.clone(),
                actual,
            }
        }
    }

    /// Record checksum at frame
    pub fn record_checksum(&mut self, frame: u32, emulator: &Emulator) {
        if frame % self.checksum_interval == 0 {
            let checksum = emulator.state_checksum();
            self.frame_checksums.push(checksum);
        }
    }

    /// Verify playback matches recorded checksums
    pub fn verify_checksum(&self, frame: u32, emulator: &Emulator) -> VerifyResult {
        let index = (frame / self.checksum_interval) as usize;

        if let Some(&expected) = self.frame_checksums.get(index) {
            let actual = emulator.state_checksum();

            if actual == expected {
                VerifyResult::Ok
            } else {
                VerifyResult::Desync {
                    frame,
                    expected,
                    actual,
                }
            }
        } else {
            VerifyResult::Ok
        }
    }
}

#[derive(Debug)]
pub enum VerifyResult {
    Ok,
    RomMismatch { expected: String, actual: String },
    Desync { frame: u32, expected: u64, actual: u64 },
}
```

---

## Performance Considerations

### Optimization Strategies

```rust
/// Optimized movie storage
pub struct OptimizedMovie {
    /// Run-length encoded input data
    rle_data: Vec<RleEntry>,

    /// Frame count
    frame_count: usize,

    /// Decompression cache
    cache: LruCache<usize, Vec<FrameInput>>,
}

#[derive(Clone)]
struct RleEntry {
    input: FrameInput,
    count: u32,
}

impl OptimizedMovie {
    /// Compress frame data using RLE
    pub fn compress(frames: &[FrameInput]) -> Self {
        let mut rle_data = Vec::new();
        let mut current: Option<(FrameInput, u32)> = None;

        for frame in frames {
            match &mut current {
                Some((input, count)) if input == frame => {
                    *count += 1;
                }
                Some((input, count)) => {
                    rle_data.push(RleEntry {
                        input: *input,
                        count: *count,
                    });
                    current = Some((*frame, 1));
                }
                None => {
                    current = Some((*frame, 1));
                }
            }
        }

        if let Some((input, count)) = current {
            rle_data.push(RleEntry { input, count });
        }

        Self {
            rle_data,
            frame_count: frames.len(),
            cache: LruCache::new(16),
        }
    }

    /// Get input at frame (with caching)
    pub fn get_frame(&mut self, frame: usize) -> Option<FrameInput> {
        if frame >= self.frame_count {
            return None;
        }

        // Check cache
        let chunk = frame / 1024;
        if !self.cache.contains(&chunk) {
            // Decompress chunk
            let chunk_frames = self.decompress_chunk(chunk);
            self.cache.put(chunk, chunk_frames);
        }

        self.cache.get(&chunk)
            .and_then(|chunk_data| chunk_data.get(frame % 1024))
            .copied()
    }

    fn decompress_chunk(&self, chunk: usize) -> Vec<FrameInput> {
        let start = chunk * 1024;
        let end = ((chunk + 1) * 1024).min(self.frame_count);
        let mut result = Vec::with_capacity(end - start);

        let mut frame = 0;
        for entry in &self.rle_data {
            for _ in 0..entry.count {
                if frame >= start && frame < end {
                    result.push(entry.input);
                }
                frame += 1;
                if frame >= end {
                    break;
                }
            }
            if frame >= end {
                break;
            }
        }

        result
    }
}
```

---

## References

### Related Documentation

- [FM2 Format Specification](../formats/FM2_FORMAT.md)
- [Save State Format](../api/SAVESTATE_FORMAT.md)
- [Input Handling](../input/INPUT_HANDLING.md)

### External Resources

- [FCEUX TAS Documentation](http://www.fceux.com/web/help/taseditor/)
- [TASVideos](https://tasvideos.org/)
- [NESdev: Input](https://www.nesdev.org/wiki/Controller_reading)

### Source Files

```
crates/rustynes-tas/
├── src/
│   ├── lib.rs           # Module exports
│   ├── movie.rs         # TasMovie struct
│   ├── fm2.rs           # FM2 parser/writer
│   ├── recording.rs     # Input recording
│   ├── playback.rs      # Movie playback
│   ├── rerecord.rs      # Re-recording system
│   ├── greenzone.rs     # Greenzone implementation
│   ├── editor.rs        # TAS editor integration
│   └── verify.rs        # Verification system
└── tests/
    ├── fm2_tests.rs     # FM2 format tests
    └── playback_tests.rs
```
