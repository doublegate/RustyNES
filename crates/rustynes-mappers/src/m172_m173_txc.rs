// SPDX-License-Identifier: GPL-3.0-or-later
//! Mappers 172 and 173: two copy-protected TXC / Idea-Tek boards built around
//! a latch-adder-inverter ASIC (v3.2.0).
//!
//! Written from the vendored `NESdev` pages `INES_Mapper_172`,
//! `INES_Mapper_173`, and the two pages they link for the chip itself,
//! `INES_Mapper_136` (the JV001 register file mapper 172 shares) and
//! `TXC_05_00002_010_pinout` (the 24-pin chip on mapper 173). No
//! reference-emulator source was read. The project's mapper 132
//! (`m132_txc_22211.rs`) and mapper 136 (`m136_sachen_3011.rs`) model
//! relatives of these chips, but neither is reused: this module states the two
//! register files exactly as their pages give them, so each bit can be checked
//! against one sentence of documentation.
//!
//! ## The protection chip
//!
//! Both chips hold a small **register** that the game can load from an
//! **input** latch (optionally inverted), increment in place, and read back
//! through `$4100`. A write anywhere in `$8000-$FFFF` then copies the register
//! into an **output** latch whose pins drive the CHR-ROM address lines. The
//! value written to `$8000` is ignored. A game proves the chip is present by
//! putting it through a sequence of loads, inversions and increments and then
//! comparing what `$4100` returns; a board that simply latched the `$8000`
//! data would fail that check, which is the copy protection.
//!
//! ### Mapper 172: JV001, mounted upside down (Super Mega P-4070)
//!
//! Five registers: Input (6 bits), Output (6 bits), Register (6 bits), Mode
//! (1 bit) and Invert (1 bit), at mask `$E103`. The board connects CPU D0-D5 to
//! the chip in **reverse order** ("the JV001 ASIC being mounted upside down",
//! `INES_Mapper_136`), so every value crossing the data bus is bit-reversed.
//! This model stores the chip's registers in the chip's own bit order and
//! reverses at the bus:
//!
//! | access | effect (chip order) |
//! |---|---|
//! | read `$4100-$4103` | Register, bits 4-5 inverted when Invert = 1, reversed onto D0-D5; D6-D7 open bus |
//! | write `$4100` | Mode 0: Register := Input, bits 0-3 inverted when Invert = 1. Mode 1: Register bits 0-3 += 1, bits 4-5 kept |
//! | write `$4101` | Invert := CPU D5 |
//! | write `$4102` | Input := CPU D0-D5, reversed |
//! | write `$4103` | Mode := CPU D5 |
//! | write `$8000-$FFFF` | Output := Register; mirroring := Horizontal if Invert = 0, Vertical if Invert = 1 |
//!
//! "Bits 0/1 of the 8 KiB CHR ROM bank number come from Output 5/4, or 0/1
//! after taking the reverse bit order into account", so in chip order the
//! 8 KiB CHR bank is `Output & 3`. PRG is a fixed 32 KiB. Unlike the other
//! JV001 boards, mirroring is software-controlled, so the board reports
//! [`Mapper::has_hardwired_mirroring`] `false`. Until the first `$8000` write
//! the header's mirroring stands: the page says only what a write changes it to.
//!
//! ### Mapper 173: 05-00002-010 (Idea-Tek ET-xx)
//!
//! Six registers: "P", "R" and Output (three bits each); "S", inCrement and
//! inVert (one bit each), at the CPU's own bit order:
//!
//! | access | effect |
//! |---|---|
//! | read `$4100` (mask `$E100`) | `[xxxx SRRR]`: RRR, and S XOR V on D3; D4-D7 open bus |
//! | write `$4100` (mask `$E103`) | C set: RRR += 1. Else RRR := PPP, or ~PPP when V is set. S unchanged |
//! | write `$4101` | V := D0 |
//! | write `$4102` | S := D3, PPP := D0-D2 |
//! | write `$4103` | C := D0 |
//! | write `$8000-$FFFF` (mask `$8000`) | Output := the bottom two bits of RRR |
//!
//! "8 KiB CHR-ROM bank number := (Output &1) | (~Invert <<1)": CHR A13 is
//! Output bit 0 and CHR A14 is the *inverse* of V, read live rather than
//! latched, so a `$4101` write moves CHR A14 immediately. PRG is a fixed 32 KiB
//! (16 KiB on *Xiao Ma Li*, mirrored). Mirroring is hard-wired.
//!
//! *Xiao Ma Li* replaces the 32 KiB CHR-ROM with an 8 KiB EPROM whose pin 27 is
//! `/PGM` instead of A14, and "setting Invert on this board just disables
//! CHR-ROM". The page does not say what the PPU then reads, so that is not
//! modelled: an 8 KiB CHR image simply wraps every bank onto its one bank.
//!
//! The TXC re-release of *Mahjong Block* (headerless CRC32 `0ACFC3CD`) "is
//! commonly set to mapper 173, but since it expects Output bit 1 to select CHR
//! A14, it actually uses INES Mapper 132". It is a mapper 132 board with a
//! mapper 173 header, and is not emulated as 173 here.

