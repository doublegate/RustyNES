//! CPU `Bus` trait.
//!
//! Per `docs/cpu-6502.md` §Interfaces. Phase 1 keeps the surface minimal:
//! address-fanout reads/writes plus interrupt polling. The DMA halt mechanism
//! lands when the APU does (Phase 3), and the cycle-level tick callback is
//! enough to drive `cpu_timing_test` and golden-log compares without the full
//! lockstep scheduler.

use crate::scheduler::M2Phase;

/// Address-space bus seen by the CPU.
///
/// The CPU borrows `&mut Bus` for the duration of an instruction; the bus
/// fans the access out to RAM, PPU registers, APU registers, controllers,
/// and the cartridge's mapper.
pub trait Bus {
    /// Read a byte at `addr`.
    fn cpu_read(&mut self, addr: u16) -> u8;

    /// Write `value` to `addr`.
    fn cpu_write(&mut self, addr: u16, value: u8);

    /// Edge-triggered NMI poll. Returns `true` exactly once per high-to-low
    /// transition of the NMI line; subsequent calls return `false` until the
    /// next transition.
    fn poll_nmi(&mut self) -> bool {
        false
    }

    /// Level-sensitive IRQ. Sampled by the CPU on every instruction's
    /// second-to-last cycle; only honored when the CPU's I flag is clear.
    fn poll_irq(&mut self) -> bool {
        false
    }

    /// Phase-aware level-sensitive IRQ sample.
    ///
    /// Returns the IRQ line as seen at the requested half of the 6502
    /// cycle.  Phase-aware bus implementations override this to expose
    /// the M2-low vs M2-high asymmetry the C1 IRQ-timing rework relies
    /// on (see `docs/adr/0002-irq-timing-coordination.md`); the default
    /// impl simply delegates to [`Bus::poll_irq`], so legacy / test bus
    /// stubs that don't model the phase distinction stay correct without
    /// needing to import [`M2Phase`].
    ///
    /// Phase B3 of the C1 rework: `Cpu::idle_tick` calls
    /// `bus.poll_irq_at_phase(M2Phase::High)` — semantically identical
    /// to the previous `bus.poll_irq()` call because the production
    /// [`crate::Bus`] impl on `LockstepBus` takes its M2-high snapshot
    /// at the same end-of-cycle point the historical `poll_irq` query
    /// fired from.
    fn poll_irq_at_phase(&mut self, phase: M2Phase) -> bool {
        let _ = phase;
        self.poll_irq()
    }

    /// Called once per CPU cycle consumed. Used by the scheduler to advance
    /// the PPU/APU in lockstep (Phase 2+) and by the test harness to count
    /// cycles for golden-log compare.
    fn on_cpu_cycle(&mut self) {}

    /// φ1 (pre-access) half of one CPU cycle, for the C1 access-reorder
    /// axis attempt 17.  Called BEFORE the bus access in
    /// `Cpu::read1` / `Cpu::write1` when the
    /// `cpu-c1-attempt-17-access-reorder` feature is enabled.
    ///
    /// On the production [`crate::Bus`] (`LockstepBus`), this ticks
    /// PPU sub-dot 0 (1 PPU dot) and captures the M2-low IRQ
    /// snapshot.  Default impl is a no-op so legacy / test buses
    /// don't accidentally advance state when paired with the φ2
    /// default (which calls [`Bus::on_cpu_cycle`] to do all the
    /// work).
    fn cpu_cycle_phi1(&mut self) {}

    /// φ2 (post-access) half of one CPU cycle.  Called AFTER the
    /// bus access in `Cpu::read1` / `Cpu::write1`
    /// when the `cpu-c1-attempt-17-access-reorder` feature is
    /// enabled.
    ///
    /// On the production `LockstepBus`, this ticks PPU sub-dots 1+2
    /// (2 PPU dots), increments the bus-side cycle counter, fires
    /// `notify_cpu_cycle` + `tick_with_external`, and captures the
    /// M2-high IRQ snapshot.
    ///
    /// The default impl delegates to [`Bus::on_cpu_cycle`] so
    /// legacy / test buses keep their current behaviour: φ1 is a
    /// no-op, φ2 does all the work, same total per-cycle work as a
    /// single `on_cpu_cycle` call.
    fn cpu_cycle_phi2(&mut self) {
        self.on_cpu_cycle();
    }

