# The Divergence Lens

**Status:** v2.3.8 "Parallax". Core mechanism landed in `crates/rustynes-probe`;
the panel surface is tracked separately. `debug-hooks` is required for the audio
path and for capture; the pixel path works without it.

## The gap this closes

`rustynes-probe` (v2.3.6) already answers **whether** two configurations of the
same ROM diverge and **at which frame**. A trial reduces each frame to one `u64`,
and `Probe::first_divergence` scans the two sample vectors.

That reduction is the right shape for *detecting* a difference and the wrong
shape for *explaining* one. `Observable` is:

```rust
pub enum Observable { Framebuffer, IndexFramebuffer, Wram, AudioEnergy }
```

A hash says frame 412 differs. It cannot say which pixel, so it has nothing to
hand to Pixel Provenance — which is where an answer that names an instruction
actually lives. The Lens is therefore not "find the divergence" but **"narrow the
frame the detector already found, then hand off"**.

## Three answers, and the third is the point

```rust
pub enum Localisation { Identical, Differs(PixelDivergence), Inconclusive }
```

`Inconclusive` is **not** a synonym for `Identical`. The Latency Oracle set this
precedent in v2.3.6 — `None` and `Some(0)` are different answers and were never
collapsed — and it applies unchanged here. A tool that reports "no divergence"
when it merely stopped looking is worse than one that reports nothing, because
the caller acts on it.

`Inconclusive` is returned when:

- the trial budget cannot fund all four trials (checked **up front**, see below);
- a trial returned fewer frames than asked, so "they agreed as far as we looked"
  cannot be distinguished from "they agreed";
- the coarse detector and the fine localiser disagree — the detector found a
  diverging frame and the pixel or cycle diff found nothing in it.

That last case is worth stating plainly: two measurements disagreeing is a fact
about the run, not an answer about the ROM, and picking a winner would be
inventing one.

## Budget

One localisation is **four trials**: two to detect the frame, two to re-run to it
and keep the full output. `divergence::LOCALISE_TRIALS` exports the figure and
`divergence::budget_for` builds a `Budget` that admits exactly one call, the same
way `latency::budget_for` does.

The budget is checked **before the first trial**, mirroring
`atlas::verify_liveness`. Spending two trials on detection and then discovering
the localisation pair is unaffordable returns `Inconclusive` having consumed the
budget that would have answered the question on a second attempt.

**Detect cheap, localise expensive, and only once.** Retaining two 61,440-pixel
frames for every frame of a long trial is exactly the memory blow-up the `u64`
reduction exists to avoid.

## It leaves the emulator where it found it

Both entry points snapshot the live state on the way in and restore it on the
way out, the same contract `latency::measure_in_place` offers. A tool that
answers "what does this byte change?" by leaving the user's game thirty frames
further on than they left it is a worse bug than any it was asked about.

This is not free, and it was not right the first time. `localise` originally ran
its four trials and returned — because a trial restores the anchor on the way
**in** and not on the way out, which is the property the pixel path relies on to
read the diverging frame. The result was correct and the emulator was left
advanced. A test asserting `nes.snapshot()` is unchanged across the call caught
it.

The restore is wrapped in the probe engine's `TrialGuard`, because
`Nes::restore_inner` clears both provenance stores and this restore is exactly
the same-timeline case that exception exists for. Without the guard, asking the
Lens a question would empty the Pixel Provenance and Audio Provenance panels —
the v2.3.7 defect reintroduced one layer up. **The snapshot comparison cannot
catch that**, because provenance is deliberately not in the snapshot, so it is
pinned by its own test and its own mutation.

Both facts are structural rather than observed: every path out of `localise`
goes through one wrapper, so "all six early returns restore" is a property of
the shape rather than of inspection.

## The pixel path

`divergence::localise` reports, for the first diverging frame:

| Field | Why it is there |
| --- | --- |
| `count` | one pixel is a sprite or a palette entry; 256 in a row is a scanline; tens of thousands is a scroll or a mode change |
| `first` | somewhere to point, in raster order |
| `bbox` | the *shape*, which `count` alone does not give — two pixels far apart and a filled rectangle have very different boxes |

`is_single_scanline` is offered rather than left to call sites, because it is the
distinction a caller acts on and the inclusive comparison is easy to get wrong.

`PixelDivergence` is never empty. "They agree" is `Localisation::Identical`, not
a count of zero — a zero-population divergence and an agreement are the same
fact, and offering two encodings of it invites testing the wrong one.

### Why the index framebuffer, not the RGBA one

`Nes::index_framebuffer` is 256x240 `u16`s, each `(emphasis << 6) | colour` — the
PPU's own per-pixel output, before the palette lookup that produces RGBA. It is
half the bytes and at least as sensitive, because the RGBA buffer is a pure
function of this one **given the same palette**.

That proviso is stated rather than assumed. It holds because both trials run on
the same `Nes` instance and therefore share whatever palette — stock, generated,
or user-edited — is loaded. It would **not** hold for two separately configured
instances, and a future two-instance lens must revisit it rather than inherit it.

### No new engine primitive was needed

A trial restores the anchor on the way **in** and not on the way out, so the
emulator is left holding the trial's final frame and the Lens reads it straight
off `nes`.

That is pinned by `a_trial_leaves_the_emulator_at_its_end_state`, and the fixture
is the load-bearing part. The obvious `synth_nrom` renders a blank screen, where
"left at the anchor" and "left at the end" are byte-identical — so the test would
pass under both behaviours and prove nothing. The pin uses a ROM that drives
PPUMASK emphasis from work RAM, and asserts the screen varies at all before
asserting anything about the engine.

