# RustyNES Development Roadmap

**Document Version:** 2.0.4
**Last Updated:** 2026-07-09
**Project Status:** v2.0.4 "Harbor" released — the v2.0.x "Harbor" mobile-finalization train atop the v2.0.0 "Timebase" MAJOR cut.

---

## Where we are

RustyNES is well past v1.0.0. The current release is **v2.0.4 "Harbor"** (2026-07-09) — the head of the v2.0.x "Harbor" mobile-finalization train, sitting atop **v2.0.0 "Timebase"** (2026-07-03), the designated MAJOR cut that replaced the PPU-dot lockstep scheduler with the one-clock / every-cycle-bus-access model. Between v1.0.0 and v2.0.0 the additive v1.x line delivered deep desktop tooling and three whole new platforms (native Android, iOS / iPadOS, and a Libretro / RetroArch core); the v2.0.x train then re-ported mobile onto the new core and, at **v2.0.3**, promoted the 2-cycle-ALE PPU fetch model to the default to reach **AccuracyCoin 100% (141/141)**.

**This root ROADMAP is a historical snapshot of the v1.0.0 cut.** For the authoritative, current forward roadmap see **[`to-dos/ROADMAP.md`](to-dos/ROADMAP.md)**; for the authoritative current-state pass counts and platform matrix see **[`docs/STATUS.md`](docs/STATUS.md)**; for the full per-release history see **[`CHANGELOG.md`](CHANGELOG.md)**. Many of the "post-1.0 directions" listed further down (mobile, Lua scripting, TAS editor, Vs. DualSystem, HD packs, hosted netplay) have since shipped — the tables below record what was **done at v1.0.0**, not the current feature set.

> **Note on versioning.** v1.0.0 is the production cut that integrates the cycle-accurate emulation engine with the ported desktop UX shell and documentation synthesis. Deep technical narrative under `docs/` (the master-clock refactor, ADRs, audit logs) references the upstream engine lineage — read those as engineering history, not RustyNES release numbers.

---

## Delivered at v1.0.0

### Accuracy (DONE)

- **AccuracyCoin 100.00% (139/139)** (RAM-direct decoder).
- **`nestest` 0-diff** against the Nintendulator golden log.
- blargg / kevtris / `mmc3_test_2` suites green; the master-clock-precise PPU-dot lockstep scheduler is the only path (no legacy fallback).
- Region timing (NTSC / PAL / Dendy) modeled as data, with the exact CPU:PPU ratios (3:1 NTSC/Dendy, 3.2:1 PAL).
- A 60-ROM commercial-ROM regression oracle plus an extended commercial survey, all visually verified.

### Cartridge / platform compatibility (DONE)

- **51 mapper families** (NROM, the MMC1–MMC5 line, the VRC1/2/4/6/7 family, FME-7, Namco 163, and the broad Taito / Sunsoft / Irem / Jaleco / Bandai / Konami long tail), including expansion audio (VRC6, VRC7 OPLL FM, Sunsoft 5B, Namco 163, MMC5, FDS 2C33).
- **Famicom Disk System** — real-BIOS boot, disk read/write, multi-side swap, writable `.fds.sav` persistence.
- **Vs. System / PlayChoice-10** — hardware RGB PPU palettes (2C03/2C04/2C05), DIP switches, coin/service input.

### Features (DONE)

- **Rollback netplay** — GGPO-style, deterministic re-simulation; UDP (native) + WebRTC (browser); 2–4 players; live-verified.
- **RetroAchievements** — opt-in, native-only, over the vendored rcheevos C library (login, hardcore, unlock toasts, rich presence, badge images).
- **TAS movies** (`.rnm`) — frame-perfect deterministic record / playback / branching.
- **Save-states, rewind, run-ahead**, Game Genie + raw-RAM cheats, Four Score, Arkanoid Vaus + Zapper input.

### Frontend & UX (DONE)

- Pure-Rust frontend (`winit` + `wgpu` + `cpal` + `egui`).
- Display-sync pacing matrix, a dedicated emulation thread, late-latched input, a lock-free audio ring with dynamic rate control — for the smoothest, lowest-latency play.
- **Desktop UX shell** — native menu bar, recent-ROMs list, tabbed Settings window, light/dark/system themes, 8:7 pixel-aspect correction, status bar.
- An egui debugger overlay (CPU/PPU/APU/memory views, performance panel) and an optional NTSC post-process filter.
- **WebAssembly / GitHub Pages** build (winit+wgpu flavour and a lightweight canvas embed), live at <https://doublegate.github.io/RustyNES/>.

### Engineering quality (DONE)

- The chip stack is `#![no_std]` + `alloc`, cross-compiled in CI to `thumbv7em-none-eabihf`.
- CI gates: `fmt`, `clippy --all-targets -D warnings` (incl. wasm32), `doc` (warnings-as-errors), multi-platform tests (Linux/macOS/Windows), MSRV pin (1.86), a frame-time regression bench, and a wasm size budget.
- Dual-licensed MIT OR Apache-2.0.

---

## Post-1.0 directions (not committed)

These are candidate directions, not promises or a dated plan. They are ordered roughly by interest, not priority.

| Area | Description | Status |
|------|-------------|--------|
| **Mobile** | iOS / Android frontends over the existing core | Not started |
| **Mapper long tail** | Additional and obscure mapper families as compatibility gaps surface | Ongoing, demand-driven |
| **RetroAchievements allowlisting** | A live RA-account pass to get the client server-side allowlisted | Pending (request to the RA team) |
| **Vs. DualSystem** | Two-CPU/two-PPU Vs. carts (Tennis / Mahjong / Wrecking Crew / Balloon Fight) | Designed, deferred |
| **FDS side-B / interactive-boot games** | Kid Icarus FDS name-registration path and similar | Investigation item |
| **Lua scripting** | A scripting API for tooling / TAS | Not built; candidate only |
| **Hosted netplay infra** | A hosted signaling + STUN/TURN deployment for browser netplay | Reference bundle exists; not hosted |
| **TAS editor** | A piano-roll editor on top of the `.rnm` movie format | Not built; candidate only |
| **Video filters** | Additional CRT / scaling shaders beyond the NTSC filter | Candidate only |

---

## Working conventions

- Tickets live under `to-dos/` with stable IDs (`T-PS-NNN`); reference them in commits.
- For accuracy work, pin the failing test-ROM expectation first, then implement until it passes.
- `docs/STATUS.md` is the authoritative per-suite pass-count + mapper-coverage matrix.

## Related documentation

- [`VERSION-PLAN.md`](VERSION-PLAN.md) — versioning strategy and history.
- [`ARCHITECTURE.md`](ARCHITECTURE.md) / [`OVERVIEW.md`](OVERVIEW.md) — system design and project vision.
- [`CHANGELOG.md`](CHANGELOG.md) — full release history (incl. engine lineage).
- [`docs/STATUS.md`](docs/STATUS.md) — status matrix.
