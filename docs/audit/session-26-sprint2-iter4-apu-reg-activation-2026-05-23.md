# Session 26 — Sprint 2 iter 4: APU Register Activation Test 4

**Date:** 2026-05-23
**Branch:** `main` (HEAD `9ab5c48` at session start)
**Scope:** Sprint 2 iteration 4 of v1.0.0-final: flip
`APU Tests :: APU Register Activation` (`AccuracyCoin` result address
`$045C`) from error 4 (Fail Test 4) to a pass (`$01`) or pass-with-code
(`$09` — the Mesen2 result), by closing the OAM-DMA-source-page-$40
register-activation axis.

**Predecessors:**
- `docs/audit/session-25-sprint2-iter3-frame-counter-irq-2026-05-23.md`
  — Sprint 2 iter 3 LANDED template (`irq_flag_clear_cycle` lazy schedule).
- `docs/audit/session-24-phase4-implied-dummy-dmc-2026-05-23.md` —
  identifies APU Register Activation Test 4 as a clean-oracle alternate
  to Implied Dummy.
- `docs/audit/session-24-phase3-controller-strobing-2026-05-23.md` —
  Phase 3 LANDED template (M2-low-defer for `$4016` strobe commit).
- `docs/audit/session-23-accuracycoin-source-audit-2026-05-22.md` —
  per-test tractability.

## Phase 1A — Custom-ROM validation

`tests/roms/AccuracyCoin/sub-tests/apu-reg-activation.nes` (Session-23
infrastructure + Session-24 Phase 3 controller-drain + Phase 4
dep-pre-seed) was re-validated under `validate_sub_test_rom`:

| Emulator | `$045C` result | Decoded |
|---|---|---|
| RustyNES (HEAD `9ab5c48`) | `$12 = (4<<2)\|2` | **Fail at Test 4** |
| Mesen2 | `$09 = (2<<2)\|1` | **PassWithCode(2)** (passes Test 4, fails some later sub-test) |

The ROM is clean as an oracle — both emulators reach
`TEST_APURegActivation` and write to `$045C` in 31 frames.

## Phase 1B — Mesen2 + RustyNES trace tooling

### New artifacts

| File | Description |
|---|---|
| `scripts/mesen2_apu_reg_activation_trace.lua` | Per-`$4014`/`$4015`/`$4016`/`$4017` access Lua oracle. Captures cycle, frame, scanline, dot, parity-derived M2 phase, access type, value. |
| `crates/nes-test-harness/src/bin/trace_apu_reg_activation.rs` | RustyNES counterpart binary. Uses the `irq-timing-trace` feature to record per-CPU-cycle bus access, then emits a CSV filtered to `$4014`/`$4015`/`$4016`/`$4017` rows + result-addr write + ErrorCode writes. Build/run requires `--features irq-timing-trace`; no-feature build emits a stub `main` that prints a usage hint. |
| `crates/nes-test-harness/golden/irq_trace/apu-reg-activation.csv` | RustyNES trace (committed). |
| `crates/nes-test-harness/golden/irq_trace/mesen2/apu-reg-activation.csv` | Mesen2 trace (committed). |

## Phase 1C — Cross-diff: Test 4 OAM DMA from page $40

The clean diagnostic is the `STA $4014` with `A = $40` (OAM DMA from
page $40 — `TEST_APURegActivation` line 8104). Test 4's pass condition:
the subsequent `LDA $4015 / AND #$40` must observe bit 6 SET, meaning
the OAM DMA did NOT clear the frame-counter IRQ flag.

| Phase | Mesen2 (cycle, m2, access/addr/value) | RustyNES (cycle, m2, access/addr/value) |
|---|---|---|
| `STA $4014, A=$40` | 623286 L W $4014 = $40 | 921096 L W $4014 = $40 |
| OAM DMA read of $4015 | (not visible at API level; internal bus silent) | 921140 L **r** $4015 = $40 — **triggers `apu.read_status()` -> clears IRQ flag** |
| OAM DMA read of $4016 | (not visible) | 921142 L **r** $4016 = $41 — **clocks controller-1 shift** |
| OAM DMA read of $4017 | (not visible) | 921144 L **r** $4017 = $41 — **clocks controller-2 shift** |
| `LDA $4015` post-DMA | 623804 L R $4015 = $40 (bit 6 **STILL SET** — PASS) | 921613 H R $4015 = $00 (bit 6 **CLEARED** — FAIL) |

