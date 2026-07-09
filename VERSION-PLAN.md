# RustyNES Version Plan

**Current release: v2.0.4 "Harbor"** — the head of the v2.0.x "Harbor" mobile-finalization train, atop the **v2.0.0 "Timebase"** MAJOR cut (the one-clock / every-cycle-bus-access scheduler rewrite). **v1.0.0** was the first stable, production cut. `docs/STATUS.md` is the authoritative current-state record; `CHANGELOG.md` carries the full per-release history.

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

> **Engine lineage note.** The deep technical history under `docs/` (the `v2.0` master-clock refactor, ADRs, audit logs, the long accuracy program) describes the **upstream engine lineage**. Those old "v1.x"/"v2.x" anchors are engineering history, **not** RustyNES release versions. RustyNES's own release line is v0.1.0 → v0.8.6 → (documentary v0.9.0–v0.9.7) → **v1.0.0** → the v1.1.0–v1.10.0 additive feature line → **v2.0.0 "Timebase"** (the designated MAJOR break) → the v2.0.x "Harbor" line (current: **v2.0.4**).

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
| **v2.0.4 "Harbor"** (current) | Android release candidate — host-only RC scaffolding; core byte-identical to v2.0.3 |

> **Forward path.** The remaining v2.0.x "Harbor" steps — **v2.0.5 → v2.0.8** iOS finalization → **v2.0.9** both-apps readiness — lead to **v2.1.0**, the joint Google Play + Apple App Store + AltStore PAL + F-Droid launch. `to-dos/ROADMAP.md` is the authoritative forward roadmap.

## Versioning guidelines

- **Bump MINOR** (the middle digit — e.g. `vMAJOR.MINOR.0`) for: new mapper families, new frontend features, new platforms (e.g. mobile), new input devices — anything backwards-compatible that adds capability.
- **Bump PATCH** (the last digit — e.g. `vMAJOR.MINOR.PATCH`) for: bug fixes, accuracy refinements, dependency bumps, and documentation that does not change behavior.
- **Bump MAJOR** (`vMAJOR.0.0`) only for: an incompatible public-API break or a save-state-format break that cannot migrate — exactly what **v2.0.0 "Timebase"** did (ADR 0028 bumped the `.rns`/`.rnm` epochs).

### Breaking-change policy

- Public-API and save-state-format breaks are MAJOR bumps and must be documented in `CHANGELOG.md` with a migration note.
- Save-state cross-version compatibility is best-effort (tagged per-chip sections with a version byte); the on-disk `.rnm` movie format and the public `rustynes-core` API are the stable surfaces.

## Accuracy milestones (met)

- `nestest` 0-diff, blargg / kevtris suites green, **AccuracyCoin 100.00% (141/141)** on the current v2.0.x line (139/139 at the v1.0.0 cut, before the v2.0.1 oracle re-sync grew the catalog to 141 assigned tests), and a byte-identical 60-ROM commercial regression oracle. `docs/STATUS.md` is the authoritative pass-count source.

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
