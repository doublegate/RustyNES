//! BNROM and NINA-001 (mapper 34) -- two incompatible boards sharing one
//! mapper number.
//!
//! iNES mapper 34 is overloaded. **BNROM** (Nintendo/Irem) has a single
//! write-anywhere `$8000-$FFFF` register selecting a 32 KiB PRG bank, with
//! CHR-RAM and no CHR banking. **NINA-001** (AVE) instead decodes three
//! registers in the PRG-RAM window at `$7FFD-$7FFF` -- one 32 KiB PRG select
//! and two 4 KiB CHR selects -- and carries CHR-ROM.
//!
//! The two are told apart by CHR-ROM presence (a NINA-001 board has it; a
//! BNROM board cannot), captured in [`M34Variant`].
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

const CHR_BANK_4K: usize = 0x1000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

/// Version byte this board writes in its mapper save-state section.
///
/// **v1** (through v2.9.1) carried the PRG and CHR bank registers, the
/// variant, the 8 KiB PRG-RAM and the 2 KiB nametable RAM. The 8 KiB CHR-RAM
/// -- which every BNROM board has, *Deadly Towers* among them -- was left
/// out, and the `.rns` container has no other section that carries cartridge
/// RAM, so every save-state load, rewind step, run-ahead frame and netplay
/// rollback kept the running game's tiles instead of the saved ones (the
/// v2.9.2 cartridge-RAM sweep; the same omission core audit AUD-02 found on
/// the Konami VRC boards). **v2** appends the CHR-RAM when present.
/// Since v2.9.8 (ADR 0042)
/// `load_state` reads v2 only and refuses a v1 blob, which it used to load
/// with the RAM left untouched.
const M34_SECTION_VERSION: u8 = 2;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// Mapper 34 variant.
#[derive(Debug, Clone, Copy)]
pub enum M34Variant {
    /// BNROM: PRG-bank-only, no CHR banking.
    Bnrom,
    /// NINA-001: PRG bank @ $7FFD, CHR banks @ $7FFE / $7FFF.
    Nina001,
}

/// Mapper 34 (BNROM / NINA-001).
pub struct M34 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    prg_ram: Box<[u8]>,
    prg_bank: u8,
    chr_bank_lo: u8,
    chr_bank_hi: u8,
    variant: M34Variant,
    mirroring: Mirroring,
}

impl M34 {
    /// Construct a new M34 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        variant: M34Variant,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(32 * 1024) {
            return Err(MapperError::Invalid(format!(
                "Mapper 34 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_4K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Mapper 34 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; 8 * 1024].into_boxed_slice(),
            prg_bank: 0,
            chr_bank_lo: 0,
            chr_bank_hi: 0,
            variant,
            mirroring,
        })
    }
}