    /// Notify the bus that the CPU is about to perform an interrupt
    /// vector fetch from `vector` (`$FFFE` for IRQ/BRK, `$FFFA` for NMI,
    /// or `$FFFA` if an IRQ/BRK service sequence was hijacked by an NMI
    /// edge during cycles 1..=5 of the service sequence).  `is_nmi` is
    /// `true` for an NMI service entry and `false` for an IRQ or BRK
    /// service entry (so the bus can distinguish hijack from a clean
    /// NMI even when the vector is the same).
    ///
    /// Default impl is a no-op; production buses with the
    /// `irq-timing-trace` feature override this to emit a
    /// [`ServiceEvent`] into the IRQ trace fixture.  Phase 1.2 of
    /// Track C1 attempt 14 added this method to close the schema gap
    /// with Mesen2's `emu.eventType.irq` / `emu.eventType.nmi` oracle.
    ///
    /// [`ServiceEvent`]: # "see rustynes_core::irq_trace::ServiceEvent"
    fn notify_irq_service(&mut self, vector: u16, is_nmi: bool) {
        let _ = vector;
        let _ = is_nmi;
    }

    /// Cumulative bus-side cycle counter.
    ///
    /// On the production `LockstepBus`, this is `self.cycle` —
    /// the total number of CPU cycles the bus has ticked, INCLUDING
    /// DMC DMA halt + dummy + alignment + transfer cycles (which
    /// the CPU's own `Cpu::cycles` field does NOT count because
    /// they advance through `bus.tick_one_cpu_cycle()` rather than
    /// the CPU's `idle_tick`).
    ///
    /// Used by the SH* unstable-store family (`SHA / SHX / SHY /
    /// SHS / TAS`) to detect when DMC DMA interrupted the
    /// instruction's dummy-read cycle: per Mesen2 `NesCpu.h`
    /// `SyaSxaAxa` (lines 716-745), if the dummy read consumed
    /// more than 1 bus cycle, a DMA fired, and the value written
    /// is `valueReg` un-ANDed with the H+1 byte (the DMA pulled
    /// the bus low / corrupted the latch).  Mesen2 detects this
    /// via `_state.CycleCount - cyc > 1` after the dummy read;
    /// we mirror via `bus.cycle_count() - before > 1`.
    ///
    /// Default impl returns `0` for legacy / test bus stubs.
    fn cycle_count(&self) -> u64 {
        0
    }

    /// Most recent value driven onto the **internal** CPU data bus.
    ///
    /// The 2A03 silicon has two distinct data buses: the **internal**
    /// data bus carries CPU instruction fetches, operand reads, ALU
    /// results, and writes; the **external** data bus is shared with
    /// the DMC DMA fetch path and is observable via the open-bus
    /// latch.  The two buses are equal on every cycle where the CPU
    /// drives the bus, but diverge during DMC DMA halt: the DMC
    /// fetch drives the external bus (the "open bus") while the CPU
    /// is halted and the internal bus retains its prior value.
    ///
    /// Default impl returns `0` for legacy / test bus stubs that do
    /// not model the distinction.  The production `LockstepBus`
    /// overrides this to expose the latched internal value (mirrored
    /// from every CPU read but NOT updated by DMC DMA fetches).
    ///
    /// Used by the SH* unstable-store family (`SHA / SHX / SHY / SHS
    /// / TAS`, opcodes `$93 / $9C / $9E / $9F / $9B`) when computing
    /// the address-high-byte AND-and-write quantity under DMC DMA
    /// interleaving, and by the `$4015` read path for the bit-5
    /// open-bus exposure that `CPU Behavior :: Open Bus` Test 9
    /// brackets.  Phase 1 of the v1.0.0-final
    /// `linked-puzzling-sutherland` brief (see
    /// `to-dos/phase-6-v1.0.0-final/sprint-6-sh-unstable-stores.md`).
    ///
    fn internal_data_bus(&self) -> u8 {
        0
    }

