//! v2.8.0 Phase 5 increment 3 — the dedicated emulation thread (native).
//!
//! When the default-ON `emu-thread` feature is built, single-player frame
//! production moves OFF the winit event-loop thread onto a dedicated thread
//! that owns the pacer + run-ahead + the `Send` [`AudioProducer`]
//! ([`crate::audio::AudioProducer`]). The winit thread is then free to
//! service window events, egui, and the wgpu submit/present without ever
//! stalling emulation cadence — the last of the v2.8.0 root causes (the
//! shared-thread head-of-line blocking).
//!
//! The thread reads its inputs from a lock-free [`SharedInput`] (published
//! by the winit thread on every input event + gamepad pump) and its
//! regime/lifecycle from [`EmuControl`] (written by the winit thread's
//! `resolve_pacing` / ROM-load / exit paths). After each produced frame it
//! pings the winit loop with [`crate::app::AppEvent::EmuFrame`] so the UI
//! thread does the housekeeping (perf/HUD pushes, FDS flush, perf logging)
//! and requests a redraw.
//!
//! Concurrency model:
//! - The [`crate::emu::EmuHandle`] mutex is the one synchronization point.
//!   The emu thread holds it only for the brief latch+produce region; the
//!   winit thread holds it briefly for input commands, the
//!   debugger-hidden present's framebuffer copy, and (on the RA build) the
//!   per-frame RA drive. Neither ever blocks the other on I/O or present.
//! - **Netplay always runs synchronously on the winit thread** (it owns the
//!   `UdpSocket`); while a session is active the emu thread is *paused*
//!   (`EmuControl`'s netplay-paused flag) so the two never both drive the core.
//! - **`RetroAchievements` stays on the winit thread** (`rc_client` is
//!   single-threaded C): the emu thread produces with `ra: None`, and the
//!   winit thread drives RA per published frame.
//!
//! When the feature is OFF, none of this is compiled and `App` drives
//! production synchronously exactly as in Phases 0-4 (the A/B fallback,
//! collapsed in a later release once the threaded path is proven).

// The emu-lock guard in the drive functions deliberately spans the
// latch+produce region (and the under-lock netplay-pause re-check); the
// nursery drop-tightening lint would split it without changing behavior.
#![allow(clippy::significant_drop_tightening)]

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, AtomicU8, AtomicU16, AtomicU32, AtomicU64, Ordering};
use std::sync::mpsc::{Receiver, RecvTimeoutError, SyncSender, sync_channel};
use std::thread::JoinHandle;
use std::time::Duration;

use web_time::Instant;
use winit::event_loop::EventLoopProxy;

use crate::app::AppEvent;
use crate::audio::AudioProducer;
use crate::config::ExpansionDevice;
use crate::emu::{EmuHandle, FrameInputs, FrameSinks};

/// Native-only precise-pacing spin margin (see [`block_until_native`]).
/// Mirrors `App`'s wall-clock pacer constant: sleep until ~2 ms before the
/// target, then busy-spin to the exact instant to remove OS-timer jitter.
const SPIN_MARGIN: Duration = Duration::from_millis(2);

/// Maximum length of any single sleep inside [`block_until_native`], so one
/// OS oversleep can overshoot by at most this before the loop re-measures.
const SLEEP_CHUNK: Duration = Duration::from_millis(2);

/// Display-regime occlusion watchdog: if no present-driven tick arrives for
/// this long (minimized / fully occluded window), the thread produces due
/// frames wall-clock so emulation + audio keep running.
const DISPLAY_TICK_TIMEOUT: Duration = Duration::from_millis(25);

/// Park interval while the thread is idle (no ROM, or netplay-paused). Short
/// enough that a resume is near-immediate, long enough to not spin a core.
const IDLE_PARK: Duration = Duration::from_millis(8);

/// A [`Duration`] as `u64` nanoseconds, saturating (a frame duration is
/// always far below `u64::MAX` ns ≈ 584 years, so this never clamps).
fn dur_nanos(d: Duration) -> u64 {
    u64::try_from(d.as_nanos()).unwrap_or(u64::MAX)
}

/// The active pacing regime, encoded for `EmuControl`'s regime field.
pub mod regime {
    /// Wall-clock pacer + configured present mode (Mailbox default).
    pub const WALLCLOCK: u8 = 0;
    /// Fifo vsync is the clock: one emulated frame per display refresh.
    pub const DISPLAY: u8 = 1;
    /// VRR: Fifo + the wall-clock pacer at the exact console rate.
    pub const VRR: u8 = 2;
}

