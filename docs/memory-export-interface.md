<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# The memory-export interface and speedrun timing

v3.5.0 (`T-SPEEDRUN-MEMIF`, ADR 0047). This page is the normative description
of two things a speedrunning tool needs from RustyNES:

1. **Auto-timing**: start, split and reset conditions written as RAM predicates
   in the RetroAchievements condition grammar, with the time of a run counted in
   the console's master-clock cycles.
2. **The memory-export interface** (the "memif"): a named shared-memory block,
   at fixed and versioned offsets, carrying the console's RAM, its frame and
   master-clock counters and the timer's state after every frame, so a
   LiveSplit-style tool reads it without attaching to the process and without
   per-version base addresses.

The block's offsets are a contract. A field never moves within layout version
1; a reader written against this page works on every RustyNES build that
publishes version 1. `scripts/memif/reference_splitter.py` is such a reader,
written from this page alone, and the gate below runs it against two different
builds without changing it.

## Timing

### What is counted

A run's time is the number of master-clock cycles between the end of the frame
in which the `start` condition first held and the end of the frame in which the
last split's condition first held. The master clock is the counter the CPU
advances by the region's divider on every CPU cycle (12 NTSC, 16 PAL, 15
Dendy), so a difference of two readings is exact, whatever the frames' lengths
(the NTSC odd-frame skip, lag frames, the extra-scanline option). Seconds are
`cycles / f`, with `f` the master-clock frequency:

| Region | Master clock | As a fraction (Hz) |
| --- | --- | --- |
| NTSC | 21.477272... MHz | 236,250,000 / 11 |
| PAL | 26.6017125 MHz | 53,203,425 / 2 |
| Dendy | 26.6017125 MHz | 53,203,425 / 2 |

The conditions are evaluated once per frame, after the frame, which is the
cadence RetroAchievements evaluates achievements at. A split's time therefore
has one-frame resolution, like every RAM-driven splitter; what is exact is the
length of the frames between the start and the split.

The time is emulated time. It does not depend on the host's speed, a pause,
fast-forward or a stutter, which is what lets a run timed from a movie give
the same number on every machine and every replay.

**Discontinuities.** A state load, a rewind or a power cycle moves the master
clock backward, or forward by more than sixteen PAL frames between two frames.
The timer then abandons the run (it returns to idle) rather than report a
time that no longer measures one run.

### The splits file

```text
# RustyNES splits definition (comments and blank lines are ignored)
game = Super Mario Bros.
category = Any%
start = 0xH0770=1_d0xH0770=0
split = World 1 | 0xH075F=1_d0xH075F=0
split = Bowser | 0xH0770=2
reset = 0xH0770=0
```

| Key | Meaning |
| --- | --- |
| `game`, `category` | Labels, copied into the export. Optional. |
| `start` | The run starts on the first frame this holds while the timer is idle. Required. |
| `split = <name> \| <condition>` | The splits, in order; each is armed only after the one before it fires, and the last ends the run. At least one, at most 64. |
| `reset` | While a run is going or finished, the timer returns to idle on the first frame this holds. Optional. |

Keys and values are split at the first `=`, so a condition keeps its own `=`
signs; a split's name and condition are split at the first `|`.

### The condition grammar

The subset of the RetroAchievements condition syntax the timer implements,
written from RetroAchievements' public documentation of the syntax:

- A **condition set** is groups separated by `S`. It holds when the first group
  holds and, if there are more, at least one of the others does.
- A **group** is conditions separated by `_`, all of which must hold.
- A **condition** is `operand comparison operand`, optionally followed by a hit
  target `.N.` (it must have held on `N` evaluations while its trigger was
  armed; once reached it stays reached until the trigger is re-armed).

| Operand | Meaning |
| --- | --- |
| `0xHaaaa` | the byte at `aaaa` |
| `0xaaaa`, `0x aaaa` | 16-bit little-endian |
| `0xWaaaa`, `0xXaaaa` | 24-bit and 32-bit little-endian |
| `0xIaaaa`, `0xJaaaa`, `0xGaaaa` | 16-, 24- and 32-bit big-endian |
| `0xMaaaa` ... `0xTaaaa` | bit 0 ... bit 7 of the byte |
| `0xLaaaa`, `0xUaaaa` | the low and high nibble |
| `0xKaaaa` | the number of set bits in the byte |
| `d` + memory | delta: the value at the previous evaluation (on the very first, the current value, so a `d` comparison never reports a change that did not happen) |
| `p` + memory | prior: the value before the last change |
| `b` + memory | BCD: the hex digits read as decimal |
| `~` + memory | the bitwise inverse within the operand's size |
| `12`, `h1F` | a decimal or hexadecimal constant |

