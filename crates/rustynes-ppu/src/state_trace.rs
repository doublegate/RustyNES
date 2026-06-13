//! Per-PPU-dot state-tracing fixture (Session-10 observability tooling).
//!
//! Records, for every PPU dot inside a caller-specified `(frame_range,
//! scanline_range, dot_range)` window, a [`PpuStateRecord`] that captures
//! the register file, internal scroll latches, sprite-evaluation FSM
//! state, sprite line-up, BG pipeline state, secondary OAM, and a
//! 64-bit FNV-1a hash of primary OAM. The fixture is the PPU-side
//! analogue of `crate::irq_trace` (`rustynes-core::irq_trace`) — same
//! design pattern (linear buffer with overflow counter, public CSV
//! emitter, integration-test consumer) applied to the PPU instead of
//! the bus.
//!
//! # Why
//!
//! The persistent Cascade A sprite-evaluation regression across three
//! Mesen2-faithful `sprite_eval_base_from_OAMADDR` implementation
//! variants (Session 9) indicates the load-bearing failure is
//! intermediate-state corruption — a misaligned eval pass propagating
//! via secondary OAM, sprite shifters, or the sprite-overflow flag —
//! rather than the dirty-flag gating itself. The next investigation
//! session needs runtime-state visibility comparable to Mesen2's
//! debugger: a per-dot trace of the PPU's internal state, diffable
//! against a Mesen2-emitted reference trace from the same input.
//!
//! See `docs/adr/0005-ppu-state-trace.md` for the design rationale
//! and `docs/ppu-trace-tooling.md` for usage.
//!
//! # Feature gating
//!
//! The recording code is gated on the `ppu-state-trace` cargo feature
//! (off by default). When the feature is disabled, [`Ppu::tick`] does
//! NOT call into this module — every byte of overhead is gone via
//! `#[cfg(feature = "ppu-state-trace")]` at the call site. This module
//! itself is feature-gated at the crate root so it does not compile
//! into the default build at all.
//!
//! # Output format
//!
//! Two parallel output formats live here:
//!
//! * **Binary** (default): a 12-byte header (`"RUSTYNES_PPU"` ASCII
//!   magic + 2-byte little-endian schema version) followed by zero or
//!   more 113-byte little-endian-packed [`PpuStateRecord`]s. Mesen2's
//!   Lua reference-trace script (`scripts/mesen2_ppu_trace.lua`)
//!   emits the SAME format so the diff tool can compare both sides
//!   record-for-record.
//! * **CSV**: human-readable, one row per record, header line first.
//!   Same column order as the binary layout for ease of cross-reference.
//!
//! # Usage
//!
//! ```ignore
//! # use rustynes_ppu::Ppu;
//! # use rustynes_ppu::state_trace::{PpuStateTrace, PpuTraceConfig};
//! # let mut ppu = Ppu::new(rustynes_ppu::PpuRegion::Ntsc);
//! let cfg = PpuTraceConfig::visible_only(0..=600);
//! ppu.enable_state_trace(PpuStateTrace::with_capacity(8_000_000, cfg));
//! // ... run ROM ...
//! let trace = ppu.take_state_trace().unwrap();
//! std::fs::write("trace.bin", trace.to_binary()).unwrap();
//! std::fs::write("trace.csv", trace.to_csv()).unwrap();
//! ```

#![allow(dead_code)] // Most surfaces are only exercised when the feature is on.

use alloc::string::String;
use alloc::vec::Vec;
use core::ops::RangeInclusive;

/// Schema version for the binary trace layout. Bump on any
/// breaking change to [`PpuStateRecord`]'s byte layout or to the
/// magic/header format. See [`BINARY_MAGIC`] and
/// [`PpuStateTrace::to_binary`].
///
/// Version history:
///
/// * `1` (2026-05-20): initial Session-10 schema.
pub const PPU_TRACE_SCHEMA_VERSION: u16 = 1;

/// Magic bytes prefixing every binary trace file. ASCII
/// "`RUSTYNES_PPU`" — distinguishes our format from Mesen2 native
/// trace logs and from the `irq_trace` CSV files.
pub const BINARY_MAGIC: &[u8; 12] = b"RUSTYNES_PPU";

/// Length of a single [`PpuStateRecord`] in the packed binary layout.
///
/// Stable for the lifetime of [`PPU_TRACE_SCHEMA_VERSION`].
pub const RECORD_SIZE: usize = 113;

/// Header length (magic + 2-byte schema version + 2-byte
/// reserved-for-flags). Records start at this offset.
pub const HEADER_SIZE: usize = BINARY_MAGIC.len() + 2 + 2;