/// Lock-free input snapshot, winit thread (writer) to emu thread (reader).
///
/// Published once per input event / gamepad pump and read once per produced
/// frame. Every field is the flattened form of one [`FrameInputs`] field;
/// [`Self::publish`] / [`Self::load`] are the single round-trip (so the
/// `FrameInputs` shape stays the one source of truth for the mapping).
#[derive(Debug, Default)]
pub struct SharedInput {
    buttons: [AtomicU8; 4],
    four_score: AtomicBool,
    rewind_held: AtomicBool,
    hardcore_blocked: AtomicBool,
    run_ahead: AtomicU8,
    /// `ExpansionDevice` as `u8` (0 None / 1 Zapper / 2 Vaus / 3 Power Pad /
    /// 4 SNES mouse / 5 Family BASIC keyboard).
    expansion: AtomicU8,
    /// `(x as u16) << 16 | (y as u16)` NES-screen coords (`u16::MAX` = off).
    mouse: AtomicU32,
    mouse_pressed: AtomicBool,
    /// v1.1.0 beta.1 (T-110-B2) — turbo/autofire mask (`Buttons` bits) + period.
    turbo_mask: AtomicU8,
    turbo_period: AtomicU32,
    /// v1.1.0 beta.1 (T-110-B1) — Power Pad mat button mask.
    power_pad: AtomicU16,
    /// v1.2.0 Workstream D — SNES-mouse motion `(dx as u16) << 16 | (dy as u16)`.
    mouse_delta: AtomicU32,
    /// v1.2.0 Workstream D — SNES-mouse right button (left reuses `mouse_pressed`).
    mouse_right: AtomicBool,
    /// v1.2.0 Workstream D — Family BASIC keyboard matrix bitmap: rows 0..=7
    /// packed little-endian into a u64, row 8 in `family_keyboard_hi`.
    family_keyboard_lo: AtomicU64,
    family_keyboard_hi: AtomicU8,
}

impl SharedInput {
    /// Publish the winit thread's latest input snapshot (Relaxed: the emu
    /// thread tolerates reading a one-frame-stale field; there is no
    /// cross-field invariant to order).
    pub fn publish(&self, inputs: &FrameInputs) {
        for (slot, b) in self.buttons.iter().zip(inputs.buttons.iter()) {
            slot.store(b.bits(), Ordering::Relaxed);
        }
        self.four_score.store(inputs.four_score, Ordering::Relaxed);
        self.rewind_held
            .store(inputs.rewind_held, Ordering::Relaxed);
        self.hardcore_blocked
            .store(inputs.hardcore_blocked, Ordering::Relaxed);
        #[allow(clippy::cast_possible_truncation)]
        self.run_ahead.store(
            inputs.run_ahead.min(u32::from(u8::MAX)) as u8,
            Ordering::Relaxed,
        );
        self.expansion.store(
            match inputs.expansion {
                ExpansionDevice::None => 0,
                ExpansionDevice::Zapper => 1,
                ExpansionDevice::Vaus => 2,
                ExpansionDevice::PowerPad => 3,
                ExpansionDevice::SnesMouse => 4,
                ExpansionDevice::FamilyKeyboard => 5,
            },
            Ordering::Relaxed,
        );
        let (mx, my) = inputs.mouse_nes;
        self.mouse
            .store((u32::from(mx) << 16) | u32::from(my), Ordering::Relaxed);
        self.mouse_pressed
            .store(inputs.mouse_pressed, Ordering::Relaxed);
        self.turbo_mask
            .store(inputs.turbo_mask.bits(), Ordering::Relaxed);
        self.turbo_period
            .store(inputs.turbo_period, Ordering::Relaxed);
        self.power_pad.store(inputs.power_pad, Ordering::Relaxed);
        // v1.2.0 Workstream D — SNES mouse + Family BASIC keyboard.
        let (dx, dy) = inputs.mouse_delta;
        // Reinterpret the signed deltas as their two's-complement u16 bits
        // (round-tripped back to i16 on load); not a value-narrowing cast.
        #[allow(clippy::cast_sign_loss)]
        let packed = (u32::from(dx as u16) << 16) | u32::from(dy as u16);
        self.mouse_delta.store(packed, Ordering::Relaxed);
        self.mouse_right
            .store(inputs.mouse_right, Ordering::Relaxed);
        let kb = inputs.family_keyboard;
        let lo = u64::from_le_bytes([kb[0], kb[1], kb[2], kb[3], kb[4], kb[5], kb[6], kb[7]]);
        self.family_keyboard_lo.store(lo, Ordering::Relaxed);
        self.family_keyboard_hi.store(kb[8], Ordering::Relaxed);
    }