Comparisons: `=`, `!=`, `<`, `<=`, `>`, `>=`.

**Addresses** are RetroAchievements NES addresses, which are CPU-bus
addresses (the rcheevos region table, and the mapping `rustynes-cheevos`
uses): system RAM at `0x0000-0x07FF` with its mirrors, cartridge RAM at
`0x6000-0x7FFF`, ROM at `0x8000-0xFFFF`, all read without side effects.
Anything above `0xFFFF` reads 0. A condition copied from an achievement set
therefore means the same thing here. (Before v3.5.0's review this page, the
timer and the desktop achievements client put cartridge RAM at
`0x0800-0x27FF`, which is not RA's layout.)

**Refused** (an error naming the text, never a silent misreading): condition
flags (`R:`, `P:`, `A:`, `B:`, `C:`, `N:`, `M:`, `Q:`, `I:`, `T:`, `Z:`, `O:`,
`D:`, `K:`), arithmetic operators, float and signed constants, and the float
sizes. The splits file's own `reset` covers what `R:` is mostly used for.

## Running it

| Use | Command |
| --- | --- |
| Time a movie headlessly | `rustynes speedrun --rom game.nes --splits any.rnsplits --movie run.rnm` |
| ... and write the result | `... --json run-splits.json` |
| Run a ROM for N frames with no input | `rustynes speedrun --rom game.nes --splits any.rnsplits --frames 3600` |
| Publish the memory-export block while it runs | `... --memif` (a build with the `memif` feature) |
| Time and publish a live session in the window | `rustynes game.nes --splits any.rnsplits --memif` |

`rustynes speedrun` prints one line per event (`start`, `split`, `finish`,
`reset`, `discontinuity`) with its frame and master clock, then the result. Its
exit code is 0 when the run finished, 5 when it did not (the start or a split
never fired), and 1 on an error. 2 stays clap's usage error, 3 `verify`'s "no
attestation" and 4 `dump-check`'s "a finding", so a script can tell every
outcome apart.

### The result

`--json` writes (and stdout ends with) a document of format
`rustynes-splits-result`, version 1:

```json
{
  "format": "rustynes-splits-result",
  "version": 1,
  "game": "Super Mario Bros.",
  "category": "Any%",
  "region": "NTSC",
  "master_clock_hz": {"numerator": 236250000, "denominator": 11},
  "state": "finished",
  "start_master_clock": 4325712,
  "start_frame": 12,
  "splits": [
    {"name": "World 1", "master_clock": 9686392, "frame": 27, "elapsed_cycles": 5360680, "segment_cycles": 5360680, "elapsed_seconds": 0.249597799}
  ],
  "elapsed_cycles": 5360680,
  "elapsed_seconds": 0.249597799
}
```

Cycle counts are exact integers; the seconds are derived from them and the
fraction above, and are for display.

## The memory-export block

### Opening it

| Platform | Object | Opened with |
| --- | --- | --- |
| Linux | POSIX shared memory, the file `/dev/shm/rustynes-memif.<uid>` | `shm_open("/rustynes-memif.<uid>", O_RDONLY)` + `mmap` |
| macOS | POSIX shared memory | `shm_open("/rustynes-memif.<uid>", O_RDONLY)` + `mmap` |
| Windows | a named file mapping in the session's `Local\` namespace | `OpenFileMappingW(FILE_MAP_READ, FALSE, L"Local\\rustynes-memif")` + `MapViewOfFile` |

`<uid>` is the user's effective uid in lowercase hexadecimal (`3e8` for uid
1000). In Python: `SharedMemory(f"rustynes-memif.{os.geteuid():x}")` on POSIX
and `SharedMemory("Local\\rustynes-memif")` on Windows (with `track=False` on
Python 3.13 and later, so the reader does not remove the block when it exits).

The block exists while a RustyNES process publishing it runs. It is per user
and owner-only, and a reader should check that before trusting it:

