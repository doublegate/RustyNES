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

Current pass rate (measured via the RAM-direct decoder), after the
v2.0.1 upstream re-sync grew the catalog to 146 rows / 141 assigned tests:
**98.58%** (139 of 141 assigned tests). The two new upstream PPU tests
("ALE + Read", "Hybrid Addresses") are known gaps. Floor: 0.60. See
`docs/STATUS.md` for the authoritative breakdown.

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
