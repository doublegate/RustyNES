//! Taito TC0690 (iNES mapper 48) implementation.
//!
//! The IRQ-bearing sibling of the TC0190 (mapper 33). Same PRG/CHR banking
//! shape — two switchable 8 KiB PRG banks (the upper two slots fixed to the
//! last two banks), two switchable 2 KiB CHR banks, four switchable 1 KiB CHR
//! banks — plus an **MMC3-style A12 scanline IRQ counter** and a mirroring
//! register at `$E000` bit 6. Used by Don Doko Don 2, Flintstones 2, Jetsons,
//! Bakushou!! Jinsei Gekijou 3.
//!
//! Register map (nesdev `INES_Mapper_048.xhtml`):
//!
//! ```text
//!   $8000 [M... PPPP]  M = (subm) mirroring on some boards; P = PRG reg 0 (8 KiB @ $8000)
//!   $8001 [..PP PPPP]  PRG reg 1 (8 KiB @ $A000)
//!   $8002 [CCCC CCCC]  CHR reg 0 (2 KiB @ $0000)
//!   $8003 [CCCC CCCC]  CHR reg 1 (2 KiB @ $0800)
//!   $A000 [CCCC CCCC]  CHR reg 2 (1 KiB @ $1000)
//!   $A001 [CCCC CCCC]  CHR reg 3 (1 KiB @ $1400)
//!   $A002 [CCCC CCCC]  CHR reg 4 (1 KiB @ $1800)
//!   $A003 [CCCC CCCC]  CHR reg 5 (1 KiB @ $1C00)
//!   $C000 [IIII IIII]  IRQ latch (reload value, inverted on this board)
//!   $C001 [....  ...]  IRQ reload (clear counter, reload on next A12 rise)
//!   $C002 [....  ...]  IRQ enable (acknowledge + enable)
//!   $C003 [....  ...]  IRQ disable (acknowledge + disable)
//!   $E000 [.M.. ....]  M = mirroring (bit 6: 0 = Vertical, 1 = Horizontal)
//! ```
//!
//! The IRQ counter is the MMC3 A12-edge model. The TC0690 latch byte is the
//! reload value XOR-decremented by hardware (`value ^ 0xFF`, then the counter
//! reloads at `latch + 1`); we model the latch as `value ^ 0xFF` so the
//! counter behaves like an MMC3 reload of that period. The TC0690 has a
//! 1-CPU-cycle IRQ-assert delay relative to MMC3 that we do not model exactly
//! (close enough for every licensed game; documented in `docs/mappers.md`).
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

/// Taito TC0690 mapper (iNES mapper 48).
#[allow(clippy::struct_excessive_bools)]
pub struct TaitoTc0690 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: [u8; 2],
    chr_2k: [u8; 2],
    chr_1k: [u8; 4],
    mirroring: Mirroring,
    // MMC3-style A12 IRQ counter state.
    irq_latch: u8,
    irq_counter: u8,
    irq_reload_pending: bool,
    irq_enabled: bool,
    irq_pending_line: bool,
    last_a12: bool,
    cpu_cycle: u64,
    a12_low_cycle: u64,
}

impl TaitoTc0690 {
    /// Construct a new Taito TC0690 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB. CHR-RAM is selected when
    /// `chr_rom` is empty; otherwise CHR-ROM length must be a multiple of 2 KiB.
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
                "Taito-48 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len() % CHR_BANK_2K == 0 {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "Taito-48 expects a 2 KiB multiple of CHR; got {} bytes",
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
            irq_latch: 0,
            irq_counter: 0,
            irq_reload_pending: false,
            irq_enabled: false,
            irq_pending_line: false,
            last_a12: false,
            cpu_cycle: 0,
            a12_low_cycle: 0,
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
            2 => bank_count - 2,
            _ => bank_count - 1,
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
                let idx = ((addr >> 10) & 0x03) as usize;
                let base = (self.chr_1k[idx] as usize) * CHR_BANK_1K;
                (base + (addr as usize & (CHR_BANK_1K - 1))) % len
            }
        }
    }

    /// Clock the MMC3-style IRQ counter on a filtered A12 rising edge.
    /// Returns `true` if the counter transitioned to zero (assert).
    const fn clock_irq(&mut self) -> bool {
        if self.irq_counter == 0 || self.irq_reload_pending {
            self.irq_counter = self.irq_latch;
            self.irq_reload_pending = false;
        } else {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
        }
        self.irq_counter == 0 && self.irq_enabled
    }
}

