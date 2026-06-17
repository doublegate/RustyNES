//! Sprint 10 discrete-logic / multicart / pirate mappers (v1.5.0 "Lens"
//! Workstream F mapper-breadth continuation).
//!
//! A best-effort (Tier-2) batch of small pirate / unlicensed / multicart
//! boards documented concretely on the nesdev wiki (and ported in reference
//! emulators such as `Mesen2` / `GeraNES` / `puNES`). Like
//! `sprint5`..`sprint9`, banking math is translated into direct slice
//! indexing and bank selects wrap with `% count`, so a register write can
//! never index out of bounds (no panics on register access — required for the
//! `#![no_std]` chip stack).
//!
//! Most boards here are hook-free ([`MapperCaps::NONE`]); two carry a simple
//! IRQ:
//!
//! - **Mapper 12** is intentionally *not* here (it needs an MMC3-style A12 IRQ
//!   and lives with the MMC3 family); the IRQ boards in this batch use a
//!   CPU-cycle (M2) counter instead, which is the simpler hook.
//!
//! Boards implemented here:
//!
//! - **Mapper 40** (NTDEC 2722, *Super Mario Bros. 2J* pirate): fixed PRG
//!   layout with one switchable 8 KiB window at `$C000`, an enable-gated M2
//!   IRQ that fires `4096` CPU cycles after being armed (CPU-cycle hook).
//! - **Mapper 81** (NTDEC Super Gun, CNROM-like): a single `$8000-$FFFF`
//!   register, PRG bits 2-3 (16 KiB) + CHR bits 0-1 (8 KiB); header mirroring.
//! - **Mapper 95** (NAMCOT-3425, *Dragon Buster*): an MMC3-subset register
//!   port whose CHR bank-1 register's high bit drives single-screen mirroring.
//! - **Mapper 112** (NTDEC ASDER / Huang-1): an indexed `$8000`/`$A000`
//!   register port (no A12 IRQ) selecting two 8 KiB PRG + 8/2 KiB CHR banks
//!   plus a `$E000` mirroring register.
//! - **Mapper 137** (Sachen 8259D): a `$4100/$4101` command/data protection
//!   board — 32 KiB fixed PRG + four 2 KiB CHR banks with a simple bank-mode.
//! - **Mapper 156** (DIS23C01 DAOU / Open Corp): separate low/high CHR-bank
//!   registers (`$C000-$C003`/`$C008-$C00B`), a 16 KiB PRG register (`$C010`),
//!   and an explicit one-screen mirroring register (`$C014`).
//! - **Mapper 162** (Waixing FS304, *San Guo Zhi II*): four `$5000-$5FFF`
//!   nibble registers compose a 32 KiB PRG bank select; 8 KiB CHR-RAM.
//! - **Mapper 178** (Waixing / educational): a `$4800-$4803` register block
//!   (PRG mode + bank + mirroring) plus 8 KiB work-RAM at `$6000`; CHR-RAM.
//! - **Mapper 244** (Decathlon): a `$8065-$80FF` address-decoded multicart —
//!   PRG bits 3-5, CHR bits 0-2 from the low address byte; CHR-ROM.
//! - **Mapper 250** (Nitra, *Time Diver Avenger*): an MMC3-register-compatible
//!   board where the register index/value is carried in the *address* bits
//!   (`A0-A7`) rather than the data byte; CPU-cycle (M2) IRQ counter.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_1K: usize = 0x0400;
const CHR_BANK_2K: usize = 0x0800;
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

/// Mapper 40 (NTDEC 2722, *SMB2J* pirate).
pub struct Ntdec2722M40 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    switch_bank: u8,
    irq_enabled: bool,
    irq_counter: u16,
    irq_pending: bool,
    mirroring: Mirroring,
}

