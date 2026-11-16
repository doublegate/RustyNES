# MiSTer FPGA and SuperStation One - the co-simulation boundary

**Spec, not history.** Update this file in the same change as any behaviour
change to `crates/rustynes-cosim` or the golden formats it emits. It describes
the boundary as it stands at v3.7.1 "Imprint". How each part of it was built,
release by release from v2.4.1 to v3.0.x, is in the frozen
[`archive/mister-history-v2.5-to-v3.0.md`](archive/mister-history-v2.5-to-v3.0.md);
per-release detail after that is in `CHANGELOG.md` and the release notes.

> **No hardware has run any bitstream.** Every `.rbf` attached to a release is a
> **release candidate, not hardware-verified**
> ([ADR 0043](adr/0043-v3-is-the-api-major-and-a-release-candidate-core.md)).
> Timing closure and a reproducible bitstream are claimed. A booting core, a
> synced display, audible audio and a working controller are not.

**Decision records:**
[ADR 0037](adr/0037-mister-fpga-core-independent-hdl-implementation.md) (the
programme and the HDL firewall) ·
[ADR 0038](adr/0038-cosim-interrupt-injection-api.md) (the interrupt-injection
API) · [ADR 0039](adr/0039-sdram-clock-domain.md) (the SDRAM clock domain) ·
[ADR 0040](adr/0040-public-release-metadata-is-outside-the-reference-firewall.md)
(what may be said about a black-boxed core) ·
[ADR 0041](adr/0041-hardware-release-is-v3.0.0.md) as amended by ADR 0043 (v3.0.0
is the API major with a release-candidate core).
**Hardware verification:** planned for a later v3.x release,
[`to-dos/plans/v3.x-hardware-verification-plan.md`](../to-dos/plans/v3.x-hardware-verification-plan.md).
**Execution tracking:** [`to-dos/mister/`](../to-dos/mister/).
**Device-under-test:** <https://github.com/doublegate/RustyNES_MiSTer> (private).
**Agent notes:** [`agents/mister-cosim.md`](agents/mister-cosim.md) (measured
findings and traps, read on demand).

## What this is, and what it is not

RustyNES is **not** being ported to FPGA. A MiSTer core is SystemVerilog compiled
by Quartus 17.0.2 into a Cyclone V bitstream; Rust does not become a bitstream.

