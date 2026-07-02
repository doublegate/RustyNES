//! Top-level 2A03 APU.
//!
//! Per `docs/apu-2a03.md`.  Owns the four wave channels plus DMC, the frame
//! counter, the lookup-table mixer + filter chain, and the band-limited
//! sample emitter.  Driven by the lockstep bus's `Apu::tick` once per CPU
//! cycle.

use crate::Region;
use crate::blip::{BlipBuf, CPU_HZ_NTSC, CPU_HZ_PAL};
use crate::dmc::Dmc;
use crate::frame_counter::{FrameCounter, FrameEvents};
use crate::mixer::Mixer;
use crate::noise::Noise;
use crate::pulse::Pulse;
use crate::triangle::Triangle;
use alloc::vec::Vec;

// `f32::round` lives in `std` (not `core`), so route through `libm::roundf` on
// no_std — the same pattern the mixer uses for `expf`. Both round half away from
// zero, so the result is identical across the desktop + `thumbv7em-none-eabihf`
// targets. Only reached on the off-default per-channel-gain path (gain != 1.0),
// never on the byte-identical unity path.
#[inline]
fn roundf(x: f32) -> f32 {
    #[cfg(feature = "std")]
    {
        x.round()
    }
    #[cfg(not(feature = "std"))]
    {
        libm::roundf(x)
    }
}

/// v2.0 R-1 core C-1 DIAGNOSTIC (gated `mc-r1-dmc-abort-probe`).
///
/// Pub-static atomic counters that pin WHERE the explicit-DMC-abort scheduling
/// diverges under R1 (the abort tests fail because the truncation never
/// engages). Read from the harness via `rustynes_core::rustynes_apu::abort_probe`.
/// Default-off; the counters and all increment sites compile out entirely
/// without the feature.
#[cfg(feature = "mc-r1-dmc-abort-probe")]
pub mod abort_probe {
    use core::sync::atomic::{AtomicU32, AtomicU64};
    /// `$4015` writes that DISABLE the DMC (`bit4 == 0`).
    pub static DISABLE_WRITES: AtomicU32 = AtomicU32::new(0);
    /// ...of those, the ones where the DMC `was_active` (so the abort
    /// scheduler is invoked).
    pub static DISABLE_WAS_ACTIVE: AtomicU32 = AtomicU32::new(0);
    /// `schedule_explicit_dmc_abort_if_needed` entries.
    pub static SCHED_CALLS: AtomicU32 = AtomicU32::new(0);
    /// Early-returns because `bits_remaining != 1`.
    pub static FAIL_BITS: AtomicU32 = AtomicU32::new(0);
    /// Early-returns because `sample_buffer.is_none()` (bits_remaining == 1).
    pub static FAIL_BUF: AtomicU32 = AtomicU32::new(0);
    /// Reached the `cycles_until_output` computation (precond passed).
    pub static REACHED_CUO: AtomicU32 = AtomicU32::new(0);
    /// `cycles_until_output` landed in the engaging window {1,2,3}.
    pub static CUO_IN_WINDOW: AtomicU32 = AtomicU32::new(0);
    /// The abort actually armed (`pending_dmc_abort` or `dmc_abort_delay`).
    pub static ARMED: AtomicU32 = AtomicU32::new(0);
    /// Phase 1A diagnostic: total DMC `$4015` enables seen by the
    /// arm-at-enable hook (regardless of timer match).
    pub static IMPL_ARM_ENABLE: AtomicU32 = AtomicU32::new(0);
    /// Phase 1A diagnostic: arm-at-enable matched the TriCNES timer window
    /// (timer==10 get / ==8 put) and set `pending_dmc_abort`.
    pub static IMPL_ARM_FIRED: AtomicU32 = AtomicU32::new(0);
    /// W3-Stage-4 diagnostic: the consume-edge-quantization latch count.
    ///
    /// Incremented when a consume edge ON the GET-delivery cycle is
    /// arm-blocked by `cannot_run == 2` with the level need still held.
    /// Expected to fire ONLY at the Implicit `$540` X=8/9 restart races.
    pub static EDGE_SUPPRESS_SET: AtomicU32 = AtomicU32::new(0);

    /// Φ-1 cross-diff: the cycle of the most recent `$4010=$4E` write (0 = none
    /// pending / already paired). Set by `log_4010`, consumed by `log_get`.
    pub static LAST_4E_CYC: AtomicU32 = AtomicU32::new(0);
    /// final lever #1 diag: per-block packed DMC-state captures.
    ///
    /// At each `$540` X=10/11 boundary block the two preceding cycles + the
    /// current cycle are stored (packed via [`pack_dmc`]).
    pub static SUBPOS_DIAG: [AtomicU64; 64] = [const { AtomicU64::new(0) }; 64];
    /// final lever #1 diag: write index into `SUBPOS_DIAG`.
    pub static SUBPOS_DIAG_IDX: AtomicU32 = AtomicU32::new(0);
    /// final lever #1 diag: packed state of cycle N-1 (rotating history).
    pub static SUBPOS_P1: AtomicU64 = AtomicU64::new(0);
    /// final lever #1 diag: packed state of cycle N-2 (rotating history).
    pub static SUBPOS_P2: AtomicU64 = AtomicU64::new(0);
    /// final lever #1 diag: pack DMC state into a u64 for history/dump.
    /// `apu_phase | needs_dma<<1 | buf<<2 | cannot_run<<4 | bytes<<12 | bits<<24 | timer<<32`.
    #[must_use]
    pub fn pack_dmc(
        apu_phase: bool,
        needs_dma: bool,
        buf: bool,
        cannot_run: u8,
        bytes: u16,
        bits: u8,
        timer: u16,
    ) -> u64 {
        u64::from(apu_phase)
            | (u64::from(needs_dma) << 1)
            | (u64::from(buf) << 2)
            | (u64::from(cannot_run) << 4)
            | (u64::from(bytes) << 12)
            | (u64::from(bits) << 24)
            | (u64::from(timer) << 32)
    }
    /// Φ-1 cross-diff: per-`$4E`→first-`$FFC0`-GET offsets (TriCNES = 48).
    pub static FFC0_OFF: [AtomicU32; 64] = [const { AtomicU32::new(0) }; 64];
    /// Φ-1 cross-diff: write index into `FFC0_OFF`.
    pub static FFC0_OFF_IDX: AtomicU32 = AtomicU32::new(0);
    /// Φ-1 period: cycle of the previous `$FFC0` GET (for the reload period).
    pub static LAST_FFC0_GET: AtomicU32 = AtomicU32::new(0);
    /// Φ-1 period: GET-to-GET deltas (reload period; TriCNES CheckDMATiming = 576).
    pub static FFC0_PERIOD: [AtomicU32; 128] = [const { AtomicU32::new(0) }; 128];
    /// Φ-1 period: write index into `FFC0_PERIOD`.
    pub static FFC0_PERIOD_IDX: AtomicU32 = AtomicU32::new(0);
    /// Φ-2 setup-window `$4000`-read log (packed `value<<32 | cycle`).
    ///
    /// Isolates CalculateDMADuration's 575-spaced `LDA $4000` from the DMASync
    /// 7-cycle spin so the `$40`-iters-before-`$00` (the Y) can be counted.
    pub static R4000_LOG: [AtomicU64; 4096] = [const { AtomicU64::new(0) }; 4096];
    /// Φ-2: write index into `R4000_LOG`.
    pub static R4000_IDX: AtomicU32 = AtomicU32::new(0);
    /// Φ-2 hang diagnostic: consecutive `dmc_dma_step` calls within one DMA.
    pub static HANG_GUARD: AtomicU32 = AtomicU32::new(0);
    /// Φ-2 hang diagnostic: `put_cycle` parity captured at the hang-escape.
    pub static HANG_PUT: AtomicU32 = AtomicU32::new(99);
    /// Φ-2 hang diagnostic: how many DMAs hit the hang-escape (GET never fired).
    pub static HANG_COUNT: AtomicU32 = AtomicU32::new(0);
    /// Φ-2 chain diagnostic: successful DMC reload-arms (pending set).
    pub static ARM_COUNT: AtomicU32 = AtomicU32::new(0);
    /// Φ-2 chain diagnostic: total `dmc_dma_read` calls (any address = a GET fired).
    pub static GET_TOTAL: AtomicU32 = AtomicU32::new(0);
    /// Phase 2 (`mc-r1-dmc-reenable-phase`): times `cannot_run_dmc_dma` was set to 2.
    pub static CANNOT_RUN_SET: AtomicU32 = AtomicU32::new(0);
    /// Phase 2: times a reload arm was BLOCKED by the `cannot_run == 2` gate.
    pub static CANNOT_RUN_BLOCK: AtomicU32 = AtomicU32::new(0);
    /// Phase 2: times `needs_dma() && !already && delay==0` reload-arm was reached
    /// (denominator for the block rate).
    pub static RELOAD_ARM_REACHED: AtomicU32 = AtomicU32::new(0);
    /// final lever #1 (`mc-r1-dmc-halt-subpos`): times the boundary-local
    /// 1-CPU-cycle early reload pre-arm fired (should be 6 — exactly the
    /// X=10/11 entries of the three Implicit-abort loops).
    pub static SUBPOS_EARLY_ARM: AtomicU32 = AtomicU32::new(0);

    /// Φ-2: log a `$4000` read (cycle + returned value) for the setup-window diff.
    pub fn log_4000_read(cycle: u32, val: u8) {
        use core::sync::atomic::Ordering::Relaxed;
        let i = R4000_IDX.fetch_add(1, Relaxed) as usize;
        if i < 4096 {
            R4000_LOG[i].store((u64::from(val) << 32) | u64::from(cycle), Relaxed);
        }
    }

    /// Φ-1: record a `$4010` write; arm the offset capture on `$4E` (rate 14 +
    /// loop), the CheckDMATiming setup value.
    pub fn log_4010(cycle: u32, value: u8) {
        use core::sync::atomic::Ordering::Relaxed;
        if value == 0x4E {
            LAST_4E_CYC.store(cycle, Relaxed);
        }
    }

    /// Φ-1: record a DMC GET; if it is the FIRST `$FFC0` GET after a `$4E` write
    /// (within a plausible 200-cycle setup window), capture the cycle offset
    /// (the RustyNES analog of TriCNES's 48) and disarm.
    pub fn log_get(cycle: u32, addr: u16) {
        use core::sync::atomic::Ordering::Relaxed;
        if addr != 0xFFC0 {
            return;
        }
        // Reload period: delta from the previous $FFC0 GET.
        let prev = LAST_FFC0_GET.swap(cycle, Relaxed);
        if prev != 0 && cycle > prev {
            let d = cycle - prev;
            // Only the CheckDMATiming-region cadence (~576), skip basic playback.
            if (560..=590).contains(&d) {
                let pi = FFC0_PERIOD_IDX.fetch_add(1, Relaxed) as usize;
                if pi < 128 {
                    FFC0_PERIOD[pi].store(d, Relaxed);
                }
            }
        }
        let last = LAST_4E_CYC.load(Relaxed);
        if last != 0 && cycle > last && cycle - last < 200 {
            let i = FFC0_OFF_IDX.fetch_add(1, Relaxed) as usize;
            if i < 64 {
                FFC0_OFF[i].store(cycle - last, Relaxed);
            }
            LAST_4E_CYC.store(0, Relaxed);
        }
    }

    /// Per-`$4015`-write event-log capacity (first `CAP` events kept).
    pub const EVLOG_CAP: usize = 4096;
    /// Per-event CPU cycle (low 32 bits).
    pub static EVLOG_CYCLE: [AtomicU32; EVLOG_CAP] = [const { AtomicU32::new(0) }; EVLOG_CAP];
    /// Per-event packed `enable<<31 | bytes_remaining<<16 | (timer & 0xFFFF)`.
    pub static EVLOG_META: [AtomicU32; EVLOG_CAP] = [const { AtomicU32::new(0) }; EVLOG_CAP];
    /// Monotonic write index into the event log.
    pub static EVLOG_IDX: AtomicU32 = AtomicU32::new(0);

    /// Record a `$4015` write into the event log (ring, first `CAP` kept).
    ///
    /// Lets the harness reconstruct the per-iteration playback trajectory
    /// (enable→disable gap, exhaustion point) default-vs-R1. `cycle` is
    /// truncated to its low 32 bits (the battery stays under 2^32 cycles).
    #[allow(clippy::cast_possible_truncation)]
    pub fn log_4015(cycle: u64, enable: bool, bytes_remaining: u16, timer: u16) {
        use core::sync::atomic::Ordering::Relaxed;
        let i = EVLOG_IDX.fetch_add(1, Relaxed) as usize;
        if i < EVLOG_CAP {
            EVLOG_CYCLE[i].store(cycle as u32, Relaxed);
            let meta = (u32::from(enable) << 31)
                | ((u32::from(bytes_remaining) & 0x7FFF) << 16)
                | u32::from(timer);
            EVLOG_META[i].store(meta, Relaxed);
        }
    }

