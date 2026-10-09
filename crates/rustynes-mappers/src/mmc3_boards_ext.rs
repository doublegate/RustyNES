// SPDX-License-Identifier: GPL-3.0-or-later
//! MMC3-clone boards added in v3.2.0, written from their NESdev pages:
//! mappers 114 and 182 (the SuperGame / Hosenkan register scramblers) and
//! 215 (Sugar Softec's selectable scrambler, UNL-8237 / UNL-8237A).
//!
//! These are, like the boards in `mmc3_boards.rs`, a stock MMC3 with a
//! board-specific layer in front of it or behind it. The difference that
//! earns them a module of their own is *where* the layer sits: these boards
//! rewrite the CPU's register writes before the MMC3 sees them (an address
//! and index scramble), or rewire the PPU address lines the MMC3 sees, rather
//! than only post-processing the MMC3's bank outputs. The MMC3 itself is still
//! the project's own [`Mmc3`] used as a register file and IRQ counter
//! ([`Mmc3::register_core`]), so the A12 filter and the Sharp / NEC IRQ
//! revisions of `m004_mmc3.rs` are shared, and the mapper 4 path is untouched.
//!
//! **Provenance.** Each board is implemented from the vendored NESdev page
//! named on its [`ExtBoard`] variant (`nesdev_wiki/output/INES_Mapper_NNN.md`)
//! and from `MMC3.md`. No reference-emulator source was read. This file is
//! kept apart from `mmc3_clones.rs` for the same reason `mmc3_boards.rs` is:
//! that file carries a Mesen2 derivation record, and independently written
//! boards must not blur which regions it covers.
//!
//! **Bank arithmetic.** Every bank is reduced onto the image by
//! [`mirror_bank`] (the NESdev doubling rule for non-power-of-two sizes), so no
//! register value can index out of bounds. The chip stack is `#![no_std]` and
//! cannot afford a panic on a register write.
//!
//! ## Mapper 114 / 182 (`INES_Mapper_114.md`)
//!
//! An MMC3 clone whose register *addresses* and `$8000` *indices* are
//! permuted, plus two board registers that are not on the MMC3's WRAM
//! interface (they answer whether or not WRAM is enabled):
//!
//! - `$6000` (mask `$E001`): `M.S. BBBb`. With `M` set, a 16 KiB NROM-style
//!   bank `BBBb` replaces the MMC3's PRG banks at both halves; `S` set makes it
//!   NROM-256 by substituting CPU A14 for `b`.
//! - `$6001` (mask `$E001`): bit 0 is CHR A18, a 256 KiB outer CHR bank.
//!
//! Submapper 0 (Aladdin, The Lion King, Hosenkan's titles) and submapper 1
//! (Boogerman) use different permutations; both tables are on the page and are
//! transcribed into [`M114_SUB0`] / [`M114_SUB1`]. The page states the IRQ
//! behaves like the MMC3A's (a latch of 0 disables it, which Aladdin needs),
//! so the core runs [`Mmc3Revision::Nec`], the project's name for that
//! behaviour.
//!
//! **Mapper 182 is mapper 114.** `INES_Mapper_182.md` is a one-line redirect
//! to the 114 page, which says 182 "is a duplicate of Mapper 114" and that
//! Hosenkan's 182 games write zero to `$6000` / `$6001` once and never again.
//! So 182 is dispatched to mapper 114 submapper 0 with no separate model. The
//! zero writes are harmless under the full 114 model (NROM override off, CHR
//! A18 = 0), so there is no reason to model the "no `$600x` registers"
//! simplification some implementations use.
//!
//! ## Mapper 215 (`INES_Mapper_215.md`)
//!
//! Sugar Softec's later boards: the 114 idea with the scramble made
//! selectable and the board registers moved to `$5000-$5FFF` (mask `$F007`):
//!
//! - `$5000` `MCS. BBBb` (power-up `$00`): `M` and `S`/`BBBb` are 114's NROM
//!   override; `C` selects 128 KiB outer banks, replacing PRG A17 and CHR A17
//!   with `$5001` bits 4 and 5.
//! - `$5001` (power-up `$xF`, and reset to it on a console reset): the outer
//!   bank. Submapper 0 (UNL-8237): PRG A18-A19 = bits 0-1, CHR A18-A19 =
//!   bits 2-3. Submapper 1 (UNL-8237A): PRG A18/A19/A20 = bits 0/1/3, CHR
//!   A18/A19/A20 = bits 1/2/3.
//! - `$5007` (power-up `$00`): the scramble pattern, 0-7. Patterns 0, 2, 5, 6
//!   and 7 are the identity; 1, 3 and 4 permute the MMC3 register addresses
//!   and the `$8000` index per the page's two tables ([`M215_SCRAMBLE`]).
//!
//! The page gives no IRQ revision, saying only that the unscrambled registers
//! "function the same as the MMC3's", so the default (Sharp) behaviour is used.
//! Where the page leaves the NROM override and the 128 KiB mode's A17
//! substitution unordered, `C` is applied last, to whichever bank (MMC3 or
//! NROM) is in force: it is described as a substitution of the PRG A17 line,
//! which both sources drive.

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::doc_markdown
)]

