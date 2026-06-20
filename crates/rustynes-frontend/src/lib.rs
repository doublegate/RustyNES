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
// v1.7.0 "Forge" H3 — frontend stereo output DSP (panning / Schroeder reverb /
// headphone crossfeed). Bypass-by-default (center pan, 0% reverb, 0 crossfeed)
// reproduces today's mono-duplicated-to-stereo output bit-for-bit.
pub mod audio_dsp;
// v1.6.0 "Studio" Workstream G — A/V (video + synchronized audio) recording.
// A read-only frontend tap on the already-produced framebuffer + drained audio
// that pipes them to an external `ffmpeg` to mux an .mp4/.mkv. Native-only +
// behind the default-OFF `av-record` feature, so the shipped / wasm / `no_std`
// builds are byte-identical with it off. It NEVER mutates the core or the
// per-frame output, so the determinism contract is unaffected.
#[cfg(all(not(target_arch = "wasm32"), feature = "av-record"))]
pub mod av_record;
// About-dialog input helper (native only; safe Rust, portable across all arches).
#[cfg(not(target_arch = "wasm32"))]
mod about_fx;
pub mod cheats;
// v1.4.0 Workstream H — native CLI (clap 4) + structured help-topic registry.
// Native-only: a browser tab has no terminal, and the clap / clap_complete /
// color-print deps are gated out of the wasm target in `Cargo.toml`.
#[cfg(not(target_arch = "wasm32"))]
pub mod cli;
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
// v1.1.0 beta.2 (T-110-D2) — optional graphic EQ output stage (frontend-only).
pub mod eq;
// v1.1.0 — embedded app icon (winit window icon + About dialog). Native-only
// (`png` is in the cfg(not(wasm)) dep table; a browser tab has no window icon).
pub mod game_db;
pub mod genie_db;
// v1.7.0 "Forge" Workstream H9 — Game Genie encoder + `.tbl` text tables
// (frontend-only, pure; round-trips through the core decoder).
pub mod genie_encode;
pub mod gfx;
// v1.4.0 Workstream H3 — interactive ratatui help browser. Native-only +
// behind the default-on `help-tui` feature (a minimal build can drop it); the
// ratatui / crossterm deps are gated out of the wasm target in `Cargo.toml`.
#[cfg(all(not(target_arch = "wasm32"), feature = "help-tui"))]
pub mod help_tui;
// v1.7.0 "Forge" Workstream D1 — the HistoryViewer: a scrubbable full-session
// timeline over the rewind ring (per-frame input log + periodic start-anchors)
// with export-last-N-seconds-as-`.rnm`. Output-only: it observes the inputs the
// frontend already latched and copies the save-states the core already produced,
// so it cannot perturb the deterministic timeline. Native + `wasm-winit`.
pub mod history_viewer;
// v1.2.0 beta.2 (Workstream C3) — HD-pack / mod loader (native-only, default OFF).
#[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
pub mod hdpack;
// v1.6.0 "Studio" Workstream H — HD-pack HD AUDIO: `<bgm>`/`<sfx>` OGG tracks
// mixed into the frontend audio path on the `$4100` control register (output-
// only). Gated with `hdpack` (native-only, default OFF).
#[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
pub mod hd_audio;
// v1.7.0 "Forge" Workstream G5 — HD-Pack BUILDER. The authoring counterpart to
// `hdpack` (which plays packs): an in-emulator recorder that observes the same
// per-frame PPU tile-source telemetry and emits a Mesen-compatible `hires.txt`
// + `tiles.png` starter pack. Output-only + native-only; gated with `hdpack`,
// default OFF. See ADR 0017.
#[cfg(all(feature = "hd-pack", not(target_arch = "wasm32")))]
pub mod hdpack_builder;
#[cfg(not(target_arch = "wasm32"))]
pub mod icon;
pub mod icons;
// v1.7.0 "Forge" Workstream H5 — internationalization (i18n). A lightweight,
// compile-time string-catalog layer (no runtime file I/O / Fluent / ICU, so it
// is wasm-safe and adds only a few KiB). English is the default + fallback, so
// with the default locale every label is byte-identical to v1.6.0. See ADR 0023.
pub mod i18n;
pub mod input;
// v1.5.0 "Lens" Workstream I5 — shared lit-button colour palette for the
// consolidated "Input Display" panel (v1.7.0 "Forge" beta.5, #51).
pub mod input_colors;
// v1.7.0 "Forge" Workstream H9 — movie subtitles → SubRip (`.srt`) export
// (frontend-only, pure; driven by TAStudio markers).
pub mod movie_srt;
pub mod movie_ui;
// v1.2.0 Workstream B — ROM soft-patching (IPS/UPS/BPS). Pure byte ops applied
// to the in-memory ROM at the load chokepoint, before format detection, so the
// patched image flows through the deterministic parse unchanged.
pub mod patch;
// v1.7.0 "Forge" Workstream H4 — per-game `<rom>.json` config overlay (region /
// mapper / mirroring corrections + Vs. DIP switches), layered on the v1.2.0
// game-DB. Frontend-only; an absent file is a no-op, so the default load path
// stays byte-identical and the deterministic core never consults it.
pub mod per_game;
// v2.8.0 Phase 0 — frame-pacing / presentation / audio instrumentation
// (produced vs presented interval histograms, produce cost, audio-queue
// health). Target-agnostic; rendered by the debugger Performance panel.
pub mod perf;
// v1.5.0 "Lens" Workstream H1 — lock-free triple-buffer framebuffer handoff,
// so the present (winit) thread never blocks on the emu mutex to copy the
// produced frame. Native-only (it exists to decouple the dedicated emulation
// thread from the present thread; the wasm builds are single-threaded).
#[cfg(all(not(target_arch = "wasm32"), feature = "emu-thread"))]
pub mod present_buffer;
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
pub mod ntsc_bisqwit;
// v1.6.0 "Studio" Workstream I — shader/filter ecosystem additions: an
// LMP88959-style RGBA composite NTSC/PAL pass, hqNx/xBRZ pixel-art upscaler
// passes, and a constrained RetroArch `.slangp`/`.cgp` preset importer. All
// extend the v1.2.0 composable ShaderStack (ADR 0013) and are output-only.
pub mod ntsc_lmp88959;
pub mod save_state;
pub mod slang_preset;
pub mod upscale;
// v1.2.0 C2 — composable post-process shader stack (ping-pong RT executor +
// `#pragma parameter` model + CRT preset bank). An empty stack falls through to
// the existing direct blit (byte-identical), so this is purely additive.
pub mod shader_pass;
// v1.0.0 — the Save-States manager window (thumbnail grid). Native-only: the
// slot files live on the filesystem; on wasm the slots are in `localStorage`
// and the window is not built (the existing F1/F4 path is untouched).
#[cfg(not(target_arch = "wasm32"))]
pub mod save_states_ui;
// v1.7.0 "Forge" Workstream E1 — the host-mediated `comm.*` IPC bridge
// (native-only, behind `script-ipc`). The component that OWNS every TCP / HTTP /
// WebSocket / memory-mapped-file connection so the Lua sandbox never gets a raw
// socket; off by default, disabled under a locked session. See ADR 0016.
#[cfg(all(not(target_arch = "wasm32"), feature = "script-ipc"))]
pub mod script_host;
// v1.4.0 Workstream D (D1) — debugger symbol/label file loading
// (`.sym` / Mesen `.mlb` / FCEUX `.nl`). Frontend display aid only; the parsed
// `address -> label` map annotates the disassembler / breakpoint / trace views
// and never touches the deterministic core.
pub mod symbols;
// v1.0.0 — the always-on desktop UX shell (menu bar, status bar, settings
// window, welcome/about/shortcuts modals). Runs on native + `wasm-winit`; only
// the filesystem-backed actions are native-gated at the dispatch site.
pub mod ui_shell;

