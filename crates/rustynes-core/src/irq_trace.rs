//! Per-CPU-cycle IRQ-timing tracing fixture (Track C1 pre-work).
//!
//! Records, for every CPU cycle, the tuple `(cpu_cycle, ppu_scanline,
//! ppu_dot, irq_pending_{mapper,apu}_at_low,
//! irq_pending_{mapper,apu}_at_high, nmi_line, a12_events)`.  Two IRQ
//! snapshots are taken per CPU cycle — once at the conventional M2-low
//! boundary (after PPU sub-dot 0) and once at the conventional M2-high
//! boundary (after PPU sub-dot 2, the end of the cycle).  Designed as
//! the empirical oracle the four rolled-back IRQ-timing attempts could
//! not be evaluated against (the M2 phase was implicit in each); Phase A
//! of the C1 plan makes the asymmetry observable.
//!
//! See `docs/adr/0002-irq-timing-coordination.md` "Decision (revised,
//! 2026-05-13)" → "Test fixture" for the design.
//!
//! # Feature gating
//!
//! The fixture is gated on the `irq-timing-trace` cargo feature (off by
//! default).  When the feature is disabled the public-facing API still
//! exists as no-op stubs, so call sites in `LockstepBus` can stay
//! unconditional without `#[cfg(...)]` clutter.
//!
//! # Usage
//!
//! ```ignore
//! # use rustynes_core::Nes;
//! let mut nes = Nes::from_rom(&rom_bytes)?;
//! nes.bus_mut().enable_irq_trace(/* capacity */ 32_768);
//! for _ in 0..600 { nes.run_frame(); }
//! let trace = nes.bus_mut().take_irq_trace().unwrap();
//! std::fs::write("trace.csv", trace.to_csv()).unwrap();
//! # Ok::<(), rustynes_mappers::RomError>(())
//! ```

#![allow(dead_code)] // Most surfaces are only used when the feature is on.

use alloc::string::String;
use alloc::vec::Vec;

// The canonical `M2Phase` reference enum lives in `crate::scheduler` and
// is re-exported from the crate root unconditionally; the fixture keeps
// using it via a local import so the rest of this module reads the same
// as before Phase B1.
pub use crate::scheduler::M2Phase;

/// One A12 transition observed during the 3 PPU-dot tick window of a
/// single CPU cycle.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct A12Event {
    /// Which of the 3 PPU dots inside the CPU cycle the transition was
    /// observed at (0, 1, or 2).
    pub sub_dot: u8,
    /// New A12 level (`true` = rising, `false` = falling).
    pub level: bool,
}

/// Vector-service event type (Phase 1.2 of Track C1 attempt 14).
///
/// Mirrors Mesen2's `emu.eventType.irq` (= service IRQ vector fetch at
/// `$FFFE/$FFFF`) and `emu.eventType.nmi` (= NMI vector fetch at
/// `$FFFA/$FFFB`) so the `RustyNES` trace can be cross-diffed against
/// Mesen2's Lua oracle on the SAME axis (service-cycle, not IRQ-line-
/// transition cycle).  The pre-Phase-1.2 trace was state-transition-
/// driven (one row per IRQ-line edge); Mesen2 is event-driven (one row
/// per CPU vector fetch).  The two schemas were not directly comparable
/// (Session-15 confound 3).  This event type closes that gap.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ServiceKind {
    /// CPU serviced an IRQ — vector fetch from `$FFFE/$FFFF`.
    Irq,
    /// CPU serviced an NMI — vector fetch from `$FFFA/$FFFB`.
    Nmi,
}

impl ServiceKind {
    /// Stable text representation used by the CSV writer; mirrors
    /// Mesen2's `event_type` column values.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Irq => "irq_svc",
            Self::Nmi => "nmi_svc",
        }
    }
}

/// One CPU interrupt-vector-fetch event captured during the trace.
///
/// Emitted from `Cpu::service_interrupt` via the new
/// `Bus::notify_irq_service` trait method right before the CPU reads the
/// vector low byte.  Records the cycle / PPU position / actual vector
/// fetched (which can differ from the requested vector under NMI-hijack
/// semantics — see `Cpu::service_interrupt`).
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct ServiceEvent {
    /// Cumulative CPU cycle counter at the moment of the vector fetch.
    pub cpu_cycle: u64,
    /// PPU scanline at the moment of the vector fetch.
    pub ppu_scanline: i16,
    /// PPU dot at the moment of the vector fetch.
    pub ppu_dot: u16,
    /// PPU frame at the moment of the vector fetch.
    pub ppu_frame: u64,
    /// Whether IRQ or NMI was serviced.
    pub kind: ServiceKind,
    /// Effective vector low byte address actually read.  `$FFFE` for IRQ
    /// (or BRK), `$FFFA` for NMI, but if NMI hijacked an IRQ/BRK service
    /// sequence this will be `$FFFA` even though `kind` reports the
    /// requestor's original kind.
    pub vector: u16,
}

