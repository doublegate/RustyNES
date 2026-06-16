//! Sprint 7 simple Sachen / multicart / discrete mappers (v1.2.0 "Curator"
//! workstream A, Tier-2 best-effort).
//!
//! A batch of small unlicensed Sachen, Tengen, Nichibutsu and pirate-multicart
//! boards that share the latch-and-bank shape of the stock discrete mappers
//! (`NROM`, `CNROM`, `UxROM`, `GxROM`, `AxROM`). None of these need MMC3-style
//! A12 IRQ counters or on-cart audio. Banking / mirroring semantics are
//! cross-checked against the `GeraNES` reference
//! (`ref-proj/GeraNES/src/GeraNES/Mappers/Mapper0NN.h`) and the nesdev wiki.
//!
//! Because these are Tier-2 best-effort boards (no commercial-oracle ROM in the
//! tree), every mapper here is validated by register-decode unit tests only:
//! synthesize a ROM, write the banking registers, and assert the resolved
//! PRG / CHR offsets and mirroring.
//!
//! Boards implemented here:
//!
//! - **Mapper 147** (Sachen 3018 / TXC `JV001`): simple data-latch model of the
//!   `PCCC CP..` byte, latched on writes to `$4100-$5FFF` and `$8000-$FFFF`
//!   (the `$8000-$FFFF` half carries bus conflicts).
//! - **Mapper 148** (Sachen `SA-008-A` / Tengen 800008): mapper-79 bit layout
//!   moved into `$8000-$FFFF` (bus conflicts).
//! - **Mapper 149** (Sachen `SA-0036`): `CNROM`-like, CHR bit in bit 7
//!   (bus conflicts).
//! - **Mapper 150** (Sachen `SA-015`/`SA-630`, `UNL-Sachen-74LS374N`): eight
//!   readable 3-bit registers via `$4100`/`$4101`, switchable H/V/single-screen/
//!   custom mirroring.
//! - **Mapper 180** (Nichibutsu `UNROM`-inverted, Crazy Climber): switches only
//!   the `$C000` bank; `$8000` is fixed to bank 0 (bus conflicts).
//! - **Mapper 185** (`CNROM` with CHR-disable copy protection).
//! - **Mapper 200** (`MG109` NROM-128 multicart, address latch).
//! - **Mapper 201** (NROM-256 multicart, address-line `BNROM`+`CNROM` overlay).
//! - **Mapper 202** (150-in-1 multicart, address latch with 16/32 KiB PRG mode).
//! - **Mapper 203** (35-in-1 multicart, data latch `PPPPPPCC`).
//! - **Mapper 212** (`BMC` Super `HiK` 300-in-1, address latch with 16/32 KiB PRG).
//! - **Mapper 213** (9999999-in-1 multicart, address latch; duplicate of 58).
//! - **Mapper 214** (Super Gun 20-in-1 multicart, address latch).
//!
//! Mapper 240 is implemented in `sprint5.rs`; it is NOT redone here.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_16K: usize = 0x4000;
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

// ===========================================================================
// Mapper 147 — Sachen 3018 (TXC JV001).
//
// Driven by the TXC JV001 scrambling-accumulator ASIC. Four internal registers
// are written via $4100-$4103 (decoded on `addr & 0x4103`); the scrambled
// output latch updates on any $4100 / $8000-$FFFF write. The boot code performs
// a protection handshake by WRITING a value to $4102/$4100, then READING the
// chip back at $4100 and comparing — so the read MUST return the scrambled
// value, not open bus, or the boot validation loops forever.
//
// JV001 chip read value:  output = (accumulator & 0x3F) | ((inverter ^ inv) & 0xC0)
// Bank decode from the chip output latch (PRG A bits + CHR low bits):
//   PRG (32 KiB) = (output >> 4) & 0x03      (up to 128 KiB)
//   CHR ( 8 KiB) =  output       & 0x0F
// Writes land at $4100-$5FFF (register file) and at $8000-$FFFF (output latch,
// with bus conflict). Mirroring header-fixed; no IRQ.
// ===========================================================================

/// The TXC JV001 scrambling-accumulator chip (mapper 147). Distinct from the
/// non-JV001 `TxcChip` in `sprint6` (different register/output bit positions).
#[derive(Clone, Copy, Default)]
struct Jv001Chip {
    accumulator: u8,
    inverter: u8,
    staging: u8,
    output: u8,
    increase: bool,
    invert: bool,
}

impl Jv001Chip {
    const MASK: u8 = 0x0F;

    /// The value the chip returns on a $4100 read (the protection handshake).
    ///
    /// Per the JV001 hardware spec (and the module comment above), the read
    /// value splits at `0x3F` (6-bit accumulator) / `0xC0` (2-bit inverter) —
    /// distinct from the `0x0F`/`0xF0` nibble split the board's *bank-output*
    /// latch (`self.output`) uses. The handshake read and the bank latch are
    /// separate chip outputs.
    const fn read(self) -> u8 {
        let inv = if self.invert { 0xFF } else { 0x00 };
        (self.accumulator & 0x3F) | ((self.inverter ^ inv) & 0xC0)
    }

