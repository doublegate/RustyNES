//! v2.3.3 "Lucid" — per-byte **write attribution** for PPU-visible memory.
//!
//! # What this is for
//!
//! The pixel-provenance debugger answers "why is this pixel this color?" by
//! walking backwards from an emitted pixel through the PPU fetch pipeline to the
//! bytes that fed it — the nametable byte, the attribute byte, the pattern-table
//! bytes, the palette entry, the sprite record. That chain stops dead at the
//! bytes themselves unless something remembers **who put them there**.
//!
//! The existing devtools each hold one piece and none holds this one. The Trace
//! Logger has the PC but not the effect; the Event Viewer
//! (`LockstepBus::events`) has the CPU-side `$2000-$3FFF` write with its PPU
//! position but neither the resolved VRAM address nor the PC; the memory-access
//! counter has per-address read/write counts and a last-access cycle stamp but,
//! again, no PC. This module supplies the missing edge: for each byte of the
//! PPU's own memories, the **program counter and CPU cycle of the write that
//! last stored it**.
//!
//! # Why the storage lives here and not on the bus
//!
//! Attribution cannot be recorded at [`crate::PpuBus`]'s CPU-write boundary,
//! because the *effective* destination is not visible there. A nametable byte is
//! written by a `STA $2007` whose target address lives in the PPU's internal `v`
//! register, set earlier by two `$2006` writes; a palette entry is the same
//! `$2007` store landing in a different memory; an OAM byte arrives either
//! through `$2004` or as one of 256 bytes of an OAM DMA burst triggered by a
//! single `$4014` store. Only the PPU knows where each one actually went, so the
//! record is stamped at the store site — [`Ppu::write_vram`], [`Ppu::write_palette`],
//! and the two OAM write paths — with a `(pc, cycle)` context the bus latches per
//! instruction and hands down.
//!
//! [`Ppu::write_vram`]: crate::Ppu
//! [`Ppu::write_palette`]: crate::Ppu
//!
//! # Cost
//!
//! The whole module is `debug-hooks`-gated, and even under that feature the store
//! is allocated lazily — [`Ppu::write_attribution`] is `None` until the frontend
//! arms it, so a `debug-hooks` build with the provenance panel closed pays one
//! `Option` discriminant test per PPU-memory write and nothing else. Armed, it
//! costs [`WriteAttribution::HEAP_BYTES`] of heap and one 16-byte store per
//! write. Nothing here is read by emulation, so the framebuffer, the audio, and
//! the cycle counts are bit-identical whether it is armed or not.
//!
//! [`Ppu::write_attribution`]: crate::Ppu
//!
//! # Deliberate scope limit
//!
//! CHR writes (`$0000-$1FFF`) are **not** attributed. That window is owned by the
//! mapper — it may be ROM (undriveable), CHR-RAM, or a board-specific window with
//! its own banking — so a byte offset here is not a stable identity across a bank
//! switch the way CIRAM, palette RAM, and OAM offsets are. Pattern-table
//! provenance is reported as "supplied by mapper bank N", which the mapper
//! already knows, rather than being faked with an unstable offset.

extern crate alloc;

use alloc::boxed::Box;
use alloc::vec;

/// Number of attributed CIRAM bytes — the 2 KiB of internal nametable RAM.
///
/// Mapper-supplied nametable memory (`MMC5` `ExRAM`, 4-screen boards) is written
/// through [`crate::PpuBus::write_nametable`] and is not covered; see the module
/// docs.
pub const CIRAM_LEN: usize = 0x0800;

/// Number of attributed OAM bytes (64 sprites x 4).
pub const OAM_LEN: usize = 0x0100;

/// Number of attributed palette-RAM entries.
///
/// Indices are post-mirroring (the `$3F10/$14/$18/$1C` aliases fold onto
/// `$3F00/$04/$08/$0C`), so an attribution looked up through either address
/// returns the same record — which is correct, because they *are* the same byte.
pub const PALETTE_LEN: usize = 0x20;

/// One write-attribution record: who wrote a byte, when, and with what.
///
/// `cycle` is the CPU cycle counter at the instruction that performed the write,
/// which is what makes a record comparable against the Trace Logger and the
/// Event Viewer for the same frame. It is not the PPU dot — the PPU position is
/// recoverable from the cycle and is already carried by the Event Viewer.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub struct WriteAttrib {
    /// CPU cycle count at the writing instruction's opcode fetch.
    pub cycle: u64,
    /// Program counter of the writing instruction.
    ///
    /// For an OAM DMA burst this is the PC of the `STA $4014` that triggered it,
    /// not a synthetic per-byte PC — 256 bytes genuinely share one cause.
    pub pc: u16,
    /// The byte value stored, **as stored**. Palette writes are masked to 6 bits
    /// before recording, so this matches what a subsequent read returns rather
    /// than what the CPU put on the bus.
    pub value: u8,
}

/// One attribution slot: a [`WriteAttrib`] plus whether anything has been
/// written yet.
///
/// A `written` flag rather than a sentinel cycle, because cycle 0 is a legitimate
/// value — the reset sequence performs real writes — and a sentinel would
/// silently misreport the earliest writes in a run as "never written".
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
struct Slot {
    rec: WriteAttribInner,
    written: bool,
}

/// Storage-side mirror of [`WriteAttrib`] with a `Default`, so a slot array can
/// be built without inventing a meaningless public default PC.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq, Hash)]
struct WriteAttribInner {
    cycle: u64,
    pc: u16,
    value: u8,
}