/// Type of bus access performed during a CPU cycle.
///
/// Added in Session-21 (Sprint 1 iteration 2 prereq) as the per-cycle
/// bus-access dimension required to diagnose the DMC DMA scheduler
/// calibration mismatch that Sprint 1 iteration 1's coordinated implied-
/// dummy-read attempt regressed on (Session-20 + Session-19 cascade).
///
/// `Idle` is the canonical 6502 "internal" cycle (T2 of `RTS`,
/// implied/accumulator opcode burn cycles, etc.) — bus quiet but the
/// open-bus latch retains its prior driver.  `Read` / `Write` are the
/// 6502's normal external bus cycles.  `DmaRead` / `DmaWrite` are the
/// halted-CPU DMA fetches the bus performs while the CPU is stalled.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum BusAccess {
    /// No external bus access this cycle (internal / idle / halted-CPU
    /// pre-DMA cycle with no in-flight DMA).
    Idle,
    /// Normal CPU read.
    Read,
    /// Normal CPU write.
    Write,
    /// DMA read (OAM DMA, DMC DMA dummy/get, or halt-replay read of the
    /// prior CPU address per the 2A03 DMC register-conflict path).
    DmaRead,
    /// DMA write (OAM DMA `$2004` transfer half).
    DmaWrite,
}

impl BusAccess {
    /// Stable single-letter text representation used by the CSV writer.
    /// `I`=Idle, `R`=Read, `W`=Write, `r`=`DmaRead`, `w`=`DmaWrite`.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "I",
            Self::Read => "R",
            Self::Write => "W",
            Self::DmaRead => "r",
            Self::DmaWrite => "w",
        }
    }
}

