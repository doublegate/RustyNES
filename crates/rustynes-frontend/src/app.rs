//! winit `ApplicationHandler` that drives the emulator + render + audio.
//!
//! # Frame pacing (v2.8.0 Phase 2 — the display-sync matrix)
//!
//! Native pacing is a three-regime matrix (`[graphics] pacing_mode`,
//! resolved in `App::resolve_pacing`):
//!
//! - **display-sync** (`auto` engages it when the monitor refresh is within
//!   0.5% of the console rate): Fifo vsync is the clock — `RedrawRequested`
//!   produces exactly ONE emulated frame per display refresh
//!   (`App::display_sync_produce`), the ≤0.5% speed bend is invisible,
//!   and the audio DRC absorbs the rate difference. Zero beat judder on
//!   matching fixed-refresh panels. An occlusion watchdog in
//!   `about_to_wait` keeps emulation+audio alive when the compositor
//!   throttles redraws; sustained missed presents fall back to wallclock.
//! - **vrr** (user-asserted G-Sync/FreeSync): Fifo + the wall-clock pacer
//!   at the exact console rate; the variable-refresh display follows the
//!   emulator's presents.
//! - **wallclock** (the fallback + high-refresh fixed panels): the
//!   strategy described below — the pre-v2.8.0 behavior.
//!
//! The historical rationale for wall-clock pacing (still the fallback
//! regime): without a skew gate, `PresentMode::Fifo` would tick
//! `Nes::run_frame` at the monitor's refresh rate — fine at 60 Hz, but on
//! a 144 Hz monitor that's `144 / 60.0988 ≈ 2.4×` real-hardware speed.
//!
//! Wall-clock pacing strategy:
//!
//! 1. `next_frame_time = Instant + frame_duration` (per-region, exposed
//!    by `Nes::frame_duration()`).
//! 2. **Native** keeps `ControlFlow::Poll` and blocks to the exact target
//!    inside the pacer with a hybrid *sleep-then-spin* wait: it sleeps
//!    until ~1.5 ms before the target (cheap, no battery burn), then
//!    busy-spins (`std::hint::spin_loop`) the last stretch to the precise
//!    `Instant`. This is deliberately **not** `ControlFlow::WaitUntil`:
//!    winit documents `WaitUntil` wakes as a lower bound, and on
//!    X11/Wayland the wake is serviced by the `calloop` poll dispatch
//!    interleaved with compositor/input events, so its cadence jitters by
//!    several ms. With a non-vsync present mode (`Mailbox`) that jitter is
//!    shown directly as uneven motion — the residual stutter the present-
//!    mode default did not fix. Spinning the last ~1.5 ms removes it.
//! 3. **wasm32** drives production from `requestAnimationFrame`, not from
//!    `ControlFlow`. There is no usable `thread::sleep`/spin in the
//!    browser, and in winit 0.30 the web backend services
//!    `ControlFlow::Poll`/`WaitUntil` via `Scheduler.postTask`/
//!    `setTimeout` — neither is synced to the display refresh, so pacing
//!    production off them jitters. The only vsync-synced signal winit
//!    exposes on the web is `Window::request_redraw()` →
//!    `RedrawRequested` (winit's web backend wires `request_redraw` to
//!    `requestAnimationFrame`). So on wasm32 the frame loop lives in
//!    `RedrawRequested` (`App::pace_and_produce_wasm`): it produces the
//!    frames due by `web_time::Instant` delta (so wall-clock NTSC speed
//!    stays correct on non-60 Hz panels), then re-arms the next rAF via
//!    `request_redraw()` UNCONDITIONALLY (every tick, including the
//!    pre-ROM `nes.is_none()` path — the re-arm is the sole heartbeat;
//!    skipping it on any tick stalls the loop and freezes the canvas).
//!    The idle `ControlFlow` is `Wait`, NOT `Poll`: on winit's web
//!    backend `Poll` reschedules immediately via
//!    `Scheduler.postTask`/`setTimeout(0)`, busy-looping in PARALLEL with
//!    the rAF loop and starving the heavy emulation (the v1.3.2 stutter +
//!    periodic freezes). `Wait` lets the loop sleep until the next rAF
//!    callback, so production is driven purely from `requestAnimationFrame`
//!    with no competing scheduler.
//! 4. On each iteration we drain however many `frame_duration` slots have
//!    elapsed (≤ a small catch-up window of 3 frames; beyond that we
//!    snap to `now` so a hibernated process doesn't run 50 frames in a
//!    burst on resume).
//! 5. Rendering (`RedrawRequested`) only **presents** the latest
//!    framebuffer in the wallclock/vrr regimes; it never advances the
//!    emulator there. The display can therefore re-present the same frame
//!    multiple times on a high-refresh monitor without speeding the
//!    emulator up. (In the display-sync regime, production deliberately
//!    moves INTO `RedrawRequested` — one frame per refresh IS the regime.)
//!
//! Audio is produced once per emulator frame (~735 samples at 44.1 kHz
//! when paced at 60.0988 Hz) and pushed through the DRC resampler stage
//! ([`crate::audio::AudioOutput::push_samples`]) into the lock-free
//! [`crate::audio::SampleQueue`]; the CPAL thread drains at the host
//! rate. Dynamic rate control servos the queue occupancy to the
//! `[audio] latency_ms` target so clock drift never surfaces as pops or
//! silence gaps.
//!
//! Save state, rewind and config persistence: see `save_state.rs`,
//! `config.rs`, and the `rustynes_core::Nes::{snapshot, restore, rewind_*}`
//! API.

// v2.8.0 Phase 5 — `EmuHandle` guard scoping in this module is deliberate
// and explicit: a guard spans exactly the region that needs the core (and
// never a call into another locking helper — the mutex is non-reentrant),
// with the genuinely blocking work (file dialogs, file I/O, the
// debugger-hidden present) already restructured to run with the guard
// dropped. The nursery drop-tightening lint would scatter `drop(guard)`
// calls / rebinds through the remaining short regions without changing
// behavior; readability of the locking regions wins.
#![allow(clippy::significant_drop_tightening)]

// `Path`/`PathBuf` are only used by the native filesystem ROM-load +
// save-state paths; wasm32 loads ROMs via the browser file picker.
#[cfg(not(target_arch = "wasm32"))]
use std::path::{Path, PathBuf};
use std::sync::Arc;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Duration;

// For the wasm32 `with_canvas` downcast in `create_window`.
#[cfg(target_arch = "wasm32")]
use wasm_bindgen::JsCast;
// v1.3.0 Sprint 1.1 — `Instant` from `web_time` instead of `std::time`.
// On native, `web_time::Instant` IS `std::time::Instant` (re-exported).
// On wasm32, it routes to `Performance.now()`. winit's
// `ControlFlow::WaitUntil(Instant)` expects `web_time::Instant` on
// wasm32, so using this consistently fixes the type mismatch.
use web_time::Instant;

use rustynes_core::Nes;
use winit::application::ApplicationHandler;
use winit::dpi::LogicalSize;
use winit::event::WindowEvent;
use winit::event_loop::{ActiveEventLoop, ControlFlow};
use winit::window::{Window, WindowId};

#[cfg(not(target_arch = "wasm32"))]
use crate::audio::AudioOutput;
use crate::config::Config;
use crate::debugger::DebuggerOverlay;
use crate::gfx::{Gfx, NES_H, NES_W};
use crate::input::{InputState, SysAction};
#[cfg(not(target_arch = "wasm32"))]
use crate::save_state;

/// v1.3.0 Sprint 1.4 — winit custom user-event type, used by both
/// native and wasm32 (native simply never sends one).
///
/// On wasm32 the wgpu init is async and the ROM arrives via the
/// browser file picker, so neither can be produced synchronously
/// inside `ApplicationHandler::resumed`. Instead they're delivered
/// back into the event loop as user events via an
/// [`winit::event_loop::EventLoopProxy`]. See
/// `docs/audit/v1.3-sprint-1.4-winit-wgpu-unification-2026-05-24.md`.
pub enum AppEvent {
    /// The async `Gfx::new` future resolved (wasm32). Boxed because
    /// `Gfx` is large and clippy flags a big enum variant otherwise.
    GfxReady(Box<Gfx>),
    /// The browser file picker delivered ROM bytes (wasm32).
    RomLoaded(Vec<u8>),
    /// v1.6.0 Sprint 4 — the browser file picker delivered `.rnm` movie
    /// bytes (wasm32). Deserialized + played back via the `App`'s movie
    /// state machine (the gesture-driven `<input>` lives in `wasm_winit`).
    MovieLoaded(Vec<u8>),
    /// v2.2.0 — the browser file picker delivered FDS BIOS (`disksys.rom`)
    /// bytes (wasm32). Validated (8 KiB) + stashed for the FDS disk load.
    FdsBiosLoaded(Vec<u8>),
    /// v2.8.0 Phase 5 increment 3 — the dedicated emulation thread produced
    /// a frame; the winit thread does the UI-side housekeeping (perf/HUD
    /// pushes, FDS flush, perf logging, RA drive) + requests a redraw.
    /// Native-only + behind the `emu-thread` feature.
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    EmuFrame,
}

/// Default window scale (each NES pixel becomes Nx host pixels at startup).
const INITIAL_SCALE: u32 = 3;

/// The required size of the Famicom Disk System BIOS (`disksys.rom`): 8 KiB.
const FDS_BIOS_SIZE: usize = 8192;

/// Detect whether `bytes` is a Famicom Disk System `.fds` disk image.
///
/// Recognizes both container forms the core parser accepts (see
/// `rustynes-mappers::fds`):
/// - the fwNES 16-byte header form, which opens with the ASCII magic
///   `"FDS\x1A"`; and
/// - the headerless raw form, whose first side opens with the disk-info
///   signature `\x01*NINTENDO-HVC*`.
///
/// A standard iNES / NES 2.0 cartridge opens with `"NES\x1A"`, so this never
/// misfires on the `.nes` path — the standard ROM load stays untouched.
#[must_use]
fn is_fds_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"FDS\x1A") || bytes.starts_with(b"\x01*NINTENDO-HVC*")
}

/// Native-only precise-pacing spin margin. When the next frame is within
/// this window we busy-spin (`std::hint::spin_loop`) to the exact target
/// `Instant` instead of sleeping. Sleeping covers everything before the
/// margin (cheap, no battery burn); the spin removes the OS-timer /
/// event-loop wake jitter from the last stretch so frame *production* lands
/// on a precise cadence.
///
/// 2 ms comfortably covers the `thread::sleep` slop on a tuned Linux timer
/// plus the coarser `ControlFlow::WaitUntil` / `calloop`-poll wake latency
/// on X11/Wayland (winit's own docs note `WaitUntil` wakes are a lower
/// bound, not precise — see the module-level `Frame pacing` doc). Spinning
/// ~2 ms out of every 16.6 ms frame costs ~12% of one core, still cheap.
///
/// v1.3.x: bumped 1.5 ms -> 2 ms and paired with [`SLEEP_CHUNK`]-capped
/// sleeps (see [`App::block_until_native`]). On a *loaded* or un-tuned host
/// a single `thread::sleep` can overshoot its requested duration by several
/// ms; with the old single-shot `sleep(remaining - margin)` that overshoot
/// could blow straight past `target`, so the precise spin never engaged and
/// the frame was produced late — the residual stutter. Capping each sleep
/// and re-measuring keeps the wait converging on the target even when an
/// individual sleep oversleeps.
#[cfg(not(target_arch = "wasm32"))]
const SPIN_MARGIN: Duration = Duration::from_millis(2);

/// Native-only: maximum length of any single `thread::sleep` inside the
/// sleep-then-spin pacer. Capping the nap (rather than sleeping the whole
/// `remaining - SPIN_MARGIN` in one shot) bounds how far a single OS
/// oversleep can overshoot before [`App::block_until_native`] re-measures
/// `now` and re-decides. 2 ms keeps the loop responsive near the deadline
/// while staying coarse enough that the sleep count per frame is tiny
/// (≈ 7 naps across a 14.6 ms pre-spin window) — negligible overhead.
#[cfg(not(target_arch = "wasm32"))]
const SLEEP_CHUNK: Duration = Duration::from_millis(2);

/// v2.8.0 Phase 2 — the resolved frame-pacing regime (the canonical
/// display-sync matrix; see `[graphics] pacing_mode` in `config.rs`).
/// Native-only: the wasm rAF loop has its own pacing.
#[cfg(not(target_arch = "wasm32"))]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ActivePacing {
    /// Wall-clock pacer + configured present mode (Mailbox default) — the
    /// pre-v2.8.0 behavior; right for high-refresh fixed panels.
    Wallclock,
    /// Fifo vsync is the clock: exactly one emulated frame per display
    /// refresh (speed bent <= the skew gate; audio DRC absorbs it). Zero
    /// judder on fixed panels whose refresh matches the console rate.
    Display,
    /// VRR (G-Sync/FreeSync): Fifo + the wall-clock pacer at the exact
    /// console rate; the display follows the emulator's presents.
    Vrr,
}

/// v2.8.0 Phase 2 — how far the display refresh may deviate from the ROM's
/// nominal rate for display-sync to engage (0.5%: 60.0988 Hz content on a
/// 59.95-60.10 Hz panel run -0.25%..+0.0% — invisible; the audio DRC band
/// is ±0.5% so it absorbs the difference with margin).
#[cfg(not(target_arch = "wasm32"))]
const DISPLAY_SYNC_MAX_SKEW: f64 = 0.005;

/// v2.8.0 Phase 2 — display-sync occlusion watchdog: when `RedrawRequested`
/// stops arriving for this long (minimized / fully occluded window on a
/// frame-callback-throttled compositor), `about_to_wait` produces due
/// frames wall-clock so emulation + audio + netplay keep running.
#[cfg(not(target_arch = "wasm32"))]
const DISPLAY_SYNC_WATCHDOG: Duration = Duration::from_millis(25);

/// Application state. Constructed in `resumed()` (per winit 0.30 idiom),
/// torn down on exit.
// The app legitimately tracks several independent boolean modes (exit
// request, mouse button, display-sync fallback, run-ahead throttle, ...);
// packing them into bitflags would obscure, not clarify.
#[allow(clippy::struct_excessive_bools)]
pub struct App {
    rom_bytes: Vec<u8>,
    rom_label: String,
    /// v2.8.0 Phase 5 — the emulation core: ALL per-frame produce state
    /// (the `Nes`, movie, run-ahead, perf, presented framebuffer, pacing
    /// deadlines, …) extracted from `App` into `emu.rs`. `App` keeps the
    /// platform-resident surface (window/gfx, cpal stream, input devices,
    /// config, dialogs) and drives the core from the pacer.
    ///
    /// Increment 2b: shared `Arc<Mutex<_>>` handle ([`crate::emu::EmuHandle`]).
    /// ⚠️ Non-reentrant — see the handle's docs for the guard-scoping rules;
    /// no guard may span a call into another helper that locks.
    emu: crate::emu::EmuHandle,
    /// v2.8.0 Phase 5 — render-path staging copy of the presented
    /// framebuffer, so the common (debugger-hidden) render never holds the
    /// emu lock across the GPU encode + present (Fifo vsync would block the
    /// emulation thread). Reused; never shrinks below one frame.
    present_staging: Vec<u8>,
    gfx: Option<Gfx>,
    /// Native audio output (cpal). wasm32 uses the Web Audio path in
    /// `wasm.rs` instead, so this field is native-only.
    #[cfg(not(target_arch = "wasm32"))]
    audio: Option<AudioOutput>,
    input: InputState,
    config: Config,
    #[cfg(not(target_arch = "wasm32"))]
    data_dir: Option<PathBuf>,
    /// Sprint 5-3 debugger overlay (lazily constructed alongside `Gfx`).
    debugger: Option<DebuggerOverlay>,
    /// gilrs runtime for gamepad polling. `None` if gilrs fails to
    /// initialize (e.g. no input subsystem available); the emulator
    /// falls back to keyboard-only. Native-only — wasm32 uses
    /// browser gamepad/keyboard events.
    #[cfg(not(target_arch = "wasm32"))]
    gamepad: Option<gilrs::Gilrs>,
    /// wasm32 — proxy for delivering async `Gfx` + ROM bytes back
    /// into the event loop. `None` on native.
    #[cfg(target_arch = "wasm32")]
    proxy: Option<winit::event_loop::EventLoopProxy<AppEvent>>,
    /// Raised when the user asks to quit (Esc, window close).
    should_exit: bool,
    /// v2.8.0 Phase 2 — the resolved pacing regime (config `pacing_mode` ×
    /// measured/declared display refresh × ROM region). Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    active_pacing: ActivePacing,
    /// Sticky display-sync fallback: set when display-sync sustained missed
    /// presents (or was requested with an out-of-band refresh), so the
    /// session stays on the wall-clock pacer until the user re-applies.
    #[cfg(not(target_arch = "wasm32"))]
    display_fallback: bool,
    /// Timestamp of the last `RedrawRequested` seen while display-synced —
    /// the occlusion-watchdog input.
    #[cfg(not(target_arch = "wasm32"))]
    last_redraw: Option<Instant>,
    /// Presents since the last display-sync health check (the sustained-
    /// miss fallback test runs every 60 presents, not every frame).
    #[cfg(not(target_arch = "wasm32"))]
    presents_since_check: u32,
    /// v2.8.0 — opt-in interval CSV performance logger, driven by the Perf
    /// panel's "Logging" checkbox (default OFF). Writes under `perf-logs/`.
    #[cfg(not(target_arch = "wasm32"))]
    perf_logger: crate::perf_log::PerfLogger,
    /// v2.8.0 Phase 5 increment 3 — the dedicated emulation thread. Spawned
    /// once `Gfx` + audio are ready; idles until a ROM loads; owns
    /// single-player frame production (the winit thread only presents +
    /// services UI). `None` until spawned. Native-only + `emu-thread`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    emu_thread: Option<crate::emu_thread::EmuThread>,
    /// v2.8.0 Phase 5 increment 3 — the `EventLoopProxy` the emu thread uses
    /// to deliver [`AppEvent::EmuFrame`]. Captured by `run` before
    /// `run_app`. Native-only + `emu-thread` (wasm uses its own `proxy`).
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    emu_proxy: Option<winit::event_loop::EventLoopProxy<AppEvent>>,
    /// v2.3.0 — netplay (rollback netcode) state machine. Native-only: it
    /// drives a `std::net::UdpSocket` (absent on wasm32). When active it
    /// REPLACES the single-player `run_frame` in `produce_one_frame`; when
    /// idle the produce path is byte-for-byte the single-player path.
    /// Mutually exclusive with movie record/playback (guarded at the start
    /// gestures + the produce hook).
    #[cfg(not(target_arch = "wasm32"))]
    netplay: crate::netplay_ui::NetplayUi,
    /// v2.7.0 — browser (WebRTC) netplay driver + lobby. wasm-only: a browser
    /// cannot open a UDP socket, so this drives the same `RollbackSession` core
    /// over a `WebRtcTransport` brokered through a WebSocket signaling server.
    /// `None` until a ROM is loaded (the session needs the ROM hash). When
    /// active it REPLACES the single-player `run_frame` in the wasm produce
    /// path, exactly as the native `netplay` field does natively.
    #[cfg(target_arch = "wasm32")]
    browser_netplay: Option<crate::wasm_netplay::BrowserNetplay>,
    /// v2.7.0 — the wasm netplay lobby UI state (signaling URL / room / players
    /// / pending connect-or-leave request). wasm-only.
    #[cfg(target_arch = "wasm32")]
    wasm_lobby: crate::wasm_lobby::WasmLobbyState,
    /// v2.1.0 — last cursor position in physical window pixels, used to derive
    /// the Zapper aim point / Vaus paddle position. `None` until the first
    /// `CursorMoved`. Native-only input source (mouse).
    #[cfg(not(target_arch = "wasm32"))]
    cursor_pos: Option<(f64, f64)>,
    /// v2.1.0 — current window inner size in physical pixels (tracked from
    /// `Resized`), needed to scale `cursor_pos` into the 256x240 NES screen.
    #[cfg(not(target_arch = "wasm32"))]
    window_size: (u32, u32),
    /// v2.1.0 — whether the left mouse button is currently held (Zapper
    /// trigger / Vaus fire). Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    mouse_pressed: bool,
    /// v2.2.0 — the uploaded Famicom Disk System BIOS (`disksys.rom`) bytes
    /// (wasm32). The browser build has no filesystem prompt, so the user
    /// uploads the BIOS via a `<input type="file">` and it is stashed here for
    /// the FDS disk-load path. `None` until uploaded. wasm-only.
    #[cfg(target_arch = "wasm32")]
    fds_bios_bytes: Option<Vec<u8>>,
    /// v2.7.0 — `RetroAchievements` session. `Some` whenever the
    /// `retroachievements` feature is built (so the login dialog always works;
    /// the `[retroachievements] enabled` flag only gates the startup auto-login,
    /// v2.7.1). Native-only (it links the vendored rcheevos C library). The
    /// per-frame produce hook drives `do_frame`/`idle` (idle + cheap when no
    /// user is logged in), and the hardcore-gating predicate refuses save-state
    /// load / rewind / cheats / RAM-watch.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    ra: Option<crate::ra_session::RaSession>,
}

