//! Sprint 9 discrete-logic / multicart mappers (v1.4.0 "Fidelity"
//! workstream G mapper-breadth continuation).
//!
//! A best-effort (Tier-2) batch of small, hook-free pirate / homebrew /
//! multicart boards documented concretely on the nesdev wiki (and ported in
//! reference emulators such as `Mesen2` / `GeraNES`). Each has no IRQ, no
//! on-cart audio, and no per-cycle / A12 hook, so every board reports
//! [`MapperCaps::NONE`]. Like `sprint5`/`sprint6`/`sprint7`/`sprint8`, banking
//! math is translated into direct slice indexing; bank selects wrap with
//! `% count`.
//!
//! Boards implemented here:
//!
//! - **Mapper 28** (Action 53 homebrew multicart): an outer `$5xxx` register
//!   plus an inner `$8000-$FFFF` bank latch; a 2-bit mode field picks
//!   NROM-128 / NROM-256 / UNROM-style PRG layout, a 2-bit mirroring field
//!   selects the four mirroring modes, CHR is 8 KiB RAM.
//! - **Mapper 30** (`UNROM-512` homebrew): 16 KiB PRG bank (bits 0-4) + 8 KiB
//!   CHR-RAM bank (bits 5-6) + a one-screen mirroring bit (bit 7) from a single
//!   `$8000-$FFFF` latch with bus conflict; fixed last 16 KiB bank at `$C000`.
//! - **Mapper 63** (NTDEC `0324` "Powerful 250-in-1"): an address-decoded
//!   multicart selecting two 16 KiB PRG banks (or one 32 KiB bank) + a
//!   mirroring bit; CHR-RAM.
//! - **Mapper 76** (`NAMCOT-3446`): four `$8000/$8001` register pairs select two
//!   8 KiB PRG banks (fixed last two) + four 2 KiB CHR banks; software H/V
//!   mirroring is header-fixed.
//! - **Mapper 174** (NTDEC `5-in-1`): an address-decoded register selecting a
//!   16/32 KiB PRG bank + 8 KiB CHR bank + a mirroring bit.
//! - **Mapper 225** (`ColorDreams` `72-in-1`): an address-decoded register
//!   selecting 16/32 KiB PRG + 8 KiB CHR + a mirroring bit, with a separate
//!   four-byte `$5800-$5FFF` scratch-RAM register block (modelled as RAM).
//! - **Mapper 226** (`76-in-1` BMC): two latch registers across `$8000-$FFFF`
//!   selecting a 32 KiB PRG bank + mirroring; CHR-RAM.
//! - **Mapper 227** (`1200-in-1` BMC): an address-decoded register selecting
//!   16/32 KiB PRG + a fixed-bank mode + a mirroring bit; CHR-RAM.
//! - **Mapper 229** (`31-in-1` BMC): an address-decoded multicart — low address
//!   bits 0-4 = 0 means a fixed NROM-32 bank, otherwise a 16 KiB bank pair from
//!   the low 5 bits + an 8 KiB CHR bank + a mirroring bit.
//! - **Mapper 233** (`42-in-1` reset-based BMC): an address-decoded register
//!   selecting 16/32 KiB PRG + a 2-bit mirroring field; the reset-selected
//!   outer block is host-driven and modelled as a fixed power-on `0`.
//! - **Mapper 242** (Waixing `43-in-1` / Wai Xing Zhan Shi): a `$8000-$FFFF`
//!   address-decoded 32 KiB PRG select + a mirroring bit; CHR-RAM.
//! - **Mapper 246** (`Fong Shen Bang` / G0151-1): four `$6000-$6003` banking
//!   registers (two 16 KiB PRG halves + two 4 KiB CHR halves... modelled as
//!   8 KiB PRG quarters + 2 KiB CHR quarters) plus on-cart PRG-RAM at
//!   `$6800-$7FFF`; CHR-ROM, header-fixed mirroring.

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
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
// Mapper 28 — Action 53 homebrew multicart.
//
// A single outer register at $5000-$5FFF selects which inner register a
// $8000-$FFFF write targets (reg index in bits 7-6 of the $5xxx value). The
// four inner registers are:
//   reg 0 ($00): CHR bank (8 KiB CHR-RAM is single-bank, so this only stores).
//   reg 1 ($01): low PRG bank bits.
//   reg 2 ($80): mode/mirroring: bits 0-1 = mirroring, bits 2-3 = PRG mode,
//                bits 4-5 = outer-bank size mask.
//   reg 3 ($81): outer PRG bank.
// We model the documented PRG-banking + mirroring; CHR is 8 KiB RAM. No IRQ.
//
// The resolved PRG layout follows the nesdev "Action 53" decode: the 32 KiB
// CPU window splits into two 16 KiB halves. Mode (bits 2-3 of reg 2) picks:
//   0/1 (NROM-256): both halves track the selected 32 KiB bank.
//   2  (UNROM):     $8000 = selectable 16 KiB, $C000 = fixed last-in-outer.
//   3  (NROM-128):  both halves mirror one 16 KiB bank.
// ===========================================================================

/// Mapper 28 (Action 53 homebrew multicart).
pub struct Action53M28 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    reg_select: u8,
    chr_reg: u8,
    inner_prg: u8,
    mode: u8,
    outer_prg: u8,
}

impl Action53M28 {
    /// Construct a new mapper 28 board.
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
                "mapper 28 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_select: 0,
            chr_reg: 0,
            inner_prg: 0,
            mode: 0,
            outer_prg: 0,
        })
    }

    /// Resolve the 16 KiB PRG bank serving a CPU address in $8000-$FFFF.
    fn prg_bank_for(&self, addr: u16) -> usize {
        let count16 = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        // The outer bank is shifted left by the size mask (bits 4-5 of mode).
        let size = (self.mode >> 4) & 0x03;
        let outer = (self.outer_prg as usize) << (size + 1);
        let prg_mode = (self.mode >> 2) & 0x03;
        let high = addr >= 0xC000;
        let inner = self.inner_prg as usize;
        let bank = match prg_mode {
            // NROM-256: a 32 KiB bank; the high half is +1.
            0 | 1 => (outer & !1) | usize::from(high),
            // UNROM: low half selectable, high half fixed to the outer top.
            2 => {
                if high {
                    outer | 0x01
                } else {
                    (outer & !1) | (inner & 0x01)
                }
            }
            // NROM-128: both halves are the same 16 KiB bank.
            _ => outer | (inner & 0x01),
        };
        bank % count16
    }
}

