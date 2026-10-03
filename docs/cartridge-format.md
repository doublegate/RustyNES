# Cartridge file format (iNES + NES 2.0)

**References:** `ref-docs/research-report.md` §Cartridge format and mappers
→ iNES and NES 2.0; `ref-docs/nesdev-wiki-technical-report.md` §ROM And
Music File Formats; Nesdev [iNES](https://www.nesdev.org/wiki/INES) and
[NES 2.0](https://www.nesdev.org/wiki/NES_2.0).

## Purpose

Parse iNES 1.0 and NES 2.0 ROM files into a `Cartridge` value that the mapper subsystem can use.

## File structure

```text
[ 16 bytes header ]
[ 512 bytes trainer (only if header[6] bit 2 set) ]
[ PRG-ROM ]
[ CHR-ROM ]
[ misc ROM, NES 2.0 only ]
```

## Header layout

| Off | Field |
|-----|-------|
| 0-3 | Magic: `"NES\x1A"` (`0x4E 0x45 0x53 0x1A`) |
| 4 | PRG-ROM size LSB |
| 5 | CHR-ROM size LSB |
| 6 | Flags 6: nametable arrangement (bit 0), battery PRG-RAM (bit 1), trainer present (bit 2), four-screen (bit 3), mapper D3-D0 (bits 4-7) |
| 7 | Flags 7: console type LSBs (bits 0-1), NES 2.0 ID (bits 2-3 == `10` for NES 2.0), mapper D7-D4 (bits 4-7) |
| 8 | Mapper D11-D8 (bits 0-3), submapper (bits 4-7) — NES 2.0 only |
| 9 | PRG-ROM size MSB nibble (bits 0-3), CHR-ROM size MSB nibble (bits 4-7) — NES 2.0 only |
| 10 | PRG-RAM shift (bits 0-3), PRG-NVRAM shift (bits 4-7) — NES 2.0 only |
| 11 | CHR-RAM shift (bits 0-3), CHR-NVRAM shift (bits 4-7) — NES 2.0 only |
| 12 | CPU/PPU timing (bits 0-1: 0=NTSC, 1=PAL, 2=multi, 3=Dendy) — NES 2.0 only |
| 13 | Console type 1 (Vs. System): Vs. PPU type (bits 0-3), Vs. hardware type (bits 4-7). Console type 3 (Extended): extended console type (bits 0-3), bits 4-7 reserved. Otherwise unused — NES 2.0 only |
| 14 | Misc ROM count (bits 0-1) — NES 2.0 only |
| 15 | Default expansion device (bits 0-6) — NES 2.0 only |

### Detection rule

```rust
is_nes2 = (header[7] & 0x0C) == 0x08;
```

If false, parse as iNES 1.0, but treat upper mapper bits cautiously. Older
tools often wrote non-zero padding or signature strings into bytes 7-15,
corrupting mapper high bits ("DiskDude!" puts `'D'` = `$44` in byte 7 and adds
64 to the mapper number). RustyNES applies the NESdev "iNES" rule: **if the
header is not NES 2.0 and bytes 12-15 are not all zero, the upper four mapper
bits (byte 7's high nibble) are masked off** (since v2.9.8). A clean iNES 1.0
header has zeros there, so a well-formed dump of mapper 16-255 is unaffected,
and NES 2.0 headers are exempt because bytes 12-15 are real fields. The
per-game database still runs first in the frontend and the coverage harness,
but it rewrites only bytes 6-7, not the dirty tail, so a database mapper of 16
or more on such a dump would be masked as well. Every corrected "DiskDude!"
dump in the local corpus has a database mapper below 16, so none is affected.

### Mapper number

```rust
dirty = !is_nes2 && header[12..16] != [0; 4];
mapper = (header[6] >> 4)
    | (if dirty { 0 } else { header[7] & 0xF0 })
    | (if is_nes2 { (header[8] & 0x0F) << 8 } else { 0 });
```

iNES 1.0 mapper numbers cover 0..=255. NES 2.0 extends to 0..=4095.

### ROM sizing

**Standard notation** (MSB nibble != `$F`): `size = ((MSB << 8) | LSB) * unit_size`. PRG unit = 16 KiB; CHR unit = 8 KiB.

**Exponent-multiplier** (MSB nibble == `$F`): the LSB byte encodes `EEEEEEMM` where E is exponent (6 bits) and MM is multiplier code; size = `2^E * (MM*2 + 1)` bytes. MM values map to multipliers 1, 3, 5, 7. Used for non-power-of-2 ROM sizes.

### RAM sizing (NES 2.0)

```rust
prg_ram_size = if shift == 0 { 0 } else { 64 << shift };
```

Same encoding for CHR-RAM and the NVRAM variants. `Header` carries all four:
`prg_ram_size` and `chr_ram_size` (the volatile low nibbles), `prg_nvram_size`
and `chr_nvram_size` (the non-volatile high nibbles). The PRG-RAM window a board
allocates at `$6000-$7FFF` is `Header::prg_ram_window()`, the volatile and
non-volatile sizes together, because some carts (StarTropics / MMC6) declare
their save RAM only in the NVRAM nibble. Until v2.9.8 `prg_ram_size` held that
sum and the NVRAM sizes had no field; the window, and so every board's
allocation, is unchanged. No board allocates CHR-NVRAM from the header.

iNES 1.0 has no RAM size fields: `prg_ram_size` reports a nominal 8 KiB,
`chr_ram_size` 8 KiB when there is no CHR-ROM (else 0), and both NVRAM sizes 0.

### Mirroring

iNES: bit 0 of header[6] = vertical (1) or horizontal (0); bit 3 = four-screen override. NES 2.0 same encoding (mirroring is mostly a property of mapper-controlled state for any non-trivial mapper — this header bit is the *initial* mirroring).

### Console type

NES 2.0 byte 7 bits 0-1: `00` = NES/Famicom, `01` = Vs. System, `10` = Playchoice 10, `11` = extended (see byte 13).

Byte 13 is decoded per console type (NESdev "NES 2.0" §"Vs. System Type" and
§"Extended Console Type"):

- **Vs. System** (console type 1): `Header::vs_ppu_type` from the low nibble
  (`VsPpuType`; the reserved nibbles `$1`, `$6`, `$7`, `$C-$F` decode as the
  2C03), and `Header::vs_hardware_type` from the high nibble
  (`VsHardwareType`: `UniSystem` and its four protection variants, the two
  `DualSystem` types, and `Reserved(7..=15)`). `Header::is_vs_dual_system()` is
  true for types 5 and 6; until v2.9.8 that was a `vs_dual_system` field and the
  type itself was not kept, so a canonical encode wrote every dual board as 5.
- **Extended** (console type 3): `Header::extended_console_type` from the low
  nibble (`ExtendedConsoleType`: `$0-$C` named, `Reserved(13..=15)`).
- Otherwise byte 13 is unused and both fields are `None`.

The two `Option` fields are `Some` exactly when the header is NES 2.0 and the
console type is the matching one, so an iNES 1.0 dump's junk in byte 13 never
becomes a hardware type.

### Miscellaneous ROMs (NES 2.0)

Byte 14 bits 0-1 give the number of miscellaneous ROMs present
(`Header::misc_rom_count`, `0..=3`; 0 on iNES 1.0). The area itself is whatever
follows CHR-ROM in the file.

### Region (NES 2.0)

Byte 12 bits 0-1: `00` = NTSC, `01` = PAL, `10` = multi-region, `11` = Dendy. The core plays multi-region at NTSC timing. iNES 1.0 has only the legacy bit in byte 9 (largely useless; many ROMs are mis-tagged), and `parse_header` ignores it: an iNES 1.0 image always parses as NTSC. Old dump tools wrote "DiskDude!" over bytes 7-15 (byte 9 = `'s'` = `$73`, bit 0 set), and on the staged corpus three pirate multicart images carry `$01` there in an otherwise clean header with nothing to say whether it is meant.

**Where a PAL iNES 1.0 game gets its region (v2.9.8).** From the per-game
database, on the load path, before the core parses the header
(`rustynes_gamedb::load_time_entry` then `apply_header_overrides`, the one
chokepoint the desktop, browser and coverage harness share). A PAL or Dendy row
rewrites the iNES 1.0 header as the NES 2.0 header that describes the **same
board**, with the region in byte 12: the mapper number the iNES 1.0 parse
settled on, submapper 0, the plain byte-4/5 sizes (byte 9 = 0), PRG-RAM and
CHR-RAM stated as the iNES 1.0 heuristics give them (8 KiB, and 8 KiB of CHR-RAM
when there is no CHR-ROM), and bytes 13-15 zero. Because NES 2.0 sizes are taken
at their word where iNES 1.0 sizes are not -- the v2.9.6 MMC3 multicart boards
take their own default instead of iNES 1.0's nominal 8 KiB -- a second candidate
states no RAM at all. Each candidate is parsed and compared with the iNES 1.0
board (cartridge identity, mirroring, console type, battery, and the mapper's
serialized state, debug view, capability flags and RAM sizes); the first that
matches is used, and if none does the region is refused rather than bought with
a different board. A unit sweep over every mapper id finds no board refused.
Vs. System and PlayChoice-10 carts are never promoted (those cabinets exist only
in NTSC timing). A NES 2.0 header keeps its own region: a vendored database row
never rewrites byte 12. `parse_header` itself is unchanged -- the byte-9 rule
above still holds for an image no database row describes.

### Default input device (NES 2.0)

Byte 15 bits 0-6 identify the default expansion or input device. Since v2.9.8
it is parsed into `Header::default_expansion_device` (`ExpansionDevice`: the
NESdev codes `$00-$4F` by name, `Unassigned(n)` for `$06` and `$50-$7F`;
`Unspecified` on iNES 1.0), but nothing selects an input device from it: the
frontend still assumes standard controllers unless mapper or test harness metadata
overrides it. Full use of this field belongs with the v1.x expanded-input
work, especially for Zapper, Four Score, Famicom expansion devices, and
special controllers.

## Public API

```rust
pub fn parse(bytes: &[u8]) -> Result<Cartridge, RomError>;

pub enum RomError {
    Truncated { needed: usize, got: usize },
    BadMagic,
    UnsupportedMapper(u16),
    InvalidConfig(String),
}
```

`parse` validates the magic, applies the detection rule, computes expected file length (header + optional trainer + PRG + CHR + optional misc), errors `Truncated` if short, then dispatches on mapper to construct the right `dyn Mapper` and returns `Cartridge`.

The 16-byte header itself has a separate decode/encode pair used by tooling:

```rust
pub fn parse_header(bytes: &[u8]) -> Result<Header, RomError>;
pub fn serialize_header_preserving(h: &Header, original: &[u8; HEADER_LEN]) -> [u8; HEADER_LEN];
```

Since v2.9.8 `Header` models every field the header defines, and it is
`#[non_exhaustive]`: outside `rustynes-mappers` a `Header` comes from
`parse_header`, or from `Header::default()` (an empty iNES 1.0 mapper-0 header)
plus field assignment, so a later field is not an API break. What it does not
model are the reserved bits (byte 12 bits 2-7, byte 13 for console types 0 and
2, byte 13 bits 4-7 for console type 3, byte 14 bits 2-7, byte 15 bit 7), the
exact value of a reserved Vs. PPU nibble, and iNES 1.0 bytes 8-15, which are not
part of that format.

The crate-private canonical encoding writes every field, choosing the standard
size notation where it can express the size and the exponent-multiplier
notation otherwise (the order the NESdev page prescribes). For every header
`parse_header` produces, `parse_header(canonical(h)) == h`;
`canonical_encoding_round_trips_every_parsed_header` checks that over every
byte-7 x byte-13 pair, a byte-4/byte-9 sweep and 200,000 random headers, and
`canonical_encoding_round_trips_constructed_nes2_headers` over headers built
field by field. Until v2.9.8 that encoding was public as `serialize_header`
(deprecated since v2.9.3) and lost the NVRAM nibbles, the exponent notation,
the Vs. hardware type, the extended console type and bytes 14-15; it was
removed, per ADR 0042's 2026-10-01 amendment.

`serialize_header_preserving` is the public writer. It starts
from the 16 bytes the `Header` was parsed from and rewrites only the bits of
fields whose value changed, each in its canonical encoding. An unedited header
therefore comes back byte for byte, reserved bits included, and an edit touches
only its own bits. Two
edits re-encode more: a console-type change rewrites byte 13, because byte 13
means something different for each console type, and toggling NES 2.0 re-encodes
the whole header canonically, because bytes 7-15 change meaning with the format.
Tests pin both properties: identity over every byte-7 x byte-13 pair plus
200,000 random headers, and a changed-bit mask per editable field.

v2.9.3 changed this. Until then the header editor wrote the canonical
encoding, which lost the bits listed above. That encoding also wrote mapper
bits 8-11 into byte 7's high nibble instead of bits 4-7, so every mapper from 16
up was saved wrong (mapper 66 as 2). The encoder is fixed and
`canonical_encoding_round_trips_every_mapper_id` guards it. `serialize_header`
was deprecated then and removed at v2.9.8.

## Header editor (v1.7.0 "Forge" Workstream A2, frontend tooling)

The frontend ships an **iNES / NES 2.0 header editor + read-only "Cartridge
Info" pane** (`crates/rustynes-frontend/src/debugger/header_editor.rs`,
native-only, opened from **Debug → Cartridge Info / Header Editor...**). It edits
the 16-byte header of a ROM **file on disk** — never the running core. It
decodes with `parse_header`, so it cannot drift from the loader. "Write header
to file" writes the edits over the bytes the file held with
`serialize_header_preserving` (above) and overwrites only the file's first 16
bytes (the ROM body is untouched). Sizes are edited in their 16 KiB / 8 KiB unit
counts, capped at `$EFF` units so the re-encode stays in the standard notation
(a byte-9 nibble of `$F` would select the exponent notation; until v2.9.8 the
cap was 4,095 and a count above `$EFF` was written as an exponent-notation size
nobody asked for). Since v2.9.8 every field is editable: the NVRAM sizes, the
Vs. hardware type, the extended console type, the miscellaneous ROM count and
the expansion-device code. Source inspiration:
FCEUX `iNesHeaderEditor.cpp`.

## Edge cases

1. **Lying iNES headers.** Many old dumps incorrectly set the four-screen bit, the trainer bit, or claim a mapper variant. Strategy: trust the file but expose overrides via `parse_with_override(bytes, options)` for the test harness.
2. **Trainer.** The 512-byte trainer block (if present) loads at `$7000-$71FF`. Few games use it; keep parser support but don't make UI features for it.
3. **PRG/CHR size mismatches.** If the file is shorter than the header claims, error. If longer, accept the trailing bytes as misc-ROM (NES 2.0) or warn (iNES 1.0).
4. **CHR-RAM detection.** iNES 1.0 has CHR-ROM size = 0 mean CHR-RAM. NES 2.0 separates them via byte 11.
5. **PRG-RAM detection.** iNES 1.0 has no reliable PRG-RAM size field. Strategy: if mapper is known to use PRG-RAM (MMC1, MMC3, MMC5), assume 8 KB. NES 2.0 byte 10 is authoritative.
6. **NES 2.0 submappers are not optional metadata.** MMC3 revision, VRC2/VRC4
   address wiring, BNROM/NINA variants, bus-conflict-free homebrew boards, and
   several multicarts can require submapper-specific behavior.
7. **Alternative nametable bit is mapper-specific.** Do not globally interpret
   header[6] bit 3 as "four-screen" for every mapper. Some mappers use it for
   one-screen, alternate CIRAM wiring, or mapper-specific board variants.

## Test plan

- **Round-trip parse**: parse a corpus of test ROMs, re-serialize the header, confirm byte-equivalence (with garbage zeroed for iNES 1.0).
- **Property tests**: random byte arrays starting with the magic; assert `parse` either succeeds or returns a typed error (no panics).
- **Real-world**: parse the `nes-test-roms` corpus; assert no `UnsupportedMapper` for mapper IDs in our coverage matrix.

## Open questions

- **`UNIF` format support.** UNIF (`.unf`) is an alternate, chunked container used by translation patches, pirate dumps, and multicarts. It carries no mapper number — the cartridge is identified by a board-name string in its `MAPR` chunk. **Implemented in v1.6.0 (Workstream E2):** `rustynes_mappers::unif` parses the 32-byte header + length-prefixed chunks (`MAPR`/`PRG?`/`CHR?`/`MIRR`/`BATR`/`TVCI`), resolves the board name to an iNES mapper via `board_to_mapper` (the puNES/Mesen2 table, RustyNES-implemented subset, vendor-prefix-tolerant, incl. the Sachen 8259 A/B/C/D split), and synthesizes an equivalent NES 2.0 image that flows through the standard `parse()` path (so all mapper construction is shared). `parse()` dispatches on the `"UNIF"` magic. This unlocks the UNIF-only dumps that have no iNES equivalent.
- **`.fds` Famicom Disk System format.** Out of v1.0 scope; defer to FDS support phase.
