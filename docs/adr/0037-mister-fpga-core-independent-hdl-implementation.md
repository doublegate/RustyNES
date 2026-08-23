# 37. A MiSTer FPGA NES core: an independent HDL implementation verified against RustyNES, not a port

Date: 2026-08-20

## Status

Accepted. Opens the **v2.4.1 - v2.5.0 "Fabric"** line and the v2.6-v2.9 programme
that follows it. Extends the reference firewall of
`docs/ai-emulator-provenance-guardrails.md` from emulator source to **HDL source**.
Does not supersede any ADR.

## Context

The request was for "an initial MiSTer FPGA core port from RustyNES". Research
turned up five facts that shape what is actually buildable, each stated here
because the decision is built around them rather than despite them.

**There is no port path.** A MiSTer core is SystemVerilog compiled by Quartus
17.0.2 into a Cyclone V bitstream. Rust does not become a bitstream, and
high-level synthesis of a cycle-accurate emulator's control flow is not a
technique that produces usable hardware. What is buildable is a **new NES
implementation in SystemVerilog, written from public hardware documentation**,
with RustyNES serving as a **verification oracle**. That is the one role RustyNES
is uniquely equipped for and the reason to attempt this here rather than
elsewhere.

**`NES_MiSTer` already exists**, is GPL-3.0, covers ~150 mappers and FDS, and
scores 121/125 on AccuracyCoin -- where real Famicom AV hardware also scores
~121/125. Its four remaining failures are edge PPU behaviour where hardware
agrees with it. There is no published accuracy headroom, and MiSTer's
contribution guidance discourages redundant cores. **A second NES core may be
declined.**

**The Retro Remake SuperStation One is the same target, not a second one** -- a
Cyclone V with 128 MB integrated SDRAM that runs MiSTer cores directly. One
`.rbf` serves both boards. It removes a hardware prerequisite (a DE10-Nano needs
the SDRAM add-on for any NES core, since the NES reads cartridge ROM directly),
and Retro Remake hosts cores itself, so it is also a **second possible home** if
MiSTer-devel declines.

**RustyNES has no per-cycle step.** `Nes` exposes `run_frame()` and
`step_instruction()` and nothing finer. Live cycle-lockstep would require new
core API on the hot path.

**The schedule does not fit by roughly an order of magnitude.** A from-scratch
cycle-accurate NES core with per-cycle gating is 7-13 months of full-time work;
the demonstrated release cadence puts v2.4.1..v2.5.0 at two to four weeks.

## Decision

**1. Write the RTL independently; use RustyNES as an oracle.** Hardware behaviour
is a fact and may be implemented from `nesdev_wiki/`, `ref-docs/`, datasheets,
die studies and RustyNES's own subsystem specs. The specific code expression of
another implementation is copyrighted.

**2. Extend the reference firewall to HDL.** `NES_MiSTer` and `fpganes` `rtl/`
are **strict black boxes** -- never opened, read, quoted or transcribed.
Instantiating a third-party core as an opaque module to compare *outputs* is
permitted; reading its source is not. Anything unimplementable from documentation
escalates to a new ADR **before** any source is opened. This is the same rule
`ref-proj/` already encodes for emulator source, and it is enforced the same way:
the third-party repositories stay physically outside the workspace.

**3. Replay, not lockstep.** RustyNES emits goldens ahead of time; the Verilator
testbench writes the **same byte formats** from the DUT; the diff CLIs that
already exist compare them. The determinism contract -- same seed + ROM + input
yields a bit-identical framebuffer and audio -- makes a pre-recorded trace
*exactly* the trace a lockstep run would have produced, so this is not a
weakening. It is additionally better in two ways: goldens are re-diffable without
re-simulating, and the two sides can run on different machines at different times.
`scripts/mesen2_cpu_boot_trace.lua` already writes `cpu_boot_trace` from a foreign
emulator, so the FPGA testbench becomes the format's *third* writer, not its
first.