use crate::cartridge::Mirroring;
use crate::m004_mmc3::{Mmc3, Mmc3Revision};
use crate::mapper::{Mapper, MapperCaps, MapperDebugInfo, MapperError};
use crate::mmc3_boards::mirror_bank;
use alloc::string::ToString;
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;

/// Save-state layout version for [`Mmc3BoardExt`].
const SAVE_STATE_VERSION: u8 = 1;

/// Fixed head of the save state: version, id, variant tag, four registers,
/// one latch byte, and the embedded MMC3 state's length.
const HEAD: usize = 3 + 4 + 1 + 4;

/// Which board an [`Mmc3BoardExt`] models.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExtBoard {
    /// Mapper 114 (and 182), `INES_Mapper_114.md`. `sub1` selects the
    /// submapper 1 (Boogerman) scrambling pattern.
    M114 {
        /// NES 2.0 submapper 1.
        sub1: bool,
    },
    /// Mapper 215, `INES_Mapper_215.md`. `sub1` selects the UNL-8237A outer
    /// bank layout (PRG and CHR A20).
    M215 {
        /// NES 2.0 submapper 1 (UNL-8237A).
        sub1: bool,
    },
}

impl ExtBoard {
    const fn id(self) -> u16 {
        match self {
            Self::M114 { .. } => 114,
            Self::M215 { .. } => 215,
        }
    }

    const fn name(self) -> &'static str {
        match self {
            Self::M114 { sub1: false } => "SuperGame MMC3 scramble (114.0)",
            Self::M114 { sub1: true } => "SuperGame MMC3 scramble (114.1)",
            Self::M215 { sub1: false } => "Sugar Softec UNL-8237 (215.0)",
            Self::M215 { sub1: true } => "Sugar Softec UNL-8237A (215.1)",
        }
    }

    /// A one-byte tag for the save state, distinct per variant so a state
    /// cannot be restored onto a different wiring of the same mapper.
    const fn state_tag(self) -> u8 {
        match self {
            Self::M114 { sub1: false } => 0,
            Self::M114 { sub1: true } => 1,
            Self::M215 { sub1: false } => 2,
            Self::M215 { sub1: true } => 3,
        }
    }
}

/// A register scramble: `addr[i]` is the MMC3 register (an `$E001`-masked
/// address) that a write to the `i`-th of `$8000, $8001, $A000, $A001, $C000,
/// $C001, $E000, $E001` reaches, and `index[v]` is the MMC3 bank-select index
/// that a written index `v` (the low three bits of a `$8000` value) means.
struct Scramble {
    addr: [u16; 8],
    index: [u8; 8],
}

/// Mapper 114 submapper 0, transcribed from the page's two tables.
const M114_SUB0: Scramble = Scramble {
    addr: [
        0xA001, 0xA000, 0x8000, 0xC000, 0x8001, 0xC001, 0xE000, 0xE001,
    ],
    index: [0, 3, 1, 5, 6, 7, 2, 4],
};

/// Mapper 114 submapper 1 ("different scrambling pattern").
const M114_SUB1: Scramble = Scramble {
    addr: [
        0xA001, 0x8001, 0x8000, 0xC001, 0xA000, 0xC000, 0xE000, 0xE001,
    ],
    index: [0, 2, 5, 3, 6, 1, 7, 4],
};

