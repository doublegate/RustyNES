# Testing strategy

**References:** `ref-docs/research-report.md` §Source manifest (test ROMs);
`ref-docs/nesdev-wiki-technical-report.md` §Test Strategy; Nesdev
[Emulator tests](https://www.nesdev.org/wiki/Emulator_tests) and
[Tricky-to-emulate games](https://www.nesdev.org/wiki/Tricky-to-emulate_games).

## Purpose

Define the layered testing approach that takes the project from "compiles" to "passes the cycle-accuracy bar set by Mesen2 and ares". Tests are the primary specification — when written-down rules conflict, the test ROM behavior wins.

## Layer 1 — unit tests (per crate)

Each crate has its own `#[test]` coverage in `src/` and `tests/`:

- `rustynes-cpu`: every opcode × every addressing mode (~600 cases). Flag-update tables for ADC, SBC, CMP, BIT. Property tests via `proptest` for arithmetic flag correctness.
- `rustynes-ppu`: register read/write semantics; OAMADDR rules; loopy `t/v/x/w` updates per the documented rules; sprite evaluation FSM (with the buggy `n+m` increment).
- `rustynes-apu`: register layout; frame counter sequence per mode; length counter halt timing; envelope/sweep arithmetic; mixer table values (compared against the closed-form formula within 0.1%).
- `rustynes-mappers`: per-mapper bank-resolution tables; MMC1 serial protocol; MMC3 IRQ counter (deterministic toggle test); bus conflict cases.

Aim: > 90% line coverage on the chip crates.

## Layer 2 — golden-log compare (CPU only)

`nestest.nes` is the canonical 6502 golden master. The test harness:

1. Forces PC to `$C000` (nestest's automated test entry point).
2. After each instruction, captures `(PC, A, X, Y, P, SP, CYC, PPU dot, scanline)`.
3. Diffs against the bundled `nestest.log` (Nintendulator-generated).
4. First mismatch → test fails, prints the diff.

## Layer 3 — test ROM corpus (subsystem coverage)

The full blargg + kevtris + community test ROM suite, vendored in `tests/roms/` (all are CC0 / public-domain individual ROMs from `christopherpow/nes-test-roms`). Each ROM is run by `rustynes-test-harness::run_until_complete()`, which:

1. Steps until `$6000` reads `$80→$00..$7F` (test complete) or `$81` (needs reset) — automated reset on `$81`.
2. Reads the result code at `$6000` and the message string at `$6004` onward.
3. Compares result code against expected (usually 0); fails the test with the message on mismatch.

| Category | ROMs | Pass target |
|----------|------|-------------|
| CPU instruction | `instr_test_v5/*` (16 sub-ROMs) | All |
| CPU reset/power | `cpu_reset` (2) | **Closed (Phase 7 T-71-002):** power-on register dump asserted strictly; full interactive reset protocol `#[ignore]`'d (not headlessly driveable); semantics covered by `Cpu::power_on`/`Nes::reset` unit tests |
| CPU timing | `cpu_timing_test6`, `instr_timing` (2) | **`instr_timing` closed (Phase 7 T-71-003):** both sub-ROMs strict-pass on the full `Nes` |
| CPU branches | `branch_timing_tests/*` (3) | All |
| CPU interrupts | `cpu_interrupts_v2/*` (5) | All except the 3 C1-axis residuals (`#[ignore]`'d, deferred to v2.0) |
| CPU dummy reads/writes | `cpu_dummy_reads`, `cpu_dummy_writes_*`, `instr_misc` (5) | **`instr_misc` closed (Phase 7 T-71-003):** all 5 strict-pass on the full `Nes` (incl. `04-dummy_reads_apu`) |
| PPU VBL/NMI | `ppu_vbl_nmi/*` (10) | All |
| PPU open bus | `ppu_open_bus` | Pass |
| PPU sprite | `sprite_overflow_tests/*` (5), `sprite_hit_tests_2005.10.05/*` (10), `ppu_sprite_hit/*`, `oam_read`, `oam_stress` | All |
| APU | `apu_test/*` (8), `apu_mixer/*` (4) | All |
| DMC DMA | `dmc_dma_during_read4/*` (4) | All |
| MMC3 | `mmc3_test_2/*` (5), `mmc3_irq_tests/*` (6) | All |
| Mapper coverage | `holy_mapperel`, `holy_diver_battery_test`; `vrc24test` → in-tree VRC2/4 unit tests + `m22` baseline (T-71-005) | Pass for implemented mappers |
| Input | standard-controller strobe/read tests (T-71-004); DMC-conflict / Four Score / Zapper documented in `compatibility.md` | Standard-pad path strict; expansion devices deferred |
| Accuracy battery | `AccuracyCoin` (single ROM) | Pass-rate target ≥ 90% by v1.0 |

## Layer 4 — golden framebuffer / audio comparison

For a curated corpus of freely-distributable demos (NESDev compo entries, homebrew releases under permissive licenses):

- Run for 600 frames with deterministic reset and zero controller input.
- Capture frame 60, 180, 300, 600.
- Compare pixel-exact against a stored reference (generated initially by hand-validation against Mesen2).
- For audio: compute PSNR against a stored reference WAV. Fail if PSNR drops below threshold.

This catches drift caused by accidental changes that the unit and ROM tests miss (e.g., subtle audio mixer drift).

## Layer 4.5 — commercial-ROM regression-prevention oracle (since 2026-05-17)

The May-2026 SMB / Excitebike / Kid Icarus regression motivated a
dedicated commercial-ROM oracle. Distinct from layer 4 because it
covers the *commercial* library (gitignored under
`tests/roms/external/`), not freely-distributable demos.

- **Harness:** `crates/rustynes-test-harness/tests/external_real_games.rs`
  (60 tests across 15 mappers). Feature-gated on `commercial-roms`
  (default off; CI never depends on non-distributable assets).
- **Per-test contract:** ROM SHA-256 + framebuffer FNV-1a 64-bit hash
  at one or more checkpoints + cumulative CPU cycle count + audio
  FNV-1a hash + audio sample count, all asserted against a committed
  `insta` snapshot (~500 B / file).
- **Visual companion:** `screenshots/` corpus (81 PNGs at 256×240
  RGBA8). Regenerated via `RUSTYNES_DUMP_FRAMES=1
  RUSTYNES_DUMP_DIR=$PWD/screenshots cargo test ...`.
- **Auto-bisect:** `scripts/regression-bisect/` is a permanent
  turn-key wrapper around `git bisect run`. Single-ROM or per-mapper
  filter (`HARNESS_FILTER=external_mmc3_ ./run.sh`). Drove the
  May-2026 recovery in 5 iterations (`0b1d4b66..HEAD` →
  `63d8dea` first-bad).
- **Trade-off:** snapshots commit emulator output (deterministic
  bytes), not ROM bytes (copyrighted). User-supplied dumps under
  `tests/roms/external/mapper-NNN-NAME/` are gitignored and entirely
  the user's responsibility.

## Layer 5 — fuzz testing

`cargo-fuzz` harnesses for:

- **Cartridge parser**: arbitrary `&[u8]` → `parse()`. Must not panic. Errors typed.
- **CPU step**: arbitrary RAM contents + arbitrary opcode sequence → `cpu.step_instruction()`. Must not panic; must respect read/write counts.
- **Mapper writes**: arbitrary write sequences to mapper registers. Must not panic; bank indices stay in range.

## Layer 6 — CI gating

GitHub Actions workflow (`.github/workflows/ci.yml`):

- Lint: `cargo fmt --check`, `cargo clippy -- -D warnings`.
- Build: stable + MSRV (1.75) on Linux, macOS, Windows.
- Unit tests: `cargo test --workspace`.
- Test ROM suite: `cargo test --workspace --features test-roms` (gated to avoid pulling 30+ MB of ROMs in default builds).
- Doc build: `cargo doc --workspace --no-deps`.
- Optional: nightly run with overflow checks enabled in release mode (catches regressions the fast path hides).

## Test ROM licensing

All vendored ROMs are individually CC0 or public-domain per their authors' dedications. Redistribution rights are documented in `tests/roms/LICENSES.md`. **No commercial Nintendo ROMs are bundled.** A separate `tests/roms/external/` directory is `.gitignore`'d for users who own ROMs they want to test against locally.

## Nesdev Completeness Audit

When adding or re-baselining tests, use the Nesdev emulator-test index as the
coverage checklist rather than the current repository contents. Gap status
(Phase 7 / v1.5.0):

- `instr_misc` (5) and `instr_timing` (2) — **CLOSED (T-71-003):** vendored
  (blargg PD) and strict-passing on the full `Nes`.
- `cpu_reset` (2) — **CLOSED (T-71-002):** wired; power-on register dump
  asserted strictly. The full interactive reset protocol is `#[ignore]`'d
  because the headless harness cannot supply an externally-timed reset; the
  reset register/RAM semantics are guarded by `Cpu::power_on` / `Nes::reset`
  unit tests.
- `vrc24test` — **REPLACED (T-71-005):** the original forum attachment link is
  permanently rotted (auth-walled; no mirror). Replaced by in-tree VRC2/VRC4
  register/wiring unit tests in `crates/rustynes-mappers` plus the `m22` baseline
  harness.
- Input-device coverage — **standard pad CLOSED (T-71-004):** strobe/serial-read
  bit-order tests on both ports. Four Score, Zapper, Famicom expansion devices,
  microphone, and DMC-DMA controller-bit corruption are a documented decision
  in `docs/compatibility.md` (Sprint 4 T-74-005); they remain deferred unless
  permissive fixtures and user demand surface.
- PAL/Dendy validation needs dedicated timing ROMs and golden snapshots rather
  than NTSC-derived expectations.
- FDS, Vs. System, PlayChoice-10, and non-stock PPU palettes are out of v1.0
  but should be represented as explicit unsupported-platform tests or fixture
  metadata when support begins.

## Open questions

- **Snapshot tests for chip state.** `insta` would let us snapshot CPU register state mid-run. Useful for debugging but adds CI noise. Defer until needed.
- **TAS (tool-assisted) movie playback.** Replaying recorded controller inputs over many minutes is the highest-leverage compatibility test. Defer to Phase 5 frontend tooling.
- **Public dashboard of pass rates.** Auto-generate a markdown table of pass/fail for every test ROM and post to the README on each release. Good marketing; defer to v0.5.
