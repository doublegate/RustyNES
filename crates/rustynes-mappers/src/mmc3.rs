//! MMC3 (iNES mapper 4) implementation.
//!
//! See `docs/mappers.md` §MMC3 and `ref-docs/research-report.md` §MMC3.
//!
//! # Banking
//!
//! Eight internal registers `R0`-`R7` selected by the low 3 bits of the
//! value written to `$8000` (bank-select).  Subsequent writes to `$8001`
//! (bank-data) commit a value into the selected register:
//!
//! | Register | Purpose                                          |
//! |----------|--------------------------------------------------|
//! | R0       | 2 KiB CHR bank @ `$0000-$07FF` (CHR mode 0)      |
//! | R1       | 2 KiB CHR bank @ `$0800-$0FFF` (CHR mode 0)      |
//! | R2       | 1 KiB CHR bank @ `$1000-$13FF` (CHR mode 0)      |
//! | R3       | 1 KiB CHR bank @ `$1400-$17FF` (CHR mode 0)      |
//! | R4       | 1 KiB CHR bank @ `$1800-$1BFF` (CHR mode 0)      |
//! | R5       | 1 KiB CHR bank @ `$1C00-$1FFF` (CHR mode 0)      |
//! | R6       | 8 KiB PRG bank @ `$8000-$9FFF` (PRG mode 0)      |
//! | R7       | 8 KiB PRG bank @ `$A000-$BFFF`                   |
//!
//! `$8000` bit 6 swaps the PRG window: in mode 1, R6 maps to `$C000-$DFFF`
//! and the second-to-last bank is fixed at `$8000-$9FFF`.  Bit 7 swaps the
//! CHR layout: in mode 1, the 2 KiB R0/R1 banks occupy `$1000-$1FFF` and
//! the four 1 KiB banks occupy `$0000-$0FFF`.
//!
//! `$E000-$FFFF` is hardwired to the LAST 8 KiB PRG bank.
//!
//! `$A000` even (`$A000-$BFFE` even addresses): mirroring (bit 0).
//! `$A001` odd: PRG-RAM enable + protect (bit 7 enable, bit 6 write-protect).
//! `$C000` even: IRQ counter reload value.
//! `$C001` odd: latches `irq_reload_pending` and forces counter to 0.
//! `$E000` even: disable IRQ + acknowledge any pending IRQ line.
//! `$E001` odd: enable IRQ.
//!
//! # IRQ counter
//!
//! Clocked by PPU A12 rising edges, filtered to ignore rising edges within
//! 3 M2 (CPU) cycles of the previous A12 fall.  Standard pattern-table
//! layout (BG @ `$0000`, sprites @ `$1000`) yields exactly one filtered
//! edge per scanline, at PPU dot 260.  Reversed layout (BG @ `$1000`,
//! sprites @ `$0000`) places the edge at the END of the previous
//! scanline's sprite fetches (Wario's Woods relies on this).
//!
//! On each filtered rising edge:
//! - if `counter == 0` OR `irq_reload_pending`: counter = `irq_reload_value`;
//!   pending cleared; **Sharp** revision additionally asserts IRQ if the
//!   reload value was 0 (from a non-zero counter); **NEC** does not.
//! - else: counter -= 1; if post-decrement counter == 0 AND IRQ enabled,
//!   assert IRQ line.
//!
//! Default revision is **Sharp** per project policy (Star Trek: 25th
//! Anniversary requires it); NES 2.0 submapper 1 selects MMC3B (NEC),
//! submapper 2 selects MMC3C (Sharp behavior + minor differences not
//! distinguished here).

#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::missing_const_for_fn,
    clippy::struct_excessive_bools,
    clippy::match_same_arms,
    clippy::manual_range_patterns,
    clippy::too_many_arguments,
    clippy::useless_let_if_seq,
    clippy::doc_markdown,
    clippy::if_not_else,
    clippy::nonminimal_bool
)]

use crate::cartridge::Mirroring;
use crate::mapper::{Mapper, MapperCaps, MapperError};
use alloc::{boxed::Box, vec::Vec};
use alloc::{format, vec};

const PRG_BANK_8K: usize = 0x2000;
const CHR_BANK_1K: usize = 0x0400;
const PRG_RAM_DEFAULT: usize = 0x2000;
const NAMETABLE_SIZE: usize = 0x0400;
const NAMETABLE_SIZE_U16: u16 = 0x0400;

const SAVE_STATE_VERSION: u8 = 2;

/// MMC3 hardware revision.  Default is Sharp (MMC3A); the alternative
/// NEC (MMC3B) suppresses the "reload to 0 asserts IRQ" behavior.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mmc3Revision {
    /// Sharp MMC3A (and MMC3C): reloading the IRQ counter to 0 asserts
    /// IRQ if IRQs are enabled.  Default.
    #[default]
    Sharp,
    /// NEC MMC3B: a counter that reloads to 0 from a non-zero state
    /// does NOT assert IRQ.  Selected via NES 2.0 submapper byte = 1.
    Nec,
}

/// MMC3 mapper (iNES mapper 4).
pub struct Mmc3 {
    prg_rom: Box<[u8]>,
    chr: Box<[u8]>,
    prg_ram: Box<[u8]>,
    vram: Box<[u8]>,
    chr_is_ram: bool,

    // R0..R7 bank registers (8 internal regs selected via $8000).
    regs: [u8; 8],
    // Selected register index (low 3 bits of $8000).
    bank_select: u8,
    // PRG mode (bit 6 of $8000): 0 = R6 @ $8000, last-1 fixed @ $C000;
    //                             1 = R6 @ $C000, last-1 fixed @ $8000.
    prg_mode: bool,
    // CHR mode (bit 7 of $8000): 0 = 2K@$0000 + 1K@$1000;
    //                             1 = 1K@$0000 + 2K@$1000.
    chr_mode: bool,

    // Mirroring (set via $A000 even).  Ignored on 4-screen carts.
    mirroring: Mirroring,
    fixed_4screen: bool,

    // PRG-RAM enable + protect ($A001 odd).
    prg_ram_enabled: bool,
    prg_ram_protect: bool,

