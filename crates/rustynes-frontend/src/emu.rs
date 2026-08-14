//! v2.8.0 Phase 5 — the emulation core extracted from `App`.
//!
//! [`EmuCore`] owns everything the per-frame produce path touches — the
//! `Nes`, the TAS movie state machine, run-ahead, the presented
//! framebuffer, perf instrumentation, raw cheats, the Vs. coin latch, and
//! the pacing deadlines — while `App` keeps the platform-resident pieces
//! (window/gfx, the cpal stream, input devices, config, dialogs) and
//! passes them in per call ([`FrameSinks`] / [`FrameInputs`]).
//!
//! Increment 1 (this step) is a PURE STRUCTURAL refactor: `App` owns the
//! `EmuCore` and calls it synchronously from exactly the old call sites, so
//! behavior is byte-for-byte unchanged. The split is the load-bearing
//! boundary for the emulation-thread separation (increment 3): everything
//! in `EmuCore` is `Send`-owned per-frame state; everything that stays on
//! `App` is the winit-thread-resident surface.
//!
//! Netplay deliberately does NOT live here: a netplay session paces
//! wall-clock with its own stall/frame-advantage logic and is driven from
//! `App` (it early-exits before the core produce path), exactly as before.

use std::time::Duration;

use rustynes_core::{Buttons, Nes};
use web_time::Instant;

use crate::cheats::RawCheat;
use crate::movie_ui::MovieUi;
use crate::perf::PerfStats;

/// v1.7.0 "Forge" Workstream A1 — one queued debugger writeback edit.
///
/// Applied through the SAME gated post-frame poke stage the raw RAM cheats use.
/// Unlike a `RawCheat` it is one-shot (drained, not retained), so a
/// tile/palette/OAM edit from a debugger panel lands exactly once on the next
/// produced frame and then stops perturbing the run — preserving the
/// determinism contract for any later replay/record.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DebugPoke {
    /// Write one byte into CPU work RAM (`$0000-$1FFF`); the same target as a
    /// raw RAM cheat, exposed for the assembler + hex-editor inline poke.
    CpuRam {
        /// CPU work-RAM address (`$0000-$1FFF`; the core no-ops outside it).
        addr: u16,
        /// Byte value to write.
        value: u8,
    },
    /// Write one byte into the PPU bus (`$0000-$3FFF`): CHR / nametable /
    /// palette. Routes through `Nes::debug_poke_ppu`.
    PpuBus {
        /// PPU-bus address (`$0000-$3FFF`).
        addr: u16,
        /// Byte value to write.
        value: u8,
    },
    /// Write one OAM byte (`idx` = 0..256). Routes through `Nes::poke_oam_byte`.
    Oam {
        /// OAM byte index (0..256: per sprite, 0 = Y, 1 = tile, 2 = attr, 3 = X).
        idx: u8,
        /// Byte value to write.
        value: u8,
    },
}

#[cfg(not(target_arch = "wasm32"))]
use crate::config::ExpansionDevice;

/// Maximum number of frames to catch up in a single pace iteration.
/// If the system slipped further (e.g. hibernate, debugger pause), we snap
/// `next_frame_time = now` instead of producing a long burst.
pub const MAX_CATCHUP_FRAMES: u32 = 3;

/// Cloneable shared handle to the emulation core (v2.8.0 Phase 5,
/// increment 2b): an `Arc<Mutex<EmuCore>>` behind a tiny API so every
/// access site goes through one explicit, deliberate `lock()`.
///
/// ⚠️ The mutex is **non-reentrant**: never call `lock()` (directly or via
/// any `App` helper that locks internally — produce paths, housekeeping,
/// `flush_fds_save`, save/load state, ROM load) while a guard from a prior
/// `lock()` is still alive in the same scope. The conversion convention in
/// `app.rs` is: bind a guard explicitly (`let mut guard = self.emu.lock();
/// let emu = &mut *guard;`) for multi-field regions and let single-field
/// accesses use a statement-temporary guard; a guard never spans a call
/// into another locking helper.
///
/// On wasm32 there are no threads, so the lock never contends — the
/// uniform type keeps the module tree identical across targets. On native
/// the emulation thread (increment 3) holds the lock for the duration of
/// each frame produce; the winit thread takes it briefly for UI reads,
/// input-latch writes, and command handling.
#[derive(Clone)]
pub struct EmuHandle {
    inner: std::sync::Arc<std::sync::Mutex<EmuCore>>,
}

/// Hard cap on run-ahead depth, in frames.
///
/// One constant because two independent `3` literals are exactly what caused
/// the defect they now both express: `update_runahead_throttle` predicted a
/// depth the produce path would never run, because its cap and
/// `effective_run_ahead`'s cap were separate literals that drifted. (PR #358
/// review.)
/// Native-only: both users (`effective_run_ahead`, `update_runahead_throttle`)
/// are, and the wasm frontend has no run-ahead path.
#[cfg(not(target_arch = "wasm32"))]
const MAX_RUN_AHEAD_DEPTH: u32 = 3;

/// v2.3.3 F21 — fraction of the frame budget at which the run-ahead throttle
/// engages, measured rather than chosen.
///
/// It was **0.85**, and the F18 depth sweep shows that is too high to protect
/// what it exists to protect. Sixteen validity-gated captures, four per depth,
/// with the display-side hold distribution measured from `since_present`:
///
/// | depth | median produce cost | % of budget | frames held for the wrong duration |
/// |---|---|---|---|
/// | 1 | 8.671 ms | 52.1% | **1.7%** |
/// | 2 | 12.884 ms | 77.4% | **10.7%** |
///
/// At 77.4% the display is already visibly degraded — one frame in nine shown
/// for the wrong number of refreshes — while the old 0.85 gate had not fired.
/// That band, harmful but unthrottled, is the defect this constant fixes.
///
/// **0.75 is chosen to sit between the two MEASURED points**, above the healthy
/// 52.1% and below the harmful 77.4%. It is deliberately not tuned finer: the
/// sweep has no conditions between those two, so any more precise value would be
/// interpolation presented as measurement. The engage test uses the produce
/// MEDIAN (not p95) for the reason the surrounding comment gives — on the
/// emulation thread the tail is OS descheduling, not run-ahead's compute.
#[cfg(not(target_arch = "wasm32"))]
const RUNAHEAD_THROTTLE_ENGAGE: f32 = 0.75;

/// Samples the produce-cost ring must hold before its median is read at all.
///
/// This is a MINIMUM (is the statistic meaningful yet?), and is emphatically not
/// the ring's capacity or its turnover period — [`crate::perf::WINDOW`] is both
/// of those. Conflating the two is what F27 fixes; the name says which one this
/// is so the next reader cannot repeat the substitution.
#[cfg(not(target_arch = "wasm32"))]
const THROTTLE_MIN_SAMPLES: usize = 120;

/// Fraction of the frame budget the PREDICTED cost one depth up must fall under
/// before a throttled step is given back.
///
/// The gap against `RUNAHEAD_THROTTLE_ENGAGE` is the hysteresis. It was a bare
/// `0.70` at both its use sites while the engage side had a named constant and a
/// documented derivation, which is the asymmetry that let the release arm be
/// re-tuned twice (F21's 0.55 experiment, F23) without either change landing
/// next to the reasoning for the value it replaced.
#[cfg(not(target_arch = "wasm32"))]
const RUNAHEAD_THROTTLE_RELEASE: f32 = 0.70;

/// The two must not be swapped back. A minimum-to-report at or above the ring's
/// capacity would mean the throttle either never reports or reports only on a
/// full ring, and the F27 defect was precisely a confusion between these two
/// quantities -- so the relationship is enforced by the compiler rather than
/// left to the next reader to notice.
#[cfg(not(target_arch = "wasm32"))]
const _: () = assert!(THROTTLE_MIN_SAMPLES < crate::perf::WINDOW);

impl EmuHandle {
    /// Wrap a fresh core in the shared handle.
    #[must_use]
    pub fn new(core: EmuCore) -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(core)),
        }
    }

    /// [`Self::lock`], adding the time spent BLOCKED to `acc`.
    ///
    /// v2.3.3 — the winit thread acquires this mutex six times in one redraw
    /// handler and none of those acquisitions was timed, while the producer's
    /// single acquisition was (`PerfStats::produce_wait`). A redraw blocked
    /// behind a produce was therefore billed entirely as render *work* — the
    /// third instance in this campaign of blocking recorded as work, after
    /// `cost_*` and the F8 `wait` series itself.
    ///
    /// Accumulates rather than returning, because what matters is the total
    /// blocked time across a whole redraw, not any single acquisition; the
    /// caller keeps one `Duration` for the handler and passes it here at each
    /// site. Costs two `Instant::now()` calls per acquisition, which is the
    /// same overhead the producer side has carried since v2.3.3 F1.
    pub fn lock_timed(&self, acc: &mut Duration) -> std::sync::MutexGuard<'_, EmuCore> {
        let t = Instant::now();
        let guard = self.lock();
        *acc += t.elapsed();
        guard
    }

    /// Lock the core. Poisoning is ignored deliberately: a panic on one
    /// thread must not wedge the other (the core's state is a plain value;
    /// the next frame either works or panics identically).
    pub fn lock(&self) -> std::sync::MutexGuard<'_, EmuCore> {
        #[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
        debug_assert!(
            !gpu_phase_active(),
            "EmuHandle::lock() called from the GPU phase of a frame. v2.3.0 split the \
             shell render into `run_shell_ui` (holds this lock) and `paint_shell` (must \
             not), because holding it across the blocking swapchain acquire + present \
             parked the emulation thread for up to a full display refresh every frame. \
             Re-locking inside the paint/overlay path silently reintroduces that stall \
             with no other symptom than the stutter coming back."
        );
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
    }
}

// v2.3.0 "Datum II" — debug-only guard for the phase-1/phase-2 lock-scope
// invariant introduced when `render_shell` was split.
//
// The split is load-bearing for frame pacing but is enforced by nothing except
// care: today the native `extra` closure deliberately uses the `ss_dir` / `ss_sha`
// values snapshotted *before* the pass precisely so it need not re-lock, and a
// future panel that re-locked inside the paint path would compile fine, pass every
// test, and quietly restore the stall. This makes that mistake fail loudly in debug
// builds. Zero cost in release (`debug_assertions` off) and never compiled on wasm,
// which has no emulation thread to starve.
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
thread_local! {
    static GPU_PHASE: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

/// True while the calling thread is inside a frame's GPU phase.
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
#[must_use]
pub fn gpu_phase_active() -> bool {
    GPU_PHASE.with(std::cell::Cell::get)
}

/// RAII marker for the GPU phase of a frame — construct it around the
/// paint/encode/present region so any `EmuHandle::lock` inside trips the
/// debug assertion above.
#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
pub struct GpuPhaseGuard;

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
impl GpuPhaseGuard {
    /// Enter the GPU phase.
    #[must_use]
    pub fn enter() -> Self {
        GPU_PHASE.with(|c| c.set(true));
        Self
    }
}

#[cfg(all(debug_assertions, not(target_arch = "wasm32")))]
impl Drop for GpuPhaseGuard {
    fn drop(&mut self) {
        GPU_PHASE.with(|c| c.set(false));
    }
}

/// The emulation thread (increment 3) moves an `EmuHandle` across a thread
/// boundary, which requires `EmuCore: Send`. `Box<dyn Mapper>` is `Send`
/// (the `Mapper` trait bounds it) and the core holds no `Rc`/`Cell`, so it
/// is — assert it at compile time so a future non-`Send` field is caught
/// here, not at the spawn site. Native-only (wasm has no threads).
#[cfg(not(target_arch = "wasm32"))]
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<EmuCore>();
};

