//! `Mapper` trait + supporting `MapperError` type.
//!
//! Concrete mappers live in their own modules (e.g. `nrom`). The trait
//! interface follows `docs/mappers.md` §Interfaces; the rationale for
//! mapper-resident IRQ logic is in `docs/mappers.md` §IRQ counter mechanisms.

use alloc::{string::String, vec::Vec};
use thiserror::Error;

use crate::cartridge::Mirroring;

/// Per-tile extended-attribute / extended-bank info supplied by mappers that
/// implement an extended attribute mode (currently MMC5 in `$5104` mode 01).
///
/// The PPU consults this at the nametable-byte fetch boundary of each BG
/// tile. When `Some`, the PPU:
///
/// - Replaces the standard 2-bit attribute-derived palette with [`Self::palette`].
/// - Routes the BG pattern fetch for that tile through [`Self::chr_bank`]
///   (a 12-bit physical 4 KiB bank index — see MMC5 `ExGrafix` mode docs).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExAttribute {
    /// 2-bit palette select (replaces the AT-byte palette).
    pub palette: u8,
    /// 12-bit physical CHR bank for this tile (in 4 KiB units). Combined with
    /// the fine-Y / column offset by the mapper at fetch time.
    pub chr_bank: u16,
}

/// Per-tile override produced by a mapper that implements a vertical
/// split-screen mode (currently MMC5 via `$5200`-`$5202`).
///
/// MMC5's split allows a vertical band of the screen to render from an
/// independently scrolled "alt region" backed by ExRAM-as-nametable and a
/// dedicated 4 KiB CHR bank. The PPU consults
/// [`Mapper::bg_split_state`] once per 8-dot BG fetch group (at the NT-byte
/// fetch boundary). When `Some(...)` is returned, the PPU uses the supplied
/// nametable address, attribute address, and fine-Y in place of its own
/// loopy-v derivation for that tile, and the mapper internally latches the
/// CHR bank for the following BG pattern fetches.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BgSplitState {
    /// Synthesized nametable byte address in `$2000-$3EFF`, computed from
    /// the alt-region scroll and the current coarse-X.
    pub nt_addr: u16,
    /// Synthesized attribute byte address in the same nametable.
    pub at_addr: u16,
    /// Fine-Y (0..=7) within the alt-region's logical row.
    pub fine_y: u8,
    /// 4 KiB CHR bank index (from `$5202` for MMC5) for the alt region's
    /// BG pattern fetches.
    pub chr_bank: u8,
}

/// Read-only debug snapshot of mapper-internal state for the UI.
///
/// Mappers override [`Mapper::debug_info`] to surface their banking
/// registers and any IRQ-counter state. Fields are pre-formatted strings
/// so the UI doesn't have to know the protocol of each mapper.
#[derive(Debug, Default, Clone)]
pub struct MapperDebugInfo {
    /// Mapper id (e.g. `4` for MMC3).
    pub mapper_id: u16,
    /// Human-readable mapper name (e.g. `"MMC3 (Sharp)"`).
    pub name: String,
    /// Current mirroring layout name (`"Horizontal"`, `"Vertical"`,
    /// `"SingleScreen"`, `"FourScreen"`).
    pub mirroring: &'static str,
    /// PRG bank registers — one (label, value) entry per register the
    /// mapper exposes. Values are hex-formatted by the mapper.
    pub prg_banks: Vec<(String, String)>,
    /// CHR bank registers — same shape as `prg_banks`.
    pub chr_banks: Vec<(String, String)>,
    /// IRQ counter state — `(label, value)` pairs, e.g.
    /// `[("counter", "0x12"), ("reload", "0x80"), ("enabled", "true")]`.
    pub irq_state: Vec<(String, String)>,
    /// Free-form extra status (envelope shape, sub-mapper flags, ...).
    pub extra: Vec<(String, String)>,
    // v1.5.0 "Lens" Workstream I8 — cartridge-level metadata, populated by the
    // bus (it owns the `Cartridge`) when it builds the debug view, NOT by each
    // mapper. All default to empty / 0 / None so a mapper's own `debug_info()`
    // (which leaves these untouched) is unchanged, and the headless / no-pack
    // path is byte-identical (this is an output-only inspection struct).
    /// NES 2.0 submapper id (0 for iNES 1.0).
    pub submapper: u8,
    /// Accuracy-evidence tier name (`"Core"` / `"Curated"` / `"BestEffort"`),
    /// empty if the id isn't classified.
    pub tier: &'static str,
    /// PRG-ROM size in bytes.
    pub prg_rom_size: usize,
    /// CHR-ROM size in bytes (0 when the board uses CHR-RAM).
    pub chr_rom_size: usize,
    /// Requested PRG-RAM size in bytes.
    pub prg_ram_size: usize,
    /// Requested CHR-RAM size in bytes (0 when the board ships CHR-ROM).
    pub chr_ram_size: usize,
    /// True when PRG-RAM is battery-backed (save RAM / NVRAM present).
    pub has_battery: bool,
    /// The IRQ mechanism, when the board has one (e.g. `"PPU A12 (MMC3)"`,
    /// `"PPU scanline (MMC5)"`, `"CPU cycle (VRC/FME-7/N163)"`). Empty if the
    /// board has no IRQ source.
    pub irq_kind: &'static str,
    /// On-cart expansion-audio chip name, if any (e.g. `"VRC6"`).
    pub expansion_audio: Option<&'static str>,
}

