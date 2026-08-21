# MiSTer FPGA and SuperStation One - the co-simulation boundary

**Spec, not history.** Update this file in the same change as any behaviour
change to `crates/rustynes-cosim` or the golden formats it emits.

**Decision record:** [ADR 0037](adr/0037-mister-fpga-core-independent-hdl-implementation.md).
**Execution plan:** [`to-dos/plans/v2.5.0-fabric-plan.md`](../to-dos/plans/v2.5.0-fabric-plan.md).
**Research archive:** [`to-dos/plans/research/v2.5.0-research-mister-fpga.md`](../to-dos/plans/research/v2.5.0-research-mister-fpga.md).

---

## What this is, and what it is not

RustyNES is **not** being ported to FPGA. A MiSTer core is SystemVerilog compiled
by Quartus 17.0.2 into a Cyclone V bitstream; Rust does not become a bitstream.

What the "Fabric" line builds is a **new NES implementation in SystemVerilog,
written from public hardware documentation**, in a sibling repository
(`RustyNES_MiSTer`), with **RustyNES as its verification oracle**. This document
specifies the boundary between the two - the one part that lives in this
repository.

## The firewall applies to HDL

`NES_MiSTer` and `fpganes` `rtl/` are **strict black boxes**: never opened, read,
quoted or transcribed. This is the same rule
`docs/ai-emulator-provenance-guardrails.md` states for emulator source, extended
to hardware description.

- **Permitted:** instantiating a third-party core as an opaque testbench module
  and comparing its *outputs* against ours.
- **Not permitted:** reading its source, constants, tables, identifier names or
  comments - not "for reference", not once.
- **Anything unimplementable from documentation escalates to a new ADR before any
  source is opened.**

Enforcement is mechanical rather than dispositional: the repositories stay
physically outside the workspace, and CI carries a path denylist plus an
identifier grep - the same shape as the `/ref-proj/` guard.

## `crates/rustynes-cosim`

A pure wrapper over `rustynes-core`. It adds **no** core API and changes **no**
core behaviour; it is additive, absent from the default build, and its presence
cannot move AccuracyCoin or nestest.

### It is excluded from the workspace, deliberately

`rustynes-cosim` is in the root manifest's `[workspace] exclude`, not its
`members`. That is not tidiness — it is the only mechanism that makes the
isolation real.