/// Frames to hold a Vs. System coin-insert latch (~50 ms at 60 fps).
const VS_COIN_HOLD_FRAMES: u8 = 3;

/// Resolve the effective Vs. System DIP-switch byte from the config and an
/// optional per-game database entry (v2.7.0).
///
/// Precedence: **explicit config `[vs] dip` > per-game DB default > 0**. The
/// user signals "explicit" via `[vs] dip_set = true` (serde-default `false`, so
/// the DB preset is preferred for in-DB games unless the user opts in). A game
/// not in the DB falls back to the config `dip` (0 by default).
const fn resolve_vs_dip(
    cfg: crate::config::VsConfig,
    db_entry: Option<rustynes_core::VsDbEntry>,
) -> u8 {
    if cfg.dip_set {
        cfg.dip
    } else if let Some(entry) = db_entry {
        entry.vs_dip
    } else {
        cfg.dip
    }
}

/// Emit the "this is a Vs. dual-system cart" note once such a ROM loads.
/// These titles need two CPUs + two PPUs; this single-system core renders only a
/// black/attract screen. Full support is a documented future feature.
fn log_dual_system_note() {
    let note = "RustyNES: this is a Vs. DualSystem title (two CPUs / two PPUs, \
                e.g. Tennis / Mahjong / Wrecking Crew / Balloon Fight). The \
                single-system core cannot boot it past the attract screen; \
                DualSystem support is a planned future feature.";
    #[cfg(not(target_arch = "wasm32"))]
    eprintln!("{note}");
    #[cfg(target_arch = "wasm32")]
    web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(note));
}

