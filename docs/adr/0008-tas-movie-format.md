# ADR 0008 — TAS Movie (`.rnm`) Recording / Playback Format

**Status:** Accepted (v1.4.0 Sprint 4.1).
**Date:** 2026-05-24
**Author:** RustyNES maintainers
**Supersedes:** None.
**Relates to:** ADR 0003 (save-state migration policy) — the movie format
embeds the `.rns` save-state blob as one of its two "start point" variants,
so the movie's forward-compat story is layered on top of ADR 0003's.
**Numbering note:** Eighth in the `docs/adr/` sequence.

---

## Context

v1.4.0 introduces tool-assisted-speedrun (TAS) movie recording and playback.
The emulator already guarantees the hard determinism contract documented in
`CLAUDE.md`:

> Same seed + ROM + input sequence ⇒ bit-identical framebuffer and audio.

A movie is therefore nothing more than **a reproducible start point plus the
per-frame input stream that was applied on top of it.** No emulator-state
deltas, no frame hashes, no random-seed capture beyond what the start point
already pins down — replaying the recorded inputs from the recorded start
point re-derives every pixel and sample bit-for-bit.

This sprint lands the *core* infrastructure in `rustynes-core` only (a versioned
container, a `MovieRecorder` + `MoviePlayer`, and the per-frame input hook).
The frontend UI (record/stop hotkeys, on-screen frame counter, the input
display, branch tree visualisation) is Sprint 4.2.

### Constraints

- `rustynes-core` is `#![no_std] + alloc`. The movie module MUST compile on the
  `thumbv7em-none-eabihf` CI target. No `std`, no system time, no RNG, no
  threads, no float nondeterminism anywhere in the movie path.
- The container must be self-describing and versioned so a future
  save-state format change (ADR 0003) does not silently mis-replay an old
  movie.
- The format must be portable and reproducible byte-for-byte across
  platforms and builds (little-endian fixed-width integers only, exactly
  like the `.rns` save-state container).

## Structural references (clean-room; no verbatim copy)