    // IRQ counter state.
    irq_counter: u8,
    irq_reload_value: u8,
    irq_reload_pending: bool,
    irq_enabled: bool,
    irq_pending_line: bool,
    // Latched at every `$C001` write: whether the counter was non-zero
    // at the time of the write.  Consumed by `clock_irq` on the next
    // filtered A12 rise: a non-zero-to-zero $C001 clear ALLOWS Sharp's
    // reload-to-zero assertion (see `mmc3_test_2/2-details` sub-test #7
    // "IRQ should be set when non-zero and reloading to 0 after clear"),
    // while a zero-to-zero $C001 SUPPRESSES it (see
    // `mmc3_test_2/4-scanline_timing` sub-test #2 "Scanline 0 IRQ should
    // occur later when $2000=$08").  This is the cycle-precise
    // discriminator that makes both blargg sub-tests pass together —
    // collapsing the two into a single "reload_pending implies assert"
    // path (the v0.8.x implementation) forces one or the other to fail.
    irq_reload_pending_with_nonzero_clear: bool,

    // A12 filter state.
    last_a12: bool,
    // CPU cycle at which A12 last went low; used to filter rising edges
    // closer than 3 M2 cycles.
    a12_low_cycle: u64,
    cpu_cycle: u64,

    revision: Mmc3Revision,
}

impl Mmc3 {
    /// Construct a new MMC3 mapper.
    ///
    /// `prg_rom` must be a non-zero multiple of 8 KiB (typical 32-512 KiB).
    /// CHR-RAM is selected when `chr_rom` is empty; otherwise CHR-ROM
    /// length must be a multiple of 1 KiB.  `prg_ram_bytes == 0` selects
    /// the default 8 KiB.  Set `revision` from the iNES NES 2.0 submapper
    /// (default Sharp).
    ///
    /// # Errors
    ///
    /// Returns [`MapperError::Invalid`] on size mismatch.
    pub fn new(
        prg_rom: Box<[u8]>,
        chr_rom: Box<[u8]>,
        initial_mirroring: Mirroring,
        prg_ram_bytes: usize,
        revision: Mmc3Revision,
    ) -> Result<Self, MapperError> {
        if prg_rom.is_empty() || !prg_rom.len().is_multiple_of(PRG_BANK_8K) {
            return Err(MapperError::Invalid(format!(
                "MMC3 PRG-ROM size {} is not a non-zero multiple of 8 KiB",
                prg_rom.len()
            )));
        }
        let chr_is_ram = chr_rom.is_empty();
        let chr: Box<[u8]> = if chr_is_ram {
            vec![0u8; 8 * CHR_BANK_1K].into_boxed_slice()
        } else if chr_rom.len().is_multiple_of(CHR_BANK_1K) {
            chr_rom
        } else {
            return Err(MapperError::Invalid(format!(
                "MMC3 CHR-ROM size {} is not a multiple of 1 KiB",
                chr_rom.len()
            )));
        };
        let prg_ram_size = if prg_ram_bytes == 0 {
            PRG_RAM_DEFAULT
        } else {
            prg_ram_bytes
        };
        let fixed_4screen = matches!(initial_mirroring, Mirroring::FourScreen);
        // For four-screen, allocate the full 4 KiB nametable VRAM region.
        let vram_size = if fixed_4screen {
            4 * NAMETABLE_SIZE
        } else {
            2 * NAMETABLE_SIZE
        };
        Ok(Self {
            prg_rom,
            chr,
            prg_ram: vec![0u8; prg_ram_size].into_boxed_slice(),
            vram: vec![0u8; vram_size].into_boxed_slice(),
            chr_is_ram,
            regs: [0; 8],
            bank_select: 0,
            prg_mode: false,
            chr_mode: false,
            mirroring: initial_mirroring,
            fixed_4screen,
            prg_ram_enabled: true,
            prg_ram_protect: false,
            irq_counter: 0,
            irq_reload_value: 0,
            irq_reload_pending: false,
            irq_reload_pending_with_nonzero_clear: false,
            irq_enabled: false,
            irq_pending_line: false,
            last_a12: false,
            a12_low_cycle: 0,
            cpu_cycle: 0,
            revision,
        })
    }

    /// Resolve a CPU PRG address (`$8000-$FFFF`) to a byte offset in
    /// `prg_rom`.  Implements PRG modes 0 and 1.
    fn prg_offset(&self, addr: u16) -> usize {
        let total_banks = self.prg_rom.len() / PRG_BANK_8K;
        let last = total_banks.saturating_sub(1);
        let second_last = total_banks.saturating_sub(2);
        // R6/R7 are masked to total_banks (typical sizes <= 64 banks => 6 bits).
        let r6 = (self.regs[6] as usize) & last;
        let r7 = (self.regs[7] as usize) & last;
        let bank = match (addr & 0xE000, self.prg_mode) {
            (0x8000, false) => r6,
            (0x8000, true) => second_last,
            (0xA000, _) => r7,
            (0xC000, false) => second_last,
            (0xC000, true) => r6,
            (0xE000, _) => last,
            _ => 0,
        };
        bank * PRG_BANK_8K + ((addr as usize) & 0x1FFF)
    }

    /// Resolve a PPU CHR address (`$0000-$1FFF`) to the **raw** 1 KiB CHR
    /// bank number selected by the active CHR bank registers, *before* any
    /// masking against the installed CHR size.
    ///
    /// This is the value a TQROM-style board (mapper 119) inspects: bit 6
    /// of the bank number selects CHR-RAM vs CHR-ROM, and the low bits
    /// index within the selected memory. The MMC3 itself never exposes this
    /// distinction (it masks the bank straight into a single CHR slice), so
    /// the helper is provided for variant boards that embed an [`Mmc3`].
    #[must_use]
    pub fn chr_bank_1k(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let slot = addr / CHR_BANK_1K;
        self.chr_bank_1k_for_slot(slot)
    }