#![allow(clippy::cast_possible_truncation, clippy::missing_const_for_fn)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError, mirroring_name};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const SAVE_STATE_VERSION: u8 = 1;

/// Reverse bits 0-5 of `v` (bits 6-7 are dropped): the mapper 172 board wires
/// CPU D0-D5 to the JV001's D5-D0.
const fn reverse6(v: u8) -> u8 {
    let mut out = 0;
    let mut i = 0;
    while i < 6 {
        if v & (1 << i) != 0 {
            out |= 1 << (5 - i);
        }
        i += 1;
    }
    out
}

/// The chip's register window: `$4100-$5FFF` addresses whose `$E100` bits read
/// `$4100`. Both pages decode the read with `$E100` (mapper 172's "Read
/// `$4100-$4103`" under mask `$E103` names all four low addresses, so A0-A1
/// are don't-care there too).
const fn in_read_window(addr: u16) -> bool {
    addr & 0xE100 == 0x4100
}

/// Validate the shared ROM geometry: PRG a non-zero multiple of 16 KiB (one or
/// two 16 KiB halves of the fixed 32 KiB window) and non-empty CHR-ROM in
/// 8 KiB banks.
fn check_geometry(id: u16, prg: &[u8], chr: &[u8]) -> Result<(), MapperError> {
    if prg.is_empty() || !prg.len().is_multiple_of(0x4000) {
        return Err(MapperError::Invalid(format!(
            "mapper {id} PRG-ROM size {} is not a non-zero multiple of 16 KiB",
            prg.len()
        )));
    }
    if chr.is_empty() || !chr.len().is_multiple_of(CHR_BANK_8K) {
        return Err(MapperError::Invalid(format!(
            "mapper {id} CHR-ROM size {} is not a non-zero multiple of 8 KiB",
            chr.len()
        )));
    }
    Ok(())
}

/// The fixed 32 KiB PRG window. A larger image keeps its first 32 KiB (neither
/// board has a PRG bank line); a 16 KiB image is mirrored.
fn prg_byte(prg: &[u8], addr: u16) -> u8 {
    let off = (usize::from(addr) - 0x8000) % PRG_BANK_32K.min(prg.len());
    prg[off]
}

fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / 0x0400) & 0x03) as u8;
    mirroring.physical_bank(table) * NAMETABLE_SIZE + (usize::from(addr) & (NAMETABLE_SIZE - 1))
}

// ---------------------------------------------------------------------------
// Mapper 172
// ---------------------------------------------------------------------------

/// Mapper 172: the Super Mega P-4070 board (*1991 Du Ma Racing*, the original
/// *Mahjong Block*, *Venice Beach Volley*). See the module docs.
pub struct Txc172 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    /// The six-bit Input latch, chip bit order.
    input: u8,
    /// The six-bit Register, chip bit order.
    register: u8,
    /// The six-bit Output latch, chip bit order.
    output: u8,
    /// Mode: `true` makes a `$4100` write increment instead of load.
    increment: bool,
    /// Invert.
    invert: bool,
    /// The mirroring the last `$8000-$FFFF` write chose (the header's until then).
    mirroring: Mirroring,
}