The crate enables `cpu-boot-trace` and `irq-timing-trace` on `rustynes-core`, and
**cargo unifies features across a workspace build**. As a member, it made
`cargo build --workspace` compile the core once with the union, so every
workspace build linked the **instrumented** per-cycle PPU tick loop —
`irq-timing-trace` selects a different `for sub_dot in 0..3` loop, not an inert
branch. CI's accuracy battery is `cargo test --workspace --release --features
test-roms`, so it was validating a scheduler no user runs.

The measured cost was +1.2% to +1.9% on `full_frame`, *below* this project's 3%
adoption bar. Stated precisely because it shows the fix was never about speed: a
gate pointed at the wrong code path is wrong at any percentage.

**What exclusion costs, and how each cost is closed:**

| Cost | Closed by |
|---|---|
| Cannot use `field.workspace = true`; version/edition/license/lints duplicated | `cosim_manifest_audit.rs` asserts every duplicated value equals the workspace's |
| Someone re-adds it to `members` | the same audit asserts it is still in `exclude` |
| `cargo fmt --all` / `clippy --workspace` / `test --workspace` do not reach it | explicit `fmt`, `clippy` and `test` steps in `ci.yml` |
| Its dependencies leave the workspace `cargo deny` graph | today only `sha2`, already in the graph; re-check if that changes |

The clippy step justified itself immediately, reporting a `must_use_candidate`
that `cargo clippy --workspace` had never surfaced.

### Crate types

`["rlib", "staticlib", "cdylib"]` - `rlib` so the golden-export binary and the
crate's own tests use the safe Rust API directly, `staticlib`/`cdylib` so a
Verilator C++ testbench links the same code through the C ABI.

### The trace features are mandatory

`Cargo.toml` enables `cpu-boot-trace` and `irq-timing-trace` unconditionally
rather than re-exposing them as this crate's own optional features.

A build without them would compile, link, run, and export **empty** goldens - an
absence of signal that reads exactly like agreement. Making them mandatory turns
that into a compile error.

A side effect worth recording: **no CI invocation previously enabled either
feature for clippy**, so `cpu_boot_trace.rs` and `irq_trace.rs` had never passed
the lint gate. Adding this crate surfaced six pre-existing findings in them.

### The power-on frame latch

The PPU is constructed at dot 340 of the pre-render line, so the 7-cycle reset
sequence ticks past the frame wrap and leaves `frame_complete` latched. **The
first `run_frame()` after construction therefore consumes that latch and returns
without stepping a single cycle** - measured, not inferred: frame 0 advances the
cycle counter by 0, frames 1..3 by ~29,780 each.

Every other caller in the workspace runs thousands of frames, so one lost frame is
invisible to them. It is not invisible to a golden export: a bare
`for _ in 0..n { run_frame() }` would emit an (n-1)-frame golden under a manifest
claiming n.

`Oracle::advance_frames(n)` therefore gates on the **frame counter**, not the call
count, and bails out on a jammed CPU. The behaviour is pinned by
`the_first_run_frame_after_power_on_advances_nothing`, so a future core change
that removes the quirk fails a test that names it rather than silently altering
every golden's length.

## Golden formats

`nes_golden_export --rom <path> --out <dir>` writes, under `<dir>`:

| File | Format | Consumed by |
|---|---|---|
| `<stem>.boot.bin` | `CpuBootTrace` binary - 12-byte magic `RUSTYNES_CPU`, schema version, packed records | `cpu_boot_trace_diff` |
| `<stem>.irq.csv` | per-CPU-cycle IRQ/bus CSV, two samples per cycle | `scripts/irq_trace_cross_diff.py` |
| `<stem>.index_fb.bin` | 256x240 little-endian `u16`, **pre-palette** | the testbench's frame comparison |
| `<stem>.ram.bin` | 2 KiB CPU work RAM | `accuracy_coin_catalog::decode_results` |
| `<stem>.ckpt.bin` | rolling per-cycle hash checkpoints, `(u64 through_cycle, u64 hash)` LE, headerless | `checkpoint_diff` |
| `<stem>.manifest.txt` | provenance | humans, and the drift guard below |

The framebuffer is exported **pre-palette** on purpose: a palette difference must
not be able to masquerade as a rendering difference. That is the failure mode
v2.3.8 "Parallax" was built to prevent.

### The manifest is not decoration

The determinism contract covers the framebuffer and audio. It says **nothing**
about trace-format stability, and `cpu_boot_trace` is at schema version 1 with a
history of being reshaped. A routine RustyNES accuracy fix can therefore change a
golden and turn the FPGA repository's CI red for a reason unrelated to its RTL.

The manifest records the ROM SHA-256, the seed, the **requested and actually
simulated** frame counts, the `run_frame()` call count, the cumulative CPU cycle
count, and the emulator version - so a red diff is attributable to the right side
of the boundary in one look rather than by bisecting two repositories.

Requested and actual are recorded separately because they can legitimately differ:
a ROM that jams stops advancing frames, and the export still succeeds. What must
never happen is the manifest claiming a frame count that was not simulated.

## The null-DUT gate

`crates/rustynes-cosim/tests/null_dut_self_diff.rs` is rung 0's half of the
ladder that lives here: feed RustyNES's own golden back in as if it were the DUT
and require zero divergences. It asserts three things, and the second and third
exist because the first alone proves less than it appears to.

- **Two independent exports are byte-identical** across the boot trace, the index
  framebuffer, work RAM, the cycle count and the call count. This is the
  determinism contract observed at this crate's boundary; if it fails, a
  pre-generated golden is *not* the trace a lockstep run would produce and
  replay-as-oracle is unsound.
- **A one-bit corruption is caught.** A comparator that always reports agreement
  passes a self-diff trivially.
- **Five frames is five NTSC frames' worth of cycles**, in six `run_frame()`
  calls. Checked in integer half-cycles, without trusting the counter that
  produced the number.

Verified against the real `cpu_boot_trace_diff` CLI as well as in-process: it
reports `All 5464 aligned records match` and exits 0 on the self-diff, and on a
one-bit corruption reports the divergence at cycle 561, PC `$C419`, naming the
field and both values. The zero therefore comes from a tool that can tell the
difference, not from one that cannot.

The other half of rung 0 -- hash-checkpoint agreement with full capture over a
100k-cycle window, and the Verilator side of the writers -- is v2.4.2.

## Replay, not lockstep

Build time: RustyNES emits goldens. Run time: Verilator runs the DUT, the C++
testbench writes **the same byte formats**, and the diff CLIs that already exist
compare them.

The determinism contract makes this *equivalent* to lockstep - same seed + ROM +
input yields a bit-identical framebuffer and audio, so a pre-recorded trace is
exactly the trace a lockstep run would have produced. It is additionally better in
two ways: goldens are re-diffable without re-simulating, and the two sides can run
on different machines at different times.

`scripts/mesen2_cpu_boot_trace.lua` already writes `cpu_boot_trace` from a foreign
emulator, so the FPGA testbench is the format's **third** writer, not its first.

### No DPI-C

DPI-C would push `import "DPI-C"` into RTL that must also pass Quartus, then
require `` `ifdef SIMULATION `` guards - the exact construct that lets a simulated
netlist drift from the synthesised one.

