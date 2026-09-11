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

`https://github.com/100thCoin/AccuracyCoin` (main branch, commit `69c8860`,
fetched 2026-09-11; previously `71f57fb`, fetched 2026-05-10). Repository LICENSE is the MIT License,
"Copyright (c) 2025 Chris Siebert".

## What it is

AccuracyCoin is a single-NROM-cartridge battery of **144 NES accuracy
tests** (plus 5 print-only `DRAW` tests) spanning CPU, PPU, APU, bus, IRQ, NMI, dummy-read / dummy-write,
DMA, and mapper behaviour. It is interactive on real hardware (the user
navigates with D-Pad / A / Start), but our harness uses a fixed
button-press script that triggers "run all" and then reads the result
addresses out of CPU RAM directly.

Current pass rate (measured via the RAM-direct decoder), after the
2026-09 upstream re-sync grew the catalog to 149 rows / 144 assigned tests:
**99.31%** (143 of 144 assigned tests). The one gap is the new
`Advanced Sprite Evaluation` test "Frozen OAM2 Increment", secondary-OAM
address behaviour during a rendering toggle that this PPU does not model;
its cause is named in `docs/accuracy-ledger.md` (the PPU register write
lands at M2-low where a 6502 commits at phi2). "Misaligned OAM2 Address"
was the second gap and closed in v2.6.17. Nothing that passed before
stopped passing. Floor: 0.60.
See `docs/STATUS.md` for the authoritative breakdown.

## Harness

| Harness file | Purpose |
|--------------|---------|
| `crates/rustynes-test-harness/src/accuracy_coin.rs` | Drives ROM from power-on; reads pass-rate from RAM via the catalog decoder. |
| `crates/rustynes-test-harness/src/accuracy_coin_catalog.rs` | `OnceLock`-lazy 149-entry catalog parsed from the TSV in `../AccuracyCoin/SOURCE_CATALOG.tsv`. |
| `crates/rustynes-test-harness/tests/accuracycoin.rs` | The CI gate — prints per-suite breakdown + per-failing-test list. |

## Why two directories?

The integration harness `include_str!`s the upstream test catalog
(`../AccuracyCoin/SOURCE_CATALOG.tsv`) at compile time. The same harness
also reads the `.nes` ROM at runtime via the workspace-root-relative
path `tests/roms/accuracycoin/AccuracyCoin.nes`. The two directories
exist because of this case-sensitivity split — they are NOT duplicates
in spirit. The uppercase directory mirrors the upstream repository name.

## License

MIT (full text in `LICENSE`).
