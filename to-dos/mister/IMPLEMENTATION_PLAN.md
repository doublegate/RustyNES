# RustyNES MiSTer core — implementation plan, v2.5.1 → v2.7.0

**Companion to** `to-dos/plans/v2.7.0-mister-core-plan.md` (the narrative plan) and
`docs/mister.md` (the living spec). This file is the execution view: what is done,
what is next, and what each release owes.

**Goal:** a functioning, feature-complete RustyNES core for MiSTer FPGA at
**v2.7.0**, suitable for contributing per
`ref-docs/2026-08-23-mister-core-contribution-requirements.md`.

## Where the core actually is

| Component | State |
|---|---|
| 6502 | **Done.** `rtl/cpu6502.sv`, nine opcode-group ROMs, 2115 records on rung 1; the bus gate at 49,993 cycles on `ppuscroll`; **59,554 cycles of nestest** (was 27,388 — the old bound was a missing `$2002` answer, not a CPU wall, and it moved the moment the PPU register file existed); the interrupt sweep at 60 injection points |
| PPU | **Four of seven steps.** `rtl/ppu2c02.sv`: the register file (v2.5.2, 12,840 records), the scroll address logic (v2.5.3, 19,813 records) the background fetch pipeline (v2.5.4, 6,247 fetches) and background rendering (v2.5.5, **all 61,440 pixels**, the first full frame). **No sprites, no VBlank/NMI timing.** v2.5.6, sprite evaluation, is in progress |
| APU | **Not started** |
| Cartridge / mappers | **Not started** |
| SDRAM controller | **Not started** |
| `sys/` + `emu` integration | **Not started** — `sys/` is an empty placeholder |
| `.rbf` | **Never produced** |

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