impl App {
    /// Build an app from a path to a `.nes` file (native).
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if the file can't be read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(rom_path: &std::path::Path) -> std::io::Result<Self> {
        let rom_bytes = std::fs::read(rom_path)?;
        let rom_label = rom_path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("rom.nes")
            .to_string();
        let config = Config::load_or_default();
        let input = InputState::from_config(&config.input);
        let data_dir = Config::default_data_dir();
        Ok(Self {
            rom_bytes,
            rom_label,
            emu: crate::emu::EmuHandle::new(crate::emu::EmuCore::new()),
            present_staging: Vec::new(),
            gfx: None,
            audio: None,
            input,
            config,
            data_dir,
            debugger: None,
            gamepad: gilrs::Gilrs::new()
                .map_err(|e| {
                    eprintln!("rustynes: gamepad subsystem disabled: {e}");
                })
                .ok(),
            should_exit: false,
            // Placeholder until `resumed()` reads the cartridge region;
            // any reasonable default (NTSC) keeps `WaitUntil` math sane
            // before the ROM is loaded.
            active_pacing: ActivePacing::Wallclock,
            display_fallback: false,
            last_redraw: None,
            presents_since_check: 0,
            perf_logger: crate::perf_log::PerfLogger::default(),
            #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
            emu_thread: None,
            #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
            emu_proxy: None,
            netplay: crate::netplay_ui::NetplayUi::default(),
            cursor_pos: None,
            window_size: (NES_W * INITIAL_SCALE, NES_H * INITIAL_SCALE),
            mouse_pressed: false,
            #[cfg(feature = "retroachievements")]
            ra: Some(Self::init_ra_session()),
        })
    }

    /// v2.7.0 — build the `RetroAchievements` session. When the
    /// `retroachievements` feature is compiled in, the session is **always**
    /// created so the login dialog works the first time (the `enabled` flag only
    /// gates the startup *auto-login* from a saved token — NOT whether the
    /// session exists; otherwise a first-time user could never log in, since
    /// `enabled` is only set true *after* a successful login). The per-frame RA
    /// drive idles cheaply when no user is logged in / no game is loaded.
    /// Native-only + feature-gated.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn init_ra_session() -> crate::ra_session::RaSession {
        // The config used here is loaded freshly so the helper is callable from
        // the struct literal (it cannot borrow `self.config`); the caller has
        // the same config in hand. Keeping it a free read keeps `new` simple.
        let config = Config::load_or_default();
        let mut session = crate::ra_session::RaSession::new(&config.retroachievements);
        // `auto_login` is a no-op unless `enabled` + a saved username/token.
        session.auto_login(&config.retroachievements);
        session
    }

    /// v1.3.0 Sprint 1.4 — build an empty app for wasm32 (no ROM
    /// yet; it arrives via the browser file picker as an
    /// [`AppEvent::RomLoaded`]). Config is the in-memory default
    /// (no filesystem on the web). The `proxy` is wired by `run`.
    #[cfg(target_arch = "wasm32")]
    #[must_use]
    pub fn new_empty(proxy: winit::event_loop::EventLoopProxy<AppEvent>) -> Self {
        let config = Config::default();
        let input = InputState::from_config(&config.input);
        Self {
            rom_bytes: Vec::new(),
            rom_label: "(no ROM)".to_string(),
            emu: crate::emu::EmuHandle::new(crate::emu::EmuCore::new()),
            present_staging: Vec::new(),
            gfx: None,
            input,
            config,
            debugger: None,
            proxy: Some(proxy),
            should_exit: false,
            browser_netplay: None,
            wasm_lobby: crate::wasm_lobby::WasmLobbyState::default(),
            fds_bios_bytes: None,
        }
    }

    fn create_window(&self, event_loop: &ActiveEventLoop) -> Result<Arc<Window>, String> {
        let attrs = Window::default_attributes()
            .with_title(format!("RustyNES v2 - {}", self.rom_label))
            .with_inner_size(LogicalSize::new(
                NES_W * INITIAL_SCALE,
                NES_H * INITIAL_SCALE,
            ));

        // v1.3.0 Sprint 1.4 — on wasm32, render INTO the existing
        // `<canvas id="nes-canvas">` from index.html (so its CSS
        // sizing + the page layout apply) rather than letting winit
        // create a detached canvas. Per the winit 0.30 web platform
        // docs, this is `WindowAttributesExtWebSys::with_canvas`.
        #[cfg(target_arch = "wasm32")]
        let attrs = {
            use winit::platform::web::WindowAttributesExtWebSys;
            let canvas = web_sys::window()
                .and_then(|w| w.document())
                .and_then(|d| d.get_element_by_id("nes-canvas"))
                .and_then(|el| el.dyn_into::<web_sys::HtmlCanvasElement>().ok());
            if canvas.is_none() {
                return Err("missing <canvas id=\"nes-canvas\">".to_string());
            }
            attrs.with_canvas(canvas)
        };

        event_loop
            .create_window(attrs)
            .map(Arc::new)
            .map_err(|e| e.to_string())
    }

    /// Open the rfd file dialog. On selection, hand off to
    /// [`Self::load_rom_from_path`]. No-op if the dialog returns `None`
    /// (user cancelled).
    ///
    /// v1.3.0 Sprint 1.1 — rfd is native-only (it depends on
    /// `xdg-portal`/`AppKit`/`COM`); on wasm32 the ROM picker uses
    /// the browser-native `<input type="file">` path wired by
    /// Sprint 1.3. This function is a no-op on wasm32.
    #[cfg(not(target_arch = "wasm32"))]
    fn open_rom_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("NES / FDS image", &["nes", "fds"])
            .pick_file()
        else {
            return;
        };
        self.load_rom_from_path(&path);
    }

    /// Apply the Vs. System per-game database (v2.7.0) to a freshly built
    /// `Nes`, then apply the effective DIP switches.
    ///
    /// Looks up the ROM's SHA-256 in [`rustynes_core::vs_db`]. When found, the DB's
    /// PPU type is applied unconditionally (it is authoritative for the output
    /// palette — iNES-1.0 dumps default to the 2C03 and need the DB to pick the
    /// right 2C04-000x / 2C05 LUT). The DIP follows a precedence chain:
    /// explicit `[vs] dip` config > per-game DB default > 0. This is a no-op on
    /// non-Vs. carts (`set_vs_ppu_type` / `set_vs_dip` ignore them) and changes
    /// nothing about normal NES play.
    fn apply_vs_db(&self, nes: &mut Nes) {
        let db_entry = rustynes_core::vs_db::lookup(nes.rom_sha256());
        // The DB is authoritative for the palette: apply its PPU type whenever
        // the ROM is in the DB, independent of the DIP precedence below.
        if let Some(entry) = db_entry {
            nes.set_vs_ppu_type(entry.vs_ppu_type);
            // A Vs. DualSystem cart (two CPUs + two PPUs) cannot boot past the
            // attract screen on this single-system core. Surface a clear note
            // rather than leaving the user staring at a black screen. Full
            // two-system support is a documented future feature
            // (docs/audit/vs-dualsystem-design-2026-06-11.md).
            if entry.dual_system {
                log_dual_system_note();
            }
        }
        let dip = resolve_vs_dip(self.config.vs, db_entry);
        nes.set_vs_dip(dip);
    }

    /// Replace the current ROM with the one at `path`. Reuses the
    /// existing audio queue and rebuilds the `Nes` (and rewind ring if
    /// enabled) against the new cartridge. On any error, the old `Nes`
    /// is preserved so the running session isn't lost.
    ///
    /// Native-only (filesystem). On wasm32 the ROM arrives as
    /// [`AppEvent::RomLoaded`] from the browser file picker.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_rom_from_path(&mut self, path: &Path) {
        let bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("rustynes: failed to read {}: {e}", path.display());
                return;
            }
        };
        let sample_rate = self.audio.as_ref().map_or(44_100, |a| a.sample_rate);
        // v2.2.0 — a Famicom Disk System `.fds` image needs the disksys.rom
        // BIOS + the writable-disk save path; the standard cartridge `.nes`
        // path is unchanged. Detect by the disk-image magic (never matches a
        // `"NES\x1A"` cartridge).
        let mut nes = if is_fds_image(&bytes) {
            match self.build_fds_nes(&bytes, sample_rate) {
                Some(n) => n,
                // BIOS cancelled / wrong size / unparseable disk: keep the
                // running session (already logged), don't crash.
                None => return,
            }
        } else {
            // Not FDS — clear any prior FDS save key so a later flush is inert.
            self.emu.lock().fds_disk_sha256 = None;
            match Nes::from_rom_with_sample_rate(&bytes, sample_rate) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("rustynes: failed to load ROM {}: {e}", path.display());
                    return;
                }
            }
        };
        if self.config.rewind.enabled {
            let max_bytes: usize =
                ((self.config.rewind.max_seconds as usize) * 60).max(60) * 200 * 1024;
            nes.enable_rewind_with(
                max_bytes.min(rustynes_core::REWIND_DEFAULT_MAX_BYTES),
                self.config.rewind.keyframe_period.max(1),
            );
        }
        // v1.7.0 — arm the Four Score 4-player adapter per config. Off by
        // default, so `$4016`/`$4017` reads stay byte-identical to two
        // controllers until the user enables it in the rebind UI.
        nes.set_four_score(self.config.input.four_score);
        // v2.5.0 — apply the Vs. System DIP switches (no-op for non-Vs. games).
        // v2.7.0 — the per-game DB supplies the correct PPU palette + a DIP
        // preset (explicit config dip wins over the DB; see `apply_vs_db`).
        self.apply_vs_db(&mut nes);

        self.rom_label = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("rom.nes")
            .to_string();
        self.rom_bytes = bytes;
        {
            let mut guard = self.emu.lock();
            let emu = &mut *guard;
            emu.frame_duration = nes.frame_duration();
            emu.next_frame_time = Some(Instant::now() + emu.frame_duration);
            emu.audio_buf.clear();
            emu.perf.clear();
            emu.present_fb.clear();
            emu.nes = Some(nes);
        }
        // v2.8.0 Phase 5 increment 3 — a reload keeps the pacing regime but
        // may change the region (NTSC<->PAL frame duration); refresh the
        // emulation thread's frame + keep `has_rom` set.
        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
        {
            self.publish_emu_thread_regime();
            if let Some(thread) = self.emu_thread.as_ref() {
                thread.control().set_has_rom(true);
            }
        }
        self.apply_cheats_for_current_rom();
        // v2.7.0 — (re)identify the ROM with RetroAchievements + load its saved
        // progress sidecar. No-op when no RA session is active.
        #[cfg(feature = "retroachievements")]
        self.load_ra_game();
        // v2.1.0 — attach the configured non-standard input device (if any) on
        // the player-2 port. No-op when ExpansionDevice::None.
        #[cfg(not(target_arch = "wasm32"))]
        self.sync_expansion_device();
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window
                .set_title(&format!("RustyNES v2 - {}", self.rom_label));
        }
        eprintln!("rustynes: loaded {}", path.display());
    }

    /// v1.6.0 / v1.7.0 — load the current ROM's persisted cheats, apply every
    /// ENABLED Game Genie code to the running `Nes`, prime the enabled raw RAM
    /// cheats for the per-frame produce path, and seed the debugger's cheat
    /// panel with both lists + the per-ROM persistence context. Native-only —
    /// the wasm32 build has no filesystem, so no cheats are persisted there
    /// (the in-memory panel still works). No-op if no `Nes` or no data dir.
    #[cfg(not(target_arch = "wasm32"))]
    fn apply_cheats_for_current_rom(&mut self) {
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let Some(nes) = emu.nes.as_mut() else {
            return;
        };
        let Some(dir) = self.data_dir.as_ref() else {
            return;
        };
        let rom_sha256 = *nes.rom_sha256();
        let loaded = crate::cheats::load(dir, &rom_sha256);
        nes.clear_genie_codes();
        for entry in &loaded.genie {
            if entry.enabled {
                if let Err(e) = nes.add_genie_code(&entry.code) {
                    eprintln!("rustynes: cheat {} skipped: {e}", entry.code);
                }
            }
        }
        // v1.7.0 — prime the per-frame raw-cheat list from this ROM's enabled
        // entries so they apply from frame 1 (before the panel's first pull).
        emu.raw_cheats = loaded.raw.iter().filter(|c| c.enabled).cloned().collect();
        if let Some(debugger) = self.debugger.as_mut() {
            debugger.set_cheat_persist(dir.clone(), rom_sha256, loaded.genie, loaded.raw);
        }
    }

    /// Per-ROM FDS writable-disk save directory (`<data_dir>/fds-saves/`).
    /// Returns `None` if no data dir is available.
    #[cfg(not(target_arch = "wasm32"))]
    fn fds_saves_dir(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("fds-saves"))
    }

    /// The on-disk path of the `.fds.sav` writable-disk file for a given disk
    /// SHA-256 (`<data_dir>/fds-saves/<hex>.fds.sav`). `None` if no data dir.
    #[cfg(not(target_arch = "wasm32"))]
    fn fds_save_path(&self, rom_sha256: &[u8; 32]) -> Option<PathBuf> {
        self.fds_saves_dir().map(|d| {
            d.join(format!(
                "{}.fds.sav",
                crate::save_state::hex_sha256(rom_sha256)
            ))
        })
    }

    /// Resolve the FDS BIOS (`disksys.rom`) bytes.
    ///
    /// Uses the configured [`crate::config::FdsConfig::bios_path`] when it
    /// points at a readable 8 KiB file. Otherwise prompts the user once via an
    /// `rfd` file dialog, validates the selection is exactly 8 KiB, and
    /// persists the chosen path to the config file. Returns `None` (with a
    /// clear status message) when the user cancels or the file is the wrong
    /// size, so the caller can abort the load without crashing. Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_fds_bios(&mut self) -> Option<Vec<u8>> {
        // 1) Try the configured path first.
        if let Some(path) = self.config.fds.bios_path.clone() {
            match std::fs::read(&path) {
                Ok(bytes) if bytes.len() == FDS_BIOS_SIZE => return Some(bytes),
                Ok(bytes) => eprintln!(
                    "rustynes: configured FDS BIOS {} is {} bytes (expected {FDS_BIOS_SIZE}); re-prompting",
                    path.display(),
                    bytes.len()
                ),
                Err(e) => eprintln!(
                    "rustynes: configured FDS BIOS {} unreadable ({e}); re-prompting",
                    path.display()
                ),
            }
        }

        // 2) Prompt for it.
        let Some(path) = rfd::FileDialog::new()
            .set_title("Select Famicom Disk System BIOS (disksys.rom)")
            .add_filter("FDS BIOS", &["rom", "bin"])
            .pick_file()
        else {
            eprintln!("rustynes: FDS BIOS selection cancelled; cannot load disk image");
            return None;
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("rustynes: failed to read FDS BIOS {}: {e}", path.display());
                return None;
            }
        };
        if bytes.len() != FDS_BIOS_SIZE {
            eprintln!(
                "rustynes: FDS BIOS {} is {} bytes; the disksys.rom BIOS must be exactly {FDS_BIOS_SIZE} bytes",
                path.display(),
                bytes.len()
            );
            return None;
        }
        // Persist the validated path so we never prompt again.
        self.config.fds.bios_path = Some(path.clone());
        if let Err(e) = self.config.save() {
            eprintln!("rustynes: could not persist FDS BIOS path: {e}");
        } else {
            eprintln!("rustynes: FDS BIOS path saved -> {}", path.display());
        }
        Some(bytes)
    }

    /// Construct an FDS `Nes` from `disk_bytes` (+ a resolved BIOS), preferring
    /// any persisted writable-disk `.fds.sav` so prior in-game writes carry
    /// over. On success, stores the ORIGINAL disk image's SHA-256 in
    /// [`Self::fds_disk_sha256`] so the `.fds.sav` stays keyed by the same hash
    /// even though the running `Nes` may have been reloaded from the saved
    /// bytes. Returns `None` (logging) if BIOS resolution is cancelled or the
    /// disk/BIOS fails to parse. Native-only (filesystem + rfd).
    #[cfg(not(target_arch = "wasm32"))]
    fn build_fds_nes(&mut self, disk_bytes: &[u8], sample_rate: u32) -> Option<Nes> {
        let bios = self.resolve_fds_bios()?;
        // Build from the ORIGINAL disk first so `rom_sha256()` reports the
        // canonical hash; that is the key under which the `.fds.sav` is stored.
        let nes = match Nes::from_disk_with_sample_rate(disk_bytes, &bios, sample_rate) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("rustynes: failed to load FDS disk image: {e}");
                return None;
            }
        };
        let original_sha = *nes.rom_sha256();
        self.emu.lock().fds_disk_sha256 = Some(original_sha);

        // If a writable-disk save exists, reload the `Nes` from the SAVED
        // (already-modified) `.fds` bytes so prior in-game writes persist. The
        // saved image is a full `.fds` container, so this is the simplest
        // correct restore. We keep `fds_disk_sha256` = the original hash so the
        // save keeps the same on-disk key.
        if let Some(saved) = self
            .fds_save_path(&original_sha)
            .and_then(|p| std::fs::read(&p).ok())
        {
            match Nes::from_disk_with_sample_rate(&saved, &bios, sample_rate) {
                Ok(n) => {
                    eprintln!("rustynes: restored FDS writable disk from save");
                    return Some(n);
                }
                Err(e) => {
                    eprintln!("rustynes: FDS save corrupt ({e}); falling back to the pristine disk");
                }
            }
        }
        Some(nes)
    }

    /// Flush the FDS writable disk (see [`crate::emu::EmuCore::flush_fds_save`]).
    #[cfg(not(target_arch = "wasm32"))]
    fn flush_fds_save(&self) {
        let data_dir = self.data_dir.clone();
        self.emu.lock().flush_fds_save(data_dir.as_deref());
    }

    /// Cycle the inserted FDS disk side: ejected -> side 0 -> side 1 -> ... ->
    /// wrap back to ejected. A no-op for non-FDS games. Flushes any pending
    /// writes to the `.fds.sav` (native) before swapping so they aren't lost.
    fn cycle_disk_side(&self) {
        // Flush before swapping so an in-progress write isn't lost across the
        // eject. Native-only (the wasm build has no `.fds.sav` filesystem).
        #[cfg(not(target_arch = "wasm32"))]
        self.flush_fds_save();
        let mut guard = self.emu.lock();
        let Some(nes) = guard.nes.as_mut() else {
            return;
        };
        let count = nes.disk_side_count();
        if count == 0 {
            return;
        }
        // None -> Some(0) -> Some(1) -> ... -> Some(count-1) -> None.
        let next = match nes.inserted_disk_side() {
            None => Some(0),
            Some(s) if s + 1 < count => Some(s + 1),
            Some(_) => None,
        };
        nes.set_disk_side(next);
        match next {
            Some(s) => eprintln!("rustynes: FDS disk -> Side {}/{count}", s + 1),
            None => eprintln!("rustynes: FDS disk ejected"),
        }
    }

    /// Drain any pending gilrs events into the input state. Called once
    /// per pacer iteration. Cheap when no pad is connected — just a hash
    /// lookup of the connected-devices list. Native-only (gilrs);
    /// wasm32 uses browser gamepad/keyboard events.
    #[cfg(not(target_arch = "wasm32"))]
    fn pump_gamepad(&mut self) {
        let Some(gilrs) = self.gamepad.as_mut() else {
            return;
        };
        while let Some(ev) = gilrs.next_event() {
            self.input.handle_gamepad_event(ev.id, &ev.event);
            // If the input rebind modal is listening for a pad button,
            // feed the event there so it can capture the binding.
            if let Some(debugger) = self.debugger.as_mut() {
                debugger.maybe_capture_gamepad(&ev.event);
            }
        }
    }

    /// Save state to a filesystem slot. Native-only; wasm32 uses the
    /// `localStorage` path in `wasm.rs` (F1).
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_save_state(&self) {
        // Snapshot under a short lock; the file write runs with it dropped.
        let snapshot = {
            let guard = self.emu.lock();
            guard
                .nes
                .as_ref()
                .map(|nes| (*nes.rom_sha256(), nes.snapshot()))
        };
        let Some((rom_sha256, blob)) = snapshot else {
            return;
        };
        let Some(dir) = self.data_dir.as_ref() else {
            eprintln!("rustynes: no data directory available; save state skipped");
            return;
        };
        match save_state::save_to_slot(dir, &rom_sha256, 0, &blob) {
            Ok(path) => eprintln!("rustynes: saved state -> {}", path.display()),
            Err(e) => eprintln!("rustynes: save state failed: {e}"),
        }
    }

    /// Load state from a filesystem slot. Native-only (see
    /// [`Self::handle_save_state`]).
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_load_state(&self) {
        // Read the ROM key under a short lock; the file read runs with it
        // dropped; the restore takes a second short lock.
        let Some(rom_sha256) = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256()) else {
            return;
        };
        let Some(dir) = self.data_dir.as_ref() else {
            eprintln!("rustynes: no data directory available; load state skipped");
            return;
        };
        match save_state::load_from_slot(dir, &rom_sha256, 0) {
            Ok(blob) => {
                let mut guard = self.emu.lock();
                let Some(nes) = guard.nes.as_mut() else {
                    return;
                };
                match nes.restore(&blob) {
                    Ok(()) => eprintln!("rustynes: loaded state from slot 0"),
                    Err(e) => eprintln!("rustynes: restore failed: {e}"),
                }
            }
            Err(e) => eprintln!("rustynes: load state failed: {e}"),
        }
    }

    /// Per-ROM movies directory (`<data_dir>/movies/`). Created lazily on
    /// first save. Returns `None` if no data dir is available.
    #[cfg(not(target_arch = "wasm32"))]
    fn movies_dir(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("movies"))
    }

    /// `F6` — toggle TAS movie recording (native).
    ///
    /// **Start**: power-cycle the running `Nes` and begin recording from
    /// that fresh power-on (the most portable start point). **Stop**:
    /// finish the movie, serialize it, and prompt for a `.rnm` save path
    /// via the rfd dialog. No-op if no ROM is loaded.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_movie_record_toggle(&self) {
        if self.emu.lock().movie.is_recording() {
            // Finish under a short lock; the (blocking) rfd save dialog runs
            // with the guard dropped.
            let finished = self.emu.lock().movie.finish_recording();
            let Some(movie) = finished else {
                return;
            };
            self.movie_save_dialog(&movie);
        } else {
            // v2.3.0 — movies and netplay are mutually exclusive.
            if self.netplay.is_active() {
                eprintln!("rustynes: leave netplay before recording a movie");
                return;
            }
            let mut guard = self.emu.lock();
            let emu = &mut *guard;
            let Some(nes) = emu.nes.as_mut() else {
                eprintln!("rustynes: movie record: no ROM loaded");
                return;
            };
            emu.movie.start_recording_power_on(nes);
            // Reset frame pacing so the power-cycle's first frame is due now.
            emu.next_frame_time = Some(Instant::now());
            eprintln!("rustynes: movie recording started (power-on)");
        }
    }

    /// `F7` — toggle TAS movie playback (native).
    ///
    /// **Start**: open a `.rnm` file via the rfd dialog, deserialize it,
    /// seek the running `Nes` to the movie's start point, and begin
    /// playback (the movie's input overrides live input). **Stop**: end
    /// playback and return control to live input.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_movie_play_toggle(&self) {
        if self.emu.lock().movie.is_playing() {
            self.emu.lock().movie.stop_playback();
            eprintln!("rustynes: movie playback stopped");
            return;
        }
        // v2.3.0 — movies and netplay are mutually exclusive.
        if self.netplay.is_active() {
            eprintln!("rustynes: leave netplay before playing a movie");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("RustyNES movie", &["rnm"])
            .set_directory(self.movies_dir().unwrap_or_else(|| PathBuf::from(".")))
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("rustynes: movie open failed {}: {e}", path.display());
                return;
            }
        };
        let movie = match rustynes_core::Movie::deserialize(&bytes) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("rustynes: movie parse failed {}: {e}", path.display());
                return;
            }
        };
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let Some(nes) = emu.nes.as_mut() else {
            eprintln!("rustynes: movie play: no ROM loaded");
            return;
        };
        if let Err(e) = movie.seek_to_start(nes) {
            eprintln!("rustynes: movie seek failed (wrong ROM?): {e}");
            return;
        }
        let total = movie.len();
        emu.movie.start_playback(movie);
        // The seek (power-cycle or restore) reset emulator state; restart
        // the frame clock so the first replayed frame is due now.
        emu.next_frame_time = Some(Instant::now());
        eprintln!(
            "rustynes: movie playback started ({total} frames) from {}",
            path.display()
        );
    }

    /// `F8` — branch the current state into a new recording (native).
    ///
    /// Stops any in-progress playback and begins recording a new movie
    /// from `nes`'s current state (an embedded save-state start point), so
    /// the user can diverge from a replayed run and record their own
    /// continuation. No-op if no ROM is loaded.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_movie_branch(&self) {
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let Some(nes) = emu.nes.as_ref() else {
            eprintln!("rustynes: movie branch: no ROM loaded");
            return;
        };
        emu.movie.start_recording_branch(nes);
        eprintln!("rustynes: movie branch — recording from current state");
    }

    /// Serialize + write `movie` to a `.rnm` file chosen via the rfd save
    /// dialog (native). Defaults the directory to `<data_dir>/movies/`.
    #[cfg(not(target_arch = "wasm32"))]
    fn movie_save_dialog(&self, movie: &rustynes_core::Movie) {
        let dir = self.movies_dir();
        if let Some(d) = dir.as_ref() {
            // Best-effort: create the movies dir so the dialog opens there.
            let _ = std::fs::create_dir_all(d);
        }
        let mut dialog = rfd::FileDialog::new()
            .add_filter("RustyNES movie", &["rnm"])
            .set_file_name("movie.rnm");
        if let Some(d) = dir {
            dialog = dialog.set_directory(d);
        }
        let Some(path) = dialog.save_file() else {
            eprintln!("rustynes: movie save cancelled; recording discarded");
            return;
        };
        let bytes = movie.serialize();
        match std::fs::write(&path, &bytes) {
            Ok(()) => eprintln!(
                "rustynes: movie saved ({} frames, {} bytes) -> {}",
                movie.len(),
                bytes.len(),
                path.display()
            ),
            Err(e) => eprintln!("rustynes: movie save failed {}: {e}", path.display()),
        }
    }

    /// v1.6.0 Sprint 4 — F1 save state to `localStorage` (wasm32).
    ///
    /// The browser counterpart of [`Self::handle_save_state`]: serialize the
    /// running `Nes` and stash it in `localStorage` keyed by the ROM SHA-256
    /// + slot 0 (base64-encoded). No-op if no ROM is loaded.
    #[cfg(target_arch = "wasm32")]
    fn handle_save_state_wasm(&self) {
        let guard = self.emu.lock();
        let Some(nes) = guard.nes.as_ref() else {
            crate::wasm_io::log("save state: no ROM loaded");
            return;
        };
        let blob = nes.snapshot();
        crate::wasm_io::localstorage_save_state(nes.rom_sha256(), 0, &blob);
    }

    /// v1.6.0 Sprint 4 — F4 load state from `localStorage` (wasm32).
    ///
    /// The browser counterpart of [`Self::handle_load_state`]: read the
    /// per-ROM slot 0 blob back from `localStorage` and restore the `Nes`.
    #[cfg(target_arch = "wasm32")]
    fn handle_load_state_wasm(&self) {
        let mut guard = self.emu.lock();
        let Some(nes) = guard.nes.as_mut() else {
            crate::wasm_io::log("load state: no ROM loaded");
            return;
        };
        let Some(blob) = crate::wasm_io::localstorage_load_state(nes.rom_sha256(), 0) else {
            return;
        };
        match nes.restore(&blob) {
            Ok(()) => crate::wasm_io::log("state loaded"),
            Err(e) => crate::wasm_io::log(&format!("load state: restore failed: {e:?}")),
        }
    }

    /// v1.6.0 Sprint 4 — F6 toggle TAS movie recording (wasm32).
    ///
    /// The browser counterpart of [`Self::handle_movie_record_toggle`].
    /// **Start**: power-cycle the running `Nes` and record from that fresh
    /// power-on. **Stop**: finish the movie, serialize it, and trigger a
    /// browser download of the `.rnm` bytes (the `rfd` save dialog has no
    /// web equivalent). No-op if no ROM is loaded.
    #[cfg(target_arch = "wasm32")]
    fn handle_movie_record_toggle_wasm(&self) {
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        if emu.movie.is_recording() {
            let Some(movie) = emu.movie.finish_recording() else {
                return;
            };
            let bytes = movie.serialize();
            crate::wasm_io::download_bytes("rustynes-movie.rnm", &bytes);
            crate::wasm_io::log(&format!(
                "movie finished ({} frames, {} bytes) — download triggered",
                movie.len(),
                bytes.len()
            ));
        } else {
            let Some(nes) = emu.nes.as_mut() else {
                crate::wasm_io::log("movie record: no ROM loaded");
                return;
            };
            emu.movie.start_recording_power_on(nes);
            emu.next_frame_time = Some(Instant::now());
            crate::wasm_io::log("movie recording started (power-on)");
        }
    }

    /// v1.6.0 Sprint 4 — F7 toggle TAS movie playback (wasm32).
    ///
    /// The browser counterpart of [`Self::handle_movie_play_toggle`].
    /// **Stop**: end playback and return to live input. **Start**: open the
    /// hidden `.rnm` file picker (wired in `wasm_winit`); when the bytes
    /// arrive as [`AppEvent::MovieLoaded`] they're deserialized + played by
    /// [`Self::start_movie_from_bytes`]. The `.click()` runs inside this
    /// hotkey handler (a user gesture), satisfying the browser file-picker
    /// policy.
    #[cfg(target_arch = "wasm32")]
    fn handle_movie_play_toggle_wasm(&self) {
        let mut guard = self.emu.lock();
        if guard.movie.is_playing() {
            guard.movie.stop_playback();
            crate::wasm_io::log("movie playback stopped");
            return;
        }
        drop(guard);
        crate::wasm_io::click_file_input("rnm-input");
    }

    /// v1.6.0 Sprint 4 — F8 branch the current state into a new recording
    /// (wasm32). The browser counterpart of [`Self::handle_movie_branch`].
    #[cfg(target_arch = "wasm32")]
    fn handle_movie_branch_wasm(&self) {
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let Some(nes) = emu.nes.as_ref() else {
            crate::wasm_io::log("movie branch: no ROM loaded");
            return;
        };
        emu.movie.start_recording_branch(nes);
        crate::wasm_io::log("movie branch — recording from current state");
    }

    /// v1.6.0 Sprint 4 — deserialize uploaded `.rnm` bytes, seek the running
    /// `Nes` to the movie's start point, and begin playback (wasm32). Driven
    /// by [`AppEvent::MovieLoaded`] from the browser file picker.
    #[cfg(target_arch = "wasm32")]
    fn start_movie_from_bytes(&self, bytes: &[u8]) {
        let movie = match rustynes_core::Movie::deserialize(bytes) {
            Ok(m) => m,
            Err(e) => {
                crate::wasm_io::log(&format!("movie parse failed: {e:?}"));
                return;
            }
        };
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let Some(nes) = emu.nes.as_mut() else {
            crate::wasm_io::log("movie play: no ROM loaded");
            return;
        };
        if let Err(e) = movie.seek_to_start(nes) {
            crate::wasm_io::log(&format!("movie seek failed (wrong ROM?): {e:?}"));
            return;
        }
        let total = movie.len();
        emu.movie.start_playback(movie);
        // The seek (power-cycle or restore) reset emulator state; restart the
        // frame clock so the first replayed frame is due now.
        emu.next_frame_time = Some(Instant::now());
        crate::wasm_io::log(&format!("movie playback started ({total} frames)"));
    }

    /// v2.2.0 — accept uploaded FDS BIOS (`disksys.rom`) bytes (wasm32).
    ///
    /// Validates the upload is exactly 8 KiB and stashes it for the FDS disk
    /// load path. Driven by [`AppEvent::FdsBiosLoaded`] from the browser
    /// BIOS-upload `<input>`. If a `.fds` disk is already pending (the user
    /// picked the disk before the BIOS), this kicks off the deferred load.
    #[cfg(target_arch = "wasm32")]
    fn set_fds_bios_wasm(&mut self, bytes: Vec<u8>, event_loop: &ActiveEventLoop) {
        if bytes.len() != FDS_BIOS_SIZE {
            crate::wasm_io::log(&format!(
                "FDS BIOS upload is {} bytes; disksys.rom must be exactly {FDS_BIOS_SIZE} bytes",
                bytes.len()
            ));
            return;
        }
        crate::wasm_io::log("FDS BIOS accepted (8192 bytes)");
        self.fds_bios_bytes = Some(bytes);
        // If a `.fds` disk is already loaded in `rom_bytes` but no Nes was
        // built (BIOS was missing), build it now.
        if self.emu.lock().nes.is_none()
            && !self.rom_bytes.is_empty()
            && is_fds_image(&self.rom_bytes)
        {
            self.start_nes(
                crate::wasm_audio::sample_rate().unwrap_or(44_100),
                event_loop,
            );
        }
    }

    /// v2.2.0 — build an FDS `Nes` from `self.rom_bytes` (the uploaded disk)
    /// plus the uploaded BIOS (wasm32). There is no writable-disk `.fds.sav`
    /// on wasm (no filesystem). Returns `None` (logging a hint) when the BIOS
    /// has not been uploaded yet, so the caller keeps waiting.
    #[cfg(target_arch = "wasm32")]
    fn build_fds_nes_wasm(&self, sample_rate: u32) -> Option<Nes> {
        let Some(bios) = self.fds_bios_bytes.clone() else {
            crate::wasm_io::log(
                "FDS disk loaded, but no BIOS yet — upload disksys.rom via the FDS BIOS button",
            );
            return None;
        };
        match Nes::from_disk_with_sample_rate(&self.rom_bytes, &bios, sample_rate) {
            Ok(n) => Some(n),
            Err(e) => {
                crate::wasm_io::log(&format!("failed to load FDS disk image: {e:?}"));
                None
            }
        }
    }

    /// Push the latest input state into the running emulator (controllers +
    /// expansion devices) — see [`crate::emu::EmuCore::latch`].
    fn latch_input(&self) {
        let inputs = self.frame_inputs();
        self.emu.lock().latch(&inputs);
    }

    /// v2.1.0 — (re)attach the configured non-standard device on the player-2
    /// port, or detach it (returning to the standard controller). Called after
    /// a ROM loads and whenever the device selection changes.
    #[cfg(not(target_arch = "wasm32"))]
    fn sync_expansion_device(&self) {
        use crate::config::ExpansionDevice;
        let device = self.config.input.expansion_device;
        let mut guard = self.emu.lock();
        if let Some(nes) = guard.nes.as_mut() {
            match device {
                ExpansionDevice::None => nes.set_expansion_device(1, None),
                ExpansionDevice::Zapper => {
                    nes.set_zapper(1, u16::MAX, u16::MAX, false);
                }
                ExpansionDevice::Vaus => {
                    nes.set_paddle(1, 0x80, false);
                }
            }
        }
    }

    /// Advance the emulator by exactly one frame. Netplay (native UDP /
    /// browser WebRTC) is routed here — an active session OWNS frame
    /// advancement; otherwise the single-player produce lives in
    /// [`crate::emu::EmuCore::produce_one_frame`].
    fn produce_one_frame(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        if self.netplay.is_active() {
            self.produce_one_frame_netplay();
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if self
            .browser_netplay
            .as_ref()
            .is_some_and(crate::wasm_netplay::BrowserNetplay::is_active)
        {
            self.produce_one_frame_browser_netplay();
            return;
        }
        let inputs = self.frame_inputs();
        // Scope the sink borrows of `self.audio` / `self.ra` so they end
        // before `apply_produce_fx` re-borrows `self`.
        let fx = {
            #[cfg(not(target_arch = "wasm32"))]
            let mut sinks = Self::sync_sinks(
                &mut self.audio,
                #[cfg(feature = "retroachievements")]
                &mut self.ra,
            );
            #[cfg(target_arch = "wasm32")]
            let mut sinks = crate::emu::FrameSinks {
                _marker: core::marker::PhantomData,
            };
            self.emu.lock().produce_one_frame(&inputs, &mut sinks)
        };
        self.apply_produce_fx(fx);
    }

    /// v2.8.0 Phase 5 — build the synchronous (winit-thread) drive's
    /// [`crate::emu::FrameSinks`] from disjoint field borrows: the live
    /// `!Send` [`AudioOutput`] coerced to `&mut dyn AudioSink`, plus the RA
    /// session. Taking the fields directly (not `&mut self`) keeps the two
    /// borrows disjoint. Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    // The shared `'a` ties two params on the RA build; on the default build
    // only `audio` remains, so clippy sees an elidable single lifetime.
    #[cfg_attr(not(feature = "retroachievements"), allow(clippy::needless_lifetimes))]
    fn sync_sinks<'a>(
        audio: &'a mut Option<AudioOutput>,
        #[cfg(feature = "retroachievements")] ra: &'a mut Option<crate::ra_session::RaSession>,
    ) -> crate::emu::FrameSinks<'a> {
        crate::emu::FrameSinks {
            audio: audio
                .as_mut()
                .map(|a| a as &mut dyn crate::audio::AudioSink),
            #[cfg(feature = "retroachievements")]
            ra: ra.as_mut(),
        }
    }

    /// Build the per-pace input snapshot for the emulation core from the
    /// winit-thread-resident input state (keyboard maps, gilrs, mouse).
    fn frame_inputs(&self) -> crate::emu::FrameInputs {
        let hardcore_blocked = self.ra_hardcore_blocks();
        crate::emu::FrameInputs {
            buttons: [
                self.input.player1(),
                self.input.player2(),
                self.input.player3(),
                self.input.player4(),
            ],
            four_score: self.config.input.four_score,
            // v2.7.0 — RA hardcore disables rewind; fold the gate here.
            rewind_held: self.input.rewind_held() && !hardcore_blocked,
            hardcore_blocked,
            run_ahead: self.config.input.run_ahead,
            #[cfg(not(target_arch = "wasm32"))]
            expansion: self.config.input.expansion_device,
            // Map the cursor (physical window px) to the 256x240 NES screen,
            // assuming the framebuffer fills the window (letterbox bars read
            // as off-screen — the correct Zapper "no light" behavior).
            #[cfg(not(target_arch = "wasm32"))]
            mouse_nes: self.cursor_pos.map_or((u16::MAX, u16::MAX), |(cx, cy)| {
                let (ww, wh) = self.window_size;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let x = ((cx / f64::from(ww.max(1))) * 256.0) as i64;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let y = ((cy / f64::from(wh.max(1))) * 240.0) as i64;
                (
                    u16::try_from(x).unwrap_or(u16::MAX),
                    u16::try_from(y).unwrap_or(u16::MAX),
                )
            }),
            #[cfg(not(target_arch = "wasm32"))]
            mouse_pressed: self.mouse_pressed,
        }
    }

    /// v2.8.0 Phase 5 increment 3 — `true` when the dedicated emulation
    /// thread owns single-player frame production (spawned + a ROM loaded +
    /// netplay inactive). When it does, the winit thread's synchronous
    /// produce paths (`pace_frames` single-player branch,
    /// `display_sync_produce`) stand down and only present; netplay always
    /// runs synchronously (the thread is paused). Always `false` when the
    /// `emu-thread` feature is off (the synchronous Phases 0-4 drive).
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    const fn emu_thread_drives(&self) -> bool {
        self.emu_thread.is_some() && !self.netplay.is_active()
    }

    /// `emu_thread_drives` stub for the native build with the `emu-thread`
    /// feature OFF (the synchronous A/B path); production is always
    /// synchronous. wasm has no caller (its produce paths are all
    /// `not(target_arch = "wasm32")`-excluded), so no stub is needed there.
    #[cfg(all(not(target_arch = "wasm32"), not(feature = "emu-thread")))]
    #[allow(clippy::unused_self)]
    const fn emu_thread_drives(&self) -> bool {
        false
    }

    /// v2.8.0 Phase 5 increment 3 — publish the latest input snapshot into
    /// the emulation thread's lock-free [`crate::emu_thread::SharedInput`]
    /// so the next produced frame latches it. No-op when the thread isn't
    /// spawned. Native-only + `emu-thread`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    fn publish_shared_input(&self) {
        if let Some(thread) = self.emu_thread.as_ref() {
            thread.shared_input().publish(&self.frame_inputs());
        }
    }

    /// v2.8.0 Phase 5 increment 3 — publish the active pacing regime + the
    /// current per-region frame duration to the emulation thread's control
    /// block. Called from `resolve_pacing` and the ROM-reload path. No-op
    /// when the thread isn't spawned. Native-only + `emu-thread`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    fn publish_emu_thread_regime(&self) {
        if let Some(thread) = self.emu_thread.as_ref() {
            let regime = match self.active_pacing {
                ActivePacing::Wallclock => crate::emu_thread::regime::WALLCLOCK,
                ActivePacing::Display => crate::emu_thread::regime::DISPLAY,
                ActivePacing::Vrr => crate::emu_thread::regime::VRR,
            };
            thread
                .control()
                .set_regime(regime, self.emu.lock().frame_duration);
        }
    }

    /// v2.8.0 Phase 5 increment 3 — spawn the dedicated emulation thread once
    /// `Gfx` + audio are ready. The thread is given the shared
    /// [`crate::emu::EmuHandle`], a `Send` [`crate::audio::AudioProducer`]
    /// made from the cpal output (the stream + consumer callback stay here),
    /// and the proxy for [`AppEvent::EmuFrame`]. It idles until a ROM loads
    /// (`finish_start_nes` flips `set_has_rom`). No-op if already spawned or
    /// the proxy is missing.
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    fn spawn_emu_thread(&mut self) {
        if self.emu_thread.is_some() {
            return;
        }
        let Some(proxy) = self.emu_proxy.clone() else {
            eprintln!("rustynes: emu thread not spawned (no event-loop proxy)");
            return;
        };
        let producer = self
            .audio
            .as_ref()
            .map(|a| a.make_producer(self.config.audio.drc));
        let control = std::sync::Arc::new(crate::emu_thread::EmuControl::new());
        let shared_input = std::sync::Arc::new(crate::emu_thread::SharedInput::default());
        self.emu_thread = Some(crate::emu_thread::EmuThread::spawn(
            self.emu.clone(),
            producer,
            proxy,
            control,
            shared_input,
        ));
    }

    /// v2.8.0 Phase 5 increment 3 — handle [`AppEvent::EmuFrame`]: the emu
    /// thread just produced a frame, so the winit thread does the UI-side
    /// housekeeping (perf/HUD/FDS/perf-log pushes), drives RA (it stays on
    /// this thread), republishes input for the next frame, and requests a
    /// redraw. Native-only + `emu-thread`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    fn on_emu_frame(&mut self) {
        // RA stays on the winit thread (`rc_client` is single-threaded): the
        // emu thread produced with `ra: None`, so drive it here against the
        // freshly produced (between-frames) core state — mirroring the
        // synchronous path's in-produce RA drive + `apply_produce_fx`.
        #[cfg(feature = "retroachievements")]
        {
            let (status, just_logged_in) = {
                let mut guard = self.emu.lock();
                self.ra.as_mut().map_or((None, false), |ra| {
                    let s = crate::emu::drive_ra(guard.nes.as_mut(), ra, true);
                    let jl = crate::ra_session::RaSession::take_just_logged_in(ra);
                    (Some(s), jl)
                })
            };
            if let Some(status) = status {
                if let Some(debugger) = self.debugger.as_mut() {
                    debugger.set_cheevos_status(status);
                }
            }
            if just_logged_in {
                self.persist_ra_token_if_new();
                if self.emu.lock().nes.is_some() {
                    self.load_ra_game();
                }
            }
        }
        self.post_produce_housekeeping();
        self.pump_gamepad();
        self.publish_shared_input();
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// Act on the UI-thread side effects a core produce surfaced (RA status
    /// push + login-edge token persistence / game identify).
    // On the default build (RA feature off) the body is empty, so self/fx
    // are unused and the fn is const-able — all artifacts of the cfg.
    #[allow(
        clippy::needless_pass_by_value,
        clippy::needless_pass_by_ref_mut,
        clippy::unused_self,
        clippy::missing_const_for_fn,
        unused_variables
    )]
    fn apply_produce_fx(&mut self, fx: crate::emu::ProduceFx) {
        #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
        {
            if let Some(status) = fx.ra_status {
                if let Some(debugger) = self.debugger.as_mut() {
                    debugger.set_cheevos_status(status);
                }
            }
            // On the login-success edge: persist the freshly-issued token and
            // (re-)identify the currently-loaded ROM (a ROM opened BEFORE the
            // login could not be loaded into rc_client yet).
            if fx.ra_just_logged_in {
                self.persist_ra_token_if_new();
                if self.emu.lock().nes.is_some() {
                    self.load_ra_game();
                }
            }
        }
    }

    /// v2.7.0 — `true` when a `RetroAchievements` session is active AND in
    /// hardcore mode, so the "soft" affordances (save-state load, rewind,
    /// cheats, frame-advance, RAM-watch) must be refused. Always `false` when
    /// the `retroachievements` feature is off or no session is active, so every
    /// gated site is a no-op on the default build.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn ra_hardcore_blocks(&self) -> bool {
        self.ra
            .as_ref()
            .is_some_and(crate::ra_session::RaSession::hardcore_blocks)
    }

    /// v2.7.0 — the hardcore-gating predicate, always `false` when the
    /// `retroachievements` feature is not built (no RA session can exist), so
    /// the gated sites compile to plain no-ops on the default build.
    #[cfg(not(all(not(target_arch = "wasm32"), feature = "retroachievements")))]
    #[allow(clippy::unused_self)]
    const fn ra_hardcore_blocks(&self) -> bool {
        false
    }

    /// v2.7.0 — the per-ROM RA progress sidecar directory
    /// (`<data_dir>/ra-progress/`). `None` if no data dir is available.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn ra_progress_dir(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("ra-progress"))
    }

    /// v2.7.0 — the on-disk path of the RA progress sidecar for a given ROM
    /// SHA-256 (`<data_dir>/ra-progress/<hex>.rap`). `None` if no data dir.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn ra_progress_path(&self, rom_sha256: &[u8; 32]) -> Option<PathBuf> {
        self.ra_progress_dir()
            .map(|d| d.join(format!("{}.rap", crate::save_state::hex_sha256(rom_sha256))))
    }

    /// v2.7.0 — identify the freshly-loaded ROM with `RetroAchievements` and
    /// queue its saved progress sidecar (applied once the async load
    /// completes). No-op when no RA session is active. Native-only +
    /// feature-gated.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn load_ra_game(&mut self) {
        // Save the OUTGOING game's progress before switching ROMs.
        self.save_ra_progress();
        // Read the rom hash + sidecar path before borrowing `self.ra` mutably.
        let Some(rom_sha256) = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256()) else {
            return;
        };
        let sidecar = self
            .ra_progress_path(&rom_sha256)
            .and_then(|p| std::fs::read(p).ok());
        // The ROM bytes RA hashes are `self.rom_bytes` (set on every load).
        let rom = self.rom_bytes.clone();
        if let Some(ra) = self.ra.as_mut() {
            ra.begin_load_game(&rom, rom_sha256, sidecar);
        }
    }

    /// v2.7.0 — save the RA progress sidecar for the current game (on ROM
    /// close / exit). No-op when no RA session is active or no game loaded.
    /// Native-only + feature-gated.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn save_ra_progress(&mut self) {
        let Some(sha) = self
            .ra
            .as_ref()
            .and_then(crate::ra_session::RaSession::game_sha256)
        else {
            return;
        };
        let Some(path) = self.ra_progress_path(&sha) else {
            return;
        };
        let blob = self
            .ra
            .as_mut()
            .map(crate::ra_session::RaSession::serialize_progress);
        if let Some(blob) = blob {
            if blob.is_empty() {
                return;
            }
            if let Some(parent) = path.parent() {
                let _ = std::fs::create_dir_all(parent);
            }
            if let Err(e) = std::fs::write(&path, &blob) {
                eprintln!("rustynes: failed to save RA progress: {e}");
            }
        }
    }

    /// v2.7.0 — act on a cheevos panel request (login / logout / hardcore).
    /// Persists the returned login token / hardcore flag to config. Native-only
    /// + feature-gated.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn handle_cheevos_request(&mut self, req: crate::debugger::CheevosRequest) {
        use crate::debugger::CheevosRequest;
        let Some(ra) = self.ra.as_mut() else { return };
        match req {
            CheevosRequest::LoginPassword { username, password } => {
                ra.begin_login_password(&username, &password);
                // Persist the username now; the token is written after login
                // succeeds (polled in `persist_ra_token_if_new`).
                self.config.retroachievements.username = username;
                self.config.retroachievements.enabled = true;
                let _ = self.config.save();
            }
            CheevosRequest::Logout => {
                ra.logout();
                self.config.retroachievements.token.clear();
                self.config.retroachievements.enabled = false;
                let _ = self.config.save();
            }
            CheevosRequest::SetHardcore(on) => {
                ra.set_hardcore(on);
                self.config.retroachievements.hardcore = on;
                let _ = self.config.save();
            }
        }
    }

    /// v2.7.0 — if a login just completed, persist the returned token to config
    /// (so the next launch can token-login). Cheap: only writes when the token
    /// changed. Native-only + feature-gated.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn persist_ra_token_if_new(&mut self) {
        let Some(token) = self
            .ra
            .as_ref()
            .and_then(crate::ra_session::RaSession::user_token)
        else {
            return;
        };
        if !token.is_empty() && self.config.retroachievements.token != token {
            self.config.retroachievements.token = token;
            self.config.retroachievements.enabled = true;
            if let Some(name) = self
                .ra
                .as_ref()
                .and_then(crate::ra_session::RaSession::user_info)
                .map(|u| u.username)
            {
                self.config.retroachievements.username = name;
            }
            let _ = self.config.save();
        }
    }

    /// Reset the running emulator (and keep `RetroAchievements` in sync when the
    /// feature is active — Reset is always allowed, even in hardcore).
    // `&mut self` is only exercised by the RA build (`reset_ra`); the default
    // build mutates through the emu lock alone — a cfg artifact.
    #[allow(clippy::needless_pass_by_ref_mut)]
    fn do_reset(&mut self) {
        {
            let mut guard = self.emu.lock();
            if let Some(nes) = guard.nes.as_mut() {
                nes.reset();
            }
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
        self.reset_ra();
    }

    /// Power-cycle the running emulator (and keep `RetroAchievements` in sync).
    // Same cfg artifact as `do_reset`.
    #[allow(clippy::needless_pass_by_ref_mut)]
    fn do_power_cycle(&mut self) {
        {
            let mut guard = self.emu.lock();
            if let Some(nes) = guard.nes.as_mut() {
                nes.power_cycle();
            }
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
        self.reset_ra();
    }

    /// v2.7.0 — propagate an emulator reset/power-cycle into the RA session so
    /// achievement state is reset alongside. Disjoint borrow of `ra` + `nes`.
    /// Native-only + feature-gated.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn reset_ra(&mut self) {
        let Some(ra) = self.ra.as_mut() else { return };
        let mut guard = self.emu.lock();
        let Some(nes) = guard.nes.as_mut() else {
            return;
        };
        ra.reset(&mut |a| nes.cpu_bus_peek(a));
    }

    /// v2.7.0 — log a "blocked in hardcore mode" message (and, when an RA
    /// session is active, surface it as an on-screen toast). A plain no-op-ish
    /// helper available in both feature states so the gated call sites compile
    /// uniformly.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    #[allow(clippy::unused_self)]
    fn toast_hardcore(&self, msg: &str) {
        eprintln!("rustynes: {msg}");
    }

    /// v2.7.0 — `toast_hardcore` stub for builds without the RA feature; it is
    /// never reached (`ra_hardcore_blocks()` is `const false` there), so it
    /// just keeps the call site compiling.
    #[cfg(not(all(not(target_arch = "wasm32"), feature = "retroachievements")))]
    #[allow(clippy::unused_self)]
    const fn toast_hardcore(&self, _msg: &str) {}

    /// v2.3.0 — the netplay produce path, used in place of the single-player
    /// `produce_one_frame` body while a session is active. Feeds this peer's
    /// LOCAL input (`player1()` — the keyboard/gamepad, routed to the right
    /// NES port by the session's `local_player`) and ticks the rollback
    /// session, which advances the emulator (or stalls for time-sync). On a
    /// produced frame the APU samples are drained into the cpal queue exactly
    /// as the single-player path does; on a stall (or while connecting / on
    /// error) no frame is produced and no audio is pushed. Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    fn produce_one_frame_netplay(&mut self) {
        let local = self.input.player1();
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let Some(nes) = emu.nes.as_mut() else {
            return;
        };
        let tick = self.netplay.tick(nes, local);

        // Push the freshly produced frame's audio, mirroring the single-player
        // path. Only on an actual produced frame (a connecting / stalled / error
        // tick advances nothing, so there are no new samples to drain).
        if tick.produced_frame {
            if let Some(audio) = &mut self.audio {
                let target = ((u64::from(audio.sample_rate) / 50) as usize).max(1024);
                if emu.audio_buf.len() < target {
                    emu.audio_buf.resize(target, 0.0);
                }
                let n = nes.drain_audio_into(&mut emu.audio_buf);
                // v2.8.0 Phase 1 — through the DRC resampler stage.
                audio.push_samples(&emu.audio_buf[..n]);
            }
            // v2.8.0 Phase 3 — refresh the presented framebuffer.
            emu.present_fb.clear();
            emu.present_fb.extend_from_slice(nes.framebuffer());
        }

        // Surface the latest status into the debugger HUD + panel.
        let status = self.netplay.status();
        if let Some(debugger) = self.debugger.as_mut() {
            debugger.set_netplay_status(netplay_status_view(&status));
        }
    }

    /// v2.7.0 — the browser (WebRTC) netplay produce path, used in place of the
    /// single-player wasm frame body while a browser session is active. Feeds
    /// this peer's LOCAL input (`player1()` — keyboard/gamepad) and ticks the
    /// rollback session over the WebRTC data channel; the session advances the
    /// emulator (or stalls / connects). The freshly produced frame's audio is
    /// pushed into the Web Audio ring exactly as the single-player wasm path
    /// does. The lobby's status line is refreshed from the driver's phase +
    /// message. wasm-only.
    #[cfg(target_arch = "wasm32")]
    fn produce_one_frame_browser_netplay(&mut self) {
        let local = self.input.player1();
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let (Some(driver), Some(nes)) = (self.browser_netplay.as_mut(), emu.nes.as_mut()) else {
            return;
        };
        let consumed = driver.tick(nes, local);
        // On an actual produced frame, push this frame's APU samples into the
        // shared Web Audio ring (mirrors the single-player wasm path). A
        // connecting / stalled / error tick advances nothing.
        if consumed
            && matches!(
                driver.phase(),
                crate::wasm_netplay::BrowserNetplayPhase::InGame
            )
        {
            crate::wasm_audio::push_samples(&nes.drain_audio());
            // v2.8.0 Phase 3 — refresh the presented framebuffer.
            emu.present_fb.clear();
            emu.present_fb.extend_from_slice(nes.framebuffer());
        }
        // Surface the latest status into the lobby UI.
        let phase = driver.phase();
        let message = driver.message();
        self.wasm_lobby.set_status(phase, message);
    }

    /// v2.7.0 — act on a browser-netplay lobby request (connect / leave). A
    /// Connect is rejected (logged) when no ROM is loaded (the session needs the
    /// ROM hash for the handshake). wasm-only.
    #[cfg(target_arch = "wasm32")]
    fn handle_lobby_request(&mut self, req: crate::wasm_lobby::LobbyRequest) {
        use crate::wasm_lobby::LobbyRequest;
        match req {
            LobbyRequest::Leave => {
                if let Some(driver) = self.browser_netplay.as_mut() {
                    driver.leave();
                }
                self.wasm_lobby.set_status(
                    crate::wasm_netplay::BrowserNetplayPhase::Idle,
                    String::new(),
                );
            }
            LobbyRequest::Connect {
                signaling_url,
                room,
                host: _,
                num_players,
            } => {
                let Some(rom_hash) = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256()) else {
                    crate::wasm_io::log("rustynes: browser netplay needs a loaded ROM first");
                    return;
                };
                let mut driver = crate::wasm_netplay::BrowserNetplay::new(rom_hash);
                driver.set_num_players(num_players);
                let ice = self.config.netplay.stun_servers.clone();
                match driver.connect(&signaling_url, &room, &ice) {
                    Ok(()) => {
                        self.browser_netplay = Some(driver);
                    }
                    Err(e) => {
                        self.wasm_lobby.set_status(
                            crate::wasm_netplay::BrowserNetplayPhase::Error,
                            format!("connect failed: {e:?}"),
                        );
                    }
                }
            }
        }
    }

    /// v2.3.0 — act on a netplay panel request (host / join / leave).
    ///
    /// Host = player 0 (P1); joiner = player 1 (P2). A host/join is rejected
    /// (logged) when no ROM is loaded, when a TAS movie is recording/playing
    /// (netplay is mutually exclusive with movies), or when the peer address
    /// fails to parse. Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_netplay_request(&mut self, req: crate::debugger::NetplayRequest) {
        use crate::debugger::NetplayRequest;
        match req {
            NetplayRequest::Leave => {
                self.netplay.leave();
                // v2.8.0 Phase 5 increment 3 — netplay released: resume the
                // emulation thread (single-player produce returns to it). The
                // thread also re-bases its pacer from `next_frame_time`.
                #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
                if let Some(thread) = self.emu_thread.as_ref() {
                    self.emu.lock().next_frame_time = Some(Instant::now());
                    thread.control().set_netplay_paused(false);
                }
                // Clear the HUD back to Idle.
                if let Some(debugger) = self.debugger.as_mut() {
                    debugger.set_netplay_status(netplay_status_view(&self.netplay.status()));
                }
            }
            NetplayRequest::Host { .. } | NetplayRequest::Join { .. }
                if self.emu.lock().nes.is_none() =>
            {
                eprintln!("rustynes: netplay needs a loaded ROM first");
            }
            NetplayRequest::Host { .. } | NetplayRequest::Join { .. }
                if {
                    let emu = self.emu.lock();
                    emu.movie.is_recording() || emu.movie.is_playing()
                } =>
            {
                eprintln!(
                    "rustynes: netplay is mutually exclusive with TAS movie \
                     record/playback — stop the movie first"
                );
            }
            NetplayRequest::Host { port, num_players } => {
                let Some(rom_hash) = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256()) else {
                    return;
                };
                // v2.8.0 Phase 5 increment 3 — pause the emulation thread
                // BEFORE the session takes over: netplay drives the core
                // synchronously on the winit thread; the two must never both
                // produce.
                #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
                self.pause_emu_thread_for_netplay();
                // Host "listen" mode: bind the local port and learn the joiner's
                // address from its first Sync — no remote to pre-enter or parse.
                self.netplay.start_host(port, num_players, rom_hash);
            }
            NetplayRequest::Join { remote } => {
                let Some(rom_hash) = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256()) else {
                    return;
                };
                match remote.parse::<std::net::SocketAddr>() {
                    Ok(addr) => {
                        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
                        self.pause_emu_thread_for_netplay();
                        self.netplay.start_join(addr, rom_hash);
                    }
                    Err(e) => eprintln!("rustynes: bad host address {remote:?}: {e}"),
                }
            }
        }
    }

    /// v2.8.0 Phase 5 increment 3 — pause the emulation thread for a netplay
    /// session and confirm it has parked, so a stray single-player frame can
    /// never advance the core out from under the rollback session. The
    /// confirmation is a brief lock acquisition: once we hold the emu lock
    /// after setting the pause flag, the thread is either parked (it checks
    /// the flag before locking) or blocked on this lock and will park on its
    /// next iteration. Native-only + `emu-thread`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    fn pause_emu_thread_for_netplay(&self) {
        if let Some(thread) = self.emu_thread.as_ref() {
            thread.control().set_netplay_paused(true);
            // Acquire+release the lock to fence against an in-flight produce:
            // after this returns, any further thread produce sees the flag
            // and parks before touching the core.
            drop(self.emu.lock());
        }
    }

    /// Native hybrid sleep-then-spin wait to a precise `target` `Instant`.
    ///
    /// Sleeps until [`SPIN_MARGIN`] before `target` (releasing the core),
    /// then busy-spins to the exact instant. This blocks the event-loop
    /// thread on purpose — paired with `ControlFlow::Poll`, frame
    /// production therefore lands on a precise cadence free of the
    /// `WaitUntil`/`calloop`-poll wake jitter. wasm32 must never call this
    /// (no real `thread::sleep`/spin in the browser); see `pace_frames`.
    #[cfg(not(target_arch = "wasm32"))]
    fn block_until_native(target: Instant) {
        loop {
            let now = Instant::now();
            if now >= target {
                return;
            }
            let remaining = target - now;
            if remaining > SPIN_MARGIN {
                // Sleep in `SLEEP_CHUNK`-capped naps, not one big
                // `sleep(remaining - SPIN_MARGIN)`. A single OS oversleep
                // (common on a loaded/un-tuned host) could otherwise
                // overshoot `target` entirely, so the precise spin below
                // would never run and the frame would land late — the
                // residual stutter. Capping + re-measuring keeps the wait
                // converging on `target`; the spin still owns the final
                // `SPIN_MARGIN`.
                // `remaining > SPIN_MARGIN` is guaranteed by the branch, so
                // this never saturates to zero — `saturating_sub` just keeps
                // the subtraction panic-free (clippy::unchecked_time_subtraction).
                let nap = remaining.saturating_sub(SPIN_MARGIN).min(SLEEP_CHUNK);
                std::thread::sleep(nap);
            } else {
                std::hint::spin_loop();
            }
        }
    }

    /// Wall-clock pacer. Called from `about_to_wait`.
    ///
    /// **Native** blocks to the exact `next_frame_time` (hybrid
    /// sleep+spin) and then produces the frame here, so the production
    /// cadence is precise; it stays on `ControlFlow::Poll` so
    /// `about_to_wait` re-runs immediately and the spin owns the cadence.
    /// If at least one `frame_duration` has elapsed it produces enough
    /// frames to catch up (capped at `MAX_CATCHUP_FRAMES`).
    ///
    /// **wasm32** does NOT produce frames here. In winit 0.30 the browser
    /// backend services `ControlFlow::Poll`/`WaitUntil` via
    /// `Scheduler.postTask`/`setTimeout` — neither is synced to the
    /// display's vsync, so pacing production off them jitters (the Pages
    /// stutter). The only vsync-synced (`requestAnimationFrame`) signal
    /// winit exposes is `Window::request_redraw()` → `RedrawRequested`.
    /// So on wasm32 the frame loop is driven from `RedrawRequested`
    /// (see [`Self::pace_and_produce_wasm`]); `pace_frames` only idles the
    /// event loop on `ControlFlow::Wait` and lets that rAF loop run.
    /// `Wait` (not `Poll`) is load-bearing: `Poll` busy-loops on the web
    /// backend in parallel with rAF, starving the emulation — the v1.3.2
    /// regression. Input is latched in `pace_and_produce_wasm`, just
    /// before production.
    // On wasm32 the body only reads `self` (the rAF-driven
    // `RedrawRequested` handler owns mutation); `&mut self` is required by
    // the native body + the `about_to_wait` caller, so allow the lint there.
    #[cfg_attr(target_arch = "wasm32", allow(clippy::needless_pass_by_ref_mut))]
    fn pace_frames(&mut self, event_loop: &ActiveEventLoop) {
        if self.emu.lock().nes.is_none() {
            return;
        }

        // wasm32: the rAF-driven `RedrawRequested` handler owns frame
        // production + input latching + presentation. Here we idle the
        // event loop with `ControlFlow::Wait` — NOT `Poll`. On winit's web
        // backend `Poll` reschedules the loop immediately via
        // `Scheduler.postTask`/`setTimeout(0)`, which runs a busy-loop in
        // PARALLEL with the rAF redraw loop: two schedulers competing for
        // the single wasm main thread, starving the (heavy) emulation —
        // the severe v1.3.2 stutter + periodic freezes. With `Wait` the
        // event loop sleeps until the next event; the ONLY heartbeat is
        // the `request_redraw()` self-reschedule inside
        // `pace_and_produce_wasm` (which winit wires to
        // `requestAnimationFrame`), so production is driven purely from
        // rAF with no competing busy-loop.
        // (No `return` needed: the native block below is cfg'd out on
        // wasm32, so this is the function tail.)
        #[cfg(target_arch = "wasm32")]
        event_loop.set_control_flow(ControlFlow::Wait);

        #[cfg(not(target_arch = "wasm32"))]
        {
            // v2.8.0 Phase 5 increment 3 — when the dedicated emulation
            // thread owns single-player production, the winit thread does NOT
            // produce here: it idles until the next event (an input, a
            // resize, or the thread's `EmuFrame` ping). `ControlFlow::Wait`
            // (not `Poll`) keeps this thread off the CPU so it never
            // contends with the producing thread.
            if self.emu_thread_drives() {
                event_loop.set_control_flow(ControlFlow::Wait);
                return;
            }

            // v2.8.0 Phase 2 — display-sync regime: frame production is
            // driven from `RedrawRequested` (one emulated frame per display
            // refresh, paced by Fifo backpressure). Here `about_to_wait`
            // only runs the OCCLUSION WATCHDOG: when redraws stop arriving
            // (minimized / fully occluded window on a frame-callback-
            // throttled compositor), produce due frames wall-clock so
            // emulation + audio keep running, and re-kick the redraw loop.
            // Netplay always takes the wall-clock path below (its session
            // needs the exact console rate + one-frame-per-pace).
            if self.active_pacing == ActivePacing::Display && !self.netplay.is_active() {
                let now = Instant::now();
                let stalled = self
                    .last_redraw
                    .is_none_or(|t| now.duration_since(t) > DISPLAY_SYNC_WATCHDOG);
                if stalled {
                    self.pump_gamepad();
                    self.latch_input();
                    let next = self.emu.lock().next_frame_time.unwrap_or(now);
                    self.produce_due_frames(now, next);
                    self.post_produce_housekeeping();
                    if let Some(gfx) = self.gfx.as_ref() {
                        gfx.window.request_redraw();
                    }
                }
                // Wake again within the watchdog window even with no OS
                // events (the redraw loop itself normally generates them).
                event_loop.set_control_flow(ControlFlow::WaitUntil(now + DISPLAY_SYNC_WATCHDOG));
                return;
            }

            let next = self.emu.lock().next_frame_time.unwrap_or_else(Instant::now);

            // Block precisely to the target with sleep+spin so frame
            // production happens on an even cadence.
            if Instant::now() < next {
                Self::block_until_native(next);
            }

            // v2.8.0 Phase 2 — LATE input latch: poll devices AFTER the
            // pacer block, immediately before `run_frame` consumes them.
            // Latching before the block (the old order) aged inputs by up
            // to a full frame before emulation even saw them.
            self.pump_gamepad();
            self.latch_input();

            let now = Instant::now();
            // Netplay (like the wasm path, commit 7dc0331) must advance the
            // rollback session by AT MOST ONE frame per pace: the normal
            // `produce_due_frames` catch-up burst (up to MAX_CATCHUP_FRAMES) and
            // its snap-forward would step the session several frames at once or
            // jump the local frame counter, desyncing the peer — which is
            // exactly the native two-instance desync. Advance one frame and pace
            // to ~60 Hz (snap-forward WITHOUT a burst if behind); the session's
            // own stall / frame-advantage keeps the peers time-synced.
            if self.netplay.is_active() {
                let t0 = Instant::now();
                self.produce_one_frame();
                let mut guard = self.emu.lock();
                let emu = &mut *guard;
                emu.perf.record_produce_cost(t0.elapsed());
                emu.perf.record_produced(now);
                let stepped = next + emu.frame_duration;
                emu.next_frame_time = Some(if stepped <= now { now } else { stepped });
            } else {
                self.produce_due_frames(now, next);
            }

            self.post_produce_housekeeping();

            // Ask the OS to present the freshly produced frame; rendering
            // happens in `RedrawRequested` (decoupled from emu pacing).
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }

            // Stay in `Poll` so `about_to_wait` re-runs immediately and
            // `block_until_native` does the precise wait for the next
            // frame (no event-loop suspension; the spin owns the cadence).
            event_loop.set_control_flow(ControlFlow::Poll);
        }
    }

    /// v2.8.0 Phase 2 — per-produced-frame housekeeping shared by the
    /// wall-clock pacer (`pace_frames`) and the display-sync redraw path:
    /// FDS save flush + audio-health refresh + perf/fps/movie pushes into
    /// the debugger + the raw-cheat pull.
    #[cfg(not(target_arch = "wasm32"))]
    fn post_produce_housekeeping(&mut self) {
        // v2.2.0 — persist the FDS writable disk if it changed this frame.
        // Cheap when clean / non-FDS (a `disk_is_dirty()` check only).
        self.flush_fds_save();

        // Push the measured fps + movie status into the debugger so the
        // user can read them from the top toolbar. One scoped lock builds
        // the whole perf snapshot (the gfx fields are App-resident and are
        // filled in after the guard drops).
        let (fps, movie_status, mut perf_view) = {
            let mut guard = self.emu.lock();
            let emu = &mut *guard;
            // v2.8.0 Phase 0 — refresh the audio-queue health + snapshot
            // the perf view for the Performance panel.
            if let Some(audio) = self.audio.as_ref() {
                emu.perf.audio = crate::perf::AudioHealth {
                    queued_samples: audio.queue.len(),
                    sample_rate: audio.sample_rate,
                    underruns: audio.queue.underruns(),
                    overrun_dropped: audio.queue.overrun_dropped(),
                };
            }
            let mut view = emu.perf.view();
            view.target_ms = emu.frame_duration.as_secs_f32() * 1000.0;
            // v2.8.0 Phase 3 — feed the run-ahead budget throttle. Keyed off
            // the median (steady-state) produce cost, not the p95 tail (which
            // on the emu thread is OS-deschedule noise, not run-ahead cost).
            emu.update_runahead_throttle(view.produce_cost.p50_ms, view.produce_cost.count);
            (emu.current_fps(), emu.movie.status(), view)
        };
        perf_view.pacing = self.pacing_label();
        if let Some(gfx) = self.gfx.as_ref() {
            perf_view.present_mode = format!("{:?}", gfx.effective_present_mode());
            perf_view.present_mode_fell_back = gfx.present_mode_fell_back();
            perf_view.gpu_ms = gfx.last_gpu_pass_ms();
        }
        // v2.8.0 — opt-in perf logging (the Perf panel "Logging" checkbox):
        // reconcile the logger with the checkbox, append the interval row,
        // and reflect the destination/error back into the panel.
        let log_enabled = self
            .debugger
            .as_ref()
            .is_some_and(DebuggerOverlay::perf_logging_enabled);
        let log_ctx = self
            .perf_logger
            .wants_start(log_enabled, &self.rom_label)
            .then(|| self.perf_log_context());
        self.perf_logger
            .sync(log_enabled, &self.rom_label, || log_ctx.unwrap_or_default());
        self.perf_logger.record(&perf_view);
        let log_note = self.perf_logger.note();
        if let Some(debugger) = self.debugger.as_mut() {
            debugger.set_fps(fps);
            debugger.set_movie_status(movie_status);
            debugger.set_perf_log_note(log_note);
            debugger.set_perf_view(perf_view);
            // v1.7.0 — pull the live enabled raw-cheat list edited in the
            // cheat panel so the next produce iteration pokes the current
            // set (mirrors the fps / movie-status pull pattern).
            self.emu.lock().raw_cheats = debugger.enabled_raw_cheats();
        }
    }

    /// v2.8.0 — the static run context written into a perf-log header: the
    /// game identity plus every configuration axis that shapes the numbers
    /// in the rows (pacing, present mode, audio, run-ahead, rewind,
    /// display, build). Built once per log-file start, not per frame.
    #[cfg(not(target_arch = "wasm32"))]
    fn perf_log_context(&self) -> crate::perf_log::PerfLogContext {
        // One short lock pulls the emu-side facts; the string building below
        // runs with the guard dropped.
        let (target_ms, runahead_throttled, rom_sha256) = {
            let emu = self.emu.lock();
            (
                emu.frame_duration.as_secs_f32() * 1000.0,
                emu.runahead_throttled,
                emu.nes
                    .as_ref()
                    .map(|n| crate::save_state::hex_sha256(n.rom_sha256())),
            )
        };
        let monitor_hz = self
            .gfx
            .as_ref()
            .and_then(|g| g.window.current_monitor())
            .and_then(|m| m.refresh_rate_millihertz())
            .map_or_else(
                || "unknown".to_string(),
                |mhz| format!("{:.3}", f64::from(mhz) / 1000.0),
            );
        let mut settings: Vec<(&'static str, String)> = vec![
            ("version", env!("CARGO_PKG_VERSION").to_string()),
            (
                "build",
                if cfg!(debug_assertions) {
                    "debug".to_string()
                } else {
                    "release".to_string()
                },
            ),
            (
                "os",
                format!("{} {}", std::env::consts::OS, std::env::consts::ARCH),
            ),
            ("target_ms", format!("{target_ms:.3}")),
            ("monitor_refresh_hz", monitor_hz),
            ("pacing_mode", self.config.graphics.pacing_mode.clone()),
            ("pacing_active", self.pacing_label()),
            (
                "present_mode_pref",
                self.config.graphics.present_mode.clone(),
            ),
            (
                "max_frame_latency",
                self.config.graphics.max_frame_latency.to_string(),
            ),
            ("ntsc_filter", self.config.graphics.ntsc_filter.clone()),
            (
                "audio_sample_rate_pref",
                self.config.audio.sample_rate.to_string(),
            ),
            (
                "audio_sample_rate",
                self.audio
                    .as_ref()
                    .map_or_else(|| "none".to_string(), |a| a.sample_rate.to_string()),
            ),
            ("audio_latency_ms", self.config.audio.latency_ms.to_string()),
            ("audio_drc", self.config.audio.drc.to_string()),
            ("run_ahead", self.config.input.run_ahead.to_string()),
            ("run_ahead_throttled", runahead_throttled.to_string()),
            ("rewind_enabled", self.config.rewind.enabled.to_string()),
        ];
        if let Some(gfx) = self.gfx.as_ref() {
            settings.push((
                "present_mode_effective",
                format!("{:?}", gfx.effective_present_mode()),
            ));
        }
        crate::perf_log::PerfLogContext {
            rom_label: self.rom_label.clone(),
            rom_sha256,
            settings,
        }
    }

    /// v2.8.0 Phase 2 — resolve the pacing regime from `[graphics]
    /// pacing_mode` × the monitor's declared refresh × the ROM's nominal
    /// rate, and apply the matching present mode to the surface.
    ///
    /// - `display` engages only when the refresh is within
    ///   [`DISPLAY_SYNC_MAX_SKEW`] of the console rate — one frame per
    ///   refresh at 144 Hz would run the game 2.4× fast, so an out-of-band
    ///   request falls back to `wallclock` with a warning.
    /// - The sticky [`Self::display_fallback`] (sustained missed presents)
    ///   also forces `wallclock` until the user re-applies the setting.
    /// - Netplay is handled at use sites (it always paces wall-clock).
    #[cfg(not(target_arch = "wasm32"))]
    fn resolve_pacing(&mut self) {
        let mode = self.config.graphics.pacing_mode.to_ascii_lowercase();
        let nominal_hz = 1.0 / self.emu.lock().frame_duration.as_secs_f64();
        let monitor_hz = self
            .gfx
            .as_ref()
            .and_then(|g| g.window.current_monitor())
            .and_then(|m| m.refresh_rate_millihertz())
            .map(|mhz| f64::from(mhz) / 1000.0);
        let within_skew = monitor_hz
            .is_some_and(|hz| ((hz - nominal_hz) / nominal_hz).abs() <= DISPLAY_SYNC_MAX_SKEW);

        let want = match mode.as_str() {
            "display" => {
                if within_skew && !self.display_fallback {
                    ActivePacing::Display
                } else {
                    if !within_skew {
                        eprintln!(
                            "rustynes: pacing_mode=display requested but the monitor \
                             refresh ({}) is not within 0.5% of the console rate \
                             ({nominal_hz:.4} Hz) — using wallclock pacing.",
                            monitor_hz
                                .map_or_else(|| "unknown".to_string(), |hz| format!("{hz:.3} Hz"))
                        );
                    }
                    ActivePacing::Wallclock
                }
            }
            "vrr" => ActivePacing::Vrr,
            "wallclock" => ActivePacing::Wallclock,
            // "auto" (and anything unrecognized): display-sync when the
            // panel matches the console rate, else the wall-clock pacer.
            _ => {
                if within_skew && !self.display_fallback {
                    ActivePacing::Display
                } else {
                    ActivePacing::Wallclock
                }
            }
        };

        self.active_pacing = want;
        self.last_redraw = None;
        self.presents_since_check = 0;
        // v2.8.0 Phase 5 increment 3 — publish the regime + per-region frame
        // duration to the emulation thread so its pacer matches.
        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
        self.publish_emu_thread_regime();
        if let Some(gfx) = self.gfx.as_mut() {
            match want {
                // Display-sync + VRR both ride Fifo (always supported):
                // display-sync uses it as the clock; VRR presents at the
                // console rate and the variable-refresh display follows.
                ActivePacing::Display | ActivePacing::Vrr => {
                    let _ = gfx.set_present_mode(wgpu::PresentMode::Fifo);
                }
                // Wallclock restores the configured preference (Mailbox
                // default) so the pacer is the single timing authority.
                ActivePacing::Wallclock => {
                    gfx.apply_present_mode_pref(&self.config.graphics.present_mode);
                }
            }
        }
        eprintln!(
            "rustynes: pacing = {} (config \"{}\", console {nominal_hz:.4} Hz, monitor {})",
            self.pacing_label(),
            self.config.graphics.pacing_mode,
            monitor_hz.map_or_else(|| "unknown".to_string(), |hz| format!("{hz:.3} Hz")),
        );
    }

    /// v2.8.0 Phase 2 — the display-sync produce step, run at the top of
    /// `RedrawRequested`: exactly ONE emulated frame per redraw, before the
    /// present. Fifo backpressure paces the loop at the display refresh
    /// (within 0.5% of the console rate by the skew gate); input is latched
    /// here — the latest possible point before `run_frame`. No-op outside
    /// the display regime (and during netplay, which paces wall-clock).
    #[cfg(not(target_arch = "wasm32"))]
    fn display_sync_produce(&mut self) {
        // v2.8.0 Phase 5 increment 3 — when the emulation thread drives, it
        // produces the display-regime frame on the present tick
        // (`display_sync_after_present` notifies it); the winit thread only
        // presents. Stand down.
        if self.emu_thread_drives() {
            return;
        }
        if self.active_pacing != ActivePacing::Display
            || self.netplay.is_active()
            || self.emu.lock().nes.is_none()
        {
            return;
        }
        self.last_redraw = Some(Instant::now());
        self.pump_gamepad();
        self.latch_input();
        let t0 = Instant::now();
        self.produce_one_frame();
        {
            let mut guard = self.emu.lock();
            let emu = &mut *guard;
            emu.perf.record_produce_cost(t0.elapsed());
            emu.perf.record_produced(Instant::now());
            // Keep the watchdog base fresh so an occlusion stall resumes
            // from "now", not from minutes ago.
            emu.next_frame_time = Some(Instant::now() + emu.frame_duration);
        }
        self.post_produce_housekeeping();
    }

    /// v2.8.0 Phase 2 — display-sync self-drive + health check, run after a
    /// successful present: re-arm the next redraw (Fifo backpressure makes
    /// the loop run at exactly the display refresh) and, every 60 presents,
    /// verify the cadence is actually being held — sustained misses (p95 of
    /// presented intervals > 1.5× the frame target) fall back to the
    /// wall-clock pacer, sticky for the session.
    #[cfg(not(target_arch = "wasm32"))]
    fn display_sync_after_present(&mut self) {
        if self.active_pacing != ActivePacing::Display || self.netplay.is_active() {
            return;
        }
        // v2.8.0 Phase 5 increment 3 — in threaded display mode the present
        // is the clock: ping the emulation thread to produce the next frame.
        // (When the thread is off, the winit thread produces it in
        // `display_sync_produce` on the redraw this re-arms.)
        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
        if let Some(thread) = self.emu_thread.as_ref() {
            thread.notify_present();
        }
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
        self.presents_since_check += 1;
        if self.presents_since_check >= 60 {
            self.presents_since_check = 0;
            let (stats, target) = {
                let emu = self.emu.lock();
                (
                    emu.perf.view().presented,
                    emu.frame_duration.as_secs_f32() * 1000.0,
                )
            };
            if stats.count >= 240 && stats.p95_ms > target * 1.5 {
                self.display_fallback = true;
                eprintln!(
                    "rustynes: display-sync is missing presents (p95 {:.2} ms vs \
                     {:.2} ms target) — falling back to wallclock pacing for this session.",
                    stats.p95_ms, target
                );
                self.resolve_pacing();
            }
        }
    }

    /// v2.8.0 Phase 3 — one-time backfill of the presented framebuffer when
    /// a redraw arrives before the first produce (associated fn so the
    /// caller's disjoint `nes` borrow stays simple).
    fn backfill_present_fb(present_fb: &mut Vec<u8>, nes: &Nes) {
        if present_fb.is_empty() {
            present_fb.extend_from_slice(nes.framebuffer());
        }
    }

    /// Human-readable active-pacing label for the Performance panel.
    #[cfg(not(target_arch = "wasm32"))]
    fn pacing_label(&self) -> String {
        let base = match self.active_pacing {
            ActivePacing::Wallclock => "wallclock",
            ActivePacing::Display => "display-sync",
            ActivePacing::Vrr => "vrr",
        };
        if self.display_fallback && self.active_pacing == ActivePacing::Wallclock {
            format!("{base} (display-sync fell back)")
        } else {
            base.to_string()
        }
    }

    /// Produce the elapsed frame slots (see
    /// [`crate::emu::EmuCore::produce_due_frames`]) and apply the surfaced
    /// UI side effects.
    fn produce_due_frames(&mut self, now: Instant, next: Instant) {
        let inputs = self.frame_inputs();
        // Scope the sink borrows of `self.audio` / `self.ra` so they end
        // before `apply_produce_fx` re-borrows `self`.
        let fx = {
            #[cfg(not(target_arch = "wasm32"))]
            let mut sinks = Self::sync_sinks(
                &mut self.audio,
                #[cfg(feature = "retroachievements")]
                &mut self.ra,
            );
            #[cfg(target_arch = "wasm32")]
            let mut sinks = crate::emu::FrameSinks {
                _marker: core::marker::PhantomData,
            };
            self.emu
                .lock()
                .produce_due_frames(now, next, &inputs, &mut sinks)
        };
        self.apply_produce_fx(fx);
    }

    /// v2.8.0 Phase 6 — whether the wasm rAF cadence matches the console rate
    /// closely enough to engage one-frame-per-rAF display-sync. `true` when
    /// the measured presented (rAF) interval is within 3% of the console
    /// frame period — i.e. a ~60 Hz panel showing 60.0988 Hz content (the
    /// audio DRC absorbs the sub-percent difference). On 120/144 Hz panels the
    /// cadence is far off, so the wall-clock-delta catch-up stays. The looser
    /// 3% gate (vs native's 0.5%) reflects the browser rAF's coarser timing.
    #[cfg(target_arch = "wasm32")]
    fn wasm_display_sync(&self) -> bool {
        let (presented, target_ms) = {
            let emu = self.emu.lock();
            (
                emu.perf.view().presented,
                emu.frame_duration.as_secs_f32() * 1000.0,
            )
        };
        presented.count >= 60
            && target_ms > 0.0
            && ((presented.p50_ms - target_ms) / target_ms).abs() <= 0.03
    }

    /// wasm32 rAF-driven pacing step, run from `RedrawRequested` (which
    /// winit delivers on `requestAnimationFrame`, i.e. display-refresh
    /// synced). Latches input, produces the frames that are due by
    /// `web_time::Instant` delta (catch-up capped), updates the fps
    /// readout, and re-arms the next rAF via `request_redraw()` so the
    /// loop keeps stepping ~once per refresh. Producing by elapsed-time
    /// delta (not one-frame-per-rAF unconditionally) keeps wall-clock
    /// NTSC speed correct on non-60 Hz panels.
    #[cfg(target_arch = "wasm32")]
    fn pace_and_produce_wasm(&mut self) {
        // Produce the due frames only when a ROM is loaded; the
        // `request_redraw()` re-arm at the end runs UNCONDITIONALLY (even
        // on the `nes.is_none()` pre-ROM path) so the rAF loop never dies.
        // If any tick failed to re-arm, winit's web backend would stop
        // calling `requestAnimationFrame` and the canvas would freeze.
        if self.emu.lock().nes.is_some() {
            // Latch the browser-sourced input just before producing.
            self.latch_input();
            let now = Instant::now();
            let netplay_active = self
                .browser_netplay
                .as_ref()
                .is_some_and(crate::wasm_netplay::BrowserNetplay::is_active);
            // v2.8.0 Phase 6 — rAF display-sync: when the measured rAF cadence
            // matches the console rate (a ~60 Hz panel), produce exactly one
            // frame per rAF and let the audio DRC absorb the sub-percent rate
            // difference — eliminating the wall-clock-vs-rAF beat that dups /
            // drops a frame every ~9 s. Off during netplay (one-frame-per-tick
            // is driven below) and on non-60 Hz panels (wall-clock catch-up).
            let display_sync = !netplay_active && self.wasm_display_sync();

            let produced = if netplay_active {
                // Browser netplay must advance the rollback session by AT MOST
                // ONE frame per tick: the normal `produce_due_frames` catch-up
                // burst (and its snap-forward) would step the session several
                // frames at once or jump the local frame counter, desyncing the
                // peer. Advance one frame and pace to ~60 Hz, snapping forward
                // (without a burst) if we fell behind — e.g. a backgrounded,
                // rAF-throttled tab. The session's own stall / frame-advantage
                // keeps the two peers time-synced.
                let next = self.emu.lock().next_frame_time.unwrap_or(now);
                if now >= next {
                    let t0 = Instant::now();
                    self.produce_one_frame();
                    let mut guard = self.emu.lock();
                    let emu = &mut *guard;
                    emu.perf.record_produce_cost(t0.elapsed());
                    emu.perf.record_produced(now);
                    let stepped = next + emu.frame_duration;
                    emu.next_frame_time = Some(if stepped <= now { now } else { stepped });
                    true
                } else {
                    false
                }
            } else if display_sync {
                // One frame per rAF — the present is the clock (winit delivers
                // RedrawRequested on requestAnimationFrame).
                let t0 = Instant::now();
                self.produce_one_frame();
                let mut guard = self.emu.lock();
                let emu = &mut *guard;
                emu.perf.record_produce_cost(t0.elapsed());
                emu.perf.record_produced(now);
                emu.next_frame_time = Some(now + emu.frame_duration);
                true
            } else {
                let next = self.emu.lock().next_frame_time.unwrap_or(now);
                if now >= next {
                    self.produce_due_frames(now, next);
                    true
                } else {
                    false
                }
            };

            if produced {
                let (fps, movie_status, mut perf_view) = {
                    let emu = self.emu.lock();
                    let mut view = emu.perf.view();
                    view.target_ms = emu.frame_duration.as_secs_f32() * 1000.0;
                    (emu.current_fps(), emu.movie.status(), view)
                };
                perf_view.pacing = if display_sync { "raf-display" } else { "raf" }.into();
                // v2.8.0 Phase 6 — wire the AudioWorklet ring health into the
                // Perf panel (occupancy / underruns / overruns), the wasm
                // analog of the native cpal queue counters.
                let (queued_samples, sample_rate, underruns, overrun_dropped) =
                    crate::wasm_audio::audio_health();
                perf_view.audio = crate::perf::AudioHealth {
                    queued_samples,
                    sample_rate,
                    underruns,
                    overrun_dropped,
                };
                if let Some(gfx) = self.gfx.as_ref() {
                    perf_view.present_mode = format!("{:?}", gfx.effective_present_mode());
                    perf_view.present_mode_fell_back = gfx.present_mode_fell_back();
                    perf_view.gpu_ms = gfx.last_gpu_pass_ms();
                }
                if let Some(debugger) = self.debugger.as_mut() {
                    debugger.set_fps(fps);
                    debugger.set_movie_status(movie_status);
                    debugger.set_perf_view(perf_view);
                    // v1.7.0 — pull the live enabled raw-cheat list edited in
                    // the cheat panel so the next produce iteration pokes the
                    // current set (mirrors the fps / movie-status pull).
                    self.emu.lock().raw_cheats = debugger.enabled_raw_cheats();
                }
            }
        }
        // Re-arm the next rAF tick. `request_redraw()` -> winit's web
        // backend `requestAnimationFrame`, so the next `RedrawRequested`
        // fires on the next display refresh (smooth, vsync-paced
        // production). This self-reschedule is the SOLE heartbeat of the
        // wasm frame loop — it MUST run on every tick, including the
        // pre-ROM `nes.is_none()` path, or the loop stalls.
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// v1.3.0 Sprint 1.4 — shared post-wgpu-init setup, called by
    /// `resumed` on native and by `user_event(GfxReady)` on wasm32.
    /// Installs the NTSC filter + egui debugger, then starts the
    /// emulator. On native the NES is created here (the ROM bytes are
    /// present from `App::new`); on wasm32 the NES is deferred until
    /// an `AppEvent::RomLoaded` arrives (the browser file picker).
    fn on_gfx_ready(&mut self, mut gfx: Gfx, event_loop: &ActiveEventLoop) {
        // Sprint 5-3 — optional NTSC filter.
        if self.config.graphics.ntsc_filter != "off" {
            gfx.enable_ntsc();
        }
        // Sprint 5-3 — egui debugger overlay.
        let surface_format = gfx.surface_format();
        let mut debugger = DebuggerOverlay::new(&gfx.device, gfx.window.as_ref(), surface_format);
        // v2.8.0 Phase 0 — surface a present-mode fallback instead of
        // silently double-gating the wall-clock pacer against vsync.
        if gfx.present_mode_fell_back() {
            debugger.set_present_mode_warning(Some(format!(
                "\"{}\" is not supported here — running Fifo (vsync). Expect a \
                 periodic hitch every ~10 s on a 60 Hz panel.",
                self.config.graphics.present_mode
            )));
        }
        self.gfx = Some(gfx);
        self.debugger = Some(debugger);

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Native: cpal audio + NES from the ROM bytes loaded in `new`.
            // v2.8.0 Phase 1 — the `[audio]` config is finally wired: the
            // preferred sample rate is requested from the device (falling
            // back to its default), and the latency target + DRC toggle
            // configure the resampler stage.
            let audio = match AudioOutput::try_new(
                Some(self.config.audio.sample_rate),
                self.config.audio.latency_ms,
                self.config.audio.drc,
            ) {
                Ok(a) => Some(a),
                Err(e) => {
                    eprintln!("rustynes: audio disabled: {e}");
                    None
                }
            };
            let sample_rate = audio.as_ref().map_or(44_100, |a| a.sample_rate);
            self.audio = audio;
            // v2.8.0 Phase 5 increment 3 — spawn the emulation thread (it
            // idles until `start_nes` flips its `has_rom`). The `Send` audio
            // producer is made from `self.audio` here, so it must precede
            // `start_nes` (which sets `has_rom` and lets the thread run).
            #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
            self.spawn_emu_thread();
            self.start_nes(sample_rate, event_loop);
        }
        #[cfg(target_arch = "wasm32")]
        {
            // wasm32: audio is the Web Audio path (Sprint 1.4a, in
            // `wasm.rs`); the NES waits for `AppEvent::RomLoaded`
            // unless bytes are somehow already present.
            if self.rom_bytes.is_empty() {
                // Idle the event loop on `Wait` (NOT the bootstrap `Poll`,
                // which busy-loops on winit's web backend) and kick the
                // first rAF. `pace_and_produce_wasm` re-arms unconditionally
                // from here on, so the rAF heartbeat stays alive even before
                // a ROM is loaded.
                event_loop.set_control_flow(ControlFlow::Wait);
                if let Some(g) = &self.gfx {
                    g.window.request_redraw();
                }
            } else {
                self.start_nes(44_100, event_loop);
            }
        }
    }

    /// Create the `Nes` from `self.rom_bytes` at `sample_rate`, wire
    /// rewind + frame timing, and schedule the first frame. Shared by
    /// the native `on_gfx_ready` and the wasm `RomLoaded` path.
    fn start_nes(&mut self, sample_rate: u32, event_loop: &ActiveEventLoop) {
        // v2.2.0 — FDS path: build from the disk image + the disksys.rom BIOS.
        // The standard cartridge path is unchanged. wasm32 resolves the BIOS
        // from the in-memory `fds_bios_bytes` upload (no filesystem prompt).
        #[cfg(not(target_arch = "wasm32"))]
        if is_fds_image(&self.rom_bytes) {
            let disk = std::mem::take(&mut self.rom_bytes);
            let built = self.build_fds_nes(&disk, sample_rate);
            self.rom_bytes = disk;
            if let Some(nes) = built {
                return self.finish_start_nes(nes, event_loop);
            }
            // BIOS cancelled / wrong size: a startup FDS load can't proceed.
            // Native: fatal (no running session yet).
            event_loop.exit();
            return;
        }
        #[cfg(target_arch = "wasm32")]
        if is_fds_image(&self.rom_bytes) {
            // wasm: if the BIOS isn't uploaded yet, keep waiting (the user can
            // upload it, which then retries the build via `set_fds_bios_wasm`).
            if let Some(nes) = self.build_fds_nes_wasm(sample_rate) {
                return self.finish_start_nes(nes, event_loop);
            }
            return;
        }

        let nes = match Nes::from_rom_with_sample_rate(&self.rom_bytes, sample_rate) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("rustynes: failed to load ROM: {e}");
                // A bad ROM at native startup is fatal; on wasm32 we
                // just keep waiting for another file-picker selection.
                #[cfg(not(target_arch = "wasm32"))]
                event_loop.exit();
                return;
            }
        };
        // v2.2.0 — clear any prior FDS save key (standard cartridge path).
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.emu.lock().fds_disk_sha256 = None;
        }
        self.finish_start_nes(nes, event_loop);
    }

    /// Common post-construction wiring shared by the cartridge + FDS branches
    /// of [`Self::start_nes`]: rewind ring, Four Score, frame timing, the first
    /// redraw kick, and the cheat/expansion-device sync.
    // `&mut self` is only exercised by the native body (`resolve_pacing` /
    // `apply_cheats_for_current_rom`); the wasm build mutates through the emu
    // lock alone — a cfg artifact.
    #[cfg_attr(target_arch = "wasm32", allow(clippy::needless_pass_by_ref_mut))]
    fn finish_start_nes(&mut self, mut nes: Nes, event_loop: &ActiveEventLoop) {
        if self.config.rewind.enabled {
            // 60 fps × max_seconds × ~120 KiB/snapshot keyframe ≈ ~7 MiB
            // before delta compression; we cap at 32 MiB by default.
            let max_bytes: usize =
                ((self.config.rewind.max_seconds as usize) * 60).max(60) * 200 * 1024;
            nes.enable_rewind_with(
                max_bytes.min(rustynes_core::REWIND_DEFAULT_MAX_BYTES),
                self.config.rewind.keyframe_period.max(1),
            );
        }
        // v1.7.0 — arm the Four Score 4-player adapter per config (off by
        // default; two-controller path stays byte-identical when off).
        nes.set_four_score(self.config.input.four_score);
        // v2.5.0 — apply the Vs. System DIP switches (no-op for non-Vs. games).
        // v2.7.0 — per-game DB palette + DIP preset (explicit config dip wins).
        self.apply_vs_db(&mut nes);
        {
            let mut guard = self.emu.lock();
            let emu = &mut *guard;
            // Capture the cartridge's nominal frame duration — consults the
            // cartridge region (NTSC: ~16.64 ms, PAL/Dendy: ~20 ms).
            emu.frame_duration = nes.frame_duration();
            emu.next_frame_time = Some(Instant::now() + emu.frame_duration);
            // v2.8.0 Phase 0 — fresh perf rings for the new ROM (and don't
            // log the load gap as a giant frame interval).
            emu.perf.clear();
            // v2.8.0 Phase 3 — drop the previous ROM's presented frame (the
            // render path backfills from the new `Nes` until the first
            // produce).
            emu.present_fb.clear();
        }
        // v2.8.0 Phase 2 — resolve the pacing regime for this ROM's region
        // against the monitor refresh (native; the wasm rAF loop is its own
        // regime). Locks internally, so the cluster guard above is dropped.
        #[cfg(not(target_arch = "wasm32"))]
        self.resolve_pacing();
        self.emu.lock().nes = Some(nes);
        // v2.8.0 Phase 5 increment 3 — let the (idle) emulation thread start
        // producing now that the core holds a ROM. Set AFTER `nes` is in
        // place so the thread never produces on an empty core; `resolve_pacing`
        // above already published the regime + frame duration.
        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
        if let Some(thread) = self.emu_thread.as_ref() {
            thread.control().set_has_rom(true);
        }
        // v1.6.0 — apply this ROM's persisted Game Genie cheats (native).
        #[cfg(not(target_arch = "wasm32"))]
        self.apply_cheats_for_current_rom();
        // v2.7.0 — identify the ROM with RetroAchievements + load its progress
        // sidecar. No-op when no RA session is active.
        #[cfg(feature = "retroachievements")]
        self.load_ra_game();
        // v2.1.0 — attach the configured non-standard input device (native).
        #[cfg(not(target_arch = "wasm32"))]
        self.sync_expansion_device();
        // First frame kick. On native this redraw just presents; the
        // wall-clock pacer in `about_to_wait` drives production. On wasm32
        // this is the FIRST `requestAnimationFrame` of the rAF-driven
        // frame loop — `RedrawRequested` -> `pace_and_produce_wasm` then
        // re-arms each subsequent rAF.
        if let Some(g) = &self.gfx {
            g.window.request_redraw();
        }
        // Native: arm `WaitUntil(next)` so the event loop sleeps until the
        // first frame is due (the pacer flips it back to `Poll`). wasm32:
        // use `Wait` so the event loop idles between rAF callbacks — the
        // rAF self-reschedule (via `request_redraw`) is the SOLE heartbeat.
        // `Poll` here would busy-loop in parallel with rAF (two schedulers
        // on one main thread), starving the emulation (the v1.3.2 stutter).
        #[cfg(not(target_arch = "wasm32"))]
        {
            let next_frame_time = self.emu.lock().next_frame_time;
            if let Some(target) = next_frame_time {
                event_loop.set_control_flow(ControlFlow::WaitUntil(target));
            }
        }
        #[cfg(target_arch = "wasm32")]
        event_loop.set_control_flow(ControlFlow::Wait);
    }
}

