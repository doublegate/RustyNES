// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 228: Active Enterprises' *Action 52* and *Cheetahmen II* board
//! (v2.9.6 "Roster").
//!
//! Written from `nesdev_wiki/output/INES_Mapper_228.md`. One register, written
//! anywhere in `$8000-$FFFF`, is latched from the ADDRESS as well as the data:
//!
//! ```text
//! Address           Data
//! FEDCBA98 76543210 76543210
//! 1.MHHPPP PPS.CCCC ......CC
//!   ||||||  ||| ||||       ++- CHR bank bits 0-1
//!   ||||||  ||| ++++---------- CHR bank bits 2-5 (8 KiB CHR bank at $0000)
//!   ||||||  ||+--------------- PRG bank size: 0 = 32 KiB (P&~1 at $8000,
//!   ||||||  ||                 P|1 at $C000), 1 = the same 16 KiB twice
//!   |||+++--++---------------- 16 KiB PRG bank within the chip
//!   |++----------------------- which 512 KiB PRG chip
//!   +------------------------- mirroring: 0 vertical, 1 horizontal
//! ```
//!
//! *Action 52* has three 512 KiB PRG chips, 0, 1 and 3; chip 2 does not exist
//! and selecting it reads open bus. Its `.nes` image stores the three chips
//! back to back (1.5 MiB), so chip 3 is the image's third 512 KiB. Any other
//! image size is indexed directly and wrapped.
//!
//! The games "expect `$00` to be written to `$8000` on powerup/reset", so a
//! reset clears the register ([`Mapper::reset`]). The rumoured 4x4-bit RAM at
//! `$4020-$5FFF` "is definitely not present on either cartridge", so that
//! window floats.

// Bank arithmetic narrows values already reduced modulo a bank count, and the
// accessors stay non-`const` to match the other mapper modules; both lints
// are allowed crate-wide in the sibling modules for the same reasons.
#![allow(clippy::cast_possible_truncation, clippy::missing_const_for_fn)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;
const CHIP: usize = 512 * 1024;
const SAVE_STATE_VERSION: u8 = 1;

/// Mapper 228 (*Action 52* / *Cheetahmen II*).
pub struct Action52M228 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    /// The latched address, `$8000-$FFFF`.
    addr: u16,
    /// The latched data (CHR bits 0-1).
    data: u8,
}

impl Action52M228 {
    /// Construct the board.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 16 KiB
    /// or CHR is not a multiple of 8 KiB.
    pub fn new(prg_rom: Box<[u8]>, chr_rom: Box<[u8]>) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 228 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 228 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            chr_rom
        };
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 0x800].into_boxed_slice(),
            addr: 0x8000,
            data: 0,
        })
    }

    fn chip(&self) -> usize {
        usize::from((self.addr >> 11) & 0x03)
    }

    /// Chip 2 is absent on the three-chip *Action 52* image.
    fn chip_absent(&self) -> bool {
        self.prg_rom.len() == 3 * CHIP && self.chip() == 2
    }

    /// The byte offset in `prg_rom` for `addr` (`$8000-$FFFF`).
    fn prg_offset(&self, cpu: u16) -> usize {
        let page = usize::from((self.addr >> 6) & 0x1F);
        let page = if self.addr & 0x0020 != 0 {
            page
        } else if cpu < 0xC000 {
            page & !1
        } else {
            page | 1
        };
        let chip = match (self.prg_rom.len() == 3 * CHIP, self.chip()) {
            // The image stores chips 0, 1, 3.
            (true, 3) => 2,
            (_, c) => c,
        };
        let bank = (chip * 32 + page) % (self.prg_rom.len() / PRG_BANK_16K);
        bank * PRG_BANK_16K + (usize::from(cpu) & 0x3FFF)
    }

    fn chr_bank(&self) -> usize {
        let bank = (usize::from(self.addr & 0x0F) << 2) | usize::from(self.data & 0x03);
        bank % (self.chr.len() / CHR_BANK_8K)
    }

    fn mirroring(&self) -> Mirroring {
        if self.addr & 0x2000 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn nt_offset(&self, addr: u16) -> usize {
        let table = ((addr - 0x2000) / 0x400) & 0x03;
        self.mirroring().physical_bank(table as u8) * 0x400 + (usize::from(addr) & 0x3FF)
    }
}

