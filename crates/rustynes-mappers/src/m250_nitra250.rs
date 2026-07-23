//! Nitra (mapper 250) -- Time Diver Avenger.
//!
//! An MMC3 work-alike that moves the register interface into the *address*:
//! the value written is ignored, and the low byte of the address supplies
//! the data instead. The underlying bank/IRQ behaviour is MMC3's, which is
//! why this board carries an A12-driven scanline IRQ counter where the rest
//! of its size class carries none.
//!
//! A best-effort (Tier-2) board: register-decode correctness verified against
//! the `GeraNES` reference (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`)
//! and the nesdev wiki, with no commercial-oracle ROM in the tree. Banking math
//! is direct slice indexing and every bank select wraps with `% count`, so a
//! register write can never index out of bounds -- required for the `#![no_std]`
//! chip stack, which cannot afford a panic on a register access.
//!
//! See `tier.rs` (`MapperTier::BestEffort`), `docs/adr/0011-mapper-tiering.md`,
//! and `docs/mappers.md` §Mapper coverage matrix.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Shared nametable helper (mirrors the one in the other simple-mapper modules).
// ---------------------------------------------------------------------------

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

// ===========================================================================
// Mapper 40 — NTDEC 2722 (Super Mario Bros. 2J pirate conversion).
//
// PRG layout is fixed except for one switchable window:
//   $6000-$7FFF -> 8 KiB bank 6 (a copy of PRG bank 6; some dumps use it as
//                  the "intro" bank — modelled as bank 6 of the image).
//   $8000-$9FFF -> fixed bank 4
//   $A000-$BFFF -> fixed bank 5
//   $C000-$DFFF -> switchable 8 KiB bank (low 3 bits of any $E000-$FFFF write)
//   $E000-$FFFF -> fixed bank 7
// Registers (data ignored; address-decoded):
//   $8000-$9FFF : IRQ disable + acknowledge (counter held in reset).
//   $A000-$BFFF : IRQ enable (counter starts counting M2 cycles).
//   $E000-$FFFF : select the $C000 8 KiB bank (value & 0x07).
// The IRQ counter is a 12-bit M2 counter: once enabled it counts up and, when
// it reaches 4096 (0x1000), asserts the IRQ and holds. CHR is 8 KiB RAM.
// ===========================================================================

/// Mapper 250 (Nitra, *Time Diver Avenger*).
// Independent banking / mode / IRQ flags; grouping them would obscure the
// MMC3-equivalent register decode for no gain (mirrors `MapperCaps`).
#[allow(clippy::struct_excessive_bools)]
pub struct Nitra250 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    reg_index: u8,
    bank_regs: [u8; 8],
    prg_mode: bool,
    chr_mode: bool,
    horizontal_mirroring: bool,
    irq_latch: u8,
    irq_counter: u8,
    irq_reload: bool,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Nitra250 {
    /// Construct a new mapper 250 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB or CHR-ROM is empty / not a multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 250 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper 250 CHR-ROM size {} is not a non-zero multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_index: 0,
            bank_regs: [0; 8],
            prg_mode: false,
            chr_mode: false,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload: false,
            irq_enabled: false,
            irq_pending: false,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }

    fn prg_bank_for(&self, addr: u16) -> usize {
        let last = (self.prg_rom.len() / PRG_BANK_8K).max(1) - 1;
        let r6 = self.bank_regs[6] as usize;
        let r7 = self.bank_regs[7] as usize;
        match (self.prg_mode, addr) {
            (false, 0x8000..=0x9FFF) | (true, 0xC000..=0xDFFF) => r6,
            (false, 0xC000..=0xDFFF) | (true, 0x8000..=0x9FFF) => last - 1,
            (_, 0xA000..=0xBFFF) => r7,
            _ => last,
        }
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let count1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        // MMC3-style: chr_mode swaps the two 2 KiB and four 1 KiB regions.
        let region = (addr >> 10) & 0x07;
        let region = if self.chr_mode { region ^ 0x04 } else { region };
        let bank1k = match region {
            0 => self.bank_regs[0] as usize & !1,
            1 => (self.bank_regs[0] as usize & !1) + 1,
            2 => self.bank_regs[1] as usize & !1,
            3 => (self.bank_regs[1] as usize & !1) + 1,
            4 => self.bank_regs[2] as usize,
            5 => self.bank_regs[3] as usize,
            6 => self.bank_regs[4] as usize,
            _ => self.bank_regs[5] as usize,
        };
        let bank = bank1k % count1k;
        self.chr_rom[bank * CHR_BANK_1K + (addr as usize & 0x03FF)]
    }
}

