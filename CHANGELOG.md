# Changelog

All notable changes to RustyNES will be documented in this file.

The format is based on [Keep a Changelog](https://keepachangelog.com/en/1.0.0/),
and this project adheres to [Semantic Versioning](https://semver.org/spec/v2.0.0.html).

> **v1.0.0 integrates a cycle-accurate emulation engine.** The original RustyNES
> emulation core (v0.1.0 -> v0.8.6, preserved below) was replaced wholesale with a
> new master-clock-precise, lockstep-scheduled core that reaches 100% on the
> AccuracyCoin hardware-accuracy suite. The `v0.9.0` -> `v0.9.7` entries are the
> **documentary lineage** of how that core was built and hardened (each notes the
> engine-internal milestone it came from); `v1.0.0` is the production cut that ports
> the original RustyNES desktop experience onto the new engine.

## [Unreleased]

Toward **v1.0.1** (compatibility + hygiene patch):

### Fixed

- **Mapper 89 (Sunsoft-2) — Mito Koumon background rendering.** The `$8000-$FFFF`
  register decode had bit 7 and bit 3 swapped: RustyNES used bit 7 for the
  one-screen mirroring select and bit 3 for the CHR-bank high bit, but the
  hardware layout (`CPPP MCCC`, per nesdev `INES_Mapper_089` and Mesen2's
  `Sunsoft89`) is bit 7 = CHR high bit (A16), bit 3 = mirroring. The wrong
  mirroring displayed the empty single-screen B while the game wrote its
  background to single-screen A, so the background appeared blank (only sprites
  rendered). *Tenka no Goikenban: Mito Koumon* now renders its title screen and
  backgrounds correctly. Isolated to mapper 89; AccuracyCoin 100% and every other
  game are unaffected.

### Changed (docs / hygiene)

- Reconciled `docs/STATUS.md` to the shipped v1.0.0 reality: the master-clock core
  is the default and only scheduler (the `mc-r1-full-cpu` umbrella was promoted to
  default and the flag removed), so the AccuracyCoin row now reads **100.00%**
  (not the pre-promotion 90.65%), the "Known residuals (deferred to v2.0)" section
  is re-titled **CLOSED**, and the removed feature-flag rows are marked historical.
- Archived the stale `to-dos/phase-7-*` + `phase-8-*` accuracy plans under
  `to-dos/archive/` and stood up release-named `to-dos/v1.0.1-compat-hygiene/` +
  `to-dos/v1.1.0-features/` (the live forward roadmap); updated `ROADMAP.md` +
  `to-dos/README.md` pointers.
- Re-worded the 7 "pin-the-floor" `#[ignore]` unit probes (cpu/apu/ppu) whose
  reasons cited the removed `mc-r1-full-cpu` flag as opt-in — they now state plainly
  that they pin superseded pre-master-clock unit behaviour (real coverage is the
  now-green `cpu_interrupts_v2` 5/5 + AccuracyCoin 100%); scrubbed the stale
  "known fail on the default lockstep build" comments in `cpu_interrupts_v2.rs`.

---

## [1.0.0] - 2026-06-13 - "Cycle-Accurate" (Production Release)

**The headline: RustyNES's emulation core has been replaced with a new
cycle-accurate, master-clock-precise engine, and the original RustyNES desktop
experience has been ported onto it.** This is the first 1.0 release: a
hardware-accurate NES emulator with a polished desktop shell, a live in-browser
WASM demo, and a synthesized documentation set.

The `v0.9.x` sections below (`v0.9.0` -> `v0.9.7`, all dated this same synthesis
day) are the **documentary lineage** of the new engine — they record, re-versioned
onto RustyNES's numbering, the work that produced the core now shipping in 1.0.0.
This `[1.0.0]` entry is the synthesis itself: the new core plus the v1.0.0-specific
shell, docs, and web work.

### The new core (replaces the entire v0.8.6-and-earlier emulation engine)

- **Master-clock-precise lockstep scheduler.** The PPU is the master clock; the
  CPU advances on its region-correct divider (3:1 NTSC/Dendy, 3.2:1 PAL), the APU
  every other CPU cycle — one shared timebase rather than the old
  PPU-stepped-around-CPU-instructions model. Mid-instruction PPU events
  (sprite-zero hit at an exact dot, MMC3 IRQ at a precise dot, mid-scanline scroll
  writes) are visible to subsequent CPU code without per-quirk patches.
- **AccuracyCoin 100.00% (139/139).** The kevtris AccuracyCoin hardware-accuracy
  battery passes completely (the prior core's accuracy ceiling was the Blargg
  suite; this clears the substantially harder AccuracyCoin gate, RAM-direct
  decoded). nestest matches the golden log with zero diff.
- **Determinism is a hard contract.** Same seed + ROM + input sequence yields a
  bit-identical framebuffer and audio stream — the foundation for save-state
  round-trips, regression oracles, TAS movies, and rollback netplay.
- **Band-limited audio.** A polyphase BLEP / windowed-sinc synthesizer
  (256 phases x 32 taps, ~81 dB SFDR) with a lookup-table non-linear mixer and a
  3-stage analog filter chain.
- **51 mapper families** (up from the original 5), including the expansion-audio
  chips (VRC6, VRC7 OPLL FM, Sunsoft 5B, Namco 163, MMC5), the full VRC family,
  Sunsoft FME-7, and the Taito / Sunsoft / Irem / Jaleco / Bandai / Konami long tail.
- **Famicom Disk System** (real-BIOS boot, read/write drive, writable `.fds.sav`,
  2C33 wavetable audio) and **Nintendo Vs. System / PlayChoice-10** RGB-PPU
  arcade support (2C03/2C04/2C05 palettes).
- **Rollback netplay** (GGPO-style, 2-4 players, native UDP + browser WebRTC mesh),
  **TAS movie** record/playback, **Game Genie + raw-RAM cheats**, **rewind**, and
  an opt-in native **RetroAchievements** integration (achievements, leaderboards,
  rich presence, hardcore mode).
- **WebAssembly target** with a live in-browser build, plus an integrated egui
  debugger overlay (CPU / PPU / APU / memory).
- **Performance-tuned frontend:** display-sync pacing, a lock-free audio ring with
  dynamic rate control, run-ahead to cancel game-internal input lag, and a
  dedicated emulation thread — the smoothest, lowest-latency playback path
  RustyNES has shipped.

### Added — v1.0.0 synthesis (the desktop shell + web + docs ported onto the new core)

- **Desktop UX shell.** An always-on egui menu bar (File / Emulation / Tools /
  View / Debug / Help) and status bar frame the NES image independently of the
  `` ` `` debugger overlay:
  - **File** — Open ROM (`F12`), Open Recent (missing files greyed out, with
    Clear Recent), save / load state, a ten-slot (0–9) Save Slot picker (set
    active slot + save/load to a specific slot), a thumbnail **Save States…**
    manager, Take Screenshot, Copy Screenshot to Clipboard (native), and the FDS
    Swap Disk Side (`F9`) item when an FDS game is loaded.
  - **Emulation** — Pause/Resume (`Space`, disabled during netplay), Reset (`F2`),
    Power Cycle (`F3`), Frame Advance (`\`, steps one frame while paused), a
    hold-`Tab` Fast Forward hint, **Speed presets** (25 / 50 / 75 / 100 / 150 /
    200 / 300 %, keys `=` / `-` / `0`), Run-Ahead 0–3, a read-only region label,
    and Vs. System Insert Coin (`F10`) for Vs. titles.
  - **Tools** — Cheats, TAS Movies (record `F6` / play `F7` / branch `F8`),
    Netplay, RetroAchievements, and the Performance Monitor, each opened as a
    floating window without needing the debugger overlay.
  - **View** — the tabbed Settings window (Display / Audio / Input / Advanced),
    Light / Dark / System themes, an 8:7 pixel-aspect toggle, a Hide Overscan
    toggle, Fullscreen (`F11`), Window Size presets (1x / 2x / 3x / 4x of the NES
    resolution), a Show FPS toggle, a Pause When Unfocused toggle (auto-pause on
    focus loss), and Show Menu Bar (`M`).
- **Audio, speed, and presentation controls.**
  - **Master volume** slider + mute, and **per-APU-channel mutes** (Pulse 1 /
    Pulse 2 / Triangle / Noise / DMC / Mapper Audio) in the Settings Audio tab —
    all-on by default (a playback overlay; the deterministic core output is
    byte-identical).
  - **Emulation-speed presets** (25 %–300 %) with pitch-shifted, glitch-free audio
    at non-100 % speeds; the status bar shows the speed when it is not 100 %.
  - A **thumbnail save-state manager** (File → Save States…) showing each slot's
    frame thumbnail + timestamp with per-slot save / load.
  - **Optional overscan cropping** (`[graphics] hide_overscan`), a **pause-dim**
    overlay, a **gamepad deadzone slider**, **controller hot-plug** toasts,
    **screenshot-to-clipboard**, a **Reset-to-Defaults** button per Settings
    section, and a frame-time **sparkline** in the Performance panel.
- **Frontend quality-of-life keys.** Hold-`Tab` fast-forward (runs the emulator
  unthrottled with audio muted) and `\` frame-advance (steps exactly one frame
  while paused), both default-bound in `[input.system]` and rebindable.
  - **Debug / Help** — the debugger overlay and per-chip panels; Keyboard
    Shortcuts and About.
- **Status bar** showing the ROM name, region, mapper, run-ahead depth, the
  Running / Paused / Netplay state, and the FPS readout, plus a translucent
  "PAUSED" overlay over the picture when paused.
- **First-run Welcome modal** with a quick-start shortcut list, shown once on a
  fresh install.
- **Live WebAssembly / GitHub Pages demo.** A playable in-browser build hosted on
  GitHub Pages, with the egui debugger overlay, AudioWorklet audio, and rAF
  display-sync.
- **Synthesized documentation set.** Per-subsystem specifications
  (CPU 6502, PPU 2C02, APU 2A03, mappers, cartridge format, scheduler), the
  cross-cutting architecture / testing-strategy / performance / compatibility
  docs, the Architecture Decision Records, and a unified user guide — re-grounded
  on the new core.
- **Branding and imagery** refreshed for the 1.0.0 release (logo, screenshots,
  showcase montage).

### Fixed

- **8:7 pixel-aspect and fullscreen now letterbox correctly** — the NES image is
  framed with clean black bars instead of garbage / smeared edges around it.
- **File / Tools submenus auto-close on hover-away**, matching standard menu-bar
  behaviour.
- **About window** credits "Created by DoubleGate" and its **GitHub** link opens
  the project page in a browser.
- **Pause is reachable and reversible** — a `Space` keybind toggles pause/resume,
  and the menu bar keeps repainting (~30 Hz) while paused so Resume is always
  clickable (previously the parked emulator left the menu frozen).
- **View -> Window Size keeps the chrome readable** — the size presets scale only
  the emulated picture (which letterboxes), with the menu / status bars held at a
  fixed width, so the menu no longer clips and the mouse stays aligned with its
  hit-areas.
- **Game Genie cheats apply reliably** — enabled codes are re-synced to the live
  core every frame the Cheats panel is open and re-applied after Reset / Power
  Cycle, so they keep working across a soft reset and a cold boot.

### Changed

- The desktop frontend now targets the new core's deterministic, lockstep API
  surface. Save-state, configuration, and input handling are re-wired onto it.
- Audio path reworked end-to-end (lock-free ring + dynamic rate control) and
  display pacing made display-sync-aware; input is latched immediately before
  each emulated frame to cut button-to-pixel latency.

### Notes

- The `v2.x` numbers referenced as "engine lineage" inside the `v0.9.x` entries
  are the upstream cycle-accurate-engine version tags, recorded for traceability
  only; they are not RustyNES release numbers.
- A small set of deep edge cases (a handful of internal-bus / IRQ-sample-timing
  AccuracyCoin sub-tests) remain documented in `docs/compatibility.md`;
  AccuracyCoin still reports a clean 139/139 on the shipping configuration.

---

## [0.9.7] - 2026-06-13 - Optimized Performance (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.8.0 work). Delta versus the prior engine stage — the stock-NES per-frame output
is byte-identical (AccuracyCoin 100.00% / 139/139; commercial oracles unchanged):
dynamic rate control lives in a frontend resampler stage and run-ahead is frontend
orchestration of the existing snapshot/restore, so the core is untouched.*

### Added

- **Display-sync pacing matrix** (`pacing_mode = auto | display | vrr | wallclock`,
  default `auto`): `display` makes vsync the clock (one emulated frame per refresh
  when the panel is within 0.5% of the console rate, the audio DRC absorbing the
  sub-percent bend) with an occlusion watchdog and a sustained-miss sticky fallback;
  `vrr` is vsync + a wall-clock pacer for G-Sync/FreeSync; `wallclock` is the
  sleep-then-spin pacer. Configurable swapchain depth (`max_frame_latency`).
- **Run-ahead** (`run_ahead`, default 1, 0-3): runs hidden frames each visible
  frame to remove the game's own internal input lag; the persistent timeline stays
  byte-identical, auto-disabled during netplay / movies / rewind, median-cost
  budget-throttled.
- **Dynamic rate control** (4-tap Hermite resampler nudging the output rate +/-0.5%
  from audio-queue occupancy) so audio never underruns/overruns from host-vs-DAC
  clock drift; off is a bit-exact passthrough.
- **Dedicated emulation thread** (default-on native) so UI / egui / wgpu-submit /
  file-I/O stalls no longer disturb emulation cadence, with best-effort Linux
  thread-priority elevation; inputs read from a lock-free `SharedInput` so the late
  latch survives.
- **Debugger Performance panel** with produced-vs-presented interval histograms
  (p50/p95/p99/max), audio-queue health, pacer-anomaly counters, optional GPU pass
  timing, and an opt-in 1 Hz CSV performance log.
- **Browser AudioWorklet** output (replacing the deprecated `ScriptProcessorNode`)
  + **rAF display-sync** on ~60 Hz panels (eliminating the wall-clock-vs-rAF beat).

### Changed

- Audio output is a hand-rolled lock-free SPSC ring with an allocation-free
  callback, start-gating, and a hard resync.
- Input is latched immediately before each emulated frame (the late latch),
  removing up to a frame of button-to-emulation latency.
- Core micro-optimizations: a `MapperCaps` capability cache skips unused
  per-CPU-cycle virtual calls, a 512-entry `(emphasis, color) -> RGBA` LUT for pixel
  emission, fat LTO, and auto-vectorized BLEP-scatter / rewind-XOR loops — all
  byte-identical by construction.

### Performance

- Rendering-heavy bench -26.0%, nestest bench -16.0%; snapshot fast path
  36 -> 14.6 us. Comfortably inside the 16.64 ms NTSC budget (~5-8x realtime).

---

## [0.9.6] - 2026-06-13 - Platform Expansion + RetroAchievements (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.6.0 -> v2.7.1 work). No accuracy/behaviour change to stock NES play
(AccuracyCoin 100.00%); every new path is gated on mapper / console-type /
player-count.*

### Added

- **RetroAchievements** (opt-in, native-only): achievements, leaderboards, rich
  presence, and hardcore mode via the vendored MIT `rcheevos` C library wrapped in
  a safe `RaClient` (hand-written FFI, no bindgen). Login with persisted token, an
  achievements panel with measured progress, unlock toasts, badge images fetched
  off-thread, and per-game progress persisted outside the deterministic snapshot.
  Hardcore mode disables save-load / rewind / cheats / frame-advance / debugger
  memory.
- **Vs. System / PlayChoice-10 RGB game-verification** (mapper 99 + clean-iNES
  byte-7 detection immune to the `0x0A` corruption) — VS Excitebike / Clu Clu Land /
  Castlevania and the PC10 dumps render through the 2C03 RGB palette; a SHA-256
  Vs.-game database supplies DIP presets and exact 2C04-000x palettes.
- **+13 mapper families (38 -> 51):** Taito (TC0190 / TC0690 / X1-005 / X1-017),
  Sunsoft-1/2/3R, Irem G-101, Jaleco, Bandai, Konami-VS, and more — all visually
  verified.
- **N-peer netplay:** a UDP multi-joiner roster handshake (2-4 players) plus a
  full browser **WebRTC mesh** (one data channel per peer) with a reference
  signaling server and a deployable `deploy/` bundle (signaling + `wss://` proxy +
  STUN/TURN).

### Fixed

- **Real-BIOS Famicom Disk System boot now works** (the device previously hung on
  "NOW LOADING"): the read engine synthesizes the on-disk wire format the `.fds`
  container omits (gaps / start-mark / CRC-16) and corrects four masked drive
  registers plus a `$4025` bit-3 mirroring inversion (the "DISK TROUBLE ERR.20"
  root cause). Verified across all three BIOS revisions.
- **Rollback netplay now works in real two-instance sessions** (native and browser):
  the root cause was that `power_cycle()` was not a true cold boot — it left the
  master clock (`ppu_clock`) and several DMA/latch fields carrying residual
  run-history phase, so two peers that had run a different number of frames diverged
  from frame 0. Power-cycle now fully cold-boots (including rebuilding stateful
  mappers), with an input-resend + cumulative-ack reliability layer and exact
  one-frame-per-pace driving on both transports.
- An NTSC-filter shader crash (dynamic indexing of `let` arrays rejected by naga)
  and several RetroAchievements login / badge-lock-state / User-Agent issues.

---

## [0.9.5] - 2026-06-13 - Netplay (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.3.0 -> v2.5.0 work). No accuracy/behaviour change to single-player NES play
(AccuracyCoin 100.00%); single-player frame output is byte-for-byte unchanged.*

### Added

- **GGPO-style rollback netplay.** Each peer runs the bit-deterministic core
  locally, predicts the remote input, advances, and on a misprediction rolls back
  to a save-state and re-simulates — the determinism contract guaranteeing the
  re-sim matches. A new netplay crate provides `RollbackSession` (per-frame input
  history + save-state ring + periodic checksum desync detection + seeded RNG, no
  wall-clock), a `Transport` trait (in-memory for tests + non-blocking UDP that
  never panics on hostile packets), and a host/join connection with a `Sync`
  handshake + ROM-hash check. The cross-peer digest is `framebuffer ^ cycle`, not
  the full snapshot (which carries audio-drain transients that never affect future
  frames).
- **Up to 4-player rollback** (generalized session + a fully-connected mesh
  transport; Four Score auto-enabled above 2 players), with native frontend host /
  join UI and a netplay HUD.
- **Internet-netplay + arcade groundwork:** a STUN client (RFC 5389) and a hole-punch
  state machine; the netplay crate made `wasm32`-buildable with a WebRTC
  `Transport`; and the Vs. System / PlayChoice-10 RGB-PPU foundation (2C03/2C04/2C05
  palettes, NES 2.0 byte-13 parsing, Vs. DIP switches + coin/service inputs).

---

## [0.9.4] - 2026-06-13 - Coverage + Input + FDS (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.1.0 -> v2.2.0 work). No accuracy regression (AccuracyCoin 100.00%; the
default/standard-pad paths are byte-identical — new paths are opt-in / separate
parse).*

