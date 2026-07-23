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
| GUI shell + overlays | [egui](https://github.com/emilk/egui) 0.34 + `egui-wgpu` | The always-on desktop shell (menu bar / status bar / settings) **and** the toggleable debugger panels |
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

### The `full` maximal-native build (`cargo full-run`)

For the most fully-featured desktop binary in one command, the frontend has an
opt-in **`full`** feature that aggregates the maximal NATIVE feature set —
`retroachievements`, `scripting`, `script-ipc`, `hd-pack`, `debug-hooks`, and
`av-record` — additively on top of the `default` set. Two cargo aliases in
`.cargo/config.toml` are the "cargo --full equivalent":

```bash
cargo full-run path/to/rom.nes   # run the maximal native binary
cargo full-run --fullscreen rom.nes  # the `full-run` alias ends in `--`, so flags/args forward to the binary
cargo full-build                 # = build --release -p rustynes-frontend --features full
```

WASM-only features are deliberately excluded because `full` targets a native
binary: `script-wasm` is wasm-only *and* mutually exclusive with `scripting`
(piccolo vs. mlua), and `browser-cheevos` / `wasm-canvas` are browser-only. The
build is purely opt-in — the shipped/default build and the emulation core are
unchanged (`hd-pack` / `debug-hooks` only forward to the off-by-default
`rustynes-core` telemetry, proven byte-identical), so AccuracyCoin is unaffected.

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
  (`resampler.rs`) whose ratio is nudged up to ±1% by queue occupancy —
  **dynamic rate control** (Near's law: `ratio = (1-δ) + 2·fill·δ`,
  δ = 0.01, equilibrium at the `[audio] latency_ms` target, default
  60 ms). `[audio] drc = false` bypasses to a bit-exact push. **v1.5.0 "Lens"
  Workstream H4** widened δ from the ±0.5% Near/RetroArch default to ±1% (~17
  cents — far below audibility) so the servo can drain a catch-up-burst over-fill
  in ~5 s instead of ~10 s (a real high-refresh capture showed the queue
  oscillating 68–91 ms around the 60 ms target instead of tracking it); on a
  high-refresh panel (> 75 Hz) the latency target also gets a one-time +20 ms
  bump for ring headroom against the larger bursts. The resampler stage changes
  audio *timing* only — the core's emitted samples (the determinism + audio
  oracle contract) are untouched.
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
*reduce* the beat (a present-aligned-to-production cadence under Mailbox) stays
deferred: it needs on-device validation across real refresh rates and carries
pacing-regression risk, so it was explicitly **dropped under the v1.5.0 "Lens"
Workstream H measure-first rule** (no headless validation path). H1's lock-free
framebuffer handoff already removes the present-blocking that amplified the beat;
these counters remain the signal for whether the cadence work is later warranted.

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

**v1.5.0 "Lens" Workstream H8 — CSV ↔ panel parity.** The exporter had drifted
behind the panel; it is now built from a single ordered `columns()` list shared
by the header and every data row, and a `csv_columns_cover_panel_metrics` test
asserts every panel-surfaced `PerfView` metric has a column so the two can't
silently drift again. The row now logs the formerly-missing
`present_mode_fell_back` / `target_ms`, the audio DRC servo ratio + latency
setpoint, the run-ahead depth/throttle + rewind enabled/buffered state, and a
real `gpu_ms` (Workstream H5 put the `gpu-timing` feature in the default native
set; it requests `TIMESTAMP_QUERY` only when the adapter offers it, so the
presented image is byte-identical with it on/off and the wasm builds are
unchanged). **Workstream H7** adds a scripted capture gate:
`scripts/perf/perf_capture.sh` drives a bounded windowed run with logging
auto-enabled via the `RUSTYNES_PERF_LOG` env hook, then
`scripts/perf/perf_log_check.py` asserts `underruns` / `produced_max` /
`catchup_bursts` / `snap_forwards` stay within bounds (a maintainer-local /
on-display gate — pacing + audio behavior needs a real display + audio device,
so it skips cleanly when headless).

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
- **Graphic EQ** (`[audio] eq_enabled` / `eq_bands` / `eq_20_band` / `eq_bands_20`)
  — a default-off frontend output stage owned by the producer (`audio.rs::EqStage`),
  bypassed (zero overhead) when off / flat. The classic five-band voicing
  (60 / 240 / 1k / 3.8k / 12k Hz, ±12 dB) is the default; **v1.7.0 "Forge"
  Workstream H3** adds a **20-band graphic EQ** at the ISO third-octave centers
  (25 Hz–20 kHz, `eq.rs::EQ20_FREQS`), selected by the "20-band graphic EQ"
  checkbox (`eq_20_band`). The `Equalizer` is band-count-generic (a `Vec<Biquad>`
  cascade); a flat bank in either mode is a true bypass and bit-identical to a
  no-EQ build. A pre-v1.7.0 config (no `eq_20_band` / `eq_bands_20` keys) loads
  with the 5-band voicing and a flat 20-band default — byte-identical.

The following four groups are all **v1.7.0 "Forge" Workstream H3** additions —
each a frontend, output-only stage applied after the core has handed off its
mono samples, all bypass-by-default so the shipped sound is byte-identical
(see `docs/adr/0020`):

- **Stereo image** (`[audio] pan` / `reverb_mix` / `reverb_room` / `crossfeed`) —
  the NES APU is mono and the deterministic core hands the frontend one mono
  master, so this widens that master to stereo in the cpal callback
  (`audio_dsp.rs::StereoStage`). A per-APU-channel **pan** array (`-1`=L … `+1`=R,
  default all `0.0`=center) collapses to the *average* master pan applied to the
  mono master (true per-channel panning needs the core split deferred to v2.0); a
  small Schroeder **reverb** (4-comb + 2-allpass, `reverb_mix` wet, `reverb_room`
  decay, default `0.0`=dry); and a headphone **crossfeed** (default `0.0`=off). At
  center pan / 0 reverb / 0 crossfeed (the default) `StereoStage::is_bypass()` is
  true and the callback emits the mono value duplicated to L/R **bit-for-bit** —
  the byte-identical pre-H3 output. The reverb is rebuilt on the audio thread only
  when a param generation changes (the params are pushed lock-free from the UI).
- **Per-context volume** (`[audio] master_volume` / `volume_game` / `volume_menu`)
  — master × per-context (game vs menu) legs folded into the single cpal consume
  gain alongside `volume` / `muted` (`AudioConfig::effective_gain_for`). All three
  default to `1.0`, so the product equals the existing `effective_gain()` exactly
  until a slider moves — byte-identical default.
