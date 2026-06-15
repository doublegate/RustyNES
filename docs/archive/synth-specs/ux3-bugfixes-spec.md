# UX round-3 bug fixes (3 user-reported runtime bugs)

Frontend: `/home/parobek/Code/OSS_Public-Projects/RustyNES/crates/rustynes-frontend/`.
winit 0.30 + wgpu + egui 0.29 + cpal + a dedicated emulation thread. Build on the current
(committed) tree. Goal: bug-free. NO new `unsafe`. Match surrounding style. CANNOT runtime-test
(headless) — reason carefully; the user will smoke-test. All three produce paths matter where noted
(emu-thread default, synchronous `--no-default-features`, wasm).

## BUG 1 — Pause has no way to unpause (menu freezes while paused) + no keybind

Symptom: Emulation -> Pause pauses, but the menu bar stops responding (can't click Resume), and there
is no keyboard bind for pause, so the only escape is closing the window.
Two parts:

1. **Add a Pause keybind.** Add `SysAction::TogglePause` (input.rs) + a `[input.system] pause` bind.
   Pick a DEFAULT key that is NOT already bound (check `SystemBindings` defaults AND the pad maps —
   note `KeyP` is P2 Start, so do NOT use P). `Space` or `Pause` (the Pause/Break key) or `Backslash`
   is taken by frame-advance — use `Space` if free, else `Pause`. In `window_event`'s SysAction match,
   `SysAction::TogglePause => self.set_paused(!self.ui.paused)` (the same path as the menu).
2. **Keep the UI responsive while paused.** The emu thread parks when paused, so no `EmuFrame` events
   fire, so no `request_redraw`, so the menu never repaints to receive the Resume click. A round-2
   fix added an `egui_wants_repaint()` pump in `window_event` but it is INSUFFICIENT in practice.
   Make it robust: while `self.ui.paused` (and a ROM is loaded, not netplay), in `pace_frames` set
   `ControlFlow::WaitUntil(now + ~33ms)` AND `request_redraw()` so the shell redraws at ~30 Hz and
   stays fully interactive (menu hover/click work). Verify the emu thread does NOT produce frames while
   paused (only the UI redraws). Confirm on emu-thread + synchronous paths (wasm already self-arms rAF).
   The Pause keybind also gives a guaranteed escape even if a redraw edge is missed.

## BUG 2 — View > Window Size scales the WHOLE window (chrome unreadable) + desyncs the mouse

Symptom: selecting a Window Size scales the entire window including the menu/status bars; at small
sizes everything is unreadable and the mouse pointer is offset from the menu hit-areas (you must click
off-target). Dynamic drag-resize works perfectly (chrome stays normal, the game letterboxes) — so the
rendering handles arbitrary sizes; the bug is in `App::set_window_scale` (app.rs ~2167) and/or how the
requested size interacts with the chrome.
Root cause analysis: `set_window_scale` requests `LogicalSize(NES_W*scale, NES_H*scale + 48)`. At 1x
that is 256x288 — too NARROW for the menu bar (File/Emulation/Tools/View/Debug/Help need ~480+ logical
px), so the menu overflows/clips and its hit-areas land off the visible text (the "mouse desync").
Fix:

- The window must be wide enough for the chrome. Compute `width = max(NES_W*scale, MIN_CHROME_WIDTH)`
  with `MIN_CHROME_WIDTH ~= 560` (enough for the 6-menu bar + status bar comfortably), and
  `height = NES_H*scale + CHROME_HEIGHT` where `CHROME_HEIGHT` is the real menu+status bar height (~56
  logical px; measure if you can, else a constant). The game letterboxes within (drag-resize already
  does this correctly).
- Verify the resize propagates cleanly: `request_inner_size` -> a `Resized` event -> `on_window_event`
  feeds egui + `gfx.resize` reconfigures the surface (app.rs ~3710). Confirm egui's pointer hit-test
  matches the render after the resize (no stale pixels_per_point / size). If a ScaleFactorChanged is
  involved, handle it. The goal: Window Size sets the GAME to ~Nx with the chrome at a fixed, readable
  size and the mouse aligned — exactly like a manual drag-resize.
- Native only (the menu item is already cfg-gated).

## BUG 3 — Game Genie cheats added + toggled On do nothing (even after Reset / Power Cycle)

Symptom: codes added in Tools -> Cheats, enabled, have no effect in the running game, nor after Reset
or Power Cycle. `cheat_panel::body` correctly marks `changed` on add/toggle/remove, and `show()` calls
`resync_nes(state, nes)` (clear_genie_codes + add_genie_code for each ENABLED entry) ONLY when changed.
Investigate the ACTUAL root cause (it likely fails for one or more of these — fix whichever apply):

1. **`changed`-only resync.** After Reset/Power-Cycle (which may clear/rebuild the core's genie map),
   the panel does NOT resync (the list didn't change), so the codes are gone from the core. FIX:
   resync the genie codes to the live core whenever it could have lost them — re-apply the panel's
   enabled codes in `do_reset` / `do_power_cycle` / on ROM load, AND/OR resync EVERY frame the panel is
   open (move `resync_nes` out of the `if changed` guard so the live core always reflects the enabled
   set). The every-frame resync is cheap (a few BTreeMap ops) and robust.
2. **Live-core delivery.** Verify the cheat panel actually receives the LIVE emu-thread core: when
   Cheats is open with the deep debugger OFF, `render_shell` must take the locked branch
   (`needs_nes = dbg_visible || any_nes_tool_open()`, and `any_nes_tool_open()` must include
   `show_cheat`) and pass `&mut emu.nes` (the running core) to the tool panels — NOT `None`, not a copy.
   Confirm `tool_panels` forwards the real `&mut Nes` to `cheat_panel::show`.
3. **Run-ahead interaction.** Run-ahead (default 1) snapshots + restores the core each frame. The genie
   overlay is a runtime map NOT in the save-state, so it SHOULD survive snapshot/restore — VERIFY this:
   if run-ahead (or its snapshot/restore) drops the genie codes, the codes would be wiped every frame.
   If so, ensure the genie map is preserved across the run-ahead snapshot/restore (it must not be reset).
   Test this if feasible (a unit/integration test: add a genie code, run a frame with run-ahead, assert
   the code still applies / a PRG read reflects it).
Make cheats reliably apply: live, persistent, and surviving Reset/Power-Cycle. Add a regression test if
you can isolate the root cause.

## Mandatory gates (run ALL; all must pass)

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --lib --bins -- -D warnings`
4. `cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --no-default-features --features wasm-canvas --lib --bins -- -D warnings`
5. `cargo clippy -p rustynes-frontend --no-default-features -- -D warnings`
6. `cargo clippy -p rustynes-frontend --features retroachievements --all-targets -- -D warnings`
7. `RUSTDOCFLAGS="-D warnings" cargo doc -p rustynes-frontend --no-deps`
8. `cargo test -p rustynes-frontend` (+ `cargo test -p rustynes-core genie` / `cargo test --features test-roms` if you add a cheat/run-ahead test that needs the core)
WGSL strings are double-quoted Rust strings (no `"` in shader `//` comments). clippy is pedantic+nursery
(empty no-op fns -> `const fn` + `&self`; gate dispatch bodies with `#[cfg]`, keep variants un-gated).
Do NOT touch docs (updated separately) or unrelated core code. Frontend only (unless BUG-3 root cause
genuinely requires a tiny core fix for the genie/run-ahead interaction — if so, keep it minimal + add a
test, and the change must NOT alter the no-cheat byte-identical path).

## Report

Per bug (1/2/3): root cause found + exactly how you fixed it (and which of BUG-3's 3 hypotheses was the
real cause). The Pause + any new key binds chosen. The exact pass/fail of all 8 gates. Flag everything
as needing a runtime smoke-test. Be honest about anything you could not fully root-cause.
