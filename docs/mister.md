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

## The ladder becomes something a reviewer can run (v2.6.15)

Rung 6 is blocked on hardware and v2.7.0 now waits for it, so this release is
about the other half of a submission: not what the core does, but what a
reviewer can **check** about it.

The contributing page states the bar for AI-assisted code in one sentence --
*"Fully AI generated code should meet a minimum reasonable bar for readability
and include some evidence of quality and accuracy testing."* This programme's
evidence is 142 co-simulation gates with a mutation record apiece. And
`tb/regress.sh` says in its own header that it *"is NOT a CI gate and cannot
be"*, because it needs the oracle's goldens and a cargo build of a crate in
another repository. So the strongest thing here was a set of **documents
describing checks a reader cannot run**.

`docs/golden-fetching.md` in the sibling specified the fix and carried
`Status: NOT BUILT`. The smallest end-to-end slice is now built: the nine
opcode-group ROMs export from a **pinned** oracle commit (`tb/ORACLE_COMMIT`)
and compare in CI. It is a subset -- no PPU, no APU, no AccuracyCoin, no nestest
-- and the job's name says so, because a job that looks like a gate and is not
is the defect this programme keeps finding in itself.

**Pinning is the point rather than a caveat.** The determinism contract covers
the framebuffer and the audio and says nothing about trace-format stability, so
an unpinned oracle could turn the sibling red for a reason unrelated to its RTL.
Green there means green against one recorded commit, and moving the pin is a
deliberate edit.

### The first independent interrupt oracle

`docs/rung5-accuracycoin.md` carries the sentence this closes: *"Rung 4 had
blargg as an independent check and it found six defects no self-written gate
could see; rung 5 has no equivalent, and that is the single most important
sentence in this document."* Interrupts had the same hole -- every interrupt
gate compares the DUT against RustyNES, so a shared error between them is
invisible by construction.

`cpu_interrupts_v2`'s five single-purpose ROMs are now verdict gates, on
**mapper 0** (the combined ROM is mapper 1; the singles are not, which removes
MMC1 as a variable). `5-branch_delays_irq` is the sharpest of them: *"A taken
non-page-crossing branch ignores IRQ during its last clock, so that next
instruction executes before the IRQ."* That is the exact behaviour v2.6.7
changed in the **oracle** -- caveat C6, `skip_irq_sample_q` -- from documentation
reasoning alone, with no ROM adjudicating it. This one adjudicates it, on both
consoles.

Alignment is **recorded rather than assumed**: blargg's readme says
`2-nmi_and_brk` *"Occasionally fails on NES due to PPU-CPU synchronization"*, so
its verdict is alignment-sensitive by design and a pass is a pass at the shipped
alignment, not an absolute one.

### `T-ORACLE-001`'s opening claim is retracted

The ticket says RustyNES never clocks the MMC3 counter on the pre-render line.
It does. `mmc3_test_2/2-details` sub-test 8 is, verbatim, *"Counter should be
clocked 241 times in PPU frame"* -- 240 visible plus the pre-render line -- and
RustyNES passes it, as it has every release.

The claim came from `--ppu-state-trace`, and the ticket's own *instrument traps*
section says why that instrument cannot answer the question: it carries no CHR
address column, so it reports what the sprite state was and never what address
was driven. **A trace that could not see the event was read as evidence the
event did not happen.** This is the second time an instrument has been mistaken
for its subject here -- v2.6.9's `apuconflict039` was the first, where nine
divergences "by design" were a defect in the testbench.

The verdicts are unchanged: the oracle still fails `4-scanline_timing` at
sub-test 3 and the DUT at sub-test 12, so on that ROM the DUT is still the more
accurate of the two and `mapper4mmc3irq065` stays unregistered. Only the
explanation was wrong.

### Three claims that became checks

`sys/` verbatim rested on **one measurement taken at v2.6.6** plus a manual
procedure nothing ran -- eight releases, several of them touching the build. It
now pins all 57 files and catches a changed, a missing **and a stray** file; the
third mode is why the directory is enumerated rather than the manifest merely
walked, and it is not hypothetical, since `sys/README.md` and `sys/.gitkeep`
both lived there through v2.6.5. Re-verified against upstream in the same pass:
`Template_MiSTer`'s HEAD **is** the pinned commit and a clone reports 0
differences -- two claims, measured separately, because a vendored tree can be
faithful to a commit upstream has moved past.