impl Mapper for TaitoTc0690 {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
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
        match addr & 0xE003 {
            0x8000 => self.prg_bank[0] = value & 0x3F,
            0x8001 => self.prg_bank[1] = value & 0x3F,
            0x8002 => self.chr_2k[0] = value,
            0x8003 => self.chr_2k[1] = value,
            0xA000 => self.chr_1k[0] = value,
            0xA001 => self.chr_1k[1] = value,
            0xA002 => self.chr_1k[2] = value,
            0xA003 => self.chr_1k[3] = value,
            0xC000 => {
                // The TC0690 latch is the one's-complement of the reload value.
                self.irq_latch = value ^ 0xFF;
            }
            0xC001 => {
                self.irq_counter = 0;
                self.irq_reload_pending = true;
            }
            0xC002 => {
                self.irq_enabled = true;
            }
            0xC003 => {
                self.irq_enabled = false;
                self.irq_pending_line = false;
            }
            0xE000 => {
                self.mirroring = if (value & 0x40) != 0 {
                    Mirroring::Horizontal
                } else {
                    Mirroring::Vertical
                };
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

    fn notify_a12(&mut self, level: bool) {
        // MMC3 A12 filter: a rising edge < 3 CPU cycles after the prior fall
        // is filtered. The TC0690 uses the same A12-edge counter mechanism.
        if !self.last_a12 && level {
            let gap = self.cpu_cycle.saturating_sub(self.a12_low_cycle);
            if gap >= 3 && self.clock_irq() {
                self.irq_pending_line = true;
            }
        } else if self.last_a12 && !level {
            self.a12_low_cycle = self.cpu_cycle;
        }
        self.last_a12 = level;
    }

    fn notify_cpu_cycle(&mut self) {
        self.cpu_cycle = self.cpu_cycle.wrapping_add(1);
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending_line
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 48,
            name: "Taito TC0690 (48)".into(),
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
        info.irq_state
            .push(("counter".into(), format!("{:#04x}", self.irq_counter)));
        info.irq_state
            .push(("latch".into(), format!("{:#04x}", self.irq_latch)));
        info.irq_state
            .push(("enabled".into(), format!("{}", u8::from(self.irq_enabled))));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            64 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_bank);
        out.extend_from_slice(&self.chr_2k);
        out.extend_from_slice(&self.chr_1k);
        out.push(u8::from(self.mirroring == Mirroring::Horizontal));
        out.push(self.irq_latch);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_reload_pending));
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending_line));
        out.push(u8::from(self.last_a12));
        out.extend_from_slice(&self.cpu_cycle.to_le_bytes());
        out.extend_from_slice(&self.a12_low_cycle.to_le_bytes());
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let need_chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        // 1 ver + 2 prg + 2 chr2k + 4 chr1k + 1 mir + 1 latch + 1 counter
        //   + 1 reload + 1 enabled + 1 pending + 1 last_a12 + 8 cpu + 8 low
        //   = 32 header bytes.
        let header = 32;
        let expected = header + self.vram.len() + need_chr;
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
        self.irq_latch = data[10];
        self.irq_counter = data[11];
        self.irq_reload_pending = data[12] != 0;
        self.irq_enabled = data[13] != 0;
        self.irq_pending_line = data[14] != 0;
        self.last_a12 = data[15] != 0;
        self.cpu_cycle = u64::from_le_bytes(data[16..24].try_into().unwrap());
        self.a12_low_cycle = u64::from_le_bytes(data[24..32].try_into().unwrap());
        let mut cursor = header;
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

    fn pulse_a12(m: &mut TaitoTc0690) {
        // Advance CPU cycles so the A12 filter gap is satisfied, then rise.
        m.notify_a12(false);
        for _ in 0..4 {
            m.notify_cpu_cycle();
        }
        m.notify_a12(true);
    }

    #[test]
    fn prg_banks_and_fixed_tail() {
        let mut m = TaitoTc0690::new(synth_prg(8), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xA000), 0);
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
        m.cpu_write(0x8000, 0x03);
        m.cpu_write(0x8001, 0x05);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xA000), 5);
    }

    #[test]
    fn mirroring_at_e000_bit6() {
        let mut m = TaitoTc0690::new(synth_prg(4), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 0x40);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0xE000, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn chr_2k_and_1k_banks() {
        let mut m = TaitoTc0690::new(synth_prg(4), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8002, 0x02); // 2 KiB bank 2 = 1 KiB idx 4
        assert_eq!(m.ppu_read(0x0000), 4);
        m.cpu_write(0xA000, 0x09);
        assert_eq!(m.ppu_read(0x1000), 9);
        m.cpu_write(0xA003, 0x0B);
        assert_eq!(m.ppu_read(0x1C00), 11);
    }

    #[test]
    fn irq_counts_down_and_asserts() {
        let mut m = TaitoTc0690::new(synth_prg(4), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        // Latch value 0xFE -> latch byte = 0xFE ^ 0xFF = 0x01 (period 1).
        m.cpu_write(0xC000, 0xFE);
        m.cpu_write(0xC001, 0x00); // reload pending
        m.cpu_write(0xC002, 0x00); // enable
                                   // First filtered rise: reload counter to latch (1).
        pulse_a12(&mut m);
        assert!(!m.irq_pending());
        // Second filtered rise: decrement 1 -> 0, assert.
        pulse_a12(&mut m);
        assert!(m.irq_pending());
        // Disable acknowledges the line.
        m.cpu_write(0xC003, 0x00);
        assert!(!m.irq_pending());
    }

    #[test]
    fn irq_filter_rejects_close_rise() {
        let mut m = TaitoTc0690::new(synth_prg(4), synth_chr_1k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0xC000, 0xFF); // latch = 0x00 (period 0)
        m.cpu_write(0xC001, 0x00);
        m.cpu_write(0xC002, 0x00);
        // Rise too soon after fall (gap < 3): filtered, no clock.
        m.notify_a12(false);
        m.notify_cpu_cycle();
        m.notify_a12(true);
        assert!(!m.irq_pending());
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = TaitoTc0690::new(synth_prg(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x04);
        m.cpu_write(0x8002, 0x03);
        m.cpu_write(0xC000, 0xF0);
        m.cpu_write(0xC002, 0x00);
        m.cpu_write(0xE000, 0x40);
        pulse_a12(&mut m);
        let blob = m.save_state();
        let mut m2 = TaitoTc0690::new(synth_prg(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
        assert_eq!(m.current_mirroring(), m2.current_mirroring());
        assert_eq!(m.irq_pending(), m2.irq_pending());
    }
}
