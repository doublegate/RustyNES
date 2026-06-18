# Per-DMA Mesen-exact OAM/DMC DMA cycle-count fix — implementation design

Branch: refactor/v2.0-master-clock. Feature gate: `r4-cpu-dma` (NOT in the
default feature set — default = std, accuracycoin-sprite-eval-base-from-oamaddr,
mc-dmc-rdy-stall, cpu-implied-dummy-reads). All work lands behind `r4-cpu-dma`
(+ a new sub-flag) so the default build stays byte-identical.

## Root cause recap (proven, §15/§16)
- `Cpu::process_pending_dma` (crates/nes-cpu/src/cpu.rs:828) drives get/put off
  `self.cycles & 1` via the `get_parity` knob. `self.cycles` (post-increment CPU
  counter) and the real APU clock half are LOCKED but at OPPOSITE parity vs the
  bus `self.cycle` — so a single parity proxy gives 513 at one OAM DMA and 514 at
  another, never both Mesen-exact.
- The DMC-during-OAM conflict is mis-modelled: the current loop uses the
  `sep_dummy`/`r4_dmc_noops_done` fixed-noop hack instead of Mesen's single
  shared loop where sprite DMA cycles naturally count as DMC halt/dummy cycles.
- Mesen reference is transcribed in scripts/mesen2-irq-oracle/mesen2-irq-oracle.full.patch
  lines 277-380 and docs/audit/v2.0-master-clock-precise-design-2026-05-26.md §1.

## Canonical Mesen model (the target semantics)
From the full.patch ProcessPendingDma:
1. Halt: `_needHalt=false; StartCpuCycle; Read(readAddr, DmaRead); EndCpuCycle`.
2. Loop while `_dmcDmaRunning || _spriteDmaTransfer`. Each iteration (`processCycle`
   lambda) first clears ONE of `_needHalt`/`_needDummyRead` (halt first), then:
   - `getCycle = (CycleCount & 1) == 0`.
   - getCycle:
     - if `_dmcDmaRunning && !_needHalt && !_needDummyRead`: DMC GET — read
       GetDmcReadAddress, EndCpuCycle, `_dmcDmaRunning=false`, SetDmcReadBuffer.
     - else if `_spriteDmaTransfer`: sprite READ at `0x100*offset + counter/2`.
     - else: dummy read at readAddr (DMC waiting on halt/dummy).
   - putCycle:
     - if `_spriteDmaTransfer && (counter & 1)`: sprite WRITE to $2004; counter++;
       at 0x200 → `_spriteDmaTransfer=false`.
     - else: align dummy read at readAddr.
KEY: the conflict cycles are EMERGENT, not special-cased. When DMC arms mid-OAM,
its halt+dummy consume what would have been sprite read/put cycles, and the DMC
get displaces a sprite cycle — yielding the +2 (and edge +1/+3 at the last puts)
automatically. There is NO `sep_dummy`/noop counter in Mesen. The get/put parity
is `CycleCount & 1` where `CycleCount` is Mesen's master cycle counter, which is
phase-exact because Mesen advances master_clock per cycle from power-on.

The RustyNES translation of "CycleCount & 1 == 0 is get" is the REAL APU clock
half: `apu_phase == true` is the get half (apu.rs:721, 886). So drive getCycle
off `bus.apu_phase()` instead of `self.cycles & 1`.

---

## PART 1 — Restructure `process_pending_dma`

### 1a. Source get/put from the real APU clock half
New Bus trait method (crates/nes-cpu/src/bus.rs, near dmc_dma_pending ~line 132):

    /// True when the current CPU cycle is the GET half of the APU's two-cycle
    /// phase (apu_phase==true). The DMA get/put alternation keys off this, NOT
    /// a self.cycles&1 proxy (§15/§16: the proxy cannot be Mesen-exact at both
    /// OAM DMAs). Default false for test stubs.
    fn dma_get_cycle(&self) -> bool { false }

LockstepBus impl (crates/nes-core/src/bus.rs, beside dmc_dma_pending ~3952):

    #[cfg(feature = "r4-cpu-dma")]
    fn dma_get_cycle(&self) -> bool { self.apu.apu_phase() }

