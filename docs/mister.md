# MiSTer FPGA and SuperStation One - the co-simulation boundary

**Spec, not history.** Update this file in the same change as any behaviour
change to `crates/rustynes-cosim` or the golden formats it emits.

**Decision records:** [ADR 0037](adr/0037-mister-fpga-core-independent-hdl-implementation.md)
(the programme and the HDL firewall) ·
[ADR 0038](adr/0038-cosim-interrupt-injection-api.md) (the interrupt-injection API).
**Execution plan:** [`to-dos/plans/v2.7.0-mister-core-plan.md`](../to-dos/plans/v2.7.0-mister-core-plan.md)
-- **supersedes** [`v2.5.0-fabric-plan.md`](../to-dos/plans/v2.5.0-fabric-plan.md),
which is delivered.
**Execution tracking:** [`to-dos/mister/`](../to-dos/mister/).
**Research archive:** [`to-dos/plans/research/v2.5.0-research-mister-fpga.md`](../to-dos/plans/research/v2.5.0-research-mister-fpga.md),
plus four dated files in `ref-docs/` (contribution requirements, the MiSTer
framework, the hardware **source map**, and alternative FPGA targets).
**Device-under-test:** <https://github.com/doublegate/RustyNES_MiSTer> (private).

## Rung 3 CLOSES: VBlank, NMI, the `$2002` race (v2.5.8)

The VBlank flag's full CPU-visible behaviour — the set, the clear, the
destructive read, `suppress_vbl` for the one-clock-before race, and **the PPU's
/NMI wired to the CPU for the first time**. Four ROMs carry it, every one with
its stimulus measured from the oracle's trace before any gate ran; a first
draft put a handler inside the power-on NOP slide and reset executed it, both
sides agreeing because both read the same wrong ROM.

**Both structural fixes were deletions.** The testbench's cycle split was
`[2 pre-dots | access | 1 post]` and the oracle's is `[1 | access | 2]`
(`read_split(12) = (5,7)`): a ~2-dot /NMI pulse from a read racing the VBL set
was invisible to the DUT's end-of-cycle sample, and `PPU_LEAD=3` +
`ACCESS_DOT=1` (same absolute access dot, moved boundary) fixed it — the
pulse-stretcher built first was measured dead and deleted. And
`render_for_skip` — v2.5.7's deferred skip-check delay — does not exist: the
oracle's two-PPU-clock rule plus the commit-edge sampling asymmetry lands
exactly on the rendering enable, so the extra tap was deleted and the pipe
shrank to one stage. `ppuvbl024` had caught the DUT skipping ten pre-renders
the oracle never skipped, invisible to the bus gate for eight frames because
NMI delivery quantizes away single-dot drifts.

**Twelve of twelve mutations CAUGHT** — the last via a cadence-breaking frame,
because its one firing landing (enable at pre-render dot 338, odd frame) is
unreachable by any fixed-cadence ROM: the 3-dot CPU quantum and the skip's
1-dot drift lock odd-frame landings to one residue mod 3.

**nestest 0-diff at 5,002,992 cycles — the 5M window closes**, and with it
rung 3's acceptance criteria in full. The harness serves open-bus `$40` for
`$4016`/`$4017`; the next divergence anywhere is an APU or controller surface,
which is rung 4. Full record: the sibling's `docs/rung3-ppu.md`.

## Rung 3 continues: sprite rendering, and the phase (v2.5.7)

Sprite rendering, priority, the sprite-0 hit and its no-hit-at-x=255 quirk, the
left-8 masks, the garbage nametable fetches and the 337/339 dummies — and the
release's real finding: **the CPU–PPU power-on phase was wrong by two dots, and
every OAM window was compensating.** The boot traces agreed on `scanline`/`dot`
at every instruction boundary while the per-cycle mappings differed by exactly
two — a phase error hidden by an equal record-point offset, two errors
cancelling. `PPU_LEAD=2` (the earlier sweep tried 0 and 3; the answer sat
between them) moves every window from documented-minus-three to
**documented-minus-one, which is registered-assignment semantics and no residual
fudge** — the code came to what the RTL's comments had claimed all along.

