# RustyNES MiSTer core — task board

Legend: `[ ]` open · `[~]` in progress · `[x]` done

## v2.5.1 — the interrupt sweep (rung 2 completes)

- [x] Implement ADR 0038's injection API behind `cosim-interrupt-inject`
- [x] **Precondition A:** structural, not benchmarked — `inject_` appears **0**
      times in the expanded default core and **17** with the feature on, measured
      with a live control. When the default build emits none of it there is no
      hot-path quantity left to measure.
- [x] **Precondition B:** AccuracyCoin **141/141** (RAM decoder) and nestest
      0-diff, verified rather than asserted, because `rustynes-core` changed.
- [x] Sweep: NMI, IRQ **and both together** at every offset across 20
      instructions — **60 injection points, 0 divergences**.
- [x] Rung-2 interrupt findings recorded in the sibling's `docs/rung1-6502.md`,
      with its cannot-verify section. (Kept there rather than in a new
      `rung2-interrupts.md`: the sweep is rung 2's interrupt half and its
      findings are inseparable from the rung-1 ROMs they were found against.)
- [x] Neither precondition failed, so the fallback to option B (waiting for rung
      4's `cpu_interrupts_v2`) was not needed.

## v2.5.2 – v2.5.8 — rung 3, the 2C02

- [x] v2.5.2 register file `$2000-$2007`, VRAM/palette bus, mirroring — **12,840 records, 0 divergences**, 8 mutations all caught; also the post-reset masking window, which the plan did not anticipate
- [x] v2.5.3 `v`/`t`/`x`/`w` and the `$2005`/`$2006` sequence — **19,813 records, 0 divergences**; bus gate 49,993/49,993; and a 3-dot delay on toggling rendering that the plan did not anticipate, adjudicated by the wiki because both implementations were self-consistent
- [x] v2.5.4 background fetch pipeline (NT/AT/pattern) — **6,247 background fetches, 0 divergences**, 8 mutations all caught. Gated on the ADDRESS BUS, not the latches. Found that the testbench presented every CPU access two dots early (a 6502 commits at phi2) — invisible to all five existing gates, which sample once per CPU cycle. nestest's window went **27,388 → 59,554 cycles**. Shift registers and fine-X are NOT here: they are diagnostic, and the pixel they feed is v2.5.5's gate
- [x] v2.5.5 background rendering into `index_framebuffer` — **all 61,440 pixels match**, 15 mutations all caught. The oracle needed NO change (the golden has existed since v2.4.1). Five NOT CAUGHT mutations indicted the STIMULUS, one of them three times over
- [x] v2.5.6 **sprite evaluation FSM** — 59,993/59,993 cycles, 9/9 behavioural mutants caught
- [x] v2.5.7 sprite rendering, priority, sprite-0 hit, overflow — every rung gate exact (zero divergences), all ten catalog mutations CAUGHT, `PPU_LEAD=2` phase fix + odd-frame skip + `cpu_ce`
- [x] v2.5.8 VBlank/NMI timing, `$2002` race, the skip's write-boundary edge — **RUNG 3 CLOSED**: four ROMs, 12/12 mutations CAUGHT, nestest 5,002,992 cycles, two structures deleted
- [x] Write the rung-3 gate/diagnostic partition **before** the rung — landed as
      the sibling's `docs/rung3-ppu.md`, not `docs/mister-ppu-rung.md` as planned
      here (it belongs beside the RTL it constrains). v2.5.3 is the proof it
      works: `ppu-state-trace` located the bug and never became a gate
- [~] On rung 3 close: re-run nestest **unbounded** and the 5 M-cycle window,
      which v2.5.0 could not reach. **Partly done at v2.5.4**: the window was
      never a CPU wall, only a missing `$2002` answer, so it moved 27,388 →
      59,554 cycles the moment the register file existed. It is now bounded by
      the two-frame golden's length — an artifact budget — so "unbounded" means
      exporting more frames, not finding a new failure

## v2.5.9 – v2.6.2 — rung 4, the 2A03

- [x] v2.5.9 **APU pulse channels + frame counter** — rung 4 OPEN: two ROMs, 9/10 mutations CAUGHT, one characterised 1-tick residual carried to v2.6.0
- [x] v2.6.0 **triangle + noise** (plus the sweep unit) — and an audit of how much
      of the APU was fitted to the oracle rather than derived from documentation
- [x] v2.6.1 **DMC, and its DMA stealing back into the CPU** — cycle-exact on the bus
- [x] v2.6.2 **frame-counter IRQ edges; blargg APU battery — rung 4 CLOSES.**
      Battery **11/11 exact**, **48 gates green, 0 failed**, catalog 56 entries, **56/56 mutations CAUGHT**.
      Six root causes (ledger 8.3-8.9), the largest being that the frame
      sequencer counted APU cycles where the documentation's step positions are
      CPU cycles — which forced every constant to be calibrated per step.
      Two of the six rules are absent from the nesdev wiki and stated in
      blargg's own `readme.txt`: the length-halt delay and the reload drop.
- [ ] Run `cpu_interrupts_v2` — the independent interrupt oracle, now reachable
- [ ] **Carried from v2.6.2:** the power-up `$4017` rewrite (blargg's readme:
      the APU acts as if `$4017` were written with `$00` 9-12 clocks before the
      first instruction). The oracle models it, the DUT does not, and no ROM in
      the battery isolates it — see ledger 8.10

## v2.6.3 – v2.6.4 — rung 5, NROM + AccuracyCoin

- [~] v2.6.3 NROM cartridge; first end-to-end AccuracyCoin run
  - [x] `rtl/cart/cart_nrom.sv` — written from `nesdev_wiki/NROM.xhtml`
        (public source: <https://www.nesdev.org/wiki/NROM>),
        instantiated beneath the existing gates and cross-checked per access
        rather than trusted. Runtime write port, not `$readmemh`: on MiSTer the
        image arrives from `hps_io` at runtime, so the port that ships is the
        port that is tested
  - [x] `rtl/wram.sv` — 2 KiB and the `$0000-$1FFF` decode. **50 gates green**,
        5 mutations CAUGHT. Its mirroring was invisible to the entire prior
        corpus: all 48 ROMs stay inside `$0000-$07FF`, so three mirror-only
        mutations came back **NOT CAUGHT** against `blargg03`'s 893,410 cycles
        and CAUGHT against the new `wrammirror044`. A stimulus NOT CAUGHT — the
        fourth distinct meaning that verdict has had here
  - [x] The harness's own memory model was wrong first: it wrote `mem[addr]`
        unmirrored, so a correctly-mirrored RAM would have been reported as the
        defect. Fixed before the RTL was trusted
  - [x] `rtl/cpu_bus.sv` — the address decode, read mux and open-bus latch.
        **The flat 64 KiB array no longer answers the CPU**; the RTL bus does,
        and the harness's array survives only as a model scoped to the windows
        it can independently speak for. The testbench's hardcoded `$40` for a
        `$4016` read is now DERIVED from the latch
  - [x] `rtl/controller.sv` — the standard pad's shift register. `cpu_bus`
        first returned a bare 0 in D0, which is not a model of "no controller"
        but a model of nothing, and diverged from the oracle on the NINTH read
  - [x] **Found the ORACLE wrong, not the DUT** — NROM provides PRG-RAM at
        `$6000-$7FFF` where the board has none. Recorded in
        `docs/accuracy-ledger.md` with the wiki citation and the games the wiki
        names as breaking on it. First time the ladder has caught the oracle;
        risk 6 in the v2.5.0 plan
  - [x] `rtl/dma.sv` — the documented four-phase DMC sequence, load-bearing.
        The byte comes off `cpu_bus` at the DMA's own address, so a DMC fetch is
        a real bus cycle by a **second requester**. Two ordering defects found
        by side-by-side comparison, in opposite directions: the load delay one
        cycle late (non-blocking assignment), then one cycle early (a `$4015`
        write landing on an APU phase edge must be armed AND decremented by it)
  - [x] `rtl/controller.sv` — and the address mux placed at the CONSOLE level,
        not inside the read mux, so the memories see the DMA's address
  - [x] **Prediction recorded as WRONG.** The previous step predicted in writing
        that two inert open-bus mutations would flip to CAUGHT once DMA drove
        the bus. They did not, and a third joined them. The cause is stimulus:
        no ROM in the corpus both runs DMC fetches and reads open bus
  - [ ] **`rtl/nes_top.sv` is still a shell** — the one remaining piece. Every
        part exists and is gated, but they are tied together by the
        co-simulation wrapper rather than by the core's own top level, and that
        top level must also divide the master clock. That is the apparatus rung
        3's phase calibration was built on, so it is a change to the timing
        substrate rather than a rewiring — named as its own step for that reason
  - [ ] First end-to-end AccuracyCoin run — blocked on the above, and NOT
        attempted. Producing a status vector is v2.6.3; matching it is v2.6.4
- [ ] v2.6.4 status vector identical **entry-for-entry**, including `Skipped` and
      `NotRun` — **rung 5 closes**. State a floor, not a target

## v2.6.5 – v2.6.6 — rung 6, MiSTer integration and hardware

- [ ] v2.6.5 `sys/` verbatim; `emu` module; `hps_io`; `CE_PIXEL` video; `CONF_STR`
      OSD; `VIDEO_ARX/ARY` at **8:7, set deliberately**; `files.qip`, `.sdc`,
      `clean.bat`; **Quartus timing closure**; first `.rbf`
- [ ] v2.6.6 hardware bring-up: DE10-Nano + SDRAM add-on, SuperStation One,
      **one `.rbf` boots both**; on-device AccuracyCoin — **rung 6 closes**

## v2.6.7 – v2.6.9 — rung 7, memory and mappers

- [ ] v2.6.7 SDRAM controller from spec (ADR first)
- [ ] v2.6.8 MMC1, UxROM, CNROM, AxROM
- [ ] v2.6.9 **MMC3** — A12 filtering, IRQ counter — **rung 7 closes**

## v2.7.0 — the contribution package

- [ ] Requirements checklist green (`contribution-checklist.md`)
- [ ] `releases/RustyNES_YYYYMMDD.rbf`
- [ ] Unique Home folder chosen
- [ ] Email `newcores@misterfpga.org`
- [ ] **Decide deliberately** whether to transfer the repository to MiSTer-devel —
      acceptance means the repo moves, and that is one-way
