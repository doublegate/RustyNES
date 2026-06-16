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

Work toward **v1.3.0 "Bedrock"** (beta.1-4 / rc): toolchain modernization (Rust edition
2024, MSRV → latest stable, and the egui 0.34 / wgpu 29 / rfd 0.17 dependency tier), the
frame-pacing measurement/judder fix, GeraNES-class developer tooling (PPU event viewer,
symbol-file loading, memory compare, trace annotation), mapper breadth (an aggressive
BestEffort sweep 87 → 100+ and Vs. DualSystem), HD-pack conditions + background-region
replacement, niche peripherals + netplay desync diagnostics, and casual-mode browser
RetroAchievements. The hard-tier accuracy residuals were **re-baselined**:
`cpu_interrupts_v2` is now strict-pass (closed by the master clock); the remaining three
(`mmc3_test_2/4` #3, two `apu_reset` cases) share one fractional-master-clock root cause
and stay deferred as a future v2.0-scale item (see `docs/STATUS.md`). See the v1.3.0 plan.

### Added

- **14 new mapper families (87 → 101 coverage)** (v1.3.0 Workstream D1 mapper sweep,
  `sprint8.rs`): mappers 29 (Sealie RET-CUFROM), 31 (INL NSF-style / "2A03 Puritans"),
  58 (multicart), 60 (reset-based 4-in-1 multicart), 94 (UN1ROM), 101 (Jaleco JF-10 CHR
  latch), 107 (Magic Dragon), 111 (GTROM / Cheapocabra), 143 (Sachen TCA01), 177 / 179
  (Hengedianzi + variant), 218 (Magic Floor), 231 (20-in-1 multicart), and 234 (Maxi 15 /
  BNROM-like multicart). All are Tier-2 **BestEffort** discrete / homebrew / multicart
  boards ported from the GeraNES reference — no IRQ, no expansion audio, no per-cycle / A12
  hook (`MapperCaps::NONE`) — register-decode + save-state unit-tested only and explicitly
  **not** accuracy-gated (the AccuracyCoin / commercial-ROM oracle never references them).
- **Vs. DualSystem header detection** (v1.3.0 Workstream D2): the NES 2.0 byte-13 high
  nibble (Vs. hardware type 5/6) is now parsed into `Header`/`Cartridge.vs_dual_system`
  and exposed as `Nes::is_vs_dual_system()`, so the frontend's "DualSystem not yet
  emulated" note fires for properly-headered DualSystem ROMs, not only the four
  SHA-256-DB-known dumps. The two-CPU/two-PPU *emulation* remains a documented v2.0
  deferral (no committable test-ROM oracle; see `docs/STATUS.md` + the design audit).
- **Memory Compare (cheat-hunt memory search)** debugger panel (v1.3.0 Workstream C, C3).
  A classic emulator memory search over the 2 KB CPU work RAM (`$0000-$07FF`): snapshot a
  baseline, then iteratively narrow a candidate set by how each byte moved since the last
  snapshot — changed / unchanged / increased / decreased / equals-value — until one
  address remains (feed it to the raw-RAM cheat panel). Read-only (samples via the
  side-effect-free `cpu_bus_peek`; never writes the core, determinism unaffected), opened
  from **Debug → Memory Compare**, and disabled under RetroAchievements hardcore mode like
  the Memory viewer + cheat panel.

### Changed

- **Menu-bar reorganization + Settings auto-save** (v1.3.0 Workstream C, UI). The
  top-level menu order is now **File / Emulation / View / Tools / Debug / Help**.
  Items were regrouped for discoverability: a **Close ROM** entry and a grouped
  **Save States** submenu (Save/Load State, Active Slot, Save-/Load-to-Slot, Manage
  States) in File; **Swap Disk Side** moved File → Emulation; **NSF Player** moved
  Debug → Tools; **Performance Monitor** moved Tools → Debug; and the standalone
  "Mod" menu folded into a Tools **HD Pack** submenu (still `hd-pack`-feature +
  native-gated). The Settings window gains a dedicated **Shaders** tab (the
  composable shader stack, split out of Video) and renames "Advanced" → **Emulation**
  (run-ahead + rewind); it now snapshots the config each frame and persists on any
  change, so **every setting in every tab auto-saves**. The redundant debugger
  checkbox toolbar was removed (the menu bar already surfaces every panel). Pure UI;
  no determinism surface. (The egui-0.34 "menu lingers until several clicks" report
  is documented in `ui_shell::menu_bar` pending an on-device repro to pin the exact
  `MenuState` trigger — not hacked blind.)
- **Rust edition 2021 → 2024** (v1.3.0 Workstream A, toolchain modernization). The
  whole workspace now compiles on the 2024 edition. The migration was mechanical and
  determinism-neutral: `extern "C"` FFI blocks became `unsafe extern "C"`
  (`rustynes-cheevos`), the `disasm` opcode-table macro pins its fragment specifiers
  to `expr_2021` (preserving 2021 macro-matching), one `gen` local became the raw
  identifier `r#gen` (`gen` is a reserved keyword in 2024), and a redundant
  block-return brace was removed. Imports were reformatted to the 2024 rustfmt style.
  No `tail_expr_drop_order` restructuring was needed. **Verified byte-identical:**
  AccuracyCoin 100% (139/139), `visual_regression` golden framebuffers, `nestest`
  0-diff, and `cpu_interrupts_v2` 5/5 strict all pass unchanged; the chip stack still
  cross-compiles `no_std` (`thumbv7em-none-eabihf`).
- **MSRV 1.86 → 1.96 + the egui / wgpu / rfd dependency tier** (v1.3.0 Workstream A).
  The pinned toolchain (`rust-toolchain.toml`), `[workspace.package] rust-version`, the
  two explicit-version crates, and the CI MSRV jobs all move to **Rust 1.96** (latest
  stable). That unblocks the coordinated UI-stack bump: **egui / egui-wgpu / egui-winit
  0.32 → 0.34.3**, **wgpu 25 → 29.0.3** (egui 0.34 requires wgpu 29), naga 25, and
  **rfd 0.14 → 0.17.2**. The wgpu 29 migration is presentation-only (no core/determinism
  surface): `get_current_texture()` now returns the `CurrentSurfaceTexture` enum (handled
  via a small `gfx::PresentError` that preserves the reconfigure-on-lost/outdated
  behavior), `RenderPassColorAttachment` gains `depth_slice`, `RenderPassDescriptor` and
  `RenderPipelineDescriptor` use `multiview_mask`, `PipelineLayoutDescriptor` uses
  `immediate_size` + `Option`-wrapped bind-group layouts, samplers use `MipmapFilterMode`,
  and `InstanceDescriptor` / `DeviceDescriptor` follow the new constructors. egui 0.34
  deprecations were migrated (`Context::run` → `run_ui` with a root `Ui`, `Panel::top/
  bottom(..).show_inside(..)`, `content_rect`, `global_style`, `egui_wants_*_input`,
  `RendererOptions`). The newer 1.96 clippy's lints were cleaned across the workspace
  (collapsible let-chains, `map_unwrap_or`, `manual_is_multiple_of`, etc.). **Verified:**
  AccuracyCoin 100% (139/139), `visual_regression` golden + `nestest` unchanged; clippy
  `-D warnings` clean (native + `scripting,hd-pack` + both wasm flavours); `no_std`
  cross-compile; `trunk build --release` succeeds with the wasm size budget at 3.06 / 5.00
  MiB gzip (wasm-bindgen pin 0.2.125 unchanged).
- **CI: GitHub Actions runner/action versions bumped to latest** (v1.3.0 Workstream A):
  `actions/upload-artifact` v4 → v7; all other actions (`checkout@v6`,
  `configure-pages@v6`, `deploy-pages@v5`, `upload-pages-artifact@v5`,
  `action-gh-release@v3`, `taiki-e/install-action@v2`, `Swatinem/rust-cache@v2`,
  `dtolnay/rust-toolchain@master`) and the `*-latest` runner images already track newest.

### Fixed

- **`.zip` / soft-patched ROMs passed on the command line now load** (a v1.2.0 ingest
  bug). The CLI / initial-ROM path (`App::new`, used by `rustynes <rom>`) read the file
  and parsed the raw bytes directly, skipping the `.zip` extraction + same-stem
  `.ips`/`.ups`/`.bps` soft-patching that the menu / drag-drop / recent-ROM path
  (`load_rom_from_path`) performs — so `rustynes game.zip` failed with "rom magic bytes
  do not match `NES\x1A`". The ingest preprocessing is now factored into a shared helper
  that both paths use, so a zipped or patched ROM on argv reaches the deterministic parse
  and the CRC-keyed per-game database as the extracted/patched image, identical to every
  other load path. (Regression-tested.)
- **Performance panel "presented" cadence no longer shows phantom judder** (v1.3.0
  Workstream B1). The present-to-present interval was timestamped *after*
  `surface.present()` returned, so it folded GPU-submit + vsync-gate +
  coalesced-`RedrawRequested` jitter into the series — the graph bottomed out and
  "rushed to catch up" while the steadier "produced" (emulation) series stayed flat.
  It is now timestamped at the `RedrawRequested` display-refresh signal (still only on
  an actual present), so present-to-present deltas reflect the display's true visible
  cadence; any small, steady offset from "produced" is just the NTSC 60.0988 Hz rate
  beating against the display refresh. Measurement-only (presentation/instrumentation;
  no core or determinism surface). The Performance panel's "presented" legend gains a
  tooltip explaining the semantics; see `docs/frontend.md`.
- **Performance panel: present/produce "beat" diagnostic** (v1.3.0 Workstream B, B3
  groundwork). Added duplicate-present and dropped-produce counters (`presented_dups` /
  `produced_dropped`, also in the CSV log) surfaced in the panel beside the existing
  present-mode + pacer-anomaly readouts — read-only instrumentation (no pacer change).
  Under display-sync both stay ~0; under wall-clock they tick ~once every ~10 s for the
  NES 60.0988 Hz vs 60.000 Hz display beat, so a sudden burst (real judder) is now
  distinguishable from the harmless inherent beat. This is the signal for deciding
  whether the deeper present-mode / pacer mitigation is worth its regression risk.

### Security

- **Resolved all 10 open CodeQL code-scanning alerts.** (1) Added a least-privilege
  top-level `permissions: contents: read` to `ci.yml` and `security.yml` — the 9
  `actions/missing-workflow-permissions` alerts (one per job; every job is read-only
  build/test/lint/audit) — so neither workflow's `GITHUB_TOKEN` carries default write
  scopes. (2) Fixed the one high-severity `py/incomplete-url-substring-sanitization` in
  `scripts/download_missing_nesdev_pages.py`: the opensearch-host decision used a
  `"mediawiki.org" in url` substring test (which a crafted host like
  `mediawiki.org.example.com` or a query string would satisfy); it now parses the URL and
  compares the actual hostname (`== "mediawiki.org" or endswith(".mediawiki.org")`).

## [1.2.0] - 2026-06-15 - "Curator" (Feature Release)

**v1.2.0 "Curator" is a broad, additive feature release** on the cycle-accurate v1.0.0
core (shipped through v1.1.0 "Scriptable"). Theme: **library breadth + compatibility +
reach** — getting more games to load, run correctly, and be playable anywhere, plus the
polish to curate them. Mapper coverage rises 51 → **87 families** behind a CI-enforced
accuracy-tiering honesty gate; ROMs now load from `.zip` and auto-apply
`.ips` / `.ups` / `.bps` soft-patches; a per-game database + in-app ROM-Database editor
correct region / mapper / mirroring; the video stack gains live NTSC knobs, a composable
shader stack + CRT preset bank, and a (default-off) HD-pack loader; new peripherals
(Family BASIC keyboard, SNES mouse, Arkanoid-both-ports, Game Genie code DB) join the
input layer; the Lua engine gains `onNmi` / `onIrq` / `setInput`; the menu bar gets
contextual enable/disable, a remappable shortcut registry, and Font Awesome icons; the
web build gains on-screen touch controls + a Power Pad feed + an experimental piccolo
wasm-Lua backend; netplay ships a turn-key `deploy/` bundle; and a manual/release-only
PGO CI promotion gate lands. **Every addition is off-by-default or additive, so the
shipped / native / `no_std` / wasm builds stay byte-identical and AccuracyCoin holds
100% (139/139), `nestest` 0-diff.** The SMB3 World 1-1 sprite flicker (a PPU
OAM-row-corruption model bug) is fixed via a faithful TriCNES eval-pointer port, and
Mapper 89 now models its bus conflict. ADRs **0011** (mapper tiering), **0012** (wasm-Lua
piccolo backend), **0013** (composable shader stack).

### Added

- **PGO / BOLT CI promotion gate** (v1.2.0 Workstream G — performance
  infrastructure, no core behavior change). A new manual-/release-only
  `.github/workflows/pgo.yml` (`PGO`) gates the previously-unused
  `scripts/pgo/run.sh` recipe into CI. Triggered by `workflow_dispatch` (with
  optional `frames` / `run_bolt` inputs) and on push of a release tag (`v*`) —
  never per-PR (the instrument + train + rebuild cycle is too slow for the PR
  gate). It builds the plain-release `full_frame` baseline, runs the PGO recipe
  (instrument → train on the seven committed CC0/MIT ROMs → optimized rebuild),
  re-benches, and **promotes the PGO binary only when BOTH** (a) it beats plain
  release by **> 3%** on the `full_frame` Criterion mean (same-runner A/B) AND
  (b) the full `--features test-roms` oracle — AccuracyCoin 139/139, `nestest`
  0-diff, blargg/kevtris, golden-framebuffer `visual_regression`, the APU
  mixer/volume audio suites — is **byte-identical under the PGO codegen**
  (`cargo pgo optimize test`). An optional Linux-only `bolt` job runs behind the
  same gate (best-effort; skips cleanly when `llvm-bolt` is unavailable). A
  failed gate is informational and never blocks a release. Documented in
  `docs/performance.md` §"Profile-guided optimization (PGO)". No Rust changed;
  the shipped/wasm/`no_std` builds are unaffected.
- **Experimental wasm Lua scripting via a piccolo backend** (v1.2.0 Workstream
  F4; default OFF, behind the new `script-wasm` feature; see
  `docs/adr/0012-wasm-lua-piccolo-backend.md`). `rustynes-script` now sits behind
  a `VmBackend` trait with two compile-time-selected backends: the native
  **mlua** Lua 5.4 engine (the crate-default `mlua-backend` feature, what the
  frontend's `scripting` feature pulls in — **byte-identical to v1.1.0**) and an
  experimental pure-Rust **piccolo** VM that compiles to
  `wasm32-unknown-unknown` with no C toolchain. piccolo's `Fuel` maps onto the
  per-frame instruction budget. On wasm the engine is loadable from the browser
  (`rustynes_load_script` / `rustynes_stop_script` JS bridge) and supports
  `emu.onFrame`, `emu.read`/`peek`/`readRange`, `emu.cpu`/`frame`/`cycle`,
  `emu.log` + `print`, the overlay draws, and gated `emu.write`. It is
  **explicitly NOT byte-parity** with the native mlua engine (a different VM) —
  acceptable because scripts are observational/overlay + gated writes and are
  never part of the framebuffer/audio determinism oracle. The per-access
  (`onExec`/`onRead`/`onWrite`) and per-interrupt (`onNmi`/`onIrq`) replay
  callbacks are a documented native-only limitation (registered as no-ops on
  piccolo). All native builds — shipped, the default wasm flavours, and the
  `no_std` chip stack — are byte-identical to before (piccolo is never pulled
  unless `script-wasm` is explicitly enabled). The native 13-test
  `rustynes-script` suite is unchanged; the piccolo backend adds 4
  backend-specific tests (deferred-write-lands, deferred-write-gated,
  fuel-budget, native-only-callbacks-are-no-ops).
- **Web/wasm input parity — on-screen touch controls + Power Pad** (v1.2.0
  Workstream F1 + F2). The browser build gains a translucent Pointer-Events
  touch overlay (pure DOM/CSS in `web/index.html`, zero Rust binary weight
  beyond the new `web-sys` pointer/touch feature flags): an on-screen D-pad /
  A / B / Start / Select with multi-touch + pointer-capture handling, a
  selectable target **port** (player 1-4, for Four Score), and a 12-button
  **Power Pad** mat. The overlay drives a shared `wasm_touch` thread-local
  bridge that BOTH wasm frontends read at the SAME deterministic late-latch a
  keypress uses — the `wasm-canvas` embed folds it into the per-frame
  `set_buttons` / `set_power_pad` call, and the `wasm-winit` path ORs it into
  `FrameInputs` so it flows through `EmuCore::latch` exactly like a keyboard
  bit. Touch input is therefore recorded/replayed identically by TAS movies +
  netplay and adds no new determinism surface. The native Power Pad latch arm
  (previously `cfg(not(wasm32))`-gated behind the mouse/cursor block) is
  narrowed so the Power Pad — which needs only a `u16` mask — also feeds on
  wasm; Zapper / Vaus stay native-gated. The native build is byte-identical
  (all touch state is wasm-only). On-device touch UX needs a browser on a
  touch device to verify.
- **Menu-bar responsiveness — per-item contextual enable/disable** (v1.2.0
  Workstream H, H1; GeraNES `MenuUI.inl`-inspired). Menu items now grey out
  when the action would be a no-op or unsafe in the current state, instead of
  being always-clickable. Predicates threaded from live app state into the
  shell: with **no ROM loaded** Save/Load State (+ slots + the Save-States
  manager), Reset, Power-cycle, Frame Advance, Speed, FDS disk-swap, Vs. Insert
  Coin, ROM Database, and HD-pack load/unload are disabled; during a **netplay
  session** Open ROM / Open Recent / Reset / Power-cycle / Frame Advance / Vs.
  coin / Movies (TAS) / HD-pack are locked and the Speed submenu allows only
  100% (mirrors GeraNES `netplayRomChangeRestricted` / `isNetplaySpeedRestricted`);
  while a **TAS movie is recording or playing** Load State (+ load-from-slot),
  Reset, Power-cycle, disk-swap, Netplay, and HD-pack are locked, and the
  Movies submenu disables the conflicting Record-vs-Play actions (mirrors
  GeraNES `replayInteractionLocked` / `replayRecordingActive`). Pure UI — the
  `MenuAction` dispatch set is unchanged.
- **Remappable system-hotkey registry surfaced in the rebind UI** (v1.2.0
  Workstream H, H2). The `[input.system]` config section already drives both
  the global hotkey handler and the menu's inline accelerator labels; the
  Settings -> Input rebind panel now exposes **all** of those bindings (Open
  ROM, Pause, Frame Advance, Fast Forward, Fullscreen, Toggle Menu Bar, Speed
  up/down/reset, Movie record/play/branch, FDS disk-swap, Vs. Insert Coin) for
  rebinding, not just the original seven — so a rebind takes effect live and
  the menu accelerator label updates to match. Every field keeps its
  `#[serde(default)]`, so existing configs and the default build are
  byte-identical (a new `shortcut_registry_defaults_are_byte_identical` test
  pins this).
- **Menu icons** (v1.2.0 Workstream H, H3; GeraNES `withMenuIcon`). Font Awesome
  6 Free **Solid** glyphs now precede every top-level menu (File / Emulation /
  Tools / Mod / View / Debug / Help) and their items, alongside the H1
  enable/disable state and H2 accelerator labels. The icon font
  (`assets/fonts/fa-solid-900.ttf`, SIL OFL-1.1, license shipped beside it) is
  embedded via `include_bytes!` and registered with egui as a trailing fallback
  family, so ordinary UI text is untouched and any missing glyph degrades to a
  box rather than crashing (`crate::icons`). Measured against
  `scripts/wasm_size_budget.sh`, the full font **fit** the 5 MiB gzip wasm-deploy
  budget (total 2.78 MiB gzip, 2.22 MiB headroom; ~13 KB gzip added), so the
  **same full font ships on native and both wasm flavours** — no per-target
  subsetting was needed. The lightweight `wasm-canvas` embed has no egui menu and
  is unaffected. Pure UI; the core and the `MenuAction` dispatch set are
  unchanged.

- **Mapper accuracy tiering** (v1.2.0 Workstream A). Every supported mapper
  family is now classified `Core` / `Curated` / `BestEffort` by a single
  `const fn mapper_tier(id, submapper)` (`rustynes-mappers::tier`). The tier is
  an honesty marker — runtime behaviour is identical — that keeps the accuracy
  claim precise as long-tail coverage grows: a CI invariant forbids any
  `BestEffort` mapper from backing an AccuracyCoin / oracle ROM. ADR 0011.
- **Nine curated (Tier-1) mapper families** (v1.2.0 Workstream A): 38 (Bit Corp
  UNL-PCI556), 41 (Caltron 6-in-1), 79 (AVE NINA-03/06), 86 (Jaleco JF-13), 113
  (NINA-006 / MB-91, register-controlled mirroring), 140 (Jaleco JF-11/14), 232
  (Camerica Quattro / BF9096), 240 (C&E), and 241 (BxROM-like). Each is a
  discrete-logic board with register-decode unit tests.
- **Twenty-seven best-effort (Tier-2) mapper families** (v1.2.0 Workstream A):
  the aggressive long-tail sweep ported from the GeraNES / Mesen2 references —
  15, 36, 39, 61, 62, 72, 77, 92, 96, 97, 132, 133, 145, 146 (sprint6) and 147,
  148, 149, 150, 180, 185, 200, 201, 202, 203, 212, 213, 214 (sprint7). Mostly
  multicart / Sachen / discrete boards. Register-decode unit-tested only and
  **not** accuracy-gated (see the tiering note). Total mapper coverage rises
  from 51 to **87 families** (51 Core + 9 Curated + 27 BestEffort).
- **Per-game database — region / mapper / submapper overrides** (v1.2.0
  Workstream B). The CRC32-keyed per-game DB grew from a `(crc, Mirroring)`
  table to a full `GameDbEntry` (region / mapper / submapper / mirroring /
  title) parsed from the vendored TetaNES columns. Region / mapper / submapper
  corrections apply by rewriting the iNES (or NES 2.0) header before the core
  parses it — frontend-only and idempotent, so the determinism firewall holds
  (the core test suites never patch). Mirroring / Vs. corrections continue
  through the existing post-construction setters.
- **In-app ROM-database editor** (v1.2.0 Workstream B, B4): a new **Tools -> ROM
  Database** panel shows the loaded ROM's effective per-game entry (user overlay
  merged over the vendored base, keyed on the ROM CRC32) and lets you edit
  mirroring / region / mapper / submapper / title. Edits persist to an editable
  user-overlay file (`<data-dir>/game_db_user.txt`) that overrides the vendored
  base; the mirroring override applies live, the rest at the next ROM load.
  "Reset to Default" reverts to the vendored entry. Native; frontend-only.
- **ROM soft-patching** (v1.2.0 Workstream B): a same-stem `.bps` / `.ups` /
  `.ips` patch sitting beside a ROM is auto-applied at load (in that
  precedence), before format detection, so the patched image flows through the
  deterministic parse unchanged — save-states / netplay / oracle all see the
  patched bytes. UPS and BPS verify their in-format source and target CRC32s.
  Native; a malformed patch is surfaced and the unpatched ROM still loads.
- **`.zip` ROM loading** (v1.2.0 Workstream B): a `.zip` opened as a ROM has its
  first NES / FDS / NSF entry extracted in memory and loaded as usual (a
  same-stem soft-patch beside the archive still applies). Native-only (`zip`
  crate, `deflate`-only to reuse the existing flate2/miniz_oxide); the wasm
  builds stay byte-identical (the dep is in the `cfg(not(wasm))` table).
- **NTSC per-knob live tuning** (v1.2.0 Workstream C1): the true-composite
  (`composite-rt`, Bisqwit) NTSC filter's Contrast / Saturation / Brightness /
  Hue are now live `[graphics]` settings (`ntsc_contrast`, `ntsc_saturation`,
  `ntsc_brightness`, `ntsc_hue`), adjustable from sliders in Settings -> Graphics
  and persisted to `config.toml`. Promoted from baked WGSL constants to a
  per-frame uniform: the YIQ matrix is rebuilt in-shader from the knobs each
  frame. Output-only (the deterministic core framebuffer and AccuracyCoin /
  oracle results are unaffected). All four default to `0.0` (Bisqwit's neutral
  values), at which the in-shader decode is bit-identical to the previous
  hardcoded coefficients — existing configs and the default build are
  byte-for-byte unchanged.
- **Composable shader stack + CRT preset bank** (v1.2.0 Workstream C2,
  `GeraNES`-`ShaderPass`-inspired). The single-select CRT / NTSC / composite-rt
  post-process filter is now an ordered, composable `ShaderStack`: enabled passes
  render by ping-ponging two NES-resolution intermediate render targets, with the
  final pass blitting the letterboxed image to the swapchain. A new
  Settings -> Graphics -> "Shader stack (composable)" section adds / removes /
  reorders / toggles passes and exposes each pass's tunable knobs as generic
  sliders, parsed from RetroArch-style `#pragma parameter` shader headers (new
  `rustynes-frontend::shader_pass` module). A built-in CRT preset bank (Sharp /
  Classic / Heavy-Aperture, all reusing the existing CRT shader) plus
  Save / Load / Delete persist named stacks under `[graphics.shader_presets]`.
  The Bisqwit `composite-rt` pass is special-cased (it consumes the `R16Uint`
  palette-index texture, so it is only honoured as the first pass). Frontend /
  presentation-only — the deterministic core framebuffer and AccuracyCoin /
  oracle results are unaffected. **An empty / all-disabled stack (the default,
  and what any pre-C2 `config.toml` deserializes to) falls through to the exact
  pre-C2 direct blit — the default presented image is byte-for-byte unchanged.**
  Rejected as out of scope: a RetroArch `.slangp` importer / GLSL->WGSL
  translation. ADR 0013.
- **HD-pack / mod loader — minimal first cut** (v1.2.0 Workstream C3,
  Mesen-HD-pack-inspired; native-only, behind the default-OFF `hd-pack` cargo
  feature). A new per-pixel **tile-source telemetry** export in the PPU
  (`rustynes_ppu::HdTileSource`, gated on `rustynes-ppu/hd-pack`) records, for
  each visible pixel, the CHR pattern-table tile base address, the final
  palette, the sprite flip flags, and whether the pixel came from a sprite or
  the background — written in `emit_pixel` in lockstep with the existing
  `index_framebuffer`, observing only already-computed state. It is
  **output-only**: it reads no new VRAM, issues no new A12 / mapper events,
  mutates no emulation state, and is not part of the save-state. A new frontend
  `hdpack` module loads a Mesen-style `hires.txt` (folder or `.zip`), parses the
  supported subset (`<scale>`, `<patternTable>`, and **unconditional** CHR-hash
  `<tile>` rules), hashes each rendered tile's 16 CHR bytes with the
  Mesen-compatible CRC32, and substitutes hi-res replacement images at blit time
  (a dedicated upscaled-RGBA blit path in `gfx`). A **Mod -> Load HD Pack** menu
  entry enables a pack per-game (keyed on `rom_sha256`, persisted under
  `[graphics] hd_packs`). Out of scope (deferred, not v1.2.0): conditions,
  palette keys, background regions, HD audio, and a `<patternTable>`-bank
  substitution path. **With the `hd-pack` feature OFF the shipped / wasm /
  `no_std` builds are byte-identical to today; with it ON but no pack loaded the
  presentation is also byte-identical** — proven by the full ROM corpus
  (AccuracyCoin 139/139, `nestest` 0-diff, blargg / kevtris green) passing
  identically with the feature on and off.
- **Family BASIC keyboard** (v1.2.0 Workstream D1): a new
  `InputDevice::FamilyKeyboard` core device implements the Famicom keyboard's
  9-row x 2-column-half matrix scan — `$4016` write selects the column-half
  (bit 0), clocks the row counter (bit 1 rising edge advances, low resets), and
  enables the matrix (bit 2); `$4017` read returns the four selected key
  switches on bits 4..1, active-low. A new `Nes::set_family_keyboard` setter
  (mirroring `set_power_pad`) feeds the 72-key bitmap. The frontend maps host
  keys via `input::family_keyboard_index` (a direct, one-to-one passthrough; a
  faithful positional layout is a follow-up) and offers it as the player-2
  device in the input rebind panel. Unit-verified matrix scan; save-state
  round-trips (snapshot tag 5).
- **SNES-style serial mouse + Arkanoid on both ports** (v1.2.0 Workstream D2): a
  new `InputDevice::SnesMouse` core device implements the 32-bit MSb-first D0
  report (signature nibble `0b0001`, sensitivity, left/right buttons, signed
  7-bit X/Y movement, idles high after the report), with a `Nes::set_snes_mouse`
  setter; the frontend derives per-frame movement from the cursor delta and maps
  the left/right mouse buttons. The Arkanoid **Vaus** paddle is now selectable on
  **either** port (the core already permits `set_paddle` on port 0). Both are
  selectable as the player-2 expansion device. Unit-verified serial protocol;
  save-state round-trips (mouse snapshot tag 4). Konami / Bandai Hyper Shot, the
  Family Trainer, and the Subor keyboard are noted follow-ups (not implemented).
- **Game Genie code-name database** (v1.2.0 Workstream D3): a small committed,
  CRC32-keyed asset (`genie_database.tsv`, public Galoob / community code lists)
  maps a ROM to named Game Genie codes. The cheat panel shows a pick-list of the
  loaded ROM's matching codes; selecting one feeds the **existing**
  `GenieCode` decode + cheat persistence. Pure frontend — the core Game Genie
  substitution (`rustynes-core/src/genie.rs`) is unchanged; every offered code is
  validated through `GenieCode::new`, so the no-cheat PRG-read path is
  byte-identical. The asset is compiled in for both native and wasm (modest size;
  commercial ROMs are never committed — only the codes).
- **Determinism contract held for all of Workstream D**: every new input device
  is additive and OFF by default (`ExpansionDevice::None` stays the default; new
  `FrameInputs` / `SharedInput` / config fields are `#[serde(default)]` and
  default to "no device / no keys"), and the core input additions are pure /
  deterministic. The shipped build + AccuracyCoin **(139/139)** + `nestest`
  0-diff + blargg / kevtris are byte-identical to before; the no_std chip stack
  still cross-compiles to `thumbv7em-none-eabihf`.
- **Lua scripting depth — `emu.onNmi` / `emu.onIrq`** (v1.2.0 Workstream E, E1,
  T-110-E1). Two new observational callbacks fire once per interrupt the CPU
  serviced that frame (`fn(vector)`). They tap the **committed** service commit
  point (`Bus::notify_irq_service`, the same point the IRQ trace uses) — not the
  speculative `poll_nmi` / `poll_irq` sampler ADR 0010 flagged as unreliable — so
  a script sees exactly the interrupts that committed, in order, classified by the
  fetched vector (`0xFFFA` ⇒ NMI, `0xFFFE` ⇒ IRQ/BRK, robust under NMI hijack). A
  new `debug-hooks`-gated per-frame interrupt-service log on `Nes` mirrors
  `exec_log` (cleared at the top of `run_frame`; `interrupt_log()` /
  `set_interrupt_logging()` accessors); the engine enables it only while an
  `onNmi`/`onIrq` callback is registered.
- **Lua scripting depth — `emu.setInput` wired through the late-latch** (v1.2.0
  Workstream E, E2, T-110-E2). `emu.setInput(port, buttons)` now applies a per-
  port controller override at the deterministic late-latch (`EmuCore::latch`,
  before `produce_one_frame`) — the *same* point a real keypress enters — so a
  recorded / replayed session stays bit-identical. The override is one-shot per
  call. It is **gated identically to `emu.write`**: the engine drops the command
  at the source under a locked session and the host re-checks the identical
  `netplay_locked || movie_locked` condition (which folds in RA-hardcore) before
  storing it, so a script can never perturb a netplay / TAS-replay / RA-hardcore
  run. (Supersedes the v1.1.0 "accepted but not applied" stub + its one-time
  console warning.)
- **Determinism contract held for Workstream E (E1 + E2)**: the interrupt log is
  `debug-hooks`-gated (a zero-cost no-op when off) and the scripting integration
  is behind the default-OFF `scripting` feature, so the shipped / wasm / no_std
  builds are byte-identical to today. AccuracyCoin **(139/139)** + `nestest`
  0-diff + blargg / kevtris stay green; the no_std chip stack still cross-compiles
  to `thumbv7em-none-eabihf`.

### Changed

- **Hosted browser-netplay deploy made turn-key + honest** (v1.2.0 Workstream
  F3, deploy + verify-prep). The existing `deploy/` bundle (signaling server +
  Caddy TLS proxy + coturn STUN/TURN) is now deployable with no source edits:
  the `Dockerfile` builds the correct `rustynes-netplay` crate (was a stale
  `nes-netplay`), a workspace-root `.dockerignore` (referenced but previously
  missing) keeps `target/` + ROMs + docs out of the image context, and the
  per-deploy values (`DOMAIN`, `TURN_USER`/`TURN_SECRET`/`TURN_REALM`) come from
  a `.env` (new `deploy/.env.example` template; coturn credential/realm injected
  as CLI flags from env so `turnserver.conf` carries no checked-in secret). All
  stale `RustyNES v2` / `nes-frontend` references scrubbed; `README.md` now uses
  clearly-placeholder values (`signaling.example.com`, `TURN_SECRET=changeme`).
  `docs/netplay-webrtc.md` §3.4/§4 flipped from "Pending" to
  **deployment-ready; live verification pending the maintainer's hosted run**
  (explicitly **not** "Verified"), and a copy-pasteable **manual verification
  checklist** (2-tab → 2-machine → 4-player matrix + ops/DNS/TLS steps + the
  TURN-bandwidth ops caveat) added to `deploy/README.md`. Documented that
  browser netplay needs **no COOP/COEP / SharedArrayBuffer** (DataChannel +
  AudioWorklet). Docs/deploy only — no Rust changes; the `[netplay]`
  `signaling_url` / `stun_servers` config defaults were already correct and stay
  byte-identical. Live multi-browser netplay verification remains the
  maintainer's manual step (not run here).

### Fixed

- **PR #75 review hardening (beta.2 bot-review follow-up).** Adopted the
  worthwhile bot-review findings from the merged beta.2 PR:
  - *Security (path traversal):* HD-pack replacement-image filenames parsed
    from `hires.txt` are now sanitised (`hdpack::sanitize_image_name`) — a
    malicious pack can no longer reference `../../etc/passwd`, an absolute path,
    or any path with separators / drive prefixes to escape the pack directory;
    such rules are rejected.
  - *Security (zip bomb / OOM):* `hdpack::read_zip_entry` now caps a single
    archive entry at 64 MiB (declared size **and** actual read bounded),
    matching `app.rs::extract_rom_from_zip`.
  - *Panic guard:* `SnesMouseState::enc_axis` no longer panics on `i16::MIN`
    (used `unsigned_abs` instead of the overflowing `-v`).
  - *Save-state robustness:* `FamilyKeyboardState::from_parts` clamps a restored
    `row` to the matrix bound, so a corrupt/malicious save-state cannot drive an
    out-of-bounds index in `read()`.
  - *Lock discipline:* the HD compositor's CPU-heavy upscale/tile-hash/blit now
    runs **after** the emu lock is dropped — only the framebuffer, PPU
    tile-source telemetry, and the 8 KiB CHR pattern space are snapshotted under
    the lock.
  - *Replay-lock consistency:* the Load-State **hotkey** and `MenuAction`
    dispatch now honour the same movie-record/playback lock the File menu greys
    the item under (previously bypassable via the bound key).
  - *Perf (default path):* the Bisqwit NTSC WGSL skips the per-fragment
    `cos()`/`sin()` hue rotation when `hue == 0` (the default), keeping the
    default present byte-identical (the C1 `default_knobs_match_legacy_matrix`
    guard still passes).
  - *Docs:* corrected the `InputDevice::FamilyKeyboard` doc reference
    (`input::family_keyboard_index`, not a nonexistent `FAMILY_KEYBOARD_KEYMAP`)
    and the `hash_tile` doc to state it hashes raw unflipped CHR bytes (it does
    not consult `flip_h`/`flip_v`).
- **SMB3 (and any MMC3/other game using a mid-scanline `$2001` split) — sprite
  flicker.** The PPU OAM-row-corruption model keyed the corrupted row off the
  raw PPU dot (`dot >> 1`), so *Super Mario Bros. 3*'s mid-scanline rendering
  disable (its HUD split) intermittently flagged Mario's OAM row and wiped his
  sprite, flickering him in/out (~26% of frames) in World 1-1. Replaced with a
  faithful port of TriCNES's eval-pointer model: the corruption index is the
  live secondary-OAM evaluation pointer (`OAM2Address`), captured at the
  rendering-disable edge during dots 1-64 and committed on re-enable. Default
  builds stay byte-identical for games without such a split; AccuracyCoin's
  OAM-Corruption test (0x047B) and the full 139/139 suite, nestest 0-diff, and
  blargg/kevtris remain green. `docs/ppu-2c02.md` edge-case 2 updated.
- **RetroAchievements badge decode** now validates PNG dimensions (from the
  IHDR header) *before* allocating the decode buffer, so a crafted/corrupt
  badge declaring huge dimensions can no longer OOM. (Behind the off-by-default
  `retroachievements` feature.)
- **Lua scripting** surfaces a failed instruction-budget hook install
  (`mlua::set_hook`, fallible since 0.11) as a `ScriptError` instead of
  silently running scripts without the runaway-guard. (Behind the off-by-default
  `scripting` feature.)
- **Mapper 89 (Sunsoft-2) — *Tenka no Goikenban: Mito Koumon*** now models the
  board's **bus conflict** (a register write is ANDed with the PRG-ROM byte at
  the written address, per nesdev `INES_Mapper_089` "BUS CONFLICTS"). Without it
  the raw value selected the wrong CHR/PRG bank and the background nametable
  never latched. Isolated to mapper 89 (its only dump); AccuracyCoin and all
  other titles unaffected.