/// Per-pace input snapshot, computed by `App` and consumed by the core.
///
/// `App` derives it from its winit-thread-resident input state (keyboard
/// maps, gilrs, mouse); this is the data that crosses the future thread
/// boundary as plain values.
// Independent gate/state bits — bitflags would obscure the produce logic.
#[allow(clippy::struct_excessive_bools)]
#[derive(Debug, Clone, Copy)]
pub struct FrameInputs {
    /// Latched controller state for players 1-4.
    pub buttons: [Buttons; 4],
    /// Whether the Four Score adapter is enabled (players 3/4 latch).
    pub four_score: bool,
    /// v2.2.0 "Capstone" — Famicom built-in microphone held (read on `$4016`
    /// bit 2). `false` (default) keeps the `$4016` read byte-identical.
    pub microphone: bool,
    /// The rewind gesture is held (already hardcore-gated by `App`).
    pub rewind_held: bool,
    /// RA hardcore gating is active (disables raw cheats; rewind is
    /// already folded into `rewind_held`).
    pub hardcore_blocked: bool,
    /// Configured run-ahead depth (0-3); the core further gates it on
    /// movie activity + the budget throttle.
    pub run_ahead: u32,
    /// Configured expansion device on the player-2 port.
    #[cfg(not(target_arch = "wasm32"))]
    pub expansion: ExpansionDevice,
    /// Cursor mapped to NES screen coordinates (`u16::MAX` = off-screen),
    /// for the Zapper / Vaus.
    #[cfg(not(target_arch = "wasm32"))]
    pub mouse_nes: (u16, u16),
    /// Left mouse button held (Zapper trigger / Vaus fire).
    #[cfg(not(target_arch = "wasm32"))]
    pub mouse_pressed: bool,
    /// v1.1.0 beta.1 (T-110-B2) — turbo/autofire mask: the buttons that
    /// rapid-fire while held (empty = off, byte-identical input). Applied at
    /// latch keyed on the emulated frame, to every player port.
    pub turbo_mask: Buttons,
    /// Frames the turbo buttons hold each on/off state (clamped to >= 1).
    pub turbo_period: u32,
    /// v1.1.0 beta.1 (T-110-B1) — Power Pad mat button mask (bit `i` = mat
    /// button `i+1`). Consumed only when the player-2 expansion device is a
    /// Power Pad.
    pub power_pad: u16,
    /// v1.2.0 Workstream F2 — whether the Power Pad is the active wasm touch
    /// device. On native the Power Pad selection lives in `expansion`
    /// (above); on wasm there is no `expansion` field, so this non-gated flag
    /// gates the wasm-only Power Pad latch arm in [`EmuCore::latch`].
    /// All-zero/`false` by default = byte-identical latch.
    #[cfg(target_arch = "wasm32")]
    pub power_pad_active: bool,
    /// v1.2.0 Workstream D — accumulated mouse motion this frame `(dx, dy)`,
    /// consumed only when the expansion device is a SNES mouse.
    #[cfg(not(target_arch = "wasm32"))]
    pub mouse_delta: (i16, i16),
    /// v1.2.0 Workstream D — right mouse button held (SNES mouse right button;
    /// the left button reuses `mouse_pressed`).
    #[cfg(not(target_arch = "wasm32"))]
    pub mouse_right: bool,
    /// v1.5.0 "Lens" Workstream D4 — SNES-mouse reported sensitivity (0 = low,
    /// 1 = medium, 2 = high), the 2-bit serial-report field. Default `0` (low)
    /// matches the previous hardcoded value, so the device report is
    /// byte-identical to a pre-D4 config.
    #[cfg(not(target_arch = "wasm32"))]
    pub mouse_sensitivity: u8,
    /// v1.2.0 Workstream D — Family BASIC keyboard pressed-key matrix bitmap
    /// (one byte per matrix row). Consumed only when the expansion device is a
    /// Family BASIC keyboard. All-zero (no keys) by default = byte-identical.
    #[cfg(not(target_arch = "wasm32"))]
    pub family_keyboard: [u8; 9],
    /// v1.3.0 Workstream F1 — Konami Hyper Shot button mask (bit 0 = P1 Run,
    /// 1 = P1 Jump, 2 = P2 Run, 3 = P2 Jump). Consumed only when the expansion
    /// device is a Konami Hyper Shot. 0 by default = byte-identical latch.
    #[cfg(not(target_arch = "wasm32"))]
    pub konami_hyper_shot: u8,
    /// v1.3.0 Workstream F1 — Bandai Hyper Shot sensor mask (bits 0..=3 = the
    /// A=0 group, 4..=7 = the A=1 group). Consumed only when the expansion
    /// device is a Bandai Hyper Shot. 0 by default = byte-identical latch.
    #[cfg(not(target_arch = "wasm32"))]
    pub bandai_hyper_shot: u8,
}

/// v1.1.0 beta.1 (T-110-B2) — apply turbo/autofire to one port's buttons.
///
/// The `mask` buttons strobe on/off while held; all other buttons pass through.
/// The phase is a pure function of the emulated `frame` number (and `period`),
/// so the result is deterministic and reproducible under rollback / TAS replay
/// — the gate is applied where input meets the NES, and the gated bits are what
/// get latched / recorded / sent over netplay.
#[must_use]
pub(crate) fn apply_turbo(buttons: Buttons, frame: u64, mask: Buttons, period: u32) -> Buttons {
    // Nothing to strobe if no turbo button is configured OR none is held.
    if mask.is_empty() || (buttons & mask).is_empty() {
        return buttons;
    }
    let period = u64::from(period.max(1));
    let on = (frame / period).is_multiple_of(2);
    if on { buttons } else { buttons & !mask }
}

/// Mutable borrows of the caller-resident sinks the produce path feeds.
///
/// The synchronous (winit-thread) drive passes the `!Send`
/// [`AudioOutput`](crate::audio::AudioOutput) + the live RA session; the
/// emulation thread passes its owned `Send`
/// [`AudioProducer`](crate::audio::AudioProducer) and `ra: None` (RA stays
/// on the winit thread — `rc_client` is single-threaded C — driven per
/// published frame via `drive_ra`).
pub struct FrameSinks<'a> {
    /// Audio sink (the DRC resampler stage lives behind it).
    #[cfg(not(target_arch = "wasm32"))]
    pub audio: Option<&'a mut dyn crate::audio::AudioSink>,
    /// `RetroAchievements` session (None when the emu thread produces).
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    pub ra: Option<&'a mut rustynes_ra::RaSession>,
    /// wasm has no app-resident sinks (Web Audio is a thread-local ring).
    #[cfg(target_arch = "wasm32")]
    pub _marker: core::marker::PhantomData<&'a ()>,
}

/// Side effects a produce call surfaced for `App` to act on (UI-thread
/// work: status pushes into the debugger, RA token persistence).
#[derive(Default)]
pub struct ProduceFx {
    /// Latest RA status snapshot for the debugger (refreshed per frame).
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    pub ra_status: Option<crate::debugger::CheevosStatusView>,
    /// The RA login completed this frame — `App` persists the token and
    /// (re-)identifies the loaded ROM.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    pub ra_just_logged_in: bool,
    /// v1.1.0 beta.2 (Workstream C) — a breakpoint fired this frame at this PC.
    /// `App` pauses emulation + opens the debugger so the user can inspect.
    pub breakpoint_hit: Option<u16>,
    /// v1.4.0 Workstream D (D2) — an event-driven breakpoint fired this frame.
    /// `App` pauses emulation + opens the debugger and reports the event kind +
    /// frame/cycle/scanline/dot context.
    pub event_break_hit: Option<rustynes_core::EventBreakHit>,
}

impl ProduceFx {
    /// Merge a later frame's effects into the accumulated set (statuses:
    /// last-wins; edges: sticky-OR).
    // On the default build (RA off) the body is empty — self/later unused
    // and const-able are cfg artifacts.
    #[allow(
        unused_variables,
        clippy::needless_pass_by_value,
        clippy::needless_pass_by_ref_mut,
        clippy::unused_self,
        clippy::missing_const_for_fn
    )]
    fn merge(&mut self, later: Self) {
        #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
        {
            if later.ra_status.is_some() {
                self.ra_status = later.ra_status;
            }
            self.ra_just_logged_in |= later.ra_just_logged_in;
        }
        if later.breakpoint_hit.is_some() {
            self.breakpoint_hit = later.breakpoint_hit;
        }
        if later.event_break_hit.is_some() {
            self.event_break_hit = later.event_break_hit;
        }
    }
}