/// v2.3.0 — convert the netplay UI's status into the debugger's
/// (target-agnostic) view struct for the HUD + panel. Native-only (the source
/// `NetplayStatus` lives in the native-only `netplay_ui` module).
#[cfg(not(target_arch = "wasm32"))]
fn netplay_status_view(s: &crate::netplay_ui::NetplayStatus) -> crate::debugger::NetplayStatusView {
    use crate::debugger::NetplayPhaseView;
    use crate::netplay_ui::NetplayPhase;
    let phase = match s.phase {
        NetplayPhase::Idle => NetplayPhaseView::Idle,
        NetplayPhase::Connecting => NetplayPhaseView::Connecting,
        NetplayPhase::InGame => NetplayPhaseView::InGame,
        NetplayPhase::Error => NetplayPhaseView::Error,
    };
    crate::debugger::NetplayStatusView {
        phase,
        is_host: s.is_host,
        ping_ms: s.ping_ms,
        current_frame: s.current_frame,
        confirmed_frame: s.confirmed_frame,
        rolled_back: s.rolled_back,
        resimulated_frames: s.resimulated_frames,
        stalled: s.stalled,
        message: s.message.clone(),
    }
}

impl ApplicationHandler<AppEvent> for App {
    fn resumed(&mut self, event_loop: &ActiveEventLoop) {
        if self.gfx.is_some() {
            return;
        }
        // Window + GPU surface.
        let window = match self.create_window(event_loop) {
            Ok(w) => w,
            Err(e) => {
                eprintln!("rustynes: failed to create window: {e}");
                event_loop.exit();
                return;
            }
        };
        // v1.3.0 Sprint 1.4 — `Gfx::new` is async so the same path
        // works on wasm32. Native drives it to completion via
        // `pollster::block_on` and continues synchronously; wasm32
        // spawns the future and delivers the result back through the
        // `EventLoopProxy<AppEvent>` (handled in `user_event`).
        #[cfg(not(target_arch = "wasm32"))]
        match pollster::block_on(Gfx::new(
            window,
            &self.config.graphics.present_mode,
            self.config.graphics.max_frame_latency,
        )) {
            Ok(gfx) => self.on_gfx_ready(gfx, event_loop),
            Err(e) => {
                eprintln!("rustynes: failed to init wgpu: {e}");
                event_loop.exit();
            }
        }
        #[cfg(target_arch = "wasm32")]
        if let Some(proxy) = self.proxy.clone() {
            let present_mode = self.config.graphics.present_mode.clone();
            let max_frame_latency = self.config.graphics.max_frame_latency;
            wasm_bindgen_futures::spawn_local(async move {
                match Gfx::new(window, &present_mode, max_frame_latency).await {
                    Ok(gfx) => {
                        let _ = proxy.send_event(AppEvent::GfxReady(Box::new(gfx)));
                    }
                    Err(e) => web_sys::console::error_1(
                        &format!("rustynes: wgpu init failed: {e}").into(),
                    ),
                }
            });
        }
    }