/// One per-CPU-cycle trace record.
///
/// IRQ state is sampled TWICE per CPU cycle: once at the conventional
/// M2-low boundary (after PPU sub-dot 0 ticks) and once at the
/// conventional M2-high boundary (after PPU sub-dot 2 ticks, i.e. the
/// end of the cycle).  The asymmetry between `_at_low` and `_at_high`
/// is the empirical signal Phase A of Track C1 exposes for the
/// coordinated CPU/Bus/PPU IRQ-timing rework.
///
/// As of Session-21 (Sprint 1 iteration 2 prereq) the record also
/// captures per-cycle DMC DMA scheduler state and the bus access type
/// performed during the cycle.  These columns are the empirical signal
/// the Implied-Dummy-Read + DMC coordinated fix needs — Sprint 1
/// iteration 1 rolled back twice because `RustyNES`'s DMC scheduler
/// compensating delays (`dmc_abort_delay`, `dmc_dma_cooldown`,
/// `dmc_dma_short`, `pending_dmc_dma`) were calibrated to a bus-quiet
/// implied opcode T2 baseline; adding the canonical implied dummy read
/// without re-calibrating the scheduler cascaded the `Implicit DMA
/// Abort` test from strict-pass to error 2.  The new columns make the
/// scheduler's per-cycle behavior diff-able against Mesen2's
/// `NesDmc.cpp` (the reference implementation).
#[derive(Clone, Debug)]
// 5 IRQ-line bool fields (mapper/apu × low/high, plus NMI) are the
// fixture's reason for existing — bundling them into a sub-struct would
// just obscure the per-record schema downstream tooling reads from CSV.
#[allow(clippy::struct_excessive_bools)]
pub struct CycleRecord {
    /// Cumulative CPU cycle counter (matches `LockstepBus::cycle()`).
    pub cpu_cycle: u64,
    /// PC of the CPU instruction currently executing (the most recent
    /// `Cpu::step` opcode-fetch PC). Captured via the `trace_instr` hook
    /// (`cpu-instr-cycle-trace` feature); stays at the halted instruction's
    /// PC across DMA-insertion cycles. `0` when `cpu-instr-cycle-trace` is
    /// not enabled. Used by the `TriCNES` per-cycle cross-diff to name the
    /// instruction at each divergence.
    pub pc: u16,
    /// PPU scanline AT THE START of the CPU cycle.
    pub ppu_scanline: i16,
    /// PPU dot AT THE START of the CPU cycle.
    pub ppu_dot: u16,
    /// PPU frame counter AT THE START of the CPU cycle.
    pub ppu_frame: u64,
    /// Mapper-asserted IRQ state at the M2-low snapshot (after sub-dot 0).
    pub irq_pending_mapper_at_low: bool,
    /// APU-asserted IRQ state at the M2-low snapshot (after sub-dot 0).
    pub irq_pending_apu_at_low: bool,
    /// Mapper-asserted IRQ state at the M2-high snapshot (after sub-dot 2).
    pub irq_pending_mapper_at_high: bool,
    /// APU-asserted IRQ state at the M2-high snapshot (after sub-dot 2).
    pub irq_pending_apu_at_high: bool,
    /// PPU NMI line state at the end of the CPU cycle.
    pub nmi_line: bool,
    /// A12 transitions observed during the CPU cycle's 3 PPU dots.
    pub a12_events: Vec<A12Event>,
    // --- Session-21 DMC scheduler + bus-access extension ---
    /// `Apu::pending_dmc_dma` snapshotted at the start of the cycle (M2-
    /// low boundary, BEFORE `tick_with_external` advances the APU).
    pub dmc_dma_pending_pre: bool,
    /// `Apu::pending_dmc_dma` snapshotted at the end of the cycle (after
    /// `tick_with_external` runs).  Differs from `_pre` when the APU's
    /// per-cycle DMC timer rolls over the sample-buffer-empty edge or
    /// when a `dmc_dma_delay` countdown reaches 0.
    pub dmc_dma_pending_post: bool,
    /// `Apu::dmc_dma_short` snapshotted at the end of the cycle.  When
    /// true, the bus services the pending DMA via the 3-cycle short path
    /// (halt + dummy + get) instead of the 4-cycle long path.
    pub dmc_dma_short_post: bool,
    /// `Apu::dmc_abort_pending` snapshotted at the end of the cycle.
    pub dmc_abort_pending_post: bool,
    /// `Apu::dmc_abort_delay` countdown at the end of the cycle.  Counts
    /// CPU cycles until `pending_dmc_abort` flips to `true`.
    pub dmc_abort_delay_post: u8,
    /// `Apu::dmc_dma_cooldown` countdown at the end of the cycle.  Counts
    /// CPU cycles during which a newly-empty DMC sample buffer must NOT
    /// raise a new DMA request (suppresses the "next reload race").
    pub dmc_dma_cooldown_post: u8,
    /// `Apu::dmc_dma_delay` countdown at the end of the cycle.  Counts
    /// CPU cycles until an initial-load DMA after `$4015` enable
    /// transitions from "armed" to `pending_dmc_dma = true`.
    pub dmc_dma_delay_post: u8,
    /// `Apu::apu_phase` snapshotted at the end of the cycle.  False = put
    /// phase (cycle just ticked was a put), true = get phase.  Mesen2
    /// calls this `_state.PutCycle` (inverted).
    pub apu_phase_post: bool,
    /// `Bus::in_dmc_dma` snapshotted at the end of the cycle.  True
    /// during the 3-4 CPU cycles the bus is halted servicing a DMC DMA
    /// fetch.
    pub in_dmc_dma: bool,
    /// `Bus::dma_cycles_owed` snapshotted at the end of the cycle (OAM
    /// DMA remaining cycles, 0 = idle).
    pub dma_cycles_owed: u32,
    /// Bus access type performed during this CPU cycle.  See
    /// [`BusAccess`].
    pub bus_access: BusAccess,
    /// Bus address driven during this cycle.  Meaningless when
    /// `bus_access == BusAccess::Idle`.  For DMA cycles this is the
    /// DMA-driven address (the halted CPU's last-read address is
    /// captured in `dma_halt_addr` but we don't surface it separately;
    /// the test fixture can reconstruct it from prior cycles).
    pub bus_addr: u16,
    /// Bus data byte driven during this cycle.  Meaningless when
    /// `bus_access == BusAccess::Idle`.  For reads this is the byte read
    /// FROM `bus_addr`; for writes this is the byte written TO
    /// `bus_addr`.
    pub bus_data: u8,
    /// `Apu::put_cycle` (`TriCNES` `APU_PutCycle`) snapshotted at the end of
    /// the cycle. This is the interleaved-DMA get/put flip-flop the R1
    /// `dmc_dma_step` GET is gated on (`get = !put_cycle`); distinct from
    /// `apu_phase_post`. Only toggled under `dmc_driven_externally` (R1), so
    /// it is `false` on the default lockstep path.
    pub put_cycle_post: bool,
    /// DMC internal byte-timer countdown at end-of-cycle. Exposes the
    /// (bus-invisible) byte-timer phase that drives `clock_output` and thus the
    /// abort-context reload-arm timing (the +4-cycle `A->B` divergence source).
    pub dmc_timer_post: u16,
    /// DMC output-unit bits-remaining at end-of-cycle.
    pub dmc_bits_remaining_post: u8,
    /// DMC output-unit silence flag at end-of-cycle.
    pub dmc_silence_post: bool,
    /// DMC sample-buffer-occupied flag at end-of-cycle.
    pub dmc_buffer_full_post: bool,
}

