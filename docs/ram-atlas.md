# RAM Atlas

> **Status:** the classifier and the panel are implemented (v2.3.6). Export paths
> and per-game persistence are **not** — see
> [Deliberately not implemented](#deliberately-not-implemented). This document is
> the spec, so it is updated in the same change as the code it describes.

## What the feature answers

**What is each byte of the 2 KiB work RAM for?**

Not "which addresses currently hold 42" — every emulator answers that, RustyNES
included. The unanswered question is what an address *is*, and the RAM Atlas
answers it in two steps with deliberately different confidence:

1. **How did it behave?** Untouched, ticking every frame, counting up, counting
   down, changing rarely, or churning. This is a **hypothesis**.
2. **Does changing it matter?** Poke the byte, re-simulate the same interval, and
   see whether the observed output differs. This is a **fact**, and a narrow one.

## Why this is not just wiring up existing panels

RustyNES already ships the pieces a RAM tool is usually built from:

| Existing surface | What it gives | What it cannot say |
|---|---|---|
| RAM Search (`memory_compare_panel.rs`) | Addresses matching a value or a delta | Anything about addresses you have not already guessed |
| RAM Watch | A live view of chosen addresses | Why you chose them |
| Access counter (`access_counter.rs`) | Read/write counts, last-access cycle | That an address was *touched*, not what it means |

All three narrow a set the user already has a hypothesis about. None classifies,
and none can distinguish causation from coincidence — because **observation alone
cannot**. An address counting up while the score counts up might be the score, or
might be a frame counter that happens to be running. Separating the two requires
changing the byte and re-simulating.

That is why this tool exists here and not elsewhere. Re-simulating one interval
twice under a single controlled difference is only sound if the replay is
bit-identical, which is RustyNES's determinism contract
([`testing-strategy.md`](./testing-strategy.md)) — a hard guarantee, and the same
one save-states, TAS replay and netplay rollback already depend on.

## Stage 1 — observation and classification

`rustynes_probe::atlas::observe` captures all 2048 work-RAM bytes once per frame
over a window (the panel uses 180 frames, about three seconds).

Storage is **address-major** — all frames for `$0000`, then all frames for
`$0001`. Every consumer walks one address across time, so the transpose turns a
strided gather over 2 KiB into a contiguous slice. Capture itself is frame-major
(work RAM is contiguous, so a frame is one `extend_from_slice`) and transposed
once at the end.

`classify` then reduces each address's series to an `AddressStats` and a
`Behaviour`:

| `Behaviour` | Criterion | Typical meaning |
|---|---|---|
| `Untouched` | zero changes | most of work RAM on any given screen |
| `FrameTick` | changed on ≥ `FRAME_TICK_RATIO` (90%) of transitions | animation phase, frame counter, scroll |
| `RisingCounter` | ≥ `COUNTER_MIN_STEPS` (2) steps up, none down | score, progress |
| `FallingCounter` | ≥ 2 steps down, none up | timer, lives, health, ammo |
| `Sparse` | ≤ `SPARSE_MAX_CHANGES` (3) changes | event-driven state |
| `Volatile` | changes often, both directions, below the tick threshold | churn |

Every threshold is a **public** constant. That is a deliberate API decision, not
an oversight: the module's claim is that its cutoffs are arguable, and a cutoff
that is documented but unreachable cannot be shown beside the label it produced.
The panel renders "changed on 178 of 179 transitions, at or above the 90%
frame-tick threshold" precisely because the constant is public.

### Ordering is load-bearing

`Untouched` and `FrameTick` are decided **before** the counter tests. A frame
counter is monotonic and would satisfy `RisingCounter`, but "ticks every frame" is
the more specific and more useful statement, so it wins. A test pins this
(`a_frame_counter_is_a_frame_tick_not_a_rising_counter`) because it is exactly the
kind of ordering a later tidy-up would invert without noticing.

### Wrap handling is load-bearing

A counter rolling `0xFF → 0x00` is still counting up. Judged by the two values
straddling the byte boundary (`prev >= WRAP_HIGH && next <= WRAP_LOW`), not by
arithmetic difference, which cannot tell a wrap from a large decrease. Without it,
that single transition registers as a decrease and a real counter is demoted from
`RisingCounter` to `Volatile`.

## Stage 2 — liveness verification

`verify_liveness` runs two trials against a `Probe` anchor: a baseline, and a run
with the byte perturbed. Divergence means the byte drives the observable.

The perturbation is the **bitwise complement** of the current value. It differs
from the original for every possible input — checked across the whole byte domain
by a test, because the fixed points of a naive scheme (`0x00` under multiply,
`0xFF` under OR) are exactly the values a real address is most likely to hold —
and it is a *large* change, which is far more likely to be observable than a
`+1` nudge that may move a sprite within the same pixel.

`Probe::run_perturbed` applies the perturbation after the anchor restore and
before frame 0, so it is provably the only difference between the two trials.
That is what licenses attributing a divergence to it. It counts against the same
trial budget as an ordinary trial: a perturbation sweep is the easiest way to
spend unbounded trials and must not have a cheaper path to the emulator.

### Liveness is relative to its lens

`Liveness` is not a property of an address. It is a property of an address *and*
an observable:

| Lens | Question |
|---|---|
| `Framebuffer` (panel default) | does this drive what I see? |
| `AudioEnergy` | does this drive what I hear? |
| `Wram` | did the poke reach memory? (almost always `Live`; true and useless) |

The same byte is routinely `Live` through `Wram` and `Inert` through
`Framebuffer`. Two tests assert exactly that pair — one perturbation, two lenses,
opposite verdicts — which is what proves `Inert` is a real verdict rather than a
stuck default. Every verdict the panel shows names the lens that produced it; one
that did not would be over-claiming.

### `Untested` is a third state, not an absence

`Liveness::Untested` is distinct from `Inert` on purpose. "We did not look" and
"we looked and saw nothing" are different claims, and collapsing them is how a
budget-limited sweep starts reporting addresses it never examined as dead.

Affordability is checked **up front**: if the budget cannot fund both trials, the
function returns `Untested` having spent *zero* trials, rather than burning a
baseline it cannot match. A test asserts `trials_used == 0`.

## What a label does not mean

This section is longer than the one above it, deliberately. The failure mode of
this tool is a confident wrong label that someone then builds a cheat, a Lua
script, or a RetroAchievements condition on.

- **`Inert` is not "unused."** An address the game rewrites from a master copy
  every frame reads `Inert` because the poke is overwritten before it can matter.
  That is a genuinely interesting fact about the address; it is not "dead".
- **`Inert` is bounded by the window.** The panel verifies over 8 frames. A byte
  whose effect is slower reads `Inert`, and the panel says "changed nothing the
  lens observed within 8 frames" rather than "changed nothing".
- **`Live` does not identify the byte.** It says the byte participates in what the
  lens observes. It does not say it *is* a coordinate, a score, or health.
- **A `Behaviour` is never upgraded by verification.** `RisingCounter` + `Live`
  is two independent observations — "counts up" and "drives something visible" —
  not a conclusion that it is the score.

## Cost, and why the panel admits it

The two actions have very different costs, and the UI is shaped around that
rather than hiding it.

| Action | Cost | Offered as |
|---|---|---|
| Observe | 180 frames ≈ 3 s of emulation | one button |
| Verify | **2 trials per address** | per-address, or a bounded batch of 16 |

A full 2048-address sweep would be 4096 trials and tens of minutes. It is
therefore **not offered**. A button that quietly takes twenty minutes is a worse
affordance than one that admits its limit.

The batch also skips `Untouched` addresses. They are the bulk of work RAM, and
perturbing a byte the game never reads is the one case guaranteed to be
uninformative; spending trials there would crowd out the addresses that moved.

Both actions snapshot, act, and `restore_quiet` — the live timeline and the
user's rewind ring end exactly where they started. `restore_quiet` rather than
`restore` matters: the loud variant clears the rewind ring, which is right for a
state loaded from elsewhere and wrong for a snapshot taken from this timeline
moments earlier.

## Determinism and the core

The atlas is **output-only**. It reads work RAM, and it perturbs only a scratch
replay that is discarded. The emulation core is unchanged by it, so AccuracyCoin
and nestest are unaffected — verified rather than asserted, since v2.3.6 also
added a `const fn` getter to `Nes`.

One property is load-bearing enough to be pinned permanently:
`a_trials_samples_are_independent_of_the_callers_rewind_state` asserts that a
trial's samples do not change when the caller has rewind armed and capturing. The
engine's premise is that a replay from one anchor is bit-identical, so anything
that silently perturbed state would invalidate the primitive rather than one
measurement.

## Panel

**Tools → Analysis → RAM Atlas.** Native and wasm; needs a loaded ROM.

- **Observe** classifies all 2048 addresses; **lens** selects the observable
  verification will use.
- The address list is **virtualized** (`ScrollArea::show_rows`) — with untouched
  addresses shown it is 2048 rows, and building that many selectable labels per
  frame is a cost with no purpose.
- The evidence pane sits **below** the list rather than inline under its row.
  Virtualization requires a uniform row height, and it reads better anyway: the
  detail does not shift the rows around it when opened, and stays visible while
  scrolling. The selection resolves from the *filtered* set, so a row hidden by
  the current filter stops showing evidence.
- An atlas is ROM-bound and is discarded on every ROM transition via
  `DebuggerOverlay::clear_rom_bound_analysis`. 2048 stale labels are a worse lie
  than one stale number, because they look like a map.

## Deliberately not implemented

Named here so they read as decisions rather than oversights:

- **Export paths** — seeding the Watch and Cheat panels, the Lua API, and
  RetroAchievements authoring. Additive on top of the labels; better shaped once
  the labels have been used in anger.
- **Per-game persistence** — an atlas keyed on the ROM hash via the
  `rustynes-gamedb` overlay.
- **Coordinate cross-referencing against OAM** — the original design sketched
  correlating candidate addresses against sprite X/Y. It is not implemented, and
  the `Behaviour` set makes no coordinate claim, so nothing currently over-states
  it.
- **Input-varying observation** — `observe` takes an input closure and the panel
  passes idle. An atlas taken while walking right describes movement state, which
  is useful and a confusing default.

## See also

- [`pixel-provenance.md`](./pixel-provenance.md) — the same discipline applied to
  video: a causal chain with explicit limits on what it claims.
- [`testing-strategy.md`](./testing-strategy.md) — the determinism contract this
  depends on.
- [`frontend.md`](./frontend.md) — where the panel sits in the shell.
