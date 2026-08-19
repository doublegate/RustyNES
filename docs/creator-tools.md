# Creator Tools

**References:** the authoritative detail is in [`frontend.md`](frontend.md); scripting has its own spec in [`scripting.md`](scripting.md). This page is a curated handbook entry point.

Beyond just running games, RustyNES ships a suite of tools for TAS authors,
ROM hackers, cheat writers, and the merely curious. They are all **frontend-side**
and, where they read the emulator, read-only against the deterministic core (the
overlay never advances emulator-visible state). Most are surfaced from the **Tools**
and **Debug** menus.

## Debugger

A Mesen2-style debugging overlay (toggle with `` ` ``) layers chip-inspection
panels over the running game: CPU (registers + disassembly), PPU, OAM, APU,
Memory (+ a Memory Compare), Mapper, an execution Trace, Watches, Events, and an
NSF panel. Chip panels need `&mut Nes` and a per-frame core poll, so they render
only while the overlay is visible. See [`frontend.md`](frontend.md) §
Chip panels vs tool panels and [`user-guide/debugger.md`](user-guide/debugger.md).

## Cheats & Game Genie

The **Cheats** tool panel edits Game Genie and raw address/value codes. Codes are
keyed on the ROM's identifying CRC32s (the header-excluded key *and* the full-file
No-Intro key), so a code matches whichever dump variant the user has. A curated,
header-robust Game Genie code database ships with the app (v2.1.3 "Codex").

## ROM Info & ROM Database

- **ROM Database** (editable) — view and correct the loaded ROM's per-game DB
  entry (mirroring / region / mapper / submapper / title), persisted to a user
  overlay. Mirroring applies live; header overrides apply on next load.
- **ROM Info** (read-only, v2.2.0 "Capstone") — a purely observational companion:
  the loaded ROM's two identity CRC32s (game-DB key + No-Intro full-file key),
  its SHA-256, its effective DB entry, and its decoded cartridge header (mapper,
  region, PRG-ROM / CHR-ROM sizes). See [`frontend.md`](frontend.md).

## Movies & TAStudio

RustyNES records, plays, and branches input movies (`.rnm`), and imports foreign
formats (`.fm2` / `.bk2` and legacy `.fcm` / `.fmv` / `.mc2` / `.vmv`). The
**TAStudio** piano-roll editor gives a frame-by-frame input timeline with seeking
and branching. Determinism (same ROM + seed + input ⇒ byte-identical frames) is
what makes movie replay exact — see [`testing-strategy.md`](testing-strategy.md)
for the determinism contract. The `.rnm` deserializer is hardened against
malformed input (bounded allocations; fuzz-tested, see `fuzz/`).

**Movies record two ports.** `FrameInput` models P1 and P2, so a recording made
with the **Four Score** adapter active captures half of what drove the run and
replays as a different one. The console carries four ports and the emulator
drives all four; the movie format does not. This is a documented limit rather
than a defect being worked around: widening `FrameInput` is a `.rnm` format epoch
change (ADR 0028), not an additive one, and the `.fm2` importer takes the same
position — it keeps pads 1 and 2, drops 3 and 4, and preserves the `fourscore`
flag so the caller is not misled.

The failure is silent at record time, so it is surfaced where the claim is made:
the Replay panel prints the caveat directly under the "Four Score (P1..P4)"
topology row, and `rustynes verify` catches a divergent replay after the fact
because its attestation folds in the video the run produced, not just the input.

## Scripting (Lua)

An embedded Lua engine exposes an emulation API (memory peek/poke, frame hooks,
input, drawing) for tool-assisted automation and overlays. See the dedicated
[Scripting (Lua)](scripting.md) page.

## RetroAchievements

A built-in RetroAchievements client (login, achievement list, hardcore mode).
See [RetroAchievements](cheevos-browser.md).