**4. No DPI-C.** It would push `import "DPI-C"` into RTL that must also pass
Quartus, then require `` `ifdef SIMULATION `` guards -- the exact construct that
lets a simulated netlist drift from the synthesised one. Observation ports live
in `tb/nes_top_cosim.sv`, never listed in `files.qip`, plus Verilator
`--public-flat-rd` hierarchical reads. Net synthesis impact: zero.

**5. Hash first, capture on divergence.** A 4200-frame AccuracyCoin run is
~125 M CPU cycles, which is ~7.5 GB of per-cycle CSV. Both sides instead chain a
64-bit hash over the per-cycle tuple and compare checkpoints every 4096 cycles
(244 KB for a full run); on the first mismatch, binary-search the window and
re-run only that window with full capture. This is a rung-0 design constraint,
not a later optimisation.

**6. Two repositories.** `crates/rustynes-cosim` here; the RTL in a sibling
`RustyNES_MiSTer`. MiSTer's fixed `rtl/` + `sys/` + `releases/` layout fights
Cargo's, and `sys/` carries its own licence terms.

**7. Scope v2.5.0 to "the 6502 rung closes".** The co-simulation harness plus a
cycle-exact 6502, gated on nestest 0-diff and per-cycle bus equality. PPU, APU and
MiSTer integration become the v2.6-v2.9 programme. Stating this now is better
than discovering it at v2.4.6.

**8. The emulation core is untouched.** **AMENDED by [ADR 0038](0038-cosim-interrupt-injection-api.md) (2026-08-23):** one default-off, feature-gated interrupt-injection API is admitted for rung 2's sweep, under constraints that void the decision if the byte-identity or zero-cost checks fail. The claim is therefore "untouched in the default build", and must be restated that way rather than quoted unqualified. No behaviour change to
`rustynes-{cpu,ppu,apu,mappers,core}`, no new hot-path API. AccuracyCoin stays at
141/141 on the RAM decoder and nestest stays 0-diff. `rustynes-cosim` is additive
and absent from the default build.

## Consequences

**Gained.** The verification apparatus is a deliverable in its own right and
retains its value even if the core is declined as a duplicate -- MiSTer's guidance
now explicitly asks that AI-assisted contributions "include some evidence of
quality and accuracy testing", and per-cycle co-simulation against a 141/141
oracle is a stronger form of that evidence than any existing core can show.
Building the harness also exercises RustyNES's trace formats against a genuinely
foreign consumer, which is how format defects surface.

**Paid.** The black-box rule is hardest to keep exactly where debugging pressure
is highest: when the DUT and RustyNES disagree at dot 260 of scanline 241 and the
documentation is ambiguous, the pull toward reading `NES_MiSTer` is maximal. The
mitigation is mechanical rather than dispositional -- a path denylist and
identifier grep in CI, and the repositories kept off disk.

**Accepted risk: the oracle can be wrong.** 141/141 on AccuracyCoin is not
"matches silicon". Where RustyNES is wrong, co-simulation will drive the RTL
confidently toward its bug. Mitigation: every rung is labelled by whether it has
an **independent** oracle. nestest (against the Nintendulator log) and the blargg
ROMs do; trace fields with no Mesen2 counterpart do not, and are advisory only.

**Accepted risk: cross-repository golden drift.** The determinism contract covers
the framebuffer and audio; it says nothing about trace-format stability, and
`cpu_boot_trace` is at schema version 1 with a history of being reshaped. A
routine RustyNES accuracy fix can therefore turn the FPGA repository's CI red for
a reason unrelated to its RTL. Mitigation: pin the RustyNES commit, encode its
hash in the golden filename, and record provenance in a manifest beside every
golden set.

**Open, with a scheduled experiment.** If any `Template_MiSTer` `sys/` file is
GPL-2.0-**only**, the combined bitstream is undistributable and the RTL must be
GPL-2.0-or-later instead. Tabulating every licence header in `sys/` is an hour of
work and must happen **before any RTL is written** -- relicensing after 10k lines
exist is precisely the failure `docs/originality-and-provenance.md` documents.