### Dependencies

- Dependency modernization (v1.2.0 beta.1). Merged the safe Dependabot bumps
  (naga 28, toml 1.1, gilrs 0.11, tokio-tungstenite 0.29) and `cargo update`'d
  the rest of the compatible tree (image 0.25.9, tiff 0.10, windows 0.62, …).
  Migrated the breaking bumps: **png 0.18** (decoder `Read + Seek` + fallible
  `output_buffer_size`), **criterion 0.8** (`std::hint::black_box`), **mlua
  0.11** (`set_hook` now fallible), **cpal 0.18** (`SampleRate` is a `u32` alias;
  `StreamConfig` is `Copy` + passed by value). `rfd 0.17` is deferred — its
  sync xdg-portal/libdbus backend fails to compile on Rust 1.86 (an upstream
  rfd bug), so rfd stays at 0.14 pending a fixed release. The egui / egui-wgpu /
  egui-winit + wgpu UI-stack cluster bump (**egui 0.29 → 0.32, wgpu 22 → 25,
  naga → 25**) landed as one coordinated upgrade; `deny.toml` now allows the
  `Ubuntu-font-1.0` license that egui 0.32's bundled default font declares.

## [1.1.0] - 2026-06-15 - "Scriptable" (Feature Release)

The first feature release after the v1.0.0 production cut. It folds in the
(never-separately-tagged) **v1.0.1** compatibility + hygiene work and all four
**v1.1.0** feature workstreams: visual filters (full NES_NTSC composite + a
CRT / scanline shader pass + `.pal` palette loading), input & peripherals
(NES Power Pad, turbo / autofire, an input-display overlay, and a per-game
nametable-mirroring override database), debugger devtools (breakpoints, a cycle
trace logger, an event viewer), audio (NSF / NSFe music player, 5-band graphic
EQ), and the flagship **Lua scripting engine** (sandboxed
Lua 5.4, Mesen2 / FCEUX-style `emu` API). Additive only — the determinism
contract and **AccuracyCoin 100%** hold; every new state-mutating path
(Lua writes, new cheats) is gated off in netplay / TAS replay / RA-hardcore.

