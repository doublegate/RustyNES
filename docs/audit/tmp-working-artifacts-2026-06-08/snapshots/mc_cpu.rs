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
    clippy::cast_sign_loss,
    // Doc comments reference reference-emulator file names (Mesen2,
    // TetaNES, NesCpu.cpp) extensively. Backticking every one would
    // noise up the prose; the module-level allow keeps the references
    // readable. R6 can revisit if desired.
    clippy::doc_markdown
)]

use crate::bus::Bus;
use crate::status::Status;

/// Master clocks per CPU cycle (NTSC). Mirrors Mesen `_cpuDivider` /
/// TetaNES `Cpu::CPU_DIVIDER`. PAL is 16 (handled when the bus drives
/// region selection at a later phase; the CPU itself uses NTSC here).
const CPU_DIVIDER_NTSC: u64 = 12;

/// PPU sub-cycle offset for the catch-up target: the PPU is run to
/// `master_clock - PPU_OFFSET` in BOTH halves of every access (the
/// double catch-up). Mesen `_ppuOffset = 1` (NesCpu.cpp:242), TetaNES
/// `PPU_OFFSET` (cpu.rs:106). This is the ENTIRE CPU↔PPU phase relation
/// in the clean v2.0 model — replaces the v1.x combo's `OFFSET_MC=1 +
/// PHASE_COMP_MC=8 = 9 mc` lag.
const PPU_OFFSET: u64 = 1;

/// READ access master-clock split: `master_clock += pre` in `start_cycle`,
/// `+= post` in `end_cycle`. Mesen `_startClockCount = 6` minus the
/// `_ppuOffset = 1` for the read = 5; the symmetric `_endClockCount + 1`
/// = 7. Total = `CPU_DIVIDER_NTSC` either way.
const READ_PRE_MC: u64 = CPU_DIVIDER_NTSC / 2 - PPU_OFFSET;
const READ_POST_MC: u64 = CPU_DIVIDER_NTSC - READ_PRE_MC;

/// WRITE access master-clock split — swapped (writes commit 2 mc later in
/// the cycle than reads, Mesen NesCpu.cpp:430).
const WRITE_PRE_MC: u64 = CPU_DIVIDER_NTSC / 2 + PPU_OFFSET;
const WRITE_POST_MC: u64 = CPU_DIVIDER_NTSC - WRITE_PRE_MC;