impl Slot {
    const fn get(self) -> Option<WriteAttrib> {
        if self.written {
            Some(WriteAttrib {
                cycle: self.rec.cycle,
                pc: self.rec.pc,
                value: self.rec.value,
            })
        } else {
            None
        }
    }

    const fn set(&mut self, pc: u16, cycle: u64, value: u8) {
        self.rec = WriteAttribInner { cycle, pc, value };
        self.written = true;
    }
}

/// Per-byte write attribution for the PPU's own memories.
///
/// Construct via [`WriteAttribution::new`]. Every index is masked, so no accessor
/// can panic on an out-of-range offset — a provenance query is a debugging
/// convenience and must never be able to take down the emulator it is inspecting.
#[derive(Clone, Debug)]
pub struct WriteAttribution {
    ciram: Box<[Slot]>,
    oam: Box<[Slot]>,
    palette: [Slot; PALETTE_LEN],
}

impl WriteAttribution {
    /// Heap cost of one armed store, in bytes. Reported by the UI so the memory
    /// price of arming is visible rather than folded into general process growth.
    pub const HEAP_BYTES: usize = (CIRAM_LEN + OAM_LEN) * size_of::<Slot>();

    /// Allocate a cleared attribution store.
    #[must_use]
    pub fn new() -> Self {
        Self {
            ciram: vec![Slot::default(); CIRAM_LEN].into_boxed_slice(),
            oam: vec![Slot::default(); OAM_LEN].into_boxed_slice(),
            palette: [Slot::default(); PALETTE_LEN],
        }
    }

    /// Forget every recorded write, keeping the allocation.
    ///
    /// Called on power-cycle and on save-state restore: attribution describes the
    /// history of *this* run, and a restored state's bytes were not written by
    /// any instruction this session executed. Reporting stale PCs after a restore
    /// would be worse than reporting nothing.
    pub fn clear(&mut self) {
        self.ciram.fill(Slot::default());
        self.oam.fill(Slot::default());
        self.palette = [Slot::default(); PALETTE_LEN];
    }

    /// Record a write to internal nametable RAM at physical offset `off`
    /// (mirroring already resolved by the caller).
    pub fn record_ciram(&mut self, off: usize, pc: u16, cycle: u64, value: u8) {
        self.ciram[off & (CIRAM_LEN - 1)].set(pc, cycle, value);
    }

    /// Record a write to OAM at byte index `idx`.
    pub fn record_oam(&mut self, idx: u8, pc: u16, cycle: u64, value: u8) {
        self.oam[idx as usize].set(pc, cycle, value);
    }

    /// Record a write to palette RAM at post-mirroring index `idx`.
    pub const fn record_palette(&mut self, idx: usize, pc: u16, cycle: u64, value: u8) {
        self.palette[idx & (PALETTE_LEN - 1)].set(pc, cycle, value);
    }

    /// The write that last stored the CIRAM byte at physical offset `off`, or
    /// `None` if nothing has written it since the store was armed or cleared.
    #[must_use]
    pub fn ciram(&self, off: usize) -> Option<WriteAttrib> {
        self.ciram[off & (CIRAM_LEN - 1)].get()
    }

    /// The write that last stored OAM byte `idx`.
    #[must_use]
    pub fn oam(&self, idx: u8) -> Option<WriteAttrib> {
        self.oam[idx as usize].get()
    }

    /// The write that last stored palette entry `idx` (post-mirroring).
    #[must_use]
    pub const fn palette(&self, idx: usize) -> Option<WriteAttrib> {
        self.palette[idx & (PALETTE_LEN - 1)].get()
    }
}

impl Default for WriteAttribution {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn unwritten_slots_report_none() {
        let a = WriteAttribution::new();
        assert_eq!(a.ciram(0), None);
        assert_eq!(a.oam(0), None);
        assert_eq!(a.palette(0), None);
    }

    #[test]
    fn cycle_zero_is_a_real_record_not_a_sentinel() {
        // The reset sequence performs writes at low cycle counts; a store that
        // used `cycle == 0` as "never written" would erase them.
        let mut a = WriteAttribution::new();
        a.record_ciram(0x123, 0xC000, 0, 0x42);
        assert_eq!(
            a.ciram(0x123),
            Some(WriteAttrib {
                cycle: 0,
                pc: 0xC000,
                value: 0x42
            })
        );
    }

    #[test]
    fn indices_wrap_instead_of_panicking() {
        let mut a = WriteAttribution::new();
        a.record_ciram(CIRAM_LEN + 5, 0x8000, 7, 0x11);
        assert_eq!(a.ciram(5).map(|r| r.value), Some(0x11));
        a.record_palette(PALETTE_LEN + 3, 0x8001, 8, 0x22);
        assert_eq!(a.palette(3).map(|r| r.value), Some(0x22));
    }

    #[test]
    fn last_write_wins() {
        let mut a = WriteAttribution::new();
        a.record_oam(9, 0xA000, 10, 0x01);
        a.record_oam(9, 0xB000, 20, 0x02);
        assert_eq!(
            a.oam(9),
            Some(WriteAttrib {
                cycle: 20,
                pc: 0xB000,
                value: 0x02
            })
        );
    }

    #[test]
    fn clear_forgets_everything() {
        let mut a = WriteAttribution::new();
        a.record_ciram(1, 0x8000, 1, 1);
        a.record_oam(1, 0x8000, 1, 1);
        a.record_palette(1, 0x8000, 1, 1);
        a.clear();
        assert_eq!(a.ciram(1), None);
        assert_eq!(a.oam(1), None);
        assert_eq!(a.palette(1), None);
    }
}
