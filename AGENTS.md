<!-- Managed by Master-Claude. Universal rules come from the imported/inlined core.
     Edit only inside the MC-PROJECT block; mc-sync overwrites everything else. -->
<!-- mc-core: 0.2.0 | mode=import | lang=rust -->
# AGENTS.md — RustyNES

@~/.claude/master-core/AGENTS.base.md
@~/.claude/master-core/lang/rust.md
@~/.claude/master-core/modules/10-commits-and-versioning.md
@~/.claude/master-core/modules/20-testing-and-accuracy.md
@~/.claude/master-core/modules/30-quality-gates.md
@~/.claude/master-core/modules/40-docs-and-adrs.md
@~/.claude/master-core/modules/50-architecture-patterns.md
@~/.claude/master-core/modules/60-security.md
@~/.claude/master-core/modules/70-release-ceremony.md
@~/.claude/master-core/modules/80-phase-sprint-workflow.md
@~/.claude/master-core/modules/90-multi-language-integration.md
@~/.claude/master-core/modules/91-agent-system-architecture.md
@~/.claude/master-core/modules/95-named-pattern-library.md

<<< MC-PROJECT-START >>>

## Project: RustyNES

## MOST IMPORTANT RULE — Provenance & license firewall (read first, applies to every task)

**This rule outranks everything else in this file.** RustyNES exists because of a real, corrected provenance failure (GPL emulator code was reproduced despite a black-box instruction, then the honest "ported from" comments were scrubbed; the project was relicensed to **GPL-3.0-or-later** and every derived site re-attributed). The full account is `docs/provenance-failure-postmortem.md`; the preventive ruleset is **`docs/ai-emulator-provenance-guardrails.md`** (PDFs of both in `ref-docs/`). **Read the guardrails doc and treat it as binding.** The non-negotiable core:

- **REFERENCE FIREWALL.** Reference emulators (Mesen2, puNES, FCEUX, Nestopia, higan, ares, GeraNES, TriCNES, tetanes, …) are **black-box oracles**. You may run them and read their *output* (framebuffers, traces, audio, logs). You **must not** open, read, quote, or reproduce their **source** (`.c`/`.cpp`/`.h`/`.cs`/`.rs`), constants, tables, variable names, code ordering, or comments — not "for reference," not once. **The local `ref-proj/` reference-emulator clone has been removed from disk and stays gitignored (`/ref-proj/`), so the source is out of reach by design. Do not re-clone it into the working tree.** If you find such source in reach, report that it should be removed; do not read it.
- **TriCNES exception and the HDL firewall (ADR 0037).** TriCNES (MIT) is the one named exception, consulted only from OUTSIDE the repository after rungs 1-3 of the escalation ladder; what follows is an attributed port. HDL (`NES_MiSTer`, `fpganes`, any NES `rtl/`) is a strict black box: never opened, read, quoted or transcribed. Terms: `docs/agents/provenance-firewall.md`; read it before either.
- **IMPLEMENT FROM DOCS.** Write hardware behavior from public documentation (`nesdev_wiki/`, `ref-docs/`, datasheets, die studies) and pin it to public test ROMs / golden vectors. Hardware behavior is a fact; the specific *code expression* is copyrighted.
- **IF YOU DERIVE, SAY SO — AND STOP.** If you do port/adapt/closely-model an external source, (1) it is a derivative work under that source's license; (2) attribute it at the site + in `docs/originality-and-provenance.md` §1 + in `NOTICE` + via an SPDX header; (3) the project license must stay compatible (GPL-3.0-or-later) — flag it to the maintainer before proceeding.
- **NEVER LAUNDER.** Never reword or delete an honest "ported/derived from X" comment to make code look independent. Scrubbing provenance is the cardinal failure — worse than the original port. The response to "this says GPL code was incorporated" is relicense-and-attribute, never scrub-the-comment.
- **NO OVER-ATTRIBUTION.** Do not tag a genuine oracle *comparison* ("matches Mesen2's behavior," "cross-checked against ares") as "derived from." Attribute real ports; leave genuinely-independent code independent.
- **DO NOT SELF-CERTIFY.** Never assert "no third-party code is incorporated" / "license-clean" as a finished claim. Surface provenance status for human + expert review; state uncertainty. AI self-attestation of license compliance is not trustworthy — an outside NESdev reviewer, not the tooling, is what caught this.