### Added

- **+13 mapper families (25 -> 38):** Bandai FCG (+minimal EEPROM), Jaleco
  SS88006, Tengen RAMBO-1, Irem H3001, Sunsoft-3/4, Bandai discrete, Konami VRC3,
  Holy Diver, Namco 118 / DxROM / TxSROM, Namco 175/340 — spec-implemented with
  register/IRQ unit tests and boot smokes.
- **Expansion input devices** (opt-in per-port overlay; the default pad / Four
  Score path stays byte-identical): the **Arkanoid Vaus** paddle (game-ROM-verified)
  and the **Zapper** light gun (framebuffer-luma light detection).
- **Famicom Disk System support.** The FDS RAM adaptor (32K PRG-RAM + 8K CHR-RAM +
  a user-supplied `disksys.rom` BIOS), the `$4020-$4026`/`$4030-$4033` register map
  with the per-CPU-cycle timer IRQ, the disk read+write drive with multi-side
  eject/insert, writable-disk persistence (`.fds.sav`), and the 2C33 wavetable
  audio. New `Nes::from_disk` API; frontend `.fds` open / drag-drop + a one-time
  BIOS prompt + a disk-swap key. (The BIOS is never committed; the device and audio
  are unit-tested.)
- Substantial test-ROM coverage growth (the blargg PPU/APU/timing suites and the
  expansion-audio corpus wired in).

