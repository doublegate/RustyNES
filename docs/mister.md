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
first mismatch, binary-search the window and re-run only that window with full
capture and waveforms.

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