Both emulators see the same `STA $4014, A=$40` write, but Mesen2's
OAM DMA leaves the frame-counter IRQ flag intact while RustyNES's OAM
DMA destroys it.

The lowercase 'r' in RustyNES rows marks `BusAccess::DmaRead` — the
per-cycle trace captures the actual register-read side effects of the
OAM DMA's `raw_cpu_read` call.

## Phase 2 — Hypothesis

### Architectural background (nesdev `APU page` + `DMA page`)

The 2A03 has three internal address buses:
- 6502 address bus (driven by the CPU core).
- OAM DMA address bus (driven by the OAM DMA engine; `dma_page << 8 + index`).
- DMC DMA address bus (driven by the DMC channel).

Only one of these is connected to the external 2A03 address bus on
any given cycle. The APU registers (`$4000-$401F`) are physically
located inside the 2A03 chip, but their CHIP SELECT signal is gated
by `6502_addr ∈ $4000-$401F`. The DMA address buses do NOT drive the
APU CHIP SELECT — only the 6502 address bus does.

Consequently: if the 6502 address bus is OUTSIDE `$4000-$401F` when
an OAM DMA reads from a source address in `$4000-$40FF`, the OAM DMA
sees the EXTERNAL DATA BUS — i.e., open bus (the floating-bus latch).
Crucially, no APU/controller register side-effects fire — the chip
select is inactive.

During OAM DMA, the 6502 is halted, so its address bus is parked at
the last-driven address (`self.dma_halt_addr`). In Test 4, that
halted_addr is outside `$4000-$401F` (the test code lives in `$8000+`
PRG), so the APU registers are INACTIVE during the DMA.

### Mesen2 reference (cross-reference)

Mesen2's OAM DMA implementation in `Core/NES/CpuTypes.h` +
`Core/NES/NesConsole.cpp` reads from the source page via the same
bus dispatcher as the 6502, BUT the APU/controller register read
methods internally check whether the 6502 address bus is in the
register-active range. The chip-select gate is correctly modelled
at the register level, not at the DMA-dispatcher level.

The `dmc_dma_read` helper in our codebase already implements an
equivalent check (`(halted_addr & 0xFFE0) != 0x4000`) — it just
wasn't applied to OAM DMA.

### RustyNES current model

`crates/nes-core/src/bus.rs` `clock_oam_dma_cycle` (line 1123-1151)
calls `self.raw_cpu_read(src_addr)` unconditionally. When
`src_addr ∈ $4000-$401F`, this hits the `$4015 / $4016 / $4017`
read paths in `raw_cpu_read` (line 1361-1422), which:
- `$4015`: calls `apu.read_status()` (clears the frame-counter IRQ
  flag).
- `$4016` / `$4017`: shifts the controller's internal shift register.

These side-effects fire regardless of `dma_halt_addr`. This is the
Test 4 axis.

### Single-axis hypothesis

**The OAM DMA's `raw_cpu_read` should honour the APU chip-select gate:
when `(dma_halt_addr & 0xFFE0) != 0x4000` AND `(src_addr & 0xFFE0) == 0x4000`,
the read should return `self.open_bus` (the floating-bus latch) without
triggering any register side-effects.**

Specifically:
- Add a `raw_oam_dma_read(src_addr) -> u8` helper that wraps
  `raw_cpu_read`.
- When the halted_addr is outside `$4000-$401F` AND `src_addr` is
  inside that range, return the open-bus latch.
- Otherwise (`src_addr` outside `$4000-$401F`, or halted_addr inside
  `$4000-$401F`), proceed through `raw_cpu_read` as before. (The
  Test 5 conflict-path semantics for the halted_addr-inside case
  are NOT being added here — Test 5 is a separate axis with its own
  PPU-bus-databus / "$8D / $14 / $40" prep dance that depends on a
  fully-correct OAM DMA-with-6502-bus-inside-$4000-$401F model. This
  iteration ONLY closes Test 4.)
- The change is structural enough that it doesn't fit cleanly behind
  a feature flag (per Phase 3 controller-defer precedent). Lands as
  the new canonical behavior, gated by the full regression test suite.

**Falsifiable prediction**: `APU Register Activation` `$045C` flips
from `$12` (Fail Test 4) to `$09` (PassWithCode(2) — matches Mesen2)
or higher (one or more subsequent sub-tests may also flip).

**Cascade risk**:
- `apu_test` 8 sub-tests: NONE-LOW. None of the blargg apu_test
  ROMs exercise OAM DMA from page $40.
