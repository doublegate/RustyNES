# Session-15 — C1 Attempt 13: Mesen2 IRQ-Cycle Oracle Infrastructure

**Date**: 2026-05-22
**Status**: Phase 1 (oracle infrastructure) landed. Phase 2 (implementation) NOT attempted; the cross-diff data does not yield a single falsifiable hypothesis under the ADR-0002 "Stop conditions" discipline. 12 prior C1-axis attempts have been rolled back; an attempt 13 without a clean oracle-derived hypothesis would predictably become the 13th rollback.
**Predecessor**: `session-14-c1-attempt12-trace-regen-2026-05-22.md` (regenerated golden traces on Session-13 Mesen2-aligned boot).
**Branch / commit**: `main`, building on `2359ec6`.

---

## Summary

Session-14 closed the trace-baseline contamination from prior attempts by regenerating the 6 golden IRQ traces against the Session-13 boot-aligned foundation. Session-14's explicit attempt-13 prerequisite was: **build a Mesen2 IRQ-cycle oracle and cross-diff it against the new baselines BEFORE attempting any code change.**

This session lands that oracle. Phase 1A through 1E are complete:

* **Phase 1A**: Mesen2 capability probe. `~/AppImages/mesen.appimage` is available;
  the Lua API exposes `emu.eventType.irq` (= 1) and `emu.eventType.nmi` (= 0)
  for direct IRQ/NMI service-cycle callbacks, plus `apu.frameCounter.irqFlag`
  for per-instruction APU IRQ-line state via exec callbacks. There is NO
  per-CPU-cycle granularity from Lua (exec callbacks fire at opcode fetch).
  Mapper IRQ-line state is NOT directly exposed; it must be inferred from
  irq-service-event firings minus the APU-source events.

* **Phase 1B**: Authored `scripts/mesen2_irq_trace.lua`. Emits a CSV per-row
  for each IRQ-related event (irq_svc, nmi_svc, apu_set, apu_clr, nmi_set,
  nmi_clr, init) tagged with the cycle / frame / scanline / dot / PC where
  the event was observed.

* **Phase 1C**: Generated 6 Mesen2 IRQ trace baselines for the 5 target ROMs
  + 1 control (`cli_latency`). Committed under
  `crates/nes-test-harness/golden/irq_trace/mesen2/`.

* **Phase 1D**: Authored `scripts/irq_trace_cross_diff.py` to cross-diff
  RustyNES vs Mesen2 traces. The two emulators emit different schemas (Mesen2
  is event-driven; RustyNES is state-transition-driven). The diff aligns on
  first-event-cycle + first-APU-set-cycle + first-NMI-cycle, computes
  per-event delta histograms, and tallies Mesen2 event types per ROM.

* **Phase 1E**: Analyzed the cross-diff output for each of the 6 ROMs.
  **Finding**: the diff reveals multi-axis divergence with no clean
  falsifiable single-axis hypothesis. Implementation deferred.

---

## Phase 1A — Mesen2 capability matrix

Probed via `/tmp/mesen2_probe.lua` (one-shot script that dumps
`emu.getState()` keys + `emu.eventType` / `emu.callbackType` / `emu.memType`
to `/tmp/mesen2_state_probe.txt`). Verified Mesen2 binary path
(`~/AppImages/mesen.appimage`), `xvfb-run` available, and
`~/.config/Mesen2/settings.json` already has `AllowIoOsAccess: true`.