---

## [0.9.3] - 2026-06-13 - Master-Clock Scheduler -> 100% Accuracy (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v2.0.0 -> v2.0.1 work). This is the stage where the master-clock scheduler became
the only path and the AccuracyCoin gate was fully cleared.*

### Changed

- **The master-clock-precise scheduler became the default and only path.** The
  prior integer-lockstep scheduler — and the umbrella of accuracy feature gates it
  hid behind — were removed entirely once the master-clock path proved itself,
  collapsing dozens of experiment flags into unconditional code with zero
  behaviour change on the shipping build.
- **Region-exact CPU:PPU ratios** parametrized: 3:1 for NTSC/Dendy and **3.2:1 for
  PAL** (the prior core only modelled 3:1).

### Fixed / Accuracy

- **AccuracyCoin reached 100.00% (139/139).** A long accuracy program closed via a
  unified per-cycle DMA engine (replacing the two-driver span split), a put-cycle
  end-flip parity fix, delayed-`$4015` and edge-arm-suppress handling, and the
  OAM-DMA open-bus rule — plus the master-clock-precise CPU/PPU phase relationship
  that finally closed the long-standing IRQ-sample-timing residuals. nestest 0-diff;
  the blargg `cpu_interrupts_v2` suite passes; region-exact CPU timing verified.

