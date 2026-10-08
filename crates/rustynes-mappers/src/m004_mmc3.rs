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
//! On each filtered rising edge (`clock_irq`):
//! - a `$C001` reload (`irq_reload_pending`): counter = `irq_reload_value`,
//!   flag cleared; BOTH revisions assert if the new value is 0 and IRQs are
//!   enabled (v3.1.0; the alternate one used not to, see `clock_irq`);
//! - else if `counter == 0`: counter = `irq_reload_value`; only **Sharp**
//!   asserts if the new value is 0, so a latch of 0 fires every scanline on
//!   Sharp and stops on the alternate chip;
//! - else: counter -= 1; if it reached 0 and IRQs are enabled, assert.
//!
//! Default revision is **Sharp** per project policy (Star Trek: 25th
//! Anniversary requires it). NES 2.0 submapper 4 selects the alternate
//! behaviour of the MMC3A and non-Sharp MMC3B ([`Mmc3Revision::Nec`]), 1 the
//! MMC6 (v2.9.6 corrected both; this paragraph said "submapper 1 selects
//! MMC3B (NEC)" until v3.1.0). For an iNES 1.0 dump, which cannot say,
//! `Mapper::set_mmc3_revision_override` selects it (v3.1.0).

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

/// v3 (v2.9.6) appends the MMC6 PRG-RAM state and the MC-ACC prescaler.
/// Only v3 is read since v2.9.8 (ADR 0042); v1 and v2 used to load with the
/// later fields at defaults.
const SAVE_STATE_VERSION: u8 = 4;

/// MMC6 internal PRG-RAM: 1 KiB, two 512-byte halves (`MMC6.md`).
const MMC6_RAM: usize = 0x0400;

/// MMC3 IRQ-counter behaviour. Default is the Sharp ("new") behaviour; the
/// alternative suppresses the "reload to 0 asserts IRQ" behaviour.
///
/// **Naming, corrected in v2.9.6.** These two variants were documented as
/// "Sharp MMC3A" and "NEC MMC3B". `MMC3.md` says otherwise: the old or
/// alternate behaviour belongs to the MMC3A and to non-Sharp MMC3B chips
/// ("1 (Sharp MMC3B, MMC3C) or 2 (MMC3A, Non-Sharp MMC3B) to 256 scanlines"),
/// and the NES 2.0 submapper for it is 4, not 1 (`NES_2_0_submappers.md`).
/// The behaviour of each variant was always right; only the labels were
/// wrong, together with the submapper mapping that followed them.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub enum Mmc3Revision {
    /// Sharp MMC3B and MMC3C: reloading the IRQ counter to 0 asserts IRQ if
    /// IRQs are enabled, so a latch of 0 fires every scanline. Default.
    #[default]
    Sharp,
    /// NEC MMC3B and the MMC3A: the IRQ fires on the counter's 1 -> 0
    /// transition, so a latch of 0 stops IRQs. NES 2.0 submapper 4.
    Nec,
}

