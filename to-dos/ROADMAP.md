# RustyNES — Roadmap

This is the entry point for project planning. Each phase below links to its overview file. Each phase contains sprints; each sprint contains tickets.

The phase bodies preserve the **engine-lineage** development history — the
internal engine line (v0.9.x → v2.x markers) whose increments produced the
RustyNES v1.0.0 technology. Those version markers are historical anchors, not
RustyNES releases of their own; the RustyNES production core shipped at
**v1.0.0**, and the **v1.1.0 → v1.10.0** feature/platform releases ship on top
of it, followed by the breaking **v2.0.0 "Timebase"** release (code-complete
as of 2026-07-03, tag pending).

**RustyNES release line:** `v0.1.0…v0.8.6` (the parent emulator) →
`v0.9.0…v0.9.7` (engine-lineage integration stages — the inbound cycle-accurate
engine being folded in, stage by stage) → **`v1.0.0`** (this synthesis: the
engine + the ported desktop-UX shell + production polish) → **`v1.1.0`
"Scriptable" → `v1.2.0` "Curator" → `v1.3.0` "Bedrock" → `v1.4.0` "Fidelity"**
(+ the `v1.4.1` patch) **→ `v1.5.0` "Lens" → `v1.6.0` "Studio" → `v1.7.0`
"Forge"** (+ the `v1.7.1` patch) **→ `v1.8.0` … `v1.8.9` "Atlas"** (the Android
platform train) **→ `v1.9.0` … `v1.9.9` "Workshop"** (the iOS/iPadOS TestFlight
train) **→ `v1.10.0` "Arcade"** (the native Libretro core) — all additive,
off-by-default feature/platform releases on the v1.0.0 core. The forward path
then landed the breaking **RustyNES `v2.0.0` "Timebase"** (the
one-clock/every-cycle-bus-access scheduler collapse, ADR 0002/0029, shipped
2026-07-03), the **v2.0.1 → v2.0.9 "Harbor"** mobile-finalization train, the
**v2.1.0 → v2.1.10 "Fathom"** accuracy line, the **v2.2.0 "Capstone"** milestone
that closed the "deepen the existing project" run, and the maintenance /
distribution / provenance patches **v2.2.1 → v2.2.5**, the **v2.2.6 → v2.3.0**
de-monetization + NESdev-remediation line, and the **v2.3.1 → v2.3.9** tooling /
measurement / gates line — of which **`v2.3.9` "Crucible" (what the gates actually
cover) is the current shipped tag**. The freed **`v2.3.0`** slot is repurposed as the accuracy/fidelity
capstone of the **v2.2.6 → v2.3.0** "de-monetization + NESdev-remediation" line (see
below). **RustyNES is permanently open-source and income-free (ADR 0035): all planned
monetization is removed and the native apps are kept as free FOSS apps.** A **free**
mobile-app distribution (GitHub sideload today; optionally a free F-Droid / App Store
listing later) may still happen — with no ads, no tracking, and no paid unlock.
Where the detailed sections below carry the inbound engine's own `v1.x`/`v2.x`
tags, read them as upstream engine history (its v2.0–v2.8 line), which maps
onto the integration stages roughly as: engine v1.0.0 → RustyNES v0.9.0;
v1.1.0–v1.4.0 → v0.9.1; v1.5.0–v1.7.0 → v0.9.2; v2.0.0–v2.0.1 → v0.9.3;
v2.1.0–v2.2.0 → v0.9.4; v2.3.0–v2.5.0 → v0.9.5; v2.6.0–v2.7.1 → v0.9.6;
v2.8.0 → v0.9.7; the synthesis itself = **v1.0.0**.

> **Two distinct "v2.0"s — do not conflate them (historical note, both now
> resolved).** The engine-lineage **v2.0** (the master-clock work that took
> AccuracyCoin to **100.00%**) is *upstream engine history* and shipped as the
> **v1.0.0 production core**. The forward **RustyNES v2.0.0 "Timebase"**
> (ADR 0002/0029) was a *different* milestone — the one-clock/every-cycle-bus-
> access scheduler collapse — which **shipped as v2.0.0 on 2026-07-03** (see
> the "v2.0.0 'Timebase' — historical landing snapshot" section below for
> what shipped, including the one known gap: the MMC3 R1/R2
> IRQ-timing residual, by-design-deferred rather than closed). The engine's
> own `v1.x`/`v2.x` markers in the bullets and "Phases" sections remain
> historical anchors, **never** RustyNES release numbers.

## Status