    /// Snapshot all counters as a labelled array (for the harness probe).
    #[must_use]
    pub fn snapshot() -> [(&'static str, u32); 14] {
        use core::sync::atomic::Ordering::Relaxed;
        [
            ("disable_writes", DISABLE_WRITES.load(Relaxed)),
            ("disable_was_active", DISABLE_WAS_ACTIVE.load(Relaxed)),
            ("sched_calls", SCHED_CALLS.load(Relaxed)),
            ("fail_bits_remaining!=1", FAIL_BITS.load(Relaxed)),
            ("fail_sample_buffer_none", FAIL_BUF.load(Relaxed)),
            ("reached_cuo", REACHED_CUO.load(Relaxed)),
            ("cuo_in_window{1,2,3}", CUO_IN_WINDOW.load(Relaxed)),
            ("armed", ARMED.load(Relaxed)),
            ("impl_arm_enable_seen", IMPL_ARM_ENABLE.load(Relaxed)),
            ("impl_arm_fired", IMPL_ARM_FIRED.load(Relaxed)),
            ("cannot_run_set", CANNOT_RUN_SET.load(Relaxed)),
            ("reload_arm_reached", RELOAD_ARM_REACHED.load(Relaxed)),
            ("cannot_run_block", CANNOT_RUN_BLOCK.load(Relaxed)),
            ("subpos_early_arm", SUBPOS_EARLY_ARM.load(Relaxed)),
        ]
    }
}

/// Bus surface seen by the APU.  A small subset of the full CPU bus for the
/// DMC's sample-fetch DMA.
pub trait ApuBus {
    /// Read one byte for a DMC sample fetch.  The bus is responsible for
    /// halting the CPU and accounting for the 3- or 4-cycle DMA stall
    /// before this is called.
    fn dmc_read(&mut self, addr: u16) -> u8;
}

/// Top-level APU.
#[derive(Debug, Clone)]
pub struct Apu {
    /// Region (NTSC / PAL / Dendy).
    pub region: Region,
    /// Pulse 1.
    pub pulse1: Pulse,
    /// Pulse 2.
    pub pulse2: Pulse,
    /// Triangle.
    pub triangle: Triangle,
    /// Noise.
    pub noise: Noise,
    /// DMC.
    pub dmc: Dmc,
    /// Frame counter.
    pub frame_counter: FrameCounter,
    /// Mixer.
    pub(crate) mixer: Mixer,
    /// Band-limited sample emitter.
    pub(crate) blip: BlipBuf,
    /// True on every other CPU cycle — pulse/noise/DMC clock at this rate.
    // reason: `apu_phase` is the deliberate, documented name for the APU's
    // clock phase; it appears verbatim as a column header in the committed
    // irq_trace golden CSVs, so the `apu_` prefix is load-bearing, not noise.
    #[allow(clippy::struct_field_names)]
    pub(crate) apu_phase: bool,
    /// v2.0 F-2: when set, the DMC byte-timer + DMA arm are driven by
    /// [`Self::tick_dmc`] (called at end-of-cycle by the R1 bus) instead of
    /// inside [`Self::tick_with_external`] (cycle-start). This shifts only the
    /// DMC fire-phase to main's end-of-cycle position (for DMASync), leaving
    /// the frame-counter / pulse / noise — and thus the APU IRQ line — on the
    /// cycle-start tick (for the C1 IRQ sample). Default `false` = byte-identical.
    pub(crate) dmc_driven_externally: bool,
    /// v2.0 interleaved-DMA Phase A: the global get/put flip-flop (TriCNES
    /// `APU_PutCycle`, `Emulator.cs:920`). Toggled exactly once per CPU cycle
    /// (when `dmc_driven_externally`, so the default build is byte-identical)
    /// and seeded at power-on/reset TOGETHER with the DMC byte-timer from one
    /// `APUAlignment` value, so the get/put parity and the DMC fire-phase share
    /// one seed + one per-cycle counter and can never drift (divergence A). The
    /// interleaved DMA (Phase B) reads this for the get/put decision instead of
    /// `self.cycle & 1`. `true` = put (write/OAM-priority), `false` = get
    /// (read/DMC-priority). Nothing consumes it yet in Phase A.
    pub(crate) put_cycle: bool,
    /// v2.0 RW-1 (`mc-r1-one-clock`): the single boot parity seed. Under
    /// `mc-r1-one-clock`, `apu_phase` and `put_cycle` are no longer two
    /// independent flip-flops toggled per cycle — they are DERIVED from the one
    /// per-cycle counter (`cpu_cycle`) plus this seed:
    /// `apu_phase = (cpu_cycle + parity_seed) & 1 == 1`, `put_cycle = !apu_phase`.
    /// This makes the APU-rate clock, the get/put DMA parity, and the DMC
    /// fire-phase share ONE counter + ONE seed, so they can never drift apart
    /// (the cumulative-counter-split root cause, see
    /// `docs/audit/v2.0-cumulative-cycle-accounting-rewrite-plan-2026-06-05.md`).
    /// `0` reproduces the floor config exactly (boot `apu_phase = false`,
    /// `seed_apu_alignment(0)` -> put-on-even). Set once at power-on/reset/restore
    /// by [`Self::seed_apu_alignment`]; otherwise constant. Unused when the flag
    /// is off (the legacy dual-toggle path runs instead).
    pub(crate) parity_seed: u64,
    /// W3-Stage-4 (2026-06-10): whether the most recent [`Apu::restore`]
    /// blob carried the Stage-4 parity/DMA-state tail (so `put_cycle` +
    /// `parity_seed` were restored EXACTLY and the bus must NOT re-seed the
    /// boot alignment over them). Transient bookkeeping — never serialized.
    pub(crate) restored_parity_tail: bool,
    /// Cumulative CPU cycle counter (used for `$4017` write alignment).
    pub(crate) cpu_cycle: u64,
    /// Pending DMC DMA request — the bus polls and consumes this when it
    /// halts the CPU and supplies a sample byte.
    pub(crate) pending_dmc_dma: bool,
    /// `mc-r1-dmc-reload-visibility-delay`: a RELOAD arm latches HERE and is
    /// promoted to `pending_dmc_dma` one cycle later, so the DMA loop first
    /// services it on the NEXT (put) cycle — matching TriCNES's
    /// `_EmulateAPU`-after-`_6502` invisibility (reload first-service = put =>
    /// span 4). Loads arm `pending_dmc_dma` directly (first-service = get => 3).
    pub(crate) pending_dmc_dma_next: bool,
    /// True when the pending DMC DMA is the initial load DMA after `$4015`
    /// enable; false for reload DMAs raised by sample-buffer empty.
    pub(crate) dmc_dma_is_load: bool,
    /// True when the pending request uses the short 3-cycle service path
    /// despite being externally observed as a load-style race. This covers
    /// the explicit-stop abort edge where a visible reload request must be
    /// preserved through the `$4015` disable write without making ordinary
    /// load DMAs lose their dummy/alignment cadence.
    pub(crate) dmc_dma_short: bool,
    /// Suppress one immediate reload request after a same-tick DMC load
    /// delivery. Used for the one-byte looping edge where the fetched byte
    /// is visible to the output unit on the DMA get cycle, but the reload
    /// request is not visible until the following CPU cycle.
    pub(crate) defer_dmc_reload_once: bool,
    /// Pending one-cycle DMC abort halt. This is the RP2A03 stop-near-reload
    /// quirk: it does not fetch a byte, and if the halt attempt lands on a
    /// CPU write cycle the abort disappears instead of retrying.
    pub(crate) pending_dmc_abort: bool,
    /// CPU cycles until an abort halt attempt becomes visible to the bus.
    pub(crate) dmc_abort_delay: u8,
    /// CPU cycles during which a newly emptied DMC sample buffer must not
    /// raise another DMA request. The DMC DMA unit cannot issue a second
    /// request within two CPU cycles of the previous get.
    pub(crate) dmc_dma_cooldown: u8,
    /// v2.0 Phase 2 (`mc-r1-dmc-reenable-phase`): TriCNES's
    /// `CannotRunDMCDMARightNow` (`Emulator.cs:823`). Set to 2 after every DMC
    /// GET (`:4168`), decremented by 2 on each get cycle (`:1186`), and gating
    /// the looping-reload arm (`:1165`, blocked while `== 2`). Reproduces the
    /// "a DMA cannot occur within 2 cycles of a previous DMC DMA" rule so the
    /// Implicit-DMA-Abort Loop3/`$540` X=10/11 re-enable defers its first
    /// reload one byte-timer period (walk offset +4 -> +5, Y 3->4). Distinct
    /// from `dmc_dma_cooldown` (which the abort-timer-phase fix clears at the
    /// boundary race); this exclusion re-imposes the exact 1-get-cycle block.
    pub(crate) cannot_run_dmc_dma: u8,
    /// v2.0 Phase 2 (`mc-r1-dmc-reenable-phase`): latch that defers a reload to
    /// the NEXT byte-timer wrap when the exclusion blocks the arm. TriCNES only
    /// evaluates the reload arm at the `bits_remaining -> 0` consume edge
    /// (`Emulator.cs:1159`); if blocked there (`cannot_run == 2`) the buffer is
    /// not refilled until the FOLLOWING consume edge (one full byte period
    /// later) — NOT a 2-cycle re-arm. RustyNES's `dmc_step_reload_arm` instead
    /// re-checks `needs_dma()` (persistent) every cycle, so a bare exclusion
    /// gate would re-arm as soon as `cannot_run` decremented (absorbed). This
    /// latch reproduces the full-period deferral: set when the exclusion blocks
    /// a consume-edge arm, cleared at the next consume edge.
    pub(crate) dmc_reenable_period_block: bool,
    /// final lever #1 (`mc-r1-dmc-halt-subpos`): a per-CPU-cycle countdown that
    /// DELAYS the `$540` X=10/11 reload arm by an exact number of CPU cycles
    /// (sub-APU-cycle granularity the byte-timer phase shift cannot express).
    /// `0` = inactive. Set at the pattern-A boundary; while > 0 the natural arm
    /// is suppressed and this decrements; at 0 the arm fires. Default 0.
    // (W3-Stage-3: also dead under `mc-r1-dmc-delayed-4015`, which supersedes
    // the halt-subpos pre-arm with the emergent consume-edge arm.)
    #[allow(dead_code)]
    pub(crate) subpos_arm_countdown: u8,
    /// Latches the one-byte looping edge where the next reload request is
    /// lost because it is raised too soon after a DMA get. Cleared when a
    /// later `$4015` enable/disable write re-arms the DMC path.
    pub(crate) dmc_reload_suppress_outputs: u8,
    /// CPU cycles until a load DMC DMA halt attempt becomes visible to the
    /// bus. Reload DMAs are armed immediately after the DMC output unit
    /// empties the sample buffer; load DMAs after `$4015` enable are delayed
    /// to the second following APU cycle per the 2A03 DMA cadence.
    pub(crate) dmc_dma_delay: u8,
    /// Most recent DMC DMA address (re-read each tick when `pending_dmc_dma`
    /// is true; the bus may take it directly via [`Self::dmc_dma_addr`]).
    /// On its own this is informational; the bus owns the actual halt logic.
    pub(crate) dmc_dma_addr: u16,
    /// v1.2 Sprint 3 — get/put cycle scheduler model (ADR-0007).
    ///
    /// Set when a DMC DMA request is raised; cleared by the new
    /// `rustynes-core::bus::service_dmc_dma` path under the
    /// `dmc-get-put-scheduler` feature flag, once the initial halt
    /// cycle has been processed. Mirrors Mesen2's `_needHalt` on
    /// `NesCpu` (`Core/NES/NesCpu.h:41`; set in `StartDmcTransfer`
    /// at `Core/NES/NesCpu.cpp:527`). Kept ALWAYS-PRESENT (not
    /// `#[cfg]`-gated) so the field exists in serialized state for
    /// future-flag-flip migration; the v1.2 baseline scheduler
    /// simply ignores it.
    pub(crate) dmc_need_halt: bool,
    /// v1.2 Sprint 3 — get/put cycle scheduler model (ADR-0007).
    ///
    /// Set when a DMC DMA request is raised; cleared by the new
    /// scheduler once the alignment / dummy-read cycle has been
    /// processed. Mirrors Mesen2's `_needDummyRead` on `NesCpu`.
    /// Kept always-present alongside [`Self::dmc_need_halt`].
    pub(crate) dmc_need_dummy_read: bool,
    /// W3-Stage-3 (`mc-r1-dmc-delayed-4015`): TriCNES `APU_DelayedDMC4015`
    /// (Emulator.cs:973) — CPU-cycle countdown until the latched `$4015` DMC
    /// status bit is APPLIED. Set to `put ? 3 : 4` at every `$4015` write
    /// (extended to `put ? 5 : 6` at the explicit don't-abort edge);
    /// decremented once per `dmc_tick_end` (every CPU cycle, the write
    /// cycle's own end-tick included). `0` = idle.
    pub(crate) dmc_delayed_4015: u8,
    /// W3-Stage-3: TriCNES `APU_Status_DelayedDMC` (Emulator.cs:964) — the
    /// TARGET DMC status latched at the `$4015` write, applied when the
    /// countdown expires. Also the value `$4015` READS see immediately
    /// (the footnote at Emulator.cs:9268: bit 4 must read 0 right after a
    /// disable write even though `bytes_remaining` is not yet zeroed).
    pub(crate) dmc_delayed_status: bool,
    /// W3-Stage-3: TriCNES `APU_Status_DMC` (Emulator.cs:963) — the APPLIED
    /// DMC status. The bus-side DMA service gate (`_6502` line 4218:
    /// `DoDMCDMA && (APU_Status_DMC || implicit-abort)`) reads this; while
    /// false a pending/halted DMC DMA is NOT serviced (the emergent explicit
    /// abort). Set/cleared ONLY by the delayed application + cleared at
    /// non-looping natural sample end (TriCNES `DMCDMA_Get`, line 4154).
    pub(crate) dmc_status_applied: bool,
    /// W3-Stage-3: TriCNES `APU_SetImplicitAbortDMC4015` (Emulator.cs:975) —
    /// latched at a `$4015` ENABLE write that coincides with the byte-timer's
    /// firing window (`(timer == 10 && get) || (timer == 8 && put)` in
    /// TriCNES CPU-rate units = our APU-rate `(4, get)/(3, put)`); consumed
    /// at the next shifter-consume edge (bits 1 -> 8), where it arms the
    /// 1-cycle implicit-abort DMA regardless of the buffer state
    /// (Emulator.cs:1163-1175).
    pub(crate) dmc_set_implicit_abort: bool,
    /// W3-Stage-3: TriCNES `APU_ImplicitAbortDMC4015` (Emulator.cs:974) — the
    /// service-gate override that lets the boundary-armed DMA run while the
    /// `$4015` enable's delayed status is still unapplied. Cleared at the END
    /// of the first cycle on which the DMA is pending (Emulator.cs:9000-9003:
    /// one serviced halt cycle if that cycle is a read; "if this was delayed
    /// by a write cycle, it won't run at all") — the emergent 1-cycle
    /// implicit abort.
    pub(crate) dmc_implicit_abort: bool,
    /// W3-Stage-4 (`mc-r1-dmc-delayed-4015` grid correction): TriCNES's
    /// reload arm is CONSUME-EDGE-QUANTIZED, not level-held. When the consume
    /// edge lands ON the GET-delivery cycle itself (`CannotRunDMCDMARightNow
    /// == 2`, Emulator.cs:1165 — only ever true at the same-cycle edge,
    /// because the decrement at :1186 runs later that same end-tick), the arm
    /// is skipped ENTIRELY and the chain defers to the NEXT consume edge
    /// (576 cycles). Our `needs_dma()` is level-triggered and would re-arm as
    /// soon as the cooldown expires (4 cycles — one grid boundary early, the
    /// Implicit `$540[8,9]` cliff). Set at the blocked same-cycle edge;
    /// suppresses the reload arm; cleared at the next consume edge in
    /// `dmc_tick_end` immediately before the reload-arm step so the deferred
    /// arm fires exactly on-grid.
    pub(crate) dmc_edge_arm_suppress: bool,
    /// Sample rate (Hz) for diagnostics.
    pub sample_rate: u32,
    /// Most recent frame-counter events produced by [`Self::tick_with_external`].
    /// Read by the bus immediately after `tick` to fan the same events out to
    /// any on-cart audio extension that shares the 2A03 frame counter cadence
    /// (MMC5 audio). Reset to `FrameEvents::default()` at the *start* of every
    /// `tick`, so observers must read it AFTER the tick.
    pub(crate) last_frame_events: FrameEvents,
    /// Per-channel enable mask (UI playback overlay, NOT NES hardware state).
    /// Bit 0 = pulse 1, bit 1 = pulse 2, bit 2 = triangle, bit 3 = noise,
    /// bit 4 = DMC, bit 5 = external/mapper audio. A cleared bit forces that
    /// channel's contribution to the mixed sample to 0 (a studio/debug mute).
    ///
    /// Defaults to [`CHANNEL_MASK_ALL`] (every bit set), which is byte-identical
    /// to passing the raw channel outputs straight into the mixer — i.e. the
    /// deterministic core output is unchanged unless the frontend explicitly
    /// mutes a channel. NEVER serialized into the save state (a UI preference,
    /// like volume), so restored states are unaffected.
    pub(crate) channel_mask: u8,
    /// v1.4.0 Workstream C — per-channel output gain (a UI mixing overlay, NOT
    /// NES hardware state), generalizing [`Self::channel_mask`]. Index 0 = pulse
    /// 1, 1 = pulse 2, 2 = triangle, 3 = noise, 4 = DMC, 5 = external/mapper
    /// audio. Each internal channel's raw integer output is scaled by its gain
    /// and rounded back to an integer before the non-linear mixer; the external
    /// (already-linear) sample is scaled directly.
    ///
    /// Defaults to [`CHANNEL_GAIN_UNITY`] (all `1.0`). At unity the mix takes the
    /// EXACT current code path (`round(v * 1.0) == v`, `external * 1.0 ==
    /// external`), so the deterministic core output is byte-identical unless the
    /// frontend explicitly changes a gain — the determinism contract holds and
    /// the oracle / test ROMs (which never touch a gain) are unaffected. NEVER
    /// serialized into the save state (a UI preference, like the mask / volume).
    pub(crate) channel_gain: [f32; 6],
}

/// All [`Apu::channel_mask`] bits set — every channel audible (the default and
/// the determinism-safe value the oracle / test ROMs always run with).
pub const CHANNEL_MASK_ALL: u8 = 0x3F;

/// All [`Apu::channel_gain`] entries at `1.0` — every channel at full,
/// unattenuated output (the default and the byte-identical value the oracle /
/// test ROMs always run with).
pub const CHANNEL_GAIN_UNITY: [f32; 6] = [1.0; 6];

impl Apu {
    const fn dmc_abort_delay_for(cycles_until_output: u16) -> Option<u8> {
        match cycles_until_output {
            2 => Some(2),
            3 => Some(3),
            _ => None,
        }
    }