### Added

- **Lua scripting — frontend integration** (v1.1.0 beta.3, Workstream E flagship, T-110-E5 —
  **completes Workstream E**). The Lua engine is now usable from the app (behind the
  default-OFF, native-only `scripting` feature): a **Lua Script console** (Debug → Lua Script)
  loads / reloads / stops a `.lua` file and shows its log + errors + `onFrame` callback count.
  The engine is pumped once per redraw under the emu lock with the live `Nes`; script overlay
  draws (`drawText`/`drawRect`/`drawPixel`) render through the egui pass, and control commands
  apply via the existing pause / save-state path. **Write-gating** is wired: `emu.write` is
  disabled during netplay, TAS replay/record, and RA-hardcore (the cheat-path policy). Ships
  with `examples/scripts/` (`hud.lua`, `ram_watch.lua`) and a full `docs/scripting.md` API
  reference. The feature is off by default, so the shipped/wasm/no_std builds are byte-identical
  (`setInput` application + pixel-perfect overlay mapping + `onNmi`/`onIrq` are documented
  follow-ups).
- **Lua scripting API — callbacks, control & overlay** (v1.1.0 beta.3, Workstream E flagship,
  T-110-E2 — engine surface). Extends the Lua engine with the full callback + control + draw
  surface: `emu.onExec(addr,fn)` / `onRead(addr,fn)` / `onWrite(addr,fn)` (dispatched by
  replaying the frame's exec PCs and a new gated bus-access log — reads + writes + values),
  control commands `emu.pause` / `saveState` / `loadState` / `setInput`, and an overlay draw API
  `emu.drawText` / `drawRect` / `drawPixel`. Control + draw requests are *collected* (drained by
  the host) so the host stays the sole owner of emulator control and can gate state-mutating
  actions. The bus-access log is a new `debug-hooks` tap (output-only, off by default, cleared
  per frame), so determinism / AccuracyCoin are unaffected (reverified byte-identical). The
  frontend script console + overlay rendering + write-gating wiring + `examples/scripts/` +
  `docs/scripting.md` land in the next beta.3 PR.
- **Lua scripting engine** (v1.1.0 beta.3, Workstream E flagship, T-110-E1..E4 — foundation).
  A new `rustynes-script` crate embeds sandboxed **Lua 5.4** (vendored `mlua`) and exposes a
  Mesen2 / FCEUX-style `emu` API: `emu.read` / `readRange` / `write`, `emu.cpu()` (A/X/Y/S/P/PC),
  `emu.frame` / `cycle`, `emu.log`, and `emu.onFrame(fn)` per-frame callbacks. The engine is
  **host-driven** — the frontend calls it once per frame and binds live-`Nes` accessors via
  `mlua::Lua::scope` (see ADR 0010). **Sandboxed** (only `table`/`string`/`math`/`coroutine`;
  no `io`/`os`/`package`/`require`/`debug`/`load`/`dofile`) with a per-frame VM-instruction
  budget against runaway scripts. **Determinism-safe:** the crate is pulled in only behind the
  frontend's (off-by-default) `scripting` feature — so the shipped build, the wasm build, and
  the `no_std` cross-compile never compile it and stay byte-identical — and `emu.write` pokes
  only system RAM after `run_frame` (cheat-path mechanism) and is gated off via
  `set_writes_locked` (netplay / TAS / RA-hardcore). Headless API + sandbox-escape + budget
  tests. (The frontend script console, `onExec`/`onRead`/`onWrite` event callbacks, the overlay
  draw API, save-state/input control, `examples/scripts/`, and `docs/scripting.md` land in the
  next beta.3 PR.)
- **Graphic equalizer** (v1.1.0 beta.2, Workstream D, T-110-D2). An optional 5-band graphic
  EQ (60 / 240 / 1k / 3.8k / 12k Hz, ±12 dB each) in Settings → Audio. It runs as a
  **frontend output stage** — cascaded RBJ peaking biquads applied on the producer side after
  the dynamic-rate resampler, exactly like the existing master-gain stage — so it never
  touches the deterministic core synthesis. Off by default and bypassed when flat, so audio
  is **byte-identical** to before unless you engage it; live-updates while playing (the
  Settings sliders push params through the shared audio queue and the producer rebuilds its
  biquads on the next frame). Native-only for now (a wasm feed is a follow-up).
- **NSF / NSFe music player** (v1.1.0 beta.2, Workstream D, T-110-D1). RustyNES now plays
  `.nsf` chiptune files: drop one in (or File → Open) and an **NSF Player** panel opens with
  the file's title / artist / copyright and a track selector (Prev / Next / Restart + a
  song-index slider). Mechanism follows Mesen2 / FCEUX — a parsed NSF builds a synthetic
  `NsfMapper`: the program image is mapped (with `$5FF8-$5FFF` 4 KiB bank-switching), a tiny
  6502 driver is served at `$5000`, and the reset / NMI vectors point at it. The driver runs
  `init` for the selected song, enables vblank NMI, then spins; the ordinary 60 Hz NMI calls
  `play` once per frame. Playback therefore runs through the **unchanged** `Nes::run_frame`
  lockstep loop (new `Nes::from_nsf` / `nsf_set_song` construction path; cartridge games are
  untouched and stay byte-identical). Scope: base 2A03 APU, NTSC 60 Hz. Expansion-chip audio
  (VRC6/7, MMC5, N163, 5B, FDS), exact non-60 Hz play rates, and a wasm NSF loader are
  documented follow-ups.