/// Which chip or board wiring an [`Mmc3`] models, beyond the IRQ revision
/// (the NES 2.0 submappers of mapper 4, `NES_2_0_submappers.md`). v2.9.6.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Mmc3Variant {
    /// A standard MMC3 board (submappers 0 and 4).
    #[default]
    Standard,
    /// Submapper 1, the MMC6 (`MMC6.md`): 1 KiB of internal PRG-RAM at
    /// `$7000-$7FFF`, enabled by `$8000` bit 5, with separate read and write
    /// enables for each 512-byte half in `$A001` bits 4-7.
    Mmc6,
    /// Submapper 2: an MMC3C with hard-wired mirroring; `$A000` does nothing.
    HardwiredMirroring,
    /// Submapper 3, Acclaim's MC-ACC (`MMC3.md`, "IRQ Specifics"): the
    /// scanline counter is clocked by FALLING edges of PPU A12 through a
    /// divide-by-8 prescaler instead of the M2 filter. The page gives only
    /// that. The two details used here come from the forum measurement it
    /// links (forums.nesdev.org, p=242427): writing `$C001` resets the
    /// prescaler, and the counter clocks on the first edge of each group of
    /// eight. That evidence is forum-level, so mapper 4 submapper 3 is
    /// BestEffort in `tier.rs`.
    McAcc,
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
    // A clocking A12 rise sets this instead of the IRQ line, and the first
    // `notify_cpu_cycle` after the rise moves it to `irq_pending_line`
    // (T-ORACLE-001, v2.9.9). WHICH cycle that is depends on the bus, not on
    // this mapper: `Cpu::start_cycle` catches the PPU up to the access and
    // then calls `SystemBus::cpu_clock`, which calls `notify_cpu_cycle`. So a
    // rise caught up before the access is raised in its own cycle, and one
    // caught up after the access (`end_cycle`) from the next cycle on. (This
    // comment said "one CPU cycle after the rise" for every rise until the
    // #583 review; ADR 0002's 2026-10-05 correction has the detail.)
    //
    // Why: the oracle raised it a cycle early relative to the MiSTer
    // sibling's MMC3, which registers its IRQ output on the CPU clock enable
    // as a synchronous design must. Its per-cycle trace of
    // `mapper4mmc3irq065` and of blargg's `4-scanline_timing` put the /IRQ
    // fall one cycle apart, and with the oracle deferred by one cycle the two
    // traces agree for 6,253,826 cycles instead of 1,250,766. The deferral
    // also makes `mmc3_test` v1 `5-MMC3` pass and moves `4-scanline_timing`'s
    // first failure from sub-test 3 to sub-test 9. It is not a filter: which
    // rises clock the counter is unchanged, only when the line is seen.
    //
    // This replaced v2.0.0's `mmc3-m2-phase-irq` experiment, which deferred
    // only rises seen in the M2-high half of a cycle. Measured against the
    // same ROMs it passed the same set, because the split above is in effect
    // the same one; this form is kept because it gets it from the order of
    // the catch-up and the per-cycle hook, with no phase data from the bus.
    // A delay of one cycle for EVERY rise cannot be built here: a pre-access
    // rise of cycle N and a post-access rise of cycle N-1 both arrive between
    // the same two `notify_cpu_cycle` calls.
    irq_assert_pending_next_cycle: bool,

    // A12 filter state.
    last_a12: bool,
    // CPU cycle at which A12 last went low; used to filter rising edges
    // closer than 3 M2 cycles.
    a12_low_cycle: u64,
    cpu_cycle: u64,

    revision: Mmc3Revision,
    /// v3.1.0 (`T-MMC3-NEC-OVERRIDE`): the revision the cartridge header
    /// selected, which `revision` returns to when an override is cleared.
    /// Board identity, fixed at construction; not save-state (a state carries
    /// the live `revision`).
    header_revision: Mmc3Revision,
    variant: Mmc3Variant,
    /// MMC6: `$8000` bit 5, the PRG-RAM enable.
    mmc6_ram_enabled: bool,
    /// MMC6: `$A001` bits 4-7 (`HhLl`): read/write enables of each half.
    mmc6_protect: u8,
    /// MC-ACC: falling A12 edges counted, modulo 8.
    mcacc_prescaler: u8,

    // v2.1.5 F5.0 MMC3 R1/R2 residual instrumentation study (`mmc3-a12-phase-
    // probe`, default-off): purely OBSERVATIONAL tallies of *qualifying*
    // (`gap >= 3`) A12 rising edges, bucketed by the M2-phase half of the host
    // CPU cycle in which they were observed. `sub_dot < 2` is the PRE-access
    // (M2-low, φ1) half; `sub_dot >= 2` is the POST-access (M2-high, φ2) half.
    // The `*_irq_*` pair further restricts to rises that actually clocked the
    // counter to a state that asserts the IRQ line (`clock_irq()` returned
    // true). These are counters only — they do NOT influence `irq_pending_line`
    // or any emulated state, so the timeline is byte-identical to the default
    // build. Surfaced via `debug_state().extra` for the study fixture to read.
    // See ADR 0002 §"Decision update (2026-07-11, v2.1.5 F5.0 instrumentation
    // study)". Compiled out entirely when the feature is off.
    #[cfg(feature = "mmc3-a12-phase-probe")]
    probe: Mmc3A12PhaseProbe,
}

