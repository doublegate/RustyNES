# RustyNES MiSTer core — sprint plan

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
| M15 | v2.6.5 | `sys/`, `emu`, `hps_io`, video, OSD | **Timing closure**; first `.rbf` | — |
| M16 | v2.6.6 | Hardware bring-up, both boards | **One `.rbf` boots both.** Rung 6 closes | — |
| M17 | v2.6.7 | SDRAM controller | MMC3's 6 Mb addressable | — |
| M18 | v2.6.8 | MMC1, UxROM, CNROM, AxROM | `holy_mapperel` per board | — |
| M19 | v2.6.9 | MMC3 | `mmc3_test_2`, `mmc1_a12`. **Rung 7 closes** | — |
| M20 | v2.7.0 | Contribution package | Checklist green; submission sent | — |

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