// v1.6.0 Workstream A — the TAStudio piano-roll TAS *editor*: a frame-keyed
// save-state greenzone + an editable input log + deterministic seek/edit
// plumbing. The egui piano-roll grid (A2) and branches/projects (A4) layer on
// top of this model.
pub mod tastudio;

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
// v1.4.0 Workstream E2 — IndexedDB save-state store (async, binary,
// multi-slot, thumbnail-grid-backing) with a localStorage fallback +
// one-time migration. Used by both wasm frontends. wasm-only.
#[cfg(target_arch = "wasm32")]
pub mod wasm_idb;
// v1.4.0 Workstream E2 — the browser Save-States manager (egui thumbnail
// grid backed by the IndexedDB store). wasm-only.
#[cfg(target_arch = "wasm32")]
pub mod wasm_save_states;
// v1.2.0 Workstream F1/F2 — shared on-screen touch input (Pointer-Events
// overlay → button/Power-Pad mask). Read at the late-latch by both wasm
// frontends, so touch is recorded/replayed like a keypress. wasm-only.
#[cfg(target_arch = "wasm32")]
pub mod wasm_touch;
// v1.7.0 "Forge" beta.5 Workstream H6 — shared browser Gamepad API input
// (`navigator.getGamepads()` polled in JS → button mask). Read at the same
// late-latch as touch/keyboard by both wasm frontends. wasm-only.
#[cfg(target_arch = "wasm32")]
pub mod wasm_gamepad;
// v1.7.0 "Forge" beta.5 Workstream H6 — `?settings=` share-links: a curated
// base64url-encoded subset of `Config` read on load + minted for "Copy share
// link". wasm-only. See ADR 0022.
#[cfg(target_arch = "wasm32")]
pub mod wasm_share;
// v1.2.0 Workstream F4 — EXPERIMENTAL wasm Lua engine JS bridge (piccolo). Only
// when the off-by-default `script-wasm` feature is on. See ADR 0012.
#[cfg(all(target_arch = "wasm32", feature = "script-wasm"))]
pub mod wasm_script;
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
// v1.5.0 "Lens" Workstream G — EXPERIMENTAL casual-only browser RetroAchievements
// (ADR 0015). wasm-only + behind the default-OFF `browser-cheevos` feature. It
// bridges to an Emscripten-built rcheevos side module (`web/cheevos/`) rather
// than the native `rustynes-cheevos` C FFI; casual-only is structural (no
// hardcore API). Off by default, so the shipped wasm builds never see it.
#[cfg(all(target_arch = "wasm32", feature = "browser-cheevos"))]
pub mod wasm_cheevos;