**Every gate in the rung is exact for the first time**: `ppuspr019/020/021`
119,115 cycles each, `ppuscroll` 49,998, `ppusprender` 119,114, `ppusprite`
59,993, `ppuregs` 12,841, fetch traces 7,058 / 88,685 (the v2.5.4 narrowing is
**removed** — sprite fetches and the dummies are compared now), all three index
framebuffers 61,440, `nestest` 59,554 — plus **`ppu-phase-gate`**, new: the
inverse of `cpu-gate`'s skip list, `scanline`/`dot` only over twelve frames,
98,562 records, the only gate that can see the **odd-frame skipped dot**'s
one-dot-per-odd-frame drift (implemented this release; five mutations CAUGHT).

**All ten of the rung's mutation catalog are CAUGHT**, re-run in full at the
corrected phase — the tenth (sprite-0 flag read from the register only) by
`ppuspr020` at exactly one divergence, the read landing on the hit dot. Also
found by instrument rather than argument: `PPU_SUBDOT` (master-clock-resolution
clocking, built to test the half-dot between `read_split` and `write_split` —
unobservable, now measured) exposed **a register file gated by nothing**; fixed
with `cpu_ce`, a one-clock commit strobe, latent until v2.6.5 where a held
address would have latched twelve times per access. Deferred with owners named:
the VBlank-race stimulus and the skip-check delay (v2.5.8), `chr_wr` (v2.6.3).
Full record: the sibling's `docs/rung3-ppu.md`.

## Rung 3 continues: sprite evaluation (v2.5.6)

The evaluation FSM, secondary OAM, the eight-sprite limit, the documented
overflow-search bug and the wiki's step 4. **All 59,993 overlapping cycles
match**, with **seven of eight mutations CAUGHT** and the baseline verified
passing first. The programme plan named this the hardest single item in it.

**The gate observed a model the diagnostic did not expose**, and that is the
finding worth carrying out of this step. `ppu-state-trace` carries
`sprite_eval_n`, `sprite_eval_m` and `sprite_eval_found`, which belong to the
oracle's *real* evaluation FSM. What a CPU read of `$2004` returns does not come
from that machine: it comes from `tick_oam_bus`, a second, side-effect-free model
kept alongside it. So two edits made faithful to the traced fields each moved the
DUT **away** from the observable — 41 → 112 and 39 → 68 — and were reverted as
regressions. Adding `oam_bus_copybuffer` to `PpuStateRecord` at schema 2 is what
made every later measurement valid; it immediately showed the FSM sitting frozen
at `n = 34` while the bus kept walking for the rest of the line.

**One of those two regressions was then right.** The overflow halt had been
measured while phase 4 was itself mis-implemented, so it moved the DUT into a
broken destination. Re-measured against a correct phase 4 it is worth 28 of the
39. *A change rejected against a broken baseline is not a rejected change.*

**And the fix that closed it is the opposite of the obvious one.** The wiki says
phase 4 copies `OAM[n][0]`, but pinning the byte index to 0 is right on scanline
55 and wrong on scanline 58: phase 4 advances only the high half of the address,
so the low half keeps whatever ended the walk. Three of the four paths that
finish evaluation clear it; the sprite-eval bug path does not.

**Two mutants are INERT rather than uncaught, and a first pass got that wrong.**
The `eval_ovf_cnt` reset was reported as fixing a latent defect the stimulus
could not reach; the stale count in fact cannot occur — a probe fires zero times
at 528 window ends while its inverted predicate fires 528, the trace is
byte-identical without the reset, and the bound is structural (88 decide steps to
the latest hit, consumed by 91, in a 96-step window). It is kept as defensive
code, not as a fix. The second inert mutant is out of *scope* rather than
unreachable: `sprite_overflow` reaches the CPU only via `$2002`, which this ROM
never reads. Both were classified by byte comparison, because NOT CAUGHT has
meant four different things here.

Detail: `docs/rung3-ppu.md` in the sibling repository.

## Rung 3 continues: background rendering (v2.5.5)

The shift registers, the fine-X multiplexer, the palette lookup and a per-pixel
index — the first full frame this core has drawn.

**Gate:** `ppurender` **all 61,440 pixels match**, **fifteen mutations all
CAUGHT**, baseline verified passing first. The surface is the **index**
framebuffer, pre-palette, so a palette-table difference cannot masquerade as a
rendering one — different bugs, different rungs.