    /// wasm32 — async `Gfx` + browser ROM bytes arrive here.
    fn user_event(&mut self, event_loop: &ActiveEventLoop, event: AppEvent) {
        match event {
            AppEvent::GfxReady(gfx) => self.on_gfx_ready(*gfx, event_loop),
            AppEvent::RomLoaded(bytes) => {
                self.rom_bytes = bytes;
                // Match the AudioContext's actual sample rate (set up
                // by `wasm_winit::start`'s file-picker gesture) so the
                // APU output needs no resampling. Falls back to
                // 44.1 kHz if audio init failed. The `wasm_audio`
                // module only exists on wasm32; this arm is only ever
                // reached there (native never sends a user event), but
                // it's compiled on both, so gate the calls.
                #[cfg(target_arch = "wasm32")]
                let sr = {
                    crate::wasm_audio::clear_ring();
                    crate::wasm_audio::sample_rate().unwrap_or(44_100)
                };
                #[cfg(not(target_arch = "wasm32"))]
                let sr = 44_100;
                self.start_nes(sr, event_loop);
                // v2.7.0 — surface the browser netplay lobby now that a ROM is
                // loaded (the WebRTC handshake needs the ROM hash). The user can
                // close it; the `~` debugger overlay must be visible to see it.
                #[cfg(target_arch = "wasm32")]
                {
                    self.wasm_lobby.open = true;
                }
            }
            AppEvent::MovieLoaded(bytes) => {
                // v1.6.0 Sprint 4 — uploaded `.rnm` movie bytes (wasm32).
                // Native never sends this event; gate the call so the
                // method is only referenced where it's compiled.
                #[cfg(target_arch = "wasm32")]
                self.start_movie_from_bytes(&bytes);
                #[cfg(not(target_arch = "wasm32"))]
                let _ = bytes;
            }
            AppEvent::FdsBiosLoaded(bytes) => {
                // v2.2.0 — uploaded FDS BIOS bytes (wasm32). Native never
                // sends this event (it prompts via rfd instead).
                #[cfg(target_arch = "wasm32")]
                self.set_fds_bios_wasm(bytes, event_loop);
                #[cfg(not(target_arch = "wasm32"))]
                let _ = bytes;
            }
            // v2.8.0 Phase 5 increment 3 — the emulation thread produced a
            // frame: do the winit-side housekeeping + request a redraw.
            #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
            AppEvent::EmuFrame => self.on_emu_frame(),
        }
    }