CRITICAL ORDERING: `apu_phase` toggles inside `tick_with_external` (apu.rs:574),
which runs in `apu_advance_one` → called from `start_cycle`'s `cpu_clock`
(bus.rs:3898). So `bus.dma_get_cycle()` must be sampled AFTER the cycle's
start_cycle has ticked the APU — i.e. it reflects THIS cycle's half. In the
current loop the parity is read at the TOP before any read_dummy. The new loop
must instead determine get/put from `apu_phase` as it will be for the cycle the
helper is about to emit. Two viable shapes:
  (i) Read `bus.dma_get_cycle()` at the top of each iteration. Because the
      previous helper's start_cycle already toggled apu_phase for that emitted
      cycle, the value at loop-top is the half of the NEXT cycle to emit only if
      the toggle cadence is 1 toggle per emitted cycle. Confirm: every
      read_dummy/read_dummy_oam/write_dummy_oam calls start_cycle→cpu_clock→
      apu_advance_one exactly once, so apu_phase flips once per emitted DMA cycle.
      Therefore at loop-top, `bus.dma_get_cycle()` == half of the LAST emitted
      cycle; the NEXT cycle will be its inverse. So getCycle for the upcoming
      cycle = `!bus.dma_get_cycle()`. VALIDATE this inversion empirically against
      the dma_loop_trace vs the oracle dmaRdyTrace (see Part 2); if RustyNES's
      apu_phase is defined as "post-tick state for the cycle just emitted", the
      upcoming cycle uses the negation. Make the polarity a one-line const, not a
      knob, once proven.
  (ii) Add a non-mutating "peek next half" to the APU
      (`pub const fn apu_phase_next(&self) -> bool { !self.apu_phase }`) and
      forward it as `dma_get_cycle_next()` so the loop reads the upcoming cycle's
      half directly without the mental inversion. Preferred for clarity.

Replace the get_parity block (cpu.rs:954-958):
    let get_cycle = if k.get_parity == 0 { (self.cycles & 1) == 0 } else { ... };
with:
    let get_cycle = bus.dma_get_cycle_next(); // real APU clock half, not a proxy
Keep `get_parity` knob only as a debug A/B override under irq-timing-trace
(wrap: `if k.get_parity==2 { apu-half } else { legacy proxy }`) so sweeps still
work; default path uses the APU half unconditionally.

### 1b. Model the DMC-during-OAM conflict (delete the sep_dummy hack)
Rewrite the loop body (cpu.rs:907-1121) to mirror Mesen's `processCycle` exactly:

1. Remove the `was_ready_pre_cascade` / `dmc_ready` / `dmc_fire` / sep_dummy /
   `r4_dmc_noops_done` machinery (cpu.rs:916-1003).
2. Per iteration, FIRST clear one flag (keep the existing cascade, default Mesen
   order — cpu.rs:925-935 is already correct; keep cascade knob for sweeps).
3. Then `get_cycle = bus.dma_get_cycle_next()`.
4. Branch structure identical to Mesen:
   - if get_cycle:
       - if `self.r4_dmc_dma_running && !self.r4_need_halt && !self.r4_need_dummy_read`:
           DMC GET — `let a=bus.dmc_dma_addr(); let b=self.read_dummy_dmc(bus,a);
           bus.dmc_dma_complete(b); self.r4_dmc_dma_running=false;` (drop noop reset).
       - else if `self.r4_sprite_dma_transfer`: sprite read (existing branch at
         1030-1050, unchanged).
       - else: dummy read at held addr (existing 1051-1068).
   - else (put):
       - if `self.r4_sprite_dma_transfer && (counter & 1)==1`: sprite write
         (existing 1071-1101, unchanged incl. the 0x200 completion + r4_oam_trace).
       - else: align dummy (existing 1102-1119).
5. Mid-loop DMC poll: the conflict REQUIRES catching a DMC armed mid-OAM. The
   `mid_loop_dmc_poll` knob (cpu.rs:912-915) becomes mandatory — promote it to
   always-on under the new sub-flag. When it fires, set `r4_dmc_dma_running=true;
   r4_need_halt=true; r4_need_dummy_read=true;` (NOT just need_dummy_read — Mesen
   gives the freshly-armed DMC a full halt+dummy, which is what produces the +2).
   This is the single behavioral change that adds the missing conflict cycles.

Result: a PLAIN OAM DMA = halt(1) + 512 read/write + alignment(0/1 depending on
the entry get/put half) = 513 or 514, parity now driven by apu_phase so it is
Mesen-exact at BOTH OAM DMAs. A DMC-during-OAM = the same loop with the DMC
halt+dummy+get displacing 2-3 sprite cycles → the +2 (edge +1 at 2nd-to-last
put, +3 at last put) emerge from the shared loop, no special case.

### 1c. Fields to add / change (crates/nes-cpu/src/cpu.rs struct ~201-248)
- ADD nothing structurally required; the conflict is emergent.
- REMOVE (or leave dead under the old knob path): `r4_dmc_noops_done` (238). Keep
  the field for the irq-timing-trace A/B knob path but stop using it on the
  default r4 path.
- KEEP: r4_need_halt, r4_need_dummy_read, r4_dmc_dma_running, r4_sprite_dma_transfer,
  r4_sprite_dma_offset, r4_sprite_dma_counter, r4_in_process_pending_dma,
  r4_dma_cycle_active — all still used.