    /// `absolute` is the full CPU address; `value` the written byte.
    const fn write(&mut self, absolute: u16, value: u8) {
        if absolute < 0x8000 {
            match absolute & 0x4103 {
                0x4100 => {
                    if self.increase {
                        self.accumulator = self.accumulator.wrapping_add(1);
                    } else {
                        let inv = if self.invert { 0xFF } else { 0x00 };
                        self.accumulator =
                            ((self.accumulator & !Self::MASK) | (self.staging & Self::MASK)) ^ inv;
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
        }
        // The output latch refreshes on every chip access.
        self.output = (self.accumulator & 0x0F) | (self.inverter & 0xF0);
    }
}

/// Mapper 147 (Sachen 3018 / TXC `JV001`).
pub struct Sachen3018M147 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    jv001: Jv001Chip,
    mirroring: Mirroring,
}

impl Sachen3018M147 {
    /// Construct a new mapper 147 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 147 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 147 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            jv001: Jv001Chip::default(),
            mirroring,
        })
    }

    /// PRG 32 KiB bank from the chip output latch.
    const fn prg_bank(&self) -> usize {
        ((self.jv001.output >> 4) & 0x03) as usize
    }

    /// CHR 8 KiB bank from the chip output latch.
    const fn chr_bank(&self) -> usize {
        (self.jv001.output & 0x0F) as usize
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = self.prg_bank() % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = self.chr_bank() % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Sachen3018M147 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // The JV001 protection register answers reads at $4100 (decoded on
        // A0/A1 == 0). Everything else in $4020-$5FFF is open bus.
        !((0x4100..=0x5FFF).contains(&addr) && (addr & 0x0103) == 0x0100)
            && (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            // JV001 protection handshake read.
            0x4100..=0x5FFF if (addr & 0x0103) == 0x0100 => self.jv001.read(),
            0x8000..=0xFFFF => self.read_prg(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4100..=0x5FFF => self.jv001.write(addr, value),
            0x8000..=0xFFFF => {
                // Bus conflict in the PRG window; the write refreshes the latch.
                let effective = value & self.read_prg(addr);
                self.jv001.write(addr, effective);
            }
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
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
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        // 6 JV001 fields (accumulator, inverter, staging, output, increase, invert).
        let mut out = Vec::with_capacity(7 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.jv001.accumulator);
        out.push(self.jv001.inverter);
        out.push(self.jv001.staging);
        out.push(self.jv001.output);
        out.push(u8::from(self.jv001.increase));
        out.push(u8::from(self.jv001.invert));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 7 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.jv001.accumulator = data[1];
        self.jv001.inverter = data[2];
        self.jv001.staging = data[3];
        self.jv001.output = data[4];
        self.jv001.increase = data[5] != 0;
        self.jv001.invert = data[6] != 0;
        let mut cursor = 7;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 148 — Sachen SA-008-A / Tengen 800008.
//
// The mapper-79 bit layout (`.... PCCC`: CHR = bits 0-2, PRG = bit 3) moved
// into the $8000-$FFFF window, introducing bus conflicts:
//   PRG (32 KiB) = (value >> 3) & 0x01
//   CHR (8 KiB)  = value & 0x07
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 148 (Sachen `SA-008-A` / Tengen 800008).
pub struct Sachen148 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen148 {
    /// Construct a new mapper 148 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 148 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 148 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = (self.prg_bank as usize) % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Sachen148 {
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
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict.
            let effective = value & self.read_prg(addr);
            self.prg_bank = (effective >> 3) & 0x01;
            self.chr_bank = effective & 0x07;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
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
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 3 + self.vram.len() + chr_extra;
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
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 149 — Sachen SA-0036.
//
// CNROM-like: fixed 32 KiB PRG, switchable 8 KiB CHR. The CHR bank is a single
// bit in bit 7 of the value written to $8000-$FFFF, with bus conflicts:
//   CHR (8 KiB) = (value >> 7) & 0x01
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 149 (Sachen `SA-0036`).
pub struct Sachen149 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Sachen149 {
    /// Construct a new mapper 149 board.
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
                "mapper 149 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 149 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_bank: 0,
            mirroring,
        })
    }

    fn read_prg(&self, addr: u16) -> u8 {
        // Fixed first 32 KiB bank.
        self.prg_rom[addr as usize - 0x8000]
    }
}

impl Mapper for Sachen149 {
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
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict.
            let effective = value & self.read_prg(addr);
            self.chr_bank = (effective >> 7) & 0x01;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                let bank = (self.chr_bank as usize) % count;
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
        let mut out = Vec::with_capacity(2 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 2 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_bank = data[1];
        self.vram.copy_from_slice(&data[2..2 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 150 — Sachen SA-015 / SA-630 (UNL-Sachen-74LS374N).
//
// An eight-register ASIC at $4100 (register index, write) / $4101
// (register data, read+write). Both decode on the $C101 mask: A8 selects
// index ($4100) vs. data ($4101). Each register holds 3 bits and is fully
// readable (Shogi Gakuen checks this as protection). Banking is derived from
// the registers:
//   PRG (32 KiB) = reg[5] & 0x03
//   CHR (8 KiB)  = ((reg[4] & 0x01) << 2) | (reg[6] & 0x03)
//   mirroring (reg[7] >> 1) & 0x03:
//       0: custom S0-S0-S0-S1 (lower-right unique)
//       1: Horizontal
//       2: Vertical
//       3: Single-screen A
// Reads at $4101 return (open_bus & 0xF8) | (reg[index] & 0x07); we approximate
// open bus with 0 (the protected program only inspects the low 3 bits).
// Writes are also accepted via the $6000-$7FFF mirror (addr | 0x1000). No IRQ.
// ===========================================================================

/// Mapper 150 (Sachen `SA-015`/`SA-630`, `UNL-Sachen-74LS374N`).
pub struct Sachen150 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    current_register: u8,
    reg: [u8; 8],
}

impl Sachen150 {
    /// Construct a new mapper 150 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(prg_rom: Box<[u8]>, chr_rom: Box<[u8]>) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 150 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 150 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            current_register: 0,
            reg: [0u8; 8],
        })
    }

    const fn prg_bank(&self) -> u8 {
        self.reg[5] & 0x03
    }

    const fn chr_bank(&self) -> u8 {
        ((self.reg[4] & 0x01) << 2) | (self.reg[6] & 0x03)
    }

    /// Mirroring selector value `(reg[7] >> 1) & 0x03`.
    const fn mirror_sel(&self) -> u8 {
        (self.reg[7] >> 1) & 0x03
    }

    const fn write_register(&mut self, addr: u16, value: u8) {
        match addr & 0x0101 {
            0x0100 => self.current_register = value & 0x07,
            0x0101 => self.reg[(self.current_register & 0x07) as usize] = value & 0x07,
            _ => {}
        }
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank = (self.prg_bank() as usize) % count;
        self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank() as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Sachen150 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $4100-$5FFF has the readable protection register at $4101 (decoded
        // on A8); $4020-$40FF and $4200+ without A8 are open bus.
        (0x4020..=0x5FFF).contains(&addr) && (addr & 0x0101) != 0x0101
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x4100..=0x5FFF if (addr & 0x0101) == 0x0101 => {
                // Open-bus high 5 bits approximated as 0.
                self.reg[(self.current_register & 0x07) as usize] & 0x07
            }
            0x8000..=0xFFFF => self.read_prg(addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x4100..=0x5FFF => self.write_register(addr, value),
            // $6000-$7FFF mirror: the ASIC sees these as register writes at
            // (addr + 0x1000) per the SaveRAM-mapped register path.
            0x6000..=0x7FFF => self.write_register(addr.wrapping_add(0x1000), value),
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[self.resolve_nametable(addr)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = self.resolve_nametable(addr);
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        match self.mirror_sel() {
            1 => Mirroring::Horizontal,
            2 => Mirroring::Vertical,
            3 => Mirroring::SingleScreenA,
            // 0 = custom S0-S0-S0-S1; report as MapperControlled (the PPU
            // routes through our resolve_nametable for that case).
            _ => Mirroring::MapperControlled,
        }
    }

    #[allow(clippy::cast_possible_truncation)]
    fn nametable_address(&self, addr: u16) -> u16 {
        // CIRAM offset is always < 0x800, so the truncation is a no-op.
        self.resolve_nametable(addr) as u16
    }

    fn save_state(&self) -> Vec<u8> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(2 + 8 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.current_register);
        out.extend_from_slice(&self.reg);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 2 + 8 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.current_register = data[1];
        let mut cursor = 2;
        self.reg.copy_from_slice(&data[cursor..cursor + 8]);
        cursor += 8;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

impl Sachen150 {
    /// Resolve a nametable address to a CIRAM offset (`0..0x800`), applying the
    /// custom S0-S0-S0-S1 layout for mirroring selector 0 and the standard
    /// layouts otherwise.
    const fn resolve_nametable(&self, addr: u16) -> usize {
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        match self.mirror_sel() {
            // Custom S0-S0-S0-S1: tables 0/1/2 -> bank 0, table 3 -> bank 1.
            0 => {
                let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as usize;
                let physical = if table == 3 { 1 } else { 0 };
                physical * NAMETABLE_SIZE + local
            }
            1 => nametable_offset(addr, Mirroring::Horizontal),
            3 => nametable_offset(addr, Mirroring::SingleScreenA),
            // selector 2 (vertical) and any stray value default to vertical.
            _ => nametable_offset(addr, Mirroring::Vertical),
        }
    }
}

// ===========================================================================
// Mapper 180 — Nichibutsu UNROM (inverted), Crazy Climber.
//
// Like UxROM (mapper 2) but using AND logic, so the FIXED bank is at $8000
// (bank 0) and the SWITCHABLE bank is at $C000:
//   CPU $8000-$BFFF: 16 KiB, fixed to bank 0
//   CPU $C000-$FFFF: 16 KiB, selected by (value & 0x07)
// Bus conflicts on the bank-select write. CHR is 8 KiB RAM. Mirroring
// header-fixed; no IRQ.
// ===========================================================================

/// Mapper 180 (Nichibutsu `UNROM`-inverted, Crazy Climber).
pub struct Nichibutsu180 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    mirroring: Mirroring,
}

impl Nichibutsu180 {
    /// Construct a new mapper 180 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 180 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len() == CHR_BANK_8K {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 180 expects 8 KiB CHR (RAM or ROM); got {} bytes",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            mirroring,
        })
    }

    fn read_prg(&self, bank: usize, offset_in_bank: usize) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_16K + offset_in_bank]
    }
}

