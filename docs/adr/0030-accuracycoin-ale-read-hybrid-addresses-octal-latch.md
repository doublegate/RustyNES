# ADR 0030 — AccuracyCoin "ALE + Read" / "Hybrid Addresses": the octal-latch gap and the 2-cycle-ALE-fetch prerequisite

**Status:** Accepted — records a deferred-with-roadmap decision (keep the honest
139/141 baseline; attempt Option 2 as PR B3, and Option 1 as a separate enduring branch).
**Date:** 2026-07-08
**Author:** DoubleGate
**Related:** [ADR 0002 (IRQ-timing coordination)](0002-irq-timing-coordination.md),
[ADR 0029 (one-clock / every-cycle timebase)](0029-one-clock-every-cycle-timebase.md),
the v2.0.1 AccuracyCoin re-sync, and `to-dos/DEFERRED-AND-CARRYOVER-FEATURES.md`.

## Context

The v2.0.1 AccuracyCoin re-sync (upstream `100thCoin/AccuracyCoin` commit `71f57fb`)
grew the oracle catalog from 144 to **146 rows / 141 assigned tests**, adding the two
newest upstream PPU tests to the "PPU Misc." suite:

- **"ALE + Read"** (`$0491`) — a mid-render `LDA $2007` whose read cadence collides
  with the background fetch cadence, corrupting a background pattern fetch.
- **"Hybrid Addresses"** (`$0492`) — a mid-fetch `STA $2006` whose partial address
  update corrupts a nametable fetch.

The v2.0.0 "Timebase" core **passes 139 of the 141 assigned tests and fails these two**
(the eight pre-existing PPU-Misc tests still pass, so nothing regressed). The honest
current figure is therefore **139/141 (98.58%)**, down from 139/139 (100%) only because
the denominator grew by two hard tests. A bounded implementation attempt was made under
this ADR; it did not converge. This ADR records the hardware mechanism, why RustyNES's
current PPU model cannot reproduce it cheaply, and the concrete prerequisite for a future
attempt — so the next accuracy session starts from a plan rather than from scratch.

### The hardware mechanism (authoritative: `AccuracyCoin.asm:2541-2614, 3163-3333`; nesdev `PPU_rendering.xhtml:65,116,195-201`)

To save pins, the PPU multiplexes its lower eight VRAM address pins with the eight VRAM
data pins (PA0-7 = AD7-0). Each VRAM access therefore takes **two PPU cycles**:

1. **Cycle 1 (ALE high):** the full 14-bit address is driven; an external **octal latch**
   (74LS373-class) captures the low eight bits (A7-A0).
2. **Cycle 2 (ALE low):** the PPU drives only the high six bits (A13-A8); the octal latch
   supplies A7-A0; the data byte is read/written on AD7-0.

So every read's effective address is `{octal-latch low 8}:{current high 6}`. When those two
halves **desync**, the PPU reads from an address it never coherently output — a **hybrid
address**:

- **Hybrid Addresses:** a `$2006` second write applies its new high byte to A13-A8
  immediately, but A7-A0 still comes from the octal latch loaded on the previous cycle. In
  the test, `v` updates to `$2F00` while the latch still holds `$19` from the prior `$2C19`
  fetch, so the nametable fetch reads `$2F19` and lights a single pixel a sprite-zero hit
  detects.
- **ALE + Read:** the `$2007` PPUDATA state machine takes **three** PPU cycles while the
  background read cadence takes **two**, so they inevitably overlap. On the overlap cycle
  both ALE and Read are asserted and the octal latch enters an analog feedback loop; it is
  **not** updated to the new Pattern-Address-Register low byte and retains the stale data
  value ($FF), so the next pattern fetch reads `{new PAR high 6}:{stale low 8}` (`$0F03` →
  `$0FFF`), producing eight visible pixels over a transparent tile.

### Reference-emulator survey (`ref-proj/`)