- **Current release:** **RustyNES v2.6.15 "Warrant"** (2026-09-04) — the claims v2.7.0 will make become checkable, and the instrument pays the oracle back. Built on **v2.6.14 "Docket"** (2026-09-03) — the submission checklist becomes auditable, and auditing it finds five boxes already true, two ticked on evidence that expired, and one that could never have been ticked honestly. v2.7.0 IS the submission, so the list in `to-dos/mister/contribution-checklist.md` decides whether the core is ready -- and it had 30 boxes, 16 unticked, and FOURTEEN OF THOSE SIXTEEN saying nothing at all about why. That ambiguity is the defect: an unticked box with no reason cannot be told apart from work outstanding, work blocked outside this repository, and WORK ALREADY DONE AND NEVER TICKED, and the third case occurred five times -- the provenance CI job, the SPDX sweep, the firewall statement, the AccuracyCoin vector and the preservation-value case were all true and all unticked, so the list reported the project as further from submission than it is by a fifth of its own length. ONE BOX COULD NEVER HAVE BEEN TICKED HONESTLY: it asked that `docs/provenance.md` state "that no NES core was ever opened", and that same document's section Do not self-certify forbids exactly that class of finished claim, so satisfying the box required writing the one sentence the provenance rules exist to prevent -- a wrong requirement rather than a missing tick, and only reading every item found it. RE-MEASURING THE TICKED HALF found two more, which is v2.6.9's lesson in a different document: `RustyNES.sdc` still said "there is exactly one core clock", true of v2.6.3 and false since v2.6.13, which added a 4x SDRAM clock and a phase-shifted pin clock that the shipped timing report names alongside it; and the `.qsf` entry said all 109 pin assignments come from `sys/sys.tcl`, where 109 is what that script supplies before stopping short of the I/O board, so a core must also source one of two 36-assignment variants -- 145 in total, from two scripts. THE ALARMING READING OF THE FIRST WAS CHECKED BEFORE IT WAS WRITTEN DOWN: it looks as though the framework's `set_clock_groups -exclusive` might be cutting the console-to-SDRAM crossing and leaving ADR 0039's safety argument unfalsifiable, and it is not -- exclusive cuts paths BETWEEN groups, all three outputs match one glob, and v2.6.13's own -24.769 ns measurement of that crossing is only observable because those paths are analysed. THE NAMING DIVERGENCE IS MEASURED RATHER THAN PARAPHRASED, from `Main_MiSTer/file_io.cpp` instead of the wiki: `get_display_name` searches for the literal underscore-two-zero and TRUNCATES THE DISPLAY NAME THERE, taking the rest as a datecode, and `DirentComp` groups by that truncated name -- so a version-named bitstream makes every release a SEPARATE CORE ENTRY, named for the version rather than the core, ordered alphabetically, which puts v2.6.9 after v2.6.13. The fix is one line and is NOT taken here, because version-naming is a maintainer decision with a stated rationale and reversing it is not an audit's call. THE TASK BOARD HAD THE SAME DEFECT AND ONE ROW WORSE: four delivered items never ticked, a hardware row still naming a version seven releases past, and an SDRAM row whose precondition -- "after a board exists" -- v2.6.13 simply did not follow, having accepted the controller against a behavioural model written from the datasheet instead; that is rung 7's own recorded lesson repeating one row below where it is written, since the blocker applied to hardware ACCEPTANCE rather than to building the thing. A CLAIM v2.6.13 SHIPPED IS RETRACTED: answering a review finding, I wrote that MMC1 was the sixth approved family and not yet implemented, and the cartridge decodes mapper 1 at three sites with a registered gate green since rung 7 opened -- asserted from memory inside a reply correcting somebody else's reading of the same line. The gate that keeps all this true asserts a SHAPE rather than a judgement and is demonstrated by five mutations, one of which proves the continuation-line folding load-bearing by producing nine false violations without it. The bitstream is BYTE-IDENTICAL to v2.6.13's, which is the point: the only sibling change is a comment, and an identical artifact demonstrates it. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction. Built on **v2.6.13 "Slack"** (2026-09-03) — the cartridge outgrows the die, and three consumers want the same bus. An SDR SDRAM controller, a behavioural part model, a four-way arbiter and a console bridge, all written from the AS4C32M16SB-7 datasheet revision 1.4 -- no third-party controller read, ADR 0037 applying. The budget the previous step worked to was a single figure read off the fetch structure and never measured; `nes_top`'s `CHR_LAT` sweep asks the console directly, and there are THREE answers: a background or sprite fetch tolerates 28 cycles and uses 17, the PPUDATA data port tolerates 8, and the CPU sampling at mc7 has 24 -- four cycles of every budget going to the console-domain crossing, which is not optional, because publishing a runtime modulo combinationally into an 11.64 ns domain costs -24.769 ns of setup. The PPUDATA port could never fit, half its budget being the crossing, so it leaves the shared bus entirely: `ppu2c02` gains `BUFFER_HANDSHAKE` and fills its read buffer through a port of its own on the arbiter, affordable because the CPU does not read that buffer until its next PPUDATA access. THAT FIX SHIPPED A DEFECT ONLY A BANKED CARTRIDGE COULD SEE -- the request carried the RAW fourteen bits the PPU presents, which is right by coincidence on NROM because the mapper's translation is the identity there, and reads BANK ZERO on everything else; `ppu-misc-2007-stress` passed off-die throughout while the two CNROM gates read a zero byte where the oracle reads one, one and two. The fix REMOVES the address rather than correcting it: `cart.sv` already publishes the translation for the fetch path, so the request carries none and there is one source of truth for where CHR lives instead of two that agree only on NROM. A DELETION WAS THEN REFUTED BY ONE CYCLE: with the address fixed, a control issuing at a flat +7 passed both gates, which read as the anti-contention deferral buying nothing, so it was removed -- and the deployed code issued at +6, and both gates failed again. The control and the code differed by a single cycle, which is the measurement of how thin the CPU's off-die deadline is and the concrete argument for scheduling the bus rather than arbitrating it. Two INFERRED LATCHES that Verilator cannot see: `ppu2c02` assigned two signals only under `BUFFER_HANDSHAKE`, so in the shipped on-die build the only assignment either reached was the reset branch, and a variable holding its previous value on every live path is a latch -- Quartus said so twice while the lint gate stayed green, and the comment beside them asserted the defect as a virtue ("outside BUFFER_HANDSHAKE neither signal ever moves", which is true and is exactly the condition). And a defect in the HARNESS: `USE_SDRAM` reaches Verilator as `-G` rather than as a file, so make ran ONE binary under both configurations' names and a log labelled on-die was the off-die build, reproducing the off-die failures exactly -- closed with a stamp-file prerequisite, demonstrated by mutation. THE OPEN ROW: accesses no longer auto-precharge, a hit costs 6 cycles against 10 for a miss, tRAS's 120 us MAXIMUM is respected by an early close in idle, and two defects came out of the rewrite -- re-entering idle with the request still asserted issued every access TWICE, and subtracting one from CAS the way the other waits are computed broke every read to all zeroes, CAS being "data appears at cycle N" rather than a command-to-command gap. Two MiSTer tickets close: `sdram_sz` is consumed VALIDITY BIT FIRST, so `absent` is deliberately not `!present` and a power-on all zeroes cannot read as "no board" (gated exhaustively over all 65,536 values), and `status_menumask` is computed rather than tied off, greying Reset whenever the console is already held in reset. OFF THE DIE THE CONSOLE PASSES 142 OF 142, every gate the on-die build passes, with better timing margin and 384 fewer M10K blocks -- and it still SHIPS ON the die, because an off-die core cannot run at all without the SDRAM add-on while rung 7's five mapper families fit on the die at 468 of 553 blocks. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction. Built on **v2.6.12 "Groundwork"** (2026-09-02) — the bitstream was an NROM-only console. Rung 7 landed five mapper families and 142 co-simulation gates verify them, and the layer that turns that RTL into a bitstream was never told: `rtl/emu.sv` left `cart_mapper`, `cart_prg_16k_banks` and `cart_chr_8k_banks` unconnected, so Quartus tied all three to GND -- mapper 0 for EVERY cartridge, `prg_8k_count = 0` collapsing PRG to an 8 KiB window, and CHR forced to RAM. The declared 256 KiB PRG and 128 KiB CHR were implemented as 8 KiB each; connecting three wires takes block memory from 666,061 to 3,680,717 bits and timing still closes at all four corners. NOTHING COULD HAVE CAUGHT IT: simulation cannot, because `emu.sv` is not in the testbench file list and the harness drives those ports itself, so all 142 gates exercised a correctly-configured cartridge; and Quartus DID say so three times, in messages that cite an INSTANCE path rather than a file and are absent from the "0 errors, N warnings" tally, so the existing checker read 0 of 125. Two gates close that -- one fails on an unconnected pin of any module this repository declares, the other pins the warning SET rather than its count -- and both are demonstrated to fail by mutation. The `hps_io` tie-off audit that followed raised nine `T-MISTER-*` tickets, and annotating each with a blocker turned "none of these landed" into a measurement: not one is blocked on EFFORT, so the list is the rung-6 agenda rather than a backlog. `T-MISTER-SAVE` was attempted and refuted -- every save route terminates in `hps_io`, which no gate here instantiates. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction. Built on **v2.6.11 "Exposure"** (2026-09-02) — a picture is a gate the ladder did not have. All 141 co-simulation gates THEN IN THE SUITE were green (it ends this release at 142, the one it added) and TWO OF SIX commercial games rendered wrong -- a CHR-RAM write was taking the shared-pin composite address built for FETCHES instead of `v`, so the layout was right and the tiles were scrambled. The split is exactly CHR-ROM against CHR-RAM, which named the mechanism before any tracing, and a CONTROL says it is not v2.6.10's regression: the pre-M10K-fix RTL differs by the IDENTICAL 16,565 pixels, so the defect dates from the cartridge landing in v2.6.9. It was not UNREACHED -- the DUT asserts `chr_wr` 9,600 times in the Battletoads run -- it was UNCOMPARED: only THREE of the 141 gates compare a framebuffer, all three ship CHR-ROM, every other gate is CPU-side, and AccuracyCoin, the widest gate in the suite, is CHR-ROM too. The rung-7 gates' own comment says what they are for -- "these gates are about BANKING and nothing else" -- and it was accurate, and it was the whole coverage. Six commercial titles now render byte-identically to the oracle over all 61,440 pixels, published as a montage built by a script that REFUSES to publish a tile that differs from the oracle. The v2.6.10 bitstream carries the defect and a published version is immutable, so the corrected `.rbf` ships here. The same release finds EIGHT release leads describing v2.6.10 with v2.6.9's summary, and v2.6.9 gone from the lineage entirely, with `docs/STATUS.md` naming v2.6.10 under the codename "Abeyance" -- every existing check passing CORRECTLY, because they pin the version TOKEN and the token was right. Prose cannot be audited; an ORDERING can, so two gates are added and both are demonstrated to fail by mutation. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction. Rung 6 does NOT close -- no DE10-Nano and no SuperStation One are attached to this machine, confirmed by checking rather than assumed. Built on **v2.6.10 "Inference"** (2026-09-01) — the cartridge meets the synthesiser. Five cartridge boards verified across 141 co-simulation gates had **never been through Quartus**, and Analysis & Synthesis refused the design: `chr` was written from TWO `always_ff` blocks, which cannot infer as one M10K, so 128 KB of CHR stayed in flip-flops -- **1,048,576 registers against roughly 166,000**. Simulation cannot ask this question: Verilator accepts both forms without complaint. It is v2.6.6's finding one layer out -- that release established that an M10K read is REGISTERED, this one that a correctly registered memory still will not infer with two writers. The fitter was also throttling itself under Auto Fit while the `.qsf` carried no optimisation assignments at all: at full effort **all six seeds close** where two had failed, so the effort settings move the whole distribution across zero and the seed only picks where in it you land -- and the project had been pinned to seed 4, the WORST of the six. Pinned at seed 3, +0.531 ns setup and +0.099 ns hold, byte-identical across two independent compiles. The bitstream v2.6.9 could not produce ships here. Built on **v2.6.9 "Abeyance"** (2026-08-31) — an exclusion hides improvement as well as regression, and both denied co-simulation streams close. The larger one was never the console: `apuconflict039` had been carried for seven releases as a declared diagnostic whose bus surface "carries nine divergences BY DESIGN", and the nine were a defect in the HARNESS -- on a cycle the CPU is held, the testbench built its record's bus data from a stale local rather than from the RTL's own latch. Taking it from the latch makes the stream IDENTICAL on all 357,361 overlapping cycles and all 88 checkpoints, and the local is now dead and deleted. The phrase "by design" is what stopped anyone re-checking it, because it reads as a property of the thing under test when it was a property of the instrument reading it. The other stream differs on EXACTLY ONE cycle, a documented and attributed OAM-corruption asymmetry -- and carrying that needed an instrument the suite did not have, because the PLANNED mechanism was refuted by its own mutation pass: an allowance by checkpoint index cannot work on a rolling hash, since one divergent cycle poisons every checkpoint after it, so allowing the first differing window simply moved the failure to the next one and allowing the rest is the all-or-nothing deny it was meant to replace. A per-cycle nine-field comparator with a scoped allowance costs ONE cycle of coverage instead of seventy-one checkpoints -- 357,360 of 357,361, against nothing at all before -- and it fails BOTH ways, so a DUT that improves cannot leave a stale allowance quietly hiding coverage; six mutations confirm it, including a cycle outside the compared window being REFUSED rather than allowed to match nothing. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction and were re-run anyway. Rung 6 does NOT close -- no DE10-Nano and no SuperStation One are attached to this machine, confirmed by checking rather than assumed. Built on **v2.6.8 "Arrears"** — a deny list is an assertion about the thing under test and nobody re-measured it -- four of six denied co-simulation streams were already passing, three of them never run by the suite at all, and the nestest gate widened 19x to all 5,062,680 cycles, closing caveat C4 by demonstration. Built on **v2.6.7 "Detent"** — the bitstream becomes a published release artifact and a one-cycle disagreement is pinned to the cycle it happens on. Every release from here ships a `.rbf` -- committed to the sibling's `releases/` and attached to the GitHub release on BOTH repositories -- reversing v2.6.6, which produced one and withheld it because no hardware had run it: the MiSTer distribution mechanism reads that path out of the REPOSITORY, so an empty `releases/` describes an undistributable core rather than a cautious one, and the caution moves from an absence into a disclosure naming what the ladder cannot reach by construction (the PPU gate compares the pre-palette index and the APU gate per-channel integer levels, so the palette, the video timing constants, the audio absolute level and its band-limiting all sit downstream of every gate). The build is REPRODUCIBLE and that is now measured rather than argued -- a from-scratch compile and an incremental one produce a byte-identical bitstream -- which is also how v2.6.6's published slack figures came to be WITHDRAWN: no corner of a clean rebuild reproduces them, the innocent explanation (a different timing corner) was checked first and refuted, and the correct pair is +0.108 ns setup and +0.042 ns hold at the binding corner. THE RELEASE GATE WAS READING THE WRONG CORNER -- Slow 100C is not the binding one on this design, so a bitstream failing at Slow -40C would have passed while the gate reported three times the real margin -- and the checker that reads it was wrong twice before mutation found both: it first extracted ZERO rows from both summary tables and reported that as "no negative slack", then, once fixed, reported FOURTEEN clocks from a report emptied of its data, having run past the closing rule into the next tables. Caveat C2 splits in two. The first residual was a TRACE OBSERVATION POINT -- the harness built its record after eight of a CPU cycle's twelve master clocks while the oracle reads at end-of-cycle, and the frame-counter interrupt asserts on the final edge -- and closing it took checkpoint comparisons from 3 to 11 and failures at one checkpoint from 30 to 3. The second is REAL, and the FIRST fix for it was REFUTED in a way that found the right one: moving all four effects of the write one cycle later to match the oracle drops blargg from 11/11 to 4 of 11, one of the seven being the ROM written to probe exactly that timing. Read as a measurement that PROVES the sequencer's maturation is correctly placed, which leaves only the other effects the same write schedules -- so separating ONLY the interrupt clear lands it at write+3, the documented cycle, while the frame counter's zeroing stays put. Checkpoint streams go from 11 identical to 52 of 58 and the suite from 87 to 122, with blargg still 11/11 and the bus still matching on all 2,680,239 overlapping cycles. The checkpoint gate is registered over 51 comparisons and STATES ITS BLIND SPOT: not one gated golden ever raises an NMI, so it cannot catch an nmi_line defect, and the attempt to close that hole found a FOURTH divergence cluster that three goldens had been hiding inside a "26 skipped" tally line. The emulation core is unchanged, so AccuracyCoin 141/141 and nestest 0-diff hold by construction. Rung 6 does NOT close -- no DE10-Nano and no SuperStation One are attached to this machine, confirmed by checking rather than assumed. Built on **v2.6.6 "Chassis"** (2026-08-29) — the console becomes a MiSTer core -- `sys/` vendored byte-identical against `Template_MiSTer@3ea1134c` (57 files, 0 content differences), a top level, a clock, a palette, video sync and an audio mixer, compiled by Quartus 17.0.2 into a Cyclone V bitstream with 0 errors, timing CLOSED, and a warning count taken from 111 to THREE -- all three inside the vendored framework or Quartus's own megafunction, none of them citing this project's RTL (worst setup +0.086 ns and worst hold +0.096 ns at the binding corners, seed 3 -- v2.6.7 also withdraws the +0.363/+0.245 v2.6.6 published, which a clean rebuild of that configuration reproduces at no corner, TNS 0.000 on every clock; the console own clock +13.514 ns at an Fmax of 30.26 MHz against the 21.477272 MHz it needs). The emulation core is unchanged, so the co-simulation suite is an ACCEPTANCE CRITERION rather than a formality -- 87 passed, 0 failed -- and it earned that immediately, because the cartridge memories had to be rewritten: an M10K read is REGISTERED, so 40 KiB of asynchronously-read cartridge was 393,216 registers against roughly 166,000 available, under a comment claiming it inferred block RAM from the source style alone, and the README had stated the correct rule since v2.4.3. Two defects were found only by asking whether the outputs would work on real hardware: the audio would have been a full-scale DC rail, because the mixer output is unipolar with silence at zero and the framework maps unsigned silence to -32768, and two OSD scanline options did nothing because VGA_SL was tied to zero. And a convention enforced by a glob has no error message: sys_top.sdc groups the core clock by matching the hierarchical name pattern *|pll|pll_inst|altera_pll_i|*, so a differently-named PLL matched no group at all and every crossing to the framework audio, HDMI and HPS domains was analysed as synchronous -- -13.901 ns of slack and -422,601 ns of TNS on a design whose Fmax was already above requirement, with the compile succeeding and the Assembler reporting 0 errors and 0 warnings throughout. Built on **v2.6.5 "Muster"** (2026-08-29) — rung 5 closes — the AccuracyCoin status vector is identical entry for entry across all 146 entries, with 146 of 146 executed on both sides and none NotRun, where the same gate read 5 of 146 at the version's start. A muster is a roll call where every name is called AND answered, which is the two-clause acceptance exactly. Five PPU defects close the last six differing entries and four were invisible to every gate that existed when the version opened: the background shift registers' RELOAD and their shift clock need SEPARATE gates (with one shared gate the serial-in test was not merely failing but ARITHMETICALLY UNREACHABLE, since reload dots are absolute and the reload discards the low seven bits, so a serial-in one can never reach bit 7 on any alignment — and modelling both structures reproduces BOTH measured shifter values); the sprite X counters are NOT gated on rendering, which AccuracyCoin states outright and the ROM that states it passes either way, because it expects no hit at X=254 and a sprite shoved 18 dots right is also off the line; the PPUADDR second-write v-copy is DELAYED, as the wiki says inside the write sequence itself, swept 1 to 4 dots against a control at 8 and 12 that fails; and the pre-render line CLEARS secondary OAM, without which scanline 0 draws what scanline 239 left — no sprite can ever render on scanline 0, because OAM Y is one less than the display row, and a sprite-0 probe over the full 134 M-cycle battery found 24 hits with four of them there; and the octal latch holding across the read dot, which is verified by exactly ONE gate and was unverifiable until the v-copy delay landed, the two composing the hybrid address together and neither producing it alone. A DIAGNOSIS IS RETRACTED: the residual was read as a two-dot CPU/PPU alignment error from comparing dot spans across two instruments, and at the committed alignment the two consoles execute identical pc, bus_addr and bus_access for 1,695,131 cycles while a two-dot shift moves the first fork back to 593,228 and takes the differing share from 5.13% to 66.80%. The oracle changes on the default path, so AccuracyCoin 141/141 (RAM decoder) and nestest 0-diff are VERIFIED, not asserted. Built on **v2.6.4 "Rubric"** (2026-08-26) — OAM DMA lands and all nine AccuracyCoin disagreements close, every rule that closed the last three stated by the test ROM and by neither nesdev page — and then the gate that certified them is measured to cover 88 of 146 entries. The emulation core is unchanged. Built on **v2.6.3 "Mainspring"** (2026-08-25) — the DUT runs on one master clock, and four enables that were never enabling — plus AccuracyCoin end to end and a status vector that names its disagreements by test. The emulation core is unchanged. Built on **v2.6.2 "Witness"** (2026-08-24) — rung 4 closes: blargg APU battery 11/11 on the co-simulation DUT, six defects no self-written gate could see, and a suite that had been asserting nothing for five minor releases. The emulation core is unchanged. Built on **v2.6.1 "Interleave"** (2026-08-24) — the DMC and its DMA cycle steal in the MiSTer co-simulation DUT, cycle-exact on the bus. The emulation core is unchanged. Built on **v2.6.0 "Assay"** (2026-08-24) — the triangle, the noise channel and the sweep unit **in the MiSTer co-simulation DUT** — and an audit of how much of the APU was fitted to the oracle rather than derived from documentation. The emulation core is unchanged. Built on **v2.5.9 "Overture"** (2026-08-24) — rung 4 opens: the two pulse channels, the frame counter, and four ROM defects the stimulus measurement found first. Built on **v2.5.8 "Blanking"** (2026-08-24) — VBlank, NMI and the PPUSTATUS race close rung 3 — and both fixes were deletions. Built on **v2.5.7 "Collimation"** (2026-08-24) — sprite rendering closes exact — the phase was wrong by two dots, and every window was compensating. Built on **v2.5.6 "Vestige"** (2026-08-23) — Sprite evaluation closes: all 59,993 overlapping cycles match, nine of nine behavioural mutants caught and two proved inert (announced as seven of eight at the cut), and the fix is a byte index that outlives the walk that set it. Built on **v2.5.5 "Raster"** (2026-08-23) — the first full frame, and three blind spots in the stimulus that fed it. Built on **v2.5.4 "Escapement"** (2026-08-23) — the background fetch pipeline, and an access two dots early that five gates could not see. Built on **v2.5.3 "Hysteresis"** (2026-08-23) — toggling rendering takes effect three dots after the write, and four instruments to prove it. Built on **v2.5.2 "Dormant"** (2026-08-23) — the 2C02 register file, and a gate that passed while testing nothing. Built on **v2.5.1 "Retrace"** (2026-08-23) — the interrupt sweep closes rung 2, and a gate reported a pass it could not have earned. Built on **v2.5.0 "Rungwork"** (2026-08-23) — the 6502 rung, and the two gates it cannot reach. Built on **v2.4.9 "Plumbline II"** (2026-08-23) — the bus half of rung 2, and what it found the day it existed. Built on **v2.4.8 "Palimpsest"** (2026-08-23) — read-modify-write, and a gate that cannot see its own subject. Built on **v2.4.7 "Keystone"** (2026-08-23) — the stack closes, and a dead line proves itself dead. Built on **v2.4.6 "Abacus"** (2026-08-22) — the core learns arithmetic. Built on **v2.4.5 "Compass"** (2026-08-22) — the core reaches memory, and chooses. Built on **v2.4.4 "Ignition"** (2026-08-22) — the first real RTL. The 6502's eight-cycle reset and the seventeen single-byte implied opcodes, in SystemVerilog in the sibling repository (`RustyNES_MiSTer@7f092bd`), matching the oracle on all seven CPU fields -- 29 records, and the gate demonstrated to fail on four mutations. The DUT is the **third writer** of the oracle's `CpuBootTrace` format, so `cpu_boot_trace_diff` reads it with no modification and the rung needed no oracle-side change at all. **The oracle settled a question our own prose could not**: reset is EIGHT cycles, and `docs/cpu-6502.md` said both seven and eight -- corrected here. The emulation core is untouched. Built on **v2.4.3 "Touchstone"** (2026-08-22) — what the synthesiser accepts, and what the licence requires. A touchstone is a stone you rub gold against; the streak tells you what the metal actually is. This release settles the **two Fabric-plan risks that had to be answered before any RTL exists**, and both were answered by evidence that contradicted what the plan assumed. **Risk 4, the Quartus subset, is FITTED**: Quartus Prime Lite 17.0.2 Build 602 on a 5CSEBA6U23I7 produced a placed-and-routed netlist with **0 synthesis warnings**, and the 2 KiB array inferred as **2 M10K blocks with 29 total registers** — not 16,413 — from the source style alone, no `ramstyle` attribute. The `initial` block became a real MIF (so a boot ROM lands inside the block) and the `enum` was one-hot encoded. Nine constructs are promoted to *fitted*; plain `case`, `priority case` and `$bits` are deliberately left *documented* because the kitchen sink does not exercise them. **Risk 1, the `sys/` licence, inverts the plan's own hedge**: 57 files, **zero GPL-2.0-only**, and `hps_io.sv` — GPL-3.0-or-later and not optional, since it is how a core receives a ROM and reaches the OSD — forces the combined bitstream **up** to GPL-3.0-or-later, already RustyNES's licence. The emulation core is untouched. Built on **v2.4.2 "Cairn"** (2026-08-22) — the **rung-0 compare surface**: rolling per-cycle hash checkpoints, measured at **15,263x** smaller than the equivalent CSV; the v2.4.2 acceptance gate made executable; and the partition between what RustyNES *models* and what a device can *observe*. Built on **v2.4.1 "Fabric"** (2026-08-20) — the **oracle** release, opening the **v2.4.1 → v2.5.0 "Fabric"** line: a new NES core written in SystemVerilog from public hardware documentation, in a sibling repository, with this emulator as its **verification oracle**. RustyNES is not being ported to FPGA and cannot be; `crates/rustynes-cosim` is the boundary (a narrow C ABI a Verilator testbench links, plus `nes_golden_export`), and the provenance firewall extends to HDL per ADR 0037. **v2.5.0 is scoped to "the 6502 rung closes"**, not a finished core. Excluding the crate from the workspace is the load-bearing detail — cargo unifies features, `irq-timing-trace` selects a *different* per-dot loop in `Bus::tick_one_cpu_cycle`, and the accuracy battery had been validating a scheduler no user runs. It also carries **v2.4.0 "Concordance"**, which merged to `main` and was never tagged: atomic durable writes on every path that persists user data, `Nes::timeline_generation()`, and the 15-anchor release audit. AccuracyCoin **141/141** verified, not asserted. Built on **v2.3.9 "Crucible"** (2026-08-20) — the **gates** release. A crucible tests to destruction rather than inspects, and that is what this release does to the project's own checks: what they cover, what they only *appear* to cover, and where a regression could still reach `main` unchallenged. The v2.3.x line added five tools in four releases, and the recurring finding across all of them was never that the emulation was wrong — it was that **a check reported a pass it had not earned**. **The docs-only CI skip had never worked**: `dorny/paths-filter`’s `predicate-quantifier` defaults to `some`, so the `code` filter’s leading `'**'` matched everything and all seven `!` exclusions under it were dead from the day they were written — a markdown-only PR logged `Filter code = true`. Fixed with **two** filter steps, because the quantifier is step-level and `accuracy` is a list of *alternatives* that becomes unsatisfiable under `every`: the naive one-line fix would have silently disabled the accuracy battery while repairing a different gate. **`test-roms` now runs at review time**, path-filtered over the chip crates, the core, `rustynes-gamedb`, the harness and `tests/` — measured first at 11 of the last 40 merged PRs, so ~72% still pay nothing. **A freeze from one cartridge kept writing into the next** — not a stale label but an active per-frame write into the wrong game, closed by a ROM-transition sweep across every panel under one rule: derived output is discarded, user-authored input is kept, and only input that actively *writes* is neutralised. **The config file is now written atomically and durably** (seven properties, five of them from review rather than the first draft). Plus **257 lines of dead code removed**, the SAFETY-comment rule made a clippy gate (`undocumented_unsafe_blocks`, demonstrated to fail), and two `cargo deny` advisory ignores retired on their own stated condition. `rustynes-apu` and `rustynes-core` both change, so **AccuracyCoin 141/141 (100.00%, RAM decoder) and nestest 0-diff are VERIFIED, not asserted.** **`docs/STATUS.md` is the authoritative current-state record.**
- **Shipped, inside v2.4.1 — v2.4.0 "Concordance".** It merged to `main` and was never tagged, because the workspace version never sat at 2.4.0 on any commit; v2.4.1 carries it. There is deliberately no `v2.4.0` tag. Its scope was: A concordance is an index of where every term actually occurs, and the release is scoped as one: reconcile what the project says about itself with what is true outside it. Four items, each traceable to a recorded deferral rather than newly invented — **(A)** the **owed upstream libretro sync** (`libretro-super` + `libretro/docs`), the one carried obligation with an outside deadline; **(B)** a core-side **timeline generation counter** replacing the last-seen-`cycle()` heuristic for stale telemetry (it covers a restore to a *later* state, which the heuristic cannot), deliberately **not** serialized, so it must land with its consumers and be AccuracyCoin-**verified**; **(C)** a **shared atomic-write helper**, lifting v2.3.9's seven properties out of `config.rs` and giving the Windows tail a real implementation rather than a portable spine; and **(D)** `skip_serializing_if` on `hd_packs` / `shader_presets`, which carry the same false byte-identity claim v2.3.9 corrected in prose only. Explicitly out of scope, and recorded as decisions rather than oversights: the remaining RAM Atlas exports (a cheat is a **write**, so it needs a locked-session predicate the watch export correctly does without), RAM Atlas per-game persistence (a restored verdict without its evidence is a claim that cannot be checked — this panel's whole argument in reverse), APU workstreams **D2 and D4** (unmeasured on purpose; their prior is a null, not an unknown), a CHANGELOG gate (**measured and rejected** — 62% false positives against the project's own history), and any store launch. See [`plans/v2.4.0-concordance-plan.md`](plans/v2.4.0-concordance-plan.md).
- **Programme after v2.4.0 — the v2.4.1 → v2.5.0 "Fabric" line, and the v2.6–v2.9 programme behind it.** An **independently-written NES core in SystemVerilog for MiSTer FPGA and the Retro Remake SuperStation One, verified against RustyNES as an oracle.** Not a port, and it cannot be one: a MiSTer core is SystemVerilog compiled by Quartus 17.0.2 into a Cyclone V bitstream. The reference firewall therefore extends to HDL — `NES_MiSTer` and `fpganes` `rtl/` are **strict black boxes**, instantiable as opaque modules to compare *outputs*, never readable as source. **v2.5.0 is scoped to "the 6502 rung closes"** — the co-simulation harness plus a cycle-exact 6502, gated, **as planned**, on nestest 0-diff and per-cycle bus equality — of which **per-cycle bus equality was achieved and nestest 0-diff was not**: it stops at a `$2002` read where *both sides address it* and only the data differs, because the DUT has no PPU. That and the 5 M-cycle window are **reclassified as rung-3 acceptance criteria** rather than carried as v2.5.0 debt — because the arithmetic does not support more: a from-scratch cycle-accurate NES core is **7–13 months FTE** against a two-to-four-week window at demonstrated cadence. PPU, APU and MiSTer integration are **v2.6–v2.9**; stating that now is better than discovering it at v2.4.6. The design is **replay, not lockstep** (the determinism contract makes a pre-recorded trace exactly the trace a lockstep run would produce, and `Nes` has no per-cycle step to lockstep *with*), **no DPI-C** (it would put `` `ifdef SIMULATION `` guards into RTL that must also pass Quartus — the exact construct that lets a simulated netlist drift from the synthesised one), and **hash first, capture on divergence** (a 4200-frame AccuracyCoin run is ~7.5 GB of per-cycle CSV; 4096-cycle hash checkpoints are ~480 KB). **Two risks are accepted in writing:** the core may be **declined as a duplicate** — `NES_MiSTer` already scores 121/125 on AccuracyCoin, and *real Famicom AV hardware also scores ~121/125*, so there is no published accuracy headroom; and **the oracle can be wrong**, since 141/141 is not "matches silicon", so every rung is labelled by whether it has an **independent** oracle. Retro Remake is a planned fallback home, not a contingency. See ADR 0037, `docs/mister.md`, and [`plans/v2.5.0-fabric-plan.md`](plans/v2.5.0-fabric-plan.md).
- **Programme after v2.5.0 — the v2.5.1 → v2.7.0 line: the rest of the console, and a contributable package.** The Fabric line is delivered and the 6502 rung is closed; this line builds the PPU, APU, mappers and MiSTer integration, and takes the core to a state worth submitting to MiSTer-devel. **Maintainer decisions, 2026-08-23:** hardware is **both boards eventually** — a DE10-Nano **plus the SDRAM add-on** (mandatory: the NES reads cartridge ROM directly and the onboard DDR3 is too slow) and a SuperStation One (128 MB integrated), with **one `.rbf` booting both** turning "SS1 runs MiSTer cores unmodified" from an inherited claim into a measured one; mappers are **the top six** — NROM, MMC1, UxROM, CNROM, MMC3, AxROM, ~90% of the licensed library by title count, explicitly **not** FDS, expansion audio, or the remaining ~168 families; and v2.7.0 is **scoped to what genuinely fits**, with the arithmetic stated up front (**rung 3 8–16 wk · rung 4 4–8 wk · rung 5 2–4 wk + a 4–12 wk tail · rung 6 2–4 wk · rung 7 4–8 wk = 20–40 weeks FTE** before the AccuracyCoin tail, across twenty release slots — **milestones, not dates**). **Rung 6 comes before rung 7 deliberately**: NROM at 327 Kb fits on-chip, so hardware bring-up needs no memory controller, and getting a board in the loop before writing the SDRAM controller de-risks the second largest technical item. Two v2.5.0 gates — **nestest 0-diff and the 5 M-cycle window** — are not carried as debt but reclassified as **rung-3 acceptance criteria**: both stop at a `$2002` read where *both sides address it* and only the data differs, because the DUT has no PPU. The contribution requirements were **fetched from the MiSTer-devel wiki rather than recalled**, and one line of it is the whole case for this programme: on AI-generated code the project asks for *"a minimum reasonable bar for readability and… evidence of quality and accuracy testing"* — the co-simulation apparatus **is** that evidence, and no incumbent core can show its equivalent. See [`plans/v2.7.0-mister-core-plan.md`](plans/v2.7.0-mister-core-plan.md), [`mister/`](mister/), and the four dated research files in `ref-docs/`.
- **Historical detail — v2.2.4** (2026-07-24) — a **libretro / RetroArch distribution** cut whose purpose is that the RustyNES core **builds and installs cleanly through the Libretro buildbot** (<https://git.libretro.com/libretro/RustyNES>) for in-RetroArch use. **Zero emulation-core changes** — the deterministic `#![no_std]` chip stack, save-state / TAS / netplay formats, and every golden vector are byte-identical to v2.2.3, so **AccuracyCoin holds 141/141 (100.00%)**, nestest 0-diff, by construction. The work is a libretro-completeness audit + metadata correction: the core is confirmed to inherit every v2.2.3 change automatically (the fast-dot-path default, the `PPU_SNAPSHOT_VERSION` 8 / APU v4 save-state schema handled transparently by the dynamic `snapshot_core_into` sizing, the `Mapper::mix_audio` i32 widening, the Zapper model, and the `mNNN_` mapper rename), and both buildbot cross-ABIs the GitHub gate models — `x86_64-pc-windows-gnu` and `aarch64-linux-android` — build clean. `rustynes_libretro.info` (the metadata RetroArch's core downloader reads) is corrected: **`disk_control` `false` → `true`** (the FDS multi-side Disk Control interface has been wired since the buildbot recipe landed, but was advertised as absent — the real fix), `display_version` `v1.0.0` → `v2.2.4`, and the mapper count `168` → `172`. Also: the reviewer-tooling standardization onto the shared Antigravity template rides along (`scripts/agy-review.sh` + workflow). Documented libretro follow-up: **core options** (region / overscan / palette / accuracy toggles) remain unexposed (`core_options = "false"` is accurate, not stale) — a deliberate future enhancement, not a v2.2.4 gap. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.2.4]` + `docs/libretro/`.
- **Release line since v2.1.0:** the v2.1.x **"Fathom"** accuracy line (v2.1.0 → v2.1.10) → **v2.2.0 "Capstone"** (the milestone cut closing the "deepen the existing project" run) → **v2.2.1** (housekeeping) → **v2.2.2 "Conduit"** (build / distribution / CI-integrity) → **v2.2.3 "Datum"** (performance appraisal + the last two Holy Mapperel residuals closed) → **v2.2.4 "Cartridge"** (the libretro/RetroArch distribution cut) → **v2.2.5 "Colophon"** → **v2.2.6 "Almanac"** → **v2.2.7 "Timbre II"** → **v2.2.8 "Aperture II"** → **v2.2.9 "Studio II"** → **v2.3.0 "Datum II"** → **v2.3.1 "Plumb Line"** → **v2.3.2 "Lucid"** → **v2.3.3 "Cadence"** → **v2.3.4 "Ledger"** → **v2.3.5 "Manifest"** → **v2.3.6 "Sounding"** → **v2.3.7 "Overtone"** → **v2.3.8 "Parallax"** → **v2.3.9 "Crucible"** → the **v2.4.x "Fabric"** co-simulation line (**v2.4.1 "Fabric"** → **v2.4.2 "Cairn"** → **v2.4.3 "Touchstone"** → **v2.4.4 "Ignition"** → **v2.4.5 "Compass"** → **v2.4.6 "Abacus"** → **v2.4.7 "Keystone"** → **v2.4.8 "Palimpsest"** → **v2.4.9 "Plumbline II"** → **v2.5.0 "Rungwork"** → **v2.5.1 "Retrace"** → **v2.5.2 "Dormant"** → **v2.5.3 "Hysteresis"** → **v2.5.4 "Escapement"** → **v2.5.5 "Raster"** → **v2.5.6 "Vestige"** → **v2.5.7 "Collimation"** → **v2.5.8 "Blanking"** → **v2.5.9 "Overture"** → **v2.6.0 "Assay"** → **v2.6.1 "Interleave"** → **v2.6.2 "Witness"** → **v2.6.3 "Mainspring"** → **v2.6.4 "Rubric"** → **v2.6.5 "Muster"** → **v2.6.6 "Chassis"** → **v2.6.7 "Detent"** (the bitstream becomes a published, reproducible release artifact and a one-cycle disagreement is attributed rather than fitted away) → **v2.6.8 "Arrears"** (the gates the previous release fixed and never widened: four of six denied checkpoint streams had been passing unnoticed, three of them not run by the suite at all, and nestest widens 19x and gains the nine-field comparison that closes the gate's stated `nmi_line` blind spot) → **v2.6.9 "Abeyance"** (an exclusion hides improvement as well as regression -- both denied co-simulation streams close, and the larger one was a defect in the HARNESS rather than the console, carried for seven releases behind the phrase "by design") → **v2.6.10 "Inference"** (the cartridge meets the synthesiser, and simulation could not have asked the question -- rung 7's five boards had never been through Quartus, and `chr` written from two separate `always_ff` blocks could not infer as an M10K, so 128 KB stayed in flip-flops: 1,048,576 registers against roughly 166,000. The fix is behaviourally invisible -- 141 gates unchanged -- and the release carries the bitstream the previous one could not produce) → **v2.6.11 "Exposure"** (a picture is a gate the ladder did not have -- all 141 co-simulation gates green and two of six commercial games rendering wrong, because only THREE of those gates compare a framebuffer and all three ship CHR-ROM, so a CHR-RAM write taking the shared-pin composite address built for fetches was reached 9,600 times and compared never)) -> **v2.6.12 "Groundwork"** (the bitstream was an NROM-only console -- `emu.sv` left `cart_mapper`, `cart_prg_16k_banks` and `cart_chr_8k_banks` unconnected, so Quartus tied all three to GND and the fitted cartridge was mapper 0 with 8 KiB of PRG and 8 KiB of CHR against a declared 256 and 128; no gate could see it because `emu.sv` is not in the testbench file list, so all 142 gates exercised a correctly-configured cartridge, and Quartus said so three times in messages that cite an instance path rather than a file) -> **v2.6.13 "Slack"** (the cartridge outgrows the die -- an SDR SDRAM controller, a behavioural part model, a four-way arbiter and a console bridge written from the AS4C32M16SB-7 datasheet, and three consumers measured to three different budgets rather than the one figure the previous step assumed; the PPUDATA port leaves the shared bus through a handshake and that fix shipped a defect only a banked cartridge could see, the request carrying the raw PPU address where the mapper's translation was needed, so every banked-CHR board read bank zero; off the die the console passes 142 of 142 and it still ships on the die, because an off-die core cannot run without the add-on) -> **v2.6.14 "Docket"** (the submission checklist becomes auditable -- 30 boxes, 16 unticked and fourteen of those saying nothing about why, so an unticked box could not be told apart from work outstanding, work blocked elsewhere, and work already done and never ticked; the third case occurred five times. One box asked `docs/provenance.md` to state that no NES core was ever opened, which that document's own Do-not-self-certify section forbids, so it could only have been ticked by writing the sentence the provenance rules exist to prevent. Re-measuring the ticked half found two claims that had expired, the task board four delivered items never ticked and an SDRAM precondition the previous release did not follow, and its claim that MMC1 was unimplemented is retracted) → **v2.6.15 "Warrant"** (the claims the submission will make become checkable, and the instrument pays the oracle back — the `.rbf` name this core shipped would have distributed NOTHING, because `Distribution_MiSTer`'s builder skips any file whose stem does not end in `_` plus eight digits, so an accepted core would appear in the Cores table and ship nothing with no error anywhere; two of the four R1/R2 residuals ADR 0002 closed turn out never to have been IRQ-timing residuals, resting on an assertion blargg WITHDREW in the successor ROM; `sys/` verbatim, the `.qsf`'s single seed table and the bitstream name all become checks instead of claims; the nine rung-1 gates run in CI against a PINNED oracle commit, so the accuracy evidence stops being a document describing a check a reader cannot run; `cpu_interrupts_v2` lands on the DUT as the first INDEPENDENT interrupt oracle, five of five; and `T-ORACLE-001`'s opening claim is retracted — RustyNES does clock the MMC3 counter on the pre-render line, which `2-details` sub-test 8 has asserted all along, the current release). AccuracyCoin holds **141/141** throughout — but not always *by construction*: v2.3.4, v2.3.7, v2.3.9 and the rung-3 releases v2.5.4-v2.5.6 change the core, so for those the number is **verified** rather than inherited, and saying which is which is the point. **Full per-release detail is in `CHANGELOG.md` and `docs/STATUS.md` (the single source of truth)** — the entries below (v2.1.0 "Fathom" was the prior anchor here; v2.0.8 → v2.0.1) are the older historical trail, retained rather than duplicated.
- **Preceding release:** **RustyNES v2.0.8 "Harbor"** (2026-07-09) — the eighth release of the **v2.0.x mobile-finalization train** and the **iOS release candidate** ("Harborlight"), the final release of the iOS finalization window (**v2.0.5 → v2.0.8**). A **host / iOS-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.7** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched). It stages the App Store scaffolding for v2.1.0: version-controlled **App Store Connect listing metadata** (`fastlane/metadata/ios/{en-US,es-ES}/`, mirroring the Android tree, files-only), a **dormant App Store `release` lane** in `fastlane/Fastfile` that stages the build + listing but **does not submit** (`submit_for_review: false`) and is **not** CI-wired (the interim channel stays **TestFlight**), and an **App-Review §4.7 self-audit** (no bundled/downloadable ROMs, ownership notice, searchable library, 4+ rating) in `docs/ios-v2.0.8-readiness.md`. Version bump (workspace `2.0.7 → 2.0.8`; iOS `MARKETING_VERSION → 2.0.8`). **No store submission** (that is v2.1.0); screenshots, real signing, the listing upload, and the App-Review submission are the **maintainer / v2.0.9 / v2.1.0** closeout. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.8]` + `docs/ios-v2.0.8-readiness.md` + `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.7 "Harbor"** (2026-07-09) — the seventh release of the **v2.0.x mobile-finalization train** and the **third iOS finalization release** ("Trim"), continuing the iOS window (**v2.0.5 → v2.0.8**). A **host / iOS-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.6** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched). It wires the **App Store submission floor** (Apple mandates the **iOS 26 SDK / Xcode 26** for every App Store Connect upload from **2026-04-28**, so the tag-gated iOS CI now selects the newest Xcode 26.x on the runner — a build-SDK pin, non-breaking fallback on older images), **reconciles the deployment target `iOS 15.0 → 17.0`** to match the code's real API floor (`NavigationStack` iOS 16 + `.topBarTrailing` iOS 17, unguarded at 12+ sites — the prior 15.0 was never buildable), and **re-audits `PrivacyInfo.xcprivacy`** against the v2.0.6 crash reporter (no new data type / required-reason API — local-only, backup-excluded, off by default). Version bump (workspace `2.0.6 → 2.0.7`; iOS `MARKETING_VERSION → 2.0.7`). **TestFlight-only** (App Store + AltStore PAL deferred to v2.1.0); on-device profiling + the Xcode-26 archive are a **maintainer / v2.0.9** step. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.7]` + `docs/ios-v2.0.7-readiness.md` + `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.6 "Harbor"** (2026-07-09) — the sixth release of the **v2.0.x mobile-finalization train** and the **second iOS finalization release** ("Parity"), continuing the iOS window (**v2.0.5 → v2.0.8**). A **host / iOS-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.5** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched), so no accuracy / save-state / determinism number moves. It adds a **new opt-in, privacy-first crash-reporting surface** (off by default — the iOS analogue of the Android v1.8.8 `CrashReporter`, closing the v1.9.9 iOS-applicable deferral): **Settings → Diagnostics** installs an uncaught-`NSException` handler that writes **local** crash logs the user can view + copy in-app — **nothing is uploaded**, so the "Data Not Collected" privacy label is unchanged (EN + ES); the handler re-checks the live opt-in at crash time so opting out stops new logs immediately. It also records the **feature-parity re-verification** of the v1.9.x host features (Game Center, CloudKit save sync, MFi controllers, capture / PiP, accessibility) against the unchanged v2.0.0 bridge surface. Version bump (workspace `2.0.5 → 2.0.6`; iOS `MARKETING_VERSION → 2.0.6`). **TestFlight-only** (App Store + AltStore PAL deferred to v2.1.0); on-device crash-capture verification is a **maintainer / v2.0.9** step. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.6]` + `docs/ios-v2.0.6-readiness.md` + `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.5 "Harbor"** (2026-07-09) — the fifth release of the **v2.0.x mobile-finalization train** and the **first iOS finalization release** ("Landfall"), opening the iOS window (**v2.0.5 → v2.0.8**) that mirrors the Android v2.0.1 → v2.0.4 window. A **host / iOS-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.4** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched), so no accuracy / save-state / determinism number moves. It re-ports the frozen v1.9.9 SwiftUI / Metal app onto the v2.0.0 "Timebase" core: **(1)** the **pre-Timebase movie warning surfaced + localized on iOS** — a non-blocking notice on its own channel (multiplexed through a single alert that prefers an error when both are queued, **EN + ES**, drained via `EmulatorCore.drainWarnings()` → `NesController.drainWarningCodes()`, wording byte-identical to the Android v2.0.4 string) so loading a pre-v2.0.0 `.rnm` tells the user byte-exact framebuffer/audio reproduction isn't guaranteed across the ADR-0028 timebase change; **(2)** the **UniFFI-Swift binding surface re-confirmed** against the v2.0.0 bridge (`drainWarningCodes` / `HostWarning.preTimebaseMovie` / `moviePlay`, host-verified Swift emit); and the **version bump** (workspace `2.0.4 → 2.0.5`; iOS `MARKETING_VERSION 1.9.1 → 2.0.5`, realigned from the frozen v1.9.x default). **TestFlight-only** (App Store + AltStore PAL deferred to v2.1.0); the on-device closeout — the xcframework build on macOS (**Xcode 26 / iOS 26 SDK**), save-state migration from a v1.9.x install, and the AccuracyCoin / SMB / Zelda determinism smoke on Apple silicon — is a **maintainer / v2.0.9** step. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.5]` + `docs/ios-v2.0.5-readiness.md` + `to-dos/plans/v2.0.5-v2.0.8-ios-finalization-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.4 "Harbor" ("Slipway")** (2026-07-08) — the fourth release of the **v2.0.x mobile-finalization train** and the **Android release-candidate** milestone. A **host / Android-only** cut: the cycle-accurate core is **unchanged and byte-identical to v2.0.3** (AccuracyCoin still **141/141, 100.00%**; nestest 0-diff; `#![no_std]` chip stack untouched), so no accuracy / save-state / determinism number moves. It stages the RC scaffolding a maintainer needs to upload the Android app to a Play Console testing track: the `release` build type wired to the upload keystore with a **graceful debug-signing fallback** (keyless CI / local `assemble{Foss,Play}Release` still produces an installable — debug-signed, never shippable — RC artifact); debug-only **StrictMode** diagnostics (`DebugStrictMode`, thread + VM, log-only, `BuildConfig.DEBUG`-guarded, inert in release) as the host complement to the on-device crash-free-rate / ANR gate; version-controlled **fastlane Play Console listing metadata** (`fastlane/metadata/android/{en-US,es-ES}/`); an **R8/ProGuard final hardening review** (keep set confirmed complete, none loosened); and the **version bump** (workspace `2.0.3 → 2.0.4`; Android `versionCode 20003 → 20004` / `versionName → 2.0.4`). The `foss` flavor stays **behaviour-identical**. **No store submission** (that is v2.1.0); the on-device closeout — real-keystore signing, internal/closed testing track, crash-free-rate + ANR gate on hardware, live monetization runtime, the deferred per-feature gate migration — is a **maintainer / v2.0.9** step. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.4]` + `to-dos/plans/v2.0.4-android-rc-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.3 "Harbor" ("Keel")** (2026-07-08) — the third release of the **v2.0.x mobile-finalization train** and the one that makes the octal-latch accuracy work real at the shipped default. The **2-cycle-ALE PPU fetch model is promoted from the experimental `mc-ppu-2cycle-ale` flag to the unconditional, only PPU fetch path** (ADR 0030), so the shipped default now scores **AccuracyCoin 141/141 (100.00%, RAM-authoritative)** — both **"ALE + Read"** (`$0491`) and **"Hybrid Addresses"** (`$0492`) pass out of the box (previously an honest 139/141). This is the genuine two-dot fetch (even-dot ALE-drive + `octal_latch` load; odd-dot `(address & 0x3F00) | octal_latch` splice + read) where the latch *naturally* carries the stale byte (`copy_v_delay = 4` → NT splice `$2F19` for Hybrid; `$2007`-ALE overlap freeze → `$0FFF` for ALE+Read), replacing v2.0.2's whole-dot `+1 coarse-X` stand-in. **Both experiment flags retired** (`mc-ppu-2cycle-ale` + `mc-ppu-bus-addr-hybrid`); stand-in code deleted; `octal_trace` survives behind the new default-off `ppu-octal-trace`. Verified: **60-ROM oracle 60/60** with two documented re-blesses (SMB3, Uchuu Keibitai SDF — single-tile `$2006`-during-render shifts, more TriCNES-faithful, audio/cycle byte-identical), nestest 0-diff, mmc3 18/18, `ppu_sprites` 19/19; ~10% headless frame-cost rise (~4.15 ms/frame). **Save-state:** additive **`PPU_SNAPSHOT_VERSION` 4 → 5** tail (netplay-rollback determinism; pre-v5 `.rns` still load; forward-incompatible with ≤v2.0.2 but not an ADR-0028 epoch break). Also: the **Harbor Android foss/play monetization glue** (step 5 — AppLovin MAX + RevenueCat 8.10.0 `MonetizationGate`, gating/paywall/session/progress; no-op `foss` twin; both flavors assemble, dormant pending v2.0.9 on-device verify) + a **host-localizable mobile bridge-warning** API (`HostWarning` enum + `drain_warning_codes()`). See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.3]` + `to-dos/plans/v2.0.3-2cycle-ale-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.2 "Harbor" ("Soundings")** (2026-07-08) — the second release of the **v2.0.x mobile-finalization train** and Harbor's **headline accuracy release**: the two new upstream AccuracyCoin PPU tests v2.0.1 documented as honest gaps — **"ALE + Read"** (`$0491`) and **"Hybrid Addresses"** (`$0492`) — are now **solved flag-on** by a whole-dot port of TriCNES's **octal-latch multiplexed-bus PPU model** (ADR 0030, commit `27c103c`), behind the pre-existing default-off `mc-ppu-bus-addr-hybrid` flag. **Shipped default stays honest 139/141 (98.58%), byte-identical to v2.0.1; flag-on the same build is verified 141/141 (100.00%)** (framebuffer 100%, nestest 0-diff, mmc3 A12 + IRQ all pass, `ppu_sprites` 19/19). The campaign corrected two ADR 0030 premises — **Mesen2 does NOT pass these tests** (both bytes `0x0A`; the correct oracle is TriCNES, the AccuracyCoin author's own MIT emulator, TriCNES (upstream) commit `9199870`), and **a whole-dot port suffices** (the full 2-cycle-ALE refactor was not required). Per the maintainer's **refine-then-promote** decision (ADR 0030), the flag ships **default-off** in v2.0.2 and is **promoted to default (shipped 141/141) in v2.0.3** — after the Hybrid `+1 coarse-X` approximation is reworked to a first-principles latch-carry model and gated on the 60-ROM commercial byte-identity oracle. No snapshot-format bump (`PPU_SNAPSHOT_VERSION` stays 4). **This release does not claim the shipped build is 141/141, nor that the flag is promoted.** See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.2]` + `to-dos/plans/v2.0.2-harbor-plan.md`.
- **Earlier in the train:** **RustyNES v2.0.1 "Harbor" ("Mooring")** (2026-07-08) — the first release of the **v2.0.x mobile-finalization train** on the v2.0.0 "Timebase" core: the Android core re-port + `foss`/`play` flavor-split scaffolding (ADR 0025), the **AccuracyCoin oracle re-sync** (catalog 144→146 rows / 139→141 assigned; measured honestly at **139/141, 98.58%** — the two new upstream PPU tests "ALE + Read" / "Hybrid Addresses" documented as gaps, then solved flag-on in v2.0.2 per ADR 0030), the **CI cost optimization** (heavy suite gated to `release/*` + a weekly cron), the **dependency sweep** (uniffi 0.32 / mlua 0.12 / wgpu-naga 29.0.4 / cc 1.2.66; wgpu 30 deferred on the egui 0.35 pin), and the **`mc-r1-dmc-abort-probe` housekeeping removal**. Every core change is behaviour-neutral, so the deterministic core is byte-identical to v2.0.0: the **139 passing** AccuracyCoin tests and nestest 0-diff are unchanged — only the *denominator* grew (139→141) as the oracle re-sync added the two new upstream PPU tests. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[2.0.1]` + `to-dos/plans/v2.0.1-harbor-plan.md`.
- **Historical anchor — the last v1.x release:** **RustyNES v1.10.0 "Arcade"** (2026-07-01) — the native **Libretro core** (`crates/rustynes-libretro` builds `rustynes_libretro` for RetroArch: allocation-free video, batched-audio dynamic-rate sync, WRAM/SRAM RetroAchievements maps, deterministic rollback-ready save-states) plus the egui 0.34.3 → 0.35.0 dependency-tier refresh. It closed an unbroken additive/off-by-default chain running all the way back to v1.0.0: the **v1.1.0 → v1.7.1 "Forge"** desktop-feature line, the **v1.8.0 → v1.8.9 "Atlas"** Android platform train, and the **v1.9.0 → v1.9.9 "Workshop"** iOS TestFlight train (see the sub-bullets below for each). AccuracyCoin has held **100.00% (139/139)** and nestest **0-diff** through every one of these releases; mapper coverage is **172 families** (Core / Curated / BestEffort, CI honesty-gated). RustyNES ships as: a native desktop app (Linux/macOS/Windows), a WebAssembly build (browser demo), a native Android app (GitHub-sideload; Google Play deferred to v2.1.0), a native iOS/iPadOS app (TestFlight; App Store deferred to v2.1.0), and a native Libretro/RetroArch core. See `docs/STATUS.md` (single source of truth) + `CHANGELOG.md` `[1.10.0]`…`[1.0.0]`.
- **v2.0.0 "Timebase" — released 2026-07-03.** The forward architectural milestone this Status block used to describe as a distant, high-risk future refactor (see "The path to v2.0.0" below, now updated) has landed and shipped: the one-clock/every-cycle-bus-access scheduler promote (beta.1→beta.4, PRs #217-220), full Vs. `DualSystem` dual-console support with a real commercial-title boot (beta.5, PR #221), and the save-state/movie format break + the two capstone ADRs (rc.1, PR #222 — ADR 0028 save-state v3 + ADR 0029 the timebase architecture) are all merged to `main`. AccuracyCoin held 100% (139/139) at every gate across all five betas + rc.1. The MMC3 R1/R2 IRQ-timing residual was investigated exhaustively (21+ documented attempts total, including two dedicated 2026-07-02 campaigns) and is by-design-deferred beyond v2.0.0 with a mechanism-level explanation (ADR 0002's decision-update section) rather than closed — this is the one known gap in an otherwise complete cut. The tag + release-ceremony + binary publish are done, and the **v2.0.1 "Harbor" ("Mooring")** train now builds on it.
- **RustyNES feature/platform-release history (on the v1.0.0 core; all additive / off-by-default; AccuracyCoin held 100% (139/139) throughout):**
  - **v1.1.0 "Scriptable"** (2026-06-15) — full NES_NTSC composite + CRT/scanline shaders + `.pal` palette filters; NES Power Pad + turbo/autofire + an input-display overlay + a per-game nametable-mirroring override DB; debugger breakpoints + a cycle trace logger + an event viewer (behind `debug-hooks`); an NSF/NSFe player + a 5-band graphic EQ; and the flagship **Lua scripting engine** (`rustynes-script`, ADR 0010). See `CHANGELOG.md` `[1.1.0]`.
  - **v1.2.0 "Curator"** (2026-06-15) — library / compatibility / reach: mapper tiering (Core / Curated / BestEffort, ADR 0011) **51 → 87 families** behind a CI honesty gate; `.zip` loading + `.ips`/`.ups`/`.bps` soft-patching; a per-game DB + in-app ROM-Database editor; live NTSC knobs + a composable ShaderStack + CRT preset bank (ADR 0013) + a default-off HD-pack loader; Family BASIC keyboard / SNES mouse / Arkanoid-both-ports / Game-Genie code DB; Lua `onNmi`/`onIrq`/`setInput`; menu-bar UX + FontAwesome icons; web touch controls + Power Pad + an experimental wasm Lua piccolo backend (ADR 0012); a turn-key netplay `deploy/` bundle; and a PGO CI gate. The SMB3 World 1-1 sprite-flicker (a PPU OAM-row-corruption bug) and the Mapper 89 bus conflict were fixed. See `CHANGELOG.md` `[1.2.0]`.
  - **v1.3.0 "Bedrock"** (2026-06-16) — toolchain modernization (edition 2024 / Rust 1.96 / egui 0.34.3 + wgpu 29.0.3 + rfd 0.17.2); a frame-pacing fix; a Memory Compare panel + a menu/Settings reorg + per-setting auto-save; mapper coverage **87 → 101 families** + Vs. DualSystem header detection (NES 2.0 byte-13); HD-pack `<condition>`/`<background>` rules (ADR 0014); netplay desync diagnostics + niche peripheral aliases; and a PGO/BOLT CI gate. See `CHANGELOG.md` `[1.3.0]`.
  - **v1.4.0 "Fidelity"** (2026-06-16) + the **v1.4.1** patch (2026-06-16) — accuracy polish (triangle ultrasonic silence; the DMC-DMA ↔ controller-read conflict verified + documented); per-channel audio mixing; devtools finish (symbol-file `.sym`/`.mlb`/`.nl` loading + event breakpoints); browser QoL (wasm `.rnm` movie I/O + IndexedDB save-states); a measure-first perf pass (−8% on the rendering-heavy bench); a clap-4 styled `--help` + a `rustynes help` ratatui TUI (native-only); and mapper coverage **101 → 113 families** (boot-smoke verified). v1.4.1 added four more BestEffort boot/decode fixes (m92 / m94 / m145 / m147) + a screenshot-corpus tier reorg. See `CHANGELOG.md` `[1.4.0]` + `[1.4.1]`.
  - **v1.5.0 "Lens"** (2026-06-17) — the insight + scriptability + creator-tooling + polish release, eight additive workstreams: debugger visualization (Input Miniatures overlay, PPU event-viewer heatmap, per-scanline trace viewer, HD-pack per-pixel inspector); Lua dev/TAS API depth; creator/TAS tooling (a TASVideos compatibility pass, NSF waveform scope); frontend pacing & audio-sync perf; a native-UI overhaul + in-app Documentation pane; UX polish (named-palette editor, an "Enhancements" group with sprite-limit-disable/overclock staged-but-inert pending v2.0 per ADR 0002); accessibility (UI scaling, high-contrast + Okabe-Ito themes, keyboard-only nav); mapper breadth **113 → 123 families**; and casual-mode browser RetroAchievements *scaffolding* (ADR 0015, off-by-default `browser-cheevos`). See `CHANGELOG.md` `[1.5.0]`.
  - **v1.6.0 "Studio"** (2026-06-18) — the studio / TAS-tooling / debugger-depth / accuracy-and-breadth release: the TAStudio piano-roll TAS editor + `.fm2`/`.bk2` movie interop + Lua driving/data; Mesen2-class debugger depth (expression/conditional breakpoints + R/W/X watchpoints + a hex editor + RAM search); off-axis-accuracy verification; mapper breadth → **150 families** + the UNIF (`.unf`) loader; FDS-proper; A/V recording; HD audio; and the shader/filter ecosystem (LMP88959 NTSC/PAL + hqNx/xBRZ + constrained `.slangp`/`.cgp` import). See `CHANGELOG.md` `[1.6.0]`.
  - **v1.7.0 "Forge"** (2026-06-19) + the **v1.7.1** patch — the writable/programmable-tooling + accuracy + mapper-breadth + reach release (MAXIMAL A–H over five betas + a wave-2 reach pass): F accuracy hardening; G1 reusable-ASIC mappers **150 → 168 families**; A editing-capable tools + inline 6502 assembler; C debugger depth (callstack/step + `.dbg` source maps); B scriptable TAStudio (`tastudio.*`) + full Lua parity; E host IPC/automation behind the off-by-default `script-ipc` feature (ADR 0016); D Zwinder rewind + movie import; G2/G3 expansion-audio; G5 HD-Pack Builder (ADR 0017) + the real-Mesen `<tile>` loader fix (ADR 0018); plus the H1–H9 reach wave (browser-RA finish + RA HUD, spectator netplay, per-game `<rom>.json` overrides + DIP editor + lag counter (ADR 0019), audio depth (ADR 0020), web/wasm parity (ADRs 0021/0022), an i18n framework (ADR 0023), and the `full` maximal-native-feature build). v1.7.1 added seven bugfix/polish fixes. See `CHANGELOG.md` `[1.7.0]` + `[1.7.1]`.
  - **v1.8.0 … v1.8.9 "Atlas"** (2026-06-19 … 2026-06-20) — the **Android platform train** (the first *platform* releases; new crates `rustynes-mobile` UniFFI bridge + `rustynes-android` JNI glue + an `android/` Gradle/Compose app, ADR 0024). v1.8.0 foundation → v1.8.5 power-user (palette/HD-pack/`.zip`/movies) → v1.8.6 (Lua + RA + direct-IP/LAN netplay) → v1.8.7 "Connectivity completion" (CGNAT/TURN room-code netplay + robust hardware controllers P1–P4) → v1.8.8 "Atlas" (AGP9/Gradle9 + Window-Size-Class adaptive + edge-to-edge/Material You; EN/ES i18n; box-art library; Baseline Profiles + R8 full-mode; capture/MP4-clip + PiP/tile/shortcuts/Glance-widget; TV/Leanback + a11y; Play Games cloud-saves/achievements + Play-Integrity + update/review/vitals, all default-off) → **v1.8.9** (13-PR Dependabot consolidation; a then-dormant `rustynes-monetization` crate wired into the Android build — **since removed permanently in v2.2.6 per ADR 0035; it never went live**). See `CHANGELOG.md` `[1.8.0]`…`[1.8.9]`.
  - **v1.9.0 … v1.9.9 "Workshop"** (2026-06-25 … 2026-06-26) — the **iOS/iPadOS TestFlight train**, mirroring the Android arc release-for-release on the byte-identical core (new crates `rustynes-ios` Metal/CoreAudio shim reusing `rustynes-mobile` verbatim, ADR 0026). v1.9.0 "Sunrise" foundation (SwiftUI shell + xcframework) → v1.9.4 "Lens" (full wgpu→Metal renderer + WGSL shader stack) → v1.9.6 "Link" (Lua + RetroAchievements + LAN netplay) → v1.9.7 "Relay" (CGNAT/TURN room-code netplay + iCloud/CloudKit save-state sync) → v1.9.8 "Horizon" (accessibility + EN/ES i18n + ReplayKit + Game Center + the dormant StoreKit seam, ADR 0027 §4.7 compliance) → **v1.9.9 "Workshop"** (creator/power tools: Cheats, a FOSS-gated read-only debugger, a touch TAStudio piano-roll, foreign movie import, host-side audio-depth DSP — the final pre-Timebase readiness gate). Distributed by TestFlight only; App Store deferred to v2.1.0 alongside Google Play. See `CHANGELOG.md` `[1.9.0]`…`[1.9.9]`.
  - **v1.10.0 "Arcade"** (2026-07-01) — the native **Libretro core** (`crates/rustynes-libretro`, RetroArch integration) + the egui 0.34.3 → 0.35.0 dependency-tier refresh. See `CHANGELOG.md` `[1.10.0]`.
- **Historical — the v2.0.0 tag itself, shipped 2026-07-03.** All development work for v2.0.0 "Timebase" is merged to `main` (see the bullet above); the only remaining step is the release ceremony (pre-release gate checklist, tag, `release-auto.yml` binary publish).
- **Engine-lineage — the "optimized performance" pass** (folded into v1.0.0): a frontend + build performance pass — a Performance panel + CSV "Logging" checkbox; a lock-free SPSC audio ring + **dynamic rate control**; a **display-sync pacing matrix** (`auto|display|vrr|wallclock`) + **late input latch**; a **snapshot fast path** (36→14.6 µs) + **run-ahead** (default 1, persistent timeline byte-identical); **mapper-caps + pixel-LUT + fat-LTO + SIMD** (**−26%** rendering-heavy bench, −16% nestest); a **dedicated emulation thread** (default-ON `emu-thread`, lock-free `SharedInput`, netplay-pause TOCTOU-closed) + best-effort Linux priority elevation; and a browser **AudioWorklet** + **rAF display-sync**. See `docs/release-notes/v2.8.0.md` (engine-line detail).
- **Engine-lineage — the master-clock milestone:** the engine's v2.0 line made the R1 `u64` master clock the default (AccuracyCoin 90.65%→**100.00%**, region-exact 3.2:1 PAL via the unified DMA engine) and then removed the legacy integer-lockstep scheduler (R1 is the only path; the `mc-r1-*` flags no longer exist). See `docs/audit/v2.0-phase7f-r1-default-promotion-2026-06-10.md`.

> **The bullets that follow (down to the engine-lineage Phase 6 entry) are the
> inbound engine's own release line — its `v1.x`/`v2.x` tags + 2026-05-2x dates.
> They are *engine history*, folded into the RustyNES v1.0.0 core; they are NOT
> the RustyNES v1.x feature releases listed at the top of this Status block.**

- **Engine-lineage phase:** **engine v1.7.0 (2026-05-25)** — **niceties milestone**: Four Score 4-player support (bus `$4016`/`$4017` 24-read multiplex of 4 controllers + adapter signature; opt-in, OFF by default = byte-identical two-controller reads; a P3/P4 keyboard + gamepad rebind UI + a "Four Score" toggle), GameShark-style raw RAM cheats (`Nes::poke_ram` applied caller-side after `run_frame`, alongside the v1.6.0 Game Genie support; a `RawCheat` `$addr=$value [if $compare]` section in the cheat panel persisted per-ROM), and an in-app graphics/audio/rewind settings panel. **Additive, independent of the deferred v2.0 master-clock axis**; AccuracyCoin held **90.65%**, oracle 60/60, sacred trio + B4 byte-identical, determinism preserved. Workspace `--features test-roms`: **702 strict + 10 ignored**. See `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` §2 + `CHANGELOG.md` `[1.7.0]`.
- **Engine-lineage phase:** **engine v1.6.0 (2026-05-25)** — **frontend-polish milestone** (the engine's v2.0.0 plan's original v1.5.0 content, deferred when Phase 7 took that slot). **Additive, independent of the deferred v2.0 master-clock axis**; AccuracyCoin held **90.65%**, oracle 60/60, sacred trio + B4 byte-identical, determinism preserved. Landed across 6 sprints: (0) `x86_64-apple-darwin` release target dropped (ADR 0009, Aug-2027 runner sunset); (1) Game Genie cheats (core `rustynes-core/src/genie.rs` runtime overlay — off by default, not in the save-state — + a debugger cheat panel with per-ROM persistence); (2) in-app gamepad rebinding UI (config-driven `[input.gamepad1/2]` + P2 keyboard rows + axis-as-dpad; serde default = the legacy Xbox layout); (3) controls/configuration doc-sync; (4) browser (wasm) `.rnm` movie download/upload + localStorage save-states; (5) a non-flaky frame-time regression CI gate + a rendering-heavy `flowing_palette` bench. Workspace `--features test-roms`: **688 strict + 10 ignored**. See `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` + `CHANGELOG.md` `[1.6.0]`.
- **Engine-lineage phase:** Phase 7 — **engine v1.5.0 (2026-05-24)**: **Nesdev Accuracy Hardening** (the genuinely-skipped phase; see numbering note below). Coverage + region validation + developer ergonomics + documented scope closure — **additive only**, AccuracyCoin held at **90.65%**, oracle 60/60, sacred trio + B4 byte-identical. Landed across 4 sprints: (1) blargg `instr_misc`/`instr_timing`/`cpu_reset` corpus wired (+8 strict); (2) seeded power-on RAM randomization developer mode (`Nes::from_rom_with_power_on_seed`; default path unchanged) + NMI/IRQ B-flag + `$4015` open-bus guards; (3) automated PAL/Dendy timing gates (per-region constant table + frame-structure integration test); (4) VRC2/4 + M34 NINA-001 submapper fixtures (replacing the rotted `vrc24test`) + `compatibility.md` platform-scope closure (FDS plan, Vs/PC10, PPU variants, input devices, long-tail policy). Workspace `--features test-roms`: **661 strict + 10 ignored**. Deferred to v2.0 (master-clock axis): C1 IRQ-sample, `$2002` sub-cycle, SH\* internal-bus, stale-shifter, `$2007` rendering, FDS code, PAL 3.2:1 CPU:PPU ratio. See `docs/audit/phase-7-*` + `CHANGELOG.md` `[1.5.0]`.
- **Engine-lineage phase:** Phase 10 — **engine v1.4.0 (2026-05-24)**: **TAS movie recording/playback**. Deterministic `.rnm` record/replay + save-state branching (ADR 0008: `RNESMOV1` header + ROM SHA-256 + optional `.rns` start point + per-frame input stream); `MovieRecorder`/`MoviePlayer` in `rustynes-core` (no_std) + record/play/branch hotkeys (`F6`/`F7`/`F8`) + a read-only REC/PLAY egui overlay; native `.rnm` save/load (wasm I/O is a follow-up). No API break (additive `Nes::buttons` getter; `run_frame` byte-for-byte unchanged) → oracle 60/60, AccuracyCoin 90.65%, B4 + sacred trio preserved. Determinism proven by byte-identical round-trip tests; **636 strict + 8 ignored**. Clean-room from Mesen2 `Core/Shared/Movies/` + FCEUX `.fm2` + TetaNES `.replay`. Delivered across Sprints 4.1 (core) + 4.2 (frontend UI). The prior **Phase 9 — v1.3.3 RELEASED (2026-05-24)**: bug-fix patch (frontend-only; native unchanged, pixel-identical) closing two wasm/GitHub-Pages issues + a native pacing refinement — (1) wasm/Pages severe stutter + freezes (v1.3.2 regression): the wasm idle path busy-looped on `ControlFlow::Poll` alongside the rAF loop + a missing `request_redraw()` re-arm could stall it; fixed to `ControlFlow::Wait` + an unconditional rAF re-arm; (2) wasm/WebGL2 palette wrong: wgpu-hal double-encodes sRGB on the GL surface, so the GL pipeline now stays UNORM (zero conversion, matches the correct canvas-2D path); native keeps sRGB → pixel-identical; (3) native residual stutter: chunked pacer sleep + 2 ms spin margin. Both wasm fixes need browser confirmation. Workspace **616 strict + 6 ignored** (unchanged). The prior **v1.3.2 RELEASED (2026-05-24)** closed two v1.3.1 follow-ups: dead keyboard input after the config migration (`parse_keycode` legacy keycode aliases) + a first wasm rAF-pacing attempt. **v1.3.1 RELEASED (2026-05-24)** was a bug-fix patch on the v1.3.0 WebAssembly milestone with three fixes (no API break, no accuracy change): (1) green/garbage left-edge column while scrolling — BG attribute (palette) shifters were one tile out of phase with the pattern shifters (`086ce4d` regression), now 16-bit + lockstep (AccuracyCoin-neutral; PPU save-state v1→v2); (2) stutter / non-smooth framerate — configurable present mode (default `Mailbox`) + a native sleep-then-spin frame pacer replacing the jittery `ControlFlow::WaitUntil` cadence (user-confirmed smooth); (3) legacy `config.toml` now migrated in place (backup + loud summary) instead of silently dropped. MM3 MMC3 stage-select shear investigated, confirmed not-a-regression, deferred to v2.0 (C1 axis). Oracle 60/60; AccuracyCoin 90.65%; B4 + sacred trio preserved. See `CHANGELOG.md` `[1.3.1]`. **v1.3.0 (2026-05-24)** landed the WebAssembly target: `wasm32-unknown-unknown` frontend in two flavours (`wasm-winit` default = full winit+wgpu+egui, 2.12 MiB gzip; `wasm-canvas` ~316 KB embed), GitHub Pages deploy (`https://doublegate.github.io/RustyNES/`), CI `wasm` clippy job + 5 MiB size-budget gate, all Pages actions on Node 24 — delivered across Sprints 1.1 → 1.2 → 1.3 → 1.4a → 1.4b → 1.4c → 2.
- **Engine-lineage phase:** Phase 8 — **engine v1.2.0 (2026-05-24).** DMC DMA scheduler refactor landed under default-off cargo feature `dmc-get-put-scheduler` introducing Mesen2's canonical get/put cycle alternation model alongside the v1.1.0 phase-agnostic scheduler via the parallel-implementation pattern (ADR 0007). AccuracyCoin DMA cluster under flag-on: **6/10 match baseline** (closing 4 → 0 deferred to v1.2.x patches or v2.0 master-clock absorption). Default build bit-identical to v1.1.0.
- **Engine-lineage — earlier work:** **engine v1.1.0 (2026-05-25)** — VRC7 OPLL FM audio via clean-room pure-Rust port of `emu2413 v1.5.9` (MIT); ADR 0006 supersedes ADR 0004; *Lagrange Point* plays with audio. (engine v1.1.0 was an engine v2.0.0-release-plan milestone slotted between Phase 6 and Phase 8, **not** the ROADMAP's Phase 7 — see the numbering note below.) Phase 6 — **engine v1.0.0 (2026-05-23)**: AccuracyCoin gate CLEARED at 90.65% (126/139); T-60-001 C1 IRQ-timing residuals (3 `cpu_interrupts_v2` sub-ROMs + `mmc3_test_2/4` #3) deferred to the master-clock-precise scheduling refactor (Session-29 empirically falsified Option A global PPU-position shift; 17 documented rollbacks). [That engine-lineage master-clock work subsequently landed in the RustyNES v1.0.0 core, taking AccuracyCoin to 100%.]
- **Phase-numbering note:** the shipped releases v1.1.0 → v1.4.0 were sequenced from the v2.0.0 release plan and back-labelled in the detailed sections as v1.1.0 (VRC7) → Phase 8 (v1.2.0 DMC) → Phase 9 (v1.3.0 wasm) → Phase 10 (v1.4.0 TAS). **Phase 7 — Nesdev Accuracy Hardening (below) was authored but never executed**; it is now being executed as **v1.5.0**. See `docs/audit/phase-7-assessment-2026-05-24.md` for the full intent-vs-accomplished-vs-completable disposition.
- **Current state (historical snapshot; see the Status section at the top for the live answer):** at the time this bullet was written, **RustyNES v2.2.6 "Almanac" was the latest release** (see the Status section at the top; the v2.2.4 "Cartridge" libretro-cut detail there is historical). `docs/STATUS.md` + `CHANGELOG.md` carry the authoritative v2.1.0 → v2.2.6 line. **v2.0.0 "Timebase" shipped 2026-07-03** — the paragraph below describing it as "code-complete, tag pending" is retained as a historical snapshot of that release's landing, not a current-state claim. Every accuracy, compatibility, platform, netplay, RetroAchievements, FDS, Vs/PC10, and performance milestone in the engine-lineage history above is folded into the v1.0.0 core; the v1.1.0 → v1.7.x feature releases then layered (in order) the Lua scripting engine + visual filters/peripherals/devtools/NSF, the library/compatibility/reach pass, the toolchain modernization + Memory-Compare + Vs.-DualSystem detection, the accuracy-and-finish pass, the insight/scriptability/creator-tooling/polish pass, the studio/TAS-tooling/debugger-depth pass, and the writable/programmable-tooling "Forge" pass; the v1.8.x train ported the whole core to Android, the v1.9.x train ported it to iOS/iPadOS, and v1.10.0 added the native Libretro core. Mapper coverage rose **51 → 172 families** across these releases, all additive / off-by-default, with AccuracyCoin holding **100% (139/139)** the entire time. v2.0.0 then landed the one-clock/every-cycle timebase promote + full Vs. `DualSystem` support + the save-state/movie format break — the first genuinely BREAKING release since v1.0.0, by design (ADR 0028/0029). The engine-lineage version markers (v0.9.x → v2.x) in the bullets above and the phase bodies are upstream history, not RustyNES releases.

**v2.0.0 "Timebase" — historical landing snapshot (shipped 2026-07-03; this section was written as it landed). What shipped (2026-07-01 → 2026-07-03):**

The forward architectural milestone this section used to describe as a distant, XL/HIGH-risk future refactor has landed, across beta.1 → beta.5 → rc.1 (PRs #217-222). What was originally scoped as workstreams A-F in `to-dos/plans/v2.0.0-master-clock-plan.md`:

- **A — the one-clock, every-cycle-bus-access timebase (beta.1 → beta.4, PRs #217-220).** Collapsed the five-counter substrate (`Cpu::master_clock`, `Cpu::cycles`, `LockstepBus::cycle`, `LockstepBus::ppu_clock`, `Apu::cpu_cycle`) to ONE canonical counter; made every CPU instruction cycle a real bus access (no busless filler cycles, matching Mesen2's `StartCpuCycle → Read → EndCpuCycle` split-around-the-access model); a cycle-accurate warm-reset sequence. Promoted to the shipped default in beta.4 (BREAKING by design, ADR 0029). AccuracyCoin held 100% (139/139) at every gate.
- **B — residual closure (beta.3 + the 2026-07-02 bounded-effort campaign).** R3 (`apu_reset/len_ctrs_enabled`) closed — reclassified as a harness bug, not a core residual. R4 (`apu_reset/4017_written`) closed via the cycle-accurate reset. R5 (DMC-DMA span) found already-closed pre-beta.1. **R1/R2 (the MMC3 IRQ-timing bracket) investigated exhaustively — 21+ documented attempts total (17 historical + 4 new on 2026-07-02) — and by-design-deferred beyond v2.0.0**, not closed: a mechanism-level finding (the bracket measures a differential interval invariant to any consistent batch re-phasing) explains why every phase/order lever has failed, and identifies the true fix as needing a genuinely finer-than-CPU-cycle scheduler granularity. See ADR 0002's 2026-07-02 decision-update for the full evidence trail and the DO-NOT-RETRY list.
- **C — full Vs. `DualSystem` dual-core support (beta.5, PR #221).** The four `DualSystem` cabinet boards (Tennis, Mahjong, Wrecking Crew, Balloon Fight) now construct and run as genuine two-console pairs via the `Emu` enum front door — core-and-test-harness-only this release (frontend dual-console rendering deferred). **Vs. Balloon Fight boots to a legible, correct attract-mode screen** on a combined dump assembled from a legitimately-owned MAME romset (the previously-circulating "GVS" dumps are provably incomplete — MAME `maincpu` region only, confirmed by CRC32 cross-reference). Wrecking Crew is inconclusive (cross-wiring demonstrably active, but no confirmed title screen); Tennis and Mahjong remain infrastructure-only (no local sub-CPU dump available). This retires the "not yet emulated" DualSystem deferral this section used to carry.
- **D — the breaking-API/save-state/doc-baseline close (rc.1, PR #222).** `CPU_SNAPSHOT_VERSION` 2→3 + `save_state::FORMAT_VERSION` 1→2 (ADR 0028 — clean rejection of pre-v2.0.0 `.rns` slot files, no migration code, per ADR 0003's own MAJOR-boundary policy) and `MOVIE_FORMAT_VERSION` 1→2 (warn-not-reject for `.rnm` movies — input replay still works, the bit-identical guarantee is flagged unverified across the boundary). ADR 0029 formalizes the one-clock timebase as the canonical architecture, superseding the dot-lockstep framing; `docs/architecture.md` got the same banner treatment `docs/scheduler.md`/`docs/cpu-6502.md`/`docs/apu-2a03.md` already had.
- **E — mapper breadth.** Frozen at **172 families** for the v2.0.0 cut (no mapper work landed in the v2.0.0 line — confirmation, not a change).
- **F — perf re-baseline.** Done in beta.4; both configurations clear the 16.639 ms NTSC deadline with wide margin.

**Remaining:** the tag + release-ceremony + binary publish. Once tagged, v2.0.0 becomes the prerequisite the mobile finalization train below has been waiting on.

**Beyond v2.0.0 — the mobile finalization train (maintainer decision, 2026-06-23; unchanged by v2.0.0's completion, just now unblocked):**

- **The Android (v1.8.x) and iOS (v1.9.x) apps ship together, after v2.0.0 — the v2.0.1 → v2.1.0 finalization train.** Both apps were deliberately held back from their app stores until the v2.0.0 "Timebase" core landed, so they can finalize and launch **together**: **v2.0.1–v2.0.4** = final Android additions/modifications/enhancements/fixes re-ported onto the v2.0.0 core; **v2.0.5–v2.0.8** = the same iOS finalization; **v2.0.9** = true correctness checks + ready-for-release verification for *both* apps. Until then the apps continue as **GitHub-sideload** (Android) and **TestFlight** (iOS v1.9.0–v1.9.9, already complete) only. Full plan: [`plans/v2.0.x-mobile-finalization-plan.md`](plans/v2.0.x-mobile-finalization-plan.md).
  - **No monetization (ADR 0035, v2.2.6).** The previously-planned ad-supported freemium model (AppLovin MAX + RevenueCat, a one-time "Remove Ads" unlock, rewarded-ad session extensions, premium features) is **removed permanently** — RustyNES is open-source and income-free forever. The `rustynes-monetization` crate is deleted. The native apps remain **free FOSS apps** (no ads, no tracking, every feature unlocked). Any future store listing is a **free** app.
  - **The `foss` / `play` Android flavor split (ADR 0025, amended by 0035).** A **`foss`** flavor (default — no Google SDKs; the F-Droid + sideload artifact) and a **`play`** flavor (the *free* Google-Play services — Play Games achievements, Cast, Integrity, in-app update, cloud save; no ads, no billing). The service subsystems live behind `src/play/` façades (no-op in `src/foss/`).
- **Beyond v2.1.0 (separate initiatives, no fixed version yet).**
  - **The R1/R2 MMC3 IRQ-timing axis** — the one open technical gap from v2.0.0 (see above). Next credible avenue per the 2026-07-02 campaign: M2-edge-precise (not CPU-cycle-integer) `gap >= 3` low-time accounting on the falling edge, an axis distinct from everything tried so far — genuinely untested, flagged for a future dedicated session rather than squeezed into any near-term release.
  - **Vs. Tennis and Vs. Mahjong DualSystem boot** — needs the missing sub-CPU program dumps (not available locally as of v2.0.0; Balloon Fight and Wrecking Crew's dumps were sourced from a legitimately-owned MAME romset).
  - **Vs. DualSystem frontend integration** — dual-console rendering + 4-port input routing; core-only as of v2.0.0.
  - **Browser / wasm Lua** maturity (the native Lua engine is feature-complete; the wasm piccolo backend, ADR 0012, is explicitly not byte-parity with native mlua).
  - **Finishing browser RetroAchievements** — the v1.5.0 scaffolding (ADR 0015, off-by-default `browser-cheevos`) needs the auth-proxy deploy, the wasm trampoline marshalling, and a live-browser verify; native RA is unaffected. Plus the live RA-account allowlisting pass with the RA team (the `RustyNES/<ver>` User-Agent is already sent; the allowlisting itself is a request, not a code change).
  - **Long-tail mapper coverage** toward the full ~300-mapper set + **100% TASVideos** compatibility.
- **Engine-lineage forward-roadmap history (folded into the v1.0.0 core; retained for context — NOT a RustyNES release plan):** the inbound engine's own roadmap completed engine v2.6.0 (Vs/PC10 RGB game-verified, +11 mappers→51, N-peer netplay, real-BIOS FDS), engine v2.7.0 (RetroAchievements via the vendored rcheevos FFI; the Vs.-System per-game DIP/2C04-palette DB; deployable browser WebRTC netplay), and engine v2.7.1 (netplay-hardening + live verification, the `power_cycle` cold-boot desync fix, the >2-player browser WebRTC mesh, RA fixes, the MMC6 PRG-RAM fix, the NTSC-filter WGSL crash fix, Vs. DualSystem detection groundwork). All of this is present in the RustyNES v1.0.0 core; stock NES is byte-identical and AccuracyCoin is 100%.
- **Done:** Phases 1-4 complete; Phase 5 Sprints 1-3 shipped — Frontend MVP, save state + rewind + TOML rebinding, egui debugger overlay (CPU/PPU/OAM/APU/memory/mapper panels + in-app rebind modal closing T-52-007), simplified Blargg-style NTSC wgsl post-pass, release workflow + README badges. **Regression-prevention buildout closed (2026-05-17):** 21-ROM permissive baselines + 60-ROM commercial-ROM oracle (54 strict + 6 ignored across 15 mappers) + 81-PNG visual corpus + permanent `scripts/regression-bisect/` tooling + `docs/audit/` decision-rationale tier. Real-game regression on SMB / Excitebike / Kid Icarus closed by the FSM dot-64 reset fix on `accuracy-stabilization` (`834be9e`). Residual accuracy gaps tracked in `CHANGELOG.md` `[Unreleased]` → "Investigated and rolled back". (Historical note: when this bullet was written, v1.0.0 was still gated on the C1 IRQ-timing rework + AccuracyCoin ≥ 90% (then 69.78%) + multi-OS smoke + the 6 ignored commercial ROMs. All of those resolved: **v1.0.0 released** (the 90.65% gate was an interim engine-lineage milestone), the 6 ROMs are strict-passing, and the master-clock refactor (the engine-lineage "v2.0" axis) **shipped as the v1.0.0 default core**, closing the C1 + sub-cycle residuals — the default build measures **AccuracyCoin 100%**. See `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` for the historical trajectory.)
- **Status matrix (single source of truth):** see [`docs/STATUS.md`](../docs/STATUS.md) for the per-test-ROM-suite pass count, mapper coverage matrix, feature flag state, and version policy. This roadmap intentionally keeps a short summary only.
- **Deferred / carryover backlog:** see [`DEFERRED-AND-CARRYOVER-FEATURES.md`](DEFERRED-AND-CARRYOVER-FEATURES.md) for the consolidated catalogue of every deferred, carried-over, manual-verify, and not-yet-implemented feature (reconciled against `main`), grouped by theme with target releases and source plans/ADRs.

## Phases

> **Reminder:** the `v1.x`/`v2.x` version tags inside the Phase bodies below are
> **engine-lineage** markers (the inbound engine's own line, dated 2026-05-2x),
> retained as historical anchors. They are **not** the RustyNES v1.1.0 → v1.8.8
> feature/platform releases (dated 2026-06-1x) tracked in the Status block above,
> and the
> Phase-body "v2.0" deferrals refer to the *engine's* master-clock work that
> already shipped in the RustyNES v1.0.0 core — distinct from the forward
> **RustyNES v2.0.0** timebase refactor (ADR 0002) in "The path to v2.0.0".

### Phase 1 — Foundation

**Goal:** Empty Cargo workspace builds cleanly with CI green; cartridge parser passes round-trip tests; CPU executes the nestest golden log without diverging.

**Exit criterion:** `cargo test --workspace` green; `nestest.nes` golden-log compare passes; iNES + NES 2.0 parser handles the test ROM corpus without errors.

**Estimated duration:** 4-6 weeks

[Phase 1 overview](archive/phase-1-foundation/overview.md)

Sprints:

- [Sprint 1 — Workspace + CI + lints](archive/phase-1-foundation/sprint-1-workspace.md)
- [Sprint 2 — Cartridge parser (iNES + NES 2.0)](archive/phase-1-foundation/sprint-2-cartridge.md)
- [Sprint 3 — CPU core: official opcodes](archive/phase-1-foundation/sprint-3-cpu-official.md)
- [Sprint 4 — CPU core: unofficial opcodes + nestest](archive/phase-1-foundation/sprint-4-cpu-unofficial.md)

---

### Phase 2 — Graphics + Timing

**Goal:** PPU renders correct pictures for NROM, MMC1, UxROM, AxROM, CNROM, GxROM titles; lockstep scheduler operational; blargg PPU test ROMs pass.

**Exit criterion:** `ppu_vbl_nmi/*`, `ppu_open_bus`, `sprite_overflow_tests/*`, `oam_read`, `oam_stress` all pass; visual diff against Mesen2 reference for a curated demo set.

**Estimated duration:** 6-8 weeks

[Phase 2 overview](archive/phase-2-graphics-timing/overview.md)

Sprints:

- [Sprint 1 — PPU bus, registers, memory map](archive/phase-2-graphics-timing/sprint-1-ppu-bus.md)
- [Sprint 2 — Background rendering + scrolling](archive/phase-2-graphics-timing/sprint-2-background.md)
- [Sprint 3 — Sprite evaluation + rendering + sprite-zero hit](archive/phase-2-graphics-timing/sprint-3-sprites.md)
- [Sprint 4 — Lockstep scheduler + DMA + simple mappers (NROM, UxROM, CNROM, AxROM, GxROM, MMC1)](archive/phase-2-graphics-timing/sprint-4-scheduler-mappers.md)

---

### Phase 3 — Audio + Polish

**Goal:** APU produces correct audio; lookup-table mixer and analog filter chain in place; band-limited synthesis emits at host sample rate; CPU illegal opcodes complete.

**Exit criterion:** `apu_test/*`, `apu_mixer/*`, `dmc_dma_during_read4/*`, `cpu_interrupts_v2/*` all pass.

**Estimated duration:** 4-6 weeks

[Phase 3 overview](archive/phase-3-audio-polish/overview.md)

Sprints:

- [Sprint 1 — APU channels (pulse 1, pulse 2, triangle, noise)](archive/phase-3-audio-polish/sprint-1-apu-channels.md)
- [Sprint 2 — DMC channel + DMC DMA + frame counter](archive/phase-3-audio-polish/sprint-2-dmc-frame.md)
- [Sprint 3 — Mixer + filters + band-limited synthesis](archive/phase-3-audio-polish/sprint-3-mixer.md)

---

### Phase 4 — Mapper Coverage

**Goal:** Top-25 mappers implemented; MMC3 IRQ accuracy validated; MMC5 (no audio); audio extension mappers (VRC6, Sunsoft 5B, Namco 163) functional.

**Exit criterion:** Per-mapper boot test passes for one ROM per supported mapper; `mmc3_test_2/*`, `mmc3_irq_tests/*`, `vrc24test`, holy_mapperel pass; AccuracyCoin pass rate ≥ 80%.

**Estimated duration:** 6-8 weeks

[Phase 4 overview](archive/phase-4-mapper-coverage/overview.md)

Sprints:

- [Sprint 1 — MMC3 (the defining mid-life mapper)](archive/phase-4-mapper-coverage/sprint-1-mmc3.md)
- [Sprint 2 — MMC2/MMC4 + Color Dreams + CPROM + BNROM/NINA + Camerica + VRC1](archive/phase-4-mapper-coverage/sprint-2-misc-mappers.md)
- [Sprint 3 — VRC2/4/6 + Sunsoft FME-7 + Namco 163](archive/phase-4-mapper-coverage/sprint-3-vrc-extended.md)
- [Sprint 4 — MMC5 (without audio extension)](archive/phase-4-mapper-coverage/sprint-4-mmc5.md)

---

### Phase 5 — Frontend + Tooling

**Goal:** `rustynes` binary playable end-to-end with save state + rewind + debugger overlays + NTSC filter; CI publishes signed binaries on tag.

**Exit criterion:** Binary builds and runs on Linux/macOS/Windows; passes manual smoke test of compatibility-difficulty corpus; release pipeline green.

**Estimated duration:** 4-6 weeks

[Phase 5 overview](archive/phase-5-frontend-tooling/overview.md)

Sprints:

- [Sprint 1 — winit + wgpu + cpal frontend (minimum viable player)](archive/phase-5-frontend-tooling/sprint-1-frontend-mvp.md)
- [Sprint 2 — Save state + rewind + input bindings](archive/phase-5-frontend-tooling/sprint-2-save-rewind.md)
- [Sprint 3 — Debugger overlays (egui) + NTSC filter + release pipeline](archive/phase-5-frontend-tooling/sprint-3-debugger-release.md)

---

---

### Phase 6 — v1.0.0 Closeout (SUPERSEDED — accuracy closed by the engine-lineage master-clock work)

> **Superseded.** The engine-lineage continued past this closeout plan: the
> master-clock refactor took AccuracyCoin to **100.00% (139/139)** and the C1
> IRQ-timing + sub-cycle residuals these sprints chased were closed (or
> documented-deferred) along the way. The sprint backlog below was **not**
> executed as written; it is retained as the historical gate plan. RustyNES
> ships at **v1.0.0** with the accuracy bar fully cleared.

**Original goal (historical):** close all open v1.0.0 gates and ship the v1.0.0
tag.

**Original exit criterion (historical):** `cargo test --features test-roms`
shows the C1 `cpu_interrupts_v2/{2,3,5}` + `mmc3_test_2/4-scanline_timing`
sub-test #3 flipped + AccuracyCoin ≥ 90% + multi-OS release-artifact smoke test
green + the 6 `#[ignore]`'d commercial ROMs investigated. (All resolved by the
engine-lineage work; AccuracyCoin is now 100%.)

[Phase 6 overview](archive/phase-6-v1-closeout/overview.md)
[Phase 6 v1.0.0-final sprint backlog](archive/phase-6-v1.0.0-final/overview.md)
— ordered six-sprint plan to close the AccuracyCoin 90% gate + the 4
C1 IRQ-timing residuals (Sprint 1: Implied-Dummy + DMC coordinated;
Sprint 2: APU put/get phase; Sprint 3: sprite-eval residuals;
Sprint 4: PPU misc residuals; Sprint 5: C1 axis attempt 17;
Sprint 6: SH* unstable stores).

Tickets (informal — formal sprint files when work begins). The `[~]` markers
below are **historical**: they record each ticket's state *at this superseded
phase*, not now — all were closed or documented-deferred by the engine-lineage
master-clock work (current AccuracyCoin **100.00%**). They are not live TODOs.

- [~] **T-60-001 — Coordinated CPU/Bus/PPU IRQ-sample-timing rework
  (Track C1). DEFERRED to v1.x.** 11 independent fix attempts rolled
  back across multiple sessions; no empirical breakthrough on the
  canonical CPU `T_last - 1` IRQ-sample-point axis. Residuals:
  `cpu_interrupts_v2/{2-nmi_and_brk, 3-nmi_and_irq, 5-branch_delays_irq}`
  - `mmc3_test_2/4-scanline_timing` sub-test #3. Infrastructure landed
  (ADR-0002 Decision section + per-CPU-cycle IRQ tracing fixture + 6
  golden baseline traces + M2-phase plumbing + Phase B4 reload-pending
  discriminator). Does not affect any real game; commercial game
  compatibility intact. Carries forward to v1.x roadmap.
- [~] **T-60-002 — Push AccuracyCoin pass rate from 69.78% to ≥ 90%.
  IN PROGRESS at 82.73%** (Cascade B closed 2026-05-19 in commit
  `9b0c81c` + Cascade A partial closure 2026-05-19 via OAMADDR reset
  during dots 257-320 in `f29f7ca` + session-6 `$2004` dots 1-64 `$FF`
  in `6c2664e` + session-7 OAMADDR-walks-during-eval + $4-aligned
  `$2004` write in `c230489` + session-7 RMW ABS,X/Y unfixed-address
  dummy read in `32d5b18` + **session-8 BG-pipeline cycle-9 reload +
  post-emit shift in `086ce4d` (architectural closure of Cascade A's
  `VerifySpriteZeroHits` step-2 geometric puzzle per
  `docs/audit/cascade-a-investigation-2026-05-19.md`)**; trajectory
  `64.03% → 67.63% → 69.06% → 69.78% → 76.98% → 78.42% → 79.14% →
  79.86% → 82.73%`, exceeds CI floor of 0.60 by 22.7pp and
  **CLEARED the v0.9.x 80% target by 2.7pp**). **Cascade B
  (DMC DMA halt-cycle precision) CLOSED** — all 8 tests in "APU
  Registers and DMA tests" flipped + 3 net side-benefit flips
  elsewhere; +11 tests. **Cascade A (Sprite Zero Hit BG-pipeline
  geometry) PARTIALLY CLOSED** — the load-bearing architectural
  axis (BG shift-register cycle-9 reload + post-emit shift per
  Mesen2 + nesdev wiki) landed in session 8, flipping 4 tests
  (Sprite 0 Hit behavior, Sprite overflow behavior, Suddenly
  Resize Sprite, $2007 read w/ rendering). The remaining 24
  failing tests cluster as documented in
  `docs/audit/accuracycoin-readme-analysis-2026-05-17.md`'s
  2026-05-19 addendum +`docs/audit/cascade-a-investigation-2026-05-19.md`'s
  RESOLUTION section:
  - **Cascade A residuals — 10 tests (post-BG-pipeline-fix):** 4
    sprite-eval ($2002 flag timing, Arbitrary Sprite zero, Misaligned
    OAM behavior, OAM Corruption) + 6 PPU misc (Stale BG/Sprite
    Shift Regs, BG Serial In, Sprites On Scanline 0, $2004/$2007
    Stress Tests). Cluster gated on stale-shift-register modeling +
    post-B8 sprite-FSM interactions + $2002 sub-cycle flag timing.
    The session-8 BG-pipeline fix closed the geometric root cause
    (`VerifySpriteZeroHits` step-2) but left these subtler
    cycle-precision residuals for future sessions.
  - **C1 IRQ-timing axis — 5 tests (4 × `cpu_interrupts_v2/{2..5}` +
    `mmc3_test_2/4` sub-test #3) — DEFERRED, see T-60-001.**
  - **Internal-bus model — ~5 tests** (`CPU Behavior :: Open Bus
    [error 9]`, 5 × SH*opcodes `[error 7]`, `CPU Behavior 2 ::
    Implied Dummy Reads [error 2]`). Requires internal-vs-external
    bus model rework that previously regressed Internal Data Bus
    Test 2. The SH* tests are "Coupled to Cascade B" per audit but
    they did NOT flip when Cascade B landed — confirming SH*
    address corruption needs an explicit RDY-low-2-cycles rule
    rather than just DMC DMA halt modeling.
  - **APU residuals — 5 tests** (Frame Counter IRQ, DMC Channel,
    APU Register Activation, Controller Strobing/Clocking). Each
    is a distinct $4015 RMW / put-vs-get-cycle bracket; bundled
    with the internal-bus-model rework above.
  - **PPU residuals — 2 tests** (Rendering Flag Behavior,
    `$2007` read w/ rendering). Distinct from Cascade A.

  **Realistic v1.0.0 trajectory**: if the remaining Cascade A
  geometric residual (VerifySpriteZeroHits step-2; characterisation
  reproducer at `crates/rustynes-ppu/src/ppu.rs` landed in `b629ace`) closes
  without regressing baselines, pass rate would advance
  `79.86% → ~88%`. The v1.0.0 90% gate remains contingent on Cascade A
  full closure + C1 IRQ-timing axis. T-60-002 carries forward to v1.x
  roadmap with the 79.86% baseline.
- [x] **T-60-003a — long-intro budget extensions (CLOSED, 2026-05-17)**:
  Mr. Gimmick + Tiny Toon Adventures 2 flipped from `#[ignore]`'d to
  passing via the `LONG_INTRO_START_3600` input script (idle 3600 →
  START tap → free-run 240, captures at f3661 / f3901). Commit `7fa2c90`.
  Ignored count: `6 → 4`.
- [x] **T-60-003b/c — CLOSED (2026-05-17)**: all 4 remaining stuck
  ROMs flipped via 2 architectural mapper fixes. Root cause: VRC2 /
  VRC4 / VRC6 / MMC4 mapper impls were missing the `$6000-$7FFF`
  WRAM read/write paths. Reads returned 0; writes silently dropped.
  Konami's save-bearing titles stalled in save-validation. Fixes:
  - commit `895e426`: VRC2/VRC4/VRC6 8 KiB `prg_ram` field added +
    read/write paths in `crates/rustynes-mappers/src/m022_vrc2.rs` +
    `m021_vrc4.rs` +
    `m024_vrc6.rs`. Flipped Esper Dream 2, Mouryou Senki Madara,
    Ganbare Goemon 2.
  - commit `42f31ff`: MMC4 same pattern in
    `crates/rustynes-mappers/src/m010_mmc4.rs`. Flipped Fire Emblem Gaiden.

  **T-60-003 is now FULLY CLOSED — all 6 originally-stuck commercial
  ROMs strict-passing. Commercial-roms count: 60 strict + 0 ignored.**
- [ ] T-60-004 — Multi-OS release-artifact smoke test (T-51-009 carried
  forward from Phase 5 Sprint 1). The `v1.0.0-rc1` tag triggers the
  GitHub Actions release workflow which produces Linux/macOS/Windows
  artifacts. User to smoke-test each on a representative ROM (e.g.,
  nestest.nes) before promoting to `v1.0.0`. PENDING USER VERIFICATION.
- [~] **T-60-005 — `v1.0.0` tag + release notes. SUPERSEDED by
  `v1.0.0-rc2`** (2026-05-22). The rc2 tag captures the
  post-Mesen2-alignment release-candidate state with the four C1
  IRQ-timing residuals + the ~20 non-C1 AccuracyCoin residuals
  explicitly carried forward into the
  `to-dos/phase-6-v1.0.0-final/` sprint backlog. The final `v1.0.0`
  tag is gated on AccuracyCoin ≥ 90% + T-60-001 closure (4 C1
  residuals flipped). Sprint 1 of the v1.0.0-final backlog targets
  the Implied-Dummy + DMC DMA coordinated fix that Session-19 surfaced
  as the highest-leverage entry point. Prior rc1 tag remains as the
  pre-Mesen2-alignment baseline.

---

### Phase 7 — Nesdev Accuracy Hardening (COMPLETE — v1.5.0, 2026-05-24)

**Outcome:** all 4 sprints landed; +25 strict tests, AccuracyCoin held at
90.65% (additive only; the master-clock-axis residuals are explicitly deferred
to v2.0). See `docs/audit/phase-7-assessment-2026-05-24.md` + the per-sprint
audit docs (`docs/audit/phase-7-sprint-{2,3,4}-*.md`).

**Goal:** close the hardware-accuracy and documentation gaps identified by
`ref-docs/nesdev-wiki-technical-report.md` and
`docs/nesdev-hardware-emulation-checklist.md`.

**Exit criterion:** all stock NES/Famicom behaviors in the Nesdev-derived
checklist are implemented, explicitly out of scope, or guarded by tests; missing
Nesdev-indexed test categories are vendored or replaced with licensed fixtures;
PAL/Dendy and remaining AccuracyCoin residuals have automated coverage; platform
expansion scope is documented.

[Phase 7 overview](archive/phase-7-nesdev-accuracy-hardening/overview.md)

Sprints:

- [Sprint 1 — Source and test corpus closure](archive/phase-7-nesdev-accuracy-hardening/sprint-1-source-test-corpus.md)
- [Sprint 2 — CPU, DMA, and internal bus closure](archive/phase-7-nesdev-accuracy-hardening/sprint-2-cpu-dma-internal-bus.md)
- [Sprint 3 — PPU residuals and region variants](archive/phase-7-nesdev-accuracy-hardening/sprint-3-ppu-region-variants.md)
- [Sprint 4 — Mapper, expansion audio, and platform variants](archive/phase-7-nesdev-accuracy-hardening/sprint-4-mappers-expansion-platforms.md)

### Phase 8 — v1.2.0 DMC DMA Scheduler (COMPLETE; broader accuracy residuals deferred)

**Scope reconciliation:** the original v2.0.0 plan framed v1.2.0 as a broad
"accuracy residuals" milestone (sprite-eval + PPU-misc + APU edge cases +
6 ignored commercial ROMs → AccuracyCoin ~97%). What **actually shipped** as
v1.2.0 was a narrower, focused slice: the **DMC DMA get/put scheduler**
landed behind a default-off cargo feature via the parallel-implementation
pattern (ADR 0007). The broader accuracy residuals were **not** done and are
**deferred to v1.6 / v2.0** (several fall out of the v2.0 master-clock
refactor for free); AccuracyCoin remains **90.65%**, not the 97% the original
plan targeted for v1.2.0.

**Exit criterion (MET, as shipped):** v1.2.0 tag landed with
`dmc-get-put-scheduler` parallel-implementation in place (default-off),
equivalence harness shipped, AccuracyCoin DMA cluster matching v1.1.0
baseline at 6/10 under the flag (the remaining 4 — `DMA + $4015 Read`,
`DMC DMA + OAM DMA`, `Explicit/Implicit DMA Abort` — deferred to v2.0
absorption; ADR 0007 option c). Default build bit-identical to v1.1.0; no
regression to the 60-ROM oracle, sacred trio, or B4 invariant.

[Phase 8 overview](archive/phase-8-v1.2.0-accuracy-residuals/overview.md)

Sprints:

- [Sprint 3 — DMC get/put scheduler parallel implementation](archive/phase-8-v1.2.0-accuracy-residuals/sprint-3-dmc-get-put-scheduler.md)
  — Sprint 3.1-3.5 + iter 3 (DMC abort path port) all LANDED. ADR 0007 written.
  v1.2.0 tag landed 2026-05-24.

> **Deferred to v1.6 / v2.0** (tracked here so it isn't lost): (a) DMC get/put
> completion 6/10 → 10/10 + default-on promotion (ADR 0007); (b) the broader
> AccuracyCoin residuals — sprite-eval ($2002 flag timing, Arbitrary Sprite
> zero, Misaligned OAM, OAM Corruption), PPU-misc (Stale BG/Sprite shift regs,
> BG Serial In, Sprites On Scanline 0, $2004/$2007 Stress), APU edge cases
> (Frame Counter IRQ #7, DMC, Reg Activation, Controller Strobing), and the
> 6 ignored commercial ROMs (mapper-026 VRC6b pair shares one bug). Many are
> on the C1 IRQ-sample-point axis and close with the v2.0 master-clock refactor.
> See `docs/STATUS.md` version policy for the full residual list.

### Phase 9 — v1.3.0 WebAssembly Target + v1.3.1/.2/.3 patches (COMPLETE)

**Goal:** Ship a `wasm32-unknown-unknown` build of the frontend that runs in
the browser, per the v2.0.0 release plan. No API break (the chip stack is
already `no_std + alloc`).

**Exit criterion (MET):** v1.3.0 tag landed; the frontend builds for wasm32
in two flavours (`wasm-winit` default + `wasm-canvas` embed); a GitHub Pages
demo is live at `https://doublegate.github.io/RustyNES/`; CI gates a
wasm32 clippy build + a 5 MiB compressed size budget. Workspace tests
preserved (599+6 ignored); AccuracyCoin 90.65%, commercial oracle 60/60,
sacred trio + B4 invariant — all preserved bit-identically.

Sprints (all LANDED): 1.1 scaffolding → 1.2 entry point + browser host →
1.3 canvas-2D MVP → 1.4a audio + save state → 1.4b winit/wgpu/egui
unification → 1.4c audio on the unified path → 2 GitHub Pages deploy + CI
wasm32 gate + size budget. See `docs/audit/v1.3-sprint-*.md`.

**Follow-on patches (COMPLETE):** v1.3.1 (left-edge BG attribute-shifter
palette fix + native present-mode/sleep-spin stutter fix + legacy
`config.toml` migration), v1.3.2 (legacy keycode-name aliases fixing
post-migration dead input + first wasm rAF pacing attempt), v1.3.3 (wasm
`ControlFlow::Wait` + unconditional rAF heartbeat fixing the Pages
stutter/freeze regression + WebGL2 UNORM palette fix + native chunked-sleep
pacing). All frontend-only; native pixel-identical; 616 strict + 6 ignored;
AccuracyCoin 90.65% preserved. See `docs/audit/v1.3.x-*.md`.

### Phase 10 — v1.4.0 TAS Movie Recording/Playback (COMPLETE)

**Goal:** Frame-perfect input recording + playback with save-state branching,
per the v2.0.0 release plan. Exposes the already-met determinism contract
(same seed + ROM + input ⇒ bit-identical framebuffer + audio). No API break.

**Exit criterion (MET):** byte-identical record → replay (framebuffer +
audio FNV-1a + cycle count) proven by integration tests on a committed CC0
ROM; save-state-branch replays deterministically; record/play/branch UI
wired (`F6`/`F7`/`F8`); the `.rnm` movie format is versioned for
forward-compat (ADR 0008, layered on ADR 0003). 636 strict + 8 ignored;
oracle 60/60; AccuracyCoin 90.65%; B4 + sacred trio preserved.

Sprints (LANDED): **4.1** — core movie infra in `crates/rustynes-core/src/movie.rs`
(`MovieRecorder`/`MoviePlayer`, `.rnm` serialize/deserialize, the additive
read-only `Nes::buttons` hook; `run_frame` untouched), ADR 0008, +13 tests.
**4.2** — frontend `crates/rustynes-frontend/src/movie_ui.rs` (record/play/branch
hotkeys, `MovieUi` state machine in the frame loop, native `rfd` `.rnm`
save/load, read-only egui REC/PLAY overlay), +7 tests. Clean-room from
Mesen2 `Core/Shared/Movies/` (structural, GPL-3.0) + FCEUX `.fm2` + the
local TetaNES clone (upstream TetaNES) + nesdev TAS. wasm `.rnm` file I/O
deferred to a v1.4.x follow-up (UI compiles + no-ops on wasm). See
`docs/adr/0008-tas-movie-format.md`.

### Release engineering (v1.x)

- [→] **CI: `macos-15-intel` runner sunset — August 2027.** GitHub will
  decommission the `macos-15-intel` label after that date (per
  `actions/runner-images#13045`). Plan: migrate to `cargo-zigbuild`
  cross-compile from Linux, or drop `x86_64-apple-darwin` from the
  release binary matrix. Non-blocking forward reminder. The Session-22
  `macos-13` → `macos-15-intel` migration (commit `a9333ba`,
  `.github/workflows/release.yml` +
  `docs/audit/ci-release-workflow-macos-x86_64-2026-05-22.md`) resolved
  the prior deprecation; this entry tracks the next deadline.

---

## Cross-phase dependencies

- Phase 2 Sprint 4 depends on Phase 1 complete (CPU core).
- Phase 3 depends on Phase 1 (CPU) and Phase 2 Sprint 4 (scheduler) complete; Sprint 2 of Phase 3 depends on Phase 2 Sprint 4 (DMA).
- Phase 4 depends on Phase 2 (PPU) for mapper-PPU integration; Phase 3 (APU) for audio-extension mappers.
- Phase 5 depends on all previous phases.
- Phase 7 depends on the Phase 6 closeout decision for which v1.0 residuals
  carry forward. It also depends on the Nesdev checklist staying current with
  upstream source pages and local `docs/STATUS.md` pass counts.

## Deferred — MiSTer framework surface this core does not use (opened v2.6.12)

The v2.6.12 audit of `rtl/emu.sv`'s `hps_io` tie-offs asked, of each one,
whether it was a feature a NES core should offer. Nine were. They are filed
here with IDs so the tie-offs read as decisions rather than omissions.

**The result of annotating them is itself the finding: every one is blocked,
and not one of them is blocked on effort.** Four are deferred to v2.8+ by the
approved plan; three cannot be *verified* by anything in this repository because
every route to them runs through `hps_io`, which needs the HPS; and two wait on
a subsystem that does not exist yet.

**Amended at v2.6.13, and the amendment is a retraction.** "Blocked" was too
strong for two of them, and the research that showed it is
`ref-docs/2026-09-02-mister-sdram-hardware-reference.md`:

- **`T-MISTER-SAVE`'s blocker was wrong.** Its implementation is unblocked; only
  its verification is rung 6, which is what the ticket said before v2.6.12
  "corrected" it. Scheduled for v2.6.13.
- **`T-MISTER-SDRAM-SZ` is scheduled too**, behind the SDRAM controller that
  v2.6.13 builds -- and its semantics turn out to be a validity bit rather than
  a size, which is a different and better-defined ticket than it looked.

The rest stand. So the list is still largely the rung-6 agenda, but "close these
tickets" no longer resolves *entirely* to "attach a board".

A caution recorded with them: these were originally filed under the heading
below, which says they are cases where the DUT is *more accurate than the
oracle*. None of them is. That was a filing error in the same session that
minted them, and it is corrected rather than quietly moved, because a ticket
under a wrong heading is read by its heading.

### T-MISTER-SAVE — battery-backed save RAM is not persisted

**Owner-facing summary.** `rtl/cart/cart.sv` has 8 KiB of PRG-RAM and the MiSTer
core does nothing to persist it. Every MMC1 or MMC3 game with a battery —
*Zelda*, *Final Fantasy*, *Kirby's Adventure*, *Crystalis* — writes saves there
and **loses them at power-off**. This is the only item in the group with a
user-visible cost, and it is the highest-value one.

**Mechanism.** MiSTer offers two routes, both via `hps_io`
([framework docs](https://mister-devel.github.io/MkDocs_MiSTer/developer/hps_io/)):
the `sd_*` block interface with `img_mounted` / `img_size`, or the simpler
`ioctl_upload_req` / `ioctl_din` NVRAM path. Both ports are tied off in
`rtl/emu.sv` today, with a comment naming this ticket.

**Acceptance.** A battery game's SRAM survives a power cycle on hardware.

**MEASURED at v2.6.15 — can a gate reach `hps_io` at all?** v2.6.12 attempted
this ticket and refuted its own attempt on the grounds that "every save route
terminates in `hps_io`, which no gate here instantiates". That is a claim about
whether it CAN be instantiated, and it had not been tested. It now has:

```console
$ verilator --lint-only -Isys --top-module hps_io -GCONF_STR=1 sys/hps_io.sv
%Error-PROCASSWIRE: sys/hps_io.sv:299:14: Procedural assignment to wire ...
   ... 11 errors, chiefly PROCASSWIRE at one line, plus 71 warnings
```

So it is **close, and not free**. Without a `CONF_STR` value it stops at one
error — that parameter has no default — and with one supplied it reaches
elaboration and produces eleven, nearly all `PROCASSWIRE` from a single line
assigning four `wire`s procedurally. Verilator treats that as an error where
Quartus does not.

That makes the choice a real one rather than a guess, which is the point of
measuring it:

- **Instantiate the real `hps_io`** — needs `-Wno-PROCASSWIRE` and a handful of
  siblings. Suppressing errors in vendored framework code to build a testbench
  is a decision, not a flag: it weakens the lint gate for every file compiled in
  that invocation, and `sys/` is the one tree this project may not fix at
  source.
- **Model the protocol instead** — a stimulus model of `sd_*` and
  `ioctl_upload_req`/`ioctl_din` written from the framework's port
  documentation. Narrower, no suppression, and it tests the CORE's side of the
  contract, which is the side that can be wrong here. Against it: a model of an
  interface can agree with the core about a protocol they both misread, which is
  the "agreement about an unasked question" failure this project has hit before.

**Recommended: the model, with the real module as a later cross-check** — the
same shape as `tb/sdram_model.sv`, which is a behavioural part written from a
datasheet and states in `docs/sdram.md` exactly what it cannot see. One enabler
serves three tickets: this one, `T-MISTER-SAVESTATE`, and `T-MISTER-CHEATS`
through `ioctl` index routing.

**RETRACTED at v2.6.13 — the blocker below was WRONG, and the ticket's original
text was right.** v2.6.12 recorded this as blocked because "there is no
OSD-close signal to flush on". The MiSTer developer documentation states the
contract in one line and it needs no such signal:

> `ioctl_upload_req` -- set to 1 to ask the HPS to initiate an NVRAM save, for
> autosave, **HPS only reads this when the OSD is open**

The core raises the request and the HPS collects it while the OSD is open. The
same page puts this project's case explicitly on that path -- *"Use ioctl upload
for: smaller NVRAM/save files"* -- with the `sd_*` block interface reserved for
virtual hard drives; the declaration is the `F` option's `S` modifier
(`F[S][#],{Ext}...`, "core supports save files"). Both routes are present in our
vendored `hps_io.sv`.

**So the implementation is unblocked and only the verification is rung 6** --
which is what this ticket said before v2.6.12 "corrected" it. The error is
recorded rather than erased because it shipped in a release: reasoning from the
absence of a signal in the framework SOURCE to "the mechanism cannot work"
skipped reading the PROTOCOL that uses it, and one page of vendor documentation
settled what a session of source-reading had concluded backwards. Scheduled for
**v2.6.13**. Full account: `ref-docs/2026-09-02-mister-sdram-hardware-reference.md`.

The superseded reasoning, kept so the retraction has a subject:

- ~~**There is no OSD-close signal to flush on.** This `sys/` vintage exposes no
  `OSD_STATUS` output, and MiSTer's convention is to write the save when the
  user leaves the OSD. The nearest available signal, `buttons`, is already the
  reset button at `rtl/emu.sv`'s reset expression -- so the obvious trigger is
  both wrong and, being a plausible-looking wrong, exactly the kind that
  ships.~~ **Refuted: no core-side trigger is required at all.**
- ~~**The NVRAM route is not self-contained.**~~ `hps_io.sv:152` does say
  `ioctl_upload_req // request to save (must be supported on HPS side for
  specific core)`, and that quote is accurate -- but the support it refers to
  **is** the standard autosave mechanism above, reached by declaring the save
  file with the `F` option's `S` modifier. Read as a blocker, it was read
  wrongly.
- **Neither route is reachable by any gate here.** `sd_*` and `ioctl_*` both
  terminate in `hps_io`, which the co-simulation testbench does not instantiate
  and cannot drive. **This one still stands**, and it is the whole of what
  remains: the *verification* is rung 6.

What that leaves is an ordinary rung-6-pending feature rather than a blocked
one, and this project ships those -- the entire bitstream is rung-6-pending. The
half that CAN be gated here is the cartridge's own save port: request raised,
`ioctl_din` presenting the byte at `ioctl_addr`, the save-index download
restoring it, and PRG-RAM round-tripping byte-identically. The `hps_io` half is
disclosed as unverified, the same way every other downstream-of-the-gates
property in this programme is.

**Scheduled for v2.6.13** (`to-dos/plans/v2.6.13-plan.md`, item E). Hardware
acceptance -- a save surviving a power cycle -- still needs a board.

### T-MISTER-4PLAYER — Four Score / four controllers

`joystick_2` and `joystick_3` are unconnected. A real NES accessory
(*Bomberman II*, *Gauntlet II*, *Super Off Road*). Needs the Four Score's serial
protocol in `controller.sv`, which currently models two ports.

**BLOCKED — deferred by the approved plan**, which names "Four Score" in its
*explicitly out of scope until v2.8+* list. The oracle models it
(`bus_snapshot.rs` carries `four_score`, its per-port index and signature), so
a gate is buildable -- but it also needs an exporter stimulus flag and a ROM
that reads four ports, neither of which exists. **Unblocks at v2.8+.**

### T-MISTER-PADDLE — the Arkanoid Vaus controller

`paddle_0` / `spinner_0`. A real NES peripheral — and *Arkanoid* is one of the
six titles in the v2.6.11 montage, so the core renders a game it cannot yet be
played with properly.

**BLOCKED — same class as T-MISTER-4PLAYER**: a peripheral input device, and
the plan defers that class to v2.8+. **Unblocks at v2.8+.**

### T-MISTER-ZAPPER — the light gun

The ORACLE already implements it (`Nes::set_zapper`, with the beam-relative
light model added at v2.2.3), so this is a wiring question on the DUT side
rather than a modelling one.

**BLOCKED — deferred by the approved plan**, which names "Zapper" in its
*explicitly out of scope until v2.8+* list. Note that "the oracle already models
it" makes this *cheaper*, not *unblocked* -- a gate still needs a stimulus the
exporter cannot currently produce. **Unblocks at v2.8+.**

### T-MISTER-SAVESTATE — the framework has save states and this core has none

**Opened v2.6.13 by the framework audit.** MiSTer defines a save-state contract:
a core declares `SS{base addr}:{savestate size}` in its CONF_STR, the framework
reserves four slots in **DDR3**, and each slot opens with a 64-bit control word
whose low half is a change detector and whose high half is the size in 32-bit
words. The documentation notes saves "are performed rather quickly after the
write occurs, and **do not require opening of the OSD**".

**This is the largest capability gap between the oracle and the DUT.** RustyNES
the emulator has had save states since v1.0 -- `Nes::snapshot`, the `.rns`
format, a versioned per-chip schema and a standing schema audit -- and the
MiSTer core has nothing. The oracle even knows how to serialise every chip, so
the *content* of a save state is a solved problem here; only the transport is not.

**BLOCKED on DDR3, which this core declines.** All ten `DDRAM_*` ports are tied
off and three sit in `unused_misc`. That is the dependency, and it is also the
scheduling constraint: **v2.6.13 brings up SDRAM, and bringing up DDR3 in the
same release would make any timing regression impossible to attribute.**

**Unblocks after v2.6.13.** Wants its own release.

### T-MISTER-CHEATS — no cheat menu

**Opened v2.6.13.** The CONF_STR offers `C[,{Text}]`, "enables a cheat menu
entry". RustyNES ships a full Game Genie implementation plus a header-robust
code database; the DUT has neither.

Worth noting for whoever takes it: because the oracle already decodes Game Genie
codes, this is a good candidate for an **oracle-gated** rung -- apply a code on
both sides and compare the bus, exactly as every other rung works -- rather than
a feature verified by playing a game.

**Unblocks:** any time. Not blocked on hardware.

### T-MISTER-OSD — five status bits of a hundred and twenty-eight

**Opened v2.6.13.** This core uses `status[0]`, `status[2:1]` and `status[4:3]`
and nothing else, so **123 bits of user-facing options are unoffered**: region,
overscan, palette selection, per-mapper toggles, audio mixing. Alongside that,
`sys/video_freak.sv` is not instantiated (which is why only two of the four
aspect-ratio entries exist -- our own CONF_STR comment says so), `video_freezer`
is not either, and the OSD is flat because `P` sub-pages are unused.

Filed as ONE ticket rather than five because they are one piece of work. An
options surface added a bit at a time produces a menu nobody designed, and the
`P` pages only matter once there is enough to page.

**Unblocks:** any time. `AUDIO_MIX`, `LED_DISK` and `jn` are split out of it and
land in v2.6.13, being one line each.

### T-MISTER-SDRAM-SZ — report SDRAM presence and size

`sdram_sz[1:0]` reports none / 32 / 64 / 128 MB. **Needed the moment the SDRAM
controller lands**, which is the next major item: a core that assumes the
DE10-Nano's add-on is present on a board without it should say so rather than
misbehave.

**LANDED v2.6.13** (`rtl/sdram_presence.sv`). The controller exists, so the
declaration has something behind it -- and the ticket turned out to be about
**bit 15**, not about `[1:0]`. The word powers up as `0x0000`, so "the HPS has
not answered yet" and "there is no board" are the same two bits; a core that
reads only the size announces an absent add-on on a machine that has one, every
time, until the HPS replies. `absent` is therefore deliberately not `!present`:
both are false while the answer is unknown, and the arbiter's `ready` is gated
on `usable`, so no access is granted against memory nothing has confirmed
exists. The gate is exhaustive over all 65,536 values against an independent
model, and six mutations are caught, including the ticket's own defect.

### T-MISTER-DIRECTVIDEO — analog I/O board output

`direct_video` is asserted when HDMI is wired as VGA. Users running MiSTer on a
CRT through the analog board are a large fraction of the audience.

**BLOCKED — rung 6.** The RTL is one assignment; the acceptance is a picture on
a CRT through the analog I/O board, which no gate here can produce. Landing the
assignment without it would repeat v2.6.6's `VGA_SL` defect, where two OSD
options did nothing because the signal they fed was tied to zero and nothing
looked. **Unblocks on hardware plus an analog I/O board.**

### T-MISTER-KEYBOARD / T-MISTER-MENUMASK / T-MISTER-VMODE

The Famicom Family BASIC keyboard (`ps2_key`, niche); conditional OSD entries
(`status_menumask`, polish); and a runtime NTSC/PAL switch (`new_vmode`, which
needs PAL timing to exist first).

**MENUMASK LANDED v2.6.13** (`rtl/osd_menumask.sv`): the Reset entry is greyed
out whenever the console is already held in reset -- no cartridge, or a mapper
this core cannot decode -- because a menu entry that does nothing when selected
reads as a working feature. Gated on the computed VALUE against the `CONF_STR`
`D0` polarity, which needs no display; only the appearance still needs eyes.

**The other two stay blocked.** KEYBOARD is a peripheral input device,
the class the plan defers to v2.8+. VMODE needs PAL timing, which this core does
not implement at all; a switch between one mode and a mode that does not exist
is not a partial feature.

**MENUMASK is no longer rung 6, and the reason it was filed there is worth
keeping.** It was recorded as "OSD behaviour, verifiable only by looking at an
OSD", which conflated two different things. `status_menumask` is a **value this
core computes** -- `mapper_ok` already exists and is exactly the condition a mask
would encode -- and a gate can assert the computed value with no display
anywhere. Only whether the greyed-out entry LOOKS right needs eyes, and that is
a much smaller claim than "the feature is unverifiable". **Scheduled for
v2.6.13** (plan item F). The general lesson: "verified by looking at it" is
often true of the *presentation* and false of the *computation*, and filing the
whole ticket under the former is how something cheap gets deferred for releases.

**None of these landed in v2.6.12**, and after annotation that is a measurement
rather than a scoping choice: 4 are deferred to v2.8+ by the approved plan, 3
are unverifiable without a board, and 2 wait on unwritten subsystems.

**Amended at v2.6.13.** The framework audit added three more tickets
(`T-MISTER-SAVESTATE`, `T-MISTER-CHEATS`, `T-MISTER-OSD`) and moved three of the
original nine off the blocked list: **SAVE** (blocker refuted), **SDRAM-SZ**
(behind the controller v2.6.13 builds) and **MENUMASK** (promoted -- the mask is
a computed value, not a picture). So of twelve tickets, **three are scheduled for
v2.6.13, two are unblocked and unscheduled, and seven remain blocked** on the
approved plan's v2.8+ line, on hardware, or on DDR3. They are named
with IDs so the tie-offs in `emu.sv` read as decisions rather than omissions.
The full table, including what is genuinely *not applicable* to a NES, is in
`RustyNES_MiSTer/docs/rung6-integration.md`.

## Deferred — oracle accuracy items the co-simulation found (opened v2.6.9)

The MiSTer co-simulation is a two-way instrument. These are cases where the
**DUT is measurably more accurate than RustyNES itself**, which ADR 0037
anticipated in writing ("the oracle can be wrong") and which the ladder is
supposed to surface rather than absorb.

### T-ORACLE-001 — MMC3 IRQ timing (mechanism RETRACTED v2.6.15)

**Owner-facing summary, rewritten v2.6.15.** RustyNES fails
`mmc3_test_2/4-scanline_timing` at sub-test **3**; the MiSTer co-simulation DUT
fails at sub-test **12**. On this ROM the DUT is the more accurate of the two,
and that part stands.

**The mechanism is NOT known, and the fix is NOT known.** This summary used to
say both were. The pre-render A12 claim below is refuted — RustyNES clocks the
counter 241 times per frame, which `mmc3_test_2/2-details` sub-test 8 asserts
and it passes — so the diagnosis that named a missing clock is gone, and with it
the fix built on it. What survives is a bounded measurement: sub-tests 2 and 3
bracket the IRQ to **one PPU dot**, RustyNES passes "should occur later" and
fails "should occur sooner", so its IRQ is late by **at least a dot** and the
ROM says nothing about how much more.

Note the direction, because it rules out the second half of the proposed fix
too: **registering `/IRQ` makes the assertion LATER**, and this residual is
already late. Whatever closes it must move the assertion earlier, which the
recorded fix does not.

It remains a CORE change if it is ever attempted, so it still needs its own
version — but v2.6.15 did **not** attempt it, and nothing here should be read as
a plan that is ready to execute.

#### RETRACTED, v2.6.15: claim 1 is false, and claim 2 is not what it looks like

**RustyNES clocks 241 times per PPU frame**, which is 240 visible lines plus the
pre-render line. `mmc3_test_2/2-details` sub-test 8 is, in the ROM's own words,
*"Counter should be clocked 241 times in PPU frame"* — it loads the counter with
241, renders one frame, asserts the IRQ is still clear, clocks once by hand and
asserts it is set, so 240 clocks fails it and 242 fails it. **RustyNES passes
it**, and has for every release this suite has run.

The claim came from `--ppu-state-trace`, and this ticket's own *instrument traps*
section below says why that instrument cannot answer the question: it **carries
no CHR address column**, so it reports what the sprite state was and never what
address was driven. A trace that cannot see an A12 rise was read as evidence
that no rise occurred. `ppu.rs`'s sprite-fetch dispatch has stated the opposite
intent in a comment since v2.0 — the dummy fetch *"must run on both visible
scanlines and the pre-render line"* and contributes *"the 241st A12 rising edge
per frame"* — and the v2.2.3 fast dot path cannot bypass it, being gated on
`cached_visible` and `dot <= 256`.

Claim 2's measurement (1,250,873 against 1,250,760) stands as a measurement. Its
*label* does not: 113 CPU cycles is about a scanline, but "about a scanline late"
was inferred from the missing pre-render clock, and there is no missing clock.
What the ROM pins is narrower — `4-scanline_timing` #2 and #3 bracket the IRQ to
**one PPU dot** at 6976 dots after the VBL flag is set, and RustyNES passes #2
("should occur later") while failing #3 ("should occur sooner"), so its IRQ is
late by at least one dot and the ROM says nothing about how much more.

**Two of the four residuals this ticket proposed to fix are not IRQ-timing
residuals at all** — `mmc3_test_v1/5` #2 and `/6` #2 rest on an assertion the
author withdrew in the successor ROM. Measured, reverted, and written up in
ADR 0002's v2.6.15 decision update, which also narrows the R1/R2 residual set
from four sub-tests to two.

The text below is preserved as issued rather than edited, because it was cited
in the v2.6.15 plan and in this file, and a retracted claim that leaves no trace
is how the retraction gets re-derived.

#### The claim, and how each part was measured

1. **RETRACTED — see above. RustyNES never clocks the MMC3 counter on the
   pre-render line.** Its own
   PPU state trace for `4-scanline_timing` shows scanline 261 with `mask=24`
   (rendering on), `ctrl=8` (sprites at `$1000`), `spr_count=0`, and no clock —
   while scanline 0, with *identical* sprite state, clocks normally. The
   omission is keyed to the pre-render line, not to the sprite count.
2. **So its `/IRQ` is a scanline late**: cycle **1,250,873** against the DUT's
   **1,250,760**, on CPU streams that are identical up to that point.
3. **The clock is required.** The NESdev MMC3 page: filtered A12 "oscillates
   exactly one time per scanline and **241 times per frame**", and sprite
   patterns are fetched "even if no sprites are visible". 241 = 240 visible +
   pre-render. Suppressing the clock in the DUT fixed `4-scanline_timing`
   sub-test 2 and **broke** `2-details` sub-test 8, the 241-clock test — so any
   fix must keep the clock.
4. **The remaining error is one CPU cycle, not one scanline.** blargg's `cli`
   for sub-test 2 is fetched at cycle 1,250,755, so `end_` runs cli(755-756)
   nop(757-758) nop(759-760) `inc irq_flag`(761-765). The A12 rise is at
   1,250,759. Asserting combinationally raises `/IRQ` at 1,250,760 — inside the
   second nop — so the handler beats the `inc`. One cycle later it lands after
   it, which is what the ROM asks for.

#### Reproducing it before changing anything

```bash
O=~/Code/OSS_Public-Projects/RustyNES; C=$O/crates/rustynes-cosim
OUT=~/.cache/rustynes-cosim/blargg-mmc3; mkdir -p "$OUT"   # NOT /tmp: it is tmpfs
B=$O/tests/roms/blargg/mmc3_test_2

# (a) the oracle's own IRQ assertion + the ROM's cli, from one export
cargo run -q --manifest-path "$C/Cargo.toml" --bin nes_golden_export -- \
  --rom "$B/4-scanline_timing.nes" --out "$OUT" --frames 60 --irq-trace 2000000
#   then scan the .obs.bin: writes to $E001, rises of (flags>>2|flags>>3),
#   and opcode fetches (bus_addr == pc) of $58 (CLI).

# (b) the pre-render line's sprite state -- needs the feature, it is off by default
cargo run -q --manifest-path "$C/Cargo.toml" --features ppu-state-trace \
  --bin nes_golden_export -- --rom "$B/4-scanline_timing.nes" --out "$OUT" \
  --frames 45 --ppu-state-trace 200000 --pst-frames 42:43 \
  --pst-scanlines 255:261 --pst-dots 250:330
```

**Two instrument traps that cost time and will cost it again.**
`--fetch-trace` **excludes sprite pattern fetches** — a full line records 154 of
the 170 fetches, the missing 16 being exactly the 8 sprites x 2 pattern fetches
— so it cannot see this A12 rise at all, and its sprite window shows only the
garbage `$2xxx` nametable fetches. And `--ppu-state-trace` carries no CHR
address column, so it answers *what the sprite state was*, never *what address
was driven*. The question was settled by the `/IRQ` timing plus the wiki, not by
either trace.

#### The fix, ported from the DUT, in this order

1. **Drive A12 from the pre-render line's sprite-pattern fetches.** Control:
   `2-details` sub-test 8 must still pass — it is the 241-clock assertion and it
   is what catches over- or under-counting.
2. **Register the `/IRQ` output** so the assertion the CPU samples trails the
   counter reaching zero by one CPU cycle. On the DUT this is
   `rtl/cart/cart.sv`'s `mmc3_irq_out`; here it is whatever `Mapper::irq()`
   feeds. A 0..8-cycle sweep on the DUT picked **1** uniquely — 0 fails at
   sub-test 2, 2-5 overshoot to sub-test 3 — so sweep rather than assume, and
   report the shape.

#### Acceptance

- `mmc3_test_2` reported **before and after**, per ROM, by status byte. Target:
  `4-scanline_timing` past sub-test 3; `1`, `2`, `3`, `5` still `$00`;
  `6-MMC3_alt` still failing (it is NEC rev B and this project models Sharp rev
  A — if it starts passing, the default revision has silently flipped).
- **AccuracyCoin re-measured at 141/141** via the RAM decoder — the authoritative
  line is `AccuracyCoin (RAM): pass rate = 100.00% over 141 assigned tests`; the
  framebuffer decoder's 121 is the known-buggy one and is not the figure to
  quote. nestest re-run. Neither may be asserted "by construction": this changes
  the core.
- Then **regenerate `mapper4mmc3irq065`'s golden** in the sibling and re-measure.
  It sits at **570 of 178,676** diverging cycles today (940 on nine fields),
  first at cycle **60,329**, entirely MMC3 IRQ timing. If the theory is right it
  goes to **0** and the gate can be registered — it is deliberately unregistered
  now, with its reason and numbers in `tb/regress.sh`.
- The sibling's `blargg-mmc3-gate` expectations must be revisited: they assert
  the DUT's verdicts **exactly, including the failures**, so a change in either
  console shows up as a gate failure rather than drifting.

#### What would refute this, and the standing risk

If driving the pre-render rise moves `4-scanline_timing` to sub-test 2 rather
than past 3, the clock is not the missing piece and the diagnosis is wrong —
that is precisely how the DUT's first two candidate fixes died (a flag-vs-zero
reload rule, refuted by `2-details` sub-test 7; and suppressing the clock,
refuted by sub-test 8). **Test against the ROM, not against the other console.**

The standing risk is the reverse of the usual one: RustyNES is the *oracle* for
every rung, so changing its MMC3/PPU behavior invalidates every golden that
exercises them. Regenerate and re-run the full co-simulation suite, not just the
MMC3 gates.

#### Related escape hatches to re-check in the same pass

Running `cargo test -p rustynes-test-harness --features test-roms --test mmc3`
shows this is not one isolated hatch. The **older** `mmc3_test` (v1) suite
carries the same ADR 0002 F5.0 closure on three more tests:

- `mmc3_test_v1_4_scanline_timing_strict` — ignored, "unmoved on the
  one-clock/every-cycle substrate"
- `mmc3_test_v1_5_mmc3_strict` — ignored, closed with R1
- `mmc3_test_v1_6_mmc6_strict` — ignored, closed with R1

All three were closed for the same stated reason as sub-ROM 4 of `mmc3_test_2`,
and all three are IRQ-timing tests on the same counter. If the pre-render A12
rise is the mechanism, they are the natural place to look for it to pay off
twice, so re-run the whole file before and after and report every `_strict` /
`_currently_fails` pair rather than only the one that prompted the work. Each
`_currently_fails` probe asserts the failure SHAPE by message, so a change that
moves a residual without closing it will fail loudly rather than silently pass —
that is the behaviour to preserve when updating them.

#### Where the prior record lives, unaltered

`crates/rustynes-test-harness/tests/mmc3.rs` records this residual as "CLOSED
by-design-permanent (ADR 0002 F5.0, 2026-07-09)". That call was made without
this mechanism in hand and is left in place, with a dated note beside it. When
the fix lands, flip `mmc3_test_2_4_scanline_timing_strict` off `#[ignore]` only
if it genuinely passes; otherwise update the `_currently_fails` probe's expected
sub-test, which is asserted by name and will fail loudly if the shape changes.

## Open questions blocking planning

None block Phase 1. Open questions in the docs (esp. `architecture.md`, `mappers.md`) will be revisited at the start of the phase that needs them resolved.
