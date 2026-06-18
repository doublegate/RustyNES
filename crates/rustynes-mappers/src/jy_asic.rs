//! J.Y. Company ASIC (iNES mappers 90 / 209 / 211) implementation.
//!
//! 晶太 (J.Y. Company)'s proprietary ASIC backs their later single-game
//! cartridges and most of their multicarts. It exposes a very flexible banking
//! surface: four PRG modes (32/16/8 KiB + an 8 KiB mode with the low 7 bank
//! bits reversed), four CHR modes (8/4/2/1 KiB granularity), an MMC4-like
//! automatic CHR-latch, optional ROM nametables / extended (per-1 KiB) CIRAM
//! mirroring, a hardware multiplier, and a configurable prescaler+counter IRQ
//! with four selectable clock sources.
//!
//! The three iNES mappers share one silicon implementation and differ only in
//! how the ROM-nametable / extended-mirroring feature is wired:
//!
//! - **209** is the standard implementation; the feature is register-enabled.
//! - **090** has the feature inhibited via a board jumper — it behaves as
//!   though `$D000` bit 5 (ROM nametables) and `$D001` bit 3 (extended
//!   mirroring) were always clear, so it always uses the simple `$D001` MM
//!   mirroring select.
//! - **211** behaves as though the feature were always enabled. No such PCB
//!   exists in hardware; the mapper number predates the discovery of `$D001`
//!   bit 3 and is a duplicate of 209 with correct emulation.
//!
//! This port follows the nesdev "J.Y. Company ASIC" page
//! (`nesdev_wiki/J_Y__Company_ASIC.xhtml`) and the Mesen2 `JyCompany`
//! implementation (`ref-proj/Mesen2/Core/NES/Mappers/JyCompany/JyCompany.h`).
//!
//! # Registers
//!
//! The wiki documents per-block address masks (`$5xxx`/`$8xxx`/`$Dxxx` are
//! `$F803`-masked, `$9xxx`/`$Axxx`/`$Bxxx` are `$F807`-masked, `$Cxxx` is
//! `$F007`-masked). The "Mask" column below is the mask this port actually
//! decodes, which follows **Mesen2** rather than the per-block wiki masks:
//! the `$5000-$5FFF` window decodes with `$F803`, and the entire
//! `$8000-$FFFF` register space decodes with a single `$F007` (Mesen2's
//! `WriteRegister` `switch(addr & 0xF007)`). `$F007` keeps A0-A2 and A12-A15
//! and discards A3-A11 — so, unlike the wiki's `$F803`/`$F807`, it does NOT
//! mask A11 (`$8800` still writes the `$8000` register). Because A2 survives
//! the mask, the eight-address banking blocks (PRG `$8000-$8007`, CHR
//! `$9000-$9007`/`$A000-$A007`, NT `$B000-$B007`) list all eight cases; the
//! PRG bank index then drops A2 via `& 0x03` so `$8004-$8007` alias
//! `$8000-$8003`. The `$C000-$C007` IRQ block uses all eight addresses as
//! distinct registers. The `$D000` mode block acts only on `$D000-$D003`
//! (Mesen2 lists no `$D004-$D007` cases), so those four addresses are inert.
//! This matches Mesen2 bit-for-bit; no known game
//! depends on the stricter wiki decode, so the unified mask is the accuracy
//! reference here.
//!
//! | Range          | Mask    | Purpose                                       |
//! |----------------|---------|-----------------------------------------------|
//! | `$5000`        | `$F803` | Jumper/dip read (we return 0)                 |
//! | `$5800/$5801`  | `$F803` | Hardware multiplier operands / result         |
//! | `$5803`        | `$F803` | Test/accumulator register (read/write)         |
//! | `$8000-$8007`  | `$F007` | PRG bank registers (7-bit; `$8004-7` alias `$8000-3`) |
//! | `$9000-$9007`  | `$F007` | CHR bank LSB registers                          |
//! | `$A000-$A007`  | `$F007` | CHR bank MSB registers                          |
//! | `$B000-$B003`  | `$F007` | Nametable bank LSB registers                    |
//! | `$B004-$B007`  | `$F007` | Nametable bank MSB registers                    |
//! | `$C000-$C007`  | `$F007` | IRQ control / prescaler / counter / XOR        |
//! | `$D000-$D003`  | `$F007` | Mode / mirroring / PPU-config / outer bank (`$D004-7` inert) |
//!
//! # IRQ
//!
//! A prescaler clocks an 8-bit counter. The clock source (`$C001` bits 0-1) is
//! one of: CPU M2 rise, PPU A12 rise, PPU render reads, or CPU writes. The
//! direction (`$C001` bits 6-7) selects increment (1), decrement (2), or
//! disabled (0/3). The prescaler mask (`$C001` bit 2) is `$FF` or `$07`. When
//! the masked prescaler wraps, the counter is clocked; when the counter wraps
//! ($FF->$00 up, or $00->$FF down) an IRQ is asserted (if enabled). Disabling
//! acknowledges the IRQ, inhibits counting, and resets the prescaler to zero.
//! `$C004`/`$C005` set the prescaler/counter (XORed with `$C006` first).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::match_same_arms,
    clippy::doc_markdown,
    clippy::struct_excessive_bools,
    clippy::similar_names,
    clippy::missing_const_for_fn,
    clippy::too_many_lines
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, format, vec, vec::Vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 1;

/// Which iNES mapper number wired the ASIC. Selects the ROM-nametable /
/// extended-mirroring policy and the mapper 209 MMC4 auto-latch behaviour.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum JyBoard {
    /// iNES mapper 90: ROM nametables / extended mirroring jumper-inhibited.
    M90,
    /// iNES mapper 209: standard implementation (feature register-enabled).
    M209,
    /// iNES mapper 211: feature always enabled (a 209 duplicate).
    M211,
}

impl JyBoard {
    const fn mapper_id(self) -> u16 {
        match self {
            Self::M90 => 90,
            Self::M209 => 209,
            Self::M211 => 211,
        }
    }
}

/// IRQ clock source from `$C001` bits 0-1.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum IrqSource {
    /// CPU M2 rise (every CPU cycle).
    CpuClock,
    /// PPU A12 rising edge (unfiltered).
    PpuA12Rise,
    /// PPU render reads.
    PpuRead,
    /// CPU writes.
    CpuWrite,
}

impl IrqSource {
    const fn from_bits(bits: u8) -> Self {
        match bits & 0x03 {
            0 => Self::CpuClock,
            1 => Self::PpuA12Rise,
            2 => Self::PpuRead,
            _ => Self::CpuWrite,
        }
    }

    const fn to_bits(self) -> u8 {
        match self {
            Self::CpuClock => 0,
            Self::PpuA12Rise => 1,
            Self::PpuRead => 2,
            Self::CpuWrite => 3,
        }
    }
}

/// J.Y. Company ASIC mapper (iNES 90 / 209 / 211).
pub struct JyAsic {
    board: JyBoard,
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    chr_is_ram: bool,

    // --- Banking registers ---
    prg_regs: [u8; 4],      // $8000-$8003 (7-bit)
    chr_low_regs: [u8; 8],  // $9000-$9007
    chr_high_regs: [u8; 8], // $A000-$A007
    nt_low_regs: [u8; 4],   // $B000-$B003
    nt_high_regs: [u8; 4],  // $B004-$B007