- **THE FIREWALL IS PER-REGION, NOT PER-REPO.** A `// Provenance:` header marks GPL-derived regions that are black boxes for HDL / sibling work even though the file is ours; list them with `grep -rln "^// Provenance:" crates` (the command is the authority). Read the escalation ladder in `docs/agents/provenance-firewall.md` before reading oracle source for other work.

Enforcement lives alongside the prose: `/ref-proj/` is gitignored/`.dockerignore`d/`.markdownlintignore`d and excluded from CodeRabbit; `deny.toml` gates dependency licenses; every derived file carries an SPDX + provenance header. A rule the tooling enforces beats a rule you are merely asked to follow.

## What this is

RustyNES is a cycle-accurate Nintendo Entertainment System emulator written in pure Rust. The accuracy bar is Mesen2 / higan / ares: tight lockstep scheduling at PPU-dot resolution on a master-clock-precise timebase, sub-instruction PPU events visible to subsequent CPU code, and a lookup-table non-linear audio mixer with band-limited synthesis. The frontend is pure Rust (`winit` + `wgpu` + `cpal` + `egui`).

**Current release: v3.7.0 "Spindle"** (2026-11-10) — the Famicom Disk System on the MiSTer core, spectators who join a match through the hosted relay, opt-in desync forensics, live ROM edits recorded as BPS patches, and the mobile extras. Built on **v3.6.2 "Latchkey"** (2026-11-06) — AccuracyCoin 147 of 147 and a per-game *Rad Racer* correction (epoch 10). **No hardware has run any MiSTer bitstream** (ADR 0043). The full release-line chain back to v3.0.0 is in `docs/agents/release-line.md`; suite counts, the mapper matrix and per-release detail live in `docs/STATUS.md`, `CHANGELOG.md` and the GitHub releases.

Platform builds (Android sideload, iOS TestFlight, the libretro core), the 208-family mapper breadth and release automation: `docs/agents/release-line.md`.

**Format epochs.** The `.rns` / `.rnm` / netplay format breaks (v2.0.0, v2.9.8, v3.0.0) are recorded in `docs/agents/release-line.md`. The rule that applies to every change:

- **Movies and netplay carry `EMULATION_EPOCH`** (ADR 0045): `.rnm` format 5, netplay protocol 6 (`"RNE6"`). A different epoch, or an older RustyNES peer, is refused with a reason. **The epoch must rise in the same change as anything that alters a frame, a sample or a bus cycle against the last release** (a re-blessed golden or moved snapshot is the trigger; `docs/agents/ci-and-release.md`).

---

Workspace baseline: edition 2024, Rust **1.100**, license **GPL-3.0-or-later** (ADR 0036), author **DoubleGate**. Release history is in `CHANGELOG.md`; the engine-lineage versioning note (why pre-2026-07-03 "v2.0" anchors are not RustyNES releases) is in `docs/agents/release-line.md`.

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
# --fix compiles only the active feature set (why: docs/agents/build-and-frontend.md).
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
# Maximal native build (`full` feature): cargo full-run / full-build / full-native / full-v3 /
# full-baseline (aliases in .cargo/config.toml). Default keys, the WebAssembly (trunk) builds,
# wasm clippy gates and the wasm-bindgen pin GOTCHA: docs/agents/build-and-frontend.md.

