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

use rustynes_core::{Buttons, Nes};
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

/// v1.0.0 (UX3 BUG-2) — minimum window WIDTH (logical px) that keeps the
/// always-on chrome readable. The menu bar (File / Emulation / Tools / View /
/// Debug / Help) plus the status bar need a comfortable width; at 1x the NES is
/// only 256 px wide, far too narrow, so View -> Window Size clamps the width up
/// to this floor and the game letterboxes within (exactly like a drag-resize).
#[cfg(not(target_arch = "wasm32"))]
const MIN_CHROME_WIDTH: f64 = 560.0;

/// v1.0.0 (UX3 BUG-2) — the chrome HEIGHT (logical px) reserved above/below the
/// emulated image for the menu bar + status bar. The game area is
/// `NES_H * scale`; the total window height is that plus this allowance so the
/// emulated picture lands near the requested multiple with room for the chrome.
#[cfg(not(target_arch = "wasm32"))]
const CHROME_HEIGHT: f64 = 56.0;

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

/// Extract the first NES / FDS / NSF entry from a `.zip` archive, returning its
/// base file name and bytes. Returns `None` if the archive is unreadable or
/// holds no recognized ROM entry. Native-only (uses the `zip` crate, which is in
/// the `cfg(not(wasm))` dependency table).
#[cfg(not(target_arch = "wasm32"))]
fn extract_rom_from_zip(zip_bytes: &[u8]) -> Option<(String, Vec<u8>)> {
    use std::io::Read;
    // Reject an implausibly large entry (zip bomb / corrupt archive) before
    // reading it into memory — NES images are at most a few MiB. Both the
    // declared size AND the actual read are bounded, since the declared size
    // can lie (Gemini security-high + Copilot, PR #74).
    const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).ok()?;
    let idx = (0..archive.len()).find(|&i| {
        archive.by_index(i).is_ok_and(|f| {
            std::path::Path::new(f.name()).extension().is_some_and(|e| {
                e.eq_ignore_ascii_case("nes")
                    || e.eq_ignore_ascii_case("fds")
                    || e.eq_ignore_ascii_case("nsf")
                    || e.eq_ignore_ascii_case("unf")
                    || e.eq_ignore_ascii_case("unif")
            })
        })
    })?;
    let file = archive.by_index(idx).ok()?;
    if file.size() > MAX_ENTRY_BYTES {
        return None;
    }
    let name = std::path::Path::new(file.name())
        .file_name()
        .map_or_else(|| "rom".to_string(), |n| n.to_string_lossy().into_owned());
    let cap = usize::try_from(file.size()).unwrap_or(0);
    let mut out = Vec::with_capacity(cap);
    file.take(MAX_ENTRY_BYTES).read_to_end(&mut out).ok()?;
    Some((name, out))
}

/// Read a ROM file and run the same ingest preprocessing the in-app loader does
/// (`load_rom_from_path`): if the path is a `.zip`, extract the first NES / FDS /
/// NSF entry; then auto-apply a same-stem `.bps` / `.ups` / `.ips` soft-patch
/// (that precedence; highest present wins), before any format detection — so the
/// deterministic parse + the CRC-keyed per-game DB see the extracted / patched
/// image. Returns the processed bytes and a display label (the inner archive
/// entry name when unzipped, else the file name). Used by `App::new` so a ROM
/// passed on argv loads identically to one opened from the menu / drag-drop.
/// Native-only (the in-app menu path handles the running-app case; the wasm
/// `AppEvent::RomLoaded` path preprocesses separately).
#[cfg(not(target_arch = "wasm32"))]
fn load_and_preprocess_rom(rom_path: &Path) -> std::io::Result<(Vec<u8>, String)> {
    let mut bytes = std::fs::read(rom_path)?;
    let mut label = rom_path
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("rom.nes")
        .to_string();
    if rom_path
        .extension()
        .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
    {
        match extract_rom_from_zip(&bytes) {
            Some((name, rom)) => {
                eprintln!("rustynes: loaded {name} from archive");
                bytes = rom;
                label = name;
            }
            None => {
                return Err(std::io::Error::new(
                    std::io::ErrorKind::InvalidData,
                    format!("no NES/FDS/NSF entry in {}", rom_path.display()),
                ));
            }
        }
    }
    for ext in ["bps", "ups", "ips"] {
        let patch_path = rom_path.with_extension(ext);
        if patch_path == rom_path {
            continue;
        }
        let Ok(patch_bytes) = std::fs::read(&patch_path) else {
            continue;
        };
        match crate::patch::detect_and_apply(&bytes, &patch_bytes, ext) {
            Ok(patched) => {
                eprintln!("rustynes: applied {ext} patch: {}", patch_path.display());
                bytes = patched;
            }
            Err(e) => {
                eprintln!("rustynes: {ext} patch {} failed: {e}", patch_path.display());
            }
        }
        break;
    }
    Ok((bytes, label))
}

/// `true` when `bytes` is an NSF music file — classic `NESM\x1A` or the
/// extended chunked `NSFE` container (both play through `Nes::from_nsf`).
#[cfg(not(target_arch = "wasm32"))]
fn is_nsf_image(bytes: &[u8]) -> bool {
    bytes.starts_with(b"NESM\x1A") || bytes.starts_with(b"NSFE")
}

/// Parse the (title, artist, copyright) strings from an NSF file for display in
/// the NSF player panel. Delegates to [`rustynes_core::rustynes_mappers::parse_nsf`],
/// which reads the classic `NESM\x1A` header's 32-byte NUL-terminated fields at
/// `$0E` / `$2E` / `$4E` **and** decodes the extended `NSFE` container's `auth`
/// chunk. Using the shared parser (rather than a fixed-offset read here) keeps
/// this correct for both containers: an `NSFE` file's chunk stream is *not*
/// classic-header-shaped, so a raw `$0E`/`$2E`/`$4E` read would surface arbitrary
/// chunk bytes as text. Returns empty strings when the file fails to parse or a
/// field is absent/truncated (`parse_nsf` already trims each field at its first
/// NUL and defaults missing `auth` strings to empty).
#[cfg(not(target_arch = "wasm32"))]
fn nsf_header_strings(bytes: &[u8]) -> (String, String, String) {
    match rustynes_core::rustynes_mappers::parse_nsf(bytes) {
        Ok(nsf) => (
            nsf.song_name.to_string(),
            nsf.artist.to_string(),
            nsf.copyright.to_string(),
        ),
        Err(_) => (String::new(), String::new(), String::new()),
    }
}

/// CRC-32 (IEEE) over a byte slice (PNG chunk CRC). Used by [`encode_png_rgba`].
#[cfg(not(target_arch = "wasm32"))]
fn png_crc32(bytes: &[u8]) -> u32 {
    let mut crc: u32 = 0xFFFF_FFFF;
    for &b in bytes {
        crc ^= u32::from(b);
        for _ in 0..8 {
            let mask = (crc & 1).wrapping_neg();
            crc = (crc >> 1) ^ (0xEDB8_8320 & mask);
        }
    }
    !crc
}

/// Adler-32 (zlib stream check) over a byte slice. Used by [`encode_png_rgba`].
#[cfg(not(target_arch = "wasm32"))]
fn png_adler32(bytes: &[u8]) -> u32 {
    let (mut a, mut b): (u32, u32) = (1, 0);
    for &byte in bytes {
        a = (a + u32::from(byte)) % 65521;
        b = (b + a) % 65521;
    }
    (b << 16) | a
}

/// Append a length-prefixed, CRC'd PNG chunk to `out`.
#[cfg(not(target_arch = "wasm32"))]
fn png_write_chunk(out: &mut Vec<u8>, kind: [u8; 4], data: &[u8]) {
    let len = u32::try_from(data.len()).unwrap_or(u32::MAX);
    out.extend_from_slice(&len.to_be_bytes());
    let start = out.len();
    out.extend_from_slice(&kind);
    out.extend_from_slice(data);
    let crc = png_crc32(&out[start..]);
    out.extend_from_slice(&crc.to_be_bytes());
}

