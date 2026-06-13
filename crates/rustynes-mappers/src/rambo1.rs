//! Tengen RAMBO-1 (iNES mapper 64) implementation.
//!
//! The RAMBO-1 is Tengen's MMC3 variant with three extra features:
//!
//! - A **third** switchable 8 KiB PRG bank (register `RF`) at the slot the
//!   MMC3 fixes to the second-to-last bank, so three of the four 8 KiB PRG
//!   windows are switchable (`$8000`/`$A000`/`$C000`), only `$E000` is fixed
//!   to the last bank.
//! - **Finer CHR banking**: a "full 1 KiB" mode bit (`$8000` bit 5) replaces
//!   the two 2 KiB CHR banks (R0/R1) with four 1 KiB banks (R0/R8 + R1/R9),
//!   so the whole `$0000-$0FFF` half can be eight 1 KiB banks.
//! - A **dual-mode IRQ**: a scanline (PPU A12) mode like MMC3 *and* a
//!   CPU-cycle mode (clocked every 4 CPU cycles), selected by `$C001` bit 0.
//!
//! See `nesdev_wiki/RAMBO_1.xhtml`, `nesdev_wiki/INES_Mapper_064.xhtml`, and
//! `docs/mappers.md`.
//!
//! # Banking registers
//!
//! `$8000` (even) bank-select: bits 0-3 pick the register, bit 5 = full-1KiB
//! CHR mode (K), bit 6 = PRG mode (P), bit 7 = CHR A12 inversion (C).
//! `$8001` (odd) writes the selected register:
//!
//! | Reg | Purpose                                                       |
//! |-----|---------------------------------------------------------------|
//! | R0  | 2 KiB CHR @ `$0000` (or 1 KiB when K=1)                        |
//! | R1  | 2 KiB CHR @ `$0800` (or 1 KiB when K=1)                        |
//! | R2  | 1 KiB CHR @ `$1000`                                            |
//! | R3  | 1 KiB CHR @ `$1400`                                            |
//! | R4  | 1 KiB CHR @ `$1800`                                            |
//! | R5  | 1 KiB CHR @ `$1C00`                                            |
//! | R6  | 8 KiB PRG @ `$8000` (P=0) / `$C000` (P=1)                      |
//! | R7  | 8 KiB PRG @ `$A000`                                            |
//! | R8  | 1 KiB CHR @ `$0400` (only when K=1)                            |
//! | R9  | 1 KiB CHR @ `$0C00` (only when K=1)                            |
//! | RF  | 8 KiB PRG @ `$C000` (P=0) / `$8000` (P=1)                      |
//!
//! Bit 7 (CHR A12 inversion) swaps the `$0xxx` and `$1xxx` halves exactly like
//! the MMC3 CHR mode bit.
//!
//! # IRQ
//!
//! `$C000` (even): IRQ latch / reload value.
//! `$C001` (odd): bit 0 = mode select (0 = scanline / PPU A12, 1 = CPU cycle).
//! Writing `$C001` also clears the counter so it reloads on the next clock.
//! `$E000` (even): disable + acknowledge. `$E001` (odd): enable.
//!
//! The IRQ counter, on each clock (scanline A12 rise or every 4 CPU cycles):
//! if a `$C001` write happened since the last clock, reload from the latch;
//! else if the counter is 0, reload; else decrement and assert on reaching 0
//! (when enabled). The +1 reload-kick quirk (`if non-zero, value | 1`) and the
//! one-cycle assertion delay are documented but not separately modelled here —
//! we treat the assert as immediate, which is sufficient for the boot-smoke +
//! register-level verification we can perform (no behavioural fixtures exist).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::match_same_arms,
    clippy::doc_markdown,
    clippy::doc_lazy_continuation
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Tengen RAMBO-1 mapper (iNES mapper 64).
pub struct Rambo1 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    // R0..R9 + RF (index 10) — 11 bank registers.
    regs: [u8; 16],
    bank_select: u8,
    prg_mode: bool,    // $8000 bit 6
    chr_mode: bool,    // $8000 bit 7 (A12 inversion)
    chr_1k_mode: bool, // $8000 bit 5 (K)

    mirroring: Mirroring,

    // IRQ.
    irq_latch: u8,
    irq_counter: u8,
    irq_reload_pending: bool,
    irq_enabled: bool,
    irq_pending: bool,
    irq_cpu_mode: bool, // false = scanline (A12), true = CPU cycle
    irq_prescaler: u8,  // /4 prescaler for CPU-cycle mode

    // A12 filter state (scanline mode).
    last_a12: bool,
    a12_low_cycle: u64,
    cpu_cycle: u64,
}

