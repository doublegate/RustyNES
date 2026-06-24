// The debugger panels are UI scaffolding with lots of layout math; the
// pedantic / cast / floats complaints here aren't really actionable.
#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_precision_loss,
    clippy::cast_possible_wrap,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::suboptimal_flops,
    clippy::items_after_statements,
    clippy::struct_excessive_bools,
    clippy::too_many_arguments,
    clippy::too_many_lines
)]

//! egui-wgpu debugger overlay.
//!
//! Sprint 5-3 (T-53-001 ... T-53-007 + T-52-007). The overlay is rendered
//! into the same surface frame as the main NES blit, on top of everything.
//! Toggle via `~` (configurable in `[input.system].debug_overlay`).
//!
//! The overlay is read-only: it never advances emulator-visible state.
//! Panels poll the inspection API exposed by `rustynes_core::Nes` once per
//! visible frame at 60 Hz.
//!
//! Sub-modules:
//!
//! (The panel sub-modules are private implementation detail; named
//! here as code spans rather than intra-doc links since they're not
//! part of the crate's public API.)
//!
//! - `cpu_panel` — registers + flags + scrollable disassembly.
//! - `ppu_panel` — nametable / pattern table / palette / scroll cursor.
//! - `oam_panel` — sprite list + visual grid.
//! - `apu_panel` — per-channel waveform scope.
//! - `memory_panel` — CPU + PPU bus hex viewer with go-to-address.
//! - `mapper_panel` — mapper bank registers + IRQ counter state.
//! - `input_rebind_panel` — modal rebinding flow (T-52-007).
//! - `cheat_panel` — Game Genie cheat list + per-ROM persistence (v1.6.0).
//! - `settings_panel` — graphics / audio / rewind config editor (v1.7.0).

use std::sync::Arc;

use rustynes_core::Nes;
use winit::event::WindowEvent;
use winit::window::Window;

use crate::config::Config;
use crate::movie_ui::{MovieStatus, ReplayInfo};
use crate::ui_shell::{ShellFrame, ShellOutput, UiShell};
pub use replay_panel::ReplayRequest;
pub use tastudio_panel::TasRequest;

mod access_counter;
mod apu_panel;
// v1.7.0 "Forge" Workstream A3 — inline 6502 assembler used by the CPU panel.
mod assembler;
// v1.7.0 "Forge" Workstream A2 — iNES/NES 2.0 header editor + Cartridge Info
// pane. Native-only (edits a ROM file on disk via std::fs + rfd).
#[cfg(not(target_arch = "wasm32"))]
mod header_editor;
// v2.7.1 — RetroAchievements badge-image cache (native-only, feature-gated).
// The badge worker + texture cache; the default / wasm builds never see it.
#[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
mod badge_cache;
// v1.7.0 "Forge" Workstream C (C1) — live call-stack tracker + step verbs over
// the observational `debug-hooks` exec / interrupt logs.
mod callstack;
mod cheat_panel;
mod cheevos_panel;
mod cpu_panel;
// v1.5.0 "Lens" Workstream I10 — in-app Documentation browser. Native-only (it
// reuses the native-only `cli::HELP_TOPICS` registry; a browser tab has no
// terminal help to share with).
#[cfg(not(target_arch = "wasm32"))]
mod doc_panel;
mod event_panel;
// v1.8.9 "Backlog" — BasicBot input-search control panel.
mod basic_bot_panel;
// v1.6.0 "Studio" Workstream C (C1) — the debugger expression evaluator (CPU /
// PPU / memory / access-context tokens + the C-style operator set). Shared by
// the watch panel's conditional breakpoints / watchpoints / watch window /
// conditional trace. Pure + frontend-only; the unit tests live in the module.
mod expr;
mod game_db_panel;
// v1.5.0 "Lens" Workstream A4 — HD-pack per-pixel inspector (native + hd-pack).
// `pub(crate)` so `app.rs` can drive its `show` (the panel needs the compositor
// + per-frame snapshots the app owns, so its render lives there, not here).
#[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
pub(crate) mod hd_pixel_panel;
// v1.7.0 "Forge" beta.5 (#51) — the live "Input Display" panel: a single
// consolidated controller HUD covering the standard pads + every expansion
// peripheral (the former standalone `input_display_panel` was folded in, its
// superset capability retained). The module keeps its historical filename; the
// user-facing window + menu entry read "Input Display".
mod input_miniatures_panel;
mod input_rebind_panel;
mod mapper_panel;
mod memory_compare_panel;
mod memory_panel;
mod netplay_panel;
mod nsf_panel;
mod oam_panel;
mod replay_panel;
// v2.8.0 Phase 0 — frame-pacing / audio-health instrumentation panel.
mod perf_panel;
mod ppu_panel;
mod script_panel;
mod settings_panel;
// v1.7.0 "Forge" Workstream C (C3) — ca65/cc65 `.dbg` source-line mapping
// (frontend parser; annotates the disassembly with original source lines).
mod source_map;
mod tastudio_panel;
mod trace_panel;
// v1.6.0 "Studio" Workstream C (C1 keystone + C4 free riders) — the
// Mesen2-class Watch / conditional-breakpoint / watchpoint panel built on the
// `expr` evaluator + the core's observational `debug-hooks` per-frame logs.
mod watch_panel;

pub use cheevos_panel::{CheevosRequest, CheevosStatusView};
// v1.5.0 "Lens" Workstream A1 — the input-miniatures snapshot the app pushes.
pub use input_miniatures_panel::{ExpansionMini, MiniaturesSnapshot};
pub use netplay_panel::{
    CrcCompareView, NetplayDiagnosticsView, NetplayPhaseView, NetplayRequest, NetplayStatusView,
};
pub use script_panel::ScriptAction;
pub use settings_panel::SettingsApply;

/// A non-chip tool panel surfaced directly from the menu bar (v1.0.0).
///
/// These render as floating windows regardless of whether the deep debugger
/// overlay is toggled on (see [`DebuggerOverlay::open_panel`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolPanel {
    /// Game Genie / raw cheats panel.
    Cheats,
    /// Graphics / audio / rewind settings panel.
    Settings,
    /// Netplay host/join panel (native).
    Netplay,
    /// `RetroAchievements` login/list panel.
    Cheevos,
    /// Performance instrumentation panel.
    Perf,
    /// Input rebinding panel.
    Input,
    /// Per-game ROM-database editor (v1.2.0 Workstream B, B4).
    GameDb,
    /// Live "Input Display" panel — the consolidated controller + expansion-
    /// device HUD (v1.7.0 "Forge" beta.5, #51; the v1.5.0 "Lens" Workstream A1
    /// Input Miniatures overlay absorbed the former standalone Input Display).
    InputDisplay,
    /// Replay / TAS window (v1.5.0 "Lens" Workstream C2).
    Replay,
    /// `TAStudio` piano-roll editor (v1.6.0 "Studio" Workstream A2).
    TasStudio,
    /// HD-pack per-pixel inspector (v1.5.0 "Lens" Workstream A4; native +
    /// `hd-pack`). The enum variant is unconditional so the menu IA + dispatch
    /// match stay exhaustive; the actual panel + open path is feature-gated.
    HdPixelInspector,
    /// v1.8.9 — the `BasicBot` input-search control panel.
    BasicBot,
}

/// A chip-inspection panel surfaced from the Debug menu (v1.0.0).
///
/// These only render while the deep overlay is visible (they need `&mut Nes`);
/// opening one forces the overlay visible (see
/// [`DebuggerOverlay::open_chip_panel`]).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ChipPanel {
    /// CPU registers + disassembly.
    Cpu,
    /// PPU nametable / pattern / palette viewer.
    Ppu,
    /// OAM sprite list + grid.
    Oam,
    /// APU per-channel scope.
    Apu,
    /// CPU/PPU bus hex viewer.
    Memory,
    /// Memory-search / cheat-hunt panel (v1.3.0 Workstream C, C3).
    MemoryCompare,
    /// Mapper bank registers + IRQ state.
    Mapper,
    /// Cycle trace logger (T-110-C2).
    Trace,
    /// Watch / conditional-breakpoint / watchpoint panel (v1.6.0 "Studio"
    /// Workstream C, C1 keystone + C4 free riders).
    Watch,
    /// Event viewer (T-110-C3): scanline×dot write-event timeline.
    Events,
    /// NSF music player (T-110-D1): track selector + metadata.
    Nsf,
    /// Lua script console (T-110-E5): load/reload/stop + log.
    Script,
    /// v1.7.0 "Forge" Workstream A2 — Cartridge Info / iNES-NES2.0 header editor
    /// (native-only; edits a ROM file on disk).
    #[cfg(not(target_arch = "wasm32"))]
    HeaderEditor,
}

