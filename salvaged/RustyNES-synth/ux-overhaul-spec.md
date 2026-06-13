# RustyNES desktop UX overhaul — combined implementation spec

Frontend: `/home/parobek/Code/OSS_Public-Projects/RustyNES/crates/rustynes-frontend/`.
egui 0.29 + winit 0.30 + wgpu + cpal. Line numbers are anchors — re-confirm before editing.
Goal: production-quality, reference-grade, **bug-free**. Implement in the order below.
NO new `unsafe`. Match surrounding style (doc comments on pub items, no emojis).

## MANDATORY GATES (all must pass before done)
- `cargo fmt --all --check`
- `cargo clippy --workspace --all-targets -- -D warnings`
- `cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --lib --bins -- -D warnings` (wasm-winit)
- `cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --no-default-features --features wasm-canvas --lib --bins -- -D warnings`
- `cargo clippy -p rustynes-frontend --no-default-features -- -D warnings` (synchronous path, emu-thread OFF)
- `cargo clippy -p rustynes-frontend --features retroachievements --all-targets -- -D warnings`
- `RUSTDOCFLAGS="-D warnings" cargo doc -p rustynes-frontend --no-deps`
- `cargo test -p rustynes-frontend`
New `MenuAction` variants/dispatch arms must compile in ALL flavours: gate the dispatch *bodies* with `#[cfg]`, keep the *variants* un-gated for exhaustive matches (the `LoadRom` idiom at app.rs ~1784).

---

# PHASE 1 — BUG FIXES (do first, highest value)

## BUG-1 Pause toggles ON but not OFF (root cause: redraw heartbeat dies)
The native frame loop is a self-sustaining redraw ping-pong (emu produces → `EmuFrame` → `on_emu_frame` → `request_redraw`). When paused, the emu thread parks and never sends `EmuFrame`, so no `request_redraw` is ever issued again; `pace_frames` sets `ControlFlow::Wait`. The menu can't repaint, so the "Resume" click (only dispatched inside `RedrawRequested`) is never received → wedged. Fix (native; wasm self-arms rAF so it's unaffected):
1. Add to `DebuggerOverlay` (mod.rs near `wants_egui_input` ~274):
   ```rust
   #[must_use]
   pub fn egui_wants_repaint(&self) -> bool { self.state.egui_ctx().has_requested_repaint() }
   ```
2. In `app.rs` `window_event` (after `egui_consumed` is computed ~3211), add a native redraw pump so egui repaints on input while idle/paused:
   ```rust
   #[cfg(not(target_arch = "wasm32"))]
   if let (Some(debugger), Some(gfx)) = (self.debugger.as_ref(), self.gfx.as_ref()) {
       if debugger.egui_wants_repaint() { gfx.window.request_redraw(); }
   }
   ```
3. Add `EmuThread::unpark(&self)` (emu_thread.rs) → `self.handle.as_ref().map(|h| h.thread().unpark())`. In `set_paused` (app.rs ~1848), on resume (`!paused`) call `thread.unpark()` AND rebase the pacer: `self.emu.lock().next_frame_time = Some(Instant::now());` (before the existing `request_redraw` ~1862; mirrors the netplay-leave rebase ~2065) — avoids a catch-up burst.
Verify pause/resume on emu-thread, `--no-default-features` (synchronous — same heartbeat fix applies), and wasm builds.

## BUG-2 Fullscreen double-toggle
ui_shell.rs ~378: `checkbox(&mut self.fullscreen, ...)` flips the mirror, then dispatch `toggle_fullscreen` (app.rs ~1868) flips `self.ui.fullscreen` AGAIN → net no-op. Fix with the read-only-mirror pattern (like "Show Debugger" ~386):
```rust
let mut fs = self.fullscreen;
if ui.checkbox(&mut fs, "Fullscreen").changed() { out.action = Some(MenuAction::ToggleFullscreen); ui.close_menu(); }
```
`toggle_fullscreen` stays the single source that flips + applies. Add `SysAction::ToggleFullscreen` (input.rs enum ~53) + default bind `"F11"` + a `config.input.system.fullscreen` field (`#[serde(default = "...")]` default "F11") + `try_bind`; in `window_event` SysAction match (~3291) add `SysAction::ToggleFullscreen => self.toggle_fullscreen(),`.

