//! Mapper 42 -- Famicom Disk System to cartridge conversion (Bio Miracle
//! Bokutte Upa, Mario Baby).
//!
//! Running an FDS title from a cartridge is harder than it sounds: the game
//! expects RAM where a cartridge has ROM, and it expects the disk BIOS's timer
//! IRQ to be ticking. So this board does two unusual things at once -- it maps
//! an 8 KiB PRG window a plain multicart would not need, and it carries a
//! free-running CPU-cycle IRQ counter purely to stand in for the BIOS timer the
//! game is polling.
//!
//! The other conversion board is mapper 50, in
//! `m050_fds_conv_smb2j.rs`. The real FDS is emulated in `fds.rs`.//!
//! A best-effort (Tier-2) board: register-decode correctness verified against
//! the reference emulators (`Mesen2`, `GeraNES`) and the nesdev wiki, with no
//! commercial-oracle ROM in the tree. Banking math is direct slice indexing and
//! every bank select wraps with `% count`, so a register write can never index
//! out of bounds -- required for the `#![no_std]` chip stack, which cannot
//! afford a panic on a register access.
//!
//! See `tier.rs` (`MapperTier::BestEffort`), `docs/adr/0011-mapper-tiering.md`,
//! and `docs/mappers.md` §Mapper coverage matrix.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::bool_to_int_with_if
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_8K: usize = 0x2000;
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

const fn mirroring_to_byte(m: Mirroring) -> u8 {
    match m {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::SingleScreenA => 2,
        Mirroring::SingleScreenB => 3,
        Mirroring::FourScreen => 4,
        Mirroring::MapperControlled => 5,
    }
}

const fn byte_to_mirroring(b: u8, fallback: Mirroring) -> Mirroring {
    match b {
        0 => Mirroring::Horizontal,
        1 => Mirroring::Vertical,
        2 => Mirroring::SingleScreenA,
        3 => Mirroring::SingleScreenB,
        4 => Mirroring::FourScreen,
        5 => Mirroring::MapperControlled,
        _ => fallback,
    }
}

// ===========================================================================
// Mmc3Clone — reusable MMC3-style core for the clone boards.
//
// The MMC3 register protocol (NES 2.0 mapper 4):
//   $8000 even : bank-select (low 3 bits = R index, bit 6 = PRG mode,
//                bit 7 = CHR mode).
//   $8001 odd  : bank-data (the value loaded into the selected R register).
//   $A000 even : mirroring (bit 0: 0 = vertical, 1 = horizontal).
//   $C000 even : IRQ latch (reload value).
//   $C001 odd  : IRQ reload (force a reload on the next A12 rise).
//   $E000 even : IRQ disable + acknowledge.
//   $E001 odd  : IRQ enable.
//
// The A12 IRQ counter clocks on every PPU A12 rising edge: if the counter is 0
// or a reload is pending, it reloads from the latch; otherwise it decrements.
// After the update, if the counter is 0 and IRQs are enabled, the IRQ asserts.
// ===========================================================================

/// Mapper 42 (FDS-to-cart conversion: *Mario Baby* / *Ai Senshi Nicol*).
pub struct Mapper42 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    prg_ram_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
    irq_counter: u16,
    irq_enabled: bool,
    irq_pending: bool,
}