    // ================================================================
    // v2.0 master-clock R1 substrate — clean Bus contract (Phase 1).
    //
    // These methods exist ALONGSIDE the legacy lockstep methods above and
    // are only consulted by the `mc-r1-substrate` CPU loop (Phases 2+). They
    // carry default impls delegating to the legacy surface so every existing
    // `Bus` impl (test stubs included) keeps compiling unchanged; the
    // production `LockstepBus` overrides them with the real master-clock
    // catch-up. Gated so the default build's trait surface is unchanged.
    // See `docs/audit/v2.0-master-clock-r1-port-plan-2026-06-03.md`.
    // ================================================================

    /// Pure address-space read (no per-cycle work). Under R1 the cycle work
    /// is done by [`Bus::run_ppu_to`] + [`Bus::cpu_clock`], which the CPU
    /// calls around the access. Default delegates to [`Bus::cpu_read`].
    fn read(&mut self, addr: u16) -> u8 {
        self.cpu_read(addr)
    }

    /// Pure address-space write. Default delegates to [`Bus::cpu_write`].
    fn write(&mut self, addr: u16, value: u8) {
        self.cpu_write(addr, value);
    }

    /// Master clocks per CPU cycle for the cartridge region: NTSC 12, PAL 16,
    /// Dendy 15 (the master-clock unit is shared with [`Bus::run_ppu_to`]'s
    /// `ppu_divider`, so per CPU cycle the PPU advances `cpu_divider /
    /// ppu_divider` dots — 3:1 NTSC, 3.2:1 PAL, 3:1 Dendy). The R1 CPU loop
    /// advances `master_clock` and derives its read/write split off this. The
    /// default (12) keeps test stubs + the non-regioned path on NTSC; the
    /// `LockstepBus` overrides from the cartridge region.
    fn cpu_divider(&self) -> u64 {
        12
    }

    /// Catch the PPU up to `target` master clocks (Mesen `NesPpu::Run` /
    /// `TetaNES` `clock_to`). Ticks whole PPU dots while
    /// `ppu_clock + ppu_divider <= target`. Called by the R1 CPU loop in
    /// BOTH halves of each access (the double catch-up). Default no-op.
    ///
    /// `is_post_access` distinguishes WHICH half of the CPU cycle this
    /// catch-up belongs to: `false` for the pre-access half (called from
    /// `Cpu::start_cycle`, before the bus access — mirrors Mesen's
    /// `StartCpuCycle`), `true` for the post-access half (called from
    /// `Cpu::end_cycle`, after the bus access — mirrors `EndCpuCycle`).
    /// R1c-3 (`mmc3-m2-phase-irq`, default-off experiment): `LockstepBus`
    /// forwards this as the real M2-phase label on the `PpuBusAdapter` it
    /// constructs, replacing the previously call-local (and therefore
    /// almost-always-zero) `sub_dot` counter with a value that actually
    /// distinguishes the pre-access (M2-low, φ1) and post-access
    /// (M2-high, φ2) halves for any A12 transition ticked during this
    /// catch-up. See `docs/adr/0002-irq-timing-coordination.md` and
    /// `docs/audit/r1r2-per-dot-scheduler-attempt-2026-07-02.md`.
    fn run_ppu_to(&mut self, target: u64, is_post_access: bool) {
        let _ = (target, is_post_access);
    }

    /// One CPU cycle of bus-side work (Mesen `ProcessCpuClock`): APU +
    /// frame counter + per-cycle mapper hook + bus-side DMA drain + cycle
    /// counter. The PPU advance is in [`Bus::run_ppu_to`], not here. Default
    /// delegates to [`Bus::on_cpu_cycle`] (legacy combined per-cycle work).
    fn cpu_clock(&mut self) {
        self.on_cpu_cycle();
    }

