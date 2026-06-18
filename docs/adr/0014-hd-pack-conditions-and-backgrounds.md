# 14. HD-pack `<condition>` gating and `<background>` regions

Date: 2026-06-16

## Status

Accepted (v1.3.0 Workstream E1).

## Context

The v1.2.0 HD-pack loader (ADR 0013's sibling; behind the default-off `hd-pack`
feature) handled only **unconditional** Mesen `hires.txt` tile replacement:
`<scale>` / `<patternTable>` / CHR-hash `<tile>` rules. Two Mesen capabilities
were explicitly deferred:

1. **`<condition>` gating** — a `<tile>`/`<background>` rule that only applies when
   a runtime predicate holds (a memory-address compare, a frame-number range, a
   sprite mirror/palette match).
2. **`<background>` region replacement** — substituting a full image region rather
   than per-CHR-tile.

The hard problem for conditions is *where the predicate reads its memory*. RustyNES'
render path copies the framebuffer under a **brief** `Arc<Mutex<EmuCore>>` lock and
then composites/presents with the lock **not** held (`nes = None` in the present
branch — see `docs/frontend.md`). So the HD compositor cannot read live `Nes`
memory while it runs. Reading emulator memory off-lock would also be a determinism
and data-race hazard.

## Decision

Mirror Mesen2's `HdScreenInfo::WatchedAddressValues` model: **snapshot only the
finite set of watched addresses once per frame, at produce time, under the lock**,
then evaluate conditions against that snapshot during the lock-free composite.

- The parser (`crates/rustynes-frontend/src/hdpack.rs`) gains `ConditionKind`
  (`MemoryCheck`, `MemoryCheckConstant`, `FrameRange`, `HMirror`, `VMirror`,
  `SpritePalette`), a name→index resolver for `<tile>`/`<background>` rules that
  reference a condition (AND-joined when multiple), and `<background>` parsing into
  `BackgroundRegion`. Memory addresses use Mesen's `PPU_MEMORY_MARKER = 0x8000_0000`
  (bit 31) to select PPU- vs CPU-space.
- `HdPack::watched_addresses()` returns the union of addresses referenced by all
  parsed memory conditions. The frontend's produce path (`app.rs`, under the emu
  lock) fills a `WatchedMemory` map via **read-only, side-effect-free** peeks
  (`Nes::cpu_bus_peek` / `peek_ppu`) for exactly those addresses, into
  `present_watched_mem`, which is handed to the compositor.
- `HdCompositor::eval_condition` evaluates `(watched[addr] & mask) <op> value` for
  memory checks, the current frame counter for `FrameRange`, and the per-pixel
  `HdTileSource` (mirror/palette) for the sprite predicates. Tile substitution and
  `<background>` blits are gated on their condition(s); an unresolved condition
  fails **closed** (rule does not apply).

## Consequences

- **Determinism-safe.** The snapshot is a read-only view of already-deterministic
  state taken under the lock; nothing in the core changes. With `hd-pack` off the
  shipped / wasm / `no_std` builds are byte-identical (the E1 code + the produce
  snapshot are `#[cfg(feature = "hd-pack")]`-gated). AccuracyCoin / the oracle are
  untouched.
- **Lock-correct.** No emulator memory is read during the lock-free composite; the
  watched-address set is finite (only what the pack references), so the per-frame
  snapshot cost is negligible and proportional to the pack, not to RAM size.
- **Not full Mesen parity.** Implemented: memoryCheck(Constant), frameRange,
  hmirror/vmirror, sprite-palette, and full-image/region backgrounds. Still
  deferred (no demand / heavier model): `TileNearby` / `TileAtPos` / `SpriteNearby`
  / `SpriteAtPos` neighbor predicates, position checks, `<addition>` /
  `<fallback>` / `<options>`, and the full blend/priority/parallax compositor.
  These can be added later without changing the snapshot architecture.
- **HD audio landed in v1.6.0 "Studio" Workstream H** (the biggest remaining
  Mesen2 gap). The `hires.txt` `<bgm>` / `<sfx>` declarations are parsed in
  `hdpack.rs`; `src/hd_audio.rs` decodes their OGG tracks (pure-Rust `lewton`,
  pulled only by `hd-pack`) and an `HdAudioMixer` sums the selected track into
  the drained APU buffer **in place** in the FRONTEND audio path, gated on the
  `$4100` HD-pack audio-control register. The same determinism stance as the
  visual path holds: the mixer reads only a side-effect-free `$4100` peek of the
  already-produced bus state and never touches core synthesis or the
  deterministic per-frame audio buffer, so AccuracyCoin and the audio oracle are
  unchanged and the audio is byte-identical with no audio pack loaded / the
  feature off. The `$4100` selection is best-effort (RustyNES does not intercept
  the register write, so the frontend reads it back and edge-detects); folder
  packs are supported, `.zip`-pack audio + the full `$4100`..`$4106` Mesen state
  machine are future extensions. Audible playback is a maintainer manual-check
  (no audio device in CI); the parse / trigger-edge / mixer buffering are
  unit-tested.
- HD-pack remains an **output-only**, default-off, native-oriented feature; the
  per-pixel HD-pack debug inspector (to trace which condition gated a substitution)
  is tracked as a future devtools item (v1.4.0 plan, Workstream D).