- **Threat model.** Shared-memory names are a namespace other local users can
  create objects in. RustyNES puts the user in the name (the `.<uid>` suffix,
  or Windows' per-session `Local\` namespace), creates the block owner-only
  (mode `0600` with `O_EXCL`; on Windows a protected DACL granting only the
  owner), and refuses to adopt an existing object that is not owned by the
  user with no group or other access. A reader should apply the same check on
  POSIX (`fstat`: `st_uid == geteuid()` and `st_mode & 0o077 == 0`), as the
  reference reader does, so it cannot be fed a block another user planted.
- **Another name.** `--memif-name <name>` (1-21 characters of
  `A-Z a-z 0-9 _ . -`) publishes under another name, for example to run two
  instances.

### Layout version 1

All multi-byte fields are little-endian.

| Offset | Size | Field |
| --- | --- | --- |
| `0x0000` | 8 | magic `RNESMEMF` |
| `0x0008` | 4 | layout version, `1` |
| `0x000C` | 4 | offset of the CPU RAM block, `0x0100` |
| `0x0010` | 4 | layout size, `0x2B00` |
| `0x0014` | 4 | sequence counter (odd while a publish is in progress) |
| `0x0018` | 8 | publish count (rises by one per publish) |
| `0x0020` | 8 | frame counter (PPU frames since power-on) |
| `0x0028` | 8 | CPU master clock |
| `0x0030` | 4 | master-clock frequency numerator, in Hz |
| `0x0034` | 4 | master-clock frequency denominator |
| `0x0038` | 1 | region: 0 NTSC, 1 PAL, 2 Dendy |
| `0x0039` | 1 | flags: bit 0 a game is loaded, bit 1 a speedrun timer is defined |
| `0x003A` | 1 | timer state: 0 idle, 1 running, 2 finished |
| `0x003B` | 1 | reserved |
| `0x003C` | 4 | splits completed |
| `0x0040` | 4 | splits defined |
| `0x0044` | 4 | the core's emulation epoch (`EMULATION_EPOCH`) |
| `0x0048` | 8 | timer start, as a master-clock value |
| `0x0050` | 8 | timer elapsed, in master-clock cycles (live while running, final once finished, 0 when idle) |
| `0x0058` | 8 | reserved |
| `0x0060` | 32 | SHA-256 of the ROM without its 16-byte header (the hash movies and saves carry) |
| `0x0080` | 128 | reserved, zero |
| `0x0100` | 2048 | CPU RAM `$0000-$07FF` |
| `0x0900` | 8192 | cartridge RAM window `$6000-$7FFF`, as the CPU would read it (side-effect-free) |
| `0x2900` | 512 | split table: 64 entries of 8 bytes, the elapsed cycles at each completed split (0 for one not reached) |

The two RAM blocks hold the two RAM windows of the CPU bus, which are also the
RetroAchievements addresses a condition uses: RAM address `x` (`$0000-$1FFF`,
with its mirrors) is at `0x0100 + (x & 0x07FF)`, and cartridge RAM address `x`
(`$6000-$7FFF`) is at `0x0900 + (x - 0x6000)`. (Until v3.5.0's review this
page said the block was the RA "flat space" at `0x0100 + x`, which held only
under the earlier, wrong reading of RA's NES addresses; the layout itself has
not changed.)

When no game is loaded the header (magic to layout size, the publish count and
the epoch) is written and every other byte is zero; check flag bit 0 before
reading RAM.

### Reading it consistently

Every publish is one update under the sequence counter at `0x0014`:

1. Load the counter. If it is odd, a publish is in progress: try again.
2. Copy what you need.
3. Load the counter again. If it changed, the copy may mix two frames: go back
   to 1.

The writer makes the counter odd before its first byte and even after its last,
so a copy taken between two equal even readings comes from one frame. Read the
counter with an atomic (or at least volatile) 32-bit load.

### Compatibility promise

- Within layout version 1, no field moves, changes size or changes meaning.
- Reserved bytes may gain a meaning in a later release; they are zero until
  then, and a reader must not require them to stay zero.
- A change that cannot keep these rules gets a new layout version, and the
  release notes say so.
- The block is output only. RustyNES rewrites it whole every frame and never
  reads it back, so a tool writing into it changes nothing in the emulator, and
  attaching one cannot change a frame, a sample or a bus cycle.

## The gate

`T-SPEEDRUN-MEMIF`'s plan gate has two halves.

**"A run timed from a movie matches its cycle count on every replay."**
`crates/rustynes-test-harness/tests/speedrun_timer.rs` records a movie on a
synthetic ROM (written in the test; it stores the controller into RAM every
frame and counts frames), times it while recording, then replays the movie
from its serialized bytes three times. Each replay must give the same start,
the same split clocks and the same total, and the total must equal an
independent count: the CPU cycles between the start and end frames times the
region's divider.

**"A reference splitter reads the documented interface on two builds without
changes."** `scripts/memif/two_build_check.sh` builds `rustynes` twice (a
debug build with `--features memif` and a release build with `--features
full`), runs `rustynes speedrun --memif` on each, and has
`scripts/memif/reference_splitter.py` (written from this page alone; standard
library only) follow the block until the run finishes. Both builds must give
the reader the same splits, equal to the ones each build reports itself.

What the gate does not cover: a tool other than the reference reader (LiveSplit
itself was not run), Windows and macOS (the shared-memory code is compile- and
lint-checked for both, but the reader has only been run on Linux), and the
desktop window's live `--splits` path, which shares the timer and the block with
the headless command but has no automated test of its own.