/// Per-CPU-cycle IRQ trace.
///
/// Linear buffer bounded at `capacity`: records past the cap are
/// silently dropped (`overflow` counter advances).  The Track C1
/// fixture sets `capacity` large enough (a few million records, ~17
/// NTSC frames) to comfortably cover each target ROM's
/// IRQ-measurement window before the cap is hit.
#[derive(Debug)]
pub struct IrqTrace {
    records: Vec<CycleRecord>,
    /// Service-cycle (vector-fetch) events captured by the CPU's
    /// `service_interrupt` path via `Bus::notify_irq_service`.  Added
    /// Phase 1.2 of Track C1 attempt 14 to close the schema asymmetry
    /// with Mesen2's `emu.eventType.irq` / `nmi` oracle.  Independent of
    /// `records` (vector fetches happen on a subset of CPU cycles; we
    /// keep them in their own list so the per-cycle CSV stays
    /// byte-identical to the Phase A baseline).
    service_events: Vec<ServiceEvent>,
    /// Maximum number of records the buffer holds.  Records past the cap
    /// are silently dropped.
    capacity: usize,
    /// Number of records dropped because the buffer hit capacity.
    overflow: u64,
    /// Diagnostic: total `notify_a12` calls observed (whether or not they
    /// produced a transition record).  Helps differentiate "no A12 traffic"
    /// from "tracing path broken".
    pub notify_a12_count: u64,
    /// Diagnostic: number of pushed records that had ≥ 1 A12 event in
    /// their `a12_events` Vec.  If this is 0 but `notify_a12_count` is
    /// nonzero, A12 events were attached to records that got dropped by
    /// the capacity cap (raise `capacity` and re-run).
    pub records_with_a12_count: u64,
    /// Diagnostic: total `notify_irq_service` calls observed.  Helps
    /// differentiate "test ROM never serviced an IRQ during the trace
    /// window" from "tracing path broken".
    pub notify_irq_service_count: u64,
}