impl Mapper for M34 {
    /// Fixed mirroring (v2.7.2, core audit §5.6): `nesdev_wiki/INES_Mapper_034: "Fixed V" (BNROM and NINA-001 alike)`, so a
    /// per-game database correction of a wrong header bit is safe here.
    fn has_hardwired_mirroring(&self) -> bool {
        true
    }

    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize % self.prg_ram.len()],
            0x8000..=0xFFFF => {
                let total_32k = (self.prg_rom.len() / (32 * 1024)).max(1);
                let bank = (self.prg_bank as usize) % total_32k;
                self.prg_rom[(bank * 32 * 1024 + (addr as usize - 0x8000)) % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match (self.variant, addr) {
            (M34Variant::Nina001, 0x7FFD) => self.prg_bank = value & 0x01,
            (M34Variant::Nina001, 0x7FFE) => self.chr_bank_lo = value & 0x0F,
            (M34Variant::Nina001, 0x7FFF) => self.chr_bank_hi = value & 0x0F,
            (_, 0x6000..=0x7FFF) => {
                let off = (addr - 0x6000) as usize % self.prg_ram.len();
                self.prg_ram[off] = value;
            }
            (M34Variant::Bnrom, 0x8000..=0xFFFF) => self.prg_bank = value,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match (addr, self.variant) {
            (0x0000..=0x0FFF, M34Variant::Nina001) => {
                let total_4k = (self.chr.len() / CHR_BANK_4K).max(1);
                let bank = (self.chr_bank_lo as usize) % total_4k;
                self.chr[(bank * CHR_BANK_4K + addr as usize) % self.chr.len()]
            }
            (0x1000..=0x1FFF, M34Variant::Nina001) => {
                let total_4k = (self.chr.len() / CHR_BANK_4K).max(1);
                let bank = (self.chr_bank_hi as usize) % total_4k;
                self.chr[(bank * CHR_BANK_4K + (addr as usize - 0x1000)) % self.chr.len()]
            }
            (0x0000..=0x1FFF, _) => self.chr[addr as usize % self.chr.len()],
            (0x2000..=0x3EFF, _) => {
                self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()]
            }
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let len = self.chr.len();
                    self.chr[addr as usize % len] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(8 + self.prg_ram.len() + self.vram.len() + self.ram_block_len());
        out.push(M34_SECTION_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank_lo);
        out.push(self.chr_bank_hi);
        out.push(match self.variant {
            M34Variant::Bnrom => 0,
            M34Variant::Nina001 => 1,
        });
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        // --- v2 tail: the CHR-RAM, if any (see `M34_SECTION_VERSION`) ---
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let version = data.first().copied().unwrap_or(0);
        // Only the current layout is read (v2.9.8, ADR 0042). A v1 blob, which
        // stopped before the CHR-RAM block, is refused rather than loaded with the
        // CHR-RAM left as it was.
        if version != M34_SECTION_VERSION {
            return Err(MapperError::UnsupportedVersion(version));
        }
        let ram_len = self.ram_block_len();
        // The whole length is validated before the first field is written.
        let core_len = 5 + self.prg_ram.len() + self.vram.len();
        let expected = core_len + ram_len;
        if data.len() != expected {
            return Err(MapperError::WrongLength {
                expected,
                got: data.len(),
            });
        }
        self.prg_bank = data[1];
        self.chr_bank_lo = data[2];
        self.chr_bank_hi = data[3];
        self.variant = match data[4] {
            0 => M34Variant::Bnrom,
            1 => M34Variant::Nina001,
            other => return Err(MapperError::Invalid(format!("variant {other}"))),
        };
        let mut cur = 5usize;
        self.prg_ram
            .copy_from_slice(&data[cur..cur + self.prg_ram.len()]);
        cur += self.prg_ram.len();
        self.vram.copy_from_slice(&data[cur..core_len]);
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[core_len..]);
        }
        Ok(())
    }
}