impl Rambo1 {
    /// Construct a new RAMBO-1 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB; CHR-ROM (when present)
    /// must be a multiple of 1 KiB. CHR-RAM (8 KiB) is allocated when no
    /// CHR-ROM is supplied.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || prg_rom.len() % PRG_BANK_8K != 0 {
            return Err(MapperError::Invalid(format!(
                "RAMBO-1 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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
                "RAMBO-1 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            regs: [0; 16],
            bank_select: 0,
            prg_mode: false,
            chr_mode: false,
            chr_1k_mode: false,
            mirroring,
            irq_latch: 0,
            irq_counter: 0,
            irq_reload_pending: false,
            irq_enabled: false,
            irq_pending: false,
            irq_cpu_mode: false,
            irq_prescaler: 0,
            last_a12: false,
            a12_low_cycle: 0,
            cpu_cycle: 0,
        })
    }

    /// RF lives at register index 15.
    const RF: usize = 15;

    /// PRG: R6/RF swap between `$8000` and `$C000` per `prg_mode`; R7 fixed at
    /// `$A000`; `$E000` always the last bank.
    fn prg_offset(&self, addr: u16) -> usize {
        let total = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let last = total - 1;
        let r6 = self.regs[6] as usize;
        let r7 = self.regs[7] as usize;
        let rf = self.regs[Self::RF] as usize;
        let bank = match (addr & 0xE000, self.prg_mode) {
            (0x8000, false) => r6,
            (0x8000, true) => rf,
            (0xA000, _) => r7,
            (0xC000, false) => rf,
            (0xC000, true) => r6,
            (0xE000, _) => last,
            _ => 0,
        };
        (bank % total) * PRG_BANK_8K + (addr as usize & 0x1FFF)
    }

    /// CHR slot -> 1 KiB bank index, accounting for the K (full-1KiB) mode,
    /// the C (A12 inversion) mode, and the 2 KiB even-bank forcing.
    fn chr_bank_1k(&self, slot: usize) -> usize {
        // Resolve the slot to the "low-half" semantic slot (0..8) before
        // applying A12 inversion: when chr_mode is set, the $0xxx and $1xxx
        // halves are swapped.
        let s = if self.chr_mode { slot ^ 0x4 } else { slot };
        // s now refers to the canonical layout where slots 0-3 are the
        // R0/R1 (2 KiB) region and slots 4-7 are R2-R5.
        match s {
            0 => {
                if self.chr_1k_mode {
                    self.regs[0] as usize
                } else {
                    (self.regs[0] as usize) & !1
                }
            }
            1 => {
                if self.chr_1k_mode {
                    self.regs[8] as usize
                } else {
                    ((self.regs[0] as usize) & !1) | 1
                }
            }
            2 => {
                if self.chr_1k_mode {
                    self.regs[1] as usize
                } else {
                    (self.regs[1] as usize) & !1
                }
            }
            3 => {
                if self.chr_1k_mode {
                    self.regs[9] as usize
                } else {
                    ((self.regs[1] as usize) & !1) | 1
                }
            }
            4 => self.regs[2] as usize,
            5 => self.regs[3] as usize,
            6 => self.regs[4] as usize,
            7 => self.regs[5] as usize,
            _ => 0,
        }
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_1k = (self.chr.len() / CHR_BANK_1K).max(1);
        let slot = addr / CHR_BANK_1K;
        let bank = self.chr_bank_1k(slot) % total_1k;
        bank * CHR_BANK_1K + (addr & (CHR_BANK_1K - 1))
    }

    fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.mirroring.physical_bank(table);
        physical * NAMETABLE_SIZE + local
    }

    /// Clock the 8-bit IRQ counter. Returns `true` if the IRQ line should
    /// assert. Shared by scanline and CPU-cycle modes.
    fn clock_irq(&mut self) -> bool {
        let mut would_assert = false;
        if self.irq_reload_pending {
            self.irq_counter = self.irq_latch;
            self.irq_reload_pending = false;
        } else if self.irq_counter == 0 {
            self.irq_counter = self.irq_latch;
        } else {
            self.irq_counter = self.irq_counter.wrapping_sub(1);
            if self.irq_counter == 0 && self.irq_enabled {
                would_assert = true;
            }
        }
        would_assert
    }
}

impl Mapper for Rambo1 {
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    self.bank_select = value & 0x0F;
                    self.chr_1k_mode = (value & 0x20) != 0;
                    self.prg_mode = (value & 0x40) != 0;
                    self.chr_mode = (value & 0x80) != 0;
                } else {
                    let idx = self.bank_select as usize;
                    // Registers 0-9 + RF (15) are valid; 10-14 unused.
                    if idx <= 9 || idx == Self::RF {
                        self.regs[idx] = value;
                    }
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    self.mirroring = if value & 1 == 0 {
                        Mirroring::Vertical
                    } else {
                        Mirroring::Horizontal
                    };
                }
                // $A001 odd: unimplemented on RAMBO-1 (no PRG-RAM).
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    self.irq_latch = value;
                } else {
                    self.irq_cpu_mode = (value & 1) != 0;
                    // Clear counter so it reloads on next clock; reset the
                    // CPU-cycle prescaler.
                    self.irq_reload_pending = true;
                    self.irq_counter = 0;
                    self.irq_prescaler = 0;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    self.irq_enabled = false;
                    self.irq_pending = false;
                } else {
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr[off % self.chr.len()]
            }
            0x2000..=0x3EFF => self.vram[self.nametable_offset(addr) % self.vram.len()],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    let len = self.chr.len();
                    self.chr[off % len] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = self.nametable_offset(addr) % self.vram.len();
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        let off = self.nametable_offset(addr);
        u16::try_from(off & 0x07FF).unwrap_or(0)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn notify_a12(&mut self, level: bool) {
        self.notify_a12_at_sub_dot(level, 1);
    }

    fn notify_a12_at_sub_dot(&mut self, level: bool, _sub_dot: u8) {
        // Scanline (PPU A12) mode only; CPU-cycle mode ignores A12.
        if self.irq_cpu_mode {
            self.last_a12 = level;
            return;
        }
        if !self.last_a12 && level {
            let gap = self.cpu_cycle.saturating_sub(self.a12_low_cycle);
            if gap >= 3 && self.clock_irq() {
                self.irq_pending = true;
            }
        } else if self.last_a12 && !level {
            self.a12_low_cycle = self.cpu_cycle;
        }
        self.last_a12 = level;
    }

    fn notify_cpu_cycle(&mut self) {
        self.cpu_cycle = self.cpu_cycle.wrapping_add(1);
        if !self.irq_cpu_mode {
            return;
        }
        // CPU-cycle mode: clock the counter every 4 CPU cycles.
        self.irq_prescaler = self.irq_prescaler.wrapping_add(1);
        if self.irq_prescaler >= 4 {
            self.irq_prescaler = 0;
            if self.clock_irq() {
                self.irq_pending = true;
            }
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 64,
            name: "Tengen RAMBO-1 (64)".into(),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("mode".into(), format!("{}", u8::from(self.prg_mode))));
        info.prg_banks
            .push(("R6".into(), format!("{:#04x}", self.regs[6])));
        info.prg_banks
            .push(("R7".into(), format!("{:#04x}", self.regs[7])));
        info.prg_banks
            .push(("RF".into(), format!("{:#04x}", self.regs[Self::RF])));
        info.chr_banks
            .push(("K".into(), format!("{}", u8::from(self.chr_1k_mode))));
        info.chr_banks
            .push(("C".into(), format!("{}", u8::from(self.chr_mode))));
        for i in 0..6 {
            info.chr_banks
                .push((format!("R{i}"), format!("{:#04x}", self.regs[i])));
        }
        info.chr_banks
            .push(("R8".into(), format!("{:#04x}", self.regs[8])));
        info.chr_banks
            .push(("R9".into(), format!("{:#04x}", self.regs[9])));
        info.irq_state.push((
            "mode".into(),
            if self.irq_cpu_mode {
                "cpu".into()
            } else {
                "scanline".into()
            },
        ));
        info.irq_state
            .push(("counter".into(), format!("{:#04x}", self.irq_counter)));
        info.irq_state
            .push(("latch".into(), format!("{:#04x}", self.irq_latch)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(
            48 + self.vram.len() + if self.chr_is_ram { self.chr.len() } else { 0 },
        );
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.regs);
        out.push(self.bank_select);
        out.push(u8::from(self.prg_mode));
        out.push(u8::from(self.chr_mode));
        out.push(u8::from(self.chr_1k_mode));
        out.push(self.mirroring as u8);
        out.push(self.irq_latch);
        out.push(self.irq_counter);
        out.push(u8::from(self.irq_reload_pending));
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending));
        out.push(u8::from(self.irq_cpu_mode));
        out.push(self.irq_prescaler);
        out.push(u8::from(self.last_a12));
        out.extend_from_slice(&self.a12_low_cycle.to_le_bytes());
        out.extend_from_slice(&self.cpu_cycle.to_le_bytes());
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_part = if self.chr_is_ram { self.chr.len() } else { 0 };
        // 1 (ver) + 16 (regs) + 1+1+1+1+1+1+1+1+1+1+1+1+1 (13 scalars) + 8 + 8
        let scalar_len = 1 + 16 + 13 + 8 + 8;
        let expected = scalar_len + self.vram.len() + chr_part;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.regs.copy_from_slice(&data[1..17]);
        let mut c = 17usize;
        self.bank_select = data[c];
        c += 1;
        self.prg_mode = data[c] != 0;
        c += 1;
        self.chr_mode = data[c] != 0;
        c += 1;
        self.chr_1k_mode = data[c] != 0;
        c += 1;
        self.mirroring = match data[c] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => return Err(MapperError::Invalid(format!("mirroring {other}"))),
        };
        c += 1;
        self.irq_latch = data[c];
        c += 1;
        self.irq_counter = data[c];
        c += 1;
        self.irq_reload_pending = data[c] != 0;
        c += 1;
        self.irq_enabled = data[c] != 0;
        c += 1;
        self.irq_pending = data[c] != 0;
        c += 1;
        self.irq_cpu_mode = data[c] != 0;
        c += 1;
        self.irq_prescaler = data[c];
        c += 1;
        self.last_a12 = data[c] != 0;
        c += 1;
        self.a12_low_cycle = u64::from_le_bytes(
            data[c..c + 8]
                .try_into()
                .map_err(|_| MapperError::Invalid("a12_low_cycle truncated".into()))?,
        );
        c += 8;
        self.cpu_cycle = u64::from_le_bytes(
            data[c..c + 8]
                .try_into()
                .map_err(|_| MapperError::Invalid("cpu_cycle truncated".into()))?,
        );
        c += 8;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation, clippy::identity_op)]