/// The emulation core: the per-frame produce state extracted from `App`.
pub struct EmuCore {
    /// The running single-console emulator (None until a single-console ROM is
    /// loaded, or while a Vs. `DualSystem` cabinet is loaded — see [`Self::dual`]).
    pub nes: Option<Nes>,
    /// v2.3.3 F3 — the loaded ROM's mapper display name, cached at load.
    ///
    /// The status bar renders this every displayed frame. Reading it from
    /// `Nes::mapper_info()` instead costs **1,367 ns per frame measured**, because
    /// `Mapper::debug_info()` builds a whole debug structure — MMC3's runs ~25
    /// `format!` calls and four `Vec` allocations — of which the status bar keeps
    /// only `.name` and drops the rest. That happened *inside* `self.emu.lock()`,
    /// so it also widened the lock window the emulator thread contends for.
    ///
    /// This is **allocation hygiene, not an optimization**: 1.37 µs is 0.008% of
    /// a 16.639 ms frame and no frame-time change is claimed or expected (see
    /// `docs/performance.md` v2.3.3 F1). It removes ~1,500 discarded allocations
    /// per second because there is no reason to make them, not because they were
    /// costing anything measurable.
    ///
    /// Maintained by [`Self::set_nes`] / [`Self::set_dual`] / [`Self::clear_rom`]
    /// so it cannot drift from `nes`; do not assign `nes` directly.
    pub mapper_name: String,
    /// v2.1.2 F2.1 — a loaded Vs. `DualSystem` cabinet (two cross-wired consoles).
    /// Mutually exclusive with [`Self::nes`]: exactly one is `Some` while a ROM
    /// is loaded. Additive so the single-console produce/present/latch paths stay
    /// byte-identical; the dual path is a parallel branch at each chokepoint. The
    /// advanced frontend features (run-ahead, rewind, netplay, TAS, save-state)
    /// are scoped out while a cabinet is loaded (ADR 0032) — they snapshot a
    /// single `Nes` and are disabled in dual mode.
    pub dual: Option<Box<rustynes_core::VsDualSystem>>,
    /// TAS movie record/playback state machine.
    pub movie: MovieUi,
    /// Frame-pacing / audio instrumentation (Phase 0).
    pub perf: PerfStats,
    /// The framebuffer the renderer presents (with run-ahead active it is
    /// the visible FUTURE frame while `nes` holds the persistent one). In dual
    /// mode this holds the MAIN console's framebuffer (the sub console's is in
    /// [`Self::present_fb_sub`]); the present path composes the two per the
    /// configured layout.
    pub present_fb: Vec<u8>,
    /// v2.1.2 F2.1 — the SUB console's framebuffer in Vs. `DualSystem` mode
    /// (empty otherwise). Harvested each produced frame alongside [`Self::present_fb`]
    /// so the present path can compose the two-screen image from these two owned
    /// buffers. The compose runs under the same brief present lock as the
    /// single-console `present_fb` copy (a cheap memcpy, not heavy work).
    pub present_fb_sub: Vec<u8>,
    /// v1.8.9 — the 8 KiB CHR pattern space ($0000-$1FFF) captured at PRODUCE
    /// time (the same visible frame as `present_fb`). With run-ahead active, the
    /// `nes` is rolled back to the persistent frame after the visible frame is
    /// harvested, so peeking CHR at present time would read a 1-2-frame-stale
    /// snapshot and animated HD-pack tiles (e.g. fire) would flicker. Captured
    /// here it stays in lock-step with `present_fb` + the surviving tile-source
    /// telemetry. Empty unless [`Self::hd_capture`] is set (a pack is loaded).
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    pub hd_chr_snapshot: Vec<u8>,
    /// v1.8.9 — set by `App` while an HD-pack compositor is loaded; gates the
    /// produce-time CHR snapshot above (no cost when no pack is active).
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    pub hd_capture: bool,
    /// v1.7.0 "Forge" Workstream D1 — the scrubbable-session `HistoryViewer`: a
    /// per-frame input log + periodic start-anchors recorded in lock-step with
    /// the rewind ring, used to scrub the timeline and export the last N seconds
    /// as a `.rnm`. Output-only — it observes the inputs already latched and
    /// copies the save-states the core already produced, so it cannot perturb
    /// the deterministic timeline. Recorded on persistent forward frames only
    /// (never on a rewind step). Empty / unrecorded keeps the produce path
    /// byte-identical.
    pub history: crate::history_viewer::HistoryViewer,
    /// Enabled raw RAM cheats, pulled from the cheat panel each frame.
    pub raw_cheats: Vec<RawCheat>,
    /// v1.7.0 "Forge" Workstream A1 — one-shot debugger writeback edits, queued
    /// by the editing-capable debugger panels (tile/CHR, palette, nametable,
    /// OAM, hex). Drained in the SAME gated post-frame stage as `raw_cheats`,
    /// under the SAME write gate (`writes_locked` + `hardcore_blocked`), so the
    /// edits are a no-op under netplay / TAS replay/record / RA-hardcore. Empty
    /// (the no-edit default) makes the produce path byte-identical.
    pub debug_pokes: Vec<DebugPoke>,
    /// v1.7.0 "Forge" Workstream A1 — the combined write gate (`true` under
    /// netplay / TAS replay or record). `App` republishes it each frame from the
    /// EXACT same condition `emu.write` uses (T-110-E2). When `true`, the
    /// post-frame `debug_pokes` drain is skipped — locked = no-op = byte-identical.
    pub writes_locked: bool,
    /// Vs. System coin-hold countdown (frames until `clear_coin`).
    pub vs_coin_frames: u8,
    /// Per-region frame duration (NTSC ~16.639 ms, PAL/Dendy ~19.997 ms).
    /// This is the *console rate base* and never changes with the speed
    /// preset (region / display-sync / perf logic read it); the speed factor
    /// scales the PACING only, via [`Self::effective_frame_duration`].
    pub frame_duration: Duration,
    /// v1.0.0 — emulation-speed factor (transient, NOT persisted; always
    /// launches at 1.0). 200% (`2.0`) halves the effective frame period so
    /// the pacer produces twice the frames/sec; 50% (`0.5`) doubles it.
    /// Applied only at the pacing sites through
    /// [`Self::effective_frame_duration`] — `frame_duration` (the console
    /// rate) is left intact for region / display-sync / perf math.
    pub speed: f32,
    /// Wall-clock target for the next emulator frame.
    pub next_frame_time: Option<Instant>,
    /// Scratch buffer for draining APU samples toward the audio sink.
    pub audio_buf: Vec<f32>,
    /// Run-ahead scratch (persistent-frame snapshot + discard sink).
    #[cfg(not(target_arch = "wasm32"))]
    pub runahead: crate::runahead::RunAhead,
    /// Run-ahead budget throttle (hysteresis on the produce-cost MEDIAN — see
    /// `update_runahead_throttle`, which explains why p95 was the wrong signal).
    #[cfg(not(target_arch = "wasm32"))]
    pub runahead_throttled: bool,
    /// v2.3.3 F21 — how many depths the budget throttle has taken OFF the
    /// configured run-ahead, rather than whether it fired at all.
    ///
    /// The throttle used to be all-or-nothing: over budget, drop to depth 0.
    /// Measured at `run_ahead = 2` on a host that cannot afford it, that traded
    /// **8.57% -> 0.73%** of frames held for the wrong duration — a real win —
    /// but it bought it by disabling the feature outright, and the F18 sweep
    /// shows it did not have to. Depth 1 costs 52.1% of the frame budget and
    /// shows **1.7%** wrong: stepping 2 -> 1 keeps nearly all the cadence
    /// benefit AND one frame of latency reduction, where 2 -> 0 discards the
    /// latter for nothing.
    ///
    /// `runahead_throttled` is kept as `steps > 0` so the Performance panel and
    /// the perf log keep their existing meaning ("the throttle is doing
    /// something") without needing to understand the depth arithmetic.
    pub runahead_throttle_steps: u32,
    /// v2.3.3 F21 — cumulative run-ahead depth CHANGES, either direction.
    ///
    /// The only metric that sees the artefact matching the reported shudder: a
    /// depth change displaces the displayed frame by the run-ahead depth, so the
    /// picture jumps forward N frames and back. Every hold-duration statistic is
    /// blind to it, because the frames either side are each shown for exactly
    /// the right number of refreshes.
    ///
    /// **Counts depth STEPS, not visible displacements (v2.3.3 F28).** Since the
    /// engage arm may cascade, one evaluation can drop several depths in a single
    /// frame — which the viewer sees as ONE displacement, of several depths,
    /// while this counter advances once per depth. The two were the same number
    /// until F28 and are not any more. Read it as "how far the throttle moved",
    /// and pair it with [`Self::runahead_engages`] against
    /// [`Self::runahead_releases`] to tell a cascade from an oscillation: a
    /// cascade is engages arriving together with no release between them.
    /// Flagged in review on PR #371, where the doc still promised the older
    /// meaning.
    pub runahead_throttle_toggles: u64,
    /// v2.3.3 F24 — produced frames since the last throttle transition.
    ///
    /// `update_runahead_throttle` is called after EVERY produced frame but was
    /// only guarded on `samples < 120`, with no notion of whether the median
    /// window had advanced. One unchanged median could therefore drive a
    /// transition on every consecutive frame: depth 2 -> 1 -> 0 without ever
    /// MEASURING depth 1, and the release path had the identical defect.
    ///
    /// It also made the toggle counter untrustworthy. Several transitions
    /// against one stale median are indistinguishable from real oscillation, and
    /// F22's "3 engages / 2 releases, identical across three captures" is
    /// exactly what that artefact would produce — its reproducibility, which
    /// read as evidence the signal was genuine, is equally consistent with
    /// re-evaluating a single measurement. Raised in review on PR #368.
    ///
    /// **F27 — the counter was right, its threshold was wrong.** F24 gated on
    /// 120 frames, which is the ring's minimum-to-report, not its capacity of
    /// [`crate::perf::WINDOW`] = 600. So the guard permitted a second transition
    /// while 80% of the median still described the pre-transition depth, and the
    /// oscillation survived it unchanged — which is why F24 measured as no
    /// improvement and looked like a refutation of the stale-median theory when
    /// it was really an under-strength test of it.
    ///
    /// The per-evaluation log (F26) is what settled it. Transitions arrive in
    /// immediate pairs sharing a median to three decimals — `12.958` engaging
    /// twice, `4.994` releasing twice — so the second of each pair was decided
    /// on a measurement of the depth the first had just left.
    pub throttle_frames_since_change: usize,
    /// v2.3.3 F22 — depth changes that REDUCED run-ahead (the engage arm).
    ///
    /// `runahead_throttle_toggles` proved the oscillation exists (~3 depth
    /// changes per 45 s at `run_ahead = 2`) but one counter cannot say which arm
    /// fires, and the two have different causes. An engage means the MEASURED
    /// cost crossed the band; a release means the PREDICTED cost one depth up
    /// fell under it. Three toggles could be 2 engages + 1 release (the throttle
    /// chasing a rising cost), 1 + 2 (a release predictor that is too
    /// optimistic), or an alternating pair (genuine hysteresis failure) — three
    /// different defects, and the aggregate is consistent with all of them.
    ///
    /// Split rather than theorised about: two theory-first attempts at this
    /// oscillation have already been falsified by measurement today.
    ///
    /// Like [`Self::runahead_throttle_toggles`], this counts depth STEPS: an F28
    /// cascade that drops two depths in one evaluation advances it twice. That
    /// is the intended reading — the question this counter answers is how far
    /// the engage arm moved, and the convergence measurements in
    /// `docs/performance.md` are stated in those units.
    pub runahead_engages: u64,
    /// v2.3.3 F22 — depth changes that RESTORED run-ahead (the release arm).
    /// See [`Self::runahead_engages`].
    pub runahead_releases: u64,
    /// v2.3.3 F23 — the MEASURED median cost, ms, when the engage arm last fired.
    ///
    /// F22 established the oscillation alternates (3 engages / 2 releases,
    /// identical across three captures — deterministic, not noise). Alternation
    /// that survives a 20-point band and a four-window debounce says the two
    /// arms are not testing the same quantity: engage compares the MEASURED
    /// median at the current depth, release compares a PREDICTED cost one depth
    /// up. A band between two different quantities is not hysteresis.
    ///
    /// These three fields record what each arm actually saw, so that stops being
    /// an inference. If `engage_cost` and `release_cost` are near-identical while
    /// `release_pred` sits under the release band, the prediction is turning one
    /// cost into two verdicts and the arms need a common quantity — not a wider
    /// gap between them.
    pub thr_engage_cost_ms: f32,
    /// v2.3.3 F23 — the MEASURED median cost, ms, when the release arm last
    /// fired. See [`Self::thr_engage_cost_ms`].
    pub thr_release_cost_ms: f32,
    /// v2.3.3 F23 — the PREDICTED one-depth-up cost, ms, that the release arm
    /// accepted. See [`Self::thr_engage_cost_ms`].
    pub thr_release_pred_ms: f32,
    /// SHA-256 of the loaded FDS disk (keys the `.fds.sav` sidecar).
    #[cfg(not(target_arch = "wasm32"))]
    pub fds_disk_sha256: Option<[u8; 32]>,
    /// v1.2.0 (T-110-E2) — Lua `emu.setInput` per-port button override, applied
    /// at the next [`Self::latch`] (the deterministic late-latch point, the same
    /// place a real keypress enters) then consumed (one-shot per command).
    /// `None` (default) leaves `latch` byte-identical. Set ONLY by the host's
    /// `pump_scripts` AFTER the script's write-gate clears (netplay / TAS replay
    /// / RA-hardcore), so a locked / replayed session is never perturbed.
    #[cfg(feature = "scripting")]
    pub script_input_override: [Option<u8>; 2],
    /// v1.6.0 "Studio" Workstream G — active A/V recorder, armed via the
    /// Tools menu. A read-only tap: when `Some`, each produced frame's
    /// framebuffer + drained audio are *copied* into the recorder AFTER the
    /// emulator has produced them (it never advances the emulator or alters
    /// the per-frame output, so determinism is unaffected). Native-only +
    /// behind the default-OFF `av-record` feature; `None` (idle) is fully
    /// inert and byte-identical to a build without the feature.
    #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
    pub av_recorder: Option<crate::av_record::AvRecorder>,
    /// v1.6.0 "Studio" Workstream H — HD-pack HD-AUDIO mixer, installed by the
    /// host when a pack that declares `<bgm>`/`<sfx>` tracks loads. A read-only
    /// tap on the FRONTEND audio path: when `Some`, each produced frame the
    /// mixer peeks the `$4100` HD-audio-control register (a side-effect-free
    /// read of the already-produced bus state) and sums the selected OGG track
    /// into the drained APU buffer in place — AFTER the core handed the frame
    /// off, so it never advances the emulator or alters the deterministic
    /// per-frame audio. Native-only + behind the default-OFF `hd-pack` feature;
    /// `None` (idle, and the only state with no audio pack) is byte-identical to
    /// a build without HD audio.
    #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
    pub hd_audio: Option<crate::hd_audio::HdAudioMixer>,
    /// v1.7.0 "Forge" Workstream H4 — running count of "lag frames" since the
    /// ROM loaded: produced (non-rewind) frames in which the running program did
    /// NOT read a controller port (`$4016`/`$4017`). Sampled via the core's
    /// output-only `debug-hooks` `was_input_polled_this_frame()` telemetry AFTER
    /// each forward frame — a pure observation that never advances the emulator
    /// or perturbs the deterministic timeline. Reset to 0 on ROM load / reset /
    /// power-cycle. Displayed (off by default) in the status bar.
    pub lag_frames: u32,
}

impl EmuCore {
    /// Install a freshly-loaded single-console emulator, refreshing the cached
    /// [`Self::mapper_name`] in the same step.
    ///
    /// **Assign through this, never `emu.nes = Some(..)` directly.** The cache is
    /// only correct if every install refreshes it, and there are several install
    /// sites (ROM load, reset-with-new-config, netplay session start, movie
    /// load). Routing them through one setter makes that structural instead of a
    /// convention four call sites have to remember.
    pub fn set_nes(&mut self, nes: Nes) {
        self.mapper_name = nes.mapper_info().name;
        self.dual = None;
        self.nes = Some(nes);
    }

    /// Install a Vs. `DualSystem` cabinet, refreshing the cached mapper name from
    /// the MAIN console (the one whose status the shell reports).
    pub fn set_dual(&mut self, dual: Box<rustynes_core::VsDualSystem>) {
        self.mapper_name = dual.main().mapper_info().name;
        self.nes = None;
        self.dual = Some(dual);
    }

    /// Drop any loaded ROM and the cached name with it, so a stale mapper label
    /// cannot outlive the ROM it described.
    pub fn clear_rom(&mut self) {
        self.nes = None;
        self.dual = None;
        self.mapper_name.clear();
    }

    /// Empty core (no ROM).
    #[must_use]
    pub fn new() -> Self {
        Self {
            nes: None,
            mapper_name: String::new(),
            dual: None,
            movie: MovieUi::default(),
            perf: PerfStats::default(),
            present_fb: Vec::new(),
            present_fb_sub: Vec::new(),
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            hd_chr_snapshot: Vec::new(),
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            hd_capture: false,
            history: crate::history_viewer::HistoryViewer::default(),
            raw_cheats: Vec::new(),
            debug_pokes: Vec::new(),
            writes_locked: false,
            vs_coin_frames: 0,
            frame_duration: rustynes_core::FRAME_DURATION_NTSC,
            speed: 1.0,
            next_frame_time: None,
            audio_buf: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            runahead: crate::runahead::RunAhead::default(),
            #[cfg(not(target_arch = "wasm32"))]
            runahead_throttled: false,
            runahead_throttle_steps: 0,
            runahead_throttle_toggles: 0,
            // F27 — start "one window elapsed" so the FIRST decision is gated
            // only by the ring having enough samples to report. There is nothing
            // stale to wait out at power-on: the ring fills from empty at the
            // configured depth, so every sample in it already describes the
            // regime being judged. Starting at 0 would instead make a host that
            // genuinely cannot afford its configured depth run over budget for a
            // full 10 s window before the throttle was allowed to help it.
            throttle_frames_since_change: crate::perf::WINDOW,
            runahead_engages: 0,
            runahead_releases: 0,
            thr_engage_cost_ms: 0.0,
            thr_release_cost_ms: 0.0,
            thr_release_pred_ms: 0.0,
            #[cfg(not(target_arch = "wasm32"))]
            fds_disk_sha256: None,
            #[cfg(feature = "scripting")]
            script_input_override: [None, None],
            #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
            av_recorder: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
            hd_audio: None,
            lag_frames: 0,
        }
    }

