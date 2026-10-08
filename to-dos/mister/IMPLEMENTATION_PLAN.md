# RustyNES MiSTer core — implementation plan

## v3.1 → v4.0: feature parity, then the hardware release (current, 2026-10-07)

The execution view of the MiSTer half of
[`v3.1-to-v4.0-line-plan.md`](../plans/v3.1-to-v4.0-line-plan.md). The
decisions it cites (D1-D29) were taken by the maintainer on 2026-10-07 and are
tabled there. **D29, later that day, moved Phase H from right after v3.1.0 to
the end of v3.9.x**, after every feature phase, so the board verifies the most
complete core; the phase table below is in the new order. The narrative for the hardware release is
[`v3.x-hardware-verification-plan.md`](../plans/v3.x-hardware-verification-plan.md).

### Where the core actually is (after v3.0.1)

<!-- Present tense, so it goes stale silently. Update it in the same change as
     the thing it describes, or delete the row. -->

| Component | State |
|---|---|
| Rungs 1-5, 7 (banking) | Closed. The CPU, PPU, APU, the six mappers and the full system are cycle-exact against the oracle on every gate. Ladder **199 passed / 0 failed / 1 expected failure** on-die and **200 / 0 / 1** off-die at v3.0.0; v3.0.1 adds `mapper4mmc3oddskip080` and the dot-0 A12 fix (ledger 3.49) |
| Rung 6 (hardware) | **Open.** The SuperStation One is in hand. **No hardware has run any bitstream** |
| Mappers | 0 NROM, 1 MMC1 (up to 256 KiB PRG; SUROM/SXROM refused), 2 UxROM, 3 CNROM, 4 MMC3 (rev A; NES 2.0 board-variant submappers refused), 7 AxROM (`rtl/ines_header.sv`) |
| Cartridge size | on-die 256 K PRG / 128 K CHR; off-die 512 K / 256 K |
| Saves, OSD, input | Battery saves through the HPS (v2.6.21, not yet seen on hardware); aspect, scandoubler, scale, crop and palette options (v2.9.8, not yet seen); two pads |
| Region, audio | NTSC only; the 2A03 only, with no band-limiting and no expansion audio |
| Absent | Save states, cheats, FDS, NSF, Vs. System, the Zapper, Four Score, paddle, keyboard, PAL |
| Fit | On-die 22,922 / 41,910 ALMs (55%), **468 / 553 M10K (85%)**, 33 / 112 DSP; off-die 24,570 ALMs, 84 M10K (15%) |
| CI | Nine rung-1 gates against a pinned oracle, plus the module gates. The full ladder runs by hand from a frozen worktree, and from v3.1.0 on a self-hosted runner (D16) |

### The phases

| Phase | Release slot | Content | Decisions |
|---|---|---|---|
| **S** (submission prep, docs only) | v3.1.0, then kept current through to H | SUB-1 (a dated `ref-docs/` record of the live contribution page; rewritten 2026-09-19/20, last edited 2026-09-26; **done v3.1.0**, `ref-docs/2026-10-07-mister-core-contribution-requirements-update.md`), SUB-2 (re-scope the checklist), SUB-5 (refresh `submission-case.md`); TL-5 (this file, done) | D10, D14 (the RTL keeps its long comments) |
| **F1** (cheap breadth) | v3.2.0 | FB-16 options first (custom palette, +8 sprites), FB-2 SUROM/SXROM, the 206 family, 66, 11, 79, 9/10, 118/119, 71/232, 34, the trivial discretes, FB-10 paddle, FB-8 Four Score, FB-6 cheats | D12, D26 |
| **F2** (the memory platform) | v3.3.0 | FB-20 arbiter (RTL-9 closes), DDR3, FB-4 save states, FB-5 rewind, the real `hps_io` under Verilator; **the off-die build becomes the headline** and on-die a "lite" build | D4, D12, D16 |
| **F3** (big boards, audio) | v3.4.0-v3.5.0 | MMC2/4, FME-7/5B, VRC2/4, the Zapper (v3.4.0); MMC5, N163, VRC6, VRC7, Bandai FCG with expansion audio, Famicom peripherals (v3.5.0) | D15 |
| **F4** (region and media) | v3.6.0-v3.8.0 | PAL/Dendy with VMODE (v3.6.0); FDS (v3.7.0); NSF, Vs. System, band-limited audio (v3.8.0) | D15 |
| **Freeze** | v3.9.0 | the RTL feature freeze, the parity re-measure, the release-candidate pair the board session runs on | D29 |
| **H** (hardware) | the last v3.9.x (D29), numbered after the session, no later than v4.0.0 | HW-0, Strands A-F on the SuperStation One on the frozen pair, HW-O6, fixes as gates, the re-sweep, the anchors flipped. No feature RTL | D1, D11, D13, D29 |
| **Parity** | v4.0.0 | save states, cheats, PAL/Dendy, FDS with expansion audio, the Zapper, Four Score, the licensed-library mapper list, the re-measured incumbent | D3 |
| **After** | v4.x | the SuperStation One distribution channel, then an openFPGA (Analogue Pocket) port | D17 |