---

## [0.9.2] - 2026-06-13 - Accuracy Hardening + Frontend Features (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v1.5.0 -> v1.7.0 work). Additive — accuracy held while frontend features and a
nesdev-accuracy pass landed.*

### Added

- **Nesdev accuracy-hardening pass:** vendored/wired blargg `instr_misc` /
  `instr_timing` / `cpu_reset` suites, VRC2/4 register fixtures, automated
  PAL/Dendy region-timing gates, a seeded power-on RAM fill (developer mode; the
  default path stays byte-identical), and documented scope closure across the
  mapper / input / region matrix.
- **Game Genie + raw-RAM cheats.** Clean-room 6/8-char Game Genie decoding applied
  as a runtime overlay (off by default, not captured in the deterministic
  snapshot/movie), plus `$addr=$value [if $compare]` raw-RAM cheats — both with a
  debugger cheat panel persisted per-ROM.
- **Four Score** (4-controller adapter) support over the canonical 24-read serial
  sequence (opt-in; the standard two-controller read stays byte-identical), with
  P3/P4 keyboard + gamepad rebinding.
- **Config-driven gamepad rebinding UI** (per-player keyboard + pad bindings,
  analog-stick-as-D-pad) and an egui graphics / audio / rewind **settings panel**.
- Browser persistence: in-browser TAS movie download/upload and `localStorage`
  save-states keyed by ROM hash.

---

## [0.9.1] - 2026-06-13 - Expansion Audio + Web + TAS (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v1.1.0 -> v1.4.0 work). Additive over the new core's v0.9.0 baseline.*

### Added

- **VRC7 OPLL FM audio** via a clean-room pure-Rust port of `emu2413` (MIT),
  completing the expansion-audio family (VRC6, Sunsoft 5B, Namco 163, MMC5, and now
  VRC7 FM) — *Lagrange Point* plays with in-game audio.
- **The WebAssembly target.** The frontend builds for `wasm32-unknown-unknown` in
  two flavours: a full winit + wgpu + egui browser app (WebGPU with a WebGL2
  fallback, the egui debugger overlay, an NTSC filter) and a lightweight canvas-2D
  embed mode, with shared Web Audio and a GitHub Pages deploy.
- **TAS movie record/playback.** A versioned `.rnm` movie format (deterministic
  start point + a raw per-frame input stream; optional save-state start), recorder /
  player, and save-state branching — proven by byte-identical round-trip tests
  (framebuffer + audio + cycle count). Record / play / branch hotkeys in the
  frontend.
- A canonical get/put-cycle DMC-DMA scheduler model (introduced alongside the
  existing scheduler under the parallel-implementation pattern).

---

## [0.9.0] - 2026-06-13 - Cycle-Accurate Core Engine + Frontend MVP (documentary lineage)

*Documentary lineage of the cycle-accurate engine (integrated from the engine's
v1.0.0 work). This is the new hardware-accurate core that replaces RustyNES's
original emulation engine — the baseline the rest of the `v0.9.x` lineage builds on.*

### Added

- **A new master-clock-precise, lockstep-scheduled core** (CPU + PPU + APU on one
  shared timebase; the PPU is the master clock, the CPU advances on its region
  divider, the APU every other CPU cycle). Mid-instruction PPU events are visible to
  subsequent CPU code without per-quirk patches. The Bus owns all mutable state; the
  workspace dependency graph is one-directional so each chip is independently
  fuzzable and benchmarkable.
- **Band-limited audio synthesis** (polyphase BLEP / windowed-sinc, ~81 dB SFDR,
  lookup-table non-linear mixer, 3-stage analog filter chain).
- **15 mappers** including MMC1-MMC5 (with MMC5 audio + ExGrafix), the VRC family
  (1/2/4/6/7), Sunsoft FME-7, and Namco 163 — with the expansion-audio extension
  family (VRC6, Sunsoft 5B, Namco 163, MMC5; VRC7 FM landed in the v0.9.1 stage).
- **Frontend MVP** (winit + wgpu + cpal + egui): save-states + **rewind**, TOML
  input rebinding with an in-app rebind modal, an **egui debugger overlay** (CPU /
  PPU / APU / memory, strictly read-only to preserve determinism), a simplified
  NTSC post-pass, `rfd` file-open + drag-and-drop ROM loading, and gilrs gamepad
  support.
- A six-layer testing strategy with the blargg / kevtris / AccuracyCoin suites as
  the closed-form definition of "cycle-accurate," plus a commercial-ROM regression
  oracle (ROM hash + framebuffer hash + audio hash + cycle count) and a visual
  baseline corpus.

---

## [0.8.6] - 2025-12-29 - Sub-Cycle Accuracy Improvements

**Status**: Phase 1.5 Stabilization - M11 Sub-Cycle Accuracy (Sprints 3-5 Complete, 83%)

This release implements critical sub-cycle accuracy features including DMC DMA cycle stealing, NES open bus behavior, and per-CPU-cycle mapper clocking.

### Highlights

- **DMC DMA Cycle Stealing:** Proper CPU stall handling during DMC sample fetches
- **NES Open Bus Behavior:** `last_bus_value` tracking for hardware-accurate unmapped reads
- **Controller Open Bus:** Bits 5-7 correctly mixed from open bus in controller reads
- **Per-Cycle Mapper Clocking:** `mapper.clock(1)` in `on_cpu_cycle()` for IRQ accuracy
- **Test Suite:** 522+ tests passing (0 failures, 1 ignored doctest, 2 new tests)
- **100% Blargg Pass Rate:** All 90/90 Blargg tests continue to pass

### Added

#### DMC DMA Cycle Stealing (Sprint 3)

- **New Field:** `dmc_stall_cycles` in Bus struct tracks pending CPU stalls
- **Purpose:** DMC sample fetches steal 4 CPU cycles per sample
- **Implementation:** CPU halted during stalls while PPU/APU/mapper continue running
- **Method:** `take_dmc_stall_cycles()` for Console integration
- **Console Integration:** tick() handles stalls with proper component stepping

#### NES Open Bus Behavior (Sprint 4)

- **New Field:** `last_bus_value` in Bus struct tracks last value on data bus
- **Purpose:** NES hardware returns last bus value for unmapped reads (not 0)
- **Unmapped Regions:** $4018-$401F and $4020-$5FFF return open bus value
- **Controller Reads:** $4016/$4017 mix bits 5-7 from open bus with controller data bits 0-4
- **Writes:** Also update bus value (hardware-accurate behavior)
- **New Tests:** `test_open_bus_behavior()`, `test_controller_open_bus_bits()`

#### Per-Cycle Mapper Clocking (Sprint 5)

- **New Call:** `self.mapper.clock(1)` in `on_cpu_cycle()` callback
- **Purpose:** Mappers clocked once per CPU cycle for cycle-accurate timing
- **Critical For:** VRC cycle-based IRQs, MMC3 A12 detection
- **Effect:** Maintains sub-cycle accuracy for all mapper types

### Fixed

- Controller reads now correctly mix bits 5-7 from open bus with controller data
- Unmapped regions return last bus value instead of 0 (hardware-accurate)
- DMC sample fetches now properly stall CPU while other components continue

### Changed

