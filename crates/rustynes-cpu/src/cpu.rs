//! Ricoh 2A03 CPU (6502 derivative without BCD mode).
//!
//! See `docs/cpu-6502.md` for the spec. The implementation here matches:
//!
//! - all 151 documented 6502 opcodes,
//! - all 105 unofficial / illegal opcodes that real software depends on,
//! - the 12 JAM / KIL / STP halt opcodes,
//! - cycle counts including page-crossing penalties on indexed reads, the
//!   `+1 if branch taken / +2 if branch crosses page` branch convention, and
//!   the dummy-read / dummy-write cycles of read-modify-write opcodes,
//! - NMI (edge), IRQ (level), and BRK with the documented BRK/IRQ B-flag
//!   distinction,
//! - the `JMP ($XXFF)` indirect page-bug.
//!
//! The CPU steps one *instruction* at a time, returning the cycle count. The
//! `Bus::on_cpu_cycle` callback is invoked once per consumed cycle so the
//! scheduler / test harness can advance the PPU and count cycles. This is
//! sufficient for nestest, blargg `instr_test_v5`, `cpu_timing_test`, and
//! `branch_timing_tests` — the Phase-2 lockstep `tick()` is layered on top in
//! a later sprint without changing this stepping interface.

// Truncating casts are intentional throughout: this module is byte-arithmetic
// against the 6502's 8/16-bit register file. `as u8` / `as i8` is the
// canonical encoding of the wrap behavior the hardware exhibits.
#![allow(
    clippy::cast_possible_truncation,
    clippy::cast_lossless,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss
)]

use crate::bus::Bus;
// `M2Phase` is used only by the legacy lockstep interrupt-sampling paths
// (idle_tick / read1 / write1), which are gated off under the R1 substrate.
use crate::status::Status;

/// Stack base address: the CPU stack lives at `$0100 + S`.
const STACK_BASE: u16 = 0x0100;

// v2.0 master-clock R1 substrate constants (Phase 2; `mc-r1-substrate`).
// The PPU sub-cycle offset and the read/write master-clock split (`pre` in
// start_cycle, `post` in end_cycle). Mesen `_ppuOffset=1` /
// `_startClockCount`/`_endClockCount`. Master clocks per CPU cycle are NOT a
// constant — they are the cartridge region's `cpu_divider` (NTSC 12 / PAL 16 /
// Dendy 15), read from `bus.cpu_divider()` and fed to `read_split`/
// `write_split`; the master-clock unit is shared with the bus's `run_ppu_to`,
// which does the regioned dot conversion off `ppu_divider`.
/// PPU sub-cycle offset: the PPU is run to `master_clock - PPU_OFFSET` in BOTH
/// halves of every access (the double catch-up). Mesen `_ppuOffset = 1`.
const PPU_OFFSET: u64 = 1;

/// The effective PPU-sample offset for `run_ppu_to` (no BP sweep: the constant).
#[inline]
const fn ppu_sample_offset() -> u64 {
    PPU_OFFSET
}
/// READ access master-clock split (`+= pre` in `start_cycle`, `+= post` in
/// `end_cycle`); `pre + post = div`, the region's `cpu_divider`. Derived per
/// region from `bus.cpu_divider()` so PAL (16) / Dendy (15) get the right
/// CPU<->PPU phase; for the NTSC divisor 12 these are exactly (5, 7), so the
/// NTSC path is byte-identical to the prior `const`s.
#[inline]
const fn read_split(div: u64) -> (u64, u64) {
    let pre = div / 2 - PPU_OFFSET;
    (pre, div - pre)
}
/// WRITE access split — swapped (writes commit `2 * PPU_OFFSET` mc later than
/// reads). NTSC divisor 12 → (7, 5), byte-identical to the prior `const`s.
#[inline]
const fn write_split(div: u64) -> (u64, u64) {
    let pre = div / 2 + PPU_OFFSET;
    (pre, div - pre)
}

/// NMI vector low byte address (`$FFFA/B`).
const NMI_VECTOR: u16 = 0xFFFA;

/// Reset vector low byte address (`$FFFC/D`).
const RESET_VECTOR: u16 = 0xFFFC;

/// IRQ / BRK vector low byte address (`$FFFE/F`).
const IRQ_VECTOR: u16 = 0xFFFE;

/// 6502 CPU core.
//
// Multiple boolean state bits track distinct interrupt-pipeline stages
// (jam, NMI pending, NMI armed, IRQ pending, IRQ armed).  These map directly
// onto orthogonal hardware latches; collapsing them into an enum or bitflags
// would obscure rather than clarify the model.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone)]
pub struct Cpu {
    /// Accumulator.
    pub a: u8,
    /// X index register.
    pub x: u8,
    /// Y index register.
    pub y: u8,
    /// Program counter.
    pub pc: u16,
    /// Stack pointer (low byte; effective address `0x0100 | s`).
    pub s: u8,
    /// Processor status.
    pub p: Status,
    /// Cumulative CPU cycle count.
    pub cycles: u64,
    /// `true` when the CPU has executed a JAM/KIL/STP and is waiting for reset.
    pub jammed: bool,
    /// Edge-detected NMI latch.  Real hardware samples the NMI line at the
    /// second-to-last cycle of every instruction; if asserted, the NMI
    /// sequence is queued for AFTER the current instruction completes.  Our
    /// `step` model dispatches all bus operations atomically before ticking
    /// cycles, so a write that itself raises NMI (e.g.  enabling NMI in
    /// `$2000` while VBL is set) is observed by the bus's edge detector
    /// during this instruction's cycle tally.  Hardware would not have
    /// observed it at the second-to-last cycle (the write logically happens
    /// at the LAST cycle), so the NMI is queued for the NEXT instruction's
    /// sample point and serviced AFTER the next instruction completes.
    /// We approximate that by promoting `pending_nmi` to `armed_nmi` after
    /// each instruction; only `armed_nmi` actually services.
    pub(crate) pending_nmi: bool,
    /// NMI ready to be serviced before the next instruction starts.
    pub(crate) armed_nmi: bool,
    /// IRQ pending — captured edge of the level line that fires once the
    /// I-flag is clear.  Same double-latch promotion as NMI.
    pub(crate) pending_irq: bool,
    /// IRQ ready to be serviced before the next instruction starts.
    pub(crate) armed_irq: bool,
    /// First tick (within the current instruction's tally loop) at which
    /// NMI was sampled high; `u8::MAX` if not seen.  Used by the
    /// second-to-last-cycle interrupt classification.
    pub(crate) nmi_first_tick: u8,
    /// First tick at which IRQ was sampled high; `u8::MAX` if not seen.
    pub(crate) irq_first_tick: u8,
    /// Snapshot of the I flag at the *start* of the current instruction.
    /// Hardware samples IRQ near the end of the instruction with the old
    /// I-flag value; CLI / SEI / PLP / RTI take effect at the very last
    /// cycle, AFTER the IRQ sample point.  We arm IRQ only when this
    /// snapshot says I was clear at sample time.
    pub(crate) irq_sample_i_flag: bool,
    /// Number of cycles emitted by the per-cycle helpers
    /// (`read1`/`write1`/`idle_tick`) within the *current* instruction.
    /// Reset to zero at the top of `step()`. Used by the trailing
    /// "burn remaining cycles" loop so we can incrementally migrate
    /// opcodes from atomic-dispatch + trailing-loop to fully per-cycle
    /// emission.
    pub(crate) cycles_emitted: u8,
    /// When `true`, [`Cpu::idle_tick`] does NOT update `irq_first_tick`.
    /// Used by branch opcodes to model the `branch_delays_irq` quirk:
    /// real 6502 branches poll IRQ at the same point a 2-cycle untaken
    /// branch would (the opcode-fetch cycle, which `step()` performs
    /// before entering dispatch).  The operand fetch and any extra
    /// taken / page-cross cycles do *not* re-sample IRQ.  The branch
    /// dispatch sets this flag *before* the operand fetch and `step()`
    /// clears it at the top of every instruction.
    /// NMI sampling is unaffected — the quirk is IRQ-only.
    pub(crate) skip_irq_sample: bool,

    // === v2.0 master-clock R1 substrate (Phase 2; `mc-r1-substrate`) ===
    /// The CPU's authoritative master clock (Mesen `_masterClock` / `TetaNES`
    /// `Cpu::master_clock`). Advanced by `start_cycle`/`end_cycle`; the bus is
    /// caught up to `master_clock - PPU_OFFSET` from BOTH halves (double
    /// catch-up). Only live under `mc-r1-substrate`.
    pub(crate) master_clock: u64,
    /// NMI edge-recognition latch (set on a /NMI rising edge in
    /// `handle_interrupts`; consumed by the cycle-5 hijack in
    /// `service_interrupt`). Mesen `_needNmi`.
    pub(crate) mc_need_nmi: bool,
    /// One-cycle-delayed copy of `mc_need_nmi` (the dispatch + hijack gate).
    /// Mesen `_prevNeedNmi`.
    pub(crate) mc_prev_need_nmi: bool,
    /// Live IRQ-recognition latch (`irq_level && !irq_sample_i_flag`,
    /// recomputed every `end_cycle`). Mesen `_runIrq`.
    pub(crate) mc_run_irq: bool,
    /// One-cycle-delayed copy of `mc_run_irq` (the dispatch gate). Mesen
    /// `_prevRunIrq`.
    pub(crate) mc_prev_run_irq: bool,
    /// Previous /NMI line level, for the φ2 rising-edge detector in
    /// `handle_interrupts`. Mesen `_prevNmiFlag`.
    pub(crate) mc_prev_nmi_line: bool,
    /// v2.0.0 beta.2 (A2 scoping diagnostic): per-opcode count of cycles the
    /// trailing burn-loop had to fill (`cycles - cycles_emitted`) — the exact
    /// remaining busless-cycle surface the every-cycle-bus-access conversion
    /// must turn into dummy reads of the held address. Read by the harness
    /// `burn_probe` bin; never consulted by emulation.
    #[cfg(feature = "cpu-instr-cycle-trace")]
    pub burn_histogram: [u64; 256],
}

impl Default for Cpu {
    fn default() -> Self {
        Self::new()
    }
}

/// Effective address + page-crossed flag, returned by addressing-mode resolvers.
#[derive(Clone, Copy)]
struct Operand {
    addr: u16,
    page_crossed: bool,
}

