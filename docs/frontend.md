# Frontend

**References:** Phase 1 frontend stack decision (`winit + wgpu + cpal + egui`); `ref-docs/research-report.md` §Frontend (Rust ecosystem).

## Purpose

Implement the user-facing application in `crates/rustynes-frontend`: the windowing, rendering, audio, input, and debugger UI. The binary is named `rustynes`.

## Stack

| Concern | Crate | Notes |
|---------|-------|-------|
| Window + event loop | [winit](https://github.com/rust-windowing/winit) | Linux (X11/Wayland), macOS (AppKit), Windows (Win32), Web (`wasm-winit` / `wasm-canvas`) |
| GPU rendering | [wgpu](https://github.com/gfx-rs/wgpu) | WebGPU API; Vulkan/Metal/D3D12/GLES backends |
| Audio output | [cpal](https://github.com/RustAudio/cpal) | Cross-platform PCM stream |
| GUI shell + overlays | [egui](https://github.com/emilk/egui) 0.29 + `egui-wgpu` | The always-on desktop shell (menu bar / status bar / settings) **and** the toggleable debugger panels |
| Gamepads | [gilrs](https://gitlab.com/gilrs-project/gilrs) | XInput, evdev, GameController.framework (native) |
| File dialogs | [rfd](https://github.com/PolyMeilex/rfd) | Native open/save dialogs |
| Config paths | [directories](https://github.com/dirs-dev/directories-rs) | XDG / Apple / Windows |

The frontend is a polished desktop application, not a bare emulator window:
egui runs **every frame** to draw an always-on menu bar + status bar +
tabbed settings window over the NES image, with the deep CPU/PPU/APU/memory
debugger panels layered on top only when toggled. The threading model
(below) keeps the emulator off the winit event-loop thread so UI, file I/O,
and GPU submit never disturb emulation cadence.

## Threading model (native, default `emu-thread`)

On native builds with the default-ON `emu-thread` feature, single-player
frame production runs on a **dedicated emulation thread** (`emu_thread.rs`),
not the winit thread:

- `App.emu` is a shared `Arc<Mutex<EmuCore>>` (`crate::emu::EmuHandle`). The
  emu thread owns the pacer, run-ahead, and the `Send` audio producer; it
  holds the lock only for the brief latch+produce region of each frame, then
  pings the winit loop with `AppEvent::EmuFrame`.
- The winit thread services window events, builds + submits egui, and
  presents — it takes the emu lock only briefly (input commands, the
  framebuffer copy for the debugger-hidden present path, the per-frame RA
  drive on the RA build). Neither thread blocks the other on I/O or present.
- It reads inputs from a lock-free `SharedInput` (published by the winit
  thread on every input event + gamepad pump) so the late input latch
  survives the thread split.
- **Netplay runs synchronously on the winit thread** (it owns the
  `UdpSocket`); while a session is active the emu thread is paused so the two
  never both drive the core. **RetroAchievements stays on the winit thread**
  (`rc_client` is single-threaded C).
- Best-effort Linux priority elevation runs on the emu thread (SCHED_RR →
  `nice` → `PR_SET_TIMERSLACK`, degrading silently without the `realtime`
  rlimit).

`cargo run --no-default-features` keeps the synchronous path (the emulator
runs on the winit thread) for A/B comparison. The browser builds
(`wasm-winit` / `wasm-canvas`) always run on the single main thread, driven
by `requestAnimationFrame`.

## Run loop

`App` implements winit's `ApplicationHandler<AppEvent>`. Frame *production*
and frame *presentation* are decoupled: the emulation thread produces frames
on its own pacer (display-sync / vrr / wallclock — see below) and pings the
winit loop with `AppEvent::EmuFrame`; `RedrawRequested` only **presents** the
latest produced framebuffer plus the egui shell. The display can therefore
re-present the same frame (compositor throttling) without stalling
emulation, and emulation can run ahead without waiting on vsync.

```
fn main() {
    let event_loop = EventLoop::with_user_event().build()?;
    event_loop.run_app(App::new())?;        // window, wgpu surface, cpal stream,
                                            // egui ctx, and (native) the emu thread
}

impl ApplicationHandler<AppEvent> for App {
    fn user_event(&mut self, _, AppEvent::EmuFrame) {
        // the emu thread finished a frame: housekeeping (perf/HUD pushes,
        // FDS flush, perf logging) then request_redraw().
    }
    fn window_event(&mut self, event_loop, _, event) {
        match event {
            WindowEvent::RedrawRequested => {
                self.upload_framebuffer();   // texture copy of the presented frame
                self.render_egui_shell();    // menu bar + status bar + (opt) debugger
                self.surface.present();
            }
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event),
            ...
        }
    }
}
```

(On wasm the produce step happens inside `RedrawRequested` itself —
`pace_and_produce_wasm` — since the only redraw hook the web backend exposes
is `request_redraw()` → `RedrawRequested` on rAF.)

## Audio architecture (the performance-pass audio engine)

- cpal opens an output stream at the configured `[audio] sample_rate` when
  the device supports it (falling back to the device default).
- The audio callback runs on cpal's audio thread and is **allocation-free**
  (closure-owned reusable mono scratch).
- A hand-rolled **lock-free SPSC ring** (`audio.rs::SampleQueue` —
  power-of-two capacity, atomic f32-bit slots, acquire/release head/tail)
  sits between the run-loop thread (producer) and the audio thread
  (consumer). No external dep; single-producer/single-consumer by
  convention.
- The run loop drains the APU after each frame and pushes through
  [`AudioOutput::push_samples`]: a 4-tap Hermite (Catmull-Rom) resampler
  (`resampler.rs`) whose ratio is nudged up to ±0.5% by queue occupancy —
  **dynamic rate control** (Near's law: `ratio = (1-δ) + 2·fill·δ`,
  δ = 0.005, equilibrium at the `[audio] latency_ms` target, default
  60 ms). `[audio] drc = false` bypasses to a bit-exact push.
- **Start-gating + hard resync**: the callback plays silence (without
  consuming) until the queue holds the latency target, then starts; a true
  underrun re-gates until refilled (one clean gap, not a crackle spiral).
  If occupancy overshoots the target by >50 ms after a produce stall, the
  producer skips batches until it returns (counted as overrun-dropped).
- Underrun / overrun counters + occupancy are exposed in the debugger
  Performance panel (Phase 0).

The Performance panel also has a **Logging** checkbox (session-only,
default OFF, native-only): while set, the app appends one CSV row per
second of everything the panel shows (produced / presented / produce-cost
interval stats, pacer anomaly counters, audio health, GPU pass time, the
active pacing regime + present mode) to
`perf-logs/perf-<rom>-<utc>.csv` under the working directory, with the
game identity (label + SHA-256) and the full run configuration (pacing
mode, present mode, audio latency/DRC, run-ahead, rewind, monitor
refresh, build) as `#`-commented header lines. Loading a different ROM
while logging rotates to a fresh file. `perf-logs/` is gitignored — the
files are offline performance-analysis artifacts (`perf_log.rs`).

Sample rate matching: the APU is configured at startup with the stream's
actual sample rate and emits directly at that rate via blip_buf-style
band-limited synthesis. The Hermite stage only absorbs the residual
host-clock vs DAC-clock drift — the core's emitted samples are part of the
determinism contract and never depend on wall-clock feedback.

### Browser audio (AudioWorklet)

The wasm builds (`wasm_audio.rs`) output through an **AudioWorklet** whose
`process()` callback runs on the browser's dedicated audio rendering
thread — decoupling audio from the single wasm main thread exactly as the
native lock-free ring + cpal callback decouples it on desktop (it replaces
the deprecated `ScriptProcessorNode`, which kept its callback on the main
thread and contended with the emulator + rAF loop). The worklet processor
is a small JS module embedded as a string and loaded via a `Blob:` URL (no
separate asset to ship, no GitHub-Pages `--public-url` path concern).
GitHub Pages can't set the COOP/COEP headers `SharedArrayBuffer` needs, so
the main thread `postMessage`s each frame's samples to the worklet and the
worklet `postMessage`s its ring occupancy back — and that occupancy drives
the **same Hermite DRC law as native** (`resampler.rs` is target-agnostic),
holding the worklet ring at an 80 ms target. `ScriptProcessorNode` remains
an automatic fallback when the browser lacks `AudioWorklet`. The worklet
ring health (occupancy / underruns / overruns) is surfaced in the wasm
Performance panel.

### Browser pacing (rAF display-sync)

On wasm the frame loop is driven by `requestAnimationFrame` (winit's web
backend delivers `RedrawRequested` on rAF). When the measured rAF cadence
matches the console rate within 3% (a ~60 Hz panel showing 60.0988 Hz
content), the pacer engages **display-sync**: exactly one emulated frame
per rAF, with the audio DRC absorbing the sub-percent rate difference —
eliminating the wall-clock-vs-rAF beat that otherwise dups/drops a frame
every ~9 s. On 120/144 Hz panels (cadence far from the console rate) it
keeps the wall-clock-delta catch-up, which is correct there. The Perf
panel's pacing field reads `raf-display` or `raf` accordingly.

## Rendering

- The PPU emits a `[u8; 256*240*4]` RGBA8 sRGB framebuffer.
- Each frame, the frontend uploads it to a wgpu texture (256×240, `Rgba8UnormSrgb`).
- A full-screen triangle pass samples the texture with nearest filter, scaled
  to the window through an aspect-ratio-correct **letterbox** transform
  (`gfx.rs::letterbox`).
- Optional NTSC filter / CRT shader runs as a second pass.

**Pixel aspect ratio.** When `[ui] pixel_aspect_correction` is on, the
letterbox targets the NES's native **8:7** PAR (display aspect
`(256 · 8/7) / 240`); off, it keeps the square-pixel 256:240 aspect. The
toggle (View menu / Settings → Video) rewrites the letterbox uniform live via
`Gfx::set_pixel_aspect`.

**Lock discipline at present (`emu-thread`).** The egui shell runs every
frame but **never holds the emu lock inside the egui closure**. Two render
branches in `RedrawRequested`:

- The common **hidden** branch (debugger off, no `nes`-reading tool panel
  open) copies the presented framebuffer into `App.present_staging` under a
  *brief* lock, **drops the lock**, then renders the shell with `nes = None`
  and presents with the lock released (so Fifo vsync at present can't block
  the emu thread).
- The **locked** branch is taken when the debugger overlay is visible OR a
  tool panel that reads `&mut Nes` is open (today: Cheats) — `needs_nes`. It
  holds the lock across the egui pass so the chip panels can inspect the live
  core.

A snapshot of core facts the status bar / menu IA need (ROM loaded, FPS, FDS
disk-side count, Vs.-System flag, mapper + region labels, movie state) is
captured under that same brief lock *before* the egui pass and passed in as a
`ShellFrame`, so the build closure never re-locks.

Vsync / pacing (the display-sync matrix, `[graphics]
pacing_mode`, default `auto`):

| Regime | Clock master | Present mode | When |
|---|---|---|---|
| `display` | Fifo vsync (1 emulated frame per refresh; ≤0.5% speed bend, audio DRC absorbs) | `Fifo` | refresh within 0.5% of the console rate (`auto` engages it) |
| `vrr` | wall clock at the exact console rate; the VRR display follows | `Fifo` | user-asserted G-Sync/FreeSync (best fullscreen) |
| `wallclock` | wall clock (sleep-then-spin pacer) | configured (`Mailbox` default) | high-refresh fixed panels / fallback |

Display-sync has an occlusion watchdog (emulation+audio keep running when
the compositor throttles redraws) and a sustained-miss fallback to
`wallclock` (sticky per session, reported in the Performance panel).
`[graphics] max_frame_latency` (1|2) sets the swapchain depth. Input is
latched immediately before `run_frame` in every regime (late latch).

## Run-ahead (`[input] run_ahead`, default 1, native)

Removes the game's OWN internal input lag (most NES titles buffer input
≥ 1 frame): each visible frame the emulator runs one persistent frame with
the freshly latched input, saves state (`Nes::snapshot_core_into`, ~15 µs),
runs N−1 hidden + 1 visible frame, presents the visible (future) frame's
video+audio, and rolls back (`Nes::restore_quiet`). The persistent timeline
is byte-identical to a plain run (unit-proven in
`crates/rustynes-frontend/src/runahead.rs`), so save-states, rewind, movies and
RA process the real timeline. Auto-disabled during netplay + movie
record/playback; budget-throttled (hysteresis on produce-cost p95) on hosts
that can't afford the extra frames. Cost: N extra `run_frame`s + ~140 µs of
state churn per visible frame (`docs/benchmarks.md` §8).

## Desktop UX shell (always-on egui)

The production desktop experience is an **always-on egui shell** that frames
the NES image independently of the (separately toggled) debugger overlay.
egui runs every frame — earlier engine lines only ran egui when the debugger
was visible.

A single `DebuggerOverlay::render_shell` (`debugger/mod.rs`) runs one
`ctx.run` per frame that, in order:

1. applies the configured theme (only on change — `ui_shell::apply_theme`);
2. builds the shell UI — menu bar + status bar + tabbed settings window +
   welcome / about / shortcuts windows + the paused overlay — via
   `ui_shell.rs`;
3. draws the wasm-netplay lobby (`extra_ui`, a no-op on native);
4. renders the tool panels (Cheats / Settings / Netplay / Cheevos / Perf /
   Input) whenever their `show_*` flag is set, and the chip panels +
   debugger toolbar HUD **only** when the overlay is visible.

**The `MenuAction` / dispatch idiom.** The shell never mutates `App` from
inside the egui closure (the closure already mutably borrows `config`, and
re-locking the emu there would deadlock). A menu/window interaction that
needs `&mut App` is returned as a `MenuAction` in the `ShellOutput`, which
`App::dispatch_menu_action` acts on **after** the egui pass. Read-only
toggles that the menu mirrors but does not own (Fullscreen, Show Menu Bar,
Show Debugger) use a "read-only mirror" pattern — the menu shows the current
state and emits the toggle action; the app flips the real flag.

### `ui_shell` module

`ui_shell.rs` owns the shell types:

- `UiShell` — menu/window visibility, the active `SettingsTab`, the transient
  status toast, and mirrors of the pause / fullscreen / active-save-slot
  flags.
- `MenuAction` — the deferred-action enum (Open ROM, Load/Save slot, Pause,
  Reset, Power Cycle, Toggle Debugger/Fullscreen/Menu Bar, Cycle Disk Side,
  Screenshot, Insert Coin, movie record/play/branch, `OpenPanel(ToolPanel)`,
  `OpenChipPanel(ChipPanel)`, ...).
- `ShellFrame` — the per-frame read-only context captured under the brief
  lock (see Rendering).
- `SettingsTab` (Video / Audio / Input / Advanced), `StatusMessage` (a
  colored, auto-fading status-bar toast), and `apply_theme`.

### Menu IA

The menu bar is **File / Emulation / Tools / View / Debug / Help**:

- **File** — Open ROM (`F12`, native), Open Recent (MRU, missing files greyed
  out), FDS Swap Disk Side (`F9`, FDS games only), Save/Load State, Save Slot
  (0-7 radio), Save-to-Slot / Load-from-Slot, Take Screenshot (native), Quit.
- **Emulation** — Pause/Resume (disabled during netplay), Reset, Power Cycle,
  Frame Advance (`\`, single-steps one frame while paused), a hold-`Tab` Fast
  Forward hint, Run-Ahead selector (0-3), Region (read-only display), Vs. Insert
  Coin (`F10`, Vs. games only).
- **Tools** — Cheats, Movies (TAS: Record/Play/Branch), Netplay (native),
  RetroAchievements (native + feature), Performance Monitor.
- **View** — Settings, Theme (Light/Dark/System), 8:7 Pixel Aspect,
  Fullscreen (`F11`, native), Window Size (1x-4x of the NES resolution, native),
  Show FPS, Pause When Unfocused (auto-pause on focus loss), Show Menu Bar (`M`).
- **Debug** — Show Debugger (`` ` ``), then CPU / PPU / APU / Memory / OAM /
  Mapper.
- **Help** — Keyboard Shortcuts, About.

Tools surfaced this way appear as **floating windows without** opening the
`` ` `` debugger overlay. Menu items carry their accelerator hint from the
live `[input.system]` binding.

### Chip panels vs tool panels

The panels split by what they need:

- **Tool panels** (`ToolPanel`: Cheats, Settings, Netplay, Cheevos, Perf,
  Input) render whenever open, with the deep overlay off. `OpenPanel` sets the
  flag without forcing the overlay visible. (Cheats reads `&mut Nes`, so the
  render path takes the locked branch — `any_nes_tool_open` — when it is
  open.)
- **Chip panels** (`ChipPanel`: Cpu, Ppu, Oam, Apu, Memory, Mapper) need
  `&mut Nes` and a per-frame core poll, so they render only while the overlay
  is visible. `OpenChipPanel` therefore forces the overlay visible.

### Pause and fullscreen

Pausing parks the emu thread (no `EmuFrame` pings), so the shell keeps
repainting on input via `egui_wants_repaint` and draws a translucent
centred **PAUSED** overlay. Fullscreen is a native borderless window mode
(`F11`); the menu item is a read-only mirror that emits `ToggleFullscreen`.
Optional **pause-on-focus-loss** (`[ui] pause_on_focus_loss`, default off)
auto-pauses on a `WindowEvent::Focused(false)` and auto-resumes on regaining
focus, leaving a manual pause and netplay sessions untouched.

### Fast-forward and frame-advance

Two playback quality-of-life keys ride alongside rewind: **fast-forward**
(hold `Tab`) runs the emulator unthrottled in a bounded back-to-back burst
with audio muted (a `None` audio sink) so the producer never overruns the
ring; **frame-advance** (press `\`) single-steps exactly one frame, meant for
use while paused. Both are default-bound in `[input.system]` and rebindable.

## Input

Keyboard mapping (default — every binding is rebindable in `config.toml`
or via the in-app rebind modal in the debugger overlay):

| NES button | Player 1 key | Player 2 key |
|------------|--------------|--------------|
| A | `Z` | `Q` |
| B | `X` | `E` |
| Select | `RShift` | `L` |
| Start | `Enter` | `P` |
| Up | `ArrowUp` | `W` |
| Down | `ArrowDown` | `S` |
| Left | `ArrowLeft` | `A` |
| Right | `ArrowRight` | `D` |

Gamepads: `gilrs` maps standard button layouts. South = A, East = B,
Back = Select, Start = Start, dpad = dpad. Routed to Player 1 alongside
the keyboard.

System hotkeys (rebindable via `[input.system]`):

- `F1` — save state to the current slot
- `F4` — load state from the current slot
- `F5` — hold to rewind
- `F2` — soft reset
- `F3` — power cycle
- `F6` / `F7` / `F8` — TAS movie record / play / branch
- `F9` — swap FDS disk side (FDS games)
- `F10` — insert a Vs. System coin (Vs. games)
- `F11` — toggle borderless fullscreen (native)
- `F12` — open a different ROM via the system file dialog
- `M` — toggle the menu bar
- `Esc` — exit fullscreen if fullscreen, otherwise quit
- `` ` `` (backtick) — toggle the egui debugger overlay

ROMs can also be loaded by **dragging a `.nes` file onto the window**.

**Input gating.** Emulator key/mouse input is suppressed when egui consumed
the event, when a settings text field is focused (`wants_egui_input`), or when
a menu / popup / modal window is open (`shell_is_capturing` plus the
shell-window flags) — so clicking a menu or typing in a field does not also
drive the NES controller. The late latch publishes the held input into the
lock-free `SharedInput` for the emu thread to read immediately before
`run_frame`.

## Debugger panels (egui)

Toggled with `` ` ``; opening just shows the debugger toolbar HUD (frame /
cycle / FPS / movie / disk / netplay / RA status) with the chip sub-windows
closed — the user opens the panels they want from the toolbar checkboxes or
the Debug menu. The panels are read-only: they never advance
emulator-visible state, polling the inspection API on `rustynes_core::Nes`
once per visible frame.

- **CPU**: registers, current instruction, disassembly window (scrollable), breakpoints, step-instruction button.
- **PPU**: nametable viewer (4 tables side-by-side, scroll-cursor overlaid), pattern table viewer (both tables, with palette selector), OAM viewer (sprite list + visual), palette RAM viewer.
- **APU**: per-channel scope (waveform), volume meters, register dump.
- **Memory**: hex viewer of CPU bus + PPU bus, with go-to-address (disabled in RetroAchievements hardcore mode).
- **Mapper**: bank registers, IRQ counter state.

All panels are floating windows in egui's window system. The tool panels
(Cheats / Settings / Netplay / Cheevos / Perf / Input) are the same windows
the menu bar surfaces directly (see "Chip panels vs tool panels" above).

## Settings

Stored in `directories::ProjectDirs::config_dir() / "RustyNES" / "config.toml"`.
Includes: input bindings, audio device + latency + DRC, video filter
selection, pacing mode, rewind buffer size, run-ahead depth, default region
for region-less ROMs.

The settings window is the same tabbed UI the menu's View → Settings opens —
Video / Audio / Input / Advanced — each tab routing to the matching
debugger-settings / input-rebind section so the live-apply plumbing is
shared (a present-mode / NTSC-filter / rewind change applies immediately).

v1.0.0 added a `[ui]` section and a few top-level keys:

- `[ui] theme` — `light` / `dark` / `system` (egui visuals; `system` follows
  the windowing system's reported preference).
- `[ui] pixel_aspect_correction` — the NES 8:7 PAR toggle (default off).
- `[ui] show_fps` — show the FPS readout in the status bar.
- `[recent_roms] paths` — the File → Open Recent MRU list.
- `welcome_shown` — set the first time the welcome modal displays (so it never
  re-nags).
- `[input.system] fullscreen` (default `F11`) and `toggle_menu_bar` (default
  `M`).

## Save state files

- File extension: `.rns` (RustyNES State).
- Stored in `directories::ProjectDirs::data_dir() / "RustyNES" / "saves" / "<rom-sha256>" / "slot-N.rns"`.
- Format: tagged sections per chip with version header. See the module-level rustdoc of [`crates/rustynes-core/src/save_state.rs`](../crates/rustynes-core/src/save_state.rs) for the on-wire layout (`HEADER` magic + format version + truncated ROM SHA-256 tag, followed by `BUS / CPU / PPU / APU / MAP` sections in any order with per-section version bytes). The CHANGELOG `[Unreleased]` entries also document per-chip section version bumps as they happen (e.g., MMC5 v2→v3 when vertical split-screen landed).

## ROM file handling

- Drag-and-drop a `.nes` file → load it.
- File menu → Open → native dialog.
- Recent files list (last 10).
- ROMs are *not* copied; the frontend stores absolute paths. (Save states are keyed by SHA-256 of the ROM, so moving the ROM doesn't break the save.)

## Shipped / open

All of the original open questions are resolved in v1.0.0:

- **WebAssembly target** — shipped (`wasm-winit` full winit+wgpu+egui build +
  `wasm-canvas` lightweight embed). The Web Audio user-gesture requirement is
  handled by arming `AudioContext` at the file-picker gesture.
- **CRT / NTSC shader** — a Blargg-NTSC-style filter ships as a wgpu post-pass
  (toggleable in Settings). Slang-shader ports remain a future enhancement.
- **Movie recording (TAS)** — shipped (`.rnm` record/play/branch).
- **Netplay** — shipped (rollback netcode, 2-4 players, native UDP + browser
  WebRTC), enabled by the deterministic core.

Future work tracked in `to-dos/ROADMAP.md`: mobile (iOS/Android) frontends and
additional CRT/slang-shader ports.