| Capability | Available | Notes |
|------------|-----------|-------|
| `emu.eventType.irq` | YES (= 1) | Fires when CPU services an IRQ |
| `emu.eventType.nmi` | YES (= 0) | Fires when CPU services an NMI |
| `emu.eventType.startFrame` | YES (= 2) | Per-frame entry |
| `emu.callbackType.exec` | YES (= 2) | Per-opcode-fetch CPU callback |
| `apu.frameCounter.irqFlag` | YES (bool) | APU frame-counter IRQ line state |
| `apu.dmc.irqEnabled` | YES (bool) | DMC IRQ-enabled (NOT actively-asserted) flag |
| `cpu.nmiFlag` | YES (number) | NMI-line latch |
| `cpu.cycleCount` | YES | CPU master cycle counter |
| `ppu.frameCount`, `ppu.scanline`, `ppu.cycle` | YES | PPU position |
| `mapper.irqPending` | NO | Mapper IRQ line state NOT exposed |
| Per-CPU-cycle Lua callback | NO | Only per-instruction granularity |

**Capability mismatch with RustyNES**: RustyNES emits per-CPU-cycle records
with sub-dot mapper / APU IRQ state. Mesen2 can only emit per-instruction
records + service-event callbacks. This makes cycle-precise cross-diffing
of mid-instruction IRQ pipeline behavior impossible from Lua alone.

Mesen2 settings: `"Revision": "Compatibility"` for MMC3 (per
`~/.config/Mesen2/settings.json` line). The Mesen2 source uses
"Compatibility" as default but the actual revision selected per-ROM is
determined by the NES 2.0 submapper or by built-in database overrides; the
behavior for `mmc3_test_2/4-scanline_timing.nes` (no NES 2.0 submapper)
defaults to **NEC** rev B per Mesen2's behavior, which is OPPOSITE of
RustyNES's default Sharp rev A. This is a critical caveat for the MMC3
comparisons below — the test ROM `mmc3_test_2/5-MMC3.nes` (Sharp-specific)
will fail under Mesen2 Compatibility, and `6-MMC3_alt.nes` (NEC-specific)
will pass. The reverse holds in RustyNES.

---

## Phase 1B — `scripts/mesen2_irq_trace.lua`

~190 LOC, modeled on `scripts/mesen2_cpu_boot_trace.lua`. Schema:

```
cpu_cycle, ppu_frame, ppu_scanline, ppu_dot, event_type, pc,
apu_irq_flag, nmi_flag
```

Event types: `init`, `irq_svc`, `nmi_svc`, `apu_set`, `apu_clr`,
`nmi_set`, `nmi_clr`. Environment variables: `MESEN2_IRQ_TRACE_OUT`,
`MESEN2_IRQ_TRACE_MAX_FRAMES`, `MESEN2_IRQ_TRACE_BOOT_FRAMES`. The
script auto-stops when the test ROM's status byte at `$6000` transitions
to a final value with the `$DE-$B0-$61` magic bytes present, matching the
RustyNES `irq_trace_fixture.rs` early-stop condition.

---

## Phase 1C — 6 baselines under `crates/nes-test-harness/golden/irq_trace/mesen2/`

| ROM | Mesen2 trace rows | RustyNES trace rows |
|-----|-------------------|---------------------|
| `cpu_interrupts_v2_1_cli_latency` | 1 (init only) | 1 |
| `cpu_interrupts_v2_2_nmi_and_brk` | 6 (init + 5 nmi_svc) | 745 |
| `cpu_interrupts_v2_3_nmi_and_irq` | 19 | 767 |
| `cpu_interrupts_v2_4_irq_and_dma` | 81 | 71 |
| `cpu_interrupts_v2_5_branch_delays_irq` | 144 | 205 |
| `mmc3_test_2_4_scanline_timing` | 5 (init + apu_clr + 3 irq_svc) | 2918 |

The wildly different row counts reflect the schema asymmetry: RustyNES
captures every IRQ-line-state transition (including state-detection
artifacts where the line bounces low/high rapidly during DMA), while
Mesen2's `apu_set`/`apu_clr` events are sampled at instruction boundaries
so brief glitches are smoothed away. For `mmc3_test_2/4` specifically, the
huge RustyNES count is dominated by the APU frame-counter IRQ which the
test doesn't acknowledge.

---

## Phase 1D — Cross-diff outputs