    chr_latch: [u8; 2], // MMC4-like CHR latch selectors (window 0 / 1)

    // --- Mode register $D000 ---
    prg_mode: u8,              // bits 0-2
    enable_prg_at_6000: bool,  // bit 7
    chr_mode: u8,              // bits 3-4
    advanced_nt_control: bool, // bit 5 (ROM nametables)
    nt_global: bool,           // bit 6 (ROM nametables for all)

    // --- $D001 / $D002 / $D003 ---
    mirroring_reg: u8,        // $D001 bits 0-1
    extended_mirroring: bool, // $D001 bit 3
    nt_ram_select_bit: u8,    // $D002 bit 7
    chr_block_mode: bool,     // derived from $D003 bit 5 (== 0 enables block mode)
    chr_block: u8,            // outer CHR block from $D003
    mirror_chr: bool,         // $D003 bit 7

    // --- IRQ ---
    irq_enabled: bool,
    irq_source: IrqSource,
    irq_count_direction: u8, // 0/3 disabled, 1 up, 2 down
    irq_small_prescaler: bool,
    irq_prescaler: u8,
    irq_counter: u8,
    irq_xor: u8,
    irq_funky_reg: u8, // $C007 (unknown mode; stored for round-trip)
    irq_pending: bool,

    // --- Misc registers ---
    mul1: u8,
    mul2: u8,
    test_reg: u8,

    last_ppu_addr: u16,
}

impl JyAsic {
    /// Construct a new J.Y. Company ASIC mapper for `board`.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB. CHR-ROM (when present)
    /// must be a multiple of 1 KiB; if absent, 256 KiB of CHR-RAM is allocated
    /// (the ASIC's max addressable CHR window) so games that bank CHR-RAM work.
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on a PRG/CHR size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        mirroring: Mirroring,
        board: JyBoard,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "JY ASIC PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            // 256 KiB CHR-RAM covers the largest CHR window the ASIC selects.
            vec![0u8; 256 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "JY ASIC CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        // The header mirroring seeds `$D001`; the ASIC overrides it at runtime.
        let mirroring_reg = match mirroring {
            Mirroring::Horizontal => 1,
            Mirroring::SingleScreenA => 2,
            Mirroring::SingleScreenB => 3,
            // Vertical (and any odd header value) -> 0.
            _ => 0,
        };
        Ok(Self {
            board,
            prg_rom,
            chr,
            chr_is_ram,
            prg_regs: [0; 4],
            chr_low_regs: [0; 8],
            chr_high_regs: [0; 8],
            nt_low_regs: [0; 4],
            nt_high_regs: [0; 4],
            // Mesen seeds the latch to {0, 4} so the two 4 KiB windows index the
            // distinct low/high CHR register groups before any MMC4 fetch.
            chr_latch: [0, 4],
            prg_mode: 0,
            enable_prg_at_6000: false,
            chr_mode: 0,
            advanced_nt_control: false,
            nt_global: false,
            mirroring_reg,
            extended_mirroring: false,
            nt_ram_select_bit: 0,
            chr_block_mode: false,
            chr_block: 0,
            mirror_chr: false,
            irq_enabled: false,
            irq_source: IrqSource::CpuClock,
            irq_count_direction: 0,
            irq_small_prescaler: false,
            irq_prescaler: 0,
            irq_counter: 0,
            irq_xor: 0,
            irq_funky_reg: 0,
            irq_pending: false,
            mul1: 0,
            mul2: 0,
            test_reg: 0,
            last_ppu_addr: 0,
        })
    }

    /// Whether the board exposes ROM nametables / extended mirroring.
    ///
    /// Mapper 211 forces it on; mapper 90 forces it off; mapper 209 follows the
    /// `$D000` bit 5 / `$D001` bit 3 registers.
    const fn nt_control_active(&self) -> bool {
        match self.board {
            JyBoard::M211 => true,
            JyBoard::M90 => false,
            JyBoard::M209 => self.advanced_nt_control || self.extended_mirroring,
        }
    }

    /// Apply the PRG bank-number reversal used by PRG mode 3 (`$D000` bits
    /// 0-1 == 3).
    ///
    /// The wiki describes this as "bank numbers bits 0-6 reversed". This is a
    /// verbatim port of Mesen2's `InvertPrgBits`, which reverses the three
    /// outer bit pairs (0<->6, 1<->5, 2<->4) and notably does **not** carry
    /// bit 3 through: a faithful "reverse a 7-bit field" would leave the
    /// centre bit (3) in place, but neither Mesen2 nor Disch's original
    /// writeup preserves it, so this port drops it to match the accuracy
    /// reference bit-for-bit (no known game distinguishes the two; the JY
    /// ASIC is BestEffort tier). If a future test ROM proves bit 3 must be
    /// preserved, OR `reg & 0x08` back into the result here.
    const fn invert_prg_bits(reg: u8, invert: bool) -> u8 {
        if invert {
            (reg & 0x01) << 6
                | (reg & 0x02) << 4
                | (reg & 0x04) << 2
                | (reg & 0x10) >> 2
                | (reg & 0x20) >> 4
                | (reg & 0x40) >> 6
        } else {
            reg
        }
    }

    /// Resolve an 8 KiB PRG bank index for the given CPU window.
    ///
    /// `window` is the 8 KiB slot 0..=3 (`$8000`/`$A000`/`$C000`/`$E000`).
    fn prg_bank_8k(&self, window: usize) -> usize {
        let invert = (self.prg_mode & 0x03) == 0x03;
        let r: [usize; 4] = [
            Self::invert_prg_bits(self.prg_regs[0], invert) as usize,
            Self::invert_prg_bits(self.prg_regs[1], invert) as usize,
            Self::invert_prg_bits(self.prg_regs[2], invert) as usize,
            Self::invert_prg_bits(self.prg_regs[3], invert) as usize,
        ];
        let last_switchable = (self.prg_mode & 0x04) != 0;
        match self.prg_mode & 0x03 {
            // 32 KiB: one bank across all four windows.
            0 => {
                let base = if last_switchable { r[3] * 4 } else { 0x3C };
                base + window
            }
            // 16 KiB: r1<<1 for the low half, r3 (or fixed $3E) for the high.
            1 => {
                if window < 2 {
                    (r[1] << 1) + window
                } else {
                    let base = if last_switchable { r[3] << 1 } else { 0x3E };
                    base + (window - 2)
                }
            }
            // 8 KiB (modes 2 and 3): each window has its own register; the last
            // window is fixed to $3F unless made switchable.
            _ => match window {
                0 => r[0],
                1 => r[1],
                2 => r[2],
                _ => {
                    if last_switchable {
                        r[3]
                    } else {
                        0x3F
                    }
                }
            },
        }
    }

    /// Resolve the 8 KiB PRG bank mapped at `$6000-$7FFF` (only when `$D000`
    /// bit 7 enables it).
    fn prg_bank_6000(&self) -> usize {
        let invert = (self.prg_mode & 0x03) == 0x03;
        let r3 = Self::invert_prg_bits(self.prg_regs[3], invert) as usize;
        match self.prg_mode & 0x03 {
            0 => r3 * 4 + 3,
            1 => (r3 << 1) | 1,
            _ => r3,
        }
    }