Instead, observation ports live in `tb/nes_top_cosim.sv`, **never listed in
`files.qip`**, plus Verilator `--public-flat-rd` hierarchical reads. Net synthesis
impact: zero.

### Hash first, capture on divergence

A 4200-frame AccuracyCoin run is ~125 M CPU cycles, which as per-cycle CSV is
~7.5 GB per side. Both sides instead chain a 64-bit hash over the per-cycle tuple
and compare checkpoints every 4096 cycles - **244 KB** for a full run. On the
first mismatch, re-run only that window with full capture and waveforms.

Implemented in `crates/rustynes-cosim/src/checkpoint.rs`. **Measured on a real
export** rather than projected: 3 frames of AccuracyCoin is 89,343 CPU cycles,
which is **5,372,427 bytes** of `irq.csv` against **352 bytes** of `ckpt.bin` -
a factor of **15,263**.

#### What is hashed, and what deliberately is not

`CycleRecord` carries 29 fields, and most of them are *`RustyNES`'s model*, not
hardware. `checkpoint::Observable` is the subset an external device-under-test
can genuinely produce, and `Observable::from_cycle_record` is the single place
the partition is applied - so widening it has to pass
`model_internal_state_cannot_cause_a_divergence`, which perturbs every dropped
field at once and asserts the hash does not move.

| In | Why |
|---|---|
| `cpu_cycle` | the axis both sides count on |
| `bus_access`, `bus_addr`, `bus_data` | pin-visible |
| `put_cycle` | the R/W phase half of the M2 cycle - pin-visible |
| `nmi_line` | a pin |
| `irq_line_at_low`, `irq_line_at_high` | the /IRQ pin, sampled twice per cycle |
| `pc` | **not** pin-visible; see below |

Two caveats are stated rather than buried.

**The IRQ line is one wire.** `CycleRecord` splits its samples into
`irq_pending_mapper_*` and `irq_pending_apu_*`, which is `RustyNES` *attributing*
the assertion to a source. Hardware has a single wire-OR'd /IRQ input and cannot
make that distinction, so the pairs are OR'd before hashing. Hashing them apart
would fail a correct DUT for disagreeing about something it cannot observe.

**`pc` is DUT-observable, not pin-observable.** The 6502 does not expose its
program counter. It is in because the testbench wrapper can expose the internal
register and rung 1 compares it directly - but a `pc`-only mismatch means
something weaker than a bus mismatch.

