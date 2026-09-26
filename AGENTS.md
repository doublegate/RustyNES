<!-- Managed by Master-Claude. Universal rules come from the imported/inlined core.
     Edit only inside the MC-PROJECT block; mc-sync overwrites everything else. -->
<!-- mc-core: 0.2.0 | mode=import | lang=rust -->
# AGENTS.md — RustyNES

@/home/parobek/.claude/master-core/AGENTS.base.md
@/home/parobek/.claude/master-core/lang/rust.md
@/home/parobek/.claude/master-core/modules/10-commits-and-versioning.md
@/home/parobek/.claude/master-core/modules/20-testing-and-accuracy.md
@/home/parobek/.claude/master-core/modules/30-quality-gates.md
@/home/parobek/.claude/master-core/modules/40-docs-and-adrs.md
@/home/parobek/.claude/master-core/modules/50-architecture-patterns.md
@/home/parobek/.claude/master-core/modules/60-security.md
@/home/parobek/.claude/master-core/modules/70-release-ceremony.md
@/home/parobek/.claude/master-core/modules/80-phase-sprint-workflow.md
@/home/parobek/.claude/master-core/modules/90-multi-language-integration.md
@/home/parobek/.claude/master-core/modules/91-agent-system-architecture.md
@/home/parobek/.claude/master-core/modules/95-named-pattern-library.md

<<< MC-PROJECT-START >>>

## Project: RustyNES

> **New here since v0.8.x?** The emulation core was replaced with the cycle-accurate engine and the repo was re-cut as v1.0.0. Read `docs/v1.0.0-synthesis-handoff-2026-06-13.md` first — it explains what changed, the `rustynes-*` architecture, where everything moved, and the hard constraints. Then update this file + your memory as you work.

## MOST IMPORTANT RULE — Provenance & license firewall (read first, applies to every task)

**This rule outranks everything else in this file.** RustyNES exists because of a real, corrected provenance failure (GPL emulator code was reproduced despite a black-box instruction, then the honest "ported from" comments were scrubbed; the project was relicensed to **GPL-3.0-or-later** and every derived site re-attributed). The full account is `docs/provenance-failure-postmortem.md`; the preventive ruleset is **`docs/ai-emulator-provenance-guardrails.md`** (PDFs of both in `ref-docs/`). **Read the guardrails doc and treat it as binding.** The non-negotiable core:

- **REFERENCE FIREWALL.** Reference emulators (Mesen2, puNES, FCEUX, Nestopia, higan, ares, GeraNES, TriCNES, tetanes, …) are **black-box oracles**. You may run them and read their *output* (framebuffers, traces, audio, logs). You **must not** open, read, quote, or reproduce their **source** (`.c`/`.cpp`/`.h`/`.cs`/`.rs`), constants, tables, variable names, code ordering, or comments — not "for reference," not once. **The local `ref-proj/` reference-emulator clone has been removed from disk and stays gitignored (`/ref-proj/`), so the source is out of reach by design. Do not re-clone it into the working tree.** If you find such source in reach, report that it should be removed; do not read it.
- **THE FIREWALL COVERS HDL TOO (ADR 0037, 2026-08-20).** The v2.4.1 → v2.5.0 "Fabric" line writes a **new** NES core in SystemVerilog. `NES_MiSTer`, `fpganes`, and any other NES `rtl/` are **strict black boxes** on exactly the same terms as emulator source: never opened, read, quoted or transcribed — not the RTL, not its constants, not its module or signal names. **Permitted:** instantiating a third-party core as an opaque testbench module and comparing its *outputs*. **Not permitted:** reading it. Keep those repositories physically outside the workspace, exactly as was done for `ref-proj/`. Anything genuinely unimplementable from documentation escalates to a **new ADR before any source is opened** — this is the rule most at risk of quiet erosion, because the pull toward reading a working core is strongest exactly when the DUT and RustyNES disagree at dot 260 of scanline 241 and nesdev is ambiguous.
- **IMPLEMENT FROM DOCS.** Write hardware behavior from public documentation (`nesdev_wiki/`, `ref-docs/`, datasheets, die studies) and pin it to public test ROMs / golden vectors. Hardware behavior is a fact; the specific *code expression* is copyrighted.
- **IF YOU DERIVE, SAY SO — AND STOP.** If you do port/adapt/closely-model an external source, (1) it is a derivative work under that source's license; (2) attribute it at the site + in `docs/originality-and-provenance.md` §1 + in `NOTICE` + via an SPDX header; (3) the project license must stay compatible (GPL-3.0-or-later) — flag it to the maintainer before proceeding.
- **NEVER LAUNDER.** Never reword or delete an honest "ported/derived from X" comment to make code look independent. Scrubbing provenance is the cardinal failure — worse than the original port. The response to "this says GPL code was incorporated" is relicense-and-attribute, never scrub-the-comment.
- **NO OVER-ATTRIBUTION.** Do not tag a genuine oracle *comparison* ("matches Mesen2's behavior," "cross-checked against ares") as "derived from." Attribute real ports; leave genuinely-independent code independent.
- **DO NOT SELF-CERTIFY.** Never assert "no third-party code is incorporated" / "license-clean" as a finished claim. Surface provenance status for human + expert review; state uncertainty. AI self-attestation of license compliance is not trustworthy — an outside NESdev reviewer, not the tooling, is what caught this.