- **Mesen2** (passes AccuracyCoin 100%) models neither a literal octal latch nor a 2-cycle
  access. Both behaviors emerge from one persistent `_ppuBusAddress` (the last address the
  fetch drove): `$2007` reads/writes during render operate on `_ppuBusAddress`, not `v`;
  `$2006` writes stage a **3-dot-delayed `v` update** and the bus is deliberately **not
  re-synced to `v` while rendering**, so bus-low and `v`-high diverge into the hybrid.
- **ares** keeps an explicit `io.busAddress` and models the `$2007` side, but does **not**
  implement the `$2006` hybrid corruption.
- **higan** simply blocks `$2007` during rendering (returns 0) and models no bus latch.

### RustyNES's current PPU model (`crates/rustynes-ppu/src/ppu.rs`)

Fetches are **single-step**: `fetch_nt`/`fetch_at`/`fetch_bg_lo`/`fetch_bg_hi` each compute
their address fresh from `v` and call `read_vram` once, at one dot of the 8-dot cadence.
There is **no persistent PPU bus-address register** and **no modeled 2-cycle ALE/read
split** — the only VRAM-side latch is `render_data_bus` (the last *data* byte). The
`$2007`-during-render machinery (`render_data_bus` + `ppudata_sm_countdown` +
`ppudata_v_inc_pending`) is what passes `$2007 Stress`, but it corrupts only the read
*buffer*, never a fetch *address*. `$2006` case-6 does an unconditional `v = t` with no
render-time hook.

## Decision