/// APU frame-counter event mask fanned out to the mapper, used by on-cart
/// audio extensions (MMC5, FDS, …) whose internal envelopes / length counters
/// share the CPU's frame-counter clocks.
///
/// The bus calls [`Mapper::notify_frame_event`] once per CPU cycle, passing
/// the events the APU's frame counter fired on the same cycle. For mappers
/// without on-cart audio (or with audio that doesn't use envelopes / length
/// counters, like VRC6) the default no-op impl is used.
///
/// The struct shape matches `rustynes_apu::frame_counter::FrameEvents` 1:1; the
/// duplication is deliberate — `rustynes-mappers` does not depend on `rustynes-apu`,
/// and the bus is responsible for translating between the two types.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct MapperFrameEvents {
    /// Quarter-frame: clock envelopes.
    pub quarter: bool,
    /// Half-frame: clock length counters (and sweeps, for 2A03 — but MMC5
    /// pulses have no sweep unit).
    pub half: bool,
}

/// Errors produced by mapper save-state load.
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum MapperError {
    /// Save-state blob is shorter than expected for this mapper.
    #[error("mapper save state truncated: expected {expected} bytes, got {got}")]
    Truncated {
        /// Expected byte count.
        expected: usize,
        /// Actual byte count.
        got: usize,
    },

    /// Save-state blob carries an unknown version tag.
    #[error("mapper save state has unsupported version {0}")]
    UnsupportedVersion(u8),

    /// Save-state blob would put the mapper into an inconsistent state.
    #[error("mapper save state invalid: {0}")]
    Invalid(String),
}

/// v2.8.0 Phase 4 — per-CPU-cycle mapper capability flags (see
/// [`Mapper::caps`]). Each flag corresponds to one of the four hooks the
/// bus would otherwise virtually dispatch every CPU cycle.
// Four INDEPENDENT capability bits, one per skippable hook — a bitflags
// type would obscure the 1:1 hook mapping for zero gain.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MapperCaps {
    /// The mapper overrides [`Mapper::notify_cpu_cycle`] (CPU-clocked IRQ
    /// counters: VRC4/6/7, FME-7, Namco 163, MMC3's A12 filter clock, …).
    pub cpu_cycle_hook: bool,
    /// The mapper overrides [`Mapper::mix_audio`] (on-cart expansion audio;
    /// only meaningful when the `mapper-audio` feature is compiled in).
    pub audio: bool,
    /// The mapper overrides [`Mapper::notify_frame_event`] (MMC5 audio's
    /// frame-counter-cadenced envelope/length clocks).
    pub frame_event_hook: bool,
    /// The mapper overrides [`Mapper::irq_pending`] (it can assert IRQs).
    pub irq_source: bool,
}

impl MapperCaps {
    /// Every hook dispatched — the safe default for unannotated mappers.
    pub const ALL: Self = Self {
        cpu_cycle_hook: true,
        audio: true,
        frame_event_hook: true,
        irq_source: true,
    };
    /// No hooks — discrete boards (NROM/UxROM/CNROM/AxROM/…) with no IRQ,
    /// no audio, and no per-cycle state.
    pub const NONE: Self = Self {
        cpu_cycle_hook: false,
        audio: false,
        frame_event_hook: false,
        irq_source: false,
    };
    /// CPU-cycle hook + IRQ source (the common IRQ-mapper shape).
    pub const CYCLE_IRQ: Self = Self {
        cpu_cycle_hook: true,
        audio: false,
        frame_event_hook: false,
        irq_source: true,
    };
}