    fn prg_offset(&self, addr: u16) -> usize {
        let total = (self.prg_rom.len() / PRG_BANK_8K).max(1);
        let bank = if (0x6000..0x8000).contains(&addr) {
            self.prg_bank_6000()
        } else {
            let window = ((addr >> 13) & 0x03) as usize; // ($8000..) / 0x2000
            self.prg_bank_8k(window)
        };
        (bank % total) * PRG_BANK_8K + (addr as usize & 0x1FFF)
    }

    /// Resolve the effective CHR register value for index 0..=7, applying the
    /// CHR-block (outer-bank) mode and `mirror_chr` aliasing.
    fn chr_reg(&self, index: usize) -> u32 {
        // mirror_chr aliases the high half of the CHR window onto the low half
        // in 2/1 KiB modes (Mesen: chrMode >= 2 && mirrorChr && index 2/3).
        let index = if self.chr_mode >= 2 && self.mirror_chr && (index == 2 || index == 3) {
            index - 2
        } else {
            index
        };
        if self.chr_block_mode {
            let (mask, shift): (u32, u32) = match self.chr_mode {
                0 => (0x1F, 5),
                1 => (0x3F, 6),
                2 => (0x7F, 7),
                _ => (0xFF, 8),
            };
            (self.chr_low_regs[index] as u32 & mask) | ((self.chr_block as u32) << shift)
        } else {
            self.chr_low_regs[index] as u32 | ((self.chr_high_regs[index] as u32) << 8)
        }
    }

    /// Resolve a physical CHR offset for a PPU pattern-table address.
    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_1k = (self.chr.len() / CHR_BANK_1K).max(1);
        let slot_1k = addr / CHR_BANK_1K; // 0..=7
        let bank_1k: usize = match self.chr_mode {
            // 8 KiB: reg 0 selects the whole window.
            0 => (self.chr_reg(0) as usize) * 8 + slot_1k,
            // 4 KiB: two windows, MMC4 latch picks the register for each.
            1 => {
                let reg = if addr < 0x1000 {
                    self.chr_latch[0] as usize
                } else {
                    self.chr_latch[1] as usize
                };
                (self.chr_reg(reg) as usize) * 4 + (slot_1k & 0x03)
            }
            // 2 KiB: regs 0/2/4/6 select four 2 KiB windows.
            2 => {
                let reg = (slot_1k & !1) & 0x07;
                (self.chr_reg(reg) as usize) * 2 + (slot_1k & 0x01)
            }
            // 1 KiB: each slot has its own register.
            _ => self.chr_reg(slot_1k) as usize,
        };
        (bank_1k % total_1k) * CHR_BANK_1K + (addr & (CHR_BANK_1K - 1))
    }

    fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        let physical = self.ciram_bank_for(table);
        physical * NAMETABLE_SIZE + local
    }

    /// Resolve a logical nametable index 0..=3 to a physical CIRAM bank (0 or 1).
    ///
    /// When the ROM-nametable / Extended-Mirroring feature is active
    /// ([`Self::nt_control_active`]), each nametable picks its CIRAM page
    /// independently from `$B00x` bit 0 (Extended Mirroring, `$D001` bit 3).
    /// This mirrors Mesen2's `UpdateMirroringState`, which calls
    /// `SetNametable(i, _ntLowRegs[i] & 0x01)` for each of the four tables
    /// whenever advanced NT control is on (and the PCB is not mapper 90).
    /// Otherwise the simple `$D001` MM register selects the layout.
    const fn ciram_bank_for(&self, table: u8) -> usize {
        if self.nt_control_active() {
            (self.nt_low_regs[table as usize & 0x03] & 0x01) as usize
        } else {
            Self::physical_bank_for(table, self.mirroring_reg)
        }
    }

    /// Resolve a logical nametable index 0..=3 to a physical CIRAM bank (0 or 1)
    /// using the `$D001` MM register directly (the ASIC's MM encoding differs
    /// from the header [`Mirroring`] enum).
    const fn physical_bank_for(table: u8, mirroring_reg: u8) -> usize {
        match mirroring_reg & 0x03 {
            // 0: Vertical (tables 0/2 -> A, 1/3 -> B).
            0 => table as usize & 1,
            // 1: Horizontal (tables 0/1 -> A, 2/3 -> B).
            1 => (table >> 1) as usize & 1,
            // 2: one-screen page 0.
            2 => 0,
            // 3: one-screen page 1.
            _ => 1,
        }
    }

    /// Update the MMC4-like CHR latch on mapper 209 when a pattern fetch hits a
    /// sentinel address ($x FD8-$x FDF / $x FE8-$x FEF).
    fn update_chr_latch_209(&mut self, addr: u16) {
        if self.board != JyBoard::M209 {
            return;
        }
        match addr & 0x2FF8 {
            0x0FD8 | 0x0FE8 => {
                // Mesen: chrLatch[addr>>12] = (addr>>4) & ((addr>>10 & 4) | 2)
                let idx = (addr >> 12) as usize & 0x01;
                self.chr_latch[idx] = ((addr >> 4) as u8) & (((addr >> 10) as u8 & 0x04) | 0x02);
            }
            _ => {}
        }
    }

    /// Whether the given nametable index ($2000-$2FFF / index 0..=3) reads from
    /// ROM (CHR) rather than CIRAM, under the current `$D000`/`$D001`/`$D002`
    /// configuration. Only meaningful when [`Self::nt_control_active`].
    fn nt_index_is_rom(&self, nt_index: usize) -> bool {
        if self.extended_mirroring {
            // Extended mirroring uses CIRAM banks only (bit 0 of $B00x).
            return false;
        }
        if !self.advanced_nt_control && self.board != JyBoard::M211 {
            return false;
        }
        if self.nt_global {
            // ROM nametables for all four nametables.
            true
        } else {
            // Per-nametable: ROM when $B00x bit 7 differs from $D002 bit 7.
            (self.nt_low_regs[nt_index] & 0x80) != (self.nt_ram_select_bit & 0x80)
        }
    }

    /// Read a CHR/ROM nametable byte for an address in $2000-$2FFF.
    fn nt_rom_byte(&self, addr: u16) -> u8 {
        let nt_index = ((addr & 0x0FFF) / NAMETABLE_SIZE_U16) as usize & 0x03;
        let page =
            self.nt_low_regs[nt_index] as usize | ((self.nt_high_regs[nt_index] as usize) << 8);
        let off = page * NAMETABLE_SIZE + (addr as usize & 0x3FF);
        if off < self.chr.len() {
            self.chr[off]
        } else {
            0
        }
    }

    /// Tick the IRQ prescaler+counter once for the active clock source.
    fn tick_irq(&mut self) {
        if self.irq_count_direction != 0x01 && self.irq_count_direction != 0x02 {
            return; // counting disabled (directions 0 and 3)
        }
        let mask: u8 = if self.irq_small_prescaler { 0x07 } else { 0xFF };
        let mut prescaler = self.irq_prescaler & mask;
        let mut clock_counter = false;
        if self.irq_count_direction == 0x01 {
            prescaler = prescaler.wrapping_add(1);
            if (prescaler & mask) == 0 {
                clock_counter = true;
            }
        } else {
            prescaler = prescaler.wrapping_sub(1);
            if (prescaler & mask) == mask {
                clock_counter = true;
            }
        }
        self.irq_prescaler = (self.irq_prescaler & !mask) | (prescaler & mask);

        if clock_counter {
            if self.irq_count_direction == 0x01 {
                self.irq_counter = self.irq_counter.wrapping_add(1);
                if self.irq_counter == 0 && self.irq_enabled {
                    self.irq_pending = true;
                }
            } else {
                self.irq_counter = self.irq_counter.wrapping_sub(1);
                if self.irq_counter == 0xFF && self.irq_enabled {
                    self.irq_pending = true;
                }
            }
        }
    }

    /// Disable + acknowledge the IRQ, inhibit counting, and reset the prescaler.
    fn irq_disable(&mut self) {
        self.irq_enabled = false;
        self.irq_pending = false;
        self.irq_prescaler = 0;
    }
}

