# Latency Oracle

> **Status:** measurement and the panel are implemented (v2.3.6). Per-game
> persistence and the end-to-end (game + host pipeline) figure are **not** — see
> [Deliberately not implemented](#deliberately-not-implemented). This document is
> the spec, so it is updated in the same change as the code it describes.

## What the feature answers

**How many frames of input lag does *this* game have, and what run-ahead depth
removes them?**

Most NES titles sample the controller in their NMI handler and act on it one or
more frames later. That delay is real on hardware, and run-ahead removes it by
simulating those frames in advance — but only if you tell it how many.

Every emulator makes finding that number a manual ritual: hold a direction,
frame-advance until the sprite moves, subtract one. RetroArch documents exactly
that procedure. RustyNES's own settings panel said only "1 fits most games".
Nothing measured it.

Note this is a different quantity from RetroArch's "automatic frame delay", which
adapts the *host* pipeline's delay. This measures the **game's internal** lag.

## Method

From a live anchor, run two trials — one with a probe button held from frame 0,
one with nothing pressed — and find the first frame at which an observable
diverges. That index **is** the game's internal lag, because it is the first frame
on which pressing the button could have changed anything.

On a deterministic core, two replays of identical state can differ for exactly one
reason: the input. That is what makes the answer a property of the ROM rather than
of the run, and it rests on the determinism contract in
[`testing-strategy.md`](./testing-strategy.md).

### Why it probes six buttons and three observables

A single button and a single observable produce a number that is often wrong, and
wrong in a way indistinguishable from right. Two failure modes drive the design:

- **A game may ignore most buttons.** `PROBE_BUTTONS` is `RIGHT, LEFT, DOWN, UP,
  A, B` — directions first, since they are what games act on soonest and what a
  player is holding when latency matters. **`START` is deliberately excluded:** it
  pauses many games, which *is* a reaction, but a reaction to a menu rather than
  gameplay input, and counting it would over-report.
- **A reaction may not be visible yet.** `OBSERVABLE_ORDER` is `Framebuffer`,
  `AudioEnergy`, `Wram`. Framebuffer first because a visible reaction is what a
  player perceives as latency; audio next, since a sound effect often fires the
  same frame the input is accepted; work RAM last, because it detects a reaction
  the player cannot yet perceive — useful as evidence the pad was read at all, but
  the least representative of *felt* latency. A menu that commits a highlight to a
  variable one frame before drawing it would otherwise read as slower than it is.

The measurement returns as soon as one observable yields agreeing answers, so the
common case costs one observable's worth of trials rather than all three.

> **The audio stage did not work until v2.3.6.** The trial loop emptied its audio
> buffer and never filled it, so `AudioEnergy` summed an empty slice and reported
> zero energy on every frame of every trial. Nothing failed, because a lens that
> returns a constant never disagrees with itself — the fallback simply degraded to
> work RAM without saying so. Fixed, and pinned by a wiring test that asserts
> through the emulator's audio queue rather than through sample values, because a
> silent fixture's drained audio hashes identically to an empty slice.

## Being honest is the hard part

A latency number is **acted on** — it sets run-ahead depth, which costs real frame
budget. So the module is built to decline rather than guess.

`LatencyReport::frames` is `Option<u32>`, and `None` and `Some(0)` are different
answers that must never be collapsed. `Some(0)` means the game reacted on the very
next frame; `None` means the probe could not tell. Reporting the second as the
first is how a latency tool starts lying.

`Confidence` accompanies every result:

| `Confidence` | Meaning |
|---|---|
| `Unanimous` | every button that reacted agreed on the same frame |
| `Majority` | a majority agreed; at least one reacting button disagreed — treat as approximate |
| `Inconclusive` | nothing reacted inside the budget, or the reacting buttons did not agree |

The panel renders `Inconclusive` as "inconclusive" with the per-button evidence,
**never** as "0 frames". A latency tool that cannot say "I don't know" is worse
than none, because its wrong answers become indistinguishable from its right ones.

Per-button evidence is shown for confident results too. A tool that publishes only
its conclusion cannot be checked.

### It recommends; it does not apply

A measured depth is never written to the config on its own. Run-ahead is linear in
the core's frame cost — roughly 34% / 52% / 78% of the NTSC budget at depth 0 / 1
/ 2 — so silently raising it can push a marginal host into dropped frames for a
change the user never asked for. The number appears with an explicit **Apply**
button.

`take_pending_apply` is set only by that button, and
`a_measurement_alone_never_requests_an_apply` fails if storing a report ever
queues a config write by itself. Routing the depth through a drained field keeps
"measured" and "applied" as two separate, auditable steps.

### The recommendation is clamped, not discarded

A game measuring deeper than the run-ahead range is reported honestly and the
*recommendation* clamped to `MAX_RUN_AHEAD_DEPTH`. Discarding the measurement
would hide a real finding.

That constant is shared with `effective_run_ahead` and the run-ahead throttle
rather than redeclared. It exists because those two caps were once separate `3`
literals that drifted apart (PR #358), and a third copy in the panel would have
reopened the same seam (PR #385 review).

### Milliseconds are derived, not transcribed

The felt-latency figure multiplies the frame count by the console's own
`Nes::frame_duration`, captured **at measurement time**.

A hardcoded NTSC `16.639` would understate PAL and Dendy by 20.2% — the identical
figure and mechanism as the v2.3.5 libretro defect, where a hardcoded `60.0988`
fps had lost all connection to the constant it was copied from and ran every PAL
cartridge fast. Captured at measurement time rather than read at render time
because the duration is a property of the *measurement*: unloading the ROM, or
loading a PAL game after measuring an NTSC one, must not restate an old result in
the new region's units.

`felt_milliseconds_track_the_region_not_a_constant` fails if the conversion is
ever hardcoded again.

## Cost, and the timeline

The measurement drives several hundred frames synchronously under the emulator
lock, on the button press, like `BasicBot`'s search. The UI pauses briefly and the
button says so rather than pretending the work is free.

`measure_in_place` snapshots a restore point, probes the live instance, and
restores it — the live timeline is exactly where it was. It exists because the
frontend already holds `&mut Nes` under the lock and cloning a `Nes` per
measurement is a cost with no purpose there.

The trial budget is **exactly** the trials the loop can run: one idle baseline
plus one held trial per button, per observable. Not "plus headroom" — a ceiling
with slack in it is not a ceiling, and `Probe::run` makes it binding, so a future
edit that adds a trial fails closed rather than silently spending more of the
caller's time than advertised.

### The rewind interaction, and a fix that was not one

Trials restore their anchor through `Probe::run_uncounted`, which uses
`restore_quiet` and suppresses rewind capture for the trial's duration.

Both were bugs before v2.3.6, and the history is worth recording. PR #385 review
reported that a measurement destroyed the user's rewind history; that fix changed
only `measure_in_place`'s **final** restore. Every *trial* still used the loud
`restore`, and a measurement runs up to 21 trials against the live emulator — so
the ring was still being cleared, twenty-one times over, behind a fix that
reported the bug closed. Fixing it exposed a second defect underneath: the ring
then *grows*, polluted with replayed frames that never happened on the user's
timeline.

The test asserts the ring returns **exactly** as it was. A weaker "not cleared"
assertion would have passed while the pollution remained — which is precisely how
the incomplete fix cleared review.

Separately,
`a_trials_samples_are_independent_of_the_callers_rewind_state` pins that none of
this affects what a trial *measures*, so no measurement taken before the fix
needed re-running.

## Panel

**Tools → Analysis → Latency Oracle.** Needs a loaded ROM.

The measurement runs **after** the egui render rather than inside the window
closure, so `nes` is never captured by the viewport callback — the same deferred
shape the other `&mut Nes` panels use.

A report is ROM-bound and discarded on every ROM transition via
`DebuggerOverlay::clear_rom_bound_analysis`. The queued Apply depth is cleared
with it, and that half matters more: a stale report is a cosmetic lie, but a stale
`pending_apply` is one click from applying a depth measured on a different
cartridge (PR #385 review).

## Deliberately not implemented

- **Per-game persistence** of the applied depth via the `rustynes-gamedb`
  overlay.
- **The end-to-end figure** — the measured internal lag *plus* the frontend's own
  pipeline cost, which `perf.rs` already tracks. The panel currently reports only
  the game's internal lag, and says so.
- **Automatic application**, including cross-checking the depth against the frame
  budget before applying. The original design sketched applying automatically;
  recommend-with-Apply was chosen instead, which makes the budget cross-check a
  smaller concern than it would have been.

## See also

- [`ram-atlas.md`](./ram-atlas.md) — the other consumer of the same probe engine.
- [`performance.md`](./performance.md) — run-ahead's cost as a fraction of the
  frame budget.
- [`testing-strategy.md`](./testing-strategy.md) — the determinism contract.
