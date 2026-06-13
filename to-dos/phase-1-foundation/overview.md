# Phase 1 — Foundation

## Goal

Stand up the Cargo workspace with CI on green, implement the iNES + NES 2.0 cartridge parser, and bring the CPU core to nestest-passing parity. By the end of this phase the repository compiles, lints clean, and a `cargo test` invocation runs the nestest golden-log comparison successfully — even though no picture renders and no sound plays yet.

## Exit criteria

- [x] `cargo build --workspace` succeeds on stable Rust 1.86 across Linux, macOS, Windows. (MSRV bumped from 1.75 → 1.86 during bootstrap; see project `CLAUDE.md` for rationale.)
- [x] `cargo clippy --workspace --all-targets -- -D warnings` clean.
- [x] `cargo fmt --all --check` clean.
- [x] `cargo test --workspace` green.
- [x] `nestest.nes` golden-log comparison passes (PC=$C000 mode). 8991 instructions compared, zero diff.
- [ ] `instr_test_v5/*` (16 sub-ROMs) all pass. *Deferred: ROMs are mapper 1 (MMC1); blocked on Sprint 5 (Phase 2 mapper work). ROMs vendored at `tests/roms/blargg/instr_test_v5/`.*
- [x] iNES + NES 2.0 parser handles the bundled `nes-test-roms` NROM corpus subset without errors. (Full-corpus parse including MMC1-and-beyond ROMs lands as more mappers come online; the parser surface is generic.)

## Scope

In-scope:
- Cargo workspace skeleton with all 7 crates (empty placeholders for chips not yet implemented).
- CI pipeline (`.github/workflows/ci.yml`).
- Cartridge file format parsing (no mapper logic yet — just data extraction).
- 6502 / 2A03 CPU core: all 151 official opcodes + all 105 unofficial opcodes, exact cycle counts.
- CPU interrupt logic (NMI edge detection, IRQ level sampling, BRK, hijacking).
- A trivial `Bus` impl for the test harness (RAM-backed, no real PPU/APU/mapper).
- Test harness for nestest golden-log comparison.

Out-of-scope (deferred to later phases):
- Real PPU/APU integration (stubs only).
- Mapper implementations beyond NROM (which is "no banking" — trivial).
- Frontend.
- Save states.

## Sprints

- [Sprint 1 — Workspace + CI + lints](sprint-1-workspace.md) — repo skeleton, CI green
- [Sprint 2 — Cartridge parser (iNES + NES 2.0)](sprint-2-cartridge.md) — parse the file format
- [Sprint 3 — CPU core: official opcodes](sprint-3-cpu-official.md) — 151 documented opcodes, all addressing modes, cycle-accurate
- [Sprint 4 — CPU core: unofficial opcodes + nestest](sprint-4-cpu-unofficial.md) — 105 illegal opcodes; nestest golden log passing

## Dependencies

None. This is the first phase.

## Risks

- **Risk: subtle CPU cycle-count bugs that nestest doesn't catch.** Detection: add `cpu_timing_test6` to CI in Phase 2; mitigation: aggressive unit tests for every opcode × addressing mode.
- **Risk: NES 2.0 vs iNES 1.0 confusion in parser.** Detection: round-trip parse the entire test ROM corpus. Mitigation: clearly typed `RomFormat` enum and dedicated test cases.
- **Risk: clippy warnings from `pedantic` lints producing too much noise.** Detection: developer friction. Mitigation: `[workspace.lints]` allow-lists for noisy lints (see `docs/build-and-tooling.md`).
- **Risk: MSRV creep.** Detection: CI builds against MSRV explicitly. Mitigation: pin to 1.86 (bumped from 1.75 during bootstrap to satisfy `edition2024` transitive deps in the frontend stack — `icu_*`, `idna_adapter`); bump only on documented dep requirements.

## Reference docs

- [docs/architecture.md](../../docs/architecture.md) — workspace shape and module boundaries
- [docs/cpu-6502.md](../../docs/cpu-6502.md) — CPU specification
- [docs/cartridge-format.md](../../docs/cartridge-format.md) — iNES + NES 2.0 layout
- [docs/build-and-tooling.md](../../docs/build-and-tooling.md) — toolchain, lints, profiles
- [docs/testing-strategy.md](../../docs/testing-strategy.md) — testing layers and CI gating
