# Sprint 2.4 / Iter 1 — OAM DMA Conflict-Mirror Rollback

**Date:** 2026-05-25 (post-v1.1.0)
**Target:** Close `APU Tests :: APU Register Activation [error 6]`
by mirroring the existing `dmc_dma_read` conflict-path semantics
into `raw_oam_dma_read` for the `halted_addr-IN-$4000-$401F` case
(the deferral surface flagged by Session-26 iter 4).
**Outcome:** **ROLLED BACK** — the conflict-mirror fires correctly
but its register side effects (controller strobe, `read_status`)
cascade into the AccuracyCoin test ROM's own bootstrap and cause
41 tests to be marked `not_run`. Net regression.

## What was attempted

`bus.rs::raw_oam_dma_read` was extended with a second gate
mirroring `bus.rs::dmc_dma_read` lines 1490-1514:

```rust
fn raw_oam_dma_read(&mut self, src_addr: u16) -> u8 {
    // Existing Test 4 gate (Session-26 iter 4): halted_addr OUT,
    // src_addr IN $4000-$401F → silent (return open_bus latch).
    if (self.dma_halt_addr & 0xFFE0) != 0x4000
        && (src_addr & 0xFFE0) == 0x4000 {
        return self.open_bus;
    }
    let sample = self.raw_cpu_read(src_addr);

    // NEW: Test 5/6 conflict-mirror gate: halted_addr IN $4000-$401F
    // → fire $4015/$4016/$4017 side effects based on conflict_addr =
    // 0x4000 | (src_addr & 0x001F), mirroring dmc_dma_read.
    if matches!(self.apu_region(), ApuRegion::Pal) {
        return sample;
    }
    if (self.dma_halt_addr & 0xFFE0) != 0x4000 {
        return sample;
    }
    let conflict_addr = 0x4000 | (src_addr & 0x001F);
    match conflict_addr {
        0x4015 => { let _ = self.apu.read_status(); sample }
        0x4016 => { /* mix controller into sample */ }
        0x4017 => { /* mix controller into sample */ }
        _ => sample,
    }
}
```

## Why it broke

The conflict-mirror correctly models real-silicon behavior, but
the AccuracyCoin test ROM's bootstrap code apparently lands
`dma_halt_addr` inside `$4000-$401F` (via a routine instruction
sequence — e.g., a `LDA $4015` followed by `STA $4014` without an
intervening PRG-ROM read) and the new register side effects
disrupt the ROM's controller / IRQ state expectations downstream.

Per-suite breakdown post-fix (vs pre-fix baseline):

| Suite | Pre-fix | Post-fix | Delta |
|---|---:|---:|---:|
| `APU Registers and DMA tests` | 10/0 | 10/0 | — |
| `APU Tests` | 5/3 | 5/1 (3 `not_run`) | -2 fail (but 3 untested) |
| `CPU Behavior` | 9/0 | 9/0 | — |
| `CPU Behavior 2` | 5/1 | 0/0 (5 `not_run`) | **regressed** |
| `CPU Interrupts` | 0/3 | 0/3 | — |
| `PPU Behavior` | 7/0 | 0/0 (7 `not_run`) | **regressed** |
| `PPU Misc.` | 2/6 | 0/0 (8 `not_run`) | **regressed** |
| `PPU VBlank Timing` | 7/0 | 0/0 (7 `not_run`) | **regressed** |
| `Power On State` | 0/0 (5 `not_run` by design) | 0/0 (5) | — |
| `Sprite Evaluation` | 8/1 | 0/0 (9 `not_run`) | **regressed** |
| Others (unofficial instructions, etc.) | 7-10 pass each | unchanged | — |

The `not_run` markers mean the test ROM didn't *reach* those test
cases — execution hit a stuck state before the relevant test code
ran. The headline "96% pass rate over 100 assigned tests" is
ARTIFACTUAL: the failing tests dropped because most of the
previously-failing ones never ran to be measured.

Workspace strict tests stayed 599/0/6 — the regression is invisible
at the standard `cargo test` layer; only the AccuracyCoin
per-suite drill-down exposes it.

## Why the fix shape was wrong

Session-26 iter 4's note on Test 5/6 conflict-path semantics:

> The Test 5 conflict-path semantics (where the 6502 bus IS in
> `$4000-$401F` because the test uses `JSR $3FFE` + the BRK trick)
> need additional modelling…