- Bus struct tracks `last_bus_value` for open bus emulation
- Bus struct tracks `dmc_stall_cycles` for DMC DMA handling
- Console tick() handles DMC stalls with proper PPU/APU/mapper stepping
- Mappers clocked via `on_cpu_cycle()` callback for sub-cycle accuracy
- Consolidated unmapped read ranges for cleaner match arm

### Quality Metrics

- cargo clippy: PASSING (zero warnings)
- cargo fmt: PASSING
- cargo test: 522+ tests passing
- cargo build --release: SUCCESS
- Zero unsafe code maintained

---

## [0.8.5] - 2025-12-29 - Cycle-Accurate CPU/PPU Synchronization

**Status**: Phase 1.5 Stabilization - M11 Sub-Cycle Accuracy (Sprints 1 & 2 Complete)

This release implements true cycle-accurate CPU/PPU synchronization, enabling VBlank timing tests to pass with zero-cycle accuracy.

### Highlights

- **Cycle-Accurate Synchronization:** CpuBus trait with on_cpu_cycle() callback for PPU stepping
- **VBlank Timing Tests:** Now pass with +/-0 cycle accuracy (previously failed with +/-51 and +/-10 cycles)
- **cpu.tick() Method:** Cycle-by-cycle CPU execution for sub-instruction timing precision
- **Test Suite:** 520+ tests passing (0 failures, 1 ignored doctest)
- **100% Blargg Pass Rate:** All 90/90 Blargg tests continue to pass

### Added

- **CpuBus Trait:** extends `Bus` with `on_cpu_cycle()` callback so the PPU is stepped 3 dots per CPU cycle before each memory access; NMI captured during the callback and delivered via `cpu.trigger_nmi()`.
- **cpu.tick() Cycle-by-Cycle Execution:** `cpu.tick(&mut bus)` executes one CPU cycle, exposing internal cycle state for sub-instruction-accurate timing.

### Fixed

- **ppu_02-vbl_set_time:** Was +/-51 cycles off, now exact (0 cycles).
- **ppu_03-vbl_clear_time:** Was +/-10 cycles off, now exact (0 cycles).
- **Root Cause:** PPU was only stepped after full CPU instructions completed; solution steps PPU 3 dots BEFORE each CPU memory access via the callback.

### Changed

- Test harness uses CpuBus for cycle-accurate validation while existing Bus-trait tests continue to work.

### Quality Metrics

- cargo clippy / fmt: PASSING; 520+ tests passing; zero unsafe code.

---

## [0.8.4] - 2025-12-28 - CPU/PPU Timing & Version Consistency

**Status**: Phase 1.5 Stabilization - Timing Improvements & Bug Fixes

### Highlights

- **CPU/PPU Timing:** PPU now stepped BEFORE CPU cycle in tick() for accurate $2002 reads at the VBlank boundary.
- **Version Consistency:** Fixed About window and Settings showing outdated version numbers.
- **Documentation:** Fixed clone_mapper doctest by implementing full Clone for mapper Box types.
- **Test Suite:** 517+ tests passing (0 failures, 2 ignored for known architectural limitations); 100% Blargg pass rate.

### Fixed

- **VBlank boundary reads:** reordered tick() to step the PPU 3 dots before each CPU cycle so `$2002` status reads at frame boundaries are accurate (2 VBlank timing tests remain ignored pending a full cycle-by-cycle CPU refactor).
- **Version strings:** updated About window, Settings dialog, CLI `--version`, and Cargo.toml files to 0.8.4.
- **clone_mapper doctest:** implemented Clone for BoxedMapper types so the doctest compiles.

### Quality Metrics

- cargo clippy / fmt: PASSING; 517+ tests passing; zero unsafe code.

---

## [0.8.3] - 2025-12-28 - Critical Rendering Bug Fix

**Status**: Phase 1.5 Stabilization - Critical Bug Fix Release

### Highlights

- **Critical Rendering Fix:** Fixed a framebuffer display showing "4 faint postage stamp copies."
- **Palette Index to RGB Conversion:** NES palette indices (0-63) now correctly converted to RGB via the 64-entry NES_PALETTE lookup table in `update_framebuffer()`.
- **Documentation Improvements:** Changed 3 doctests from `ignore` to `no_run` for compile-time verification.
- **Zero Regressions:** 516+ tests passing, 100% Blargg pass rate maintained.

### Fixed

- **Root Cause:** the framebuffer passed raw palette indices directly as RGBA instead of converting them; the fix performs index -> RGB conversion, restoring proper full-window rendering with the correct palette.

### Changed

- `lib.rs` / `rom.rs` (rustynes-mappers) doctests changed from `ignore` to `no_run` so they are compile-checked during `cargo test`.

### Quality Metrics

- cargo clippy / fmt: PASSING; 516+ tests passing; zero unsafe code.

---

## [0.8.2] - 2025-12-28 - M10-S1 UI/UX Improvements

**Status**: Phase 1.5 Stabilization - M10 Sprint 1 Complete (UI/UX Polish)

This release completes M10-S1, delivering comprehensive desktop GUI polish: theme support, status bar, tabbed settings, keyboard shortcuts, modal dialogs, and visual feedback.

### Highlights

- **Theme Support:** Light/Dark/System themes with persistence and real-time switching.
- **Status Bar:** FPS counter, ROM name display, color-coded status messages with auto-expiry.
- **Tabbed Settings Dialog:** Video/Audio/Input/Advanced tabs with comprehensive tooltips.
- **Keyboard Shortcuts:** Ctrl+O/P/R/Q, F1-F3, M, Escape with consistent behavior.
- **Modal Dialogs:** Welcome screen, error dialogs, confirmation prompts, help window.
- **Zero Regressions:** 508+ tests passing, 100% Blargg pass rate maintained.

### Added

- **Themes:** Light/Dark/System via the egui Visuals API, saved to RON config, applied in real time (`ctx.set_visuals()`), with OS dark-mode detection.
- **Status bar:** real-time FPS (500 ms), ROM filename, color-coded auto-expiring status messages, responsive layout.
- **Tabbed settings:** Video (theme, scale 1-8x, fullscreen, VSync, 8:7 PAR, FPS counter), Audio (mute, volume, sample rate, buffer size), Input (P1/P2 keyboard bindings with reset), Advanced (debug, recent ROMs, app info); Save/Reset buttons; per-setting tooltips.
- **Keyboard shortcuts:** Ctrl+O/Q, Ctrl+P, Ctrl+R/F2, F1/F2/F3 debug windows, M mute, Escape.
- **Modal dialogs:** first-run welcome screen (tracks `first_run`), error dialogs, confirmation prompts, and a Help window with shortcut reference.

### Changed

- Guard-pattern input-handler refactor; improved settings-renderer borrowing; removed unused code; new `AppTheme` enum + `first_run` config field with theme persistence.

### Quality Metrics

- cargo clippy / fmt: PASSING; 508+ tests passing; zero unsafe code.

---

## [0.8.1] - 2025-12-28 - M9 Known Issues Resolution (85% Complete)

**Status**: Phase 1.5 Stabilization - M9 Sprints 1-3 Core Implementation Complete

### Highlights