- `apu_mixer` 4 sub-tests: NONE.
- `dmc_dma_during_read4` 5 sub-tests: NONE — DMC DMA uses its own
  `dmc_dma_read` path; the OAM DMA helper is separate.
- `cpu_dummy_writes_oam` 1 strict: LOW — exercises `$2003`, adjacent
  to `$4014` but the OAM DMA source pages are inside PRG ROM, not
  $40.
- Commercial-ROM oracle (60 ROMs): LOW — games strobe OAM DMA from
  page `$02` typically (the standard sprite scratch buffer), never
  from page `$40`. The fix only changes behavior when DMA source is
  in `$4000-$40FF` AND the halted_addr is outside `$4000-$401F`.
- Controller Strobing (Phase 3 landing): NONE (separate $4016 write
  surface).
- Frame Counter IRQ (Session-25 iter 3 landing): LOW-NONE. The
  iter-3 landing is the deferred IRQ-flag clear schedule; this
  iteration touches OAM DMA bus dispatch only. Re-validate via the
  custom-ROM `frame-counter-irq.nes`.
- B4 MMC3 invariants: NONE (mapper IRQ; distinct surface).

## Phase 2 outcome

Hypothesis fully formed. Implementation proceeds to Phase 3.

## Phase 3 — Implementation

### Phase 3.1 — Surface

`crates/nes-core/src/bus.rs`:
- New `raw_oam_dma_read(src_addr) -> u8` helper. Mirrors the existing
  `dmc_dma_read` chip-select gate: when
  `(self.dma_halt_addr & 0xFFE0) != 0x4000 && (src_addr & 0xFFE0) == 0x4000`,
  returns `self.open_bus` without triggering any register side-effects
  (no `apu.read_status()` clear, no controller shift, no open-bus
  latch update). Otherwise delegates to `raw_cpu_read(src_addr)`.
- `clock_oam_dma_cycle` now calls `self.raw_oam_dma_read(src_addr)`
  instead of `self.raw_cpu_read(src_addr)`. Production line-count
  delta: +44 lines (one new helper + doc-comment), -1 line (call-site
  change).

No save-state version bump required — the change is pure dispatch
and adds no persisted state. The Test 5 conflict-path semantics
(where `dma_halt_addr` IS in `$4000-$401F` because the test JSRs
to `$3FFE`) are NOT being modelled here; that's a separate axis
that would require a full halted_addr-conflict address-mapping
similar to `dmc_dma_read`'s `conflict_addr` path.