- **Event viewer** (v1.1.0 beta.2, Workstream C, T-110-C3). A new **Events** debugger panel
  (Debug → Event Viewer) plots the frame's CPU writes — PPU (`$2000-$3FFF`), APU
  (`$4000-$4017`), and mapper (`$4020-$FFFF`) — on a scanline×dot grid coloured by kind,
  so you can see *when* in the frame a game touches scroll / mapper / APU registers. Built
  on the `debug-hooks` event log (a single tap in the bus write path, tagged with the live
  PPU position, reset per frame); output-only and off by default, so determinism /
  AccuracyCoin are unaffected. (NMI/IRQ markers are a follow-up.)
- **Cycle trace logger** (v1.1.0 beta.2, Workstream C, T-110-C2). A new **Trace** debugger
  panel (Debug → Trace Logger) records each executed instruction's CPU register file +
  cycle into a bounded ring (50k), shows a live disassembled tail, and **exports** the
  full trace to a text file. Built on the `debug-hooks` feature; the ring is output-only
  (bounded, mutates no emulation state) and off by default, so determinism / AccuracyCoin
  are unaffected and headless builds keep a byte-identical hot path.
- **Debugger breakpoints** (v1.1.0 beta.2, Workstream C, T-110-C1). Exec/PC breakpoints:
  add addresses in the CPU debugger panel's new **Breakpoints** section (armed toggle,
  hex add, per-row remove, clear); when the program counter reaches one, emulation
  **pauses and the CPU panel opens** on the stopped PC. Built on a new off-by-default
  `debug-hooks` core feature (a break-check at the top of the `run_frame` loop) — the
  hook is output-only (stops the partial frame + records the PC, mutating nothing), so
  determinism / AccuracyCoin hold even with it on, and the headless test/bench builds
  (which omit the feature) keep a byte-identical hot path. Read/write watchpoints +
  conditions are a follow-up.
