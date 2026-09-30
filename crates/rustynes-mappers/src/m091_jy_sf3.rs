// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 91: the *Super Fighter III* board and its J.Y. Company clones
//! (v2.9.6 "Roster").
//!
//! Written from `nesdev_wiki/output/INES_Mapper_091.md`.
//!
//! | | submapper 0 (JY830623C, YY840238C) | submapper 1 (EJ-006-1) |
//! |---|---|---|
//! | register mask | `$F003` | `$F007` |
//! | IRQ | 64 unfiltered rises of PPU A12 | 16-bit counter, down by 5 every 4th M2 |
//! | mirroring | hard-wired | `$6004` H / `$6005` V |
//! | outer bank | `$8000-$9FFF` address bits: A0 = CHR A19, A2-A1 = PRG A18-A17 | none |
//!
//! Banks: two 8 KiB PRG banks at `$8000`/`$A000` (`$7000`/`$7001`), the last
//! 16 KiB fixed at `$C000`, and four 2 KiB CHR banks (`$6000-$6003`).
//! Submapper 0's outer bank limits the inner PRG bank to 128 KiB (A13-A16),
//! so "the last bank" is the last 16 KiB of the selected 128 KiB block.
//!
//! **Submapper 1's IRQ** is described only as "clocked by the M2 signal with a
//! factor of 5/4, meaning counting down by five every fourth M2 cycle". The
//! page does not say when it asserts. This board asserts when a decrement would
//! take the counter below zero, then stops until `$7007` restarts it. That is
//! why mapper 91 submapper 1 is `BestEffort` in `tier.rs` while submapper 0 is
//! Curated.

// Bank arithmetic narrows values already reduced modulo a bank count, and the
// accessors stay non-`const` to match the other mapper modules; both lints
// are allowed crate-wide in the sibling modules for the same reasons.
// `Jy91` mirrors the board's independent flags (CHR-RAM, IRQ enable, IRQ
// line, A12 level) one to one; folding them would obscure that mapping.
#![allow(
    clippy::cast_possible_truncation,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_2K: usize = 0x0800;
const SAVE_STATE_VERSION: u8 = 1;

/// Submapper 0 fires after this many unfiltered PPU A12 rises.
const A12_RISES_PER_IRQ: u8 = 64;

/// Mapper 91 (JY830623C / EJ-006-1).
pub struct Jy91 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    submapper: u8,
    mirroring: Mirroring,
    chr_regs: [u8; 4],
    prg_regs: [u8; 2],
    /// Submapper 0 outer bank, from the `$8000-$9FFF` write address.
    outer: u8,
    irq_enabled: bool,
    irq_pending: bool,
    /// Submapper 0: A12 rises counted. Submapper 1: the 16-bit counter.
    irq_counter: u16,
    /// Submapper 1: M2 cycles since the last decrement (0-3).
    irq_prescale: u8,
    last_a12: bool,
}

impl Jy91 {
    /// Construct the board.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 16 KiB
    /// or CHR is not a multiple of 2 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        submapper: u8,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(2 * PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 91 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            return Err(MapperError::Invalid(format!(
                "mapper 91 CHR-ROM size {} is not a multiple of 2 KiB",
                chr_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr = if chr_is_ram {
            vec![0u8; 4 * CHR_BANK_2K].into_boxed_slice()
        } else {
            chr_rom
        };
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 0x800].into_boxed_slice(),
            submapper: u8::from(submapper == 1),
            mirroring,
            chr_regs: [0; 4],
            prg_regs: [0; 2],
            outer: 0,
            irq_enabled: false,
            irq_pending: false,
            irq_counter: 0,
            irq_prescale: 0,
            last_a12: false,
        })
    }

    fn prg_bank(&self, addr: u16) -> usize {
        let count = self.prg_rom.len() / PRG_BANK_8K;
        let bank = if self.submapper == 0 {
            let base = usize::from((self.outer >> 1) & 0x03) << 4;
            match addr & 0xE000 {
                0x8000 => base | usize::from(self.prg_regs[0] & 0x0F),
                0xA000 => base | usize::from(self.prg_regs[1] & 0x0F),
                0xC000 => base | 0x0E,
                _ => base | 0x0F,
            }
        } else {
            match addr & 0xE000 {
                0x8000 => usize::from(self.prg_regs[0]),
                0xA000 => usize::from(self.prg_regs[1]),
                0xC000 => count.saturating_sub(2),
                _ => count - 1,
            }
        };
        bank % count
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let slot = usize::from((addr >> 11) & 0x03);
        let mut bank = usize::from(self.chr_regs[slot]);
        if self.submapper == 0 {
            bank |= usize::from(self.outer & 0x01) << 8;
        }
        (bank % (self.chr.len() / CHR_BANK_2K)) * CHR_BANK_2K + usize::from(addr & 0x07FF)
    }

    fn nt_offset(&self, addr: u16) -> usize {
        let table = ((addr - 0x2000) / 0x400) & 0x03;
        self.mirroring.physical_bank(table as u8) * 0x400 + usize::from(addr & 0x3FF)
    }
}