impl Mapper for Action53M28 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let bank = self.prg_bank_for(addr);
            self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x5000..=0x5FFF => self.reg_select = value & 0x81,
            0x8000..=0xFFFF => match self.reg_select {
                0x00 => self.chr_reg = value,
                0x01 => self.inner_prg = value,
                0x80 => self.mode = value,
                _ => self.outer_prg = value,
            },
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
        match self.mode & 0x03 {
            0 => Mirroring::SingleScreenA,
            1 => Mirroring::SingleScreenB,
            2 => Mirroring::Vertical,
            _ => Mirroring::Horizontal,
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_select);
        out.push(self.chr_reg);
        out.push(self.inner_prg);
        out.push(self.mode);
        out.push(self.outer_prg);
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
        self.reg_select = data[1];
        self.chr_reg = data[2];
        self.inner_prg = data[3];
        self.mode = data[4];
        self.outer_prg = data[5];
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
// Mapper 30 — UNROM-512 homebrew.
//
// A single $8000-$FFFF latch (with bus conflict): the 16 KiB PRG bank at $8000
// is bits 0-4; the 8 KiB CHR-RAM bank is bits 5-6; bit 7 selects one-screen
// mirroring (when the cart is wired for it). $C000 is fixed to the last 16 KiB
// bank. CHR is 32 KiB RAM (four 8 KiB banks). No IRQ.
// ===========================================================================

/// Mapper 30 (`UNROM-512`).
pub struct Unrom512M30 {
    prg_rom: Box<[u8]>,
    /// 32 KiB CHR-RAM (four 8 KiB banks).
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    chr_bank: u8,
    one_screen_b: bool,
    header_mirroring: Mirroring,
}

impl Unrom512M30 {
    /// Construct a new mapper 30 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 30 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; 4 * CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            chr_bank: 0,
            one_screen_b: false,
            header_mirroring: mirroring,
        })
    }

    fn read_prg(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let count = (self.chr_ram.len() / CHR_BANK_8K).max(1);
        let bank = (self.chr_bank as usize) % count;
        bank * CHR_BANK_8K + (addr as usize & 0x1FFF)
    }
}