**The oracle needed no change at all.** `index_fb.bin` has been exported since
v2.4.1: the third rung step in a row costing the oracle side nothing, which is
what choosing the compare surface *before* the rung buys.

**The fault was one pixel, and the incomplete fix identified the mechanism.** The
first run differed on 46,730 of 61,440 pixels, which reads as a broken renderer
and was not: 13 distinct values on both sides, near-identical histograms, and
`act[x] == ref[x-1]`. The shift registers and the dot counter advance on the same
edge, so the documented "shift on dots 2-257" applied each shift one dot after
the pixel that should show it. Moving only the shift window took the first wrong
pixel from x=9 to x=17 — one tile further in — which is what proved the reload
was out of phase too. The resolution **removes** a register: the reload is the
pattern-high fetch's own dot, so it takes `chr_din` directly.

**Five NOT CAUGHT mutations indicted the stimulus, not the gate** — and one of
them three times, for three different reasons: horizontal arrangement aliasing
the nametables, a zero coarse-X scroll whose only wrap `copy_x` undid before it
reached a pixel, and a fill whose 256-period ramp made both nametables
byte-identical. Each fix looked like it had closed the hole. **Re-run mutations
after a stimulus change, not only after a code change.**

`fb_diff.py` refuses a reference frame too uniform to test anything and is
demonstrated firing in all four paths; the window is **read** from the oracle's
manifest rather than transcribed. `docs/golden-fetching.md` in the sibling
repository now specifies the standing note that had sat across six releases —
including the requirement that golden fetching use a **pinned oracle commit**.
That pin is not a detail: the determinism contract covers the framebuffer and
audio and says *nothing* about trace-format stability, so an unpinned fetcher
would turn the DUT's CI red for a reason unrelated to its RTL.

## Rung 3 continues: the background fetch pipeline (v2.5.4)

NT / AT / pattern-low / pattern-high fetches on the documented 8-dot cadence,
compared against the oracle as an **address-bus** trace -- real pins, so a
correct chip cannot differ, which is what makes it a gate rather than a
diagnostic under `docs/rung3-ppu.md`'s partition.

**Gate:** `ppufetch` **6,247 background fetches / 0 divergences** on scanline,
dot and address, across two rendering windows. **Eight mutations, all CAUGHT**,
baseline verified passing first.

**The comparison was narrowed and said so on every run.** At this release a
rendering scanline issued 154 fetches and the gate compared the 136 background
ones (dots 1-256 and 321-336), excluding sprite fetches and the two dummy
nametable reads; `fetch_diff.py` printed the excluded count for both sides
because a narrowing that is not announced reads as full coverage. **The
narrowing is gone as of v2.5.7** -- `COMPARED_DOT_SPANS` is `(1, 340)`, sprite
fetches and the dummies included.

**The finding: the CPU access was presented two dots early, and no existing gate
could see it.** The DUT issued one extra nametable fetch at the leading edge of
each rendering window and dropped one at the trailing edge, both by the same two
dots -- one quantity wrong by one constant, not two faults. The cause was in
`tb/cpu_main.cpp`, which presented the access on the second of the cycle's three
PPU dots; a 6502 commits a write and samples a read at **phi2**, the third.

Five gates stayed green across the move, **in both directions**: rung 1's
registers on nine ROMs, rung 2's per-cycle bus, the interrupt sweep, and the
v2.5.2 register and v2.5.3 scroll gates. That is not evidence the shift was
harmless. Every one of them reads state **once per CPU cycle**, so a uniform
two-dot shift in when a write lands *inside* a cycle moves nothing they compare.
This is the rung's first gate keyed to the DOT counter, and the first that could
see it.

**nestest more than doubled.** Rung 2's window was bounded at 27,396 cycles by a
missing peripheral -- nestest reads `$2002` there and the testbench had no PPU to
answer. With the register file answering it now matches for **59,554 cycles**,
2.18x the old extent, and the bound is an artifact budget rather than a wall.

**Two mutations came back NOT CAUGHT against a correct gate**, because the ROM
rendered from `$2000` with `PPUCTRL = 0` and so held `v[11]` and `ctrl[4]` at
zero throughout. The gate was not blind; the stimulus was. A second rendering
window at `$2800` with background patterns at `$1000` took the fetch count from
3,099 to 6,247 and both mutations to CAUGHT.

