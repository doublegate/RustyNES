# Path β — Paired oracle capture session (Sprint 2.3 Step 3 progress)

**Date:** 2026-05-25 (immediately after the
`path-beta-dmc-trace-tooling-2026-05-25.md` foundation work).
**Outcome:** Tooling validated end-to-end on a real cascade-sentinel
surface; structural blockers in Mesen2-side timing semantics
identified; the v1.1.0 release-notes-overwrite CI bug root-caused
+ fixed in the same session. **Multi-axis DMC recalibration NOT
landed — blocked on cycle-precise Mesen2 trace symmetry.**

---

## Empirical findings (concrete data points)

### Finding 1 — Cascade is deterministically reproducible

Running RustyNES against the full `AccuracyCoin.nes` battery
(START-pressed at frame 306, traced from frame 1406 onward at 9 M-
cycle buffer):

| Build | `$0478` final | Encoding |
|---|---|---|
| baseline (no feature) | `0x09` | PASS |
| `--features cpu-implied-dummy-reads` | `0x0A` | FAIL (error code 2) |

Both reach `$0478 = non-zero` at frame 214 from trace start
(absolute frame ~1620). The cascade is byte-deterministic — same
result address, same frame, different value.

### Finding 2 — Sub-test ROMs DO NOT reproduce the cascade

