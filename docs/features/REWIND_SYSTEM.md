# Rewind System Implementation Guide

Complete reference for implementing frame-by-frame rewind functionality in RustyNES, enabling instant game state reversal.

## Table of Contents

1. [Overview](#overview)
2. [Architecture](#architecture)
3. [State Ring Buffer](#state-ring-buffer)
4. [Compression Strategies](#compression-strategies)
5. [Delta Encoding](#delta-encoding)
6. [Memory Management](#memory-management)
7. [UI Integration](#ui-integration)
8. [Performance Optimization](#performance-optimization)
9. [Configuration](#configuration)
10. [References](#references)

---

## Overview

The rewind system provides instant reversal of game state, allowing players to undo mistakes by holding a rewind button. States are captured every frame and stored in a circular buffer with compression to minimize memory usage.

### Key Features

1. **Frame-by-Frame Reversal**: Smooth backwards playback
2. **Configurable Duration**: 10 seconds to 10+ minutes of history
3. **Efficient Compression**: Delta encoding reduces memory usage by 90%+
4. **Seamless Integration**: Works with save states and achievements
5. **Audio Reversal**: Optional reversed audio playback

### Design Goals

- Sub-frame capture latency
- Minimal memory footprint
- Smooth visual experience
- No impact on emulation accuracy

---

## Architecture

### Component Overview

```
┌─────────────────────────────────────────────────────────────┐
│                     Rewind System                            │
│                                                              │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │   State     │    │  Compress   │    │    Buffer       │  │
│  │  Capturer   │───►│   Engine    │───►│   Manager       │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
│         ▲                                      │            │
│         │                                      ▼            │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────────┐  │
│  │  Emulator   │    │  Decompress │    │    Restore      │  │
│  │    Core     │◄───│   Engine    │◄───│    Handler      │  │
│  └─────────────┘    └─────────────┘    └─────────────────┘  │
└─────────────────────────────────────────────────────────────┘
```

### Core Types

```rust
/// Rewind system main controller
pub struct RewindSystem {
    /// State buffer
    buffer: StateBuffer,

    /// Compression configuration
    compression: CompressionConfig,

    /// Current state
    state: RewindState,

    /// Configuration
    config: RewindConfig,

    /// Statistics
    stats: RewindStats,
}

/// Rewind system state
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum RewindState {
    /// Normal gameplay, capturing states
    Recording,

    /// Actively rewinding
    Rewinding,

    /// Paused (not capturing)
    Paused,

    /// Disabled
    Disabled,
}

/// Rewind configuration
#[derive(Clone)]
pub struct RewindConfig {
    /// Maximum rewind duration in seconds
    pub max_duration_secs: u32,

    /// Capture interval (frames between full states)
    pub keyframe_interval: u32,

    /// Enable delta compression
    pub use_delta_compression: bool,

    /// Memory limit (bytes, 0 = unlimited)
    pub memory_limit: usize,

    /// Enable audio reversal
    pub reverse_audio: bool,

    /// Rewind speed multiplier
    pub rewind_speed: f32,
}

impl Default for RewindConfig {
    fn default() -> Self {
        Self {
            max_duration_secs: 60,      // 1 minute
            keyframe_interval: 60,      // Keyframe every second
            use_delta_compression: true,
            memory_limit: 256 * 1024 * 1024, // 256 MB
            reverse_audio: false,
            rewind_speed: 1.0,
        }
    }
}

/// Rewind statistics
#[derive(Clone, Default)]
pub struct RewindStats {
    /// Total frames captured
    pub frames_captured: u64,

    /// Current buffer size (bytes)
    pub buffer_bytes: usize,

    /// Current buffer duration (frames)
    pub buffer_frames: usize,

    /// Compression ratio
    pub compression_ratio: f32,

    /// Average capture time (microseconds)
    pub avg_capture_us: u32,

    /// Average restore time (microseconds)
    pub avg_restore_us: u32,
}
```

---

## State Ring Buffer

### Ring Buffer Implementation

```rust
/// Circular buffer for rewind states
pub struct StateBuffer {
    /// Storage for state data
    data: Vec<u8>,

    /// Entry metadata
    entries: VecDeque<StateEntry>,

    /// Write position in data buffer
    write_pos: usize,

    /// Total capacity
    capacity: usize,

    /// Current usage
    used: usize,

    /// Frame counter
    frame_counter: u64,
}

/// Metadata for a stored state
#[derive(Clone)]
struct StateEntry {
    /// Frame number
    frame: u64,

    /// Position in data buffer
    offset: usize,

    /// Size of state data
    size: usize,

    /// Is this a keyframe (full state)?
    is_keyframe: bool,

    /// Reference frame for delta (if not keyframe)
    reference_frame: Option<u64>,
}

impl StateBuffer {
    pub fn new(capacity: usize) -> Self {
        Self {
            data: vec![0; capacity],
            entries: VecDeque::new(),
            write_pos: 0,
            capacity,
            used: 0,
            frame_counter: 0,
        }
    }

    /// Push a new state
    pub fn push(&mut self, state_data: &[u8], is_keyframe: bool) {
        let size = state_data.len();

        // Remove old entries if needed
        while self.used + size > self.capacity && !self.entries.is_empty() {
            self.pop_oldest();
        }

        // Calculate write position (wrap around)
        let end_pos = self.write_pos + size;
        if end_pos > self.capacity {
            // Wrap around - need contiguous space
            self.write_pos = 0;
            // Remove entries that would be overwritten
            while let Some(entry) = self.entries.front() {
                if entry.offset < size {
                    self.pop_oldest();
                } else {
                    break;
                }
            }
        }

        // Write data
        self.data[self.write_pos..self.write_pos + size].copy_from_slice(state_data);

        // Add entry
        self.entries.push_back(StateEntry {
            frame: self.frame_counter,
            offset: self.write_pos,
            size,
            is_keyframe,
            reference_frame: if is_keyframe {
                None
            } else {
                self.entries.back().map(|e| e.frame)
            },
        });

        self.write_pos += size;
        self.used += size;
        self.frame_counter += 1;
    }

    /// Pop and return the most recent state
    pub fn pop(&mut self) -> Option<(u64, Vec<u8>)> {
        let entry = self.entries.pop_back()?;
        let data = self.data[entry.offset..entry.offset + entry.size].to_vec();
        self.used -= entry.size;
        self.frame_counter = entry.frame;
        Some((entry.frame, data))
    }

    /// Get state at specific frame
    pub fn get(&self, frame: u64) -> Option<Vec<u8>> {
        self.entries
            .iter()
            .find(|e| e.frame == frame)
            .map(|entry| self.data[entry.offset..entry.offset + entry.size].to_vec())
    }

    /// Get most recent state without removing
    pub fn peek(&self) -> Option<&[u8]> {
        self.entries.back().map(|entry| {
            &self.data[entry.offset..entry.offset + entry.size]
        })
    }

    /// Pop oldest entry
    fn pop_oldest(&mut self) {
        if let Some(entry) = self.entries.pop_front() {
            self.used -= entry.size;
        }
    }

    /// Get number of stored frames
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Get buffer statistics
    pub fn stats(&self) -> BufferStats {
        BufferStats {
            frames: self.entries.len(),
            bytes_used: self.used,
            bytes_capacity: self.capacity,
            oldest_frame: self.entries.front().map(|e| e.frame).unwrap_or(0),
            newest_frame: self.entries.back().map(|e| e.frame).unwrap_or(0),
        }
    }

    /// Clear all stored states
    pub fn clear(&mut self) {
        self.entries.clear();
        self.write_pos = 0;
        self.used = 0;
    }
}

#[derive(Clone, Debug)]
pub struct BufferStats {
    pub frames: usize,
    pub bytes_used: usize,
    pub bytes_capacity: usize,
    pub oldest_frame: u64,
    pub newest_frame: u64,
}
```

---

## Compression Strategies

### Compression Configuration

```rust
/// Compression configuration
#[derive(Clone)]
pub struct CompressionConfig {
    /// Compression algorithm
    pub algorithm: CompressionAlgorithm,

    /// Compression level (1-9)
    pub level: u32,

    /// Enable delta encoding
    pub use_delta: bool,

    /// Delta keyframe interval
    pub keyframe_interval: u32,
}

#[derive(Clone, Copy, Debug)]
pub enum CompressionAlgorithm {
    /// No compression
    None,

    /// LZ4 (fast)
    Lz4,

    /// Zstandard (balanced)
    Zstd,

    /// LZMA (high compression, slow)
    Lzma,
}

impl Default for CompressionConfig {
    fn default() -> Self {
        Self {
            algorithm: CompressionAlgorithm::Lz4,
            level: 1,
            use_delta: true,
            keyframe_interval: 60,
        }
    }
}
```

### Compression Engine

```rust
pub struct CompressionEngine {
    config: CompressionConfig,
}

impl CompressionEngine {
    pub fn new(config: CompressionConfig) -> Self {
        Self { config }
    }

    /// Compress state data
    pub fn compress(&self, data: &[u8]) -> Vec<u8> {
        match self.config.algorithm {
            CompressionAlgorithm::None => data.to_vec(),

            CompressionAlgorithm::Lz4 => {
                lz4_flex::compress_prepend_size(data)
            }

            CompressionAlgorithm::Zstd => {
                zstd::encode_all(data, self.config.level as i32)
                    .unwrap_or_else(|_| data.to_vec())
            }

            CompressionAlgorithm::Lzma => {
                let mut output = Vec::new();
                let mut encoder = lzma::LzmaWriter::new_compressor(
                    &mut output,
                    self.config.level,
                ).unwrap();
                std::io::Write::write_all(&mut encoder, data).unwrap();
                encoder.finish().unwrap();
                output
            }
        }
    }

    /// Decompress state data
    pub fn decompress(&self, data: &[u8]) -> Vec<u8> {
        match self.config.algorithm {
            CompressionAlgorithm::None => data.to_vec(),

            CompressionAlgorithm::Lz4 => {
                lz4_flex::decompress_size_prepended(data)
                    .unwrap_or_else(|_| data.to_vec())
            }

            CompressionAlgorithm::Zstd => {
                zstd::decode_all(data)
                    .unwrap_or_else(|_| data.to_vec())
            }

            CompressionAlgorithm::Lzma => {
                let mut output = Vec::new();
                let mut decoder = lzma::LzmaReader::new_decompressor(data).unwrap();
                std::io::Read::read_to_end(&mut decoder, &mut output).unwrap();
                output
            }
        }
    }
}
```

---

## Delta Encoding

### Delta State System

Delta encoding stores only the differences between frames, dramatically reducing memory usage for slowly-changing states.

```rust
/// Delta encoder for state compression
pub struct DeltaEncoder {
    /// Last keyframe data
    keyframe: Vec<u8>,

    /// Keyframe interval
    keyframe_interval: u32,

    /// Frames since last keyframe
    frames_since_keyframe: u32,
}

impl DeltaEncoder {
    pub fn new(keyframe_interval: u32) -> Self {
        Self {
            keyframe: Vec::new(),
            keyframe_interval,
            frames_since_keyframe: 0,
        }
    }

    /// Encode state as delta or keyframe
    pub fn encode(&mut self, state: &[u8]) -> EncodedState {
        self.frames_since_keyframe += 1;

        if self.keyframe.is_empty() || self.frames_since_keyframe >= self.keyframe_interval {
            // Store as keyframe
            self.keyframe = state.to_vec();
            self.frames_since_keyframe = 0;

            EncodedState {
                data: state.to_vec(),
                is_keyframe: true,
            }
        } else {
            // Store as delta
            let delta = self.compute_delta(&self.keyframe, state);

            EncodedState {
                data: delta,
                is_keyframe: false,
            }
        }
    }

    /// Decode state from delta or keyframe
    pub fn decode(&self, encoded: &EncodedState) -> Vec<u8> {
        if encoded.is_keyframe {
            encoded.data.clone()
        } else {
            self.apply_delta(&self.keyframe, &encoded.data)
        }
    }

    /// Compute delta between two states
    fn compute_delta(&self, base: &[u8], current: &[u8]) -> Vec<u8> {
        let mut delta = Vec::new();
        let mut run_start = 0;
        let mut in_diff = false;

        for i in 0..current.len().min(base.len()) {
            let differs = base[i] != current[i];

            if differs && !in_diff {
                // Start of difference run
                run_start = i;
                in_diff = true;
            } else if !differs && in_diff {
                // End of difference run
                self.write_diff_run(&mut delta, run_start, &current[run_start..i]);
                in_diff = false;
            }
        }

        // Handle trailing difference
        if in_diff {
            self.write_diff_run(&mut delta, run_start, &current[run_start..]);
        }

        // Handle size difference
        if current.len() > base.len() {
            self.write_diff_run(&mut delta, base.len(), &current[base.len()..]);
        }

        delta
    }

    /// Write a difference run to delta buffer
    fn write_diff_run(&self, delta: &mut Vec<u8>, offset: usize, data: &[u8]) {
        // Format: [offset:u32][length:u16][data...]
        delta.extend_from_slice(&(offset as u32).to_le_bytes());
        delta.extend_from_slice(&(data.len() as u16).to_le_bytes());
        delta.extend_from_slice(data);
    }

    /// Apply delta to base state
    fn apply_delta(&self, base: &[u8], delta: &[u8]) -> Vec<u8> {
        let mut result = base.to_vec();
        let mut pos = 0;

        while pos + 6 <= delta.len() {
            let offset = u32::from_le_bytes([
                delta[pos], delta[pos + 1], delta[pos + 2], delta[pos + 3]
            ]) as usize;
            let length = u16::from_le_bytes([
                delta[pos + 4], delta[pos + 5]
            ]) as usize;
            pos += 6;

            if pos + length > delta.len() {
                break;
            }

            // Extend result if needed
            if offset + length > result.len() {
                result.resize(offset + length, 0);
            }

            result[offset..offset + length].copy_from_slice(&delta[pos..pos + length]);
            pos += length;
        }

        result
    }

    /// Update keyframe reference
    pub fn update_keyframe(&mut self, state: &[u8]) {
        self.keyframe = state.to_vec();
        self.frames_since_keyframe = 0;
    }
}

/// Encoded state data
pub struct EncodedState {
    pub data: Vec<u8>,
    pub is_keyframe: bool,
}
```

### XOR-based Delta

Alternative delta encoding using XOR for simpler implementation:

```rust
/// XOR-based delta encoder (simpler, sometimes more efficient)
pub struct XorDeltaEncoder {
    previous: Vec<u8>,
}

impl XorDeltaEncoder {
    pub fn new() -> Self {
        Self {
            previous: Vec::new(),
        }
    }

    /// Encode using XOR delta
    pub fn encode(&mut self, current: &[u8]) -> Vec<u8> {
        if self.previous.is_empty() {
            self.previous = current.to_vec();
            return current.to_vec();
        }

        // XOR with previous
        let mut delta = Vec::with_capacity(current.len());
        for (i, &byte) in current.iter().enumerate() {
            let prev = self.previous.get(i).copied().unwrap_or(0);
            delta.push(byte ^ prev);
        }

        self.previous = current.to_vec();

        // Run-length encode the delta (lots of zeros expected)
        self.rle_encode(&delta)
    }

    /// Decode XOR delta
    pub fn decode(&mut self, delta: &[u8]) -> Vec<u8> {
        let decoded_delta = self.rle_decode(delta);

        let mut current = Vec::with_capacity(decoded_delta.len());
        for (i, &delta_byte) in decoded_delta.iter().enumerate() {
            let prev = self.previous.get(i).copied().unwrap_or(0);
            current.push(delta_byte ^ prev);
        }

        self.previous = current.clone();
        current
    }

    /// Run-length encode
    fn rle_encode(&self, data: &[u8]) -> Vec<u8> {
        let mut encoded = Vec::new();
        let mut i = 0;

        while i < data.len() {
            if data[i] == 0 {
                // Count zeros
                let mut count = 1u16;
                while i + count as usize < data.len()
                    && data[i + count as usize] == 0
                    && count < 0x7FFF
                {
                    count += 1;
                }

                if count >= 3 {
                    // Encode as zero run
                    encoded.push(0x80 | ((count >> 8) as u8));
                    encoded.push(count as u8);
                    i += count as usize;
                } else {
                    encoded.push(data[i]);
                    i += 1;
                }
            } else {
                encoded.push(data[i]);
                i += 1;
            }
        }

        encoded
    }

    /// Run-length decode
    fn rle_decode(&self, encoded: &[u8]) -> Vec<u8> {
        let mut decoded = Vec::new();
        let mut i = 0;

        while i < encoded.len() {
            if encoded[i] & 0x80 != 0 {
                // Zero run
                let count = ((encoded[i] as u16 & 0x7F) << 8) | encoded[i + 1] as u16;
                decoded.extend(std::iter::repeat(0).take(count as usize));
                i += 2;
            } else {
                decoded.push(encoded[i]);
                i += 1;
            }
        }

        decoded
    }
}
```

---

## Memory Management

### Adaptive Memory Management

```rust
/// Adaptive memory manager for rewind buffer
pub struct MemoryManager {
    /// Maximum allowed memory
    limit: usize,

    /// Current usage
    usage: usize,

    /// Memory pressure threshold (0.0 - 1.0)
    pressure_threshold: f32,

    /// Compression level adjustment
    compression_adjustment: CompressionAdjustment,
}

#[derive(Clone, Copy)]
enum CompressionAdjustment {
    /// Normal operation
    Normal,

    /// Increase compression to save memory
    Aggressive,

    /// Memory critical - drop frames
    Critical,
}

impl MemoryManager {
    pub fn new(limit: usize) -> Self {
        Self {
            limit,
            usage: 0,
            pressure_threshold: 0.8,
            compression_adjustment: CompressionAdjustment::Normal,
        }
    }

    /// Update memory usage
    pub fn update_usage(&mut self, bytes: usize) {
        self.usage = bytes;
        self.adjust_compression();
    }

    /// Check if we can allocate more memory
    pub fn can_allocate(&self, bytes: usize) -> bool {
        self.usage + bytes <= self.limit
    }

    /// Get current memory pressure (0.0 - 1.0)
    pub fn pressure(&self) -> f32 {
        self.usage as f32 / self.limit as f32
    }

    /// Adjust compression based on memory pressure
    fn adjust_compression(&mut self) {
        let pressure = self.pressure();

        self.compression_adjustment = if pressure > 0.95 {
            CompressionAdjustment::Critical
        } else if pressure > self.pressure_threshold {
            CompressionAdjustment::Aggressive
        } else {
            CompressionAdjustment::Normal
        };
    }

    /// Get recommended action based on memory pressure
    pub fn get_action(&self) -> MemoryAction {
        match self.compression_adjustment {
            CompressionAdjustment::Normal => MemoryAction::Continue,
            CompressionAdjustment::Aggressive => MemoryAction::IncreaseCompression,
            CompressionAdjustment::Critical => MemoryAction::DropOldestFrames(10),
        }
    }
}

pub enum MemoryAction {
    Continue,
    IncreaseCompression,
    DropOldestFrames(usize),
}
```

---

## UI Integration

### Rewind Controller

```rust
impl RewindSystem {
    /// Create new rewind system
    pub fn new(config: RewindConfig) -> Self {
        let max_frames = config.max_duration_secs * 60; // Assuming 60 FPS
        let estimated_state_size = 4096; // ~4KB per compressed state
        let buffer_size = (max_frames as usize * estimated_state_size)
            .min(config.memory_limit);

        Self {
            buffer: StateBuffer::new(buffer_size),
            compression: CompressionConfig::default(),
            state: RewindState::Recording,
            config,
            stats: RewindStats::default(),
        }
    }

    /// Capture current emulator state
    pub fn capture(&mut self, emulator: &Emulator) {
        if self.state != RewindState::Recording {
            return;
        }

        let start = std::time::Instant::now();

        // Get raw state
        let raw_state = emulator.save_state_raw();

        // Determine if keyframe
        let is_keyframe = self.buffer.len() % self.config.keyframe_interval as usize == 0;

        // Compress
        let compressed = if self.config.use_delta_compression && !is_keyframe {
            // Delta encode then compress
            self.delta_encode_and_compress(&raw_state)
        } else {
            // Just compress
            self.compress(&raw_state)
        };

        // Store
        self.buffer.push(&compressed, is_keyframe);

        // Update stats
        let elapsed = start.elapsed();
        self.stats.frames_captured += 1;
        self.stats.buffer_bytes = self.buffer.stats().bytes_used;
        self.stats.buffer_frames = self.buffer.len();
        self.stats.avg_capture_us = ((self.stats.avg_capture_us as u64 * 7
            + elapsed.as_micros() as u64) / 8) as u32;
    }

    /// Start rewinding
    pub fn start_rewind(&mut self) {
        if self.state == RewindState::Recording && self.buffer.len() > 0 {
            self.state = RewindState::Rewinding;
        }
    }

    /// Stop rewinding and resume recording
    pub fn stop_rewind(&mut self) {
        if self.state == RewindState::Rewinding {
            self.state = RewindState::Recording;
        }
    }

    /// Rewind one frame
    pub fn rewind_frame(&mut self, emulator: &mut Emulator) -> bool {
        if self.state != RewindState::Rewinding {
            return false;
        }

        let start = std::time::Instant::now();

        // Pop and restore state
        if let Some((frame, compressed)) = self.buffer.pop() {
            let raw = self.decompress(&compressed);
            emulator.load_state_raw(&raw);

            // Update stats
            let elapsed = start.elapsed();
            self.stats.avg_restore_us = ((self.stats.avg_restore_us as u64 * 7
                + elapsed.as_micros() as u64) / 8) as u32;

            true
        } else {
            // No more states, stop rewinding
            self.state = RewindState::Recording;
            false
        }
    }

    /// Get rewind progress (0.0 = empty, 1.0 = full)
    pub fn progress(&self) -> f32 {
        let stats = self.buffer.stats();
        if stats.bytes_capacity == 0 {
            0.0
        } else {
            stats.bytes_used as f32 / stats.bytes_capacity as f32
        }
    }

    /// Get duration available (in seconds)
    pub fn duration_available(&self) -> f32 {
        self.buffer.len() as f32 / 60.0
    }

    /// Check if rewinding is possible
    pub fn can_rewind(&self) -> bool {
        self.buffer.len() > 0
    }

    fn compress(&self, data: &[u8]) -> Vec<u8> {
        lz4_flex::compress_prepend_size(data)
    }

    fn decompress(&self, data: &[u8]) -> Vec<u8> {
        lz4_flex::decompress_size_prepended(data)
            .unwrap_or_default()
    }

    fn delta_encode_and_compress(&self, _data: &[u8]) -> Vec<u8> {
        // Implementation would use DeltaEncoder
        Vec::new()
    }
}
```

### UI Rendering

```rust
/// Rewind UI overlay
pub fn render_rewind_ui(ui: &mut egui::Ui, rewind: &RewindSystem) {
    let stats = rewind.buffer.stats();

    // Progress bar
    let progress = rewind.progress();
    ui.horizontal(|ui| {
        ui.label("Rewind Buffer:");
        ui.add(egui::ProgressBar::new(progress)
            .text(format!("{:.1}s / {:.1}s",
                rewind.duration_available(),
                rewind.config.max_duration_secs as f32
            )));
    });

    // Status
    let status_text = match rewind.state {
        RewindState::Recording => "Recording",
        RewindState::Rewinding => "⏪ Rewinding",
        RewindState::Paused => "Paused",
        RewindState::Disabled => "Disabled",
    };

    let status_color = match rewind.state {
        RewindState::Recording => egui::Color32::GREEN,
        RewindState::Rewinding => egui::Color32::YELLOW,
        RewindState::Paused => egui::Color32::GRAY,
        RewindState::Disabled => egui::Color32::RED,
    };

    ui.colored_label(status_color, status_text);

    // Stats (collapsible)
    egui::CollapsingHeader::new("Statistics").show(ui, |ui| {
        ui.label(format!("Frames: {}", stats.frames));
        ui.label(format!("Memory: {:.2} MB / {:.2} MB",
            stats.bytes_used as f64 / 1024.0 / 1024.0,
            stats.bytes_capacity as f64 / 1024.0 / 1024.0
        ));
        ui.label(format!("Capture: {} μs", rewind.stats.avg_capture_us));
        ui.label(format!("Restore: {} μs", rewind.stats.avg_restore_us));
    });
}

/// Rewind visual effect (screen tint during rewind)
pub fn render_rewind_effect(framebuffer: &mut [u8], intensity: f32) {
    // Blue tint during rewind
    let tint_r = (0.8 * (1.0 - intensity * 0.3) * 255.0) as u8;
    let tint_g = (0.8 * (1.0 - intensity * 0.2) * 255.0) as u8;
    let tint_b = 255u8;

    for pixel in framebuffer.chunks_mut(4) {
        pixel[0] = ((pixel[0] as u32 * tint_r as u32) / 255) as u8;
        pixel[1] = ((pixel[1] as u32 * tint_g as u32) / 255) as u8;
        pixel[2] = ((pixel[2] as u32 * tint_b as u32) / 255) as u8;
    }
}
```

---

## Performance Optimization

### Optimized State Capture

```rust
/// Optimized state for rewind (minimal data)
#[repr(C)]
pub struct RewindState {
    /// CPU registers (7 bytes)
    pub cpu: CpuRewindState,

    /// PPU state (minimal)
    pub ppu: PpuRewindState,

    /// RAM (2KB)
    pub ram: [u8; 2048],

    /// VRAM (2KB)
    pub vram: [u8; 2048],

    /// Mapper state (variable)
    pub mapper: Vec<u8>,
}

#[repr(C, packed)]
pub struct CpuRewindState {
    pub pc: u16,
    pub sp: u8,
    pub a: u8,
    pub x: u8,
    pub y: u8,
    pub p: u8,
}

#[repr(C, packed)]
pub struct PpuRewindState {
    pub v: u16,
    pub t: u16,
    pub x: u8,
    pub w: u8,
    pub ctrl: u8,
    pub mask: u8,
    pub status: u8,
    pub oam_addr: u8,
    pub scanline: u16,
    pub cycle: u16,
}

impl RewindState {
    /// Serialize to bytes (zero-copy where possible)
    pub fn to_bytes(&self) -> Vec<u8> {
        let cpu_bytes = unsafe {
            std::slice::from_raw_parts(
                &self.cpu as *const _ as *const u8,
                std::mem::size_of::<CpuRewindState>()
            )
        };

        let ppu_bytes = unsafe {
            std::slice::from_raw_parts(
                &self.ppu as *const _ as *const u8,
                std::mem::size_of::<PpuRewindState>()
            )
        };

        let mut bytes = Vec::with_capacity(
            cpu_bytes.len() + ppu_bytes.len() + self.ram.len() +
            self.vram.len() + self.mapper.len() + 4
        );

        bytes.extend_from_slice(cpu_bytes);
        bytes.extend_from_slice(ppu_bytes);
        bytes.extend_from_slice(&self.ram);
        bytes.extend_from_slice(&self.vram);
        bytes.extend_from_slice(&(self.mapper.len() as u32).to_le_bytes());
        bytes.extend_from_slice(&self.mapper);

        bytes
    }
}
```

### Parallel Compression

```rust
use rayon::prelude::*;

/// Batch compress multiple states in parallel
pub fn batch_compress(states: &[Vec<u8>]) -> Vec<Vec<u8>> {
    states.par_iter()
        .map(|state| lz4_flex::compress_prepend_size(state))
        .collect()
}
```

---

## Configuration

### Settings UI

```rust
pub fn render_rewind_settings(ui: &mut egui::Ui, config: &mut RewindConfig) {
    ui.heading("Rewind Settings");

    ui.horizontal(|ui| {
        ui.label("Duration (seconds):");
        ui.add(egui::Slider::new(&mut config.max_duration_secs, 10..=600));
    });

    ui.horizontal(|ui| {
        ui.label("Memory Limit (MB):");
        let mut mb = config.memory_limit / 1024 / 1024;
        if ui.add(egui::Slider::new(&mut mb, 64..=2048)).changed() {
            config.memory_limit = mb * 1024 * 1024;
        }
    });

    ui.checkbox(&mut config.use_delta_compression, "Delta Compression");

    ui.horizontal(|ui| {
        ui.label("Keyframe Interval:");
        ui.add(egui::Slider::new(&mut config.keyframe_interval, 15..=120));
    });

    ui.checkbox(&mut config.reverse_audio, "Reverse Audio");

    ui.horizontal(|ui| {
        ui.label("Rewind Speed:");
        ui.add(egui::Slider::new(&mut config.rewind_speed, 0.5..=4.0));
    });
}
```

---

## References

### Related Documentation

- [Save State Format](../api/SAVESTATE_FORMAT.md)
- [Memory Map](../bus/MEMORY_MAP.md)
- [Configuration](../api/CONFIGURATION.md)

### Source Files

```
crates/rustynes-core/src/
├── rewind/
│   ├── mod.rs           # Module exports
│   ├── system.rs        # RewindSystem implementation
│   ├── buffer.rs        # StateBuffer ring buffer
│   ├── compression.rs   # Compression engine
│   ├── delta.rs         # Delta encoding
│   └── memory.rs        # Memory management
└── state.rs             # Save state integration
```
