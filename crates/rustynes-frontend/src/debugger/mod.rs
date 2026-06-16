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

use rustynes_core::{Buttons, Nes};
use winit::event::WindowEvent;
use winit::window::Window;

use crate::config::Config;
use crate::movie_ui::{MovieMode, MovieStatus};
use crate::ui_shell::{ShellFrame, ShellOutput, UiShell};

mod apu_panel;
// v2.7.1 — RetroAchievements badge-image cache (native-only, feature-gated).
// The badge worker + texture cache; the default / wasm builds never see it.
#[cfg(all(not(target_arch = "wasm32"), feature = "retroachievements"))]
mod badge_cache;
mod cheat_panel;
mod cheevos_panel;
mod cpu_panel;
mod event_panel;
mod game_db_panel;
mod input_display_panel;
mod input_rebind_panel;
mod mapper_panel;
mod memory_compare_panel;
mod memory_panel;
mod netplay_panel;
mod nsf_panel;
mod oam_panel;
// v2.8.0 Phase 0 — frame-pacing / audio-health instrumentation panel.
mod perf_panel;
mod ppu_panel;
mod script_panel;
mod settings_panel;
mod trace_panel;

pub use cheevos_panel::{CheevosRequest, CheevosStatusView};
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
    /// Live input-display controller HUD (v1.1.0 beta.1, Workstream B).
    InputDisplay,
    /// Per-game ROM-database editor (v1.2.0 Workstream B, B4).
    GameDb,
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
    /// Event viewer (T-110-C3): scanline×dot write-event timeline.
    Events,
    /// NSF music player (T-110-D1): track selector + metadata.
    Nsf,
    /// Lua script console (T-110-E5): load/reload/stop + log.
    Script,
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
    show_events: bool,
    show_nsf: bool,
    show_script: bool,
    show_input: bool,
    show_input_display: bool,
    show_cheat: bool,
    show_settings: bool,
    show_netplay: bool,
    show_cheevos: bool,
    show_perf: bool,
    show_game_db: bool,
    /// CPU panel state (auto-follow PC, address jump, ...).
    cpu_ui: cpu_panel::CpuPanelState,
    /// PPU panel state.
    ppu_ui: ppu_panel::PpuPanelState,
    /// OAM panel state.
    oam_ui: oam_panel::OamPanelState,
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
    /// Event viewer panel state (T-110-C3).
    event_ui: event_panel::EventPanelState,
    /// NSF player panel state (T-110-D1).
    nsf_ui: nsf_panel::NsfPanelState,
    /// Lua script console state (T-110-E5).
    script_ui: script_panel::ScriptPanelState,
    /// Input rebind modal state.
    input_ui: input_rebind_panel::InputPanelState,
    /// Input-display HUD state (v1.1.0 beta.1, Workstream B).
    input_display_ui: input_display_panel::InputDisplayPanelState,
    /// Held buttons per player, pushed each frame via
    /// [`Self::set_input_display`] and drawn by the input-display HUD.
    input_pads: [Buttons; 4],
    /// Number of active players to show in the input-display HUD (2, or 4 with
    /// Four Score).
    input_players: usize,
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
    /// Updated by [`DebuggerOverlay::set_fps`] from the frontend's pacing
    /// loop; rendered in the top toolbar so users can visually confirm the
    /// emulator runs at the target 60.0988 Hz (NTSC) / 50.0070 Hz (PAL).
    fps: f32,
    /// v1.4.0 Sprint 4.2 — current TAS movie record/playback status,
    /// pushed by [`DebuggerOverlay::set_movie_status`] from the pacing
    /// loop and shown read-only in the top toolbar.
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
            show_events: false,
            show_nsf: false,
            show_script: false,
            show_input: false,
            show_input_display: false,
            show_cheat: false,
            show_settings: false,
            show_netplay: false,
            show_cheevos: false,
            show_perf: false,
            show_game_db: false,
            cpu_ui: cpu_panel::CpuPanelState::default(),
            ppu_ui: ppu_panel::PpuPanelState::default(),
            oam_ui: oam_panel::OamPanelState::default(),
            apu_ui: apu_panel::ApuPanelState::default(),
            memory_ui: memory_panel::MemoryPanelState::default(),
            memory_compare_ui: memory_compare_panel::MemoryComparePanelState::default(),
            mapper_ui: mapper_panel::MapperPanelState::default(),
            trace_ui: trace_panel::TracePanelState::default(),
            event_ui: event_panel::EventPanelState,
            nsf_ui: nsf_panel::NsfPanelState::default(),
            script_ui: script_panel::ScriptPanelState::default(),
            input_ui: input_rebind_panel::InputPanelState::default(),
            input_display_ui: input_display_panel::InputDisplayPanelState,
            input_pads: [Buttons::empty(); 4],
            input_players: 2,
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
        }
    }

    /// Toggle overlay visibility.
    pub const fn toggle(&mut self) {
        self.visible = !self.visible;
    }

    /// Update the measured wall-clock FPS shown in the top toolbar. Called
    /// from the frontend's wall-clock pacer on each completed frame.
    pub const fn set_fps(&mut self, fps: f32) {
        self.fps = fps;
    }

    /// v1.1.0 beta.1 (Workstream B) — push the current held-button snapshot for
    /// the input-display HUD. `pads` is per-player (P1..P4); `players` is how
    /// many to show (2, or 4 with Four Score). Mirrors the [`Self::set_fps`]
    /// pull pattern; a no-op for rendering when the HUD is closed.
    pub const fn set_input_display(&mut self, pads: [Buttons; 4], players: usize) {
        self.input_pads = pads;
        self.input_players = players;
    }

    /// Update the TAS movie record/playback status shown in the top
    /// toolbar. Called from the frontend's pacer alongside [`Self::set_fps`].
    pub const fn set_movie_status(&mut self, status: MovieStatus) {
        self.movie = status;
    }

    /// Returns `true` when the overlay is currently visible. The render
    /// path uses this to pick its emu-lock policy (v2.8.0 Phase 5): the
    /// egui pass needs `&mut Nes`, so a visible overlay holds the lock
    /// across the render; hidden renders from the staging copy instead.
    #[must_use]
    pub const fn is_visible(&self) -> bool {
        self.visible
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
    #[must_use]
    pub fn enabled_raw_cheats(&self) -> Vec<crate::cheats::RawCheat> {
        self.cheat_ui.enabled_raw_cheats()
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
            ToolPanel::InputDisplay => self.show_input_display = true,
            ToolPanel::GameDb => self.show_game_db = true,
        }
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
            ChipPanel::Events => self.show_events = true,
            ChipPanel::Nsf => self.show_nsf = true,
            ChipPanel::Script => self.show_script = true,
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

    /// v1.0.0 — force the deep overlay visible (used when opening a chip panel
    /// from the menu so its window actually renders).
    pub const fn force_visible(&mut self) {
        self.visible = true;
    }

    /// v1.0.0 — whether any tool panel that needs `&mut Nes` is currently open.
    /// Today only the Cheats panel reads `nes`; the render path uses this to
    /// take the locked branch (which passes a real `nes`) even when the deep
    /// overlay is off.
    #[must_use]
    pub const fn any_nes_tool_open(&self) -> bool {
        self.show_cheat
    }

    /// Build the egui UI for this frame (the deep-overlay path: toolbar HUD +
    /// chip panels + tool panels, all with a live `nes`). Used by [`Self::render`]
    /// and by [`Self::render_shell`] when the overlay is visible.
    fn ui(&mut self, ui: &mut egui::Ui, nes: &mut Nes, config: &mut Config) {
        self.chip_panels(ui, nes);
        self.tool_panels(ui.ctx(), Some(nes), config);
    }

    /// v1.0.0 — the chip-inspection UI: the debugger toolbar HUD + the
    /// CPU / PPU / OAM / APU / Memory / Mapper windows. These all read `&mut Nes`
    /// and only render when the deep overlay is visible.
    fn chip_panels(&mut self, root_ui: &mut egui::Ui, nes: &mut Nes) {
        let ctx = root_ui.ctx().clone();
        let ctx = &ctx;
        egui::Panel::top("debugger_top").show_inside(root_ui, |ui| {
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("RustyNES debugger").strong());
                ui.separator();
                // v1.3.0 Workstream C — the per-panel toggle checkboxes were
                // removed here: every panel now opens from the always-visible
                // menu bar (Debug menu for chip inspectors, Tools menu for
                // Cheats / Netplay / Perf / ROM Database / ...), so this HUD no
                // longer duplicates them. It keeps only the live read-outs the
                // menu bar does NOT carry (frame/cycle, fps, movie status).
                ui.label(format!(
                    "frame={} cycle={}",
                    nes.ppu_snapshot().frame,
                    nes.cycle()
                ));
                ui.separator();
                ui.label(format!("fps: {:.1}", self.fps));
                // v1.4.0 Sprint 4.2 — TAS movie status indicator (read-only).
                match self.movie.mode {
                    MovieMode::Idle => {}
                    MovieMode::Recording => {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!("REC {} frames", self.movie.cursor))
                                .color(egui::Color32::from_rgb(0xE0, 0x40, 0x40))
                                .strong(),
                        );
                    }
                    MovieMode::Playing => {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!(
                                "PLAY {}/{}",
                                self.movie.cursor, self.movie.total
                            ))
                            .color(egui::Color32::from_rgb(0x40, 0xC0, 0x40))
                            .strong(),
                        );
                    }
                }
                // v2.2.0 — FDS disk-side indicator (read-only). Only shown for
                // FDS games (disk_side_count() > 0). Read straight off `nes`.
                let disk_sides = nes.disk_side_count();
                if disk_sides > 0 {
                    ui.separator();
                    let label = nes.inserted_disk_side().map_or_else(
                        || "Disk: Ejected".to_string(),
                        |s| format!("Disk: Side {}/{disk_sides}", s + 1),
                    );
                    ui.label(
                        egui::RichText::new(label)
                            .color(egui::Color32::from_rgb(0xF0, 0xC0, 0x40))
                            .strong(),
                    );
                }

                // v2.3.0 — netplay HUD (read-only). Only shown while a
                // session is active or connecting.
                let net = self.netplay_ui.status();
                let (txt, color) = match net.phase {
                    NetplayPhaseView::Idle => (String::new(), egui::Color32::WHITE),
                    NetplayPhaseView::Connecting => (
                        "NET connecting".to_string(),
                        egui::Color32::from_rgb(0xF0, 0xC0, 0x40),
                    ),
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
                            use std::fmt::Write as _;
                            let _ = write!(s, " rb{}", net.resimulated_frames);
                        }
                        if net.stalled {
                            s.push_str(" stall");
                        }
                        (s, egui::Color32::from_rgb(0x40, 0xC0, 0xF0))
                    }
                    NetplayPhaseView::Error => (
                        "NET error".to_string(),
                        egui::Color32::from_rgb(0xE0, 0x40, 0x40),
                    ),
                };
                if !txt.is_empty() {
                    ui.separator();
                    ui.label(egui::RichText::new(txt).color(color).strong());
                }

                // v2.7.0 — RetroAchievements HUD (read-only). Points + an
                // optional rich-presence string + active leaderboard trackers,
                // shown only when an RA session is logged in.
                let ra = self.cheevos_ui.status();
                if ra.enabled && ra.logged_in {
                    ui.separator();
                    let mut s = format!(
                        "RA {}{} {}pts",
                        ra.display_name,
                        if ra.hardcore { " [HC]" } else { "" },
                        ra.score
                    );
                    if ra.total > 0 {
                        use std::fmt::Write as _;
                        let _ = write!(s, " {}/{}", ra.unlocked, ra.total);
                    }
                    ui.label(
                        egui::RichText::new(s)
                            .color(egui::Color32::from_rgb(0xF0, 0xD0, 0x60))
                            .strong(),
                    );
                    if !ra.rich_presence.is_empty() {
                        ui.label(egui::RichText::new(&ra.rich_presence).italics());
                    }
                    for tr in &ra.trackers {
                        ui.separator();
                        ui.label(
                            egui::RichText::new(format!("LB {tr}"))
                                .color(egui::Color32::from_rgb(0x40, 0xC0, 0xF0)),
                        );
                    }
                }
            });
        });

        if self.show_cpu {
            cpu_panel::show(ctx, &mut self.show_cpu, &mut self.cpu_ui, nes);
        }
        if self.show_ppu {
            ppu_panel::show(ctx, &mut self.show_ppu, &mut self.ppu_ui, nes);
        }
        if self.show_oam {
            oam_panel::show(ctx, &mut self.show_oam, &mut self.oam_ui, nes);
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
                memory_panel::show(ctx, &mut self.show_memory, &mut self.memory_ui, nes);
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
            trace_panel::show(ctx, &mut self.show_trace, &mut self.trace_ui, nes);
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

        if self.show_input {
            input_rebind_panel::show(ctx, &mut self.show_input, &mut self.input_ui, config);
        }
        if self.show_input_display {
            input_display_panel::show(
                ctx,
                &mut self.show_input_display,
                &mut self.input_display_ui,
                &self.input_pads,
                self.input_players,
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
