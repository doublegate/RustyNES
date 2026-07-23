//! TXC `UNL-22211` (mapper 132).
//!
//! Built around the TXC bank-select ASIC, modelled here as [`TxcChip`]: a
//! small accumulator-and-latch state machine driven through the
//! `$4100-$4103` window whose output only reaches the banking registers when
//! the game subsequently writes `$8000`. That two-stage handshake is the whole
//! point of the chip -- it is a crude copy-protection measure, since a naive
//! emulator that banks on the `$4100` write alone produces the wrong bank.
//!
//! The simpler TXC board on mapper 36 is in `m036_txc_policeman.rs`; Sachen's
//! 3011 drives a variant of the same chip, duplicated in `m136_sachen_3011.rs`.
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
    clippy::bool_to_int_with_if,
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::doc_markdown,
    clippy::match_same_arms,
    clippy::missing_const_for_fn,
    clippy::similar_names,
    clippy::struct_excessive_bools,
    clippy::too_many_lines,
    clippy::unreadable_literal
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_32K: usize = 0x8000;
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

/// The TXC scrambling-accumulator chip (mappers 132 / 172 / 173 family). This
/// is the non-JV001 variant used by mapper 132.
#[derive(Clone, Copy, Default)]
struct TxcChip {
    accumulator: u8,
    inverter: u8,
    staging: u8,
    output: u8,
    increase: bool,
    invert: bool,
}

impl TxcChip {
    const MASK: u8 = 0x07;

    const fn output(self) -> u8 {
        self.output
    }

    const fn read(self) -> u8 {
        let invert_xor = if self.invert { 0xFF } else { 0x00 };
        (self.accumulator & Self::MASK) | ((self.inverter ^ invert_xor) & !Self::MASK)
    }

    /// `absolute` is the full CPU address of the write (e.g. `0x4100` or
    /// `0x8000`); `value` is the 4-bit-masked data already supplied by the
    /// caller for the register path.
    const fn write(&mut self, absolute: u16, value: u8) {
        if absolute < 0x8000 {
            match absolute & 0xE103 {
                0x4100 => {
                    if self.increase {
                        self.accumulator = self.accumulator.wrapping_add(1);
                    } else {
                        let invert_xor = if self.invert { 0xFF } else { 0x00 };
                        self.accumulator = ((self.accumulator & !Self::MASK)
                            | (self.staging & Self::MASK))
                            ^ invert_xor;
                    }
                }
                0x4101 => self.invert = (value & 0x01) != 0,
                0x4102 => {
                    self.staging = value & Self::MASK;
                    self.inverter = value & !Self::MASK;
                }
                0x4103 => self.increase = (value & 0x01) != 0,
                _ => {}
            }
        } else {
            // $8000+ latches the scrambled output (non-JV001 layout).
            self.output = (self.accumulator & 0x0F) | ((self.inverter & 0x08) << 1);
        }
    }
}

/// Mapper 132 (TXC 22211).
pub struct Txc132 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    txc: TxcChip,
    mirroring: Mirroring,
}

impl Txc132 {
    /// Construct a new mapper 132 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 132 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 132 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            txc: TxcChip::default(),
            mirroring,
        })
    }
}

impl Mapper for Txc132 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The chip's read port lives at $4100-$5FFF (mapped); only the $4020-$40FF
    // gap below it is open bus. $8000-$FFFF PRG-ROM stays mapped (the trait
    // default) — a `!(...)` here would wrongly open-bus the program ROM and the
    // reset vector, so the board never boots.
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x40FF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4100..=0x5FFF => {
                // GeraNES decodes the read on (addr & 0x0103) == 0x0100.
                if (addr & 0x0103) == 0x0100 {
                    self.txc.read() & 0x0F
                } else {
                    0
                }
            }
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
                let bank = (((self.txc.output() >> 2) & 0x01) as usize) % count;
                self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x4100..=0x5FFF).contains(&addr) || (0x8000..=0xFFFF).contains(&addr) {
            self.txc.write(addr, value & 0x0F);
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = ((self.txc.output() & 0x03) as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(7 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.txc.accumulator);
        out.push(self.txc.inverter);
        out.push(self.txc.staging);
        out.push(self.txc.output);
        out.push(u8::from(self.txc.increase));
        out.push(u8::from(self.txc.invert));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 7 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.txc.accumulator = data[1];
        self.txc.inverter = data[2];
        self.txc.staging = data[3];
        self.txc.output = data[4];
        self.txc.increase = data[5] != 0;
        self.txc.invert = data[6] != 0;
        self.vram.copy_from_slice(&data[7..7 + self.vram.len()]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
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
    fn m132_txc_chip_drives_banks() {
        let mut m = Txc132::new(synth_prg_32k(2), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // Program the chip: set staging via $4102 (low 3 bits = staging,
        // high bits -> inverter), set increase off ($4103 = 0), then $4100
        // loads accumulator from staging, then $8000 latches the output.
        m.cpu_write(0x4103, 0x00); // increase = false
        m.cpu_write(0x4102, 0b0000_1011 & 0x0F); // staging = 3 (0b011), inverter = 0b1000
        m.cpu_write(0x4100, 0x00); // accumulator = staging (no invert) = 3
        m.cpu_write(0x8000, 0x00); // latch: output = (acc&0xF) | ((inv&8)<<1)
        // acc = 3, inverter low nibble 0b1000 -> (8<<1)=0x10
        // output = 3 | 0x10 = 0x13.
        // PRG = (0x13>>2)&1 = 0; CHR = 0x13&3 = 3.
        assert_eq!(m.cpu_read(0x8000), 0); // PRG bank 0
        assert_eq!(m.ppu_read(0x0000), 3); // CHR bank 3
        // Register read window is mapped (not open bus).
        assert!(!m.cpu_read_unmapped(0x4100));
    }

    #[test]
    fn m132_save_state_round_trips_txc_chip_state() {
        let mut t = Txc132::new(synth_prg_32k(2), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        t.cpu_write(0x4103, 0x00);
        t.cpu_write(0x4102, 0x03);
        t.cpu_write(0x4100, 0x00);
        t.cpu_write(0x8000, 0x00);
        let blob = t.save_state();
        let mut t2 = Txc132::new(synth_prg_32k(2), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        t2.load_state(&blob).unwrap();
        assert_eq!(t2.ppu_read(0x0000), t.ppu_read(0x0000));
    }
}
