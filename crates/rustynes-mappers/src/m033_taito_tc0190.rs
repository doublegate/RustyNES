//! Taito TC0190 / TC0350 (iNES mapper 33) implementation.
//!
//! A Taito ASIC board with two switchable 8 KiB PRG banks (the upper two 8 KiB
//! PRG slots are hardwired to the last two banks), two switchable 2 KiB CHR
//! banks, four switchable 1 KiB CHR banks, and a software mirroring control.
//! There is **no IRQ** — that is the very-similar mapper 48 (TC0690), which
//! adds the MMC3-style A12 IRQ counter and inverts the mirroring bit.
//!
//! Register map (nesdev `INES_Mapper_033.xhtml`), `$8000-$BFFF` mask `$A003`:
//!
//! ```text
//!   $8000 [.MPP PPPP]  M = mirroring (0 = Vertical, 1 = Horizontal)
//!                      P = PRG reg 0 (8 KiB @ $8000)
//!   $8001 [..PP PPPP]  PRG reg 1 (8 KiB @ $A000)
//!   $8002 [CCCC CCCC]  CHR reg 0 (2 KiB @ $0000)
//!   $8003 [CCCC CCCC]  CHR reg 1 (2 KiB @ $0800)
//!   $A000 [CCCC CCCC]  CHR reg 2 (1 KiB @ $1000)
//!   $A001 [CCCC CCCC]  CHR reg 3 (1 KiB @ $1400)
//!   $A002 [CCCC CCCC]  CHR reg 4 (1 KiB @ $1800)
//!   $A003 [CCCC CCCC]  CHR reg 5 (1 KiB @ $1C00)
//! ```
//!
//! The 2 KiB CHR registers do NOT drop their LSB (unlike MMC3): the written
//! value is the bank offset in 2 KiB units.
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const CHR_BANK_2K: usize = 0x0800;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Taito TC0190 / TC0350 mapper (iNES mapper 33).
pub struct TaitoTc0190 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: [u8; 2],
    /// Two 2 KiB CHR banks ($8002/$8003) then four 1 KiB CHR banks ($A000-3).
    chr_2k: [u8; 2],
    chr_1k: [u8; 4],
    mirroring: Mirroring,
}

impl TaitoTc0190 {
    /// Construct a new Taito-33 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB. CHR-RAM is selected when
    /// `chr_rom` is empty; otherwise CHR-ROM length must be a multiple of 8 KiB.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match the constraints.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "Taito-33 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Taito-33 expects a 2 KiB multiple of CHR; got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: [0, 0],
            chr_2k: [0, 0],
            chr_1k: [0, 0, 0, 0],
            mirroring,
        })
    }

    const fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let bank_count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let slot = (addr >> 13) & 0x03; // 0=$8000,1=$A000,2=$C000,3=$E000
        let bank = match slot {
            0 => self.prg_bank[0] as usize,
            1 => self.prg_bank[1] as usize,
            // `saturating_sub`: a single-bank (8 KiB) PRG image is accepted by
            // the constructor, and a bare `- 2` underflows on that untrusted-ROM
            // path (panic under overflow checks; only the later `% bank_count`
            // saves release builds).
            2 => bank_count.saturating_sub(2), // fixed second-last
            _ => bank_count - 1,               // fixed last: `.max(1)` makes this safe
        } % bank_count;
        let off = (addr as usize) & (PRG_BANK_8K - 1);
        self.prg_rom[bank * PRG_BANK_8K + off]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let len = self.chr.len().max(1);
        match addr {
            0x0000..=0x07FF => {
                let base = (self.chr_2k[0] as usize) * CHR_BANK_2K;
                (base + (addr as usize & (CHR_BANK_2K - 1))) % len
            }
            0x0800..=0x0FFF => {
                let base = (self.chr_2k[1] as usize) * CHR_BANK_2K;
                (base + (addr as usize & (CHR_BANK_2K - 1))) % len
            }
            _ => {
                // $1000-$1FFF: four 1 KiB banks.
                let idx = ((addr >> 10) & 0x03) as usize; // 0..=3 over $1000-$1FFF
                let base = (self.chr_1k[idx] as usize) * CHR_BANK_1K;
                (base + (addr as usize & (CHR_BANK_1K - 1))) % len
            }
        }
    }
}

