# RustyNES — Roadmap

This is the entry point for project planning. Each phase below links to its overview file. Each phase contains sprints; each sprint contains tickets.

The phase bodies preserve the **engine-lineage** development history — the
internal engine line (v0.9.x → v2.x markers) whose increments produced the
RustyNES v1.0.0 technology. Those version markers are historical anchors, not
RustyNES releases of their own; the RustyNES production core shipped at
**v1.0.0**, and the **v1.1.0 → v1.5.0** feature releases (with **v1.6.0
"Studio"** now in development) ship on top of it.

**RustyNES release line:** `v0.1.0…v0.8.6` (the parent emulator) →
`v0.9.0…v0.9.7` (engine-lineage integration stages — the inbound cycle-accurate
engine being folded in, stage by stage) → **`v1.0.0`** (this synthesis: the
engine + the ported desktop-UX shell + production polish) → **`v1.1.0`
"Scriptable" → `v1.2.0` "Curator" → `v1.3.0` "Bedrock" → `v1.4.0` "Fidelity"**
(+ the `v1.4.1` patch) **→ `v1.5.0` "Lens"** — the additive, off-by-default
feature releases on that core, of which **`v1.5.0` is the current shipped tag**
— → **`v1.6.0` "Studio"** (in development). The forward path then targets the
real **RustyNES `v2.0.0`** (the fractional master-clock timebase refactor, ADR
0002) and beyond. Where the detailed sections below carry the inbound engine's
own `v1.x`/`v2.x` tags, read them as upstream engine history (its v2.0–v2.8
line), which maps onto the integration stages roughly as: engine v1.0.0 →
RustyNES v0.9.0; v1.1.0–v1.4.0 → v0.9.1; v1.5.0–v1.7.0 → v0.9.2; v2.0.0–v2.0.1 →
v0.9.3; v2.1.0–v2.2.0 → v0.9.4; v2.3.0–v2.5.0 → v0.9.5; v2.6.0–v2.7.1 → v0.9.6;
v2.8.0 → v0.9.7; the synthesis itself = **v1.0.0**.

> **Two distinct "v2.0"s — do not conflate them.** The engine-lineage **v2.0**
> (the master-clock work that took AccuracyCoin to **100.00%**) is *upstream
> engine history* and **already shipped, as the v1.0.0 production core**. The
> forward **RustyNES v2.0.0** in "The path to v2.0.0 and beyond" below is a
> *different, still-future* milestone: the integer/fractional master-clock
> **timebase** refactor (ADR 0002) that closes the documented hard-tier accuracy
> residuals. The engine's own `v1.x`/`v2.x` markers in the bullets and "Phases"
> sections are historical anchors, **never** RustyNES release numbers.

## Status

