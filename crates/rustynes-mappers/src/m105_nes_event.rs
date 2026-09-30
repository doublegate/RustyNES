// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 105: NES-EVENT, the *Nintendo World Championships 1990* board
//! (v2.9.6 "Roster").
//!
//! Written from `nesdev_wiki/output/NES_EVENT.md` (the page, Disch's notes on
//! it, and its hardware notes). The board is an MMC1 with its CHR lines
//! rewired, two 128 KiB PRG EPROMs, 8 KiB of CHR-RAM, 8 KiB of PRG-RAM, and a
//! 30-bit M2 counter.
//!
//! The MMC1 is the project's own [`Mmc1`]: its serial port (the five-write
//! protocol, the reset bit, the consecutive-write filter) is exactly the
//! chip's. This board reads the four resulting registers and wires them:
//!
//! - **`$A000` (CHR bank 0)** is `...I OAA.`. `I` controls the timer and the
//!   lock, `O` picks the PRG chip, and `AA` is a 32 KiB bank in the first chip.
//! - **PRG.** Until unlocked, the first 32 KiB of the first chip, "no matter
//!   what". Once unlocked, `O=0` takes the 32 KiB bank `AA` of the first chip.
//!   `O=1` banks the second chip the ordinary MMC1 way, from `$E000` and the
//!   `$8000` PRG mode.
//! - **Unlock.** Writing `I=0` then `I=1` unlocks PRG banking. Power-on and
//!   reset lock it again.
//! - **Timer.** While `I=0` the counter counts up every M2 cycle. `I=1` resets
//!   it to 0, holds it, and acknowledges the IRQ. It fires on reaching
//!   `$20000000 | DIP << 25`. The page's tournament setting has switch C
//!   closed (DIP `%0100`, about 6.25 minutes on NTSC), and that is the
//!   default here; [`NesEvent105::set_dip`] changes it.
//! - **CHR** is 8 KiB of RAM, never banked: the MMC1's CHR registers are
//!   rewired to the lines above.
//! - **WRAM** at `$6000-$7FFF` obeys `$E000` bit 4, as on any MMC1.

// Bank arithmetic narrows values already reduced modulo a bank count, and the
// accessors stay non-`const` to match the other mapper modules; both lints
// are allowed crate-wide in the sibling modules for the same reasons.
#![allow(clippy::cast_possible_truncation, clippy::missing_const_for_fn)]

use crate::cartridge::Mirroring;
use crate::m001_mmc1::Mmc1;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_16K: usize = 0x4000;
const CHIP: usize = 0x2_0000;
const CHR_RAM: usize = 0x2000;
const WRAM: usize = 0x2000;
const SAVE_STATE_VERSION: u8 = 1;

/// The tournament DIP setting: switch C closed.
pub const NWC_TOURNAMENT_DIP: u8 = 0b0100;

/// Mapper 105 (NES-EVENT).
pub struct NesEvent105 {
    mmc1: Mmc1,
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    wram: Box<[u8]>,
    unlocked: bool,
    /// `I=0` has been seen since power-on or reset (the first half of the
    /// unlock sequence).
    seen_i_low: bool,
    counter: u32,
    irq_pending: bool,
    dip: u8,
}

impl NesEvent105 {
    /// Construct the board.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 32 KiB.
    pub fn new(prg_rom: Box<[u8]>, mirroring: Mirroring) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(2 * PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 105 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        // The embedded MMC1 only runs the serial port and the nametables; it
        // is given a minimal ROM it never reads and no PRG-RAM of its own.
        let mmc1 = Mmc1::new(
            vec![0u8; 2 * PRG_BANK_16K].into_boxed_slice(),
            Box::new([]),
            mirroring,
            0,
        )?;
        Ok(Self {
            mmc1,
            prg_rom,
            chr_ram: vec![0u8; CHR_RAM].into_boxed_slice(),
            wram: vec![0u8; WRAM].into_boxed_slice(),
            unlocked: false,
            seen_i_low: false,
            counter: 0,
            irq_pending: false,
            dip: NWC_TOURNAMENT_DIP,
        })
    }

    /// Set the four timer DIP switches (bits 28-25 of the target).
    pub fn set_dip(&mut self, dip: u8) {
        self.dip = dip & 0x0F;
    }

    fn target(&self) -> u32 {
        0x2000_0000 | (u32::from(self.dip) << 25)
    }

