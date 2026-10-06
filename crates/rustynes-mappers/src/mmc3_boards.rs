// SPDX-License-Identifier: GPL-3.0-or-later
//! MMC3-based boards written from their NESdev pages (v2.9.6 "Roster"):
//! mappers 12, 37, 45, 47, 74, 121, 191, 192, 194, 195 and 249, and mapper 4
//! submapper 5 (the T9552 address scrambler).
//!
//! Every board here is a stock MMC3 (or a clone that behaves as one) with
//! something between the MMC3's bank outputs and the memories: an outer bank
//! register, a CHR-RAM overlay, or a protection latch that overrides PRG
//! banks. The MMC3 itself is the project's own [`Mmc3`] (`m004_mmc3.rs`),
//! used as a register file and IRQ counter through
//! [`Mmc3::register_core`]; this module owns the ROM and resolves each access
//! from the raw bank outputs ([`Mmc3::prg_bank_raw`], [`Mmc3::chr_bank_1k`]).
//! That keeps the IRQ timing work of `m004_mmc3.rs` (the A12 filter, the
//! Sharp / NEC revisions) shared by every board, and it leaves the mapper 4
//! path itself untouched.
//!
//! **Provenance.** Each board is implemented from the vendored NESdev page
//! named in its [`Board`] variant (`nesdev_wiki/output/INES_Mapper_NNN.md`)
//! and from `MMC3.md`. No reference-emulator source was read. This is
//! deliberately a separate module from `mmc3_clones.rs`, whose MMC3 variants
//! carry a Mesen2 derivation record (`docs/originality-and-provenance.md`
//! §1). Mixing independently written boards into that file would blur which
//! regions that record covers.
//!
//! **Bank arithmetic.** Every bank number is reduced modulo the size of the
//! memory it indexes before use, so no register value can index out of bounds.
//! This is required for the `#![no_std]` chip stack, which cannot afford a
//! panic on a register write.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::doc_markdown,
    clippy::match_same_arms
)]

use crate::cartridge::Mirroring;
use crate::m004_mmc3::{Mmc3, Mmc3Revision};
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use alloc::string::ToString;
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const WRAM_8K: usize = 0x2000;

/// Save-state layout version for [`Mmc3Board`].
const SAVE_STATE_VERSION: u8 = 1;

/// Which board an [`Mmc3Board`] models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Board {
    /// Mapper 12 submapper 0, Gouder SL-5020B (`INES_Mapper_012.md`): an
    /// MMC3A plus a GAL register at `$4100` (mask `$E100`) supplying CHR A18
    /// separately for each pattern table.
    M12,
    /// Mapper 37, "Super Mario Bros. + Tetris + Nintendo World Cup"
    /// (`INES_Mapper_037.md`): a 74HC161 in the MMC3's PRG-RAM window.
    M37,
    /// Mapper 45, GA23C (`INES_Mapper_045.md`): four outer registers written
    /// in turn at `$6000`, a lock bit, and a `$6001` reset.
    M45,
    /// Mapper 47, "Super Spike V'Ball + Nintendo World Cup"
    /// (`INES_Mapper_047.md`): one block bit in the PRG-RAM window.
    M47,
    /// Mapper 74, Waixing 43-393 (`INES_Mapper_074.md`): CHR banks 8 and 9
    /// are 2 KiB of CHR-RAM.
    M74,
    /// Mapper 121, Kasheng A9711 / A9713 (`INES_Mapper_121.md`): a protection
    /// array at `$5000` and bit-reversed PRG overrides at `$8001` / `$8003`.
    M121,
    /// Mapper 191 (`INES_Mapper_191.md`): CHR bank bit 7 selects 2 KiB of
    /// CHR-RAM.
    M191,
    /// Mapper 192, Waixing FS308 (`INES_Mapper_192.md`): CHR banks 8-11 are
    /// 4 KiB of CHR-RAM.
    M192,
    /// Mapper 194 (`INES_Mapper_194.md`): CHR banks 0 and 1 are 2 KiB of
    /// CHR-RAM.
    M194,
    /// Mapper 195, Waixing FS303 (`INES_Mapper_195.md`): which CHR banks are
    /// RAM is chosen by the bank a PPU write lands on.
    M195,
    /// Mapper 4 submapper 5 (`T9552.md`): Waixing's T9552 scrambles PRG
    /// A14-A17 and CHR A12-A17 by the pattern written to `$5000`. The file
    /// stores the banks in the `$5000 = $02` order, the true one.
    M4T9552,
    /// Mapper 249 (`INES_Mapper_249.md` → `T9552.md`): the same board, with
    /// the file stored in the `$5000 = $00` order.
    M249,
}

