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
    /// The running emulator (None until a ROM is loaded).
    pub nes: Option<Nes>,
    /// TAS movie record/playback state machine.
    pub movie: MovieUi,
    /// Frame-pacing / audio instrumentation (Phase 0).
    pub perf: PerfStats,
    /// The framebuffer the renderer presents (with run-ahead active it is
    /// the visible FUTURE frame while `nes` holds the persistent one).
    pub present_fb: Vec<u8>,
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
    /// Run-ahead budget throttle (hysteresis on produce-cost p95).
    #[cfg(not(target_arch = "wasm32"))]
    pub runahead_throttled: bool,
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
    /// Empty core (no ROM).
    #[must_use]
    pub fn new() -> Self {
        Self {
            nes: None,
            movie: MovieUi::default(),
            perf: PerfStats::default(),
            present_fb: Vec::new(),
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
                    // v1.6.0 H — HD-pack HD audio: peek the `$4100` control
                    // register (side-effect-free) and mix the selected OGG track
                    // into the drained buffer IN PLACE before it reaches the
                    // queue. Output-only (see `hd_audio` docs); skipped when no
                    // audio pack is loaded.
                    #[cfg(feature = "hd-pack")]
                    if let Some(mixer) = self.hd_audio.as_mut() {
                        let control = nes.cpu_bus_peek(crate::hd_audio::HD_AUDIO_CONTROL);
                        mixer.mix(&mut self.audio_buf[..n], control);
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

                #[cfg(not(target_arch = "wasm32"))]
                if let Some(audio) = sinks.audio.as_mut() {
                    let target = ((u64::from(audio.sample_rate()) / 50) as usize).max(1024);
                    if self.audio_buf.len() < target {
                        self.audio_buf.resize(target, 0.0);
                    }
                    let n = nes.drain_audio_into(&mut self.audio_buf);
                    // v1.6.0 H — HD-pack HD audio: peek the `$4100` control
                    // register (side-effect-free) and mix the selected OGG track
                    // into the drained buffer IN PLACE before the DRC stage.
                    // Output-only (see `hd_audio` docs); skipped when no audio
                    // pack is loaded.
                    #[cfg(feature = "hd-pack")]
                    if let Some(mixer) = self.hd_audio.as_mut() {
                        let control = nes.cpu_bus_peek(crate::hd_audio::HD_AUDIO_CONTROL);
                        mixer.mix(&mut self.audio_buf[..n], control);
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
