//! Taito X1-005 (iNES mapper 80) implementation.
//!
//! A Taito ASIC board (Kyonshiizu 2, Kyuukyoku Harikiri Koushien on the
//! related X1-017 board, Bakushou!! Jinsei Gekijou, etc.). The chip exposes a
//! small register window high in the `$6000-$7FFF` space plus an on-cart
//! 128-byte battery RAM. There is **no IRQ**.
//!
//! Register map (nesdev `INES_Mapper_080.xhtml`):
//!
//! ```text
//!   $7EF0 [CCCC CCCM]  2 KiB CHR bank -> PPU $0000-$07FF (value & 0xFE)
//!   $7EF1 [CCCC CCCM]  2 KiB CHR bank -> PPU $0800-$0FFF (value & 0xFE)
//!   $7EF2 [CCCC CCCC]  1 KiB CHR bank -> PPU $1000-$13FF
//!   $7EF3 [CCCC CCCC]  1 KiB CHR bank -> PPU $1400-$17FF
//!   $7EF4 [CCCC CCCC]  1 KiB CHR bank -> PPU $1800-$1BFF
//!   $7EF5 [CCCC CCCC]  1 KiB CHR bank -> PPU $1C00-$1FFF
//!   $7EF6 [.... ...M]  M = mirroring (0 = Horizontal, 1 = Vertical)
//!   $7EF8 [VVVV VVVV]  RAM enable port A (write $A3 to enable)
//!   $7EF9 [VVVV VVVV]  RAM enable port B (write $A3 to enable)
//!   $7EFA/$7EFB [PPPP PPPP]  8 KiB PRG bank -> $8000-$9FFF
//!   $7EFC/$7EFD [PPPP PPPP]  8 KiB PRG bank -> $A000-$BFFF
//!   $7EFE/$7EFF [PPPP PPPP]  8 KiB PRG bank -> $C000-$DFFF
//!   $7F00-$7FFF        128-byte battery RAM (mirrored every $80) — enabled by
//!                      writing $A3 to BOTH $7EF8 and $7EF9.
//! ```
//!
//! The two 2 KiB CHR registers ($7EF0/$7EF1) carry the CHR bank in the upper
//! seven bits; the standard board ignores the low "nametable" bit and uses the
//! `$7EF6` software H/V control for CIRAM mirroring, which is what we model.
//! There are THREE switchable 8 KiB PRG banks ($8000/$A000/$C000) selected by
//! `$7EFA`/`$7EFC`/`$7EFE` (each with an odd-address alias); only `$E000` is
//! fixed to the last bank. (nesdev `INES_Mapper_080`, verified against the
//! Mesen2 `TaitoX1005` board: missing the `$7EFE` $C000 register stranded the
//! reset bank and blanked `Kyonshiizu 2`; `$7EF6` polarity is 0=Horz/1=Vert.)
//!
//! See `docs/mappers.md` §Mapper coverage matrix.

#![allow(clippy::cast_possible_truncation, clippy::doc_markdown)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;
const RAM_LEN: usize = 0x80; // 128-byte on-cart battery RAM
const RAM_MAGIC: u8 = 0xA3;

const SAVE_STATE_VERSION: u8 = 1;

/// Taito X1-005 mapper (iNES mapper 80).
pub struct TaitoX1005 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    ram: [u8; RAM_LEN],
    chr_is_ram: bool,
    /// 1 KiB CHR bank registers for PPU `$0000-$1FFF` (eight 1 KiB windows).
    chr_1k: [u8; 8],
    /// 8 KiB PRG banks for `$8000`, `$A000` and `$C000` (`$E000` is fixed).
    prg_bank: [u8; 3],
    mirroring: Mirroring,
    /// `$7EF8` / `$7EF9` enable-latch; RAM is readable/writable only when both
    /// hold `$A3`.
    ram_enable: [u8; 2],
}

