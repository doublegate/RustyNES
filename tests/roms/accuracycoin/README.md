# `accuracycoin/` — AccuracyCoin Battery (Lowercase Layout)

The runtime ROM + license file for Chris Siebert's AccuracyCoin battery.
The lowercase directory is the path that the runtime harness expects;
the uppercase [`../AccuracyCoin/`](../AccuracyCoin/) directory holds the
upstream test catalog (TSV) that the diagnostic decoder needs as well as
a synced copy of the ROM.

## Files

| File | Author | License |
|------|--------|---------|
| `AccuracyCoin.nes` | Chris Siebert (100thCoin) | MIT |
| `LICENSE` | Chris Siebert | MIT (full text) |

## Source

`https://github.com/100thCoin/AccuracyCoin` (main branch, fetched
2026-05-10). Repository LICENSE is the MIT License,
"Copyright (c) 2025 Chris Siebert".

## What it is

AccuracyCoin is a single-NROM-cartridge battery of **144 NES accuracy
tests** spanning CPU, PPU, APU, bus, IRQ, NMI, dummy-read / dummy-write,
DMA, and mapper behaviour. It is interactive on real hardware (the user
navigates with D-Pad / A / Start), but our harness uses a fixed
button-press script that triggers "run all" and then reads the result
addresses out of CPU RAM directly.

Current pass rate (measured via the Phase D2 RAM-direct decoder):
**67.63%** (94 pass + 1 pass-with-code of 139 assigned tests).
Floor: 0.60. v1.0.0 gate: 0.90. See `docs/STATUS.md` for the
breakdown.

## Harness

| Harness file | Purpose |
|--------------|---------|
| `crates/nes-test-harness/src/accuracy_coin.rs` | Drives ROM from power-on; reads pass-rate from RAM via the catalog decoder. |
| `crates/nes-test-harness/src/accuracy_coin_catalog.rs` | `OnceLock`-lazy 144-entry catalog parsed from the TSV in `../AccuracyCoin/SOURCE_CATALOG.tsv`. |
| `crates/nes-test-harness/tests/accuracycoin.rs` | The CI gate — prints per-suite breakdown + per-failing-test list. |

## Why two directories?

The integration harness `include_str!`s the upstream test catalog
(`../AccuracyCoin/SOURCE_CATALOG.tsv`) at compile time. The same harness
also reads the `.nes` ROM at runtime via the workspace-root-relative
path `tests/roms/accuracycoin/AccuracyCoin.nes`. The two directories
exist because of this case-sensitivity split — they are NOT duplicates
in spirit. The uppercase directory mirrors the upstream repository name.

## License

MIT (full text in `LICENSE`).