    /// Reconstruct the [`FrameInputs`] the emu thread feeds to the produce
    /// path for the next frame.
    #[must_use]
    pub fn load(&self) -> FrameInputs {
        use rustynes_core::Buttons;
        let buttons = std::array::from_fn(|i| {
            Buttons::from_bits_truncate(self.buttons[i].load(Ordering::Relaxed))
        });
        let mouse = self.mouse.load(Ordering::Relaxed);
        FrameInputs {
            buttons,
            four_score: self.four_score.load(Ordering::Relaxed),
            rewind_held: self.rewind_held.load(Ordering::Relaxed),
            hardcore_blocked: self.hardcore_blocked.load(Ordering::Relaxed),
            run_ahead: u32::from(self.run_ahead.load(Ordering::Relaxed)),
            expansion: match self.expansion.load(Ordering::Relaxed) {
                1 => ExpansionDevice::Zapper,
                2 => ExpansionDevice::Vaus,
                3 => ExpansionDevice::PowerPad,
                4 => ExpansionDevice::SnesMouse,
                5 => ExpansionDevice::FamilyKeyboard,
                _ => ExpansionDevice::None,
            },
            #[allow(clippy::cast_possible_truncation)]
            mouse_nes: ((mouse >> 16) as u16, mouse as u16),
            mouse_pressed: self.mouse_pressed.load(Ordering::Relaxed),
            turbo_mask: Buttons::from_bits_truncate(self.turbo_mask.load(Ordering::Relaxed)),
            turbo_period: self.turbo_period.load(Ordering::Relaxed),
            power_pad: self.power_pad.load(Ordering::Relaxed),
            mouse_delta: {
                let m = self.mouse_delta.load(Ordering::Relaxed);
                #[allow(clippy::cast_possible_truncation, clippy::cast_possible_wrap)]
                (((m >> 16) as u16) as i16, (m as u16) as i16)
            },
            mouse_right: self.mouse_right.load(Ordering::Relaxed),
            family_keyboard: {
                let lo = self
                    .family_keyboard_lo
                    .load(Ordering::Relaxed)
                    .to_le_bytes();
                let hi = self.family_keyboard_hi.load(Ordering::Relaxed);
                [lo[0], lo[1], lo[2], lo[3], lo[4], lo[5], lo[6], lo[7], hi]
            },
        }
    }
}

/// Lifecycle + regime control shared between the winit thread (writer) and
/// the emulation thread (reader).
#[derive(Debug)]
pub struct EmuControl {
    /// Set on exit; the thread observes it and returns.
    stop: AtomicBool,
    /// Set while a netplay session is active: the emu thread parks so the
    /// winit thread can drive the rollback session unopposed.
    netplay_paused: AtomicBool,
    /// v1.0.0 — set while the user paused emulation from the UX shell (the
    /// Emulation -> Pause menu). Distinct from [`netplay_paused`] so the two
    /// pause sources don't collide: a user pause must not clear a netplay pause
    /// (or vice-versa). The thread idles while either is set.
    user_paused: AtomicBool,
    /// `true` once a ROM is loaded (the thread idles until then).
    has_rom: AtomicBool,
    /// The active pacing regime (see [`regime`]).
    regime: AtomicU8,
    /// Per-region frame duration in nanoseconds.
    frame_nanos: AtomicU64,
    /// Set while the fast-forward key is held: the thread produces frames
    /// back-to-back (unthrottled) and mutes audio so the lock-free ring does
    /// not overrun.
    fast_forward: AtomicBool,
    /// Pending frame-advance steps (incremented when the user presses the
    /// frame-advance key while paused). The thread consumes one per loop and
    /// produces exactly one unthrottled frame for each.
    frame_advance: AtomicU32,
}

impl EmuControl {
    /// Build the control block in the initial idle state.
    #[must_use]
    pub fn new() -> Self {
        Self {
            stop: AtomicBool::new(false),
            netplay_paused: AtomicBool::new(false),
            user_paused: AtomicBool::new(false),
            has_rom: AtomicBool::new(false),
            regime: AtomicU8::new(regime::WALLCLOCK),
            frame_nanos: AtomicU64::new(dur_nanos(rustynes_core::FRAME_DURATION_NTSC)),
            fast_forward: AtomicBool::new(false),
            frame_advance: AtomicU32::new(0),
        }
    }

    /// Mark a ROM loaded (or cleared) so the thread starts (or idles).
    pub fn set_has_rom(&self, on: bool) {
        self.has_rom.store(on, Ordering::Release);
    }

    /// Set the active regime + per-region frame duration (from
    /// `App::resolve_pacing`).
    pub fn set_regime(&self, regime: u8, frame: Duration) {
        self.frame_nanos.store(dur_nanos(frame), Ordering::Release);
        self.regime.store(regime, Ordering::Release);
    }

