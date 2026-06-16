//! Taito X1-017 (iNES mapper 82) implementation.
//!
//! A Taito ASIC board (Kyuukyoku Harikiri Koushien, Kyuukyoku Harikiri Stadium
//! III, SD Keiji - Blader). Like the X1-005 it exposes a register window high
//! in `$6000-$7FFF`, but with three protectable 8 KiB PRG-RAM regions, a CHR
//! A12-inversion mode bit, and a distinct CHR-bank-to-PPU mapping. The chip
//! has an IRQ surface ($7EFD-$7EFF) that the licensed games do not use; it is
//! modelled as register storage only (no counter clock), which is sufficient
//! for those titles.
//!
//! Register map (nesdev `INES_Mapper_082.xhtml`):
//!
//! ```text
//!   $7EF0 [CCCC CCC.]  2 KiB CHR bank 0 (value >> 1)
//!   $7EF1 [CCCC CCC.]  2 KiB CHR bank 1 (value >> 1)
//!   $7EF2 [CCCC CCCC]  1 KiB CHR bank 2
//!   $7EF3 [CCCC CCCC]  1 KiB CHR bank 3
//!   $7EF4 [CCCC CCCC]  1 KiB CHR bank 4
//!   $7EF5 [CCCC CCCC]  1 KiB CHR bank 5
//!   $7EF6 [.... ..IM]  I = CHR A12 inversion (bit 1), M = mirroring (bit 0;
//!                      0 = Horizontal, 1 = Vertical)
//!   $7EF7 [VVVV VVVV]  PRG-RAM enable $6000-$67FF (write $CA)
//!   $7EF8 [VVVV VVVV]  PRG-RAM enable $6800-$6FFF (write $69)
//!   $7EF9 [VVVV VVVV]  PRG-RAM enable $7000-$73FF (write $84)
//!   $7EFA [..DC BA..]  8 KiB PRG bank -> $8000 (value >> 2)
//!   $7EFB [..DC BA..]  8 KiB PRG bank -> $A000 (value >> 2)
//!   $7EFC [..DC BA..]  8 KiB PRG bank -> $C000 (value >> 2)
//! ```
//!
//! **CHR mapping (the X1-017 quirk).** When `$7EF6` bit 1 is clear (mode 0) the
//! two 2 KiB banks occupy PPU `$0000-$0FFF` and the four 1 KiB banks
//! `$1000-$1FFF`; when set (mode 1) the layout inverts so the 1 KiB banks land
//! at `$0000-$0FFF` and the 2 KiB banks at `$1000-$1FFF`. The 2 KiB selects are
//! 7-bit and shift right by one during address calculation.
//!
//! The last 8 KiB PRG bank ($E000) is fixed to the final bank.
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
const PRG_RAM_LEN: usize = 0x2000; // one contiguous 8 KiB WRAM window $6000-$7FFF

const RAM_MAGIC0: u8 = 0xCA;
const RAM_MAGIC1: u8 = 0x69;
const RAM_MAGIC2: u8 = 0x84;

const SAVE_STATE_VERSION: u8 = 1;

/// Taito X1-017 mapper (iNES mapper 82).
pub struct TaitoX1017 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    prg_ram: Box<[u8]>,
    chr_is_ram: bool,
    /// 2 KiB CHR bank registers ($7EF0/$7EF1), stored as the 1 KiB-resolution
    /// base (`value >> 1` shifted to 1 KiB units = `(value >> 1) << 1`).
    chr_2k: [u8; 2],
    /// 1 KiB CHR bank registers ($7EF2-$7EF5).
    chr_1k: [u8; 4],
    /// CHR A12 inversion ($7EF6 bit 1).
    chr_invert: bool,
    /// 8 KiB PRG banks for $8000 / $A000 / $C000.
    prg_bank: [u8; 3],
    mirroring: Mirroring,
    /// Per-region PRG-RAM enable latches ($7EF7/$7EF8/$7EF9).
    ram_enable: [u8; 3],
    /// IRQ register surface (latch / control / acknowledge) — stored but the
    /// counter is not clocked (the licensed games never enable it).
    irq_regs: [u8; 2],
}

impl TaitoX1017 {
    /// Construct a new Taito X1-017 mapper.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when sizes don't match the constraints.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_8K != 0 {
            return Err(MapperError::Invalid(format!(
                "Taito-X1-017 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_1K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Taito-X1-017 expects a 1 KiB multiple of CHR; got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; PRG_RAM_LEN].into_boxed_slice(),
            chr_is_ram,
            chr_2k: [0, 0],
            chr_1k: [0, 0, 0, 0],
            chr_invert: false,
            prg_bank: [0, 1, 2],
            mirroring,
            ram_enable: [0, 0, 0],
            irq_regs: [0, 0],
        })
    }