impl TaitoX1005 {
    /// Construct a new Taito X1-005 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB. CHR-RAM is selected when
    /// `chr_rom` is empty; otherwise CHR-ROM length must be a multiple of 1 KiB.
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
                "Taito-X1-005 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Taito-X1-005 expects a 1 KiB multiple of CHR; got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            ram: [0u8; RAM_LEN],
            chr_is_ram,
            chr_1k: [0, 1, 2, 3, 4, 5, 6, 7],
            prg_bank: [0, 1, 2],
            mirroring,
            ram_enable: [0, 0],
        })
    }

    const fn ram_enabled(&self) -> bool {
        self.ram_enable[0] == RAM_MAGIC && self.ram_enable[1] == RAM_MAGIC
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
            2 => self.prg_bank[2] as usize,
            // $E000-$FFFF is hard-wired to the last 8 KiB bank (the reset
            // vector lives here).
            _ => bank_count - 1,
        } % bank_count;
        let off = (addr as usize) & (PRG_BANK_8K - 1);
        self.prg_rom[bank * PRG_BANK_8K + off]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let len = self.chr.len().max(1);
        let idx = ((addr >> 10) & 0x07) as usize; // 0..=7 over $0000-$1FFF
        let base = (self.chr_1k[idx] as usize) * CHR_BANK_1K;
        (base + (addr as usize & (CHR_BANK_1K - 1))) % len
    }
}

