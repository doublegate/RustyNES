# FDS real-BIOS boot — disk wire-format synthesis + register fixes (2026-06-11)

Branch: `feat/v2.6.0-netplay-compat`
Scope: `crates/nes-mappers/src/fds.rs` only (FDS RAM-adapter device). No iNES
change. Diagnostic harness: `crates/nes-test-harness/src/bin/fds_smoke.rs`
(+ `Cargo.toml` registration) — both uncommitted, NOT CI.

## TL;DR

The v2.2.0 FDS device streamed the **raw `.fds` block payloads** to the BIOS
(`deliver_byte`: `read_data = side[head]; head += 1`). The `.fds` container omits
the on-disk **gap → start-mark → CRC** framing, so the BIOS block-scan never
found a block start and every disk stuck forever on "NOW LOADING".

This change makes the FDS device synthesize the hardware **wire image** on read
and fixes **four additional latent register bugs** that the never-completed
real-BIOS boot had masked. With these fixes the BIOS now **reads the entire disk
correctly** — every block (`01 02 03 04 03 04 …`) in order, all files, the game
reset vector loaded, and the licence-screen tilemap reaches VRAM — proven by
register/VRAM tracing on the real `disksys-fcd.rom` BIOS booting commercial
Zelda/Metroid dumps.

**One residual blocker remains** (documented below): the BIOS's licence-screen
VRAM **self-verification** runs while file 0's IRQ-driven VRAM write is still in
progress, so the readback mismatches and the BIOS shows "DISK TROUBLE ERR.24"
(file-header-block-expected → the licence check fails). This is a
disk-transfer/IRQ-vs-main-thread **sequencing** residual, not a disk-read defect
— the bytes themselves are delivered correctly.

## The wire-format synthesis

A `.fds` side is a concatenation of block payloads. Each block is self-describing
via its leading block-code byte + a derivable length (`parse_side_blocks`,
fds.rs ~line 182):

- `$01` disk-info — **56 bytes** (`$00–$37`; the `$38` CRC is omitted from `.fds`).
- `$02` file-amount — **2 bytes** (byte 1 = file count).
- `$03` file-header — **16 bytes** (file size = LE-u16 at offset `$0D`).
- `$04` file-data — **`1 + size` bytes** (size from the preceding `$03` header).
- Trailing `$00` padding terminates the walk.

`build_side_wire` (fds.rs ~line 236) precomputes a per-side **wire image**
`Vec<u8>` = for each block:

```
[gap $00 × G] [$80 start mark] [block bytes] [crc_lo] [crc_hi]
```

with a long **lead-in gap** before block 1 (`WIRE_LEAD_IN_GAP = 200`) and
shorter **inter-block gaps** (`WIRE_BLOCK_GAP = 100`), then a trailing gap so the
head reads `$00` past the last block. `$80` is the gap-terminating `1` bit
(little-endian). The CRC is **CRC-16/KERMIT** (`fds_block_crc`, reflected poly
`0x8408`, over the `$80` mark + block bytes) — synthesized for faithfulness; the
BIOS does not strictly verify it (the RP2C33 would set `$4030.D4`, which this
device never asserts → CRC always "passes").

`WireBlock { raw_start, len, wire_payload_start }` maps each block's wire payload
region back to its raw `.fds` offset so the **write path** can persist
BIOS-written blocks (`wire_head_to_raw`, `store_byte`).

### Head + read/write paths

- `head` is now a **wire-image offset** (not a raw side offset).
- `deliver_byte` reads `wire[head]`. The RP2C33 controller **bit-shifts past the
  gap + `$80` mark in hardware** without raising a byte-transfer event; the first
  event delivers the byte *after* the mark. This is modelled by a
  **`read_skipping_gap`** state: when armed, `deliver_byte` silently advances the
  head over the `$00` gap run + the `$80` mark, then delivers the first block
  byte (fds.rs ~line 1170). It is re-armed on every read (re)start and on the
  transfer-reset rising edge, so the BIOS — which toggles reset between blocks —
  re-syncs to each block's start mark while the head keeps advancing across the
  inter-block gap.
- `store_byte` writes the BIOS write-stream (itself the wire format) into the
  wire image and mirrors block-payload bytes back to the raw side via
  `wire_head_to_raw`, so `.fds.sav` persistence keeps working. Writes landing on
  gap/mark/CRC positions modify only the synthesized framing (regenerated from
  the raw side), so they are not mirrored.
- On reaching the inner track (`head >= wire.len()`), `end_of_head` is flagged
  and `$00` delivered (the BIOS detects "no more data" via `$4030.D6`).
- `rebuild_wire` regenerates the wire image on insert/side-swap and on restore;
  the wire image is **derived state** (not stored in the save-state — it is
  reconstructed from the saved raw side contents).

## The four masked register bugs (all pre-existing)

Real-BIOS boot, never exercised before, surfaced four latent `$4025`/`$4030`
register-layout bugs:

1. **`$4025` bit0/bit1 swapped.** The code had `transfer_reset = value & 0x02`
   and `motor_on = (value & 0x01)==0`. Per nesdev / Takuika die-scan, **bit0 =
   transfer reset, bit1 = drive motor** (0: start, 1: stop). With the swap the
   whole `$2E/$2C/$ED` BIOS control sequence was nonsensical (the motor never
   actually started). Fixed in `write_control`.