## BUG-3 (audit M1) Escape quits even in fullscreen (hard-quit hazard)
Esc = `SysAction::Quit`. Change the Quit arm (app.rs ~3292) to exit fullscreen first:
```rust
SysAction::Quit => { if self.ui.fullscreen { self.toggle_fullscreen(); } else { self.should_exit = true; event_loop.exit(); } }
```

## BUG-4 (audit C1) Pausing wedges native netplay
`pace_frames` checks `if self.ui.paused { return; }` (~2252) BEFORE the netplay branch (~2314); same on wasm (~2745 gates the netplay branch at ~2761). Pausing during a session stalls rollback → peer desync. Fix: do NOT pause while a netplay session is active. Gate the pause early-return with `&& !self.netplay.is_active()` on BOTH the native (~2252) and wasm (~2745) paths, AND disable the Pause menu item / refuse `set_paused(true)` when netplay is active (add a `netplay_active: bool` to `ShellFrame`, captured before the egui pass; `add_enabled(frame.rom_loaded && !frame.netplay_active, Pause)`). Confirm `self.netplay.is_active()` exists (netplay_ui) — else use the session-active check the pacer already uses.

## BUG-5 (audit C2) Keyboard leaks to the NES while a menu/modal is open
The input gate (`wants_egui_input` = `wants_keyboard_input() || wants_pointer_input()`) does NOT catch an open egui *menu* (no focused text widget) → with a menu dropped, arrows/Z/X/Enter drive the NES and Esc both closes the menu and quits. Fix: also suppress NES key handling when a menu or any shell window/modal is open. Add to the gate a "shell is capturing" check: `ctx.memory(|m| m.any_popup_open())` OR a `UiShell` flag set true when `show_settings_window || show_about || show_shortcuts || show_welcome`. Thread this into the `handle_key` gate in `window_event` (~3268-3394) so emulator key handling (and `latch_input`) is skipped when the shell is capturing. Keep the existing `egui_consumed` gate too.

## BUG-6 (audit C3) Input gate is one frame stale → first keystroke into a field leaks
`wants_egui_input()` reflects the PREVIOUS `ctx.run`. The frame a text field gains focus it's still false → that first keystroke types into the field AND drives the NES. Fix: gate `handle_key` on the CURRENT event's `egui_consumed` (the bool already in scope ~3206) — i.e. skip `handle_key` when `egui_consumed` is true (today `egui_consumed` only gates `latch_input` at ~3385, not `handle_key`). Combine with BUG-5's shell-capturing check.

## BUG-7 (audit M2) `paused` mirror not reset on power-cycle
`do_power_cycle` (app.rs ~1883) and `do_reset` don't clear `self.ui.paused`. After Power Cycle the status bar can read "Paused" with a running core. Fix: clear `self.ui.paused` (+ `set_user_paused(false)` + unpark) in `do_power_cycle` (a cold boot should run). Leave `do_reset` paused-state alone unless it reads better to also resume — your call, document it.

## BUG-8 (audit M3) FPS readout frozen while paused
`current_fps()` returns the last rolling mean (e.g. 60.0) while paused. In the status bar (ui_shell.rs ~445-449) show `0.0` (or hide FPS) when `self.paused`.

## BUG-9 (audit M4) Emu thread produces one extra frame after pause (TOCTOU)
`drive_one`/`drive_wallclock` re-check `netplay_paused` under the lock (~407) but not `user_paused`. Add `|| control.user_paused.load(Acquire)` to that under-lock re-check so a just-issued pause is honored before producing.

---

# PHASE 2 — SETTINGS WINDOW SPLIT (eliminate duplication)
`settings_panel::body` (settings_panel.rs ~159) renders Graphics+Audio+Latency+Rewind all at once; the Settings window (ui_shell.rs ~482-525) calls it for Video, Audio, AND Advanced → every tab shows everything. Fix:
- Split `body` into `video_section` / `audio_section` / `advanced_section` (settings_panel.rs), each `pub fn (ui, &mut SettingsPanelState, &mut Config)`, preserving `SettingsApply` accumulation:
  - **Video**: present_mode (~163-176), pacing_mode (~180-198), max_frame_latency (~202-210), ntsc_filter (~213-228).
  - **Audio**: sample_rate (~235-250), latency_ms (~254-263), drc (~266-269).
  - **Advanced**: run-ahead (~277-285) + rewind enable/window/keyframe (~291-315).
