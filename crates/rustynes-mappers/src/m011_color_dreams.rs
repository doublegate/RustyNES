//! Color Dreams (mapper 11) -- unlicensed discrete-logic board.
//!
//! A single write-anywhere register in `$8000-$FFFF`: the low two bits select
//! a 32 KiB PRG bank, the high four bits an 8 KiB CHR bank. Because the board
//! is discrete logic with no write-enable gating, writes are subject to *bus
//! conflicts* -- the value written is `AND`ed with the byte the PRG ROM is
//! simultaneously driving onto the data bus at that address.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::needless_pass_by_ref_mut,
    clippy::manual_range_patterns,
    clippy::match_same_arms,
    clippy::too_many_arguments
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

/// Version byte this board writes in its mapper save-state section.
///
/// **v1** (through v2.9.1) carried the PRG and CHR bank registers and the
/// 2 KiB nametable RAM. On a cartridge with no CHR-ROM the 8 KiB CHR-RAM was
/// left out, and the `.rns` container has no other section that carries
/// cartridge RAM -- so every save-state load, rewind step, run-ahead frame and
/// netplay rollback kept the running game's CHR-RAM instead of the saved one
/// (the v2.9.2 cartridge-RAM sweep; the same omission core audit AUD-02 found
/// on the Konami VRC boards). **v2** appends the CHR-RAM when present.
/// Since v2.9.8 (ADR 0042)
/// `load_state` reads v2 only and refuses a v1 blob, which it used to load
/// with the RAM left untouched.
const COLOR_DREAMS_SECTION_VERSION: u8 = 2;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// Color Dreams (Mapper 11).
pub struct ColorDreams {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl ColorDreams {
    /// Construct a new Color Dreams mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(32 * 1024) {
            return Err(MapperError::Invalid(format!(
                "Color Dreams PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Color Dreams CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for ColorDreams {
    /// Fixed mirroring (v2.7.2, core audit §5.6): `nesdev_wiki/Color_Dreams: "Fixed H or V, controlled by solder pads"`, so a
    /// per-game database correction of a wrong header bit is safe here.
    fn has_hardwired_mirroring(&self) -> bool {
        true
    }

    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr < 0x8000 {
            return 0;
        }
        let total_32k = (self.prg_rom.len() / (32 * 1024)).max(1);
        let bank = (self.prg_bank as usize) % total_32k;
        let off = bank * 32 * 1024 + (addr as usize - 0x8000);
        self.prg_rom[off % self.prg_rom.len()]
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            // Bus conflict: AND with the current PRG byte at this address.
            let conflict = self.cpu_read(addr);
            let v = value & conflict;
            self.prg_bank = v & 0x03;
            self.chr_bank = (v >> 4) & 0x0F;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let total_8k = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % total_8k;
                self.chr_rom[(bank * CHR_BANK_8K + addr as usize) % self.chr_rom.len()]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring) % self.vram.len();
            self.vram[off] = value;
        } else if (0x0000..=0x1FFF).contains(&addr) && self.chr_is_ram {
            let total_8k = (self.chr_rom.len() / CHR_BANK_8K).max(1);
            let bank = (self.chr_bank as usize) % total_8k;
            let off = (bank * CHR_BANK_8K + addr as usize) % self.chr_rom.len();
            self.chr_rom[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + self.ram_block_len());
        out.push(COLOR_DREAMS_SECTION_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        // --- v2 tail: the CHR-RAM, if any (see `COLOR_DREAMS_SECTION_VERSION`) ---
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let version = data.first().copied().unwrap_or(0);
        // Only the current layout is read (v2.9.8, ADR 0042). A v1 blob, which
        // stopped before the RAM block, is refused rather than loaded with the
        // RAM left as it was.
        if version != COLOR_DREAMS_SECTION_VERSION {
            return Err(MapperError::UnsupportedVersion(version));
        }
        let ram_len = self.ram_block_len();
        // The whole length is validated before the first field is written.
        let core_len = 3 + self.vram.len();
        let expected = core_len + ram_len;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        self.prg_bank = data[1];
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..core_len]);
        if self.chr_is_ram {
            self.chr_rom.copy_from_slice(&data[core_len..]);
        }
        Ok(())
    }
}

impl ColorDreams {
    /// Bytes the v2 tail adds: the 8 KiB CHR-RAM when the cartridge has no
    /// CHR-ROM, else nothing. Derived from the loaded ROM, so a save and its
    /// load (same ROM, checked by the `.rns` hash tag) agree.
    fn ram_block_len(&self) -> usize {
        if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_dreams_bus_conflict() {
        let mut prg = vec![0u8; 32 * 1024];
        // Make ROM byte at $8000 = 0x55 -> AND with 0xFF gives 0x55.
        prg[0] = 0x55;
        let m_prg: Box<[u8]> = prg.into_boxed_slice();
        let chr = vec![0u8; 8 * 1024].into_boxed_slice();
        let mut m = ColorDreams::new(m_prg, chr, Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0xFF);
        // Effective value = 0xFF & 0x55 = 0x55. PRG bank = 0x55 & 0x03 = 1.
        assert_eq!(m.prg_bank, 1);
    }

    /// v2.9.2 cartridge-RAM sweep: the section carries the 8 KiB CHR-RAM of
    /// a board with no CHR-ROM. The whole-machine pin is
    /// `rustynes_core::nes::tests::every_board_snapshot_carries_cartridge_ram`.
    #[test]
    fn color_dreams_save_state_carries_chr_ram() {
        let mut m = ColorDreams::new(
            vec![0u8; 32 * 1024].into_boxed_slice(),
            Box::new([]),
            Mirroring::Vertical,
        )
        .unwrap();
        m.chr_rom[0x0000] = 0x11;
        m.chr_rom[0x1FFF] = 0x22;
        let blob = m.save_state();
        let mut m2 = ColorDreams::new(
            vec![0u8; 32 * 1024].into_boxed_slice(),
            Box::new([]),
            Mirroring::Vertical,
        )
        .unwrap();
        m2.load_state(&blob).expect("round-trip");
        assert_eq!(m2.chr_rom[0x0000], 0x11);
        assert_eq!(m2.chr_rom[0x1FFF], 0x22);
    }

    /// v2.9.8 (ADR 0042): a v1 blob (no RAM tail, written through v2.9.1)
    /// is refused. Until then it loaded and left the RAM as it was.
    #[test]
    fn color_dreams_v1_blob_is_refused() {
        let mut m = ColorDreams::new(
            vec![0u8; 32 * 1024].into_boxed_slice(),
            Box::new([]),
            Mirroring::Vertical,
        )
        .unwrap();
        m.chr_bank = 5;
        let core_len = 3 + m.vram.len();
        let mut v1 = m.save_state()[..core_len].to_vec();
        v1[0] = 1;
        let mut m2 = ColorDreams::new(
            vec![0u8; 32 * 1024].into_boxed_slice(),
            Box::new([]),
            Mirroring::Vertical,
        )
        .unwrap();
        assert!(matches!(
            m2.load_state(&v1),
            Err(MapperError::UnsupportedVersion(1))
        ));
    }

    /// A v2 blob one byte short (inside the CHR-RAM tail) is rejected.
    #[test]
    fn color_dreams_truncated_chr_ram_tail_is_rejected() {
        let m = ColorDreams::new(
            vec![0u8; 32 * 1024].into_boxed_slice(),
            Box::new([]),
            Mirroring::Vertical,
        )
        .unwrap();
        let blob = m.save_state();
        let mut m2 = ColorDreams::new(
            vec![0u8; 32 * 1024].into_boxed_slice(),
            Box::new([]),
            Mirroring::Vertical,
        )
        .unwrap();
        let err = m2
            .load_state(&blob[..blob.len() - 1])
            .expect_err("a truncated v2 blob must be rejected");
        assert!(matches!(err, MapperError::Truncated { .. }), "{err:?}");
    }
}