/// State of the debugger overlay.
pub struct DebuggerOverlay {
    /// egui frontend state (window-event integration).
    state: egui_winit::State,
    /// egui rendering pipeline (wgpu-backed).
    renderer: egui_wgpu::Renderer,
    /// Toggle visibility (default off). Bound to `~`.
    visible: bool,
    /// Per-panel "open" flags.
    show_cpu: bool,
    show_ppu: bool,
    show_oam: bool,
    show_apu: bool,
    show_memory: bool,
    show_memory_compare: bool,
    show_mapper: bool,
    show_trace: bool,
    /// v1.7.0 "Forge" Workstream A2 — Cartridge Info / header-editor window open
    /// flag (native-only; edits a ROM file on disk).
    #[cfg(not(target_arch = "wasm32"))]
    show_header_editor: bool,
    /// Watch / conditional-breakpoint / watchpoint panel open flag (v1.6.0
    /// "Studio" Workstream C).
    show_watch: bool,
    show_events: bool,
    /// v1.8.9 — `BasicBot` control panel visible.
    show_basic_bot: bool,
    show_nsf: bool,
    show_replay: bool,
    /// `TAStudio` piano-roll editor open flag (v1.6.0 "Studio" Workstream A2).
    show_tas: bool,
    show_script: bool,
    show_input: bool,
    show_cheat: bool,
    show_settings: bool,
    show_netplay: bool,
    show_cheevos: bool,
    show_perf: bool,
    show_game_db: bool,
    /// "Input Display" panel open flag (v1.7.0 "Forge" beta.5, #51; née the
    /// v1.5.0 A1 Input Miniatures overlay).
    show_input_display: bool,
    /// v1.5.0 A4 — HD-pack pixel inspector open flag (native + `hd-pack`).
    #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
    show_hd_pixel: bool,
    /// CPU panel state (auto-follow PC, address jump, ...).
    cpu_ui: cpu_panel::CpuPanelState,
    /// PPU panel state.
    ppu_ui: ppu_panel::PpuPanelState,
    /// OAM panel state.
    oam_ui: oam_panel::OamPanelState,
    /// v1.7.0 "Forge" Workstream A2 — Cartridge Info / header-editor state
    /// (native-only).
    #[cfg(not(target_arch = "wasm32"))]
    header_editor_ui: header_editor::HeaderEditorState,
    /// APU panel state (rolling sample buffers).
    apu_ui: apu_panel::ApuPanelState,
    /// Memory hex viewer state.
    memory_ui: memory_panel::MemoryPanelState,
    /// Memory-compare (cheat-hunt) panel state.
    memory_compare_ui: memory_compare_panel::MemoryComparePanelState,
    /// Mapper panel state (currently stateless).
    mapper_ui: mapper_panel::MapperPanelState,
    /// Cycle trace logger panel state (T-110-C2).
    trace_ui: trace_panel::TracePanelState,
    /// Watch / conditional-breakpoint / watchpoint panel state — also owns the
    /// per-frame observational replay that evaluates the expressions (v1.6.0
    /// "Studio" Workstream C, C1 + C4).
    watch_ui: watch_panel::WatchPanelState,
    /// Event viewer panel state (T-110-C3).
    event_ui: event_panel::EventPanelState,
    /// v1.8.9 — `BasicBot` panel state.
    basic_bot_ui: basic_bot_panel::BasicBotPanel,
    /// NSF player panel state (T-110-D1).
    nsf_ui: nsf_panel::NsfPanelState,
    /// Replay / TAS window state (v1.5.0 "Lens" Workstream C2).
    replay_ui: replay_panel::ReplayPanelState,
    /// `TAStudio` piano-roll UI state — pending requests + view toggles
    /// (v1.6.0 "Studio" Workstream A2).
    tas_ui: tastudio_panel::TasStudioPanelState,
    /// The `TAStudio` editor model, present while a `TAStudio` session is active.
    /// Lives here (not in the app) so the panel renders in the always-on tool
    /// loop; the app drives its `Nes`-touching ops after the egui pass.
    tas_editor: Option<crate::tastudio::TasEditor>,
    /// Lua script console state (T-110-E5).
    script_ui: script_panel::ScriptPanelState,
    /// Input rebind modal state.
    input_ui: input_rebind_panel::InputPanelState,
    /// "Input Display" panel state (v1.7.0 "Forge" beta.5, #51).
    input_display_ui: input_miniatures_panel::InputMiniaturesPanelState,
    /// The live input snapshot the app pushes each frame (standard pads + the
    /// active expansion device), drawn by the "Input Display" panel.
    input_display: MiniaturesSnapshot,
    /// v1.5.0 A4 — HD-pack pixel inspector state (native + `hd-pack`).
    #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
    hd_pixel_ui: hd_pixel_panel::HdPixelPanelState,
    /// v1.5.0 I10 — Documentation browser state (native-only).
    #[cfg(not(target_arch = "wasm32"))]
    doc_ui: doc_panel::DocPanelState,
    /// v1.5.0 I10 — whether the Documentation window is open (native-only).
    #[cfg(not(target_arch = "wasm32"))]
    show_documentation: bool,
    /// Game Genie cheat panel state (v1.6.0).
    cheat_ui: cheat_panel::CheatPanelState,
    /// ROM-database editor panel state (v1.2.0 Workstream B, B4).
    game_db_ui: game_db_panel::GameDbPanelState,
    /// CRC32 of the currently-loaded ROM (PRG+CHR, header-excluded), pushed by
    /// [`DebuggerOverlay::set_rom_crc`] at load. `None` for FDS / NSF / no ROM.
    /// The ROM-database editor keys its overlay edits on this.
    rom_crc: Option<u32>,
    /// Graphics / audio / rewind settings panel state (v1.7.0).
    settings_ui: settings_panel::SettingsPanelState,
    /// Netplay host/join panel + status HUD state (v2.3.0).
    netplay_ui: netplay_panel::NetplayPanelState,
    /// Performance instrumentation panel state (v2.8.0 Phase 0).
    perf_ui: perf_panel::PerfPanelState,
    /// `RetroAchievements` login/list panel + status HUD state (v2.7.0).
    cheevos_ui: cheevos_panel::CheevosPanelState,
    /// v2.7.0 — `true` when an RA session is active AND hardcore mode is on.
    /// Pushed by [`Self::set_cheevos_status`]; gates the Memory panel (it would
    /// otherwise be a RAM-watch cheat surface) to a "disabled in hardcore"
    /// placeholder.
    hardcore_active: bool,
    /// Native-only per-ROM cheat persistence context (data dir + ROM hash),
    /// set by the app via [`DebuggerOverlay::set_cheat_persist`] each time a
    /// ROM is loaded. `None` until then; absent on wasm32 (no filesystem).
    #[cfg(not(target_arch = "wasm32"))]
    cheat_persist: Option<cheat_panel::CheatPersist>,
    /// Most recent measured frames-per-second (wall-clock moving average).
    /// Updated by [`DebuggerOverlay::set_fps`] from the frontend's pacing loop.
    /// v1.7.0 "Forge" beta.5 (#55) — the toolbar HUD that displayed this was
    /// removed; the status bar shows FPS from its own `ShellFrame`. The setter
    /// + field are retained for the stable public API (and future panels).
    #[allow(dead_code)]
    fps: f32,
    /// v1.4.0 Sprint 4.2 — current TAS movie record/playback status, pushed by
    /// [`DebuggerOverlay::set_movie_status`] from the pacing loop. v1.7.0
    /// "Forge" beta.5 (#55) — retained for the public API after the toolbar HUD
    /// that displayed it was removed (movie state is now in the menu + status
    /// bar).
    #[allow(dead_code)]
    movie: MovieStatus,
    /// v2.7.1 — `RetroAchievements` badge-image cache (achievement icons in the
    /// panel rows + unlock toasts). Lazily created the first time a badge URL is
    /// rendered so the worker thread is only spawned when RA is actually in use.
    /// Native-only + feature-gated; absent on the default / wasm builds.
    #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
    badge_cache: Option<badge_cache::BadgeCache>,
    /// v1.0.0 (audit m6) — the last theme applied to the egui context, so
    /// [`crate::ui_shell::apply_theme`] only calls `ctx.set_visuals` on a change
    /// instead of rebuilding the whole `Visuals` every frame.
    last_theme: Option<crate::config::AppTheme>,
    /// v1.4.0 Workstream D (D1) — loaded debugger symbols (`address -> label`),
    /// merged from `.sym` / Mesen `.mlb` / FCEUX `.nl` files. Consulted by the
    /// CPU disassembler + breakpoint list + trace view to annotate raw
    /// addresses. Empty until the user loads a file (display-only; never touches
    /// the deterministic core).
    symbols: crate::symbols::SymbolMap,
    /// v1.4.0 Workstream D (D1) — the last symbol-load status line (file +
    /// label count, or an error), shown in the CPU panel.
    symbols_status: Option<String>,
    /// v1.7.0 "Forge" Workstream C (C1) — live 6502 call stack + step verbs,
    /// rebuilt each frame from the observational `debug-hooks` exec / interrupt
    /// logs. Output-only (never touches the deterministic core).
    callstack: callstack::CallstackTracker,
    /// v1.7.0 "Forge" Workstream C (C2) — per-address read/write/exec counters
    /// plus uninitialized-read detection, folded from the per-frame access and
    /// exec logs. Output-only side-array.
    access_counter: access_counter::MemoryAccessCounter,
    /// v1.7.0 "Forge" Workstream C (C3) — `address -> (source file, line)` map
    /// parsed from a ca65/cc65 `.dbg` file. Annotates the disassembly with the
    /// original source line. Empty until a `.dbg` is loaded (display-only).
    source_map: source_map::SourceMap,
    /// v1.7.0 "Forge" Workstream C (C3) — the last `.dbg`-load status line.
    source_map_status: Option<String>,
}

/// v1.7.1 — the pure visibility predicate behind
/// [`DebuggerOverlay::any_chip_panel_open`]: `true` when ANY chip-inspection
/// panel is open. Kept as a free function (one bool per chip `show_*` flag) so
/// the visibility lifecycle is unit-testable without constructing the
/// GPU-backed overlay. `header_editor` is the native-only header-editor flag
/// (pass `false` on wasm, where that panel doesn't exist).
#[must_use]
#[allow(clippy::fn_params_excessive_bools, clippy::too_many_arguments)]
const fn chip_panels_open(
    cpu: bool,
    ppu: bool,
    oam: bool,
    apu: bool,
    memory: bool,
    memory_compare: bool,
    mapper: bool,
    trace: bool,
    watch: bool,
    events: bool,
    nsf: bool,
    script: bool,
    header_editor: bool,
) -> bool {
    cpu || ppu
        || oam
        || apu
        || memory
        || memory_compare
        || mapper
        || trace
        || watch
        || events
        || nsf
        || script
        || header_editor
}