impl Cpu {
    /// New CPU in "post-reset" state. Caller must invoke [`Cpu::reset`] with a
    /// real bus before stepping (PC is undefined until reset reads `$FFFC/D`).
    ///
    /// This constructor is the convenience entry-point used by unit tests and
    /// nestest fixtures that drive the CPU without going through a full
    /// power-on path: `S=$FD`, `P=$24` (`UNUSED` + `INTERRUPT_DISABLE`). If the
    /// caller subsequently invokes [`Cpu::reset`] the stack pointer will be
    /// decremented by 3 (per the reset sequence), landing on `$FA` — that is
    /// the input shape several `tests/opcodes.rs` fixtures expect.
    ///
    /// **For the real cold-boot path** (`Nes::from_rom`, `Nes::power_cycle`),
    /// use [`Cpu::power_on`] instead, which seeds `S=$00`. After the 3-decrement
    /// reset sequence that lands `S=$FD`, matching Mesen2's power-up state.
    /// See `docs/audit/session-13-cpu-boot-fix-2026-05-21.md` for the reference
    /// behaviour from `Core/NES/NesCpu.cpp::NesCpu::Reset(softReset=false)`.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            a: 0,
            x: 0,
            y: 0,
            pc: 0,
            s: 0xFD,
            p: Status::power_on(),
            cycles: 0,
            jammed: false,
            pending_nmi: false,
            armed_nmi: false,
            pending_irq: false,
            armed_irq: false,
            nmi_first_tick: u8::MAX,
            irq_first_tick: u8::MAX,
            irq_sample_i_flag: true,
            cycles_emitted: 0,
            skip_irq_sample: false,
            master_clock: 0,
            mc_need_nmi: false,
            mc_prev_need_nmi: false,
            mc_run_irq: false,
            mc_prev_run_irq: false,
            mc_prev_nmi_line: false,
            #[cfg(feature = "cpu-instr-cycle-trace")]
            burn_histogram: [0; 256],
        }
    }

    /// New CPU in real-hardware cold-boot state (`S=$00`).
    ///
    /// Real silicon comes up with the stack pointer in an undefined state;
    /// the convention used by Mesen2 (and adopted here for trace parity) is
    /// to treat power-up as `S=$00` and rely on the reset sequence's three
    /// "phantom" decrements to wrap into `S=$FD`. See Mesen2
    /// `Core/NES/NesCpu.cpp::Reset(softReset=false)`:
    ///
    /// ```cpp
    /// if(softReset) {
    ///     _state.SP -= 0x03;          // soft reset path
    /// } else {
    ///     _state.SP = 0xFD;           // power-up: direct assignment
    /// }
    /// ```
    ///
    /// `RustyNES` models that two-path behaviour by gating the SP delta through
    /// the constructor: `Cpu::power_on() + reset()` ⇒ `$00 - 3 = $FD` (cold);
    /// `cpu.reset()` again ⇒ `$FD - 3 = $FA` (subsequent soft reset).
    ///
    /// `P` is left at `$24` (`INTERRUPT_DISABLE` | `UNUSED`). Mesen2's trace
    /// surface shows `P = $04` because it masks `UNUSED` out of the displayed
    /// byte, but the bit is conventionally always set on a 6502 internally
    /// (nesdev: "Bit 5: Always 1, the so-called 'unused' bit"); the trace
    /// divergence on P is cosmetic.
    #[must_use]
    pub const fn power_on() -> Self {
        let mut cpu = Self::new();
        cpu.s = 0x00;
        cpu
    }

    /// Returns `true` when the CPU has executed a JAM/KIL/STP.
    #[must_use]
    pub const fn is_jammed(&self) -> bool {
        self.jammed
    }

    /// Read-only accessor for the CPU's authoritative master clock
    /// (v2.0.0-beta.1 one-clock instrumentation).
    ///
    /// `master_clock` counts master-clock units (NTSC: 12 per CPU cycle,
    /// PAL: 16, Dendy: 15) and is advanced only by `start_cycle` /
    /// `end_cycle` (the asymmetric read 5/7 vs write 7/5 φ1/φ2 split on
    /// NTSC) plus the bus-side DMA coherence fold
    /// (`Bus::take_dma_mc_consumed`). It is the counter the v2.0.0
    /// "Timebase" rewrite (ADR 0002) promotes to the ONE canonical
    /// timebase; the test harness asserts the affine relation
    /// `master_clock == seed + cpu_divider * cycles` against the other
    /// cycle counters (`one_clock_invariants.rs`) as the gate for the
    /// beta.1 counter collapse.
    #[must_use]
    pub const fn master_clock(&self) -> u64 {
        self.master_clock
    }

    /// Reset (warm boot).
    ///
    /// Real hardware: 8-cycle sequence with suppressed pushes, then PC loads
    /// from the reset vector and the I flag is set. We model the cycle count
    /// (advances `cycles` by 8 and fires `on_cpu_cycle` 8 times) without
    /// mutating registers other than P (set I), S (decrement by 3), and PC.
    ///
    /// Matches Mesen2's `NesCpu::Reset()` 8-cycle post-power-up loop ("CPU
    /// takes 8 cycles before it starts executing the ROM's code"). Combined
    /// with the PPU power-up at (scanline=-1, dot=340) (see `Ppu::new`),
    /// this closes the +344-dot PPU offset identified empirically in
    /// Session-13 (docs/audit/session-13-cpu-boot-fix-2026-05-21.md).
    pub fn reset<B: Bus>(&mut self, bus: &mut B) {
        // Real hardware decrements S three times during reset (no actual
        // pushes occur, but the decrements happen).
        self.s = self.s.wrapping_sub(3);
        self.p.insert(Status::INTERRUPT_DISABLE);
        self.jammed = false;
        self.pending_nmi = false;
        self.armed_nmi = false;
        self.pending_irq = false;
        self.armed_irq = false;
        self.nmi_first_tick = u8::MAX;
        self.irq_first_tick = u8::MAX;
        // R1/R3 cold-boot: advance master_clock by one CPU divider BEFORE the
        // 8-cycle reset loop (Mesen `NesCpu::Reset()` `_masterClock += cpuDivider
        // + cpuOffset`). Without it the first start_cycle's `run_ppu_to(mc-1)`
        // would leave the PPU 3 dots behind. See R1 port plan / branch `acddd22`.
        {
            self.master_clock = self.master_clock.wrapping_add(bus.cpu_divider());
        }
        // V-axis: Mesen adds `cpuDivider + cpuOffset` (13), not just cpuDivider
        // (12). The +PPU_OFFSET corrects R1's power-up CPU/PPU sub-cycle
        // alignment to the reference, shifting every $2002 poll-exit to match
        // hardware (nesdev `PPU_frame_timing`: the read sees the flag change iff
        // it starts at/after the set tick). Default-off; A/B against Y/C1/6-10.
        // 8-cycle reset sequence: 6 idle/internal cycles + 2 vector reads.
        self.cycles_emitted = 0;
        for _ in 0..6 {
            self.idle_tick(bus);
        }
        let lo = self.read1(bus, RESET_VECTOR);
        let hi = self.read1(bus, RESET_VECTOR + 1);
        self.pc = u16::from(lo) | (u16::from(hi) << 8);
    }

    /// Force PC to `addr`. Used by the nestest harness which enters at
    /// `$C000` rather than the reset vector.
    pub const fn set_pc(&mut self, addr: u16) {
        self.pc = addr;
    }

    /// Step one instruction (or service an interrupt). Returns the number of
    /// CPU cycles consumed.
    ///
    /// On a JAM-state CPU this is a no-op returning 0.
    ///
    /// # Interrupt timing model
    ///
    /// Real 6502 hardware samples the NMI / IRQ lines at the *second-to-last*
    /// cycle of every instruction and, if asserted there, queues the
    /// interrupt to be serviced *after* the current instruction completes.
    /// Our model dispatches all bus operations atomically before ticking
    /// cycles, so a write that itself raises NMI (e.g. `STA $2000` enabling
    /// NMI while VBL is set) appears to the bus's edge detector during the
    /// FIRST cycle of the tally loop — earlier than hardware would observe
    /// it.  Hardware places the actual write at the *last* cycle of the
    /// instruction, so the second-to-last sample point would NOT see the new
    /// line state; only the NEXT instruction's sample sees it.  We model
    /// that by introducing a one-instruction promotion delay: edges captured
    /// at end-of-step land in `pending_*` and, after the following step,
    /// promote to `armed_*` which is the gate that actually triggers
    /// service.  This passes `04-nmi_control` test 11 ("Immediate occurence
    /// should be after NEXT instruction") without regressing the
    /// instruction-count-insensitive tests like `02-vbl_set_time`,
    /// `09-even_odd_frames`, or any `instr_test_v5` ROM (which never raise
    /// NMI from within a single instruction).
    #[allow(clippy::too_many_lines, clippy::missing_panics_doc)]
    pub fn step<B: Bus>(&mut self, bus: &mut B) -> u8 {
        if self.jammed {
            return 0;
        }
        // v2.0 master-clock R1 UNIFIED interrupt dispatch (`mc-r1-substrate`):
        // a SINGLE service sequence gated on the one-cycle-delayed `prev_*`
        // copies (Mesen `_prevRunIrq || _prevNeedNmi`). The vector is chosen
        // INSIDE `service_interrupt` by the live `mc_need_nmi` at cycle 5 (the
        // NMI hijack); NMI priority is resolved there, not here. Setting
        // `irq_sample_i_flag = true` BEFORE the service masks the φ2 sampler
        // for the 7-cycle sequence (no re-entry). Clears `skip_irq_sample`
        // (a prior taken-branch could have left it set, freezing the recompute)
        // and defers any still-pending NMI by one instruction.
        if self.mc_prev_run_irq || self.mc_prev_need_nmi {
            self.armed_irq = false;
            self.irq_sample_i_flag = true;
            self.skip_irq_sample = false;
            self.service_interrupt(bus, IRQ_VECTOR, false);
            self.mc_prev_need_nmi = false;
            self.promote_post_step_interrupts(7);
            return 7;
        }
        // Service an armed interrupt before the next instruction.  NMI has
        // priority over IRQ; both are mutually exclusive for a single
        // service window.
        // Once armed, the IRQ services unconditionally — the I-flag
        // gating already happened at the sample point (second-to-last
        // cycle of the prior instruction).  This is what produces the
        // "CLI SEI should still allow one IRQ to fire" behavior:
        // SEI's I=1 takes effect at end-of-SEI but the sample at SEI's
        // second-to-last cycle saw I=0 (CLI cleared it) and queued the
        // IRQ, which now fires regardless of the current I-flag.

        // Per-instruction state for the per-cycle helpers
        // (`read1`/`write1`/`idle_tick`).  These track the FIRST tick at
        // which each interrupt line was seen high; hardware samples at the
        // second-to-last cycle so seen < last_tick = arm now;
        // seen == last_tick = defer one instruction (the next instruction's
        // sample window catches it instead).
        self.nmi_first_tick = u8::MAX;
        self.irq_first_tick = u8::MAX;
        self.cycles_emitted = 0;
        // Cleared every instruction; set inside the branch dispatch arms
        // (after the operand fetch / canonical IRQ poll) to suppress
        // further IRQ sampling on the additional taken / page-cross
        // branch cycles.
        self.skip_irq_sample = false;
        // Snapshot the I flag for this instruction.  CLI / SEI / PLP /
        // RTI mutate `self.p` *during* the instruction, but the hardware
        // IRQ sample reads the I value as it was at the start.  This is
        // what produces the documented "exactly one instruction after
        // CLI executes before IRQ is taken" delay.
        self.irq_sample_i_flag = self.p.contains(Status::INTERRUPT_DISABLE);

        #[cfg(feature = "cpu-instr-cycle-trace")]
        bus.trace_instr(self.pc, self.cycles);

        let opcode = self.fetch_pc(bus);
        let mut cycles = 0u8;
        self.dispatch(bus, opcode, &mut cycles);
        // Burn whichever cycles the dispatch did NOT emit through helpers.
        // As opcodes migrate to fully per-cycle emission, this loop runs
        // for fewer iterations; eventually it can be removed entirely.
        //
        // v2.0.0 beta.2 (A2 scoping): the diagnostic histogram below records,
        // per opcode, how many cycles the burn-loop had to fill — the exact
        // empirical work list for the every-cycle-bus-access conversion (the
        // remaining busless cycles that must become dummy reads of the held
        // address). Default-off; the `burn_probe` harness bin prints it.
        #[cfg(feature = "cpu-instr-cycle-trace")]
        {
            let burned = cycles.saturating_sub(self.cycles_emitted);
            if burned > 0 {
                self.burn_histogram[opcode as usize] =
                    self.burn_histogram[opcode as usize].saturating_add(u64::from(burned));
            }
        }
        // v2.0.0 beta.2 (A2): with the one-clock feature ON, every
        // instruction cycle is a bus access — the resolvers + RMW arms emit
        // the canonical dummy reads, so the burn-loop must never fire.
        // Proven empirically at zero across AccuracyCoin, nestest, both
        // blargg_nes_cpu_test5 suites, and cpu_timing_test6 (the full
        // official + unofficial opcode space); this assert makes any future
        // under-emitting dispatch arm fail loud in dev-profile runs instead
        // of silently reintroducing a busless cycle.
        #[cfg(feature = "mc-one-clock-v2")]
        debug_assert!(
            self.cycles_emitted >= cycles,
            "opcode ${opcode:02X} under-emitted: declared {cycles} cycles but emitted \
             only {} — a busless burn-loop cycle would fill the gap (A2 regression; \
             see the v2.0.0 plan Workstream A2)",
            self.cycles_emitted
        );
        while self.cycles_emitted < cycles {
            self.idle_tick(bus);
        }
        self.promote_post_step_interrupts(cycles);
        cycles
    }

    /// Promote any per-instruction interrupt edges captured by the
    /// per-cycle helpers into the `armed_*` / `pending_*` latches the
    /// next [`Cpu::step`] consults.  Hardware samples interrupts at the
    /// second-to-last cycle of an instruction; we approximate that with
    /// "first sampled tick strictly before the last cycle = arm now,
    /// else defer one instruction."
    const fn promote_post_step_interrupts(&mut self, cycles: u8) {
        // Promote any previously-pending interrupt (latched at the very
        // last cycle of the prior instruction).
        if self.pending_nmi {
            self.armed_nmi = true;
            self.pending_nmi = false;
        }
        if self.pending_irq {
            self.armed_irq = true;
            self.pending_irq = false;
        }
        let last_tick = cycles.saturating_sub(1);
        if self.nmi_first_tick != u8::MAX {
            if self.nmi_first_tick < last_tick {
                self.armed_nmi = true;
            } else {
                self.pending_nmi = true;
            }
        }
        if self.irq_first_tick != u8::MAX {
            // IRQ is masked by the I-flag value as it was at the START of
            // this instruction; CLI / SEI / PLP / RTI mutations take effect
            // at end-of-instruction.  If IRQ was already disabled when we
            // entered, the second-to-last-cycle sample sees I=1 and the
            // edge is dropped (the next instruction's sample will pick it
            // up if I has since cleared).
            if !self.irq_sample_i_flag {
                if self.irq_first_tick < last_tick {
                    self.armed_irq = true;
                } else {
                    self.pending_irq = true;
                }
            }
        }
    }

    fn fetch_pc<B: Bus>(&mut self, bus: &mut B) -> u8 {
        let v = self.read1(bus, self.pc);
        self.pc = self.pc.wrapping_add(1);
        v
    }

    fn fetch_pc_u16<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.fetch_pc(bus);
        let hi = self.fetch_pc(bus);
        u16::from(lo) | (u16::from(hi) << 8)
    }

    fn read_u16_with_wrap<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u16 {
        // Used by indirect modes to honor the 6502 page-wrap quirk.
        let lo = self.read1(bus, addr);
        let hi_addr = (addr & 0xFF00) | u16::from((addr as u8).wrapping_add(1));
        let hi = self.read1(bus, hi_addr);
        u16::from(lo) | (u16::from(hi) << 8)
    }

    fn push<B: Bus>(&mut self, bus: &mut B, value: u8) {
        self.write1(bus, STACK_BASE | u16::from(self.s), value);
        self.s = self.s.wrapping_sub(1);
    }

    fn pull<B: Bus>(&mut self, bus: &mut B) -> u8 {
        self.s = self.s.wrapping_add(1);
        self.read1(bus, STACK_BASE | u16::from(self.s))
    }

    fn push_u16<B: Bus>(&mut self, bus: &mut B, value: u16) {
        self.push(bus, (value >> 8) as u8);
        self.push(bus, (value & 0xFF) as u8);
    }

    fn pull_u16<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let lo = self.pull(bus);
        let hi = self.pull(bus);
        u16::from(lo) | (u16::from(hi) << 8)
    }

    // ------------------------------------------------------------------
    // Per-cycle bus interleaving primitives.
    //
    // Real 6502 hardware reads or writes the bus *exactly once per CPU
    // cycle*; "internal" cycles (ALU work, stack pointer increment, etc.)
    // still tick the system clock without driving the address bus.
    // `read1` / `write1` / `idle_tick` model that one-cycle granularity:
    // each one ticks the bus exactly once and samples the NMI/IRQ lines
    // at the end of the cycle, mirroring what the existing trailing
    // tally loop in `step` did but with the polling now coupled to the
    // *actual* memory access ordering.
    //
    // `step()` resets `cycles_emitted` to 0; each call here increments
    // it.  The trailing burn-loop in `step` consumes whatever cycles
    // the opcode declared but didn't emit through these helpers.
    // ------------------------------------------------------------------

    // === v2.0 master-clock R1 substrate core (Phase 2; `mc-r1-substrate`) ===

    /// Start half of one CPU cycle: advance `master_clock` by the PRE split,
    /// catch the PPU up to `master_clock - PPU_OFFSET`, then fire the bus's
    /// per-cycle work (`cpu_clock`). After this the PPU is at the access's
    /// exact master clock (so a `$2002`/`$2007` read sees on-time state).
    fn start_cycle<B: Bus>(&mut self, bus: &mut B, for_read: bool) {
        let div = bus.cpu_divider();
        let pre = if for_read {
            read_split(div).0
        } else {
            write_split(div).0
        };
        self.master_clock = self.master_clock.wrapping_add(pre);
        bus.run_ppu_to(self.master_clock.saturating_sub(ppu_sample_offset()));
        bus.cpu_clock();
        // v2.0.0 beta.1 (A1 one-clock collapse): `cycles` is ASSIGNED from the
        // canonical bus cycle counter at this single per-cycle site instead of
        // being independently incremented by every `read1`/`write1`/
        // `idle_tick`/DMA-loop caller. `bus.cpu_clock()` above advanced the
        // canonical counter for THIS cycle, so the assignment lands on the
        // same post-increment value the caller-side `+= 1` produced (the
        // `one_clock_invariants` harness test pins the residue at zero).
        #[cfg(feature = "mc-one-clock-v2")]
        {
            self.cycles = bus.cycle_count();
        }
    }

    /// End half of one CPU cycle: fold any bus-side DMA span into
    /// `master_clock` (coherence — keeps the CPU<->PPU phase aligned across a
    /// DMA), advance by the POST split, catch the PPU up again (the double
    /// catch-up), then sample interrupts (φ2, the T_last-1 rule).
    fn end_cycle<B: Bus>(&mut self, bus: &mut B, for_read: bool) {
        // v2.0.0 beta.1 (A1 one-clock collapse): the `dma_mc_consumed`
        // coherence fold is RETIRED. On the live unified-DMA path every DMA
        // cycle is a first-class `start_cycle`/`end_cycle` (advancing
        // `master_clock` directly), so the bus-side accumulator is
        // structurally zero — the fold only ever mattered for the legacy
        // bus-side burst engine, which is dead code. The accumulator is
        // drained UNCONDITIONALLY (identical dev/release behavior — clippy's
        // `debug_assert_with_mut_call` rightly forbids the take inside the
        // assertion) and the structural-zero claim is asserted in dev
        // profiles; the flag-on byte-identity gate (AccuracyCoin 139/139 +
        // nestest 0-diff) proves it for release.
        #[cfg(feature = "mc-one-clock-v2")]
        {
            let folded = bus.take_dma_mc_consumed();
            debug_assert_eq!(
                folded, 0,
                "dma_mc_consumed accumulated on the live path — a legacy \
                 bus-side DMA cycle ran outside the unified engine (see the \
                 v2.0.0 plan A1)"
            );
            let _ = folded;
        }
        #[cfg(not(feature = "mc-one-clock-v2"))]
        {
            self.master_clock = self.master_clock.wrapping_add(bus.take_dma_mc_consumed());
        }
        let div = bus.cpu_divider();
        let post = if for_read {
            read_split(div).1
        } else {
            write_split(div).1
        };
        self.master_clock = self.master_clock.wrapping_add(post);
        bus.run_ppu_to(self.master_clock.saturating_sub(ppu_sample_offset()));
        // F-2: tick the DMC byte-timer at END of cycle (after the access),
        // matching main's DMC fire-phase for DMASync, BEFORE the φ2 interrupt
        // sample so handle_interrupts sees the post-tick DMC IRQ line.
        bus.cpu_clock_apu_dmc();
        self.handle_interrupts(bus);
        // Diagnostic trace hook (no-op unless the bus enables irq-timing-trace).
        bus.trace_end_cycle();
    }

    /// φ2 interrupt sampler (Mesen `EndCpuCycle`): edge-detect /NMI into
    /// `mc_need_nmi` (after copying the one-cycle-delayed `mc_prev_need_nmi`),
    /// and recompute `mc_run_irq = irq_level && !irq_sample_i_flag` (after the
    /// `mc_prev_run_irq` copy). The `step()`-top dispatch reads the `prev_*`
    /// copies — i.e. second-to-last-cycle recognition. The I-mask uses the
    /// start-of-instruction snapshot (`irq_sample_i_flag`), not live `self.p`,
    /// so CLI/SEI/PLP delay their I-change one instruction.
    #[allow(clippy::needless_pass_by_ref_mut)] // &mut B for signature parity
    fn handle_interrupts<B: Bus>(&mut self, bus: &mut B) {
        self.mc_prev_need_nmi = self.mc_need_nmi;
        let nmi_level = bus.nmi_level();
        if !self.mc_prev_nmi_line && nmi_level {
            self.mc_need_nmi = true;
        }
        self.mc_prev_nmi_line = nmi_level;
        self.mc_prev_run_irq = self.mc_run_irq;
        // W1 (`mc-r1-branch-poll-points`): a taken branch polls IRQ ONCE —
        // before C2 — so while `skip_irq_sample` is set (the branch dispatch
        // arms set it after the C1 opcode fetch, before the C2 operand fetch)
        // the recognition latch is FROZEN at its end-of-C1 value instead of
        // recomputed from the live line every cycle. DMC-DMA halt cycles
        // drained inside the branch's own `read1`/`idle_tick` therefore
        // cannot make a freshly-asserted IRQ visible to THIS instruction
        // (AccuracyCoin `Interrupt flag latency` Test A; TriCNES polls at
        // C2-start only, plus a can-set poll at C4-start handled in
        // `branch()`). The `mc_prev_run_irq` copy above still runs, so the
        // held end-of-C1 value is what the next `step()` dispatch reads. NMI
        // edge detection above is untouched (sampled every cycle, per
        // hardware — the quirk is IRQ-only).
        if self.skip_irq_sample {
            return;
        }
        let irq_level = bus.irq_level();
        self.mc_run_irq = irq_level && !self.irq_sample_i_flag;
    }

    /// Tick the bus once and sample interrupt lines, *without* a bus
    /// access.  Models a 6502 internal cycle.
    fn idle_tick<B: Bus>(&mut self, bus: &mut B) {
        {
            // F-2 re-coupling (`mc-r1-dmc-idle-halt`): a DMC DMA can halt the CPU
            // on a 6502 INTERNAL cycle too — on hardware every cycle is a bus
            // read, and Mesen's `ProcessPendingDma` runs on every `MemoryRead`
            // (incl. dummy reads), NOT only instruction/operand reads. R1's
            // `read1` loop only services on real reads, so the DMA waits through
            // internal cycles (the `lat=4` idle-runs the per-fetch trace pinned
            // as the period-jitter source). Service it here too, on the held
            // (last-read) bus address. Default-off; the banked 6/10 path skips it.
            // W3-Stage-1 (`mc-r1-dma-unified`): the unified-engine replacement
            // for the idle DMC drain above — same loop shape, ONE engine. The
            // bus supplies the held (last-read) address for the parked 6502
            // address bus. Same budget accounting as the loop it replaces.
            while bus.unified_dma_pending() {
                self.cycles_emitted = self.cycles_emitted.saturating_add(1);
                #[cfg(not(feature = "mc-one-clock-v2"))]
                {
                    self.cycles = self.cycles.wrapping_add(1);
                }
                self.start_cycle(bus, true);
                bus.unified_dma_cycle_idle();
                self.end_cycle(bus, true);
            }
            // R1: a pure internal cycle — busless (idle_tick stays busless).
            self.cycles_emitted = self.cycles_emitted.saturating_add(1);
            #[cfg(not(feature = "mc-one-clock-v2"))]
            {
                self.cycles = self.cycles.wrapping_add(1);
            }
            self.start_cycle(bus, true);
            self.end_cycle(bus, true);
        }
    }

    /// Canonical cycle-2 PC dummy read for implied / accumulator /
    /// transfer / flag instructions (per nesdev `6502_cpu.txt` + MOS
    /// 6502 datasheet). Real silicon fetches the byte AFTER the opcode
    /// during cycle 2 of these single-byte instructions and discards
    /// it (the would-be operand). Without this dummy read, our emulator
    /// instead "burns an idle cycle" via `idle_tick` for the second
    /// cycle, which counts the cycle for time but produces no bus
    /// access — diverging from real silicon's bus-access pattern.
    ///
    /// Wired into 22 dispatch arms (ASL/LSR/ROL/ROR A; CLC/SEC/CLI/SEI/
    /// CLV/CLD/SED; TAX/TAY/TSX/TXA/TXS/TYA; INX/DEX/INY/DEY; NOP;
    /// 6 unofficial 1-byte NOPs) under the `cpu-implied-dummy-reads`
    /// cargo feature. Default-off pending the coordinated DMC scheduler
    /// audit per `docs/audit/sprint-2.3-implied-dummy-dmc-recon-2026-05-25.md`
    /// — Session-19 documented that this fix alone (Step 1+2 of the
    /// recipe) cascades into `Implicit DMA Abort [error 2]`. Step 3
    /// (DMC scheduler awareness of cycle-2 bus-active reads) is the
    /// next-session attack.
    ///
    /// When the feature flag is OFF, this helper compiles to a no-op
    /// (the `bus` parameter is silenced via `_ = bus`), and the
    /// existing `*cycles = 2` + caller's idle-tick burn loop preserves
    /// pre-Sprint-2.3 behavior byte-identically.
    // `&mut self` + `&mut bus` are required for the feature-ON branch;
    // when the feature is off the helper is a no-op (cfg-gated). The
    // lint suppressions cover the OFF branch's "unused argument /
    // could be const fn / inline(always) is suspicious" complaints.
    #[inline(always)]
    #[allow(
        clippy::inline_always,
        clippy::needless_pass_by_ref_mut,
        clippy::unused_self,
        clippy::missing_const_for_fn
    )]
    fn implied_dummy_read<B: Bus>(&mut self, bus: &mut B) {
        {
            let _ = self.read1(bus, self.pc);
        }
    }

    /// Read a byte at `addr` *and* consume one CPU cycle (with bus tick
    /// + interrupt sampling).
    #[allow(clippy::too_many_lines)] // mc-r1 DMA-interleave arms push this past 100
    fn read1<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        {
            // accuracycoin-100 Phase 2 (`mc-r1-dmc-abort-cancel`): a 1-byte
            // non-looping implicit abort that matured during the prior APU tick
            // is serviced HERE, before the (cancelled) reload could run. On a
            // GET (read) cycle the abort is a 1-cycle DMA (one halt re-read) →
            // CalculateDMADuration Y=1; on a PUT cycle it does NOT occur → Y=0
            // ("the 1-cycle abort will not land on a write cycle"). Both clear
            // the pending reload so the `dmc_dma_pending` loop below skips it.
            if bus.dmc_abort_pending() {
                if bus.dmc_abort_is_get_cycle() {
                    self.cycles_emitted = self.cycles_emitted.saturating_add(1);
                    #[cfg(not(feature = "mc-one-clock-v2"))]
                    {
                        self.cycles = self.cycles.wrapping_add(1);
                    }
                    self.start_cycle(bus, true);
                    bus.dmc_abort_halt_step(addr);
                    self.end_cycle(bus, true);
                } else {
                    bus.dmc_abort_cancel();
                }
            }
            // W3-Stage-1 (`mc-r1-dma-unified`): ONE DMA loop replacing the
            // three loops below (the standalone DMC drain, the sequential
            // Stage-D OAM loop, and the Program-M overlap loop). Each
            // iteration is one full R1 cycle (start_cycle -> the bus's
            // unified TriCNES-dispatch cycle -> end_cycle), so every DMA
            // cycle keeps the φ2 IRQ sample — the C1-safe shape. The
            // engine itself (ONE driver for standalone DMC, standalone OAM,
            // and the overlap) lives bus-side in `unified_dma_cycle`; the
            // load-get-entry defer is folded into `unified_dma_pending`
            // (pre-cycle, like the floor's while-gate) AND the engine's
            // in-cycle entry gate (post-flip parity).
            while bus.unified_dma_pending() {
                // DMA halt cycles count against `cycles_emitted`.
                self.cycles_emitted = self.cycles_emitted.saturating_add(1);
                #[cfg(not(feature = "mc-one-clock-v2"))]
                {
                    self.cycles = self.cycles.wrapping_add(1);
                }
                self.start_cycle(bus, true);
                bus.unified_dma_cycle(addr);
                self.end_cycle(bus, true);
            }
            // Phase B (interleaved DMC DMA): a DMC DMA halts the CPU only on a
            // READ cycle (TriCNES `CPU_Read`). While one is pending, consume R1
            // cycles ONE AT A TIME — each a full R1 cycle (PPU caught up,
            // `tick_dmc` advances the DMC timer once = the span↔fire feedback,
            // arm gated by `in_dmc_dma` = no cascade) — BEFORE the CPU's own
            // read. `dmc_dma_step` re-reads `addr` on halt/align cycles and
            // fetches the sample on the get cycle (`!put_cycle`).
            // W3-Stage-0: when an OAM DMA can overlap this DMC
            // (`oam_dma_overlap_ready` — under `mc-r1-counter-collapse` that
            // includes a `$4014` write still pending its first cycle), do NOT
            // drain the DMC standalone here: the combined overlap loop below
            // services both engines as ONE shared-cycle event. Draining it here
            // first pays a full unshared reload span = the DMC+OAM idx[7]
            // regime-transition `03`. Without the overlap feature this bus query
            // is the trait default (`oam_dma_in_flight` = `false` at read1 entry),
            // so the floor path is unchanged by construction.
            // W3-Stage-1: replaced by the unified engine loop above under
            // `mc-r1-dma-unified` (cfg'd out, not deleted).
            // Stage-D (`mc-r1-full-cpu`): OAM DMA runs CPU-driven, one cycle at a
            // time through start_cycle/end_cycle (so each OAM cycle samples
            // IRQ/NMI via the φ2 `_prev*` pipeline in end_cycle — the surface the
            // bus burst bypassed and RW-2 regressed). A pending DMC DMA preempts
            // (the DMC loop above already drained first = DMC-get-before-OAM-get).
            //
            // NOTE: this SEQUENTIAL nested form drains a mid-OAM DMC DMA fully
            // before resuming OAM (no overlap), so the DMC+OAM test's `02/01`
            // shared-cycle entries never appear. `mc-r1-dmc-oam-overlap` replaces
            // it with the overlap model below.
            // Program M (M-2, `mc-r1-dmc-oam-overlap`): the DMC-DMA-during-OAM-DMA
            // overlap model. A single combined loop services BOTH engines per
            // cycle: when a DMC DMA is pending while an OAM DMA is IN FLIGHT, the
            // DMC halt/dummy/align cycles SHARE an OAM cycle (the 6502 is
            // RDY-halted but the OAM engine keeps consuming its bus slot), and
            // only the DMC GET steals an OAM slot. This is the per-cycle analogue
            // of lockstep `service_dmc_dma_during_oam`, which produces the test's
            // canonical `04,03,...,02,01` sweep (nesdev `DMA#DMC_DMA_during_OAM`).
            // R1 clean access shape: start_cycle (PPU caught up to the
            // access's exact mc + bus cpu_clock) → bus.read → end_cycle
            // (double catch-up + φ2 interrupt sample). Mesen `MemoryRead`.
            self.cycles_emitted = self.cycles_emitted.saturating_add(1);
            #[cfg(not(feature = "mc-one-clock-v2"))]
            {
                self.cycles = self.cycles.wrapping_add(1);
            }
            self.start_cycle(bus, true);
            let v = bus.read(addr);
            self.end_cycle(bus, true);
            v
        }
    }

    /// Write `value` to `addr` *and* consume one CPU cycle (with bus
    /// tick + interrupt sampling).
    fn write1<B: Bus>(&mut self, bus: &mut B, addr: u16, value: u8) {
        {
            // accuracycoin-100 Phase 2: a CPU write cycle cannot be RDY-halted,
            // so a 1-byte implicit abort matured before a write does NOT occur
            // (Y=0). Cancel it with no halt cycle — this is the "will not land on
            // a write cycle" half of the sweep that the read-only `read1` path
            // can't reach.
            if bus.dmc_abort_pending() {
                bus.dmc_abort_cancel();
            }
            // R1 clean write shape (symmetric split — writes commit 2 mc
            // later than reads). No interrupt sample latches here; end_cycle's
            // handle_interrupts does the φ2 sample.
            self.cycles_emitted = self.cycles_emitted.saturating_add(1);
            #[cfg(not(feature = "mc-one-clock-v2"))]
            {
                self.cycles = self.cycles.wrapping_add(1);
            }
            self.start_cycle(bus, false);
            bus.write(addr, value);
            self.end_cycle(bus, false);
        }
    }

    /// SH* unstable-store family helper (`SHA / SHX / SHY / SHS / TAS`,
    /// opcodes `$9F / $93 / $9E / $9C / $9B`).
    ///
    /// Faithful port of Mesen2's `SyaSxaAxa` (`Core/NES/NesCpu.h` lines
    /// 716-745).  Implements the canonical 6502-derivative
    /// unstable-store algorithm:
    ///
    /// 1. Compute the page-crossed flag against `base + index_reg`.
    /// 2. Perform a dummy read at the **unfixed** address
    ///    (`base + index_reg - 0x100` if page-crossed, else
    ///    `base + index_reg`).  This is the cycle DMC DMA can
    ///    interrupt.
    /// 3. Detect DMC-DMA interruption via `bus.cycle_count()`
    ///    before/after the dummy read — if more than 1 bus cycle
    ///    elapsed, a DMA fired.
    /// 4. On page-cross, the address-high-byte is corrupted to
    ///    `original_addr_high AND value_reg`.
    /// 5. Compute the store value:
    ///    - With DMA: just `value_reg` (the H+1 AND is suppressed
    ///      because the DMC pulled the bus low).
    ///    - Without DMA: `value_reg AND ((base >> 8) + 1)`.
    /// 6. Write to the (possibly corrupted) final address.
    ///
    /// This shape is what `AccuracyCoin Unofficial Instructions: SH*`
    /// sub-test 7 ("the cycle before the write had a DMA") brackets.
    /// Pre-2026-05-23 `RustyNES` skipped the dummy read entirely and
    /// always wrote `value_reg & (H+1)`, failing sub-test 7 across
    /// all 5 SH* opcodes (error code 7).
    fn sh_store<B: Bus>(&mut self, bus: &mut B, base: u16, index_reg: u8, value_reg: u8) {
        let addr = base.wrapping_add(u16::from(index_reg));
        let page_crossed = (base & 0xFF00) != (addr & 0xFF00);

        // Dummy read at the unfixed address (this is the cycle DMC
        // DMA can halt).  We sample the bus-side cycle count before
        // and after to detect interruption — DMC DMA service path
        // advances `bus.cycle` by 3+ extra ticks while the CPU's
        // own `Cpu::cycles` (which `idle_tick` increments) only goes
        // up by 1 for the read itself.
        let cyc_before = bus.cycle_count();
        let dummy_addr = if page_crossed {
            addr.wrapping_sub(0x100)
        } else {
            addr
        };
        let _dummy = self.read1(bus, dummy_addr);
        let had_dma = bus.cycle_count().wrapping_sub(cyc_before) > 1;

        let addr_high = (addr >> 8) as u8;
        let addr_low = (addr & 0xFF) as u8;
        let final_high = if page_crossed {
            addr_high & value_reg
        } else {
            addr_high
        };

        let write_value = if had_dma {
            // DMC DMA interrupted the dummy read — bus latch was
            // overwritten by the DMC fetch, so the store value loses
            // its AND-with-(H+1) component.  Per Mesen2 `SyaSxaAxa`.
            value_reg
        } else {
            // Canonical "documented behavior 1" path: AND with
            // (base_high + 1).
            value_reg & ((base >> 8) as u8).wrapping_add(1)
        };

        let final_addr = (u16::from(final_high) << 8) | u16::from(addr_low);
        self.write1(bus, final_addr, write_value);
    }

    fn service_interrupt<B: Bus>(&mut self, bus: &mut B, vector: u16, brk: bool) {
        // Per-cycle interrupt sequence (7 cycles total when entered from
        // an interrupt edge, 6 from BRK because its opcode fetch already
        // burned cycle 1):
        //   C1: opcode fetch (BRK only — IRQ/NMI skip this and instead
        //       perform an extra dummy read in C2).
        //   C2: dummy read of PC+1 (BRK)  /  filler/internal read (IRQ/NMI).
        //   C3-C5: push PCH, PCL, P.
        //   C6-C7: read vector lo, hi.
        // `cycles_emitted` is reset here so the caller's accounting starts
        // from this routine's first tick (the opcode-fetch tick from a BRK
        // is harmless — the BRK arm sets *cycles = 0 to suppress the
        // trailing burn loop).
        self.cycles_emitted = 0;
        // Reset the per-instruction interrupt sample latches so any NMI
        // edge during the push sequence below is captured here.
        self.nmi_first_tick = u8::MAX;
        self.irq_first_tick = u8::MAX;
        // Two filler reads for IRQ/NMI; one for BRK (the opcode fetch
        // counted as the other).
        //
        // W3-Stage-3 Part B (`mc-r1-brk-padding-read`): BRK's C2 is the
        // canonical PADDING-BYTE read at PC+1 — a REAL bus access on
        // silicon, not an internal cycle. AccuracyCoin `Implied Dummy
        // Reads` error 31 brackets exactly this: the test choreographs a
        // BRK whose padding read lands on `$4015`, which must clear the
        // frame-counter IRQ flag (RTI/RTS already emit their canonical
        // reads under `cpu-stack-dummy-reads`; BRK was the one gap — with
        // it the whole test PASSES, one sub-check beyond Mesen2's error
        // 34). The dispatch arm has already advanced PC past the padding
        // byte, so it sits at `pc - 1`. IRQ/NMI keep their filler idle
        // ticks (the C1 trio canary is on that path). Own flag (NOT
        // `cpu-stack-dummy-reads`, which sits inside the `mc-r1-full-cpu`
        // floor) so the floor stays byte-identical.
        if brk {
            let _ = self.read1(bus, self.pc.wrapping_sub(1));
        } else {
            // v2.0.0 beta.2 (A2 every-cycle-bus-access): canonical hardware
            // IRQ/NMI cycles 1-2 are DUMMY READS of the interrupted PC (the
            // suppressed opcode fetch + suppressed operand fetch — nesdev
            // `6502_cpu.txt`; Mesen2 `NesCpu::IRQ` issues two `DummyRead`s).
            // This is the C1-trio canary path: the conversion keeps the exact
            // same two-cycle start/end structure (φ2 samples unchanged) and
            // only adds the bus access + held-address update; the
            // cpu_interrupts_v2 5/5 strict gate + AccuracyCoin 139/139 must
            // hold with the flag on (verified at the beta.2 gate).
            #[cfg(feature = "mc-one-clock-v2")]
            {
                let _ = self.read1(bus, self.pc);
                let _ = self.read1(bus, self.pc);
            }
            // Flag-off: the shipped build keeps the busless filler ticks
            // byte-identically.
            #[cfg(not(feature = "mc-one-clock-v2"))]
            {
                self.idle_tick(bus);
                self.idle_tick(bus);
            }
        }
        self.push_u16(bus, self.pc);
        let mut p = self.p | Status::UNUSED;
        if brk {
            p.insert(Status::BREAK);
        } else {
            p.remove(Status::BREAK);
        }
        self.push(bus, p.bits());
        self.p.insert(Status::INTERRUPT_DISABLE);
        // NMI hijacking: real 6502 latches the vector to read on the
        // CYCLE just before the vector reads; if NMI is asserted at that
        // point, BRK / IRQ both read $FFFA / $FFFB instead of the
        // declared vector.  We approximate "NMI asserted by now" with
        // "the NMI sample latch was hit during cycles 1..=5 of this
        // sequence."
        // R1: the hijack reads the DELAYED `mc_prev_need_nmi`, NOT the live
        // `mc_need_nmi` — an NMI edge latched ON the P-push cycle's φ2 sampler
        // must NOT hijack (the BRK/IRQ completes to its own vector and the NMI
        // is taken after one handler instruction). `mc_prev_need_nmi` is the
        // pre-edge value: 1 only if the NMI was pending BEFORE this cycle
        // (oracle-derived, cpu_interrupts_v2/2). Legacy uses `nmi_first_tick`.
        let effective_vector = if self.mc_prev_need_nmi && vector != NMI_VECTOR {
            self.mc_need_nmi = false;
            self.mc_prev_need_nmi = false;
            NMI_VECTOR
        } else {
            vector
        };
        // Phase 1.2 of Track C1 attempt 14: notify the bus of the vector
        // fetch BEFORE the low-byte read so the trace records the cycle
        // at which the CPU enters its vector-fetch micro-op (C6 of the
        // 7-cycle service sequence).  `is_nmi` distinguishes a clean NMI
        // service entry from an IRQ/BRK service entry that an NMI edge
        // has hijacked to `$FFFA` — both fetch from `$FFFA` but only one
        // has `vector == NMI_VECTOR` at this call site.
        bus.notify_irq_service(effective_vector, vector == NMI_VECTOR);
        let lo = self.read1(bus, effective_vector);
        let hi = self.read1(bus, effective_vector + 1);
        self.pc = u16::from(lo) | (u16::from(hi) << 8);
    }

    // ------------------------------------------------------------------
    // Addressing-mode resolvers. Each returns the effective address plus a
    // page-crossed flag; the caller decides whether to add a cycle.
    // ------------------------------------------------------------------

    fn addr_zp<B: Bus>(&mut self, bus: &mut B) -> Operand {
        Operand {
            addr: u16::from(self.fetch_pc(bus)),
            page_crossed: false,
        }
    }

    fn addr_zp_x<B: Bus>(&mut self, bus: &mut B) -> Operand {
        let base = self.fetch_pc(bus);
        // v2.0.0 beta.2 (A2 every-cycle-bus-access): canonical 6502 cycle 3
        // reads the UN-indexed zero-page address while the index add
        // completes, then discards it (nesdev `6502_cpu.txt`; Mesen2 models
        // it as a real `MemoryRead`). Zero-page addresses are always RAM
        // ($0000-$00FF), so the read is register-side-effect-free — but it
        // parks a real address on the bus (the held address a DMA halt
        // re-reads) instead of leaving the cycle busless in the burn-loop.
        // The burn-probe histogram pinned this family as 99% of the
        // remaining busless surface ($95 STA zp,X alone = 8,955 of 9,795
        // burned cycles over the AccuracyCoin battery). Default-off: the
        // shipped build keeps the busless burn-loop fill byte-identically.
        #[cfg(feature = "mc-one-clock-v2")]
        {
            let _ = self.read1(bus, u16::from(base));
        }
        Operand {
            addr: u16::from(base.wrapping_add(self.x)),
            page_crossed: false,
        }
    }

    fn addr_zp_y<B: Bus>(&mut self, bus: &mut B) -> Operand {
        let base = self.fetch_pc(bus);
        // A2: same canonical un-indexed dummy read as `addr_zp_x` (cycle 3
        // of LDX/STX zp,Y and the unofficial LAX/SAX zp,Y arms).
        #[cfg(feature = "mc-one-clock-v2")]
        {
            let _ = self.read1(bus, u16::from(base));
        }
        Operand {
            addr: u16::from(base.wrapping_add(self.y)),
            page_crossed: false,
        }
    }

    fn addr_abs<B: Bus>(&mut self, bus: &mut B) -> Operand {
        Operand {
            addr: self.fetch_pc_u16(bus),
            page_crossed: false,
        }
    }

    fn addr_abs_x<B: Bus>(&mut self, bus: &mut B) -> Operand {
        let base = self.fetch_pc_u16(bus);
        let addr = base.wrapping_add(u16::from(self.x));
        let page_crossed = (base & 0xFF00) != (addr & 0xFF00);
        if page_crossed {
            // Canonical 6502 page-cross dummy read at the unfixed
            // address: (base_hi << 8) | ((base_lo + X) & 0xFF). The
            // high byte hasn't been incremented yet. This read has
            // side effects on PPU registers (`$2002` clears VBlank,
            // `$2007` advances the buffer) and is the hardware oracle
            // AccuracyCoin's `CPU Behavior :: Dummy read cycles`
            // Test 1 brackets via `LDA $20F2, X` with X=$10 reading
            // $2002 through the mirror.
            let dummy = (base & 0xFF00) | (addr & 0x00FF);
            let _ = self.read1(bus, dummy);
        }
        Operand { addr, page_crossed }
    }

    fn addr_abs_y<B: Bus>(&mut self, bus: &mut B) -> Operand {
        let base = self.fetch_pc_u16(bus);
        let addr = base.wrapping_add(u16::from(self.y));
        let page_crossed = (base & 0xFF00) != (addr & 0xFF00);
        if page_crossed {
            // See addr_abs_x for the page-cross dummy-read rationale.
            let dummy = (base & 0xFF00) | (addr & 0x00FF);
            let _ = self.read1(bus, dummy);
        }
        Operand { addr, page_crossed }
    }

    // ABS,X / ABS,Y operands for read-modify-write opcodes (ASL, LSR, ROL,
    // ROR, INC, DEC, and the unofficial SLO/RLA/SRE/RRA/DCP/ISC). Canonical
    // 6502: the unfixed-address dummy read happens UNCONDITIONALLY at
    // cycle 4 (not just on page cross) because the CPU has 7 cycles to
    // fill and cannot know the fixed address until the high-byte add
    // completes. Reads with side effects (`$2002` clears VBlank, `$4015`
    // clears frame-IRQ, `$2007` advances buffer) therefore fire twice on
    // RMW ABS,X. Bracketed by AccuracyCoin's `Implied Dummy Reads`
    // test 2: `SLO $4015,X` with X=0 expects the dummy read to clear the
    // frame-IRQ flag so the subsequent real read returns 0.
    fn addr_abs_x_rmw<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let base = self.fetch_pc_u16(bus);
        let addr = base.wrapping_add(u16::from(self.x));
        let dummy = (base & 0xFF00) | (addr & 0x00FF);
        let _ = self.read1(bus, dummy);
        addr
    }

    fn addr_abs_y_rmw<B: Bus>(&mut self, bus: &mut B) -> u16 {
        let base = self.fetch_pc_u16(bus);
        let addr = base.wrapping_add(u16::from(self.y));
        let dummy = (base & 0xFF00) | (addr & 0x00FF);
        let _ = self.read1(bus, dummy);
        addr
    }

    /// (zp),Y operand for the unofficial read-modify-write opcodes
    /// (SLO/RLA/SRE/RRA/DCP/ISB `(zp),Y` — `$13/$33/$53/$73/$D3/$F3`).
    ///
    /// v2.0.0 beta.2 (A2 every-cycle-bus-access): canonical 6502 8-cycle
    /// (zp),Y RMW performs the unfixed-address dummy read UNCONDITIONALLY at
    /// cycle 5 (like RMW ABS,X/Y above — the CPU cannot know the fixed
    /// address until the high-byte add completes), not only on page cross.
    /// The burn-probe histogram pinned these six arms as the last
    /// instruction-dispatch busless cycles (25 of the original 9,795).
    /// Flag-off delegates to the plain [`Self::addr_ind_y`] (dummy read on
    /// page cross only; the burn-loop fills the non-crossing cycle) so the
    /// shipped build stays byte-identical.
    fn addr_ind_y_rmw<B: Bus>(&mut self, bus: &mut B) -> u16 {
        #[cfg(feature = "mc-one-clock-v2")]
        {
            let ptr = self.fetch_pc(bus);
            let lo = self.read1(bus, u16::from(ptr));
            let hi = self.read1(bus, u16::from(ptr.wrapping_add(1)));
            let base = u16::from(lo) | (u16::from(hi) << 8);
            let addr = base.wrapping_add(u16::from(self.y));
            let dummy = (base & 0xFF00) | (addr & 0x00FF);
            let _ = self.read1(bus, dummy);
            addr
        }
        #[cfg(not(feature = "mc-one-clock-v2"))]
        {
            self.addr_ind_y(bus).addr
        }
    }

    fn addr_ind_x<B: Bus>(&mut self, bus: &mut B) -> Operand {
        let base = self.fetch_pc(bus);
        // A2: canonical (zp,X) cycle 3 — dummy read of the UN-indexed
        // pointer address while the X add completes (same silicon behavior
        // as `addr_zp_x`; zero-page, so register-side-effect-free).
        #[cfg(feature = "mc-one-clock-v2")]
        {
            let _ = self.read1(bus, u16::from(base));
        }
        let ptr = base.wrapping_add(self.x);
        let lo = self.read1(bus, u16::from(ptr));
        let hi = self.read1(bus, u16::from(ptr.wrapping_add(1)));
        Operand {
            addr: u16::from(lo) | (u16::from(hi) << 8),
            page_crossed: false,
        }
    }

    fn addr_ind_y<B: Bus>(&mut self, bus: &mut B) -> Operand {
        let ptr = self.fetch_pc(bus);
        let lo = self.read1(bus, u16::from(ptr));
        let hi = self.read1(bus, u16::from(ptr.wrapping_add(1)));
        let base = u16::from(lo) | (u16::from(hi) << 8);
        let addr = base.wrapping_add(u16::from(self.y));
        let page_crossed = (base & 0xFF00) != (addr & 0xFF00);
        if page_crossed {
            // Page-cross dummy read at the unfixed address — same as
            // addr_abs_x/y. Canonical 6502 behavior for LDA (zp),Y on
            // page crossing.
            let dummy = (base & 0xFF00) | (addr & 0x00FF);
            let _ = self.read1(bus, dummy);
        }
        Operand { addr, page_crossed }
    }

    // ------------------------------------------------------------------
    // Top-level dispatch.
    //
    // The 256-way match is the cleanest way to express the entire opcode
    // table; the doc-comments are intentionally absent at the arm level
    // because each one is a single line of the standard 6502 reference and
    // adding individual arm comments would overwhelm the readability of the
    // table.
    // ------------------------------------------------------------------

    #[allow(
        clippy::cognitive_complexity,
        clippy::too_many_lines,
        clippy::match_same_arms
    )]
    fn dispatch<B: Bus>(&mut self, bus: &mut B, op: u8, cycles: &mut u8) {
        match op {
            // === Loads ===
            0xA9 => {
                let v = self.fetch_pc(bus);
                self.lda(v);
                *cycles = 2;
            }
            0xA5 => {
                let o = self.addr_zp(bus);
                self.lda_addr(bus, o.addr);
                *cycles = 3;
            }
            0xB5 => {
                let o = self.addr_zp_x(bus);
                self.lda_addr(bus, o.addr);
                *cycles = 4;
            }
            0xAD => {
                let o = self.addr_abs(bus);
                self.lda_addr(bus, o.addr);
                *cycles = 4;
            }
            0xBD => {
                let o = self.addr_abs_x(bus);
                self.lda_addr(bus, o.addr);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0xB9 => {
                let o = self.addr_abs_y(bus);
                self.lda_addr(bus, o.addr);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0xA1 => {
                let o = self.addr_ind_x(bus);
                self.lda_addr(bus, o.addr);
                *cycles = 6;
            }
            0xB1 => {
                let o = self.addr_ind_y(bus);
                self.lda_addr(bus, o.addr);
                *cycles = 5 + u8::from(o.page_crossed);
            }

            0xA2 => {
                let v = self.fetch_pc(bus);
                self.ldx(v);
                *cycles = 2;
            }
            0xA6 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.ldx(v);
                *cycles = 3;
            }
            0xB6 => {
                let o = self.addr_zp_y(bus);
                let v = self.read1(bus, o.addr);
                self.ldx(v);
                *cycles = 4;
            }
            0xAE => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.ldx(v);
                *cycles = 4;
            }
            0xBE => {
                let o = self.addr_abs_y(bus);
                let v = self.read1(bus, o.addr);
                self.ldx(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }

            0xA0 => {
                let v = self.fetch_pc(bus);
                self.ldy(v);
                *cycles = 2;
            }
            0xA4 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.ldy(v);
                *cycles = 3;
            }
            0xB4 => {
                let o = self.addr_zp_x(bus);
                let v = self.read1(bus, o.addr);
                self.ldy(v);
                *cycles = 4;
            }
            0xAC => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.ldy(v);
                *cycles = 4;
            }
            0xBC => {
                let o = self.addr_abs_x(bus);
                let v = self.read1(bus, o.addr);
                self.ldy(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }

            // === Stores ===
            0x85 => {
                let o = self.addr_zp(bus);
                self.write1(bus, o.addr, self.a);
                *cycles = 3;
            }
            0x95 => {
                let o = self.addr_zp_x(bus);
                self.write1(bus, o.addr, self.a);
                *cycles = 4;
            }
            0x8D => {
                let o = self.addr_abs(bus);
                self.write1(bus, o.addr, self.a);
                *cycles = 4;
            }
            0x9D => {
                let o = self.addr_abs_x(bus);
                // Canonical 6502: STA absolute,X performs a dummy
                // read at cycle 4 even when no page is crossed (unlike
                // LDA where cycle 4 is the real read). `addr_abs_x`
                // already issues the dummy read at the unfixed address
                // when page-crossed; for the no-page-cross case we add
                // it here at the final address.
                if !o.page_crossed {
                    let _ = self.read1(bus, o.addr);
                }
                self.write1(bus, o.addr, self.a);
                *cycles = 5;
            }
            0x99 => {
                let o = self.addr_abs_y(bus);
                if !o.page_crossed {
                    let _ = self.read1(bus, o.addr);
                }
                self.write1(bus, o.addr, self.a);
                *cycles = 5;
            }
            0x81 => {
                let o = self.addr_ind_x(bus);
                self.write1(bus, o.addr, self.a);
                *cycles = 6;
            }
            0x91 => {
                let o = self.addr_ind_y(bus);
                // Canonical STA (zp),Y always dummy-reads at cycle 5
                // even when no page is crossed. `addr_ind_y` already
                // handles the page-cross dummy at the unfixed address;
                // add the no-page-cross dummy here at the final address.
                if !o.page_crossed {
                    let _ = self.read1(bus, o.addr);
                }
                self.write1(bus, o.addr, self.a);
                *cycles = 6;
            }

            0x86 => {
                let o = self.addr_zp(bus);
                self.write1(bus, o.addr, self.x);
                *cycles = 3;
            }
            0x96 => {
                let o = self.addr_zp_y(bus);
                self.write1(bus, o.addr, self.x);
                *cycles = 4;
            }
            0x8E => {
                let o = self.addr_abs(bus);
                self.write1(bus, o.addr, self.x);
                *cycles = 4;
            }

            0x84 => {
                let o = self.addr_zp(bus);
                self.write1(bus, o.addr, self.y);
                *cycles = 3;
            }
            0x94 => {
                let o = self.addr_zp_x(bus);
                self.write1(bus, o.addr, self.y);
                *cycles = 4;
            }
            0x8C => {
                let o = self.addr_abs(bus);
                self.write1(bus, o.addr, self.y);
                *cycles = 4;
            }

            // === Transfers ===
            0xAA => {
                self.implied_dummy_read(bus);
                self.x = self.a;
                self.p.set_nz(self.x);
                *cycles = 2;
            }
            0xA8 => {
                self.implied_dummy_read(bus);
                self.y = self.a;
                self.p.set_nz(self.y);
                *cycles = 2;
            }
            0xBA => {
                self.implied_dummy_read(bus);
                self.x = self.s;
                self.p.set_nz(self.x);
                *cycles = 2;
            }
            0x8A => {
                self.implied_dummy_read(bus);
                self.a = self.x;
                self.p.set_nz(self.a);
                *cycles = 2;
            }
            0x9A => {
                self.implied_dummy_read(bus);
                self.s = self.x;
                *cycles = 2;
            }
            0x98 => {
                self.implied_dummy_read(bus);
                self.a = self.y;
                self.p.set_nz(self.a);
                *cycles = 2;
            }

            // === Stack ===
            0x48 => {
                // PHA: C2 dummy read PC (the 6502 always reads the next byte on
                // the second cycle of a stack push), then the push.
                let _ = self.read1(bus, self.pc);
                self.push(bus, self.a);
                *cycles = 3;
            }
            0x08 => {
                let _ = self.read1(bus, self.pc);
                self.push(bus, (self.p | Status::BREAK | Status::UNUSED).bits());
                *cycles = 3;
            }
            0x68 => {
                // PLA: C2 dummy read PC, C3 dummy stack read (pre-increment),
                // then the pull.
                {
                    let _ = self.read1(bus, self.pc);
                    let _ = self.read1(bus, STACK_BASE | u16::from(self.s));
                }
                self.a = self.pull(bus);
                self.p.set_nz(self.a);
                *cycles = 4;
            }
            0x28 => {
                {
                    let _ = self.read1(bus, self.pc);
                    let _ = self.read1(bus, STACK_BASE | u16::from(self.s));
                }
                let v = self.pull(bus);
                let mut new_p = Status::from_bits_truncate(v);
                new_p.remove(Status::BREAK);
                new_p.insert(Status::UNUSED);
                self.p = new_p;
                *cycles = 4;
            }

            // === Logical ===
            0x29 => {
                let v = self.fetch_pc(bus);
                self.and(v);
                *cycles = 2;
            }
            0x25 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.and(v);
                *cycles = 3;
            }
            0x35 => {
                let o = self.addr_zp_x(bus);
                let v = self.read1(bus, o.addr);
                self.and(v);
                *cycles = 4;
            }
            0x2D => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.and(v);
                *cycles = 4;
            }
            0x3D => {
                let o = self.addr_abs_x(bus);
                let v = self.read1(bus, o.addr);
                self.and(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0x39 => {
                let o = self.addr_abs_y(bus);
                let v = self.read1(bus, o.addr);
                self.and(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0x21 => {
                let o = self.addr_ind_x(bus);
                let v = self.read1(bus, o.addr);
                self.and(v);
                *cycles = 6;
            }
            0x31 => {
                let o = self.addr_ind_y(bus);
                let v = self.read1(bus, o.addr);
                self.and(v);
                *cycles = 5 + u8::from(o.page_crossed);
            }

            0x09 => {
                let v = self.fetch_pc(bus);
                self.ora(v);
                *cycles = 2;
            }
            0x05 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.ora(v);
                *cycles = 3;
            }
            0x15 => {
                let o = self.addr_zp_x(bus);
                let v = self.read1(bus, o.addr);
                self.ora(v);
                *cycles = 4;
            }
            0x0D => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.ora(v);
                *cycles = 4;
            }
            0x1D => {
                let o = self.addr_abs_x(bus);
                let v = self.read1(bus, o.addr);
                self.ora(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0x19 => {
                let o = self.addr_abs_y(bus);
                let v = self.read1(bus, o.addr);
                self.ora(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0x01 => {
                let o = self.addr_ind_x(bus);
                let v = self.read1(bus, o.addr);
                self.ora(v);
                *cycles = 6;
            }
            0x11 => {
                let o = self.addr_ind_y(bus);
                let v = self.read1(bus, o.addr);
                self.ora(v);
                *cycles = 5 + u8::from(o.page_crossed);
            }

            0x49 => {
                let v = self.fetch_pc(bus);
                self.eor(v);
                *cycles = 2;
            }
            0x45 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.eor(v);
                *cycles = 3;
            }
            0x55 => {
                let o = self.addr_zp_x(bus);
                let v = self.read1(bus, o.addr);
                self.eor(v);
                *cycles = 4;
            }
            0x4D => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.eor(v);
                *cycles = 4;
            }
            0x5D => {
                let o = self.addr_abs_x(bus);
                let v = self.read1(bus, o.addr);
                self.eor(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0x59 => {
                let o = self.addr_abs_y(bus);
                let v = self.read1(bus, o.addr);
                self.eor(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0x41 => {
                let o = self.addr_ind_x(bus);
                let v = self.read1(bus, o.addr);
                self.eor(v);
                *cycles = 6;
            }
            0x51 => {
                let o = self.addr_ind_y(bus);
                let v = self.read1(bus, o.addr);
                self.eor(v);
                *cycles = 5 + u8::from(o.page_crossed);
            }

            0x24 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.bit(v);
                *cycles = 3;
            }
            0x2C => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.bit(v);
                *cycles = 4;
            }

            // === Arithmetic ===
            0x69 => {
                let v = self.fetch_pc(bus);
                self.adc(v);
                *cycles = 2;
            }
            0x65 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.adc(v);
                *cycles = 3;
            }
            0x75 => {
                let o = self.addr_zp_x(bus);
                let v = self.read1(bus, o.addr);
                self.adc(v);
                *cycles = 4;
            }
            0x6D => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.adc(v);
                *cycles = 4;
            }
            0x7D => {
                let o = self.addr_abs_x(bus);
                let v = self.read1(bus, o.addr);
                self.adc(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0x79 => {
                let o = self.addr_abs_y(bus);
                let v = self.read1(bus, o.addr);
                self.adc(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0x61 => {
                let o = self.addr_ind_x(bus);
                let v = self.read1(bus, o.addr);
                self.adc(v);
                *cycles = 6;
            }
            0x71 => {
                let o = self.addr_ind_y(bus);
                let v = self.read1(bus, o.addr);
                self.adc(v);
                *cycles = 5 + u8::from(o.page_crossed);
            }

            0xE9 | 0xEB => {
                let v = self.fetch_pc(bus);
                self.sbc(v);
                *cycles = 2;
            }
            0xE5 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.sbc(v);
                *cycles = 3;
            }
            0xF5 => {
                let o = self.addr_zp_x(bus);
                let v = self.read1(bus, o.addr);
                self.sbc(v);
                *cycles = 4;
            }
            0xED => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.sbc(v);
                *cycles = 4;
            }
            0xFD => {
                let o = self.addr_abs_x(bus);
                let v = self.read1(bus, o.addr);
                self.sbc(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0xF9 => {
                let o = self.addr_abs_y(bus);
                let v = self.read1(bus, o.addr);
                self.sbc(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0xE1 => {
                let o = self.addr_ind_x(bus);
                let v = self.read1(bus, o.addr);
                self.sbc(v);
                *cycles = 6;
            }
            0xF1 => {
                let o = self.addr_ind_y(bus);
                let v = self.read1(bus, o.addr);
                self.sbc(v);
                *cycles = 5 + u8::from(o.page_crossed);
            }

            // === Compare ===
            0xC9 => {
                let v = self.fetch_pc(bus);
                self.cmp_with(self.a, v);
                *cycles = 2;
            }
            0xC5 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.a, v);
                *cycles = 3;
            }
            0xD5 => {
                let o = self.addr_zp_x(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.a, v);
                *cycles = 4;
            }
            0xCD => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.a, v);
                *cycles = 4;
            }
            0xDD => {
                let o = self.addr_abs_x(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.a, v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0xD9 => {
                let o = self.addr_abs_y(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.a, v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0xC1 => {
                let o = self.addr_ind_x(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.a, v);
                *cycles = 6;
            }
            0xD1 => {
                let o = self.addr_ind_y(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.a, v);
                *cycles = 5 + u8::from(o.page_crossed);
            }

            0xE0 => {
                let v = self.fetch_pc(bus);
                self.cmp_with(self.x, v);
                *cycles = 2;
            }
            0xE4 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.x, v);
                *cycles = 3;
            }
            0xEC => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.x, v);
                *cycles = 4;
            }

            0xC0 => {
                let v = self.fetch_pc(bus);
                self.cmp_with(self.y, v);
                *cycles = 2;
            }
            0xC4 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.y, v);
                *cycles = 3;
            }
            0xCC => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.cmp_with(self.y, v);
                *cycles = 4;
            }

            // === Increments / decrements ===
            0xE6 => {
                let o = self.addr_zp(bus);
                self.inc_addr(bus, o.addr);
                *cycles = 5;
            }
            0xF6 => {
                let o = self.addr_zp_x(bus);
                self.inc_addr(bus, o.addr);
                *cycles = 6;
            }
            0xEE => {
                let o = self.addr_abs(bus);
                self.inc_addr(bus, o.addr);
                *cycles = 6;
            }
            0xFE => {
                let addr = self.addr_abs_x_rmw(bus);
                self.inc_addr(bus, addr);
                *cycles = 7;
            }
            0xC6 => {
                let o = self.addr_zp(bus);
                self.dec_addr(bus, o.addr);
                *cycles = 5;
            }
            0xD6 => {
                let o = self.addr_zp_x(bus);
                self.dec_addr(bus, o.addr);
                *cycles = 6;
            }
            0xCE => {
                let o = self.addr_abs(bus);
                self.dec_addr(bus, o.addr);
                *cycles = 6;
            }
            0xDE => {
                let addr = self.addr_abs_x_rmw(bus);
                self.dec_addr(bus, addr);
                *cycles = 7;
            }
            0xE8 => {
                self.implied_dummy_read(bus);
                self.x = self.x.wrapping_add(1);
                self.p.set_nz(self.x);
                *cycles = 2;
            }
            0xCA => {
                self.implied_dummy_read(bus);
                self.x = self.x.wrapping_sub(1);
                self.p.set_nz(self.x);
                *cycles = 2;
            }
            0xC8 => {
                self.implied_dummy_read(bus);
                self.y = self.y.wrapping_add(1);
                self.p.set_nz(self.y);
                *cycles = 2;
            }
            0x88 => {
                self.implied_dummy_read(bus);
                self.y = self.y.wrapping_sub(1);
                self.p.set_nz(self.y);
                *cycles = 2;
            }

            // === Shifts ===
            0x0A => {
                self.implied_dummy_read(bus);
                self.a = self.asl_value(self.a);
                *cycles = 2;
            }
            0x06 => {
                let o = self.addr_zp(bus);
                self.asl_addr(bus, o.addr);
                *cycles = 5;
            }
            0x16 => {
                let o = self.addr_zp_x(bus);
                self.asl_addr(bus, o.addr);
                *cycles = 6;
            }
            0x0E => {
                let o = self.addr_abs(bus);
                self.asl_addr(bus, o.addr);
                *cycles = 6;
            }
            0x1E => {
                let addr = self.addr_abs_x_rmw(bus);
                self.asl_addr(bus, addr);
                *cycles = 7;
            }

            0x4A => {
                self.implied_dummy_read(bus);
                self.a = self.lsr_value(self.a);
                *cycles = 2;
            }
            0x46 => {
                let o = self.addr_zp(bus);
                self.lsr_addr(bus, o.addr);
                *cycles = 5;
            }
            0x56 => {
                let o = self.addr_zp_x(bus);
                self.lsr_addr(bus, o.addr);
                *cycles = 6;
            }
            0x4E => {
                let o = self.addr_abs(bus);
                self.lsr_addr(bus, o.addr);
                *cycles = 6;
            }
            0x5E => {
                let addr = self.addr_abs_x_rmw(bus);
                self.lsr_addr(bus, addr);
                *cycles = 7;
            }

            0x2A => {
                self.implied_dummy_read(bus);
                self.a = self.rol_value(self.a);
                *cycles = 2;
            }
            0x26 => {
                let o = self.addr_zp(bus);
                self.rol_addr(bus, o.addr);
                *cycles = 5;
            }
            0x36 => {
                let o = self.addr_zp_x(bus);
                self.rol_addr(bus, o.addr);
                *cycles = 6;
            }
            0x2E => {
                let o = self.addr_abs(bus);
                self.rol_addr(bus, o.addr);
                *cycles = 6;
            }
            0x3E => {
                let addr = self.addr_abs_x_rmw(bus);
                self.rol_addr(bus, addr);
                *cycles = 7;
            }

            0x6A => {
                self.implied_dummy_read(bus);
                self.a = self.ror_value(self.a);
                *cycles = 2;
            }
            0x66 => {
                let o = self.addr_zp(bus);
                self.ror_addr(bus, o.addr);
                *cycles = 5;
            }
            0x76 => {
                let o = self.addr_zp_x(bus);
                self.ror_addr(bus, o.addr);
                *cycles = 6;
            }
            0x6E => {
                let o = self.addr_abs(bus);
                self.ror_addr(bus, o.addr);
                *cycles = 6;
            }
            0x7E => {
                let addr = self.addr_abs_x_rmw(bus);
                self.ror_addr(bus, addr);
                *cycles = 7;
            }

            // === Branches ===
            //
            // The `branch_delays_irq` quirk: real 6502 branches poll IRQ
            // at the same point a 2-cycle untaken branch would — at the
            // opcode-fetch cycle (the canonical 2-cycle "second-to-last"
            // poll).  The operand-fetch cycle and any extra taken /
            // page-cross cycles do NOT re-sample IRQ.  We suppress IRQ
            // sampling for the remaining cycles of the instruction
            // immediately *before* the operand fetch — the opcode-fetch
            // sample (in `step()`) has already happened by this point.
            // See `docs/cpu-6502.md` §Interrupt logic and
            // <https://www.nesdev.org/wiki/CPU_interrupts>.
            0x10 => {
                self.skip_irq_sample = true;
                let off = self.fetch_pc(bus);
                *cycles = self.branch(bus, off, !self.p.contains(Status::NEGATIVE));
            }
            0x30 => {
                self.skip_irq_sample = true;
                let off = self.fetch_pc(bus);
                *cycles = self.branch(bus, off, self.p.contains(Status::NEGATIVE));
            }
            0x50 => {
                self.skip_irq_sample = true;
                let off = self.fetch_pc(bus);
                *cycles = self.branch(bus, off, !self.p.contains(Status::OVERFLOW));
            }
            0x70 => {
                self.skip_irq_sample = true;
                let off = self.fetch_pc(bus);
                *cycles = self.branch(bus, off, self.p.contains(Status::OVERFLOW));
            }
            0x90 => {
                self.skip_irq_sample = true;
                let off = self.fetch_pc(bus);
                *cycles = self.branch(bus, off, !self.p.contains(Status::CARRY));
            }
            0xB0 => {
                self.skip_irq_sample = true;
                let off = self.fetch_pc(bus);
                *cycles = self.branch(bus, off, self.p.contains(Status::CARRY));
            }
            0xD0 => {
                self.skip_irq_sample = true;
                let off = self.fetch_pc(bus);
                *cycles = self.branch(bus, off, !self.p.contains(Status::ZERO));
            }
            0xF0 => {
                self.skip_irq_sample = true;
                let off = self.fetch_pc(bus);
                *cycles = self.branch(bus, off, self.p.contains(Status::ZERO));
            }

            // === Jumps / subroutine ===
            0x4C => {
                self.pc = self.fetch_pc_u16(bus);
                *cycles = 3;
            }
            0x6C => {
                let ptr = self.fetch_pc_u16(bus);
                self.pc = self.read_u16_with_wrap(bus, ptr);
                *cycles = 5;
            }
            0x20 => {
                // Canonical 6502 JSR cycle sequence — the high byte of
                // the target is read AFTER PC is pushed to the stack.
                // Wrong order is observable when JSR overwrites its own
                // operand via the pushed return address (AccuracyCoin
                // `CPU Behavior 2 :: JSR Edge Cases` Test 2 brackets
                // this exactly):
                //   C1: opcode fetch (already done by `tick` dispatcher)
                //   C2: fetch low byte of target → advances PC
                //   C3: dummy read from stack at $0100|S (no-op)
                //   C4: push PC high (PC is currently at the high-byte
                //       operand address, which is exactly the return
                //       address minus one)
                //   C5: push PC low
                //   C6: fetch high byte of target → PC = target
                let lo = self.fetch_pc(bus);
                let _ = self.read1(bus, STACK_BASE | u16::from(self.s));
                // self.pc now points at the high-byte operand; this is
                // the "return - 1" address JSR canonically pushes.
                let return_minus_one = self.pc;
                self.push(bus, (return_minus_one >> 8) as u8);
                self.push(bus, (return_minus_one & 0xFF) as u8);
                let hi = self.fetch_pc(bus);
                self.pc = u16::from(lo) | (u16::from(hi) << 8);
                *cycles = 6;
            }
            0x60 => {
                // Canonical 6502 RTS bus pattern (every cycle is a bus access):
                //   C1 opcode fetch (dispatcher) | C2 dummy read PC |
                //   C3 dummy stack read (pre-increment) |
                //   C4 pull PCL | C5 pull PCH | C6 dummy read at the return addr.
                // Default build burns C2/C3/C6 as `idle_tick` (no bus access);
                // `cpu-stack-dummy-reads` emits the canonical dummy reads — the
                // DC-6 Y=3-vs-4 fix. See the cell-trace cross-diff.
                {
                    let _ = self.read1(bus, self.pc);
                    let _ = self.read1(bus, STACK_BASE | u16::from(self.s));
                    let v = self.pull_u16(bus);
                    let _ = self.read1(bus, v);
                    self.pc = v.wrapping_add(1);
                }
                *cycles = 6;
            }
            0x40 => {
                // Canonical RTI bus pattern: C2 dummy read PC, C3 dummy stack
                // read (pre-increment) before the pulls. Default-off helper.
                {
                    let _ = self.read1(bus, self.pc);
                    let _ = self.read1(bus, STACK_BASE | u16::from(self.s));
                }
                let p = self.pull(bus);
                let mut new_p = Status::from_bits_truncate(p);
                new_p.remove(Status::BREAK);
                new_p.insert(Status::UNUSED);
                self.p = new_p;
                // RTI's I-flag change is observed by the IRQ sample
                // (unlike PLP / CLI / SEI which delay one instruction).
                self.irq_sample_i_flag = self.p.contains(Status::INTERRUPT_DISABLE);
                self.pc = self.pull_u16(bus);
                *cycles = 6;
            }
            0x00 => {
                // BRK is a 7-cycle interrupt with PC+2 pushed (PC already
                // advanced by fetch; advance one more for the padding byte).
                self.pc = self.pc.wrapping_add(1);
                self.service_interrupt(bus, IRQ_VECTOR, true);
                // R1/A2: suppress an NMI that became pending during/just-after
                // the BRK sequence so the FIRST instruction of the IRQ handler
                // runs before the NMI is taken (Mesen2 `NesCpu::BRK`
                // `_prevNeedNmi = false`; "needed for nmi_and_brk"). The NMI is
                // not lost — `mc_need_nmi` stays set and re-arms next cycle.
                {
                    self.mc_prev_need_nmi = false;
                }
                // service_interrupt already burned 7 cycles; do NOT double-count.
                *cycles = 0;
            }
            0xEA => {
                self.implied_dummy_read(bus);
                *cycles = 2;
            }

            // === Flag manipulation ===
            0x18 => {
                self.implied_dummy_read(bus);
                self.p.remove(Status::CARRY);
                *cycles = 2;
            }
            0x38 => {
                self.implied_dummy_read(bus);
                self.p.insert(Status::CARRY);
                *cycles = 2;
            }
            0x58 => {
                self.implied_dummy_read(bus);
                self.p.remove(Status::INTERRUPT_DISABLE);
                *cycles = 2;
            }
            0x78 => {
                self.implied_dummy_read(bus);
                self.p.insert(Status::INTERRUPT_DISABLE);
                *cycles = 2;
            }
            0xB8 => {
                self.implied_dummy_read(bus);
                self.p.remove(Status::OVERFLOW);
                *cycles = 2;
            }
            0xD8 => {
                self.implied_dummy_read(bus);
                self.p.remove(Status::DECIMAL);
                *cycles = 2;
            }
            0xF8 => {
                self.implied_dummy_read(bus);
                self.p.insert(Status::DECIMAL);
                *cycles = 2;
            }

            // === Unofficial NOP variants ===
            // Implied / 1-byte NOPs
            0x1A | 0x3A | 0x5A | 0x7A | 0xDA | 0xFA => {
                self.implied_dummy_read(bus);
                *cycles = 2;
            }
            // Immediate / zero-page DOP (double NOP) variants: skip 1 byte.
            0x80 | 0x82 | 0x89 | 0xC2 | 0xE2 => {
                let _ = self.fetch_pc(bus);
                *cycles = 2;
            }
            0x04 | 0x44 | 0x64 => {
                let o = self.addr_zp(bus);
                let _ = self.read1(bus, o.addr); // unofficial DOP dummy read
                *cycles = 3;
            }
            0x14 | 0x34 | 0x54 | 0x74 | 0xD4 | 0xF4 => {
                let o = self.addr_zp_x(bus);
                let _ = self.read1(bus, o.addr); // unofficial DOP dummy read
                *cycles = 4;
            }
            // Absolute "TOP" (triple NOP) — must dummy-read the target so
            // that PPU-mirror side-effects (e.g. clearing $2002.7) fire,
            // matching real silicon and AccuracyCoin's All-NOPs Test 2.
            0x0C => {
                let o = self.addr_abs(bus);
                let _ = self.read1(bus, o.addr);
                *cycles = 4;
            }
            0x1C | 0x3C | 0x5C | 0x7C | 0xDC | 0xFC => {
                let o = self.addr_abs_x(bus);
                let _ = self.read1(bus, o.addr); // dummy read on TOP
                *cycles = 4 + u8::from(o.page_crossed);
            }

            // === Stable unofficial: LAX, SAX ===
            0xA7 => {
                let o = self.addr_zp(bus);
                let v = self.read1(bus, o.addr);
                self.lax(v);
                *cycles = 3;
            }
            0xB7 => {
                let o = self.addr_zp_y(bus);
                let v = self.read1(bus, o.addr);
                self.lax(v);
                *cycles = 4;
            }
            0xAF => {
                let o = self.addr_abs(bus);
                let v = self.read1(bus, o.addr);
                self.lax(v);
                *cycles = 4;
            }
            0xBF => {
                let o = self.addr_abs_y(bus);
                let v = self.read1(bus, o.addr);
                self.lax(v);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0xA3 => {
                let o = self.addr_ind_x(bus);
                let v = self.read1(bus, o.addr);
                self.lax(v);
                *cycles = 6;
            }
            0xB3 => {
                let o = self.addr_ind_y(bus);
                let v = self.read1(bus, o.addr);
                self.lax(v);
                *cycles = 5 + u8::from(o.page_crossed);
            }
            0xAB => {
                let v = self.fetch_pc(bus);
                self.lax(v);
                *cycles = 2;
            } // LAX immediate (often listed as ATX). We follow nestest behavior.

            0x87 => {
                let o = self.addr_zp(bus);
                self.write1(bus, o.addr, self.a & self.x);
                *cycles = 3;
            }
            0x97 => {
                let o = self.addr_zp_y(bus);
                self.write1(bus, o.addr, self.a & self.x);
                *cycles = 4;
            }
            0x8F => {
                let o = self.addr_abs(bus);
                self.write1(bus, o.addr, self.a & self.x);
                *cycles = 4;
            }
            0x83 => {
                let o = self.addr_ind_x(bus);
                self.write1(bus, o.addr, self.a & self.x);
                *cycles = 6;
            }

            // === DCP (DEC + CMP) ===
            0xC7 => {
                let o = self.addr_zp(bus);
                self.dcp_addr(bus, o.addr);
                *cycles = 5;
            }
            0xD7 => {
                let o = self.addr_zp_x(bus);
                self.dcp_addr(bus, o.addr);
                *cycles = 6;
            }
            0xCF => {
                let o = self.addr_abs(bus);
                self.dcp_addr(bus, o.addr);
                *cycles = 6;
            }
            0xDF => {
                let addr = self.addr_abs_x_rmw(bus);
                self.dcp_addr(bus, addr);
                *cycles = 7;
            }
            0xDB => {
                let addr = self.addr_abs_y_rmw(bus);
                self.dcp_addr(bus, addr);
                *cycles = 7;
            }
            0xC3 => {
                let o = self.addr_ind_x(bus);
                self.dcp_addr(bus, o.addr);
                *cycles = 8;
            }
            0xD3 => {
                let addr = self.addr_ind_y_rmw(bus);
                self.dcp_addr(bus, addr);
                *cycles = 8;
            }

            // === ISC (INC + SBC) ===
            0xE7 => {
                let o = self.addr_zp(bus);
                self.isc_addr(bus, o.addr);
                *cycles = 5;
            }
            0xF7 => {
                let o = self.addr_zp_x(bus);
                self.isc_addr(bus, o.addr);
                *cycles = 6;
            }
            0xEF => {
                let o = self.addr_abs(bus);
                self.isc_addr(bus, o.addr);
                *cycles = 6;
            }
            0xFF => {
                let addr = self.addr_abs_x_rmw(bus);
                self.isc_addr(bus, addr);
                *cycles = 7;
            }
            0xFB => {
                let addr = self.addr_abs_y_rmw(bus);
                self.isc_addr(bus, addr);
                *cycles = 7;
            }
            0xE3 => {
                let o = self.addr_ind_x(bus);
                self.isc_addr(bus, o.addr);
                *cycles = 8;
            }
            0xF3 => {
                let addr = self.addr_ind_y_rmw(bus);
                self.isc_addr(bus, addr);
                *cycles = 8;
            }

            // === SLO (ASL + ORA) ===
            0x07 => {
                let o = self.addr_zp(bus);
                self.slo_addr(bus, o.addr);
                *cycles = 5;
            }
            0x17 => {
                let o = self.addr_zp_x(bus);
                self.slo_addr(bus, o.addr);
                *cycles = 6;
            }
            0x0F => {
                let o = self.addr_abs(bus);
                self.slo_addr(bus, o.addr);
                *cycles = 6;
            }
            0x1F => {
                let addr = self.addr_abs_x_rmw(bus);
                self.slo_addr(bus, addr);
                *cycles = 7;
            }
            0x1B => {
                let addr = self.addr_abs_y_rmw(bus);
                self.slo_addr(bus, addr);
                *cycles = 7;
            }
            0x03 => {
                let o = self.addr_ind_x(bus);
                self.slo_addr(bus, o.addr);
                *cycles = 8;
            }
            0x13 => {
                let addr = self.addr_ind_y_rmw(bus);
                self.slo_addr(bus, addr);
                *cycles = 8;
            }

            // === RLA (ROL + AND) ===
            0x27 => {
                let o = self.addr_zp(bus);
                self.rla_addr(bus, o.addr);
                *cycles = 5;
            }
            0x37 => {
                let o = self.addr_zp_x(bus);
                self.rla_addr(bus, o.addr);
                *cycles = 6;
            }
            0x2F => {
                let o = self.addr_abs(bus);
                self.rla_addr(bus, o.addr);
                *cycles = 6;
            }
            0x3F => {
                let addr = self.addr_abs_x_rmw(bus);
                self.rla_addr(bus, addr);
                *cycles = 7;
            }
            0x3B => {
                let addr = self.addr_abs_y_rmw(bus);
                self.rla_addr(bus, addr);
                *cycles = 7;
            }
            0x23 => {
                let o = self.addr_ind_x(bus);
                self.rla_addr(bus, o.addr);
                *cycles = 8;
            }
            0x33 => {
                let addr = self.addr_ind_y_rmw(bus);
                self.rla_addr(bus, addr);
                *cycles = 8;
            }

            // === SRE (LSR + EOR) ===
            0x47 => {
                let o = self.addr_zp(bus);
                self.sre_addr(bus, o.addr);
                *cycles = 5;
            }
            0x57 => {
                let o = self.addr_zp_x(bus);
                self.sre_addr(bus, o.addr);
                *cycles = 6;
            }
            0x4F => {
                let o = self.addr_abs(bus);
                self.sre_addr(bus, o.addr);
                *cycles = 6;
            }
            0x5F => {
                let addr = self.addr_abs_x_rmw(bus);
                self.sre_addr(bus, addr);
                *cycles = 7;
            }
            0x5B => {
                let addr = self.addr_abs_y_rmw(bus);
                self.sre_addr(bus, addr);
                *cycles = 7;
            }
            0x43 => {
                let o = self.addr_ind_x(bus);
                self.sre_addr(bus, o.addr);
                *cycles = 8;
            }
            0x53 => {
                let addr = self.addr_ind_y_rmw(bus);
                self.sre_addr(bus, addr);
                *cycles = 8;
            }

            // === RRA (ROR + ADC) ===
            0x67 => {
                let o = self.addr_zp(bus);
                self.rra_addr(bus, o.addr);
                *cycles = 5;
            }
            0x77 => {
                let o = self.addr_zp_x(bus);
                self.rra_addr(bus, o.addr);
                *cycles = 6;
            }
            0x6F => {
                let o = self.addr_abs(bus);
                self.rra_addr(bus, o.addr);
                *cycles = 6;
            }
            0x7F => {
                let addr = self.addr_abs_x_rmw(bus);
                self.rra_addr(bus, addr);
                *cycles = 7;
            }
            0x7B => {
                let addr = self.addr_abs_y_rmw(bus);
                self.rra_addr(bus, addr);
                *cycles = 7;
            }
            0x63 => {
                let o = self.addr_ind_x(bus);
                self.rra_addr(bus, o.addr);
                *cycles = 8;
            }
            0x73 => {
                let addr = self.addr_ind_y_rmw(bus);
                self.rra_addr(bus, addr);
                *cycles = 8;
            }

            // === ANC, ALR, ARR, AXS ===
            0x0B | 0x2B => {
                let v = self.fetch_pc(bus);
                self.a &= v;
                self.p.set_nz(self.a);
                self.p.set(Status::CARRY, self.a & 0x80 != 0);
                *cycles = 2;
            }
            0x4B => {
                let v = self.fetch_pc(bus);
                self.a &= v;
                let new_carry = self.a & 0x01 != 0;
                self.a >>= 1;
                self.p.set_nz(self.a);
                self.p.set(Status::CARRY, new_carry);
                *cycles = 2;
            }
            0x6B => {
                let v = self.fetch_pc(bus);
                self.a &= v;
                let carry_in = self.p.contains(Status::CARRY);
                self.a = (self.a >> 1) | (u8::from(carry_in) << 7);
                self.p.set_nz(self.a);
                let bit6 = self.a & 0x40 != 0;
                let bit5 = self.a & 0x20 != 0;
                self.p.set(Status::CARRY, bit6);
                self.p.set(Status::OVERFLOW, bit6 ^ bit5);
                *cycles = 2;
            }
            0xCB => {
                let v = self.fetch_pc(bus);
                let ax = self.a & self.x;
                let (res, overflow) = ax.overflowing_sub(v);
                self.x = res;
                self.p.set(Status::CARRY, !overflow);
                self.p.set_nz(res);
                *cycles = 2;
            }

            // === Unstable: XAA, LAS, TAS, SHA, SHX, SHY ===
            0x8B => {
                // XAA / ANE: A = (A | const) & X & operand. nestest expects this.
                let v = self.fetch_pc(bus);
                self.a = (self.a | 0xFF) & self.x & v;
                self.p.set_nz(self.a);
                *cycles = 2;
            }
            0xBB => {
                let o = self.addr_abs_y(bus);
                let v = self.read1(bus, o.addr);
                let res = self.s & v;
                self.a = res;
                self.x = res;
                self.s = res;
                self.p.set_nz(res);
                *cycles = 4 + u8::from(o.page_crossed);
            }
            0x9B => {
                // TAS / SHS / XAS abs,Y: S = A & X; then SHA-style write
                // using `S` as the value register.
                let base = self.fetch_pc_u16(bus);
                self.s = self.a & self.x;
                self.sh_store(bus, base, self.y, self.s);
                *cycles = 5;
            }
            0x9F => {
                // SHA abs,Y. value_reg = A & X.
                let base = self.fetch_pc_u16(bus);
                self.sh_store(bus, base, self.y, self.a & self.x);
                *cycles = 5;
            }
            0x93 => {
                // SHA (zp),Y. Indirect; base from zp-pointer-resolved
                // low/high bytes.  value_reg = A & X.
                let zp = self.fetch_pc(bus);
                let lo = self.read1(bus, u16::from(zp));
                let hi_byte = self.read1(bus, u16::from(zp.wrapping_add(1)));
                let base = u16::from(lo) | (u16::from(hi_byte) << 8);
                self.sh_store(bus, base, self.y, self.a & self.x);
                *cycles = 6;
            }
            0x9E => {
                // SHX abs,Y. value_reg = X.
                let base = self.fetch_pc_u16(bus);
                self.sh_store(bus, base, self.y, self.x);
                *cycles = 5;
            }
            0x9C => {
                // SHY abs,X. value_reg = Y. Index register is X here.
                let base = self.fetch_pc_u16(bus);
                self.sh_store(bus, base, self.x, self.y);
                *cycles = 5;
            }

            // === JAM / KIL / STP ===
            0x02 | 0x12 | 0x22 | 0x32 | 0x42 | 0x52 | 0x62 | 0x72 | 0x92 | 0xB2 | 0xD2 | 0xF2 => {
                self.jammed = true;
                *cycles = 2;
            }
        }
    }

    // ------------------------------------------------------------------
    // Helpers / micro-ops.
    // ------------------------------------------------------------------

    fn lda(&mut self, value: u8) {
        self.a = value;
        self.p.set_nz(value);
    }

    fn lda_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let v = self.read1(bus, addr);
        self.lda(v);
    }

    fn ldx(&mut self, value: u8) {
        self.x = value;
        self.p.set_nz(value);
    }

    fn ldy(&mut self, value: u8) {
        self.y = value;
        self.p.set_nz(value);
    }

    fn and(&mut self, value: u8) {
        self.a &= value;
        self.p.set_nz(self.a);
    }

    fn ora(&mut self, value: u8) {
        self.a |= value;
        self.p.set_nz(self.a);
    }

    fn eor(&mut self, value: u8) {
        self.a ^= value;
        self.p.set_nz(self.a);
    }

    fn bit(&mut self, value: u8) {
        let result = self.a & value;
        self.p.set(Status::ZERO, result == 0);
        self.p.set(Status::NEGATIVE, value & 0x80 != 0);
        self.p.set(Status::OVERFLOW, value & 0x40 != 0);
    }

    fn adc(&mut self, value: u8) {
        let carry = u16::from(self.p.contains(Status::CARRY));
        let sum = u16::from(self.a) + u16::from(value) + carry;
        let result = sum as u8;
        self.p.set(Status::CARRY, sum > 0xFF);
        let overflow = ((self.a ^ result) & (value ^ result) & 0x80) != 0;
        self.p.set(Status::OVERFLOW, overflow);
        self.a = result;
        self.p.set_nz(self.a);
    }

    fn sbc(&mut self, value: u8) {
        // SBC = ADC of inverted value.
        self.adc(value ^ 0xFF);
    }

    fn cmp_with(&mut self, lhs: u8, rhs: u8) {
        let (r, borrow) = lhs.overflowing_sub(rhs);
        self.p.set(Status::CARRY, !borrow);
        self.p.set_nz(r);
    }

    fn inc_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let original = self.read1(bus, addr);
        // RMW dummy write: real 6502 writes the original byte back to the
        // same address before writing the modified value (visible at memory-
        // mapped registers like $4014 and $2007). See `docs/cpu-6502.md` and
        // nesdev wiki "Dummy writes".
        self.write1(bus, addr, original);
        let v = original.wrapping_add(1);
        self.write1(bus, addr, v);
        self.p.set_nz(v);
    }

    fn dec_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let original = self.read1(bus, addr);
        self.write1(bus, addr, original);
        let v = original.wrapping_sub(1);
        self.write1(bus, addr, v);
        self.p.set_nz(v);
    }

    fn asl_value(&mut self, value: u8) -> u8 {
        self.p.set(Status::CARRY, value & 0x80 != 0);
        let r = value << 1;
        self.p.set_nz(r);
        r
    }

    fn asl_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let v = self.read1(bus, addr);
        // RMW dummy write — see `inc_addr`.
        self.write1(bus, addr, v);
        let r = self.asl_value(v);
        self.write1(bus, addr, r);
    }

    fn lsr_value(&mut self, value: u8) -> u8 {
        self.p.set(Status::CARRY, value & 0x01 != 0);
        let r = value >> 1;
        self.p.set_nz(r);
        r
    }

    fn lsr_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let v = self.read1(bus, addr);
        self.write1(bus, addr, v);
        let r = self.lsr_value(v);
        self.write1(bus, addr, r);
    }

    fn rol_value(&mut self, value: u8) -> u8 {
        let carry_in = u8::from(self.p.contains(Status::CARRY));
        self.p.set(Status::CARRY, value & 0x80 != 0);
        let r = (value << 1) | carry_in;
        self.p.set_nz(r);
        r
    }

    fn rol_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let v = self.read1(bus, addr);
        self.write1(bus, addr, v);
        let r = self.rol_value(v);
        self.write1(bus, addr, r);
    }

    fn ror_value(&mut self, value: u8) -> u8 {
        let carry_in = u8::from(self.p.contains(Status::CARRY)) << 7;
        self.p.set(Status::CARRY, value & 0x01 != 0);
        let r = (value >> 1) | carry_in;
        self.p.set_nz(r);
        r
    }

    fn ror_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let v = self.read1(bus, addr);
        self.write1(bus, addr, v);
        let r = self.ror_value(v);
        self.write1(bus, addr, r);
    }

    fn branch<B: Bus>(&mut self, bus: &mut B, offset: u8, condition: bool) -> u8 {
        if !condition {
            return 2;
        }
        // T-60-001 (2026-05-17 — 9th C1 attempt, branch axis): per
        // nesdev wiki §"CPU interrupts" §"Branch instructions", TAKEN
        // branches DELAY IRQ detection. Our `step()` samples IRQ at
        // the opcode-fetch (cycle 1 = tick 0) via `idle_tick`, then
        // each branch opcode sets `skip_irq_sample = true` before the
        // operand fetch to suppress sampling on the extra cycles. But
        // the cycle-1 sample is still recorded in `irq_first_tick`
        // and `promote_post_step_interrupts` will ARM the IRQ at the
        // end of this instruction (cycle-1 sample < last_tick on a
        // 3- or 4-cycle taken branch). That contradicts the
        // "branches delay IRQ" rule — IRQ should be deferred to the
        // NEXT instruction's poll. Drop the cycle-1 sample here on
        // taken branches; the next instruction's opcode fetch will
        // re-sample the (still-asserted, level-triggered) IRQ line
        // and arm it normally. NMI is edge-triggered and sampled on
        // every cycle the CPU is alive (per nesdev) — its first-tick
        // latch is intentionally NOT dropped.
        self.irq_first_tick = u8::MAX;
        // Canonical 6502 branch cycle sequence per nesdev wiki and
        // AccuracyCoin `CPU Behavior 2 :: Branch Dummy Reads` Test 4:
        //   C1: opcode fetch (done by `tick` dispatcher)
        //   C2: operand fetch (done by per-opcode `fetch_pc` before call)
        //   C3: dummy read of PC (the byte after the operand) — this is
        //       cycle 3 of the taken branch, and is observable as a
        //       second consecutive read of `$2002` mirror through which
        //       AccuracyCoin brackets the dummy.
        //   C4: (only if page-crossed) dummy read of (old_pch | new_pcl)
        //       — the unfixed-high-byte address before the high-byte
        //       carry propagates.
        let _ = self.read1(bus, self.pc); // C3 dummy
        let signed = offset as i8 as i16;
        let old_pc = self.pc;
        let new_pc = (self.pc as i32 + i32::from(signed)) as u16;
        let crossed = (old_pc & 0xFF00) != (new_pc & 0xFF00);
        if crossed {
            // W1 (`mc-r1-branch-poll-points`): a page-cross taken branch
            // polls a SECOND time at C4-start — TriCNES's
            // `PollInterrupts_CantDisableIRQ` in the BPL microcode
            // (`golden/tricnes/tricnes-full-src/Emulator.cs`): if the C2-start
            // poll already saw the IRQ this one cannot un-see it (can-SET-
            // not-clear). `mc_run_irq` is frozen across the branch's
            // remaining cycles by the `handle_interrupts` early-return, so
            // sample the live line here (state as of end-of-C3) and OR it in;
            // the end-of-C4 `mc_prev_run_irq` copy then exposes it to the
            // next `step()` dispatch.
            if !self.mc_run_irq {
                self.mc_run_irq = bus.irq_level() && !self.irq_sample_i_flag;
            }
            // C4 page-cross dummy read at the unfixed address.
            let dummy = (old_pc & 0xFF00) | (new_pc & 0x00FF);
            let _ = self.read1(bus, dummy);
        }
        self.pc = new_pc;
        if crossed { 4 } else { 3 }
    }

    fn lax(&mut self, value: u8) {
        self.a = value;
        self.x = value;
        self.p.set_nz(value);
    }

    fn dcp_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let original = self.read1(bus, addr);
        // RMW dummy write.
        self.write1(bus, addr, original);
        let v = original.wrapping_sub(1);
        self.write1(bus, addr, v);
        self.cmp_with(self.a, v);
    }

    fn isc_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let original = self.read1(bus, addr);
        self.write1(bus, addr, original);
        let v = original.wrapping_add(1);
        self.write1(bus, addr, v);
        self.sbc(v);
    }

    fn slo_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let v = self.read1(bus, addr);
        self.write1(bus, addr, v);
        let r = self.asl_value(v);
        self.write1(bus, addr, r);
        self.a |= r;
        self.p.set_nz(self.a);
    }

    fn rla_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let v = self.read1(bus, addr);
        self.write1(bus, addr, v);
        let r = self.rol_value(v);
        self.write1(bus, addr, r);
        self.a &= r;
        self.p.set_nz(self.a);
    }

    fn sre_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let v = self.read1(bus, addr);
        self.write1(bus, addr, v);
        let r = self.lsr_value(v);
        self.write1(bus, addr, r);
        self.a ^= r;
        self.p.set_nz(self.a);
    }

    fn rra_addr<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        let v = self.read1(bus, addr);
        self.write1(bus, addr, v);
        let r = self.ror_value(v);
        self.write1(bus, addr, r);
        self.adc(r);
    }
}