impl Board {
    const fn id(self) -> u16 {
        match self {
            Self::M12 => 12,
            Self::M37 => 37,
            Self::M45 => 45,
            Self::M47 => 47,
            Self::M74 => 74,
            Self::M121 => 121,
            Self::M191 => 191,
            Self::M192 => 192,
            Self::M194 => 194,
            Self::M195 => 195,
            Self::M4T9552 => 4,
            Self::M249 => 249,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::M12 => "SL-5020B (12)",
            Self::M37 => "SMB+Tetris+NWC (37)",
            Self::M45 => "GA23C (45)",
            Self::M47 => "Spike V'Ball+NWC (47)",
            Self::M74 => "Waixing 43-393 (74)",
            Self::M121 => "Kasheng A9711/A9713 (121)",
            Self::M191 => "Waixing MMC3 CHR-RAM (191)",
            Self::M192 => "Waixing FS308 (192)",
            Self::M194 => "Waixing MMC3 CHR-RAM (194)",
            Self::M195 => "Waixing FS303 (195)",
            Self::M4T9552 => "MMC3 + T9552 (4.5)",
            Self::M249 => "Waixing T9552 (249)",
        }
    }

    /// Default CHR-RAM overlay size for the mixed ROM/RAM boards.
    const fn overlay_bytes(self) -> usize {
        match self {
            Self::M74 | Self::M191 | Self::M194 => 2 * CHR_BANK_1K,
            Self::M192 => 4 * CHR_BANK_1K,
            // FS303 mounts 32 KiB, of which CHR A10-A12 reach 8 KiB.
            Self::M195 => 8 * CHR_BANK_1K,
            _ => 0,
        }
    }

    /// The Waixing boards are MMC3 boards with the usual 8 KiB of work RAM
    /// (their games save to it); the multicarts put a register there instead.
    const fn default_wram(self) -> usize {
        match self {
            Self::M74
            | Self::M191
            | Self::M192
            | Self::M194
            | Self::M195
            | Self::M4T9552
            | Self::M249 => WRAM_8K,
            _ => 0,
        }
    }
}

/// T9552 PRG patterns (`T9552.md`): column `$5000 & 3`, row r = the ROM
/// line (A14-A17) that MMC3 output line r's position maps to. Values 4-7 use
/// the same columns as 0-3.
const T9552_PRG: [[u8; 4]; 4] = [
    [16, 17, 15, 14],
    [17, 16, 14, 15],
    [14, 15, 16, 17],
    [15, 14, 17, 16],
];

/// T9552 CHR patterns: column `$5000 & 7`, rows over CHR A12-A17.
const T9552_CHR: [[u8; 6]; 8] = [
    [15, 12, 16, 17, 14, 13],
    [14, 15, 13, 12, 17, 16],
    [12, 13, 14, 15, 16, 17],
    [16, 14, 12, 13, 17, 15],
    [15, 13, 17, 16, 12, 14],
    [14, 12, 15, 16, 17, 13],
    [13, 16, 14, 15, 12, 17],
    [12, 15, 16, 17, 13, 14],
];

/// Apply a T9552 pattern to the address lines `first..first+N` of `bank`
/// (bank bit 0 is line `lsb_line`). Following the page: for each MMC3 line
/// that is set, find its row in the current column, and take the line named
/// in the same row of the file's column.
fn t9552_scramble<const N: usize>(
    bank: usize,
    lsb_line: u8,
    table: &[[u8; N]],
    current: usize,
    file: usize,
) -> usize {
    // Column 2 is the identity, so it lists the scrambled lines in order.
    // Fixed-size loops over the table, no allocation: this runs on every PRG
    // and CHR access.
    let mut out = bank;
    for &line in &table[2] {
        out &= !(1 << (line - lsb_line));
    }
    for &line in &table[2] {
        if bank & (1 << (line - lsb_line)) != 0 {
            let row = table[current].iter().position(|&l| l == line).unwrap_or(0);
            out |= 1 << (table[file][row] - lsb_line);
        }
    }
    out
}

