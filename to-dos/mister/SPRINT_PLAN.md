# RustyNES MiSTer core — sprint plan

## v3.1 → v4.0 (current, 2026-10-07)

One release per phase step. The release plans in `to-dos/plans/` carry the
gates per row. Decisions D1-D28 are tabled in
[`v3.1-to-v4.0-line-plan.md`](../plans/v3.1-to-v4.0-line-plan.md).

| Sprint | Release | Deliverable | Gate | Status |
|---|---|---|---|---|
| M21 | v3.1.0 | RTL-1, RTL-5, RTL-10; TL-1; the self-hosted ladder runner (D16); Phase S documents (SUB-1, SUB-2, SUB-5) | Each RTL item measured per cycle; the runner runs one PR's ladder end to end; `contribution_checklist_audit.rs` green | — |
| M16 | HW (numbered after the session, D1) | **Hardware bring-up on the SuperStation One** (D11): Strands A-F, HW-O6, the fixes as gates, the anchors flipped | Every `bringup-log.md` row PASS against the console's md5; the ladder green on the tagged commit. **Rung 6 closes** | **Waiting on the session**; no longer blocked on a second board |
| M20 | after HW | The contribution decision (D10): the feature delta, the incumbent number, then public repository and submission yes or no | The maintainer's decision recorded | — |
| M22 | v3.2.0 | Phase F1: options, SUROM/SXROM, about ten cheap families, paddle, Four Score, cheats | Per family: bus, checkpoint and commercial-frame gates, a caught mutant; one seed sweep | — |
| M23 | v3.3.0 | Phase F2: the arbiter, DDR3, save states, rewind, the real `hps_io`; off-die becomes the headline (D4) | Save states oracle-gated by a round trip; no SDRAM request over budget | — |
| M24 | v3.4.0 | Phase F3a: MMC2/4, FME-7/5B, VRC2/4, the Zapper | As M22, plus audio gates | — |
| M25 | v3.5.0 | Phase F3b: MMC5, N163, VRC6, VRC7, Bandai FCG, expansion audio, Famicom peripherals | As M24 | — |
| M26 | v3.6.0 | Phase F4a: PAL and Dendy, VMODE | PAL goldens gated | — |
| M27 | v3.7.0 | Phase F4b: FDS with its audio | FDS goldens gated; a disk write round-tripped through the HPS | — |
| M28 | v3.8.0 | Phase F4c: NSF, Vs. System, band-limited audio | Each gated against the oracle | — |
| M29 | v3.9.x | The parity re-measure; the RC pair | Two clean compiles of each build byte-identical | — |
| M30 | v4.0.0 | **Feature parity** (D3) | The parity table complete, with the re-measured incumbent | — |

### Re-planning triggers

- **HW-A8 says the device is the 5CSXFC6D6F31.** Stop feature work. A second
  Quartus target and sweep is an L-sized re-target. The hardware release's
  number (D1) is chosen with that cost known.
- **On-die M10K passes about 95%.** Move that release's memory-hungry family to
  off-die only, rather than shrinking something that already works.
- **A Provenance-headered family will not yield to rungs 1-3.** Escalate to an
  ADR 0037 amendment naming the maintainer (D15) before anything is read, or
  defer the family.
- **A seed sweep has no seed that closes on both builds.** Ship the previous
  timing-clean pair, as v2.9.3 and v2.9.8 did, and record it.

---

## History: v2.5.1 → v3.0.0

> **The version numbers in this section are STALE.** Rung 7 sits above rung 6 in a
> ladder where a rung may not start until the one below is green, and rung 6 is
> blocked on hardware this machine does not have. v2.6.7 – v2.6.9 went to the
> bitstream and to widening the co-simulation gates instead. Treat the ordering
> here as real and the version labels as not yet assigned; `to-dos/mister/TASKS.md`
> carries the corrected form.

One release per sprint. Each sprint is done when its gate is green **and** the
per-rung doc records what the rung cannot verify.

