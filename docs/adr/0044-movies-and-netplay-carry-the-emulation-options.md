# 44. Movies and netplay carry the emulation options

Date: 2026-10-01

## Status

Accepted. Decided by the maintainer on 2026-10-01 ("a replay or a peer can
never silently diverge"), as part of the v2.9.8 work that lands every
breaking improvement now. **Supersedes alternative 2 of
[ADR 0028](0028-v2-0-0-save-state-and-movie-format-break.md)** ("reject
pre-v2.0.0 movies outright ... rejected"): from `.rnm` format 3 older movies
are refused. ADR 0028's epoch mechanism itself is unchanged and is what this
uses. [ADR 0008](0008-tas-movie-format.md)'s format description is extended,
not replaced.

## Context

The determinism contract is "same ROM + same seed + same input ⇒
bit-identical output". A `.rnm` recorded the ROM and the input; netplay's
handshake compared the ROM. Neither recorded the host settings that change
what the console computes: the console model (NES / Famicom, v2.9.8), the PPU
and 2A03 die revisions, OAM decay, the power-on RAM fill, the power-up
palette, the overclock, the Four Score, the Zapper light model, the Vs. DIP
switches and PPU type, the mirroring override and Game Genie codes. Each was
whatever the player had set, so a replay on a differently configured machine
diverged without a word, and two netplay peers connected and then desynced.

Three further facts:

- From v2.9.8 `Nes::rom_sha256` excludes the 16-byte header (`084daf28`), so
  two images with the same body and different headers -- another region,
  mapper or mirroring, or a changed game-database correction -- share a ROM
  identity while building different machines.
- `Nes::power_cycle` rebuilds the PPU, which drops OAM decay and the
  overclock, so a power-on recording ran without them whatever the player had
  set, and nothing said so.
- `FrameInput` held two players, so a four-player session recorded half of
  what drove it, and the `.fm2` importer dropped pads 3 and 4.

## Decision

1. **One canonical record.** `rustynes_core::HardwareOptions` holds every
   host-settable option that changes emulation; `BoardDescription` holds the
   parsed cartridge board. Both have a strict, explicit-byte encoding: an
   unknown enum byte or a malformed field is an error, never a default.
2. **`.rnm` format 3.** The options and (for a recorded movie) the board go in
   a length-prefixed block after the fixed header; the per-frame record widens
   to 5 bytes (P1-P4, expansion). `MIN_MOVIE_FORMAT_VERSION` is 3: an older
   movie does not say which machine it ran on, so it is refused with an error
   that says to re-record it.
3. **Playback applies, then checks what it cannot apply.** `seek_to_start`
   refuses another ROM, region or board (naming the field), then applies the
   options before the start point. Hosts hold them in place every frame and
   put the player's back when playback stops.
4. **Netplay refuses, it does not adopt.** The `Sync` handshake carries a
   `SessionIdentity`: the ROM hash plus a SHA-256 over region, board and
   options. A mismatch is its own disconnect reason. The guest does not adopt
   the host's options: adoption is the same silent change in the other
   direction, and the power-on options would need a coordinated re-power-on
   after the handshake. A refusal is explicit and costs one retry.
5. **`FrameInput` gains `p3` / `p4` and becomes `#[non_exhaustive]`.** The
   next field (an expansion-device byte, a soft-reset command) is foreseeable;
   the format already stores its per-frame width, so `#[non_exhaustive]` makes
   the API side additive too. Constructors (`new`, `four_players`) and public
   fields stay the way to build and edit a frame.
6. **Imports state the stock NES.** `.fm2` / `.bk2` / `.fcm` / `.fmv` / `.vmv`
   record `HardwareOptions::default()` explicitly, no board, plus the one
   option a format declares (`.fm2`'s `fourscore`).

What is recorded and what is not, knob by knob, is the survey table in
`docs/frontend.md` § "What a movie records and a netplay peer must match".

## Consequences

### Positive

- A movie replays the machine it was recorded on whatever the player's
  settings, or refuses with a reason. Two peers either run the same machine
  or are told why not.
- Four-player recordings and `.fm2` imports keep players 3 and 4.
- Power-on movies keep OAM decay and the overclock through their power cycle.

### Negative

- Every `.rnm` written before v2.9.8 is refused. The maintainer accepted this.
- Netplay peers on v2.9.7 and v2.9.8 never sync (`PROTOCOL_VERSION` 5; a
  v4 `Sync` is too short to decode).
- `FrameInput` and the netplay constructors are API breaks.
- Settings changed while a movie plays take effect when it stops; while a
  movie records, at the next ROM load or power cycle.

### Neutral

- Emulation output is unchanged: the default options are the default build.
- Inputs that are not controller buttons (expansion devices, the microphone,
  Vs. coins / service, FDS disk events) are still not in the input stream; the
  documented limit is unchanged.
- A change of options during a netplay session is detected by the existing
  desync checksum, not prevented.
