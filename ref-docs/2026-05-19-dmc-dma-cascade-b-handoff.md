# Cascade B DMC DMA Handoff

**Date:** 2026-05-19  
**Audience:** Claude Code or another follow-up coding agent  
**Scope:** DMC DMA halt-cycle precision, AccuracyCoin APU Registers and DMA suite  
**Outcome:** AccuracyCoin RAM result for `APU Registers and DMA tests` is now `10 pass | 0 fail`.

## Why This Work Was Needed

The original target was Cascade B: DMC DMA halt-cycle precision in
`crates/rustynes-core/src/bus.rs:service_dmc_dma`, with `dmc_dma_during_read4` as
the regression guard. A previous "more accurate" local DMC patch had exposed
broader CPU/APU timing coupling, so the final fix was not limited to one DMA
service routine. The scheduler needed enough state to distinguish DMC load
DMA, reload DMA, one-cycle abort halts, OAM-overlap cases, and the DMC
readout bug side effects.

Cascade A, Sprite Zero Hit cycle precision, was discussed earlier but was not
remediated in this DMC pass. The worktree still has unrelated changes in
`crates/rustynes-ppu/src/ppu.rs`; review those separately before attributing them
to the DMC fix.

## External References Used

- NESdev DMA: https://www.nesdev.org/wiki/DMA
- NESdev APU DMC: https://www.nesdev.org/wiki/APU_DMC
- AccuracyCoin: https://github.com/100thCoin/AccuracyCoin

The important hardware constraints from NESdev were:

- DMC DMA uses halt, dummy, optional alignment, and get/read cycles.
- CPU halts only succeed on read cycles.
- DMC DMA can collide with OAM DMA and preempt/realign the OAM transfer.
- NTSC 2A03 DMA no-op cycles can repeat the halted CPU read, causing side
  effects on `$2007`, `$4015`, `$4016`, and `$4017`; PAL 2A07 avoids this bug.
- DMC load DMA after `$4015` enable and reload DMA after the sample buffer
  empties do not have identical scheduling.

## Files Changed

### `crates/rustynes-apu/src/apu.rs`

The APU now owns explicit DMC DMA scheduler state:

- `dmc_dma_is_load`: distinguishes initial load DMA after `$4015` enable from
  reload DMA raised by the DMC output unit.
- `dmc_dma_short`: lets race-like load/stop cases use the short no-op service
  path without changing ordinary load cadence.
- `defer_dmc_reload_once`: suppresses the immediate same-tick reload request
  after a byte is delivered before the get-cycle APU tick.
- `pending_dmc_abort` and `dmc_abort_delay`: model the one-cycle stop-near-
  reload abort halt.
- `dmc_dma_cooldown`: prevents a too-soon second reload request after a DMA
  get.
- `dmc_reload_suppress_outputs`: latches the one-byte looping edge where a
  reload request is lost until a later `$4015` enable/disable write re-arms
  the path.
- `dmc_dma_delay`: schedules delayed load DMA requests after `$4015` enable.

Important behavior:

- `write_status` now schedules DMC load DMA with an APU-phase-dependent delay:
  `4` cycles when `apu_phase` is true, `3` when false. The narrow
  `implicit_stop_edge` case subtracts one cycle instead of globally shifting
  all `$4015` load DMA timing.
- `tick_with_external` decrements load delay, abort delay, and cooldown before
  the APU timer clocks, then raises reload DMA only when no suppress/defer/
  cooldown latch blocks it.
- `complete_dmc_dma` schedules implicit one-byte non-loop aborts when the next
  output clock is 2 or 3 CPU cycles away, clears the pending DMA, and applies
  a cooldown of `4`.
- `complete_dmc_dma_before_get_tick` handles the looped one-byte edge where
  the fetched byte must be visible before the get-cycle APU tick. It applies
  a cooldown of `5` and defers the immediate reload once.
- `schedule_explicit_dmc_abort_if_needed` handles `$4015` disable near the
  final output bit, including the immediate one-cycle abort case.
- `clear_frame_irq_immediate_for_dma` exists because DMA no-op reads of
  `$4015` need immediate side effects, while normal CPU-visible `$4015` reads
  still use the deferred frame-IRQ clear behavior needed by timing tests.

Do not casually change the cooldown values (`4` and `5`) or the non-loop
abort window (`2..=3` CPU cycles). During troubleshooting, nearby values made
individual AccuracyCoin DMC rows move in opposite directions.

### `crates/rustynes-core/src/bus.rs`

The bus now models DMC DMA as a scheduler interaction rather than a fixed
four-cycle stall.

Key changes:

- `drain_dma` accepts `read_addr: Option<u16>`. OAM DMA starts only on CPU
  read cycles. DMC DMA is serviced only when the CPU is on a readable bus
  cycle; a pending abort disappears on write cycles instead of retrying.
- `dma_halt_addr` records the CPU read address held while OAM DMA owns the
  bus, so DMC DMA during OAM can replay the correct no-op side effects.
- `deferred_dma_replay_addr` catches absolute-address register reads where
  the high-byte operand was halted one read before the side-effect register
  access.