/// Trait implemented by every cartridge mapper.
///
/// Visible read/write addresses follow `docs/architecture.md`
/// §Per-memory-access fanout. The PPU bus call covers the whole
/// `$0000-$3FFF` address space because nametable mirroring is mapper-controlled
/// (see `docs/mappers.md` §Mirroring).
///
/// IRQ-emitting mappers signal pending IRQs via [`Mapper::irq_pending`]; the
/// CPU bus polls this on the same cycle it polls APU/external IRQs.
///
/// All trait methods are `&mut self` because every mapper has at least some
/// internal state — open-bus latch, bank registers, IRQ counters, etc. —
/// even on an apparent read.
pub trait Mapper: Send {
    /// Read a byte from the CPU address space `$4020-$FFFF`.
    fn cpu_read(&mut self, addr: u16) -> u8;

    /// Write a byte to the CPU address space `$4020-$FFFF`.
    fn cpu_write(&mut self, addr: u16, value: u8);

    /// Returns `true` when `addr` is **not** wired to mapper-resident
    /// memory — i.e. when `cpu_read(addr)` returns junk and the bus
    /// should fall through to the open-bus latch instead of overwriting
    /// it. The CPU databus is left floating in this case, so the most
    /// recently driven byte stays visible to the next read.
    ///
    /// Default impl covers stock NROM-class boards: the entire
    /// `$4020-$5FFF` window is unmapped (no PRG-RAM, no mapper
    /// registers); `$6000-$FFFF` is always considered mapped (PRG-RAM
    /// or PRG-ROM). Mappers that DO map any subset of `$4020-$5FFF`
    /// (MMC5 audio + `ExRAM`, FME-7 IRQ control, VRC family register
    /// banks, etc.) must override this to return `false` for their
    /// mapped sub-ranges so the bus uses the real value.
    ///
    /// This is the canonical hardware oracle for `AccuracyCoin`'s
    /// `CPU Behavior :: Open Bus` Test 1 (`LDA $5000` should read
    /// `$50`, not `$00`).
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x5FFF).contains(&addr)
    }

    /// Read a byte from the PPU address space `$0000-$3FFF` (pattern table
    /// + nametable mirror window). Used as the BG-side / generic fetch path.
    fn ppu_read(&mut self, addr: u16) -> u8;

    /// Read a byte from the PPU pattern-table window (`$0000-$1FFF`) on
    /// behalf of a *sprite* tile fetch. MMC5 in 8x16 sprite mode uses a
    /// separate set of CHR bank registers (`$5120-$5127`) for sprite
    /// fetches; other mappers default to forwarding to [`Mapper::ppu_read`].
    fn ppu_read_sprite(&mut self, addr: u16) -> u8 {
        self.ppu_read(addr)
    }

    /// Write a byte to the PPU address space `$0000-$3FFF`.
    fn ppu_write(&mut self, addr: u16, value: u8);

    /// Optionally synthesize a nametable byte for `addr` ($2000-$3EFF).
    ///
    /// When the mapper returns `Some(v)`, the PPU uses `v` directly and
    /// skips its CIRAM read. MMC5 uses this for "fill mode" (`$5105`
    /// per-1KiB selector value 0b11), where every nametable-byte fetch
    /// returns the fill tile (`$5106`) and every attribute-byte fetch
    /// returns a 2-bit attribute (`$5107`) replicated 4 ways.
    ///
    /// Default returns `None` (mapper does not synthesize; PPU reads CIRAM).
    fn nametable_fetch(&mut self, _addr: u16) -> Option<u8> {
        None
    }

    /// Optionally absorb a nametable write for `addr` ($2000-$3EFF) directly
    /// into mapper-resident storage.
    ///
    /// Returns `true` if the mapper consumed the write (PPU should NOT also
    /// write its CIRAM). MMC5 uses this for ExRAM-mapped nametables and for
    /// fill mode (where writes are silently dropped).
    ///
    /// Default returns `false` (PPU continues with the CIRAM write path).
    fn nametable_write(&mut self, _addr: u16, _value: u8) -> bool {
        false
    }

    /// Optionally provide per-tile extended attribute + CHR-bank override for
    /// the BG tile currently being fetched.
    ///
    /// `v` is the PPU's loopy-v register value at NT-byte fetch time
    /// (`fetch_nt`); the lower 12 bits encode the 32x30 tile coordinate.
    /// MMC5 in `$5104` mode 01 returns `Some` here. The PPU uses the
    /// returned palette to override the AT byte. The mapper itself caches
    /// the `chr_bank` internally so that the subsequent BG pattern fetches
    /// (`ppu_read` calls during the same 8-dot fetch group) consult it
    /// instead of the standard BG bank registers.
    ///
    /// Default returns `None` (no extended attribute mode active).
    fn peek_ex_attribute(&mut self, _v: u16) -> Option<ExAttribute> {
        None
    }

    /// Optionally redirect a BG fetch group into a vertical split-screen
    /// "alt region".
    ///
    /// Called by the PPU at the NT-byte fetch boundary of each 8-dot BG
    /// fetch group, once per tile column (32 times per visible scanline).
    /// `scanline_y` is the current visible-scanline index in 0..=239 (the
    /// pre-render line passes 0 here to keep the path branchless). `coarse_x`
    /// is the loopy-v coarse-X (0..=31) for the tile about to be fetched.
    ///
    /// Returning `Some(state)` instructs the PPU to use the supplied
    /// `nt_addr` / `at_addr` / `fine_y` instead of those derived from `v`,
    /// and instructs the mapper to internally latch the CHR bank for the
    /// pattern fetches that immediately follow.
    ///
    /// Default returns `None`.
    fn bg_split_state(&mut self, _scanline_y: u16, _coarse_x: u16) -> Option<BgSplitState> {
        None
    }

    /// Resolve a nametable address in `$2000-$3EFF` to a CIRAM offset in
    /// `0..0x800`. The PPU owns the 2 KiB CIRAM and uses this hook to apply
    /// per-mapper mirroring without giving the mapper direct access to the
    /// console-side VRAM.
    ///
    /// Default impl applies the mirroring reported by [`Mapper::current_mirroring`]
    /// via [`crate::Mirroring::physical_bank`]. Mappers with on-cart 4-screen
    /// VRAM (Gauntlet, Rad Racer II) can override this; the lockstep bus
    /// will trampoline the read/write back through the mapper for any offset
    /// outside `0..0x800` if needed (Phase 4).
    #[allow(clippy::cast_possible_truncation)]
    fn nametable_address(&self, addr: u16) -> u16 {
        const NAMETABLE_SIZE: u16 = 0x0400;
        let table = ((addr.wrapping_sub(0x2000)) / NAMETABLE_SIZE) & 0x03;
        let local = addr & (NAMETABLE_SIZE - 1);
        // `physical_bank` always returns 0 or 1; the truncation is a no-op.
        let physical = self.current_mirroring().physical_bank(table as u8) as u16;
        physical * NAMETABLE_SIZE + local
    }

    /// Notify of a PPU A12 line transition. Default no-op; MMC3 / MMC5
    /// override this for IRQ counter clocking.
    fn notify_a12(&mut self, _level: bool) {}

    /// Notify of a PPU A12 line transition with the current sub-dot of
    /// the host CPU cycle (0 / 1 = M2-low half; 2 = M2-high half).
    /// Default impl falls through to [`Self::notify_a12`] so existing
    /// mappers compile unchanged; MMC3 overrides this for the
    /// M2-phase-aware IRQ-output propagation delay required by
    /// `mmc3_test_2/4-scanline_timing` sub-test #3 (C1 step B4 successor).
    fn notify_a12_at_sub_dot(&mut self, level: bool, _sub_dot: u8) {
        self.notify_a12(level);
    }

    /// Notify of a CPU cycle. Default no-op; VRC2/4/6, FME-7, Namco 163
    /// override this for IRQ counter clocking.
    fn notify_cpu_cycle(&mut self) {}

    /// Notify the mapper of the APU frame-counter events fired on the
    /// current CPU cycle (quarter-frame envelope clock, half-frame length
    /// clock). Only on-cart audio extensions that re-use the 2A03 frame
    /// counter cadence need to handle this (MMC5 audio's two pulse
    /// channels). Default no-op.
    fn notify_frame_event(&mut self, _events: MapperFrameEvents) {}

    /// Notify the mapper that the PPU is starting a new rendered scanline.
    ///
    /// Called by the PPU at the start of each visible scanline (and the
    /// pre-render line) before any tile fetches happen. MMC5 uses this to
    /// drive its scanline IRQ counter (which clocks at PPU cycle 4 of each
    /// rendered line — different from MMC3's A12-edge-driven counter).
    ///
    /// Default is a no-op; only mappers with scanline-counter IRQs override.
    fn notify_scanline_start(&mut self) {}

    /// Notify the mapper that the PPU has entered vertical blank.
    ///
    /// MMC5 uses this to clear its "in-frame" flag (bit 6 of `$5204`).
    /// Default no-op.
    fn notify_vblank(&mut self) {}

    /// Returns `true` if the mapper is currently asserting an IRQ.
    fn irq_pending(&self) -> bool {
        false
    }

    /// Acknowledge a pending IRQ. Default no-op; mappers that latch IRQ state
    /// override this.
    fn irq_acknowledge(&mut self) {}

    /// Return one signed audio sample for mappers with on-cart audio
    /// (VRC6/7, MMC5, Sunsoft 5B, Namco 163, FDS). Default returns silence.
    fn mix_audio(&mut self) -> i16 {
        0
    }

    /// v2.8.0 Phase 4 — the mapper's per-CPU-cycle capability flags.
    ///
    /// The bus fans four virtual calls out to the mapper EVERY CPU cycle
    /// (~30 k/frame each): [`Self::notify_cpu_cycle`], [`Self::mix_audio`],
    /// [`Self::notify_frame_event`], and [`Self::irq_pending`]. For most
    /// boards all four are the default no-ops, so the bus caches these
    /// flags at construction and skips the dispatch entirely.
    ///
    /// The contract is mechanical: a flag may be `false` ONLY when the
    /// mapper does not override the corresponding default method (skipping
    /// a default no-op is provably byte-identical). The default returns
    /// [`MapperCaps::ALL`], so an unannotated mapper keeps every dispatch —
    /// always correct, just slower.
    fn caps(&self) -> MapperCaps {
        MapperCaps::ALL
    }

    /// Returns the mapper's current effective mirroring layout.
    ///
    /// Most mappers report the static mirroring set in the cartridge header;
    /// mappers with runtime mirroring control (MMC1, MMC3, `AxROM`, ...) report
    /// the live state.
    fn current_mirroring(&self) -> Mirroring;

    // --- Optional Famicom Disk System (FDS) disk interface ---
    //
    // Only the FDS device (`fds::Fds`) overrides these; every other mapper uses
    // the default no-op / empty impls. The bus and `Nes` surface them so a
    // frontend can drive side-swap and persist a modified disk without
    // downcasting the `Box<dyn Mapper>`.

    /// Number of disk sides in the inserted image (0 for non-FDS mappers).
    fn disk_side_count(&self) -> usize {
        0
    }

    /// The currently inserted disk side index, or `None` when ejected (or for
    /// non-FDS mappers).
    fn inserted_disk_side(&self) -> Option<usize> {
        None
    }

    /// Insert disk side `i` (`Some`) or eject the disk (`None`). No-op for
    /// non-FDS mappers; an out-of-range index is ignored by the FDS device.
    fn set_disk_side(&mut self, _side: Option<usize>) {}

    /// Start recording the diagnostic FDS read-stream trace (off by default;
    /// observation-only). No-op for non-FDS mappers. See [`crate::FdsTraceRec`].
    fn enable_fds_trace(&mut self) {}

    /// Drain the accumulated FDS read-stream trace records (empty for non-FDS
    /// mappers / when tracing was never enabled).
    fn take_fds_trace(&mut self) -> Vec<crate::FdsTraceRec> {
        Vec::new()
    }

    /// Re-serialize the (possibly-modified) disk image to its byte layout for
    /// host persistence. Returns an empty vector for non-FDS mappers.
    fn disk_image_bytes(&self) -> Vec<u8> {
        Vec::new()
    }

    /// Whether the disk image has unsaved writes. Always `false` for non-FDS
    /// mappers.
    fn disk_is_dirty(&self) -> bool {
        false
    }

    /// Clear the disk dirty flag (a host calls this after persisting). No-op
    /// for non-FDS mappers.
    fn clear_disk_dirty(&mut self) {}

    /// Mark the inserted disk read-only (`true`) or writable (`false`). No-op
    /// for non-FDS mappers.
    fn set_disk_write_protected(&mut self, _protected: bool) {}

    // --- Optional NSF music-player interface ---
    //
    // Only the NSF player (`nsf::NsfMapper`) overrides these; every other mapper
    // uses the default 0 / no-op impls. The bus and `Nes` surface them so a
    // frontend can drive track selection without downcasting the boxed mapper.

    /// Number of selectable songs (0 for a non-NSF mapper).
    fn nsf_song_count(&self) -> u8 {
        0
    }

    /// The currently-selected 0-based song (0 for a non-NSF mapper).
    fn nsf_current_song(&self) -> u8 {
        0
    }

    /// Select a 0-based song. Returns `true` if this is an NSF mapper (so the
    /// caller knows to re-run the reset that re-vectors into the driver's
    /// `init`). Default no-op returning `false`.
    fn nsf_set_song(&mut self, _song: u8) -> bool {
        false
    }

    /// Encode the mapper's mutable state into a tagged save-state blob.
    fn save_state(&self) -> Vec<u8>;

    /// Decode a previously [`Mapper::save_state`] blob back into the mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError`] when the blob is truncated, has the wrong
    /// version tag, or otherwise fails internal consistency checks.
    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError>;

    /// Surface read-only debug info for the UI. Override per mapper to
    /// expose bank registers, IRQ counters, etc. Default returns a
    /// minimal entry naming the mapper id.
    fn debug_info(&self) -> MapperDebugInfo {
        // The bus fills the cartridge-level metadata fields (submapper, tier,
        // sizes, battery, irq_kind, expansion_audio) — see
        // `Bus::mapper_debug_info` — so they default here (v1.5.0 I8).
        MapperDebugInfo {
            mapper_id: 0,
            name: "(unknown)".into(),
            mirroring: mirroring_name(self.current_mirroring()),
            prg_banks: Vec::new(),
            chr_banks: Vec::new(),
            irq_state: Vec::new(),
            extra: Vec::new(),
            ..Default::default()
        }
    }
}