`tests/roms/AccuracyCoin/sub-tests/implied-dummy-reads.nes` produces
`final=0x0E` (FAIL #3) on BOTH baseline AND `cpu-implied-dummy-
reads` ON. The cascade-source test (`Implicit DMA Abort`) is
exclusively in the full battery — it cannot be isolated as a
standalone sub-test ROM. **Full battery trace pairing is the only
viable path forward.**

### Finding 3 — Geometric divergence between Mesen2 and RustyNES on the canonical `$4015=$10`

At the canonical DMC-enable + IRQ-enable signal (the test's setup
write), both emulators write to `$4015` with value `$10` at the
same vertical position (PPU scanline 241) but at different
horizontal positions:

| Side | CPU cycle | Frame | Scanline | Dot |
|---|---|---|---|---|
| RustyNES baseline | 46931957 | 1576 | 241 | **286** |
| Mesen2 (frame-cnt 14 in Lua) | 384825 | 14 | 241 | **193** |

Both reach `sl=241` consistently. **Dot delta: 93 PPU dots
= 31 CPU cycles.** Mesen2 reaches the `STA $4015` ~31 CPU cycles
earlier in the same scanline.

This is **NOT** a DMC scheduler issue — it's a structural
CPU/PPU phase alignment mismatch between the two emulators on a
test that pulls in 1.4 M cycles of bootstrap before the test
phase. The 4 compensating delays the cross-diff is supposed to
measure are SUBSEQUENT to this geometric offset and would have
to be measured by aligning at the `$10` write, then comparing
DMC fetch deltas downstream.

### Finding 4 — Mesen2 START-press driver is unreliable in this AppImage

The auto-start logic (`emu.setInput({start = true}, 0, 0)`
inside `inputPolled`) does fire BUT Mesen2's `ppu.frameCount`
reaches only 14-74 within several thousand Lua-side ticks. The
ROM stays in its title-screen / menu phase and never reaches the
`Implicit DMA Abort` test. Comparing:

| Side | First `$4015=$10` |
|---|---|
| RustyNES (START at frame 306) | frame 1576 |
| Mesen2 (autostart fires at lua frame 306) | frame 14 (boot-init DMC config, NOT the test) |

Mesen2's `$4015=$10` at frame 14 is a boot-time DMC init pattern,
not the test trigger. Mesen2 doesn't progress past the title
screen — either `emu.setInput` is silently ignored, the API has
moved, or `--testRunner` mode disables input injection.

**This is the load-bearing blocker for the cycle-precise oracle
diff.** Without Mesen2 actually executing the battery to the
`Implicit DMA Abort` test, we cannot capture a matched cycle-
sequence for the 4 compensating delays.

### Finding 5 — RustyNES + Mesen2 have systematic `cpu.cycleCount` semantics divergence

For the same geometric position (`sl=241 dot=193`, value `$10`):

* RustyNES: `cpu_cycle = 46_931_957` (CPU cycles since reset)
* Mesen2:   `cpu.cycleCount = 384_825`

The 122× ratio difference is too large to be drift — Mesen2's
`cpu.cycleCount` field exposed via Lua `emu.getState()` must
count something different (master clocks not counted during DMA
halts? CPU instructions executed? something else). The
cross-diff aligned the offset successfully (-46.5 M cycles) so
this isn't a fatal blocker, but it does mean we cannot infer
absolute cycle deltas across emulators — only relative deltas
WITHIN each emulator's own clock, then compared via the offset.

---

## Tooling improvements this session

### `trace_dmc_dma.rs` extensions

- `--battery` flag: presses START at frame 306 for 6 frames
  (matches `accuracy_coin::run_battery_capturing_ram` protocol)
- `--start-frame N` flag: runs N frames without tracing, then
  enables `irq-timing-trace`. Lets us target a specific test
  phase without the 9 M-cycle buffer filling on boot.
- `--buffer-cycles N` flag: configurable trace buffer size
- Tight result-stop: now breaks immediately on first
  `$RESULT_ADDR != 0` (matches Mesen2 Lua's stop-on-result
  semantics so row counts are comparable across emulators)

### `mesen2_dmc_dma_trace.lua` extensions

- `MESEN2_DMC_TRACE_AUTOSTART_FRAME` env var: AccuracyCoin
  START-press driver
- `MESEN2_DMC_TRACE_AUTOSTART_PRESS_FRAMES`: press duration
- `MESEN2_DMC_TRACE_START_FRAME`: skip events before this PPU
  frame (avoids boot + menu noise in the CSV)
- Tightened `dmc_get` detection: only emits when `bytes_rem`
  decrements by exactly 1 AND `prev_bytes_rem > 0` (filters out
  spurious "DMC disabled" decrements that the v1 detection
  over-emitted; previous capture had 33 spurious events)

### `dmc_dma_trace_cross_diff.py` extensions

- `--align-value` flag: align on the first `$4015 W` whose
  `bus_data == value` (canonical `$10` enable; lets us skip
  early `$00` disables that don't map to the same logical event)
- Rusty DMC-fetch filter narrowed: rising-edge of
  `dmc_pending_post`. Previous filter (`access == "r" and
  bus_addr >= 0x8000`) included OAM-DMA bursts that share the
  `DmaRead` access type. New filter correctly distinguishes
  DMC-DMA from OAM-DMA.

### `scan_dma_abort.rs` (NEW)

Helper that runs the full AccuracyCoin battery and reports the
exact frame each DMA-test result address gets set. Output for the
v1.1.0 baseline:

| Test | Frame | Result |
|---|---|---|
| DMA + Open Bus | 1577 | 0x01 |
| Implicit DMA Abort | **1620** | 0x09 (PASS) |
| Implied Dummy Reads | 4926 | 0x0E (FAIL) |
| APU Reg Activation | 1735 | 0x1A |

The Implicit DMA Abort cascade-sentinel runs at absolute frame
1620 — a tight ~200-frame window that fits in a 9 M-cycle trace
buffer when skipped to with `--start-frame 1100`.

### `dump_battery_ram.rs` (NEW)

Helper that runs `accuracy_coin::run_battery_capturing_ram` and
dumps the post-battery RAM bytes for the DMA-test address range.
Used to cross-check the catalog addresses against actual RAM
state when the scanner produces unexpected results.

---

## Out-of-band: v1.1.0 release-notes CI overwrite root-caused + fixed

Mid-session the user reported that the v1.1.0 GitHub release body
showed only the 582-char CI placeholder, not the 14 KB
comprehensive notes I authored at tag time. This is the SAME
failure mode as v1.0.0 — the established 5-step release protocol
documents the post-tag verification but the actual root cause was
in `.github/workflows/release.yml`:

```yaml
- uses: softprops/action-gh-release@v3
  with:
    body: |  # <-- OVERWRITES release body on every matrix invocation
      RustyNES v2 ${{ steps.tag.outputs.tag }} pre-built binaries.
      ...
    generate_release_notes: true  # <-- ALSO overwrites
```

`softprops/action-gh-release` runs once per matrix target (Linux +
macOS-x86 + macOS-arm + Windows = 4 invocations) and each
overwrites the release body. Even after a manual `gh release edit
--notes-file ...` post the maintainer-authored body gets clobbered
~15-20 minutes later when the matrix completes.

**Fix landed this session** (working-tree change): removed both
`body:` and `generate_release_notes: true` from the workflow so
CI only attaches artifacts. The release body is owned by the
maintainer's manual `gh release edit` and the workflow no longer
touches it.

**Manual re-application of v1.1.0 notes**: posted via
`gh release edit v1.1.0 --notes-file /tmp/RustyNES_v2/release-
notes-v1.1.0.md`. Verified: body length now 14398 chars.

**Protocol memory updated**: `feedback_release_protocol.md` now
documents the CI-overwrite root cause + the structural fix + a
re-verify-after-15-min requirement for any future maintainer who
might re-add the `body:` field.

---

## What blocks closing Sprint 2.3 Step 3

In priority order:

1. **Mesen2 START-press fix** — without the battery actually
   running in Mesen2, we cannot capture a matched cycle-sequence
   for the `Implicit DMA Abort` test. Options:
   - Investigate why `emu.setInput` doesn't progress past title
     screen in this Mesen2 build (API change? testRunner mode
     restriction? need different field name?)
   - Use Mesen2's command-line `--input` flag (if it exists) to
     hardcode START as held during early boot
   - Save-state-based: load an AccuracyCoin save state captured
     at the moment the battery is running, skip the START step
     entirely
2. **CPU-cycle semantics reconciliation** — even with Mesen2
   running the battery, the 122× cycle-count ratio between the
   two emulators means we need a "PPU dot + scanline + frame"
   alignment rather than a "CPU cycle" alignment. The cross-diff
   should optionally pair events by `(ppu_frame, ppu_scanline,
   ppu_dot)` tuple rather than cycle, for cross-emulator
   comparisons. (Same-emulator before-vs-after IS cycle-precise.)
3. **Multi-axis correlation** — once we have matched cycle-
   sequences, we can compare RustyNES (with `cpu-implied-dummy-
   reads` ON) vs Mesen2 on the failing test fetch-by-fetch. The
   4 compensating delays (`dmc_dma_short`, `dmc_dma_cooldown`,
   `dmc_abort_delay_for`, `dmc_dma_pending`) each shift the
   fetch cycle by ±1; the cross-diff's signed delta column maps
   directly to the axis to adjust. THIS is the multi-day work
   Session-20 and prior audits anticipated.

---

## Workspace state at end of this session

* Tests: 537 strict pass + 5 ignored across 34 suites with
  `--features test-roms`. **PRESERVED.**
* AccuracyCoin: **90.65% (126/139) PRESERVED.**
* 60-ROM commercial-ROM oracle: **60/60 PRESERVED** (no
  production code change).
* Sacred trio: **PRESERVED.**
* B4 invariant: **PRESERVED.**
* v1.1.0 GitHub release body: **RE-POSTED**, length 14398 chars
  (was 582 placeholder).
* New artifacts:
  - `crates/nes-test-harness/src/bin/trace_dmc_dma.rs`
    (extended with `--battery`, `--start-frame`,
    `--buffer-cycles`, tight result-stop)
  - `crates/nes-test-harness/src/bin/scan_dma_abort.rs` (NEW)
  - `crates/nes-test-harness/src/bin/dump_battery_ram.rs` (NEW)
  - `scripts/mesen2_dmc_dma_trace.lua` (extended with
    autostart + start-frame + tightened dmc_get detection)
  - `scripts/dmc_dma_trace_cross_diff.py` (extended with
    `--align-value`; DMC-fetch filter rewritten to rising-edge
    of `dmc_pending_post`)
  - `.github/workflows/release.yml` — `body:` + `generate_
    release_notes:` removed from `softprops/action-gh-release`
  - this audit doc
* Memory updates:
  - `feedback_release_protocol.md`: CI overwrite root-cause +
    structural fix documented

---

## Cross-references

* Trace-tooling foundation (previous session):
  `path-beta-dmc-trace-tooling-2026-05-25.md`
* Sprint 2.3 Step 3 iter 1+2 (single-axis insufficiency):
  `sprint-2.3-step-3-iter-1-2-cooldown-empirical-2026-05-25.md`
* Sprint 2.3 recon: `sprint-2.3-implied-dummy-dmc-recon-2026-05-
  25.md`
* Session-20 (origin of the 4-compensating-delay diagnosis):
  `session-20-sprint1-dmc-abort-investigation-2026-05-22.md`
* Session-29 (Mesen2 master-clock semantics finding):
  `session-29-c1-axis-final-conclusion-2026-05-23.md`
* Existing-known-good Mesen2 trace template (whose
  `inputPolled` driver this session's Lua copied):
  `scripts/mesen2_irq_trace.lua` lines 469-489
* v2.0.0 release plan:
  `/home/parobek/.claude/plans/generate-a-new-plan-snug-
  starlight.md`