    /// v1.7.0 "Forge" Workstream H4 — the lag-frame count since this ROM loaded
    /// (forward frames in which the program polled no controller). Pure
    /// observation; reset by [`Self::reset_lag_frames`] on ROM load / reset.
    #[must_use]
    pub const fn lag_frames(&self) -> u32 {
        self.lag_frames
    }

    /// Reset the lag-frame counter (called on ROM load / reset / power-cycle so
    /// the readout reflects only the current session).
    // A runtime state mutator, not a compile-time helper — deliberately not
    // `const fn` so it reads as the side-effecting reset it is.
    #[allow(clippy::missing_const_for_fn)]
    pub fn reset_lag_frames(&mut self) {
        self.lag_frames = 0;
    }

    /// Latch the per-pace input snapshot into the emulator (controllers +
    /// any expansion device). The single latest-possible point before
    /// `run_frame` consumes it.
    // const-able only on wasm (the expansion-device block is native-only).
    #[cfg_attr(target_arch = "wasm32", allow(clippy::missing_const_for_fn))]
    pub fn latch(&mut self, inputs: &FrameInputs) {
        // v1.2.0 (T-110-E2) — consume the Lua setInput override (one-shot per
        // command) regardless of whether a ROM is loaded, so a stale override
        // can never carry into a freshly-loaded ROM. Taken here so the override
        // enters at exactly the same point a real keypress does — the
        // deterministic late-latch. It is only ever SET (in `pump_scripts`) when
        // the script write-gate is clear, so under netplay / TAS replay /
        // RA-hardcore it stays `None` and this whole block is a no-op.
        #[cfg(feature = "scripting")]
        let script_override = core::mem::take(&mut self.script_input_override);
        // v2.1.2 F2.1 — Vs. `DualSystem` cabinet: P1/P2 drive the main console's
        // ports, P3/P4 the sub console's (`VsDualSystem::set_buttons` splits them).
        // The four-score flag does not apply — a cabinet always wires both
        // consoles' two ports — so all four are latched. Turbo keys off the main
        // console's frame for a reproducible strobe.
        if let Some(dual) = self.dual.as_mut() {
            let frame = dual.main().frame();
            let turbo = |b| apply_turbo(b, frame, inputs.turbo_mask, inputs.turbo_period);
            for port in 0..4 {
                dual.set_buttons(port, turbo(inputs.buttons[port]));
            }
            return;
        }
        if let Some(nes) = self.nes.as_mut() {
            // v1.1.0 beta.1 (T-110-B2) — gate turbo/autofire here, keyed on the
            // emulated frame, so the strobe is reproducible under rollback / TAS
            // and the latched bits are exactly what a real turbo pad would emit.
            let frame = nes.frame();
            let turbo = |b| apply_turbo(b, frame, inputs.turbo_mask, inputs.turbo_period);
            nes.set_buttons(0, turbo(inputs.buttons[0]));
            nes.set_buttons(1, turbo(inputs.buttons[1]));
            if inputs.four_score {
                nes.set_buttons(2, turbo(inputs.buttons[2]));
                nes.set_buttons(3, turbo(inputs.buttons[3]));
            }
            // v2.2.0 "Capstone" — Famicom microphone ($4016 bit 2). Latched at
            // the SAME point as the buttons; byte-identical when released.
            nes.set_microphone(inputs.microphone);
            // v1.2.0 (T-110-E2) — Lua setInput override: replace ports 0/1 AFTER
            // the keyboard/turbo latch, so the script's recorded bitmask wins for
            // this frame. Applied at the late-latch point, so a session that
            // replays this exact input stream stays bit-identical. Gated to never
            // set under lock (see the `take` above + `pump_scripts`).
            #[cfg(feature = "scripting")]
            for (port, ov) in script_override.iter().enumerate() {
                if let Some(bits) = ov {
                    nes.set_buttons(port, Buttons::from_bits_truncate(*bits));
                }
            }
            // v2.1.0 — feed the mouse into any attached non-standard device
            // on the player-2 port ($4017). Native-only (mouse input source).
            #[cfg(not(target_arch = "wasm32"))]
            {
                let (nx, ny) = inputs.mouse_nes;
                match inputs.expansion {
                    ExpansionDevice::None => {}
                    ExpansionDevice::Zapper => {
                        nes.set_zapper(1, nx, ny, inputs.mouse_pressed);
                    }
                    ExpansionDevice::Vaus => {
                        #[allow(clippy::cast_possible_truncation)]
                        let pos = if nx == u16::MAX {
                            0x80
                        } else {
                            nx.min(255) as u8
                        };
                        nes.set_paddle(1, pos, inputs.mouse_pressed);
                    }
                    ExpansionDevice::PowerPad => {
                        nes.set_power_pad(1, inputs.power_pad);
                    }
                    ExpansionDevice::SnesMouse => {
                        let (dx, dy) = inputs.mouse_delta;
                        // v1.5.0 D4 — reported sensitivity is now configurable
                        // (was hardcoded 0); default 0 keeps the report
                        // byte-identical.
                        nes.set_snes_mouse(
                            1,
                            dx,
                            dy,
                            inputs.mouse_pressed,
                            inputs.mouse_right,
                            inputs.mouse_sensitivity.min(2),
                        );
                    }
                    ExpansionDevice::FamilyKeyboard => {
                        nes.set_family_keyboard(1, inputs.family_keyboard);
                    }
                    ExpansionDevice::FamilyTrainer => {
                        nes.set_family_trainer(1, inputs.power_pad);
                    }
                    ExpansionDevice::SuborKeyboard => {
                        nes.set_subor_keyboard(1, inputs.family_keyboard);
                    }
                    ExpansionDevice::KonamiHyperShot => {
                        nes.set_konami_hyper_shot(1, inputs.konami_hyper_shot);
                    }
                    ExpansionDevice::BandaiHyperShot => {
                        nes.set_bandai_hyper_shot(1, inputs.bandai_hyper_shot);
                    }
                }
            }
            // v1.2.0 Workstream F2 — Power Pad on wasm. The native expansion
            // block above is gated out on wasm (it needs the cursor / mouse
            // fields, which don't exist there); the Power Pad only needs the
            // non-gated `power_pad` u16 mask, so feed it here at the SAME
            // late-latch point. `set_power_pad` self-attaches the mat on port
            // 1; the `power_pad_active` gate keeps the no-Power-Pad path
            // byte-identical (the arm is a no-op when the touch UI has not
            // selected the device).
            #[cfg(target_arch = "wasm32")]
            if inputs.power_pad_active {
                nes.set_power_pad(1, inputs.power_pad);
            }
        }
    }

    /// v2.8.0 Phase 3 — the run-ahead depth to use for the next frame.
    ///
    /// 0 (plain frame) when: configured off; movie recording/playback is
    /// active (the hidden frames would consume movie inputs); or the budget
    /// throttle is engaged. Netplay never reaches this path (it is driven
    /// from `App` and never calls the core produce).
    #[cfg(not(target_arch = "wasm32"))]
    fn effective_run_ahead(&self, configured: u32) -> u32 {
        if self.movie.status().mode != crate::movie_ui::MovieMode::Idle {
            return 0;
        }
        // F21 — step DOWN, do not zero. `saturating_sub` bottoms out at 0, so a
        // host that cannot afford any depth still ends up there; it just takes
        // one median window per step instead of arriving in one jump.
        configured
            .min(MAX_RUN_AHEAD_DEPTH)
            .saturating_sub(self.runahead_throttle_steps)
    }

