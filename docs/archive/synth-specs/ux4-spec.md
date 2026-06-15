# UX #4 — volume, speed presets, save-state thumbnails + curated ref-proj harvest

Frontend: `/home/parobek/Code/OSS_Public-Projects/RustyNES/crates/rustynes-frontend/`. Build on the
current committed tree (branch `feat/v1.0.0-synthesis`). egui 0.29 + winit 0.30 + wgpu + cpal + a
dedicated emulation thread. Goal: production-quality, bug-free, world-class polish. NO new `unsafe`.
Match surrounding style (doc comments, no emojis, the `MenuAction`/dispatch + `EmuControl` atomic +
`[serde(default)]` config idioms). You CANNOT runtime-test (headless) — reason carefully; the user
smoke-tests.

## HARD DETERMINISM RULE (the release gate)

Everything here is **frontend-only** EXCEPT feature I (per-APU-channel mute), which makes ONE minimal,
determinism-safe edit to `rustynes-apu` + a `rustynes-core` passthrough (a channel-enable mask that
defaults to all-on and is byte-identical in that default — see feature I). Apart from that single
documented exception, DO NOT modify `rustynes-core`, `rustynes-ppu`, `rustynes-apu`, `rustynes-cpu`,
or `rustynes-mappers`. The deterministic core output (framebuffer + per-frame audio)
must stay byte-identical: AccuracyCoin 100% (139/139) and the commercial oracles (60/60 + 52/52) MUST
still pass unchanged. Volume/speed/overscan/thumbnails/etc. are all playback/presentation-layer or
frontend-pacing changes that never touch the core's synthesis. Defaults must reproduce today's
behavior exactly (volume 1.0, speed 100%, overscan off). The save-state thumbnail already exists in
the CORE (`Nes::thumbnail()` / `Nes::extract_thumbnail()`); you only SURFACE it in the UI — no core
edit needed.

All three produce paths matter where relevant: the dedicated emu thread (default `emu-thread`), the
synchronous native path (`--no-default-features`), and wasm (`pace_and_produce_wasm`).

---

## A. Master volume control  (REQUESTED)

- **Config:** add `volume: f32` to `AudioConfig` (`config.rs` ~line 627), `#[serde(default = "default_audio_volume")]`,
  default **1.0**, clamped to `0.0..=1.0` on load. Also a `muted: bool` (default false) if you add a
  mute toggle (optional but nice).
