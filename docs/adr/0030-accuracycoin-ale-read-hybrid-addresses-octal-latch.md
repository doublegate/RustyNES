# ADR 0030 — AccuracyCoin "ALE + Read" / "Hybrid Addresses": the octal-latch gap and the 2-cycle-ALE-fetch prerequisite

- Status: Accepted (records a deferred-with-roadmap decision)
- Date: 2026-07-08
- Deciders: DoubleGate
- Supersedes / Superseded-by: —
- Related: [ADR 0002 (IRQ-timing coordination)](0002-irq-timing-coordination.md),
  [ADR 0029 (one-clock / every-cycle timebase)](0029-one-clock-every-cycle-timebase.md),
  the v2.0.1 AccuracyCoin re-sync, `to-dos/DEFERRED-AND-CARRYOVER-FEATURES.md`.

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