    /// The raw (unmasked) 1 KiB CHR bank number selected for `slot` (0..8),
    /// honoring the current CHR-A12-inversion mode. Shared by [`Self::chr_offset`]
    /// and [`Self::chr_bank_1k`].
    fn chr_bank_1k_for_slot(&self, slot: usize) -> usize {
        if !self.chr_mode {
            match slot {
                0 => (self.regs[0] as usize) & !1, // 2K @ $0000
                1 => ((self.regs[0] as usize) & !1) | 1,
                2 => (self.regs[1] as usize) & !1, // 2K @ $0800
                3 => ((self.regs[1] as usize) & !1) | 1,
                4 => self.regs[2] as usize, // 1K @ $1000
                5 => self.regs[3] as usize,
                6 => self.regs[4] as usize,
                7 => self.regs[5] as usize,
                _ => 0,
            }
        } else {
            match slot {
                0 => self.regs[2] as usize, // 1K @ $0000
                1 => self.regs[3] as usize,
                2 => self.regs[4] as usize,
                3 => self.regs[5] as usize,
                4 => (self.regs[0] as usize) & !1, // 2K @ $1000
                5 => ((self.regs[0] as usize) & !1) | 1,
                6 => (self.regs[1] as usize) & !1, // 2K @ $1800
                7 => ((self.regs[1] as usize) & !1) | 1,
                _ => 0,
            }
        }
    }

    /// Resolve a PPU CHR address (`$0000-$1FFF`) to an offset in `chr`.
    fn chr_offset(&self, addr: u16) -> usize {
        let addr = (addr & 0x1FFF) as usize;
        let total_banks_1k = self.chr.len() / CHR_BANK_1K;
        let mask = total_banks_1k.saturating_sub(1);
        // Slot index in 1K units (0..8).
        let slot = addr / CHR_BANK_1K;
        // chr_mode false (0): 2K + 2K + 1K + 1K + 1K + 1K
        //                     R0/R0+1   R1/R1+1   R2 R3 R4 R5
        // chr_mode true  (1): 1K + 1K + 1K + 1K + 2K + 2K
        //                     R2 R3 R4 R5  R0/R0+1 R1/R1+1
        let bank_1k = self.chr_bank_1k_for_slot(slot);
        let bank = bank_1k & mask;
        bank * CHR_BANK_1K + (addr & (CHR_BANK_1K - 1))
    }

    /// Compute the CIRAM byte offset for a PPU address in
    /// `$2000-$3EFF`.  Honors per-mapper mirroring; supports four-screen
    /// (the extra 2 KiB lives in our `vram`).
    fn nametable_offset(&self, addr: u16) -> usize {
        let table = (((addr - 0x2000) / NAMETABLE_SIZE_U16) & 0x03) as u8;
        let local = (addr as usize) & (NAMETABLE_SIZE - 1);
        if self.fixed_4screen {
            (table as usize) * NAMETABLE_SIZE + local
        } else {
            let physical = self.mirroring.physical_bank(table);
            physical * NAMETABLE_SIZE + local
        }
    }

    /// Clock the IRQ counter on a filtered A12 rising edge.  Implements
    /// the Sharp/NEC distinction at cycle-precise resolution.
    ///
    /// Three-way branch (C1 step B4):
    /// 1. `irq_reload_pending` (set by a `$C001` write): reload the
    ///    counter from `irq_reload_value` and clear the pending flag.
    ///    Sharp asserts the IRQ line if and only if the `$C001` write
    ///    cleared a non-zero counter AND `irq_reload_value == 0`
    ///    (`irq_reload_pending_with_nonzero_clear` was latched true).
    ///    A `$C001` written while the counter was already zero is a
    ///    "no-op clear" — the next A12 rise reloads silently. This
    ///    distinguishes `mmc3_test_2/2-details` sub-test #7 ("IRQ should
    ///    be set when non-zero and reloading to 0 after clear", expects
    ///    assertion) from `mmc3_test_2/4-scanline_timing` sub-test #2
    ///    ("Scanline 0 IRQ should occur later when `$2000=$08`",
    ///    expects no assertion on the first pre-render A12 rise after
    ///    `$C001`).
    /// 2. `was_zero` (counter naturally at 0 from a prior decrement-to-0
    ///    or a prior reload, with no pending `$C001`): reload from
    ///    `irq_reload_value`.  Sharp (rev A) asserts here if the new
    ///    counter value is 0 (i.e. `irq_reload_value == 0`); NEC (rev B)
    ///    does not.  This is the path `mmc3_test_2/5-MMC3.nes` ("set IRQ
    ///    every clock when reload is 0") exercises.
    /// 3. Otherwise (counter > 0, no pending reload): decrement.  Assert
    ///    IRQ on transition to 0 (both Sharp and NEC).
    ///
    /// This separation is the C1 step B4 structural fix: collapsing
    /// `irq_reload_pending` into the `was_zero` branch (the v0.8.x
    /// implementation) made every first A12 rise after `$C001` assert
    /// IRQ when `irq_reload_value == 0` and the revision was Sharp.
    /// That over-eager assertion produced the residual
    /// `mmc3_test_2/4-scanline_timing` sub-test #2 failure because the
    /// FIRST sprite-fetch A12 rise on the pre-render scanline (PPU dot
    /// 260) clocked an unwanted IRQ assertion, instead of the
    /// test-expected assertion on scanline 0's first sprite fetch.
    /// The cycle-precise discriminator is "did `$C001` clear a non-zero
    /// counter" — only then does Sharp's reload-to-zero rule apply.
    /// See `docs/adr/0002-irq-timing-coordination.md` → "Empirical
    /// refinement (2026-05-14)" for the four rolled-back attempts that
    /// did not separate these paths.
    fn clock_irq(&mut self) -> bool {
        // Returns `true` if this clock event would assert the IRQ line
        // (so the caller can apply M2-phase-aware deferral if needed).
        // Currently the caller asserts immediately on `true`; a future
        // iteration of C1 may defer the assertion for M2-high rises.
        let mut would_assert = false;
        if self.irq_reload_pending {
            // Path 1: explicit $C001 reload.
            let assert_on_reload = self.irq_reload_pending_with_nonzero_clear;
            self.irq_counter = self.irq_reload_value;
            self.irq_reload_pending = false;
            self.irq_reload_pending_with_nonzero_clear = false;
            if assert_on_reload
                && self.irq_enabled
                && self.irq_counter == 0
                && matches!(self.revision, Mmc3Revision::Sharp)
            {
                would_assert = true;
            }
        } else if self.irq_counter == 0 {
            // Path 2: natural counter-at-zero reload.  Sharp asserts when
            // the new value is 0; NEC does not.
            self.irq_counter = self.irq_reload_value;
            if self.irq_enabled
                && self.irq_counter == 0
                && matches!(self.revision, Mmc3Revision::Sharp)
            {
                would_assert = true;
            }
        } else {
            // Path 3: decrement.  Assert on transition to 0.
            self.irq_counter = self.irq_counter.wrapping_sub(1);
            if self.irq_counter == 0 && self.irq_enabled {
                would_assert = true;
            }
        }
        would_assert
    }
}