**Also closed: four trace features had never been linted.** No CI invocation
named `cpu-boot-trace`, `irq-timing-trace`, `ppu-state-trace` or the new
`ppu-fetch-trace`, and `--workspace --all-targets` reaches default features
only, so the `rustynes-cosim` clippy step compiled those modules **as
dependencies**, where warnings are not denied. One explicit step per feature
now; `ppu-state-trace` had **six** `-D warnings` errors waiting in it.

## Rung 3 continues: the scroll address logic (v2.5.3)

`inc_x`, `inc_y`, `copy_x` and `copy_y` at their documented dots, the
`$2007`-during-rendering dual increment, and -- found by the rung rather than
planned into it -- **a 3-dot delay on toggling rendering**.

**Gates:** `ppuscroll` **19,813 records / 0 divergences**; rung 2's bus
comparison on the same ROM **49,993 of 49,993 cycles matching** on `pc`,
`bus_addr`, `bus_data` and `bus_access`; phase identical throughout.

> "Toggling rendering takes effect approximately 3-4 dots after the write. This
> delay is required by Battletoads to avoid a crash."
> -- `nesdev_wiki/PPU_registers.xhtml`

This core applied a `$2001` write immediately. The rendering window was a full
CPU cycle too wide **at both ends**, costing exactly one coarse-X increment and
invisible to everything except a read-back of `v`.

**Both implementations were self-consistent; only the documentation could say
which was wrong.** The oracle can be wrong -- here it was not, and that was
established rather than assumed, which is the rung-labelling rule from ADR 0037
doing its job.

**Four instruments, each killing the previous hypothesis**, and the order
mattered more than any one of them:

1. `tb/phase_delta.py` over every instruction boundary: the phase offset was
   constant, then zero once the *testbench* was fixed -- **and the gate still
   failed**. That proved the two faults independent and stopped an alignment
   change being credited with a fix it had not made.
2. Rung 2's bus gate on `ppuscroll`: 3 of 49,993 cycles diverged, all read-back
   *values*, with the `$2001` writes byte-identical. Write-timing refuted.
3. `ppu-state-trace` on the oracle -- the designated **diagnostic** -- showed `v`
   incrementing at 112, 120, 128, 136, 144 **and 152**. The sixth increment was
   at the END of the window, not the start.
4. The wiki adjudicated.

**The diagnostic never became a gate**, which is what `docs/rung3-ppu.md`
reserves it for.

## Rung 3 has started (v2.5.2)

`rtl/ppu2c02.sv` implements the 2C02's CPU-visible register file: `$2000-$2007`
with `$2008-$3FFF` mirroring, the VRAM/palette bus and its read buffer, palette
and nametable mirroring, OAM, and the data-bus latch. **12,840 records, 0
divergences, 8 mutations all caught.**

The compare surface is CPU-visible reads, carried by rung 2's **existing** bus
comparison -- a step that needs no new oracle format cannot be failed by one.
`docs/rung3-ppu.md` in the sibling was written **before** the rung and fixes
which fields may fail it (`index_framebuffer`, register reads, `nmi_line`, public
test ROMs) and which may only explain a failure (`ppu-state-trace`, `v`/`t`/`x`/`w`,
shift registers, anything RGBA).

**The step also landed a behaviour the plan did not anticipate**: writes to
PPUCTRL, PPUMASK, PPUSCROLL and PPUADDR are ignored for **~29,658 CPU clocks**
after reset. Not modelling it made the DUT disagree on every VRAM and palette
access while open bus and OAM stayed correct -- so the register file *looked*
right.

**And the ROM passed on its first run while testing nothing.** Four defects, each
making both sides agree about a behaviour neither was being asked about; all four
found by mutation, none visible by reading. The lesson is recorded in the
sibling's `docs/rung3-ppu.md`: a passing gate is evidence only once something has
been shown to make it fail.

## Rung 2 is closed (v2.5.1)

Its interrupt half was the last piece. `tb/interrupt_sweep.py` asserts /NMI,
/IRQ, or **both together** before instruction K and holds it, for every K across
a hazard program, driving identical stimulus into both sides: **60 injection
points, 0 divergences** on all seven CPU fields.