impl Mapper for Action52M228 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn reset(&mut self) {
        self.addr = 0x8000;
        self.data = 0;
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match addr {
            0x4020..=0x7FFF => true,
            _ => self.chip_absent(),
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr >= 0x8000 && !self.chip_absent() {
            self.prg_rom[self.prg_offset(addr)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.addr = addr;
            self.data = value;
        }
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        if self.chr_is_ram {
            None
        } else {
            u32::try_from(self.chr_bank() * CHR_BANK_8K + usize::from(addr & 0x1FFF)).ok()
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_bank() * CHR_BANK_8K + usize::from(addr)],
            0x2000..=0x3EFF => self.vram[self.nt_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let off = self.chr_bank() * CHR_BANK_8K + usize::from(addr);
                self.chr[off] = value;
            }
            0x2000..=0x3EFF => {
                let off = self.nt_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        u16::try_from(self.nt_offset(addr)).unwrap_or(0)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring()
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = MapperDebugInfo {
            mapper_id: 228,
            name: "Action 52 (228)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("chip".into(), format!("{}", self.chip())));
        info.prg_banks
            .push(("page".into(), format!("{:#04x}", (self.addr >> 6) & 0x1F)));
        info.chr_banks
            .push(("bank".into(), format!("{:#04x}", self.chr_bank())));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + CHR_BANK_8K);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.addr.to_le_bytes());
        out.push(self.data);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 4 + self.vram.len() + chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.addr = u16::from_le_bytes([data[1], data[2]]) | 0x8000;
        self.data = data[3];
        self.vram.copy_from_slice(&data[4..4 + self.vram.len()]);
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[4 + self.vram.len()..]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Every 16 KiB PRG bank opens with its index; every 8 KiB CHR bank too.
    fn board(prg_16k: usize, chr_8k: usize) -> Action52M228 {
        let mut prg = vec![0u8; prg_16k * PRG_BANK_16K];
        for b in 0..prg_16k {
            prg[b * PRG_BANK_16K] = b as u8;
        }
        let mut chr = vec![0u8; chr_8k * CHR_BANK_8K];
        for b in 0..chr_8k {
            chr[b * CHR_BANK_8K] = b as u8;
        }
        Action52M228::new(prg.into_boxed_slice(), chr.into_boxed_slice()).unwrap()
    }

    /// The register address for the mirroring bit, chip, page, size bit and
    /// CHR high bits.
    fn reg(horizontal: bool, chip: u16, page: u16, mirrored_16k: bool, chr_hi: u16) -> u16 {
        0x8000
            | (u16::from(horizontal) << 13)
            | (chip << 11)
            | (page << 6)
            | (u16::from(mirrored_16k) << 5)
            | chr_hi
    }

    #[test]
    fn action52_chips_0_1_3_and_chip_2_is_open_bus() {
        let mut m = board(96, 64); // 1.5 MiB PRG, 512 KiB CHR.
        m.cpu_write(reg(false, 0, 5, true, 0), 0);
        assert_eq!(m.cpu_read(0x8000), 5);
        m.cpu_write(reg(false, 1, 5, true, 0), 0);
        assert_eq!(m.cpu_read(0x8000), 32 + 5);
        m.cpu_write(reg(false, 3, 5, true, 0), 0);
        assert_eq!(
            m.cpu_read(0x8000),
            64 + 5,
            "chip 3 is the image's third chip"
        );
        m.cpu_write(reg(false, 2, 5, true, 0), 0);
        assert!(m.cpu_read_unmapped(0x8000), "chip 2 does not exist");
        assert!(m.cpu_read_unmapped(0xC000));
    }

    #[test]
    fn prg_size_bit_selects_32k_or_mirrored_16k() {
        let mut m = board(96, 64);
        m.cpu_write(reg(false, 0, 7, false, 0), 0);
        assert_eq!(m.cpu_read(0x8000), 6, "32 KiB: P & ~1 at $8000");
        assert_eq!(m.cpu_read(0xC000), 7, "32 KiB: P | 1 at $C000");
        m.cpu_write(reg(false, 0, 6, true, 0), 0);
        assert_eq!(m.cpu_read(0x8000), 6);
        assert_eq!(m.cpu_read(0xC000), 6, "16 KiB mirrored");
    }

    #[test]
    fn chr_bank_takes_four_address_bits_and_two_data_bits() {
        let mut m = board(96, 64);
        m.cpu_write(reg(false, 0, 0, true, 0x0B), 0x02);
        assert_eq!(m.ppu_read(0x0000), (0x0B << 2) | 2);
        m.cpu_write(reg(false, 0, 0, true, 0x0B), 0xFD); // only D1-D0 count
        assert_eq!(m.ppu_read(0x0000), (0x0B << 2) | 1);
    }

    #[test]
    fn mirroring_bit_and_reset() {
        let mut m = board(96, 64);
        m.cpu_write(reg(true, 1, 3, true, 1), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.reset();
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        assert_eq!(m.cpu_read(0x8000), 0, "reset = a write of $00 to $8000");
        assert_eq!(m.ppu_read(0x0000), 0);
    }

    #[test]
    fn a_single_chip_image_is_indexed_directly() {
        // Cheetahmen II: 256 KiB PRG. Chip 2 is not special on it.
        let mut m = board(16, 8);
        m.cpu_write(reg(false, 2, 3, true, 0), 0);
        assert!(!m.cpu_read_unmapped(0x8000));
        assert_eq!(m.cpu_read(0x8000), 3, "(2*32 + 3) wraps to bank 3");
    }

    #[test]
    fn state_round_trips() {
        let mut a = board(96, 64);
        a.cpu_write(reg(true, 3, 9, false, 5), 1);
        a.ppu_write(0x2001, 0x44);
        let blob = a.save_state();
        let mut b = board(96, 64);
        b.load_state(&blob).unwrap();
        assert_eq!(b.cpu_read(0x8000), a.cpu_read(0x8000));
        assert_eq!(b.ppu_read(0x0000), a.ppu_read(0x0000));
        assert_eq!(b.ppu_read(0x2001), 0x44);
        assert_eq!(b.save_state(), blob);
    }
}