- **Apply at the audio output (cpal consume side), single point:** in `audio.rs`, add a
  `gain: AtomicU64` (f32 bits, like the existing slot encoding) to `QueueInner` (default 1.0), a
  `set_gain(&self, g: f32)` / `gain(&self) -> f32` on the queue handle, and multiply each output
  sample by the gain inside `pop_or_silence` (the cpal callback path, ~line 210: `*o = f32::from_bits(bits) * gain;`).
  This is the correct master-volume point (post-resampler, lock-free, affects the buffered tail too).
  Read the gain once per callback into a local (don't reload per sample). A muted state = gain 0.
- **Wire it:** the producer/output side already shares the `QueueInner` Arc; expose a way for the App
  (winit thread) to set the gain — store an `Arc` handle to the queue (or a small `Arc<AtomicU64>`
  gain) on `App`/`EmuCore` so the Settings slider can update it live. The emu thread's fast-forward
  path already mutes via `sinks_for(None)`; master volume is independent of (and composes with) that.
- **UI:** a **Volume slider (0–100%)** in the Settings window **Audio tab** (live-applied + `save_config`
  on release), and a mute checkbox if you added `muted`. Optionally also a compact slider in the
  Tools/View area — but the Audio tab is the required home. Show the percentage.
- **Determinism:** the core still produces identical samples; gain is applied only at output. Default
  1.0 = today's sound exactly.

## B. Emulation-speed presets  (REQUESTED — 50% / 200% and friends)

- **Runtime state (transient, NOT persisted — always launches at 100%):** add `speed: f32` to `App`
  (default 1.0). Presets: **25% / 50% / 75% / 100% / 150% / 200% / 300%** (the user named 50% and
  200% explicitly; provide the full sensible set).
- **Pacing:** the target frame period is `EmuCore.frame_duration` (`emu.rs` ~196, init
  `FRAME_DURATION_NTSC`). Apply speed by pacing to `frame_duration / speed` (200% → half the period →
  2x frames/sec; 50% → double the period). Do this WITHOUT mutating the stored `frame_duration` base
  (keep the console rate intact for region/display logic) — scale at the pacing site(s):
  `produce_due_frames` / `block_until_native` targets in `emu.rs` + `emu_thread.rs`, and the
  synchronous `pace_frames` + `pace_and_produce_wasm` paths. A clean approach: add an
  `effective_frame_duration()` = `frame_duration.div_f32(speed)` used by every pacer site, or thread
  the speed factor through. Cap the per-tick catch-up burst (reuse the existing fast-forward cap) so a
  high speed can't wedge the UI.
- **Display-sync interaction:** when `speed != 1.0`, the display-sync pacing mode (one emulated frame
  per refresh) cannot represent a fractional rate — fall back to the wall-clock pacer for the duration
  (same idea as the existing sustained-miss fallback). At speed 1.0 the behavior is unchanged.
- **Audio at alt speed (pitch-shifted, glitch-free — the right behavior):** at 200% the emu produces
  ~2x samples/sec; if the resampler stayed at ratio ~1.0 the ring would overrun. Make the resampler's
  DRC band CENTER on the speed factor: in `resampler.rs`, add a `base_ratio: f64` (default 1.0) to
  `HermiteResampler`; `set_ratio`/the DRC law clamps the requested ratio to
  `[base_ratio*(1-MAX_DRC_DELTA), base_ratio*(1+MAX_DRC_DELTA)]` instead of around 1.0; add
  `set_base_ratio(speed)`. The frontend sets `base_ratio = speed` whenever speed changes. Result:
  audio consumes `speed`x input per output → no overrun, natural pitch shift (slow-mo at 0.5x, chipmunk
  at 2x) — exactly what users expect. (At speed 1.0, base_ratio 1.0 → byte-identical to today.) IF this
  resampler change proves risky, the acceptable fallback is to MUTE audio while `speed != 1.0` (the
  proven fast-forward path) — but prefer the pitch-shift.
- **UI + keys:** an **Emulation -> Speed** submenu (the 7 presets, the current one checkmarked) →
  `MenuAction::SetSpeed(f32)`. Add `SysAction::SpeedUp` / `SpeedDown` keybinds (step through the preset
  list; pick free defaults — e.g. `Equal`/`Minus` are conventional; VERIFY they're unbound) +
  optionally `SpeedReset`. Show the current speed in the status bar when `!= 100%` (e.g. "200%"). The
  fast-forward HOLD key (Tab) is separate and unchanged.

## C. Save-state thumbnails  (REQUESTED)

The core ALREADY captures a 128x120 RGBA8 thumbnail in `snapshot()` (the `THM` section) and exposes
`Nes::extract_thumbnail(&[u8]) -> Result<Option<Vec<u8>>, _>` to read it from a slot blob WITHOUT
restoring. This is pure frontend surfacing.

- **Find the slot files:** `app.rs` uses `save_state::save_to_slot(dir, &rom_sha256, slot, &blob)` /
  `load_from_slot(...)` (~lines 1071-1116). Find/extend the `save_state` module with a way to read a
  slot's raw blob or its path (a `slot_path(dir, sha, slot)` or `peek_slot`) so you can pull each
  slot's thumbnail + mtime without loading the full state into the running `Nes`.
- **Build a "Save States" manager window** (open from **File -> Save States…** and/or a menu item):
  a grid of the N slots (reconcile the real slot count — `active_save_slot` doc says 0-7 but the File
  menu's Save Slot submenu lists 0-9; make the manager cover the SAME range the menus use). Each tile
  shows the slot's thumbnail (or an "Empty" placeholder), the slot number, and the save timestamp; the
  active slot is highlighted. Each tile has **Save** (overwrite this slot from the current state) and
  **Load** buttons (or click=load, a Save-mode toggle). Loading/saving routes through the existing
  `handle_save_state(slot)` / `handle_load_state(slot)`.
- **egui textures:** decode each 128x120 RGBA8 thumbnail into an `egui::ColorImage` →
  `ctx.load_texture(...)` → `egui::TextureHandle`; CACHE the handles (keyed by slot) and INVALIDATE a
  slot's cached texture when that slot is (re)saved, when the ROM changes, and lazily refresh on window
  open. Don't rebuild textures every frame. Free/replace handles on ROM change to avoid leaks.
- **Native-first.** On wasm the slots live in `localStorage` (base64) — if surfacing thumbnails there
  is non-trivial, gate the manager window to native (`#[cfg(not(wasm32))]`) and leave wasm as-is; note it.

---

## Curated ref-proj harvest (frontend-only, low-risk — INCLUDE these)

### D. Controller hot-plug toast  (trivial)

In the gilrs pump (`app.rs` ~line 1061, the `while let Some(ev) = gilrs.next_event()` loop), match
`gilrs::EventType::Connected` / `Disconnected` and emit a `StatusMessage::info("Controller connected"
/ "...disconnected")` (reuse the existing toast/status system). Today these events are silently dropped.

### E. Deadzone slider in the Input settings tab  (trivial — exposes existing config)

`config.input.gamepad.axis_deadzone` already exists and is honored by `input.rs` (clamped 0.05–0.95)
but is editable only by hand. Add an `egui::Slider` (0.05..=0.95) for it to the **Input** settings tab
(`debugger/input_rebind_panel.rs` or the Input settings body) + `save_config`; the live `InputState`
re-reads it on rebuild (verify it picks up the change — re-apply if needed).

### F. Overscan crop toggle  (small — high value)

Add `hide_overscan: bool` to `GraphicsConfig` (`config.rs` ~555), `#[serde(default)]`, default
**false** (byte-identical default presentation). When on, the `gfx.rs` letterbox/blit samples a source
rect that excludes the top and bottom 8 NES scanlines (the CRT-cropped overscan) — adjust the blit UV
rect math (the same `letterbox()` / UV computation used for pixel-aspect), no new pass. Add a **View ->
Hide Overscan** checkbox + a Video-tab toggle. (Optional: a small px spinner; the bool is enough for v1.)

### G. Pause-screen dimming overlay  (small — premium feel)

When `frame.paused` (and a ROM is loaded), paint a semi-transparent dark rect (~40% black) over the
emulated viewport plus a large centered "PAUSED" label, in the shell build pass (`ui_shell.rs`, near
the existing status-bar pause handling). Don't cover the menu/status bars or modal windows. No shader,
no core change. Make it look intentional and clean.

### H. "Reset to Defaults" in Settings  (small — safety net)

Add a button in the Settings window (per-tab is cleanest, or a single footer button) that restores the
relevant config section to its `Default` and calls `save_config` + re-applies live (reuse the existing
graphics/audio live-apply paths). Guard with a confirmation (a second click / a small confirm) so it's
not a foot-gun.

---

## I. Per-APU-channel mute toggles  (survey #1 — the one item that touches the core APU)

Checkboxes to enable/disable Pulse1, Pulse2, Triangle, Noise, DMC (and mapper/expansion audio)
individually — the staple "studio/debug" audio feature.

- **Core change (the ONLY permitted core edit in this batch — must be determinism-safe):** add a
  channel-enable mask to `rustynes-apu` (a `channel_mask: u8` or per-channel `enabled` bools),
  **defaulting to ALL channels enabled**, consulted in the APU mixer so a disabled channel contributes
  0 to the mixed sample. Expose a passthrough on `Nes` (e.g. `Nes::set_apu_channel_enabled(ch, on)` /
  a `set_apu_channel_mask(u8)`), mirrored through `rustynes-core`.
- **DETERMINISM (hard):** the default (all channels enabled) MUST be byte-identical to today — the mask
  is a pure runtime PLAYBACK overlay that the oracle/AccuracyCoin/tests NEVER set, so with the default
  mask the mixer output is bit-for-bit unchanged. Do NOT put the mask in the save-state (it's a UI
  preference, like volume); if you must, default it all-on so restored states are unchanged. Gate 9
  (AccuracyCoin 139/139 + the commercial oracles) is the proof — it MUST stay green. If you cannot make
  all-on byte-identical, STOP and report rather than regress byte-identity.
- **UI:** 5–6 checkboxes ("Pulse 1 / Pulse 2 / Triangle / Noise / DMC / Mapper Audio") in the Settings
  **Audio tab** (next to the master volume slider from feature A), live-applied through the emu thread
  - `save_config` (persist the mask as a frontend `[audio]` preference, default all-on). The emu thread
  must push the mask to the core under the emu lock (respect the lock discipline).
- **Complexity:** small-medium. Keep the mixer hook minimal (mask each channel's contribution).

## J. Screenshot-to-clipboard  (survey #3 — adds the `arboard` dep, native-only)

Copy the current frame to the system clipboard, in addition to the existing save-to-PNG file.

- Add the `arboard` crate as a **native-only** dependency (`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]`
  in `crates/rustynes-frontend/Cargo.toml`); it must NOT enter the wasm build.
- Reuse the existing framebuffer-grab in `app.rs` `take_screenshot` (~line 1128): hand the RGBA8 buffer
  (width 256, height 240 — or the post-pixel-aspect image; the raw NES RGBA is simplest and correct) to
  `arboard::Clipboard::set_image(ImageData { width, height, bytes })` instead of (or in addition to) the
  PNG encoder. Add `MenuAction::ScreenshotToClipboard` (File menu, near Take Screenshot) + optionally a
  `SysAction` keybind (pick an unbound key; verify). Emit a `StatusMessage` toast ("Screenshot copied to
  clipboard"). All `#[cfg(not(target_arch = "wasm32"))]`-gated; the menu item is hidden/disabled on wasm.
- **Complexity:** trivial-small. Handle the `arboard` error path gracefully (toast on failure, never panic).

## K. Live FPS / frame-time graph  (survey #6 — prefer NO new dep)

An optional on-screen rolling graph of frame-time (ms), beyond the numeric Performance-panel table.

- The Performance panel (`debugger/perf_panel.rs`) already collects rolling interval stats
  (`IntervalStats`, the produced/presented series). Feed a small ring buffer (e.g. last 120–240
  frame-times) into a **lightweight custom `ui.painter()` sparkline / line plot** drawn inside the perf
  panel (a `Vec<Pos2>` polyline over a bounded rect, with a target-frame-time reference line at
  16.64 ms). PREFER the hand-rolled painter approach to avoid adding the `egui_plot` dependency; only add
  `egui_plot` if a hand-rolled plot is genuinely unworkable (and if so, note the dep cost).
- Label the axes/scale minimally (max ms, a 16.64 ms NTSC deadline line). Keep it in the existing perf
  panel (no new always-on overlay required for v1; a panel graph is enough). Update both the
  "produced" and "presented" series if cheap, or at least presented (where visible judder lives).
- **Complexity:** medium (the data exists; the work is the painter widget). Frontend-only, no core touch.

(Originally these three — I/J/K — were deferred; the user asked to include the full survey set, so they
are now in scope. They may be implemented as a SECOND pass on top of features A–H.)

---

## Mandatory gates (run ALL; all must pass)

1. `cargo fmt --all`
2. `cargo clippy --workspace --all-targets -- -D warnings`
3. `cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --lib --bins -- -D warnings`
4. `cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown --no-default-features --features wasm-canvas --lib --bins -- -D warnings`
5. `cargo clippy -p rustynes-frontend --no-default-features -- -D warnings`
6. `cargo clippy -p rustynes-frontend --features retroachievements --all-targets -- -D warnings`
7. `RUSTDOCFLAGS="-D warnings" cargo doc -p rustynes-frontend --no-deps`
8. `cargo test -p rustynes-frontend`
9. **Determinism re-verify (the release gate):** `cargo test --workspace --features test-roms accuracycoin`
   (must stay 100% / 139) AND `cargo test -p rustynes-test-harness --features test-roms,commercial-roms --test external_real_games --test external_extended` (must stay 60/60 + 52/52). These MUST be unchanged — if
   any byte-identity test regresses, you touched the deterministic path; fix it (the speed/volume work
   must not alter the core's per-frame output).

WGSL strings in `gfx.rs` are double-quoted Rust strings — NO `"` inside any `//` shader comment.
clippy is pedantic+nursery: empty no-op fns need `const fn` + `&self`; `#[cfg]` dispatch bodies but
keep `MenuAction`/`SysAction` variants un-gated (exhaustive matches). Add a unit test where it makes
sense (e.g. speed→effective-frame-duration math, resampler base_ratio clamp band, volume gain applied,
thumbnail-texture cache invalidation). Read every file before editing it.

## Report

Per feature (A–H): DONE/PARTIAL + how you wired it on each relevant produce path + the exact UI/keybind
added. The exact pass/fail of all 9 gates with final output lines (call out gate 9 — the determinism
re-verify — explicitly). Any new keybinds chosen (and proof they were unbound). Flag everything as
compile/clippy/test-verified only (headless) for a runtime smoke-test, especially the alt-speed audio
pitch-shift, the volume live-apply, and the thumbnail texture caching.