impl Mapper for TaitoX1005 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x7F00..=0x7FFF => {
                if self.ram_enabled() {
                    self.ram[(addr as usize) & (RAM_LEN - 1)]
                } else {
                    0
                }
            }
            0x8000..=0xFFFF => self.read_prg(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // Register window. The chip decodes $7EF0-$7EFF; the 128-byte RAM
            // occupies $7F00-$7FFF. The two 2 KiB registers each drive a pair
            // of adjacent 1 KiB slots (value & 0xFE = even base, +1 for the
            // second half).
            0x7EF0 => {
                let base = value & 0xFE;
                self.chr_1k[0] = base;
                self.chr_1k[1] = base | 1;
            }
            0x7EF1 => {
                let base = value & 0xFE;
                self.chr_1k[2] = base;
                self.chr_1k[3] = base | 1;
            }
            0x7EF2 => self.chr_1k[4] = value,
            0x7EF3 => self.chr_1k[5] = value,
            0x7EF4 => self.chr_1k[6] = value,
            0x7EF5 => self.chr_1k[7] = value,
            0x7EF6 | 0x7EF7 => {
                // $7EF6 bit 0: 0 = Horizontal, 1 = Vertical (nesdev mapper 080;
                // Mesen2 TaitoX1005).
                self.mirroring = if (value & 0x01) != 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            0x7EF8 => self.ram_enable[0] = value,
            0x7EF9 => self.ram_enable[1] = value,
            0x7EFA | 0x7EFB => self.prg_bank[0] = value,
            0x7EFC | 0x7EFD => self.prg_bank[1] = value,
            0x7EFE | 0x7EFF => self.prg_bank[2] = value,
            0x7F00..=0x7FFF if self.ram_enabled() => {
                self.ram[(addr as usize) & (RAM_LEN - 1)] = value;
            }
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
            mapper_id: 80,
            name: "Taito X1-005 (80)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        for (i, b) in self.prg_bank.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr_1k.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR{i}"), format!("{b:#04x}")));
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // Header: 1 (version) + 8 (chr_1k) + 3 (prg_bank) + 1 (mirroring) +
        // 2 (ram_enable) = 15 bytes.
        let mut out = Vec::with_capacity(
            15 + RAM_LEN + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.chr_1k);
        out.extend_from_slice(&self.prg_bank);
        out.push(match self.mirroring {
            Mirroring::Horizontal => 1,
            _ => 0,
        });
        out.extend_from_slice(&self.ram_enable);
        out.extend_from_slice(&self.ram);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 15 + RAM_LEN + self.vram.len() + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_1k.copy_from_slice(&data[1..9]);
        self.prg_bank.copy_from_slice(&data[9..12]);
        self.mirroring = if data[12] == 1 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
        self.ram_enable.copy_from_slice(&data[13..15]);
        let mut cursor = 15;
        self.ram.copy_from_slice(&data[cursor..cursor + RAM_LEN]);
        cursor += RAM_LEN;
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
        let mut m = TaitoX1005::new(synth_prg(8), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        // Default: $8000 = bank 0, $A000 = bank 1, $C000 = bank 2, $E000 = {-1} = 7.
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 1);
        assert_eq!(m.cpu_read(0xC000), 2);
        assert_eq!(m.cpu_read(0xE000), 7);
        // All three switchable banks ($7EFA/$7EFC/$7EFE) select independently;
        // only $E000 is hard-wired to the last bank.
        m.cpu_write(0x7EFA, 3);
        m.cpu_write(0x7EFC, 5);
        m.cpu_write(0x7EFE, 4);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xC000), 4);
        assert_eq!(m.cpu_read(0xE000), 7);
        // Odd-address aliases hit the same registers.
        m.cpu_write(0x7EFB, 1);
        m.cpu_write(0x7EFD, 2);
        m.cpu_write(0x7EFF, 6);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.cpu_read(0xA000), 2);
        assert_eq!(m.cpu_read(0xC000), 6);
    }

    #[test]
    fn chr_2k_and_1k_banks() {
        let mut m = TaitoX1005::new(synth_prg(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        // 2K register $7EF0 (value & 0xFE) -> slots 0/1; value 4 keeps bank 4.
        m.cpu_write(0x7EF0, 4);
        assert_eq!(m.ppu_read(0x0000), 4);
        assert_eq!(m.ppu_read(0x0400), 5); // adjacent 1K slot is bank+1
        // Second 2K register $7EF1 -> slots 2/3.
        m.cpu_write(0x7EF1, 8);
        assert_eq!(m.ppu_read(0x0800), 8);
        assert_eq!(m.ppu_read(0x0C00), 9);
        // 1K registers $7EF2-$7EF5 -> slots 4..7.
        m.cpu_write(0x7EF2, 11);
        assert_eq!(m.ppu_read(0x1000), 11);
        m.cpu_write(0x7EF5, 13);
        assert_eq!(m.ppu_read(0x1C00), 13);
    }

    #[test]
    fn mirroring_register() {
        let mut m = TaitoX1005::new(synth_prg(4), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        // $7EF6 bit 0: 1 = Vertical, 0 = Horizontal (nesdev mapper 080).
        m.cpu_write(0x7EF6, 0x01);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0x7EF6, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn battery_ram_needs_both_magic_writes() {
        let mut m = TaitoX1005::new(synth_prg(4), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        // Not enabled: writes are dropped, reads return 0.
        m.cpu_write(0x7F00, 0x55);
        assert_eq!(m.cpu_read(0x7F00), 0);
        // Only one magic latch -> still disabled.
        m.cpu_write(0x7EF8, RAM_MAGIC);
        m.cpu_write(0x7F00, 0x55);
        assert_eq!(m.cpu_read(0x7F00), 0);
        // Both magic latches -> enabled.
        m.cpu_write(0x7EF9, RAM_MAGIC);
        m.cpu_write(0x7F00, 0x55);
        assert_eq!(m.cpu_read(0x7F00), 0x55);
        // Mirrors every 128 bytes within $7F00-$7FFF.
        assert_eq!(m.cpu_read(0x7F80), 0x55);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = TaitoX1005::new(synth_prg(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x7EFA, 5);
        m.cpu_write(0x7EFC, 6);
        m.cpu_write(0x7EF0, 4);
        m.cpu_write(0x7EF6, 0x01);
        m.cpu_write(0x7EF8, RAM_MAGIC);
        m.cpu_write(0x7EF9, RAM_MAGIC);
        m.cpu_write(0x7F10, 0xAB);
        let blob = m.save_state();
        let mut m2 = TaitoX1005::new(synth_prg(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.cpu_read(0xA000), m2.cpu_read(0xA000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
        assert_eq!(m2.cpu_read(0x7F10), 0xAB);
    }
}
