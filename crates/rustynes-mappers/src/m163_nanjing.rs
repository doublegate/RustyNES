// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 163: the Nanjing FC-001 board (v2.9.6 "Roster").
//!
//! Written from `nesdev_wiki/output/INES_Mapper_163.md` and its talk page.
//!
//! - `$6000-$7FFF`: 8 KiB of battery-backed PRG-RAM.
//! - `$8000-$FFFF`: one 32 KiB PRG bank, from `$5000` (A18-A15) and `$5200`
//!   (A20-A19). While the mode register's A bit is clear, PRG A15/A16 read
//!   `11`, which is why a reset boots in bank 3.
//! - CHR: 8 KiB of CHR-RAM. With `$5000` bit 7 set, CHR A12 is PPU A9
//!   latched on the last rise of PPU A13 instead of PPU A12. That puts the left
//!   pattern table on the top half of every nametable and the right one on
//!   the bottom half, whatever the scroll.
//! - Mirroring is hard-wired.
//!
//! **Modelling the A13 rise.** Every PPU bus read reaches the mapper as either
//! a pattern access (`ppu_read`, A13 = 0) or a nametable access
//! (`nametable_fetch`, A13 = 1). A13 therefore rises exactly on a nametable
//! access that follows a pattern access, and that access's A9 is latched. The
//! attribute fetch that follows a nametable fetch has no rise between them, so
//! it does not re-latch. That matters: attribute addresses (`$23C0+`) have A9
//! set.
//!
//! The mode register's B bit swaps D0 and D1 of writes to `$5000-$5200`
//! (the feedback register included). It does not swap writes to itself. The
//! page notes that 1 MiB boards wire both ASIC PRG A19 and A20 to ROM A19,
//! "effectively exempting this register from the bit-swap"; the `$5200` swap
//! is therefore skipped on 1 MiB images.

// Bank arithmetic narrows values already reduced modulo a bank count, and the
// accessors stay non-`const` to match the other mapper modules; both lints
// are allowed crate-wide in the sibling modules for the same reasons.
#![allow(clippy::cast_possible_truncation, clippy::missing_const_for_fn)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_32K: usize = 0x8000;
const CHR_RAM: usize = 0x2000;
const WRAM: usize = 0x2000;
const SAVE_STATE_VERSION: u8 = 1;

/// Mapper 163 (Nanjing FC-001).
pub struct Nanjing163 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    wram: Box<[u8]>,
    mirroring: Mirroring,
    /// `$5000`: PRG A18-A15 (bits 0-3), auto-switch enable (bit 7).
    reg_lo: u8,
    /// `$5200`: PRG A20-A19 (bits 0-1).
    reg_hi: u8,
    /// `$5300`: A (bit 2) and B (bit 0).
    mode: u8,
    /// The feedback bit F, read back inverted at `$5500`.
    feedback: bool,
    /// PPU A13 as of the last PPU access.
    a13: bool,
    /// PPU A9 latched on the last rise of A13.
    a9_latch: bool,
}

impl Nanjing163 {
    /// Construct the board.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 32 KiB.
    pub fn new(prg_rom: Box<[u8]>, mirroring: Mirroring) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 163 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_RAM].into_boxed_slice(),
            wram: vec![0u8; WRAM].into_boxed_slice(),
            mirroring,
            reg_lo: 0,
            reg_hi: 0,
            mode: 0,
            feedback: false,
            a13: false,
            a9_latch: false,
        })
    }

    /// D0/D1 swapped when the mode register's B bit is set.
    fn swap(&self, v: u8) -> u8 {
        if self.mode & 0x01 != 0 {
            (v & 0xFC) | ((v & 0x01) << 1) | ((v >> 1) & 0x01)
        } else {
            v
        }
    }

    fn prg_bank(&self) -> usize {
        let mut bank = usize::from(self.reg_lo & 0x0F) | (usize::from(self.reg_hi & 0x03) << 4);
        if self.mode & 0x04 == 0 {
            bank |= 0x03;
        }
        bank % (self.prg_rom.len() / PRG_BANK_32K)
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let a = usize::from(addr & 0x0FFF);
        let a12 = if self.reg_lo & 0x80 != 0 {
            self.a9_latch
        } else {
            addr & 0x1000 != 0
        };
        a | (usize::from(a12) << 12)
    }

    /// A nametable access: A13 rises if the previous access was a pattern one.
    fn on_nametable_access(&mut self, addr: u16) {
        if !self.a13 {
            self.a9_latch = addr & 0x0200 != 0;
        }
        self.a13 = true;
    }
}