- r4_knobs.rs (crates/nes-cpu/src/r4_knobs.rs): add `apu_half_get: u8` (default 1
  = use apu_phase; 0 = legacy proxy) so the change is sweep-reversible. Keep
  sep_dummy/get_parity as legacy-only A/B axes.

### 1d. New sub-feature flag
Add `r4-dma-apu-half` to crates/nes-cpu/Cargo.toml and forward from
crates/nes-core/Cargo.toml (`r4-dma-apu-half = ["nes-cpu/r4-dma-apu-half"]`).
The new loop body + dma_get_cycle path compile only under this flag; without it
the existing sep_dummy loop is retained verbatim. This isolates the change for
bisection and lets the validation gate A/B the two loops in one binary.

---

## PART 2 — Cumulative-cycle-exactness (oracle validation + fix)

The apu_phase get/put only lands Mesen-exact if RustyNES's apu_phase at each DMA
matches Mesen's CycleCount parity — i.e. the cumulative cycle count from power-on
must agree. Validate and fix via the Mesen2 oracle.

### Oracle build
scripts/mesen2-irq-oracle/ + apply mesen2-irq-oracle.full.patch (the .full has the
DMA traces; the plain .patch does not). Build per run-irq-trace.sh.

### Traces to capture (env vars from the full.patch + run-irq-trace.sh)
- Mesen side:
  - MESEN_DMA_RDY_TRACE_OUT → g_dmaRdyTrace: `cpu_cycle,kind,read_addr,mc`
    (kind 0=halt, 1=dummy, 3=DMC-get). The per-DMA-cycle ground truth.
  - g_oamTrace (OAM span): `entry_cyc,exit_cyc,cyc_span,entry_mc,exit_mc,mc_span,page,halted_addr`.
  - g_dmcTrace: `cpu_cycle,getcycle,dmc_addr,halted_addr,value,conflict4000,mc`.
  - PPU2002 + 4015 traces for the downstream flag-timing alignment.
- RustyNES side (irq-timing-trace feature):
  - dma_loop_trace (RUSTYNES_DMA_LOOP_TRACE_CSV): same kind schema
    `cpu_cycle,master_clock,kind,held_addr,...` — direct analogue of g_dmaRdyTrace.
  - r4_oam_trace (the OAM span emitted at counter==0x200, cpu.rs:1093) vs g_oamTrace.
  - reg_read_trace / ppu2002_trace for the $2002 read-position cross-diff.

### Cross-diff key
Primary join key: `cpu_cycle` (Mesen `_state.CycleCount` ↔ RustyNES `self.cycles`).
Secondary: `master_clock` (Mesen `_masterClock` ↔ RustyNES `self.master_clock`).
For each DMA event row, diff (kind sequence, cyc_span, mc_span). The OAM-span diff
(r4_oam_trace vs g_oamTrace) is the headline metric: target cyc_span match at
EVERY OAM DMA, not just the boot one. Use scripts/mesen2-irq-oracle/bus_event_diff.py
and ppu2002_diff.py as the diff harness (extend with a dma_span_diff if needed —
mirror sample_oam_span.csv / sample_r4_oam_span.csv format already in the dir).

### Iterate loop
1. Pick a deterministic ROM that exercises DMC-during-OAM early: the AccuracyCoin
   sub-test ROMs under tests/roms/AccuracyCoin/sub-tests/ (frame-counter-irq,
   apu-reg-activation, controller-strobing) via the trace_* bins
   (crates/nes-test-harness/src/bin/trace_dmc_dma.rs etc.), and iflag.nes for the
   boot OAM DMA.
2. Capture both traces over the same cpu_cycle window (DMC_TRACE_START/END set to
   the first OAM/DMC region).
3. Diff. The FIRST diverging row tells you whether the gap is (a) get/put polarity
   (Part 1a inversion wrong), (b) a missing conflict cycle (Part 1b mid-loop poll),
   or (c) a cumulative cycle drift earlier than the DMA (apu_phase offset at entry).
4. For (c): the apu_phase phase at DMA entry is set by the boot sequence. Verify
   RustyNES's power-on apu_phase + cpu cycle count matches Mesen's by diffing the
   FIRST few DMA entries' (cpu_cycle, master_clock). If the entry parity is off by
   1, the drift is upstream (boot master_clock / first idle cycles) — that is the
   "make cumulative count from power-on match hardware" half: confirm the boot
   `master_clock = CPU_DIVIDER_NTSC` one-shot (cpu.rs:400) and the apu_phase init
   (apu.rs:153 false) reproduce Mesen's boot. Do NOT hand-tune; if entry parity
   diverges, the fix is in boot/reset sequencing, validated by the same diff.
5. Repeat until dma_loop_trace kind-sequence == g_dmaRdyTrace and OAM cyc_span
   matches at every DMA in window.

---

## PART 3 — Validation gate (run after EVERY change, in order)