impl Mapper for Nichibutsu180 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg(0, addr as usize - 0x8000),
            0xC000..=0xFFFF => self.read_prg(self.prg_bank as usize, addr as usize - 0xC000),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict: AND with the byte currently visible at addr.
            let prg_byte = self.cpu_read_at_for_conflict(addr);
            let effective = value & prg_byte;
            self.prg_bank = effective & 0x07;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr[addr as usize],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    self.chr[addr as usize] = value;
                }
            }
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
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(2 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 2 + self.vram.len() + chr_extra;
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
        let mut cursor = 2;
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

impl Nichibutsu180 {
    /// The byte currently visible at `addr` in the $8000-$FFFF window, used for
    /// bus-conflict masking (mirrors the active `cpu_read` banking).
    fn cpu_read_at_for_conflict(&self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg(0, addr as usize - 0x8000),
            _ => self.read_prg(self.prg_bank as usize, addr as usize - 0xC000),
        }
    }
}

// ===========================================================================
// Mapper 185 — CNROM with CHR-disable copy protection.
//
// Stock CNROM banking (8 KiB CHR latch in $8000-$FFFF, bus conflicts), plus a
// copy-protection scheme: certain values written to the CHR register DISABLE
// CHR-ROM, causing reads to return $FF. The submapper selects which 2-bit
// pattern enables CHR; submapper 0 (the common heuristic) enables CHR whenever
// either of the low two bits is set (i.e. value & 0x03 != 0). We model the
// data-driven enable test (the per-read $2007 heuristic of GeraNES is not
// needed for the data-bus protection most mapper-185 ROMs use).
//   CHR (8 KiB) = effective & mask
//   CHR enabled (submapper 0) iff (effective & 0x03) != 0
//   submapper 4/5/6/7 enable iff (effective & 0x03) == 0/1/2/3 respectively
// PRG is fixed (16 or 32 KiB NROM). Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 185 (`CNROM` with CHR-disable copy protection).
pub struct CnRom185 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_reg_raw: u8,
    chr_bank: u8,
    sub_mapper: u8,
    mirroring: Mirroring,
}