impl IrqTrace {
    /// Allocate a trace buffer with the given record capacity.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            records: Vec::with_capacity(capacity),
            // Service events are vastly rarer than per-cycle records
            // (interrupts service ≪ once per ~1k cycles on the target
            // ROMs).  Start with a small capacity that grows as needed.
            service_events: Vec::new(),
            capacity,
            overflow: 0,
            notify_a12_count: 0,
            records_with_a12_count: 0,
            notify_irq_service_count: 0,
        }
    }

    /// Number of records captured so far.
    #[must_use]
    pub fn len(&self) -> usize {
        self.records.len()
    }

    /// True if no records have been captured.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.records.is_empty()
    }

    /// Number of records dropped because the buffer was full.
    #[must_use]
    pub const fn overflow(&self) -> u64 {
        self.overflow
    }

    /// Borrow the records.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)] // Vec::as_slice is not stable as const fn yet.
    pub fn records(&self) -> &[CycleRecord] {
        &self.records
    }

    /// Push a new record.  Silently drops if the buffer is at capacity.
    pub fn push(&mut self, rec: CycleRecord) {
        if self.records.len() < self.capacity {
            self.records.push(rec);
        } else {
            self.overflow = self.overflow.saturating_add(1);
        }
    }

    /// Borrow the service events captured so far.
    #[must_use]
    #[allow(clippy::missing_const_for_fn)]
    pub fn service_events(&self) -> &[ServiceEvent] {
        &self.service_events
    }

    /// Push a new vector-service event.  Unbounded (events are rare; the
    /// per-cycle `records` cap is the headline memory pressure).  The
    /// `notify_irq_service_count` diagnostic is incremented unconditionally
    /// so callers can detect "trace never armed" vs "ROM never serviced".
    pub fn push_service(&mut self, ev: ServiceEvent) {
        self.notify_irq_service_count = self.notify_irq_service_count.saturating_add(1);
        self.service_events.push(ev);
    }

    /// Render the service-event list as a UTF-8 CSV string.  Header row
    /// included.
    ///
    /// Columns:
    /// `cpu_cycle, ppu_frame, ppu_scanline, ppu_dot, event_type, vector`.
    /// The schema deliberately mirrors Mesen2's `mesen2_irq_trace.lua`
    /// service-event rows (modulo PC and the apu/nmi flag-state columns,
    /// which `RustyNES` does not snapshot at the vector-fetch boundary).
    #[must_use]
    pub fn service_events_to_csv(&self) -> String {
        use core::fmt::Write as _;
        let mut out = String::new();
        out.push_str("cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,event_type,vector\n");
        for ev in &self.service_events {
            let _ = writeln!(
                &mut out,
                "{},{},{},{},{},{:#06X}",
                ev.cpu_cycle,
                ev.ppu_frame,
                ev.ppu_scanline,
                ev.ppu_dot,
                ev.kind.as_str(),
                ev.vector,
            );
        }
        out
    }

    /// Render the trace as a UTF-8 CSV string.  Header row included.
    ///
    /// Columns: `cpu_cycle, ppu_frame, ppu_scanline, ppu_dot,
    /// irq_pending_mapper_at_low, irq_pending_apu_at_low,
    /// irq_pending_mapper_at_high, irq_pending_apu_at_high, nmi_line,
    /// a12_events, dmc_dma_pending_pre, dmc_dma_pending_post,
    /// dmc_dma_short_post, dmc_abort_pending_post, dmc_abort_delay_post,
    /// dmc_dma_cooldown_post, dmc_dma_delay_post, apu_phase_post,
    /// in_dmc_dma, dma_cycles_owed, bus_access, bus_addr, bus_data`.
    ///
    /// The `a12_events` column is a `|`-separated list of `sub_dot:level`
    /// pairs (e.g. `1:1|2:0` = rise on dot 1, fall on dot 2 within this
    /// CPU cycle), or empty if no transitions.
    ///
    /// Backward compatibility: tools that parsed the pre-Session-21
    /// 10-column schema can ignore the trailing columns (they appear
    /// AFTER `a12_events` so the column order through column 10 is
    /// byte-identical).  The Session-21 DMC tooling consumes the new
    /// columns; `scripts/irq_trace_cross_diff.py` was extended in lock-
    /// step to handle either schema.
    #[must_use]
    pub fn to_csv(&self) -> String {
        self.to_csv_filtered(|_, _| true)
    }

    /// Render only the records matching `keep(record, prev_record)`.
    /// Useful for trimming a multi-megabyte raw trace to just the
    /// IRQ-relevant cycles (e.g. records where any IRQ line is asserted
    /// or an A12 transition occurred).  The first record's `prev` is
    /// `None`.
    #[must_use]
    pub fn to_csv_filtered<F>(&self, mut keep: F) -> String
    where
        F: FnMut(&CycleRecord, Option<&CycleRecord>) -> bool,
    {
        use core::fmt::Write as _;
        let mut out = String::new();
        out.push_str(
            "cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,\
             irq_pending_mapper_at_low,irq_pending_apu_at_low,\
             irq_pending_mapper_at_high,irq_pending_apu_at_high,\
             nmi_line,a12_events,\
             dmc_dma_pending_pre,dmc_dma_pending_post,dmc_dma_short_post,\
             dmc_abort_pending_post,dmc_abort_delay_post,\
             dmc_dma_cooldown_post,dmc_dma_delay_post,apu_phase_post,\
             in_dmc_dma,dma_cycles_owed,bus_access,bus_addr,bus_data\n",
        );
        let mut prev: Option<&CycleRecord> = None;
        for r in &self.records {
            if !keep(r, prev) {
                prev = Some(r);
                continue;
            }
            let _ = write!(
                &mut out,
                "{},{},{},{},{},{},{},{},{},",
                r.cpu_cycle,
                r.ppu_frame,
                r.ppu_scanline,
                r.ppu_dot,
                u8::from(r.irq_pending_mapper_at_low),
                u8::from(r.irq_pending_apu_at_low),
                u8::from(r.irq_pending_mapper_at_high),
                u8::from(r.irq_pending_apu_at_high),
                u8::from(r.nmi_line),
            );
            let mut first = true;
            for ev in &r.a12_events {
                if !first {
                    out.push('|');
                }
                first = false;
                let _ = write!(&mut out, "{}:{}", ev.sub_dot, u8::from(ev.level));
            }
            // Session-21 DMC + bus-access columns.
            let _ = writeln!(
                &mut out,
                ",{},{},{},{},{},{},{},{},{},{},{},{:#06X},{:#04X}",
                u8::from(r.dmc_dma_pending_pre),
                u8::from(r.dmc_dma_pending_post),
                u8::from(r.dmc_dma_short_post),
                u8::from(r.dmc_abort_pending_post),
                r.dmc_abort_delay_post,
                r.dmc_dma_cooldown_post,
                r.dmc_dma_delay_post,
                u8::from(r.apu_phase_post),
                u8::from(r.in_dmc_dma),
                r.dma_cycles_owed,
                r.bus_access.as_str(),
                r.bus_addr,
                r.bus_data,
            );
            prev = Some(r);
        }
        out
    }

    /// Convenience filter: keep records where an IRQ-line transitioned
    /// at EITHER M2 phase, the NMI line was high, or an A12 transition
    /// was observed.  These are the cycles that the coordinated change
    /// (Track C1) actually alters; the steady-state idle cycles are
    /// uninteresting.
    pub fn is_irq_event(r: &CycleRecord, prev: Option<&CycleRecord>) -> bool {
        if !r.a12_events.is_empty() {
            return true;
        }
        if r.nmi_line {
            return true;
        }
        prev.map_or(
            r.irq_pending_mapper_at_low
                || r.irq_pending_apu_at_low
                || r.irq_pending_mapper_at_high
                || r.irq_pending_apu_at_high,
            |p| {
                r.irq_pending_mapper_at_low != p.irq_pending_mapper_at_low
                    || r.irq_pending_apu_at_low != p.irq_pending_apu_at_low
                    || r.irq_pending_mapper_at_high != p.irq_pending_mapper_at_high
                    || r.irq_pending_apu_at_high != p.irq_pending_apu_at_high
                    || r.nmi_line != p.nmi_line
            },
        )
    }

    /// Convenience filter for the Session-21 DMC tooling: keep records
    /// where any DMC SCHEDULER state changed (pending/abort/cooldown/
    /// delay/short/`in_dmc_dma`) OR any IRQ-event signal triggered (per
    /// [`Self::is_irq_event`]).
    ///
    /// NOTE: this filter does NOT retain records on bus-access type
    /// changes alone (`R` → `W` → `I` switches happen on every
    /// instruction boundary, exploding the golden CSV size).  Bus-
    /// access columns ARE captured in the kept records — they're just
    /// not the trigger for retention.  Phase B's diagnosis pass uses
    /// `target/irq_trace/<slug>.full.csv` (every cycle, not filtered)
    /// for the dense bus-access cross-diff window.
    ///
    /// This is the filter Sprint 1 iteration 2's diagnosis pass uses
    /// to trim multi-megabyte raw traces to DMC-relevant cycles for
    /// cross-diffing against Mesen2.  Steady-state CPU execution
    /// without DMC activity collapses to header-only output.
    pub fn is_dmc_or_irq_event(r: &CycleRecord, prev: Option<&CycleRecord>) -> bool {
        if Self::is_irq_event(r, prev) {
            return true;
        }
        prev.map_or(
            r.dmc_dma_pending_pre
                || r.dmc_dma_pending_post
                || r.dmc_abort_pending_post
                || r.dmc_abort_delay_post > 0
                || r.dmc_dma_cooldown_post > 0
                || r.dmc_dma_delay_post > 0
                || r.in_dmc_dma
                || r.dma_cycles_owed > 0,
            |p| {
                r.dmc_dma_pending_pre != p.dmc_dma_pending_pre
                    || r.dmc_dma_pending_post != p.dmc_dma_pending_post
                    || r.dmc_dma_short_post != p.dmc_dma_short_post
                    || r.dmc_abort_pending_post != p.dmc_abort_pending_post
                    || r.dmc_abort_delay_post != p.dmc_abort_delay_post
                    || r.dmc_dma_cooldown_post != p.dmc_dma_cooldown_post
                    || r.dmc_dma_delay_post != p.dmc_dma_delay_post
                    || r.in_dmc_dma != p.in_dmc_dma
                    || r.dma_cycles_owed != p.dma_cycles_owed
            },
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn empty_trace_renders_header_only() {
        let t = IrqTrace::with_capacity(0);
        let csv = t.to_csv();
        assert!(csv.starts_with("cpu_cycle,"));
        assert_eq!(csv.lines().count(), 1);
    }

    /// Test helper: produce a default-valued record at the given cycle.
    /// Keeps each test's setup focused on the fields it actually exercises.
    fn rec(cyc: u64) -> CycleRecord {
        CycleRecord {
            cpu_cycle: cyc,
            pc: 0,
            ppu_scanline: 0,
            ppu_dot: 0,
            ppu_frame: 0,
            irq_pending_mapper_at_low: false,
            irq_pending_apu_at_low: false,
            irq_pending_mapper_at_high: false,
            irq_pending_apu_at_high: false,
            nmi_line: false,
            a12_events: Vec::new(),
            dmc_dma_pending_pre: false,
            dmc_dma_pending_post: false,
            dmc_dma_short_post: false,
            dmc_abort_pending_post: false,
            dmc_abort_delay_post: 0,
            dmc_dma_cooldown_post: 0,
            dmc_dma_delay_post: 0,
            apu_phase_post: false,
            in_dmc_dma: false,
            dma_cycles_owed: 0,
            bus_access: BusAccess::Idle,
            bus_addr: 0,
            bus_data: 0,
            put_cycle_post: false,
            dmc_timer_post: 0,
            dmc_bits_remaining_post: 0,
            dmc_silence_post: false,
            dmc_buffer_full_post: false,
        }
    }

    #[test]
    fn push_respects_capacity() {
        let mut t = IrqTrace::with_capacity(2);
        for cyc in 0..5 {
            t.push(rec(cyc));
        }
        // Linear cap: keep the first `capacity` records.
        assert_eq!(t.len(), 2);
        // 5 pushed total; 3 dropped.
        assert_eq!(t.overflow(), 3);
        assert_eq!(t.notify_a12_count, 0);
        // Chronological order: cycles 0 and 1 (the first two).
        assert_eq!(t.records()[0].cpu_cycle, 0);
        assert_eq!(t.records()[1].cpu_cycle, 1);
    }

    #[test]
    fn csv_format_round_trip_a12_events() {
        let mut t = IrqTrace::with_capacity(4);
        let mut r = rec(42);
        r.ppu_dot = 260;
        r.ppu_frame = 1;
        r.irq_pending_mapper_at_low = true;
        r.irq_pending_mapper_at_high = true;
        r.a12_events = alloc::vec![A12Event {
            sub_dot: 1,
            level: true
        }];
        t.push(r);
        let csv = t.to_csv();
        // Schema (Session-21+): the IRQ columns through `a12_events` are
        // byte-identical to the pre-Session-21 baseline; the DMC and
        // bus-access columns follow.  This test pins the IRQ-columns
        // prefix shape so the irq_trace_cross_diff.py tool keeps
        // working unchanged.
        assert!(
            csv.contains("42,1,0,260,1,0,1,0,0,1:1,"),
            "CSV row missing expected two-phase shape: {csv}"
        );
        // The DMC + bus-access suffix is appended after `a12_events`.
        // Default record has DMC scheduler idle / bus access idle.
        assert!(
            csv.contains(",0,0,0,0,0,0,0,0,0,0,I,0x0000,0x00"),
            "CSV row missing Session-21 DMC + bus-access suffix: {csv}"
        );
    }

    /// Phase A invariant: when the M2-low and M2-high snapshots differ,
    /// the CSV emits BOTH columns and the two columns disagree.  This is
    /// the empirical signal Track C1's coordinated change is designed to
    /// observe.
    #[test]
    fn csv_format_distinguishes_low_and_high_phase() {
        let mut t = IrqTrace::with_capacity(2);
        // M2-low: nothing asserted yet.  M2-high: mapper IRQ is now high
        // (asserted by an A12 transition mid-cycle).  APU stays low at
        // both phases.
        let mut r = rec(1_369_997);
        r.ppu_dot = 260;
        r.ppu_frame = 17;
        r.irq_pending_mapper_at_high = true;
        r.a12_events = alloc::vec![A12Event {
            sub_dot: 0,
            level: true
        }];
        t.push(r);
        let csv = t.to_csv();
        // Expect mapper@low=0, apu@low=0, mapper@high=1, apu@high=0.
        assert!(
            csv.contains("1369997,17,0,260,0,0,1,0,0,0:1,"),
            "CSV row should expose the M2-low → M2-high asymmetry: {csv}"
        );
        // Header must list both phases so downstream tooling can pick the
        // right column.
        let header = csv.lines().next().expect("header");
        assert!(header.contains("irq_pending_mapper_at_low"));
        assert!(header.contains("irq_pending_mapper_at_high"));
        assert!(header.contains("irq_pending_apu_at_low"));
        assert!(header.contains("irq_pending_apu_at_high"));
        // Session-21: header MUST also expose the DMC + bus-access columns
        // so the cross-diff tool knows what schema it's parsing.
        assert!(header.contains("dmc_dma_pending_pre"));
        assert!(header.contains("dmc_dma_pending_post"));
        assert!(header.contains("dmc_abort_pending_post"));
        assert!(header.contains("dmc_dma_cooldown_post"));
        assert!(header.contains("apu_phase_post"));
        assert!(header.contains("in_dmc_dma"));
        assert!(header.contains("bus_access"));
        assert!(header.contains("bus_addr"));
        assert!(header.contains("bus_data"));
    }

    /// Session-21 DMC + bus-access columns surface in the CSV with the
    /// documented schema.  This test exercises a non-trivial record so
    /// the field-to-column mapping is pinned.
    #[test]
    fn csv_format_dmc_and_bus_access_columns() {
        let mut t = IrqTrace::with_capacity(2);
        let mut r = rec(123);
        // A DMC DMA halt cycle in progress: pending+short, abort delay
        // counting down, cooldown is 4 (just after a get), APU is in get
        // phase, bus is busy with a DMA read at $C100 = $48.
        r.dmc_dma_pending_pre = true;
        r.dmc_dma_pending_post = true;
        r.dmc_dma_short_post = true;
        r.dmc_abort_pending_post = false;
        r.dmc_abort_delay_post = 2;
        r.dmc_dma_cooldown_post = 4;
        r.dmc_dma_delay_post = 0;
        r.apu_phase_post = true;
        r.in_dmc_dma = true;
        r.dma_cycles_owed = 0;
        r.bus_access = BusAccess::DmaRead;
        r.bus_addr = 0xC100;
        r.bus_data = 0x48;
        t.push(r);
        let csv = t.to_csv();
        // The DMC + bus-access suffix is appended after the a12 column.
        assert!(
            csv.contains(",1,1,1,0,2,4,0,1,1,0,r,0xC100,0x48"),
            "Session-21 DMC + bus-access columns mis-serialized: {csv}"
        );
    }

    /// `is_dmc_or_irq_event` filter retains records with DMC scheduler
    /// state changes but EXCLUDES bus-access-only transitions (those
    /// would explode the golden CSV size on every instruction
    /// boundary).
    #[test]
    fn dmc_event_filter_catches_scheduler_changes_only() {
        let mut t = IrqTrace::with_capacity(4);
        // Cycle 0: all idle. NO transition, NO active state -> DROP.
        t.push(rec(0));
        // Cycle 1: DMC DMA armed (pending_pre flips false → true). KEEP.
        let mut r1 = rec(1);
        r1.dmc_dma_pending_pre = true;
        r1.dmc_dma_pending_post = true;
        t.push(r1);
        // Cycle 2: still pending (no DMC scheduler state change), but
        // bus accesses changed Idle → Read.  The Session-21 filter
        // intentionally DOES NOT retain on bus-access-only changes:
        // bus access flips happen on every instruction boundary and
        // would explode the golden file size for ROMs that don't
        // exercise DMC.  Bus-access columns are still captured in the
        // kept records — Phase B uses the `.full.csv` for the dense
        // bus-access window.  -> DROP.
        let mut r2 = rec(2);
        r2.dmc_dma_pending_pre = true;
        r2.dmc_dma_pending_post = true;
        r2.bus_access = BusAccess::Read;
        r2.bus_addr = 0x1000;
        r2.bus_data = 0xAA;
        t.push(r2);
        // Cycle 3: cooldown counted down (still nonzero, but the field
        // value CHANGED from 0 to 4) — DMC scheduler state change.
        // -> KEEP.
        let mut r3 = rec(3);
        r3.dmc_dma_pending_pre = true;
        r3.dmc_dma_pending_post = true;
        r3.dmc_dma_cooldown_post = 4;
        r3.bus_access = BusAccess::Read;
        r3.bus_addr = 0x1000;
        r3.bus_data = 0xAA;
        t.push(r3);
        let csv = t.to_csv_filtered(IrqTrace::is_dmc_or_irq_event);
        // Lines: header + cycle 1 (pending_pre flip) + cycle 3 (cooldown
        // change).  Cycle 0 has no state at all; cycle 2 only changed
        // bus_access (excluded by the filter).
        let lines: Vec<&str> = csv.lines().collect();
        assert_eq!(lines.len(), 3, "expected 2 data rows after header: {csv}");
        assert!(
            lines[1].starts_with("1,"),
            "cycle 1 should be first data row: {}",
            lines[1]
        );
        assert!(
            lines[2].starts_with("3,"),
            "cycle 3 should be second data row: {}",
            lines[2]
        );
    }

    /// `M2Phase` is retained as the canonical reference enum used by the
    /// docs/ADR and by Phases B-and-later when the bus exposes the phase
    /// rail.  Phase A removes its per-record usage; this test just
    /// guards against accidental deletion.
    #[test]
    fn m2_phase_enum_still_exposed() {
        assert_eq!(M2Phase::Low.as_str(), "L");
        assert_eq!(M2Phase::High.as_str(), "H");
    }

    /// Phase 1.2 of Track C1 attempt 14: service events are captured into
    /// a sidecar list (NOT into the per-cycle records) and emitted as a
    /// separate CSV.  The per-cycle CSV stays byte-identical to the
    /// Phase A baseline.
    #[test]
    fn service_events_recorded_and_rendered_independently() {
        let mut t = IrqTrace::with_capacity(0);
        t.push_service(ServiceEvent {
            cpu_cycle: 1_370_004,
            ppu_scanline: 0,
            ppu_dot: 263,
            ppu_frame: 46,
            kind: ServiceKind::Irq,
            vector: 0xFFFE,
        });
        t.push_service(ServiceEvent {
            cpu_cycle: 1_500_000,
            ppu_scanline: 241,
            ppu_dot: 2,
            ppu_frame: 50,
            kind: ServiceKind::Nmi,
            vector: 0xFFFA,
        });
        // Diagnostic count matches push count.
        assert_eq!(t.notify_irq_service_count, 2);
        // Per-cycle CSV stays empty (just header) — Phase A invariant.
        assert_eq!(t.to_csv().lines().count(), 1);
        // Service CSV holds both events with the documented schema.
        let svc = t.service_events_to_csv();
        assert!(svc.starts_with("cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,event_type,vector\n"));
        assert!(
            svc.contains("1370004,46,0,263,irq_svc,0xFFFE"),
            "service CSV missing IRQ row: {svc}"
        );
        assert!(
            svc.contains("1500000,50,241,2,nmi_svc,0xFFFA"),
            "service CSV missing NMI row: {svc}"
        );
        // Service events accessible by &[ServiceEvent] for downstream
        // tooling that wants struct-typed access.
        assert_eq!(t.service_events().len(), 2);
        assert_eq!(t.service_events()[0].kind, ServiceKind::Irq);
        assert_eq!(t.service_events()[1].kind, ServiceKind::Nmi);
    }
}