    // Event-loop dispatch is naturally branchy; the retroachievements
    // feature's extra cfg arms tip cognitive_complexity one over the
    // threshold on that build flavour.
    #[allow(clippy::too_many_lines, clippy::cognitive_complexity)]
    fn window_event(
        &mut self,
        event_loop: &ActiveEventLoop,
        _window_id: WindowId,
        event: WindowEvent,
    ) {
        // Forward to the debugger overlay first; if it consumed the event
        // (e.g. egui textbox focus) we still let the system bindings see
        // it so global hotkeys keep working.
        let egui_consumed =
            if let (Some(debugger), Some(gfx)) = (self.debugger.as_mut(), self.gfx.as_ref()) {
                debugger.on_window_event(gfx.window.as_ref(), &event)
            } else {
                false
            };

        match event {
            WindowEvent::CloseRequested => {
                self.should_exit = true;
                event_loop.exit();
            }
            WindowEvent::Resized(size) => {
                if let Some(gfx) = &mut self.gfx {
                    gfx.resize(size.width, size.height);
                }
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.window_size = (size.width.max(1), size.height.max(1));
                }
            }
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::CursorMoved { position, .. } => {
                // Track the cursor for the Zapper aim / Vaus paddle position.
                self.cursor_pos = Some((position.x, position.y));
            }
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::MouseInput { state, button, .. } => {
                if button == winit::event::MouseButton::Left {
                    self.mouse_pressed = state == winit::event::ElementState::Pressed;
                }
            }
            WindowEvent::DroppedFile(path) => {
                // Native filesystem drag-and-drop. On wasm32 browser
                // file drops are a follow-up (the file picker is the
                // primary ROM-load path there).
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Accept any path with a `.nes` or `.fds` extension
                    // (case-insensitive). v2.2.0 added `.fds`.
                    let ok = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        e.eq_ignore_ascii_case("nes") || e.eq_ignore_ascii_case("fds")
                    });
                    if ok {
                        self.load_rom_from_path(&path);
                    } else {
                        eprintln!(
                            "rustynes: ignored dropped file (not a .nes / .fds image): {}",
                            path.display()
                        );
                    }
                }
                #[cfg(target_arch = "wasm32")]
                let _ = path;
            }
            WindowEvent::KeyboardInput { event, .. } => {
                // If the debugger is capturing a key for rebinding, route
                // every key press there and skip emulator input.
                let dbg_capturing = self
                    .debugger
                    .as_ref()
                    .is_some_and(DebuggerOverlay::wants_keyboard);
                if dbg_capturing {
                    return;
                }
                let action = self.input.handle_key(event.physical_key, event.state);
                if let Some(act) = action {
                    match act {
                        SysAction::Quit => {
                            self.should_exit = true;
                            event_loop.exit();
                        }
                        SysAction::SaveState => {
                            // Native: filesystem slot. wasm32 (v1.6.0
                            // Sprint 4): per-ROM `localStorage` slot keyed
                            // by ROM SHA-256 (synchronous; no IndexedDB).
                            #[cfg(not(target_arch = "wasm32"))]
                            self.handle_save_state();
                            #[cfg(target_arch = "wasm32")]
                            self.handle_save_state_wasm();
                        }
                        SysAction::LoadState => {
                            // v2.7.0 — load-state is disabled in RA hardcore
                            // mode (it would restore achievement-relevant state).
                            // Save-state SAVE stays allowed.
                            if self.ra_hardcore_blocks() {
                                self.toast_hardcore("Load state disabled (hardcore)");
                            } else {
                                #[cfg(not(target_arch = "wasm32"))]
                                self.handle_load_state();
                                #[cfg(target_arch = "wasm32")]
                                self.handle_load_state_wasm();
                            }
                        }
                        SysAction::Rewind => {
                            // No-op here. `InputState::handle_key` already
                            // toggled `rewind_held`; the actual per-frame
                            // rewind step runs in `about_to_wait` based on
                            // that flag.
                        }
                        SysAction::Reset => {
                            self.do_reset();
                        }
                        SysAction::PowerCycle => {
                            self.do_power_cycle();
                        }
                        SysAction::ToggleDebug => {
                            if let Some(d) = self.debugger.as_mut() {
                                d.toggle();
                            }
                        }
                        SysAction::OpenRom => {
                            #[cfg(not(target_arch = "wasm32"))]
                            self.open_rom_dialog();
                            // On wasm32, ROM loading is wired through the
                            // browser-native `<input type="file">` path in
                            // Sprint 1.3; the in-app OpenRom action is a
                            // no-op here.
                        }
                        SysAction::MovieRecordToggle => {
                            // Native: toggle recording; saving on stop uses
                            // the rfd `.rnm` dialog. wasm32 (v1.6.0 Sprint
                            // 4): saving on stop triggers a browser Blob
                            // download of the `.rnm` bytes.
                            #[cfg(not(target_arch = "wasm32"))]
                            self.handle_movie_record_toggle();
                            #[cfg(target_arch = "wasm32")]
                            self.handle_movie_record_toggle_wasm();
                        }
                        SysAction::MoviePlayToggle => {
                            // Native: rfd open dialog. wasm32: open the
                            // hidden `.rnm` file picker (gesture-safe here).
                            #[cfg(not(target_arch = "wasm32"))]
                            self.handle_movie_play_toggle();
                            #[cfg(target_arch = "wasm32")]
                            self.handle_movie_play_toggle_wasm();
                        }
                        SysAction::MovieBranch => {
                            #[cfg(not(target_arch = "wasm32"))]
                            self.handle_movie_branch();
                            #[cfg(target_arch = "wasm32")]
                            self.handle_movie_branch_wasm();
                        }
                        SysAction::DiskSwap => {
                            // v2.2.0 — cycle the FDS disk side (no-op for
                            // non-FDS games). Same on native + wasm.
                            self.cycle_disk_side();
                        }
                        SysAction::InsertCoin => {
                            // v2.5.0 — insert a Vs. System coin (acceptor #1).
                            // No-op for non-Vs. games. The coin latch is cleared
                            // automatically a few frames later (see `vs_coin_*`).
                            let mut guard = self.emu.lock();
                            let emu = &mut *guard;
                            if let Some(nes) = emu.nes.as_mut() {
                                nes.insert_coin(0);
                                emu.vs_coin_frames = VS_COIN_HOLD_FRAMES;
                            }
                        }
                    }
                }
                if !egui_consumed {
                    // v2.8.0 Phase 5 increment 3 — when the emu thread drives,
                    // publish the new input into its SharedInput immediately so
                    // a key press doesn't wait a full frame for the next
                    // `EmuFrame` republish; the direct latch is the
                    // synchronous-path write.
                    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
                    self.publish_shared_input();
                    self.latch_input();
                }
            }
            WindowEvent::RedrawRequested => {
                // Native: rendering is decoupled from emulation — this
                // branch only presents the most recent framebuffer.
                // Emulator advance happens in `about_to_wait` on a
                // wall-clock schedule, so a 144 Hz monitor can re-present
                // the same frame multiple times without speeding the NES
                // up (which is what the old "run a frame per redraw" loop
                // did wrong).
                //
                // wasm32: this branch is the rAF-driven heartbeat. winit's
                // web backend delivers `RedrawRequested` on
                // `requestAnimationFrame`, so we advance the emulator here
                // (display-refresh synced — the fix for the Pages stutter)
                // and then present. `pace_and_produce_wasm` re-arms the
                // next rAF via `request_redraw()`.
                #[cfg(target_arch = "wasm32")]
                self.pace_and_produce_wasm();

                // v2.8.0 Phase 2 — display-sync regime (native): produce
                // exactly one emulated frame per redraw, BEFORE presenting.
                #[cfg(not(target_arch = "wasm32"))]
                self.display_sync_produce();

                // v2.8.0 Phase 3 — the renderer presents `present_fb` (the
                // harvested per-frame framebuffer; with run-ahead it is the
                // VISIBLE future frame while `nes` holds the persistent
                // one). Backfill once if a redraw arrives before the first
                // produce; afterwards this never allocates (the historical
                // per-render `to_vec()` is gone).
                //
                // v2.8.0 Phase 5 — lock policy: when the debugger overlay is
                // VISIBLE its egui pass needs `&mut Nes`, so the emu lock is
                // held across the render (acceptable while actively
                // debugging). When HIDDEN — the common path — the presented
                // framebuffer is copied into the App staging buffer under a
                // brief lock that is DROPPED before the GPU encode +
                // present, so Fifo vsync backpressure can never block the
                // emulation side of the lock.
                let dbg_visible = self
                    .debugger
                    .as_ref()
                    .is_some_and(DebuggerOverlay::is_visible);
                let render_result = if dbg_visible {
                    let mut guard = self.emu.lock();
                    let emu = &mut *guard;
                    let Some(nes) = emu.nes.as_mut() else {
                        return;
                    };
                    Self::backfill_present_fb(&mut emu.present_fb, nes);
                    let Some(gfx) = self.gfx.as_mut() else {
                        return;
                    };
                    let present_fb = &emu.present_fb;
                    let debugger = self
                        .debugger
                        .as_mut()
                        .expect("dbg_visible implies a debugger");
                    let window = gfx.window.clone();
                    let config = &mut self.config;
                    // wasm-only: draw the browser-netplay lobby into the same
                    // egui frame (the `App` owns the lobby state). Native passes
                    // a no-op closure, so the signature churn is wasm-free.
                    #[cfg(target_arch = "wasm32")]
                    let wasm_lobby = &mut self.wasm_lobby;
                    gfx.render_with_overlay(present_fb, |device, queue, encoder, view, size| {
                        #[cfg(target_arch = "wasm32")]
                        let extra = |ctx: &egui::Context, cfg: &mut crate::config::Config| {
                            crate::wasm_lobby::show(ctx, wasm_lobby, cfg);
                        };
                        #[cfg(not(target_arch = "wasm32"))]
                        let extra = |_ctx: &egui::Context, _cfg: &mut crate::config::Config| {};
                        debugger.render(
                            device, queue, encoder, &window, view, size, nes, config, extra,
                        );
                    })
                } else {
                    {
                        let mut guard = self.emu.lock();
                        let emu = &mut *guard;
                        let Some(nes) = emu.nes.as_ref() else {
                            return;
                        };
                        Self::backfill_present_fb(&mut emu.present_fb, nes);
                        self.present_staging.clear();
                        self.present_staging.extend_from_slice(&emu.present_fb);
                    }
                    let Some(gfx) = self.gfx.as_mut() else {
                        return;
                    };
                    gfx.render(&self.present_staging)
                };
                match render_result {
                    Ok(()) => {
                        // v2.8.0 Phase 0 — the display-visible cadence. The
                        // produced-frame histogram alone cannot see judder;
                        // this one can (present-to-present deltas).
                        self.emu.lock().perf.record_presented(Instant::now());
                        // v2.8.0 Phase 2 — display-sync self-drive + health.
                        #[cfg(not(target_arch = "wasm32"))]
                        self.display_sync_after_present();
                    }
                    Err(wgpu::SurfaceError::Lost | wgpu::SurfaceError::Outdated) => {
                        if let Some(gfx) = self.gfx.as_mut() {
                            let size = gfx.window.inner_size();
                            gfx.resize(size.width, size.height);
                        }
                    }
                    Err(e) => eprintln!("rustynes: render error: {e}"),
                }

                // If the rebind modal changed a binding (or reset to
                // defaults) this frame, rebuild the live input maps so it
                // takes effect immediately instead of after a restart.
                if self
                    .debugger
                    .as_mut()
                    .is_some_and(DebuggerOverlay::take_input_bindings_dirty)
                {
                    self.input.reload_bindings(&self.config.input);
                    // v1.7.0 — the Four Score checkbox flags the bindings
                    // dirty too, so push its (possibly changed) state to
                    // the running `Nes` here on the same reload path.
                    // v2.7.0 — re-apply Four Score + the effective DIP (and the
                    // DB palette, idempotent) so a live DIP edit takes effect;
                    // explicit config dip wins over the DB preset. Take/restore
                    // the `Nes` to borrow-split `&self` (config) from the
                    // `&mut Nes` the helper needs (taken under one short lock,
                    // restored under another — `apply_vs_db` reads config only).
                    let taken = self.emu.lock().nes.take();
                    if let Some(mut nes) = taken {
                        nes.set_four_score(self.config.input.four_score);
                        self.apply_vs_db(&mut nes);
                        self.emu.lock().nes = Some(nes);
                    }
                    // v2.1.0 — the expansion-device menu selection also flags
                    // the bindings dirty; re-sync the attached device here.
                    #[cfg(not(target_arch = "wasm32"))]
                    self.sync_expansion_device();
                }

                // v1.7.0 — apply the settings panel's live-applicable edits
                // (NTSC filter toggle + rewind enable) this frame so they
                // take effect immediately. Present mode / sample rate /
                // rewind capacity are persisted-only (they need a surface /
                // stream / ring rebuild) and are labelled "(restart to
                // apply)" in the panel, so they are not handled here.
                let settings = self
                    .debugger
                    .as_mut()
                    .map(DebuggerOverlay::take_settings_apply)
                    .unwrap_or_default();
                if settings.ntsc_filter {
                    if let Some(gfx) = self.gfx.as_mut() {
                        if self.config.graphics.ntsc_filter == "off" {
                            gfx.disable_ntsc();
                        } else {
                            gfx.enable_ntsc();
                        }
                    }
                }
                if settings.rewind_enabled {
                    let mut guard = self.emu.lock();
                    if let Some(nes) = guard.nes.as_mut() {
                        if self.config.rewind.enabled {
                            let max_bytes: usize = ((self.config.rewind.max_seconds as usize) * 60)
                                .max(60)
                                * 200
                                * 1024;
                            nes.enable_rewind_with(
                                max_bytes.min(rustynes_core::REWIND_DEFAULT_MAX_BYTES),
                                self.config.rewind.keyframe_period.max(1),
                            );
                        } else {
                            nes.disable_rewind();
                        }
                    }
                }
                // v2.8.0 Phase 2 — re-resolve the pacing regime live when
                // the user changed `pacing_mode` in the settings panel. An
                // explicit re-apply also clears the sticky fallback so the
                // user can retry display-sync.
                #[cfg(not(target_arch = "wasm32"))]
                if settings.pacing_mode {
                    self.display_fallback = false;
                    self.resolve_pacing();
                    // A switch INTO display-sync needs a redraw to start the
                    // self-driving loop.
                    if let Some(gfx) = self.gfx.as_ref() {
                        gfx.window.request_redraw();
                    }
                }

                // v2.3.0 — act on a netplay host/join/leave the user clicked
                // in the netplay panel this frame. Native-only (UDP socket).
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(req) = self
                    .debugger
                    .as_mut()
                    .and_then(DebuggerOverlay::take_netplay_request)
                {
                    self.handle_netplay_request(req);
                }

                // v2.7.0 — act on a browser-netplay lobby connect/leave the user
                // clicked this frame. wasm-only (WebRTC over a signaling server).
                #[cfg(target_arch = "wasm32")]
                if let Some(req) = self.wasm_lobby.take_request() {
                    self.handle_lobby_request(req);
                }

                // v2.7.0 — act on a RetroAchievements login/logout/hardcore
                // request the user clicked in the cheevos panel this frame, and
                // persist a freshly-issued login token. Native-only +
                // feature-gated.
                #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
                {
                    if let Some(req) = self
                        .debugger
                        .as_mut()
                        .and_then(DebuggerOverlay::take_cheevos_request)
                    {
                        self.handle_cheevos_request(req);
                    }
                    self.persist_ra_token_if_new();
                }
            }
            _ => {}
        }
    }

    fn about_to_wait(&mut self, event_loop: &ActiveEventLoop) {
        if self.should_exit {
            // v2.8.0 Phase 5 increment 3 — stop + join the emulation thread
            // BEFORE the final flushes so it can't produce mid-teardown.
            #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
            if let Some(mut thread) = self.emu_thread.take() {
                thread.shutdown();
            }
            // v2.2.0 — final FDS writable-disk flush so the last writes aren't
            // lost on quit. No-op when clean / non-FDS. Native-only.
            #[cfg(not(target_arch = "wasm32"))]
            self.flush_fds_save();
            // v2.7.0 — save the RA progress sidecar on quit. No-op when no RA
            // session / game. Native-only + feature-gated.
            #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
            self.save_ra_progress();
            event_loop.exit();
            return;
        }
        // Wall-clock pacer. Native: produce up to one frame (with bounded
        // catch-up) and stay on `Poll`; the actual present happens on the
        // resulting `RedrawRequested`. wasm32: this is a no-op keep-alive
        // (`Poll`) — production runs in the rAF-driven `RedrawRequested`.
        self.pace_frames(event_loop);
    }
}

