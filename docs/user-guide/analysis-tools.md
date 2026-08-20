# Analysis tools

**Tools → Analysis** holds the tools that answer questions about the running game
rather than changing it. This page covers four of them, plus **Audio Provenance**,
which lives under **Tools → Audio** because that is where you are when you need it;
the Analysis submenu also contains **BasicBot**, an input-search tool documented
separately. All of them are **output-only in effect**: an analysis may advance the emulator or change memory
while it runs, but each restores the live timeline — and your rewind history —
before it returns. Nothing they do reaches a save state, a movie, or a netplay
peer, which is also why they are unavailable during those sessions.

They share one design rule worth knowing before you use them: **each says what it
does not know.** None of them will give you a confident number in place of "I could
not tell", because a wrong answer from a tool like this is worse than no answer —
you would act on it.

| Tool | Answers |
|---|---|
| [Latency Oracle](#latency-oracle) | How many frames of input lag does this game have? |
| [RAM Atlas](#ram-atlas) | What is each byte of work RAM for? |
| [Pixel Provenance](#pixel-provenance) | Why does this pixel look like that? |
| [Audio Provenance](#audio-provenance) | Why does it sound like that, and what wrote the register? |
| [Divergence Lens](#divergence-lens) | Two settings render this game differently — *which pixels*, and why? |

---

## Latency Oracle

Most NES games read the controller and act on it a frame or more later. Run-ahead
hides that delay by simulating those frames in advance — but you have to tell it
how many, and in every other emulator finding the number means holding a direction
and frame-advancing until the sprite moves.

**Use it:** load a game, get into actual gameplay, then **Tools → Analysis →
Latency Oracle → Measure now**.

The emulator pauses for a moment while it replays the current moment twice — once
with a button held, once with nothing pressed — and reports the first frame that
differs. Your game is put back exactly where it was, including your rewind
history.

**Measure during gameplay, not on a title screen.** The tool needs the game to
react to a button. On a menu or a cut-scene it will usually say *inconclusive*,
which is the honest answer rather than a failure.

### Reading the result

- **Internal lag: 2 frames** — with roughly how many milliseconds that is on your
  console's region.
- **Confidence** — whether every reacting button agreed, or only a majority
  ("treat as approximate").
- **Recommended run-ahead: 2** with an **Apply** button.
- **Per-button evidence** — expand it to see which buttons reacted and when. It is
  shown even for confident results, so you can check the tool rather than trust it.

**It never changes your run-ahead setting on its own.** Each extra frame of
run-ahead costs roughly a whole frame of emulation, so raising it can cause dropped
frames on a marginal machine. Applying is your decision, always one explicit
click.

If it says **inconclusive**, it means the probe buttons disagreed or nothing
reacted in the window. It will not offer a depth. Try again during active play.

---

## RAM Atlas

A map of the console's 2 KiB of work RAM: what each byte was doing, and whether
changing it actually affects anything.

This is not a RAM search. A RAM search narrows a list you already have a guess
about. The Atlas classifies every address, then lets you **verify** a candidate.

**Use it:** **Tools → Analysis → RAM Atlas → Observe**. About three seconds of
emulation, then every address is labelled.

### The labels

| Label | What it means | Often |
|---|---|---|
| untouched | never changed during the window | most of RAM |
| frame tick | changed nearly every frame | animation, scroll, frame counter |
| rising | only counted up | score, progress |
| falling | only counted down | timer, lives, health, ammo |
| sparse | changed a handful of times | event-driven state |
| volatile | changed often, both directions | working scratch |

**These are hypotheses.** An address that counts up while your score counts up
might be the score — or a frame counter that happens to be running. The label
alone cannot tell you which.

### Verifying

Click an address, then **Verify this address** (or **Verify next 16** for a
batch). The tool changes the byte, re-simulates, and compares:

- **LIVE** — changing it changed what the lens observed. It participates in what
  you see. It does **not** tell you the byte *is* the score.
- **inert** — changing it changed nothing the lens observed in that window. **This
  is not the same as unused.** A byte the game rewrites from a master copy every
  frame reads inert because your change is overwritten before it can matter.
- **untested** — not checked yet. Never shown as "inert".

The **lens** dropdown picks what counts as an effect: *screen* (the default and
usually what you want), *audio*, or *work RAM* (which reports almost everything
live, because your poke *is* a work-RAM change — true and useless). Every verdict
names the lens it used, because liveness depends on it.

**Observe and Verify are unavailable during netplay, TAS recording or playback, or
RetroAchievements hardcore** — the two *actions*, not the panel. Both advance the
emulator and Verify changes memory, so they would diverge a session other people
are synchronised to, or make a memory write hardcore mode exists to forbid. The
panel stays open and everything already classified stays readable and
exportable; it says which reason applies.

**Why there is no "verify everything".** Verification costs two re-simulations per
address, so all 2048 would be over four thousand runs and tens of minutes. The
batch is capped at 16 and skips untouched addresses.

Each row shows its evidence — change count, direction, range, and the threshold
that decided the label — so you can disagree with it.

### Sending an address to RAM Watch

**Send to RAM Watch** moves a classified address into the watch list, carrying its
verdict **and the lens that produced it**. The lens travels with the address on
purpose: liveness is relative to what was observed, so an unqualified "LIVE" in a
watch list would outlive the panel that qualified it and become a claim nobody can
check. The export is a pure read of a result the panel already holds, so unlike
**Observe** and **Verify** it stays available in a locked session — it advances
nothing and writes nothing.

The remaining exports — cheat, Lua, and RetroAchievements authoring — are
deliberately not built yet. A cheat is a **write**, so it needs a locked-session
predicate this one correctly does without.

---

## Pixel Provenance

Click any pixel and get its whole causal chain: the PPU dot and scanline that
emitted it, which layer won the priority decision, the nametable / attribute /
pattern addresses of the tile actually on screen, the palette entry, and the CPU
instruction and cycle that last wrote each of those bytes.

**Use it:** **Tools → Analysis → Pixel Provenance**, tick **Enable**, let a frame
run, then click the game image.

Full detail in [`pixel-provenance.md`](../pixel-provenance.md).

> **If you used this before v2.3.6, it did not work.** The report was empty for
> anyone with the default run-ahead setting, and clicking a pixel was never
> implemented at all. Both are fixed.

---

## Audio Provenance

The APU counterpart of Pixel Provenance, and it answers two different questions.
**Register attribution** says *what wrote this, and from which instruction* — the
CPU instruction and cycle behind a given APU register write. The **mix trace** says
*what the channels were actually doing*, sampled per **CPU cycle** rather than per
output sample, because that is the cadence at which the mix is genuinely computed.

The trace carries **raw** pre-mix channel values, so a record describes the chip
rather than your mixer sliders. That is the difference between a record you can
compare against hardware and one that only describes your own settings.

**Use it:** **Tools → Audio → Audio Provenance**. Output-only and off by default;
nothing it records enters a save state.

Two registers are handled by the bus rather than by `Apu::write_register` and so
are **not** attributed: `$4014` (OAM DMA) and `$4016` (controller strobe). They
were documented as attributed before v2.3.7 and were not; the panel now says so.

Full detail in [`audio-provenance.md`](../audio-provenance.md).

---

## Divergence Lens

Two configurations of the same game render differently and you want to know
*where*. The Lens answers with pixels rather than a frame number.

Detection alone was already possible — a trial reduces each frame to one hash, so
it can say "frame 412 differs". That is the right shape for **detecting** a
difference and the wrong shape for **explaining** one: a hash has nothing to hand
to Pixel Provenance, which is where an answer lives. The Lens re-runs both
configurations to the detected frame, keeps the whole frame instead of its hash,
and reports the **shape** of the difference:

- **how many pixels** differ,
- the **first** one in raster order,
- and the **bounding box** containing all of them.

Count and box separate kinds of bug from each other. One pixel is a sprite or a
palette entry. A row of 256 is a scanline. Tens of thousands is a scroll or a mode
change. From there, hand the located pixel to Pixel Provenance and the answer
becomes a cause rather than a coordinate. An **audio** lens resolves a divergence
to the CPU cycle instead.

**Three verdicts, and the third is the point.** `Identical`, `Differs`, and
**`Inconclusive`** — for an exhausted budget, or two trials that cannot be
compared. "I stopped looking" never arrives wearing the same shape as "they
agree". The budget is checked up front for all four trials, so the Lens cannot
spend its allowance on detection and then discover it can no longer afford to
localise.

**Use it:** **Tools → Analysis → Divergence Lens**.

Full detail in [`divergence-lens.md`](../divergence-lens.md).

---

## See also

- [Menu reference](./menus.md) — where these sit in the menu tree
- [Debugger](./debugger.md) — the chip inspectors, memory views and breakpoints
- [`ram-atlas.md`](../ram-atlas.md), [`latency-oracle.md`](../latency-oracle.md),
  [`divergence-lens.md`](../divergence-lens.md),
  [`audio-provenance.md`](../audio-provenance.md) — the full specs, including what
  each tool deliberately does not claim