/// One per-PPU-dot trace record.
///
/// The byte layout matches the binary trace file format: every
/// field is serialized little-endian in declaration order. See
/// [`PpuStateRecord::to_bytes`] for the canonical encoder and
/// `RECORD_SIZE` for the total length.
///
/// Schema version: [`PPU_TRACE_SCHEMA_VERSION`].
#[derive(Clone, Debug, Eq, PartialEq)]
#[allow(clippy::struct_excessive_bools)] // 1-bit PPU latches; not a refactor target.
pub struct PpuStateRecord {
    // === Frame/scanline/dot anchor ===
    /// PPU frame counter at the start of this dot.
    pub frame: u32,
    /// PPU scanline (-1 in pre-render, 0..=239 visible, 240
    /// post-render, 241..=260/310 vblank). Stored signed so the
    /// pre-render line stays representable.
    pub scanline: i16,
    /// PPU dot 0..=340.
    pub dot: u16,

    // === CPU-facing registers ===
    /// PPUCTRL ($2000) snapshot.
    pub ctrl: u8,
    /// PPUMASK ($2001) snapshot.
    pub mask: u8,
    /// PPUSTATUS ($2002) snapshot.
    pub status: u8,
    /// OAMADDR ($2003) snapshot.
    pub oam_addr: u8,

    // === Internal scroll/address registers (loopy v/t/x/w) ===
    /// 15-bit current VRAM address.
    pub v: u16,
    /// 15-bit temporary VRAM address.
    pub t: u16,
    /// 3-bit fine X scroll.
    pub fine_x: u8,
    /// 1-bit write toggle for $2005 / $2006.
    pub w_toggle: bool,

    // === Sprite eval FSM ===
    /// Primary-OAM sprite index 0..=63.
    pub sprite_eval_n: u8,
    /// Per-sprite byte index 0..=3 (drives the buggy `n+m`
    /// overflow increment).
    pub sprite_eval_m: u8,
    /// Number of in-range sprites found so far in this scanline's
    /// eval pass.
    pub sprite_eval_found: u8,
    /// Write index into secondary OAM 0..=31.
    pub sprite_eval_sec_idx: u8,
    /// `true` when the current sprite is still being copied (bytes
    /// 1, 2, 3 land in subsequent even-dot writes).
    pub sprite_eval_copying: bool,
    /// `true` when 8 in-range sprites have been latched and the
    /// FSM is in overflow-detection mode (buggy `n+m` increment
    /// active).
    pub sprite_eval_overflow_search: bool,
    /// `true` when eval has exhausted primary OAM or overflow has
    /// been detected — remaining dots 65-256 idle out.
    pub sprite_eval_done: bool,
    /// Sprite-eval read latch: byte read from primary OAM on odd
    /// cycles, consumed by the immediately-following even-cycle
    /// write into secondary OAM.
    pub sprite_eval_read_latch: u8,

    // === Per-scanline sprite line-up (latched at end of fetch
    //     phase 257-320; valid for the NEXT scanline) ===
    /// Number of sprites loaded for the current scanline.
    pub spr_count: u8,
    /// `true` if sprite 0 is in the current scanline's sprite
    /// line-up.
    pub spr_zero_in_line: bool,
    /// Per-sprite pattern-low shift register.
    pub spr_shift_lo: [u8; 8],
    /// Per-sprite pattern-high shift register.
    pub spr_shift_hi: [u8; 8],
    /// Per-sprite latched attribute byte.
    pub spr_attr: [u8; 8],
    /// Per-sprite X-coordinate counter.
    pub spr_x: [u8; 8],

    // === BG pipeline ===
    /// 16-bit BG pattern low shift register.
    pub bg_shift_lo: u16,
    /// 16-bit BG pattern high shift register.
    pub bg_shift_hi: u16,
    /// 16-bit attribute low shift register.
    pub at_shift_lo: u16,
    /// 16-bit attribute high shift register.
    pub at_shift_hi: u16,
    /// Latched nametable byte from the current 8-cycle fetch
    /// group.
    pub nt_latch: u8,
    /// Latched attribute byte from the current 8-cycle fetch
    /// group.
    pub at_latch: u8,
    /// Latched BG pattern low byte from the current 8-cycle
    /// fetch group.
    pub bg_lo_latch: u8,
    /// Latched BG pattern high byte from the current 8-cycle
    /// fetch group.
    pub bg_hi_latch: u8,

    // === Secondary OAM snapshot (32 bytes) ===
    /// 32-byte secondary OAM (8 sprites × 4 bytes) — the staging
    /// area that the per-dot sprite-eval FSM writes into and the
    /// dots-257..=320 sprite-tile fetcher reads from.
    pub secondary_oam: [u8; 32],

    // === Primary OAM digest ===
    /// FNV-1a 64-bit hash of the 256-byte primary OAM. Full OAM
    /// is too verbose for per-dot capture (256 bytes × 89 480
    /// dots/frame ≈ 23 MB/frame); the hash gives a one-shot
    /// "did OAM change between two dots" comparison.
    pub oam_fnv1a64: u64,

    // === NMI line ===
    /// `true` when the PPU is asserting NMI.
    pub nmi_line: bool,
}

/// Total record size as packed by [`PpuStateRecord::to_bytes`].
///
/// Compile-time sanity check; if the byte layout drifts this
/// would fail to compile (panic in const context).
const _RECORD_SIZE_CHECK: () = assert!(RECORD_SIZE == compute_record_size());

