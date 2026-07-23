//! Simple `BMC` multicart boards sharing one parameterised core: Waixing 164,
//! `BMC-810544-C-A1`, `BMC-60311C`, `BMC-830425C`, `K-3046`, `G-146`, and BS-5.
//!
//! Each is a handful of latch registers with no IRQ and no audio, differing
//! only in which address bits select which bank and in the NROM/UNROM-style
//! mode each supports. Rather than seven near-identical implementations,
//! [`SimpleBmc`] carries the common banking machinery and [`SimpleBoard`]
//! selects the per-board decode -- so a fix to the shared wrap-and-index math
//! lands on all of them at once.
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
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::doc_markdown,
    clippy::similar_names,
    clippy::too_many_lines,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::bool_to_int_with_if,
    clippy::unreadable_literal
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const PRG_BANK_16K: usize = 0x4000;
const PRG_BANK_32K: usize = 0x8000;
const CHR_BANK_2K: usize = 0x0800;
const CHR_BANK_8K: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

// ---------------------------------------------------------------------------
// Shared nametable + mirroring helpers (mirror the other simple-mapper modules).
// ---------------------------------------------------------------------------

const fn nametable_offset(addr: u16, mirroring: Mirroring) -> usize {
    let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
    let local = (addr as usize) & (NAMETABLE_SIZE - 1);
    let physical = mirroring.physical_bank(table);
    physical * NAMETABLE_SIZE + local
}

const fn mirroring_to_byte(m: Mirroring) -> u8 {
    match m {
        Mirroring::Horizontal => 0,
        Mirroring::Vertical => 1,
        Mirroring::SingleScreenA => 2,
        Mirroring::SingleScreenB => 3,
        Mirroring::FourScreen => 4,
        Mirroring::MapperControlled => 5,
    }
}

const fn byte_to_mirroring(b: u8, fallback: Mirroring) -> Mirroring {
    match b {
        0 => Mirroring::Horizontal,
        1 => Mirroring::Vertical,
        2 => Mirroring::SingleScreenA,
        3 => Mirroring::SingleScreenB,
        4 => Mirroring::FourScreen,
        5 => Mirroring::MapperControlled,
        _ => fallback,
    }
}

/// Validate a PRG-ROM image is a non-zero multiple of 8 KiB.
fn check_prg(prg: &[u8], id: u16) -> Result<(), MapperError> {
    if prg.is_empty() || !prg.len().is_multiple_of(PRG_BANK_8K) {
        return Err(MapperError::Invalid(format!(
            "mapper {id} PRG-ROM size {} is not a non-zero multiple of 8 KiB",
            prg.len()
        )));
    }
    Ok(())
}

/// Which discrete BMC board the [`SimpleBmc`] body models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SimpleBoard {
    /// Mapper 164 (Waixing "Final Fantasy V", 32 KiB PRG).
    M164,
    /// Mapper 261 (BMC-810544-C-A1).
    M261,
    /// Mapper 289 (BMC-60311C).
    M289,
    /// Mapper 320 (BMC-830425C-4391T).
    M320,
    /// Mapper 336 (BMC-K-3046).
    M336,
    /// Mapper 349 (BMC-G-146).
    M349,
    /// Mapper 286 (Waixing BS-5).
    M286,
}

/// A discrete BMC multicart with a simple (IRQ-free) register surface.
pub struct SimpleBmc {
    board: SimpleBoard,
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,
    vram: Box<[u8]>,
    mirroring: Mirroring,
    /// 16 KiB PRG window for $8000 and $C000.
    prg0: usize,
    prg1: usize,
    /// 8 KiB CHR window (164/261/289/320/336/349).
    chr8: usize,
    /// Per-2 KiB CHR windows (286).
    chr2: [usize; 4],
    /// Per-8 KiB PRG window for 286 (four 8 KiB windows).
    prg8: [usize; 4],
    // Board scratch registers.
    reg_inner: u8,
    reg_outer: u8,
    reg_mode: u8,
    dip: u8,
}

impl SimpleBmc {
    const SAVE_LEN: usize = 4 + 8 + 8 + 4 + 1;