| Sprint | Release | Deliverable | Gate | Status |
|---|---|---|---|---|
| M1 | v2.5.1 | ADR 0038 injection API; interrupt sweep | 0 divergences across ~20 hazard opcodes; **both ADR preconditions measured** | **done** — 60 injection points, 0 divergences |
| M2 | v2.5.2 | PPU register file, VRAM/palette bus, mirroring | Register side effects per cycle | **done** — 12,840 records |
| M3 | v2.5.3 | `v`/`t`/`x`/`w`, `$2005`/`$2006` | Mid-frame scroll writes | **done** — 19,813 records |
| M4 | v2.5.4 | Background fetch pipeline | Per-dot fetch addresses | **done** — 6,247 fetches |
| M5 | v2.5.5 | Background render → `index_framebuffer` | First frame, popcount 0 | **done** — 61,440 pixels |
| M6 | v2.5.6 | **Sprite evaluation FSM** | Evaluation behaviour as seen through `$2004` reads on the CPU bus — the only pin-observable window onto it, and the method the wiki says the behaviour was characterised by | **done** — 59,993 cycles exact |
| M7 | v2.5.7 | Sprite render, priority, sprite-0, overflow | Three purpose-built sprite-0 edge ROMs + four bus/fb surfaces (the blargg sprite ROMs need VBlank sync, which is v2.5.8) | **done** — every rung gate exact; all ten mutations CAUGHT; `PPU_LEAD=2` |
| M8 | v2.5.8 | VBlank/NMI, `$2002` race, the skip's write-boundary edge | **Rung 3 CLOSED**; nestest at 5,002,992 cycles | **done** — 12/12 mutations; both fixes were deletions |
| M9 | v2.5.9 | **APU pulse channels + frame counter** | `$4015`, the `/IRQ` pin, and the integer channel levels per CPU cycle | **done** — rung 4 open; 9/10 mutations; one characterised residual |
| M10 | v2.6.0 | APU triangle + noise | Same | — |
| M11 | v2.6.1 | APU DMC + DMA stealing | Cycle-exact CPU stall | — |
| M12 | v2.6.2 | Frame-counter IRQ; blargg APU battery | **Rung 4 closes**; `cpu_interrupts_v2` | — |
| M13 | v2.6.3 | NROM; full system in simulation | First AccuracyCoin run | — |
| M14 | v2.6.4 | AccuracyCoin parity | Entry-for-entry. **Rung 5 closes** | — |
| M15 | v2.6.6 | `sys/`, `emu`, `hps_io`, video, OSD | **Timing closure**; first `.rbf` | **Done** at v2.6.6 (`sys/` verbatim, 57 files); the `.rbf` shipped at v2.6.7 |
| M16 | — | Hardware bring-up, both boards | **One `.rbf` boots both.** Rung 6 closes | **BLOCKED — no board**, checked each release rather than assumed. The version is struck: eight releases have passed the slot, so naming one is a prediction |
| M17 | v2.6.13 | SDRAM controller | Read/write timing against the real part | **Done, and the blocker was REFUTED.** "Needs hardware" applied to ACCEPTANCE against the real part, not to building the thing: v2.6.13 wrote the controller, a four-way arbiter and a console bridge from the datasheet and accepted them against a behavioural part model. Enabling it is a separate question — see `USE_SDRAM_CART` |
| M18 | v2.6.9 | MMC1, UxROM, CNROM, AxROM | Per-board bus + checkpoint gates | **Done** — shipped with M19 |
| M19 | v2.6.9 | MMC3 | `mmc3_test_2` 4/6, level with the oracle | **Banking done**; rung 7's remaining half is the off-die build, which is written and measured at 140/142, not the controller being absent |
| M20 | v3.x | Contribution package | Checklist green; submission sent | **BLOCKED on hardware evidence**, by maintainer decision (2026-09-04): the submission waits for a core that has run on a board rather than one nothing has run. A SuperStation One is now in hand; the bring-up is v2.9.2, after the audit lines. Was v2.7.0; re-targeted to v3.0.0 by ADR 0041 (2026-09-22), then to v3.x, the hardware-verification release, by ADR 0043 (2026-09-29). v3.0.0 shipped the release-candidate core (2026-10-06) |

**Status is a claim about a recorded manual run, not about CI.** None of the DUT
gates run in the sibling repository's workflows — they need the oracle's goldens
and a `cargo` build of `rustynes-cosim`. Results live in that repo's
`docs/rung1-6502.md`, `docs/rung3-ppu.md` and `docs/rung4-apu.md`.

## Re-planning triggers

Named now, so a slip is a decision rather than a drift:

- **M6 (sprite evaluation) overruns by more than one sprint.** Expected; it is the
  hardest item. Split it rather than compressing M7.
- **M14's AccuracyCoin floor lands below ~90/141.** Stop and diagnose before
  proceeding to integration — a low score means a PPU or APU defect that rungs 3
  and 4 did not catch, and that is more valuable to know than a `.rbf`.
- **M15 fails timing closure.** Re-fit history exists from every prior rung close,
  so the regression is bisectable. Do not proceed to hardware on a failing fit.
- **M17's SDRAM controller needs a third-party reference.** ADR **before** any
  source is opened — no exceptions, and this is the item most likely to test it.