const fn compute_record_size() -> usize {
    // frame(4) + scanline(2) + dot(2)
    let anchor = 4 + 2 + 2;
    // ctrl(1) + mask(1) + status(1) + oam_addr(1)
    let regs = 1 + 1 + 1 + 1;
    // v(2) + t(2) + fine_x(1) + w_toggle(1)
    let scroll = 2 + 2 + 1 + 1;
    // sprite-eval FSM: n, m, found, sec_idx, copying, overflow_search,
    // done, read_latch — 5 u8s + 3 bools = 8 bytes.
    let eval = 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1;
    // spr_count(1) + spr_zero(1) + 4 arrays × 8 = 32
    let spr = 1 + 1 + 32;
    // bg_shift_lo(2) + bg_shift_hi(2) + at_shift_lo(2) + at_shift_hi(2)
    //   + nt_latch(1) + at_latch(1) + bg_lo_latch(1) + bg_hi_latch(1)
    let bg = 2 + 2 + 2 + 2 + 1 + 1 + 1 + 1;
    // 32-byte secondary OAM
    let secondary = 32;
    // OAM FNV-1a (8 bytes) + NMI line (1 byte)
    let tail = 8 + 1;
    anchor + regs + scroll + eval + spr + bg + secondary + tail
}

impl PpuStateRecord {
    /// Pack the record into a fixed-size [`RECORD_SIZE`]-byte
    /// little-endian buffer. The companion decoder is
    /// [`Self::from_bytes`].
    #[must_use]
    pub fn to_bytes(&self) -> [u8; RECORD_SIZE] {
        let mut buf = [0u8; RECORD_SIZE];
        let mut i = 0usize;

        // Helper closures used by the packer.
        let copy_u8 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: u8| {
            buf[*i] = v;
            *i += 1;
        };
        let copy_u16 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: u16| {
            let bytes = v.to_le_bytes();
            buf[*i..*i + 2].copy_from_slice(&bytes);
            *i += 2;
        };
        let copy_i16 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: i16| {
            let bytes = v.to_le_bytes();
            buf[*i..*i + 2].copy_from_slice(&bytes);
            *i += 2;
        };
        let copy_u32 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: u32| {
            let bytes = v.to_le_bytes();
            buf[*i..*i + 4].copy_from_slice(&bytes);
            *i += 4;
        };
        let copy_u64 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: u64| {
            let bytes = v.to_le_bytes();
            buf[*i..*i + 8].copy_from_slice(&bytes);
            *i += 8;
        };
        let copy_bool = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: bool| {
            buf[*i] = u8::from(v);
            *i += 1;
        };
        let copy_arr8 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: &[u8; 8]| {
            buf[*i..*i + 8].copy_from_slice(v);
            *i += 8;
        };
        let copy_arr32 = |buf: &mut [u8; RECORD_SIZE], i: &mut usize, v: &[u8; 32]| {
            buf[*i..*i + 32].copy_from_slice(v);
            *i += 32;
        };

        copy_u32(&mut buf, &mut i, self.frame);
        copy_i16(&mut buf, &mut i, self.scanline);
        copy_u16(&mut buf, &mut i, self.dot);
        copy_u8(&mut buf, &mut i, self.ctrl);
        copy_u8(&mut buf, &mut i, self.mask);
        copy_u8(&mut buf, &mut i, self.status);
        copy_u8(&mut buf, &mut i, self.oam_addr);
        copy_u16(&mut buf, &mut i, self.v);
        copy_u16(&mut buf, &mut i, self.t);
        copy_u8(&mut buf, &mut i, self.fine_x);
        copy_bool(&mut buf, &mut i, self.w_toggle);
        copy_u8(&mut buf, &mut i, self.sprite_eval_n);
        copy_u8(&mut buf, &mut i, self.sprite_eval_m);
        copy_u8(&mut buf, &mut i, self.sprite_eval_found);
        copy_u8(&mut buf, &mut i, self.sprite_eval_sec_idx);
        copy_bool(&mut buf, &mut i, self.sprite_eval_copying);
        copy_bool(&mut buf, &mut i, self.sprite_eval_overflow_search);
        copy_bool(&mut buf, &mut i, self.sprite_eval_done);
        copy_u8(&mut buf, &mut i, self.sprite_eval_read_latch);
        copy_u8(&mut buf, &mut i, self.spr_count);
        copy_bool(&mut buf, &mut i, self.spr_zero_in_line);
        copy_arr8(&mut buf, &mut i, &self.spr_shift_lo);
        copy_arr8(&mut buf, &mut i, &self.spr_shift_hi);
        copy_arr8(&mut buf, &mut i, &self.spr_attr);
        copy_arr8(&mut buf, &mut i, &self.spr_x);
        copy_u16(&mut buf, &mut i, self.bg_shift_lo);
        copy_u16(&mut buf, &mut i, self.bg_shift_hi);
        copy_u16(&mut buf, &mut i, self.at_shift_lo);
        copy_u16(&mut buf, &mut i, self.at_shift_hi);
        copy_u8(&mut buf, &mut i, self.nt_latch);
        copy_u8(&mut buf, &mut i, self.at_latch);
        copy_u8(&mut buf, &mut i, self.bg_lo_latch);
        copy_u8(&mut buf, &mut i, self.bg_hi_latch);
        copy_arr32(&mut buf, &mut i, &self.secondary_oam);
        copy_u64(&mut buf, &mut i, self.oam_fnv1a64);
        copy_bool(&mut buf, &mut i, self.nmi_line);

        debug_assert_eq!(i, RECORD_SIZE, "PpuStateRecord packer underflow/overflow");
        buf
    }