mod tests {
    use super::*;

    fn synth_prg(banks_8k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_8k * PRG_BANK_8K];
        for b in 0..banks_8k {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_chr(banks_1k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_1k * CHR_BANK_1K];
        for b in 0..banks_1k {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn fresh() -> Rambo1 {
        Rambo1::new(synth_prg(32), synth_chr(64), Mirroring::Vertical).unwrap()
    }

    fn select_write(m: &mut Rambo1, reg: u8, value: u8) {
        m.cpu_write(0x8000, reg);
        m.cpu_write(0x8001, value);
    }

    #[test]
    fn prg_three_switchable_plus_fixed_last() {
        let mut m = fresh();
        // mode 0: R6@$8000, R7@$A000, RF@$C000, last@$E000.
        select_write(&mut m, 6, 3); // R6 = 3
        select_write(&mut m, 7, 5); // R7 = 5
        select_write(&mut m, 0x0F, 9); // RF = 9
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xC000), 9);
        assert_eq!(m.cpu_read(0xE000), 31); // last of 32 banks
    }

    #[test]
    fn prg_mode_swaps_r6_and_rf() {
        let mut m = fresh();
        select_write(&mut m, 6, 3);
        select_write(&mut m, 0x0F, 9);
        // mode 1: RF@$8000, R6@$C000.
        m.cpu_write(0x8000, 0x40 | 0); // select R0 but set prg_mode bit
        assert_eq!(m.cpu_read(0x8000), 9); // RF
        assert_eq!(m.cpu_read(0xC000), 3); // R6
    }