2. **`$4030` byte-transfer flag at the wrong bit.** The code reported the
   byte-transfer flag at **bit1 (`0x02`)**; the real layout is **bit7 (`0x80`)**
   (bit1 is the DRAM-refresh watchdog). The BIOS polls `$4030.D7`, so it never
   saw a transfer. Fixed in `read_status_4030`; the end-of-head/CRC/timer bits
   were already correct.

3. **`$4030` read cleared the byte-transfer flag.** Per the Takuika reference,
   reading `$4030` acknowledges the **timer** IRQ but does **not** clear the
   byte-transfer flag (only a `$4024`/`$4031` service does). The old code cleared
   it on `$4030` reads, defeating the BIOS's `$4030.D7` poll. Fixed.

4. **Byte-transfer flag not gated on CRC-enable.** Per Takuika line 138/139 the
   flag updates only when **`$4025.D6` (CRC enable) + `$4025.D5`** are set (read
   mode additionally requires the motor on; write mode is motor-independent). It
   is **not** gated on transfer-reset — the BIOS holds reset asserted (`$ED`)
   while it arms CRC/IRQ and re-syncs between blocks, yet still expects byte
   transfers. `update_transfer_state` was rewritten to gate on
   `crc_enabled && crc_control` (+ motor for reads) and transfer-reset was
   demoted to "re-arm gap-skip + reset the byte-timer on its rising edge" (it no
   longer rewinds the head nor stops the transfer).

## Drive spin-up model

The BIOS reset disk-check waits for the drive's **not-ready → ready** spin-up
transition. The v2.2.0 window was only `INSERT_NOT_READY_CYCLES = 149` and closed
before the BIOS even polled. Added **`MOTOR_SPIN_UP_CYCLES = 50_000`** (~28 ms),
opened on the **first** motor-off→on edge after an insert (tracked by a new
`spun_up` flag). Crucially the window does **not** re-open on the motor restarts
the BIOS performs between blocks during a multi-block read (the physical disk
keeps spinning) — otherwise the mid-read `$4032.1` ready check at BIOS `$E745`
would spuriously trip the disk-error path. `spun_up` is reset on insert/eject and
persisted in the v3 disk-tail (`disk_flags` bit 2); v1/v2 blobs default it true.

## Save-state

`head` is now a wire offset (clamped after `rebuild_wire` on restore).
`read_skipping_gap` was added to the packed flags (bit 15) and `spun_up` to the
v3 `disk_flags` byte (bit 2) — both strictly additive; v1/v2 blobs restore with
safe defaults. Save version stays **3**.

## Verification (real BIOS, commercial dumps — NOT CI)

Driven via `fds_smoke` with `tests/roms/external/fds/{disksys-fcd,
disksys-fcd-rev1,disksys-twin}.rom` over Zelda/Metroid dumps. Register + VRAM +
CPU-PC tracing proved, **after the fix** (was: never any disk read at all):

- The BIOS spins the motor up, enables CRC+IRQ (`$ED`), and **reads the whole
  side**: 40 551 bytes for Zelda side A, block codes `01 02 03 04 03 04 …` in
  perfect order (verified `*NINTENDO-HVC*`, `ZEL`, file-amount = 7, all 7
  file-header/data pairs, byte-for-byte against the raw `.fds`).
- The game reset vector loads (`($DFFC)` = `$0405` for Zelda).
- The licence-screen tilemap (`NINTENDO ©`) reaches CIRAM `$2800`.

### Residual: licence-screen self-verification race

The BIOS licence check (`$F490`–`$F4A0`) reads VRAM `$2800` (224 bytes) and
`CMP`s it against its built-in `$ED37` "NINTENDO ©" reference. The check runs
while file 0's **IRQ-driven** `$2007` write to `$2800` is still in progress
(traced: at the verify moment only ~11 of 224 bytes were written, so positions
≥11 read `24` (space) instead of the `17 12 …` tiles), so the compare mismatches
→ `LoadFiles` returns error `$24` (file-header-block-expected) → "DISK TROUBLE
ERR.24" → the BIOS waits for a disk eject/insert. The disk bytes are delivered
**correctly** (the same data is in `$2800` by frame 250); the residual is the
verify-vs-IRQ-write **sequencing**, i.e. the BIOS reaching the readback loop
before the file-0 VRAM write the disk IRQ is performing has completed. This is a
disk-transfer/IRQ-timing precision item, not a wire-format/disk-read defect, and
is the only thing between the current state and a full title-screen boot.

Colour counts therefore remain ~7 (the licence/error plate) across all three BIOS
revisions and all games — but the underlying disk subsystem is now correct.

## Gates

- AccuracyCoin **100 %** ("no failing tests") — unaffected (FDS is a separate
  mapper-20 path).
- 60-ROM + 52-entry oracles byte-identical — iNES untouched.
- FDS unit tests (`-p nes-mappers --lib fds`, 56) + harness (`--test fds`, 6)
  pass. Adapted the unit tests that asserted the **old raw-delivery** semantics
  (head == raw offset; first read == block code with no gap; `$4025` minimal
  values without CRC; `$4030.D7` flag position; transfer-reset rewinding the
  head) to the wire-format model, via a `seek_head`/`settle_drive` helper pair
  and CRC-enabled `$4025` values — each adaptation is commented in-test.
- `cargo fmt`, `clippy -D warnings` (workspace, no commercial-roms), `cargo
  build --workspace`, `RUSTDOCFLAGS="-D warnings" cargo doc`, and
  `nes-mappers --no-default-features --target thumbv7em-none-eabihf` (no_std)
  all clean. One scoped `#[allow(clippy::too_many_lines)]` on the now-longer
  sequential `load_state` deserializer.
