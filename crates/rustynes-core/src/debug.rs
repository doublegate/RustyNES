//! Read-only debug snapshots for the frontend's egui debugger UI.
//!
//! Plain-data structs the UI can hold for the lifetime of a frame
//! without borrowing into the emulator. The accessors on [`crate::Nes`]
//! produce these once per visible frame at 60 Hz; a few KiB of cloning
//! is cheap compared to actually running a frame.
//!
//! Determinism contract: opening the debugger must NOT alter the
//! emulator's deterministic state. Every method in this module is
//! read-only.

pub use rustynes_mappers::MapperDebugInfo;

/// Snapshot of CPU register file.
#[derive(Debug, Clone)]
pub struct CpuDebugView {
    /// Accumulator.
    pub a: u8,
    /// X index register.
    pub x: u8,
    /// Y index register.
    pub y: u8,
    /// Stack pointer.
    pub s: u8,
    /// Program counter.
    pub pc: u16,
    /// Raw status flags.
    pub p: u8,
    /// `true` when the CPU is jammed (illegal halt opcode).
    pub jammed: bool,
    /// Cumulative CPU cycle count.
    pub cycles: u64,
}

/// Snapshot of PPU state.
#[derive(Debug, Clone)]
pub struct PpuDebugView {
    /// Current dot (0..=340).
    pub dot: u16,
    /// Current scanline.
    pub scanline: i16,
    /// Current frame count.
    pub frame: u64,
    /// PPUCTRL ($2000) snapshot.
    pub ctrl: u8,
    /// PPUMASK ($2001) snapshot.
    pub mask: u8,
    /// PPUSTATUS ($2002) snapshot.
    pub status: u8,
    /// OAMADDR ($2003) snapshot.
    pub oam_addr: u8,
    /// Loopy `v` register.
    pub v: u16,
    /// Loopy `t` register.
    pub t: u16,
    /// Fine X scroll.
    pub fine_x: u8,
    /// Write toggle (`w`).
    pub w_toggle: bool,
    /// 8x16 sprites enabled.
    pub sprite_size_16: bool,
    /// Background pattern table base (0 or 0x1000).
    pub bg_pattern_base: u16,
    /// Sprite pattern table base (0 or 0x1000).
    pub sprite_pattern_base: u16,
    /// NMI line currently asserted.
    pub nmi_line: bool,
}

/// Snapshot of APU channel outputs and frame counter.
#[derive(Debug, Clone)]
pub struct ApuDebugView {
    /// Pulse 1 raw output (0..=15).
    pub pulse1: u8,
    /// Pulse 2 raw output (0..=15).
    pub pulse2: u8,
    /// Triangle raw output (0..=15).
    pub triangle: u8,
    /// Noise raw output (0..=15).
    pub noise: u8,
    /// DMC output (0..=127).
    pub dmc: u8,
    /// v2.1.6 "Expansion Audio" — the most recent RAW on-cart expansion-audio
    /// sample (VRC6/VRC7/FDS/MMC5/Namco 163/Sunsoft 5B), pre-UI-gain. `0.0`
    /// when the board has no expansion audio. A read-only display tap for the
    /// frontend Audio Mixer expansion oscilloscope / VU meter; sampling it does
    /// not perturb the deterministic mix.
    pub external: f32,
    /// Frame counter IRQ pending.
    pub frame_irq: bool,
    /// DMC IRQ pending.
    pub dmc_irq: bool,
}

/// Snapshot of mapper state — alias for [`rustynes_mappers::MapperDebugInfo`].
pub type MapperDebugView = MapperDebugInfo;