/// Map a bank number onto an image of `count` banks that may not be a power
/// of two (`nesdev_wiki/output/Non_power_of_two_ROM_size.md`).
///
/// The page's doubling algorithm grows such an image to the next power of
/// two by repeatedly copying its last `lowbit(size)` banks onto its end: a
/// 24-bank (192 KiB) image becomes `ABCC` (banks 24-31 repeat 16-23), a
/// 20-bank (160 KiB) one grows 20 -> 24 -> 32. Plain `bank % count` is wrong
/// for those images: it sends the MMC3's all-ones fixed bank (`$FF`) to bank
/// 15 of 24 instead of 23, so `$E000` holds no reset vector and the game
/// never starts (the 192 KiB and 160 KiB translations on mapper 191).
///
/// This is the inverse of the doubling, computed without building the grown
/// image: reduce `bank` modulo the grown size, then, while it lies in a copied
/// region `[s, s + lowbit(s))`, step it back onto the region's source. For a
/// power-of-two `count` it is exactly `bank % count`. No allocation, a few
/// iterations at most (one per set bit of `count`): this runs on every PRG and
/// CHR-ROM access.
fn mirror_bank(bank: usize, count: usize) -> usize {
    if count == 0 {
        return 0;
    }
    let mut bank = bank & (count.next_power_of_two() - 1);
    while bank >= count {
        // Find the doubling stage whose copied region holds `bank`.
        let mut size = count;
        while size + (size & size.wrapping_neg()) <= bank {
            size += size & size.wrapping_neg();
        }
        bank -= size & size.wrapping_neg();
    }
    bank
}

/// Where a pattern-table access lands.
enum Chr {
    Rom(usize),
    Ram(usize),
}

/// Mapper 195's power-on CHR-RAM selection (`$80`: banks `$28-$2B`).
const M195_POWER_ON_MODE: u8 = 0x80;

/// Mapper 45's outer registers at power-on, after a soft reset and after a
/// `$6001` write (T-GA23C-POWERON). The page gives no value: it says only that
/// `$6001` resets them "as a soft reset would". Register 2's 4-bit CHR-AND
/// field is `$F`, which `chr_target` decodes to the 8-bit mask `$FF`: every
/// MMC3 CHR bank bit passes. It is `$F` because two *Famicom Yarou* menus draw
/// with CHR banks 0-7 before their first outer-register write, which needs a
/// mask of at least three bits. A black-box trace of both dumps showed no
/// `$5000-$7FFF` access before rendering. The maintainer chose `$F` over the
/// bare minimum (2026-10-05), matching the PRG-AND's inverted encoding, where 0
/// is the full window. PRG-OR, PRG-AND and CHR-OR stay 0, so the power-on PRG
/// window is unchanged.
const M45_RESET_REGS: [u8; 4] = [0x00, 0x00, 0x0F, 0x00];

/// The four-entry protection array mapper 121 returns at `$5000-$5FFF`.
const M121_PROTECTION: [u8; 4] = [0x83, 0x83, 0x42, 0x00];

/// An MMC3 board with a board-specific layer between the MMC3's bank outputs
/// and the memories. See the module docs.
pub struct Mmc3Board {
    board: Board,
    core: Mmc3,
    prg_rom: Box<[u8]>,
    /// CHR-ROM, or 8 KiB of CHR-RAM when the image has none.
    chr: Box<[u8]>,
    chr_is_ram: bool,
    /// The CHR-RAM overlay of the mixed ROM/RAM boards (74/191/192/194/195).
    chr_ram: Box<[u8]>,
    /// Work RAM at `$6000-$7FFF` (mapper 195: `$5000-$5FFF`), when present.
    wram: Box<[u8]>,
    /// Board registers. Meaning per board:
    /// - 12: `regs[0]` = the `$4100` CHR A18 register;
    /// - 37 / 47: `regs[0]` = the outer latch;
    /// - 45: `regs[0..4]` = the four outer registers;
    /// - 121: `regs[0]` = `$5180` outer bit, `regs[1]` = `$8001` latch,
    ///   `regs[2]` = `$8003` index, `regs[3]` = CHR A18 mode;
    /// - 195: `regs[0]` = the CHR-RAM selection.
    regs: [u8; 4],
    /// 45: which outer register the next `$6000` write reaches.
    /// 121: the protection-array index.
    index: u8,
    /// 45: the outer registers are locked (`$6000` #3 bit 6).
    locked: bool,
    /// 121: PRG overrides for `$A000`, `$C000`, `$E000` (`None` = MMC3).
    prg_override: [Option<u8>; 3],
    /// 121: which override slot later `$8001` writes keep updating.
    sticky: Option<u8>,
    /// 45: the menu DIP switch position (0-7).
    dip: u8,
}

