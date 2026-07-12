//! 2C02 PPU core: state, register surface, scanline counter, NMI signaling.
//!
//! See `docs/ppu-2c02.md`. Background and sprite *rendering* (per-dot tile
//! fetch, shift registers, sprite evaluation, sprite-zero hit) is plumbed
//! through this struct but the visible-pixel output path is filled in by
//! Sprints 2-2 and 2-3 — the surface and scanline FSM here is what
//! Sprint 2-1 delivers.

use crate::bus::{BgSplitState, ExAttribute, PpuBus};
use crate::palette::{build_rgba_lut, build_rgba_lut_from_base};
use crate::registers::{PpuCtrl, PpuMask, PpuStatus};
use alloc::boxed::Box;
use alloc::vec;

/// RGBA8 framebuffer length in bytes (256 × 240 × 4).
pub const FRAMEBUFFER_LEN: usize = 256 * 240 * 4;

/// Visible pixel count (256 × 240) — length of the parallel
/// [`Ppu::index_framebuffer`] (one `u16` per pixel).
pub const FRAMEBUFFER_PIXELS: usize = 256 * 240;

/// v1.2.0 beta.2 (Workstream C3) — per-pixel HD-pack tile-source record.
///
/// One entry per visible pixel (parallel to [`Ppu::index_framebuffer`]),
/// populated in the pixel-emit path only when the `hd-pack` cargo feature is
/// enabled. It records the **identity of the CHR tile** that produced the
/// pixel — the 16-byte pattern-table tile base address (in PPU `$0000..=$1FFF`
/// pattern space, fine-Y masked off), the 2-bit attribute/sprite palette, the
/// sprite flip flags, and whether the source was a sprite or the background.
///
/// It is pure output telemetry: it mirrors data the renderer already computed,
/// reads no new VRAM, and changes no emulation state — so it is byte-identical
/// with the feature on or off and is never serialized into a save-state. The
/// frontend's Mesen-style HD-pack loader groups these by 8×8 screen cell, hashes
/// the referenced CHR bytes, and substitutes hi-res replacement tiles at blit
/// time. See `docs/ppu-2c02.md` §HD-pack tile-source export.
#[cfg(feature = "hd-pack")]
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct HdTileSource {
    /// 16-byte CHR tile base address in pattern space (`$0000..=$1FF0`, low
    /// nibble always 0). `tile = (addr >> 4) & 0xFF`, `table = addr & 0x1000`.
    /// `0xFFFF` marks a transparent / universal-background pixel (no tile).
    pub chr_addr: u16,
    /// Final palette group: BG attribute (0..=3) or sprite palette (0..=3).
    pub palette: u8,
    /// `true` when the pixel came from a sprite (vs. the background).
    pub is_sprite: bool,
    /// Sprite horizontal flip (always `false` for BG pixels).
    pub flip_h: bool,
    /// Sprite vertical flip (always `false` for BG pixels).
    pub flip_v: bool,
    /// Mesen `PaletteColors`: the tile's active palette packed as the HD-pack
    /// tile-identity key expects. BG = `pr[base+3] | pr[base+2]<<8 |
    /// pr[base+1]<<16 | pr[0]<<24`; sprite = `0xFF000000 | pr[base+3] |
    /// pr[base+2]<<8 | pr[base+1]<<16` (top byte `0xFF` = the sprite/BG
    /// discriminator). Part of the CHR-RAM/CHR-ROM tile key (HdNesPpu.h:119/167).
    pub palette_colors: u32,
    /// Which texel COLUMN (0..=7) of the 8x8 tile this screen pixel samples —
    /// Mesen `OffsetX` (HdNesPpu.h:172). For BG it folds in fine-X scroll
    /// (`(fineX + (pixel_x & 7)) & 7`); for a sprite it is the column within the
    /// sprite tile. The HD compositor samples the replacement at this column so
    /// the high-def tile tracks the scrolled / sprite position pixel-for-pixel
    /// (flips are applied at sample time). Output-only.
    pub offset_x: u8,
    /// Which texel ROW (0..=7) of the 8x8 tile this screen pixel samples — Mesen
    /// `OffsetY`. BG = fine-Y; sprite = the row within the sprite tile.
    pub offset_y: u8,
    /// Mesen CHR-ROM `TileIndex`: the ABSOLUTE post-banking CHR-ROM tile number
    /// (`chr_phys(addr) / 16`) for a CHR-ROM cart, or [`HD_CHR_RAM`] when CHR is
    /// RAM (the tile is content-hashed instead). The HD-pack key uses
    /// `TileIndex ^ PaletteColors` for CHR-ROM and `CalculateHash(palette ++
    /// data)` for CHR-RAM (Mesen `HdTileKey`). Captured at fetch (the only point
    /// with mapper access). Output-only.
    pub chr_tile_index: u32,
    /// v1.8.9 — the `$2001` grayscale + emphasis bits at this pixel
    /// (`mask.bits() & 0xE1`: bit 0 = grayscale, bits 5-7 = R/G/B emphasis). The
    /// HD compositor re-applies them to the replacement texel (Mesen
    /// `ProcessGrayscaleAndEmphasis`) so HD tiles track grayscale / emphasis fades
    /// like the base frame, which already has them baked in. Output-only.
    pub color_mask: u8,
    /// v1.8.9 — every opaque sprite covering this pixel (up to 4), front-to-back,
    /// for Mesen `spriteAtPosition` / `spriteNearby` (which match ANY covering
    /// sprite, including ones a higher-priority BG occludes). `sprites[0]` is the
    /// front-most. The existing `is_sprite` + tile fields still describe the
    /// VISIBLE pixel (the winning layer); these add the hidden layers. Output-only.
    pub sprites: [HdSprite; 4],
    /// Number of valid entries in [`Self::sprites`] (`0..=4`).
    pub sprite_count: u8,
}

/// One sprite covering a pixel, for the HD-pack multi-sprite conditions
/// (`spriteAtPosition` / `spriteNearby`). Carries just the identity those
/// conditions match on. Output-only telemetry.
#[cfg(feature = "hd-pack")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct HdSprite {
    /// Absolute CHR-ROM tile index (`chr_phys/16`), or [`HD_CHR_RAM`] for CHR-RAM.
    pub chr_tile_index: u32,
    /// The sprite's packed `PaletteColors` (Mesen key form).
    pub palette_colors: u32,
}

/// `chr_tile_index` sentinel meaning "CHR is RAM" — the tile is keyed by its 16
/// CHR bytes (content) rather than by an absolute CHR-ROM tile index.
#[cfg(feature = "hd-pack")]
pub const HD_CHR_RAM: u32 = u32::MAX;

#[cfg(feature = "hd-pack")]
impl Default for HdTileSource {
    /// A blank record: no tile, and the CHR-RAM sentinel (so an unwritten /
    /// default record is content-keyed, never mistaken for CHR-ROM tile 0).
    fn default() -> Self {
        Self {
            chr_addr: 0,
            palette: 0,
            is_sprite: false,
            flip_h: false,
            flip_v: false,
            palette_colors: 0,
            offset_x: 0,
            offset_y: 0,
            chr_tile_index: HD_CHR_RAM,
            color_mask: 0,
            sprites: [HdSprite::default(); 4],
            sprite_count: 0,
        }
    }
}

/// Sentinel `chr_addr` for a transparent / universal-background HD-pack pixel.
#[cfg(feature = "hd-pack")]
pub const HD_TILE_NONE: u16 = 0xFFFF;

/// Diagnostic: capture the (frame, scanline, dot, mask) of `$2007` reads to
/// pin where the `$2007 Stress` test's per-dot reads land vs the visible
/// scanline they target. Gated; default build unaffected.
pub mod read2007_diag {
    use core::sync::atomic::AtomicU32;
    /// Next free slot (count of captured `$2007` reads).
    pub static IDX: AtomicU32 = AtomicU32::new(0);
    /// Packed: `((scanline+1)<<18) | (dot<<5) | (rendering_enabled<<1) | is_render`.
    pub static LOG: [AtomicU32; 1024] = [const { AtomicU32::new(0) }; 1024];
    /// Tunable PPU-dot countdown: `$2007` read-during-rendering to reload.
    ///
    /// The `PPUDATA` state machine reloads `data_buffer` from the fetch cadence
    /// (`TriCNES` `PPU_DATA_StateMachine` latch cascade: read-end -> ALE +2 ->
    /// reload +4 dots; with R1's fixed CPU<->PPU mod-4 phase the landing is a
    /// constant offset from the register-read sample point). 0 = immediate.
    /// Default 5 = the empirical winner (W2 sweep 0-12: 5 -> 170/170 stable
    /// reads on the `$2007 Stress` answer key; 6/7 -> 169; everything else
    /// far below). Env knob `RUSTYNES_2007_DELAY` (wired in the diag bins).
    pub static RENDER_BUFFER_DOT_DELAY: AtomicU32 = AtomicU32::new(5);
    /// Sub-knob: defer the `$2007` v-glitch increment to the `TStep` dot.
    ///
    /// The `TStep` is the SAME dot as the buffer reload — `TriCNES`
    /// `PPU_DATA_StateMachine_Half`: `PPU_2007_TStep = TStep_Latch || PD_RB`
    /// — instead of read time. With the immediate increment, every fetch in
    /// the read-to-reload window uses the post-glitch `v` (coarse-x +1 -> NT
    /// byte = tile+1; fine-y +1 -> PT row+1) — exactly the per-index mismatch
    /// signature the W2 baseline measured. 1 = deferred (default), 0 =
    /// immediate (legacy). Env knob `RUSTYNES_2007_VINC` (wired in the diag
    /// bins).
    pub static RENDER_BUFFER_DEFER_V_INC: AtomicU32 = AtomicU32::new(1);
}

/// v2.0.3 (ADR 0030) octal-latch calibration tracer.
///
/// Entirely a diagnostic: a lock-free ring of packed per-event records captured
/// only when `ENABLE` is set (via the env knob wired in the test harness). It
/// costs nothing in an untraced run — each `push` call site is a single relaxed
/// atomic load + early-return branch, and every call site sits in a cold
/// corruption branch (never the steady-state per-dot path). Records the
/// corruption-relevant events (`$2006`/`$2007` during render, hybrid/stale
/// splices) on scanlines 2-5 so a run can be cross-diffed against the `TriCNES`
/// per-dot bus sequence.
///
/// Gated behind the default-off `ppu-octal-trace` dev feature. Behavior never
/// depends on it — the 2-cycle-ALE fetch model (now the only PPU fetch path, ADR
/// 0030) drives its `push` call sites unconditionally, but with the feature off
/// `push` is a zero-cost no-op ([`stub`](self)) and the ring's 64-bit-atomic
/// storage does not exist, so the shipped hot path is untouched and the
/// `#![no_std]` chip stack (whose `thumbv7em` target lacks 64-bit atomics) still
/// builds. Enable with `--features ppu-octal-trace` to capture a trace.
#[cfg(feature = "ppu-octal-trace")]
pub mod octal_trace {
    use core::sync::atomic::{AtomicU32, AtomicU64};
    /// 1 = capture enabled. Off by default; an untraced flag-on run pays only a
    /// single relaxed atomic load + branch per `push` call site.
    pub static ENABLE: AtomicU32 = AtomicU32::new(0);
    /// Next free slot (saturating; stops at `LOG.len()`).
    pub static IDX: AtomicU32 = AtomicU32::new(0);
    /// Packed record: `(kind<<58) | (frame<<44) | (scanline<<32) | (dot<<20) | value`
    /// where `value` is event-specific (a 20-bit address / latch payload).
    pub static LOG: [AtomicU64; 4096] = [const { AtomicU64::new(0) }; 4096];

    /// Event kind: `$2006` second write during rendering (value = new `v`).
    pub const K_W2006: u64 = 1;
    /// Event kind: `$2007` read during rendering (value = `v`).
    pub const K_R2007: u64 = 2;
    /// Event kind: hybrid nametable splice fired (value = effective address).
    pub const K_HYBRID: u64 = 3;
    /// Event kind: stale-latch pattern splice fired (value = effective address).
    pub const K_STALE: u64 = 4;
    /// Event kind: `$2007` state-machine countdown landed (value = data byte).
    pub const K_SMLAND: u64 = 5;

    /// Push one record (no-op unless `ENABLE` and slots remain).
    #[allow(clippy::cast_sign_loss)] // scanline filtered to 2..=5 (always >= 0)
    pub fn push(kind: u64, frame: u64, scanline: i16, dot: u16, value: u32) {
        if ENABLE.load(core::sync::atomic::Ordering::Relaxed) == 0 {
            return;
        }
        // Restrict to the corruption-relevant visible scanlines (ALE+Read is
        // scanline 3, Hybrid scanline 4) to keep the ring from overflowing on
        // the many unrelated `$2006`/`$2007`-during-render writes elsewhere.
        if !(2..=5).contains(&scanline) {
            return;
        }
        let i = IDX.fetch_add(1, core::sync::atomic::Ordering::Relaxed) as usize;
        if i >= LOG.len() {
            return;
        }
        let sl = (u64::from((scanline & 0x0FFF) as u16)) << 32;
        let packed = (kind << 58)
            | ((frame & 0x3FFF) << 44)
            | sl
            | ((u64::from(dot) & 0xFFF) << 20)
            | u64::from(value & 0xF_FFFF);
        LOG[i].store(packed, core::sync::atomic::Ordering::Relaxed);
    }
}

/// Zero-cost no-op stand-in for the octal-latch tracer.
///
/// Compiled when the `ppu-octal-trace` dev feature is off (the default, and the
/// only config the `#![no_std]` chip stack builds — the full tracer's ring needs
/// 64-bit atomics the `thumbv7em` target lacks). Exposes the same `push`
/// signature and `K_*` event-kind constants the 2-cycle-ALE fetch path
/// references, so those call sites stay unconditional; `push` here is an empty
/// body the optimizer removes entirely. Behavior is thus byte-identical to the
/// full tracer with capture disabled — the tracer only ever observes, never
/// influences, emulation.
#[cfg(not(feature = "ppu-octal-trace"))]
pub mod octal_trace {
    /// Event kind: `$2006` second write during rendering.
    pub const K_W2006: u64 = 1;
    /// Event kind: `$2007` read during rendering.
    pub const K_R2007: u64 = 2;
    /// Event kind: hybrid nametable splice fired.
    pub const K_HYBRID: u64 = 3;
    /// Event kind: stale-latch pattern splice fired.
    pub const K_STALE: u64 = 4;
    /// Event kind: `$2007` state-machine countdown landed.
    pub const K_SMLAND: u64 = 5;

    /// No-op (the `ppu-octal-trace` feature is off). Compiles to nothing.
    #[inline(always)]
    pub const fn push(_kind: u64, _frame: u64, _scanline: i16, _dot: u16, _value: u32) {}
}

/// v2.0.3 (ADR 0030, Option 1) — the delayed-`CopyV` countdown length in PPU
/// dots (`TriCNES` `PPU_Update2006Delay`, `Emulator.cs:9837-9843`, which is 4
/// for three of the four CPU/PPU sub-cycle alignments and 5 for the fourth).
/// `RustyNES`'s lockstep bus applies the `$2006` write at the start of a CPU
/// cycle; the corrupted nametable read is the phase-1 dot of the fetch group one
/// coarse-X past the write. Empirically calibrated against the `TriCNES` per-dot
/// bus trace so the countdown lands on that read after exactly one `inc_hori_v`
/// and one phase-0 NT ALE (which loads the one-tile-ahead `$19` low byte). See
/// the v2.0.3 campaign plan.
const COPY_V_DELAY: u8 = 4;

/// v2.1.4 F2.3 — optional OAM decay threshold, in **CPU cycles**.
///
/// The 2C02's Object Attribute Memory is dynamic RAM: each row is implicitly
/// refreshed every time sprite evaluation (or a `$2004` access) reads it during
/// rendering, but with rendering disabled long enough the un-refreshed rows lose
/// their charge and decay to a fixed garbage pattern. Mesen2 models this as a
/// per-8-byte-row CPU-cycle timestamp with a 3000-cycle refresh window
/// (`NesPpu::OamDecayCycleCount`, `Core/NES/NesPpu.cpp`); a read/write that lands
/// within 3000 CPU cycles of the row's last touch refreshes it, otherwise the row
/// has decayed. This value mirrors Mesen2's constant exactly so the two agree on
/// when a row is considered stale.
///
/// This whole model is **off by default** (`Ppu::oam_decay_enabled == false`) and
/// **NTSC/Dendy-only** — on PAL the far more frequent refresh cadence masks decay
/// entirely, so the feature is never applied there. With it off, no OAM access
/// consults the decay state and the PPU is byte-identical to a build that never
/// had the field. See `docs/ppu-2c02.md` (§OAM decay).
const OAM_DECAY_CPU_CYCLES: u64 = 3000;

/// Region governs the size of the post-render-to-pre-render scanline span.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum PpuRegion {
    /// NTSC (and Famicom). 262 scanlines per frame, pre-render = scanline 261.
    Ntsc,
    /// PAL. 312 scanlines per frame, pre-render = scanline 311.
    Pal,
    /// Dendy (Russian PAL famiclone). 312 scanlines, but VBL starts at 291.
    Dendy,
}

impl PpuRegion {
    /// Pre-render scanline number.
    #[must_use]
    pub const fn prerender_line(self) -> i16 {
        match self {
            Self::Ntsc => 261,
            Self::Pal | Self::Dendy => 311,
        }
    }

    /// Last visible scanline (always 239).
    #[must_use]
    pub const fn last_visible_line(self) -> i16 {
        239
    }

    /// Scanline at which V-blank starts (and `PPUSTATUS.VBLANK` is set on dot 1).
    #[must_use]
    pub const fn vblank_start_line(self) -> i16 {
        match self {
            Self::Ntsc | Self::Pal => 241,
            Self::Dendy => 291,
        }
    }

    /// Number of CPU cycles `$2000`/`$2001`/`$2005`/`$2006` writes are
    /// ignored after a power-on / reset. Per nesdev wiki:
    /// NTSC ≈ 29,658; PAL ≈ 33,132.
    #[must_use]
    pub const fn post_reset_mask_cycles(self) -> u32 {
        match self {
            Self::Ntsc => 29_658,
            Self::Pal | Self::Dendy => 33_132,
        }
    }
}

/// v2.1.7 P5 — selectable 2C02 die revision, gating revision-dependent quirks.
///
/// Additive and **default-off**: the [`Default`] ([`Self::Rp2c02H`]) preserves
/// `RustyNES`'s established behavior byte-for-byte, so `AccuracyCoin`, the
/// commercial oracle, and the visual / audio regression suites are unaffected at
/// the default. Only the opt-in [`Self::Rp2c02G`] selection changes any emulated
/// behavior (see below).
///
/// Real RP2C02 dies shipped across several letter revisions. The one behavioral
/// difference `RustyNES` currently models per-revision is the **OAMADDR
/// (`$2003`) write-during-rendering OAM corruption** glitch: writing `$2003`
/// while rendering is enabled on a visible / pre-render scanline copies one
/// 8-byte OAM "row" from row 0 over the row the write's high bits target, on the
/// next rendered dot (the same `CorruptOAM` mechanism the rendering-disable
/// model uses; see `Ppu::process_oam_corruption`). A handful of titles —
/// notably *Huge Insect* — trip it. It is **not** enabled on the default
/// revision.
///
/// Honesty note (see `docs/accuracy-ledger.md`): the exact mapping of the
/// `$2003` corruption onto specific 2C02 letter revisions is not firmly
/// established in the public literature, and the precise per-title byte output
/// of the glitch is not independently oracle-verified in this cut. `RustyNES`
/// therefore offers the model as an opt-in approximation keyed to a single
/// "earlier revision" selection ([`Self::Rp2c02G`]) rather than claiming exact
/// silicon-revision fidelity. This is config, **not** save-state: like
/// [`PpuRegion`] it is re-applied on load and is not part of the snapshot.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum PpuRevision {
    /// Default. Later RP2C02 die (the "H"-class revision `RustyNES` has always
    /// modeled). The OAMADDR (`$2003`) write-during-rendering OAM corruption is
    /// **not** modeled, so the deterministic output is byte-identical to a build
    /// without this feature.
    #[default]
    Rp2c02H,
    /// Earlier RP2C02 die ("rev E+" in the nesdev notes). Additionally models the
    /// OAMADDR (`$2003`) write-during-rendering OAM row-corruption glitch that
    /// *Huge Insect* and a few other titles trip. Opt-in; changes emulated
    /// behavior only for software that writes `$2003` mid-render.
    Rp2c02G,
}

impl PpuRevision {
    /// Whether this revision models the OAMADDR (`$2003`) write-during-rendering
    /// OAM corruption glitch. Only [`Self::Rp2c02G`] does; the default returns
    /// `false`, keeping the default build byte-identical.
    #[must_use]
    pub const fn models_oamaddr_corruption(self) -> bool {
        matches!(self, Self::Rp2c02G)
    }
}

/// v2.1.7 P5 — selectable power-up palette-RAM contents.
///
/// The 2C02's palette RAM is not cleared at power-on; different consoles (and
/// thus different emulator authors' reference dumps) come up with different
/// garbage. This is a documented power-up option, **default-off**: [`Default`]
/// ([`Self::Zeroed`]) keeps `RustyNES`'s established all-zero power-up palette,
/// so default rendering is byte-identical. It writes only `Ppu::palette_ram`,
/// which is already part of the save-state snapshot, so it needs no
/// snapshot-format change.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash, Default)]
pub enum PaletteInit {
    /// Default. All 32 palette-RAM bytes power up to `0x00` — `RustyNES`'s
    /// established deterministic power-up state. Byte-identical.
    #[default]
    Zeroed,
    /// The canonical "Blargg" power-up palette dump (the 32-byte pattern used by
    /// blargg's NES and mirrored by `TriCNES`'s `BlarggPalette`). A documented,
    /// deterministic known pattern for software that samples uninitialized
    /// palette RAM before writing it. Opt-in.
    Blargg,
}

/// The canonical "Blargg" power-up palette-RAM contents (32 bytes). Mirrors
/// `TriCNES`'s `BlarggPalette` table (`Emulator.cs`) verbatim. Only applied when
/// [`PaletteInit::Blargg`] is selected.
const BLARGG_POWER_UP_PALETTE: [u8; 32] = [
    0x09, 0x01, 0x00, 0x01, 0x00, 0x02, 0x02, 0x0D, 0x08, 0x10, 0x08, 0x24, 0x00, 0x00, 0x04, 0x2C,
    0x09, 0x01, 0x34, 0x03, 0x00, 0x04, 0x00, 0x14, 0x08, 0x3A, 0x00, 0x02, 0x00, 0x20, 0x2C, 0x08,
];

/// 2C02 PPU.
///
/// `tick(bus)` advances one PPU dot. The PPU is the master clock;
/// `rustynes-core` calls it three times per CPU cycle (NTSC).
#[derive(Debug)]
#[allow(clippy::struct_excessive_bools)] // PPU's many 1-bit latches are spec
pub struct Ppu {
    /// Region (governs frame structure).
    pub(crate) region: PpuRegion,

    // === CPU-facing register state ===
    pub(crate) ctrl: PpuCtrl,
    pub(crate) mask: PpuMask,
    /// Two-stage delay pipeline of `mask` consumed exclusively by the
    /// pre-render dot-339 odd-frame skip check. `mask_for_skip_check` is
    /// the value seen *this* dot; `mask_skip_pipe1` is the staged value
    /// that will become visible *next* dot. Both shift at the end of every
    /// `advance_dot`. The total visible delay between a PPUMASK write and
    /// the dot-skip detector is two PPU clocks — enough to compensate for
    /// the lockstep bus model applying `cpu_write` at the *start* of a CPU
    /// cycle (before the cycle's 3 PPU ticks) while real hardware latches
    /// the write at φ2 (effectively the *end* of the cycle). Required by
    /// blargg `ppu_vbl_nmi/10-even_odd_timing`; tests 1-9 of the same
    /// corpus are unaffected because rendering is enabled long before
    /// the boundary.
    pub(crate) mask_for_skip_check: PpuMask,
    pub(crate) mask_skip_pipe1: PpuMask,
    pub(crate) status: PpuStatus,
    /// `$2003` OAMADDR.
    pub(crate) oam_addr: u8,
    /// `$2007` PPUDATA read buffer.
    pub(crate) data_buffer: u8,
    /// `mc-ppu-2007-render-buffer`: the most recent VRAM data-bus value (every
    /// rendering fetch updates it). During rendering a `$2007` read returns THIS
    /// (the pipeline's current fetch byte), not a read at `v` — the model the
    /// `AccuracyCoin` `$2007 Stress` test brackets (`$2007` read on every dot).
    pub(crate) render_data_bus: u8,
    /// `mc-ppu-2007-render-buffer`: PPU-dot countdown from a `$2007` read during
    /// rendering to the `PPUDATA` state machine's `data_buffer` reload. `TriCNES`
    /// reloads `PPU_ReadBuffer` via a latch cascade ~4 dots after read-END
    /// (`Emulator.cs` `PPU_DATA_StateMachine`); with R1's fixed CPU<->PPU mod-4
    /// phase the hardware landing dot is a constant +5 dots from the
    /// register-read sample point (empirical W2 sweep winner: 170/170 stable
    /// reads). Set at the read (`RENDER_BUFFER_DOT_DELAY`),
    /// decremented once per dot in `Ppu::tick` (end of the fetch dispatch); at
    /// 0 the buffer latches `render_data_bus` (the fetch cadence's latched bus
    /// value — never a fresh VRAM read, so zero new A12/mapper events).
    /// 0 = inactive. Serialized in the PPU snapshot v3 tail (W3-Stage-4,
    /// 2026-06-10) so an in-flight reload survives a save-state restore.
    pub(crate) ppudata_sm_countdown: u8,
    /// `mc-ppu-2007-render-buffer`: the v-glitch increment of the in-flight
    /// `$2007` read is still pending — performed at the `TStep` (= the countdown
    /// landing dot, after the buffer reload), per `TriCNES`
    /// `PPU_DATA_StateMachine_Half`. Serialized in the PPU snapshot v3 tail
    /// (paired with `ppudata_sm_countdown`).
    pub(crate) ppudata_v_inc_pending: bool,
    /// `mc-ppu-2007-render-buffer`: raw (pre-h-flip) sprite pattern bytes
    /// captured by `fetch_sprite_tile` per slot, so the per-dot sprite-fetch
    /// read cadence (dots 257-320) feeds `render_data_bus` the PT-lo / PT-hi
    /// values the PPU drove on the bus (the `$2007` buffer captures the raw
    /// bus byte, not the flipped shifter contents). Serialized in the PPU
    /// snapshot v3 tail.
    pub(crate) spr_fetch_lo_raw: [u8; 8],
    /// See [`Self::spr_fetch_lo_raw`].
    pub(crate) spr_fetch_hi_raw: [u8; 8],
    /// `mc-ppu-2007-render-buffer`: the nametable address latched by the ALE
    /// of sprite slot 0's first garbage NT read (dot 257) — captured BEFORE
    /// the dot-257 copy-hori, so the read at dot 258 uses the OLD `v` (the
    /// only sprite-interval read that does). Serialized in the PPU snapshot
    /// v3 tail (refreshed every rendered scanline).
    pub(crate) ppudata_spr0_nt_addr: u16,

    // === v2.0.3 (ADR 0030) octal-latch / 2-cycle-ALE multiplexed-bus model ===
    // Now the ONLY PPU fetch path (promoted from the experimental flag in
    // v2.0.3; the superseded v2.0.2 whole-dot stand-in was retired). In continuous
    // play these fields self-heal within a scanline (they reload on the next fetch
    // ALE), but a mid-render rollback/save-state checkpoint can capture them live —
    // so they ARE serialized in the `PPU_SNAPSHOT_VERSION` v5 tail (added in v2.0.3)
    // for netplay-rollback determinism. That bump is ADDITIVE (pre-v5 blobs upconvert
    // to the inactive rest defaults), NOT an ADR-0028 save-state format-epoch break.
    //
    // Ported from TriCNES (`ref-proj/TriCNES/Emulator.cs`, MIT, commit 9199870),
    // the AccuracyCoin author's own transistor-level emulator, which is the
    // ground-truth oracle for the "ALE + Read" / "Hybrid Addresses" tests (the
    // vendored Mesen2 build does NOT pass them — see the ADR 0030 campaign audit).
    //
    // The PPU multiplexes its low 8 VRAM address pins (PA0-7) with the 8 data
    // pins (AD7-0). A 74LS373-class external octal latch (`octal_latch`) captures
    // A7-A0 on the address-latch-enable (ALE) half of each 2-cycle VRAM access;
    // on the read half the PPU drives only A13-A8 (the fetch address's high 6
    // bits, `& 0x3F00`) and the latch supplies A7-A0. The *effective* read
    // address is therefore the splice `(fetch_addr & 0x3F00) | octal_latch`
    // (TriCNES `FetchPPU`:153). When those halves desync — a mid-fetch `$2006`
    // high-byte update, or a `$2007`-read ALE that overlaps the fetch cadence and
    // freezes the latch on a stale DATA byte — the PPU reads a "hybrid" address it
    // never coherently drove.
    /// 74LS373 low-address octal latch (A7-A0). Loaded on each fetch's ALE
    /// with the driven address's low byte; frozen (goes stale) across a
    /// `$2007`-read/background-fetch ALE overlap. Serialized in the PPU snapshot
    /// v5 tail (v2.0.3): it self-heals within a scanline in continuous play, but a
    /// mid-render rollback checkpoint needs it restored for determinism.
    pub(crate) octal_latch: u8,
    /// The multiplexed PPU address/data bus. On a fetch's ALE (even) dot the full
    /// driven 14-bit address is written here; on the read (odd) dot the DATA byte
    /// is written back into the low 8 bits (the AD7-0 pins), so the register always
    /// reflects the true multiplexed bus value. The effective read address is the
    /// splice `(address_bus & 0x3F00) | octal_latch`. Serialized in the PPU snapshot
    /// v5 tail (v2.0.3) for mid-render rollback determinism (self-heals on the next
    /// fetch ALE in continuous play).
    pub(crate) address_bus: u16,
    /// Set on a fetch's ALE (even) dot after the address is driven onto
    /// [`Self::address_bus`]; consumed (cleared) by the matching read (odd) dot's
    /// [`Self::ale_splice`]. Distinguishes a read that followed a real ALE (main
    /// background dispatch, phases 0/2/4/6 → 1/3/5/7) from a read with NO preceding
    /// ALE (the dot-337-340 garbage nametable fetches), so the latter stays
    /// behavior-neutral (drives + latches coherently in-place rather than splicing
    /// a stale bus).
    pub(crate) ale_armed: bool,
    /// One-shot: the `$2007`-read ALE overlap froze `octal_latch` on the read's
    /// DATA byte, so the next background PATTERN fetch reads
    /// `(PAR-high 6):(stale low 8)` (`$0F03` -> `$0FFF`) — the "ALE + Read"
    /// corruption. Consumed by the next pattern (BG-lo/BG-hi) fetch: the latch is
    /// frozen at the PPUDATA state-machine landing dot and carried to the next
    /// pattern read via the natural ALE/read multiplex.
    pub(crate) pattern_latch_stale: bool,
    /// The delayed-`CopyV` countdown (`TriCNES`
    /// `PPU_Update2006Delay`, `Emulator.cs:1684-1704`). A `$2006` second write
    /// that lands DURING rendering does NOT copy `t -> v` immediately; it stages
    /// this PPU-dot countdown instead. While it runs, the background fetch
    /// cadence keeps advancing coarse-X (via `inc_hori_v` at phase 7) and the
    /// per-group phase-0 nametable ALE keeps loading `octal_latch` with the
    /// CURRENT (pre-copy) `v`'s NT-low byte — so by the time the countdown lands
    /// the latch NATURALLY holds the one-tile-ahead low byte (`$19`), and the
    /// landing's `address_bus = v` splice yields the hybrid address (`$2F19`)
    /// with no `+1 coarse-X` reconstruction. 0 = inactive.
    pub(crate) copy_v_delay: u8,

    // === Internal scroll/address registers (loopy v/t/x/w) ===
    /// 15-bit "current VRAM address".
    pub(crate) v: u16,
    /// 15-bit "temporary VRAM address" (latched scroll/PPUADDR target).
    pub(crate) t: u16,
    /// 3-bit fine X scroll.
    pub(crate) x: u8,
    /// 1-bit write toggle for `$2005` / `$2006`.
    pub(crate) w: bool,

    // === Memory ===
    /// Console-side nametable VRAM (CIRAM, 2 KiB). Owned by the PPU; the
    /// mapper exposes a per-cart `nametable_address` mirroring map via
    /// [`PpuBus::nametable_address`] so the PPU can read/write CIRAM directly
    /// without going through `bus.ppu_read/write` for `$2000-$3EFF`.
    pub(crate) ciram: Box<[u8]>,
    /// Object Attribute Memory: 64 sprites × 4 bytes.
    pub(crate) oam: Box<[u8]>,
    /// Secondary OAM: up to 8 sprites for the next scanline. Populated
    /// during sprite evaluation in Sprint 2-3.
    #[allow(dead_code)]
    pub(crate) secondary_oam: [u8; 32],
    /// Palette RAM: 32 entries, 6-bit each (high 2 bits open-bus on read).
    pub(crate) palette_ram: [u8; 32],

    // === v2.1.4 F2.3 optional OAM decay (opt-in, default-OFF) ===
    /// Per-8-byte-row last-touch timestamp, in **CPU cycles** (`dot_counter / 3`).
    /// OAM is 256 bytes = 32 rows of 8 bytes; `oam_decay_cycles[addr >> 3]` is the
    /// CPU cycle at which row `addr >> 3` was last refreshed by an OAM read/write.
    /// Only consulted when [`Self::oam_decay_enabled`] is set AND the region is
    /// NTSC/Dendy; otherwise it is dead state that never influences a read. Mirrors
    /// Mesen2's `_oamDecayCycles` (`Core/NES/NesPpu.cpp`).
    ///
    /// Serialized as a *relative age* in the PPU snapshot v7 tail (see
    /// `snapshot.rs`): the absolute timestamps reference the free-running,
    /// **un-serialized** `dot_counter`, so a raw absolute value would be meaningless
    /// after a rollback/restore rebased that counter. Storing `now - timestamp`
    /// (and reconstructing `now - age` on load, relative to the live counter) keeps
    /// a run-ahead / netplay `snapshot`→`restore` byte-identical to the forward run.
    pub(crate) oam_decay_cycles: [u64; 32],
    /// Master enable for the OAM-decay model. **`false` by default** — a frontend /
    /// config knob (re-applied on load like `region` / `active_palette`), NOT part
    /// of the save-state. While `false`, every decay hook early-returns, so OAM
    /// reads/writes touch neither this flag's siblings nor `oam_decay_cycles`, and
    /// the framebuffer/audio/replay output is byte-identical to a decay-free build.
    pub(crate) oam_decay_enabled: bool,

    // === Open-bus latch (for $2000-$3FFF) ===
    /// Most recent value driven onto the PPU bus by any register access.
    pub(crate) open_bus: u8,
    /// Per-bit-group decay counters (in CPU cycles) until each bit group of
    /// the open-bus latch reads as 0.  Three groups, each with its own timer:
    ///   `[0]` bits 0-4, `[1]` bit 5, `[2]` bits 6-7.
    /// Required by `ppu_open_bus.nes` tests 7 and 9, which assert that some
    /// reads refresh only a subset of the bit groups (e.g., reading $2002
    /// must not refresh the low 5 bits' decay timer; palette $2007 reads
    /// must not refresh the high 2 bits' decay timer).
    pub(crate) open_bus_decay: [u32; 3],

    // === NMI line + frame counter ===
    /// `true` while the PPU is asserting NMI.
    pub(crate) nmi_line: bool,
    /// True for one frame after a `cpu_read_register($2002)` race so we
    /// suppress the VBL flag set + NMI for that frame (per
    /// `ppu_vbl_nmi/06-suppression.nes`). Toggled on the cycle the read
    /// hits at scanline 241 dot 0 / dot 1.
    pub(crate) suppress_vbl_this_frame: bool,
    /// Last-observed A12 level, for edge-triggered notifications.
    pub(crate) last_a12_level: bool,

    // === Scanline FSM ===
    /// Current dot (0..=340).
    pub(crate) dot: u16,
    /// Current scanline (-1 in pre-render, 0..=239 visible, 240 post-render,
    /// 241..=260/310 vblank). Stored as i16 to allow temporary -1.
    pub(crate) scanline: i16,
    /// Frame counter (for odd-frame skip).
    pub(crate) frame: u64,
    /// `frame_complete` latch — set to `true` on the dot the PPU finishes
    /// a frame; consumed by the run loop and cleared on next read.
    pub(crate) frame_complete: bool,

    // === Power-on / reset masking window ===
    /// CPU cycles remaining in the post-reset masking window. While > 0,
    /// writes to PPUCTRL/PPUMASK/PPUSCROLL/PPUADDR are silently ignored
    /// (reads still work).
    pub(crate) post_reset_mask_remaining: u32,

    // === Background fetch + shift register state ===
    /// Latched nametable byte from the current 8-cycle fetch group.
    pub(crate) nt_latch: u8,
    /// Latched attribute byte (palette) from the current 8-cycle fetch group.
    pub(crate) at_latch: u8,
    /// Latched BG pattern low byte from the current 8-cycle fetch group.
    pub(crate) bg_lo_latch: u8,
    /// Latched BG pattern high byte from the current 8-cycle fetch group.
    pub(crate) bg_hi_latch: u8,
    /// 16-bit BG pattern low shift register.
    pub(crate) bg_shift_lo: u16,
    /// 16-bit BG pattern high shift register.
    pub(crate) bg_shift_hi: u16,
    /// 16-bit attribute low shift register.
    ///
    /// Mirrors the 16-bit BG pattern shifters exactly: at each 8-dot
    /// reload the latched attribute bit is expanded to a full byte
    /// (`0x00` or `0xFF`) into bits 0-7, shifted left by 1 after each
    /// emit, and shifted left by 8 at the pre-fetch boundary (dots 328 /
    /// 336). Keeping it 16-bit (not the prior 8-bit + 1-bit-feed model)
    /// is what keeps the attribute in lockstep with the pattern bits
    /// through the dots 321-336 pre-fetch region, where `shift_bg` does
    /// not run and only the explicit `<<= 8` advances the registers.
    pub(crate) at_shift_lo: u16,
    /// 16-bit attribute high shift register. See [`Self::at_shift_lo`].
    pub(crate) at_shift_hi: u16,
    /// Optional per-tile extended attribute (MMC5 `ExGrafix`). Latched at
    /// the NT-byte fetch boundary; consumed by AT / BG-low / BG-high
    /// fetches in the same 8-dot group.
    pub(crate) ex_attr_latch: Option<ExAttribute>,
    /// Optional vertical split-screen state (MMC5 `$5200`-`$5202`). Latched
    /// at the NT-byte fetch boundary; consumed by AT / BG-low / BG-high
    /// fetches in the same 8-dot group. When `Some`, the BG fetches use the
    /// alt region's nametable address, attribute address, fine-Y, and CHR
    /// bank instead of the values derived from `v`.
    pub(crate) bg_split_latch: Option<BgSplitState>,

    // === Sprite rendering state ===
    /// Per-sprite shift registers (low + high pattern).
    pub(crate) spr_shift_lo: [u8; 8],
    pub(crate) spr_shift_hi: [u8; 8],
    /// Per-sprite latched attribute byte.
    pub(crate) spr_attr: [u8; 8],
    /// Per-sprite X-coordinate counter.
    pub(crate) spr_x: [u8; 8],
    /// v2.0 (ppu-sprite-shifter-counter): per-sprite persistent "halted" latch.
    /// Set when the X-counter reaches 0 (the sprite is drawing) and PERSISTS
    /// across a render-disable and the frame boundary; re-armed to "counting"
    /// at dot 339 for the loaded slots. Carries the Stale Sprite Shift Regs
    /// t5/6 behavior (a reloaded-but-halted sprite draws on the next rendering
    /// re-enable). Default build: absent (legacy `spr_x == 0` predicate).
    pub(crate) spr_halted: [bool; 8],
    /// Number of sprites loaded for the current scanline.
    pub(crate) spr_count: u8,
    /// `true` if sprite 0 is in the current scanline's sprite line-up.
    pub(crate) spr_zero_in_line: bool,

    // === Per-dot sprite-evaluation FSM state ===
    /// Sprite-eval read latch: byte read from primary OAM on odd cycles
    /// (1, 3, 5, ...) of dots 65-256, consumed by the immediately-following
    /// even-cycle write into secondary OAM.
    pub(crate) sprite_eval_read_latch: u8,
    /// Primary-OAM sprite index 0..=63 walked during dots 65-256.
    pub(crate) sprite_eval_n: u8,
    /// Per-sprite byte index 0..=3 walked during dots 65-256 (drives the
    /// buggy `n+m` increment when overflow detection mode is active).
    pub(crate) sprite_eval_m: u8,
    /// Number of in-range sprites found so far in this scanline's eval pass.
    pub(crate) sprite_eval_found: u8,
    /// Write index into `secondary_oam` (0..=31). Tracks how many bytes the
    /// per-dot FSM has committed so far.
    pub(crate) sprite_eval_sec_idx: u8,
    /// `true` when the current sprite (the one whose `y` byte just tested
    /// in-range) is still being copied — bytes 1, 2, 3 land in subsequent
    /// even-dot writes.
    pub(crate) sprite_eval_copying: bool,
    /// `true` when eval has exhausted primary OAM (n wrapped past 63) or
    /// overflow has been detected — remaining dots 65-256 idle out.
    pub(crate) sprite_eval_done: bool,
    /// `true` when 8 in-range sprites have been latched and the FSM is
    /// in overflow-detection mode (buggy `n+m` increment active).
    pub(crate) sprite_eval_overflow_search: bool,
    /// Eval-side latch for "sprite 0 is in the line being evaluated."
    /// Set during the current scanline's eval pass (dots 65..=256) when
    /// sprite 0 lands in-range; committed to [`Self::spr_zero_in_line`]
    /// at dot 256 alongside [`Self::spr_count`]. Keeping the eval-side
    /// latch separate from the rendering-side flag ensures the FSM
    /// doesn't trample the CURRENT scanline's sprite-0-hit signal while
    /// it's still being read by the dots 1..=256 sprite-pixel evaluator.
    pub(crate) sprite_eval_zero_found: bool,
    /// Phase 3a flag — tracks whether current scanline's eval is on
    /// its FIRST iteration (PPU cycle 66, first y-test).  Set at
    /// dot 0 of each visible scanline; cleared after the first y-test
    /// fires (in-range or not).  Per Mesen2 `ProcessSpriteEvaluation`
    /// line 1040-1044, sprite-zero fires IFF the FIRST y-test is in
    /// range — not "first in-range sprite found".  When OAMADDR is 0
    /// at eval start and OAM[0].y is in range, this matches the legacy
    /// `n == 0` check.  When OAMADDR != 0, this fires on whichever
    /// sprite the start position points to (sprite at OAMADDR / 4)
    /// if its y is in range, else NO sprite-zero is detected.
    pub(crate) sprite_eval_first_iter: bool,

    /// v2.0 Tier 1.2 — isolated OAM-data-bus model (parallel port of Mesen2's
    /// `ProcessSpriteEvaluation` + `_oamCopybuffer`). These fields exist ONLY
    /// under `ppu-oam-data-bus` and are read solely by `$2004` during
    /// rendering — the rendering / sprite-zero / overflow / MMC3 sprite-fetch
    /// FSM uses `secondary_oam` + `sprite_eval_*` + `spr_*`, all untouched.
    /// `oam_bus_copybuffer` mirrors `_oamCopybuffer` (the value `$2004`
    /// returns while the screen is drawn).
    pub(crate) oam_bus_copybuffer: u8,
    /// Parallel secondary OAM (`_secondarySpriteRam`) for the bus model only.
    pub(crate) oam_bus_secondary: [u8; 32],
    /// `_spriteAddrH` (the eval pointer's sprite index, 0..=63).
    pub(crate) oam_bus_addr_h: u8,
    /// `_spriteAddrL` (the eval pointer's byte-in-sprite, 0..=3).
    pub(crate) oam_bus_addr_l: u8,
    /// `_secondaryOamAddr` (write index into the parallel secondary OAM).
    pub(crate) oam_bus_secondary_addr: u8,
    /// `_oamCopyDone` (primary OAM fully scanned / wrapped).
    pub(crate) oam_bus_copy_done: bool,
    /// `_spriteInRange` (currently copying an in-range sprite).
    pub(crate) oam_bus_sprite_in_range: bool,
    /// `_overflowBugCounter` (the 8-sprite-overflow PPU-bug countdown).
    pub(crate) oam_bus_overflow_counter: u8,

    /// OAM-corruption model — faithful port of `TriCNES`'s eval-pointer
    /// machinery (`Emulator.cs` `PPU_Render_SpriteEvaluation` lines
    /// 2664-2770 + `CorruptOAM` lines 2635-2651). Replaces the earlier
    /// Mesen2 `_corruptOamRow` row-flag model (`dot >> 1` index), which
    /// Mesen ships OFF by default (`EnablePpuOamRowCorruption=false`) and
    /// documents as unfinished. The bug it fixes: SMB3 (MMC3) toggles
    /// PPUMASK mid-visible-scanline to split its HUD; NMI/DMA jitter
    /// shifts the disable dot, and the raw-dot row index intermittently
    /// landed on Mario's OAM row (offset 40), wiping his sprite.
    ///
    /// `TriCNES` model: when rendering is disabled (1 -> 0) DURING sprite
    /// evaluation (dots 1-64, secondary-OAM clear, NOT the pre-render
    /// line), the corruption is DEFERRED — `oam_corruption_pending` is
    /// set and `oam_corruption_index` captures the live secondary-OAM
    /// write pointer (`OAM2Address`) at that instant. When rendering
    /// RE-ENABLES (or at the pre-render line), one OAM "row" of 8 bytes
    /// is replaced from row 0: `oam[index*8 + i] = oam[i]` for i in
    /// 0..8 (index 0x20 wraps to 0), and `secondary_oam[index] =
    /// secondary_oam[0]`.
    ///
    /// `oam2_addr` is the `OAM2Address` analogue maintained across the
    /// dots 1-64 clear window (our dots 65-256 active eval already walks
    /// `sprite_eval_sec_idx`, but the SMB3 HUD-split disable lands in the
    /// clear window where `sprite_eval_sec_idx` is held at 0, so the
    /// dedicated pointer is required to capture the right index).
    ///
    /// `oam_corruption_disabled` / `_instant` mirror `TriCNES`'s
    /// `PPU_OAMCorruptionRenderingDisabledOutOfVBlank` (1-dot-delayed,
    /// armed by the `$2001` write-delay) and `..._Instant` (the
    /// data-bus-immediate path: OAM eval observes the disable the same
    /// cycle). The disable edge is captured into `pending`/`index`
    /// during the dots 1-64 eval window; the actual corruption is
    /// committed at re-enable / pre-render.
    ///
    /// None of these fields are persisted in the PPU snapshot — like the
    /// rest of the per-dot sprite-eval FSM state (`sprite_eval_*`), they
    /// re-derive within a scanline/frame, matching the prior row-flag
    /// OAM-corruption model (also un-snapshotted).
    pub(crate) oam_corruption_pending: bool,
    pub(crate) oam_corruption_index: u8,
    pub(crate) oam_corruption_disabled: bool,
    pub(crate) oam_corruption_disabled_instant: bool,
    /// `OAM2Address` analogue: the secondary-OAM write pointer as it
    /// walks the dots 1-64 secondary-OAM clear window. Reset to 0 at the
    /// dot-1 boundary and incremented once per even clear dot, masked to
    /// 0x1F, exactly as `TriCNES` drives `OAM2Address` during dots 1-64.
    pub(crate) oam2_addr: u8,
    /// Previous-tick rendering-enabled state — tracks the rising /
    /// falling edge of `mask.rendering_enabled()` so the 1->0 edge
    /// BG-shifter fix-up fires on the correct transition.
    pub(crate) prev_rendering_enabled: bool,
    /// v2.0 (ported from branch `ae30785`) — 1-PPU-dot-delayed rendering-enabled
    /// gate. Per Mesen2 `NesPpu::UpdateState`: a `$2001` write toggling
    /// `SHOW_BG|SHOW_SPRITE` takes effect on the rendering pipeline one PPU dot
    /// later (the `mask` bit-fields update immediately for pixel output; this
    /// delayed copy gates the fetch/shift/sprite-eval pipeline). When rendering
    /// is stable this equals `mask.rendering_enabled()`, so only mid-scanline
    /// `$2001` toggles observe the delay. The `ppu-sprite-shifter-counter`
    /// feature reads it (via `rendering_gate`); the default build uses the
    /// immediate value, so flag-off is byte-identical. Updated at tick end.
    pub(crate) rendering_enabled_delayed: bool,

    /// v2.0 Phase 6 (`mc-ppu-subpos`): the analog `$2001` BG-shift-register
    /// RELOAD delay. The shifter reload gates on `bg_reload_render`, which tracks
    /// the live `self.mask` rendering-enable bit EXCEPT during the
    /// `MASK_WRITE_DELAY`-dot window after a `$2001` write, where it stays frozen
    /// at its prior value (`TriCNES` gates the fetch/reload on
    /// `PPU_Mask_Show*_Delayed` while the per-half-dot SHIFT runs on the
    /// IMMEDIATE mask). So on a render re-enable the shifter advances for
    /// `MASK_WRITE_DELAY` dots (injecting the serial-in '1') BEFORE the reload
    /// resumes -> one reload is SKIPPED and the accumulated '1's reach the output
    /// (BG Serial In), without perturbing the sprite/pixel/shift path. Because it
    /// re-syncs to the live mask whenever settled, a direct mask set (unit tests,
    /// save-state restore) leaves it consistent. `mask_write_delay` is the
    /// remaining freeze countdown (0 = settled). Serialized in the PPU
    /// snapshot v3 tail (W3-Stage-4, 2026-06-10) so an in-flight freeze
    /// survives a save-state restore.
    pub(crate) bg_reload_render: bool,
    pub(crate) mask_write_delay: u8,

    /// v1.4.0 Workstream F (F1) — scanline-stable rendering-classification
    /// cache. `visible` / `pre_render` / `render_line` are pure functions of
    /// `self.scanline` + `self.region`, so they only change when the scanline
    /// advances. The hot per-dot `tick` recomputes them ~7 branches deep
    /// 89,342 times/frame; instead we recompute them once when the scanline
    /// changes (detected via the `flags_cached_scanline` sentinel) and read the
    /// cached copies on every other dot. Byte-identical by construction (same
    /// values, computed less often) and self-healing across reset / save-state
    /// restore (the sentinel starts mismatched, forcing a recompute on the
    /// first tick). NOT part of the PPU snapshot — pure derived data.
    pub(crate) cached_visible: bool,
    pub(crate) cached_pre_render: bool,
    pub(crate) cached_render_line: bool,
    /// Sentinel: the scanline `cached_*` were last computed for. `i16::MIN`
    /// (an impossible scanline) forces a recompute on the first tick.
    pub(crate) flags_cached_scanline: i16,

    /// Active output palette. Defaults to the 2C02 composite palette so normal
    /// NES/Famicom rendering is byte-for-byte unchanged; set to one of the RGB
    /// variants for Vs. System / PlayChoice-10 carts (see [`Ppu::set_palette`]).
    /// Construction-time configuration only — never mutated during emulation, so
    /// it is intentionally NOT part of the PPU save-state snapshot (it is
    /// re-derived from the cartridge header on load).
    pub(crate) active_palette: crate::palette::PpuPalette,
    /// v2.8.0 Phase 4 — precomputed `(emphasis bits << 6) | color` → RGBA8
    /// lookup (8 emphasis combinations × 64 colors = 512 entries, 2 KiB).
    /// Built from the same pure [`crate::palette::palette_color_to_rgba`]
    /// the per-pixel path used to call, so it is byte-identical by
    /// construction; rebuilt whenever [`Ppu::set_palette`] changes the
    /// active palette. Saves a palette-variant match + emphasis branches
    /// per emitted pixel (61,440/frame). NOT part of the save-state
    /// (derived data).
    pub(crate) rgba_lut: [[u8; 4]; 512],
    /// v1.1.0 beta.1 (T-110-A3) — optional custom 64-entry base palette from a
    /// loaded `.pal` file. `None` (default) = use the built-in palette for the
    /// active [`crate::palette::PpuPalette`], so default rendering is
    /// byte-identical. When `Some`, [`Self::rgba_lut`] is built from it via the
    /// composite emphasis model. A frontend presentation override — NOT part of
    /// the save-state (it persists across save/load like the active palette).
    pub(crate) custom_palette: Option<[[u8; 3]; 64]>,
    /// True when this is a 2C05 PPU: `$2000`/`$2001` are swapped and `$2002`
    /// returns the 2C05 sub-variant identifier in its low bits. Default false
    /// (a 2C02 / 2C03 / 2C04, none of which swap or report an id).
    pub(crate) is_2c05: bool,
    /// 2C05 sub-variant `$2002` identifier byte (e.g. `$3D` for 2C05-02). Only
    /// consulted when [`Self::is_2c05`] is true. Combined into the low 5 bits
    /// of a `$2002` read per nesdev "PPU registers" §2C05 identifier.
    pub(crate) id_2c05: u8,

    /// v2.1.7 P5 — selected 2C02 die revision. Gates the OAMADDR (`$2003`)
    /// write-during-rendering OAM corruption glitch (see [`PpuRevision`]). The
    /// [`PpuRevision::default`] ([`PpuRevision::Rp2c02H`]) models NO extra
    /// corruption, so the default build is byte-identical. Construction / config
    /// only — never mutated by emulation and, like [`Self::active_palette`] /
    /// [`Self::region`], re-applied on load rather than serialized in the
    /// snapshot (the corruption *state* it can arm — `oam_corruption_pending` /
    /// `oam_corruption_index` — IS in the v6 snapshot tail, so an armed
    /// corruption still round-trips).
    pub(crate) die_revision: PpuRevision,
    /// v2.1.7 P5 — the power-up palette-RAM contents selected for this PPU (see
    /// [`PaletteInit`]). Stored so a power-cycle can re-apply it after the PPU is
    /// reconstructed. The [`PaletteInit::default`] ([`PaletteInit::Zeroed`])
    /// leaves palette RAM all-zero (the established default), keeping default
    /// rendering byte-identical. Config, not serialized (it writes
    /// [`Self::palette_ram`], which the snapshot already carries).
    pub(crate) power_up_palette: PaletteInit,

    /// Framebuffer (RGBA8). Filled by Sprint 2-2/2-3 rendering.
    pub(crate) framebuffer: Box<[u8]>,

    /// v1.1.0 beta.1 (T-110-A1) — parallel per-pixel **palette-index**
    /// framebuffer for the true composite `NES_NTSC` filter. Each entry is the
    /// 9-bit `(emphasis << 6) | colour_index` value (0..=511) written in the
    /// same emit path as [`Self::framebuffer`], so it is a faithful index-space
    /// mirror of the RGBA output. The frontend uploads it as an `R16Uint`
    /// texture and reconstructs the composite signal in a shader. Purely an
    /// output buffer: it changes no logical state, so the determinism /
    /// `AccuracyCoin` contract is unaffected. Unlike `framebuffer` (which IS in
    /// the save-state), this and `dot_counter` / `frame_ntsc_phase` are NOT
    /// serialized — they are regenerated on the next emitted frame, so a state
    /// loaded while paused shows correct NTSC from the first frame after resume.
    pub(crate) index_framebuffer: Box<[u16]>,

    /// Free-running PPU master-cycle counter (one increment per [`Self::tick`]),
    /// the basis for the per-frame NTSC colour phase. Output-only / cosmetic
    /// (drives only the optional NTSC filter's dot-crawl); not part of the
    /// save-state. Wraps harmlessly.
    pub(crate) dot_counter: u64,

    /// NTSC composite colour phase snapshotted at each frame boundary, the
    /// per-frame `videoPhase` consumed by the `NES_NTSC` filter (the shader
    /// derives the per-scanline / per-pixel phase from this base). `0..=2` on
    /// NTSC; on PAL/Dendy (no 3-phase crawl) it is the frame parity (`0..=1`).
    /// Cosmetic; not part of the save-state.
    pub(crate) frame_ntsc_phase: u8,

    /// v1.7.0 "Forge" Workstream F3 — PPU extra-scanlines overclock.
    ///
    /// Number of EXTRA blank scanlines to insert into the vblank period each
    /// frame (immediately before the pre-render line), at the existing dot
    /// resolution (Mesen2 `UpdateTimings`). These lines render nothing, emit no
    /// pixels, set/clear no PPU flags, and fire no VBL/NMI/A12 events — they are
    /// pure additional CPU run-time per frame, giving games more compute headroom
    /// without altering the visible image. **Off by default (`0`)**; the
    /// `advance_dot` insertion path is entirely guarded by `extra_scanlines != 0`,
    /// so at the default this field changes nothing and the frame is
    /// byte-identical to stock. Distinct from the CPU-multiplier overclock (a
    /// v2.0 timebase item). A frontend config knob, NOT part of the save-state
    /// (re-applied by the frontend on restore, like `region` / `active_palette`).
    pub(crate) extra_scanlines: u16,
    /// v1.7.0 F3 — countdown of extra blank scanlines remaining for the CURRENT
    /// frame's vblank insertion. Loaded from [`Self::extra_scanlines`] when the
    /// PPU reaches the insertion point and decremented one extra line at a time.
    /// `0` when no insertion is in flight. Snapshotted (snapshot v4) so a
    /// save-state taken mid-insertion restores the in-flight countdown rather
    /// than resuming as `0` and desyncing. The configured count itself
    /// (`extra_scanlines`) stays a non-persisted frontend knob, re-applied on
    /// restore. At the default `extra_scanlines == 0` this is always `0`.
    pub(crate) extra_lines_remaining: u16,

    /// Optional per-PPU-dot state trace (Session-10 observability
    /// tooling). Gated on the `ppu-state-trace` cargo feature so
    /// the default build pays no memory or codegen cost. See
    /// `docs/adr/0005-ppu-state-trace.md`.
    #[cfg(feature = "ppu-state-trace")]
    pub(crate) state_trace: Option<crate::state_trace::PpuStateTrace>,

    /// v1.2.0 beta.2 (Workstream C3) — per-pixel HD-pack tile-source buffer
    /// (256 × 240 [`HdTileSource`] records), written in [`Self::emit_pixel`]
    /// in lockstep with [`Self::index_framebuffer`]. Output-only telemetry,
    /// gated on the `hd-pack` cargo feature so the default build pays no
    /// memory or codegen cost. Not part of the save-state.
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_tile_source: Box<[HdTileSource]>,

    /// v1.2.0 beta.2 (Workstream C3) — BG tile CHR base address latched at
    /// `fetch_bg_lo` time, then reloaded into the 2-stage `hd_bg_addr_*`
    /// queue in `reload_bg_shift_regs` so it tracks the BG pattern shift
    /// registers tile-for-tile. Pure telemetry; only touched when `hd-pack`
    /// is enabled.
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_bg_addr_latch: u16,
    /// CHR base address of the BG tile currently feeding the shifters' high
    /// byte (the tile being displayed). See [`Self::hd_bg_addr_latch`].
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_bg_addr_cur: u16,
    /// CHR base address of the next BG tile (shifters' low byte). Promoted to
    /// `hd_bg_addr_cur` on the prefetch byte-shift / per-tile boundary.
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_bg_addr_next: u16,
    /// CHR base address fetched per sprite slot at `fetch_sprite_tile` time,
    /// consumed by `emit_pixel` for HD-pack sprite substitution.
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_spr_addr: [u16; 8],
    /// The sprite's ORIGIN screen X per slot (the un-decremented `spr_x`), so
    /// `emit_pixel` can derive the column within the sprite for HD positioning.
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_spr_x: [u8; 8],
    /// The sprite's flip-baked texel ROW (0..=7) per slot, captured at fetch (the
    /// `emit_pixel`-time scanline isn't enough to recover it post-shift).
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_spr_off_y: [u8; 8],
    /// Absolute CHR-ROM tile index (`chr_phys/16`, or [`HD_CHR_RAM`]) tracked in
    /// lock-step with the `hd_bg_addr_*` cascade — the CHR-ROM HD-pack key.
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_bg_idx_latch: u32,
    /// CHR-ROM tile index of the BG tile feeding the shifters' high byte.
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_bg_idx_cur: u32,
    /// CHR-ROM tile index of the next BG tile (shifters' low byte).
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_bg_idx_next: u32,
    /// Absolute CHR-ROM tile index per sprite slot (or [`HD_CHR_RAM`]).
    #[cfg(feature = "hd-pack")]
    pub(crate) hd_spr_idx: [u32; 8],
}

/// v2.0 Phase 6 (`mc-ppu-subpos`): the analog `$2001` PPUMASK write delay.
///
/// In PPU dots. `TriCNES` applies a `$2001` write 2-3 dots after the CPU write
/// (`PPU_Update2001Delay`, sub-dot alignment dependent); this emulator's reload
/// sits one dot later than that plus the existing 1-dot render gate, so the
/// effective default is 4. Runtime-tunable (an atomic) so the exact phase can be
/// swept against the `BG Serial In` / `Stale BG Shift` keys without rebuilding.
/// Only consulted under `mc-ppu-subpos`.
pub static MASK_WRITE_DELAY: core::sync::atomic::AtomicU8 = core::sync::atomic::AtomicU8::new(4);
impl Ppu {
    /// New PPU in power-on state.
    #[must_use]
    // The power-on field initialization is naturally long (the PPU has many
    // state fields); the feature-gated `hd-pack` initializers nudge it over the
    // 100-line lint. Splitting the struct literal would hurt readability.
    #[allow(clippy::too_many_lines)]
    pub fn new(region: PpuRegion) -> Self {
        let mut p = Self {
            region,
            ctrl: PpuCtrl::empty(),
            mask: PpuMask::empty(),
            mask_for_skip_check: PpuMask::empty(),
            mask_skip_pipe1: PpuMask::empty(),
            status: PpuStatus::empty(),
            oam_addr: 0,
            data_buffer: 0,
            render_data_bus: 0,
            ppudata_sm_countdown: 0,
            ppudata_v_inc_pending: false,
            spr_fetch_lo_raw: [0; 8],
            spr_fetch_hi_raw: [0; 8],
            ppudata_spr0_nt_addr: 0x2000,
            octal_latch: 0,
            address_bus: 0,
            ale_armed: false,
            pattern_latch_stale: false,
            copy_v_delay: 0,
            v: 0,
            t: 0,
            x: 0,
            w: false,
            ciram: vec![0u8; 0x0800].into_boxed_slice(),
            oam: vec![0u8; 0x0100].into_boxed_slice(),
            secondary_oam: [0xFF; 32],
            oam_bus_copybuffer: 0xFF,
            oam_bus_secondary: [0xFF; 32],
            oam_bus_addr_h: 0,
            oam_bus_addr_l: 0,
            oam_bus_secondary_addr: 0,
            oam_bus_copy_done: false,
            oam_bus_sprite_in_range: false,
            oam_bus_overflow_counter: 0,
            palette_ram: [0u8; 32],
            // v2.1.4 F2.3 — OAM decay is off by default; the timestamps start at 0
            // (row "last touched at cycle 0"). While disabled they are never read,
            // and `set_oam_decay(true)` re-bases them to the current cycle so
            // enabling mid-run does not instantly decay every row.
            oam_decay_cycles: [0; 32],
            oam_decay_enabled: false,
            open_bus: 0,
            open_bus_decay: [0; 3],
            nmi_line: false,
            suppress_vbl_this_frame: false,
            last_a12_level: false,
            // Power-up position matches Mesen2's NesPpu::Reset(false) endpoint
            // (_scanline=-1, _cycle=340). After the first PPU tick, wraps to
            // (scanline=0, dot=0, frame+=1), putting the post-power-on PPU
            // position within ~2 dots of Mesen2's. Combined with the 8-cycle
            // CPU reset (see Cpu::reset), this closes the +344-dot PPU offset
            // identified empirically in Session-13 (docs/audit/
            // session-13-cpu-boot-fix-2026-05-21.md).
            //
            // SESSION-29 CRITICAL FINDING: Option (a) "PPU re-baseline"
            // empirically attempted and DOES NOT CLOSE THE C1 AXIS.
            // Shifting PPU init by +2 dots to (scanline=0, dot=1):
            //   - Generates 24 snapshot regressions (audio_db, visual,
            //     m22, Cascade A) — all are "expected" cosmetic shifts.
            //   - BUT the cpu_interrupts_v2/{2,3,5}_strict probes STILL
            //     FAIL — confirmed via `cargo test ... --include-ignored`.
            //
            // The +2 dot shift moves everything uniformly: VBL set position
            // AND BIT $2002 read position both shift by +2 dots, preserving
            // the relative race-window relationship.  The BIT $2002 polling
            // loop inside blargg `sync_vbl` still hits the pre-VBL-set side
            // of the race window.
            //
            // CONCLUSION: closing C1 requires changing the PHASE
            // RELATIONSHIP between CPU and PPU (Option b — master-clock-
            // precise scheduling refactor), NOT a global PPU init shift.
            // The 4 C1 IRQ-timing residuals are deferred to v2.0 with the
            // master-clock refactor; v1.0.0 ships at 90.65% AccuracyCoin
            // with the 4 residuals documented as v2.0-deferred.  See
            // `docs/audit/session-29-c1-axis-final-conclusion-2026-05-23.md`
            // + `docs/audit/session-29-option-a-empirical-falsification.md`.
            dot: 340,
            scanline: region.prerender_line(),
            frame: 0,
            frame_complete: false,
            post_reset_mask_remaining: region.post_reset_mask_cycles(),
            nt_latch: 0,
            at_latch: 0,
            bg_lo_latch: 0,
            bg_hi_latch: 0,
            bg_shift_lo: 0,
            bg_shift_hi: 0,
            at_shift_lo: 0,
            at_shift_hi: 0,
            ex_attr_latch: None,
            bg_split_latch: None,
            spr_shift_lo: [0; 8],
            spr_shift_hi: [0; 8],
            spr_attr: [0; 8],
            spr_x: [0; 8],
            spr_halted: [true; 8],
            spr_count: 0,
            spr_zero_in_line: false,
            sprite_eval_read_latch: 0xFF,
            sprite_eval_n: 0,
            sprite_eval_m: 0,
            sprite_eval_found: 0,
            sprite_eval_sec_idx: 0,
            sprite_eval_copying: false,
            sprite_eval_done: false,
            sprite_eval_overflow_search: false,
            sprite_eval_zero_found: false,
            sprite_eval_first_iter: false,
            oam_corruption_pending: false,
            oam_corruption_index: 0,
            oam_corruption_disabled: false,
            oam_corruption_disabled_instant: false,
            oam2_addr: 0,
            prev_rendering_enabled: false,
            rendering_enabled_delayed: false,
            bg_reload_render: false,
            mask_write_delay: 0,
            cached_visible: false,
            cached_pre_render: false,
            cached_render_line: false,
            flags_cached_scanline: i16::MIN,
            active_palette: crate::palette::PpuPalette::Composite2C02,
            rgba_lut: build_rgba_lut(crate::palette::PpuPalette::Composite2C02),
            custom_palette: None,
            is_2c05: false,
            id_2c05: 0,
            // v2.1.7 P5 — default revision models no extra corruption; default
            // power-up palette is all-zero. Both keep the default build
            // byte-identical (the `palette_ram: [0u8; 32]` above already reflects
            // the `PaletteInit::Zeroed` default).
            die_revision: PpuRevision::Rp2c02H,
            power_up_palette: PaletteInit::Zeroed,
            framebuffer: vec![0u8; FRAMEBUFFER_LEN].into_boxed_slice(),
            index_framebuffer: vec![0u16; FRAMEBUFFER_PIXELS].into_boxed_slice(),
            dot_counter: 0,
            frame_ntsc_phase: 0,
            extra_scanlines: 0,
            extra_lines_remaining: 0,
            #[cfg(feature = "ppu-state-trace")]
            state_trace: None,
            #[cfg(feature = "hd-pack")]
            hd_tile_source: vec![HdTileSource::default(); FRAMEBUFFER_PIXELS].into_boxed_slice(),
            #[cfg(feature = "hd-pack")]
            hd_bg_addr_latch: HD_TILE_NONE,
            #[cfg(feature = "hd-pack")]
            hd_bg_addr_cur: HD_TILE_NONE,
            #[cfg(feature = "hd-pack")]
            hd_bg_addr_next: HD_TILE_NONE,
            #[cfg(feature = "hd-pack")]
            hd_spr_addr: [HD_TILE_NONE; 8],
            #[cfg(feature = "hd-pack")]
            hd_spr_x: [0; 8],
            #[cfg(feature = "hd-pack")]
            hd_spr_off_y: [0; 8],
            #[cfg(feature = "hd-pack")]
            hd_bg_idx_latch: HD_CHR_RAM,
            #[cfg(feature = "hd-pack")]
            hd_bg_idx_cur: HD_CHR_RAM,
            #[cfg(feature = "hd-pack")]
            hd_bg_idx_next: HD_CHR_RAM,
            #[cfg(feature = "hd-pack")]
            hd_spr_idx: [HD_CHR_RAM; 8],
        };
        // Clear status flags that match power-on per nesdev wiki: VBL is
        // unspecified on power-on. We start clear.
        p.status = PpuStatus::empty();
        p
    }

    /// Configure the PPU's hardware variant for Vs. System / PlayChoice-10
    /// arcade carts.
    ///
    /// `palette` selects the output palette (the RGB PPUs replace the 2C02
    /// composite palette with a fixed hardware RGB lookup). `is_2c05` enables
    /// the 2C05's register quirks: a write to `$2000` sets MASK and a write to
    /// `$2001` sets CTRL (swapped), and a `$2002` read ORs `id` into its low
    /// bits. For a 2C02 (the default NES/Famicom path) this is never called, so
    /// `active_palette` stays [`crate::palette::PpuPalette::Composite2C02`],
    /// `is_2c05` stays `false`, and behaviour is byte-for-byte unchanged.
    pub const fn set_palette(
        &mut self,
        palette: crate::palette::PpuPalette,
        is_2c05: bool,
        id: u8,
    ) {
        self.active_palette = palette;
        // v2.8.0 Phase 4 — keep the per-pixel RGBA lookup in sync with the active
        // palette (byte-identical by construction; see `rgba_lut`). A loaded `.pal`
        // (`custom_palette`) overrides the built-in table; `rebuild_rgba_lut`
        // honours it.
        self.rebuild_rgba_lut();
        self.is_2c05 = is_2c05;
        self.id_2c05 = id;
    }

    /// v1.1.0 beta.1 (T-110-A3) — install (or clear with `None`) a custom 64-entry
    /// base palette from a loaded `.pal` file and rebuild the RGBA lookup. `None`
    /// restores the built-in palette for the active [`crate::palette::PpuPalette`]
    /// (byte-identical to default). A frontend presentation override.
    pub const fn set_custom_palette(&mut self, base: Option<[[u8; 3]; 64]>) {
        self.custom_palette = base;
        self.rebuild_rgba_lut();
    }

    /// v1.7.0 "Forge" Workstream F3 — set the number of EXTRA blank vblank
    /// scanlines to insert per frame (the PPU extra-scanlines overclock).
    ///
    /// `0` (the default) is stock NES timing and is **byte-identical** to a PPU
    /// that never calls this. A non-zero value lengthens vblank by that many
    /// idle scanlines each frame (more CPU run-time, no visible change), at the
    /// existing dot resolution. Off by default; a frontend config knob, not part
    /// of the save-state. Distinct from the CPU-multiplier overclock (v2.0).
    ///
    /// Changing the count cancels any in-flight insertion for the current
    /// frame: the per-frame countdown (`extra_lines_remaining`) is
    /// reset to `0` so it cannot remain stale or out-of-bounds relative to
    /// the new `lines` (e.g. shrinking 8 → 2, or disabling N → 0). The next
    /// frame reloads the countdown from the new value at the insertion point.
    pub const fn set_extra_scanlines(&mut self, lines: u16) {
        self.extra_scanlines = lines;
        self.extra_lines_remaining = 0;
    }

    /// v1.7.0 F3 — the currently-configured extra-scanline count (`0` = stock).
    #[must_use]
    pub const fn extra_scanlines(&self) -> u16 {
        self.extra_scanlines
    }

    /// v2.1.4 F2.3 — enable or disable the optional OAM-decay accuracy model.
    ///
    /// **Off by default.** When off (the default) OAM reads/writes never consult
    /// the decay state and the deterministic output is **byte-identical** to a
    /// build without the feature — `AccuracyCoin`, the commercial oracle, and the
    /// visual/`external_real_games` regression suites are unaffected. When on, the
    /// PPU refreshes each 8-byte OAM row on every read (sprite evaluation + `$2004`)
    /// and write; a row that goes un-refreshed for more than `OAM_DECAY_CPU_CYCLES`
    /// (3000) CPU cycles decays to Mesen2's canonical garbage pattern on the next
    /// read (`oam_decay_on_read`). The model is NTSC/Dendy-only (PAL's refresh
    /// cadence masks decay) — the region gate lives in the hooks, so it is safe to
    /// enable on any region.
    ///
    /// A frontend/config knob (re-applied on load like `region` / `active_palette`),
    /// **not** part of the save-state. Turning the model ON re-bases every row's
    /// timestamp to the current CPU cycle so a freshly-enabled model does not report
    /// every row as instantly decayed; turning it OFF leaves the timestamps as-is
    /// (they are simply no longer consulted).
    pub const fn set_oam_decay(&mut self, enabled: bool) {
        if enabled && !self.oam_decay_enabled {
            // Freshly enabling: treat every row as just-refreshed so the first
            // post-enable reads don't spuriously report a multi-second-old row as
            // decayed. Idempotent re-enables (already on) skip this so a long
            // rendering-disabled span already in progress keeps decaying.
            let now = self.dot_counter / 3;
            let mut i = 0;
            while i < self.oam_decay_cycles.len() {
                self.oam_decay_cycles[i] = now;
                i += 1;
            }
        }
        self.oam_decay_enabled = enabled;
    }

    /// v2.1.4 F2.3 — whether the optional OAM-decay model is currently enabled.
    #[must_use]
    pub const fn oam_decay_enabled(&self) -> bool {
        self.oam_decay_enabled
    }

    /// v2.1.7 P5 — select the emulated 2C02 die revision (see [`PpuRevision`]).
    ///
    /// The [`PpuRevision::default`] ([`PpuRevision::Rp2c02H`]) models no extra
    /// quirks, so at the default this is behaviorally inert and the PPU is
    /// byte-identical to a build without the field. Selecting
    /// [`PpuRevision::Rp2c02G`] additionally arms the OAMADDR (`$2003`)
    /// write-during-rendering OAM corruption glitch. A construction/config knob,
    /// re-applied on load like the region / active palette — not part of the
    /// save-state.
    pub const fn set_revision(&mut self, revision: PpuRevision) {
        self.die_revision = revision;
    }

    /// v2.1.7 P5 — the currently-selected 2C02 die revision.
    #[must_use]
    pub const fn revision(&self) -> PpuRevision {
        self.die_revision
    }

    /// v2.1.7 P5 — apply a power-up palette-RAM pattern (see [`PaletteInit`]).
    ///
    /// Writes all 32 palette-RAM bytes to the selected pattern and records the
    /// selection so a subsequent power-cycle can re-apply it. The
    /// [`PaletteInit::default`] ([`PaletteInit::Zeroed`]) writes all-zero — the
    /// established power-up state — so at the default this leaves the PPU
    /// byte-identical. Intended to be called at construction / power-on (palette
    /// RAM is not cleared on a warm reset, matching real hardware). It writes
    /// [`Self::palette_ram`] directly, which the snapshot already serializes, so
    /// no snapshot-format change is required.
    pub const fn apply_power_up_palette(&mut self, init: PaletteInit) {
        self.power_up_palette = init;
        match init {
            PaletteInit::Zeroed => {
                let mut i = 0;
                while i < self.palette_ram.len() {
                    self.palette_ram[i] = 0;
                    i += 1;
                }
            }
            PaletteInit::Blargg => {
                let mut i = 0;
                while i < self.palette_ram.len() {
                    // Palette-RAM cells are 6-bit; mask to match a `$2007` write.
                    self.palette_ram[i] = BLARGG_POWER_UP_PALETTE[i] & 0x3F;
                    i += 1;
                }
            }
        }
    }

    /// v2.1.7 P5 — the currently-selected power-up palette pattern.
    #[must_use]
    pub const fn power_up_palette(&self) -> PaletteInit {
        self.power_up_palette
    }

    /// v2.1.4 F2.3 — `true` when the OAM-decay model should act this access:
    /// enabled AND the region is NTSC/Dendy (PAL's frequent refresh masks decay,
    /// so Mesen2 never decays there). This is the single gate every decay hook
    /// funnels through; at the default (disabled) it is a single bool test and the
    /// hooks are behaviour-neutral.
    #[inline]
    const fn oam_decay_active(&self) -> bool {
        self.oam_decay_enabled && !matches!(self.region, PpuRegion::Pal)
    }

    /// v2.1.4 F2.3 — OAM-read decay hook. Call **immediately before** reading
    /// `oam[addr]` at every primary-OAM read site (the `$2004` read and both
    /// sprite-evaluation read paths). Faithful port of Mesen2's `ReadSpriteRam`
    /// (`Core/NES/NesPpu.cpp`):
    ///
    /// - If the model is inactive (disabled or PAL), this is a no-op — `oam` and
    ///   the timestamps are left untouched, so the read is byte-identical to stock.
    /// - Else, for the 8-byte row containing `addr`: if the last touch was within
    ///   [`OAM_DECAY_CPU_CYCLES`] CPU cycles, refresh the row's timestamp (the DRAM
    ///   cell was recharged by this access). Otherwise the row has decayed — rewrite
    ///   all 8 of its bytes to the canonical pattern `((sprAddr & 3) == 2) ?
    ///   (sprAddr & 0xE3) : sprAddr` (the attribute byte keeps only its implemented
    ///   bits; the others read back their own low address) and leave the stale
    ///   timestamp (so the row keeps reading decayed until a write refreshes it,
    ///   exactly like Mesen2).
    ///
    /// The subsequent `oam[addr]` read then returns the (possibly decayed) byte.
    #[inline]
    fn oam_decay_on_read(&mut self, addr: u8) {
        if !self.oam_decay_active() {
            return;
        }
        let row = (addr >> 3) as usize;
        let now = self.dot_counter / 3;
        // Saturating (wrapping) subtraction: `now` is monotone ≥ the stored
        // timestamp in practice, but `wrapping_sub` keeps this total even across a
        // (astronomically unlikely) u64 counter wrap.
        let elapsed = now.wrapping_sub(self.oam_decay_cycles[row]);
        if elapsed <= OAM_DECAY_CPU_CYCLES {
            self.oam_decay_cycles[row] = now;
        } else {
            let base = addr & 0xF8;
            for i in 0..8u8 {
                let spr_addr = base | i;
                self.oam[spr_addr as usize] = if spr_addr & 0x03 == 0x02 {
                    spr_addr & 0xE3
                } else {
                    spr_addr
                };
            }
        }
    }

    /// v2.1.4 F2.3 — OAM-write decay hook. Call **after** writing `oam[addr]` at
    /// every primary-OAM write site (`$2004` / OAM DMA). Faithful port of Mesen2's
    /// `WriteSpriteRam`: a write recharges the row's DRAM cells, so refresh the
    /// row's last-touch timestamp. Inactive (disabled or PAL) ⇒ no-op, so the write
    /// path is byte-identical to stock at the default.
    #[inline]
    const fn oam_decay_on_write(&mut self, addr: u8) {
        if !self.oam_decay_active() {
            return;
        }
        self.oam_decay_cycles[(addr >> 3) as usize] = self.dot_counter / 3;
    }

    /// Rebuild [`Self::rgba_lut`] from the custom palette when one is loaded,
    /// otherwise from the active built-in [`crate::palette::PpuPalette`].
    const fn rebuild_rgba_lut(&mut self) {
        self.rgba_lut = match &self.custom_palette {
            Some(base) => build_rgba_lut_from_base(base),
            None => build_rgba_lut(self.active_palette),
        };
    }

    /// Map a CPU-visible PPU register index (0-7) to the internal register,
    /// applying the 2C05 `$2000`<->`$2001` swap.
    ///
    /// On a 2C05 a write/read of `$2000` (reg 0) targets MASK (reg 1) and vice
    /// versa; all other registers are unaffected. On every other PPU (the
    /// default path) this is the identity, so normal NES behaviour is unchanged.
    const fn map_register(&self, reg: u8) -> u8 {
        if self.is_2c05 {
            match reg & 7 {
                0 => 1,
                1 => 0,
                other => other,
            }
        } else {
            reg & 7
        }
    }

    /// Returns a reference to the internal CIRAM (nametables).
    pub fn vram_ref(&self) -> &[u8] {
        &self.ciram
    }

    /// Returns a mutable reference to the internal CIRAM (nametables).
    pub fn vram_mut(&mut self) -> &mut [u8] {
        &mut self.ciram
    }

    /// Performs a soft-reset of the PPU (warm boot). Per `docs/ppu-2c02.md`:
    ///   - PPUCTRL := 0
    ///   - PPUMASK := 0
    ///   - w toggle := 0
    ///   - PPUSTATUS bits 7 (VBL) unchanged on real hardware (we leave it
    ///     as-is for parity with `$2002`-race tests)
    ///   - PPUDATA buffer := 0
    ///   - Mask window restarts (writes to $2000/$2001/$2005/$2006 ignored
    ///     for the documented number of cycles after reset).
    pub const fn reset(&mut self) {
        self.ctrl = PpuCtrl::empty();
        self.mask = PpuMask::empty();
        self.mask_for_skip_check = PpuMask::empty();
        self.mask_skip_pipe1 = PpuMask::empty();
        self.w = false;
        self.data_buffer = 0;
        self.post_reset_mask_remaining = self.region.post_reset_mask_cycles();
        self.nmi_line = false;
        // v2.1.4 F2.3 — mark every OAM row freshly refreshed at reset, matching
        // Mesen2's `NesPpu::Reset` (which stamps `_oamDecayCycles` with the current
        // clock unconditionally). Doing this regardless of the enable flag keeps the
        // timestamps sane if decay is toggled on after a reset, and is inert while
        // decay is off (the array is never read). `dot_counter / 3` is the current
        // CPU cycle (NTSC/Dendy have 3 dots per CPU cycle).
        let now = self.dot_counter / 3;
        let mut i = 0;
        while i < self.oam_decay_cycles.len() {
            self.oam_decay_cycles[i] = now;
            i += 1;
        }
    }

    /// Returns `true` if the PPU is asserting the NMI line.
    #[must_use]
    pub const fn nmi_line(&self) -> bool {
        self.nmi_line
    }

    /// Consume and return the per-frame "frame complete" latch.
    pub const fn take_frame_complete(&mut self) -> bool {
        let r = self.frame_complete;
        self.frame_complete = false;
        r
    }

    /// Install a state-trace buffer. Subsequent calls to
    /// [`Self::tick`] will append one [`PpuStateRecord`] per dot
    /// for dots inside the buffer's filter window. Pre-existing
    /// records (if any) are dropped.
    ///
    /// Read-only: every call to [`Self::tick`] reads PPU state
    /// after the dot's effects have applied; it never mutates
    /// emulator state, so the determinism contract is preserved
    /// (`docs/architecture.md` §Determinism).
    ///
    /// See `docs/adr/0005-ppu-state-trace.md` and the rustdoc on
    /// [`crate::state_trace`].
    ///
    /// [`PpuStateRecord`]: crate::state_trace::PpuStateRecord
    #[cfg(feature = "ppu-state-trace")]
    pub fn enable_state_trace(&mut self, trace: crate::state_trace::PpuStateTrace) {
        self.state_trace = Some(trace);
    }

    /// Take the accumulated state trace, leaving the PPU's trace
    /// slot empty. Returns `None` if tracing was never enabled.
    #[cfg(feature = "ppu-state-trace")]
    #[must_use]
    pub const fn take_state_trace(&mut self) -> Option<crate::state_trace::PpuStateTrace> {
        self.state_trace.take()
    }

    /// Borrow the in-flight state trace without taking it.
    #[cfg(feature = "ppu-state-trace")]
    #[must_use]
    pub const fn state_trace(&self) -> Option<&crate::state_trace::PpuStateTrace> {
        self.state_trace.as_ref()
    }

    /// Build a [`PpuStateRecord`] snapshot from the PPU's
    /// current state. Used by the per-dot recording hook at the
    /// end of [`Self::tick`]; exposed publicly so external
    /// tooling (e.g. the trace fixture's end-of-frame snapshot)
    /// can re-use the canonical packer.
    ///
    /// [`PpuStateRecord`]: crate::state_trace::PpuStateRecord
    #[cfg(feature = "ppu-state-trace")]
    #[must_use]
    pub fn build_state_record(&self) -> crate::state_trace::PpuStateRecord {
        crate::state_trace::PpuStateRecord {
            // Frames easily exceed u16 over a 600-frame test run.
            // The `as u32` truncates the upper bits of the u64
            // counter — which is fine for any realistic capture
            // window (u32::MAX ≈ 71 days of NES wall time).
            frame: self.frame as u32,
            scanline: self.scanline,
            dot: self.dot,
            ctrl: self.ctrl.bits(),
            mask: self.mask.bits(),
            status: self.status.bits(),
            oam_addr: self.oam_addr,
            v: self.v,
            t: self.t,
            fine_x: self.x,
            w_toggle: self.w,
            sprite_eval_n: self.sprite_eval_n,
            sprite_eval_m: self.sprite_eval_m,
            sprite_eval_found: self.sprite_eval_found,
            sprite_eval_sec_idx: self.sprite_eval_sec_idx,
            sprite_eval_copying: self.sprite_eval_copying,
            sprite_eval_overflow_search: self.sprite_eval_overflow_search,
            sprite_eval_done: self.sprite_eval_done,
            sprite_eval_read_latch: self.sprite_eval_read_latch,
            spr_count: self.spr_count,
            spr_zero_in_line: self.spr_zero_in_line,
            spr_shift_lo: self.spr_shift_lo,
            spr_shift_hi: self.spr_shift_hi,
            spr_attr: self.spr_attr,
            spr_x: self.spr_x,
            bg_shift_lo: self.bg_shift_lo,
            bg_shift_hi: self.bg_shift_hi,
            at_shift_lo: self.at_shift_lo,
            at_shift_hi: self.at_shift_hi,
            nt_latch: self.nt_latch,
            at_latch: self.at_latch,
            bg_lo_latch: self.bg_lo_latch,
            bg_hi_latch: self.bg_hi_latch,
            secondary_oam: self.secondary_oam,
            oam_fnv1a64: crate::state_trace::fnv1a64(&self.oam),
            nmi_line: self.nmi_line,
        }
    }

    /// Borrow the (possibly partial) framebuffer.
    #[must_use]
    pub fn framebuffer(&self) -> &[u8] {
        &self.framebuffer
    }

    /// v1.7.0 "Forge" Workstream B (B3) — overwrite the RGBA8 output framebuffer
    /// (the Lua `emu:setScreenBuffer(t)` paints output only). Copies up to the
    /// framebuffer length; a short source leaves the tail untouched. Output-only
    /// — it touches only the display buffer the frontend presents, NOT any
    /// register / latch / scroll state, so the determinism contract is
    /// unaffected (a later real frame fully repaints it). `debug-hooks`-gated and
    /// reached only through the script crate's gated post-frame path, so the
    /// shipped build is byte-identical.
    #[cfg(feature = "debug-hooks")]
    pub fn debug_set_framebuffer(&mut self, rgba: &[u8]) {
        let n = rgba.len().min(self.framebuffer.len());
        self.framebuffer[..n].copy_from_slice(&rgba[..n]);
    }

    /// Borrow the parallel per-pixel **palette-index** framebuffer
    /// (256 × 240 `u16`s, each `(emphasis << 6) | colour`, 0..=511) used by the
    /// true composite `NES_NTSC` filter (T-110-A1). A faithful index-space mirror
    /// of [`Self::framebuffer`]; output-only, so the determinism contract holds.
    #[must_use]
    pub fn index_framebuffer(&self) -> &[u16] {
        &self.index_framebuffer
    }

    /// Pack a background tile's active palette into Mesen's `PaletteColors` key
    /// form (`pr[base+3] | pr[base+2]<<8 | pr[base+1]<<16 | pr[0]<<24`, with
    /// `base = $3F00 | group<<2` and `pr[0]` the universal backdrop). Used only
    /// to key HD-pack tile replacements; output-only.
    #[cfg(feature = "hd-pack")]
    fn hd_bg_palette_colors(&self, group: u8) -> u32 {
        let base = 0x3F00 | (u16::from(group) << 2);
        let p0 = u32::from(self.read_palette(0x3F00) & 0x3F);
        let p1 = u32::from(self.read_palette(base | 1) & 0x3F);
        let p2 = u32::from(self.read_palette(base | 2) & 0x3F);
        let p3 = u32::from(self.read_palette(base | 3) & 0x3F);
        p3 | (p2 << 8) | (p1 << 16) | (p0 << 24)
    }

    /// Pack a sprite tile's palette (`0xFF000000 | pr[base+3] | pr[base+2]<<8 |
    /// pr[base+1]<<16`, `base = $3F10 | group<<2`; the `0xFF` top byte is the
    /// sprite/BG discriminator and there is no `pr[0]` term).
    #[cfg(feature = "hd-pack")]
    fn hd_sprite_palette_colors(&self, group: u8) -> u32 {
        let base = 0x3F10 | (u16::from(group) << 2);
        let p1 = u32::from(self.read_palette(base | 1) & 0x3F);
        let p2 = u32::from(self.read_palette(base | 2) & 0x3F);
        let p3 = u32::from(self.read_palette(base | 3) & 0x3F);
        0xFF00_0000 | p3 | (p2 << 8) | (p1 << 16)
    }

    /// v1.2.0 beta.2 (Workstream C3) — borrow the per-pixel HD-pack
    /// tile-source buffer (256 × 240 [`HdTileSource`] records, parallel to
    /// [`Self::index_framebuffer`]). Each entry names the CHR tile that
    /// produced the pixel. Output-only telemetry; the determinism /
    /// `AccuracyCoin` contract is unaffected. See
    /// `docs/ppu-2c02.md` §HD-pack tile-source export.
    #[cfg(feature = "hd-pack")]
    #[must_use]
    pub fn hd_tile_source(&self) -> &[HdTileSource] {
        &self.hd_tile_source
    }

    /// The per-frame NTSC composite colour phase — the `videoPhase` the
    /// `NES_NTSC` filter feeds its signal generator. `0..=2` on NTSC; on
    /// PAL/Dendy it is the frame parity (`0..=1`). Snapshotted at the last frame
    /// boundary. Cosmetic (drives only the optional filter's dot-crawl).
    #[must_use]
    pub const fn ntsc_phase(&self) -> u8 {
        self.frame_ntsc_phase
    }

    /// Snapshot the per-frame NTSC colour phase from the master-cycle counter.
    /// NES NTSC steps the colour phase through 3 frame states (the source of the
    /// dot-crawl); PAL/Dendy have no equivalent 3-phase crawl, so the frame
    /// parity is exposed instead. Called at each frame boundary.
    const fn snapshot_ntsc_phase(&mut self) {
        self.frame_ntsc_phase = if matches!(self.region, PpuRegion::Ntsc) {
            (self.dot_counter % 3) as u8
        } else {
            (self.frame & 1) as u8
        };
    }

    /// Current dot (0..=340).
    #[must_use]
    pub const fn dot(&self) -> u16 {
        self.dot
    }

    /// Current scanline.
    #[must_use]
    pub const fn scanline(&self) -> i16 {
        self.scanline
    }

    /// Current frame counter.
    #[must_use]
    pub const fn frame(&self) -> u64 {
        self.frame
    }

    /// Snapshot of CPU-visible register bytes (for the debugger UI).
    ///
    /// Returns `[ctrl, mask, status, oam_addr]`. Read-only — does NOT clear
    /// VBL or toggle the write latch (unlike `cpu_read_register`).
    #[must_use]
    pub const fn debug_registers(&self) -> [u8; 4] {
        [
            self.ctrl.bits(),
            self.mask.bits(),
            self.status.bits(),
            self.oam_addr,
        ]
    }

    /// Snapshot of loopy scroll registers `(v, t, x, w)`.
    #[must_use]
    pub const fn debug_scroll(&self) -> (u16, u16, u8, bool) {
        (self.v, self.t, self.x, self.w)
    }

    /// v1.8.9 — the frame's background scroll `(x, y)` in NES pixels, decoded
    /// from the `t` (temp VRAM addr) register + fine-X, including the nametable
    /// bits (Mesen HD-pack `_scrollX`/`scrollY`). Used by the HD compositor to
    /// offset parallax `<background>` layers by `scroll * ratio`. A frame-level
    /// value (the scroll at `t`), not per-scanline. Output-only.
    #[must_use]
    pub const fn hd_bg_scroll(&self) -> (i32, i32) {
        let t = self.t;
        let x = ((t & 0x1F) << 3) | (self.x as u16) | if t & 0x0400 != 0 { 0x100 } else { 0 };
        let y =
            (((t & 0x03E0) >> 2) | ((t & 0x7000) >> 12)) + if t & 0x0800 != 0 { 240 } else { 0 };
        (x as i32, y as i32)
    }

    /// Borrow the 32-byte palette RAM (read-only).
    #[must_use]
    pub const fn palette_ram(&self) -> &[u8; 32] {
        &self.palette_ram
    }

    /// Borrow OAM (256 bytes = 64 sprites x 4 bytes).
    #[must_use]
    pub fn oam(&self) -> &[u8] {
        &self.oam
    }

    /// Borrow nametable CIRAM (2 KiB).
    #[must_use]
    pub fn ciram(&self) -> &[u8] {
        &self.ciram
    }

    /// v1.7.0 "Forge" Workstream A1 — debugger writeback: store one palette-RAM
    /// byte directly (`idx` masked to 0..32, value masked to the 6-bit palette
    /// width), reusing the same canonical mirroring/masking as the live
    /// `$2007` write path. Used only by the `debug-hooks` editor writeback,
    /// which routes through the gated post-frame poke path — so the default
    /// (no-edit) build never calls it and stays byte-identical.
    #[cfg(feature = "debug-hooks")]
    pub const fn debug_poke_palette(&mut self, idx: u8, value: u8) {
        // `palette_index` mirrors $3F10/$14/$18/$1C → $3F00/.. and folds the
        // 32-byte window; feed it the raw address so an editor index maps the
        // same way a $2007 write would.
        let addr = 0x3F00u16 | ((idx & 0x1F) as u16);
        let i = palette_index(addr);
        self.palette_ram[i] = value & 0x3F;
    }

    /// v1.7.0 "Forge" Workstream A1 — debugger writeback: store one OAM byte
    /// directly. `debug-hooks`-gated; only reached through the gated post-frame
    /// poke path, so the default build is byte-identical.
    #[cfg(feature = "debug-hooks")]
    pub const fn debug_poke_oam(&mut self, idx: u8, value: u8) {
        self.oam[idx as usize] = value;
    }

    /// v1.7.0 "Forge" Workstream A1 — debugger writeback: store one CIRAM byte
    /// at a physical offset (caller resolves mirroring via the mapper).
    /// `debug-hooks`-gated; only reached through the gated post-frame poke path.
    #[cfg(feature = "debug-hooks")]
    pub const fn debug_poke_ciram(&mut self, phys: usize, value: u8) {
        self.ciram[phys & 0x07FF] = value;
    }

    /// `true` when sprites are rendered in 8x16 mode (CTRL bit 5).
    #[must_use]
    pub const fn sprite_size_16(&self) -> bool {
        self.ctrl
            .contains(crate::registers::PpuCtrl::SPRITE_SIZE_16)
    }

    /// Base address of the BG pattern table (`$0000` or `$1000`).
    #[must_use]
    pub const fn bg_pattern_base(&self) -> u16 {
        if self
            .ctrl
            .contains(crate::registers::PpuCtrl::BG_PATTERN_HIGH)
        {
            0x1000
        } else {
            0x0000
        }
    }

    /// Base address of the sprite pattern table (8x8 mode only).
    #[must_use]
    pub const fn sprite_pattern_base(&self) -> u16 {
        if self
            .ctrl
            .contains(crate::registers::PpuCtrl::SPRITE_PATTERN_HIGH)
        {
            0x1000
        } else {
            0x0000
        }
    }

    /// OAM DMA byte write: place `value` at `oam[oam_addr]` and increment
    /// `oam_addr`. Used by the bus's OAM DMA state machine.
    ///
    /// Bypasses the OAMADDR-during-rendering corruption modeled by
    /// `cpu_write_register` for `$2004` direct writes — DMA writes always
    /// hit OAM directly per nesdev.
    pub fn oam_dma_write(&mut self, value: u8) {
        self.oam[self.oam_addr as usize] = value;
        // v2.1.4 F2.3 — OAM-decay write hook (no-op at the default): the DMA byte
        // recharges the written row's DRAM cells, so refresh its timestamp. Mesen2
        // routes DMA writes through the same `WriteSpriteRam` refresh.
        self.oam_decay_on_write(self.oam_addr);
        self.oam_addr = self.oam_addr.wrapping_add(1);
    }

    /// Notify the PPU that one CPU cycle has elapsed. Used to drive the
    /// post-reset masking window and the open-bus decay timers.
    pub const fn on_cpu_cycle(&mut self) {
        self.post_reset_mask_remaining = self.post_reset_mask_remaining.saturating_sub(1);
        // Open-bus decay: per-bit-group, three independent timers.  When a
        // group's timer hits 0 those bits clear in the latch.  Per
        // docs/ppu-2c02.md, real hardware decays in 3-30 ms; we use ~600 ms
        // (≈ 1,073,447 CPU cycles at NTSC, rounded to one million).  This is
        // conservative but well within the window the `ppu_open_bus` test
        // cares about.
        let mut i = 0;
        while i < 3 {
            if self.open_bus_decay[i] > 0 {
                self.open_bus_decay[i] -= 1;
                if self.open_bus_decay[i] == 0 {
                    self.open_bus &= !Self::OPEN_BUS_GROUP_MASKS[i];
                }
            }
            i += 1;
        }
    }

    /// Per-bit-group masks for the open-bus latch decay model.  Group 0 is
    /// bits 0-4 (refreshed by writes, $2004 reads, and $2007 reads — both
    /// palette and non-palette).  Group 1 is bit 5 (refreshed by writes,
    /// $2002 reads, $2004 reads, and $2007 reads).  Group 2 is bits 6-7
    /// (refreshed by writes, $2002 reads, $2004 reads, and $2007 non-palette
    /// reads — but not by palette reads).
    const OPEN_BUS_GROUP_MASKS: [u8; 3] = [0x1F, 0x20, 0xC0];

    /// Decay-timer reload value (~600 ms at NTSC).
    const OPEN_BUS_DECAY_RELOAD: u32 = 1_000_000;

    /// Refresh the open-bus latch.  `group_mask` is a bitmap selecting which
    /// of the three decay groups to refresh: bit 0 = bits 0-4, bit 1 = bit 5,
    /// bit 2 = bits 6-7.  Only the bits in those groups are copied from
    /// `value`; bits in groups not selected retain their previous latch value
    /// and their decay timer is left to keep counting down.
    const fn refresh_open_bus(&mut self, value: u8, group_mask: u8) {
        let mut i = 0;
        while i < 3 {
            if (group_mask >> i) & 1 == 1 {
                let m = Self::OPEN_BUS_GROUP_MASKS[i];
                self.open_bus = (self.open_bus & !m) | (value & m);
                self.open_bus_decay[i] = Self::OPEN_BUS_DECAY_RELOAD;
            }
            i += 1;
        }
    }

    /// Refresh **all** bit groups of the open-bus latch — used by writes and
    /// any read that drives all 8 bits (e.g. $2004 OAMDATA, $2007 non-palette
    /// PPUDATA).
    const fn touch_open_bus(&mut self, value: u8) {
        self.refresh_open_bus(value, 0b111);
    }

    /// Number of PPU dots between a `$2002` read's start (M2 high, when the
    /// VBL flag is latched) and its end (M2 low, when the *unlatched*
    /// sprite-0-hit / overflow flags are sampled). On a revision-G 2A03 M2's
    /// 15/24 duty cycle puts read-end ~1.875 PPU dots after read-start; we
    /// round to the nearest whole dot for the lockstep computed sample. This
    /// is the empirically-tuned knob for the `$2002 flag timing` test.
    ///
    /// Tuned to **1**: with the test's reads spaced 1 PPU dot apart, a 1-dot
    /// window yields the primary answer key `$E0,$E0,$80,$00` (read 3 = `$80`:
    /// VBL latched set, sprite flags read 0). A 2-dot window also passes Test 1
    /// (via the alt key `$E0,$80,$80,$00`) but masks one extra read position,
    /// which regresses `$2004 Stress` (whose `$2002` sync read lands there).
    const STATUS_READ_END_DOTS: u16 = 1;

    /// True when advancing `dots` PPU dots from the current `(scanline, dot)`
    /// reaches or passes the pre-render dot-1 flag-clear, where the
    /// sprite-0-hit and overflow flags are cleared. Used by the `$2002`
    /// two-point read model to sample bits 6/5 as-of read-end. Forward
    /// distance only: a position already at/after this frame's clear is *not*
    /// "imminent" (its flags are already cleared in the status register, and
    /// the full-frame wrap distance never satisfies the small `dots` bound).
    fn sprite_flags_clear_imminent(&self, dots: u16) -> bool {
        const DOTS_PER_LINE: i32 = 341;
        let lines = i32::from(self.region.prerender_line()) + 1;
        let frame_dots = lines * DOTS_PER_LINE;
        let target = i32::from(self.region.prerender_line()) * DOTS_PER_LINE + 1;
        let cur = i32::from(self.scanline) * DOTS_PER_LINE + i32::from(self.dot);
        let until = (target - cur).rem_euclid(frame_dots);
        until > 0 && until <= i32::from(dots)
    }

    /// CPU register read at `$2000-$3FFF` (only the low 3 bits matter).
    #[allow(clippy::too_many_lines)]
    pub fn cpu_read_register<B: PpuBus>(&mut self, reg: u8, bus: &mut B) -> u8 {
        // 2C05 swaps $2000<->$2001 (no-op on every other PPU).
        match self.map_register(reg) {
            // $2000 / $2001 / $2003 / $2005 / $2006 are write-only; reads
            // return open-bus.
            0 | 1 | 3 | 5 | 6 => self.open_bus,
            2 => {
                // $2002 PPUSTATUS. High 3 bits are real; low 5 are open-bus.
                // `v` is the value as-of read-start (M2 high); it both feeds the
                // open-bus I/O-latch refresh below and is the base for the
                // CPU-visible return value. The Tier-1.1 read-end sample (below)
                // is applied ONLY to the returned byte, NOT to the open-bus
                // refresh — keeping the decay model byte-identical so real games
                // that read open bus after a `$2002` read are unaffected.
                // On a 2C05 the low 5 bits return the PPU identifier (copy
                // protection) instead of PPU open bus; on every other PPU they
                // are the open-bus latch (byte-identical to the legacy path).
                let low5 = if self.is_2c05 {
                    self.id_2c05 & 0x1F
                } else {
                    self.open_bus & 0x1F
                };
                let v = (self.status.bits() & 0xE0) | low5;
                // Clear VBL and the w toggle as a side effect.
                self.status.remove(PpuStatus::VBLANK);
                self.w = false;
                // R2 (master-clock R1 substrate): a $2002 read drops the /NMI
                // line UNCONDITIONALLY (Mesen2 `UpdateStatusFlag:588`
                // `ClearNmiFlag()`; TetaNES `read_status: nmi_pending=false`).
                // Correct under R1's on-time access (the read lands at the
                // access's exact dot); the CPU's φ2 edge detector sees the
                // level fall on the same access.
                {
                    self.nmi_line = false;
                }
                // Race: reading PPUSTATUS at exactly the cycle VBL would
                // have been set suppresses VBL + NMI for that frame. We
                // approximate the race window as scanline 241 dot 0 (the
                // dot before set) and dot 1 (the set dot).
                //
                // Session-18 / C1 attempt 16 (PPU-axis) investigated
                // tightening the predicate from `dot <= 1` to `dot == 0`
                // (matching Mesen2's `Core/NES/NesPpu.cpp::
                // UpdateStatusFlag()` `_cycle == 0` strict 1-dot window
                // and the nesdev wiki spec) but ROLLED IT BACK because
                // it did NOT flip the failing `cpu_interrupts_v2/{2,3,5}`
                // tests. The empirical oracle in
                // `ppu::tests::vbl_race_window_2002_read_sweep` (added
                // in Session-18) shows the predicate change cleanly
                // narrows the window AT the unit-test layer, but the
                // load-bearing axis at the integration-test layer is the
                // CPU-vs-PPU per-cycle access interleaving — a deeper
                // architectural surface that the Session-13 cold-boot
                // alignment closed at the FRAME-anchor level but NOT at
                // the INTRA-CYCLE phase level. See
                // `docs/audit/session-18-c1-attempt16-ppu-axis-rollback-2026-05-22.md`
                // and ADR-0002 §"Decision update (2026-05-22, Session-18)".
                // C1 attempt 18 (coordinated with the CPU-side φ1/φ2
                // split): when the access-reorder feature is enabled,
                // narrow the suppression window from `dot <= 1` to
                // `dot == 0` per Mesen2 line 590 + nesdev wiki spec.
                // The CPU-side shift puts our BIT $2002 reads at dot 1
                // (post-φ1-tick) instead of dot 0, and the dot-1 read
                // should NOT trigger suppression (it sees the
                // just-set VBL).  Both changes together close the
                // `cpu_interrupts_v2/{2,3,5}` sync_vbl divergence
                // documented in Session-17/18 audits.
                // R2 (mc-r1-substrate) narrows the race window to `dot == 0`
                // (Mesen2 `:590`) — on the on-time substrate a dot-1 read is a
                // normal post-set read; only dot-0 (one PPU clock before VBL
                // set) arms suppression.
                let in_race_window =
                    self.scanline == self.region.vblank_start_line() && self.dot == 0;
                if in_race_window {
                    self.suppress_vbl_this_frame = true;
                    // If NMI was already raised on dot 1 this same cycle,
                    // pull it back down too.
                    self.nmi_line = false;
                }
                // Reading PPUSTATUS only refreshes the upper 3 bits of the
                // open-bus latch (the bits sourced from the status register);
                // the lower 5 bits retain both their previous value AND their
                // decay timer.  See nesdev wiki "PPU registers" §"Open bus",
                // `cpu_dummy_writes_ppumem` test ROM (open_bus_read_test 2),
                // and `ppu_open_bus.nes` test 7
                // ("Reading $2002 shouldn't refresh low 5 bits of decay value").
                // Refresh groups 1 (bit 5) and 2 (bits 6-7) only.
                // Uses the read-start value `v` (the Tier-1.1 read-end mask is
                // applied to the return value only, below).
                self.refresh_open_bus(v, 0b110);
                // v2.0 Tier 1.1 — $2002 two-point intra-read flag sampling.
                // VBL (bit 7) is latched at read-start (M2 high) = the current
                // dot, already captured in `v`. The sprite-0 (bit 6) and
                // overflow (bit 5) flags are NOT latched; the CPU samples them
                // at read-end (M2 low), ~1.875 PPU dots later. A read straddling
                // the pre-render dot-1 flag-clear therefore returns VBL still set
                // while the sprite flags already read 0 — the AccuracyCoin
                // `$2002 flag timing` answer key `$E0,$E0,$80,$00`. Mask bits 6/5
                // on the returned byte when read-end lands at/after the clear
                // (TriCNES `EmulateUntilEndOfRead`). Returned-value-only so the
                // open-bus latch stays byte-identical for real games.
                if self.sprite_flags_clear_imminent(Self::STATUS_READ_END_DOTS) {
                    return v & !0x60;
                }
                v
            }
            4 => {
                // $2004 OAMDATA. Returns OAM[OAMADDR] without auto-increment.
                // Sprite attribute bytes (every 4th byte starting at offset 2)
                // have bits 2-4 unimplemented in OAM and always read as 0,
                // even though writes can store them.  See nesdev wiki "PPU
                // OAM" → "Byte 2 (attributes)".
                //
                // v2.0 Tier 1.2: while the screen is being drawn on a visible
                // scanline, $2004 returns the value the PPU is currently using
                // for sprite evaluation / loading (the OAM data bus), NOT
                // OAM[OAMADDR]. The isolated `ppu-oam-data-bus` model
                // (`oam_data_bus_read`) reproduces this per AccuracyCoin
                // `$2004 Stress`; see Mesen2 `NesPpu.cpp:298-313/361-380`.
                {
                    if self.scanline <= 239
                        && self.is_render_scanline()
                        && self.mask.rendering_enabled()
                    {
                        let v = self.oam_data_bus_read();
                        self.touch_open_bus(v);
                        return v;
                    }
                }
                // v2.1.4 F2.3 — OAM-decay read hook (no-op at the default): a
                // non-rendering `$2004` read refreshes the row, or returns the
                // decayed pattern if it has gone stale. Must run before the read.
                self.oam_decay_on_read(self.oam_addr);
                let mut v = self.oam[self.oam_addr as usize];
                if (self.oam_addr & 0x03) == 0x02 {
                    v &= 0xE3;
                }
                // Per nesdev wiki "PPU registers" §$2004 + AccuracyCoin
                // "Address $2004 behavior" sub-tests 4 + 9: during dots
                // 1-64 of every rendered scanline (the secondary-OAM
                // clear phase) AND during dots 257-320 (the sprite-tile-
                // loading interval — also when the secondary-OAM bytes
                // are being read out to the shift registers), $2004
                // reads return $FF.
                //
                // (When `ppu-oam-data-bus` is on, the rendering case returns
                // above; this fallback covers the flag-off build + the
                // non-rendering paths.)
                if self.is_render_scanline()
                    && self.mask.rendering_enabled()
                    && ((1..=64).contains(&self.dot) || (257..=320).contains(&self.dot))
                {
                    v = 0xFF;
                }
                self.touch_open_bus(v);
                v
            }
            7 => {
                // Diagnostic: log where each $2007 read lands (scanline/dot/mask).
                if (4400..6200).contains(&self.frame) {
                    use core::sync::atomic::Ordering::Relaxed;
                    let i = read2007_diag::IDX.fetch_add(1, Relaxed) as usize;
                    if i < 1024 {
                        #[allow(clippy::cast_sign_loss)]
                        let sl = ((i32::from(self.scanline) + 1) as u32) & 0x1FF;
                        let packed = (sl << 18)
                            | ((u32::from(self.dot) & 0x1FFF) << 5)
                            | (u32::from(self.mask.rendering_enabled()) << 1)
                            | u32::from(self.is_render_scanline());
                        read2007_diag::LOG[i].store(packed, Relaxed);
                    }
                }
                // $2007 PPUDATA. Buffered for $0000-$3EFF; palette reads
                // bypass the buffer but still update it with the underlying
                // nametable mirror.
                let addr = self.v & 0x3FFF;
                let is_palette = addr >= 0x3F00;
                // W2: set when THIS read arms the PPUDATA SM countdown with the
                // defer-v-inc sub-knob on — the v-glitch increment then happens
                // at the TStep (the countdown landing dot), not here.
                let mut defer_v_inc = false;
                let result = if is_palette {
                    // Palette read: high 2 bits = open bus.
                    let palette = self.read_palette(addr);
                    let v_with_open_bus = (palette & 0x3F) | (self.open_bus & 0xC0);
                    // Buffer gets the underlying nametable byte (from CIRAM).
                    self.data_buffer = self.read_vram(bus, addr & 0x2FFF);
                    v_with_open_bus
                } else {
                    let r = self.data_buffer;
                    // During rendering the buffer is NOT loaded from a read at
                    // `v`. TriCNES (`Emulator.cs` `PPU_DATA_StateMachine` +
                    // the `$2007` CPU read): the read-END arms a latch cascade
                    // and the actual `PPU_ReadBuffer` reload happens ~4 dots
                    // later, latching the value the BG/sprite FETCH cadence
                    // drove on the VRAM bus at the LANDING dot (the fetch has
                    // bus priority). Modeled as a PPU-dot countdown consumed
                    // in `Ppu::tick`; the returned value stays the OLD buffer
                    // (the priming-read contract). Delay 0 = immediate latch
                    // of the current bus value.
                    if self.mask.rendering_enabled() && self.is_render_scanline() {
                        octal_trace::push(
                            octal_trace::K_R2007,
                            self.frame,
                            self.scanline,
                            self.dot,
                            u32::from(self.v & 0x3FFF),
                        );
                        let n = read2007_diag::RENDER_BUFFER_DOT_DELAY
                            .load(core::sync::atomic::Ordering::Relaxed)
                            as u8;
                        if n == 0 {
                            self.data_buffer = self.render_data_bus;
                        } else {
                            self.ppudata_sm_countdown = n;
                            // TriCNES `PPU_DATA_StateMachine_Half`: the TStep
                            // (v-glitch increment) fires at the SAME dot as the
                            // PD_RB buffer reload, AFTER it — so the fetches in
                            // the read-to-reload window still use the OLD `v`.
                            if read2007_diag::RENDER_BUFFER_DEFER_V_INC
                                .load(core::sync::atomic::Ordering::Relaxed)
                                != 0
                            {
                                self.ppudata_v_inc_pending = true;
                                defer_v_inc = true;
                            }
                        }
                    } else {
                        self.data_buffer = self.read_vram(bus, addr);
                    }
                    r
                };
                // Per nesdev "PPU rendering"
                // (https://www.nesdev.org/wiki/PPU_scrolling#$2007_reads_and_writes_during_rendering):
                // "Reading or writing PPUDATA during rendering (on the
                // pre-render line and the visible lines 0-239, only when
                // rendering is enabled) does not increment the address
                // normally, but instead increments both coarse X scroll
                // and Y scroll simultaneously, with normal wrapping."
                // This is the canonical "$2007 read w/ rendering" quirk
                // that AccuracyCoin's `PPU Behavior :: $2007 read w/
                // rendering` Test 2 brackets.
                //
                // W2 (`mc-ppu-2007-render-buffer` + defer-v-inc sub-knob): when
                // this read armed the PPUDATA SM countdown, the increment is
                // performed at the TStep (the countdown landing dot in
                // `Ppu::tick`) instead of here.
                let apply_inc_now = !defer_v_inc;
                if apply_inc_now {
                    if self.mask.rendering_enabled() && self.is_render_scanline() {
                        self.inc_hori_v();
                        self.inc_vert_v();
                    } else {
                        let inc = if self.ctrl.contains(PpuCtrl::VRAM_INCREMENT_32) {
                            32
                        } else {
                            1
                        };
                        self.v = self.v.wrapping_add(inc) & 0x7FFF;
                    }
                }
                // A12 transition can occur here.
                self.observe_a12(bus);
                if is_palette {
                    // Palette reads only refresh bits 0-5 of the decay model
                    // (palette is 6-bit); bits 6-7 retain their previous
                    // value AND timer.  Required by `ppu_open_bus.nes` test 9.
                    self.refresh_open_bus(result, 0b011);
                } else {
                    self.touch_open_bus(result);
                }
                result
            }
            _ => unreachable!(),
        }
    }

    /// CPU register write.
    // Large by nature: an 8-way `$2000-$2007` register-dispatch match, each arm
    // carrying its own hardware-quirk handling (the v2.0.2 octal-latch `$2006`
    // hook nudged it past the 100-line lint threshold).
    #[allow(clippy::too_many_lines)]
    pub fn cpu_write_register<B: PpuBus>(&mut self, reg: u8, value: u8, bus: &mut B) {
        // Open-bus latch always picks up the written value.
        self.touch_open_bus(value);
        // 2C05 swaps $2000<->$2001 (no-op on every other PPU).
        match self.map_register(reg) {
            0 => {
                // $2000 PPUCTRL.
                if self.post_reset_mask_remaining > 0 {
                    return;
                }
                let prev_nmi_enable = self.ctrl.contains(PpuCtrl::NMI_ENABLE);
                self.ctrl = PpuCtrl::from_bits_truncate(value);
                // t bits 11-10 = nametable bits 1-0.
                self.t = (self.t & 0xF3FF) | ((u16::from(value) & 0x03) << 10);
                // NMI bit 0->1 transition while VBL set asserts NMI immediately.
                let new_nmi_enable = self.ctrl.contains(PpuCtrl::NMI_ENABLE);
                if !prev_nmi_enable && new_nmi_enable && self.status.contains(PpuStatus::VBLANK) {
                    self.nmi_line = true;
                }
                if !new_nmi_enable {
                    // Disabling NMI lowers the line.
                    self.nmi_line = false;
                }
            }
            1 => {
                // $2001 PPUMASK.
                if self.post_reset_mask_remaining > 0 {
                    return;
                }
                let was_rendering = self.mask.rendering_enabled();
                self.mask = PpuMask::from_bits_truncate(value);
                self.arm_oam_corruption_disable(was_rendering);
                // v2.0 Phase 6 (mc-ppu-subpos): arm the analog `$2001` BG-reload
                // delay. `self.mask` (and so the sprite-eval / shift / pixel
                // path) updates IMMEDIATELY — only the BG shift-register RELOAD
                // is gated on a value delayed `MASK_WRITE_DELAY` dots behind the
                // mask (TriCNES gates `PPU_Render_ShiftRegistersAndBitPlanes` ->
                // the reload on `PPU_Mask_Show*_Delayed`, while the per-half-dot
                // SHIFT runs on the IMMEDIATE mask). On a render re-enable edge
                // the shifter therefore advances for several dots (injecting the
                // serial-in '1') BEFORE the reload resumes — so one reload is
                // SKIPPED and the accumulated '1's reach the output (BG Serial
                // In) WITHOUT perturbing the sprite path (Stale Sprite Shift
                // Regs) or normal rendering (the reload value latched between
                // toggles equals the live mask -> byte-identical).
                // Freeze the BG-reload gate at its prior value for the analog
                // write-delay window; `tick` re-syncs it to the live mask once
                // the countdown settles.
                {
                    self.mask_write_delay =
                        MASK_WRITE_DELAY.load(core::sync::atomic::Ordering::Relaxed);
                }
            }
            2 => {
                // $2002 is read-only; writes only update the open-bus latch
                // (already done above) and otherwise have no effect.
            }
            3 => {
                // $2003 OAMADDR.
                self.oam_addr = value;
                // v2.1.7 P5 — OAMADDR ($2003) write-during-rendering OAM
                // corruption, modeled only on the earlier `Rp2c02G` revision
                // (default `Rp2c02H` skips this entirely → byte-identical). On
                // real "rev E+" 2C02 silicon, writing $2003 while rendering is
                // active corrupts one OAM "row"; RustyNES arms the shared
                // `CorruptOAM` row-copy (see `process_oam_corruption`) targeting
                // the row the write's high bits select, committed on the next
                // rendered dot. The `!oam_corruption_pending` guard defers to an
                // already-armed corruption (e.g. the rendering-disable model) so
                // the two sources never race. See `docs/ppu-2c02.md` (§OAMADDR
                // corruption) and `docs/accuracy-ledger.md` for the honesty note.
                if self.die_revision.models_oamaddr_corruption()
                    && self.mask.rendering_enabled()
                    && self.is_render_scanline()
                    && !self.oam_corruption_pending
                {
                    self.oam_corruption_pending = true;
                    self.oam_corruption_index = (value >> 3) & 0x1F;
                }
            }
            4 => {
                // $2004 OAMDATA write. Per nesdev §PPU OAM:
                //
                // - Outside rendering (or rendering disabled): write the
                //   value to OAM[OAMADDR] and increment OAMADDR by 1.
                // - During rendering (visible / pre-render scanline with
                //   rendering enabled): the write is BLOCKED (real chip
                //   does a glitchy "OAM read" instead, value discarded),
                //   but OAMADDR is still incremented by **4** (NOT 1) —
                //   the silicon's OAMADDR-bump-on-rendering-write quirk
                //   that AccuracyCoin's `Sprite Evaluation :: Misaligned
                //   OAM behavior` test (T-60-002, 2026-05-17) brackets.
                //
                // Pre-fix our impl always incremented by 1; matches the
                // outside-rendering path but is wrong during rendering.
                if self.mask.rendering_enabled() && self.is_render_scanline() {
                    // During-rendering quirk: OAMADDR += 4, then mask
                    // with $FC (clear bottom 2 bits — re-align to a
                    // 4-byte sprite boundary). Required for
                    // AccuracyCoin's "Address $2004 behavior" sub-test
                    // A which writes $2004 with OAMADDR=1 during
                    // rendering, then expects subsequent reads at
                    // OAMADDR=4 (= (1+4) & $FC) to read OAM[4].
                    self.oam_addr = self.oam_addr.wrapping_add(4) & 0xFC;
                } else {
                    self.oam[self.oam_addr as usize] = value;
                    // v2.1.4 F2.3 — OAM-decay write hook (no-op at the default):
                    // a direct `$2004` write refreshes the written row.
                    self.oam_decay_on_write(self.oam_addr);
                    self.oam_addr = self.oam_addr.wrapping_add(1);
                }
            }
            5 => {
                // $2005 PPUSCROLL.
                if self.post_reset_mask_remaining > 0 {
                    return;
                }
                if self.w {
                    // Second write — Y scroll.
                    self.t = (self.t & 0x8C1F)
                        | ((u16::from(value) & 0xF8) << 2)
                        | ((u16::from(value) & 0x07) << 12);
                    self.w = false;
                } else {
                    // First write — X scroll.
                    self.t = (self.t & 0xFFE0) | (u16::from(value) >> 3);
                    self.x = value & 0x07;
                    self.w = true;
                }
            }
            6 => {
                // $2006 PPUADDR.
                if self.post_reset_mask_remaining > 0 {
                    return;
                }
                if self.w {
                    // Second write — low byte; copy t to v.
                    self.t = (self.t & 0xFF00) | u16::from(value);
                    // v2.0.3 (ADR 0030, Option 1) — "Hybrid Addresses" the natural
                    // way. During rendering, a `$2006` second write does NOT copy
                    // `t -> v` immediately; it stages the delayed-`CopyV` countdown
                    // (`TriCNES` `PPU_Update2006Delay`). The `v = t` and the
                    // `address_bus = v` splice happen when the countdown lands
                    // (`Self::tick`), by which point the fetch cadence has advanced
                    // coarse-X and the per-group phase-0 nametable ALE has NATURALLY
                    // loaded `octal_latch` with the one-tile-ahead NT-low (`$19`),
                    // so the landing read splices `$2F00 | $19 = $2F19` with no
                    // reconstruction. Outside rendering the copy is immediate (the
                    // delay is unobservable there and would only risk shifting
                    // tightly-timed non-render code), so non-render behavior is
                    // unchanged.
                    let deferred_copy_v = self.mask.rendering_enabled()
                        && self.is_render_scanline()
                        // Only within the active BG-fetch window (visible dots
                        // 1..=256 + the dots-321..=336 prefetch). The "Hybrid
                        // Addresses" corruption can ONLY manifest when a background
                        // fetch is in flight to consume the stale octal latch; a
                        // `$2006` write during the sprite/HBlank interval
                        // (257..=320) has no BG-fetch consumer, so deferring `v = t`
                        // there would serve no accuracy purpose and only risk
                        // shifting the many commercial mid-frame scroll splits
                        // (SMB3's status-bar `$2006`/`$2005`, MMC5 titles) that
                        // write during HBlank. Narrowing to the fetch window keeps
                        // the delayed-`CopyV` surgical to the modeled artifact.
                        && ((1..=256).contains(&self.dot) || (321..=336).contains(&self.dot))
                        && {
                            // Alignment-dependent delay (TriCNES uses 4 for three of
                            // four CPU/PPU phases, 5 for one). The `$2006` write is
                            // applied at the START of a CPU cycle in RustyNES's
                            // lockstep bus (before that cycle's 3 PPU ticks); the
                            // corrupted NT read is the phase-1 dot of the fetch group
                            // one coarse-X past the write. Empirically calibrated
                            // against the TriCNES per-dot trace (see the campaign
                            // plan); the AccuracyCoin test also retries across
                            // frames/alignments so at least one alignment lands.
                            self.copy_v_delay = COPY_V_DELAY;
                            octal_trace::push(
                                octal_trace::K_W2006,
                                self.frame,
                                self.scanline,
                                self.dot,
                                u32::from(self.t & 0x3FFF),
                            );
                            true
                        };
                    if !deferred_copy_v {
                        self.v = self.t;
                        // PPUADDR write can flip A12.
                        self.observe_a12(bus);
                    }
                    self.w = false;
                } else {
                    // First write — high byte (clears bit 14 of t).
                    self.t = (self.t & 0x00FF) | ((u16::from(value) & 0x3F) << 8);
                    self.w = true;
                }
            }
            7 => {
                // $2007 PPUDATA write. Same rendering quirk as the
                // read path (see `cpu_read_register` case 7 docstring):
                // writes during rendering increment both coarse-X and
                // Y scroll instead of the normal `inc` value.
                let addr = self.v & 0x3FFF;
                if addr >= 0x3F00 {
                    self.write_palette(addr, value);
                } else {
                    self.write_vram(bus, addr, value);
                }
                if self.mask.rendering_enabled() && self.is_render_scanline() {
                    self.inc_hori_v();
                    self.inc_vert_v();
                } else {
                    let inc = if self.ctrl.contains(PpuCtrl::VRAM_INCREMENT_32) {
                        32
                    } else {
                        1
                    };
                    self.v = self.v.wrapping_add(inc) & 0x7FFF;
                }
                self.observe_a12(bus);
            }
            _ => unreachable!(),
        }
    }

    /// Address-bus A12 = `v` bit 12 during `$0000-$3FFF` accesses. Notify the
    /// mapper on every transition.  Also called by `observe_a12_addr` for
    /// the actual pattern fetch addresses (background and sprite fetches
    /// directly read CHR via the address bus, not via `v`).
    fn observe_a12<B: PpuBus>(&mut self, bus: &mut B) {
        let level = (self.v & 0x1000) != 0;
        if level != self.last_a12_level {
            bus.notify_a12(level);
            self.last_a12_level = level;
        }
    }

    /// Notify the mapper of an A12 transition implied by an explicit
    /// pattern-table fetch address (BG / sprite fetches that bypass `v`).
    fn observe_a12_addr<B: PpuBus>(&mut self, bus: &mut B, addr: u16) {
        let level = (addr & 0x1000) != 0;
        if level != self.last_a12_level {
            bus.notify_a12(level);
            self.last_a12_level = level;
        }
    }

    /// Read from PPU memory `$0000-$3EFF` honoring CIRAM ownership: CHR
    /// (`$0000-$1FFF`) goes to the bus/mapper; nametable (`$2000-$3EFF`)
    /// reads come from the PPU-owned CIRAM through the mapper-supplied
    /// mirroring map.
    ///
    /// The bus is consulted via `peek_nametable` first; mappers like MMC5
    /// in fill mode or ExRAM-as-nametable mode synthesize the byte
    /// directly. Only when the bus declines (`None`) do we hit CIRAM.
    // `&mut self` is required under `mc-ppu-2007-render-buffer` (it latches
    // `self.render_data_bus` below); clippy's needless-pass-by-ref-mut only fires
    // on the default build where that cfg is off, so allow it here.
    #[allow(clippy::needless_pass_by_ref_mut)]
    fn read_vram<B: PpuBus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        let a = addr & 0x3FFF;
        let val = if a < 0x2000 {
            bus.ppu_read(a)
        } else {
            // Mirror $3000-$3EFF to $2000-$2EFF.
            let nt_addr = if a >= 0x3000 { a - 0x1000 } else { a };
            if let Some(v) = bus.peek_nametable(nt_addr) {
                v
            } else {
                let off = bus.nametable_address(nt_addr) as usize;
                self.ciram[off & 0x07FF]
            }
        };
        // The VRAM data bus latches every read (the rendering fetches drive it);
        // a `$2007` read during rendering returns this, not a read at `v`.
        {
            self.render_data_bus = val;
        }
        // v2.0.3 (ADR 0030) — TriCNES `FetchPPU`: after the read the multiplexed
        // bus's low 8 bits hold the DATA (AD7-0). Crucially, `octal_latch` is NOT
        // refreshed here — it retains the ADDRESS low it was loaded with at ALE, so
        // a following `$2007`-read ALE-overlap freezes it on this stale data byte
        // (the "ALE + Read" corruption; the latch is managed by `ale_splice` /
        // `drive_bus`).
        val
    }

    /// W2 ($2007 Stress) — per-dot sprite-tile fetch read cadence (dots
    /// 257-320), feeding `render_data_bus` for the deferred `$2007` `PPUDATA`
    /// buffer reload. Per `AccuracyCoin` `$2007 Stress` (and `TriCNES`'s per-dot
    /// PPU), each 8-dot sprite slot does TWO nametable reads in a row (not
    /// NT+AT) then the sprite PT-lo / PT-hi. Both garbage NT reads use the
    /// (horizontally-reset) `v` address — the sprite-fetch interval does no
    /// coarse-X increment, so it is constant across all 8 slots. Reads land
    /// on the slot-local odd dots (1,3,5,7). The PT bytes come from the raw
    /// stash captured by `fetch_sprite_tile` (no fresh CHR read, so no new
    /// A12/mapper events); the NT reads go through `read_vram`, which latches
    /// `render_data_bus` itself.
    fn tick_sprite_fetch_read<B: PpuBus>(&mut self, bus: &mut B) {
        let local = (self.dot - 257) % 8;
        let slot = ((self.dot - 257) / 8) as usize;
        match local {
            1 | 3 => {
                // Slot 0's FIRST garbage NT read straddles the dot-257
                // copy-hori boundary: its ALE (dot 257) latched the OLD `v`
                // address (`ppudata_spr0_nt_addr`). Every later garbage NT
                // read uses the (horizontally reset) live `v`.
                let nt = if local == 1 && slot == 0 {
                    self.ppudata_spr0_nt_addr
                } else {
                    0x2000 | (self.v & 0x0FFF)
                };
                // `read_vram` latches `render_data_bus` with the value read.
                let _ = self.read_vram(bus, nt);
            }
            5 if slot < 8 => self.render_data_bus = self.spr_fetch_lo_raw[slot],
            7 if slot < 8 => self.render_data_bus = self.spr_fetch_hi_raw[slot],
            _ => {}
        }
    }

    /// Write to PPU memory `$0000-$3EFF`. Mirrors [`Self::read_vram`].
    fn write_vram<B: PpuBus>(&mut self, bus: &mut B, addr: u16, value: u8) {
        let a = addr & 0x3FFF;
        if a < 0x2000 {
            bus.ppu_write(a, value);
        } else {
            let nt_addr = if a >= 0x3000 { a - 0x1000 } else { a };
            // Give the mapper a chance to absorb the write (ExRAM
            // nametables, fill-mode drops, etc.). If declined, write CIRAM.
            if !bus.write_nametable(nt_addr, value) {
                let off = bus.nametable_address(nt_addr) as usize;
                self.ciram[off & 0x07FF] = value;
            }
        }
    }

    /// Read palette RAM. Mirrors:
    ///   $3F10/$14/$18/$1C → $3F00/$04/$08/$0C
    ///   anything past $3F1F mirrors back into the 32-byte window.
    const fn read_palette(&self, addr: u16) -> u8 {
        let idx = palette_index(addr);
        // Apply the greyscale mask if PPUMASK bit 0 is set.
        let raw = self.palette_ram[idx];
        if self.mask.contains(PpuMask::GREYSCALE) {
            raw & 0x30
        } else {
            raw
        }
    }

    const fn write_palette(&mut self, addr: u16, value: u8) {
        let idx = palette_index(addr);
        // Palette is 6-bit storage.
        self.palette_ram[idx] = value & 0x3F;
    }

    /// Tick exactly one dot.
    #[allow(clippy::too_many_lines)] // the per-dot FSM + the ppu-oam-data-bus tick hook
    #[allow(clippy::cognitive_complexity)] // + the ppu-sprite-shifter-counter render-toggle branches
    pub fn tick<B: PpuBus>(&mut self, bus: &mut B) {
        // Advance the dot/scanline FSM first, then handle per-dot events at
        // the post-advance position.
        self.advance_dot();

        // v2.0.3 (ADR 0030, Option 1) — the delayed-`CopyV` landing (`TriCNES`
        // `Emulator.cs:1684-1704`). Ticked at the TOP of the dot, BEFORE the fetch
        // dispatch, so the `address_bus = v` splice is in place for THIS dot's
        // nametable read (the corrupted "Hybrid Addresses" fetch). The phase-0 NT
        // ALE of the corrupt group ran on the PREVIOUS dot and already loaded
        // `octal_latch` with the one-tile-ahead NT-low, so the armed splice at the
        // read below yields `(v & 0x3F00) | octal_latch = $2F19` naturally.
        if self.copy_v_delay > 0 {
            self.copy_v_delay -= 1;
            if self.copy_v_delay == 0 {
                self.v = self.t;
                self.address_bus = self.v;
                // Preserve the `$2006`-write A12 edge (MMC3 timing). It is delayed
                // by the countdown vs the flag-off immediate copy, but `$2006`
                // writes during active render are rare and A12 during render is
                // dominated by the dot-260 sprite fetch, so this stays inside the
                // fetch-address-derived-timing budget (verified by the battery).
                self.observe_a12(bus);
            }
        }

        // v2.0 Phase 6 (mc-ppu-subpos): track the BG-reload gate. It follows the
        // live `self.mask` rendering bit EXCEPT during the analog `$2001`
        // write-delay window, where it stays frozen at the prior value (TriCNES
        // `PPU_Update2001Delay` -> `PPU_Mask_Show*_Delayed`). Re-syncing to the
        // live mask when settled keeps it consistent under a direct mask set
        // (unit tests / save-state restore). Done at the top of the dot, before
        // the reload runs; the live `self.mask` is unaffected.
        if self.mask_write_delay > 0 {
            self.mask_write_delay -= 1;
        } else {
            self.bg_reload_render = self.mask.rendering_enabled();
        }

        // v1.4.0 Workstream F (F1): the scanline-classification flags are pure
        // functions of `self.scanline` + `self.region`, so recompute them only
        // when the scanline changes; every other dot reads the cached copies.
        // Byte-identical (same values), self-healing on reset / restore.
        if self.scanline != self.flags_cached_scanline {
            self.cached_visible =
                self.scanline >= 0 && self.scanline <= self.region.last_visible_line();
            self.cached_pre_render = self.scanline == self.region.prerender_line();
            self.cached_render_line = self.cached_visible || self.cached_pre_render;
            self.flags_cached_scanline = self.scanline;
        }
        let visible = self.cached_visible;
        let pre_render = self.cached_pre_render;
        let render_line = self.cached_render_line;
        let rendering = self.mask.rendering_enabled();
        // v2.0 (ae30785): the fetch/shift/sprite-eval pipeline gates on the
        // 1-PPU-dot-delayed rendering value under `ppu-sprite-shifter-counter`
        // (a mid-scanline `$2001` toggle takes effect one dot later — Stale
        // BG/Sprite). Default build = the immediate value (byte-identical).
        let rendering_gate = self.rendering_enabled_delayed;

        // OAM corruption (TriCNES eval-pointer model). The disable edge
        // itself is armed by the `$2001` write (see the PPUMASK handler);
        // the index is captured against the live secondary-OAM eval
        // pointer during the dots 1-64 window (`capture_oam_corruption`,
        // called from the sprite-eval FSM); and the actual corruption is
        // committed when rendering RE-ENABLES on a render line, or at the
        // pre-render line. Here we only retain the unrelated BG-shifter
        // fix-up that the prior model happened to share the 1->0 edge with.
        if render_line && rendering != self.prev_rendering_enabled && !rendering {
            // v2.0 (ppu-sprite-shifter-counter): if rendering is disabled
            // mid-pre-fetch (dots 329-336, the SECOND fetch group after the
            // dot-329 reload), the in-progress group's pending `<<= 8` would be
            // skipped by the now-gated pipeline, freezing the just-reloaded tile
            // in the BG shifter's bits 0-7. Complete it ONCE here (one-time on
            // the 1->0 edge) so the tile lands in bits 8-15 and surfaces at the
            // correct pixel on re-enable (Stale Sprite Shift Regs t5/6).
            if (329..=336).contains(&self.dot) {
                self.prefetch_shift_bg_regs();
            }
        }
        // Commit pending OAM corruption at the START of the pre-render line
        // (TriCNES handles this via the dots 1-64 eval path on the
        // pre-render line; the dot-0 hook covers the case where rendering
        // was re-enabled during VBlank and stays on into pre-render).
        // Per TriCNES `CorruptOAM`, the corruption applies on the first
        // rendered dot once rendering is (re-)enabled.
        if self.scanline == self.region.prerender_line()
            && self.dot == 0
            && rendering
            && self.oam_corruption_pending
        {
            self.process_oam_corruption();
        }
        // OAM corruption (TriCNES eval-pointer model): maintain the
        // `OAM2Address` analogue across the dots 1-64 clear window, capture
        // the corruption index at the disable edge, and commit on re-enable.
        // Driven every render-line dot independent of the rendering gate so
        // the disable edge is observed even though the sprite-eval FSM below
        // stops once rendering is gated off.
        if render_line {
            self.tick_oam_corruption(rendering);
        }
        self.prev_rendering_enabled = rendering;
        // v2.0 (ae30785): update the 1-dot-delayed copy AFTER this dot's gate
        // read above, so the next dot sees the delayed value.
        {
            self.rendering_enabled_delayed = rendering;
        }

        // === Dot-1 / dot-0 events ===
        // VBL flag is set at scanline 241 dot 1 per nesdev wiki.  The
        // PPU's /NMI line is pulled low one PPU clock later (dot 2),
        // matching the behavior blargg's `ppu_vbl_nmi/05-nmi_timing` and
        // `08-nmi_off_timing` were calibrated to: a /NMI assertion-edge
        // sample ~6-7 PPU clocks after VBL set, given how our bus
        // interleaves CPU bus accesses before the 3-PPU-tick `on_cpu_cycle`
        // hook (vs. real hardware's mid-cycle phi1 access).
        if self.scanline == self.region.vblank_start_line()
            && self.dot == 1
            && !self.suppress_vbl_this_frame
        {
            self.status.insert(PpuStatus::VBLANK);
            // Inform the mapper that we have entered VBL — MMC5 uses this
            // to clear its in-frame flag.
            bus.notify_vblank();
            // R2 (mc-r1-substrate): /NMI is asserted on the SAME dot as
            // VBL-set when NMI_ENABLE is set (Mesen2 `NesPpu.cpp:1339-1343`).
            // On R1's on-time access the CPU read at dot 1 lands AFTER
            // VBL+NMI set, so the v1.x `dot==3` lag-comp band-aid (below) is
            // obsolete and disabled.
            if self.ctrl.contains(PpuCtrl::NMI_ENABLE) {
                self.nmi_line = true;
            }
        }
        // v1.x default path: /NMI raised at dot 3 to compensate for the
        // lockstep PPU running ~8 mc late. Disabled under the on-time R1
        // substrate (R2 raises /NMI at dot 1, above).
        if pre_render && self.dot == 1 {
            self.status.remove(
                PpuStatus::VBLANK
                    .union(PpuStatus::SPRITE_ZERO_HIT)
                    .union(PpuStatus::SPRITE_OVERFLOW),
            );
            self.nmi_line = false;
            self.suppress_vbl_this_frame = false;
        }

        // Notify the mapper that a rendered scanline has started. We fire
        // this on dot 0 of every visible line and the pre-render line,
        // before any pattern/attribute fetches happen. MMC5 uses this to
        // tick its scanline IRQ counter (which conceptually fires at PPU
        // cycle ~4 of each rendered line — close enough for v0). Other
        // mappers default to no-op.
        if render_line && self.dot == 0 {
            bus.notify_scanline_start();
        }

        // === Background rendering pipeline (visible + pre-render lines) ===
        if render_line && rendering_gate {
            // Sprite evaluation: per-PPU-dot FSM matching real-hardware
            // behavior (cycles 1-64 secondary-OAM clear, 65-256 alternating
            // odd/even read/write with the documented buggy `n+m`
            // overflow-detection increment).  Visible scanlines evaluate
            // for the next visible scanline; the pre-render line evaluates
            // for scanline 0.  Without the pre-render eval, secondary OAM
            // from the last visible scanline would leak into pre-render's
            // dummy sprite tile fetches, causing wrong A12 emissions and
            // incorrect sprite-zero state.
            if visible || pre_render {
                self.tick_sprite_eval_per_dot();
            }
            // v2.0 Tier 1.2: drive the isolated OAM-data-bus model on visible
            // scanlines when rendering, so a CPU $2004 read mid-frame observes
            // the sprite-eval / load data bus (AccuracyCoin `$2004 Stress`).
            // Side-effect-free w.r.t. the rendering FSM above.
            if visible && self.mask.rendering_enabled() {
                self.tick_oam_bus();
            }
            // Sprite tile fetch + A12 emission.  Real hardware spreads the
            // 8 sprite slots' pattern fetches across cycles 257..=320 — for
            // each slot, garbage NT bytes at +1/+3, sprite pattern lo at
            // +5/+6, sprite pattern hi at +7/+8.  We collapse that to per-
            // slot emission at dot 260 for slot 0, 268 for slot 1, …, 316
            // for slot 7.  This is the canonical "MMC3 IRQ at PPU dot 260"
            // timing — the first A12 rise to the sprite pattern table
            // happens here for standard pattern-table layout (BG=$0000,
            // sprites=$1000), per `docs/mappers.md` §MMC3 → IRQ counter
            // mechanism.
            //
            // CRITICAL for MMC3: even unused sprite slots ALWAYS perform
            // the dummy sprite-pattern fetch on real hardware (using the
            // cleared secondary-OAM tile $FF), so A12 toggles into the
            // sprite pattern table once per scanline regardless of how
            // many real sprites are visible.  This must run on both
            // visible scanlines and the pre-render line — pre-render
            // sprite fetches are for scanline 0's sprites and contribute
            // the 241st A12 rising edge per frame (240 visible + 1
            // pre-render) that MMC3's IRQ counter expects.
            if (260..=316).contains(&self.dot) {
                let phase = self.dot.wrapping_sub(260);
                if phase.trailing_zeros() >= 3 {
                    let slot = (phase >> 3) as usize;
                    self.fetch_sprite_tile(bus, slot);
                }
            }

            // OAMADDR reset: per nesdev wiki "PPU registers" §OAMADDR,
            // "OAMADDR is set to 0 during each of ticks 257-320 (the
            // sprite tile loading interval) of the pre-render and visible
            // scanlines." This is the hardware behaviour that lets games
            // STX $4014 their OAM-staging page after rendering without
            // having to remember to STA $2003 #0 first. Required for
            // AccuracyCoin TEST_Sprite0Hit_Behavior subtest 1 (which
            // relies on the prior test-runner's OAMADDR perturbation
            // being washed away by the previous frame's rendering).
            if (257..=320).contains(&self.dot) {
                self.oam_addr = 0;
            }

            // v2.0 (ppu-sprite-shifter-counter): at dot 339 of a render line,
            // re-arm the LOADED sprite counters (the `spr_count` slots fetched
            // for the next scanline) to "counting". This whole block is gated on
            // `rendering_gate`, so a render-disable across dot 339 leaves the
            // loaded slots halted — a reloaded-but-halted counter draws
            // immediately on re-enable (Stale Sprite Shift Regs t5/6). Slots
            // beyond `spr_count` retain their halted latch.
            if self.dot == 339 {
                for i in 0..self.spr_count as usize {
                    self.spr_halted[i] = false;
                }
            }

            // BG fetches happen at dots 1..=256 and 321..=336.
            //
            // CYCLE-PRECISE BG PIPELINE (Mesen2-faithful, fixes Cascade A
            // VerifySpriteZeroHits step-2 off-by-one):
            //
            // Per nesdev wiki "PPU rendering": "The shifters are reloaded
            // during ticks 9, 17, 25, ..., 257." Per Mesen2
            // `Core/NES/NesPpu.cpp::LoadTileInfo()` (line 667), the reload
            // is `case 1` of `(_cycle & 0x07)` — i.e., phase 0 of each
            // 8-cycle group, OR'ing the latched LowByte/HighByte into the
            // shifter's low 8 bits. The PRIOR group's 8 shifts (one per
            // cycle of dots 1..=256 of a visible scanline) leave bits 0-7
            // zeroed, so the OR is effectively an overwrite. The pre-fetch
            // groups at dots 321..=336 do NOT shift per-cycle; instead
            // Mesen2 substitutes a `<<= 8` at phase 7 (dots 328 and 336)
            // to clear bits 0-7 for the next reload.
            //
            // The matching pixel-emit + shift ordering is: emit_pixel reads
            // bit (15 - fine_x) FIRST, then shift_bg runs LAST. This is the
            // critical change from the prior (off-by-one) implementation
            // that shifted BEFORE emit and reloaded at phase 7 (cycle 8).
            // See `docs/audit/cascade-a-investigation-2026-05-19.md` for
            // the empirical analysis and the per-cycle trace of
            // VerifySpriteZeroHits step 2 demonstrating why this is the
            // load-bearing change.
            let in_bg_fetch = (1..=256).contains(&self.dot) || (321..=336).contains(&self.dot);
            if in_bg_fetch {
                let phase = (self.dot.wrapping_sub(1)) & 7;
                // Phase 0 (cycles 1, 9, 17, ..., 249, 321, 329): reload the
                // shifter's low 8 bits from the latches written by the
                // PRIOR fetch group. Implementation note: `reload_bg_shift_regs`
                // overwrites bits 0-7 via `(shift & 0xFF00) | latch`; this
                // matches Mesen2's `|=` because the 8 shifts since the prior
                // reload guarantee bits 0-7 are zero before the OR.
                if phase == 0 {
                    // v2.0 Phase 6 (mc-ppu-subpos): the reload is additionally
                    // gated on the analog-delayed `$2001` value, so a render
                    // re-enable lands the reload `MASK_WRITE_DELAY` dots after
                    // the shifter has already resumed advancing -> one reload is
                    // SKIPPED and the serial-in '1's survive (BG Serial In).
                    // When stable, `bg_reload_render` == the live rendering gate,
                    // so this is byte-identical.
                    let reload_gate = self.bg_reload_render;
                    if reload_gate {
                        self.reload_bg_shift_regs();
                    }
                }

                // v2.0.3 (ADR 0030, Option 1) — the ALE (address-latch-enable)
                // half of each 2-cycle VRAM access. On the EVEN dot of each pair
                // (phases 0/2/4/6, i.e. one dot BEFORE the corresponding read at
                // phases 1/3/5/7) the PPU drives the full 14-bit fetch address onto
                // `address_bus` and captures its low byte into `octal_latch`; the
                // read half (the existing `fetch_*` below) reads via the splice
                // `(address_bus & 0x3F00) | octal_latch`. For a coherent fetch the
                // splice returns the intended address, so this is behavior-neutral;
                // the split exists so a later phase's `$2006`/`$2007` corruption can
                // desync the two halves naturally.
                match phase {
                    // Phase 0 (NT ALE): drive the PLAIN nametable address
                    // (`0x2000 | (v & 0x0FFF)`) and load `octal_latch` with its low
                    // byte. This is a TRUE two-dot ALE for the common (non-MMC5-
                    // split) case, so the latch naturally carries the NT-low the
                    // "Hybrid Addresses" corruption needs. The MMC5 vertical-split
                    // query stays at the read dot (phase 1, `fetch_nt`) because its
                    // `split_chr_bank_latch` side effect is mapper-observable; when
                    // that query turns out to be split-active, `fetch_nt` disarms
                    // this ALE and reads `split.nt_addr` co-located instead, so
                    // split rendering is byte-identical (no phase-0 mapper query).
                    0 => self.ale_drive_nt(),
                    2 => self.ale_drive_at(),
                    4 => self.ale_drive_bg_lo(),
                    6 => self.ale_drive_bg_hi(),
                    _ => {}
                }

                // 8-cycle fetch group: dot phase = (dot - 1) & 7
                //   1 -> NT byte fetch     (cycle 2 of group)
                //   3 -> AT byte fetch     (cycle 4 of group)
                //   5 -> BG-low fetch      (cycle 6 of group)
                //   7 -> BG-high fetch +
                //        coarse-X increment (cycle 8 of group)
                match phase {
                    1 => self.fetch_nt(bus),
                    3 => self.fetch_at(bus),
                    5 => self.fetch_bg_lo(bus),
                    7 => self.fetch_bg_hi(bus),
                    _ => {}
                }
                if phase == 7 {
                    self.inc_hori_v();
                    // Pre-fetch region only (dots 328 and 336): explicit
                    // `<<= 8` to substitute for the missing per-cycle
                    // shifts during pre-fetch. Per Mesen2
                    // `ProcessScanlineImpl()` lines 941-944.
                    if (321..=336).contains(&self.dot) {
                        self.prefetch_shift_bg_regs();
                    }
                }
            }
            // Dot 257: the LAST shift-register reload of the visible region
            // consumes the latches from the dots-249..=256 fetch group. Dot
            // 257 is outside the dots 1..=256 `in_bg_fetch` range above (it
            // belongs to the sprite-tile-fetch window 257..=320), but per
            // Mesen2's `_cycle <= 256` LoadTileInfo cycle range, this reload
            // actually never fires in Mesen2 either — the dot-256 fetch's
            // bg_lo/bg_hi latches are consumed by the dot-321 reload (which
            // OR's them in, then the dot-328 `<<= 8` shifts them up to
            // bits 8-15). So for our model, the dot-256 fetch's latches
            // similarly persist past dot 256 into dot 321's reload.
            // (Intentionally no dot-257 reload here.)

            // Cycle 256: vertical-V increment.
            if self.dot == 256 {
                self.inc_vert_v();
            }
            // Cycle 257: copy hori(t) -> hori(v).
            if self.dot == 257 {
                // W2 ($2007 Stress): the FIRST garbage NT read of sprite slot
                // 0 has its ALE at dot 257, BEFORE hori(v) is reset — so its
                // ADDRESS uses the OLD `v` (coarse-x wrapped past the row's
                // last column) even though the data lands at dot 258. Latch
                // the address here; `tick_sprite_fetch_read` uses it for the
                // slot-0 first read (key idx 128 = `02`). The later garbage
                // NT reads (ALE dots 259+) all use the reset `v`.
                {
                    self.ppudata_spr0_nt_addr = 0x2000 | (self.v & 0x0FFF);
                }
                self.copy_hori_t_to_v();
            }
            // Pre-render cycles 280..=304: copy vert(t) -> vert(v).
            if pre_render && (280..=304).contains(&self.dot) {
                self.copy_vert_t_to_v();
            }
            // Sprite tile fetch happens in fetch_sprite_tile (dots 260, 268, ..., 316).
            // Cycles 337..=340: 2 garbage NT fetches (no-op except A12).
            if (337..=340).contains(&self.dot) && (self.dot & 1) == 1 {
                self.fetch_nt(bus);
            }

            // W2 ($2007 Stress): the per-dot sprite-fetch read cadence (dots
            // 257-320) feeds `render_data_bus` (NT,NT,PT-lo,PT-hi per 8-dot
            // slot) so a deferred `$2007` buffer reload landing in HBlank
            // captures the byte the sprite fetch drove on the VRAM bus. The
            // real fetch is the collapsed `fetch_sprite_tile` above; this is
            // side-effect-free w.r.t. rendering and A12 (the garbage NT reads
            // are CIRAM/nametable reads; the PT bytes come from the stash).
            if (257..=320).contains(&self.dot) {
                self.tick_sprite_fetch_read(bus);
            }
        }

        // W2 ($2007 Stress): the PPUDATA state machine's read step (PD_RB) +
        // TStep. A `$2007` read during rendering armed this countdown; one
        // tick per PPU dot (unconditionally, so a mid-flight rendering
        // disable cannot wedge it), and at 0:
        //   1. `data_buffer` latches the value the FETCH cadence drove on the
        //      VRAM bus at THIS dot (`render_data_bus`, freshly set by the
        //      fetch dispatch above). Latched bus value only — never a fresh
        //      VRAM read (zero new A12/mapper events).
        //   2. The deferred v-glitch increment (the TStep) fires AFTER the
        //      reload, per TriCNES `PPU_DATA_StateMachine_Half` — so every
        //      fetch in the read-to-reload window used the OLD `v`. The
        //      rendering-vs-blanking choice uses the state AT the TStep dot
        //      (mirrors TriCNES's `PPU_2007_BLNK_Latch`).
        if self.ppudata_sm_countdown > 0 {
            self.ppudata_sm_countdown -= 1;
            if self.ppudata_sm_countdown == 0 {
                self.data_buffer = self.render_data_bus;
                // v2.0.2 (ADR 0030) — "ALE + Read": the $2007 read's PPUDATA
                // state machine takes 3 PPU cycles to the background cadence's 2,
                // so on the reload dot its ALE overlaps a fetch's read. Both ALE
                // and READ are asserted, so the octal latch is frozen on the
                // read's DATA byte (`render_data_bus`) instead of the next
                // Pattern-Address-Register low byte. The next pattern fetch then
                // reads `{PAR high 6}:{stale data low}` (`$0F03` -> `$0FFF`).
                if self.mask.rendering_enabled() && self.is_render_scanline() {
                    // Freeze the latch on the read's DATA byte here. The frozen byte
                    // is then carried by `drive_bus` (which suppresses the next
                    // pattern ALE's latch reload) and consumed by the pattern read's
                    // natural `ale_splice`, so the next pattern fetch reads
                    // `(PAR high 6):(stale $FF) = $0FFF` with no explicit splice.
                    self.octal_latch = self.render_data_bus;
                    self.pattern_latch_stale = true;
                    octal_trace::push(
                        octal_trace::K_SMLAND,
                        self.frame,
                        self.scanline,
                        self.dot,
                        u32::from(self.render_data_bus),
                    );
                }
                if self.ppudata_v_inc_pending {
                    self.ppudata_v_inc_pending = false;
                    if self.mask.rendering_enabled() && self.is_render_scanline() {
                        self.inc_hori_v();
                        self.inc_vert_v();
                    } else {
                        let inc = if self.ctrl.contains(PpuCtrl::VRAM_INCREMENT_32) {
                            32
                        } else {
                            1
                        };
                        self.v = self.v.wrapping_add(inc) & 0x7FFF;
                    }
                    // The v change can move A12 (mirrors the read-time path).
                    self.observe_a12(bus);
                }
            }
        }

        // === Pixel emission (visible scanlines, dots 1..=256) ===
        // Per Mesen2 `ProcessScanlineImpl()` (lines 881-884), the
        // canonical order is: LoadTileInfo (reload at phase 0, fetches at
        // phases 1/3/5/7) THEN DrawPixel THEN ShiftTileRegisters. The
        // shift-AFTER-emit ordering is the load-bearing other half of the
        // Cascade A BG-pipeline fix: emit reads bit (15 - fine_x) of the
        // shifter at its CURRENT state (post-reload, pre-shift), then the
        // shift advances the register for the next emit. Combined with
        // the phase-0 reload above, this places the newly-fetched tile's
        // MSB at shift-register bit 15 (the emit read point) at exactly
        // PPU dot 9 of each 8-cycle group = pixel column 8.
        if visible && (1..=256).contains(&self.dot) {
            self.emit_pixel();
            // v2.0 (ae30785): the BG shifter advances on the 1-dot-delayed
            // rendering gate (under the feature), so a precisely-timed `$2001`
            // toggle that skips the reload while rendering stays enabled
            // surfaces the serial-in (BG Serial In / Stale BG Shift).
            //
            // v2.0 Phase 6 (mc-ppu-subpos): TriCNES drives the SHIFT off the
            // IMMEDIATE PPUMASK (`_EmulateHalfPPU` -> `PPU_UpdateBackground
            // ShiftRegisters`, gated on `PPU_Mask_Show*` not `*_Delayed`) while
            // the fetch+RELOAD ride the 1-dot-DELAYED mask
            // (`PPU_Render_ShiftRegistersAndBitPlanes`, gated on `*_Delayed`).
            // So on a render re-enable edge there is a 1-dot window where the
            // shifter advances (injecting the serial-in '1') but the reload is
            // still gated off -> a single reload is SKIPPED while shifting
            // continues, surfacing the accumulated serial-in '1's at the output
            // (AccuracyCoin "BG Serial In"). When no mid-scanline toggle is in
            // flight immediate == delayed, so normal rendering is byte-identical.
            let shift_gate = render_line && rendering;
            if shift_gate {
                self.shift_bg();
            }
        }

        // === Per-PPU-dot state-trace recording (Session-10) ===
        //
        // Gated on the `ppu-state-trace` cargo feature so the
        // default build's hot tick path is byte-identical to
        // pre-Session-10. The hook reads `self`'s state AFTER
        // all this dot's effects have applied, so the captured
        // record reflects "PPU state at the end of dot
        // (scanline, dot)". It NEVER writes to PPU state — the
        // determinism contract is preserved.
        //
        // See `docs/adr/0005-ppu-state-trace.md`.
        #[cfg(feature = "ppu-state-trace")]
        if self.state_trace.is_some() {
            let rec = self.build_state_record();
            if let Some(t) = self.state_trace.as_mut() {
                t.maybe_push(rec);
            }
        }
    }

    // ------------------------------------------------------------------
    // Background fetch + shift + increment helpers.
    // ------------------------------------------------------------------

    /// Fetch the nametable byte for the current `v`. Address: `$2000 |
    /// (v & 0x0FFF)`.
    ///
    /// MMC5 vertical split-screen: at the boundary of each 8-dot BG fetch
    /// group, the mapper is consulted via `bus.bg_split_state(...)`. If
    /// the current tile column falls within the alt region, the returned
    /// state supplies the synthesized NT / AT addresses, the alt fine-Y,
    /// and the 4 KiB CHR bank index. We latch it onto `bg_split_latch` for
    /// consumption by AT / BG-lo / BG-hi within the same fetch group.
    #[allow(clippy::cast_sign_loss)]
    #[inline]
    fn fetch_nt<B: PpuBus>(&mut self, bus: &mut B) {
        // Compute the (scanline_y, coarse_x) the alt region would be sampled
        // at. The pre-render line passes 0 (the alt region only renders on
        // visible lines, but the query is benign for pre-render).
        let scanline_y = if self.scanline < 0 {
            0
        } else {
            self.scanline as u16
        };
        let coarse_x = self.v & 0x001F;
        // NOTE (v2.0.3 / ADR 0030, Option 1): the MMC5 vertical-split query stays
        // HERE at the read dot — its `split_chr_bank_latch` side effect (which
        // `nametable_fetch`/`chr_offset` read) is mapper-observable, and moving it
        // one dot earlier to a phase-0 ALE shifts the Uchuu Keibitai SDF split
        // rendering. Consequently the NT fetch's octal-latch load co-locates with
        // its read (via `ale_splice`'s not-armed path below) rather than a phase-0
        // ALE; the AT / pattern fetches ARE true two-dot ALEs. See the plan.
        self.bg_split_latch = bus.bg_split_state(scanline_y, coarse_x);

        let nt_addr = if let Some(split) = self.bg_split_latch {
            split.nt_addr
        } else {
            0x2000 | (self.v & 0x0FFF)
        };
        // v2.0.3 (ADR 0030, Option 1) — 2-cycle-ALE read half. For the common
        // (non-split) case the phase-0 NT ALE already drove the plain address and
        // loaded `octal_latch`, so `ale_splice` takes its armed path and the read
        // address is the true multiplexed splice `(address_bus & 0x3F00) |
        // octal_latch` — transparent for a coherent fetch, and the divergence
        // point for the delayed-`CopyV` "Hybrid Addresses" corruption. For an
        // MMC5-split fetch the phase-0 ALE drove the PLAIN address (the split
        // query lives HERE for its mapper-observable side effect), so disarm and
        // read `split.nt_addr` co-located instead — byte-identical to a coherent
        // fetch.
        let nt_addr = {
            if self.bg_split_latch.is_some() {
                self.ale_armed = false;
            }
            self.ale_splice(nt_addr)
        };
        self.nt_latch = self.read_vram(bus, nt_addr);
        // Data phase: drive the byte just read back onto the multiplexed bus's low
        // 8 bits (AD7-0). Behavior-neutral (the next fetch's ALE overwrites it).
        self.ale_drive_data(self.nt_latch);
        // Latch any per-tile extended-attribute info (MMC5 ExGrafix). Skip
        // when split is active: the alt region uses standard 4-bit AT
        // semantics, not ExGrafix.
        self.ex_attr_latch = if self.bg_split_latch.is_some() {
            None
        } else {
            bus.peek_ex_attribute(self.v)
        };
    }

    /// Fetch the attribute byte for the current `v`. Address:
    /// `$23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07)`.
    #[inline]
    fn fetch_at<B: PpuBus>(&mut self, bus: &mut B) {
        // Split active: use the alt AT address and recover coarse-X / coarse-Y
        // from the latched split state's NT address (where coarse-X = bits
        // 0..=4, coarse-Y = bits 5..=9).
        if let Some(split) = self.bg_split_latch {
            let at_addr = split.at_addr;
            // v2.0.3 (ADR 0030, Option 1) — 2-cycle-ALE read half (split AT path):
            // splice / consume the ALE arm so it can't leak to the next fetch.
            let at_addr = self.ale_splice(at_addr);
            let byte = self.read_vram(bus, at_addr);
            self.ale_drive_data(byte);
            let coarse_x = (split.nt_addr & 0x001F) as u8;
            let coarse_y = ((split.nt_addr >> 5) & 0x001F) as u8;
            let shift = ((coarse_y & 0x02) << 1) | (coarse_x & 0x02);
            self.at_latch = (byte >> shift) & 0x03;
            return;
        }
        let v = self.v;
        let at_addr = 0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07);
        // v2.0.3 (ADR 0030, Option 1) — 2-cycle-ALE read half (normal AT path).
        let at_addr = self.ale_splice(at_addr);
        let byte = self.read_vram(bus, at_addr);
        self.ale_drive_data(byte);
        // Pick the 2-bit attribute based on coarse-X[1] and coarse-Y[1].
        let coarse_x = (v & 0x1F) as u8;
        let coarse_y = ((v >> 5) & 0x1F) as u8;
        let shift = ((coarse_y & 0x02) << 1) | (coarse_x & 0x02);
        let standard_palette = (byte >> shift) & 0x03;
        // ExGrafix override: replace the 2-bit palette with the per-tile
        // value latched at NT-fetch time.
        self.at_latch = self
            .ex_attr_latch
            .map_or(standard_palette, |ex| ex.palette & 0x03);
    }

    /// Fetch BG pattern low byte for the current `nt_latch` + fine-Y of `v`.
    ///
    /// In MMC5 `ExGrafix` mode the mapper has internally latched a per-tile
    /// 4 KiB CHR bank from the most recent `peek_ex_attribute` call; it
    /// will resolve this `addr` against that bank rather than the standard
    /// BG bank registers. No address-bus rerouting required.
    ///
    /// In MMC5 vertical split-screen mode the mapper has likewise latched
    /// the `$5202` 4 KiB CHR bank from the most recent `bg_split_state`
    /// call, and the alt fine-Y replaces `v`'s fine-Y.
    #[inline]
    fn fetch_bg_lo<B: PpuBus>(&mut self, bus: &mut B) {
        let bg_table = u16::from(self.ctrl.contains(PpuCtrl::BG_PATTERN_HIGH)) << 12;
        let fine_y = self
            .bg_split_latch
            .map_or((self.v >> 12) & 0x07, |s| u16::from(s.fine_y) & 0x07);
        let addr = bg_table | (u16::from(self.nt_latch) << 4) | fine_y;
        self.observe_a12_addr(bus, addr);
        // v2.0.3 (ADR 0030, Option 1) — 2-cycle-ALE read half: A12 above stays on the
        // INTENDED `addr`; only the DATA read address goes through the ALE splice
        // (stale-latch "ALE + Read"). `addr` itself is preserved for the hd-pack
        // tile-base latch below.
        let read_addr = self.ale_splice(addr);
        self.bg_lo_latch = self.read_vram(bus, read_addr);
        self.ale_drive_data(self.bg_lo_latch);
        // v1.2.0 C3 (hd-pack): latch the 16-byte tile base (fine-Y masked off)
        // for this fetch group. Promoted into the `hd_bg_addr_*` queue at the
        // next shifter reload so it tracks the BG pattern shifters tile-for-tile.
        // Output-only; no new VRAM read, no A12.
        #[cfg(feature = "hd-pack")]
        {
            self.hd_bg_addr_latch = addr & 0x1FF0;
            // CHR-ROM absolute tile index (offset/16), or the CHR-RAM sentinel.
            self.hd_bg_idx_latch = bus.chr_phys(addr).map_or(HD_CHR_RAM, |o| o / 16);
        }
    }

    /// Fetch BG pattern high byte (offset +8 from the low fetch).
    #[inline]
    fn fetch_bg_hi<B: PpuBus>(&mut self, bus: &mut B) {
        let bg_table = u16::from(self.ctrl.contains(PpuCtrl::BG_PATTERN_HIGH)) << 12;
        let fine_y = self
            .bg_split_latch
            .map_or((self.v >> 12) & 0x07, |s| u16::from(s.fine_y) & 0x07);
        let addr = bg_table | (u16::from(self.nt_latch) << 4) | 0x08 | fine_y;
        self.observe_a12_addr(bus, addr);
        // v2.0.3 (ADR 0030, Option 1) — 2-cycle-ALE read half (see `fetch_bg_lo`):
        // A12 above stays on the INTENDED `addr`; only the DATA read address goes
        // through the ALE splice (stale-latch "ALE + Read").
        let read_addr = self.ale_splice(addr);
        self.bg_hi_latch = self.read_vram(bus, read_addr);
        self.ale_drive_data(self.bg_hi_latch);
    }

    // === v2.0.3 (ADR 0030, Option 1) — 2-cycle-ALE fetch model ===============
    //
    // A genuine two-dot VRAM transaction. The attribute + pattern fetches' EVEN
    // dot (the ALE half, phases 2/4/6) drives the full 14-bit address onto
    // `address_bus` and captures its low byte into `octal_latch` via
    // [`Self::drive_bus`]; the following ODD dot (the read half, phases 3/5/7 —
    // the existing `fetch_*`) resolves the effective address through
    // [`Self::ale_splice`] and drives the DATA byte back onto the low bus via
    // [`Self::ale_drive_data`]. For a coherent fetch the address the ALE drove
    // equals the address the read would compute (`v` is constant across the 8-dot
    // group's phases 0..=6; the coarse-X increment is at phase 7 AFTER the read),
    // so the splice returns the intended address and this is behavior-neutral.
    //
    // The NAMETABLE fetch is the exception: its MMC5 vertical-split query
    // (`bg_split_state`, whose `split_chr_bank_latch` side effect
    // `nametable_fetch`/`chr_offset` read) is mapper-observable and must fire at
    // the read dot (phase 1), so the NT octal-latch load co-locates with the read
    // via `ale_splice`'s not-armed path (there is no phase-0 NT ALE); the NT ALE
    // (`ale_drive_nt`) still drives the plain address so the latch naturally
    // carries the one-tile-ahead NT-low the "Hybrid Addresses" corruption needs.

    /// ALE half of the nametable fetch (phase 0). Drives the PLAIN nametable
    /// address `0x2000 | (v & 0x0FFF)` and loads the octal latch with its low
    /// byte — a true two-dot ALE for the common (non-split) case, which is what
    /// lets `octal_latch` naturally carry the one-tile-ahead NT-low the "Hybrid
    /// Addresses" corruption needs. The MMC5-split query is deferred to the read
    /// dot (phase 1, `fetch_nt`); when it turns out split-active, `fetch_nt`
    /// disarms this ALE and reads the synthesized `split.nt_addr` co-located.
    const fn ale_drive_nt(&mut self) {
        let nt_addr = 0x2000 | (self.v & 0x0FFF);
        self.drive_bus(nt_addr, false);
    }

    /// ALE half of the attribute fetch (phase 2). Uses the `bg_split_latch`
    /// already set by the nametable read at phase 1.
    const fn ale_drive_at(&mut self) {
        let at_addr = if let Some(split) = self.bg_split_latch {
            split.at_addr
        } else {
            let v = self.v;
            0x23C0 | (v & 0x0C00) | ((v >> 4) & 0x38) | ((v >> 2) & 0x07)
        };
        self.drive_bus(at_addr, false);
    }

    /// ALE half of the BG pattern-low fetch (phase 4). Uses `nt_latch` (set at
    /// the phase-1 nametable read) and `v`'s fine-Y (or the split fine-Y).
    fn ale_drive_bg_lo(&mut self) {
        let bg_table = u16::from(self.ctrl.contains(PpuCtrl::BG_PATTERN_HIGH)) << 12;
        let fine_y = self
            .bg_split_latch
            .map_or((self.v >> 12) & 0x07, |s| u16::from(s.fine_y) & 0x07);
        let addr = bg_table | (u16::from(self.nt_latch) << 4) | fine_y;
        self.drive_bus(addr, true);
    }

    /// ALE half of the BG pattern-high fetch (phase 6). Same as the low plane
    /// with bit 3 set (the +8 byte offset).
    fn ale_drive_bg_hi(&mut self) {
        let bg_table = u16::from(self.ctrl.contains(PpuCtrl::BG_PATTERN_HIGH)) << 12;
        let fine_y = self
            .bg_split_latch
            .map_or((self.v >> 12) & 0x07, |s| u16::from(s.fine_y) & 0x07);
        let addr = bg_table | (u16::from(self.nt_latch) << 4) | 0x08 | fine_y;
        self.drive_bus(addr, true);
    }

    /// Drive a full 14-bit fetch address onto the multiplexed bus (the ALE
    /// half): `address_bus` takes the whole address and the 74LS373 octal latch
    /// captures A7-A0. Arms `ale_armed` for the matching read half.
    ///
    /// `is_pattern` marks the two BG-pattern ALEs (phases 4/6). While the "ALE +
    /// Read" freeze (`pattern_latch_stale`) is pending — a `$2007`-read ALE
    /// overlapped the fetch cadence and froze `octal_latch` on the read's DATA
    /// byte — the latch is NOT reloaded (the frozen DATA byte survives across any
    /// intervening ALE); the first pattern ALE afterwards consumes the flag, so
    /// its read splices `(PAR high 6):(stale DATA low 8)` = `$0FFF`.
    const fn drive_bus(&mut self, addr: u16, is_pattern: bool) {
        self.address_bus = addr;
        if self.pattern_latch_stale {
            if is_pattern {
                self.pattern_latch_stale = false;
            }
        } else {
            self.octal_latch = (addr & 0xFF) as u8;
        }
        self.ale_armed = true;
    }

    /// Resolve a fetch's effective read address through the multiplexed bus (the
    /// read half). When a real ALE preceded this read (`ale_armed`), the address
    /// is the splice of the ALE-driven high 6 bits with the latched low 8:
    /// `(address_bus & 0x3F00) | octal_latch` — transparent for a coherent fetch
    /// (Phase 1), the divergence point for the Phase-3 corruptions. With NO
    /// preceding ALE (the dot-337-340 garbage nametable fetches), drive + latch
    /// `intended` in place so the read stays behavior-neutral.
    #[allow(clippy::missing_const_for_fn)] // u16::from is not yet const-stable
    fn ale_splice(&mut self, intended: u16) -> u16 {
        if self.ale_armed {
            self.ale_armed = false;
            let effective = (self.address_bus & 0x3F00) | u16::from(self.octal_latch);
            // Diagnostic: record any read whose spliced effective address diverges
            // from the intended one (the two corruptions) for the TriCNES per-dot
            // cross-diff. `push` self-filters to scanlines 2-5, so this is cheap.
            if effective != intended {
                octal_trace::push(
                    if intended >= 0x2000 {
                        octal_trace::K_HYBRID
                    } else {
                        octal_trace::K_STALE
                    },
                    self.frame,
                    self.scanline,
                    self.dot,
                    u32::from(effective),
                );
            }
            effective
        } else {
            self.address_bus = intended;
            self.octal_latch = (intended & 0xFF) as u8;
            intended
        }
    }

    /// Data half of a VRAM access: drive the byte just read back onto the
    /// multiplexed bus's low 8 bits (AD7-0). `octal_latch` is NOT refreshed here
    /// (the 74LS373 latch holds the ADDRESS low from the ALE) — that retention is
    /// what a `$2007`-read ALE overlap exploits (the "ALE + Read" corruption). It
    /// is otherwise transparent: the next fetch's ALE overwrites `address_bus`
    /// wholesale.
    #[allow(clippy::missing_const_for_fn)] // u16::from is not yet const-stable
    fn ale_drive_data(&mut self, data: u8) {
        self.address_bus = (self.address_bus & 0xFF00) | u16::from(data);
    }

    /// Shift the BG pattern and attribute shift registers by one bit.
    ///
    /// All four registers are 16-bit and advance in lockstep so the
    /// attribute palette tracks the same tile column as the pattern bits.
    const fn shift_bg(&mut self) {
        self.bg_shift_lo <<= 1;
        self.bg_shift_hi <<= 1;
        self.at_shift_lo <<= 1;
        self.at_shift_hi <<= 1;
        // v2.0 Phase 6 (mc-ppu-subpos): BG-shifter SERIAL-IN. Per nesdev "PPU
        // signals", the bit shifted into the pattern shifters from the right is
        // a constant 0 for the LOW plane and a constant 1 for the HIGH plane.
        // It is normally invisible: the dot%8==1 reload overwrites bits 0-7
        // every 8 shifts, so the injected '1' never reaches the output bits
        // 8-15 before being washed (=> framebuffer byte-identical for normal
        // rendering, oracle-safe). It surfaces ONLY when a precisely-timed
        // `$2001` render-toggle SKIPS a reload while shifting continues, drawing
        // opaque BG pixels on an all-translucent nametable (AccuracyCoin "BG
        // Serial In"). The attribute shifters have no serial-in (the test only
        // needs a non-transparent pattern bit, not a specific palette).
        {
            self.bg_shift_hi |= 1;
        }
    }

    /// Pre-fetch (dots 328 / 336) byte shift: advance all four BG shift
    /// registers by 8 bits in lockstep, moving the just-reloaded tile
    /// data from bits 0-7 to bits 8-15 and clearing bits 0-7 for the next
    /// reload. This substitutes for the per-cycle `shift_bg` that does not
    /// run during the dots 321-336 pre-fetch region. The attribute
    /// registers MUST shift identically to the pattern registers here —
    /// omitting them was the 086ce4d left-edge palette regression.
    #[inline]
    const fn prefetch_shift_bg_regs(&mut self) {
        self.bg_shift_lo <<= 8;
        self.bg_shift_hi <<= 8;
        self.at_shift_lo <<= 8;
        self.at_shift_hi <<= 8;
        // v1.2.0 C3 (hd-pack): the `<<= 8` promotes the low (next) tile into the
        // high (displayed) byte — mirror the address queue. Telemetry only.
        #[cfg(feature = "hd-pack")]
        {
            self.hd_bg_addr_cur = self.hd_bg_addr_next;
            self.hd_bg_idx_cur = self.hd_bg_idx_next;
        }
    }

    /// Reload the low bytes of the BG pattern and attribute shift
    /// registers from the latched fetch bytes.
    ///
    /// The 2-bit attribute is constant across all 8 pixels of a tile, so
    /// each attribute bit is expanded to a full `0xFF`/`0x00` byte into
    /// bits 0-7 — the same low-byte slot the pattern bytes occupy. This
    /// keeps the attribute shifter bit-for-bit aligned with the pattern
    /// shifters through both the per-cycle shifts (dots 1-256) and the
    /// pre-fetch `<<= 8` (dots 328 / 336).
    #[inline]
    const fn reload_bg_shift_regs(&mut self) {
        self.bg_shift_lo = (self.bg_shift_lo & 0xFF00) | self.bg_lo_latch as u16;
        self.bg_shift_hi = (self.bg_shift_hi & 0xFF00) | self.bg_hi_latch as u16;
        let at_lo = if (self.at_latch & 0x01) != 0 {
            0xFF
        } else {
            0x00
        };
        let at_hi = if (self.at_latch & 0x02) != 0 {
            0xFF
        } else {
            0x00
        };
        self.at_shift_lo = (self.at_shift_lo & 0xFF00) | at_lo;
        self.at_shift_hi = (self.at_shift_hi & 0xFF00) | at_hi;
        // v1.2.0 C3 (hd-pack): mirror the pattern reload — the prior `next`
        // tile is now in the high byte (displayed), and the freshly-latched
        // tile fills the low byte. Telemetry only; no state effect.
        #[cfg(feature = "hd-pack")]
        {
            self.hd_bg_addr_cur = self.hd_bg_addr_next;
            self.hd_bg_addr_next = self.hd_bg_addr_latch;
            self.hd_bg_idx_cur = self.hd_bg_idx_next;
            self.hd_bg_idx_next = self.hd_bg_idx_latch;
        }
    }

    /// Increment coarse X with nametable-X wrap.
    ///
    /// Note: this is an internal loopy-register increment.  It does NOT
    /// drive the PPU address bus, so it must not emit A12 transitions —
    /// the address bus stays on the last-fetched address (BG-high) until
    /// the next fetch.  An earlier version of this code called
    /// `observe_a12` here, which spuriously interpreted `v`'s fine-Y bit
    /// 0 as A12 and produced ~16 false A12 rising edges per scanline,
    /// breaking MMC3's IRQ count (which expects exactly 1 rise per
    /// rendered scanline, at PPU dot ~260, with standard pattern-table
    /// layout).
    const fn inc_hori_v(&mut self) {
        if (self.v & 0x001F) == 31 {
            self.v &= !0x001F;
            self.v ^= 0x0400;
        } else {
            self.v += 1;
        }
    }

    /// Increment fine Y, with the 29->0 wrap-and-flip-nametable-Y quirk.
    ///
    /// Same A12 caveat as [`Self::inc_hori_v`]: this is an internal
    /// register increment, not an address-bus driver.
    const fn inc_vert_v(&mut self) {
        if (self.v & 0x7000) == 0x7000 {
            self.v &= !0x7000;
            let mut y = (self.v & 0x03E0) >> 5;
            if y == 29 {
                y = 0;
                self.v ^= 0x0800;
            } else if y == 31 {
                y = 0;
            } else {
                y += 1;
            }
            self.v = (self.v & !0x03E0) | (y << 5);
        } else {
            self.v += 0x1000;
        }
    }

    /// Copy horizontal bits of `t` into `v` (bits 0-4 + 10).
    const fn copy_hori_t_to_v(&mut self) {
        self.v = (self.v & !0x041F) | (self.t & 0x041F);
    }

    /// Copy vertical bits of `t` into `v` (bits 5-9 + 11-14).
    const fn copy_vert_t_to_v(&mut self) {
        self.v = (self.v & !0x7BE0) | (self.t & 0x7BE0);
    }

    // ------------------------------------------------------------------
    // Pixel emission.
    // ------------------------------------------------------------------

    /// Emit one pixel into the framebuffer at the current `(scanline, dot)`.
    #[allow(clippy::cast_sign_loss)]
    #[allow(clippy::too_many_lines)] // + the ppu-sprite-shifter-counter X-counter/shift loop
    fn emit_pixel(&mut self) {
        let pixel_x = self.dot - 1;
        let pixel_y = self.scanline as u16; // already validated >= 0 by caller
        let fx = self.x;
        // BG pixel (bits 0-1 = pattern, bits 2-3 = palette)
        let (bg_idx, bg_pal) = if self.mask.contains(PpuMask::SHOW_BG)
            && (pixel_x >= 8 || self.mask.contains(PpuMask::SHOW_BG_LEFT))
        {
            let mask = 0x8000u16 >> fx;
            let p0 = u8::from((self.bg_shift_lo & mask) != 0);
            let p1 = u8::from((self.bg_shift_hi & mask) != 0);
            let idx = (p1 << 1) | p0;
            let a0 = u8::from((self.at_shift_lo & mask) != 0);
            let a1 = u8::from((self.at_shift_hi & mask) != 0);
            (idx, (a1 << 1) | a0)
        } else {
            (0, 0)
        };

        // Sprite pixel evaluation (Sprint 2-3).
        let mut spr_idx: u8 = 0;
        let mut spr_pal: u8 = 0;
        let mut spr_priority_front = false;
        let mut spr_zero_pixel = false;
        #[cfg(feature = "hd-pack")]
        let mut spr_slot: usize = 0;
        // v1.8.9 — every opaque sprite covering this pixel (slot indices), for the
        // HD-pack multi-sprite conditions. Collected but never consulted by the
        // winner logic below, so the framebuffer stays byte-identical.
        #[cfg(feature = "hd-pack")]
        let mut hd_sprites: [usize; 4] = [0; 4];
        #[cfg(feature = "hd-pack")]
        let mut hd_spr_n: usize = 0;
        if self.mask.contains(PpuMask::SHOW_SPRITE)
            && (pixel_x >= 8 || self.mask.contains(PpuMask::SHOW_SPRITE_LEFT))
        {
            for i in 0..self.spr_count as usize {
                // v2.0 (ppu-sprite-shifter-counter): a sprite emits when its
                // X-counter is 0 OR it is in the persistent halted state. The
                // `spr_x == 0` term keeps the legacy px-0 emit timing (an X=0
                // sprite re-armed at dot 339 must emit at px 0, not px 1 — else a
                // spurious Sprite-0-Hit test-8 hit), while `spr_halted` carries
                // Stale Sprite t5/6. Default build: the legacy `spr_x == 0`.
                let emit_active = self.spr_x[i] == 0 || self.spr_halted[i];
                if !emit_active {
                    continue;
                }
                let lo = u8::from((self.spr_shift_lo[i] & 0x80) != 0);
                let hi = u8::from((self.spr_shift_hi[i] & 0x80) != 0);
                let val = (hi << 1) | lo;
                if val == 0 {
                    continue;
                }
                // hd-pack: record every opaque sprite covering this pixel.
                #[cfg(feature = "hd-pack")]
                if hd_spr_n < 4 {
                    hd_sprites[hd_spr_n] = i;
                    hd_spr_n += 1;
                }
                // The first opaque sprite (priority order) is the VISIBLE winner;
                // `spr_idx == 0` gates it so only the first sets the render state.
                if spr_idx == 0 {
                    spr_idx = val;
                    spr_pal = self.spr_attr[i] & 0x03;
                    spr_priority_front = (self.spr_attr[i] & 0x20) == 0;
                    #[cfg(feature = "hd-pack")]
                    {
                        spr_slot = i;
                    }
                    if i == 0 && self.spr_zero_in_line {
                        spr_zero_pixel = true;
                    }
                }
                // Default build stops at the winner (byte-identical); the hd-pack
                // build keeps scanning to collect the hidden sprites above (which
                // never touch the winner state, so the framebuffer is unchanged).
                #[cfg(not(feature = "hd-pack"))]
                break;
            }
        }

        // Combine BG + sprite per priority.
        let final_idx = if bg_idx == 0 && spr_idx == 0 {
            // Universal background ($3F00) — EXCEPT the palette backdrop-override
            // (F1.1): with rendering DISABLED and the VRAM address `v` pointing
            // into palette space ($3F00-$3FFF), the palette's shared address line
            // is driven by `v`, so hardware outputs the color at `v & 0x1F`
            // INSTEAD of the backdrop (NESdev "PPU palettes"; Mesen2 `NesPpu.cpp`
            // / ares output stage). This is a DISPLAY artifact only — palette RAM
            // is not mutated. It cannot fire while rendering is enabled: there
            // the fetch pipeline owns `v` and this branch means a transparent
            // pixel, which is the genuine backdrop. `read_palette` applies the
            // $10/$14/$18/$1C mirror + greyscale, so the override is mirror- and
            // greyscale-correct with no extra handling.
            if !self.mask.rendering_enabled() && (self.v & 0x3F00) == 0x3F00 {
                self.read_palette(0x3F00 | (self.v & 0x1F)) & 0x3F
            } else {
                self.read_palette(0x3F00) & 0x3F
            }
        } else if bg_idx == 0 {
            self.read_palette(0x3F10 | (u16::from(spr_pal) << 2) | u16::from(spr_idx)) & 0x3F
        } else if spr_idx == 0 {
            self.read_palette(0x3F00 | (u16::from(bg_pal) << 2) | u16::from(bg_idx)) & 0x3F
        } else {
            // Both opaque. Sprite-0 hit detection (constraints per nesdev).
            if spr_zero_pixel
                && pixel_x < 255
                && !(pixel_x < 8
                    && (!self.mask.contains(PpuMask::SHOW_BG_LEFT)
                        || !self.mask.contains(PpuMask::SHOW_SPRITE_LEFT)))
            {
                self.status.insert(PpuStatus::SPRITE_ZERO_HIT);
            }
            if spr_priority_front {
                self.read_palette(0x3F10 | (u16::from(spr_pal) << 2) | u16::from(spr_idx)) & 0x3F
            } else {
                self.read_palette(0x3F00 | (u16::from(bg_pal) << 2) | u16::from(bg_idx)) & 0x3F
            }
        };

        // Write RGBA8 to framebuffer.
        let off = ((pixel_y as usize) * 256 + pixel_x as usize) * 4;
        // v2.8.0 Phase 4 — route through the precomputed
        // `(emphasis << 6) | color` lookup (built from the same pure
        // `palette_color_to_rgba`, so byte-identical to the old per-pixel
        // call for both the 2C02 composite default and the Vs./PC10 RGB
        // palettes) and store all four bytes with one bounds-checked slice
        // copy instead of four indexed stores.
        let emph = usize::from((self.mask.bits() >> 5) & 0x07);
        let lut_idx = (emph << 6) | usize::from(final_idx);
        let rgba = self.rgba_lut[lut_idx];
        self.framebuffer[off..off + 4].copy_from_slice(&rgba);
        // Parallel palette-index output for the `NES_NTSC` composite filter
        // (T-110-A1). Same `(emphasis << 6) | colour` value, in index space;
        // `off` is the RGBA byte offset, so `off >> 2` is the pixel index.
        self.index_framebuffer[off >> 2] = lut_idx as u16;

        // v1.2.0 C3 (hd-pack): record the CHR tile that produced this pixel,
        // mirroring the BG-vs-sprite priority decision above. Output-only; this
        // reads only already-computed local state, so the framebuffer and all
        // timing are byte-identical whether the feature is on or off.
        #[cfg(feature = "hd-pack")]
        {
            // A pixel shows the SPRITE iff the sprite pixel is opaque AND
            // (the BG pixel is transparent OR the sprite has front priority) —
            // the same condition the `final_idx` priority match encodes.
            let shows_sprite = spr_idx != 0 && (bg_idx == 0 || spr_priority_front);
            // Multi-sprite telemetry: the identity of every opaque sprite covering
            // this pixel (front-to-back), for `spriteAtPosition` / `spriteNearby`.
            let hd_sprite_list: [HdSprite; 4] = {
                let mut arr = [HdSprite::default(); 4];
                for (k, slot) in hd_sprites.iter().take(hd_spr_n).enumerate() {
                    arr[k] = HdSprite {
                        chr_tile_index: self.hd_spr_idx[*slot],
                        palette_colors: self.hd_sprite_palette_colors(self.spr_attr[*slot] & 0x03),
                    };
                }
                arr
            };
            let hd_sprite_n = u8::try_from(hd_spr_n).unwrap_or(4);
            let rec = if shows_sprite {
                let attr = self.spr_attr[spr_slot];
                let flip_h = (attr & 0x40) != 0;
                // Column within the sprite (screen X minus the sprite's origin X),
                // then flip so the captured offset samples the UNFLIPPED
                // replacement directly (composite is flip-free).
                let col = pixel_x.wrapping_sub(u16::from(self.hd_spr_x[spr_slot])) & 7;
                let off_x = if flip_h { 7 - col } else { col };
                HdTileSource {
                    chr_addr: self.hd_spr_addr[spr_slot],
                    palette: spr_pal,
                    is_sprite: true,
                    flip_h,
                    flip_v: (attr & 0x80) != 0,
                    palette_colors: self.hd_sprite_palette_colors(spr_pal),
                    offset_x: u8::try_from(off_x).unwrap_or(0),
                    offset_y: self.hd_spr_off_y[spr_slot], // already flip-baked at fetch
                    chr_tile_index: self.hd_spr_idx[spr_slot],
                    color_mask: self.mask.bits() & 0xE1,
                    sprites: hd_sprite_list,
                    sprite_count: hd_sprite_n,
                }
            } else if bg_idx != 0 {
                // Fine-X picks which of the two shifter tiles this pixel shows +
                // the column within it (Mesen usePrev / OffsetX); fine-Y is the row.
                let pos = u16::from(fx) + (pixel_x & 7);
                let chr = if pos < 8 {
                    self.hd_bg_addr_cur
                } else {
                    self.hd_bg_addr_next
                };
                HdTileSource {
                    chr_addr: chr,
                    palette: bg_pal,
                    is_sprite: false,
                    flip_h: false,
                    flip_v: false,
                    palette_colors: self.hd_bg_palette_colors(bg_pal),
                    offset_x: u8::try_from(pos & 7).unwrap_or(0),
                    offset_y: u8::try_from((self.v >> 12) & 7).unwrap_or(0),
                    chr_tile_index: if pos < 8 {
                        self.hd_bg_idx_cur
                    } else {
                        self.hd_bg_idx_next
                    },
                    color_mask: self.mask.bits() & 0xE1,
                    sprites: hd_sprite_list,
                    sprite_count: hd_sprite_n,
                }
            } else {
                // Universal background — no tile to substitute.
                HdTileSource {
                    chr_addr: HD_TILE_NONE,
                    palette: 0,
                    is_sprite: false,
                    flip_h: false,
                    flip_v: false,
                    offset_x: 0,
                    offset_y: 0,
                    chr_tile_index: HD_CHR_RAM,
                    palette_colors: 0,
                    color_mask: 0,
                    sprites: hd_sprite_list,
                    sprite_count: hd_sprite_n,
                }
            };
            self.hd_tile_source[off >> 2] = rec;
        }

        // Decrement sprite X-counters / shift sprite shift regs.
        //
        // v2.0 (ppu-sprite-shifter-counter): the X-COUNTER decrements every
        // visible dot regardless of rendering (Stale Sprite test 2 — forced
        // blank does NOT halt the counters), but the SHIFTER only advances while
        // rendering is ENABLED on the 1-PPU-dot-delayed gate (`rendering_enabled_
        // delayed`, the same gate `shift_bg` uses — test 3: the shifter PAUSES in
        // forced blank so a sprite's data survives a long blank and still draws
        // on re-enable). `spr_halted` is the persistent latch (set at counter==0
        // or across a disable; re-armed at dot 339) carrying Stale Sprite t5/6.
        // Default build: the legacy unconditional shift.
        for i in 0..self.spr_count as usize {
            if self.spr_halted[i] || self.spr_x[i] == 0 {
                // Halted / drawing: latch and shift while rendering is enabled.
                // The `spr_x == 0` term is load-bearing — a slot re-armed at dot
                // 339 with the counter already 0 must SHIFT this dot (not just
                // latch) to match the legacy `spr_x == 0 => shift` timing.
                self.spr_halted[i] = true;
                if self.rendering_enabled_delayed {
                    self.spr_shift_lo[i] <<= 1;
                    self.spr_shift_hi[i] <<= 1;
                }
            } else {
                // Counting: decrement every visible dot (forced blank does not
                // halt — test 2). On reaching 0, halt this tick.
                self.spr_x[i] -= 1;
                if self.spr_x[i] == 0 {
                    self.spr_halted[i] = true;
                }
            }
        }
    }

    // ------------------------------------------------------------------
    // Sprite evaluation + tile fetch.
    // ------------------------------------------------------------------

    /// Per-PPU-dot sprite-evaluation FSM.
    ///
    /// Reproduces the 2C02's three-phase sprite-eval state machine across
    /// dots 1..=256 of every visible scanline and the pre-render line:
    ///
    /// - **Dot 0**: reset FSM working state.
    /// - **Dots 1..=64**: clear secondary OAM to `$FF`. One byte cleared
    ///   every two dots (32 bytes over 64 dots). Reads of `$2004` during
    ///   this phase return `$FF` on real hardware.
    /// - **Dots 65..=256**: 192 dots = 96 read/write pairs. Odd dots read
    ///   a byte from primary OAM into a latch; even dots commit the latch
    ///   into secondary OAM (when copying is enabled). The buggy `n+m`
    ///   increment for overflow detection (when 8 sprites are already
    ///   latched) matches the documented hardware quirk that
    ///   `sprite_overflow_tests/4-Obscure` and `/5-Emulator` exercise.
    /// - **Dot 256**: commit `spr_count` and pre-clear unused slot
    ///   rendering-side arrays so the pixel pipeline never emits stale
    ///   sprite pixels.
    ///
    /// The actual per-slot pattern-table fetch (and its A12 transitions)
    /// happens later, in [`Self::fetch_sprite_tile`], unchanged. Sprite-
    /// tile fetches still dispatch at dots 260, 268, ..., 316 — preserving
    /// the canonical "241 A12 rises per NTSC frame" MMC3 IRQ count.
    /// v2.0 Tier 1.2 — value `$2004` returns while the screen is being drawn.
    ///
    /// Mirrors Mesen2 `NesPpu::ReadRam`'s `SpriteData` case
    /// (`NesPpu.cpp:361-380`): during the sprite-tile-load window (dots
    /// 257-320) the OAM data bus carries `secondary_oam[sprite*4 + min(step,3)]`
    /// (the 4th byte held for the 5 idle fetch cycles); at every other rendered
    /// dot it carries `oam_bus_copybuffer` (the sprite-eval data latch
    /// maintained by [`Self::tick_oam_bus`]). Caller has already checked
    /// `scanline <= 239 && rendering`.
    fn oam_data_bus_read(&self) -> u8 {
        if (257..=320).contains(&self.dot) {
            let phase = (self.dot - 257) % 8;
            let step = if phase > 3 { 3 } else { phase };
            let oam_addr = ((self.dot - 257) / 8) * 4 + step;
            self.oam_bus_secondary[(oam_addr & 0x1F) as usize]
        } else {
            self.oam_bus_copybuffer
        }
    }

    /// v2.0 Tier 1.2 — per-dot driver for the isolated OAM-data-bus model.
    ///
    /// A faithful, side-effect-free port of Mesen2's
    /// `NesPpu::ProcessSpriteEvaluation` (`NesPpu.cpp:1015-1141`, default
    /// config — `EnablePpuSpriteEvalBug` off) plus the cycle-321 copybuffer
    /// reset (`NesPpu.cpp:945-951`). It maintains ONLY `oam_bus_copybuffer` +
    /// the parallel `oam_bus_secondary`; it reads primary `oam` read-only and
    /// NEVER touches the real sprite-eval / overflow / sprite-zero state (so
    /// the existing rendering FSM is unperturbed — `$2004` reads are the sole
    /// observable effect of this whole feature). Called each dot on visible
    /// scanlines (0-239) when rendering is enabled.
    fn tick_oam_bus(&mut self) {
        let cycle = self.dot;
        let sprite_height: i16 = if self.ctrl.contains(PpuCtrl::SPRITE_SIZE_16) {
            16
        } else {
            8
        };
        // Y-test reference: the scanline being evaluated (sprites render on
        // scanline+1). Mesen uses `_scanline` directly here.
        let scan = self.scanline;

        if cycle == 0 {
            return;
        }
        if cycle < 65 {
            // Secondary-OAM clear (cycles 1-64): the bus carries $FF and the
            // parallel secondary OAM is filled with $FF, 1 byte per 2 dots.
            self.oam_bus_copybuffer = 0xFF;
            self.oam_bus_secondary[((cycle - 1) >> 1) as usize] = 0xFF;
            return;
        }
        if cycle <= 256 {
            if cycle & 1 == 1 {
                // Odd cycle: read a byte from primary OAM into the bus latch.
                if cycle == 65 {
                    // ProcessSpriteEvaluationStart: seed the eval pointer from
                    // OAMADDR (eval can begin mid-sprite if $2003 was written).
                    self.oam_bus_sprite_in_range = false;
                    self.oam_bus_secondary_addr = 0;
                    self.oam_bus_overflow_counter = 0;
                    self.oam_bus_copy_done = false;
                    self.oam_bus_addr_h = (self.oam_addr >> 2) & 0x3F;
                    self.oam_bus_addr_l = self.oam_addr & 0x03;
                }
                let addr = ((self.oam_bus_addr_l & 0x03) | (self.oam_bus_addr_h << 2)) as usize;
                // v2.1.4 F2.3 — OAM-decay read hook (no-op at the default): a
                // sprite-evaluation primary-OAM read refreshes the row's DRAM
                // cells (this is what keeps OAM alive during normal rendering).
                self.oam_decay_on_read((addr & 0xFF) as u8);
                let raw = self.oam[addr & 0xFF];
                // OAM byte 2 (attributes) bits 2-4 are unimplemented (read 0).
                self.oam_bus_copybuffer = if addr & 0x03 == 0x02 { raw & 0xE3 } else { raw };
            } else {
                // Even cycle: copy / decide.
                let cb = self.oam_bus_copybuffer as i16;
                let cb_in_range = scan >= cb && scan < cb + sprite_height;
                if self.oam_bus_copy_done {
                    self.oam_bus_addr_h = (self.oam_bus_addr_h + 1) & 0x3F;
                    // OAM write-disable turns secondary-OAM writes into reads.
                    // On early (pre-rev-G) 2C02s the data bus reads back the
                    // last byte the OAM-address counter rests on EVEN when fewer
                    // than 8 sprites were found (secondary_addr < 0x20) — the
                    // "OAM2[OAM2Address] every other cycle" behavior AccuracyCoin
                    // `$2004 Stress` section 6 documents. Mesen2 gates this on
                    // `secondary_addr >= 0x20` (rev-G+), which is why no Mesen
                    // config reproduces the section-6 `$03`; the test's answer
                    // key (the spec) wants the unconditional read. Each
                    // out-of-range sprite's Y was already written to
                    // `secondary[secondary_addr]` (the frozen index) below, so
                    // this reads back that last-written Y.
                    self.oam_bus_copybuffer =
                        self.oam_bus_secondary[(self.oam_bus_secondary_addr & 0x1F) as usize];
                } else {
                    if !self.oam_bus_sprite_in_range && cb_in_range {
                        self.oam_bus_sprite_in_range = true;
                    }
                    if self.oam_bus_secondary_addr < 0x20 {
                        // Copy one byte to (parallel) secondary OAM.
                        self.oam_bus_secondary[self.oam_bus_secondary_addr as usize] =
                            self.oam_bus_copybuffer;
                        if self.oam_bus_sprite_in_range {
                            self.oam_bus_addr_l += 1;
                            self.oam_bus_secondary_addr += 1;
                            if self.oam_bus_addr_l >= 4 {
                                self.oam_bus_addr_h = (self.oam_bus_addr_h + 1) & 0x3F;
                                self.oam_bus_addr_l = 0;
                                if self.oam_bus_addr_h == 0 {
                                    self.oam_bus_copy_done = true;
                                }
                            }
                            if self.oam_bus_secondary_addr.trailing_zeros() >= 2 {
                                // Finished copying all 4 bytes of this sprite.
                                self.oam_bus_sprite_in_range = false;
                                if self.oam_bus_addr_l != 0 && !cb_in_range {
                                    self.oam_bus_addr_l = 0;
                                }
                            }
                        } else {
                            // Nothing to copy — skip to the next sprite.
                            self.oam_bus_addr_h = (self.oam_bus_addr_h + 1) & 0x3F;
                            self.oam_bus_addr_l = 0;
                            if self.oam_bus_addr_h == 0 {
                                self.oam_bus_copy_done = true;
                            }
                        }
                    } else {
                        // 8 sprites found: secondary-OAM writes become reads.
                        self.oam_bus_copybuffer =
                            self.oam_bus_secondary[(self.oam_bus_secondary_addr & 0x1F) as usize];
                        if self.oam_bus_sprite_in_range {
                            // Overflow detected. (NOTE: the REAL SpriteOverflow
                            // flag is owned by the existing eval FSM — this
                            // isolated model deliberately does not set it.)
                            self.oam_bus_addr_l += 1;
                            if self.oam_bus_addr_l == 4 {
                                self.oam_bus_addr_h = (self.oam_bus_addr_h + 1) & 0x3F;
                                self.oam_bus_addr_l = 0;
                            }
                            if self.oam_bus_overflow_counter == 0 {
                                self.oam_bus_overflow_counter = 3;
                            } else {
                                self.oam_bus_overflow_counter -= 1;
                                if self.oam_bus_overflow_counter == 0 {
                                    self.oam_bus_copy_done = true;
                                    self.oam_bus_addr_l = 0;
                                }
                            }
                        } else {
                            // Sprite-eval bug: increment BOTH H and L.
                            self.oam_bus_addr_h = (self.oam_bus_addr_h + 1) & 0x3F;
                            self.oam_bus_addr_l = (self.oam_bus_addr_l + 1) & 0x03;
                            if self.oam_bus_addr_h == 0 {
                                self.oam_bus_copy_done = true;
                            }
                        }
                    }
                }
            }
            return;
        }
        if cycle == 321 {
            // After sprite loading, the bus rests on secondary OAM index 0.
            self.oam_bus_copybuffer = self.oam_bus_secondary[0];
        }
    }

    pub(crate) fn tick_sprite_eval_per_dot(&mut self) {
        // Y-test reference line for sprite evaluation. Per nesdev
        // "PPU OAM" (Byte 0): "The first scanline that the sprite is
        // rendered on is one greater than this value." Hardware
        // performs the y-test `(scanline - y) in [0, h-1]` using the
        // CURRENT scanline counter — the eval at scanline N produces
        // sprites that render on scanline N+1. So sprite Y=N renders
        // on scanlines N+1..=N+h.
        //
        // Pre-render (scanline 261) prepares for scanline 0, but
        // scanline 0 never displays sprites per nesdev. We model
        // this by using -1 as the y-test reference, which makes
        // `-1 - y < 0` for all OAM y values, so the y-test always
        // fails at pre-render and scanline 0 sees no sprites.
        let next_line: i16 = if self.scanline == self.region.prerender_line() {
            -1
        } else {
            self.scanline
        };
        let sprite_height: i16 = if self.ctrl.contains(PpuCtrl::SPRITE_SIZE_16) {
            16
        } else {
            8
        };

        match self.dot {
            0 => {
                // Start-of-scanline: reset FSM working state. We do NOT
                // touch the rendering-side `spr_*` arrays or
                // `spr_zero_in_line` here — they were committed at the
                // PREVIOUS scanline's dot 256 and are about to be read
                // by this scanline's sprite-pixel evaluator on dots
                // 1..=256.
                self.sprite_eval_n = 0;
                self.sprite_eval_m = 0;
                self.sprite_eval_found = 0;
                self.sprite_eval_sec_idx = 0;
                self.sprite_eval_copying = false;
                self.sprite_eval_done = false;
                self.sprite_eval_overflow_search = false;
                self.sprite_eval_read_latch = 0xFF;
                self.sprite_eval_zero_found = false;
                // Phase 3a: capture eval base from OAMADDR at the
                // dot-0 reset so the dots 65-256 active loop starts
                // walking from the captured `(start_n, start_m)`
                // position.  Mesen2 captures at cycle 65 (in
                // ProcessSpriteEvaluationStart); we capture at dot 0
                // because our FSM does the eval-base read BEFORE
                // dot 65 (the first read at dot 65 already needs
                // the offset).  This matters when the CPU writes
                // $2003 mid-vblank to set OAMADDR before the next
                // scanline's eval begins.
                {
                    self.sprite_eval_n = (self.oam_addr >> 2) & 0x3F;
                    self.sprite_eval_m = self.oam_addr & 0x03;
                }
                self.sprite_eval_first_iter = true;
            }
            1..=64 => {
                // Clear phase. Even-dot writes a $FF into secondary OAM
                // (1 byte per 2 dots, 32 bytes over 64 dots). Odd dots
                // are idle reads (driving $FF onto the bus).
                //
                // The pre-2026-05-17 implementation also reset the
                // rendering-side `spr_*` arrays + `spr_count` +
                // `spr_zero_in_line` here at dot 64. That was a B8a
                // regression: the rendering loop at line 1146..=1220
                // READS those arrays on dots 1..=256 of the CURRENT
                // scanline, so resetting them mid-scanline destroyed
                // sprites for dots 64..=256 (the right ~75% of every
                // scanline). The dot 256 End-of-eval fixup below is
                // the correct time to commit the NEXT scanline's
                // values; the dot 64 reset has been removed.
                if (self.dot & 1) == 0 {
                    let idx = ((self.dot - 1) >> 1) as usize;
                    if idx < self.secondary_oam.len() {
                        self.secondary_oam[idx] = 0xFF;
                    }
                }
            }
            65..=256 => {
                if !self.sprite_eval_done {
                    self.tick_sprite_eval_active_dot(next_line, sprite_height);
                }

                if self.dot == 256 {
                    // End-of-eval fixup: commit spr_count and the
                    // eval-side sprite-0 latch onto the rendering-side
                    // arrays. Pre-clear slots we did NOT fill so unused
                    // ones produce no output even though
                    // `fetch_sprite_tile` always runs all 8 slots.
                    self.spr_count = self.sprite_eval_found;
                    self.spr_zero_in_line = self.sprite_eval_zero_found;
                    for i in (self.spr_count as usize)..8 {
                        self.spr_shift_lo[i] = 0;
                        self.spr_shift_hi[i] = 0;
                        self.spr_attr[i] = 0;
                        self.spr_x[i] = 0xFF;
                    }
                }
            }
            _ => {
                // Dots 257..=340: eval is idle; sprite tile fetches happen
                // elsewhere (`fetch_sprite_tile`, scheduled at dots 260,
                // 268, ..., 316 from the tick() main path).
            }
        }
    }

    /// Per-active-dot helper for the per-PPU-dot FSM. Drives the
    /// alternating read/write semantics of dots 65..=256 when eval has
    /// not yet exhausted primary OAM or set overflow.
    #[allow(clippy::too_many_lines)] // Phase 3a feature-gated branches expand the line count beyond the threshold; refactoring into sub-helpers would require sharing 5+ mutable fields by reference, hurting readability.
    fn tick_sprite_eval_active_dot(&mut self, next_line: i16, sprite_height: i16) {
        if (self.dot & 1) == 1 {
            // Odd dot: read.
            // Per nesdev wiki "PPU sprite evaluation": during dots 65-256,
            // the hardware updates OAMADDR to track the current eval read
            // position. A CPU $2004 read at this time sees the OAM byte
            // at that walking index. We surface the eval position into
            // `oam_addr` so that CPU reads of $2004 during sprite eval
            // observe the same behavior as real silicon. The dot-257-320
            // OAMADDR-reset added in `Ppu::tick` washes this back to 0
            // after eval, preserving the post-eval semantics that the
            // existing $4014 OAM DMA / blargg sprite_hit_tests rely on.
            // Phase 3a: under the eval-base-from-OAMADDR feature, the
            // y-test address ALWAYS uses `n*4 + m` so a misaligned
            // start (`oam_addr & 0x03 != 0` at dot 0) reads the
            // appropriate byte of the start sprite as the Y candidate
            // (Mesen2 `_spriteAddrL` model).  Under the legacy path,
            // `m` is reset to 0 between sprites and the y-test always
            // reads byte 0; the legacy special-case is preserved for
            // bit-exact compatibility.
            let addr = ((self.sprite_eval_n as usize) * 4) + (self.sprite_eval_m as usize);
            // v2.1.4 F2.3 — OAM-decay read hook (no-op at the default): the legacy
            // per-dot sprite-eval read path also refreshes the row it reads, so both
            // eval models keep OAM alive identically during rendering.
            self.oam_decay_on_read((addr & 0xFF) as u8);
            self.sprite_eval_read_latch = self.oam[addr & 0xFF];
            // Expose the current eval index via the OAMADDR register
            // (truncated to u8 via the `& 0xFF` mask). This is the
            // documented hardware behavior — see AccuracyCoin
            // `TEST_ArbitrarySpriteZero` sub-test 2's lengthy comment
            // explaining the eval / OAMADDR interaction.
            self.oam_addr = (addr & 0xFF) as u8;
        } else {
            // Even dot: write/decide.
            let latch = self.sprite_eval_read_latch;
            if self.sprite_eval_overflow_search {
                // Treat the read byte as a y-coord candidate.
                let row = next_line - (latch as i16);
                if row >= 0 && row < sprite_height {
                    self.status.insert(PpuStatus::SPRITE_OVERFLOW);
                    self.sprite_eval_done = true;
                } else {
                    // Buggy n+m increment: increment BOTH.
                    self.sprite_eval_m = (self.sprite_eval_m + 1) & 0x03;
                    if self.sprite_eval_n == 63 {
                        self.sprite_eval_done = true;
                    } else {
                        self.sprite_eval_n += 1;
                    }
                }
            } else if self.sprite_eval_copying {
                // Copy byte (m == 1, 2, 3) into secondary OAM.
                let sec_idx = self.sprite_eval_sec_idx as usize;
                if sec_idx < self.secondary_oam.len() {
                    self.secondary_oam[sec_idx] = latch;
                }
                self.sprite_eval_sec_idx += 1;
                self.sprite_eval_m += 1;
                // Phase 3a: under the eval-base feature, continue
                // copying until the secondary OAM is aligned to a
                // sprite boundary (sec_idx % 4 == 0) — Mesen2's model
                // (`_secondaryOamAddr & 0x03 == 0` check at line
                // 1062).  This handles misaligned start where 4
                // sequential reads from `(start_n*4+start_m)` span
                // sprite boundaries.  Under the legacy path,
                // `m == 4` is identical to "sec_idx & 3 == 0" because
                // copying always starts at m=1 (after y-test at m=0),
                // so they're equivalent in the legacy case.
                let copy_done = self.sprite_eval_sec_idx.trailing_zeros() >= 2;
                if self.sprite_eval_m == 4 {
                    self.sprite_eval_m = 0;
                    self.sprite_eval_n = (self.sprite_eval_n + 1) & 0x3F;
                }
                if copy_done {
                    // Finished this sprite. found was already
                    // incremented when the y-byte landed.
                    self.sprite_eval_copying = false;
                    self.sprite_eval_m = 0;
                    // Under feature: the m==4 wrap above already
                    // advanced n once.  Don't double-increment.
                    // Under legacy: m never wrapped, so n advances
                    // here for the first (and only) time.
                    {
                        // n was already advanced in the m==4 wrap
                        // block above; just check terminal conditions.
                        if self.sprite_eval_found == 8 {
                            self.sprite_eval_overflow_search = true;
                        }
                        if self.sprite_eval_n == 0 {
                            // n wrapped past 63 to 0 — done.
                            self.sprite_eval_done = true;
                        }
                    }
                }
            } else {
                // Y-test for sprite n.
                let row = next_line - (latch as i16);
                let in_range = row >= 0 && row < sprite_height;
                if in_range && self.sprite_eval_found < 8 {
                    // Write y into secondary OAM and start copying
                    // bytes 1..=3 over the next 3 even-dot writes.
                    let sec_idx = self.sprite_eval_sec_idx as usize;
                    if sec_idx < self.secondary_oam.len() {
                        self.secondary_oam[sec_idx] = latch;
                    }
                    self.sprite_eval_sec_idx += 1;
                    // Sprite-zero-hit eligibility: per nesdev wiki +
                    // Mesen2 (`NesPpu::ProcessSpriteEvaluation` line
                    // 1040-1044, "If the first Y coordinate we load
                    // is in range, set the sprite 0 flag — this
                    // happens even if this isn't actually the first
                    // sprite in OAM (i.e. because OAMADDR was not 0
                    // when evaluation started)"), the sprite at the
                    // eval-start position is sprite-zero IFF its Y
                    // is in range — NOT "first in-range sprite found".
                    // If the start sprite is out-of-range, no sprite
                    // on this scanline is sprite-zero.  Under Phase 3a,
                    // gate on `sprite_eval_first_iter` (the first y-test
                    // of the scanline); the legacy path keeps the
                    // canonical `n == 0` check.
                    let is_first_inrange = self.sprite_eval_first_iter;
                    if is_first_inrange {
                        self.sprite_eval_zero_flag_on();
                    }
                    self.sprite_eval_found += 1;
                    self.sprite_eval_copying = true;
                    // Phase 3a: increment from CURRENT m (handles
                    // misaligned start where eval began at m != 0).
                    // Legacy path resets to m=1 (canonical "skip Y,
                    // copy bytes 1..=3" pattern).
                    {
                        self.sprite_eval_m += 1;
                        if self.sprite_eval_m == 4 {
                            // Wrapped past end of sprite — already
                            // "copied" the whole sprite from its
                            // misaligned start.  Advance n, reset m.
                            self.sprite_eval_copying = false;
                            self.sprite_eval_m = 0;
                            if self.sprite_eval_n == 63 {
                                self.sprite_eval_done = true;
                            } else {
                                self.sprite_eval_n += 1;
                            }
                        }
                    }
                } else if in_range && self.sprite_eval_found == 8 {
                    // Defensive: 9th in-range sprite at the y-tested
                    // cell. In practice the `found == 8` transition
                    // happens at the end of copying sprite 7, which
                    // flips into `overflow_search` mode, so this branch
                    // is unreachable. Kept for safety.
                    self.status.insert(PpuStatus::SPRITE_OVERFLOW);
                    self.sprite_eval_done = true;
                } else {
                    // Not in range: advance to next sprite.
                    if self.sprite_eval_n == 63 {
                        self.sprite_eval_done = true;
                    } else {
                        self.sprite_eval_n += 1;
                    }
                }
                // Phase 3a: clear the "first-iteration" flag AFTER the
                // first y-test fires (regardless of in-range result).
                // Per Mesen2 `_cycle == 66` semantics — sprite-zero is
                // set only on the FIRST y-test that lands in range,
                // and only if it's the FIRST iteration overall.
                self.sprite_eval_first_iter = false;
            }
        }
    }

    /// Helper: set the per-scanline sprite-zero-in-line flag from the
    /// FSM. Sets the EVAL-side latch (`sprite_eval_zero_found`); the
    /// rendering-side flag (`spr_zero_in_line`) is committed from this
    /// latch at dot 256.
    const fn sprite_eval_zero_flag_on(&mut self) {
        self.sprite_eval_zero_found = true;
    }

    /// Arm the OAM-corruption disable edge on a `$2001` write — faithful
    /// port of `TriCNES`'s `$2001` write path (`Emulator.cs` lines
    /// 9684-9696 / 1740-1755). When rendering was ON before the write and
    /// the new mask turns BOTH BG + sprites OFF while on a render line (NOT
    /// in vblank), set the disable flags. `_instant` is the
    /// data-bus-immediate path (OAM eval observes the disable the same
    /// cycle); the non-instant flag is the regular 1-dot-delayed path. The
    /// disable edge is captured against the live eval pointer during the
    /// dots 1-64 window and committed on re-enable; the `!pending` guard
    /// stops a write from re-arming over an already-captured corruption.
    const fn arm_oam_corruption_disable(&mut self, was_rendering: bool) {
        if was_rendering
            && !self.mask.rendering_enabled()
            && !self.oam_corruption_pending
            && (self.scanline < self.region.vblank_start_line()
                || self.scanline == self.region.prerender_line())
        {
            self.oam_corruption_disabled = true;
            self.oam_corruption_disabled_instant = true;
        }
    }

    /// OAM-corruption per-dot driver — faithful port of `TriCNES`'s
    /// `PPU_Render_SpriteEvaluation` corruption handling (`Emulator.cs`
    /// lines 2664-2762). Called every render-line dot (independent of the
    /// rendering gate, so the disable edge is observed even after the
    /// sprite-eval FSM stops). Three responsibilities, in `TriCNES` order:
    ///
    /// 1. **Commit on re-enable.** If rendering is currently enabled and a
    ///    corruption is pending, apply it (`TriCNES` applies on the first
    ///    rendered dot once `PPU_Mask_Show*_Instant` is set again). The
    ///    pre-render-line dot-0 hook in `tick` handles the
    ///    re-enable-during-vblank case.
    /// 2. **Maintain `OAM2Address` across the dots 1-64 clear window.**
    ///    Reset at dot 1; incremented once per even clear dot, masked to
    ///    0x1F — exactly as `TriCNES` drives `OAM2Address` during dots 1-64.
    /// 3. **Capture the index at the disable edge.** When the disable
    ///    flag (`oam_corruption_disabled` / `_instant`, armed by the
    ///    `$2001` write) is set during the dots 1-64 window on a NON
    ///    pre-render line, set `oam_corruption_pending` and capture
    ///    `oam_corruption_index = oam2_addr` (the live secondary-OAM write
    ///    pointer). The pre-render line is excluded from the capture (it is
    ///    a read-only eval line for OAM-corruption purposes in `TriCNES`).
    fn tick_oam_corruption(&mut self, rendering: bool) {
        let pre_render = self.scanline == self.region.prerender_line();

        // (1) Commit pending corruption once rendering is (re-)enabled
        // during the eval window. The pre-render dot-0 path in `tick`
        // covers the re-enable-in-VBlank case separately.
        if rendering && self.oam_corruption_pending && !pre_render && self.dot >= 1 {
            self.process_oam_corruption();
        }

        // (2) + (3) only matter inside the dots 1-64 secondary-OAM clear
        // window. Outside it the disable flags simply persist until the
        // next eval window (or are committed above on re-enable).
        if (1..=64).contains(&self.dot) {
            if self.dot == 1 {
                // TriCNES resets OAM2Address at dot 1 of the eval window.
                self.oam2_addr = 0;
            }

            // Capture the disable edge against the LIVE pointer, on a
            // non-pre-render line only (TriCNES: capture is skipped on the
            // read-only pre-render eval line).
            if (self.oam_corruption_disabled || self.oam_corruption_disabled_instant)
                && !pre_render
                && !self.oam_corruption_pending
            {
                self.oam_corruption_pending = true;
                self.oam_corruption_index = self.oam2_addr;
            }
            // The disable arming is single-shot: clear it once observed in
            // the eval window (TriCNES clears both flags when it fires).
            self.oam_corruption_disabled = false;
            self.oam_corruption_disabled_instant = false;

            // Advance OAM2Address on even clear dots (mirrors TriCNES's
            // `OAM2[OAM2Address] = latch; OAM2Address = (OAM2Address+1) &
            // 0x1F` on even cycles of the dots 1-64 clear).
            if (self.dot & 1) == 0 {
                self.oam2_addr = (self.oam2_addr + 1) & 0x1F;
            }
        }
    }

    /// Apply a pending OAM corruption — faithful port of `TriCNES`'s
    /// `CorruptOAM` (`Emulator.cs` lines 2635-2651): one OAM "row" of 8
    /// bytes is overwritten from row 0, plus the corresponding secondary-OAM
    /// byte. The index (`oam_corruption_index`) was captured at the disable
    /// edge from the live secondary-OAM eval pointer; index 0x20 wraps to 0.
    /// Clears the pending flag.
    fn process_oam_corruption(&mut self) {
        let mut index = self.oam_corruption_index as usize;
        if index == 0x20 {
            index = 0;
        }
        // OAM[index*8 + i] = OAM[i] for i in 0..8 (a no-op when index == 0,
        // matching TriCNES — it still runs, copying row 0 onto itself).
        let first_eight: [u8; 8] = [
            self.oam[0],
            self.oam[1],
            self.oam[2],
            self.oam[3],
            self.oam[4],
            self.oam[5],
            self.oam[6],
            self.oam[7],
        ];
        let dst = index * 8;
        if dst + 8 <= self.oam.len() {
            self.oam[dst..dst + 8].copy_from_slice(&first_eight);
        }
        // Also corrupt secondary OAM: OAM2[index] = OAM2[0].
        if index < self.secondary_oam.len() {
            self.secondary_oam[index] = self.secondary_oam[0];
        }
        self.oam_corruption_pending = false;
    }

    /// Fetch one sprite slot's pattern bytes.  Always called for all 8
    /// slots — for unused slots the secondary-OAM bytes are $FF, producing
    /// a dummy fetch that still toggles A12 to the sprite pattern table on
    /// real hardware.  This is what generates the per-scanline A12 rising
    /// edge that MMC3's IRQ counter clocks on.
    #[allow(clippy::cast_sign_loss)]
    fn fetch_sprite_tile<B: PpuBus>(&mut self, bus: &mut B, slot: usize) {
        // Mirrors the y-test convention in `tick_sprite_eval_per_dot`:
        // `next_line` is the y-test reference = the CURRENT scanline
        // counter (or -1 for pre-render). The fetched row index is
        // `next_line - y`, which matches the row that will be
        // displayed on `next_line + 1` (the next scanline that
        // renders the eval result).
        //
        // v2.0 (ppu-sprite-shifter-counter): treat the pre-render line as
        // scanline `(prerender_line & 0xFF)` (NTSC 261 & 255 = 5) for the
        // sprite-tile in-range check, so a sprite whose pixel lands on row 5
        // loads into the shifters for scanline 0 (the stale secondary-OAM slots
        // filtered by the `load` gate below) — AccuracyCoin "Sprites On Scanline
        // 0". Default keeps the `-1` reference (scanline 0 sees no sprites).
        let next_line: i16 = if self.scanline == self.region.prerender_line() {
            self.region.prerender_line() & 0xFF
        } else {
            self.scanline
        };
        let sprite_height: i16 = if self.ctrl.contains(PpuCtrl::SPRITE_SIZE_16) {
            16
        } else {
            8
        };
        let base = slot * 4;
        let y = self.secondary_oam[base] as i16;
        let tile = self.secondary_oam[base + 1];
        let attr = self.secondary_oam[base + 2];
        let xpos = self.secondary_oam[base + 3];
        let in_use = slot < self.spr_count as usize;
        let flip_v = (attr & 0x80) != 0;
        let flip_h = (attr & 0x40) != 0;

        // For unused slots, the row delta isn't meaningful (Y=$FF makes it
        // negative or huge) — pin to 0 so the address arithmetic is well
        // defined.  The only thing that matters here is that the pattern
        // address lands in the sprite pattern table, which it does because
        // the sprite-table-select bit is set as PPUCTRL bit 3 (8x8 mode)
        // or tile bit 0 (8x16 mode); for the cleared $FF tile in 8x16 mode
        // bit 0 = 1 picks the $1000 table.
        let mut row: u16 = if in_use {
            (next_line.wrapping_sub(y)).clamp(0, sprite_height - 1) as u16
        } else {
            0
        };

        let (table, tile_idx, in_tile_row) = if sprite_height == 16 {
            let table = u16::from(tile & 0x01) << 12;
            let mut tindex = tile & 0xFE;
            if flip_v && in_use {
                row = 15 - row;
            }
            if row >= 8 {
                tindex = tindex.wrapping_add(1);
                row -= 8;
            }
            (table, tindex, row)
        } else {
            let table = u16::from(self.ctrl.contains(PpuCtrl::SPRITE_PATTERN_HIGH)) << 12;
            let r = if flip_v && in_use { 7 - row } else { row };
            (table, tile, r)
        };

        let addr_lo = table | (u16::from(tile_idx) << 4) | in_tile_row;
        let addr_hi = addr_lo | 0x08;
        self.observe_a12_addr(bus, addr_lo);
        // Sprite CHR fetch: route through `ppu_read_sprite` so MMC5
        // (and any other mapper with split sprite vs. BG CHR banking)
        // can use its sprite-specific bank registers.
        let mut lo = bus.ppu_read_sprite(addr_lo);
        self.observe_a12_addr(bus, addr_hi);
        let mut hi = bus.ppu_read_sprite(addr_hi);
        // W2 ($2007 Stress): stash the RAW (pre-h-flip) pattern bytes so the
        // per-dot sprite-fetch read cadence (`tick_sprite_fetch_read`) can
        // feed `render_data_bus` for the deferred `$2007` PPUDATA reload.
        {
            self.spr_fetch_lo_raw[slot] = lo;
            self.spr_fetch_hi_raw[slot] = hi;
        }
        // v2.0 (ppu-sprite-shifter-counter): gate the shifter load on the sprite
        // being in-range of `next_line`. On visible scanlines this is a no-op
        // (the eval already guarantees every `in_use` slot is in-range), but on
        // the pre-render line (feature on, `next_line = 5`) it filters the STALE
        // secondary-OAM slots so only sprites whose pixel lands on row 5 load for
        // scanline 0. Default (feature off): load every `in_use` slot.
        let load = in_use && {
            let r = next_line.wrapping_sub(y);
            r >= 0 && r < sprite_height
        };
        if load {
            if flip_h {
                lo = reverse_bits(lo);
                hi = reverse_bits(hi);
            }
            self.spr_shift_lo[slot] = lo;
            self.spr_shift_hi[slot] = hi;
            self.spr_attr[slot] = attr;
            self.spr_x[slot] = xpos;
            // v1.2.0 C3 (hd-pack): stash the 16-byte tile base (in-tile row
            // masked off) for this sprite slot so `emit_pixel` can name the
            // CHR tile. Telemetry only; no new VRAM read here.
            #[cfg(feature = "hd-pack")]
            {
                self.hd_spr_addr[slot] = addr_lo & 0x1FF0;
                self.hd_spr_x[slot] = xpos;
                // `in_tile_row` is the post-flip-V fetch row, i.e. exactly the
                // (unflipped) replacement texel row to sample.
                self.hd_spr_off_y[slot] = u8::try_from(in_tile_row & 0x07).unwrap_or(0);
                // CHR-ROM absolute tile index for the sprite, or the CHR-RAM
                // sentinel (the common mappers share BG/sprite CHR banking).
                self.hd_spr_idx[slot] = bus.chr_phys(addr_lo).map_or(HD_CHR_RAM, |o| o / 16);
            }
        } else {
            #[cfg(feature = "hd-pack")]
            {
                self.hd_spr_addr[slot] = HD_TILE_NONE;
            }
        }
        // Else: shift regs already cleared in tick_sprite_eval_per_dot.
    }

    fn advance_dot(&mut self) {
        // Count every PPU master cycle (one per dot processed) for the NES_NTSC
        // colour phase. Output-only / cosmetic; never gates emulation.
        self.dot_counter = self.dot_counter.wrapping_add(1);

        // Odd-frame skip: when the frame is odd and rendering is enabled,
        // the pre-render scanline 261 dot 339 transitions to (0, 0)
        // immediately, skipping dot 340.
        //
        // The rendering check reads `mask_for_skip_check` (two-stage
        // pipeline of `mask`, shifted at the bottom of this function), not
        // `mask` directly. The two-PPU-clock visibility delay between a
        // `$2001` write and this check is what makes blargg
        // `ppu_vbl_nmi/10-even_odd_timing` pass: lockstep applies the
        // PPUMASK write at the *start* of a CPU cycle, while real hardware
        // latches at φ2 (end of cycle). Without the delay the dot-339 skip
        // detector observes the write up to two PPU clocks earlier than
        // hardware does, mispredicting the skip when the write straddles
        // dot 339.
        if self.scanline == self.region.prerender_line()
            && self.dot == 339
            && (self.frame & 1) == 1
            && self.mask_for_skip_check.rendering_enabled()
            && self.region == PpuRegion::Ntsc
        {
            self.dot = 0;
            self.scanline = 0;
            self.frame = self.frame.wrapping_add(1);
            self.frame_complete = true;
            self.snapshot_ntsc_phase();
            self.mask_for_skip_check = self.mask_skip_pipe1;
            self.mask_skip_pipe1 = self.mask;
            return;
        }

        self.dot += 1;
        if self.dot > 340 {
            self.dot = 0;
            // Advance scanline.
            if self.scanline == self.region.prerender_line() {
                self.scanline = 0;
                self.frame = self.frame.wrapping_add(1);
                self.frame_complete = true;
                self.snapshot_ntsc_phase();
            } else if self.extra_scanlines != 0 && self.scanline + 1 == self.region.prerender_line()
            {
                // v1.7.0 F3 — PPU extra-scanlines overclock. The line just
                // before pre-render is a pure idle vblank line (not visible,
                // not the VBL-set line, not pre-render): repeating it emits no
                // pixels, sets/clears no flags, and fires no VBL/NMI/A12 event
                // — it only adds CPU run-time (the surrounding scheduler still
                // clocks the CPU every third dot). When the counter is exhausted
                // we fall through to the pre-render line as usual. This whole
                // branch is unreachable while `extra_scanlines == 0`, so the
                // default build is byte-identical.
                if self.extra_lines_remaining == 0 {
                    self.extra_lines_remaining = self.extra_scanlines;
                }
                self.extra_lines_remaining -= 1;
                if self.extra_lines_remaining == 0 {
                    // Done inserting: advance to pre-render as normal.
                    self.scanline += 1;
                }
                // else: hold on this idle line and run it again.
            } else {
                self.scanline += 1;
            }
        }
        self.mask_for_skip_check = self.mask_skip_pipe1;
        self.mask_skip_pipe1 = self.mask;
    }

    const fn is_render_scanline(&self) -> bool {
        // Visible (0..=239) and pre-render line.
        self.scanline >= 0 && self.scanline <= self.region.last_visible_line()
            || self.scanline == self.region.prerender_line()
    }
}

/// Resolve an address in `$3F00-$3FFF` to a palette RAM index, applying the
/// `$3F10/$14/$18/$1C → $3F00/$04/$08/$0C` mirror.
const fn palette_index(addr: u16) -> usize {
    let mut idx = (addr & 0x1F) as usize;
    if matches!(idx, 0x10 | 0x14 | 0x18 | 0x1C) {
        idx -= 0x10;
    }
    idx
}

/// Reverse the bit order of a byte (used for horizontally-flipped sprites).
const fn reverse_bits(b: u8) -> u8 {
    b.reverse_bits()
}

#[cfg(test)]
mod tests {
    use super::*;

    // T-73-005 / T-73-006 (Phase 7): pin the per-region timing table so an
    // accidental edit to a region constant trips a test instead of silently
    // mis-timing PAL/Dendy. The runtime frame-structure consequences are
    // gated by the integration test in
    // `crates/rustynes-test-harness/tests/region_timing.rs`.
    #[test]
    fn ppu_region_constants_match_hardware() {
        // NTSC: 262 lines (pre-render 261), VBL@241, no odd-frame skip caveat.
        assert_eq!(PpuRegion::Ntsc.prerender_line(), 261);
        assert_eq!(PpuRegion::Ntsc.vblank_start_line(), 241);
        assert_eq!(PpuRegion::Ntsc.post_reset_mask_cycles(), 29_658);
        // PAL: 312 lines (pre-render 311), VBL@241, longer reset mask.
        assert_eq!(PpuRegion::Pal.prerender_line(), 311);
        assert_eq!(PpuRegion::Pal.vblank_start_line(), 241);
        assert_eq!(PpuRegion::Pal.post_reset_mask_cycles(), 33_132);
        // Dendy: 312 lines, but VBL starts at 291 (the distinguishing trait).
        assert_eq!(PpuRegion::Dendy.prerender_line(), 311);
        assert_eq!(PpuRegion::Dendy.vblank_start_line(), 291);
        assert_eq!(PpuRegion::Dendy.post_reset_mask_cycles(), 33_132);
        // Last visible line is 239 in every region.
        for r in [PpuRegion::Ntsc, PpuRegion::Pal, PpuRegion::Dendy] {
            assert_eq!(r.last_visible_line(), 239);
        }
    }

    #[test]
    fn odd_frame_dot_skip_is_ntsc_only() {
        // The pre-render dot-339 odd-frame skip only fires on NTSC with
        // rendering enabled. Drive a rendering-enabled odd pre-render frame in
        // each region and confirm only NTSC collapses dot 340.
        fn skips(region: PpuRegion) -> bool {
            let mut ppu = Ppu::new(region);
            // Force an odd frame, rendering on, parked at pre-render dot 339.
            ppu.frame = 1;
            ppu.mask = PpuMask::SHOW_BG;
            ppu.mask_for_skip_check = PpuMask::SHOW_BG;
            ppu.scanline = region.prerender_line();
            ppu.dot = 339;
            ppu.advance_dot();
            // A skip lands us at (scanline 0, dot 0); no skip steps to dot 340.
            ppu.scanline == 0 && ppu.dot == 0
        }
        assert!(skips(PpuRegion::Ntsc), "NTSC odd frame skips dot 340");
        assert!(!skips(PpuRegion::Pal), "PAL never skips");
        assert!(!skips(PpuRegion::Dendy), "Dendy never skips");
    }

    /// Test bus that owns 8 KiB of CHR-RAM with horizontal mirroring map.
    /// CIRAM lives in the PPU; this bus only services CHR + A12.
    struct TestBus {
        chr: [u8; 0x2000],
        a12_count: u32,
        last_a12: bool,
    }

    impl TestBus {
        fn new() -> Self {
            Self {
                chr: [0u8; 0x2000],
                a12_count: 0,
                last_a12: false,
            }
        }
    }

    impl PpuBus for TestBus {
        fn ppu_read(&mut self, addr: u16) -> u8 {
            if addr < 0x2000 {
                self.chr[addr as usize]
            } else {
                0
            }
        }
        fn ppu_write(&mut self, addr: u16, value: u8) {
            if addr < 0x2000 {
                self.chr[addr as usize] = value;
            }
        }
        fn notify_a12(&mut self, level: bool) {
            if level != self.last_a12 {
                self.a12_count += 1;
                self.last_a12 = level;
            }
        }
        fn nametable_address(&self, addr: u16) -> u16 {
            // Horizontal mirroring: tables 0/1 -> bank 0, 2/3 -> bank 1.
            let table = ((addr.wrapping_sub(0x2000)) / 0x0400) & 0x03;
            let local = addr & 0x03FF;
            let phys = u16::from(table >= 2);
            phys * 0x0400 + local
        }
    }

    fn fresh_ppu() -> (Ppu, TestBus) {
        let mut ppu = Ppu::new(PpuRegion::Ntsc);
        // Drive past the post-reset masking window.
        ppu.post_reset_mask_remaining = 0;
        (ppu, TestBus::new())
    }

    // F1.1 (Fathom accuracy remediation) — palette backdrop-override.
    // When rendering is disabled and the VRAM address `v` points into palette
    // space ($3F00-$3FFF), the palette's shared address input is driven by `v`,
    // so the PPU outputs the color at `v & 0x1F` INSTEAD of the universal
    // backdrop ($3F00). This is a display artifact only — palette RAM is never
    // mutated, and rendering-enabled output is unchanged. See NESdev "PPU
    // palettes"; mirrors Mesen2 `NesPpu.cpp` / ares output-stage behavior.
    #[test]
    fn palette_backdrop_override_when_rendering_disabled() {
        let (mut p, _b) = fresh_ppu();
        p.mask = PpuMask::empty(); // rendering disabled
        p.palette_ram[palette_index(0x3F00)] = 0x0F; // backdrop
        p.palette_ram[palette_index(0x3F05)] = 0x16; // override target (red)
        p.scanline = 10;
        p.dot = 20; // pixel_x = 19 (visible)
        let off = (10usize * 256 + 19) * 4;
        let red = crate::palette::nes_color_to_rgba(0x16);
        let backdrop = crate::palette::nes_color_to_rgba(0x0F);

        // v in palette range, rendering off -> output palette[v & 0x1F].
        p.v = 0x3F05;
        p.emit_pixel();
        assert_eq!(&p.framebuffer[off..off + 4], &red, "override -> palette[5]");

        // v NOT in palette range, rendering off -> universal backdrop.
        p.v = 0x2000;
        p.emit_pixel();
        assert_eq!(&p.framebuffer[off..off + 4], &backdrop, "non-palette v");

        // Rendering ENABLED with transparent BG -> backdrop, never overridden
        // (the fetch pipeline owns `v` while rendering).
        p.mask = PpuMask::SHOW_BG | PpuMask::SHOW_BG_LEFT;
        p.v = 0x3F05;
        p.emit_pixel();
        assert_eq!(
            &p.framebuffer[off..off + 4],
            &backdrop,
            "enabled -> no override"
        );

        // $3F10 mirrors to $3F00 (universal backdrop), not a distinct entry.
        p.mask = PpuMask::empty();
        p.v = 0x3F10;
        p.emit_pixel();
        assert_eq!(
            &p.framebuffer[off..off + 4],
            &backdrop,
            "$3F10 mirrors backdrop"
        );

        // The override is display-only: palette RAM is unmodified.
        assert_eq!(p.palette_ram[palette_index(0x3F05)], 0x16);
    }

    // F1.2 (Fathom) — OAM / $2004 quirks. Both behaviors below are already
    // implemented and covered by the AccuracyCoin `$2004`/`Sprite0Hit` ROMs;
    // this is a FAST regression guard so an edit trips a unit test instead of
    // only the ~57s ROM battery. (The `OAMADDR & 0xF8` render-start copy is NOT
    // modeled on the DEFAULT revision — Mesen2, ares, and TriCNES all omit it as
    // a revision-dependent, oracle-less corner. As of v2.1.7 P5 the related
    // OAMADDR `$2003` write-during-render corruption is available as an opt-in
    // `PpuRevision::Rp2c02G` model; see `docs/accuracy-ledger.md`.)
    #[test]
    fn oam_2004_attribute_mask_and_oamaddr_257_320_forcing() {
        // (1) $2004 read of a sprite ATTRIBUTE byte (OAM offset & 3 == 2) masks
        // bits 4-2 with $E3 (they don't exist in OAM); other bytes are unmasked.
        // Read outside the rendering windows so the plain OAM path is taken.
        let (mut p, mut b) = fresh_ppu();
        p.mask = PpuMask::empty(); // rendering disabled
        p.scanline = 250; // vblank -> not a render scanline
        p.oam[2] = 0xFF; // attribute byte
        p.oam[1] = 0xFF; // tile byte (no mask)
        p.oam_addr = 2;
        assert_eq!(p.cpu_read_register(4, &mut b), 0xE3, "attr byte $E3-masked");
        p.oam_addr = 1;
        assert_eq!(
            p.cpu_read_register(4, &mut b),
            0xFF,
            "non-attr byte unmasked"
        );

        // (2) OAMADDR is forced to 0 across dots 257-320 of a rendered scanline
        // (the sprite-tile-load interval), washing away a perturbed value.
        let (mut p, mut b) = fresh_ppu();
        p.mask = PpuMask::SHOW_BG | PpuMask::SHOW_SPRITE;
        p.scanline = 10; // visible render line
        p.dot = 256;
        p.oam_addr = 0x40; // perturbed; nothing but the 257-320 wash zeroes it
        for _ in 0..70 {
            p.tick(&mut b); // dot 256 -> 326, through the whole window
        }
        assert_eq!(p.oam_addr, 0, "OAMADDR washed to 0 across dots 257-320");
    }

    // v2.1.4 F2.3 — optional OAM decay (opt-in, default-OFF). Models Mesen2's
    // `ReadSpriteRam`: a row un-refreshed for > OAM_DECAY_CPU_CYCLES CPU cycles
    // decays to `((sprAddr & 3) == 2) ? (sprAddr & 0xE3) : sprAddr` on the next
    // read. The read path used here is the plain (non-rendering) `$2004` read —
    // `mask` empty + a vblank scanline keeps out of the rendering / dot-1-64 /
    // dot-257-320 forcing windows.
    #[test]
    fn oam_decay_disabled_by_default_leaves_oam_untouched() {
        let (mut p, mut b) = fresh_ppu();
        p.mask = PpuMask::empty();
        p.scanline = 250; // vblank — plain OAM read path
        // Seed a distinctive value in row 0 (bytes 0..8).
        for i in 0..8u8 {
            p.oam[i as usize] = 0xAA;
        }
        // Advance the clock WELL past the decay window with no OAM access.
        p.dot_counter = (OAM_DECAY_CPU_CYCLES + 10_000) * 3;
        // Default is disabled — a read must return the seeded byte, and OAM must
        // be byte-for-byte unchanged (no decay pattern written).
        p.oam_addr = 0;
        assert!(!p.oam_decay_enabled(), "decay off by default");
        assert_eq!(
            p.cpu_read_register(4, &mut b),
            0xAA,
            "no decay when disabled"
        );
        for i in 0..8usize {
            assert_eq!(p.oam[i], 0xAA, "OAM row untouched when decay disabled");
        }
    }

    #[test]
    fn oam_decay_enabled_decays_stale_row_to_mesen_pattern() {
        let (mut p, mut b) = fresh_ppu();
        p.set_oam_decay(true);
        p.mask = PpuMask::empty();
        p.scanline = 250;
        // Seed row 3 (OAM $18..$20) with a value distinct from the decay pattern.
        for a in 0x18u8..0x20 {
            p.oam[a as usize] = 0x5A;
        }
        // `set_oam_decay(true)` re-based the timestamps to the then-current cycle
        // (0). Advance PAST the window so the row is stale on the next read.
        p.dot_counter = (OAM_DECAY_CPU_CYCLES + 1) * 3;
        // Read byte $1A (an attribute byte: $1A & 3 == 2) — the whole row decays
        // first, then the (now-decayed) byte is returned. Expected decay byte for
        // $1A = $1A & 0xE3 = $02; but `$2004` additionally $E3-masks an attr byte
        // on the way out ($02 & $E3 == $02), so the observed value is $02.
        p.oam_addr = 0x1A;
        let v = p.cpu_read_register(4, &mut b);
        assert_eq!(v, 0x1A & 0xE3, "attribute byte decays to sprAddr & 0xE3");
        // The full row now holds the canonical pattern.
        for a in 0x18u8..0x20 {
            let expect = if a & 0x03 == 0x02 { a & 0xE3 } else { a };
            assert_eq!(p.oam[a as usize], expect, "row byte ${a:02X} decayed");
        }
    }

    #[test]
    fn oam_decay_access_within_window_refreshes_row() {
        let (mut p, mut b) = fresh_ppu();
        p.set_oam_decay(true);
        p.mask = PpuMask::empty();
        p.scanline = 250;
        for a in 0x18u8..0x20 {
            p.oam[a as usize] = 0x5A;
        }
        // Touch the row just before the window closes (elapsed == threshold →
        // still a refresh, not a decay), which re-stamps the timestamp.
        p.dot_counter = OAM_DECAY_CPU_CYCLES * 3;
        p.oam_addr = 0x18;
        assert_eq!(
            p.cpu_read_register(4, &mut b),
            0x5A,
            "in-window read: no decay"
        );
        // Advance another (threshold) cycles from the refresh point — still within
        // the window relative to the refreshed timestamp, so no decay.
        p.dot_counter += OAM_DECAY_CPU_CYCLES * 3;
        p.oam_addr = 0x19;
        assert_eq!(
            p.cpu_read_register(4, &mut b),
            0x5A,
            "refresh kept row alive"
        );
        for a in 0x18u8..0x20 {
            assert_eq!(p.oam[a as usize], 0x5A, "row still holds seeded data");
        }
    }

    #[test]
    fn oam_decay_is_pal_disabled() {
        // PAL's frequent refresh cadence masks decay, so the model never acts
        // there even when enabled (matches Mesen2). Same stale-row setup as the
        // NTSC decay test, but on PAL the read must NOT decay.
        let mut p = Ppu::new(PpuRegion::Pal);
        p.post_reset_mask_remaining = 0;
        let mut b = TestBus::new();
        p.set_oam_decay(true);
        p.mask = PpuMask::empty();
        p.scanline = 250;
        for a in 0x18u8..0x20 {
            p.oam[a as usize] = 0x5A;
        }
        p.dot_counter = (OAM_DECAY_CPU_CYCLES + 1) * 3;
        p.oam_addr = 0x18;
        assert_eq!(p.cpu_read_register(4, &mut b), 0x5A, "PAL: decay disabled");
        for a in 0x18u8..0x20 {
            assert_eq!(p.oam[a as usize], 0x5A, "PAL row untouched");
        }
    }

    #[test]
    fn oam_decay_write_refreshes_row() {
        let (mut p, mut b) = fresh_ppu();
        p.set_oam_decay(true);
        p.mask = PpuMask::empty();
        p.scanline = 250;
        for a in 0x18u8..0x20 {
            p.oam[a as usize] = 0x5A;
        }
        // A `$2004` write of row 3 well past the window still refreshes it, so a
        // subsequent in-window read of that row does not decay.
        p.dot_counter = (OAM_DECAY_CPU_CYCLES + 5_000) * 3;
        p.oam_addr = 0x18;
        p.cpu_write_register(4, 0x33, &mut b); // writes $18, refreshes row 3
        // Read $19 (same row) a short time later — within the window of the write.
        p.dot_counter += 10 * 3;
        p.oam_addr = 0x19;
        assert_eq!(p.cpu_read_register(4, &mut b), 0x5A, "write kept row alive");
    }

    // v2.1.7 P5 — PPU revision + power-up palette (opt-in, default-off).

    #[test]
    fn revision_defaults_to_rp2c02h_no_corruption() {
        let (p, _b) = fresh_ppu();
        assert_eq!(p.revision(), PpuRevision::Rp2c02H, "default revision");
        assert!(
            !p.revision().models_oamaddr_corruption(),
            "default revision models no OAMADDR corruption"
        );
    }

    #[test]
    fn default_revision_2003_write_during_render_does_not_corrupt() {
        // On the default revision a $2003 write mid-render must NOT arm any OAM
        // corruption — the byte-identity guarantee.
        let (mut p, mut b) = fresh_ppu();
        p.mask = PpuMask::SHOW_BG | PpuMask::SHOW_SPRITE;
        p.scanline = 10; // visible render line
        // Distinct row-0 vs row-1 so a spurious copy would be observable.
        for i in 0..8u8 {
            p.oam[i as usize] = 0x11;
            p.oam[8 + i as usize] = 0x22;
        }
        p.cpu_write_register(3, 0x08, &mut b); // OAMADDR = row 1
        assert!(
            !p.oam_corruption_pending,
            "default revision: no corruption armed"
        );
    }

    #[test]
    fn rp2c02g_2003_write_during_render_arms_and_corrupts_row() {
        // On the earlier `Rp2c02G` revision a $2003 write while rendering is
        // active arms the row-copy corruption; committing it copies row 0 over
        // the targeted row (index = value >> 3).
        let (mut p, mut b) = fresh_ppu();
        p.set_revision(PpuRevision::Rp2c02G);
        p.mask = PpuMask::SHOW_BG | PpuMask::SHOW_SPRITE;
        p.scanline = 10; // visible render line
        for i in 0..8u8 {
            p.oam[i as usize] = 0x11; // row 0
            p.oam[8 + i as usize] = 0x22; // row 1 (target)
        }
        p.cpu_write_register(3, 0x08, &mut b); // OAMADDR = 0x08 → row index 1
        assert!(p.oam_corruption_pending, "Rp2c02G: corruption armed");
        assert_eq!(p.oam_corruption_index, 1, "targets row 1");
        // Commit and verify row 1 now mirrors row 0.
        p.process_oam_corruption();
        for i in 0..8u8 {
            assert_eq!(
                p.oam[8 + i as usize],
                0x11,
                "row 1 byte {i} corrupted from row 0"
            );
        }
    }

    #[test]
    fn rp2c02g_2003_write_outside_render_does_not_corrupt() {
        // Even on the corrupting revision, a $2003 write with rendering disabled
        // (or in vblank) must NOT arm corruption — the glitch is render-gated.
        let (mut p, mut b) = fresh_ppu();
        p.set_revision(PpuRevision::Rp2c02G);
        p.mask = PpuMask::empty(); // rendering disabled
        p.scanline = 250; // vblank
        p.cpu_write_register(3, 0x08, &mut b);
        assert!(
            !p.oam_corruption_pending,
            "Rp2c02G but no rendering: no corruption"
        );
    }

    #[test]
    fn power_up_palette_defaults_zeroed() {
        let (p, _b) = fresh_ppu();
        assert_eq!(p.power_up_palette(), PaletteInit::Zeroed, "default palette");
        assert_eq!(
            p.palette_ram, [0u8; 32],
            "default power-up palette all-zero"
        );
    }

    #[test]
    fn power_up_palette_blargg_applies_masked_pattern() {
        let (mut p, _b) = fresh_ppu();
        p.apply_power_up_palette(PaletteInit::Blargg);
        assert_eq!(p.power_up_palette(), PaletteInit::Blargg);
        // Byte 0 = 0x09, an attr-index that survives the 6-bit mask untouched.
        assert_eq!(p.palette_ram[0], 0x09, "Blargg byte 0");
        // Every cell must be 6-bit masked (matching a `$2007` write path).
        for (i, &b) in p.palette_ram.iter().enumerate() {
            assert_eq!(b, BLARGG_POWER_UP_PALETTE[i] & 0x3F, "cell {i} masked");
        }
        // Re-applying Zeroed restores the byte-identical default state.
        p.apply_power_up_palette(PaletteInit::Zeroed);
        assert_eq!(p.palette_ram, [0u8; 32], "re-zeroed");
    }

    // F1.3 (Fathom) — PPU open-bus refresh map. The Blargg `ppu_open_bus` table
    // is: a read DRIVES (and refreshes) some bits and passes others through from
    // the decay latch. $2000-$2003/$2005/$2006 = all decay; $2004 + $2007
    // (non-palette) = all driven; $2002 = `---D DDDD` (bits 7-5 driven); $2007
    // palette = `DD-- ----` (bits 7-6 decay). The $2002 low-5 case is covered by
    // `ppustatus_*` above; this locks the $2007-palette and write-only cases.
    #[test]
    fn open_bus_refresh_map_2007_palette_and_write_only() {
        // Reading a WRITE-ONLY register drives no bits -> the full decay latch.
        let (mut p, mut b) = fresh_ppu();
        p.open_bus = 0xA5;
        assert_eq!(
            p.cpu_read_register(0, &mut b),
            0xA5,
            "$2000 read = pure open bus"
        );

        // $2007 PALETTE read drives bits 5-0 (palette) and passes bits 7-6 from
        // open bus (Blargg map: palette = `DD-- ----`).
        let (mut p, mut b) = fresh_ppu();
        p.open_bus = 0xFF; // bits 7-6 set
        p.mask = PpuMask::empty(); // no render-window $FF path
        p.v = 0x3F00;
        p.palette_ram[palette_index(0x3F00)] = 0x15;
        assert_eq!(
            p.cpu_read_register(7, &mut b),
            0x15 | 0xC0,
            "$2007 palette: bits 5-0 palette, 7-6 open bus"
        );
    }

    #[test]
    fn ppustatus_read_clears_vbl_and_w() {
        let (mut p, mut b) = fresh_ppu();
        p.status.insert(PpuStatus::VBLANK);
        p.w = true;
        let v = p.cpu_read_register(2, &mut b);
        assert!(v & 0x80 != 0, "VBL should have been set on read");
        assert!(!p.status.contains(PpuStatus::VBLANK));
        assert!(!p.w);
    }

    #[test]
    fn default_ppu_uses_composite_palette_no_2c05() {
        let (p, _b) = fresh_ppu();
        assert_eq!(p.active_palette, crate::palette::PpuPalette::Composite2C02);
        assert!(!p.is_2c05);
        // map_register is the identity on a non-2C05 PPU.
        for r in 0u8..8 {
            assert_eq!(p.map_register(r), r);
        }
    }

    #[test]
    fn c2c05_swaps_2000_and_2001() {
        let (mut p, mut b) = fresh_ppu();
        p.set_palette(crate::palette::PpuPalette::Rgb2C05, true, 0x3D);
        // A write to $2000 (reg 0) on a 2C05 sets MASK; a write to $2001 sets
        // CTRL. Use a distinct, register-valid value for each.
        // PPUMASK bit 3 = SHOW_BG. PPUCTRL bit 7 = NMI_ENABLE.
        p.cpu_write_register(0, 0b0000_1000, &mut b); // -> MASK SHOW_BG
        assert!(p.mask.contains(PpuMask::SHOW_BG), "$2000 write set MASK");
        assert!(p.ctrl.is_empty(), "$2000 write did NOT touch CTRL");

        let (mut p2, mut b2) = fresh_ppu();
        p2.set_palette(crate::palette::PpuPalette::Rgb2C05, true, 0x3D);
        p2.cpu_write_register(1, 0b1000_0000, &mut b2); // -> CTRL NMI_ENABLE
        assert!(
            p2.ctrl.contains(PpuCtrl::NMI_ENABLE),
            "$2001 write set CTRL on a 2C05"
        );
        assert!(p2.mask.is_empty(), "$2001 write did NOT touch MASK");
    }

    #[test]
    fn c2c05_2002_returns_identifier_in_low_bits() {
        let (mut p, mut b) = fresh_ppu();
        p.set_palette(crate::palette::PpuPalette::Rgb2C05, true, 0x3D);
        // Set the VBL flag so the high bits are deterministic.
        p.status.insert(PpuStatus::VBLANK);
        let v = p.cpu_read_register(2, &mut b);
        // 2C05-02 id = $3D; low 5 bits => $3D & $1F = $1D.
        assert_eq!(v & 0x1F, 0x3D & 0x1F);
        assert!(v & 0x80 != 0, "VBL still reported in bit 7");
    }

    #[test]
    fn non_2c05_2002_keeps_open_bus_low_bits() {
        // Without is_2c05, the low 5 bits remain open-bus (byte-identical to
        // the legacy path).
        let (mut p, mut b) = fresh_ppu();
        p.cpu_write_register(3, 0x1F, &mut b); // load open bus with $1F
        let v = p.cpu_read_register(2, &mut b);
        assert_eq!(v & 0x1F, 0x1F);
    }

    #[test]
    fn ppustatus_low_5_bits_are_open_bus() {
        let (mut p, mut b) = fresh_ppu();
        // Touch the open-bus latch via a $2003 write.
        p.cpu_write_register(3, 0xAB, &mut b);
        p.status.insert(PpuStatus::VBLANK);
        let v = p.cpu_read_register(2, &mut b);
        // Bits 7-5 from status (only VBL set), bits 4-0 from open-bus (0x0B).
        assert_eq!(v & 0xE0, 0x80);
        assert_eq!(v & 0x1F, 0xAB & 0x1F);
    }

    #[test]
    fn ppustatus_read_preserves_low_5_bits_of_open_bus_latch() {
        // Reading $2002 only refreshes the upper 3 bits of the open-bus
        // latch (the bits sourced from PPUSTATUS); the lower 5 bits must
        // retain their previous value.  Required by the `open_bus_read_test`
        // sub-routine of `cpu_dummy_writes_ppumem.nes` (Bisqwit), which
        // performs `lda $2002; eor $2000` and expects the result to be 0
        // after AND-masking with 0x1F.
        let (mut p, mut b) = fresh_ppu();
        // Seed the open-bus latch via a $2003 write; pick a value with low
        // bits set so the bug-fix is observable.
        p.cpu_write_register(3, 0xAB, &mut b);
        p.status.insert(PpuStatus::VBLANK);
        // Read $2002 — should expose status high bits + latch low 5 bits.
        let v = p.cpu_read_register(2, &mut b);
        assert_eq!(v, 0x80 | (0xAB & 0x1F));
        // Now read $2000 (write-only): should return the refreshed latch
        // = (status & 0xE0) | (old_latch & 0x1F) — i.e., the same value.
        let after = p.cpu_read_register(0, &mut b);
        assert_eq!(
            after, v,
            "$2002 read must refresh only the high 3 bits of open-bus; \
             the low 5 bits must survive into subsequent reads of \
             write-only ports"
        );
    }

    #[test]
    fn ppudata_buffered_read_returns_previous_byte() {
        let (mut p, mut b) = fresh_ppu();
        // CIRAM lives in the PPU now.
        p.ciram[0] = 0xAB;
        p.ciram[1] = 0xCD;
        // Set v to $2000.
        p.cpu_write_register(6, 0x20, &mut b);
        p.cpu_write_register(6, 0x00, &mut b);
        // First read: returns buffer (0), refills from $2000.
        let r1 = p.cpu_read_register(7, &mut b);
        assert_eq!(r1, 0);
        // Second read: returns refill (0xAB), refills with next byte.
        let r2 = p.cpu_read_register(7, &mut b);
        assert_eq!(r2, 0xAB);
        let r3 = p.cpu_read_register(7, &mut b);
        assert_eq!(r3, 0xCD);
    }

    #[test]
    fn ppudata_palette_read_bypasses_buffer() {
        let (mut p, mut b) = fresh_ppu();
        p.palette_ram[0] = 0x12;
        // Stash a different value in the underlying nametable mirror so we
        // see the buffer get the underlying value, not the palette byte.
        p.ciram[0] = 0xCC;
        // Set v to $3F00.
        p.cpu_write_register(6, 0x3F, &mut b);
        p.cpu_write_register(6, 0x00, &mut b);
        let r = p.cpu_read_register(7, &mut b);
        // High 2 bits open-bus. Low 6 bits: 0x12.
        assert_eq!(r & 0x3F, 0x12);
        // Buffer should now contain underlying nametable mirror at $2F00
        // (= $3F00 & $2FFF), via horizontal mirroring tables 2/3 -> bank 1.
    }

    #[test]
    fn ppudata_increment_1_or_32() {
        let (mut p, mut b) = fresh_ppu();
        p.cpu_write_register(6, 0x21, &mut b);
        p.cpu_write_register(6, 0x00, &mut b);
        // Increment by 1 default.
        p.cpu_read_register(7, &mut b);
        assert_eq!(p.v & 0x7FFF, 0x2101);
        // Switch to increment 32.
        p.cpu_write_register(0, PpuCtrl::VRAM_INCREMENT_32.bits(), &mut b);
        p.cpu_read_register(7, &mut b);
        assert_eq!(p.v & 0x7FFF, 0x2121);
    }

    #[test]
    fn ppuctrl_post_reset_mask_window_blocks_writes() {
        let mut p = Ppu::new(PpuRegion::Ntsc);
        // Don't override post_reset_mask_remaining — it's the documented
        // count.
        let mut b = TestBus::new();
        p.cpu_write_register(0, PpuCtrl::NMI_ENABLE.bits(), &mut b);
        assert!(
            !p.ctrl.contains(PpuCtrl::NMI_ENABLE),
            "PPUCTRL write must be ignored during post-reset window"
        );
        // Drive past the window.
        for _ in 0..30_000 {
            p.on_cpu_cycle();
        }
        p.cpu_write_register(0, PpuCtrl::NMI_ENABLE.bits(), &mut b);
        assert!(p.ctrl.contains(PpuCtrl::NMI_ENABLE));
    }

    #[test]
    fn ppuctrl_nmi_enable_during_vbl_asserts_nmi_immediately() {
        let (mut p, mut b) = fresh_ppu();
        p.status.insert(PpuStatus::VBLANK);
        // NMI not yet enabled => line low.
        assert!(!p.nmi_line);
        p.cpu_write_register(0, PpuCtrl::NMI_ENABLE.bits(), &mut b);
        assert!(p.nmi_line);
    }

    #[test]
    fn ppuscroll_two_writes_load_t_and_x() {
        let (mut p, mut b) = fresh_ppu();
        p.cpu_write_register(5, 0b1010_1011, &mut b); // X = 0xAB
        // t bits 4-0 = X[7:3] = 0b10101 = 0x15. x = X[2:0] = 0b011 = 0x03.
        assert_eq!(p.t & 0x001F, 0x15);
        assert_eq!(p.x, 0x03);
        assert!(p.w);
        p.cpu_write_register(5, 0b0101_1100, &mut b); // Y = 0x5C
        // t bits 14-12 = Y[2:0] = 0b100, t bits 9-5 = Y[7:3] = 0b01011.
        assert_eq!((p.t >> 12) & 0x07, 0x04);
        assert_eq!((p.t >> 5) & 0x1F, 0x0B);
        assert!(!p.w);
    }

    #[test]
    fn ppuaddr_two_writes_copy_t_to_v() {
        let (mut p, mut b) = fresh_ppu();
        p.cpu_write_register(6, 0x3F, &mut b); // high
        // After first write t bits 13-8 = 0x3F & 0x3F; bit 14 cleared.
        assert_eq!((p.t >> 8) & 0x7F, 0x3F);
        assert!(p.w);
        p.cpu_write_register(6, 0x10, &mut b); // low; copy t to v
        assert_eq!(p.v, 0x3F10);
        assert!(!p.w);
    }

    #[test]
    fn vbl_set_and_nmi_at_scanline_241_dot_1() {
        let (mut p, mut b) = fresh_ppu();
        p.cpu_write_register(0, PpuCtrl::NMI_ENABLE.bits(), &mut b);
        // Tick until scanline 241 dot 1.
        // Starting at pre-render dot 0 (after construction we set scanline
        // = prerender_line, dot = 0). Tick advances first. We need to
        // reach scanline 241 dot 1. Simplest: just tick enough.
        let mut saw_nmi = false;
        for _ in 0..(341 * 263) {
            p.tick(&mut b);
            if p.nmi_line {
                saw_nmi = true;
                break;
            }
        }
        assert!(saw_nmi, "NMI must assert during VBlank");
        assert!(p.status.contains(PpuStatus::VBLANK));
    }

    #[test]
    fn frame_complete_latch_fires_once_per_frame() {
        let (mut p, mut b) = fresh_ppu();
        // Tick a full frame's worth.
        let mut frames_seen = 0;
        for _ in 0..(341 * 262 * 2) {
            p.tick(&mut b);
            if p.take_frame_complete() {
                frames_seen += 1;
            }
        }
        assert!(frames_seen >= 2);
    }

    #[test]
    fn index_framebuffer_mirrors_rgba_output() {
        // T-110-A1: the parallel palette-index framebuffer must be a faithful
        // index-space mirror of the RGBA framebuffer — for every emitted pixel,
        // `rgba_lut[index] == framebuffer[pixel]`. This is the contract that
        // makes the index buffer a safe, determinism-neutral output: it carries
        // exactly the LUT index used to produce the displayed RGBA.
        let (mut p, mut b) = fresh_ppu();
        // Enable background rendering so the full visible area is emitted.
        p.cpu_write_register(1, 0x08, &mut b); // PPUMASK: show background
        // Run two full frames so every visible pixel has been written.
        for _ in 0..(341 * 262 * 2) {
            p.tick(&mut b);
        }
        let fb = p.framebuffer();
        let idx = p.index_framebuffer();
        assert_eq!(idx.len(), FRAMEBUFFER_PIXELS);
        for (i, &lut_idx) in idx.iter().enumerate() {
            assert!((lut_idx as usize) < 512, "index in range at pixel {i}");
            let expected = p.rgba_lut[lut_idx as usize];
            assert_eq!(
                &fb[i * 4..i * 4 + 4],
                &expected,
                "pixel {i}: index {lut_idx} must reproduce the RGBA output"
            );
        }
    }

    #[test]
    fn ntsc_phase_in_range_and_crawls() {
        // The per-frame NTSC phase must stay in 0..=2 (NTSC) and visit more than
        // one value across frames (the dot-crawl the filter reproduces).
        let (mut p, mut b) = fresh_ppu();
        p.cpu_write_register(1, 0x08, &mut b); // rendering on (odd-frame skip active)
        let mut seen = [false; 3];
        for _ in 0..(341 * 262 * 8) {
            p.tick(&mut b);
            if p.take_frame_complete() {
                let ph = p.ntsc_phase();
                assert!(ph <= 2, "NTSC phase {ph} must be 0..=2");
                seen[ph as usize] = true;
            }
        }
        let distinct = seen.iter().filter(|&&s| s).count();
        assert!(
            distinct >= 2,
            "phase must crawl across frames (saw {distinct})"
        );
    }

    #[test]
    fn palette_mirrors_3f10_alias_3f00() {
        let (mut p, mut b) = fresh_ppu();
        p.cpu_write_register(6, 0x3F, &mut b);
        p.cpu_write_register(6, 0x10, &mut b); // v = $3F10
        p.cpu_write_register(7, 0x21, &mut b); // write palette
        // The mirror should land at index 0 (= $3F00).
        assert_eq!(p.palette_ram[0], 0x21);
        assert_eq!(p.palette_ram[0x10], 0); // not actually written
    }

    #[test]
    fn oamdata_write_increments_oamaddr() {
        let (mut p, mut b) = fresh_ppu();
        p.oam_addr = 0x40;
        p.cpu_write_register(4, 0xCC, &mut b);
        assert_eq!(p.oam[0x40], 0xCC);
        assert_eq!(p.oam_addr, 0x41);
    }

    /// Diagnostic: with standard MMC3 layout (BG=$0000, sprites=$1000)
    /// and rendering enabled, the PPU should produce exactly 241 A12
    /// rising edges per NTSC frame (240 visible scanlines + 1 pre-render
    /// scanline).  This is what MMC3's IRQ counter clocks on.
    #[test]
    fn a12_rising_edges_match_241_per_ntsc_frame_standard_layout() {
        struct CountingBus {
            chr: [u8; 0x2000],
            rises: u32,
            last_a12: bool,
            // diagnostic: count rises in each phase
            rises_visible: u32,
            rises_prerender: u32,
            cur_scanline_is_pre: bool,
        }
        impl PpuBus for CountingBus {
            fn ppu_read(&mut self, addr: u16) -> u8 {
                if addr < 0x2000 {
                    self.chr[addr as usize]
                } else {
                    0
                }
            }
            fn ppu_write(&mut self, addr: u16, value: u8) {
                if addr < 0x2000 {
                    self.chr[addr as usize] = value;
                }
            }
            fn notify_a12(&mut self, level: bool) {
                if level != self.last_a12 {
                    if level {
                        self.rises += 1;
                        if self.cur_scanline_is_pre {
                            self.rises_prerender += 1;
                        } else {
                            self.rises_visible += 1;
                        }
                    }
                    self.last_a12 = level;
                }
            }
            fn nametable_address(&self, addr: u16) -> u16 {
                let table = ((addr.wrapping_sub(0x2000)) / 0x0400) & 0x03;
                let local = addr & 0x03FF;
                let phys = u16::from(table >= 2);
                phys * 0x0400 + local
            }
        }
        let mut p = Ppu::new(PpuRegion::Ntsc);
        p.post_reset_mask_remaining = 0;
        let mut b = CountingBus {
            chr: [0u8; 0x2000],
            rises: 0,
            last_a12: false,
            rises_visible: 0,
            rises_prerender: 0,
            cur_scanline_is_pre: false,
        };
        // Standard layout: BG=$0000 (PPUCTRL bit 4 = 0),
        //                  sprites=$1000 (PPUCTRL bit 3 = 1).
        p.cpu_write_register(0, PpuCtrl::SPRITE_PATTERN_HIGH.bits(), &mut b);
        // Enable BG + sprite rendering (PPUMASK bits 3 + 4).
        p.cpu_write_register(1, (PpuMask::SHOW_BG | PpuMask::SHOW_SPRITE).bits(), &mut b);

        // Advance past a complete frame.  Reset rise counters at the start of
        // the frame and then tick exactly one NTSC frame (262 scanlines × 341
        // dots — odd-frame skip not triggered because frame counter is 0).
        // First, advance to scanline 0 dot 0.
        while !(p.scanline() == 0 && p.dot() == 0) {
            p.tick(&mut b);
        }
        b.rises = 0;
        b.rises_visible = 0;
        b.rises_prerender = 0;
        b.last_a12 = false;
        // Now run exactly one frame.
        let start_frame = p.frame();
        while p.frame() == start_frame {
            b.cur_scanline_is_pre = p.scanline() == PpuRegion::Ntsc.prerender_line();
            p.tick(&mut b);
        }
        assert_eq!(
            b.rises, 241,
            "expected 241 A12 rises per NTSC frame (240 visible + 1 pre-render), \
             got {} (visible={}, prerender={})",
            b.rises, b.rises_visible, b.rises_prerender
        );
    }

    #[test]
    fn a12_transitions_notify_bus() {
        let (mut p, mut b) = fresh_ppu();
        // Set v to $1234 (A12 high), then $0234 (A12 low) — two transitions.
        p.cpu_write_register(6, 0x12, &mut b);
        p.cpu_write_register(6, 0x34, &mut b);
        assert_eq!(b.a12_count, 1);
        p.cpu_write_register(6, 0x02, &mut b);
        p.cpu_write_register(6, 0x34, &mut b);
        assert_eq!(b.a12_count, 2);
    }

    // -------------------------------------------------------------------
    // T-23-002: sprite-evaluation FSM with buggy n+m overflow increment.
    // -------------------------------------------------------------------

    /// Regression: 8 in-range sprites must populate secondary OAM and
    /// leave `spr_count == 8` without setting the `SPRITE_OVERFLOW` flag,
    /// PROVIDED the diagonal-read scan over the remaining 56 sprites
    /// never lands on an in-range byte. To pin that condition we fill
    /// the entire off-screen OAM region with 0xF0, so every byte the
    /// buggy `n+m` walk could land on reads as y=240 (out of range).
    #[test]
    fn sprite_eval_8_sprites_no_overflow() {
        let (mut p, _b) = fresh_ppu();
        p.scanline = 0;
        // 8 in-range sprites with non-zero, non-conflicting byte values
        // that don't read as "in-range y" if the diagonal walk hits them.
        for i in 0..8 {
            let base = i * 4;
            p.oam[base] = 0; // y = 0 (in range)
            p.oam[base + 1] = 0xF0; // tile (also out of range if read as y)
            p.oam[base + 2] = 0xF0;
            p.oam[base + 3] = 0xF0;
        }
        // Sprites 8..63: every byte = 0xF0 so diagonal read finds nothing.
        for i in 8..64 {
            for j in 0..4 {
                p.oam[i * 4 + j] = 0xF0;
            }
        }
        run_per_dot_fsm(&mut p);
        assert_eq!(p.spr_count, 8, "exactly 8 in-range sprites must fill");
        assert!(
            !p.status.contains(PpuStatus::SPRITE_OVERFLOW),
            "8 sprites + all-off-screen-remainder is not overflow"
        );
    }

    /// The headline case: 9 in-range sprites must set `SPRITE_OVERFLOW`.
    /// On real hardware the buggy `n+m` increment reads the wrong byte
    /// of sprite #9, but here sprite #9 is in-range and its y-byte
    /// (which the diagonal walk reads first at n=9, m=0 if found==8)
    /// is in-range, so the flag fires.
    #[test]
    fn sprite_eval_9_sprites_sets_overflow() {
        let (mut p, _b) = fresh_ppu();
        p.scanline = 0;
        for i in 0..9 {
            let base = i * 4;
            p.oam[base] = 0; // y = 0 (in range)
            p.oam[base + 1] = 0xF0; // tile (out of range as y)
            p.oam[base + 2] = 0xF0;
            p.oam[base + 3] = 0xF0;
        }
        for i in 9..64 {
            for j in 0..4 {
                p.oam[i * 4 + j] = 0xF0;
            }
        }
        run_per_dot_fsm(&mut p);
        assert_eq!(p.spr_count, 8, "secondary OAM holds first 8 only");
        assert!(
            p.status.contains(PpuStatus::SPRITE_OVERFLOW),
            "9 in-range sprites must set overflow"
        );
    }

    /// Empty OAM: no in-range sprites, no overflow.
    #[test]
    fn sprite_eval_empty_oam_no_overflow() {
        let (mut p, _b) = fresh_ppu();
        p.scanline = 0;
        // Every byte off-screen, so the eval pass never finds anything
        // and never enters overflow-detection mode.
        for byte in &mut p.oam {
            *byte = 0xF0;
        }
        run_per_dot_fsm(&mut p);
        assert_eq!(p.spr_count, 0);
        assert!(!p.status.contains(PpuStatus::SPRITE_OVERFLOW));
    }

    /// The buggy `n+m` increment: when 8 sprites have been found, the
    /// overflow-detection FSM reads `OAM[n*4+m].y` and increments BOTH
    /// `n` and `m` together on each iteration. If sprite #9 is OUT of
    /// range but sprite #10's *non-y byte* (which the bug reads as a
    /// y-coordinate) happens to be in-range, the overflow flag will
    /// fire — that's the documented hardware quirk, not a bug in our
    /// FSM.
    ///
    /// Construct a case where:
    /// - Sprites 0..7 are in-range (fill secondary OAM, found = 8).
    /// - Sprite 8's y is far off-screen (y = 0xF0, normal y-read would
    ///   say not-in-range).
    /// - Sprite 9's TILE byte (byte index 1, which the buggy m=1 read
    ///   when n=9 lands on) is set to a value that, interpreted as y,
    ///   would put the sprite on the next scanline.
    ///
    /// With the buggy FSM the overflow flag fires because the diagonal
    /// read finds sprite 9's tile byte (= 0) as a "y" that maps to a
    /// row in-range for an 8-tall sprite. A correct (non-buggy) FSM
    /// reading sprite #8's y first would NOT fire because sprite 8 is
    /// out of range.
    ///
    /// This test pins the buggy behavior; flipping it to non-buggy
    /// would change the assertion direction.
    #[test]
    fn sprite_eval_buggy_n_plus_m_finds_diagonal_overflow() {
        let (mut p, _b) = fresh_ppu();
        p.scanline = 0;
        // Start with the entire OAM off-screen.
        for byte in &mut p.oam {
            *byte = 0xF0;
        }
        // Sprites 0..7 in-range with all non-y bytes off-screen.
        for i in 0..8 {
            let base = i * 4;
            p.oam[base] = 0; // y = 0 (in range)
            // bytes 1,2,3 keep the 0xF0 fill so a stray read
            // doesn't mis-fire the diagonal test.
        }
        // Sprite 8 y is 0xF0 (from the bulk fill) — out of range.
        // Sprite 9 tile byte (OAM[9*4+1]) is the second diagonal read
        // target (after sprite 8's y). Setting it to 0 (= in-range y)
        // forces the buggy FSM to fire overflow on the SECOND iteration
        // of the inner loop.
        p.oam[9 * 4 + 1] = 0;
        run_per_dot_fsm(&mut p);
        assert_eq!(p.spr_count, 8);
        assert!(
            p.status.contains(PpuStatus::SPRITE_OVERFLOW),
            "buggy n+m increment must find the diagonal-read overflow at sprite 9 byte 1"
        );
    }

    // -------------------------------------------------------------------
    // Sprite-eval FSM regression corpus. Originally introduced as the
    // parallel-implementation firewall gating the B8 swap from single-
    // shot to per-dot FSM. The single-shot collapse was removed in B8c;
    // these tests are now the regression net pinning the FSM's observable
    // output against a straight-line reference implementation
    // (`reference_eval`).
    //
    // The corpus targets:
    //   - Empty OAM (no in-range)
    //   - Exactly 8 in-range (no overflow)
    //   - 9+ in-range (clean overflow)
    //   - Diagonal-read scenarios (sprite N out-of-range, sprite (N+k)'s
    //     non-y byte in-range)
    //   - 8x8 + 8x16 sprite heights
    //   - Boundary scanlines (0, 1, 239, prerender)
    //
    // Random fuzz + structured edge cases combined give 1013 cases.
    // -------------------------------------------------------------------

    /// Tiny xorshift PRNG so the test is hermetic (no `rand` dep).
    struct XorShift(u64);
    impl XorShift {
        const fn new(seed: u64) -> Self {
            Self(if seed == 0 {
                0xDEAD_BEEF_CAFE_BABE
            } else {
                seed
            })
        }
        const fn next_u64(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x << 13;
            x ^= x >> 7;
            x ^= x << 17;
            self.0 = x;
            x
        }
        fn next_u8(&mut self) -> u8 {
            (self.next_u64() & 0xFF) as u8
        }
    }

    /// Snapshot of the observable post-dot-256 state for equivalence
    /// comparison.
    #[derive(Debug, Clone, PartialEq, Eq)]
    struct EvalObservable {
        secondary_oam: [u8; 32],
        spr_count: u8,
        spr_zero_in_line: bool,
        overflow: bool,
    }

    fn observe(p: &Ppu) -> EvalObservable {
        EvalObservable {
            secondary_oam: p.secondary_oam,
            spr_count: p.spr_count,
            spr_zero_in_line: p.spr_zero_in_line,
            overflow: p.status.contains(PpuStatus::SPRITE_OVERFLOW),
        }
    }

    /// Build a fresh PPU and seed `oam`, `scanline`, and `ctrl` from the
    /// given parameters.
    fn build_case(oam: &[u8; 256], scanline: i16, ctrl: PpuCtrl) -> Ppu {
        let mut p = Ppu::new(PpuRegion::Ntsc);
        p.post_reset_mask_remaining = 0;
        p.oam.copy_from_slice(oam);
        p.scanline = scanline;
        p.ctrl = ctrl;
        // Reset the overflow flag so we can observe per-case sets.
        p.status.remove(PpuStatus::SPRITE_OVERFLOW);
        // Pre-fill secondary OAM with a poison value so the per-dot FSM's
        // clear phase is observable (single-shot also starts by writing
        // $FF into all 32 bytes, so the final state must match).
        p.secondary_oam = [0xAA; 32];
        p.spr_count = 0;
        p.spr_zero_in_line = false;
        p
    }

    /// Drive the per-dot FSM through dots 0..=256 on `p`.
    fn run_per_dot_fsm(p: &mut Ppu) {
        for dot in 0..=256u16 {
            p.dot = dot;
            p.tick_sprite_eval_per_dot();
        }
    }

    /// Run one case through the FSM and assert observable matches the
    /// expected pinned state. The expected state is built by computing
    /// the result in a non-buggy reference implementation (the
    /// `reference_eval` below).
    fn assert_case_matches(label: &str, oam: &[u8; 256], scanline: i16, ctrl: PpuCtrl) {
        let expected = reference_eval(oam, scanline, ctrl);

        let mut pf = build_case(oam, scanline, ctrl);
        run_per_dot_fsm(&mut pf);
        let actual = observe(&pf);

        assert_eq!(
            expected,
            actual,
            "FSM mismatch for case `{label}` \
             (scanline={scanline}, 8x16={}, sprite_zero_y={:#04x})",
            ctrl.contains(PpuCtrl::SPRITE_SIZE_16),
            oam[0],
        );
    }

    /// Reference implementation: a straight-line sprite-eval emulation
    /// matching the 2C02's behavior, used as the golden expected output
    /// for the FSM regression corpus. Originally the FSM was validated
    /// against the old single-shot collapse via the 1013-case equivalence
    /// harness (B8a); after B8c removed the single-shot, this stand-alone
    /// reference plays the same role.
    fn reference_eval(oam: &[u8; 256], scanline: i16, ctrl: PpuCtrl) -> EvalObservable {
        // Y-test convention: see `tick_sprite_eval_per_dot` docstring.
        // Pre-render uses -1 (always-fail), visible uses the current
        // scanline; sprite Y=N renders on scanlines N+1..=N+h.
        let next_line: i16 = if scanline == PpuRegion::Ntsc.prerender_line() {
            -1
        } else {
            scanline
        };
        let sprite_height: i16 = if ctrl.contains(PpuCtrl::SPRITE_SIZE_16) {
            16
        } else {
            8
        };

        let mut secondary_oam = [0xFFu8; 32];
        let mut found = 0u8;
        let mut spr_zero_in_line = false;
        let mut overflow = false;

        let mut n_idx = 0usize;
        while n_idx < 64 {
            let base = n_idx * 4;
            let y = oam[base] as i16;
            let row = next_line - y;
            if row >= 0 && row < sprite_height {
                let sec_base = (found as usize) * 4;
                secondary_oam[sec_base] = oam[base];
                secondary_oam[sec_base + 1] = oam[base + 1];
                secondary_oam[sec_base + 2] = oam[base + 2];
                secondary_oam[sec_base + 3] = oam[base + 3];
                if n_idx == 0 {
                    spr_zero_in_line = true;
                }
                found += 1;
                if found == 8 {
                    n_idx += 1;
                    let mut m = 0u8;
                    while n_idx < 64 {
                        let nb = n_idx * 4 + (m as usize);
                        let by = oam[nb] as i16;
                        let brow = next_line - by;
                        if brow >= 0 && brow < sprite_height {
                            overflow = true;
                            break;
                        }
                        m = (m + 1) & 0x03;
                        n_idx += 1;
                    }
                    break;
                }
            }
            n_idx += 1;
        }

        EvalObservable {
            secondary_oam,
            spr_count: found,
            spr_zero_in_line,
            overflow,
        }
    }

    #[test]
    fn sprite_fsm_equivalence_edge_cases() {
        // 1: empty OAM (all 0xFF y) -> no found, no overflow.
        let mut oam = [0xFFu8; 256];
        assert_case_matches("empty_oam_y_ff", &oam, 0, PpuCtrl::empty());

        // 2: every byte 0xF0 (out of range) -> no found, no overflow.
        oam = [0xF0u8; 256];
        assert_case_matches("empty_oam_y_f0", &oam, 0, PpuCtrl::empty());

        // 3: 8 in-range sprites, all other bytes 0xF0 -> 8 found, no
        // overflow.
        oam = [0xF0u8; 256];
        for i in 0..8 {
            oam[i * 4] = 0;
        }
        assert_case_matches("8_in_range", &oam, 0, PpuCtrl::empty());

        // 4: 9 in-range sprites -> overflow set.
        oam = [0xF0u8; 256];
        for i in 0..9 {
            oam[i * 4] = 0;
        }
        assert_case_matches("9_in_range", &oam, 0, PpuCtrl::empty());

        // 5: 8 in-range + diagonal-read overflow (sprite 9 byte 1 = 0
        // forces buggy n+m to fire).
        oam = [0xF0u8; 256];
        for i in 0..8 {
            oam[i * 4] = 0;
        }
        oam[9 * 4 + 1] = 0;
        assert_case_matches("diagonal_overflow", &oam, 0, PpuCtrl::empty());

        // 6: 8x16 sprite mode.
        oam = [0xF0u8; 256];
        for i in 0..3 {
            oam[i * 4] = 0;
        }
        assert_case_matches("8x16_mode", &oam, 0, PpuCtrl::SPRITE_SIZE_16);

        // 7: pre-render line (evaluates for scanline 0).
        oam = [0xF0u8; 256];
        for i in 0..5 {
            oam[i * 4] = 0;
        }
        let prerender = PpuRegion::Ntsc.prerender_line();
        assert_case_matches("prerender_line", &oam, prerender, PpuCtrl::empty());

        // 8: last visible scanline.
        oam = [0xF0u8; 256];
        for i in 0..2 {
            oam[i * 4] = 239;
        }
        assert_case_matches("scanline_239", &oam, 238, PpuCtrl::empty());

        // 9: sprite zero NOT in range -> spr_zero_in_line must stay false.
        oam = [0xF0u8; 256];
        oam[0] = 0xF0; // sprite 0 out of range
        for i in 1..3 {
            oam[i * 4] = 0;
        }
        assert_case_matches("zero_out_of_range", &oam, 0, PpuCtrl::empty());

        // 10: sprite zero in range but not first -> still must be true
        // because sprite 0 is at OAM index 0.
        oam = [0xF0u8; 256];
        oam[0] = 0; // sprite 0 in range
        for i in 5..10 {
            oam[i * 4] = 0;
        }
        assert_case_matches("zero_in_range_plus_others", &oam, 0, PpuCtrl::empty());

        // 11: exactly 1 in-range at the last possible sprite (sprite 63).
        oam = [0xF0u8; 256];
        oam[63 * 4] = 0;
        assert_case_matches("only_sprite_63", &oam, 0, PpuCtrl::empty());

        // 12: 8 in-range scattered among the 64 entries.
        oam = [0xF0u8; 256];
        for (slot, &n) in [0u8, 5, 11, 18, 27, 35, 44, 55].iter().enumerate() {
            let _ = slot;
            oam[(n as usize) * 4] = 0;
        }
        assert_case_matches("8_scattered", &oam, 0, PpuCtrl::empty());

        // 13: all 64 sprites in range -> 8 found + overflow.
        oam = [0u8; 256];
        for i in 0..64 {
            oam[i * 4] = 0; // y = 0
            oam[i * 4 + 1] = 0xAB;
            oam[i * 4 + 2] = 0xCD;
            oam[i * 4 + 3] = 0xEF;
        }
        assert_case_matches("all_64_in_range", &oam, 0, PpuCtrl::empty());
    }

    #[test]
    fn sprite_fsm_equivalence_randomized_corpus() {
        // 1000 fully-random cases + the 13 edge cases above = 1013 total
        // regression checks. Each invocation runs the FSM on a random
        // OAM/scanline/ctrl seed and asserts observable equality with
        // the straight-line reference implementation.
        const N: usize = 1000;
        let mut rng = XorShift::new(0x1234_5678_9ABC_DEF0);

        for case in 0..N {
            let mut oam = [0u8; 256];
            for b in &mut oam {
                *b = rng.next_u8();
            }
            // Choose scanline from {0..=239, prerender=261}. Use a bias
            // toward 0..=239 since that's the realistic case.
            let r = rng.next_u64();
            let scanline: i16 = if r.trailing_zeros() >= 5 {
                PpuRegion::Ntsc.prerender_line()
            } else {
                ((r >> 8) & 0xFF) as i16 % 240
            };
            // 8x16 mode in 1/4 of cases.
            let ctrl = if rng.next_u64().trailing_zeros() >= 2 {
                PpuCtrl::SPRITE_SIZE_16
            } else {
                PpuCtrl::empty()
            };

            let expected = reference_eval(&oam, scanline, ctrl);

            let mut pf = build_case(&oam, scanline, ctrl);
            run_per_dot_fsm(&mut pf);
            let actual = observe(&pf);

            assert_eq!(
                expected,
                actual,
                "FSM regressed against reference at case #{case} \
                 (scanline={scanline}, 8x16={}, oam[0]={:#04x})",
                ctrl.contains(PpuCtrl::SPRITE_SIZE_16),
                oam[0],
            );
        }
    }

    /// Cascade A reproducer V3: mimics `AccuracyCoin`'s
    /// `VerifySpriteZeroHits` step 2 (the version that EXPECTS a hit).
    /// Sprite 0 at Y=5 X=8 tile $C0. BG tile $C0 at nametable $2C21
    /// (NT 3 col 1 row 1). v = $2C00.
    ///
    /// Tile $C0 has a SINGLE opaque pixel at (col=0, row=0). With v=$2C00,
    /// BG tile at NT 3 position $21 displays at screen pixels (8, 8).
    /// Sprite at (Y=5, X=8) tile $C0 draws at scanline 6 (per nesdev:
    /// sprite occupies scanlines Y+1..Y+8). Sprite tile $C0's only opaque
    /// pixel is (col 0, row 0) → screen (8, 6).
    ///
    /// Sprite (8, 6) vs BG (8, 8) — NO geometric overlap. The test asserts
    /// a hit IS expected here, which is impossible without sprite Y
    /// semantics being different from what nesdev documents. This unit
    /// test makes the discrepancy concrete so it can be investigated
    /// against Mesen2 or other reference emulators.
    #[test]
    fn cascade_a_verify_sprite_zero_hits_step2() {
        let (mut p, mut b) = fresh_ppu();
        // Pin the PPU to (prerender, dot=0) so this diagnostic harness runs
        // through exactly one frame starting from the prerender boundary.
        // Required because Ppu::new() now starts at (prerender, dot=340)
        // per Session-13 Option B (close the +344-dot offset vs Mesen2);
        // without this reset the test's "advance one frame" loop would begin
        // mid-prerender and the sprite-zero-hit window would shift relative
        // to the BG-pipeline cycle-9 reload point this test was designed to
        // characterise (see docs/audit/cascade-a-investigation-2026-05-19.md
        // and docs/audit/session-13-cpu-boot-fix-2026-05-21.md).
        p.dot = 0;
        let tile_c0_base = 0xC0 * 16;
        // Tile $C0: only the (col 0, row 0) pixel is opaque (lo=$80 hi=$80).
        b.chr[tile_c0_base] = 0x80;
        b.chr[tile_c0_base + 8] = 0x80;
        // Tile $24: fully transparent (all-zero bytes already).
        // Fill NT 3 (bank 1 of CIRAM, horizontal mirroring) with $24, then
        // write $C0 at position $21.
        for i in 0..0x400 {
            p.ciram[0x400 + i] = 0x24;
        }
        p.ciram[0x400 + 0x021] = 0xC0;
        // OAM page mimics OAM DMA from a $FF-cleared page + sprite 0 init.
        for i in 0..256 {
            p.oam[i] = 0xFF;
        }
        p.oam[0] = 0x05; // Y = 5 (step 2)
        p.oam[1] = 0xC0; // CHR
        p.oam[2] = 0x03; // ATT
        p.oam[3] = 0x08; // X = 8
        // v = $2C00 (NT 3 top-left).
        p.v = 0x2C00;
        p.t = 0x2C00;
        // PPUCTRL = 0 (both pattern tables at $0000).
        p.ctrl = PpuCtrl::empty();
        // Enable rendering.
        let mask = PpuMask::SHOW_BG
            | PpuMask::SHOW_SPRITE
            | PpuMask::SHOW_BG_LEFT
            | PpuMask::SHOW_SPRITE_LEFT;
        p.mask = mask;
        p.mask_skip_pipe1 = mask;
        p.mask_for_skip_check = mask;
        p.status = PpuStatus::empty();
        // Advance ~1 full frame to allow sprite-zero hit to fire if it should.
        for _ in 0..(262 * 341) {
            p.tick(&mut b);
        }
        let hit = p.status.contains(PpuStatus::SPRITE_ZERO_HIT);
        // POST-FIX EXPECTATION: with the cycle-9 reload + post-emit shift
        // BG-pipeline correction landed (see
        // `docs/audit/cascade-a-investigation-2026-05-19.md`), tile $C0's
        // single opaque BG pixel lands at screen column 8 (PPU dot 9 of
        // scanline 6), exactly overlapping the sprite-zero opaque pixel
        // at (8, 6) → SPRITE-ZERO HIT must fire.
        //
        // The test ROM's geometry: sprite Y=5 X=8 tile $C0 has its only
        // opaque pixel at sprite-local (col 0, row 0) → screen (8, 6).
        // BG tile $C0 at NT 3 position $21 with v=$2C00 (fine Y=2,
        // coarse Y=0) renders at scanline 6, screen column 8, with its
        // only opaque pixel matching → overlap → hit.
        assert!(
            hit,
            "BG-pipeline fix regression: sprite-zero hit must fire for \
             VerifySpriteZeroHits step 2 (BG opaque at (8,6) overlaps \
             sprite-zero opaque at (8,6)) — see \
             docs/audit/cascade-a-investigation-2026-05-19.md."
        );
    }

    /// Cascade A reproducer V2: start in VBL, enable rendering via the
    /// CPU-visible `$2001` write (with the 2-PPU-clock pipeline delay), do
    /// OAM DMA via the CPU-visible `$2003 + $2004` writes, then advance
    /// past pre-render → scanline 0 → scanline 1. More faithful to the
    /// real ROM execution path than the V1 reproducer.
    #[test]
    fn cascade_a_sprite_zero_hit_y0_x8_via_register_writes() {
        let (mut p, mut b) = fresh_ppu();
        // Load tile $FC into pattern table 0 fully-opaque.
        let tile_fc_base = 0xFC * 16;
        for row in 0..8 {
            b.chr[tile_fc_base + row] = 0xFF;
            b.chr[tile_fc_base + 8 + row] = 0x00;
        }
        // Write nametable $2001 = $FC via $2006 + $2007 (CPU-visible path).
        p.cpu_write_register(6, 0x20, &mut b); // hi
        p.cpu_write_register(6, 0x01, &mut b); // lo (v = $2001)
        p.cpu_write_register(7, 0xFC, &mut b);
        // Reset scroll: v = $2000 via $2006 + $2006.
        p.cpu_write_register(6, 0x20, &mut b);
        p.cpu_write_register(6, 0x00, &mut b);
        // Mimic the ROM's OAM page: ClearPage2 fills with $FF, then
        // InitializeSpriteZero writes sprite 0. So OAM[0..4] is the sprite,
        // OAM[4..256] is $FF (Y=$FF -> off-screen).
        for i in 0..256 {
            p.oam[i] = 0xFF;
        }
        // OAM DMA: write sprite 0 via $2003 (OAMADDR) + $2004 (OAMDATA).
        p.cpu_write_register(3, 0x00, &mut b); // OAMADDR = 0
        p.cpu_write_register(4, 0x00, &mut b); // sprite 0 Y = 0
        p.cpu_write_register(4, 0xFC, &mut b); // sprite 0 CHR = $FC
        p.cpu_write_register(4, 0x00, &mut b); // sprite 0 ATT = 0
        p.cpu_write_register(4, 0x08, &mut b); // sprite 0 X = 8
        // Advance to scanline 241 dot 1 (VBL start) — matches the ROM
        // post-WaitForVBlank position.
        while !(p.scanline == 241 && p.dot == 1) {
            p.tick(&mut b);
        }
        // Enable rendering via $2001 write (BG + SPR + show-left).
        let mask = (PpuMask::SHOW_BG
            | PpuMask::SHOW_SPRITE
            | PpuMask::SHOW_BG_LEFT
            | PpuMask::SHOW_SPRITE_LEFT)
            .bits();
        p.cpu_write_register(1, mask, &mut b);
        // PPUSTATUS may have VBL set; clear sprite-zero-hit start clean.
        p.status.remove(PpuStatus::SPRITE_ZERO_HIT);
        // Now advance through ~30 scanlines (rest of VBL + pre-render +
        // visible 0-9), matching what Clockslide_3000 covers in the ROM.
        for _ in 0..(30 * 341) {
            p.tick(&mut b);
        }
        assert!(
            p.status.contains(PpuStatus::SPRITE_ZERO_HIT),
            "Expected sprite-zero hit set after 30 scanlines past VBL. \
             Actual status=0x{:02X}, scanline={}, dot={}, \
             spr_count={}, spr_zero_in_line={}, \
             spr_x[0]={}, spr_shift_lo[0]=0x{:02X}, spr_shift_hi[0]=0x{:02X}, \
             mask=0x{:02X}, ctrl=0x{:02X}",
            p.status.bits(),
            p.scanline,
            p.dot,
            p.spr_count,
            p.spr_zero_in_line,
            p.spr_x[0],
            p.spr_shift_lo[0],
            p.spr_shift_hi[0],
            p.mask.bits(),
            p.ctrl.bits(),
        );
    }

    /// Cascade A reproducer: the exact `AccuracyCoin TEST_Sprite0Hit_Behavior`
    /// sub-test 1 scenario, constructed directly without going through the
    /// CPU/test-ROM.
    ///
    /// Setup (matches `AccuracyCoin.asm:PREP_SpriteZeroHit` + the test's
    /// pre-state):
    ///
    /// - Sprite 0: `Y=$00, CHR=$FC, ATT=$00, X=$08`.
    /// - BG nametable: `vram[$2001] = $FC` (tile $FC at col=1, row=0).
    /// - CHR pattern table 0, tile $FC, all 8 rows: `lo=$FF / hi=$00`
    ///   (fully opaque pixels of palette colour 1).
    /// - `PPUMASK = $1E` (BG + SPR + `BG_LEFT` + grayscale; the actual
    ///   `PPUMASK_COPY` value the diagnostic probe in
    ///   `crates/rustynes-test-harness/src/accuracy_coin.rs` captures at frame
    ///   3393 — see `docs/audit/accuracycoin-readme-analysis-2026-05-17.md`
    ///   §"Addendum (2026-05-19, session 5)").
    /// - `PPUCTRL = $00` (BG and sprite pattern tables both at `$0000`).
    /// - `v = $2000` (top-left of nametable 0).
    ///
    /// **Expected**: sprite zero hit (PPUSTATUS bit 6) is set by the end
    /// of scanline 1 — sprite pixel (8..15, 1) overlaps BG pixel (8..15,
    /// 1) and both are opaque.
    ///
    /// **Current (2026-05-19, pre-fix)**: this test FAILS. The
    /// diagnostic probe shows PPUSTATUS bit 6 = 0 in the live battery
    /// (full ROM run). This unit test is the isolated reproducer.
    #[test]
    fn cascade_a_sprite_zero_hit_y0_x8_tile_fc_overlap() {
        let (mut p, mut b) = fresh_ppu();
        // 1. Load tile $FC into pattern table 0 with fully-opaque pixels.
        let tile_fc_base = 0xFC * 16;
        for row in 0..8 {
            b.chr[tile_fc_base + row] = 0xFF; // lo plane (palette bit 0)
            b.chr[tile_fc_base + 8 + row] = 0x00; // hi plane (palette bit 1)
        }
        // 2. Write tile $FC into nametable position $2001 (col=1, row=0).
        //    CIRAM bank 0 directly (horizontal mirroring: $2000-$23FF -> ciram[0..0x400]).
        p.ciram[0x001] = 0xFC;
        // 3. Sprite 0: Y=$00, CHR=$FC, ATT=$00, X=$08.
        p.oam[0] = 0x00;
        p.oam[1] = 0xFC;
        p.oam[2] = 0x00;
        p.oam[3] = 0x08;
        // 4. PPUMASK = SHOW_BG | SHOW_SPRITE | SHOW_BG_LEFT | grayscale.
        let mask_bits = PpuMask::SHOW_BG
            | PpuMask::SHOW_SPRITE
            | PpuMask::SHOW_BG_LEFT
            | PpuMask::SHOW_SPRITE_LEFT;
        p.mask = mask_bits;
        // Pipeline the mask through the two skip-check stages so the
        // rendering-enabled signal is stable immediately.
        p.mask_skip_pipe1 = mask_bits;
        p.mask_for_skip_check = mask_bits;
        // 5. PPUCTRL = 0 (BG and sprite both at pattern table 0).
        p.ctrl = PpuCtrl::empty();
        // 6. v = $2000 (top-left of nametable 0).
        p.v = 0x2000;
        // Make sure sprite-zero-hit and VBL start clean.
        p.status = PpuStatus::empty();
        // Pre-render starts; advance ~3 full scanlines so we cross
        // pre-render → scanline 0 → scanline 1 → scanline 2. By the end
        // of scanline 1, the sprite-zero hit should be set.
        // Frame is 262*341 dots. We need at least scanlines 261..=2 = 4
        // scanlines = 4*341 = 1364 dots. Use 1500 for safety.
        for _ in 0..1500 {
            p.tick(&mut b);
        }
        assert!(
            p.status.contains(PpuStatus::SPRITE_ZERO_HIT),
            "Expected sprite-zero hit (PPUSTATUS bit 6) to be set after \
             scanline 1 with sprite 0 at (Y=0, X=8) tile $FC overlapping \
             BG nametable[$2001]=$FC (both fully opaque). \
             Actual status=0x{:02X}, scanline={}, dot={}, \
             spr_count={}, spr_zero_in_line={}, \
             spr_x[0]={}, spr_shift_lo[0]=0x{:02X}, spr_shift_hi[0]=0x{:02X}",
            p.status.bits(),
            p.scanline,
            p.dot,
            p.spr_count,
            p.spr_zero_in_line,
            p.spr_x[0],
            p.spr_shift_lo[0],
            p.spr_shift_hi[0],
        );
    }

    // =========================================================
    // $2002 VBL race-window sweep — Mesen2-independent oracle
    // (Session-18 / C1 attempt 16, PPU axis).
    //
    // The nesdev wiki [`PPU registers`] page documents the race:
    //
    //   "Reading the status register within two cycles of when VBL is
    //    set will return 0 in bit 7 but clear the latch anyway, causing
    //    the program to miss frames."
    //
    //   "Reading PPUSTATUS at the exact start of vertical blank will
    //    return 0 in bit 7 but clear the latch anyway, causing NMI to
    //    not occur that frame."
    //
    // Three documented dot-cohorts straddling scanline 241 dot 1:
    //
    //   * dot < the-VBL-set-dot (i.e. dot 0 of scanline 241, or
    //     earlier): VBL bit is 0 in PPUSTATUS, latch was never set,
    //     suppression DOES happen if read lands on dot 0 of scanline
    //     241 (the one-dot-before window).
    //   * dot == the-VBL-set-dot (= dot 1 of scanline 241): the
    //     "exact start of VBL" window — read returns 0, latch is
    //     cleared, and the in-frame VBL set is suppressed.
    //   * dot > the-VBL-set-dot (dot 2 or later of scanline 241):
    //     read returns 1 (VBL was set), latch is cleared by the
    //     read, no suppression of subsequent VBL/NMI within that
    //     frame because the set already happened.
    //
    // This unit test sweeps the PPU position across that boundary
    // (scanline 240 dot 339 through scanline 241 dot 5) and tabulates
    // the four observables per scenario: (a) the read return value's
    // bit 7, (b) whether suppress_vbl_this_frame got set, (c) whether
    // PPUSTATUS.VBLANK is set inside the PPU after the read, (d) the
    // value the next read of $2002 returns once we tick past dot 1.
    //
    // The test asserts the expected race-window semantics for ALL
    // dot positions. If `RustyNES` honours the nesdev spec, every
    // assertion passes. If not, the failing rows expose the exact
    // boundary off-by-one.
    //
    // After the test the table itself is `println!`'d for human
    // inspection via `--nocapture`.
    /// Loop budget: one full NTSC frame's worth of dots plus a
    /// 1024-dot safety margin, ample to sweep into scanline 242.
    #[cfg(test)]
    const VBL_SWEEP_MAX_TICKS: u32 = 262 * 341 + 1024;

    #[test]
    #[allow(clippy::too_many_lines)]
    #[allow(clippy::items_after_statements)]
    fn vbl_race_window_2002_read_sweep() {
        use alloc::format;
        use alloc::string::String;
        use alloc::vec::Vec;
        // `eprintln!` lives in std; tests run in a `std` cargo unit so
        // this is fine.
        extern crate std;
        use std::eprintln;
        // The window we sweep, in (scanline, dot) pairs, listed in
        // tick-order. We use NTSC (vblank_start_line = 241).
        //
        // Layout choice: scan two extra dots into scanline 240 (the
        // last visible line) so the "VBL never gets set this frame"
        // pre-window is observable; then sweep dots 0..=5 of scanline
        // 241; then sweep two dots into scanline 242 for the post-VBL
        // tail. Total = 2 + 6 + 2 = 10 sample points.
        //
        // We re-create a fresh PPU for each sample-point so the
        // suppression-latch carries no contamination from the prior
        // sample. The PPU's internal state between samples is the
        // confounding factor we MUST isolate.

        #[derive(Debug, Clone, Copy)]
        struct ExpectedRow {
            scanline: i16,
            dot: u16,
            // Bit 7 of the value returned by the $2002 read.
            // None = no specific spec assertion (don't enforce).
            read_bit7: Option<u8>,
            // Whether `suppress_vbl_this_frame` should be set after
            // the read. None = don't enforce.
            suppress_set: Option<bool>,
            // Whether `status.VBLANK` is set after the read (the
            // read always clears it, so this should be `false` for
            // any cohort where the read happens AT or AFTER the set
            // dot; and `false` for cohorts where the set never
            // happened either).
            vblank_after_read: Option<bool>,
        }

        // Per the wiki, the VBL flag is set at scanline 241 dot 1.
        // The "race window" is documented as:
        //   - dot 0 of scanline 241: read returns 0, suppresses VBL set
        //   - dot 1 of scanline 241: read returns 0, suppresses VBL set
        //   - dot 2 of scanline 241: read returns 1, normal clear
        //
        // RustyNES's current impl reads back `dot <= 1` for the
        // suppression branch (see `cpu_read_register` case 2 above:
        // `if self.scanline == self.region.vblank_start_line() &&
        // self.dot <= 1 { self.suppress_vbl_this_frame = true; ... }`).
        //
        // The exact rendering of "read on the same dot as set"
        // depends on whether the set callback in tick() fires
        // BEFORE the read or AFTER. Since the test ticks the PPU
        // to a position FIRST then issues a synchronous read in the
        // same test step, the read sees the post-tick state — i.e.
        // the read on dot 1 of scanline 241 sees VBL set.
        let expected: [ExpectedRow; 10] = [
            ExpectedRow {
                scanline: 240,
                dot: 339,
                read_bit7: Some(0),
                suppress_set: Some(false),
                vblank_after_read: Some(false),
            },
            ExpectedRow {
                scanline: 240,
                dot: 340,
                read_bit7: Some(0),
                suppress_set: Some(false),
                vblank_after_read: Some(false),
            },
            ExpectedRow {
                scanline: 241,
                dot: 0,
                read_bit7: Some(0),
                suppress_set: Some(true),
                vblank_after_read: Some(false),
            },
            ExpectedRow {
                scanline: 241,
                dot: 1,
                // VBL is set on this tick BEFORE the read; the read
                // returns 1 AND `suppress_vbl_this_frame` is latched
                // (RustyNES's `dot <= 1` race window — 2 PPU dots wide).
                //
                // Session-18 / C1 attempt 16 (PPU-axis, rolled back):
                // tightening the predicate to `dot == 0` (matching
                // Mesen2 + nesdev wiki) did not flip the failing
                // `cpu_interrupts_v2/{2,3,5}` tests at the integration
                // layer — the load-bearing axis is the CPU-vs-PPU
                // intra-cycle access interleaving, not the suppression
                // predicate's literal dot range. Restored 2-dot window
                // as the cleaner regression invariant; the unit test
                // documents the actual behavior. See
                // `docs/audit/session-18-c1-attempt16-ppu-axis-rollback-2026-05-22.md`.
                read_bit7: Some(1),
                // R2 (mc-r1-substrate): the dot==0 race window (Mesen2
                // `UpdateStatusFlag:590`) makes the dot-1 read a normal
                // post-set read — no suppression. Default (2-dot window):
                // suppression latches at dot 1 too.
                suppress_set: Some(false),
                vblank_after_read: Some(false),
            },
            ExpectedRow {
                scanline: 241,
                dot: 2,
                read_bit7: Some(1),
                suppress_set: Some(false),
                vblank_after_read: Some(false),
            },
            ExpectedRow {
                scanline: 241,
                dot: 3,
                read_bit7: Some(1),
                suppress_set: Some(false),
                vblank_after_read: Some(false),
            },
            ExpectedRow {
                scanline: 241,
                dot: 4,
                read_bit7: Some(1),
                suppress_set: Some(false),
                vblank_after_read: Some(false),
            },
            ExpectedRow {
                scanline: 241,
                dot: 5,
                read_bit7: Some(1),
                suppress_set: Some(false),
                vblank_after_read: Some(false),
            },
            ExpectedRow {
                scanline: 242,
                dot: 0,
                read_bit7: Some(1),
                suppress_set: Some(false),
                vblank_after_read: Some(false),
            },
            ExpectedRow {
                scanline: 242,
                dot: 1,
                read_bit7: Some(1),
                suppress_set: Some(false),
                vblank_after_read: Some(false),
            },
        ];

        // Per-row capture for human inspection.
        #[derive(Debug)]
        struct ObservedRow {
            scanline: i16,
            dot: u16,
            read_value: u8,
            read_bit7: u8,
            suppress_set_after: bool,
            status_vblank_after: bool,
        }
        let mut observed = Vec::<ObservedRow>::new();

        for row in &expected {
            // Build a fresh PPU and tick it to the target (scanline, dot).
            // Strategy: tick UNTIL we land on the target. Each `tick`
            // call calls `advance_dot()` first, so the post-tick state
            // is (scanline + 1, dot=1 wraparound) etc. Hence we tick
            // until p.scanline()/p.dot() match.
            //
            // Disable rendering so we don't trigger A12 emissions,
            // sprite eval, etc. — keeps the test focused on VBL +
            // $2002.
            let (mut p, mut b) = fresh_ppu();
            // No PPUMASK render bits. No PPUCTRL bits (so NMI off).
            // post_reset_mask_remaining = 0 already (fresh_ppu sets it).
            // Tick to the target. Loop bound is one full NTSC frame plus
            // safety margin (see `VBL_SWEEP_MAX_TICKS` above).
            let mut ticks = 0u32;
            while !(p.scanline == row.scanline && p.dot == row.dot) {
                p.tick(&mut b);
                ticks += 1;
                assert!(
                    ticks < VBL_SWEEP_MAX_TICKS,
                    "could not reach (scanline={}, dot={}) within one frame; \
                     loop bug or scheduler change",
                    row.scanline,
                    row.dot,
                );
            }

            // Issue the $2002 read.
            let read_value = p.cpu_read_register(2, &mut b);
            let read_bit7 = (read_value >> 7) & 1;
            let suppress_set_after = p.suppress_vbl_this_frame;
            let status_vblank_after = p.status.contains(PpuStatus::VBLANK);

            observed.push(ObservedRow {
                scanline: row.scanline,
                dot: row.dot,
                read_value,
                read_bit7,
                suppress_set_after,
                status_vblank_after,
            });
        }

        // Print the table for human inspection (only visible with
        // --nocapture).
        eprintln!();
        eprintln!("=== $2002 VBL race-window sweep ===");
        eprintln!(
            "{:>3} {:>3}  {:>8} {:>7} {:>11} {:>14}",
            "sl", "dot", "read", "bit7", "suppress?", "PPU.VBLANK?",
        );
        for o in &observed {
            eprintln!(
                "{:>3} {:>3}  0x{:02X}     {:>5}    {:>9}    {:>10}",
                o.scanline,
                o.dot,
                o.read_value,
                o.read_bit7,
                o.suppress_set_after,
                o.status_vblank_after,
            );
        }
        eprintln!();

        // Assert the spec — but only on rows where `expected` carries
        // a concrete claim. Rows with `None` are recording-only.
        let mut failures = Vec::<String>::new();
        for (i, row) in expected.iter().enumerate() {
            let obs = &observed[i];
            if let Some(want) = row.read_bit7
                && obs.read_bit7 != want
            {
                failures.push(format!(
                    "(sl={}, dot={}): expected read bit7 = {}, got {}",
                    row.scanline, row.dot, want, obs.read_bit7,
                ));
            }
            if let Some(want) = row.suppress_set
                && obs.suppress_set_after != want
            {
                failures.push(format!(
                    "(sl={}, dot={}): expected suppress_vbl = {}, got {}",
                    row.scanline, row.dot, want, obs.suppress_set_after,
                ));
            }
            if let Some(want) = row.vblank_after_read
                && obs.status_vblank_after != want
            {
                failures.push(format!(
                    "(sl={}, dot={}): expected status.VBLANK after read = {}, got {}",
                    row.scanline, row.dot, want, obs.status_vblank_after,
                ));
            }
        }

        assert!(
            failures.is_empty(),
            "$2002 race-window sweep mismatches vs. nesdev wiki spec:\n  {}",
            failures.join("\n  "),
        );
    }

    // -------------------------------------------------------------------
    // v1.3.x left-edge regression: BG attribute (palette) shift register
    // must stay in lockstep with the BG pattern shift registers through
    // the dots 321-336 pre-fetch boundary.
    //
    // 086ce4d moved the BG pattern pipeline to the Mesen2 cycle-9 reload +
    // post-emit shift model and added an explicit `<<= 8` at pre-fetch
    // dots 328/336 for the 16-bit pattern shifters, but left the 8-bit
    // `at_shift` + 1-bit `at_feed` attribute model untouched. The two
    // pipelines then advanced at different rates across the pre-fetch
    // region, so the palette (attribute) bits drifted one tile out of
    // phase with the pattern bits in the leftmost columns — the source of
    // the "green tint / garbage palette in the left 1-2 columns while
    // scrolling" regression. The fix makes the attribute shifters 16-bit
    // and shift them in lockstep with the pattern shifters.
    // -------------------------------------------------------------------

    /// Render one visible scanline with a SOLID pattern everywhere
    /// (pattern value 1 in every tile) but a per-tile-group ATTRIBUTE
    /// boundary, then return the (pattern, palette) the PPU emitted at
    /// each of the first 24 columns. Because the pattern value is the
    /// same everywhere, any column-to-column change is purely an
    /// attribute (palette) change — which is exactly what the AT shift
    /// register controls. Misalignment between the pattern and attribute
    /// pipelines therefore shows up as the palette boundary landing on
    /// the wrong column.
    ///
    /// Returns a Vec of `(palette_value)` per column 0..24 of the target
    /// scanline (pattern value is always 1, verified internally).
    fn diag_attr_palette_per_column(fine_x: u8, coarse_x: u16) -> alloc::vec::Vec<u8> {
        use alloc::vec::Vec;
        let target_line: usize = 5;
        let (mut p, mut b) = fresh_ppu();

        // Single solid tile: tile 1 = pattern value 1 on all rows.
        for row in 0..8u16 {
            b.chr[(0x0010 + row) as usize] = 0xFF; // lo plane all set
            b.chr[(0x0018 + row) as usize] = 0x00; // hi plane clear -> value 1
        }

        // Nametable 0: every tile = tile 1 (solid). Attribute table sets
        // a palette boundary: tile-column groups 0-1 use palette 1,
        // everything else uses palette 0. Each attribute byte covers a
        // 4x4-tile (32x32px) region split into four 2x2-tile quadrants.
        // We set the top-left quadrant of attribute byte 0 to palette 1.
        for off in 0..0x03C0u16 {
            p.ciram[off as usize] = 0x01; // tile index 1 everywhere
        }
        // Attribute table starts at $23C0 -> CIRAM offset 0x03C0.
        // Byte 0 covers tile columns 0-3, rows 0-3. Bits 1-0 = top-left
        // quadrant (tile cols 0-1, rows 0-1); bits 3-2 = top-right
        // quadrant (tile cols 2-3, rows 0-1). Set TL=palette 1, TR=
        // palette 2, the rest palette 0. This puts an attribute boundary
        // at every 16px (tile-pair) step so coarse-X scroll moves the
        // boundary across the pre-fetch-fed leftmost tile — the exact
        // condition that exposed the 086ce4d AT lockstep regression.
        p.ciram[0x03C0] = 0b00_00_10_01; // TL=pal1, TR=pal2.
        // The target scanline is row 5 -> tile row 0 -> top quadrants.

        // Palettes: pattern value 1...
        //   palette 0 -> $3F01
        //   palette 1 -> $3F05
        //   palette 2 -> $3F09
        p.palette_ram[palette_index(0x3F00)] = 0x0F; // universal
        p.palette_ram[palette_index(0x3F01)] = 0x30; // pal0 value1 = white
        p.palette_ram[palette_index(0x3F05)] = 0x16; // pal1 value1 = red
        p.palette_ram[palette_index(0x3F09)] = 0x2A; // pal2 value1 = green

        // No sprites.
        for i in 0..256 {
            p.oam[i] = 0xF0;
        }

        p.ctrl = PpuCtrl::empty();
        p.mask = PpuMask::SHOW_BG | PpuMask::SHOW_BG_LEFT;

        // Scroll: coarse-X into t bits 0-4, fine-x into p.x.
        p.t = coarse_x & 0x1F;
        p.v = 0;
        p.x = fine_x;

        p.scanline = p.region.prerender_line();
        p.dot = 0;
        p.last_a12_level = false;

        for _ in 0..(341 * (target_line + 2)) {
            p.tick(&mut b);
        }

        let line = target_line;
        let pal0 = crate::palette::nes_color_to_rgba(0x30);
        let pal1 = crate::palette::nes_color_to_rgba(0x16);
        let pal2 = crate::palette::nes_color_to_rgba(0x2A);
        let universal = crate::palette::nes_color_to_rgba(0x0F);
        let mut out = Vec::with_capacity(24);
        for x in 0..24usize {
            let off = (line * 256 + x) * 4;
            let px = [
                p.framebuffer[off],
                p.framebuffer[off + 1],
                p.framebuffer[off + 2],
                p.framebuffer[off + 3],
            ];
            // Map color back to palette index: 0/1/2 = palette, 254 =
            // universal, 255 = unexpected.
            let v = if px == pal0 {
                0u8
            } else if px == pal1 {
                1u8
            } else if px == pal2 {
                2u8
            } else if px == universal {
                254u8
            } else {
                255u8
            };
            out.push(v);
        }
        out
    }

    /// The expected palette index for screen column `x` given a scroll of
    /// `coarse_x` tiles + `fine_x` pixels. Tile column C maps to: 0-1 ->
    /// palette 1, 2-3 -> palette 2, 4+ -> palette 0. With a total left
    /// shift of `coarse_x*8 + fine_x` pixels, screen column `x`
    /// corresponds to source pixel `x + coarse_x*8 + fine_x`, whose tile
    /// column is that pixel / 8.
    fn expected_palette(x: usize, fine_x: u8, coarse_x: u16) -> u8 {
        let src_pixel = x + (coarse_x as usize) * 8 + fine_x as usize;
        let tile_col = src_pixel / 8;
        match tile_col {
            0 | 1 => 1,
            2 | 3 => 2,
            _ => 0,
        }
    }

    /// With NO scroll the palette-1 region must cover exactly tile columns
    /// 0-1 (screen columns 0-15), palette-2 tile columns 2-3 (16-31), and
    /// palette-0 beyond. Visible-region pipeline only; must hold both
    /// before and after the fix.
    #[test]
    fn bg_attribute_boundary_no_scroll() {
        let cols = diag_attr_palette_per_column(0, 0);
        for (x, &v) in cols.iter().enumerate() {
            let want = expected_palette(x, 0, 0);
            assert_eq!(
                v, want,
                "col {x}: expected palette {want}, got {v} (full: {cols:?})"
            );
        }
    }

    /// The palette boundary must stay glued to the pattern across BOTH
    /// fine-X and coarse-X scroll. This is the case the 086ce4d AT-register
    /// regression broke: the pre-fetch `<<= 8` (added for the 16-bit
    /// pattern shifters) was not applied to the 8-bit attribute model, so
    /// the palette drifted one tile relative to the pattern across the
    /// dots 321-336 pre-fetch boundary — wrong palette in the leftmost
    /// tile column (screen columns 0-7). With the 16-bit AT shifters the
    /// boundary tracks the pattern exactly at every scroll value.
    ///
    /// Empirically: on the pre-fix (HEAD) tree the `coarse_x` cases below
    /// mis-paint screen columns 0-7; on the fixed tree every column
    /// matches `expected_palette`.
    #[test]
    fn bg_attribute_boundary_tracks_pattern_under_scroll() {
        for coarse_x in 0..6u16 {
            for fine_x in 0..8u8 {
                let cols = diag_attr_palette_per_column(fine_x, coarse_x);
                for (x, &v) in cols.iter().enumerate() {
                    let want = expected_palette(x, fine_x, coarse_x);
                    assert_eq!(
                        v, want,
                        "coarse_x={coarse_x} fine_x={fine_x} col {x}: \
                         expected palette {want}, got {v}\nfull: {cols:?}\n\
                         (palette boundary must stay glued to the pattern; \
                         a mismatch in columns 0-7 is the 086ce4d \
                         AT-register lockstep regression)"
                    );
                }
            }
        }
    }

    /// Register-level invariant: the attribute shift registers must track
    /// the BG pattern shift registers bit-for-bit through the exact
    /// reload / shift / pre-fetch-`<<= 8` sequence that a real scanline
    /// boundary performs. This is the direct, hermetic guard against the
    /// 086ce4d regression where the attribute pipeline (then an 8-bit
    /// register + 1-bit feed) advanced at a different rate than the 16-bit
    /// pattern pipeline across the dots 321-336 pre-fetch boundary.
    ///
    /// The check exploits an exact structural equivalence: for any tile
    /// whose pattern low byte is `0xFF` (all 8 pixels opaque in plane 0),
    /// the pattern-low shift register's per-pixel bit equals 1 for that
    /// tile's 8 columns. An attribute bit that is set (`at_latch` bit
    /// set) expands to `0xFF` in `reload_bg_shift_regs`, so the AT-low
    /// register must hold the IDENTICAL 8-bit run as the pattern-low
    /// register for that tile. We reload two tiles with pattern-low
    /// `0xFF` + attribute bit set, run the pre-fetch `<<= 8` boundary,
    /// then assert the AT registers equal the pattern registers exactly.
    #[test]
    #[ignore = "permanent-by-design: pins the SUPERSEDED pre-master-clock BG shifter feed. The default master-clock core injects the BG serial-in '1' source the AccuracyCoin 'BG Serial In' test proves correct, so this unit assertion is kept as a historical pin and cannot be un-ignored. Visual coverage: visual_regression 7/7 on the default build."]
    fn bg_attribute_register_lockstep_through_prefetch() {
        let (mut p, _b) = fresh_ppu();

        // Start clean.
        p.bg_shift_lo = 0;
        p.bg_shift_hi = 0;
        p.at_shift_lo = 0;
        p.at_shift_hi = 0;

        // Tile A: pattern low = 0xFF, high = 0xFF; attribute = 0b11 (both
        // bits set -> both AT bytes expand to 0xFF). After reload the low
        // byte of every register is 0xFF.
        p.bg_lo_latch = 0xFF;
        p.bg_hi_latch = 0xFF;
        p.at_latch = 0b11;
        p.reload_bg_shift_regs();
        assert_eq!(p.bg_shift_lo & 0x00FF, 0x00FF);
        assert_eq!(p.at_shift_lo & 0x00FF, 0x00FF);
        assert_eq!(p.at_shift_hi & 0x00FF, 0x00FF);

        // Pre-fetch `<<= 8` (dots 328 / 336): the pattern and attribute
        // registers MUST be shifted identically. Exercises the exact
        // production helper used inside `tick`.
        p.prefetch_shift_bg_regs();

        // Tile B: same content reloaded into the low byte.
        p.bg_lo_latch = 0xFF;
        p.bg_hi_latch = 0xFF;
        p.at_latch = 0b11;
        p.reload_bg_shift_regs();

        // After the boundary, both tiles' data is present and the
        // attribute registers must be bit-identical to the pattern
        // registers (because both tiles set every plane-0 / plane-1 bit
        // AND every attribute bit). Any drift between the two pipelines
        // (the regression) makes these diverge.
        assert_eq!(
            p.at_shift_lo, p.bg_shift_lo,
            "AT-low shifter must track pattern-low shifter bit-for-bit \
             through the pre-fetch boundary (086ce4d lockstep regression)"
        );
        assert_eq!(
            p.at_shift_hi, p.bg_shift_hi,
            "AT-high shifter must track pattern-high shifter bit-for-bit \
             through the pre-fetch boundary (086ce4d lockstep regression)"
        );

        // Now shift one pixel (post-emit `shift_bg`) and re-check lockstep.
        p.shift_bg();
        assert_eq!(p.at_shift_lo, p.bg_shift_lo, "lockstep after shift_bg");
        assert_eq!(p.at_shift_hi, p.bg_shift_hi, "lockstep after shift_bg");
    }
}