    /// v2.8.0 Phase 3 — run-ahead budget throttle with hysteresis, fed by
    /// the produce-cost **median** (which INCLUDES the run-ahead frames).
    /// Engages above `RUNAHEAD_THROTTLE_ENGAGE` of the frame budget (a plain
    /// code span, not an intra-doc link: the const is private and linking to it
    /// from public docs fails the `-D warnings` rustdoc gate), and releases a
    /// step when the PREDICTED cost one depth up falls below 70%.
    ///
    /// **The two arms are deliberately asymmetric (v2.3.3 F28).** Engaging may
    /// drop SEVERAL depths within one evaluation: it steps while the predicted
    /// cost at the reduced depth is still over the band, so it lands on the
    /// depth that fits rather than taking one window per step. Releasing gives
    /// back exactly one step and only after a full median window, because
    /// releasing on a stale median is what produced the F27 oscillation.
    /// Prediction is trusted only in the direction where being wrong is
    /// corrected by the other arm, within a window.
    ///
    /// This doc has twice described a contract the code no longer had — it said
    /// "releases below 40%" after the release test moved to predicting the
    /// re-enabled cost, and "engages one step at a time" after F28 made the
    /// engage arm cascade. Both were caught in review rather than by a gate;
    /// the contract and the code change together or the doc is a liability.
    ///
    /// **The hysteresis is known to be insufficient.** Measured at
    /// `run_ahead = 2`, the throttle changes depth ~3 times per 45 s — and each
    /// change displaces the displayed frame by the run-ahead depth, which the
    /// user sees as the picture jumping forward and back. Widening the band to
    /// 55% and debouncing the release over four windows was tried and measured
    /// WORSE on both counts (toggles unchanged, cadence 0.84% → 2.21-3.56% of
    /// frames held for the wrong duration), so it was reverted: the cause lies
    /// elsewhere. `runahead_throttle_toggles` is the metric to watch, and this
    /// is an open defect, not a solved one.
    ///
    /// v2.8.0 Phase 5 — keyed off the MEDIAN, not the p95. On the dedicated
    /// emulation thread the p95/p99 tail is dominated by occasional OS
    /// descheduling (the thread's priority is best-effort), NOT by
    /// run-ahead's compute cost — so a p95 gate threw the throttle on a
    /// machine with 4-5 ms steady-state produce and ~12 ms of headroom,
    /// needlessly disabling the latency feature. The median is the true
    /// steady-state budget signal; a genuinely heavy game (median produce
    /// approaching the budget with run-ahead's 2x) still throttles
    /// correctly, while a deschedule spike no longer does.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn update_runahead_throttle(&mut self, produce_p50_ms: f32, samples: usize, depth: u32) {
        if samples < THROTTLE_MIN_SAMPLES {
            return;
        }
        // F24, corrected by F27 — one transition per median WINDOW, not per
        // frame. F24's reasoning was right and its constant was wrong: it gated
        // on 120 frames because 120 is the number of samples the ring needs
        // before it reports at all, and mistook that MINIMUM for the ring's
        // CAPACITY. The ring holds `perf::WINDOW` = 600. After 120 produced
        // frames, 480 of the 600 samples still predate the transition, and the
        // p50 sits at index 300 -- so the median remains DOMINATED by the old
        // regime, and where the two regimes separate cleanly (which is the case
        // that matters here: depth N and depth N-1 differ by a whole emulated
        // frame, ~4.3 ms) the reported p50 is still literally a pre-transition
        // sample. It is not that 20% turnover cannot move the median in general
        // -- overlapping distributions would shift it -- it is that it cannot
        // move it off the wrong regime in the case this gate exists to handle.
        // The gate waited a fifth of a window and called it one.
        //
        // Expressed in terms of the ring it reads, so the two cannot drift
        // again. That drift is the entire defect: a magic number here had no
        // stated relationship to the statistic it was supposed to be pacing.
        self.throttle_frames_since_change = self.throttle_frames_since_change.saturating_add(1);
        if self.throttle_frames_since_change < crate::perf::WINDOW {
            return;
        }
        let target = self.frame_duration.as_secs_f32() * 1000.0;
        // v2.3.3 F26 — per-EVALUATION logging of the release predicate.
        //
        // F22 and F23 record the last transition; three successive theories have
        // now died on inference from that single sample (an over-optimistic
        // predictor, stale medians, an unrepresentative dip -- each tested and
        // each falsified). What none of them could see is the SEQUENCE: how the
        // measured cost, the prediction and the band move across the windows
        // BETWEEN transitions, including every evaluation that decided to do
        // nothing.
        //
        // Emitted on stderr rather than through the perf log because it is one
        // line per median window (~0.5/s), gated on the same env var as the
        // frame trace, and needs no new plumbing on a path that already has
        // enough. Redirect stderr to capture it.
        let trace_throttle = crate::perf_log::frame_trace_requested();
        let bounded = depth.min(MAX_RUN_AHEAD_DEPTH);
        let running = bounded.saturating_sub(self.runahead_throttle_steps);
        // Evaluated ONCE and used by both the log and the branch. Raised by the
        // Antigravity reviewer on PR #369, and it is the right call for a reason
        // specific to this function: an instrument that recomputes the predicate
        // it reports can drift from the predicate that acts, and every wrong
        // turn in this campaign has come from a measurement that did not
        // describe the thing it was believed to describe.
        let engage_band = target * RUNAHEAD_THROTTLE_ENGAGE;
        let engage = running > 0 && produce_p50_ms > engage_band;
        if trace_throttle {
            eprintln!(
                "THR check depth={running} steps={} cost={produce_p50_ms:.3} \
                 engage_band={engage_band:.3} engage={engage}",
                self.runahead_throttle_steps,
            );
        }
        if engage {
            // F21 — one step, not all of them. Re-measured on the next median
            // window; if the reduced depth is still over budget this fires
            // again, so an unaffordable host converges to 0 without the cliff.
            self.runahead_throttle_steps += 1;
            self.runahead_throttled = true;
            self.runahead_throttle_toggles += 1;
            self.throttle_frames_since_change = 0;
            self.runahead_engages += 1;
            self.thr_engage_cost_ms = produce_p50_ms;
            // F28 `Predict` — keep stepping while the PREDICTED cost at the
            // reduced depth is still over the band, using F18's per-frame-linear
            // model (increments +4.491 and +4.213 ms, equal within 6%) that the
            // release arm already relies on. This is the whole asymmetry: the
            // engage direction stops waiting for the ring and computes what the
            // next depth costs instead, while the release direction still
            // demands a real measurement.
            //
            // It converges to the SAME depth a sequence of windowed engages
            // would reach — the cascade stops when the prediction fits, not at
            // zero — but reaches it inside one evaluation instead of one window
            // per step. A mispredicted overshoot is corrected by the release arm
            // within a window, and only ever in the safe direction.
            //
            // The arithmetic runs in `f64` so every conversion is BOTH infallible
            // and lossless: `f64: From<u32>` and `f64: From<f32>` are total, so
            // there is no fallible cast to swallow and no precision to lose on
            // values that are millisecond costs and depths of 0..=3. The obvious
            // `f32::from(u8::try_from(d).unwrap_or(0))` is worse than it looks --
            // the fallback is unreachable today, and if `MAX_RUN_AHEAD_DEPTH`
            // ever exceeded 255 it would substitute a depth of ZERO, which
            // predicts a cost of one frame and silently stops the cascade at the
            // most expensive depth. A fallback whose failure mode is "keep the
            // configuration that is over budget" is worse than no fallback.
            // Raised by both reviewers on PR #371.
            let band = f64::from(engage_band);
            let per_frame = f64::from(produce_p50_ms) / (f64::from(running) + 1.0);
            while self.runahead_throttle_steps < bounded {
                let from = bounded.saturating_sub(self.runahead_throttle_steps);
                let predicted = per_frame * (f64::from(from) + 1.0);
                if predicted <= band {
                    break;
                }
                self.runahead_throttle_steps += 1;
                self.runahead_throttle_toggles += 1;
                self.runahead_engages += 1;
                if trace_throttle {
                    // `predicted` describes the depth being LEFT, so the log names
                    // both ends rather than printing a post-increment step count
                    // beside a pre-increment prediction. An instrument that
                    // mislabels which depth a number belongs to is the exact
                    // failure this campaign has corrected twice.
                    let to = bounded.saturating_sub(self.runahead_throttle_steps);
                    eprintln!(
                        "THR cascade {from} -> {to}: predicted={predicted:.3} at depth {from} \
                         exceeds band={band:.3}"
                    );
                }
            }
            let now_at = bounded.saturating_sub(self.runahead_throttle_steps);
            eprintln!(
                "rustynes: median produce cost {produce_p50_ms:.2} ms is too close to the \
                 {target:.2} ms frame budget — run-ahead reduced to depth {now_at}."
            );
        } else if self.runahead_throttle_steps > 0 {
            // v2.3.3 F6 — the release test must be in the SAME units as the
            // engage test, i.e. cost *with run-ahead re-enabled*. Comparing the
            // throttled (run-ahead-off) cost against a fixed 40% band compares
            // two different quantities, and the 85%/40% gap does not span them
            // — it sits BETWEEN them. At depth 2, cost is ~3x base with
            // run-ahead on and 1x base with it off, so any ROM whose base cost
            // lands between ~28% and 40% of budget engages at 3x, immediately
            // reads under 40% at 1x, releases, and re-engages: a ~2 s
            // oscillation (the median window). Each toggle shifts the displayed
            // frame by the run-ahead depth, so the picture jumps forward N
            // frames and back again — measured on Bad Dudes at three toggles
            // per 45 s. Predicting the re-enabled cost makes the comparison
            // honest and the hysteresis real.
            // Clamped to the depth that will actually RUN. `effective_run_ahead`
            // caps at 3, but `run_ahead` is an unvalidated `u32` straight out of
            // serde — nothing in `config.rs` clamps it — so a config saying
            // `run_ahead = 10` reached here as 10 and predicted an 11x cost
            // against a 4x reality, leaving run-ahead throttled forever. The
            // comment this replaces asserted the value was "config-clamped",
            // which was simply not true. (PR #357 review, found post-merge.)
            // F21 — predict the cost of giving back ONE step, not of restoring
            // the full configured depth. The cost model is per-frame-linear
            // (F18: +4.49 and +4.21 ms per depth, equal within 6%), so one more
            // frame is `cost / (running + 1) * (running + 2)`.
            // `f64` for the same reason as the engage cascade: total, lossless
            // conversions with no fallible cast to swallow. This arm carried the
            // `u8::try_from(..).unwrap_or(0)` pattern first and the engage arm
            // inherited it; the fallback substitutes a depth of zero, which here
            // would predict the cost of a SINGLE frame and release into a
            // configuration that cannot afford it. Fixed in both places at once
            // rather than only in the code review happened to be looking at.
            let per_frame = f64::from(produce_p50_ms) / (f64::from(running) + 1.0);
            let predicted_one_more = per_frame * (f64::from(running) + 2.0);
            let release_band = f64::from(target) * f64::from(RUNAHEAD_THROTTLE_RELEASE);
            let release = predicted_one_more < release_band;
            if trace_throttle {
                eprintln!(
                    "THR eval depth={running} steps={} cost={produce_p50_ms:.3} \
                     per_frame={per_frame:.3} pred={predicted_one_more:.3} \
                     band={release_band:.3} release={release}",
                    self.runahead_throttle_steps,
                );
            }
            // v2.3.3 F21 — band left at 0.70 and NOT debounced. Widening it to
            // 0.55 with a 4-window debounce was tried to stop the oscillation
            // and MEASURED WORSE on both counts: toggles stayed at 3 per 45 s
            // capture (target was 1) and cadence regressed from 0.84% to
            // 2.21-3.56% of frames held for the wrong duration. Reverted rather
            // than kept on the strength of its reasoning. The oscillation is a
            // real, open defect — see `runahead_throttle_toggles` — but this was
            // not its cause, so a wider band only removed a release path the
            // build needed.
            if release {
                self.runahead_throttle_toggles += 1;
                self.throttle_frames_since_change = 0;
                self.runahead_releases += 1;
                self.thr_release_cost_ms = produce_p50_ms;
                // Narrowed only for the diagnostic snapshot: the DECISION above
                // is made in f64, and this field exists to be formatted to three
                // decimals in a CSV column. The value is a millisecond cost in
                // the low tens, so f32 holds it to far more precision than the
                // column prints.
                // On a `let`, not on the assignment: attributes on expressions
                // are still unstable (E0658), so `#[allow]` cannot sit directly
                // on `self.x = ...`. A binding takes it on stable and needs no
                // wrapper block.
                #[allow(
                    clippy::cast_possible_truncation,
                    reason = "diagnostic snapshot of a ~10 ms value printed to 3dp"
                )]
                let pred_ms = predicted_one_more as f32;
                self.thr_release_pred_ms = pred_ms;
                self.runahead_throttle_steps -= 1;
                self.runahead_throttled = self.runahead_throttle_steps > 0;
                let now_at = bounded.saturating_sub(self.runahead_throttle_steps);
                eprintln!(
                    "rustynes: produce cost recovered ({produce_p50_ms:.2} ms, ~{predicted_one_more:.2} ms \
                     one depth up) — run-ahead restored to depth {now_at}."
                );
            }
        }
    }

    /// v1.8.9 — peek the HD-pack HD-audio register file `$4100..=$4106`
    /// (side-effect-free) into the array `apply_registers` expects.
    #[cfg(feature = "hd-pack")]
    fn peek_hd_audio_regs(nes: &mut Nes) -> [u8; 7] {
        core::array::from_fn(|i| {
            nes.cpu_bus_peek(crate::hd_audio::HD_AUDIO_CONTROL + u16::try_from(i).unwrap_or(0))
        })
    }

    /// v1.8.9 — snapshot the 8 KiB CHR pattern space from the VISIBLE frame at
    /// produce time, so the HD-pack composite hashes tiles against the same frame
    /// it presents (`present_fb`). Without this, run-ahead rolls `nes` back after
    /// the visible frame is harvested and a present-time CHR peek would be stale,
    /// flickering animated HD tiles. No-op unless a pack is loaded (`hd_capture`).
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    fn capture_hd_chr(snapshot: &mut Vec<u8>, capture: bool, nes: &mut Nes) {
        if !capture {
            return;
        }
        if snapshot.len() != 0x2000 {
            snapshot.resize(0x2000, 0);
        }
        for (addr, slot) in (0u16..0x2000).zip(snapshot.iter_mut()) {
            *slot = nes.peek_ppu(addr);
        }
    }

    /// Advance the emulator by exactly one SINGLE-PLAYER frame and push the
    /// produced audio into the sink. Caller is responsible for the
    /// wall-clock schedule and for routing netplay AROUND this method.
    #[allow(clippy::too_many_lines)]
    // the verbatim produce path; splitting hurts auditability.
    // `sinks` is consumed by the native audio/RA paths only; on wasm the
    // audio sink is the thread-local Web Audio ring.
    #[cfg_attr(target_arch = "wasm32", allow(unused_variables))]
    pub fn produce_one_frame(
        &mut self,
        inputs: &FrameInputs,
        sinks: &mut FrameSinks<'_>,
    ) -> ProduceFx {
        // `mut` is only exercised on the RA-feature build (fx stays default
        // otherwise).
        #[allow(unused_mut)]
        let mut fx = ProduceFx::default();
        // v2.1.2 F2.1 — a loaded Vs. `DualSystem` cabinet takes a parallel, much
        // simpler produce path: step both consoles, harvest both framebuffers,
        // push the MAIN console's audio. The advanced single-`Nes` features
        // (run-ahead, rewind, TAS, breakpoints, HD-pack, A/V record) are scoped
        // out in dual mode (ADR 0032) — `dual` and `nes` are mutually exclusive,
        // so the whole single path below is dead when a cabinet is loaded. Dual
        // is native-only (the wasm frontend has no dual present path).
        #[cfg(not(target_arch = "wasm32"))]
        if self.dual.is_some() {
            self.produce_dual_frame(sinks);
            return fx;
        }
        let hardcore_blocked = inputs.hardcore_blocked;
        // v2.8.0 Phase 3 — resolve the run-ahead depth before borrowing
        // `nes`. 0 = plain frame.
        #[cfg(not(target_arch = "wasm32"))]
        let run_ahead_n = self.effective_run_ahead(inputs.run_ahead);
        // v1.6.0 "Studio" Workstream G — count of audio samples drained into
        // `audio_buf` this frame, so the A/V recorder can tap the same samples
        // after the `nes` borrow ends (audio + video captured together at the
        // tail, keeping them synchronized). 0 = no audio this frame (rewind /
        // no audio sink).
        #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
        let mut av_audio_n: usize = 0;
        let Some(nes) = self.nes.as_mut() else {
            return fx;
        };
        // v2.7.0 — RetroAchievements hardcore mode disables rewind (already
        // folded into `inputs.rewind_held` by `App`).
        let rewinding = inputs.rewind_held;
        if rewinding {
            // Rewind is a live-only gesture; it never advances a movie
            // cursor or captures a frame.
            // v2.3.2 "Lucid": a SUCCESSFUL rewind moves the emulator off the
            // timeline any in-progress attestation describes, while the input
            // log keeps its full prefix — so the frame counts stay consistent
            // and `Movie::verify` would report `Mismatch` on an honest
            // recording. Drop the attestation instead: "not attested" is the
            // truthful outcome. (Review catch on PR #356.)
            if nes.rewind_step_back() {
                self.movie.invalidate_attestation();
            }
            // v2.8.0 Phase 3 — refresh the presented framebuffer from the
            // restored state.
            self.present_fb.clear();
            self.present_fb.extend_from_slice(nes.framebuffer());
        } else {
            // v1.4.0 Sprint 4.2 — TAS movie hook, AFTER the live
            // `set_buttons` latch and BEFORE `run_frame`. When recording it
            // captures the held input; when playing it overrides the held
            // input with the movie's recorded input for this frame. A
            // `false` return means the movie is exhausted — stop playback
            // and fall through to a normal live frame.
            if !self.movie.before_frame(nes) {
                self.movie.stop_playback();
                eprintln!("rustynes: movie playback finished");
            }
            // v1.7.0 "Forge" Workstream D1 — record this forward frame into the
            // HistoryViewer timeline, at the SAME point as the TAS movie hook
            // (after the input latch + any movie override, BEFORE `run_frame`),
            // mirroring `MovieRecorder::capture`: the captured `(state-before,
            // input-for-this-frame)` pair makes an exported clip replay
            // bit-identically. Disjoint borrow (`self.movie`/`self.history` vs
            // `self.nes`). Output-only — it reads the latched input + copies the
            // pre-frame save-state, so it never perturbs emulation. Skipped on
            // rewind (this branch is the non-rewind path).
            self.history.record_frame(nes);
            // v2.5.0 — Vs. System coin latch: a coin-insert (F10) holds the
            // acceptor signal for a few frames, then auto-clears.
            if self.vs_coin_frames > 0 {
                self.vs_coin_frames -= 1;
                if self.vs_coin_frames == 0 {
                    nes.clear_coin();
                }
            }
            // v2.8.0 Phase 3 — run-ahead (native): run the persistent frame
            // + N hidden/visible frames, harvest the VISIBLE frame's
            // framebuffer + audio, then roll back to the persistent frame.
            #[cfg(not(target_arch = "wasm32"))]
            let ran_ahead = run_ahead_n > 0 && {
                self.runahead.run_frame_ahead(nes, run_ahead_n);
                self.present_fb.clear();
                self.present_fb.extend_from_slice(nes.framebuffer());
                // v1.8.9 — capture CHR from the VISIBLE frame BEFORE `finish()`
                // rolls back, so animated HD-pack tiles stay in sync (no flicker).
                #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                Self::capture_hd_chr(&mut self.hd_chr_snapshot, self.hd_capture, nes);
                if let Some(audio) = sinks.audio.as_mut() {
                    let target = ((u64::from(audio.sample_rate()) / 50) as usize).max(1024);
                    if self.audio_buf.len() < target {
                        self.audio_buf.resize(target, 0.0);
                    }
                    let n = nes.drain_audio_into(&mut self.audio_buf);
                    // v1.6.0 H / v1.8.9 — HD-pack HD audio: peek the full
                    // `$4100..=$4106` register file (side-effect-free) and mix the
                    // selected OGG track into the drained buffer IN PLACE before it
                    // reaches the queue. Output-only (see `hd_audio` docs); skipped
                    // when no audio pack is loaded.
                    #[cfg(feature = "hd-pack")]
                    if let Some(mixer) = self.hd_audio.as_mut() {
                        let regs = Self::peek_hd_audio_regs(nes);
                        mixer.mix_registers(&mut self.audio_buf[..n], regs);
                    }
                    audio.push_samples(&self.audio_buf[..n]);
                    // v1.6.0 G — record the same samples for the A/V tap (after
                    // the `nes` borrow ends, at the tail).
                    #[cfg(feature = "av-record")]
                    {
                        av_audio_n = n;
                    }
                }
                self.runahead.finish(nes);
                true
            };
            #[cfg(target_arch = "wasm32")]
            let ran_ahead = false;

            if !ran_ahead {
                nes.run_frame();
                // v1.1.0 beta.2 (Workstream C) — surface a breakpoint hit so
                // `App` can pause + open the debugger. (Run-ahead's speculative
                // frames don't check breakpoints — only this persistent path.)
                fx.breakpoint_hit = nes.take_break_hit();
                // v1.4.0 Workstream D (D2) — surface an event-breakpoint hit so
                // `App` can pause + open the debugger (same persistent-path-only
                // policy as exec breakpoints; run-ahead's speculative frames
                // don't check it).
                fx.event_break_hit = nes.take_event_break_hit();
                // v2.8.0 Phase 3 — harvest the presented framebuffer into a
                // reused buffer.
                self.present_fb.clear();
                self.present_fb.extend_from_slice(nes.framebuffer());
                // v1.8.9 — keep the HD-pack CHR snapshot in lock-step with the
                // presented frame (uniform with the run-ahead path above).
                #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                Self::capture_hd_chr(&mut self.hd_chr_snapshot, self.hd_capture, nes);
                // v2.3.2 "Lucid" — fold this frame's output into the movie
                // attestation. Deliberately ONLY on this path: the run-ahead
                // branch above presents the frame N ahead of the persistent
                // timeline, and a verification replay (which has no run-ahead)
                // re-derives persistent frames — so attesting the presented image
                // would record a hash nobody can reproduce. Skipping those frames
                // leaves the attestation short, which `Movie::deserialize` detects
                // and discards the tail for: the failure mode is "no attestation",
                // never "a wrong one".
                self.movie.after_frame(&self.present_fb);

                #[cfg(not(target_arch = "wasm32"))]
                if let Some(audio) = sinks.audio.as_mut() {
                    let target = ((u64::from(audio.sample_rate()) / 50) as usize).max(1024);
                    if self.audio_buf.len() < target {
                        self.audio_buf.resize(target, 0.0);
                    }
                    let n = nes.drain_audio_into(&mut self.audio_buf);
                    // v1.6.0 H / v1.8.9 — HD-pack HD audio: peek the full
                    // `$4100..=$4106` register file (side-effect-free) and mix the
                    // selected OGG track into the drained buffer IN PLACE before
                    // the DRC stage. Output-only; skipped when no audio pack.
                    #[cfg(feature = "hd-pack")]
                    if let Some(mixer) = self.hd_audio.as_mut() {
                        let regs = Self::peek_hd_audio_regs(nes);
                        mixer.mix_registers(&mut self.audio_buf[..n], regs);
                    }
                    // v2.8.0 Phase 1 — through the DRC resampler stage.
                    audio.push_samples(&self.audio_buf[..n]);
                    // v1.6.0 G — record the same samples for the A/V tap.
                    #[cfg(feature = "av-record")]
                    {
                        av_audio_n = n;
                    }
                }
                // wasm32 (Sprint 1.4c): push this frame's APU samples into
                // the shared Web Audio ring.
                #[cfg(target_arch = "wasm32")]
                crate::wasm_audio::push_samples(&nes.drain_audio());
            }

            // v1.7.0 — apply the enabled raw RAM cheats AFTER the frame,
            // caller-side. With run-ahead the pokes land on the PERSISTENT
            // state (post-rollback). Disabled under RA hardcore.
            if !hardcore_blocked {
                for cheat in &self.raw_cheats {
                    match cheat.compare {
                        Some(c) if nes.bus_mut().debug_peek_cpu(cheat.address) != c => {}
                        _ => nes.poke_ram(cheat.address, cheat.value),
                    }
                }
            }
            // v1.7.0 "Forge" Workstream A1 — apply the queued debugger writeback
            // edits, AFTER the frame, in the same caller-side stage as the raw
            // cheats. Gated EXACTLY like `emu.write`: a no-op under netplay / TAS
            // replay/record (`writes_locked`) and under RA-hardcore
            // (`hardcore_blocked`). One-shot: drained, so a later replay/record
            // sees no residual perturbation. The empty (no-edit) queue keeps the
            // produce path byte-identical.
            if !self.writes_locked && !hardcore_blocked {
                for poke in self.debug_pokes.drain(..) {
                    match poke {
                        DebugPoke::CpuRam { addr, value } => nes.poke_ram(addr, value),
                        DebugPoke::PpuBus { addr, value } => nes.debug_poke_ppu(addr, value),
                        DebugPoke::Oam { idx, value } => nes.poke_oam_byte(idx, value),
                    }
                }
            } else {
                // Locked / hardcore: discard queued edits so they never leak into
                // a later unlocked frame (locked = no-op, not deferred).
                self.debug_pokes.clear();
            }
            // v1.7.0 "Forge" Workstream H4 — tally a lag frame when the program
            // polled no controller this forward frame. `was_input_polled_this_frame`
            // is the core's output-only `debug-hooks` telemetry (the frontend always
            // builds the core with `debug-hooks` on); reading it is a pure
            // observation — it neither advances the emulator nor alters the
            // per-frame output, so determinism is unaffected. Counted on forward
            // frames only (this is the non-rewind branch); a rewind step never
            // re-runs a frame, so it cannot create or undo a lag verdict.
            if !nes.was_input_polled_this_frame() {
                self.lag_frames = self.lag_frames.saturating_add(1);
            }
        }

        // v2.7.0 — drive RetroAchievements after the frame. Only the
        // synchronous (winit-thread) drive passes a session; the emulation
        // thread leaves `ra` None and drives RA itself per published frame.
        #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
        {
            if let Some(ra) = sinks.ra.as_deref_mut() {
                fx.ra_status = Some(drive_ra(self.nes.as_mut(), ra, !rewinding));
                fx.ra_just_logged_in = rustynes_ra::RaSession::take_just_logged_in(ra);
            }
        }
        // v1.6.0 "Studio" Workstream G — A/V recording tap. A read-only copy of
        // the framebuffer + the same audio samples this frame produced into the
        // recorder (audio then video, captured together here AFTER the `nes`
        // borrow ends so they stay synchronized). A broken pipe (ffmpeg died)
        // auto-stops + drops the recorder. This runs AFTER the emulator has
        // fully produced the frame, so it cannot perturb the emulation or the
        // per-frame output.
        #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
        {
            if av_audio_n > 0 {
                self.av_capture_audio(av_audio_n);
            }
            self.av_capture_video();
        }
        fx
    }

    /// v2.1.2 F2.1 — produce one frame of a Vs. `DualSystem` cabinet.
    ///
    /// Steps both cross-wired consoles (`VsDualSystem::run_frame`), harvests the
    /// MAIN framebuffer into [`Self::present_fb`] and the SUB framebuffer into
    /// [`Self::present_fb_sub`] (the present path composes them per the configured
    /// layout without holding the emu lock), and pushes the MAIN console's audio
    /// to the sink. The SUB console's audio is drained and discarded so its APU
    /// sample buffer cannot grow without bound. This path deliberately omits the
    /// single-`Nes` machinery (run-ahead / rewind / TAS / breakpoints / HD-pack /
    /// A/V record), which is scoped out in dual mode (ADR 0032). Native only.
    #[cfg(not(target_arch = "wasm32"))]
    fn produce_dual_frame(&mut self, sinks: &mut FrameSinks<'_>) {
        // v2.5.0 / F2.1 — Vs. System coin latch: a coin-insert holds the acceptor
        // for a few frames, then auto-clears (uniform with the single path).
        let clear_coin = self.vs_coin_frames > 0 && {
            self.vs_coin_frames -= 1;
            self.vs_coin_frames == 0
        };
        let Some(dual) = self.dual.as_mut() else {
            return;
        };
        if clear_coin {
            dual.clear_coin();
        }
        dual.run_frame();
        self.present_fb.clear();
        self.present_fb.extend_from_slice(dual.main_framebuffer());
        self.present_fb_sub.clear();
        self.present_fb_sub
            .extend_from_slice(dual.sub_framebuffer());
        // Push the MAIN console's audio; the frontend presents one audio stream.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(audio) = sinks.audio.as_mut() {
            let target = ((u64::from(audio.sample_rate()) / 50) as usize).max(1024);
            if self.audio_buf.len() < target {
                self.audio_buf.resize(target, 0.0);
            }
            let n = dual.main_mut().drain_audio_into(&mut self.audio_buf);
            audio.push_samples(&self.audio_buf[..n]);
        }
        // Bound the SUB console's APU buffer even though its audio is not played:
        // drain it into the reusable `audio_buf` scratch (never a fresh `Vec`),
        // looping until a partial fill signals the buffer is empty.
        if self.audio_buf.is_empty() {
            self.audio_buf.resize(1024, 0.0);
        }
        while dual.sub_mut().drain_audio_into(&mut self.audio_buf) == self.audio_buf.len() {}
    }

    /// v1.6.0 "Studio" Workstream G — feed this frame's produced framebuffer
    /// (`present_fb`, RGBA8 256x240) to the active A/V recorder, if any. A
    /// read-only copy of the already-produced output; on a broken video pipe
    /// (ffmpeg died / was killed) the recorder is dropped so emulation carries
    /// on untouched.
    #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
    fn av_capture_video(&mut self) {
        let Some(rec) = self.av_recorder.as_mut() else {
            return;
        };
        if let Err(e) = rec.push_video(&self.present_fb) {
            eprintln!("rustynes: A/V recording stopped (video): {e}");
            self.av_recorder = None;
        }
    }

    /// v1.6.0 "Studio" Workstream G — feed `n` of this frame's drained audio
    /// samples (`audio_buf[..n]`, mono `f32`) to the active A/V recorder, if
    /// any. Called at each per-frame drain site (the audio is captured from the
    /// SAME samples pushed to the audio sink, keeping A/V in sync). Read-only;
    /// a sidecar write failure drops the recorder.
    #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
    fn av_capture_audio(&mut self, n: usize) {
        let Some(rec) = self.av_recorder.as_mut() else {
            return;
        };
        if let Err(e) = rec.push_audio(&self.audio_buf[..n]) {
            eprintln!("rustynes: A/V recording stopped (audio): {e}");
            self.av_recorder = None;
        }
    }

    /// v1.0.0 — the pacing period after applying the emulation-speed factor:
    /// `frame_duration / speed`. 200% → half the period (2x frames/sec); 50%
    /// → double the period. The speed is clamped to a sane positive range so
    /// a degenerate value can't produce a zero / infinite period. At speed
    /// 1.0 this returns the console-rate `frame_duration` exactly.
    #[must_use]
    pub fn effective_frame_duration(&self) -> Duration {
        // Speed 1.0 returns the console-rate base EXACTLY (no float round-trip),
        // so the default pacing is bit-identical to the pre-v1.0.0 pacer.
        #[allow(clippy::float_cmp)] // 1.0 is the exact preset value.
        if self.speed == 1.0 {
            return self.frame_duration;
        }
        let speed = self.speed.clamp(0.05, 16.0);
        self.frame_duration.div_f32(speed)
    }

    /// Produce however many `frame_duration` slots have elapsed since
    /// `next` up to `now`, capped at [`MAX_CATCHUP_FRAMES`], and advance
    /// `next_frame_time`. Records perf samples per produced frame.
    pub fn produce_due_frames(
        &mut self,
        now: Instant,
        next: Instant,
        inputs: &FrameInputs,
        sinks: &mut FrameSinks<'_>,
    ) -> ProduceFx {
        // `mut` is only exercised on the RA-feature build.
        #[allow(unused_mut)]
        let mut fx = ProduceFx::default();
        // v1.0.0 — pace against the speed-scaled period (1.0 = console rate).
        let period = self.effective_frame_duration();
        // v1.5.0 "Lens" Workstream H2 — if the gap since the last scheduled
        // frame already exceeds the catch-up window, the system slipped on an
        // OS deschedule / UI stall rather than running behind cadence. Break
        // the produced/presented interval phase BEFORE the first
        // `record_produced` so the one-off stall gap is not logged as a frame
        // interval — otherwise a single transient stall dominates
        // `produced_max` and reads as sustained judder in the panel / perf log.
        // The MAX_CATCHUP_FRAMES cap below already bounds the snowball; this
        // only keeps the interval rings honest about steady-state cadence.
        let stall_threshold = period * MAX_CATCHUP_FRAMES;
        if now.saturating_duration_since(next) > stall_threshold {
            self.perf.break_phase();
        }
        let mut target = next;
        let mut produced = 0u32;
        while target <= now && produced < MAX_CATCHUP_FRAMES {
            let t0 = Instant::now();
            fx.merge(self.produce_one_frame(inputs, sinks));
            self.perf.record_produce_cost(t0.elapsed());
            self.perf.record_produced(Instant::now());
            target += period;
            produced += 1;
            // A breakpoint stops the (partial) frame; don't run the rest of the
            // catch-up burst past it, or the core would advance beyond the stop
            // PC before the UI pauses (Copilot #41).
            if fx.breakpoint_hit.is_some() || fx.event_break_hit.is_some() {
                break;
            }
        }
        // v2.8.0 Phase 0 — pacer anomaly counters.
        if produced >= 2 {
            self.perf.catchup_bursts += 1;
        }
        if target <= now {
            // Far behind — snap forward so we don't replay the catch-up
            // window indefinitely (the H2 phase-break at the top of this fn
            // already kept the stall gap out of the interval rings).
            self.perf.snap_forwards += 1;
            target = now + period;
        }
        self.next_frame_time = Some(target);
        fx
    }

    /// Measured wall-clock fps (derived from the produced-interval mean).
    #[must_use]
    pub fn current_fps(&self) -> f32 {
        let mean = self.perf.view_produced_mean_ms();
        if mean > 0.0 { 1000.0 / mean } else { 0.0 }
    }

    /// Flush the FDS writable disk to `<data_dir>/fds-saves/<sha>.fds.sav`
    /// when it has been modified since the last flush. Cheap when clean
    /// (only a `disk_is_dirty()` check). Native-only (filesystem). No-op
    /// for non-FDS games or when no data dir is available.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn flush_fds_save(&mut self, data_dir: Option<&std::path::Path>) {
        let Some(rom_sha256) = self.fds_disk_sha256 else {
            return;
        };
        let Some(nes) = self.nes.as_mut() else { return };
        if nes.disk_side_count() == 0 || !nes.disk_is_dirty() {
            return;
        }
        let bytes = nes.disk_image_bytes();
        let Some(path) = data_dir.map(|d| {
            d.join("fds-saves").join(format!(
                "{}.fds.sav",
                crate::save_state::hex_sha256(&rom_sha256)
            ))
        }) else {
            return;
        };
        if let Some(parent) = path.parent()
            && let Err(e) = std::fs::create_dir_all(parent)
        {
            eprintln!("rustynes: could not create fds-saves dir: {e}");
            return;
        }
        match std::fs::write(&path, &bytes) {
            Ok(()) => {
                if let Some(nes) = self.nes.as_mut() {
                    nes.clear_disk_dirty();
                }
            }
            Err(e) => eprintln!("rustynes: FDS disk save failed {}: {e}", path.display()),
        }
    }
}