    fn new(
        board: SimpleBoard,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        id: u16,
    ) -> Result<Self, MapperError> {
        check_prg(&prg_rom, id)?;
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; CHR_BANK_8K].into_boxed_slice()
        } else {
            chr_rom
        };
        let mut m = Self {
            board,
            prg_rom,
            chr,
            chr_is_ram,
            vram: vec![0u8; 2 * NAMETABLE_SIZE].into_boxed_slice(),
            mirroring,
            prg0: 0,
            prg1: 0,
            chr8: 0,
            chr2: [0; 4],
            prg8: [0; 4],
            reg_inner: 0,
            reg_outer: 0,
            reg_mode: 0,
            dip: 0,
        };
        m.reset_banks();
        Ok(m)
    }

    fn prg_count_16k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_16K).max(1)
    }

    fn prg_count_8k(&self) -> usize {
        (self.prg_rom.len() / PRG_BANK_8K).max(1)
    }

    fn chr_count_8k(&self) -> usize {
        (self.chr.len() / CHR_BANK_8K).max(1)
    }

    /// Initial / power-on bank layout per board.
    fn reset_banks(&mut self) {
        let last16 = self.prg_count_16k() - 1;
        match self.board {
            SimpleBoard::M164 => {
                // $5000/$5100 split 32 KiB PRG select; power-on 0x0F.
                self.reg_inner = 0x0F;
                self.update_m164();
            }
            SimpleBoard::M286 => {
                let last8 = self.prg_count_8k() - 1;
                let last_chr2 = (self.chr.len() / CHR_BANK_2K).max(1) - 1;
                for s in &mut self.prg8 {
                    *s = last8;
                }
                for c in &mut self.chr2 {
                    *c = last_chr2;
                }
            }
            SimpleBoard::M320 => self.update_m320(),
            SimpleBoard::M289 => self.update_m289(),
            _ => {
                self.prg0 = 0;
                self.prg1 = last16;
            }
        }
    }

    fn update_m164(&mut self) {
        // 32 KiB PRG window selected by the 8-bit split register.
        let count32 = (self.prg_rom.len() / PRG_BANK_32K).max(1);
        let bank32 = (self.reg_inner as usize) % count32;
        self.prg0 = bank32 * 2;
        self.prg1 = bank32 * 2 + 1;
    }

    fn update_m289(&mut self) {
        let page = self.reg_outer as usize
            | (if self.reg_mode & 0x04 != 0 {
                0
            } else {
                self.reg_inner as usize
            });
        match self.reg_mode & 0x03 {
            0 => {
                self.prg0 = page;
                self.prg1 = page;
            }
            1 => {
                let b = page & 0xFE;
                self.prg0 = b;
                self.prg1 = b | 1;
            }
            2 => {
                self.prg0 = page;
                self.prg1 = self.reg_outer as usize | 7;
            }
            _ => {}
        }
        self.mirroring = if self.reg_mode & 0x08 != 0 {
            Mirroring::Horizontal
        } else {
            Mirroring::Vertical
        };
    }

    fn update_m320(&mut self) {
        let outer = (self.reg_outer as usize) << 3;
        if self.reg_mode != 0 {
            // UNROM mode.
            self.prg0 = (self.reg_inner as usize & 0x07) | outer;
            self.prg1 = 0x07 | outer;
        } else {
            // UOROM mode.
            self.prg0 = (self.reg_inner as usize) | outer;
            self.prg1 = 0x0F | outer;
        }
    }

    fn prg_byte(&self, slot16: usize, addr: u16) -> u8 {
        let count = self.prg_count_16k();
        let bank = slot16 % count;
        self.prg_rom[bank * PRG_BANK_16K + (addr as usize & 0x3FFF)]
    }
}

