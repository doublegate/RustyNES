# Plan — Remediate all 9 remaining AccuracyCoin fails (RustyNES v2, `feat/v2.0-from-main`)

## Context

`feat/v2.0-from-main` (worktree `/home/parobek/Code/Commercial_Private-Projects/RustyNES_v2-from-main`)
is the "additive pivot" off `main` (v1.7.0): it keeps main's **working lockstep DMA cluster**
(which the abandoned `refactor/v2.0-master-clock` rewrite regressed) and has reached **93.53%
AccuracyCoin (9 fail)** via two oracle-validated PPU features. The goal now is to remediate
**all 9** remaining fails. Read-only research this session (Plan agents over Mesen2, ares,
TetaNES, TriCNES `Emulator.cs`, nesdev_wiki, perfect6502/phantom2c02, the full branch history,
the memory files, and in-code comments) established a precise, tiered closability map. The
overarching constraint: **never re-break the DMA cluster** (the pivot's whole reason to exist)
— honor `feedback_adr0002_ask_before_rollback` + `feedback_paired_pipeline_oracle_masking`.

The 9 fails (AccuracyCoin RAM-direct): NMI Overlap BRK[2], NMI Overlap IRQ[1], Interrupt flag
latency[11], Delta Modulation Channel[21], APU Register Activation[6], $2002 flag timing[1],
BG Serial In[2], $2007 Stress[2], Implied Dummy Reads[3].

**Research verdict (closability tiers):**
- **Tier 1 (additive, lockstep-closable, DMA-safe):** $2002 flag timing, Delta Modulation
  Channel, NMI Overlap BRK.
- **Tier 2 (master-clock / cycle-exact DMC-DMA — the deferred multi-week core):** NMI Overlap
  IRQ, Interrupt flag latency, Implied Dummy Reads, APU Register Activation.
- **Tier 3 (bleeding-edge; no emulator passes; needs `mc-ppu-subpos`):** BG Serial In, $2007
  Stress. **Decision: chase via `mc-ppu-subpos`** (knowing $2007 likely plateaus at 166/170).

Execute the tiers in order. Tier 1 is independent and bankable now. Tiers 2+3 both ride the
master-clock substrate, so **Tier 2's prerequisite (the DMA-on-master-clock integration) is the
make-or-break of the entire "all 9" goal** and the bulk of the effort.

**Use the references throughout execution** (not just for the plan): pin each behavior against
the AccuracyCoin asm (`/tmp/RustyNES_v2/AccuracyCoin.asm`) + nesdev_wiki + the matching exemplar,
and validate sub-dot behavior against perfect6502/phantom2c02 (`/tmp/RustyNES_v2/{perfect6502,
phantom2c02}/`). The branch `refactor/v2.0-master-clock` already contains turn-key oracle scripts,
audit docs, and a working (DMA-broken) master-clock substrate to port FROM.

---

## Tier 1 — additive lockstep fixes (target 93.53% → ~95–96%, DMA stays green)

Each is a localized, feature-gated change validated like the prior PPU ports: default byte-identical,
60-ROM oracle 60/60, DMA cluster green, all canaries green, then promote to default.

### T1.1 — `$2002 flag timing` [error 1]  (PPU-only)
The test walks four `$2002` reads across the pre-render dot-1 flag-clear; on hardware a `$2002`
read latches **VBL (bit7) at read-start (M2 high)** but samples **sprite-0 (bit6) / overflow
(bit5) ~1.875 PPU dots later at read-end (M2 low)**. main collapses all three to a single
cycle-start sample (`crates/nes-ppu/src/ppu.rs` `cpu_read_register` $2002 arm ~`:749-752`).
**Fix (TriCNES model, `Emulator.cs:939-947` `EmulateUntilEndOfRead` + `:9181-9192`):** behind a
new `ppu-2002-read-end-flags` feature, in the $2002 arm sample+clear VBL first, advance the PPU
~2 dots (via the bus adapter), then sample bits 6/5 — and suppress the cycle's subsequent lockstep
dot-advance so the PPU is not double-advanced (the read "owns" its sub-dot advance). Note: memory's
"$2002 = OAM-span-downstream" verdict was on the master-clock core; on lockstep the two-point
intra-read sample is the correct, DMA-free lever (memory `project_r1_dma_regression.md:59` records
it passed on legacy lockstep before).

### T1.2 — `Delta Modulation Channel` [error 21, fails Test L]  (APU-only)
Test L places a `$4015` DMC-enable write 0/1/2 cycles before the DMC timer hits 0 and checks the
resulting 3-vs-4-cycle DMA delay. main derives the enable delay from the binary `apu_phase`
(`crates/nes-apu/src/apu.rs` `write_status` ~`:667`: `if apu_phase {4} else {3}`), which cannot
resolve the sub-cycle timer phase. **Fix (ares model, `ares/fc/apu/dmc.cpp:7` `dmaDelayCounter =
periodCounter & 1 ? 2 : 3`):** key the enable delay on the **live DMC timer parity** (`self.dmc.timer
& 1`) and model the halt-retry (DMA waits until DMC is actually enabled). The lockstep DMC timer is
rigidly CPU-cycle-coupled (one APU tick/cycle), so the timer value is coherent at write time —
unlike the master-clock core where this lever destabilized. Confine to `write_status` /
`tick_with_external` / `service_dmc_dma`; re-verify the DMA+$4015 / DMC-Bus-Conflicts cluster.

### T1.3 — `NMI Overlap BRK` [error 2]  (CPU-only)
BRK needs no DMC arming (unlike NMI Overlap IRQ), so this is the one C1-trio test closable on the
lockstep. **Fix (Mesen2 `NesCpu.cpp:445-464` BRK + `:531-551` EndCpuCycle; TriCNES `Emulator.cs:4392-4488`):**
port the canonical recognition state-machine into main's existing per-cycle hooks — the
`mc_need_nmi/mc_prev_need_nmi/mc_run_irq/mc_prev_run_irq` φ2-sample-and-delay latches, unified
`prev_*` dispatch with the I-mask set **before** the 7-cycle service, the cycle-5 vector chosen from
the **live** `need_nmi`, and the BRK `prev_need_nmi` clear (`NesCpu.cpp:463` "needed for nmi_and_brk").
This replaces main's `nmi_first_tick`-window approximation (`crates/nes-cpu/src/cpu.rs`
`service_interrupt` ~`:699-714`). **Risk:** the M2-high vs M2-low sample position is coupled to
blargg #4/#5 on the lockstep (memory `project_c1_trace_loop_ceiling.md` §29) — keep the M2-high
(T_last-1) sample, change only the dispatch/hijack/deferral logic, and **re-run the full
`cpu_interrupts_v2` 1–5 gauntlet** to confirm no trade-off regression.

