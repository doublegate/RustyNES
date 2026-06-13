# Frontend

**References:** Phase 1 frontend stack decision (`winit + wgpu + cpal + egui`); `ref-docs/research-report.md` §Frontend (Rust ecosystem).

## Purpose

Implement the user-facing application in `crates/rustynes-frontend`: the windowing, rendering, audio, input, and debugger UI. The binary is named `rustynes`.

## Stack

| Concern | Crate | Notes |
|---------|-------|-------|
| Window + event loop | [winit](https://github.com/rust-windowing/winit) | Linux (X11/Wayland), macOS (AppKit), Windows (Win32), Web (WebAssembly later) |
| GPU rendering | [wgpu](https://github.com/gfx-rs/wgpu) | WebGPU API; Vulkan/Metal/D3D12/GLES backends |
| Audio output | [cpal](https://github.com/RustAudio/cpal) | Cross-platform PCM stream |
| GUI overlays | [egui](https://github.com/emilk/egui) + `egui-wgpu` | Debugger panels |
| Gamepads | [gilrs](https://gitlab.com/gilrs-project/gilrs) | XInput, evdev, GameController.framework |
| File dialogs | [rfd](https://github.com/PolyMeilex/rfd) | Native open/save dialogs |
| Config paths | [directories](https://github.com/dirs-dev/directories-rs) | XDG / Apple / Windows |

## Run loop

```
fn main() {
    let event_loop = EventLoop::new();
    let app = App::new(&event_loop)?;       // creates window, wgpu surface, cpal stream, egui ctx
    event_loop.run(app);
}

impl ApplicationHandler for App {
    fn window_event(&mut self, event_loop, window_id, event) {
        match event {
            WindowEvent::RedrawRequested => {
                self.nes.run_frame();        // emulator advances 1 frame
                self.upload_framebuffer();   // texture copy
                self.draw_egui_overlays();   // CPU disasm, PPU viewer, etc.
                self.surface.present();
                self.window.request_redraw(); // continuous render
            }
            WindowEvent::KeyboardInput { event, .. } => self.handle_key(event),
            ...
        }
    }
}
```

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
- A full-screen triangle pass samples the texture with nearest filter, scaled to the window's aspect-ratio-corrected viewport.
- Optional NTSC filter / CRT shader runs as a second pass.

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
- `F12` — open a different ROM via the system file dialog
- `Esc` — quit
- `~` (backtick) — toggle the egui debugger overlay

ROMs can also be loaded by **dragging a `.nes` file onto the window**.

## Debugger panels (egui)

- **CPU**: registers, current instruction, disassembly window (scrollable), breakpoints, step-instruction button.
- **PPU**: nametable viewer (4 tables side-by-side, scroll-cursor overlaid), pattern table viewer (both tables, with palette selector), OAM viewer (sprite list + visual), palette RAM viewer.
- **APU**: per-channel scope (waveform), volume meters, register dump.
- **Memory**: hex viewer of CPU bus + PPU bus, with go-to-address.
- **Mapper**: bank registers, IRQ counter state.

All panels are dock-able via egui's window system.

## Settings

Stored in `directories::ProjectDirs::config_dir() / "RustyNES" / "config.toml"`. Includes: input bindings, audio device, video filter selection, rewind buffer size, default region for region-less ROMs.

## Save state files

- File extension: `.rns` (RustyNES State).
- Stored in `directories::ProjectDirs::data_dir() / "RustyNES" / "saves" / "<rom-sha256>" / "slot-N.rns"`.
- Format: tagged sections per chip with version header. See the module-level rustdoc of [`crates/rustynes-core/src/save_state.rs`](../crates/rustynes-core/src/save_state.rs) for the on-wire layout (`HEADER` magic + format version + truncated ROM SHA-256 tag, followed by `BUS / CPU / PPU / APU / MAP` sections in any order with per-section version bytes). The CHANGELOG `[Unreleased]` entries also document per-chip section version bumps as they happen (e.g., MMC5 v2→v3 when vertical split-screen landed).

## ROM file handling

- Drag-and-drop a `.nes` file → load it.
- File menu → Open → native dialog.
- Recent files list (last 10).
- ROMs are *not* copied; the frontend stores absolute paths. (Save states are keyed by SHA-256 of the ROM, so moving the ROM doesn't break the save.)

## Open questions

- **WebAssembly target.** Defer to a stretch goal. The stack supports it but cpal's WASM audio backend has constraints (user gesture required).
- **CRT shaders.** Initial implementation: a single Blargg-NTSC-style filter as a wgpu post-pass. Ports of slang-shaders (cgwg, crt-easymode) deferred to Phase 5.
- **Movie recording (TAS).** Defer to Phase 5.
- **Netplay.** Out of v1.0 scope; deterministic core enables it later.
