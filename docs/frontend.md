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

## Command-line interface (native, v1.4.0 Workstream H)

The native binary uses a [clap 4](https://github.com/clap-rs/clap) `Command`
(`src/cli.rs`) rather than a hand-rolled argv loop. It preserves the historical
contract: `rustynes <ROM>` loads and runs a ROM; a missing ROM file exits 1; a
bad argument exits 2 (clap's usage-error code). clap auto-provides
`-h`/`--help`/`-V`/`--version`, styles `--help` with ANSI accents
(`Command::styles`), honours `NO_COLOR` / `--color <auto|always|never>`, and
appends a colored "Examples" + "Keyboard" footer (`color-print`'s `cstr!`).

| Invocation | Behavior |
|------------|----------|
| `rustynes <ROM>` | Load and run the ROM. |
| `rustynes` (no args) | Prints help and exits 2 (the native binary has no bare-launch path; load further ROMs from the menu / F12 / drag-and-drop once a session is open). |
| `rustynes --help` / `-V` | clap-styled help / version (exit 0). |
| `rustynes help [<topic>]` | Topic help (see below). |
| `rustynes completions <bash\|zsh\|fish\|powershell>` | Print a shell-completion script (`clap_complete`). |

Help topics (`controls`, `hotkeys`, `gamepad`, `features`, `mappers`, `config`,
`scripting`, `netplay`, `about`) come from a single structured registry
(`cli::HELP_TOPICS`) kept in sync with this doc, the README, and the in-app
"Keyboard Shortcuts" window — so the CLI help and the docs can't drift. A
registry-completeness test guards every topic.

`rustynes help` with no topic on a TTY (or `rustynes help --interactive`)
launches a full-screen [ratatui](https://github.com/ratatui/ratatui) +
crossterm browser (`src/help_tui.rs`): a left topic list, a scrollable colored
content pane, `/` search, and arrow/Tab/PgUp-Dn/Home-End nav (`q`/`Esc` quit).
It is behind the **default-on `help-tui`** cargo feature (a minimal build can
drop it with `--no-default-features`). When stdout is **not** a terminal (piped
output / CI), or the feature is off, it falls back to the static styled topic
page, so `rustynes help mappers | less` never blocks on a TUI.

All of this is **native-only**: the wasm `main` is an empty shim, and the clap /
clap_complete / color-print / anstyle / ratatui dependency cluster lives in the
`[target.'cfg(not(target_arch = "wasm32"))'.dependencies]` table, so the wasm
build and its size budget are unaffected. There is **zero determinism surface**
— everything runs before any emulation begins.

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

```rust
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

**Presented-cadence measurement (v1.3.0 Workstream B1).** The "presented"
interval is timestamped at the `RedrawRequested` (display-refresh) signal — the
instant the compositor asks for a frame — and **not** after `surface.present()`
returns. Timestamping after present folded GPU-submit + vsync-gate +
coalesced-`RedrawRequested` jitter into the series, which made the panel's
"presented" graph bottom out and rush to catch up even while "produced"
(emulation) stayed flat. Measured at the refresh signal, present-to-present
deltas reflect the display's true visible cadence; a small, steady offset from
"produced" is the NTSC 60.0988 Hz emulation rate beating against the display
refresh, not stutter. (The timestamp is still recorded only on an actual present,
so a skipped / early-returned redraw is not counted.) A true scan-out timestamp
(a GPU present-timing query) is a possible future refinement — see
`docs/performance.md`.

The panel also surfaces two **present/produce "beat" counters** (`presented_dups`
/ `produced_dropped`, also logged to the perf CSV): a present with no new produced
frame is a duplicate (the display repeated a frame); >1 produce between presents
means the extra frames were dropped (unshown). Under display-sync both stay ~0;
under wall-clock pacing they tick roughly once every ~10 s for the 60.0988-vs-60.000
Hz beat. They are read-only diagnostics — the deeper pacer mitigation that would
*reduce* the beat is deferred (it needs on-device validation across real refresh
rates and carries pacing-regression risk); these counters are the signal for whether
that work is warranted.

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

### Audio settings (Settings → Audio)

The Audio tab exposes the output-mix controls, all of which default to a
byte-identical no-op so the shipped sound is unchanged until the user touches
them:

- **Master volume / Mute** (`[audio] volume` / `muted`) — a single gain applied
  at the cpal consume point (lock-free, post-resampler). Default `1.0` / un-muted.
- **Per-channel mute** (`[audio] channel_mask`) — six checkboxes (pulse 1 / pulse
  2 / triangle / noise / DMC / Mapper Audio). A cleared bit forces that channel's
  contribution to `0` before the non-linear mixer. Default `0x3F` (all on).
- **Per-channel volume** (`[audio] channel_gain`, v1.4.0 Workstream C) — a slider
  (`0.0`–`2.0`) per channel, generalizing the mute mask (`0.0` = muted, `1.0` =
  full). The five internal APU channels are always shown; the **expansion-audio**
  channel (index 5) appears only when the loaded mapper has on-cart audio, labelled
  with the chip — discovered dynamically via `Nes::expansion_audio_chip()` (which
  consults the cached `MapperCaps::audio` flag + mapper id: VRC6, VRC7 (OPLL),
  MMC5, Namco 163, Sunsoft 5B, FDS). The gains are pushed into the core under the
  emu lock (`Nes::set_apu_channel_gain`) and re-pushed on each ROM load / power
  cycle (a fresh `Nes` boots at unity). In the core APU mixer the gain scales each
  channel's raw integer output and rounds back to an integer before the non-linear
  lookup mixer (so it is a clean generalization of the integer mute gate); a "Reset
  volumes (1.0)" button restores unity. Default (all `1.0`) takes the exact
  pre-gain mixer path — byte-identical, so the determinism contract + the audio
  oracle are unaffected. (Per-channel gain must touch the core because the
  non-linear mixer combines the channels before output — there is no per-channel
  split downstream — but the unity fast-path guarantees the default is provably
  unchanged.)
- **Graphic EQ** (`[audio] eq_enabled` / `eq_bands`) — a default-off five-band
  (60 / 240 / 1k / 3.8k / 12k Hz, ±12 dB) frontend output stage owned by the
  producer (`audio.rs::EqStage`), bypassed (zero overhead) when off / flat.

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

**Custom palette (`.pal`).** A user-supplied `.pal` file re-tints the display
through the PPU's colour LUT. Settings → Display offers a **Load .pal… /
Built-in** picker (native); the chosen file is remembered in `[graphics]
palette_file` and re-applied on every ROM load. A 64-entry palette is the
192-byte form (longer 512-entry files use the first 64 colours); the standard
2C02 composite emphasis is applied to the custom base table by
`rustynes-ppu::build_rgba_lut_from_base` / `Ppu::set_custom_palette`, routed
through `Nes::set_custom_palette`. The default (none) is byte-identical to the
built-in palette. Native-only file I/O (a no-op on wasm).

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
- `SettingsTab` (Video / Shaders / Audio / Input / Emulation; v1.3.0 split the
  shader stack into its own tab and renamed "Advanced" → "Emulation"),
  `StatusMessage` (a colored, auto-fading status-bar toast), and `apply_theme`.

### Menu IA

The menu bar is **File / Emulation / View / Tools / Debug / Help** (v1.3.0
reorganized the order and regrouped several items — see the per-menu notes):

- **File** — Open ROM (`F12`, native), Open Recent (MRU, missing files greyed
  out), Close ROM (v1.3.0), a **Save States** submenu (Save/Load State, Active
  Slot 0-7 radio, Save-to-Slot / Load-from-Slot, Manage States…), Take
  Screenshot + Copy to Clipboard (native), Quit.
- **Emulation** — Pause/Resume (disabled during netplay), Reset, Power Cycle,
  Frame Advance (`\`, single-steps one frame while paused), a hold-`Tab` Fast
  Forward hint, Run-Ahead selector (0-3), Speed presets, Region (read-only
  display), Vs. Insert Coin (`F10`, Vs. games only), and FDS Swap Disk Side
  (`F9`, FDS games only; moved here from File in v1.3.0).
- **View** — Settings, Theme (Light/Dark/System), 8:7 Pixel Aspect, Hide
  Overscan, Fullscreen (`F11`, native), Window Size (1x-4x of the NES
  resolution, native), Show FPS, Pause When Unfocused (auto-pause on focus
  loss), Show Menu Bar (`M`).
- **Tools** — Cheats, Movies (TAS: Record/Play/Branch), Netplay (native),
  RetroAchievements (native + feature), Input Display, Input Miniatures, NSF
  Player (moved here from Debug in v1.3.0), Replay / TAS (v1.5.0 C2), ROM
  Database, and an **HD Pack** submenu (`hd-pack` feature + native; folded in
  from the former standalone "Mod" menu).
- **Debug** — Show Debugger (`` ` ``), Performance Monitor (moved here from
  Tools in v1.3.0), then the chip/state inspectors: CPU / PPU / APU / Memory /
  Memory Compare / OAM / Mapper / Trace Logger / Event Viewer / Lua Script.
- **Help** — Documentation (v1.5.0 I10; native, searchable in-app manual),
  Keyboard Shortcuts, About.

**v1.5.0 "Lens" Workstream I — native-UI fixes + menu overhaul.** Frontend-only,
determinism-neutral. Headline items: the **Fast Forward** (`Tab`) and **Frame
Advance** (`\`) global hotkeys now fire even when egui claims the key for menu /
widget focus navigation — the keyboard gate routes *system hotkeys only* through
`InputState::handle_system_key` (never the NES controller) on the egui-busy path,
while a genuinely focused text field still blocks everything (I2); the Emulation
menu enables Frame Advance only while paused and shows a live Fast-Forward state
instead of a permanently-greyed hint. **Copy Screenshot to Clipboard** holds a
persistent `arboard` handle so the image survives on X11 / Wayland (the clipboard
is owned by the live process), fixing the silent no-op + false success toast
(I1). **Tools -> ROM Database** opens standalone now that the locked-render
predicate (`DebuggerOverlay::any_nes_tool_open`) lists every `nes`-reading tool
panel — Cheats *and* the ROM Database editor (I6). The **RetroAchievements**
readout moved into the bottom status bar between the emulator-state label and the
FPS counter (I7). The **Keyboard Shortcuts** window reads the live `[input]` /
`[input.system]` bindings with a Player/device selector (I9). The **Input
Display** uses a per-group palette (D-pad green / Select-Start yellow / B-A
Nintendo red), mirrored in the Input Miniatures overlay (I5). Settings: the
Shaders "Shader stack" header defaults open, the Input tab auto-saves every
control in both the Settings window and the standalone window, and the old "Save
to disk" button is now "Export config..." (I3).

Tools surfaced this way appear as **floating windows without** opening the
`` ` `` debugger overlay. Menu items carry their accelerator hint from the
live `[input.system]` binding.

Each top-level menu and its items are prefixed with a **Font Awesome 6 Free
Solid** glyph (v1.2.0 Workstream H3; the `GeraNES` `withMenuIcon` model). The
icon font (`crates/rustynes-frontend/assets/fonts/fa-solid-900.ttf`, SIL
OFL-1.1) is embedded with `include_bytes!` and registered once in
`DebuggerOverlay::new` via `crate::icons::install` as a trailing egui fallback
family, so ordinary text is unaffected and a missing glyph degrades to a box.
The glyph codepoint constants live in `crate::icons::glyph`. The full font fits
the 5 MiB gzip wasm budget, so the **same full font ships on native and both
wasm flavours** — there is no per-target subset (the `wasm-canvas` embed has no
egui menu and is unaffected).

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

**Turbo / autofire** (`[input] turbo_a` / `turbo_b`, Settings → Input). The A
and/or B button can rapid-fire while held; `[input] turbo_period` sets the
frames per on/off half-cycle. Off by default (empty mask = byte-identical
input). The strobe is keyed on the emulated frame number (`Nes::frame()`) and
applied where input meets the NES — in `EmuCore::latch` and on the local input
in both netplay paths — so the gated bits are what get latched / recorded /
sent: deterministic and rollback / TAS / netplay-safe. The native and
`wasm-winit` paths apply it; the lightweight `wasm-canvas` embed does not.

**NES Power Pad.** The Power Pad / Family Fun Fitness mat is selectable as the
player-2 expansion device (Settings → Input "Port 2 device"). Its 12 mat
buttons default to a left-hand grid (`1`–`4` / `Q W E R` / `A S D F`, chosen to
avoid the P1 and system-speed keys) and implement the dual-8-bit-shift-register
serial protocol on `$4017`. Off by default (standard controller), so the
default + Four Score paths stay byte-identical. Native (keyboard mat keys) +
both wasm frontends (v1.2.0 Workstream F2: the touch-overlay Power Pad mat,
fed through the same late-latch — see "Touch controls" below). Rebindable mat
keys are a follow-up.

**Touch controls (wasm)** (v1.2.0 Workstream F1 + F2). The browser build adds a
translucent Pointer-Events touch overlay (pure DOM/CSS in `web/index.html`, with
the `wasm_touch.rs` bridge) — an on-screen D-pad / A / B / Start / Select, a
selectable target port (player 1–4, for Four Score), and a 12-button Power Pad
mat. The overlay translates `pointerdown`/`pointerup`/`pointercancel` into a
`Buttons` mask (and a Power Pad mat mask) and pushes them through the
`rustynes_touch_*` bindings into a thread-local that BOTH wasm frontends read at
the SAME deterministic late-latch a keypress uses: the `wasm-canvas` embed folds
it into its per-frame `set_buttons` / `set_power_pad` call, and the `wasm-winit`
path ORs it into `FrameInputs` so it flows through `EmuCore::latch` exactly like
a keyboard bit. Touch input is therefore recorded/replayed identically by TAS
movies + netplay and adds no new determinism surface. Hidden by default (the
"Touch controls" checkbox reveals it), so the desktop keyboard UX is unchanged;
the native build is byte-identical (all touch state is wasm-only).

**Browser save-states + movies (wasm)** (v1.4.0 Workstream E). The browser
build reaches native QoL parity for two persistence features:

- **TAS movie `.rnm` I/O (E1).** Both wasm frontends bind the native F6/F7/F8
  movie hotkeys. F6 toggles recording (a power-on `MovieRecorder`); stopping
  serializes the `.rnm` and triggers a `Blob` download (the browser has no
  `rfd` save dialog). F7 toggles playback by `.click()`ing a hidden
  `<input accept=".rnm">` from within the keydown gesture; the selected bytes
  are `Movie::deserialize`d, `seek_to_start`ed, and replayed. F8 branches the
  current state into a new recording. Both frontends reuse the
  target-agnostic `MovieUi` state machine and the `wasm_io` Blob-download /
  file-picker helpers, so the browser records/replays the SAME deterministic
  `.rnm` format byte-for-byte as native (the `wasm-winit` path wired this on
  the unified `App`; v1.4.0 E1 folds the same hooks into the `wasm-canvas`
  embed's single-latch rAF loop).
- **IndexedDB save-states (E2).** Browser save-states moved off `localStorage`
  (string-only, ~5 MiB, base64-bloated) onto **IndexedDB** (`wasm_idb.rs`):
  binary `Uint8Array` blobs in the `rustynes`/`save-states` store, keyed by
  `"<rom_sha256_hex>:slot<N>"`, holding the exact `Nes::snapshot()` blob the
  native filesystem slots do (same `THM` thumbnail section). IDB is async, so
  the API is `async fn`-shaped (each `IdbRequest` Promise-wrapped + awaited via
  `JsFuture`) and the F1/F4/menu handlers drive it through
  `wasm_bindgen_futures::spawn_local` — never holding the emu lock across an
  `.await`, and re-checking the ROM SHA after the read in case the user swapped
  games mid-load. `localStorage` stays as a fallback (private-mode / IDB-blocked
  browsers) and a one-time migration source. **File → Manage States…** now
  works in the browser too via `wasm_save_states.rs`, an egui thumbnail grid
  (the wasm analogue of the native Save-States manager) populated by an async
  slot scan and rendered in the same egui `extra` closure as the netplay lobby.
  wasm-only; native + the desktop save-state format are byte-identical. The
  on-device record/replay + IDB-grid behavior can't be headlessly CI-verified —
  it carries a maintainer on-device manual-verify like the v1.2.0 F1/F3 items.

**Input-display overlay.** A read-only tool panel
(`debugger/input_display_panel.rs`) that draws a stylized NES controller per
active player — D-pad, Select/Start, B/A — with each currently-held button lit;
shows P1+P2 (and P3/P4 with Four Score). Open from **Tools → Input Display** or
the debugger toolbar's "Input HUD" checkbox. Useful for TAS authoring and
streaming; it reads the same held-button snapshot the emulator is fed
(`DebuggerOverlay::set_input_display`), so it is frontend-only with no core or
determinism impact.

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

- **CPU**: registers, current instruction, disassembly window (scrollable), step-instruction button, a **Breakpoints** section — exec/PC breakpoints (armed toggle, hex add, per-row remove, clear); when the program counter reaches one, emulation pauses and the CPU panel opens on the stopped PC — and (v1.4.0 Workstream D) an **Event breakpoints** section that breaks on hardware events (see below). Loaded symbol labels (D1) annotate the disassembly (a `label:` line above the matching address) and the breakpoint rows.
- **PPU**: nametable viewer (4 tables side-by-side, scroll-cursor overlaid), pattern table viewer (both tables, with palette selector), OAM viewer (sprite list + visual), palette RAM viewer. **v1.5.0 "Lens" Workstream A3** adds a **Scanline trace** tab (the per-scanline scroll/render register-write trace — $2000/$2001/$2005/$2006 — derived from the `debug-hooks` event log, surfacing mid-frame raster splits) and a native **Export CHR to PNG…** button (the combined 256×128 pattern dump) on the Patterns tab.
- **APU**: per-channel scope (waveform), volume meters, register dump.
- **Memory**: hex viewer of CPU bus + PPU bus, with go-to-address (disabled in RetroAchievements hardcore mode).
- **Mapper**: identity (mapper id + submapper + name + accuracy tier), ROM/RAM
  sizes with bank counts + battery/NVRAM, the IRQ mechanism (PPU A12 / scanline /
  CPU-cycle) and expansion-audio chip, the live PRG (`$8000-$FFFF`) / CHR
  (`$0000-$1FFF`) bank windows, IRQ-counter state, and the register state log
  (**v1.5.0 "Lens" Workstream I8** deepened this from the bare bank/IRQ dump). The
  cartridge-level metadata is filled by the bus on the read-only `MapperDebugInfo`
  view (output-only; no per-mapper change, byte-identical).

Two additional devtool panels are gated behind the off-by-default
`debug-hooks` feature (the headless test/bench builds omit it and keep a
byte-identical hot path; the hooks are output-only so determinism /
AccuracyCoin are unaffected):

- **Trace** (Debug → Trace Logger): a bounded ring (50k) of each executed
  instruction's CPU register file + cycle, with a live disassembled tail and
  an **export** of the full trace to a text file.
- **Events** (Debug → Event Viewer): the frame's CPU register accesses plotted
  on a scanline×dot grid, so you can see *when* in the frame a game touches scroll
  / mapper / APU registers. **v1.5.0 "Lens" Workstream A2** turned this into the
  GeraNES-class **graphical PPU Event Viewer**: a full 341 × 312 per-dot
  read/write **heatmap** (blue = PPU-register read, red = write) with a hover/click
  tooltip (register name + value + scanline + dot) and a synchronized
  register-access table whose selection follows clicks on the heatmap. It captures
  PPU/APU/mapper writes (`$2000-$3FFF` / `$4000-$4017` / `$4020-$FFFF`) **and** PPU
  register reads (`$2000-$3FFF`); each record now carries the accessed byte. Backed
  by the same `debug-hooks` event log (output-only, per-frame reset).

### v1.4.0 Workstream D — symbol loading + event breakpoints

These extend the existing devtools; both are output-only and live behind the
same `debug-hooks` feature (determinism / AccuracyCoin unaffected, feature-off
build byte-identical).

- **Symbol / label files (D1)** — **Debug → Load Symbols (.sym/.mlb/.nl)…**
  (native) picks a label file in one of three formats and merges its
  `address → label` map into the debugger:
  - **`.sym`** — ca65 / WLA-DX `ADDR LABEL` table (bank-prefixed `00:8000`
    addresses keep the low 16 bits; `;` comments and INI `[section]` headers
    are tolerated).
  - **Mesen `.mlb`** — `MemoryType:Address[-End]:Label[:Comment]`; the
    CPU-visible memory types map into CPU space (`G` system RAM → as-is, `R`
    WRAM → `$6000+`, `P` PRG ROM → `$8000+`); a range labels its start.
  - **FCEUX `.nl`** — `$ADDR#Name#Comment` name list (bank banners + comment
    lines skipped).

  Loaded labels annotate the CPU disassembler (a `label:` line above the
  matching address), the breakpoint rows, and the Trace Logger tail / export.
  **Debug → Clear Symbols** drops them. Parsing is a small hand-rolled,
  line-based reader (no new dependency); malformed lines are skipped, never
  fatal. Display-only — the deterministic core is never consulted or mutated.

- **Event breakpoints (D2)** — the CPU panel's **Event breakpoints** section
  arms break-on-event categories: **NMI entry**, **IRQ entry**, **Sprite-0
  hit** (observed where games detect it — a `$2002` read returning bit 6),
  **OAM DMA** (`$4014` write), **DMC DMA** (the sample GET), and
  **PPU/APU/mapper register read/write**. When an armed event fires, emulation
  pauses, the CPU panel opens, and the status bar reports the event kind +
  address + **frame / CPU cycle / scanline / dot**. The taps sit at the same
  observational commit points the event-viewer / interrupt-service / bus-access
  logs already use (`Bus::cpu_read` / `cpu_write` / `notify_irq_service` / the
  DMC GET), record-only — they never perturb emulator-visible state, so the
  determinism contract holds. The default (no category armed) is a single
  `mask == 0` early-out, so the unarmed hot path is unchanged. Like exec
  breakpoints, only the persistent run path checks them (run-ahead's
  speculative frames don't).

- **HD-pack per-pixel inspector (D3)** — **landed in v1.5.0 "Lens" Workstream A4**
  (see below). The v1.4.0 documented deferral is resolved.

### v1.5.0 "Lens" Workstream A — debugger visualization

These finish the GeraNES-class *visualization* devtools; all are output-only and
determinism-neutral (the new core telemetry is `debug-hooks`-gated and off in the
headless / shipped builds, so AccuracyCoin / the determinism oracle are unaffected
and the feature-off build is byte-identical).

- **Input Miniatures overlay (A1)** — **Tools → Input Miniatures** opens a live
  panel drawing every connected input device with real-time button / axis state:
  the standard pads (P1..P4, all four with the Four Score) and whatever
  non-standard device occupies the port-2 / expansion slot — Zapper (trigger +
  light-sensor strip), Arkanoid Vaus (paddle-knob slider + button), SNES mouse
  (left/right buttons + motion delta), Power Pad / Family Trainer mat (12-button
  grid), Family BASIC / Subor keyboard (pressed-key count), Konami Hyper Shot
  (P1/P2 Run/Jump), Bandai Hyper Shot (8-sensor mat). The app builds a frontend
  `MiniaturesSnapshot` each frame from the same host-side input state the emulator
  is fed (`input_miniatures_snapshot`) and pushes it via
  `DebuggerOverlay::set_input_miniatures` — no core touch, no determinism surface.

- **Graphical PPU Event Viewer (A2)** — the read/write heatmap described under the
  **Events** panel above (a new `debug-hooks`-gated PPU-register **read** capture
  in the core event log: `EventKind::PpuRead` + a `value` byte on `EventRec`).

- **PPU scanline-trace viewer + CHR→PNG export (A3)** — the **Scanline trace** tab
  plus the CHR→PNG export described under the **PPU** panel above.

- **HD-pack per-pixel inspector (A4)** — **Tools → HD Pack → Pixel Inspector**
  (native + `hd-pack`). For a chosen NES pixel it shows the HD-pack composition
  trace via a new `HdCompositor::inspect_pixel` query: the dominant tile's CHR
  identity (address / sprite-or-bg / flips / palette) + the Mesen CHR hash, the
  replacement rule that matched (image index) or the gated-off candidate, the
  gating `<condition>` names with their per-frame outcomes (ADR 0014), the base
  (stock) vs final (composited) RGBA, and an original/mod blend slider. The panel
  renders in the app's egui `extra` closure because it needs the live compositor +
  the per-frame snapshots (`present_hd_tiles` / `present_watched_mem` /
  `present_chr_snapshot`) the app captured under the emu lock for `composite`; the
  inspector only reads those already-deterministic snapshots and mutates nothing.

All panels are floating windows in egui's window system. The tool panels
(Cheats / Settings / Netplay / Cheevos / Perf / Input / Input Miniatures /
Replay / TAS / HD Pixel Inspector) are the same windows the menu bar surfaces
directly (see "Chip panels vs tool panels" above).

### v1.5.0 "Lens" Workstream C — creator / TAS / speedrun tooling

All frontend-only and additive; replay stays bit-identical (no new determinism
surface).

- **Replay / TAS window (C2)** — **Tools → Replay / TAS** opens a dedicated
  control + read-out surface for the `.rnm` movie machinery (modelled on
  GeraNES's `ReplayWindowUI`), complementing the status-bar HUD. It shows the
  mode (Idle / Recording / Playing) + a frame-progress bar, a **timebase**
  read-out (region + whole-Hz estimate + elapsed / total wall-clock time
  derived from the frame cursor), and a **port-topology** read-out (the device
  on each port — standard pad / Zapper / Vaus / SNES mouse / Power Pad /
  keyboard / Hyper Shot — and whether the Four Score adapter multiplexes
  P1..P4). Controls mirror the F6/F7/F8 shortcuts (Record / Play / Branch /
  Stop) and add **seek-to-frame** (a slider plus Start / −10 / +1 / +10
  buttons) for playback. The app pushes a read-only `ReplayInfo` snapshot each
  frame (`DebuggerOverlay::set_replay_info`, built from the host `[input]`
  config + `Nes::region`); button clicks are drained as a `ReplayRequest`
  (`take_replay_request`) and dispatched under the emu lock
  (`App::handle_replay_request`). **Seek is deterministic**:
  `MovieUi::seek_playback` re-derives state by `seek_to_start` + replaying the
  recorded inputs frame-by-frame — exactly the live replay path — so the
  post-seek framebuffer + cycle are bit-identical to having played to that
  frame (proven by `seek_is_bit_identical_to_linear_playback`).

- **NSF waveform visualizer (C3)** — the **NSF Player** window (Tools → NSF
  Player) gains a per-channel oscilloscope below the track controls: pulse 1/2,
  triangle, noise, and DMC, each a rolling 256-sample strip fed from the
  read-only `Nes::apu_snapshot()` DAC levels once per redraw (the same tap the
  APU debugger scope uses). When the loaded NSF drives an expansion-audio chip
  (`Nes::expansion_audio_chip()` — VRC6 / VRC7 / FME-7 / Namco 163 / MMC5 /
  FDS), the chip name is surfaced and the expansion channels are noted as summed
  into the master mix the standard APU plays. Output-only eye-candy: it samples
  a copy for display and changes no synthesis.

## Settings

Stored in `directories::ProjectDirs::config_dir() / "RustyNES" / "config.toml"`.
Includes: input bindings, audio device + latency + DRC, video filter
selection, pacing mode, rewind buffer size, run-ahead depth, default region
for region-less ROMs.

The settings window is the same tabbed UI the menu's View → Settings opens —
Video / Shaders / Audio / Input / Emulation — each tab routing to the matching
debugger-settings / input-rebind section so the live-apply plumbing is
shared (a present-mode / NTSC-filter / rewind change applies immediately).
v1.3.0 split the composable shader stack into its own **Shaders** tab and
renamed the latency/rewind tab to **Emulation**; the window also snapshots the
config before each frame and persists it if any control mutated it, so **every
setting in every tab auto-saves on change** (the per-control `save_config`
calls remain as a redundant backstop).

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

**Per-game database (nametable-mirroring override).** A CRC32-keyed game
database (vendored from TetaNES, ~2.6k entries) auto-corrects ROMs whose iNES
header carries the wrong mirroring flag: at load the frontend computes the
ROM's CRC32 (over PRG-ROM + CHR-ROM) and, if listed, applies a nametable
mirroring override via `Nes::set_mirroring_override`. The override lives in the
bus's nametable translation (uniform across all mappers, no per-mapper edits),
does not touch mapper-supplied VRAM (4-screen), and is persisted in the
save-state so rollback / restore stay consistent. It is frontend-only and
`None` by default (the core test suites construct the `Nes` directly and never
consult the database, so the suites stay byte-identical) and deterministic
(same CRC ⇒ same mirroring, so netplay peers agree). Scope is mirroring only —
region / mapper overrides and a Game Genie code database are not part of it.

## Lua Script console (`scripting` feature, native-only)

When built with the off-by-default, native-only `scripting` feature,
**Debug → Lua Script** opens a console that loads / reloads / stops a `.lua`
file and shows its log, errors, and `onFrame` callback count. The
`rustynes-script` crate embeds sandboxed **Lua 5.4** (vendored `mlua`) and is
host-driven: the frontend pumps it once per redraw under the emu lock with the
live `Nes`, script overlay draws (`drawText` / `drawRect` / `drawPixel`) render
through the egui pass, and control commands (`pause` / `saveState` /
`loadState` / `setInput`) apply via the existing pause / save-state path.
State-mutating script writes (`emu.write`) are gated off during netplay, TAS
replay/record, and RA-hardcore (the cheat-path policy). Because the feature is
off by default, the shipped / wasm / `no_std` builds are byte-identical. The
full `emu` API (memory access, CPU state, `onFrame` / `onExec` / `onRead` /
`onWrite` callbacks, control, and overlay draw) is documented in
[scripting.md](scripting.md); `examples/scripts/` ships `hud.lua` and
`ram_watch.lua`.

## In-app Documentation (v1.5.0 "Lens" Workstream I10, native)

**Help -> Documentation** opens a searchable egui manual
(`debugger/doc_panel.rs`) that reuses the SAME structured help-topic registry as
the `rustynes help` CLI / ratatui TUI (`cli::HELP_TOPICS`), so the terminal help
and the GUI manual cannot drift. A left topic list (with a `/`-style search box
filtering by title or body) selects between: the shared CLI topics
(controls / hotkeys / gamepad / features / mappers / config / scripting /
netplay), GUI-only topics authored in the panel (menu map, debugger & devtools,
settings), an **About** card (version / license / author / accuracy / features /
links), and a **per-release CHANGELOG** browser (the embedded `CHANGELOG.md`
split by its `## [version]` headings). Native-only (the topic registry lives in
the native-only `cli` module); the window reads no `nes`, so it renders in the
always-on tool-panel path. Frontend + output-only — no determinism surface.

## Shipped / open

All of the original open questions are resolved in v1.0.0:

- **WebAssembly target** — shipped (`wasm-winit` full winit+wgpu+egui build +
  `wasm-canvas` lightweight embed). The Web Audio user-gesture requirement is
  handled by arming `AudioContext` at the file-picker gesture.
- **CRT / NTSC shaders** — wgpu post-passes, toggleable in Settings → Display
  (render priority CRT > true-NTSC > simplified blur > plain blit):
  - A simplified Blargg-style blur (`ntsc.rs`, `ntsc_filter = "composite"`/`"rgb"`).
  - A CRT / scanline + aperture-grille pass (`crt.rs`, `crt_filter`).
  - The **true composite NES_NTSC filter** (`ntsc_bisqwit.rs`,
    `ntsc_filter = "composite-rt"`, T-110-A1): a faithful Bisqwit `nes_ntsc` port
    that samples the PPU's `R16Uint` palette-index framebuffer + per-frame phase
    (the core's `index_framebuffer()` / `ntsc_phase()` outputs), reconstructs the
    composite signal, and demodulates it per fragment with a windowed Y/I/Q filter
    for genuine dot-crawl / fringing artifacts. The index buffer is uploaded only
    while the filter is active; all tables are baked into the WGSL (WebGL2-safe, no
    storage buffers). Frontend-only — no core / determinism impact.
  Slang-shader ports remain a future enhancement.
- **Movie recording (TAS)** — shipped (`.rnm` record/play/branch).
- **Netplay** — shipped (rollback netcode, 2-4 players, native UDP + browser
  WebRTC), enabled by the deterministic core.

Future work tracked in `to-dos/ROADMAP.md`: mobile (iOS/Android) frontends and
additional CRT/slang-shader ports.
