//! Konami VRC1 (mapper 75) -- the first and simplest VRC ASIC.
//!
//! Three 8 KiB PRG banks plus a fixed last bank, two 4 KiB CHR banks, and
//! mirroring control. The quirk worth knowing: each CHR bank register is
//! only four bits wide, and its *fifth* bit lives in the mirroring register
//! at `$9000` -- so a CHR bank select above 15 requires writing two
//! different registers.
//!
//! Unlike VRC2/VRC4/VRC6/VRC7 there is no IRQ counter and no on-cart audio;
//! see `m022_vrc2.rs`, `m021_vrc4.rs`, `m073_vrc3.rs`, `m024_vrc6.rs`, `m085_vrc7.rs` for those.
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

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_4K: usize = 0x1000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

/// Version byte this board writes in its mapper save-state section. Shared by
/// mapper 151 (Konami VS), which wraps this core and forwards its section.
///
/// **v1** (through v2.9.1) carried the three PRG banks, the CHR bank
/// registers, the mirroring and the 2 KiB nametable RAM. On a cartridge with
/// no CHR-ROM the 8 KiB CHR-RAM was left out, and the `.rns` container has no
/// other section that carries cartridge RAM -- so every save-state load,
/// rewind step, run-ahead frame and netplay rollback kept the running game's
/// CHR-RAM instead of the saved one (the v2.9.2 cartridge-RAM sweep; the same
/// omission core audit AUD-02 found on the other Konami VRC boards). **v2**
/// appends the CHR-RAM when present. Since v2.9.8 (ADR 0042)
/// `load_state` reads v2 only and refuses a v1 blob, which it used to load
/// with the RAM left untouched.
const VRC1_SECTION_VERSION: u8 = 2;

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// VRC1 (Mapper 75).
pub struct Vrc1 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_banks: [u8; 3], // $8000, $A000, $C000
    chr_lo: u8,
    chr_hi: u8,
    chr_lo_msb: u8,
    chr_hi_msb: u8,
    mirroring: Mirroring,
}

impl Vrc1 {
    /// Construct a new VRC1 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "VRC1 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "VRC1 CHR-ROM size {} is not a multiple of 4 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom: chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_banks: [0, 1, 2],
            chr_lo: 0,
            chr_hi: 0,
            chr_lo_msb: 0,
            chr_hi_msb: 0,
            mirroring,
        })
    }
}