    #[test]
    fn chr_2k_mode_forces_even_banks() {
        let mut m = fresh();
        select_write(&mut m, 0, 4); // R0 = 4 -> 2 KiB @ $0000
        assert_eq!(m.ppu_read(0x0000), 4);
        assert_eq!(m.ppu_read(0x0400), 5); // even-forced second 1 KiB
        select_write(&mut m, 2, 9); // R2 = 1 KiB @ $1000
        assert_eq!(m.ppu_read(0x1000), 9);
    }

    #[test]
    fn chr_1k_mode_uses_r8_r9() {
        let mut m = fresh();
        // Enable K (bit 5) and set R0/R8/R1/R9.
        m.cpu_write(0x8000, 0x20 | 0); // select R0, K=1
        m.cpu_write(0x8001, 10);
        m.cpu_write(0x8000, 0x20 | 8); // select R8, K=1
        m.cpu_write(0x8001, 11);
        assert_eq!(m.ppu_read(0x0000), 10); // R0 1 KiB
        assert_eq!(m.ppu_read(0x0400), 11); // R8 1 KiB
    }

    #[test]
    fn chr_a12_inversion_swaps_halves() {
        let mut m = fresh();
        select_write(&mut m, 0, 4); // R0 (2 KiB)
        select_write(&mut m, 2, 9); // R2 (1 KiB)
                                    // Without inversion: R0 @ $0000, R2 @ $1000.
        assert_eq!(m.ppu_read(0x0000), 4);
        assert_eq!(m.ppu_read(0x1000), 9);
        // With inversion (bit 7): R2 region moves to $0000, R0 to $1000.
        m.cpu_write(0x8000, 0x80 | 0);
        assert_eq!(m.ppu_read(0x0000), 9); // R2 now @ $0000
        assert_eq!(m.ppu_read(0x1000), 4); // R0 now @ $1000
    }

    #[test]
    fn scanline_irq_decrements_and_asserts() {
        let mut m = fresh();
        m.cpu_write(0xC000, 3); // latch = 3
        m.cpu_write(0xC001, 0); // scanline mode (bit 0 = 0), reload pending
        m.cpu_write(0xE001, 0); // enable
        for _ in 0..5 {
            m.notify_a12(false);
            for _ in 0..4 {
                m.notify_cpu_cycle();
            }
            m.notify_a12(true);
        }
        // Edge 1 reload 3; edges 2-4 -> 2,1,0 (assert).
        assert!(m.irq_pending());
    }

    #[test]
    fn cpu_cycle_irq_clocks_every_four() {
        let mut m = fresh();
        m.cpu_write(0xC000, 2); // latch = 2
        m.cpu_write(0xC001, 1); // CPU-cycle mode (bit 0 = 1), reload pending
        m.cpu_write(0xE001, 0); // enable
                                // A12 must be ignored in CPU-cycle mode.
        m.notify_a12(false);
        m.notify_a12(true);
        assert!(!m.irq_pending());
        // First /4 clock: reload 2. Next two /4 clocks: 1, 0 (assert).
        // That's 3 clocks = 12 cpu cycles.
        for _ in 0..12 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
    }

    #[test]
    fn e000_acks_and_disables() {
        let mut m = fresh();
        m.irq_pending = true;
        m.cpu_write(0xE000, 0);
        assert!(!m.irq_pending());
        assert!(!m.irq_enabled);
    }

    #[test]
    fn save_state_round_trip() {
        let mut m = fresh();
        select_write(&mut m, 6, 3);
        select_write(&mut m, 0x0F, 9);
        m.cpu_write(0xC000, 0x42);
        m.cpu_write(0xC001, 1); // cpu mode
        m.cpu_write(0xE001, 0);
        m.ppu_write(0x2000, 0x77);
        let blob = m.save_state();
        let mut m2 = fresh();
        m2.load_state(&blob).unwrap();
        assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
        assert_eq!(m.cpu_read(0xC000), m2.cpu_read(0xC000));
        assert_eq!(m.irq_cpu_mode, m2.irq_cpu_mode);
        assert_eq!(m.ppu_read(0x2000), m2.ppu_read(0x2000));
    }
}