- `service_dmc_dma` uses `2` no-op cycles for the short path and `3` for the
  normal path, then performs one get/read cycle.
- `service_dmc_dma_during_oam` overlaps DMC halt/dummy/alignment no-op cycles
  with OAM DMA when possible. The DMC get owns the memory read cycle and can
  skip an OAM slot, forcing OAM to realign.
- `service_dmc_abort` performs the one no-op read and completes the abort
  without fetching a DMC byte.
- `replay_dma_noop_read` reproduces NTSC readout-bug side effects for
  `$2002`, `$2007`, `$4015`, `$4016`, and `$4017`; PAL returns early.
- `dmc_dma_read` models the register-conflict path where the halted CPU read
  contributes address bits 15..=5 while the DMC DMA supplies low bits 4..=0.
  This matters for controller/status behavior around `$4015-$4017`.

### `crates/rustynes-apu/src/snapshot.rs`

APU snapshots append the new DMC scheduler latches after the existing APU
state. Restore treats them as optional tail bytes, so older version-1 states
remain readable:

- `dmc_dma_delay`
- `dmc_dma_is_load`
- `pending_dmc_abort`
- `dmc_abort_delay`
- `dmc_dma_short`
- `defer_dmc_reload_once`
- `dmc_dma_cooldown`
- `dmc_reload_suppress_outputs`

### `crates/rustynes-core/src/bus_snapshot.rs`

BUS snapshots now include:

- `dma_halt_addr`
- `deferred_dma_replay_addr`

Decode reads both as optional tail fields to preserve compatibility with older
BUS blobs.

## Troubleshooting Notes

Temporary diagnostics were used and then removed before final verification:

- A temporary AccuracyCoin probe example under
  `crates/rustynes-test-harness/examples/accuracycoin_probe.rs`.
- A temporary unconditional `extern crate std;` in `crates/rustynes-core/src/lib.rs`.
- Temporary `trace_dmc`, `trace_implicit_window`, and `dmc_trace_state` hooks.

The final probe before cleanup showed the previously failing `Implicit DMA
Abort` row returning `PassWithCode(2)`, matching the expected pre-1990 CPU
classification. After cleanup, the aggregate AccuracyCoin RAM result reports
five `pass_with_code` rows total and no failures in the APU Registers and DMA
suite.

The main local finding was that treating DMC service as a fixed stall was the
wrong abstraction. Correcting only `service_dmc_dma` moved some rows but
regressed others. The stable fix required coupling the APU-side request timing
with the bus-side halt/read-cycle model.

## Verification Performed

All commands were run with `env -u RUSTC_WRAPPER`.

```sh
cargo fmt --all --check
cargo clippy -p rustynes-apu -p rustynes-core -p rustynes-test-harness --all-targets --features test-roms -- -D warnings
cargo test -p rustynes-apu dmc -- --nocapture
cargo test -p rustynes-test-harness --features test-roms dmc_ -- --nocapture
cargo test -p rustynes-test-harness --features test-roms save_state -- --nocapture
cargo test -p rustynes-test-harness --features test-roms accuracycoin_pass_rate_meets_floor -- --nocapture
```

Observed results:

- `rustynes-apu dmc`: 12 passed.
- `dmc_` ROM filter: `apu_test/7-dmc_basics`,
  `apu_test/8-dmc_rates`, and all five `dmc_dma_during_read4` harness cases
  passed.
- Save-state filter: 2 passed.
- Clippy on touched crates passed with `-D warnings`.
- AccuracyCoin RAM summary:
  - `total=144`
  - `pass=103`
  - `pass_with_code=5`
  - `fail=31`
  - `not_run=5`
  - RAM pass rate `77.70%`
  - `APU Registers and DMA tests | 10 pass | 0 fail | 0 not_run | 0 skipped`

## Remaining Work Not Solved Here

AccuracyCoin still reports 31 failing RAM tests outside the fixed DMC DMA
suite. The visible remaining categories include CPU open bus, unofficial SH*
opcodes, CPU interrupt overlap/latency, general APU tests, PPU rendering
behavior, sprite evaluation, and PPU miscellaneous timing. These are unrelated
to the completed Cascade B DMC DMA flip.

Cascade A Sprite Zero Hit cycle precision was not completed in this pass.
There are existing local modifications in `crates/rustynes-ppu/src/ppu.rs`; inspect
and validate them separately before continuing Sprite Zero work.

The worktree also contains untracked `AGENTS.md` and `GEMINI.md` files. They
were not part of the DMC DMA code remediation.

## Guidance For Future Agents

- Treat DMC timing changes as cross-module scheduler changes. Check both
  `rustynes-apu` and `rustynes-core` before adjusting behavior.
- Re-run the AccuracyCoin aggregate after any DMC timing change. Small local
  unit tests can pass while one AccuracyCoin row regresses.
- Keep snapshot compatibility in mind when adding scheduler fields. This
  repository currently appends optional tail fields for version-1 APU/BUS
  snapshot compatibility.
- Preserve PAL exclusions for DMC readout-bug behavior unless there is a new
  PAL-specific test proving otherwise.
- Do not restore the temporary tracing/probe code unless actively debugging;
  if restored, remove it before finalizing.