impl Mapper for Vrc1 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let total_8k = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last = total_8k - 1;
        let bank = match addr & 0xE000 {
            0x8000 => (self.prg_banks[0] as usize) % total_8k,
            0xA000 => (self.prg_banks[1] as usize) % total_8k,
            0xC000 => (self.prg_banks[2] as usize) % total_8k,
            0xE000 => last,
            _ => return 0,
        };
        self.prg_rom[(bank * PRG_BANK_8K + (addr as usize & 0x1FFF)) % self.prg_rom.len()]
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xF000 {
            0x8000 => self.prg_banks[0] = value & 0x0F,
            0x9000 => {
                // Mirroring (bit 0) + CHR MSB bits.
                self.mirroring = if value & 1 == 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
                self.chr_lo_msb = (value >> 1) & 1;
                self.chr_hi_msb = (value >> 2) & 1;
            }
            0xA000 => self.prg_banks[1] = value & 0x0F,
            0xC000 => self.prg_banks[2] = value & 0x0F,
            0xE000 => self.chr_lo = value & 0x0F,
            0xF000 => self.chr_hi = value & 0x0F,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x0FFF => {
                let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
                let bank = (((self.chr_lo_msb as usize) << 4) | (self.chr_lo as usize)) % total_4k;
                self.chr_rom[(bank * CHR_BANK_4K + addr as usize) % self.chr_rom.len()]
            }
            0x1000..=0x1FFF => {
                let total_4k = (self.chr_rom.len() / CHR_BANK_4K).max(1);
                let bank = (((self.chr_hi_msb as usize) << 4) | (self.chr_hi as usize)) % total_4k;
                self.chr_rom[(bank * CHR_BANK_4K + (addr as usize - 0x1000)) % self.chr_rom.len()]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let len = self.chr_rom.len();
                    self.chr_rom[addr as usize % len] = value;
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
        let mut out = Vec::with_capacity(16 + self.vram.len() + self.ram_block_len());
        out.push(VRC1_SECTION_VERSION);
        out.extend_from_slice(&self.prg_banks);
        out.push(self.chr_lo);
        out.push(self.chr_hi);
        out.push(self.chr_lo_msb);
        out.push(self.chr_hi_msb);
        out.push(self.mirroring as u8);
        out.extend_from_slice(&self.vram);
        // --- v2 tail: the CHR-RAM, if any (see `VRC1_SECTION_VERSION`) ---
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
        if version != VRC1_SECTION_VERSION {
            return Err(MapperError::UnsupportedVersion(version));
        }
        let ram_len = self.ram_block_len();
        // The whole length is validated before the first field is written.
        let core_len = 9 + self.vram.len();
        let expected = core_len + ram_len;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        self.prg_banks.copy_from_slice(&data[1..4]);
        self.chr_lo = data[4];
        self.chr_hi = data[5];
        self.chr_lo_msb = data[6];
        self.chr_hi_msb = data[7];
        self.mirroring = match data[8] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        self.vram.copy_from_slice(&data[9..core_len]);
        if self.chr_is_ram {
            self.chr_rom.copy_from_slice(&data[core_len..]);
        }
        Ok(())
    }
}

impl Vrc1 {
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
    fn vrc1_basic_banking() {
        let mut m = Vrc1::new(synth(8), synth_chr_4k(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $E000 is fixed last bank.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    /// v2.9.2 cartridge-RAM sweep: the section carries the 8 KiB CHR-RAM of
    /// a board with no CHR-ROM. The whole-machine pin is
    /// `rustynes_core::nes::tests::every_board_snapshot_carries_cartridge_ram`.
    #[test]
    fn vrc1_save_state_carries_chr_ram() {
        let mut m = Vrc1::new(synth(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.chr_rom[0x0000] = 0x11;
        m.chr_rom[0x1FFF] = 0x22;
        let blob = m.save_state();
        let mut m2 = Vrc1::new(synth(8), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).expect("round-trip");
        assert_eq!(m2.chr_rom[0x0000], 0x11);
        assert_eq!(m2.chr_rom[0x1FFF], 0x22);
    }

    /// v2.9.8 (ADR 0042): a v1 blob (no RAM tail, written through v2.9.1)
    /// is refused. Until then it loaded and left the RAM as it was.
    #[test]
    fn vrc1_v1_blob_is_refused() {
        let mut m = Vrc1::new(synth(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 3);
        let core_len = 9 + m.vram.len();
        let mut v1 = m.save_state()[..core_len].to_vec();
        v1[0] = 1;
        let mut m2 = Vrc1::new(synth(8), Box::new([]), Mirroring::Vertical).unwrap();
        assert!(matches!(
            m2.load_state(&v1),
            Err(MapperError::UnsupportedVersion(1))
        ));
    }

    /// A v2 blob one byte short (inside the CHR-RAM tail) is rejected.
    #[test]
    fn vrc1_truncated_chr_ram_tail_is_rejected() {
        let m = Vrc1::new(synth(8), Box::new([]), Mirroring::Vertical).unwrap();
        let blob = m.save_state();
        let mut m2 = Vrc1::new(synth(8), Box::new([]), Mirroring::Vertical).unwrap();
        let err = m2
            .load_state(&blob[..blob.len() - 1])
            .expect_err("a truncated v2 blob must be rejected");
        assert!(matches!(err, MapperError::Truncated { .. }), "{err:?}");
    }
}