/// Helper for mapper `debug_info` overrides.
#[must_use]
pub const fn mirroring_name(m: Mirroring) -> &'static str {
    match m {
        Mirroring::Horizontal => "Horizontal",
        Mirroring::Vertical => "Vertical",
        Mirroring::SingleScreenA => "SingleScreen (A)",
        Mirroring::SingleScreenB => "SingleScreen (B)",
        Mirroring::FourScreen => "FourScreen",
        Mirroring::MapperControlled => "MapperControlled",
    }
}

#[cfg(test)]
mod caps_tests {
    use super::MapperCaps;
    use crate::{Mapper, parse};
    use alloc::{boxed::Box, vec, vec::Vec};

    /// Build a minimal iNES image for `mapper_id` (32 KiB PRG + 8 KiB CHR —
    /// 32 KiB satisfies every family probed below, incl. `AxROM`'s
    /// 32-KiB-multiple requirement).
    fn synth_rom(mapper_id: u8) -> Vec<u8> {
        let mut rom = vec![
            b'N',
            b'E',
            b'S',
            0x1A,
            2, // 32 KiB PRG
            1, // 8 KiB CHR
            (mapper_id << 4),
            (mapper_id & 0xF0),
        ];
        rom.resize(16, 0);
        rom.resize(16 + 32 * 1024 + 8 * 1024, 0);
        rom
    }

