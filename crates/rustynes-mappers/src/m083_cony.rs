// SPDX-License-Identifier: GPL-3.0-or-later
//! Mapper 83: Cony / Yoko (v2.9.6 "Roster").
//!
//! Written from `nesdev_wiki/output/INES_Mapper_083.md`. Three subtypes,
//! told apart by NES 2.0 submapper or, on an iNES header, by CHR-ROM size as
//! the page describes:
//!
//! | submapper | CHR-ROM | CHR banking | `$6000-$7FFF` |
//! |---|---|---|---|
//! | 0 | 256 KiB | 1 KiB x 8 | 8 KiB PRG-ROM (reg 3) when mode bit 5 is set, else open bus |
//! | 1 | 512 KiB | 2 KiB x 4 (`$8310/1/6/7`) | as submapper 0 |
//! | 2 | 1 MiB | 1 KiB x 8 inside a 256 KiB outer bank | 32 KiB WRAM, 8 KiB bank from `$8000` bits 6-7 |
//!
//! Registers (masks from the page): `$8000` PRG register 4 / outer bank
//! (`$8300`), `$8100` mode (`$8300`), `$8200` IRQ low + acknowledge and `$8201`
//! IRQ high + enable (`$8301`), `$8300-$8303` PRG registers (`$8313`), and
//! `$8310-$8317` CHR registers (`$831F`).
//!
//! PRG modes (mode bits 3-4): 0 = 16 KiB at `$8000` from register 4 plus the
//! last 16 KiB; 1 = 32 KiB from register 4 >> 1; 2 and 3 = three 8 KiB banks
//! from registers 0-2 plus the last 8 KiB. On submapper 2, all of it is inside
//! the 256 KiB outer bank from register 4 bits 4-5.
//!
//! IRQ: a 16-bit counter that, while enabled and non-zero, counts up (mode
//! bit 6 clear) or down (set) every M2 cycle. On reaching zero it asserts and
//! disables itself. `$8201` copies mode bit 7 to the (otherwise inaccessible)
//! enable.
//!
//! The page gives the DIP switch (`$5000`) and scratch-RAM (`$5100-$5FFF`)
//! masks only as "probably `$DF00`" / "probably `$DF03`". Taken literally,
//! `$DF00` would also decode at `$7000`, which submappers 0 and 1 use for
//! PRG-ROM, so both are decoded inside `$5000-$5FFF` only.

// Bank arithmetic narrows values already reduced modulo a bank count, and the
// accessors stay non-`const` to match the other mapper modules; both lints
// are allowed crate-wide in the sibling modules for the same reasons.
#![allow(clippy::cast_possible_truncation, clippy::missing_const_for_fn)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const WRAM_SUB2: usize = 0x8000;
const SAVE_STATE_VERSION: u8 = 1;