    /// Pause (netplay starting) or resume (netplay left) the emu thread.
    pub fn set_netplay_paused(&self, on: bool) {
        self.netplay_paused.store(on, Ordering::Release);
    }

    /// Whether the emu thread is currently paused for netplay.
    #[must_use]
    pub fn is_netplay_paused(&self) -> bool {
        self.netplay_paused.load(Ordering::Acquire)
    }

    /// v1.0.0 — pause (or resume) emulation from the UX shell. Independent of
    /// [`Self::set_netplay_paused`]; the thread idles while either is set.
    pub fn set_user_paused(&self, on: bool) {
        self.user_paused.store(on, Ordering::Release);
    }

    /// Set (or clear) fast-forward. While set the emu thread produces frames
    /// unthrottled (no pacer block / no tick wait) and mutes audio.
    pub fn set_fast_forward(&self, on: bool) {
        self.fast_forward.store(on, Ordering::Release);
    }

    /// Whether fast-forward is currently engaged.
    #[must_use]
    pub fn is_fast_forward(&self) -> bool {
        self.fast_forward.load(Ordering::Acquire)
    }

    /// Request one frame-advance step (consumed by the idle gate to produce
    /// exactly one frame while paused). Increments a pending counter so two
    /// quick presses step two frames.
    pub fn request_frame_advance(&self) {
        self.frame_advance.fetch_add(1, Ordering::AcqRel);
    }

    /// Consume one pending frame-advance step. Returns `true` (and decrements)
    /// if one was pending, else `false`. A compare-and-swap loop keeps the
    /// decrement race-free against concurrent `request_frame_advance`.
    pub fn take_frame_advance(&self) -> bool {
        let mut current = self.frame_advance.load(Ordering::Acquire);
        loop {
            if current == 0 {
                return false;
            }
            match self.frame_advance.compare_exchange_weak(
                current,
                current - 1,
                Ordering::AcqRel,
                Ordering::Acquire,
            ) {
                Ok(_) => return true,
                Err(actual) => current = actual,
            }
        }
    }
}

impl Default for EmuControl {
    fn default() -> Self {
        Self::new()
    }
}

/// Owner handle for the spawned emulation thread, held by `App`.
pub struct EmuThread {
    handle: Option<JoinHandle<()>>,
    control: Arc<EmuControl>,
    shared_input: Arc<SharedInput>,
    /// Display-regime present tick (bounded depth 1; `try_send` from
    /// `App::display_sync_after_present`).
    tick_tx: SyncSender<()>,
}

impl EmuThread {
    /// Spawn the emulation thread. `audio` is the `Send` producer half made
    /// from the cpal output (the stream + the consumer callback stay on the
    /// winit thread); `None` when audio init failed.
    #[must_use]
    pub fn spawn(
        emu: EmuHandle,
        audio: Option<AudioProducer>,
        proxy: EventLoopProxy<AppEvent>,
        control: Arc<EmuControl>,
        shared_input: Arc<SharedInput>,
    ) -> Self {
        let (tick_tx, tick_rx) = sync_channel::<()>(1);
        let control_t = Arc::clone(&control);
        let shared_t = Arc::clone(&shared_input);
        let handle = std::thread::Builder::new()
            .name("rustynes-emu".into())
            .spawn(move || run_loop(&emu, audio, &proxy, &control_t, &shared_t, &tick_rx))
            .map_err(|e| eprintln!("rustynes: emu thread spawn failed: {e}"))
            .ok();
        Self {
            handle,
            control,
            shared_input,
            tick_tx,
        }
    }

    /// The shared input the winit thread publishes into each event/pump.
    #[must_use]
    pub const fn shared_input(&self) -> &Arc<SharedInput> {
        &self.shared_input
    }

    /// The control block (regime / ROM / netplay-pause writes). Returns the
    /// `Arc` ref (not a deref-coerced `&EmuControl`) so it stays `const`;
    /// callers auto-deref for method calls.
    #[must_use]
    pub const fn control(&self) -> &Arc<EmuControl> {
        &self.control
    }

    /// Nudge the display-regime loop with a present tick. Bounded depth 1,
    /// non-blocking: a full channel means a tick is already pending, which
    /// is the same signal — drop the duplicate.
    pub fn notify_present(&self) {
        let _ = self.tick_tx.try_send(());
    }

