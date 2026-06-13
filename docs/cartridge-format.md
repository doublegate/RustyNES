# Cartridge file format (iNES + NES 2.0)

**References:** `ref-docs/research-report.md` §Cartridge format and mappers
→ iNES and NES 2.0; `ref-docs/nesdev-wiki-technical-report.md` §ROM And
Music File Formats; Nesdev [iNES](https://www.nesdev.org/wiki/INES) and
[NES 2.0](https://www.nesdev.org/wiki/NES_2.0).

## Purpose

Parse iNES 1.0 and NES 2.0 ROM files into a `Cartridge` value that the mapper subsystem can use.

## File structure

```
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
| 13 | Vs. PPU type (bits 0-3) or extended console type (bits 4-7) — NES 2.0 only |
| 14 | Misc ROM count (bits 0-1) — NES 2.0 only |
| 15 | Default expansion device (bits 0-5) — NES 2.0 only |

### Detection rule

```
is_nes2 = (header[7] & 0x0C) == 0x08;
```

If false, parse as iNES 1.0, but treat upper mapper bits cautiously. Older
tools often wrote non-zero padding or signature strings into bytes 7-15,
corrupting mapper high bits. A strict loader may reject dirty headers; RustyNES
parses leniently for compatibility but should surface a diagnostic when dirty
padding changes the mapper or region interpretation.

### Mapper number

```
mapper = (header[6] >> 4) | (header[7] & 0xF0) | (if is_nes2 { (header[8] & 0x0F) << 8 } else { 0 });
```

iNES 1.0 mapper numbers cover 0..=255. NES 2.0 extends to 0..=4095.

### ROM sizing

**Standard notation** (MSB nibble != `$F`): `size = ((MSB << 8) | LSB) * unit_size`. PRG unit = 16 KiB; CHR unit = 8 KiB.

**Exponent-multiplier** (MSB nibble == `$F`): the LSB byte encodes `EEEEEEMM` where E is exponent (6 bits) and MM is multiplier code; size = `2^E * (MM*2 + 1)` bytes. MM values map to multipliers 1, 3, 5, 7. Used for non-power-of-2 ROM sizes.

### RAM sizing (NES 2.0)

```
prg_ram_size = if shift == 0 { 0 } else { 64 << shift };
```

Same encoding for CHR-RAM and the NVRAM variants.

### Mirroring

iNES: bit 0 of header[6] = vertical (1) or horizontal (0); bit 3 = four-screen override. NES 2.0 same encoding (mirroring is mostly a property of mapper-controlled state for any non-trivial mapper — this header bit is the *initial* mirroring).

### Console type

NES 2.0 byte 7 bits 0-1: `00` = NES/Famicom, `01` = Vs. System, `10` = Playchoice 10, `11` = extended (see byte 13).

### Region (NES 2.0)

Byte 12 bits 0-1: `00` = NTSC, `01` = PAL, `10` = multi-region, `11` = Dendy. iNES 1.0 has only the legacy bit in byte 9 (largely useless; many ROMs are mis-tagged).

### Default input device (NES 2.0)

Byte 15 bits 0-5 identify the default expansion or input device. The current
frontend assumes standard controllers unless mapper or test harness metadata
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

- **`UNIF` format support.** UNIF is an alternate format used by some translation patches and pirate dumps. Out of v1.0 scope.
- **`.fds` Famicom Disk System format.** Out of v1.0 scope; defer to FDS support phase.