impl Txc172 {
    /// Construct the board.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 16 KiB
    /// or CHR-ROM is empty or not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_geometry(172, &prg_rom, &chr_rom)?;
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            input: 0,
            register: 0,
            output: 0,
            increment: false,
            invert: false,
            mirroring,
        })
    }

    /// The value the chip drives for a `$4100` read, in chip bit order:
    /// Register with bits 4-5 inverted while Invert is set.
    fn chip_read(&self) -> u8 {
        self.register ^ if self.invert { 0x30 } else { 0 }
    }

    fn chr_bank(&self) -> usize {
        usize::from(self.output & 0x03) % (self.chr_rom.len() / CHR_BANK_8K)
    }
}

impl Mapper for Txc172 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        matches!(addr, 0x4020..=0x7FFF) && !in_read_window(addr)
    }

    /// D6-D7 of a `$4100` read are open bus.
    fn cpu_read_driven_mask(&self, addr: u16) -> u8 {
        if in_read_window(addr) { 0x3F } else { 0xFF }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => prg_byte(&self.prg_rom, addr),
            _ if in_read_window(addr) => reverse6(self.chip_read()),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            // The written value is ignored.
            self.output = self.register;
            self.mirroring = if self.invert {
                Mirroring::Vertical
            } else {
                Mirroring::Horizontal
            };
            return;
        }
        match addr & 0xE103 {
            0x4100 => {
                self.register = if self.increment {
                    (self.register & 0x30) | (self.register.wrapping_add(1) & 0x0F)
                } else {
                    self.input ^ if self.invert { 0x0F } else { 0 }
                };
            }
            // CPU D5 is chip D0 on this board.
            0x4101 => self.invert = value & 0x20 != 0,
            0x4102 => self.input = reverse6(value),
            0x4103 => self.increment = value & 0x20 != 0,
            _ => {}
        }
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        u32::try_from(self.chr_bank() * CHR_BANK_8K + usize::from(addr & 0x1FFF)).ok()
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_bank() * CHR_BANK_8K + usize::from(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_peek(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_bank() * CHR_BANK_8K + usize::from(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            self.vram[nametable_offset(addr, self.mirroring)] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = MapperDebugInfo {
            mapper_id: 172,
            name: "TXC JV001 reversed (172)".into(),
            mirroring: mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.chr_banks
            .push(("8K".into(), format!("{:#04x}", self.chr_bank())));
        info.extra.push((
            "register".into(),
            format!("{:#04x} (chip order)", self.register),
        ));
        info.extra
            .push(("input".into(), format!("{:#04x}", self.input)));
        info.extra
            .push(("output".into(), format!("{:#04x}", self.output)));
        info.extra.push((
            "mode/invert".into(),
            format!("{}/{}", u8::from(self.increment), u8::from(self.invert)),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(7 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.input);
        out.push(self.register);
        out.push(self.output);
        out.push(u8::from(self.increment));
        out.push(u8::from(self.invert));
        out.push(u8::from(self.mirroring == Mirroring::Vertical) | 0x80);
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 7 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::WrongLength {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.input = data[1] & 0x3F;
        self.register = data[2] & 0x3F;
        self.output = data[3] & 0x3F;
        self.increment = data[4] != 0;
        self.invert = data[5] != 0;
        // Bit 7 marks a saved H/V choice; the header's mirroring is the only
        // other value the board can hold, and it is H or V on every dump, so
        // the bit-0 encoding is complete.
        self.mirroring = if data[6] & 0x01 != 0 {
            Mirroring::Vertical
        } else {
            Mirroring::Horizontal
        };
        self.vram.copy_from_slice(&data[7..]);
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Mapper 173
// ---------------------------------------------------------------------------

/// Mapper 173: Idea-Tek's unmarked ET-xx board (*Puzzle*, *Xiao Ma Li*,
/// *F-15 City War*, *Poker Jingling*, *Zhanguo Sichuan Sheng*). See the module
/// docs.
pub struct Txc173 {
    prg_rom: Box<[u8]>,
    chr_rom: Box<[u8]>,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    /// "PPP": three bits loaded from `$4102`.
    p: u8,
    /// "RRR": three bits, loaded or incremented by a `$4100` write.
    r: u8,
    /// "S": one bit from `$4102` D3, read back immediately (XOR V).
    s: bool,
    /// Output: the bottom two bits of RRR at the last `$8000-$FFFF` write.
    output: u8,
    /// "C", increment mode.
    increment: bool,
    /// "V", invert mode; its inverse is CHR A14.
    invert: bool,
}

impl Txc173 {
    /// Construct the board. Mirroring is hard-wired, so the header's stands.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 16 KiB
    /// or CHR-ROM is empty or not a multiple of 8 KiB.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
    ) -> Result<Self, MapperError> {
        check_geometry(173, &prg_rom, &chr_rom)?;
        Ok(Self {
            prg_rom,
            chr_rom,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            p: 0,
            r: 0,
            s: false,
            output: 0,
            increment: false,
            invert: false,
        })
    }

    /// `[xxxx SRRR]`.
    fn chip_read(&self) -> u8 {
        (self.r & 0x07) | (u8::from(self.s ^ self.invert) << 3)
    }

    /// `(Output & 1) | (~Invert << 1)`.
    fn chr_bank(&self) -> usize {
        let bank = usize::from(self.output & 0x01) | (usize::from(!self.invert) << 1);
        bank % (self.chr_rom.len() / CHR_BANK_8K)
    }
}

impl Mapper for Txc173 {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn has_hardwired_mirroring(&self) -> bool {
        true
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        matches!(addr, 0x4020..=0x7FFF) && !in_read_window(addr)
    }

    /// D4-D7 of a `$4100` read are open bus.
    fn cpu_read_driven_mask(&self, addr: u16) -> u8 {
        if in_read_window(addr) { 0x0F } else { 0xFF }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => prg_byte(&self.prg_rom, addr),
            _ if in_read_window(addr) => self.chip_read(),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr >= 0x8000 {
            self.output = self.r & 0x03;
            return;
        }
        match addr & 0xE103 {
            0x4100 => {
                self.r = if self.increment {
                    self.r.wrapping_add(1) & 0x07
                } else if self.invert {
                    !self.p & 0x07
                } else {
                    self.p
                };
            }
            0x4101 => self.invert = value & 0x01 != 0,
            0x4102 => {
                self.s = value & 0x08 != 0;
                self.p = value & 0x07;
            }
            0x4103 => self.increment = value & 0x01 != 0,
            _ => {}
        }
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        u32::try_from(self.chr_bank() * CHR_BANK_8K + usize::from(addr & 0x1FFF)).ok()
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_bank() * CHR_BANK_8K + usize::from(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_peek(&self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => self.chr_rom[self.chr_bank() * CHR_BANK_8K + usize::from(addr)],
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if let 0x2000..=0x3EFF = addr {
            self.vram[nametable_offset(addr, self.mirroring)] = value;
        }
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = MapperDebugInfo {
            mapper_id: 173,
            name: "TXC 05-00002-010 (173)".into(),
            mirroring: mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.chr_banks
            .push(("8K".into(), format!("{:#04x}", self.chr_bank())));
        info.extra.push((
            "P/R/S".into(),
            format!("{}/{}/{}", self.p, self.r, u8::from(self.s)),
        ));
        info.extra.push((
            "C/V".into(),
            format!("{}/{}", u8::from(self.increment), u8::from(self.invert)),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(7 + self.vram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.p);
        out.push(self.r);
        out.push(u8::from(self.s));
        out.push(self.output);
        out.push(u8::from(self.increment));
        out.push(u8::from(self.invert));
        out.extend_from_slice(&self.vram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let expected = 7 + self.vram.len();
        if data.len() != expected {
            return Err(MapperError::WrongLength {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        self.p = data[1] & 0x07;
        self.r = data[2] & 0x07;
        self.s = data[3] != 0;
        self.output = data[4] & 0x03;
        self.increment = data[5] != 0;
        self.invert = data[6] != 0;
        self.vram.copy_from_slice(&data[7..]);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::parse;

    /// An iNES 1.0 image for `mapper`: `prg_16k` x 16 KiB PRG whose every
    /// 16 KiB half opens with its index, and `chr_8k` x 8 KiB CHR whose every
    /// bank opens with its index. Vertical mirroring in the header.
    fn image(mapper: u8, prg_16k: usize, chr_8k: usize) -> Vec<u8> {
        let mut v = vec![0u8; 16];
        v[0..4].copy_from_slice(b"NES\x1A");
        v[4] = prg_16k as u8;
        v[5] = chr_8k as u8;
        v[6] = ((mapper & 0x0F) << 4) | 0x01;
        v[7] = mapper & 0xF0;
        for b in 0..prg_16k {
            let mut bank = vec![0u8; 0x4000];
            bank[0] = b as u8;
            v.extend_from_slice(&bank);
        }
        for b in 0..chr_8k {
            let mut bank = vec![0u8; CHR_BANK_8K];
            bank[0] = b as u8;
            v.extend_from_slice(&bank);
        }
        v
    }

    /// Build through `crate::parse`, so the dispatch arm is under test too.
    fn board(mapper: u8, prg_16k: usize, chr_8k: usize) -> Box<dyn Mapper> {
        parse(&image(mapper, prg_16k, chr_8k))
            .unwrap_or_else(|e| panic!("mapper {mapper} must parse: {e:?}"))
            .1
    }

    #[test]
    fn reverse6_reverses_d0_to_d5() {
        assert_eq!(reverse6(0b00_0001), 0b10_0000);
        assert_eq!(reverse6(0b00_0011), 0b11_0000);
        assert_eq!(reverse6(0b10_1100), 0b00_1101);
        assert_eq!(reverse6(0xC0), 0, "D6-D7 are not connected");
    }

    // --- 172 ---------------------------------------------------------------

    #[test]
    fn m172_parses_with_fixed_32k_prg_and_header_mirroring_until_8000() {
        let mut m = board(172, 2, 4);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 1, "fixed 32 KiB, no PRG banking");
        assert_eq!(m.current_mirroring(), Mirroring::Vertical, "header");
        assert!(!m.has_hardwired_mirroring(), "software-controlled");
    }

    #[test]
    fn m172_load_and_read_back_are_bit_reversed() {
        let mut m = board(172, 2, 4);
        // CPU $01 is chip bit 5; CPU $08 is chip bit 2.
        m.cpu_write(0x4102, 0x09);
        m.cpu_write(0x4100, 0); // Mode 0, Invert 0: Register := Input.
        assert_eq!(m.cpu_read(0x4100), 0x09, "reversed in, reversed out");
        assert_eq!(m.cpu_read(0x4103), 0x09, "all of $4100-$4103 read");
        assert_eq!(m.cpu_read(0x5F01), 0x09, "mask $E100");
        assert_eq!(m.cpu_read_driven_mask(0x4100), 0x3F, "D6-D7 open bus");
        assert!(!m.cpu_read_unmapped(0x4100));
        assert!(m.cpu_read_unmapped(0x4000 + 0x20), "outside the mask");
        assert!(m.cpu_read_unmapped(0x4200));
        assert!(m.cpu_read_unmapped(0x6100), "A13 set is not the chip");
    }

    #[test]
    fn m172_invert_flips_loaded_bits_0_3_and_read_bits_4_5() {
        let mut m = board(172, 2, 4);
        m.cpu_write(0x4101, 0x20); // CPU D5 = chip D0: Invert := 1.
        m.cpu_write(0x4102, 0x00); // Input := 0.
        m.cpu_write(0x4100, 0); // Register := Input ^ 0x0F (chip order).
        // Chip register 0x0F; read inverts chip bits 4-5 -> 0x3F; reversed
        // onto the bus -> 0x3F.
        assert_eq!(m.cpu_read(0x4100), 0x3F);
        m.cpu_write(0x4101, 0x00); // Invert := 0.
        // Chip 0x0F reversed: chip bits 0-3 are CPU bits 5-2 -> 0x3C.
        assert_eq!(m.cpu_read(0x4100), 0x3C);
        m.cpu_write(0x4101, 0x01); // CPU D0 is chip D5: not Invert.
        assert_eq!(m.cpu_read(0x4100), 0x3C, "Invert is CPU D5 only");
    }

    #[test]
    fn m172_increment_wraps_bits_0_3_and_keeps_4_5() {
        let mut m = board(172, 2, 4);
        // Chip register 0x3F: load chip Input 0x3F (CPU 0x3F reversed).
        m.cpu_write(0x4102, 0x3F);
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x4103, 0x20); // Mode := 1 (CPU D5).
        m.cpu_write(0x4100, 0); // Bits 0-3: 0xF + 1 -> 0x0; bits 4-5 kept.
        // Chip 0x30 reversed -> CPU 0x03.
        assert_eq!(m.cpu_read(0x4100), 0x03);
        m.cpu_write(0x4100, 0);
        // Chip 0x31 reversed -> CPU 0x23.
        assert_eq!(m.cpu_read(0x4100), 0x23);
        m.cpu_write(0x4103, 0x01); // CPU D0 is not Mode: back to load.
        m.cpu_write(0x4100, 0);
        assert_eq!(m.cpu_read(0x4100), 0x3F, "Mode 0 reloads Input");
    }

    #[test]
    fn m172_8000_latches_output_into_chr_and_sets_mirroring_from_invert() {
        let mut m = board(172, 2, 4);
        // Chip register 0x02 (CHR bank 2): CPU 0x10 reversed is chip 0x02.
        m.cpu_write(0x4102, 0x10);
        m.cpu_write(0x4100, 0);
        assert_eq!(m.ppu_read(0x0000), 0, "Output not yet latched");
        m.cpu_write(0x8000, 0xFF); // The written value is ignored.
        assert_eq!(m.ppu_read(0x0000), 2);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal, "Invert 0");
        // Chip 0x01 = CPU 0x20: CHR bank 1. Invert set before the latch.
        m.cpu_write(0x4102, 0x20);
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x4101, 0x20);
        m.cpu_write(0xFFFF, 0x00);
        assert_eq!(m.ppu_read(0x1FFF - 0x1FFF), 1);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical, "Invert 1");
        // Chip bits 2-5 of Output do not reach CHR (32 KiB, A13-A14 only).
        m.cpu_write(0x4101, 0);
        m.cpu_write(0x4102, 0x01); // chip 0x20
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x8000, 0);
        assert_eq!(m.ppu_read(0x0000), 0);
    }

    #[test]
    fn m172_state_round_trips() {
        let mut a = board(172, 2, 4);
        a.cpu_write(0x4102, 0x2A);
        a.cpu_write(0x4101, 0x20);
        a.cpu_write(0x4100, 0);
        a.cpu_write(0x8000, 0);
        a.cpu_write(0x4103, 0x20);
        a.ppu_write(0x2005, 0x77);
        let blob = a.save_state();
        let mut b = board(172, 2, 4);
        b.load_state(&blob).unwrap();
        assert_eq!(b.save_state(), blob);
        assert_eq!(b.cpu_read(0x4100), a.cpu_read(0x4100));
        assert_eq!(b.ppu_read(0x0000), a.ppu_read(0x0000));
        assert_eq!(b.current_mirroring(), a.current_mirroring());
        assert!(b.load_state(&blob[1..]).is_err());
    }

    // --- 173 ---------------------------------------------------------------

    #[test]
    fn m173_parses_with_hardwired_mirroring_and_mirrors_16k_prg() {
        let mut m = board(173, 1, 1);
        assert_eq!(m.cpu_read(0x8000), 0);
        assert_eq!(m.cpu_read(0xC000), 0, "16 KiB mirrored");
        assert!(m.has_hardwired_mirroring());
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        let mut m = board(173, 2, 4);
        assert_eq!(m.cpu_read(0xC000), 1, "32 KiB fixed");
    }

    #[test]
    fn m173_read_is_s_xor_v_over_rrr_with_d4_d7_open() {
        let mut m = board(173, 2, 4);
        m.cpu_write(0x4102, 0x0D); // S = 1, PPP = 5.
        assert_eq!(
            m.cpu_read(0x4100),
            0x08,
            "S reads back at once, RRR not yet"
        );
        m.cpu_write(0x4100, 0); // RRR := PPP.
        assert_eq!(m.cpu_read(0x4100), 0x0D);
        assert_eq!(m.cpu_read(0x41FF), 0x0D, "mask $E100");
        assert_eq!(m.cpu_read_driven_mask(0x4100), 0x0F);
        m.cpu_write(0x4101, 0x01); // V = 1: S XOR V = 0.
        assert_eq!(m.cpu_read(0x4100), 0x05, "RRR itself is not inverted");
        assert!(m.cpu_read_unmapped(0x4000 + 0x80));
        assert!(m.cpu_read_unmapped(0x7100));
    }

    #[test]
    fn m173_load_inverts_ppp_under_v_and_increment_wraps_three_bits() {
        let mut m = board(173, 2, 4);
        m.cpu_write(0x4102, 0x02);
        m.cpu_write(0x4101, 0x01);
        m.cpu_write(0x4100, 0);
        assert_eq!(m.cpu_read(0x4100) & 0x07, 0x05, "~PPP");
        m.cpu_write(0x4103, 0x01); // C = 1.
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x4100, 0);
        assert_eq!(m.cpu_read(0x4100) & 0x07, 0x00, "5 + 3 wraps in 3 bits");
        // Mask $E103: A2-A7 are not decoded, so $4104 is $4100; an address
        // with A8 clear ($4020) is not the chip.
        m.cpu_write(0x4104, 0);
        assert_eq!(m.cpu_read(0x4100) & 0x07, 0x01, "$4104 mirrors $4100");
        m.cpu_write(0x4020, 0);
        assert_eq!(m.cpu_read(0x4100) & 0x07, 0x01, "$4020 is not decoded");
    }

    #[test]
    fn m173_chr_is_output_bit_0_and_inverse_v() {
        let mut m = board(173, 2, 4);
        // V = 0: CHR A14 = 1 -> banks 2/3.
        m.cpu_write(0x4102, 0x01);
        m.cpu_write(0x4100, 0);
        assert_eq!(m.ppu_read(0x0000), 2, "Output 0, ~V = 1");
        m.cpu_write(0x8000, 0);
        assert_eq!(m.ppu_read(0x0000), 3, "Output bit 0 = 1");
        // V moves CHR A14 at once, without a $8000 latch.
        m.cpu_write(0x4101, 0x01);
        assert_eq!(m.ppu_read(0x0000), 1, "~V = 0");
        // Output bit 1 does not reach CHR on this board.
        m.cpu_write(0x4101, 0x00);
        m.cpu_write(0x4102, 0x02);
        m.cpu_write(0x4100, 0);
        m.cpu_write(0x8000, 0);
        assert_eq!(m.ppu_read(0x0000), 2, "RRR = 2: Output bit 0 clear");
    }

    #[test]
    fn m173_state_round_trips() {
        let mut a = board(173, 2, 4);
        a.cpu_write(0x4102, 0x0B);
        a.cpu_write(0x4100, 0);
        a.cpu_write(0x8000, 0);
        a.cpu_write(0x4103, 0x01);
        a.cpu_write(0x4101, 0x01);
        a.ppu_write(0x2400, 0x12);
        let blob = a.save_state();
        let mut b = board(173, 2, 4);
        b.load_state(&blob).unwrap();
        assert_eq!(b.save_state(), blob);
        assert_eq!(b.cpu_read(0x4100), a.cpu_read(0x4100));
        assert_eq!(b.ppu_read(0x0000), a.ppu_read(0x0000));
        assert_eq!(b.ppu_read(0x2400), 0x12);
    }

    #[test]
    fn rejects_bad_geometry() {
        assert!(Txc172::new(vec![0; 0x4000].into(), Box::default(), Mirroring::Vertical).is_err());
        assert!(
            Txc173::new(
                vec![0; 0x1000].into(),
                vec![0; 0x2000].into(),
                Mirroring::Vertical
            )
            .is_err()
        );
    }
}