    /// v1.0.0 (BUG-1) — wake the emulation thread out of its idle park. Called
    /// on resume so the just-cleared `user_paused` flag is observed
    /// immediately rather than after the (up to `IDLE_PARK`) park timeout.
    /// `park_timeout` consumes the unpark token, so a stray unpark is harmless.
    pub fn unpark(&self) {
        if let Some(h) = self.handle.as_ref() {
            h.thread().unpark();
        }
    }

    /// Signal stop and join the thread (called on exit).
    pub fn shutdown(&mut self) {
        self.control.stop.store(true, Ordering::Release);
        // A pending tick or the park timeout wakes the loop within ~25 ms.
        if let Some(h) = self.handle.take() {
            let _ = h.join();
        }
    }
}

impl Drop for EmuThread {
    fn drop(&mut self) {
        self.shutdown();
    }
}

/// The emulation thread's main loop.
fn run_loop(
    emu: &EmuHandle,
    mut audio: Option<AudioProducer>,
    proxy: &EventLoopProxy<AppEvent>,
    control: &EmuControl,
    shared_input: &SharedInput,
    tick_rx: &Receiver<()>,
) {
    elevate_thread_priority();
    loop {
        if control.stop.load(Ordering::Acquire) {
            return;
        }
        // Idle: no ROM yet, netplay owns the core on the winit thread, or the
        // user paused emulation from the UX shell.
        let idle = !control.has_rom.load(Ordering::Acquire)
            || control.netplay_paused.load(Ordering::Acquire)
            || control.user_paused.load(Ordering::Acquire);
        if idle {
            // Frame-advance: while user-paused (but with a ROM and not in
            // netplay), a pending step produces EXACTLY ONE unthrottled frame
            // then re-parks. The netplay/no-ROM idle never single-steps.
            if control.has_rom.load(Ordering::Acquire)
                && !control.netplay_paused.load(Ordering::Acquire)
                && control.take_frame_advance()
            {
                // `drive_one` would bail under the user-pause re-check (the
                // step happens precisely WHILE user-paused), so use the
                // frame-advance drive, which honors the netplay bail but
                // deliberately steps through the user pause.
                if drive_frame_advance(emu, audio.as_mut(), shared_input, control)
                    && proxy.send_event(AppEvent::EmuFrame).is_err()
                {
                    return;
                }
                continue;
            }
            std::thread::park_timeout(IDLE_PARK);
            continue;
        }

        // Fast-forward: run unthrottled and mute audio (a `None` sink) so the
        // producer doesn't outpace the cpal consumer and overrun the ring.
        let fast_forward = control.fast_forward.load(Ordering::Acquire);
        let regime = control.regime.load(Ordering::Acquire);
        let produced = if fast_forward {
            // Produce back-to-back regardless of regime: no pacer block, and
            // in the DISPLAY regime drain any pending present tick without
            // waiting on it. `drive_fast_forward` rebases `next_frame_time` to
            // `now` after the frame, so when FF is released the wall-clock /
            // display path resumes at the current instant with no catch-up
            // burst. Audio is muted (a `None` sink) so the ring never overruns.
            if regime == regime::DISPLAY {
                // Consume one pending tick if present, but never block on it.
                let _ = tick_rx.try_recv();
            }
            drive_fast_forward(emu, shared_input, control)
        } else if regime == regime::DISPLAY {
            // Fifo vsync is the clock: one frame per present tick, with a
            // watchdog that keeps producing if presents stop arriving.
            match tick_rx.recv_timeout(DISPLAY_TICK_TIMEOUT) {
                Ok(()) => drive_one(emu, audio.as_mut(), shared_input, control),
                Err(RecvTimeoutError::Timeout) => {
                    drive_wallclock(emu, audio.as_mut(), shared_input, control)
                }
                Err(RecvTimeoutError::Disconnected) => return,
            }
        } else {
            // Wall-clock / VRR: block precisely to the next frame, then
            // produce the due slot(s).
            let next = emu.lock().next_frame_time.unwrap_or_else(Instant::now);
            if Instant::now() < next {
                block_until_native(next);
            }
            drive_wallclock(emu, audio.as_mut(), shared_input, control)
        };

        if produced {
            // Wake the winit thread for housekeeping + redraw. A dead proxy
            // (event loop gone) means we're shutting down.
            if proxy.send_event(AppEvent::EmuFrame).is_err() {
                return;
            }
        }
    }
}

/// Build the produce sinks from the thread-owned audio producer (RA stays
/// on the winit thread, so `ra: None`).
#[cfg(feature = "retroachievements")]
fn sinks_for(audio: Option<&mut AudioProducer>) -> FrameSinks<'_> {
    FrameSinks {
        audio: audio.map(|a| a as &mut dyn crate::audio::AudioSink),
        ra: None,
    }
}

