//! CPU `Bus` trait.
//!
//! Per `docs/cpu-6502.md` §Interfaces. The trait is the whole surface the
//! 6502 core sees: address-fanout reads/writes, the per-cycle hooks the
//! one-clock scheduler calls in each half of a CPU cycle (`run_ppu_to`,
//! `cpu_clock`, `cpu_clock_apu_dmc`), the live /IRQ and /NMI line levels the
//! CPU edge-detects itself, and the unified DMC/OAM DMA engine's per-cycle
//! entry points. Every method other than `cpu_read` / `cpu_write` has a
//! default, so a test bus implements only what it models.
//!
//! v2.9.8 removed the 18 methods deprecated at v2.7.5 (ADR 0042): the
//! `poll_nmi` / `poll_irq` family, the pre-v2.0.0 per-phase hooks
//! (`cpu_cycle_phi1` / `cpu_cycle_phi2`), `internal_data_bus`, and the
//! per-engine DMC / OAM / overlap DMA hooks the unified engine replaced. None
//! had a caller after the v2.0.0 one-clock scheduler.

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

    /// Called once per CPU cycle consumed. Used by the scheduler to advance
    /// the PPU/APU in lockstep (Phase 2+) and by the test harness to count
    /// cycles for golden-log compare.
    fn on_cpu_cycle(&mut self) {}

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
    /// On the production `SystemBus`, this is `self.cycle` —
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

    // ================================================================
    // The one-clock scheduler's contract (ADR 0002 / ADR 0029).
    //
    // `Cpu::start_cycle` / `Cpu::end_cycle` call these around every access.
    // The defaults delegate to `cpu_read` / `cpu_write` / `on_cpu_cycle`, so
    // a simple test bus keeps working without modelling the split; the
    // production `SystemBus` overrides them with the real master-clock
    // catch-up. History: `docs/audit/v2.0-master-clock-r1-port-plan-2026-06-03.md`.
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
    /// `SystemBus` overrides from the cartridge region.
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
    /// R1c-3 (`mmc3-m2-phase-irq`, default-off experiment): `SystemBus`
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

    /// `mc-r1-dmc-load-get-entry`: defer a LOAD whose first-service would be a PUT
    /// cycle by 1 CPU cycle so it enters on a GET (span-3 hardware load). Gates BOTH
    /// the read1 loop AND the `idle_tick` loop (`DMASync`'s load fires during NOPs=idle).
    fn dmc_dma_defer_load_entry(&self) -> bool {
        false
    }

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
    /// last-read address). Default no-op.
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
    /// `SystemBus` `tick_one_cpu_cycle` push). Default no-op; the production
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