/// The identity scramble (mapper 215 patterns 0, 2, 5, 6 and 7).
const IDENTITY: Scramble = Scramble {
    addr: [
        0x8000, 0x8001, 0xA000, 0xA001, 0xC000, 0xC001, 0xE000, 0xE001,
    ],
    index: [0, 1, 2, 3, 4, 5, 6, 7],
};

/// Mapper 215's eight patterns, selected by `$5007`, transcribed from the
/// page's address table (row = `$5007`, columns `$8000 ... $E001` written)
/// and index table (row = `$5007`, column = the value written).
const M215_SCRAMBLE: [Scramble; 8] = [
    IDENTITY,
    Scramble {
        addr: [
            0xA001, 0xA000, 0x8000, 0xC000, 0x8001, 0xC001, 0xE000, 0xE001,
        ],
        index: [0, 2, 6, 1, 7, 3, 4, 5],
    },
    Scramble {
        addr: IDENTITY.addr,
        index: [0, 5, 4, 1, 7, 2, 6, 3],
    },
    Scramble {
        addr: [
            0xC001, 0x8000, 0x8001, 0xA000, 0xA001, 0xE001, 0xE000, 0xC000,
        ],
        index: [0, 6, 3, 7, 5, 2, 4, 1],
    },
    Scramble {
        addr: [
            0xA001, 0x8001, 0x8000, 0xC000, 0xA000, 0xC001, 0xE000, 0xE001,
        ],
        index: [0, 2, 5, 3, 6, 1, 7, 4],
    },
    IDENTITY,
    IDENTITY,
    IDENTITY,
];

/// Mapper 215's `$5001` power-up value, `$xF`: the last outer bank, so the
/// MMC3's fixed `$E000` bank lands on the image's reset vector. The page
/// leaves the high nibble undefined; it is taken as 0.
const M215_OUTER_POWER_UP: u8 = 0x0F;

/// The position of an `$E001`-masked MMC3 address in a [`Scramble`] row.
const fn scramble_slot(addr: u16) -> usize {
    ((((addr >> 13) & 3) << 1) | (addr & 1)) as usize
}

/// Unscramble one CPU write: the real MMC3 register address, and the value
/// with its index bits rewritten when that register is `$8000` (bank select).
/// Bits 3-7 of a `$8000` value pass through ("bits 6 and 7 ... are unchanged";
/// bits 3-5 are unused by the MMC3).
fn unscramble(table: &Scramble, addr: u16, value: u8) -> (u16, u8) {
    let real = table.addr[scramble_slot(addr)];
    let value = if real == 0x8000 {
        (value & 0xF8) | table.index[usize::from(value & 7)]
    } else {
        value
    };
    (real, value)
}

/// The NROM override shared by mappers 114 and 215: `M.S. BBBb`. With `M`
/// (bit 7) set, the 16 KiB bank `BBBb` replaces both MMC3 halves; with `S`
/// (bit 5) set, CPU A14 replaces `b` (NROM-256). Returns the 8 KiB bank, or
/// `None` while the MMC3's own banks apply.
fn nrom_override(reg: u8, addr: u16) -> Option<usize> {
    if reg & 0x80 == 0 {
        return None;
    }
    let mut bank16 = usize::from(reg & 0x0F);
    if reg & 0x20 != 0 {
        bank16 = (bank16 & !1) | usize::from((addr >> 14) & 1);
    }
    Some((bank16 << 1) | usize::from((addr >> 13) & 1))
}

/// An MMC3 board with a scramble or rewiring layer. See the module docs.
pub struct Mmc3BoardExt {
    board: ExtBoard,
    core: Mmc3,
    prg_rom: Box<[u8]>,
    /// CHR-ROM, or 8 KiB of CHR-RAM when the image has none.
    chr: Box<[u8]>,
    chr_is_ram: bool,
    /// Work RAM at `$6000-$7FFF`, only when a NES 2.0 header declares it.
    wram: Box<[u8]>,
    /// Board registers. 114: `regs[0]` = `$6000`, `regs[1]` = `$6001`.
    /// 215: `regs[0]` = `$5000`, `regs[1]` = `$5001`, `regs[2]` = `$5007`.
    regs: [u8; 4],
}