- Keep the debugger's standalone `show` (~104) working by calling the three sections sequentially (or keep `body` as a thin wrapper calling all three).
- Rewire the shell: change `UiShell::build`'s `settings_body` closure to take a `SettingsTab` arg (`impl FnMut(&mut Ui, &mut Config, SettingsTab)`); `render_shell` (mod.rs ~795) routes to the right section per tab. In `settings_window` (ui_shell.rs ~455): Video tab keeps theme combo + 8:7 + show-fps THEN calls `settings_body(ui, config, Video)`; Audio/Advanced call only their section; Input calls `input_body`. Remove the `Audio | Advanced => settings_body(...)` catch-all that caused the dup. `SettingsApply` plumbing (take_settings_apply ~mod.rs:320, applied ~app.rs:3646+3682) stays intact.

---

# PHASE 3 — MENU IA + SURFACE BURIED FEATURES
Surface Netplay/RA/Cheats/Movies/Perf/save-slots/disk/screenshot in the menu bar. Reuse the EXISTING debugger panels as floating windows (don't rebuild them).

## Plumbing
- Make tool panels render as floating windows even when the deep debugger overlay is OFF. Split `DebuggerOverlay::ui` (mod.rs ~388) into `chip_panels` (CPU/PPU/OAM/APU/Memory/Mapper — need `nes`, gated on `visible`) and `tool_panels` (Cheats/Settings/Netplay/Cheevos/Perf/Input — render whenever their `show_*` bool is set, regardless of `visible`). In `render_shell` (mod.rs ~803): always call `tool_panels`; call `chip_panels` only when `visible`. Tool panels reading `nes` (Cheats) must no-op when `nes` is None.
- Add `pub enum ToolPanel { Cheats, Settings, Netplay, Cheevos, Perf, Input }` + `pub fn open_panel(&mut self, p: ToolPanel)` (sets the `show_*` bool) + `pub fn force_visible(&mut self)` (for chip panels) + `pub fn any_nes_tool_open(&self) -> bool` (returns `self.show_cheat`).
- **Render-branch fix (critical):** the hidden branch (app.rs ~3521-3571) passes `nes = None`. When a `nes`-needing tool panel (Cheats) is open while the deep overlay is off, it must take the locked branch. Compute `needs_nes = dbg_visible || self.debugger.as_ref().is_some_and(|d| d.any_nes_tool_open())` and use it to choose the locked vs staging branch at ~3469. KEEP all panel rendering inside the single `ctx.run` closure with the passed-in `nes` — do NOT add a second `self.emu.lock()` inside the closure (double-lock).
- Add `MenuAction::OpenPanel(ToolPanel)` → dispatch via `self.debugger.as_mut().map(|d| d.open_panel(p))` (+ `force_visible` for chip variants).
- Save-state slots: add `App.active_save_slot: u8` (default 0); parameterize `handle_save_state`/`handle_load_state` (~942/967) by slot (replace hardcoded `0`). Add `MenuAction::{SetSaveSlot(u8), SaveStateSlot(u8), LoadStateSlot(u8)}`.
- `ShellFrame` (ui_shell.rs ~212) gains: `netplay_active: bool`, `disk_sides: usize`, `vs_system: bool`, `movie_mode` (an enum/label), `mapper_label: &str`, `region_label: &str`, `run_ahead: u32`, `paused: bool`. Capture them under the brief lock before the egui pass (app.rs ~3454) — use existing getters; if a getter is missing (mapper name), omit that field rather than add core API.

## Menu structure (rewrite `menu_bar`, ui_shell.rs ~260)
Enable: `R`=rom_loaded, `N`=`#[cfg(not(wasm32))]`, FDS=`disk_sides>0`, Vs=`vs_system`, RA=`feature="retroachievements"` + N.
- **File**: Open ROM…(N), Open Recent ▸(N), —, Insert/Eject Disk + Swap Disk Side (R+FDS → `CycleDiskSide`), —, Save State (R), Load State (R), Save Slot ▸ 1-8 (radio `SetSaveSlot`), Save to Slot/Load from Slot ▸ (`SaveStateSlot`/`LoadStateSlot`), —, Take Screenshot (R,N → `Screenshot`), —, Quit.
- **Emulation**: Pause/Resume (R, disabled if netplay_active), Reset (R), Power Cycle (R), —, Run-Ahead ▸ 0-3 (radio on `config.input.run_ahead`, set in closure + save_config), Region: NTSC/PAL (read-only label from region), Vs. Insert Coin (R+Vs → `InsertCoin`).
- **Tools**: Cheats… (`OpenPanel(Cheats)`), Movies (TAS) ▸ Record/Play/Branch (R → `MovieRecordToggle`/`MoviePlayToggle`/`MovieBranch` → existing `handle_movie_*` ~1006/1041/1101; labels reflect `movie_mode`), Netplay…(N → `OpenPanel(Netplay)`), RetroAchievements…(RA → `OpenPanel(Cheevos)`), Performance Monitor (`OpenPanel(Perf)`).
- **View**: Settings…, Theme ▸, 8:7 Pixel Aspect, Fullscreen (fixed), Show FPS (checkbox on `config.ui.show_fps` + save), Show Menu Bar (toggle `self.menu_visible` — add `SysAction::ToggleMenuBar` default `Ctrl+M` so there's a keyboard way back). [Scale ▸ — DEFER, don't block.]
- **Debug**: Show Debugger (` ` `), CPU/PPU/APU/Memory/OAM/Mapper (→ `OpenPanel(...)` + `force_visible`).
- **Help**: Keyboard Shortcuts, About.

## New MenuAction variants + dispatch (app.rs ~1779)
`CycleDiskSide, Screenshot, SetSaveSlot(u8), SaveStateSlot(u8), LoadStateSlot(u8), MovieRecordToggle, MoviePlayToggle, MovieBranch, InsertCoin, OpenPanel(ToolPanel)`. Each arm delegates to the existing App method; gate bodies with `#[cfg]`, keep variants un-gated.

## Screenshot (app.rs new method, N)
`take_screenshot`: under a brief lock copy `nes.framebuffer()` (256×240 RGBA), PNG-encode (use the `image` crate if already a dep; else a minimal writer), write `<data_dir>/screenshots/<rom>-<utc>.png`, toast the path. wasm: no-op.

---

# PHASE 4 — POLISH (world-class feel)
- **Paused overlay**: when `shell.paused`, draw a translucent full-screen `egui::Area` with a centered "PAUSED" label (no emoji). In `render_shell` closure after the shell build.
- **Disable-when-no-ROM consistency**: every R-gated menu item uses `add_enabled(frame.rom_loaded, ...)`; non-ROM tools (Settings/Perf/Theme) stay enabled.
- **Richer status bar**: ROM • Region • Mapper(if getter) • Running/Paused • RA pts(if available) • Run-Ahead(if >0) • FPS.
- **Accelerator hints in menus**: a helper rendering `label` + right-aligned weak key, pulled from `config.input.system.*` so it tracks rebinds. Apply to File/Emulation/Tools/View.
- **(audit m2) Welcome modal**: make it dismissible/modal AND persist `welcome_shown = true` the first time it's shown (not only on "Get Started"), so it never re-nags. Use `egui::Window::open(&mut ...)` or set `welcome_shown` on first display.
- **(audit m3) Recent ROMs**: gray out (or skip) entries whose `path.exists()` is false; surface a `StatusMessage` on a failed `LoadRom` dispatch (load_rom_from_path should report failure).
- **(audit m4) Fullscreen on wasm**: gate the Fullscreen menu item out on wasm OR accept browser behavior; don't let the mirror lie — if kept, note it. Simplest: `#[cfg(not(wasm32))]` the Fullscreen item.
- **(audit m6) apply_theme**: only call `ctx.set_visuals` when the theme actually changed (cache last-applied theme) instead of every frame.
- **Welcome/shortcuts**: add the new F11/F6/F7/F8/F9/F10 rows; drive from config where feasible.

---

# RISKS (from the design + audit)
- render_shell closure borrows `&mut config/ui/debugger` and must NOT lock the emu; keep all panel rendering inside the single `ctx.run` with the passed-in `nes` (no second lock).
- emu-thread: `unpark` is safe (park_timeout consumes the token); the `next_frame_time` rebase write goes through the `EmuHandle` mutex (set_paused runs on the winit thread post-egui — lock is free).
- wasm parity: Netplay + RA menu items native-only; wasm uses its own `wasm_lobby`. Pause needs no wasm change.
- Region menu is read-only (no core setter) — display only.
- Keep a keyboard path back when the menu bar is hidden (`ToggleMenuBar`).