- **Audio Improvements (S1 Complete):** Two-stage decimation via rubato (1.79 MHz -> 192 kHz -> 48 kHz), A/V sync with adaptive speed adjustment (0.99x-1.01x), dynamic buffer sizing (2048-16384), hardware-accurate mixer with NES filter chain.
- **PPU Edge Cases (S2 Complete):** Sprite overflow bug emulation (false positive/negative matching hardware), palette RAM mirroring at $3F10/$3F14/$3F18/$3F1C, mid-scanline write detection for split-screen, attribute-byte extraction verified.
- **Performance Optimization (S3 Core Complete):** `#[inline]` hints on CPU/PPU hot paths (step(), execute_opcode(), handle_nmi(), handle_irq(), step_with_chr()).
- **Zero Regressions:** 508+ tests passing, 100% Blargg pass rate maintained.

### Quality Metrics

- cargo clippy / fmt: PASSING; 508+ tests passing; zero unsafe code.

---

## [0.8.0] - 2025-12-28 - Rust 2024 Edition & Dependency Modernization

**Status**: Phase 1.5 Stabilization - M9 Sprint 0 Complete (Dependency Upgrade)

Comprehensive dependency modernization: Rust 2024 Edition (MSRV 1.88), eframe/egui 0.33, cpal 0.16, rubato 0.16, ron 0.12, thiserror 2.0, bitflags 2.10.

### Highlights

- **Rust 2024 Edition** adopted across all crates (MSRV 1.75 -> 1.88).
- **GUI/Audio:** eframe 0.33 + egui 0.33, cpal 0.16, NEW rubato 0.16 high-quality resampling for flexible output sample rates.
- **Config/Errors:** ron 0.12, thiserror 2.0, bitflags 2.10.
- **Performance:** `#[inline]` hints on critical audio/rendering paths, buffer-reuse patterns, optimized frame-timing accumulator.
- **Test Suite:** 508+ tests passing (0 failures, 0 ignored).

### Added

- **Audio Resampling:** rubato 0.16 integration for high-quality sample-rate conversion and better cross-platform audio.

### Changed

| Component | Previous | New |
|-----------|----------|-----|
| eframe / egui | 0.29 | 0.33 |
| cpal | 0.15 | 0.16 |
| ron | 0.8 | 0.12 |
| thiserror | 1.x | 2.0 |
| bitflags | 2.4 | 2.10 |
| rubato | - | 0.16 (NEW) |

### Migration Notes

- No breaking changes to user-facing functionality; config files and save states remain compatible. Developers need Rust 1.88+ (`rustup update`); a clean rebuild is recommended.

### Quality Metrics

- cargo clippy / fmt: PASSING; 508+ tests passing; zero unsafe code.

---

## [0.7.1] - 2025-12-27 - Desktop GUI Framework Migration

**Status**: Phase 1.5 Stabilization - GUI Reimplementation Complete

Complete migration of the desktop frontend from Iced+wgpu to eframe+egui for a simpler, more maintainable GUI layer.

### Changed

- **Desktop Frontend:** Iced 0.13 -> eframe 0.29 / egui 0.29 (immediate mode GUI); cpal ring-buffer audio (8192 samples); gilrs gamepad support with hotplug; RON config via the `directories` crate; native file dialogs via `rfd`.

### Added

- **Debug Windows:** CPU (registers, flags, cycle counter), PPU (frame/state overview), APU (audio info, sample buffer, channel overview), and a hex Memory viewer with navigation + ASCII.
- **Menu System** (File, Emulation, Options, Debug, Help), **Settings Dialog** (video/audio/input/debug), accumulator-based 60.0988 Hz NTSC frame timing.

### Removed

- Custom wgpu shader pipeline, Iced view components, the ROM library scanner (to be reimplemented), and unused modules (runahead, metrics, theme, palette).

### Fixed

- Resolved wgpu version conflicts (pixels 0.17 vs egui-wgpu 22) via eframe's bundled solution; clippy warnings; simplified event loop with full frame-timing control.

---

## [0.7.0] - 2025-12-21 - "Perfect Accuracy" (Milestone 8: Test ROM Validation Complete)

**Status**: Phase 1.5 Stabilization - Milestone 8 COMPLETE (100% Blargg Pass Rate)

This release marks the historic completion of Milestone 8 (Test ROM Validation), achieving a **100% pass rate** across all Blargg test suites (CPU, PPU, APU, Mappers) — 500 tests passing, zero failures.

### Highlights

- **100% Blargg Test Pass Rate:** CPU 22/22, PPU 25/25, APU 15/15, Mappers 28/28 (90 total).
- **Cycle-Accurate CPU State Machine:** Complete `tick()` implementation with dummy read timing.
- **CPU Interrupt Handling:** NMI hijacking during BRK; all 5 cpu_interrupts sub-tests passing.
- **PPU Open Bus Emulation:** Data latch with decay counter, correct read-only register handling.
- **CHR-RAM Support:** Fixed a critical design flaw enabling pattern-table writes to mappers.
- **APU Frame Counter:** Immediate clocking on $4017 write; DMC IRQ/DMA fixes.
- **Test Suite:** 500 tests passing (0 failures, 0 ignored).

### Added

- **CPU (22/22):** cycle-accurate `tick()` (1-access-per-cycle discipline, dummy read/write cycles, hardware-accurate RMW timing); NMI hijacking during BRK; IRQ polling between instructions. All cpu_instr / dummy-write / interrupt / timing tests passing.
- **PPU (25/25):** open-bus data latch with 1-second decay; correct $2002 / write-only register behavior; CHR-RAM write routing to the mapper; frame-accurate VBlank/NMI timing; OAM attribute-bit masking; VRAM read-buffer palette behavior.
- **APU (15/15):** frame-counter immediate clocking on $4017; DMC sample-buffer refill + IRQ-acknowledge fixes + 2-stage sample pipeline; timer-period off-by-one and clock-parity fixes.
- **Mappers (28/28):** Holy Mapperel suite across NROM, MMC1, UxROM, CNROM, MMC3 (banking, mirroring, IRQ timing).
- Comprehensive test harnesses (blargg CPU/PPU/APU, holy_mapperel) and an 800+ line M8 technical analysis doc.

### Changed

- CPU refactored from `step()` (instruction-level) to `tick()` (cycle-level); PPU gained open-bus + CHR-RAM routing; APU frame-counter / DMC fixes; test suite at 500 passing with all prior "known limitations" resolved.

### Quality Metrics

- cargo clippy / fmt: PASSING; 500 tests passing, 0 failures, 0 ignored; zero unsafe across all 6 crates.

---

## [0.6.0] - 2025-12-20 - "Accuracy Improvements" (Milestone 7: Complete + M8 Progress)

**Status**: Phase 1.5 Stabilization In Progress - Milestone 7 Complete, Milestone 8 In Progress

Completion of Milestone 7 (Accuracy Improvements) — timing refinements across CPU, PPU, APU, and bus synchronization — plus M8 progress (Blargg CPU tests 90% pass rate). 469 tests passing, zero regressions.

### Highlights

- **M7 Complete:** APU frame-counter precision, hardware-accurate mixer, OAM DMA 513/514-cycle timing.
- **M8 Progress:** Blargg CPU tests 18/20 (90%), up from 13/20 (65%).
- **CPU Timing Fixes:** hardware-accurate dummy read/write cycles, IRQ handling, illegal-opcode fixes.

### Added