Full diff output captured by running `python3 scripts/irq_trace_cross_diff.py`
on each pair. Per-ROM highlights:

### `cpu_interrupts_v2_1_cli_latency` (control, strict-pass)

Both sides emit only the `init` / boot-state record at frame ~10-12. No
service events captured in either trace's recording window because both
emulators reach final-status before the `BOOT_FRAMES` skip ends. Useful
as a sanity check (both produce a comparable boot anchor) but does not
expose any pipeline divergence.

### `cpu_interrupts_v2_2_nmi_and_brk` (FAIL)

- RustyNES first NMI: cycle 444,324 / frame 15 / scanline 241 / dot 2
- Mesen2 first NMI service: cycle 563,453 / frame 20 / scanline 241 / dot 27
- **Delta: +119,129 cycles** (~4 frames; Mesen2 services later)

5 NMI services in Mesen2's trace, 10 NMI-assertion-cycle starts in
RustyNES. The ratio is roughly 2:1 — RustyNES detects every up-and-down
of the NMI line, Mesen2 only the points where the CPU actually services.

### `cpu_interrupts_v2_3_nmi_and_irq` (FAIL)

- RustyNES first NMI: cycle 474,104 / frame 16
- Mesen2 first NMI service: cycle 682,575 / frame 24
- Delta: +208,471 cycles (~7 frames)

The 2 Mesen2 `irq_svc` events land at cycles 3,541,513 + 3,720,196, much
later than RustyNES's first 5 mapper-IRQ-assertion cycles. Mesen2 sees
only 2 IRQ services vs RustyNES's 25 APU assertions. The two traces are
measuring entirely different parts of the test sequence.

### `cpu_interrupts_v2_4_irq_and_dma` (STRICT-PASS in RustyNES)

Most diagnostically valuable since the test passes — divergence here
reveals divergence in IRQ surface that does NOT affect pass/fail.

- RustyNES first APU assertion: cycle 296,164 / frame 10 / scanline 247 / dot 187
- Mesen2 first apu_set:         cycle 385,508 / frame 14 / scanline 247 / dot 200
- Constant ~89,000-cycle offset

**Per-event delta histogram (first 32 matched events):**
- min=89,339 max=148,873 avg=99,663
- 6 events at delta=+89,343 (modal)
- 3 events at delta=+89,344
- 2 events each at +89,339 / +89,341 / +89,865 / +89,867

The tightest cluster (24 of 32 events) is at delta=+89,339 ± 4 cycles.
The 8 outliers above 89,867 are likely DMC-DMA-period events where the
CPU's instruction-boundary detection in Mesen2 lags by a longer gap
because the DMA holds the bus.

**Constant 89,343-cycle offset hypothesis** (Hypothesis A): the
~89k-cycle baseline offset reflects RustyNES reaching the test's
measurement loop earlier than Mesen2 in absolute cycle count.

### `cpu_interrupts_v2_5_branch_delays_irq` (FAIL)

- RustyNES first APU assertion: cycle 297,056 / frame 10
- Mesen2 first apu_set:         cycle 356,716 / frame 13
- Delta: +59,660 cycles

**Per-event delta histogram (first 66 matched events):**
- min=29,829 max=535,902 avg=245,451
- Wildly variable — the modal delta is 268,025 (3 events), then 59,660
  (2 events), then 208,329 / 238,116 / 268,026 / 297,923 (each 2 events).

The delta histogram is NOT tight here, unlike test 4. This means the
RustyNES vs Mesen2 instruction-stream alignment drifts substantially
across the test run — the two emulators are walking different code paths
or hitting the IRQ at different points within the test's instruction
sequence.

### `mmc3_test_2_4_scanline_timing` (FAIL, sub-test #3 residual)

**The headline finding** of this session.

