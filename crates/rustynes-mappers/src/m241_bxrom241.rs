//! `BxROM`-like pirate board (mapper 241), e.g. the Mortal Kombat bootlegs.
//!
//! The whole written byte selects a 32 KiB PRG bank -- no masking, no bus
//! conflict -- with CHR-RAM and fixed mirroring. Effectively BNROM with a
//! wider bank field; see `m034_bnrom_nina001.rs` for BNROM proper.
//!
//! A discrete-logic board in the shape of the stock mappers (`NROM`, `CNROM`,
//! `UxROM`, `GxROM`, `AxROM`): bank-select latch registers, no IRQ, no on-cart
//! audio. Banking / mirroring semantics are cross-checked against the
//! `GeraNES` reference emulator (cross-referenced, not copied)
//! and the nesdev wiki, and validated by register-decode + save-state unit
//! tests.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

/// v2 (v2.7.2) appends the 8 KiB WRAM; a v1 blob loads with it zeroed.
const SAVE_STATE_VERSION: u8 = 2;
const WRAM_SIZE: usize = 0x2000;

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

/// Mapper 241 (`BxROM`-like pirate board).
pub struct Bxrom241 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    mirroring: Mirroring,
    /// "8 KiB of WRAM at CPU $6000-$7FFF that can be battery-backed"
    /// (`nesdev_wiki/INES_Mapper_241.xhtml`). Absent before v2.7.2.
    wram: Box<[u8]>,
    /// Whether the header marks it battery-backed (then it is the save).
    battery: bool,
}

impl Bxrom241 {
    /// Construct a new mapper 241 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 241 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() == CHR_BANK_8K {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 241 expects 8 KiB CHR (RAM or ROM); got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            mirroring,
            wram: vec![0u8; WRAM_SIZE].into_boxed_slice(),
            battery: false,
        })
    }

    /// Mark the WRAM battery-backed (from the header), making it the save.
    #[must_use]
    pub const fn with_battery(mut self, battery: bool) -> Self {
        self.battery = battery;
        self
    }
}

impl Mapper for Bxrom241 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn sram(&self) -> &[u8] {
        if self.battery { &self.wram } else { &[] }
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        if self.battery {
            &mut self.wram
        } else {
            &mut []
        }
    }

    /// The WRAM is always present, so `$6000-$7FFF` stays mapped whether or
    /// not it is battery-backed.
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x6000..=0x7FFF).contains(&addr) {
            return self.wram[usize::from(addr - 0x6000)];
        }
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.wram[usize::from(addr - 0x6000)] = value;
            return;
        }
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = value;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    self.chr[addr as usize] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(2 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out.extend_from_slice(&self.wram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let version = *data.first().ok_or(MapperError::Truncated {
            expected: 1,
            got: 0,
        })?;
        let wram_len = match version {
            1 => 0,
            SAVE_STATE_VERSION => self.wram.len(),
            v => return Err(MapperError::UnsupportedVersion(v)),
        };
        let expected = 2 + self.vram.len() + chr_extra + wram_len;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        self.prg_bank = data[1];
        let mut cursor = 2;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr
                .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
            cursor += self.chr.len();
        }
        if wram_len == 0 {
            self.wram.fill(0);
        } else {
            self.wram.copy_from_slice(&data[cursor..cursor + wram_len]);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m241_full_byte_selects_32k_prg() {
        let mut m = Bxrom241::new(synth_prg_32k(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 5);
        assert_eq!(m.cpu_read(0x8000), 5);
        // No bus conflict: the written value sticks even though offset 0 of the
        // landing bank is not 0xFF.
        m.cpu_write(0xFFFF, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
    }

    #[test]
    fn m241_chr_ram_round_trip() {
        let mut m = Bxrom241::new(synth_prg_32k(2), Box::new([]), Mirroring::Vertical).unwrap();
        m.ppu_write(0x0010, 0xAB);
        assert_eq!(m.ppu_read(0x0010), 0xAB);
    }
}
