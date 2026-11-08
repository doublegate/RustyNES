<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# The live-patch recorder and regression replay (v3.7.0)

`T-HACK-BPS-RECORDER`, `crates/rustynes-frontend/src/patch_recorder.rs`, and
the `rustynes patch-regress` command.

A ROM hack is made a byte at a time: a PRG byte to change a rule, a CHR byte to
change a tile, a RAM pin to hold a value while testing. The recorder turns those
edits into the two files the hacking community exchanges, and the regression
replay checks the result against reference play.

Frontend-only: nothing here runs inside the emulation core, so the determinism
contract, every golden and the emulation epoch are untouched.

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
`set_chr`), not as CPU or PPU addresses: the debugger resolves an address
through the mapper's current bank before recording it. The image's layout comes
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

## 5. Not done

The debugger has no PRG-ROM or CHR-ROM edit path: the core ignores CPU writes
to ROM, and `debug_poke_ppu` writes CHR-RAM only. The recorder is therefore a
library and a command; a debugger panel that edits ROM bytes (and, to see the
edit live, reloads the patched image) is the remaining step, and needs a
decision on whether ROM edits apply live through a core API or by a reload.