- **NES Power Pad — playable** (v1.1.0 beta.1, Workstream B, T-110-B1). The Power Pad /
  Family Fun Fitness mat is now selectable as the player-2 expansion device (Settings →
  Input "Port 2 device"); its 12 mat buttons default to a left-hand grid (`1`–`4` /
  `Q W E R` / `A S D F`, chosen to avoid the P1 and system-speed keys). Completes the
  device shipped in the previous beta entry: the held mat
  buttons flow through `InputState` → `FrameInputs` (+ the emu-thread `SharedInput`) and
  are fed to the device in `EmuCore::latch`. Off by default (standard controller), so
  the default + Four Score paths stay byte-identical. Native path (like Zapper/Vaus);
  rebindable mat keys + a wasm-canvas feed are follow-ups.
- **Per-game database — nametable mirroring override** (v1.1.0 beta.1, Workstream B,
  T-110-B4). A CRC32-keyed game database (vendored from TetaNES, MIT OR Apache-2.0,
  ~2.6k entries) that auto-corrects ROMs whose iNES header carries the wrong mirroring
  flag: at load the frontend computes the ROM's CRC32 (over PRG-ROM+CHR-ROM) and, if
  listed, applies a nametable mirroring override via `Nes::set_mirroring_override`. The
  override is implemented in the bus's nametable translation (uniform across all
  mappers, no per-mapper edits), does not touch mapper-supplied VRAM (4-screen), and is
  persisted in the save-state (so rollback / restore stay consistent). It is
  **frontend-only** and **`None` by default**: the core test suites construct the `Nes`
  directly and never consult the database, so AccuracyCoin / the commercial oracle stay
  byte-identical. Deterministic (same CRC ⇒ same mirroring), so netplay peers agree.
  (Region / mapper overrides and the Game Genie code-name database are tracked
  follow-ups.)