impl DebuggerOverlay {
    /// Construct the overlay, allocating egui-wgpu textures + binding the
    /// winit integration.
    #[must_use]
    pub fn new(
        device: &wgpu::Device,
        window: &Window,
        surface_format: wgpu::TextureFormat,
    ) -> Self {
        let ctx = egui::Context::default();
        // v1.2.0 (H3) — register the Font Awesome Solid icon font so the menu
        // bar can prefix labels with glyphs. Purely cosmetic + a trailing
        // fallback, so ordinary UI text is unaffected and missing glyphs
        // degrade to a box rather than crashing.
        crate::icons::install(&ctx);
        let viewport_id = ctx.viewport_id();
        let state = egui_winit::State::new(ctx, viewport_id, window, None, None, None);
        let renderer = egui_wgpu::Renderer::new(
            device,
            surface_format,
            egui_wgpu::RendererOptions {
                msaa_samples: 1,
                depth_stencil_format: None,
                dithering: false,
                predictable_texture_filtering: false,
            },
        );
        Self {
            state,
            renderer,
            visible: false,
            // Default the CPU/PPU sub-windows CLOSED so opening the debugger
            // (`~`) just shows the toolbar; the user opens the panels they want
            // via the CPU/PPU/OAM/APU/Memory checkboxes.
            show_cpu: false,
            show_ppu: false,
            show_oam: false,
            show_apu: false,
            show_memory: false,
            show_memory_compare: false,
            show_mapper: false,
            show_trace: false,
            #[cfg(not(target_arch = "wasm32"))]
            show_header_editor: false,
            show_watch: false,
            show_events: false,
            show_basic_bot: false,
            show_nsf: false,
            show_replay: false,
            show_tas: false,
            show_script: false,
            show_input: false,
            show_cheat: false,
            show_settings: false,
            show_netplay: false,
            show_cheevos: false,
            show_perf: false,
            show_game_db: false,
            show_input_display: false,
            #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
            show_hd_pixel: false,
            cpu_ui: cpu_panel::CpuPanelState::default(),
            ppu_ui: ppu_panel::PpuPanelState::default(),
            oam_ui: oam_panel::OamPanelState::default(),
            #[cfg(not(target_arch = "wasm32"))]
            header_editor_ui: header_editor::HeaderEditorState::default(),
            apu_ui: apu_panel::ApuPanelState::default(),
            memory_ui: memory_panel::MemoryPanelState::default(),
            memory_compare_ui: memory_compare_panel::MemoryComparePanelState::default(),
            mapper_ui: mapper_panel::MapperPanelState::default(),
            trace_ui: trace_panel::TracePanelState::default(),
            watch_ui: watch_panel::WatchPanelState::default(),
            event_ui: event_panel::EventPanelState::default(),
            basic_bot_ui: basic_bot_panel::BasicBotPanel::default(),
            nsf_ui: nsf_panel::NsfPanelState::default(),
            replay_ui: replay_panel::ReplayPanelState::default(),
            tas_ui: tastudio_panel::TasStudioPanelState::default(),
            tas_editor: None,
            script_ui: script_panel::ScriptPanelState::default(),
            input_ui: input_rebind_panel::InputPanelState::default(),
            input_display_ui: input_miniatures_panel::InputMiniaturesPanelState,
            input_display: MiniaturesSnapshot::default(),
            #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
            hd_pixel_ui: hd_pixel_panel::HdPixelPanelState::default(),
            #[cfg(not(target_arch = "wasm32"))]
            doc_ui: doc_panel::DocPanelState::default(),
            #[cfg(not(target_arch = "wasm32"))]
            show_documentation: false,
            cheat_ui: cheat_panel::CheatPanelState::default(),
            game_db_ui: game_db_panel::GameDbPanelState::default(),
            rom_crc: None,
            settings_ui: settings_panel::SettingsPanelState::default(),
            netplay_ui: netplay_panel::NetplayPanelState::default(),
            perf_ui: perf_panel::PerfPanelState::default(),
            cheevos_ui: cheevos_panel::CheevosPanelState::default(),
            hardcore_active: false,
            #[cfg(not(target_arch = "wasm32"))]
            cheat_persist: None,
            fps: 0.0,
            movie: MovieStatus::default(),
            #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
            badge_cache: None,
            last_theme: None,
            symbols: crate::symbols::SymbolMap::default(),
            symbols_status: None,
            callstack: callstack::CallstackTracker::default(),
            access_counter: access_counter::MemoryAccessCounter::default(),
            source_map: source_map::SourceMap::default(),
            source_map_status: None,
        }
    }

    /// v1.4.0 Workstream D (D1) — merge a parsed symbol file's `text` (in
    /// `format`) into the debugger's label map and record a status line. The
    /// `name` is just for the status message (the picked file's display name).
    /// Display-only; the map annotates the disassembler / breakpoint / trace
    /// views and never touches the core.
    pub fn load_symbols(&mut self, name: &str, text: &str, format: crate::symbols::SymbolFormat) {
        let added = self.symbols.merge_str(text, format);
        self.symbols_status = Some(format!(
            "{name}: +{added} labels ({} total)",
            self.symbols.len()
        ));
    }

    /// v1.4.0 Workstream D (D1) — drop every loaded symbol.
    pub fn clear_symbols(&mut self) {
        self.symbols.clear();
        self.symbols_status = Some("symbols cleared".to_owned());
    }

    /// v1.5.0 Workstream B (B4) — the loaded symbols as `(address, label)` pairs,
    /// for pushing into the Lua scripting engine's `sym:` query tables.
    #[must_use]
    pub fn symbol_pairs(&self) -> Vec<(u16, String)> {
        self.symbols.pairs()
    }

    /// Update the measured wall-clock FPS shown in the top toolbar. Called
    /// from the frontend's wall-clock pacer on each completed frame.
    pub const fn set_fps(&mut self, fps: f32) {
        self.fps = fps;
    }

    /// Update the TAS movie record/playback status shown in the top
    /// toolbar. Called from the frontend's pacer alongside [`Self::set_fps`].
    pub const fn set_movie_status(&mut self, status: MovieStatus) {
        self.movie = status;
    }

    /// v1.5.0 "Lens" Workstream C2 — push the movie status + port-topology /
    /// timebase snapshot for the Replay / TAS window. Called from the pacer
    /// alongside [`Self::set_movie_status`]. Display-only.
    pub fn set_replay_info(&mut self, status: MovieStatus, info: ReplayInfo) {
        self.replay_ui.set(status, info);
    }

    /// v1.5.0 "Lens" Workstream C2 — return (and clear) the pending Replay-window
    /// request (record/play/branch/stop/seek). The app dispatches it under the
    /// emu lock.
    pub fn take_replay_request(&mut self) -> Option<ReplayRequest> {
        self.replay_ui.take_request()
    }

    /// v1.6.0 "Studio" A2 — `true` while a `TAStudio` editing session is active.
    #[must_use]
    pub const fn tas_active(&self) -> bool {
        self.tas_editor.is_some()
    }

    /// v1.6.0 "Studio" A2 — `true` while the `TAStudio` window is open.
    #[must_use]
    pub const fn tas_visible(&self) -> bool {
        self.show_tas
    }

    /// v1.6.0 "Studio" A2 — install the editor model for a new `TAStudio` session
    /// (the app builds it from the current `Nes`, which the editor needs).
    pub fn set_tas_editor(&mut self, editor: crate::tastudio::TasEditor) {
        self.tas_editor = Some(editor);
    }

    /// v1.6.0 "Studio" A2 — mutable access to the editor so the app can apply
    /// `Nes`-touching requests (seek / branch / record) after the egui pass.
    pub const fn tas_editor_mut(&mut self) -> Option<&mut crate::tastudio::TasEditor> {
        self.tas_editor.as_mut()
    }

    /// v1.7.0 "Forge" Workstream B (B1) — read-only access to the editor so the
    /// app can build the `tastudio.*` Lua snapshot pushed into the script engine.
    #[must_use]
    pub const fn tas_editor(&self) -> Option<&crate::tastudio::TasEditor> {
        self.tas_editor.as_ref()
    }

    /// v1.6.0 "Studio" A2 — drain the pending `TAStudio` requests for the app to
    /// dispatch under the emu lock.
    pub fn take_tas_requests(&mut self) -> Vec<TasRequest> {
        self.tas_ui.take_requests()
    }

    /// v1.6.0 "Studio" A2 — end the `TAStudio` session and close its window.
    /// The app calls this whenever the loaded ROM changes (load / close /
    /// power-cycle): a session anchors on one `Nes`, so a stale editor would
    /// otherwise replay its inputs/branches against a different game.
    pub fn clear_tas_editor(&mut self) {
        self.tas_editor = None;
        self.show_tas = false;
    }

