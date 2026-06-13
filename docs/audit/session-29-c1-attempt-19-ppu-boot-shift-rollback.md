# Session-29 — C1 Attempt 19: PPU Boot Position +2 Dot Shift (16th rollback)

**Date**: 2026-05-23
**Status**: ROLLED BACK. 24 strict regressions. Cycles=8 reset path empirically confirmed +2 PPU dot residual vs Mesen2.

---

## Hypothesis

Per Session-29 boot-trace analysis (using `RUSTYNES_CPU_BOOT_TRACE_*` env vars + `cpu_boot_trace_diff` Rust binary), the divergence at the first instruction (SEI at $E683) between RustyNES and Mesen2 on `cpu_interrupts_v2/2-nmi_and_brk.nes` is:

```
[diff @ cycle=7 PC=$E683 frame=1 scanline=0 dot=25] SEI
    cycle    ref=7            actual=8
    dot      ref=25           actual=23
```

So at the same PC, Mesen2 is at PPU dot 25 while RustyNES is at dot 23 — a 2-PPU-dot residual after the 8-cycle reset sequence.

Mesen2's reset pre-loop (`Core/NES/NesCpu.cpp` line 158, `_masterClock += cpuDivider + cpuOffset;` = 12 master clocks = 3 PPU dots) advances the PPU BEFORE the 8-cycle loop, without incrementing `_state.CycleCount`. With `_ppuOffset = 1`, the effective PPU advance is 11 master clocks = 2.75 PPU dots ≈ 2 full dots.

Our impl doesn't replicate this — PPU advances in lockstep with the CPU's reset cycles only.

Attempt 19 was to shift PPU init from `(prerender_line, dot=340)` to `(scanline=0, dot=1)`, putting the PPU 2 dots ahead at power-up. After 8 reset cycles (24 PPU ticks), RustyNES would land at `(scanline=0, dot=25)` — matching Mesen2.

---

## Result — 24 STRICT REGRESSIONS

Gauntlet with the shift applied:

```
TOTAL: 521 pass, 24 fail, 5 ignored
```

Regressions span:

* `ppu::tests::cascade_a_verify_sprite_zero_hits_step2` — Cascade A sprite-eval test
* `audio_db_apu`, `audio_tri_silence`, `audio_dac_square` — APU/mixer audio
* `audio_test_vrc7`, `audio_noise_vrc7`, `audio_db_vrc7`, `audio_patch_vrc7`, `audio_clip_vrc7` — VRC7 audio tests (note: VRC7 FM is deferred per ADR-0004, but the wrapper tests still rely on PPU timing)
* `audio_db_vrc6a`, `audio_db_mmc5` — mapper audio
* (plus 13 more across blargg ppu_vbl_nmi, sprite_hit_tests, etc.)

Every test in our gauntlet that depends on PPU/CPU phase alignment is calibrated to the CURRENT `(prerender_line, dot=340)` setting. Shifting it globally breaks them.

---

## Empirical conclusion (consistent with Sessions 13, 17, 18)

The C1 axis cannot be closed via a single global PPU-position or CPU-side ordering shift. Per Session-17 trace data, only `cpu_interrupts_v2/{2,3,5}` and `mmc3_test_2/4` show the +1 CPU cycle absolute anchor — `cpu_interrupts_v2/{1,4}` are byte-identical with Mesen2 at the same anchor.

This means the divergence is **test-specific** — something about how tests 2/3/5 exercise the CPU/PPU/APU during their specific boot sequences consumes 1 cycle differently than Mesen2's exact implementation.

The next-iteration approach needs to be:

1. **Per-instruction trace at the 0..200 cycle boot window of test 2-nmi_and_brk** (the simplest of the FAILING tests). Compare RustyNES's per-instruction CPU state to Mesen2's. Identify the EXACT instruction where the +1 cycle drift first appears.

2. **Once identified, examine the specific opcode** for any 1-cycle deviation between RustyNES's dispatch and Mesen2's. The fix is then a TARGETED 1-line opcode change, not a global PPU-position or access-ordering shift.

3. **Verify the fix doesn't regress the PASSING tests** (1-cli_latency, 4-irq_and_dma) — these already align byte-for-byte and any cycle change on the wrong opcode would break them.

---

## What landed

`docs/audit/session-29-c1-attempt-19-ppu-boot-shift-rollback.md` — this document.

Production code REVERTED to pre-attempt state. Gauntlet: 545 strict + 5 ignored.
AccuracyCoin: 90.65% (unchanged).
Commercial-ROM oracle: 60/60.

---

## C1 axis rollback count

16th C1 axis rollback (attempts 1-4 + Phase B4 prototype + mid-cycle snapshot + M2-low CPU IRQ sample + Sessions 14-15 prereq + Session-17 hypothesis-only + Session-18 PPU-axis predicate + Session-29 attempt-17 φ1/φ2 v1 (3-dot flat) + Session-29 attempt-17 φ1/φ2 v2 (1+2 split) + Session-29 attempt-18 combined + this Session-29 attempt-19 PPU boot shift).

The C1 axis IRQ-timing residuals (4 tests: `cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4` sub-test #3) remain documented v1.x-deferred. AccuracyCoin ≥ 90% gate is CLEARED (90.65% on `main`).