/// Build the produce sinks (no RA feature: just the audio producer).
#[cfg(not(feature = "retroachievements"))]
fn sinks_for(audio: Option<&mut AudioProducer>) -> FrameSinks<'_> {
    FrameSinks {
        audio: audio.map(|a| a as &mut dyn crate::audio::AudioSink),
    }
}

/// Display regime: latch + produce exactly one frame, mirroring
/// `App::display_sync_produce` (perf cost/sample + watchdog-base refresh).
/// Returns `true` if a frame was produced (so the caller pings the winit
/// thread); `false` if it bailed because netplay claimed the core between
/// the loop-top check and acquiring the lock (the TOCTOU close).
fn drive_one(
    emu: &EmuHandle,
    audio: Option<&mut AudioProducer>,
    shared_input: &SharedInput,
    control: &EmuControl,
) -> bool {
    let inputs = shared_input.load();
    let mut sinks = sinks_for(audio);
    let t0 = Instant::now();
    let mut guard = emu.lock();
    // Re-check UNDER the lock: the winit thread sets `netplay_paused` then
    // fences on this same lock, so once it holds the lock we observe the
    // flag and never advance the core out from under the rollback session.
    // v1.0.0 (BUG-9) — also honor a just-issued user pause under the lock so
    // the thread cannot produce one extra frame after `set_user_paused(true)`.
    if control.netplay_paused.load(Ordering::Acquire) || control.user_paused.load(Ordering::Acquire)
    {
        return false;
    }
    let core = &mut *guard;
    core.latch(&inputs);
    // RA is None in `sinks`, so `fx` is always default — discard it.
    let _ = core.produce_one_frame(&inputs, &mut sinks);
    core.perf.record_produce_cost(t0.elapsed());
    core.perf.record_produced(Instant::now());
    // v1.0.0 — display regime advances by the speed-scaled period.
    core.next_frame_time = Some(Instant::now() + core.effective_frame_duration());
    true
}

/// Frame-advance: latch + produce exactly one frame UNTHROTTLED while
/// user-paused. Unlike [`drive_one`] it does NOT bail on the user-pause flag
/// (the step happens precisely while paused) — but it DOES honor the netplay
/// pause under the lock (the TOCTOU close), since a netplay session must never
/// be single-stepped from here (the idle gate already excludes netplay, so
/// this is belt-and-braces). Audio passes through (a single stepped frame
/// cannot overrun the ring). Rebases `next_frame_time` to `now`.
fn drive_frame_advance(
    emu: &EmuHandle,
    audio: Option<&mut AudioProducer>,
    shared_input: &SharedInput,
    control: &EmuControl,
) -> bool {
    let inputs = shared_input.load();
    let mut sinks = sinks_for(audio);
    let t0 = Instant::now();
    let mut guard = emu.lock();
    if control.netplay_paused.load(Ordering::Acquire) {
        return false;
    }
    let core = &mut *guard;
    core.latch(&inputs);
    let _ = core.produce_one_frame(&inputs, &mut sinks);
    core.perf.record_produce_cost(t0.elapsed());
    core.perf.record_produced(Instant::now());
    core.next_frame_time = Some(Instant::now());
    true
}

/// Fast-forward: latch + produce exactly one frame UNTHROTTLED with audio
/// MUTED (a `None` sink, so the lock-free ring never overruns while the
/// producer outpaces the cpal consumer), then rebase `next_frame_time` to
/// `now` so releasing fast-forward resumes paced production without a
/// catch-up burst. Returns `false` on the same netplay/user-pause-claimed
/// bail as [`drive_one`].
fn drive_fast_forward(emu: &EmuHandle, shared_input: &SharedInput, control: &EmuControl) -> bool {
    let inputs = shared_input.load();
    let mut sinks = sinks_for(None);
    let t0 = Instant::now();
    let mut guard = emu.lock();
    if control.netplay_paused.load(Ordering::Acquire) || control.user_paused.load(Ordering::Acquire)
    {
        return false;
    }
    let core = &mut *guard;
    core.latch(&inputs);
    let _ = core.produce_one_frame(&inputs, &mut sinks);
    core.perf.record_produce_cost(t0.elapsed());
    core.perf.record_produced(Instant::now());
    // Rebase so leaving FF doesn't burst-catch-up the elapsed (fast) frames.
    core.next_frame_time = Some(Instant::now());
    true
}

