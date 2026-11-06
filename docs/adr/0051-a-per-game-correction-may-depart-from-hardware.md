# 51. A per-game correction may depart from hardware, on by default for one game

## Status

Accepted, 2026-11-06 (maintainer). v3.6.2, `T-RADRACER-COHERENT-COPY`.

## Context

RustyNES emulates the hardware, glitches included. The test ROMs are the spec, and a picture a real console draws is the picture RustyNES draws.

*Rad Racer* draws stray tiles at the right of the screen on real hardware. The game writes `$2006` mid-scanline from sprite-0-timed loops whose timing varies by a few dots from frame to frame. When the delayed `t -> v` copy lands inside an 8-dot background fetch group, that group's tile is fetched from two addresses:

- a nametable read between the group's two halves takes the "Hybrid Addresses" splice (ADR 0030, tested by AccuracyCoin);
- or a pattern read takes the new fine Y under the old tile number.

On about one frame in eleven this draws an 8-pixel wrong tile in the sand right of the road (line 139), and on most frames one tile at the right of each line of the band from the horizon down (lines 107-139; the specks where the ground meets the horizon are line 114's). A 2012 nesdev report describes this as "a very noticable glitch" on the right on a real NES.

The maintainer wants those glitches gone in *Rad Racer*, and does not want every other game's PPU changed to get there. v3.6.2's first attempt changed every game's attribute read on a wrong diagnosis. It is withdrawn: see ADR 0030, "Withdrawal — 2026-11-06".

## Decision

A **per-game correction** may depart from hardware when all of these hold:

1. **It is keyed to specific dumps.** Here that is the headerless CRC32s of Rad Racer (USA) `8B9D3E9C`, Rad Racer (Europe) `654F4E90` and Highway Star (Japan) `02589598`, the keys the vendored game database uses. The table is `rustynes_core::game_corrections`. The core turns the correction on at load for those dumps only; every other game is untouched.
2. **It is switchable.** The user can turn it off to see the hardware's picture: `[enhancements] disable_game_corrections` on the desktop, `rustynes_game_corrections` in the libretro core.
3. **It is recorded.** It is a `HardwareOptions` field. A movie records it (`.rnm` format 8), the netplay configuration digest covers it, and the libretro save-state options record carries it. A replay or a peer therefore runs the same picture.
4. **It is render-only, or as close to it as the mechanism allows,** and stated as a departure wherever it is described.

**The correction itself (`coherent_midline_copy`).** A delayed `$2006` copy whose countdown ends inside a fetch group, on a rendered line with rendering on (dots 2-248, not a group's first dot), is held to the end of that group. It lands just before the group's coarse-X increment (dots 8-248), in the fetch pipeline itself, so the increment applies to it under the same rendering gate as the hardware path. So every tile is fetched from one address, and from there on `v` is exactly what the unheld copy gives.

No snapshot field is added: a held copy is the `copy_v_delay` value `COPY_V_HELD` (0xFF). The PPU's hardware path is unchanged when the correction is off.

**Measured** (scripted races, frames 700-1499, `rad_racer_movie`):

- The line-139 dash goes from 73 and 68 frames to 0 in both races.
- With the correction on, the frames differ from the hardware path only at x 185-255 on lines 107-139, on 530 to 568 of the 800 frames: one tile at the right of each line the game splits.
- The y-172 detector hits that remain are game content: Mesen shows the same counts.

## Consequences

- *Rad Racer* draws no stray tiles by default. The picture is not the hardware's, and every place that describes the correction says so.
- Movies and netplay depend on the setting, as they do on the sprite-limit option. `.rnm` format 7 is refused, at no extra cost: every format-7 movie was recorded under epoch 9 or earlier, which epoch 10 refuses anyway.
- The MiSTer core does not implement the correction. It stays hardware-faithful, and its co-simulation goldens are exported with the correction off (the oracle's default for every test ROM).
- **A new correction needs the same four properties, and its own ADR or an amendment to this one.** The precedent is narrow on purpose: it is not a licence to change the PPU for a game whose glitch is authentic, only to hide one where the maintainer asks.

**Rejected:**

- Changing the PPU for every game. AccuracyCoin's Hybrid Addresses test and the documented fine-Y behaviour say the hardware draws the split tile.
- Holding v3.6.2 for a hardware capture. No capture is available, and the nesdev report already places a glitch on that line on a console.
