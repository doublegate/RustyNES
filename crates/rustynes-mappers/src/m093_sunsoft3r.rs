//! Sunsoft-3R / Sunsoft-2 IC (iNES mapper 93) implementation.
//!
//! The Sunsoft-2 IC on the Sunsoft-3R board (Shanghai, Fantasy Zone). A simple
//! UxROM-style discrete-logic board: a single `$8000-$FFFF` write register
//! `[.PPP ...E]` selects a 16 KiB switchable PRG bank at `$8000-$BFFF`
//! (bits 4-6) and a CHR-RAM enable bit (bit 0). The last 16 KiB PRG bank is
//! fixed at `$C000-$FFFF`. CHR is 8 KiB of RAM. Mirroring is fixed from the
//! iNES header (the one-screen-mirroring / CHR-ROM-banking variant is mapper
//! 89 on the Sunsoft-3 board).
//!
//! When the CHR-RAM enable bit is 0, CHR writes are ignored and reads are
//! open bus (no licensed game uses this disabled mode); the IC powers up
//! enabled.
//!
//! Register (nesdev `INES_Mapper_093.xhtml`, BUS CONFLICTS), `$8000-$FFFF`:
//!
//! ```text
//!   [.PPP ...E]  P = PRG reg (16 KiB @ $8000)
//!               E = CHR-RAM enable (0 = disabled, 1 = normal)
//! ```
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Sunsoft-3R / Sunsoft-2 IC mapper (iNES mapper 93).
pub struct Sunsoft3r {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_ram_enabled: bool,
    mirroring: Mirroring,
}

impl Sunsoft3r {
    /// Construct a new Sunsoft-3R mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 16 KiB. CHR is always 8 KiB of
    /// RAM (any CHR-ROM in the header is ignored — this board is CHR-RAM only).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when the PRG-ROM size is invalid.
    pub fn new(prg_rom: Box<[u8]>, mirroring: Mirroring) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "Sunsoft-3R PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_ram_enabled: true,
            mirroring,
        })
    }

    /// PRG-ROM read, shared by [`Mapper::cpu_read`] and the bus-conflict mask in
    /// [`Mapper::cpu_write`] (which needs `&self`, not the trait's `&mut self`).
    fn read_prg(&self, addr: u16) -> u8 {
        let bank_count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank as usize) % bank_count;
                let off = (addr - 0x8000) as usize;
                self.prg_rom[bank * PRG_BANK_16K + off]
            }
            0xC000..=0xFFFF => {
                // `.max(1)` above makes this subtraction safe for any accepted image.
                let last = bank_count - 1;
                let off = (addr - 0xC000) as usize;
                self.prg_rom[last * PRG_BANK_16K + off]
            }
            _ => 0,
        }
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }
}

impl Mapper for Sunsoft3r {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        self.read_prg(addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if let 0x8000..=0xFFFF = addr {
            // The Sunsoft-3R board has **bus conflicts** — this module's own
            // header already cites nesdev `INES_Mapper_093.xhtml` "BUS
            // CONFLICTS", but the mask was missing, so the doc and the code
            // disagreed. The register shares the address space with PRG-ROM, so
            // a store drives the written byte ANDed with the ROM byte already at
            // that address. Same treatment as the sibling Sunsoft-2 board in
            // `m089_sunsoft2.rs`, and matching the designated reference
            // `ref-proj/GeraNES/src/GeraNES/Mappers/Mapper093.h`, whose
            // `writePrg` opens with `data &= readPrg(addr);`.
            // Decode every field from the masked value.
            let value = value & self.read_prg(addr);
            // [.PPP ...E]: bits 4-6 = 16K PRG bank, bit 0 = CHR-RAM enable.
            self.prg_bank = (value >> 4) & 0x07;
            self.chr_ram_enabled = (value & 0x01) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_ram_enabled {
                    self.chr_ram[addr as usize]
                } else {
                    // CHR disabled: reads are open bus (0 here).
                    0
                }
            }
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_ram_enabled {
                    self.chr_ram[addr as usize] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = self.nametable_offset(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 93,
            name: "Sunsoft-3R (93)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("PRG".into(), format!("{:#04x}", self.prg_bank)));
        info.extra.push((
            "CHR-RAM".into(),
            if self.chr_ram_enabled {
                "enabled".into()
            } else {
                "disabled".into()
            },
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.chr_ram_enabled));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank = data[1];
        self.chr_ram_enabled = data[2] != 0;
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    /// Filled `0xFF` — NOT `0x00` — with only the first byte of each bank
    /// carrying its index as a marker.
    ///
    /// This board has bus conflicts: `cpu_write` ANDs the written byte with the
    /// ROM byte at that address. A `0x00` fill would silently mask every
    /// register write to zero and make these tests assert the wrong behavior,
    /// so register writes below target `$8001` (a `0xFF` byte, mask
    /// transparent) rather than `$8000` (the bank marker). Same convention as
    /// the sibling `m089_sunsoft2.rs`.
    fn synth_prg(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn defaults_first_prg_last_fixed() {
        let mut m = Sunsoft3r::new(synth_prg(8), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 7); // last 16K fixed
    }

    #[test]
    fn prg_bank_select_bits_4_to_6() {
        let mut m = Sunsoft3r::new(synth_prg(8), Mirroring::Vertical).unwrap();
        // [.PPP ...E]: PRG=5 (bits 4-6) + CHR enable (bit 0) -> 0b0101_0001.
        m.cpu_write(0x8001, 0b0101_0001);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 7); // fixed unchanged
    }

    #[test]
    fn chr_ram_enable_gates_access() {
        let mut m = Sunsoft3r::new(synth_prg(2), Mirroring::Horizontal).unwrap();
        // Default enabled: round-trip works.
        m.ppu_write(0x0010, 0xCD);
        assert_eq!(m.ppu_read(0x0010), 0xCD);
        // Disable CHR-RAM (bit 0 = 0): writes ignored, reads open bus (0).
        m.cpu_write(0x8001, 0x00);
        m.ppu_write(0x0020, 0xEE);
        assert_eq!(m.ppu_read(0x0020), 0);
        // Re-enable: previously-written byte still there.
        m.cpu_write(0x8001, 0x01);
        assert_eq!(m.ppu_read(0x0010), 0xCD);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = Sunsoft3r::new(synth_prg(4), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8001, 0b0010_0001);
        m.ppu_write(0x0001, 0x77);
        let blob = m.save_state();
        let mut m2 = Sunsoft3r::new(synth_prg(4), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0001), m2.ppu_read(0x0001));
    }
}
