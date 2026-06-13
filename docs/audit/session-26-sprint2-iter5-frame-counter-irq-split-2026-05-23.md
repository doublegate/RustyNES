# Session 26 — Sprint 2 iter 5: Frame Counter `irq_flag` vs CPU IRQ-line split

**Date:** 2026-05-23
**Branch:** `main` (HEAD post-iter-4-landing)
**Scope:** Sprint 2 iteration 5 of v1.0.0-final: separate the
`$4015` bit 6 visibility (`FrameCounter::irq_flag`) from the CPU
IRQ-line driver (`FrameCounter::irq_line_active`), enabling the
AccuracyCoin `APU Tests :: Frame Counter IRQ` Tests I/J/K/L "the
flag is visible during inhibit for 2 CPU cycles" surface to PASS
without spuriously asserting the CPU IRQ line (the Session-25
cascade that broke 4 MMC3 commercial ROMs).

**Predecessors:**
- `docs/audit/session-25-sprint2-iter3-frame-counter-irq-2026-05-23.md`
  — Sprint 2 iter 3 LANDED + the Test J refinement attempt that was
  rolled back because of the conflation.
- `docs/audit/session-26-sprint2-iter4-apu-reg-activation-2026-05-23.md`
  — Sprint 2 iter 4 LANDED (immediately prior in this session).

## Phase 2A — Conflation analysis (re-read from Session-25)