- **THE FIREWALL IS PER-REGION, NOT PER-REPO — the oracle's own source has exceptions (2026-08-26).** "RustyNES's code is ours, so it is readable" is true of the repository and **not uniformly true of every block in it.** **26 files** (at v2.7.1; the count is itself a snapshot — regenerate it, never quote this number) carry a `// Provenance:` header disclosing that a REGION of them is derived from a GPL reference emulator, and those regions are **black boxes for HDL purposes even though the file is ours** — writing SystemVerilog from them launders the original expression into the DUT *through* us, which is precisely what ADR 0037 exists to prevent. This was found on the first task of v2.6.4: its headline job is fixing the five `SH`-group stores in the DUT, and `crates/rustynes-cpu/src/cpu.rs` line 3 discloses `SHA/SHX/SHY/SHS/TAS` as **derived from Mesen2's `SyaSxaAxa`** (`Core/NES/NesCpu.h`) — so the single most relevant block of oracle source for that version is one that must not be read to write the RTL. Nothing in the tooling said so. **Before reading oracle source to inform sibling/HDL work, `grep -n "Provenance:" <file>`**; regenerate the list with `grep -rln "^// Provenance:" crates` rather than trusting a snapshot. **The command used to read `crates/*/src/*.rs`, and that glob does not descend** — it silently missed `debugger/source_map.rs` and `bin/pgo_trainer.rs`, so it too reported a short list (24 against the real 26 at v2.7.1). `crates/rustynes-test-harness/tests/provenance_record_audit.rs` now walks the whole tree and fails if a header and its §1 row in `docs/originality-and-provenance.md` disagree in either direction. The ones that bite HDL work are `rustynes-cpu/src/cpu.rs` (SH group — rungs 1/5), **`rustynes-ppu/src/ppu.rs` (rungs 3/5 — the sprite-evaluation FSM and OAM-data-bus model from Mesen2, and the ALE/octal-latch address-multiplex and OAM-corruption behaviour from TriCNES)**, `rustynes-apu/src/blip.rs` (BLEP), and six mapper files (rung 7). **`ppu.rs` was missing from this sentence until 2026-09-19 and its absence was acted on**: the v2.6.22 CHR-during-rendering work read `ppu.rs` case 7 to form an RTL hypothesis, having checked the SENTENCE rather than running the command one line above it. The region read — the `$2007` PPUDATA write path — is outside all four disclosed regions, and the in-file note at `ppu.rs:777` re-states that scope, so nothing laundered; but the order was wrong, and it was an external reviewer that prompted the check rather than the process. **This sentence is a convenience, not the authority. The command is the authority, and the reason it exists is that a list of file names goes stale exactly the way the count above did — by ten files.** **The escalation ladder, maintainer-directed, in order — exhaust each rung before the next:** (1) vendored public documentation; (2) **the open Internet** — the vendored wiki is PARTIAL, documenting `SHX`/`SHY` in full and carrying nothing on `SHA`/`TAS`, which one web search supplied; (3) **black-box comparison** — a per-cycle golden diff needs no source at all and usually resolves faster, because at that point the question is "which cycle differs", not "what is the rule"; (4) the derived oracle source, **last resort**. Rung 4 is permitted — the licences are compatible, both repos being GPL-3.0-or-later — **but the existing attributions live in the ORACLE, and the sibling is a separate repository**, so reading it obliges declaring the derivation there too: a site comment, the sibling's provenance doc, `NOTICE`, and an ADR 0037 amendment naming who authorised it. Never silently. On v2.6.4 rungs 1-3 were sufficient and the escalation went unspent — worth knowing, because the pull toward rung 4 is strongest exactly when the DUT and the oracle disagree, which is the moment this rule matters.

Enforcement lives alongside the prose: `/ref-proj/` is gitignored/`.dockerignore`d/`.markdownlintignore`d and excluded from CodeRabbit; `deny.toml` gates dependency licenses; every derived file carries an SPDX + provenance header. A rule the tooling enforces beats a rule you are merely asked to follow.

## What this is

RustyNES is a cycle-accurate Nintendo Entertainment System emulator written in pure Rust. The accuracy bar is Mesen2 / higan / ares: tight lockstep scheduling at PPU-dot resolution on a master-clock-precise timebase, sub-instruction PPU events visible to subsequent CPU code, and a lookup-table non-linear audio mixer with band-limited synthesis. The frontend is pure Rust (`winit` + `wgpu` + `cpal` + `egui`).