---

## Tier 2 — the master-clock core (closes NMI Overlap IRQ, flag latency, Implied Dummy Reads,
APU Register Activation). This is the deferred multi-week axis; its gate is keeping the DMA green.

**Why these 4 are coupled:** all need **cycle-exact DMC-DMA scheduling** (the DMC arm/halt landing
on the exact CPU read it conflicts with) — NMI Overlap IRQ's IRQ is armed by a DMC-DMA; flag latency
needs the level detector lowered on a specific cycle; Implied Dummy Reads needs the cycle-2 dummy
read to clear `$4015` on the DMA-relative cycle (and turning the existing `cpu-implied-dummy-reads`
on regresses Implicit DMA Abort without this); APU Register Activation needs the per-cycle OAM↔DMC
get/put interleave + DMC-databus-conflict. The master-clock substrate closes them, but on the
abandoned branch the same substrate (`r4-cpu-dma`+`mc-apu-subcycle`) **regressed the DMA cluster**.

**T2.0 — PREREQUISITE (the crux): DMA-on-master-clock integration.** Make the master-clock
substrate keep the DMA cluster green. This is the existing **Phase T** work — see the committed
plan `docs/audit/v2.0-apu-dmc-master-clock-integration-plan-2026-06-03.md` (on
`refactor/v2.0-master-clock`) + the strategic-pivot doc + memory `project_t_track_dmc_integration`
/ `project_r1_dma_regression`. T-0/T-1 (DMC byte-timer on the integrated counter) are done there;
the unsolved core is the **cumulative per-fetch DMC span coherence** so `DMASync_50CyclesRemaining`'s
exact-cycle DMA positioning holds. This is the 17+-rollback axis — port the master-clock substrate
onto `feat/v2.0-from-main` behind features, and do NOT promote anything until the 60-ROM oracle +
the full DMA cluster are green. **Use TriCNES `Emulator.cs` (the answer-key author's DMA model),
ares `fc/cpu/timing.cpp::dma()` (the single `oddCycle` get/put loop), Mesen2 `ProcessPendingDma`
(`NesCpu.cpp:577+`, `enableInternalRegReads`), and nesdev `DMA.xhtml`/`APU_DMC.xhtml` as the spec.**

**T2.1 — IRQ recognition + R2 PPU-event timing on the master-clock substrate.** Once T2.0 holds
the DMA, the recognition-model port (T1.3, already separable per the research) + R2 "on-time
VBL/NMI/$2002" PPU-event timing close NMI Overlap IRQ + flag latency. The recognition logic is
CPU-only; R2's PPU-event dots ride the master-clock sub-dot substrate (the integer-dot ceiling that
blocks them on pure lockstep — memory §18). References: the branch's R1/R2 commits (`git -C
/home/parobek/Code/Commercial_Private-Projects/RustyNES_v2 log main..refactor/v2.0-master-clock`),
`project_c1_trace_loop_ceiling.md`.

**T2.2 — Implied Dummy Reads + APU Register Activation.** With cycle-exact DMC-DMA: enable
`cpu-implied-dummy-reads` (now safe — the cycle-2 dummy read lands DMA-coherently and no longer
regresses Implicit DMA Abort), and port the per-cycle OAM↔DMC get/put interleave with
DMC-databus-conflict ($4001-mirror alignment) into the DMA path (`crates/nes-core/src/bus.rs`
`service_dmc_dma_during_oam` / `drain_dma` — currently a bus-internal burst with the documented
v1.2 scope boundary). HIGH blast radius on the DMA cluster — gate, oracle-validate, ASK before any
drain rewrite.

---

## Tier 3 — `mc-ppu-subpos` for the bleeding-edge 2 (BG Serial In, $2007 Stress)

Both need the **master-clock PPU sub-cycle position** (φ1/φ2), so they ride Tier 2's substrate.
No emulator passes either (verified: Mesen2/ares/TetaNES all lack the BG serial-in `|1` bit and use
naive v-address `$2007` reads); the branch's silicon-faithful mechanism still fails on CPU-write-dot
alignment, and $2007 plateaus at **166/170** (4 residuals need a transistor-faithful half-PPU-clock
D-latch). **Decision: chase anyway**, with eyes open to the 166/170 ceiling.

- **T3.1 — `mc-ppu-subpos`:** implement the master-clock φ1/φ2 PPU sub-position so a CPU `$2001`/`$2007`
  access commits at its true sub-dot. This is the shared blocker.
- **T3.2 — BG Serial In:** port the serial-in bit (`shift_bg`: `bg_shift_hi = (<<1)|1`, reload masks
  `&0xFF00`) — already on the branch — and rely on T3.1 for the `dot%8`-precise `$2001`-toggle
  landing. Validate against `phantom2c02/bg_toggle.mjs` + `docs/audit/v2.0-visual2c02-bg-shifter-groundtruth-*.md`.
- **T3.3 — $2007 Stress:** port `ppu-2007-read-buffer` (branch `56cd72e`) + build the transistor-faithful
  PPU-DATA ALE/Read D-latch state machine (asm circuit `AccuracyCoin.asm:2622-2700`) driven per
  half-PPU-clock. Validate against `phantom2c02/read2007_v2.mjs` + the pinned residual-4 types
  (dots 183/255/335/339) in `docs/audit/v2.0-2007-stress-ppudata-*.md`. Accept 166/170 if the
  residual 4 don't close.

---

## Critical files (by tier)

- **T1.1 / T3.2 / T3.3 (PPU):** `crates/nes-ppu/src/ppu.rs` — `cpu_read_register` ($2002 arm
  ~749, $2007 arm), `shift_bg` (~1551), `reload_bg_shift_regs` (~1581), `fetch_sprite_tile`,
  the `tick`/`rendering_enabled_delayed` gating. Cargo feature wiring in `crates/nes-ppu/Cargo.toml`
  → `nes-core` → `nes-test-harness` (the established 3-crate forward pattern).
- **T1.2 / T2.x (APU):** `crates/nes-apu/src/apu.rs` (`write_status` ~644-687, `tick_with_external`
  ~505-573), `crates/nes-apu/src/dmc.rs` (`clock_timer` ~201, the live `timer` field).
- **T1.3 / T2.1 (CPU):** `crates/nes-cpu/src/cpu.rs` (`step`/dispatch ~280-345,
  `promote_post_step_interrupts` ~353-387, `idle_tick`/`read1`/`write1` ~449-594,
  `service_interrupt` ~666-726, `implied_dummy_read` ~517-524). Port FROM
  `git show refactor/v2.0-master-clock:crates/nes-cpu/src/cpu.rs` (`handle_interrupts` ~686-719,
  dispatch ~493-518, start/end_cycle ~761-814).
- **T2.0 / T2.2 (DMA core):** `crates/nes-core/src/bus.rs` (`service_dmc_dma` ~1592,
  `service_dmc_dma_during_oam` ~1782, `drain_dma` ~1420, the master-clock `run_apu_to`/`cpu_clock`
  paths). The whole Phase T plan + scaffold lives on `refactor/v2.0-master-clock`.

## Reference exemplars to use during execution (read-only)

- TriCNES `/tmp/TriCNES/Emulator.cs` (answer-key spec: interrupts 4183-4488, $2002 9181-9192 +
  `EmulateUntilEndOfRead` 939-947, DMC L/M/N 9504-9548, DMA/OAM 9500-9548).
- Mesen2 `/home/parobek/Code/OSS_Public-Projects/RustyNES/ref-proj/Mesen2/Core/NES/` (`NesCpu.cpp`
  IRQ/BRK/EndCpuCycle/ProcessPendingDma; `NesPpu.cpp` status/$2007/shifters; `APU/DeltaModulationChannel.h`).
- ares `.../ref-proj/ares/ares/fc/` (`apu/dmc.cpp`, `cpu/timing.cpp::dma()`, `ppu/`).
- TetaNES `.../ref-proj/tetanes/tetanes-core/src/` (`cpu.rs`, `apu/`, `ppu.rs`).
- nesdev_wiki/ (CPU_interrupts, CPU_interrupt_quirks, Interrupt_Hijacking, DMA, APU_DMC,
  PPU_registers/rendering). Hardware sims `/tmp/RustyNES_v2/{perfect6502,phantom2c02}/`.

## Verification (after every step — the hard invariants)

Run on `feat/v2.0-from-main`. Default build byte-identical until a feature is oracle-validated +
promoted. After each change:
1. **DMA cluster green** — the non-negotiable: `cargo test --release -p nes-test-harness
   --features test-roms[,FEATURE] accuracycoin_pass_rate_meets_floor -- --nocapture`; confirm no
   DMA/Open Bus/DMC/SH* in the fail list and the AccuracyCoin fail count is **non-increasing**.
2. **60-ROM commercial oracle byte-identical** (the masking guard) — symlink the ROMs
   (`ln -s /home/parobek/Code/Commercial_Private-Projects/RustyNES_v2/tests/roms/external
   tests/roms/external`), run `--features test-roms,commercial-roms[,FEATURE] --test
   external_real_games` → must be **60/60**; remove the symlink after.
3. **Canaries:** `cpu_interrupts_v2` (1–5; Tier-1/2 must not regress), `ppu_vbl_nmi` 10/10,
   `nes-ppu` 41/41, `visual_regression` 7/7.
4. **Quality gates:** `cargo fmt --all --check`, `cargo clippy --workspace --all-targets -- -D
   warnings`, no_std (`cargo build -p nes-core --no-default-features --target
   thumbv7em-none-eabihf`), `--no-default-features` builds.
5. **Promotion:** flip a feature to `nes-core` default ONLY after 1–4 pass with it on; update
   CHANGELOG `[Unreleased]` + the pivot port-log; commit; push.

**Honest ceiling:** Tier 1 → ~95–96% (DMA-safe, achievable now). Tier 2 is gated on solving the
DMA-on-master-clock (the genuine multi-week core; ASK before promoting anything that touches the
DMA). Tier 3's $2007 may cap at 166/170 → "100% on the chosen 2A03/2C02 revision modulo the
documented bleeding-edge residual," matching how TriCNES/Mesen scope it.