### Phase 3.2 — Validation gauntlet results

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets --features test-roms -- -D warnings` | PASS |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `cargo build --workspace` | PASS |
| `cargo test --workspace --features test-roms` | **545 strict pass + 5 ignored** (preserved from Session-25 baseline) |
| `cargo test --test apu_test --features test-roms` | **8/8** PASS (apu_test load-bearing surface preserved) |
| `cargo test --test apu_mixer --features test-roms` | **4/4** PASS |
| `cargo test --test dmc_dma --features test-roms` | **5/5** PASS |
| `cargo test --test mmc3 --features test-roms` | **12 strict + 2 ignored** PASS (B4 invariant preserved) |
| Controller Strobing (Session-24 Phase 3 landing) | `$01` PASS preserved |
| Frame Counter IRQ (Session-25 iter 3 landing) | `$4E` Fail Test J preserved (residual, same shape) |
| Custom ROM `apu-reg-activation.nes` | `$045C = $1A` **Fail Test 6 (was `$12` Fail Test 4 — 2 sub-tests advanced)** |
| AccuracyCoin RAM-direct | 83.45% (unchanged headline; internal Test 4 → Test 6 advancement = 2 sub-tests now pass that previously didn't run; the APU Register Activation catalog entry is one "test" so the per-suite count stays at 6 pass / 3 fail. Same pattern as Session-25 iter 3 landed-with-unchanged-headline.) |
| AccuracyCoin framebuffer | 89.83% (unchanged) |
| Commercial-ROM oracle (60 ROMs, `--features test-roms,commercial-roms`) | **60/60** PASS |
| 4 MMC3 commercial canary ROMs | **all PASS** — `external_mmc3_mega_man_3`, `external_mmc3_tmnt3`, `external_mmc3_ninja_gaiden_2`, `external_mmc3_tiny_toon_adventures_2` |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) visual legibility | preserved (subset of commercial oracle) |

### Phase 3.3 — Post-fix trace verification

The golden RustyNES CSV at
`crates/nes-test-harness/golden/irq_trace/apu-reg-activation.csv`
was regenerated AFTER the fix landed. The Test 4 OAM DMA sub-region
now matches Mesen2's behavior:

| Region | Mesen2 (committed) | RustyNES post-fix (committed) |
|---|---|---|
| `STA $4014, A=$40` | 623286 L W $4014 = $40 | 921096 L W $4014 = $40 |
| OAM DMA read of $4015 | (internal — bus silent) | 921140 L r $4015 = `$40` (open bus; **no read_status() fired**) ✓ |
| OAM DMA read of $4016 | (internal) | 921142 L r $4016 = `$40` (open bus; **no controller shift**) ✓ |
| OAM DMA read of $4017 | (internal) | 921144 L r $4017 = `$40` (open bus) ✓ |
| `LDA $4015` post-DMA | 623804 L R $4015 = `$40` (bit 6 SET — PASS) | 921613 H R $4015 = `$40` (**bit 6 SET — PASS**) ✓ |

The Test 4 axis closure is empirically confirmed: RustyNES now sees
the IRQ flag still set after the OAM DMA from page `$40`, matching
Mesen2's behavior. The test then progresses to Test 5 (PASS for both
emulators), and finally fails at Test 6 (the wacky JSR `$3FFE` +
BRK trick's OAM-content verification, which depends on the Test 5
conflict-path semantics that this iteration explicitly does NOT
model). The advancement from `Fail Test 4` to `Fail Test 6` is the
architecturally-clean closure of the Test 4 axis.

### Phase 3.4 — Outcome decision

**LANDED.** The primary architectural target (Test 4 OAM-DMA-source-page
APU-chip-select gate) is closed. No regressions. The APU Register
Activation catalog entry advances from `[error 4]` to `[error 6]` (2
sub-tests further internally, but the per-test catalog metric remains
1 fail in the suite, so the 83.45% RAM-direct headline is unchanged).

The Test 6 residual (and the broader Test-5-conflict-path semantics)
is a separate axis — closing it requires modelling the
halted_addr-IN-$4000-$401F path (i.e., when the 6502 bus is parked
inside the APU register window during OAM DMA, the OAM DMA reads
must follow the `conflict_addr = 0x4000 | (src_addr & 0x001F)`
mirror-pattern that `dmc_dma_read` already implements). That's a
future-sprint target.

The architectural fix is the foundation that any subsequent Test 6/7
work will build on.

## Final references

- `crates/nes-core/src/bus.rs:1141` (call-site change in
  `clock_oam_dma_cycle`).
- `crates/nes-core/src/bus.rs:1153-1195` (new `raw_oam_dma_read`
  helper with the chip-select gate doc-comment).
- `crates/nes-core/src/bus.rs:1329-1356` (existing `dmc_dma_read`
  helper that this fix mirrors).
- Mesen2 `Core/NES/NesApu.cpp` `ReadRam` / `Core/NES/NesConsole.cpp`
  (the chip-select model — APU registers gate on the 6502 address
  bus only).
- AccuracyCoin `AccuracyCoin.asm` lines 8091-8109 (TEST_APURegActivation
  Test 4 comment + assertion path) + lines 8111-8211 (Test 5 wacky
  JSR `$3FFE` setup) + lines 8213-8281 (Test 6 OAM-content
  verification).
- `docs/audit/session-25-sprint2-iter3-frame-counter-irq-2026-05-23.md`
  (the iteration-3 LANDED template — audit-doc structure used here).
- `docs/audit/session-24-phase3-controller-strobing-2026-05-23.md`
  (Phase 3 LANDED template — first "structural change, no feature
  flag" precedent in Sprint 2).

## References

- `100thCoin/AccuracyCoin@main` `AccuracyCoin.asm` lines 8000-8211
  (TEST_APURegActivation — Tests 1-7; Test 4 commentary on
  lines 8091-8109).
- nesdev wiki `APU registers`
  (`https://www.nesdev.org/wiki/APU_registers`) — chip-select gate
  on 6502 address bus.
- nesdev wiki `DMA`
  (`https://www.nesdev.org/wiki/DMA`) — three address-bus model;
  OAM DMA does not assert APU chip select.
- `crates/nes-core/src/bus.rs:1123-1151` (current `clock_oam_dma_cycle`).
- `crates/nes-core/src/bus.rs:1329-1356` (existing `dmc_dma_read`
  with the equivalent `(halted_addr & 0xFFE0) != 0x4000` check).
- `docs/audit/session-25-sprint2-iter3-frame-counter-irq-2026-05-23.md`
  (Sprint 2 iter 3 LANDED template — the audit-doc structure used here).