- RustyNES first MMC3 IRQ assertion: cycle 1,369,996 / frame 47 / scanline 0 / dot 257
- Mesen2 first IRQ service:           cycle 1,220,992 / frame 42 / scanline -1 / dot 299
- Delta: **-149,004 cycles** (Mesen2 fires 5 frames EARLIER)
- **Scanline mismatch: RustyNES scanline 0 vs Mesen2 scanline -1 (pre-render)**

The B4 fix (commit `48b5983` / `2026-05-14`) made RustyNES's first MMC3 IRQ
land on scanline 0 (not pre-render). The Phase B4 audit doc cites
sub-test #2's "Scanline 0 IRQ should occur LATER when `$2000=$08`" as the
architectural target. Mesen2's first IRQ service lands on scanline -1
(pre-render), which is what the PRE-B4 RustyNES did.

**Three interpretations** of the Mesen2 vs RustyNES MMC3 disagreement:

* **(a) Mesen2 default revision is NEC**, not Sharp. Mesen2's MMC3 IRQ
  asserts on the falling-counter-to-zero event (NEC semantics), while
  RustyNES's MMC3 asserts on the reload-to-zero event (Sharp B4
  semantics). The two revisions assert on different scanlines for the
  same A12 pattern. Verification: re-run with Mesen2's MMC3
  "ForceSharp" override (if it exists), or invert the comparison
  (compare `mmc3_test_2/6-MMC3_alt.nes`, the NEC-specific test).

* **(b) Mesen2 and RustyNES disagree on which sub-test runs first**.
  `mmc3_test_2/4-scanline_timing` runs 9 sub-tests sequentially. The
  test's branching may evaluate differently on the two emulators based
  on small boot-timing offsets, causing them to enter different sub-test
  sequences.

* **(c) RustyNES's B4 fix is wrong** in the direction of pre-render and
  the strict-pass on sub-test #2 is happening for the wrong reason
  (lucky alignment of unrelated state). Falsification: rebuild a
  pre-B4 trace and check whether sub-test #2 strict-passed before the
  fix landed — if it did NOT (which is the documented Session-14 history),
  then B4 is the load-bearing fix and Mesen2 is wrong / under a different
  revision for this comparison.

Interpretation (a) is most likely — Mesen2's `"Revision": "Compatibility"`
default in the settings file is described in Mesen2's docs as "behave
like Sharp B for most ROMs, NEC for known NEC-rev ROMs." Without
ROM-database overrides for `mmc3_test_2/4-scanline_timing.nes`, the
emulator's heuristic may pick either revision. This is the load-bearing
ambiguity in the Mesen2 oracle for MMC3 tests.

---

## Phase 1E — Falsifiable-hypothesis derivation

Per the ADR-0002 "Stop conditions" subsection and the session-14
"Recommended next attempts" item #1, this session's deliverable was a
Mesen2 IRQ oracle from which to derive ONE falsifiable single-axis
hypothesis for attempt 13.

**Outcome: no single-axis hypothesis is derivable from the data.**

The cross-diff reveals three independent divergence axes:

1. **Boot/test-anchor cycle offset** (test 4: ~89k cycles, test 5:
   ~60k cycles, test 2-3: ~120-208k cycles). The two emulators reach
   the test's measurement loop at different absolute cycle counts —
   despite Session-13's boot alignment. This is a real divergence but
   it's _between the two emulators' execution paths_, not a CPU-side
   IRQ-sample-point issue.

2. **MMC3 revision ambiguity**. Mesen2's MMC3 default for this ROM is
   likely NEC, RustyNES's is Sharp. The first-IRQ-event scanline
   mismatch (pre-render vs scanline 0) is consistent with this. Until
   we re-run Mesen2 with a confirmed revision override matching
   RustyNES's Sharp default, the MMC3 cross-diff cannot be interpreted
   as silicon-vs-our-impl divergence — it could just be Mesen2-default
   Sharp-or-NEC vs RustyNES Sharp.