impl Mapper for Nanjing163 {
    fn sram(&self) -> &[u8] {
        &self.wram
    }

    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.wram
    }

    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn has_hardwired_mirroring(&self) -> bool {
        true
    }

    /// "All registers are initialized to `$00` on reset."
    fn reset(&mut self) {
        self.reg_lo = 0;
        self.reg_hi = 0;
        self.mode = 0;
        self.feedback = false;
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match addr {
            0x4020..=0x5FFF => addr & 0xF300 != 0x5100 || addr < 0x5000,
            _ => false,
        }
    }

    fn cpu_read_driven_mask(&self, _addr: u16) -> u8 {
        0x04
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // `$5500-$5501`, mask `$F300`: the inverted F bit on D2.
            0x5000..=0x5FFF if addr & 0xF300 == 0x5100 => {
                if self.feedback {
                    0
                } else {
                    0x04
                }
            }
            0x6000..=0x7FFF => self.wram[usize::from(addr - 0x6000)],
            0x8000..=0xFFFF => {
                self.prg_rom[self.prg_bank() * PRG_BANK_32K + usize::from(addr & 0x7FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => self.wram[usize::from(addr - 0x6000)] = value,
            0x5000..=0x5FFF => match addr & 0xFF00 {
                0x5000 => self.reg_lo = self.swap(value),
                0x5100 => {
                    let v = self.swap(value);
                    if addr & 0x01 == 0 {
                        // A=0: F latched.
                        self.feedback = v & 0x04 != 0;
                    } else if v & 0x01 != 0 {
                        // A=1: flip F when E=1.
                        self.feedback = !self.feedback;
                    }
                }
                0x5200 => {
                    self.reg_hi = if self.prg_rom.len() == 1024 * 1024 {
                        value
                    } else {
                        self.swap(value)
                    };
                }
                0x5300 => self.mode = value,
                _ => {}
            },
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        if addr < 0x2000 {
            self.a13 = false;
            self.chr_ram[self.chr_offset(addr)]
        } else {
            0
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if addr < 0x2000 {
            self.a13 = false;
            let off = self.chr_offset(addr);
            self.chr_ram[off] = value;
        }
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        self.on_nametable_access(addr);
        None
    }

    fn nametable_write(&mut self, addr: u16, _value: u8) -> bool {
        self.on_nametable_access(addr);
        false
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = MapperDebugInfo {
            mapper_id: 163,
            name: "Nanjing FC-001 (163)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("32K".into(), format!("{:#04x}", self.prg_bank())));
        info.chr_banks.push((
            "auto".into(),
            format!(
                "{} (A9 latch {})",
                self.reg_lo >> 7,
                u8::from(self.a9_latch)
            ),
        ));
        info.extra
            .push(("mode".into(), format!("{:#04x}", self.mode)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(8 + CHR_RAM + WRAM);
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_lo);
        out.push(self.reg_hi);
        out.push(self.mode);
        out.push(u8::from(self.feedback));
        out.push(u8::from(self.a13));
        out.push(u8::from(self.a9_latch));
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.wram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 7 + CHR_RAM + WRAM;
        if data.len() != expected {
            return Err(MapperError::WrongLength {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg_lo = data[1];
        self.reg_hi = data[2];
        self.mode = data[3];
        self.feedback = data[4] != 0;
        self.a13 = data[5] != 0;
        self.a9_latch = data[6] != 0;
        self.chr_ram.copy_from_slice(&data[7..7 + CHR_RAM]);
        self.wram.copy_from_slice(&data[7 + CHR_RAM..]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(banks_32k: usize) -> Nanjing163 {
        let mut prg = vec![0u8; banks_32k * PRG_BANK_32K];
        for b in 0..banks_32k {
            prg[b * PRG_BANK_32K] = b as u8;
        }
        Nanjing163::new(prg.into_boxed_slice(), Mirroring::Vertical).unwrap()
    }

    #[test]
    fn boots_in_bank_3_until_the_a_bit_is_set() {
        let mut m = board(32);
        assert_eq!(m.cpu_read(0x8000), 3);
        m.cpu_write(0x5000, 0x04);
        assert_eq!(m.cpu_read(0x8000), 0x07, "A clear: A15/A16 forced to 11");
        m.cpu_write(0x5300, 0x04);
        assert_eq!(m.cpu_read(0x8000), 0x04);
        m.cpu_write(0x5200, 0x01);
        assert_eq!(m.cpu_read(0x8000), 0x14, "$5200 bits are A19-A20");
        m.reset();
        assert_eq!(m.cpu_read(0x8000), 3);
    }

    #[test]
    fn b_bit_swaps_d0_d1_on_5000_to_5200_but_not_5300() {
        let mut m = board(64); // 2 MiB: the `$5200` swap applies.
        m.cpu_write(0x5300, 0x05); // A and B set; its own D0 is not swapped.
        m.cpu_write(0x5000, 0x01);
        assert_eq!(m.cpu_read(0x8000), 0x02);
        m.cpu_write(0x5200, 0x02);
        assert_eq!(m.cpu_read(0x8000), 0x12);
    }

    #[test]
    fn one_mib_boards_do_not_swap_5200() {
        let mut m = board(32); // 1 MiB.
        m.cpu_write(0x5300, 0x05);
        m.cpu_write(0x5200, 0x01);
        assert_eq!(m.cpu_read(0x8000) >> 4, 1);
    }

    #[test]
    fn feedback_is_read_back_inverted_and_flipped_by_5101() {
        let mut m = board(4);
        assert_eq!(m.cpu_read(0x5500), 0x04, "F=0 reads back as 1");
        assert_eq!(m.cpu_read_driven_mask(0x5500), 0x04);
        assert!(!m.cpu_read_unmapped(0x5501));
        m.cpu_write(0x5100, 0x04);
        assert_eq!(m.cpu_read(0x5500), 0x00);
        m.cpu_write(0x5101, 0x01);
        assert_eq!(m.cpu_read(0x5500), 0x04, "E=1 at $5101 flips F");
        m.cpu_write(0x5101, 0x04);
        assert_eq!(m.cpu_read(0x5500), 0x04, "E=0: F kept, D2 ignored");
    }

    /// With auto-switch on, a nametable fetch in the top half selects the
    /// left pattern table and one in the bottom half the right, and the
    /// attribute fetch (A9 set) that follows does not re-latch.
    #[test]
    fn auto_switch_latches_a9_on_the_a13_rise() {
        let mut m = board(4);
        m.chr_ram[0x0010] = 0x11;
        m.chr_ram[0x1010] = 0x22;
        m.cpu_write(0x5000, 0x80);
        let _ = m.ppu_read(0x0000);
        let _ = m.nametable_fetch(0x2020); // top half: A9 = 0
        let _ = m.nametable_fetch(0x23C1); // attribute: no rise
        assert_eq!(m.ppu_read(0x1010), 0x11, "left table despite PPU A12=1");
        let _ = m.nametable_fetch(0x2220); // after a pattern access: rises
        assert_eq!(m.ppu_read(0x0010), 0x22, "right table despite PPU A12=0");
        let _ = m.nametable_fetch(0x2020); // rises: latches 0
        let _ = m.nametable_fetch(0x2220); // A13 already high: no re-latch
        assert_eq!(m.ppu_read(0x0010), 0x11, "no rise, no re-latch");
        m.cpu_write(0x5000, 0x00);
        assert_eq!(m.ppu_read(0x0010), 0x11, "auto-switch off: PPU A12");
    }

    #[test]
    fn wram_and_state_round_trip() {
        let mut a = board(8);
        a.cpu_write(0x6005, 0x99);
        a.cpu_write(0x5300, 0x04);
        a.cpu_write(0x5000, 0x86);
        a.ppu_write(0x0123, 0x77);
        let blob = a.save_state();
        let mut b = board(8);
        b.load_state(&blob).unwrap();
        assert_eq!(b.cpu_read(0x6005), 0x99);
        assert_eq!(b.cpu_read(0x8000), a.cpu_read(0x8000));
        assert_eq!(b.save_state(), blob);
    }
}
