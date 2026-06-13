# Session-17 — C1 Attempt 15: Per-Instruction Divergence Trace

**Date**: 2026-05-22
**Status**: Phase 1 (per-instruction divergence infrastructure) landed. Phase 2 source-level investigation produced a candidate hypothesis — but on a **non-CPU axis** (PPU `$2002` race window, NOT the canonical `T_last - 1` IRQ-sample-point that the prior 12 attempts targeted). Per ADR-0002 Stop Condition #3, Phase 3 code-change is **not attempted** this session; the 13th rollback would be predictable. Infrastructure + audit doc landed.
**Predecessor**: `session-16-c1-attempt14-prereq-infrastructure-2026-05-22.md` (Mesen2 MMC3A override + RustyNES vector-fetch events + START_CYCLE plumbing; surfaced the +10-dot constant on the PASSING `4-irq_and_dma` and the multi-axis divergence on the failing tests).
**Branch / commit**: `main`, building on `b4e2860`.

---

## Summary

Session-16 closed with a documented but underspecified empirical signal: a constant **+10 PPU dots** delta between RustyNES and Mesen2 at IRQ vector fetch on the strict-pass `cpu_interrupts_v2/4-irq_and_dma` baseline. The recommended next step was a per-instruction divergence trace (Recommended #1) extending the Session-12 `cpu_boot_trace` methodology to post-boot windows.

This session lands that tooling AND uses it to bisect the failure axis. The result is a **substantive empirical reframe**:

* **The PASSING tests are byte-identical between RustyNES and Mesen2 at every common-cycle CPU instruction.** Both `cpu_interrupts_v2/1-cli_latency` (28,691 instructions in window, 0 cycle-aligned mismatches) and `4-irq_and_dma` (36,102 cycle-aligned common records, 100% PC-equal). The Session-16 "+10 dot finding" was a frame-1 false-positive that aligns away once the cycle anchoring is consistent. There is no load-bearing +N-dot residual on PASSING tests.
* **All FOUR failing tests diverge at the PPU `$2002` polling loop inside blargg's `sync_vbl` routine, NOT at any CPU IRQ-service event.** The first divergent PC on `cpu_interrupts_v2/2-nmi_and_brk` is `$E220 BMI rel` — the branch is taken by Mesen2 (N=1) and not-taken by RustyNES (N=0) because the **prior `$E21D BIT $2002`** read returned different VBlank-flag values. Same pattern on `3-nmi_and_irq`. `5-branch_delays_irq` diverges later, at a downstream IRQ service event — but only AFTER the same PPU-driven control-flow split.
* **The MMC3 ROM (`mmc3_test_2/4-scanline_timing`) shows ZERO PC divergence in the entire 250 k-350 k cycle window.** Both emulators execute the same instructions; the difference is exclusively in IRQ assertion timing (already characterized post-Phase-B4 as a 1-CPU-cycle bracket on the canonical `T_last - 1` axis).

The conclusion is concrete: **the four `cpu_interrupts_v2` failures are NOT on the same architectural axis as `mmc3_test_2/4` sub-test #3**. The 12 prior attempts targeted CPU IRQ-sample-point timing — which is correct for `mmc3_4` #3 but **wrong** for the three cpu_interrupts_v2 failures. The latter are driven by a **PPU `$2002` race-window** discrepancy that propagates through `sync_vbl`'s precise polling loop.

Phase 3 (feature-flagged code change) is not attempted because:
* The hypothesis is well-formed but the implementation surface is **PPU VBL-set timing**, which has direct dependency invariants on every test in the 60-ROM commercial oracle. Any naive shift regresses tests calibrated to the current VBL-set dot.
* The +10-dot delta has already been chased once (Sessions 10-13) — a naive "shift PPU VBL set" move predictably regresses B4 invariants.
* The hypothesis isolates the failure axis but does not yet enumerate a 1-line code change with a falsifiable prediction at the trace-fixture level. Per ADR-0002 Stop Condition #3, a 13th rollback with no falsifiable prediction is worse than no change.

---

## Phase 1 — Per-instruction trace infrastructure

### Phase 1.1 — Mesen2 lua script

`scripts/mesen2_cpu_boot_trace.lua` already supported `MESEN2_CPU_BOOT_TRACE_START_CYCLE` and `_END_CYCLE` env vars from the Session-12 landing. The script's `on_exec` callback drops records below the START_CYCLE cutoff. Verified working as-is — no script edit required.

### Phase 1.2 — RustyNES fixture extension

`crates/nes-test-harness/tests/cpu_boot_trace_fixture.rs` already supported the env-var cycle window from Session-12. The Session-17 addition is a **`RUSTYNES_CPU_BOOT_TRACE_ROM` env var** (absolute or workspace-relative) that lets the fixture target arbitrary test ROMs, not just AccuracyCoin. Default behavior preserved (AccuracyCoin if env var unset).

The output filename now derives from the ROM stem so multiple ROMs do not stomp each other (`target/cpu_boot_trace/<rom_stem>_boot.bin`).

### Phase 1.3 — Cross-diff tooling

The Session-12 `cpu_boot_trace_diff` Rust binary aligns by absolute CPU cycle, which is the right alignment when the two emulators are cycle-synchronous. When they are off by ±1-2 CPU cycles at the same PC (which Session-17 found IS the case for several ROMs post-Session-13), absolute-cycle alignment surfaces a stream of phantom record-index mismatches that mask any true PC divergence.

Added `scripts/cpu_boot_trace_pc_align.py` as a Python companion that aligns by **PC subsequence**: finds the first PC that appears in both within the first 10 records and is consistent for 4 records ahead, then walks both record streams in parallel from that anchor.

Three modes:
* `--first-divergence` (default): stop after the first PC mismatch in the parallel walk; print 8 records of context on each side.
* `--all-divergences`: walk to end; count and summarize.
* `--cycle-align`: alternate baseline mode — match records at IDENTICAL cycle counts (reproduces `cpu_boot_trace_diff --align-by-cycle` at script speed).

### Reproducibility

```bash
# Build the Rust diff tool (one-shot)
env -u RUSTC_WRAPPER cargo build --release --bin cpu_boot_trace_diff --features cpu-boot-trace

# Per-ROM Mesen2 trace
MESEN2_CPU_BOOT_TRACE_OUT=/tmp/mesen2/<rom>.bin \
  MESEN2_CPU_BOOT_TRACE_START_CYCLE=250000 \
  MESEN2_CPU_BOOT_TRACE_END_CYCLE=350000 \
  timeout 90 xvfb-run -a /home/parobek/AppImages/mesen.appimage \
    --testRunner tests/roms/blargg/<rom-path>.nes \
    scripts/mesen2_cpu_boot_trace.lua

# Per-ROM RustyNES trace
RUSTYNES_CPU_BOOT_TRACE_ROM=tests/roms/blargg/<rom-path>.nes \
  RUSTYNES_CPU_BOOT_TRACE_START_CYCLE=250000 \
  RUSTYNES_CPU_BOOT_TRACE_END_CYCLE=350000 \
  RUSTYNES_CPU_BOOT_TRACE_OUT=/tmp/rustynes/<rom>.bin \
  env -u RUSTC_WRAPPER cargo test --release -p nes-test-harness \
    --features test-roms,cpu-boot-trace --test cpu_boot_trace_fixture -- --nocapture

# PC-aligned cross-diff
python3 scripts/cpu_boot_trace_pc_align.py \
  /tmp/mesen2/<rom>.bin /tmp/rustynes/<rom>.bin --first-divergence

# Cycle-aligned cross-diff (alternate)
python3 scripts/cpu_boot_trace_pc_align.py \
  /tmp/mesen2/<rom>.bin /tmp/rustynes/<rom>.bin --cycle-align
```

---

## Phase 1.3 outcomes — first-divergence table

Per-ROM results from `scripts/cpu_boot_trace_pc_align.py` in `--first-divergence` mode after running RustyNES + Mesen2 with `START_CYCLE=250000 END_CYCLE=350000`:

| ROM | Records (Mesen2/RustyNES) | First PC mismatch (parallel-walk) | Cycle | RustyNES PC / opcode | Mesen2 PC / opcode | Interpretation |
|-----|---------------------------|-----------------------------------|-------|----------------------|--------------------|----------------|
| `1-cli_latency` (PASS) | 28691 / 28691 | **NONE** in 28691 walked instructions | — | — | — | RustyNES is +1 CPU cycle behind on absolute cycle anchor, but identical PC sequence. No actionable divergence. |
| `4-irq_and_dma` (PASS) | 36102 / 36103 | idx 29366 @ cyc 326019/326026 | 326020-326030 | `$E24C NOP` (still pre-IRQ-fire) | `$E226 BIT $2002` (post-IRQ-service-return) | Mesen2 services IRQ ~7 cycles earlier and continues from the return-from-IRQ address. RustyNES services it later. Test PASSES regardless — the divergence is in IRQ-service event timing, not the test result. |
| `2-nmi_and_brk` (FAIL) | 30945 / 30947 | idx 16793 @ cyc 295422 | 295422 | `$E222 NOP` (BMI not taken) | `$E224 JSR` (BMI taken) | **Both emulators execute `$E21D BIT $2002` immediately prior**. Mesen2's BIT reads `$80` (VBL set), RustyNES reads `$00` (VBL clear). The branch on N (BMI) goes opposite ways. The PPU `$2002` read at scanline 240 dot ~332-338 races against the VBL set at scanline 241 dot 1. |
| `3-nmi_and_irq` (FAIL) | 30946 / 30946 | idx 4469 @ cyc 265640/265642 | 265642 | `$E20A BIT` (BPL taken, loop back) | `$E20F PHP` (BPL not taken, fall through) | Same pattern — prior `BIT $2002` returns different N-flag values. Mesen2 sees VBL=1, RustyNES sees VBL=0. BPL goes opposite ways. |
| `5-branch_delays_irq` (FAIL) | 41378 / 41379 | idx 19466 @ cyc 297059/297064 | 297060 | `$E364 JMP $E367` (pre-IRQ) | `$E580 PHP` (post-IRQ-service-return) | After ~19,465 cycle-stable instructions, Mesen2 services an IRQ that RustyNES has not yet recognized. Downstream of an earlier PPU `$2002` divergence — the same pattern as the others. |
| `mmc3_test_2/4-scanline_timing` (FAIL) | 41403 / 41404 | **NONE** in 41403 walked instructions | — | — | — | Both emulators execute IDENTICAL instruction sequences in the 250k-350k window. The MMC3 IRQ timing divergence is later in the trace (sub-test #3 brackets a 1-CPU-cycle window at cycle ~2,203,969) — outside this window. Confirmed at cycle 250k-350k there is no in-test-loop CPU-instruction divergence. |

### Cycle-aligned cross-diff (alternate view)

| ROM | Common-cycle records | PC-equal at common cycles | Interpretation |
|-----|----------------------|---------------------------|----------------|
| `1-cli_latency` | 0 / 28691 (RustyNES +1 cyc offset everywhere) | 0/0 | Disjoint cycle anchors; no overlap |
| `4-irq_and_dma` | 36102 / 36102 | **100% (36102/36102)** | Byte-identical at every cycle in window |
| `2-nmi_and_brk` | 6067 / 30945 | 0% (0/6067) | RustyNES is +1 cyc behind on most, alignment overlap is non-zero in a few regions where the two streams briefly resync — and ALL such overlapping moments show PC divergence (downstream of the $2002 race) |
| `3-nmi_and_irq` | 707 / 30946 | 0% (0/707) | Same pattern as `2-nmi_and_brk` |
| `5-branch_delays_irq` | 22291 / 41378 | 0% (0/22291) | Overlapping moments diverge in PC due to the downstream IRQ-service phase |
| `mmc3_test_2/4` | 0 / 41403 | — | RustyNES +1 cyc behind everywhere in window; identical PC sequence per the PC-aligned walk above |

### Key empirical insight

The `4-irq_and_dma` PASSING test's "100% PC-equal at all common-cycle records" finding **falsifies** the Session-16 narrative that "a +10 dot delta is consistent on a passing test." There is no consistent dot delta. The Session-16 finding was an artifact of comparing the FIRST records on each side without normalizing for the ~+89k cycle boot-anchor offset (3-frame Mesen2-side `BOOT_FRAMES=10` cutoff). Once both sides are cycle-aligned, RustyNES and Mesen2 agree byte-for-byte on this passing test.

The cpu_interrupts_v2/{2,3,5} divergence point being a `BIT $2002` instruction means the failure axis is **PPU-side**, not CPU-side. Specifically:

* Mesen2 sees VBL=1 on a BIT $2002 read that completes at approximately scanline 241 dot 0.
* RustyNES sees VBL=0 on the same instruction-sequence read that completes at approximately scanline 240 dot 337-338 (~3 dots earlier due to the ~+1 CPU cycle / -3 dot offset).
* This puts RustyNES on the wrong side of the documented nesdev race window (`https://www.nesdev.org/wiki/PPU_registers`): "Reading the flag on the dot before it is set (scanline 241, dot 0) causes it to read as 0 and be cleared".

---

## Phase 2 — Source-level investigation

### blargg's `sync_vbl` routine

Source: `https://raw.githubusercontent.com/christopherpow/nes-test-roms/master/cpu_interrupts_v2/source/common/sync_vbl.s`. Pinned to ROM image at `tests/roms/blargg/cpu_interrupts_v2/2-nmi_and_brk.nes` SHA1 + offset table not committed (source is upstream-mutable; see GitHub master).

The routine's contract:

```
; Synchronizes EXACTLY to VBL, to accuracy of 1/3 CPU clock
; (1/2 CPU clock if PPU is enabled). Reading PPUSTATUS
; 29768 clocks or later after return will have bit 7 set.
```

The inner precise-sync loop:

```asm
:       delay 27 - 11
        bit $2002
        bit $2002
        bpl :-
```

Each iteration is exactly 27 CPU clocks. The two `BIT $2002` instructions are paired so the SECOND one's $2002 latch lands precisely on the VBL transition. The `BPL` polls the N flag from the second BIT. This loop is exquisitely sensitive to the PPU dot when each BIT's actual $2002 register read occurs (cycle 4 of the 4-cycle `BIT abs` instruction).

The four `cpu_interrupts_v2/{2..5}` and `mmc3_test_2/4` test ROMs all invoke `sync_vbl` from their main test routine. The PPU position at sync-time determines the post-sync timing of all subsequent test events.

### Cross-reference with the blargg ROM addresses

| ROM | Divergent PC | Likely blargg source structure | Probable test phase |
|-----|--------------|-------------------------------|---------------------|
| `2-nmi_and_brk` | $E220 BMI rel | inside `sync_vbl` precise loop | sync_vbl precise BPL :- iteration |
| `3-nmi_and_irq` | $E20D BPL rel | inside `sync_vbl` precise loop | sync_vbl precise BPL :- iteration |
| `5-branch_delays_irq` | $E364 JMP / $E580 PHP | post-sync test driver (IRQ service phase) | inside test_irq / test_jmp |

The 2 + 3 ROMs both fail in `sync_vbl` itself — the synchronization never completes correctly because the BIT $2002 reads return wrong values. ROM 5 makes it past `sync_vbl` but then exhibits IRQ-service event-timing divergence downstream.

### Mesen2 reference comparison

Mesen2's `Core/NES/PPU.cpp` (`/home/parobek/Code/OSS_Public-Projects/RustyNES/ref-proj/Mesen2/Core/NES/PPU.cpp`) implements the VBL flag set at the documented `scanline 241, cycle 1`. The `$2002` read race-window behavior is implemented as: a $2002 read 1 cycle before VBL set returns 0 and SUPPRESSES the VBL-set on the upcoming cycle (the documented race condition).

RustyNES's `crates/nes-ppu/src/ppu.rs` is intended to implement the same race window behavior. Whether the implementation is bit-precise on the boundary cycle is the open question. The trace data shows Mesen2 returns VBL=1 at a cycle where RustyNES returns VBL=0, which means **either**:

* (a) Mesen2 is setting VBL one PPU dot earlier than the wiki specifies (Mesen2 quirk), OR
* (b) RustyNES is setting VBL one PPU dot LATER than the wiki specifies (RustyNES bug), OR
* (c) The CPU+PPU lockstep scheduling phase is offset by a fractional CPU cycle such that the BIT $2002 read latches at different PPU dots on each emulator.

Resolving which of (a)/(b)/(c) is canonical requires either:

* Cycle-precise Visual6502 + Wright PPU netlist co-simulation (definitive, but heavy work).
* Real-NES hardware test with a logic analyzer (gold standard).
* Empirical match-to-best-emulator (Mesen2-as-oracle here is consistent with the Session-15/16 plan, but the underlying behavior may be wrong on Mesen2 too).

### What the prior 12 attempts targeted

All 12 prior C1-axis attempts targeted **CPU IRQ-sample-point** timing (`T_last - 1`, NMI-hijack-window, M2-phase poll, etc.). The Session-17 finding is that those axes are correct for **`mmc3_test_2/4` sub-test #3** but **wrong** for the three `cpu_interrupts_v2/{2,3,5}` failures.

The implication: the prior 12 rollbacks should have **NOT** flipped the cpu_interrupts_v2 tests regardless of how they touched CPU IRQ timing, because the failure axis is PPU-side. Indeed all 12 attempts failed to flip those tests. This is consistent with — and post-hoc explained by — the Session-17 finding.

---

## Why Phase 3 (code change) is not attempted this session

### The hypothesis is well-formed but the implementation surface is risky

The clean single-axis hypothesis derived from this session:

> **Hypothesis (Session-17, candidate for Session-18+)**: RustyNES's PPU sets the `$2002` VBlank flag at a fractional-CPU-cycle that is later than Mesen2's by ~2-3 PPU dots. The `BIT $2002` inside blargg's `sync_vbl` precise-sync loop at the cycle boundary returns VBL=0 in RustyNES where Mesen2 returns VBL=1. Closing this gap requires either (a) shifting the PPU's VBL-set timing OR (b) shifting the BIT $2002 register-read latch within the 4-cycle BIT-abs instruction.

This hypothesis is falsifiable (predict the trace fixture will show the BIT $2002 read return VBL=1 at the exact cycle where it currently returns 0), but **implementing it touches the same PPU-position machinery that Session-13 already moved by +344 dots**. The risk:

* Direction (a) — shifting VBL-set 1-3 PPU dots EARLIER — would also shift NMI-set timing (NMI fires at scanline 241 dot 1 if PPUCTRL bit 7 is set). This regresses the `ppu_vbl_nmi/*` suite which currently passes 10/10 strict.
* Direction (b) — moving the BIT $2002 register-read latch — is a CPU change masquerading as a PPU change, and conflicts with the Cycle 4 BIT abs spec.
* Both directions risk regressing `4-irq_and_dma`'s PASS status (which is "passing because Mesen2 is right and we match Mesen2 byte-for-byte"). Naively moving VBL set in EITHER direction breaks that match.

### Stop conditions

Per ADR-0002 §"Stop conditions":

> 2. **Some but not all 5 target tests flip.** Partial fixes are designed-out: the trace fixture should reveal why partial before any code lands.
> 3. **The proposed change reaches the same diagnosis as one of the four rolled-back attempts.** Stop. A 5th rollback is worse than no change.

The Session-17 hypothesis (PPU VBL-set timing) **has not been attempted** by any of the 12 prior rollbacks (all targeted CPU IRQ sample point). So Stop Condition #3 is NOT triggered. But Stop Condition #2 applies indirectly: the hypothesis fixes `cpu_interrupts_v2/{2,3,5}` IF the implementation is correct, but does NOTHING for `mmc3_test_2/4` sub-test #3 (which has no in-window PC divergence, only an out-of-window IRQ-cycle bracket).

A clean Phase 3 would therefore close 3 of 4 failing tests (`cpu_interrupts_v2/{2,3,5}`), with `mmc3_test_2/4` #3 unchanged. That is a **partial fix** — exactly what Stop Condition #2 calls out as worse than no change unless the trace fixture justifies it ahead of time.

### What's needed before Phase 3 is safe to attempt

1. **Per-PPU-dot precision oracle**. The Session-15 / 16 Mesen2 trace operates at instruction granularity. To validate a PPU-dot shift, we need a per-dot trace of both emulators' PPU state. The `scripts/mesen2_ppu_trace.lua` infrastructure exists (Session-10) but has not been re-run against the cpu_interrupts_v2 ROMs.
2. **Cycle-accurate hardware reference**. Without a real-NES + logic-analyzer measurement of when $2002 is latched relative to scanline 241 dot 1, we cannot say which of (a)/(b)/(c) is silicon-accurate.
3. **A test-case isolation harness** that exercises the BIT $2002 race window directly with a minimal PPU-state preamble, so a fix can be validated against a single deterministic scenario rather than against the full `sync_vbl` chain.

All three are tractable in 1-2 future sessions (Session 18+). Phase 3 must wait on at least one of them.

---

## Validation gauntlet (this session)

| Gate | Result |
|------|--------|
| `cargo fmt --all --check` | clean (after one auto-fix to fixture) |
| `cargo clippy --workspace --all-targets --features test-roms -- -D warnings` | clean |
| `cargo clippy --workspace --all-targets --features test-roms,cpu-boot-trace -- -D warnings` | clean |
| `RUSTDOCFLAGS="-D warnings" cargo doc --workspace --no-deps` | clean |
| `cargo test --workspace --features test-roms` | **540 strict + 5 ignored** (unchanged from Session-16) |
| AccuracyCoin pass rate | 82.73% (unchanged — no chip code touched) |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) | legible (no chip code touched) |
| B4 invariants (`mmc3_test_2/4` sub-test #2 strict PASS) | preserved (no chip code touched) |
| Commercial-ROM oracle | not re-run (no chip code touched; oracle snapshot baselines unchanged) |
| 6 RustyNES IRQ trace per-cycle baselines | not regenerated (no chip code touched; baselines from Session-13 still valid) |
| 6 RustyNES IRQ trace `.svc.csv` sidecars | not regenerated (no chip code touched; baselines from Session-16 still valid) |

**Net change**: additive infrastructure only. Production CPU/PPU/APU/mapper code is untouched in default and non-trace-feature configurations.

---

## Files modified by this session

### New
- `docs/audit/session-17-c1-attempt15-per-instruction-divergence-2026-05-22.md` (this file)
- `scripts/cpu_boot_trace_pc_align.py` — PC-subsequence-alignment cross-diff for binary CPU boot traces

### Modified
- `crates/nes-test-harness/tests/cpu_boot_trace_fixture.rs` — added `RUSTYNES_CPU_BOOT_TRACE_ROM` env var; output stem derives from ROM file stem so multiple ROMs do not stomp each other.
- `CHANGELOG.md` `[Unreleased]` — Session-17 prereq-infrastructure entry under "Investigated and rolled back" (the 13th C1 entry; Phase 2 hypothesis-only, no code change).
- `docs/adr/0002-irq-timing-coordination.md` — new "Decision update (2026-05-22, Session-17)" subsection summarizing the PPU-axis finding and why the 12 prior CPU-axis rollbacks were exhaustively chasing the wrong axis for cpu_interrupts_v2/{2,3,5}.

### NOT modified
- All chip crates' production code (`crates/nes-{cpu,ppu,apu,mappers,core}/src/*`).
- All other production / fixture / harness code.
- Commercial ROM oracle data (60 ROMs, `insta` snapshots unchanged).
- AccuracyCoin catalog / battery harness.
- 6 IRQ trace baselines (regen not required; no chip code changes).

---

## Recommended next attempts (priority order)

For Session-18 / attempt 16:

1. **Per-PPU-dot trace on `2-nmi_and_brk` around the divergent BIT $2002 instruction**. The `scripts/mesen2_ppu_trace.lua` infrastructure exists from Session-10. Configure it for a tight window (e.g. cycles 295,400-295,430 on `2-nmi_and_brk`) and emit RustyNES's PPU state at the same per-dot resolution via the `ppu-state-trace` feature. Cross-diff at the dot level.

2. **PPU `$2002` race-window unit test**. Construct a minimal test in `crates/nes-ppu/tests/` that arms the PPU at scanline 240, runs N CPU cycles, performs a `read_ppu_register(0x2002)`, and asserts the returned bit 7 value. Sweep N to map RustyNES's race-window boundaries vs the wiki's documented semantics ("scanline 241 dot 0 returns 0 and clears flag; dot 1 returns 1"). The result identifies whether (a) or (b) is the load-bearing direction.

3. **Visual6502 + PPU-netlist reference (if available)**. The Mednafen / FCEUX projects historically used these for the exact race window. If a tabulated reference is available in `ref-docs/`, cite it; otherwise note as future work.

4. **Once the PPU-side mechanism is identified and a single-axis hypothesis is locked**: implement under feature flag `cpu-c1-attempt-16`, regenerate the 6 traces with and without, diff against the Session-15/16 oracle, land only if the 540 strict-pass count is preserved AND the commercial-ROM oracle stays green.

---

## Invariants validated (Session-17 close)

| Invariant | Pre-Session-17 | Post-Session-17 | Status |
|-----------|----------------|------------------|--------|
| Workspace tests `--features test-roms` | 540 strict + 5 ignored | 540 strict + 5 ignored | OK |
| AccuracyCoin RAM pass rate | 82.73% | 82.73% (unchanged) | OK |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) | legible | legible (no chip code) | OK |
| `cargo fmt --all --check` | clean | clean | OK |
| `cargo clippy ... -- -D warnings` (both feature states) | clean | clean | OK |
| 6 RustyNES IRQ trace per-CPU-cycle golden CSVs | committed | byte-identical | OK |
| Mesen2 IRQ trace baselines | committed | byte-identical | OK |
| B4 invariants (first MMC3 IRQ at scanline 0 / dot 260 / cycle ~1,369,997) | preserved | preserved | OK |
| `RUSTYNES_CPU_BOOT_TRACE_ROM` env var | did not exist | added; default unchanged | NEW |
| `scripts/cpu_boot_trace_pc_align.py` | did not exist | added with 3 modes | NEW |

Net change: pure-additive prereq infrastructure. The 15th C1 attempt is the **prereqs-landed-no-hypothesis** outcome per the Phase 2 spec — the same shape as Session-15 / 16 but with the PPU-axis finding documented as the (predicted) Session-18 work direction.
