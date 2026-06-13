//! PPU-side bus trait.
//!
//! The PPU owns its CIRAM (2 KiB nametable VRAM in real hardware), OAM, and
//! palette RAM. CHR-ROM / CHR-RAM / nametable mirroring all go through the
//! mapper's PPU port — modeled here as the `PpuBus` trait. Mappers also
//! receive A12 transition notifications via `notify_a12` so MMC3 / MMC5 can
//! drive their IRQ counters.
//!
//! Per `docs/ppu-2c02.md` §Interfaces.

/// Bus interface the PPU sees.
///
/// In production the lockstep bus in `rustynes-core` routes:
///
/// - CHR reads/writes (`$0000-$1FFF`) → mapper.
/// - Nametable reads/writes (`$2000-$3EFF`) → PPU's own CIRAM, with the
///   mapper-supplied mirroring offset via [`PpuBus::nametable_address`].
/// - A12 transitions → mapper.
///
/// In tests, a small in-memory [`PpuBus`] impl owns 8 KiB of CHR-RAM and a
/// dummy mirroring map.
pub trait PpuBus {
    /// Read a byte at `addr`. The PPU passes addresses in the full
    /// `$0000-$3FFF` window; the bus is responsible for routing CHR
    /// (`$0000-$1FFF`) and nametables (`$2000-$3EFF`) appropriately.
    fn ppu_read(&mut self, addr: u16) -> u8;

    /// Read a byte from the pattern-table window (`$0000-$1FFF`) on behalf
    /// of a *sprite* tile fetch. MMC5 in 8x16 sprite mode uses a different
    /// CHR bank set (`$5120-$5127`) for sprite fetches than for BG; other
    /// mappers default to the same path as [`Self::ppu_read`].
    fn ppu_read_sprite(&mut self, addr: u16) -> u8 {
        self.ppu_read(addr)
    }

    /// Write a byte at `addr`.
    fn ppu_write(&mut self, addr: u16, value: u8);

    /// Optionally synthesize a nametable byte for `addr` ($2000-$3EFF).
    ///
    /// When the bus returns `Some(v)`, the PPU uses `v` directly and skips
    /// its CIRAM read. MMC5 uses this for fill mode and ExRAM-as-nametable.
    /// Default returns `None`.
    fn peek_nametable(&mut self, _addr: u16) -> Option<u8> {
        None
    }

    /// Optionally absorb a nametable write directly into mapper storage.
    ///
    /// Returns `true` if consumed; PPU then skips its CIRAM write. Default
    /// returns `false`.
    fn write_nametable(&mut self, _addr: u16, _value: u8) -> bool {
        false
    }

    /// Optional per-tile extended attribute + CHR-bank override for the BG
    /// tile currently being fetched (loopy-v passed in `v`). MMC5 in `$5104`
    /// mode 01 (`ExGrafix`) returns `Some(...)` here. Default returns `None`.
    fn peek_ex_attribute(&mut self, _v: u16) -> Option<ExAttribute> {
        None
    }

    /// Optional vertical split-screen override for the BG fetch group about
    /// to start at `(scanline_y, coarse_x)`. MMC5 with split enabled
    /// (`$5200` bit 7) returns `Some(...)` here for tile columns that fall
    /// within the alt region. Default returns `None`.
    fn bg_split_state(&mut self, _scanline_y: u16, _coarse_x: u16) -> Option<BgSplitState> {
        None
    }

    /// Notification of a PPU A12 line transition (rising or falling). The
    /// PPU calls this on every transition, with `level = true` for high.
    /// MMC3 / MMC5 use this internally for IRQ counter clocking.
    fn notify_a12(&mut self, _level: bool) {}

    /// Notification that the PPU is starting a new rendered scanline (visible
    /// or pre-render). MMC5 uses this to drive its scanline IRQ counter.
    /// Default no-op.
    fn notify_scanline_start(&mut self) {}

    /// Notification that the PPU has entered vertical blank. MMC5 uses this
    /// to clear its "in-frame" flag. Default no-op.
    fn notify_vblank(&mut self) {}

    /// Resolve a logical nametable address in `$2000-$3EFF` to a CIRAM offset
    /// in `0..0x800` under the mapper's currently-selected mirroring.
    ///
    /// Default impl uses a vertical-mirroring fallback so this trait remains
    /// drop-in for ad-hoc test buses; the lockstep bus in `rustynes-core`
    /// overrides this to delegate to `Mapper::nametable_address`.
    fn nametable_address(&self, addr: u16) -> u16 {
        // Default fallback: vertical mirroring (tables 0/2 -> bank 0, 1/3 -> bank 1).
        let table = ((addr.wrapping_sub(0x2000)) / 0x0400) & 0x03;
        let local = addr & 0x03FF;
        ((table & 1) * 0x0400) | local
    }
}

/// Re-export of the mapper-side per-tile extended-attribute info.
///
/// Lives in `rustynes-mappers` (the canonical owner) and is re-declared here as
/// a small POD to avoid making `rustynes-ppu` depend on `rustynes-mappers`. The
/// lockstep bus's `PpuBusAdapter` translates between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct ExAttribute {
    /// 2-bit palette select for this tile.
    pub palette: u8,
    /// 12-bit physical CHR bank (4 KiB units) for this tile.
    pub chr_bank: u16,
}

/// Vertical split-screen override (MMC5 `$5200`-`$5202` and equivalents).
///
/// Lives in `rustynes-mappers` (the canonical owner) and is re-declared here as
/// a small POD to avoid making `rustynes-ppu` depend on `rustynes-mappers`. The
/// lockstep bus's `PpuBusAdapter` translates between the two.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct BgSplitState {
    /// Synthesized nametable byte address for the alt region (`$2000-$3EFF`).
    pub nt_addr: u16,
    /// Synthesized attribute byte address for the alt region.
    pub at_addr: u16,
    /// Fine-Y (0..=7) for the alt region's logical row.
    pub fine_y: u8,
    /// 4 KiB CHR bank index for the alt region's BG pattern fetches.
    pub chr_bank: u8,
}