impl Mapper for Jy91 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn has_hardwired_mirroring(&self) -> bool {
        self.submapper == 0
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if addr >= 0x8000 {
            self.prg_rom[self.prg_bank(addr) * PRG_BANK_8K + usize::from(addr & 0x1FFF)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                let mask = if self.submapper == 0 { 0xF003 } else { 0xF007 };
                match (addr & mask, self.submapper) {
                    (0x6000..=0x6003, _) => self.chr_regs[usize::from(addr & 0x03)] = value,
                    (0x6004, 1) => self.mirroring = Mirroring::Horizontal,
                    (0x6005, 1) => self.mirroring = Mirroring::Vertical,
                    (0x6006, 1) => {
                        self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(value);
                    }
                    (0x6007, 1) => {
                        self.irq_counter = (self.irq_counter & 0x00FF) | (u16::from(value) << 8);
                    }
                    (0x7000..=0x7001, _) => self.prg_regs[usize::from(addr & 0x01)] = value,
                    // `$7006` (`$7002` under submapper 0's mask): stop + acknowledge.
                    (0x7002, 0) | (0x7006, 1) => {
                        self.irq_enabled = false;
                        self.irq_pending = false;
                    }
                    // `$7007`: start / reset.
                    (0x7003, 0) | (0x7007, 1) => {
                        self.irq_enabled = true;
                        self.irq_pending = false;
                        self.irq_prescale = 0;
                        if self.submapper == 0 {
                            self.irq_counter = 0;
                        }
                    }
                    _ => {}
                }
            }
            0x8000..=0x9FFF if self.submapper == 0 => self.outer = (addr & 0x07) as u8,
            _ => {}
        }
    }

    fn notify_a12(&mut self, level: bool) {
        let rising = level && !self.last_a12;
        self.last_a12 = level;
        if self.submapper != 0 || !rising || !self.irq_enabled {
            return;
        }
        self.irq_counter += 1;
        if self.irq_counter >= u16::from(A12_RISES_PER_IRQ) {
            self.irq_pending = true;
            self.irq_enabled = false;
        }
    }

    fn notify_cpu_cycle(&mut self) {
        if self.submapper != 1 || !self.irq_enabled {
            return;
        }
        self.irq_prescale = (self.irq_prescale + 1) & 0x03;
        if self.irq_prescale == 0 {
            if self.irq_counter < 5 {
                self.irq_counter = 0;
                self.irq_pending = true;
                self.irq_enabled = false;
            } else {
                self.irq_counter -= 5;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        if self.chr_is_ram {
            None
        } else {
            u32::try_from(self.chr_offset(addr & 0x1FFF)).ok()
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[self.nt_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let off = self.chr_offset(addr);
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
        self.mirroring
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = MapperDebugInfo {
            mapper_id: 91,
            name: format!("J.Y. 91 (submapper {})", self.submapper),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        for (i, r) in self.prg_regs.iter().enumerate() {
            info.prg_banks.push((format!("P{i}"), format!("{r:#04x}")));
        }
        for (i, r) in self.chr_regs.iter().enumerate() {
            info.chr_banks.push((format!("C{i}"), format!("{r:#04x}")));
        }
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.extra
            .push(("outer".into(), format!("{:#04x}", self.outer)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(20 + self.vram.len() + self.chr.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.submapper);
        out.push(match self.mirroring {
            Mirroring::Horizontal => 0,
            _ => 1,
        });
        out.extend_from_slice(&self.chr_regs);
        out.extend_from_slice(&self.prg_regs);
        out.push(self.outer);
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(self.irq_prescale);
        out.push(u8::from(self.last_a12));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        const HEAD: usize = 16;
        let chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = HEAD + self.vram.len() + chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        if data[1] != self.submapper {
            return Err(MapperError::Invalid(format!(
                "state is for mapper 91 submapper {}, this board is {}",
                data[1], self.submapper
            )));
        }
        // Submapper 0 counts A12 rises up to `A12_RISES_PER_IRQ` and stops
        // there, so an enabled counter is below it and a stopped one at most
        // equal. Anything else loaded cleanly and overflowed `+= 1` on the
        // next rise. Submapper 1's counter is written whole by `$6006/$6007`,
        // so every value is one it can hold. Validated before any assignment.
        let counter = u16::from_le_bytes([data[12], data[13]]);
        let limit = u16::from(A12_RISES_PER_IRQ);
        if self.submapper == 0 && (counter > limit || (data[10] != 0 && counter >= limit)) {
            return Err(MapperError::Invalid(format!(
                "mapper 91 IRQ counter {counter} is not one submapper 0 reaches \
                 (enabled {})",
                data[10] != 0
            )));
        }
        if self.submapper == 1 {
            self.mirroring = if data[2] == 0 {
                Mirroring::Horizontal
            } else {
                Mirroring::Vertical
            };
        }
        self.chr_regs.copy_from_slice(&data[3..7]);
        self.prg_regs.copy_from_slice(&data[7..9]);
        self.outer = data[9] & 0x07;
        self.irq_enabled = data[10] != 0;
        self.irq_pending = data[11] != 0;
        self.irq_counter = counter;
        self.irq_prescale = data[14] & 0x03;
        self.last_a12 = data[15] != 0;
        self.vram
            .copy_from_slice(&data[HEAD..HEAD + self.vram.len()]);
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[HEAD + self.vram.len()..]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(prg_8k: usize, chr_2k: usize, sub: u8) -> Jy91 {
        let mut prg = vec![0u8; prg_8k * PRG_BANK_8K];
        for b in 0..prg_8k {
            prg[b * PRG_BANK_8K] = b as u8;
        }
        let mut chr = vec![0u8; chr_2k * CHR_BANK_2K];
        for b in 0..chr_2k {
            chr[b * CHR_BANK_2K] = b as u8;
            chr[b * CHR_BANK_2K + 1] = (b >> 8) as u8;
        }
        Jy91::new(
            prg.into_boxed_slice(),
            chr.into_boxed_slice(),
            Mirroring::Vertical,
            sub,
        )
        .unwrap()
    }

    fn chr_at(m: &mut Jy91, addr: u16) -> usize {
        usize::from(m.ppu_read(addr)) | usize::from(m.ppu_read(addr + 1)) << 8
    }

    #[test]
    fn banks_follow_the_register_table() {
        let mut m = board(16, 16, 0);
        m.cpu_write(0x7000, 3);
        m.cpu_write(0x7001, 5);
        m.cpu_write(0x6002, 7);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xC000), 14, "the last 16 KiB is fixed");
        assert_eq!(m.cpu_read(0xE000), 15);
        assert_eq!(chr_at(&mut m, 0x1000), 7);
        // Submapper 0's mask `$F003` mirrors `$6002` at `$6006`.
        m.cpu_write(0x6FF6, 9);
        assert_eq!(chr_at(&mut m, 0x1000), 9);
    }

    #[test]
    fn submapper0_outer_bank_comes_from_the_write_address() {
        let mut m = board(64, 512, 0);
        m.cpu_write(0x7000, 3);
        m.cpu_write(0x6000, 0x05);
        m.cpu_write(0x8007, 0xFF); // PRG A18-A17 = 3, CHR A19 = 1
        assert_eq!(m.cpu_read(0x8000), 0x30 | 3);
        assert_eq!(m.cpu_read(0xE000), 0x3F, "last bank of the 128 KiB block");
        assert_eq!(chr_at(&mut m, 0x0000), 0x105);
        m.cpu_write(0x9FF2, 0x00); // PRG A18-A17 = 1, CHR A19 = 0
        assert_eq!(m.cpu_read(0x8000), 0x13);
        assert_eq!(chr_at(&mut m, 0x0000), 0x05);
    }

    #[test]
    fn submapper0_irq_after_64_unfiltered_a12_rises() {
        let mut m = board(16, 16, 0);
        m.cpu_write(0x7007, 0);
        for _ in 0..63 {
            m.notify_a12(true);
            m.notify_a12(false);
        }
        assert!(!m.irq_pending());
        m.notify_a12(true);
        assert!(m.irq_pending(), "the 64th rise asserts");
        m.cpu_write(0x7006, 0);
        assert!(!m.irq_pending(), "$7006 acknowledges");
    }

    #[test]
    fn submapper1_mirroring_and_m2_irq() {
        let mut m = board(16, 16, 1);
        m.cpu_write(0x6004, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x6005, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // 10 counts = two decrements of five, eight M2 cycles, then the
        // third decrement underflows at M2 cycle twelve.
        m.cpu_write(0x6006, 10);
        m.cpu_write(0x6007, 0);
        m.cpu_write(0x7007, 0);
        for _ in 0..11 {
            m.notify_cpu_cycle();
        }
        assert!(!m.irq_pending());
        m.notify_cpu_cycle();
        assert!(m.irq_pending());
        // Submapper 1's `$F007` mask: `$6006` is no longer CHR register 2.
        assert!(!m.has_hardwired_mirroring());
    }

    #[test]
    fn state_refuses_a_counter_the_board_cannot_reach() {
        // Submapper 0 counts A12 rises up to 64 and stops there, so an enabled
        // counter is below 64 and a stopped one at most 64. 0xFFFF loaded
        // cleanly and overflowed `+= 1` on the next rise.
        let good = board(16, 16, 0).save_state();
        for (enabled, counter) in [(1u8, 0xFFFFu16), (1, 64), (0, 65)] {
            let mut blob = good.clone();
            blob[10] = enabled;
            blob[12..14].copy_from_slice(&counter.to_le_bytes());
            let mut m = board(16, 16, 0);
            assert!(
                matches!(m.load_state(&blob), Err(MapperError::Invalid(_))),
                "enabled {enabled} counter {counter}"
            );
            assert_eq!(m.save_state(), good, "refused before any assignment");
        }
        for (enabled, counter) in [(1u8, 63u16), (0, 64)] {
            let mut blob = good.clone();
            blob[10] = enabled;
            blob[12..14].copy_from_slice(&counter.to_le_bytes());
            board(16, 16, 0).load_state(&blob).unwrap();
        }
        // Submapper 1 loads the counter from its registers: any value is real.
        let mut blob = board(16, 16, 1).save_state();
        blob[10] = 1;
        blob[12..14].copy_from_slice(&0xFFFFu16.to_le_bytes());
        board(16, 16, 1).load_state(&blob).unwrap();
    }

    #[test]
    fn state_round_trips() {
        for sub in [0, 1] {
            let mut a = board(64, 512, sub);
            a.cpu_write(0x7000, 7);
            a.cpu_write(0x6001, 9);
            a.cpu_write(0x8005, 0);
            a.cpu_write(0x7007, 0);
            let blob = a.save_state();
            let mut b = board(64, 512, sub);
            b.load_state(&blob).unwrap();
            assert_eq!(b.save_state(), blob);
            assert_eq!(b.cpu_read(0x8000), a.cpu_read(0x8000));
        }
        assert!(
            board(16, 16, 1)
                .load_state(&board(16, 16, 0).save_state())
                .is_err()
        );
    }
}