    fn i_bit(&self) -> bool {
        self.mmc1.registers().1 & 0x10 != 0
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let (control, chr0, _, prg) = self.mmc1.registers();
        let within = usize::from(addr & 0x3FFF);
        let high = addr & 0x4000 != 0;
        let bank16 = if !self.unlocked {
            usize::from(high)
        } else if chr0 & 0x08 == 0 {
            usize::from((chr0 >> 1) & 0x03) * 2 + usize::from(high)
        } else {
            let chip = CHIP / PRG_BANK_16K;
            let p = usize::from(prg & 0x07);
            let inner = match (control >> 2) & 0x03 {
                0 | 1 => (p & !1) + usize::from(high),
                2 => {
                    if high {
                        p
                    } else {
                        0
                    }
                }
                _ => {
                    if high {
                        chip - 1
                    } else {
                        p
                    }
                }
            };
            chip + inner
        };
        (bank16 * PRG_BANK_16K + within) % self.prg_rom.len()
    }

    fn wram_disabled(&self) -> bool {
        self.mmc1.registers().3 & 0x10 != 0
    }
}

impl Mapper for NesEvent105 {
    fn sram(&self) -> &[u8] {
        &self.wram
    }

    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.wram
    }

    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn reset(&mut self) {
        self.unlocked = false;
        self.seen_i_low = false;
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match addr {
            0x6000..=0x7FFF => self.wram_disabled(),
            _ => addr < 0x6000,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.wram[usize::from(addr - 0x6000)],
            0x8000..=0xFFFF => self.prg_rom[self.prg_offset(addr)],
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF if !self.wram_disabled() => {
                self.wram[usize::from(addr - 0x6000)] = value;
            }
            0x8000..=0xFFFF => {
                let before = self.mmc1.shift_count();
                self.mmc1.cpu_write(addr, value);
                let committed = before == 4 && self.mmc1.shift_count() == 0 && value & 0x80 == 0;
                if !(committed && addr & 0xE000 == 0xA000) {
                    return;
                }
                // A committed `$A000` write: the I bit's two jobs.
                if self.i_bit() {
                    self.counter = 0;
                    self.irq_pending = false;
                    if self.seen_i_low {
                        self.unlocked = true;
                    }
                } else {
                    self.seen_i_low = true;
                }
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        self.mmc1.notify_cpu_cycle();
        if self.i_bit() || self.irq_pending {
            return;
        }
        self.counter += 1;
        if self.counter >= self.target() {
            self.irq_pending = true;
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        if addr < 0x2000 {
            self.chr_ram[usize::from(addr)]
        } else {
            self.mmc1.ppu_read(addr)
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if addr < 0x2000 {
            self.chr_ram[usize::from(addr)] = value;
        } else {
            self.mmc1.ppu_write(addr, value);
        }
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        self.mmc1.nametable_address(addr)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mmc1.current_mirroring()
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = self.mmc1.debug_info();
        info.mapper_id = 105;
        info.name = "NES-EVENT (105)".into();
        info.irq_state
            .push(("timer".into(), format!("{:#010x}", self.counter)));
        info.irq_state
            .push(("target".into(), format!("{:#010x}", self.target())));
        info.extra
            .push(("unlocked".into(), format!("{}", self.unlocked)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let inner = self.mmc1.save_state();
        let mut out = Vec::with_capacity(16 + inner.len() + CHR_RAM + WRAM);
        out.push(SAVE_STATE_VERSION);
        out.push(u8::from(self.unlocked));
        out.push(u8::from(self.seen_i_low));
        out.extend_from_slice(&self.counter.to_le_bytes());
        out.push(u8::from(self.irq_pending));
        out.push(self.dip);
        out.extend_from_slice(&(inner.len() as u32).to_le_bytes());
        out.extend_from_slice(&inner);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.wram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        const HEAD: usize = 13;
        if data.len() < HEAD {
            return Err(MapperError::Truncated {
                expected: HEAD,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let inner_len = u32::from_le_bytes([data[9], data[10], data[11], data[12]]) as usize;
        let expected = HEAD + inner_len + CHR_RAM + WRAM;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        self.mmc1.load_state(&data[HEAD..HEAD + inner_len])?;
        self.unlocked = data[1] != 0;
        self.seen_i_low = data[2] != 0;
        self.counter = u32::from_le_bytes([data[3], data[4], data[5], data[6]]);
        self.irq_pending = data[7] != 0;
        self.dip = data[8] & 0x0F;
        let cur = HEAD + inner_len;
        self.chr_ram.copy_from_slice(&data[cur..cur + CHR_RAM]);
        self.wram.copy_from_slice(&data[cur + CHR_RAM..]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board() -> NesEvent105 {
        let mut prg = vec![0u8; 2 * CHIP];
        for b in 0..(2 * CHIP / PRG_BANK_16K) {
            prg[b * PRG_BANK_16K] = b as u8;
        }
        NesEvent105::new(prg.into_boxed_slice(), Mirroring::Horizontal).unwrap()
    }

    /// One serial-port register write, five bits LSB first.
    fn mmc1(m: &mut NesEvent105, addr: u16, value: u8) {
        for i in 0..5 {
            m.cpu_write(addr, (value >> i) & 1);
            // The consecutive-write filter needs a gap between writes.
            m.notify_cpu_cycle();
            m.notify_cpu_cycle();
        }
    }

    #[test]
    fn locked_to_the_first_32k_until_i_goes_low_then_high() {
        let mut m = board();
        mmc1(&mut m, 0xA000, 0x16); // I=1, O=0, AA=3: still locked
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 1);
        mmc1(&mut m, 0xA000, 0x06); // I=0
        assert_eq!(m.cpu_read(0x8000), 0, "I=0 alone does not unlock");
        mmc1(&mut m, 0xA000, 0x16); // I=1: unlocked
        assert_eq!(m.cpu_read(0x8000), 6, "32 KiB bank 3 of chip 0");
        assert_eq!(m.cpu_read(0xC000), 7);
        m.reset();
        assert_eq!(m.cpu_read(0x8000), 0, "reset locks it again");
    }

    #[test]
    fn o_bit_selects_chip_1_with_mmc1_banking() {
        let mut m = board();
        mmc1(&mut m, 0xA000, 0x00);
        mmc1(&mut m, 0xA000, 0x18); // I=1, O=1: unlocked, chip 1
        mmc1(&mut m, 0x8000, 0x0C); // PRG mode 3
        mmc1(&mut m, 0xE000, 0x02);
        assert_eq!(m.cpu_read(0x8000), 8 + 2);
        assert_eq!(m.cpu_read(0xC000), 15, "fixed last bank of chip 1");
        mmc1(&mut m, 0x8000, 0x08); // PRG mode 2
        assert_eq!(m.cpu_read(0x8000), 8);
        assert_eq!(m.cpu_read(0xC000), 10);
        mmc1(&mut m, 0x8000, 0x00); // 32 KiB mode
        mmc1(&mut m, 0xE000, 0x05);
        assert_eq!(m.cpu_read(0x8000), 8 + 4);
        assert_eq!(m.cpu_read(0xC000), 8 + 5);
    }

    #[test]
    fn timer_counts_while_i_is_low_and_fires_at_the_dip_target() {
        let mut m = board();
        m.set_dip(0);
        mmc1(&mut m, 0xA000, 0x00); // I=0: counting from here
        m.counter = 0x2000_0000 - 3;
        m.notify_cpu_cycle();
        m.notify_cpu_cycle();
        assert!(!m.irq_pending());
        m.notify_cpu_cycle();
        assert!(m.irq_pending(), "$20000000 with every switch open");
        mmc1(&mut m, 0xA000, 0x10); // I=1: acknowledge + reset + hold
        assert!(!m.irq_pending());
        assert_eq!(m.counter, 0);
        m.notify_cpu_cycle();
        assert_eq!(m.counter, 0, "held while I=1");
    }

    #[test]
    fn the_tournament_dip_is_the_default() {
        let m = board();
        assert_eq!(m.target(), 0x2800_0000);
    }

    #[test]
    fn chr_is_unbanked_ram_and_wram_obeys_e000_bit4() {
        let mut m = board();
        mmc1(&mut m, 0x8000, 0x10); // 4 KiB CHR mode: must not bank anything
        mmc1(&mut m, 0xC000, 0x01);
        m.ppu_write(0x1234, 0x5A);
        assert_eq!(m.ppu_read(0x1234), 0x5A);
        m.cpu_write(0x6000, 0x77);
        assert_eq!(m.cpu_read(0x6000), 0x77);
        mmc1(&mut m, 0xE000, 0x10);
        assert!(m.cpu_read_unmapped(0x6000));
    }

    #[test]
    fn state_round_trips() {
        let mut a = board();
        mmc1(&mut a, 0xA000, 0x00);
        mmc1(&mut a, 0xA000, 0x1A);
        a.ppu_write(0x0001, 3);
        a.cpu_write(0x6001, 4);
        let blob = a.save_state();
        let mut b = board();
        b.load_state(&blob).unwrap();
        assert_eq!(b.cpu_read(0x8000), a.cpu_read(0x8000));
        assert_eq!(b.save_state(), blob);
    }
}