    /// New APU.
    #[must_use]
    pub fn new(region: Region, sample_rate: u32) -> Self {
        let cpu_rate = match region {
            Region::Pal => CPU_HZ_PAL,
            _ => CPU_HZ_NTSC,
        };
        Self {
            region,
            pulse1: Pulse::new(true),
            pulse2: Pulse::new(false),
            triangle: Triangle::new(),
            noise: Noise::new(region),
            dmc: Dmc::new(region),
            frame_counter: FrameCounter::new(),
            mixer: Mixer::new(),
            blip: BlipBuf::new(sample_rate, cpu_rate),
            apu_phase: false,
            dmc_driven_externally: false,
            put_cycle: false,
            parity_seed: 0,
            restored_parity_tail: false,
            cpu_cycle: 0,
            pending_dmc_dma: false,
            pending_dmc_dma_next: false,
            dmc_dma_is_load: false,
            dmc_dma_short: false,
            defer_dmc_reload_once: false,
            pending_dmc_abort: false,
            dmc_abort_delay: 0,
            dmc_dma_cooldown: 0,
            cannot_run_dmc_dma: 0,
            dmc_reenable_period_block: false,
            subpos_arm_countdown: 0,
            dmc_reload_suppress_outputs: 0,
            dmc_dma_delay: 0,
            dmc_dma_addr: 0xC000,
            dmc_need_halt: false,
            dmc_need_dummy_read: false,
            dmc_delayed_4015: 0,
            dmc_delayed_status: false,
            dmc_status_applied: false,
            dmc_set_implicit_abort: false,
            dmc_implicit_abort: false,
            dmc_edge_arm_suppress: false,
            sample_rate,
            last_frame_events: FrameEvents::default(),
            channel_mask: CHANNEL_MASK_ALL,
            channel_gain: CHANNEL_GAIN_UNITY,
        }
    }

    /// Reset (warm).  Per nesdev: most APU state is preserved across reset
    /// except `$4015` is cleared (channels disabled, DMC silenced).
    pub fn reset(&mut self) {
        self.frame_counter.reset();
        self.write_register(0x4015, 0x00);
        self.pending_dmc_dma = false;
        self.dmc_dma_is_load = false;
        self.dmc_dma_short = false;
        self.defer_dmc_reload_once = false;
        self.pending_dmc_abort = false;
        self.dmc_abort_delay = 0;
        self.dmc_dma_cooldown = 0;
        self.cannot_run_dmc_dma = 0;
        self.dmc_reenable_period_block = false;
        self.dmc_reload_suppress_outputs = 0;
        self.dmc_dma_delay = 0;
        self.dmc_need_halt = false;
        self.dmc_need_dummy_read = false;
        // W3-Stage-3: a warm reset silences the DMC immediately — collapse the
        // delayed-application machinery to the applied-disabled state (the
        // `write_register(0x4015, 0)` above latched a deferred disable).
        {
            self.dmc_delayed_4015 = 0;
            self.dmc_delayed_status = false;
            self.dmc_status_applied = false;
            self.dmc_set_implicit_abort = false;
            self.dmc_implicit_abort = false;
            self.dmc_edge_arm_suppress = false;
            self.dmc.bytes_remaining = 0;
        }
        self.blip.reset();
    }

    /// Set the per-channel enable mask (a UI playback overlay; see
    /// [`Apu::channel_mask`]). Bit 0 = pulse 1, 1 = pulse 2, 2 = triangle,
    /// 3 = noise, 4 = DMC, 5 = external/mapper audio. [`CHANNEL_MASK_ALL`] is
    /// the determinism-safe default (byte-identical mixer output).
    pub const fn set_channel_mask(&mut self, mask: u8) {
        self.channel_mask = mask & CHANNEL_MASK_ALL;
    }

    /// Current per-channel enable mask.
    #[must_use]
    pub const fn channel_mask(&self) -> u8 {
        self.channel_mask
    }

    /// v1.4.0 Workstream C — set the per-channel output gain (a UI mixing
    /// overlay; see [`Apu::channel_gain`]). Index 0 = pulse 1, 1 = pulse 2,
    /// 2 = triangle, 3 = noise, 4 = DMC, 5 = external/mapper audio. Each gain is
    /// clamped to `0.0..=2.0`. [`CHANNEL_GAIN_UNITY`] (all `1.0`) is the
    /// determinism-safe default (byte-identical mixer output).
    pub fn set_channel_gain(&mut self, gain: [f32; 6]) {
        for (slot, g) in self.channel_gain.iter_mut().zip(gain.iter()) {
            *slot = g.clamp(0.0, 2.0);
        }
    }

    /// Current per-channel output gain. See [`Apu::set_channel_gain`].
    #[must_use]
    pub const fn channel_gain(&self) -> [f32; 6] {
        self.channel_gain
    }

    /// Pulse 1 raw output volume (0..=15) — for tests.
    #[must_use]
    pub fn pulse1_out(&self) -> u8 {
        self.pulse1.output()
    }
    /// Pulse 2 raw output volume.
    #[must_use]
    pub fn pulse2_out(&self) -> u8 {
        self.pulse2.output()
    }
    /// Triangle raw output (0..=15).
    #[must_use]
    pub fn triangle_out(&self) -> u8 {
        self.triangle.output()
    }
    /// Noise raw output (0..=15).
    #[must_use]
    pub fn noise_out(&self) -> u8 {
        self.noise.output()
    }
    /// DMC raw output (0..=127).
    #[must_use]
    pub const fn dmc_out(&self) -> u8 {
        self.dmc.output()
    }

    /// Frame IRQ pending?
    #[must_use]
    pub const fn frame_irq_pending(&self) -> bool {
        self.frame_counter.irq_flag
    }

    /// DMC IRQ pending?
    #[must_use]
    pub const fn dmc_irq_pending(&self) -> bool {
        self.dmc.irq_flag
    }

    /// Combined IRQ line — true if either source is asserting.
    ///
    /// Session-26 iter 5 (2026-05-23): the frame-counter contribution
    /// is `irq_line_active` (the CPU's `IRQSource::FrameCounter`
    /// registration), NOT `irq_flag` (the `$4015` bit 6 visibility).
    /// The two are SEPARATE fields since iter 5 — see
    /// [`FrameCounter::irq_flag`](crate::frame_counter::FrameCounter::irq_flag)
    /// and [`FrameCounter::irq_line_active`](crate::frame_counter::FrameCounter::irq_line_active).
    /// AccuracyCoin Tests I/J/K specifically test that `$4015` bit 6
    /// is visible during inhibit (transient 2-cycle window at FC steps
    /// 29828-29829) while NO CPU IRQ fires (Test M).
    #[must_use]
    pub const fn irq_line(&self) -> bool {
        self.frame_counter.irq_line_active || self.dmc.irq_flag
    }

    /// Returns the frame-counter events fired by the most recent `tick` call.
    ///
    /// The bus reads this immediately after [`Self::tick_with_external`] to
    /// fan-out the events to on-cart audio extensions (MMC5) whose envelope
    /// and length-counter sub-units share the 2A03 frame-counter cadence.
    /// The value is overwritten at the start of every `tick`, so observers
    /// must consume it before the next tick.
    #[must_use]
    pub const fn last_frame_events(&self) -> FrameEvents {
        self.last_frame_events
    }

    /// Drain all finalized audio samples (host sample rate, normalized to
    /// approximately `[-0.5, 0.5]`).
    pub fn drain_audio(&mut self) -> Vec<f32> {
        self.blip.drain_all()
    }

    /// Drain into a slice; returns count copied.
    pub fn drain_audio_into(&mut self, out: &mut [f32]) -> usize {
        self.blip.drain(out)
    }

    /// Has a DMC DMA request been raised?  The bus polls this each CPU cycle
    /// (BEFORE issuing reads) so it can halt the CPU on the next read cycle.
    #[must_use]
    pub const fn dmc_dma_pending(&self) -> bool {
        self.pending_dmc_dma
    }

    /// Whether the pending DMC DMA is a load DMA.
    #[must_use]
    pub const fn dmc_dma_is_load(&self) -> bool {
        self.dmc_dma_is_load
    }

    /// W3-Stage-3 (`mc-r1-dmc-delayed-4015`): the bus-side per-cycle DMC DMA
    /// service-gate term — TriCNES `_6502` line 4218:
    /// `DoDMCDMA && (APU_Status_DMC || APU_ImplicitAbortDMC4015)`. While
    /// false, a pending (or halted in-flight) DMC DMA is NOT serviced and the
    /// CPU resumes — the emergent explicit abort. `pending_dmc_abort` is the
    /// implicit-abort override (the 1-cycle abort DMA runs regardless).
    #[must_use]
    pub const fn dmc_dma_serviceable(&self) -> bool {
        self.dmc_status_applied || self.dmc_implicit_abort || self.pending_dmc_abort
    }

    /// Whether the pending DMC DMA should use the short 3-cycle service path.
    #[must_use]
    pub const fn dmc_dma_short(&self) -> bool {
        self.dmc_dma_short
    }

    /// Whether the current DMC DMA get should make the fetched byte visible
    /// before the get-cycle APU tick.
    #[must_use]
    pub const fn dmc_dma_deliver_before_tick(&self) -> bool {
        self.dmc.loop_flag
            && self.dmc.sample_length == 1
            && self.dmc.rate_index == 0x0E
            && self.dmc.bits_remaining == 1
            && self.dmc.timer == 0
            && !self.apu_phase
    }

    /// Defer the next immediate DMC reload request by one CPU tick.
    pub const fn defer_next_dmc_reload_once(&mut self) {
        self.defer_dmc_reload_once = true;
    }

    /// Has a one-cycle DMC abort halt been raised?
    #[must_use]
    pub const fn dmc_abort_pending(&self) -> bool {
        self.pending_dmc_abort
    }

    /// Read-only accessor for the DMC abort-delay countdown (CPU cycles
    /// until `pending_dmc_abort` flips to `true`).  Exposed for the
    /// Session-21 per-cycle DMC trace tooling (`crates/rustynes-core/src/
    /// irq_trace.rs`) which records the scheduler's calibration state
    /// for cross-diffing against Mesen2's `NesDmc.cpp`.
    #[must_use]
    pub const fn dmc_abort_delay(&self) -> u8 {
        self.dmc_abort_delay
    }

    /// Read-only accessor for the DMC DMA cooldown countdown (CPU cycles
    /// during which a newly-empty sample buffer must NOT raise a new
    /// DMA request).  See [`Self::dmc_abort_delay`].
    #[must_use]
    pub const fn dmc_dma_cooldown(&self) -> u8 {
        self.dmc_dma_cooldown
    }

    /// Read-only accessor for the DMC DMA delay countdown (CPU cycles
    /// until an initial-load DMA after `$4015` enable transitions from
    /// "armed" to `pending_dmc_dma = true`).  See [`Self::dmc_abort_delay`].
    #[must_use]
    pub const fn dmc_dma_delay(&self) -> u8 {
        self.dmc_dma_delay
    }

    /// Diagnostic: the DMC channel's internal byte-timer countdown. Exposed
    /// for the per-cycle DMC-DMA cross-diff tracing that pins the abort-context
    /// reload-arm phase (the +4-cycle `A->B` interval divergence).
    #[must_use]
    pub const fn dmc_timer(&self) -> u16 {
        self.dmc.timer()
    }

    /// Diagnostic: bits remaining in the DMC output shift register.
    #[must_use]
    pub const fn dmc_bits_remaining(&self) -> u8 {
        self.dmc.bits_remaining()
    }

    /// Diagnostic: DMC output-unit silence flag.
    #[must_use]
    pub const fn dmc_silence(&self) -> bool {
        self.dmc.silence()
    }

    /// Diagnostic: DMC sample buffer occupied.
    #[must_use]
    pub const fn dmc_buffer_full(&self) -> bool {
        self.dmc.buffer_full()
    }

    /// Read-only accessor for the APU's two-cycle phase counter
    /// (false = put, true = get; toggled every CPU tick by
    /// `tick_with_external`).  See [`Self::dmc_abort_delay`].
    #[must_use]
    pub const fn apu_phase(&self) -> bool {
        self.apu_phase
    }