impl Ntdec2722M40 {
    /// Construct a new mapper 40 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 40 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
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

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for Ntdec2722M40 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.read_prg(6, addr),
            0x8000..=0x9FFF => self.read_prg(4, addr),
            0xA000..=0xBFFF => self.read_prg(5, addr),
            0xC000..=0xDFFF => self.read_prg(self.switch_bank as usize, addr),
            0xE000..=0xFFFF => self.read_prg(7, addr),
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $6000-$FFFF is mapped PRG; the $4020-$5FFF window is open bus.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF => {
                // IRQ disable + acknowledge; counter held in reset.
                self.irq_enabled = false;
                self.irq_pending = false;
                self.irq_counter = 0;
            }
            0xA000..=0xBFFF => self.irq_enabled = true,
            0xE000..=0xFFFF => self.switch_bank = value & 0x07,
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
        // 12-bit M2 counter; asserts (and holds) at 4096.
        if self.irq_counter >= 0x1000 {
            self.irq_pending = true;
        } else {
            self.irq_counter += 1;
            if self.irq_counter >= 0x1000 {
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
// Mapper 81 — NTDEC Super Gun (CNROM-like with a wider PRG select).
//
// A single $8000-$FFFF register; the written byte carries:
//   bits 2-3 : 16 KiB PRG bank at $8000 (the $C000 half is fixed to the last).
//   bits 0-1 : 8 KiB CHR bank.
// Mirroring is header-fixed. No IRQ.
// ===========================================================================

/// Mapper 81 (NTDEC Super Gun).
pub struct Ntdec81 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Ntdec81 {
    /// Construct a new mapper 81 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 81 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 81 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Ntdec81 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank as usize) % count;
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            0xC000..=0xFFFF => {
                let last = count - 1;
                self.prg_rom[last * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = (value >> 2) & 0x03;
            self.chr_bank = value & 0x03;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if (0x2000..=0x3EFF).contains(&addr) {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len();
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
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 95 — NAMCOT-3425 (Dragon Buster).
//
// An MMC3-subset register port at $8000 (index) / $8001 (data), but with no
// A12 IRQ and no PRG/CHR mode bits. The eight register slots map like MMC3's
// banking-only subset:
//   index 0/1 -> 2 KiB CHR at $0000 / $0800
//   index 2..5 -> 1 KiB CHR at $1000 / $1400 / $1800 / $1C00
//   index 6/7 -> 8 KiB PRG at $8000 / $A000 ($C000/$E000 fixed to last two)
// The board's distinctive feature: bit 5 of the value written to CHR register
// 0 (and 1) drives one-screen nametable selection (A on 0, B on 1) for that
// half of the screen; we model the simpler whole-screen single-screen select
// derived from CHR reg 0 bit 5, which is what the documented Dragon Buster
// decode uses. CHR is ROM.
// ===========================================================================

/// Mapper 95 (`NAMCOT-3425`, *Dragon Buster*).
pub struct Namcot3425M95 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    reg_index: u8,
    prg_banks: [u8; 2],
    // chr[0],chr[1] are 2 KiB selects; chr[2..6] are 1 KiB selects.
    chr_regs: [u8; 6],
    one_screen_b: bool,
}

impl Namcot3425M95 {
    /// Construct a new mapper 95 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB or CHR-ROM is empty / not a multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 95 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper 95 CHR-ROM size {} is not a non-zero multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_index: 0,
            prg_banks: [0, 1],
            chr_regs: [0; 6],
            one_screen_b: false,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let count1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        // Resolve the 1 KiB bank for this CHR address.
        let bank1k = match addr {
            0x0000..=0x07FF => (self.chr_regs[0] as usize & !1) + ((addr as usize >> 10) & 1),
            0x0800..=0x0FFF => (self.chr_regs[1] as usize & !1) + ((addr as usize >> 10) & 1),
            0x1000..=0x13FF => self.chr_regs[2] as usize,
            0x1400..=0x17FF => self.chr_regs[3] as usize,
            0x1800..=0x1BFF => self.chr_regs[4] as usize,
            _ => self.chr_regs[5] as usize,
        };
        let bank = bank1k % count1k;
        self.chr_rom[bank * CHR_BANK_1K + (addr as usize & 0x03FF)]
    }
}

impl Mapper for Namcot3425M95 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let last = (self.prg_rom.len() / PRG_BANK_8K).max(1) - 1;
        match addr {
            0x8000..=0x9FFF => self.read_prg(self.prg_banks[0] as usize, addr),
            0xA000..=0xBFFF => self.read_prg(self.prg_banks[1] as usize, addr),
            0xC000..=0xDFFF => self.read_prg(last - 1, addr),
            0xE000..=0xFFFF => self.read_prg(last, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF if addr & 1 == 0 => self.reg_index = value & 0x07,
            0x8000..=0x9FFF => match self.reg_index {
                0 => {
                    self.chr_regs[0] = value & 0x3F;
                    // CHR reg 0 bit 5 drives one-screen select on this board.
                    self.one_screen_b = (value & 0x20) != 0;
                }
                1 => self.chr_regs[1] = value & 0x3F,
                2..=5 => self.chr_regs[self.reg_index as usize] = value & 0x3F,
                6 => self.prg_banks[0] = value,
                _ => self.prg_banks[1] = value,
            },
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

    fn current_mirroring(&self) -> Mirroring {
        if self.one_screen_b {
            Mirroring::SingleScreenB
        } else {
            Mirroring::SingleScreenA
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(11 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_index);
        out.extend_from_slice(&self.prg_banks);
        out.extend_from_slice(&self.chr_regs);
        out.push(u8::from(self.one_screen_b));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 11 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg_index = data[1];
        self.prg_banks.copy_from_slice(&data[2..4]);
        self.chr_regs.copy_from_slice(&data[4..10]);
        self.one_screen_b = data[10] != 0;
        self.vram.copy_from_slice(&data[11..11 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 112 — NTDEC ASDER / Huang-1.
//
// An indexed register port (no A12 IRQ — distinct from the MMC3 it resembles):
//   $8000 : register index (bits 0-2).
//   $A000 : register data.
//   $C000 : CHR high bits / outer (modelled as an outer CHR bank add).
//   $E000 : mirroring (bit 0: 0 = vertical, 1 = horizontal).
// Register slots:
//   0 -> PRG bank at $8000 (8 KiB)
//   1 -> PRG bank at $A000 (8 KiB)
//   2 -> CHR 2 KiB at $0000
//   3 -> CHR 2 KiB at $0800
//   4..7 -> CHR 1 KiB at $1000/$1400/$1800/$1C00
// $C000/$E000 are fixed to the last two 8 KiB PRG banks. CHR is ROM.
// ===========================================================================

/// Mapper 112 (NTDEC ASDER / Huang-1).
pub struct NtdecAsder112 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    reg_index: u8,
    prg_banks: [u8; 2],
    // chr[0..2] = 2 KiB selects; chr[2..6] = 1 KiB selects.
    chr_regs: [u8; 6],
    horizontal_mirroring: bool,
}

impl NtdecAsder112 {
    /// Construct a new mapper 112 board.
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
                "mapper 112 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper 112 CHR-ROM size {} is not a non-zero multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_index: 0,
            prg_banks: [0, 1],
            chr_regs: [0; 6],
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let count1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let bank1k = match addr {
            0x0000..=0x07FF => (self.chr_regs[0] as usize & !1) + ((addr as usize >> 10) & 1),
            0x0800..=0x0FFF => (self.chr_regs[1] as usize & !1) + ((addr as usize >> 10) & 1),
            0x1000..=0x13FF => self.chr_regs[2] as usize,
            0x1400..=0x17FF => self.chr_regs[3] as usize,
            0x1800..=0x1BFF => self.chr_regs[4] as usize,
            _ => self.chr_regs[5] as usize,
        };
        let bank = bank1k % count1k;
        self.chr_rom[bank * CHR_BANK_1K + (addr as usize & 0x03FF)]
    }
}

impl Mapper for NtdecAsder112 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let last = (self.prg_rom.len() / PRG_BANK_8K).max(1) - 1;
        match addr {
            0x8000..=0x9FFF => self.read_prg(self.prg_banks[0] as usize, addr),
            0xA000..=0xBFFF => self.read_prg(self.prg_banks[1] as usize, addr),
            0xC000..=0xDFFF => self.read_prg(last - 1, addr),
            0xE000..=0xFFFF => self.read_prg(last, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr & 0xE001 {
            0x8000 => self.reg_index = value & 0x07,
            0xA000 => match self.reg_index {
                0 => self.prg_banks[0] = value,
                1 => self.prg_banks[1] = value,
                2 => self.chr_regs[0] = value,
                3 => self.chr_regs[1] = value,
                4 => self.chr_regs[2] = value,
                5 => self.chr_regs[3] = value,
                6 => self.chr_regs[4] = value,
                _ => self.chr_regs[5] = value,
            },
            0xE000 => self.horizontal_mirroring = (value & 0x01) != 0,
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

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(11 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_index);
        out.extend_from_slice(&self.prg_banks);
        out.extend_from_slice(&self.chr_regs);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 11 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.reg_index = data[1];
        self.prg_banks.copy_from_slice(&data[2..4]);
        self.chr_regs.copy_from_slice(&data[4..10]);
        self.horizontal_mirroring = data[10] != 0;
        self.vram.copy_from_slice(&data[11..11 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 137 — Sachen 8259D.
//
// A $4100/$4101 command/data protection-style board (the 8259 family). $4100
// latches a 3-bit command index; $4101 supplies the data for that command:
//   cmd 0..3 : CHR 2 KiB bank selects (slots 0..3 at $0000/$0800/$1000/$1800).
//   cmd 4    : (high CHR bits — modelled as an outer CHR add; we keep it as a
//              stored register that biases all CHR slots).
//   cmd 5    : PRG 32 KiB bank select (low bits).
//   cmd 7    : mirroring / mode (bit 0: 0 = vertical, 1 = horizontal).
// CHR is ROM, four 2 KiB banks. The 8259D variant uses straight 2 KiB CHR
// slots (8259A/B/C reorder the low CHR address lines; that reorder is omitted
// here as it does not affect the register-decode contract).
// ===========================================================================

/// Mapper 137 (Sachen 8259D).
pub struct Sachen8259M137 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    cmd: u8,
    chr_banks: [u8; 4],
    chr_outer: u8,
    prg_bank: u8,
    horizontal_mirroring: bool,
}

impl Sachen8259M137 {
    /// Construct a new mapper 137 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is empty / not a multiple of
    /// 32 KiB or CHR-ROM is empty / not a multiple of 2 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 137 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            return Err(MapperError::Invalid(format!(
                "mapper 137 CHR-ROM size {} is not a non-zero multiple of 2 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            cmd: 0,
            chr_banks: [0; 4],
            chr_outer: 0,
            prg_bank: 0,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let count2k = (self.chr_rom.len() / CHR_BANK_2K).max(1);
        let slot = (addr as usize >> 11) & 0x03;
        let bank = (self.chr_banks[slot] as usize | ((self.chr_outer as usize) << 4)) % count2k;
        self.chr_rom[bank * CHR_BANK_2K + (addr as usize & 0x07FF)]
    }
}

impl Mapper for Sachen8259M137 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
        } else {
            0
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $4100/$4101 are write-only registers; the rest of $4020-$5FFF is open
        // bus. $8000-$FFFF is mapped PRG.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4100 => self.cmd = value & 0x07,
            0x4101 => match self.cmd {
                0..=3 => self.chr_banks[self.cmd as usize] = value & 0x07,
                4 => self.chr_outer = value & 0x07,
                5 => self.prg_bank = value & 0x07,
                7 => self.horizontal_mirroring = (value & 0x01) != 0,
                _ => {}
            },
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

    fn current_mirroring(&self) -> Mirroring {
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(9 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.cmd);
        out.extend_from_slice(&self.chr_banks);
        out.push(self.chr_outer);
        out.push(self.prg_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 9 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.cmd = data[1];
        self.chr_banks.copy_from_slice(&data[2..6]);
        self.chr_outer = data[6];
        self.prg_bank = data[7];
        self.horizontal_mirroring = data[8] != 0;
        self.vram.copy_from_slice(&data[9..9 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 156 — DIS23C01 DAOU (Open Corp / Daou Infosys).
//
// Separate low/high CHR-bank register banks plus a 16 KiB PRG register and an
// explicit one-screen mirroring register, all decoded in the $C000-$C014
// window:
//   $C000-$C003 : CHR low bits for 1 KiB slots 0..3.
//   $C004-$C007 : CHR low bits for 1 KiB slots 4..7.
//   $C008-$C00B : CHR high bits for slots 0..3.
//   $C00C-$C00F : CHR high bits for slots 4..7.
//   $C010       : 16 KiB PRG bank at $8000 ($C000 half fixed to last).
//   $C014       : mirroring (bit 0: 0 = SingleScreenA, 1 = SingleScreenB).
// CHR is ROM (eight 1 KiB slots). No IRQ.
// ===========================================================================

/// Mapper 156 (DIS23C01 DAOU).
pub struct Daou156 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    // 8 low nibbles + 8 high nibbles, composed into a 1 KiB bank per slot.
    chr_lo: [u8; 8],
    chr_hi: [u8; 8],
    mirroring: Mirroring,
}

impl Daou156 {
    /// Construct a new mapper 156 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 1 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 156 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper 156 CHR-ROM size {} is not a non-zero multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_lo: [0; 8],
            chr_hi: [0; 8],
            // DAOU/DIS23C01 powers on single-screen (nametable A) per Mesen2
            // InitMapper; the $C014 register flips it to H/V at runtime.
            mirroring: Mirroring::SingleScreenA,
        })
    }

    fn read_chr(&self, addr: u16) -> u8 {
        let count1k = (self.chr_rom.len() / CHR_BANK_1K).max(1);
        let slot = (addr as usize >> 10) & 0x07;
        let bank = ((self.chr_lo[slot] as usize) | ((self.chr_hi[slot] as usize) << 8)) % count1k;
        self.chr_rom[bank * CHR_BANK_1K + (addr as usize & 0x03FF)]
    }
}

impl Mapper for Daou156 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        match addr {
            0x8000..=0xBFFF => {
                let bank = (self.prg_bank as usize) % count;
                self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            0xC000..=0xFFFF => {
                let last = count - 1;
                self.prg_rom[last * PRG_BANK_16K + (addr as usize & 0x3FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            // $C000-$C00F: 16 CHR-bank-nibble registers. Mesen2 decodes the
            // 1 KiB slot as (addr & 0x03) + (addr >= 0xC008 ? 4 : 0) and selects
            // the low/high nibble array by bit 2 (0x04) — NOT a flat lo[0..8] /
            // hi[0..8] split. The old flat decode wrote the wrong slot's nibble,
            // so CHR banks resolved to garbage → blank/garbled boot.
            0xC000..=0xC00F => {
                let slot = ((addr & 0x03) + if addr >= 0xC008 { 4 } else { 0 }) as usize;
                if addr & 0x04 != 0 {
                    self.chr_hi[slot] = value;
                } else {
                    self.chr_lo[slot] = value;
                }
            }
            0xC010 => self.prg_bank = value,
            // $C014: 0 = vertical, 1 = horizontal (Mesen2). The old code mapped
            // this to a single-screen A/B toggle, which never matched the game's
            // expected nametable layout.
            0xC014 => {
                self.mirroring = if value & 0x01 != 0 {
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

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(19 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.chr_lo);
        out.extend_from_slice(&self.chr_hi);
        out.push(match self.mirroring {
            Mirroring::Horizontal => 0,
            Mirroring::Vertical => 1,
            Mirroring::SingleScreenB => 2,
            _ => 3, // SingleScreenA (power-on default) + any other
        });
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 19 + self.vram.len();
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
        self.chr_lo.copy_from_slice(&data[2..10]);
        self.chr_hi.copy_from_slice(&data[10..18]);
        self.mirroring = match data[18] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenB,
            _ => Mirroring::SingleScreenA,
        };
        self.vram.copy_from_slice(&data[19..19 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 162 — Waixing FS304 (San Guo Zhi II, and similar Waixing RPGs).
//
// Four registers in the $5000-$5FFF window (index = address bits 8-9) compose a
// 32 KiB PRG-ROM bank select from individual A15-A20 bits, with a mode selector
// in $5300 (NESdev INES_Mapper_162):
//   regs[0]=$5000: A18..A17 = bits 3..2; A16 = bit 1 (when $5300.2=1);
//                  A15 = bit 0 (when $5300.2=1 and $5300.0=1).
//   regs[1]=$5100: A15 = bit 1 (when $5300.0=0).
//   regs[2]=$5200: A20..A19 = bits 1..0.
//   regs[3]=$5300: bit 2 = A16 mode, bit 0 = A15 mode.
// Because reset clears all registers, games boot in 32 KiB bank #2 (A16=1,
// A15=0) — the OLD decode booted bank 0 instead, so the reset vector read the
// wrong bank and the game hung/blanked. CHR is 8 KiB RAM, mirroring header-
// fixed. No IRQ.
// ===========================================================================

/// Mapper 162 (Waixing FS304).
pub struct WaixingFs304M162 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    /// 8 KiB battery-backed PRG-RAM at CPU $6000-$7FFF. The Waixing RPGs read
    /// it during boot; without it they hang on a blank frame.
    prg_ram: Box<[u8]>,
    regs: [u8; 4],
    mirroring: Mirroring,
}

impl WaixingFs304M162 {
    /// Construct a new mapper 162 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 162 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; PRG_BANK_8K].into_boxed_slice(),
            regs: [0; 4],
            mirroring,
        })
    }

    const fn prg_bank(&self) -> usize {
        let r0 = self.regs[0] as usize;
        let r1 = self.regs[1] as usize;
        let r2 = self.regs[2] as usize;
        let r3 = self.regs[3] as usize;
        let a = (r3 >> 2) & 1; // $5300.2 — A16 mode
        let b = r3 & 1; // $5300.0 — A15 mode
        let a16 = if a == 0 { 1 } else { (r0 >> 1) & 1 };
        let a15 = if b == 0 {
            (r1 >> 1) & 1
        } else if a == 0 {
            1
        } else {
            r0 & 1
        };
        let a17 = (r0 >> 2) & 1;
        let a18 = (r0 >> 3) & 1;
        let a19 = r2 & 1;
        let a20 = (r2 >> 1) & 1;
        a15 | (a16 << 1) | (a17 << 2) | (a18 << 3) | (a19 << 4) | (a20 << 5)
    }
}

impl Mapper for WaixingFs304M162 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
                let bank = self.prg_bank() % count;
                self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
            }
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $5000-$5FFF carries the write-only register block; the rest of
        // $4020-$5FFF is open bus. $6000-$7FFF is PRG-RAM and $8000-$FFFF is
        // mapped PRG.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_ram[(addr - 0x6000) as usize] = value;
        } else if (0x5000..=0x5FFF).contains(&addr) {
            let idx = ((addr >> 8) & 0x03) as usize;
            self.regs[idx] = value;
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

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(5 + self.vram.len() + self.chr_ram.len() + self.prg_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.regs);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.prg_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len() + self.chr_ram.len() + self.prg_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.regs.copy_from_slice(&data[1..5]);
        let mut cursor = 5;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        cursor += self.chr_ram.len();
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 178 — Waixing / San Guo Zhong Chen (educational series).
//
// A $4800-$4803 register block plus 8 KiB work-RAM at $6000 (NESdev
// INES_Mapper_178 / Waixing FS305):
//   $4800 : bit 0 = mirroring (0 = vertical, 1 = horizontal),
//           bits 1-2 = PRG banking mode
//             0 = NROM-256 / BNROM (32 KiB switchable)
//             1 = UNROM (16 KiB switchable at $8000, fixed-111b at $C000)
//             2 = NROM-128 (16 KiB mirrored)
//             3 = UNROM variant ($C000 = inner|1 instead of all-ones).
//   $4801 : bits 0-2 = inner PRG bank (PRG A16..A14, i.e. 16 KiB units).
//   $4802 : outer PRG bank (PRG A17+).
//   $4803 : PRG-RAM bank (stored only; the staged games use a single 8 KiB).
// 16 KiB bank = (reg2 << 3) | (reg1 & 0x07). The OLD code read bit 0 of $4800
// as the PRG mode (it is the MIRRORING bit) and bit 1 as mirroring (it is a
// PRG-mode bit) — the two were swapped, and the bank composition masked wrong,
// so educational titles booted the wrong bank and blanked. CHR is 8 KiB RAM.
// ===========================================================================

/// Mapper 178 (Waixing educational series).
pub struct Waixing178 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_ram: Box<[u8]>,
    regs: [u8; 4],
}

impl Waixing178 {
    /// Construct a new mapper 178 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 178 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; PRG_BANK_8K].into_boxed_slice(),
            regs: [0; 4],
        })
    }

    /// Composed inner+outer 16 KiB bank: outer ($4802) shifted past the 3-bit
    /// inner ($4801 bits 0-2).
    const fn prg_base16(&self) -> usize {
        ((self.regs[2] as usize) << 3) | (self.regs[1] as usize & 0x07)
    }

    /// PRG banking mode from $4800 bits 1-2 (0..=3).
    const fn prg_mode(&self) -> u8 {
        (self.regs[0] >> 1) & 0x03
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Waixing178 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize],
            0x8000..=0xBFFF => {
                let base = self.prg_base16();
                // $8000: NROM-256/BNROM (mode 0) presents the even bank of the
                // 32 KiB pair; every other mode switches a 16 KiB bank directly.
                let bank = if self.prg_mode() == 0 {
                    base & !1
                } else {
                    base
                };
                self.read_prg(bank, addr)
            }
            0xC000..=0xFFFF => {
                let base = self.prg_base16();
                let bank = match self.prg_mode() {
                    // NROM-256 / BNROM: high half of the 32 KiB pair.
                    0 => (base & !1) | 1,
                    // UNROM: $C000 fixed to the last bank of the outer block
                    // (inner bits = 111b).
                    1 => (base & !0x07) | 0x07,
                    // NROM-128: 16 KiB mirrored.
                    2 => base,
                    // UNROM variant: $C000 = inner | 1.
                    _ => base | 1,
                };
                self.read_prg(bank, addr)
            }
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $4800-$4803 are write-only registers; $6000-$FFFF is mapped
        // (work-RAM + PRG).  The remaining $4020-$5FFF window is open bus.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4800..=0x4803 => self.regs[(addr - 0x4800) as usize] = value,
            0x6000..=0x7FFF => self.prg_ram[(addr - 0x6000) as usize] = value,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[addr as usize] = value,
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        // $4800 bit 0: 0 = vertical, 1 = horizontal.
        if (self.regs[0] & 0x01) != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(5 + self.prg_ram.len() + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.regs);
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.prg_ram.len() + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.regs.copy_from_slice(&data[1..5]);
        let mut cursor = 5;
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        cursor += self.prg_ram.len();
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 244 — Decathlon (Mega Soft).
//
// A $8000-$FFFF data-decoded multicart. The bank select is carried in the
// written DATA byte (not the address) through two scramble LUTs, with bit 3
// selecting CHR vs PRG:
//   value & 0x08 != 0 -> CHR 8 KiB bank = LUT_CHR[(value>>4)&7][value&7].
//   else              -> PRG 32 KiB bank = LUT_PRG[(value>>4)&3][value&3].
// CHR is ROM, mirroring header-fixed. No IRQ.
// ===========================================================================

/// Mapper 244 (Decathlon).
pub struct Decathlon244 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Decathlon244 {
    /// Construct a new mapper 244 board.
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
                "mapper 244 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 244 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }
}

impl Mapper for Decathlon244 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize & 0x7FFF)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        // Mapper 244 decodes the written DATA byte (not the address) through two
        // scramble LUTs, selecting CHR vs PRG by bit 3:
        //   value & 0x08 != 0 -> CHR 8 KiB = LUT_CHR[(value>>4)&7][value&7]
        //   else              -> PRG 32 KiB = LUT_PRG[(value>>4)&3][value&3]
        // The old code ignored the data byte and decoded address bits with no
        // scramble, so it banked to the wrong PRG/CHR and the menu never drew.
        // (Mesen2 Mapper244 / puNES mapper_244 carry the identical tables.)
        const LUT_PRG: [[u8; 4]; 4] = [[0, 1, 2, 3], [3, 2, 1, 0], [0, 2, 1, 3], [3, 1, 2, 0]];
        const LUT_CHR: [[u8; 8]; 8] = [
            [0, 1, 2, 3, 4, 5, 6, 7],
            [0, 2, 1, 3, 4, 6, 5, 7],
            [0, 1, 4, 5, 2, 3, 6, 7],
            [0, 4, 1, 5, 2, 6, 3, 7],
            [0, 4, 2, 6, 1, 5, 3, 7],
            [0, 2, 4, 6, 1, 3, 5, 7],
            [7, 6, 5, 4, 3, 2, 1, 0],
            [7, 6, 5, 4, 3, 2, 1, 0],
        ];
        if (0x8000..=0xFFFF).contains(&addr) {
            if value & 0x08 != 0 {
                self.chr_bank = LUT_CHR[((value >> 4) & 0x07) as usize][(value & 0x07) as usize];
            } else {
                self.prg_bank = LUT_PRG[((value >> 4) & 0x03) as usize][(value & 0x03) as usize];
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
                self.chr_rom[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if (0x2000..=0x3EFF).contains(&addr) {
            let off = nametable_offset(addr, self.mirroring);
            self.vram[off] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.vram.len();
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
        self.chr_bank = data[2];
        self.vram.copy_from_slice(&data[3..3 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 250 — Nitra (Time Diver Avenger).
//
// An MMC3-register-compatible board, but the register index/value normally
// carried in the data byte is instead carried in the *address* bits A0-A7,
// and the data byte is ignored. The effective MMC3 write is:
//   reg select  ($8000-$9FFE, even) : index = A0-A7.
//   reg data    ($8001-$9FFF, odd)  : value = A0-A7.
//   mirroring   ($A000-$BFFE, even) : A0.
// The board provides the MMC3 banking subset (two 8 KiB PRG + the fixed-last
// layout + 2 KiB/1 KiB CHR slots) plus a CPU-cycle (M2) IRQ counter modelled
// like the VRC-style 8-bit reload counter. CHR is ROM.
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

    /// 32 KiB-banked PRG: byte 0 of each 32 KiB bank holds the bank index.
    fn synth_prg_32k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_32K];
        for b in 0..banks {
            v[b * PRG_BANK_32K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 16 KiB-banked PRG: byte 0 of each 16 KiB bank holds the bank index.
    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 8 KiB-banked PRG: byte 0 of each 8 KiB bank holds the bank index.
    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 8 KiB-banked CHR: byte 0 of each 8 KiB bank holds the bank index.
    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 1 KiB-banked CHR: byte 0 of each 1 KiB bank holds the bank index.
    fn synth_chr_1k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_1K];
        for b in 0..banks {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    /// 2 KiB-banked CHR: byte 0 of each 2 KiB bank holds the bank index.
    fn synth_chr_2k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_2K];
        for b in 0..banks {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    // --- Mapper 40 ---------------------------------------------------------

    #[test]
    fn m40_fixed_layout_and_switchable_window() {
        let mut m = Ntdec2722M40::new(synth_prg_8k(8), &[], Mirroring::Vertical).unwrap();
        // Fixed banks.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xE000), 7);
        // Switch $C000 to bank 3.
        m.cpu_write(0xE000, 3);
        assert_eq!(m.cpu_read(0xC000), 3);
    }

    #[test]
    fn m40_irq_fires_after_enable() {
        let mut m = Ntdec2722M40::new(synth_prg_8k(8), &[], Mirroring::Vertical).unwrap();
        assert!(!m.irq_pending());
        m.cpu_write(0xA000, 0); // enable
        for _ in 0..0x1000 {
            m.notify_cpu_cycle();
        }
        assert!(m.irq_pending());
        m.cpu_write(0x8000, 0); // disable + ack
        assert!(!m.irq_pending());
    }

    #[test]
    fn m40_save_state_round_trip() {
        let mut m = Ntdec2722M40::new(synth_prg_8k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0xE000, 2);
        m.cpu_write(0xA000, 0);
        m.notify_cpu_cycle();
        m.ppu_write(0x0005, 0x9A);
        let blob = m.save_state();
        let mut m2 = Ntdec2722M40::new(synth_prg_8k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xC000), 2);
        assert_eq!(m2.ppu_read(0x0005), 0x9A);
    }

    // --- Mapper 81 ---------------------------------------------------------

    #[test]
    fn m81_prg_and_chr_select() {
        let mut m = Ntdec81::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // PRG bits 2-3 = 2, CHR bits 0-1 = 1. value = (2<<2)|1 = 0x09.
        m.cpu_write(0x8000, 0x09);
        assert_eq!(m.cpu_read(0x8000), 2);
        // $C000 fixed to last (7).
        assert_eq!(m.cpu_read(0xC000), 7);
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn m81_save_state_round_trip() {
        let mut m = Ntdec81::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x06);
        let blob = m.save_state();
        let mut m2 = Ntdec81::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
    }

    // --- Mapper 95 ---------------------------------------------------------

    #[test]
    fn m95_prg_select_and_one_screen() {
        let mut m =
            Namcot3425M95::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        // PRG reg 6 -> $8000.
        m.cpu_write(0x8000, 6);
        m.cpu_write(0x8001, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // CHR reg 0, value with bit 5 set -> one-screen B.
        m.cpu_write(0x8000, 0);
        m.cpu_write(0x8001, 0x20);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
        // $C000/$E000 fixed to last two (6,7).
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn m95_save_state_round_trip() {
        let mut m =
            Namcot3425M95::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 7);
        m.cpu_write(0x8001, 4);
        m.cpu_write(0x8000, 0);
        m.cpu_write(0x8001, 0x20);
        let blob = m.save_state();
        let mut m2 =
            Namcot3425M95::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xA000), m.cpu_read(0xA000));
        assert_eq!(m2.current_mirroring(), Mirroring::SingleScreenB);
    }

    // --- Mapper 112 --------------------------------------------------------

    #[test]
    fn m112_indexed_prg_chr_and_mirroring() {
        let mut m =
            NtdecAsder112::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        // Select reg 0 (PRG $8000) -> bank 3.
        m.cpu_write(0x8000, 0);
        m.cpu_write(0xA000, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // Select reg 1 (PRG $A000) -> bank 2.
        m.cpu_write(0x8000, 1);
        m.cpu_write(0xA000, 2);
        assert_eq!(m.cpu_read(0xA000), 2);
        // Mirroring register.
        m.cpu_write(0xE000, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // Fixed last two.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn m112_save_state_round_trip() {
        let mut m =
            NtdecAsder112::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0);
        m.cpu_write(0xA000, 5);
        m.cpu_write(0xE000, 1);
        let blob = m.save_state();
        let mut m2 =
            NtdecAsder112::new(synth_prg_8k(8), synth_chr_1k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
    }

    // --- Mapper 137 --------------------------------------------------------

    #[test]
    fn m137_command_data_chr_and_prg() {
        let mut m =
            Sachen8259M137::new(synth_prg_32k(4), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        // cmd 5 -> PRG 32 KiB bank 2.
        m.cpu_write(0x4100, 5);
        m.cpu_write(0x4101, 2);
        assert_eq!(m.cpu_read(0x8000), 2);
        // cmd 0 -> CHR slot 0 = bank 3.
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x4101, 3);
        assert_eq!(m.ppu_read(0x0000), 3);
        // cmd 7 -> horizontal mirroring.
        m.cpu_write(0x4100, 7);
        m.cpu_write(0x4101, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m137_save_state_round_trip() {
        let mut m =
            Sachen8259M137::new(synth_prg_32k(4), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x4100, 5);
        m.cpu_write(0x4101, 1);
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x4101, 2);
        let blob = m.save_state();
        let mut m2 =
            Sachen8259M137::new(synth_prg_32k(4), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
    }

    // --- Mapper 156 --------------------------------------------------------

    #[test]
    fn m156_chr_compose_prg_and_mirroring() {
        let mut m = Daou156::new(synth_prg_16k(8), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        // Power-on mirroring is single-screen A (Mesen2 InitMapper).
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        // PRG $C010 -> bank 3.
        m.cpu_write(0xC010, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 7); // fixed last
        // CHR slot 0: low = 5 ($C000), high = 0 -> bank 5.
        m.cpu_write(0xC000, 5);
        assert_eq!(m.ppu_read(0x0000), 5);
        // High nibble of slot 0 lives at $C004 (bit 2 selects the high array):
        // low 5 | (high 1 << 8) = 0x105, wraps mod 32 -> 5.
        m.cpu_write(0xC004, 1);
        assert_eq!(m.ppu_read(0x0000), (0x105usize % 32) as u8);
        // Mirroring $C014: 1 = horizontal, 0 = vertical.
        m.cpu_write(0xC014, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0xC014, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m156_save_state_round_trip() {
        let mut m = Daou156::new(synth_prg_16k(8), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        m.cpu_write(0xC010, 2);
        m.cpu_write(0xC001, 4);
        m.cpu_write(0xC014, 1);
        let blob = m.save_state();
        let mut m2 = Daou156::new(synth_prg_16k(8), synth_chr_1k(32), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0400), m.ppu_read(0x0400));
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
    }

    // --- Mapper 162 --------------------------------------------------------

    #[test]
    fn m162_regs_compose_prg() {
        let mut m = WaixingFs304M162::new(synth_prg_32k(64), &[], Mirroring::Vertical).unwrap();
        // Reset: all regs 0 -> A=$5300.2=0 -> A16=1, A15=$5100.1=0 -> bank #2.
        assert_eq!(m.cpu_read(0x8000), 2);
        // Waixing mode $5300=$04 ($5300.2=1): A16=$5000.1, A15=$5100.1.
        m.cpu_write(0x5300, 0x04);
        m.cpu_write(0x5000, 0x02); // $5000.1 = 1 -> A16 = 1 -> bank still has A16 set
        assert_eq!(m.cpu_read(0x8000), 2); // A16=1 -> bank 2
        m.cpu_write(0x5100, 0x02); // $5100.1 = 1 -> A15 = 1 -> bank 3
        assert_eq!(m.cpu_read(0x8000), 3);
        m.cpu_write(0x5000, 0x00); // $5000.1 = 0 -> A16 = 0; A15 still 1 -> bank 1
        assert_eq!(m.cpu_read(0x8000), 1);
        // A17/A18 from $5000 bits 2/3; A19/A20 from $5200 bits 0/1.
        m.cpu_write(0x5000, 0x0C); // bits 3,2 -> A18,A17 = 1,1 -> +12; A16=0,A15(=$5100.1)=1 -> 1
        m.cpu_write(0x5200, 0x03); // A20,A19 = 1,1 -> +48
        assert_eq!(m.cpu_read(0x8000), 1 + 12 + 48);
    }

    #[test]
    fn m162_save_state_round_trip() {
        let mut m = WaixingFs304M162::new(synth_prg_32k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x5000, 6);
        m.ppu_write(0x0012, 0x44);
        let blob = m.save_state();
        let mut m2 = WaixingFs304M162::new(synth_prg_32k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0012), 0x44);
    }

    // --- Mapper 178 --------------------------------------------------------

    #[test]
    fn m178_prg_mode_and_work_ram() {
        let mut m = Waixing178::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // UNROM mode (bits 1-2 = 01 -> $4800 = 0x02); inner reg1 = 3 -> base 3.
        m.cpu_write(0x4800, 0x02);
        m.cpu_write(0x4801, 0x03);
        m.cpu_write(0x4802, 0x00);
        assert_eq!(m.cpu_read(0x8000), 3); // switchable 16 KiB at $8000
        assert_eq!(m.cpu_read(0xC000), 7); // UNROM: $C000 fixed to inner 111b
        // NROM-256 / BNROM (mode 0): 32 KiB pair from the even bank.
        m.cpu_write(0x4800, 0x00);
        m.cpu_write(0x4801, 0x02); // base 2 -> 32 KiB pair (2,3)
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3);
        // Work-RAM round trips.
        m.cpu_write(0x6000, 0x77);
        assert_eq!(m.cpu_read(0x6000), 0x77);
        // Mirroring: $4800 bit 0 (1 = horizontal).
        m.cpu_write(0x4800, 0x01);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x4800, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m178_save_state_round_trip() {
        let mut m = Waixing178::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x4800, 0x02);
        m.cpu_write(0x4801, 0x02);
        m.cpu_write(0x6010, 0x55);
        let blob = m.save_state();
        let mut m2 = Waixing178::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.cpu_read(0x6010), 0x55);
    }

    // --- Mapper 244 --------------------------------------------------------

    #[test]
    fn m244_value_decoded_banks() {
        let mut m =
            Decathlon244::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // PRG select (value & 0x08 == 0): value 0x11 -> LUT_PRG[(1)][1] = 2.
        m.cpu_write(0x8000, 0x11);
        assert_eq!(m.cpu_read(0x8000), 2);
        // value 0x30 -> LUT_PRG[3][0] = 3.
        m.cpu_write(0x8000, 0x30);
        assert_eq!(m.cpu_read(0x8000), 3);
        // CHR select (value & 0x08 != 0): value 0x09 -> LUT_CHR[0][1] = 1.
        m.cpu_write(0x8000, 0x09);
        assert_eq!(m.ppu_read(0x0000), 1);
        // value 0x6E -> LUT_CHR[6][6] = 1 (table row 6 reversed).
        m.cpu_write(0x8000, 0x6E);
        assert_eq!(m.ppu_read(0x0000), 1);
    }

    #[test]
    fn m244_save_state_round_trip() {
        let mut m =
            Decathlon244::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x11); // PRG = LUT_PRG[1][1] = 2
        let blob = m.save_state();
        let mut m2 =
            Decathlon244::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
    }

    // --- Mapper 250 --------------------------------------------------------

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
