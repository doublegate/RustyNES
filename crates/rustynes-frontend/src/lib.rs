//! `RustyNES` v2 frontend library — shared between the native
//! `[[bin]]` target (`src/main.rs`) and the wasm32 `cdylib` artifact
//! that `trunk` consumes for the browser build.
//!
//! v1.3.0 Sprint 1.2 — restructured from a binary-only crate so the
//! same module tree powers both the desktop binary and the web
//! frontend. All native code paths (`rfd` file dialog, `cpal` audio,
//! `winit::run_app`) are preserved as-is on native; wasm32 routes
//! through `wasm::start` (the `wasm` module is gated
//! `#[cfg(target_arch = "wasm32")]`, so it's absent from native
//! rustdoc — named here as a code span rather than an intra-doc
//! link). It renders the PPU framebuffer to a `<canvas>` via the
//! 2D `ImageData` path and drives the run loop with
//! `requestAnimationFrame`.

#![warn(missing_docs)]

pub mod app;
pub mod audio;
pub mod cheats;
pub mod config;
pub mod debugger;
// v2.8.0 Phase 5 — the emulation core extracted from `App` (per-frame
// produce state; the boundary the emulation thread spawns onto).
pub mod emu;
// v2.8.0 Phase 5 increment 3 — the dedicated emulation thread (native,
// behind the default-ON `emu-thread` feature). Single-player frame
// production runs here, off the winit event-loop thread.
#[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
pub mod emu_thread;
// v1.1.0 beta.1 (T-110-A2) — CRT / scanline post-process wgsl pass.
pub mod crt;
pub mod gfx;
pub mod input;
pub mod movie_ui;
// v2.8.0 Phase 0 — frame-pacing / presentation / audio instrumentation
// (produced vs presented interval histograms, produce cost, audio-queue
// health). Target-agnostic; rendered by the debugger Performance panel.
pub mod perf;
// v2.8.0 — opt-in interval CSV performance logging (the Perf panel's
// "Logging" checkbox). Native-only: it writes files under `perf-logs/`.
#[cfg(not(target_arch = "wasm32"))]
pub mod perf_log;
// v2.8.0 Phase 1 — 4-tap Hermite resampler + Near's dynamic-rate-control
// law (the frontend half of "video master + audio DRC"; the core's sample
// output stays byte-identical).
pub mod resampler;
// v2.8.0 Phase 3 — run-ahead (removes the game's internal input lag via
// muted-frame + snapshot/restore cycles). Native-only at the call site;
// the module itself is target-agnostic and unit-tested headless.
pub mod runahead;
// v2.3.0 — netplay UI state machine + run-loop driver. Native-only: it
// drives a `std::net::UdpSocket` (absent on wasm32). The browser builds
// compile with this module absent and the netplay panel shows a
// "native-only" note.
#[cfg(not(target_arch = "wasm32"))]
pub mod netplay_ui;
pub mod ntsc;
// v2.7.0 — RetroAchievements session state. Native-only and behind the
// default-OFF `retroachievements` feature (it links the vendored rcheevos C
// library via `rustynes-cheevos`). The browser builds never see it.
#[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
pub mod ra_session;
pub mod save_state;
// v1.0.0 — the Save-States manager window (thumbnail grid). Native-only: the
// slot files live on the filesystem; on wasm the slots are in `localStorage`
// and the window is not built (the existing F1/F4 path is untouched).
#[cfg(not(target_arch = "wasm32"))]
pub mod save_states_ui;
// v1.0.0 — the always-on desktop UX shell (menu bar, status bar, settings
// window, welcome/about/shortcuts modals). Runs on native + `wasm-winit`; only
// the filesystem-backed actions are native-gated at the dispatch site.
pub mod ui_shell;

// v1.3.0 Sprint 1.4 — two wasm32 frontends, selected by cargo
// feature (each provides a unique `#[wasm_bindgen(start)]`):
//   - `wasm-winit` (default): the unified winit + wgpu + egui App,
//     same as native (debugger overlay + NTSC filter on the web).
//   - `wasm-canvas`: the lightweight canvas-2D embed mode (Web Audio
//     + localStorage save state, no debugger).
#[cfg(all(target_arch = "wasm32", feature = "wasm-canvas"))]
pub mod wasm;
#[cfg(all(target_arch = "wasm32", feature = "wasm-winit"))]
pub mod wasm_winit;
// Shared Web Audio output for both wasm frontends (Sprint 1.4c).
#[cfg(target_arch = "wasm32")]
pub mod wasm_audio;
// v1.6.0 Sprint 4 — shared browser I/O: localStorage save-states, base64
// codec, Blob downloads, file-picker triggers. Used by both wasm frontends.
#[cfg(target_arch = "wasm32")]
pub mod wasm_io;
// v2.6.0 — browser netplay over WebRTC (a WebSocket signaling client + the
// RtcPeerConnection / RtcDataChannel handshake yielding a `WebRtcTransport`
// that drives a `RollbackSession`). Compile-verified; a full browser session
// needs the signaling server deployed + two browsers (see `docs/netplay-webrtc.md`).
#[cfg(target_arch = "wasm32")]
pub mod wasm_netplay;
// v2.7.0 — the wasm-only browser netplay lobby UI (egui overlay) that drives
// `wasm_netplay::BrowserNetplay`. wasm-only.
#[cfg(target_arch = "wasm32")]
pub mod wasm_lobby;