/// Drive the native run loop. Consumes the [`App`] and the `EventLoop`.
///
/// v1.3.0 Sprint 1.4 — the event loop is now typed
/// `EventLoop<AppEvent>` (via `with_user_event`) so the same `App`
/// `ApplicationHandler<AppEvent>` impl serves both native and wasm32.
/// Native never sends a user event (it creates `Gfx` synchronously);
/// the typed loop is functionally identical to the old untyped one
/// here.
///
/// # Errors
///
/// Propagates the winit run error.
#[cfg(not(target_arch = "wasm32"))]
pub fn run(rom_path: &Path) -> Result<(), Box<dyn std::error::Error>> {
    use winit::event_loop::EventLoop;
    let event_loop = EventLoop::<AppEvent>::with_user_event().build()?;
    // Initial control flow is `Poll` so `resumed()` runs immediately;
    // after the ROM is loaded, the wall-clock pacer switches to
    // `WaitUntil(next_frame_time)` to avoid burning CPU between frames.
    event_loop.set_control_flow(ControlFlow::Poll);
    let mut app = App::new(rom_path)?;
    // v2.8.0 Phase 5 increment 3 — hand the app the proxy the emulation
    // thread uses to deliver `AppEvent::EmuFrame` back into this loop.
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    {
        app.emu_proxy = Some(event_loop.create_proxy());
    }
    event_loop.run_app(&mut app)?;
    Ok(())
}