impl CnRom185 {
    /// Construct a new mapper 185 board.
    ///
    /// `sub_mapper` selects the CHR-enable pattern (0 = default heuristic,
    /// 4..=7 = exact-match `value & 0x03`).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not 16/32 KiB or CHR-ROM is
    /// empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        sub_mapper: u8,
    ) -> Result<Self, MapperError> {
        if prg_rom.len() != PRG_BANK_16K && prg_rom.len() != PRG_BANK_32K {
            return Err(MapperError::Invalid(format!(
                "mapper 185 expects 16 or 32 KiB PRG, got {} bytes",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 185 expects non-empty CHR-ROM in 8 KiB units, got {} bytes",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_reg_raw: 0,
            chr_bank: 0,
            sub_mapper: sub_mapper & 0x0F,
            mirroring,
        })
    }

    fn read_prg(&self, addr: u16) -> u8 {
        let off = (addr - 0x8000) as usize;
        if self.prg_rom.len() == PRG_BANK_16K {
            self.prg_rom[off & (PRG_BANK_16K - 1)]
        } else {
            self.prg_rom[off]
        }
    }

    // The per-submapper checks compare the low two CHR-register bits against a
    // fixed pattern; `trailing_zeros` would obscure that 2-bit comparison.
    #[allow(clippy::verbose_bit_mask)]
    const fn chr_enabled(&self) -> bool {
        match self.sub_mapper {
            4 => (self.chr_reg_raw & 0x03) == 0,
            5 => (self.chr_reg_raw & 0x03) == 1,
            6 => (self.chr_reg_raw & 0x03) == 2,
            7 => (self.chr_reg_raw & 0x03) == 3,
            // Default heuristic: CHR enabled while either low bit is set.
            _ => (self.chr_reg_raw & 0x03) != 0,
        }
    }
}

impl Mapper for CnRom185 {
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
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict (mapper 185 always has AND-type bus conflicts).
            let effective = value & self.read_prg(addr);
            self.chr_reg_raw = effective;
            let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
            let mask = u8::try_from((count - 1) | 0x03).unwrap_or(u8::MAX);
            self.chr_bank = effective & mask;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_enabled() {
                    let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
                    let bank = (self.chr_bank as usize) % count;
                    self.chr_rom[bank * CHR_BANK_8K + addr as usize]
                } else {
                    // CHR disabled by protection: bus reads $FF.
                    0xFF
                }
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
        let mut out = Vec::with_capacity(4 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.chr_reg_raw);
        out.push(self.chr_bank);
        out.push(self.sub_mapper);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.chr_reg_raw = data[1];
        self.chr_bank = data[2];
        self.sub_mapper = data[3] & 0x0F;
        self.vram.copy_from_slice(&data[4..4 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 200 — MG109 NROM-128 multicart (address latch).
//
// Submapper 0: write $8000-$FFFF, the value is ignored; the ADDRESS bits drive
// the bank/mirroring:
//   A~[1... .... .... bBBB]
//   PRG (16 KiB, mirrored at $8000 and $C000) = addr & 0x07
//   CHR (8 KiB)                               = addr & 0x07
//   mirroring = bit 3 of addr (0: vertical, 1: horizontal)
// CPU $8000-$BFFF mirrors CPU $C000-$FFFF (NROM-128). Header-fixed CHR present;
// CHR-RAM accepted. No IRQ.
// ===========================================================================

/// Mapper 200 (`MG109` NROM-128 multicart).
pub struct Multicart200 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    bank: u8,
    horizontal_mirroring: bool,
}

impl Multicart200 {
    /// Construct a new mapper 200 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 200 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 200 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            bank: 0,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }
}

impl Mapper for Multicart200 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // NROM-128: $8000-$BFFF mirrors $C000-$FFFF.
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.bank as usize) % count;
            let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.bank = (addr & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x08) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let count = (self.chr.len() / CHR_BANK_8K).max(1);
                let bank = (self.bank as usize) % count;
                self.chr[bank * CHR_BANK_8K + addr as usize]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let count = (self.chr.len() / CHR_BANK_8K).max(1);
                    let bank = (self.bank as usize) % count;
                    self.chr[bank * CHR_BANK_8K + addr as usize] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
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
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram { self.chr.len() } else { 0 };
        let expected = 3 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.bank = data[1];
        self.horizontal_mirroring = data[2] != 0;
        let mut cursor = 3;
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
// Mapper 201 — NROM-256 multicart (BNROM + CNROM overlaid, address-driven).
//
// Write $8000-$FFFF: the ADDRESS low byte selects one bank that drives both a
// 32 KiB PRG bank and an 8 KiB CHR bank:
//   PRG (32 KiB) = addr & 0x03   (masked to PRG bank count)
//   CHR (8 KiB)  = addr & 0x07   (masked to CHR bank count)
// (All known games use only the low 2 bits.) Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 201 (NROM-256 multicart).
pub struct Multicart201 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Multicart201 {
    /// Construct a new mapper 201 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 201 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 201 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart201 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = (addr & 0x03) as u8;
            self.chr_bank = (addr & 0x07) as u8;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
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
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 3 + self.vram.len() + chr_extra;
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
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 202 — 150-in-1 multicart (address latch).
//
// Write $8000-$FFFF, ADDRESS-driven (data ignored):
//   A~[.... .... .... O..O]  (PRG mode bits, combine to form a 2-bit value)
//   A~[.... .... .... RRRM]  (R = page register, M = mirroring)
//   prg_mode_is_32k = (((addr >> 1) & 0x01) == 1 && (addr & 0x01) == 1)
//                   i.e. the two "O" bits (addr bit 3 and addr bit 0) == 0b11
//   page = (addr >> 1) & 0x07
//   mirroring = addr & 0x01 (0: vertical, 1: horizontal)
// In 16 KiB mode the page maps both halves; in 32 KiB mode (page>>1) selects a
// 32 KiB bank. CHR (8 KiB) = page. Mirroring runtime; no IRQ.
//
// Per the nesdev wiki the "O" bits are addr bit 3 and addr bit 0; if both set,
// 32 KiB mode. We follow the BizHawk/Disch convention used in the wiki note.
// ===========================================================================

/// Mapper 202 (150-in-1 multicart).
pub struct Multicart202 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    page: u8,
    prg_32k_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart202 {
    /// Construct a new mapper 202 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 202 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 202 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            page: 0,
            prg_32k_mode: false,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.page as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart202 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                if self.prg_32k_mode {
                    // 32 KiB bank (page >> 1) spread across the whole window.
                    let lo16 = ((self.page >> 1) << 1) as usize % count;
                    let off = addr as usize - 0x8000;
                    self.prg_rom[lo16 * PRG_BANK_16K + off]
                } else {
                    // 16 KiB mode: same page mirrored at $8000 and $C000.
                    let bank = (self.page as usize) % count;
                    let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
                    self.prg_rom[bank * PRG_BANK_16K + off]
                }
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // "O" bits = addr bit 3 and addr bit 0; both set => 32 KiB mode.
            self.prg_32k_mode = (addr & 0x09) == 0x09;
            self.page = ((addr >> 1) & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x01) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
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
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(4 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.page);
        out.push(u8::from(self.prg_32k_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 4 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.page = data[1];
        self.prg_32k_mode = data[2] != 0;
        self.horizontal_mirroring = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 203 — 35-in-1 multicart (data latch).
//
// Write $8000-$FFFF, DATA-driven:
//   PPPP PPCC
//   PRG (16 KiB, mirrored at $8000 and $C000) = (data >> 2) & 0x3F
//   CHR (8 KiB)                               = data & 0x03
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 203 (35-in-1 multicart).
pub struct Multicart203 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Multicart203 {
    /// Construct a new mapper 203 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 203 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 203 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart203 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // NROM-128: $8000-$BFFF mirrors $C000-$FFFF.
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_bank as usize) % count;
            let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = (value >> 2) & 0x3F;
            self.chr_bank = value & 0x03;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
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
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 3 + self.vram.len() + chr_extra;
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
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 212 — BMC Super HiK 300-in-1 (address latch).
//
// Write $8000-$FFFF, ADDRESS-driven (data ignored):
//   A~[1o.. .... .... MBBb]
//   prg_32k_mode = bit 14 of addr ("o")
//   page (3-bit)  = addr & 0x07 (drives 16 KiB PRG, 32 KiB PRG and 8 KiB CHR)
//   mirroring = bit 3 of addr (0: vertical, 1: horizontal)
//   16 KiB mode: page maps both $8000 and $C000 windows
//   32 KiB mode: (page >> 1) selects a 32 KiB bank
//   CHR (8 KiB) = page (regardless of "o")
// Reads at $6000-$7FFF with (addr & 0x10) == 0 return bit 7 set (a protection
// signature). Mirroring runtime; no IRQ.
// ===========================================================================

/// Mapper 212 (`BMC` Super `HiK` 300-in-1 multicart).
pub struct Multicart212 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    page: u8,
    prg_32k_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart212 {
    /// Construct a new mapper 212 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 212 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 212 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            page: 0,
            prg_32k_mode: false,
            horizontal_mirroring: mirroring == Mirroring::Horizontal,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.page as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart212 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        // $6000-$7FFF carries the protection signature (mapped). $4020-$5FFF
        // is unmapped open bus, as for stock boards.
        (0x4020..=0x5FFF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                // Protection: (addr & 0x10) == 0 reads $80; else open-bus-ish 0.
                if (addr & 0x0010) == 0 { 0x80 } else { 0x00 }
            }
            0x8000..=0xFFFF => {
                let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
                if self.prg_32k_mode {
                    let lo16 = ((self.page >> 1) << 1) as usize % count;
                    let off = addr as usize - 0x8000;
                    self.prg_rom[lo16 * PRG_BANK_16K + off]
                } else {
                    let bank = (self.page as usize) % count;
                    let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
                    self.prg_rom[bank * PRG_BANK_16K + off]
                }
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_32k_mode = (addr & 0x4000) != 0;
            self.page = (addr & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x0008) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
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
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(4 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.page);
        out.push(u8::from(self.prg_32k_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 4 + self.vram.len() + chr_extra;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.page = data[1];
        self.prg_32k_mode = data[2] != 0;
        self.horizontal_mirroring = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 213 — 9999999-in-1 multicart (address latch; duplicate of 58).
//
// Write $8000-$FFFF, ADDRESS-driven (data ignored):
//   CHR (8 KiB)  = (addr >> 3) & 0x07
//   PRG (32 KiB) = (addr >> 1) & 0x03
// NROM-256-style mirroring (header-fixed). No IRQ.
// ===========================================================================

/// Mapper 213 (9999999-in-1 multicart).
pub struct Multicart213 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Multicart213 {
    /// Construct a new mapper 213 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 213 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 213 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart213 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.chr_bank = ((addr >> 3) & 0x07) as u8;
            self.prg_bank = ((addr >> 1) & 0x03) as u8;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
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
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 3 + self.vram.len() + chr_extra;
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
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

// ===========================================================================
// Mapper 214 — Super Gun 20-in-1 multicart (address latch).
//
// Write $8000-$FFFF, ADDRESS-driven (data ignored):
//   CHR (8 KiB)  = addr & 0x03
//   PRG (16 KiB, mirrored at $8000 and $C000) = (addr >> 2) & 0x03
// Mirroring header-fixed; no IRQ.
// ===========================================================================

/// Mapper 214 (Super Gun 20-in-1 multicart).
pub struct Multicart214 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,
    prg_bank: u8,
    chr_bank: u8,
    mirroring: Mirroring,
}

impl Multicart214 {
    /// Construct a new mapper 214 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB, or CHR-ROM (when present) is not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 214 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr_rom: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "mapper 214 CHR-ROM size {} is not a multiple of 8 KiB",
                chr_rom.len()
            )));
        };
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            chr_is_ram,
            prg_bank: 0,
            chr_bank: 0,
            mirroring,
        })
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_rom.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + addr as usize
    }
}

impl Mapper for Multicart214 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            // NROM-128: $8000-$BFFF mirrors $C000-$FFFF.
            let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
            let bank = (self.prg_bank as usize) % count;
            let off = (addr as usize - 0x8000) & (PRG_BANK_16K - 1);
            self.prg_rom[bank * PRG_BANK_16K + off]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.chr_bank = (addr & 0x03) as u8;
            self.prg_bank = ((addr >> 2) & 0x03) as u8;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    let off = self.chr_offset(addr);
                    self.chr_rom[off] = value;
                }
            }
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
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let mut out = Vec::with_capacity(3 + self.vram.len() + chr_extra);
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr_rom);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_extra = if self.chr_is_ram {
            self.chr_rom.len()
        } else {
            0
        };
        let expected = 3 + self.vram.len() + chr_extra;
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
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        if self.chr_is_ram {
            self.chr_rom
                .copy_from_slice(&data[cursor..cursor + self.chr_rom.len()]);
        }
        Ok(())
    }
}

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    /// 32 KiB-banked PRG: byte 0 of each 32 KiB bank holds the bank index, the
    /// rest is 0xFF (so a bus-conflict AND at offset 0 is observable while
    /// other offsets are transparent).
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

    /// 8 KiB-banked CHR: byte 0 of each 8 KiB bank holds the bank index.
    fn synth_chr_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_8K];
        for b in 0..banks {
            v[b * CHR_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    // --- Mapper 147 --------------------------------------------------------

    #[test]
    fn m147_jv001_protection_read_and_bank_decode() {
        let mut m =
            Sachen3018M147::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // $4102 <- 0x35: staging = 0x05, inverter = 0x30 (invert off).
        m.cpu_write(0x4102, 0x35);
        // $4100 latch (increase off): accumulator = (0 & 0xF0) | (staging & 0x0F)
        //   = 5; output = (acc & 0x0F) | (inverter & 0xF0) = 0x35.
        m.cpu_write(0x4100, 0x00);
        // The protection read at $4100 returns the JV001 scrambled value with
        // the 0x3F/0xC0 split: (accumulator & 0x3F) | ((inverter ^ inv) & 0xC0)
        //   = (0x05 & 0x3F) | (0x30 & 0xC0) = 0x05. (The bank-output latch keeps
        // its own 0x0F/0xF0 split, so the bank-decode asserts below are 0x35.)
        assert_eq!(m.cpu_read(0x4100), 0x05);
        // Bank decode from the output latch: PRG = (0x35 >> 4) & 3 = 3,
        // CHR = 0x35 & 0x0F = 5.
        assert_eq!(m.cpu_read(0x8000), 3); // bank 3 of 4 -> byte 0 = 3
        assert_eq!(m.ppu_read(0x0000), 5); // chr bank 5 of 8 -> byte 0 = 5
    }

    #[test]
    fn m147_prg_window_has_bus_conflict() {
        // A $8000-$FFFF write refreshes the output latch but ANDs with the PRG
        // byte first. PRG byte 0 of bank 0 is 0, so a $8000 write latches 0.
        let mut m =
            Sachen3018M147::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x4102, 0x35);
        m.cpu_write(0x4100, 0x00); // output = 0x35
        m.cpu_write(0x8000, 0xFF); // bus conflict with PRG byte 0 (==0)
        // The $8000 write does not touch the JV001 register file (addr >= 0x8000
        // path only refreshes the latch from the current accumulator/inverter),
        // so the output is unchanged.
        assert_eq!(m.cpu_read(0x8000), 3);
    }

    #[test]
    fn m147_save_state_round_trip() {
        let mut m =
            Sachen3018M147::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x4102, 0x35);
        m.cpu_write(0x4100, 0x00);
        let blob = m.save_state();
        let mut m2 =
            Sachen3018M147::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        // JV001 0x3F/0xC0 handshake read = 0x05 (the bank latch keeps 0x35).
        assert_eq!(m2.cpu_read(0x4100), 0x05);
        assert_eq!(m2.cpu_read(0x8000), 3);
        assert_eq!(m2.ppu_read(0x0000), 5);
    }

    // --- Mapper 148 --------------------------------------------------------

    #[test]
    fn m148_latch_selects_prg_and_chr_with_conflict() {
        // PRG all-0xFF except offset 0, so the in-window write sees 0xFF.
        let mut m =
            Sachen148::new(synth_prg_32k(2), synth_chr_8k(8), Mirroring::Horizontal).unwrap();
        // value .... PCCC: PRG = bit3, CHR = bits 0-2.
        // Write at $8001 (PRG byte 0xFF -> no masking): 0b0000_1101 -> PRG 1, CHR 5.
        m.cpu_write(0x8001, 0b0000_1101);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 5);
    }

    // --- Mapper 149 --------------------------------------------------------

    #[test]
    fn m149_chr_bit_in_bit7() {
        let mut m = Sachen149::new(synth_prg_32k(1), synth_chr_8k(2), Mirroring::Vertical).unwrap();
        // Write at $8001 (PRG byte 0xFF -> no masking): bit7 set -> CHR 1.
        m.cpu_write(0x8001, 0x80);
        assert_eq!(m.ppu_read(0x0000), 1);
        // PRG is fixed bank 0.
        assert_eq!(m.cpu_read(0x8000), 0);
        // bit7 clear -> CHR 0.
        m.cpu_write(0x8001, 0x00);
        assert_eq!(m.ppu_read(0x0000), 0);
    }

    // --- Mapper 150 --------------------------------------------------------

    #[test]
    fn m150_register_protocol_and_banking() {
        let mut m = Sachen150::new(synth_prg_32k(4), synth_chr_8k(8)).unwrap();
        // Select register 5 (PRG), write value 2 -> PRG bank 2.
        m.cpu_write(0x4100, 5); // index
        m.cpu_write(0x4101, 2); // data
        assert_eq!(m.cpu_read(0x8000), 2);
        // Register 6 = CHR low 2 bits; register 4 bit0 = CHR bit2.
        // Set reg6 = 0b01, reg4 = 1 -> CHR = (1<<2)|1 = 5.
        m.cpu_write(0x4100, 6);
        m.cpu_write(0x4101, 0b001);
        m.cpu_write(0x4100, 4);
        m.cpu_write(0x4101, 1);
        assert_eq!(m.ppu_read(0x0000), 5);
        // Registers are readable (protection).
        m.cpu_write(0x4100, 6);
        assert_eq!(m.cpu_read(0x4101), 0b001);
    }

    #[test]
    fn m150_mirroring_modes() {
        let mut m = Sachen150::new(synth_prg_32k(1), synth_chr_8k(1)).unwrap();
        // reg7 mirroring sel = (reg7 >> 1) & 3.
        m.cpu_write(0x4100, 7);
        m.cpu_write(0x4101, 1 << 1); // sel 1 -> horizontal
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0x4101, 2 << 1); // sel 2 -> vertical
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0x4101, 3 << 1); // sel 3 -> single-screen A
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        // sel 0 -> custom S0-S0-S0-S1; table 3 maps to bank 1.
        m.cpu_write(0x4101, 0);
        assert_eq!(m.current_mirroring(), Mirroring::MapperControlled);
        // table 0 ($2000) -> bank 0; table 3 ($2C00) -> bank 1.
        m.ppu_write(0x2000, 0xAA);
        m.ppu_write(0x2C00, 0xBB);
        assert_eq!(m.ppu_read(0x2000), 0xAA);
        assert_eq!(m.ppu_read(0x2C00), 0xBB);
        // table 1 ($2400) shares bank 0 with table 0 in this custom mode.
        assert_eq!(m.ppu_read(0x2400), 0xAA);
    }

    // --- Mapper 180 --------------------------------------------------------

    #[test]
    fn m180_fixes_low_switches_high() {
        let mut m =
            Nichibutsu180::new(synth_prg_16k(8), Box::new([]), Mirroring::Vertical).unwrap();
        // $8000-$BFFF is fixed to bank 0.
        assert_eq!(m.cpu_read(0x8000), 0);
        // Write at $C001 (PRG byte 0xFF -> no masking) selects $C000 bank 3.
        m.cpu_write(0xC001, 3);
        assert_eq!(m.cpu_read(0xC000), 3);
        // $8000 still fixed.
        assert_eq!(m.cpu_read(0x8000), 0);
    }

    #[test]
    fn m180_bus_conflict() {
        // $C000 bank 0 offset 0 holds the bank index (0). Writing 3 there ANDs
        // with 0 -> bank 0.
        let mut m =
            Nichibutsu180::new(synth_prg_16k(8), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0xC000, 3);
        assert_eq!(m.cpu_read(0xC000), 0);
    }

    // --- Mapper 185 --------------------------------------------------------

    #[test]
    fn m185_chr_disable_protection_default() {
        let mut m = CnRom185::new(
            synth_prg(PRG_BANK_32K, 0xFF),
            synth_chr_8k(4),
            Mirroring::Vertical,
            0,
        )
        .unwrap();
        // Default submapper: CHR enabled while (value & 3) != 0.
        // Write 1 -> enabled, bank = 1 & mask.
        m.cpu_write(0x8000, 1);
        assert_eq!(m.ppu_read(0x0000), 1);
        // Write 0 -> CHR disabled -> reads $FF.
        m.cpu_write(0x8000, 0);
        assert_eq!(m.ppu_read(0x0000), 0xFF);
    }

    #[test]
    fn m185_submapper_exact_match() {
        let mut m = CnRom185::new(
            synth_prg(PRG_BANK_32K, 0xFF),
            synth_chr_8k(4),
            Mirroring::Vertical,
            4, // enabled iff (value & 3) == 0
        )
        .unwrap();
        m.cpu_write(0x8000, 0); // (0 & 3) == 0 -> enabled, bank 0
        assert_eq!(m.ppu_read(0x0000), 0);
        m.cpu_write(0x8000, 1); // (1 & 3) == 1 != 0 -> disabled
        assert_eq!(m.ppu_read(0x0000), 0xFF);
    }

    fn synth_prg(bytes: usize, fill: u8) -> Box<[u8]> {
        vec![fill; bytes].into_boxed_slice()
    }

    // --- Mapper 200 --------------------------------------------------------

    #[test]
    fn m200_address_latch() {
        let mut m =
            Multicart200::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // Write address with low bits = 3 and bit3 set (horizontal).
        m.cpu_write(0x8000 | 0x0B, 0x00); // 0x0B = 0b1011: bank 3, H bit set
        assert_eq!(m.cpu_read(0x8000), 3);
        // NROM-128: $8000 mirrors $C000.
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // bit3 clear -> vertical.
        m.cpu_write(0x8000 | 0x02, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        assert_eq!(m.cpu_read(0x8000), 2);
    }

    // --- Mapper 201 --------------------------------------------------------

    #[test]
    fn m201_address_drives_prg_and_chr() {
        let mut m =
            Multicart201::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // addr low 3 bits = 0b011: PRG = 3 & 3 = 3, CHR = 3.
        m.cpu_write(0x8000 | 0x03, 0x00);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.ppu_read(0x0000), 3);
        // addr low 3 bits = 0b101: PRG = 5 & 3 = 1, CHR = 5.
        m.cpu_write(0x8000 | 0x05, 0x00);
        assert_eq!(m.cpu_read(0x8000), 1);
        assert_eq!(m.ppu_read(0x0000), 5);
    }

    // --- Mapper 202 --------------------------------------------------------

    #[test]
    fn m202_16k_mode() {
        let mut m =
            Multicart202::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // page = (addr>>1)&7. Pick page 3 -> addr bits 3..1 = 0b011 -> addr = 0b0110.
        // O bits (bit3 and bit0): bit3 = 0, bit0 = 0 -> not both set -> 16k mode.
        // mirroring = addr bit0 = 0 -> vertical.
        m.cpu_write(0x8000 | 0b0110, 0x00);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3); // mirrored in 16k mode
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m202_32k_mode_and_mirroring() {
        let mut m =
            Multicart202::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // Both O bits set: addr bit3 = 1 and bit0 = 1.
        // page = (addr>>1)&7. addr = 0b1001 | (page<<1). Pick page 2 -> page<<1 = 0b100.
        // addr = 0b1101 = 0x0D: bit3=1, bit0=1 -> 32k mode. page = (0xD>>1)&7 = 6&7 = 6.
        // Recompute to make page even/clear: choose addr = 0x09 (0b1001): page = (9>>1)&7 = 4.
        //   bit3=1, bit0=1 -> 32k. mirroring = bit0 = 1 -> horizontal.
        m.cpu_write(0x8000 | 0x09, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // 32k bank = (page>>1)<<1 = (4>>1)<<1 = 4. Bank 4 at $8000, bank 5 at $C000.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    // --- Mapper 203 --------------------------------------------------------

    #[test]
    fn m203_data_latch() {
        let mut m =
            Multicart203::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // value PPPPPPCC: PRG = value>>2, CHR = value&3.
        // 0b0000_1110 = 0x0E: PRG = 3, CHR = 2.
        m.cpu_write(0x8000, 0x0E);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3); // mirrored
        assert_eq!(m.ppu_read(0x0000), 2);
    }

    // --- Mapper 212 --------------------------------------------------------

    #[test]
    fn m212_16k_mode_and_protection_read() {
        let mut m =
            Multicart212::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // 16k mode (bit14 clear). page = addr & 7 = 3, mirroring bit3 = 1 (H).
        m.cpu_write(0x8000 | 0x0B, 0x00); // 0b1011: page 3, H bit set
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.ppu_read(0x0000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        // Protection read: $6000 (addr&0x10 == 0) -> bit7 set.
        assert_eq!(m.cpu_read(0x6000) & 0x80, 0x80);
        assert_eq!(m.cpu_read(0x6010), 0x00);
    }

    #[test]
    fn m212_32k_mode() {
        let mut m =
            Multicart212::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // bit14 set -> 32k mode. page = addr & 7 = 4. 32k bank = (4>>1)<<1 = 4.
        m.cpu_write(0xC000 | 0x04, 0x00); // 0xC004: bit14 set, page 4
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    // --- Mapper 213 --------------------------------------------------------

    #[test]
    fn m213_address_latch() {
        let mut m =
            Multicart213::new(synth_prg_32k(4), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // CHR = (addr>>3)&7, PRG = (addr>>1)&3.
        // Pick CHR 5, PRG 2: addr bits: (5<<3)|(2<<1) = 0x28 | 0x04 = 0x2C.
        m.cpu_write(0x8000 | 0x2C, 0x00);
        assert_eq!(m.ppu_read(0x0000), 5);
        assert_eq!(m.cpu_read(0x8000), 2);
    }

    // --- Mapper 214 --------------------------------------------------------

    #[test]
    fn m214_address_latch() {
        let mut m =
            Multicart214::new(synth_prg_16k(8), synth_chr_8k(4), Mirroring::Vertical).unwrap();
        // CHR = addr & 3, PRG = (addr>>2)&3.
        // Pick PRG 2, CHR 1: addr bits = (2<<2)|1 = 0x09.
        m.cpu_write(0x8000 | 0x09, 0x00);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 2); // mirrored
        assert_eq!(m.ppu_read(0x0000), 1);
    }
}