`ppu_scanline`, `ppu_dot`, `ppu_frame` and `a12_events` are out. `a12_events` is
the sharpest case: A12 transitions genuinely are observable on the cartridge
connector, so it is excluded for **scope**, not observability, and becomes a gate
when the PPU rung opens.

#### The hash must be reimplementable in ten lines of C++

The top risk at rung 0 is a format-packing mismatch masquerading as an RTL bug.
So the hash is **FNV-1a 64** - chosen for exactly one property, that a testbench
can reimplement it without a library - and `Observable::encode` defines a fixed
**16-byte little-endian** layout with an explicit zero pad byte, so the C++ side
cannot hash uninitialised struct padding. Both are pinned to a hardcoded vector
by `the_wire_encoding_is_pinned_to_a_fixed_vector`; a reordered field fails that
test rather than producing a phantom RTL defect on the next co-simulation run.

#### Three answers, and the third is the point

`checkpoint_diff <reference.ckpt.bin> <candidate.ckpt.bin>` exits `0` identical,
`1` diverged (printing the window to re-run), `2` usage/IO, and **`3`
inconclusive**. A truncated run, a DUT that stopped early, and two runs at
different intervals all produce "no divergence was found", and reporting that as
agreement is this project's recurring failure. A green job must mean the streams
were compared and matched, never that there was nothing to compare.

Cycle **alignment is checked before the hash**: two streams checkpointing at
different cycles cover different spans, so a hash difference between them says
nothing, and calling it a divergence would send a full-capture re-run at a window
where nothing is wrong.

#### A capacity-limited trace refuses rather than truncating

`IrqTrace::push` silently drops records once it reaches the capacity it was armed
with, advancing an `overflow` counter nobody has to read. A checkpoint stream
computed over a dropped-record trace hashes *fewer cycles than it claims*, and
the two sides then disagree for a reason that has nothing to do with the DUT -
which is worse than useless, because it looks like a legitimate divergence. So
`Oracle::take_checkpoints` returns `CheckpointError::TraceOverflowed` naming the
capacity to retry with, `rn_write_checkpoints` returns `-5`, and the exporter
aborts rather than writing a short stream.

#### One take, two artifacts

`Bus::take_irq_trace` **moves** the trace out, so asking for the CSV and then the
checkpoints yields `None` for whichever came second - and `None` is
indistinguishable from "never armed". `Oracle::take_irq_artifacts` derives both
from a single take; the hazard is pinned by
`taking_the_csv_first_leaves_no_trace_for_checkpoints` so it is a documented
behaviour rather than a surprise.

## What is a gate and what is diagnostic

| Surface | Role | Why |
|---|---|---|
| nestest 7 CPU fields | **gate** | independent oracle - RustyNES is 0-diff against the Nintendulator log |
| per-cycle bus / IRQ samples | **gate** | where 6502 implementations actually die |
| `index_framebuffer` | **gate** | pre-palette, so palette cannot masquerade as rendering |
| `MixRecord` integer channel levels | **gate** | 0-15 per channel, 0-127 DMC |
| AccuracyCoin RAM status vector | **gate** | compared entry for entry, including `Skipped` / `NotRun` |
| `ppu-state-trace` FSM fields | **diagnostic only** | encodes RustyNES's *modelling choices*, not hardware facts |
| mixed `f32` audio output | **diagnostic only** | the non-linear mixer and BLEP resampler are software artifacts |

The two "diagnostic only" rows are the load-bearing ones. Gating on
`ppu-state-trace` would force the HDL to transliterate a Rust data structure - bad
hardware, and an odd form of self-derivation given the black-box premise. Gating on
mixed `f32` audio would either force the HDL to reproduce a software resampler or
produce permanent unresolvable false failures.

**Partition every trace field into hardware-observable versus model-internal
before the PPU rung starts.**

## The oracle can be wrong

141/141 on AccuracyCoin is not "matches silicon". Where RustyNES is wrong,
co-simulation will drive the RTL confidently toward its bug.

Every rung is therefore labelled by whether it has an **independent** oracle:
nestest (Nintendulator) and the blargg ROMs do; trace fields with no Mesen2
counterpart do not, and are advisory only.
