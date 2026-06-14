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

impl EmuHandle {
    /// Wrap a fresh core in the shared handle.
    #[must_use]
    pub fn new(core: EmuCore) -> Self {
        Self {
            inner: std::sync::Arc::new(std::sync::Mutex::new(core)),
        }
    }

    /// Lock the core. Poisoning is ignored deliberately: a panic on one
    /// thread must not wedge the other (the core's state is a plain value;
    /// the next frame either works or panics identically).
    pub fn lock(&self) -> std::sync::MutexGuard<'_, EmuCore> {
        self.inner
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
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
    let on = (frame / period) % 2 == 0;
    if on {
        buttons
    } else {
        buttons & !mask
    }
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
    pub ra: Option<&'a mut crate::ra_session::RaSession>,
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
    }
}

/// The emulation core: the per-frame produce state extracted from `App`.
pub struct EmuCore {
    /// The running emulator (None until a ROM is loaded).
    pub nes: Option<Nes>,
    /// TAS movie record/playback state machine.
    pub movie: MovieUi,
    /// Frame-pacing / audio instrumentation (Phase 0).
    pub perf: PerfStats,
    /// The framebuffer the renderer presents (with run-ahead active it is
    /// the visible FUTURE frame while `nes` holds the persistent one).
    pub present_fb: Vec<u8>,
    /// Enabled raw RAM cheats, pulled from the cheat panel each frame.
    pub raw_cheats: Vec<RawCheat>,
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
    /// Run-ahead budget throttle (hysteresis on produce-cost p95).
    #[cfg(not(target_arch = "wasm32"))]
    pub runahead_throttled: bool,
    /// SHA-256 of the loaded FDS disk (keys the `.fds.sav` sidecar).
    #[cfg(not(target_arch = "wasm32"))]
    pub fds_disk_sha256: Option<[u8; 32]>,
}

impl EmuCore {
    /// Empty core (no ROM).
    #[must_use]
    pub fn new() -> Self {
        Self {
            nes: None,
            movie: MovieUi::default(),
            perf: PerfStats::default(),
            present_fb: Vec::new(),
            raw_cheats: Vec::new(),
            vs_coin_frames: 0,
            frame_duration: rustynes_core::FRAME_DURATION_NTSC,
            speed: 1.0,
            next_frame_time: None,
            audio_buf: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            runahead: crate::runahead::RunAhead::default(),
            #[cfg(not(target_arch = "wasm32"))]
            runahead_throttled: false,
            #[cfg(not(target_arch = "wasm32"))]
            fds_disk_sha256: None,
        }
    }

    /// Latch the per-pace input snapshot into the emulator (controllers +
    /// any expansion device). The single latest-possible point before
    /// `run_frame` consumes it.
    // const-able only on wasm (the expansion-device block is native-only).
    #[cfg_attr(target_arch = "wasm32", allow(clippy::missing_const_for_fn))]
    pub fn latch(&mut self, inputs: &FrameInputs) {
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
                }
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
        if self.runahead_throttled || self.movie.status().mode != crate::movie_ui::MovieMode::Idle {
            return 0;
        }
        configured.min(3)
    }

    /// v2.8.0 Phase 3 — run-ahead budget throttle with hysteresis, fed by
    /// the produce-cost **median** (which INCLUDES the run-ahead frames).
    /// Engages at 85% of the frame budget, releases below 40% — the gap
    /// prevents oscillation (cost drops when the extra frames stop).
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
    pub fn update_runahead_throttle(&mut self, produce_p50_ms: f32, samples: usize) {
        if samples < 120 {
            return;
        }
        let target = self.frame_duration.as_secs_f32() * 1000.0;
        if !self.runahead_throttled && produce_p50_ms > target * 0.85 {
            self.runahead_throttled = true;
            eprintln!(
                "rustynes: median produce cost {produce_p50_ms:.2} ms is too close to the \
                 {target:.2} ms frame budget — run-ahead disabled until it recovers."
            );
        } else if self.runahead_throttled && produce_p50_ms < target * 0.40 {
            self.runahead_throttled = false;
            eprintln!("rustynes: produce cost recovered — run-ahead re-enabled.");
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
        let hardcore_blocked = inputs.hardcore_blocked;
        // v2.8.0 Phase 3 — resolve the run-ahead depth before borrowing
        // `nes`. 0 = plain frame.
        #[cfg(not(target_arch = "wasm32"))]
        let run_ahead_n = self.effective_run_ahead(inputs.run_ahead);
        let Some(nes) = self.nes.as_mut() else {
            return fx;
        };
        // v2.7.0 — RetroAchievements hardcore mode disables rewind (already
        // folded into `inputs.rewind_held` by `App`).
        let rewinding = inputs.rewind_held;
        if rewinding {
            // Rewind is a live-only gesture; it never advances a movie
            // cursor or captures a frame.
            let _ = nes.rewind_step_back();
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
                if let Some(audio) = sinks.audio.as_mut() {
                    let target = ((u64::from(audio.sample_rate()) / 50) as usize).max(1024);
                    if self.audio_buf.len() < target {
                        self.audio_buf.resize(target, 0.0);
                    }
                    let n = nes.drain_audio_into(&mut self.audio_buf);
                    audio.push_samples(&self.audio_buf[..n]);
                }
                self.runahead.finish(nes);
                true
            };
            #[cfg(target_arch = "wasm32")]
            let ran_ahead = false;

            if !ran_ahead {
                nes.run_frame();
                // v2.8.0 Phase 3 — harvest the presented framebuffer into a
                // reused buffer.
                self.present_fb.clear();
                self.present_fb.extend_from_slice(nes.framebuffer());

                #[cfg(not(target_arch = "wasm32"))]
                if let Some(audio) = sinks.audio.as_mut() {
                    let target = ((u64::from(audio.sample_rate()) / 50) as usize).max(1024);
                    if self.audio_buf.len() < target {
                        self.audio_buf.resize(target, 0.0);
                    }
                    let n = nes.drain_audio_into(&mut self.audio_buf);
                    // v2.8.0 Phase 1 — through the DRC resampler stage.
                    audio.push_samples(&self.audio_buf[..n]);
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
        }

        // v2.7.0 — drive RetroAchievements after the frame. Only the
        // synchronous (winit-thread) drive passes a session; the emulation
        // thread leaves `ra` None and drives RA itself per published frame.
        #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
        {
            if let Some(ra) = sinks.ra.as_deref_mut() {
                fx.ra_status = Some(drive_ra(self.nes.as_mut(), ra, !rewinding));
                fx.ra_just_logged_in = crate::ra_session::RaSession::take_just_logged_in(ra);
            }
        }
        fx
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
        let mut target = next;
        let mut produced = 0u32;
        while target <= now && produced < MAX_CATCHUP_FRAMES {
            let t0 = Instant::now();
            fx.merge(self.produce_one_frame(inputs, sinks));
            self.perf.record_produce_cost(t0.elapsed());
            self.perf.record_produced(Instant::now());
            target += period;
            produced += 1;
        }
        // v2.8.0 Phase 0 — pacer anomaly counters.
        if produced >= 2 {
            self.perf.catchup_bursts += 1;
        }
        if target <= now {
            // Far behind — snap forward so we don't replay the catch-up
            // window indefinitely.
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
        if mean > 0.0 {
            1000.0 / mean
        } else {
            0.0
        }
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
        if let Some(parent) = path.parent() {
            if let Err(e) = std::fs::create_dir_all(parent) {
                eprintln!("rustynes: could not create fds-saves dir: {e}");
                return;
            }
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
    ra: &mut crate::ra_session::RaSession,
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