impl Mapper for JyAsic {
    // CPU-cycle hook (for the CPU-clock IRQ source) + IRQ source. No audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x5000..=0x5FFF => match addr & 0xF803 {
                // Jumper/dip switches — return 0 (no multicart selection).
                0x5000 => 0,
                0x5800 => (self.mul1 as u16 * self.mul2 as u16) as u8,
                0x5801 => ((self.mul1 as u16 * self.mul2 as u16) >> 8) as u8,
                0x5803 => self.test_reg,
                _ => 0,
            },
            0x6000..=0x7FFF if self.enable_prg_at_6000 => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match addr {
            // The $5000-$5FFF register window is mapped (multiplier / test / dip).
            0x5000..=0x5FFF => false,
            // $6000-$7FFF only maps when PRG is routed there.
            0x6000..=0x7FFF => !self.enable_prg_at_6000,
            // The rest of $4020-$5FFF is unmapped (open bus).
            _ => (0x4020..0x5000).contains(&addr),
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        if addr < 0x8000 {
            match addr & 0xF803 {
                0x5800 => self.mul1 = value,
                0x5801 => self.mul2 = value,
                0x5803 => self.test_reg = value,
                _ => {}
            }
        } else {
            // Mesen2 decodes the whole $8000-$FFFF register space with a single
            // $F007 mask (`switch(addr & 0xF007)`), not the wiki's per-block
            // $F803/$F807. $F007 keeps A0-A2 and A12-A15 but discards A3-A11,
            // so A11 is NOT masked (unlike the wiki's $F803 — $8800 still hits
            // the $8000 register). A2 is kept by the mask, so register blocks
            // that span eight addresses (PRG $8000-$8007, the CHR/NT $9/$A/$B
            // blocks) must list all eight; the PRG bank index then drops A2 via
            // `& 0x03` so $8004-$8007 alias $8000-$8003 exactly as Mesen2 does.
            // (The $C000 IRQ and $D000 mode blocks intentionally only act on
            // $x000-$x003 — Mesen2 lists no $x004-$x007 cases there either, so
            // those addresses are inert.) See the module-level register table.
            match addr & 0xF007 {
                // PRG bank registers (7-bit). $8004-$8007 alias $8000-$8003
                // (A2 dropped by the `& 0x03` index), matching Mesen2's eight
                // explicit $8000-$8007 cases.
                0x8000..=0x8007 => self.prg_regs[(addr & 0x03) as usize] = value & 0x7F,
                // CHR LSB registers.
                0x9000..=0x9007 => self.chr_low_regs[(addr & 0x07) as usize] = value,
                // CHR MSB registers.
                0xA000..=0xA007 => self.chr_high_regs[(addr & 0x07) as usize] = value,
                // Nametable bank LSB.
                0xB000..=0xB003 => self.nt_low_regs[(addr & 0x03) as usize] = value,
                // Nametable bank MSB.
                0xB004..=0xB007 => self.nt_high_regs[(addr & 0x03) as usize] = value,
                // IRQ enable.
                0xC000 => {
                    if value & 0x01 != 0 {
                        self.irq_enabled = true;
                    } else {
                        self.irq_disable();
                    }
                }
                // IRQ mode/flags.
                0xC001 => {
                    self.irq_count_direction = (value >> 6) & 0x03;
                    self.irq_small_prescaler = (value & 0x04) != 0;
                    self.irq_source = IrqSource::from_bits(value);
                }
                // IRQ disable (acknowledge).
                0xC002 => self.irq_disable(),
                // IRQ enable.
                0xC003 => self.irq_enabled = true,
                // Set prescaler (XORed with $C006).
                0xC004 => self.irq_prescaler = value ^ self.irq_xor,
                // Set counter (XORed with $C006).
                0xC005 => self.irq_counter = value ^ self.irq_xor,
                // Set XOR value.
                0xC006 => self.irq_xor = value,
                // Unknown mode configuration ($C007).
                0xC007 => self.irq_funky_reg = value,
                // Mode select.
                0xD000 => {
                    self.prg_mode = value & 0x07;
                    self.chr_mode = (value >> 3) & 0x03;
                    self.advanced_nt_control = (value & 0x20) != 0;
                    self.nt_global = (value & 0x40) != 0;
                    self.enable_prg_at_6000 = (value & 0x80) != 0;
                }
                // Mirroring select.
                0xD001 => {
                    self.mirroring_reg = value & 0x03;
                    self.extended_mirroring = (value & 0x08) != 0;
                }
                // PPU address-space config.
                0xD002 => self.nt_ram_select_bit = value & 0x80,
                // Outer bank select / CHR block / MMC4 mode.
                0xD003 => {
                    self.mirror_chr = (value & 0x80) != 0;
                    self.chr_block_mode = (value & 0x20) == 0;
                    self.chr_block = ((value & 0x18) >> 2) | (value & 0x01);
                }
                _ => {}
            }
        }
        // A CPU write also clocks the IRQ when the CPU-write source is active.
        if self.irq_source == IrqSource::CpuWrite {
            self.tick_irq();
        }
    }

    fn ppu_read(&mut self, addr: u16) -> u8 {
        // The bus masks pattern-table reads to `$0000-$1FFF` before they reach
        // here; nametable bytes are routed through `nametable_fetch` /
        // `nametable_address` instead (the PPU owns CIRAM), so this only
        // services CHR.
        let addr = addr & 0x1FFF;
        // PPU-read IRQ source ticks on render reads (pattern fetches).
        if self.irq_source == IrqSource::PpuRead {
            self.tick_irq();
        }
        self.update_chr_latch_209(addr);
        let off = self.chr_offset(addr);
        self.chr[off % self.chr.len()]
    }

    fn ppu_write(&mut self, addr: u16, value: u8) {
        // Pattern-table writes only (CHR-RAM). Nametable writes arrive via
        // `nametable_write`; the bus never routes `$2000+` to `ppu_write`.
        let addr = addr & 0x1FFF;
        if self.chr_is_ram {
            let off = self.chr_offset(addr);
            let len = self.chr.len();
            self.chr[off % len] = value;
        }
    }

    fn nametable_fetch(&mut self, addr: u16) -> Option<u8> {
        // Serve a ROM (CHR) nametable byte when ROM nametables are active for
        // this table; otherwise return `None` so the PPU reads CIRAM (banked
        // via `nametable_address`).
        let masked = addr & 0x2FFF;
        let nt_index = ((masked - 0x2000) / NAMETABLE_SIZE_U16) as usize & 0x03;
        if self.nt_control_active() && self.nt_index_is_rom(nt_index) {
            Some(self.nt_rom_byte(masked))
        } else {
            None
        }
    }

    fn nametable_write(&mut self, addr: u16, _value: u8) -> bool {
        // ROM nametables are not writable: absorb (drop) the write so the PPU
        // does not fall through to CIRAM. CIRAM-backed tables return `false`
        // so the PPU performs its normal banked write.
        let masked = addr & 0x2FFF;
        let nt_index = ((masked - 0x2000) / NAMETABLE_SIZE_U16) as usize & 0x03;
        self.nt_control_active() && self.nt_index_is_rom(nt_index)
    }

    fn notify_a12(&mut self, level: bool) {
        // PPU A12-rise IRQ source: tick on the rising edge.
        if self.irq_source == IrqSource::PpuA12Rise && level && (self.last_ppu_addr & 0x1000) == 0 {
            self.tick_irq();
        }
        self.last_ppu_addr = if level { 0x1000 } else { 0x0000 };
    }

    fn notify_cpu_cycle(&mut self) {
        if self.irq_source == IrqSource::CpuClock {
            self.tick_irq();
        }
    }

    fn irq_pending(&self) -> bool {
        self.irq_pending
    }

    fn current_mirroring(&self) -> Mirroring {
        // Mapper-controlled: the PPU defers to `nametable_address`, which uses
        // the live `$D001` register. Report the closest enum for the UI.
        match self.mirroring_reg & 0x03 {
            0 => Mirroring::Vertical,
            1 => Mirroring::Horizontal,
            2 => Mirroring::SingleScreenA,
            _ => Mirroring::SingleScreenB,
        }
    }

    fn nametable_address(&self, addr: u16) -> u16 {
        (self.nametable_offset(addr) & 0x07FF) as u16
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: self.board.mapper_id(),
            name: match self.board {
                JyBoard::M90 => "J.Y. Company ASIC (90)".into(),
                JyBoard::M209 => "J.Y. Company ASIC (209)".into(),
                JyBoard::M211 => "J.Y. Company ASIC (211)".into(),
            },
            mirroring: crate::mapper::mirroring_name(self.current_mirroring()),
            ..Default::default()
        };
        for (i, b) in self.prg_regs.iter().enumerate() {
            info.prg_banks.push((format!("P{i}"), format!("{b:#04x}")));
        }
        info.prg_banks
            .push(("mode".into(), format!("{:#04x}", self.prg_mode)));
        for i in 0..8 {
            info.chr_banks
                .push((format!("C{i}"), format!("{:#06x}", self.chr_reg(i))));
        }
        info.chr_banks
            .push(("mode".into(), format!("{}", self.chr_mode)));
        info.irq_state
            .push(("source".into(), format!("{:?}", self.irq_source)));
        info.irq_state
            .push(("prescaler".into(), format!("{:#04x}", self.irq_prescaler)));
        info.irq_state
            .push(("counter".into(), format!("{:#04x}", self.irq_counter)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending)));
        info.extra
            .push(("ntROM".into(), format!("{}", self.nt_control_active())));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64 + if self.chr_is_ram { self.chr.len() } else { 0 });
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.prg_regs);
        out.extend_from_slice(&self.chr_low_regs);
        out.extend_from_slice(&self.chr_high_regs);
        out.extend_from_slice(&self.nt_low_regs);
        out.extend_from_slice(&self.nt_high_regs);
        out.extend_from_slice(&self.chr_latch);
        out.push(self.prg_mode);
        out.push(u8::from(self.enable_prg_at_6000));
        out.push(self.chr_mode);
        out.push(u8::from(self.advanced_nt_control));
        out.push(u8::from(self.nt_global));
        out.push(self.mirroring_reg);
        out.push(u8::from(self.extended_mirroring));
        out.push(self.nt_ram_select_bit);
        out.push(u8::from(self.chr_block_mode));
        out.push(self.chr_block);
        out.push(u8::from(self.mirror_chr));
        out.push(u8::from(self.irq_enabled));
        out.push(self.irq_source.to_bits());
        out.push(self.irq_count_direction);
        out.push(u8::from(self.irq_small_prescaler));
        out.push(self.irq_prescaler);
        out.push(self.irq_counter);
        out.push(self.irq_xor);
        out.push(self.irq_funky_reg);
        out.push(u8::from(self.irq_pending));
        out.push(self.mul1);
        out.push(self.mul2);
        out.push(self.test_reg);
        out.extend_from_slice(&self.last_ppu_addr.to_le_bytes());
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_part = if self.chr_is_ram { self.chr.len() } else { 0 };
        // 1 (ver) + 4 + 8 + 8 + 4 + 4 + 2 (latch) + scalars(23) + 2 (last addr).
        // The 23 scalars: 11 mode/mirroring flags + 9 IRQ fields + 3 misc regs.
        let scalar_len = 1 + 4 + 8 + 8 + 4 + 4 + 2 + 23 + 2;
        let expected = scalar_len + chr_part;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        if data[0] != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(data[0]));
        }
        let mut c = 1usize;
        let mut take = |n: usize| {
            let s = &data[c..c + n];
            c += n;
            s
        };
        self.prg_regs.copy_from_slice(take(4));
        self.chr_low_regs.copy_from_slice(take(8));
        self.chr_high_regs.copy_from_slice(take(8));
        self.nt_low_regs.copy_from_slice(take(4));
        self.nt_high_regs.copy_from_slice(take(4));
        self.chr_latch.copy_from_slice(take(2));
        // The CHR latch indexes the 8-entry `chr_low_regs`/`chr_high_regs`
        // groups (via `chr_reg`) in CHR mode 1, so a corrupted/hand-edited
        // save-state must not be able to push it out of range. In normal
        // operation the latch is only ever {0,2,4,6} (init {0,4}), all < 8.
        self.chr_latch[0] &= 0x07;
        self.chr_latch[1] &= 0x07;
        self.prg_mode = take(1)[0];
        self.enable_prg_at_6000 = take(1)[0] != 0;
        self.chr_mode = take(1)[0];
        self.advanced_nt_control = take(1)[0] != 0;
        self.nt_global = take(1)[0] != 0;
        self.mirroring_reg = take(1)[0];
        self.extended_mirroring = take(1)[0] != 0;
        self.nt_ram_select_bit = take(1)[0];
        self.chr_block_mode = take(1)[0] != 0;
        self.chr_block = take(1)[0];
        self.mirror_chr = take(1)[0] != 0;
        self.irq_enabled = take(1)[0] != 0;
        self.irq_source = IrqSource::from_bits(take(1)[0]);
        self.irq_count_direction = take(1)[0];
        self.irq_small_prescaler = take(1)[0] != 0;
        self.irq_prescaler = take(1)[0];
        self.irq_counter = take(1)[0];
        self.irq_xor = take(1)[0];
        self.irq_funky_reg = take(1)[0];
        self.irq_pending = take(1)[0] != 0;
        self.mul1 = take(1)[0];
        self.mul2 = take(1)[0];
        self.test_reg = take(1)[0];
        let la = take(2);
        self.last_ppu_addr = u16::from_le_bytes([la[0], la[1]]);
        if self.chr_is_ram {
            let n = self.chr.len();
            self.chr.copy_from_slice(take(n));
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

    fn synth_chr(banks_1k: usize) -> Box<[u8]> {
        let mut v = vec![0u8; banks_1k * CHR_BANK_1K];
        for b in 0..banks_1k {
            v[b * CHR_BANK_1K] = b as u8;
        }
        v.into_boxed_slice()
    }

    fn fresh(board: JyBoard) -> JyAsic {
        // 64 * 8 KiB = 512 KiB PRG so $3F resolves to a distinct bank;
        // 256 * 1 KiB = 256 KiB CHR.
        JyAsic::new(synth_prg(64), synth_chr(256), Mirroring::Vertical, board).unwrap()
    }

    #[test]
    fn prg_mode2_8k_banks_select_each_window() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x02); // PRG mode 2 (8 KiB), last bank fixed.
        m.cpu_write(0x8000, 5);
        m.cpu_write(0x8001, 6);
        m.cpu_write(0x8002, 7);
        assert_eq!(m.cpu_read(0x8000), 5);
        assert_eq!(m.cpu_read(0xA000), 6);
        assert_eq!(m.cpu_read(0xC000), 7);
        // Last window fixed to $3F when $D000 bit 2 clear.
        assert_eq!(m.cpu_read(0xE000), 0x3F);
    }

    #[test]
    fn prg_mode2_last_bank_switchable() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x06); // mode 2 + switchable last bank.
        m.cpu_write(0x8003, 9);
        assert_eq!(m.cpu_read(0xE000), 9);
    }

    #[test]
    fn prg_mode0_32k_uses_fixed_3c_window() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x00); // 32 KiB, hard-wired to $3C..$3F.
        assert_eq!(m.cpu_read(0x8000), 0x3C);
        assert_eq!(m.cpu_read(0xA000), 0x3D);
        assert_eq!(m.cpu_read(0xC000), 0x3E);
        assert_eq!(m.cpu_read(0xE000), 0x3F);
    }

    #[test]
    fn prg_mode1_16k_low_and_high_windows() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x01); // 16 KiB, high fixed to $3E/$3F.
        m.cpu_write(0x8001, 2); // low 16 KiB = banks (2<<1) = 4,5.
        assert_eq!(m.cpu_read(0x8000), 4);
        assert_eq!(m.cpu_read(0xA000), 5);
        assert_eq!(m.cpu_read(0xC000), 0x3E);
        assert_eq!(m.cpu_read(0xE000), 0x3F);
    }

    #[test]
    fn prg_bits_reversed_in_mode3() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x03); // mode 3 = mode 2 with reversed bank bits.
        // reg value 0x01 -> reversed -> 0x40.
        m.cpu_write(0x8000, 0x01);
        assert_eq!(m.cpu_read(0x8000), 0x40 % 64);
    }

    #[test]
    fn prg_mode3_bit3_matches_mesen2() {
        // Verify the Mesen2-matching reversal (bits 0<->6, 1<->5, 2<->4) and
        // pin the documented bit-3 behaviour: Mesen2's `InvertPrgBits` drops
        // bit 3 (0x08) rather than carrying it through the centre. A bare
        // 0x08 input therefore reverses to 0x00.
        assert_eq!(JyAsic::invert_prg_bits(0x08, true), 0x00);
        // Bit pairs are swapped as documented.
        assert_eq!(JyAsic::invert_prg_bits(0x01, true), 0x40); // bit0 -> bit6
        assert_eq!(JyAsic::invert_prg_bits(0x40, true), 0x01); // bit6 -> bit0
        assert_eq!(JyAsic::invert_prg_bits(0x02, true), 0x20); // bit1 -> bit5
        assert_eq!(JyAsic::invert_prg_bits(0x04, true), 0x10); // bit2 -> bit4
        // A value with bit 3 set alongside others keeps the reversed pairs but
        // still discards bit 3 (0x09 = bit0|bit3 -> 0x40, the bit-3 part lost).
        assert_eq!(JyAsic::invert_prg_bits(0x09, true), 0x40);
        // No-invert is the identity.
        assert_eq!(JyAsic::invert_prg_bits(0x7F, false), 0x7F);
    }

    #[test]
    fn prg_register_decode_ignores_a2_accepts_a11() {
        // Mesen2 decodes $8000-$FFFF with `addr & 0xF007`: A2 is ignored (so
        // $8004 aliases $8000) and A11 is NOT masked (so $8800 is still a
        // register write, unlike the wiki's stricter $F803).
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x02); // PRG mode 2 (8 KiB windows).
        m.cpu_write(0x8004, 5); // A2 set -> aliases $8000.
        assert_eq!(m.cpu_read(0x8000), 5, "$8004 must alias the $8000 register");
        // A11 set ($8800) also decodes to the $8000 register under $F007.
        m.cpu_write(0x8800, 9);
        assert_eq!(
            m.cpu_read(0x8000),
            9,
            "$8800 must still write the $8000 reg"
        );
    }

    #[test]
    fn prg_at_6000_when_enabled() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x82); // mode 2 + enable PRG @ $6000.
        m.cpu_write(0x8003, 4);
        assert!(!m.cpu_read_unmapped(0x6000));
        assert_eq!(m.cpu_read(0x6000), 4);
    }

    #[test]
    fn chr_8k_mode() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x00); // CHR mode 0 (8 KiB).
        m.cpu_write(0x9000, 3); // reg0 low = 3 -> 8 KiB window from 1k bank 24.
        assert_eq!(m.ppu_read(0x0000), 24);
        assert_eq!(m.ppu_read(0x0400), 25);
    }

    #[test]
    fn chr_1k_mode_each_slot() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x18); // CHR mode 3 (1 KiB).
        m.cpu_write(0x9000, 10);
        m.cpu_write(0x9007, 20);
        assert_eq!(m.ppu_read(0x0000), 10);
        assert_eq!(m.ppu_read(0x1C00), 20);
    }

    #[test]
    fn chr_high_byte_extends_bank() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x18); // 1 KiB.
        m.cpu_write(0x9000, 0x05);
        m.cpu_write(0xA000, 0x01); // high byte -> bank 0x105 = 261, masked to 256.
        assert_eq!(m.ppu_read(0x0000), (0x105 % 256) as u8);
    }

    #[test]
    fn chr_block_mode_outer_bank() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x18); // 1 KiB.
        // $D003: chr_block_mode is ON when bit 5 == 0; set chr_block bits.
        m.cpu_write(0xD003, 0x01); // chr_block = ((0&0x18)>>2)|1 = 1.
        m.cpu_write(0x9000, 0x00);
        // mode 3 -> mask 0xFF, shift 8 -> bank = 0 | (1<<8) = 256, masked -> 0.
        assert_eq!(m.ppu_read(0x0000), 0);
    }

    #[test]
    fn mirroring_register_select() {
        let mut m = fresh(JyBoard::M90);
        m.cpu_write(0xD001, 0x00);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        m.cpu_write(0xD001, 0x01);
        assert_eq!(m.current_mirroring(), Mirroring::Horizontal);
        m.cpu_write(0xD001, 0x02);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenA);
        m.cpu_write(0xD001, 0x03);
        assert_eq!(m.current_mirroring(), Mirroring::SingleScreenB);
    }

    #[test]
    fn mapper90_inhibits_rom_nametables() {
        let mut m = fresh(JyBoard::M90);
        // Enable ROM nametables globally via $D000; mapper 90 ignores it.
        m.cpu_write(0xD000, 0x60); // bit 5 (NT ROM) + bit 6 (global).
        assert!(!m.nt_control_active());
    }

    #[test]
    fn mapper211_forces_rom_nametables() {
        let m = fresh(JyBoard::M211);
        // No register write needed; 211 always has the feature on.
        assert!(m.nt_control_active());
    }

    #[test]
    fn mapper209_rom_nametable_global_reads_chr() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x60); // NT ROM + global.
        m.cpu_write(0xB000, 7); // NT0 -> CHR 1k page 7.
        // ROM nametables are served via `nametable_fetch` (the PPU's
        // nametable path), NOT `ppu_read` (which only handles $0000-$1FFF).
        // Page 7 in synth CHR has its first byte == 7.
        assert_eq!(m.nametable_fetch(0x2000), Some(7));
    }

    #[test]
    fn mapper209_ciram_nametable_returns_none() {
        // With ROM nametables off, `nametable_fetch` must decline so the PPU
        // reads its own CIRAM (banked via `nametable_address`).
        let mut m = fresh(JyBoard::M209);
        assert_eq!(m.nametable_fetch(0x2000), None);
    }

    #[test]
    fn extended_mirroring_selects_per_nt_ciram_page() {
        // $D001 bit 3 enables Extended Mirroring: each nametable's CIRAM page
        // comes from $B00x bit 0. Verify `nametable_address` honours it (it
        // previously always used the MM register -> the field was inert).
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD001, 0x08); // Extended Mirroring on, MM bits = 0.
        assert!(m.nt_control_active());
        // All four B-regs bit 0 = 0 -> every table maps to CIRAM page 0.
        for table in 0..4u16 {
            let addr = 0x2000 + table * 0x400;
            assert_eq!(
                m.nametable_address(addr) & 0x0400,
                0,
                "table {table} page 0"
            );
        }
        // Set $B001 / $B003 bit 0 -> tables 1 and 3 move to CIRAM page 1.
        m.cpu_write(0xB001, 0x01);
        m.cpu_write(0xB003, 0x01);
        assert_eq!(m.nametable_address(0x2000) & 0x0400, 0x000); // NT0 -> page 0
        assert_eq!(m.nametable_address(0x2400) & 0x0400, 0x400); // NT1 -> page 1
        assert_eq!(m.nametable_address(0x2800) & 0x0400, 0x000); // NT2 -> page 0
        assert_eq!(m.nametable_address(0x2C00) & 0x0400, 0x400); // NT3 -> page 1
    }

    #[test]
    fn extended_mirroring_off_uses_mm_register() {
        // With Extended Mirroring / advanced-NT control off, the MM register
        // drives CIRAM mapping (vertical here): NT0/NT2 -> page 0/0... actually
        // vertical maps tables 0,2 -> A and 1,3 -> B.
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD001, 0x00); // MM = 0 (vertical), Extended Mirroring off.
        assert!(!m.nt_control_active());
        assert_eq!(m.nametable_address(0x2000) & 0x0400, 0x000); // table 0 -> A
        assert_eq!(m.nametable_address(0x2400) & 0x0400, 0x400); // table 1 -> B
        assert_eq!(m.nametable_address(0x2800) & 0x0400, 0x000); // table 2 -> A
        assert_eq!(m.nametable_address(0x2C00) & 0x0400, 0x400); // table 3 -> B
    }

    #[test]
    fn rom_nametable_write_is_absorbed() {
        // A write to a ROM nametable must be absorbed (drop), so the PPU does
        // not also touch CIRAM; CIRAM-backed tables decline the write.
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x60); // ROM nametables, global.
        assert!(m.nametable_write(0x2000, 0x42)); // ROM NT -> absorbed.
        let mut m2 = fresh(JyBoard::M209); // no ROM nametables.
        assert!(!m2.nametable_write(0x2000, 0x42)); // CIRAM NT -> PPU handles.
    }

    #[test]
    fn irq_cpu_clock_increment_wraps_to_assert() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xC006, 0x00); // XOR = 0.
        m.cpu_write(0xC005, 0xFF); // counter = 0xFF.
        m.cpu_write(0xC004, 0xFF); // prescaler = 0xFF (small mask off -> $FF).
        // $C001: direction increment (bit 6 set), source CPU clock (bits 0-1=0),
        // small prescaler off.
        m.cpu_write(0xC001, 0x40);
        m.cpu_write(0xC000, 0x01); // enable.
        // First tick: prescaler 0xFF -> 0x00 wraps, counter 0xFF -> 0x00 wraps.
        m.notify_cpu_cycle();
        assert!(m.irq_pending());
    }

    #[test]
    fn irq_decrement_underflow_asserts() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xC006, 0x00);
        m.cpu_write(0xC005, 0x00); // counter = 0.
        m.cpu_write(0xC004, 0x00); // prescaler = 0.
        m.cpu_write(0xC001, 0x80); // direction decrement (bits 6-7 = 0b10).
        m.cpu_write(0xC000, 0x01); // enable.
        // prescaler 0 -> 0xFF (mask wrap) clocks counter 0 -> 0xFF -> assert.
        m.notify_cpu_cycle();
        assert!(m.irq_pending());
    }

    #[test]
    fn irq_small_prescaler_mask() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xC006, 0x00);
        m.cpu_write(0xC005, 0xFF); // counter at top.
        m.cpu_write(0xC004, 0x07); // prescaler low 3 bits = 7.
        // small prescaler (bit 2) + increment direction + CPU clock source.
        m.cpu_write(0xC001, 0x44);
        m.cpu_write(0xC000, 0x01);
        m.notify_cpu_cycle(); // prescaler 7 -> 0 wraps (mask 0x07), counter wraps.
        assert!(m.irq_pending());
    }

    #[test]
    fn irq_xor_applied_to_counter_and_prescaler() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xC006, 0x0F); // XOR.
        m.cpu_write(0xC005, 0xF0); // counter = 0xF0 ^ 0x0F = 0xFF.
        m.cpu_write(0xC001, 0x40); // increment, CPU clock.
        m.cpu_write(0xC004, 0xF0); // prescaler = 0xF0 ^ 0x0F = 0xFF.
        m.cpu_write(0xC000, 0x01);
        m.notify_cpu_cycle();
        assert!(m.irq_pending());
    }

    #[test]
    fn irq_disable_acknowledges_and_resets_prescaler() {
        let mut m = fresh(JyBoard::M209);
        m.irq_pending = true;
        m.irq_prescaler = 0x42;
        m.cpu_write(0xC000, 0x00); // disable -> ack + reset prescaler.
        assert!(!m.irq_pending());
        assert_eq!(m.irq_prescaler, 0);
        m.irq_pending = true;
        m.cpu_write(0xC002, 0x00); // explicit disable.
        assert!(!m.irq_pending());
    }

    #[test]
    fn irq_source_cpu_write_ticks_on_write() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xC006, 0x00);
        // source = CPU writes (bits 0-1 = 3), increment direction. Every CPU
        // write (including register writes) clocks the prescaler while the
        // CPU-write source is active; capture the prescaler immediately before
        // a plain data write and confirm that write advances it by one.
        m.cpu_write(0xC001, 0x43);
        m.cpu_write(0xC000, 0x01); // enable.
        let before = m.irq_prescaler;
        m.cpu_write(0x5803, 0x00); // a data write -> one prescaler increment.
        assert_eq!(m.irq_prescaler, before.wrapping_add(1));
    }

    #[test]
    fn irq_source_cpu_clock_does_not_tick_on_write() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xC006, 0x00);
        m.cpu_write(0xC001, 0x40); // increment, source = CPU clock (not write).
        m.cpu_write(0xC000, 0x01);
        let before = m.irq_prescaler;
        m.cpu_write(0x5803, 0x00); // a data write must NOT tick (wrong source).
        assert_eq!(m.irq_prescaler, before);
    }

    #[test]
    fn irq_a12_rise_source_ticks_on_rising_edge() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xC006, 0x00);
        m.cpu_write(0xC005, 0xFF);
        m.cpu_write(0xC004, 0xFF);
        m.cpu_write(0xC001, 0x41); // increment, source A12 rise.
        m.cpu_write(0xC000, 0x01);
        m.notify_a12(false); // low first.
        assert!(!m.irq_pending());
        m.notify_a12(true); // rising edge -> tick -> assert.
        assert!(m.irq_pending());
    }

    #[test]
    fn mapper209_mmc4_latch_switches_chr() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0xD000, 0x08); // CHR mode 1 (4 KiB).
        // A fetch at $0FE8 should set window-0 latch.
        m.ppu_read(0x0FE8);
        // chr_latch[0] = (0x0FE8>>4) & ((0x0FE8>>10 & 4)|2)
        //             = 0xFE & ((3 & 4)|2) = 0xFE & 2 = 2.
        assert_eq!(m.chr_latch[0], 2);
    }

    #[test]
    fn mapper90_no_mmc4_latch() {
        let mut m = fresh(JyBoard::M90);
        let before = m.chr_latch;
        m.ppu_read(0x0FE8);
        assert_eq!(m.chr_latch, before); // mapper 90 doesn't auto-latch.
    }

    #[test]
    fn multiplier_register() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0x5800, 6);
        m.cpu_write(0x5801, 7);
        assert_eq!(m.cpu_read(0x5800), 42); // 6 * 7 = 42, LSB.
        assert_eq!(m.cpu_read(0x5801), 0); // MSB.
        m.cpu_write(0x5800, 0xFF);
        m.cpu_write(0x5801, 0xFF);
        assert_eq!(m.cpu_read(0x5800), 0x01); // 0xFE01 LSB.
        assert_eq!(m.cpu_read(0x5801), 0xFE); // MSB.
    }

    #[test]
    fn test_register_read_write() {
        let mut m = fresh(JyBoard::M209);
        m.cpu_write(0x5803, 0x5A);
        assert_eq!(m.cpu_read(0x5803), 0x5A);
    }

    #[test]
    fn save_state_round_trip() {
        for board in [JyBoard::M90, JyBoard::M209, JyBoard::M211] {
            let mut m = fresh(board);
            m.cpu_write(0xD000, 0x9A); // mode bits + NT-ROM + PRG@6000.
            m.cpu_write(0x8000, 3);
            m.cpu_write(0x8003, 9);
            m.cpu_write(0x9000, 0x11);
            m.cpu_write(0xA000, 0x01);
            m.cpu_write(0xB000, 4);
            m.cpu_write(0xB004, 1);
            m.cpu_write(0xD001, 0x09); // extended mirroring + MM.
            m.cpu_write(0xD003, 0xA1);
            m.cpu_write(0xC006, 0x0F);
            m.cpu_write(0xC005, 0x12);
            m.cpu_write(0xC004, 0x34);
            m.cpu_write(0xC001, 0x41);
            m.cpu_write(0xC000, 0x01);
            m.ppu_read(0x0FE8); // move the latch.

            let blob = m.save_state();
            let mut m2 = fresh(board);
            m2.load_state(&blob).unwrap();

            assert_eq!(m.cpu_read(0x8000), m2.cpu_read(0x8000));
            assert_eq!(m.cpu_read(0xE000), m2.cpu_read(0xE000));
            assert_eq!(m.ppu_read(0x0000), m2.ppu_read(0x0000));
            assert_eq!(m.chr_latch, m2.chr_latch);
            assert_eq!(m.irq_counter, m2.irq_counter);
            assert_eq!(m.irq_prescaler, m2.irq_prescaler);
            assert_eq!(m.current_mirroring(), m2.current_mirroring());
            assert_eq!(m.test_reg, m2.test_reg);
        }
    }

    #[test]
    fn save_state_rejects_bad_version() {
        let mut m = fresh(JyBoard::M209);
        let mut blob = m.save_state();
        blob[0] = 0xFF;
        assert!(matches!(
            m.load_state(&blob),
            Err(MapperError::UnsupportedVersion(0xFF))
        ));
    }

    #[test]
    fn save_state_rejects_truncated() {
        let mut m = fresh(JyBoard::M209);
        let blob = m.save_state();
        assert!(matches!(
            m.load_state(&blob[..blob.len() - 1]),
            Err(MapperError::Truncated { .. })
        ));
    }

    #[test]
    fn load_state_clamps_chr_latch() {
        // A corrupted/hand-edited save-state must not be able to push the CHR
        // latch past 7 (it indexes the 8-entry CHR register groups in CHR mode
        // 1). Inject out-of-range latch bytes and confirm `load_state` masks
        // them to 0..=7 so a subsequent CHR fetch cannot panic.
        let mut m = fresh(JyBoard::M209);
        let mut blob = m.save_state();
        // Latch bytes sit right after prg(4)+chrLow(8)+chrHigh(8)+ntLow(4)
        // +ntHigh(4) = 28 scalars past the 1-byte version header.
        let latch_off = 1 + 4 + 8 + 8 + 4 + 4;
        blob[latch_off] = 0xFF;
        blob[latch_off + 1] = 0xFE;
        m.load_state(&blob).unwrap();
        assert!(m.chr_latch[0] < 8);
        assert!(m.chr_latch[1] < 8);
        assert_eq!(m.chr_latch[0], 0x07);
        assert_eq!(m.chr_latch[1], 0x06);
        // CHR mode 1 fetch through the (now-clamped) latch must not panic.
        m.cpu_write(0xD000, 0x08); // CHR mode 1 (4 KiB, MMC4 latch path).
        let _ = m.ppu_read(0x0000);
        let _ = m.ppu_read(0x1000);
    }
}