# Benchmarks (criterion)
cargo bench -p rustynes-cpu
cargo bench -p rustynes-ppu
cargo bench -p rustynes-mappers
cargo bench -p rustynes-core
```

Toolchain is **Rust 1.100** pinned in `rust-toolchain.toml`. **The libretro buildbot follows the same pin**: `.gitlab-ci.yml`'s `RUSTUP_TOOLCHAIN` must equal its `channel` (CI's `libretro-cross` job fails otherwise), so move both in one change; history in `docs/agents/build-and-frontend.md`.

On Linux, anything that pulls in `rustynes-frontend` (which `cargo test --workspace` does) needs the wgpu/winit/cpal system deps:

```bash
sudo apt-get install -y libxkbcommon-dev libwayland-dev libxkbcommon-x11-dev libasound2-dev libudev-dev
# CachyOS / Arch:
sudo pacman -S --needed libxkbcommon wayland alsa-lib systemd-libs
```

## Architecture — load-bearing facts

These cross-cutting decisions span multiple files. Reading individual chip docs without them in mind will mislead.

**One master clock; the Bus owns everything mutable.** One master-clock counter (ADR 0002 / 0029); each CPU cycle is two halves that catch the PPU up to every access. `SystemBus` owns PPU, APU, mapper, WRAM and controllers; the CPU borrows it per instruction. Full primer: `docs/agents/architecture-primer.md`; spec: `docs/scheduler.md`.

**Workspace dependency graph is one-directional.** `rustynes-cpu` has no PPU or APU dep. `rustynes-ppu` depends on no other workspace crate (its only dependencies are the external `bitflags`, `thiserror` and `libm`): it reaches CHR and nametables through its own `PpuBus` trait, which the core implements over the mapper. `rustynes-apu` is independent. `rustynes-core` ties them together. Result: each chip is fuzzable and benchmarkable in isolation. Adding a cross-chip dependency breaks this invariant — don't.

**Mapper IRQ logic lives in the mapper, not the PPU**; every mapper hook is default-no-op on `Mapper` (primer: `docs/agents/architecture-primer.md`; table: `docs/mappers.md`).

**Determinism is a hard contract.** Same seed + ROM + input sequence ⇒ bit-identical framebuffer and audio. CPU/PPU initial phase alignment is randomized at power-on from a seeded PRNG; reset preserves alignment. This is required for save-state round-trip, regression tests, TAS replay, and netplay rollback. Don't introduce hidden non-determinism (system time, thread scheduling, OS RNG) into the core. Netplay's dynamic rate control and run-ahead live in the **frontend** (a resampler stage / snapshot-restore orchestration), never in the core's synthesis — that is what keeps the contract intact.

**The frontend is an always-on egui shell**; never hold the emu lock inside the egui closure (primer: `docs/agents/architecture-primer.md`; spec: `docs/frontend.md`).

**Test ROMs are the spec.** When the docs and a passing test ROM disagree, the ROM wins — the docs get updated. The blargg / kevtris / mmc3_test_2 / AccuracyCoin suites in `tests/roms/` are the closed-form definition of "cycle-accurate." See `docs/testing-strategy.md` for the testing layers.

## Where things live

- `crates/rustynes-*/` — crate name = dir name; the binary is `rustynes` (in `rustynes-frontend`); `rustynes-cosim` is excluded from the workspace. Full crate inventory: `docs/agents/architecture-primer.md`. The `android/` and `ios/` dirs hold the Compose / SwiftUI apps.
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
- Hot paths (`Cpu::tick`, `Ppu::tick`, mapper register access): no allocations, profile before abstracting. Any optimization must be byte-identical and pass `scripts/perf/ab_check.sh` with a second independent run; the `≤ 2 ms/frame` figure is an aspiration, not a gate (`docs/agents/conventions.md`).
- `unsafe` requires a `// SAFETY:` comment explaining the invariant. The chip stack is `#![no_std]` + `extern crate alloc;`; only `rustynes-frontend` and `rustynes-cheevos` (FFI) carry `unsafe`.
- **Comprehensive rustdoc + comments (project rule).** Craft extensive `//!` crate/module preambles and `///` / `//` inline comments matching the quantity, quality, and technical depth of the existing `rustynes-*` crates — explain the *why* alongside the architectural detail, the memory-safety guarantees, and the lockstep-timing considerations.
- **Comprehensive commit bodies (project rule).** Commit message bodies are robust, comprehensive, and technically detailed: go beyond a summary to explain architectural impact, the mathematical implementation, memory constraints, and the deep technical specifics (the maintainer's house style).
- Code style: rustfmt defaults + the crate-level import grouping in `rustfmt.toml`; `.editorconfig` mandates UTF-8 / LF / a final newline and indentation of four spaces for Rust, two for Markdown / TOML / YAML. Justify any local `#[allow]`.

## Operating notes for Claude Code

### The detail lives in `docs/agents/`, indexed here

Measured findings live in topic files, verbatim, read on demand:

| read when the task touches | file | bullets |
|---|---|---|
| GitHub Actions, workflows, the release ceremony, lint coverage | [`docs/agents/ci-and-release.md`](docs/agents/ci-and-release.md) | 26 |
| a PR review — the ceremony, where findings hide, which claims recur | [`docs/agents/review-bots.md`](docs/agents/review-bots.md) | 15 |
| the libretro core, the buildbot, the upstream `.info`, RetroArch | [`docs/agents/libretro.md`](docs/agents/libretro.md) | 19 |
| the MiSTer sibling — the RTL, Quartus, the rungs, the bitstream | [`docs/agents/mister-cosim.md`](docs/agents/mister-cosim.md) | 38 |
| accuracy work — AccuracyCoin, blargg, sub-tests, goldens, the PPU | [`docs/agents/accuracy-oracle.md`](docs/agents/accuracy-oracle.md) | 24 |
| reading a result — what it does and does not prove | [`docs/agents/measurement-discipline.md`](docs/agents/measurement-discipline.md) | 22 |
| the shell, `pre-commit`, `gh`, `/tmp`, long-running jobs | [`docs/agents/tooling-traps.md`](docs/agents/tooling-traps.md) | 30 |
| a dependency bump, or why one is blocked | [`docs/agents/dependencies.md`](docs/agents/dependencies.md) | 5 |
| a performance claim, or a debugger panel that outlives its `Nes` | [`docs/agents/perf-and-panels.md`](docs/agents/perf-and-panels.md) | 9 |

Moved from this file on 2026-11-14 (paragraphs, verbatim):

- `docs/agents/release-line.md` — release chain, platform apps, format epochs, lineage
- `docs/agents/provenance-firewall.md` — TriCNES exception, HDL firewall, escalation ladder
- `docs/agents/architecture-primer.md` — scheduler, bus, mapper IRQs, egui shell, crate list
- `docs/agents/build-and-frontend.md` — `clippy --fix` trap, toolchain, aliases, keys, wasm
- `docs/agents/conventions.md` — frame cost, markdownlint, RA User-Agent, plans

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
- Engine-lineage "v2.x" narrative is upstream history, never a current RustyNES release version. The current release is **v3.7.0 "Spindle"** (2026-11-10). **Never claim any version *later* than v3.7.0 is released.** The two distinct "v2.0"s are explained in `docs/agents/release-line.md`.
- `to-dos/ROADMAP.md` is the planning entry point (detail: `docs/agents/conventions.md`).
- The v1.0.0 release + GitHub Pages/CI + post-release record is in `docs/v1.0.0-synthesis-handoff-2026-06-13.md` — read it before touching CI, Pages, or release tooling. Full per-release history is in `CHANGELOG.md`.
- **Markdownlint is a CI gate** (pinned `markdownlint-cli v0.50.0`): verify with `pre-commit run markdownlint --all-files`, and never lint or reformat the `.markdownlintignore` trees. Keep the RetroAchievements User-Agent's leading `RustyNES/` token (`RA_USER_AGENT`, regression-tested). Detail: `docs/agents/conventions.md`.
- **Symlinked Agent Configs:** Ensure symlinked agent files (like `GEMINI.md` -> `AGENTS.md`) are explicitly removed from `.gitignore` so they are correctly tracked by version control.

<<< MC-PROJECT-END >>>