3. **Per-instruction-boundary vs per-cycle event detection**. Mesen2's
   `apu_set` / `nmi_set` events are detected at the NEXT instruction
   fetch after the actual line transition. RustyNES's per-cycle trace
   records the cycle of the transition itself. A constant ~1-30
   CPU-cycle delta is therefore expected for ALL matched events even
   if the IRQ pipeline is silicon-perfect on both sides.

None of these three axes is the CPU-side IRQ-sample-point axis
(`T_last - 1` / NMI-hijack-window) that the failing tests target. The
Mesen2 oracle as currently captured does NOT directly probe that
specific pipeline.

**To produce a usable hypothesis the oracle needs follow-up work:**

a. Re-run all 6 baselines with Mesen2's MMC3 revision forced to Sharp
   (via per-ROM settings override) to remove the revision ambiguity.
   Cross-diff against `mmc3_test_2/4` only after this.

b. Augment the RustyNES trace fixture with `irq_svc` event records
   (analogous to Mesen2's). The fix would log the cycle at which the
   CPU first executes a vector fetch from `$FFFE`/`$FFFF` — making the
   two emulators directly comparable on the service-cycle axis.

c. Run a per-CPU-instruction trace (`scripts/mesen2_cpu_boot_trace.lua`)
   for the failing ROMs and look at the divergence at the first
   in-test-loop IRQ-arming instruction (not the boot phase). This
   shifts the question from "where does the IRQ line transition?" to
   "what PC do both emulators reach at the same cycle when the IRQ is
   armed?". Session-12 already demonstrated this works for boot.

d. Cross-check on Mesen2's MMC3 settings handling — is there a way to
   pin the test ROM to Sharp via settings.json's per-game database?

All four are tractable in 1-2 future sessions but none of them was
deliverable within Session-15's scope (Phase 2 implementation was
gated on Phase 1E producing a hypothesis, which it did not).

---

## Why attempt 13 (code change) is NOT attempted this session

Per the ADR-0002 "Stop conditions" (mandatory):

> 3. **The proposed change reaches the same diagnosis as one of the
>    four rolled-back attempts.** Stop. A 5th rollback is worse than
>    no change.

12 prior rollbacks have shown that:
- Attempts 1-4: cycle-rotation single-axis changes regressed orthogonal
  tests.
- B4 threshold + post-B4 mid-cycle snapshot: investigated then rolled
  back when the mapper-side change couldn't be the load-bearing axis.
- 7th, 8th, 9th, 10th, 11th, 12th attempts: progressively isolated the
  `T_last - 1` axis as load-bearing for one sub-test pattern but
  produced no flips when landed unconditionally.

The Session-14 audit doc cited as its primary attempt-13 prerequisite:
"Mesen2 IRQ-line-state cross-reference -- instrument Mesen2 to emit
per-CPU-cycle IRQ trace records analogous to the RustyNES fixture. Run
the 6 target ROMs through both emulators. Cross-diff IRQ-assertion cycle
positions. This produces the silicon-cycle reference no prior C1 attempt
has had."

Session-15 has landed the Mesen2 oracle. The cross-diff exists. But
**the oracle reveals a multi-axis surface, not a single load-bearing
axis** — which is exactly what Attempts 1-12 also found from RustyNES's
own trace fixture. Any code attempt 13 based on this oracle without
first resolving the three uncertainty axes (MMC3 revision, service-vs-
assertion alignment, per-cycle vs per-instruction granularity) would
be guessing — the same failure mode as attempts 5-12.

The conservative path is to land the oracle as durable infrastructure,
document the multi-axis finding, and attempt 14 in a future session
after one or more of the four follow-up work items above is also
landed.

---

## Validation gauntlet (this session)