impl Mmc3BoardExt {
    /// Construct a board. `wram_bytes` is the NES 2.0 PRG-RAM size, or 0.
    ///
    /// # Errors
    ///
    /// [`MapperError::Invalid`] when PRG is not a non-zero multiple of 8 KiB
    /// or CHR is not a multiple of 1 KiB.
    pub fn new(
        board: ExtBoard,
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        wram_bytes: usize,
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
        // 114: "IRQ behavior resembles that of the MMC3A, i.e. a latch value
        // of zero disables the IRQ, on which Aladdin depends."
        let revision = match board {
            ExtBoard::M114 { .. } => Mmc3Revision::Nec,
            ExtBoard::M215 { .. } => Mmc3Revision::Sharp,
        };
        let mut regs = [0u8; 4];
        if matches!(board, ExtBoard::M215 { .. }) {
            regs[1] = M215_OUTER_POWER_UP;
        }
        Ok(Self {
            board,
            core: Mmc3::register_core(mirroring, revision),
            prg_rom,
            chr,
            chr_is_ram,
            wram: vec![0u8; wram_bytes].into_boxed_slice(),
            regs,
        })
    }

    /// The 8 KiB PRG bank for `addr` (`$8000-$FFFF`), before the mirror.
    fn prg_bank(&self, addr: u16) -> usize {
        let raw = self.core.prg_bank_raw(addr) as usize;
        match self.board {
            ExtBoard::M114 { .. } => nrom_override(self.regs[0], addr).unwrap_or(raw),
            ExtBoard::M215 { sub1 } => {
                let (mode, outer) = (self.regs[0], usize::from(self.regs[1]));
                let inner = nrom_override(mode, addr).unwrap_or(raw);
                // PRG A13-A17 inner; in 128 KiB mode A17 is `$5001` bit 4.
                let inner = if mode & 0x40 != 0 {
                    (inner & 0x0F) | (((outer >> 4) & 1) << 4)
                } else {
                    inner & 0x1F
                };
                // PRG A18-A19 (sub 0), A18-A20 (sub 1).
                let hi = if sub1 {
                    (outer & 3) | (((outer >> 3) & 1) << 2)
                } else {
                    outer & 3
                };
                inner | (hi << 5)
            }
        }
    }

    /// The 1 KiB CHR-ROM bank for a pattern-table address, before the mirror.
    fn chr_bank(&self, addr: u16) -> usize {
        let raw = self.core.chr_bank_1k(addr);
        match self.board {
            // `$6001` bit 0 is CHR A18 over the MMC3's A10-A17.
            ExtBoard::M114 { .. } => (raw & 0xFF) | (usize::from(self.regs[1] & 1) << 8),
            ExtBoard::M215 { sub1 } => {
                let outer = usize::from(self.regs[1]);
                // CHR A10-A17 inner; in 128 KiB mode A17 is `$5001` bit 5.
                let inner = if self.regs[0] & 0x40 != 0 {
                    (raw & 0x7F) | (((outer >> 5) & 1) << 7)
                } else {
                    raw & 0xFF
                };
                // CHR A18-A19 (sub 0: bits 2-3), A18-A20 (sub 1: bits 1-3).
                let hi = if sub1 {
                    (outer >> 1) & 7
                } else {
                    (outer >> 2) & 3
                };
                inner | (hi << 8)
            }
        }
    }

    fn chr_offset(&self, addr: u16) -> usize {
        let addr = addr & 0x1FFF;
        let bank = mirror_bank(self.chr_bank(addr), self.chr.len() / CHR_BANK_1K);
        bank * CHR_BANK_1K + (usize::from(addr) & (CHR_BANK_1K - 1))
    }

    fn write_high(&mut self, addr: u16, value: u8) {
        let (addr, value) = match self.board {
            ExtBoard::M114 { sub1: false } => unscramble(&M114_SUB0, addr, value),
            ExtBoard::M114 { sub1: true } => unscramble(&M114_SUB1, addr, value),
            ExtBoard::M215 { .. } => {
                unscramble(&M215_SCRAMBLE[usize::from(self.regs[2] & 7)], addr, value)
            }
        };
        self.core.cpu_write(addr, value);
    }
}

