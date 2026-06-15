# UX #6 — standard QoL features (fast-forward, frame-advance, pause-on-focus)

Frontend: `/home/parobek/Code/OSS_Public-Projects/RustyNES/crates/rustynes-frontend/`.
These three features are universal in Mesen2/fceux/nestopia and missing here. The current
tree already has uncommitted UI/UX fixes (#1–#5) — build on them. Goal: production-quality,
bug-free. egui 0.29 + winit 0.30. NO new `unsafe`. Match surrounding style (doc comments, no
emojis, the existing `MenuAction`/dispatch + `EmuControl` atomic idioms).

The emu thread is delicate (a recent pause bug lived in its idle gate) — be precise there.
Everything must work on THREE produce paths: the dedicated emu thread (default `emu-thread`),
the synchronous native path (`--no-default-features`), and wasm (`pace_and_produce_wasm`).

## A. Fast-forward (hold a key to run unthrottled)

- `EmuControl` (emu_thread.rs ~178): add `fast_forward: AtomicBool` + `set_fast_forward(&self, on)` / `is_fast_forward(&self)`.
- Emu-thread produce path (run loop ~340-368): when `fast_forward` is set, SKIP the throttle —
  in the WALLCLOCK/VRR branch don't call `block_until_native(next)`; in the DISPLAY branch use a
  short `recv_timeout` or just `drive_wallclock` immediately (produce back-to-back). After a
  fast-forwarded frame, set `next_frame_time = Instant::now()` so leaving FF doesn't burst.
- Synchronous native path (`pace_frames`) + wasm path (`pace_and_produce_wasm`): when fast-forward
  is active, produce extra frames per tick (or skip the sleep/`WaitUntil` throttle). Cap the
  catch-up (e.g. <= 8 frames per pace) so a held key can't wedge the UI.
- **Audio during FF**: running unthrottled overruns the lock-free audio ring (producer outpaces
  the cpal consumer). MUTE during fast-forward: in `drive_one`/`drive_wallclock`, pass `None` for
  the audio sink when `fast_forward` is set (so no samples are pushed — the cpal callback plays
  its underrun-silence). Do NOT let the ring spam overruns. Verify `sinks_for(None)` is valid.
- Input: add a HELD fast-forward key. Mirror the existing Rewind held-key path exactly:
  `InputState::rewind_held()` (input.rs) + the per-frame use at app.rs ~1622 (`rewind_held: ...`).
  Add `InputState::fast_forward_held()` + a `[input.system] fast_forward` bind (default key — pick
  one not already bound; `Tab` or `Equal` are conventional; check input.rs binds + the keymap
  table). Each frame (where the app already reads `rewind_held` / pushes per-frame state), push
  `self.input.fast_forward_held()` -> `thread.control().set_fast_forward(..)` on the emu-thread
  path, and read it directly on the sync/wasm paths.
- Menu: in the Emulation menu (ui_shell.rs), add a disabled/info "Fast Forward (hold <key>)" or a
  checkbox showing the live held state — your call; a labelled hint is fine since it's a held key.

## B. Frame advance (press a key to step exactly one frame while paused)

- `EmuControl`: add `frame_advance: AtomicU32` + `request_frame_advance(&self)` (fetch_add 1) +
  `take_frame_advance(&self) -> bool` (compare-and-decrement: if >0, decrement and return true).
- Emu-thread idle gate (~332-338): currently parks when `!has_rom || netplay_paused || user_paused`.
  Change so a pending frame-advance wakes it: if the idle condition holds AND `frame_advance == 0`,
  park as before; else if `has_rom && !netplay_paused` and `frame_advance > 0` (i.e. user-paused but
  a step was requested), `take_frame_advance()` and produce EXACTLY ONE frame UNTHROTTLED
  (`drive_one`/`drive` directly, no `block_until_native`, no tick wait), send `AppEvent::EmuFrame`,
  then loop (re-park). Frame-advance should also push input for that one frame (use the latched
  input as a normal frame does).
- App: add `SysAction::FrameAdvance` (input.rs) + a `[input.system] frame_advance` bind (default
  e.g. `Backslash` or `Period`). In `window_event`'s SysAction match, on FrameAdvance:
  `thread.control().request_frame_advance(); thread.unpark();` (emu-thread). On the sync/wasm paths,
  produce one frame directly. Only meaningful while paused (a no-op / single-step while running is
  acceptable — or gate on `self.ui.paused`).
- Menu: Emulation menu -> "Frame Advance" item (enabled when ROM loaded), emitting
  `MenuAction::FrameAdvance` -> dispatch to the same request path. Show the accelerator.

## C. Pause when window loses focus (opt-in QoL)

- Config: `[ui] pause_on_focus_loss: bool` (default `false`, so no behavior change unless enabled).
  Add the field to `UiConfig` (config.rs) with serde default false + the manual Default.
- App: handle `WindowEvent::Focused(focused)` (app.rs `window_event`). When `config.ui.pause_on_focus_loss`
  and NOT in a netplay session: on `Focused(false)` -> if not already paused, `set_paused(true)` and
  remember it was an AUTO pause (add `auto_paused: bool` to `App` or `UiShell`); on `Focused(true)` ->
  if `auto_paused`, `set_paused(false)` and clear the flag. Don't fight a manual user pause (only
  auto-resume what auto-pause paused). Never auto-pause during netplay.
- Menu: View menu -> a "Pause When Unfocused" checkbox bound to `config.ui.pause_on_focus_loss`
  (+ `save_config` on change).

## New MenuAction variants

`FrameAdvance` (+ the dispatch arm). Fast-forward is a held key (no MenuAction needed unless you add
a toggle). Keep variants un-gated; `#[cfg]` the dispatch bodies as needed.

## Mandatory gates (run ALL; all must pass — this also re-verifies the uncommitted #1–#5)

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --lib --bins -- -D warnings`
4. `cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --no-default-features --features wasm-canvas --lib --bins -- -D warnings`
5. `cargo clippy -p rustynes-frontend --no-default-features -- -D warnings`
6. `cargo clippy -p rustynes-frontend --features retroachievements --all-targets -- -D warnings`
7. `RUSTDOCFLAGS="-D warnings" cargo doc -p rustynes-frontend --no-deps`
8. `cargo test -p rustynes-frontend`
WGSL string literals use double-quoted Rust strings — do NOT put `"` inside any `//` shader comment
(it terminates the string). clippy is pedantic+nursery: empty no-op fns need `const fn` + `&self`.

## Do NOT touch

Docs (the README/user-guide/menus/CHANGELOG are updated separately afterward). Core crates
(`rustynes-core` etc.) — frontend-only. The shader (`gfx.rs` SHADER_SRC) is already fixed.

## Report

Per feature (A/B/C): DONE/PARTIAL + how you wired the pacer / idle-gate / audio-mute / focus, on all
three produce paths. The exact pass/fail of all 8 gates. Flag every behavior that is compile-verified
only (all of it — headless) for a runtime smoke-test, especially the FF audio-mute + the frame-advance
single-step + the pacer catch-up cap.