    /// Decode a single record from a [`RECORD_SIZE`]-byte
    /// little-endian buffer.
    ///
    /// Returns `None` if the slice is too short.
    #[must_use]
    #[allow(clippy::too_many_lines)] // Field-by-field decoder is intentionally tabular.
    pub fn from_bytes(buf: &[u8]) -> Option<Self> {
        if buf.len() < RECORD_SIZE {
            return None;
        }
        let mut i = 0usize;
        let read_u8 = |buf: &[u8], i: &mut usize| {
            let v = buf[*i];
            *i += 1;
            v
        };
        let read_u16 = |buf: &[u8], i: &mut usize| {
            let v = u16::from_le_bytes([buf[*i], buf[*i + 1]]);
            *i += 2;
            v
        };
        let read_i16 = |buf: &[u8], i: &mut usize| {
            let v = i16::from_le_bytes([buf[*i], buf[*i + 1]]);
            *i += 2;
            v
        };
        let read_u32 = |buf: &[u8], i: &mut usize| {
            let v = u32::from_le_bytes([buf[*i], buf[*i + 1], buf[*i + 2], buf[*i + 3]]);
            *i += 4;
            v
        };
        let read_u64 = |buf: &[u8], i: &mut usize| {
            let v = u64::from_le_bytes([
                buf[*i],
                buf[*i + 1],
                buf[*i + 2],
                buf[*i + 3],
                buf[*i + 4],
                buf[*i + 5],
                buf[*i + 6],
                buf[*i + 7],
            ]);
            *i += 8;
            v
        };
        let read_bool = |buf: &[u8], i: &mut usize| {
            let v = buf[*i] != 0;
            *i += 1;
            v
        };
        let read_arr8 = |buf: &[u8], i: &mut usize| {
            let mut a = [0u8; 8];
            a.copy_from_slice(&buf[*i..*i + 8]);
            *i += 8;
            a
        };
        let read_arr32 = |buf: &[u8], i: &mut usize| {
            let mut a = [0u8; 32];
            a.copy_from_slice(&buf[*i..*i + 32]);
            *i += 32;
            a
        };

        let frame = read_u32(buf, &mut i);
        let scanline = read_i16(buf, &mut i);
        let dot = read_u16(buf, &mut i);
        let ctrl = read_u8(buf, &mut i);
        let mask = read_u8(buf, &mut i);
        let status = read_u8(buf, &mut i);
        let oam_addr = read_u8(buf, &mut i);
        let v = read_u16(buf, &mut i);
        let t = read_u16(buf, &mut i);
        let fine_x = read_u8(buf, &mut i);
        let w_toggle = read_bool(buf, &mut i);
        let sprite_eval_n = read_u8(buf, &mut i);
        let sprite_eval_m = read_u8(buf, &mut i);
        let sprite_eval_found = read_u8(buf, &mut i);
        let sprite_eval_sec_idx = read_u8(buf, &mut i);
        let sprite_eval_copying = read_bool(buf, &mut i);
        let sprite_eval_overflow_search = read_bool(buf, &mut i);
        let sprite_eval_done = read_bool(buf, &mut i);
        let sprite_eval_read_latch = read_u8(buf, &mut i);
        let spr_count = read_u8(buf, &mut i);
        let spr_zero_in_line = read_bool(buf, &mut i);
        let spr_shift_lo = read_arr8(buf, &mut i);
        let spr_shift_hi = read_arr8(buf, &mut i);
        let spr_attr = read_arr8(buf, &mut i);
        let spr_x = read_arr8(buf, &mut i);
        let bg_shift_lo = read_u16(buf, &mut i);
        let bg_shift_hi = read_u16(buf, &mut i);
        let at_shift_lo = read_u16(buf, &mut i);
        let at_shift_hi = read_u16(buf, &mut i);
        let nt_latch = read_u8(buf, &mut i);
        let at_latch = read_u8(buf, &mut i);
        let bg_lo_latch = read_u8(buf, &mut i);
        let bg_hi_latch = read_u8(buf, &mut i);
        let secondary_oam = read_arr32(buf, &mut i);
        let oam_fnv1a64 = read_u64(buf, &mut i);
        let nmi_line = read_bool(buf, &mut i);
        debug_assert_eq!(i, RECORD_SIZE);

        Some(Self {
            frame,
            scanline,
            dot,
            ctrl,
            mask,
            status,
            oam_addr,
            v,
            t,
            fine_x,
            w_toggle,
            sprite_eval_n,
            sprite_eval_m,
            sprite_eval_found,
            sprite_eval_sec_idx,
            sprite_eval_copying,
            sprite_eval_overflow_search,
            sprite_eval_done,
            sprite_eval_read_latch,
            spr_count,
            spr_zero_in_line,
            spr_shift_lo,
            spr_shift_hi,
            spr_attr,
            spr_x,
            bg_shift_lo,
            bg_shift_hi,
            at_shift_lo,
            at_shift_hi,
            nt_latch,
            at_latch,
            bg_lo_latch,
            bg_hi_latch,
            secondary_oam,
            oam_fnv1a64,
            nmi_line,
        })
    }
}