| Emulator | Format | What we borrowed (structure only) |
|---|---|---|
| **Mesen2** (GPL-3.0, structural reference only) | `.mmo` (zip of KV-text settings + `Input.txt` + optional `SaveState.mss`) | The `RecordMovieFrom { StartWithoutSaveData, StartWithSaveData, CurrentState }` concept → our `StartPoint` enum; "store the ROM hash + format version in a header"; "optionally embed a save-state as the start point". We did NOT copy the zip/KV-text encoding — Mesen2's `MovieRecorder.h:MovieFormatVersion = 2` and `MovieTypes.h:MovieKeys`. |
| **FCEUX** | `.fm2` | The binary input-log convention: one byte per standard controller where `bit0=A, bit1=B, bit2=Select, bit3=Start, bit4=Up, bit5=Down, bit6=Left, bit7=Right`. This is **identical** to our `Buttons` bitflags layout, so a recorded frame's P1/P2 bytes are exactly `Buttons::bits()`. Also borrowed: required header keys `version` / `romChecksum` / `length`. |
| **TetaNES** (Rust cross-reference) | `.replay` | Confirms the Rust-emulator idiom: replay is a deterministic-input stream re-applied from a pinned power-up state (TetaNES pins RAM-init state; we pin it via the embedded start point + the core's seeded power-on phase). |

Sources consulted:

- Mesen2 `Core/Shared/Movies/{MovieTypes.h, MesenMovie.h, MovieRecorder.h, MovieRecorder.cpp}` (local clone).
- FCEUX FM2 spec: <https://fceux.com/web/FM2.html> and <https://fceux.com/web/help/fm2.html>.
- TetaNES: <https://github.com/lukexor/tetanes> (record/replay flags + deterministic RAM-init).
- NESdev TAS conventions: <https://www.nesdev.org/wiki/TAS>.

## Decision

A compact, versioned **binary** container, `.rnm` ("RustyNES Movie"), built
on the same `BinWriter` / `BinReader` primitives and section discipline as
the `.rns` save-state container (`crates/rustynes-core/src/save_state.rs`).

### On-wire layout

```text
HEADER:
    magic           : "RNESMOV1"     (8 bytes)
    format version  : u16 LE          (currently 1 = MOVIE_FORMAT_VERSION)
    region          : u8              (0 = NTSC, 1 = PAL, 2 = Dendy)
    flags           : u8              (bit0 = has-embedded-save-state start
                                       point; bits 1-7 reserved, MUST be 0)
    rom sha-256     : [u8; 32]        (full hash — authoritative ROM identity)
    frame count     : u32 LE          (number of input frames that follow)
    bytes per frame : u8              (currently 3: P1, P2, expansion-reserved)

START POINT (only present when flags bit0 set):
    length-prefixed `.rns` save-state blob (u32 LE length + bytes)
    Absence (bit0 clear) means "start from a fresh power-on of this ROM".

INPUT STREAM:
    frame_count * bytes_per_frame raw bytes.
    Each frame = [p1, p2, expansion] where p1/p2 are `Buttons::bits()`
    (bit0=A .. bit7=Right) and `expansion` is reserved (currently always 0).
```

Integers are little-endian, matching `.rns`. The header is fixed up to the
`bytes_per_frame` byte; the start point and input stream follow.

### Start point: `StartPoint` enum

```rust
pub enum StartPoint {
    /// Power-on this ROM fresh, then apply the input stream from frame 0.
    PowerOn,
    /// Restore this embedded `.rns` snapshot, then apply inputs from there.
    /// Enables save-state branching (a movie that begins mid-game).
    SaveState(Vec<u8>),
}
```

This is the clean-room analogue of Mesen2's `RecordMovieFrom`. We collapse
`StartWithoutSaveData` → `PowerOn` and both `StartWithSaveData` /
`CurrentState` → `SaveState(blob)` because, from the *replay* side, both are
just "restore this blob and go" — the distinction (battery vs full state)
only matters at *record* time and is the frontend's concern.

### Per-frame input hook

`run_frame()` is **not modified** — the determinism contract is preserved by
keeping the core's frame loop untouched. Instead:

- **Recording** is driven by the caller: each frame, after the frontend
  calls `set_buttons(0, p1); set_buttons(1, p2)`, it calls
  `recorder.capture(&nes)` (which reads `Nes::buttons(0)` / `Nes::buttons(1)`
  — a new read-only getter) *before* `run_frame()`. This records exactly the
  inputs that frame consumes.
- **Playback** is also caller-driven: each frame the caller asks the player
  for the next frame's `[p1, p2]`, applies them via `set_buttons`, then calls
  `run_frame()`. A convenience `MoviePlayer::apply_next(&mut nes) -> bool`
  does the `set_buttons` for the caller and returns `false` at end-of-movie.

Keeping the hook caller-side (rather than swallowing `run_frame`) means the
movie path adds **zero** branches to the hot frame loop and cannot perturb
determinism.

### Save-state branching (data-level)

A movie recorded with `StartPoint::SaveState(blob)` begins from a restored
snapshot. The frontend's "branch here" gesture (Sprint 4.2 UI) maps to:
take `nes.snapshot()` at the branch frame, start a *new* `MovieRecorder`
seeded with that blob as its start point, and continue. The core mechanism
(record-from-snapshot + replay-from-snapshot) is implemented and tested in
this sprint; only the UI/tree visualisation is deferred.

### no_std / SHA-256 decision

**Chosen: `rustynes-core` computes and stores the full 32-byte hash itself.**
`rustynes-core` already depends on `sha2` and already computes `rom_sha256` at
`Nes::from_rom` time (`nes.rs:76`); the `std` cargo feature forwards to
`sha2/std` while the default-features-off build uses `sha2`'s no_std mode
(proven by the existing `thumbv7em-none-eabihf` gate). The movie format
therefore reuses `Nes::rom_sha256()` directly — no caller-passed hash, no new
dependency, and the no_std build stays green. We store the **full** 32 bytes
(not the 6-byte truncated tag the `.rns` header uses) because a movie is a
shareable artefact where ROM-identity collisions matter more than in a
local slot file.

### Forward-compatibility contract (layered on ADR 0003)

- `MOVIE_FORMAT_VERSION` gates the *container*. A reader rejects any movie
  whose `format version > MOVIE_FORMAT_VERSION` with a clean
  `MovieError::UnsupportedFormat` (never a panic).
- The embedded `.rns` start-point blob carries its own `FORMAT_VERSION` and
  per-section version bytes, so ADR 0003's three-tier policy governs whether
  an old movie's embedded state still restores on a newer build. A movie
  recorded from `StartPoint::PowerOn` has no embedded state and is therefore
  the most durable across version transitions (it depends only on the ROM
  and the deterministic power-on).
- `bytes_per_frame` is stored explicitly so a future expansion-port byte (a
  3rd device, e.g. a Zapper or Four Score) can grow the per-frame record
  *without* a container version bump — readers that understand fewer bytes
  per frame than the file declares fail cleanly rather than mis-parse, and
  the reserved 3rd byte is already allocated.

## Consequences

### Positive

- Movies are tiny: ~3 bytes/frame (≈10.8 KiB/minute at 60 fps), and the
  power-on start point adds nothing. The format is trivially streamable and
  diff-friendly.
- Reuses the battle-tested `BinWriter`/`BinReader`/section machinery; no new
  parsing surface to fuzz beyond the thin header.
- The hot frame loop is untouched; determinism is structurally guaranteed.

### Negative / Costs

- Raw (un-RLE'd) input means a held-button-heavy movie is slightly larger
  than a delta/RLE encoding would be. Accepted: 3 bytes/frame is already
  negligible, and raw bytes are deterministic and seek-trivial (frame N is at
  a constant offset), which matters for the Sprint 4.2 scrubbing UI.
- Embedding a save-state start point couples movie durability to the `.rns`
  format's stability (mitigated by `StartPoint::PowerOn` movies and ADR 0003).

### Neutral

- The format is independent of the host: little-endian fixed-width fields,
  no platform types, compiles on `thumbv7em-none-eabihf`.

## Alternatives considered

1. **Text format like FCEUX `.fm2` / Mesen2 KV-text.** Rejected for the
   core: text parsing pulls in formatting/allocation churn and is harder to
   keep byte-deterministic in `no_std`. A binary container matches the
   existing `.rns` design and is leaner. (A future export-to-`.fm2`
   converter could live in the frontend.)

   *Update (v1.6.0 B1):* `.fm2` (FCEUX) **and** `.bk2` (BizHawk) **interop**
   now exist as separate, opt-in import/export converters
   (`movie_interop` / `bk2_interop` in `rustynes-core`) — the `.rnm` binary
   container stays the native format. Both text parsers are `no_std`-clean
   (the `.bk2` ZIP container is read / written frontend-side); imported movies
   become `StartPoint::PowerOn` `.rnm` movies that replay from the canonical
   deterministic cold boot, so the interop never weakens the determinism
   contract.

2. **RLE / delta-encoded input stream.** Rejected for v1 as premature: 3
   bytes/frame is already tiny, and raw bytes give O(1) random frame access
   for the scrubbing UI. Can be added later behind a `bytes_per_frame=0`
   "compressed" sentinel + a container version bump without breaking the
   header.

3. **Capturing periodic frame hashes for desync detection.** Useful for
   diagnosing a determinism *bug*, but the contract says there are none; a
   desync would be a regression caught by the existing oracle/visual-baseline
   suites, not something the movie format should police. Deferred (could be
   an optional trailing section later).

4. **Reusing the `.rns` section container verbatim** (tag/version/length
   sections) for the whole movie. Rejected: the input stream is a single
   large homogeneous blob, not a set of heterogeneous tagged sections, so a
   purpose-built fixed header + raw stream is simpler and smaller. We still
   reuse `BinWriter`/`BinReader` for the field encoding.

## Test plan

Implemented in `crates/rustynes-core/src/movie.rs` (`#[cfg(test)]`) and the
workspace integration suite:

1. **Determinism round-trip**: record a fixed synthetic input sequence on a
   committed ROM, then replay from the movie's start point and assert the
   framebuffer FNV-1a, audio FNV-1a, and cumulative cycle count are
   byte-identical between the original run and the replay.
2. **Save-state branch**: restore a mid-stream snapshot, continue on a new
   branch recorder, and assert both the original and branch replays are each
   internally deterministic (re-running yields identical output).
3. **Format round-trip**: `serialize` → `deserialize` → structural equality;
   bad magic / too-new format version produce a clean `MovieError`, not a
   panic; a truncated input stream is rejected cleanly.