/// Mapper 83 (Cony / Yoko).
pub struct Cony83 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    wram: Box<[u8]>,
    submapper: u8,
    mode: u8,
    /// `$8000`: PRG register 4 / outer bank / WRAM bank.
    reg4: u8,
    prg_regs: [u8; 4],
    chr_regs: [u8; 8],
    scratch: [u8; 4],
    dip: u8,
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Cony83 {
    /// Construct the board. `submapper` is the NES 2.0 submapper, or `None`
    /// on an iNES header, in which case the page's CHR-size heuristic picks it.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 8 KiB
    /// or CHR is not a multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        submapper: Option<u8>,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 83 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper 83 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        let submapper = match submapper {
            Some(s @ 0..=2) => s,
            Some(_) => 0,
            None => match chr_rom.len() {
                0x8_0000 => 1,
                0x10_0000 => 2,
                _ => 0,
            },
        };
        let chr_is_ram = chr_rom.is_empty();
        let chr = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else {
            chr_rom
        };
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 0x800].into_boxed_slice(),
            wram: vec![0u8; if submapper == 2 { WRAM_SUB2 } else { 0 }].into_boxed_slice(),
            submapper,
            mode: 0,
            reg4: 0,
            prg_regs: [0; 4],
            chr_regs: [0; 8],
            scratch: [0; 4],
            dip: 0,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
        })
    }

    /// Set the DIP switch (0-3) read at `$5000`.
    pub fn set_dip(&mut self, dip: u8) {
        self.dip = dip & 0x03;
    }

    /// The 8 KiB PRG bank for `addr` (`$8000-$FFFF`).
    fn prg_bank(&self, addr: u16) -> usize {
        let total = self.prg_rom.len() / PRG_BANK_8K;
        let (base, window) = if self.submapper == 2 {
            (usize::from((self.reg4 >> 4) & 0x03) * 32, 32.min(total))
        } else {
            (0, total)
        };
        let last = window - 1;
        let slot = usize::from((addr >> 13) & 0x03);
        let r4 = usize::from(self.reg4 & 0x0F);
        let inner = match (self.mode >> 3) & 0x03 {
            0 => {
                if slot < 2 {
                    r4 * 2 + slot
                } else {
                    last - 3 + slot
                }
            }
            1 => (r4 >> 1) * 4 + slot,
            _ => {
                if slot < 3 {
                    usize::from(self.prg_regs[slot])
                } else {
                    last
                }
            }
        };
        let inner = if self.submapper == 2 {
            inner % window
        } else {
            inner
        };
        (base + inner) % total
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let within = usize::from(addr & 0x03FF);
        let slot = usize::from((addr >> 10) & 0x07);
        let bank = match self.submapper {
            1 => {
                // 2 KiB banks from `$8310`, `$8311`, `$8316`, `$8317`.
                let reg = [0, 0, 1, 1, 6, 6, 7, 7][slot];
                usize::from(self.chr_regs[reg]) * 2 + (slot & 1)
            }
            2 => usize::from(self.chr_regs[slot]) | (usize::from((self.reg4 >> 4) & 0x03) << 8),
            _ => usize::from(self.chr_regs[slot]),
        };
        (bank % (self.chr.len() / CHR_BANK_1K)) * CHR_BANK_1K + within
    }

    fn mirroring(&self) -> Mirroring {
        match self.mode & 0x03 {
            0 => Mirroring::Vertical,
            1 => Mirroring::Horizontal,
            2 => Mirroring::SingleScreenA,
            _ => Mirroring::SingleScreenB,
        }
    }

    fn nt_offset(&self, addr: u16) -> usize {
        let table = ((addr - 0x2000) / 0x400) & 0x03;
        self.mirroring().physical_bank(table as u8) * 0x400 + usize::from(addr & 0x3FF)
    }

    /// Submappers 0/1: `$6000-$7FFF` is ROM when mode bit 5 is set.
    fn low_rom_enabled(&self) -> bool {
        self.submapper != 2 && self.mode & 0x20 != 0
    }

    fn wram_offset(&self, addr: u16) -> usize {
        (usize::from(self.reg4 >> 6) * 0x2000 + usize::from(addr & 0x1FFF)) % self.wram.len()
    }
}