/// v1.0.0 — minimal self-contained PNG encoder for an RGBA8 framebuffer (used
/// by the Take Screenshot menu action). Emits a valid 8-bit truecolor-alpha PNG
/// using zlib STORED (uncompressed) blocks, so it needs no compression crate —
/// the 256x240 NES frame is ~245 KiB, a fine size for a screenshot. Native-only
/// (wasm has no filesystem to write to).
#[cfg(not(target_arch = "wasm32"))]
fn encode_png_rgba(rgba: &[u8], width: u32, height: u32) -> std::io::Result<Vec<u8>> {
    use std::io::{Error, ErrorKind};

    let (w, h) = (width as usize, height as usize);
    if rgba.len() < w * h * 4 {
        return Err(Error::new(ErrorKind::InvalidInput, "framebuffer too small"));
    }

    // Raw image data: one filter byte (0 = None) per scanline + the RGBA row.
    let mut raw = Vec::with_capacity(h * (1 + w * 4));
    for y in 0..h {
        raw.push(0u8);
        let row = &rgba[y * w * 4..(y + 1) * w * 4];
        raw.extend_from_slice(row);
    }

    // zlib wrapper around STORED deflate blocks.
    let mut zlib = Vec::with_capacity(raw.len() + raw.len() / 65535 + 16);
    zlib.extend_from_slice(&[0x78, 0x01]); // zlib header (CM=8, no preset dict).
    let mut idx = 0usize;
    while idx < raw.len() {
        let chunk = (raw.len() - idx).min(0xFFFF);
        let last = idx + chunk >= raw.len();
        zlib.push(u8::from(last)); // BFINAL bit, BTYPE=00 (stored).
        let len = u16::try_from(chunk).unwrap_or(u16::MAX);
        zlib.extend_from_slice(&len.to_le_bytes());
        zlib.extend_from_slice(&(!len).to_le_bytes());
        zlib.extend_from_slice(&raw[idx..idx + chunk]);
        idx += chunk;
    }
    if raw.is_empty() {
        // Degenerate: emit a single empty final stored block.
        zlib.extend_from_slice(&[0x01, 0x00, 0x00, 0xFF, 0xFF]);
    }
    zlib.extend_from_slice(&png_adler32(&raw).to_be_bytes());

    let mut out = Vec::with_capacity(zlib.len() + 64);
    out.extend_from_slice(&[0x89, b'P', b'N', b'G', 0x0D, 0x0A, 0x1A, 0x0A]);

    // IHDR: width, height, bit depth 8, color type 6 (RGBA), no interlace.
    let mut ihdr = Vec::with_capacity(13);
    ihdr.extend_from_slice(&width.to_be_bytes());
    ihdr.extend_from_slice(&height.to_be_bytes());
    ihdr.extend_from_slice(&[8, 6, 0, 0, 0]);
    png_write_chunk(&mut out, *b"IHDR", &ihdr);
    png_write_chunk(&mut out, *b"IDAT", &zlib);
    png_write_chunk(&mut out, *b"IEND", &[]);
    Ok(out)
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
    /// v1.1.0 beta.1 (T-110-A1) — render-path staging copy of the PPU
    /// palette-index framebuffer (256x240 u16), snapshotted under the same brief
    /// lock as `present_staging` but only while the true composite `NES_NTSC`
    /// filter is active (empty otherwise = zero cost). Paired with
    /// `present_phase`.
    present_index_staging: Vec<u16>,
    /// v1.1.0 beta.1 (T-110-A1) — the per-frame NTSC colour phase (0..=2) that
    /// goes with `present_index_staging`.
    present_phase: u8,
    /// v2.1.2 F2.1 — the composed Vs. `DualSystem` two-screen image (main + sub
    /// arranged side-by-side / stacked), filled under the present lock and blitted
    /// via `Gfx::render_dual`. Empty unless a cabinet is loaded.
    #[cfg(not(target_arch = "wasm32"))]
    present_dual: Vec<u8>,
    /// v2.1.2 F2.1 — cached "a Vs. `DualSystem` cabinet is loaded" flag, set at
    /// load / cleared at close, so the per-redraw present path can branch without
    /// taking the emu lock just to check.
    dual_mode: bool,
    gfx: Option<Gfx>,
    /// v1.2.0 beta.2 (Workstream C3) — the active HD-pack compositor, `Some`
    /// only while a pack is loaded for the current ROM. `None` (the default,
    /// and the only state when no pack is configured) means the present path is
    /// byte-identical to the stock build.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    hd_compositor: Option<crate::hdpack::HdCompositor>,
    /// v1.7.0 "Forge" G5 — the active HD-Pack BUILDER recorder, `Some` only
    /// while the user is recording a starter pack. It observes the same per-frame
    /// tile-source + CHR snapshot the compositor consumes (output-only); `None`
    /// (the default) leaves the present path byte-identical to stock.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    hd_pack_builder: Option<crate::hdpack_builder::HdPackBuilder>,
    /// v1.2.0 C3 — scratch staging for the PPU per-pixel HD tile-source
    /// telemetry, copied under the emu lock alongside the framebuffer.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    present_hd_tiles: Vec<rustynes_core::rustynes_ppu::HdTileSource>,
    /// v1.2.0 C3 — snapshot of the 8 KiB PPU pattern space (`$0000..$2000`),
    /// copied under the emu lock so the CPU-heavy HD composite (upscale +
    /// tile-hash + blit) runs *after* the lock is dropped, honouring the
    /// frontend's "never hold the emu lock during heavy work" discipline.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    present_chr_snapshot: Vec<u8>,
    /// v1.8.9 — the frame's BG scroll `(x, y)` captured under the emu lock, used
    /// to offset parallax HD-pack `<background>` layers in the unlocked composite.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    present_bg_scroll: (i32, i32),
    /// v1.3.0 E1 — per-frame snapshot of the watched memory addresses
    /// referenced by the HD-pack's `<condition>` declarations (Mesen's
    /// `WatchedAddressValues`). Captured under the emu lock at produce time and
    /// read by the compositor after the lock drops, mirroring the CHR snapshot.
    /// Empty when the loaded pack uses no memory conditions.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    present_watched_mem: crate::hdpack::WatchedMemory,
    /// Native audio output (cpal). wasm32 uses the Web Audio path in
    /// `wasm.rs` instead, so this field is native-only.
    #[cfg(not(target_arch = "wasm32"))]
    audio: Option<AudioOutput>,
    input: InputState,
    config: Config,
    #[cfg(not(target_arch = "wasm32"))]
    data_dir: Option<PathBuf>,
    /// v1.5.0 "Lens" Workstream I1 — a **persistent** system clipboard handle
    /// for File -> Copy Screenshot to Clipboard. On X11 / Wayland the clipboard
    /// content is *owned by the live process*: the previous code created a
    /// throwaway `arboard::Clipboard`, called `set_image`, and dropped it
    /// immediately — so on Linux the image vanished the instant the function
    /// returned (the toast still claimed success). Holding the `Clipboard`
    /// instance for the whole session keeps the app available to serve paste
    /// requests, so the copied image actually reaches other applications.
    /// Lazily created on first use (a missing clipboard subsystem is reported,
    /// not fatal). Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    clipboard: Option<arboard::Clipboard>,
    /// Sprint 5-3 debugger overlay (lazily constructed alongside `Gfx`).
    debugger: Option<DebuggerOverlay>,
    /// v1.1.0 beta.3 (Workstream E) — the Lua scripting engine (native, behind
    /// the `scripting` feature). `None` until a script is loaded. Lives on the
    /// winit thread (mlua is `!Send`); pumped once per redraw under the emu lock.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    script: Option<rustynes_script::ScriptEngine>,
    /// Overlay draw commands the script issued this frame, rendered through the
    /// egui pass and refreshed on each pump.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    script_draws: Vec<rustynes_script::DrawCmd>,
    /// The `TasEditor` revision (or `None` for no editor) the last pushed
    /// `TasSnapshot` was built from. Lets `pump_scripts` skip the per-frame
    /// rebuild-and-clone of the whole editor when its state is unchanged — an
    /// idle `TAStudio` costs no allocation each frame (the engine keeps the
    /// prior snapshot). Reset when the editor's revision moves or it opens/closes.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    tas_snapshot_revision: Option<u64>,
    /// v1.7.0 "Forge" Workstream E1 — the host-mediated IPC bridge that OWNS the
    /// `comm.*` TCP / HTTP / WebSocket / memory-mapped-file connections. `None`
    /// until a script is loaded. The Lua sandbox never sees a socket: the engine
    /// queues marshalled `CommCmd`s (gated like `emu.write`), this host does the
    /// I/O off the emu lock, and feeds `CommResult`s back. See ADR 0016.
    #[cfg(all(feature = "script-ipc", not(target_arch = "wasm32")))]
    script_host: Option<crate::script_host::ScriptHost>,
    /// v1.2.0 Workstream F4 — the EXPERIMENTAL wasm Lua engine (piccolo, behind
    /// the `script-wasm` feature). Same shape as the native `script` field but
    /// over the `rustynes_script_wasm` (piccolo) backend; loaded from the
    /// browser via the `wasm_load_script` bridge. See ADR 0012.
    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
    script_wasm: Option<rustynes_script_wasm::ScriptEngine>,
    /// Overlay draw commands the wasm script issued this frame (egui pass).
    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
    script_draws_wasm: Vec<rustynes_script_wasm::DrawCmd>,
    /// v1.0.0 — the always-on desktop UX shell state (menu/status-bar
    /// visibility, settings tab, status toast, mirrored pause/fullscreen
    /// flags). The shell UI itself is built inside the debugger overlay's
    /// single egui pass each frame; this holds its persistent state.
    ui: crate::ui_shell::UiShell,
    /// v1.0.0 — the active save-state slot (0-7) selected from the File menu.
    /// `Save State` / `Load State` use this slot; the per-slot menu items
    /// (`Save to Slot` / `Load from Slot`) target an explicit slot instead.
    active_save_slot: u8,
    /// v1.0.0 — emulation-speed factor (transient, NOT persisted; always
    /// launches at 1.0). Mirrors `EmuCore::speed` (written through
    /// [`Self::set_speed`], which also centers the audio DRC band on the
    /// factor and re-resolves pacing). Read by the status bar / Speed menu.
    speed: f32,
    /// v1.0.0 — the Save-States manager window (thumbnail grid). Native-only
    /// (the slot files live on the filesystem). Surfaces the core's existing
    /// per-slot thumbnails; routes Save / Load through the existing handlers.
    #[cfg(not(target_arch = "wasm32"))]
    save_states_ui: crate::save_states_ui::SaveStatesUi,
    /// v1.8.9 — the desktop on-screen virtual pad (clickable controller for
    /// player 1). Native-only; its held mask folds into `frame_inputs`.
    #[cfg(not(target_arch = "wasm32"))]
    virtual_pad: crate::virtual_pad::VirtualPad,
    /// v1.0.0 — cached previous value of `config.ui.pixel_aspect_correction`,
    /// so a change made in the menu / settings window is detected after the
    /// egui pass and pushed into the gfx letterbox (mirrors the NTSC live-apply
    /// pattern).
    prev_par_correction: bool,
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
    /// `true` when emulation was auto-paused because the window lost focus
    /// (the `[ui] pause_on_focus_loss` `QoL`). Only an auto-pause is
    /// auto-resumed on regaining focus — a manual user pause is left alone.
    auto_paused: bool,
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
    /// v1.5.0 "Lens" Workstream H1 — lock-free triple-buffer framebuffer
    /// handoff. The emu thread publishes each produced frame into it; the
    /// winit present path takes the freshest frame without ever blocking on
    /// the emu mutex (`docs/frontend.md` "Lock discipline at present").
    /// Shared with the emu thread on `spawn`. Native-only + `emu-thread`.
    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
    present_buffer: std::sync::Arc<crate::present_buffer::PresentBuffer>,
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
    /// v1.2.0 Workstream D — whether the RIGHT mouse button is held (SNES mouse
    /// right button). Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    mouse_right_pressed: bool,
    /// v1.2.0 Workstream D — accumulated raw mouse motion since the last frame
    /// latch (`MouseMotion` deltas), consumed + reset by [`Self::frame_inputs`]
    /// when the SNES mouse is the active device. Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    mouse_motion_accum: (f64, f64),
    /// v1.2.0 Workstream D — live Family BASIC keyboard matrix bitmap (one byte
    /// per row), driven by host key events via [`crate::input::family_keyboard_index`].
    /// Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    family_keyboard: [u8; 9],
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
    ra: Option<rustynes_ra::RaSession>,
    /// v1.5.0 "Lens" Workstream G — EXPERIMENTAL casual-only browser
    /// `RetroAchievements` session (ADR 0015). wasm-only + behind the
    /// default-OFF `browser-cheevos` feature. Structurally casual-only (no
    /// hardcore API at any layer); the per-frame drive + UI caveat live here.
    /// `None` until the side module initializes (see `wasm_cheevos`).
    #[cfg(all(target_arch = "wasm32", feature = "browser-cheevos"))]
    browser_ra: Option<crate::wasm_cheevos::BrowserRaSession>,
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

/// v1.7.0 "Forge" G4 — the recomputed ROM digests stamped onto an exported TAS
/// movie so it is verifiable on `TASVideos`. Native-only (movie export is a
/// filesystem operation).
#[cfg(not(target_arch = "wasm32"))]
struct MovieRomHashes {
    /// Base64 of the 16 raw MD5 bytes (the `.fm2` `romChecksum` `base64:` value).
    md5_b64: String,
    /// Lower-case hex SHA-1 (the `.bk2` `SHA1` header value).
    sha1: String,
}

/// Standard (RFC 4648) base64-encode `bytes` with `=` padding. A tiny local
/// encoder so the native export path needs no extra base64 dependency (the wasm
/// `base64_encode` uses the browser's `btoa`, which is unavailable on native).
#[cfg(not(target_arch = "wasm32"))]
fn base64_std(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::with_capacity(bytes.len().div_ceil(3) * 4);
    for chunk in bytes.chunks(3) {
        let b0 = u32::from(chunk[0]);
        let b1 = chunk.get(1).copied().map_or(0, u32::from);
        let b2 = chunk.get(2).copied().map_or(0, u32::from);
        let n = (b0 << 16) | (b1 << 8) | b2;
        out.push(ALPHABET[((n >> 18) & 0x3F) as usize] as char);
        out.push(ALPHABET[((n >> 12) & 0x3F) as usize] as char);
        out.push(if chunk.len() > 1 {
            ALPHABET[((n >> 6) & 0x3F) as usize] as char
        } else {
            '='
        });
        out.push(if chunk.len() > 2 {
            ALPHABET[(n & 0x3F) as usize] as char
        } else {
            '='
        });
    }
    out
}

impl App {
    /// Build an app from a path to a `.nes` file (native).
    ///
    /// # Errors
    ///
    /// Returns an `io::Error` if the file can't be read.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn new(rom_path: &std::path::Path) -> std::io::Result<Self> {
        // The CLI / initial-ROM path must run the same `.zip` extraction +
        // same-stem soft-patching as `load_rom_from_path` (see the helper).
        let (rom_bytes, rom_label) = load_and_preprocess_rom(rom_path)?;
        let config = Config::load_or_default();
        let input = InputState::from_config(&config.input);
        let data_dir = Config::default_data_dir();
        let ui = crate::ui_shell::UiShell::new(&config);
        let prev_par_correction = config.ui.pixel_aspect_correction;
        Ok(Self {
            rom_bytes,
            rom_label,
            emu: crate::emu::EmuHandle::new(crate::emu::EmuCore::new()),
            present_staging: Vec::new(),
            present_index_staging: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            present_dual: Vec::new(),
            dual_mode: false,
            present_phase: 0,
            gfx: None,
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            hd_compositor: None,
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            hd_pack_builder: None,
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            present_hd_tiles: Vec::new(),
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            present_chr_snapshot: Vec::new(),
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            present_bg_scroll: (0, 0),
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            present_watched_mem: crate::hdpack::WatchedMemory::new(),
            audio: None,
            input,
            config,
            data_dir,
            #[cfg(not(target_arch = "wasm32"))]
            clipboard: None,
            debugger: None,
            #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
            script: None,
            #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
            script_draws: Vec::new(),
            #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
            tas_snapshot_revision: None,
            #[cfg(all(feature = "script-ipc", not(target_arch = "wasm32")))]
            script_host: None,
            #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
            script_wasm: None,
            #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
            script_draws_wasm: Vec::new(),
            ui,
            active_save_slot: 0,
            speed: 1.0,
            save_states_ui: crate::save_states_ui::SaveStatesUi::default(),
            virtual_pad: crate::virtual_pad::VirtualPad::default(),
            prev_par_correction,
            gamepad: gilrs::Gilrs::new()
                .map_err(|e| {
                    eprintln!("rustynes: gamepad subsystem disabled: {e}");
                })
                .ok(),
            should_exit: false,
            auto_paused: false,
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
            present_buffer: crate::present_buffer::PresentBuffer::new(),
            #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
            emu_proxy: None,
            netplay: crate::netplay_ui::NetplayUi::default(),
            cursor_pos: None,
            window_size: (NES_W * INITIAL_SCALE, NES_H * INITIAL_SCALE),
            mouse_pressed: false,
            #[cfg(not(target_arch = "wasm32"))]
            mouse_right_pressed: false,
            #[cfg(not(target_arch = "wasm32"))]
            mouse_motion_accum: (0.0, 0.0),
            #[cfg(not(target_arch = "wasm32"))]
            family_keyboard: [0; 9],
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
    fn init_ra_session() -> rustynes_ra::RaSession {
        // The config used here is loaded freshly so the helper is callable from
        // the struct literal (it cannot borrow `self.config`); the caller has
        // the same config in hand. Keeping it a free read keeps `new` simple.
        let config = Config::load_or_default();
        // v1.8.6 — map the frontend's `RetroAchievementsConfig` onto the
        // host-agnostic `rustynes_ra::RaConfig` (the `host` field is not part of
        // the session's config surface — rcheevos uses its default host).
        let ra_config = Self::ra_config_from(&config.retroachievements);
        let mut session = rustynes_ra::RaSession::new(&ra_config);
        // `auto_login` is a no-op unless `enabled` + a saved username/token.
        session.auto_login(&ra_config);
        session
    }

    /// Map the persisted frontend `RetroAchievementsConfig` onto the
    /// host-agnostic [`rustynes_ra::RaConfig`] the session consumes.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn ra_config_from(c: &crate::config::RetroAchievementsConfig) -> rustynes_ra::RaConfig {
        rustynes_ra::RaConfig {
            enabled: c.enabled,
            username: c.username.clone(),
            token: c.token.clone(),
            hardcore: c.hardcore,
        }
    }

    /// v1.3.0 Sprint 1.4 — build an empty app for wasm32 (no ROM
    /// yet; it arrives via the browser file picker as an
    /// [`AppEvent::RomLoaded`]). Config is the in-memory default
    /// (no filesystem on the web). The `proxy` is wired by `run`.
    #[cfg(target_arch = "wasm32")]
    #[must_use]
    pub fn new_empty(proxy: winit::event_loop::EventLoopProxy<AppEvent>) -> Self {
        // v1.7.0 "Forge" beta.5 Workstream H6 — apply any `?settings=` share-link
        // over the in-memory default (no filesystem on the web). Falls back to the
        // plain default when there is no/invalid blob.
        let config = crate::wasm_share::config_from_url_or_default();
        crate::wasm_share::publish_live(&config);
        let input = InputState::from_config(&config.input);
        let ui = crate::ui_shell::UiShell::new(&config);
        let prev_par_correction = config.ui.pixel_aspect_correction;
        Self {
            rom_bytes: Vec::new(),
            rom_label: "(no ROM)".to_string(),
            emu: crate::emu::EmuHandle::new(crate::emu::EmuCore::new()),
            present_staging: Vec::new(),
            present_index_staging: Vec::new(),
            #[cfg(not(target_arch = "wasm32"))]
            present_dual: Vec::new(),
            dual_mode: false,
            present_phase: 0,
            gfx: None,
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            hd_compositor: None,
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            hd_pack_builder: None,
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            present_hd_tiles: Vec::new(),
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            present_chr_snapshot: Vec::new(),
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            present_bg_scroll: (0, 0),
            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
            present_watched_mem: crate::hdpack::WatchedMemory::new(),
            input,
            config,
            debugger: None,
            #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
            script: None,
            #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
            script_draws: Vec::new(),
            #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
            tas_snapshot_revision: None,
            #[cfg(all(feature = "script-ipc", not(target_arch = "wasm32")))]
            script_host: None,
            #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
            script_wasm: None,
            #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
            script_draws_wasm: Vec::new(),
            ui,
            active_save_slot: 0,
            speed: 1.0,
            prev_par_correction,
            proxy: Some(proxy),
            should_exit: false,
            auto_paused: false,
            browser_netplay: None,
            wasm_lobby: crate::wasm_lobby::WasmLobbyState::default(),
            fds_bios_bytes: None,
            // v1.5.0 Workstream G — start with the casual-only browser RA session
            // created (un-initialized); `init` loads the side module lazily.
            #[cfg(feature = "browser-cheevos")]
            browser_ra: Some(crate::wasm_cheevos::BrowserRaSession::new()),
        }
    }

    fn create_window(&self, event_loop: &ActiveEventLoop) -> Result<Arc<Window>, String> {
        let attrs = Window::default_attributes()
            .with_title(format!("RustyNES - {}", self.rom_label))
            .with_inner_size(LogicalSize::new(
                NES_W * INITIAL_SCALE,
                NES_H * INITIAL_SCALE,
            ));

        // v1.1.0 — set the taskbar / title-bar icon from the embedded app icon
        // (native only; a browser tab has no window icon). `None` if decode
        // fails — the window just falls back to the platform default.
        #[cfg(not(target_arch = "wasm32"))]
        let attrs = attrs.with_window_icon(crate::icon::window_icon());

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
            .add_filter("NES / FDS / UNIF image", &["nes", "fds", "unf", "unif"])
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
        }
        // A Vs. DualSystem cart (two CPUs + two PPUs) cannot boot past the
        // attract screen on this single-system core. Surface a clear note
        // rather than leaving the user staring at a black screen. Full
        // two-system support is a documented future feature
        // (docs/audit/vs-dualsystem-design-2026-06-11.md). v1.3.0 D2: the note
        // now also fires for header-flagged DualSystem ROMs (NES 2.0 byte-13
        // high nibble), not only the SHA-256-DB-known dumps.
        if db_entry.is_some_and(|e| e.dual_system) || nes.is_vs_dual_system() {
            log_dual_system_note();
        }
        let dip = resolve_vs_dip(self.config.vs, db_entry);
        nes.set_vs_dip(dip);
    }

    /// v1.1.0 beta.1 (T-110-B4) — apply the per-game database's nametable
    /// mirroring override (a load-time fix for a wrong iNES mirroring flag),
    /// keyed on the ROM's CRC32. A no-op when the ROM is not listed (or not an
    /// iNES image — e.g. FDS), so the default path is byte-identical. The core
    /// test suites never call this, so `AccuracyCoin` / the oracle are unaffected.
    fn apply_game_db(nes: &mut Nes, bytes: &[u8]) {
        if let Some(crc) = crate::game_db::rom_crc32(bytes)
            && let Some(m) = crate::game_db::mirroring_for_crc(crc)
            // A game-DB mirroring correction is only valid for a hardwired-
            // mirroring board. Force-applying it to a mapper that controls its
            // own mirroring (MMC1/3/5, AxROM, VRC, …) corrupts rendering — e.g.
            // Wizards & Warriors (AxROM), whose DB row's spurious `Horizontal`
            // blanked the status-bar split and hung the game. Skip it there.
            && nes.mapper_has_hardwired_mirroring()
        {
            nes.set_mirroring_override(Some(m));
        }
    }

    /// Replace the current ROM with the one at `path`. Reuses the
    /// existing audio queue and rebuilds the `Nes` (and rewind ring if
    /// enabled) against the new cartridge. On any error, the old `Nes`
    /// is preserved so the running session isn't lost.
    ///
    /// Native-only (filesystem). On wasm32 the ROM arrives as
    /// [`AppEvent::RomLoaded`] from the browser file picker.
    /// v1.3.0 — close the current ROM: tear down the `Nes` and return to the
    /// no-ROM state (the inverse of the install in [`Self::load_rom_from_path`]).
    /// The menu gates this behind a loaded ROM + no active netplay session.
    fn close_rom(&mut self) {
        {
            let mut guard = self.emu.lock();
            let emu = &mut *guard;
            emu.nes = None;
            emu.dual = None;
            emu.perf.clear();
            emu.present_fb.clear();
            emu.present_fb_sub.clear();
            emu.audio_buf.clear();
            emu.next_frame_time = None;
        }
        self.dual_mode = false;
        // v1.6.0 "Studio" A2 — a TAStudio session anchors on the closed ROM; end it.
        if let Some(d) = self.debugger.as_mut() {
            d.clear_tas_editor();
        }
        // Stop the dedicated emulation thread from producing frames.
        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
        if let Some(thread) = self.emu_thread.as_ref() {
            thread.control().set_has_rom(false);
        }
        self.rom_label = String::new();
        self.rom_bytes = Vec::new();
        // v1.7.1 — present a clean BLANK screen on close, not a frozen last
        // frame and never an empty slice. Reset the lock-free triple buffer so
        // `has_published()` reports "no frame" (the present path then falls
        // through to the zeroed-staging branch instead of re-presenting a
        // stale slot), and replace `present_staging` with a valid zeroed
        // `NES_W*NES_H*4` frame so the next upload uploads black, not nothing.
        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
        self.present_buffer.reset();
        self.present_staging.clear();
        self.present_staging.resize((NES_W * NES_H * 4) as usize, 0);
        // Drop the sibling presentation buffers too, so a stale frame can't be
        // re-presented after the ROM is closed.
        self.present_index_staging.clear();
        #[cfg(feature = "hd-pack")]
        {
            self.present_hd_tiles.clear();
            self.present_chr_snapshot.clear();
        }
        self.ui
            .set_status(crate::ui_shell::StatusMessage::info("ROM closed"));
    }

    #[cfg(not(target_arch = "wasm32"))]
    #[allow(clippy::too_many_lines)] // sequential per-format load + device/cheat/DB setup
    fn load_rom_from_path(&mut self, path: &Path) {
        let mut bytes = match std::fs::read(path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("rustynes: failed to read {}: {e}", path.display());
                // (audit m3) surface the failure in the status bar (a Recent-ROM
                // whose file vanished is the common case).
                self.ui.set_status(crate::ui_shell::StatusMessage::new(
                    format!("Failed to open ROM: {e}"),
                    egui::Color32::from_rgb(230, 90, 90),
                    std::time::Duration::from_secs(4),
                ));
                return;
            }
        };
        // v1.2.0 Workstream B — load a ROM straight out of a `.zip` archive:
        // extract the first NES / FDS / NSF entry, then continue exactly as for
        // a bare file (soft-patching + the deterministic parse see the extracted
        // image). A same-stem patch beside the `.zip` still resolves below.
        if path
            .extension()
            .is_some_and(|e| e.eq_ignore_ascii_case("zip"))
        {
            if let Some((name, rom)) = extract_rom_from_zip(&bytes) {
                bytes = rom;
                self.ui.set_status(crate::ui_shell::StatusMessage::new(
                    format!("Loaded {name} from archive"),
                    egui::Color32::from_rgb(120, 200, 120),
                    std::time::Duration::from_secs(3),
                ));
            } else {
                eprintln!("rustynes: no NES/FDS/NSF entry in {}", path.display());
                self.ui.set_status(crate::ui_shell::StatusMessage::new(
                    "No ROM found inside the .zip archive".to_string(),
                    egui::Color32::from_rgb(230, 90, 90),
                    std::time::Duration::from_secs(4),
                ));
                return;
            }
        }
        // v1.2.0 Workstream B — auto-apply a same-stem soft-patch sitting beside
        // the ROM (`.bps`/`.ups`/`.ips`, in that precedence), BEFORE any format
        // detection, so the patched image flows through the deterministic parse
        // unchanged (save-states / netplay / oracle all see the patched bytes).
        // Only the highest-precedence patch present is applied; a malformed
        // patch is surfaced and the unpatched ROM still loads.
        for ext in ["bps", "ups", "ips"] {
            let patch_path = path.with_extension(ext);
            if patch_path == path {
                continue;
            }
            let Ok(patch_bytes) = std::fs::read(&patch_path) else {
                continue;
            };
            match crate::patch::detect_and_apply(&bytes, &patch_bytes, ext) {
                Ok(patched) => {
                    bytes = patched;
                    self.ui.set_status(crate::ui_shell::StatusMessage::new(
                        format!("Applied {ext} patch: {}", patch_path.display()),
                        egui::Color32::from_rgb(120, 200, 120),
                        std::time::Duration::from_secs(3),
                    ));
                }
                Err(e) => {
                    eprintln!("rustynes: {ext} patch {} failed: {e}", patch_path.display());
                    self.ui.set_status(crate::ui_shell::StatusMessage::new(
                        format!("Patch failed ({ext}): {e}"),
                        egui::Color32::from_rgb(230, 90, 90),
                        std::time::Duration::from_secs(4),
                    ));
                }
            }
            break;
        }
        // v1.2.0 Workstream B — apply the per-game DB's region / mapper /
        // submapper corrections by rewriting the iNES header BEFORE the core
        // parses it (keyed on the header-excluded CRC32, so stable across the
        // rewrite). Mirroring / Vs. corrections apply post-construction below.
        // Frontend-only: the core test suites never patch, so the oracle is
        // byte-identical.
        if let Some(crc) = crate::game_db::rom_crc32(&bytes)
            && let Some(entry) = crate::game_db::entry_for_crc(crc)
        {
            crate::game_db::apply_header_overrides(&mut bytes, &entry);
        }
        // v1.7.0 "Forge" Workstream H4 — resolve the per-game `<rom>.json`
        // overlay (config-dir overlay wins over a sibling, mirroring the v1.2.0
        // game-DB user-overlay precedence), keyed on the SAME header-excluded
        // CRC32. Its load-time `overrides` rewrite the iNES header through the
        // SAME `apply_header_overrides` path (so they stack on the game-DB
        // corrections and the CRC key stays stable). Its mirroring / DIP are
        // applied post-construction below. Absent / inert file ⇒ no-op ⇒
        // byte-identical. Frontend-only — the core never reads it.
        let rom_crc = crate::game_db::rom_crc32(&bytes);
        let per_game = rom_crc.and_then(|crc| crate::per_game::resolve(crc, Some(path)));
        if let Some(cfg) = per_game.as_ref()
            && !cfg.overrides.is_empty()
            && let Some(crc) = rom_crc
        {
            let entry = cfg.overrides.to_game_db_entry(crc, String::new());
            crate::game_db::apply_header_overrides(&mut bytes, &entry);
        }
        let sample_rate = self.audio.as_ref().map_or(44_100, |a| a.sample_rate);
        // v2.2.0 — a Famicom Disk System `.fds` image needs the disksys.rom
        // BIOS + the writable-disk save path; the standard cartridge `.nes`
        // path is unchanged. Detect by the disk-image magic (never matches a
        // `"NES\x1A"` cartridge).
        let mut nes = if is_nsf_image(&bytes) {
            // v1.1.0 beta.2 — NSF music file: no cartridge, no CHR; a synthetic
            // driver runs init/play through the standard lockstep loop.
            self.emu.lock().fds_disk_sha256 = None;
            match Nes::from_nsf_with_sample_rate(&bytes, sample_rate) {
                Ok(n) => n,
                Err(e) => {
                    eprintln!("rustynes: failed to load NSF {}: {e}", path.display());
                    self.ui.set_status(crate::ui_shell::StatusMessage::new(
                        format!("Failed to load NSF: {e}"),
                        egui::Color32::from_rgb(230, 90, 90),
                        std::time::Duration::from_secs(4),
                    ));
                    return;
                }
            }
        } else if is_fds_image(&bytes) {
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
                    self.ui.set_status(crate::ui_shell::StatusMessage::new(
                        format!("Failed to load ROM: {e}"),
                        egui::Color32::from_rgb(230, 90, 90),
                        std::time::Duration::from_secs(4),
                    ));
                    return;
                }
            }
        };
        // v2.1.2 F2.1 — Vs. `DualSystem` cabinet detection (native, cartridge
        // ROMs only). When the probe `nes` above is a DualSystem board, build the
        // real two-console cabinet from the same bytes and apply the Vs.-DB DIP +
        // RGB palette to BOTH consoles; the probe `nes` becomes a discarded
        // throwaway (its single-console post-setup below is harmless). The install
        // block routes this into `EmuCore::dual`. On wasm (no dual present path) a
        // DualSystem ROM keeps running as the single main-console probe.
        #[cfg(not(target_arch = "wasm32"))]
        let dual_cabinet: Option<Box<rustynes_core::VsDualSystem>> = if !is_nsf_image(&bytes)
            && !is_fds_image(&bytes)
            && (nes.is_vs_dual_system()
                || rustynes_core::vs_db::lookup(nes.rom_sha256()).is_some_and(|e| e.dual_system))
        {
            match rustynes_core::VsDualSystem::from_rom_with_sample_rate(&bytes, sample_rate) {
                Ok(mut vs) => {
                    self.apply_vs_db(vs.main_mut());
                    self.apply_vs_db(vs.sub_mut());
                    Some(Box::new(vs))
                }
                Err(e) => {
                    eprintln!("rustynes: Vs. DualSystem build failed, running main-only: {e}");
                    None
                }
            }
        } else {
            None
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
        Self::apply_game_db(&mut nes, &bytes);
        // v1.7.0 "Forge" Workstream H4 — apply the per-game overlay's
        // post-construction settings, layered ON TOP of the game-DB / Vs.-DB
        // results above so the per-game file has the final say: an explicit
        // mirroring override wins over the game-DB mirroring, and an explicit
        // DIP value wins over the `[vs] dip` / Vs.-DB precedence. Both flow
        // through the same core setters the game-DB editor uses; a no-op for a
        // non-Vs. cart / absent override, so the default path is byte-identical.
        if let Some(cfg) = per_game.as_ref() {
            if let Some(m) = cfg
                .overrides
                .mirroring
                .as_deref()
                .and_then(crate::per_game::mirroring_from_token)
            {
                // Same hazard as the game-DB path: a static mirroring override is
                // only valid for a hardwired-mirroring board. Honor an explicit
                // per-game override there; on a mapper-controlled board, decline
                // it (with a note) so a stray value can't corrupt rendering.
                if nes.mapper_has_hardwired_mirroring() {
                    nes.set_mirroring_override(Some(m));
                } else {
                    eprintln!(
                        "rustynes: ignoring per-game mirroring override ({m:?}) — this \
                         mapper controls its own mirroring; a static override would \
                         corrupt rendering"
                    );
                }
            }
            if let Some(dip) = cfg.dip_switches {
                nes.set_vs_dip(dip);
            }
        }
        // v1.2.0 (B4) — let the ROM-database editor key its overlay on this ROM.
        let rom_crc = crate::game_db::rom_crc32(&bytes);
        if let Some(debugger) = self.debugger.as_mut() {
            debugger.set_rom_crc(rom_crc);
        }

        self.rom_label = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("rom.nes")
            .to_string();
        self.rom_bytes = bytes;
        // v2.1.2 F2.1 — cache the dual flag for the per-redraw present branch.
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.dual_mode = dual_cabinet.is_some();
        }
        {
            let mut guard = self.emu.lock();
            let emu = &mut *guard;
            emu.frame_duration = nes.frame_duration();
            emu.next_frame_time = Some(Instant::now() + emu.frame_duration);
            emu.audio_buf.clear();
            emu.perf.clear();
            emu.present_fb.clear();
            emu.present_fb_sub.clear();
            // v1.7.0 "Forge" D1 — a new ROM starts a fresh session timeline.
            emu.history.clear();
            // v1.7.0 "Forge" H4 — a new ROM starts a fresh lag-frame tally.
            emu.reset_lag_frames();
            // v2.1.2 F2.1 — install a Vs. `DualSystem` cabinet into `dual`
            // (mutually exclusive with `nes`), else the single console. The probe
            // `nes` is dropped in the dual case.
            #[cfg(not(target_arch = "wasm32"))]
            if let Some(vs) = dual_cabinet {
                emu.nes = None;
                emu.dual = Some(vs);
            } else {
                emu.dual = None;
                emu.nes = Some(nes);
            }
            #[cfg(target_arch = "wasm32")]
            {
                emu.nes = Some(nes);
            }
        }
        // v1.6.0 "Studio" A2 — the new ROM invalidates any TAStudio session
        // (it anchored on the previous game); end it so the editor can't
        // replay inputs/branches against a different `Nes`.
        if let Some(d) = self.debugger.as_mut() {
            d.clear_tas_editor();
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
        // v1.2.0 C3 — auto-load a configured HD-pack for this ROM (no-op when
        // none is configured, so the default presentation is byte-identical).
        #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
        self.maybe_load_hd_pack_for_rom();
        // v1.0.0 — re-push the per-APU-channel mute mask (the fresh `Nes` booted
        // with the all-on default). Default mask 0x3F = byte-identical audio.
        self.apply_apu_channel_mask();
        // v1.4.0 Workstream C — re-push the per-APU-channel output gain (the fresh
        // `Nes` booted at unity). Default (all 1.0) = byte-identical audio.
        self.apply_apu_channel_gain();
        self.apply_apu_filter_model();
        // v1.4.0 Workstream C — refresh the Settings panel's expansion-audio chip
        // label so the expansion-channel volume slider matches the loaded mapper.
        self.refresh_expansion_audio_chip();
        // v1.1.0 beta.1 / v1.5.0 D1 — re-apply the active palette (named bank
        // entry, else legacy .pal / built-in) onto the fresh Nes (booted with
        // the built-in palette). None / unselected = byte-identical.
        self.apply_active_palette();
        // v2.7.0 — (re)identify the ROM with RetroAchievements + load its saved
        // progress sidecar. No-op when no RA session is active.
        #[cfg(feature = "retroachievements")]
        self.load_ra_game();
        // v2.1.0 — attach the configured non-standard input device (if any) on
        // the player-2 port. No-op when ExpansionDevice::None.
        #[cfg(not(target_arch = "wasm32"))]
        self.sync_expansion_device();
        // v1.1.0 beta.2 — for an NSF music file, feed the header metadata to the
        // NSF player panel and pop it open (the framebuffer is blank, so the
        // panel is the primary UI).
        if is_nsf_image(&self.rom_bytes) {
            let (title, artist, copyright) = nsf_header_strings(&self.rom_bytes);
            if let Some(d) = self.debugger.as_mut() {
                d.set_nsf_metadata(title, artist, copyright);
                d.open_chip_panel(crate::debugger::ChipPanel::Nsf);
            }
        }
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window
                .set_title(&format!("RustyNES - {}", self.rom_label));
        }
        // v1.0.0 — record the ROM in the File -> Recent MRU list and surface a
        // status toast. Resuming from a user pause is intentional: loading a ROM
        // is an explicit "start playing this" gesture.
        self.config.recent_roms.add(path.to_path_buf());
        let _ = self.config.save();
        if self.ui.paused {
            self.set_paused(false);
        }
        self.ui
            .set_status(crate::ui_shell::StatusMessage::success(format!(
                "Loaded: {}",
                self.rom_label
            )));
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
            if entry.enabled
                && let Err(e) = nes.add_genie_code(&entry.code)
            {
                eprintln!("rustynes: cheat {} skipped: {e}", entry.code);
            }
        }
        // v1.7.0 — prime the per-frame raw-cheat list from this ROM's enabled
        // entries so they apply from frame 1 (before the panel's first pull).
        emu.raw_cheats = loaded.raw.iter().filter(|c| c.enabled).cloned().collect();
        if let Some(debugger) = self.debugger.as_mut() {
            debugger.set_cheat_persist(dir.clone(), rom_sha256, loaded.genie, loaded.raw);
        }
    }

    /// v1.2.0 beta.2 (Workstream C3) — load the HD-pack configured for the
    /// current ROM (if any) into a compositor. No-op when no entry is configured
    /// for the loaded ROM's hash — so the default presentation is byte-identical.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    fn maybe_load_hd_pack_for_rom(&mut self) {
        self.hd_compositor = None;
        let key = {
            let guard = self.emu.lock();
            let Some(nes) = guard.nes.as_ref() else {
                return;
            };
            crate::save_state::hex_sha256(nes.rom_sha256())
        };
        let Some(path) = self.config.graphics.hd_packs.get(&key).cloned() else {
            return;
        };
        self.load_hd_pack_from_path(&path);
    }

    /// v1.2.0 C3 — open a folder/zip picker, load the HD-pack, and persist the
    /// per-ROM mapping. Native + feature-gated.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    fn load_hd_pack_dialog(&mut self) {
        let has_rom = self.emu.lock().nes.is_some();
        if !has_rom {
            self.ui.set_status(crate::ui_shell::StatusMessage::new(
                "Load a ROM before loading an HD pack".to_string(),
                egui::Color32::from_rgb(230, 180, 90),
                std::time::Duration::from_secs(4),
            ));
            return;
        }
        // Accept either a pack folder (containing hires.txt) or a .zip archive.
        let picked = rfd::FileDialog::new()
            .set_title("Select HD-pack folder or .zip")
            .add_filter("HD pack archive", &["zip"])
            .pick_folder()
            .or_else(|| {
                rfd::FileDialog::new()
                    .set_title("Select HD-pack .zip")
                    .add_filter("HD pack archive", &["zip"])
                    .pick_file()
            });
        let Some(path) = picked else {
            return;
        };
        if self.load_hd_pack_from_path(&path) {
            // Persist the per-ROM mapping.
            let key = {
                let guard = self.emu.lock();
                guard
                    .nes
                    .as_ref()
                    .map(|n| crate::save_state::hex_sha256(n.rom_sha256()))
            };
            if let Some(key) = key {
                self.config.graphics.hd_packs.insert(key, path);
                let _ = self.config.save();
            }
        }
    }

    /// v1.2.0 C3 — load a pack from an explicit path into the compositor and
    /// surface the result. Returns `true` on success.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    fn load_hd_pack_from_path(&mut self, path: &Path) -> bool {
        if let Some(pack) = crate::hdpack::HdPack::load(path) {
            let rules = pack.rule_count();
            let scale = pack.scale();
            // v1.6.0 H — HD audio: decode the pack's `<bgm>`/`<sfx>` OGG tracks
            // (folder packs only — a `.zip` pack's audio is a future extension)
            // and install a frontend-side mixer on the shared `EmuCore`. The
            // mixer is an OUTPUT-ONLY tap; `None` (no audio decls / a zip pack /
            // all tracks failed to decode) leaves the audio path byte-identical.
            let audio_tracks = if path.is_dir() {
                let rate = self.audio.as_ref().map_or(44_100, |a| a.sample_rate);
                crate::hd_audio::decode_tracks_from_folder(path, pack.audio_decls(), rate)
            } else {
                Vec::new()
            };
            let track_count = audio_tracks.len();
            {
                let rate = self.audio.as_ref().map_or(44_100, |a| a.sample_rate);
                let mut guard = self.emu.lock();
                guard.hd_audio = crate::hd_audio::HdAudioMixer::new(audio_tracks, rate);
            }
            self.hd_compositor = Some(crate::hdpack::HdCompositor::new(pack));
            let audio_note = if track_count > 0 {
                format!(", {track_count} audio tracks")
            } else {
                String::new()
            };
            self.ui
                .set_status(crate::ui_shell::StatusMessage::success(format!(
                    "HD pack loaded: {rules} tiles, {scale}x{audio_note}"
                )));
            true
        } else {
            self.hd_compositor = None;
            self.ui.set_status(crate::ui_shell::StatusMessage::new(
                "No usable hires.txt rules found in HD pack".to_string(),
                egui::Color32::from_rgb(230, 90, 90),
                std::time::Duration::from_secs(4),
            ));
            false
        }
    }

    /// v1.2.0 C3 — unload the active HD-pack and drop the per-ROM mapping.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    fn unload_hd_pack(&mut self) {
        self.hd_compositor = None;
        let key = {
            let mut guard = self.emu.lock();
            // v1.6.0 H — drop the HD-audio mixer so the audio path returns to
            // byte-identical stock output.
            guard.hd_audio = None;
            guard
                .nes
                .as_ref()
                .map(|n| crate::save_state::hex_sha256(n.rom_sha256()))
        };
        if let Some(key) = key
            && self.config.graphics.hd_packs.remove(&key).is_some()
        {
            let _ = self.config.save();
        }
        self.ui.set_status(crate::ui_shell::StatusMessage::success(
            "HD pack unloaded".to_string(),
        ));
    }

    /// v1.7.0 "Forge" G5 — start recording an HD-pack STARTER pack from live
    /// play. The recorder observes the per-frame tile-source + CHR snapshot in
    /// the present path (output-only) and accumulates the distinct tiles the
    /// game draws. No-op if already recording or no ROM is loaded.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    fn hd_pack_builder_start(&mut self) {
        if self.hd_pack_builder.is_some() {
            return;
        }
        if self.emu.lock().nes.is_none() {
            self.ui.set_status(crate::ui_shell::StatusMessage::error(
                "Load a ROM before building an HD pack".to_string(),
            ));
            return;
        }
        self.hd_pack_builder = Some(crate::hdpack_builder::HdPackBuilder::new());
        self.ui.set_status(crate::ui_shell::StatusMessage::info(
            "HD-Pack Builder recording — play through the scenes you want to capture".to_string(),
        ));
    }

    /// v1.7.0 "Forge" G5 — stop the HD-Pack Builder and write the captured
    /// `hires.txt` + `tiles.png` starter pack to a folder the user picks. No-op
    /// if not recording.
    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
    fn hd_pack_builder_stop(&mut self) {
        let Some(builder) = self.hd_pack_builder.take() else {
            return;
        };
        if builder.tile_count() == 0 {
            self.ui.set_status(crate::ui_shell::StatusMessage::error(
                "HD-Pack Builder captured no tiles (was anything rendered?)".to_string(),
            ));
            return;
        }
        let mut dialog = rfd::FileDialog::new().set_file_name("MyHdPack");
        if let Some(d) = self.data_dir.clone() {
            dialog = dialog.set_directory(d);
        }
        let Some(dir) = dialog.pick_folder() else {
            self.ui.set_status(crate::ui_shell::StatusMessage::info(
                "HD-Pack Builder save cancelled".to_string(),
            ));
            return;
        };
        match builder.write_pack(&dir) {
            Ok(n) => self
                .ui
                .set_status(crate::ui_shell::StatusMessage::success(format!(
                    "HD pack written: {n} tile(s) -> {}",
                    dir.display()
                ))),
            Err(e) => self
                .ui
                .set_status(crate::ui_shell::StatusMessage::error(format!(
                    "HD-Pack Builder write failed: {e}"
                ))),
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
                    eprintln!(
                        "rustynes: FDS save corrupt ({e}); falling back to the pristine disk"
                    );
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

    /// v1.8.9 — insert a specific FDS disk side (`Some(i)`) or eject (`None`) — the
    /// Multi-Disk submenu's direct counterpart to [`Self::cycle_disk_side`] (a
    /// multi-disk game prompts "insert side N"). An out-of-range side is ignored.
    fn set_disk_side(&self, side: Option<usize>) {
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
        if let Some(s) = side
            && s >= count
        {
            return; // stale menu click past the side count.
        }
        nes.set_disk_side(side);
        match side {
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
        // v1.0.0 — surface controller hot-plug as a status toast. The events
        // were previously consumed silently; collect any to report after the
        // drain so the toast set can borrow `self.ui` without the gilrs borrow.
        let mut hotplug: Option<&'static str> = None;
        while let Some(ev) = gilrs.next_event() {
            match ev.event {
                gilrs::EventType::Connected => hotplug = Some("Controller connected"),
                gilrs::EventType::Disconnected => hotplug = Some("Controller disconnected"),
                _ => {}
            }
            self.input.handle_gamepad_event(ev.id, &ev.event);
            // If the input rebind modal is listening for a pad button,
            // feed the event there so it can capture the binding.
            if let Some(debugger) = self.debugger.as_mut() {
                debugger.maybe_capture_gamepad(&ev.event);
            }
        }
        if let Some(msg) = hotplug {
            self.ui
                .set_status(crate::ui_shell::StatusMessage::info(msg));
        }
    }

    /// Save state to a filesystem slot. Native-only; wasm32 uses the
    /// `localStorage` path in `wasm.rs` (F1).
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_save_state(&self, slot: u8) {
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
        match save_state::save_to_slot(dir, &rom_sha256, slot, &blob) {
            Ok(path) => eprintln!("rustynes: saved state -> {}", path.display()),
            Err(e) => eprintln!("rustynes: save state failed: {e}"),
        }
    }

    /// Load state from a filesystem slot. Native-only (see
    /// [`Self::handle_save_state`]).
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_load_state(&self, slot: u8) {
        // Read the ROM key under a short lock; the file read runs with it
        // dropped; the restore takes a second short lock.
        let Some(rom_sha256) = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256()) else {
            return;
        };
        let Some(dir) = self.data_dir.as_ref() else {
            eprintln!("rustynes: no data directory available; load state skipped");
            return;
        };
        match save_state::load_from_slot(dir, &rom_sha256, slot) {
            Ok(blob) => {
                let mut guard = self.emu.lock();
                let Some(nes) = guard.nes.as_mut() else {
                    return;
                };
                match nes.restore(&blob) {
                    Ok(()) => eprintln!("rustynes: loaded state from slot {slot}"),
                    Err(e) => eprintln!("rustynes: restore failed: {e}"),
                }
            }
            Err(e) => eprintln!("rustynes: load state failed: {e}"),
        }
    }

    /// v1.0.0 — capture the current framebuffer to a PNG under
    /// `<data_dir>/screenshots/<rom>-<utc>.png` and toast the path. Native-only
    /// (the wasm build has no filesystem; the menu item is gated out there).
    #[cfg(not(target_arch = "wasm32"))]
    fn take_screenshot(&mut self) {
        use crate::ui_shell::StatusMessage;
        // Copy the framebuffer under a brief lock; the encode + write run with
        // the guard dropped.
        let frame = {
            let guard = self.emu.lock();
            guard.nes.as_ref().map(|nes| nes.framebuffer().to_vec())
        };
        let Some(frame) = frame else {
            self.ui
                .set_status(StatusMessage::info("Screenshot: no ROM loaded"));
            return;
        };
        let Some(dir) = self.data_dir.as_ref() else {
            self.ui
                .set_status(StatusMessage::info("Screenshot: no data directory"));
            return;
        };
        let shots = dir.join("screenshots");
        if let Err(e) = std::fs::create_dir_all(&shots) {
            eprintln!("rustynes: screenshot dir create failed: {e}");
            self.ui
                .set_status(StatusMessage::info("Screenshot failed (mkdir)"));
            return;
        }
        // Build a filesystem-safe stem from the ROM label + a UTC timestamp.
        let stem: String = self
            .rom_label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let path = shots.join(format!("{stem}-{secs}.png"));
        match encode_png_rgba(&frame, NES_W, NES_H) {
            Ok(png) => match std::fs::write(&path, png) {
                Ok(()) => {
                    eprintln!("rustynes: screenshot -> {}", path.display());
                    self.ui.set_status(StatusMessage::success(format!(
                        "Screenshot saved: {}",
                        path.display()
                    )));
                }
                Err(e) => {
                    eprintln!("rustynes: screenshot write failed: {e}");
                    self.ui
                        .set_status(StatusMessage::info("Screenshot failed (write)"));
                }
            },
            Err(e) => {
                eprintln!("rustynes: screenshot encode failed: {e}");
                self.ui
                    .set_status(StatusMessage::info("Screenshot failed (encode)"));
            }
        }
    }

    /// v1.0.0 — copy the current framebuffer to the system clipboard as a raw
    /// RGBA8 image (256x240, the NES native resolution), in addition to the
    /// save-to-PNG path. Native-only (the wasm build has no `arboard` / system
    /// clipboard for images; the menu item is gated out there). Reuses the same
    /// brief-lock framebuffer grab as [`Self::take_screenshot`]. The `arboard`
    /// error path is handled with a toast — it never panics.
    #[cfg(not(target_arch = "wasm32"))]
    fn screenshot_to_clipboard(&mut self) {
        use crate::ui_shell::StatusMessage;
        // Copy the framebuffer under a brief lock; the clipboard set runs with
        // the guard dropped.
        let frame = {
            let guard = self.emu.lock();
            guard.nes.as_ref().map(|nes| nes.framebuffer().to_vec())
        };
        let Some(frame) = frame else {
            self.ui
                .set_status(StatusMessage::info("Clipboard: no ROM loaded"));
            return;
        };
        // `arboard` wants the bytes as `Cow<[u8]>`; the framebuffer is already
        // tightly-packed RGBA8 (256 * 240 * 4 = NES_W * NES_H * 4).
        let expected = NES_W as usize * NES_H as usize * 4;
        if frame.len() != expected {
            eprintln!(
                "rustynes: clipboard screenshot: unexpected framebuffer size {} (want {expected})",
                frame.len()
            );
            self.ui
                .set_status(StatusMessage::info("Clipboard failed (bad frame)"));
            return;
        }
        let image = arboard::ImageData {
            width: NES_W as usize,
            height: NES_H as usize,
            bytes: std::borrow::Cow::Owned(frame),
        };
        // v1.5.0 I1 — use the PERSISTENT clipboard handle (lazily created). On
        // X11 / Wayland the clipboard content lives in the owning process, so a
        // throwaway `Clipboard` dropped at the end of this function would clear
        // the image immediately (the original no-op bug). The session-long
        // `App.clipboard` keeps us available to serve paste requests.
        if self.clipboard.is_none() {
            match arboard::Clipboard::new() {
                Ok(cb) => self.clipboard = Some(cb),
                Err(e) => {
                    eprintln!("rustynes: clipboard unavailable: {e}");
                    self.ui
                        .set_status(StatusMessage::info("Clipboard unavailable"));
                    return;
                }
            }
        }
        // SAFETY of the unwrap: just ensured `Some` above (or returned).
        let cb = self
            .clipboard
            .as_mut()
            .expect("clipboard initialized above");
        match cb.set_image(image) {
            Ok(()) => {
                self.ui
                    .set_status(StatusMessage::success("Screenshot copied to clipboard"));
            }
            Err(e) => {
                eprintln!("rustynes: clipboard screenshot failed: {e}");
                self.ui
                    .set_status(StatusMessage::info("Clipboard copy failed"));
            }
        }
    }

    /// Per-ROM movies directory (`<data_dir>/movies/`). Created lazily on
    /// first save. Returns `None` if no data dir is available.
    #[cfg(not(target_arch = "wasm32"))]
    fn movies_dir(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("movies"))
    }

    /// v1.6.0 "Studio" Workstream G — A/V recordings directory
    /// (`<data_dir>/recordings/`). Returns `None` if no data dir is available.
    #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
    fn recordings_dir(&self) -> Option<PathBuf> {
        self.data_dir.as_ref().map(|d| d.join("recordings"))
    }

    /// v1.6.0 "Studio" Workstream G — whether an A/V recording is currently
    /// armed. Always `false` on wasm or a build without the `av-record`
    /// feature (so the menu item / status reporting compile target-agnostically).
    #[must_use]
    #[cfg_attr(
        not(all(not(target_arch = "wasm32"), feature = "av-record")),
        allow(clippy::unused_self, clippy::missing_const_for_fn)
    )]
    fn av_recording_active(&self) -> bool {
        #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
        let active = self.emu.lock().av_recorder.is_some();
        #[cfg(not(all(not(target_arch = "wasm32"), feature = "av-record")))]
        let active = false;
        active
    }

    /// v1.6.0 "Studio" Workstream G — toggle A/V recording (native).
    ///
    /// **Start**: open a `.mp4` / `.mkv` save dialog, then arm an
    /// [`crate::av_record::AvRecorder`] (an `ffmpeg`-piped video + audio
    /// recorder) keyed to the loaded ROM's region frame rate + the audio device
    /// sample rate. If `ffmpeg` is absent the arm fails gracefully with a toast
    /// and emulation continues untouched. **Stop**: finalize the in-progress
    /// recording (flush, close the pipe, wait for `ffmpeg` to mux) and toast the
    /// output path. The recorder is a READ-ONLY tap on the produced framebuffer
    /// + drained audio — it never advances the emulator, so determinism holds.
    #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
    fn handle_av_record_toggle(&mut self) {
        use crate::av_record::{AvParams, AvRecorder};
        use crate::ui_shell::StatusMessage;

        // Stop path: take the recorder out under a brief lock, then finalize
        // with the guard dropped (the ffmpeg wait can block).
        if self.av_recording_active() {
            let recorder = self.emu.lock().av_recorder.take();
            if let Some(recorder) = recorder {
                let frames = recorder.frames();
                match recorder.stop() {
                    Ok(path) => {
                        eprintln!(
                            "rustynes: A/V recording -> {} ({frames} frames)",
                            path.display()
                        );
                        self.ui.set_status(StatusMessage::success(format!(
                            "A/V recording saved: {}",
                            path.display()
                        )));
                    }
                    Err(e) => {
                        eprintln!("rustynes: A/V recording finalize failed: {e}");
                        self.ui
                            .set_status(StatusMessage::info("A/V recording failed (encode)"));
                    }
                }
            }
            return;
        }

        // Start path: need a loaded ROM. Capture the region frame duration +
        // sample rate under a brief lock; the dialog + spawn run unlocked.
        let frame_nanos = {
            let guard = self.emu.lock();
            if guard.nes.is_none() {
                drop(guard);
                self.ui
                    .set_status(StatusMessage::info("A/V recording: no ROM loaded"));
                return;
            }
            guard.frame_duration.as_nanos()
        };
        // Exact rational fps = 1e9 / frame_nanos (drift-free over a long take).
        let fps_num: u32 = 1_000_000_000;
        let fps_den: u32 = u32::try_from(frame_nanos).unwrap_or(16_639_267);
        let sample_rate = self.audio.as_ref().map_or(44_100, |a| a.sample_rate);

        let dir = self.recordings_dir();
        if let Some(d) = dir.as_ref() {
            let _ = std::fs::create_dir_all(d);
        }
        // Filesystem-safe default name from the ROM label + a UTC timestamp.
        let stem: String = self
            .rom_label
            .chars()
            .map(|c| if c.is_ascii_alphanumeric() { c } else { '_' })
            .collect();
        let secs = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_or(0, |d| d.as_secs());
        let mut dialog = rfd::FileDialog::new()
            .add_filter("Video (MP4/MKV)", &["mp4", "mkv"])
            .set_file_name(format!("{stem}-{secs}.mp4"));
        if let Some(d) = dir {
            dialog = dialog.set_directory(d);
        }
        let Some(out_path) = dialog.save_file() else {
            eprintln!("rustynes: A/V recording cancelled");
            return;
        };

        // Codec-depth options (encoder / CRF / preset / audio bitrate). The
        // v1.8.9 — read the persisted codec-depth picker (Settings -> Recording).
        // Unknown ids fall back to the defaults (H.264 / CRF 18 / veryfast / 192k),
        // reproducing the prior fixed pipeline.
        let rec = &self.config.recording;
        let opts = crate::av_record::AvRecordOptions::from_parts(
            &rec.video_codec,
            rec.crf,
            &rec.preset,
            rec.audio_bitrate_k,
        );
        let params = AvParams {
            out_path: out_path.clone(),
            sample_rate,
            fps_num,
            fps_den,
            video_codec: opts.video_codec,
            crf: opts.crf,
            preset: opts.preset,
            audio_bitrate_k: opts.audio_bitrate_k,
        };
        match AvRecorder::start(params) {
            Ok(recorder) => {
                self.emu.lock().av_recorder = Some(recorder);
                eprintln!("rustynes: A/V recording armed -> {}", out_path.display());
                self.ui.set_status(StatusMessage::success(format!(
                    "Recording A/V to {}",
                    out_path.display()
                )));
            }
            Err(e) => {
                eprintln!("rustynes: A/V recording could not start: {e}");
                self.ui.set_status(StatusMessage::info(
                    "A/V recording unavailable (no ffmpeg?)",
                ));
            }
        }
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
        // ADR 0028: a movie recorded before the v2.0.0 one-clock timebase
        // promote still plays as pure input replay, but its bit-identical
        // reproduction guarantee is unverified across the engine boundary.
        if rustynes_core::recorded_before_v2_timebase(&bytes).is_ok_and(|v| v) {
            eprintln!(
                "rustynes: warning: {} was recorded on a pre-v2.0.0 build \
                 -- input replay proceeds, but exact framebuffer/audio \
                 reproduction is not guaranteed across the engine-timebase \
                 boundary (see ADR 0028)",
                path.display()
            );
        }
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

    /// v1.6.0 B1 — import an external TAS movie (FCEUX `.fm2` / `BizHawk` `.bk2`)
    /// and begin playback against the running ROM.
    ///
    /// `.fm2` is flat text; `.bk2` is a ZIP whose `Header.txt` + `Input Log.txt`
    /// members are read out and handed to the `no_std` core parser. Both produce
    /// a [`StartPoint::PowerOn`](rustynes_core::StartPoint) movie that replays
    /// from the **canonical movie-import power-on alignment** (a deterministic
    /// zeroed-RAM cold boot via `seek_to_start`), so imports never desync. The
    /// running ROM's SHA-256 is stamped onto the imported movie as its
    /// authoritative identity (the external formats carry only MD5 / SHA-1).
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_movie_import(&self) {
        if self.netplay.is_active() {
            eprintln!("rustynes: leave netplay before importing a movie");
            return;
        }
        let Some(path) = rfd::FileDialog::new()
            .add_filter("TAS movie", &["fm2", "bk2", "fcm", "fmv", "vmv"])
            .set_directory(self.movies_dir().unwrap_or_else(|| PathBuf::from(".")))
            .pick_file()
        else {
            return;
        };
        // The imported movie inherits the running ROM's identity, so a ROM must
        // be loaded first. We grab the hash under a short lock, then parse +
        // play under a second lock (the file read + parse hold no lock).
        let rom_sha = {
            let guard = self.emu.lock();
            let Some(nes) = guard.nes.as_ref() else {
                eprintln!("rustynes: movie import: no ROM loaded");
                return;
            };
            *nes.rom_sha256()
        };
        let movie = match Self::parse_movie_file(&path, rom_sha) {
            Ok(m) => m,
            Err(e) => {
                eprintln!("rustynes: movie import failed {}: {e}", path.display());
                return;
            }
        };
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let Some(nes) = emu.nes.as_mut() else {
            return;
        };
        if let Err(e) = movie.seek_to_start(nes) {
            eprintln!("rustynes: movie import seek failed (wrong ROM?): {e}");
            return;
        }
        let total = movie.len();
        emu.movie.start_playback(movie);
        emu.next_frame_time = Some(Instant::now());
        eprintln!(
            "rustynes: imported movie playing ({total} frames) from {}",
            path.display()
        );
    }

    /// Parse a `.fm2` / `.bk2` movie file into a [`Movie`], stamping `rom_sha` as
    /// its identity. `.bk2` ZIP handling stays frontend-side (the core stays
    /// `no_std`); the text members are parsed by the core.
    #[cfg(not(target_arch = "wasm32"))]
    fn parse_movie_file(
        path: &std::path::Path,
        rom_sha: [u8; 32],
    ) -> Result<rustynes_core::Movie, String> {
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        match ext.as_str() {
            "bk2" => {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let (header, input_log) = Self::read_bk2_members(&bytes)?;
                let (movie, _meta) =
                    rustynes_core::bk2_interop::import_bk2(&header, &input_log, rom_sha)
                        .map_err(|e| e.to_string())?;
                Ok(movie)
            }
            // v1.7.0 "Forge" G4 — legacy binary TAS containers. Each is a small
            // binary header + per-frame input; the core parses bytes -> Movie and
            // reuses the canonical power-on alignment (so replay is bit-identical).
            "fcm" | "fmv" | "vmv" | "mc2" => {
                let bytes = std::fs::read(path).map_err(|e| e.to_string())?;
                let (movie, _meta) = match ext.as_str() {
                    "fcm" => rustynes_core::import_fcm(&bytes, rom_sha),
                    "fmv" => rustynes_core::import_fmv(&bytes, rom_sha),
                    "vmv" => rustynes_core::import_vmv(&bytes, rom_sha),
                    // `.mc2` is a PC Engine format, not NES — rejected cleanly.
                    _ => rustynes_core::import_mc2(&bytes, rom_sha),
                }
                .map_err(|e| e.to_string())?;
                Ok(movie)
            }
            // `.fm2` (or any other extension): flat text.
            _ => {
                let text = std::fs::read_to_string(path).map_err(|e| e.to_string())?;
                let (movie, _meta) = rustynes_core::movie_interop::import_fm2(&text, rom_sha)
                    .map_err(|e| e.to_string())?;
                Ok(movie)
            }
        }
    }

    /// Read the `Header.txt` + `Input Log.txt` members out of a `.bk2` ZIP.
    #[cfg(not(target_arch = "wasm32"))]
    fn read_bk2_members(zip_bytes: &[u8]) -> Result<(String, String), String> {
        use std::io::Read as _;
        // A `.bk2`'s text members are small; cap the read so a corrupt / hostile
        // archive can't OOM the host (same guard as the ROM zip path).
        const MAX_MEMBER_BYTES: u64 = 64 * 1024 * 1024;
        let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes))
            .map_err(|e| format!("not a valid .bk2 zip: {e}"))?;
        let read_member =
            |archive: &mut zip::ZipArchive<_>, name: &str| -> Result<String, String> {
                let f = archive
                    .by_name(name)
                    .map_err(|_| format!(".bk2 missing `{name}`"))?;
                if f.size() > MAX_MEMBER_BYTES {
                    return Err(format!(".bk2 member `{name}` is implausibly large"));
                }
                let mut s = String::new();
                f.take(MAX_MEMBER_BYTES)
                    .read_to_string(&mut s)
                    .map_err(|e| format!(".bk2 member `{name}` read failed: {e}"))?;
                Ok(s)
            };
        let header = read_member(&mut archive, rustynes_core::bk2_interop::BK2_HEADER_MEMBER)?;
        let input_log = read_member(
            &mut archive,
            rustynes_core::bk2_interop::BK2_INPUT_LOG_MEMBER,
        )?;
        Ok((header, input_log))
    }

    /// v1.6.0 B1 / v1.7.0 G4 — export the current recording / loaded movie to an
    /// external TAS movie file. The chosen extension (`.fm2` / `.bk2`) selects the
    /// format; `.bk2` is packed into a ZIP frontend-side. The matching ROM
    /// checksum (MD5 for `.fm2`, SHA-1 for `.bk2`) is recomputed and stamped on so
    /// the movie is verifiable on `TASVideos`.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_movie_export(&self) {
        // Prefer an open, non-empty TAStudio edit — it carries the TAS
        // re-record count (and is the most likely thing a user means by
        // "export" while the piano-roll is up). Else the in-progress recording
        // (finished here), else the loaded playback movie.
        let movie = {
            let tas_movie = self
                .debugger
                .as_ref()
                .and_then(crate::debugger::DebuggerOverlay::tas_editor)
                .filter(|ed| !ed.is_empty())
                .and_then(|ed| {
                    let guard = self.emu.lock();
                    guard
                        .nes
                        .as_ref()
                        .map(|nes| ed.to_movie(nes.region(), *nes.rom_sha256()))
                });
            tas_movie.or_else(|| {
                let mut guard = self.emu.lock();
                guard
                    .movie
                    .finish_recording()
                    .or_else(|| guard.movie.playing_movie())
            })
        };
        let Some(movie) = movie else {
            eprintln!("rustynes: movie export: nothing to export");
            return;
        };
        let dir = self.movies_dir();
        if let Some(d) = dir.as_ref() {
            let _ = std::fs::create_dir_all(d);
        }
        let mut dialog = rfd::FileDialog::new()
            .add_filter("TAS movie (FCEUX/BizHawk)", &["fm2", "bk2"])
            .set_file_name("movie.bk2");
        if let Some(d) = dir {
            dialog = dialog.set_directory(d);
        }
        let Some(path) = dialog.save_file() else {
            eprintln!("rustynes: movie export cancelled");
            return;
        };
        // v1.7.0 "Forge" G4 — recompute the ROM digests the interchange formats
        // verify against (`.fm2` wants an MD5 `romChecksum`, `.bk2` a SHA-1) from
        // the loaded ROM, so an exported RustyNES movie is checkable on TASVideos.
        let hashes = self.movie_rom_hashes();
        if let Err(e) = Self::write_movie_file(&path, &movie, hashes.as_ref()) {
            eprintln!("rustynes: movie export failed {}: {e}", path.display());
        } else {
            eprintln!(
                "rustynes: exported movie ({} frames) -> {}",
                movie.len(),
                path.display()
            );
        }
    }

    /// v1.7.0 "Forge" Workstream H9 — export the current `TAStudio` movie's
    /// markers as a `SubRip` (`.srt`) subtitle track, frame-exact at the
    /// region's frame rate, for muxing into an `A/V` dump. Requires the
    /// `TAStudio` editor to be open with at least one named marker; otherwise it
    /// reports and returns.
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_movie_export_subtitles(&mut self) {
        use crate::ui_shell::StatusMessage;
        let markers: Vec<(u64, String)> = self
            .debugger
            .as_ref()
            .and_then(crate::debugger::DebuggerOverlay::tas_editor)
            .map(|editor| {
                editor
                    .markers()
                    .map(|(f, l)| (f as u64, l.to_owned()))
                    .collect()
            })
            .unwrap_or_default();
        if markers.iter().all(|(_, l)| l.trim().is_empty()) {
            self.ui.set_status(StatusMessage::info(
                "Subtitle export: no TAStudio markers to export",
            ));
            return;
        }

        // The region's exact rational frame rate (the A/V recorder convention:
        // fps = 1e9 / frame_nanos). `movie_srt` consumes (num, den) directly.
        let frame_nanos = self.emu.lock().frame_duration.as_nanos();
        let fps_num: u32 = 1_000_000_000;
        let fps_den: u32 = u32::try_from(frame_nanos).unwrap_or(16_639_267);
        // A ~2s tail for the final marker's cue.
        let tail_frames = u64::from(fps_num) / u64::from(fps_den.max(1)) * 2;
        let srt = crate::movie_srt::markers_to_srt(markers, fps_num, fps_den, tail_frames.max(1));

        let dir = self.movies_dir();
        if let Some(d) = dir.as_ref() {
            let _ = std::fs::create_dir_all(d);
        }
        let mut dialog = rfd::FileDialog::new()
            .add_filter("SubRip subtitles", &["srt"])
            .set_file_name("movie.srt");
        if let Some(d) = dir {
            dialog = dialog.set_directory(d);
        }
        let Some(path) = dialog.save_file() else {
            eprintln!("rustynes: subtitle export cancelled");
            return;
        };
        match std::fs::write(&path, srt) {
            Ok(()) => {
                self.ui.set_status(StatusMessage::success(format!(
                    "Subtitles -> {}",
                    path.display()
                )));
            }
            Err(e) => {
                self.ui
                    .set_status(StatusMessage::error(format!("Subtitle export failed: {e}")));
            }
        }
    }

    /// v1.7.0 "Forge" G4 — recompute the legacy/interchange ROM digests from the
    /// loaded ROM bytes for movie export: the MD5 a `.fm2` `romChecksum` uses and
    /// the SHA-1 a `.bk2` `SHA1` header uses. Returns `None` when no ROM is
    /// loaded. Both are rendered as the lower-case hex the formats expect.
    #[cfg(not(target_arch = "wasm32"))]
    fn movie_rom_hashes(&self) -> Option<MovieRomHashes> {
        use md5::Md5;
        use sha1::Sha1;
        // md-5 / sha1 are on the RustCrypto 0.11 `digest` trait family; pull
        // `Digest` from one of them (NOT `sha2`, which the core keeps on 0.10 for
        // its no_std Sha256) so the `Md5::digest` / `Sha1::digest` calls resolve.
        use sha1::Digest as _;
        if self.rom_bytes.is_empty() {
            return None;
        }
        // `.fm2`'s `romChecksum` convention is `base64:<base64 of the 16 raw MD5
        // bytes>`; `.bk2`'s `SHA1` is lower-case hex.
        let md5_b64 = base64_std(Md5::digest(&self.rom_bytes).as_slice());
        let sha1 = hex::encode(Sha1::digest(&self.rom_bytes));
        Some(MovieRomHashes { md5_b64, sha1 })
    }

    /// v1.7.0 "Forge" Workstream D1 — export the trailing `seconds` of the live
    /// session timeline (the `HistoryViewer` over the rewind ring) as a replayable
    /// `.rnm` clip. The clip starts at the nearest start-anchor at-or-before the
    /// window and replays bit-identically (its `StartPoint` is a real
    /// save-state + the recorded input stream).
    #[cfg(not(target_arch = "wasm32"))]
    fn handle_history_export_clip(&self, seconds: f64) {
        let movie = {
            let guard = self.emu.lock();
            // The region frame rate (NTSC ~60, PAL/Dendy ~50) for seconds->frames.
            let fps = guard.nes.as_ref().map_or(60.0, |nes| {
                let d = nes.frame_duration().as_secs_f64();
                if d > 0.0 { 1.0 / d } else { 60.0 }
            });
            guard.history.export_last_seconds(seconds, fps)
        };
        let movie = match movie {
            Ok(m) => m,
            Err(e) => {
                eprintln!("rustynes: history clip export: {e}");
                return;
            }
        };
        let dir = self.movies_dir();
        if let Some(d) = dir.as_ref() {
            let _ = std::fs::create_dir_all(d);
        }
        let mut dialog = rfd::FileDialog::new()
            .add_filter("RustyNES movie", &["rnm"])
            .set_file_name("clip.rnm");
        if let Some(d) = dir {
            dialog = dialog.set_directory(d);
        }
        let Some(path) = dialog.save_file() else {
            eprintln!("rustynes: history clip export cancelled");
            return;
        };
        match std::fs::write(&path, movie.serialize()) {
            Ok(()) => eprintln!(
                "rustynes: exported {:.0}s history clip ({} frames) -> {}",
                seconds,
                movie.len(),
                path.display()
            ),
            Err(e) => eprintln!(
                "rustynes: history clip export failed {}: {e}",
                path.display()
            ),
        }
    }

    /// Serialize `movie` to a `.fm2` / `.bk2` file (extension selects the
    /// format). `.bk2` packs the core's two text members into a ZIP. When `hashes`
    /// is supplied, the export carries the matching ROM checksum so the movie is
    /// verifiable on `TASVideos`.
    #[cfg(not(target_arch = "wasm32"))]
    fn write_movie_file(
        path: &std::path::Path,
        movie: &rustynes_core::Movie,
        hashes: Option<&MovieRomHashes>,
    ) -> Result<(), String> {
        // The export file's stem doubles as the ROM/game name the formats record.
        let name = path
            .file_stem()
            .and_then(|s| s.to_str())
            .map(std::string::ToString::to_string);
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .unwrap_or("")
            .to_ascii_lowercase();
        if ext == "fm2" {
            let opts = rustynes_core::movie_interop::Fm2ExportOpts {
                rom_filename: name.map(|n| format!("{n}.nes")),
                rom_checksum_md5: hashes.map(|h| format!("base64:{}", h.md5_b64)),
                rerecord_count: u64::from(movie.rerecord_count),
                ..Default::default()
            };
            let text = rustynes_core::movie_interop::export_fm2(movie, &opts)
                .map_err(|e| e.to_string())?;
            std::fs::write(path, text).map_err(|e| e.to_string())
        } else {
            // `.bk2` (the default): pack the two text members into a ZIP.
            let opts = rustynes_core::bk2_interop::Bk2ExportOpts {
                game_name: name,
                sha1: hashes.map(|h| h.sha1.clone()),
                rerecord_count: u64::from(movie.rerecord_count),
                ..Default::default()
            };
            let parts =
                rustynes_core::bk2_interop::export_bk2(movie, &opts).map_err(|e| e.to_string())?;
            let bytes = Self::pack_bk2_zip(&parts)?;
            std::fs::write(path, bytes).map_err(|e| e.to_string())
        }
    }

    /// Pack a `.bk2`'s `Header.txt` + `Input Log.txt` text members into a ZIP.
    #[cfg(not(target_arch = "wasm32"))]
    fn pack_bk2_zip(parts: &rustynes_core::bk2_interop::Bk2Text) -> Result<Vec<u8>, String> {
        use std::io::Write as _;
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            w.start_file(rustynes_core::bk2_interop::BK2_HEADER_MEMBER, opts)
                .map_err(|e| e.to_string())?;
            w.write_all(parts.header.as_bytes())
                .map_err(|e| e.to_string())?;
            w.start_file(rustynes_core::bk2_interop::BK2_INPUT_LOG_MEMBER, opts)
                .map_err(|e| e.to_string())?;
            w.write_all(parts.input_log.as_bytes())
                .map_err(|e| e.to_string())?;
            w.finish().map_err(|e| e.to_string())?;
        }
        Ok(buf)
    }

    /// v1.6.0 "Studio" A2 — lazily start a `TAStudio` session when the window is
    /// first opened on a loaded ROM. The current emulator state becomes the
    /// project's frame-0 anchor (non-destructive: we do not power-cycle the
    /// running game). No-op if a session is already active or no ROM is loaded.
    fn ensure_tas_editor(&mut self) {
        use crate::debugger::DebuggerOverlay;
        // Greenzone budget + keyframe spacing for the editing session. 256 MiB
        // holds a generous run of save-states; the editor LRU-evicts past it.
        const TAS_GREENZONE_BUDGET: usize = 256 << 20;
        const TAS_CAPTURE_INTERVAL: usize = 16;

        if self
            .debugger
            .as_ref()
            .is_none_or(DebuggerOverlay::tas_active)
        {
            return; // no overlay to host it, or a session already exists
        }
        let editor = {
            let guard = self.emu.lock();
            let Some(nes) = guard.nes.as_ref() else {
                return; // no ROM loaded — nothing to anchor on
            };
            crate::tastudio::TasEditor::new(nes, TAS_GREENZONE_BUDGET, TAS_CAPTURE_INTERVAL)
        };
        if let Some(d) = self.debugger.as_mut() {
            d.set_tas_editor(editor);
        }
    }

    /// v1.6.0 "Studio" A2 — apply the `TAStudio` panel's queued requests after the
    /// egui pass. Edits/seeks touch the `TasEditor` (and, for seek / branch, the
    /// `Nes`) under the emu lock; seeking re-derives state by replaying inputs,
    /// so determinism is preserved (no new non-deterministic surface).
    fn handle_tas_requests(&mut self) {
        use crate::debugger::TasRequest;
        let reqs = match self.debugger.as_mut() {
            Some(d) => d.take_tas_requests(),
            None => return,
        };
        if reqs.is_empty() {
            return;
        }
        // First pass, no lock: the `.rnmproj` file ops open a native dialog
        // (and are no-ops on wasm). Everything else is an editor/`Nes` edit —
        // collect those so we take the emu lock ONCE for the whole batch
        // (drag-paint emits many `SetInput`s per frame; per-request locking
        // would thrash the mutex, mirroring how replay-seek holds it once).
        let mut edits = Vec::with_capacity(reqs.len());
        for req in reqs {
            match req {
                TasRequest::SaveProject => {
                    #[cfg(not(target_arch = "wasm32"))]
                    self.save_tas_project();
                }
                TasRequest::LoadProject => {
                    #[cfg(not(target_arch = "wasm32"))]
                    self.load_tas_project();
                }
                edit => edits.push(edit),
            }
        }
        if edits.is_empty() {
            return;
        }
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let Some(nes) = emu.nes.as_mut() else { return };
        let Some(ed) = self
            .debugger
            .as_mut()
            .and_then(DebuggerOverlay::tas_editor_mut)
        else {
            return;
        };
        for edit in edits {
            match edit {
                TasRequest::Seek(f) => ed.seek(nes, f),
                TasRequest::SetInput { frame, input } => {
                    ed.set_input(frame, input);
                }
                TasRequest::SetMarker { frame, label } => ed.set_marker(frame, label),
                TasRequest::RemoveMarker(f) => ed.remove_marker(f),
                TasRequest::InsertFrame(f) => ed.insert_frame(f),
                TasRequest::DeleteFrame(f) => ed.delete_frame(f),
                TasRequest::CreateBranch => {
                    ed.create_branch(nes);
                }
                TasRequest::LoadBranch(i) => {
                    ed.load_branch(i, nes);
                }
                TasRequest::DeleteBranch(i) => ed.delete_branch(i),
                TasRequest::StampMacro { start, frames } => {
                    ed.stamp_macro(start, &frames);
                }
                // Handled in the first pass (outside the lock — they open dialogs).
                TasRequest::SaveProject | TasRequest::LoadProject => {}
            }
        }
    }

    /// v1.6.0 "Studio" A2 — write the active `TAStudio` project to a chosen
    /// `.rnmproj` file (native file dialog).
    #[cfg(not(target_arch = "wasm32"))]
    fn save_tas_project(&mut self) {
        use crate::debugger::DebuggerOverlay;
        let Some(bytes) = self
            .debugger
            .as_mut()
            .and_then(DebuggerOverlay::tas_editor_mut)
            .map(|e| e.to_rnmproj())
        else {
            return;
        };
        let Some(path) = rfd::FileDialog::new()
            .add_filter("TAStudio project", &["rnmproj"])
            .set_file_name("project.rnmproj")
            .save_file()
        else {
            return;
        };
        if let Err(e) = std::fs::write(&path, &bytes) {
            eprintln!(
                "rustynes: TAStudio project save failed {}: {e}",
                path.display()
            );
        }
    }

    /// v1.6.0 "Studio" A2 — load a `.rnmproj` project into the active session.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_tas_project(&mut self) {
        use crate::debugger::DebuggerOverlay;
        let Some(path) = rfd::FileDialog::new()
            .add_filter("TAStudio project", &["rnmproj"])
            .pick_file()
        else {
            return;
        };
        let bytes = match std::fs::read(&path) {
            Ok(b) => b,
            Err(e) => {
                eprintln!(
                    "rustynes: TAStudio project read failed {}: {e}",
                    path.display()
                );
                return;
            }
        };
        if let Some(e) = self
            .debugger
            .as_mut()
            .and_then(DebuggerOverlay::tas_editor_mut)
            && let Err(err) = e.load_rnmproj(&bytes)
        {
            eprintln!("rustynes: TAStudio project parse failed: {err}");
        }
    }

    /// v1.5.0 "Lens" Workstream C2 — dispatch a Replay / TAS window action.
    ///
    /// Record / Play / Branch reuse the existing F6/F7/F8 menu handlers (so the
    /// window and the shortcuts share one code path). Stop maps to the active
    /// mode's toggle (stop-recording goes through the save path; stop-playback
    /// releases live input). Seek deterministically re-derives playback state by
    /// replaying the recorded inputs (`MovieUi::seek_playback`) under the emu
    /// lock — replay stays bit-identical, no new determinism surface.
    fn handle_replay_request(
        &mut self,
        req: crate::debugger::ReplayRequest,
        event_loop: &ActiveEventLoop,
    ) {
        use crate::debugger::ReplayRequest;
        match req {
            ReplayRequest::RecordToggle => {
                self.dispatch_menu_action(
                    crate::ui_shell::MenuAction::MovieRecordToggle,
                    event_loop,
                );
            }
            ReplayRequest::PlayToggle => {
                self.dispatch_menu_action(crate::ui_shell::MenuAction::MoviePlayToggle, event_loop);
            }
            ReplayRequest::Branch => {
                self.dispatch_menu_action(crate::ui_shell::MenuAction::MovieBranch, event_loop);
            }
            ReplayRequest::Stop => {
                let (recording, playing) = {
                    let emu = self.emu.lock();
                    (emu.movie.is_recording(), emu.movie.is_playing())
                };
                if recording {
                    // Route through the record toggle so the recording is saved
                    // rather than silently dropped.
                    self.dispatch_menu_action(
                        crate::ui_shell::MenuAction::MovieRecordToggle,
                        event_loop,
                    );
                } else if playing {
                    self.emu.lock().movie.stop_playback();
                }
            }
            ReplayRequest::Seek(target) => {
                let mut guard = self.emu.lock();
                let emu = &mut *guard;
                // Split the borrows: `nes` and `movie` are distinct fields.
                let seeked = match emu.nes.as_mut() {
                    Some(nes) => emu.movie.seek_playback(nes, target),
                    None => false,
                };
                if seeked {
                    // The seek re-derived emulator state; restart the frame
                    // clock so the next replayed frame is due now.
                    emu.next_frame_time = Some(Instant::now());
                }
            }
        }
    }

    /// v1.6.0 Sprint 4 / v1.4.0 E2 — F1 save state to `IndexedDB` (wasm32).
    ///
    /// The browser counterpart of [`Self::handle_save_state`]: serialize the
    /// running `Nes` and persist it to `IndexedDB` keyed by the ROM SHA-256
    /// and the active slot (binary, no base64 bloat). Falls back to
    /// `localStorage` if `IndexedDB` is unavailable. The snapshot is captured
    /// synchronously (under the lock); the async IDB write is then driven on
    /// the microtask queue via `spawn_local`, so the hotkey handler returns
    /// immediately and never holds the emu lock across an `.await`.
    /// No-op if no ROM is loaded.
    #[cfg(target_arch = "wasm32")]
    fn handle_save_state_wasm(&self, slot: u8) {
        let (sha, blob) = {
            let guard = self.emu.lock();
            let Some(nes) = guard.nes.as_ref() else {
                crate::wasm_io::log("save state: no ROM loaded");
                return;
            };
            (*nes.rom_sha256(), nes.snapshot())
        };
        wasm_bindgen_futures::spawn_local(async move {
            crate::wasm_idb::put_state(sha, slot, blob).await;
        });
    }

    /// v1.6.0 Sprint 4 / v1.4.0 E2 — F4 load state from `IndexedDB` (wasm32).
    ///
    /// The browser counterpart of [`Self::handle_load_state`]: read the
    /// per-ROM active-slot blob back from `IndexedDB` (or migrate it in from
    /// `localStorage`) and restore the `Nes`. The async read runs on a
    /// `spawn_local` task that re-locks the emu (a `Clone` `EmuHandle`) AFTER
    /// the `.await` resolves — never holding the lock across the await — and
    /// guards against the user swapping ROMs mid-read by re-checking the SHA.
    #[cfg(target_arch = "wasm32")]
    fn handle_load_state_wasm(&self, slot: u8) {
        let Some(sha) = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256()) else {
            crate::wasm_io::log("load state: no ROM loaded");
            return;
        };
        let emu = self.emu.clone();
        wasm_bindgen_futures::spawn_local(async move {
            let Some(blob) = crate::wasm_idb::get_state(sha, slot).await else {
                crate::wasm_io::log(&format!("load state: no saved state in slot {}", slot + 1));
                return;
            };
            let mut guard = emu.lock();
            let Some(nes) = guard.nes.as_mut() else {
                return;
            };
            // The ROM may have been swapped while the async read was in
            // flight; only restore if it is still the same game.
            if *nes.rom_sha256() != sha {
                crate::wasm_io::log("load state: ROM changed during load — skipped");
                return;
            }
            match nes.restore(&blob) {
                Ok(()) => crate::wasm_io::log("state loaded"),
                Err(e) => crate::wasm_io::log(&format!("load state: restore failed: {e:?}")),
            }
        });
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
            let byte_len = bytes.len();
            // v1.7.0 "Forge" beta.5 Workstream H6 — save through the File System
            // Access API (a real "Save As" dialog) where the browser supports
            // it, falling back to the synthetic-anchor download elsewhere.
            // `bytes` is moved in (owned), avoiding an extra copy.
            crate::wasm_io::save_file_with_fallback(
                "rustynes-movie.rnm",
                "RustyNES TAS movie",
                ".rnm",
                "application/octet-stream",
                bytes,
            );
            crate::wasm_io::log(&format!(
                "movie finished ({} frames, {byte_len} bytes) — save triggered",
                movie.len(),
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
        // ADR 0028: see the desktop movie-load path's identical warning.
        if rustynes_core::recorded_before_v2_timebase(bytes).is_ok_and(|v| v) {
            crate::wasm_io::log(
                "warning: this movie was recorded on a pre-v2.0.0 build -- \
                 input replay proceeds, but exact framebuffer/audio \
                 reproduction is not guaranteed across the engine-timebase \
                 boundary (see ADR 0028)",
            );
        }
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

    /// v1.5.0 "Lens" Workstream C2 — build the read-only port-topology +
    /// timebase snapshot for the Replay / TAS window from the host config +
    /// the cartridge `region`. Frontend-only; never touches the replay path.
    const fn build_replay_info(
        &self,
        region: rustynes_core::Region,
    ) -> crate::movie_ui::ReplayInfo {
        use crate::config::ExpansionDevice;
        let (region_label, region_hz) = match region {
            rustynes_core::Region::Pal => ("PAL", 50),
            rustynes_core::Region::Dendy => ("Dendy", 50),
            rustynes_core::Region::Ntsc => ("NTSC", 60),
        };
        let port2 = match self.config.input.expansion_device {
            ExpansionDevice::None => "Standard pad",
            ExpansionDevice::Zapper => "Zapper",
            ExpansionDevice::Vaus => "Arkanoid Vaus",
            ExpansionDevice::PowerPad => "Power Pad",
            ExpansionDevice::SnesMouse => "SNES mouse",
            ExpansionDevice::FamilyKeyboard => "Family BASIC keyboard",
            ExpansionDevice::FamilyTrainer => "Family Trainer mat",
            ExpansionDevice::SuborKeyboard => "Subor keyboard",
            ExpansionDevice::KonamiHyperShot => "Konami Hyper Shot",
            ExpansionDevice::BandaiHyperShot => "Bandai Hyper Shot",
        };
        crate::movie_ui::ReplayInfo {
            region: region_label,
            region_hz,
            port1: "Standard pad",
            port2,
            four_score: self.config.input.four_score,
        }
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
                ExpansionDevice::PowerPad => {
                    nes.set_power_pad(1, 0);
                }
                ExpansionDevice::SnesMouse => {
                    nes.set_snes_mouse(1, 0, 0, false, false, 0);
                }
                ExpansionDevice::FamilyKeyboard => {
                    nes.set_family_keyboard(1, [0; 9]);
                }
                ExpansionDevice::FamilyTrainer => {
                    nes.set_family_trainer(1, 0);
                }
                ExpansionDevice::SuborKeyboard => {
                    nes.set_subor_keyboard(1, [0; 9]);
                }
                ExpansionDevice::KonamiHyperShot => {
                    nes.set_konami_hyper_shot(1, 0);
                }
                ExpansionDevice::BandaiHyperShot => {
                    nes.set_bandai_hyper_shot(1, 0);
                }
            }
        }
    }

    /// Advance the emulator by exactly one frame. Netplay (native UDP /
    /// browser WebRTC) is routed here — an active session OWNS frame
    /// advancement; otherwise the single-player produce lives in
    /// [`crate::emu::EmuCore::produce_one_frame`].
    /// v1.0.0 — whether a netplay session is active, in a cfg-uniform way (the
    /// native `netplay` field and the wasm `browser_netplay` field are mutually
    /// exclusive). Used by the shell-frame capture + the Pause gate.
    // Const on native (where `is_active` is const) but not on wasm (`is_some_and`
    // is not const-stable), so it can't be uniformly `const fn`.
    #[allow(clippy::missing_const_for_fn)]
    #[must_use]
    fn netplay_is_active(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        {
            self.netplay.is_active()
        }
        #[cfg(target_arch = "wasm32")]
        {
            self.browser_netplay
                .as_ref()
                .is_some_and(crate::wasm_netplay::BrowserNetplay::is_active)
        }
    }

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
        // v1.2.0 Workstream D — the frame consumed this frame's mouse motion.
        #[cfg(not(target_arch = "wasm32"))]
        self.drain_mouse_motion();
    }

    /// v2.8.0 Phase 5 — build the synchronous (winit-thread) drive's
    /// [`crate::emu::FrameSinks`] from disjoint field borrows: the live
    /// `!Send` [`AudioOutput`] coerced to `&mut dyn AudioSink`, plus the RA
    /// session. Taking the fields directly (not `&mut self`) keeps the two
    /// borrows disjoint. Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    // The shared `'a` ties two params on the RA build; on the default build
    // only `audio` remains, so clippy sees an elidable single lifetime. The
    // `'a` is genuinely used by the cfg-gated `ra` param under the feature, so
    // it must stay declared; suppress the elision lints only on the non-feature
    // path (Rust 1.96 renamed `needless_lifetimes` -> `elidable_lifetime_names`
    // for this case, so allow both).
    #[cfg_attr(
        not(feature = "retroachievements"),
        allow(clippy::needless_lifetimes, clippy::elidable_lifetime_names)
    )]
    fn sync_sinks<'a>(
        audio: &'a mut Option<AudioOutput>,
        #[cfg(feature = "retroachievements")] ra: &'a mut Option<rustynes_ra::RaSession>,
    ) -> crate::emu::FrameSinks<'a> {
        crate::emu::FrameSinks {
            audio: audio
                .as_mut()
                .map(|a| a as &mut dyn crate::audio::AudioSink),
            #[cfg(feature = "retroachievements")]
            ra: ra.as_mut(),
        }
    }

    /// v1.5.0 "Lens" Workstream A1 — build the live Input Miniatures snapshot
    /// from the host-side input state: the standard pads (already computed) plus
    /// the port-2 / expansion device + its live button/axis state. Frontend-only
    /// (reads the same `self.input` / cursor / mouse state the emulator is fed);
    /// no core touch, no determinism surface. Native-only — the expansion-device
    /// live state (cursor / mouse / family-keyboard) is in `cfg(not(wasm32))`
    /// fields; the wasm builds push a pads-only snapshot inline at the call site.
    #[cfg(not(target_arch = "wasm32"))]
    fn input_miniatures_snapshot(
        &self,
        pads: [Buttons; 4],
        players: usize,
    ) -> crate::debugger::MiniaturesSnapshot {
        use crate::debugger::{ExpansionMini, MiniaturesSnapshot};
        let expansion = {
            use crate::config::ExpansionDevice;
            // Map the cursor X into 0..=255 for the Vaus paddle / aim, and decide
            // whether the cursor is on the NES screen (Zapper light sensor).
            let (knob, on_screen) = self.cursor_pos.map_or((0x80u8, false), |(cx, cy)| {
                let (ww, wh) = self.window_size;
                let nx = (cx / f64::from(ww.max(1))) * 256.0;
                let ny = (cy / f64::from(wh.max(1))) * 240.0;
                let on = (0.0..256.0).contains(&nx) && (0.0..240.0).contains(&ny);
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let knob = nx.clamp(0.0, 255.0) as u8;
                (knob, on)
            });
            #[allow(clippy::cast_possible_truncation)]
            let (mdx, mdy) = {
                let (ax, ay) = self.mouse_motion_accum;
                (
                    ax.clamp(-127.0, 127.0) as i16,
                    ay.clamp(-127.0, 127.0) as i16,
                )
            };
            let kb_pressed = u8::try_from(
                self.family_keyboard
                    .iter()
                    .map(|b| b.count_ones())
                    .sum::<u32>(),
            )
            .unwrap_or(u8::MAX);
            let khs = self.input.konami_hyper_shot();
            match self.config.input.expansion_device {
                ExpansionDevice::None => ExpansionMini::None,
                ExpansionDevice::Zapper => ExpansionMini::Zapper {
                    trigger: self.mouse_pressed,
                    on_screen,
                },
                ExpansionDevice::Vaus => ExpansionMini::Vaus {
                    knob,
                    button: self.mouse_pressed,
                },
                ExpansionDevice::SnesMouse => ExpansionMini::SnesMouse {
                    left: self.mouse_pressed,
                    right: self.mouse_right_pressed,
                    dx: mdx,
                    dy: mdy,
                },
                ExpansionDevice::PowerPad => ExpansionMini::PowerPad {
                    mask: self.input.power_pad(),
                    family_trainer: false,
                },
                ExpansionDevice::FamilyTrainer => ExpansionMini::PowerPad {
                    mask: self.input.power_pad(),
                    family_trainer: true,
                },
                ExpansionDevice::FamilyKeyboard => ExpansionMini::Keyboard {
                    pressed: kb_pressed,
                    subor: false,
                },
                ExpansionDevice::SuborKeyboard => ExpansionMini::Keyboard {
                    pressed: kb_pressed,
                    subor: true,
                },
                ExpansionDevice::KonamiHyperShot => ExpansionMini::KonamiHyperShot {
                    p1_run: khs & 0x01 != 0,
                    p1_jump: khs & 0x02 != 0,
                    p2_run: khs & 0x04 != 0,
                    p2_jump: khs & 0x08 != 0,
                },
                ExpansionDevice::BandaiHyperShot => ExpansionMini::BandaiHyperShot {
                    mask: self.input.bandai_hyper_shot(),
                },
            }
        };
        MiniaturesSnapshot {
            pads,
            players,
            expansion,
        }
    }

    /// Build the per-pace input snapshot for the emulation core from the
    /// winit-thread-resident input state (keyboard maps, gilrs, mouse).
    fn frame_inputs(&self) -> crate::emu::FrameInputs {
        let hardcore_blocked = self.ra_hardcore_blocks();
        // v1.2.0 Workstream F1/F2 — fold the on-screen touch overlay into the
        // per-frame snapshot. The touch buttons OR into the routed port and the
        // Power Pad mat mask ORs into `power_pad`; this is read at the SAME
        // late-latch a keypress is, so touch is recorded/replayed identically by
        // TAS movies + netplay. No-op when nothing is touched (byte-identical).
        #[cfg(target_arch = "wasm32")]
        let mut buttons = [
            self.input.player1(),
            self.input.player2(),
            self.input.player3(),
            self.input.player4(),
        ];
        #[cfg(target_arch = "wasm32")]
        {
            buttons[crate::wasm_touch::touch_target_port()] |= crate::wasm_touch::touch_buttons();
            // v1.7.0 "Forge" beta.5 Workstream H6 — fold the browser Gamepad API
            // poll into player 1 (the Xbox-style native USB-pad binding), at the
            // SAME late-latch the touch overlay + keyboard use. Empty when no pad
            // is connected, so this is byte-identical to the keyboard-only path.
            buttons[0] |= crate::wasm_gamepad::gamepad_buttons();
        }
        #[cfg(not(target_arch = "wasm32"))]
        let mut buttons = [
            self.input.player1(),
            self.input.player2(),
            self.input.player3(),
            self.input.player4(),
        ];
        // Fold the desktop on-screen virtual pad into player 1, at the SAME
        // late-latch the keyboard / gamepad use, so an on-screen press records +
        // replays identically in TAS movies and netplay. Empty (byte-identical)
        // when the pad window is closed or no button is held.
        #[cfg(not(target_arch = "wasm32"))]
        {
            buttons[0] |= self.virtual_pad.mask();
        }
        crate::emu::FrameInputs {
            buttons,
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
            // v1.5.0 D4 — the Vaus paddle X applies the pointer-scale gain
            // around the screen centre (pointer_scale 1.0 = the prior 1:1 map,
            // byte-identical); the Zapper aim is unaffected (the gain is only
            // meaningful for the paddle and the clamp keeps the aim sane).
            #[cfg(not(target_arch = "wasm32"))]
            mouse_nes: self.cursor_pos.map_or((u16::MAX, u16::MAX), |(cx, cy)| {
                let (ww, wh) = self.window_size;
                let scale = f64::from(self.config.input.pointer_scale.clamp(0.1, 8.0));
                let raw_x = (cx / f64::from(ww.max(1))) * 256.0;
                // Apply the paddle gain (deviation from centre 128, scaled) only
                // for the Vaus device; the Zapper keeps the raw cursor map.
                let mapped_x = if matches!(
                    self.config.input.expansion_device,
                    crate::config::ExpansionDevice::Vaus
                ) {
                    (raw_x - 128.0).mul_add(scale, 128.0)
                } else {
                    raw_x
                };
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let x = mapped_x.clamp(0.0, 255.0) as i64;
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                let y = ((cy / f64::from(wh.max(1))) * 240.0) as i64;
                (
                    u16::try_from(x).unwrap_or(u16::MAX),
                    u16::try_from(y).unwrap_or(u16::MAX),
                )
            }),
            #[cfg(not(target_arch = "wasm32"))]
            mouse_pressed: self.mouse_pressed,
            turbo_mask: self.turbo_mask(),
            turbo_period: self.config.input.turbo_period,
            // v1.2.0 Workstream F1 — OR the touch Power Pad mat mask into the
            // keyboard-driven one (native keymap on wasm-winit + touch overlay).
            #[cfg(target_arch = "wasm32")]
            power_pad: self.input.power_pad() | crate::wasm_touch::touch_power_pad(),
            // v1.5.0 D4 — remap the mat mask for the selected Power Pad layout
            // side (Side A = identity, byte-identical).
            #[cfg(not(target_arch = "wasm32"))]
            power_pad: self
                .config
                .input
                .power_pad_layout
                .remap_mask(self.input.power_pad()),
            // v1.2.0 Workstream F2 — the Power Pad is the active wasm device when
            // the touch UI selected it (gates the wasm-only latch arm in
            // `EmuCore::latch`; native keys off `expansion` instead).
            #[cfg(target_arch = "wasm32")]
            power_pad_active: crate::wasm_touch::touch_power_pad_active(),
            // v1.2.0 Workstream D — SNES mouse: report the motion accumulated
            // since the last frame latch (drained by the produce / publish path).
            // v1.5.0 D4 — scale the accumulated motion by the pointer-speed
            // multiplier (1.0 = the prior 1:1 motion, byte-identical) before the
            // ±127 clamp the serial mouse reports.
            #[cfg(not(target_arch = "wasm32"))]
            #[allow(clippy::cast_possible_truncation)]
            mouse_delta: {
                let (ax, ay) = self.mouse_motion_accum;
                let scale = f64::from(self.config.input.pointer_scale.clamp(0.1, 8.0));
                (
                    (ax * scale).clamp(-127.0, 127.0) as i16,
                    (ay * scale).clamp(-127.0, 127.0) as i16,
                )
            },
            #[cfg(not(target_arch = "wasm32"))]
            mouse_right: self.mouse_right_pressed,
            #[cfg(not(target_arch = "wasm32"))]
            mouse_sensitivity: self.config.input.mouse_sensitivity.min(2),
            #[cfg(not(target_arch = "wasm32"))]
            family_keyboard: self.family_keyboard,
            // v1.3.0 Workstream F1 — Konami / Bandai Hyper Shot masks. Consumed
            // only by their expansion-device arms; 0 otherwise (byte-identical).
            #[cfg(not(target_arch = "wasm32"))]
            konami_hyper_shot: self.input.konami_hyper_shot(),
            #[cfg(not(target_arch = "wasm32"))]
            bandai_hyper_shot: self.input.bandai_hyper_shot(),
        }
    }

    /// v1.2.0 Workstream D — drain the SNES-mouse motion accumulator after the
    /// per-frame latch has consumed it. Called once per produced / published
    /// frame so each NES poll sees only that frame's motion (a real serial mouse
    /// reports movement-since-last-strobe). Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    const fn drain_mouse_motion(&mut self) {
        self.mouse_motion_accum = (0.0, 0.0);
    }

    /// v1.1.0 beta.1 (T-110-B2) — the configured turbo/autofire button mask
    /// (empty = off). The gate itself is applied at latch keyed on the emulated
    /// frame; this just resolves which buttons participate.
    fn turbo_mask(&self) -> Buttons {
        let mut m = Buttons::empty();
        if self.config.input.turbo_a {
            m |= Buttons::A;
        }
        if self.config.input.turbo_b {
            m |= Buttons::B;
        }
        m
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
    fn publish_shared_input(&mut self) {
        if let Some(thread) = self.emu_thread.as_ref() {
            thread.shared_input().publish(&self.frame_inputs());
            // Fast-forward is a control-block flag (not part of the per-frame
            // input snapshot), so push the live held state alongside.
            thread
                .control()
                .set_fast_forward(self.input.fast_forward_held());
        }
        // v1.2.0 Workstream D — the published snapshot carried this frame's
        // mouse motion; drain so the next publish reports only new movement.
        self.drain_mouse_motion();
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
        // v1.5.0 H1 — the emu thread publishes produced frames into this
        // shared handoff; the present path takes them lock-free.
        self.present_buffer.reset();
        self.emu_thread = Some(crate::emu_thread::EmuThread::spawn(
            self.emu.clone(),
            producer,
            proxy,
            control,
            shared_input,
            std::sync::Arc::clone(&self.present_buffer),
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
                    let jl = rustynes_ra::RaSession::take_just_logged_in(ra);
                    (Some(s), jl)
                })
            };
            if let Some(status) = status
                && let Some(debugger) = self.debugger.as_mut()
            {
                debugger.set_cheevos_status(status);
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
        // v1.1.0 beta.2 (Workstream C) — a breakpoint fired: pause emulation and
        // open the CPU debugger so the user lands on the stopped PC.
        if let Some(pc) = fx.breakpoint_hit {
            self.set_paused(true);
            if let Some(d) = self.debugger.as_mut() {
                d.open_chip_panel(crate::debugger::ChipPanel::Cpu);
            }
            self.ui
                .set_status(crate::ui_shell::StatusMessage::info(format!(
                    "Breakpoint hit at ${pc:04X} — paused"
                )));
        }
        // v1.4.0 Workstream D (D2) — an event-driven breakpoint fired: pause +
        // open the CPU debugger and report the kind + timing context.
        if let Some(hit) = fx.event_break_hit {
            self.set_paused(true);
            if let Some(d) = self.debugger.as_mut() {
                d.open_chip_panel(crate::debugger::ChipPanel::Cpu);
            }
            self.ui
                .set_status(crate::ui_shell::StatusMessage::info(format!(
                    "Event breakpoint: {} (${:04X}) — frame {} cyc {} sl {} dot {} — paused",
                    hit.kind.label(),
                    hit.addr,
                    hit.frame,
                    hit.cycle,
                    hit.scanline,
                    hit.dot
                )));
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
        {
            if let Some(status) = fx.ra_status
                && let Some(debugger) = self.debugger.as_mut()
            {
                debugger.set_cheevos_status(status);
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
            .is_some_and(rustynes_ra::RaSession::hardcore_blocks)
    }

    /// v2.7.0 — the hardcore-gating predicate, always `false` when the
    /// `retroachievements` feature is not built (no RA session can exist), so
    /// the gated sites compile to plain no-ops on the default build.
    #[cfg(not(all(not(target_arch = "wasm32"), feature = "retroachievements")))]
    #[allow(clippy::unused_self)]
    const fn ra_hardcore_blocks(&self) -> bool {
        false
    }

    /// v1.7.0 "Forge" H2 — hardcore pause-gating. rcheevos throttles how often
    /// the player may pause in hardcore (to prevent pause-abuse). Returns
    /// `Some(frames_remaining)` when a pause must be deferred, or `None` when a
    /// pause is allowed right now. In softcore / no-game / feature-off this is
    /// always `None` (a pause is always allowed). Only call when actually trying
    /// to pause — the rcheevos call is stateful.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    fn ra_pause_gate(&mut self) -> Option<u32> {
        self.ra.as_mut().and_then(|ra| {
            let (allowed, frames) = ra.can_pause();
            if allowed { None } else { Some(frames) }
        })
    }

    /// v1.7.0 "Forge" H2 — the pause-gating predicate, always `None` when the
    /// `retroachievements` feature is not built, so `set_paused` compiles to the
    /// unchanged path on the default build.
    #[cfg(not(all(not(target_arch = "wasm32"), feature = "retroachievements")))]
    #[allow(clippy::unused_self, clippy::needless_pass_by_ref_mut)]
    const fn ra_pause_gate(&mut self) -> Option<u32> {
        None
    }

    /// PR #75 review (H1) — load-state restores the timeline, so it is forbidden
    /// while a TAS movie is RECORDING (it would rewrite the recording) OR PLAYING
    /// BACK (it would desync playback). The File menu greys "Load State" /
    /// "Load from Slot" out under this same condition (`ui_shell::rom_interactive`
    /// = `rom && !replay_locked`), but the hotkey + `MenuAction` dispatch must
    /// honour it too — otherwise the greyed item is bypassable via the bound key.
    /// Mirrors `GeraNES` `replayInteractionLocked` / `replayRecordingActive`.
    fn replay_interaction_locked(&self) -> bool {
        let emu = self.emu.lock();
        emu.movie.is_recording() || emu.movie.is_playing()
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
            .and_then(rustynes_ra::RaSession::game_sha256)
        else {
            return;
        };
        let Some(path) = self.ra_progress_path(&sha) else {
            return;
        };
        let blob = self
            .ra
            .as_mut()
            .map(rustynes_ra::RaSession::serialize_progress);
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
            .and_then(rustynes_ra::RaSession::user_token)
        else {
            return;
        };
        if !token.is_empty() && self.config.retroachievements.token != token {
            self.config.retroachievements.token = token;
            self.config.retroachievements.enabled = true;
            if let Some(name) = self
                .ra
                .as_ref()
                .and_then(rustynes_ra::RaSession::user_info)
                .map(|u| u.username)
            {
                self.config.retroachievements.username = name;
            }
            let _ = self.config.save();
        }
    }

    /// Reset the running emulator (and keep `RetroAchievements` in sync when the
    /// feature is active — Reset is always allowed, even in hardcore).
    fn do_reset(&mut self) {
        {
            let mut guard = self.emu.lock();
            if let Some(nes) = guard.nes.as_mut() {
                nes.reset();
                // v1.0.0 (UX3 BUG-3) — re-apply the configured Game Genie codes
                // to the post-reset core (disjoint borrow: `guard.nes` + the
                // separate `debugger` field), so cheats keep working across a
                // Reset even with the Cheats panel closed. A no-op when no codes
                // are enabled (the no-cheat path stays byte-identical).
                if let Some(debugger) = self.debugger.as_mut() {
                    debugger.reapply_genie_codes(nes);
                    // v1.7.0 "Forge" Workstream C (C1/C2) — the reconstructed
                    // call stack + access counters are stale after a reset.
                    debugger.reset_debug_telemetry();
                }
            }
            // v1.7.0 "Forge" H4 — a reset starts a fresh lag-frame tally.
            guard.reset_lag_frames();
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
        self.reset_ra();
    }

    /// v1.0.0 — dispatch a UX-shell [`crate::ui_shell::MenuAction`]. Run AFTER
    /// the egui pass (the build closure cannot hold `&mut self`).
    #[allow(clippy::too_many_lines)]
    fn dispatch_menu_action(
        &mut self,
        action: crate::ui_shell::MenuAction,
        event_loop: &ActiveEventLoop,
    ) {
        use crate::ui_shell::{MenuAction, StatusMessage};
        match action {
            MenuAction::OpenRom => {
                #[cfg(not(target_arch = "wasm32"))]
                self.open_rom_dialog();
            }
            MenuAction::LoadRom(path) => {
                // The Recent-ROMs menu is native-only (filesystem paths), so on
                // wasm this arm is unreachable — drop the payload to keep the
                // match exhaustive without an unused binding.
                #[cfg(not(target_arch = "wasm32"))]
                self.load_rom_from_path(&path);
                #[cfg(target_arch = "wasm32")]
                let _ = path;
            }
            MenuAction::ClearRecent => {
                self.config.recent_roms.paths.clear();
                #[cfg(not(target_arch = "wasm32"))]
                let _ = self.config.save();
                self.ui
                    .set_status(StatusMessage::info("Recent ROMs cleared"));
            }
            MenuAction::CloseRom => {
                self.close_rom();
            }
            MenuAction::SaveState => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_save_state(self.active_save_slot);
                #[cfg(target_arch = "wasm32")]
                self.handle_save_state_wasm(self.active_save_slot);
                self.ui.set_status(StatusMessage::success("State saved"));
            }
            MenuAction::LoadState => {
                if self.ra_hardcore_blocks() {
                    self.ui
                        .set_status(StatusMessage::info("Load state disabled (hardcore)"));
                } else if self.replay_interaction_locked() {
                    self.ui
                        .set_status(StatusMessage::info("Load state disabled during movie"));
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    self.handle_load_state(self.active_save_slot);
                    #[cfg(target_arch = "wasm32")]
                    self.handle_load_state_wasm(self.active_save_slot);
                    self.ui.set_status(StatusMessage::success("State loaded"));
                }
            }
            MenuAction::Quit => {
                self.should_exit = true;
                event_loop.exit();
            }
            MenuAction::TogglePause => {
                // A manual pause/resume takes ownership of the pause state, so
                // a subsequent focus-regain must not auto-resume it (and a
                // manual resume clears any pending auto-pause flag).
                self.auto_paused = false;
                self.set_paused(!self.ui.paused);
            }
            MenuAction::Reset => {
                self.do_reset();
                self.ui.set_status(StatusMessage::info("Reset"));
            }
            MenuAction::PowerCycle => {
                self.do_power_cycle();
                self.ui.set_status(StatusMessage::info("Power cycled"));
            }
            MenuAction::ToggleFullscreen => {
                self.toggle_fullscreen();
            }
            MenuAction::ToggleMenuBar => {
                self.ui.menu_visible = !self.ui.menu_visible;
            }
            MenuAction::SetWindowScale(scale) => {
                self.set_window_scale(scale);
            }
            MenuAction::CycleDiskSide => {
                self.cycle_disk_side();
            }
            MenuAction::SetDiskSide(side) => {
                self.set_disk_side(side);
            }
            MenuAction::Screenshot => {
                #[cfg(not(target_arch = "wasm32"))]
                self.take_screenshot();
            }
            MenuAction::ScreenshotToClipboard => {
                #[cfg(not(target_arch = "wasm32"))]
                self.screenshot_to_clipboard();
            }
            MenuAction::SetSaveSlot(slot) => {
                self.active_save_slot = slot;
                self.ui
                    .set_status(StatusMessage::info(format!("Save slot {}", slot + 1)));
            }
            MenuAction::SaveStateSlot(slot) => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_save_state(slot);
                #[cfg(target_arch = "wasm32")]
                self.handle_save_state_wasm(slot);
                self.ui.set_status(StatusMessage::success(format!(
                    "Saved to slot {}",
                    slot + 1
                )));
            }
            MenuAction::LoadStateSlot(slot) => {
                if self.ra_hardcore_blocks() {
                    self.ui
                        .set_status(StatusMessage::info("Load state disabled (hardcore)"));
                } else if self.replay_interaction_locked() {
                    self.ui
                        .set_status(StatusMessage::info("Load state disabled during movie"));
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    self.handle_load_state(slot);
                    #[cfg(target_arch = "wasm32")]
                    self.handle_load_state_wasm(slot);
                    self.ui.set_status(StatusMessage::success(format!(
                        "Loaded from slot {}",
                        slot + 1
                    )));
                }
            }
            MenuAction::MovieRecordToggle => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_movie_record_toggle();
                #[cfg(target_arch = "wasm32")]
                self.handle_movie_record_toggle_wasm();
            }
            MenuAction::MoviePlayToggle => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_movie_play_toggle();
                #[cfg(target_arch = "wasm32")]
                self.handle_movie_play_toggle_wasm();
            }
            MenuAction::MovieBranch => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_movie_branch();
                #[cfg(target_arch = "wasm32")]
                self.handle_movie_branch_wasm();
            }
            MenuAction::MovieImport => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_movie_import();
            }
            MenuAction::MovieExport => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_movie_export();
            }
            MenuAction::MovieExportSubtitles => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_movie_export_subtitles();
            }
            MenuAction::HistoryExportClip { seconds } => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_history_export_clip(seconds);
                #[cfg(target_arch = "wasm32")]
                let _ = seconds;
            }
            MenuAction::FrameAdvance => {
                self.request_frame_advance();
            }
            MenuAction::ToggleVirtualPad => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    self.virtual_pad.visible = !self.virtual_pad.visible;
                }
            }
            MenuAction::OpenSaveStates => {
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Force a fresh rebuild on the next egui pass (which has the
                    // `ctx` needed to upload thumbnail textures).
                    self.save_states_ui.invalidate_all();
                    self.save_states_ui.open = true;
                    if let Some(gfx) = self.gfx.as_ref() {
                        gfx.window.request_redraw();
                    }
                }
                #[cfg(target_arch = "wasm32")]
                {
                    // v1.4.0 E2 — open the browser Save-States grid and kick
                    // off the async IndexedDB slot scan for the current ROM.
                    let sha = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256());
                    crate::wasm_save_states::open(sha);
                    if let Some(gfx) = self.gfx.as_ref() {
                        gfx.window.request_redraw();
                    }
                }
            }
            MenuAction::SetSpeed(speed) => {
                self.set_speed(speed);
            }
            MenuAction::SetOverscan(on) => {
                if let Some(gfx) = self.gfx.as_mut() {
                    gfx.set_hide_overscan(on);
                    gfx.window.request_redraw();
                }
            }
            MenuAction::AvRecordToggle => {
                #[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
                self.handle_av_record_toggle();
            }
            MenuAction::InsertCoin => {
                let mut guard = self.emu.lock();
                let emu = &mut *guard;
                if let Some(nes) = emu.nes.as_mut() {
                    nes.insert_coin(0);
                    emu.vs_coin_frames = VS_COIN_HOLD_FRAMES;
                } else if let Some(dual) = emu.dual.as_mut() {
                    // v2.1.2 F2.1 — coin into the MAIN cabinet acceptor.
                    dual.insert_coin(0);
                    emu.vs_coin_frames = VS_COIN_HOLD_FRAMES;
                }
            }
            MenuAction::OpenPanel(panel) => {
                // v1.6.0 "Studio" A2 — opening TAStudio on a loaded ROM starts a
                // session by anchoring the editor on the current emulator state.
                if panel == crate::debugger::ToolPanel::TasStudio {
                    self.ensure_tas_editor();
                }
                if let Some(d) = self.debugger.as_mut() {
                    d.open_panel(panel);
                }
            }
            MenuAction::OpenChipPanel(panel) => {
                if let Some(d) = self.debugger.as_mut() {
                    d.open_chip_panel(panel);
                }
            }
            MenuAction::LoadHdPack => {
                #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                self.load_hd_pack_dialog();
            }
            MenuAction::UnloadHdPack => {
                #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                self.unload_hd_pack();
            }
            MenuAction::HdPackBuilderStart => {
                #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                self.hd_pack_builder_start();
            }
            MenuAction::HdPackBuilderStop => {
                #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                self.hd_pack_builder_stop();
            }
            MenuAction::LoadSymbols => {
                #[cfg(not(target_arch = "wasm32"))]
                self.load_symbols_dialog();
            }
            MenuAction::ClearSymbols => {
                if let Some(d) = self.debugger.as_mut() {
                    d.clear_symbols();
                }
                #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                self.refresh_script_symbols();
            }
            MenuAction::OpenDocumentation => {
                // v1.5.0 "Lens" Workstream I10 — open the in-app Documentation
                // browser. Native-only (the panel reuses the native `cli` topic
                // registry); the wasm dispatch is a no-op.
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(d) = self.debugger.as_mut() {
                    d.open_documentation();
                }
            }
        }
    }

    /// v1.4.0 Workstream D (D1) — pick a symbol/label file (`.sym` / Mesen
    /// `.mlb` / FCEUX `.nl`), parse it, and merge its labels into the debugger's
    /// annotation map. Display-only (the deterministic core is untouched);
    /// native-only (it reads a picked file). No-op if the debugger isn't built.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_symbols_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("Symbol / source-map files", &["sym", "mlb", "nl", "dbg"])
            .add_filter("All files", &["*"])
            .pick_file()
        else {
            return;
        };
        let ext = path
            .extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase);
        // v1.7.0 "Forge" Workstream C (C3) — a ca65/cc65 `.dbg` file maps source
        // lines (handled separately from the name-only symbol formats).
        if ext.as_deref() == Some("dbg") {
            self.load_source_map_file(&path);
            return;
        }
        let format = ext
            .as_deref()
            .and_then(crate::symbols::SymbolFormat::from_extension);
        let Some(format) = format else {
            self.ui.set_status(crate::ui_shell::StatusMessage::error(
                "Unrecognized file extension (expected .sym / .mlb / .nl / .dbg)",
            ));
            return;
        };
        let text = match std::fs::read_to_string(&path) {
            Ok(t) => t,
            Err(e) => {
                self.ui
                    .set_status(crate::ui_shell::StatusMessage::error(format!(
                        "symbol load failed: {e}"
                    )));
                return;
            }
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("symbols")
            .to_owned();
        if let Some(d) = self.debugger.as_mut() {
            d.load_symbols(&name, &text, format);
            d.open_chip_panel(crate::debugger::ChipPanel::Cpu);
            self.ui
                .set_status(crate::ui_shell::StatusMessage::info(format!(
                    "Loaded symbols from {name}"
                )));
        }
        // v1.5.0 B4 — push the freshly-loaded labels into a running Lua script's
        // `sym:` query tables (no-op if no script is loaded). The dev/TAS symbol
        // bridge is native-only (mlua); guard the call to match
        // `refresh_script_symbols`'s own `not(wasm32)` cfg so a wasm32 +
        // `scripting` build doesn't reference a method that isn't compiled.
        #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
        self.refresh_script_symbols();
    }

    /// v1.7.0 "Forge" Workstream C (C3) — read a ca65/cc65 `.dbg` file and load
    /// its `address -> (source file, line)` mapping into the debugger's source
    /// map (annotates the disassembly). Display-only; never touches the core.
    #[cfg(not(target_arch = "wasm32"))]
    fn load_source_map_file(&mut self, path: &std::path::Path) {
        let text = match std::fs::read_to_string(path) {
            Ok(t) => t,
            Err(e) => {
                self.ui
                    .set_status(crate::ui_shell::StatusMessage::error(format!(
                        "source map load failed: {e}"
                    )));
                return;
            }
        };
        let name = path
            .file_name()
            .and_then(|n| n.to_str())
            .unwrap_or("debug")
            .to_owned();
        if let Some(d) = self.debugger.as_mut() {
            d.load_source_map(&name, &text);
            d.open_chip_panel(crate::debugger::ChipPanel::Cpu);
            self.ui
                .set_status(crate::ui_shell::StatusMessage::info(format!(
                    "Loaded source map from {name}"
                )));
        }
    }

    /// v1.5.0 Workstream B (B4) — push the debugger's current symbol map into a
    /// running Lua script's `sym:` query tables. Called after a symbol load /
    /// clear so `sym:addr` / `sym:name` track the loaded labels. Read-only on the
    /// engine side; never perturbs the deterministic core. No-op without a script.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    fn refresh_script_symbols(&self) {
        let Some(engine) = self.script.as_ref() else {
            return;
        };
        let pairs = self
            .debugger
            .as_ref()
            .map(crate::debugger::DebuggerOverlay::symbol_pairs)
            .unwrap_or_default();
        engine.set_symbols(&pairs);
    }

    /// v1.0.0 — the emulation-speed presets surfaced in the Emulation -> Speed
    /// submenu and stepped through by the Speed Up / Down keys. 100% (`1.0`)
    /// is the determinism-safe default the app always launches at.
    const SPEED_PRESETS: [f32; 7] = [0.25, 0.5, 0.75, 1.0, 1.5, 2.0, 3.0];

    /// v1.0.0 — apply the configured master volume / mute to the live audio
    /// output gain (the single cpal consume point). Native-only; cheap, called
    /// at startup + on every volume / mute edit. No-op when audio is disabled.
    #[cfg(not(target_arch = "wasm32"))]
    fn apply_audio_gain(&self) {
        if let Some(audio) = self.audio.as_ref() {
            // v1.7.0 H3 — fold in the per-context mixer legs (master ×
            // game/menu). All the new legs default to 1.0, so this equals
            // `effective_gain()` until a per-context slider moves — the default
            // output stays byte-identical.
            let in_game = self.emu.lock().nes.is_some();
            audio
                .queue
                .set_gain(self.config.audio.effective_gain_for(in_game));
        }
    }

    /// v1.1.0 beta.3 (Workstream E, T-110-E5) — pump the Lua engine once this
    /// redraw: handle a console action, then (if a script is loaded) run its
    /// callbacks against the live `Nes` under the emu lock and apply the
    /// resulting log / control / draw output.
    ///
    /// mlua is `!Send`, so the engine lives on this (winit) thread and is
    /// pumped at display rate; the access/trace logs hold the most-recent
    /// emulated frame. Script writes are gated off in a locked / deterministic
    /// session (netplay / TAS replay / RA-hardcore), like the cheat path.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    fn pump_scripts(&mut self) {
        // Console action (load / reload / stop) — taken without holding the
        // debugger borrow across the `&mut self` handler.
        let action = self
            .debugger
            .as_mut()
            .and_then(|d| d.script_panel().take_action());
        if let Some(action) = action {
            self.handle_script_action(action);
        }

        if self.script.is_none() {
            return;
        }

        // These reads don't need the emu lock, so resolve them first to keep
        // the lock window below tight (and non-reentrant).
        let netplay_locked = self.netplay.is_active() || self.ra_hardcore_blocks();

        // v1.7.0 "Forge" Workstream B (B1) — push a read-only snapshot of the
        // live TAStudio editor so the script's `tastudio.*` query API resolves
        // against current editor state (the `set_symbols` host-push pattern).
        // Built before borrowing `self.script` so the two `&self`/`&mut self`
        // borrows don't overlap. Only rebuilt (and re-cloned) when the editor's
        // edit revision moved or it opened/closed — an idle TAStudio costs no
        // per-frame clone of the whole input log + lag vector + markers +
        // branches; the engine retains the previously-pushed snapshot.
        let editor_revision = self
            .debugger
            .as_ref()
            .and_then(crate::debugger::DebuggerOverlay::tas_editor)
            .map(crate::tastudio::TasEditor::revision);
        if editor_revision != self.tas_snapshot_revision {
            let tas_snapshot = Self::build_tas_snapshot(self.debugger.as_ref());
            let engine = self.script.as_mut().expect("checked is_some");
            engine.set_tas_snapshot(tas_snapshot);
            self.tas_snapshot_revision = editor_revision;
        }
        let engine = self.script.as_mut().expect("checked is_some");

        // Pump under ONE emu lock with the live Nes, collecting the outputs
        // (gemini #48: a single lock, dropped before applying control commands).
        // M2: only `on_frame` needs the live `Nes`, so the lock is held *just*
        // around it; the log/control/draw drains touch engine-side buffers only
        // and run after the guard drops, minimizing contention with the
        // emulation thread.
        let mut err = None;
        // The combined write-gate, assigned once inside the lock so the SetInput
        // control application below reuses the EXACT same condition as
        // `emu.write` (T-110-E2). Every path that reaches the later read has run
        // through the block (the early `return` exits the whole function).
        // v1.7.0 "Forge" E1 — feed back any host-fulfilled IPC results BEFORE
        // the pump, so a script polling `comm.receive()` sees this frame's
        // replies. The host (`script_host`) did the I/O off the emu lock; here
        // we only move already-marshalled plain values into the engine. Done
        // before the lock window so it never extends lock hold time.
        #[cfg(feature = "script-ipc")]
        if let Some(host) = self.script_host.as_ref() {
            for result in host.drain_results() {
                engine.push_comm_result(result);
            }
        }

        let writes_locked;
        {
            let mut guard = self.emu.lock();
            // Read the movie flags before the `nes` borrow — a `MutexGuard`
            // deref borrows the whole guard, so the two can't overlap.
            let movie_locked = guard.movie.is_playing() || guard.movie.is_recording();
            writes_locked = netplay_locked || movie_locked;
            let Some(nes) = guard.nes.as_mut() else {
                return;
            };
            // Determinism gate (same policy as the raw-RAM cheat path). It also
            // gates the new `comm.*` IPC + the `client.*` mutators identically.
            engine.set_writes_locked(writes_locked);
            // Enable the per-frame exec / access logs the registered callbacks
            // need. The exec log is independent of the Trace Logger panel's
            // `set_trace_enabled`, so scripting never fights the user's trace
            // setting (Copilot #48).
            nes.set_exec_logging(engine.needs_exec_log());
            nes.set_access_logging(engine.needs_access_log());
            // T-110-E1 — the interrupt-service log for the Lua onNmi/onIrq
            // callbacks (independent of the access / exec logs).
            nes.set_interrupt_logging(engine.needs_interrupt_log());
            if let Err(e) = engine.on_frame(nes) {
                err = Some(e.to_string());
            }
        } // emu lock dropped here

        // Drain engine-side buffers (no `Nes` access) outside the lock (M2).
        let log = engine.drain_log();
        let controls = engine.drain_controls();
        let draws = engine.drain_draws();
        // v1.7.0 "Forge" E2 — the `client.*` automation verbs this frame.
        let clients = engine.drain_clients();
        // v1.7.0 "Forge" E1 — the host-mediated `comm.*` IPC requests. The
        // engine already dropped them at the source if `writes_locked`, so this
        // is empty under a locked session; forward the rest to the host bridge,
        // which owns the connection and does the I/O off this lock.
        #[cfg(feature = "script-ipc")]
        {
            let comm = engine.drain_comm();
            if let Some(host) = self.script_host.as_ref() {
                for cmd in comm {
                    host.submit(cmd);
                }
            }
        }

        // Feed the console + stash the overlay draws (engine borrow ended).
        if let Some(dbg) = self.debugger.as_mut() {
            let p = dbg.script_panel();
            p.push_log(log);
            if let Some(e) = err {
                p.set_error(e);
            }
        }
        self.script_draws = draws;

        // v1.7.0 "Forge" Workstream B (B1) — drain + apply the `tastudio.*`
        // editor commands. They were already gated AT THE SOURCE (the script
        // crate drops every mutator when `writes_locked`), so an empty drain
        // under lock is the expected case; the host applies whatever the v1.6.0
        // editor model supports.
        let engine = self.script.as_mut().expect("checked is_some");
        let tas_cmds = engine.drain_tas_commands();
        if !tas_cmds.is_empty() {
            self.apply_tas_commands(&tas_cmds);
        }

        // Apply control commands (these `&mut self` methods re-lock the emu).
        // `writes_locked` is the same gate `emu.write` uses; SetInput honors it.
        for cmd in &controls {
            self.apply_script_control(cmd, writes_locked);
        }

        // v1.7.0 "Forge" E2 — apply the `client.*` automation verbs. The
        // state-changing verbs (reboot / cheats) were already dropped at the
        // source when locked; this re-checks `writes_locked` as defence in depth.
        for cmd in &clients {
            self.apply_script_client(cmd, writes_locked);
        }
    }

    /// v1.7.0 "Forge" Workstream B (B1) — build the read-only `TAStudio` snapshot
    /// the script's `tastudio.*` query API reads. Empty (editor not engaged)
    /// when no editor session exists, so a script's `tastudio:engaged()` is
    /// `false` and every query returns its `nil` / empty form.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    fn build_tas_snapshot(
        debugger: Option<&crate::debugger::DebuggerOverlay>,
    ) -> rustynes_script::TasSnapshot {
        use rustynes_script::{TasBranchInfo, TasSnapshot};
        let Some(editor) = debugger.and_then(crate::debugger::DebuggerOverlay::tas_editor) else {
            return TasSnapshot::default();
        };
        // Per-frame lag verdicts for the played prefix (None reads back as
        // `false`; a script bound-checks against `input_len`).
        let lag: Vec<bool> = (0..editor.len())
            .map(|f| editor.lag_at(f) == Some(true))
            .collect();
        let state_frames: Vec<usize> = editor.greenzone().cached_frames().collect();
        let markers: Vec<(usize, String)> =
            editor.markers().map(|(f, l)| (f, l.to_owned())).collect();
        // The v1.6.0 `Branch` model has no annotation text yet, so `text` is
        // empty (forward-compatible: `setbranchtext` is a documented host stub).
        let branches: Vec<TasBranchInfo> = (0..editor.branch_count())
            .filter_map(|i| editor.branch(i))
            .map(|b| TasBranchInfo {
                frame: b.frame,
                text: String::new(),
                input: b
                    .input_log
                    .iter()
                    .map(|fi| (fi.p1.bits(), fi.p2.bits()))
                    .collect(),
            })
            .collect();
        TasSnapshot {
            engaged: true,
            // `recording` / `selection` are piano-roll UI concerns the v1.6.0
            // editor model doesn't own; reported as the default until the model
            // tracks them.
            recording: false,
            seek_frame: editor.cursor(),
            selection: None,
            lag,
            state_frames,
            markers,
            branches,
            input_len: editor.len(),
        }
    }

    /// v1.7.0 "Forge" Workstream B (B1) — apply the drained `tastudio.*` editor
    /// commands to the live `TasEditor`. Every command was already gated at the
    /// source (dropped by the script crate under a locked session), so this
    /// path runs only for an unlocked session. Edits re-seek the editor so the
    /// `Nes` tracks the change (the same deterministic replay path the panel
    /// uses). Commands the v1.6.0 editor model can't yet honour
    /// (`SetRecording` / `SetLag` / `SetBranchText`) are accepted no-ops.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    fn apply_tas_commands(&mut self, cmds: &[rustynes_script::TasCmd]) {
        use rustynes_core::Buttons;
        use rustynes_script::TasCmd;
        // Resolve any `setplayback(markerName)` targets to frames BEFORE taking
        // the mutable editor borrow (avoids an immutable+mutable overlap).
        let marker_targets: std::collections::HashMap<String, usize> = self
            .debugger
            .as_ref()
            .and_then(crate::debugger::DebuggerOverlay::tas_editor)
            .map(|ed| {
                let wanted: std::collections::HashSet<&str> = cmds
                    .iter()
                    .filter_map(|c| match c {
                        TasCmd::SetPlaybackMarker(name) => Some(name.as_str()),
                        _ => None,
                    })
                    .collect();
                ed.markers()
                    .filter(|(_, l)| wanted.contains(l))
                    .map(|(f, l)| (l.to_owned(), f))
                    .collect()
            })
            .unwrap_or_default();
        let mut guard = self.emu.lock();
        let Some(nes) = guard.nes.as_mut() else {
            return;
        };
        let Some(editor) = self
            .debugger
            .as_mut()
            .and_then(crate::debugger::DebuggerOverlay::tas_editor_mut)
        else {
            return;
        };
        // Batch `SetInput` edits: each `set_input` only invalidates the
        // greenzone; the expensive deterministic replay is the `seek`. So we
        // accumulate input edits and re-seek the cursor ONCE — either when a
        // command that itself seeks intervenes (to preserve ordering) or at the
        // end of the batch — instead of re-seeking per edit (the common
        // `applyinputchanges` case stages a whole batch of `SetInput`s).
        let mut input_dirty = false;
        for cmd in cmds {
            match cmd {
                TasCmd::SetPlaybackFrame(f) => {
                    input_dirty = false; // the explicit seek subsumes any pending edit re-seek.
                    editor.seek(nes, *f);
                }
                TasCmd::SetPlaybackMarker(name) => {
                    if let Some(&f) = marker_targets.get(name) {
                        input_dirty = false;
                        editor.seek(nes, f);
                    }
                }
                TasCmd::SetMarker { frame, text } => editor.set_marker(*frame, text.clone()),
                TasCmd::RemoveMarker(frame) => editor.remove_marker(*frame),
                TasCmd::SetInput {
                    frame,
                    port,
                    buttons,
                } => {
                    // Merge the edit into the frame's existing input for the
                    // edited port (the script boundary already rejects any
                    // `port > 1`; the match mirrors that defensively so an
                    // out-of-range port can never silently apply to P2 here),
                    // then mark the downstream state dirty for ONE re-seek.
                    let mut input = editor.input_at(*frame).unwrap_or_default();
                    let pad = Buttons::from_bits_truncate(*buttons);
                    match *port {
                        0 => input.p1 = pad,
                        1 => input.p2 = pad,
                        _ => continue, // unreachable past the boundary; ignore.
                    }
                    input_dirty |= editor.set_input(*frame, input);
                }
                TasCmd::LoadBranch(idx) => {
                    input_dirty = false; // load_branch reseats the whole timeline.
                    editor.load_branch(*idx, nes);
                }
                // No editor target in the v1.6.0 model: `SetRecording` /
                // `SetLag` / `SetBranchText` are documented host stubs. The
                // wildcard also absorbs them (and any future variant added
                // script-side, since `TasCmd` is `#[non_exhaustive]`).
                _ => {}
            }
        }
        // Flush the batched input edits with a single deterministic re-seek.
        if input_dirty {
            let cursor = editor.cursor();
            editor.seek(nes, cursor);
        }
    }

    /// v1.6.0 "Studio" Workstream C — drive the debugger Watch panel's
    /// per-frame observational replay (conditional breakpoints + read/write/exec
    /// watchpoints + watch window + conditional trace). Runs after a frame is
    /// produced, under the emu lock, exactly like [`Self::pump_scripts`]; it only
    /// *reads* the just-finished frame's exec/access logs, so determinism is
    /// unaffected. Runs on every build (the frontend always pulls `debug-hooks`),
    /// so it is not behind the `scripting` gate.
    fn pump_watchpoints(&mut self) {
        let mut step_satisfied = false;
        let mut step_still_pending = false;
        {
            let Some(dbg) = self.debugger.as_mut() else {
                return;
            };
            let mut guard = self.emu.lock();
            if let Some(nes) = guard.nes.as_mut() {
                dbg.pump_watchpoints(nes);
                // v1.7.0 "Forge" Workstream C (C1) — a queued step verb
                // (step-over / step-out / run-to-NMI / run-to-IRQ) was satisfied
                // this frame; take the edge so we pause below.
                step_satisfied = dbg.take_step_satisfied();
                step_still_pending = dbg.step_pending();
            }
        } // emu lock + debugger borrow dropped here
        if step_satisfied {
            self.set_paused(true);
            if let Some(d) = self.debugger.as_mut() {
                d.open_chip_panel(crate::debugger::ChipPanel::Cpu);
            }
            self.ui.set_status(crate::ui_shell::StatusMessage::info(
                "Step complete — paused".to_owned(),
            ));
        } else if step_still_pending && self.ui.paused {
            // The step verb isn't satisfied yet: keep advancing frame-by-frame
            // (the user is paused; this drives the step to completion without
            // resuming free-running play).
            self.request_frame_advance();
        }
    }

    /// v1.2.0 Workstream F4 — load a Lua script into the EXPERIMENTAL wasm
    /// (piccolo) engine, replacing any running one. Called from the
    /// `wasm_load_script` JS bridge. Logs to the browser console; piccolo is
    /// not byte-parity with the native mlua engine (ADR 0012).
    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
    pub fn load_script_wasm(&mut self, src: &str) {
        self.script_wasm = None;
        self.script_draws_wasm.clear();
        let mut engine = match rustynes_script_wasm::ScriptEngine::new() {
            Ok(e) => e,
            Err(e) => {
                Self::wasm_console_warn(&format!("[script] engine init failed: {e}"));
                return;
            }
        };
        match engine.load(src) {
            Ok(()) => {
                self.script_wasm = Some(engine);
                Self::wasm_console_warn("[script] loaded (experimental piccolo backend)");
            }
            Err(e) => Self::wasm_console_warn(&format!("[script] load error: {e}")),
        }
    }

    /// v1.2.0 Workstream F4 — pump the EXPERIMENTAL wasm (piccolo) engine for
    /// one produced frame: gate writes under a browser-netplay session, run the
    /// `onFrame` callbacks under the live `Nes`, then stash the overlay draws +
    /// surface log/errors to the browser console. Mirrors the native
    /// [`Self::pump_scripts`] minus the native-only console / file-dialog UI.
    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
    fn pump_scripts_wasm(&mut self) {
        // A JS `rustynes_load_script` / `rustynes_stop_script` arrives via the
        // `wasm_script` thread-local bridge; apply it before pumping.
        if let Some(src) = crate::wasm_script::take_pending() {
            if src.is_empty() {
                self.script_wasm = None;
                self.script_draws_wasm.clear();
                Self::wasm_console_warn("[script] stopped");
            } else {
                self.load_script_wasm(&src);
            }
        }
        if self.script_wasm.is_none() {
            return;
        }
        // Browser netplay is the wasm analog of the native lock (TAS / movie
        // replay is native-only on wasm), so it is the sole write-gate here.
        let netplay_locked = self
            .browser_netplay
            .as_ref()
            .is_some_and(crate::wasm_netplay::BrowserNetplay::is_active);
        let engine = self.script_wasm.as_mut().expect("checked is_some");
        let mut err = None;
        {
            let mut guard = self.emu.lock();
            let movie_locked = guard.movie.is_playing() || guard.movie.is_recording();
            let writes_locked = netplay_locked || movie_locked;
            let Some(nes) = guard.nes.as_mut() else {
                return;
            };
            engine.set_writes_locked(writes_locked);
            if let Err(e) = engine.on_frame(nes) {
                err = Some(e.to_string());
            }
        }
        let log = engine.drain_log();
        // Control commands are accepted but applied minimally on wasm (pause /
        // save / load state route through the existing handlers); setInput is
        // intentionally NOT wired on wasm in this first cut (documented).
        let _controls = engine.drain_controls();
        self.script_draws_wasm = engine.drain_draws();
        for line in log {
            Self::wasm_console_warn(&format!("[script] {line}"));
        }
        if let Some(e) = err {
            Self::wasm_console_warn(&format!("[script] runtime error: {e}"));
        }
    }

    /// v1.5.0 "Lens" Workstream G — paint the loud, persistent casual-only
    /// browser `RetroAchievements` caveat (ADR 0015) as a top banner. `banner`
    /// is `None` only if the session was dropped; otherwise it always says
    /// casual-only + experimental (and, when the auth proxy is not configured,
    /// that login + unlocks are unavailable). The banner is intentionally
    /// unmissable: hardcore is native-only and the browser path is experimental.
    #[cfg(all(target_arch = "wasm32", feature = "browser-cheevos"))]
    fn paint_browser_ra_caveat(ctx: &egui::Context, banner: Option<&'static str>) {
        let Some(text) = banner else { return };
        egui::Window::new("RetroAchievements (browser)")
            .collapsible(false)
            .resizable(false)
            .anchor(egui::Align2::CENTER_TOP, egui::vec2(0.0, 8.0))
            .show(ctx, |ui| {
                ui.colored_label(egui::Color32::from_rgb(0xFF, 0xB0, 0x40), text);
            });
    }

    /// v1.2.0 Workstream F4 — paint the wasm script's overlay draws through the
    /// egui pass. Mirrors [`Self::paint_script_overlay`] for the
    /// `rustynes_script_wasm::DrawCmd` type (the two `DrawCmd`s are distinct
    /// because the wasm engine is a separate crate alias).
    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::suboptimal_flops
    )]
    fn paint_script_overlay_wasm(
        ctx: &egui::Context,
        draws: &[rustynes_script_wasm::DrawCmd],
        par_8_7: bool,
        hide_overscan: bool,
    ) {
        use rustynes_script_wasm::DrawCmd;
        if draws.is_empty() {
            return;
        }
        let screen = ctx.content_rect();
        let crop_top = if hide_overscan { 8.0 } else { 0.0 };
        let visible_h = if hide_overscan { 224.0 } else { 240.0 };
        let img_w = if par_8_7 { 256.0 * 8.0 / 7.0 } else { 256.0 };
        let nes_aspect = img_w / visible_h;
        let win_aspect = screen.width() / screen.height().max(1.0);
        let (game_w, game_h) = if win_aspect > nes_aspect {
            (screen.height() * nes_aspect, screen.height())
        } else {
            (screen.width(), screen.width() / nes_aspect)
        };
        let origin_x = screen.min.x + (screen.width() - game_w) * 0.5;
        let origin_y = screen.min.y + (screen.height() - game_h) * 0.5;
        let sx = game_w / 256.0;
        let sy = game_h / visible_h;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("lua_script_overlay_wasm"),
        ));
        let col = |c: u32| {
            egui::Color32::from_rgba_unmultiplied(
                (c >> 24) as u8,
                (c >> 16) as u8,
                (c >> 8) as u8,
                c as u8,
            )
        };
        let p = |x: i32, y: i32| {
            egui::pos2(
                origin_x + x as f32 * sx,
                origin_y + (y as f32 - crop_top) * sy,
            )
        };
        for d in draws {
            match d {
                DrawCmd::Text { x, y, color, text } => {
                    painter.text(
                        p(*x, *y),
                        egui::Align2::LEFT_TOP,
                        text,
                        egui::FontId::monospace(10.0 * sy.max(1.0)),
                        col(*color),
                    );
                }
                DrawCmd::Rect { x, y, w, h, color } => {
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            p(*x, *y),
                            egui::vec2(*w as f32 * sx, *h as f32 * sy),
                        ),
                        0.0,
                        col(*color),
                    );
                }
                DrawCmd::Pixel { x, y, color } => {
                    painter.rect_filled(
                        egui::Rect::from_min_size(p(*x, *y), egui::vec2(sx.max(1.0), sy.max(1.0))),
                        0.0,
                        col(*color),
                    );
                }
            }
        }
    }

    /// Best-effort browser-console warn (wasm script logging sink).
    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
    fn wasm_console_warn(msg: &str) {
        web_sys::console::warn_1(&wasm_bindgen::JsValue::from_str(msg));
    }

    /// Handle a console load/reload/stop request.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    fn handle_script_action(&mut self, action: crate::debugger::ScriptAction) {
        use crate::debugger::ScriptAction;
        match action {
            ScriptAction::Load => {
                if let Some(path) = rfd::FileDialog::new()
                    .add_filter("Lua script", &["lua"])
                    .pick_file()
                {
                    self.load_script_from_path(&path);
                }
            }
            ScriptAction::Reload => {
                let label = self
                    .debugger
                    .as_mut()
                    .map(|d| d.script_panel().loaded_label().to_owned())
                    .unwrap_or_default();
                if !label.is_empty() {
                    self.load_script_from_path(&PathBuf::from(label));
                }
            }
            ScriptAction::Stop => {
                self.script = None;
                self.script_draws.clear();
                if let Some(dbg) = self.debugger.as_mut() {
                    let p = dbg.script_panel();
                    p.set_unloaded();
                    p.push_log(["[script stopped]".to_owned()]);
                }
            }
        }
    }

    /// Read + load a `.lua` file into a fresh engine, reporting to the console.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    fn load_script_from_path(&mut self, path: &Path) {
        // Drop any running script up front so a failed (re)load can't leave the
        // old script's callbacks + overlay running behind an error (gemini #48).
        self.script = None;
        self.script_draws.clear();
        let src = match std::fs::read_to_string(path) {
            Ok(s) => s,
            Err(e) => {
                if let Some(dbg) = self.debugger.as_mut() {
                    dbg.script_panel().set_error(format!("read failed: {e}"));
                }
                return;
            }
        };
        let mut engine = match rustynes_script::ScriptEngine::new() {
            Ok(e) => e,
            Err(e) => {
                if let Some(dbg) = self.debugger.as_mut() {
                    dbg.script_panel().set_error(format!("engine init: {e}"));
                }
                return;
            }
        };
        // v1.7.0 "Forge" Workstream B (B3) — point `emu.getScriptDataFolder()`
        // at a sandboxed `scripts/` dir under the app data dir (created lazily by
        // a script that writes there). The clean persist-without-arbitrary-FS
        // path; never touches deterministic state.
        if let Some(dir) = crate::config::Config::default_data_dir() {
            let scripts = dir.join("scripts");
            engine.set_script_data_folder(Some(scripts.display().to_string()));
        }
        match engine.load(&src) {
            Ok(()) => {
                let cbs = engine.frame_callback_count();
                // v1.5.0 B4 — seed the engine's `sym:` query tables with any
                // already-loaded debugger symbols (read-only; never deterministic).
                if let Some(dbg) = self.debugger.as_ref() {
                    let pairs = dbg.symbol_pairs();
                    if !pairs.is_empty() {
                        engine.set_symbols(&pairs);
                    }
                }
                self.script = Some(engine);
                // v1.7.0 "Forge" E1 — spawn the host-mediated IPC bridge that
                // owns the `comm.*` connections (off the emu lock). The Lua
                // sandbox never gets a socket; the host marshals plain values.
                #[cfg(feature = "script-ipc")]
                {
                    self.script_host = Some(crate::script_host::ScriptHost::new());
                }
                if let Some(dbg) = self.debugger.as_mut() {
                    let p = dbg.script_panel();
                    p.set_loaded(path.display().to_string(), cbs);
                    p.push_log([format!("[loaded {}]", path.display())]);
                    dbg.open_chip_panel(crate::debugger::ChipPanel::Script);
                }
            }
            Err(e) => {
                if let Some(dbg) = self.debugger.as_mut() {
                    dbg.script_panel().set_error(format!("load error: {e}"));
                    dbg.open_chip_panel(crate::debugger::ChipPanel::Script);
                }
            }
        }
    }

    /// Paint the script's overlay draw commands through the egui pass, mapped
    /// onto the **actual letterboxed game rect** (L3): the NES image is fit into
    /// the window preserving its (optionally 8:7-corrected) aspect, matching the
    /// wgpu blit's `letterbox_uniform`, so script HUD coords line up with game
    /// pixels. `par_8_7` / `hide_overscan` mirror the live present settings.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    #[allow(
        clippy::cast_possible_truncation,
        clippy::cast_precision_loss,
        clippy::suboptimal_flops
    )] // color byte-extract + px coord mapping.
    fn paint_script_overlay(
        ctx: &egui::Context,
        draws: &[rustynes_script::DrawCmd],
        par_8_7: bool,
        hide_overscan: bool,
    ) {
        use rustynes_script::DrawCmd;
        if draws.is_empty() {
            return;
        }
        let screen = ctx.content_rect();
        // Visible NES region + its display aspect (replicates gfx letterbox).
        let crop_top = if hide_overscan { 8.0 } else { 0.0 };
        let visible_h = if hide_overscan { 224.0 } else { 240.0 };
        let img_w = if par_8_7 { 256.0 * 8.0 / 7.0 } else { 256.0 };
        let nes_aspect = img_w / visible_h;
        let win_aspect = screen.width() / screen.height().max(1.0);
        // Fit the NES image into the window, preserving aspect, centered.
        let (game_w, game_h) = if win_aspect > nes_aspect {
            (screen.height() * nes_aspect, screen.height())
        } else {
            (screen.width(), screen.width() / nes_aspect)
        };
        let origin_x = screen.min.x + (screen.width() - game_w) * 0.5;
        let origin_y = screen.min.y + (screen.height() - game_h) * 0.5;
        // One framebuffer pixel in screen points (x over 256, y over visible_h).
        let sx = game_w / 256.0;
        let sy = game_h / visible_h;
        let painter = ctx.layer_painter(egui::LayerId::new(
            egui::Order::Foreground,
            egui::Id::new("lua_script_overlay"),
        ));
        let col = |c: u32| {
            egui::Color32::from_rgba_unmultiplied(
                (c >> 24) as u8,
                (c >> 16) as u8,
                (c >> 8) as u8,
                c as u8,
            )
        };
        // Map a framebuffer coord into the game rect (y is relative to the
        // visible window so overscan-cropped scanlines map correctly).
        let p = |x: i32, y: i32| {
            egui::pos2(
                origin_x + x as f32 * sx,
                origin_y + (y as f32 - crop_top) * sy,
            )
        };
        for d in draws {
            match d {
                DrawCmd::Text { x, y, color, text } => {
                    painter.text(
                        p(*x, *y),
                        egui::Align2::LEFT_TOP,
                        text,
                        egui::FontId::monospace(10.0 * sy.max(1.0)),
                        col(*color),
                    );
                }
                DrawCmd::Rect { x, y, w, h, color } => {
                    painter.rect_filled(
                        egui::Rect::from_min_size(
                            p(*x, *y),
                            egui::vec2(*w as f32 * sx, *h as f32 * sy),
                        ),
                        0.0,
                        col(*color),
                    );
                }
                DrawCmd::Pixel { x, y, color } => {
                    painter.rect_filled(
                        egui::Rect::from_min_size(p(*x, *y), egui::vec2(sx.max(1.0), sy.max(1.0))),
                        0.0,
                        col(*color),
                    );
                }
            }
        }
    }

    /// Apply one script-issued control command to the emulator.
    ///
    /// `writes_locked` is the SAME determinism gate `emu.write` uses
    /// (`netplay_locked || movie_locked`, which already folds in
    /// `ra_hardcore_blocks()` via `netplay_locked`). `SetInput` honors it so a
    /// script can never perturb a netplay / TAS-replay / RA-hardcore session.
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    fn apply_script_control(&mut self, cmd: &rustynes_script::ControlCmd, writes_locked: bool) {
        use rustynes_script::ControlCmd;
        match cmd {
            ControlCmd::Pause => self.set_paused(true),
            ControlCmd::SaveState(slot) => self.handle_save_state(*slot),
            ControlCmd::LoadState(slot) => {
                if !self.ra_hardcore_blocks() {
                    self.handle_load_state(*slot);
                }
            }
            // v1.2.0 (T-110-E2) — stash the per-port override on the core; it is
            // merged at the next `EmuCore::latch` (the deterministic late-latch
            // point a real keypress enters at) and consumed there. GATED
            // identically to `emu.write`: under lock the override is NEVER
            // stored, so `latch` stays byte-identical and a locked / replayed
            // session is provably unperturbed.
            ControlCmd::SetInput { port, buttons } => {
                if writes_locked {
                    return;
                }
                if *port < 2 {
                    self.emu.lock().script_input_override[*port as usize] = Some(*buttons);
                }
            }
            // v1.7.0 "Forge" Workstream B (B3) — `emu.takeScreenshot()`: write
            // the current frame to a PNG (the host owns the encoder). A
            // read-only side effect, so it is NOT write-gated.
            ControlCmd::Screenshot => self.take_screenshot(),
        }
    }

    /// v1.7.0 "Forge" Workstream E2 — apply one drained `client.*` automation
    /// verb. Mirrors [`Self::apply_script_control`]: the host stays the single
    /// owner of window / tool / capture / cheat state. The observational verbs
    /// (open tool, screenshot, window size, speed, frameskip, A/V pause) are
    /// presentation-only and never perturb the deterministic core; the
    /// state-changing verbs (`reboot_core`, cheats) re-check `writes_locked` as
    /// defence in depth — the engine already dropped them at the source when
    /// locked (so a netplay / TAS / RA-hardcore session is provably unperturbed).
    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
    fn apply_script_client(&mut self, cmd: &rustynes_script::ClientCmd, writes_locked: bool) {
        use rustynes_script::ClientCmd;
        match cmd {
            ClientCmd::OpenTool(name) => {
                let panel = match name.to_ascii_lowercase().as_str() {
                    "cpu" => Some(crate::debugger::ChipPanel::Cpu),
                    "ppu" => Some(crate::debugger::ChipPanel::Ppu),
                    "oam" => Some(crate::debugger::ChipPanel::Oam),
                    "apu" => Some(crate::debugger::ChipPanel::Apu),
                    "memory" | "hex" => Some(crate::debugger::ChipPanel::Memory),
                    "mapper" => Some(crate::debugger::ChipPanel::Mapper),
                    "trace" => Some(crate::debugger::ChipPanel::Trace),
                    "watch" => Some(crate::debugger::ChipPanel::Watch),
                    "events" => Some(crate::debugger::ChipPanel::Events),
                    "script" | "lua" => Some(crate::debugger::ChipPanel::Script),
                    _ => None,
                };
                if let (Some(panel), Some(dbg)) = (panel, self.debugger.as_mut()) {
                    dbg.open_chip_panel(panel);
                }
            }
            ClientCmd::Screenshot => self.take_screenshot(),
            ClientCmd::ScreenshotToClipboard => self.screenshot_to_clipboard(),
            ClientCmd::SetWindowSize(scale) => self.set_window_scale(*scale),
            ClientCmd::SpeedMode(pct) => {
                // `pct` is a percentage (100 = realtime); `set_speed` clamps.
                #[allow(clippy::cast_precision_loss)]
                self.set_speed(*pct as f32 / 100.0);
            }
            ClientCmd::FrameSkip(n) => {
                // RustyNES renders every frame (no frame-skip pipeline today);
                // record the request rather than silently dropping it.
                if let Some(dbg) = self.debugger.as_mut() {
                    dbg.script_panel()
                        .push_log([format!("[client.frameskip({n}) — not yet supported]")]);
                }
            }
            ClientCmd::PauseAv | ClientCmd::UnpauseAv => {
                // The A/V recorder is start/stop only (no pause); surface the
                // intent without faking a capability.
                if let Some(dbg) = self.debugger.as_mut() {
                    dbg.script_panel()
                        .push_log(["[client A/V pause — recorder is start/stop only]".to_string()]);
                }
            }
            ClientCmd::RebootCore => {
                if !writes_locked {
                    self.do_power_cycle();
                }
            }
            ClientCmd::AddCheat(code) => {
                if !writes_locked {
                    let mut guard = self.emu.lock();
                    if let Some(nes) = guard.nes.as_mut()
                        && let Err(e) = nes.add_genie_code(code)
                    {
                        drop(guard);
                        if let Some(dbg) = self.debugger.as_mut() {
                            dbg.script_panel()
                                .push_log([format!("[client.addcheat {code} skipped: {e}]")]);
                        }
                    }
                }
            }
            ClientCmd::RemoveCheat(code) => {
                if !writes_locked {
                    let mut guard = self.emu.lock();
                    if let Some(nes) = guard.nes.as_mut() {
                        nes.remove_genie_code(code);
                    }
                }
            }
        }
    }

    /// v1.1.0 beta.2 (T-110-D2) — push the configured graphic-EQ params into the
    /// shared audio queue; the producer rebuilds its biquads on the next push.
    /// Applied at startup + on every EQ edit. Off (default) = byte-identical
    /// output. No-op when audio is disabled.
    #[cfg(not(target_arch = "wasm32"))]
    fn apply_audio_eq(&self) {
        if let Some(audio) = self.audio.as_ref() {
            // v1.7.0 H3 — push both the 5-band and 20-band gains + the mode flag.
            // Flat / off (the default) is byte-identical to a no-EQ build.
            audio.queue.set_eq_full(
                self.config.audio.eq_enabled,
                self.config.audio.eq_20_band,
                self.config.audio.eq_bands,
                self.config.audio.eq_bands_20,
            );
        }
    }

    /// v1.7.0 "Forge" H3 — push the configured stereo output DSP params
    /// (per-channel pan, reverb mix/room, headphone crossfeed) into the shared
    /// audio queue; the cpal callback rebuilds its reverb on the next change.
    /// Applied at startup + on every edit. Center pan / 0 reverb / 0 crossfeed
    /// (the default) is a true bypass → byte-identical output. No-op when audio
    /// is disabled.
    #[cfg(not(target_arch = "wasm32"))]
    fn apply_stereo_dsp(&self) {
        if let Some(audio) = self.audio.as_ref() {
            audio.queue.set_stereo(
                self.config.audio.pan,
                self.config.audio.reverb_mix,
                self.config.audio.reverb_room,
                self.config.audio.crossfeed,
            );
        }
    }

    /// v1.0.0 — push the configured per-APU-channel enable mask into the core
    /// under the emu lock (respecting the lock discipline). A UI playback
    /// overlay: the default `0x3F` (all six channels on) is byte-identical to
    /// today's mixer output, so the deterministic per-frame audio is unchanged
    /// unless a channel is explicitly muted. Cheap; called at startup, on every
    /// channel-checkbox edit, and after each fresh ROM load (a new `Nes` boots
    /// with the all-on default, so the mask must be re-pushed).
    fn apply_apu_channel_mask(&self) {
        let mask = self.config.audio.channel_mask;
        let mut guard = self.emu.lock();
        if let Some(nes) = guard.nes.as_mut() {
            nes.set_apu_channel_mask(mask);
        }
    }

    /// v1.4.0 Workstream C — push the configured per-APU-channel output gain into
    /// the running core (under the emu lock, respecting the lock discipline). A UI
    /// mixing overlay generalizing [`Self::apply_apu_channel_mask`]: the default
    /// (all `1.0`) is byte-identical to today's mixer output, so the deterministic
    /// per-frame audio + the oracle stay byte-identical unless a slider is moved.
    /// Cheap; called at startup, on every gain-slider edit, and after each fresh
    /// ROM load (a new `Nes` boots at unity, so the gains must be re-pushed).
    fn apply_apu_channel_gain(&self) {
        let gain = self.config.audio.channel_gain;
        let mut guard = self.emu.lock();
        if let Some(nes) = guard.nes.as_mut() {
            nes.set_apu_channel_gain(gain);
        }
    }

    /// v2.1.3 — push the configured APU analog output-filter model to the core.
    /// Default (`"nes"`) is byte-identical to earlier builds; `"famicom"` /
    /// `"clean"` drop the aggressive 440 Hz high-pass for a fuller low end.
    /// Called on ROM load, after a power-cycle, and on a Settings change.
    fn apply_apu_filter_model(&self) {
        let model = crate::config::parse_filter_model(&self.config.audio.filter_model);
        let mut guard = self.emu.lock();
        if let Some(nes) = guard.nes.as_mut() {
            nes.set_apu_filter_model(model);
        }
    }

    /// v1.4.0 Workstream C — query the loaded mapper's expansion-audio chip name
    /// from the core and push it into the Settings panel, so the Audio tab shows
    /// the expansion-channel volume slider only for boards with on-cart audio.
    /// Called on each ROM load (after the `Nes` is built) and at startup.
    fn refresh_expansion_audio_chip(&mut self) {
        let chip = {
            let guard = self.emu.lock();
            guard.nes.as_ref().and_then(Nes::expansion_audio_chip)
        };
        if let Some(debugger) = self.debugger.as_mut() {
            debugger.set_expansion_audio_chip(chip);
        }
    }

    /// v1.1.0 beta.1 (T-110-A3) — load + apply the configured `.pal` palette to the
    /// running core (or clear it when none / unreadable). Called on startup and on
    /// ROM load so a configured palette survives a reload. Native-only (no
    /// filesystem on wasm); a no-op there.
    #[cfg_attr(
        target_arch = "wasm32",
        allow(
            clippy::unused_self,
            clippy::missing_const_for_fn,
            clippy::needless_pass_by_ref_mut
        )
    )]
    fn apply_palette_from_config(&mut self) {
        #[cfg(not(target_arch = "wasm32"))]
        {
            let pal = match self.config.graphics.palette_file.as_ref() {
                None => None,
                Some(path) => {
                    let loaded = std::fs::read(path)
                        .ok()
                        .and_then(|b| crate::config::parse_pal(&b));
                    if loaded.is_none() {
                        // Don't fail silently: a missing/corrupt `.pal` would
                        // otherwise leave a phantom filename in the config + UI.
                        // Surface it and clear the entry so we fall back to the
                        // built-in palette cleanly.
                        eprintln!(
                            "rustynes: palette file {} could not be loaded; using built-in palette",
                            path.display()
                        );
                        self.config.graphics.palette_file = None;
                        let _ = self.config.save();
                    }
                    loaded
                }
            };
            let mut guard = self.emu.lock();
            if let Some(nes) = guard.nes.as_mut() {
                nes.set_custom_palette(pal);
            }
        }
    }

    /// v1.5.0 "Lens" Workstream D1 — apply the active named palette to the core
    /// (or clear back to the built-in / legacy `.pal`). When an entry in the
    /// named bank is selected it takes precedence over the legacy `.pal` file;
    /// when nothing is selected we fall back to `apply_palette_from_config`
    /// (built-in or legacy file). Presentation-only; built-in/unselected is
    /// byte-identical.
    #[cfg_attr(
        target_arch = "wasm32",
        allow(clippy::missing_const_for_fn, clippy::needless_pass_by_ref_mut)
    )]
    fn apply_active_palette(&mut self) {
        // v2.1.2 F1.4 — the generated NTSC palette, when enabled, takes
        // precedence over the named bank + legacy `.pal` + built-in. It is a
        // pure function of the stored params, synthesized fresh here (cheap:
        // 64 colors, one-time on apply) and pushed as a custom base — the PPU
        // applies the same emphasis LUT it uses for any custom palette.
        if self.config.graphics.ntsc_palette_enabled {
            let params = self.config.graphics.ntsc_palette.to_params();
            let base = rustynes_core::rustynes_ppu::generate_base_palette(&params);
            let mut guard = self.emu.lock();
            if let Some(nes) = guard.nes.as_mut() {
                nes.set_custom_palette(Some(base));
            }
            return;
        }
        let base = self
            .config
            .graphics
            .active_palette
            .as_ref()
            .and_then(|name| self.config.graphics.palettes.palettes.get(name))
            .map(crate::config::CustomPalette::to_base);
        match base {
            Some(pal) => {
                let mut guard = self.emu.lock();
                if let Some(nes) = guard.nes.as_mut() {
                    nes.set_custom_palette(Some(pal));
                }
            }
            // No named palette selected — fall back to the built-in or the
            // legacy `.pal` file (handles the "cleared to built-in" case too).
            None => self.apply_palette_from_config(),
        }
    }

    /// v1.1.0 beta.1 — open a `.pal` file dialog; on a valid pick, apply it to the
    /// core + persist the path. Native-only (rfd's native picker).
    #[cfg(not(target_arch = "wasm32"))]
    fn pick_palette_dialog(&mut self) {
        let Some(path) = rfd::FileDialog::new()
            .add_filter("NES palette", &["pal"])
            .pick_file()
        else {
            return;
        };
        let Some(pal) = std::fs::read(&path)
            .ok()
            .and_then(|b| crate::config::parse_pal(&b))
        else {
            eprintln!(
                "rustynes: {} is not a valid .pal (>= 192 bytes)",
                path.display()
            );
            return;
        };
        {
            let mut guard = self.emu.lock();
            if let Some(nes) = guard.nes.as_mut() {
                nes.set_custom_palette(Some(pal));
            }
        }
        self.config.graphics.palette_file = Some(path.clone());
        if let Err(e) = self.config.save() {
            eprintln!("rustynes: could not persist palette path: {e}");
        } else {
            eprintln!("rustynes: palette loaded -> {}", path.display());
        }
    }

    /// v1.1.0 beta.1 — clear the custom palette back to the built-in one + persist.
    #[cfg(not(target_arch = "wasm32"))]
    fn clear_palette(&mut self) {
        {
            let mut guard = self.emu.lock();
            if let Some(nes) = guard.nes.as_mut() {
                nes.set_custom_palette(None);
            }
        }
        self.config.graphics.palette_file = None;
        let _ = self.config.save();
    }

    /// v1.0.0 — set the emulation-speed factor (one of [`Self::SPEED_PRESETS`],
    /// but any positive value is accepted + clamped by the core). Writes
    /// through to `EmuCore::speed`, centers the audio DRC band on the factor
    /// (so the resampler consumes `speed`x input — natural pitch shift, no
    /// overrun), re-resolves pacing (display-sync can't represent a fractional
    /// rate, so a non-1.0 speed forces wall-clock), and rebases the pacer so
    /// the change takes effect without a catch-up burst.
    fn set_speed(&mut self, speed: f32) {
        use crate::ui_shell::StatusMessage;
        let speed = speed.clamp(0.05, 16.0);
        self.speed = speed;
        {
            let mut guard = self.emu.lock();
            guard.speed = speed;
            // Rebase so the new period applies from "now" (no burst / no stall).
            guard.next_frame_time = Some(Instant::now());
        }
        // Center the audio DRC band on the speed factor so alt-speed audio is
        // pitch-shifted + glitch-free (the shared queue carries this across to
        // the emu thread's producer). Native-only; wasm uses its own ring.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(audio) = self.audio.as_ref() {
            audio.queue.set_base_ratio(speed);
        }
        // Display-sync ⇄ wallclock depends on whether speed == 1.0.
        #[cfg(not(target_arch = "wasm32"))]
        self.resolve_pacing();
        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
        let pct = (speed * 100.0).round() as u32;
        self.ui
            .set_status(StatusMessage::info(format!("Speed {pct}%")));
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// v1.0.0 — step the speed up (`+1`) or down (`-1`) through
    /// [`Self::SPEED_PRESETS`]. Snaps to the nearest preset first so a custom
    /// value still steps sensibly; clamps at the ends.
    fn step_speed(&mut self, up: bool) {
        // Find the current preset index (nearest preset to the live speed).
        let mut idx = 0usize;
        let mut best = f32::INFINITY;
        for (i, &p) in Self::SPEED_PRESETS.iter().enumerate() {
            let d = (p - self.speed).abs();
            if d < best {
                best = d;
                idx = i;
            }
        }
        let last = Self::SPEED_PRESETS.len() - 1;
        let next = if up {
            (idx + 1).min(last)
        } else {
            idx.saturating_sub(1)
        };
        self.set_speed(Self::SPEED_PRESETS[next]);
    }

    /// v1.0.0 — pause or resume emulation from the UX shell. On the emu-thread
    /// path this flips the thread's atomic gate; on the synchronous native +
    /// wasm paths the produce loop checks `self.ui.paused` directly.
    fn set_paused(&mut self, paused: bool) {
        use crate::ui_shell::StatusMessage;
        // v1.0.0 (BUG-4) — refuse to pause during a netplay session (it would
        // stall the rollback loop and desync the peer). Resume is always honored.
        if paused && self.netplay_is_active() {
            self.ui
                .set_status(StatusMessage::info("Cannot pause during netplay"));
            return;
        }
        // v1.7.0 "Forge" H2 — hardcore pause-gating: rcheevos throttles pausing
        // in hardcore to prevent pause-abuse. Defer the pause when not yet
        // allowed, telling the user how long remains. No-op in softcore /
        // feature-off (resume is always honored).
        if paused && let Some(frames) = self.ra_pause_gate() {
            // Use the loaded ROM's region frame duration (NTSC ~60, PAL/Dendy
            // ~50 Hz) so the remaining-time hint is correct off-NTSC, rather
            // than a hardcoded 60 fps. Fall back to ~60 Hz if no ROM is loaded.
            let frame_secs = self.emu.lock().frame_duration.as_secs_f64();
            let frame_secs = if frame_secs > 0.0 {
                frame_secs
            } else {
                1.0 / 60.0
            };
            let secs = f64::from(frames) * frame_secs;
            self.ui.set_status(StatusMessage::info(format!(
                "Pause held by hardcore mode ({secs:.1}s remaining)"
            )));
            return;
        }
        self.ui.paused = paused;
        // v1.7.1 — engage a sticky audio pause gate so the cpal callback plays
        // silence WITHOUT draining the buffered tail. Otherwise the producer
        // stops pushing while the callback keeps consuming: the ring drains and
        // the first short fill counts one (sticky) underrun — the spurious "one
        // underrun on resume" from the perf logs. The gate is sticky because
        // `pop_or_silence` would otherwise re-open the start-gate the instant
        // `avail >= start_threshold` (true in steady state at pause time) and
        // keep draining (#152 review). On resume we clear the gate so the
        // normal threshold-gated startup re-opens once the producer has
        // refilled — with the buffered tail intact. No-op effect when never
        // pausing, so steady-state playback is byte-identical.
        #[cfg(not(target_arch = "wasm32"))]
        if let Some(audio) = self.audio.as_ref() {
            if paused {
                audio.queue.gate_for_pause();
            } else {
                audio.queue.resume_from_pause();
            }
        }
        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
        if let Some(thread) = self.emu_thread.as_ref() {
            thread.control().set_user_paused(paused);
            // v1.0.0 (BUG-1) — on resume, wake the parked emu thread so it
            // observes the cleared flag immediately (instead of after its idle
            // park timeout).
            if !paused {
                thread.unpark();
            }
        }
        // v1.0.0 (BUG-1) — on resume, rebase the pacer to "now" so the producer
        // does not burst-catch-up the frames that elapsed while paused (mirrors
        // the netplay-leave rebase).
        //
        // v1.7.1 — rebasing `next_frame_time` makes `now - next` ~0 on the first
        // produce, so the emu-thread stall-threshold guard does NOT fire
        // `break_phase()`. The produced/presented interval rings still hold the
        // pre-pause `last` timestamp, so the very next `record_produced` would
        // log the *entire pause duration* as one frame interval — the spurious
        // `produced_max_ms` spike (675 ms / 1395 ms in the captured logs). Break
        // the interval phase here so the paused wall-clock gap is dropped, not
        // counted as a produced/presented frame time or a catch-up burst.
        if !paused {
            let mut guard = self.emu.lock();
            guard.next_frame_time = Some(Instant::now());
            guard.perf.break_phase();
        }
        self.ui.set_status(StatusMessage::info(if paused {
            "Paused"
        } else {
            "Resumed"
        }));
        // Keep the render loop alive so the status bar / overlay stay
        // responsive while paused.
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// v1.5.0 "Lens" Workstream I2 — run a resolved system action. Extracted
    /// from the keyboard-event handler so BOTH the normal path (key not claimed
    /// by egui) and the global-hotkey path (key claimed by egui — `Tab` Fast
    /// Forward, `\` Frame Advance — routed via `handle_system_key`) dispatch
    /// through one place. `event_loop` is needed only for the `Quit` exit.
    fn dispatch_sys_action(&mut self, act: SysAction, event_loop: &ActiveEventLoop) {
        match act {
            SysAction::Quit => {
                // v1.0.0 (BUG-3) — Esc (the Quit bind) must not hard-quit while
                // fullscreen: exit fullscreen first, only quit from windowed.
                if self.ui.fullscreen {
                    self.toggle_fullscreen();
                } else {
                    self.should_exit = true;
                    event_loop.exit();
                }
            }
            SysAction::SaveState => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_save_state(self.active_save_slot);
                #[cfg(target_arch = "wasm32")]
                self.handle_save_state_wasm(self.active_save_slot);
            }
            SysAction::LoadState => {
                // v2.7.0 — load-state disabled in RA hardcore mode; PR #75 (H1)
                // — also disabled while a movie records/plays (matches the greyed
                // menu item, so the hotkey can't bypass it).
                if self.ra_hardcore_blocks() {
                    self.toast_hardcore("Load state disabled (hardcore)");
                } else if self.replay_interaction_locked() {
                    self.ui.set_status(crate::ui_shell::StatusMessage::info(
                        "Load state disabled during movie",
                    ));
                } else {
                    #[cfg(not(target_arch = "wasm32"))]
                    self.handle_load_state(self.active_save_slot);
                    #[cfg(target_arch = "wasm32")]
                    self.handle_load_state_wasm(self.active_save_slot);
                }
            }
            SysAction::Rewind | SysAction::FastForward => {
                // No-op here. `InputState` already toggled the held flag; the
                // per-frame rewind step runs in `about_to_wait`, and the
                // fast-forward state is picked up via `publish_shared_input` (emu
                // thread) or read directly on the sync / wasm produce paths.
            }
            SysAction::Reset => self.do_reset(),
            SysAction::PowerCycle => self.do_power_cycle(),
            SysAction::ToggleDebug => {
                // v1.7.0 "Forge" beta.5 (#55) — the backtick (`` ` ``) key no
                // longer toggles the debugger overlay: every chip inspector now
                // opens directly from the Debug menu (v1.7.1 also removed the
                // vestigial Debug -> Show Debugger toggle). The key now toggles
                // the status-bar RetroAchievements read-out between its compact
                // and long-form variants — the only distinct content the retired
                // toolbar HUD carried.
                self.ui.ra_detail = !self.ui.ra_detail;
            }
            SysAction::OpenRom => {
                #[cfg(not(target_arch = "wasm32"))]
                self.open_rom_dialog();
                // On wasm32, ROM loading is wired through the browser-native
                // `<input type="file">` path; the in-app OpenRom action no-ops.
            }
            SysAction::MovieRecordToggle => {
                #[cfg(not(target_arch = "wasm32"))]
                self.handle_movie_record_toggle();
                #[cfg(target_arch = "wasm32")]
                self.handle_movie_record_toggle_wasm();
            }
            SysAction::MoviePlayToggle => {
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
            SysAction::DiskSwap => self.cycle_disk_side(),
            SysAction::InsertCoin => {
                // v2.5.0 — insert a Vs. System coin (acceptor #1). No-op for
                // non-Vs. games. The coin latch clears a few frames later.
                let mut guard = self.emu.lock();
                let emu = &mut *guard;
                if let Some(nes) = emu.nes.as_mut() {
                    nes.insert_coin(0);
                    emu.vs_coin_frames = VS_COIN_HOLD_FRAMES;
                } else if let Some(dual) = emu.dual.as_mut() {
                    // v2.1.2 F2.1 — coin into the MAIN cabinet acceptor.
                    dual.insert_coin(0);
                    emu.vs_coin_frames = VS_COIN_HOLD_FRAMES;
                }
            }
            SysAction::ToggleFullscreen => self.toggle_fullscreen(),
            SysAction::ToggleMenuBar => {
                self.ui.menu_visible = !self.ui.menu_visible;
                if let Some(gfx) = self.gfx.as_ref() {
                    gfx.window.request_redraw();
                }
            }
            SysAction::FrameAdvance => self.request_frame_advance(),
            SysAction::TogglePause => {
                // UX3 BUG-1 — the keyboard path to pause/resume (same as the
                // Emulation -> Pause menu item).
                self.set_paused(!self.ui.paused);
            }
            SysAction::SpeedUp => self.step_speed(true),
            SysAction::SpeedDown => self.step_speed(false),
            SysAction::SpeedReset => self.set_speed(1.0),
        }
    }

    /// Step the emulator exactly one frame. Only meaningful while paused
    /// (a single-step while running is a no-op so a stray press can't perturb
    /// live cadence) and never during a netplay session (it would desync the
    /// peer). Works on all three produce paths:
    /// - emu-thread: bump the control-block counter + `unpark` the thread so
    ///   its idle gate produces one unthrottled frame and re-parks;
    /// - synchronous native: produce one frame inline here;
    /// - wasm: produce one frame inline + re-arm the rAF loop.
    fn request_frame_advance(&mut self) {
        if !self.ui.paused || self.netplay_is_active() {
            return;
        }
        // Make sure the core has the latest input for this stepped frame.
        #[cfg(not(target_arch = "wasm32"))]
        self.pump_gamepad();
        self.latch_input();

        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
        {
            if self.emu_thread.is_some() {
                // Publish the freshly-latched input into the SharedInput too,
                // so the stepped frame sees the current buttons.
                self.publish_shared_input();
                if let Some(thread) = self.emu_thread.as_ref() {
                    thread.control().request_frame_advance();
                    thread.unpark();
                }
                return;
            }
            self.frame_advance_inline();
        }
        #[cfg(not(all(not(target_arch = "wasm32"), feature = "emu-thread")))]
        self.frame_advance_inline();
    }

    /// Produce exactly one frame synchronously (the synchronous-native + wasm
    /// frame-advance step). Shared by the non-emu-thread paths.
    fn frame_advance_inline(&mut self) {
        let now = Instant::now();
        self.produce_one_frame();
        {
            let mut guard = self.emu.lock();
            let emu = &mut *guard;
            emu.perf.record_produce_cost(now.elapsed());
            emu.perf.record_produced(Instant::now());
            // Stay paused: rebase the pacer to "now" so resuming play later
            // doesn't burst-catch-up the stepped frame's interval.
            emu.next_frame_time = Some(Instant::now());
        }
        #[cfg(not(target_arch = "wasm32"))]
        self.post_produce_housekeeping();
        if let Some(gfx) = self.gfx.as_ref() {
            gfx.window.request_redraw();
        }
    }

    /// v1.0.0 — toggle borderless fullscreen, tracking the state on the shell.
    fn toggle_fullscreen(&mut self) {
        self.ui.fullscreen = !self.ui.fullscreen;
        if let Some(gfx) = self.gfx.as_ref() {
            let mode = if self.ui.fullscreen {
                Some(winit::window::Fullscreen::Borderless(None))
            } else {
                None
            };
            gfx.window.set_fullscreen(mode);
        }
    }

    /// v1.0.0 — resize the window to `scale`x the NES resolution (View > Window
    /// Size). Exits fullscreen first (a fixed size while fullscreen is moot) and
    /// adds a small allowance for the menu + status bars so the emulated image
    /// area lands near the requested multiple. Native only.
    #[cfg(not(target_arch = "wasm32"))]
    fn set_window_scale(&mut self, scale: u32) {
        if self.ui.fullscreen {
            self.toggle_fullscreen();
        }
        let scale = scale.clamp(1, 8);
        // v1.0.0 (UX3 BUG-2) — the chrome (menu + status bars) is drawn as an
        // egui overlay at a FIXED readable size on top of the window; the game
        // letterboxes into whatever space is left (the drag-resize path already
        // does this correctly). So the requested size only needs to (a) be wide
        // enough that the menu bar isn't clipped — clamp the width up to
        // `MIN_CHROME_WIDTH` (at 1x the raw `NES_W * scale` of 256 px is far too
        // narrow, which clipped the menu and offset its hit-areas, the "mouse
        // desync") — and (b) leave `CHROME_HEIGHT` for the bars above the
        // `NES_H * scale` game area.
        let w = f64::from(NES_W * scale).max(MIN_CHROME_WIDTH);
        let h = f64::from(NES_H * scale) + CHROME_HEIGHT;
        let requested = winit::dpi::LogicalSize::new(w, h);
        // winit MAY return the granted physical size synchronously (in which
        // case no `Resized` event follows); otherwise the request triggers a
        // `Resized` that `window_event` feeds to egui + `gfx.resize`. Handle the
        // synchronous case here so egui's pointer hit-test stays aligned with the
        // render in both cases.
        let granted = self
            .gfx
            .as_ref()
            .and_then(|gfx| gfx.window.request_inner_size(requested));
        if let Some(granted) = granted {
            self.window_size = (granted.width.max(1), granted.height.max(1));
            if let Some(gfx) = self.gfx.as_mut() {
                gfx.resize(granted.width, granted.height);
            }
            if let Some(gfx) = self.gfx.as_ref() {
                gfx.window.request_redraw();
            }
        }
    }

    /// wasm: the canvas size is controlled by the page, not the app.
    #[cfg(target_arch = "wasm32")]
    #[allow(clippy::unused_self)]
    const fn set_window_scale(&self, _scale: u32) {}

    /// Power-cycle the running emulator (and keep `RetroAchievements` in sync).
    fn do_power_cycle(&mut self) {
        {
            let mut guard = self.emu.lock();
            if let Some(nes) = guard.nes.as_mut() {
                nes.power_cycle();
                // v1.0.0 (UX3 BUG-3) — re-apply the configured Game Genie codes
                // to the freshly cold-booted core (disjoint borrow: `guard.nes` +
                // the `debugger` field) so cheats keep working across a Power-
                // Cycle even with the Cheats panel closed. A no-op when no codes
                // are enabled (the no-cheat path stays byte-identical).
                if let Some(debugger) = self.debugger.as_mut() {
                    debugger.reapply_genie_codes(nes);
                    // v1.7.0 "Forge" Workstream C (C1/C2) — drop the stale call
                    // stack + access counters across a cold boot.
                    debugger.reset_debug_telemetry();
                }
                // v1.0.0 — `power_cycle` rebuilds the APU (all-on default), so
                // re-push the per-channel mute mask. Default 0x3F = byte-identical.
                nes.set_apu_channel_mask(self.config.audio.channel_mask);
                // v1.4.0 Workstream C — `power_cycle` rebuilds the APU at unity,
                // so re-push the per-channel gain. Default (all 1.0) =
                // byte-identical.
                nes.set_apu_channel_gain(self.config.audio.channel_gain);
            }
            // v1.7.0 "Forge" D1 — a cold boot restarts the session timeline.
            guard.history.clear();
            // v1.7.0 "Forge" H4 — a cold boot restarts the lag-frame tally.
            guard.reset_lag_frames();
        }
        // v1.0.0 (BUG-7) — a cold boot should RUN: clear any prior pause so the
        // status bar doesn't read "Paused" with a freshly-booted, running core.
        // (Reset / warm boot leaves the pause state alone — it's a softer action
        // and a paused user likely wants to stay paused across a reset.)
        if self.ui.paused {
            self.set_paused(false);
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
        let raw_local = self.input.player1();
        let turbo_mask = self.turbo_mask();
        let turbo_period = self.config.input.turbo_period;
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let Some(nes) = emu.nes.as_mut() else {
            return;
        };
        // v1.1.0 beta.1 (T-110-B2) — expand turbo on the LOCAL input before it
        // is sent: the gated bits are what cross the wire + are stored in the
        // rollback ring, so both peers replay them verbatim (deterministic).
        let local = crate::emu::apply_turbo(raw_local, nes.frame(), turbo_mask, turbo_period);
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
                // v1.6.0 H — HD-pack HD audio: peek `$4100` (side-effect-free)
                // and mix the selected OGG track into the drained buffer in
                // place before the DRC stage. Output-only; skipped when no audio
                // pack is loaded. Disjoint field borrows: `nes` is `emu.nes`,
                // the mixer + buffer are other `emu` fields.
                #[cfg(feature = "hd-pack")]
                if let Some(mixer) = emu.hd_audio.as_mut() {
                    let control = nes.cpu_bus_peek(crate::hd_audio::HD_AUDIO_CONTROL);
                    mixer.mix(&mut emu.audio_buf[..n], control);
                }
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
        let raw_local = self.input.player1();
        let turbo_mask = self.turbo_mask();
        let turbo_period = self.config.input.turbo_period;
        let mut guard = self.emu.lock();
        let emu = &mut *guard;
        let (Some(driver), Some(nes)) = (self.browser_netplay.as_mut(), emu.nes.as_mut()) else {
            return;
        };
        // v1.1.0 beta.1 (T-110-B2) — expand turbo on the local input keyed on
        // the emulated frame, so the bits sent to the peer replay verbatim.
        let local = crate::emu::apply_turbo(raw_local, nes.frame(), turbo_mask, turbo_period);
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
            NetplayRequest::Host { .. }
            | NetplayRequest::Join { .. }
            | NetplayRequest::Spectate { .. }
                if self.emu.lock().nes.is_none() =>
            {
                eprintln!("rustynes: netplay needs a loaded ROM first");
            }
            NetplayRequest::Host { .. }
            | NetplayRequest::Join { .. }
            | NetplayRequest::Spectate { .. }
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
            // v1.7.0 H8 — read-only spectator: same ROM + emu-thread plumbing as
            // Join, but the spectator never authors input (see `start_spectate`).
            NetplayRequest::Spectate { remote } => {
                let Some(rom_hash) = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256()) else {
                    return;
                };
                match remote.parse::<std::net::SocketAddr>() {
                    Ok(addr) => {
                        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
                        self.pause_emu_thread_for_netplay();
                        self.netplay.start_spectate(addr, rom_hash);
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
            // v1.0.0 — no ROM yet: keep the always-on UX shell (menu/status
            // bar/welcome modal) drawing + interactive. Re-render at ~30 Hz via
            // `WaitUntil` (so status toasts fade smoothly) rather than an
            // immediate `request_redraw` busy-loop. Native only; the wasm rAF
            // loop re-arms itself unconditionally in `pace_and_produce_wasm`.
            #[cfg(not(target_arch = "wasm32"))]
            {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(33),
                ));
                if let Some(gfx) = self.gfx.as_ref() {
                    gfx.window.request_redraw();
                }
            }
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
            // v1.0.0 (UX3 BUG-1) — user-paused (Emulation -> Pause) with a ROM
            // loaded and netplay inactive: the producer (emu thread OR the
            // synchronous pacer) is stopped, so it sends no `EmuFrame` pings and
            // re-arms no redraws. Without an independent heartbeat the menu bar
            // never repaints, so the "Resume" click (or a hover) is never
            // serviced and pause looks frozen. Drive the shell at ~30 Hz here
            // with `WaitUntil` + `request_redraw` so the menu stays fully
            // interactive while paused; the producer stays idle (no frames). This
            // MUST run before the `emu_thread_drives()` stand-down below — when
            // the thread drives, the parked thread is exactly what stops the
            // pings, so the winit thread has to own the paused redraw cadence.
            // (BUG-4) NEVER pause while netplay is active (stalling the rollback
            // session desyncs the peer); the Pause menu item is disabled then too.
            if self.ui.paused && !self.netplay.is_active() {
                event_loop.set_control_flow(ControlFlow::WaitUntil(
                    Instant::now() + Duration::from_millis(33),
                ));
                if let Some(gfx) = self.gfx.as_ref() {
                    gfx.window.request_redraw();
                }
                return;
            }

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

            // Fast-forward (synchronous-native path): skip the pacer block and
            // produce a capped burst of frames UNTHROTTLED with audio muted.
            // Applies in every (non-netplay) regime; netplay always takes the
            // exact-rate one-frame-per-pace path below. Stay on `Poll` so the
            // burst repeats immediately next `about_to_wait`.
            if self.input.fast_forward_held() && !self.netplay.is_active() {
                self.pump_gamepad();
                self.latch_input();
                self.produce_fast_forward_frames();
                self.post_produce_housekeeping();
                if let Some(gfx) = self.gfx.as_ref() {
                    gfx.window.request_redraw();
                }
                event_loop.set_control_flow(ControlFlow::Poll);
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
        let (fps, movie_status, region, mut perf_view) = {
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
            // v1.5.0 "Lens" Workstream H8 — surface the run-ahead + rewind
            // state the Performance panel/log previously omitted, captured
            // under the same lock as the perf view.
            view.run_ahead = self.config.input.run_ahead;
            view.run_ahead_throttled = emu.runahead_throttled;
            view.rewind_enabled = self.config.rewind.enabled;
            view.rewind_frames = emu.nes.as_ref().map_or(0, rustynes_core::Nes::rewind_len);
            let region = emu
                .nes
                .as_ref()
                .map_or(rustynes_core::Region::Ntsc, rustynes_core::Nes::region);
            (emu.current_fps(), emu.movie.status(), region, view)
        };
        let replay_info = self.build_replay_info(region);
        perf_view.pacing = self.pacing_label();
        // v1.5.0 H8 — the live audio DRC servo ratio + latency setpoint.
        if let Some(audio) = self.audio.as_ref() {
            perf_view.drc_ratio = audio.drc_ratio_now();
            perf_view.audio_latency_target_ms = audio.latency_target_ms();
        }
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
        // v1.1.0 beta.1 (Workstream B) — push the held-button snapshot for the
        // input-display HUD (P1..P4; 4 players shown only with Four Score).
        let input_players = if self.config.input.four_score { 4 } else { 2 };
        let input_pads = [
            self.input.player1(),
            self.input.player2(),
            self.input.player3(),
            self.input.player4(),
        ];
        // v1.5.0 "Lens" Workstream A1 — build the live Input Miniatures snapshot
        // (standard pads + whatever non-standard device occupies port 2) from the
        // host-side input state. Frontend-only; no core touch. Native resolves the
        // expansion device; wasm pushes a pads-only snapshot (the expansion live
        // state is native-only).
        #[cfg(not(target_arch = "wasm32"))]
        let miniatures = self.input_miniatures_snapshot(input_pads, input_players);
        #[cfg(target_arch = "wasm32")]
        let miniatures = crate::debugger::MiniaturesSnapshot {
            pads: input_pads,
            players: input_players,
            expansion: crate::debugger::ExpansionMini::None,
        };
        if let Some(debugger) = self.debugger.as_mut() {
            debugger.set_fps(fps);
            // v1.7.0 "Forge" beta.5 (#51) — the single consolidated "Input
            // Display" panel (standard pads + active expansion device).
            debugger.set_input_display(miniatures);
            debugger.set_movie_status(movie_status);
            debugger.set_replay_info(movie_status, replay_info);
            debugger.set_perf_log_note(log_note);
            debugger.set_perf_view(perf_view);
            // v1.7.0 — pull the live enabled raw-cheat list edited in the
            // cheat panel so the next produce iteration pokes the current
            // set (mirrors the fps / movie-status pull pattern).
            let raw_cheats = debugger.enabled_raw_cheats();
            // v1.7.0 "Forge" Workstream A1 — harvest the one-shot debugger
            // writeback edits (tile/CHR, palette, nametable, OAM, hex) queued
            // by the editing panels. They land in the SAME gated post-frame
            // poke stage as the raw cheats, under the SAME `emu.write` gate.
            let debug_pokes = debugger.take_debug_pokes();
            // Republish the combined write gate (netplay / RA-hardcore / TAS
            // replay or record) computed from the EXACT same condition
            // `emu.write` uses, so the produce path drops the edits when
            // locked (locked = no-op = byte-identical).
            let netplay_locked = self.netplay_is_active() || self.ra_hardcore_blocks();
            let mut guard = self.emu.lock();
            let movie_locked = guard.movie.is_playing() || guard.movie.is_recording();
            guard.writes_locked = netplay_locked || movie_locked;
            guard.raw_cheats = raw_cheats;
            guard.debug_pokes.extend(debug_pokes);
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

        // v1.0.0 — at a non-100% emulation speed the target rate is no longer
        // an integer multiple of the display refresh, so display-sync (one
        // emulated frame per refresh) cannot represent it. Force the
        // wall-clock pacer for the duration; speed 1.0 restores the configured
        // regime. (Same idea as the sustained-miss display-sync fallback.)
        #[allow(clippy::float_cmp)] // 1.0 is the exact preset value.
        let speed_locks_wallclock = self.emu.lock().speed != 1.0;

        let want = if speed_locks_wallclock {
            ActivePacing::Wallclock
        } else {
            match mode.as_str() {
                "display" => {
                    if within_skew && !self.display_fallback {
                        ActivePacing::Display
                    } else {
                        if !within_skew {
                            eprintln!(
                                "rustynes: pacing_mode=display requested but the monitor \
                             refresh ({}) is not within 0.5% of the console rate \
                             ({nominal_hz:.4} Hz) — using wallclock pacing.",
                                monitor_hz.map_or_else(
                                    || "unknown".to_string(),
                                    |hz| format!("{hz:.3} Hz")
                                )
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
            || self.ui.paused
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

    /// v1.1.0 beta.1 (T-110-A1) — select the gfx NTSC post-pass to match the
    /// `[graphics] ntsc_filter` mode, keeping the two NTSC filters mutually
    /// exclusive: `"composite-rt"` = the true composite Bisqwit filter,
    /// `"off"` = neither, any other non-`"off"` value = the simplified blur.
    fn apply_ntsc_mode(gfx: &mut Gfx, graphics: &crate::config::GraphicsConfig) {
        match graphics.ntsc_filter.as_str() {
            "off" => {
                gfx.disable_ntsc();
                gfx.disable_ntsc_bisqwit();
            }
            "composite-rt" => {
                gfx.disable_ntsc();
                gfx.enable_ntsc_bisqwit();
                // v1.2.0 C1 — seed the live picture knobs on enable (default 0 =
                // byte-identical to the pre-C1 decode).
                gfx.set_ntsc_bisqwit_knobs(Self::ntsc_knobs_from(graphics));
            }
            _ => {
                gfx.disable_ntsc_bisqwit();
                gfx.enable_ntsc();
            }
        }
    }

    /// v1.2.0 C1 — build the live Bisqwit NTSC picture knobs from the graphics
    /// config. Defaults (all 0) decode byte-identically to the pre-C1 filter.
    const fn ntsc_knobs_from(
        graphics: &crate::config::GraphicsConfig,
    ) -> crate::ntsc_bisqwit::NtscKnobs {
        crate::ntsc_bisqwit::NtscKnobs {
            contrast: graphics.ntsc_contrast,
            saturation: graphics.ntsc_saturation,
            brightness: graphics.ntsc_brightness,
            hue: graphics.ntsc_hue,
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

    /// Maximum frames to produce per pace iteration while fast-forwarding, so
    /// a held fast-forward key can never wedge the UI (the event loop still
    /// services input/resize/redraw between iterations).
    const FAST_FORWARD_MAX_FRAMES: u32 = 8;

    /// Produce up to [`Self::FAST_FORWARD_MAX_FRAMES`] frames back-to-back,
    /// UNTHROTTLED, with audio MUTED on the native lock-free ring (a `None`
    /// audio sink), so the producer can run ahead without overrunning the ring
    /// (the cpal callback plays its underrun-silence). Rebases the pacer to
    /// `now` after the burst so releasing fast-forward doesn't catch-up burst.
    /// The synchronous-native + wasm fast-forward drive. (On wasm the audio
    /// ring is a thread-local `AudioWorklet` ring that drops overruns, so the
    /// produced samples are simply discarded by the ring under fast-forward.)
    fn produce_fast_forward_frames(&mut self) {
        let inputs = self.frame_inputs();
        for _ in 0..Self::FAST_FORWARD_MAX_FRAMES {
            let t0 = Instant::now();
            let fx = {
                // Mute audio: pass `None` so the produce path pushes no
                // samples into the ring (native). RA still drives.
                #[cfg(not(target_arch = "wasm32"))]
                let mut sinks = crate::emu::FrameSinks {
                    audio: None,
                    #[cfg(feature = "retroachievements")]
                    ra: self.ra.as_mut(),
                };
                #[cfg(target_arch = "wasm32")]
                let mut sinks = crate::emu::FrameSinks {
                    _marker: core::marker::PhantomData,
                };
                self.emu.lock().produce_one_frame(&inputs, &mut sinks)
            };
            self.apply_produce_fx(fx);
            let mut guard = self.emu.lock();
            let emu = &mut *guard;
            emu.perf.record_produce_cost(t0.elapsed());
            emu.perf.record_produced(Instant::now());
        }
        // Rebase so leaving fast-forward resumes paced play from "now".
        self.emu.lock().next_frame_time = Some(Instant::now());
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
        // v1.0.0 (BUG-4) — never honor pause while a browser netplay session is
        // active (it would stall the rollback session and desync the peer).
        let netplay_active = self
            .browser_netplay
            .as_ref()
            .is_some_and(crate::wasm_netplay::BrowserNetplay::is_active);
        if self.emu.lock().nes.is_some() && (!self.ui.paused || netplay_active) {
            // Latch the browser-sourced input just before producing.
            self.latch_input();
            let now = Instant::now();
            // v2.8.0 Phase 6 — rAF display-sync: when the measured rAF cadence
            // matches the console rate (a ~60 Hz panel), produce exactly one
            // frame per rAF and let the audio DRC absorb the sub-percent rate
            // difference — eliminating the wall-clock-vs-rAF beat that dups /
            // drops a frame every ~9 s. Off during netplay (one-frame-per-tick
            // is driven below) and on non-60 Hz panels (wall-clock catch-up).
            let display_sync = !netplay_active && self.wasm_display_sync();
            // Fast-forward: outside netplay, run a capped burst unthrottled.
            let fast_forward = !netplay_active && self.input.fast_forward_held();

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
            } else if fast_forward {
                // Fast-forward: produce a capped burst back-to-back this rAF
                // tick (the cap stops a held key from wedging the page). The
                // AudioWorklet ring drops overruns, so the extra frames'
                // samples are simply discarded under fast-forward.
                self.produce_fast_forward_frames();
                true
            } else if display_sync {
                // One frame per rAF — the present is the clock (winit delivers
                // RedrawRequested on requestAnimationFrame).
                let t0 = Instant::now();
                self.produce_one_frame();
                let mut guard = self.emu.lock();
                let emu = &mut *guard;
                emu.perf.record_produce_cost(t0.elapsed());
                emu.perf.record_produced(now);
                // v1.0.0 — speed-scaled period (1.0 = console rate).
                emu.next_frame_time = Some(now + emu.effective_frame_duration());
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
                // v1.2.0 Workstream F4 — pump the EXPERIMENTAL wasm Lua engine
                // for this produced frame (after the frame, before present), so
                // its overlay draws are ready for the egui pass.
                #[cfg(feature = "script-wasm")]
                self.pump_scripts_wasm();

                // v1.7.0 "Forge" beta.5 Workstream H6 — refresh the live
                // share-link snapshot so "Copy share link" reflects any settings
                // the user changed this frame. Cheap (flat primitives + one
                // String clone); no determinism surface.
                crate::wasm_share::publish_live(&self.config);

                let (fps, movie_status, region, mut perf_view) = {
                    let emu = self.emu.lock();
                    let mut view = emu.perf.view();
                    view.target_ms = emu.frame_duration.as_secs_f32() * 1000.0;
                    let region = emu
                        .nes
                        .as_ref()
                        .map_or(rustynes_core::Region::Ntsc, rustynes_core::Nes::region);
                    (emu.current_fps(), emu.movie.status(), region, view)
                };
                let replay_info = self.build_replay_info(region);
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
                    debugger.set_replay_info(movie_status, replay_info);
                    debugger.set_perf_view(perf_view);
                    // v1.7.0 — pull the live enabled raw-cheat list edited in
                    // the cheat panel so the next produce iteration pokes the
                    // current set (mirrors the fps / movie-status pull).
                    let raw_cheats = debugger.enabled_raw_cheats();
                    // v1.7.0 "Forge" Workstream A1 — harvest the one-shot
                    // debugger writeback edits + republish the write gate (TAS
                    // replay/record; wasm has no native netplay/RA-hardcore).
                    let debug_pokes = debugger.take_debug_pokes();
                    let mut guard = self.emu.lock();
                    guard.writes_locked = guard.movie.is_playing() || guard.movie.is_recording();
                    guard.raw_cheats = raw_cheats;
                    guard.debug_pokes.extend(debug_pokes);
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
        // Sprint 5-3 — optional NTSC filter. v1.1.0 beta.1 — optional CRT filter
        // (mutually exclusive with NTSC; CRT wins if both are somehow configured).
        // v1.2.0 C2 — a non-empty composable shader stack takes priority over the
        // legacy single-select filters and OWNS the post-process path; an empty
        // stack (the default) leaves the legacy chain in place, byte-identically.
        if self.config.graphics.shader_stack.has_enabled_passes() {
            gfx.set_stack_ntsc_knobs(Self::ntsc_knobs_from(&self.config.graphics));
        }
        if !gfx.apply_shader_stack(&self.config.graphics.shader_stack) {
            if self.config.graphics.crt_filter {
                gfx.enable_crt(self.config.graphics.crt_scanline);
            } else {
                Self::apply_ntsc_mode(&mut gfx, &self.config.graphics);
            }
        }
        // Sprint 5-3 — egui debugger overlay.
        let surface_format = gfx.surface_format();
        let mut debugger = DebuggerOverlay::new(&gfx.device, gfx.window.as_ref(), surface_format);
        // v1.7.0 "Forge" H3 — populate the Audio settings device picker with the
        // enumerated cpal output devices (native-only; cheap one-time scan).
        #[cfg(not(target_arch = "wasm32"))]
        debugger.set_audio_output_devices(AudioOutput::output_device_names());
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

        // v1.5.0 "Lens" Workstream H7 — `RUSTYNES_PERF_LOG=1` auto-enables the
        // perf-logging checkbox at launch so a scripted SMB capture
        // (`scripts/perf/perf_capture.sh`) produces a CSV without UI
        // interaction; the regression gate then parses `produced_max` /
        // `underruns` / `catchup_bursts` from it. No effect when unset.
        #[cfg(not(target_arch = "wasm32"))]
        if std::env::var_os("RUSTYNES_PERF_LOG").is_some()
            && let Some(debugger) = self.debugger.as_mut()
        {
            debugger.force_perf_logging();
        }

        #[cfg(not(target_arch = "wasm32"))]
        {
            // Native: cpal audio + NES from the ROM bytes loaded in `new`.
            // v2.8.0 Phase 1 — the `[audio]` config is finally wired: the
            // preferred sample rate is requested from the device (falling
            // back to its default), and the latency target + DRC toggle
            // configure the resampler stage.
            // v1.5.0 "Lens" Workstream H4 — on a high-refresh panel the
            // wall-clock pacer's catch-up bursts dump several frames of audio
            // into the ring at once, so a 60 ms target underruns on the
            // recovery dip. Bump the effective latency target a one-time +20 ms
            // when the monitor refresh is well above the console rate, giving
            // the ring headroom to ride out a burst (capped by `try_new`'s
            // 250 ms clamp). The user's configured `[audio] latency_ms` is the
            // floor; this only ever raises it, and only for high-refresh hosts.
            let latency_ms = {
                let refresh_hz = self
                    .gfx
                    .as_ref()
                    .and_then(|g| g.window.current_monitor())
                    .and_then(|m| m.refresh_rate_millihertz())
                    .map_or(60.0, |mhz| f64::from(mhz) / 1000.0);
                if refresh_hz > 75.0 {
                    self.config.audio.latency_ms.saturating_add(20)
                } else {
                    self.config.audio.latency_ms
                }
            };
            let audio = match AudioOutput::try_new(
                Some(self.config.audio.sample_rate),
                latency_ms,
                self.config.audio.drc,
                self.config.audio.output_device.as_deref(),
            ) {
                Ok(a) => Some(a),
                Err(e) => {
                    eprintln!("rustynes: audio disabled: {e}");
                    None
                }
            };
            let sample_rate = audio.as_ref().map_or(44_100, |a| a.sample_rate);
            self.audio = audio;
            // v1.0.0 — apply the persisted master volume / mute to the output
            // gain now that the queue exists. Default (1.0, not muted) is a
            // no-op so the default sound is byte-identical.
            self.apply_audio_gain();
            // v1.1.0 beta.2 — apply the persisted graphic-EQ params (off by
            // default → byte-identical).
            self.apply_audio_eq();
            // v1.7.0 H3 — apply the persisted stereo DSP params (center pan /
            // 0 reverb / 0 crossfeed by default → bypass, byte-identical).
            self.apply_stereo_dsp();
            // v1.0.0 — push the persisted per-APU-channel mute mask to the core
            // (no-op if no ROM is loaded yet; re-applied on each ROM load).
            // Default 0x3F (all on) leaves the deterministic audio unchanged.
            self.apply_apu_channel_mask();
            // v1.4.0 Workstream C — push the persisted per-APU-channel gain.
            // Default (all 1.0) leaves the deterministic audio unchanged.
            self.apply_apu_channel_gain();
            self.apply_apu_filter_model();
            // v1.1.0 beta.1 / v1.5.0 D1 — re-apply the active palette (named
            // bank entry, else legacy .pal / built-in).
            self.apply_active_palette();
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
        // v1.1.0 beta.1 (T-110-B4) — per-game nametable mirroring override.
        Self::apply_game_db(&mut nes, &self.rom_bytes);
        // v1.7.0 "Forge" Workstream H4 — resolve + apply the per-game overlay's
        // post-construction settings (mirroring / Vs. DIP) for the CLI/startup
        // ROM too, layered on top of the game-DB / Vs.-DB results above. (The
        // overlay's load-time iNES header `overrides` are applied at the menu /
        // drag load chokepoint; a CLI ROM uses the unrewritten image — its
        // mirroring/DIP still take effect here.) Absent / inert ⇒ no-op.
        #[cfg(not(target_arch = "wasm32"))]
        {
            let per_game = crate::game_db::rom_crc32(&self.rom_bytes)
                .and_then(|crc| crate::per_game::resolve(crc, None));
            if let Some(cfg) = per_game.as_ref() {
                if let Some(m) = cfg
                    .overrides
                    .mirroring
                    .as_deref()
                    .and_then(crate::per_game::mirroring_from_token)
                {
                    nes.set_mirroring_override(Some(m));
                }
                if let Some(dip) = cfg.dip_switches {
                    nes.set_vs_dip(dip);
                }
            }
        }
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
        // v1.6.0 "Studio" A2 — a fresh ROM here invalidates any prior TAStudio
        // session (it anchored on the previous `Nes`).
        if let Some(d) = self.debugger.as_mut() {
            d.clear_tas_editor();
        }
        // v2.8.0 Phase 5 increment 3 — let the (idle) emulation thread start
        // producing now that the core holds a ROM. Set AFTER `nes` is in
        // place so the thread never produces on an empty core; `resolve_pacing`
        // above already published the regime + frame duration.
        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
        if let Some(thread) = self.emu_thread.as_ref() {
            thread.control().set_has_rom(true);
        }
        // v1.0.0 — a new ROM was loaded: drop the Save-States manager's cached
        // thumbnail textures so the next open rebuilds for the new game (and
        // the old game's GPU textures are freed).
        #[cfg(not(target_arch = "wasm32"))]
        self.save_states_ui.invalidate_all();
        // v1.6.0 — apply this ROM's persisted Game Genie cheats (native).
        #[cfg(not(target_arch = "wasm32"))]
        self.apply_cheats_for_current_rom();
        // v1.0.0 — re-push the per-APU-channel mute mask onto the fresh `Nes`
        // (booted all-on); default 0x3F = byte-identical audio.
        self.apply_apu_channel_mask();
        // v1.4.0 Workstream C — re-push the per-APU-channel gain onto the fresh
        // `Nes` (booted at unity); default (all 1.0) = byte-identical audio.
        self.apply_apu_channel_gain();
        self.apply_apu_filter_model();
        // v1.4.0 Workstream C — refresh the Settings panel's expansion-audio chip
        // label so the expansion-channel volume slider matches the loaded mapper.
        self.refresh_expansion_audio_chip();
        // v1.1.0 beta.1 / v1.5.0 D1 — re-apply the active palette (named bank
        // entry, else legacy .pal / built-in).
        self.apply_active_palette();
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
        NetplayPhase::Spectating => NetplayPhaseView::Spectating,
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
        spectator_pending: s.spectator_pending,
        message: s.message.clone(),
        diagnostics: s.diagnostics.clone(),
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
            self.config.ui.pixel_aspect_correction,
            self.config.graphics.hide_overscan,
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
            let par_correction = self.config.ui.pixel_aspect_correction;
            let hide_overscan = self.config.graphics.hide_overscan;
            wasm_bindgen_futures::spawn_local(async move {
                match Gfx::new(
                    window,
                    &present_mode,
                    max_frame_latency,
                    par_correction,
                    hide_overscan,
                )
                .await
                {
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

        // v1.0.0 (BUG-1) — native redraw pump. The frame loop is a
        // self-sustaining produce -> `EmuFrame` -> `request_redraw` ping-pong;
        // when paused (or pre-ROM idle) the emu thread parks and stops sending
        // `EmuFrame`, so no redraw is ever re-armed. egui only repaints inside
        // `RedrawRequested`, so without this an input event (e.g. clicking
        // "Resume") would never reach the shell. Pump a redraw whenever egui
        // wants to repaint after processing this event. wasm self-arms its rAF
        // loop, so this is native-only.
        #[cfg(not(target_arch = "wasm32"))]
        if let (Some(debugger), Some(gfx)) = (self.debugger.as_ref(), self.gfx.as_ref())
            && debugger.egui_wants_repaint()
        {
            gfx.window.request_redraw();
        }

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
                // v1.2.0 Workstream D — accumulate the SNES-mouse relative motion
                // from the cursor delta, scaled to NES pixels (the window maps to
                // the 256x240 screen). Drained per produced/published frame.
                if let Some((px, py)) = self.cursor_pos {
                    let (ww, wh) = self.window_size;
                    let dx = (position.x - px) * 256.0 / f64::from(ww.max(1));
                    let dy = (position.y - py) * 240.0 / f64::from(wh.max(1));
                    self.mouse_motion_accum.0 += dx;
                    self.mouse_motion_accum.1 += dy;
                }
                // Track the cursor for the Zapper aim / Vaus paddle position.
                self.cursor_pos = Some((position.x, position.y));
            }
            #[cfg(not(target_arch = "wasm32"))]
            WindowEvent::MouseInput { state, button, .. } => {
                // v1.0.0 — a click on the always-on shell (a menu / window /
                // status bar) must not also fire the Zapper / Vaus. Skip the NES
                // mouse-press when egui claimed the pointer.
                let egui_pointer = self
                    .debugger
                    .as_ref()
                    .is_some_and(DebuggerOverlay::wants_egui_input);
                if button == winit::event::MouseButton::Left && !egui_pointer {
                    self.mouse_pressed = state == winit::event::ElementState::Pressed;
                }
                // v1.2.0 Workstream D — the SNES mouse's right button.
                if button == winit::event::MouseButton::Right && !egui_pointer {
                    self.mouse_right_pressed = state == winit::event::ElementState::Pressed;
                }
            }
            WindowEvent::DroppedFile(path) => {
                // Native filesystem drag-and-drop. On wasm32 browser
                // file drops are a follow-up (the file picker is the
                // primary ROM-load path there).
                #[cfg(not(target_arch = "wasm32"))]
                {
                    // Accept any path with a `.nes` / `.fds` / `.unf` / `.unif`
                    // extension (case-insensitive). v2.2.0 added `.fds`; v1.6.0
                    // (E2) added the UNIF container.
                    let ok = path.extension().and_then(|e| e.to_str()).is_some_and(|e| {
                        e.eq_ignore_ascii_case("nes")
                            || e.eq_ignore_ascii_case("fds")
                            || e.eq_ignore_ascii_case("unf")
                            || e.eq_ignore_ascii_case("unif")
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
                // v1.0.0 — when the shell is interacting with this key, do NOT
                // also drive the NES controller / system hotkeys from it. The
                // always-on shell is interactive even with the debugger overlay
                // hidden. Three independent gates:
                //   - `wants_egui_input`: a settings text field is focused (the
                //     PREVIOUS-frame state — kept for the focused-field steady
                //     state).
                //   - (BUG-5) `shell_is_capturing`: an egui MENU / popup / modal
                //     is open. A dropped menu has no focused text widget, so the
                //     gate above misses it — arrows/Z/X/Enter would otherwise
                //     drive the NES while the menu is down.
                //   - (BUG-6) `egui_consumed`: THIS event was claimed by egui
                //     this frame. `wants_egui_input` reflects the previous
                //     `ctx.run`, so the first keystroke into a freshly-focused
                //     field is still false there; the current-event flag closes
                //     that one-frame leak.
                let shell_window_open = self.ui.show_settings_window
                    || self.ui.show_about
                    || self.ui.show_shortcuts
                    || self.ui.show_welcome;
                // v1.5.0 "Lens" Workstream I2 — split the "egui is busy" gate into
                // two distinct levels:
                //   - `text_input`: a real text widget has keyboard focus (a
                //     settings field, the Lua console, a search box, ...) OR a
                //     modal shell window is open. Here NOTHING reaches the
                //     emulator — neither the NES controller nor global hotkeys —
                //     because the keystroke is genuinely text the user is typing.
                //   - `egui_busy`: egui passively consumed the key (e.g. `Tab`,
                //     which egui claims for menu-bar / widget focus navigation) or
                //     a menu/popup is merely dropped. Here the NES controller is
                //     still suppressed (a key under an open menu must not drive the
                //     pad), but GLOBAL HOTKEYS MUST STILL FIRE — this is the I2 bug
                //     fix: `Tab` (Fast Forward) and `\` (Frame Advance) were eaten
                //     by egui's focus navigation before ever reaching the hotkey
                //     path. We now route them through `handle_system_key` (system
                //     bindings only — never the per-player NES maps).
                let text_input = shell_window_open
                    || self
                        .debugger
                        .as_ref()
                        .is_some_and(DebuggerOverlay::wants_egui_input);
                let egui_busy = egui_consumed
                    || self
                        .debugger
                        .as_ref()
                        .is_some_and(DebuggerOverlay::shell_is_capturing);
                if text_input {
                    return;
                }
                if egui_busy {
                    // System hotkeys only (FF / Frame Advance / save-state / ...).
                    // No NES controller bits, no Family BASIC matrix update.
                    let action = self
                        .input
                        .handle_system_key(event.physical_key, event.state);
                    if let Some(act) = action {
                        self.dispatch_sys_action(act, event_loop);
                    }
                    // Publish the (possibly changed) held-key state for the emu
                    // thread, but do NOT latch NES controller input from this key.
                    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
                    self.publish_shared_input();
                    return;
                }
                // v1.2.0 Workstream D — Family BASIC keyboard matrix: map the
                // host key to a matrix index and set/clear the row bit. Tracked
                // unconditionally (cheap); consumed only when a Family BASIC
                // keyboard is the active device. Native-only.
                #[cfg(not(target_arch = "wasm32"))]
                if let winit::keyboard::PhysicalKey::Code(code) = event.physical_key
                    && let Some(idx) = crate::input::family_keyboard_index(code)
                {
                    let row = idx / 8;
                    let bit = 1u8 << (idx % 8);
                    if event.state == winit::event::ElementState::Pressed {
                        self.family_keyboard[row] |= bit;
                    } else {
                        self.family_keyboard[row] &= !bit;
                    }
                }
                let action = self.input.handle_key(event.physical_key, event.state);
                if let Some(act) = action {
                    self.dispatch_sys_action(act, event_loop);
                }
                // BUG-6 — reaching here means this key was NOT claimed by egui as
                // text input, so the NES controller may latch it.
                // v2.8.0 Phase 5 increment 3 — when the emu thread drives, publish
                // the new input into its SharedInput immediately so a key press
                // doesn't wait a full frame for the next `EmuFrame` republish; the
                // direct latch is the synchronous-path write.
                #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
                self.publish_shared_input();
                self.latch_input();
            }
            WindowEvent::Focused(focused) => {
                // Pause-on-focus-loss (opt-in): auto-pause when the window
                // loses focus and auto-resume when it regains focus. Never
                // fights a manual user pause (only auto-resume what auto-pause
                // paused), and never auto-pauses during a netplay session
                // (stalling the rollback loop would desync the peer).
                if self.config.ui.pause_on_focus_loss && !self.netplay_is_active() {
                    if focused {
                        if self.auto_paused {
                            self.auto_paused = false;
                            self.set_paused(false);
                        }
                    } else if !self.ui.paused {
                        self.set_paused(true);
                        // `set_paused` refuses during netplay (guarded above)
                        // — only mark the auto-pause if it actually took.
                        self.auto_paused = self.ui.paused;
                    }
                }
            }
            WindowEvent::RedrawRequested => {
                // v1.3.0 Workstream B (B1) — timestamp the display-refresh
                // SIGNAL here, at the instant this RedrawRequested fired, and
                // feed THAT to `record_presented` on a successful present below
                // (not `Instant::now()` taken after `surface.present()` returns).
                // The post-present timestamp folded GPU-submit + vsync-gate +
                // coalesced-RedrawRequested jitter into the "Presented" series —
                // the cause of the panel "bottoming out then rushing to catch
                // up" while "Produced" stayed flat. The redraw signal is the
                // display's own refresh tick, so present-to-present deltas now
                // measure the display cadence. (Still recorded only on an actual
                // present — the Ok arm — so a skipped/early-returned redraw is
                // not counted as a presented frame.)
                let redraw_signal = Instant::now();
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

                // v1.1.0 beta.3 (Workstream E) — pump the Lua engine for this
                // redraw (after the frame is produced, before present), so its
                // overlay draws are ready for the egui pass below.
                #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                self.pump_scripts();

                // v1.6.0 "Studio" Workstream C — drive the debugger Watch panel's
                // observational replay (conditional breakpoints / watchpoints /
                // watch window / conditional trace). Runs after the frame is
                // produced; observational-only, so determinism holds.
                self.pump_watchpoints();

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
                // v1.0.0 — the egui pass now runs EVERY frame so the always-on
                // UX shell (menu bar / status bar / settings / modals) draws
                // whether or not the debugger overlay is toggled on. The shell
                // build closure never touches the emu lock; the conditional
                // debugger panels (which DO read `nes`) only build when the
                // overlay is visible.
                //
                // Lock policy is unchanged from v2.8.0 Phase 5: a VISIBLE
                // overlay holds the lock across the render (its panels need
                // `&mut Nes`); the common HIDDEN path copies the framebuffer
                // into the App staging buffer under a brief lock that is DROPPED
                // before the GPU encode + present.
                let dbg_visible = self
                    .debugger
                    .as_ref()
                    .is_some_and(DebuggerOverlay::is_visible);

                // The shell's status bar + menu IA need a snapshot of core
                // facts (rom loaded, fps, disk sides, Vs flag, mapper / region
                // labels). Capture them under a brief lock BEFORE the egui pass
                // so the build closure never re-locks. The owned `String`s live
                // in locals that outlive `shell_frame` (which borrows them).
                let (
                    rom_loaded,
                    fps,
                    disk_sides,
                    inserted_disk_side,
                    vs_system,
                    mapper_label,
                    region_label,
                    movie_recording,
                    movie_playing,
                    // v1.7.0 H4 — running lag-frame tally (independent of `nes`).
                    lag_frames,
                ) = {
                    let mut emu = self.emu.lock();
                    let f = emu.current_fps();
                    let rec = emu.movie.is_recording();
                    let play = emu.movie.is_playing();
                    let lag = emu.lag_frames();
                    emu.nes.as_mut().map_or_else(
                        || {
                            (
                                false,
                                f,
                                0usize,
                                None,
                                false,
                                String::new(),
                                String::new(),
                                rec,
                                play,
                                lag,
                            )
                        },
                        |nes| {
                            let region = match nes.region() {
                                rustynes_core::Region::Pal => "PAL",
                                rustynes_core::Region::Dendy => "Dendy",
                                rustynes_core::Region::Ntsc => "NTSC",
                            };
                            (
                                true,
                                f,
                                nes.disk_side_count(),
                                nes.inserted_disk_side(),
                                nes.is_vs_system(),
                                nes.mapper_info().name,
                                region.to_string(),
                                rec,
                                play,
                                lag,
                            )
                        },
                    )
                };
                let netplay_active = self.netplay_is_active();
                let run_ahead = self.config.input.run_ahead;
                // Keep the shell's save-slot mirror in sync with the app's.
                self.ui.active_slot = self.active_save_slot;
                let shell_frame = crate::ui_shell::ShellFrame {
                    rom_label: &self.rom_label,
                    rom_loaded,
                    fps,
                    netplay_active,
                    disk_sides,
                    inserted_disk_side,
                    vs_system,
                    mapper_label: &mapper_label,
                    region_label: &region_label,
                    run_ahead,
                    speed: self.speed,
                    paused: self.ui.paused,
                    movie_recording,
                    movie_playing,
                    // v1.6.0 "Studio" Workstream G — A/V recording armed flag
                    // (drives the Tools menu Record/Stop label). Always false on
                    // wasm / builds without the `av-record` feature.
                    av_recording: self.av_recording_active(),
                    // v1.7.0 "Forge" G5 — HD-Pack Builder recording flag (drives
                    // the HD Pack menu Start/Stop label). Always false on wasm /
                    // builds without the `hd-pack` feature.
                    hd_pack_building: {
                        #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                        {
                            self.hd_pack_builder.is_some()
                        }
                        #[cfg(not(all(feature = "hd-pack", not(target_arch = "wasm32"))))]
                        {
                            false
                        }
                    },
                    fast_forwarding: self.input.fast_forward_held(),
                    // v1.5.0 I7 — RA readout for the status bar (None unless the
                    // feature is on AND logged in). v1.7.0 "Forge" beta.5 (#55) —
                    // the backtick-toggled `ui.ra_detail` flag selects the
                    // long-form line (name/score/rich-presence/trackers) over the
                    // compact one.
                    ra_status: self.debugger.as_ref().and_then(|d| {
                        if self.ui.ra_detail {
                            d.ra_status_long()
                        } else {
                            d.ra_status_line()
                        }
                    }),
                    // v1.7.0 "Forge" beta.5 (#55) — the netplay read-out moved
                    // from the retired toolbar HUD to the status bar.
                    netplay_detail: self
                        .debugger
                        .as_ref()
                        .and_then(DebuggerOverlay::netplay_status_line),
                    // v1.7.0 "Forge" H4 — surface the lag-frame count only while a
                    // ROM is loaded (no ROM ⇒ no meaningful tally ⇒ hidden).
                    lag_frames: rom_loaded.then_some(lag_frames),
                };

                let mut shell_out = crate::ui_shell::ShellOutput::default();
                // v1.0.0 — Save-States manager inputs, captured BEFORE the
                // render branches so the `extra` egui closure can render it
                // without re-locking the emu (the locked branch holds the
                // guard across the pass). Native-only.
                #[cfg(not(target_arch = "wasm32"))]
                let ss_sha: Option<[u8; 32]> =
                    self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256());
                #[cfg(not(target_arch = "wasm32"))]
                let ss_dir: Option<PathBuf> = self.data_dir.clone();
                #[cfg(not(target_arch = "wasm32"))]
                let ss_slot = self.active_save_slot;
                // v1.4.0 E2 — the wasm Save-States grid needs only the active
                // slot + a rom-loaded flag (its slot data is held in the
                // `wasm_save_states` thread-local, populated by the async scan).
                #[cfg(target_arch = "wasm32")]
                let ss_slot_wasm = self.active_save_slot;
                #[cfg(target_arch = "wasm32")]
                let ss_rom_loaded_wasm = self.emu.lock().nes.is_some();
                // v1.0.0 — render-branch selection: take the LOCKED branch (which
                // passes a live `&mut Nes` to the egui pass) when EITHER the deep
                // overlay is visible OR a tool panel that reads `nes` (Cheats) is
                // open. Otherwise take the staging branch (no `nes`). This keeps
                // the Cheats panel functional with the overlay off without ever
                // taking a SECOND emu lock inside the egui closure.
                // v2.1.2 F2.1 — a Vs. `DualSystem` cabinet never takes the
                // `needs_nes` (single-`Nes` debugger / HD) branch; it composes both
                // console screens in the common branch below. The debugger + HD are
                // scoped out in dual mode (ADR 0032).
                let needs_nes = !self.dual_mode
                    && (dbg_visible
                        || self
                            .debugger
                            .as_ref()
                            .is_some_and(DebuggerOverlay::any_nes_tool_open));
                // v1.1.0 beta.1 (T-110-A1) — snapshot the palette-index
                // framebuffer + phase only while the true composite `NES_NTSC`
                // filter is active (zero cost otherwise). v1.2.0 C2 — also when a
                // leading composite-rt pass is active in the shader stack.
                // v2.1.2 F2.2 — `want_index` gates the (expensive) palette-index
                // framebuffer copy; `want_phase` gates the (cheap `u8`) colour-
                // phase snapshot. They differ because LMP88959 needs the live
                // phase for dot-crawl but samples RGBA (no index), so a Lmp-only
                // stack wants the phase without the index upload.
                let (want_index, want_phase) = self.gfx.as_ref().map_or((false, false), |g| {
                    let idx = g.ntsc_bisqwit_active() || g.shader_stack_needs_index();
                    (idx, idx || g.shader_stack_needs_phase())
                });
                // The early-return arm guarantees both `gfx` and `debugger` are
                // `Some` in the later arms, but the `as_mut().expect(...)` must be
                // deferred into those arms: binding them up front would hold a
                // `&mut self.gfx` / `&mut self.debugger` borrow across the emu-lock
                // acquire + the `&mut self.config` / `&mut self.ui` borrows below.
                // So the guard-then-expect is intentional, not a redundant unwrap.
                #[allow(clippy::unnecessary_unwrap)]
                let render_result = if self.debugger.is_none() || self.gfx.is_none() {
                    // No overlay yet (pre-`resumed`): nothing to render.
                    return;
                } else if needs_nes {
                    // v1.2.0 C3 — `(width, height)` of a composited HD-pack frame
                    // when an HD compositor is active; `None` means the stock
                    // NES-resolution present path (byte-identical).
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let mut hd_dims: Option<(u32, u32)> = None;
                    // v1.7.1 (#154 review) — snapshot ALL HD-composite inputs into
                    // owned locals under a BRIEF lock, then DROP the guard before the
                    // CPU-heavy `comp.composite` runs (the frontend discipline: never
                    // hold the emu lock during heavy work — see docs/frontend.md). This
                    // scope mirrors the common `else` branch exactly; the only reason
                    // this branch later re-takes the lock is that the debugger pass
                    // needs a live `&mut Nes`, which the common branch does not.
                    {
                        let mut guard = self.emu.lock();
                        let emu = &mut *guard;
                        // Backfill the presented framebuffer into staging under the
                        // held lock (a ROM may or may not be loaded). The debugger
                        // panels read `nes` from the re-acquired lock below. v1.7.1
                        // (#3): bound `as_mut` so the HD-snapshot peeks (`peek_ppu` /
                        // `ppu_bus_peek` / `cpu_bus_peek` all take `&mut self`,
                        // side-effect-free) can run here; the framebuffer backfill
                        // reborrows it shared.
                        if let Some(nes) = emu.nes.as_mut() {
                            Self::backfill_present_fb(&mut emu.present_fb, nes);
                            self.present_staging.clear();
                            self.present_staging.extend_from_slice(&emu.present_fb);
                            if want_index {
                                self.present_index_staging.clear();
                                self.present_index_staging
                                    .extend_from_slice(nes.index_framebuffer());
                            }
                            if want_phase {
                                self.present_phase = nes.ntsc_phase();
                            }
                            // v1.7.1 (#3) — the deep-overlay / nes-tool branch must
                            // STILL run the HD-pack composite, or a loaded pack is
                            // silently inert whenever the debugger overlay or a Cheats
                            // / ROM-Database panel is open (it would always present the
                            // stock framebuffer below). Snapshot the same inputs the
                            // common `else` branch does — the per-pixel tile-source,
                            // the 8 KiB CHR pattern space, and the watched-memory set —
                            // while the lock is held. The `&mut nes` here only drives
                            // side-effect-free peeks; `nes_for_render` (`as_mut`) is
                            // re-taken below for the debugger pass. The composite +
                            // present then mirror the common `else` branch. Skipped
                            // (no snapshot, no cost) when no pack is loaded.
                            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                            {
                                // v1.8.9 — gate the produce-time CHR snapshot.
                                emu.hd_capture = self.hd_compositor.is_some();
                            }
                            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                            if self.hd_compositor.is_some() {
                                self.present_hd_tiles.clear();
                                self.present_hd_tiles
                                    .extend_from_slice(nes.hd_tile_source());
                                // v1.8.9 — capture the frame's BG scroll for
                                // parallax `<background>` layers.
                                self.present_bg_scroll = nes.hd_bg_scroll();
                                // v1.8.9 — use the produce-time CHR snapshot (the
                                // visible frame) so run-ahead doesn't flicker
                                // animated tiles; fall back to a live peek before
                                // the first capture lands.
                                if emu.hd_chr_snapshot.len() == 0x2000 {
                                    self.present_chr_snapshot.clear();
                                    self.present_chr_snapshot
                                        .extend_from_slice(&emu.hd_chr_snapshot);
                                } else {
                                    if self.present_chr_snapshot.len() != 0x2000 {
                                        self.present_chr_snapshot.resize(0x2000, 0);
                                    }
                                    for (addr, slot) in
                                        (0u16..0x2000).zip(self.present_chr_snapshot.iter_mut())
                                    {
                                        *slot = nes.peek_ppu(addr);
                                    }
                                }
                                let watched = &mut self.present_watched_mem;
                                if let Some(comp) = self.hd_compositor.as_ref() {
                                    for &tagged in comp.watched_addresses() {
                                        let lo = (tagged & 0xFFFF) as u16;
                                        let val = if tagged & crate::hdpack::PPU_MEMORY_MARKER != 0
                                        {
                                            nes.ppu_bus_peek(lo)
                                        } else {
                                            nes.cpu_bus_peek(lo)
                                        };
                                        watched.set(tagged, val);
                                    }
                                }
                            }
                        } else {
                            self.present_staging.clear();
                            self.present_staging.resize((NES_W * NES_H * 4) as usize, 0);
                        }
                        // `guard` drops here, releasing the emu lock BEFORE the
                        // CPU-heavy composite below.
                    }
                    // v1.7.1 (#154 review) — lock dropped: run the CPU-heavy HD
                    // composite off the captured snapshots, exactly like the common
                    // `else` branch. The `chr_peek` closure reads the local snapshot,
                    // so no `Nes` borrow is held; the `&mut self.hd_compositor` +
                    // `&self.present_*` borrows are disjoint from the emu lock.
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    if let Some(comp) = self.hd_compositor.as_mut() {
                        let (w, h) = comp.dimensions();
                        comp.set_frame_scroll(self.present_bg_scroll.0, self.present_bg_scroll.1);
                        let chr = &self.present_chr_snapshot;
                        comp.composite(
                            &self.present_staging,
                            &self.present_hd_tiles,
                            &self.present_watched_mem,
                            |addr| chr.get((addr & 0x1FFF) as usize).copied().unwrap_or(0),
                        );
                        hd_dims = Some((w, h));
                    }
                    // v1.7.1 (#154 review) — re-acquire the lock to hand the debugger
                    // pass a live `&mut Nes`. The composite is already done (above,
                    // unlocked), so the lock is now held only across the egui /
                    // `render_shell` pass — the same discipline the rest of the shell
                    // follows. This `guard` stays alive until after the present call.
                    let mut guard = self.emu.lock();
                    let emu = &mut *guard;
                    let nes_for_render = emu.nes.as_mut();
                    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                    let script_draws = &self.script_draws;
                    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                    let script_par = self.config.ui.pixel_aspect_correction;
                    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                    let script_overscan = self.config.graphics.hide_overscan;
                    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
                    let script_draws_wasm = &self.script_draws_wasm;
                    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
                    let script_par_wasm = self.config.ui.pixel_aspect_correction;
                    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
                    let script_overscan_wasm = self.config.graphics.hide_overscan;
                    // v1.7.1 (#3) — borrow the composited HD frame (a disjoint
                    // field) before the `gfx` borrow so the present can hand it off.
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let hd_frame: Option<&[u8]> = hd_dims
                        .and(self.hd_compositor.as_ref())
                        .map(crate::hdpack::HdCompositor::frame);
                    let gfx = self.gfx.as_mut().expect("checked above");
                    let debugger = self
                        .debugger
                        .as_mut()
                        .expect("dbg_visible implies a debugger");
                    let window = gfx.window.clone();
                    let config = &mut self.config;
                    let ui_shell = &mut self.ui;
                    // wasm-only: draw the browser-netplay lobby into the same
                    // egui frame (the `App` owns the lobby state). Native passes
                    // the Save-States manager window instead.
                    #[cfg(target_arch = "wasm32")]
                    let wasm_lobby = &mut self.wasm_lobby;
                    // v1.5.0 Workstream G — capture the casual-only browser RA
                    // caveat (ADR 0015) as a &'static str before the render
                    // closure so it doesn't fight the other field borrows.
                    #[cfg(all(target_arch = "wasm32", feature = "browser-cheevos"))]
                    let browser_ra_caveat = self
                        .browser_ra
                        .as_ref()
                        .map(crate::wasm_cheevos::BrowserRaSession::caveat_banner);
                    #[cfg(not(target_arch = "wasm32"))]
                    let save_states_ui = &mut self.save_states_ui;
                    #[cfg(not(target_arch = "wasm32"))]
                    let virtual_pad = &mut self.virtual_pad;
                    let index_arg = want_index.then_some(self.present_index_staging.as_slice());
                    let video_phase = self.present_phase;
                    let overlay = |device: &wgpu::Device,
                                   queue: &wgpu::Queue,
                                   encoder: &mut wgpu::CommandEncoder,
                                   view: &wgpu::TextureView,
                                   size: (u32, u32)| {
                        #[cfg(target_arch = "wasm32")]
                        let extra = |ctx: &egui::Context, cfg: &mut crate::config::Config| {
                            crate::wasm_lobby::show(ctx, wasm_lobby, cfg);
                            // v1.5.0 Workstream G — the casual-only browser RA caveat.
                            #[cfg(feature = "browser-cheevos")]
                            Self::paint_browser_ra_caveat(ctx, browser_ra_caveat);
                            // v1.4.0 E2 — the browser Save-States thumbnail grid.
                            crate::wasm_save_states::show(ctx, ss_slot_wasm, ss_rom_loaded_wasm);
                            #[cfg(feature = "script-wasm")]
                            Self::paint_script_overlay_wasm(
                                ctx,
                                script_draws_wasm,
                                script_par_wasm,
                                script_overscan_wasm,
                            );
                        };
                        #[cfg(not(target_arch = "wasm32"))]
                        let extra = |ctx: &egui::Context, _cfg: &mut crate::config::Config| {
                            save_states_ui.show(
                                ctx,
                                ss_dir.as_deref(),
                                ss_sha,
                                ss_slot,
                                rom_loaded,
                            );
                            // v1.8.9 — the on-screen virtual pad (a floating
                            // egui window; no-op + empty mask when hidden).
                            virtual_pad.show(ctx);
                            #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                            Self::paint_script_overlay(
                                ctx,
                                script_draws,
                                script_par,
                                script_overscan,
                            );
                        };
                        shell_out = debugger.render_shell(
                            device,
                            queue,
                            encoder,
                            &window,
                            view,
                            size,
                            nes_for_render,
                            config,
                            ui_shell,
                            &shell_frame,
                            extra,
                        );
                    };
                    // v1.7.1 (#3) — present the upscaled HD buffer when a pack
                    // composited this redraw (the deep-overlay panels still draw on
                    // top via `overlay`); else the stock NES-resolution present path
                    // (byte-identical to before).
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let render_result = match (hd_dims, hd_frame) {
                        (Some((w, h)), Some(frame)) => {
                            gfx.render_hd_with_overlay(frame, w, h, overlay)
                        }
                        _ => gfx.render_with_overlay(
                            &self.present_staging,
                            index_arg,
                            video_phase,
                            overlay,
                        ),
                    };
                    #[cfg(not(all(feature = "hd-pack", not(target_arch = "wasm32"))))]
                    let render_result = gfx.render_with_overlay(
                        &self.present_staging,
                        index_arg,
                        video_phase,
                        overlay,
                    );
                    render_result
                } else {
                    // Common path: copy the presented framebuffer under a brief
                    // lock, DROP the guard, then encode + present from staging.
                    // v1.2.0 C3 — `(width, height)` of a composited HD-pack frame
                    // when an HD compositor is active for the loaded ROM; `None`
                    // means the stock NES-resolution present path (byte-identical).
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let mut hd_dims: Option<(u32, u32)> = None;
                    // v1.5.0 H1 — when the dedicated emu thread is producing AND
                    // this present needs nothing from the live `Nes` beyond the
                    // RGBA framebuffer (no NTSC composite-rt index buffer, no live
                    // colour phase, no HD pack), take the freshest frame from the
                    // lock-free handoff and skip the emu mutex entirely — so the
                    // present thread never blocks on the ~8.5 ms
                    // `produce_one_frame`. The bytes are the same deterministic
                    // `present_fb` the locked path copied; only the
                    // synchronization changed.
                    //
                    // v2.1.2 F2.2 — gate on `!want_phase`, not `!want_index`:
                    // `want_index` implies `want_phase`, and an LMP88959-only stack
                    // wants the live phase without the index buffer. The phase is
                    // captured only under the emu lock (the handoff carries just the
                    // framebuffer), so a phase-consuming pass MUST take the locked
                    // path or `present_phase` would freeze and the dot-crawl stall.
                    // v2.1.2 F2.1 — when a cabinet is loaded, the composed
                    // two-screen dimensions (else `None` → the single present).
                    #[cfg(not(target_arch = "wasm32"))]
                    let mut dual_present_dims: Option<(u32, u32)> = None;
                    #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
                    let lockfree_fb = {
                        #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                        let hd_active = self.hd_compositor.is_some();
                        #[cfg(not(all(feature = "hd-pack", not(target_arch = "wasm32"))))]
                        let hd_active = false;
                        self.emu_thread_drives()
                            && !want_phase
                            && !hd_active
                            // v2.1.2 F2.1 — dual needs BOTH framebuffers from the
                            // lock (the handoff carries only the main screen).
                            && !self.dual_mode
                            && self.present_buffer.has_published()
                    };
                    #[cfg(not(all(not(target_arch = "wasm32"), feature = "emu-thread")))]
                    let lockfree_fb = false;
                    if lockfree_fb {
                        // Lock-free: refresh staging from the handoff if a new
                        // frame arrived; otherwise keep the previously presented
                        // staging (the display simply re-presents it).
                        #[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
                        if !self.present_buffer.take_into(&mut self.present_staging)
                            && self.present_staging.is_empty()
                        {
                            self.present_staging.resize((NES_W * NES_H * 4) as usize, 0);
                        }
                    } else {
                        let mut guard = self.emu.lock();
                        let emu = &mut *guard;
                        // v2.1.2 F2.1 — Vs. `DualSystem`: compose the two harvested
                        // console framebuffers (main = `present_fb`, sub =
                        // `present_fb_sub`) into `present_dual` per the configured
                        // layout, under the same brief lock. `emu.dual`/`emu.nes` are
                        // mutually exclusive, so this replaces the single fill.
                        #[cfg(not(target_arch = "wasm32"))]
                        if emu.dual.is_some() {
                            let layout = crate::gfx::DualLayout::from_config(
                                &self.config.graphics.dual_screen_layout,
                            );
                            dual_present_dims = Some(crate::gfx::compose_dual_into(
                                &mut self.present_dual,
                                &emu.present_fb,
                                &emu.present_fb_sub,
                                layout,
                            ));
                        }
                        if let Some(nes) = emu.nes.as_mut() {
                            Self::backfill_present_fb(&mut emu.present_fb, nes);
                            self.present_staging.clear();
                            self.present_staging.extend_from_slice(&emu.present_fb);
                            if want_index {
                                self.present_index_staging.clear();
                                self.present_index_staging
                                    .extend_from_slice(nes.index_framebuffer());
                            }
                            if want_phase {
                                self.present_phase = nes.ntsc_phase();
                            }
                            // v1.2.0 C3 — under the lock, snapshot ONLY the inputs
                            // the HD composite needs: the PPU per-pixel tile-source
                            // telemetry + the 8 KiB CHR pattern space. The CPU-heavy
                            // composite (upscale + tile-hash + blit) runs AFTER the
                            // lock is dropped (below), honouring the frontend's
                            // "never hold the emu lock during heavy work" discipline.
                            // Skipped entirely when no pack is loaded AND the
                            // G5 builder is not recording (the builder needs the
                            // same tile-source + CHR snapshot to capture tiles).
                            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                            {
                                // v1.8.9 — gate the produce-time CHR snapshot.
                                emu.hd_capture =
                                    self.hd_compositor.is_some() || self.hd_pack_builder.is_some();
                            }
                            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                            if self.hd_compositor.is_some() || self.hd_pack_builder.is_some() {
                                self.present_hd_tiles.clear();
                                self.present_hd_tiles
                                    .extend_from_slice(nes.hd_tile_source());
                                self.present_bg_scroll = nes.hd_bg_scroll();
                                // v1.8.9 — prefer the produce-time CHR snapshot (the
                                // visible frame, in lock-step with `present_fb` +
                                // the tile-source) so run-ahead doesn't flicker
                                // animated tiles; live-peek fallback before the
                                // first capture lands.
                                if emu.hd_chr_snapshot.len() == 0x2000 {
                                    self.present_chr_snapshot.clear();
                                    self.present_chr_snapshot
                                        .extend_from_slice(&emu.hd_chr_snapshot);
                                } else {
                                    if self.present_chr_snapshot.len() != 0x2000 {
                                        self.present_chr_snapshot.resize(0x2000, 0);
                                    }
                                    for (addr, slot) in
                                        (0u16..0x2000).zip(self.present_chr_snapshot.iter_mut())
                                    {
                                        *slot = nes.peek_ppu(addr);
                                    }
                                }
                                // v1.3.0 E1 — snapshot ONLY the finite set of
                                // watched memory addresses referenced by the
                                // pack's `<condition>` declarations (Mesen's
                                // `WatchedAddressValues`). Read under the lock so
                                // the compositor evaluates conditions after the
                                // lock drops without touching `Nes`. Each address
                                // carries bit 31 (`PPU_MEMORY_MARKER`) to select
                                // PPU- vs CPU-space; both peeks are side-effect-free.
                                let watched = &mut self.present_watched_mem;
                                if let Some(comp) = self.hd_compositor.as_ref() {
                                    for &tagged in comp.watched_addresses() {
                                        let lo = (tagged & 0xFFFF) as u16;
                                        let val = if tagged & crate::hdpack::PPU_MEMORY_MARKER != 0
                                        {
                                            nes.ppu_bus_peek(lo)
                                        } else {
                                            nes.cpu_bus_peek(lo)
                                        };
                                        watched.set(tagged, val);
                                    }
                                }
                            }
                        } else {
                            // No ROM: present a black NES image (the shell still
                            // draws on top).
                            self.present_staging.clear();
                            self.present_staging.resize((NES_W * NES_H * 4) as usize, 0);
                        }
                    }
                    // v1.2.0 C3 — lock dropped: now run the CPU-heavy HD composite
                    // off the captured snapshots (framebuffer + tile-source + CHR).
                    // `chr_peek` reads the local snapshot, so no `Nes` borrow is held.
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    if let Some(comp) = self.hd_compositor.as_mut() {
                        let (w, h) = comp.dimensions();
                        comp.set_frame_scroll(self.present_bg_scroll.0, self.present_bg_scroll.1);
                        let chr = &self.present_chr_snapshot;
                        comp.composite(
                            &self.present_staging,
                            &self.present_hd_tiles,
                            &self.present_watched_mem,
                            |addr| chr.get((addr & 0x1FFF) as usize).copied().unwrap_or(0),
                        );
                        hd_dims = Some((w, h));
                    }
                    // v1.7.0 "Forge" G5 — feed the HD-Pack BUILDER the same
                    // already-captured snapshots (the *native* framebuffer, the
                    // per-pixel tile-source, and the CHR snapshot). Output-only:
                    // it records distinct tiles + their native pixels and mutates
                    // nothing, so the present path stays byte-identical. Runs after
                    // the lock drops (CPU-heavy hashing) like the composite.
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    if let Some(builder) = self.hd_pack_builder.as_mut() {
                        let chr = &self.present_chr_snapshot;
                        builder.observe(&self.present_staging, &self.present_hd_tiles, |addr| {
                            chr.get((addr & 0x1FFF) as usize).copied().unwrap_or(0)
                        });
                    }
                    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                    let script_draws = &self.script_draws;
                    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                    let script_par = self.config.ui.pixel_aspect_correction;
                    #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                    let script_overscan = self.config.graphics.hide_overscan;
                    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
                    let script_draws_wasm = &self.script_draws_wasm;
                    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
                    let script_par_wasm = self.config.ui.pixel_aspect_correction;
                    #[cfg(all(feature = "script-wasm", target_arch = "wasm32"))]
                    let script_overscan_wasm = self.config.graphics.hide_overscan;
                    // v1.2.0 C3 — borrow the composited HD frame (disjoint field)
                    // before the `gfx` borrow so the HD branch can hand it off.
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let hd_frame: Option<&[u8]> = hd_dims
                        .and(self.hd_compositor.as_ref())
                        .map(crate::hdpack::HdCompositor::frame);
                    // v1.5.0 "Lens" Workstream A4 — extract the HD pixel-inspector
                    // open flag + a clone of its state so the `extra` egui closure
                    // can render it against the captured per-frame snapshots
                    // (borrow-disjoint from the debugger's own `render_shell` pass,
                    // which `extra` runs inside). Written back after render_shell.
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let hd_pixel_open = self
                        .debugger
                        .as_ref()
                        .is_some_and(crate::debugger::DebuggerOverlay::hd_pixel_open);
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let mut hd_pixel_state = self
                        .debugger
                        .as_ref()
                        .map(crate::debugger::DebuggerOverlay::hd_pixel_state);
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let hd_comp_ref = self.hd_compositor.as_ref();
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let hd_fb = &self.present_staging;
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let hd_ts = &self.present_hd_tiles;
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let hd_watched = &self.present_watched_mem;
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let hd_chr = &self.present_chr_snapshot;
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let mut hd_pixel_open_after = hd_pixel_open;
                    let gfx = self.gfx.as_mut().expect("checked above");
                    let debugger = self.debugger.as_mut().expect("checked above");
                    let window = gfx.window.clone();
                    let config = &mut self.config;
                    let ui_shell = &mut self.ui;
                    #[cfg(target_arch = "wasm32")]
                    let wasm_lobby = &mut self.wasm_lobby;
                    // v1.5.0 Workstream G — capture the casual-only browser RA
                    // caveat (ADR 0015) before the render closure (see above).
                    #[cfg(all(target_arch = "wasm32", feature = "browser-cheevos"))]
                    let browser_ra_caveat = self
                        .browser_ra
                        .as_ref()
                        .map(crate::wasm_cheevos::BrowserRaSession::caveat_banner);
                    #[cfg(not(target_arch = "wasm32"))]
                    let save_states_ui = &mut self.save_states_ui;
                    #[cfg(not(target_arch = "wasm32"))]
                    let virtual_pad = &mut self.virtual_pad;
                    let index_arg = want_index.then_some(self.present_index_staging.as_slice());
                    let video_phase = self.present_phase;
                    let overlay = |device: &wgpu::Device,
                                   queue: &wgpu::Queue,
                                   encoder: &mut wgpu::CommandEncoder,
                                   view: &wgpu::TextureView,
                                   size: (u32, u32)| {
                        #[cfg(target_arch = "wasm32")]
                        let extra = |ctx: &egui::Context, cfg: &mut crate::config::Config| {
                            crate::wasm_lobby::show(ctx, wasm_lobby, cfg);
                            // v1.5.0 Workstream G — the casual-only browser RA caveat.
                            #[cfg(feature = "browser-cheevos")]
                            Self::paint_browser_ra_caveat(ctx, browser_ra_caveat);
                            // v1.4.0 E2 — the browser Save-States thumbnail grid.
                            crate::wasm_save_states::show(ctx, ss_slot_wasm, ss_rom_loaded_wasm);
                            #[cfg(feature = "script-wasm")]
                            Self::paint_script_overlay_wasm(
                                ctx,
                                script_draws_wasm,
                                script_par_wasm,
                                script_overscan_wasm,
                            );
                        };
                        #[cfg(not(target_arch = "wasm32"))]
                        let extra = |ctx: &egui::Context, _cfg: &mut crate::config::Config| {
                            save_states_ui.show(
                                ctx,
                                ss_dir.as_deref(),
                                ss_sha,
                                ss_slot,
                                rom_loaded,
                            );
                            // v1.8.9 — the on-screen virtual pad (a floating
                            // egui window; no-op + empty mask when hidden).
                            virtual_pad.show(ctx);
                            #[cfg(all(feature = "scripting", not(target_arch = "wasm32")))]
                            Self::paint_script_overlay(
                                ctx,
                                script_draws,
                                script_par,
                                script_overscan,
                            );
                            // v1.5.0 "Lens" Workstream A4 — HD-pack pixel inspector,
                            // rendered here (not in the debugger pass) so it can
                            // read the captured compositor + per-frame snapshots.
                            #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                            if hd_pixel_open && let Some(state) = hd_pixel_state.as_mut() {
                                crate::debugger::hd_pixel_panel::show(
                                    ctx,
                                    &mut hd_pixel_open_after,
                                    state,
                                    hd_comp_ref.is_some(),
                                    hd_comp_ref,
                                    hd_fb,
                                    hd_ts,
                                    hd_watched,
                                    hd_chr,
                                );
                            }
                        };
                        // Debugger panels are skipped (overlay hidden) so
                        // `nes = None` is correct even though a ROM may exist.
                        shell_out = debugger.render_shell(
                            device,
                            queue,
                            encoder,
                            &window,
                            view,
                            size,
                            None,
                            config,
                            ui_shell,
                            &shell_frame,
                            extra,
                        );
                    };
                    // v1.2.0 C3 — when an HD-pack frame was composited this
                    // redraw, present the upscaled buffer through the dedicated
                    // HD blit; otherwise the stock NES-resolution present path
                    // (byte-identical to before).
                    // v2.1.2 F2.1 — a Vs. `DualSystem` cabinet presents the composed
                    // two-screen image via the dedicated dynamic blit first.
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    let render_result = if let Some((dw, dh)) = dual_present_dims {
                        gfx.render_dual(&self.present_dual, dw, dh, overlay)
                    } else {
                        match (hd_dims, hd_frame) {
                            (Some((w, h)), Some(frame)) => {
                                gfx.render_hd_with_overlay(frame, w, h, overlay)
                            }
                            _ => gfx.render_with_overlay(
                                &self.present_staging,
                                index_arg,
                                video_phase,
                                overlay,
                            ),
                        }
                    };
                    #[cfg(all(not(feature = "hd-pack"), not(target_arch = "wasm32")))]
                    let render_result = if let Some((dw, dh)) = dual_present_dims {
                        gfx.render_dual(&self.present_dual, dw, dh, overlay)
                    } else {
                        gfx.render_with_overlay(
                            &self.present_staging,
                            index_arg,
                            video_phase,
                            overlay,
                        )
                    };
                    #[cfg(target_arch = "wasm32")]
                    let render_result = gfx.render_with_overlay(
                        &self.present_staging,
                        index_arg,
                        video_phase,
                        overlay,
                    );
                    // v1.5.0 A4 — write the HD pixel-inspector state (px/py/blend)
                    // + the (possibly closed) open flag back into the debugger now
                    // that the `extra` closure that mutated the clone is consumed.
                    #[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
                    if let Some(state) = hd_pixel_state {
                        debugger.set_hd_pixel_state(state, hd_pixel_open_after);
                    }
                    render_result
                };
                match render_result {
                    Ok(()) => {
                        // v2.8.0 Phase 0 — the display-visible cadence. The
                        // produced-frame histogram alone cannot see judder;
                        // this one can (present-to-present deltas). v1.3.0 B1:
                        // use the redraw-signal timestamp captured at the top of
                        // this arm, not a post-present `Instant::now()`, so the
                        // metric tracks the display refresh and not submit/vsync
                        // jitter.
                        self.emu.lock().perf.record_presented(redraw_signal);
                        // v2.8.0 Phase 2 — display-sync self-drive + health.
                        #[cfg(not(target_arch = "wasm32"))]
                        self.display_sync_after_present();
                    }
                    Err(crate::gfx::PresentError::Reconfigure) => {
                        if let Some(gfx) = self.gfx.as_mut() {
                            let size = gfx.window.inner_size();
                            gfx.resize(size.width, size.height);
                        }
                    }
                    Err(e) => eprintln!("rustynes: render error: {e}"),
                }

                // v1.0.0 — dispatch the UX-shell menu action chosen this frame.
                // Deferred to here (after the egui pass) because the action
                // handlers need `&mut self`, which the build closure cannot hold
                // (it borrows `&mut self.config` / `&mut self.ui`).
                if let Some(action) = shell_out.action.take() {
                    self.dispatch_menu_action(action, event_loop);
                }

                // v1.0.0 — push a pixel-aspect-correction change (made in the
                // menu / settings window) into the gfx letterbox. Mirrors the
                // NTSC live-apply pattern: cache the previous value and act on a
                // transition.
                if self.config.ui.pixel_aspect_correction != self.prev_par_correction {
                    self.prev_par_correction = self.config.ui.pixel_aspect_correction;
                    if let Some(gfx) = self.gfx.as_mut() {
                        gfx.set_pixel_aspect(self.config.ui.pixel_aspect_correction);
                    }
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
                    // v1.5.0 "Lens" Workstream I3 — PERSIST the config when any
                    // input binding changed. The Settings window already
                    // auto-saves via its before/after config snapshot, but the
                    // STANDALONE Tools -> Input window has no such wrapper, so a
                    // rebind there used to apply live yet never reach disk (the
                    // old "Save to disk" button was the only persistence path).
                    // Saving on the same dirty edge makes the per-setting
                    // auto-save fire for every Input control in BOTH windows, so
                    // the button is no longer needed for persistence (it was
                    // relabelled to "Export config..." for its real, distinct
                    // function). Native-only (no filesystem on wasm).
                    #[cfg(not(target_arch = "wasm32"))]
                    if let Err(e) = self.config.save() {
                        eprintln!("rustynes: config auto-save failed: {e}");
                    }
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
                if settings.ntsc_filter
                    && let Some(gfx) = self.gfx.as_mut()
                {
                    // CRT (if on) takes render priority; selecting any NTSC
                    // mode turns CRT off so the settings stay coherent.
                    if self.config.graphics.ntsc_filter != "off" {
                        gfx.disable_crt();
                        self.config.graphics.crt_filter = false;
                    }
                    Self::apply_ntsc_mode(gfx, &self.config.graphics);
                }
                // v1.0.0 — master volume / mute live-apply (the cpal consume
                // gain). Native-only; wasm has no app-resident audio queue.
                #[cfg(not(target_arch = "wasm32"))]
                if settings.audio_gain {
                    self.apply_audio_gain();
                }
                // v1.1.0 beta.2 — graphic-EQ live-apply (frontend output stage).
                #[cfg(not(target_arch = "wasm32"))]
                if settings.audio_eq {
                    self.apply_audio_eq();
                }
                // v1.7.0 H3 — stereo DSP live-apply (pan / reverb / crossfeed,
                // a frontend output stage; bypass-by-default is byte-identical).
                #[cfg(not(target_arch = "wasm32"))]
                if settings.audio_stereo {
                    self.apply_stereo_dsp();
                }
                // v1.0.0 / v1.5.0 D2 — overscan crop live-apply (the gfx
                // letterbox UV rect): push BOTH the legacy toggle and the
                // per-side crop. The gfx combines them via `effective_overscan`.
                if settings.overscan
                    && let Some(gfx) = self.gfx.as_mut()
                {
                    gfx.set_hide_overscan(self.config.graphics.hide_overscan);
                    gfx.set_overscan(self.config.graphics.overscan);
                }
                // v1.1.0 beta.1 — CRT filter live-apply. CRT and NTSC are
                // mutually exclusive at render time; turning CRT on drops NTSC so
                // the settings stay coherent.
                if settings.crt_filter
                    && let Some(gfx) = self.gfx.as_mut()
                {
                    if self.config.graphics.crt_filter {
                        // CRT and NTSC are mutually exclusive; enabling CRT
                        // turns the NTSC filter off in the config too, so the
                        // settings stay coherent (and a later CRT-off
                        // restores "no filter", not a stale NTSC mode).
                        gfx.enable_crt(self.config.graphics.crt_scanline);
                        gfx.disable_ntsc();
                        gfx.disable_ntsc_bisqwit();
                        self.config.graphics.ntsc_filter = "off".to_string();
                    } else {
                        // Turning CRT off restores whatever NTSC mode the
                        // config now holds (normally "off" → no filter).
                        gfx.disable_crt();
                        Self::apply_ntsc_mode(gfx, &self.config.graphics);
                    }
                    let _ = self.config.save();
                }
                if settings.crt_scanline
                    && let Some(gfx) = self.gfx.as_mut()
                {
                    gfx.set_crt_scanline(self.config.graphics.crt_scanline);
                }
                // v1.2.0 C1 — Bisqwit-NTSC picture-knob live-apply (output-only;
                // defaults decode byte-identically to the pre-C1 filter).
                if settings.ntsc_knobs
                    && let Some(gfx) = self.gfx.as_mut()
                {
                    gfx.set_ntsc_bisqwit_knobs(Self::ntsc_knobs_from(&self.config.graphics));
                }
                // v1.2.0 C2 — composable shader-stack live-apply. Rebuild the gfx
                // ping-pong stack from `[graphics] shader_stack`; an empty /
                // all-disabled stack rebuilds to the byte-identical direct blit.
                // When a leading composite-rt pass is present its live picture
                // knobs are forwarded too.
                if settings.shader_stack
                    && let Some(gfx) = self.gfx.as_mut()
                {
                    gfx.set_stack_ntsc_knobs(Self::ntsc_knobs_from(&self.config.graphics));
                    gfx.apply_shader_stack(&self.config.graphics.shader_stack);
                }
                // v1.1.0 beta.1 — custom .pal palette: the file dialog + apply run
                // here (after the egui pass), never inside the settings closure.
                // Native-only (rfd / filesystem).
                #[cfg(not(target_arch = "wasm32"))]
                if settings.palette_pick {
                    self.pick_palette_dialog();
                }
                #[cfg(not(target_arch = "wasm32"))]
                if settings.palette_clear {
                    self.clear_palette();
                }
                // v1.5.0 D1 — named-palette select / edit live-apply: resolve the
                // active base palette and push it to the core (or clear it).
                if settings.palette_select {
                    self.apply_active_palette();
                }
                // v1.0.0 — per-APU-channel mute live-apply: push the mask into
                // the core under the emu lock. Default mask = byte-identical.
                if settings.apu_channels {
                    self.apply_apu_channel_mask();
                }
                // v1.4.0 Workstream C — per-APU-channel gain live-apply: push the
                // gains into the core under the emu lock. Default (all 1.0) =
                // byte-identical.
                if settings.apu_channel_gain {
                    self.apply_apu_channel_gain();
                }
                if settings.apu_filter_model {
                    self.apply_apu_filter_model();
                }
                // v1.0.0 — act on a Save-States manager Save / Load click this
                // frame, routing through the existing slot handlers; a Save
                // invalidates that slot's cached thumbnail so the grid refreshes.
                #[cfg(not(target_arch = "wasm32"))]
                if let Some(req) = self.save_states_ui.take_request() {
                    use crate::save_states_ui::SaveStateRequest;
                    use crate::ui_shell::StatusMessage;
                    match req {
                        SaveStateRequest::Save(slot) => {
                            self.handle_save_state(slot);
                            self.save_states_ui.invalidate_slot(slot);
                            self.ui.set_status(StatusMessage::success(format!(
                                "Saved to slot {}",
                                slot + 1
                            )));
                        }
                        SaveStateRequest::Load(slot) => {
                            if self.ra_hardcore_blocks() {
                                self.ui.set_status(StatusMessage::info(
                                    "Load state disabled (hardcore)",
                                ));
                            } else {
                                self.handle_load_state(slot);
                                self.ui.set_status(StatusMessage::success(format!(
                                    "Loaded from slot {}",
                                    slot + 1
                                )));
                            }
                        }
                    }
                }
                // v1.4.0 E2 — act on a browser Save-States grid click this frame,
                // routing through the same async IndexedDB save/load path the
                // F1/F4 hotkeys use. A Save re-scans the grid so the new thumbnail
                // shows; a Load is replay-locked like every other load path.
                #[cfg(target_arch = "wasm32")]
                if let Some(req) = crate::wasm_save_states::take_request() {
                    use crate::ui_shell::StatusMessage;
                    use crate::wasm_save_states::SlotRequest;
                    match req {
                        SlotRequest::Save(slot) => {
                            self.handle_save_state_wasm(slot);
                            let sha = self.emu.lock().nes.as_ref().map(|n| *n.rom_sha256());
                            crate::wasm_save_states::open(sha);
                            self.ui.set_status(StatusMessage::success(format!(
                                "Saved to slot {}",
                                slot + 1
                            )));
                        }
                        SlotRequest::Load(slot) => {
                            if self.ra_hardcore_blocks() {
                                self.ui.set_status(StatusMessage::info(
                                    "Load state disabled (hardcore)",
                                ));
                            } else if self.replay_interaction_locked() {
                                self.ui.set_status(StatusMessage::info(
                                    "Load state disabled during movie",
                                ));
                            } else {
                                self.handle_load_state_wasm(slot);
                                self.ui.set_status(StatusMessage::success(format!(
                                    "Loaded from slot {}",
                                    slot + 1
                                )));
                            }
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

                // v1.5.0 "Lens" Workstream C2 — act on a Replay / TAS window
                // button (record/play/branch/stop/seek) clicked this frame.
                if let Some(req) = self
                    .debugger
                    .as_mut()
                    .and_then(DebuggerOverlay::take_replay_request)
                {
                    self.handle_replay_request(req, event_loop);
                }

                // v1.6.0 "Studio" A2 — apply any TAStudio piano-roll edits /
                // seeks queued by the panel this frame (under the emu lock).
                self.handle_tas_requests();
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
    use super::{
        extract_rom_from_zip, is_fds_image, is_nsf_image, load_and_preprocess_rom,
        nsf_header_strings, resolve_vs_dip,
    };
    use crate::config::VsConfig;
    use rustynes_core::VsDbEntry;
    use rustynes_core::rustynes_mappers::VsPpuType;

    /// Build a minimal classic `NESM` file with known title/artist/copyright
    /// fields for the header-string display test.
    fn synth_nesm(title: &[u8], artist: &[u8], copyright: &[u8]) -> Vec<u8> {
        let mut f = vec![0u8; 0x80];
        f[0..5].copy_from_slice(b"NESM\x1A");
        f[0x05] = 0x01; // version
        f[0x06] = 1; // total songs
        f[0x07] = 1; // starting song
        f[0x08..0x0A].copy_from_slice(&0x8000u16.to_le_bytes()); // load
        f[0x0A..0x0C].copy_from_slice(&0x8000u16.to_le_bytes()); // init
        f[0x0C..0x0E].copy_from_slice(&0x8003u16.to_le_bytes()); // play
        f[0x0E..0x0E + title.len()].copy_from_slice(title);
        f[0x2E..0x2E + artist.len()].copy_from_slice(artist);
        f[0x4E..0x4E + copyright.len()].copy_from_slice(copyright);
        f.extend_from_slice(&[0x60, 0xEA, 0xEA, 0x60]); // trivial program image
        f
    }

    /// Build a minimal extended `NSFE` file whose title/artist/copyright live in
    /// the `auth` chunk — deliberately NOT at the classic `$0E`/`$2E`/`$4E`
    /// offsets, so a fixed-offset reader would surface chunk garbage.
    fn synth_nsfe(title: &str, artist: &str, copyright: &str) -> Vec<u8> {
        let mut f = Vec::new();
        f.extend_from_slice(b"NSFE");
        let mut chunk = |tag: &[u8; 4], body: &[u8]| {
            f.extend_from_slice(&u32::try_from(body.len()).unwrap().to_le_bytes());
            f.extend_from_slice(tag);
            f.extend_from_slice(body);
        };
        chunk(
            b"INFO",
            &[0x00, 0x80, 0x00, 0x80, 0x03, 0x80, 0, 0x00, 1, 1],
        );
        chunk(b"DATA", &[0x60, 0xEA, 0xEA, 0x60]);
        let auth = format!("{title}\0{artist}\0{copyright}\0Ripper");
        chunk(b"auth", auth.as_bytes());
        chunk(b"NEND", &[]);
        f
    }

    #[test]
    fn nsf_header_strings_reads_classic_nesm_fields() {
        let f = synth_nesm(b"Song A", b"Composer B", b"(c) 2026 C");
        assert!(is_nsf_image(&f));
        let (title, artist, copyright) = nsf_header_strings(&f);
        assert_eq!(title, "Song A");
        assert_eq!(artist, "Composer B");
        assert_eq!(copyright, "(c) 2026 C");
    }

    #[test]
    fn nsf_header_strings_reads_nsfe_auth_chunk_not_fixed_offset() {
        // Regression for PR #254: is_nsf_image now routes NSFE through the NSF
        // UI, so the header-string reader must decode the NSFE `auth` chunk
        // rather than reading classic-header offsets (which would yield garbage).
        let f = synth_nsfe("NSFE Title", "NSFE Artist", "NSFE (c)");
        assert!(is_nsf_image(&f));
        let (title, artist, copyright) = nsf_header_strings(&f);
        assert_eq!(title, "NSFE Title");
        assert_eq!(artist, "NSFE Artist");
        assert_eq!(copyright, "NSFE (c)");
    }

    #[test]
    fn nsf_header_strings_empty_on_garbage() {
        // A non-NSF / truncated buffer must not panic and yields empty fields.
        let (t, a, c) = nsf_header_strings(b"not an nsf file at all");
        assert!(t.is_empty() && a.is_empty() && c.is_empty());
    }

    #[test]
    fn zip_extracts_first_rom_entry() {
        use std::io::Write;
        // Build an in-memory zip: a junk text entry then a (stored) .nes entry.
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("readme.txt", opts).unwrap();
            w.write_all(b"not a rom").unwrap();
            w.start_file("Game (U).nes", opts).unwrap();
            w.write_all(b"NES\x1A\x01\x01rompayload").unwrap();
            w.finish().unwrap();
        }
        let (name, rom) = extract_rom_from_zip(&buf).expect("extracts the .nes entry");
        assert_eq!(name, "Game (U).nes");
        assert!(rom.starts_with(b"NES\x1A"));
    }

    #[test]
    fn zip_without_rom_returns_none() {
        use std::io::Write;
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            w.start_file(
                "notes.txt",
                zip::write::SimpleFileOptions::default()
                    .compression_method(zip::CompressionMethod::Stored),
            )
            .unwrap();
            w.write_all(b"nothing here").unwrap();
            w.finish().unwrap();
        }
        assert!(extract_rom_from_zip(&buf).is_none());
    }

    // Regression (the CLI / `App::new` initial-ROM path): a `.zip` passed on argv
    // must be extracted to its bare NES image before the core parses it. Before
    // the fix, `App::new` stored the raw archive bytes and the load failed with
    // "rom magic bytes do not match NES\x1A".
    #[test]
    fn cli_path_extracts_zip_to_nes_image() {
        use std::io::Write;
        let zip_path =
            std::env::temp_dir().join(format!("rustynes_clizip_{}.zip", std::process::id()));
        {
            let f = std::fs::File::create(&zip_path).unwrap();
            let mut w = zip::ZipWriter::new(f);
            let opts = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Stored);
            w.start_file("Cool Game (U).nes", opts).unwrap();
            w.write_all(b"NES\x1A\x02\x01payload").unwrap();
            w.finish().unwrap();
        }
        let result = load_and_preprocess_rom(&zip_path);
        let _ = std::fs::remove_file(&zip_path);
        let (bytes, label) = result.expect("the .zip extracts to a ROM");
        assert!(
            bytes.starts_with(b"NES\x1A"),
            "must return the extracted NES image, not the raw zip bytes"
        );
        assert_eq!(label, "Cool Game (U).nes");
    }

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
