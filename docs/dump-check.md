<!-- SPDX-License-Identifier: GPL-3.0-or-later -->
# Dump check

> **Status:** implemented in v3.5.0 (`T-PRESERVE-DUMP-VERIFY`, D33). This
> document is the spec of `rustynes dump-check`; it changes in the same commit
> as `crates/rustynes-frontend/src/dump_check.rs`.

```text
rustynes dump-check <ROM> [--dat <No-Intro DAT>] [--nes20db <nes20db.xml>] [--frames N]
```

A headless report on one ROM file: whether it is the dump it claims to be,
whether its header describes the cartridge, and whether its bytes are whole.
Exit codes: **0** nothing found, **4** at least one finding, **1** an error
before a report could be made (a file unreadable or over 64 MiB, a DAT or
database that does not parse). 2 stays clap's usage error, 3 `verify`'s
"no attestation" and 5 `speedrun`'s "the run did not finish".

**RustyNES ships no DAT and no database.** Both are files the user supplies:
a No-Intro DAT (Logiqx XML, as DAT-o-MATIC exports it) and NewRisingSun's NES
2.0 database (`nes20db.xml`, the same file `scripts/gg/` uses as a build
input). Without a DAT the verdict can be "no problem found", never "good".

## The checks

| Check | Input | What it reports |
| --- | --- | --- |
| Hashes | the file | SHA-256 and CRC-32 of the bytes after the 16-byte header (trainer included: the span headerless DATs and the NES 2.0 database's `<rom>` cover) and of the whole file |
| DAT | `--dat` | **verified** on a SHA-256 match; **CRC-32 and size only** when the matching row carries no SHA-256 (a weaker claim, said so); **not listed** otherwise. Both conventions are tried, headerless first, and the report says which matched |
| NES 2.0 database | `--nes20db` | the entry with this headerless CRC-32 and size, and every header field that disagrees with it: mapper, PRG-ROM, CHR-ROM, mirroring, battery, trainer; on a NES 2.0 header also submapper, PRG-RAM, PRG-NVRAM, CHR-RAM, CHR-NVRAM, console type, region, Vs. hardware and PPU, expansion device |
| Built-in game database | always | whether `rustynes_gamedb::correct_rom` (the load path every platform runs) would rewrite this header, byte by byte |
| Size | the header | a file shorter than the header declares (truncated), or longer with nothing declared there (trailing data) |
| Overdump | PRG-ROM, CHR-ROM | a block whose halves are identical, halved down to 8 KiB, reported as a *suspected* overdump with the probable chip size |
| Board behaviour | `--frames` (600) | the core runs the image as its header describes it, with no input, and counts three contradictions (below) |

### Comparison rules

- **iNES 1.0** headers are compared on the six fields iNES 1.0 can express;
  the report lists the other ten as not compared.
- **Mirroring** is not compared on a board that switches its own (the core's
  `mapper_has_hardwired_mirroring` is false), where the header bit is unused,
  nor against a database value of `1`, which is none of the header's three
  encodings (H, V, four-screen).
- **Expansion device 0** in a NES 2.0 header means "unspecified" and is not a
  disagreement.
- **Console type** is compared in the database's numbering: 0-2 as byte 7's
  low bits, 3 and up as byte 13's low nibble (an extended console type).
- **PlayChoice-10** data after CHR-ROM in an iNES 1.0 file (byte 7 bit 1,
  8,192 + 32 bytes) is not trailing data.

### The three behaviour rules

1. **NROM with registers.** The header says mapper 0, which has no registers,
   but the program writes `$8000-$FFFF`.
2. **CHR-ROM that is written.** The header declares CHR-ROM on a board that
   never carries CHR-RAM beside it (mappers 0, 1, 2, 3, 4, 7, 9, 10, 11, 66),
   and the program writes at least 1 KiB into the pattern tables through
   `$2007`. The PPU address is modelled from the CPU's `$2000`/`$2002`/
   `$2005`/`$2006`/`$2007` traffic.
3. **Work RAM that is not declared.** A NES 2.0 NROM header declares no
   PRG-RAM, but the program writes `$6000-$7FFF`.

## Verdict

Each finding has a severity, and the verdict is the worst: **BAD DUMP** (an
overdump, a size problem, or an image the core refuses), **WRONG HEADER** (a
database or a behaviour rule disagrees with the header), **UNVERIFIED** (a DAT
was supplied and does not list the dump). With no finding the verdict is
**GOOD** when a DAT verified the dump, otherwise **NO PROBLEM FOUND (not
verified)**.

## What it does not establish

- A clean report means nothing checked disagreed. Without a DAT the dump is
  unverified, whatever else passed.
- A repeated half is *suspected*: a board whose mask ROM really is repeated is
  indistinguishable from bytes alone. The DAT settles it.
- The behaviour run sees the first frames with no input. A game may touch
  CHR-RAM or work RAM only later, and its counts are not a coverage claim.
  The PPU address model ignores rendering's own increments of `v`.
- Boards outside rule 2's list are not judged on CHR writes: TQROM (119), the
  Waixing MMC3 clones, UNROM 512 and many multicarts map CHR-RAM and CHR-ROM
  together. Mirroring behaviour, IRQs and register maps are not judged.
- The DAT reader handles Logiqx XML only (`<game>` or `<machine>` with
  `<rom>` children); a clrmamepro text DAT is refused with a reason.

## Input handling

The ROM, the DAT and the database are untrusted. Each is read with a 64 MiB
cap. The XML scanner is a small tag reader that never panics: an unterminated
tag or quote ends the scan, an unknown entity is kept as written, every
number has a failure path, and a file in which no game is found is refused.
`the_xml_scanner_survives_malformed_input` feeds it the malformed shapes.

Every string the report takes from a file (DAT game names, the database's
names and mirroring values) and the ROM path pass through one helper,
`printable`, that turns each control character (C0, DEL, C1) into a visible
`\u{..}` escape, so a crafted DAT cannot drive the terminal with an ESC or
CSI (U+009B) sequence in a game name.
`file_strings_render_with_no_raw_control_character` pins it.

## Gates

`cargo test -p rustynes-frontend --lib dump_check` (fixtures are built in the
test, tiny NROM programs, never a dump):

| Fixture | Verdict |
| --- | --- |
| a 16 KiB NROM program, verified by a DAT built for it | GOOD |
| the same PRG read twice into 32 KiB | BAD DUMP (PRG repeats every 16 KiB) |
| a program that uploads 1 KiB of tiles, under a CHR-ROM header | WRONG HEADER (rule 2); clean under a CHR-RAM header |
| a program that writes `$8000`, under a mapper-0 header | WRONG HEADER (rule 1) |
| a header against a planted database entry | WRONG HEADER, the fields named |

## Measured on the local dump corpus

Run on 2026-10-21 over the 443 `.nes` files in the maintainer's gitignored
`tests/roms/external/` with the 2021-12-25 `nes20db.xml` and no DAT (so no
dump is "verified"). Commercial dumps are never committed; only these counts
are.

| | Count |
| --- | --- |
| exit 0 / exit 4 | 294 / 149 |
| listed in the NES 2.0 database / not listed | 393 / 49 |
| listed and every compared field agrees | 259 |
| field disagreements with the database | 203 (CHR-RAM 80, PRG-RAM 34, mapper 25, console type 24, submapper 20, battery 9, region 4, mirroring 3, PRG-NVRAM 2, expansion device 2) |
| headers the built-in game database corrects at load | 15 |
| suspected overdumps | 3 (a Vs. hack's CHR-ROM, two Nanjing PRG-ROMs) |
| size findings | 1 (one trailing byte) |
| images the core refuses as their header describes them | 3 |
| behaviour-rule findings (600 frames) | 0 |

Read the field rows as header disagreements, not as defects in the
database: 24 NES 2.0 headers say PlayChoice-10 (byte 7 = `$0A`) for a cart
the database lists as a plain NES game, and the mapper rows are mostly known
duplicate assignments (146 for Sachen's NINA clone the database files under
79; 213 for 58; 151 for 75).

The rules above were corrected by this run's first pass (120 frames, before
the board list and the exemptions existed), which reported 25 behaviour
findings, all false: boards that carry CHR-RAM beside CHR-ROM (TQROM, the
Waixing clones, UNROM 512, multicarts) and CHR-ROM games that write a few
bytes to the pattern tables by accident (1942: 96, Vs. Super Mario Bros.:
129, Dragon Warrior: 16); and 9 size findings, the PlayChoice-10 data. The
mirroring disagreements fell from 42 to 3 once boards that switch their own
mirroring were exempted, and the expansion-device ones from 105 to 2 once 0
read as unspecified.

What the corpus does not show: no real dump in it trips a behaviour rule, so
the rules' recall on a real wrong header is unmeasured. They are pinned by
the planted fixtures, and their false-positive rate on these 443 dumps is
zero.