/// Per-trace filter configuration.
///
/// All filters are inclusive ranges; a `None` filter means "no
/// restriction on this axis." The intersection of the three
/// ranges is what gets recorded — a dot is captured iff its
/// `(frame, scanline, dot)` lies in ALL three ranges.
///
/// Sensible defaults:
///
/// * [`Self::all`] — record every dot of every scanline of every
///   frame (millions of records, large files; useful for tiny
///   stress tests).
/// * [`Self::visible_only`] — record dots 0..=340 of scanlines
///   0..=239 (visible field only); skip post-render + vblank +
///   pre-render. Roughly 80 kB/frame.
/// * [`Self::sprite_eval_window`] — record dots 64..=256 of
///   scanlines 0..=239 (the sprite-evaluation window). Roughly
///   46 kB/frame.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct PpuTraceConfig {
    /// Inclusive frame range (`u32` because frames easily exceed
    /// `u16` range over a 600-frame test ROM).
    pub frame_range: RangeInclusive<u32>,
    /// Inclusive scanline range, or `None` for all scanlines.
    /// `i16` carries the `-1` pre-render line.
    pub scanline_range: Option<RangeInclusive<i16>>,
    /// Inclusive dot range, or `None` for all dots (0..=340).
    pub dot_range: Option<RangeInclusive<u16>>,
}

impl PpuTraceConfig {
    /// Record every dot of every scanline of every frame in `frame_range`.
    #[must_use]
    pub const fn all(frame_range: RangeInclusive<u32>) -> Self {
        Self {
            frame_range,
            scanline_range: None,
            dot_range: None,
        }
    }

    /// Record dots 0..=340 of scanlines 0..=239 (visible field
    /// only) of every frame in `frame_range`.
    #[must_use]
    pub const fn visible_only(frame_range: RangeInclusive<u32>) -> Self {
        Self {
            frame_range,
            scanline_range: Some(0..=239),
            dot_range: None,
        }
    }

    /// Record dots 64..=256 of scanlines 0..=239 of every frame
    /// in `frame_range`. Useful for the sprite-evaluation window
    /// investigation.
    #[must_use]
    pub const fn sprite_eval_window(frame_range: RangeInclusive<u32>) -> Self {
        Self {
            frame_range,
            scanline_range: Some(0..=239),
            dot_range: Some(64..=256),
        }
    }

    /// Returns `true` if the given `(frame, scanline, dot)` is
    /// inside the filter window.
    #[must_use]
    pub fn contains(&self, frame: u32, scanline: i16, dot: u16) -> bool {
        if !self.frame_range.contains(&frame) {
            return false;
        }
        if let Some(r) = self.scanline_range.as_ref() {
            if !r.contains(&scanline) {
                return false;
            }
        }
        if let Some(r) = self.dot_range.as_ref() {
            if !r.contains(&dot) {
                return false;
            }
        }
        true
    }
}

/// Per-PPU-dot state trace.
///
/// Linear buffer bounded at `capacity`: records past the cap are
/// silently dropped (`overflow` counter advances). Sized
/// generously by the caller — a full-visible-frame run is ~80 kB
/// of records per frame, and a 600-frame `AccuracyCoin` run with
/// visible-only filtering needs ~48 MB.
#[derive(Debug)]
pub struct PpuStateTrace {
    records: Vec<PpuStateRecord>,
    capacity: usize,
    overflow: u64,
    config: PpuTraceConfig,
}

impl PpuStateTrace {
    /// Allocate a trace buffer with `capacity` records and the
    /// given filter config.
    #[must_use]
    pub fn with_capacity(capacity: usize, config: PpuTraceConfig) -> Self {
        Self {
            records: Vec::with_capacity(capacity),
            capacity,
            overflow: 0,
            config,
        }
    }

    /// Number of records captured so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// True if no records have been captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Number of records dropped because the buffer was full.
    #[must_use]
    pub const fn overflow(&self) -> u64 {
        self.overflow
    }

    /// Borrow the filter config.
    #[must_use]
    pub const fn config(&self) -> &PpuTraceConfig {
        &self.config
    }

