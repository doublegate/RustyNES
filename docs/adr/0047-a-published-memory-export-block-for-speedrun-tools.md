# 47. A published memory-export block, and master-clock timing, for speedrun tools

Date: 2026-10-20

## Status

Accepted. `T-SPEEDRUN-MEMIF` (D33, `to-dos/plans/v3.5.0-plan.md` power-user
rows). Normative description: `docs/memory-export-interface.md`.

## Context

Speedrun timers for emulators read the game's RAM from outside. LiveSplit's
auto-splitters attach to the emulator process and follow pointers from base
addresses someone found for one build of one emulator; every release that
moves a structure breaks every splitter, and the attach needs debugging rights.
Time is usually taken from the host's wall clock, so a stutter or fast-forward
changes it and a movie cannot be re-timed to the same number.

RustyNES has two things that make a better contract possible: a deterministic
core with one master clock (ADR 0002 / 0029), and, since CR-02, a named
shared-memory layer in the frontend (ADR 0016's 2026-10-20 amendment).

## Decision

1. **Time is master-clock cycles.** A run is timed from the end of the frame
   its start predicate first holds to the end of the frame its last split's
   predicate first holds, as a difference of the CPU's master clock. Seconds
   are derived from an exact per-region fraction (NTSC 236,250,000/11 Hz; PAL
   and Dendy 53,203,425/2 Hz). The timer lives in `rustynes-core`
   (`speedrun`, `no_std`), reads through the side-effect-free peek, and never
   writes, so attaching it cannot change a frame, a sample or a bus cycle and
   the same movie times the same on every replay. A step backward in the clock,
   or a leap of more than sixteen PAL frames, abandons the run (a state load,
   rewind or power cycle no longer measures one run).
2. **Predicates are the RetroAchievements condition grammar**, a documented
   subset (sizes, `d`/`p`/`b`/`~`, comparisons, `S` groups, hit counts) over
   RetroAchievements' NES flat address space, so a condition copied from an
   achievement set means the same here. Flags and arithmetic are refused with
   a message rather than misread; the splits file's own `reset` covers `R:`.
   The grammar was written from RetroAchievements' public description, not
   from an implementation.
3. **A fixed, versioned block is published every frame** (`--memif`, the
   `memif` feature, part of `full`): CPU RAM and the cartridge RAM window at
   fixed offsets equal to `0x0100 +` the flat address, the frame and master
   clock, the region's clock fraction, the ROM's hash, the epoch, and the
   timer's state and split table, under one sequence counter so a reader gets
   one frame's values. Within layout version 1 no field moves; reserved bytes
   may gain meaning later. The block is output only.
4. **Names are per user and objects owner-only** (from the CR-02 review):
   `/<name>.<euid hex>` on POSIX, `Local\<name>` on Windows; created `0600`
   (an owner-only DACL on Windows); an existing object not owned by the user
   with no group or other access is refused, never adopted.
5. **The gate uses a reader written from the document alone**
   (`scripts/memif/reference_splitter.py`, standard library only) and runs it,
   unchanged, against two different builds (`scripts/memif/two_build_check.sh`).

## Consequences

- A splitter written against the page keeps working across RustyNES releases
  that publish layout 1, on every platform, with no process attachment.
- Exposing RAM is not a hardcore concern: the block shows what the screen and
  the RAM watch already show the player, and nothing written to it reaches the
  emulator. It is opt-in per session.
- The timer's resolution is one frame (as every RAM-driven splitter's is); its
  frame boundaries are exact. Sub-frame timing would need predicates evaluated
  per CPU access and is not offered.
- A Lua-side timer API (a script declaring splits) is not part of this
  decision; scripts can read RAM and drive their own logic, and could publish
  through `comm.mmfWrite`.
- Changing the layout incompatibly needs a new layout version and a release
  note; the reference reader refuses any version it does not know.