Open RTL items slot into the start of any release: RTL-1, RTL-5 and RTL-10 at
v3.1.0. RTL-2 and RTL-4 are investigations. RTL-3 (OAM corruption) and RTL-6
(APU power-on phase) wait on the board, which since D29 means the end of v3.9.x.

**Risk of D29, for this half:** features F1-F4 land verified in simulation
only, and HW-A8/HW-A9 (the FPGA device and SDRAM part) are read last. Write each
feature's `bringup-log.md` rows as it lands, so the session's checklist is ready
rather than assembled at the end. The full list:
[Risks of D29](../plans/v3.1-to-v4.0-line-plan.md#risks-of-d29).

### Scope, re-decided 2026-10-07

- **Mappers: ranked by real titles** (D26). This replaces the 2026-08-23 "top
  six" scope, under which `TASKS.md` recorded the remaining families as out of
  scope.
- **Hardware: the SuperStation One** (D11). The DE10-Nano is optional.
- **Provenance-headered families** use rungs 1-3. Rung 4 needs an ADR 0037
  amendment naming the maintainer, per family (D15). Those families are N163,
  FME-7/5B, VRC7, Bandai FCG, FDS and Vs., and anything else the command finds:
  `grep -rln "^// Provenance:" crates`.
- **Not planned:** run-ahead, rollback netplay, HD packs, HD audio, TAS, Lua,
  the debugger.

### Standing rule 1, restated

"A rung may not start until the one below is green" now means green on a
recorded ladder run. That is the frozen worktree until the self-hosted runner
exists, and both afterwards. CI's nine rung-1 gates are a subset and never
stand in for the ladder. Rules 2-7 below are unchanged.

**Rung 6 is the exception, by D29.** Rung 6 (hardware) stays open while the
feature phases above it proceed: they need a green ladder, not a board. It
closes at the hardware release, at the end of v3.9.x.

---

## History: the v2.5.1 → v3.0.0 plan

> **Re-targeted twice.** [ADR 0041](../../docs/adr/0041-hardware-release-is-v3.0.0.md)
> (2026-09-22) made the hardware-verified core and the contribution package v3.0.0;
> [ADR 0043](../../docs/adr/0043-v3-is-the-api-major-and-a-release-candidate-core.md)
> (2026-09-29) then made v3.0.0 the API major with a **release-candidate** core, and
> moved hardware verification and the contribution package to a later **v3.x**
> release. v3.0.0 shipped on 2026-10-06. Read "v2.7.0" below as the milestone that
> became v3.x. The execution view from here is
> [`v3.x-hardware-verification-plan.md`](../plans/v3.x-hardware-verification-plan.md).

**Companion to** `to-dos/plans/v2.7.0-mister-core-plan.md` (the narrative plan) and
`docs/mister.md` (the living spec). This file is the execution view: what is done,
what is next, and what each release owes.

**Goal:** a functioning, feature-complete RustyNES core for MiSTer FPGA at
**v3.x**, the hardware-verification release (v2.7.0 until ADR 0041, v3.0.0
until ADR 0043), suitable for contributing per
`ref-docs/2026-08-23-mister-core-contribution-requirements.md`.

## Where the core was (as of v2.6.x; superseded by the table at the top)

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
  remaining ~185 mapper families (191 in the emulator at v3.0.0), AccuracyCoin beyond a stated floor.

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