impl Default for EmuCore {
    fn default() -> Self {
        Self::new()
    }
}

/// v2.7.0 — advance the `RetroAchievements` session by one tick and return
/// the refreshed status view (the caller pushes it into the debugger). On a
/// produced (non-rewind) frame it runs `do_frame`; otherwise it `idle`s. A
/// `Reset` event resets the emulator.
#[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
pub(crate) fn drive_ra(
    nes: Option<&mut Nes>,
    ra: &mut rustynes_ra::RaSession,
    produced_frame: bool,
) -> crate::debugger::CheevosStatusView {
    match nes {
        Some(nes) => {
            let reset = if produced_frame {
                ra.do_frame(&mut |a| nes.cpu_bus_peek(a))
            } else {
                ra.idle(&mut |a| nes.cpu_bus_peek(a))
            };
            if reset {
                nes.reset();
                ra.reset(&mut |a| nes.cpu_bus_peek(a));
            }
        }
        None => {
            let _ = ra.idle(&mut |_| 0);
        }
    }
    ra.refresh_views();
    ra.expire_toasts();
    crate::debugger::CheevosStatusView::from_session(ra)
}

#[cfg(test)]
#[allow(clippy::suboptimal_flops)] // readability over FMA in assertions.
mod tests {
    use super::*;