/// Wall-clock / VRR (and the display watchdog): latch + produce the due
/// slot(s), mirroring `App`'s synchronous wall-clock pacer.
/// `produce_due_frames` records perf + advances `next_frame_time` itself.
/// Returns `false` on the same netplay-claimed-the-core bail as
/// [`drive_one`].
fn drive_wallclock(
    emu: &EmuHandle,
    audio: Option<&mut AudioProducer>,
    shared_input: &SharedInput,
    control: &EmuControl,
) -> bool {
    let inputs = shared_input.load();
    let mut sinks = sinks_for(audio);
    let now = Instant::now();
    let mut guard = emu.lock();
    // v1.0.0 (BUG-9) — see `drive_one`: honor netplay + user pause under the
    // lock so a just-issued pause stops the next produce.
    if control.netplay_paused.load(Ordering::Acquire) || control.user_paused.load(Ordering::Acquire)
    {
        return false;
    }
    let core = &mut *guard;
    let next = core.next_frame_time.unwrap_or(now);
    core.latch(&inputs);
    let _ = core.produce_due_frames(now, next, &inputs, &mut sinks);
    true
}

/// Native hybrid sleep-then-spin wait to a precise `target` (the same
/// strategy as `App::block_until_native`, duplicated here so the emu thread
/// has no dependency on `App`).
fn block_until_native(target: Instant) {
    loop {
        let now = Instant::now();
        if now >= target {
            return;
        }
        let remaining = target - now;
        if remaining > SPIN_MARGIN {
            std::thread::sleep(remaining.saturating_sub(SPIN_MARGIN).min(SLEEP_CHUNK));
        } else {
            std::hint::spin_loop();
        }
    }
}

/// Best-effort emu-thread priority elevation (Linux). Reduces the
/// occasional 10-40 ms OS descheduling that inflates the produce-cost tail
/// and the presented-jitter tail (a live 144 Hz capture showed both).
///
/// Strategy, in order, all per-THREAD (never the process) and degrading
/// SILENTLY when the privilege/rlimit is absent:
/// 1. `SCHED_RR` at a LOW real-time priority — preempts normal
///    (`SCHED_OTHER`) tasks so the emu thread runs on time, while a low
///    priority keeps it BELOW the audio callback thread (so audio always
///    wins) and the ~2 ms-per-frame spin can't monopolize a core on a
///    multi-core host. Needs `RLIMIT_RTPRIO` (the `realtime` group grants
///    it; see `realtime-privileges`).
/// 2. Fall back to a small negative `nice` — needs `RLIMIT_NICE`, also
///    granted by the `realtime` group. Boosts scheduling weight within
///    `SCHED_OTHER` without going real-time (safe with the spin).
/// 3. `PR_SET_TIMERSLACK` to 1 µs (always permitted for one's own thread) —
///    tightens the wall-clock pacer's sleep precision.
///
/// When none of the elevations are permitted (no `realtime` group, no
/// caps) the thread runs at default priority exactly as before — the
/// feature "just works" once the user joins the group and harms nothing
/// otherwise. macOS / Windows keep the documented no-op for now (Windows
/// `SetThreadPriority(ABOVE_NORMAL)` is the follow-up).
///
/// This is the only `unsafe` in `rustynes-frontend` (workspace `unsafe_code =
/// "warn"`): three libc scheduler syscalls on the calling thread, each with
/// a `// SAFETY:` justification below.
#[cfg(target_os = "linux")]
#[allow(unsafe_code)]
fn elevate_thread_priority() {
    // SAFETY: all three are standard libc thread/scheduler syscalls on the
    // CALLING thread (pid/who 0), with valid arguments; they only ever
    // return an error code we inspect, never write through our pointers
    // beyond the `sched_param` we own here.
    let rr = unsafe {
        // Low RR priority: above all SCHED_OTHER, below typical audio RT.
        const EMU_RT_PRIORITY: libc::c_int = 5;
        let param = libc::sched_param {
            sched_priority: EMU_RT_PRIORITY,
        };
        libc::sched_setscheduler(0, libc::SCHED_RR, &param) == 0
    };
    if rr {
        eprintln!("rustynes: emu thread elevated to SCHED_RR priority 5.");
    } else {
        // SAFETY: see above — `setpriority` on the calling thread.
        let niced = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, -10) == 0 };
        if niced {
            eprintln!("rustynes: emu thread niced to -10 (no RT rtprio limit).");
        } else {
            eprintln!(
                "rustynes: emu thread at default priority — for lower-latency \
                 scheduling, join the 'realtime' group (install realtime-privileges)."
            );
        }
    }
    // SAFETY: `prctl(PR_SET_TIMERSLACK, ...)` sets this thread's timer slack
    // (always permitted for one's own thread); extra args are ignored.
    unsafe {
        libc::prctl(libc::PR_SET_TIMERSLACK, 1_000_u64, 0, 0, 0);
    }
}