- **Current release:** **RustyNES v1.5.0 "Lens" — the insight + scriptability + creator-tooling + polish release**, the fifth feature release on the v1.0.0 production core. A cycle-accurate NES/Famicom emulator at the Mesen2 / higan / ares accuracy bar, shipped as a polished desktop application (AccuracyCoin **100.00% (139/139)**, the 60-ROM `external_real_games` + 52-entry `external_extended` oracles byte-identical, nestest 0-diff). Every feature release since v1.0.0 has been **additive / off-by-default**, so the shipped / native / `no_std` / wasm builds stay byte-identical and AccuracyCoin has held **100% (139/139)** the whole way. v1.5.0 lands eight workstreams: **A** debugger visualization (Input Miniatures overlay, a PPU event-viewer heatmap, a per-scanline trace viewer, an HD-pack per-pixel inspector — behind `debug-hooks`/`hd-pack`); **B** Lua dev/TAS API depth (memory peek/poke/range, cart/system queries, in-memory save-state scripting, breakpoint/symbol hooks, bundled example scripts); **C** creator/TAS tooling (a TASVideos compatibility pass, replay/TAS-window polish, an NSF waveform scope); **H** frontend pacing & audio-sync perf (triple-buffer framebuffer handoff, a pacer stall phase-break, audio DRC/buffer tuning, GPU pass timing `gpu_ms`, perf-log↔panel parity + a perf-capture regression gate); **I** a native-UI overhaul + in-app Documentation pane; **D** UX polish (named-palette editor, per-side overscan WYSIWYG, an "Enhancements" group with sprite-limit-disable/overclock staged-but-inert pending v2.0 per ADR 0002); **E** accessibility (configurable UI scaling, high-contrast + Okabe-Ito colorblind themes, keyboard-only menu nav); **F** mapper breadth 113 → **123 families**; and **G** casual-mode browser RetroAchievements *scaffolding* (ADR 0015) behind an off-by-default `browser-cheevos` feature (live-browser verify, proxy deploy, and trampoline marshalling remain maintainer-manual carryovers; native RA unaffected). The underlying **v1.0.0 production cut** delivers: the master-clock cycle-accurate core; the original 51 mapper families (incl. VRC6/VRC7-OPLL/Sunsoft-5B/Namco-163/MMC5 expansion audio + Vs./PC10 RGB boards); real-BIOS FDS; 2-4-player rollback netplay (native UDP + browser WebRTC); RetroAchievements (opt-in/native); TAS movie record/replay + save states/rewind; the performance + desktop-UX shell (display-sync pacing matrix + late input latch, lock-free audio ring + dynamic rate control, run-ahead, dedicated emulation thread, plus an always-on egui shell — menu bar / status bar, tabbed Settings, light/dark/system themes, 8:7 pixel-aspect correction, fullscreen, save-state slots, recent-ROMs, and the surfaced Cheats/Movies/Netplay/RA/Performance tool panels); and a WebAssembly build with an AudioWorklet audio path + rAF display-sync. The determinism contract holds (DRC is a frontend resampler stage; run-ahead is frontend snapshot/restore orchestration; the core per-frame output is untouched). See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[1.5.0]`…`[1.0.0]`.
- **RustyNES feature-release history (on the v1.0.0 core; all additive / off-by-default; AccuracyCoin held 100% (139/139) throughout):**
  - **v1.1.0 "Scriptable"** (2026-06-15) — full NES_NTSC composite + CRT/scanline shaders + `.pal` palette filters; NES Power Pad + turbo/autofire + an input-display overlay + a per-game nametable-mirroring override DB; debugger breakpoints + a cycle trace logger + an event viewer (behind `debug-hooks`); an NSF/NSFe player + a 5-band graphic EQ; and the flagship **Lua scripting engine** (`rustynes-script`, ADR 0010). See `CHANGELOG.md` `[1.1.0]`.
  - **v1.2.0 "Curator"** (2026-06-15) — library / compatibility / reach: mapper tiering (Core / Curated / BestEffort, ADR 0011) **51 → 87 families** behind a CI honesty gate; `.zip` loading + `.ips`/`.ups`/`.bps` soft-patching; a per-game DB + in-app ROM-Database editor; live NTSC knobs + a composable ShaderStack + CRT preset bank (ADR 0013) + a default-off HD-pack loader; Family BASIC keyboard / SNES mouse / Arkanoid-both-ports / Game-Genie code DB; Lua `onNmi`/`onIrq`/`setInput`; menu-bar UX + FontAwesome icons; web touch controls + Power Pad + an experimental wasm Lua piccolo backend (ADR 0012); a turn-key netplay `deploy/` bundle; and a PGO CI gate. The SMB3 World 1-1 sprite-flicker (a PPU OAM-row-corruption bug) and the Mapper 89 bus conflict were fixed. See `CHANGELOG.md` `[1.2.0]`.
  - **v1.3.0 "Bedrock"** (2026-06-16) — toolchain modernization (edition 2024 / Rust 1.96 / egui 0.34.3 + wgpu 29.0.3 + rfd 0.17.2); a frame-pacing fix; a Memory Compare panel + a menu/Settings reorg + per-setting auto-save; mapper coverage **87 → 101 families** + Vs. DualSystem header detection (NES 2.0 byte-13); HD-pack `<condition>`/`<background>` rules (ADR 0014); netplay desync diagnostics + niche peripheral aliases; and a PGO/BOLT CI gate. See `CHANGELOG.md` `[1.3.0]`.
  - **v1.4.0 "Fidelity"** (2026-06-16) + the **v1.4.1** patch (2026-06-16) — accuracy polish (triangle ultrasonic silence; the DMC-DMA ↔ controller-read conflict verified + documented); per-channel audio mixing; devtools finish (symbol-file `.sym`/`.mlb`/`.nl` loading + event breakpoints); browser QoL (wasm `.rnm` movie I/O + IndexedDB save-states); a measure-first perf pass (−8% on the rendering-heavy bench); a clap-4 styled `--help` + a `rustynes help` ratatui TUI (native-only); and mapper coverage **101 → 113 families** (boot-smoke verified). v1.4.1 added four more BestEffort boot/decode fixes (m92 / m94 / m145 / m147) + a screenshot-corpus tier reorg. See `CHANGELOG.md` `[1.4.0]` + `[1.4.1]`.
  - **v1.5.0 "Lens"** (2026-06-17) — the current release; full workstream summary in the "Current release" bullet above. Mapper coverage **113 → 123 families**. See `CHANGELOG.md` `[1.5.0]`.
- **In development — v1.6.0 "Studio" (NOT yet tagged; the in-flight feature release, accumulating on `main`):** the studio / TAS-tooling / debugger-depth / accuracy-and-breadth release. Additive / off-by-default like its predecessors, so AccuracyCoin holds **100% (139/139)** and the shipped builds stay byte-identical. **Merged to `main` so far:** **A** the **TAStudio piano-roll TAS editor** (one row per frame × one column per controller button over the editor model — click-to-toggle + column drag-paint, cursor/lag/marker tinting, a forkable-branch list, seek-on-frame-click, insert/delete-frame, and `.rnmproj` project save/load; native I/O for now; the grid virtualizes its rows so long movies stay cheap); **E** the **J.Y. Company ASIC mappers** (iNES 90 / 209 / 211, BestEffort) and the **UNIF (`.unf`) cartridge loader** (board-name → iNES resolution, taking the count to roughly **126 families**); plus a large **per-mapper screenshot-coverage pass** + the data-driven `external_coverage` harness, several **blank-boot mapper fixes** (m159 + m30/m80/m185 and a cluster of BestEffort decode fixes), a **CI paths-filter gate** (doc-only PRs report `CI success` without the heavy jobs), and **Lua data-API breadth (B3)**. **Remaining / in flight for v1.6.0:** `.fm2`/`.bk2` movie interop + Lua `movie.*`/`frameadvance` (B); Mesen2-class conditional breakpoints / watchpoints / a hex editor (C); the off-axis accuracy cluster (D — DMC/OAM-DMA controller corruption, `$2007` blocking read, sprite-overflow — adoptable *without* the v2.0 refactor); mapper breadth → ~150 families (E); FDS-proper (F); A/V recording (G); HD audio (H); shaders (I); then the release finals (version bump, tag, `release.yml` binaries, Pages, release notes).
- **Engine-lineage — the "optimized performance" pass** (folded into v1.0.0): a frontend + build performance pass — a Performance panel + CSV "Logging" checkbox; a lock-free SPSC audio ring + **dynamic rate control**; a **display-sync pacing matrix** (`auto|display|vrr|wallclock`) + **late input latch**; a **snapshot fast path** (36→14.6 µs) + **run-ahead** (default 1, persistent timeline byte-identical); **mapper-caps + pixel-LUT + fat-LTO + SIMD** (**−26%** rendering-heavy bench, −16% nestest); a **dedicated emulation thread** (default-ON `emu-thread`, lock-free `SharedInput`, netplay-pause TOCTOU-closed) + best-effort Linux priority elevation; and a browser **AudioWorklet** + **rAF display-sync**. See `docs/release-notes/v2.8.0.md` (engine-line detail).
- **Engine-lineage — the master-clock milestone:** the engine's v2.0 line made the R1 `u64` master clock the default (AccuracyCoin 90.65%→**100.00%**, region-exact 3.2:1 PAL via the unified DMA engine) and then removed the legacy integer-lockstep scheduler (R1 is the only path; the `mc-r1-*` flags no longer exist). See `docs/audit/v2.0-phase7f-r1-default-promotion-2026-06-10.md`.

> **The bullets that follow (down to the engine-lineage Phase 6 entry) are the
> inbound engine's own release line — its `v1.x`/`v2.x` tags + 2026-05-2x dates.
> They are *engine history*, folded into the RustyNES v1.0.0 core; they are NOT
> the RustyNES v1.x feature releases listed at the top of this Status block.**

- **Engine-lineage phase:** **engine v1.7.0 (2026-05-25)** — **niceties milestone**: Four Score 4-player support (bus `$4016`/`$4017` 24-read multiplex of 4 controllers + adapter signature; opt-in, OFF by default = byte-identical two-controller reads; a P3/P4 keyboard + gamepad rebind UI + a "Four Score" toggle), GameShark-style raw RAM cheats (`Nes::poke_ram` applied caller-side after `run_frame`, alongside the v1.6.0 Game Genie support; a `RawCheat` `$addr=$value [if $compare]` section in the cheat panel persisted per-ROM), and an in-app graphics/audio/rewind settings panel. **Additive, independent of the deferred v2.0 master-clock axis**; AccuracyCoin held **90.65%**, oracle 60/60, sacred trio + B4 byte-identical, determinism preserved. Workspace `--features test-roms`: **702 strict + 10 ignored**. See `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` §2 + `CHANGELOG.md` `[1.7.0]`.
- **Engine-lineage phase:** **engine v1.6.0 (2026-05-25)** — **frontend-polish milestone** (the engine's v2.0.0 plan's original v1.5.0 content, deferred when Phase 7 took that slot). **Additive, independent of the deferred v2.0 master-clock axis**; AccuracyCoin held **90.65%**, oracle 60/60, sacred trio + B4 byte-identical, determinism preserved. Landed across 6 sprints: (0) `x86_64-apple-darwin` release target dropped (ADR 0009, Aug-2027 runner sunset); (1) Game Genie cheats (core `rustynes-core/src/genie.rs` runtime overlay — off by default, not in the save-state — + a debugger cheat panel with per-ROM persistence); (2) in-app gamepad rebinding UI (config-driven `[input.gamepad1/2]` + P2 keyboard rows + axis-as-dpad; serde default = the legacy Xbox layout); (3) controls/configuration doc-sync; (4) browser (wasm) `.rnm` movie download/upload + localStorage save-states; (5) a non-flaky frame-time regression CI gate + a rendering-heavy `flowing_palette` bench. Workspace `--features test-roms`: **688 strict + 10 ignored**. See `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` + `CHANGELOG.md` `[1.6.0]`.
- **Engine-lineage phase:** Phase 7 — **engine v1.5.0 (2026-05-24)**: **Nesdev Accuracy Hardening** (the genuinely-skipped phase; see numbering note below). Coverage + region validation + developer ergonomics + documented scope closure — **additive only**, AccuracyCoin held at **90.65%**, oracle 60/60, sacred trio + B4 byte-identical. Landed across 4 sprints: (1) blargg `instr_misc`/`instr_timing`/`cpu_reset` corpus wired (+8 strict); (2) seeded power-on RAM randomization developer mode (`Nes::from_rom_with_power_on_seed`; default path unchanged) + NMI/IRQ B-flag + `$4015` open-bus guards; (3) automated PAL/Dendy timing gates (per-region constant table + frame-structure integration test); (4) VRC2/4 + M34 NINA-001 submapper fixtures (replacing the rotted `vrc24test`) + `compatibility.md` platform-scope closure (FDS plan, Vs/PC10, PPU variants, input devices, long-tail policy). Workspace `--features test-roms`: **661 strict + 10 ignored**. Deferred to v2.0 (master-clock axis): C1 IRQ-sample, `$2002` sub-cycle, SH\* internal-bus, stale-shifter, `$2007` rendering, FDS code, PAL 3.2:1 CPU:PPU ratio. See `docs/audit/phase-7-*` + `CHANGELOG.md` `[1.5.0]`.
- **Engine-lineage phase:** Phase 10 — **engine v1.4.0 (2026-05-24)**: **TAS movie recording/playback**. Deterministic `.rnm` record/replay + save-state branching (ADR 0008: `RNESMOV1` header + ROM SHA-256 + optional `.rns` start point + per-frame input stream); `MovieRecorder`/`MoviePlayer` in `rustynes-core` (no_std) + record/play/branch hotkeys (`F6`/`F7`/`F8`) + a read-only REC/PLAY egui overlay; native `.rnm` save/load (wasm I/O is a follow-up). No API break (additive `Nes::buttons` getter; `run_frame` byte-for-byte unchanged) → oracle 60/60, AccuracyCoin 90.65%, B4 + sacred trio preserved. Determinism proven by byte-identical round-trip tests; **636 strict + 8 ignored**. Clean-room from Mesen2 `Core/Shared/Movies/` + FCEUX `.fm2` + TetaNES `.replay`. Delivered across Sprints 4.1 (core) + 4.2 (frontend UI). The prior **Phase 9 — v1.3.3 RELEASED (2026-05-24)**: bug-fix patch (frontend-only; native unchanged, pixel-identical) closing two wasm/GitHub-Pages issues + a native pacing refinement — (1) wasm/Pages severe stutter + freezes (v1.3.2 regression): the wasm idle path busy-looped on `ControlFlow::Poll` alongside the rAF loop + a missing `request_redraw()` re-arm could stall it; fixed to `ControlFlow::Wait` + an unconditional rAF re-arm; (2) wasm/WebGL2 palette wrong: wgpu-hal double-encodes sRGB on the GL surface, so the GL pipeline now stays UNORM (zero conversion, matches the correct canvas-2D path); native keeps sRGB → pixel-identical; (3) native residual stutter: chunked pacer sleep + 2 ms spin margin. Both wasm fixes need browser confirmation. Workspace **616 strict + 6 ignored** (unchanged). The prior **v1.3.2 RELEASED (2026-05-24)** closed two v1.3.1 follow-ups: dead keyboard input after the config migration (`parse_keycode` legacy keycode aliases) + a first wasm rAF-pacing attempt. **v1.3.1 RELEASED (2026-05-24)** was a bug-fix patch on the v1.3.0 WebAssembly milestone with three fixes (no API break, no accuracy change): (1) green/garbage left-edge column while scrolling — BG attribute (palette) shifters were one tile out of phase with the pattern shifters (`086ce4d` regression), now 16-bit + lockstep (AccuracyCoin-neutral; PPU save-state v1→v2); (2) stutter / non-smooth framerate — configurable present mode (default `Mailbox`) + a native sleep-then-spin frame pacer replacing the jittery `ControlFlow::WaitUntil` cadence (user-confirmed smooth); (3) legacy `config.toml` now migrated in place (backup + loud summary) instead of silently dropped. MM3 MMC3 stage-select shear investigated, confirmed not-a-regression, deferred to v2.0 (C1 axis). Oracle 60/60; AccuracyCoin 90.65%; B4 + sacred trio preserved. See `CHANGELOG.md` `[1.3.1]`. **v1.3.0 (2026-05-24)** landed the WebAssembly target: `wasm32-unknown-unknown` frontend in two flavours (`wasm-winit` default = full winit+wgpu+egui, 2.12 MiB gzip; `wasm-canvas` ~316 KB embed), GitHub Pages deploy (`https://doublegate.github.io/RustyNES/`), CI `wasm` clippy job + 5 MiB size-budget gate, all Pages actions on Node 24 — delivered across Sprints 1.1 → 1.2 → 1.3 → 1.4a → 1.4b → 1.4c → 2.
- **Engine-lineage phase:** Phase 8 — **engine v1.2.0 (2026-05-24).** DMC DMA scheduler refactor landed under default-off cargo feature `dmc-get-put-scheduler` introducing Mesen2's canonical get/put cycle alternation model alongside the v1.1.0 phase-agnostic scheduler via the parallel-implementation pattern (ADR 0007). AccuracyCoin DMA cluster under flag-on: **6/10 match baseline** (closing 4 → 0 deferred to v1.2.x patches or v2.0 master-clock absorption). Default build bit-identical to v1.1.0.
- **Engine-lineage — earlier work:** **engine v1.1.0 (2026-05-25)** — VRC7 OPLL FM audio via clean-room pure-Rust port of `emu2413 v1.5.9` (MIT); ADR 0006 supersedes ADR 0004; *Lagrange Point* plays with audio. (engine v1.1.0 was an engine v2.0.0-release-plan milestone slotted between Phase 6 and Phase 8, **not** the ROADMAP's Phase 7 — see the numbering note below.) Phase 6 — **engine v1.0.0 (2026-05-23)**: AccuracyCoin gate CLEARED at 90.65% (126/139); T-60-001 C1 IRQ-timing residuals (3 `cpu_interrupts_v2` sub-ROMs + `mmc3_test_2/4` #3) deferred to the master-clock-precise scheduling refactor (Session-29 empirically falsified Option A global PPU-position shift; 17 documented rollbacks). [That engine-lineage master-clock work subsequently landed in the RustyNES v1.0.0 core, taking AccuracyCoin to 100%.]
- **Phase-numbering note:** the shipped releases v1.1.0 → v1.4.0 were sequenced from the v2.0.0 release plan and back-labelled in the detailed sections as v1.1.0 (VRC7) → Phase 8 (v1.2.0 DMC) → Phase 9 (v1.3.0 wasm) → Phase 10 (v1.4.0 TAS). **Phase 7 — Nesdev Accuracy Hardening (below) was authored but never executed**; it is now being executed as **v1.5.0**. See `docs/audit/phase-7-assessment-2026-05-24.md` for the full intent-vs-accomplished-vs-completable disposition.
- **Current state:** **RustyNES v1.5.0 "Lens" — fifth feature release on the v1.0.0 production core, shipped; v1.6.0 "Studio" in development.** Every accuracy, compatibility, platform, netplay, RetroAchievements, FDS, Vs/PC10, and performance milestone in the engine-lineage history above is folded into the v1.0.0 core; the v1.1.0 → v1.5.0 feature releases then layered (in order) the Lua scripting engine + visual filters/peripherals/devtools/NSF, the library/compatibility/reach pass (mapper tiering, soft-patching, per-game DB, shaders, HD-pack), the toolchain modernization + Memory-Compare + Vs.-DualSystem detection, the accuracy-and-finish pass (per-channel audio, symbol loading, browser QoL, `rustynes help`), and the insight/scriptability/creator-tooling/polish pass (debugger viz, Lua dev/TAS API, TASVideos, accessibility, native-UI overhaul). Mapper coverage rose **51 → 123 families** across these releases, all additive / off-by-default, with AccuracyCoin holding **100% (139/139)** the entire time. The engine-lineage version markers (v0.9.x → v2.x) in the bullets above and the phase bodies are upstream history, not RustyNES releases.

**The path to v2.0.0 and beyond:**

- **v1.6.0 "Studio" (in development — the next release).** Finish the remaining A–I workstreams (see the "In development" bullet above): `.fm2`/`.bk2` movie interop + Lua `movie.*`/`frameadvance`, Mesen2-class conditional breakpoints / watchpoints / a hex editor, the off-axis accuracy cluster (DMC/OAM-DMA controller corruption, `$2007` blocking read, sprite-overflow — adoptable *without* the v2.0 timebase refactor), mapper breadth → ~150 families, FDS-proper, A/V recording, HD audio, and shaders; then the release finals (version bump, tag, `release.yml` binaries, Pages redeploy, release notes). Additive / off-by-default, so AccuracyCoin stays **100% (139/139)** and the shipped builds stay byte-identical.
- **v2.0.0 — the integer/fractional master-clock timebase refactor (ADR 0002). The real next architectural milestone.** Switch the core timebase from the current "3 PPU dots per CPU cycle" integer-lockstep model to an exact integer **master clock** (ares' NTSC 12:4 and PAL 16:5 ratios; Mesen2's fractional-12-master-clocks-per-CPU-cycle model) so that the per-cycle *phase relationship* between CPU and PPU becomes adjustable. That is the documented prerequisite to close the hard-tier accuracy residuals that 17 prior in-place attempts could not: the C1 IRQ-sample cases (`cpu_interrupts_v2/{2,3,5}` were closed in v1.3.0's re-baseline, but `mmc3_test_2/4` sub-test #3 + the 2 `apu_reset` cases remain), the `$2002` NMI-suppression sub-cycle race, the SH\* internal-bus stores, and the exact PAL 3.2:1 CPU:PPU ratio. This is an **XL / HIGH-risk** structural change (the ADR 0002 stop-condition discipline applies); it is deliberately **deferred from v1.6.0** so the additive feature releases can ship first. Do not conflate it with the *engine-lineage* "v2.0" master-clock work, which is different and already shipped inside the v1.0.0 core. Permanent v2.0 scaffolding already lives in the tree (the `cpu-c1-attempt-17-access-reorder` feature, the `irq_trace` + golden traces, the `M2Phase` per-phase snapshots, the `vbl_race_window_2002_read_sweep` oracle).
- **Beyond v2.0.0 (separate initiatives, no fixed version yet).**
  - **Mobile (iOS / Android)** frontends.
  - **Browser / wasm Lua** maturity (the native Lua engine is feature-complete; the wasm piccolo backend, ADR 0012, is explicitly not byte-parity with native mlua).
  - **Finishing browser RetroAchievements** — the v1.5.0 scaffolding (ADR 0015, off-by-default `browser-cheevos`) needs the auth-proxy deploy, the wasm trampoline marshalling, and a live-browser verify; native RA is unaffected. Plus the live RA-account allowlisting pass with the RA team (the `RustyNES/<ver>` User-Agent is already sent; the allowlisting itself is a request, not a code change).
  - **Long-tail mapper coverage** toward the full ~300-mapper set + **100% TASVideos** compatibility.
  - **Full Vs. DualSystem** dual-console *emulation* (two-CPU / two-PPU; currently *detected* and surfaced with a "not yet emulated" note — there is no committable DualSystem test-ROM oracle, so it is a documented v2.0-era deferral; design `docs/audit/vs-dualsystem-design-2026-06-11.md`).
- **Engine-lineage forward-roadmap history (folded into the v1.0.0 core; retained for context — NOT a RustyNES release plan):** the inbound engine's own roadmap completed engine v2.6.0 (Vs/PC10 RGB game-verified, +11 mappers→51, N-peer netplay, real-BIOS FDS), engine v2.7.0 (RetroAchievements via the vendored rcheevos FFI; the Vs.-System per-game DIP/2C04-palette DB; deployable browser WebRTC netplay), and engine v2.7.1 (netplay-hardening + live verification, the `power_cycle` cold-boot desync fix, the >2-player browser WebRTC mesh, RA fixes, the MMC6 PRG-RAM fix, the NTSC-filter WGSL crash fix, Vs. DualSystem detection groundwork). All of this is present in the RustyNES v1.0.0 core; stock NES is byte-identical and AccuracyCoin is 100%.
- **Done:** Phases 1-4 complete; Phase 5 Sprints 1-3 shipped — Frontend MVP, save state + rewind + TOML rebinding, egui debugger overlay (CPU/PPU/OAM/APU/memory/mapper panels + in-app rebind modal closing T-52-007), simplified Blargg-style NTSC wgsl post-pass, release workflow + README badges. **Regression-prevention buildout closed (2026-05-17):** 21-ROM permissive baselines + 60-ROM commercial-ROM oracle (54 strict + 6 ignored across 15 mappers) + 81-PNG visual corpus + permanent `scripts/regression-bisect/` tooling + `docs/audit/` decision-rationale tier. Real-game regression on SMB / Excitebike / Kid Icarus closed by the FSM dot-64 reset fix on `accuracy-stabilization` (`834be9e`). Residual accuracy gaps tracked in `CHANGELOG.md` `[Unreleased]` → "Investigated and rolled back". (Historical note: when this bullet was written, v1.0.0 was still gated on the C1 IRQ-timing rework + AccuracyCoin ≥ 90% (then 69.78%) + multi-OS smoke + the 6 ignored commercial ROMs. All of those resolved: **v1.0.0 released** (the 90.65% gate was an interim engine-lineage milestone), the 6 ROMs are strict-passing, and the master-clock refactor (the engine-lineage "v2.0" axis) **shipped as the v1.0.0 default core**, closing the C1 + sub-cycle residuals — the default build measures **AccuracyCoin 100%**. See `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` for the historical trajectory.)
- **Status matrix (single source of truth):** see [`docs/STATUS.md`](../docs/STATUS.md) for the per-test-ROM-suite pass count, mapper coverage matrix, feature flag state, and version policy. This roadmap intentionally keeps a short summary only.

## Phases

> **Reminder:** the `v1.x`/`v2.x` version tags inside the Phase bodies below are
> **engine-lineage** markers (the inbound engine's own line, dated 2026-05-2x),
> retained as historical anchors. They are **not** the RustyNES v1.1.0 → v1.5.0
> feature releases (dated 2026-06-1x) tracked in the Status block above, and the
> Phase-body "v2.0" deferrals refer to the *engine's* master-clock work that
> already shipped in the RustyNES v1.0.0 core — distinct from the forward
> **RustyNES v2.0.0** timebase refactor (ADR 0002) in "The path to v2.0.0".

### Phase 1 — Foundation

**Goal:** Empty Cargo workspace builds cleanly with CI green; cartridge parser passes round-trip tests; CPU executes the nestest golden log without diverging.

**Exit criterion:** `cargo test --workspace` green; `nestest.nes` golden-log compare passes; iNES + NES 2.0 parser handles the test ROM corpus without errors.

**Estimated duration:** 4-6 weeks

[Phase 1 overview](archive/phase-1-foundation/overview.md)

Sprints:

- [Sprint 1 — Workspace + CI + lints](archive/phase-1-foundation/sprint-1-workspace.md)
- [Sprint 2 — Cartridge parser (iNES + NES 2.0)](archive/phase-1-foundation/sprint-2-cartridge.md)
- [Sprint 3 — CPU core: official opcodes](archive/phase-1-foundation/sprint-3-cpu-official.md)
- [Sprint 4 — CPU core: unofficial opcodes + nestest](archive/phase-1-foundation/sprint-4-cpu-unofficial.md)

---

### Phase 2 — Graphics + Timing

**Goal:** PPU renders correct pictures for NROM, MMC1, UxROM, AxROM, CNROM, GxROM titles; lockstep scheduler operational; blargg PPU test ROMs pass.

**Exit criterion:** `ppu_vbl_nmi/*`, `ppu_open_bus`, `sprite_overflow_tests/*`, `oam_read`, `oam_stress` all pass; visual diff against Mesen2 reference for a curated demo set.

**Estimated duration:** 6-8 weeks

[Phase 2 overview](archive/phase-2-graphics-timing/overview.md)

Sprints:

- [Sprint 1 — PPU bus, registers, memory map](archive/phase-2-graphics-timing/sprint-1-ppu-bus.md)
- [Sprint 2 — Background rendering + scrolling](archive/phase-2-graphics-timing/sprint-2-background.md)
- [Sprint 3 — Sprite evaluation + rendering + sprite-zero hit](archive/phase-2-graphics-timing/sprint-3-sprites.md)
- [Sprint 4 — Lockstep scheduler + DMA + simple mappers (NROM, UxROM, CNROM, AxROM, GxROM, MMC1)](archive/phase-2-graphics-timing/sprint-4-scheduler-mappers.md)

---

### Phase 3 — Audio + Polish

**Goal:** APU produces correct audio; lookup-table mixer and analog filter chain in place; band-limited synthesis emits at host sample rate; CPU illegal opcodes complete.

**Exit criterion:** `apu_test/*`, `apu_mixer/*`, `dmc_dma_during_read4/*`, `cpu_interrupts_v2/*` all pass.

**Estimated duration:** 4-6 weeks

[Phase 3 overview](archive/phase-3-audio-polish/overview.md)

Sprints:

- [Sprint 1 — APU channels (pulse 1, pulse 2, triangle, noise)](archive/phase-3-audio-polish/sprint-1-apu-channels.md)
- [Sprint 2 — DMC channel + DMC DMA + frame counter](archive/phase-3-audio-polish/sprint-2-dmc-frame.md)
- [Sprint 3 — Mixer + filters + band-limited synthesis](archive/phase-3-audio-polish/sprint-3-mixer.md)

---

### Phase 4 — Mapper Coverage

**Goal:** Top-25 mappers implemented; MMC3 IRQ accuracy validated; MMC5 (no audio); audio extension mappers (VRC6, Sunsoft 5B, Namco 163) functional.

**Exit criterion:** Per-mapper boot test passes for one ROM per supported mapper; `mmc3_test_2/*`, `mmc3_irq_tests/*`, `vrc24test`, holy_mapperel pass; AccuracyCoin pass rate ≥ 80%.

**Estimated duration:** 6-8 weeks

[Phase 4 overview](archive/phase-4-mapper-coverage/overview.md)

Sprints:

- [Sprint 1 — MMC3 (the defining mid-life mapper)](archive/phase-4-mapper-coverage/sprint-1-mmc3.md)
- [Sprint 2 — MMC2/MMC4 + Color Dreams + CPROM + BNROM/NINA + Camerica + VRC1](archive/phase-4-mapper-coverage/sprint-2-misc-mappers.md)
- [Sprint 3 — VRC2/4/6 + Sunsoft FME-7 + Namco 163](archive/phase-4-mapper-coverage/sprint-3-vrc-extended.md)
- [Sprint 4 — MMC5 (without audio extension)](archive/phase-4-mapper-coverage/sprint-4-mmc5.md)

---

### Phase 5 — Frontend + Tooling

**Goal:** `rustynes` binary playable end-to-end with save state + rewind + debugger overlays + NTSC filter; CI publishes signed binaries on tag.

**Exit criterion:** Binary builds and runs on Linux/macOS/Windows; passes manual smoke test of compatibility-difficulty corpus; release pipeline green.

**Estimated duration:** 4-6 weeks

[Phase 5 overview](archive/phase-5-frontend-tooling/overview.md)

Sprints:

- [Sprint 1 — winit + wgpu + cpal frontend (minimum viable player)](archive/phase-5-frontend-tooling/sprint-1-frontend-mvp.md)
- [Sprint 2 — Save state + rewind + input bindings](archive/phase-5-frontend-tooling/sprint-2-save-rewind.md)
- [Sprint 3 — Debugger overlays (egui) + NTSC filter + release pipeline](archive/phase-5-frontend-tooling/sprint-3-debugger-release.md)

---

---

### Phase 6 — v1.0.0 Closeout (SUPERSEDED — accuracy closed by the engine-lineage master-clock work)

> **Superseded.** The engine-lineage continued past this closeout plan: the
> master-clock refactor took AccuracyCoin to **100.00% (139/139)** and the C1
> IRQ-timing + sub-cycle residuals these sprints chased were closed (or
> documented-deferred) along the way. The sprint backlog below was **not**
> executed as written; it is retained as the historical gate plan. RustyNES
> ships at **v1.0.0** with the accuracy bar fully cleared.

**Original goal (historical):** close all open v1.0.0 gates and ship the v1.0.0
tag.

**Original exit criterion (historical):** `cargo test --features test-roms`
shows the C1 `cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4-scanline_timing`
sub-test #3 flipped + AccuracyCoin ≥ 90% + multi-OS release-artifact smoke test
green + the 6 `#[ignore]`'d commercial ROMs investigated. (All resolved by the
engine-lineage work; AccuracyCoin is now 100%.)

[Phase 6 overview](archive/phase-6-v1-closeout/overview.md)
[Phase 6 v1.0.0-final sprint backlog](archive/phase-6-v1.0.0-final/overview.md)
— ordered six-sprint plan to close the AccuracyCoin 90% gate + the 4
C1 IRQ-timing residuals (Sprint 1: Implied-Dummy + DMC coordinated;
Sprint 2: APU put/get phase; Sprint 3: sprite-eval residuals;
Sprint 4: PPU misc residuals; Sprint 5: C1 axis attempt 17;
Sprint 6: SH* unstable stores).

Tickets (informal — formal sprint files when work begins). The `[~]` markers
below are **historical**: they record each ticket's state *at this superseded
phase*, not now — all were closed or documented-deferred by the engine-lineage
master-clock work (current AccuracyCoin **100.00%**). They are not live TODOs.

- [~] **T-60-001 — Coordinated CPU/Bus/PPU IRQ-sample-timing rework
  (Track C1). DEFERRED to v1.x.** 11 independent fix attempts rolled
  back across multiple sessions; no empirical breakthrough on the
  canonical CPU `T_last - 1` IRQ-sample-point axis. Residuals:
  `cpu_interrupts_v2/{2-nmi_and_brk, 3-nmi_and_irq, 5-branch_delays_irq}`
  - `mmc3_test_2/4-scanline_timing` sub-test #3. Infrastructure landed
  (ADR-0002 Decision section + per-CPU-cycle IRQ tracing fixture + 6
  golden baseline traces + M2-phase plumbing + Phase B4 reload-pending
  discriminator). Does not affect any real game; commercial game
  compatibility intact. Carries forward to v1.x roadmap.
- [~] **T-60-002 — Push AccuracyCoin pass rate from 69.78% to ≥ 90%.
  IN PROGRESS at 82.73%** (Cascade B closed 2026-05-19 in commit
  `9b0c81c` + Cascade A partial closure 2026-05-19 via OAMADDR reset
  during dots 257-320 in `f29f7ca` + session-6 `$2004` dots 1-64 `$FF`
  in `6c2664e` + session-7 OAMADDR-walks-during-eval + $4-aligned
  `$2004` write in `c230489` + session-7 RMW ABS,X/Y unfixed-address
  dummy read in `32d5b18` + **session-8 BG-pipeline cycle-9 reload +
  post-emit shift in `086ce4d` (architectural closure of Cascade A's
  `VerifySpriteZeroHits` step-2 geometric puzzle per
  `docs/audit/cascade-a-investigation-2026-05-19.md`)**; trajectory
  `64.03% → 67.63% → 69.06% → 69.78% → 76.98% → 78.42% → 79.14% →
  79.86% → 82.73%`, exceeds CI floor of 0.60 by 22.7pp and
  **CLEARED the v0.9.x 80% target by 2.7pp**). **Cascade B
  (DMC DMA halt-cycle precision) CLOSED** — all 8 tests in "APU
  Registers and DMA tests" flipped + 3 net side-benefit flips
  elsewhere; +11 tests. **Cascade A (Sprite Zero Hit BG-pipeline
  geometry) PARTIALLY CLOSED** — the load-bearing architectural
  axis (BG shift-register cycle-9 reload + post-emit shift per
  Mesen2 + nesdev wiki) landed in session 8, flipping 4 tests
  (Sprite 0 Hit behavior, Sprite overflow behavior, Suddenly
  Resize Sprite, $2007 read w/ rendering). The remaining 24
  failing tests cluster as documented in
  `docs/audit/accuracycoin-readme-analysis-2026-05-17.md`'s
  2026-05-19 addendum +`docs/audit/cascade-a-investigation-2026-05-19.md`'s
  RESOLUTION section:
  - **Cascade A residuals — 10 tests (post-BG-pipeline-fix):** 4
    sprite-eval ($2002 flag timing, Arbitrary Sprite zero, Misaligned
    OAM behavior, OAM Corruption) + 6 PPU misc (Stale BG/Sprite
    Shift Regs, BG Serial In, Sprites On Scanline 0, $2004/$2007
    Stress Tests). Cluster gated on stale-shift-register modeling +
    post-B8 sprite-FSM interactions + $2002 sub-cycle flag timing.
    The session-8 BG-pipeline fix closed the geometric root cause
    (`VerifySpriteZeroHits` step-2) but left these subtler
    cycle-precision residuals for future sessions.
  - **C1 IRQ-timing axis — 5 tests (4 × `cpu_interrupts_v2/{2..5}` +
    `mmc3_test_2/4` sub-test #3) — DEFERRED, see T-60-001.**
  - **Internal-bus model — ~5 tests** (`CPU Behavior :: Open Bus
    [error 9]`, 5 × SH*opcodes `[error 7]`, `CPU Behavior 2 ::
    Implied Dummy Reads [error 2]`). Requires internal-vs-external
    bus model rework that previously regressed Internal Data Bus
    Test 2. The SH* tests are "Coupled to Cascade B" per audit but
    they did NOT flip when Cascade B landed — confirming SH*
    address corruption needs an explicit RDY-low-2-cycles rule
    rather than just DMC DMA halt modeling.
  - **APU residuals — 5 tests** (Frame Counter IRQ, DMC Channel,
    APU Register Activation, Controller Strobing/Clocking). Each
    is a distinct $4015 RMW / put-vs-get-cycle bracket; bundled
    with the internal-bus-model rework above.
  - **PPU residuals — 2 tests** (Rendering Flag Behavior,
    `$2007` read w/ rendering). Distinct from Cascade A.

  **Realistic v1.0.0 trajectory**: if the remaining Cascade A
  geometric residual (VerifySpriteZeroHits step-2; characterisation
  reproducer at `crates/rustynes-ppu/src/ppu.rs` landed in `b629ace`) closes
  without regressing baselines, pass rate would advance
  `79.86% → ~88%`. The v1.0.0 90% gate remains contingent on Cascade A
  full closure + C1 IRQ-timing axis. T-60-002 carries forward to v1.x
  roadmap with the 79.86% baseline.
- [x] **T-60-003a — long-intro budget extensions (CLOSED, 2026-05-17)**:
  Mr. Gimmick + Tiny Toon Adventures 2 flipped from `#[ignore]`'d to
  passing via the `LONG_INTRO_START_3600` input script (idle 3600 →
  START tap → free-run 240, captures at f3661 / f3901). Commit `7fa2c90`.
  Ignored count: `6 → 4`.
- [x] **T-60-003b/c — CLOSED (2026-05-17)**: all 4 remaining stuck
  ROMs flipped via 2 architectural mapper fixes. Root cause: VRC2 /
  VRC4 / VRC6 / MMC4 mapper impls were missing the `$6000-$7FFF`
  WRAM read/write paths. Reads returned 0; writes silently dropped.
  Konami's save-bearing titles stalled in save-validation. Fixes:
  - commit `895e426`: VRC2/VRC4/VRC6 8 KiB `prg_ram` field added +
    read/write paths in `crates/rustynes-mappers/src/sprint3.rs`. Flipped
    Esper Dream 2, Mouryou Senki Madara, Ganbare Goemon 2.
  - commit `42f31ff`: MMC4 same pattern in
    `crates/rustynes-mappers/src/sprint2.rs`. Flipped Fire Emblem Gaiden.

  **T-60-003 is now FULLY CLOSED — all 6 originally-stuck commercial
  ROMs strict-passing. Commercial-roms count: 60 strict + 0 ignored.**
- [ ] T-60-004 — Multi-OS release-artifact smoke test (T-51-009 carried
  forward from Phase 5 Sprint 1). The `v1.0.0-rc1` tag triggers the
  GitHub Actions release workflow which produces Linux/macOS/Windows
  artifacts. User to smoke-test each on a representative ROM (e.g.,
  nestest.nes) before promoting to `v1.0.0`. PENDING USER VERIFICATION.
- [~] **T-60-005 — `v1.0.0` tag + release notes. SUPERSEDED by
  `v1.0.0-rc2`** (2026-05-22). The rc2 tag captures the
  post-Mesen2-alignment release-candidate state with the four C1
  IRQ-timing residuals + the ~20 non-C1 AccuracyCoin residuals
  explicitly carried forward into the
  `to-dos/phase-6-v1.0.0-final/` sprint backlog. The final `v1.0.0`
  tag is gated on AccuracyCoin ≥ 90% + T-60-001 closure (4 C1
  residuals flipped). Sprint 1 of the v1.0.0-final backlog targets
  the Implied-Dummy + DMC DMA coordinated fix that Session-19 surfaced
  as the highest-leverage entry point. Prior rc1 tag remains as the
  pre-Mesen2-alignment baseline.

---

### Phase 7 — Nesdev Accuracy Hardening (COMPLETE — v1.5.0, 2026-05-24)

**Outcome:** all 4 sprints landed; +25 strict tests, AccuracyCoin held at
90.65% (additive only; the master-clock-axis residuals are explicitly deferred
to v2.0). See `docs/audit/phase-7-assessment-2026-05-24.md` + the per-sprint
audit docs (`docs/audit/phase-7-sprint-{2,3,4}-*.md`).

**Goal:** close the hardware-accuracy and documentation gaps identified by
`ref-docs/nesdev-wiki-technical-report.md` and
`docs/nesdev-hardware-emulation-checklist.md`.

**Exit criterion:** all stock NES/Famicom behaviors in the Nesdev-derived
checklist are implemented, explicitly out of scope, or guarded by tests; missing
Nesdev-indexed test categories are vendored or replaced with licensed fixtures;
PAL/Dendy and remaining AccuracyCoin residuals have automated coverage; platform
expansion scope is documented.

[Phase 7 overview](archive/phase-7-nesdev-accuracy-hardening/overview.md)

Sprints:

- [Sprint 1 — Source and test corpus closure](archive/phase-7-nesdev-accuracy-hardening/sprint-1-source-test-corpus.md)
- [Sprint 2 — CPU, DMA, and internal bus closure](archive/phase-7-nesdev-accuracy-hardening/sprint-2-cpu-dma-internal-bus.md)
- [Sprint 3 — PPU residuals and region variants](archive/phase-7-nesdev-accuracy-hardening/sprint-3-ppu-region-variants.md)
- [Sprint 4 — Mapper, expansion audio, and platform variants](archive/phase-7-nesdev-accuracy-hardening/sprint-4-mappers-expansion-platforms.md)

### Phase 8 — v1.2.0 DMC DMA Scheduler (COMPLETE; broader accuracy residuals deferred)

**Scope reconciliation:** the original v2.0.0 plan framed v1.2.0 as a broad
"accuracy residuals" milestone (sprite-eval + PPU-misc + APU edge cases +
6 ignored commercial ROMs → AccuracyCoin ~97%). What **actually shipped** as
v1.2.0 was a narrower, focused slice: the **DMC DMA get/put scheduler**
landed behind a default-off cargo feature via the parallel-implementation
pattern (ADR 0007). The broader accuracy residuals were **not** done and are
**deferred to v1.6 / v2.0** (several fall out of the v2.0 master-clock
refactor for free); AccuracyCoin remains **90.65%**, not the 97% the original
plan targeted for v1.2.0.

**Exit criterion (MET, as shipped):** v1.2.0 tag landed with
`dmc-get-put-scheduler` parallel-implementation in place (default-off),
equivalence harness shipped, AccuracyCoin DMA cluster matching v1.1.0
baseline at 6/10 under the flag (the remaining 4 — `DMA + $4015 Read`,
`DMC DMA + OAM DMA`, `Explicit/Implicit DMA Abort` — deferred to v2.0
absorption; ADR 0007 option c). Default build bit-identical to v1.1.0; no
regression to the 60-ROM oracle, sacred trio, or B4 invariant.

[Phase 8 overview](archive/phase-8-v1.2.0-accuracy-residuals/overview.md)

Sprints:

- [Sprint 3 — DMC get/put scheduler parallel implementation](archive/phase-8-v1.2.0-accuracy-residuals/sprint-3-dmc-get-put-scheduler.md)
  — Sprint 3.1-3.5 + iter 3 (DMC abort path port) all LANDED. ADR 0007 written.
  v1.2.0 tag landed 2026-05-24.

> **Deferred to v1.6 / v2.0** (tracked here so it isn't lost): (a) DMC get/put
> completion 6/10 → 10/10 + default-on promotion (ADR 0007); (b) the broader
> AccuracyCoin residuals — sprite-eval ($2002 flag timing, Arbitrary Sprite
> zero, Misaligned OAM, OAM Corruption), PPU-misc (Stale BG/Sprite shift regs,
> BG Serial In, Sprites On Scanline 0, $2004/$2007 Stress), APU edge cases
> (Frame Counter IRQ #7, DMC, Reg Activation, Controller Strobing), and the
> 6 ignored commercial ROMs (mapper-026 VRC6b pair shares one bug). Many are
> on the C1 IRQ-sample-point axis and close with the v2.0 master-clock refactor.
> See `docs/STATUS.md` version policy for the full residual list.

### Phase 9 — v1.3.0 WebAssembly Target + v1.3.1/.2/.3 patches (COMPLETE)

**Goal:** Ship a `wasm32-unknown-unknown` build of the frontend that runs in
the browser, per the v2.0.0 release plan. No API break (the chip stack is
already `no_std + alloc`).

**Exit criterion (MET):** v1.3.0 tag landed; the frontend builds for wasm32
in two flavours (`wasm-winit` default + `wasm-canvas` embed); a GitHub Pages
demo is live at `https://doublegate.github.io/RustyNES/`; CI gates a
wasm32 clippy build + a 5 MiB compressed size budget. Workspace tests
preserved (599+6 ignored); AccuracyCoin 90.65%, commercial oracle 60/60,
sacred trio + B4 invariant — all preserved bit-identically.

Sprints (all LANDED): 1.1 scaffolding → 1.2 entry point + browser host →
1.3 canvas-2D MVP → 1.4a audio + save state → 1.4b winit/wgpu/egui
unification → 1.4c audio on the unified path → 2 GitHub Pages deploy + CI
wasm32 gate + size budget. See `docs/audit/v1.3-sprint-*.md`.

**Follow-on patches (COMPLETE):** v1.3.1 (left-edge BG attribute-shifter
palette fix + native present-mode/sleep-spin stutter fix + legacy
`config.toml` migration), v1.3.2 (legacy keycode-name aliases fixing
post-migration dead input + first wasm rAF pacing attempt), v1.3.3 (wasm
`ControlFlow::Wait` + unconditional rAF heartbeat fixing the Pages
stutter/freeze regression + WebGL2 UNORM palette fix + native chunked-sleep
pacing). All frontend-only; native pixel-identical; 616 strict + 6 ignored;
AccuracyCoin 90.65% preserved. See `docs/audit/v1.3.x-*.md`.

### Phase 10 — v1.4.0 TAS Movie Recording/Playback (COMPLETE)

**Goal:** Frame-perfect input recording + playback with save-state branching,
per the v2.0.0 release plan. Exposes the already-met determinism contract
(same seed + ROM + input ⇒ bit-identical framebuffer + audio). No API break.

**Exit criterion (MET):** byte-identical record → replay (framebuffer +
audio FNV-1a + cycle count) proven by integration tests on a committed CC0
ROM; save-state-branch replays deterministically; record/play/branch UI
wired (`F6`/`F7`/`F8`); the `.rnm` movie format is versioned for
forward-compat (ADR 0008, layered on ADR 0003). 636 strict + 8 ignored;
oracle 60/60; AccuracyCoin 90.65%; B4 + sacred trio preserved.

Sprints (LANDED): **4.1** — core movie infra in `crates/rustynes-core/src/movie.rs`
(`MovieRecorder`/`MoviePlayer`, `.rnm` serialize/deserialize, the additive
read-only `Nes::buttons` hook; `run_frame` untouched), ADR 0008, +13 tests.
**4.2** — frontend `crates/rustynes-frontend/src/movie_ui.rs` (record/play/branch
hotkeys, `MovieUi` state machine in the frame loop, native `rfd` `.rnm`
save/load, read-only egui REC/PLAY overlay), +7 tests. Clean-room from
Mesen2 `Core/Shared/Movies/` (structural, GPL-3.0) + FCEUX `.fm2` + the
local TetaNES clone (`ref-proj/tetanes`) + nesdev TAS. wasm `.rnm` file I/O
deferred to a v1.4.x follow-up (UI compiles + no-ops on wasm). See
`docs/adr/0008-tas-movie-format.md`.

### Release engineering (v1.x)

- [→] **CI: `macos-15-intel` runner sunset — August 2027.** GitHub will
  decommission the `macos-15-intel` label after that date (per
  `actions/runner-images#13045`). Plan: migrate to `cargo-zigbuild`
  cross-compile from Linux, or drop `x86_64-apple-darwin` from the
  release binary matrix. Non-blocking forward reminder. The Session-22
  `macos-13` → `macos-15-intel` migration (commit `a9333ba`,
  `.github/workflows/release.yml` +
  `docs/audit/ci-release-workflow-macos-x86_64-2026-05-22.md`) resolved
  the prior deprecation; this entry tracks the next deadline.

---

## Cross-phase dependencies

- Phase 2 Sprint 4 depends on Phase 1 complete (CPU core).
- Phase 3 depends on Phase 1 (CPU) and Phase 2 Sprint 4 (scheduler) complete; Sprint 2 of Phase 3 depends on Phase 2 Sprint 4 (DMA).
- Phase 4 depends on Phase 2 (PPU) for mapper-PPU integration; Phase 3 (APU) for audio-extension mappers.
- Phase 5 depends on all previous phases.
- Phase 7 depends on the Phase 6 closeout decision for which v1.0 residuals
  carry forward. It also depends on the Nesdev checklist staying current with
  upstream source pages and local `docs/STATUS.md` pass counts.

## Open questions blocking planning

None block Phase 1. Open questions in the docs (esp. `architecture.md`, `mappers.md`) will be revisited at the start of the phase that needs them resolved.