- **M8 progress:** all 11 cpu_instr tests + dummy-writes + all-instrs + official-only + instr-timing passing; hardware-accurate dummy reads for implied mode + RMW dummy writes for indexed modes; ATX/LXA (0xAB) fix; RTI IRQ-ack timing; NMI hijacking for BRK. (cpu_dummy_reads + cpu_interrupts #2 documented as known limitations pending cycle-by-cycle CPU.)
- **M7-S1 (CPU):** verified all 256 opcodes' cycle counts, page-crossing penalties, branch timing, RMW dummy writes.
- **M7-S2 (PPU):** `scanline()`/`dot()` accessors, VBlank race-condition handling, exact VBlank flag timing, sprite-0 hit 2/2.
- **M7-S3 (APU):** 4-step quarter-frame timing fix (22371 -> 22372), verified 4/5-step sequences, hardware-accurate non-linear mixer (TND divisors), triangle linear counter.
- **M7-S4 (Timing):** exact 513/514-cycle OAM DMA based on CPU parity; CPU cycle-parity tracking; verified 3:1 CPU/PPU sync.

### Quality Metrics

- cargo clippy / fmt: PASSING; 469 tests passing, 0 failures, 8 ignored; all 136 APU tests passing; zero unsafe code.

---

## [0.5.0] - 2025-12-19 - "Phase 1 Complete" (Milestone 6: Desktop GUI)

**Status**: Phase 1 MVP Complete - All 6 milestones finished

This release marks the historic completion of Phase 1 MVP, delivering the `rustynes-desktop` application — a fully playable NES emulator — 6+ months ahead of the original June 2026 target.

### Highlights

- Cross-platform desktop application with egui/wgpu.
- Real-time 60 FPS NES rendering; cpal audio output with ring buffer.
- Keyboard and gamepad input (gilrs); ROM file browser with format validation.
- Configuration persistence (JSON); playback controls (pause, resume, reset).
- Complete Phase 1 MVP (all 6 milestones); zero unsafe code; 400+ tests passing.

### Added - Desktop GUI (rustynes-desktop)

- **egui application framework:** menu bar (File, Emulation, Help), ROM browser with iNES validation, playback controls, video/audio settings, About dialog.
- **wgpu rendering backend:** Vulkan/Metal/DX12/WebGPU, real-time 256x240 framebuffer, configurable window scaling, VSync, 60 FPS target.
- **cpal audio output:** 48 kHz, configurable buffer, A/V sync, volume control.
- **gilrs gamepad support:** auto-detection, button mapping, dual controllers, keyboard fallback.
- **Configuration system:** JSON persistence for video/audio/input settings, recent ROMs, last directory, window size/position.

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 47/47 | 100% |
| PPU | 85/87 | 97.7% (2 ignored) |
| APU | 136/136 | 100% |
| Mappers | 78/78 | 100% |
| Core | 18/18 | 100% |
| **Total** | **400+** | **100%** |

### Notes

- Phase 1 MVP achieved 6+ months ahead of schedule; 77.7% game compatibility with 5 essential mappers; cross-platform (Linux, Windows, macOS); zero unsafe code.

---

## [0.4.0] - 2025-12-19 - "All Systems Go" (Milestone 5: Integration Complete)

**Status**: Phase 1 In Progress - CPU, PPU, APU, Mappers, and Core Integration complete

Completion of Milestone 5 (Integration), delivering the `rustynes-core` layer that connects all subsystems into a functional NES emulator core (ROM load, frame execution, input, framebuffer output).

### Highlights

- Complete integration layer connecting CPU, PPU, APU, and Mappers.
- Hardware-accurate bus with the full NES memory map; cycle-accurate OAM DMA (513-514 cycles).
- Console coordinator with proper timing synchronization; input shift-register protocol.
- Save-state framework (serialization deferred to Phase 2); 22 new integration tests (398 total); zero unsafe code.

### Added - Core Integration Layer

- **Bus system:** full $0000-$FFFF memory map (RAM + mirrors, PPU/APU registers + mirrors, OAM DMA, controller ports, cartridge space), cycle-accurate OAM DMA with dummy-cycle alignment, mapper integration (mirroring conversion, scanline IRQ, A12 edge notification).
- **Console coordinator:** 3 PPU dots per CPU cycle, APU step per cycle, NMI (VBlank) + mapper IRQ + DMA-stall handling; public API for ROM load, single-step, frame-step, framebuffer access, dual-controller input, reset.
- **Input system:** hardware-accurate shift-register protocol (strobe latch, serial readout, open-bus behavior); dual-controller state management.
- **Save-state framework:** 64-byte header format ("RNES" magic, version, CRC32, ROM SHA-256, timestamp, frame count) with mismatch/checksum/I-O error handling.

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 46/46 | 100% |
| PPU | 83/83 | 100% |
| APU | 150/150 | 100% |
| Mappers | 78/78 | 100% |
| Core | 41/41 | 100% |
| **Total** | **398/398** | **100%** |

### Notes

- The emulator core is now functionally complete; save-state serialization deferred to Phase 2; zero unsafe code across all 5 crates.

---

## [0.3.0] - 2025-12-19 - "Mapping the Path Forward" (Milestone 4: Mappers Complete)

**Status**: Phase 1 In Progress - CPU, PPU, APU, and Mappers implementation complete

Completion of Milestone 4 (Mappers): a complete mapper subsystem enabling 77.7% NES game-library compatibility via the 5 most important mappers.

### Highlights

- Trait-based mapper framework with 5 mappers: NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4).
- 77.7% NES game-library coverage; full iNES + NES 2.0 ROM support; scanline IRQ (MMC3 A12 edge detection); battery-backed SRAM interface.
- 78 tests passing; zero unsafe code; 3,401 lines of production code.

### Added - Mappers Implementation

- **Framework:** 13-method Mapper trait (PRG/CHR read/write with banking, dynamic mirroring, IRQ generation/ack, CPU/PPU clock callbacks, PRG-RAM for battery saves); complete iNES + NES 2.0 parser (12-bit mapper numbers, size/battery/trainer/mirroring detection); all mirroring modes; a factory registry.
- **NROM (0)** - 9.5% coverage: no banking, 16/32KB PRG, 8KB CHR-ROM/RAM.
- **MMC1 (1)** - 27.9% coverage: 5-bit serial shift register, 4 PRG modes, 2 CHR modes, programmable mirroring, 8KB battery SRAM.
- **UxROM (2)** - 10.6% coverage: 16KB switchable + fixed PRG, 8KB CHR-RAM, bus-conflict emulation.
- **CNROM (3)** - 6.3% coverage: fixed PRG, 8KB switchable CHR, bus-conflict emulation.
- **MMC3 (4)** - 23.4% coverage: 8 bank-select registers, 2 PRG + 2 CHR modes, scanline-counter IRQ with A12 edge detection, PRG-RAM protection, 8KB battery SRAM.

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 46/46 | 100% |
| PPU | 83/83 | 100% |
| APU | 150/150 | 100% |
| Mappers | 78/78 | 100% |
| **Total** | **357/357** | **100%** |

### Notes

- Library-only release (no GUI yet); hardware-accurate mapper implementation with cycle-accurate MMC3 A12 IRQ timing; zero unsafe code across all 4 crates.

---

## [0.2.0] - 2025-12-19 - "The Sound of Innovation" (Milestone 3: APU Complete)

**Status**: Phase 1 In Progress - CPU, PPU, and APU implementation complete

Completion of Milestone 3 (APU): a complete, hardware-accurate 2A03 Audio Processing Unit with all 5 channels, a non-linear mixer, and a configurable resampler — cycle-accurate, zero unsafe code.