/// Non-Linux best-effort priority elevation: a documented no-op for now.
/// Rust's `std` sleeps already use high-resolution timers, so the pacer is
/// precise regardless; Windows `SetThreadPriority(ABOVE_NORMAL)` / macOS
/// QoS are the follow-up if profiling shows scheduler jitter there.
#[cfg(not(target_os = "linux"))]
#[allow(clippy::missing_const_for_fn)]
fn elevate_thread_priority() {}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::Buttons;

    #[test]
    fn shared_input_round_trips() {
        let si = SharedInput::default();
        let inputs = FrameInputs {
            buttons: [
                Buttons::from_bits_truncate(0b1010_0101),
                Buttons::from_bits_truncate(0b0101_1010),
                Buttons::empty(),
                Buttons::all(),
            ],
            four_score: true,
            rewind_held: true,
            hardcore_blocked: false,
            run_ahead: 2,
            expansion: ExpansionDevice::Vaus,
            mouse_nes: (123, 200),
            mouse_pressed: true,
            turbo_mask: Buttons::A | Buttons::B,
            turbo_period: 3,
            power_pad: 0b1010_0101_1100,
            mouse_delta: (-9, 42),
            mouse_right: true,
            family_keyboard: [0x01, 0x80, 0x00, 0xFF, 0x10, 0x00, 0x00, 0x00, 0x55],
        };
        si.publish(&inputs);
        let got = si.load();
        assert_eq!(got.buttons[0].bits(), inputs.buttons[0].bits());
        assert_eq!(got.buttons[3].bits(), inputs.buttons[3].bits());
        assert!(got.four_score);
        assert!(got.rewind_held);
        assert!(!got.hardcore_blocked);
        assert_eq!(got.run_ahead, 2);
        assert!(matches!(got.expansion, ExpansionDevice::Vaus));
        assert_eq!(got.mouse_nes, (123, 200));
        assert!(got.mouse_pressed);
        assert_eq!(got.turbo_mask, Buttons::A | Buttons::B);
        assert_eq!(got.turbo_period, 3);
        assert_eq!(got.power_pad, 0b1010_0101_1100);
        assert_eq!(got.mouse_delta, (-9, 42));
        assert!(got.mouse_right);
        assert_eq!(
            got.family_keyboard,
            [0x01, 0x80, 0x00, 0xFF, 0x10, 0x00, 0x00, 0x00, 0x55]
        );
    }

    #[test]
    fn shared_input_mouse_offscreen_sentinel() {
        let si = SharedInput::default();
        let mut inputs = FrameInputs {
            buttons: [Buttons::empty(); 4],
            four_score: false,
            rewind_held: false,
            hardcore_blocked: false,
            run_ahead: 0,
            expansion: ExpansionDevice::None,
            mouse_nes: (u16::MAX, u16::MAX),
            mouse_pressed: false,
            turbo_mask: Buttons::empty(),
            turbo_period: 2,
            power_pad: 0,
            mouse_delta: (0, 0),
            mouse_right: false,
            family_keyboard: [0; 9],
        };
        si.publish(&inputs);
        assert_eq!(si.load().mouse_nes, (u16::MAX, u16::MAX));
        inputs.run_ahead = 999; // clamps into u8 then back to u32.
        si.publish(&inputs);
        assert_eq!(si.load().run_ahead, 255);
    }

    #[test]
    fn control_regime_and_lifecycle() {
        let c = EmuControl::new();
        assert!(!c.is_netplay_paused());
        c.set_netplay_paused(true);
        assert!(c.is_netplay_paused());
        c.set_regime(regime::DISPLAY, Duration::from_micros(16_639));
        assert_eq!(c.regime.load(Ordering::Acquire), regime::DISPLAY);
        assert_eq!(c.frame_nanos.load(Ordering::Acquire), 16_639_000);
        c.set_has_rom(true);
        assert!(c.has_rom.load(Ordering::Acquire));
    }

    #[test]
    fn fast_forward_flag_round_trips() {
        let c = EmuControl::new();
        assert!(!c.is_fast_forward());
        c.set_fast_forward(true);
        assert!(c.is_fast_forward());
        c.set_fast_forward(false);
        assert!(!c.is_fast_forward());
    }

    #[test]
    fn frame_advance_take_is_compare_and_decrement() {
        let c = EmuControl::new();
        // Nothing pending yet.
        assert!(!c.take_frame_advance());
        // Two requests => two steps consumed, then empty.
        c.request_frame_advance();
        c.request_frame_advance();
        assert!(c.take_frame_advance());
        assert!(c.take_frame_advance());
        assert!(!c.take_frame_advance());
    }
}
