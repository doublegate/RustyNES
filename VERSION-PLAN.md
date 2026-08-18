# RustyNES Version Plan

**Current release: v2.3.6 "Sounding"** — measuring, and what a measurement may claim. Two shipped features are found never to have worked: **Pixel Provenance** returned an empty report for every user on the default `run_ahead = 1` (its rollback is the last thing before the frontend takes the lock, so the panel always looked after the wipe) and "click any pixel" was never implemented — two comments and four doc claims asserted the opposite of their own code, which is why four releases passed unchecked; and **Duck Hunt could never score**, its Zapper probe exactly inverting the "see nothing, then a bright spot" protocol. Two new tools built to **decline rather than guess**: the **Latency Oracle** (measures the game's own input lag; recommends a run-ahead depth and never applies one) and the **RAM Atlas** (classifies all 2 KiB of work RAM, then *verifies* a candidate by perturbing it — `Untested` is a third state distinct from `Inert`, and liveness names its lens). **APU Workstream D is closed** on three measured rejections plus the fat-LTO mechanism explaining them. Tools and Debug are regrouped by task. Core gains one `const fn` getter, so AccuracyCoin 141/141 is verified, not asserted. Built on **v2.3.5 "Manifest"** — the declaration release: what the core says about itself. A user reported RetroArch still showing the pre-relicense MIT/Apache-2.0 terms, and it was: RetroArch reads `dist/info/` from **libretro/libretro-super**, a SEPARATE copy nothing synced, so the v2.2.9 GPL relicense never reached the file users see. Corrected to `GPLv3+` with a standing `libretro_info_audit.rs` that makes the upstream sync a **copy** rather than a re-derivation, and a licence change is now a mandatory upstream-sync trigger. Auditing the wrapper then found **five further defects, every one with correct emulation behind it** — PAL ran 20.2% fast, Reset did nothing ever, unload leaked Game Genie indices, the aspect ratio assumed square pixels, and the Zapper was unreachable — plus a **use-after-free** in the controller tables caught in review. The crate went from zero tests to eight. The APU also gained its first throughput bench and a default-configuration mix specialization (−3.3% to −4.2% on `nes_run_frame_nestest`), so **AccuracyCoin 141/141 was VERIFIED, not asserted**. Built on **v2.3.4 "Ledger"** — the coverage release: three boards (mapper 176 submapper 2 WAIXING-FS005, 154 NAMCOT-3453, 243 Sachen SA-020A, breadth **172 → 174 families**), the coverage harness moved onto the frontend's real load path, and the defect that exposed — the per-game database reading a `0` Mapper column as "force NROM" and overwriting correct headers, leaving **every Sachen cartridge** unloadable since **v1.2.0**. **This release touches the emulation core**, so AccuracyCoin exactly 141/141 is **verified, not asserted by construction**. Its Workstream C (the APU at 18.7% of frame time) was carried to v2.3.5 and delivered there. Built on **v2.3.3 "Cadence"** — the display-pacing release: the run-ahead throttle oscillation traced to a stale median (a gate counting 120 frames of a 600-sample ring), a predictive engage arm that converges a `run_ahead = 3` host in 2.8 s instead of 12.1 s, and the `wp_presentation` measurement apparatus that made the diagnosis possible. **No emulation-core changes** (AccuracyCoin exactly 141/141). Built on **v2.3.2 "Lucid"** (pixel provenance + deterministic replay attestation), **v2.3.1 "Plumb Line"** (ten measured rejections), and **v2.3.0 "Datum II"**, the capstone that **closed** the v2.2.6 → v2.3.0 line (true multi-viewport OS-window detach, the emulator-lock frame-pacing fix, a −5.1% byte-identical PPU optimization, and both forum-reported accuracy items verified already-correct) — all on the **v2.0.0 "Timebase"** MAJOR base (the one-clock / every-cycle-bus-access scheduler rewrite). **v1.0.0** was the first stable, production cut. As of **v2.2.9**, RustyNES is **GPL-3.0-or-later** — a derivative work of GPL-licensed emulators (ADR 0036); a licensing correction, **not** a SemVer break (no public-API or save-state change). `docs/STATUS.md` is the authoritative current-state record; `CHANGELOG.md` carries the full per-release history.

RustyNES follows [Semantic Versioning 2.0.0](https://semver.org/).

## What v1.0.0 means

v1.0.0 is the **production cut that integrates the cycle-accurate emulation engine** (the `rustynes-*` crates) with the desktop UX shell and the documentation synthesis. It is "1.0" because the emulator clears the reference accuracy bar and ships the full platform feature set — it is **not** gated on any "300 mappers / 100% of the TASVideos catalog / Lua scripting" bar. The criteria that were actually met:

- **AccuracyCoin 100.00% (139/139)** and **`nestest` 0-diff**.
- A stable public core API (`rustynes-core::Nes`), a stable save-state format, and a stable on-disk movie format (`.rnm`).
- A complete, shippable desktop application (menu bar, settings, themes, debugger) plus a browser build.
- Green CI across Linux/macOS/Windows + wasm32, with a `no_std` chip-stack cross-compile.

## Version number components

```text
MAJOR.MINOR.PATCH[-PRERELEASE]
```

- **MAJOR** — incompatible public-API or save-state-format breaks (now at `2`, since **v2.0.0 "Timebase"** broke the `.rns` save-state / `.rnm` movie epochs per ADR 0028).
- **MINOR** — backwards-compatible features (new mappers, new frontend features, new platforms).
- **PATCH** — backwards-compatible bug fixes and accuracy refinements.
- **PRERELEASE** — `-alpha.N` / `-beta.N` / `-rc.N` when stabilizing a future minor/major.

## Version history

The pre-1.0 line tracked the MVP-through-stabilization milestones; the engine integration that produced the production cut is recorded as documentary stages **v0.9.0–v0.9.7**, culminating in **v1.0.0**.

### Pre-1.0 development (v0.1.0 – v0.8.6)

| Version | Milestone |
|---------|-----------|
| **v0.1.0** | 6502 CPU (all 256 opcodes) + 2C02 PPU; `nestest` golden-log validation |
| **v0.2.0** | 2A03 APU (all 5 channels), non-linear mixer + resampler |
| **v0.3.0** | First 5 mappers (NROM, MMC1, UxROM, CNROM, MMC3); iNES + NES 2.0 parsing |
| **v0.4.0** | Full core integration + test-ROM validation framework + controller input |
| **v0.5.0** | Desktop GUI — MVP release |
| **v0.6.0** | Accuracy pass — CPU/PPU/APU timing, OAM DMA cycle precision, hardware mixer |
| **v0.7.0 – v0.7.1** | Blargg test-ROM validation; desktop GUI iteration |
| **v0.8.0 – v0.8.6** | Dependency modernization; UI/UX polish (themes, status bar, tabbed settings); sub-cycle accuracy work (DMC DMA cycle stealing, open-bus behavior, per-cycle mapper clocking) |

### Engine integration → production (documentary stages, culminating in v1.0.0)

The cycle-accurate engine was integrated as the core in a sequence of documentary stages. Each stage corresponds to a body of upstream engine-lineage work folded into RustyNES:

| Stage | Content |
|-------|---------|
| **v0.9.0** | Cycle-accurate core on the PPU-dot lockstep scheduler |
| **v0.9.3** | Master-clock-precise scheduler reaching **AccuracyCoin 100% (139/139)** |
| **v0.9.4** | Famicom Disk System (real-BIOS boot, read/write, multi-side, FDS audio) |
| **v0.9.5** | Rollback netplay (GGPO-style, UDP + WebRTC) |
| **v0.9.6** | Platform + RetroAchievements (Vs. System / PlayChoice-10 RGB, opt-in RA) |
| **v0.9.7** | Performance pass (display-sync pacing, dedicated emu thread, audio DRC, run-ahead) |
| **v1.0.0** | Production cut — engine + ported desktop UX shell + documentation synthesis |

> **Engine lineage note.** The deep technical history under `docs/` (the `v2.0` master-clock refactor, ADRs, audit logs, the long accuracy program) describes the **upstream engine lineage**. Those old "v1.x"/"v2.x" anchors are engineering history, **not** RustyNES release versions. RustyNES's own release line is v0.1.0 → v0.8.6 → (documentary v0.9.0–v0.9.7) → **v1.0.0** → the v1.1.0–v1.10.0 additive feature line → **v2.0.0 "Timebase"** (the designated MAJOR break) → the v2.0.x "Harbor" line → the v2.1.x "Fathom" accuracy line → the v2.2.x line → v2.3.0 "Datum II" → v2.3.1 "Plumb Line" → v2.3.2 "Lucid" → v2.3.3 "Cadence" → v2.3.4 "Ledger" → **v2.3.5 "Manifest"** (current).

### Post-1.0 release line (v1.1.0 → current)

The 1.x line was **additive / off-by-default** — every release stayed byte-identical to v1.0.0 with new features off. It grew desktop tooling (Lua, HD-packs, a Mesen2-class debugger, TAStudio, A/V recording) and, in the v1.8.0–v1.10.0 minors, whole new platforms — a native Android app, an iOS / iPadOS TestFlight train, and a Libretro / RetroArch core — while the mapper catalog grew to 172 families. See `CHANGELOG.md` for the per-release detail.

| Version | Milestone |
|---------|-----------|
| **v1.1.0 – v1.7.1** | Additive desktop-feature line (scripting, HD-packs, debugger, TAStudio, shaders, mapper breadth) |
| **v1.8.0 – v1.8.9** | Native Android app (UniFFI bridge + JNI host + Compose), GitHub-Releases sideload |
| **v1.9.0 – v1.9.9** | Native iOS / iPadOS app (Metal + SwiftUI), interim TestFlight |
| **v1.10.0 "Arcade"** | Native Libretro / RetroArch core |
| **v2.0.0 "Timebase"** | **Designated MAJOR break** — one-clock / every-cycle-bus-access scheduler rewrite; `.rns`/`.rnm` epochs bump (ADR 0028); core-level Vs. `DualSystem` support. AccuracyCoin 100% (139/139) |
| **v2.0.1 "Harbor"** | First Android re-port onto Timebase + AccuracyCoin oracle re-sync (catalog → 146 rows / 141 assigned tests; briefly 139/141) |
| **v2.0.2 – v2.0.3 "Harbor"** | 2-cycle-ALE PPU fetch model promoted to the unconditional default → **AccuracyCoin 100.00% (141/141)** ("ALE + Read" + "Hybrid Addresses" now pass) |
| **v2.0.4 – v2.0.9 "Harbor"** | Mobile finalization on Timebase — Android release candidate (v2.0.4), iOS re-port + polish + App-Store floor (v2.0.5–v2.0.8), both-apps readiness (v2.0.9); host-only, core byte-identical to v2.0.3 |
| **v2.1.0 – v2.1.10 "Fathom"** | Accuracy / display / audio / creator-tools line — palette-backdrop + mapper-tier completion (v2.1.0), the W&W game-DB freeze fix (v2.1.1), display fidelity (v2.1.2 "Prism"), QoL (v2.1.3 "Codex"), accuracy hardening (v2.1.4 "Caliper"), regression net (v2.1.5 "Vernier"), expansion audio (v2.1.6 "Timbre"), hardware revisions (v2.1.7 "Stepping"), performance (v2.1.8 "Tempo"), presentation + CRT shaders (v2.1.9 "Aperture"), creator tools + web parity (v2.1.10 "Loom") — all NTSC byte-identical, **141/141** |
| **v2.2.0 "Capstone"** | Milestone cut closing the v2.1.5 → v2.2.0 "deepen the project" run — netplay matchmaking/lobby + FDS medium model + peripherals & quality/security pass |
| **v2.2.1 – v2.2.5** | Housekeeping (v2.2.1); build / distribution / CI-integrity — libretro buildbot + supply-chain hardening (v2.2.2 "Conduit"); performance + accuracy-closure (v2.2.3 "Datum"); libretro/RetroArch distribution (v2.2.4 "Cartridge"); provenance / licensing / documentation integrity (v2.2.5 "Colophon") |
| **v2.2.6 – v2.2.9** | The **de-monetization + NESdev-remediation** line — RustyNES made permanently open-source and income-free (v2.2.6 "Almanac", ADR 0035); expansion-audio fidelity (v2.2.7 "Timbre II"); gamma-correct presentation (v2.2.8 "Aperture II"); TAS/movie wiring + detachable tool windows + the **relicense to GPL-3.0-or-later** (v2.2.9 "Studio II", ADR 0036) |
| **v2.2.9 "Studio II"** | TAS/movie wiring + the GPL-3.0-or-later relicense — see `CHANGELOG.md` `[2.2.9]` |
| **v2.3.0 "Datum II"** | Head of the v2.x line; **closes** the v2.2.6 → v2.3.0 remediation line. PPU-accuracy capstone — SMB left-edge + hybrid-address (Rad Racer) verified already-correct against the AccuracyCoin oracle and locked with an exact-141/141 regression gate; hybrid-address provenance finalized (doc/oracle-derived); true multi-viewport OS-window detach; the emulator-lock frame-pacing fix; a −5.1% byte-identical PPU optimization — see `CHANGELOG.md` `[2.3.0]` |
| **v2.3.1 "Plumb Line"** | Measurement apparatus made trustworthy, then used: a harness-free frame probe, per-source-file subsystem attribution (which recovers the **APU at 18.7% of frame**, invisible in the symbol profile), an adoption A/B with an A/B/A order-bias control, and a contention-aware relative gate. **Ten core hot-path candidates measured, all ten rejected** via six distinct mechanisms — **no emulation-core changes**, AccuracyCoin exactly 141/141 — see `CHANGELOG.md` `[2.3.1]` |
| **v2.3.2 "Lucid"** | Pixel provenance — click any pixel for its full causal chain, down to **the CPU instruction and cycle that last wrote each byte** — plus deterministic replay attestation (`rustynes verify`). All `debug-hooks`-gated and output-only, so AccuracyCoin holds exactly 141/141 — see `CHANGELOG.md` `[2.3.2]` |
| **v2.3.3 "Cadence"** | Display pacing. The run-ahead throttle oscillation attributed to a **stale median** — the gate counted 120 frames of a 600-sample ring, so a p50 at index 300 could not leave the previous depth; **6-7 transitions per 24 s → 1**, spurious releases **2 → 0**. The engage arm now predicts instead of waiting (`run_ahead = 3` converges in **2.8 s vs 12.1 s**, 5/5 paired rounds, p = 0.0312) while releasing still demands a real measurement. Compositor refresh via `wp_presentation`, divisor display-sync, and a validity gate that fails closed; dropped frames **135-254 → 1-9**. Two arms measured and **rejected** with their numbers. No emulation-core changes — see `CHANGELOG.md` `[2.3.3]` |
| **v2.3.4 "Ledger"** | Mapper coverage. Three boards — **176 submapper 2** (WAIXING-FS005), **154** (NAMCOT-3453), **243** (Sachen SA-020A) — breadth **172 → 174** (51 Core + 95 Curated + 28 BestEffort), all implemented from the NESdev wiki with no reference-emulator source consulted. The coverage harness moved onto the frontend's real load path, which exposed a **v1.2.0-era defect reaching users**: the per-game database read a `0` Mapper column as "force NROM" and overwrote correct headers, leaving **12 ROMs — every Sachen board in the corpus** — unable to load. Also a Bandai FCG EEPROM debug panic, CLI launches skipping header overrides, mapper 15 PRG-RAM/CHR-RAM, save-state back-compat for 15/88/176, and #360. **Touches the core**, so AccuracyCoin 141/141 is verified, not construction. Workstream C (the APU at 18.7%) **not delivered**, carried to v2.3.5 — see `CHANGELOG.md` `[2.3.4]` |
| **v2.3.5 "Manifest"** (current) | What the core declares about itself. RetroArch reads `dist/info/rustynes_libretro.info` from **libretro/libretro-super**, a SEPARATE copy from this repo's that nothing synced — so the v2.2.9 GPL relicense reached `Cargo.toml`, `NOTICE`, `deny.toml` and the SPDX headers, and **not the file users see**, which advertised MIT/Apache-2.0 at `v2.2.1` for eleven days. Corrected to **`GPLv3+`** (libretro uses short tokens and marks "or later" with a trailing `+`, tallied across all 316 upstream cores) and pinned by a standing `libretro_info_audit.rs`, so the sync is a copy rather than a re-derivation; **a licence change is now a mandatory upstream-sync trigger**. The wrapper audit that followed found **five defects, each with correct emulation behind it**: a hardcoded 60.0988 fps for every cartridge with `retro_get_region` unimplemented (**PAL ran 20.2% fast**), `retro_reset` unimplemented (**RetroArch's Reset did nothing, ever** — the library default is a literal no-op), `retro_unload_game` unimplemented (Game Genie indices leaked across cartridges), `aspect_ratio = 0.0` (square pixels against the desktop frontend's 8:7), and no controller info (**the Zapper was unreachable** despite `Nes::set_zapper` being fully implemented). Review caught a **use-after-free**: RetroArch shallow-`memcpy`s the outer `retro_controller_info` array but RETAINS each `types` pointer, so those tables must be `'static` — `SET_INPUT_DESCRIPTORS` is different and safe, and the two must never be generalized between. The crate went **0 tests → 8**. Separately the APU (18.7% of frame time, invisible to a symbol profile because fat LTO inlines it into `cpu_clock`) gained its first throughput bench and a default-configuration mix specialization, **−3.3% to −4.2%** on `nes_run_frame_nestest`. Declared values are now DERIVED from `rustynes_core` constants rather than transcribed. **The APU implementation changed**, so AccuracyCoin 141/141 and nestest 0-diff are **verified, not true by construction**. NOT fixed here: RetroArch shows the right licence only once libretro merges, and iOS/iPadOS/tvOS availability is a hardcoded `appstore_cores` list in `libretro/RetroArch` — both upstream — see `CHANGELOG.md` `[2.3.5]` |

> **Forward path.** The v2.0.x "Harbor", v2.1.x "Fathom", and v2.2.x lines have all shipped; the v2.2.6 → v2.3.0 line has now **closed** with v2.3.0 "Datum II"; the v2.3.x performance campaign has now **shipped in full**, as three releases: **v2.3.1 "Plumb Line"** absorbed both the measurement apparatus and the core hot-path campaign, whose ten items were all measured and all rejected and so had no shippable content of their own; **v2.3.2 "Lucid"** the novel features (pixel provenance + replay attestation); and **v2.3.3 "Cadence"** the display-pacing work — the run-ahead throttle oscillation traced to a stale median, the predictive engage arm, and the `wp_presentation` measurement apparatus that made the diagnosis possible. The campaign closed there; **v2.3.4 "Ledger"** opened the next line with mapper coverage — three boards to **174 families**, and the coverage harness moved onto the frontend's real load path, which exposed a per-game-database defect that had left every Sachen cartridge unloadable since v1.2.0. Its Workstream C, the APU at 18.7% of frame time, was not delivered there and landed in **v2.3.5 "Manifest"** (current), which is otherwise about what the core declares about itself: the libretro `.info` licence drift a user reported, and the five wrapper defects auditing it uncovered. Note the codenames diverged from this plan as written: what shipped as v2.3.2 took "Lucid" rather than the planned "Grain"/"Conduit II", and v2.3.3 is "Cadence". RustyNES is **permanently open-source and income-free** (ADR 0035): the earlier "joint Google Play + App Store + AltStore + F-Droid launch" is **withdrawn** — any store listing is a **free** app with **no monetization** (no ads, tracking, or paid unlock), an unversioned later step. `to-dos/ROADMAP.md` is the authoritative forward roadmap.

## Versioning guidelines

- **Bump MINOR** (the middle digit — e.g. `vMAJOR.MINOR.0`) for: new mapper families, new frontend features, new platforms (e.g. mobile), new input devices — anything backwards-compatible that adds capability.
- **Bump PATCH** (the last digit — e.g. `vMAJOR.MINOR.PATCH`) for: bug fixes, accuracy refinements, dependency bumps, and documentation that does not change behavior.
- **Bump MAJOR** (`vMAJOR.0.0`) only for: an incompatible public-API break or a save-state-format break that cannot migrate — exactly what **v2.0.0 "Timebase"** did (ADR 0028 bumped the `.rns`/`.rnm` epochs).

### Breaking-change policy

- Public-API and save-state-format breaks are MAJOR bumps and must be documented in `CHANGELOG.md` with a migration note.
- Save-state cross-version compatibility is best-effort (tagged per-chip sections with a version byte); the on-disk `.rnm` movie format and the public `rustynes-core` API are the stable surfaces.

## Accuracy milestones (met)

- `nestest` 0-diff, blargg / kevtris suites green, **AccuracyCoin 100.00% (141/141)** from **v2.0.3** onward (139/139 at the v1.0.0 cut; the v2.0.1 oracle re-sync grew the catalog to 141 assigned tests and briefly opened two PPU gaps, so v2.0.1–v2.0.2 shipped an honest 139/141 until the v2.0.3 2-cycle-ALE promotion closed them), and a byte-identical 60-ROM commercial regression oracle. As of v2.3.0 the AccuracyCoin gate is pinned to an **exact 141/141** (zero failing tests), so a single-test regression — e.g. in the hybrid-address model — fails CI. `docs/STATUS.md` is the authoritative pass-count source.

## Git tagging

- Tag format: `vMAJOR.MINOR.PATCH` (e.g. `v1.0.0`).
- Tags are annotated; release notes summarize the `CHANGELOG.md` entry. CI builds release binaries for Linux/macOS/Windows and deploys the wasm build to GitHub Pages.

## Release workflow (summary)

1. Land all changes under `CHANGELOG.md` `[Unreleased]`.
2. Run the full quality gate (`fmt` / `clippy` / `doc` / tests / `no_std` cross-compile / wasm size budget).
3. Move `[Unreleased]` to the new version section, bump the workspace version, tag, and push.
4. Verify the GitHub release notes and the Pages deploy after CI.

## Related documentation

- [`ROADMAP.md`](ROADMAP.md) — delivered milestones and post-1.0 directions.
- [`CHANGELOG.md`](CHANGELOG.md) — full release history.
- [`docs/STATUS.md`](docs/STATUS.md) — authoritative status matrix.