    /// F-2: tick ONLY the DMC byte-timer + DMA arm, at END of cycle (called
    /// from `Cpu::end_cycle` after the access + PPU catch-up). This places the
    /// DMC fire-phase at main's end-of-cycle position (so `DMASync`'s `$4000`
    /// open-bus conflict lands), while the rest of the APU (incl. the IRQ line)
    /// stays on the cycle-start `cpu_clock` tick (so the C1 φ2 IRQ sample is
    /// unchanged). Default no-op. Pairs with `Apu::set_dmc_driven_externally`.
    fn cpu_clock_apu_dmc(&mut self) {}

    /// Master clocks consumed by bus-side DMA cycles since the last call,
    /// then reset to 0. The R1 CPU loop folds this into `master_clock` in
    /// `end_cycle` so the CPU<->PPU phase stays coherent across a bus-side
    /// DMA span. Default 0 (no bus-side DMA accounting on test stubs).
    fn take_dma_mc_consumed(&mut self) -> u64 {
        0
    }

    /// Live IRQ line level (mapper IRQ OR APU frame-counter/DMC IRQ). The
    /// CPU does the I-flag mask + one-cycle `prev_run_irq` delay itself.
    /// Default `false`; the production bus overrides this.
    fn irq_level(&self) -> bool {
        false
    }

    /// Live /NMI line level (PPU-driven). The CPU does its own edge detect +
    /// one-cycle `prev_need_nmi` delay. Default `false` (test stubs).
    fn nmi_level(&self) -> bool {
        false
    }

    /// Phase B (interleaved DMC DMA): is a DMC DMA pending and needing cycles?
    /// The CPU loops on this in `read1`, running one `dmc_dma_step` per R1 cycle
    /// BEFORE its own read (DMA halts only on read cycles). Default `false`.
    fn dmc_dma_pending(&self) -> bool {
        false
    }

    /// `mc-r1-dmc-load-get-entry`: defer a LOAD whose first-service would be a PUT
    /// cycle by 1 CPU cycle so it enters on a GET (span-3 hardware load). Gates BOTH
    /// the read1 loop AND the `idle_tick` loop (`DMASync`'s load fires during NOPs=idle).
    fn dmc_dma_defer_load_entry(&self) -> bool {
        false
    }

    /// Phase B: perform ONE cycle's worth of interleaved DMC DMA bus access
    /// (halt re-read / sample get), advancing the halt/get state. `halted_addr`
    /// is the CPU read the DMA is preempting. Default no-op.
    fn dmc_dma_step(&mut self, halted_addr: u16) {
        let _ = halted_addr;
    }

    /// `mc-r1-dmc-idle-halt`: perform one interleaved DMC-DMA cycle during a CPU
    /// INTERNAL cycle (no instruction read). The bus supplies the held address
    /// (its last-read bus address) since `idle_tick` has none. Default no-op.
    fn dmc_dma_step_idle(&mut self) {}

    /// Stage-D (`mc-r1-full-cpu`): is an OAM DMA pending or in flight? The CPU
    /// loops on this in `read1` (after the DMC loop, DMC-get-before-OAM-get), so
    /// each OAM cycle runs CPU-driven (wrapped `start_cycle`/`end_cycle`) and
    /// samples IRQ/NMI via the φ2 pipeline — the surface the bus-burst bypassed.
    /// Default `false`.
    fn oam_dma_pending(&self) -> bool {
        false
    }

    /// Stage-D: perform ONE cycle of the OAM DMA (set-up on first call from a
    /// pending `$4014`, then halt/align/read/write per cycle). Does NOT advance
    /// time — the surrounding `start_cycle`/`end_cycle` do. Default no-op.
    fn oam_dma_step(&mut self, halted_addr: u16) {
        let _ = halted_addr;
    }

    /// Program M (M-2, `mc-r1-dmc-oam-overlap`): is an OAM DMA actually IN FLIGHT
    /// (started, cycles still owed) — distinct from `oam_dma_pending`, which is
    /// true for a not-yet-started `$4014` write too. The overlap loop uses this
    /// to decide whether a DMC halt cycle can SHARE an OAM cycle. Default `false`.
    fn oam_dma_in_flight(&self) -> bool {
        false
    }