impl Mapper for Unrom512M30 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => self.read_prg(self.prg_bank as usize, addr),
            0xC000..=0xFFFF => {
                let last = (self.prg_rom.len() / PRG_BANK_16K).max(1) - 1;
                self.read_prg(last, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            // Bus conflict: AND with the byte the CPU would read at this address.
            let effective = value & self.read_prg(self.prg_bank as usize, addr);
            self.prg_bank = effective & 0x1F;
            self.chr_bank = (effective >> 5) & 0x03;
            self.one_screen_b = (effective & 0x80) != 0;
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_ram[self.chr_offset(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let off = self.chr_offset(addr);
                self.chr_ram[off] = value;
            }
            0x2000..=0x3EFF => {
                let off = nametable_offset(addr, self.current_mirroring());
                self.vram[off] = value;
            }
            _ => {}
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        // Four-screen-wired carts use the bit-7 one-screen select; otherwise the
        // header mirroring stands.
        if self.header_mirroring == Mirroring::FourScreen {
            if self.one_screen_b {
                Mirroring::SingleScreenB
            } else {
                Mirroring::SingleScreenA
            }
        } else {
            self.header_mirroring
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(self.chr_bank);
        out.push(u8::from(self.one_screen_b));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.vram.len() + self.chr_ram.len();
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
        self.one_screen_b = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 63 — NTDEC 0324 "Powerful 250-in-1".
//
// Address-decoded register across $8000-$FFFF (data byte ignored). For the
// absolute address A:
//   PRG: bits 1-6 of A select a 16 KiB bank index; bit 0 picks 32 KiB mode
//        (when A&1 == 0, the two 16 KiB halves form a 32 KiB bank).
//   mirroring = bit 0 of (A >> 1)? -> we follow the common decode: A bit 1
//        selects H/V is not used; mapper 63 uses A & 0x06 for the 16K bank and
//        bit 0 for the 32K/16K mode; mirroring follows A bit 0 of the high byte.
// We use the documented decode: bank = (A >> 1) & 0x3F; if (A & 1)==0 -> 32 KiB
// (bank &= !1, high half = bank|1); mirroring = (A & 0x0001_0000)?? — there is
// no separate mirroring line, so the board uses the standard A-bit decode:
// mirroring = if (A & 0x06) == 0x06 horizontal else vertical is NOT it either.
//
// To keep this register-decode honest and simple we implement the widely-cited
// FCEUX decode: PRG 16 KiB bank = (A >> 2) & 0x3F, 32 KiB mode when (A & 2)==0,
// CHR is 8 KiB RAM, mirroring = (A & 1) ? horizontal : vertical.
// ===========================================================================

/// Mapper 63 (NTDEC `0324` multicart).
pub struct Ntdec63 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    prg32_mode: bool,
    horizontal_mirroring: bool,
}

impl Ntdec63 {
    /// Construct a new mapper 63 board.
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
                "mapper 63 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            prg32_mode: true,
            horizontal_mirroring: false,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Ntdec63 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let base = (self.prg_bank as usize) & if self.prg32_mode { !1 } else { !0 };
                self.read_prg(base, addr)
            }
            0xC000..=0xFFFF => {
                let base = self.prg_bank as usize;
                let bank = if self.prg32_mode {
                    (base & !1) | 1
                } else {
                    base
                };
                self.read_prg(bank, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = ((addr >> 2) & 0x3F) as u8;
            self.prg32_mode = (addr & 0x02) == 0;
            self.horizontal_mirroring = (addr & 0x01) != 0;
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
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(4 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.prg32_mode));
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 4 + self.vram.len() + self.chr_ram.len();
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
        self.prg32_mode = data[2] != 0;
        self.horizontal_mirroring = data[3] != 0;
        let mut cursor = 4;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 76 — NAMCOT-3446 (Namco 109 variant, e.g. Digital Devil Story:
// Megami Tensei).
//
// MMC3-like register port at $8000 (index) / $8001 (data), but with only the
// CNROM-style 2 KiB CHR + simple PRG layout:
//   index 2 -> CHR bank 0 (2 KiB at $0000)
//   index 3 -> CHR bank 1 (2 KiB at $0800)
//   index 4 -> CHR bank 2 (2 KiB at $1000)
//   index 5 -> CHR bank 3 (2 KiB at $1800)
//   index 6 -> PRG bank at $8000 (8 KiB)
//   index 7 -> PRG bank at $A000 (8 KiB)
// $C000 and $E000 are fixed to the last two 8 KiB banks. Mirroring is
// header-fixed (the board has no mirroring register). No IRQ.
// ===========================================================================

/// Mapper 76 (`NAMCOT-3446`).
pub struct Namcot3446M76 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    reg_index: u8,
    prg_banks: [u8; 2],
    chr_banks: [u8; 4],
    mirroring: Mirroring,
}

impl Namcot3446M76 {
    /// Construct a new mapper 76 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB or CHR-ROM is empty / not a multiple of 2 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 76 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            return Err(MapperError::Invalid(format!(
                "mapper 76 CHR-ROM size {} is not a non-zero multiple of 2 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg_index: 0,
            prg_banks: [0, 1],
            chr_banks: [0, 1, 2, 3],
            mirroring,
        })
    }

    fn prg_offset(&self, bank: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for Namcot3446M76 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let last = (self.prg_rom.len() / PRG_BANK_8K).max(1) - 1;
        match addr {
            0x8000..=0x9FFF => self.prg_offset(self.prg_banks[0] as usize, addr),
            0xA000..=0xBFFF => self.prg_offset(self.prg_banks[1] as usize, addr),
            // `last - 1` would underflow on a single-8 KiB-bank ROM (`last == 0`);
            // `prg_offset`'s modulo makes both forms identical for multi-bank ROMs.
            0xC000..=0xDFFF => self.prg_offset(last.saturating_sub(1), addr),
            0xE000..=0xFFFF => self.prg_offset(last, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0x9FFF if (addr & 0x01) == 0 => self.reg_index = value & 0x07,
            0x8000..=0x9FFF => match self.reg_index {
                2 => self.chr_banks[0] = value,
                3 => self.chr_banks[1] = value,
                4 => self.chr_banks[2] = value,
                5 => self.chr_banks[3] = value,
                6 => self.prg_banks[0] = value,
                7 => self.prg_banks[1] = value,
                _ => {}
            },
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr >> 11) as usize & 0x03;
                let count = (self.chr_rom.len() / CHR_BANK_2K).max(1);
                let bank = (self.chr_banks[slot] as usize) % count;
                self.chr_rom[bank * CHR_BANK_2K + (addr as usize & 0x07FF)]
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
        let mut out = Vec::with_capacity(1 + 1 + 2 + 4 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg_index);
        out.extend_from_slice(&self.prg_banks);
        out.extend_from_slice(&self.chr_banks);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + 1 + 2 + 4 + self.vram.len();
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
        self.chr_banks.copy_from_slice(&data[4..8]);
        self.vram.copy_from_slice(&data[8..8 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 174 — NTDEC 5-in-1 multicart.
//
// Address-decoded register across $8000-$FFFF. For the absolute address A:
//   PRG: bits 4-7 of A select the bank; bit 7 picks 16 KiB (1) vs 32 KiB (0).
//   We follow the documented decode: bank = (A >> 4) & 0x07; 32 KiB mode when
//   (A & 0x80) == 0; CHR (8 KiB) bank = (A >> 1) & 0x07; mirroring = A & 1.
// CHR is ROM. No IRQ.
// ===========================================================================

/// Mapper 174 (NTDEC `5-in-1`).
pub struct Ntdec174 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    prg16_mode: bool,
    chr_bank: u8,
    horizontal_mirroring: bool,
}

impl Ntdec174 {
    /// Construct a new mapper 174 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 174 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 174 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            prg16_mode: false,
            chr_bank: 0,
            horizontal_mirroring: false,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Ntdec174 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xBFFF => {
                let base = if self.prg16_mode {
                    self.prg_bank as usize
                } else {
                    (self.prg_bank as usize) & !1
                };
                self.read_prg(base, addr)
            }
            0xC000..=0xFFFF => {
                let bank = if self.prg16_mode {
                    self.prg_bank as usize
                } else {
                    ((self.prg_bank as usize) & !1) | 1
                };
                self.read_prg(bank, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.prg_bank = ((addr >> 4) & 0x07) as u8;
            self.prg16_mode = (addr & 0x80) != 0;
            self.chr_bank = ((addr >> 1) & 0x07) as u8;
            self.horizontal_mirroring = (addr & 0x01) != 0;
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
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
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
        let mut out = Vec::with_capacity(5 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.prg16_mode));
        out.push(self.chr_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len();
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
        self.prg16_mode = data[2] != 0;
        self.chr_bank = data[3];
        self.horizontal_mirroring = data[4] != 0;
        self.vram.copy_from_slice(&data[5..5 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 225 — ColorDreams 72-in-1 multicart.
//
// Address-decoded register across $8000-$FFFF. For the absolute address A:
//   mode (bit 12): 0 = 32 KiB PRG, 1 = 16 KiB PRG.
//   high bit (bit 14): outer bank select (combines with the low bits).
//   PRG bank index = ((A >> 14) & 1) << 6 | ((A >> 7) & 0x3F)  (8-bit space)
//   CHR (8 KiB) bank = A & 0x3F (with the high bit folded in).
//   mirroring = (A >> 13) & 1 -> 1 = horizontal, 0 = vertical.
// A separate $5800-$5FFF four-byte scratch register block is modelled as RAM.
// CHR is ROM. No IRQ.
// ===========================================================================

/// Mapper 225 (`ColorDreams` `72-in-1`).
pub struct Multicart225 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    /// $5800-$5FFF 4-nibble scratch RAM (4 bytes, mirrored).
    scratch: [u8; 4],
    prg_bank: u8,
    prg16_mode: bool,
    chr_bank: u8,
    horizontal_mirroring: bool,
}

impl Multicart225 {
    /// Construct a new mapper 225 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 225 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 225 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            scratch: [0; 4],
            prg_bank: 0,
            prg16_mode: false,
            chr_bank: 0,
            horizontal_mirroring: false,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart225 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The scratch RAM answers reads in $5800-$5FFF (mapped). The rest of
    // $4020-$57FF stays open bus (the trait default); $6000-$FFFF PRG is mapped.
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x57FF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x5800..=0x5FFF => self.scratch[(addr & 0x03) as usize] & 0x0F,
            0x8000..=0xBFFF => {
                let base = if self.prg16_mode {
                    self.prg_bank as usize
                } else {
                    (self.prg_bank as usize) & !1
                };
                self.read_prg(base, addr)
            }
            0xC000..=0xFFFF => {
                let bank = if self.prg16_mode {
                    self.prg_bank as usize
                } else {
                    ((self.prg_bank as usize) & !1) | 1
                };
                self.read_prg(bank, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x5800..=0x5FFF => self.scratch[(addr & 0x03) as usize] = value & 0x0F,
            0x8000..=0xFFFF => {
                // nesdev iNES 225: the bank/mode are in the ADDRESS bits
                // A~[.HMO PPPP PPCC CCCC]: CHR = A0..A5 (6 bits), PRG = A6..A11
                // (6 bits), O (PRG mode) = A12 (1 = 16 KiB switchable,
                // 0 = 32 KiB), M (mirroring) = A13 (1 = horizontal), H (outer
                // high bit for both PRG and CHR) = A14.
                let high = ((addr >> 14) & 0x01) as u8;
                self.prg16_mode = (addr & 0x1000) != 0;
                self.prg_bank = (high << 6) | (((addr >> 6) & 0x3F) as u8);
                self.chr_bank = (high << 6) | ((addr & 0x3F) as u8);
                self.horizontal_mirroring = (addr & 0x2000) != 0;
            }
            _ => {}
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
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
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
        let mut out = Vec::with_capacity(4 + 4 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.prg16_mode));
        out.push(self.chr_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.scratch);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + 4 + self.vram.len();
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
        self.prg16_mode = data[2] != 0;
        self.chr_bank = data[3];
        self.horizontal_mirroring = data[4] != 0;
        self.scratch.copy_from_slice(&data[5..9]);
        self.vram.copy_from_slice(&data[9..9 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 226 — 76-in-1 BMC.
//
// Two latch registers across $8000-$FFFF (the low address bit selects reg0 vs
// reg1; the data byte carries the bank bits):
//   reg0 ($8000, even): bits 0-4 = PRG low, bit 5 = PRG high bit, bit 6 =
//        mirroring (1 = horizontal), bit 7 = 32/16 KiB mode.
//   reg1 ($8001, odd): bit 0 = PRG bit 6 (outer block).
// The 32 KiB PRG bank = (reg1.bit0 << 6) | (reg0.bit5 << 5) | (reg0 & 0x1F).
// In 16 KiB mode both halves use the same bank. CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 226 (`76-in-1` BMC).
pub struct Multicart226 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    reg0: u8,
    reg1: u8,
}

impl Multicart226 {
    /// Construct a new mapper 226 board.
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
                "mapper 226 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            reg0: 0,
            reg1: 0,
        })
    }

    /// 7-bit 16 KiB PRG bank index: low 6 bits from reg0, high bit from reg1.
    const fn prg_bank(&self) -> usize {
        let low = (self.reg0 & 0x3F) as usize;
        let high = (self.reg1 & 0x01) as usize;
        (high << 6) | low
    }

    /// PRG mode: reg0 bit 6 set = two 16 KiB banks; clear = one 32 KiB bank.
    const fn is_16k(&self) -> bool {
        (self.reg0 & 0x40) != 0
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart226 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x8000..=0xFFFF).contains(&addr) {
            let bank = self.prg_bank();
            if self.is_16k() {
                // Both 16 KiB halves map the same selected bank.
                self.read_prg(bank, addr)
            } else {
                // 32 KiB mode: the bank index addresses a 32 KiB page (its low
                // bit is ignored); the high half is +1.
                let base = bank & !1;
                let bank16 = base | usize::from(addr >= 0xC000);
                self.read_prg(bank16, addr)
            }
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x8000..=0xFFFF if (addr & 0x01) == 0 => self.reg0 = value,
            0x8000..=0xFFFF => self.reg1 = value,
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
        // reg0 bit 7: 0 = horizontal, 1 = vertical.
        if (self.reg0 & 0x80) != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(3 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.reg0);
        out.push(self.reg1);
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
        self.reg0 = data[1];
        self.reg1 = data[2];
        let mut cursor = 3;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 227 — 1200-in-1 BMC.
//
// Address-decoded register across $8000-$FFFF (Mesen2 Mapper227). For the
// absolute write address A:
//   prg_bank = ((A >> 2) & 0x1F) | ((A & 0x100) >> 3)   (6-bit 16 KiB index)
//   s_flag   = (A & 0x01)        (set: restrict / half-select)
//   prg_mode = (A >> 7) & 0x01   (set: NROM modes; clear: UNROM-like)
//   l_flag   = (A >> 9) & 0x01   (set: fix $C000 to bank|0x07; clear: &0x38)
//   mirroring = (A & 0x02) -> 1 = horizontal, 0 = vertical
// The two $8000/$C000 16 KiB windows are then composed per the Mesen2 mode
// table. The old decode read bit 0 as a 32 KiB mode, mis-applied bit 7, and
// IGNORED bit 9, so the fixed $C000 window pointed at the wrong bank and the
// multicart menu never drew. CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 227 (`1200-in-1` BMC).
#[allow(clippy::struct_excessive_bools)] // 4 independent decoded register flags
pub struct Multicart227 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    prg_bank: u8,
    s_flag: bool,
    l_flag: bool,
    prg_mode: bool,
    horizontal_mirroring: bool,
}

impl Multicart227 {
    /// Construct a new mapper 227 board.
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
                "mapper 227 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_bank: 0,
            s_flag: false,
            l_flag: false,
            prg_mode: false,
            horizontal_mirroring: false,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }

    /// Compose the ($8000, $C000) 16 KiB bank pair from the decoded flags,
    /// matching Mesen2 `Mapper227::WriteRegister`.
    const fn prg_pages(&self) -> (usize, usize) {
        let b = self.prg_bank as usize;
        if self.prg_mode {
            if self.s_flag {
                (b & 0xFE, (b & 0xFE) | 1) // 32 KiB pair
            } else {
                (b, b) // NROM-128 (16 KiB mirrored)
            }
        } else {
            let lo = if self.s_flag { b & 0x3E } else { b };
            let hi = if self.l_flag { b | 0x07 } else { b & 0x38 };
            (lo, hi)
        }
    }
}

impl Mapper for Multicart227 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let (p0, p1) = self.prg_pages();
        match addr {
            0x8000..=0xBFFF => self.read_prg(p0, addr),
            0xC000..=0xFFFF => self.read_prg(p1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            let low = ((addr >> 2) & 0x1F) as u8;
            let high = ((addr & 0x100) >> 3) as u8; // bit 8 -> bit 5 (0x20)
            self.prg_bank = low | high;
            self.s_flag = (addr & 0x01) != 0;
            self.prg_mode = ((addr >> 7) & 0x01) != 0;
            self.l_flag = ((addr >> 9) & 0x01) != 0;
            self.horizontal_mirroring = (addr & 0x02) != 0;
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
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(6 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.s_flag));
        out.push(u8::from(self.l_flag));
        out.push(u8::from(self.prg_mode));
        out.push(u8::from(self.horizontal_mirroring));
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
        self.prg_bank = data[1];
        self.s_flag = data[2] != 0;
        self.l_flag = data[3] != 0;
        self.prg_mode = data[4] != 0;
        self.horizontal_mirroring = data[5] != 0;
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
// Mapper 229 — 31-in-1 BMC.
//
// Address-decoded register across $8000-$FFFF. For the absolute address A:
//   When (A & 0x1E) == 0: a fixed 32 KiB NROM bank 0 (the menu).
//   Otherwise: a 16 KiB PRG bank pair = (A & 0x1F) on both $8000 and $C000?
//   The documented decode: $8000 = (A & 0x1F), $C000 = (A & 0x1F) (16 KiB,
//   same bank), CHR (8 KiB) bank = A & 0x0F, mirroring = (A & 0x20).
// CHR is ROM. No IRQ.
// ===========================================================================

/// Mapper 229 (`31-in-1` BMC).
pub struct Multicart229 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    /// Latched absolute address bits (low 6) used by the decode.
    addr_latch: u8,
    chr_bank: u8,
    horizontal_mirroring: bool,
}

impl Multicart229 {
    /// Construct a new mapper 229 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 16 KiB or CHR-ROM is empty / not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_16K) {
            return Err(MapperError::Invalid(format!(
                "mapper 229 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 229 CHR-ROM size {} is not a non-zero multiple of 8 KiB",
                chr_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            addr_latch: 0,
            chr_bank: 0,
            horizontal_mirroring: false,
        })
    }

    /// True when the latched address selects the fixed 32 KiB NROM menu bank.
    const fn is_menu(&self) -> bool {
        (self.addr_latch & 0x1E) == 0
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        let bank = bank16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart229 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if !(0x8000..=0xFFFF).contains(&addr) {
            return 0;
        }
        if self.is_menu() {
            // Fixed 32 KiB NROM bank 0.
            let bank16 = usize::from(addr >= 0xC000);
            self.read_prg(bank16, addr)
        } else {
            // 16 KiB bank from the latch, mirrored across both halves.
            let bank = (self.addr_latch & 0x1F) as usize;
            self.read_prg(bank, addr)
        }
    }

    fn cpu_write(&mut self, addr: u16, _value: u8) {
        if (0x8000..=0xFFFF).contains(&addr) {
            self.addr_latch = (addr & 0x3F) as u8;
            self.chr_bank = (addr & 0x0F) as u8;
            self.horizontal_mirroring = (addr & 0x20) != 0;
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
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.current_mirroring())],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
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
        let mut out = Vec::with_capacity(4 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.addr_latch);
        out.push(self.chr_bank);
        out.push(u8::from(self.horizontal_mirroring));
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
        self.addr_latch = data[1];
        self.chr_bank = data[2];
        self.horizontal_mirroring = data[3] != 0;
        self.vram.copy_from_slice(&data[4..4 + self.vram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 233 — 42-in-1 reset-based BMC.
//
// Address-decoded register across $8000-$FFFF. For the absolute address A:
//   PRG: bits 0-4 select a 16 KiB bank; bit 5 picks 32/16 KiB mode.
//   mirroring: bits 6-7 -> 0 = one-screen A, 1 = one-screen B, 2 = vertical,
//              3 = horizontal.
// A reset toggles a separate "outer block" line that selects the upper or lower
// half of the ROM; that line is host-driven (the physical reset button), so we
// model it as a fixed power-on `0`. CHR is 8 KiB RAM. No IRQ.
// ===========================================================================

/// Mapper 233 (`42-in-1` reset-based BMC).
pub struct Multicart233 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    /// Reset-selected outer block (host-driven; fixed at power-on).
    outer_block: u8,
    prg_bank: u8,
    mode_32k: bool,
    mirror_mode: u8,
}

impl Multicart233 {
    /// Construct a new mapper 233 board.
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
                "mapper 233 PRG-ROM size {} is not a non-zero multiple of 16 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            outer_block: 0,
            prg_bank: 0,
            mode_32k: false,
            mirror_mode: 0,
        })
    }

    fn read_prg(&self, bank16: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_16K).max(1);
        // The reset-selected outer block adds 0x20 (one half of a 32-bank ROM).
        let bank = (bank16 | ((self.outer_block as usize) << 5)) % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for Multicart233 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        let base = self.prg_bank as usize;
        match addr {
            0x8000..=0xBFFF => {
                let bank = if self.mode_32k { base & !1 } else { base };
                self.read_prg(bank, addr)
            }
            0xC000..=0xFFFF => {
                let bank = if self.mode_32k { (base & !1) | 1 } else { base };
                self.read_prg(bank, addr)
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, _addr: u16, value: u8) {
        // nesdev iNES 233: a $8000-$FFFF write carries the bank in the DATA byte
        // [MMOP PPPP]: PPPP (bits 0-3) = PRG page, O (bit 5) = mode (0 = 16 KiB
        // single bank, 1 = 32 KiB), MM (bits 6-7) = mirroring (00 = 1-screen A,
        // 01 = vertical, 10 = horizontal, 11 = 1-screen B).
        self.prg_bank = value & 0x1F;
        self.mode_32k = (value & 0x20) != 0;
        self.mirror_mode = (value >> 6) & 0x03;
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
        match self.mirror_mode {
            0 => Mirroring::SingleScreenA,
            1 => Mirroring::Vertical,
            2 => Mirroring::Horizontal,
            _ => Mirroring::SingleScreenB,
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(5 + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.outer_block);
        out.push(self.prg_bank);
        out.push(u8::from(self.mode_32k));
        out.push(self.mirror_mode);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 5 + self.vram.len() + self.chr_ram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.outer_block = data[1];
        self.prg_bank = data[2];
        self.mode_32k = data[3] != 0;
        self.mirror_mode = data[4];
        let mut cursor = 5;
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
        cursor += self.vram.len();
        self.chr_ram
            .copy_from_slice(&data[cursor..cursor + self.chr_ram.len()]);
        Ok(())
    }
}

// ===========================================================================
// Mapper 242 — Waixing 43-in-1 / Wai Xing Zhan Shi.
//
// A $8000-$FFFF address-decoded register selects a switchable 32 KiB PRG page
// (the inner bank = address bits 2..4, the outer bank = address bits 5..6) and
// a mirroring bit (address bit 1: 1 = horizontal, 0 = vertical). CHR is 8 KiB
// RAM. The board carries 8 KiB of (battery) work-RAM at $6000-$7FFF — several
// Waixing titles boot by clearing/using that RAM before any PRG bank switch, so
// it must be present and read/write-backed or the reset routine derails.
// No IRQ.
// ===========================================================================

/// Mapper 242 (Waixing `43-in-1`).
pub struct Waixing242 {
    prg_rom: Box<[u8]>,
    chr_ram: Box<[u8]>,
    vram: Box<[u8]>,
    /// 8 KiB work-RAM at $6000-$7FFF.
    prg_ram: Box<[u8]>,
    prg_bank: u8,
    horizontal_mirroring: bool,
}

impl Waixing242 {
    /// Construct a new mapper 242 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 32 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        _chr_rom: &[u8],
        _mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_32K) {
            return Err(MapperError::Invalid(format!(
                "mapper 242 PRG-ROM size {} is not a non-zero multiple of 32 KiB",
                prg_rom.len()
            )));
        }
        Ok(Self {
            prg_rom,
            chr_ram: vec![0u8; CHR_BANK_8K].into_boxed_slice(),
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; PRG_BANK_8K].into_boxed_slice(),
            prg_bank: 0,
            horizontal_mirroring: false,
        })
    }
}

impl Mapper for Waixing242 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // The $6000-$7FFF work-RAM is mapped (the trait default already treats
    // $6000-$FFFF as mapped, so no override is needed).

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_ram[(addr - 0x6000) as usize]
        } else if (0x8000..=0xFFFF).contains(&addr) {
            let count = (self.prg_rom.len() / PRG_BANK_32K).max(1);
            let bank = (self.prg_bank as usize) % count;
            self.prg_rom[bank * PRG_BANK_32K + (addr as usize - 0x8000)]
        } else {
            0
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if (0x6000..=0x7FFF).contains(&addr) {
            self.prg_ram[(addr - 0x6000) as usize] = value;
        } else if (0x8000..=0xFFFF).contains(&addr) {
            // nesdev iNES 242 decode (address-latched): bit 1 (M) = mirroring
            // (0 = vertical, 1 = horizontal); the 32 KiB PRG bank = the inner
            // bank (PRG A16..A14 = address bits 2..4) OR'd with the outer bank
            // (PRG A18..A17 = address bits 5..6) shifted into place. The whole
            // $8000-$FFFF window is one switchable 32 KiB page.
            let inner = ((addr >> 2) & 0x07) as u8;
            let outer = ((addr >> 5) & 0x03) as u8;
            // inner is A16..A14 (a 16 KiB granularity); the 32 KiB bank takes the
            // upper bits: A18..A15 = outer<<2 | inner>>1.
            self.prg_bank = (outer << 2) | (inner >> 1);
            self.horizontal_mirroring = (addr & 0x02) != 0;
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
        if self.horizontal_mirroring {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        }
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out =
            Vec::with_capacity(3 + self.prg_ram.len() + self.vram.len() + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.prg_bank);
        out.push(u8::from(self.horizontal_mirroring));
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        out.extend_from_slice(&self.chr_ram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 3 + self.prg_ram.len() + self.vram.len() + self.chr_ram.len();
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
        self.horizontal_mirroring = data[2] != 0;
        let mut cursor = 3;
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
// Mapper 246 — Fong Shen Bang / G0151-1.
//
// Four banking registers in the $6000-$6003 window (the high half of that
// window, $6800-$7FFF, is on-cart PRG-RAM):
//   $6000: PRG 8 KiB bank at $8000
//   $6001: PRG 8 KiB bank at $A000
//   $6002: PRG 8 KiB bank at $C000
//   $6003: PRG 8 KiB bank at $E000
//   $6004: CHR 2 KiB bank at $0000
//   $6005: CHR 2 KiB bank at $0800
//   $6006: CHR 2 KiB bank at $1000
//   $6007: CHR 2 KiB bank at $1800
// CHR is ROM; mirroring is header-fixed. No IRQ.
// ===========================================================================

/// Mapper 246 (`Fong Shen Bang` / G0151-1).
pub struct FongShenBang246 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    /// 2 KiB battery-backed PRG-RAM at $6800-$6FFF.
    prg_ram: Box<[u8]>,
    prg_banks: [u8; 4],
    chr_banks: [u8; 4],
    mirroring: Mirroring,
}

impl FongShenBang246 {
    /// Construct a new mapper 246 board.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] when PRG is not a non-zero multiple of
    /// 8 KiB or CHR-ROM is empty / not a multiple of 2 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper 246 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if chr_rom.is_empty() || !chr_rom.len().is_multiple_of(CHR_BANK_2K) {
            return Err(MapperError::Invalid(format!(
                "mapper 246 CHR-ROM size {} is not a non-zero multiple of 2 KiB",
                chr_rom.len()
            )));
        }
        // Power-on (per the nesdev wiki): the $6000-$6002 PRG regs are 0, but
        // $6003 (the $E000-$FFFF slot) initializes to 0xFF — so the reset vector
        // at $FFFC resolves into the last PRG bank, where the boot code lives.
        let prg_banks = [0, 0, 0, 0xFF];
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            prg_ram: vec![0u8; 0x0800].into_boxed_slice(),
            prg_banks,
            chr_banks: [0, 0, 0, 0],
            mirroring,
        })
    }

    fn prg_byte(&self, slot: usize, addr: u16) -> u8 {
        let count = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let mut bank = self.prg_banks[slot] as usize;
        // $E000-$FFFF hardware quirk: reads from $FFE4-$FFE7, $FFEC-$FFEF,
        // $FFF4-$FFF7, and $FFFC-$FFFF force PRG A17 high (bank bit 4 of an 8 KiB
        // index). The interrupt/reset vectors live in that forced region.
        if slot == 3 {
            let low = addr & 0x001F;
            let in_window = (0xFFE4..=0xFFFF).contains(&addr)
                && matches!(low, 0x04..=0x07 | 0x0C..=0x0F | 0x14..=0x17 | 0x1C..=0x1F);
            if in_window {
                bank |= 0x10;
            }
        }
        let bank = bank % count;
        self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
    }
}

impl Mapper for FongShenBang246 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    // Only the dead sub-ranges below the PRG window are open bus: $4020-$67FF
    // (the write-only register file at $6000-$67FF + the $4020-$5FFF gap) and
    // the $7000-$7FFF mirror gap. The 2 KiB PRG-RAM at $6800-$6FFF and the PRG
    // ROM at $8000-$FFFF are mapped (matching the trait default of "$6000-$FFFF
    // is mapped" but carving out the register/gap holes).
    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        (0x4020..=0x67FF).contains(&addr) || (0x7000..=0x7FFF).contains(&addr)
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6800..=0x6FFF => self.prg_ram[(addr - 0x6800) as usize],
            0x8000..=0x9FFF => self.prg_byte(0, addr),
            0xA000..=0xBFFF => self.prg_byte(1, addr),
            0xC000..=0xDFFF => self.prg_byte(2, addr),
            0xE000..=0xFFFF => self.prg_byte(3, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x6003 => self.prg_banks[(addr & 0x03) as usize] = value,
            0x6004..=0x6007 => self.chr_banks[(addr & 0x03) as usize] = value,
            0x6800..=0x6FFF => self.prg_ram[(addr - 0x6800) as usize] = value,
            _ => {}
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                let slot = (addr >> 11) as usize & 0x03;
                let count = (self.chr_rom.len() / CHR_BANK_2K).max(1);
                let bank = (self.chr_banks[slot] as usize) % count;
                self.chr_rom[bank * CHR_BANK_2K + (addr as usize & 0x07FF)]
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
        let mut out = Vec::with_capacity(1 + 4 + 4 + self.prg_ram.len() + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_banks);
        out.extend_from_slice(&self.chr_banks);
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 1 + 4 + 4 + self.prg_ram.len() + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.prg_banks.copy_from_slice(&data[1..5]);
        self.chr_banks.copy_from_slice(&data[5..9]);
        let mut cursor = 9;
        self.prg_ram
            .copy_from_slice(&data[cursor..cursor + self.prg_ram.len()]);
        cursor += self.prg_ram.len();
        self.vram
            .copy_from_slice(&data[cursor..cursor + self.vram.len()]);
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

    /// 2 KiB-banked CHR: byte 0 of each 2 KiB bank holds the bank index.
    fn synth_chr_2k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_2K];
        for b in 0..banks {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    // --- Mapper 28 ---------------------------------------------------------

    #[test]
    fn m28_nrom128_mode_mirrors_one_bank() {
        let mut m = Action53M28::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // mode reg: select reg 0x80, write PRG mode 3 (NROM-128), mirroring V (2),
        // size mask 0.
        m.cpu_write(0x5000, 0x80);
        m.cpu_write(0x8000, 0b0000_1110); // mode bits 2-3 = 3, mirroring bits 0-1 = 2
        // inner reg
        m.cpu_write(0x5000, 0x01);
        m.cpu_write(0x8000, 0x01); // inner = 1
        // outer reg
        m.cpu_write(0x5000, 0x81);
        m.cpu_write(0x8000, 0x02); // outer = 2
        // size mask (mode bits 4-5) = 0 -> outer is shifted left by (size+1)=1,
        // so outer = 2<<1 = 4. NROM-128 mode: both halves = outer|(inner&1)
        // = 4|1 = 5.
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 5);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m28_save_state_round_trip() {
        let mut m = Action53M28::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // Set NROM-128 mode (mode bits 2-3 = 3, mirroring bits 0-1 = 2).
        m.cpu_write(0x5000, 0x80);
        m.cpu_write(0x8000, 0x0E);
        // Set outer = 1.
        m.cpu_write(0x5000, 0x81);
        m.cpu_write(0x8000, 0x01);
        m.ppu_write(0x0007, 0x5A);
        let resolved = m.cpu_read(0x8000);
        let blob = m.save_state();
        let mut m2 = Action53M28::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0007), 0x5A);
        assert_eq!(m2.cpu_read(0x8000), resolved);
        assert_eq!(m2.current_mirroring(), Mirroring::Vertical);
    }

    // --- Mapper 30 ---------------------------------------------------------

    #[test]
    fn m30_latch_selects_prg_chr_and_fixed_high() {
        let mut m = Unrom512M30::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        // PRG bits 0-4 = 3, CHR bits 5-6 = 1. value = 0b0010_0011 = 0x23.
        // Offset 1 (no marker, 0xFF) -> bus conflict harmless.
        m.cpu_write(0x8001, 0x23);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $C000 fixed to last (7).
        assert_eq!(m.cpu_read(0xC000), 7);
        // CHR bank 1.
        m.ppu_write(0x0000, 0xEE);
        assert_eq!(m.ppu_read(0x0000), 0xEE);
    }

    #[test]
    fn m30_save_state_round_trip() {
        let mut m = Unrom512M30::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8001, 0x45);
        m.ppu_write(0x0003, 0x77);
        let blob = m.save_state();
        let mut m2 = Unrom512M30::new(synth_prg_16k(8), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0003), 0x77);
    }

    // --- Mapper 63 ---------------------------------------------------------

    #[test]
    fn m63_address_decoded_bank_and_mode() {
        let mut m = Ntdec63::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        // 32 KiB mode: A & 2 == 0. bank = (A>>2)&0x3F. Choose A=0x8008 ->
        // (0x8008>>2)&0x3F = 0x02 -> bank 2; (A&2)==0 -> 32K; (A&1)==0 -> V.
        m.cpu_write(0x8008, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3); // 32K high half = bank|1
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m63_save_state_round_trip() {
        let mut m = Ntdec63::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8009, 0); // A&1 == 1 -> horizontal
        m.ppu_write(0x0010, 0x12);
        let blob = m.save_state();
        let mut m2 = Ntdec63::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.current_mirroring(), Mirroring::Horizontal);
        assert_eq!(m2.ppu_read(0x0010), 0x12);
    }

    // --- Mapper 76 ---------------------------------------------------------

    #[test]
    fn m76_register_pairs_select_banks() {
        let mut m =
            Namcot3446M76::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        // index 6 -> PRG $8000 = bank 3.
        m.cpu_write(0x8000, 6);
        m.cpu_write(0x8001, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // index 2 -> CHR slot 0 = bank 5.
        m.cpu_write(0x8000, 2);
        m.cpu_write(0x8001, 5);
        assert_eq!(m.ppu_read(0x0000), 5);
        // $C000/$E000 fixed to last two banks (6, 7).
        assert_eq!(m.cpu_read(0xC000), 6);
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn m76_save_state_round_trip() {
        let mut m =
            Namcot3446M76::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 7);
        m.cpu_write(0x8001, 4); // PRG $A000 = bank 4
        let blob = m.save_state();
        let mut m2 =
            Namcot3446M76::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xA000), 4);
    }

    // --- Mapper 174 --------------------------------------------------------

    #[test]
    fn m174_address_decoded_prg_chr_mirror() {
        let mut m = Ntdec174::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // A = 0x8020: prg = (0x20>>4)&7 = 2; (A&0x80)==0 -> 32K; chr =
        // (0x20>>1)&7 = 0; (A&1)==0 -> V.
        m.cpu_write(0x8020, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.ppu_read(0x0000), 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
    }

    #[test]
    fn m174_save_state_round_trip() {
        let mut m = Ntdec174::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x80A3, 0); // 16K mode + chr select + horizontal
        let blob = m.save_state();
        let mut m2 = Ntdec174::new(synth_prg_16k(8), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.current_mirroring(), m.current_mirroring());
    }

    // --- Mapper 225 --------------------------------------------------------

    #[test]
    fn m225_address_decoded_and_scratch_ram() {
        let mut m =
            Multicart225::new(synth_prg_16k(16), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        // nesdev decode A~[.HMO PPPP PPCC CCCC]: PRG = A6..A9, O(mode) = A10,
        // M(mirror) = A11, H = A14. A = 0x8080: PRG = (0x80>>6)&0xF = 2; O = 0 ->
        // 32K; M = 0 -> vertical; CHR = A&0x3F = 0.
        m.cpu_write(0x8080, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // Scratch RAM round-trips low nibble.
        m.cpu_write(0x5800, 0xA9);
        assert_eq!(m.cpu_read(0x5800), 0x09);
    }

    #[test]
    fn m225_save_state_round_trip() {
        let mut m =
            Multicart225::new(synth_prg_16k(16), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x9180, 0); // some bank
        m.cpu_write(0x5803, 0x05);
        let blob = m.save_state();
        let mut m2 =
            Multicart225::new(synth_prg_16k(16), synth_chr_8k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.cpu_read(0x5803), 0x05);
    }

    // --- Mapper 226 --------------------------------------------------------

    #[test]
    fn m226_two_regs_select_prg_and_mirror() {
        let mut m = Multicart226::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        // reg0 (even): low bits = 3, bit6 = mirror H. value 0b0100_0011 = 0x43.
        m.cpu_write(0x8000, 0x43);
        // reg1 (odd): bit0 = 0.
        m.cpu_write(0x8001, 0x00);
        // 16K mode (reg0 bit7 = 0): bank 3 on both halves.
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m226_save_state_round_trip() {
        let mut m = Multicart226::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x85); // 32K mode, low = 5
        m.cpu_write(0x8001, 0x00);
        m.ppu_write(0x0001, 0x66);
        let blob = m.save_state();
        let mut m2 = Multicart226::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0001), 0x66);
    }

    // --- Mapper 227 --------------------------------------------------------

    #[test]
    fn m227_address_decoded_bank() {
        let mut m = Multicart227::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        // A = 0x8008: prg_bank = (0x8008>>2)&0x1F = 2; s=(A&1)=0, prg_mode=
        // (A>>7)&1=0, l=(A>>9)&1=0, mirror=(A&2)=0 -> V. UNROM-like, s=0,l=0:
        // $8000 = bank 2, $C000 = bank & 0x38 = 0.
        m.cpu_write(0x8008, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 0);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // l_flag set (A bit 9 = 0x200): $C000 fixed to bank | 0x07.
        // A = 0x8208: prg_bank still 2, l=1 -> $C000 = 2 | 7 = 7.
        m.cpu_write(0x8208, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 7);
        // prg_mode set without s (A bit 7 = 0x80): NROM-128, both halves = bank.
        // A = 0x8088: prg_bank = (0x8088>>2)&0x1F = 2; prg_mode=1, s=0.
        m.cpu_write(0x8088, 0);
        assert_eq!(m.cpu_read(0x8000), 2);
        assert_eq!(m.cpu_read(0xC000), 2);
    }

    #[test]
    fn m227_save_state_round_trip() {
        let mut m = Multicart227::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x808B, 0); // prg_mode + s (32K pair) + A&2 -> H
        m.ppu_write(0x0002, 0x33);
        let blob = m.save_state();
        let mut m2 = Multicart227::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0002), 0x33);
    }

    // --- Mapper 229 --------------------------------------------------------

    #[test]
    fn m229_menu_bank_and_game_bank() {
        let mut m =
            Multicart229::new(synth_prg_16k(16), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // A with low 5 bits zero -> menu (fixed NROM-32 bank 0).
        m.cpu_write(0x8000, 0);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 1);
        // A = 0x8003: latch = 3 (non-menu) -> 16K bank 3 on both halves.
        m.cpu_write(0x8003, 0);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3);
    }

    #[test]
    fn m229_save_state_round_trip() {
        let mut m =
            Multicart229::new(synth_prg_16k(16), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8025, 0); // latch with chr + mirror H
        let blob = m.save_state();
        let mut m2 =
            Multicart229::new(synth_prg_16k(16), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.current_mirroring(), m.current_mirroring());
    }

    // --- Mapper 233 --------------------------------------------------------

    #[test]
    fn m233_bank_and_mirror_modes() {
        let mut m = Multicart233::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        // Data-driven [MMOP PPPP]: value 0x85 = MM=10 (horizontal), O=0 (16K),
        // PPPP=5. 16K mode mirrors the one bank across both halves.
        m.cpu_write(0x8000, 0x85);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xC000), 5);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m233_save_state_round_trip() {
        let mut m = Multicart233::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x26); // O=1 (32K), bank 6
        m.ppu_write(0x0004, 0x88);
        let blob = m.save_state();
        let mut m2 = Multicart233::new(synth_prg_16k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0004), 0x88);
    }

    // --- Mapper 242 --------------------------------------------------------

    #[test]
    fn m242_address_decoded_32k_and_mirror() {
        let mut m = Waixing242::new(synth_prg_32k(16), &[], Mirroring::Vertical).unwrap();
        // A = 0x8018: bank = (0x18>>3)&0x0F = 3; mirror = (A&2)==0 -> V.
        m.cpu_write(0x8018, 0);
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        // A = 0x801A: mirror bit set -> horizontal.
        m.cpu_write(0x801A, 0);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
    }

    #[test]
    fn m242_save_state_round_trip() {
        let mut m = Waixing242::new(synth_prg_32k(16), &[], Mirroring::Vertical).unwrap();
        m.cpu_write(0x8028, 0); // bank 5
        m.ppu_write(0x0006, 0x44);
        let blob = m.save_state();
        let mut m2 = Waixing242::new(synth_prg_32k(16), &[], Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0006), 0x44);
    }

    // --- Mapper 246 --------------------------------------------------------

    #[test]
    fn m246_register_banking_and_prg_ram() {
        let mut m =
            FongShenBang246::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        // $6000 -> PRG $8000 = bank 3.
        m.cpu_write(0x6000, 3);
        assert_eq!(m.cpu_read(0x8000), 3);
        // $6004 -> CHR slot 0 = bank 5.
        m.cpu_write(0x6004, 5);
        assert_eq!(m.ppu_read(0x0000), 5);
        // PRG-RAM round-trips at $6800.
        m.cpu_write(0x6800, 0xC4);
        assert_eq!(m.cpu_read(0x6800), 0xC4);
    }

    #[test]
    fn m246_save_state_round_trip() {
        let mut m =
            FongShenBang246::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6001, 4); // PRG $A000 = bank 4
        m.cpu_write(0x6007, 6); // CHR slot 3 = bank 6
        m.cpu_write(0x6900, 0x9D); // PRG-RAM at $6800-$6FFF
        let blob = m.save_state();
        let mut m2 =
            FongShenBang246::new(synth_prg_8k(8), synth_chr_2k(8), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0xA000), 4);
        assert_eq!(m2.ppu_read(0x1800), 6);
        assert_eq!(m2.cpu_read(0x6900), 0x9D);
    }
}