**Current release: v2.8.4 "Tether"** (2026-09-26) — the MiSTer core's SDRAM build, made trustworthy: its controller now reads data on the edge the memory presents it (every off-die read would have been wrong on hardware, and only the new SDRAM timing constraints could see it), the power-up sequence and CAS-latency-3 reads follow the datasheet, the arbiter can no longer return the wrong byte or lose a write, the off-die bitstream builds from a script, both builds are swept and pinned at fitter seed 5, and the co-simulation ladder runs all 165 gates from a clean checkout. Built on **v2.8.3 "Rivet"** (2026-09-25) — the MiSTer core's reset, area and comments, measured: every reset is released on the clock that uses it and the timing analysis now checks each release, the CPU is about 4% smaller by two exact rewrites the fit report confirmed, four false comments are corrected, and the co-simulation ladder runs from a fresh checkout (164 of its 165 gates; the last needs a hand-built ROM no generator produces). v2.8.4 changes no emulation behaviour; it is the MiSTer core's off-die (SDRAM) rows, and with it the RTL audit ledger (`docs/audits/rtl-disposition.md`) records a disposition for every row of the v2.8.x line; three stay PARTIAL (R-3.1a and R-3.1b, revisited at v2.9.0; R-4.1, whose per-domain figures are approximate). v2.8.2 changed one emulation behaviour (MMC1 honours a reset written on the cycle after another write). The libretro audit ledger has no open row. The libretro core is built with `panic = "unwind"`, exercised by a C-ABI harness (`crates/rustynes-libretro/src/abi_tests.rs`), and patches a vendored `rust-libretro-sys` (`vendor/`). **AccuracyCoin 144/144 and nestest 0-diff** were measured on the v2.8.2 tree, the last that changed the emulator, and the full `--features test-roms` suite passes 2,756 tests. v2.7.4's mobile changes have a device checklist (`docs/mobile-v2.7.4-device-checklist.md`); this repository records no run of it yet. The co-simulation ladder is **165 gates green, 0 failed, 1 expected failure** on-die and **166 / 0 / 1** off-die (`USE_SDRAM=1`, which adds `sdram-latency`), each one uninterrupted run from a clean checkout of the v2.8.4 RTL with nothing skipped, and both of the sibling's bitstreams are cut at fitter seed 5 (on-die +0.414 ns setup / +0.078 ns hold, off-die +0.270 / +0.081; the SDRAM pin constraints are provisional until the SuperStation One's memory is read). **No hardware has run any bitstream**. The board session is v2.9.2 and the hardware-verified core is **v3.0.0** ([ADR 0041](docs/adr/0041-hardware-release-is-v3.0.0.md)); v2.7.x and v2.8.x act on the four audits in `docs/audits/` first. Per-release detail lives in `CHANGELOG.md` and the GitHub releases; it is deliberately not duplicated here.