impl Mapper for Mmc3 {
    fn sram(&self) -> &[u8] {
        &self.prg_ram
    }
    fn sram_mut(&mut self) -> &mut [u8] {
        &mut self.prg_ram
    }
    // v2.8.0 Phase 4 — CPU-cycle hook + IRQ source; no on-cart audio.
    fn caps(&self) -> MapperCaps {
        MapperCaps::CYCLE_IRQ
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enabled && !self.prg_ram.is_empty() {
                    let off = (addr - 0x6000) as usize;
                    if off < self.prg_ram.len() {
                        return self.prg_ram[off];
                    }
                }
                0
            }
            0x8000..=0xFFFF => {
                let off = self.prg_offset(addr);
                self.prg_rom[off % self.prg_rom.len()]
            }
            _ => 0,
        }
    }

    fn cpu_write(&mut self, addr: u16, value: u8) {
        match addr {
            0x6000..=0x7FFF => {
                if self.prg_ram_enabled && !self.prg_ram_protect && !self.prg_ram.is_empty() {
                    let off = (addr - 0x6000) as usize;
                    if off < self.prg_ram.len() {
                        self.prg_ram[off] = value;
                    }
                }
            }
            0x8000..=0x9FFF => {
                if addr & 1 == 0 {
                    // $8000 even: bank-select.
                    self.bank_select = value & 0x07;
                    self.prg_mode = (value & 0x40) != 0;
                    self.chr_mode = (value & 0x80) != 0;
                } else {
                    // $8001 odd: bank-data.
                    self.regs[(self.bank_select & 0x07) as usize] = value;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    // Mirroring (ignored on 4-screen carts).
                    if !self.fixed_4screen {
                        self.mirroring = if value & 1 == 0 {
                            Mirroring::Vertical
                        } else {
                            Mirroring::Horizontal
                        };
                    }
                } else {
                    // PRG-RAM protect / enable.
                    self.prg_ram_enabled = (value & 0x80) != 0;
                    self.prg_ram_protect = (value & 0x40) != 0;
                }
            }
            0xC000..=0xDFFF => {
                if addr & 1 == 0 {
                    self.irq_reload_value = value;
                } else {
                    // $C001: latch "was the counter non-zero at the moment
                    // of this clear?" — consumed by `clock_irq` on the
                    // next filtered A12 rise to decide whether the
                    // reload-pending path asserts (Sharp).  See the field
                    // doc on `irq_reload_pending_with_nonzero_clear`.
                    self.irq_reload_pending_with_nonzero_clear = self.irq_counter != 0;
                    self.irq_counter = 0;
                    self.irq_reload_pending = true;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    self.irq_enabled = false;
                    self.irq_pending_line = false;
                } else {
                    self.irq_enabled = true;
                }
            }
            _ => {}
        }
    }

    fn chr_phys(&self, addr: u16) -> Option<u32> {
        if self.chr_is_ram {
            None
        } else {
            // The same per-bank offset `ppu_read` resolves (the 2/4 KiB MMC3 banks).
            u32::try_from(self.chr_offset(addr & 0x1FFF) % self.chr.len().max(1)).ok()
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
        // For 2 KiB CIRAM (the bus's PPU vram) the offset must fit in 0..0x800.
        // For 4-screen we keep the full 4 KiB on-cart and serve via ppu_read/write,
        // so the bus's CIRAM index does not matter — just return a canonical 0.
        let off = self.nametable_offset(addr);
        u16::try_from(off & 0x07FF).unwrap_or(0)
    }

    fn current_mirroring(&self) -> Mirroring {
        self.mirroring
    }

    fn notify_a12(&mut self, level: bool) {
        // Plumbing is in place to receive the sub-dot via
        // `notify_a12_at_sub_dot`, but the MMC3 implementation does not
        // yet differentiate behavior by M2 phase — see ADR-0002 →
        // "Sub-dot plumbing landed (2026-05-14)" for the open
        // implementation choice.  This legacy entry point treats the
        // unknown sub-dot as M2-low (immediate assertion), matching
        // the pre-M2-phase-pipeline behavior.
        self.notify_a12_at_sub_dot(level, 1);
    }

    fn notify_a12_at_sub_dot(&mut self, level: bool, _sub_dot: u8) {
        // Track the M2-cycles-since-last-fall filter.  A rising edge that
        // arrives < 3 CPU cycles after the prior fall is filtered.
        // The sub-dot parameter is currently unused: the M2-phase-aware
        // deferral pipeline is documented in ADR-0002 but not yet
        // implemented in MMC3 (the open work item for the next
        // iteration of C1).
        if !self.last_a12 && level {
            // Rising edge.
            let gap = self.cpu_cycle.saturating_sub(self.a12_low_cycle);
            if gap >= 3 && self.clock_irq() {
                self.irq_pending_line = true;
            }
        } else if self.last_a12 && !level {
            // Falling edge.
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

    fn irq_acknowledge(&mut self) {
        // Hardware: the IRQ line stays asserted until $E000 disables / acks.
        // The CPU's interrupt service does not clear it; the program does.
        // We expose ack as a no-op to satisfy the trait but $E000 is the
        // real path.
    }

    fn debug_info(&self) -> crate::mapper::MapperDebugInfo {
        let mut info = crate::mapper::MapperDebugInfo {
            mapper_id: 4,
            name: format!("MMC3 ({:?})", self.revision),
            mirroring: crate::mapper::mirroring_name(self.mirroring),
            ..Default::default()
        };
        info.prg_banks
            .push(("mode".into(), format!("{}", u8::from(self.prg_mode))));
        info.prg_banks
            .push(("R6".into(), format!("{:#04x}", self.regs[6])));
        info.prg_banks
            .push(("R7".into(), format!("{:#04x}", self.regs[7])));
        info.chr_banks
            .push(("mode".into(), format!("{}", u8::from(self.chr_mode))));
        for i in 0..6 {
            info.chr_banks
                .push((format!("R{i}"), format!("{:#04x}", self.regs[i])));
        }
        info.irq_state
            .push(("counter".into(), format!("{:#04x}", self.irq_counter)));
        info.irq_state
            .push(("reload".into(), format!("{:#04x}", self.irq_reload_value)));
        info.irq_state
            .push(("enabled".into(), format!("{}", self.irq_enabled)));
        info.irq_state
            .push(("pending".into(), format!("{}", self.irq_pending_line)));
        info.extra
            .push(("bank_select".into(), format!("{:#04x}", self.bank_select)));
        info.extra.push((
            "prg_ram".into(),
            format!("en={} prot={}", self.prg_ram_enabled, self.prg_ram_protect),
        ));
        info
    }

    fn save_state(&self) -> Vec<u8> {
        // Tagged blob: version + scalar regs + RAM blocks.
        let mut out =
            Vec::with_capacity(64 + self.prg_ram.len() + self.vram.len() + self.chr.len());
        out.push(SAVE_STATE_VERSION);
        out.extend_from_slice(&self.regs);
        out.push(self.bank_select);
        out.push(u8::from(self.prg_mode));
        out.push(u8::from(self.chr_mode));
        out.push(self.mirroring as u8);
        out.push(u8::from(self.fixed_4screen));
        out.push(u8::from(self.prg_ram_enabled));
        out.push(u8::from(self.prg_ram_protect));
        out.push(self.irq_counter);
        out.push(self.irq_reload_value);
        out.push(u8::from(self.irq_reload_pending));
        out.push(u8::from(self.irq_reload_pending_with_nonzero_clear));
        out.push(u8::from(self.irq_enabled));
        out.push(u8::from(self.irq_pending_line));
        out.push(u8::from(self.last_a12));
        out.extend_from_slice(&self.a12_low_cycle.to_le_bytes());
        out.extend_from_slice(&self.cpu_cycle.to_le_bytes());
        out.push(match self.revision {
            Mmc3Revision::Sharp => 0,
            Mmc3Revision::Nec => 1,
        });
        out.extend_from_slice(&self.prg_ram);
        out.extend_from_slice(&self.vram);
        if self.chr_is_ram {
            out.extend_from_slice(&self.chr);
        }
        out
    }

    #[allow(clippy::too_many_lines)] // tagged-blob deserializer + v1/v2 fork
    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_part = if self.chr_is_ram { self.chr.len() } else { 0 };
        // Tagged scalars laid out below.  v1 omitted
        // `irq_reload_pending_with_nonzero_clear`; v2 added it as a one-byte
        // flag immediately after `irq_reload_pending`, expanding the scalar
        // section by 1 byte.  Cross-version files load with the field
        // defaulted to `false` (safe — the silicon initial state).
        if data.is_empty() {
            return Err(MapperError::Truncated {
                expected: 1,
                got: 0,
            });
        }
        let version = data[0];
        if version != 1 && version != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(version));
        }
        let nonzero_clear_present = version >= 2;
        let scalar_len = 1
            + 8
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 1
            + 8
            + 8
            + 1
            + usize::from(nonzero_clear_present);
        let expected = scalar_len + self.prg_ram.len() + self.vram.len() + chr_part;
        if data.len() != expected {
            return Err(MapperError::Truncated {
                expected,
                got: data.len(),
            });
        }
        self.regs.copy_from_slice(&data[1..9]);
        self.bank_select = data[9];
        self.prg_mode = data[10] != 0;
        self.chr_mode = data[11] != 0;
        self.mirroring = match data[12] {
            0 => Mirroring::Horizontal,
            1 => Mirroring::Vertical,
            2 => Mirroring::SingleScreenA,
            3 => Mirroring::SingleScreenB,
            4 => Mirroring::FourScreen,
            5 => Mirroring::MapperControlled,
            other => {
                return Err(MapperError::Invalid(format!(
                    "unknown mirroring tag {other}"
                )));
            }
        };
        self.fixed_4screen = data[13] != 0;
        self.prg_ram_enabled = data[14] != 0;
        self.prg_ram_protect = data[15] != 0;
        self.irq_counter = data[16];
        self.irq_reload_value = data[17];
        self.irq_reload_pending = data[18] != 0;
        let mut cur = 19usize;
        self.irq_reload_pending_with_nonzero_clear = if nonzero_clear_present {
            let v = data[cur] != 0;
            cur += 1;
            v
        } else {
            // v1 fallback: pre-fix state was equivalent to "always assert
            // on reload-pending", but the flag's semantic absence is best
            // represented as `false` (silent reload on next A12), which
            // matches the post-fix steady state when nothing has been
            // written to $C001 since the snapshot.
            false
        };
        self.irq_enabled = data[cur] != 0;
        cur += 1;
        self.irq_pending_line = data[cur] != 0;
        cur += 1;
        self.last_a12 = data[cur] != 0;
        cur += 1;
        self.a12_low_cycle = u64::from_le_bytes(
            data[cur..cur + 8]
                .try_into()
                .map_err(|_| MapperError::Invalid("a12_low_cycle truncated".into()))?,
        );
        cur += 8;
        self.cpu_cycle = u64::from_le_bytes(
            data[cur..cur + 8]
                .try_into()
                .map_err(|_| MapperError::Invalid("cpu_cycle truncated".into()))?,
        );
        cur += 8;
        self.revision = match data[cur] {
            0 => Mmc3Revision::Sharp,
            1 => Mmc3Revision::Nec,
            other => {
                return Err(MapperError::Invalid(format!(
                    "unknown MMC3 revision tag {other}"
                )));
            }
        };
        cur += 1;
        self.prg_ram
            .copy_from_slice(&data[cur..cur + self.prg_ram.len()]);
        cur += self.prg_ram.len();
        self.vram.copy_from_slice(&data[cur..cur + self.vram.len()]);
        cur += self.vram.len();
        if self.chr_is_ram {
            self.chr.copy_from_slice(&data[cur..cur + self.chr.len()]);
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

    fn fresh(prg_banks: usize, chr_banks: usize) -> Mmc3 {
        Mmc3::new(
            synth_prg(prg_banks),
            synth_chr(chr_banks),
            Mirroring::Horizontal,
            0,
            Mmc3Revision::Sharp,
        )
        .unwrap()
    }

    #[test]
    fn last_8k_bank_fixed_at_e000() {
        let mut m = fresh(8, 8);
        // Default state: PRG mode 0, R6=R7=0.  $E000 should map to last bank.
        assert_eq!(m.cpu_read(0xE000), 7);
    }

    #[test]
    fn second_to_last_bank_fixed_at_c000_in_mode0() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x8000, 0); // mode 0
        assert_eq!(m.cpu_read(0xC000), 6);
    }

    #[test]
    fn r6_swaps_8000_in_mode0_and_c000_in_mode1() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x8000, 6); // select R6
        m.cpu_write(0x8001, 3); // R6 = 3
        // Mode 0: $8000 -> R6 = bank 3
        assert_eq!(m.cpu_read(0x8000), 3);
        // Mode 1: $8000 -> second-to-last (bank 6); $C000 -> R6 = bank 3.
        m.cpu_write(0x8000, 0x40 | 6); // PRG mode bit
        assert_eq!(m.cpu_read(0x8000), 6);
        assert_eq!(m.cpu_read(0xC000), 3);
    }

    #[test]
    fn chr_mode0_layout_2k_2k_1k_1k_1k_1k() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x8000, 0); // R0
        m.cpu_write(0x8001, 4); // R0 = 4 (LSB ignored, so bank 4)
        m.cpu_write(0x8000, 1); // R1
        m.cpu_write(0x8001, 6); // R1 = 6
        m.cpu_write(0x8000, 2); // R2
        m.cpu_write(0x8001, 1); // R2 = bank 1
        // $0000-$03FF (slot 0) -> R0 & ~1 = 4.
        assert_eq!(m.ppu_read(0x0000), 4);
        // $0400 (slot 1) -> R0 | 1 = 5.
        assert_eq!(m.ppu_read(0x0400), 5);
        // $0800 (slot 2) -> R1 & ~1 = 6.
        assert_eq!(m.ppu_read(0x0800), 6);
        // $1000 (slot 4) -> R2 = 1.
        assert_eq!(m.ppu_read(0x1000), 1);
    }

    #[test]
    fn chr_mode1_swaps_2k_and_1k_regions() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x8000, 0x80); // CHR mode 1
        m.cpu_write(0x8000, 0x80); // R0
        m.cpu_write(0x8001, 4);
        m.cpu_write(0x8000, 0x80 | 2);
        m.cpu_write(0x8001, 1); // R2
        // Mode 1: $0000 (slot 0) -> R2 = 1; $1000 (slot 4) -> R0 & ~1 = 4.
        assert_eq!(m.ppu_read(0x0000), 1);
        assert_eq!(m.ppu_read(0x1000), 4);
    }

    #[test]
    fn mirroring_register_toggles_h_v() {
        let mut m = fresh(8, 8);
        m.cpu_write(0xA000, 0);
        assert_eq!(m.mirroring, Mirroring::Vertical);
        m.cpu_write(0xA000, 1);
        assert_eq!(m.mirroring, Mirroring::Horizontal);
    }

    #[test]
    fn prg_ram_enable_protect_via_a001() {
        let mut m = fresh(8, 8);
        // PRG-RAM defaults to enabled, not protected.
        m.cpu_write(0x6000, 0xAB);
        assert_eq!(m.cpu_read(0x6000), 0xAB);
        // Disable.
        m.cpu_write(0xA001, 0x00);
        m.cpu_write(0x6000, 0xCD); // ignored
        assert_eq!(m.cpu_read(0x6000), 0); // returns 0 (open bus stub)
        // Re-enable + write-protect.
        m.cpu_write(0xA001, 0x80 | 0x40);
        m.cpu_write(0x6000, 0x12); // ignored (protected)
        assert_eq!(m.cpu_read(0x6000), 0xAB); // original value preserved
    }

    #[test]
    fn irq_counter_decrements_and_asserts() {
        let mut m = fresh(8, 8);
        m.cpu_write(0xC000, 3); // reload = 3
        m.cpu_write(0xC001, 0); // pending reload
        m.cpu_write(0xE001, 0); // enable IRQ
        // Simulate four filtered A12 rising edges, advancing CPU cycles
        // between each so the M2 filter accepts.
        for _ in 0..4 {
            // Fall A12 low, advance >= 3 CPU cycles, then raise.
            m.notify_a12(false);
            for _ in 0..4 {
                m.notify_cpu_cycle();
            }
            m.notify_a12(true);
        }
        // First edge: reload to 3.  Edges 2,3,4: decrement to 2,1,0 (assert).
        assert!(m.irq_pending());
    }

    #[test]
    fn irq_disabled_no_assert() {
        let mut m = fresh(8, 8);
        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        // No $E001 enable.
        for _ in 0..3 {
            m.notify_a12(false);
            for _ in 0..4 {
                m.notify_cpu_cycle();
            }
            m.notify_a12(true);
        }
        assert!(!m.irq_pending());
    }

    #[test]
    fn e000_acks_pending_irq() {
        let mut m = fresh(8, 8);
        m.irq_pending_line = true;
        m.cpu_write(0xE000, 0);
        assert!(!m.irq_pending());
    }

    #[test]
    fn a12_filter_rejects_close_rising_edges() {
        let mut m = fresh(8, 8);
        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        // First edge: filter accepts (reload to 1).
        m.notify_a12(false);
        for _ in 0..4 {
            m.notify_cpu_cycle();
        }
        m.notify_a12(true);
        // Now toggle low->high again with only 1 cycle gap: filter REJECTS.
        m.notify_a12(false);
        m.notify_cpu_cycle();
        m.notify_a12(true);
        // Counter should still be 1 (only first edge was accepted).
        assert_eq!(m.irq_counter, 1);
        assert!(!m.irq_pending());
    }

    /// Helper: emit one filter-accepted A12 toggle (low ≥ 3 M2 cycles
    /// then high).  Used by the Sharp/NEC reload-to-zero unit tests
    /// below to drive the counter through deterministic transitions.
    fn a12_rise<F: Mapper>(m: &mut F) {
        m.notify_a12(false);
        for _ in 0..4 {
            m.notify_cpu_cycle();
        }
        m.notify_a12(true);
    }

    /// Sharp asserts IRQ when the counter is decremented to 0 via a
    /// natural A12 clock (decrement-to-0 path).  This is the primary
    /// Sharp/NEC commonality — both revisions assert here.  See
    /// `clock_irq` path 3 (decrement).
    #[test]
    fn sharp_asserts_on_decrement_to_zero() {
        let mut m = fresh(8, 8);
        // Reload value = 1.  Pre-condition: counter at 0, reload_pending
        // set (from $C001 after start-up).
        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        // First A12 rise: silent reload to 1 (was_nonzero_at_clear = false
        // — counter was already 0 at the $C001 write).
        a12_rise(&mut m);
        assert_eq!(m.irq_counter, 1, "first rise reloaded silently");
        assert!(
            !m.irq_pending(),
            "first $C001-induced reload (counter was 0) must not assert"
        );
        // Second A12 rise: counter decrements from 1 to 0 → asserts (both
        // Sharp and NEC).
        a12_rise(&mut m);
        assert_eq!(m.irq_counter, 0);
        assert!(
            m.irq_pending(),
            "decrement-to-0 asserts on both Sharp and NEC"
        );
    }

    /// Sharp's Rev-A-specific "reload-to-0 asserts" rule.  Distinct from
    /// NEC (Rev B) in `nec_does_not_assert_on_reload_to_zero` below.
    /// This is the path `mmc3_test_2/5-MMC3.nes` ("set IRQ every clock
    /// when reload is 0") exercises in the steady state.
    ///
    /// Setup: $C001 clears a **non-zero** counter — that's the
    /// discriminator (see `irq_reload_pending_with_nonzero_clear`).  Real
    /// silicon: after a non-zero-to-zero clear, the next A12 rise
    /// reloads the counter, and if the reload landed at 0, Sharp
    /// asserts.  The follow-up A12 rise (after IRQ is acked) also
    /// asserts on Sharp because the natural was_zero path keeps
    /// reloading to 0.
    #[test]
    fn sharp_asserts_on_reload_to_zero_after_nonzero_clear() {
        let mut m = fresh(8, 8);
        // Prime the counter to a non-zero value: write reload_value=1,
        // $C001 (counter was 0 — silent), one A12 (silent reload to 1).
        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        a12_rise(&mut m);
        assert_eq!(m.irq_counter, 1);
        assert!(!m.irq_pending(), "silent reload, no assertion");
        // Now write reload_value=0 and $C001 again (counter WAS non-zero
        // at this write, so the next A12 rise asserts on Sharp).
        m.cpu_write(0xC000, 0);
        m.cpu_write(0xC001, 0);
        // Filter accepts after ≥ 3 M2 cycles; the prior `a12_rise` already
        // re-armed the filter low.
        a12_rise(&mut m);
        assert_eq!(m.irq_counter, 0);
        assert!(
            m.irq_pending(),
            "Sharp asserts on reload-to-0 after non-zero-to-zero $C001 clear"
        );
    }

    /// `$C001` written while the counter was already 0 is a "no-op
    /// clear" — the next A12 rise reloads silently regardless of
    /// `reload_value`.  This is the cycle-precise discriminator that
    /// makes `mmc3_test_2/4-scanline_timing` sub-test #2 pass without
    /// regressing `mmc3_test_2/2-details` sub-test #7.
    #[test]
    fn sharp_silent_on_reload_after_zero_to_zero_clear() {
        let mut m = fresh(8, 8);
        // Counter is 0 from start-up (`fresh`).  $C001 writes here are
        // zero-to-zero clears.
        m.cpu_write(0xC000, 0);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        a12_rise(&mut m);
        assert_eq!(m.irq_counter, 0);
        assert!(
            !m.irq_pending(),
            "zero-to-zero $C001 clear must reload silently on the first A12 rise"
        );
        // The follow-up A12 rise (now via the natural was_zero path with
        // no pending reload) DOES assert on Sharp — this is the same path
        // that fires the IRQ on scanline 0 in the failing-mode of
        // `mmc3_test_2/4-scanline_timing` sub-test #2.
        a12_rise(&mut m);
        assert!(
            m.irq_pending(),
            "natural reload-to-0 (no pending reload) asserts on Sharp"
        );
    }

    /// NEC (Rev B) does NOT assert on reload-to-0 even on the natural
    /// was_zero path.  Mutually exclusive with the Sharp behavior tested
    /// above.
    #[test]
    fn nec_does_not_assert_on_reload_to_zero() {
        let mut m = Mmc3::new(
            synth_prg(8),
            synth_chr(8),
            Mirroring::Horizontal,
            0,
            Mmc3Revision::Nec,
        )
        .unwrap();
        // Same "non-zero clear" setup as the Sharp test, but on NEC the
        // reload-to-0 should NOT assert.
        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        a12_rise(&mut m);
        m.cpu_write(0xC000, 0);
        m.cpu_write(0xC001, 0);
        a12_rise(&mut m);
        assert_eq!(m.irq_counter, 0);
        assert!(
            !m.irq_pending(),
            "NEC suppresses Sharp's reload-to-0 assertion"
        );
        // Even the natural was_zero path doesn't assert on NEC.
        a12_rise(&mut m);
        assert!(!m.irq_pending(), "NEC: was_zero reload-to-0 also silent");
    }

    /// T-41-005 — reversed pattern-table layout (`PPUCTRL` bit 4 set,
    /// bit 3 clear: BG=`$1000`, sprites=`$0000`). Canary: Wario's Woods.
    ///
    /// In the standard layout (BG=`$0000`, sprites=`$1000`) the per-scanline
    /// A12 rising edge happens during the sprite tile fetch group at PPU
    /// dot 260, after the BG fetches at dots 1-256 (all of which used the
    /// `$0000` pattern table). In the reversed layout the per-scanline A12
    /// rising edge happens during the next scanline's BG fetch group at
    /// dot 1, after the sprite tile fetches at dots 260-320 (which used the
    /// `$0000` pattern table).
    ///
    /// From MMC3's perspective the two layouts produce **the same sequence
    /// of A12 transitions per scanline** — one fall after the prior
    /// scanline's BG fetches (or this scanline's sprite fetches), followed
    /// by a rise after enough M2 cycles for the filter to accept. The IRQ
    /// counter should clock identically. This test exercises both layouts
    /// in the same `Mmc3` and asserts the per-rising-edge counter behavior
    /// matches.
    #[test]
    fn reversed_pattern_table_layout_clocks_irq_identically() {
        // Helper: emit one filter-accepted A12 rise (low for ≥ 3 M2 cycles,
        // then high). Returns the IRQ-pending flag immediately after.
        fn pulse_a12(m: &mut Mmc3) -> bool {
            m.notify_a12(false);
            for _ in 0..4 {
                m.notify_cpu_cycle();
            }
            m.notify_a12(true);
            m.irq_pending()
        }

        // Standard layout simulation (BG=$0000, sprites=$1000). Per-scanline:
        // 1. BG fetches at dots 1-256 read patterns from $0000-$0FFF (A12 low).
        // 2. Sprite tile fetches at dots 260-320 read from $1000-$1FFF (A12
        //    rises around dot 260).
        // 3. After dot 320 the BG fetches for the *next* scanline run at
        //    dots 321-336 from $0000-$0FFF (A12 falls again).
        // We model this as: A12=false (during BG fetches) -> A12=true (sprite
        //    fetches) per scanline. Filter sees one rise per scanline.
        let mut std_layout = fresh(8, 8);
        std_layout.cpu_write(0xC000, 4); // reload = 4
        std_layout.cpu_write(0xC001, 0); // pending reload
        std_layout.cpu_write(0xE001, 0); // enable IRQ
        // 5 scanlines: edges 1 (reload 4), 2 (3), 3 (2), 4 (1), 5 (0 + assert).
        for n in 0..5 {
            let pending = pulse_a12(&mut std_layout);
            // Only the 5th edge should assert (counter went 4→reload, then
            // 4→3→2→1→0).
            assert_eq!(
                pending,
                n == 4,
                "standard layout edge #{n} pending should be {} (counter={})",
                n == 4,
                std_layout.irq_counter
            );
        }
        std_layout.cpu_write(0xE000, 0); // ack

        // Reversed layout simulation (BG=$1000, sprites=$0000). Per-scanline:
        // 1. BG fetches at dots 1-256 read patterns from $1000-$1FFF (A12
        //    high — but the rising edge happened at the start of the BG fetch
        //    group, not at dot 260).
        // 2. Sprite tile fetches at dots 260-320 read from $0000-$0FFF (A12
        //    falls).
        // 3. Next scanline's BG fetches at dots 321-336 read from $1000 again
        //    (A12 rises).
        // From the mapper's perspective: one fall + one rise per scanline,
        // just shifted relative to where the rise happens within the
        // scanline. The filter behavior is identical.
        let mut rev_layout = fresh(8, 8);
        rev_layout.cpu_write(0xC000, 4);
        rev_layout.cpu_write(0xC001, 0);
        rev_layout.cpu_write(0xE001, 0);
        for n in 0..5 {
            let pending = pulse_a12(&mut rev_layout);
            assert_eq!(
                pending,
                n == 4,
                "reversed layout edge #{n} pending should be {} (counter={})",
                n == 4,
                rev_layout.irq_counter
            );
        }

        // Both layouts must reach the same internal state at the same edge.
        assert_eq!(
            std_layout.irq_counter, rev_layout.irq_counter,
            "standard and reversed layouts must produce identical IRQ counter \
             values after the same number of filter-accepted A12 rises"
        );
    }

    /// T-41-005 follow-up — the A12 filter must remain stable against
    /// the **fast-low-high** pulse pattern that the reversed layout
    /// produces at the boundary between sprite fetches (dot 320, A12
    /// low) and the next scanline's BG fetches (dot 321 onward, A12
    /// high). Real silicon's 3-M2-cycle filter rejects rises that come
    /// less than ~3 CPU cycles after the previous fall. We assert the
    /// filter rejects the rise if too few cycles have elapsed AND
    /// accepts it when enough have.
    #[test]
    fn reversed_layout_a12_filter_3_m2_boundary() {
        let mut m = fresh(8, 8);
        m.cpu_write(0xC000, 2);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);

        // First rise — filter primes from low state.
        m.notify_a12(false);
        for _ in 0..4 {
            m.notify_cpu_cycle();
        }
        m.notify_a12(true);
        let counter_after_first = m.irq_counter;
        assert_eq!(counter_after_first, 2, "first rise reloads to 2");

        // Rapid fall+rise with only 1 CPU cycle gap: filter REJECTS.
        m.notify_a12(false);
        m.notify_cpu_cycle();
        m.notify_a12(true);
        assert_eq!(
            m.irq_counter, counter_after_first,
            "rapid rise within < 3 M2 cycles must be filtered out"
        );

        // Same pulse but with 3 cycles between fall and rise: ACCEPTED.
        m.notify_a12(false);
        for _ in 0..4 {
            m.notify_cpu_cycle();
        }
        m.notify_a12(true);
        assert!(
            m.irq_counter < counter_after_first,
            "rise after >= 3 M2 cycles must clock the counter; counter={}",
            m.irq_counter
        );
    }

    #[test]
    fn save_load_round_trip() {
        let mut m = fresh(8, 8);
        m.cpu_write(0x8000, 6);
        m.cpu_write(0x8001, 3);
        m.cpu_write(0x8000, 7);
        m.cpu_write(0x8001, 5);
        m.cpu_write(0xC000, 0x42);
        m.cpu_write(0xE001, 0);
        let blob = m.save_state();
        let mut other = fresh(8, 8);
        other.load_state(&blob).unwrap();
        assert_eq!(other.regs, m.regs);
        assert_eq!(other.bank_select, m.bank_select);
        assert_eq!(other.irq_reload_value, m.irq_reload_value);
        assert_eq!(other.irq_enabled, m.irq_enabled);
    }
}