It found two defects, and the second is the one worth remembering. A hardware
interrupt pushed a return address **one byte too high**, because `AM_BRK` fell
through a generic operand-fetch increment shared with every other addressing
mode. `BRK` and a hardware interrupt share that mode and *disagree* about it --
`BRK` advances over its second byte, an interrupt does not -- so for `BRK` two
writers assigned the same value and the fault was invisible. **`BRK` passing
186/186 is what kept it hidden**: the only opcode exercising the mode was the one
on which the bug did not show.

The oracle side is [ADR 0038](adr/0038-cosim-interrupt-injection-api.md)'s
`cosim-interrupt-inject` feature. Its precondition -- that a default build emits
none of it -- is **measured, with a live control**: `inject_` appears 0 times in
the expanded default core and **17** times with the feature on. The control is not
ceremony. The ADR's original command piped a missing `cargo-expand` through
`grep -c`, which reports the 0 it is looking for while measuring nothing.

**Two v2.5.0 gates remain open and are reclassified, not carried.** nestest 0-diff
and the 5 M-cycle window both stop at a `$2002` read where *both sides address it*
and only the data differs -- the DUT has no PPU. They are rung-3 acceptance
criteria.

---

## What this is, and what it is not

RustyNES is **not** being ported to FPGA. A MiSTer core is SystemVerilog compiled
by Quartus 17.0.2 into a Cyclone V bitstream; Rust does not become a bitstream.