impl Mmc3Board {
    /// Construct a board.
    ///
    /// `wram_bytes` is the header's PRG-RAM size, or 0 to take the board's
    /// default (8 KiB on the Waixing boards, none on the others).
    /// `chr_ram_bytes` overrides the CHR-RAM overlay size on the mixed
    /// ROM/RAM boards (NES 2.0 carries it), or 0 for the page's default.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 8 KiB
    /// or CHR is not a multiple of 1 KiB.
    pub fn new(
        board: Board,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        wram_bytes: usize,
        chr_ram_bytes: usize,
    ) -> Result<Self, MapperError> {
        let id = board.id();
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "mapper {id} PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        if !chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            return Err(MapperError::Invalid(format!(
                "mapper {id} CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else {
            chr_rom
        };
        let overlay = if board.overlay_bytes() == 0 {
            0
        } else if chr_ram_bytes >= CHR_BANK_1K {
            chr_ram_bytes
        } else {
            board.overlay_bytes()
        };
        let wram = if wram_bytes > 0 {
            wram_bytes
        } else {
            board.default_wram()
        };
        // Mapper 195's optional RAM is 4 KiB at `$5000-$5FFF`.
        let wram = if board == Board::M195 && wram_bytes == 0 {
            0
        } else if board == Board::M195 {
            0x1000
        } else {
            wram
        };
        // Mapper 12 is an MMC3A: the alternate ("NEC") IRQ behaviour, which
        // `Dragon Ball Z 5` needs (`INES_Mapper_012.md`, `MMC3.md` "IRQ
        // Specifics").
        let revision = if board == Board::M12 {
            Mmc3Revision::Nec
        } else {
            Mmc3Revision::Sharp
        };
        let mut this = Self {
            board,
            core: Mmc3::register_core(mirroring, revision),
            prg_rom,
            chr,
            chr_is_ram,
            chr_ram: vec![0u8; overlay].into_boxed_slice(),
            wram: vec![0u8; wram].into_boxed_slice(),
            regs: if board == Board::M45 {
                M45_RESET_REGS
            } else {
                [0; 4]
            },
            index: 0,
            locked: false,
            prg_override: [None; 3],
            sticky: None,
            dip: 0,
        };
        if board == Board::M195 {
            this.regs[0] = M195_POWER_ON_MODE;
        }
        Ok(this)
    }

    /// Set mapper 45's menu DIP switch (0-7). The Super New Year Cart 15-in-1
    /// reads it to pick one of eight menus.
    pub fn set_dip(&mut self, dip: u8) {
        self.dip = dip & 0x07;
    }

    /// The column of the T9552 tables the file's bank order follows.
    fn t9552_file_column(&self) -> usize {
        if self.board == Board::M249 { 0 } else { 2 }
    }

    /// Mapper 121's A9713 board carries 512 KiB of PRG and an outer bank.
    fn is_a9713(&self) -> bool {
        self.prg_rom.len() > 256 * 1024
    }

    /// The 8 KiB PRG bank for `addr` (`$8000-$FFFF`), before the modulo.
    fn prg_bank(&self, addr: u16) -> usize {
        let raw = self.core.prg_bank_raw(addr) as usize;
        let r = |i: usize| self.regs[i] as usize;
        match self.board {
            Board::M12 | Board::M74 | Board::M191 | Board::M192 | Board::M194 | Board::M195 => raw,
            Board::M4T9552 | Board::M249 => t9552_scramble(
                raw,
                13,
                &T9552_PRG,
                usize::from(self.regs[0] & 0x03),
                self.t9552_file_column(),
            ),
            Board::M37 => {
                // PRG A16 = Q0·Q1 + Q2·M16, PRG A17 = Q2 (the page's NAND
                // equations), over the MMC3's A13-A15.
                let q = r(0);
                let (q0, q1, q2) = (q & 1, (q >> 1) & 1, (q >> 2) & 1);
                let m16 = (raw >> 3) & 1;
                let a16 = (q0 & q1) | (q2 & m16);
                (raw & 0x07) | (a16 << 3) | (q2 << 4)
            }
            Board::M45 => {
                // PRG A13-A18: MMC3 AND the inverted PRG-AND, OR the PRG-OR;
                // A19-A20 from register 1's top bits, A21-A22 from
                // register 2's.
                let and = !r(3) & 0x3F;
                ((raw & and) | (r(1) & 0x3F)) | (r(1) & 0xC0) | ((r(2) & 0xC0) << 2)
            }
            Board::M47 => (raw & 0x0F) | ((r(0) & 1) << 4),
            Board::M121 => {
                let slot = match addr & 0xE000 {
                    0xA000 => Some(0),
                    0xC000 => Some(1),
                    0xE000 => Some(2),
                    _ => None,
                };
                let inner = slot
                    .and_then(|s| self.prg_override[s])
                    .map_or(raw & 0x1F, |b| b as usize & 0x1F);
                if self.is_a9713() {
                    inner | ((r(0) & 1) << 5)
                } else {
                    inner
                }
            }
        }
    }

    /// Resolve a pattern-table address to CHR-ROM or the CHR-RAM overlay.
    fn chr_target(&self, addr: u16) -> Chr {
        let addr = addr & 0x1FFF;
        let raw = self.core.chr_bank_1k(addr);
        let within = addr as usize & (CHR_BANK_1K - 1);
        let r = |i: usize| self.regs[i] as usize;
        let rom = |bank: usize| Chr::Rom(bank * CHR_BANK_1K + within);
        let ram = |bank: usize| Chr::Ram(bank * CHR_BANK_1K + within);
        match self.board {
            Board::M12 => {
                let a18 = if addr & 0x1000 == 0 {
                    r(0) & 1
                } else {
                    (r(0) >> 4) & 1
                };
                rom((raw & 0xFF) | (a18 << 8))
            }
            Board::M37 => rom((raw & 0x7F) | (((r(0) >> 2) & 1) << 7)),
            // T-GA23C-CHRRAM: CHR-RAM is addressed straight from PPU
            // A10-A12, bypassing every CHR bank. The mapper 45 page is
            // silent on CHR-RAM; mapper 372's page, the GA23C with a
            // ROM/RAM switch, documents its RAM as "unbanked".
            Board::M45 if self.chr_is_ram => Chr::Rom(usize::from(addr & 0x1FFF)),
            Board::M45 => {
                let c = r(2) & 0x0F;
                let mask = if c >= 7 { 0xFF >> (15 - c) } else { 0 };
                rom((raw & mask) | r(0) | ((r(2) & 0xF0) << 4))
            }
            Board::M47 => rom((raw & 0x7F) | ((r(0) & 1) << 7)),
            Board::M121 => {
                if self.is_a9713() {
                    rom((raw & 0xFF) | ((r(0) & 1) << 8))
                } else {
                    // A9711: CHR A18 follows PPU A12, inverted in mode 0.
                    let a12 = usize::from(addr & 0x1000 != 0);
                    let a18 = if r(3) & 0x80 != 0 { a12 } else { a12 ^ 1 };
                    rom((raw & 0xFF) | (a18 << 8))
                }
            }
            Board::M4T9552 | Board::M249 => rom(t9552_scramble(
                raw,
                10,
                &T9552_CHR,
                usize::from(self.regs[0] & 0x07),
                self.t9552_file_column(),
            )),
            Board::M74 if matches!(raw, 8 | 9) => ram(raw & 1),
            Board::M191 if raw & 0x80 != 0 => ram(raw & 1),
            Board::M192 if matches!(raw, 8..=11) => ram(raw & 3),
            Board::M194 if raw <= 1 => ram(raw & 1),
            Board::M195 if m195_ram_banks(self.regs[0]).is_some_and(|b| b.contains(&raw)) => {
                // CHR A10-A12 reach the RAM; A13 is not connected.
                ram(raw & 7)
            }
            _ => rom(raw),
        }
    }

    /// A CHR-ROM (or whole-image CHR-RAM) byte offset reduced onto the
    /// image, mirroring a non-power-of-two size per [`mirror_bank`].
    fn chr_rom_offset(&self, off: usize) -> usize {
        mirror_bank(off / CHR_BANK_1K, self.chr.len() / CHR_BANK_1K) * CHR_BANK_1K
            + (off & (CHR_BANK_1K - 1))
    }

    fn read_chr(&self, addr: u16) -> u8 {
        match self.chr_target(addr) {
            Chr::Rom(off) => self.chr[self.chr_rom_offset(off)],
            Chr::Ram(off) => self.chr_ram[off % self.chr_ram.len()],
        }
    }

    /// Mapper 121's `$8003` index write, and a `$8001` write while the index
    /// is one that keeps following the latch.
    fn m121_apply(&mut self) {
        let v = reverse_low_six(self.regs[1]);
        match self.regs[2] & 0x3F {
            0x26 => {
                self.prg_override[2] = Some(v);
                self.sticky = Some(2);
            }
            0x28 => {
                self.prg_override[1] = Some(v);
                self.sticky = Some(1);
            }
            0x2A => {
                self.prg_override[0] = Some(v);
                self.sticky = Some(0);
            }
            0x2C => {
                if v != 0 {
                    self.prg_override[2] = Some(v);
                }
                self.sticky = None;
            }
            0x2F => self.sticky = None,
            0x20 | 0x29 | 0x2B | 0x3C | 0x3F => {
                self.prg_override[2] = Some(v);
                self.sticky = None;
            }
            _ => {
                self.prg_override = [None; 3];
                self.sticky = None;
            }
        }
    }

    fn write_high(&mut self, addr: u16, value: u8) {
        if self.board == Board::M121 {
            match addr & 0xE003 {
                0x8000 | 0x8002 => {
                    if !self.is_a9713() {
                        self.regs[3] = value;
                    }
                    self.core.cpu_write(addr, value);
                }
                0x8001 => {
                    self.regs[1] = value;
                    if let Some(slot) = self.sticky {
                        self.prg_override[slot as usize] = Some(reverse_low_six(value));
                    }
                    self.core.cpu_write(addr, value);
                }
                0x8003 => {
                    self.regs[2] = value;
                    self.m121_apply();
                    // It also reaches the MMC3's `$8000`.
                    self.core.cpu_write(0x8000, value);
                }
                _ => self.core.cpu_write(addr, value),
            }
            return;
        }
        self.core.cpu_write(addr, value);
    }
}