The `.qsf` published **two** seed sweeps disagreeing about the pinned seed's
margin by 0.155 ns, one of them a number the current RTL cannot reproduce. And
the bitstream **name** turned out not to be a style question at all: the
distribution builder skips any file whose stem does not end in an underscore
plus eight digits, so a version-named core would have appeared in the Cores
table and shipped nothing.

## Rung 4 OPENS: the pulse channels and the frame counter (v2.5.9)

Both pulses -- timer, 8-step duty sequencer, length counter, envelope, the
sweep MUTE -- plus the frame counter in both modes with its IRQ and the
`$4015`/`$4017` register file. Triangle, noise and DMC are v2.6.0/v2.6.1.

**The partition was fixed before the rung** (the sibling's `docs/rung4-apu.md`),
because the APU is the hardest chip here to gate honestly: what it produces is
an analog level and what an emulator computes is a number. Gates are the
`$4015` read value, the `/IRQ` pin and each channel's **integer** DAC input;
`MixRecord`'s `f32` mix fields, the sequencer step index and `apu_phase` are
diagnostics. `--apu-trace` on `nes_golden_export` exports the integer levels
only.

**The stimulus measurement found four ROM defects before any gate ran** --
length index 3 is 2 and not 254, the 6502 boots with I set so `CLI` is
mandatory, two channels at one volume are indistinguishable, and power-on work
RAM is seeded rather than zeroed. Four findings in the DUT: the duty sequencer
counts UP, the 4-step constants must be consistently 0-based, `$4017` bit 7
clocks a quarter and half frame IMMEDIATELY, and the `$4017` reset delay depends
on bit 7 -- which the wiki's "3 or 4 CPU clock cycles" does not settle.

`apulen027` is exact on both surfaces; `apupulse026`'s bus surface is 3 and its
channel levels 1,000 -- **500 runs of exactly two cycles, one per edge**, a
uniform one-tick `$4003` write-parity sensitivity the first stimulus hid. Two
fixes tried, both rejected by measurement. v2.6.0's first item. **Nine of ten
mutations CAUGHT**, the two exceptions both indicting the stimulus.

## Rung 7 opens: the cartridge, and two PPU defects A12 found (v2.6.9)

**Five boards land** -- UxROM (2), CNROM (3), AxROM (7), MMC1 (1) and MMC3 (4)
-- in one `rtl/cart/cart.sv` whose mapper number is a runtime INPUT, because on
MiSTer the header arrives with the game and a parameter would need a different
bitstream per cartridge. All five match the oracle on every cycle and every
checkpoint, and nine of nine mutations are CAUGHT.

**"Five" here is rung 7's boards, not the console's.** NROM landed at rung 5, so
the core supports **six**: NROM, MMC1, UxROM, CNROM, MMC3 and AxROM. Both counts
are correct and they are easy to reconcile wrongly -- a reviewer did, reading
this line against the sibling's six commercial captures.

**Half the earlier "rung 7 is blocked" claim was wrong, and this corrects it.**
Rung 7 has two parts and only one ever needed hardware: the SDRAM controller,
whose acceptance is read/write timing against a real part. The mapper logic is
pure logic, verifiable exactly as rungs 1-5 were. Treating them as one item
deferred five boards behind a blocker that never applied to them.

**The MMC3 scanline counter is the first thing in this core's history to consume
PPU A12, and it found two PPU defects on its first day** -- both invisible for
four releases because nothing had looked:

- `dummy_fetch` named the RECORD dots (337, 339), not the fetch. A fetch is two
  dots, so on the even dots the address fell through to `v_addr` and the PPU put
  `v` itself on the CHR bus; `v` bit 12 is `fine_y[0]`, which alternates every
  scanline.
- The **idle dot**. With the first fixed the extras moved rather than vanished,
  to dot 0 -- the only dot of a rendering line that drives no fetch, and so the
  only one still reaching the same fallback.

Filtered A12 clocks went 1,445 -> **965, every one at dot 261**. The wiki
gives 241 clocks per frame, and `965 = 241 x 4 + 1` -- four frames plus one
clock, the run not ending on a frame boundary. Interrupts taken went 218-vs-158 to
**158 and 158**, with byte-identical work RAM, and the DUT/oracle divergence
from 10,821 cycles to 950.

**The residual was two things wearing one appearance, and blargg separated
them.** `mmc3_test_2` scores **4 of 6 on the DUT and 4 of 6 on the oracle,
failing the same two ROMs** -- `6-MMC3_alt` correctly, since it is NEC rev B and
both target Sharp rev A. Within `4-scanline_timing` the oracle fails sub-test #3
(its documented, permanently-deferred 1-cycle bracket, ADR 0002 F5.0) and the
DUT fails #2, *"Scanline 0 IRQ should occur later when `$2000=$08`"*.

So part of the offset is the oracle's own residual, which must NOT be fitted to
-- that is the failure the v2.6.0 audit named. The other part is a real DUT
defect, **shown pre-existing by control**: with both PPU fixes reverted it still
fails #2. It is left open with a 27-second reproducer and a validated pass/fail
signal, rather than tuned away against a reference that is itself off on the
same axis.

## An exclusion hides improvement as well as regression (v2.6.9)

The co-simulation suite gates most goldens on a rolling per-cycle **checkpoint**
hash, and carried a **deny list** of streams excluded from it. v2.6.8 established
that such a list is an *assertion about the thing under test* — one that v2.6.7
had invalidated twice over by changing both the DUT and the harness — and
re-measured six entries, retiring four. v2.6.9 goes one level down, to the
mechanism, and finds two things.

**First, "by design" was a claim about the instrument.** `apuconflict039`'s bus
surface had been excluded since v2.6.2 under a note saying it "carries nine
divergences **by design**". **Six of the nine had already closed** and nobody
could see it, because a denied stream is denied in both directions. The **three
that remained** were a defect in the **harness**: on a cycle
where the CPU is held, `tb/cpu_main.cpp` built the trace record's `bus_data` from
a stale local rather than from the RTL's own open-bus latch. The two differ on
exactly one rule, and it is a rule the RTL already implements correctly — a read
of the APU status register is *internal to the 2A03* and does not drive the
external lines, so the latch holds across it while a local tracking what the CPU
*received* does not. Reading the latch makes the stream **identical on all
357,361 overlapping cycles and all 88 checkpoints**.

The release plan predicted a **DUT defect** here and reasoned from the correct
rule to get there. The reasoning was sound, the rule was right, and the defect
was one layer further out — which is the standing lesson: a measurement
disagreeing with a rule you are confident in can indict the measurement.

**Second, a denied stream is denied ENTIRELY**, so a golden whose divergence is
one cycle out of 357,361 forfeits the other 357,360 — and an entry that silently
*improves* is exactly as invisible as one that silently regresses.

The planned fix was an allowance by **checkpoint index**, and it was implemented,
run, and **refuted**: `tb/checkpoint.h` chains its FNV-1a, so one divergent cycle
poisons every checkpoint after it. Allowing the first differing window moved the
failure to the next one; allowing the rest is the all-or-nothing deny it was
meant to replace. The allowance moved to a new **per-cycle** nine-field
comparator, `tb/obs_diff9.py --allow-cycle`, where an attributed difference costs
**one cycle instead of seventy-one checkpoints**.

It fails **both ways**, which is what makes it adoptable: an allowed cycle that
stops differing is a FAILURE, so an improving DUT cannot leave a stale allowance
quietly hiding coverage. Six mutations confirm it, including a cycle named
outside the compared window being **refused** rather than allowed to match
nothing.

The one remaining entry, `ppuoamcorrupt052`, differs on exactly one cycle —
70,627 — which is the documented OAM-corruption asymmetry where **the DUT
implements more of the documented rule than the oracle does** and no available
gate can adjudicate. It is allowed and named, not resolved.

## The bitstream is published from v2.6.7 (maintainer decision, 2026-08-30)

**Every release from v2.6.7 ships a `.rbf`** — committed to the sibling's
`releases/` and attached as an asset to the GitHub release on **both**
repositories. This reverses v2.6.6, which produced a bitstream and deliberately
did not publish it.

**Why the reversal is right.** The MiSTer distribution mechanism reads
`releases/RustyNES_MiSTer-vX.Y.Z.rbf` out of the *repository*, so an empty
`releases/` does not describe a cautious core — it describes an undistributable
one, withheld from exactly the people who own the boards this project does not.
And a claim nobody made is not the same as a claim marked unverified: only the
second is usable. So the caution moves from an absence into a disclosure.

**What every release body must therefore state**, because it is what the ladder
does and does not reach:

- **No hardware has run the bitstream** (true until rung 6 closes). A booting
  core, a synced display, audible audio and a working controller are not claimed.
- The co-simulation ladder establishes per-cycle agreement with a 141/141
  emulator on the declared compare surfaces, and AccuracyCoin agreement entry for
  entry across all 146 entries.
- **Unverified by construction**: the PPU gate compares the *pre-palette* index
  and the APU gate compares *per-channel integer levels*, so the palette, the
  video timing constants, the audio's absolute level and its band-limiting are
  downstream of every gate. That partition is deliberate — it is what stops a
  palette difference masquerading as a rendering one — and its price is that
  those four properties have no evidence behind them until a board runs the file.
- The Quartus version, device, error and warning counts, worst setup and hold
  slack, and the **pinned fitter seed**. The seed is load-bearing: two compiles of
  identical RTL have landed a framework HDMI path at +0.386 ns and −0.086 ns, so
  "timing closes" without a pinned seed is a statement about one placement.

**Mechanism.** `scripts/release-rbf.sh <tag>` in the sibling builds, verifies and
uploads. It checks errors and per-clock slack **against the reports rather than
the exit code**, because Quartus has been observed to abort after the resource
summary and still exit 0. Full procedure and rationale:
`RustyNES_MiSTer/docs/bitstream-release.md`.

**One artifact, two boards.** The SuperStation One is a Cyclone V console that
forks `Distribution_MiSTer` and consumes MiSTer cores directly, so the same
`.rbf` is what both take. Whether the *identical file* boots both is a hardware
claim and stays deferred.

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

**Reproducing it:** none of these is a CI gate, so the exact invocations and
the non-zero count each prints are recorded in the sibling's
`docs/rung3-ppu.md` under *Reproducing v2.5.8, exactly* -- including that
`--irq-trace` is **mandatory** for every PPU golden export, without which no
`obs.bin` is written and the bus gate cannot run at all.

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

### It happened, at rung 5 (2026-08-25)

This stopped being a stated risk and became a measurement.

`rtl/cpu_bus.sv` was written from `nesdev_wiki/CPU_memory_map.xhtml` and
`Open_bus_behavior.xhtml` and run against this emulator. The two agreed on
`$4016`, `$4017`, `$5000` and `$5C34` — including the open-bus value, which the
DUT *derives* from a latch where the testbench had hardcoded `$40` — and
disagreed only in `$6000-$7FFF`.

**The DUT was right.** An NROM board decodes nothing there, so the window reads
open bus. `crates/rustynes-mappers/src/m000_nrom.rs` allocates 8 KiB of PRG-RAM
unconditionally so accesses "don't fall off the edge", which is the iNES-era
emulator default the wiki names as a problem in its own words — and it lists
games that break on the WRAM answer, *Low G Man* and *Battletoads & Double
Dragon* among them.

Three things this establishes, in order of how much they matter:

1. **The ladder can catch the oracle.** That was asserted when the programme was
   planned and is now demonstrated, which is a different kind of claim.
2. **The correct response was to record, not to fix.** Changing it alters
   shipped behaviour on every iNES-header NROM cartridge and needs the NES 2.0
   WRAM-size field, the per-game database and the full accuracy battery. It is
   in [`accuracy-ledger.md`](accuracy-ledger.md) with its citation.
3. **The gate ROM was narrowed rather than the DUT bent.** `busopen045` reads
   `$4020-$5FFF` and not `$6000-$7FFF`, because a gate that fails for the
   oracle's limitation rather than the DUT's teaches the wrong lesson and
   eventually gets switched off.

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

### Rung 1 gets an independent oracle (v2.6.3)

The whole rung above was measured against ROMs **written in this project**, and
the section before this one says exactly what that cannot cover. That limitation
is now partly lifted.

blargg's `instr_test-v5/rom_singles` — sixteen third-party ROMs, ~2.68 M cycles
each, predating this programme by two decades and between them exercising all
256 opcodes — runs as a standing section of the sibling's `regress.sh`, compared
on rung 2's per-cycle bus surface. **16 of 16 exact**, taking the suite from 50
gates to **66**.

It is the first *independent* oracle rung 1 has had, and it earned that
description immediately by finding **three defects the self-written corpus had
missed**, none of them in the undocumented opcodes the battery was run to
validate:

| ROM | defect |
| --- | --- |
| `06-absolute` | `RRA` fed its `ADC` stage the carry from *before* the instruction rather than the one the rotate produced |
| `08-ind_x` | the 8-cycle indirect RMW forms addressed the indexed target during their *pointer* fetch cycles |
| `03-immediate` | the PPU I/O-bus latch never decayed — a 2C02 defect, reached from a CPU ROM, three rungs after rung 3 closed |

The `RRA` finding is the one worth carrying forward, because of what it says
about compare surfaces rather than about the 6502. **The instruction's own bus
trace was identical on both sides** — read `$FF`, dummy-write `$FF`, write
`$7F` — and the divergence appeared nine cycles later in the `STA` that spilled
the accumulator, off by exactly one. A gate on the memory side of
read-modify-write would have passed it, and so would this rung's own
register-boundary trace had the next instruction not stored the accumulator
straight away.

Two hooks were needed to run third-party ROMs at all, and both are about **not
transcribing a number**: the window comes from each golden's own manifest rather
than a `CYCLES_<rom>` line, and the board's `$6000-$7FFF` work RAM is presented
per-ROM (blargg reports through it) rather than by default — NROM has none, which
is the oracle defect recorded above.

## Rung 5 — the console, and the master-clock substrate

NROM, the work RAM, the CPU bus, the controller ports and DMC DMA are landed and
gated in the sibling; `nes_top` assembles them, carries **no observation ports**,
and as of v2.6.3 **divides one 21.477272 MHz master clock** rather than taking
its clock enables from the testbench.

### The divider is this repository's own substrate, in SystemVerilog

The obvious shape — a modulo-`CPU_DIV` phase counter with the dot at
`phase[1:0] == 3` — looks equivalent on NTSC and is a dead end. PAL is 16 master
clocks per CPU cycle and 5 per dot, i.e. **3.2 dots per CPU cycle**, and no
counter that restarts each CPU cycle can hold a fractional ratio.

`Bus::run_ppu_to` does not do that. It keeps `master_clock` and `ppu_clock` as
**independent accumulators in master-clock units** and emits a dot whenever
`ppu_clock + ppu_divider <= master_clock`, letting dot boundaries drift across
the CPU cycle — which is exactly what PAL does. The DUT copies that structure, so
retargeting is a parameter change (NTSC 12/4, PAL 16/5, Dendy 15/5).

Two constants come from `Cpu` rather than from a sweep. `read_split(12)` is
(5, 7) and `write_split(12)` is (7, 5) — `CPU_DIV/2 ∓ PPU_OFFSET` — with the PPU
run to that point minus `PPU_OFFSET`. On NTSC a read observes at master clock 5
and a write commits at 7, and **both fall inside the same dot**, which is why the
DUT's earlier read/write placement sweep measured them onto one dot: this model
predicted it. `PPU_OFFSET` is also applied as a real phase offset between the
accumulators, so a cycle's last dot commits *before* the CPU rather than on the
same edge.

### What it found: an enable that is constant is not an enable

The DUT's testbench used to tie `ce` high and pulse `clk` once per CPU cycle. The
clock was doing the gating the enable was supposed to do, so **any `always_ff`
not gated by `ce` was correct only by accident** — under a real master clock each
fires twelve times. Four sites, two of them previously unknown:

| site | symptom |
|---|---|
| PPU CPU-register block (v2.5.7) | a held address latched twelve times per access |
| open-bus decay reload | refresh never observed; `$2002` read `$00` for `$1F` |
| DMC DMA acknowledge | the sample pointer advanced by **twelve** per byte; 91% of cycles diverged |
| frame-counter IRQ set points | `/IRQ` rose eleven master clocks early; the CPU took the interrupt **one instruction sooner** |

The last was caught by blargg's `08.irq_timing` — third-party, so an independent
oracle rather than our own trace agreeing with itself.

### A fix that worked, and was rejected

Delaying the APU's `/IRQ` by one cycle in `nes_top` also produced 66 of 66, and
is **indistinguishable from the real fix by gate result**. It was not adopted:
`Cpu::handle_interrupts` samples IRQ at phi2 into `mc_run_irq` and dispatches on
`mc_prev_run_irq` — the one-cycle register this codebase calls
"second-to-last-cycle recognition" — and the DUT's `cpu6502.sv` already
implements exactly that, correctly gated. A second delay outside it would have
cancelled an APU-side error rather than corrected it.

**This is the general hazard of an oracle-defined compare surface**, stated once
so rungs 6 and 7 inherit it: a fix that greens the gate is not evidence the fix
is right. Continuing to look for a cause after the symptom cleared is the only
thing that separated the two here.

### Where the three-way disagreements are recorded

The sibling's `docs/oracle-vs-documentation.md` is the umbrella ledger: per
subsystem, where the DUT, this emulator and the public documentation differed,
which won, and why — using the same six categories as the APU chapter that
preceded it. It carries the one entry so far where **this emulator** was the one
that was wrong (NROM's PRG-RAM window), and the sharpest category-1 entry yet:
the PPU's open-bus decay deadline, where the documentation says 3-30 ms, this
emulator uses 558.7 ms, and the DUT's corpus forces >= 523.4 ms.

### AccuracyCoin runs end to end, and the gate is a status vector

The full run now completes on the DUT — **17,868,316 cycles**, where it
previously halted early — which is what "first end-to-end AccuracyCoin run"
in the plan's v2.6.3 row asked for.

The comparison is `accuracycoin_status`, on this side. It reads a work-RAM
dump, decodes it against the 146-entry catalog in
`accuracy_coin_catalog.rs`, and reports by test rather than by address. It
filters in both modes rather than dumping all 146 rows: given one dump it
prints the entries that are **not a clean `Pass`**, and given two it prints only
the entries where they **disagree**. The vector is decoded in full either way --
what is filtered is the output, not the comparison. First
measurement: **137 of 146 entries agree, 9 differ**, six of those sharing one
failure code — five `SH`-group stores and Open Bus — which reads as one shared
address-bus cause rather than six independent defects.

**Producing the vector is v2.6.3's deliverable. Making the two agree is
v2.6.4**, and the plan says so in its own acceptance row.

**The "six sharing one failure code" reading was wrong, and v2.6.4 measured why.**
AccuracyCoin's `TEST_Fail` reports `(ErrorCode << 2) | 2` and the runner sets
`ErrorCode` to 1 before *every* test routine, so the code is an index **within
one routine**: `Open Bus`'s code 7 is its own seventh assertion and `SHA (abs),Y`'s
code 7 is that routine's seventh. Two entries sharing a code share nothing. What
actually closed the five `SH` entries was the SH group's RDY-conditional store
and its addressing-mode-dependent dummy cycle — a real shared cause, identified
from the opcodes. `Open Bus` was untouched by that work and remained, which
should have refuted the shape argument at the time.

**And the number underneath the agreement.** Once all nine closed, the vector
reported identical entry for entry across all 146 — with **58 of those entries
`NotRun` on both sides**. The comparator was right; the run window was short.
Broken down by suite, 600 frames reaches the CPU catalog and stops partway
through `CPU Interrupts`, so the run asked the DUT nothing about the APU, PPU,
sprite-evaluation or PPU-misc suites — the chips rungs 3 and 4 exist for.
Measured rather than estimated, **4500 frames reaches all 146** (134,012,761
cycles), and that is the golden the gate now uses. A pass count is a claim about
what ran, and what ran has to be measured separately.

Decoding the codes properly is also what closed two of the last three, in
v2.6.4: `Open Bus` (a `$4015` read does not drive the data bus, and its D5 is
open bus) and `Interrupt flag latency` (the interrupt poll is the second-to-last
cycle, and branches poll before cycles 2 and 4 but never before 3). Both rules
are stated by the test ROM's own comments and by neither of the nesdev pages the
implementation was written from. `NMI Overlap BRK` is carried to v2.6.5 with its
disagreement measured per sweep step rather than as one byte — see the sibling's
`docs/rung5-accuracycoin.md`.

Two properties are worth recording, because both are about what the comparison
*refuses* rather than what it reports:

- **It is not a RAM byte-compare, deliberately.** Comparing 2 KiB of work RAM
  answers a different question and answers it wrongly in both directions: it
  reports scratch bytes — a stack slot, a loop counter, a result the ROM is
  about to overwrite — as failures, and it reports two runs that never started
  the battery as a *pass*, because two idle title screens have identical RAM.
- **An all-`NotRun` vector is refused with a non-zero exit.** That case — two
  vectors agreeing on 146 entries of nothing — is precisely the shape of the
  vacuous status-address assertion v2.6.2 found in the NTSC blargg suite, which
  reported 11/11 for five minor releases while asserting nothing. A comparison
  that cannot distinguish *agreed* from *never ran* is not a gate.

Reaching the run cost **two false passes before a real one**: AccuracyCoin idles
on its title screen until START is pressed, and only the framebuffer half of the
export had a guard that refused an idle capture. The manifest now records
`press_start` — as `A:B`, or the literal `none` — because a controller press
changes what the ROM *executes*, and a manifest omitting it describes an idle
title screen and an 88-result battery identically.

## Rung 4 — the 2A03, and the audit it prompted

Rung 4 opened at v2.5.9 with the two pulse channels and the frame counter, and
**v2.6.0 "Assay"** adds the triangle, the noise channel and the sweep unit. Eleven
gate ROMs, 35 of 35 mutations CAUGHT, and 30 gates green across rungs 1–4.

Detail lives in the sibling repository (`docs/rung4-apu.md`), but two things
belong here because they are about the *programme*, not about the APU.

### The oracle is an emulator, and rung 4 is where that stopped being abstract

Every rung-4 gate compares the DUT against RustyNES. A shared error between the
two is therefore invisible **by construction**: the DUT is tuned until it agrees,
and agreement with a wrong reference is indistinguishable from correctness.

`docs/apu-oracle-vs-documentation.md` in the sibling repo is the response —
a standing ledger of every place the DUT follows the oracle rather than the NESdev
wiki, sorted by risk, each with the documentation text it is measured against and
the independent check that would adjudicate it. Its maintenance rule is the load-
bearing part: **an item closes only when a gate exercises it *and* a mutation
against it is CAUGHT.**

The independent check was run at v2.6.0 for the first time: **the oracle passes
blargg's APU battery 29/29**. That is recorded per item rather than as a headline,
because it means different things in different places — for two items it moved
suspicion off the oracle and onto the RTL, and for one there is no adjudicating
ROM at all, which is itself the finding.

### Two errors that were cancelling, and what that says about mutation testing

The `$4017` reset delay had been keyed on the mode bit, which the wiki never
mentions. It was exact *only* in combination with a second constant held one tick
off its documented value; either correction alone costs 2 cycles, in opposite
directions. The fitted rule was not merely unfalsified by the stimulus — it was
**load-bearing for a second error**, which is why it survived both a mutation
catalog and a documentation audit that looked directly at it.

This is the third time the programme has found two errors cancelling (v2.5.7's
`PPU_LEAD`, v2.6.0's own observation point), and the first where one propped up
the other rather than merely coinciding with it. **A green mutation catalog does
not establish that constants are individually right** — only that the combination
in the tree is not detectably wrong on the current stimulus. Cross-checking each
constant against documentation is a separate activity, and rung 4 is where the
programme learned to do it.