/// Observational A12-phase probe state for the v2.1.5 F5.0 MMC3 R1/R2 residual
/// instrumentation study. See the field doc on [`Mmc3::probe`]. Every counter
/// is monotonic over a run; none feeds back into emulated state.
#[cfg(feature = "mmc3-a12-phase-probe")]
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
struct Mmc3A12PhaseProbe {
    /// Qualifying (`gap >= 3`) A12 rises seen in the PRE-access (M2-low, φ1,
    /// `sub_dot < 2`) half of a host CPU cycle.
    qual_rises_pre: u64,
    /// Qualifying (`gap >= 3`) A12 rises seen in the POST-access (M2-high, φ2,
    /// `sub_dot >= 2`) half of a host CPU cycle. **The study's headline metric:
    /// if this is 0 across all four failing sub-tests, no A12-phase / M2-half-
    /// cycle filter refinement can move the residual (axis B is dead).**
    qual_rises_post: u64,
    /// Of the qualifying rises, those that clocked the counter to an
    /// IRQ-asserting state (`clock_irq()` true) in the PRE-access half.
    irq_clock_pre: u64,
    /// Of the qualifying rises, those that clocked the counter to an
    /// IRQ-asserting state (`clock_irq()` true) in the POST-access half.
    irq_clock_post: u64,
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
            irq_assert_pending_next_cycle: false,
            irq_enabled: false,
            irq_pending_line: false,
            last_a12: false,
            a12_low_cycle: 0,
            cpu_cycle: 0,
            revision,
            header_revision: revision,
            variant: Mmc3Variant::Standard,
            mmc6_ram_enabled: false,
            mmc6_protect: 0,
            mcacc_prescaler: 0,
            #[cfg(feature = "mmc3-a12-phase-probe")]
            probe: Mmc3A12PhaseProbe::default(),
        })
    }

    /// Select the board wiring (the NES 2.0 submapper), v2.9.6. An MMC6 gets
    /// its 1 KiB of internal RAM in place of the board PRG-RAM.
    ///
    /// Call it once, on a board fresh from a constructor: choosing
    /// [`Mmc3Variant::Mmc6`] replaces the board PRG-RAM with the 1 KiB
    /// internal RAM, and a later call with another variant does not restore
    /// the original allocation.
    #[must_use]
    pub fn with_variant(mut self, variant: Mmc3Variant) -> Self {
        self.variant = variant;
        if variant == Mmc3Variant::Mmc6 {
            self.prg_ram = vec![0u8; MMC6_RAM].into_boxed_slice();
        }
        self
    }

    /// MMC6: which half `$7000-$7FFF` addresses, and its read / write
    /// enables. Half 0 is `$7000-$71FF` (bits 5 / 4), half 1 `$7200-$73FF`
    /// (bits 7 / 6), mirrored through `$7FFF`.
    const fn mmc6_half(&self, addr: u16) -> (usize, bool, bool) {
        let high = addr & 0x0200 != 0;
        let (r, w) = if high { (0x80, 0x40) } else { (0x20, 0x10) };
        (
            addr as usize & (MMC6_RAM - 1),
            self.mmc6_protect & r != 0,
            self.mmc6_protect & w != 0,
        )
    }

    /// MMC6: the `$7000-$7FFF` window floats when RAM is disabled or neither
    /// half is readable.
    const fn mmc6_window_open(&self) -> bool {
        !self.mmc6_ram_enabled || self.mmc6_protect & 0xA0 == 0
    }

    /// An MMC3 used only as a register file and IRQ counter, for boards that
    /// resolve PRG and CHR themselves (`mmc3_boards.rs`, v2.9.6).
    ///
    /// Those boards put an outer bank or a CHR-RAM overlay between the MMC3's
    /// bank outputs and the memories, so they own the ROM and read the raw
    /// outputs through [`Self::prg_bank_raw`] and [`Self::chr_bank_1k`]. The
    /// core therefore carries only placeholder memories: 8 KiB of PRG and
    /// 1 KiB of CHR it never reads, no PRG-RAM, and the real nametable VRAM
    /// (which the board delegates to it). Nothing in the Nintendo MMC3 path
    /// calls this, so mapper 4 is unchanged.
    pub(crate) fn register_core(mirroring: Mirroring, revision: Mmc3Revision) -> Self {
        // Both sizes are valid by construction, so `new` cannot fail.
        let mut core = Self::new(
            vec![0u8; PRG_BANK_8K].into_boxed_slice(),
            vec![0u8; CHR_BANK_1K].into_boxed_slice(),
            mirroring,
            PRG_RAM_DEFAULT,
            revision,
        )
        .unwrap_or_else(|_| unreachable!("fixed valid sizes"));
        core.prg_ram = Box::new([]);
        core
    }

    /// The MMC3's raw 8 KiB PRG bank output for the CPU address `addr`
    /// (`$8000-$FFFF`), before any board masking: R6 / R7 as written, and
    /// the fixed banks as the chip drives them, all-ones (`$FF`) for the last
    /// and `$FE` for the second-last. A multicart ANDs this with its inner
    /// mask and ORs its outer bank in, which is why the fixed banks must be
    /// the chip's own all-ones pattern and not "last bank of the ROM"
    /// (`nesdev_wiki/output/INES_Mapper_045.md`, "PRG-AND").
    pub(crate) fn prg_bank_raw(&self, addr: u16) -> u8 {
        match (addr & 0xE000, self.prg_mode) {
            (0x8000, false) | (0xC000, true) => self.regs[6],
            (0x8000, true) | (0xC000, false) => 0xFE,
            (0xA000, _) => self.regs[7],
            _ => 0xFF,
        }
    }

    /// `$A001` bit 7: the PRG-RAM chip enable. Boards that put a register in
    /// the PRG-RAM window (mappers 37 and 47) accept a write only while the
    /// MMC3 would let it reach RAM.
    pub(crate) const fn prg_ram_enabled(&self) -> bool {
        self.prg_ram_enabled
    }

    /// `$A001` bit 7 set and bit 6 clear: a write to `$6000-$7FFF` would
    /// reach RAM.
    pub(crate) const fn prg_ram_writable(&self) -> bool {
        self.prg_ram_enabled && !self.prg_ram_protect
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

    /// Clock the IRQ counter on a filtered A12 rising edge, and report
    /// whether the IRQ should assert.
    ///
    /// The NESdev MMC3 page's rule: "When the IRQ is clocked (filtered A12
    /// 0→1), the counter value is checked - if zero or the reload flag is
    /// true, it's reloaded with the IRQ latched value at $C000; otherwise, it
    /// decrements. If the IRQ counter is zero and IRQs are enabled ($E001),
    /// an IRQ is triggered." That is the Sharp chip; the NEC chip asserts
    /// only on a decrement to zero, not on a reload that leaves the counter
    /// at zero ("Old/alternate behavior").
    ///
    /// 1. `irq_reload_pending` (set by a `$C001` write): reload from
    ///    `irq_reload_value`, clear the flag.
    /// 2. Counter already zero: reload from `irq_reload_value`.
    /// 3. Otherwise: decrement.
    ///
    /// After any of the three, Sharp asserts when the counter is zero and
    /// IRQs are enabled. The alternate revision (`Nec`) asserts after path 3,
    /// and after path 1 when the reloaded value is 0, but never after path 2.
    ///
    /// **Changed in v3.1.0 (`T-MMC3-NEC-OVERRIDE`).** The alternate revision
    /// asserted only after path 3. `MMC3.md`: "The 'alternate revision' checks
    /// the IRQ counter transition 1→0, whether from decrementing or
    /// reloading", and "writing to $C001 with $C000 still at $00 will result in
    /// another single IRQ being generated". blargg's `mmc3_test_2/6-MMC3_alt`
    /// says the same ("IRQ should be set when reloading due to clear, even if
    /// counter was already 0") and could not run until v3.1.0 added a way to
    /// select this revision for an iNES 1.0 ROM; it failed there on exactly
    /// this, and passes now. The Sharp path is unchanged.
    ///
    /// **Changed in v2.9.9 (T-ORACLE-001).** Path 1 used to assert only when
    /// the `$C001` write had cleared a non-zero counter (a latch named
    /// `irq_reload_pending_with_nonzero_clear`). The page has no such
    /// condition; the latch existed so `4-scanline_timing` sub-test 2 would
    /// pass, and it did so by raising the IRQ a scanline late, which is what
    /// failed sub-test 3. With the IRQ output deferred to the next per-cycle hook
    /// (`irq_assert_pending_next_cycle`) the page's rule passes sub-test 2
    /// by itself. ADR 0002 keeps the history of the earlier attempts.
    fn clock_irq(&mut self) -> bool {
        let mut would_assert = false;
        if self.irq_reload_pending {
            // Path 1: explicit $C001 reload. Both revisions assert when the
            // reloaded value is 0 (for the alternate one, the single IRQ a
            // $C001 write with $C000 = $00 produces).
            self.irq_counter = self.irq_reload_value;
            self.irq_reload_pending = false;
            if self.irq_enabled && self.irq_counter == 0 {
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
    /// v3.1.0 (`T-MMC3-NEC-OVERRIDE`): `Some` forces the IRQ revision, `None`
    /// returns to the header's. Applies to mapper 4 itself; the MMC3-derived
    /// boards that embed this core keep their own revision.
    fn set_mmc3_revision_override(&mut self, revision: Option<Mmc3Revision>) -> bool {
        self.revision = revision.unwrap_or(self.header_revision);
        true
    }

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

    fn cpu_read_unmapped(&self, addr: u16) -> bool {
        match addr {
            0x4020..=0x5FFF => true,
            0x6000..=0x6FFF if self.variant == Mmc3Variant::Mmc6 => true,
            0x7000..=0x7FFF if self.variant == Mmc3Variant::Mmc6 => self.mmc6_window_open(),
            0x6000..=0x7FFF => self.prg_ram.is_empty(),
            _ => false,
        }
    }

    fn cpu_read(&mut self, addr: u16) -> u8 {
        match addr {
            0x6000..=0x7FFF if self.variant == Mmc3Variant::Mmc6 => {
                // "If only one bank is enabled for reading, the other reads
                // back as zero" (`MMC6.md`).
                let (off, readable, _) = self.mmc6_half(addr);
                if addr >= 0x7000 && readable && self.mmc6_ram_enabled {
                    self.prg_ram[off]
                } else {
                    0
                }
            }
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
            0x6000..=0x7FFF if self.variant == Mmc3Variant::Mmc6 => {
                // "The write-enable bits only have effect if that bank is
                // enabled for reading".
                let (off, readable, writable) = self.mmc6_half(addr);
                if addr >= 0x7000 && self.mmc6_ram_enabled && readable && writable {
                    self.prg_ram[off] = value;
                }
            }
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
                    if self.variant == Mmc3Variant::Mmc6 {
                        // "When PRG RAM is disabled via $8000, the mapper
                        // continuously sets $A001 to $00".
                        self.mmc6_ram_enabled = value & 0x20 != 0;
                        if !self.mmc6_ram_enabled {
                            self.mmc6_protect = 0;
                        }
                    }
                } else {
                    // $8001 odd: bank-data.
                    self.regs[(self.bank_select & 0x07) as usize] = value;
                }
            }
            0xA000..=0xBFFF => {
                if addr & 1 == 0 {
                    // Mirroring (ignored on 4-screen carts and on the
                    // hard-wired MMC3C board, submapper 2).
                    if !self.fixed_4screen && self.variant != Mmc3Variant::HardwiredMirroring {
                        self.mirroring = if value & 1 == 0 {
                            Mirroring::Vertical
                        } else {
                            Mirroring::Horizontal
                        };
                    }
                } else if self.variant == Mmc3Variant::Mmc6 {
                    if self.mmc6_ram_enabled {
                        self.mmc6_protect = value & 0xF0;
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
                    // $C001: clear the counter; the next filtered A12 rise
                    // reloads it (`clock_irq` path 1).
                    self.irq_counter = 0;
                    self.irq_reload_pending = true;
                    // MC-ACC: "Writing to $C001 resets pulse counter".
                    self.mcacc_prescaler = 0;
                }
            }
            0xE000..=0xFFFF => {
                if addr & 1 == 0 {
                    self.irq_enabled = false;
                    self.irq_pending_line = false;
                    // The ack/disable also cancels an assertion still in
                    // flight: it is the same IRQ line, one cycle earlier.
                    self.irq_assert_pending_next_cycle = false;
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

    fn notify_a12_at_sub_dot(&mut self, level: bool, sub_dot: u8) {
        // Track the M2-cycles-since-last-fall filter.  A rising edge that
        // arrives < 3 CPU cycles after the prior fall is filtered.
        //
        //
        // `sub_dot` (the M2 half of the CPU cycle the rise landed in) is read
        // only by the `mmc3-a12-phase-probe` tally below. Until v2.9.9 the
        // `mmc3-m2-phase-irq` experiment also used it to defer M2-high rises.
        // The deferral that replaced it (`irq_assert_pending_next_cycle`)
        // needs no phase: the bus's order gives it the same split (see that
        // field's doc and ADR 0002's 2026-10-05 correction).
        #[cfg(not(feature = "mmc3-a12-phase-probe"))]
        let _ = sub_dot;
        if self.variant == Mmc3Variant::McAcc {
            // Falling edges through the divide-by-8 prescaler; the counter
            // clocks on the first edge of each group of eight.
            if self.last_a12 && !level {
                if self.mcacc_prescaler == 0 && self.clock_irq() {
                    self.irq_pending_line = true;
                }
                self.mcacc_prescaler = (self.mcacc_prescaler + 1) & 0x07;
            }
            self.last_a12 = level;
            return;
        }
        if !self.last_a12 && level {
            // Rising edge.
            let gap = self.cpu_cycle.saturating_sub(self.a12_low_cycle);
            // Restructured from the original `if gap >= 3 && self.clock_irq()`
            // into a nested form so the v2.1.5 F5.0 probe can observe the
            // *qualifying* rise (gap accepted) independently of whether it went
            // on to clock the counter. This is behavior-identical: `clock_irq`
            // (which mutates) is still only evaluated when `gap >= 3`, exactly
            // the short-circuit the original `&&` provided.
            if gap >= 3 {
                // v2.1.5 F5.0 instrumentation study (observational only — no
                // emulated-state change): bucket this qualifying A12 rise by the
                // M2-phase half of the host CPU cycle it landed in. See ADR 0002
                // F5.0. `sub_dot` carries real phase data only when the paired
                // `rustynes-core/mmc3-a12-phase-probe` seeds it on the live
                // one-clock scheduler path.
                #[cfg(feature = "mmc3-a12-phase-probe")]
                {
                    if sub_dot >= 2 {
                        self.probe.qual_rises_post += 1;
                    } else {
                        self.probe.qual_rises_pre += 1;
                    }
                }
                if self.clock_irq() {
                    // Of the qualifying rises, count those that actually clocked
                    // the counter into an IRQ-asserting state, by phase half —
                    // the strictest form of the study's question.
                    #[cfg(feature = "mmc3-a12-phase-probe")]
                    {
                        if sub_dot >= 2 {
                            self.probe.irq_clock_post += 1;
                        } else {
                            self.probe.irq_clock_pre += 1;
                        }
                    }
                    // Seen by the CPU from the next cycle on; see the field
                    // doc on `irq_assert_pending_next_cycle`.
                    self.irq_assert_pending_next_cycle = true;
                }
            }
        } else if self.last_a12 && !level {
            // Falling edge.
            self.a12_low_cycle = self.cpu_cycle;
        }
        self.last_a12 = level;
    }

    fn notify_cpu_cycle(&mut self) {
        self.cpu_cycle = self.cpu_cycle.wrapping_add(1);
        if self.irq_assert_pending_next_cycle {
            self.irq_assert_pending_next_cycle = false;
            self.irq_pending_line = true;
        }
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
        // v2.1.5 F5.0 instrumentation study: surface the observational A12-phase
        // tallies so the `mmc3_r1r2_phase_probe` fixture can read them after a
        // run without new trait methods. Present only under the (default-off)
        // probe feature — the shipped `debug_state` is unchanged. See ADR 0002.
        #[cfg(feature = "mmc3-a12-phase-probe")]
        {
            info.extra.push((
                "probe_qual_pre".into(),
                format!("{}", self.probe.qual_rises_pre),
            ));
            info.extra.push((
                "probe_qual_post".into(),
                format!("{}", self.probe.qual_rises_post),
            ));
            info.extra.push((
                "probe_irq_pre".into(),
                format!("{}", self.probe.irq_clock_pre),
            ));
            info.extra.push((
                "probe_irq_post".into(),
                format!("{}", self.probe.irq_clock_post),
            ));
        }
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
        out.push(u8::from(self.irq_assert_pending_next_cycle));
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
        // v3 tail.
        out.push(u8::from(self.mmc6_ram_enabled));
        out.push(self.mmc6_protect);
        out.push(self.mcacc_prescaler);
        out
    }

    #[allow(clippy::too_many_lines)] // tagged-blob deserializer + v1/v2 fork
    fn load_state(&mut self, data: &[u8]) -> Result<(), MapperError> {
        let chr_part = if self.chr_is_ram { self.chr.len() } else { 0 };
        // Tagged scalars laid out below. Only the current version is read
        // (v2.9.8, ADR 0042): v1 and v2 used to load with defaults. v3
        // (v2.9.8) is refused too: its byte 19 was the retired
        // `irq_reload_pending_with_nonzero_clear` latch, which v4 replaces
        // with `irq_assert_pending_next_cycle` (T-ORACLE-001).
        if data.is_empty() {
            return Err(MapperError::WrongLength {
                expected: 1,
                got: 0,
            });
        }
        let version = data[0];
        if version != SAVE_STATE_VERSION {
            return Err(MapperError::UnsupportedVersion(version));
        }
        let tail = 3;
        let scalar_len = 1 + 8 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 1 + 8 + 8 + 1 + 1;
        let expected = scalar_len + self.prg_ram.len() + self.vram.len() + chr_part + tail;
        if data.len() != expected {
            return Err(MapperError::WrongLength {
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
        self.irq_assert_pending_next_cycle = data[cur] != 0;
        cur += 1;
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
            cur += self.chr.len();
        }
        self.mmc6_ram_enabled = data[cur] != 0;
        self.mmc6_protect = data[cur + 1] & 0xF0;
        self.mcacc_prescaler = data[cur + 2] & 0x07;
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
        // First edge: reload to 3.  Edges 2,3,4: decrement to 2,1,0 (assert),
        // visible from the next CPU cycle.
        m.notify_cpu_cycle();
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
    /// then high), then run the CPU cycle after it, from which an IRQ the
    /// rise asserted is visible (`irq_assert_pending_next_cycle`). Used by
    /// the Sharp/NEC reload-to-zero unit tests below.
    fn a12_rise<F: Mapper>(m: &mut F) {
        m.notify_a12(false);
        for _ in 0..4 {
            m.notify_cpu_cycle();
        }
        m.notify_a12(true);
        m.notify_cpu_cycle();
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
    /// Setup: `$C001` clears a non-zero counter; the next A12 rise reloads
    /// it to 0 and Sharp asserts. Whether the cleared counter was non-zero
    /// does not matter (see the next test); before v2.9.9 it did.
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

    /// The page's rule has no "was the cleared counter non-zero" condition:
    /// a `$C001` written while the counter is already 0 still reloads on the
    /// next rise, and a reload to 0 with IRQs enabled asserts on Sharp.
    ///
    /// Until v2.9.9 this test pinned the opposite (a "no-op clear" that
    /// reloaded silently), which existed only to pass `4-scanline_timing`
    /// sub-test 2 and raised that test's IRQ a scanline late
    /// (T-ORACLE-001). The IRQ output deferral passes sub-test 2 under
    /// the page's rule.
    #[test]
    fn sharp_asserts_on_reload_to_zero_after_zero_to_zero_clear() {
        let mut m = fresh(8, 8);
        m.cpu_write(0xC000, 0);
        m.cpu_write(0xC001, 0); // counter already 0
        m.cpu_write(0xE001, 0);
        a12_rise(&mut m);
        assert_eq!(m.irq_counter, 0);
        assert!(
            m.irq_pending(),
            "a reload to 0 asserts on Sharp whatever the cleared counter held"
        );
        m.cpu_write(0xE000, 0);
        m.cpu_write(0xE001, 0);
        a12_rise(&mut m);
        assert!(
            m.irq_pending(),
            "natural reload-to-0 (no pending reload) asserts on Sharp"
        );
    }

    /// NEC (the "alternate" revision) asserts once on a `$C001` reload to 0,
    /// and never on the natural `was_zero` reload. `MMC3.md`: it "generates
    /// only a single IRQ when `$C000` is `$00`", and "writing to `$C001` with
    /// `$C000` still at `$00` will result in another single IRQ"; blargg's
    /// `6-MMC3_alt` fails with "IRQ should be set when reloading due to
    /// clear" otherwise. v3.1.0 corrected this: until then the test pinned
    /// NEC as silent on both paths.
    #[test]
    fn nec_asserts_once_on_a_c001_reload_to_zero_and_not_after() {
        let mut m = Mmc3::new(
            synth_prg(8),
            synth_chr(8),
            Mirroring::Horizontal,
            0,
            Mmc3Revision::Nec,
        )
        .unwrap();
        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        a12_rise(&mut m);
        m.cpu_write(0xC000, 0);
        m.cpu_write(0xC001, 0);
        a12_rise(&mut m);
        assert_eq!(m.irq_counter, 0);
        assert!(m.irq_pending(), "the $C001 reload to 0 asserts on NEC too");
        // Acknowledge, then the natural was_zero reload stays silent.
        m.cpu_write(0xE000, 0);
        m.cpu_write(0xE001, 0);
        assert!(!m.irq_pending());
        a12_rise(&mut m);
        a12_rise(&mut m);
        assert!(!m.irq_pending(), "NEC: the was_zero reload to 0 is silent");
        // A second $C001 write with $C000 still 0: another single IRQ.
        m.cpu_write(0xC001, 0);
        a12_rise(&mut m);
        assert!(m.irq_pending(), "each $C001 write gives one more IRQ");
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
            m.notify_cpu_cycle();
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

    // T-ORACLE-001 (v2.9.9): a clocking A12 rise raises the IRQ line at the
    // first `notify_cpu_cycle` after it, whatever `sub_dot` the rise carries.
    // Which CPU cycle that is comes from the bus's order (see the field doc on
    // `irq_assert_pending_next_cycle`): the next test pins both cases.

    #[test]
    fn irq_becomes_visible_one_cpu_cycle_after_the_rise() {
        for sub_dot in [0u8, 2] {
            let mut m = fresh(8, 8);
            m.cpu_write(0xC000, 1);
            m.cpu_write(0xC001, 0);
            m.cpu_write(0xE001, 0);
            a12_rise(&mut m); // reload to 1, silent
            m.notify_a12_at_sub_dot(false, 0);
            for _ in 0..4 {
                m.notify_cpu_cycle();
            }
            m.notify_a12_at_sub_dot(true, sub_dot);
            assert_eq!(m.irq_counter, 0, "the counter itself moves at the rise");
            assert!(
                !m.irq_pending(),
                "sub_dot {sub_dot}: not visible in the rise's own cycle"
            );
            m.notify_cpu_cycle();
            assert!(
                m.irq_pending(),
                "sub_dot {sub_dot}: visible from the next cycle"
            );
        }
    }

    /// #583 review (Copilot): the deferral is "until the next per-cycle
    /// hook", not "one cycle for every rise". In the bus, `start_cycle` runs
    /// the pre-access PPU catch-up and then the hook, so a rise caught up
    /// there is followed by its own cycle's hook and raised in that cycle,
    /// while a rise caught up after the access waits for the next cycle's.
    /// Both orders, as the mapper sees them.
    #[test]
    fn irq_line_is_raised_at_the_first_cpu_cycle_hook_after_the_rise() {
        let armed = || {
            let mut m = fresh(8, 8);
            m.cpu_write(0xC000, 1);
            m.cpu_write(0xC001, 0);
            m.cpu_write(0xE001, 0);
            a12_rise(&mut m); // reload to 1, silent
            m.notify_a12(false);
            for _ in 0..4 {
                m.notify_cpu_cycle();
            }
            m
        };
        // Pre-access: the rise, then this cycle's hook in `cpu_clock`.
        let mut pre = armed();
        pre.notify_a12(true);
        pre.notify_cpu_cycle();
        assert!(
            pre.irq_pending(),
            "a pre-access rise is raised in its own cycle"
        );
        // Post-access: this cycle's hook already ran; the rise comes after it
        // and is raised only by the next cycle's.
        let mut post = armed();
        post.notify_cpu_cycle();
        post.notify_a12(true);
        assert!(
            !post.irq_pending(),
            "a post-access rise is not raised in its own cycle"
        );
        post.notify_cpu_cycle();
        assert!(post.irq_pending(), "it is raised from the next cycle on");
    }

    #[test]
    fn e000_ack_cancels_an_assertion_in_flight() {
        let mut m = fresh(8, 8);
        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        a12_rise(&mut m);
        m.notify_a12(false);
        for _ in 0..4 {
            m.notify_cpu_cycle();
        }
        m.notify_a12(true); // assertion queued
        assert!(!m.irq_pending());
        m.cpu_write(0xE000, 0); // ack/disable before it lands
        m.notify_cpu_cycle();
        assert!(
            !m.irq_pending(),
            "an ack write must cancel an in-flight assertion"
        );
    }

    #[test]
    fn assertion_in_flight_survives_a_save_state_round_trip() {
        let mut m = fresh(8, 8);
        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        a12_rise(&mut m);
        m.notify_a12(false);
        for _ in 0..4 {
            m.notify_cpu_cycle();
        }
        m.notify_a12(true);
        let state = m.save_state();
        let mut other = fresh(8, 8);
        other.load_state(&state).unwrap();
        assert!(!other.irq_pending());
        other.notify_cpu_cycle();
        assert!(other.irq_pending(), "the queued assertion was restored");
    }

    // ---- v2.9.6: the NES 2.0 submapper variants --------------------------

    fn variant(v: Mmc3Variant) -> Mmc3 {
        Mmc3::new(
            synth_prg(16),
            synth_chr(64),
            Mirroring::Vertical,
            0,
            Mmc3Revision::Sharp,
        )
        .unwrap()
        .with_variant(v)
    }

    /// `MMC6.md`: RAM at `$7000` only, off until `$8000` bit 5, then per
    /// half read/write enables; `$6000-$6FFF` floats.
    #[test]
    fn mmc6_prg_ram_follows_its_own_protect_scheme() {
        let mut m = variant(Mmc3Variant::Mmc6);
        assert_eq!(m.sram().len(), MMC6_RAM, "1 KiB of internal RAM");
        assert!(m.cpu_read_unmapped(0x6000));
        assert!(m.cpu_read_unmapped(0x7000), "RAM disabled at power-on");
        m.cpu_write(0xA001, 0xF0);
        assert!(
            m.cpu_read_unmapped(0x7000),
            "$A001 ignored while $8000.5 = 0"
        );
        m.cpu_write(0x8000, 0x20);
        m.cpu_write(0xA001, 0x30); // low half: read + write
        m.cpu_write(0x7001, 0x11);
        m.cpu_write(0x7201, 0x22); // high half not writable
        assert_eq!(m.cpu_read(0x7001), 0x11);
        assert_eq!(m.cpu_read(0x7201), 0x00, "the unreadable half reads zero");
        assert_eq!(m.cpu_read(0x7401), 0x11, "mirrored every 1 KiB");
        m.cpu_write(0xA001, 0x80); // high half readable only
        m.cpu_write(0x7201, 0x33);
        assert_eq!(m.cpu_read(0x7201), 0x00, "readable but not writable");
        m.cpu_write(0xA001, 0x20); // low half read-only
        m.cpu_write(0x7001, 0x44);
        assert_eq!(m.cpu_read(0x7001), 0x11);
        m.cpu_write(0x8000, 0x00); // disable: $A001 forced to $00
        m.cpu_write(0x8000, 0x20);
        assert!(m.cpu_read_unmapped(0x7000), "the protect bits were cleared");
    }

    #[test]
    fn hardwired_board_ignores_a000() {
        let mut m = variant(Mmc3Variant::HardwiredMirroring);
        m.cpu_write(0xA000, 1);
        assert_eq!(m.current_mirroring(), Mirroring::Vertical);
        let mut s = variant(Mmc3Variant::Standard);
        s.cpu_write(0xA000, 1);
        assert_eq!(s.current_mirroring(), Mirroring::Horizontal);
    }

    /// MC-ACC: falling A12 edges, /8, first edge of each group, `$C001`
    /// resets the prescaler.
    #[test]
    fn mc_acc_counts_falling_edges_through_a_prescaler() {
        let mut m = variant(Mmc3Variant::McAcc);
        m.cpu_write(0xC000, 1);
        m.cpu_write(0xC001, 0);
        m.cpu_write(0xE001, 0);
        let fall = |m: &mut Mmc3| {
            m.notify_a12(true);
            m.notify_a12(false);
        };
        fall(&mut m); // edge 0 of group 0: reload to 1
        assert!(!m.irq_pending());
        for _ in 0..7 {
            fall(&mut m); // edges 1-7: no clock
        }
        assert!(!m.irq_pending());
        fall(&mut m); // edge 0 of group 1: 1 -> 0, IRQ
        assert!(m.irq_pending());
        // `$C001` resets the prescaler. Four edges into a group (latch 1,
        // reloaded on edge 0), a `$C001` makes the next edge a group start:
        // it reloads, and the 9th edge after the write decrements 1 -> 0.
        // Without the reset the group would restart only at the 5th edge and
        // the IRQ would come at the 13th.
        let mut p = variant(Mmc3Variant::McAcc);
        p.cpu_write(0xC000, 1);
        p.cpu_write(0xE001, 0);
        for _ in 0..4 {
            fall(&mut p);
        }
        p.cpu_write(0xC001, 0);
        for _ in 0..8 {
            fall(&mut p);
        }
        assert!(!p.irq_pending());
        fall(&mut p);
        assert!(p.irq_pending(), "the 9th edge after $C001");
        // Rising edges alone never clock it.
        let mut r = variant(Mmc3Variant::McAcc);
        r.cpu_write(0xC000, 0);
        r.cpu_write(0xE001, 0);
        for _ in 0..16 {
            r.notify_a12(true);
            r.notify_cpu_cycle();
            r.notify_cpu_cycle();
            r.notify_cpu_cycle();
            r.notify_cpu_cycle();
        }
        assert!(!r.irq_pending());
    }

    #[test]
    fn variant_state_round_trips_and_v2_is_refused() {
        let mut a = variant(Mmc3Variant::Mmc6);
        a.cpu_write(0x8000, 0x20);
        a.cpu_write(0xA001, 0x30);
        a.cpu_write(0x7003, 0x99);
        let blob = a.save_state();
        let mut b = variant(Mmc3Variant::Mmc6);
        b.load_state(&blob).unwrap();
        assert_eq!(b.cpu_read(0x7003), 0x99);
        assert_eq!(b.save_state(), blob);
        // A v2 blob (no tail) is refused since v2.9.8 (ADR 0042); it used to
        // load with the MMC6 / MC-ACC state at defaults.
        let std_blob = variant(Mmc3Variant::Standard).save_state();
        let mut v2 = std_blob;
        v2[0] = 2;
        v2.truncate(v2.len() - 3);
        assert!(matches!(
            variant(Mmc3Variant::Standard).load_state(&v2),
            Err(MapperError::UnsupportedVersion(2))
        ));
    }
}