/// Mapper 195: the CHR banks the selection `mode` maps to RAM, per the
/// table on `INES_Mapper_195.md`. Bit 4 set, or `$CA`, means none.
fn m195_ram_banks(mode: u8) -> Option<core::ops::RangeInclusive<usize>> {
    if mode & 0x10 != 0 {
        return None;
    }
    match mode & 0xCA {
        0x80 => Some(0x28..=0x2B),
        0x82 => Some(0x00..=0x03),
        0x88 => Some(0x4C..=0x4F),
        0x8A => Some(0x64..=0x67),
        0xC0 => Some(0x46..=0x47),
        0xC2 => Some(0x7C..=0x7D),
        0xC8 => Some(0x0A..=0x0B),
        _ => None,
    }
}

/// Mapper 121's latch transform: the low six bits reversed, the top two kept.
const fn reverse_low_six(v: u8) -> u8 {
    (v & 0xC0) | ((v & 0x3F).reverse_bits() >> 2)
}

impl Mapper for Mmc3Board {
    fn sram(&self) -> &[u8] {
        &self.wram
    }

    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.wram
    }

    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn reset(&mut self) {
        match self.board {
            // The latch is cleared by the CIC reset line (`INES_Mapper_037.md`).
            Board::M37 => self.regs[0] = 0,
            // "resets the outer bank registers as a soft reset would".
            Board::M45 => {
                self.regs = M45_RESET_REGS;
                self.index = 0;
                self.locked = false;
            }
            _ => {}
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match (self.board, addr) {
            (Board::M12, 0x4020..=0x5FFF) => addr & 0xE100 != 0x4100,
            (Board::M45 | Board::M121, 0x5000..=0x5FFF) => false,
            (Board::M195, 0x5000..=0x5FFF) => self.wram.is_empty(),
            (Board::M195, 0x6000..=0x7FFF) => true,
            (_, 0x4020..=0x5FFF) => true,
            (_, 0x6000..=0x7FFF) => self.wram.is_empty() || !self.core.prg_ram_enabled(),
            _ => false,
        }
    }

    fn cpu_read_driven_mask(&self, addr: u16) -> u8 {
        match (self.board, addr) {
            // The language bit (12) and the DIP bit (45) drive D0 only.
            (Board::M12, _) => 0x01,
            (Board::M45, 0x5000..=0x5FFF) => 0x01,
            _ => 0xFF,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank = mirror_bank(self.prg_bank(addr), self.prg_rom.len() / PRG_BANK_8K);
                self.prg_rom[bank * PRG_BANK_8K + (addr as usize & 0x1FFF)]
            }
            // Dragon Ball Z 5's language bit. The page says every known copy
            // is hard-wired to Chinese but does not say which level that is;
            // 0 is an assumption recorded in `docs/mappers.md`.
            0x4020..=0x5FFF if self.board == Board::M12 => 0,
            0x5000..=0x5FFF if self.board == Board::M45 => {
                // `0101 AAAA AAAA ....`: A(4+n) reads 1 when the switch is at n.
                u8::from((addr >> (4 + self.dip)) & 1 != 0)
            }
            0x5000..=0x5FFF if self.board == Board::M121 => {
                M121_PROTECTION[usize::from(self.index & 3)]
            }
            0x5000..=0x5FFF if self.board == Board::M195 && !self.wram.is_empty() => {
                self.wram[usize::from(addr & 0x0FFF) % self.wram.len()]
            }
            0x6000..=0x7FFF
                if self.board != Board::M195
                    && !self.wram.is_empty()
                    && self.core.prg_ram_enabled() =>
            {
                self.wram[usize::from(addr - 0x6000) % self.wram.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match (self.board, addr) {
            (_, 0x8000..=0xFFFF) => self.write_high(addr, value),
            (Board::M12, 0x4020..=0x5FFF) if addr & 0xE100 == 0x4100 => self.regs[0] = value,
            (Board::M37, 0x6000..=0x7FFF) if self.core.prg_ram_writable() => {
                self.regs[0] = value & 0x07;
            }
            (Board::M47, 0x6000..=0x7FFF) if self.core.prg_ram_writable() => {
                self.regs[0] = value & 0x01;
            }
            (Board::M45, 0x6000..=0x7FFF) => {
                // The outer registers overlay WRAM and ignore the MMC3's
                // PRG-RAM bits; the WRAM itself still obeys them.
                match addr & 0xF001 {
                    0x6000 if !self.locked => {
                        self.regs[usize::from(self.index)] = value;
                        if self.index == 3 && value & 0x40 != 0 {
                            self.locked = true;
                        }
                        self.index = (self.index + 1) & 3;
                    }
                    0x6001 => {
                        self.regs = M45_RESET_REGS;
                        self.index = 0;
                        self.locked = false;
                    }
                    _ => {}
                }
                if !self.wram.is_empty() && self.core.prg_ram_writable() {
                    let len = self.wram.len();
                    self.wram[usize::from(addr - 0x6000) % len] = value;
                }
            }
            (Board::M4T9552 | Board::M249, 0x5000..=0x5FFF) => self.regs[0] = value,
            (Board::M121, 0x5000..=0x5FFF) => {
                self.index = value & 0x03;
                if addr & 0xF180 == 0x5180 && self.is_a9713() {
                    self.regs[0] = value >> 7;
                }
            }
            (Board::M195, 0x5000..=0x5FFF) if !self.wram.is_empty() => {
                let len = self.wram.len();
                self.wram[usize::from(addr & 0x0FFF) % len] = value;
            }
            (Board::M195, _) => {}
            (_, 0x6000..=0x7FFF) if !self.wram.is_empty() && self.core.prg_ram_writable() => {
                let len = self.wram.len();
                self.wram[usize::from(addr - 0x6000) % len] = value;
            }
            _ => {}
        }
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        match self.chr_target(addr) {
            Chr::Rom(off) if !self.chr_is_ram => u32::try_from(self.chr_rom_offset(off)).ok(),
            _ => None,
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        if addr < 0x2000 {
            self.read_chr(addr)
        } else {
            self.core.ppu_read(addr)
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if addr >= 0x2000 {
            self.core.ppu_write(addr, value);
            return;
        }
        match self.chr_target(addr) {
            Chr::Ram(off) => {
                let len = self.chr_ram.len();
                self.chr_ram[off % len] = value;
            }
            Chr::Rom(off) if self.chr_is_ram => {
                let off = self.chr_rom_offset(off);
                self.chr[off] = value;
            }
            Chr::Rom(_) => {
                // FS303: a write to a bank mapped to ROM selects which banks
                // are RAM, from that bank's number (`INES_Mapper_195.md`;
                // bit 7 "must be 1").
                if self.board == Board::M195 {
                    let bank = self.core.chr_bank_1k(addr);
                    if bank & 0x80 != 0 {
                        self.regs[0] = bank as u8;
                    }
                }
            }
        }
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        self.core.nametable_address(addr)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.core.current_mirroring()
    }

    fn notify_a12(&mut self, level: bool) {
        self.core.notify_a12(level);
    }

    fn notify_a12_at_sub_dot(&mut self, level: bool, sub_dot: u8) {
        self.core.notify_a12_at_sub_dot(level, sub_dot);
    }

    fn notify_cpu_cycle(&mut self) {
        self.core.notify_cpu_cycle();
    }

    fn irq_pending(&self) -> bool {
        self.core.irq_pending()
    }

    fn irq_acknowledge(&mut self) {
        self.core.irq_acknowledge();
    }

    fn debug_info(&self) -> MapperDebugInfo {
        let mut info = self.core.debug_info();
        info.mapper_id = self.board.id();
        info.name = self.board.name().to_string();
        for (i, r) in self.regs.iter().enumerate() {
            info.extra.push((format!("board{i}"), format!("{r:#04x}")));
        }
        if !self.chr_ram.is_empty() {
            for slot in 0u16..8 {
                let is_ram = matches!(self.chr_target(slot * 0x400), Chr::Ram(_));
                info.chr_banks.push((
                    format!("slot{slot}"),
                    if is_ram { "RAM" } else { "ROM" }.to_string(),
                ));
            }
        }
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let core = self.core.save_state();
        let chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(16 + core.len() + chr + self.chr_ram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.board.id() as u8);
        out.extend_from_slice(&self.regs);
        out.push(self.index);
        out.push(u8::from(self.locked));
        for o in self.prg_override {
            out.push(u8::from(o.is_some()));
            out.push(o.unwrap_or(0));
        }
        out.push(self.sticky.map_or(0xFF, |s| s));
        out.push(self.dip);
        out.extend_from_slice(&(core.len() as u32).to_le_bytes());
        out.extend_from_slice(&core);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out.extend_from_slice(&self.chr_ram);
        out.extend_from_slice(&self.wram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        const HEAD: usize = 2 + 4 + 2 + 6 + 2 + 4;
        if data.len() < HEAD {
            return Err(MapperError::WrongLength {
                expected: HEAD,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        if u16::from(data[1]) != self.board.id() & 0xFF {
            return Err(MapperError::Invalid(format!(
                "state is for mapper {}, this board is mapper {}",
                data[1],
                self.board.id()
            )));
        }
        let core_len = u32::from_le_bytes([data[16], data[17], data[18], data[19]]) as usize;
        let chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        // Checked: `core_len` comes from the blob, and on a 32-bit target
        // (`usize` = u32) the plain sum wraps, passing the length check and
        // panicking when the core section is sliced.
        let expected = HEAD
            .checked_add(core_len)
            .and_then(|n| n.checked_add(chr + self.chr_ram.len() + self.wram.len()));
        if expected != Some(data.len()) {
            return Err(MapperError::WrongLength {
                expected: expected.unwrap_or(usize::MAX),
                got: data.len(),
            });
        }
        let sticky = match data[14] {
            0xFF => None,
            s @ 0..=2 => Some(s),
            s => {
                return Err(MapperError::Invalid(format!(
                    "override slot {s} out of range"
                )));
            }
        };
        let mut cur = HEAD;
        self.core.load_state(&data[cur..cur + core_len])?;
        cur += core_len;
        self.regs.copy_from_slice(&data[2..6]);
        self.index = data[6] & 3;
        self.locked = data[7] != 0;
        for (i, o) in self.prg_override.iter_mut().enumerate() {
            *o = (data[8 + 2 * i] != 0).then_some(data[9 + 2 * i]);
        }
        self.sticky = sticky;
        self.dip = data[15] & 7;
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[cur..cur + chr]);
            cur += chr;
        }
        let n = self.chr_ram.len();
        self.chr_ram.copy_from_slice(&data[cur..cur + n]);
        cur += n;
        self.wram.copy_from_slice(&data[cur..]);
        Ok(())
    }
}

#[cfg(test)]
#[path = "mmc3_boards_tests.rs"]
mod tests;
