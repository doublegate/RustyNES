# RustyNES Development Roadmap

**Document Version:** 2.0.4
**Last Updated:** 2026-10-04
**Project Status:** v3.0.0 "Cornerstone" released — the API major: every break since v2.x in one place, a core timing epoch for movies and netplay, the last MMC3 timing gap closed in both cores, and a release-candidate MiSTer core. Built on **v2.9.9 "Ballast"** (the release candidate for v3.0.0: all four audit scopes re-run, MMC3 and MMC5 by their documentation, audio exact across save states, and the MiSTer core moved onto it with the release-candidate bitstream pair; the tenth release of the v2.9.x line and the sixth of the line to v3.0.0 (ADR 0043, amended)) and **v2.9.8 "Vanguard"** (v3.0.0's breaking changes landed early) and **v2.9.7 "Tandem"** (the desktop's features on the web and on phones). **No hardware has run any bitstream**; the mobile device runs and the SuperStation One board session move after v3.0.0 (maintainer, 2026-09-29).

---

## Where we are

RustyNES is well past v1.0.0. The current release is **v3.0.0 "Cornerstone"** — the API major: every break since v2.x in one place, a core timing epoch for movies and netplay, the last MMC3 timing gap closed in both cores, and a release-candidate MiSTer core. Built on **v2.9.9 "Ballast"** — the release candidate for v3.0.0: the audits re-run, MMC3 and MMC5 by their documentation, audio exact across save states, and the MiSTer core moved onto it. Built on **v2.9.8 "Vanguard"** — the preparation release for v3.0.0: v3.0.0's breaking changes landed early (a save identity that ignores the header, old states and movies refused, movies and netplay that record the machine, the API removals), every staged game was booted and the defects found were fixed, and the game database's corrections reach every platform. Built on **v2.9.7 "Tandem"** — the desktop's features on the web and on phones, the release binaries built with every native feature, and a PPU A12 fix found by real games: Acclaim's MC-ACC games, the J.Y. ASIC and mapper 91 now count at their documented rates. Built on **v2.9.6 "Roster"** — seventeen mapper families written from their NESdev pages (174 → 191), GTROM promoted to Curated with a modelled flash chip whose saves persist, mapper 4's NES 2.0 submappers corrected (MMC6, NEC, MC-ACC, T9552), and the local commercial suites re-baselined after drifting unread since about v2.0.0. Built on **v2.9.5 "Caliper"** — every open accuracy item measured, then fixed or closed: four fixes red first (the `apu_test` frame-counter coincidence, the composite 2C02 scanline-0 sprite glitch, OAM DMA filling the PPU I/O latch, KS7032 at `$6000`), 49 unreferenced test ROMs gated, the MMC3 M2-edge filter lever tried and refuted, and a save-state epoch (`PPU_SNAPSHOT_VERSION` 11). Built on **v2.9.4 "Plumb"** — the records made true, and CI made to run what it only linted: v3.0.0 decided as the API major with a release-candidate core (ADR 0043), CI now running 63 feature-gated tests it never ran, the eight fuzz targets and a 70% line-coverage floor, the mapper tiers, store status and deferred-features catalogue corrected against the code, and the OAM-decay model recorded as derived from Mesen2. Built on **v2.9.3 "Handset"** — the old review threads closed and the mobile run prepared: every dependency moved to its newest release (egui 0.36 with wgpu 30, rcheevos 12.5.0), all 244 review threads left unanswered on PRs #7-#97 answered and the ten findings that still held fixed (Action 53 multicarts rebuilt to the NESdev spec, and a ROM header editor that no longer rewrites bytes you did not edit or saves mappers from 16 up as the wrong mapper), and the Android unit tests and the iOS renderer added to CI. Built on **v2.9.2 "Candidate"** — the full audit acted on, and the release-candidate pair: all 32 findings of a fifth audit have a verdict and 16 are fixed, save states keep the cartridge RAM of twelve board families they used to drop, the MiSTer core no longer loses an NMI raised inside a DMA, and both bitstreams are cut for the SuperStation One session. Built on **v2.9.1 "Hone"** — what the optimisation bars measure, and what clears them: the A/B tool had been timing the old code on both sides of every code comparison and is fixed, a two-screen Vs. cabinet saves about 9x faster, the off-die MiSTer build keeps CHR in its own SDRAM bank, and both bitstreams are pinned at fitter seed 2 and rebuild byte-identically. Built on **v2.9.0 "Survey"** — every audit re-checked, and the SuperStation One surveyed: a Power Cycle no longer erases your save, the off-die MiSTer build boots without the menu core, and 39 new audit findings are fixed or dispositioned. Built on **v2.8.4 "Tether"** — the MiSTer core's SDRAM build, made trustworthy: its controller now reads data on the edge the memory presents it (every off-die read would have been wrong on hardware, and only the new SDRAM timing constraints could see it), the power-up sequence and CAS-latency-3 reads follow the datasheet, the arbiter can no longer return the wrong byte or lose a write, the off-die bitstream builds from a script, both builds are swept and pinned at fitter seed 5, and the co-simulation ladder runs all 165 gates from a clean checkout. Built on **v2.8.3 "Rivet"** — the MiSTer core's reset, area and comments, measured: every reset is released on the clock that uses it and the timing analysis now checks each release, the CPU is about 4% smaller by two exact rewrites the fit report confirmed, four false comments are corrected, and the co-simulation ladder runs from a fresh checkout (164 of its 165 gates; the last needs a hand-built ROM no generator produces). **No hardware has run any bitstream.**

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
- CI gates: `fmt`, `clippy --all-targets -D warnings` (incl. wasm32), `doc` (warnings-as-errors), multi-platform tests (Linux/macOS/Windows), MSRV pin (1.96), a frame-time regression bench, and a wasm size budget.
- Licensed GPL-3.0-or-later (RustyNES is a derivative work of GPL emulators; see docs/originality-and-provenance.md).

---

## Post-1.0 directions (not committed)

These are candidate directions, not promises or a dated plan. They are ordered roughly by interest, not priority. The Status column is **as of v1.0.0**. Checked at v2.9.4: mobile (Android and iOS), Lua scripting, the TAS editor (TAStudio), Vs. DualSystem (desktop and the libretro core) and the CRT shader stack have all shipped; RetroAchievements allowlisting and hosted netplay infrastructure are still open; the current plan is [`to-dos/plans/v2.9.4-to-v3.0.0-line-plan.md`](to-dos/plans/v2.9.4-to-v3.0.0-line-plan.md).

| Area | Description | Status at v1.0.0 |
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