    fn caps_of(mapper_id: u8) -> MapperCaps {
        let (_cart, mapper): (_, Box<dyn Mapper>) =
            parse(&synth_rom(mapper_id)).expect("synth rom parses");
        mapper.caps()
    }

    /// v2.8.0 Phase 4 — the capability-flag contract for the key families.
    /// A flag may be `false` ONLY when the mapper does not override the
    /// corresponding default no-op; these spot checks pin the mechanical
    /// derivation for the highest-population boards.
    #[test]
    fn caps_match_overridden_hooks_for_key_mappers() {
        // NROM / UxROM / CNROM / AxROM / GxROM: no hooks at all.
        for id in [0u8, 2, 3, 7, 66] {
            assert_eq!(caps_of(id), MapperCaps::NONE, "mapper {id}");
        }
        // MMC1: overrides ONLY notify_cpu_cycle (write throttle) — no IRQ.
        let m1 = caps_of(1);
        assert!(m1.cpu_cycle_hook && !m1.irq_source && !m1.audio && !m1.frame_event_hook);
        // MMC3: cycle hook (A12 filter clock) + IRQ source.
        assert_eq!(caps_of(4), MapperCaps::CYCLE_IRQ);
        // MMC5: cycle + IRQ + frame-event hook (+ audio when compiled in).
        let m5 = caps_of(5);
        assert!(m5.cpu_cycle_hook && m5.irq_source && m5.frame_event_hook);
        assert_eq!(m5.audio, cfg!(feature = "mapper-audio"));
        // VRC6a: cycle + IRQ + audio-when-compiled.
        let m24 = caps_of(24);
        assert!(m24.cpu_cycle_hook && m24.irq_source && !m24.frame_event_hook);
        assert_eq!(m24.audio, cfg!(feature = "mapper-audio"));
    }
}