    /// Borrow the records.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec::as_slice is not const-stable yet.
    pub fn records(&self) -> &[PpuStateRecord] {
        &self.records
    }

    /// Push a new record IF it passes the filter and the buffer
    /// isn't full. Silently drops otherwise. Called per-dot from
    /// [`crate::Ppu::tick`] when the `ppu-state-trace` feature is
    /// enabled.
    pub fn maybe_push(&mut self, rec: PpuStateRecord) {
        if !self.config.contains(rec.frame, rec.scanline, rec.dot) {
            return;
        }
        if self.records.len() < self.capacity {
            self.records.push(rec);
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    /// Render the trace as binary bytes: a 16-byte header
    /// (magic + schema-version + reserved) followed by zero or
    /// more [`RECORD_SIZE`]-byte records.
    #[must_use]
    pub fn to_binary(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(HEADER_SIZE + self.records.len() * RECORD_SIZE);
        out.extend_from_slice(BINARY_MAGIC);
        out.extend_from_slice(&PPU_TRACE_SCHEMA_VERSION.to_le_bytes());
        // 2 reserved flag bytes — must be zero in the current
        // schema. Bumped only by a future feature-flag addition,
        // which must also bump [`PPU_TRACE_SCHEMA_VERSION`].
        out.extend_from_slice(&[0u8, 0u8]);
        for r in &self.records {
            out.extend_from_slice(&r.to_bytes());
        }
        out
    }

    /// Decode a binary trace into a [`PpuStateTrace`].
    ///
    /// `cfg` is stored as the trace's filter for downstream
    /// tooling that may want to know what the original capture
    /// window was; defaults to [`PpuTraceConfig::all`] over the
    /// frame range observed in the records.
    ///
    /// # Errors
    ///
    /// Returns a string describing the parse failure: too short,
    /// bad magic, schema-version mismatch, or trailing bytes.
    pub fn from_binary(buf: &[u8]) -> Result<Self, String> {
        use core::fmt::Write as _;
        if buf.len() < HEADER_SIZE {
            return Err(alloc::format!(
                "trace too short: {} bytes (need at least {})",
                buf.len(),
                HEADER_SIZE
            ));
        }
        if &buf[..BINARY_MAGIC.len()] != BINARY_MAGIC.as_slice() {
            let mut msg = String::from("bad magic: ");
            for b in &buf[..BINARY_MAGIC.len().min(buf.len())] {
                let _ = write!(&mut msg, "{b:02X} ");
            }
            return Err(msg);
        }
        let ver = u16::from_le_bytes([buf[BINARY_MAGIC.len()], buf[BINARY_MAGIC.len() + 1]]);
        if ver != PPU_TRACE_SCHEMA_VERSION {
            return Err(alloc::format!(
                "schema mismatch: file is v{ver}, this build expects v{PPU_TRACE_SCHEMA_VERSION}"
            ));
        }
        // Skip the 2-byte reserved field.
        let body = &buf[HEADER_SIZE..];
        if body.len() % RECORD_SIZE != 0 {
            return Err(alloc::format!(
                "body length {} is not a multiple of RECORD_SIZE={RECORD_SIZE}",
                body.len()
            ));
        }
        let n = body.len() / RECORD_SIZE;
        let mut records = Vec::with_capacity(n);
        for chunk in body.chunks_exact(RECORD_SIZE) {
            let rec = PpuStateRecord::from_bytes(chunk)
                .ok_or_else(|| String::from("PpuStateRecord::from_bytes returned None"))?;
            records.push(rec);
        }
        let frame_range = records.first().map_or(0..=0, |r| {
            let lo = r.frame;
            let hi = records.last().map_or(lo, |last| last.frame);
            lo..=hi
        });
        Ok(Self {
            records,
            capacity: n,
            overflow: 0,
            config: PpuTraceConfig::all(frame_range),
        })
    }

    /// Render the trace as a UTF-8 CSV string. Header row
    /// included. Column order matches the binary layout for
    /// cross-reference; arrays are emitted as
    /// `0xHH:0xHH:...:0xHH` joined by `|` per field group.
    #[must_use]
    pub fn to_csv(&self) -> String {
        use core::fmt::Write as _;
        let mut out = String::new();
        out.push_str(
            "frame,scanline,dot,\
             ctrl,mask,status,oam_addr,\
             v,t,fine_x,w_toggle,\
             sprite_eval_n,sprite_eval_m,sprite_eval_found,sprite_eval_sec_idx,\
             sprite_eval_copying,sprite_eval_overflow_search,sprite_eval_done,\
             sprite_eval_read_latch,\
             spr_count,spr_zero_in_line,\
             spr_shift_lo,spr_shift_hi,spr_attr,spr_x,\
             bg_shift_lo,bg_shift_hi,at_shift_lo,at_shift_hi,\
             nt_latch,at_latch,bg_lo_latch,bg_hi_latch,\
             secondary_oam,oam_fnv1a64,nmi_line\n",
        );
        let write_arr = |out: &mut String, a: &[u8]| {
            let mut first = true;
            for b in a {
                if !first {
                    out.push(':');
                }
                first = false;
                let _ = write!(out, "{b:02X}");
            }
        };
        for r in &self.records {
            let _ = write!(
                out,
                "{},{},{},{},{},{},{},{:04X},{:04X},{},{},{},{},{},{},{},{},{},{},{},{},",
                r.frame,
                r.scanline,
                r.dot,
                r.ctrl,
                r.mask,
                r.status,
                r.oam_addr,
                r.v,
                r.t,
                r.fine_x,
                u8::from(r.w_toggle),
                r.sprite_eval_n,
                r.sprite_eval_m,
                r.sprite_eval_found,
                r.sprite_eval_sec_idx,
                u8::from(r.sprite_eval_copying),
                u8::from(r.sprite_eval_overflow_search),
                u8::from(r.sprite_eval_done),
                r.sprite_eval_read_latch,
                r.spr_count,
                u8::from(r.spr_zero_in_line),
            );
            write_arr(&mut out, &r.spr_shift_lo);
            out.push(',');
            write_arr(&mut out, &r.spr_shift_hi);
            out.push(',');
            write_arr(&mut out, &r.spr_attr);
            out.push(',');
            write_arr(&mut out, &r.spr_x);
            let _ = write!(
                out,
                ",{:04X},{:04X},{},{},{},{},{},{},",
                r.bg_shift_lo,
                r.bg_shift_hi,
                r.at_shift_lo,
                r.at_shift_hi,
                r.nt_latch,
                r.at_latch,
                r.bg_lo_latch,
                r.bg_hi_latch,
            );
            write_arr(&mut out, &r.secondary_oam);
            let _ = writeln!(out, ",{:016X},{}", r.oam_fnv1a64, u8::from(r.nmi_line));
        }
        out
    }
}

/// Compute the FNV-1a 64-bit hash of `bytes`.
///
/// The OAM hash on every [`PpuStateRecord`] uses this. Public so
/// the Mesen2 Lua reference-trace script can match it exactly
/// (it implements the same offset-basis / prime constants in
/// pure Lua — see `scripts/mesen2_ppu_trace.lua`).
#[must_use]
pub fn fnv1a64(bytes: &[u8]) -> u64 {
    const FNV_OFFSET: u64 = 0xCBF2_9CE4_8422_2325;
    const FNV_PRIME: u64 = 0x0000_0100_0000_01B3;
    let mut h: u64 = FNV_OFFSET;
    for &b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(FNV_PRIME);
    }
    h
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_record() -> PpuStateRecord {
        PpuStateRecord {
            frame: 42,
            scanline: 50,
            dot: 173,
            ctrl: 0x80,
            mask: 0x1E,
            status: 0xA0,
            oam_addr: 0x10,
            v: 0x2FCE,
            t: 0x00CE,
            fine_x: 3,
            w_toggle: true,
            sprite_eval_n: 12,
            sprite_eval_m: 1,
            sprite_eval_found: 4,
            sprite_eval_sec_idx: 16,
            sprite_eval_copying: true,
            sprite_eval_overflow_search: false,
            sprite_eval_done: false,
            sprite_eval_read_latch: 0x77,
            spr_count: 5,
            spr_zero_in_line: true,
            spr_shift_lo: [1, 2, 3, 4, 5, 6, 7, 8],
            spr_shift_hi: [9, 10, 11, 12, 13, 14, 15, 16],
            spr_attr: [0x20, 0x40, 0x80, 0xC0, 0x21, 0x41, 0x81, 0xC1],
            spr_x: [10, 20, 30, 40, 50, 60, 70, 80],
            bg_shift_lo: 0xABCD,
            bg_shift_hi: 0xEF01,
            at_shift_lo: 0xAA,
            at_shift_hi: 0x55,
            nt_latch: 0x42,
            at_latch: 0x03,
            bg_lo_latch: 0x3C,
            bg_hi_latch: 0xC3,
            secondary_oam: [
                0x10, 0x20, 0x30, 0x40, 0x50, 0x60, 0x70, 0x80, 0x90, 0xA0, 0xB0, 0xC0, 0xD0, 0xE0,
                0xF0, 0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08, 0x09, 0x0A, 0x0B, 0x0C, 0x0D,
                0x0E, 0x0F, 0x11, 0x22,
            ],
            oam_fnv1a64: 0x1122_3344_5566_7788,
            nmi_line: true,
        }
    }

    #[test]
    fn record_size_constants_agree() {
        let bytes = sample_record().to_bytes();
        assert_eq!(bytes.len(), RECORD_SIZE);
    }

    #[test]
    fn record_roundtrips_through_packed_bytes() {
        let original = sample_record();
        let bytes = original.to_bytes();
        let decoded = PpuStateRecord::from_bytes(&bytes).expect("decode");
        assert_eq!(original, decoded);
    }

    #[test]
    fn fnv1a64_matches_known_vector() {
        // FNV-1a of the empty string is the offset basis.
        assert_eq!(fnv1a64(b""), 0xCBF2_9CE4_8422_2325);
        // FNV-1a of "foobar" — verified against the canonical
        // FNV reference implementation
        // (http://www.isthe.com/chongo/tech/comp/fnv/).
        assert_eq!(fnv1a64(b"foobar"), 0x8594_4171_F739_67E8);
    }

    #[test]
    fn config_visible_only_filters() {
        let cfg = PpuTraceConfig::visible_only(0..=10);
        assert!(cfg.contains(5, 100, 200));
        assert!(!cfg.contains(11, 100, 200));
        assert!(!cfg.contains(5, 240, 200)); // post-render line
        assert!(!cfg.contains(5, -1, 200)); // pre-render line
    }

    #[test]
    fn config_sprite_eval_window_filters() {
        let cfg = PpuTraceConfig::sprite_eval_window(0..=10);
        assert!(cfg.contains(5, 100, 100));
        assert!(!cfg.contains(5, 100, 50)); // dot < 64
        assert!(!cfg.contains(5, 100, 300)); // dot > 256
    }

    #[test]
    fn maybe_push_respects_filter_and_capacity() {
        let cfg = PpuTraceConfig::all(0..=0);
        let mut trace = PpuStateTrace::with_capacity(2, cfg);
        for i in 0..5u16 {
            let mut r = sample_record();
            r.frame = 0;
            r.dot = i;
            trace.maybe_push(r);
        }
        // Cap = 2 → keep first 2.
        assert_eq!(trace.len(), 2);
        assert_eq!(trace.overflow(), 3);

        // Out-of-window records are dropped, not counted as overflow.
        let mut r = sample_record();
        r.frame = 99; // outside 0..=0
        trace.maybe_push(r);
        assert_eq!(trace.len(), 2);
        assert_eq!(trace.overflow(), 3);
    }

    #[test]
    fn binary_roundtrip_one_record() {
        let cfg = PpuTraceConfig::all(0..=100);
        let mut trace = PpuStateTrace::with_capacity(16, cfg);
        trace.maybe_push(sample_record());
        let bytes = trace.to_binary();
        let parsed = PpuStateTrace::from_binary(&bytes).expect("parse");
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed.records()[0], trace.records()[0]);
    }