impl Mapper for Mmc3BoardExt {
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
        // 215: "The Outer Bank Register is reset to its power-up value if a
        // Reset has been detected via an interruption of the M2 signal."
        if matches!(self.board, ExtBoard::M215 { .. }) {
            self.regs[1] = M215_OUTER_POWER_UP;
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match addr {
            0x4020..=0x5FFF => true,
            0x6000..=0x7FFF => self.wram.is_empty() || !self.core.prg_ram_enabled(),
            _ => false,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x8000..=0xFFFF => {
                let bank = mirror_bank(self.prg_bank(addr), self.prg_rom.len() / PRG_BANK_8K);
                self.prg_rom[bank * PRG_BANK_8K + (usize::from(addr) & 0x1FFF)]
            }
            0x6000..=0x7FFF if !self.wram.is_empty() && self.core.prg_ram_enabled() => {
                self.wram[usize::from(addr - 0x6000) % self.wram.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match (self.board, addr) {
            (_, 0x8000..=0xFFFF) => self.write_high(addr, value),
            (ExtBoard::M114 { .. }, 0x6000..=0x7FFF) => {
                // Not on the WRAM interface: taken whatever `$A001` says.
                if addr & 0xE001 == 0x6000 {
                    self.regs[0] = value;
                } else {
                    self.regs[1] = value;
                }
                if !self.wram.is_empty() && self.core.prg_ram_writable() {
                    let len = self.wram.len();
                    self.wram[usize::from(addr - 0x6000) % len] = value;
                }
            }
            (ExtBoard::M215 { .. }, 0x5000..=0x5FFF) => match addr & 0xF007 {
                0x5000 => self.regs[0] = value,
                0x5001 => self.regs[1] = value,
                0x5007 => self.regs[2] = value & 7,
                _ => {}
            },
            (_, 0x6000..=0x7FFF) if !self.wram.is_empty() && self.core.prg_ram_writable() => {
                let len = self.wram.len();
                self.wram[usize::from(addr - 0x6000) % len] = value;
            }
            _ => {}
        }
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        if self.chr_is_ram {
            None
        } else {
            u32::try_from(self.chr_offset(addr)).ok()
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        let addr = addr & 0x3FFF;
        if addr < 0x2000 {
            self.chr[self.chr_offset(addr)]
        } else {
            self.core.ppu_read(addr)
        }
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        let addr = addr & 0x3FFF;
        if addr >= 0x2000 {
            self.core.ppu_write(addr, value);
        } else if self.chr_is_ram {
            let off = self.chr_offset(addr);
            self.chr[off] = value;
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
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let core = self.core.save_state();
        let chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        let mut out = Vec::with_capacity(HEAD + core.len() + chr + self.wram.len());
        out.push(SAVE_STATE_VERSION);
        out.push(self.board.id() as u8);
        out.push(self.board.state_tag());
        out.extend_from_slice(&self.regs);
        out.push(0); // Reserved for a board latch byte.
        out.extend_from_slice(&(core.len() as u32).to_le_bytes());
        out.extend_from_slice(&core);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out.extend_from_slice(&self.wram);
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        if data.len() < HEAD {
            return Err(MapperError::WrongLength {
                expected: HEAD,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        if u16::from(data[1]) != self.board.id() & 0xFF || data[2] != self.board.state_tag() {
            return Err(MapperError::Invalid(format!(
                "state is for mapper {} variant {}, this board is {}",
                data[1],
                data[2],
                self.board.name()
            )));
        }
        let core_len = u32::from_le_bytes([data[8], data[9], data[10], data[11]]) as usize;
        let chr = if self.chr_is_ram { self.chr.len() } else { 0 };
        // Checked: `core_len` comes from the blob, and a plain sum can wrap on
        // a 32-bit target, pass the check, and panic at the slice.
        let expected = HEAD
            .checked_add(core_len)
            .and_then(|n| n.checked_add(chr + self.wram.len()));
        if expected != Some(data.len()) {
            return Err(MapperError::WrongLength {
                expected: expected.unwrap_or(usize::MAX),
                got: data.len(),
            });
        }
        let mut cur = HEAD;
        self.core.load_state(&data[cur..cur + core_len])?;
        cur += core_len;
        self.regs.copy_from_slice(&data[3..7]);
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[cur..cur + chr]);
            cur += chr;
        }
        self.wram.copy_from_slice(&data[cur..]);
        Ok(())
    }
}

#[cfg(test)]
#[path = "mmc3_boards_ext_tests.rs"]
mod tests;