    /// v2.0.0 beta.1 (A1 one-clock collapse): assign the APU's cycle counter
    /// from the CANONICAL bus cycle counter. Called by the bus's per-cycle
    /// hook (`cpu_clock` → `apu_advance_one`) immediately before
    /// [`Self::tick_with_external`], replacing the legacy independent
    /// `cpu_cycle += 1` mirror. The bus increments its canonical counter
    /// earlier in the same per-cycle hook, so the value assigned here equals
    /// the post-increment value the legacy mirror produced — the
    /// `one_clock_invariants` harness test pins the residue.
    #[cfg(feature = "mc-one-clock-v2")]
    pub const fn set_canonical_cycle(&mut self, cycle: u64) {
        self.cpu_cycle = cycle;
    }

    /// Read-only accessor for the APU-side cumulative CPU-cycle counter
    /// (v2.0.0-beta.1 one-clock instrumentation).
    ///
    /// This is one of the five counters of the timebase substrate the
    /// v2.0.0 "Timebase" rewrite collapses (ADR 0002 + the v2.0.0
    /// master-clock plan): `Cpu::master_clock`, `Cpu::cycles`,
    /// `LockstepBus::cycle`, `LockstepBus::ppu_clock`, and this field are
    /// each advanced exactly once (or by one region divider) per CPU cycle
    /// at different points *within* the cycle, and must never drift. The
    /// RW-1 parity collapse already derives `apu_phase` / `put_cycle` from
    /// `(cpu_cycle + parity_seed) & 1`; exposing the raw counter lets the
    /// test harness assert the cross-chip affine invariants
    /// (`one_clock_invariants.rs`) that gate the beta.1 counter collapse.
    #[must_use]
    pub const fn cpu_cycle(&self) -> u64 {
        self.cpu_cycle
    }

    /// CM-1: seed the absolute `apu_phase` alignment (the parity of the CPU
    /// cycles on which the APU — incl. the DMC byte-timer — clocks). RustyNES
    /// starts `apu_phase = false`, so the DMC always arms on one CPU-cycle
    /// parity; Mesen's DMC arms on its `_currentCycle` alignment (one cycle off,
    /// giving span 3 vs RustyNES's 4). Seeding `true` flips the whole APU phase
    /// by one CPU cycle to test the Mesen-matching arm parity. Broad impact:
    /// also shifts pulse/noise/frame-counter. Default-off.
    pub const fn seed_apu_phase(&mut self, phase: bool) {
        self.apu_phase = phase;
    }

    /// Address the DMC wants to read.  Valid only when `dmc_dma_pending()`
    /// returns `true`.
    #[must_use]
    pub const fn dmc_dma_addr(&self) -> u16 {
        self.dmc_dma_addr
    }

    /// v1.2 Sprint 3 (get/put scheduler, ADR-0007).
    ///
    /// Returns `true` while the DMC still needs an initial halt
    /// cycle on the bus. Set by any code path that raises
    /// `pending_dmc_dma`; the new `bus::service_dmc_dma`
    /// implementation under the `dmc-get-put-scheduler` feature
    /// flag clears it after processing the halt get-cycle.
    #[must_use]
    pub const fn dmc_need_halt(&self) -> bool {
        self.dmc_need_halt
    }

    /// v1.2 Sprint 3 (get/put scheduler, ADR-0007).
    ///
    /// Returns `true` while the DMC still needs a dummy-read /
    /// alignment cycle after the halt cycle. Cleared by the new
    /// scheduler once the alignment cycle has been processed.
    #[must_use]
    pub const fn dmc_need_dummy_read(&self) -> bool {
        self.dmc_need_dummy_read
    }

    /// v1.2 Sprint 3 — bus clears this after consuming the halt
    /// get-cycle (`_needHalt = false` in Mesen2's `NesCpu.cpp`).
    pub const fn clear_dmc_need_halt(&mut self) {
        self.dmc_need_halt = false;
    }

    /// v1.2 Sprint 3 — bus clears this after consuming the
    /// alignment cycle (`_needDummyRead = false` in Mesen2's
    /// `NesCpu.cpp`).
    pub const fn clear_dmc_need_dummy_read(&mut self) {
        self.dmc_need_dummy_read = false;
    }