## Capture: what a trial records, and what it must not

**A probe trial produces no provenance.** This is the finding that reshaped work
item B, and it is not obvious from either the code or the earlier docs.

`Probe::run_uncounted` restores the anchor through `restore_quiet`, and
`Nes::restore_inner` clears both provenance stores. Before v2.3.7 a trial's
records were therefore destroyed at the start of the *next* trial — along with
the caller's, which is the user-visible defect v2.3.7 closed. After it,
`TrialGuard` holds both stores aside for the trial's whole duration, leaving the
emulator **unarmed** while a trial runs. That is deliberate and correct:
re-simulated frames never happened on the user's timeline and must not contribute
attributions to it.

So capture had to be built, and v2.3.7's fix is exactly what makes it safe rather
than a leak: because the caller's stores are already held aside, a trial can arm
**fresh** ones with no path by which its records could reach the user's.

`Probe::run_capturing` therefore does three things in a strict order, and the
order is the design:

1. enter the guard (caller's stores move out) and restore the anchor;
2. arm fresh stores — **after** the restore that would otherwise clear them;
3. run the trial, then harvest the fresh stores **before** the guard drops and
   puts the caller's back.

Getting step 2 or 3 on the wrong side of its neighbour either loses the capture
or leaks it into the user's record.

Capture is opt-in **per trial**, not per `Probe`. That is a cost decision: a
per-CPU-cycle mix trace is roughly 29,780 records a frame, so a probe-level flag
would bill the Latency Oracle's 21 trials — around 625k records — for a feature
it never reads.

## The audio path

`divergence::localise_audio` detects under `Observable::AudioEnergy` and then
localises with the captured mix traces, resolving to an absolute **CPU cycle**
and both `MixRecord`s at it — mixed sample, expansion contribution, and all five
channels' raw pre-mix outputs.

This is finer than the pixel path, and it reaches sub-frame resolution **without
bisection**: work item B framed that as a choice between ~17 partial re-runs and
reading records that already exist, and for audio the records genuinely do exist
once a trial is asked to keep them.

Detection and localisation deliberately use different instruments. `AudioEnergy`
is a quantised sum of `|amplitude|` over a frame — coarse on purpose, because
exact float equality across a resampled stream compares noise rather than signal.
The mix trace is exact, per cycle, and pre-decimation. Coarse to find the frame,
exact to find the cycle. The price is that they can disagree, which returns
`Inconclusive`.

### Two guards that are not tested, and are kept anyway

`localise_audio` checks that the two traces share a `first_cycle` and that
neither is truncated. **Neither guard is reachable under the current design, and
neither is covered by a test** — verified by mutation: deleting either changes no
observable behaviour.

They are kept because both preconditions are properties of code elsewhere. The
trace is re-anchored per frame, so two trials from one anchor share a
`first_cycle`; `MIX_CAP` is 36,864 against Dendy's worst-case 35,464-cycle frame,
the largest of any supported region. If either fact changes, the result should
surface as `Inconclusive` rather than as a confidently wrong cycle.

Saying so here is the point. An untested guard described as though it were tested
is the failure mode this project has been bitten by repeatedly — see
`docs/pixel-provenance.md`, where prose asserting an intent is what stopped
anyone checking the code against it for four releases.

## Why bisection was not built

Work item B framed sub-frame localisation as a choice between two mechanisms:
bisect the frame over ~17 partial re-runs, or read the per-cycle records that
already exist. It asked for the cheap one to be proved viable before either was
written.

Proving it produced a **third** answer. For audio, riding the records works
directly and resolves to a CPU cycle. For pixels, it does something better than
bisection rather than cheaper: `localise_explained` returns the diverging pixel's
`PixelProvenance` from **both** configurations, so the answer is *which causal
input differs* — the winning layer, the pattern row, the nametable and attribute
addresses, the palette entry, the emphasis mask — rather than *at which cycle the
two runs parted*.

That is the question a user chasing a rendering bug actually has. A cycle index
says when; a differing `pattern_addr` says the two runs fetched different tile
data, which is a lead. Bisection would have cost ~17 extra trials to produce the
weaker answer.

**Bisection is therefore not implemented, and that is a decision rather than an
omission.** It remains the only route in a build without `debug-hooks`, where
capture cannot exist at all. If a case appears where the cycle genuinely is the
question — two runs whose causal inputs are identical but whose timing differs —
bisection is the mechanism to reach for, and this section is the record of why it
was not needed first.

### A doc claim this section had to retract

`differing_fields` was documented as possibly empty at a located pixel, on the
reasoning that a colour could differ "through emphasis or greyscale rather than
through anything in the causal chain". That is wrong. An index-framebuffer entry
is `(emphasis << 6) | colour`, so if two entries differ then either the colour
differs — reported as `color` — or the emphasis bits do, and those are carried in
`color_mask`. There is no third way for the entry to change.

The distinction is load-bearing rather than pedantic: under the permissive
reading, an empty result is a legitimate outcome to render. Under the correct
one, it means the two records did not come from the two configurations, which is
a defect. A mutation that read both records from the same trial was caught only
after the assertion was tightened to match.

## What the Lens deliberately does not do

- **It does not bisect the frame for a pixel divergence, and it turns out it
  does not need to.** See "Why bisection was not built" below.
- **It does not compare two ROMs, or two differently configured instances.** The
  index-framebuffer argument above depends on one shared palette, and the trial
  engine asserts both sides run the same ROM.
- **It does not attribute.** It narrows, and hands off to Pixel Provenance and
  Audio Provenance, which already answer *which instruction*.