    /// W3-Stage-0 (`mc-r1-counter-collapse` boundary realign): may a pending DMC
    /// DMA join an OAM DMA as an OVERLAP event? Default delegates to
    /// [`Bus::oam_dma_in_flight`]. Under the counter-collapse flag the bus also
    /// answers `true` for a `$4014` write that is PENDING but not yet started:
    /// the end-of-cycle byte-timer shift can surface the DMC arm in the gap
    /// between the `$4014` write and OAM's first cycle, and routing that arm to
    /// the standalone `dmc_dma_step` (full unshared span) instead of the overlap
    /// event is exactly the DMC+OAM idx\[7\] regime-transition error (lockstep
    /// latches OAM in `drain_dma` BEFORE its DMC-pending check, so the same arm
    /// overlaps OAM's halt/alignment cycles there).
    fn oam_dma_overlap_ready(&self) -> bool {
        self.oam_dma_in_flight()
    }

    /// Program M (M-2): did the most recent [`Bus::dmc_dma_step`] perform the DMC
    /// GET (the sample fetch) rather than a halt/dummy/align cycle? The overlap
    /// loop advances OAM on non-GET (halt) cycles only — the GET steals an OAM
    /// slot. Default `false`.
    fn dmc_dma_last_was_get(&self) -> bool {
        false
    }

    /// Program M (M-2): advance ONE OAM DMA cycle that is SHARED with a DMC halt
    /// cycle (the 6502 is RDY-halted by the DMC, but the OAM engine keeps
    /// consuming its read/write slot on the external bus). Does NOT advance time
    /// — the surrounding `start_cycle`/`end_cycle` do. Default no-op.
    fn oam_dma_overlap_cycle(&mut self) {}

    /// Program M (M-2, exact): begin ONE DMC-DMA-during-OAM event, mirroring the
    /// lockstep `service_dmc_dma_during_oam` prologue. Latches the DMA span + the
    /// open-bus replay and returns the UNCONDITIONAL halt/dummy/align noop count
    /// (2 for a short/load DMA, 3 for a reload) — NOT parity-gated. The CPU then
    /// runs exactly that many [`Bus::dmc_overlap_noop_cycle`]s, one
    /// [`Bus::dmc_overlap_get_cycle`], and (if OAM still owes) one
    /// [`Bus::dmc_overlap_realign_cycle`]. `halted_addr` is the CPU read the DMA
    /// pair is preempting — used as the OAM halt address when the event starts a
    /// PENDING (not-yet-latched) `$4014` OAM DMA (the counter-collapse boundary
    /// case; an already-in-flight OAM keeps its own latched halt address).
    /// Default `0` (no DMC event).
    fn dmc_overlap_begin(&mut self, halted_addr: u16) -> u32 {
        let _ = halted_addr;
        0
    }

    /// Program M (M-2, exact): one DMC halt/dummy/align cycle that OVERLAPS OAM.
    /// Replays the held CPU read's side-effect, then (if OAM still owes) advances
    /// one OAM slot. Mirrors lockstep's noop-loop body (`replay_dma_noop_read` +
    /// `clock_oam_dma_cycle`) minus the time tick. Default no-op.
    fn dmc_overlap_noop_cycle(&mut self) {}

    /// Program M (M-2, exact): the DMC GET cycle — owns the memory read; OAM is
    /// STALLED (does NOT advance). Fetches + delivers the sample and clears the
    /// DMC-DMA pending state. Mirrors lockstep's get block + the R1
    /// `dmc_dma_step` GET. Default no-op.
    fn dmc_overlap_get_cycle(&mut self) {}

    /// Program M (M-2, exact): the post-GET realign stall — ONE extra OAM-stalled
    /// cycle (OAM does NOT advance) so the next OAM read resumes on a later get,
    /// mirroring lockstep's `if dma_cycles_owed > 0 { tick }`. The cycle the prior
    /// per-cycle scaffold was MISSING. Default no-op.
    fn dmc_overlap_realign_cycle(&mut self) {}