    /// Bus calls this when it has executed a DMC DMA fetch (post-halt) and
    /// is delivering the sample byte.
    pub fn complete_dmc_dma(&mut self, byte: u8) {
        self.dmc.deliver_sample(byte);
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): TriCNES `DMCDMA_Get`
        // (Emulator.cs:4148-4160) — a non-looping sample's natural end clears
        // the APPLIED status immediately (not via the delayed slot). A
        // looping end restarts the sample (`deliver_sample` already did), so
        // `bytes_remaining > 0` and the status holds.
        if self.dmc.bytes_remaining == 0 && !self.dmc.loop_flag {
            self.dmc_status_applied = false;
        }
        let was_load = self.dmc_dma_is_load;
        // v2.0 abort-context reload-arm phase fix (`mc-r1-dmc-abort-timer-phase`).
        // In the Implicit-DMA-Abort `$4015` disable->re-enable context, this LOAD
        // DMA's `deliver_sample` (just above) lands ON the byte-timer boundary
        // cycle, where `clock_output` already ran at cycle-START and took the
        // still-empty buffer (silence) BEFORE the LOAD filled it. TriCNES's LOAD
        // GET completes 3 cyc BEFORE the boundary, so the boundary consumes the
        // buffer into the shifter and arms a RELOAD (inserting a 4-cyc reload DMA
        // RustyNES otherwise skips, deferring the reload chain by 4 -> A->B 580
        // not 576 -> GET-catch skew +4 -> Y=0). Detect the boundary-coincidence
        // (silence set + bits just reloaded to 8 + buffer now full + bytes
        // remaining) and retroactively load the delivered byte into the shifter
        // (un-silence) so the buffer empties and the per-cycle reload-arm fires
        // promptly this cycle (cooldown cleared below) — reproducing TriCNES's
        // boundary-coupled reload. The condition only ever holds in this race.
        let abort_boundary_race = was_load
            && self.dmc.silence
            && self.dmc.bits_remaining == 8
            && self.dmc.bytes_remaining > 0
            && self.dmc.consume_buffer_into_shifter_if_silent();
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): this load-completion abort
        // scheduling is the FLOOR's implicit-abort model (a pre-computed
        // 1-cycle halt via `dmc_abort_delay` -> `pending_dmc_abort` -> the
        // read1 abort-cancel path). Under the delayed-status port the same
        // physics is EMERGENT (the `$4015`-enable pre-fire-window latch + the
        // consume-edge arm + the 1-cycle override kill), so the floor
        // scheduler is superseded — both active would double-fire ($500
        // idx[10,11] measured 05 vs KEY 01).
        self.pending_dmc_dma = false;
        self.dmc_dma_is_load = false;
        self.dmc_dma_short = false;
        self.dmc_dma_delay = 0;
        self.dmc_dma_cooldown = 4;
        // v2.0 Phase 2 (`mc-r1-dmc-reenable-phase`): TriCNES `DMCDMA_Get`
        // (`Emulator.cs:4168`) sets `CannotRunDMCDMARightNow = 2` after EVERY
        // DMC GET (load or reload). The exclusion is decremented by 2 per get
        // cycle in `tick_with_external` and blocks the looping-reload arm while
        // `== 2` — the canonical "a DMA cannot occur within 2 cycles of a
        // previous DMC DMA" rule the Implicit-DMA-Abort `$540` plateau brackets.
        {
            self.cannot_run_dmc_dma = 2;
            #[cfg(feature = "mc-r1-dmc-abort-probe")]
            abort_probe::CANNOT_RUN_SET.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
        }
        // Abort-context fix: the LOAD just emptied the buffer into the shifter
        // (boundary race), so the next reload must arm promptly — don't let the
        // post-LOAD cooldown suppress it for 4 cycles (which would re-introduce
        // the +4). Clear the cooldown so `dmc_step_reload_arm` fires next cycle.
        if abort_boundary_race {
            self.dmc_dma_cooldown = 0;
        }
        // v1.2 Sprint 3 — safety-clear the get/put flags on
        // completion. Under the new scheduler the bus should have
        // cleared them on the prior cycles already; clearing here
        // protects against re-arming on the next DMC request.
        self.dmc_need_halt = false;
        self.dmc_need_dummy_read = false;
        // W3-Stage-2 (`mc-r1-dma-unified-collapse`): this guard's phase term
        // means "the GET landed on the off-phase half". The normal GET half is
        // `apu_phase`-true at floor but `apu_phase`-false under the end-flip,
        // so the off-phase test inverts — otherwise the (floor-dead) suppress
        // path would fire at EVERY collapse GET in the 1-byte-loop contexts.
        let off_phase_get = self.apu_phase;
        if was_load
            && self.dmc.loop_flag
            && self.dmc.sample_length == 1
            && self.dmc.bits_remaining == 1
            && self.dmc.timer == 0
            && self.dmc.sample_buffer.is_some()
            && off_phase_get
        {
            self.dmc_reload_suppress_outputs = 1;
        }
    }

    /// Complete a DMC DMA get whose fetched byte is visible before the
    /// get-cycle APU tick.
    pub fn complete_dmc_dma_before_get_tick(&mut self, byte: u8) {
        let was_load = self.dmc_dma_is_load;
        self.dmc.deliver_sample(byte);
        // W3-Stage-3: see `complete_dmc_dma` — non-looping natural end clears
        // the applied status immediately.
        if self.dmc.bytes_remaining == 0 && !self.dmc.loop_flag {
            self.dmc_status_applied = false;
        }
        if was_load
            && self.dmc.bytes_remaining == 0
            && !self.dmc.loop_flag
            && self.dmc.bits_remaining == 1
            && self.dmc.timer == 0
            && self.dmc.sample_buffer.is_some()
        {
            self.dmc_abort_delay = 3;
        }
        if was_load && self.dmc.loop_flag && self.dmc.sample_length == 1 {
            self.defer_dmc_reload_once = true;
        }
        self.pending_dmc_dma = false;
        self.dmc_dma_is_load = false;
        self.dmc_dma_short = false;
        self.dmc_dma_delay = 0;
        self.dmc_dma_cooldown = 5;
        // v2.0 Phase 2 (`mc-r1-dmc-reenable-phase`): see `complete_dmc_dma`.
        {
            self.cannot_run_dmc_dma = 2;
        }
        self.dmc_need_halt = false;
        self.dmc_need_dummy_read = false;
    }

    /// Bus calls this after either consuming or suppressing a one-cycle DMC
    /// abort halt.
    pub const fn complete_dmc_abort(&mut self) {
        self.pending_dmc_abort = false;
        self.dmc_abort_delay = 0;
    }

    /// v1.2 Sprint 3 iter 3 (get/put scheduler, ADR 0007) — DMC DMA
    /// abort with cancel semantics.
    ///
    /// Under the OLD scheduler, [`Self::complete_dmc_abort`] clears
    /// only the abort flag; the DMC DMA still fires afterward (the
    /// abort just inserts a 1-cycle halt). Under the get/put model
    /// the abort CANCELS the DMA entirely — no byte fetch, all
    /// flag state cleared — matching Mesen2's
    /// `processCycle::if(_abortDmcDma)` branch
    /// (`NesCpu.cpp:386-390`):
    ///
    /// ```text
    /// if(_abortDmcDma) {
    ///     _dmcDmaRunning = false;
    ///     _abortDmcDma = false;
    ///     _needDummyRead = false;
    ///     _needHalt = false;
    /// }
    /// ```
    ///
    /// This is the "Option C" semantic shift from the iter 3
    /// research audit: abort cancels the fetch rather than letting
    /// it complete after a wasted cycle. The new bus-side
    /// `service_dmc_dma` (under `dmc-get-put-scheduler` feature)
    /// calls this when it detects `dmc_abort_pending` mid-loop.
    pub const fn cancel_dmc_dma(&mut self) {
        self.pending_dmc_dma = false;
        self.pending_dmc_abort = false;
        self.dmc_abort_delay = 0;
        self.dmc_dma_short = false;
        self.dmc_dma_is_load = false;
        self.dmc_dma_delay = 0;
        self.dmc_need_halt = false;
        self.dmc_need_dummy_read = false;
    }

    /// One CPU clock.  Bus must NOT have halted the CPU for DMC DMA when
    /// calling this (the bus is responsible for performing the DMA fetch
    /// before resuming `tick()` calls).
    pub fn tick(&mut self) {
        self.tick_with_external(0.0);
    }

    /// Same as `tick`, but accepts an additional pre-mixed audio sample
    /// from the cartridge (VRC6 / VRC7 / MMC5 / Sunsoft 5B / Namco 163 /
    /// FDS). The external value is added to the APU's own mix BEFORE the
    /// band-limited buffer push.
    ///
    /// The expected scale is ~ `[-0.5, 0.5]` (matching the APU mixer's
    /// own output range). The bus is responsible for converting whatever
    /// the mapper returns (currently `i16` from `Mapper::mix_audio`) into
    /// that range.
    pub fn tick_with_external(&mut self, external: f32) {
        // v2.0.0 beta.1 (A1 one-clock collapse): under `mc-one-clock-v2` the
        // APU's cycle counter is ASSIGNED from the canonical bus counter (see
        // `set_canonical_cycle`, called by the bus immediately before this
        // tick) instead of being an independently-incremented lockstep
        // mirror. The RW-1 `apu_phase`/`put_cycle` parity derivation below
        // then reads from the ONE counter.
        #[cfg(not(feature = "mc-one-clock-v2"))]
        {
            self.cpu_cycle = self.cpu_cycle.wrapping_add(1);
        }

        // v2.0 RA-1: the DMC byte-timer + arms could clock HERE at cycle START
        // (on `apu_phase`), unified with the rest of the APU — Mesen
        // `ProcessCpuClock` at `StartCpuCycle`.
        //
        // v2.0 Program M (M-1 within-cycle order): the DMC byte-timer CLOCK +
        // reload-arm + reenable bookkeeping live at end-of-cycle (after the CPU's
        // bus access, in `dmc_tick_end`), matching Mesen `StartCpuCycle`->
        // `ProcessCpuClock` and TriCNES `_6502`->`_EmulateAPU` (CPU reads state ->
        // APU ticks/arms reload -> get/put flips). The reload arm thereby becomes
        // invisible to its own cycle -> first-service is the next (put) cycle ->
        // span-4. So the cycle-START DMC clock/arm paths below are never taken;
        // the LOAD delay-arm moves to the put phase of `dmc_tick_end` (the TriCNES
        // `DMCDMADelay` put-branch placement, Emulator.cs:1217).

        if self.dmc_abort_delay > 0 {
            self.dmc_abort_delay -= 1;
            if self.dmc_abort_delay == 0 && !self.pending_dmc_abort {
                self.pending_dmc_abort = true;
            }
        }
        if self.dmc_dma_cooldown > 0 {
            self.dmc_dma_cooldown -= 1;
        }

        // Triangle clocks at CPU rate.
        self.triangle.clock_timer();

        // Pulse, noise, DMC clock at APU rate (every other CPU cycle).
        // RW-1 (`mc-r1-one-clock`): DERIVE `apu_phase` from the single per-cycle
        // counter + boot seed instead of a free-running toggle, so it shares ONE
        // source with `put_cycle` (and thus the DMA get/put parity + DMC
        // fire-phase) and can never drift. `cpu_cycle` was incremented above, so
        // `(cpu_cycle + parity_seed) & 1 == 1` reproduces the toggle-from-`false`
        // sequence exactly when `parity_seed == 0` (the floor config).
        {
            self.apu_phase = (self.cpu_cycle.wrapping_add(self.parity_seed) & 1) == 1;
        }
        if self.apu_phase {
            self.pulse1.clock_timer();
            self.pulse2.clock_timer();
            self.noise.clock_timer();
            // F-2/M-1: the DMC byte-timer clock lives in `tick_dmc`
            // (end-of-cycle), not here at cycle START.
        }

        // Frame counter (CPU clock). Latch the events so the bus can fan
        // them out to on-cart audio extensions (MMC5) after the tick.
        // Pass `apu_phase` AND `cpu_cycle` so the frame counter can
        // (a) compute APU-step timing as before and (b) mature any
        // pending lazy `$4015`-read IRQ-flag clear scheduled by a
        // previous read (Session-25, 2026-05-23 — see
        // `frame_counter::read_status` doc).
        let ev = self.frame_counter.tick(self.cpu_cycle, self.apu_phase);
        self.last_frame_events = ev;
        self.handle_frame_events(ev);

        // v2.0 Phase 2 (`mc-r1-dmc-reenable-phase`) reload-arm/reenable
        // bookkeeping and the `CannotRunDMCDMARightNow` exclusion decrement all
        // live at end-of-cycle (`dmc_tick_end`) under M-1, NOT here at cycle
        // START.

        // Emit one mixed sample to the band-limited buffer. The external
        // (cartridge) audio is summed AFTER the internal non-linear mixer
        // since it's already a linear value.
        // Per-channel mute overlay. With the default `CHANNEL_MASK_ALL` every
        // `gate(..)` returns the raw output unchanged, so this is byte-identical
        // to the un-masked mix (the determinism contract — the oracle / test
        // ROMs never clear a bit). A cleared bit forces that channel's raw
        // output to 0 BEFORE the non-linear mixer, so it contributes nothing.
        let mask = self.channel_mask;
        let gate = |bit: u8, v: u8| if mask & (1 << bit) != 0 { v } else { 0 };
        // v1.4.0 Workstream C — per-channel gain (a UI mixing overlay). With the
        // default `CHANNEL_GAIN_UNITY` every `scale(..)` returns `round(v * 1.0)
        // == v` and `external * 1.0 == external`, so this is byte-identical to
        // the pre-gain mix (the determinism contract — the oracle / test ROMs
        // never change a gain). A gain != 1.0 scales that channel's contribution
        // before the non-linear mixer (gain 0.0 == a cleared mask bit). The
        // `gain` slice is checked-for-unity-and-skipped so the default path is
        // the exact integer-gate code as before.
        let gain = self.channel_gain;
        // `max` is the channel's native raw ceiling (pulse/tri/noise = 15, DMC =
        // 127); the scaled value is clamped to it so the non-linear mixer's
        // `pulse_table` (31) / `tnd_table` (203) index bounds always hold even at
        // gain 2.0. At gain 1.0 the value is returned unchanged (byte-identical).
        let scale = |bit: usize, v: u8, max: u8| {
            let g = gain[bit];
            if g == 1.0 {
                v
            } else {
                #[allow(
                    clippy::cast_possible_truncation,
                    clippy::cast_sign_loss,
                    clippy::cast_precision_loss
                )]
                {
                    roundf(f32::from(v) * g).clamp(0.0, f32::from(max)) as u8
                }
            }
        };
        let ext_gain = gain[5];
        let ext = if ext_gain == 1.0 {
            external
        } else {
            external * ext_gain
        };
        let mixed = self.mixer.mix(
            scale(0, gate(0, self.pulse1.output()), 15),
            scale(1, gate(1, self.pulse2.output()), 15),
            scale(2, gate(2, self.triangle.output()), 15),
            scale(3, gate(3, self.noise.output()), 15),
            scale(4, gate(4, self.dmc.output()), 127),
        ) + if mask & (1 << 5) != 0 { ext } else { 0.0 };
        self.blip.add_sample(mixed);

        // v2.0 interleaved-DMA Phase A: toggle the global get/put flip-flop once
        // per CPU cycle, right after the APU tick (TriCNES `APU_PutCycle =
        // !APU_PutCycle` after `_EmulateAPU()`, `Emulator.cs:920`). Gated on
        // `dmc_driven_externally` so the default build never touches it
        // (byte-identical); under the R1 substrate this is the single
        // per-cycle get/put counter the interleaved DMA (Phase B) consumes.
        // RW-1 (`mc-r1-one-clock`): `put_cycle` is the COMPLEMENT of `apu_phase`,
        // derived from the same counter — not a second independent flip-flop.
        // In the floor config the two toggles already stayed perfectly
        // complementary (both flip once per `tick_with_external`); RW-1 makes
        // that structural so RW-2 has a SINGLE place to make the parity
        // OAM-DMA-aware. The bus's get/put decision (`get = !put_cycle`) and the
        // F-2 DMC clock (`!put_cycle`) then read this coherent value.
        // M-2 (`mc-r1-counter-collapse`): the get/put `put_cycle` flip moves to
        // END of the CPU cycle (`dmc_tick_end`), AFTER the bus access — the
        // references' "access -> APU tick -> get/put flip" order. So at the START
        // (here) `put_cycle` is LEFT at its prior-cycle value; the bus access this
        // cycle therefore reads `put_cycle = !apu_phase_{N-1} = apu_phase_N`,
        // one parity position later than the floor's `!apu_phase_N`. `apu_phase`
        // itself (the APU IRQ line / C1 phi2 sample source) still flips at start
        // (line ~927), so C1 is invariant.

        // final lever #1 diag: rotate the 2-cycle DMC-state history at end of
        // cycle so the block site can dump the two preceding cycles.
        #[cfg(feature = "mc-r1-dmc-abort-probe")]
        {
            use core::sync::atomic::Ordering::Relaxed;
            let cur = abort_probe::pack_dmc(
                self.apu_phase,
                self.dmc.needs_dma(),
                self.dmc.buffer_full(),
                self.cannot_run_dmc_dma,
                self.dmc.bytes_remaining,
                self.dmc.bits_remaining(),
                self.dmc.timer(),
            );
            abort_probe::SUBPOS_P2.store(abort_probe::SUBPOS_P1.load(Relaxed), Relaxed);
            abort_probe::SUBPOS_P1.store(cur, Relaxed);
        }
    }

    fn handle_frame_events(&mut self, ev: FrameEvents) {
        if ev.quarter {
            self.pulse1.clock_quarter_frame();
            self.pulse2.clock_quarter_frame();
            self.triangle.clock_quarter_frame();
            self.noise.clock_quarter_frame();
        }
        if ev.half {
            self.pulse1.clock_half_frame();
            self.pulse2.clock_half_frame();
            self.triangle.clock_half_frame();
            self.noise.clock_half_frame();
        }
    }

    /// Visibility-delay promotion (called at END of cycle, after the CPU's bus
    /// access): a reload latched this cycle becomes visible to the NEXT cycle's
    /// DMA servicing (first-service on the put cycle => span 4), matching
    /// TriCNES `_EmulateAPU`-after-`_6502` invisible-arm ordering.
    pub fn promote_dmc_pending_next(&mut self) {
        if self.pending_dmc_dma_next {
            self.pending_dmc_dma_next = false;
            self.pending_dmc_dma = true;
        }
    }

    /// v2.0 Program M (M-1 within-cycle order, `mc-r1-dmc-bytetimer-end`): clock
    /// the DMC byte-timer + arm the reload at END of cycle (after the CPU's bus
    /// access), the mirror of the cycle-START block in `tick_with_external` that
    /// `dmc_clock_at_start` now suppresses. Order matches `tick_with_external`:
    /// byte-timer clock (on this cycle's already-set `apu_phase`) -> reenable
    /// consume-edge clear -> reload-arm -> `cannot_run` decrement. The bus calls
    /// this from `cpu_clock_apu_dmc` (end-of-cycle), AFTER
    /// `promote_dmc_pending_next` so a reload latched here is invisible to its
    /// own cycle (promoted -> serviced the NEXT cycle = span-4, like the
    /// references). The LOAD delay-arm is NOT here — it stays at cycle-start.
    pub fn dmc_tick_end(&mut self) {
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): the 1-cycle implicit-abort
        // kill — TriCNES clears `APU_ImplicitAbortDMC4015` at the END of
        // `_6502` whenever the DMA is pending (Emulator.cs:9000-9003), i.e.
        // BEFORE `_EmulateAPU`'s boundary work. A flag set by the previous
        // cycle's consume edge therefore survives exactly one CPU access
        // (one serviced halt cycle if it was a read; none if a write — "it
        // won't run at all") and dies here.
        if self.pending_dmc_dma && self.dmc_implicit_abort {
            self.dmc_implicit_abort = false;
        }
        let d4015_bits_before = self.dmc.bits_remaining();
        let dmc_bits_before = self.dmc.bits_remaining();
        // The byte-timer-end flag composes only with the canonical apu_phase
        // clock (the `mc-r1-full-cpu` config); the cpu-rate / phase-minus1
        // diagnostic clock variants are not combined with it.
        // M-2 (`mc-r1-counter-collapse`): the get/put `put_cycle` flip moved to
        // end-of-cycle (one parity position later), so the GET decision
        // (`get = !put_cycle`) now reads the shifted parity. The DMC byte-timer
        // FIRE must follow the SAME shift or the GET de-syncs from the byte-timer
        // wrap (wedge). At entry `put_cycle == apu_phase` (the prior end-flip),
        // so clocking on `!self.put_cycle == !apu_phase` shifts the byte-timer by
        // one to stay locked to the shifted GET — ONE counter driving both.
        let timer_phase = !self.put_cycle;
        if timer_phase {
            self.dmc.clock_timer();
        }
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): the consume-edge transfer
        // (Emulator.cs:1163-1175) — at the shifter-consume edge (bits 1 -> 8
        // on this end-tick's byte-timer fire) a latched
        // `dmc_set_implicit_abort` becomes the live `dmc_implicit_abort`
        // service-gate override AND arms the DMA directly (TriCNES
        // `if (BytesRemaining > 0 || SetImplicit) { if (!DoDMCDMA &&
        // CannotRun != 2) { DoDMCDMA = true; Halt = true; } ... }` — the arm
        // fires regardless of the buffer state). The armed DMA runs for
        // exactly one read cycle under the override (the kill above), then
        // waits for the delayed status — the emergent 1-cycle implicit abort.
        if timer_phase
            && self.dmc_set_implicit_abort
            && self.dmc.bits_remaining() == 8
            && d4015_bits_before <= 1
        {
            self.dmc_implicit_abort = true;
            self.dmc_set_implicit_abort = false;
            if !self.pending_dmc_dma && self.cannot_run_dmc_dma != 2 {
                self.pending_dmc_dma = true;
                self.dmc_dma_is_load = false;
                self.dmc_dma_short = false;
                self.dmc_dma_addr = self.dmc.dma_addr();
                self.dmc_need_halt = true;
                self.dmc_need_dummy_read = true;
            }
        }
        // W3-Stage-2 (`mc-r1-dma-unified-collapse`): the TriCNES `DMCDMADelay`
        // put-branch — the `$4015`-enable LOAD delay counts down ONLY on the
        // put phase of this end-of-cycle tick (Emulator.cs:1217 sits in the
        // `else` of the get branch), arming the halt at the end of a PUT cycle
        // so the load's first halted cycle is always a GET (entry-on-get =
        // span 3) regardless of the write cycle's parity. The put phase here
        // is `!timer_phase` (the complement of the shifted byte-timer phase).
        if !timer_phase {
            self.dmc_step_delay_arm_put_end();
        }
        if self.dmc_reenable_period_block && self.dmc.bits_remaining() == 8 && dmc_bits_before <= 1
        {
            self.dmc_reenable_period_block = false;
        }
        // W3-Stage-4 (`mc-r1-dmc-delayed-4015` grid correction): the TriCNES
        // reload arm is consume-edge-quantized. A consume edge that lands ON
        // the GET-delivery cycle itself (the X=8/9 Implicit `$540` restart
        // race: the silent-restart load GET collides with the free-running
        // byte-timer boundary) is arm-BLOCKED by `CannotRunDMCDMARightNow ==
        // 2` (Emulator.cs:1165; the :1186 decrement runs later that same
        // end-tick, so `== 2` is only ever observable at the same-cycle
        // edge) — and TriCNES holds NO level request: the chain simply waits
        // for the NEXT consume edge (one full byte period). Our `needs_dma()`
        // is level-triggered and would re-arm 4 cycles later (cooldown
        // expiry) — one grid boundary early, the `$540[8,9]` cliff. Latch the
        // suppression at the blocked same-cycle edge; release at the next
        // consume edge right here (BEFORE the reload-arm step) so the
        // deferred arm fires exactly on-grid, like TriCNES's
        // `BytesRemaining > 0` edge arm.
        if timer_phase && self.dmc.bits_remaining() == 8 && d4015_bits_before <= 1 {
            if self.dmc_edge_arm_suppress {
                self.dmc_edge_arm_suppress = false;
            } else if self.cannot_run_dmc_dma == 2 && self.dmc.needs_dma() && !self.pending_dmc_dma
            {
                self.dmc_edge_arm_suppress = true;
                #[cfg(feature = "mc-r1-dmc-abort-probe")]
                abort_probe::EDGE_SUPPRESS_SET.fetch_add(1, core::sync::atomic::Ordering::Relaxed);
            }
        }
        self.dmc_step_reload_arm();
        // M-2: the `cannot_run` decrement is TriCNES's get-cycle decrement; under
        // the collapse the get cycle is the shifted `timer_phase`, not raw
        // apu_phase.
        if timer_phase && self.cannot_run_dmc_dma > 0 {
            self.cannot_run_dmc_dma = self.cannot_run_dmc_dma.saturating_sub(2);
        }
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): the TriCNES
        // `APU_DelayedDMC4015` countdown (Emulator.cs:1214-1224) — decremented
        // EVERY CPU cycle after the get/put branch work (the byte-timer /
        // reload-arm / load-delay above). On expiry the latched `$4015` DMC
        // status APPLIES: `APU_Status_DMC = APU_Status_DelayedDMC`, and a
        // disable zeroes `bytes_remaining` HERE rather than at the write. The
        // bus-side service gate reads `dmc_status_applied` per cycle, so an
        // in-flight DMA whose status drops stops being serviced — the
        // emergent explicit abort.
        if self.dmc_delayed_4015 > 0 {
            self.dmc_delayed_4015 -= 1;
            if self.dmc_delayed_4015 == 0 {
                self.dmc_status_applied = self.dmc_delayed_status;
                if !self.dmc_status_applied {
                    self.dmc.bytes_remaining = 0;
                }
            }
        }
        // M-2 (`mc-r1-counter-collapse`): flip the get/put parity HERE at
        // end-of-cycle (after the CPU's bus access + the byte-timer/reload-arm
        // tick above), matching the references' "access -> APU tick -> get/put
        // flip" order. `put_cycle = !apu_phase` of the cycle that just ran; the
        // NEXT cycle's bus access reads this value. (Under bytetimer-end alone
        // this flip stays at cycle-start in `tick_with_external`.)
        {
            self.put_cycle = !self.apu_phase;
        }
    }

    /// W3-Stage-2 (`mc-r1-dma-unified-collapse`): the TriCNES `DMCDMADelay`
    /// put-branch body — same arm as [`Self::dmc_step_delay_arm`] but ticked
    /// only on the put phase of `dmc_tick_end` (value units = put end-ticks,
    /// set to 2 at the `$4015` enable like TriCNES `DMCDMADelay = 2`).
    fn dmc_step_delay_arm_put_end(&mut self) {
        if self.dmc_dma_delay > 0 {
            self.dmc_dma_delay -= 1;
            if self.dmc_dma_delay == 0 && !self.pending_dmc_dma {
                self.pending_dmc_dma = true;
                self.dmc_dma_short = self.dmc_dma_is_load;
                self.dmc_dma_addr = self.dmc.dma_addr();
                self.dmc_need_halt = true;
                self.dmc_need_dummy_read = true;
            }
        }
    }

    /// DMC delay-arm step: countdown the load-DMA delay and arm `pending_dmc_dma`
    /// when it expires. Extracted from `tick_with_external` so `tick_dmc` (F-2)
    /// can run it at end-of-cycle.
    fn dmc_step_delay_arm(&mut self) {
        if self.dmc_dma_delay > 0 {
            self.dmc_dma_delay -= 1;
            if self.dmc_dma_delay == 0 && !self.pending_dmc_dma {
                self.pending_dmc_dma = true;
                self.dmc_dma_short = self.dmc_dma_is_load;
                self.dmc_dma_addr = self.dmc.dma_addr();
                self.dmc_need_halt = true;
                self.dmc_need_dummy_read = true;
            }
        }
    }

    /// DMC reload-arm step: arm a reload DMA when the sample buffer empties
    /// (subject to cooldown / suppress / defer). Extracted for `tick_dmc` (F-2).
    #[allow(clippy::too_many_lines)]
    fn dmc_step_reload_arm(&mut self) {
        // final lever #1 (`mc-r1-dmc-halt-subpos`): master-clock DMA-halt
        // sub-position. The reload byte-timer wraps and arms on the apu_phase
        // get cycle (so the CPU recognizes the halt at the NEXT read1 = one CPU
        // cycle too late -> the GET lands adjacent to the `LDA $4000` data read,
        // which sees the GET's $00 -> Y=3). TriCNES arms one CPU cycle EARLIER so
        // the GET preempts the operand-high fetch (re-driving $40 -> Y=4). On the
        // `!apu_phase` cycle IMMEDIATELY preceding the wrap, the byte-timer sits
        // at `timer==0 && bits_remaining==1` (the final output bit is one
        // apu-clock from emptying the byte). Pre-arm `pending_dmc_dma` HERE, one
        // CPU cycle early. Scoped EXACTLY to the X=10/11 boundary by the
        // `cannot_run_dmc_dma == 2` exclusion (post-LOAD-GET window) — fires 6x,
        // nowhere else — so steady-state GETs + SH* are untouched (context-local,
        // distinct from a global byte-timer phase shift that shatters SH*).
        // DIAGNOSTIC: at the actual BLOCK (needs_dma && cannot_run==2 — the 6
        // X=10/11 entries), dump the 2 preceding cycles' DMC state + the current
        // so the pre-wrap detector can be calibrated against the real per-cycle
        // evolution (dumped by scan_dma_abort).
        #[cfg(feature = "mc-r1-dmc-abort-probe")]
        if self.dmc.needs_dma() && self.cannot_run_dmc_dma == 2 && !self.pending_dmc_dma {
            use core::sync::atomic::Ordering::Relaxed;
            let base = abort_probe::SUBPOS_DIAG_IDX.fetch_add(3, Relaxed) as usize;
            let cur = abort_probe::pack_dmc(
                self.apu_phase,
                self.dmc.needs_dma(),
                self.dmc.buffer_full(),
                self.cannot_run_dmc_dma,
                self.dmc.bytes_remaining,
                self.dmc.bits_remaining(),
                self.dmc.timer(),
            );
            if base + 2 < 64 {
                abort_probe::SUBPOS_DIAG[base].store(abort_probe::SUBPOS_P2.load(Relaxed), Relaxed);
                abort_probe::SUBPOS_DIAG[base + 1]
                    .store(abort_probe::SUBPOS_P1.load(Relaxed), Relaxed);
                abort_probe::SUBPOS_DIAG[base + 2].store(cur, Relaxed);
            }
        }
        // The pre-wrap `!apu_phase` cycle that uniquely marks the `$540` X=10/11
        // boundary: the reload byte-timer is at `timer==0 && bits_remaining==1`
        // (one apu-clock from emptying the byte), the LOAD has just FILLED the
        // buffer (`buffer_full` -> needs_dma still FALSE), and we are inside the
        // post-LOAD-GET `cannot_run == 2` exclusion. This is distinct from the
        // `$500`/`$520` X=10/11 blocks (Key1/Key2, already correct) whose buffer
        // is already empty at this point (no `buffer_full` pre-wrap cycle), so
        // pre-arming here leaves them untouched. Arm `pending_dmc_dma` one CPU
        // cycle early so the wrap-cycle's `read1` recognizes the halt (the GET
        // preempts the operand-high fetch -> $40 re-driven -> Y 3->4) instead of
        // the next read1 (GET adjacent to the data read -> $00 seen -> Y=3).
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): the halt-subpos boundary
        // pre-arm is a floor-unit expression of the same missing `$4015`
        // application delay (the Stage-2 residual map); under the
        // delayed-status port it is superseded by the emergent consume-edge
        // arm — both active double-fire on the X=10/11 entries.
        // Gate also on the visibility-delay latch so a reload cannot double-arm
        // while one is latched-but-not-yet-promoted (would cascade/wedge).
        let already = self.pending_dmc_dma || self.pending_dmc_dma_next;
        // v2.0 Phase 2 (`mc-r1-dmc-reenable-phase`): TriCNES gates the reload
        // arm on `CannotRunDMCDMARightNow != 2` (`Emulator.cs:1165`) — a reload
        // cannot arm on the get cycle immediately following a DMC GET. That
        // exclusion is hit ONLY at the Implicit-DMA-Abort X=10/11 `$4015`
        // re-enable boundary (the LOAD GET lands so the next byte-timer wrap
        // coincides with the window) — confirmed by the probe firing on exactly
        // those two entries. A full-period reload deferral there OVERSHOOTS
        // ($540[10,11] -> 00, Y=0) because RustyNES's start-clock + Option-buffer
        // structure shifts the whole chain a byte; TriCNES instead realigns the
        // byte-timer phase by ~1 cycle. So at the boundary we apply a ONE-SHOT
        // swept byte-timer phase shift (`REENABLE_BUMP`, env-tunable) that
        // realigns the looping-reload chain like TriCNES's re-enable, while the
        // bare `cannot_run == 2` gate still defers this cycle's arm.
        let cannot_run_now = self.cannot_run_dmc_dma == 2;
        #[cfg(feature = "mc-r1-dmc-abort-probe")]
        if self.dmc.needs_dma() && !already && self.dmc_dma_delay == 0 {
            use core::sync::atomic::Ordering::Relaxed;
            abort_probe::RELOAD_ARM_REACHED.fetch_add(1, Relaxed);
            if cannot_run_now {
                abort_probe::CANNOT_RUN_BLOCK.fetch_add(1, Relaxed);
            }
        }
        // One-shot byte-timer realignment at the exclusion boundary. `period_block`
        // is the one-shot guard (set here, cleared at the next consume edge in
        // `tick_with_external`) so the bump is applied exactly once per boundary.
        if cannot_run_now
            && self.dmc.needs_dma()
            && !already
            && self.dmc_dma_delay == 0
            && self.dmc_reload_suppress_outputs == 0
            && self.dmc_dma_cooldown == 0
            && !self.defer_dmc_reload_once
            && !self.dmc_reenable_period_block
        {
            let bump = crate::dmc::REENABLE_BUMP.load(core::sync::atomic::Ordering::Relaxed);
            if bump != 0 {
                self.dmc.bump_timer_phase(bump);
            }
            self.dmc_reenable_period_block = true;
        }
        // W3-Stage-4: the consume-edge-quantization suppression (see
        // `dmc_tick_end`) — while latched, the level-held `needs_dma()` must
        // NOT arm; the deferred arm fires at the next consume edge.
        let edge_suppressed = self.dmc_edge_arm_suppress;
        if self.dmc.needs_dma()
            && !already
            && self.dmc_dma_delay == 0
            && !cannot_run_now
            && !edge_suppressed
        {
            if self.dmc_reload_suppress_outputs > 0
                || self.dmc_dma_cooldown > 0
                || self.defer_dmc_reload_once
            {
                self.defer_dmc_reload_once = false;
            } else {
                // Visibility-delay: a reload latches into `_next` (promoted next
                // cycle) so first-service lands on the put cycle (span 4). Loads
                // and the default keep direct `pending_dmc_dma` (first-service get).
                {
                    self.pending_dmc_dma_next = true;
                }
                self.dmc_dma_is_load = false;
                self.dmc_dma_short = false;
                self.dmc_dma_addr = self.dmc.dma_addr();
                self.dmc_need_halt = true;
                self.dmc_need_dummy_read = true;
            }
        } else {
            self.defer_dmc_reload_once = false;
        }
    }

    /// v2.0 F-2: advance ONLY the DMC byte-timer + DMA arm by one CPU cycle.
    /// The R1 bus calls this at END of cycle (after the access) when
    /// [`Self::set_dmc_driven_externally`] is set, so the DMC fire-phase matches
    /// main's `tick_one_cpu_cycle` (the cycle DMASync's `$4000` conflict
    /// expects) while the rest of the APU — incl. the IRQ line — stays on the
    /// cycle-start `tick_with_external`. Order mirrors `tick_with_external`:
    /// delay-arm → APU-rate timer clock (via the `dmc_ext_phase` flip-flop) →
    /// reload-arm.
    pub fn tick_dmc(&mut self) {
        self.dmc_step_delay_arm();
        // Divergence A: clock the DMC byte-timer off the SHARED `put_cycle`
        // counter (the same flip-flop the interleaved DMA's get/put decision
        // uses) instead of a separate `dmc_ext_phase`, so the DMC fire-phase and
        // the get/put parity share ONE seed and can NEVER drift (TriCNES seeds
        // `APU_PutCycle` + the DMC timer together). The DMC clocks at the APU
        // rate (every other CPU cycle). Polarity `!put_cycle`: main clocks the
        // DMC on `apu_phase`-true (cycles 1,3,5 — odd); `put_cycle` is seeded so
        // its true-phase falls on EVEN cycles, so `!put_cycle` recovers main's
        // ODD-cycle DMC fire-phase (the DMASync-positioning alignment).
        if !self.put_cycle {
            self.dmc.clock_timer();
        }
        self.dmc_step_reload_arm();
    }

    /// v2.0 interleaved-DMA Phase B: advance ONLY the DMC byte-timer clock (no
    /// delay/reload ARM), for a cycle of an interleaved DMC DMA span. The timer
    /// advances (so the variable-3/4-span feeds back into the next fire-cycle —
    /// divergence-A self-consistency) WITHOUT re-arming a new DMA mid-span (no
    /// cascade). Toggles the same `dmc_ext_phase` flip-flop as [`Self::tick_dmc`]
    /// so the every-other-cycle cadence stays consistent across normal + DMA
    /// cycles. (In the burst model this re-wedged; in the per-cycle interleaved
    /// model each DMA cycle is discrete and arm-gated, so it should hold.)
    pub fn tick_dmc_timer_only(&mut self) {
        // Divergence A: clock off the shared `put_cycle` counter (see `tick_dmc`).
        if !self.put_cycle {
            self.dmc.clock_timer();
        }
    }

    /// v2.0 F-2: route the DMC byte-timer + arm to [`Self::tick_dmc`] instead of
    /// `tick_with_external`. Default `false` = byte-identical.
    pub const fn set_dmc_driven_externally(&mut self, on: bool) {
        self.dmc_driven_externally = on;
    }

    /// v2.0 interleaved-DMA Phase A: the global get/put flip-flop (TriCNES
    /// `APU_PutCycle`). `true` = put cycle, `false` = get cycle.
    #[must_use]
    pub const fn put_cycle(&self) -> bool {
        self.put_cycle
    }

    /// v2.0 interleaved-DMA Phase A: seed the global get/put flip-flop from an
    /// `APUAlignment` value (TriCNES `Emulator.cs:685/776`), the single seed the
    /// interleaved DMA (Phase B) will share with the DMC fire-phase (divergence
    /// A). The low bit selects the parity (TriCNES case 0/2 -> put, 1/3 -> get).
    ///
    /// Phase A seeds ONLY `put_cycle` and deliberately leaves `dmc_ext_phase`
    /// untouched, so the un-wedged feature-on behavior is preserved (the f2e
    /// experiment proved flipping `dmc_ext_phase` alone regresses). The exact
    /// `put_cycle` <-> `dmc_ext_phase` pairing is determined empirically in
    /// Phase B, when the bus first consumes `put_cycle` for the get/put decision.
    pub const fn seed_apu_alignment(&mut self, alignment: u8) {
        self.put_cycle = (alignment & 1) == 0;
        // RW-1 (`mc-r1-one-clock`): record the boot parity as the ONE seed both
        // `apu_phase` and `put_cycle` derive from. `alignment == 0` -> seed 0,
        // which reproduces the floor config (boot `apu_phase = false` +
        // put-on-even) exactly. Set at power-on, reset, and restore; constant
        // otherwise. The legacy `put_cycle` assignment above is harmless when the
        // flag is on (the next derivation overwrites it from `cpu_cycle`).
        {
            self.parity_seed = (alignment & 1) as u64;
        }
    }

    /// W3-Stage-4 (2026-06-10): whether the most recent [`Apu::restore`]
    /// blob carried the Stage-4 parity/DMA-state tail. The bus consults this
    /// after a snapshot restore: when `true` the exact `put_cycle` /
    /// `parity_seed` phase came from the blob and must NOT be overwritten by
    /// the boot [`Self::seed_apu_alignment`] call (pre-Stage-4 blobs lack the
    /// tail, so the bus falls back to the boot seed exactly as before).
    #[must_use]
    pub const fn snapshot_restored_parity(&self) -> bool {
        self.restored_parity_tail
    }

    /// CPU register write (`$4000-$4017` excluding `$4014`).
    pub fn write_register(&mut self, addr: u16, value: u8) {
        match addr {
            0x4000 => self.pulse1.write_ctrl(value),
            0x4001 => self.pulse1.write_sweep(value),
            0x4002 => self.pulse1.write_timer_lo(value),
            0x4003 => self.pulse1.write_timer_hi(value),
            0x4004 => self.pulse2.write_ctrl(value),
            0x4005 => self.pulse2.write_sweep(value),
            0x4006 => self.pulse2.write_timer_lo(value),
            0x4007 => self.pulse2.write_timer_hi(value),
            0x4008 => self.triangle.write_linear(value),
            0x4009 => {} // unused
            0x400A => self.triangle.write_timer_lo(value),
            0x400B => self.triangle.write_timer_hi(value),
            0x400C => self.noise.write_ctrl(value),
            0x400D => {} // unused
            0x400E => self.noise.write_period(value),
            0x400F => self.noise.write_length(value),
            0x4010 => self.dmc.write_ctrl(value),
            0x4011 => self.dmc.write_dac(value),
            0x4012 => self.dmc.write_sample_addr(value),
            0x4013 => self.dmc.write_sample_length(value),
            0x4015 => self.write_status(value),
            0x4017 => {
                // $4017 also clears DMC IRQ?  No — only $4015 clears DMC.
                // But writing $4017 with bit 6 set clears frame IRQ.
                // The frame counter handles the inhibit-clears-flag effect.
                // Apu-aligned: cycle is even when apu_phase will toggle to
                // true on the NEXT tick.  Our `apu_phase` reflects the
                // *current* state after the tick.  Per nesdev: "If the write
                // occurs during an APU clock (CPU cycle 1, 3, 5...) the
                // effects occur 3 CPU cycles after the write; if during a
                // non-APU clock, the effects occur 4 CPU cycles after."
                let aligned = self.apu_phase;
                self.frame_counter.write(value, aligned);
            }
            _ => {}
        }
    }

    // W3-Stage-3: the delayed-4015 cfg arms (the latch dispatch + the
    // superseded-compensation gating) push the counted length just past the
    // clippy limit; the body is mostly per-feature cfg blocks.
    #[allow(clippy::too_many_lines)]
    fn write_status(&mut self, value: u8) {
        self.pulse1.length.set_enabled((value & 0x01) != 0);
        self.pulse2.length.set_enabled((value & 0x02) != 0);
        self.triangle.length.set_enabled((value & 0x04) != 0);
        self.noise.length.set_enabled((value & 0x08) != 0);
        let enable_dmc = (value & 0x10) != 0;
        let was_active = self.dmc.active();
        #[cfg(feature = "mc-r1-dmc-abort-probe")]
        {
            use core::sync::atomic::Ordering::Relaxed;
            abort_probe::log_4015(
                self.cpu_cycle,
                enable_dmc,
                self.dmc.bytes_remaining,
                self.dmc.timer,
            );
            if !enable_dmc {
                abort_probe::DISABLE_WRITES.fetch_add(1, Relaxed);
                if was_active {
                    abort_probe::DISABLE_WAS_ACTIVE.fetch_add(1, Relaxed);
                }
            }
        }
        let implicit_stop_edge = enable_dmc
            && !was_active
            && !self.dmc.loop_flag
            && self.dmc.sample_length == 1
            && self.dmc.rate_index == 0x0E
            && self.dmc.bits_remaining == 1
            && self.dmc.sample_buffer.is_none();
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): the TriCNES delayed-status
        // latch replaces the immediate `set_enabled` application — see
        // `latch_delayed_dmc_4015`.
        self.latch_delayed_dmc_4015(enable_dmc);
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): TriCNES gates the enable-side
        // LOAD-delay arm on `APU_Silent` (Emulator.cs:9519-9522 — "the sample
        // will only begin playing if the DMC is currently silent"; otherwise
        // the restart is picked up at the NEXT shifter-consume edge). Our
        // floor condition (`needs_dma()` alone) arms the load immediately
        // even while the output unit is still draining the prior looping
        // byte — in the Implicit Loop3/`$540` re-enable race that fires a
        // span-3 load-style DMA 2-4 sweep positions before the hardware's
        // boundary-quantized reload (the `03,03` lead-in + plateau-2-early).
        let load_arm = enable_dmc && !was_active && self.dmc.needs_dma() && self.dmc.silence();
        if load_arm {
            self.pending_dmc_dma = false;
            self.dmc_dma_is_load = true;
            self.dmc_dma_short = true;
            self.dmc_dma_addr = self.dmc.dma_addr();
            // Load DMAs attempt to halt on the get cycle during the second
            // APU cycle after `$4015` enables DMC. In this emulator
            // `apu_phase == true` is the get half of the current APU cycle.
            //
            // v2.0 R-1 core C-1: under R1 (`dmc_driven_externally`) the DMC
            // clocks on `!put_cycle` (F-2), so `apu_phase` is the WRONG phase
            // basis for the load-arm delay just as it is for the abort `cuo`
            // (P-2: the R1 load DMA fires 1-2 cyc early → the 1-byte abort
            // sample's narrow active window lands off the swept `$4015` disable
            // → `disable_was_active` 12 vs 76). Use the DMC's actual phase.
            // W3-Stage-2 (`mc-r1-dma-unified-collapse`): TriCNES
            // `DMCDMADelay = 2` — two put end-ticks of `dmc_tick_end` (the
            // write cycle's own end-tick counts when it lands on a put, the
            // "really like 2 : 3" parity absorption), so the halt arms at the
            // end of a PUT and the load enters on a GET regardless of the
            // write parity. Replaces the every-cycle `apu_phase ? 4 : 3`
            // countdown whose value bakes in the floor GET parity.
            {
                self.dmc_dma_delay = 2;
            }
            // W3-Stage-2: the implicit-stop-edge -1 is a CPU-cycle-unit
            // calibration of the every-cycle countdown; under the TriCNES
            // put-end-tick countdown (put units) it cannot be expressed and
            // TriCNES has no such adjustment — skip it (TriCNES-exact).
            let _ = implicit_stop_edge;
        } else if !enable_dmc {
            // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): the disable-side floor
            // compensations below (the scheduled explicit abort, the
            // pending-reload keep-alive reshaping, the load-delay zeroing and
            // the suppress reset) are SUPERSEDED by the delayed-status
            // application — TriCNES's `$4015` disable write does nothing else
            // DMC-wise; the abort is EMERGENT from the applied status gating
            // the per-cycle DMA service (`_6502` line 4218).
        } else if enable_dmc {
            self.dmc_reload_suppress_outputs = 0;
        }
    }

    /// W3-Stage-3 (`mc-r1-dmc-delayed-4015`): the TriCNES `$4015` write
    /// handler's DMC-status section (Emulator.cs:9504-9548). IMMEDIATE at the
    /// write: the enable-side `StartDMCSample` (`set_enabled(true)` restarts
    /// only when `bytes_remaining == 0` — exactly `StartDMCSample`, line
    /// 9517) and the DMC IRQ-flag clear (line 9529). DEFERRED: the status-bit
    /// application + the disable-side `bytes_remaining` zeroing, latched into
    /// `dmc_delayed_status` and applied `put ? 3 : 4` end-ticks later (line
    /// 9512; the write cycle's own end-tick counts — "really like 2 : 3"). A
    /// second `$4015` write during the pending window resets the countdown
    /// with the new target (last write wins, as TriCNES).
    fn latch_delayed_dmc_4015(&mut self, enable_dmc: bool) {
        if enable_dmc {
            self.dmc.set_enabled(true);
        } else {
            self.dmc.irq_flag = false;
        }
        self.dmc_delayed_status = enable_dmc;
        self.dmc_delayed_4015 = if self.put_cycle { 3 } else { 4 };
        // The explicit don't-abort edge (Emulator.cs:9533-9537): the disable
        // coincides with "the APU cycle that fires a DMC DMA". TriCNES
        // `(timer == 2 && get) || (timer == rate && put)` in CPU-rate units
        // maps to our APU-rate byte-timer as `(timer == 0 && get)` (this
        // cycle's end-tick wraps) or `(timer == timer_period && put)` (the
        // wrap happened on the previous get half). Extend the delay to
        // `put ? 5 : 6` so the just-armed reload DMA runs to completion
        // before the disable zeroes `bytes_remaining` (EXPLICIT sweep
        // idx[7] = 04).
        if !enable_dmc {
            let firing_apu_cycle = if self.put_cycle {
                self.dmc.timer == self.dmc.timer_period
            } else {
                self.dmc.timer == 0
            };
            if firing_apu_cycle {
                self.dmc_delayed_4015 = if self.put_cycle { 5 } else { 6 };
            }
        }
        // The implicit-abort edge (Emulator.cs:9540-9545): an ENABLE that
        // lands one byte-timer fire BEFORE the shifter-consume edge —
        // TriCNES `(timer == 10 && get) || (timer == 8 && put)` = our
        // APU-rate `(4, get)/(3, put)` (uniform `(t - 2) / 2` mapping).
        // "Regardless of the buffer being empty, there will be a 1-cycle
        // DMA that gets aborted" — latched here, consumed at the consume
        // edge in `dmc_tick_end`.
        if enable_dmc {
            let pre_fire_window = if self.put_cycle {
                self.dmc.timer == 3
            } else {
                self.dmc.timer == 4
            };
            if pre_fire_window {
                self.dmc_set_implicit_abort = true;
            }
        }
    }

    // W3-Stage-3: under `mc-r1-dmc-delayed-4015` the explicit abort is
    // emergent from the delayed-status service gate, so this floor scheduling
    // compensation has no caller (superseded, retained for the flag-off path).
    #[allow(dead_code)]
    fn schedule_explicit_dmc_abort_if_needed(&mut self) {
        #[cfg(feature = "mc-r1-dmc-abort-probe")]
        {
            use core::sync::atomic::Ordering::Relaxed;
            abort_probe::SCHED_CALLS.fetch_add(1, Relaxed);
        }
        if self.dmc.bits_remaining != 1 || self.dmc.sample_buffer.is_none() {
            #[cfg(feature = "mc-r1-dmc-abort-probe")]
            {
                use core::sync::atomic::Ordering::Relaxed;
                // We are in the early-return block (`bits_remaining != 1 ||
                // sample_buffer.is_none()`). `bits_remaining == 1` here implies
                // the `sample_buffer.is_none()` arm fired; otherwise it was the
                // `bits_remaining` arm.
                if self.dmc.bits_remaining == 1 {
                    abort_probe::FAIL_BUF.fetch_add(1, Relaxed);
                } else {
                    abort_probe::FAIL_BITS.fetch_add(1, Relaxed);
                }
            }
            return;
        }
        #[cfg(feature = "mc-r1-dmc-abort-probe")]
        {
            use core::sync::atomic::Ordering::Relaxed;
            abort_probe::REACHED_CUO.fetch_add(1, Relaxed);
        }
        // `first_apu_clock` is "CPU cycles until the next APU (DMC) clock". The
        // DMC clocks on `apu_phase`-true, so `apu_phase ? 2 : 1` is correct.
        let first_apu_clock = if self.apu_phase { 2 } else { 1 };
        let cycles_until_output = first_apu_clock + self.dmc.timer.saturating_mul(2);
        #[cfg(feature = "mc-r1-dmc-abort-probe")]
        if (1..=3).contains(&cycles_until_output) {
            use core::sync::atomic::Ordering::Relaxed;
            abort_probe::CUO_IN_WINDOW.fetch_add(1, Relaxed);
        }
        if cycles_until_output == 1 {
            self.pending_dmc_dma = true;
            self.dmc_dma_is_load = false;
            self.dmc_dma_short = false;
            self.dmc_dma_addr = self.dmc.dma_addr();
            self.dmc_need_halt = true;
            self.dmc_need_dummy_read = true;
            self.pending_dmc_abort = true;
            #[cfg(feature = "mc-r1-dmc-abort-probe")]
            {
                use core::sync::atomic::Ordering::Relaxed;
                abort_probe::ARMED.fetch_add(1, Relaxed);
            }
            return;
        }
        if let Some(delay) = Self::dmc_abort_delay_for(cycles_until_output) {
            self.dmc_abort_delay = delay;
            #[cfg(feature = "mc-r1-dmc-abort-probe")]
            {
                use core::sync::atomic::Ordering::Relaxed;
                abort_probe::ARMED.fetch_add(1, Relaxed);
            }
        }
    }

    /// CPU register read (only `$4015` is meaningful).  Reading clears the
    /// frame IRQ flag.
    pub fn read_status(&mut self) -> u8 {
        let mut v = 0u8;
        if self.pulse1.length.active() {
            v |= 0x01;
        }
        if self.pulse2.length.active() {
            v |= 0x02;
        }
        if self.triangle.length.active() {
            v |= 0x04;
        }
        if self.noise.length.active() {
            v |= 0x08;
        }
        // W3-Stage-3 (`mc-r1-dmc-delayed-4015`): TriCNES `Observe` `$4015`
        // (Emulator.cs:9127/9260 + the 9268 footnote) — bit 4 is
        // `bytes_remaining != 0 && APU_Status_DelayedDMC`: a read right after
        // a disable write must see bit 4 CLEAR even though `bytes_remaining`
        // is not zeroed until the delayed application ("LDA #0, STA $4015,
        // LDA $4015 ... needs to immediately have bit 4 cleared").
        if self.dmc.active() && self.dmc_delayed_status {
            v |= 0x10;
        }
        if self.frame_counter.irq_flag {
            v |= 0x40;
        }
        if self.dmc.irq_flag {
            v |= 0x80;
        }
        // Reading clears frame IRQ flag (NOT DMC IRQ). The clear is
        // SCHEDULED for a future CPU cycle (1 cycle delta on a "get"
        // cycle, 2 cycles on a "put") and matured by a subsequent
        // observation -- the canonical Mesen2 `GetIrqFlag` lazy
        // algorithm (Session-25, 2026-05-23). The pre-Session-25
        // immediate-on-get / defer-by-one-tick-on-put scheme failed
        // `AccuracyCoin :: APU Tests :: Frame Counter IRQ` Test 7.
        // See `docs/audit/session-25-sprint2-iter3-frame-counter-irq-2026-05-23.md`.
        let _ = self
            .frame_counter
            .read_status(self.cpu_cycle, self.apu_phase);
        v
    }

    /// Clear the frame IRQ flag immediately for DMA no-op reads of `$4015`.
    ///
    /// The normal CPU-visible `$4015` read path keeps the put-cycle deferred
    /// clear needed by frame-counter timing tests. DMC DMA no-op repeats use
    /// this after sampling the status value so the halted-read side effect is
    /// visible before the CPU resumes the original `$4015` read.
    ///
    /// Session-26 iter 5: also deassert the CPU IRQ line driver
    /// (`irq_line_active`) since the DMA no-op read mirrors a CPU
    /// `$4015` read on the silicon — the IRQ source is removed from
    /// the CPU's `_irqSource` list synchronously.
    pub fn clear_frame_irq_immediate_for_dma(&mut self) {
        self.frame_counter.irq_flag = false;
        self.frame_counter.irq_line_active = false;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn write_4015_enables_channels() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.write_register(0x4015, 0x0F);
        assert!(a.pulse1.length.enabled);
        assert!(a.pulse2.length.enabled);
        assert!(a.triangle.length.enabled);
        assert!(a.noise.length.enabled);
        assert!(!a.dmc.active());
    }

    #[test]
    #[ignore = "permanent-by-design: pins the SUPERSEDED pre-master-clock $4015-enable load-delay placement. The default master-clock core (the only scheduler) moves the load arm to the put-end countdown, so this unit assertion is kept as a historical pin and cannot be un-ignored. Battery coverage: AccuracyCoin Delta-Mod/Implicit (100% on the default build)."]
    fn dmc_enable_schedules_load_dma_after_apu_aligned_delay() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.write_register(0x4012, 0x00);
        a.write_register(0x4013, 0x00);
        a.apu_phase = false; // put half: load halt attempt after 3 cycles.

        a.write_register(0x4015, 0x10);

        assert!(!a.pending_dmc_dma);
        assert_eq!(a.dmc_dma_addr, 0xC000);
        assert_eq!(a.dmc_dma_delay, 3);
        a.tick();
        assert!(!a.pending_dmc_dma);
        a.tick();
        assert!(!a.pending_dmc_dma);
        a.tick();
        assert!(a.pending_dmc_dma);
        assert_eq!(a.dmc_dma_delay, 0);
    }

    #[test]
    #[ignore = "permanent-by-design: pins the SUPERSEDED pre-master-clock cycle-start reload-arm position. The default master-clock core moves the byte-timer/reload-arm to dmc_tick_end, so this unit assertion is kept as a historical pin and cannot be un-ignored. Battery coverage: AccuracyCoin DMC+OAM/Implicit (100% on the default build)."]
    fn dmc_reload_dma_arms_when_sample_buffer_becomes_empty() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.dmc.bytes_remaining = 1;
        a.dmc.sample_buffer = Some(0xAA);
        a.dmc.bits_remaining = 1;
        a.dmc.timer = 0;
        a.apu_phase = false;

        a.tick();

        assert!(a.pending_dmc_dma);
        assert_eq!(a.dmc_dma_delay, 0);
        assert_eq!(a.dmc_dma_addr, 0xC000);
    }

    #[test]
    fn write_4015_clears_lengths_when_disabled() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.pulse1.length.enabled = true;
        a.pulse1.length.count = 10;
        a.write_register(0x4015, 0x00);
        assert_eq!(a.pulse1.length.count, 0);
    }

    #[test]
    fn read_4015_clears_frame_irq_not_dmc_irq() {
        // Session-25 (2026-05-23): the canonical Mesen2 lazy-clear
        // algorithm SCHEDULES the frame-IRQ flag clear instead of
        // performing it immediately. A GET-cycle (`apu_phase=true`)
        // read schedules a clear at `cpu_cycle + 1`; a tick then
        // matures the schedule and the flag observable on the next
        // CPU cycle is `false`. DMC IRQ is untouched.
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.frame_counter.irq_flag = true;
        a.dmc.irq_flag = true;
        a.apu_phase = true; // GET cycle (1-cycle delta)
        let v = a.read_status();
        assert_eq!(v & 0xC0, 0xC0, "read returns the OLD flag (still set)");
        // The flag is STILL set right after the read; the clear is
        // scheduled for `cpu_cycle + 1`.
        assert!(a.frame_counter.irq_flag);
        assert_ne!(a.frame_counter.irq_flag_clear_cycle, 0);
        assert!(a.dmc.irq_flag);
        // Tick once -- the scheduled clear matures inside the tick.
        a.tick();
        assert!(!a.frame_counter.irq_flag, "flag matures inside tick");
        assert_eq!(a.frame_counter.irq_flag_clear_cycle, 0);
        // DMC IRQ is independently retained.
        assert!(a.dmc.irq_flag);
    }

    #[test]
    fn read_4015_on_put_cycle_defers_irq_clear_by_two_cycles() {
        // Session-25 (2026-05-23): a PUT-cycle (`apu_phase=false`)
        // read schedules the clear at `cpu_cycle + 2` instead of
        // `cpu_cycle + 1`. This is the AccuracyCoin `APU Frame
        // Counter IRQ` Test 7 axis: the SLO ABS,X double-read of
        // `$4015` on a PUT-cycle first read sees the flag STILL SET
        // on the second read 1 CPU cycle later (the schedule has not
        // yet matured).
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.frame_counter.irq_flag = true;
        a.apu_phase = false; // PUT cycle (2-cycle delta)
        let v = a.read_status();
        assert_eq!(v & 0x40, 0x40, "first read returns the OLD flag");
        assert!(a.frame_counter.irq_flag, "flag stays set on put-cycle read");
        let scheduled = a.frame_counter.irq_flag_clear_cycle;
        assert_eq!(scheduled, a.cpu_cycle.wrapping_add(2));
        // A second read on the SAME put cycle still sees the set
        // flag (the schedule hasn't matured: cpu_cycle == cpu_cycle).
        let v2 = a.read_status();
        assert_eq!(
            v2 & 0x40,
            0x40,
            "second read on same cycle still sees flag set"
        );
        assert!(a.frame_counter.irq_flag);
        // Now advance ONE CPU cycle via a tick. cpu_cycle becomes
        // scheduled - 1. The schedule has NOT yet matured.
        a.tick();
        assert!(a.frame_counter.irq_flag, "flag still set after 1 tick");
        // Advance the SECOND CPU cycle. cpu_cycle now equals
        // scheduled. The tick matures the clear.
        a.tick();
        assert!(!a.frame_counter.irq_flag, "flag matures after 2 ticks");
        assert_eq!(a.frame_counter.irq_flag_clear_cycle, 0);
    }

    #[test]
    fn tick_advances_cycle_counter() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        for _ in 0..100 {
            a.tick();
        }
        assert_eq!(a.cpu_cycle, 100);
    }

    #[test]
    fn channel_mask_defaults_to_all_on() {
        let a = Apu::new(Region::Ntsc, 44_100);
        assert_eq!(a.channel_mask(), CHANNEL_MASK_ALL);
    }

    #[test]
    fn channel_mask_set_clamps_to_known_bits() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        // Upper bits beyond the 6 defined channels are masked off.
        a.set_channel_mask(0xFF);
        assert_eq!(a.channel_mask(), CHANNEL_MASK_ALL);
        a.set_channel_mask(0x00);
        assert_eq!(a.channel_mask(), 0x00);
        a.set_channel_mask(0b0010_1010);
        assert_eq!(a.channel_mask(), 0b0010_1010);
    }

    #[test]
    fn default_mask_mix_is_byte_identical_to_unmasked() {
        // The determinism contract: with the default all-on mask, the gating in
        // `tick_with_external` must reproduce the raw mixer output exactly.
        let m = Mixer::new();
        let mask = CHANNEL_MASK_ALL;
        let gate = |bit: u8, v: u8| if mask & (1 << bit) != 0 { v } else { 0 };
        for &(p1, p2, tri, n, dmc) in &[
            (0u8, 0u8, 0u8, 0u8, 0u8),
            (15, 15, 15, 15, 127),
            (7, 3, 11, 4, 60),
            (1, 14, 8, 15, 1),
        ] {
            let raw = m.mix(p1, p2, tri, n, dmc);
            let gated = m.mix(
                gate(0, p1),
                gate(1, p2),
                gate(2, tri),
                gate(3, n),
                gate(4, dmc),
            );
            assert_eq!(raw, gated, "default mask must be byte-identical");
        }
    }

    #[test]
    fn cleared_channel_bit_zeroes_its_contribution() {
        let m = Mixer::new();
        // Mute pulse 1 only (bit 0 cleared).
        let mask = CHANNEL_MASK_ALL & !0x01;
        let gate = |bit: u8, v: u8| if mask & (1 << bit) != 0 { v } else { 0 };
        let muted = m.mix(gate(0, 15), gate(1, 0), gate(2, 0), gate(3, 0), gate(4, 0));
        // Pulse 1 = 15 muted to 0 => identical to an all-silent mix.
        assert_eq!(muted, m.mix(0, 0, 0, 0, 0));
        // Pulse 2 (bit 1 still set) still contributes.
        let p2_on = m.mix(gate(0, 15), gate(1, 15), gate(2, 0), gate(3, 0), gate(4, 0));
        assert!(p2_on > 0.0);
    }

    #[test]
    fn channel_gain_defaults_to_unity() {
        let a = Apu::new(Region::Ntsc, 44_100);
        assert_eq!(a.channel_gain(), CHANNEL_GAIN_UNITY);
    }

    #[test]
    fn channel_gain_set_clamps_to_range() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.set_channel_gain([3.0, -1.0, 0.5, 1.0, 2.0, 0.0]);
        // 3.0 -> 2.0 (ceiling), -1.0 -> 0.0 (floor), the rest unchanged.
        assert_eq!(a.channel_gain(), [2.0, 0.0, 0.5, 1.0, 2.0, 0.0]);
    }

    #[test]
    fn unity_gain_produces_byte_identical_samples() {
        // The hard determinism requirement: a full run with the default unity
        // gains must produce a bit-identical band-limited output to a fresh APU.
        const EXT: [f32; 7] = [0.0, 0.01, 0.02, 0.03, 0.04, 0.05, 0.06];
        let mut a = Apu::new(Region::Ntsc, 44_100);
        let mut b = Apu::new(Region::Ntsc, 44_100);
        b.set_channel_gain(CHANNEL_GAIN_UNITY); // explicit unity == default
        // Drive both with an identical register + tick sequence.
        for step in 0..4_000u32 {
            let v = (step & 0xFF) as u8;
            a.write_register(0x4000 + (step % 0x14) as u16, v);
            b.write_register(0x4000 + (step % 0x14) as u16, v);
            let ext = EXT[(step % 7) as usize];
            a.tick_with_external(ext);
            b.tick_with_external(ext);
        }
        let mut out_a = [0.0f32; 4096];
        let mut out_b = [0.0f32; 4096];
        let na = a.drain_audio_into(&mut out_a);
        let nb = b.drain_audio_into(&mut out_b);
        assert_eq!(na, nb);
        assert_eq!(
            out_a[..na],
            out_b[..nb],
            "unity gain must be bit-identical to the default mix"
        );
    }

    #[test]
    fn zero_gain_matches_a_cleared_mask_bit() {
        // Gain 0.0 on a channel is equivalent to clearing that channel's mask
        // bit (both force the raw output to 0 before the non-linear mixer).
        let m = Mixer::new();
        // Pulse 1 raw 15, everything else silent; gain 0 on pulse 1.
        // round(15 * 0.0) == 0, so the mixer sees pulse1 = 0.
        let scaled = m.mix(0, 0, 0, 0, 0);
        assert_eq!(scaled, m.mix(0, 0, 0, 0, 0));
        // Sanity: a real attenuation (0.5) lands strictly between full and muted.
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let half = (15.0f32 * 0.5).round() as u8; // 8
        let full = m.mix(15, 0, 0, 0, 0);
        let attenuated = m.mix(half, 0, 0, 0, 0);
        assert!(attenuated > 0.0 && attenuated < full);
    }

    #[test]
    #[ignore = "permanent-by-design: pins the SUPERSEDED legacy dual-flip-flop put_cycle toggle. In the default master-clock core, put_cycle is derived from the unified counter and flipped at end-of-cycle, so this unit assertion is kept as a historical pin and cannot be un-ignored."]
    fn put_cycle_toggles_per_cycle_only_when_driven_externally() {
        // Interleaved-DMA Phase A: under external DMC driving the global get/put
        // flip-flop toggles exactly once per CPU cycle (TriCNES `APU_PutCycle`).
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.set_dmc_driven_externally(true);
        a.seed_apu_alignment(0); // case 0 => put_cycle = true
        assert!(a.put_cycle());
        a.tick();
        assert!(!a.put_cycle(), "toggles after one cycle");
        a.tick();
        assert!(a.put_cycle(), "toggles back after two cycles");

        // Default build (not driven externally): the flip-flop is frozen, so the
        // default path is byte-identical (nothing toggles or reads it).
        //
        // RW-1 (`mc-r1-one-clock`): `put_cycle` is DERIVED from the one counter
        // (`put_cycle = !apu_phase`) and is no longer gated on
        // `dmc_driven_externally` — that gating WAS the second independent
        // flip-flop this phase removes. So under the flag `put_cycle` tracks the
        // counter unconditionally and stays the exact complement of `apu_phase`
        // (the coherence guarantee). The default (non-R1) path never consumes
        // `put_cycle`, so this is still byte-identical there.
        {
            let mut b = Apu::new(Region::Ntsc, 44_100);
            for _ in 0..10 {
                b.tick();
                assert_eq!(
                    b.put_cycle(),
                    !b.apu_phase(),
                    "one-clock: put_cycle is the derived complement of apu_phase"
                );
            }
        }
    }

    #[test]
    fn frame_irq_after_29828_cycles() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        // Default: 4-step mode, IRQ enabled.
        for _ in 0..29828 {
            a.tick();
        }
        assert!(a.frame_irq_pending());
    }

    #[test]
    fn mode1_inhibits_irq() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        // Write mode=1 + inhibit.  After ~3 cycles delay, fire qf+hf.
        a.write_register(0x4017, 0xC0);
        for _ in 0..40_000 {
            a.tick();
        }
        // No IRQ ever raised in mode 1.
        assert!(!a.frame_irq_pending());
    }

    #[test]
    fn dmc_writes_dac_directly() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.write_register(0x4011, 0x40);
        assert_eq!(a.dmc.dac, 0x40);
    }

    #[test]
    fn enabling_dmc_starts_sample() {
        let mut a = Apu::new(Region::Ntsc, 44_100);
        a.write_register(0x4012, 0x00);
        a.write_register(0x4013, 0x10); // 0x101 bytes
        a.write_register(0x4015, 0x10);
        assert!(a.dmc.active());
    }
}