    /// Returns `true` when the overlay is currently visible. The render
    /// path uses this to pick its emu-lock policy (v2.8.0 Phase 5): the
    /// egui pass needs `&mut Nes`, so a visible overlay holds the lock
    /// across the render; hidden renders from the staging copy instead.
    ///
    /// v1.7.1 — derived directly from the live chip-panel state rather than the
    /// cached `visible` field, so `app.rs`'s pre-egui-pass `dbg_visible` /
    /// `needs_nes` read engages the heavier lock-holding render branch ONLY
    /// while a chip panel is actually open (the field is also kept in sync by
    /// `recompute_visible` for the in-closure `render_shell` branch).
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.any_chip_panel_open()
    }

    /// v1.6.0 — point the cheat panel at a freshly-loaded ROM: store the
    /// per-ROM persistence context (data dir + ROM hash) and seed the panel
    /// with that ROM's persisted cheats. Native-only — wasm32 has no
    /// filesystem (the in-memory panel still works there via the cheat panel).
    ///
    /// The caller (the app's ROM-load path) is responsible for actually
    /// applying the enabled codes to the `Nes`; this only updates the UI
    /// state so the panel shows the right list.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_cheat_persist(
        &mut self,
        data_dir: std::path::PathBuf,
        rom_sha256: [u8; 32],
        cheats: Vec<crate::cheats::CheatEntry>,
        raw: Vec<crate::cheats::RawCheat>,
    ) {
        self.cheat_ui.set_cheats(cheats, raw);
        self.cheat_persist = Some(cheat_panel::CheatPersist {
            data_dir,
            rom_sha256,
        });
    }

    /// v1.7.0 — the currently-ENABLED raw RAM cheats from the cheat panel,
    /// cloned for the app's produce path. Pulled once per pacer iteration so
    /// the per-frame poke loop sees the live edited list without threading the
    /// panel through the produce call stack. Empty when nothing is enabled.
    ///
    /// v1.6.0 "Studio" Workstream C (C2/C3) — the Memory hex editor's frozen
    /// bytes and the Memory Compare RAM-Watch frozen entries are merged in here,
    /// so a freeze in either tool routes through the SAME post-frame raw-cheat
    /// overlay (the determinism contract holds; the no-freeze path is empty).
    #[must_use]
    pub fn enabled_raw_cheats(&self) -> Vec<crate::cheats::RawCheat> {
        let mut cheats = self.cheat_ui.enabled_raw_cheats();
        cheats.extend(self.memory_ui.freeze_cheats());
        cheats.extend(self.memory_compare_ui.freeze_cheats());
        cheats
    }

    /// v1.7.0 "Forge" Workstream A1 — drain the one-shot editing-tool writeback
    /// edits queued by the PPU panel (tile/CHR, palette, nametable) and the OAM
    /// panel. The app forwards these into the gated post-frame poke path
    /// (`EmuCore::debug_pokes`), where they are applied after the next frame
    /// under the SAME `emu.write` gate as the cheat pokes — a no-op under
    /// netplay / TAS replay/record / RA-hardcore. Empty when no edit is pending,
    /// so the no-edit path is byte-identical.
    #[must_use]
    pub fn take_debug_pokes(&mut self) -> Vec<crate::emu::DebugPoke> {
        let mut pokes = self.ppu_ui.take_pokes();
        pokes.extend(self.oam_ui.take_pokes());
        pokes.extend(self.cpu_ui.take_pokes());
        pokes
    }

    /// v1.0.0 (UX3 BUG-3) — re-apply the cheat panel's enabled Game Genie codes
    /// to `nes`. The app calls this after a Reset / Power-Cycle so the live core
    /// reflects the configured codes even when the Cheats panel is closed (the
    /// panel's own every-frame resync only runs while it is open). An empty
    /// enabled set clears the (already-empty) map — the no-cheat path stays
    /// byte-identical.
    pub fn reapply_genie_codes(&mut self, nes: &mut Nes) {
        self.cheat_ui.reapply_to_nes(nes);
    }

    /// v1.2.0 (Workstream B, B4) — record the loaded ROM's CRC32 (PRG+CHR,
    /// header-excluded) so the ROM-database editor panel can key its overlay
    /// edits. `None` for FDS / NSF images (no CRC entry).
    pub fn set_rom_crc(&mut self, crc: Option<u32>) {
        self.rom_crc = crc;
    }

    /// Returns `true` if the overlay currently wants the keyboard (i.e.
    /// the user is typing into a text input). The main app uses this to
    /// gate emulator input.
    #[must_use]
    pub fn wants_keyboard(&self) -> bool {
        self.visible && self.input_ui.is_capturing_keyboard()
    }

    /// v1.0.0 — whether egui currently wants keyboard or pointer input (a menu
    /// is open, a settings text field is focused, ...). The app uses this to
    /// gate emulator key input so clicking a menu / typing in a settings field
    /// does NOT also drive the NES controller. Unlike [`Self::wants_keyboard`]
    /// this is NOT gated on overlay visibility (the always-on shell is
    /// interactive even with the debugger closed).
    #[must_use]
    pub fn wants_egui_input(&self) -> bool {
        let ctx = self.state.egui_ctx();
        ctx.egui_wants_keyboard_input() || ctx.egui_wants_pointer_input()
    }

    /// v1.0.0 (BUG-1) — whether egui has requested a repaint (an animation, a
    /// hover, a click that mutated widget state, ...). The native window-event
    /// pump uses this to issue a `request_redraw` while the emulator is idle or
    /// paused, so the always-on shell keeps repainting on input even when the
    /// self-sustaining produce -> `EmuFrame` -> redraw heartbeat has stopped
    /// (the emu thread parks while paused and never sends `EmuFrame`). Without
    /// this, a "Resume" click in the menu would never be received.
    #[must_use]
    pub fn egui_wants_repaint(&self) -> bool {
        self.state.egui_ctx().has_requested_repaint()
    }

    /// v1.0.0 (BUG-5/6) — whether the shell is currently capturing interaction
    /// (an open menu / popup, or any modal window). The app folds this into its
    /// NES-key gate so dropping a menu and pressing arrows / Z / X / Enter does
    /// not also drive the controller (an open menu has no focused text widget,
    /// so [`Self::wants_egui_input`] alone misses it).
    #[must_use]
    pub fn shell_is_capturing(&self) -> bool {
        egui::Popup::is_any_open(self.state.egui_ctx())
    }

    /// Forward a winit window event into egui. Returns `true` if egui
    /// consumed the event (the main app should still pass keyboard input
    /// to the emulator for keys egui didn't claim).
    pub fn on_window_event(&mut self, window: &Window, event: &WindowEvent) -> bool {
        // v1.0.0 — ALWAYS feed the event into egui so the always-on shell (menu
        // bar / status bar / settings) stays interactive when the debugger
        // overlay is closed. The rebind-capture intercept stays gated on
        // visibility (the rebind modal only lives in the debugger panels).
        let response = self.state.on_window_event(window, event);
        if self.visible {
            // Intercept the next key press for the rebind modal if it's
            // listening — this captures keys that egui would have routed to
            // its own text widgets first.
            self.input_ui.maybe_capture(event);
        }
        response.consumed
    }

    /// Feed a `gilrs` event into the input rebind modal if it's listening
    /// for a gamepad-button capture. Called from the app's gamepad pump
    /// (native only — wasm32 has no gilrs runtime). No-op unless the modal
    /// is waiting on a pad slot.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn maybe_capture_gamepad(&mut self, event: &gilrs::EventType) {
        self.input_ui.maybe_capture_gamepad(event);
    }

    /// Return (and clear) whether the input rebind modal changed the
    /// config since the last poll. The app uses this to rebuild the live
    /// [`crate::input::InputState`] maps so a rebind takes effect
    /// immediately.
    pub fn take_input_bindings_dirty(&mut self) -> bool {
        self.input_ui.take_bindings_dirty()
    }

    /// v1.7.0 — return (and clear) the pending live-apply request from the
    /// settings panel. The app uses this to enable / disable the gfx NTSC
    /// post-pass and arm / free the running `Nes` rewind ring so those
    /// settings take effect immediately instead of after a restart. Other
    /// settings (present mode, sample rate, rewind capacity) are persisted
    /// only and are not represented here.
    pub fn take_settings_apply(&mut self) -> SettingsApply {
        self.settings_ui.take_apply()
    }

    /// v2.8.0 — push (or clear) the present-mode fallback warning shown
    /// beside the settings panel's present-mode selector. Called by the app
    /// once after gfx init when the configured mode was unsupported and the
    /// swapchain silently runs `Fifo` instead.
    pub fn set_present_mode_warning(&mut self, warning: Option<String>) {
        self.settings_ui.set_present_mode_warning(warning);
    }

    /// v1.4.0 Workstream C — push the loaded mapper's expansion-audio chip name
    /// (or `None`) so the Settings Audio tab shows the expansion-channel volume
    /// slider only for boards that actually have on-cart audio.
    pub fn set_expansion_audio_chip(&mut self, chip: Option<&'static str>) {
        self.settings_ui.set_expansion_audio_chip(chip);
    }

    /// v1.7.0 "Forge" H3 — populate the Settings Audio tab's output-device
    /// picker with the enumerated device names. Pushed once by the app at
    /// startup (cpal host enumeration is native-only).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_audio_output_devices(&mut self, names: Vec<String>) {
        self.settings_ui.set_audio_output_devices(names);
    }

    /// v2.8.0 Phase 0 — push the latest performance snapshot (produced /
    /// presented / produce-cost interval stats + audio health) for the
    /// Performance panel. Called from the app's pacer alongside
    /// [`Self::set_fps`].
    pub fn set_perf_view(&mut self, view: crate::perf::PerfView) {
        self.perf_ui.set_view(view);
    }

    /// v2.8.0 — whether the Perf panel's "Logging" checkbox is set. The app
    /// polls this each housekeeping pass to start/stop its `PerfLogger`.
    /// Native-only (file logging needs a filesystem).
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub const fn perf_logging_enabled(&self) -> bool {
        self.perf_ui.logging
    }

    /// v2.8.0 — push the perf-logging status line (destination path or
    /// error) shown under the Perf panel's "Logging" checkbox.
    #[cfg(not(target_arch = "wasm32"))]
    pub fn set_perf_log_note(&mut self, note: Option<String>) {
        self.perf_ui.set_log_note(note);
    }

    /// v1.5.0 "Lens" Workstream H7 — force perf logging on (the
    /// `RUSTYNES_PERF_LOG` env hook for the scripted perf-capture gate).
    #[cfg(not(target_arch = "wasm32"))]
    pub fn force_perf_logging(&mut self) {
        self.perf_ui.force_logging();
    }

    /// v2.3.0 — push the latest netplay status snapshot for the panel + the
    /// top-toolbar HUD to render. Called from the app's pacer alongside
    /// [`Self::set_fps`] / [`Self::set_movie_status`].
    pub fn set_netplay_status(&mut self, status: NetplayStatusView) {
        self.netplay_ui.set_status(status);
    }

    /// v2.3.0 — return (and clear) the pending netplay host/join/leave request
    /// the user clicked in the netplay panel. The app acts on it by driving
    /// its `NetplayUi` (`start_host` / `start_join` / leave).
    pub fn take_netplay_request(&mut self) -> Option<NetplayRequest> {
        self.netplay_ui.take_request()
    }

    /// v2.7.0 — push the latest `RetroAchievements` status snapshot for the
    /// panel + the top-toolbar HUD (points / rich-presence / trackers / unlock
    /// toasts) to render. Called from the app's pacer alongside
    /// [`Self::set_fps`]. Also latches the hardcore-active flag that gates the
    /// Memory panel.
    pub fn set_cheevos_status(&mut self, status: CheevosStatusView) {
        self.hardcore_active = status.enabled && status.hardcore;
        self.cheevos_ui.set_status(status);
    }

    /// v1.5.0 "Lens" Workstream I7 — a compact `RetroAchievements` status line
    /// for the always-on status bar (relocated from the retired `` ` `` overlay
    /// HUD). `None` unless the feature is enabled AND a user is logged in (so the
    /// status bar stays clean for everyone not using RA). Format:
    /// `"RA <unlocked>/<total> (<points> pts)[ HARDCORE]"`. The trailing
    /// `HARDCORE` token is what the status bar keys its gold tint on.
    #[must_use]
    pub fn ra_status_line(&self) -> Option<String> {
        let s = self.cheevos_ui.status();
        if !s.enabled || !s.logged_in {
            return None;
        }
        let mut line = format!("RA {}/{} ({} pts)", s.unlocked, s.total, s.points_unlocked);
        if s.hardcore {
            line.push_str(" HARDCORE");
        }
        Some(line)
    }

    /// v1.7.0 "Forge" beta.5 (#55) — the LONG-FORM `RetroAchievements` status
    /// line for the status bar, carrying the rich read-out the retired `` ` ``
    /// toolbar HUD used to show: the display name + session score, the
    /// unlocked/total count, the hardcore flag, an optional rich-presence
    /// string, and any active leaderboard trackers. The backtick key toggles
    /// the status bar between this and the compact [`Self::ra_status_line`].
    /// `None` under the same conditions (feature off / not logged in). The
    /// trailing `HARDCORE` token is preserved so the status bar keeps keying its
    /// gold tint on it.
    #[must_use]
    pub fn ra_status_long(&self) -> Option<String> {
        use std::fmt::Write as _;
        let s = self.cheevos_ui.status();
        if !s.enabled || !s.logged_in {
            return None;
        }
        let mut line = format!(
            "RA {} - {} pts - {}/{}",
            s.display_name, s.score, s.unlocked, s.total
        );
        if !s.rich_presence.is_empty() {
            let _ = write!(line, " - {}", s.rich_presence);
        }
        for tr in &s.trackers {
            let _ = write!(line, " - LB {tr}");
        }
        if s.hardcore {
            line.push_str(" HARDCORE");
        }
        Some(line)
    }

    /// v1.7.0 "Forge" beta.5 (#55) — a compact netplay status line for the
    /// status bar (relocated from the retired `` ` `` toolbar HUD): the peer
    /// role + smoothed ping + current frame, with rollback / stall annotations.
    /// `None` while no session is active or connecting (so the status bar stays
    /// clean for single-player). Mirrors the format the old HUD used.
    #[must_use]
    pub fn netplay_status_line(&self) -> Option<String> {
        use std::fmt::Write as _;
        let net = self.netplay_ui.status();
        match net.phase {
            NetplayPhaseView::Idle => None,
            NetplayPhaseView::Connecting => Some("NET connecting".to_string()),
            NetplayPhaseView::Error => Some("NET error".to_string()),
            NetplayPhaseView::InGame => {
                let ping = net
                    .ping_ms
                    .map_or_else(|| "-".to_string(), |ms| format!("{ms}ms"));
                let mut s = format!(
                    "NET {} {ping} f{}",
                    if net.is_host { "P1" } else { "P2" },
                    net.current_frame
                );
                if net.rolled_back {
                    let _ = write!(s, " rb{}", net.resimulated_frames);
                }
                if net.stalled {
                    s.push_str(" stall");
                }
                Some(s)
            }
            NetplayPhaseView::Spectating => {
                let mut s = format!("NET spectate f{}", net.current_frame);
                if net.spectator_pending > 0 {
                    let _ = write!(s, " +{}", net.spectator_pending);
                }
                Some(s)
            }
        }
    }

    /// v2.7.0 — return (and clear) the pending RA login/logout/hardcore request
    /// the user triggered in the cheevos panel. The app acts on it by driving
    /// its `RaSession` (the feature-gated `RetroAchievements` session).
    pub fn take_cheevos_request(&mut self) -> Option<CheevosRequest> {
        self.cheevos_ui.take_request()
    }

    /// v1.0.0 — open a tool panel (Cheats / Settings / Netplay / Cheevos /
    /// Perf / Input) by setting its `show_*` flag. The tool panels render as
    /// floating windows whether or not the deep debugger overlay is toggled on
    /// (the menu bar surfaces them directly).
    pub fn open_panel(&mut self, panel: ToolPanel) {
        match panel {
            ToolPanel::Cheats => self.show_cheat = true,
            ToolPanel::Settings => self.show_settings = true,
            ToolPanel::Netplay => self.show_netplay = true,
            ToolPanel::Cheevos => self.show_cheevos = true,
            ToolPanel::Perf => self.show_perf = true,
            ToolPanel::Input => self.show_input = true,
            ToolPanel::GameDb => self.show_game_db = true,
            ToolPanel::InputDisplay => self.show_input_display = true,
            ToolPanel::Replay => self.show_replay = true,
            ToolPanel::BasicBot => self.show_basic_bot = true,
            ToolPanel::TasStudio => self.show_tas = true,
            ToolPanel::HdPixelInspector => {
                #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
                {
                    self.show_hd_pixel = true;
                }
            }
        }
    }

    /// Push the live "Input Display" snapshot (built by the app each frame from
    /// its host-side input state: the standard pads + the active expansion
    /// device). Frontend-only; no core touch (v1.7.0 "Forge" beta.5, #51; née
    /// `set_input_miniatures`).
    pub fn set_input_display(&mut self, snap: MiniaturesSnapshot) {
        self.input_display = snap;
    }

    /// v1.5.0 "Lens" Workstream I10 — open the in-app Documentation browser
    /// (Help -> Documentation). Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    pub const fn open_documentation(&mut self) {
        self.show_documentation = true;
    }

    /// v1.5.0 I10 — whether the Documentation window is open. The app folds this
    /// into its NES-key gate so typing into the doc search box doesn't drive the
    /// controller. Native-only.
    #[cfg(not(target_arch = "wasm32"))]
    #[must_use]
    pub const fn documentation_open(&self) -> bool {
        self.show_documentation
    }

    /// v1.5.0 A4 — whether the HD-pack pixel inspector window is open. Native +
    /// `hd-pack` only.
    #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
    #[must_use]
    pub const fn hd_pixel_open(&self) -> bool {
        self.show_hd_pixel
    }

    /// v1.5.0 A4 — clone the HD-pack pixel-inspector state out so the app can
    /// render the panel in its egui `extra` closure (the panel needs the
    /// compositor + per-frame snapshots the app owns, so its render lives in
    /// `app.rs`, borrow-disjoint from the debugger's own `render_shell` pass).
    #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
    #[must_use]
    pub fn hd_pixel_state(&self) -> hd_pixel_panel::HdPixelPanelState {
        self.hd_pixel_ui.clone()
    }

    /// v1.5.0 A4 — write the HD-pack pixel-inspector state + open flag back after
    /// the app rendered the panel against a clone.
    #[cfg(all(not(target_arch = "wasm32"), feature = "hd-pack"))]
    pub fn set_hd_pixel_state(&mut self, state: hd_pixel_panel::HdPixelPanelState, open: bool) {
        self.hd_pixel_ui = state;
        self.show_hd_pixel = open;
    }

    /// v1.0.0 — open a chip-inspection panel (CPU / PPU / OAM / APU / Memory /
    /// Mapper) AND make the deep overlay visible (the chip panels only render
    /// when the overlay is visible, since they need `&mut Nes`).
    pub fn open_chip_panel(&mut self, panel: ChipPanel) {
        self.visible = true;
        match panel {
            ChipPanel::Cpu => self.show_cpu = true,
            ChipPanel::Ppu => self.show_ppu = true,
            ChipPanel::Oam => self.show_oam = true,
            ChipPanel::Apu => self.show_apu = true,
            ChipPanel::Memory => self.show_memory = true,
            ChipPanel::MemoryCompare => self.show_memory_compare = true,
            ChipPanel::Mapper => self.show_mapper = true,
            ChipPanel::Trace => self.show_trace = true,
            ChipPanel::Watch => self.show_watch = true,
            ChipPanel::Events => self.show_events = true,
            ChipPanel::Nsf => self.show_nsf = true,
            ChipPanel::Script => self.show_script = true,
            #[cfg(not(target_arch = "wasm32"))]
            ChipPanel::HeaderEditor => self.show_header_editor = true,
        }
    }

    /// Populate the NSF player panel's metadata (called when an NSF is loaded).
    pub fn set_nsf_metadata(&mut self, title: String, artist: String, copyright: String) {
        self.nsf_ui.set_metadata(title, artist, copyright);
    }

    /// Mutable access to the Lua console state (the `App` feeds it log/status
    /// and polls its action each pump).
    pub fn script_panel(&mut self) -> &mut script_panel::ScriptPanelState {
        &mut self.script_ui
    }

    /// v1.6.0 "Studio" Workstream C — drive the per-frame observational debug
    /// pumps: the Watch panel's breakpoint / watchpoint / trace replay (C1/C4)
    /// and the Memory hex editor's access-type heatmap (C2). Called by `App`
    /// after a frame is produced, under the emu lock, exactly like the Lua
    /// engine's `on_frame`. Purely observational (it only reads `nes`), so
    /// determinism is unaffected.
    pub fn pump_watchpoints(&mut self, nes: &mut Nes) {
        // Refresh the hex-editor heatmap from THIS frame's access log first
        // (before `watch_ui.pump` re-arms the flag for the next frame).
        self.memory_ui.refresh_heatmap(nes);
        // v1.7.0 "Forge" Workstream C — fold this frame's exec / access /
        // interrupt logs into the call-stack tracker (C1) and the per-address
        // access counter (C2) BEFORE `watch_ui.pump` re-arms the logs for the
        // next frame. Purely observational (they only read `nes`).
        self.callstack.replay_frame(nes);
        self.access_counter.replay_frame(nes);
        // A satisfied step request pauses emulation (handled by `App`); the
        // pause edge is taken there via `take_step_satisfied`.
        self.watch_ui.pump(nes);
        // `watch_ui.pump` arms only what the watch tools need; OR back in the
        // logs the other observational consumers want so none disarms another.
        // (Each `set_*_logging(true)` is idempotent; only an unconditional
        // `set_*_logging(false)` would clobber a peer, and we never call that.)
        if self.memory_ui.wants_access_log() || self.access_counter.wants_access_log() {
            nes.set_access_logging(true);
        }
        let panel_open = self.show_cpu;
        if self.callstack.wants_exec_log(panel_open) || self.access_counter.wants_exec_log() {
            nes.set_exec_logging(true);
        }
        if self.callstack.wants_interrupt_log(panel_open) {
            nes.set_interrupt_logging(true);
        }
    }

    /// v1.7.0 "Forge" Workstream C (C1) — whether a debugger step request
    /// (step-over / step-out / run-to-NMI/IRQ) is in flight. While `true` the
    /// app keeps the emulator running frame-by-frame until it is satisfied.
    #[must_use]
    pub const fn step_pending(&self) -> bool {
        self.callstack.step_pending()
    }

    /// v1.7.0 "Forge" Workstream C (C1) — take the "step request satisfied this
    /// frame" edge (resets it). The app pauses emulation when this is `true`.
    pub fn take_step_satisfied(&mut self) -> bool {
        self.callstack.take_satisfied()
    }

    /// v1.7.0 "Forge" Workstream C (C1) — queue a "step scanline" / "step frame"
    /// verb that the app drives by advancing exactly one scanline / frame.
    /// (Step-over / step-out / run-to-NMI/IRQ are queued from the CPU panel's
    /// Call Stack section.)
    pub fn request_step(&mut self, req: callstack::StepRequest) {
        self.callstack.request_step(req);
    }

    /// v1.7.0 "Forge" Workstream C (C1/C2) — drop the call stack + zero the
    /// access counters (on reset / power-cycle / save-state load, where the
    /// reconstructed state would be stale).
    pub fn reset_debug_telemetry(&mut self) {
        self.callstack.clear();
        self.access_counter.reset();
    }

    /// v1.7.0 "Forge" Workstream C (C3) — load a ca65/cc65 `.dbg` file's `text`
    /// into the source-line map, recording a status line. The `name` is for the
    /// status message only. Display-only; never touches the core.
    pub fn load_source_map(&mut self, name: &str, text: &str) {
        let mapped = self.source_map.load_dbg(text);
        self.source_map_status = Some(format!("{name}: {mapped} addresses mapped"));
    }

    /// v1.7.0 "Forge" Workstream C (C3) — drop the loaded `.dbg` source map.
    pub fn clear_source_map(&mut self) {
        self.source_map.clear();
        self.source_map_status = Some("source map cleared".to_owned());
    }

    /// v1.0.0 — force the deep overlay visible (used when opening a chip panel
    /// from the menu so its window actually renders).
    pub const fn force_visible(&mut self) {
        self.visible = true;
    }

    /// v1.7.1 — `true` while ANY chip-inspection panel (the windows that need a
    /// live `&mut Nes` and only render when the overlay is visible) is open.
    ///
    /// This is the predicate the overlay's visibility tracks: the deep overlay
    /// is "visible" exactly when at least one of these panels is open. With the
    /// `~`/backtick toggle and the standalone debugger toolbar both removed
    /// (v1.7.0 "Forge" beta.5, #55), opening a chip panel is the only thing that
    /// shows the overlay, so closing the last one must hide it again — otherwise
    /// `self.visible` would latch on forever and permanently engage the heavier
    /// lock-holding render branch (see `recompute_visible`).
    ///
    /// EVERY chip `show_*` flag drawn by `chip_panels` must be listed
    /// here; add a new one's flag when you add a new chip panel.
    #[must_use]
    pub const fn any_chip_panel_open(&self) -> bool {
        #[cfg(not(target_arch = "wasm32"))]
        let header_editor = self.show_header_editor;
        #[cfg(target_arch = "wasm32")]
        let header_editor = false;
        chip_panels_open(
            self.show_cpu,
            self.show_ppu,
            self.show_oam,
            self.show_apu,
            self.show_memory,
            self.show_memory_compare,
            self.show_mapper,
            self.show_trace,
            self.show_watch,
            self.show_events,
            self.show_nsf,
            self.show_script,
            header_editor,
        )
    }

    /// v1.7.1 — re-derive overlay visibility from the live chip-panel state so it
    /// accurately reflects "is any chip panel currently open." Called once per
    /// frame at the top of [`Self::render`] / [`Self::render_shell`], BEFORE the
    /// egui pass that may toggle the `show_*` flags. A panel closed via its
    /// window X (which clears its `show_*` flag during the egui pass) therefore
    /// drops `visible` back to `false` on the next frame, releasing the
    /// lock-holding render branch in `app.rs` (`dbg_visible` / `needs_nes`).
    const fn recompute_visible(&mut self) {
        self.visible = self.any_chip_panel_open();
    }

    /// v1.0.0 — whether any tool panel that needs `&mut Nes` is currently open.
    /// The render path uses this to take the locked branch (which passes a real
    /// `nes` to the egui pass) even when the deep overlay is off.
    ///
    /// **v1.5.0 "Lens" Workstream I6** — this is the single predicate that
    /// gates the locked branch, so EVERY `nes`-reading tool panel in
    /// `tool_panels` must be listed here or it silently fails to open
    /// standalone (it would only render while another `nes`-reading panel
    /// happened to be open, then vanish when that one closed). Today the
    /// `nes`-reading tool panels are **Cheats** (`show_cheat`) and the
    /// **ROM Database** editor (`show_game_db`). If you add another panel that
    /// takes `&mut Nes` in `tool_panels`, add its `show_*` flag here too.
    #[must_use]
    pub const fn any_nes_tool_open(&self) -> bool {
        self.show_cheat || self.show_game_db
    }

    /// Build the egui UI for this frame (the deep-overlay path: chip panels +
    /// tool panels, all with a live `nes`). Used by [`Self::render`] and by
    /// [`Self::render_shell`] when the overlay is visible.
    fn ui(&mut self, ui: &egui::Ui, nes: &mut Nes, config: &mut Config) {
        self.chip_panels(ui, nes);
        self.tool_panels(ui.ctx(), Some(nes), config);
    }

    /// v1.0.0 — the chip-inspection UI: the CPU / PPU / OAM / APU / Memory /
    /// Mapper windows. These all read `&mut Nes` and only render when the deep
    /// overlay is visible. v1.7.0 "Forge" beta.5 (#55) removed the toolbar HUD
    /// that this used to draw first.
    fn chip_panels(&mut self, root_ui: &egui::Ui, nes: &mut Nes) {
        // v1.7.0 "Forge" beta.5 (#55) — `root_ui` is now read-only here: with the
        // `debugger_top` toolbar panel removed, the chip windows all render via
        // `ctx` (floating windows), so only the context handle is needed.
        let ctx = root_ui.ctx().clone();
        let ctx = &ctx;
        // v1.7.0 "Forge" beta.5 (#55) — the `debugger_top` toolbar HUD was
        // removed: every panel now opens from the always-on menu bar, and the
        // live read-outs it carried (frame/cycle, fps, movie/disk/netplay
        // status, and the RetroAchievements line) are all surfaced in the
        // bottom status bar instead. The freed backtick (`` ` ``) key now
        // toggles the status-bar RA display between its compact and long-form
        // variants (see `App`'s `SysAction::ToggleDebug` + `UiShell::ra_detail`).
        //
        // What follows is the chip-inspector window dispatch (each renders only
        // while its `show_*` flag is set + the overlay is visible).
        // v1.3.0 Workstream C — the per-panel toggle checkboxes had already been
        // removed; opening every panel from the menu bar (Debug menu for chip
        // inspectors, Tools menu for Cheats / Netplay / Perf / ROM Database /
        // ...) is now the only path; the read-outs moved to the status bar.

        if self.show_cpu {
            // v1.7.0 "Forge" Workstream C — the CPU panel also renders the Call
            // Stack section (C1) + source-line annotations (C3); a clicked step
            // verb is queued on the tracker.
            let step = cpu_panel::show(
                ctx,
                &mut self.show_cpu,
                &mut self.cpu_ui,
                nes,
                &self.symbols,
                self.symbols_status.as_deref(),
                &self.callstack,
                &self.source_map,
                self.source_map_status.as_deref(),
            );
            if let Some(req) = step {
                self.callstack.request_step(req);
            }
        }
        if self.show_ppu {
            ppu_panel::show(ctx, &mut self.show_ppu, &mut self.ppu_ui, nes);
        }
        if self.show_oam {
            oam_panel::show(ctx, &mut self.show_oam, &mut self.oam_ui, nes);
        }
        // v1.7.0 "Forge" Workstream A2 — Cartridge Info / header editor. Edits a
        // ROM file on disk (not `nes`), so it needs no emulator borrow.
        #[cfg(not(target_arch = "wasm32"))]
        if self.show_header_editor {
            header_editor::show(
                ctx,
                &mut self.show_header_editor,
                &mut self.header_editor_ui,
            );
        }
        if self.show_apu {
            apu_panel::show(ctx, &mut self.show_apu, &mut self.apu_ui, nes);
        }
        if self.show_memory {
            // v2.7.0 — the Memory panel is a RAM hex viewer (a potential
            // RAM-watch cheat surface), so it is disabled in hardcore mode.
            if self.hardcore_active {
                egui::Window::new("Memory")
                    .open(&mut self.show_memory)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(0xF0, 0xC0, 0x40),
                            "Disabled in hardcore mode.",
                        );
                        ui.label(
                            egui::RichText::new(
                                "The memory viewer is unavailable while \
                                 RetroAchievements hardcore mode is active.",
                            )
                            .weak(),
                        );
                    });
            } else {
                // v1.7.0 "Forge" Workstream C (C2) — the Memory panel also
                // renders the per-address access-counter heatmap section,
                // driven by the overlay-owned counter.
                memory_panel::show(
                    ctx,
                    &mut self.show_memory,
                    &mut self.memory_ui,
                    nes,
                    &mut self.access_counter,
                );
            }
        }
        if self.show_memory_compare {
            // A cheat-hunting (RAM-search) tool — disabled in hardcore mode for
            // the same reason as the Memory viewer + cheat panel.
            if self.hardcore_active {
                egui::Window::new("Memory Compare")
                    .open(&mut self.show_memory_compare)
                    .resizable(false)
                    .show(ctx, |ui| {
                        ui.colored_label(
                            egui::Color32::from_rgb(0xF0, 0xC0, 0x40),
                            "Disabled in hardcore mode.",
                        );
                        ui.label(
                            egui::RichText::new(
                                "Memory search is unavailable while RetroAchievements \
                                 hardcore mode is active.",
                            )
                            .weak(),
                        );
                    });
            } else {
                memory_compare_panel::show(
                    ctx,
                    &mut self.show_memory_compare,
                    &mut self.memory_compare_ui,
                    nes,
                );
            }
        }
        if self.show_trace {
            trace_panel::show(
                ctx,
                &mut self.show_trace,
                &mut self.trace_ui,
                nes,
                &self.symbols,
            );
        }
        if self.show_watch {
            watch_panel::show(
                ctx,
                &mut self.show_watch,
                &mut self.watch_ui,
                nes,
                &self.symbols,
            );
        }
        if self.show_events {
            event_panel::show(ctx, &mut self.show_events, &mut self.event_ui, nes);
        }
        if self.show_nsf {
            nsf_panel::show(ctx, &mut self.show_nsf, &mut self.nsf_ui, nes);
        }
        if self.show_script {
            script_panel::show(ctx, &mut self.show_script, &mut self.script_ui, nes);
        }
        if self.show_mapper {
            mapper_panel::show(ctx, &mut self.show_mapper, &mut self.mapper_ui, nes);
        }
    }

    /// v1.0.0 — the tool panels (Cheats / Settings / Netplay / Cheevos / Perf /
    /// Input) plus the RA unlock-toast stack. These render as floating windows
    /// whenever their `show_*` flag is set, REGARDLESS of whether the deep
    /// overlay is visible, so the menu bar can surface them directly. Panels
    /// that read `nes` (Cheats) no-op when `nes` is `None`.
    fn tool_panels(&mut self, ctx: &egui::Context, mut nes: Option<&mut Nes>, config: &mut Config) {
        // v1.8.9 — BasicBot input-search control panel. Renders with the optional
        // `nes` (reborrowed so the rest of the panels still get it); the search is
        // disabled when no ROM is loaded.
        if self.show_basic_bot {
            basic_bot_panel::show(
                ctx,
                &mut self.show_basic_bot,
                &mut self.basic_bot_ui,
                nes.as_deref_mut(),
            );
        }
        // v2.7.0 — transient RetroAchievements unlock / event toasts, drawn as
        // a floating top-right stack so they're visible without the toolbar
        // open. The app expires them after a few seconds. v2.7.1 — an
        // achievement-unlock toast also shows its badge image (left of the text)
        // once the badge cache has fetched + decoded it.
        {
            let toasts = self.cheevos_ui.status().toasts.clone();
            if !toasts.is_empty() {
                // v2.7.1 — lazily create + poll the badge cache so unlock-toast
                // badges resolve even when the achievements panel is closed.
                #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
                let badges = {
                    let b = self
                        .badge_cache
                        .get_or_insert_with(badge_cache::BadgeCache::new);
                    b.poll(ctx);
                    for (.., url) in &toasts {
                        b.request(url);
                    }
                    &*b
                };
                egui::Area::new(egui::Id::new("cheevos_toasts"))
                    .anchor(egui::Align2::RIGHT_TOP, [-12.0, 40.0])
                    .show(ctx, |ui| {
                        for (title, detail, is_error, badge_url) in &toasts {
                            // `badge_url` is only consumed by the feature-on
                            // badge draw below; discard it on the default build.
                            #[cfg(not(all(
                                not(target_arch = "wasm32"),
                                feature = "retroachievements"
                            )))]
                            let _ = badge_url;
                            let bg = if *is_error {
                                egui::Color32::from_rgb(0x60, 0x20, 0x20)
                            } else {
                                egui::Color32::from_rgb(0x20, 0x40, 0x20)
                            };
                            egui::Frame::new()
                                .fill(bg)
                                .inner_margin(egui::Margin::same(6))
                                .corner_radius(4)
                                .show(ui, |ui| {
                                    ui.horizontal(|ui| {
                                        #[cfg(all(
                                            not(target_arch = "wasm32"),
                                            feature = "retroachievements"
                                        ))]
                                        if let Some(tex) = badges.texture(badge_url) {
                                            let s = badge_cache::BADGE_SIZE;
                                            ui.add(
                                                egui::Image::new((tex.id(), egui::vec2(s, s)))
                                                    .maintain_aspect_ratio(true),
                                            );
                                        }
                                        ui.vertical(|ui| {
                                            ui.label(egui::RichText::new(title).strong());
                                            if !detail.is_empty() {
                                                ui.label(detail);
                                            }
                                        });
                                    });
                                });
                            ui.add_space(4.0);
                        }
                    });
            }
        }

        // v1.7.0 "Forge" H2 — RetroAchievements HUD completion. These surface
        // the leaderboard-scoreboard / challenge / progress data the session
        // already decodes. They read the inert pushed status snapshot (no `nes`
        // and no feature gate), so they render in every build configuration;
        // when the `retroachievements` feature is off the vectors are empty.
        {
            let status = self.cheevos_ui.status();

            // Active challenge indicators + the transient progress indicator,
            // drawn as a compact bottom-right stack (above the status bar).
            let challenges = status.challenges.clone();
            let progress = status.progress.clone();
            if !challenges.is_empty() || progress.is_some() {
                egui::Area::new(egui::Id::new("cheevos_indicators"))
                    .anchor(egui::Align2::RIGHT_BOTTOM, [-12.0, -48.0])
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgba_unmultiplied(
                                0x20, 0x20, 0x30, 0xC0,
                            ))
                            .inner_margin(egui::Margin::same(6))
                            .corner_radius(4)
                            .show(ui, |ui| {
                                if let Some(measured) = &progress {
                                    ui.label(
                                        egui::RichText::new(format!("Progress: {measured}"))
                                            .strong(),
                                    );
                                }
                                if !challenges.is_empty() {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Challenge x{}",
                                            challenges.len()
                                        ))
                                        .color(egui::Color32::from_rgb(0xF0, 0xC8, 0x60)),
                                    );
                                }
                            });
                    });
            }

            // Leaderboard-scoreboard popups (shown for a few seconds after a
            // submission): the player's new rank "#N of M" + the top entries.
            let scoreboards = status.scoreboards.clone();
            if !scoreboards.is_empty() {
                egui::Area::new(egui::Id::new("cheevos_scoreboards"))
                    .anchor(egui::Align2::CENTER_TOP, [0.0, 56.0])
                    .show(ctx, |ui| {
                        for sb in &scoreboards {
                            egui::Frame::new()
                                .fill(egui::Color32::from_rgba_unmultiplied(
                                    0x18, 0x18, 0x28, 0xE0,
                                ))
                                .inner_margin(egui::Margin::same(8))
                                .corner_radius(4)
                                .show(ui, |ui| {
                                    ui.label(
                                        egui::RichText::new(format!(
                                            "Leaderboard: #{} of {}",
                                            sb.new_rank, sb.num_entries
                                        ))
                                        .strong()
                                        .color(egui::Color32::from_rgb(0xB4, 0xA0, 0xDC)),
                                    );
                                    if !sb.submitted_score.is_empty() {
                                        ui.label(format!(
                                            "Your score: {}   (best: {})",
                                            sb.submitted_score, sb.best_score
                                        ));
                                    }
                                    for (rank, user, score) in &sb.top_entries {
                                        ui.label(
                                            egui::RichText::new(format!(
                                                "  {rank}. {user} - {score}"
                                            ))
                                            .weak(),
                                        );
                                    }
                                });
                            ui.add_space(4.0);
                        }
                    });
            }

            // v1.8.9 H2 — active leaderboard trackers (the live value of each
            // in-progress leaderboard, e.g. a speedrun timer), drawn bottom-left so
            // they don't collide with the challenge/progress stack (bottom-right) or
            // the scoreboard popups (top). Captured already (and mirrored into the
            // status bar); this surfaces them on-screen the way RA's HUD does.
            if !status.trackers.is_empty() {
                egui::Area::new(egui::Id::new("cheevos_trackers"))
                    .anchor(egui::Align2::LEFT_BOTTOM, [12.0, -48.0])
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(egui::Color32::from_rgba_unmultiplied(
                                0x20, 0x20, 0x30, 0xC0,
                            ))
                            .inner_margin(egui::Margin::same(6))
                            .corner_radius(4)
                            .show(ui, |ui| {
                                for tr in &status.trackers {
                                    ui.label(
                                        egui::RichText::new(format!("\u{1F3C1} {tr}"))
                                            .color(egui::Color32::from_rgb(0x9C, 0xD0, 0xF0)),
                                    );
                                }
                            });
                    });
            }
        }

        if self.show_input {
            input_rebind_panel::show(ctx, &mut self.show_input, &mut self.input_ui, config);
        }
        if self.show_input_display {
            // v1.7.0 "Forge" beta.5 (#51) — the consolidated "Input Display"
            // panel (standard pads + every expansion peripheral).
            input_miniatures_panel::show(
                ctx,
                &mut self.show_input_display,
                &mut self.input_display_ui,
                &self.input_display,
            );
        }
        if self.show_replay {
            // v1.5.0 "Lens" C2 — control + read-out surface; reads the pushed
            // status snapshot, not `nes`, so it renders in the always-on path.
            replay_panel::show(ctx, &mut self.show_replay, &mut self.replay_ui);
        }
        if self.show_tas {
            // v1.6.0 "Studio" A2 — renders the editor model read-only and queues
            // edits/seeks as `TasRequest`s the app applies under the emu lock.
            tastudio_panel::show(
                ctx,
                &mut self.show_tas,
                &mut self.tas_ui,
                self.tas_editor.as_ref(),
            );
        }
        if self.show_cheat {
            // The Cheats panel reads `nes`; with no ROM loaded there is nothing
            // to edit, so no-op (the window simply doesn't open).
            if let Some(nes) = nes.as_deref_mut() {
                #[cfg(not(target_arch = "wasm32"))]
                cheat_panel::show(
                    ctx,
                    &mut self.show_cheat,
                    &mut self.cheat_ui,
                    nes,
                    self.cheat_persist.as_ref(),
                    self.rom_crc,
                );
                #[cfg(target_arch = "wasm32")]
                cheat_panel::show(
                    ctx,
                    &mut self.show_cheat,
                    &mut self.cheat_ui,
                    nes,
                    self.rom_crc,
                );
            }
        }
        if self.show_game_db
            && let Some(nes) = nes
        {
            game_db_panel::show(
                ctx,
                &mut self.show_game_db,
                &mut self.game_db_ui,
                nes,
                self.rom_crc,
            );
        }
        if self.show_settings {
            settings_panel::show(ctx, &mut self.show_settings, &mut self.settings_ui, config);
        }
        if self.show_netplay {
            netplay_panel::show(ctx, &mut self.show_netplay, &mut self.netplay_ui, config);
        }
        if self.show_perf {
            perf_panel::show(ctx, &mut self.show_perf, &mut self.perf_ui);
        }
        // v1.5.0 "Lens" Workstream I10 — the in-app Documentation browser
        // (native-only; reuses the `cli::HELP_TOPICS` registry). It reads no
        // `nes`, so it renders in the always-on path like the other doc windows.
        #[cfg(not(target_arch = "wasm32"))]
        if self.show_documentation {
            doc_panel::show(ctx, &mut self.show_documentation, &mut self.doc_ui);
        }
        if self.show_cheevos {
            #[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
            {
                // v2.7.1 — lazily create + poll the badge-image cache, then let
                // the panel request/draw the achievement badge icons.
                let badges = self
                    .badge_cache
                    .get_or_insert_with(badge_cache::BadgeCache::new);
                badges.poll(ctx);
                cheevos_panel::show(
                    ctx,
                    &mut self.show_cheevos,
                    &mut self.cheevos_ui,
                    config,
                    badges,
                );
            }
            #[cfg(not(all(not(target_arch = "wasm32"), feature = "retroachievements")))]
            cheevos_panel::show(ctx, &mut self.show_cheevos, &mut self.cheevos_ui, config);
        }
    }

    /// Render the overlay into `view` on top of whatever was already
    /// drawn. Skips work entirely if not visible.
    ///
    /// `extra_ui` is an additional UI callback run inside the SAME egui frame
    /// (used on wasm to draw the browser-netplay lobby, which the `App` owns);
    /// native passes a no-op.
    #[allow(clippy::too_many_arguments)]
    pub fn render<F: FnOnce(&egui::Context, &mut Config)>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &Arc<Window>,
        view: &wgpu::TextureView,
        surface_size: (u32, u32),
        nes: &mut Nes,
        config: &mut Config,
        extra_ui: F,
    ) {
        // v1.7.1 — re-derive visibility from the live chip-panel state so a
        // closed-last-panel drops the overlay (see `recompute_visible`).
        self.recompute_visible();
        if !self.visible {
            return;
        }
        let raw_input = self.state.take_egui_input(window);
        let ctx = self.state.egui_ctx().clone();
        // egui 0.34 deprecated the `|ctx|` form of `Context::run` (and
        // context-level `Panel::show`) in favour of `run_ui`, which hands the
        // body a root `&mut Ui` to host the panels via `show_inside`. The
        // floating windows reached through `ui.ctx()` are unchanged, so the
        // single-pass behaviour is identical.
        // `run_ui` takes an `FnMut`; `extra_ui` is `FnOnce`. The closure runs
        // exactly once, so move it through an `Option::take`.
        let mut extra_ui = Some(extra_ui);
        let output = ctx.run_ui(raw_input, |ui| {
            self.ui(ui, nes, config);
            if let Some(extra_ui) = extra_ui.take() {
                extra_ui(ui.ctx(), config);
            }
        });
        self.state
            .handle_platform_output(window, output.platform_output);

        let pixels_per_point = ctx.pixels_per_point();
        let clipped = ctx.tessellate(output.shapes, pixels_per_point);
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [surface_size.0.max(1), surface_size.1.max(1)],
            pixels_per_point,
        };
        for (id, image) in output.textures_delta.set {
            self.renderer.update_texture(device, queue, id, &image);
        }
        self.renderer
            .update_buffers(device, queue, encoder, &clipped, &screen_desc);
        {
            let mut rp = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut rp, &clipped, &screen_desc);
        }
        for id in output.textures_delta.free {
            self.renderer.free_texture(&id);
        }
    }

    /// v1.0.0 — render the ALWAYS-ON desktop UX shell (menu bar, status bar,
    /// settings window, welcome/about/shortcuts modals) every frame, and — only
    /// when the debugger overlay is visible AND a ROM is loaded — the existing
    /// debugger panels on top.
    ///
    /// This runs a single `ctx.run` closure that (1) applies the configured
    /// theme, (2) builds the shell UI (which never touches `nes`), (3) draws the
    /// optional wasm-netplay lobby (`extra_ui`), and (4) conditionally builds
    /// the debugger panels. The shell's chosen [`MenuAction`](crate::ui_shell::MenuAction)
    /// (if any) is returned via [`ShellOutput`] for
    /// the `App` to dispatch AFTER the egui
    /// pass — none of the menu callbacks need `&mut App` inside the closure.
    ///
    /// `nes` is `Option` so the shell renders even before a ROM is loaded; the
    /// debugger panels are skipped while it is `None`.
    #[allow(clippy::too_many_arguments)]
    pub fn render_shell<F: FnOnce(&egui::Context, &mut Config)>(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        encoder: &mut wgpu::CommandEncoder,
        window: &Arc<Window>,
        view: &wgpu::TextureView,
        surface_size: (u32, u32),
        nes: Option<&mut Nes>,
        config: &mut Config,
        shell: &mut UiShell,
        shell_frame: &ShellFrame<'_>,
        extra_ui: F,
    ) -> ShellOutput {
        // v1.7.1 — re-derive visibility from the live chip-panel state BEFORE
        // the egui pass so closing the last chip panel hides the overlay again
        // (see `recompute_visible`); otherwise `visible` would latch on forever
        // and permanently engage the heavier lock-holding render branch.
        self.recompute_visible();
        let raw_input = self.state.take_egui_input(window);
        let ctx = self.state.egui_ctx().clone();
        let visible = self.visible;
        let mut nes = nes;
        let mut extra_ui = Some(extra_ui);
        let mut shell_out = ShellOutput::default();
        // (audit m6) Decide whether the theme changed BEFORE the closure (which
        // mutably borrows `self` via the panels), then write the cache back
        // after. `apply_theme` runs inside the closure only when needed.
        let theme_changed = self.last_theme != Some(config.ui.theme);
        let theme_now = config.ui.theme;
        // egui 0.34 deprecated the `|ctx|` form of `Context::run` (and
        // context-level `Panel::show`) in favour of `run_ui`, which hands the
        // body a root `&mut Ui` to host the top/bottom panels via `show_inside`.
        // The shell's floating windows + the debugger panels reached through
        // `ui.ctx()` are unchanged, so the single-pass behaviour is identical.
        let output = ctx.run_ui(raw_input, |ui| {
            // The owned `&Context` for the context-level windows/areas. Cloning
            // the handle is cheap and avoids borrowing `ui` for the whole body.
            let ctx_owned = ui.ctx().clone();
            let ctx = &ctx_owned;
            // (1) Theme first so the whole frame (shell + debugger) is themed.
            // Only rebuild + apply `Visuals` when the theme actually changed (or
            // on the first frame), not every frame.
            if theme_changed {
                crate::ui_shell::apply_theme(ctx, theme_now);
            }
            // (1b) v1.5.0 accessibility — UI zoom. `set_zoom_factor` is a no-op
            // when the value is unchanged, so calling it every frame is cheap
            // and keeps the egui shell scaled to `config.ui.zoom_factor`. The
            // emulated NES image is a raw framebuffer blit, not egui content,
            // so it is unaffected (gameplay/determinism untouched).
            ctx.set_zoom_factor(config.ui.clamped_zoom_factor());
            // (1c) v1.7.0 "Forge" Workstream H5 — i18n. Publish the configured
            // UI locale to the process-global so every `tr(..)` call this frame
            // resolves against it. A relaxed atomic store; egui re-renders each
            // frame, so a language change in Settings takes effect next frame
            // with no explicit invalidation. English (the default) reproduces
            // the verbatim pre-i18n labels.
            crate::i18n::set_locale(config.ui.locale);
            // (2) The always-on shell. Its settings/input tab bodies reuse the
            // existing debugger widgets so their live-apply plumbing is intact.
            let settings_ui = &mut self.settings_ui;
            let input_ui = &mut self.input_ui;
            shell_out = shell.build(
                ui,
                config,
                shell_frame,
                |ui, cfg, tab| {
                    // v1.0.0 settings split — route each Settings tab to its own
                    // section so a tab shows only its controls (the live-apply
                    // plumbing on `settings_ui` is shared across all three).
                    use crate::ui_shell::SettingsTab;
                    match tab {
                        SettingsTab::Video => settings_panel::video_section(ui, settings_ui, cfg),
                        // v1.3.0 — the shader stack is its own tab now.
                        SettingsTab::Shaders => {
                            settings_panel::shader_stack_section(ui, settings_ui, cfg);
                        }
                        SettingsTab::Audio => settings_panel::audio_section(ui, settings_ui, cfg),
                        // The Input tab is handled by `input_body`; treat any
                        // other tab as Emulation for exhaustiveness.
                        SettingsTab::Emulation | SettingsTab::Input => {
                            settings_panel::advanced_section(ui, settings_ui, cfg);
                        }
                    }
                },
                |ui, cfg| input_rebind_panel::body(ui, input_ui, cfg),
            );
            // (3) The wasm-netplay lobby (native passes a no-op closure).
            if let Some(extra_ui) = extra_ui.take() {
                extra_ui(ctx, config);
            }
            // (4) v1.0.0 — the tool panels (Cheats / Settings / Netplay /
            // Cheevos / Perf / Input) + RA toasts ALWAYS render when their
            // `show_*` flag is set, so the menu bar can surface them with the
            // deep overlay off. The chip panels (CPU / PPU / ...) + the debugger
            // toolbar HUD render only when the overlay is visible (they need a
            // live `&mut Nes`).
            self.tool_panels(ctx, nes.as_deref_mut(), config);
            if visible && let Some(nes) = nes.as_deref_mut() {
                self.chip_panels(ui, nes);
            }
            // (5) v1.0.0 polish — a pause-screen dimming overlay: a
            // semi-transparent dark rect (~40% black) over the emulated
            // viewport plus a large centred "PAUSED" label, whenever emulation
            // is paused AND a ROM is loaded. `available_rect` excludes the
            // already-added menu + status bars, so the dim never covers them;
            // it is painted at `Background` order so modal windows (Settings /
            // About / a tool panel) stay on top and fully readable.
            if shell_frame.paused && shell_frame.rom_loaded {
                let viewport = ctx.content_rect();
                egui::Area::new(egui::Id::new("paused_dim"))
                    .order(egui::Order::Background)
                    .interactable(false)
                    .fixed_pos(viewport.min)
                    .show(ctx, |ui| {
                        ui.painter().rect_filled(
                            viewport,
                            0.0,
                            egui::Color32::from_black_alpha(102), // ~40% black.
                        );
                    });
                egui::Area::new(egui::Id::new("paused_overlay"))
                    .fixed_pos(viewport.center() - egui::vec2(72.0, 28.0))
                    .order(egui::Order::Foreground)
                    .interactable(false)
                    .show(ctx, |ui| {
                        egui::Frame::new()
                            .fill(egui::Color32::from_black_alpha(160))
                            .inner_margin(egui::Margin::symmetric(24, 14))
                            .corner_radius(6)
                            .show(ui, |ui| {
                                ui.label(
                                    egui::RichText::new("PAUSED")
                                        .size(40.0)
                                        .strong()
                                        .color(egui::Color32::WHITE),
                                );
                            });
                    });
            }
        });
        // (audit m6) record the theme we applied so the next frame skips the
        // `set_visuals` rebuild unless it changes again.
        if theme_changed {
            self.last_theme = Some(theme_now);
        }
        self.state
            .handle_platform_output(window, output.platform_output);

        let pixels_per_point = ctx.pixels_per_point();
        let clipped = ctx.tessellate(output.shapes, pixels_per_point);
        let screen_desc = egui_wgpu::ScreenDescriptor {
            size_in_pixels: [surface_size.0.max(1), surface_size.1.max(1)],
            pixels_per_point,
        };
        for (id, image) in output.textures_delta.set {
            self.renderer.update_texture(device, queue, id, &image);
        }
        self.renderer
            .update_buffers(device, queue, encoder, &clipped, &screen_desc);
        {
            let mut rp = encoder
                .begin_render_pass(&wgpu::RenderPassDescriptor {
                    label: Some("egui-shell-pass"),
                    color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                        view,
                        depth_slice: None,
                        resolve_target: None,
                        ops: wgpu::Operations {
                            load: wgpu::LoadOp::Load,
                            store: wgpu::StoreOp::Store,
                        },
                    })],
                    depth_stencil_attachment: None,
                    timestamp_writes: None,
                    occlusion_query_set: None,
                    multiview_mask: None,
                })
                .forget_lifetime();
            self.renderer.render(&mut rp, &clipped, &screen_desc);
        }
        for id in output.textures_delta.free {
            self.renderer.free_texture(&id);
        }
        shell_out
    }
}

