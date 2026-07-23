//! Mapper 50 -- Famicom Disk System to cartridge conversion (Super Mario
//! Bros. 2 / SMB2j pirate).
//!
//! Like mapper 42 (`m042_fds_conv_bio_miracle.rs`) it substitutes a
//! free-running CPU-cycle IRQ counter for the disk BIOS timer the game expects.
//! It additionally *scrambles* the bank bits of its `$8000` window -- the
//! written value's bits are permuted before becoming a bank number, a copy
//! protection measure rather than a technical necessity, and the detail a naive
//! port gets wrong.
//!
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

/// Mapper 50 (Alibaba / *SMB2J* alternate FDS-to-cart conversion).
pub struct Mapper50 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    switch_bank: u8,
    irq_enabled: bool,
    irq_counter: u16,
    irq_pending: bool,
    mirroring: Mirroring,
}

impl Mapper50 {
    /// Construct a mapper 50 board.
    ///
    /// # Errors
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 50 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            switch_bank: 0,
            irq_enabled: false,
            irq_counter: 0,
            irq_pending: false,
            mirroring,
        })
    }

    fn prg_8k(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for Mapper50 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_8k(15, addr),
            0x8000..=0x9FFF => self.prg_8k(8, addr),
            0xA000..=0xBFFF => self.prg_8k(9, addr),
            0xC000..=0xDFFF => self.prg_8k(self.switch_bank as usize, addr),
            0xE000..=0xFFFF => self.prg_8k(11, addr),
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0x4120 {
            0x4020 => {
                self.switch_bank = (value & 0x08) | ((value & 0x01) << 2) | ((value & 0x06) >> 1);
            }
            0x4120 => {
                // Both enable and disable (re)start the counter from 0 and
                // clear any pending line; only the enable flag itself differs.
                // On IRQ-enable this means a fresh enable after a prior fire
                // counts a full period rather than tripping on a stale counter
                // / latched IRQ.
                self.irq_enabled = value & 0x01 != 0;
                self.irq_pending = false;
                self.irq_counter = 0;
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
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
        if self.irq_counter == 0x1000 {
            self.irq_pending = true;
            self.irq_enabled = false;
        }
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
        let mut out = Vec::with_capacity(6 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.switch_bank);
        out.push(u8::from(self.irq_enabled));
        out.push((self.irq_counter & 0xFF) as u8);
        out.push((self.irq_counter >> 8) as u8);
        out.push(u8::from(self.irq_pending));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 6 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.switch_bank = data[1];
        self.irq_enabled = data[2] != 0;
        self.irq_counter = u16::from(data[3]) | (u16::from(data[4]) << 8);
        self.irq_pending = data[5] != 0;
        let mut cursor = 6;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// DiscreteMapper — small hook-free single/dual-register multicart boards
// (46/51/57/104/120/290/301). 32/16 KiB PRG window + 8 KiB CHR-ROM/RAM.
// ===========================================================================

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

    #[test]
    fn m50_fixed_layout_and_scrambled_switch() {
        let mut m = Mapper50::new(synth_prg_8k(16), &[], Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x6000), 15);
        assert_eq!(m.cpu_read(0x8000), 8);
        assert_eq!(m.cpu_read(0xA000), 9);
        assert_eq!(m.cpu_read(0xE000), 11);
        // value 0x05 -> (0x05&8)|((0x05&1)<<2)|((0x05&6)>>1) = 0 | 4 | 2 = 6.
        m.cpu_write(0x4020, 0x05);
        assert_eq!(m.cpu_read(0xC000), 6);
    }

    #[test]
    fn m50_irq_fires_once_then_disables() {
        let mut m = Mapper50::new(synth_prg_8k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x4120, 0x01); // enable
        for _ in 0..0x1000 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
        m.cpu_write(0x4120, 0x00); // disable + ack
        assert!(!m.irq_pending());
    }

    #[test]
    fn m50_save_state_round_trip() {
        let mut m = Mapper50::new(synth_prg_8k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x4020, 0x05);
        m.cpu_write(0x4120, 0x01);
        m.notify_cpu_cycle();
        m.ppu_write(0x0007, 0x33);
        let blob = m.save_state();
        let mut m2 = Mapper50::new(synth_prg_8k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xC000), 6);
        assert_eq!(m2.ppu_read(0x0007), 0x33);
    }
}