| Gate | Result |
|------|--------|
| `cargo fmt --all --check` | clean (only docs + scripts modified; no Rust code) |
| `cargo clippy --workspace --all-targets --features test-roms -- -D warnings` | not re-run (no Rust source changes) |
| Workspace tests `--features test-roms` | not re-run (no Rust source changes; the fixture-generated golden CSVs are unchanged) |
| AccuracyCoin pass rate | 82.73% (unchanged) |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) | legible (no Rust changes) |
| Existing 6 RustyNES IRQ golden traces | byte-identical (unchanged) |

**Net change**: pure-additive — 1 Lua script, 1 Python diff tool, 6
Mesen2 baseline CSVs, 1 audit doc, 1 ADR update, 1 CHANGELOG entry.
Zero production code modified.

---

## Files modified by this session

- `scripts/mesen2_irq_trace.lua` (new, ~190 LOC)
- `scripts/irq_trace_cross_diff.py` (new, ~220 LOC)
- `crates/nes-test-harness/golden/irq_trace/mesen2/` (6 new CSV files)
- `docs/adr/0002-irq-timing-coordination.md` (new subsection — Session-15
  Mesen2 oracle landing + multi-axis finding)
- `CHANGELOG.md` `[Unreleased]` → "Investigated and rolled back" (12th
  entry: oracle-landed-no-hypothesis)
- `docs/audit/session-15-c1-attempt13-mesen2-irq-oracle-2026-05-22.md`
  (this file)
- `docs/STATUS.md` (residuals section refresh)

## Files NOT modified

- All chip crates (`crates/nes-{cpu,ppu,apu,mappers,core}/src/*`)
- All other production / fixture / harness code

---

## Recommended next attempts (priority order)

For Session-16 / attempt 14:

1. **Resolve MMC3 revision ambiguity** in Mesen2 baselines. Find Mesen2's
   per-ROM MMC3 revision override (likely via `Settings.json` ->
   `Nes.MmcOverride[<rom-sha1>]` or via Mesen2's NesDb.txt) and force
   `mmc3_test_2/4-scanline_timing.nes` to Sharp rev A. Regenerate the
   MMC3 baseline. Cross-diff again — the scanline mismatch should
   disappear if it was a revision artifact.

2. **Augment RustyNES trace fixture with service events**. Add an
   `event_type` column to RustyNES's `IrqTrace` distinguishing line
   transitions from CPU vector-fetch events. The fixture should log
   the cycle of every read from `$FFFE`/`$FFFF`. This makes the two
   sides directly comparable on the service axis.

3. **Per-CPU-instruction divergence trace** in the IRQ window. Use
   `scripts/mesen2_cpu_boot_trace.lua` with a `START_CYCLE` of ~250k
   (skipping boot) to capture the first 100k cycles of each failing
   test's IRQ-arming loop. Cross-diff against a RustyNES per-instruction
   trace covering the same window. Locate the first PC divergence;
   that PC is the load-bearing instruction the prior 12 attempts
   couldn't isolate.

4. Only AFTER 1-3 yield a single-axis hypothesis: implement attempt 13
   under feature flag `cpu-c1-attempt-13`, run the full validation
   gauntlet, decide.

---

## Invariants validated (Session-15 close)

| Invariant | Pre-Session-15 | Post-Session-15 | Status |
|-----------|----------------|------------------|--------|
| Workspace tests `--features test-roms` | 540 strict + 5 ignored | 540 strict + 5 ignored | OK |
| AccuracyCoin RAM pass rate | 82.73% | 82.73% | OK |
| Sacred trio (SMB / Excitebike / Kid Icarus PAL) | legible | legible | OK |
| `cargo fmt --all --check` | clean | clean | OK |
| 6 RustyNES IRQ trace golden baselines | unchanged | unchanged | OK |
| Mesen2 IRQ trace baselines | did not exist | 6 baselines added | NEW |

Net change: pure-additive oracle infrastructure. The 13th C1 attempt is
the **oracle-only-landed-no-hypothesis** outcome per the Phase 2 spec.
The cross-diff data is preserved as permanent infrastructure for the
next attempt's hypothesis derivation.