/// Drive the wasm32 run loop.
///
/// Builds the typed event loop, wires the `EventLoopProxy<AppEvent>`
/// into a fresh [`App`], and spawns it via
/// `EventLoopExtWebSys::spawn_app` (non-blocking — `run_app` would
/// block the browser event loop forever). The returned proxy is what
/// `wasm_winit.rs` uses to deliver browser ROM bytes as
/// [`AppEvent::RomLoaded`].
///
/// # Panics
///
/// Panics if the event loop can't be constructed (the browser lacks
/// the APIs winit needs) — surfaced via `console_error_panic_hook`.
#[cfg(target_arch = "wasm32")]
#[must_use]
pub fn run_wasm() -> winit::event_loop::EventLoopProxy<AppEvent> {
    use winit::event_loop::EventLoop;
    use winit::platform::web::EventLoopExtWebSys;
    let event_loop = EventLoop::<AppEvent>::with_user_event()
        .build()
        .expect("build event loop");
    event_loop.set_control_flow(ControlFlow::Poll);
    let proxy = event_loop.create_proxy();
    let app = App::new_empty(proxy.clone());
    event_loop.spawn_app(app);
    proxy
}

#[cfg(test)]
mod tests {
    use super::{is_fds_image, resolve_vs_dip};
    use crate::config::VsConfig;
    use rustynes_core::rustynes_mappers::VsPpuType;
    use rustynes_core::VsDbEntry;

    // Returns the `Some(..)` form because `resolve_vs_dip` takes the DB lookup
    // result directly (which is an `Option`).
    #[allow(clippy::unnecessary_wraps)]
    fn db(dip: u8) -> Option<VsDbEntry> {
        Some(VsDbEntry {
            vs_dip: dip,
            vs_ppu_type: VsPpuType::Rp2C04_0002,
            dual_system: false,
        })
    }

    #[test]
    fn vs_dip_precedence_explicit_config_wins() {
        // dip_set = true => the config dip always wins over the DB preset.
        let cfg = VsConfig {
            dip: 0x07,
            dip_set: true,
        };
        assert_eq!(resolve_vs_dip(cfg, db(0x10)), 0x07);
        assert_eq!(resolve_vs_dip(cfg, None), 0x07);
    }

    #[test]
    fn vs_dip_precedence_db_when_not_explicit() {
        // dip_set = false (default) => the DB preset is used for in-DB games.
        let cfg = VsConfig {
            dip: 0x00,
            dip_set: false,
        };
        assert_eq!(resolve_vs_dip(cfg, db(0x10)), 0x10);
    }

    #[test]
    fn vs_dip_precedence_falls_back_to_config_when_not_in_db() {
        // Not in the DB and not explicit => fall back to the config value
        // (which defaults to 0). Back-compat with existing `[vs] dip` configs.
        let cfg = VsConfig {
            dip: 0x05,
            dip_set: false,
        };
        assert_eq!(resolve_vs_dip(cfg, None), 0x05);
        // Default config (dip = 0) with no DB entry => 0.
        assert_eq!(resolve_vs_dip(VsConfig::default(), None), 0x00);
    }

    #[test]
    fn detects_fwnes_fds_header() {
        // The fwNES 16-byte header form opens with "FDS\x1A".
        let mut bytes = b"FDS\x1A".to_vec();
        bytes.extend_from_slice(&[1, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0]);
        assert!(is_fds_image(&bytes));
    }

    #[test]
    fn detects_headerless_fds_disk() {
        // The headerless raw form's first side opens with the disk-info
        // signature `\x01*NINTENDO-HVC*`.
        let bytes = b"\x01*NINTENDO-HVC*".to_vec();
        assert!(is_fds_image(&bytes));
    }

    #[test]
    fn rejects_ines_cartridge() {
        // A standard iNES / NES 2.0 cartridge opens with "NES\x1A" and must
        // NOT be mistaken for an FDS image (the `.nes` path is unchanged).
        let bytes = b"NES\x1A\x02\x01\x00\x00".to_vec();
        assert!(!is_fds_image(&bytes));
    }

    #[test]
    fn rejects_empty_and_short_inputs() {
        assert!(!is_fds_image(&[]));
        assert!(!is_fds_image(b"FD"));
        assert!(!is_fds_image(b"\x01*NIN"));
    }
}
