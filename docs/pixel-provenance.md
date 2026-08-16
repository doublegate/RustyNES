# Pixel provenance

> **Status:** all four phases implemented (write attribution, per-pixel
> provenance, the inspector panel, replay attestation). This document is the
> spec, so it is updated in the same change as the code it describes.
>
> **v2.3.6 — the feature did not work as shipped in v2.3.2, and this document
> was part of the reason.** Two defects, and a doc claim covering each. See
> [Two defects, and what they cost](#two-defects-and-what-they-cost) below.

## What the feature answers

Click any pixel on screen — or type its coordinates — and get the full causal
chain that produced it:

1. the PPU **dot and scanline** that emitted it;
2. the **background tile** behind it — nametable address, attribute address,
   pattern-table address, and the bytes actually fetched;
3. which **sprite**, if any, won priority at that pixel;
4. the **palette entry** the final color came from;
5. and, for each of those bytes, **the CPU instruction and cycle that last wrote
   it**.

Steps 1-4 are recoverable from state the PPU already holds. Step 5 is not, and it
is the step that turns "here is what the hardware did" into "here is what your
code did". It is the reason this subsystem exists.

## Why this is not just wiring up existing panels

RustyNES already ships every ingredient except one, and it is worth being precise
about which piece was missing, because the shape of the gap determined the design:

| existing tool | has | lacks |
|---|---|---|
| Trace Logger (`Nes::trace`) | PC, registers, cycle | any link to an effect |
| Event Viewer (`LockstepBus::events`) | the `$2000-$3FFF` CPU write, its PPU scanline/dot | the PC, and the *resolved* destination |
| memory access counter (`debugger/access_counter.rs`) | per-address read/write counts, last-access cycle | the PC |
| HD-pack tile source (`HdTileSource`) | per-pixel tile/palette/sprite context | write history |

Nothing recorded the edge from *a byte in PPU memory* back to *the instruction
that stored it*. That edge is Phase 1.

## Phase 1 — write attribution (implemented)

`crates/rustynes-ppu/src/provenance.rs` stores, for every byte of the PPU's own
memories, the PC, CPU cycle, and value of the write that last stored it:

| memory | entries | index |
|---|---|---|
| CIRAM (internal nametables) | 2048 | physical offset, mirroring resolved |
| OAM | 256 | byte index |
| palette RAM | 32 | post-mirroring index |

### Where the record is stamped, and why there

Attribution is **split across the bus/PPU boundary** because neither side knows
enough alone:

- The **bus** knows the program counter. The PPU never sees it.
- The **PPU** knows the effective destination. The bus never sees it: a nametable
  byte is written by a `STA $2007` whose target lives in the PPU's internal `v`
  register, set earlier by two `$2006` writes; the same `$2007` store lands in
  palette RAM for a different `v`; an OAM byte arrives either through `$2004` or
  as one of 256 bytes of a DMA burst.

So `Nes::run_frame` pushes the executing instruction's `(pc, cycle)` down to the
PPU once per instruction — in the block that already performs the breakpoint
check and the trace push, so no new `CpuBus` hook and no change to
`rustynes-cpu` were needed — and the store sites (`Ppu::write_vram`,
`Ppu::write_palette`, `Ppu::oam_dma_write`, and the `$2004` write path) stamp it.
`Nes::step_instruction` mirrors the push so single-stepping attributes correctly.

### OAM DMA is attributed to its trigger, not to its victim

`STA $4014` does not perform the transfer. It arms `dma_pending`, and the 513 or
514 DMA cycles are then stolen from the instructions that *follow*. Using the
live context would therefore name whichever instruction was being halted — true
about the timing and wrong about the cause.

The bus calls `Ppu::latch_dma_attrib_context()` in the `$4014` write arm, freezing
the triggering instruction, and `oam_dma_write` records against that. All 256
bytes name the one store that caused them. `oam_dma_attributes_all_256_bytes_to_the_triggering_store`
pins this; it initially failed, reporting the `JMP` that followed the trigger,
which is what surfaced the distinction.

### What is deliberately not attributed

- **CHR writes (`$0000-$1FFF`)** — that window is owned by the mapper and may be
  ROM, CHR-RAM, or a banked board window, so a byte offset is not a stable
  identity across a bank switch the way a CIRAM offset is. Pattern-table
  provenance is reported as the mapper bank, which the mapper already knows,
  rather than faked with an unstable offset.
- **Mapper-supplied nametable memory** (MMC5 ExRAM, 4-screen boards) — absorbed
  by `PpuBus::write_nametable` before reaching CIRAM.
- **The blocked `$2004`-during-rendering write** — the hardware quirk discards the
  value and only bumps `OAMADDR`. Recording it would attribute a byte to an
  instruction that demonstrably did not write it.
- **Sprite-evaluation OAM corruption** — not a CPU write; it has no PC.

### Lifetime and invalidation

Both stores — write attribution and the per-pixel frame — are cleared on
**power-cycle** and on **both save-state restore paths**
(`Nes::restore` and `Nes::restore_quiet`). A restored state's bytes were not
written by any instruction this session executed, so the PCs recorded against
those offsets describe a timeline that no longer exists.

**Run-ahead is carried around that clear, not through it** (v2.3.6).
`RunAhead::finish` moves both stores out with `Nes::take_provenance`, lets
`restore_quiet` run, and puts them back with `Nes::put_provenance`. The records
kept are the **visible** frame's — one frame ahead of the restored persistent
state, and exactly the frame on screen.

This is the one caller that needs the exception, and not because its restore is
different: because of *when* it happens. Run-ahead's rollback is the last thing
before the frontend releases the emulator lock, so the UI's first chance to look
is always after it. Clearing there discards the frame the user is looking at
rather than a stale timeline. Every other caller — a user-driven save-state load,
netplay rollback — still clears, and still should. The stash is a move of two
boxed stores, skipped entirely when neither is armed.

> **This paragraph used to say the opposite of the code.** Until v2.3.6 it read:
> "Under run-ahead the per-frame `restore_quiet` therefore leaves exactly the
> visible frame's writes — which is the timeline the user is looking at." The
> clear emptied both stores completely, and an identical claim sat in a comment
> beside the clear itself. Since run-ahead defaults to 1, the shipped inspector
> showed an empty report to every user for four releases. The prose asserting the
> intent is what stopped anyone checking the code against it.

### Cost and the determinism contract

The whole module is `debug-hooks`-gated, and even under that feature the store is
allocated lazily: `Ppu::write_attribution()` is `None` until armed, so an unarmed
`debug-hooks` build pays one `Option` discriminant test per PPU-memory write.
Armed, it costs `WriteAttribution::HEAP_BYTES` (~37 KiB) and one store per write.

Nothing in the render, timing, or audio path reads any of it. Framebuffer, audio,
and cycle counts are bit-identical armed or unarmed, and the default (no
`debug-hooks`) build is unchanged.

### API

```rust
nes.set_write_attribution(true);          // arm (allocates)
let a = nes.write_attribution().unwrap(); // Option<&WriteAttribution>
a.ciram(offset);                          // Option<WriteAttrib { pc, cycle, value }>
a.oam(index);
a.palette(index);
nes.clear_write_attribution();            // forget records, stay armed
nes.set_write_attribution(false);         // disarm (frees)
```

Every accessor masks its index, so a provenance query cannot panic on an
out-of-range offset — a debugging convenience must never be able to take down the
emulator it is inspecting.

## Phase 2 — per-pixel provenance record (implemented)

The plan was to widen the existing `hd-pack` `HdTileSource` gate. That turned out
to be the wrong shape, and the reason is worth recording: `HdTileSource` carries
Mesen-format HD-pack *keys* (`palette_colors` packed for tile identity, an
absolute CHR-ROM tile index, the four covering sprites), and widening its gate
would have dragged eight `hd-pack` fetch-telemetry fields into every `debug-hooks`
build whether or not the panel was open. Provenance wants addresses, not keys.

So Phase 2 adds a **separate, lazily-allocated record** in the same shape as
Phase 1's attribution store. `hd-pack` is untouched — byte-identical by
construction rather than by careful review.

### What each pixel records

`PixelProvenance` (`crates/rustynes-ppu/src/provenance.rs`) holds the emitting
scanline and dot, the winning layer (`Backdrop` / `Background` / `Sprite`), the
exact `$3Fxx` palette address and its post-mirroring index, the resulting 6-bit
color and the `$2001` grayscale/emphasis bits, the displayed tile's nametable /
attribute / pattern addresses, both layers' pattern and palette indices, the
winning sprite slot with its priority and sprite-0 flags, and fine-X / fine-Y.

`palette_index` indexes `WriteAttribution::palette` directly, and `nt_addr`
resolves through the mapper's mirroring to a CIRAM offset for
`WriteAttribution::ciram` — so the two phases compose into the full chain. The
test `provenance_and_attribution_compose_into_a_causal_chain` pins exactly that:
pixel → palette entry → writing instruction.

### The address cascade, and why `v` cannot answer

By the time a tile's pixels reach the screen, `v` has advanced two tiles past it.
Deriving the nametable address from `v` at emit time would be wrong for every
pixel, and wrong in a way that looks plausible. So a `ProvBgAddrs { nt, at,
pattern }` triple rides the same `latch` → `next` → `cur` cascade that moves the
pattern bytes through the shift registers.

Two findings came out of building it, both from the test failing first:

- **The tile is defined when its PATTERN is fetched, not when its NT byte is
  read.** The PPU performs two *dummy nametable fetches* at dots 337-340, after
  the pre-render line's last real tile is fetched but before the visible line's
  first reload consumes it. Writing the NT address straight into the latch let
  those dummies overwrite the pending tile, so pixels x=8..15 reported the tile
  belonging to x=16..23. The addresses are now held pending and committed in
  `fetch_bg_lo`, which the dummy fetches never reach.
- **The attribute address is carried, not derived.** An MMC5 vertical split
  supplies its own attribute address that the standard `$23C0 | ...` arithmetic
  cannot reproduce.

`at` is carried for the same reason.

### Cost

Guarded on a plain `bool` (`Ppu::prov_armed`) rather than `Option::is_some()`:
`emit_pixel` runs 61,440 times a frame and is one of the two hottest functions in
the emulator, so the unarmed cost is one predicted branch on an already-hot cache
line rather than a discriminant behind a pointer chase — the same shape as the
bus's `event_logging` flag. Armed, a frame costs
`PixelProvenanceFrame::HEAP_BYTES`.

The one shipped-path change is in `emit_pixel`'s priority chain: it now yields
the palette *address* and performs a single `read_palette` afterwards, instead of
reading inline in each arm. `read_palette` is a pure read whose greyscale mask is
untouched by the sprite-0-hit insert, so hoisting it is behaviour-preserving.

### Not recorded, and why

- **The primary OAM sprite number.** Sprite evaluation copies bytes from primary
  to secondary OAM without retaining the source index, so it does not exist at
  emit time. The record reports the secondary-OAM *slot*; the panel matches its
  Y/tile/attribute against OAM rather than being handed an index the PPU never
  kept.
- **Sprites that lost the priority decision.** Phase 2 records the winner. The
  `hd-pack` path already collects up to four covering sprites for its own
  conditions; folding that in is a Phase 3 concern if the panel wants it.

Because the primary index is absent, the panel shows **no write-attribution row
for OAM**. `WriteAttribution::oam` is keyed on primary OAM byte addresses, so
indexing it with a secondary slot would confidently name a different sprite's
writer whenever evaluation skipped an earlier one. The first version of the panel
did exactly that, a paragraph after documenting that the index does not exist;
caught in review on PR #356.

The per-pixel frame is also cleared on restore, for the same reason attribution
is. The framebuffer analogy does not excuse skipping it: the framebuffer *is*
serialized and returns consistent with the restored state, whereas this frame is
not, so a restore landing mid-frame would leave pre-restore addresses for every
pixel above the current scanline with nothing marking them stale. The first
version of this document argued the opposite.

## Phase 3 — the inspector panel (implemented)

`crates/rustynes-frontend/src/debugger/provenance_panel.rs`, reached from
**Tools → Pixel Provenance**, registered through the shared `detachable_window`
helper so it inherits v2.3.0's multi-viewport pop-out.

The panel pins a screen coordinate and reports, in four sections:

1. **Emitted** — scanline, dot, screen X, the winning layer, the palette colour
   swatch, and the grayscale/emphasis bits in effect.
2. **Palette** — the address pre-mirroring (so `$3F10` shows as `$3F10`, the
   address the program used), the entry index, and the instruction that last
   wrote it.
3. **Background tile** — nametable and attribute addresses with their resolved
   CIRAM offsets and the instructions that wrote those bytes, the palette group,
   the pattern bits, fine scroll, and the pattern address. Shown for sprite
   pixels too, since the background is what the sprite won priority *over*.
4. **Sprite** — slot, priority, pattern bits and address, and the sprite-0 flag.
   Deliberately **no OAM write-attribution row**: the primary OAM index does not
   exist at emit time, so naming a writer would name a different sprite's. See
   "What phase 2 cannot answer" above, which this line used to contradict.

**Selecting a pixel.** Click anywhere on the game view to pin that pixel, or set
the X/Y spinboxes directly. The click is captured in the winit mouse handler
rather than as an egui `Response`, because the NES image is a raw wgpu blit and
not an egui widget — there is nothing to hit-test. `gfx::window_to_nes_pixel`
does the conversion by inverting the blit's own letterbox/crop transform, so it
is correct at any window size, pixel-aspect setting and overscan crop, and a
click on a letterbox bar pins nothing rather than silently pinning an edge pixel.

Arming is in the panel: two checkboxes for the provenance frame and the
attribution store, both default off. This is the panel's only side effect on the
emulator, and both stores are determinism-neutral. The checkbox state is read
from the core every frame rather than mirrored in the panel, so loading a ROM
(which installs a fresh `Nes`) cannot desync it.

When the panel has no answer it says which kind of "no answer" it is — not armed,
armed but nothing recorded for this pixel yet, or off-screen. A cleared record is
a valid `PixelProvenance` whose every field reads as a confident "scanline 0,
dot 0, backdrop, palette `$0000`", so without that check an empty frame renders
as fact. `PixelProvenance::is_recorded()` is the discriminator: `emit_pixel`
stamps `dot` on every pixel it records and the visible dots are `1..=256`, so
dot 0 is unreachable for a real record and is exactly what `clear` leaves.

## Two defects, and what they cost

Recorded because the shape of the failure matters more than either bug.

**Defect 1 — run-ahead wiped the record before the UI could read it.** Covered
under [Lifetime and invalidation](#lifetime-and-invalidation) above.

**Defect 2 — clicking a pixel was never implemented.** The panel offered two
`DragValue` spinboxes and contained no click hit-test at all, while this document
opened with "Point at any pixel on screen" and the release notes said "pin a
screen pixel". Two later lines here — the panel "pins a screen coordinate" and
"the coordinate-picker shape follows `hd_pixel_panel.rs`" — described the real
behaviour accurately, so the document contradicted itself and the wrong half was
the one users read first.

**What both have in common:** the core data structures were well covered by unit
tests, and the frontend wiring was covered by nothing. No test drove the produce
path with run-ahead on; no test asked whether a click could reach the panel. The
same shape produced issue #360 in the same release train, where `MovieUi::after_frame`
worked in production but no test ever called it. `runahead.rs` even carried tests
pinning the determinism of the exact code path that destroyed this telemetry.

The regression net added in v2.3.6:
`runahead::tests::runahead_preserves_pixel_provenance` (with
`plain_run_leaves_pixel_provenance_populated` as its control, so a failure cannot
be misread as a bad assertion), `runahead_preserves_the_provenance_arm`, and
three `gfx::tests::window_to_nes_pixel_*` tests — one of which round-trips the
picker against the shader's own uniform rather than against a third re-derivation
of the letterbox.

### Reuse, and one thing deliberately not reused

The coordinate-picker shape follows `hd_pixel_panel.rs`; PC → source-line
resolution reuses `source_map.rs` (a bonus row, present only when the user has
loaded a `.dbg`). The read-only-over-`&Nes` wiring — `show_*` flag, `*_ui` state,
`ToolPanel` variant, `any_nes_tool_open` — follows `rom_info_panel.rs`.

`event_panel.rs`'s scanline × dot timeline was **not** folded in. The record
already carries its own scanline/dot, and a second timeline widget showing one
point would be decoration rather than information.

### Nametable address → CIRAM offset

`Nes::ciram_offset_for_nametable_addr` resolves a PPU-space address to the offset
`WriteAttribution::ciram` is keyed on, sharing `resolve_nt_addr` with the PPU's
own fetch path so a board with a per-game mirroring override reports the offset
its fetches really use. On boards with mapper-supplied nametable memory (MMC5
ExRAM, 4-screen) some writes never reach internal CIRAM; the function still
returns the standard-mirroring offset, because the only way to know whether the
mapper absorbed a particular write is to *perform* one — `write_nametable` takes
`&mut self` and has side effects. A missing attribution on such a board means
"the mapper owns this byte", and the panel says so rather than showing nothing.

### One wiring trap

The panel is **not** gated on the frontend's `debug-hooks` feature. That feature
is an alias: `rustynes-frontend` always pulls `rustynes-core` with `debug-hooks`
on, so the core API is always present, but the alias itself is off by default.
Gating on it would have shipped the panel permanently unreachable. Caught by the
menu entry compiling against a nonexistent icon glyph without erroring — the
whole block was being `cfg`'d out.

## Phase 4 — deterministic replay attestation (implemented)

A separate feature in the same release. It reuses the determinism contract rather
than the provenance store: because the core re-derives every pixel from the same
ROM plus the same inputs, a recorded hash of a run's output is something anyone
else can independently re-derive.

```console
$ rustynes verify run.rnm --rom game.nes
Verifying run.rnm against game.nes
  150 frames to replay...
VERIFIED: 150 frames reproduced exactly (hash 23248f1a4b49f4e6).
```

Exit codes are distinct on purpose: **0** verified, **1** mismatch or error,
**3** the movie carries no attestation. A movie that makes no claim has not
failed, and collapsing the two would leave a script unable to tell them apart.

### What a `Match` does and does not mean

It means *these inputs, applied to this ROM, on a verifier configured like the
recorder, produce this video*. Two limits are load-bearing:

- **Tamper-evident, not forgery-resistant.** The digest is 64-bit FNV-1a: not
  collision resistant, and its round function is invertible. It catches
  accidental divergence and casual edits; a motivated forger can edit the movie
  and recompute it. Establishing authorship would need a signature over the whole
  record with a key the verifier trusts — a different feature.
- **The verifier assumes a default core profile.** `rustynes verify` builds a
  plain `Nes` from the ROM bytes. A recording made with Four Score, a PPU
  die-revision or power-on RAM model, a per-game database override, or a
  soft-patched ROM will not reproduce, and that mismatch is the profile's fault
  rather than the movie's. The format carries no profile field, so the CLI states
  the assumption up front instead of mis-blaming the movie. Recording-side
  eligibility is follow-up work.

Both were narrowed in review on PR #356, where the prose had drifted into
"prove it is genuine and unmodified".

### No format version bump

The plan called for a "v3 tail". It turned out not to be needed: `.rnm` already
had a precedent for additive trailing fields — `rerecord_count` is read with
`r.u32().unwrap_or(0)`, so a reader that stops earlier simply ignores it. The
attestation is appended the same way behind an `ATTESTATION_MAGIC` marker, so
`MOVIE_FORMAT_VERSION` stays at 2 and every existing movie round-trips unchanged.
A pre-v2.3.2 reader parses an attested movie as a plain one; the test
`attested_movie_stays_readable_as_a_plain_movie` pins that by truncating the tail
and reparsing.

### What is attested, and the mistake that shaped it

Per frame: **the input applied, and the framebuffer it produced**, folded into
one rolling hash, with a checkpoint every
`ATTESTATION_CHECKPOINT_INTERVAL` (64) frames so a mismatch reports a 64-frame
window rather than just a verdict.

The first implementation hashed the framebuffer alone. An end-to-end tamper test
then flipped a button bit in a movie recorded against a test ROM that never reads
the controller — and `rustynes verify` confirmed the tampered movie as genuine.
It was right to: the video output really was identical. But it made the wrong
claim. Output alone does not pin the input stream, and a ROM that ignores input
(a test ROM, an attract-mode demo, a cutscene) makes an output-only hash useless
as evidence. Folding the input in makes the claim the honest one: *these inputs,
applied to this ROM, produced this output.*
`flipped_input_fails_even_when_the_rom_ignores_input` keeps it folded in.

**Hashing the core snapshot instead** would be strictly stronger at detecting
divergence, and was rejected: the snapshot schema is versioned and bumps between
releases (`PPU_SNAPSHOT_VERSION` has reached 8), so every bump would silently
invalidate every previously-recorded attestation. A 256x240 RGBA framebuffer is
stable for as long as the NES is the NES, and an attestation is only worth
recording if it can still be checked years later.

**Audio is not covered.** Samples are drained by the host as they are produced,
so the core cannot see a whole run's audio without the frontend cooperating.
Saying so is better than implying coverage that is not there.

### Recording, and the run-ahead interaction

`MovieRecorder::enable_attestation()` arms it; `attest_frame()` folds in each
completed frame. The frontend arms it automatically when recording starts —
**unless run-ahead is on**, in which case it says so and records a plain movie.

Run-ahead presents the frame *N* ahead of the persistent timeline, while a
verification replay has no run-ahead and re-derives persistent frames. Attesting
the presented image would record a hash nobody can reproduce. `emu.rs` therefore
folds in frames only on the non-run-ahead path, and if run-ahead is toggled on
mid-recording the attestation's frame count falls short of the input stream's —
which `Movie::deserialize` detects and drops the tail for. The failure mode is
"no attestation", never "a wrong one".

A successful **rewind during recording** also drops the attestation: a rewind
restores an earlier state while the input log keeps its full prefix, so the frame
counts stay self-consistent and `Movie::verify` would report `Mismatch` on an
honest recording. "Not attested" is the truthful outcome.

Two other paths deliberately produce no attestation: a **history-viewer export**
(the rewind ring stores state, not the per-frame video an attestation hashes) and
a **TAStudio export** (an edited input stream has no single continuous run whose
output could honestly be described).

## Verification

- `write_attribution_names_the_instruction_that_wrote_a_nametable_byte` — a
  hand-assembled NROM whose `STA $2007` sits at a known address; the assertion is
  pinned to that address, not to whatever the implementation records.
- `oam_dma_attributes_all_256_bytes_to_the_triggering_store` — all 256 bytes name
  the `STA $4014`.
- `write_attribution_is_invalidated_by_restore` — records are dropped across a
  save-state round-trip while the store stays armed.
- `provenance::tests` — cycle 0 is a real record and not a sentinel; indices wrap
  rather than panic; last write wins; `clear` forgets everything.
- `pixel_provenance_names_the_displayed_tile_not_the_fetch_pointer` — a
  hand-assembled CHR-RAM ROM that fills a nametable and a palette, then asserts
  the record for a background pixel names `$2002` (the tile on screen) and that
  the cascade advances exactly one tile per 8-pixel group across the scanline.
- `provenance_and_attribution_compose_into_a_causal_chain` — the end-to-end
  claim: pixel → palette entry → the PC that wrote it.
- AccuracyCoin **exactly 141/141**, nestest 0-diff, `visual_regression` green,
  with and without the feature.

### Traps these tests found, worth not rediscovering

- The PPU ignores `$2000/$2001/$2005/$2006` writes for ~29,658 CPU cycles after
  reset. Setup code that runs inside that window has its `$2006` address writes
  silently dropped, so every following `$2007` lands somewhere unintended. Test
  ROMs either loop their setup or delay past the window.
- The first `run_frame` after power-on returns on the already-latched
  frame-complete flag, having executed almost nothing.
- `$2006 = $20, $00` sets `v = $2000`, and bits 12-14 of a VRAM address **are**
  the fine-Y field — so that write leaves fine-Y at 2, not 0. A ROM that wants
  row 0 must follow it with `$2005`. The provenance record reports what the
  hardware is actually displaying, which is why this surfaced as a "wrong"
  pattern address that turned out to be right.