    /// W3-Stage-1 (`mc-r1-dma-unified`): is ANY DMA work pending for the
    /// unified DMC/OAM engine — a serviceable DMC DMA (pending and not a
    /// load deferred to its get-cycle entry, the `mc-r1-dmc-load-get-entry`
    /// rule), a `$4014` OAM DMA awaiting its first cycle, or an OAM transfer
    /// still in flight? The ONE `Cpu::read1`/`idle_tick` DMA loop spins on
    /// this, running one [`Bus::unified_dma_cycle`] per CPU cycle (each a
    /// full R1 cycle: `start_cycle` -> dispatch -> `end_cycle`, so every DMA
    /// cycle keeps the φ2 IRQ sample — the C1-safe shape). Default `false`.
    fn unified_dma_pending(&self) -> bool {
        false
    }

    /// W3-Stage-1 (`mc-r1-dma-unified`): ONE cycle of the unified DMC/OAM DMA
    /// engine — a direct port of the `TriCNES` `_6502` per-cycle DMA dispatch
    /// table (the SINGLE driver standalone DMC, standalone OAM, and the
    /// overlap all ride), at FLOOR parity for this stage. `halted_addr` is
    /// the CPU read the DMA is preempting (the parked 6502 address bus).
    /// Does NOT advance time — the surrounding `start_cycle`/`end_cycle` do.
    /// Default no-op.
    fn unified_dma_cycle(&mut self, halted_addr: u16) {
        let _ = halted_addr;
    }

    /// W3-Stage-1 (`mc-r1-dma-unified`): one unified-engine DMA cycle during
    /// a CPU INTERNAL cycle (no instruction read; the bus supplies its held
    /// last-read address). The unified replacement for
    /// [`Bus::dmc_dma_step_idle`]. Default no-op.
    fn unified_dma_cycle_idle(&mut self) {}

    /// accuracycoin-100 Phase 2 (`mc-r1-dmc-abort-cancel`): is a 1-byte
    /// non-looping implicit DMC-DMA abort matured and awaiting service? The CPU
    /// consults this at the top of `read1`/`write1`. Default `false`.
    fn dmc_abort_pending(&self) -> bool {
        false
    }

    /// accuracycoin-100 Phase 2: is the upcoming cycle a GET (read) cycle for
    /// the DMC DMA (`!put_cycle`)? On a get cycle the matured abort runs as a
    /// 1-cycle DMA (Y=1); on a put cycle (or any CPU write) it does NOT occur
    /// (Y=0, "the abort will not land on a write cycle"). Default `false`.
    fn dmc_abort_is_get_cycle(&self) -> bool {
        false
    }

    /// accuracycoin-100 Phase 2: service the matured abort as a 1-cycle DMA
    /// (Y=1) — one halt re-read of `halted_addr`, then clear the abort + the
    /// pending reload. Called by `read1` only on a get cycle. Default no-op.
    fn dmc_abort_halt_step(&mut self, halted_addr: u16) {
        let _ = halted_addr;
    }

    /// accuracycoin-100 Phase 2: cancel the matured abort with NO halt cycle
    /// (Y=0) — the abort lands on a write/put cycle so the DMA does not occur.
    /// Clears the abort + the pending reload. Default no-op.
    fn dmc_abort_cancel(&mut self) {}

    /// Diagnostic-only hook fired once per R1 CPU cycle from `Cpu::end_cycle`
    /// (after `handle_interrupts`), so the `irq-timing-trace` tooling can
    /// record a `CycleRecord` for the R1 access path (which bypasses the
    /// `LockstepBus` `tick_one_cpu_cycle` push). Default no-op; the production
    /// bus overrides it only under the `irq-timing-trace` feature, so non-trace
    /// R1 builds compile this to an empty call.
    fn trace_end_cycle(&mut self) {}

    /// Diagnostic-only hook fired once per CPU INSTRUCTION from `Cpu::step`
    /// (at the opcode fetch), with the instruction's `pc` and the cumulative
    /// CPU cycle count. Lets the `cpu-instr-cycle-trace` tooling diff R1 vs
    /// default per-instruction to pin the cumulative cycle-count divergence
    /// (the R1c-1 odd-cycle source). Default no-op.
    #[cfg(feature = "cpu-instr-cycle-trace")]
    fn trace_instr(&mut self, _pc: u16, _cpu_cycle: u64) {}
}