#[cfg(test)]
mod tests {
    use super::chip_panels_open;

    /// A minimal mirror of the overlay's chip `show_*` flags + the cached
    /// `visible` field, driven by the SAME [`chip_panels_open`] predicate the
    /// real `DebuggerOverlay::recompute_visible` uses. Lets us exercise the
    /// open -> visible -> close-all -> hidden lifecycle without standing up the
    /// GPU-backed overlay (`DebuggerOverlay::new` needs a window + wgpu device).
    #[derive(Default)]
    struct VisModel {
        cpu: bool,
        ppu: bool,
        memory: bool,
        visible: bool,
    }

    impl VisModel {
        fn any_chip_panel_open(&self) -> bool {
            chip_panels_open(
                self.cpu,
                self.ppu,
                false,
                false,
                self.memory,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
                false,
            )
        }

        // Mirrors `DebuggerOverlay::recompute_visible` (run once per frame).
        fn recompute_visible(&mut self) {
            self.visible = self.any_chip_panel_open();
        }
    }

    #[test]
    fn visibility_tracks_chip_panel_state() {
        let mut m = VisModel::default();
        // No panels open at construction => overlay hidden.
        m.recompute_visible();
        assert!(!m.visible, "no panels open => overlay must be hidden");
        assert!(!m.any_chip_panel_open());

        // Opening a chip panel (e.g. `open_chip_panel(Cpu)`) shows the overlay.
        m.cpu = true;
        m.recompute_visible();
        assert!(m.visible, "an open chip panel => overlay visible");
        assert!(m.any_chip_panel_open());

        // A second panel keeps it visible.
        m.ppu = true;
        m.recompute_visible();
        assert!(m.visible);

        // Closing ONE of two panels keeps the overlay visible.
        m.cpu = false;
        m.recompute_visible();
        assert!(m.visible, "still one panel open => overlay stays visible");
        assert!(m.any_chip_panel_open());

        // Closing the LAST panel hides the overlay again (the regression fix:
        // `visible` must not latch on once any panel was opened).
        m.ppu = false;
        m.recompute_visible();
        assert!(
            !m.visible,
            "all panels closed => overlay must hide (no sticky-true latch)"
        );
        assert!(!m.any_chip_panel_open());
    }
}