    #[test]
    fn binary_rejects_bad_magic() {
        let mut bytes = alloc::vec![0u8; HEADER_SIZE];
        bytes[0] = b'X';
        let err = PpuStateTrace::from_binary(&bytes).unwrap_err();
        assert!(err.contains("bad magic"), "expected bad-magic err: {err}");
    }

    #[test]
    fn binary_rejects_schema_mismatch() {
        let mut bytes = alloc::vec![0u8; HEADER_SIZE];
        bytes[..BINARY_MAGIC.len()].copy_from_slice(BINARY_MAGIC);
        let bogus = (PPU_TRACE_SCHEMA_VERSION + 1).to_le_bytes();
        bytes[BINARY_MAGIC.len()] = bogus[0];
        bytes[BINARY_MAGIC.len() + 1] = bogus[1];
        let err = PpuStateTrace::from_binary(&bytes).unwrap_err();
        assert!(err.contains("schema mismatch"), "got: {err}");
    }

    #[test]
    fn binary_rejects_misaligned_body() {
        let mut bytes = alloc::vec![0u8; HEADER_SIZE + RECORD_SIZE - 1];
        bytes[..BINARY_MAGIC.len()].copy_from_slice(BINARY_MAGIC);
        let ver = PPU_TRACE_SCHEMA_VERSION.to_le_bytes();
        bytes[BINARY_MAGIC.len()] = ver[0];
        bytes[BINARY_MAGIC.len() + 1] = ver[1];
        let err = PpuStateTrace::from_binary(&bytes).unwrap_err();
        assert!(err.contains("not a multiple"), "got: {err}");
    }

    #[test]
    fn csv_header_includes_all_columns() {
        let cfg = PpuTraceConfig::all(0..=0);
        let mut trace = PpuStateTrace::with_capacity(2, cfg);
        trace.maybe_push(sample_record());
        let csv = trace.to_csv();
        let header = csv.lines().next().expect("header");
        for column in [
            "frame",
            "scanline",
            "dot",
            "ctrl",
            "mask",
            "status",
            "oam_addr",
            "v,t,fine_x,w_toggle",
            "sprite_eval_n",
            "spr_count",
            "spr_zero_in_line",
            "secondary_oam",
            "oam_fnv1a64",
            "nmi_line",
        ] {
            assert!(
                header.contains(column),
                "header missing `{column}`: {header}"
            );
        }
    }

    /// Guard against silent layout drift: if the field set
    /// changes the const-time size check above will fail to
    /// compile, but this run-time assertion catches packing
    /// mismatches the const can't see.
    #[test]
    fn binary_layout_size_invariant() {
        let r = sample_record();
        assert_eq!(r.to_bytes().len(), RECORD_SIZE);
        assert_eq!(compute_record_size(), RECORD_SIZE);
    }
}