Session-25 attempted to flip Tests J/K/L by setting `irq_flag = true`
UNCONDITIONALLY at FC steps 28/29/30 (mirroring Mesen2's `_irqFlag =
true; _irqFlagClearClock = 0;` lines 104-115). That cascaded:
`Apu::irq_line()` returned `frame_counter.irq_flag || dmc.irq_flag`,
so setting the flag during inhibit caused the CPU to see spurious
IRQs and broke 4 MMC3 commercial ROMs.

Mesen2 separates these:
1. `_irqFlag: bool` in `ApuFrameCounter` — used by `NesApu::ReadRam`
   to derive `$4015` bit 6 visibility.
2. `IRQSource::FrameCounter` registration on the CPU's `_irqSource`
   list — driven by `SetIrqSource(FrameCounter) / ClearIrqSource(FrameCounter)`
   inside `ApuFrameCounter::Run`.

The two SETS/CLEARS:

| Event | `_irqFlag` (Mesen2) | `IRQSource::FrameCounter` (Mesen2) |
|---|---|---|
| FC step 3 (cycle 29828) | `true` (always) | `SetIrqSource` (only if `!_inhibitIRQ`) |
| FC step 4 (cycle 29829) | `true` (always) | `SetIrqSource` (only if `!_inhibitIRQ`) |
| FC step 5 (cycle 29830) inhibit | `false` | (no `SetIrqSource`; was never set in this run) |
| FC step 5 (cycle 29830) not-inhibit | `true` | `SetIrqSource` |
| `$4017` write inhibit | `false` | `ClearIrqSource` |
| `$4015` read | lazy-clear schedule | `ClearIrqSource` (immediate) |

## Phase 2B — Reader / writer audit (`crates/nes-apu/src/`)

Pre-iter-5 readers of `frame_counter.irq_flag`:
- `apu.rs:191` — `frame_irq_pending()`. Public, used by `Bus` for
  debug introspection. Maps to **`$4015` bit 6 visibility** → stays
  on `irq_flag`.
- `apu.rs:203` — `irq_line()`. Public, used by `Bus::poll_irq_*` to
  drive the CPU IRQ line. Maps to **CPU IRQ line** → moves to
  `irq_line_active`.
- `apu.rs:608` — `read_status()` builds the `$4015` byte. Maps to
  **`$4015` bit 6 visibility** → stays on `irq_flag`.
- Tests in `apu.rs` (lines 709, 716, 721, 737, 741, 752, 756, 760)
  exercise the lazy-clear schedule. Stays on `irq_flag` (these are
  the iter 3 contract tests).
- `snapshot.rs` — encoded/decoded as a single bool. **Bumps to v3
  format** with separate `irq_line_active`.

Pre-iter-5 writers of `frame_counter.irq_flag`:
- `frame_counter.rs:171, 202` (in `read_status` and `tick` lazy
  clear): clears the **visibility flag**. Stays on `irq_flag`.
- `frame_counter.rs:213` (in `tick` inhibit-reset path): clears the
  **visibility flag** + CPU IRQ line. Both — extends to also clear
  `irq_line_active`.
- `frame_counter.rs:250, 275, 284` (FC steps 28/29/30): currently
  set the visibility flag ONLY if `!irq_inhibit`. **Iter 5 split**:
  set visibility flag UNCONDITIONALLY at 28/29 (clears it at 30 if
  inhibited per Test L); set `irq_line_active` only if not inhibited.
- `apu.rs:635` (in `clear_frame_irq_immediate_for_dma`): clears the
  visibility flag during DMC DMA no-op. Extends to also clear
  `irq_line_active`.
- Tests in `frame_counter.rs` exercise the inhibit-reset and the FC
  step branches. Updated to assert both fields.
- Tests in `apu.rs` (lines 709, 737) exercise the lazy-clear. Updated
  to assert both fields where relevant.

## Phase 2C — Split design

New `FrameCounter` field:

```rust
pub struct FrameCounter {
    pub irq_flag: bool,        // $4015 bit 6 visibility (existing semantics)
    pub irq_line_active: bool, // CPU IRQ source driver (NEW iter-5 field)
    // ...
}
```

FC step branches (the load-bearing change):

```rust
29828 => {
    self.irq_flag = true;
    self.irq_flag_clear_cycle = 0;
    if !self.irq_inhibit {
        self.irq_line_active = true;
        ev.irq = true;
    }
}
29829 => {
    // same shape as 29828 + ev.quarter/half
}
29830 => {
    if self.irq_inhibit {
        self.irq_flag = false;            // <-- ends the 2-cycle visibility window per Test L
        self.irq_flag_clear_cycle = 0;
        self.irq_line_active = false;
    } else {
        self.irq_flag = true;
        self.irq_flag_clear_cycle = 0;
        self.irq_line_active = true;
        ev.irq = true;
    }
    self.cycle = 0;
}
```

`Apu::irq_line()`:

```rust
pub const fn irq_line(&self) -> bool {
    self.frame_counter.irq_line_active || self.dmc.irq_flag  // was: irq_flag || dmc.irq_flag
}
```

`Apu::frame_irq_pending()` unchanged (still reads `irq_flag`).

`Apu::read_status` (now: also clears `irq_line_active`):

```rust
// Lazy-clear schedule for $4015 bit 6 visibility (iter 3 unchanged):
self.frame_counter.read_status(self.cpu_cycle, self.apu_phase);
// NEW iter 5: also deassert the CPU IRQ line synchronously
// (`ClearIrqSource(FrameCounter)` in Mesen2 ReadRam).
self.frame_counter.irq_line_active = false;
```

`$4017` inhibit-set in `tick` (extends iter 3 inhibit-reset path):

```rust
if self.irq_inhibit {
    self.irq_flag = false;
    self.irq_line_active = false;  // NEW iter 5
    self.irq_flag_clear_cycle = 0;
}
```

Save-state format v2 → v3:
- v3 appends a single `bool` after the `irq_flag_clear_cycle: u64`
  for `irq_line_active`.
- v2 migration: set `irq_line_active = irq_flag` (coincided under
  the conflated model). Per ADR-0003 best-effort policy.

## Phase 2D — Implementation surface

`crates/nes-apu/src/frame_counter.rs`:
- `+9` lines (new field + new field in `new()` + new field in `reset()`).
- `+5` lines doc on field.
- `+1` line in `read_status` (the unconditional `irq_line_active = false`).
- `+1` line in `tick` inhibit-reset (the unconditional `irq_line_active = false`).
- Step branches restructured (3 branches): `irq_flag = true` unconditional at 28/29, conditional at 30; `irq_line_active = true` only if not inhibited.
- `+9` lines doc on the step branches.

`crates/nes-apu/src/apu.rs`:
- `irq_line()` body: `frame_counter.irq_flag` → `frame_counter.irq_line_active`.
- `+1` line doc on `irq_line()`.
- `clear_frame_irq_immediate_for_dma`: `+1` line (`irq_line_active = false`).

`crates/nes-apu/src/snapshot.rs`:
- `APU_SNAPSHOT_VERSION`: 2 → 3.
- `write_fc`: `+1` line (`w.bool(fc.irq_line_active)`).
- `read_fc`: `+5` lines (version check + bool decode + migration).
- Version-accept check (`restore`): `version != 1 && version != 2 && version != APU_SNAPSHOT_VERSION`.

No new tests added in this commit (the existing test suite + the
custom ROM oracles + the commercial-ROM oracle is what proves the
fix). The 4 MMC3 commercial ROMs are the load-bearing canary.

## Phase 2E — Validation gauntlet results

| Gate | Result |
|---|---|
| `cargo fmt --all --check` | PASS |
| `cargo clippy --workspace --all-targets --features test-roms -- -D warnings` | PASS |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | PASS |
| `cargo build --workspace` | PASS |
| `cargo test --workspace --features test-roms` | **545 strict + 5 ignored** (preserved) |
| `cargo test --test apu_test --features test-roms` | **8/8** PASS (apu_test load-bearing surface preserved, including `apu_test_3_irq_flag` and `apu_test_6_irq_flag_timing` blargg tests) |
| `cargo test --test apu_mixer --features test-roms` | **4/4** PASS |
| `cargo test --test dmc_dma --features test-roms` | **5/5** PASS |
| `cargo test --test mmc3 --features test-roms` | **12 strict + 2 ignored** PASS (B4 invariant preserved) |
| 4 MMC3 commercial canary ROMs (`external_mmc3_mega_man_3`, `tmnt3`, `ninja_gaiden_2`, `tiny_toon_adventures_2`) | **all PASS** — the Session-25 cascade did NOT recur |
| All 7 `external_mmc3_*` commercial ROMs (incl. SMB3, SMB2, Kirby) | **7/7** PASS |
| 60-ROM commercial-ROM oracle (`--features test-roms,commercial-roms`) | **60/60** PASS |
| Custom ROM `frame-counter-irq.nes` | `$0467 = $01` **PASS** (was `$4E` Fail Test J — Tests J/K/L all flipped, Test M also flipped, plus N/O passed) |
| Custom ROM `apu-reg-activation.nes` | `$045C = $1A` **Fail Test 6** (iter 4 LANDED preserved; the additional split doesn't affect OAM-DMA pathways) |
| Custom ROM `controller-strobing.nes` | `$045F = $01` **PASS** (Session-24 Phase 3 preserved) |
| AccuracyCoin RAM-direct pass rate | **83.45% → 84.17%** (110 pass + 7 pass_with_code of 139 assigned tests; +1 net test flipped: Frame Counter IRQ FAIL → PASS, moves out of the failing list; the per-suite APU Tests count moves from 6 pass / 3 fail to 7 pass / 2 fail) |
| AccuracyCoin framebuffer pass rate | **89.83% → 90.68%** (+1pp) |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) visual legibility | preserved (subset of commercial oracle) |

## Phase 2F — Outcome decision

**LANDED.** The Frame Counter IRQ `$4015` bit 6 vs CPU IRQ line
conflation is closed. The Session-25 cascade — which broke the 4
MMC3 commercial ROMs when Test J was attempted via a single-field
implementation — is explicitly NOT recurring under the split:

- `apu_test` 8/8 (the blargg frame-counter timing tests that
  share the IRQ-line semantic).
- All 7 MMC3 commercial ROMs (`external_mmc3_*`).
- 60/60 commercial oracle.
- B4 MMC3 invariant (mmc3_test_2/4 sub-test #2 strict PASS).

The Frame Counter IRQ custom ROM advances all the way from `$4E`
(Fail Test J) to `$01` (PASS) — Tests J, K, L, M, N, O all flipped
simultaneously. AccuracyCoin RAM-direct pass rate moves from 83.45%
to 84.17% (+1 net test, the catalog entry "Frame Counter IRQ" moves
from FAIL to PASS).

## Phase 2G — Save-state migration sanity

The save-state schema bump (v2 → v3) is per ADR-0003:
- v1 blobs continue to migrate via the iter-3 `pending_irq_clear:
  bool` → `irq_flag_clear_cycle: u64` synthesis, then onward via the
  iter-5 `irq_line_active = irq_flag` synthesis.
- v2 blobs migrate via the iter-5 `irq_line_active = irq_flag`
  synthesis only.
- v3 blobs round-trip byte-identically.

Both migration paths may show a 1-CPU-cycle transient on the very
next CPU IRQ-poll cycle as the FC step re-establishes
`irq_line_active` from scratch — acceptable per ADR-0003
"best-effort cross-version" policy.

`save_state.rs` integration tests (5 tests) all pass on the
existing test corpus; the new schema is byte-equivalent for
default-state snapshots (the `irq_line_active = false` bool appends
trivially).

## Final references

- `crates/nes-apu/src/frame_counter.rs` (line ranges shifted by ~10
  lines from iter-3): new `irq_line_active` field + split semantics
  in the FC step branches + the inhibit-reset path + `read_status`.
- `crates/nes-apu/src/apu.rs::irq_line` (≈ line 211): now reads
  `irq_line_active` instead of `irq_flag`.
- `crates/nes-apu/src/apu.rs::clear_frame_irq_immediate_for_dma`
  (≈ line 644): now clears both fields.
- `crates/nes-apu/src/snapshot.rs`: `APU_SNAPSHOT_VERSION` 2 → 3 +
  `write_fc` / `read_fc` extensions + version-accept check.
- Mesen2 `Core/NES/APU/ApuFrameCounter.h` lines 104-115 (the
  conditional IRQ-source-registration at FC steps 3/4/5).
- Mesen2 `Core/NES/APU/NesApu.cpp` `ReadRam` line ≈ 95 (`$4015`
  read clears `IRQSource::FrameCounter` synchronously).
- `100thCoin/AccuracyCoin@main` `AccuracyCoin.asm` lines 10377-10516
  (TEST_FrameCounterIRQ Tests I/J/K/L/M/N/O — the inhibit-visibility-
  window axis).
- `docs/audit/session-25-sprint2-iter3-frame-counter-irq-2026-05-23.md`
  Phase 3.3 (the Test J refinement rollback that motivated this
  iter-5 split).
- `docs/adr/0003-save-state-migration.md` (cross-version policy).