impl Mapper for TaitoTc0190 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.read_prg(addr)
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return;
        }
        // Mask $A003: registers repeat across the $8000-$FFFF window.
        match addr & 0xA003 {
            0x8000 => {
                self.mirroring = if (value & 0x40) != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
                self.prg_bank[0] = value & 0x3F;
            }
            0x8001 => self.prg_bank[1] = value & 0x3F,
            0x8002 => self.chr_2k[0] = value,
            0x8003 => self.chr_2k[1] = value,
            0xA000 => self.chr_1k[0] = value,
            0xA001 => self.chr_1k[1] = value,
            0xA002 => self.chr_1k[2] = value,
            0xA003 => self.chr_1k[3] = value,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr[off] = value;
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
            mapper_id: 33,
            name: "Taito TC0190 (33)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        for (i, b) in self.prg_bank.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr_2k.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR2k{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr_1k.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR1k{i}"), format!("{b:#04x}")));
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            10 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_bank);
        out.extend_from_slice(&self.chr_2k);
        out.extend_from_slice(&self.chr_1k);
        // Persist the software mirroring select ($8000 bit 6).
        out.push(u8::from(self.mirroring == Mirroring::Horizontal));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 10 + self.vram.len() + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_bank.copy_from_slice(&data[1..3]);
        self.chr_2k.copy_from_slice(&data[3..5]);
        self.chr_1k.copy_from_slice(&data[5..9]);
        self.mirroring = if data[9] != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
        let mut cursor = 10;
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

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
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
    fn prg_banks_and_fixed_tail() {
        let mut m = TaitoTc0190::new(synth_prg(8), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        // Defaults: PRG0=0, PRG1=0, $C000={-2}=6, $E000={-1}=7.
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 0);
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
        // Select PRG0=3, PRG1=5.
        m.cpu_write(0x8000, 0x03);
        m.cpu_write(0x8001, 0x05);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xA000), 5);
        // Fixed tail unchanged.
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn mirroring_bit_6() {
        let mut m = TaitoTc0190::new(synth_prg(4), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x40); // bit 6 set -> Horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x8000, 0x00); // bit 6 clear -> Vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn chr_2k_and_1k_banks() {
        // 2 KiB regs do not drop the LSB: written value is the 2 KiB offset.
        let mut m = TaitoTc0190::new(synth_prg(4), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        // CHR reg 0 (2 KiB @ $0000) -> bank 2 means 2 KiB offset 2 = 1 KiB idx 4.
        m.cpu_write(0x8002, 0x02);
        assert_eq!(m.ppu_read(0x0000), 4);
        // CHR reg 2 (1 KiB @ $1000) -> 1 KiB bank 9.
        m.cpu_write(0xA000, 0x09);
        assert_eq!(m.ppu_read(0x1000), 9);
        // CHR reg 5 (1 KiB @ $1C00) -> 1 KiB bank 11.
        m.cpu_write(0xA003, 0x0B);
        assert_eq!(m.ppu_read(0x1C00), 11);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = TaitoTc0190::new(synth_prg(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x42);
        m.cpu_write(0x8001, 0x05);
        m.cpu_write(0x8002, 0x03);
        m.cpu_write(0xA001, 0x07);
        let blob = m.save_state();
        let mut m2 = TaitoTc0190::new(synth_prg(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.cpu_read(0xA000), m2.cpu_read(0xA000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.ppu_read(0x1400), m2.ppu_read(0x1400));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
    }
}