- **Output device picker** (`[audio] output_device`) — a combo box listing the
  enumerated cpal output devices plus "System default" (`None`). A named device is
  opened at the next stream open (restart); an absent / now-unplugged device falls
  back to the host default gracefully (`AudioOutput::try_new`'s `device_name`).
  Native-only (cpal enumeration); the wasm path is unaffected. Default `None` =
  the system default device (today's behaviour).

### Audio Mixer panel (Tools → Audio Mixer, v2.1.6 "Expansion Audio")

A dedicated tool panel (`debugger::audio_mixer`, `ToolPanel::AudioMixer`) that
unifies the per-source **mix balance** with **per-channel visualization** in one
window, for *any* ROM — cartridge audio, not just `.nsf` tunes. It renders in
`DebuggerOverlay::tool_panels` (which owns both the persisted `Config` and the
optional `&mut Nes`), so it works whether or not the deep debugger overlay is
open, and remains usable with no ROM loaded (the scopes sit flat).

- **Mix balance** — a slider (`0.0`–`2.0`) + mute checkbox per source: the five
  base 2A03 channels (pulse 1/2, triangle, noise, DMC) and the on-cart
  **expansion** channel (index 5), which is enabled + labelled with the detected
  chip family (`Nes::expansion_audio_chip()`: VRC6 / VRC7 (OPLL) / MMC5 / Namco
  163 / Sunsoft 5B / FDS) and greyed out on boards with no expansion audio. These
  edit the **same** `[audio] channel_gain` / `channel_mask` config the Settings →
  Audio tab does — the two surfaces stay in sync — and are the existing
  determinism-safe core UI overlay (`Nes::set_apu_channel_gain` /
  `set_apu_channel_mask`). **The mix is a frontend re-weight, not a synthesis
  change**: at the unity default the core mix takes the exact integer-gate path
  and is byte-identical, and because the gains/mask are never serialized into the
  `.rns` save state or `.rnm` movie, a save-state / TAS / netplay replay is
  byte-identical **regardless of the slider positions** — the recorded sound is
  always the core's own output.
- **Presets** — `Authentic (HVC-001)` (unity — the byte-identical default),
  `Balanced` (a Mesen-style "rebalanced VRC6 vs HVC-001" bias that tames a hot
  expansion chip and nudges the DMC down), and `Expansion boost` (pushes the
  expansion channel forward), plus a `Reset to unity`. Each writes a `[f32; 6]`
  into `channel_gain` and is clamped to the slider range.
- **Per-channel visualization** — a master scope plus per-channel rolling
  oscilloscope traces and peak **VU meters** for the six sources, sampled once per
  redraw from the read-only `Nes::apu_snapshot` DAC taps. This includes the v2.1.6
  `ApuDebugView::external` **expansion tap** (`Apu::external_out()`), a
  write-only-from-synthesis / read-only-to-observers copy of the raw external
  contribution that is never read back into the mix and never serialized — so the
  scope is display-only and determinism-neutral. The scope-ring / trace / VU
  primitives live in the shared `debugger::audio_scope` module, reused verbatim by
  the NSF player panel (which now also plots the expansion chip's own scope + VU).

Edits are applied to the core immediately (after the egui pass, no emu lock held
inside the closure) and persisted to `config.audio` on change, exactly like the
other audio preferences.

### HD-pack HD audio (v1.6.0 "Studio" Workstream H, `hd-pack`)

An HD-pack can ship external, studio-quality OGG Vorbis tracks that replace /
layer over the game's audio — the biggest Mesen2 gap vs ADR 0014. The
`hires.txt` declares them with `<bgm>album,track,filename` (looping background
music) and `<sfx>album,track,filename` (one-shot sound effects); the game
*selects* a track at run time by writing to the HD-pack audio-control register
at **`$4100`**. `src/hd_audio.rs` parses the declarations, decodes the OGG files
to mono PCM (resampled to the device rate, via the pure-Rust `lewton` decoder
pulled in only by the `hd-pack` feature), and runs an `HdAudioMixer`.

This is an **output-only** tap, the audio analogue of the HD tile-substitution
on the framebuffer and Workstream G's A/V-recording tap: it sits in the
**frontend** audio path, on top of the buffer the core already produced
(`Nes::drain_audio_into`). Each produced frame `EmuCore::produce_one_frame`
*peeks* `$4100` (a side-effect-free read of the already-produced bus state, like
the HD-pack `<condition>` watched-memory snapshot) and, if the control byte
selected a track, sums the decoded PCM into the drained APU buffer **in place**
before it reaches the DRC resampler / cpal queue (the same insertion point the
A/V recorder taps). It mutates no emulation state and adds no determinism
surface — the core's deterministic per-frame audio (save-state round-trip / TAS
replay / netplay) is untouched. The mixer is `Option`-gated on `EmuCore`; with
no audio pack loaded — or the `hd-pack` feature off — the audio is byte-identical
to the stock build. The `$4100` selection is **best-effort**: RustyNES does not
intercept the register write (no core change), so the frontend reads the value
back and treats a *change* as the trigger edge — packs whose cart maps `$4100`
into readable expansion space drive it faithfully; on pure open-bus carts the
selection is inert (a documented honesty caveat, like the BestEffort mapper
tier). Folder packs are supported; `.zip`-pack audio is a future extension.
Audible playback is a **maintainer manual-check** item (no audio device in CI);
the parse, the `$4100` trigger-edge logic, and the mixer buffering are
unit-tested.

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

### Web / wasm parity (v1.7.0 "Forge" Workstream H6)

Five additive, web-only browser-platform features bring the wasm build closer
to native parity. All are wasm-only (`#[cfg(target_arch = "wasm32")]`) or behind
the existing off-by-default `script-wasm` feature, so the native build is
byte-identical and the deterministic core is untouched (AccuracyCoin 139/141;
the two newest upstream PPU tests are known gaps).

- **Lua in the browser.** The unified winit path runs the experimental piccolo
  Lua engine (`script-wasm` feature, ADR 0012) end-to-end: a `.lua` file picker
  / paste box in `web/index.html` hands source to the `rustynes_load_script` /
  `rustynes_stop_script` bridge (`wasm_script.rs`), the `App` drains it
  (`take_pending` → `load_script_wasm` → `pump_scripts_wasm`) once per produced
  frame under the live `Nes`, and overlay draws render through the egui pass.
  Writes (`emu.write`) are gated during browser netplay, mirroring the native
  cheat-path policy. piccolo is observational/overlay-only and explicitly NOT
  byte-parity with native mlua (ADR 0012); it is never part of the determinism
  oracle. With the feature off (the shipped default) the bridge is absent and
  the JS controls no-op.
- **File System Access API** (`wasm_io::save_file_with_fallback`, ADR 0021).
  TAS `.rnm` exports save through a real `showSaveFilePicker` "Save As" dialog
  where the browser supports it (Chromium-family), with a graceful fallback to
  the synthetic-anchor download (`download_bytes`) on Firefox/Safari. The API is
  reached dynamically via `js_sys::Reflect`, so no `web_sys_unstable_apis` flag
  is needed; save-states stay on IndexedDB (`wasm_idb.rs`).
- **Gamepad API** (`wasm_gamepad.rs`). The page polls
  `navigator.getGamepads()` on every `requestAnimationFrame`, folds the
  browser "standard" (Xbox) mapping + the left analog stick into a `Buttons`
  mask (South=A, West/East=B, Back=Select, Start, D-pad), and pushes it through
  the `rustynes_gamepad_set_buttons` bridge. The Rust side reads it at the SAME
  late-latch as touch/keyboard (`App::frame_inputs` for winit, the per-frame
  `set_buttons` in the canvas rAF loop), routed to player 1 — so it is
  recorded/replayed identically by TAS movies + netplay and adds no new
  determinism surface (empty when no pad is connected = byte-identical default).
- **PWA / offline.** A `manifest.webmanifest` + a service worker (`sw.js`,
  registered from `index.html`) make the demo installable and offline-capable.
  The SW uses a cache-first-then-network strategy over same-origin GETs (Trunk
  hashes the `.wasm`/`.js` filenames per build, so a fixed precache manifest
  would go stale every rebuild); the app shell is cached on first load and
  served from cache offline. ROMs are loaded from local disk and never fetched,
  so nothing proprietary is ever cached. The manifest + icons + `sw.js` are
  copied into `dist/` by the Trunk asset pipeline; the bundle stays well within
  the 5 MiB gzip size budget.
- **`?settings=` share-links** (`wasm_share.rs`, ADR 0022). A curated subset of
  `Config` (NTSC/CRT filter + knobs, overscan, theme, 8:7, zoom, FPS, volume)
  serializes to a compact URL-safe base64 blob. On load,
  `config_from_url_or_default` applies any `?settings=` over the default config;
  the "Copy share link" button mints a URL (via the `rustynes_share_link`
  bridge) reflecting the live settings and copies it to the clipboard. `decode`
  is length-capped (8 KiB) and tolerant of malformed input (silently keeps
  defaults), and the blob is version-tolerant (`#[serde(default)]` fields).

### wasm size & startup + software blitter (v2.1.8 "Performance", A2 + A4)

**A4 — release wasm size/startup.** The `<link data-trunk rel="rust">` in
`web/index.html` now carries `data-wasm-opt="4"`, so the release build runs
`wasm-opt -O4` (Binaryen's aggressive speed pipeline) on the artifact instead of
trunk's default `-Oz`; `data-wasm-opt-params="--enable-simd --enable-bulk-memory"`
keeps the SIMD + bulk-memory features enabled through the opt pass (matching the
`+simd128` blitter path). Dev builds skip wasm-opt entirely, so `trunk serve`
stays fast. Startup uses **streaming instantiation**: trunk's generated loader
calls `WebAssembly.instantiateStreaming` (compile-while-download) whenever the
server sends `Content-Type: application/wasm` — GitHub Pages does, and the
service worker (`sw.js`) serves cached `Response`s with that header preserved, so
a warm PWA cache still instantiates by streaming. The bundle stays within the
5 MiB gzip budget enforced by `scripts/wasm_size_budget.sh` and the CI `web` gate.

On **code-splitting**: the two heavy optional native features are already absent
from the wasm bundle by construction — `scripting` (mlua) and `hd-pack`
(`rustynes-hdpack`) live only in the `cfg(not(target_arch = "wasm32"))` dep
table, so there is nothing to split out (the browser Lua path is the separate
default-off `script-wasm`/piccolo backend). The one heavyweight in the default
`wasm-winit` bundle is the egui debugger overlay, which is intrinsic to that
build; the lightweight split already exists as the `wasm-canvas` embed feature (a
direct canvas-2D blit with no egui/debugger). True dynamic-`import()`
code-splitting of a single `wasm-bindgen` cdylib is not supported by the trunk
toolchain we pin, so the feature-flag split (`wasm-winit` vs `wasm-canvas`) is the
mechanism, not per-symbol lazy loading.

**A2 — vectorized software palette blitter (`src/gfx_blit.rs`).** A reusable,
byte-identical CPU implementation of the core's palette-index -> RGBA emit: given
the PPU's `index_framebuffer` (`&[u16]`) plus the 512-entry LUT, it reconstructs
the RGBA frame that `Ppu::framebuffer` would produce. Scalar reference +
`wide::u32x8` (desktop) + `core::arch::wasm32` `v128` (`+simd128`, scalar
fallback otherwise); a unit test asserts SIMD == scalar byte-for-byte and against
the `build_rgba_lut` oracle. It is a validated utility (used by its bench + tests
and available to any host that has the index frame), **not** on the shipped frame
path — that stays GPU-resident (see the "Rendering" section below and
`docs/performance.md` for the measured profile and the honest note that a LUT
gather is memory-bound, so SIMD lands within noise of scalar).

## Rendering

- The PPU emits a `[u8; 256*240*4]` RGBA8 sRGB framebuffer.
- Each frame, the frontend uploads it to a wgpu texture (256×240, `Rgba8UnormSrgb`).
- A full-screen triangle pass samples the texture with nearest filter, scaled
  to the window through an aspect-ratio-correct **letterbox** transform
  (`gfx.rs::letterbox`).
- Optional NTSC filter / CRT shader runs as a second pass.

**Custom palette + palette editor (v1.5.0 D1).** A user-supplied palette
re-tints the display through the PPU's colour LUT. **Settings → Video →
Palette** offers (a) an **active-palette picker** (built-in, or any entry in the
named bank), (b) the legacy **Load .pal… / Clear .pal** file path, and (c) a
**Palette editor** (collapsing): an 8×8 per-index colour picker over the 64 base
colours, **Save as** (into the named bank), **Import .pal into bank**, and
**Delete**. The named bank lives in `[graphics.palettes]` and the selection in
`[graphics] active_palette`; the resolved base palette is pushed to the core via
`App::apply_active_palette` → `Nes::set_custom_palette` (named entry wins over
the legacy `[graphics] palette_file`) and re-applied on startup + every ROM
load. A 64-entry palette is the 192-byte form (longer 512-entry files use the
first 64 colours); the standard 2C02 composite emphasis is applied to the custom
base table by `rustynes-ppu::build_rgba_lut_from_base` /
`Ppu::set_custom_palette`. The default (built-in / unselected) is byte-identical.
Native-only file I/O (the editor + bank work on wasm; file dialogs do not).

**Generated NTSC palette (v2.1.2 F1.4).** A fourth palette source — **Settings →
Video → Palette → "Generated NTSC"** (collapsing) — synthesizes the 64-entry base
from a model of the 2C02's composite-video output rather than a hand table or a
`.pal` file. A checkbox enables it (taking precedence over the named bank + legacy
`.pal`), with live sliders for **saturation / hue / contrast / brightness /
gamma** plus **Reset to defaults**. The base is produced by the in-core
`rustynes_ppu::generate_base_palette` (Bisqwit / ares YIQ integration) and pushed
through the same `App::apply_active_palette` → `Nes::set_custom_palette` +
`build_rgba_lut_from_base` emphasis path as any custom palette — so there is no
new emphasis model. It is **off by default** (`[graphics] ntsc_palette_enabled =
false`; params in `[graphics.ntsc_palette]`), so the shipped presentation keeps
the built-in palette and is byte-identical. The synthesizer routes every
transcendental through `libm`, so its output is byte-identical across all targets
and is locked by a committed golden (`palette_gen::tests::matches_committed_golden`).

**NTSC / CRT shader ladder (v2.1.2 F2.2).** Presentation filters run as GPU
post-passes over the PPU framebuffer — **display-only**: they never touch the
core, the index framebuffer, audio, or any golden vector (the `visual_regression`
corpus stays byte-identical with any filter active). Two selection surfaces
coexist:

- **Legacy single-select** (Settings → Video): the **NTSC filter** dropdown
  (`[graphics] ntsc_filter` = `off` / `composite` / `rgb` / `composite-rt`) plus a
  binary **CRT** toggle (`crt_filter` + `crt_scanline`). The `composite-rt`
  (Bisqwit) option is the only place the Bisqwit **picture knobs** (contrast /
  saturation / brightness / hue) have a UI; a `CompositeRt` pass added via the
  stack inherits those same global knobs (`stack_ntsc_knobs`).
- **Composable stack** (Settings → Shaders): add / reorder / toggle / remove any
  of the six `BuiltinPass` variants, each with its `#pragma parameter` sliders,
  plus a save/load preset bank and constrained `.slangp` / `.cgp` import.

**Precedence:** when the stack has any enabled pass it **owns** the post-process
path and the legacy `crt`/`ntsc`/`composite-rt` filters are bypassed; otherwise
the legacy single-select applies (`App::on_gfx_ready`, and the same order in
`Gfx::render_with_overlay`: stack → CRT → Bisqwit → NTSC → direct blit).

**The three composite rungs** (there is deliberately **no** separate
"separable-kernel `nes_ntsc`" rung — the LMP pass already covers that tier):

1. `Ntsc` — a cheap simplified blur (5-tap + scanline dim + coarse fringe); not a
   signal encode/decode.
2. `Lmp88959` — a real single-pass composite encode→decode (EMMIR/LMP model),
   RGBA post-pass, composes anywhere.
3. `CompositeRt` — the faithful **Bisqwit** per-dot composite; samples the
   `R16Uint` palette-**index** framebuffer, so it must be the first pass.

**Live dot-crawl (phase).** The NES steps its colour phase through 3 frame states
(`Ppu::ntsc_phase()` → 0..=2); that live phase drives emulator-synced crawl in
both composite passes — `CompositeRt` via its `videoPhase` uniform, and
`Lmp88959` (F2.2) by advancing its base subcarrier phase `video_phase / 3` turn
on top of the user's static `phase` slider. The frontend snapshots the phase
whenever a phase-consuming pass is active (`Gfx::shader_stack_needs_phase` /
`BuiltinPass::uses_phase`); the phase is decoupled from the (heavier) index-FB
snapshot so a Lmp-only stack gets crawl without the index upload.

**Palette ↔ pass interaction.** The generated/custom palette (F1.4, `.pal`, named
bank) re-tints the **RGBA** framebuffer, so it feeds the RGBA passes (`Ntsc`,
`Lmp88959`, `Crt`) — but **not** `CompositeRt`, which decodes the raw palette
**index** and is therefore independent of the RGBA palette choice.

**Marquee CRT stack (v2.1.9 "Presentation & Signal", B6).** Three additional
single-pass CRT shaders — WGSL ports of the reference libretro *slang* presets —
live in the shared `rustynes-gfx-shaders` crate as **new WGSL files**
(`crt_royale.wgsl`, `crt_guest.wgsl`, `megatron.wgsl`), enumerated by the
`CrtStackShader` registry (stable slug + display name + index-texture flag):

- **CRT-Royale** — a Gaussian scanline beam integrated over the nearest source
  rows in gamma-linear light (beam width follows luminance), a selectable
  phosphor mask (aperture grille / slot / shadow-dot), photometric in/out gamma,
  barrel curvature + edge vignette.
- **CRT Guest Advanced / guest-dr-venom** — a power-shaped (crisper) beam with
  configurable width + sharpness, a 5-tap halation glow mixed in linear light,
  the shared mask selection, and curvature.
- **Sony Megatron (HDR)** — per-subpixel phosphor lighting driven into an exposed
  HDR headroom, with an SDR Reinhard tone-map fallback (the HDR hook remains in
  the uniform for a real HDR swapchain path).

All three share a 64-byte `rect / crop / params / aux` uniform: `params` carries
scanline weight, mask strength, mask type, and curvature; `aux` carries the
per-shader beam/gamma/glow/HDR knobs. Each is gate-validated as real,
compilable WGSL by a **naga** parse+validate test (`crt::tests::crt_stack_shaders_parse_and_validate`,
the same front-end + validator wgpu runs at `create_shader_module`).

They are wired as first-class `BuiltinPass` variants
(`CrtRoyale` / `CrtGuest` / `Megatron`) selectable from **Settings → Shaders**,
each exposing its `#pragma parameter` sliders (declared in `crt.rs`, ordered to
match the uniform slots so the generic declaration-order fill in
`ShaderStack::render` places each knob correctly; the trailing "source rows" aux
slot is left 0 and each shader falls back to 240 via `select`). Five showcase
entries join the built-in preset bank (CRT-Royale, CRT-Royale Curved, CRT Guest
Advanced, Sony Megatron, Raw NTSC Signal). **Per-game shader presets**:
`PerGameConfig` gains an optional `shader_preset` name resolved on ROM load
against the user preset bank then the built-ins (`ShaderPresetBank::resolve`) and
applied to the live stack — `None` / an unknown name applies nothing, so the
default load path stays byte-identical and the core is untouched.

**Raw NTSC signal-decode pass (v2.1.9 P4).** `signal_decode.wgsl` is the display
companion to the new core `rustynes-ppu::raw_signal` model. Like `CompositeRt`
it samples the palette-**index** framebuffer, but instead of Bisqwit's baked
tables it reconstructs the 2C02's **actual two-level chroma square wave** from
the index + emphasis (8 sub-samples/pixel over a 12-unit subcarrier wheel, with
per-line dot-crawl phase and emphasis attenuation — byte-for-byte the same model
as `raw_signal.rs`) and demodulates it with a windowed quadrature filter. Because
it decodes the true signal, it reproduces signal-domain artifacts an RGB
re-encode structurally cannot: composite colour bleed, dot crawl, and the
waterfall/dither transparency tricks. Off by default (a deliberate visual
choice, re-blessed like the generated palette); the default framebuffer +
`visual_regression` corpus stay byte-identical.

**Vs. `DualSystem` two-screen presentation (v2.1.2 F2.1).** A loaded Vs.
`DualSystem` cabinet (Balloon Fight / Wrecking Crew / Tennis / Baseball) runs both
cross-wired consoles and presents them together. The core dual engine
(`VsDualSystem` / `Emu::Dual`) already existed; the frontend adds an **additive
`EmuCore::dual` field** (mutually exclusive with `nes`), a `produce_dual_frame`
step that harvests both framebuffers + plays the main console's audio, a
`latch` branch routing P1/P2 → main and P3/P4 → sub, and a composed two-screen
present: `Gfx::compose_dual_into` arranges the screens **side-by-side** (512×240,
default) or **stacked** (256×480) per `[graphics] dual_screen_layout`, blitted via
the always-on dynamic `Gfx::render_dual` with an aspect-correct letterbox. Coin
(F10) routes to the main acceptor. Detection + install happen at ROM load
(`Emu::from_rom_with_sample_rate`), with the Vs.-DB DIP + RGB palette applied to
both consoles. The single-console path is byte-identical (the dual path is a
parallel branch at each chokepoint). **Scoped out in dual mode (ADR 0032):**
run-ahead, rewind, netplay, TAS, dual save-state, the debugger, and HD-pack — they
snapshot a single `Nes`. Real-cabinet boot stays fixture-limited (the circulating
dumps are the MAME maincpu half only).

**Present-path parity (v2.1.10 "Web Parity").** The **libretro** core
(`crates/rustynes-libretro`) now presents Vs. `DualSystem` cabinets too: it detects
them with the same `Emu::from_rom` and composes the two 256×240 framebuffers
side-by-side into a 512×240 XRGB8888 image (MAIN left, SUB right), presented within
a 512-wide `max_width` geometry so RetroArch draws the variable width without a
geometry renegotiation. Ports 0/1 → MAIN P1/P2, 2/3 → SUB P1/P2; MAIN audio plays;
save states use `VsDualSystem::snapshot`/`restore`; memory maps expose the MAIN
console. See `docs/libretro/advanced_features.md`. The **wasm** desktop-style
present remains deferred: the CPU compositor (`Gfx::compose_dual_into`) and the
core (`Emu::Dual`) are already cross-platform, but enabling it requires adding the
`VsDualSystem` detection to the *separate* wasm ROM-load path (the wasm build loads
from bytes, not the native `load_rom_from_path`), un-gating the `present_dual` /
`dual_mode` fields, and un-gating the GPU present branch (`Gfx::render_dual` +
`ensure_dual_blit`, currently `cfg(not(wasm))`). That is a multi-site change to the
**common** wasm present hot-path (which the single-console 99.99% case also runs)
for a very niche feature — the four Vs. arcade cabinet boards in a browser tab —
and a wasm GPU present cannot be runtime-verified in CI (no headless browser GPU
present). The libretro core (above) delivers Vs. `DualSystem` for the mainstream
RetroArch target now; the wasm second-screen present stays deferred until it can be
validated in a browser. Mobile remains deferred.

**Pixel aspect ratio.** When `[ui] pixel_aspect_correction` is on, the
letterbox targets the NES's native **8:7** PAR (display aspect
`(256 · 8/7) / 240`); off, it keeps the square-pixel 256:240 aspect. The
toggle (View menu / Settings → Video) rewrites the letterbox uniform live via
`Gfx::set_pixel_aspect`.

**Overscan crop (v1.5.0 D2).** The blit uniform's `crop` half is a per-side
crop on both axes — `crop.xy` = (V-scale, V-offset), `crop.zw` = (U-scale,
U-offset). The legacy binary **Hide overscan** toggle (`[graphics]
hide_overscan`, = top + bottom 8 px) and the per-side `[graphics] overscan`
(Top/Right/Bottom/Left, in NES pixels) are folded by `gfx::effective_overscan`
and pushed live via `Gfx::set_hide_overscan` + `Gfx::set_overscan`. **Settings →
Video → Overscan (per-side)** has live sliders + reset. The same per-side crop
is applied uniformly across the direct blit and the CRT / NTSC / Bisqwit /
shader-stack final passes. All-zero + toggle off is byte-identical.

**Lock discipline at present (`emu-thread`).** The egui shell runs every
frame but **never holds the emu lock inside the egui closure**. Two render
branches in `RedrawRequested`:

- The common **hidden** branch (debugger off, no `nes`-reading tool panel
  open) refreshes `App.present_staging`, **drops the lock**, then renders the
  shell with `nes = None` and presents with the lock released (so Fifo vsync at
  present can't block the emu thread). **v1.5.0 "Lens" Workstream H1 — when the
  dedicated emu thread is producing and this present needs nothing from the live
  `Nes` beyond the RGBA framebuffer (no NTSC composite-rt index buffer, no HD
  pack), `present_staging` is refreshed from a triple-buffer handoff
  (`present_buffer.rs`) the emu thread published into, so the present path takes
  the emu mutex *not at all* for the framebuffer.** Before H1 it copied the
  240 KiB framebuffer out of `EmuCore::present_fb` under the emu lock, which
  serialized the present against the emu thread's whole `produce_one_frame`
  (~8.5 ms with run-ahead) — on a 144 Hz panel the present could block up to a
  full produce (the flat-cost / spiky-`produced_max` signature in the
  2026-06-16 perf capture). The triple buffer is guarded by a small dedicated
  mutex held only for the brief copy, never across emulation work; the published
  bytes are exactly `nes.framebuffer()`, so it is a pure presentation-path
  change (determinism unaffected). The conditional cases (NTSC composite-rt index
  buffer, HD-pack snapshots) and the synchronous / wasm single-threaded builds
  keep the prior under-lock copy.
- The **locked** branch is taken when the debugger overlay is visible OR a
  tool panel that reads `&mut Nes` is open (today: Cheats / ROM-Database) —
  `needs_nes`. It holds the lock across the egui pass so the chip panels can
  inspect the live core. When the `hd-pack` feature is active and a pack is
  loaded, this branch ALSO captures the HD snapshots (tile-source, the 8 KiB
  CHR pattern space, watched-memory) under the same lock, runs the compositor,
  and presents the upscaled buffer through `render_hd_with_overlay` — so a
  loaded pack substitutes regardless of whether the debugger/tool panels are
  open (v1.7.1 #3; previously this branch silently presented the stock
  framebuffer). The deep-overlay panels still draw on top via the `overlay`
  closure.

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
   Input / Input Display) whenever their `show_*` flag is set, and the chip
   panels **only** when the overlay is visible (v1.7.0 beta.5 #55 removed the
   debugger toolbar HUD that used to render alongside them).

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

Per-tab content the panel sections render (`debugger/settings_panel.rs`):

- **Video** — present/pacing/NTSC/CRT, the **Palette** editor (D1), and the
  per-side **Overscan** group (D2).
- **Emulation** — run-ahead + rewind, plus the **Enhancements (non-accuracy)**
  group (v1.5.0 D3): `[enhancements]` disable-sprite-limit / overclock-scanlines
  (off by default, clearly labelled, **never applied while the oracle / TAS /
  netplay run**, and currently *staged / inert* — the cycle-accurate core has no
  hook for them; deferred to the v2.0 master-clock refactor, ADR 0002) and a
  cross-linked max-rewind-window knob.
- **Input** — the rebind grids + Port-2 device selector, now with contextual
  **device config** (v1.5.0 D4): SNES-mouse reported sensitivity + pointer-speed
  multiplier, Arkanoid Vaus pointer-speed, and the Power Pad / Family Trainer mat
  layout side (A / mirrored B). New `[input]` fields `mouse_sensitivity`,
  `pointer_scale`, `power_pad_layout`; defaults match the prior behaviour
  (byte-identical device report / input). Plumbed through `FrameInputs` /
  `SharedInput` to the device feed in `emu.rs`.

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
- **View** — Settings, Theme (Light/Dark/System + the v1.5.0 accessibility
  themes High Contrast / Colorblind-Safe), 8:7 Pixel Aspect, Hide Overscan,
  Fullscreen (`F11`, native), Window Size (1x-4x of the NES resolution, native),
  Show FPS, **Show Lag Frames** (v1.7.0 H4 — a status-bar counter of forward
  frames since ROM load in which the program polled no controller; off by
  default), Pause When Unfocused (auto-pause on focus loss), Show Menu Bar
  (`M`).
- **Tools** — Cheats, Movies (TAS: Record/Play/Branch + `.fm2`/`.bk2`
  import/export; v1.7.0 G4 also **imports** the legacy binary containers `.fcm`
  (FCEUX), `.fmv` (Famtasia), and `.vmv` (VirtuaNES) — the pre-`.fm2` TASVideos
  corpus — and stamps the matching ROM checksum (MD5 for `.fm2`, SHA-1 for `.bk2`)
  onto exports so they verify on TASVideos), **Record A/V** (v1.6.0 G; native +
  `av-record` feature), Netplay (native), RetroAchievements (native + feature),
  **Input Display** (v1.7.0 beta.5 #51 — the single consolidated controller HUD
  that covers the standard pads *and* every expansion peripheral; the former
  standalone "Input Display" + "Input Miniatures" entries were merged), Export
  Last 30s (.rnm) (v1.7.0 D1), NSF Player (moved here from Debug in v1.3.0),
  Replay / TAS (v1.5.0 C2), TAStudio (v1.6.0 A2), ROM Database, and an **HD Pack**
  submenu (`hd-pack` feature + native; folded in from the former standalone "Mod"
  menu) — which v1.7.0 G5 extends with an **HD-Pack Builder** (Build HD Pack
  (Record) → Stop & Save): an in-emulator recorder that captures the distinct
  tiles a game draws and writes a real-Mesen-`<ver>106`-format `hires.txt` +
  `tiles.png` starter pack (output-only; see ADR 0017). The loader reads the same
  real format — `[Cond1&Cond2]`-prefixed conditions + a
  `bitmapIndex,tileData,palette,x,y,brightness,defaultTile` tile line keyed on the
  CRC-32 of the 16-byte CHR `tileData` — so real third-party packs (e.g. Zelda /
  SMB HD remasters) load (ADR 0018), not just builder-authored ones.
- **Debug** — Show Debugger (a checkbox; v1.7.0 beta.5 #55 removed its `` ` ``
  accelerator — see below), Performance Monitor (moved here from Tools in
  v1.3.0), then the chip/state inspectors: CPU / PPU / APU / Memory / Memory
  Compare / OAM / Mapper / Trace Logger / Watch / Breakpoints (v1.6.0 "Studio"
  Workstream C) / Event Viewer / Lua Script, plus Cartridge Info / Header Editor
  (v1.7.0 A2) and Load/Clear Symbols (v1.4.0 D1).
- **Help** — Documentation (v1.5.0 I10; native, searchable in-app manual —
  overhauled in v1.7.0 beta.5 #53 with word-wrap, colorization, navigable
  sub-pages, and intra-doc links), Keyboard Shortcuts, About.

**v1.7.0 "Forge" beta.5 — UI overhaul (#51/#52/#53/#55).** Frontend-only,
determinism-neutral (the core stays byte-identical; AccuracyCoin 139/141 — the two newest upstream PPU tests are known gaps).
(#51) The two input HUDs were **consolidated into one "Input Display" panel** —
the superset miniatures panel (standard pads + Zapper / Vaus / SNES mouse /
Power Pad / Family Trainer / Family BASIC / Subor keyboard / Konami + Bandai
Hyper Shot / Four Score) kept its capability and took the "Input Display" name;
the old standalone panel + its `ToolPanel::InputDisplay` were removed. (#52) The
menu bar was audited for full v1.7.0 coverage and every entry carries a
Font-Awesome glyph; new recording state (TAS movie REC/PLAY, A/V REC, HD-Pack
REC) and the rich netplay read-out are surfaced in the bottom status bar. (#55)
The **debugger toolbar HUD was removed** (every panel opens from the menu bar
now); its only distinct content was the long-form RetroAchievements read-out, so
the freed backtick (`` ` ``) key now **toggles the status-bar RA display between
its compact and long-form variants** (the overlay is toggled from Debug → Show
Debugger).

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
panel — Cheats, the ROM Database editor, *and* (v2.2.0) the read-only ROM Info
browser (I6). **Tools -> ROM Info** (v2.2.0 "Capstone",
`debugger::rom_info_panel`) is a purely observational companion to the ROM
Database editor: for the loaded ROM it surfaces the two dump-identity CRC32 keys
(the header-excluded game-DB key + the full-file **No-Intro** key), the SHA-256,
the effective per-game database entry (title / mapper / region / mirroring /
submapper), and the decoded cartridge header read straight off the running `Nes`
(mapper id, region, PRG-ROM / CHR-ROM sizes; "CHR-RAM" when there is no CHR ROM).
It takes `&Nes` (read-only) — it never mutates the emulator, never writes the DB
overlay, and the deterministic core never consults it. No bootgod / nescartdb
board table is vendored, so the panel is honest about surfacing only the per-game
DB + the header rather than implying provenance it does not carry. The **RetroAchievements**
readout moved into the bottom status bar between the emulator-state label and the
FPS counter (I7). The **Keyboard Shortcuts** window reads the live `[input]` /
`[input.system]` bindings with a Player/device selector (I9). The **Input
Display** uses a per-group palette (D-pad green / Select-Start yellow / B-A
Nintendo red) (I5; the two HUDs were later merged into one "Input Display" panel
in v1.7.0 beta.5 #51). Settings: the
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

### Accessibility (v1.5.0 "Lens" Workstream E)

Frontend-only, additive, off-by-default — the shipped / native / `no_std` / wasm
builds stay byte-identical and AccuracyCoin holds 139/141 (the two newest upstream PPU tests are known gaps). Three features:

- **UI scaling (`[ui] zoom_factor`, default `1.0`).** Scales the entire egui
  shell (menu bar, Settings, debugger panels, fonts) by calling
  `ctx.set_zoom_factor` once per frame in the render loop (the call is a no-op
  when the value is unchanged). The emulated NES image is a raw framebuffer blit,
  *not* egui content, so it is unaffected — gameplay and determinism are
  untouched. Surfaced as a **UI scale** slider (50%-300%, 5% steps) with a Reset
  button in **Settings -> Video -> Accessibility**; the value is clamped to
  `UiConfig::ZOOM_MIN..=ZOOM_MAX` on apply and persisted.
- **Accessibility themes.** `AppTheme` is extended past Light/Dark/System with
  **High Contrast** (`high-contrast`) and **Colorblind-Safe** (`colorblind`).
  `ui_shell::apply_theme` builds each from a stock `egui::Visuals` base: High
  Contrast pushes every foreground/background pair to WCAG 2.1 AA/AAA ratios
  (near-black panels, near-white text, a bright-cyan accent, bold focus strokes)
  for low vision; Colorblind-Safe uses the deuteranopia/protanopia-friendly
  Okabe-Ito palette for the selection/hover/active accents so the focus cues
  never collapse to an ambiguous red-green pair. Both selectors (the **View ->
  Theme** menu and the Settings combo) iterate `AppTheme::all()` so they can
  never drift. The themes are applied only on change (the existing `last_theme`
  cache).
- **Keyboard-only navigation.** egui's menu bar and Settings are already
  Tab/arrow/Enter navigable; the gap was modal dismissal, since `egui::Window`'s
  close `X` has no key equivalent and the app's Esc/Quit binding is suppressed
  while a shell window is open (`shell_window_open` in the app keyboard gate). The
  `ui_shell::esc_closes` helper consumes a pressed `Esc` during the egui pass and
  clears the window's `open` flag, giving **Settings / About / Keyboard
  Shortcuts** a consistent keyboard escape hatch.

### Internationalization (i18n, v1.7.0 "Forge" Workstream H5)

Frontend-only, additive, English-by-default — with the default locale every
label is byte-identical to v1.6.0 and AccuracyCoin holds 139/141 (the two newest upstream PPU tests are known gaps). See
ADR 0023 for the rationale (why a hand-rolled catalog over Fluent/ICU/`rust-i18n`
and the wasm size budget).

The layer lives in `crate::i18n`:

- **Compile-time string catalog.** A `Key` enum names every translatable
  string; one `const fn` per locale (`english`, `spanish`) `match`es `Key` to a
  `&'static str`. There is no runtime file I/O, no parser, no extra dependency —
  the strings are read-only data baked into the binary, which keeps the wasm
  bundle inside the `scripts/wasm_size_budget.sh` 5 MiB gate.
- **Resolution.** `tr(key) -> &'static str` resolves against the current
  process-global locale; the `t!(Key)` macro is sugar for it (`crate::t!(MenuFile)`
  == `crate::i18n::tr(crate::i18n::Key::MenuFile)`). `tr_in(locale, key)` is the
  explicit form used in tests.
- **English fallback.** `Locale::English` is the `Default` and the universal
  fallback. The English catalog defines a value for *every* key; a non-English
  catalog may return `None` for an untranslated key, which `tr_in` resolves to
  the English string — so a partial translation degrades gracefully per string.
- **Selection + persistence.** The active locale is `[ui] locale` (TOML,
  `#[serde(default)]` → English, so older configs are unchanged). It is published
  to the i18n global via `i18n::set_locale` once at startup and every frame in the
  render loop (a relaxed atomic store), and surfaced as a **Language** combo in
  **Settings -> Video -> Display** next to the Theme picker. Because egui
  re-renders every frame and each converted call site reads `tr(..)` fresh, a
  language change takes effect on the next frame with no explicit invalidation.

**Incremental conversion.** This change wires the high-visibility surfaces
through `t!(Key)` / `tr(..)` — the menu bar (top-level menus + common File/View items),
the Settings title/tabs/Display labels, and the status-bar state words. Deeper
panels keep their literals for now. To convert a string:

1. Add a `Key` variant in `i18n.rs` whose **English** value is the *verbatim*
   current literal, and add the translation to each non-English catalog (or omit
   it to fall back to English).
2. Replace the literal at the call site with `crate::t!(TheKey)` (or
   `crate::i18n::tr(Key::TheKey)` when the key is computed).

Until a string is converted it renders its literal exactly as before, so the
conversion can proceed panel-by-panel without regressions.

### Chip panels vs tool panels

The panels split by what they need:

- **Tool panels** (`ToolPanel`: Cheats, Settings, Netplay, Cheevos, Perf,
  Input, GameDb, RomInfo, …) render whenever open, with the deep overlay off.
  `OpenPanel` sets the flag without forcing the overlay visible. (Cheats, the
  ROM Database editor, and the read-only ROM Info browser read the `Nes`, so
  the render path takes the locked branch — `any_nes_tool_open` — when any of
  them is open.)
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
- `` ` `` (backtick) — toggle the status-bar RetroAchievements read-out between
  its compact and long-form variants (v1.7.0 beta.5 #55; it formerly toggled the
  debugger overlay, which now opens from Debug → Show Debugger)

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

**Capstone peripherals (v2.2.0).** Three input-path additions, all additive so
the default (no-device) input path stays byte-identical:

- **Famicom microphone.** The hardwired second Famicom controller's push-to-talk
  microphone is surfaced on **`$4016` bit 2** (a `$4016`-only signal — it never
  touches `$4017`), driven by `Nes::set_microphone(pressed)`. The frontend maps a
  hold-to-talk key to it; games such as *The Legend of Zelda* (killing Pols
  Voice) and *Kid Icarus* poll it. Default (mic released) leaves the `$4016` read
  byte-identical to a stock NES, so the standard controller path is unaffected.
  The mic is a transient live signal (like a held button), released on
  power-cycle.
- **Family BASIC keyboard.** The full `9 × 8` positional keyboard matrix
  (`FamilyKeyboardState`, and the Subor clone) is selectable as the port-2
  expansion device; `input::family_keyboard_index` maps host keys 1:1 onto the
  72-key matrix (row-select via the `$4016` strobe + column-half on `$4017`).
- **Zapper light-timing.** The photodiode now integrates a **3×3 aperture**
  (field-of-view) around the aim point rather than a single pixel, asserting
  light only when ≥2 pixels cross the luma threshold (`ZAPPER_APERTURE_*`). This
  hardens detection against sub-pixel aim error and PPU edge noise while
  remaining a deterministic, pure function of the presented framebuffer (no
  save-state change). The finer ~19-26-scanline photodiode temporal hold is
  below the per-frame sample resolution of the default model; supported
  light-gun titles re-poll every frame, so frame-granular aperture sampling
  suffices for them.

  **v2.2.3 A3 — the beam-relative temporal model (opt-in).** That refinement has
  now landed as `Nes::set_zapper_temporal_light`, **default off**. With it on,
  the light bit is derived from where the CRT beam is at the moment of the
  `$4016`/`$4017` read rather than from the completed frame: dark before the
  beam paints the aim row (this frame has not drawn it yet), lit for the
  `ZAPPER_LIGHT_HOLD_SCANLINES` photodiode hold, dark once the capacitor
  drains. The frame-granular model structurally cannot express this — it returns
  the same answer at every scanline of the frame.

  It holds **no extra state**: light is derived on demand from
  `(framebuffer, aim, scanline)`, so it adds nothing to serialize and cannot
  desync a save state or a netplay rollback. Both models share one aperture
  test, so they can differ only in *when* they sample, never in what counts as
  light. One consequence is physically correct rather than a compromise: the
  aperture rows *below* the beam still hold the previous frame's pixels, which
  is exactly what the sensor sees.

  It stays opt-in because there is **no pass/fail light-gun test ROM** to
  adjudicate it. Promoting it would change output with no oracle able to confirm
  the change is an improvement — the project's standing bar (`docs/testing-strategy.md`)
  is that an accuracy change is oracle-proven or default-off.

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

**Input Display panel.** A read-only tool panel
(`debugger/input_miniatures_panel.rs`) that draws a live diagram of every
connected input device — a stylized NES controller per active player (D-pad,
Select/Start, B/A; P1+P2, and P3/P4 with Four Score) **plus** whatever expansion
peripheral occupies port 2 (Zapper, Arkanoid Vaus, SNES mouse, Power Pad /
Family Trainer mat, Family BASIC / Subor keyboard, Konami / Bandai Hyper Shot) —
with each held button / axis lit. Open from **Tools → Input Display**. v1.7.0
beta.5 (#51) consolidated the former standalone "Input Display" + "Input
Miniatures" panels into this one superset panel. Useful for TAS authoring and
streaming; it reads the same host-side input snapshot the emulator is fed
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

Toggled from **Debug → Show Debugger** (v1.7.0 beta.5 #55 removed the `` ` ``
accelerator and the toolbar HUD entirely). Opening the overlay shows the chip
sub-windows the user has opened from the Debug menu; there is no longer a
"debugger toolbar" — the read-outs it used to carry (frame / cycle / FPS /
movie / disk / netplay / RA status) all live in the bottom status bar now. The
panels are read-only: they never advance emulator-visible state, polling the
inspection API on `rustynes_core::Nes` once per visible frame.

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

- **Input Miniatures overlay (A1)** — **Tools → Input Display** (renamed +
  consolidated in v1.7.0 beta.5 #51; this is now the single "Input Display"
  panel) opens a live panel drawing every connected input device with real-time
  button / axis state: the standard pads (P1..P4, all four with the Four Score)
  and whatever non-standard device occupies the port-2 / expansion slot — Zapper
  (trigger + light-sensor strip), Arkanoid Vaus (paddle-knob slider + button),
  SNES mouse (left/right buttons + motion delta), Power Pad / Family Trainer mat
  (12-button grid), Family BASIC / Subor keyboard (pressed-key count), Konami
  Hyper Shot (P1/P2 Run/Jump), Bandai Hyper Shot (8-sensor mat). The app builds a
  frontend `MiniaturesSnapshot` each frame from the same host-side input state
  the emulator is fed (`input_miniatures_snapshot`) and pushes it via
  `DebuggerOverlay::set_input_display` — no core touch, no determinism surface.

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
(Cheats / Settings / Netplay / Cheevos / Perf / Input / Input Display /
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

### v1.6.0 "Studio" Workstream C — debugger depth (Mesen2-class)

Frontend-only, built on the existing `debug-hooks` observational logs; replay
stays bit-identical and the feature-off core build is byte-identical. The
keystone is a small expression evaluator (`debugger::expr`) that the rest of the
workstream rides on.

- **Expression evaluator (C1 keystone)** — `debugger::expr::Expr::parse` compiles
  a Mesen-`ExpressionEvaluator`-style string into a reusable AST; `eval` runs it
  against an `EvalContext`. The language supports:
  - **CPU regs** `a x y s p pc`, **PPU** `scanline` / `cycle` (alias `dot`) /
    `frame`.
  - **Memory** `[addr]` (one byte) and `{addr}` (a little-endian 16-bit word),
    both via a non-mutating CPU-bus peek.
  - **Access-context tokens** `value`, `address` (alias `addr`), and
    `isRead` / `isWrite` / `isExec` (case-insensitive) — the access being tested
    during a watchpoint/breakpoint replay; `0` in a context-free evaluation.
  - The full C operator set: `+ - * / % & | ^ ~ << >> && || ! == != < > <= >=`,
    ternary `?:`, and parentheses, on `i64` (comparisons / logicals yield `1`/`0`;
    divide / modulo / shift are guarded so a bad operand can't panic). Number
    literals are decimal, `$hex` / `0xhex`, or `%binary`.

- **Conditional breakpoints + R/W/X watchpoints (C1)** — the
  **Watch / Breakpoints** panel (Debug → Watch / Breakpoints). A **conditional
  breakpoint** is an exec-PC (or address range) plus an optional condition
  expression; it logs a hit when an executed PC is in range and the condition is
  true. A **watchpoint** is an address range + an access class (read / write /
  exec) + an optional condition; it logs every matching access. Hits accumulate
  in a bounded hit log (frame / tag / address / value, symbol-annotated).

- **Watch window + conditional trace (C4, free riders on C1)** — the same panel
  hosts a **watch window** (a list of expressions evaluated against the
  end-of-frame state and displayed each redraw) and a **conditional trace** (a
  format string with `{token}` / `{[addr]}` / `{{addr}}` substitutions, filtered
  by an optional condition expression — both reuse the C1 evaluator).

- **Observational contract (ADR 0010)** — all of the above is **observational**:
  `App::pump_watchpoints` (mirroring the Lua `pump_scripts`) runs after each
  frame, under the emu lock, arms the per-frame exec / access logs the active
  tools need (`Nes::set_exec_logging` / `set_access_logging`), and *replays* the
  just-finished frame's `Nes::exec_log()` / `Nes::accesses()` exactly like the
  Lua `onExec` / `onRead` / `onWrite` hooks. It never intercepts mid-instruction
  or mutates emulator-visible state, so determinism / AccuracyCoin hold. One
  consequence of replay (shared with the Lua hooks): the `value` / `address` /
  `isRead` / `isWrite` / `isExec` tokens are per-access accurate, but the
  register / PPU / `[addr]` tokens reflect the machine's **end-of-frame** state,
  not the exact cycle of the logged access (the panel UI documents this).

- **Hex editor (C2)** — the **Memory** panel (Debug → Memory) is now a full hex
  editor with **CPU bus / PPU bus / OAM** domain tabs. In the CPU domain a byte
  is editable: click it, type a hex value, Enter writes it via `Nes::poke_ram`
  (only `$0000-$1FFF` work RAM is writable — the core exposes no deterministic
  poke for the PPU bus / OAM / ROM, so those domains are read-only). Right-click
  a CPU byte to **freeze** it: the panel emits the frozen address/value as a
  `RawCheat` that the app re-applies after every frame, routed through the SAME
  raw-cheat overlay the Cheats panel uses (see `DebuggerOverlay::enabled_raw_cheats`).
  An **access-type heatmap** toggle tints each CPU byte by whether it was read
  (blue) or written (red) in the last frame, driven by the `debug-hooks` access
  log (refreshed by `App::pump_watchpoints`; arming the heatmap arms the log).
  A **find** box searches the visible domain for a hex byte sequence
  (`DE AD BE EF`) and jumps to the first match at/after the cursor, wrapping
  once. All reads are side-effect-free peeks; the only write path is the
  work-RAM poke/freeze (applied like a cheat), so the no-edit path is
  byte-identical and determinism holds.

- **RAM Search + RAM Watch (C3)** — the **Memory Compare** panel (Debug → Memory
  Compare) is upgraded from the v1.3.0 changed/unchanged search into the
  BizHawk/FCEUX-class tool. **RAM Search** now has an **operator × compare-to
  matrix** — each step keeps candidates whose value satisfies an operator
  (`== != < > <= >=`) against **either** the previous snapshot (find "what went
  down when you lost a life") **or** a typed constant (find "the value that is
  now 99") — plus **sizes** (1-, 2-, or 4-byte little-endian values; changing the
  size resets the in-flight search). Each surviving candidate has **watch** and
  **freeze** buttons. **RAM Watch** is a named list of `(address, size, label)`
  entries with live values, a per-entry **freeze** checkbox (also routed through
  the raw-cheat overlay — a multi-byte freeze expands to one cheat per
  little-endian byte), and native **`.wch` save / load** (a simple
  `addr size label` text format). Read-only against the core (the freeze cheats
  are the only writes, applied post-frame like every other cheat), so the
  no-freeze path is byte-identical.

### v1.7.0 "Forge" Workstream A — editing-capable debugger tools

The inspect-only PPU/OAM panels gain **writeback editors**, and the CPU panel
gains an **inline 6502 assembler**. Every writeback is `debug-hooks`-gated and
routes through the **same gated post-frame poke path** the raw RAM cheats use —
so it is a **no-op under netplay / TAS replay or record / RA-hardcore** and
**byte-identical with the feature off** (the no-edit queue is empty). The chip
stack stays `#![no_std]`; AccuracyCoin holds 139/141 (the two newest upstream PPU tests are known gaps).

- **The gated post-frame poke path.** A new `EmuCore::debug_pokes:
  Vec<DebugPoke>` queue (CPU-RAM / PPU-bus / OAM variants) is drained inside
  `EmuCore::produce_one_frame`, in the *same* caller-side, after-`run_frame`
  stage as the raw cheats, gated by `!writes_locked && !hardcore_blocked`. When
  locked the queue is **cleared, not deferred** (locked = no-op = byte-identical;
  no residual edit can leak into a later unlocked frame). `App` republishes
  `EmuCore::writes_locked` each frame from the EXACT condition `emu.write` uses
  (`netplay || RA-hardcore || movie playing/recording`) and harvests the panels'
  queued edits via `DebuggerOverlay::take_debug_pokes`. The unit test
  `emu::tests::debug_poke_is_gated_by_writes_locked` proves all three gate cases.
- **A1 — tile/CHR + palette + nametable + OAM editors.** An **"Edit (writeback)"**
  toggle on the PPU panel (off by default → read-only) exposes: a **palette**
  editor (click a swatch → edit the 6-bit value, queued as a `$3F00+idx` PPU-bus
  poke), a **nametable** tile/attribute editor (click a 32×30 cell → edit the
  tile byte and the 2-bit attribute quadrant, the latter via a read-modify-write
  of the attribute byte), and a **CHR** byte poker (`$0000-$1FFF`; a no-op on
  CHR-ROM carts, accepted on CHR-RAM). The OAM panel's row list becomes clickable
  to select a sprite, with a Y/tile/attr/X **byte editor**. Core hooks:
  `Nes::debug_poke_ppu` (→ `Bus::debug_poke_ppu`, the structural mirror of
  `debug_peek_ppu`: mapper CHR write / mapper-or-CIRAM nametable / PPU palette)
  and `Nes::poke_oam_byte`, both `debug-hooks`-gated; the PPU-side `debug_poke_*`
  helpers gate on the new `rustynes-ppu/debug-hooks` feature.
- **A2 — iNES / NES 2.0 header editor + read-only "Cartridge Info" pane**
  (native-only, `src/debugger/header_editor.rs`, **Debug → Cartridge Info /
  Header Editor...**). Inspects (read-only by default) and optionally edits the
  16-byte header of a ROM **file on disk** — never the running core. The pane
  shows format / mapper / submapper / mirroring / PRG-CHR sizes / battery /
  trainer / region / console type / RAM sizes (+ Vs. PPU + DualSystem for Vs.
  carts). The editor exposes combo boxes + unit-count fields and, on "Write
  header to file", re-serializes via the core's canonical `serialize_header` and
  overwrites the file's first 16 bytes (the ROM body is untouched). Decode +
  re-encode reuse `parse_header` / `serialize_header`, so the editor can't drift
  from the loader.
- **A3 — inline 6502 assembler** (`src/debugger/{cpu_panel,assembler}.rs`). An
  **"Assemble (6502)"** collapsing section (off by default) with an address
  field + a multi-line source box; "Assemble + queue" assembles each line in
  sequence and queues the bytes as `DebugPoke::CpuRam` writes (work RAM
  `$0000-$1FFF` only — the same gated target as the raw cheats; writes elsewhere
  are core no-ops). The opcode-encoding table is **derived at runtime from the
  canonical disassembler** (`rustynes_cpu::disassemble_at`), so it can never
  drift from the CPU core's decode. Branch displacements are range-checked.

### v1.7.0 "Forge" Workstream C — debugger depth (source-level / step / callstack)

Frontend-only, output-only telemetry built on the SAME `debug-hooks`
observational per-frame log-replay model as the v1.6.0 Watch panel: it is folded
in `App::pump_watchpoints` (under the emu lock, after each frame) and only
*reads* the just-finished frame's `Nes::exec_log()` / `Nes::accesses()` /
`Nes::interrupt_log()` (plus side-effect-free `Nes::cpu_bus_peek`). It never
intercepts mid-instruction or mutates emulator-visible state, so determinism /
AccuracyCoin hold, and with the core's `debug-hooks` feature OFF (the headless
test / bench builds) the build is byte-identical. None of these are v2.0 items —
they ride the current PPU-dot scheduler.

- **Call stack + step verbs (C1)** — `debugger::callstack::CallstackTracker`
  rebuilds a Mesen2-`CallstackManager`-class live 6502 call stack each frame by
  walking the exec log: `JSR` (`$20`) pushes a frame (return = `pc + 3`, target =
  the next executed PC), `RTS` (`$60`) / `RTI` (`$40`) pop, and a non-sequential
  PC transition the previous opcode does not explain (not a branch / `JMP` /
  call / return) is correlated against the per-frame interrupt-service log to
  label it an **NMI** or **IRQ/BRK** frame. The CPU panel grows a **Call stack**
  section listing the frames (innermost first, symbol-annotated) plus the
  stepping verbs **step-over / step-out / run-to-NMI / run-to-IRQ /
  step-scanline / step-frame**. A clicked verb is queued on the tracker;
  `App::pump_watchpoints` keeps the (paused) emulator advancing frame-by-frame
  until the verb is satisfied (`CallstackTracker::take_satisfied`), then pauses
  and opens the CPU panel — exactly like a breakpoint hit. The tracker is dropped
  on reset / power-cycle (`DebuggerOverlay::reset_debug_telemetry`).

- **Memory access counter + uninitialized-read detection (C2)** —
  `debugger::access_counter::MemoryAccessCounter` is a Mesen2-`MemoryAccessCounter`-class
  per-address (`$0000-$FFFF`) side-array of read / write / exec counts + a
  last-access CPU-cycle stamp + a sticky **`UninitRead`** flag (set when a
  volatile-RAM address — `$0000-$1FFF` work RAM or `$6000-$7FFF` cartridge WRAM —
  is read before it has ever been written). Reads / writes come from the access
  log, executes from the exec log. The Memory panel grows an **Access counters**
  section (toggle to enable — it arms the access + exec logs; a Reset button; the
  in-view 16 addresses' R/W/X counts + an `uninit` marker). Output-only.

- **ca65/cc65 `.dbg` source-line mapping (C3)** —
  `debugger::source_map::SourceMap::load_dbg` parses the ld65 `--dbgfile` `.dbg`
  format: it gathers the `seg` (CPU base address), `span` (segment + offset +
  size), and `file` tables, then resolves every `line` record's `+`-joined span
  list to CPU addresses, building an `address -> (source file, line)` map. The
  CPU panel disassembly is annotated with the original source line (a
  `; file:line` comment line above the matching instruction), complementing the
  v1.4.0 `.sym`/`.mlb`/`.nl` symbol-name labels. Loaded via the existing
  **Debug → Load Symbols** picker (the filter + extension dispatch now also
  accept `.dbg`, routing to `DebuggerOverlay::load_source_map`). Display-only;
  the parser tolerates malformed / future-extended lines without aborting.

### v1.7.0 "Forge" Workstream D — timeline + scaling rewind engine

Two additive, output-only / determinism-neutral pieces that scale the
session-history machinery: a scrubbable full-session **HistoryViewer** with
clip export (D1, frontend), and a **Zwinder-class compressed greenzone** that
the TAStudio editor now stores its save-states through (D2, core-adjacent). Both
ride the current PPU-dot scheduler — **no timebase change** — so AccuracyCoin
(139/141; the two newest upstream PPU tests are known gaps) holds and the shipped / native / `no_std` / wasm builds stay
byte-identical (the HistoryViewer only *observes*; compression is lossless).

- **HistoryViewer (D1)** — `src/history_viewer.rs`. A bookkeeping layer over the
  per-frame rewind ring that records the live session's **input stream**
  (`FrameInput` per port) in lock-step with the emulator + periodically stashes
  a lightweight **start-anchor** save-state. It is driven from
  `EmuCore::produce_one_frame` on persistent forward frames only (never on a
  rewind step), *after* the `nes` borrow is released — it reads the
  already-latched inputs + copies an already-produced save-state, so it cannot
  perturb emulation. **Tools → Export Last 30s (.rnm)** assembles a
  `rustynes_core::Movie` covering the trailing N seconds (start = the nearest
  anchor at-or-before the window, input stream = the recorded frames forward)
  and writes a `.rnm` via the save dialog; the clip **replays bit-identically**
  through `MoviePlayer` (unit-tested in the module). Frame ordering / eviction /
  export key off a monotonic internal record index, not the emulator's
  `frame()` (which repeats once at boot), so the timeline is robust. Cleared on
  ROM load + power-cycle. Native + `wasm-winit`.

- **Zwinder-class compressed greenzone (D2)** — `rustynes_core::zwinder`
  (`ZwinderStateManager`, `#![no_std]` + `alloc`). The compressed successor to
  the v1.6.0 uncompressed greenzone: frame-keyed snapshots stored as **LZ4 XOR-
  deltas** against periodic keyframes (interval default 16), with **reserved
  anchors** (frame 0 + markers + branch points, always self-contained keyframes,
  never evicted) and **density-tiered eviction** over the *compressed* sizes
  (thin the dense, distant-from-cursor past first; keep the cursor neighbourhood
  dense). Source: BizHawk `ZwinderBuffer` / `ZwinderStateManager`. The TAStudio
  greenzone (`src/tastudio/greenzone.rs`) is now a thin `usize`-frame adapter
  over it, so the same RAM holds far more history (feature-length TASes) and the
  deterministic seek/replay contract is unchanged — the TAStudio suite + the
  greenzone adapter tests still pass. **Determinism gate:** compression is
  lossless — `restore(compress(store(s))) == s` byte-for-byte; proven by the
  in-module `round_trip_equality_lossless` test (keyframes + deltas + post-
  eviction) **and** the integration tests in `rustynes-test-harness` that drive
  real `Nes` snapshots through store → compress → decompress → restore and
  assert byte-equality. LZ4 (`lz4_flex`, already a core dep + no_std-wired) is
  reused as the deflate codec; the round-trip gate is codec-agnostic.

#### v2.1.10 "Creator Tools" (B8) — force-greenzone

The normal greenzone keeps a density-tiered *skeleton* (keyframe stride + a dense
cursor neighbourhood), so scrubbing to an arbitrary frame costs a
load-nearest-keyframe + short re-emulate. **Force-greenzone** lets the user pin a
bounded frame range where a save-state is guaranteed at *every* frame, so
scrubbing / rewinding anywhere inside it is instant — for tightening a boss
pattern or a hard movement puzzle frame-by-frame.

- **API:** `TasEditor::set_forced_greenzone_range(Some((start, end)))` /
  `forced_greenzone_range()`; toggled from the piano-roll header's **"Force GZ"**
  checkbox (`TasRequest::SetForcedGreenzone`, dispatched under the emu lock). The
  checkbox forces a window of up to `MAX_FORCED_GREENZONE_FRAMES` ending at the
  cursor.
- **Mechanism (`src/tastudio/greenzone.rs`):** `Greenzone` tracks a normalised,
  span-clamped forced range plus the set of frames it *itself* pinned as anchors.
  `store()` pins a forced frame as a non-evictable anchor (so budget eviction
  never drops it); the `seek` / `record_frame` capture loops now store at every
  forced frame, not just the keyframe stride. Shrinking / clearing the range
  releases **only** the anchors force-greenzone added — a marker / branch-point
  anchor the editor pinned for its own reasons is untouched.
- **Memory budget:** the span is clamped to `MAX_FORCED_GREENZONE_FRAMES`
  (10,800 ≈ 3 min at 60 fps). Because forced frames escape density-tiered
  eviction, an unbounded range would defeat the byte budget — hence the cap. At
  the desktop save-state cost (≤ 1 ms to capture, ≤ 64 KiB uncompressed, far less
  under the Zwinder XOR-delta + LZ4 codec) the worst-case pinned footprint is on
  the order of tens of MiB compressed, well inside `DEFAULT_GREENZONE_BUDGET`
  (256 MiB).
- **Determinism:** force-greenzone is a pure *caching* optimisation. A seek into
  the forced range is bit-identical to a linear replay (the
  `force_greenzone_caches_every_frame_and_stays_bit_identical` editor test), so
  the TAS / save-state contract is unchanged.

### v1.6.0 "Studio" Workstream G — A/V recording (`av_record`, native + `av-record` feature)

Records the running game to a `.mp4` / `.mkv` (video + synchronized audio), and
since v2.1.9 to an animated `.gif` (video-only) or a `.wav` (audio-only).
**Native-only + behind the default-OFF `av-record` feature**, so the shipped /
wasm / `no_std` builds are byte-identical with it off (the module is not even
compiled). Implemented in `src/av_record.rs`.

- **Output format = the chosen extension (v2.1.9).** The recorder always
  captures both raw streams to temp files; the file extension picked in the save
  dialog (`Container::from_path`) selects which the single stop-time `ffmpeg`
  pass consumes. `.mp4` / `.mkv` = the two-input A/V mux (unchanged); `.gif` =
  video-only through the single-pass `palettegen` / `paletteuse` filtergraph
  (per-clip optimized palette + Bayer dither, decimated to 25 fps for a small
  crisp GIF); `.wav` = audio-only transcode of the mono `f32le` PCM to canonical
  16-bit PCM. The arg builders (`ffmpeg_args` → `gif_args` / `wav_args`) are pure
  and unit-tested.

- **Capture is a read-only tap on the already-produced output.** Inside
  `EmuCore::produce_one_frame`, *after* the emulator has produced the frame, the
  recorder copies the visible framebuffer (`present_fb`, RGBA8 256x240 — the same
  source the screenshot path reads) and the same audio samples the audio sink
  received that frame (`audio_buf[..n]`, mono `f32`). It **never** advances the
  emulator, mutates the core, or alters the per-frame framebuffer / audio, so the
  **determinism contract is untouched** and **AccuracyCoin holds 139/141** (the two newest upstream PPU tests are known gaps). With
  the feature off the produce path is byte-identical.
- **Encoder = external `ffmpeg` pipe.** The recorder spawns `ffmpeg`, streams
  **rawvideo** (`rgba`, 256x240, at the region frame rate as an exact rational
  `1e9 / frame_nanos`) over **stdin** (input 0), and writes the **mono `f32le`**
  PCM to a small temp sidecar that `ffmpeg` reads as input 1 (a sidecar rather
  than a second pipe avoids the classic two-pipe deadlock without threads).
  Output is H.264 (`libx264`, `veryfast`, `yuv420p`) + AAC. The sidecar is muxed
  and deleted at stop. This keeps the default build free of heavy media codecs —
  the only dependency is the system `ffmpeg` binary at run time.
- **Graceful fallback when `ffmpeg` is absent.** Arming fails with a clear toast
  (`A/V recording unavailable (no ffmpeg?)`) and emulation continues untouched; a
  broken video/audio pipe mid-recording auto-stops + drops the recorder.
- **Menu wiring.** Tools → **Record A/V** toggles via `MenuAction::AvRecordToggle`
  dispatched *after* the egui pass (like the other tools). Start opens an rfd
  save dialog (default `<data_dir>/recordings/<rom>-<utc>.mp4`); a second click
  stops + finalizes. The menu label flips to "Stop A/V Recording" while armed.
  wasm has no menu item (the variant stays un-gated so the match is exhaustive).
- **Carryover (maintainer manual-verify):** actual encoded-file playback can't be
  headless-verified (no GPU / no codec exercise in CI), like the egui-render
  carveouts — the unit-testable parts (ffmpeg arg construction, container
  inference, frame/audio buffering bounds, start/stop state) are covered by
  `av_record`'s tests. The FCEUX-style Code/Data Logger (output-only PRG/CHR
  coverage side-array) is **deferred** (not in this cut).

### v1.7.0 "Forge" Workstream H8 — spectator netplay (read-only)

The Netplay panel gains a **Spectate (watch, read-only)** control next to
Host / Join. It dials a host and runs a `rustynes_netplay::SpectatorSession`
(`netplay_ui::NetplayUi::start_spectate`): the local emulator replays the
match's *confirmed* input stream, one frame at a time, and **sends no gameplay
input**, so your controls do nothing and you cannot perturb the match. Because a
spectator predicts nothing and rolls back never, its framebuffer is
byte-identical to the players' confirmed timeline (unit-tested). It runs behind
the live match by the network latency; the status bar shows `NET spectate fN
+pending` (`pending` = confirmed-but-unshown frames). The host-side
broadcast/relay is a documented maintainer-manual carryover — see
`docs/netplay-webrtc.md` §4.

### v1.7.0 "Forge" Workstream H9 — power-user niceties

All additive + frontend-only; the core stays byte-identical.

- **Game Genie encoder** (Cheats panel, "Game Genie encoder" section) — enter a
  PRG address (`$8000-$FFFF`), a data byte, and an optional compare byte, click
  **Encode** to get the canonical 6-/8-character code, then **Add to list**.
  The encoder (`genie_encode`) is the exact inverse of the core decoder; every
  code it emits round-trips back through `rustynes_core::GenieCode::new` to the
  same substitution.
- **`.tbl` text tables** (`genie_encode::Table`) — parse the community
  `XX=glyph` byte→glyph table format and render a byte stream into readable text
  (for games with a non-ASCII character encoding, in the hex editor / RAM
  search).
- **Movie subtitles → `.srt`** (File → Movies → "Export subtitles (.srt)") —
  export the open TAStudio movie's named markers as a frame-exact SubRip
  subtitle track at the region's frame rate (NTSC's 60.0988 fps stays
  drift-free), for muxing into an A/V dump (`movie_srt::markers_to_srt`).

**Deferred (noted for a follow-up):** Virtual Pad (clickable on-screen
controller → `SharedInput`), input Macros feeding the piano-roll pattern-paint,
BasicBot (savestate-anchored brute-force search), multi-monitor / detachable
egui multi-viewport tool windows, A/V dump codec/sync depth, FDS Firmware
Manager (BIOS hash-verify), Multi-Disk Bundler, and a first-class headless Batch
Runner. The shipped subset (spectator + Genie encoder + `.tbl` + `.srt`) is the
self-contained, fully-tested core; the deferred items are larger and more
cross-cutting (most touch `app.rs`/the emu thread heavily, which a parallel-merge
cut keeps minimal).

### Game Genie code database + per-game nomination (v1.8.9 / v2.1.3)

The Cheats panel does not only *decode* a code you type — it **nominates** the
known Game Genie codes for the loaded ROM. When a game is recognized, a
category-grouped pick-list ("Known codes for …") appears above the manual entry
box; each row feeds the SAME validated path as a typed code
(`add_code_by_str` → `rustynes_core::GenieCode::new` → the cheat persistence /
post-frame poke overlay), so nominated and hand-entered codes are
indistinguishable downstream. The pick-list is **frontend-only** (`genie_db`) —
it never touches the deterministic core, and a malformed catalog row is silently
dropped at load, so the determinism / no-cheat firewall is untouched.

Recognition is by **CRC32**, computed for a loaded ROM in two conventions — the
header-**excluded** `game_db::rom_crc32` (CRC of PRG-ROM + CHR-ROM) and the
full-file **No-Intro** `game_db::rom_crc32_full` (CRC of the whole `.nes`
including the 16-byte iNES header). The debugger stashes both at load
(`set_rom_crc` / `set_rom_crc_full`) and the panel unions matches across three
bundled catalogs, de-duplicated by code (`codes_for_crcs`):

- `genie_database.tsv` — the small curated starter set;
- `genie_database_full.tsv` — the **bulk catalog** (~10.8k codes across ~520
  USA/World games), ingested from the openly-licensed libretro-database Game
  Genie files and keyed by the **full-file** No-Intro CRC (multiple CRCs per game
  cover the dump revisions); and
- `genie_database_headerless.tsv` — the **header-excluded re-key**: the same
  ~10.8k bulk codes re-keyed to `rom_crc32` (via the NES 2.0 database's content
  CRCs, joined by game name; ~16.5k rows over 521 games).

The third catalog is what makes matching **header-insensitive for every game**:
the full-file key only matches a dump whose header is byte-identical to
No-Intro's, but a **re-headered** dump (common — different tools rewrite the iNES
header) has the same PRG + CHR content, so its header-excluded `rom_crc32` still
resolves. It is regenerated by `scripts/gg/gen_headerless_genie_db.py` (the NES
2.0 database `nes20db.xml` is a build-time input, never committed). All three
catalogs ship on **every target including the wasm browser demo** — together they
gzip to ~370 KiB, inside the wasm bundle's 5 MiB size budget (the bundle is
~3.96 MiB gzip, ~1 MiB headroom). Codes, effect names, and the No-Intro / NES 2.0
CRC32s are factual data (not Nintendo program code) and freely redistributable;
commercial ROMs are, as always, never committed.

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

**Per-game `<rom>.json` config overlay (v1.7.0 "Forge" Workstream H4).** Layered
on the v1.2.0 game-DB, a small frontend-only overlay lets a single ROM carry its
own settings (the Mesen2 "per-game config" idea). On load — after the
header-excluded CRC32 is known — the frontend resolves a `<rom>.json` from two
places: a **config-dir overlay** (`<data-dir>/per-game/<CRC8>.json`, written by
the editor) and a **sibling** `<rom-stem>.json` next to the ROM; the config-dir
overlay **wins** (mirroring the game-DB user-overlay precedence). The schema
(`per_game::PerGameConfig`) is all-`#[serde(default)]`/`Option`: an `overrides`
block (region / mapper / submapper / mirroring — applied through the *same*
`apply_header_overrides` + `set_mirroring_override` paths the game-DB uses, so
they stack on the game-DB corrections), a Vs. `dip_switches` byte (applied via
`Nes::set_vs_dip`), reserved `video`/`audio`/`input` blocks (round-tripped, not
yet consumed), and free-form `notes`. An absent or inert file applies nothing,
so the default load path is **byte-identical** to today; the deterministic core
and the test harness never read it (the firewall), and because both netplay peers
resolve the overlay from the shared ROM CRC (the same file or none) and the
resolved mirroring/DIP live in the save-state, rollback stays consistent — the
same contract as the game-DB. Edited from **Tools → ROM Database**: the editor's
DIP-switch section (shown for Vs. System carts) exposes the 8 DIP bits with
numbered switches, applies edits live via `set_vs_dip`, and persists them to the
config-dir overlay (atomic temp-file + rename; an inert overlay deletes the
file). See [ADR 0019](adr/0019-per-game-config-overlay.md) for the
precedence/firewall decision.

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

## In-app Documentation (v1.5.0 "Lens" Workstream I10, native; overhauled in v1.7.0 beta.5 #53)

**Help -> Documentation** opens a searchable egui manual
(`debugger/doc_panel.rs`) that reuses the SAME structured help-topic registry as
the `rustynes help` CLI / ratatui TUI (`cli::HELP_TOPICS`), so the terminal help
and the GUI manual cannot drift. A left topic **tree** (with a `/`-style search
box filtering by title or body) selects between: the shared CLI topics
(controls / hotkeys / gamepad / features / mappers / config / scripting /
netplay), GUI-only topics authored in the panel (menu map; debugger & devtools;
settings; TAS & movies; Lua scripting & automation) — each of which may expose
**navigable sub-pages** — an **About** card (version / license / author /
accuracy / features / links), and a **per-release CHANGELOG** browser (the
embedded `CHANGELOG.md` split by its `## [version]` headings). Native-only (the
topic registry lives in the native-only `cli` module); the window reads no
`nes`, so it renders in the always-on tool-panel path. Frontend + output-only —
no determinism surface.

**v1.7.0 "Forge" beta.5 (#53) — pane overhaul.** Four long-standing defects
fixed: (1) **word-wrap** — bodies render through `render_body`, which wraps every
paragraph to the pane width (the old `ui.monospace(body)` overflowed the
viewport); (2) **sub-level navigation** — GUI topics now carry child pages
(`GuiTopic::children`, e.g. one per chip inspector under "Debugger & devtools",
the TAStudio editor under "TAS & movies"), the sidebar renders the tree, and
every node resolves to content instead of returning nothing; (3) **colorization**
— headings (with their `===`/`---` underline consumed), indented "code"/detail
lines, and bullets are tinted for readability; (4) **intra-doc hyperlinks** — a
`[[id]]` / `[[label|id]]` token in any body becomes a clickable link that
navigates to another doc page (resolved by `resolve_link` against the shared
topic ids, GUI topic ids, and sub-page ids), with breadcrumb back-links on
sub-pages. The content was also expanded to cover the full v1.7.0 feature set
(TAStudio, A/V recording, HD-Pack Builder, IPC/automation, the consolidated
Input Display, the new debugger depth, and the removed toolbar HUD / repurposed
backtick).

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
  - **Composable shader stack (v1.2.0 C2, ADR 0013).** Settings → **Shaders** →
    "Shader stack" builds an ordered, parameterized list of post-passes (the
    ping-pong executor in `shader_pass.rs`). An empty / all-disabled stack falls
    through to the byte-identical direct blit. Each built-in pass declares its
    knobs with RetroArch-style `#pragma parameter` headers, parsed into generic
    egui sliders; per-pass overrides + a named preset bank persist in the config.

    **v1.6.0 "Studio" Workstream I — shader/filter ecosystem** adds three RGBA
    built-in passes to the stack (all output-only, so AccuracyCoin / no_std /
    wasm stay byte-identical with them off):
    - **`lmp88959`** (`ntsc_lmp88959.rs`, I1) — an LMP88959-style composite
      NTSC/PAL look (encode-then-demodulate per output texel: chroma bleed, dot
      crawl, edge fringing). Unlike the index-only Bisqwit `composite-rt` pass it
      samples the **RGBA framebuffer**, so it can sit anywhere in the stack. Knobs:
      `saturation`, `sharpness`, `tint`, `phase`, `pal`.
    - **`hqx`** / **`xbrz`** (`upscale.rs`, I2) — hqNx- and xBRZ-style
      edge-directed pixel-art smoothers (single-pass GPU adaptations of the
      hqx / xBR edge-blend kernels), each with a `strength` knob.
    - **Constrained RetroArch preset import** (`slang_preset.rs`, I3) — Settings →
      Shaders → "Import .slangp / .cgp" parses a RetroArch preset and maps the
      well-known shader filename stems (`crt-*`, `*ntsc*`/`*composite*`,
      `*hqx*`/`*hq2x*`, `*xbr*`/`*xbrz*`) onto the built-in passes, carrying over
      matching parameter overrides. It is **not** a GLSL/Slang → WGSL transpiler
      (ADR 0013 keeps source translation out of scope): passes with no built-in
      equivalent are reported as **unsupported** (not silently dropped), and the
      import status shows the mapped/unsupported counts. Visual output of every
      shader pass is maintainer-manual-verified (it can't be headless-checked);
      the parsers, stack wiring, and WGSL parse+validate are unit-tested.
- **Movie recording (TAS)** — shipped (`.rnm` record/play/branch).
- **Netplay** — shipped (rollback netcode, 2-4 players, native UDP + browser
  WebRTC), enabled by the deterministic core.

The **Android** frontend shipped across the v1.8.x line (a Jetpack Compose shell
over the shared `rustynes-mobile` UniFFI bridge + the `rustynes-android` JNI
glue; sideload now, Google Play production at v2.1.0). Future work tracked in
`to-dos/ROADMAP.md`: the iOS/iPadOS frontend (v1.9.0, reusing the same
`rustynes-mobile` bridge), additional CRT/slang-shader ports, and the v2.0.0
"Timebase" one-clock + every-cycle-bus-access master-clock rewrite.
