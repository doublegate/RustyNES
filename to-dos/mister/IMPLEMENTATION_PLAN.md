# RustyNES MiSTer core — implementation plan, v2.5.1 → v2.7.0

**Companion to** `to-dos/plans/v2.7.0-mister-core-plan.md` (the narrative plan) and
`docs/mister.md` (the living spec). This file is the execution view: what is done,
what is next, and what each release owes.

**Goal:** a functioning, feature-complete RustyNES core for MiSTer FPGA at
**v2.7.0**, suitable for contributing per
`ref-docs/2026-08-23-mister-core-contribution-requirements.md`.

## Where the core actually is

<!-- This table is present tense, so it goes stale silently. It was eight
     releases out of date when v2.6.15 swept it -- claiming the APU, the
     cartridge, the SDRAM controller and `sys/` were all "Not started" and the
     `.rbf` "Never produced", every one of which had shipped. Update it in the
     same change as the thing it describes, or delete the row. -->

| Component | State |
|---|---|
| 6502 | **Done.** `rtl/cpu6502.sv`, nine opcode-group ROMs, 2115 records on rung 1; the bus gate at 49,993 cycles on `ppuscroll`; **59,554 cycles of nestest** (was 27,388 — the old bound was a missing `$2002` answer, not a CPU wall, and it moved the moment the PPU register file existed); the interrupt sweep at 60 injection points |
| PPU | **Rung 3 CLOSED (v2.5.8).** `rtl/ppu2c02.sv`: the register file (v2.5.2), the scroll address logic (v2.5.3), the background fetch pipeline (v2.5.4), background rendering (v2.5.5, all 61,440 pixels), sprite evaluation (v2.5.6, **59,993 of 59,993 cycles**, 9 of 9 behavioural mutants caught), sprite rendering and sprite-0 (v2.5.7, exact after the two-dot phase fix), and VBlank/NMI with the `$2002` race (v2.5.8) |
| APU | **Rung 4 CLOSED (v2.6.2).** `rtl/apu2a03.sv`: both pulses, triangle, noise, sweep, the frame counter, the DMC and its DMA cycle steal. blargg's 2005 APU battery **11 of 11** on the DUT — an INDEPENDENT oracle, and it found six defects no self-written gate could see |
| Cartridge / mappers | **Rung 7, banking half GREEN (v2.6.9-v2.6.12).** `rtl/cart/cart.sv` decodes the approved six: NROM (0), MMC1 (1), UxROM (2), CNROM (3), MMC3 (4), AxROM (7). Six commercial titles render byte-identically to the oracle over all 61,440 pixels |
| SDRAM controller | **Written (v2.6.13), and not switched on.** `rtl/sdram.sv`, `sdram_arbiter.sv`, `cart_sdram.sv` plus a behavioural part model, all from the AS4C32M16SB-7 datasheet rev 1.4 with no third-party controller read. Configured off-die it compiles, closes timing at +0.199 ns and passes **148 of 148** gates (v2.6.16 re-measured this — 147 on the die, the difference being the SDRAM-latency gate, which is N/A there; the row said 140 of 142, which v2.6.16 also had to correct in three places in the sibling — the same expired number, one repository over). The CPU's PRG deadline is MEASURED on the console at **24 cycles against 24 — met, at zero margin**, on 240,303 requests; `tb/sdram_arb_main.cpp`'s 28 is a stimulus that issues CHR and PRG on the same cycle, which the console never does (zero coincidences in fifteen million). Scheduling would buy margin the console has none of, which is now the reason to build it — see `USE_SDRAM_CART` in `rtl/emu.sv` |
| `sys/` + `emu` integration | **Done (v2.6.6).** `sys/` vendored byte-identical to `Template_MiSTer@3ea1134c`, 57 files, and since v2.6.15 that is a CI gate (`tb/check_sys.py`) rather than a measurement taken once |
| `.rbf` | **Shipped every release since v2.6.7**, with the timing report checked at all four corners and a pinned fitter seed. Named `RustyNES_YYYYMMDD.rbf` since v2.6.15 — the only form both MiSTer parsers accept |

## Scope, decided 2026-08-23

- **Mappers: the top six** — NROM, MMC1, UxROM, CNROM, MMC3, AxROM (~90% of the
  licensed library by title count).
- **Hardware: both boards** — DE10-Nano + the mandatory SDRAM add-on, and a
  SuperStation One. One `.rbf` must boot both.
- **Not in this line:** FDS, expansion audio, savestates, Vs. System, NSF, the
  remaining ~168 mapper families, AccuracyCoin beyond a stated floor.

## The arithmetic, stated up front

Rung 3 8–16 wk · rung 4 4–8 wk · rung 5 2–4 wk + a 4–12 wk tail · rung 6 2–4 wk ·
rung 7 4–8 wk = **20–40 weeks FTE**. The twenty release slots between v2.5.1 and
v2.7.0 are **milestones, not dates**.

## Standing rules for every release in this line

1. **A rung may not start until the one below is green.** As written this said
   "green in CI", and that is **not currently achievable** — none of the DUT
   gates run in CI, because they need the oracle's goldens and a `cargo` build of
   `rustynes-cosim`, neither of which exists in the sibling repository's
   workflows. They are run by hand and their results recorded in the per-rung
   docs. Fetching goldens from a pinned oracle commit is the missing piece, and
   until it is built this rule means "green on a recorded manual run" — said
   plainly, because a rule nobody can satisfy is a rule that quietly stops being
   applied.
2. **Every new gate is demonstrated to fail by mutation** before it is trusted —
   three outcomes (CAUGHT / NOT CAUGHT / BUILD-FAILED), baseline captured once
   into a file the harness refuses to overwrite.
3. **Every per-rung doc records what the rung cannot verify.** This is the section
   that keeps the ladder honest; `docs/rung1-6502.md` is the shape to follow.
4. **The partition between gate and diagnostic is written before the rung
   starts**, not after a divergence forces the question.
5. **Quartus re-fit at every rung close**, with `Fmax` recorded — not once at
   v2.6.5.
6. **No upstream libretro/RetroArch sync** until the MiSTer core is complete.
7. **The firewall holds.** `NES_MiSTer` and `fpganes` stay physically outside the
   workspace. Anything unimplementable from documentation escalates to an ADR
   **before** any source is opened.