/// Stack base address: the CPU stack lives at `$0100 + S`.
const STACK_BASE: u16 = 0x0100;

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

    // === v2.0 master-clock interrupt model (cpu-c1-attempt-17 only) ===
    // A faithful port of TetaNES's `handle_interrupts` (`cpu.rs:230-282`):
    // the /NMI and IRQ lines are sampled at φ2 of every CPU cycle with a
    // one-cycle `prev_*` delay, and dispatch (between instructions) gates on
    // the DELAYED copies. This replaces the `nmi_first_tick`/`promote` +
    // `armed_*`/`pending_*` heuristic under the φ1/φ2 access reorder, where
    // that heuristic mis-times NMI by ~2 PPU clocks (blargg
    // `ppu_vbl_nmi/05-nmi_timing`). The PPU then raises /NMI coincident with
    // the VBL flag (dot 1) — the whole observed latency is this CPU-side
    // delay. See `docs/audit/v2.0-coordinated-cpu-ppu-recalibration-2026-05-25.md`.
    /// Cumulative master clock (v2.0 clean-sheet, Mesen2 `NesCpu::_masterClock` /
    /// TetaNES `Cpu::master_clock`). Advanced by `start_cycle`/`end_cycle` in
    /// every access (`+5/+7` mc for reads, `+7/+5` for writes); the CPU calls
    /// `bus.run_ppu_to(master_clock - PPU_OFFSET)` from BOTH halves (double
    /// catch-up) so the PPU is on-time relative to every access.
    pub(crate) master_clock: u64,
    /// Previous cycle's raw /NMI line level (for edge detection).
    pub(crate) mc_prev_nmi_line: bool,
    /// Latched NMI edge (`TetaNES` `IrqFlags::NMI`); set on a rising /NMI edge,
    /// cleared when the NMI is serviced.
    pub(crate) mc_need_nmi: bool,
    /// One-cycle-delayed copy of [`Self::mc_need_nmi`] (`TetaNES`
    /// `IrqFlags::PREV_NMI`) — the actual dispatch gate.
    pub(crate) mc_prev_need_nmi: bool,
    /// Current I-masked IRQ level sampled this cycle (`TetaNES` `RUN_IRQ`).
    pub(crate) mc_run_irq: bool,
    /// One-cycle-delayed copy of [`Self::mc_run_irq`] (`TetaNES`
    /// `PREV_RUN_IRQ`) — the dispatch gate.
    pub(crate) mc_prev_run_irq: bool,

    // === R4 — CPU-driven DMC/OAM DMA orchestration (clean-room port of
    // Mesen2 NesCpu's _needHalt / _needDummyRead / _dmcDmaRunning /
    // _abortDmcDma / _spriteDmaTransfer / _spriteDmaOffset state, gated on
    // the `r4-cpu-dma` cargo feature). When the feature is OFF these fields
    // are unused; when ON, `process_pending_dma` consumes them at the top
    // of every read to drive the halt/dummy/align/get state machine.
    //
    // The state transitions:
    // - APU arms DMC: bus.dmc_dma_pending() → CPU latches dmc_dma_running.
    //   At the next read, need_halt = true triggers the halt cycle.
    // - $4014 write: bus.take_oam_dma_page() returns Some(page) → CPU
    //   latches sprite_dma_transfer + sprite_dma_offset. need_halt triggers.
    // - Halt cycle: one dummy read at the held address. need_halt cleared.
    // - DMC get cycle: read at bus.dmc_dma_addr(), deliver via
    //   bus.dmc_dma_complete(byte). dmc_dma_running cleared.
    // - Sprite DMA: 256 alternating read/write cycles at 0x100 * offset.
    //   On the put cycles, write the previously-read byte to $2004.
    /// R4: `true` for one CPU cycle after DMC or OAM DMA was armed —
    /// triggers a halt (dummy) read at the held address.
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_need_halt: bool,
    /// R4: `true` if a dummy read is required before the next get cycle
    /// (e.g. when CPU is in the wrong phase for the DMC fetch).
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_need_dummy_read: bool,
    /// R4: `true` while the DMC channel has a pending sample fetch the
    /// CPU has latched (cleared after the get cycle delivers the byte).
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_dmc_dma_running: bool,
    /// R4: `true` while an OAM DMA transfer is in progress.
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_sprite_dma_transfer: bool,
    /// R4: source page (`$xx00`) for the in-progress OAM DMA.
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_sprite_dma_offset: u8,
    /// R4: byte index 0..=0x1FF of the in-progress OAM DMA. Even ticks
    /// are read cycles; odd ticks are write cycles (Mesen
    /// `spriteDmaCounter` in NesCpu.cpp:527).
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_sprite_dma_counter: u16,
    /// R4: re-entrancy guard. `process_pending_dma` is called from `read1`;
    /// without the guard, the halt/dummy/get reads inside the loop would
    /// recurse back into themselves. Mirror of Mesen's implicit re-entry
    /// avoidance (Mesen sets `_needHalt = false` at line 497 before the
    /// halt cycle's StartCpuCycle, suppressing nested entry).
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_in_process_pending_dma: bool,
    /// R4 / S2 step (3): count of "ready" DMC pre-get cycles in the current
    /// fetch (used only when `sep_dummy >= 2`). main's `service_dmc_dma` does a
    /// FIXED number of noop cycles before the get (noop_cycles=3 -> a 4-cycle
    /// fetch); r4's flag cascade gives only halt+dummy (2 noop -> 3-cycle fetch),
    /// which under-rotates the DMASync loop (63 conflicts vs main's 905). This
    /// counter lets the get wait `sep_dummy - 1` ready cycles so the fetch length
    /// matches main's (sep_dummy=3 -> 4 cycles). Reset to 0 at each DMC latch and
    /// at fetch completion.
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_dmc_noops_done: u8,
    /// v2.0 S2: latched at DMC arm — is this fetch a RELOAD (vs the initial
    /// LOAD)? Under the `reload_span` knob a reload uses a 4-cycle span.
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_dmc_is_reload: bool,
    /// R4 / P0: true while inside `process_pending_dma`'s halt/get/sprite loop
    /// (the CPU is RDY-stalled for DMA). Under the `dma_no_irq` knob, `end_cycle`
    /// SKIPS `handle_interrupts` on these cycles so the CPU's IRQ/NMI recognition
    /// pipeline does NOT advance during the DMA halt — matching the `default`
    /// (legacy `drain_dma`) model where the DMA runs inside ONE `bus.read` and
    /// the CPU samples interrupts once per access, not per DMA cycle. r4's
    /// per-DMA-cycle `end_cycle` was advancing `mc_prev_run_irq` N extra times,
    /// breaking `cpu_interrupts_v2` #4 (irq_and_dma) that default passes.
    #[cfg(feature = "r4-cpu-dma")]
    pub(crate) r4_dma_cycle_active: bool,
    /// v2.0 F-2 put-cycle flip-flop (TriCNES reference model,
    /// `docs/audit/v2.0-f2-tricnes-reference-model-2026-06-02.md`). A single
    /// persistent get/put flip-flop toggled exactly once per CPU cycle in
    /// [`Cpu::start_cycle`] (the per-cycle chokepoint every read/write/idle/DMA
    /// cycle passes through) — the analog of TriCNES's `APU_PutCycle`
    /// (`Emulator.cs:920`) and Mesen's `CycleCount & 1`. `true` = PUT cycle
    /// (write half), `false` = GET cycle (read half). Under the `r4-put-cycle`
    /// feature this is THE get/put source for `process_pending_dma`,
    /// consolidating the three derived sources (`apu_phase` / `self.cycles & 1`
    /// / bus parity) into one counter — the prerequisite for sharing the get/put
    /// phase with the DMC arm (divergence A in the reference-model doc).
    ///
    /// SCAFFOLD STATE: seeded `false` at power-on. The power-on PHASE seed (the
    /// TriCNES `APUAlignment` analog, drawn from the determinism PRNG and paired
    /// with the DMC-timer phase) and the DMC-arm coupling are the follow-up
    /// calibration steps. Default-off ⇒ byte-identical.
    #[cfg(feature = "r4-put-cycle")]
    pub(crate) put_cycle: bool,
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
            mc_prev_nmi_line: false,
            mc_need_nmi: false,
            mc_prev_need_nmi: false,
            mc_run_irq: false,
            mc_prev_run_irq: false,
            #[cfg(feature = "r4-cpu-dma")]
            r4_need_halt: false,
            #[cfg(feature = "r4-cpu-dma")]
            r4_need_dummy_read: false,
            #[cfg(feature = "r4-cpu-dma")]
            r4_dmc_dma_running: false,
            #[cfg(feature = "r4-cpu-dma")]
            r4_sprite_dma_transfer: false,
            #[cfg(feature = "r4-cpu-dma")]
            r4_sprite_dma_offset: 0,
            #[cfg(feature = "r4-cpu-dma")]
            r4_sprite_dma_counter: 0,
            #[cfg(feature = "r4-cpu-dma")]
            r4_in_process_pending_dma: false,
            #[cfg(feature = "r4-cpu-dma")]
            r4_dmc_noops_done: 0,
            #[cfg(feature = "r4-cpu-dma")]
            r4_dmc_is_reload: false,
            #[cfg(feature = "r4-cpu-dma")]
            r4_dma_cycle_active: false,
            // v2.0 F-2: seed the put-cycle flip-flop. `false` = the next cycle is
            // a GET half. The phase seed (APUAlignment analog from the determinism
            // PRNG) is the calibration follow-up; for the scaffold a fixed seed.
            #[cfg(feature = "r4-put-cycle")]
            put_cycle: false,
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
        // R3 axis #2 cold-boot timing fix: advance master_clock by one CPU
        // divider (NTSC 12 mc / PAL 16 mc / Dendy 15 mc) BEFORE the 8-cycle
        // reset loop, matching Mesen2 `NesCpu::Reset()`:
        //   `_masterClock += cpuDivider + cpuOffset;` (NesCpu.cpp:246)
        //   `for(int i = 0; i < 8; i++) { StartCpuCycle(true); EndCpuCycle(true); }`
        // (NesCpu.cpp:249). Without this, the first start_cycle's
        // `run_ppu_to(mc - 1)` runs the PPU to mc=4 (1 tick) instead of
        // mc=16 (4 ticks), leaving the PPU 3 dots behind Mesen at the end
        // of reset. The 4-PPU-dot phase offset documented in
        // `docs/audit/v2.0-r3-mmc3-cross-diff-2026-05-28.md` axis #2
        // section originates here.
        self.master_clock = self.master_clock.wrapping_add(CPU_DIVIDER_NTSC);
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
        // v2.0 master-clock NMI service (cpu-c1-attempt-17 only): dispatch on
        // the one-cycle-delayed `mc_prev_need_nmi` gate (TetaNES `cpu.rs:802`
        // gates on `PREV_NMI`). NMI has priority over a co-pending IRQ. The
        // legacy `armed_nmi` path below stays inert under this feature
        // (`nmi_first_tick` is never sampled, so `promote` never arms it).
        // v2.0 master-clock UNIFIED interrupt dispatch (cpu-c1-attempt-17 only),
        // matching Mesen2 `NesCpu::Exec` (NesCpu.cpp:205) + TetaNES `clock`
        // (cpu.rs:802): a SINGLE service sequence gated on the one-cycle-delayed
        // `prev_*` copies (`_prevRunIrq || _prevNeedNmi` / `PREV_RUN_IRQ |
        // PREV_NMI`). The vector is chosen INSIDE `service_interrupt` by the LIVE
        // `mc_need_nmi` at cycle 5 — the live-vs-delayed asymmetry IS the NMI
        // hijack. NMI priority is resolved there, not here: when both are pending
        // the cycle-5 latch reads `mc_need_nmi`, picks `$FFFA`, and drops the IRQ
        // (its source stays asserted and re-fires after the NMI handler). Setting
        // `irq_sample_i_flag` BEFORE `service_interrupt` masks the φ2 sampler for
        // the 7-cycle sequence (no re-entry). Clean-room from the authoritative
        // model.
        if self.mc_prev_run_irq || self.mc_prev_need_nmi {
            self.armed_irq = false;
            self.irq_sample_i_flag = true;
            // The dispatch runs at the TOP of step(), BEFORE the per-instruction
            // reset of `skip_irq_sample` (further down). If the interrupted
            // instruction was a taken/branch opcode it left `skip_irq_sample =
            // true`, which would make `mc_sample_interrupts` SKIP the φ2 recompute
            // for the whole 7-cycle service — freezing `mc_run_irq` at its
            // pre-service value (1) so the IRQ re-enters at the handler's first
            // instruction (cpu_interrupts_v2/5). Clear it here so the recompute
            // runs and `irq_sample_i_flag = true` actually masks `mc_run_irq` → 0.
            self.skip_irq_sample = false;
            self.service_interrupt(bus, IRQ_VECTOR, false);
            // Defer any STILL-pending NMI by one instruction (the "≥1 handler
            // instruction runs before the next interrupt" rule, same as BRK at
            // NesCpu.cpp:271). When this service was an IRQ with a co-/late-pending
            // NMI (`mc_need_nmi` not consumed by the cycle-5 hijack), the per-cycle
            // φ2 sampler re-armed `mc_prev_need_nmi = 1` during the 7-cycle service;
            // without this clear the next `step()` would dispatch the NMI
            // immediately at the handler's 1st instruction (cpu_interrupts_v2/3
            // NMI-after-IRQ re-entry). `mc_need_nmi` stays set, so the NMI re-arms
            // and fires after one handler instruction. If the service WAS the NMI
            // (hijack consumed `mc_need_nmi`), this is a harmless no-op.
            self.mc_prev_need_nmi = false;
            self.promote_post_step_interrupts(7);
            return 7;
        }
        // Service an armed interrupt before the next instruction.  NMI has
        // priority over IRQ; both are mutually exclusive for a single
        // service window.
        //
        // A2-step-2 (combo): the master-clock mc-dispatch above
        // (`mc_prev_run_irq || mc_prev_need_nmi`) is the SOLE interrupt-recognition
        // path under the combo. The legacy `armed_nmi`/`armed_irq` path is disabled
        // there because it sets `irq_sample_i_flag = true` only AFTER
        // `service_interrupt` runs — so an IRQ taken through it runs its 7-cycle
        // service UNMASKED, leaving `run_irq` asserted and re-entering at the
        // handler's first instruction (cpu_interrupts_v2/5 re-entry). The mc
        // dispatch masks BEFORE the service, matching Mesen's "≥1 handler
        // instruction runs before the next interrupt". (R0-step-2: the legacy
        // `armed_nmi`/`armed_irq` dispatch blocks were here under
        // `cfg(not(cpu-c1-attempt-17-access-reorder))` and are now DELETED —
        // the single clean path is the mc dispatch above.)

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

        let opcode = self.fetch_pc(bus);
        let mut cycles = 0u8;
        self.dispatch(bus, opcode, &mut cycles);
        // Burn whichever cycles the dispatch did NOT emit through helpers.
        // As opcodes migrate to fully per-cycle emission, this loop runs
        // for fewer iterations; eventually it can be removed entirely.
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

    /// v2.0 master-clock interrupt edge detector (`cpu-c1-attempt-17` only),
    /// sampled at φ2 (end of every CPU cycle). Faithful port of `TetaNES`'s
    /// `handle_interrupts` (`cpu.rs:256-272`):
    /// - **NMI**: copy the latch forward FIRST (the one-cycle `prev_need_nmi`
    ///   delay), then latch a fresh rising /NMI edge from the raw line level.
    /// - **IRQ**: copy `mc_run_irq` into `mc_prev_run_irq` (the "end of the
    ///   second-to-last cycle" delay), then recompute the I-masked level.
    ///   I-masked by `irq_sample_i_flag` (the start-of-instruction snapshot
    ///   that encodes the CLI/SEI/PLP one-instruction delay; RTI updates it
    ///   mid-instruction) — `1-cli_latency` brackets this. `skip_irq_sample`
    ///   (set by taken branches) suppresses the recompute on the extra branch
    ///   cycles (the `branch_delays_irq` quirk).
    // `bus` is `&mut B` to match the call sites; the sampled lines are
    // `&self`/snapshot queries, hence not used mutably.
    #[allow(clippy::needless_pass_by_ref_mut)]
    fn handle_interrupts<B: Bus>(&mut self, bus: &mut B) {
        self.mc_prev_need_nmi = self.mc_need_nmi;
        let nmi_level = bus.nmi_level();
        if !self.mc_prev_nmi_line && nmi_level {
            self.mc_need_nmi = true;
        }
        // (R0-step-2: the v1.x `take_nmi_write_edge` latch is GONE — under the
        // clean double catch-up the PPU is ticked to the access's exact mc
        // BEFORE the bus.write returns, so the /NMI rising-edge transient from a
        // `$2000` write is captured by this level sample on the same cycle.
        // ppu_vbl_nmi/07-nmi_on_timing remains in scope for R2's predicate work.)
        self.mc_prev_nmi_line = nmi_level;
        self.mc_prev_run_irq = self.mc_run_irq;
        {
            let irq_level = bus.irq_level();
            // Recompute `mc_run_irq` UNCONDITIONALLY every end_cycle (Mesen2
            // `EndCpuCycle` / TetaNES `handle_interrupts`). The taken-branch IRQ
            // delay is NOT modelled by freezing this recompute (the old
            // `skip_irq_sample` gate), which both delayed legitimate recognition
            // on the cycles after a branch AND broke the page-cross extra poll;
            // instead `Cpu::branch` suppresses a JUST-asserted IRQ once via
            // `run_irq && !prev_run_irq` and the dummy reads re-sample here.
            //
            // I-masked by `irq_sample_i_flag` (the start-of-instruction snapshot),
            // NOT live `self.p`: CLI/SEI/PLP delay their I-change one instruction
            // (the sample sees the OLD I), RTI observes it immediately (its arm
            // updates the snapshot mid-instruction); `1-cli_latency` PLP #7
            // brackets this. Re-entry during a service is prevented by the
            // dispatch setting `irq_sample_i_flag = true` BEFORE `service_interrupt`.
            self.mc_run_irq = irq_level && !self.irq_sample_i_flag;
        }
        #[cfg(feature = "irq-timing-trace")]
        self.mc_trace_row(bus, "cyc");
    }

    /// Per-cycle CSV trace row matching the Mesen2 oracle's 12-column schema
    /// (`scripts/mesen2-irq-oracle/`), for cross-diffing the IRQ-sample model.
    /// Env-gated by `RUSTYNES_IRQ_TRACE_CSV` (path) + `_START`/`_END`
    /// (cpu-cycle window for `cyc` rows; service rows always emitted).
    #[cfg(feature = "irq-timing-trace")]
    #[allow(clippy::needless_pass_by_ref_mut)]
    fn mc_trace_row<B: Bus>(&mut self, bus: &mut B, event: &str) {
        crate::mc_irq_csv::row(
            self.cycles,
            bus.trace_ppu_pos(),
            event,
            bus.irq_level(),
            bus.nmi_level(),
            self.mc_run_irq,
            self.mc_prev_run_irq,
            self.mc_need_nmi,
            self.mc_prev_need_nmi,
            self.p.contains(Status::INTERRUPT_DISABLE),
            self.pc,
            bus.trace_clocks(),
            bus.trace_last_read(),
            bus.trace_apu_fc(),
        );
    }

    // ------------------------------------------------------------------
    // R0 master-clock skeleton: start_cycle / end_cycle / process_pending_dma.
    //
    // Clean-room port of Mesen2 `StartCpuCycle`/`EndCpuCycle`/`ProcessPendingDma`
    // (NesCpu.cpp:388/426/434) + TetaNES `start_cycle`/`end_cycle`/`handle_dma`
    // (cpu.rs:285/297/317). The CPU owns `master_clock`; each access is wrapped
    // in `process_pending_dma` -> `start_cycle` -> `bus.read`/`bus.write` ->
    // `end_cycle`. `start_cycle` advances the master clock by the PRE half
    // (read +5 / write +7), catches the PPU up to `master_clock - PPU_OFFSET`,
    // and fires `bus.cpu_clock()` (the per-CPU-cycle APU + mapper tick).
    // `end_cycle` advances by the POST half (read +7 / write +5), catches the
    // PPU up again (the double catch-up), and samples interrupts via
    // `handle_interrupts` (Mesen `_prevRunIrq`/`_prevNeedNmi` at EndCpuCycle).
    // ------------------------------------------------------------------

    fn start_cycle<B: Bus>(&mut self, bus: &mut B, for_read: bool) {
        let pre = if for_read { READ_PRE_MC } else { WRITE_PRE_MC };
        self.master_clock = self.master_clock.wrapping_add(pre);
        bus.run_ppu_to(self.master_clock.saturating_sub(PPU_OFFSET));
        // v2.0 layer-2 core (S0 skeleton): APU/DMC double-catch-up. No-op default
        // (byte-identical); S1 makes it the real run-to-target advance.
        #[cfg(feature = "mc-apu-subcycle")]
        bus.run_apu_to(self.master_clock);
        bus.cpu_clock();
        // v2.0 F-2: toggle the put-cycle flip-flop once per CPU cycle, right after
        // the per-cycle APU tick (`bus.cpu_clock()`) — the placement TriCNES uses
        // (`APU_PutCycle = !APU_PutCycle` immediately after `_EmulateAPU()`,
        // Emulator.cs:920). `start_cycle` is the single chokepoint every CPU cycle
        // (normal read/write/idle AND every DMA halt/dummy/get/sprite cycle) passes
        // through exactly once, so this is a faithful once-per-cycle toggle.
        #[cfg(feature = "r4-put-cycle")]
        {
            self.put_cycle = !self.put_cycle;
        }
    }

    fn end_cycle<B: Bus>(&mut self, bus: &mut B, for_read: bool) {
        // Coherence: account for any bus-side DMA the access just drained.
        // The legacy (non-`r4-cpu-dma`) bus services OAM/DMC DMA internally,
        // advancing the real PPU + `ppu_clock` but NOT this `master_clock`.
        // Fold the DMA span back in here so the following `run_ppu_to`
        // target — and every later access's CPU↔PPU phase — stay coherent.
        // Without it, each DMA permanently shifts the PPU phase by the DMA
        // span, breaking DMA-relative register-read timing. Returns 0 when
        // no DMA ran (the common case) and under `r4-cpu-dma` (which
        // advances `master_clock` per DMA cycle itself).
        self.master_clock = self.master_clock.wrapping_add(bus.take_dma_mc_consumed());
        let post = if for_read {
            READ_POST_MC
        } else {
            WRITE_POST_MC
        };
        self.master_clock = self.master_clock.wrapping_add(post);
        bus.run_ppu_to(self.master_clock.saturating_sub(PPU_OFFSET));
        // v2.0 layer-2 core (S0 skeleton): APU/DMC double-catch-up (no-op default).
        #[cfg(feature = "mc-apu-subcycle")]
        bus.run_apu_to(self.master_clock);
        // P0 (#4 irq_and_dma): under `dma_no_irq`, skip the IRQ/NMI recognition
        // advance on DMA-halt cycles — the CPU is RDY-stalled and (like default's
        // drain-inside-bus.read) must not advance its interrupt pipeline per DMA
        // cycle. Normal cycles + default build always sample.
        #[cfg(feature = "r4-cpu-dma")]
        let skip_irq = self.r4_dma_cycle_active && crate::r4_knobs::knobs().dma_no_irq != 0;
        #[cfg(not(feature = "r4-cpu-dma"))]
        let skip_irq = false;
        if !skip_irq {
            self.handle_interrupts(bus);
        }
    }

    /// Process any pending DMC / OAM DMA at the TOP of a read (Mesen
    /// `ProcessPendingDma`, NesCpu.cpp:434-618). Under the `r4-cpu-dma`
    /// feature this runs the full halt/dummy/align/get state machine the
    /// CPU is responsible for; without the feature it is a no-op and the
    /// bus's legacy `drain_dma` continues to service DMA.
    ///
    /// Algorithm (when feature ON):
    /// 1. Poll `bus.dmc_dma_pending()` + `bus.take_oam_dma_page()`. If
    ///    either is newly armed, latch the corresponding CPU-side flags
    ///    (`r4_dmc_dma_running`, `r4_sprite_dma_transfer`,
    ///    `r4_sprite_dma_offset`) and set `r4_need_halt = true`.
    /// 2. Early return if no DMA is in progress and nothing new is armed.
    /// 3. Set `r4_in_process_pending_dma = true` (re-entrancy guard).
    /// 4. Halt cycle: one `read_dummy(addr)` at the held address. Clears
    ///    `r4_need_halt`.
    /// 5. Loop while DMC or OAM DMA is running:
    ///    - `get_cycle = (cycles & 1) == 0`. NOTE: Mesen uses
    ///      `(CycleCount & 1) == 0` where `CycleCount` is post-increment.
    ///      Our `self.cycles` is also post-increment (idle_tick +
    ///      read1/write1 all bump `cycles` before start_cycle). The parity
    ///      semantic matches.
    ///    - On `get_cycle`:
    ///      - If DMC ready (running + no halt/dummy pending): read at
    ///        `bus.dmc_dma_addr()`, deliver via `bus.dmc_dma_complete(byte)`.
    ///      - Else if sprite DMA: read at
    ///        `0x100 * sprite_dma_offset + sprite_dma_counter / 2`.
    ///        Increment counter.
    ///      - Else: dummy read at held address (still waiting on halt /
    ///        dummy).
    ///    - On `put_cycle`:
    ///      - If sprite DMA is on a write half (odd counter): write the
    ///        previously-read byte to `$2004`. Increment counter (wraps
    ///        at 0x200 → sprite_dma_transfer = false).
    ///      - Else: align/dummy read at held address.
    /// 6. Clear `r4_in_process_pending_dma`.
    ///
    /// The DMC abort path (`_abortDmcDma`) is NOT modelled in this first
    /// landing — the AccuracyCoin abort tests are a follow-up. The
    /// `TEST_NmiAndIrq` hang (the documented R4 target — see
    /// `project_c1_trace_loop_ceiling.md` §128) does not need abort
    /// handling because the test runs without OAM DMA interference.
    #[cfg(not(feature = "r4-cpu-dma"))]
    #[allow(
        clippy::unused_self,
        clippy::needless_pass_by_ref_mut,
        clippy::missing_const_for_fn
    )]
    fn process_pending_dma<B: Bus>(&mut self, bus: &mut B, _addr: u16) {
        let _ = bus;
    }

    #[cfg(feature = "r4-cpu-dma")]
    #[allow(clippy::too_many_lines)] // single cohesive DMA state machine; splitting hurts readability
    #[allow(clippy::cognitive_complexity)] // ditto: the get/put branch chain is the state machine
    fn process_pending_dma<B: Bus>(&mut self, bus: &mut B, addr: u16) {
        // Re-entrancy guard: the halt/dummy/get cycles inside the loop
        // call back into `read1`/`idle_tick`, which call this method
        // recursively. Mesen avoids this by setting `_needHalt = false`
        // before the first cycle (line 497); we use an explicit flag.
        if self.r4_in_process_pending_dma {
            return;
        }
        // R4 multi-dim sweep knobs (env-driven, init-once under
        // irq-timing-trace; defaults otherwise).
        let k = crate::r4_knobs::knobs();
        // Phase 1: poll the bus for newly-armed DMAs. The bus's
        // `dmc_dma_pending` reflects the APU's live state; if it's true
        // and we haven't latched yet, latch it now.
        if !self.r4_dmc_dma_running && bus.dmc_dma_pending() {
            let is_reload = bus.dmc_dma_is_reload();
            // v2.0 F-2 step 3 (DMC arm coupling): under `r4-put-cycle`, the DMC
            // schedules its halt on a GET cycle (load) or a PUT cycle (reload),
            // and a halt attempt on the wrong phase "fails, trying again on the
            // next CPU cycle" (nesdev DMA.xhtml; ares `dmaDelayCounter`). So only
            // LATCH the arm when `put_cycle` is the scheduled phase — this couples
            // the arm phase to the shared get/put counter (the prerequisite for a
            // 3/4 span coherent with `put_cycle`). The seed selects which phase is
            // GET (`!put_cycle` at seed 0). `process_pending_dma` runs only on CPU
            // read cycles (read1/idle_tick, not write1), so this naturally models
            // "halt can only succeed on a read cycle" — the wait IS the retry.
            #[cfg(feature = "r4-put-cycle")]
            let arm_ok = if k.arm_immediate != 0 {
                // v2.0 T-2: deliver the arm to the next read immediately (Mesen
                // `ProcessPendingDma` halts on the next read, no phase gate). The
                // DMC byte-timer is already get/put-coherent (T-1, integrated
                // counter), so the §890 phase latch is redundant; only the GET is
                // phase-gated (the alignment dummy) → the variable 3/4 span sweeps
                // the halt onto the `$4000` cycle-4 read.
                true
            } else {
                let get_phase = if k.put_cycle_seed != 0 {
                    self.put_cycle
                } else {
                    !self.put_cycle
                };
                if is_reload {
                    !get_phase
                } else {
                    get_phase
                }
            };
            #[cfg(not(feature = "r4-put-cycle"))]
            let arm_ok = true;
            if arm_ok {
                self.r4_dmc_dma_running = true;
                self.r4_need_halt = true;
                self.r4_need_dummy_read = true;
                // S2 step (3): fresh fixed-noop counter for this DMC fetch.
                self.r4_dmc_noops_done = 0;
                // v2.0 S2: latch load-vs-reload at arm for the reload-span fix.
                self.r4_dmc_is_reload = is_reload;
            }
        }
        if !self.r4_sprite_dma_transfer {
            if let Some(page) = bus.take_oam_dma_page() {
                self.r4_sprite_dma_transfer = true;
                self.r4_sprite_dma_offset = page;
                self.r4_sprite_dma_counter = 0;
                self.r4_need_halt = true;
            }
        }
        // Phase 2: no DMA in progress?
        if !self.r4_dmc_dma_running && !self.r4_sprite_dma_transfer {
            return;
        }
        // Phase 3: re-entry guard ON.
        self.r4_in_process_pending_dma = true;
        // P0: mark all halt/get/sprite cycles below as DMA-stall cycles so
        // end_cycle can skip the CPU IRQ/NMI recognition advance (dma_no_irq).
        self.r4_dma_cycle_active = true;
        // R4 root-cause (§157): capture the TOTAL OAM-DMA span entry BEFORE the
        // halt cycle, so r4's total (halt + align + transfer) is directly
        // comparable to legacy_oam_trace's pre-drain total. (γ'' captured this
        // AFTER the halt, undercounting r4's span by the 1 halt cycle.)
        #[cfg(feature = "irq-timing-trace")]
        let r4_oam_entry = (
            self.cycles,
            self.master_clock,
            self.r4_sprite_dma_offset,
            addr,
        );
        // Phase 4: halt cycle (one dummy read at the held address).
        // Mesen NesCpu.cpp:497-508: `_needHalt = false; StartCpuCycle(true);
        // _memoryManager->Read(readAddress, MemoryOperationType::DmaRead);
        // EndCpuCycle(true);`. We model the same with `read_dummy`.
        self.r4_need_halt = false;
        #[cfg(feature = "irq-timing-trace")]
        crate::dma_loop_trace::row(
            self.cycles.wrapping_add(1),
            self.master_clock,
            0, // kind 0 = halt
            addr,
            bus.irq_level(),
            bus.nmi_level(),
            self.r4_need_halt,
            self.r4_need_dummy_read,
            self.r4_dmc_dma_running,
            self.r4_sprite_dma_transfer,
            self.r4_sprite_dma_counter,
            bus.dmc_arm_cycle(),
        );
        self.read_dummy(bus, addr);
        // Phase 5: main loop. Mirror Mesen's `processCycle` semantics
        // (NesCpu.cpp:531-544): at the TOP of every cycle (after the
        // halt), clear EITHER `_needHalt` OR `_needDummyRead` (whichever
        // is currently set; cascade order). This guarantees that after
        // the halt cycle clears `_needHalt`, the NEXT cycle clears
        // `_needDummyRead`, and the cycle AFTER THAT can serve as the
        // DMC get-cycle. Without this clear-at-top, my prior code
        // permanently locked out DMC fetches when a put-cycle landed
        // before need_dummy_read was cleared.
        let mut sprite_read_byte: u8 = 0;
        // R4-γ'' OAM-DMA span trace: entry now captured BEFORE the halt (§157,
        // see Phase 3 above) so the emitted span is r4's TOTAL, comparable to
        // legacy_oam_trace. Emitted at completion (counter == 0x200).
        while self.r4_dmc_dma_running || self.r4_sprite_dma_transfer {
            // SWEEP KNOB: mid-loop DMC re-poll. When ON, catch APU DMC
            // arming mid-OAM-DMA (refuted on cpu_interrupts_v2/4 which has
            // no DMC, but may help on AccuracyCoin TEST_NmiAndIrq which
            // DOES use DMC via DMASync_50CyclesRemaining).
            if k.mid_loop_dmc_poll != 0 && !self.r4_dmc_dma_running && bus.dmc_dma_pending() {
                self.r4_dmc_dma_running = true;
                self.r4_need_dummy_read = true;
            }
            // S2 step (2) fix #2: capture readiness BEFORE the cascade clears a
            // flag this iteration. With `sep_dummy`, the DMC GET fires only when
            // the flags were ALREADY clear at iteration start (so clearing
            // need_dummy_read gets its own dummy cycle = a 3-4 cycle fetch like
            // main, de-locking the cluster DMASync loop). sep_dummy=0 keeps the
            // current post-cascade behaviour (2-cycle fetch).
            let was_ready_pre_cascade = !self.r4_need_halt && !self.r4_need_dummy_read;
            // SWEEP KNOB: processCycle flag cascade. Default Mesen order
            // (needHalt first → needDummyRead). cascade=1 inverts.
            if k.cascade == 0 {
                if self.r4_need_halt {
                    self.r4_need_halt = false;
                } else if self.r4_need_dummy_read {
                    self.r4_need_dummy_read = false;
                }
            } else if self.r4_need_dummy_read {
                self.r4_need_dummy_read = false;
            } else if self.r4_need_halt {
                self.r4_need_halt = false;
            }
            // §157 ROOT-CAUSE FIX: get/put parity. The DEFAULT is now
            // 1 = ODD=get — the emulator's established get/put phase
            // convention (legacy `drain_dma` bus.rs:2328 "ODD CPU cycle ==
            // get, EVEN == put"; the APU `apu_phase` shares it). r4's CPU
            // counter `self.cycles` and the bus counter `self.cycle` have
            // OPPOSITE parity at DMA entry, so the old even=get default
            // (the literal Mesen `(CycleCount&1)==0`, which assumes Mesen's
            // counter phase) made r4 spend ONE EXTRA alignment cycle on the
            // majority of OAM DMAs (span 514 where legacy/Mesen emit 513).
            // That +1 re-phased every subsequent APU frame-counter /
            // controller-strobe sample, causing the §151 regressions:
            // Frame Counter IRQ/4-step/5-step + Controller Strobing. Setting
            // ODD=get equalizes r4's OAM span to legacy's per-DMA and flips
            // those 4 tests fail->pass with ZERO AccuracyCoin regressions.
            // (`Sprite Evaluation :: $2002 flag timing` is a SEPARATE r4
            // defect, NOT fixed by this.) The env knob still overrides for
            // A/B sweeps. See docs/audit/r4-oam-span-plus1-rootcause-2026-05-30.md.
            // 0 = EVEN=get (the old default; Mesen literal), 1 = ODD=get.
            // v2.0 Phase 0a: under `apu_half_get`, source get/put from the REAL
            // APU clock half (`bus.dma_get_cycle_next()` = `apu_phase_next`) —
            // the canonical nesdev get/put = APU-clock-half. Otherwise the legacy
            // `self.cycles & 1` proxy (`get_parity`). Default (knob 0) byte-identical.
            // v2.0 R-1 split-source: when `oam_self_parity` is set, gate the OAM
            // sprite read/write/align on the `self.cycles` CPU-cycle parity
            // (ODD=get) instead of `apu_phase`. The boot OAM is Mesen-EXACT
            // under this source (513, exit_mc 1299792) but +1 under apu_phase.
            // The DMC get uses the `sep_dummy>=2` fixed branch (NOT get_cycle),
            // so this leaves DMC timing untouched — the get/put SOURCE differs by
            // DMA type. See R4Knobs::oam_self_parity + the R-1 de-risk doc.
            // v2.0 F-2 scaffold: under `r4-put-cycle` the dedicated put-cycle
            // flip-flop is THE get/put source (GET = read half = `!put_cycle`,
            // matching TriCNES `OAMDMA_Get`/`DMCDMA_Get` running on `!APU_PutCycle`).
            // This consolidates the three derived sources below into one counter.
            // NOTE: the decision precedes this cycle's `start_cycle` toggle, so the
            // value read here is last cycle's — a fixed phase offset that the
            // power-on seed (the calibration follow-up) absorbs.
            // v2.0 F-2 calibration 2: the power-on phase seed selects which
            // alternating phase is the GET half. Seed 0 = the literal TriCNES
            // GET = `!put_cycle`; seed 1 = the opposite power-on phase. The
            // de-lock parity (config B's ODD=get) sits on exactly one of these.
            #[cfg(feature = "r4-put-cycle")]
            let get_cycle = if k.put_cycle_seed != 0 {
                self.put_cycle
            } else {
                !self.put_cycle
            };
            #[cfg(not(feature = "r4-put-cycle"))]
            let get_cycle = if k.oam_self_parity != 0 {
                (self.cycles & 1) != 0
            } else if k.apu_half_get != 0 {
                bus.dma_get_cycle_next()
            } else if k.get_parity == 0 {
                (self.cycles & 1) == 0
            } else {
                (self.cycles & 1) != 0
            };
            // R4 DMA-loop trace: pre-cycle snapshot. The actual kind is set
            // by which branch fires below; we log INSIDE each branch with
            // the correct kind. `self.cycles.wrapping_add(1)` is the cycle
            // INDEX the upcoming start_cycle will assign — matches Mesen's
            // post-increment CycleCount logging convention.
            #[cfg(feature = "irq-timing-trace")]
            let pre_cycles = self.cycles.wrapping_add(1);
            #[cfg(feature = "irq-timing-trace")]
            let pre_irq = bus.irq_level();
            #[cfg(feature = "irq-timing-trace")]
            let pre_nmi = bus.nmi_level();
            // S2 step (2) fix #2: when sep_dummy, require the flags to have been
            // clear BEFORE this iteration's cascade (separate dummy cycle);
            // otherwise the current post-cascade readiness.
            let dmc_ready = if k.sep_dummy != 0 {
                was_ready_pre_cascade
            } else {
                !self.r4_need_halt && !self.r4_need_dummy_read
            };
            // S2 step (3): main's `service_dmc_dma` fires the DMC get
            // UNCONDITIONALLY after its fixed noop cycles — it has NO get/put
            // parity gating for the DMC (parity is an OAM-only concept). r4's
            // parity-gated DMC get forced the get onto ONE parity, making the
            // $4000 DMASync conflict (wants ODD) and the §59 Test E $4013 fire
            // (wants EVEN) irreconcilable on the single `get_parity` knob. With
            // `sep_dummy >= 2` the DMC get fires as soon as it is ready
            // regardless of parity (= main's unconditional get); the get/put
            // parity stays in effect ONLY for the OAM sprite read/write
            // alternation. `sep_dummy` 0/1 keep the legacy parity-gated DMC get.
            // S2 step (3): with `sep_dummy >= 2` the DMC get is unconditional
            // (no parity gating) and fires after `sep_dummy - 1` ready pre-get
            // cycles, so the fetch length is tunable to match main's fixed
            // noop_cycles=3 (sep_dummy=3 -> halt+dummy+dummy+get = 4 cycles =
            // main; the 3-cycle sep_dummy=2 under-rotates the DMASync loop).
            // sep_dummy 0/1 keep the legacy parity-gated get.
            let dmc_fire = if cfg!(feature = "r4-put-cycle") {
                // v2.0 F-2 calibration 1: route the DMC get through the put-cycle
                // flip-flop. The DMC fetch fires on a GET half (`!put_cycle` =
                // `get_cycle`) once halt+dummy have cleared (post-cascade), inserting
                // align dummies on the intervening PUT halves -> the variable 3/4
                // span keyed on the ONE get/put counter shared with the OAM DMA
                // (TriCNES `DMCDMA_Get` runs on a GET cycle after the halt clears).
                // Replaces the `sep_dummy=2` unconditional get; this is where the
                // de-lock comes from (cf. config B's ~102 866 DMC-loop arms). The
                // power-on seed + the DMC-arm coupling (which resolve the residual
                // Test-E/overlap stall) are the follow-up calibration steps.
                //
                // NOTE: the DMC get fires post-cascade (halt+dummy cleared) on the
                // GET half. The pre-cascade `was_ready_pre_cascade` form (a 3/4 span)
                // was REFUTED both standalone (runaway) AND with the arm coupling
                // (seed 0 starves to 43 arms / not_run 74; seed 1 runs away to 1.8M
                // / not_run 61) — the proper 3/4 span needs the per-cycle loop
                // restructure (divergence C), not a loop-readiness flip.
                self.r4_dmc_dma_running
                    && !self.r4_need_halt
                    && !self.r4_need_dummy_read
                    && get_cycle
            } else if k.dmc_bus_parity != 0 {
                // v2.0 DMC-core Step 1: fire the DMC get on the first even-BUS-cycle
                // after halt+dummy clear (Mesen `dmc-get-put-scheduler` convention,
                // bus.rs:3003), inserting align dummies until then -> variable 3/4
                // span. The BUS `self.cycle` has OPPOSITE parity to the CPU
                // `self.cycles` at DMA entry (§157), so this is DISTINCT from the
                // refuted config-B (CPU-parity). OAM stays on apu_phase (get_cycle).
                // `bus.cpu_clock()` (start_cycle) increments bus.cycle per DMA cycle,
                // so the parity alternates; the `>= 3` safety bound prevents an
                // infinite DMA loop if it ever does not (force-fire fallback).
                if self.r4_dmc_dma_running && !self.r4_need_halt && !self.r4_need_dummy_read {
                    self.r4_dmc_noops_done = self.r4_dmc_noops_done.saturating_add(1);
                    (bus.cycle_count() & 1) == 0 || self.r4_dmc_noops_done >= 3
                } else {
                    false
                }
            } else if k.sep_dummy >= 2 {
                if self.r4_dmc_dma_running && dmc_ready {
                    self.r4_dmc_noops_done = self.r4_dmc_noops_done.saturating_add(1);
                    // v2.0 S2: a RELOAD fetch gets +1 cycle (4-cycle span) under
                    // the reload_span knob — de-locks the `DMASync` `$4000` conflict
                    // while LOADs stay at the base span (so Test E does not stall).
                    let eff = if k.reload_span != 0 && self.r4_dmc_is_reload {
                        k.sep_dummy.saturating_add(1)
                    } else {
                        k.sep_dummy
                    };
                    self.r4_dmc_noops_done >= eff.saturating_sub(1)
                } else {
                    false
                }
            } else {
                self.r4_dmc_dma_running && dmc_ready && get_cycle
            };
            if dmc_fire {
                // DMC get cycle: read at dmc_dma_addr via the
                // 2-bus-aware `dma_read_dmc` hook (DMC drives only the
                // external bus, NOT the internal), then deliver the
                // byte to the APU.
                #[cfg(feature = "irq-timing-trace")]
                crate::dma_loop_trace::row(
                    pre_cycles,
                    self.master_clock,
                    1,
                    addr,
                    pre_irq,
                    pre_nmi,
                    self.r4_need_halt,
                    self.r4_need_dummy_read,
                    self.r4_dmc_dma_running,
                    self.r4_sprite_dma_transfer,
                    self.r4_sprite_dma_counter,
                    bus.dmc_arm_cycle(),
                );
                let dmc_addr = bus.dmc_dma_addr();
                let byte = self.read_dummy_dmc(bus, dmc_addr);
                bus.dmc_dma_complete(byte);
                self.r4_dmc_dma_running = false;
                // S2 step (3): reset the fixed-noop counter for the next fetch.
                self.r4_dmc_noops_done = 0;
            } else if get_cycle {
                if self.r4_sprite_dma_transfer {
                    // Sprite DMA read cycle: read at OAM source page via
                    // `dma_read_oam` (the $4000-$401F APU-silent gate).
                    #[cfg(feature = "irq-timing-trace")]
                    crate::dma_loop_trace::row(
                        pre_cycles,
                        self.master_clock,
                        2,
                        addr,
                        pre_irq,
                        pre_nmi,
                        self.r4_need_halt,
                        self.r4_need_dummy_read,
                        self.r4_dmc_dma_running,
                        self.r4_sprite_dma_transfer,
                        self.r4_sprite_dma_counter,
                        bus.dmc_arm_cycle(),
                    );
                    let src = (u16::from(self.r4_sprite_dma_offset) << 8)
                        | (self.r4_sprite_dma_counter >> 1);
                    sprite_read_byte = self.read_dummy_oam(bus, src);
                    self.r4_sprite_dma_counter = self.r4_sprite_dma_counter.wrapping_add(1);
                } else {
                    // DMC running but not ready: dummy read at held addr.
                    #[cfg(feature = "irq-timing-trace")]
                    crate::dma_loop_trace::row(
                        pre_cycles,
                        self.master_clock,
                        4,
                        addr,
                        pre_irq,
                        pre_nmi,
                        self.r4_need_halt,
                        self.r4_need_dummy_read,
                        self.r4_dmc_dma_running,
                        self.r4_sprite_dma_transfer,
                        self.r4_sprite_dma_counter,
                        bus.dmc_arm_cycle(),
                    );
                    self.read_dummy(bus, addr);
                }
            } else {
                // Put cycle.
                if self.r4_sprite_dma_transfer && (self.r4_sprite_dma_counter & 0x01) == 1 {
                    // Sprite DMA write — DIRECTLY to OAM via `dma_write_oam`.
                    #[cfg(feature = "irq-timing-trace")]
                    crate::dma_loop_trace::row(
                        pre_cycles,
                        self.master_clock,
                        3,
                        addr,
                        pre_irq,
                        pre_nmi,
                        self.r4_need_halt,
                        self.r4_need_dummy_read,
                        self.r4_dmc_dma_running,
                        self.r4_sprite_dma_transfer,
                        self.r4_sprite_dma_counter,
                        bus.dmc_arm_cycle(),
                    );
                    self.write_dummy_oam(bus, sprite_read_byte);
                    self.r4_sprite_dma_counter = self.r4_sprite_dma_counter.wrapping_add(1);
                    if self.r4_sprite_dma_counter >= 0x200 {
                        self.r4_sprite_dma_transfer = false;
                        // R4-γ'' OAM-DMA span trace: emit at completion.
                        #[cfg(feature = "irq-timing-trace")]
                        crate::r4_oam_trace::row(
                            r4_oam_entry.0,
                            self.cycles,
                            r4_oam_entry.1,
                            self.master_clock,
                            r4_oam_entry.2,
                            r4_oam_entry.3,
                        );
                    }
                } else {
                    // Align / dummy on the put half.
                    #[cfg(feature = "irq-timing-trace")]
                    crate::dma_loop_trace::row(
                        pre_cycles,
                        self.master_clock,
                        5,
                        addr,
                        pre_irq,
                        pre_nmi,
                        self.r4_need_halt,
                        self.r4_need_dummy_read,
                        self.r4_dmc_dma_running,
                        self.r4_sprite_dma_transfer,
                        self.r4_sprite_dma_counter,
                        bus.dmc_arm_cycle(),
                    );
                    self.read_dummy(bus, addr);
                }
            }
        }
        self.r4_in_process_pending_dma = false;
        self.r4_dma_cycle_active = false;
    }

    /// R4 helper: perform a one-cycle dummy read at `addr` without
    /// recursing into `process_pending_dma`. Used inside the DMA loop
    /// for halt / dummy / align / sprite-read cycles. Mirrors `read1`'s
    /// `start_cycle(read)` → `bus.read(addr)` → `end_cycle(read)`
    /// shape without the recursive DMA check.
    ///
    /// Canonical Mesen-faithful behavior: DMA helpers increment
    /// self.cycles like normal CPU cycles. Option-1 (§139) adds a
    /// `bus.notify_dma_stall_cycle()` call so the bus's
    /// `dma_stall_count` counter accumulates only true DMA stalls —
    /// SH* detection can use this instead of `bus.cycle_count()`
    /// for an unambiguous "did DMA fire?" signal.
    #[cfg(feature = "r4-cpu-dma")]
    fn read_dummy<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        bus.notify_dma_stall_cycle();
        self.cycles_emitted = self.cycles_emitted.saturating_add(1);
        self.cycles = self.cycles.wrapping_add(1);
        self.start_cycle(bus, true);
        // §150 R4-β "deeper port" — passive halt via `dma_halt_read` instead
        // of `bus.read`. The CPU is in RDY-stall during DMA halt/dummy/align
        // cycles, so a fresh `bus.read` would fire register side-effects
        // (e.g. $2002 VBL clear, $4015 frame-IRQ clear, $4016 controller
        // bump) that real silicon doesn't see on a halt cycle. Legacy
        // `service_dmc_dma`'s halt uses `replay_dma_noop_read + tick_one_cpu_cycle`,
        // which the LockstepBus impl of `dma_halt_read` mirrors. Closes the
        // 5 R4-default regressions (Frame Counter cluster + $2002 flag
        // timing + Controller Strobing).
        let v = bus.dma_halt_read(addr);
        self.end_cycle(bus, true);
        v
    }

    /// R4 helper: perform a one-cycle write at `addr` (sprite DMA put).
    /// Same shape as `write1` minus the recursive DMA check.
    /// Kept as scaffolding for future generic-DMA write paths; currently
    /// the only put-cycle write site uses [`Self::write_dummy_oam`] for
    /// OAM-direct semantics.
    #[cfg(feature = "r4-cpu-dma")]
    #[allow(dead_code)]
    fn write_dummy<B: Bus>(&mut self, bus: &mut B, addr: u16, value: u8) {
        self.cycles_emitted = self.cycles_emitted.saturating_add(1);
        self.cycles = self.cycles.wrapping_add(1);
        self.start_cycle(bus, false);
        bus.write(addr, value);
        self.end_cycle(bus, false);
    }

    /// R4 helper: DMC get-cycle read. Canonical Mesen + Option-1 stall
    /// accounting.
    #[cfg(feature = "r4-cpu-dma")]
    fn read_dummy_dmc<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        bus.notify_dma_stall_cycle();
        self.cycles_emitted = self.cycles_emitted.saturating_add(1);
        self.cycles = self.cycles.wrapping_add(1);
        self.start_cycle(bus, true);
        let v = bus.dma_read_dmc(addr);
        self.end_cycle(bus, true);
        v
    }

    /// R4 helper: OAM-DMA read. Canonical Mesen + Option-1 stall accounting.
    #[cfg(feature = "r4-cpu-dma")]
    fn read_dummy_oam<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        bus.notify_dma_stall_cycle();
        self.cycles_emitted = self.cycles_emitted.saturating_add(1);
        self.cycles = self.cycles.wrapping_add(1);
        self.start_cycle(bus, true);
        let v = bus.dma_read_oam(addr);
        self.end_cycle(bus, true);
        v
    }

    /// R4 helper: OAM-DMA write via `Bus::dma_write_oam`. Writes DIRECTLY
    /// to PPU OAM (NOT through the $2004 register write path) — DMA
    /// writes bypass the OAMADDR-during-rendering corruption that
    /// applies only to CPU $2004 writes (Ppu::oam_dma_write,
    /// ppu.rs:685-691).
    ///
    /// **Timing**: sweep knob `RUSTYNES_R4_OAM_WRITE_SEM`. Canonical
    /// Mesen behavior + Option-1 DMA-stall accounting.
    #[cfg(feature = "r4-cpu-dma")]
    fn write_dummy_oam<B: Bus>(&mut self, bus: &mut B, byte: u8) {
        let for_read = crate::r4_knobs::knobs().oam_write_sem != 0;
        bus.notify_dma_stall_cycle();
        self.cycles_emitted = self.cycles_emitted.saturating_add(1);
        self.cycles = self.cycles.wrapping_add(1);
        self.start_cycle(bus, for_read);
        bus.dma_write_oam(byte);
        self.end_cycle(bus, for_read);
    }

    /// Consume one CPU cycle with no bus access — a 6502 internal cycle.
    /// In the clean v2.0 model this is `start_cycle(for_read=true) +
    /// end_cycle(for_read=true)` matching Mesen's idle-cycle shape
    /// (read split, no actual read). Advances `master_clock` by a full
    /// CPU cycle, fires both PPU catch-ups, and samples interrupts at
    /// `end_cycle` via `handle_interrupts`.
    ///
    /// Under `r4-cpu-dma`: also calls `process_pending_dma` at the top
    /// (matching Mesen — its idle cycles do a `MemoryRead` of
    /// `MemoryOperationType::DummyRead` which routes through
    /// `ProcessPendingDma`). Without this, implied opcodes (CLC, SEI,
    /// INX, DEX, ASL A, etc.) skip DMC servicing entirely — accumulating
    /// a 12.6× DMA-cycle deficit vs the legacy `drain_dma` path which
    /// fired every CPU cycle via `cpu_clock`. Empirically confirmed via
    /// sh_store trace: legacy 1137 vs R4 90 DMA-stall cycles before the
    /// first SH* event (see project_c1_trace_loop_ceiling.md §136).
    fn idle_tick<B: Bus>(&mut self, bus: &mut B) {
        // S2 step (2): see read1 — post-tick DMC-halt placement under
        // `mc-dmc-post-tick` (after start_cycle's APU tick).
        #[cfg(all(feature = "r4-cpu-dma", not(feature = "mc-dmc-post-tick")))]
        self.process_pending_dma(bus, self.pc);
        self.cycles_emitted = self.cycles_emitted.saturating_add(1);
        self.cycles = self.cycles.wrapping_add(1);
        self.start_cycle(bus, true);
        #[cfg(all(feature = "r4-cpu-dma", feature = "mc-dmc-post-tick"))]
        self.process_pending_dma(bus, self.pc);
        self.end_cycle(bus, true);
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
        #[cfg(feature = "cpu-implied-dummy-reads")]
        {
            let _ = self.read1(bus, self.pc);
        }
        #[cfg(not(feature = "cpu-implied-dummy-reads"))]
        let _ = bus;
    }

    /// Read a byte at `addr` and consume one CPU cycle. Clean v2.0 access
    /// shape: `process_pending_dma(addr)` → `start_cycle(read=true)` →
    /// `bus.read(addr)` → `end_cycle(read=true)`. The PPU has been caught
    /// up to `master_clock - PPU_OFFSET` BEFORE the read, so the read
    /// observes PPU state at the access's exact master clock. Mirrors
    /// Mesen2 `MemoryRead` (NesCpu.cpp:355).
    fn read1<B: Bus>(&mut self, bus: &mut B, addr: u16) -> u8 {
        // S2 step (2): DMC-halt decision placement. Default (Mesen order) =
        // BEFORE start_cycle (observes the previous cycle's pending). Under
        // `mc-dmc-post-tick` = AFTER start_cycle's APU tick (matching main's
        // drain-inside-bus.read), so the DMC GET lands on the held read it's
        // concurrent with (the $4000 conflict), not one cycle early.
        #[cfg(not(feature = "mc-dmc-post-tick"))]
        self.process_pending_dma(bus, addr);
        self.cycles_emitted = self.cycles_emitted.saturating_add(1);
        self.cycles = self.cycles.wrapping_add(1);
        self.start_cycle(bus, true);
        #[cfg(feature = "mc-dmc-post-tick")]
        self.process_pending_dma(bus, addr);
        let v = bus.read(addr);
        self.end_cycle(bus, true);
        v
    }

    /// Write `value` to `addr` and consume one CPU cycle. Symmetric write
    /// split (writes commit 2 mc later in the cycle than reads, Mesen
    /// NesCpu.cpp:430).
    fn write1<B: Bus>(&mut self, bus: &mut B, addr: u16, value: u8) {
        self.cycles_emitted = self.cycles_emitted.saturating_add(1);
        self.cycles = self.cycles.wrapping_add(1);
        self.start_cycle(bus, false);
        bus.write(addr, value);
        self.end_cycle(bus, false);
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
        // DMA can halt). Option-1 §139: use `bus.dma_stall_count()`
        // which only counts DMA-stall cycles (not the read itself).
        // ANY change > 0 means DMA fired. Fallback to legacy
        // `bus.cycle_count` diff > 1 if dma_stall_count is unmodified
        // (test stubs return 0 unchanged).
        let cyc_before = bus.cycle_count();
        let stall_before = bus.dma_stall_count();
        #[cfg(feature = "irq-timing-trace")]
        let trace_cpu_before = self.cycles;
        #[cfg(feature = "irq-timing-trace")]
        let trace_idb_before = bus.internal_data_bus();
        let dummy_addr = if page_crossed {
            addr.wrapping_sub(0x100)
        } else {
            addr
        };
        let _dummy = self.read1(bus, dummy_addr);
        let cyc_after = bus.cycle_count();
        let stall_after = bus.dma_stall_count();
        let had_dma =
            stall_after.wrapping_sub(stall_before) > 0 || cyc_after.wrapping_sub(cyc_before) > 1;
        #[cfg(feature = "irq-timing-trace")]
        let trace_cpu_after = self.cycles;
        #[cfg(feature = "irq-timing-trace")]
        let trace_idb_after = bus.internal_data_bus();

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

        // SH* trace hook: emit one row per sh_store invocation with
        // both CPU-side and bus-side cycle counters + internal data
        // bus snapshots. Cross-diff R4 vs legacy reveals the exact
        // coherence axis breaking the 3 SH* sub-tests under R4.
        #[cfg(feature = "irq-timing-trace")]
        crate::sh_store_trace::row(
            self.cycles,
            base,
            index_reg,
            dummy_addr,
            trace_cpu_before,
            trace_cpu_after,
            cyc_before,
            cyc_after,
            trace_idb_before,
            trace_idb_after,
            had_dma,
            page_crossed,
            write_value,
        );

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
        // Cross-diff service-event row. Distinguish BRK (RustyNES routes it
        // through `service_interrupt`, but Mesen's `BRK()` is separate from
        // `IRQ()` and emits no service row — so `brk_svc` must be excluded
        // when comparing against the oracle's `irq_svc`/`nmi_svc`).
        // Label by the EFFECTIVE (post-hijack) vector: the A2 unified dispatch
        // always enters with `IRQ_VECTOR`, so an NMI is distinguished by the live
        // `mc_need_nmi` latch (which the cycle-5 hijack consumes → `$FFFA`) rather
        // than the entry `vector`. BRK keeps its own label (Mesen emits no BRK
        // service row, so `brk_svc` is excluded from the nmi/irq cross-diff). Only
        // compiled under the trace combo (cpu-c1 is implied, so `mc_need_nmi` is
        // valid).
        #[cfg(all(
            any(feature = "cpu-c1-attempt-17-access-reorder", feature = "r4-cpu-dma"),
            feature = "irq-timing-trace"
        ))]
        {
            let svc_label = if brk {
                "brk_svc"
            } else if self.mc_need_nmi || vector == NMI_VECTOR {
                "nmi_svc"
            } else {
                "irq_svc"
            };
            self.mc_trace_row(bus, svc_label);
        }
        // Two filler reads for IRQ/NMI; one for BRK (the opcode fetch
        // counted as the other).
        self.idle_tick(bus);
        if !brk {
            self.idle_tick(bus);
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
        // NMI-hijack detection: under the master-clock model an NMI edge
        // latched during cycles 1..=5 of this sequence is recorded in
        // `mc_need_nmi`; the legacy model uses the `nmi_first_tick` latch.
        //
        // A2 (oracle-derived, cpu_interrupts_v2/2): the hijack reads the DELAYED
        // `mc_prev_need_nmi`, NOT the live `mc_need_nmi`. Mesen's BRK/IRQ body
        // reads `_needNmi` as it was at cycle START (before that cycle's φ2 edge
        // detector runs), so an NMI edge that latches ON the P-push cycle does
        // NOT hijack — the BRK completes to its own vector and the NMI is taken
        // after one handler instruction. In RustyNES the φ2 sampler runs inside
        // the P-push (`push`→`write1`→`mc_sample_interrupts`) BEFORE this check,
        // so `mc_need_nmi` would see the just-set value (hijacking one cycle too
        // eagerly — RustyNES hijacked where Mesen vectored to the BRK handler at
        // $E316 then took the NMI). `mc_prev_need_nmi` is that pre-edge value:
        // it is 1 only if the NMI was already pending BEFORE this cycle.
        let nmi_hijack = self.mc_prev_need_nmi;
        let effective_vector = if nmi_hijack && vector != NMI_VECTOR {
            // Consume the latch (the NMI is being serviced by this
            // sequence, not the next instruction).
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
        Operand {
            addr: u16::from(base.wrapping_add(self.x)),
            page_crossed: false,
        }
    }

    fn addr_zp_y<B: Bus>(&mut self, bus: &mut B) -> Operand {
        let base = self.fetch_pc(bus);
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

    fn addr_ind_x<B: Bus>(&mut self, bus: &mut B) -> Operand {
        let base = self.fetch_pc(bus);
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
                self.push(bus, self.a);
                *cycles = 3;
            }
            0x08 => {
                self.push(bus, (self.p | Status::BREAK | Status::UNUSED).bits());
                *cycles = 3;
            }
            0x68 => {
                self.a = self.pull(bus);
                self.p.set_nz(self.a);
                *cycles = 4;
            }
            0x28 => {
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
                let v = self.pull_u16(bus);
                self.pc = v.wrapping_add(1);
                *cycles = 6;
            }
            0x40 => {
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
                // A2: suppress an NMI that became pending during/just-after the
                // BRK sequence so the FIRST instruction of the IRQ handler runs
                // before the NMI is taken (Mesen2 `NesCpu::BRK` NesCpu.cpp:271:
                // `_prevNeedNmi = false`; clean-room — "needed for nmi_and_brk").
                // The hijack path (service_interrupt) already consumed a
                // cycle-5-or-earlier NMI; this covers the late-edge case where
                // `mc_need_nmi` latched after the vector was decided. The NMI is
                // NOT lost — `mc_need_nmi` stays set and re-arms `mc_prev_need_nmi`
                // at the next cycle's edge sampler, firing after one handler op.
                self.mc_prev_need_nmi = false;
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
                let o = self.addr_ind_y(bus);
                self.dcp_addr(bus, o.addr);
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
                let o = self.addr_ind_y(bus);
                self.isc_addr(bus, o.addr);
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
                let o = self.addr_ind_y(bus);
                self.slo_addr(bus, o.addr);
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
                let o = self.addr_ind_y(bus);
                self.rla_addr(bus, o.addr);
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
                let o = self.addr_ind_y(bus);
                self.sre_addr(bus, o.addr);
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
                let o = self.addr_ind_y(bus);
                self.rra_addr(bus, o.addr);
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
        // A2-step-2 (combo): Mesen2 `BranchRelative` (NesCpu.h:435) / TetaNES
        // `branch` (instr.rs:1129): on a TAKEN branch, suppress an IRQ that became
        // visible only THIS cycle (`run_irq && !prev_run_irq`) — the one the
        // "branches poll IRQ before the 2nd cycle but not the 3rd" rule defers to
        // the next instruction. An already-pending IRQ (prev_run_irq set) is left
        // intact. The C3 (+ page-cross C4) dummy reads below then re-sample via
        // `handle_interrupts`, reproducing the page-cross extra poll for free.
        // Replaces the legacy `skip_irq_sample` freeze.
        if self.mc_run_irq && !self.mc_prev_run_irq {
            self.mc_run_irq = false;
        }
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
            // C4 page-cross dummy read at the unfixed address.
            let dummy = (old_pc & 0xFF00) | (new_pc & 0x00FF);
            let _ = self.read1(bus, dummy);
        }
        self.pc = new_pc;
        if crossed {
            4
        } else {
            3
        }
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