- **NES Power Pad device — core** (v1.1.0 beta.1, Workstream B, T-110-B1, core of 2).
  The 12-button Power Pad / Family Fun Fitness mat as an opt-in per-port `InputDevice`
  overlay: `PowerPadState` implements the dual-8-bit-shift-register serial protocol
  (the `NESdev` / Mesen bit layout — buttons read out LSb-first on `$4017` bits 3 and
  4, with the standard `$4016` strobe), exposed via `Nes::set_power_pad(port, buttons)`
  and round-tripped in the save-state. Opt-in: with no device attached the
  standard-controller + Four Score read paths stay **byte-identical**, so determinism /
  AccuracyCoin / the oracle are unaffected and the `no_std` core is unchanged. Protocol
  unit-verified (button→serial-position mapping, strobe-reload, save-state round-trip).
  (The frontend key mapping that drives the mat landed in the same beta — see "NES
  Power Pad — playable" above.)
- **Turbo / autofire** (v1.1.0 beta.1, Workstream B, T-110-B2). The A and/or B button
  can rapid-fire while held, configurable via `[input] turbo_a` / `turbo_b` and a
  `turbo_period` (frames per on/off half-cycle; Settings → Input "Turbo / autofire").
  **Off by default** (empty mask = byte-identical input). The strobe is applied where
  input meets the NES — keyed on the **emulated frame number** (a new pure
  `Nes::frame()` accessor) — in `EmuCore::latch` and on the local input in both netplay
  paths, so the **gated bits are what get latched / recorded / sent**: it is
  deterministic and rollback / TAS / netplay-safe, AccuracyCoin / the oracle are
  unaffected, and the `no_std` core is unchanged. (The lightweight `wasm-canvas` embed
  does not apply turbo; the native + wasm-winit paths do.)
- **Input-display overlay** (v1.1.0 beta.1, Workstream B, T-110-B3). A new
  read-only tool panel (`debugger/input_display_panel.rs`) that draws a stylized NES
  controller per active player — D-pad, Select/Start, B/A — with each currently-held
  button lit. Open from **Tools → Input Display** or the debugger toolbar's "Input
  HUD" checkbox; shows P1+P2 (and P3/P4 with Four Score). Useful for TAS authoring and
  streaming. It reads the same held-button snapshot the emulator is fed (pushed each
  frame via `DebuggerOverlay::set_input_display`), so it is frontend-only with no core,
  produce-path, or determinism impact.
- **True composite NES_NTSC filter** (v1.1.0 beta.1, T-110-A1, stage 2 of 2 — the
  shader). A faithful GPU port of Bisqwit's `nes_ntsc` algorithm
  (`crates/rustynes-frontend/src/ntsc_bisqwit.rs`): it reconstructs the analog
  composite signal from the PPU's palette-index framebuffer and demodulates it back to
  RGB with a windowed Y/I/Q filter, so genuine NTSC artifacts (chroma dot-crawl, colour
  fringing on vertical edges, the saturated-hue "checkerboard") fall out of the math
  rather than being faked. Selected via a new `[graphics] ntsc_filter = "composite-rt"`
  mode (Settings → Display); the existing `"composite"` / `"rgb"` remain the simplified
  blur. The index framebuffer is uploaded as an `R16Uint` texture (read via
  `textureLoad`, no sampler — WebGL2-safe) only while the filter is active, snapshotted
  under the same brief present lock as the framebuffer; all signal/sine/YIQ/emphasis
  tables are computed in Rust and baked into the WGSL as `var<private>` arrays (no
  storage buffers → WebGL2-safe). Render priority is CRT > true-NTSC > simplified blur.
  Off by default = byte-identical presentation; frontend-only (the parallel index
  buffer is read-only output) → AccuracyCoin / determinism / the commercial oracle are
  unaffected, and the `no_std` core is unchanged. WGSL parse+validate and LUT/sine unit
  tests in CI; native + both wasm flavours clippy clean. Uses Bisqwit's neutral
  defaults (per-knob tuning is a later pass).
- **NES_NTSC composite filter — core foundation** (v1.1.0 beta.1, T-110-A1, stage 1
  of 2). The cycle-accurate PPU now emits, alongside the RGBA framebuffer, a parallel
  per-pixel **palette-index** framebuffer (256×240 `u16`s, each the 9-bit
  `(emphasis << 6) | colour` value) plus a per-frame **NTSC colour phase** (0..=2, the
  `videoPhase` source of the dot-crawl, derived from a master-cycle counter). Exposed
  as `Ppu::index_framebuffer()` / `Ppu::ntsc_phase()`, routed through
  `Bus`/`Nes::index_framebuffer()` + `ntsc_phase()`. These are **output-only**: they
  carry exactly the LUT index used to produce the displayed RGBA (proven by a unit
  test that `rgba_lut[index] == framebuffer[pixel]` for every emitted pixel), feed no
  emulation logic, and are not part of the save-state — so the determinism /
  AccuracyCoin contract is unaffected, and the `no_std` chip stack still cross-compiles
  against `core` + `alloc`. The composite encode→decode WGSL shader that consumes this
  index buffer follows in stage 2.
- **CRT / scanline video filter** (v1.1.0 beta.1). A new presentation-layer wgsl
  post-pass (`crates/rustynes-frontend/src/crt.rs`) that applies source-row-space
  scanlines (a parabolic per-row brightness profile) + a subtle RGB aperture-grille
  mask, with a live **scanline-intensity** slider (`[graphics] crt_scanline`,
  default 0.5) and an on/off toggle (`[graphics] crt_filter`, default off) in the
  Settings → Display tab. Off by default = byte-identical presentation; mutually
  exclusive with the NTSC filter (CRT wins). A frontend-only effect — no core /
  framebuffer change, so AccuracyCoin + determinism are unaffected. The embedded
  WGSL is parse+validate-tested in CI.
- **Custom `.pal` palette loading** (v1.1.0 beta.1). Load a 64-entry `.pal` palette
  file (192-byte form; longer files use the first 64 colours) to re-tint the
  display via the PPU's colour LUT. `rustynes-ppu` gains `build_rgba_lut_from_base`
  - `Ppu::set_custom_palette` (applying the standard 2C02 composite emphasis to a
  custom base table), routed through `Nes::set_custom_palette`. The frontend adds a
  `[graphics] palette_file` config, a Settings → Display **Load .pal… / Built-in**
  picker (native), and re-applies the configured palette on every ROM load. Default
  (none) is **byte-identical** to the built-in palette — proven by a unit test that
  `build_rgba_lut_from_base(&NES_PALETTE)` equals the built-in composite LUT — so
  AccuracyCoin + the commercial oracle are unaffected. Native-only file I/O (a
  no-op on wasm).

### Fixed

- **Lua callback registry is isolated Rust-side (structural crash-proofing).** The
  `onFrame`/`onExec`/`onRead`/`onWrite` callbacks are stored Rust-side as `mlua::RegistryKey`s
  (a `Vec` + per-address `HashMap`), **not** in a script-visible Lua global. A script can register
  callbacks but cannot inspect, clobber, or inject junk into the registry, so no malformed
  registry value can ever error the host pump — the protection is *structural*. The per-address
  callback replay gates on the Rust `HashMap` keys (an O(1) check, no Lua FFI for the ~75k
  non-matching exec/access events per frame). Test `callback_registry_is_not_script_visible`
  asserts `__rustynes` is `nil` to scripts. (This was reached via a review cycle, #49–#57, that
  first hardened a script-visible `__rustynes` global traversal-by-traversal against junk keys /
  values / non-functions, then moved the registry Rust-side to remove the surface entirely.)
  Core untouched (AccuracyCoin byte-identical); ADR 0010 + `docs/scripting.md` updated.
- **Lua scripting — self-review hardening (M2/L1–L4).** A code-review pass on the Workstream-E
  code: (M2) `pump_scripts` holds the emulator lock only around `on_frame` (the log/control/
  draw drains moved outside it) and the default per-frame instruction budget dropped 5M→1M, so a
  runaway script stalls emulation far less; (L1/L3) the script overlay now maps onto the actual
  letterboxed game rect (8:7 PAR + overscan aware) instead of stretching to the window, and the
  NSF panel notes its ~60 Hz tempo approximation; (L2) `emu.setInput` logs a one-time
  "accepted but not yet applied" notice instead of being a silent no-op; (L4) non-primitive
  `emu.log` args render as their Lua type name, not a `{:?}` debug dump. Core untouched
  (AccuracyCoin byte-identical).
- **Bot-review sweep round 2 (PRs #38–#48)** — applied the actionable
  `gemini-code-assist` / Copilot suggestions from the v1.1.0 feature PRs:
  - **`emu.read` / debugger peek no longer has side effects** (Copilot #46): `Nes::peek`
    now uses the bus's `debug_peek_cpu`, so a script/debugger read of `$2002` doesn't clear
    the VBL flag and `$2007` doesn't advance the read buffer — restoring the determinism
    guarantee. (AccuracyCoin reverified byte-identical.)
  - **`onExec` no longer replays stale/duplicate PCs** (gemini #47, *critical*): the Lua
    `onExec` callback now reads a dedicated **per-frame exec-PC log** (`Nes::exec_log`, cleared
    each frame) instead of the 50k rolling trace buffer, and is **independent** of the Trace
    Logger panel's recording (Copilot #48).
  - **Lua replay no longer crosses the FFI per instruction/access** (gemini #47): a Rust-side
    active-address set gates the Lua lookup, so only addresses with a registered callback pay
    the cost.
  - **`emu.readRange` is bounded** (gemini/Copilot #46): capped at 64 KiB with `wrapping_add`,
    so a script can't OOM/overflow the host; control/draw command queues are per-frame capped.
  - **Breakpoints fire at a frame's starting PC** (gemini #41): replaced the blind
    "skip first iteration" with a precise `skip_breakpoint_at`, so a breakpoint at the PC after
    a reset / save-state load / manual change still triggers; and a hit now **stops the
    catch-up burst** so the core doesn't advance past the stop frame (Copilot #41).
  - **Event viewer captures `$4014` OAM DMA + `$4016` strobe** (Copilot #43): the tap now covers
    the whole `$4000-$4017` I/O window its legend advertises.
  - **Trace panel disassembly can't panic** (gemini/Copilot #42): the 3-byte peek window uses
    `.get().unwrap_or(0)` instead of an out-of-range `& 3` index.
  - **NSF programs loaded at `$6000-$7FFF` now play** (gemini #44): a non-bankswitched NSF
    serves its program there before falling back to WRAM.
  - Lua engine hardening: `pump_scripts` takes a single emu lock (gemini #48), a failed
    (re)load clears the previous script first (gemini #48), the overlay honours a non-zero
    `screen_rect().min` (gemini #48), the dead `ScriptError::Runtime` variant was removed, and
    the wasm/native `cfg` gates on the overlay path + console availability were made consistent
    (Copilot #48). Default build remains byte-identical.
- **CRT / NTSC filters now letterbox + aspect-correct like the main blit**
  (v1.1.0 beta.1, review feedback on T-110-A1/A2). The CRT, simplified-NTSC, and
  true-composite-NTSC post-passes scaled the oversized fullscreen triangle's clip-space
  **position**, which re-introduced the bottom/fullscreen edge-smear the main blit
  documents (and they ignored 8:7 pixel-aspect correction + overscan crop). All three
  now use the same proven UV-space letterbox + clip-to-black + overscan-crop as the main
  blit (the shared `gfx::letterbox_uniform`), so a filtered picture has correct aspect,
  honours "Hide Overscan" / "8:7 Pixel Aspect", and shows clean black bars instead of
  smeared edge texels.
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

- **v1.0.1 compatibility + hygiene patch** (docs-only; zero core change, AccuracyCoin stays
  100%). Audited the v1.0.1 plan: most hygiene was already complete (phase-7/8 to-do folders
  archived, all 24 `#[ignore]`s carry permanent-by-design reasons, the roadmap banner frames the
  `v1.x`/`v2.x` markers as engine lineage, `deny.toml` allows the shipped license set, no open
  Dependabot PRs). This patch closes the remainder: the present-tense `[→]` ticket markers under
  the already-"SUPERSEDED" Phase 6 roadmap section are flipped to historical `[~]` with a note
  that they record the *then-current* state (not live TODOs), so the roadmap no longer reads as
  if accuracy work is ongoing. The three flagged compatibility gaps (Mito Koumon mapper-89 render,
  FDS *Kid Icarus* side-B stall, GxROM "Mario flashing") are documented (not force-fixed — their
  dumps are gitignored / they need interactive verification and the fixes would risk the 100%
  core). Full audit in `to-dos/v1.0.1-compat-hygiene/00-patch.md`.
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
  - **rAF display-sync** on ~60 Hz panels (eliminating the wall-clock-vs-rAF beat).

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