impl Mapper for Cony83 {
    fn sram(&self) -> &[u8] {
        &self.wram
    }

    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.wram
    }

    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match addr {
            0x4020..=0x4FFF => true,
            0x6000..=0x7FFF => !self.low_rom_enabled() && self.wram.is_empty(),
            // `$5000-$5FFF` (DIP switch, scratch RAM) and PRG-ROM.
            _ => false,
        }
    }

    fn cpu_read_driven_mask(&self, addr: u16) -> u8 {
        if addr & 0xFF00 == 0x5000 { 0x03 } else { 0xFF }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x5000..=0x50FF => self.dip,
            0x5100..=0x5FFF => self.scratch[usize::from(addr & 0x03)],
            0x6000..=0x7FFF if self.low_rom_enabled() => {
                let bank = usize::from(self.prg_regs[3]) % (self.prg_rom.len() / PRG_BANK_8K);
                self.prg_rom[bank * PRG_BANK_8K + usize::from(addr & 0x1FFF)]
            }
            0x6000..=0x7FFF if !self.wram.is_empty() => self.wram[self.wram_offset(addr)],
            0x8000..=0xFFFF => {
                self.prg_rom[self.prg_bank(addr) * PRG_BANK_8K + usize::from(addr & 0x1FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x5100..=0x5FFF => self.scratch[usize::from(addr & 0x03)] = value,
            0x6000..=0x7FFF if !self.wram.is_empty() => {
                let off = self.wram_offset(addr);
                self.wram[off] = value;
            }
            0x8000..=0xFFFF => match addr & 0x8300 {
                0x8000 => self.reg4 = value,
                0x8100 => self.mode = value,
                0x8200 => {
                    if addr & 0x01 == 0 {
                        self.irq_counter = (self.irq_counter & 0xFF00) | u16::from(value);
                        self.irq_pending = false;
                    } else {
                        self.irq_counter = (self.irq_counter & 0x00FF) | (u16::from(value) << 8);
                        self.irq_enabled = self.mode & 0x80 != 0;
                    }
                }
                _ => match addr & 0x831F {
                    r @ 0x8300..=0x8303 => self.prg_regs[usize::from(r & 0x03)] = value,
                    r @ 0x8310..=0x8317 => self.chr_regs[usize::from(r & 0x07)] = value,
                    _ => {}
                },
            },
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        if !self.irq_enabled || self.irq_counter == 0 {
            return;
        }
        self.irq_counter = if self.mode & 0x40 != 0 {
            self.irq_counter.wrapping_sub(1)
        } else {
            self.irq_counter.wrapping_add(1)
        };
        if self.irq_counter == 0 {
            self.irq_pending = true;
            self.irq_enabled = false;
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
        self.mirroring()
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = MapperDebugInfo {
            mapper_id: 83,
            name: format!("Cony/Yoko (submapper {})", self.submapper),
            mirroring: crate::mapper::mirroring_name(self.mirroring()),
            ..Default::default()
        };
        info.prg_banks
            .push(("mode".into(), format!("{}", (self.mode >> 3) & 3)));
        info.prg_banks
            .push(("R4".into(), format!("{:#04x}", self.reg4)));
        for (i, r) in self.prg_regs.iter().enumerate() {
            info.prg_banks.push((format!("R{i}"), format!("{r:#04x}")));
        }
        for (i, r) in self.chr_regs.iter().enumerate() {
            info.chr_banks.push((format!("C{i}"), format!("{r:#04x}")));
        }
        info.irq_state
            .push(("counter".into(), format!("{:#06x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(32 + self.vram.len() + self.wram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.submapper);
        out.push(self.mode);
        out.push(self.reg4);
        out.extend_from_slice(&self.prg_regs);
        out.extend_from_slice(&self.chr_regs);
        out.extend_from_slice(&self.scratch);
        out.push(self.dip);
        out.extend_from_slice(&self.irq_counter.to_le_bytes());
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.wram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        const HEAD: usize = 25;
        let chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = HEAD + self.vram.len() + self.wram.len() + chr;
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
                "state is for mapper 83 submapper {}, this board is {}",
                data[1], self.submapper
            )));
        }
        self.mode = data[2];
        self.reg4 = data[3];
        self.prg_regs.copy_from_slice(&data[4..8]);
        self.chr_regs.copy_from_slice(&data[8..16]);
        self.scratch.copy_from_slice(&data[16..20]);
        self.dip = data[20] & 0x03;
        self.irq_counter = u16::from_le_bytes([data[21], data[22]]);
        self.irq_enabled = data[23] != 0;
        self.irq_pending = data[24] != 0;
        let mut cur = HEAD;
        self.vram.copy_from_slice(&data[cur..cur + self.vram.len()]);
        cur += self.vram.len();
        let n = self.wram.len();
        self.wram.copy_from_slice(&data[cur..cur + n]);
        cur += n;
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[cur..]);
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn board(prg_8k: usize, chr_1k: usize, sub: Option<u8>) -> Cony83 {
        let mut prg = vec![0u8; prg_8k * PRG_BANK_8K];
        for b in 0..prg_8k {
            prg[b * PRG_BANK_8K] = b as u8;
        }
        let mut chr = vec![0u8; chr_1k * CHR_BANK_1K];
        for b in 0..chr_1k {
            chr[b * CHR_BANK_1K] = b as u8;
            chr[b * CHR_BANK_1K + 1] = (b >> 8) as u8;
        }
        Cony83::new(prg.into_boxed_slice(), chr.into_boxed_slice(), sub).unwrap()
    }

    fn chr_at(m: &mut Cony83, addr: u16) -> usize {
        usize::from(m.ppu_read(addr)) | usize::from(m.ppu_read(addr + 1)) << 8
    }

    #[test]
    fn submapper_follows_chr_size_on_ines() {
        assert_eq!(board(32, 256, None).submapper, 0);
        assert_eq!(board(32, 512, None).submapper, 1);
        assert_eq!(board(128, 1024, None).submapper, 2);
        assert_eq!(board(32, 256, Some(2)).submapper, 2, "NES 2.0 wins");
    }

    #[test]
    fn prg_modes_follow_the_page() {
        let mut m = board(32, 256, None);
        m.cpu_write(0x8000, 0x05);
        m.cpu_write(0x8100, 0x00);
        assert_eq!(m.cpu_read(0x8000), 10, "mode 0: 16 KiB bank 5");
        assert_eq!(m.cpu_read(0xA000), 11);
        assert_eq!(m.cpu_read(0xC000), 30, "the last 16 KiB");
        assert_eq!(m.cpu_read(0xE000), 31);
        m.cpu_write(0x8100, 0x08);
        assert_eq!(m.cpu_read(0x8000), 8, "mode 1: 32 KiB bank 5>>1 = 2");
        assert_eq!(m.cpu_read(0xE000), 11);
        m.cpu_write(0x8100, 0x10);
        m.cpu_write(0x8300, 3);
        m.cpu_write(0x8301, 4);
        m.cpu_write(0x8302, 5);
        assert_eq!(m.cpu_read(0x8000), 3, "mode 2: registers 0-2");
        assert_eq!(m.cpu_read(0xA000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
        assert_eq!(m.cpu_read(0xE000), 31);
        m.cpu_write(0x8100, 0x18);
        assert_eq!(m.cpu_read(0xA000), 4, "mode 3 = mode 2");
    }

    #[test]
    fn prg_rom_at_6000_on_submappers_0_and_1() {
        let mut m = board(32, 256, None);
        m.cpu_write(0x8303, 7);
        assert!(m.cpu_read_unmapped(0x6000), "bit 5 clear: open bus");
        m.cpu_write(0x8100, 0x20);
        assert!(!m.cpu_read_unmapped(0x6000));
        assert_eq!(m.cpu_read(0x6000), 7);
    }

    #[test]
    fn register_masks_mirror_the_decodes() {
        let mut m = board(32, 256, None);
        m.cpu_write(0x8100, 0x20);
        m.cpu_write(0xB3E3, 6); // & $8313 = $8303: PRG register 3
        assert_eq!(m.cpu_read(0x6000), 6);
        m.cpu_write(0x8318, 0x44); // $8318-$831F: no function
        m.cpu_write(0xBF14, 0x21); // & $831F = $8314: CHR register 4
        assert_eq!(chr_at(&mut m, 0x1000), 0x21);
    }

    #[test]
    fn submapper1_uses_2k_chr_registers() {
        let mut m = board(32, 512, None);
        m.cpu_write(0x8310, 3);
        m.cpu_write(0x8311, 4);
        m.cpu_write(0x8316, 5);
        m.cpu_write(0x8317, 6);
        m.cpu_write(0x8312, 0x7F); // no function on submapper 1
        assert_eq!(chr_at(&mut m, 0x0000), 6);
        assert_eq!(chr_at(&mut m, 0x0400), 7);
        assert_eq!(chr_at(&mut m, 0x0800), 8);
        assert_eq!(chr_at(&mut m, 0x1000), 10);
        assert_eq!(chr_at(&mut m, 0x1C00), 13);
    }

    #[test]
    fn submapper2_outer_bank_and_wram() {
        let mut m = board(128, 1024, None);
        m.cpu_write(0x8100, 0x10);
        m.cpu_write(0x8300, 3);
        m.cpu_write(0x8314, 5);
        m.cpu_write(0x8000, 0x20); // outer bank 2
        assert_eq!(m.cpu_read(0x8000), 64 + 3);
        assert_eq!(
            m.cpu_read(0xE000),
            64 + 31,
            "last bank of the outer 256 KiB"
        );
        assert_eq!(chr_at(&mut m, 0x1000), 0x205);
        // WRAM banks from bits 6-7.
        m.cpu_write(0x6000, 0xAA);
        m.cpu_write(0x8000, 0x60);
        m.cpu_write(0x6000, 0xBB);
        m.cpu_write(0x8000, 0x20);
        assert_eq!(m.cpu_read(0x6000), 0xAA);
        assert_eq!(m.sram().len(), WRAM_SUB2);
    }

    #[test]
    fn irq_counts_up_or_down_and_disables_at_zero() {
        let mut m = board(32, 256, None);
        m.cpu_write(0x8100, 0x80); // enable latch, count up
        m.cpu_write(0x8200, 0xFE);
        m.cpu_write(0x8201, 0xFF);
        m.notify_cpu_cycle();
        assert!(!m.irq_pending());
        m.notify_cpu_cycle();
        assert!(m.irq_pending(), "$FFFE + 2 = 0");
        m.cpu_write(0x8200, 0x02);
        assert!(!m.irq_pending(), "$8200 acknowledges");
        m.cpu_write(0x8100, 0xC0); // count down
        m.cpu_write(0x8201, 0x00);
        m.notify_cpu_cycle();
        m.notify_cpu_cycle();
        assert!(m.irq_pending(), "2 - 2 = 0");
        m.cpu_write(0x8200, 0x05);
        m.notify_cpu_cycle();
        assert_eq!(m.irq_counter, 5, "disabled after firing");
        // The enable copies mode bit 7 at the $8201 write, not later.
        m.cpu_write(0x8100, 0x40);
        m.cpu_write(0x8201, 0x00);
        m.cpu_write(0x8100, 0xC0);
        m.notify_cpu_cycle();
        assert_eq!(m.irq_counter, 5);
    }

    #[test]
    fn dip_and_scratch_ram() {
        let mut m = board(32, 256, None);
        m.set_dip(2);
        assert_eq!(m.cpu_read(0x5000) & 0x03, 2);
        assert_eq!(m.cpu_read_driven_mask(0x5000), 0x03);
        m.cpu_write(0x5102, 0x5A);
        assert_eq!(m.cpu_read(0x5F06), 0x5A, "scratch byte 2, mask $DF03");
    }

    #[test]
    fn state_round_trips() {
        for chr in [256, 512, 1024] {
            let mut a = board(128, chr, None);
            a.cpu_write(0x8100, 0x93);
            a.cpu_write(0x8000, 0x65);
            a.cpu_write(0x8313, 0x22);
            a.cpu_write(0x5101, 7);
            a.cpu_write(0x6001, 3);
            let blob = a.save_state();
            let mut b = board(128, chr, None);
            b.load_state(&blob).unwrap();
            assert_eq!(b.save_state(), blob);
        }
    }
}
