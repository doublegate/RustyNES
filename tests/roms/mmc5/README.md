# `mmc5/` — MMC5 Mapper Test ROMs

Accuracy test ROMs for mapper 5 (Nintendo MMC5 / ExROM), the most
complex Nintendo first-party mapper. Used by Castlevania III (USA),
Bandit Kings of Ancient China, Uchuu Keibitai SDF, and others.

| File | Suite | Mapper | Author | License |
|------|-------|--------|--------|---------|
| `mapper_mmc5test_v1.nes` | MMC5 banking | MMC5 (5) | (Various, aggregator) | Public domain |
| `mapper_mmc5test_v2.nes` | MMC5 banking + IRQ | MMC5 (5) | (Various, aggregator) | Public domain |
| `mapper_mmc5exram.nes` | MMC5 ExRAM modes | MMC5 (5) | (Various, aggregator) | Public domain |

## Source

`christopherpow/nes-test-roms/mmc5test/{mmc5test_v1,mmc5test_v2,mmc5exram}.nes`.
The aggregator's root README places the corpora under public-domain
terms.

## What they test

- **`mapper_mmc5test_v1.nes`** — Basic MMC5 PRG/CHR banking and
  banking modes (modes 0..3 for PRG, modes 0..3 for CHR).
- **`mapper_mmc5test_v2.nes`** — Extended banking tests plus the
  scanline IRQ counter.
- **`mapper_mmc5exram.nes`** — All four ExRAM modes:
  - 00: extra nametable
  - 01: ExGrafix per-tile attributes
  - 10: general-purpose 1 KiB WRAM
  - 11: read-only ExRAM

The integration test at
`crates/nes-test-harness/tests/mmc5.rs` drives each of these ROMs
through the standard `$6000` blargg-style status protocol where
applicable, plus a small set of property tests against the `nes-mappers`
crate.

## See also

- The committed audio test for MMC5 is at
  [`../audio-tests/db_mmc5.nes`](../audio-tests/db_mmc5.nes) — exercises
  the raw-PCM amplitude path through `Apu::tick_with_external`.
- The Holy Mapperel suite does **not** include an M5 ROM (MMC5 is too
  variant-rich to fit Yerrick's PCB-detection model).

## License

Public domain via the aggregator README.