**Keep the honest 139/141 as the baseline, and pursue the fix along two tracks in v2.0.1:
Option 2 (Mesen2's model) as the low-risk PR B3, and Option 1 (the 2-cycle-ALE refactor) as
a separate enduring branch — both gated behind a default-off feature flag and the full
regression battery.** The first bounded attempt (below) is retained as the motivating record
of why the naive latch-tracking approach is insufficient.

The first bounded attempt under this ADR added an `octal_latch: u8` (fed from every `read_vram`
as `addr & 0xFF`) plus a one-shot `hybrid_fetch_addr` set by a render-time `$2006` hook and
consumed by the next `fetch_nt`. The tracking was verified inert (still 139/141, `no_std`
clean), but the hook did **not** reproduce the corruption: because fetches are single-step
and never hold the per-cycle multiplexed-bus low byte, `octal_latch` at the instant of the
`$2006` write is not the tuned value (`$19`) the test requires, so the one-shot corrupts the
wrong tile. Shipping the inert field alone would mean a save-state format bump (snapshot
v4 → v5) for an unused field — pure format churn — so the attempt was **reverted in full**
(the branch is byte-identical to the v2.0.1 AccuracyCoin re-sync).

Faithfully reproducing both behaviors requires **one** of two structural changes to what
drives the PPU bus. This is fundamentally a *fetch-model* decision — the fork proved that
merely tracking a latch byte is inert, because the corruption depends on the *address
sequence itself* at sub-cycle resolution, which the single-step model doesn't produce.

### Option 1 — 2-cycle-ALE fetch refactor (the physically-correct model)

Model each fetch as cycle-1 (drive the full address, latch PA0-7 into an octal-latch
register) + cycle-2 (read, with the octal latch supplying A7-A0).

- **Pros.** Correct once, correct forever — reproduces *every* octal-latch phenomenon (both
  tests, the ALE+Read analog feedback loop exactly, and the long tail of "unstable read
  cadence" cases) with no per-test special-casing. Self-consistent: the bus address is always
  the true multiplexed value, so `$2006`/`$2007` need no bespoke hooks, and A12/MMC3 modeling
  gets cleaner (A12 edges become real bus events). **Architecturally aligned with Timebase**
  (ADR 0029): v2.0.0 already made "every cycle is a real bus access" the core philosophy, and
  a 2-cycle bus access is the natural PPU-side expression of that — arguably more tractable
  now than under the old five-counter model.
- **Cons.** Highest regression blast radius — the fetch cadence is the calibration point for
  sprite-zero-hit dots, MMC3 A12 IRQ timing (`mmc3_test_2`), MMC5 scanline detection,
  shift-register reload, and the 60-ROM byte-identity oracle; a shifted A12 edge silently
  breaks an IRQ test. Biggest, hardest-to-review, hardest-to-bisect diff. Snapshot format
  churn. Needs explicit perf attention against the ≤2 ms/frame budget (work is redistributed
  across the two dots the emulator already ticks, so not necessarily slower — but must be
  measured).

### Option 2 — Mesen2's model (persistent `_ppuBusAddress` + 3-dot-delayed `v`)

A persistent `_ppuBusAddress` (the last address the fetch drove), a **3-dot-delayed `v`**
update on `$2006`, and the bus deliberately **not** re-synced to `v` during rendering.

- **Pros.** Proven-correct — Mesen2 passes AccuracyCoin 100% with exactly this (a known-good
  recipe, not a hypothesis). Meaningfully less invasive: keeps single-step fetches, adds a
  `bus_addr` register fed by each fetch, reroutes `$2007`/`$2006` through it, and adds a small
  delayed-`v` state machine — idioms that already exist in RustyNES (`ppudata_sm_countdown`,
  `render_data_bus`). Lower perf risk.
- **Cons.** It is an *approximation* — the octal-latch feedback is modeled implicitly via
  bus-address retention rather than the true analog loop; sufficient for these tests but a
  future test outside its coverage could need another patch. **The real trap is
  reconciliation**: RustyNES built its *own* `$2007`-Stress-passing machinery (the
  `render_data_bus` latch, the countdown, and the v-glitch increment) that differs from
  Mesen2's (`_ignoreVramRead`, deferred increment, read-from-`_ppuBusAddress`); porting
  Mesen2's model likely means *replacing*
  RustyNES's existing `$2007` path, which risks the currently-*passing* `$2007 Stress` /
  `$2004 Stress`. Delaying `$2006`'s `v = t` by 3 dots during render can also shift
  scroll-write timing other tests depend on.

### Recommendation and plan (v2.0.1)

**Attempt Option 2 first as the low-risk fix (PR B3), then attempt Option 1 on a separate
long-term branch — regardless of whether Option 2 succeeds** (per the maintainer's directive):

1. **Option 2 is the practical path to 141/141** — proven, bounded, lower regression risk.
   Land **Hybrid Addresses first** (the `$2006` side is cleaner and self-contained), then
   ALE+Read. The **ALE + Read** side is strictly harder: on top of the octal latch it needs
   the 3-cycle `$2007` state machine's ALE to overlap the 2-cycle background cadence at the
   precise corruption dot.
2. **Option 2's work is not wasted if Option 1 is later chosen** — the persistent `bus_addr`
   register is a *prerequisite for both*, so Option 2 is a legitimate stepping stone.
3. **Option 1 is the enduring, physically-correct fix** and is worth pursuing on its own
   branch as a deliberate "exhaustive PPU-bus accuracy" campaign (scoped and gated like the
   Timebase rewrite), not merely to pass two tests. It supersedes Option 2 if it lands.

**Non-negotiable guardrails for either path** (this is where the 139 is protected):

- Land it **behind a default-off feature flag** first (the *Feature-Flag Additive Change*
  pattern), bake it, then promote — never flip the fetch model in one shot.
- Gate every change on the **full 141 AccuracyCoin**, **nestest 0-diff**, **blargg/kevtris**,
  the **MMC3 IRQ suite** (A12 timing is fetch-address-derived — the most likely silent
  breakage), **sprite-zero-hit** tests, the **60-ROM commercial byte-identity oracle**, and
  the **≤2 ms/frame** perf budget.
- Use the vendored `ref-proj/Mesen2` (already carrying RustyNES oracle-logging hooks) as a
  **per-cycle bus-stream cross-diff oracle**, not just a pass/fail check.

## Consequences

- **Positive:** the headline metric is honest (139/141), the two gaps are precisely
  characterized with an authoritative mechanism, a reference-emulator survey, and two vetted
  implementation paths with a full pros/cons analysis — so both attempts (PR B3 = Option 2,
  the Option 1 branch) start from a plan rather than re-deriving it.
- **Negative:** RustyNES's public AccuracyCoin figure is 98.58% until one of the two
  fetch-path tracks lands; both are invasive, high-regression-risk, and may not converge
  within a bounded attempt. Both are guarded behind a default-off feature flag so a
  non-converging attempt never regresses the shipped build.
- **Follow-up:** tracked in `to-dos/DEFERRED-AND-CARRYOVER-FEATURES.md` ("Pass the two new
  AccuracyCoin PPU tests"). Sequencing: **Option 2 (PR B3) first** — Hybrid Addresses before
  ALE+Read — then the **Option 1 branch** as the enduring physically-correct model. This ADR
  is updated (or superseded) when either track lands and the figure returns to 141/141.

## Update — 2026-07-08 (both bounded v2.0.1 attempts made; neither converged)

Both v2.0.1 tracks laid out in the Decision were attempted under a bounded effort, and
**neither converged** — the honest **139/141 (98.58%)** figure stands as the v2.0.1 baseline:

- **Option 2 — Mesen2 persistent-bus-address model.** Landed as **PR #234** behind the
  default-off feature flag `mc-ppu-bus-addr-hybrid` (`rustynes-ppu` → forwarded through
  `rustynes-core` and `rustynes-test-harness`). It adds the persistent `bus_addr` register
  and the render-time `$2006`/`$2007` reroute, but flag-**on** it still reaches only **139/141**
  (both "ALE + Read" `$0491` and "Hybrid Addresses" `$0492` continue to fail): the implicit
  bus-address retention does not reproduce the sub-cycle octal-latch sequence the two tests
  require without also disturbing the currently-passing `$2007 Stress` path.
- **Option 1 — 2-cycle-ALE fetch refactor.** Attempted as **draft PR #236** on branch
  `feat/v2.0.1-option1-2cycle-ale`. The physically-correct two-dot bus model likewise reaches
  only **139/141** flag-on within the bounded attempt — the fetch-cadence recalibration needed
  to light exactly the right pixels without shifting sprite-zero / A12 / MMC3 timing is not
  achievable as a bounded fork.

In **both** cases the flag-**OFF** (shipped) build is **byte-identical** to the v2.0.1
AccuracyCoin re-sync baseline — **139/141, nestest 0-diff, AccuracyCoin otherwise 100%** —
so the non-converging experiments never touch the shipped core. This empirically **confirms
this ADR's central thesis**: the fix is not reachable by a bounded fork of either shape; it
needs a **dedicated Timebase-scale campaign** that models the per-cycle PPU bus *inside* the
one-clock scheduler (ADR 0029), calibrated against the vendored `ref-proj/Mesen2` per-cycle
bus-stream cross-diff oracle and gated on the full regression battery. Until that campaign is
scheduled, **139/141 is the honest v2.0.1 baseline** and both flags remain default-off
experiments. The two draft branches are retained as the starting point for that campaign.

## Update — 2026-07-08 (v2.0.2 octal-latch campaign CONVERGED — 141/141 flag-on)

The dedicated campaign this ADR called for landed on branch
`feat/v2.0.2-octal-latch-campaign`, and **both tests now pass flag-on: AccuracyCoin
141/141 (100.00%)**, with the flag-**off** shipped build still **byte-identical** at
139/141. Two corrections to the plan above, both empirical:

1. **The oracle was wrong.** The per-cycle bus cross-diff proved the vendored **Mesen2
   build does NOT pass these two tests** (both result bytes read `0x0A` = corruption not
   reproduced), so "Option 2 = proven-correct Mesen2 recipe" was false. The correct oracle
   is **TriCNES** (`ref-proj/TriCNES/Emulator.cs`, MIT, commit `9199870` — the AccuracyCoin
   author's own emulator), which models the multiplexed AD/A bus + octal latch at transistor
   level and does drive `$2F19` / `$0FFF`. The campaign audit
   (`docs/audit/v2.0.2-octal-latch-campaign-2026-07-08.md`) records the decisive finding.

2. **A whole-dot port of TriCNES's octal latch suffices** — the full 2-cycle-ALE fetch
   refactor was not required. Behind the existing `mc-ppu-bus-addr-hybrid` flag, the reworked
   model adds an 8-bit `octal_latch`, a 14-bit `address_bus`, and a `copy_v`/`pattern_latch_stale`
   pair (all cfg-gated). Every background fetch resolves its effective VRAM address through
   `octal_effective` (the TriCNES `FetchPPU` splice `(address_bus & 0x3F00) | octal_latch`),
   which is transparent (returns the intended address, reloads the latch) except on the two
   modeled corruption events:
   - **ALE + Read:** the `$2007`-read PPUDATA state-machine countdown landing during render
     freezes `octal_latch` on the read's DATA byte; the next pattern fetch reads
     `{PAR high 6}:{stale $FF}` = `$0FFF`. Verified by a per-dot trace: `$2007`R @ sl3 dot223
     → latch=$FF @ dot228 → pattern read `$0FFF` @ dot230.
   - **Hybrid Addresses:** a `$2006` second write during render sets `copy_v` and captures the
     stale octal-latch low byte (the NT low of the fetch that consumes it, one coarse-X past
     the in-flight tile in RustyNES's whole-dot cadence); the next nametable fetch splices
     `{new v high $2F00}:{stale $19}` = `$2F19`. Verified: W2006 @ sl4 dot182 → hybrid NT read
     `$2F19` @ dot186.

   A12/MMC3 notification stays on the INTENDED (un-spliced) fetch address at every call site,
   so fetch-address/A12 timing is unchanged. Regression battery flag-on: **nestest 0-diff,
   mmc3 (A12 clocking + IRQ) all pass, ppu_sprites 19/19, mmc1_a12 pass**; flag-off:
   byte-identical (accuracycoin 139/141, nestest 0-diff, mmc3, ppu_sprites all pass).

   No snapshot-format bump was needed: `octal_latch`/`address_bus` self-heal on the next
   in-blanking fetch ALE, and `copy_v`/`pattern_latch_stale` are transient one-shots consumed
   within a few dots — none carries meaningful cross-save state, so `PPU_SNAPSHOT_VERSION`
   stays at 4. The `+1 coarse-X` latch capture is a documented approximation of the whole-dot
   cadence (the corrupted NT fetch is one tile past the write's in-flight tile); it is exact
   for the tested alignment and gated entirely behind the default-off flag.

**Decision — land flag-off in v2.0.2, refine-then-promote in v2.0.3 (maintainer, 2026-07-08).**
The 60-ROM commercial byte-identity oracle (this ADR's mandated promote-safety gate) is
maintainer-run and could not be exercised at implementation time (no local dumps), and the
Hybrid path carries the `+1 coarse-X` approximation above. Per the *Feature-Flag Additive
Change* + "bake, then promote" guardrails, the model **ships behind the default-off flag in
v2.0.2** — the shipped default stays the honest **139/141**, with the experimental
`mc-ppu-bus-addr-hybrid` model verified at **141/141 flag-on**. **v2.0.3** reworks the Hybrid
path from the `+1 coarse-X` reconstruction to a **first-principles latch-carry model** (the
`octal_latch` naturally holding the stale low byte across dots, per TriCNES's ALE-driven latch,
rather than reconstructing it from `v` at the `$2006` write) so it is exact across BOTH test
alignments and generalizes; then, gated on the **60-ROM commercial oracle** (flag-on) + broader
`$2007`/`$2006`-during-render title validation + a CI job asserting flag-on 141/141, the flag is
**promoted to default** (shipped AccuracyCoin 141/141), weighing the ADR 0028 save-state /
byte-identity implication at that point. This ADR is updated (not superseded) when promotion
lands.

## Update — 2026-07-08 (first-principles refine proven infeasible at whole-dot resolution → 2-cycle-ALE campaign)

The bounded v2.0.3 first-principles attempt (make `octal_latch` *naturally* carry the stale byte,
deleting the `+1 coarse-X` reconstruction) was made and **instrumented against the TriCNES oracle
— it does not converge at RustyNES's whole-dot (single-step) fetch cadence.** The measured barrier:

- On the Hybrid test at the `$2006` second write (frame 5110, **scanline 4, dot 182**),
  `v & 0xFF = $18` (coarse-X 24). The corruption needs the latch to hold **`$19`** (coarse-X **25**,
  the NT low of `$2C19`) so the next fetch splices `$2F00 | $19 = $2F19`.
- Two first-principles variants both fail: latch loaded at every fetch ALE → holds a pattern byte
  `$44` → `$2F44` (140/141); latch loaded only at NT ALE → holds `$18` → `$2F18`, off by one
  coarse-X (140/141). RustyNES collapses each access into ONE dot and runs `inc_hori` at phase 7,
  so the latch can **never** naturally reflect coarse-X > the current `v`'s (24) — it cannot carry
  the one-tile-ahead `$19`.
- **TriCNES gets `$19` naturally because it models each fetch as a 2-DOT access**: the even-dot ALE
  drives the PAR and loads `PPU_OctalLatch = (byte)PPU_AddressBus`, the odd-dot read uses
  `(PAR & 0xFF00) | OctalLatch` (`Emulator.cs:153`), and the delayed-`CopyV` countdown
  (`Emulator.cs:1684-1704`) lets the coarse-X-25 NT ALE fire (with `IncrementScrollX` ordering)
  *before* the `$2006` update lands. That even-ALE/odd-read split **is** the full 2-cycle-ALE fetch
  refactor.

So the v2.0.2 `+1 coarse-X` reconstruction is a **faithful whole-dot stand-in** for exactly this
2-cycle-ALE artifact — it is not reachable "more correctly" without the refactor itself.

**Decision (maintainer, 2026-07-08): do the full 2-cycle-ALE fetch refactor** as a dedicated
Timebase-scale campaign spanning **v2.0.3 → v2.0.4** — model each background/`$2007`/`$2006` VRAM
access as a 2-dot transaction (even-dot ALE drives the address + loads the octal latch; odd-dot
read splices `(bus_high) | octal_latch` + writes the DATA back to the low bus) with the
delayed-`CopyV` countdown, behind a default-off flag, recalibrating every fetch-cadence-derived
timing (sprite-zero-hit dots, MMC3 A12 IRQ, MMC5 scanline, BG shift reload), gated at each phase
on the full battery (141 AccuracyCoin + nestest 0-diff + blargg/kevtris + mmc3_test_2 +
sprite-zero + 60-ROM byte-identity + ≤2 ms/frame) — then promote to default (shipped 141/141).
This mirrors the
v2.0.0 "Timebase" beta-train ceremony; plan in `to-dos/plans/v2.0.3-2cycle-ale-plan.md`.

## Update — 2026-07-08 (v2.0.3 Phases 2+3 CONVERGED — 141/141 flag-on, fully natural, no reconstruction)

The 2-cycle-ALE campaign's Phase 2 (NT true two-dot fetch) + Phase 3 (both corruptions arising
naturally) landed on branch `feat/v2.0.3-2cycle-ale-campaign` behind the default-off
`mc-ppu-2cycle-ale` flag, and **both AccuracyCoin PPU tests now pass flag-on with NO
reconstruction: AccuracyCoin 141/141 (100.00%)**, flag-**off** still byte-identical at 139/141.
The `+1 coarse-X` stand-in is gone — the octal latch carries the stale bytes on its own.

**How the natural model works (all `#[cfg(feature = "mc-ppu-2cycle-ale")]`-gated):**

- **NT true two-dot fetch (Phase 2).** A new phase-0 nametable ALE (`ale_drive_nt`) drives the
  PLAIN `0x2000 | (v & 0x0FFF)` address and loads `octal_latch` with its low byte one dot before
  the phase-1 read; the read (`fetch_nt`) resolves through the armed `ale_splice`
  `(address_bus & 0x3F00) | octal_latch`. The MMC5 vertical-split query stays at the read dot
  (its `split_chr_bank_latch` side effect is mapper-observable); when it turns out split-active,
  `fetch_nt` disarms the phase-0 ALE and reads the synthesized `split.nt_addr` co-located, so
  split rendering is byte-identical (no non-mutating peek was needed — deferring the *correction*
  to the read dot achieves the same end without a new trait method).
- **Hybrid Addresses (Phase 3) — the delayed-`CopyV` countdown** (`copy_v_delay`, `TriCNES`
  `PPU_Update2006Delay`). A `$2006` second write during the active BG-fetch window does NOT copy
  `t -> v` immediately; it stages a `COPY_V_DELAY = 4`-dot countdown (RustyNES's fixed CPU/PPU
  alignment corresponds to TriCNES's delay-4 case; the AccuracyCoin ROM's `.word` retries cover
  its two answer-key alignments and RustyNES's fixed one hits). While the countdown runs, the
  fetch cadence advances coarse-X (`inc_hori_v` at phase 7) and the per-group phase-0 NT ALE keeps
  loading the CURRENT (pre-copy) `v`'s NT-low, so by the landing `octal_latch` NATURALLY holds the
  one-tile-ahead `$19`; the landing sets `address_bus = v` (= new `t`, high `$2F00`) and the next
  NT read splices `$2F00 | $19 = $2F19`. **Verified natural** by the octal trace:
  `W2006 @ sl4 dot182 → HYBRID $2F19 @ dot186` (delay exactly 4). The deferral is gated to the
  active-fetch window (visible dots 1..=256 + the 321..=336 prefetch) — the only region where a
  BG fetch can consume the stale latch — so HBlank-window scroll writes are unaffected.
- **ALE + Read (Phase 3)** reuses the existing `render_data_bus`/`ppudata_sm_countdown` machinery:
  at the PPUDATA state-machine landing during render the latch freezes on the read's DATA byte
  (`pattern_latch_stale`), and `drive_bus` suppresses the next pattern ALE's latch reload so the
  pattern read splices `(PAR high 6):(stale $FF) = $0FFF` — no explicit reconstruction, just the
  natural multiplexed-bus splice. **Verified:** `SMLAND $FF @ sl3 dot228 → STALE $0FFF @ dot230`.

A12/mapper notification stays on the INTENDED (un-spliced) address at every call site; the only
A12 timing change is the `$2006`-write edge itself, which the delayed copy shifts by 4 dots during
render (rare; verified inert on the MMC3/MMC1 A12 suites). No snapshot-format bump (`address_bus`
/`ale_armed`/`octal_latch`/`copy_v_delay`/`pattern_latch_stale` all self-heal within a scanline;
`PPU_SNAPSHOT_VERSION` stays at 4).

**Regression battery (flag-on):** nestest 0-diff; mmc3 A12-IRQ suite 18/18; ppu_sprites (sprite-
zero) all pass; mmc1_a12 pass; nes_blargg all pass; `cargo fmt` + `clippy -D warnings` clean for
the default, `mc-ppu-bus-addr-hybrid`, and `mc-ppu-2cycle-ale` configs; `no_std` cross-compile
clean. **Flag-off:** byte-identical (AccuracyCoin 139/141, nestest 0-diff).

**60-ROM commercial byte-identity oracle (flag-on): 58/60 byte-identical.** Exactly two titles
change, and BOTH legitimately perform `$2006`-during-active-render and differ ONLY in the
framebuffer (audio FNV + CPU-cycle count byte-identical — a pure PPU-internal timing shift, not a
divergent code path):

- **Super Mario Bros. 3 (MMC3):** 8 pixels, all on scanline 194 columns 0-7 (a single leftmost
  tile).
- **Uchuu Keibitai SDF (MMC5):** 7 pixels on scanline 15 columns 152-158 (one tile at the split
  boundary).

Each is a single-tile sub-scanline shift at the exact scanline of a mid-render `$2006` scroll
write — the precise artifact the 2-cycle-ALE model is designed to reproduce, and MORE
TriCNES-faithful than the flag-off immediate-`v=t` approximation (TriCNES delays `v=t` by 4-5 dots
unconditionally). The committed `insta` snapshots are LEFT at the flag-off values so the shipped
build stays byte-identical; the two flag-on diffs are documented exceptions to be re-blessed (or
made feature-conditional) by the maintainer at the **Phase-4 promotion** gate alongside the perf
check and a CI job asserting flag-on 141/141. This ADR is updated (not superseded) at promotion.