    /// Whether the PRG-RAM byte at `addr` ($6000-$73FF) is enabled. The chip
    /// gates three sub-regions independently; the upper $7400-$7DFF is not
    /// RAM-mapped (and $7EF0+ is the register window). We gate the whole
    /// $6000-$7FFF window on the union for read/write simplicity, but honor the
    /// per-region unlock value for each access.
    const fn ram_enabled_for(&self, addr: u16) -> bool {
        match addr {
            0x6000..=0x67FF => self.ram_enable[0] == RAM_MAGIC0,
            0x6800..=0x6FFF => self.ram_enable[1] == RAM_MAGIC1,
            0x7000..=0x73FF => self.ram_enable[2] == RAM_MAGIC2,
            _ => false,
        }
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
            _ => bank_count - 1,
        } % bank_count;
        let off = (addr as usize) & (PRG_BANK_8K - 1);
        self.prg_rom[bank * PRG_BANK_8K + off]
    }

    /// Resolve the 1 KiB CHR bank index for one of the eight PPU `$0000-$1FFF`
    /// 1 KiB windows, honoring the A12-inversion mode.
    const fn chr_bank_for_window(&self, window: usize) -> usize {
        // `window` is 0..=7 (PPU addr >> 10). In mode 0 the 2 KiB banks occupy
        // the low half ($0000-$0FFF = windows 0-3) and the 1 KiB banks the high
        // half ($1000-$1FFF = windows 4-7). Mode 1 swaps the halves.
        let low_half = window < 4;
        let use_2k_for_low = !self.chr_invert; // mode 0 -> 2K low; mode 1 -> 1K low
        let in_2k_region = low_half == use_2k_for_low;
        let local = window & 0x03; // 0..=3 within the 4 KiB half
        if in_2k_region {
            // Two 2 KiB banks span this 4 KiB half (windows pair up).
            let base = (self.chr_2k[(local >> 1) & 1] as usize) << 1; // 1K base
            base + (local & 1)
        } else {
            self.chr_1k[local] as usize
        }
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let len = self.chr.len().max(1);
        let window = ((addr >> 10) & 0x07) as usize;
        let base = self.chr_bank_for_window(window) * CHR_BANK_1K;
        (base + (addr as usize & (CHR_BANK_1K - 1))) % len
    }
}

impl Mapper for TaitoX1017 {
    // v2.8.0 Phase 4 — no per-cycle hooks (no IRQ, no audio): the bus
    // skips all four per-CPU-cycle dispatches for this board.
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x73FF => {
                if self.ram_enabled_for(addr) {
                    self.prg_ram[(addr as usize) - 0x6000]
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
            0x6000..=0x73FF => {
                if self.ram_enabled_for(addr) {
                    self.prg_ram[(addr as usize) - 0x6000] = value;
                }
            }
            0x7EF0 => self.chr_2k[0] = value >> 1,
            0x7EF1 => self.chr_2k[1] = value >> 1,
            0x7EF2 => self.chr_1k[0] = value,
            0x7EF3 => self.chr_1k[1] = value,
            0x7EF4 => self.chr_1k[2] = value,
            0x7EF5 => self.chr_1k[3] = value,
            0x7EF6 => {
                self.chr_invert = (value & 0x02) != 0;
                self.mirroring = if (value & 0x01) != 0 {
                    Mirroring::Vertical
                } else {
                    Mirroring::Horizontal
                };
            }
            0x7EF7 => self.ram_enable[0] = value,
            0x7EF8 => self.ram_enable[1] = value,
            0x7EF9 => self.ram_enable[2] = value,
            0x7EFA => self.prg_bank[0] = value >> 2,
            0x7EFB => self.prg_bank[1] = value >> 2,
            0x7EFC => self.prg_bank[2] = value >> 2,
            0x7EFD => self.irq_regs[0] = value,
            0x7EFE => self.irq_regs[1] = value,
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
            mapper_id: 82,
            name: "Taito X1-017 (82)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("chr_inv".into(), format!("{}", u8::from(self.chr_invert))));
        for (i, b) in self.prg_bank.iter().enumerate() {
            info.prg_banks
                .push((format!("PRG{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr_2k.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR2K{i}"), format!("{b:#04x}")));
        }
        for (i, b) in self.chr_1k.iter().enumerate() {
            info.chr_banks
                .push((format!("CHR1K{i}"), format!("{b:#04x}")));
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            17 + PRG_RAM_LEN + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.chr_2k);
        out.extend_from_slice(&self.chr_1k);
        out.push(u8::from(self.chr_invert));
        out.extend_from_slice(&self.prg_bank);
        out.push(match self.mirroring {
            Mirroring::Vertical => 1,
            _ => 0,
        });
        out.extend_from_slice(&self.ram_enable);
        out.extend_from_slice(&self.irq_regs);
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 17 + PRG_RAM_LEN + self.vram.len() + need_chr;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_2k.copy_from_slice(&data[1..3]);
        self.chr_1k.copy_from_slice(&data[3..7]);
        self.chr_invert = data[7] != 0;
        self.prg_bank.copy_from_slice(&data[8..11]);
        self.mirroring = if data[11] == 1 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
        self.ram_enable.copy_from_slice(&data[12..15]);
        self.irq_regs.copy_from_slice(&data[15..17]);
        let mut cursor = 17;
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + PRG_RAM_LEN]);
        cursor += PRG_RAM_LEN;
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
    fn prg_banks_value_shifted_and_fixed_tail() {
        let mut m = TaitoX1017::new(synth_prg(16), synth_chr_1k(8), Mirroring::Horizontal).unwrap();
        // PRG bank value is value >> 2. Write 0x0C -> bank 3.
        m.cpu_write(0x7EFA, 0x0C);
        assert_eq!(m.cpu_read(0x8000), 3);
        m.cpu_write(0x7EFB, 0x10); // bank 4
        assert_eq!(m.cpu_read(0xA000), 4);
        m.cpu_write(0x7EFC, 0x14); // bank 5
        assert_eq!(m.cpu_read(0xC000), 5);
        // $E000 fixed to last bank (16 banks -> bank 15).
        assert_eq!(m.cpu_read(0xE000), 15);
    }