What the "Fabric" line builds is a **new NES implementation in SystemVerilog,
written from public hardware documentation**, in a sibling repository
([`doublegate/RustyNES_MiSTer`](https://github.com/doublegate/RustyNES_MiSTer),
private), with **RustyNES as its verification oracle**. This document specifies
the boundary between the two - the one part that lives in this repository.

The sibling holds a complete console: a cycle-exact 6502, the 2C02, the 2A03,
the mappers it implements, the Famicom Disk System, save states and rewind, all
wrapped in the MiSTer framework (`sys/`, vendored verbatim). Its `README.md` is
the current feature list. Each release compiles a pair of bitstreams from it
and attaches them to the GitHub release of **both** repositories, as release
candidates.

**It may be declined as a duplicate.** `NES_MiSTer` already exists, and the
sibling's README records that the incumbent's accuracy work in September 2026
made the case for a second core weaker, not stronger. The comparison number is
re-measured against the current incumbent, on the same AccuracyCoin build,
before any submission is sent (the sibling's `docs/bringup-log.md` row F1, and
`docs/submission-case.md`). Public release metadata about a black-boxed core is
outside the firewall; its source is inside it (ADR 0040).

**Licence.** The combined bitstream is **GPL-3.0-or-later**, which is already
this project's licence. The framework's `sys/` carries GPL-3.0-or-later files
that no core functions without (`hps_io.sv` among them) and no GPL-2.0-only file;
GPL-2.0-or-later combines upward and GPL-3.0-or-later does not reduce. The audit
and its re-verification against the vendored tree are the sibling's
`docs/licence-audit-2026-08-20.md`. This states the licence the bitstream must
carry; it is not a claim that the work is license-clean, which is for a human
reviewer to establish (`docs/originality-and-provenance.md`).

**The checkpoint encoding has two implementations.** `tb/checkpoint.h` in the
sibling reimplements this repository's checkpoint encoding in C++, and
`tb/checkpoint_selftest.cpp` asserts it against **the same hardcoded vector**
`the_wire_encoding_is_pinned_to_a_fixed_vector` pins on this side. That pairing
is the guard against a packing disagreement between the two halves, which
produces a hash mismatch indistinguishable from a wrong DUT. **Changing
`Observable::encode` here without changing `checkpoint.h` there breaks
co-simulation in the way that is hardest to diagnose** - so the selftest is the
first thing to run after touching either.

## The two builds

Both builds come from one RTL tree and differ in where cartridge memory lives.
The sibling's `README.md` ("The two builds") is the authoritative table; in
summary:

| | `RustyNES` (headline, off-die) | `RustyNES_Lite` (on-die) |
|---|---|---|
| Cartridge memory | off the die, in the SDRAM add-on (`USE_SDRAM` = 1) | on the die, in M10K (`USE_SDRAM` = 0) |
| SDRAM add-on on a DE10-Nano | required | not required |
| Boards | every mapper number the core implements, the **Famicom Disk System**, every expansion-audio block | the smaller set that fits on the die; the rest are refused as "Unsupported mapper" |
| Size limit | bounded by the SDRAM | 256 KiB PRG / 128 KiB CHR |
| More than eight sprites per line | no | yes |
| `releases/` file | `RustyNES_YYYYMMDD.rbf` | `RustyNES_Lite_YYYYMMDD.rbf` |
| GitHub release asset | `RustyNES_MiSTer-<tag>.rbf` | `RustyNES_MiSTer-<tag>-lite.rbf` |

**The FDS is off-die only.** Both builds keep the core name `RustyNES`, so they
share `games/RustyNES/` and its saves; a save state is refused by a build whose
chain layout or memory map differs (each state carries a fingerprint). The
`releases/` name is the only form both MiSTer parsers accept, which is why the
datecoded name is kept in the repository while the GitHub assets are
version-named (the sibling's `docs/bitstream-release.md`).

**One artifact, two boards.** The SuperStation One forks `Distribution_MiSTer`
and consumes MiSTer cores directly, so the same `.rbf` is what both a DE10-Nano
and a SuperStation One take. Whether the identical file boots both is a
hardware claim and is not made.

**How a pair is produced.** `scripts/release-rbf.sh` in the sibling verifies a
compile against the Quartus **reports** rather than its exit code (Quartus has
aborted after the resource summary and still exited 0): no errors, no negative
slack on any of the four timing corners, an unchanged warning set, and a
datecode that agrees with HEAD. `BUILD_DATE` is a constant in the design, so a
compile on a new day is a new placement; the fitter seed is pinned per release
and each build is compiled twice from a clean database and required to be
byte-identical (`scripts/double-compile.sh`). The pinned seed, the margins and
each file's md5 are in that release's notes, here (`.github/release-notes/`) and
in the sibling's `README.md` and `CHANGELOG.md`.

**Unverified by construction.** Every gate compares something upstream of the
video timing constants, the audio's absolute level, its band-limiting, and the
`sys/` menu options (aspect, scaling, crop). That partition is deliberate - it
is what stops a palette or mixer difference masquerading as a rendering or
channel one - and its price is that those properties have no evidence behind
them until a board runs the file. The palette itself is gated (`palette-gate`
compares `palette.sv`'s output entry for entry).

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

> **What the identifier grep is (2026-11-15).** Until v3.7.1 the sibling's CI
> carried only the path denylist (it fails if a directory named `NES_MiSTer`
> or `fpganes` is in the tree) and an SPDX-header check; the grep was claimed
> here and not built. v3.7.1 adds it: `tb/check_hdl_firewall.py`, run with its
> self-test in the sibling's CI, fails when a black-boxed core's name,
> repository path or author's handle appears in `rtl/`, `tb/` or the Quartus
> build inputs. **It is a tripwire for references, not evidence that nothing
> was copied.** It cannot match those cores' module, signal or constant names,
> since listing them would mean reading their source; logic transcribed with
> every name changed passes it. Independence rests on the rule and on review,
> as [ADR 0037's amendment of 2026-11-15](adr/0037-mister-fpga-core-independent-hdl-implementation.md#amendment-2026-11-15-what-the-identifier-grep-does-and-does-not-catch)
> records.

The same rule, as the project states it in
[`agents/provenance-firewall.md`](agents/provenance-firewall.md) and the sibling
states it in `docs/provenance.md`, covers **any other NES `rtl/`** as well. Two
further terms bind HDL work:

- **The firewall is per-region, not per-repo.** Files in this repository carrying
  a `// Provenance:` header disclose a region derived from a GPL reference
  emulator, and those regions are black boxes for HDL work even though the file
  is ours: writing SystemVerilog from them would launder the original expression
  into the DUT through this repository. List them with
  `grep -rln "^// Provenance:" crates`; the command is the authority.
- **The TriCNES exception does not lift the HDL rule.** TriCNES may be consulted
  under the terms in `agents/provenance-firewall.md`, and only for this
  emulator; it opens no HDL source.

## `crates/rustynes-cosim`

A pure wrapper over `rustynes-core`. It adds **no** core API and changes **no**
core behaviour; it is additive, absent from the default build, and its presence
cannot move AccuracyCoin or nestest.

### It is excluded from the workspace, deliberately

`rustynes-cosim` is in the root manifest's `[workspace] exclude`, not its
`members`. That is the only mechanism that makes the isolation real.

The crate enables trace features on `rustynes-core`, and **cargo unifies
features across a workspace build**. As a member, it made `cargo build
--workspace` compile the core with the union, so every workspace build linked
the **instrumented** per-cycle PPU tick loop - `irq-timing-trace` selects a
different loop, not an inert branch - and the accuracy battery
(`cargo test --workspace --release --features test-roms`) validated a scheduler
no user runs. The measured cost was small (+1.2% to +1.9% on `full_frame`, below
the 3% adoption bar), which shows the fix was never about speed: a gate pointed
at the wrong code path is wrong at any percentage.

**What exclusion costs, and how each cost is closed:**

| Cost | Closed by |
|---|---|
| Cannot use `field.workspace = true`; version/edition/license/lints duplicated | `cosim_manifest_audit.rs` asserts every duplicated value equals the workspace's |
| Someone re-adds it to `members` | the same audit asserts it is still in `exclude` |
| `cargo fmt --all` / `clippy --workspace` / `test --workspace` do not reach it | explicit `fmt`, `clippy` and `test` steps in `ci.yml` |
| Its dependencies leave the workspace `cargo deny` graph | today only `sha2`, already in the graph; re-check if that changes |

### Crate types

`["rlib", "staticlib", "cdylib"]` - `rlib` so the golden-export binary and the
crate's own tests use the safe Rust API directly, `staticlib`/`cdylib` so a
Verilator C++ testbench links the same code through the C ABI.

### The trace features are mandatory

`Cargo.toml` enables `cpu-boot-trace`, `irq-timing-trace`, `ppu-fetch-trace`,
`cpu-instr-cycle-trace`, `cosim-interrupt-inject` (ADR 0038) and `debug-hooks`
on `rustynes-core` unconditionally, rather than re-exposing them as this crate's
own optional features. A build without them would compile, link, run, and export
**empty** goldens - an absence of signal that reads exactly like agreement.
Making them mandatory turns that into a compile error. Each one's comment in
`Cargo.toml` records how it was shown to change no emulated output.

The one optional feature, `ppu-state-trace`, is **off by default** and is a
diagnostic: it compiles out the core's fast dot path, and the default build is
the control that `tests/fast_path_does_not_bypass_the_fetch_trace.rs` needs.

### The power-on frame latch

The PPU is constructed at dot 340 of the pre-render line, so the 7-cycle reset
sequence ticks past the frame wrap and leaves `frame_complete` latched. **The
first `run_frame()` after construction therefore consumes that latch and returns
without stepping a single cycle.**

A bare `for _ in 0..n { run_frame() }` would emit an (n-1)-frame golden under a
manifest claiming n. `Oracle::advance_frames(n)` therefore gates on the **frame
counter**, not the call count, and bails out on a jammed CPU. The behaviour is
pinned by `the_first_run_frame_after_power_on_advances_nothing`, so a core change
that removes the quirk fails a test that names it rather than silently altering
every golden's length.

## Golden formats

`nes_golden_export --rom <path> --out <dir>` writes, under `<dir>`, the
artifacts its flags arm (the binary's module documentation and its usage
message list every flag):

| File | Format | Consumed by |
|---|---|---|
| `<stem>.boot.bin` | `CpuBootTrace` binary - 12-byte magic `RUSTYNES_CPU`, schema version, packed records | `cpu_boot_trace_diff` |
| `<stem>.irq.csv` | per-CPU-cycle IRQ/bus CSV, two samples per cycle | `scripts/irq_trace_cross_diff.py` |
| `<stem>.ckpt.bin` | rolling per-cycle hash checkpoints, `(u64 through_cycle, u64 hash)` LE, headerless | `checkpoint_diff` |
| `<stem>.obs.bin` | full-capture observable stream, repeated 16-byte records | the testbench's self-diff and bus gates, and a located-window re-run |
| `<stem>.index_fb.bin` | 256x240 little-endian `u16`, **pre-palette** | the testbench's frame comparison |
| `<stem>.index_fb_frames.bin` | every frame's index framebuffer in order (`--fb-every-frame`) | per-frame picture gates |
| `<stem>.fetch.bin` | per-dot PPU bus addresses, 12-byte records (`--fetch-trace`) | the PPU fetch gates |
| `<stem>.apu.bin` | per-CPU-cycle **integer** channel levels, 16-byte records (`--apu-trace`) | the APU gates |
| `<stem>.ext_audio.bin` | per-CPU-cycle expansion audio, 12-byte records (`--ext-audio-trace`) | the expansion-audio gate |
| `<stem>.ppu_state.csv` | per-dot PPU model state (`--ppu-state-trace`, feature-gated) | **diagnosis only** |
| `<stem>.ram_init.bin` | 2 KiB CPU work RAM **before** execution | the testbench, so its memory starts where the oracle's does |
| `<stem>.ram.bin` | 2 KiB CPU work RAM after the run | `accuracy_coin_catalog::decode_results` |
| `<stem>.manifest.txt` | provenance | humans, and the drift guard below |

The framebuffer is exported **pre-palette** on purpose: a palette difference must
not be able to masquerade as a rendering difference. A trace that is armed and
fills its capacity is refused rather than written short (see *A
capacity-limited trace refuses*, below).

### The manifest is not decoration

The determinism contract covers the framebuffer and audio. It says **nothing**
about trace-format stability, and `cpu_boot_trace` has a history of being
reshaped. A routine RustyNES accuracy fix can therefore change a golden and turn
the sibling red for a reason unrelated to its RTL.

The manifest records the ROM SHA-256 (and, for a disk image booted with
`--bios`, the BIOS's), the seed, the **requested and actually simulated** frame
counts, the `run_frame()` call count, the cumulative CPU cycle count, the
checkpoint interval and counts, and the emulator version - so a red diff is
attributable to the right side of the boundary in one look rather than by
bisecting two repositories. Requested and actual are recorded separately because
they can legitimately differ: a ROM that jams stops advancing frames, and the
export still succeeds. What must never happen is the manifest claiming a frame
count that was not simulated.

A run option that changes what the ROM **executes** is recorded in the manifest:
`press_start` (as `A:B`, or `none`), the stimulus devices, `sprite_limit_off`.
Lines for options added later are written only when the option is given, so every
earlier manifest stays byte-identical.

### Input-device and cheat stimuli

The input and cheat gates need a golden with the same device attached on this
side. These exporter flags attach one for the whole run:

| Flag | Attaches | Manifest line |
|---|---|---|
| `--four-score P1:P2:P3:P4` | the Four Score, with four pads' button bytes (two hex digits each, `Buttons` bit order) | `four_score = 01:82:44:a8` |
| `--vaus POS:FIRE` | an NES (7-pin) Arkanoid controller on `$4017`: knob position (two hex digits) and fire (`0`/`1`) | `vaus = a7:1` |
| `--zapper X:Y:TRIG` | an NES Zapper on `$4017`: aim point (two hex digits each) and trigger (`0`/`1`) | `zapper = 0a:7f:0` |
| `--genie CODE` (repeatable) | a 6- or 8-letter Game Genie code | `genie = SXIOPO,YYKPOYZZ` |

They call the emulator's existing models (`Nes::set_four_score`,
`Nes::set_paddle`, `Nes::set_zapper`, `Nes::add_genie_code`) and change no
emulated behaviour of their own, so `EMULATION_EPOCH` does not move. The inputs
are held **constant** for the run: a frame-windowed schedule would need the
DUT's testbench to reproduce the frame boundaries, and a stimulus ROM that reads
the device every frame exercises every bit of it anyway.

These requests are refused rather than exported: `--four-score` with
`--press-start` (both write port 1's buttons), `--four-score` with `--vaus` (the
NESdev Arkanoid page: "No version is compatible with four player adapters"),
`--zapper` with `--vaus` or `--four-score` (all three use port 2, and the last
attached device would win it), two codes patching one address (the emulator
keeps one code per address, so the manifest would list a code that never
applied), and an undecodable code. The parsers, the refusals, the
manifest lines and the attach path are tested in the binary's `device_tests`;
the attach test runs a ROM that strobes the ports and reads them back, so a flag
that parsed and attached nothing fails it.

## The null-DUT gate

Rung 0's half of the ladder that lives here: feed RustyNES's own golden back in
as if it were the DUT and require zero divergences. Its tests are in
`crates/rustynes-cosim/src/lib.rs`, and the second and third exist because the
first alone proves less than it appears to.

- **Two independent exports are byte-identical** across the boot trace, the index
  framebuffer, work RAM, the cycle count and the call count
  (`the_null_dut_self_diff_is_zero_across_every_format`). This is the determinism
  contract observed at this crate's boundary; if it fails, a pre-generated golden
  is *not* the trace a lockstep run would produce and replay-as-oracle is
  unsound.
- **A one-bit corruption is caught**
  (`a_single_flipped_bit_is_not_mistaken_for_agreement`). A comparator that always
  reports agreement passes a self-diff trivially.
- **Five frames is five NTSC frames' worth of cycles**, in six `run_frame()`
  calls (`five_frames_requested_is_five_frames_of_cycles`), checked in integer
  half-cycles without trusting the counter that produced the number.

The cross-repository half is the sibling's `tb/selfdiff_check.sh`: it exports
goldens from the oracle, re-derives the checkpoints in C++ from `.obs.bin`
alone, compares them, and then, **in the same invocation**, flips one bit and
requires the divergence to be located - because a positive control alone is
satisfiable by a comparison that always agrees. It needs both repositories and a
test ROM, so it is not in CI; it exits **77** and says why when it cannot run, a
skip that reports itself rather than a silent pass.

## Replay, not lockstep

Build time: RustyNES emits goldens. Run time: Verilator runs the DUT, the C++
testbench writes **the same byte formats**, and the diff tools compare them.

The determinism contract makes this *equivalent* to lockstep - same seed + ROM +
input yields a bit-identical framebuffer and audio, so a pre-recorded trace is
exactly the trace a lockstep run would have produced. It is additionally better in
two ways: goldens are re-diffable without re-simulating, and the two sides can run
on different machines at different times. `Nes` exposes `run_frame()` and
`step_instruction()` and nothing finer, so lockstep would also need new core API
on the hot path.

The DUT is the **third** writer of `CpuBootTrace`, after the oracle itself and
`scripts/mesen2_cpu_boot_trace.lua`; `cpu_boot_trace_diff` reads it unmodified.
The comparison tooling does not know one side is hardware.

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
and compare checkpoints every 4096 cycles (`DEFAULT_INTERVAL`) - **~480 KB** for
a full run at 16 bytes a checkpoint. On the first mismatch, re-run only that
window with full capture and waveforms. Implemented in
`crates/rustynes-cosim/src/checkpoint.rs`; measured on a real export, 3 frames of
AccuracyCoin (89,343 CPU cycles) is 5,372,427 bytes of `irq.csv` against 352
bytes of `ckpt.bin`.

#### What is hashed, and what deliberately is not

`CycleRecord` carries many fields, and most of them are *RustyNES's model*, not
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

**The IRQ line is one wire.** `CycleRecord` splits its samples into
`irq_pending_mapper_*` and `irq_pending_apu_*`, which is RustyNES *attributing*
the assertion to a source. Hardware has a single wire-OR'd /IRQ input and cannot
make that distinction, so the pairs are OR'd before hashing. Hashing them apart
would fail a correct DUT for disagreeing about something it cannot observe.

**`pc` is DUT-observable, not pin-observable.** The 6502 does not expose its
program counter. It is in because the testbench wrapper can expose the internal
register and rung 1 compares it directly - but a `pc`-only mismatch means
something weaker than a bus mismatch.

`ppu_scanline`, `ppu_dot`, `ppu_frame` and `a12_events` are out of the hash.
A12 transitions are observable on the cartridge connector, so they are excluded
for **scope**, not observability; the PPU's address bus is compared through
`.fetch.bin` instead.

#### The hash must be reimplementable in ten lines of C++

The format-packing mismatch masquerading as an RTL bug is the risk this guards.
So the hash is **FNV-1a 64** - chosen because a testbench can reimplement it
without a library - and `Observable::encode` defines a fixed **16-byte
little-endian** layout with an explicit zero pad byte, so the C++ side cannot
hash uninitialised struct padding. Both are pinned to a hardcoded vector by
`the_wire_encoding_is_pinned_to_a_fixed_vector`; a reordered field fails that
test rather than producing a phantom RTL defect.

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

#### Checkpoints must agree with full capture

Checkpoints approximate "where do these two runs first differ", traded for four
orders of magnitude of disk, and the scheme is worthless if the approximation
can disagree with the answer. `first_full_capture_difference` computes the answer
directly; `localisation_is_consistent` states the contract as a function.

| full capture says | the comparison must |
|---|---|
| identical | report `Identical` - anything else is a false positive, and a gate that cries wolf gets switched off |
| first differs at `k` | **not** report `Identical` - that false negative passes a wrong DUT |
| first differs at `k`, and it reports `Diverged` | name a window **containing `k`** |
| first differs at `k` | `Inconclusive` is an acceptable, honest refusal |
| identical | `Inconclusive` is **not** acceptable |

A divergence report naming the wrong window sends a full-capture re-run
somewhere nothing is wrong and returns "no problem here", which reads as evidence
the DUT is fine. A sweep in `checkpoint.rs` drives every run length around the
interval boundary, corrupting a different observable field at a different
position each time. The window before the first checkpoint is `Option<u64>`
(`None` = no prior checkpoint), so a divergence at cycle zero is located inside
its window, and `Divergence::contains` is offered so call sites do not
reimplement a boundary that is half-open at one end and open-ended at the other.

#### The CSV cannot re-derive the checkpoints, so `.obs.bin` exists

`irq.csv` does not carry `pc` or `put_cycle_post`, so an external testbench
reading the CSV cannot reproduce the checkpoint hashes. `<stem>.obs.bin` is
repeated 16-byte records in the **same wire encoding the hash folds**,
headerless: the only artifact the checkpoints can be independently re-derived
from, and the input a re-run of a located window consumes. The CSV is kept
unchanged, because `scripts/irq_trace_cross_diff.py` and the committed
`crates/rustynes-test-harness/golden/irq_trace/*.csv` depend on its shape.

`Observable::decode` is the inverse, and it **refuses what it does not
understand**: a non-zero reserved pad byte, an undefined flag bit, an unknown
bus-access code, a short record, a stream length that is not a multiple of 16.
Reading a record from a newer producer as though nothing had changed is how a
*format* divergence gets reported as a *DUT* divergence.

The stream is emitted even when the checkpoints are refused for overflow. A hash
over a truncated trace claims a coverage it does not have; the records
themselves are just records, and are worth keeping for a re-run.

#### A capacity-limited trace refuses rather than truncating

`IrqTrace::push` drops records once it reaches the capacity it was armed with,
advancing an `overflow` counter. A checkpoint stream computed over a
dropped-record trace hashes *fewer cycles than it claims*, and the two sides then
disagree for a reason that has nothing to do with the DUT. So
`Oracle::take_checkpoints` returns `CheckpointError::TraceOverflowed`,
`rn_write_checkpoints` returns `-5`, and the exporter aborts rather than writing a
short stream. The APU trace is refused the same way, before anything is written.

#### One take, two artifacts

`Bus::take_irq_trace` **moves** the trace out, so asking for the CSV and then the
checkpoints yields `None` for whichever came second - and `None` is
indistinguishable from "never armed". `Oracle::take_irq_artifacts` derives both
from a single take; the hazard is pinned by
`taking_the_csv_first_leaves_no_trace_for_checkpoints`.

## What is a gate and what is diagnostic

| Surface | Role | Why |
|---|---|---|
| nestest CPU fields | **gate** | independent oracle - RustyNES is 0-diff against the Nintendulator log |
| per-cycle bus / IRQ samples (`.obs.bin`, `.ckpt.bin`) | **gate** | where 6502 implementations actually die |
| per-dot PPU addresses (`.fetch.bin`) | **gate** | real pins, so a correct chip cannot differ |
| `index_framebuffer` | **gate** | pre-palette, so palette cannot masquerade as rendering |
| integer channel levels (`.apu.bin`) | **gate** | 0-15 per channel, 0-127 DMC |
| AccuracyCoin RAM status vector | **gate** | compared entry for entry, including `Skipped` / `NotRun` |
| `ppu-state-trace` FSM fields | **diagnostic only** | encodes RustyNES's *modelling choices*, not hardware facts |
| mixed `f32` audio output | **diagnostic only** | the non-linear mixer and BLEP resampler are software artifacts |

The two "diagnostic only" rows are the load-bearing ones. Gating on
`ppu-state-trace` would force the HDL to transliterate a Rust data structure - bad
hardware, and an odd form of self-derivation given the black-box premise. Gating on
mixed `f32` audio would either force the HDL to reproduce a software resampler or
produce permanent unresolvable false failures.

**Every trace field is partitioned into hardware-observable versus model-internal
before any gate reads it.** A diagnostic may explain a failure; it never becomes
a gate.

**AccuracyCoin is a status vector, not a RAM compare.** The sibling's gate reads
the work-RAM dump through `accuracycoin_status`, which decodes it against the
catalog in `accuracy_coin_catalog.rs` and reports by test. Comparing 2 KiB of
work RAM answers a different question wrongly in both directions: it reports
scratch bytes as failures, and it reports two runs that never started the
battery as a pass. An all-`NotRun` vector is refused with a non-zero exit, and
the golden uses a window measured to reach every scored entry. **A pass count is
a claim about what ran**, so the coverage number is read alongside the agreement
number.

## How the ladder is run

The ladder is the sibling's `tb/regress.sh`: every co-simulation gate, run
against a golden directory, ending in one `TOTAL` line of passed, failed and
expected-fail gates, with skips counted and named.

```sh
# In the sibling repository.
tb/fetch-goldens.sh                       # clone the pinned oracle, export goldens/
VERIFY=1 tb/fetch-goldens.sh              # regenerate elsewhere, compare byte for byte
ORACLE=.oracle-pinned bash tb/regress.sh  # on-die ladder, against the pin
ORACLE=.oracle-pinned USE_SDRAM=1 LADDER_JOBS=6 bash tb/regress.sh   # off-die, pooled
```

**The pin.** `tb/fetch-goldens.sh` reads a full 40-hex-digit oracle commit from
`tb/ORACLE_COMMIT`, clones that commit into `.oracle-pinned/` (or uses
`ORACLE_DIR`), and exports every row of `tb/golden-recipes.tsv` into `goldens/`
(gitignored; `GOLDEN_DIR` overrides it). **Pinning is the point rather than a
caveat**: the determinism contract says nothing about trace-format stability, so
an unpinned oracle could turn the sibling red for a reason unrelated to its RTL.
Green means green against one recorded commit, and moving the pin is a
deliberate edit. `VERIFY=1` regenerates into scratch, leaves `goldens/`
untouched, and passes only if every artifact of every stem reproduces byte for
byte - which is how a moved pin is shown not to have moved a golden. Each
release records the pin and the `VERIFY=1` result.

**The goldens are the oracle's output, not a copy in the sibling.** Nothing from
this repository is vendored there: a second copy would be a drifting copy. The
`golden-provenance` gate compares every golden manifest's `rom_sha256` against
the ROM actually in the oracle checkout, and names each STALE or UNRESOLVED
golden rather than folding it into a count.

**The job pool.** `LADDER_JOBS=N` (N > 1) runs up to N gates at once; unset or 1
runs them one at a time. The log is replayed in gate order and the tally is
summed per gate, so a pooled run reads as the serial one. Each gate gets its own
scratch directory, a gate whose Verilator model is not yet built runs alone, and
gates naming the same ROM take a per-ROM lock. The pool and the in-place RTL
mutators (`tb/mutate.sh`, `tb/mutate_apu.sh`, `tb/mutate_gate.py`) exclude each
other through `tb/scratch/rtl-mutation.lock`. The pool's acceptance test is the
serial run's verdict for every gate, on both builds, with a planted failure.

**Expected failures are graded both ways.** `run_xfail` runs the gate in full and
fails the suite **if it starts passing**, so a stale allowance cannot hide
coverage. Checkpoint streams that are not compared are listed in `CKPT_DENY` in
`tb/regress.sh`, each with its reason recorded beside it; where a stream is
denied, the stem is compared another way (per cycle by `tb/obs_diff9.py`, or on
the bus and the cycle of every /IRQ rise by `tb/irq_rise_diff.py`). A per-cycle
allowance fails both ways too: an allowed cycle that stops differing is a
failure.

**Where it runs.** `regress.sh` is not a hosted CI gate and says so: it needs the
goldens (gigabytes) and a cargo build of this repository's `rustynes-cosim`.
The sibling's hosted `ci.yml` runs the selftests, the `sys/` and provenance
checks, the RTL lint and subset check, the module gates and rung 1 against the
pinned oracle, and its job names say it is a subset. The whole ladder runs by
hand or through the sibling's `ladder.yml`, a manual-dispatch workflow on a
self-hosted runner.

**Where the current pass counts live.** Not here: counts go stale. The sibling's
`README.md` carries the current release's ladder row (on-die and off-die,
passed / failed / expected-fail), the run that produced it and the RTL it ran
on; each release's notes in `.github/release-notes/` carry the same figures for
that release.

## The oracle can be wrong

RustyNES passing AccuracyCoin in full is not "matches silicon". Where RustyNES is
wrong, co-simulation will drive the RTL confidently toward its bug.

Every rung is therefore labelled by whether it has an **independent** oracle:
nestest (Nintendulator) and the blargg ROMs do; trace fields with no third-party
counterpart do not, and are advisory only.

**It has happened.** `rtl/cpu_bus.sv` was written from
`nesdev_wiki/CPU_memory_map.xhtml` and `Open_bus_behavior.xhtml` and disagreed
with this emulator only in `$6000-$7FFF`. **The DUT was right.** An NROM board
decodes nothing there, so the window reads open bus;
`crates/rustynes-mappers/src/m000_nrom.rs` allocates 8 KiB of PRG-RAM
unconditionally, the iNES-era emulator default the wiki names as a problem, and
it lists games that break on the WRAM answer, *Low G Man* and *Battletoads &
Double Dragon* among them. Three rules follow:

1. **The ladder can catch the oracle.** That is demonstrated, not asserted.
2. **The response is to record, not to fix on the spot.** Changing it alters
   shipped behaviour on every iNES-header NROM cartridge and needs the NES 2.0
   WRAM-size field, the per-game database and the full accuracy battery. It is
   in [`accuracy-ledger.md`](accuracy-ledger.md) with its citation.
3. **The gate ROM is narrowed rather than the DUT bent.** `busopen045` reads
   `$4020-$5FFF` and not `$6000-$7FFF`, because a gate that fails for the
   oracle's limitation rather than the DUT's teaches the wrong lesson and
   eventually gets switched off.

**A fix that greens the gate is not evidence the fix is right**, on an
oracle-defined compare surface: a compensating delay outside the real cause can
produce the same verdict. And **a green mutation catalog does not establish that
constants are individually right** - only that the combination in the tree is
not detectably wrong on the current stimulus. Cross-checking each constant
against documentation is a separate activity.

**Where disagreements are recorded.** Each place the DUT, this emulator and the
public documentation differed, which one won, and why:

- the sibling's `docs/oracle-vs-documentation.md` - the umbrella three-way
  ledger (the CPU, the PPU, the bus, the cartridge and the system substrate);
- the sibling's `docs/apu-oracle-vs-documentation.md` - the APU's deeper chapter,
  in the same categories; an item closes only when a gate exercises it **and** a
  mutation against it is caught;
- this repository's [`accuracy-ledger.md`](accuracy-ledger.md) - where the oracle
  itself was found wrong.

## Rung status

Strictly ordered: a rung may not start until the one below is green. This is the
state at v3.7.1, from the sibling's `README.md` ("The verification ladder") and
its `docs/rung*.md`; those are authoritative if they disagree with this table.

| Rung | Gate | Independent oracle? | Status |
|---|---|---|---|
| 0 · harness | null-DUT self-diff = 0 across every format | n/a | closed |
| 1 · 6502 functional | nestest 0-diff; per-cycle bus equality over blargg's `instr_test-v5` | **yes** - Nintendulator, blargg | closed |
| 2 · 6502 cycle-exact | per-cycle bus equality + swept interrupt injection (ADR 0038) | partial - blargg `cpu_interrupts_v2` | closed |
| 3 · 2C02 | pre-palette `index_framebuffer` equality, fetch and register gates | no | closed |
| 4 · 2A03 | integer channel levels, **not** the mixed `f32`; bus equality on blargg's battery | **yes** - blargg's APU suites | closed |
| 5 · NROM + AccuracyCoin | status vector identical **entry for entry** | no | closed |
| 6 · MiSTer integration | boots on hardware from one `.rbf` | n/a | **open** - no board has run it; `docs/bringup.md` reads NOT RUN |
| 7 · mappers + SDRAM | per-board bus and frame equality; SDRAM timing against a real part | no | banking green and shipped; the SDRAM path is unverified on hardware |

Rungs 0-5 are pure simulation and need no hardware. Rung 7's SDRAM controller
runs the headline build and its ladder, but its pin constraints are provisional
and its read capture assumes zero board delay (the sibling's `docs/sdram.md`).

Per-rung records, in the sibling: `docs/rung1-6502.md`, `docs/rung3-ppu.md`,
`docs/rung4-apu.md`, `docs/rung5-accuracycoin.md`, `docs/rung6-integration.md`,
`docs/rung7-mappers.md`. How rungs 1 to 5 were closed, release by release, is in
[`archive/mister-history-v2.5-to-v3.0.md`](archive/mister-history-v2.5-to-v3.0.md).

## Where to read more

- **The sibling repository**,
  [`doublegate/RustyNES_MiSTer`](https://github.com/doublegate/RustyNES_MiSTer)
  (private): `README.md` (current features, the two builds, the current ladder
  row, installation), `docs/provenance.md`, `docs/bitstream-release.md`,
  `docs/golden-fetching.md`, `docs/rtl-subset-policy.md` (the Quartus 17.0.2
  subset, fitted rather than read), `docs/bringup.md`.
- **Agent notes:** [`agents/mister-cosim.md`](agents/mister-cosim.md) - measured
  findings and traps for this boundary, each with its evidence.
- **History:**
  [`archive/mister-history-v2.5-to-v3.0.md`](archive/mister-history-v2.5-to-v3.0.md)
  (frozen) for v2.4.1 to v3.0.x, and `CHANGELOG.md` after that.
- **Each bitstream pair's md5, seed and timing margins:** that release's notes in
  [`.github/release-notes/`](../.github/release-notes/), and the sibling's
  `README.md` and `CHANGELOG.md`.