What the "Fabric" line builds is a **new NES implementation in SystemVerilog,
written from public hardware documentation**, in a sibling repository
([`doublegate/RustyNES_MiSTer`](https://github.com/doublegate/RustyNES_MiSTer),
private), with **RustyNES as its verification oracle**. This document specifies
the boundary between the two - the one part that lives in this repository.

The sibling repository holds the harness at rung 0 and **no RTL**, which is the
ladder's design rather than a gap: the testbench must be shown able to recognise
agreement before anything is compared. Two of its files matter to readers of
this document, because they are the other half of what is specified here.

`tb/checkpoint.h` reimplements this repository's checkpoint encoding in C++, and
`tb/checkpoint_selftest.cpp` asserts it against **the same hardcoded vector**
`the_wire_encoding_is_pinned_to_a_fixed_vector` pins on this side. That pairing
is the whole guard against the top-ranked risk at this rung: a packing
disagreement between the two halves produces a hash mismatch that is
indistinguishable from a wrong DUT, and would be debugged as one. **Changing
`Observable::encode` here without changing `checkpoint.h` there breaks
co-simulation in the way that is hardest to diagnose** - so the selftest is the
first thing to run after touching either.

Its licence audit also settled a question this side had left open. ADR 0037
recorded that a GPL-2.0-**only** file anywhere in the MiSTer framework's `sys/`
would force the RTL to GPL-2.0-or-later. All 57 files were read: **there is no
such file**, and four are GPL-3.0-or-later - including `hps_io.sv`, which no core
functions without. GPL-2.0-or-later combines upward and GPL-3.0-or-later does not
reduce, so the combined bitstream must be **GPL-3.0-or-later**, which is already
this project's licence. The hedge is inverted by the evidence rather than
confirmed by it.

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
| `<stem>.obs.bin` | full-capture observable stream, repeated 16-byte records | the testbench's self-diff, and a located-window re-run |
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
and compare checkpoints every 4096 cycles - **~480 KB** for a full run. On the
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

#### The gate: checkpoints agree with full capture

Checkpoints approximate "where do these two runs first differ", traded for four
orders of magnitude of disk, and the scheme is worthless if the approximation
can disagree with the answer. `first_full_capture_difference` computes the answer
directly; `localisation_is_consistent` states the contract, as a function rather
than as prose here.

| full capture says | the comparison must |
|---|---|
| identical | report `Identical` - anything else is a false positive, and a gate that cries wolf gets switched off |
| first differs at `k` | **not** report `Identical` - that false negative passes a wrong DUT |
| first differs at `k`, and it reports `Diverged` | name a window **containing `k`** |
| first differs at `k` | `Inconclusive` is an acceptable, honest refusal |
| identical | `Inconclusive` is **not** acceptable |

The third row is the one worth the effort. A divergence report naming the wrong
window sends a full-capture re-run somewhere nothing is wrong, spends the
debugging budget, and returns "no problem here" - which reads as evidence the DUT
is fine.

A sweep drives 331 cases across every run length around the interval boundary,
corrupting a different observable field at a different position each time. It
found a real defect on its first run: **a divergence at cycle zero was reported
in a window that did not contain it.** `after_cycle` was a `u64` in which `0`
meant both "no prior checkpoint" and "cycle zero", so the first window read as
`(0, 0]` - empty. It is now `Option<u64>`, and `Divergence::contains` is offered
so call sites do not reimplement a boundary that is half-open at one end and
open-ended at the other.

#### The CSV cannot re-derive the checkpoints, so `.obs.bin` exists

Found by trying to build the rung-0 self-diff on the CSV. `irq.csv` carries **23
columns**, and neither `pc` nor `put_cycle_post` is among them - two of the nine
observable fields are simply absent. So an external testbench reading the CSV
cannot reproduce the checkpoint hashes, and "feed `RustyNES`'s golden back in as
if it were the DUT and get zero divergences" was not implementable as designed.

`<stem>.obs.bin` closes that: repeated 16-byte records in the **same wire
encoding the hash folds**, headerless. It is the only artifact the checkpoints
can be independently re-derived from, and it is also the input a re-run of a
located window consumes - so it would have been needed regardless.

Additive. The CSV is untouched, which matters because `scripts/irq_trace_cross_diff.py`
and the committed `golden/irq_trace/*.csv` both depend on its shape.

`Observable::decode` is the inverse, and it **refuses what it does not
understand**: a non-zero reserved pad byte, an undefined flag bit, an unknown
bus-access code, a short record, a stream length that is not a multiple of 16.
None of that is pedantry - reading a record from a newer producer as though
nothing had changed is how a *format* divergence gets reported as a *DUT*
divergence.

The stream is emitted even when the checkpoints are refused for overflow. A hash
over a truncated trace claims a coverage it does not have; the records
themselves are just records, and are worth keeping for a re-run.

#### The rung-0 self-diff, measured

The oracle's own output fed back in as though it were the DUT, across the
repository boundary:

```console
$ tb/selfdiff_check.sh
1/3 exporting goldens from the oracle
2/3 re-deriving checkpoints on this side
re-derived 22 checkpoints from 89335 records
3/3 comparing
checkpoints match: 22 compared, 0 divergences

negative control: one flipped bit must be located
DIVERGED at checkpoint 10
  window to re-run with full capture: cycles (40967, 45063]  (4096 cycles)

rung 0 self-diff: agreement recognised, and disagreement located
```

89,335 records of AccuracyCoin, re-derived in C++ from `.obs.bin` alone, hashing
to byte-identical checkpoints. The negative control runs **in the same
invocation**, because a positive control alone is satisfiable by a comparison
that always agrees.

It is not in CI: it needs both repositories and a test ROM. It exits **77** and
says why when it cannot run - a skip that reports itself, never a silent pass.

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

## The two risks that had to be settled before any RTL

Both were settled in v2.4.3, and **both were answered by evidence that
contradicted what the plan assumed**. That is the point of running the
experiment rather than reasoning about it.

### Risk 1 — the `sys/` licence, which inverted its own hedge

The plan required this *before any RTL is written*, because relicensing after ten
thousand lines exist is precisely the failure
`docs/originality-and-provenance.md` documents. Every file under
`Template_MiSTer`'s `sys/` was read and classified by its own grant, with comment
markers stripped and whitespace collapsed before matching.

| Classification | Files |
|---|---|
| GPL-2.0-or-later | 9 |
| **GPL-3.0-or-later** | **4** |
| GPL by reference, no version stated | 4 |
| Copyright, no grant | 4 |
| No header | 36 |
| **GPL-2.0-only** | **0** |
| | **57 total** |

The hedge was that a GPL-2.0-**only** file would force the RTL *down* to
GPL-2.0-or-later. There is no such file, and the binding constraint runs the
other way: `ddr_svc.sv`, `hps_io.sv`, `scandoubler.v` and `sd_card.sv` are
GPL-3.0-or-later, and **`hps_io.sv` is not optional** — it is how a core receives
a ROM from the HPS and how the OSD reaches it. GPL-2.0-or-later may be combined
into a GPL-3 work because "or later" permits the upgrade; GPL-3.0-or-later cannot
be reduced to GPL-2.

**The combined bitstream must be GPL-3.0-or-later, which is already RustyNES's
licence.** No relicensing is needed.

### Risk 4 — the Quartus subset, fitted rather than read

Quartus 17.0.2's SystemVerilog subset is materially narrower than Verilator's,
and a construct rejected after ten thousand lines exist invalidates not one file
but the style every file was written in. The acceptance criterion was never "it
compiles" — it was a fitted netlist **and its resource report**, because only the
second catches a 2 KiB memory that became 16,384 flip-flops, and Quartus does
that silently.

Quartus Prime Lite 17.0.2 Build 602, device 5CSEBA6U23I7, defaults throughout:

```text
Analysis & Synthesis was successful. 0 errors, 0 warnings
Fitter was successful.               0 errors, 4 warnings

Total block memory bits  ; 16,384 / 5,662,720 ( < 1 % )
M10K blocks              ; 2 / 553
Total registers          ; 29
Logic utilization (ALMs) ; 17 / 41,910 ( < 1 % )
```

**29 registers, not 16,413.** The inference style that worked is a
`logic [7:0] mem [0:2047]` with one synchronous read port on a registered
address, one synchronous write port, and **no asynchronous read** — an async read
is the usual reason an array becomes registers. Quartus produced a Simple Dual
Port ALTSYNCRAM unaided, populated its MIF from the `initial` block (which is how
a boot ROM lands *inside* the block), and one-hot encoded the `enum` as a state
machine.

**This is now the required style, and every commit adding a memory quotes its
resource line** — because the failure is silent in isolation and only becomes
unfittable in aggregate.

Nine constructs are **fitted**. Plain `case`, `priority case` and `$bits` are
deliberately left *documented*: the kitchen sink does not exercise them, and
"near-certainly fine" is the phrase the subset policy exists to refuse.
Extending the subset means extending that module and re-fitting.

The four fitter warnings are 58 unconstrained pins and a missing `.sdc`, in a
module with no pinout and no timing constraints. Timing closure is a rung-6
question.

## Rung 1 — the 6502, and where it has actually got to

Nine opcode groups have closed. **2115 records across nine ROMs** on rung 1,
**4537 cycles** on the per-cycle bus gate, and **27,388 cycles of nestest** --
all matching the oracle, and `pc` agreeing on **3551 of 3551** cycles.

**Two of v2.5.0's stated gates do not close, and neither is a defect.** nestest
0-diff over the whole run and per-cycle equality over a 5 M-cycle window both
need a PPU: nestest reads `$2002` at cycle 27396, *both sides address it*, and
only the data differs -- a missing peripheral, rung 3 by design. The
interrupt-injection sweep has no oracle-side stimulus at all, so the pins, the
hijack and delayed-`I` are implemented and **not oracle-verified**; `BRK` is,
because a software interrupt needs no pin. `docs/adr/0038-cosim-interrupt-injection-api.md`
records the decision that would unblock the sweep, and the two preconditions
that void it. The RTL lives in the sibling
repository; `RustyNES_MiSTer/docs/rung1-6502.md` is its detailed record.

| release | scope | records |
|---|---|---|
| v2.4.4 "Ignition" | the eight-cycle reset and the single-byte implied group | 147 |
| v2.4.5 "Compass" | immediate / zero page / absolute; loads, stores, all eight branches | 140 |
| v2.4.6 "Abacus" | three indexed modes with the page-cross penalty; `ADC`/`SBC`; the compares | 286 |
| v2.4.7 "Keystone" | the stack group, `JSR`/`RTS`/`RTI`, `JMP` and its page-boundary bug | 179 |
| v2.4.8 "Palimpsest" | read-modify-write: `ASL`/`LSR`/`ROL`/`ROR` and `INC`/`DEC`, accumulator plus four memory modes | 358 |
| v2.4.9 "Plumbline II" | the logical group; the undocumented opcodes; **and rung 2's bus half** | 236 + 317 |

The earlier ROMs are re-run on every change, which is how the v2.4.5 datapath
rewrite was shown not to regress v2.4.4.

The counts are **measured** -- each one from running `cpu-gate` against a freshly
exported golden -- rather than carried forward from the release that introduced
the ROM. `opgroup1` closed v2.4.4 over a 0..64 cycle window and was
reported then as 29 records; v2.4.5 widened every ROM's window to catch the
addressing modes, and the same ROM now yields 147. Both numbers are true of
different windows, which is exactly why a table mixing them would not add up --
the totals here are all measured under the current windows.

### What the rung has established beyond the opcodes

**The DUT is the third writer of `CpuBootTrace`**, after the oracle itself and
`scripts/mesen2_cpu_boot_trace.lua`. `cpu_boot_trace_diff` reads it with no
modification, and `--skip-fields` already existed -- so the entire rung has
needed **no oracle-side change**. That is the payoff of replay-rather-than-
lockstep stated concretely: the comparison tooling does not know one side is
hardware.

**The oracle corrected our own specification.** `docs/cpu-6502.md` said reset was
a 7-cycle sequence in one section and 8 in another. An independent implementation
written *from that document* implemented seven and diverged on its first record.
Reset is eight; the document is fixed. A second implementation built from a spec
is a way of testing that spec, and this is what it found on day one.

**Three tests read correctly and verified nothing**, each found by mutation and
not by reading:

- `TXS` after `TSX` -- a wrongly-flagging `TXS` computes exactly the flags `TSX`
  had already left, so the wrong answer coincided with the right one.
- A store and load in the *same* addressing mode -- self-consistent under any
  address mutation, so it tests round-tripping rather than addressing.
- A read of RAM the program had not written -- the oracle powers on with
  deterministic **seeded** work RAM while a flat-memory testbench starts at zero,
  so the divergence had nothing to do with the CPU.

Designing each case so the **wrong answer differs from the right one** is now the
default rather than a correction.

### What CI does and does not check

The sibling repository's `cpu-smoke` builds the core and runs the ROMs to
completion, and its workflow step name says it is **not** the accuracy gate --
because the oracle's goldens are not vendored there. They are reproducible from a
pinned oracle commit, and a second copy would be a drifting copy.

The accuracy comparison is `make -C tb cpu-gate GOLDEN=...`. Automating it needs
golden fetching from a pinned commit, which is **not built yet and is not
pretended to be**. That is the largest remaining hole in this rung's
infrastructure and it is named here rather than left to be discovered.

### Still open

**The gap below is CLOSED as of v2.4.9.** `make -C tb cpu-bus-gate` catches both
mutations, and the section is kept rather than deleted because the reasoning is
what makes the gate's scope legible.

Read-modify-write closed in v2.4.8 -- but its **double write did not**, in the
sense that nothing at rung 1 verifies it. Two mutations (skip the dummy write; emit
the modified value instead of the old one) both come back **NOT CAUGHT**,
because neither changes a register, a flag, the final memory contents or the
cycle count, and those are the only things `CpuBootTrace` carries.

That is scoped to **v2.4.9**, beside the undocumented opcodes, rather than left
to v2.5.0: the bus half of rung 2 needs no new RTL. `Observable` already exists
on both sides with a byte-identical encoding, the oracle already emits it as
`.obs.bin`, and `cpu6502` already exposes its whole bus. Wiring a writer produced
**7 divergences across 793 cycles** on a program rung 1 scores 358/358 -- two
causes, one harness fidelity (the testbench zeroes RAM the oracle seeds, so every
dummy read of unwritten memory diverges) and one a genuine per-cycle access
difference where instruction boundaries still agree.

> **Historical, as of v2.4.9.** The two paragraphs below were true when written
> and are both superseded: the undocumented opcodes landed in v2.4.9, rung 2's
> bus half is live, and nestest now runs to 27,388 cycles. Kept rather than
> deleted because the reasoning still explains why the rung was scoped that way
> -- but the authoritative status is the section at the top of this file.

The undocumented opcodes are **not** delivered and also move to v2.4.9.

Rung 1's own gate -- nestest 0-diff over >= 8000 instructions -- is not met yet:
the five ROMs are hand-built opcode groups, not nestest. Rung 2, the per-cycle
bus and interrupt comparison, has not begun.