    // ---- v2.3.3 F21: the run-ahead budget throttle state machine ----------
    //
    // It had no tests at all, through an all-or-nothing implementation and a
    // step-down rewrite. Raised in review on PR #368, and the gap is why an
    // oscillation regression reached a capture run rather than a test: the
    // aggregate hold-duration metrics cannot see a toggle, so nothing but a
    // direct assertion on this state machine would have caught it.
    //
    // `EmuCore` is expensive to build, so these drive the throttle fields
    // directly through the same predicates production uses.

    /// A budget-sized fixture: 16.639 ms NTSC frame. Callers pass a sample count
    /// above `THROTTLE_MIN_SAMPLES` so the minimum-to-report guard does not
    /// swallow the call.
    #[cfg(not(target_arch = "wasm32"))]
    fn throttle_fixture() -> EmuCore {
        let mut c = EmuCore::new();
        c.frame_duration = std::time::Duration::from_secs_f32(16.639 / 1000.0);
        c
    }

    /// v2.3.3 F27 — the regression test for the defect itself, stated in the
    /// terms the bug was actually in: a transition may not be decided on a
    /// median that predates the previous transition.
    ///
    /// This reproduces the sequence the F26 per-evaluation log captured live.
    /// At two throttle steps (depth 0) a cost of 4.994 ms predicts 9.988 ms one
    /// depth up, under the 11.647 ms band, so it releases to depth 1. The ring
    /// cannot have noticed yet — the very next produced frame still reports the
    /// same 4.994 ms median — and the old gate let that stale value drive a
    /// second release straight to depth 2, which is half of the observed
    /// oscillation. Exactly one transition may come out of one median.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn throttle_will_not_act_twice_on_one_median() {
        let mut c = throttle_fixture();
        c.runahead_throttle_steps = 2;
        c.runahead_throttled = true;
        // The depth-0 cost, fed for a whole window and then some. The first
        // evaluation releases; every later one sees an unchanged median and must
        // decline, however long it is offered.
        for _ in 0..(crate::perf::WINDOW * 2) {
            c.update_runahead_throttle(4.994, 200, 2);
        }
        assert_eq!(
            c.runahead_releases, 2,
            "two windows of an unchanged median may release at most once per window"
        );
        assert_eq!(c.runahead_throttle_steps, 0);
    }

    /// v2.3.3 F28 — the cascade converges to the depth that FITS, not to zero.
    ///
    /// This is the test that makes the arm worth having. At depth 3 a cost of
    /// 17.2 ms exceeds the 12.48 ms engage band at every depth, so a gate that
    /// merely engaged on less evidence would walk 3 -> 2 -> 1 -> 0 and throw the
    /// whole feature away. The cascade stops where the PREDICTED cost fits:
    /// per-frame is 17.2/4 = 4.3 ms, so depth 2 predicts 12.9 (over), depth 1
    /// predicts 8.6 (under) -- it stops at 1, in one evaluation.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn predict_cascade_stops_at_the_depth_that_fits() {
        let mut c = throttle_fixture();
        c.update_runahead_throttle(17.2, 200, 3);
        assert_eq!(
            c.effective_run_ahead(3),
            1,
            "must land on the depth the prediction fits, not 0"
        );
        assert_eq!(c.runahead_throttle_steps, 2, "3 -> 1 is two steps");
    }

    /// A cascade must never overshoot past zero or wrap the step counter.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn predict_cascade_bottoms_out_at_zero() {
        let mut c = throttle_fixture();
        // Absurdly over budget at every depth: per-frame alone exceeds the band.
        c.update_runahead_throttle(400.0, 200, 3);
        assert_eq!(c.effective_run_ahead(3), 0);
        assert_eq!(c.runahead_throttle_steps, 3, "never more steps than depth");
    }

    /// v2.3.3 F27 — the gate must be the turnover period of the statistic it
    /// paces. Asserting the relationship rather than a literal is the point: the
    /// defect was a `120` here with no stated connection to the `600` there, and
    /// a test pinning `120` would have passed throughout.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn throttle_window_is_the_ring_that_feeds_it() {
        let mut c = throttle_fixture();
        // The counter is pre-seeded to one window (see the initializer), so the
        // very first evaluation transitions and resets it to zero. Count from
        // there, exactly, rather than in loop-sized lumps.
        c.update_runahead_throttle(15.0, 200, 2);
        assert_eq!(c.runahead_throttle_toggles, 1, "the first call acts");
        for _ in 0..(crate::perf::WINDOW - 1) {
            c.update_runahead_throttle(15.0, 200, 2);
        }
        assert_eq!(
            c.runahead_throttle_toggles, 1,
            "a window minus one frame is not a window"
        );
        c.update_runahead_throttle(15.0, 200, 2);
        assert_eq!(c.runahead_throttle_toggles, 2, "the window completes here");
    }

    /// Engage takes ONE depth, not all of them — the whole point of the
    /// step-down change. At depth 2 costing 77% of budget, the next frame runs
    /// at depth 1, not 0.
    /// v2.3.3 F24 — one transition per median WINDOW. Repeated calls against an
    /// unchanged median must produce exactly one depth change, or the throttle
    /// walks 2 -> 1 -> 0 without ever measuring depth 1 and the toggle counter
    /// reports stale-median re-evaluation as oscillation.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn throttle_allows_one_transition_per_median_window() {
        let mut c = throttle_fixture();
        // Exactly one window's worth of frames: exactly one change. An earlier
        // version of this test used a literal 600 against a gate of 120 and
        // expected 1 -- that was five windows, so five legitimate opportunities,
        // and it failed against then-correct behaviour (left: 2). The literals
        // are gone: both the gate and the loop are now `perf::WINDOW`, which is
        // what made that mismatch possible to write in the first place (F27).
        for _ in 0..crate::perf::WINDOW {
            c.update_runahead_throttle(12.884, 200, 2);
        }
        assert_eq!(c.runahead_throttle_toggles, 1, "one window, one change");
        assert_eq!(c.runahead_throttle_steps, 1);
        // A second full window with the cost still over budget takes the next
        // step -- that is the convergence path, not oscillation.
        for _ in 0..crate::perf::WINDOW {
            c.update_runahead_throttle(12.884, 200, 2);
        }
        assert_eq!(c.runahead_throttle_toggles, 2, "second window, second step");
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn throttle_engages_one_step_at_a_time() {
        let mut c = throttle_fixture();
        for _ in 0..crate::perf::WINDOW {
            c.update_runahead_throttle(12.884, 200, 2);
        }
        assert_eq!(c.runahead_throttle_steps, 1);
        assert_eq!(c.effective_run_ahead(2), 1, "2 -> 1, not 2 -> 0");
        assert!(c.runahead_throttled);
    }

    /// A host that genuinely cannot afford any depth still converges to 0 — it
    /// just takes one median window per step instead of arriving in one jump.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn throttle_converges_to_zero_when_nothing_is_affordable() {
        let mut c = throttle_fixture();
        // Two full median windows: two steps, 2 -> 1 -> 0.
        for _ in 0..(crate::perf::WINDOW * 2) {
            c.update_runahead_throttle(15.0, 200, 2);
        }
        assert_eq!(c.effective_run_ahead(2), 0);
        // ...and keeps taking that cost without the step counter growing past
        // the depth. `engage` requires `running > 0`, so once the depth is
        // exhausted the arm cannot fire at all -- but an unbounded counter would
        // need many releases to climb back out of, so the bound is asserted
        // rather than left to inspection. Raised in review on PR #371.
        for _ in 0..(crate::perf::WINDOW * 3) {
            c.update_runahead_throttle(15.0, 200, 2);
        }
        assert_eq!(
            c.runahead_throttle_steps, 2,
            "steps must never exceed the configured depth, however long it stays over budget"
        );
    }

    /// The oscillation guard. NOTE: this asserts the CURRENT 0.70 band holds a
    /// steady depth-1 cost without releasing — it does not prove the build is
    /// oscillation-free, and measurement says it is not (3 toggles per 45 s).
    /// A wider band plus a debounce was tried and measured WORSE on both toggles
    /// and cadence, so the defect is elsewhere and this test pins only what the
    /// shipped code actually guarantees. A cost that sits between the
    /// release band and the engage band must NOT produce a release: that is the
    /// oscillation that displaces the displayed frame by the run-ahead depth,
    /// which the user sees as the picture jumping forward and back. Measured at
    /// 3 toggles per 45 s before the band was widened.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn throttle_does_not_oscillate_in_the_hysteresis_gap() {
        let mut c = throttle_fixture();
        for _ in 0..crate::perf::WINDOW {
            c.update_runahead_throttle(12.884, 200, 2); // engage: 2 -> 1
        }
        let toggles_after_engage = c.runahead_throttle_toggles;
        // Now running at depth 1, ~8.56 ms. Giving back a step predicts ~12.8 ms
        // — under the OLD 0.70 band (11.65 ms)? No; but a dip cleared it. Feed
        // the steady depth-1 cost for many windows and require no release.
        for _ in 0..(crate::perf::WINDOW * 5) {
            c.update_runahead_throttle(8.556, 200, 2);
        }
        assert_eq!(c.runahead_throttle_steps, 1, "must stay at one step");
        assert_eq!(
            c.runahead_throttle_toggles, toggles_after_engage,
            "no toggle may occur while cost sits in the hysteresis gap"
        );
    }

    /// v1.7.0 "Forge" Workstream A1 — a minimal NES 2.0 NROM (16 KiB PRG / 8 KiB
    /// CHR) whose reset vector loops on itself, so `run_frame` advances without
    /// touching work RAM — leaving the queued-poke effect cleanly observable.
    #[cfg(not(target_arch = "wasm32"))]
    fn synth_nrom() -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
        rom[0..4].copy_from_slice(b"NES\x1A");
        rom[4] = 1; // 1x16 KiB PRG
        rom[5] = 1; // 1x8 KiB CHR
        // Reset vector ($FFFC/$FFFD) → $C000; opcode at $C000 is a JMP $C000.
        let prg = 16;
        rom[prg] = 0x4C; // JMP abs
        rom[prg + 1] = 0x00;
        rom[prg + 2] = 0xC0;
        let reset_lo = 16 + (0xFFFC - 0xC000);
        rom[reset_lo] = 0x00;
        rom[reset_lo + 1] = 0xC0;
        rom
    }

    /// v1.7.0 "Forge" Workstream A1 — default-quiet [`FrameInputs`] for a
    /// produce-path test (no input, no rewind, not hardcore-blocked).
    #[cfg(not(target_arch = "wasm32"))]
    fn quiet_inputs() -> FrameInputs {
        FrameInputs {
            buttons: [Buttons::empty(); 4],
            four_score: false,
            microphone: false,
            rewind_held: false,
            hardcore_blocked: false,
            run_ahead: 0,
            expansion: ExpansionDevice::None,
            mouse_nes: (u16::MAX, u16::MAX),
            mouse_pressed: false,
            turbo_mask: Buttons::empty(),
            turbo_period: 1,
            power_pad: 0,
            mouse_delta: (0, 0),
            mouse_right: false,
            mouse_sensitivity: 0,
            family_keyboard: [0; 9],
            konami_hyper_shot: 0,
            bandai_hyper_shot: 0,
        }
    }

    /// v1.7.0 "Forge" Workstream A1 — the gated-writeback contract: a queued
    /// `DebugPoke` applies after a frame when UNLOCKED, but is a no-op (and the
    /// queue is cleared) when `writes_locked` (TAS replay / netplay) — proving
    /// "locked = no-op = byte-identical" against the same poke applied unlocked.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn debug_poke_is_gated_by_writes_locked() {
        let rom = synth_nrom();
        let mut sinks = FrameSinks {
            audio: None,
            #[cfg(feature = "retroachievements")]
            ra: None,
        };
        let inputs = quiet_inputs();

        // UNLOCKED: the queued work-RAM poke lands after the frame.
        let mut core = EmuCore::new();
        core.nes = Some(Nes::from_rom(&rom).unwrap());
        core.writes_locked = false;
        core.debug_pokes.push(DebugPoke::CpuRam {
            addr: 0x0040,
            value: 0xAB,
        });
        core.produce_one_frame(&inputs, &mut sinks);
        assert_eq!(
            core.nes.as_mut().unwrap().peek(0x0040),
            0xAB,
            "unlocked poke must apply"
        );
        assert!(core.debug_pokes.is_empty(), "queue is drained after apply");

        // LOCKED: an identical poke is a no-op AND the queue is cleared, so a
        // later unlocked frame sees no residual edit (byte-identical timeline).
        let mut locked = EmuCore::new();
        locked.nes = Some(Nes::from_rom(&rom).unwrap());
        locked.writes_locked = true;
        locked.debug_pokes.push(DebugPoke::CpuRam {
            addr: 0x0040,
            value: 0xAB,
        });
        locked.produce_one_frame(&inputs, &mut sinks);
        assert_eq!(
            locked.nes.as_mut().unwrap().peek(0x0040),
            0x00,
            "locked poke must be a no-op"
        );
        assert!(
            locked.debug_pokes.is_empty(),
            "locked queue is cleared (not deferred)"
        );

        // RA-hardcore (hardcore_blocked) is gated the same way.
        let mut hc = EmuCore::new();
        hc.nes = Some(Nes::from_rom(&rom).unwrap());
        hc.writes_locked = false;
        hc.debug_pokes.push(DebugPoke::CpuRam {
            addr: 0x0040,
            value: 0xAB,
        });
        let mut hc_inputs = quiet_inputs();
        hc_inputs.hardcore_blocked = true;
        hc.produce_one_frame(&hc_inputs, &mut sinks);
        assert_eq!(
            hc.nes.as_mut().unwrap().peek(0x0040),
            0x00,
            "hardcore-blocked poke must be a no-op"
        );
    }

    /// v1.7.0 "Forge" Workstream H4 — the lag-frame tally. The synth NROM loops
    /// `JMP self` and never reads `$4016`/`$4017`, so every produced forward
    /// frame is a lag frame; a rewind step never re-runs a frame so it cannot
    /// add to the tally; and `reset_lag_frames` clears it.
    #[cfg(not(target_arch = "wasm32"))]
    #[test]
    fn lag_frames_count_unpolled_forward_frames() {
        let rom = synth_nrom();
        let mut sinks = FrameSinks {
            audio: None,
            #[cfg(feature = "retroachievements")]
            ra: None,
        };
        let inputs = quiet_inputs();

        let mut core = EmuCore::new();
        core.nes = Some(Nes::from_rom(&rom).unwrap());
        assert_eq!(core.lag_frames(), 0, "fresh core starts at 0");

        // Three forward frames of a never-polling program → three lag frames.
        for _ in 0..3 {
            core.produce_one_frame(&inputs, &mut sinks);
        }
        assert_eq!(core.lag_frames(), 3, "every unpolled forward frame counts");

        // A rewind step re-presents a prior frame without re-running one, so the
        // tally is unchanged (rewind needs the ring enabled to actually step,
        // but the counter is only touched on the non-rewind branch regardless).
        let mut rewind = inputs;
        rewind.rewind_held = true;
        core.produce_one_frame(&rewind, &mut sinks);
        assert_eq!(core.lag_frames(), 3, "a rewind step adds no lag frame");

        // Reset clears the tally (a fresh session).
        core.reset_lag_frames();
        assert_eq!(core.lag_frames(), 0, "reset_lag_frames zeroes the counter");
    }

    #[test]
    fn turbo_strobes_only_masked_buttons() {
        let mask = Buttons::A | Buttons::B;
        let held = Buttons::A | Buttons::RIGHT; // A is turbo, RIGHT is not.
        // period 1: on at even frames, off at odd.
        assert_eq!(apply_turbo(held, 0, mask, 1), Buttons::A | Buttons::RIGHT);
        assert_eq!(apply_turbo(held, 1, mask, 1), Buttons::RIGHT); // A suppressed
        assert_eq!(apply_turbo(held, 2, mask, 1), Buttons::A | Buttons::RIGHT);
        // RIGHT (non-turbo) is never suppressed.
        for f in 0..8 {
            assert!(apply_turbo(held, f, mask, 1).contains(Buttons::RIGHT));
        }
    }

    #[test]
    fn turbo_period_widens_the_strobe() {
        let mask = Buttons::A;
        let held = Buttons::A;
        // period 2: on for frames 0,1 then off for 2,3 (a 4-frame cycle).
        assert!(apply_turbo(held, 0, mask, 2).contains(Buttons::A));
        assert!(apply_turbo(held, 1, mask, 2).contains(Buttons::A));
        assert!(!apply_turbo(held, 2, mask, 2).contains(Buttons::A));
        assert!(!apply_turbo(held, 3, mask, 2).contains(Buttons::A));
        assert!(apply_turbo(held, 4, mask, 2).contains(Buttons::A));
    }

    #[test]
    fn turbo_off_is_identity() {
        let held = Buttons::A | Buttons::B | Buttons::START;
        // Empty mask = byte-identical regardless of frame/period.
        for f in 0..10 {
            assert_eq!(apply_turbo(held, f, Buttons::empty(), 2), held);
        }
        // Period 0 is clamped to 1 (no divide-by-zero), still identity for an
        // empty mask and a deterministic strobe for a non-empty one.
        assert_eq!(apply_turbo(held, 0, Buttons::A, 0), held);
        assert_eq!(apply_turbo(held, 1, Buttons::A, 0), held & !Buttons::A);
    }

    #[test]
    fn effective_frame_duration_scales_inversely_with_speed() {
        let mut core = EmuCore::new();
        let base = core.frame_duration;
        // Speed 1.0 is the console rate exactly (the determinism-safe default).
        core.speed = 1.0;
        assert_eq!(core.effective_frame_duration(), base);
        // 200% halves the period (twice the frames/sec). The `div_f32` runs in
        // f32, so allow ~1 us of rounding (far below any pacing concern).
        core.speed = 2.0;
        let half = core.effective_frame_duration();
        assert!(
            (half.as_secs_f64() - base.as_secs_f64() / 2.0).abs() < 1e-6,
            "200% should halve the period"
        );
        // 50% doubles it.
        core.speed = 0.5;
        let twice = core.effective_frame_duration();
        assert!(
            (twice.as_secs_f64() - base.as_secs_f64() * 2.0).abs() < 1e-6,
            "50% should double the period"
        );
    }

    #[test]
    fn effective_frame_duration_clamps_degenerate_speed() {
        let mut core = EmuCore::new();
        // A zero / negative speed cannot produce a zero / infinite period.
        core.speed = 0.0;
        assert!(core.effective_frame_duration() > Duration::ZERO);
        core.speed = -1.0;
        assert!(core.effective_frame_duration() > Duration::ZERO);
        // An absurdly high speed is capped (period stays non-trivial).
        core.speed = 1_000.0;
        assert!(core.effective_frame_duration() > Duration::ZERO);
    }
}
