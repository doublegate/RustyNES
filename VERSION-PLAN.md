# RustyNES Version Plan

**Current release: v1.0.0** — the first stable, production cut.

RustyNES follows [Semantic Versioning 2.0.0](https://semver.org/).

## What v1.0.0 means

v1.0.0 is the **production cut that integrates the cycle-accurate emulation engine** (the `rustynes-*` crates) with the desktop UX shell and the documentation synthesis. It is "1.0" because the emulator clears the reference accuracy bar and ships the full platform feature set — it is **not** gated on any "300 mappers / 100% of the TASVideos catalog / Lua scripting" bar. The criteria that were actually met:

- **AccuracyCoin 100.00% (139/139)** and **`nestest` 0-diff**.
- A stable public core API (`rustynes-core::Nes`), a stable save-state format, and a stable on-disk movie format (`.rnm`).
- A complete, shippable desktop application (menu bar, settings, themes, debugger) plus a browser build.
- Green CI across Linux/macOS/Windows + wasm32, with a `no_std` chip-stack cross-compile.

## Version number components

```
MAJOR.MINOR.PATCH[-PRERELEASE]
```

- **MAJOR** — incompatible public-API or save-state-format breaks (now at `1`).
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

> **Engine lineage note.** The deep technical history under `docs/` (the `v2.0` master-clock refactor, ADRs, audit logs, the long accuracy program) describes the **upstream engine lineage**. Those old "v1.x"/"v2.x" anchors are engineering history, **not** RustyNES release versions. RustyNES's own release line is v0.1.0 → v0.8.6 → (documentary v0.9.0–v0.9.7) → **v1.0.0**.

## Versioning guidelines

- **Bump MINOR (v1.x.0)** for: new mapper families, new frontend features, new platforms (e.g. mobile), new input devices — anything backwards-compatible that adds capability.
- **Bump PATCH (v1.0.x)** for: bug fixes, accuracy refinements, dependency bumps, and documentation that does not change behavior.
- **Bump MAJOR (v2.0.0)** only for: an incompatible public-API break or a save-state-format break that cannot migrate.

### Breaking-change policy

- Public-API and save-state-format breaks are MAJOR bumps and must be documented in `CHANGELOG.md` with a migration note.
- Save-state cross-version compatibility is best-effort (tagged per-chip sections with a version byte); the on-disk `.rnm` movie format and the public `rustynes-core` API are the stable surfaces.

## Accuracy milestones (met)

- `nestest` 0-diff, blargg / kevtris suites green, **AccuracyCoin 100.00% (139/139)**, and a byte-identical 60-ROM commercial regression oracle. `docs/STATUS.md` is the authoritative pass-count source.

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