impl Mapper for SimpleBmc {
    fn caps(&self) -> MapperCaps {
        MapperCaps::NONE
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        if self.board == SimpleBoard::M286 {
            if let 0x8000..=0xFFFF = addr {
                let slot = (addr as usize - 0x8000) / PRG_BANK_8K;
                let count = self.prg_count_8k();
                let bank = self.prg8[slot] % count;
                return self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)];
            }
            return 0;
        }
        match addr {
            0x8000..=0xBFFF => self.prg_byte(self.prg0, addr),
            0xC000..=0xFFFF => self.prg_byte(self.prg1, addr),
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match self.board {
            SimpleBoard::M164 => {
                if (0x5000..=0x5FFF).contains(&addr) {
                    match addr & 0x7300 {
                        0x5000 => self.reg_inner = (self.reg_inner & 0xF0) | (value & 0x0F),
                        0x5100 => self.reg_inner = (self.reg_inner & 0x0F) | ((value & 0x0F) << 4),
                        _ => {}
                    }
                    self.update_m164();
                }
            }
            SimpleBoard::M261 => {
                if addr >= 0x8000 {
                    let bank = ((addr >> 6) & 0xFFFE) as usize;
                    if addr & 0x40 != 0 {
                        self.prg0 = bank;
                        self.prg1 = bank | 1;
                    } else {
                        let b = bank | ((addr >> 5) & 0x01) as usize;
                        self.prg0 = b;
                        self.prg1 = b;
                    }
                    self.chr8 = (addr & 0x0F) as usize;
                    self.mirroring = if addr & 0x10 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            SimpleBoard::M289 => {
                if addr >= 0x8000 {
                    self.reg_inner = value & 0x07;
                } else {
                    match addr & 0xE001 {
                        0x6000 => self.reg_mode = value & 0x0F,
                        0x6001 => self.reg_outer = value,
                        _ => {}
                    }
                }
                self.update_m289();
            }
            SimpleBoard::M320 => {
                if addr >= 0x8000 {
                    self.reg_inner = value & 0x0F;
                    if addr & 0xFFE0 == 0xF0E0 {
                        self.reg_outer = (addr & 0x0F) as u8;
                        self.reg_mode = ((addr >> 4) & 0x01) as u8;
                    }
                    self.update_m320();
                }
            }
            SimpleBoard::M336 => {
                if addr >= 0x8000 {
                    let inner = value as usize & 0x07;
                    let outer = value as usize & 0x38;
                    self.prg0 = outer | inner;
                    self.prg1 = outer | 7;
                }
            }
            SimpleBoard::M349 => {
                if addr >= 0x8000 {
                    let a = addr as usize;
                    if a & 0x800 != 0 {
                        self.prg0 = (a & 0x1F) | (a & ((a & 0x40) >> 6));
                        self.prg1 = (a & 0x18) | 0x07;
                    } else if a & 0x40 != 0 {
                        self.prg0 = a & 0x1F;
                        self.prg1 = a & 0x1F;
                    } else {
                        let b = a & 0x1E;
                        self.prg0 = b;
                        self.prg1 = b | 1;
                    }
                    self.mirroring = if a & 0x80 != 0 {
                        Mirroring::Horizontal
                    } else {
                        Mirroring::Vertical
                    };
                }
            }
            SimpleBoard::M286 => {
                let bank = ((addr >> 10) & 0x03) as usize;
                match addr & 0xF000 {
                    0x8000 => self.chr2[bank] = (addr & 0x1F) as usize,
                    0xA000 if addr & (1u16 << (self.dip + 4)) != 0 => {
                        self.prg8[bank] = (addr & 0x0F) as usize;
                    }
                    _ => {}
                }
            }
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF => {
                if self.chr_is_ram {
                    return self.chr[addr as usize & (CHR_BANK_8K - 1)];
                }
                if self.board == SimpleBoard::M286 {
                    let slot = (addr as usize) / CHR_BANK_2K;
                    let count = (self.chr.len() / CHR_BANK_2K).max(1);
                    let bank = self.chr2[slot] % count;
                    return self.chr[bank * CHR_BANK_2K + (addr as usize & (CHR_BANK_2K - 1))];
                }
                let bank = self.chr8 % self.chr_count_8k();
                self.chr[bank * CHR_BANK_8K + (addr as usize & 0x1FFF)]
            }
            0x2000..=0x3EFF => self.vram[nametable_offset(addr, self.mirroring)],
            _ => 0,
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        match addr {
            0x0000..=0x1FFF if self.chr_is_ram => {
                self.chr[addr as usize & (CHR_BANK_8K - 1)] = value;
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
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(1 + Self::SAVE_LEN + self.vram.len() + chr_ram);
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&(self.prg0 as u32).to_le_bytes());
        out.extend_from_slice(&(self.prg1 as u32).to_le_bytes());
        out.extend_from_slice(&(self.chr8 as u32).to_le_bytes());
        for c in &self.chr2 {
            out.extend_from_slice(&(*c as u32).to_le_bytes());
        }
        for p in &self.prg8 {
            out.extend_from_slice(&(*p as u32).to_le_bytes());
        }
        out.push(self.reg_inner);
        out.push(self.reg_outer);
        out.push(self.reg_mode);
        out.push(self.dip);
        out.push(mirroring_to_byte(self.mirroring));
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_ram = if self.chr_is_ram { self.chr.len() } else { 0 };
        // header(1) + prg0/prg1/chr8(12) + chr2(16) + prg8(16) + 4 regs + mirror(1)
        let scratch = 1 + 12 + 16 + 16 + 4 + 1;
        let expected = scratch + self.vram.len() + chr_ram;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let rd = |c: usize| {
            u32::from_le_bytes([data[c], data[c + 1], data[c + 2], data[c + 3]]) as usize
        };
        let mut c = 1;
        self.prg0 = rd(c);
        self.prg1 = rd(c + 4);
        self.chr8 = rd(c + 8);
        c += 12;
        for s in &mut self.chr2 {
            *s = rd(c);
            c += 4;
        }
        for s in &mut self.prg8 {
            *s = rd(c);
            c += 4;
        }
        self.reg_inner = data[c];
        self.reg_outer = data[c + 1];
        self.reg_mode = data[c + 2];
        self.dip = data[c + 3];
        self.mirroring = byte_to_mirroring(data[c + 4], self.mirroring);
        c += 5;
        self.vram.copy_from_slice(&data[c..c + self.vram.len()]);
        c += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[c..c + self.chr.len()]);
        }
        Ok(())
    }
}

macro_rules! simple_ctor {
    ($fn_name:ident, $board:expr, $id:expr, $doc:expr) => {
        #[doc = $doc]
        ///
        /// # Errors
        /// [`MapperError::Invalid`] on a bad PRG/CHR size.
        pub fn $fn_name(
            prg_rom: Box<[u8]>,
            chr_rom: Box<[u8]>,
            mirroring: Mirroring,
        ) -> Result<SimpleBmc, MapperError> {
            SimpleBmc::new($board, prg_rom, chr_rom, mirroring, $id)
        }
    };
}

simple_ctor!(
    new_m164,
    SimpleBoard::M164,
    164,
    "Mapper 164 (Waixing Final Fantasy V, 32 KiB-PRG split register)."
);
simple_ctor!(
    new_m261,
    SimpleBoard::M261,
    261,
    "Mapper 261 (BMC-810544-C-A1 address-as-data multicart)."
);
simple_ctor!(
    new_m289,
    SimpleBoard::M289,
    289,
    "Mapper 289 (BMC-60311C NROM/UNROM multicart)."
);
simple_ctor!(
    new_m320,
    SimpleBoard::M320,
    320,
    "Mapper 320 (BMC-830425C-4391T UNROM/UOROM multicart)."
);
simple_ctor!(
    new_m336,
    SimpleBoard::M336,
    336,
    "Mapper 336 (BMC-K-3046 UNROM-style multicart)."
);
simple_ctor!(
    new_m349,
    SimpleBoard::M349,
    349,
    "Mapper 349 (BMC-G-146 NROM/UNROM/NROM-256 multicart)."
);
simple_ctor!(
    new_m286,
    SimpleBoard::M286,
    286,
    "Mapper 286 (Waixing BS-5 Olympic multicart)."
);

// ===========================================================================
// Kaiser FDS-conversion boards with a CPU-cycle (M2) IRQ:
//   56/142 (Kaiser202 / KS202 / KS7032), 303 (Kaiser7017), 253 (Waixing253).
// Plus the simple Kaiser PRG-window boards 305 (KS7031), 306 (KS7016),
// 312 (KS7013B). The CPU-cycle IRQ ones declare MapperCaps::CYCLE_IRQ.
// ===========================================================================

#[cfg(test)]
#[allow(clippy::cast_possible_truncation)]
mod tests {
    use super::*;

    fn synth_prg_8k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_8K];
        for b in 0..banks {
            v[b * PRG_BANK_8K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn synth_prg_16k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0xFFu8; banks * PRG_BANK_16K];
        for b in 0..banks {
            v[b * PRG_BANK_16K] = b as u8;
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

    fn synth_chr_2k(banks: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks * CHR_BANK_2K];
        for b in 0..banks {
            v[b * CHR_BANK_2K] = b as u8;
        }
        v.into_boxed_slice()
    }

    #[test]
    fn waixing164_split_prg_register() {
        let mut m = new_m164(synth_prg_16k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x5000, 0x02); // low nibble = 2
        m.cpu_write(0x5100, 0x00); // high nibble = 0 -> 32 KiB bank 2
        // 32 KiB bank 2 -> 16 KiB banks 4 and 5.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    #[test]
    fn bmc_simple_save_state_round_trip() {
        let mut m = new_m164(synth_prg_16k(16), Box::new([]), Mirroring::Vertical).unwrap();
        m.cpu_write(0x5000, 0x03);
        m.ppu_write(0x0007, 0x2B);
        let blob = m.save_state();
        let mut m2 = new_m164(synth_prg_16k(16), Box::new([]), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.cpu_read(0x8000), m.cpu_read(0x8000));
        assert_eq!(m2.ppu_read(0x0007), 0x2B);
    }

    #[test]
    fn bmc810544_address_decode() {
        let mut m = new_m261(synth_prg_16k(32), synth_chr_8k(16), Mirroring::Vertical).unwrap();
        // addr bit 6 set -> 32 KiB mode: bank = (addr>>6)&0xFFFE.
        m.cpu_write(0x8040 | (4 << 6), 0); // bank = (4<<6 ... ) wait compute below.
        // Simpler: write a precise address.
        m.cpu_write(0x8000 | (2 << 6) | 0x40, 0); // (addr>>6)&0xFFFE
        let lo = m.cpu_read(0x8000);
        let hi = m.cpu_read(0xC000);
        assert_eq!(hi, lo.wrapping_add(1)); // 32 KiB -> consecutive 16 KiB banks.
    }

    #[test]
    fn bmc60311_nrom_mode() {
        let mut m = new_m289(synth_prg_16k(8), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x6001, 0x02); // outer = 2
        m.cpu_write(0x6000, 0x00); // mode 0 = NROM-128 (mirror inner/outer)
        m.cpu_write(0x8000, 0x01); // inner = 1 -> page = 2|1 = 3
        assert_eq!(m.cpu_read(0x8000), 3);
        assert_eq!(m.cpu_read(0xC000), 3); // mirrored.
    }

    #[test]
    fn bmc830425_unrom_mode() {
        let mut m = new_m320(synth_prg_16k(32), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0xF0E0 | 0x10 | 0x02, 0x03); // outer=2, mode=1 (UNROM), inner=3
        // UNROM: prg0 = (3&7)|(2<<3)=19; prg1 = 7|(2<<3)=23.
        assert_eq!(m.cpu_read(0x8000), 19);
        assert_eq!(m.cpu_read(0xC000), 23);
    }

    #[test]
    fn bmc_k3046_unrom() {
        let mut m = new_m336(synth_prg_16k(16), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000, 0x0A); // inner=2, outer=8 -> prg0=8|2=10, prg1=8|7=15
        assert_eq!(m.cpu_read(0x8000), 10);
        assert_eq!(m.cpu_read(0xC000), 15);
    }

    #[test]
    fn bmc_g146_32k_mode() {
        let mut m = new_m349(synth_prg_16k(32), synth_chr_8k(1), Mirroring::Vertical).unwrap();
        // bit 11 clear, bit 6 clear -> 32 KiB mode: prg0=addr&0x1E, prg1=that|1.
        m.cpu_write(0x8000 | 0x04, 0); // addr&0x1E = 4
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xC000), 5);
    }

    #[test]
    fn bs5_chr_bank_decode() {
        let mut m = new_m286(synth_prg_8k(16), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        // $8000 with bank in bits 10-11, CHR index in bits 0-4.
        m.cpu_write(0x8000 | (1 << 10) | 0x05, 0); // chr bank 1 -> index 5
        assert_eq!(m.ppu_read(0x0800), 5); // 2 KiB slot 1 -> CHR bank 5.
    }

    #[test]
    fn bs5_save_state_round_trip() {
        let mut m = new_m286(synth_prg_8k(16), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        m.cpu_write(0x8000 | 0x03, 0);
        let blob = m.save_state();
        let mut m2 = new_m286(synth_prg_8k(16), synth_chr_2k(16), Mirroring::Vertical).unwrap();
        m2.load_state(&blob).unwrap();
        assert_eq!(m2.ppu_read(0x0000), m.ppu_read(0x0000));
    }
}