impl Mapper for Nitra250 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let bank = self.prg_bank_for(addr);
            self.read_prg(bank, addr)
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        // The MMC3-equivalent "data" is the low byte of the address (A0-A7); the
        // MMC3 even/odd register line is carried by A10 (bit 10 of the address),
        // not A8 — Mesen2 MMC3_250 decodes `(addr & 0xE000) | ((addr & 0x0400)
        // >> 10)`. A8 left the bank-select / mirroring writes mis-routed, so the
        // reset vector landed in the wrong PRG bank → blank boot.
        let value = (addr & 0x00FF) as u8;
        let odd = (addr & 0x0400) != 0;
        match addr & 0xE000 {
            0x8000 => {
                if odd {
                    self.bank_regs[self.reg_index as usize] = value;
                } else {
                    self.reg_index = value & 0x07;
                    self.prg_mode = (value & 0x40) != 0;
                    self.chr_mode = (value & 0x80) != 0;
                }
            }
            0xA000 => {
                if !odd {
                    self.horizontal_mirroring = (value & 0x01) != 0;
                }
            }
            0xC000 => {
                if odd {
                    self.irq_reload = true;
                } else {
                    self.irq_latch = value;
                }
            }
            0xE000 => {
                if odd {
                    self.irq_enabled = true;
                } else {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                }
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.read_chr(addr),
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if (0x2000..=0x3EFF).contains(&addr) {
            let off = nametable_offset(addr, self.current_mirroring());
            self.vram[off] = value;
        }
    }

    fn notify_cpu_cycle(&mut self) {
        // A simple 8-bit M2 reload counter (Nitra wires the MMC3 IRQ to M2 on
        // this board rather than to A12). On reload or zero, reload from latch;
        // otherwise decrement, asserting at the 1->0 transition when enabled.
        if self.irq_reload || self.irq_counter == 0 {
            self.irq_counter = self.irq_latch;
            self.irq_reload = false;
        } else {
            self.irq_counter -= 1;
            if self.irq_counter == 0 && self.irq_enabled {
                self.irq_pending = true;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn irq_acknowledge(&mut self) {
        self.irq_pending = false;
    }

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(18 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_index);
        out.extend_from_slice(&self.bank_regs);
        out.push(u8::from(self.prg_mode));
        out.push(u8::from(self.chr_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.push(self.irq_latch);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_reload));
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 18 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg_index = data[1] & 0x07;
        self.bank_regs.copy_from_slice(&data[2..10]);
        self.prg_mode = data[10] != 0;
        self.chr_mode = data[11] != 0;
        self.horizontal_mirroring = data[12] != 0;
        self.irq_latch = data[13];
        self.irq_counter = data[14];
        self.irq_reload = data[15] != 0;
        self.irq_enabled = data[16] != 0;
        self.irq_pending = data[17] != 0;
        self.vram.copy_from_slice(&data[18..18 + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m250_address_encoded_mmc3_banking() {
        let mut m = Nitra250::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        // A10 (0x0400) carries the MMC3 even/odd line; A0-A7 carry the data.
        // Even $8000 (A10=0), data 0x06 -> reg select index 6.
        m.cpu_write(0x8000 | 0x06, 0);
        // Odd $8000 (A10=1), data 0x03 -> bank_regs[6] = 3.
        m.cpu_write(0x8000 | 0x400 | 0x03, 0);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Mirroring via even $A000 (A10=0), data bit0 = 1.
        m.cpu_write(0xA000 | 0x01, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m250_irq_counts_down() {
        let mut m = Nitra250::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        // latch = 3 via even $C000 (A10=0), data 0x03.
        m.cpu_write(0xC000 | 0x03, 0);
        m.cpu_write(0xC000 | 0x400, 0); // reload (odd, A10=1)
        m.cpu_write(0xE000 | 0x400, 0); // enable (odd, A10=1)
        // First cycle reloads from latch (=3); subsequent decrements reach 0.
        for _ in 0..5 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
        m.cpu_write(0xE000, 0); // disable + ack (even, A10=0)
        assert!(!m.irq_pending());
    }

    #[test]
    fn m250_save_state_round_trip() {
        let mut m = Nitra250::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000 | 0x06, 0);
        m.cpu_write(0x8000 | 0x400 | 0x02, 0);
        m.cpu_write(0xC000 | 0x05, 0);
        m.cpu_write(0xC000 | 0x400, 0);
        m.cpu_write(0xE000 | 0x400, 0);
        m.notify_cpu_cycle();
        let blob = m.save_state();
        let mut m2 = Nitra250::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
    }
}