### Highlights

- Complete 2A03 APU with all 5 audio channels; hardware-accurate non-linear mixer with lookup tables.
- Configurable resampler (1.79 MHz -> 48 kHz) with low-pass filter; 4-step and 5-step frame-counter modes.
- Flexible DMA interface for DMC sample playback; complete $4000-$4017 register implementation.
- 150 tests passing (136 unit + 14 doc); zero unsafe code.

### Added - APU Implementation

- **Core components:** frame counter (4-/5-step, cycle-accurate, optional IRQ), envelope generator, length counter (32-entry table), sweep unit.
- **Channels:** Pulse 1 & 2 (4 duty cycles, 11-bit timer, envelope, sweep, length counter), Triangle (32-step sequence, linear counter, ultrasonic silencing), Noise (15-bit LFSR, long/short modes, 16-entry period table), DMC (1-bit delta modulation, 7-bit output, 16 sample rates, DMA interface, IRQ, loop, $FFFF->$8000 wrap).
- **Output:** non-linear mixer (hardware mixing formulas, range 0.0-~2.0), linear-interpolation resampler with ring buffer, optional low-pass filter.
- **System integration:** $4015 status register, $4017 frame-counter control, memory-callback DMA interface.

### Changed

- Reorganized `to-dos/` into a phase-based hierarchy (4 phases, 18 milestones).

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 46/46 | 100% |
| PPU | 83/83 | 100% |
| APU | 150/150 | 100% |
| **Total** | **279/279** | **100%** |

### Notes

- Library-only release; APU is hardware-accurate and cycle-precise; DMC DMA via a simple memory callback; zero unsafe code across CPU/PPU/APU.

---

## [0.1.0] - 2025-12-19 - "Precise. Pure. Powerful." (First Official Release)

**Status**: Phase 1 In Progress - CPU and PPU implementation complete

The **first official release** of RustyNES, completing Milestone 1 (CPU) and Milestone 2 (PPU) — a world-class foundation with 100% CPU test pass rate and 97.8% PPU pass rate.

### Highlights

- World-class CPU with 100% nestest.nes validation; cycle-accurate PPU at 97.8% pass rate.
- Complete 6502 CPU with all 256 opcodes (151 official + 105 unofficial); full 2C02 PPU with VBL/NMI timing and sprite rendering.
- 144 comprehensive tests passing (56 CPU + 88 PPU); zero unsafe code; 44 test ROMs acquired for validation.

### Added

- **Milestone 1 (CPU):** cycle-accurate 6502/2A03, all 256 opcodes, all 13 addressing modes with cycle-accurate timing, complete interrupt handling (NMI, IRQ, BRK, RESET), page-crossing penalties, 100% nestest.nes golden-log match, 46 unit tests.
- **Milestone 2 (PPU):** dot-level 2C02 rendering (341 dots x 262 scanlines), background rendering with Loopy scrolling, sprite rendering (8-per-scanline), sprite-0 hit, sprite overflow, cycle-accurate VBlank/NMI timing, OAM DMA, complete register implementation, palette RAM with mirroring, VRAM nametable mirroring, 83 unit tests.
- **Documentation & Organization:** Phase 1 TODO tracking, M1/M2 milestone docs, M3 (APU) / M4 (Mappers) overviews, CPU test-ROM README, `game-roms/` directory.

### Changed

- Reorganized test-ROM structure (CPU files to `test-roms/cpu/`); updated documentation references, README status, and .gitignore.

### Test Results

| Component | Tests | Pass Rate |
| --------- | ----- | --------- |
| CPU | 56/56 | 100% |
| PPU | 88/90 | 97.8% (2 ignored) |
| **Total** | **144/146** | **98.6%** |

### Notes

- Library-only release (no GUI yet); two PPU tests ignored pending cycle-accurate timing refinement (not failures); CPU is world-class (100% nestest golden-log match); zero unsafe code.

---

### Project Setup - 2025-12-18

#### Added

- Initial project structure with 10 workspace crates.
- Comprehensive documentation suite (73 markdown files, 52,402 lines): CPU/PPU/APU/mapper specifications, API reference, development guides (BUILD, CONTRIBUTING, TESTING, DEBUGGING), format specifications (iNES, NES 2.0, NSF, FM2).
- GitHub project templates (issue/PR templates, Code of Conduct, contributing guidelines, security policy, support docs).
- Development infrastructure (Dependabot, CODEOWNERS) and project documentation (README, ROADMAP, ARCHITECTURE, OVERVIEW, CHANGELOG).

---

## Development Phases

RustyNES followed a phased development approach. See [ROADMAP.md](ROADMAP.md) for complete details.

### Phase 1: MVP (delivered v0.1.0 -> v0.5.0)

- [x] Cycle-accurate 6502/2A03 CPU implementation
- [x] Dot-level 2C02 PPU rendering
- [x] Hardware-accurate 2A03 APU synthesis
- [x] Mappers 0, 1, 2, 3, 4 (77.7% game coverage)
- [x] Cross-platform desktop GUI (egui + wgpu)
- [x] Save states and battery saves
- [x] Gamepad support

### Phase 1.5: Stabilization (delivered v0.6.0 -> v0.8.6)

- [x] 100% Blargg test-ROM pass rate
- [x] Cycle-accurate CPU/PPU synchronization
- [x] Sub-cycle accuracy (DMC DMA, open bus, per-cycle mapper clocking)
- [x] Rust 2024 Edition modernization
- [x] Desktop UI/UX polish

### Phase 2 and beyond: Cycle-Accurate Core (delivered v0.9.0 -> v1.0.0)

The original Phase 2-4 roadmap goals were delivered by integrating a new
cycle-accurate engine (the `v0.9.x` documentary lineage above):

- [x] Master-clock-precise scheduler, 100% AccuracyCoin (139/139)
- [x] Integrated debugger (CPU, PPU, APU viewers)
- [x] Rewind, run-ahead, dynamic-rate audio
- [x] RetroAchievements integration (rcheevos)
- [x] GGPO rollback netplay (2-4 players, native + WebRTC)
- [x] TAS recording/playback
- [x] WebAssembly build + live GitHub Pages demo
- [x] Expansion audio (VRC6, VRC7, MMC5, FDS, N163, 5B)
- [x] Famicom Disk System + Vs. System / PlayChoice-10
- [x] 51 mapper families
- [ ] Mobile platform support (Android, iOS) - next

---

## Changelog Conventions

### Categories

- **Added**: New features
- **Changed**: Changes to existing functionality
- **Deprecated**: Soon-to-be removed features
- **Removed**: Removed features
- **Fixed**: Bug fixes
- **Security**: Security vulnerability fixes

### Versioning

RustyNES follows [Semantic Versioning](https://semver.org/):

- **MAJOR** version (X.0.0): Incompatible API changes, major features
- **MINOR** version (0.X.0): Backward-compatible functionality additions
- **PATCH** version (0.0.X): Backward-compatible bug fixes

---

## Links

- [Project Repository](https://github.com/doublegate/RustyNES)
- [Issue Tracker](https://github.com/doublegate/RustyNES/issues)
- [Discussions](https://github.com/doublegate/RustyNES/discussions)
- [Documentation](https://github.com/doublegate/RustyNES/tree/main/docs)

---

**Note**: This changelog is actively maintained as the project progresses, following the Keep a Changelog format.
</content>
</invoke>
