# Pixel provenance

> **Status:** Phase 1 (write attribution) implemented. Phases 2-4 are specified
> here and land in v2.3.3 "Lucid"; this document is the spec, so it is updated in
> the same change as the code it describes.

## What the feature answers

Point at any pixel on screen and get the full causal chain that produced it:

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

The store is cleared on **power-cycle** and on **both save-state restore paths**
(`Nes::restore` and `Nes::restore_quiet`). A restored state's bytes were not
written by any instruction this session executed, so the PCs recorded against
those offsets describe a timeline that no longer exists. Under run-ahead the
per-frame `restore_quiet` therefore leaves exactly the visible frame's writes —
which is the timeline the user is looking at.

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

## Phase 2 — per-pixel provenance record (planned)

`HdTileSource` (`crates/rustynes-ppu/src/ppu.rs`) already carries, per emitted
pixel, the CHR address, palette, sprite-vs-background flag, flips, the four
candidate sprites, and the CHR tile index. It is written in `emit_pixel` under
`hd-pack`. Phase 2 widens that gate so the record is also populated under
`debug-hooks`, extended with the emitting dot/scanline and the nametable and
attribute source addresses.

This is a gate widening, not a change to what HD packs see: the `hd-pack` path
must stay byte-identical, and the framebuffer must stay bit-identical with
`debug-hooks` on.

## Phase 3 — the panel (planned)

`crates/rustynes-frontend/src/debugger/provenance_panel.rs`, assembled from parts
that already exist rather than rebuilt:

| reused for | from |
|---|---|
| per-pixel picking and hover readout | `debugger/hd_pixel_panel.rs` |
| scanline x dot timeline rendering | `debugger/event_panel.rs` |
| PC to source-line resolution | `debugger/source_map.rs` |
| call context for the writing PC | `debugger/callstack.rs` |
| expression and formatting helpers | `debugger/expr.rs` |

Registered through the shared `detachable_window` helper, so it inherits v2.3.0's
multi-viewport pop-out.

## Phase 4 — deterministic replay attestation (planned)

Separate feature, same release, and it reuses the determinism contract rather
than the provenance store. `.rnm` movies (`crates/rustynes-core/src/movie.rs`)
already carry `rom_sha256`, region, start state, the input stream, and a re-record
count at `MOVIE_FORMAT_VERSION = 2`. Phase 4 adds an **additive v3 tail** holding
a rolling hash of per-frame core state, plus a `rustynes --verify <run>` path that
replays and compares. A v2 `.rnm` must still load unchanged.

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
- AccuracyCoin **exactly 141/141**, nestest 0-diff, `visual_regression` green,
  with and without the feature.

Both ROM-driven tests loop their write sequence and run several frames, because
the PPU ignores `$2000/$2001/$2005/$2006` writes for ~29,658 CPU cycles after
reset, and because the first `run_frame` after power-on returns on the
already-latched frame-complete flag having executed almost nothing.