Each gate must pass before proceeding. Stop and report on any regression.

1. CPU IRQ axis (must STAY 5/5):
   `cargo test -p nes-test-harness --features r4-cpu-dma,r4-dma-apu-half --test cpu_interrupts_v2`
   (crates/nes-test-harness/tests/cpu_interrupts_v2.rs). #4 irq_and_dma is the
   load-bearing one — it must not regress.
2. PPU VBL/NMI (10/10):
   `cargo test -p nes-test-harness --features ... --test ppu_vbl_nmi`
   (crates/nes-test-harness/tests/ppu_vbl_nmi.rs).
3. AccuracyCoin RAM-decode harness:
   `cargo test -p nes-test-harness --features ... --test accuracycoin`
   (crates/nes-test-harness/tests/accuracycoin.rs). Watch the DMA-cluster
   (#4 irq_and_dma family, $2002 flag timing, INC $4014, Internal Data Bus, SH*,
   Frame Counter IRQ/4-step/5-step, Controller Strobing) — these are the tests
   this fix is meant to FLIP to pass. Record before/after counts.
4. Isolated sub-test ROMs (faster signal than the full battery):
   `cargo run -p nes-test-harness --bin validate_sub_test_rom --features ... -- \
     tests/roms/AccuracyCoin/sub-tests/<name>.nes` for frame-counter-irq,
   controller-strobing, apu-reg-activation, plus the trace_* bins for diffing.
5. 60-ROM commercial oracle — MUST stay byte-identical:
   `cargo test --test external_real_games` (tests/external_real_games.rs, ROMs
   under tests/roms/external/mapper-XXX/). This runs the DEFAULT build (no
   r4-cpu-dma) → must be untouched. ALSO run it with `--features r4-cpu-dma,
   r4-dma-apu-half` to confirm the new path doesn't desync real games.
6. Full default build byte-identity:
   `cargo build` (no features) + `cargo test` default — must be unchanged since
   r4-cpu-dma and r4-dma-apu-half are non-default.

Order rationale: #1/#2 are the cheap invariants (seconds); #3/#4 the target
metric; #5/#6 the no-regression wall. Run #1-#4 on every loop iteration, #5/#6
before declaring a change complete.

---

## PART 4 — Risk / rollback

### Flag-gating (default stays byte-identical)
- `r4-cpu-dma` is NOT default (default = std, accuracycoin-sprite-eval-base-from-oamaddr,
  mc-dmc-rdy-stall, cpu-implied-dummy-reads). The default build uses legacy
  `drain_dma` (bus.rs:2426) and is completely unaffected.
- New `r4-dma-apu-half` sub-flag gates the rewritten loop. Without it, the
  existing sep_dummy loop compiles verbatim — zero risk to the current r4 path.
- `dma_get_cycle`/`dma_get_cycle_next` Bus methods get default impls (return
  false / negate) so non-Lockstep test stubs and the no_std build are unaffected.
- The legacy A/B knobs (get_parity, sep_dummy, cascade, mid_loop_dmc_poll) stay
  in r4_knobs.rs so prior sweep configs reproduce.

### Rollback
- Any single change is reversible by dropping `r4-dma-apu-half` from the test
  feature set (returns to sep_dummy loop) — no code revert needed mid-investigation.
- If a gate regresses: STOP, report the regressing test + the first diverging
  oracle row, and ASK before reverting or re-baselining. Do not silently revert.

### Guardrails (explicit)
- ASK BEFORE ROLLBACK: do not revert a landed change without confirming with the
  user which gate regressed and whether the regression is the fix exposing a
  separate latent defect (as check-then-tick did in §87).
- NO ORACLE RE-BASELINE WITHOUT VISUAL PROOF: the 60-ROM commercial oracle
  (tests/external_real_games.rs) byte-hashes must not be regenerated to "make it
  pass". If a commercial ROM's output changes under the r4 path, capture a frame
  and prove visually it is correct (or a pre-existing r4 defect) before touching
  any baseline. The default build must never change its bytes.
- The AccuracyCoin floor must not drop: record the pass count before starting;
  the fix must be net-positive (flip the DMA cluster) with zero AccuracyCoin
  regressions, matching the §157 precedent.

## Critical files for implementation
- crates/nes-cpu/src/cpu.rs (process_pending_dma 828-1124; helpers 1126-1215; struct 201-248)
- crates/nes-cpu/src/bus.rs (Bus trait DMA hooks 117-200; add dma_get_cycle[_next])
- crates/nes-core/src/bus.rs (LockstepBus DMA hooks 3952-4074; drain_dma 2426; apu tick 3898)
- crates/nes-cpu/src/r4_knobs.rs (add apu_half_get; demote sep_dummy/get_parity to A/B-only)
- crates/nes-apu/src/apu.rs (apu_phase 373; tick toggle 574; add apu_phase_next peek)
