# Save states and rewind

RustyNES v2 has two complementary "go back in time" features:

- **Save states** (`F1` / `F4`) write a full emulator snapshot to disk.
  Use these when you want a checkpoint that survives quitting the
  emulator.
- **Rewind** (hold `F5`) walks backwards through an in-memory ring of
  recent frames. Use this for second-to-second mistakes — fluffed a
  jump in a platformer, missed a turn in a racing game.

Both features capture the full emulated machine, so reloading puts you
exactly where you were — same CPU registers, same PPU mid-frame state,
same APU phase, same mapper bank registers.

## Save states

### Default hotkeys

| Key | Action |
|-----|--------|
| `F1` | Save state to the active slot |
| `F4` | Load state from the active slot |

Both are rebindable. See [Controls](./controls.md).

### Slots

There are **10 slots per ROM** on disk, numbered 0 through 9. The `F1` /
`F4` hotkeys target the **active slot**, which defaults to slot 1.
Saving overwrites without confirmation.

Pick the active slot, or save/load a specific slot directly, from the
menu bar:

- **File → Save Slot ▸** — choose the active slot (1–8) used by `F1` / `F4`.
- **File → Save to Slot ▸ / Load from Slot ▸** — write or read a chosen
  slot (1–8) without changing the active slot.

(The menu surfaces slots 1–8; the slots are numbered from 1 in the UI and
from 0 on disk, so menu "Slot 1" is `slot0.rns`.)

The on-disk layout is one file per slot, per ROM:

```
<data_dir>/saves/<rom_sha256_hex>/slot0.rns
<data_dir>/saves/<rom_sha256_hex>/slot1.rns
... up to slot9.rns
```

`<data_dir>` is your OS's standard data directory (see
[File locations](./file-locations.md)). `<rom_sha256_hex>` is the 64-character
lowercase hex SHA-256 of the ROM file's contents, so the same game with
the same dump always lands in the same directory — moving the `.nes`
file around doesn't break the saves.

### File format

Each `.rns` file starts with an 8-byte `RUSTYNES` magic, a 2-byte
little-endian format version, a 6-byte truncated ROM SHA-256 sanity tag,
then a sequence of tagged sections: `BUS `, `CPU `, `PPU `, `APU `,
`MAP `. Unknown sections are skipped on load for forward compatibility,
so a state written by a newer version that adds (for example) a
`NETPLAY ` section will still load on this version with the new section
ignored.

The format is intentionally simple — no `serde`, no `bincode`, no
`bitflags-serde`. Cross-version compatibility within v1.x is
best-effort, not guaranteed: a chip section's version byte is bumped any
time its on-disk layout changes, and loading an incompatible version
returns an error rather than silently corrupting state.

### Determinism guarantee

Given the same ROM and the same input sequence, a snapshot loaded into
two RustyNES processes produces byte-identical framebuffer and audio
output going forward. This is the same property that makes future TAS
recording and netplay possible.

### When loading fails

If the slot file is missing, an `eprintln!` warning goes to stderr —
the running emulator state isn't affected. Common causes:

- You renamed or re-dumped the ROM, changing its SHA-256.
- You moved the data directory.
- The slot file is from an older RustyNES with an incompatible chip
  section version.

The current state continues running; nothing crashes.

## Rewind

### Hotkey

| Key | Action |
|-----|--------|
| `F5` (held) | Walk backwards through the rewind ring, one frame per held tick |

Releasing `F5` resumes forward play from the point in time you rewound
to. The rewind ring continues capturing from there — you can rewind,
release, play a few seconds, then rewind again.

### How it works

Every emulated frame, the core captures a compact snapshot of the
machine state into a ring buffer:

- Once per second (every 60 frames at NTSC) the snapshot is stored as
  an LZ4-compressed full keyframe.
- The frames in between are stored as LZ4-compressed XOR deltas against
  the most recent keyframe — because NES screen content changes slowly,
  most delta bytes are 0 and they compress aggressively.

When the ring exceeds its byte budget (default 32 MiB), oldest entries
are evicted first. Orphaned deltas whose keyframe was evicted are
themselves dropped.

### Configuration

Three keys in `[rewind]` in `config.toml`:

```toml
[rewind]
enabled = true        # default — set to false to disable rewind entirely
max_seconds = 60      # default — rewind window length in seconds
keyframe_period = 60  # default — one keyframe per second (at NTSC)
```

Lowering `keyframe_period` makes step-back faster (less delta work) at
the cost of memory. Raising it does the opposite. The 32 MiB memory cap
takes precedence — even if `max_seconds` would otherwise need more,
oldest frames evict.

### What gets captured

The rewind snapshot is a strict subset of the save-state snapshot, but
covers all emulator-observable state:

- CPU registers + pending interrupt latches
- PPU palette RAM, OAM, internal scroll registers, current scanline/dot,
  open-bus state, the framebuffer
- APU per-channel state including envelope counters, sweep units, DMC
  DMA bookkeeping
- Cartridge RAM (PRG-RAM, ExRAM, CHR-RAM where applicable)
- Mapper bank registers and IRQ-counter state
- Both controller shift registers and strobe state
- Cycle counter

What is **not** captured: the host audio queue (its drop-oldest policy
absorbs the brief discontinuity), the wgpu surface (next redraw
recreates the texture from the framebuffer), or any debugger overlay
state.

### Memory footprint

A typical NES title's rewind ring sits at 1-9 MiB in steady state with
the defaults: 60 keyframes (one per second) of ~14 KiB each plus ~3540
deltas of ~1-3 KiB each. Games with heavily-changing screen content
(scrolling shooters, full-screen palette flashes) use more; games with
relatively static screens (RPG dialogue, menus) use less. The 32 MiB
upper cap is a hard ceiling.

## See also

- [Configuration](./configuration.md) — the `[rewind]` section reference
- [File locations](./file-locations.md) — where `<data_dir>/saves/` lives on your OS
- [Troubleshooting](./troubleshooting.md) — what to do when a load fails