The wording "JSR $3FFE + BRK trick" is **specific** — the ROM
deliberately parks the bus inside `$4000-$401F` only when running
those specific test setups. Normal CPU flow does NOT park the bus
there, EXCEPT during routine `LDA $4015` / `LDA $4016` / `LDA $4017`
instructions where the bus is briefly IN `$4000-$401F`.

My implementation fires the conflict-mirror on EVERY OAM DMA where
the **most recent** CPU read was inside `$4000-$401F`. That's far
too broad: any ROM that does `LDA $4015` before `STA $4014` (a
common idiom for "wait for sample finished, then DMA") now
triggers the side effects.

Real silicon's conflict path: the 6502 bus is **physically parked**
at the halt point. If the halt happens during a `LDA $4015`'s
*write-back* cycle (the cycle that drives the data bus value to
the CPU register), the bus reads `$4015` then SETTLES somewhere
else (e.g., the instruction fetch of the NEXT opcode at PRG ROM
addresses). The DMA halt freezes the bus AT THAT NEXT-OPCODE-FETCH
address, NOT at `$4015`. So `dma_halt_addr` would actually be
`$8xxx` (PRG ROM), not `$4015`.

The Test 5/6 trick `JSR $3FFE` is special: it stacks the return
address, then jumps to `$3FFE`. The CPU FETCHES from `$3FFE` for
the next opcode, putting `dma_halt_addr = $3FFE` ... but `$3FFE`
is NOT inside `$4000-$401F`. The BRK at `$3FFE` then pushes the
PC ($4000, the byte after the BRK) and the flags, and JUMPS via
the BRK vector to wherever code lives. The key is that during
the BRK's intermediate cycles, the bus is briefly at `$4000` —
exactly inside the conflict window.

So the conflict-mirror fires ONLY in this narrow case where the
CPU instruction sequence INHERENTLY parks the bus at a
`$4000-$401F` address as part of its mid-execution state. Modelling
this requires sub-instruction CPU bus-tracking — the same surface
that the C1 axis covers. **`dma_halt_addr` (set on the LAST
COMPLETED read) is not granular enough.**

## Conclusion + next steps

1. **Reverted.** `git checkout HEAD -- crates/nes-core/src/bus.rs`.
   Baseline 13 failures restored, 599 strict pass + 5 ignored.
2. **APU Register Activation [error 6] requires the C1 axis** —
   the same master-clock-precise scheduling that v2.0 Sprint A
   delivers. The `dma_halt_addr` granularity at v1.x is insufficient
   to discriminate Test-5/6's "bus parked inside $4000-$401F mid-
   instruction" from "bus parked outside $4000-$401F at end of
   last instruction".
3. **Pivot to DMC Delta Modulation Channel [error 21]** — the
   other Sprint 2.4 target. Per session-22 audit, this requires
   "a Mesen2 oracle trace covering this specific sub-test" to
   identify the failing axis. The error 21 = "K" code per
   `accuracycoin-readme-analysis-2026-05-17.md` is "DMC sample
   address should overflow to $8000 instead of $0000" — but our
   `dmc.rs` line 175-179 ALREADY implements that wrap correctly.
   So error 21 is some other DMC subtest. Needs deeper research
   that exceeds single-session scope.

## Sprint 2.4 status

**Not closable in v1.2.0 at v1.1.0 baseline.** Both candidates
(APU Reg Activation, DMC) require either C1-axis work (v2.0) or
Mesen2 oracle work (multi-session investigation). The +1-+2
AccuracyCoin gain I estimated for Sprint 2.4 is **not achievable
at v1.x without invoking the master-clock refactor**.

**Recommendation to user:** Defer Sprint 2.4's 2 tests to v2.0
Sprint A (alongside the 3 CPU Interrupts tests they share an axis
with), and pivot v1.2.0's calendar budget to either:
- (b) Sprint 2.3 (Implied Dummy Reads + DMC scheduler) — Session-19
  documented cascade target; +1 AccuracyCoin
- (d) Sprint 2.5 (6 ignored commercial ROMs) — fundamentally
  different surface; +4-6 ROM flips (different metric)

Sprint 2.2's 6 PPU Misc residuals remain EXTREME-cascade per
Session-28 and require Cascade A re-baseline authorization.
