<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# The live-patch recorder and regression replay (v3.7.0)

`T-HACK-BPS-RECORDER`, `crates/rustynes-frontend/src/patch_recorder.rs`, and
the `rustynes patch-regress` command.

A ROM hack is made a byte at a time: a PRG byte to change a rule, a CHR byte to
change a tile, a RAM pin to hold a value while testing. The recorder turns those
edits into the two files the hacking community exchanges, and the regression
replay checks the result against reference play.

The recorder and the replay are frontend code. The live ROM editor (section 5)
adds one `debug-hooks`-gated core call, `Nes::debug_poke_rom`, that nothing in
the run loop calls, so with no edit made the determinism contract, every golden
and the emulation epoch are untouched.

## 1. The BPS patch

PRG and CHR edits are written as a BPS 1.0 patch, implemented from the format's
public specification (byuu's "beat" BPS specification, public domain). No
third-party patcher's source was read.

| Part | Encoding |
|---|---|
| marker | `BPS1` |
| sizes | source size, target size, metadata size, each a BPS number |
| metadata | the bytes, unterminated (XML 1.0 in UTF-8, as the specification recommends) |
| actions | each a BPS number: the low two bits pick the action (0 `SourceRead`, 1 `TargetRead`, 2 `SourceCopy`, 3 `TargetCopy`), the rest are the length minus one; `TargetRead` is followed by its bytes |
| footer | IEEE CRC-32 of the source, of the target, and of the patch up to that last CRC, each little-endian |

A **BPS number** is seven bits a byte, least significant first, bit 7 set on
the final byte, with one subtracted from the remainder after each non-final
byte, so that no value has two encodings (128 is `00 80`, 16,511 is `7F FF`).

An edit moves nothing and changes no size, so every target byte is either the
source's byte at the same offset or an edited one. The encoder writes only
`SourceRead` and `TargetRead` runs, and joins two differing runs across an
unchanged gap shorter than three bytes, where a `SourceRead` header and a new
`TargetRead` header would cost more than the gap. The applier it is checked
against, `patch::apply_bps` (v1.2.0), implements all four actions.

Edits are addressed in PRG-ROM and CHR-ROM (`PatchRecorder::set_prg`,
`set_chr`), by offset within that section of the image, not as CPU or PPU
addresses: one offset is one byte of the file whatever bank maps it. The
debugger's ROM views use the same offsets (section 5). The image's layout comes
from its iNES or NES 2.0 header, a trainer included; the NES 2.0 exponent size
form is refused, and a CHR-RAM board has no CHR in the image to patch.

## 2. RAM pins: the sidecar

RAM is not part of the ROM image and BPS has no runtime semantics, so a RAM pin
cannot live in the patch. Pins go to `<stem>.pins.toml`, in the frontend's
existing raw RAM cheat format, the `[[raw]]` table of the per-ROM cheat file
(`address`, `value`, an optional `compare`, `enabled`), so the cheat panel's
loader reads it as a cheat file with no Game Genie codes:

```toml
[[raw]]
address = 1024
value = 92
enabled = true
```

The patch's metadata names the sidecar and its SHA-256:

```xml
<?xml version="1.0" encoding="UTF-8"?>
<patch format="rustynes-live-patch/1" generator="RustyNES 3.7.0">
  <ram-pins file="probe.pins.toml" sha256="..."/>
</patch>
```

`patch_recorder::verify_sidecar` refuses a sidecar of another name or hash.

## 3. Regression replay

`patch_recorder::replay_regression(clean, patched, movie, pins)` replays a
reference movie (`.rnm`, recorded on the clean ROM) on the clean image and on
the patched one in lockstep, applying the RAM pins after every frame on the
patched side exactly as raw cheats are applied. On the patched side the movie's
ROM identity is re-stamped with the patched image's, because a patched ROM is a
different ROM by design; every other check of `Movie::seek_to_start` (epoch,
region, board) still applies.

After each frame the two machines are compared, and the first difference is
reported with its cause, in this order:

1. the first differing CPU RAM byte, with both values;
2. else the first differing pixel;
3. else the CPU cycle count.

`rustynes patch-regress <ROM> <PATCH.bps> <MOVIE.rnm> [--pins FILE]` runs it:
exit 0 for no divergence, 4 for a reported one, 1 for an error.

## 4. Gates

In `patch_recorder::tests`:

- `numbers_use_the_specification_encoding`: the BPS number at its boundaries.
- `a_recorded_patch_reproduces_the_edited_image`: 13 PRG and CHR edits to
  nestest (`tests/roms/nestest/`), single bytes, adjacent runs, a short gap and
  each space's first and last byte; the recorded patch applied to the clean
  ROM equals the edited image byte for byte, and an empty recording changes
  nothing.
- `a_ram_pin_travels_in_the_sidecar_and_is_applied_on_replay`: one pinned
  address and no ROM edit; the patch leaves the image unchanged, the sidecar is
  named and hashed in the metadata (another name or one changed byte is
  refused), the replay reports `$0400` at frame 0, and a replay of its own
  reads the pin back at frame 30.
- `a_planted_regression_is_located_at_its_frame_and_cause`: a 37-byte fixture
  written for the test stores the first controller's A button at `$0010`
  every frame; the patch moves that store to `$0011`. The movie holds A from
  frame 40, and the replay reports frame 40, `$0010`, 1 clean and 0 patched.

Mutations caught: the BPS number's decrement removed; edited runs written as
`SourceRead`; pins applied only when they have a compare byte; the RAM
comparison skipped; the sidecar hash check skipped.

## 5. Live ROM editing in the debugger

The maintainer chose (2026-11-08) a live write API over reloading a patched
image: a ROM byte edited in the debugger changes the running game from the next
access, and the recorder captures every edit that lands.

### The core call

`Nes::debug_poke_rom(space, offset, value)` (`debug-hooks`, with
`debug_peek_rom` and `debug_rom_len` beside it) writes one byte of PRG-ROM or
CHR-ROM in place. `space` is a `RomSpace` (`Prg`, `Chr`); `offset` is the
image offset `PatchRecorder` uses. It reaches the board through two `Mapper`
methods, `debug_prg_rom_mut` and `debug_chr_rom_mut`, which return the board's
own ROM buffer and default to `None` ("not supported"). Every board keeps its
ROM in its own `Box<[u8]>` field (there is no shared storage type), so each
board that can offer it overrides the two methods, almost all with one line.

The CPU fetches the new byte at whatever address its current banking maps the
offset to, and the PPU draws the new pattern byte, from the next access on: no
board and no part of the core caches decoded ROM. `cpu_read`'s fast path in the
bus still calls the board. `Nes::rom_sha256` and `image_sha256` keep the clean
image's identity.

**Which boards.** `every_rom_buffer_a_board_offers_is_the_image_and_the_live_one`
(`rustynes-mappers`, `mapper.rs`) builds every iNES and NES 2.0 mapper number
the parser accepts, at several ROM shapes, and requires that each buffer a
board offers equals the parsed cartridge's section byte for byte, and that
complementing it changes what `cpu_peek` / `ppu_peek` returns (trying a spread
of register writes when the power-on banking maps none of it in). At v3.7.0, 204
mapper numbers offer PRG and 175 offer CHR. The exceptions are pinned in that
test:

| Section | Not offered | Why |
|---|---|---|
| PRG | 6, 8, 17 (FFE copiers) | the PRG is the copier's DRAM |
| PRG | 111 (GTROM) | the PRG is self-flashable flash and is the battery save |
| PRG | 30 (UNROM 512) when flashable | the same; the non-flashable configuration is offered |
| CHR | 13, 28, 29, 31, 40, 50, 63, 71, 73, 93, 94, 105, 162, 163, 177, 178, 179, 226, 227, 231, 232, 233, 242, 513, and the FFE copiers | the board's pattern memory is RAM the program writes; an image's CHR, where the parser accepts one, is ignored or loaded into it. `debug_poke_ppu` already writes it |
| both | FDS, NSF, Vs. `DualSystem` cabinets | not an iNES cartridge the recorder can lay out |

No CPU-address view (`$8000-$FFFF` to PRG offset through the current banking)
is offered: no board-independent translation exists (a handful of boards have a
private `prg_offset` helper), so it would be one more per-board method on 150
boards. The views work by offset, which is also what a patch records.

### The gate

The memory panel never writes ROM itself. An edit is queued as
`DebugPoke::Rom` and applied in the same post-frame stage as the other debugger
pokes (`EmuCore::produce_one_frame`), under its own gate, `rom_write_refusal`:

1. RetroAchievements hardcore refuses it;
2. a movie recording or playing refuses it (read from the movie state itself,
   not only from the app's `writes_locked`): a `.rnm` stamps the clean ROM's
   identity and would replay differently on it;
3. `writes_locked` (netplay, TAS replay) refuses it: the peers would desync.

A refused write is a no-op: it is drained, not deferred; the console, the
recorder and the frame are byte-identical to the same run with no write; and
the reason is shown in the memory panel ("refused: ..."). An accepted write is
applied to the console and to `EmuCore::rom_patch` together, or to neither: a
write is refused before the console changes when there is no recorder (FDS,
NSF, UNIF, a cabinet) or the board's section length differs from the image's.

### The debugger UI

The memory panel (`debugger/memory_panel.rs`) gains two domains, **PRG-ROM** and
**CHR-ROM**, listed by image offset (six hex digits). Click a byte, type a
value, Enter. The status line shows how many writes were recorded, the last
outcome, and **Save patch...**, which raises a request the app answers after
the egui pass (`App::save_live_patch`): it clones the recorder under a brief
emu lock, drops the lock, asks for a `.bps` path, and calls
`patch_recorder::write_patch_files`, which writes the patch and, when RAM pins
exist, the `<stem>.pins.toml` sidecar exactly as `PatchRecorder::finish`
defines. The RAM pins are the session's enabled raw RAM cheats and memory-panel
freezes (the same list applied after every frame), passed to `pin_ram` on a
copy of the recorder. Save is desktop-only; the browser build edits ROM but
drops the save request.

The recorder is started from the clean image at every ROM load
(`EmuCore::start_rom_patch`, from `App::rom_bytes`, the bytes the console was
built from, after any load-time header correction). Reinstalling the same image
(netplay start, movie load) restarts it with no edits, because the new console
has none; installing a different image drops it.

### Save states, rewind and movies

Save states carry no ROM, verified by `save_states_carry_no_rom_edit`
(`rustynes-core`): a state taken on a patched session is byte-identical to the
clean session's state at the same point. Loading it into a console of the clean
image runs the clean ROM; loading it (or rewinding) in the patched session keeps
the edit, because the ROM is not restored. `PPU_SNAPSHOT_VERSION` and the
`.rns` format are unchanged. To carry an edit across sessions, save the patch
and load the patched image.

Movies and netplay refuse ROM edits while they run, but an edit made before a
recording starts stays in the console, and the movie is stamped with the clean
image's identity. Such a movie replays correctly only on the patched image.

### Gates

- `rustynes-mappers`: `every_rom_buffer_a_board_offers_is_the_image_and_the_live_one`.
- `rustynes-core` (`--features debug-hooks`):
  `a_live_rom_write_is_read_through_the_current_banking` (a UxROM program
  fetches an edited byte of bank 1 at `$8010` and not an edited byte of the
  unmapped bank 0, and reads an edited CHR byte through `$2006` / `$2007`;
  past-the-end and CHR-RAM writes are refused) and
  `save_states_carry_no_rom_edit`.
- `rustynes-frontend`: `a_rom_write_is_refused_while_locked_recording_or_hardcore`,
  `rom_write_refusal_names_the_first_gate_that_holds`,
  `no_rom_write_means_a_byte_identical_session`,
  `queued_rom_edits_save_a_patch_that_rebuilds_the_live_rom` (edits through the
  queue, saved, then applied with `patch::apply_bps` to the clean ROM: equal to
  the live console's ROM byte for byte),
  `reinstalling_a_console_restarts_or_drops_the_recorder`, and the memory
  panel's `a_rom_view_edit_is_queued_not_written`.
