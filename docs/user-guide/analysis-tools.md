# Analysis tools

Three tools under **Tools → Analysis** answer questions about the running game
rather than changing it. All three are output-only: they never alter emulation, and
each restores the live timeline when it finishes.

They share one design rule worth knowing before you use them: **each says what it
does not know.** None of them will give you a confident number in place of "I could
not tell", because a wrong answer from a tool like this is worse than no answer —
you would act on it.

| Tool | Answers |
|---|---|
| [Latency Oracle](#latency-oracle) | How many frames of input lag does this game have? |
| [RAM Atlas](#ram-atlas) | What is each byte of work RAM for? |
| [Pixel Provenance](#pixel-provenance) | Why does this pixel look like that? |

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

**Not available during netplay, TAS recording or playback, or RetroAchievements
hardcore.** Both actions advance the emulator and Verify changes memory, so they
would diverge a session other people are synchronised to, or make a memory write
hardcore mode exists to forbid. The panel says which reason applies.

**Why there is no "verify everything".** Verification costs two re-simulations per
address, so all 2048 would be over four thousand runs and tens of minutes. The
batch is capped at 16 and skips untouched addresses.

Each row shows its evidence — change count, direction, range, and the threshold
that decided the label — so you can disagree with it.

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

## See also

- [Menu reference](./menus.md) — where these sit in the menu tree
- [Debugger](./debugger.md) — the chip inspectors, memory views and breakpoints
- [`ram-atlas.md`](../ram-atlas.md), [`latency-oracle.md`](../latency-oracle.md) —
  the full specs, including what each tool deliberately does not claim