- **Timebase (v2.0.0)** — the scheduler substrate is rewritten from a five-counter dot-lockstep model to a single canonical cycle counter, every CPU cycle clocked in two halves (`start_cycle` / `end_cycle`) with any bus access split between them, and the PPU caught up to each half (ADR 0002 / ADR 0029), now the *only* scheduler path. This is a MAJOR-boundary breaking change (ADR 0003): `.rns` save-state and `.rnm` movie format epochs bump (ADR 0028) — a pre-v2.0.0 `.rns` slot now fails to load with a clear error instead of silently misinterpreting stale bytes. Landed across five betas + rc.1 (PRs #217–223). Also new: core-level **Vs. `DualSystem`** dual-console support (`Emu::Dual`, `crates/rustynes-core`) for the four Vs. arcade cabinet boards — core-and-test-harness-only, frontend wiring deferred. The R1/R2 MMC3 IRQ-timing residual is by-design-deferred beyond this release with a mechanism-level finding recorded in ADR 0002 (not closed, not silently dropped). **AccuracyCoin now measures 141/141 (100.00%)**: the v2.0.1 upstream AccuracyCoin re-sync grew the catalog to 146 rows / 141 assigned tests and briefly opened two new PPU gaps ("ALE + Read" $0491, "Hybrid Addresses" $0492), which **v2.0.3** closed by promoting the 2-cycle-ALE PPU fetch model to the unconditional default (both experimental flags retired; additive `PPU_SNAPSHOT_VERSION` v5 tail). AccuracyCoin held 100% (139/139) throughout the v2.0.0 betas and final cut, dipped to 139/141 under the v2.0.1 re-sync, and is back to a full 141/141 from v2.0.3 onward.

- **Native Android app** — the **v1.8.0 → v1.8.9 "Android"** train (`crates/rustynes-mobile` UniFFI bridge + `crates/rustynes-android` JNI/NDK host + a Jetpack Compose app, ADR 0024): full on-device emulation, multi-touch + P1–P4 hardware controllers, wgpu `SurfaceView` rendering + the shared WGSL shader stack, save-states / battery SRAM, Lua, RetroAchievements, direct-IP + CGNAT/TURN room-code netplay, a box-art ROM library, and platform polish (adaptive / foldable / TV, Material You, capture / PiP / home-screen widget). Distributed as **GitHub-Releases sideload** now; Google Play deferred to v2.3.0 (ADR 0025 `foss`/`play` split).
- **Native iOS / iPadOS app** — the **v1.9.0 → v1.9.9 "iOS" TestFlight train** (`crates/rustynes-ios` Metal + CoreAudio shim reusing `rustynes-mobile` verbatim → UniFFI-generated Swift, ADR 0026): a native SwiftUI shell over wgpu→Metal, multi-touch + GameController, the shader stack, TAS / HD-pack / palettes / per-game DB, Lua + RetroAchievements, LAN + room-code netplay, CloudKit save-state sync, accessibility + EN/ES i18n + ReplayKit + Game Center, and the v1.9.9 creator tools (Cheats, a FOSS-gated read-only debugger, a touch TAStudio piano-roll, foreign movie import, a host audio-depth DSP). Ships to **interim TestFlight** now; App Store + AltStore PAL deferred to v2.3.0 (ADR 0027). Mobile ROM loading is iNES / NES 2.0 only (FDS / NSF a post-v2.0.0 carryover). Readiness record: `docs/ios-v1.9.9-readiness.md`.
- **Native Libretro core** — `crates/rustynes-libretro` (builds the `rustynes_libretro` shared library — `.so` / `.dylib` / `.dll` by platform, per the crate `Makefile`) integrates RustyNES into RetroArch (RetroAchievements, dynamic audio sync, deterministic save-state / rollback). Docs in `docs/libretro/`, plan in `to-dos/libretro/`, reference in `ref-docs/RustyNES-Libretro_Core.md`; the crate `Makefile` cross-compiles natively and `docs/libretro/UPSTREAM_SYNC.md` covers the re-fork / upstream-info-file (libretro-super + libretro-docs) workflow.
- **Mapper breadth → 174 families** (168 at the v1.7.x tag → 172 → 174 with v2.3.4's 176/2, 154 and 243), Core / Curated / BestEffort behind the CI accuracy-honesty gate.
- **Release automation** — `.github/workflows/release-auto.yml`: when a new version goes final-green on `main`, it auto-tags + publishes the GitHub Release (body from a maintainer-authored `.github/release-notes/vX.Y.Z.md` override, else the CHANGELOG `[X.Y.Z]` section; title codename parsed from the CHANGELOG header) and builds + attaches the desktop binaries by invoking `release.yml` via `workflow_call` (a tag pushed by `GITHUB_TOKEN` can't trigger `on: push: tags`, hence the direct call). The v1.8.0–v1.9.9 GitHub Releases are all published with comprehensive notes + Linux / macOS-aarch64 / Windows binaries.

**The one cross-version fact worth keeping here:** v2.0.0 "Timebase" is RustyNES's single designated breaking release (ADR 0003). It bumped the `.rns` save-state and `.rnm` movie format epochs (ADR 0028), so cross-version round-trip is a v1.x-only guarantee and a pre-v2.0.0 slot fails to load with a clear error rather than silently misreading stale bytes. Everything else about the release line is in `CHANGELOG.md`.

---

**Release history → `CHANGELOG.md`.** The full per-release detail — features, the mapper-count growth (51 → **172 families**), ADRs, and PR trains for **v1.0.0 → v2.0.0** (plus the documentary engine-lineage stages v0.9.0–v0.9.7) — lives in `CHANGELOG.md` (the single source of truth for user-visible change), the per-release GitHub Releases, and `to-dos/plans/`. Every release through v1.10.0 was **additive / off-by-default**, so with new features off those builds stayed byte-identical; **v2.0.0 is RustyNES's one designated breaking release** (ADR 0003) — the one-clock, every-cycle-bus-access scheduler (ADR 0002 / ADR 0029) is now the *only* path, and the old PPU-dot lockstep model is retired. **AccuracyCoin holds 100% (139/139)** on every release including v2.0.0. Workspace baseline: edition 2024, Rust **1.96**, license **GPL-3.0-or-later** (RustyNES is a derivative work of GPL emulators — Mesen2 GPLv3, puNES/FCEUX/Nestopia GPLv2-or-later; relicensed in v2.2.9 per ADR 0036, credited in `docs/originality-and-provenance.md` + `NOTICE`), author **DoubleGate**; the WebAssembly / GitHub Pages build is live at <https://doublegate.github.io/RustyNES/>.

**Engine-lineage versioning (read carefully).** The core descends from an accuracy program whose internal "v1.x / v2.x" milestones are folded into RustyNES stages v0.9.0–v0.9.7 → the v1.0.0 production cut. Read deep-narrative "v2.0" anchors from before 2026-07-03 (the master-clock refactor, old ADRs / audit logs under `docs/`) as **upstream engine lineage**, never as RustyNES release versions — that engine-lineage v2.0 work shipped as the v1.0.0 production core (2026-06-13) and is a *different* thing from RustyNES's own **v2.0.0 "Timebase"** release (2026-07-03, the base of the current v2.0.x "Harbor" line), which replaces that same dot-lockstep scheduler with the one-clock model. `docs/STATUS.md` is the authoritative per-suite pass-count + mapper matrix.

## Build / test / lint

```bash
# Build
cargo check --workspace
cargo build --workspace
cargo build --release --workspace

# Tests
cargo test --workspace                              # unit + integration
cargo test --workspace --features test-roms         # + AccuracyCoin / blargg / kevtris ROM suites
cargo test --workspace --features test-roms,commercial-roms  # + 60-ROM commercial oracle (needs local dumps)
cargo test -p rustynes-cpu                          # single crate
cargo test -p rustynes-cpu nestest                  # single test by name substring
cargo test --workspace -- --test-threads=1          # serial (for flake debugging)

# Run only the #[ignore]'d expected-fail probes
cargo test --workspace --features test-roms --no-fail-fast -- --ignored

# Quality gates (all run in CI; all must be green)
cargo fmt --all --check
cargo clippy --workspace --all-targets -- -D warnings
cargo clippy -p rustynes-frontend --all-targets --features scripting -- -D warnings  # v1.1.0 Lua engine
cargo clippy -p rustynes-frontend --all-targets --features scripting,hd-pack -- -D warnings  # v1.2.0 HD-pack
cargo clippy -p rustynes-frontend --all-targets --features retroachievements -- -D warnings  # RA FFI — DON'T skip this one
# After any `cargo clippy --fix`, re-run clippy for EVERY feature combo (incl. retroachievements):
# --fix compiles only the active feature set, so it can strip cfg-gated code that another feature
# needs (bit PR #80: an `elidable_lifetime_names` autofix removed a `<'a>` the `retroachievements`
# `ra` param uses, breaking that build). Wasm clippy commands are in the WebAssembly section below.
RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps
# NEVER `--all-features`: v1.2.0's `script-wasm` (piccolo/wasm) and `scripting` (mlua/native) are
# mutually-exclusive rustynes-script backends, so `--all-features` can't resolve. CI (and the
# pre-commit hook) use EXPLICIT features. Also: a rustdoc intra-doc link to a feature-only dep
# (e.g. [`piccolo`] / [`mlua`]) FAILS the default `cargo doc --workspace --no-deps` — use plain
# `code` spans for mutually-exclusive-feature crate names (bit PR #76).

# no_std cross-compile (the chip stack must compile against core + alloc only)
cargo build -p rustynes-core --target thumbv7em-none-eabihf --no-default-features

# Frontend (winit + wgpu + cpal + egui). Binary name is `rustynes`.
cargo run --release -p rustynes-frontend -- path/to/rom.nes
cargo run --release -p rustynes-frontend                 # opens with no ROM; menu / F12 to load
# Maximal NATIVE build — the "cargo --full equivalent" (#54): the `full` feature
# aggregates every native feature (retroachievements + scripting + script-ipc +
# hd-pack + debug-hooks + av-record, additive on top of the default set). It is
# purely opt-in (shipped/default build + core unchanged). Aliases in .cargo/config.toml:
cargo full-run path/to/rom.nes                           # run the maximal native binary (alias ends in `--`, so flags forward, e.g. `cargo full-run --fullscreen rom.nes`)
cargo full-build                                         # build it (= --release -p rustynes-frontend --features full)
# WASM-only features (script-wasm, browser-cheevos, wasm-canvas) are excluded by design.
# Default keys P1: arrows = D-pad, Z = A, X = B, Enter = Start, RShift = Select.
# Default keys P2: WASD = D-pad, Q = A, E = B, P = Start, R = Select (L before v2.7.3).
# Famicom microphone: hold N (M before v2.7.3). M toggles the menu bar.
# System: Esc = quit, F1 = save state, F4 = load state, F5 (held) = rewind,
# F2 = reset, F3 = power-cycle, F12 = open ROM, F9 = FDS disk-swap, ~ = toggle debugger.
# F6/F7/F8 = TAS movie record/play/branch. Drag-and-drop a .nes/.fds to load.
# USB gamepads auto-bind to P1 (Xbox-style: South=A, West=B, Start, Back/Select, DPad).

# WebAssembly frontend — needs `trunk` + the wasm32 target (auto-installed from
# rust-toolchain.toml). Run from crates/rustynes-frontend/web:
#   trunk serve                                          # dev server
#   trunk build --release                                # wasm-winit (default)
#   trunk build --release --no-default-features --features wasm-canvas  # lightweight embed
# wasm clippy gates (CI): cargo clippy -p rustynes-frontend --target wasm32-unknown-unknown
#   --lib --bins -- -D warnings   (and again with --no-default-features --features wasm-canvas)
# GOTCHA: web/Trunk.toml pins the wasm-bindgen CLI version, which MUST exactly match the
#   wasm-bindgen LIBRARY in Cargo.lock (grep -A1 'name = "wasm-bindgen"' Cargo.lock). A
#   mismatch fails `trunk build` (and the Pages deploy) at the wasm-bindgen step, but wasm
#   clippy still passes — so bump the pin whenever a resolve moves the library version.
# CI deploy: .github/workflows/web.yml ("Deploy Pages (demo + docs)") publishes BOTH the
#   playable demo (root) and the workspace rustdoc (/api/) to GitHub Pages from the
#   "GitHub Actions" source: https://doublegate.github.io/RustyNES/ + /api/.

# Benchmarks (criterion)
cargo bench -p rustynes-cpu
cargo bench -p rustynes-ppu
cargo bench -p rustynes-mappers
cargo bench -p rustynes-core
```

Toolchain is **Rust 1.96** pinned in `rust-toolchain.toml` (bumped from 1.86 in v1.3.0 to unblock the edition-2024 + egui 0.34 / wgpu 29 / rfd 0.17 dependency tier). CI runs the test job on stable across Linux/macOS/Windows plus an MSRV pin at 1.96 on Linux.

On Linux, anything that pulls in `rustynes-frontend` (which `cargo test --workspace` does) needs the wgpu/winit/cpal system deps:

```bash
sudo apt-get install -y libxkbcommon-dev libwayland-dev libxkbcommon-x11-dev libasound2-dev libudev-dev
# CachyOS / Arch:
sudo pacman -S --needed libxkbcommon wayland alsa-lib systemd-libs
```

## Architecture — load-bearing facts

These cross-cutting decisions span multiple files. Reading individual chip docs without them in mind will mislead.

**One master clock, and every CPU cycle is clocked in two halves.** Since v2.0.0 "Timebase" (ADR 0002 / ADR 0029) the scheduler keeps a single canonical master-clock counter. The CPU steps an instruction at a time (`Cpu::step`), and each of its cycles runs through `start_cycle` (catch the PPU up with `run_ppu_to`, run the bus's per-cycle work) and `end_cycle` (catch the PPU up to the cycle's end, tick the DMC). A cycle with a bus access (`read1` / `write1`) performs it between the two halves; an internal cycle (`idle_tick`) runs both halves with no access at all, so "every cycle is a bus access" is true of the timing, not of the traffic. The PPU runs 3 dots per CPU cycle on NTSC / Dendy and 3.2 on PAL (region dividers 12 : 4 and 16 : 5). Because the catch-up runs to every access, mid-instruction PPU events (sprite-zero hit at a precise dot, the MMC3 IRQ at dot 260, mid-scanline scroll writes) are visible to the next access without per-quirk patches. The pre-v2.0.0 dot-lockstep loop (`tick_one_dot()`) is retired. See `docs/scheduler.md`.

**The Bus owns everything mutable.** `rustynes_core`'s `LockstepBus` (the name predates v2.0.0; there is no type called `Bus` in `rustynes-core`) holds the PPU, APU, mapper-via-cart, WRAM, controllers, and open-bus latch. `Nes` holds the `Cpu` and the bus, and the CPU borrows the bus for each instruction: `self.cpu.step(&mut self.bus)`, generic over the `rustynes_cpu::Bus` trait. Per the TetaNES postmortem, this single choice avoids the borrow-checker fight that the alternative ("CPU holds PPU, but PPU also needs CPU bus") creates. The PPU sees a smaller trait, `rustynes_ppu::PpuBus`, for mapper-mediated CHR / nametable access. The APU sees no bus at all: the bus drives the DMC sample fetch through `Apu::dmc_dma_pending` / `dmc_dma_addr` / `complete_dmc_dma` inside the CPU's unified DMA. (`ApuBus` survives only as a deprecated, unimplemented trait since v2.7.5; this paragraph and the one above described the pre-v2.0.0 design until then — core audit §4.2.)

**Workspace dependency graph is one-directional.** `rustynes-cpu` has no PPU or APU dep. `rustynes-ppu` depends on `rustynes-mappers` only (CHR/nametable bus). `rustynes-apu` is independent. `rustynes-core` ties them together. Result: each chip is fuzzable and benchmarkable in isolation. Adding a cross-chip dependency breaks this invariant — don't.

**Mapper IRQ logic lives in the mapper, not the PPU.** The PPU calls `Mapper::notify_a12(level)` on every A12 transition; the mapper internally filters (MMC3's "3 falling edges of M2" lives in the MMC3 impl). MMC5 uses different scanline detection via `notify_scanline_start` / `notify_vblank`. VRC2/4/6, Sunsoft FME-7, and Namco 163 tick on `notify_cpu_cycle()`. All such hooks are default-no-op on `Mapper`. See `docs/mappers.md` for the per-mapper IRQ family table.

**Determinism is a hard contract.** Same seed + ROM + input sequence ⇒ bit-identical framebuffer and audio. CPU/PPU initial phase alignment is randomized at power-on from a seeded PRNG; reset preserves alignment. This is required for save-state round-trip, regression tests, TAS replay, and netplay rollback. Don't introduce hidden non-determinism (system time, thread scheduling, OS RNG) into the core. Netplay's dynamic rate control and run-ahead live in the **frontend** (a resampler stage / snapshot-restore orchestration), never in the core's synthesis — that is what keeps the contract intact.

**The frontend is an always-on egui shell, not a bare window.** `rustynes-frontend` is winit + wgpu + cpal + egui, and egui runs **every frame**: `DebuggerOverlay::render_shell` draws a persistent menu bar (File / Emulation / Tools / View / Debug / Help) + status bar + tabbed Settings window, with the toggleable (`` ` ``) CPU/PPU/APU/memory debugger panels layered on top. The shell never holds the emu lock inside the egui closure — menu interactions return a `MenuAction` that `App::dispatch_menu_action` runs *after* the egui pass, and the hidden render branch copies the framebuffer under a brief lock, drops it, and renders/presents with `nes = None` (the locked branch is taken only when the overlay is visible or a `nes`-reading tool panel like Cheats is open). On native the emulator runs on a dedicated thread (`emu-thread`, default-on) communicating via the `Arc<Mutex<EmuCore>>` handle + lock-free `SharedInput`; the winit thread only does UI + present. Full spec in `docs/frontend.md` (this is just the primer).

**Test ROMs are the spec.** When the docs and a passing test ROM disagree, the ROM wins — the docs get updated. The blargg / kevtris / mmc3_test_2 / AccuracyCoin suites in `tests/roms/` are the closed-form definition of "cycle-accurate." See `docs/testing-strategy.md` for the testing layers.

## Where things live

- `crates/rustynes-{cpu,ppu,apu,mappers,core,netplay,cheevos,frontend,test-harness}/` — the core emulation stack; crate name = dir name. The binary is `rustynes` (in `rustynes-frontend`). Plus the supporting crates: `rustynes-script` (Lua), `rustynes-ra` (RetroAchievements session state), `rustynes-gfx-shaders` (shared WGSL), `rustynes-hdpack` (HD-pack loader/compositor + HD audio), and the **platform crates** `rustynes-mobile` (the UniFFI bridge — generates Kotlin *and* Swift), `rustynes-android` (JNI/NDK host), `rustynes-ios` (Metal + CoreAudio shim; only the `#[cfg(target_os="ios")]` glue is iOS-specific), and `rustynes-libretro` (the RetroArch core; builds the platform-appropriate `rustynes_libretro` shared library — `.so` / `.dylib` / `.dll`). The `android/` and `ios/` dirs hold the Compose / SwiftUI apps.
- `docs/` — implementation specs. These are the **spec**, not history: update them in the same PR as the code change. Per-subsystem files (`cpu-6502.md`, `ppu-2c02.md`, `apu-2a03.md`, `mappers.md`, `cartridge-format.md`, `scheduler.md`) + cross-cutting (`architecture.md`, `testing-strategy.md`, `performance.md`, `frontend.md`, `compatibility.md`). `docs/STATUS.md` is the **single source of truth** for per-suite pass counts, the mapper matrix, and version policy. `docs/adr/` holds Michael-Nygard-format ADRs.
- `ref-docs/` — immutable hardware + emulation reference (60+ source research report). Updates go in dated supplemental files.
- `to-dos/ROADMAP.md` → phase/sprint files — tickets with stable IDs `T-PS-NNN`. Reference in commits.
- `tests/roms/` — CC0 / public-domain test ROMs (committed). `tests/roms/external/` — your own commercial dumps (gitignored).
- `tests/golden/` / `screenshots/` — reference framebuffers, audio, and the visual baseline corpus (committed). `tests/captures/` — current-run output (gitignored).

**Never commit commercial Nintendo ROMs.**

## Workflow conventions

- Branch names: `<type>/<short-desc>`.
- A chip-behavior change touches both the chip code and the chip's `docs/<subsystem>.md`. They drift apart easily; don't let them.
- For accuracy work: pin the failing test ROM expectation first, then implement until it passes.
- Hot paths (`Cpu::tick`, `Ppu::tick`, mapper register access): no allocations, prefer fixed arrays, profile (`cargo bench` + `perf record`) before adding abstractions. **On the frame-cost number:** the `≤ 2 ms/frame headless` figure in `docs/performance.md` is a **design-phase aspiration** (written before the cycle-accurate core existed, for 2018-era Skylake) — it is NOT a live gate. The implemented core measures **~3.95 ms** (`nes_run_frame_nestest_fast`) / **~2.65 ms** (`nes_run_frame_flowing_palette_fast`) on the shipped fast dot path, and ~4.46 / ~2.67 ms on the exact path (i9-10850K, 2026-09-23), which `docs/performance.md` records as knowingly accepted for the master-clock design. The stock `full_frame` benches select the exact path explicitly; from v2.2.3 to 2026-09-23 they silently measured the fast path, which `docs/performance.md` §"Current figures" corrects. That is ~23% of the 16.639 ms NTSC budget. The dominant costs are work the accuracy model *requires* — `cpu_clock` is APU BLEP synthesis + the non-linear mixer (mixer ceiling measured ≤1.9%), and `Ppu::tick` is the per-dot lockstep loop — and the obvious levers were already measured and **rejected** (`emit_pixel` bounds-check elision was *slower*; the SIMD blitter was *slower*). Do not "optimize toward 2 ms" by trading away accuracy; the real-world multiplier on frame cost is **run-ahead**, not the per-frame core cost. Any optimization must be **byte-identical**, and must pass the adoption rule in `scripts/perf/ab_check.sh`: **the bar is evidence quality, not effect size** (maintainer decision, v2.3.1) — a consistent, reproduced, statistically clean gain is adoptable even below 3%. What is not negotiable is the second independent run (a single run has already produced a p = 0.00 result on all four workloads that was pure artifact), and a mixed-sign result across workloads is a rejection, not an average. Read the A/B/A order-bias control before the candidate column. (Until v2.7.5 this line said ">3% same-runner A/B bar", which contradicted the script; older records in `docs/performance.md` cite the bar that was in force when they were written.)
- `unsafe` requires a `// SAFETY:` comment explaining the invariant. The chip stack is `#![no_std]` + `extern crate alloc;`; only `rustynes-frontend` and `rustynes-cheevos` (FFI) carry `unsafe`.
- **Comprehensive rustdoc + comments (project rule).** Craft extensive `//!` crate/module preambles and `///` / `//` inline comments matching the quantity, quality, and technical depth of the existing `rustynes-*` crates — explain the *why* alongside the architectural detail, the memory-safety guarantees, and the lockstep-timing considerations.
- **Comprehensive commit bodies (project rule).** Commit message bodies are robust, comprehensive, and technically detailed: go beyond a summary to explain architectural impact, the mathematical implementation, memory constraints, and the deep technical specifics (the maintainer's house style; see `docs/guidelines`).
- Code style: rustfmt defaults + the crate-level import grouping in `rustfmt.toml`; `.editorconfig` mandates UTF-8 / LF / a final newline and indentation of four spaces for Rust, two for Markdown / TOML / YAML. Justify any local `#[allow]`.

## Operating notes for Claude Code

### The detail lives in `docs/agents/`, indexed here

This section used to carry **110 bullets / 115,606 bytes** inline — every
measured finding this project has made, loaded into every session whether or
not the task touched any of it. It is now **nine topic files**, verbatim, read
on demand. The pointers and the release anchors stay below, because they are
what a session needs before it knows what it is doing.

| read when the task touches | file | bullets |
|---|---|---|
| GitHub Actions, workflows, the release ceremony, lint coverage | [`docs/agents/ci-and-release.md`](docs/agents/ci-and-release.md) | 20 |
| a PR review — the ceremony, where findings hide, which claims recur | [`docs/agents/review-bots.md`](docs/agents/review-bots.md) | 11 |
| the libretro core, the buildbot, the upstream `.info`, RetroArch | [`docs/agents/libretro.md`](docs/agents/libretro.md) | 17 |
| the MiSTer sibling — the RTL, Quartus, the rungs, the bitstream | [`docs/agents/mister-cosim.md`](docs/agents/mister-cosim.md) | 32 |
| accuracy work — AccuracyCoin, blargg, sub-tests, goldens, the PPU | [`docs/agents/accuracy-oracle.md`](docs/agents/accuracy-oracle.md) | 23 |
| reading a result — what it does and does not prove | [`docs/agents/measurement-discipline.md`](docs/agents/measurement-discipline.md) | 18 |
| the shell, `pre-commit`, `gh`, `/tmp`, long-running jobs | [`docs/agents/tooling-traps.md`](docs/agents/tooling-traps.md) | 13 |
| a dependency bump, or why one is blocked | [`docs/agents/dependencies.md`](docs/agents/dependencies.md) | 2 |
| a performance claim, or a debugger panel that outlives its `Nes` | [`docs/agents/perf-and-panels.md`](docs/agents/perf-and-panels.md) | 6 |

**Read the file, not a summary of it.** Each bullet carries its own evidence —
the command that was run, the number it returned, the mutation that caught it —
because this project's standing rule is that a recorded conclusion nobody
re-derives is a hypothesis that stopped being tested. A paraphrase loses
exactly the part that makes it checkable.

**Adding a finding:** put it in the topic file, not here. If it fits no file,
that is a reason to add a tenth — not a reason to grow this section back.

- `docs/STATUS.md` and the "Current release" summary above are the current-state source of truth; `CHANGELOG.md` and the `docs/audit/` logs carry the deep engine-lineage history.
- `ref-docs/` is immutable. Research updates go in dated supplemental files.
- ADRs go in `docs/adr/` (Michael Nygard format).
- `rustynes-core` re-exports the public types from the chip crates; downstream consumers (`rustynes-frontend`, `rustynes-test-harness`) should depend on `rustynes-core` rather than the chip crates directly.
- When relabeling old engine "v2.x" narrative for users, present it as upstream lineage/history — **never as a current RustyNES release version.** The current release is **v2.8.4 "Tether"** (2026-09-26). **Never claim any version *later* than v2.8.4 is released.** Two distinct "v2.0"s exist and must not be conflated: the **engine-lineage v2.0** master-clock work shipped as the **v1.0.0** production core (2026-06-13) and was the only scheduler through v1.10.0; RustyNES's own **v2.0.0 "Timebase"** (2026-07-03) is a different milestone that REPLACES that dot-lockstep scheduler with the one-clock, every-cycle-bus-access model (ADR 0002 / 0028 / 0029) and is the one release that broke byte-identity and save-state compatibility, by design. The per-release narrative that used to be inlined here is in `CHANGELOG.md`, the per-release notes under `.github/release-notes/`, and the published GitHub releases — three places that are maintained, against one copy here that was not.
- **Forward plans + roadmap live in `to-dos/`.** `to-dos/ROADMAP.md` (updated in #129) is the planning entry point and frames the release line + "the path to v2.0.0 and beyond"; `to-dos/plans/` holds the per-release plan docs (through `v1.7.0-forge-plan.md` on `main`, plus the staged-forward `v1.8.0-android-plan.md` / `v1.9.0-ios-plan.md` / `v2.0.0-master-clock-plan.md`) + the `to-dos/plans/engine-lineage/` history archive + a `to-dos/plans/research/` reference-mining archive.
- The v1.0.0 release + GitHub Pages/CI + post-release record is in `docs/v1.0.0-synthesis-handoff-2026-06-13.md` — read it before touching CI, Pages, or release tooling. Full per-release history is in `CHANGELOG.md`.
- **Markdownlint is a CI gate** (pre-commit, pinned `markdownlint-cli v0.49.1`). The pin was v0.39.0 until the v2.6.3 dependency refresh, held because the newer local binary reported rules the pin lacked — chiefly **MD060** (`table-column-style`), which was therefore NOT gated. That is now measured and resolved: MD060's inferred default reads this corpus as style `compact` and reports **1,936 findings across 122 files** and nothing else, so `.markdownlint.json` pins `MD060` to the style actually in use (`leading_and_trailing`), which measures **zero** and rewrites no document. It IS a gate now. Still verify with `pre-commit run markdownlint --all-files` rather than the bare binary — the pin and the local build can drift apart again. `.markdownlint.json` also keeps `MD013`/`MD033`/`MD041` disabled by design (long technical tables, the README HTML banner/`<img>`, the HTML-led README). `.markdownlintignore` exempts `ref-docs/`, `ref-proj/` (the reference-emulator clone, now removed from disk but kept in the ignore lists as a firewall guard so it can never re-enter the tree — see the MOST IMPORTANT RULE section above), the vendored `tricnes/` + upstream READMEs, and the frozen `docs/archive/` + `to-dos/archive/` trees — don't lint or reformat those.
- **RetroAchievements client identity:** the RA HTTP User-Agent (how RA authenticates/identifies/allowlists the client) is `RustyNES/<crate version> rcheevos/<rcheevos version>` — the `RA_USER_AGENT` const in `crates/rustynes-cheevos/src/http.rs`; the rcheevos version auto-syncs from the vendored `rc_version.h` via `build.rs` (`RCHEEVOS_VERSION`). Keep the leading `RustyNES/` token (a regression test guards it).
- **Exhaustive Documentation Sweeps:** When tasked with generating comprehensive project documentation or wikis, always recursively list and read the contents of `docs/`, `ref-docs/`, and `to-dos/` to ensure no deep technical knowledge is missed.
- **GitHub Wiki Initialization:** When assisting with GitHub Wiki deployments for the first time, instruct the user to click "Create the first page" in the GitHub UI to provision the `.wiki.git` repository. If the Wiki is cloned locally inside the main repository, ensure its folder (e.g., `RustyNES.wiki/`) is added to `.gitignore`.
- **Symlinked Agent Configs:** Ensure symlinked agent files (like `GEMINI.md` -> `AGENTS.md`) are explicitly removed from `.gitignore` so they are correctly tracked by version control.

<<< MC-PROJECT-END >>>