impl Mapper42 {
    /// Construct a mapper 42 board.
    ///
    /// # Errors
    /// [`MapperError::Invalid`] on a bad PRG size.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 42 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            chr_rom
        };
        Ok(Self {
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram_bank: 0,
            chr_bank: 0,
            mirroring,
            irq_counter: 0,
            irq_enabled: false,
            irq_pending: false,
        })
    }

    fn prg_8k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for Mapper42 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        match addr {
            0x6000..=0x7FFF => self.prg_8k(self.prg_ram_bank as usize, addr),
            0x8000..=0x9FFF => self.prg_8k(count.saturating_sub(4), addr),
            0xA000..=0xBFFF => self.prg_8k(count.saturating_sub(3), addr),
            0xC000..=0xDFFF => self.prg_8k(count.saturating_sub(2), addr),
            0xE000..=0xFFFF => self.prg_8k(count.saturating_sub(1), addr),
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xE003 {
            0x8000 => self.chr_bank = value & 0x0F,
            0xE000 => self.prg_ram_bank = value & 0x0F,
            0xE001 => {
                self.mirroring = if value & 0x08 != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
            }
            0xE002 => {
                self.irq_enabled = value & 0x02 != 0;
                if !self.irq_enabled {
                    self.irq_pending = false;
                    self.irq_counter = 0;
                }
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (self.chr.len() - 1)];
                }
                let count = (self.chr.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                let off = addr as usize & (self.chr.len() - 1);
                self.chr[off] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.mirroring);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn notify_cpu_cycle(&mut self) {
        if !self.irq_enabled {
            return;
        }
        self.irq_counter += 1;
        if self.irq_counter >= 0x8000 {
            self.irq_counter -= 0x8000;
        }
        self.irq_pending = self.irq_counter >= 0x6000;
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn irq_acknowledge(&mut self) {
        self.irq_pending = false;
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(8 + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_ram_bank);
        out.push(self.chr_bank);
        out.push(mirroring_to_byte(self.mirroring));
        out.push((self.irq_counter & 0xFF) as u8);
        out.push((self.irq_counter >> 8) as u8);
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 8 + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_ram_bank = data[1];
        self.chr_bank = data[2];
        self.mirroring = byte_to_mirroring(data[3], self.mirroring);
        self.irq_counter = u16::from(data[4]) | (u16::from(data[5]) << 8);
        self.irq_enabled = data[6] != 0;
        self.irq_pending = data[7] != 0;
        let mut cursor = 8;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr
                .copy_from_slice(&data[cursor..cursor + self.chr.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 50 — Alibaba / SMB2J alternate FDS-to-cartridge conversion.
//
// Fixed PRG layout (8 KiB banks): $6000 -> bank 15, $8000 -> bank 8,
// $A000 -> bank 9, $C000 -> switchable, $E000 -> bank 11. The $C000 bank is
// written via $4020 (addr & 0x4120 == 0x4020) with a bit-scrambled value:
//   bank = (v & 0x08) | ((v & 0x01) << 2) | ((v & 0x06) >> 1).
// $4120 (addr & 0x4120 == 0x4120): IRQ enable (bit 0). When enabled, an M2
// counter counts up and asserts once at 4096 cycles, then disables. Disabling
// clears + acknowledges. 8 KiB CHR-RAM.
// ===========================================================================

#[cfg(test)]
#[cfg(test)]
mod tests {
    use super::*;

    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn m42_fixed_tail_and_switchable_window() {
        let mut m = Mapper42::new(synth_prg_8k(8), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
        m.cpu_write(0xE000, 3);
        assert_eq!(m.cpu_read(0x6000), 3);
        m.cpu_write(0x8000, 1);
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn m42_irq_window() {
        let mut m = Mapper42::new(synth_prg_8k(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE002, 0x02); // enable
        let mut fired = false;
        for _ in 0..0x8000 {
            m.notify_cpu_cycle();
            if m.irq_pending() {
                fired = true;
                break;
            }
        }
        assert!(fired);
        m.cpu_write(0xE002, 0x00); // disable + clear
        assert!(!m.irq_pending());
    }

    #[test]
    fn m42_save_state_round_trip() {
        let mut m = Mapper42::new(synth_prg_8k(8), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 2);
        m.cpu_write(0x8000, 1);
        m.cpu_write(0xE002, 0x02);
        m.notify_cpu_cycle();
        let blob = m.save_state();
        let mut m2 = Mapper42::new(synth_prg_8k(8), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x6000), 2);
        assert_eq!(m2.ppu_read(0x0000), 1);
    }
}