impl M34 {
    /// Bytes the v2 tail adds: the 8 KiB CHR-RAM when the cartridge has no
    /// CHR-ROM (every BNROM board), else nothing. Derived from the loaded ROM,
    /// so a save and its load (same ROM, checked by the `.rns` hash tag) agree.
    fn ram_block_len(&self) -> usize {
        if self.chr_is_ram { self.chr.len() } else { 0 }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const PRG_BANK_8K: usize = 0x2000;

    fn synth(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_4k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_4K];
        for b in 0..banks {
            v[b * CHR_BANK_4K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m34_bnrom_swap() {
        let mut m = M34::new(
            synth(8),
            Box::new([]),
            Mirroring::Vertical,
            M34Variant::Bnrom,
        )
        .unwrap();
        // Default bank 0; $8000 -> 0.
        assert_eq!(m.cpu_read(0x8000), 0);
        // Test write with conflict; 32K banks here means bank index 1 -> byte at offset 32K = bank 4 of 8K banks.
        m.cpu_write(0x8000, 1);
        // Bank 1 in 32K terms = offset 32K. PRG[32768] = byte 4 of synth(8) = 4.
        assert_eq!(m.cpu_read(0x8000), 4);
    }

    #[test]
    fn m34_nina001_variant_register_layout() {
        // T-74-001 (Phase 7): NINA-001 (mapper 34 submapper 1) uses a distinct
        // register layout from BNROM — PRG bank at $7FFD, CHR lo/hi at
        // $7FFE/$7FFF — and must NOT respond to BNROM's $8000 PRG-bank write.
        let mut m = M34::new(
            synth(8),
            synth_chr_4k(8),
            Mirroring::Vertical,
            M34Variant::Nina001,
        )
        .unwrap();
        // PRG bank via $7FFD (1-bit). Bank 1 = 32K offset = 8K-bank 4 = byte 4.
        m.cpu_write(0x7FFD, 1);
        assert_eq!(m.cpu_read(0x8000), 4, "NINA-001 PRG bank selects via $7FFD");
        // A BNROM-style $8000 write must be ignored on NINA-001.
        m.cpu_write(0x8000, 0);
        assert_eq!(m.cpu_read(0x8000), 4, "$8000 write is ignored on NINA-001");
        // CHR lo/hi banks via $7FFE / $7FFF (each tagged with its index byte).
        m.cpu_write(0x7FFE, 2);
        assert_eq!(m.ppu_read(0x0000), 2, "NINA-001 CHR lo bank via $7FFE");
        m.cpu_write(0x7FFF, 3);
        assert_eq!(m.ppu_read(0x1000), 3, "NINA-001 CHR hi bank via $7FFF");
    }

    /// v2.9.2 cartridge-RAM sweep: the section carries the 8 KiB CHR-RAM of
    /// a board with no CHR-ROM. The whole-machine pin is
    /// `rustynes_core::nes::tests::every_board_snapshot_carries_cartridge_ram`.
    #[test]
    fn m34_save_state_carries_chr_ram() {
        let mut m = M34::new(
            synth(8),
            Box::new([]),
            Mirroring::Vertical,
            M34Variant::Bnrom,
        )
        .unwrap();
        m.chr[0x0000] = 0x11;
        m.chr[0x1FFF] = 0x22;
        let blob = m.save_state();
        let mut m2 = M34::new(
            synth(8),
            Box::new([]),
            Mirroring::Vertical,
            M34Variant::Bnrom,
        )
        .unwrap();
        m2.load_state(&blob).expect("round-trip");
        assert_eq!(m2.chr[0x0000], 0x11);
        assert_eq!(m2.chr[0x1FFF], 0x22);
    }

    /// v2.9.8 (ADR 0042): a v1 blob (no RAM tail, written through v2.9.1)
    /// is refused. Until then it loaded and left the RAM as it was.
    #[test]
    fn m34_v1_blob_is_refused() {
        let mut m = M34::new(
            synth(8),
            Box::new([]),
            Mirroring::Vertical,
            M34Variant::Bnrom,
        )
        .unwrap();
        m.prg_bank = 1;
        let core_len = 5 + m.prg_ram.len() + m.vram.len();
        let mut v1 = m.save_state()[..core_len].to_vec();
        v1[0] = 1;
        let mut m2 = M34::new(
            synth(8),
            Box::new([]),
            Mirroring::Vertical,
            M34Variant::Bnrom,
        )
        .unwrap();
        assert!(matches!(
            m2.load_state(&v1),
            Err(MapperError::UnsupportedVersion(1))
        ));
    }

    /// A v2 blob one byte short (inside the CHR-RAM tail) is rejected.
    #[test]
    fn m34_truncated_chr_ram_tail_is_rejected() {
        let m = M34::new(
            synth(8),
            Box::new([]),
            Mirroring::Vertical,
            M34Variant::Bnrom,
        )
        .unwrap();
        let blob = m.save_state();
        let mut m2 = M34::new(
            synth(8),
            Box::new([]),
            Mirroring::Vertical,
            M34Variant::Bnrom,
        )
        .unwrap();
        let err = m2
            .load_state(&blob[..blob.len() - 1])
            .expect_err("a truncated v2 blob must be rejected");
        assert!(matches!(err, MapperError::WrongLength { .. }), "{err:?}");
    }
}