    #[test]
    fn chr_2k_value_shifted_mode0() {
        let mut m = TaitoX1017::new(synth_prg(8), synth_chr_1k(16), Mirroring::Horizontal).unwrap();
        // Mode 0 (default): 2K banks at $0000-$0FFF, 1K banks at $1000-$1FFF.
        // $7EF0 value 8 -> chr_2k base = 8>>1 = 4 -> 1K base 8; window 0 = bank 8.
        m.cpu_write(0x7EF0, 8);
        assert_eq!(m.ppu_read(0x0000), 8);
        assert_eq!(m.ppu_read(0x0400), 9); // second 1K of the 2K bank
        m.cpu_write(0x7EF1, 10); // base = 5 -> 1K base 10
        assert_eq!(m.ppu_read(0x0800), 10);
        assert_eq!(m.ppu_read(0x0C00), 11);
        // 1K banks in the high half.
        m.cpu_write(0x7EF2, 12);
        assert_eq!(m.ppu_read(0x1000), 12);
        m.cpu_write(0x7EF5, 15);
        assert_eq!(m.ppu_read(0x1C00), 15);
    }

    #[test]
    fn chr_a12_inversion_swaps_halves() {
        let mut m = TaitoX1017::new(synth_prg(8), synth_chr_1k(16), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x7EF0, 8); // 2K bank -> base 8
        m.cpu_write(0x7EF2, 12); // 1K bank
        // Mode 1: 1K banks at low half, 2K banks at high half.
        m.cpu_write(0x7EF6, 0x02);
        assert_eq!(m.ppu_read(0x0000), 12); // 1K bank now at $0000
        assert_eq!(m.ppu_read(0x1000), 8); // 2K bank now at $1000
        assert_eq!(m.ppu_read(0x1400), 9);
    }

    #[test]
    fn mirroring_register_bit0() {
        let mut m = TaitoX1017::new(synth_prg(4), synth_chr_1k(8), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x7EF6, 0x01); // bit 0 set -> Vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0x7EF6, 0x00); // -> Horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn prg_ram_per_region_unlock() {
        let mut m = TaitoX1017::new(synth_prg(4), synth_chr_1k(8), Mirroring::Horizontal).unwrap();
        // Region 0 ($6000-$67FF) needs $CA.
        m.cpu_write(0x6000, 0x55);
        assert_eq!(m.cpu_read(0x6000), 0);
        m.cpu_write(0x7EF7, RAM_MAGIC0);
        m.cpu_write(0x6000, 0x55);
        assert_eq!(m.cpu_read(0x6000), 0x55);
        // Region 2 ($7000-$73FF) needs $84, not $CA.
        m.cpu_write(0x7EF7, RAM_MAGIC2);
        m.cpu_write(0x7000, 0x66);
        assert_eq!(m.cpu_read(0x7000), 0);
        m.cpu_write(0x7EF9, RAM_MAGIC2);
        m.cpu_write(0x7000, 0x66);
        assert_eq!(m.cpu_read(0x7000), 0x66);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m =
            TaitoX1017::new(synth_prg(16), synth_chr_1k(16), Mirroring::Horizontal).unwrap();
        m.cpu_write(0x7EFA, 0x10);
        m.cpu_write(0x7EF0, 8);
        m.cpu_write(0x7EF6, 0x03); // invert + vertical
        m.cpu_write(0x7EF7, RAM_MAGIC0);
        m.cpu_write(0x6010, 0x99);
        let blob = m.save_state();
        let mut m2 =
            TaitoX1017::new(synth_prg(16), synth_chr_1k(16), Mirroring::Horizontal).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x1000), m2.ppu_read(0x1000));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
        assert_eq!(m2.cpu_read(0x6010), 0x99);
    }
}
