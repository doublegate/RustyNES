# `accuracycoin/` — AccuracyCoin Battery (Lowercase Layout)

The runtime ROM + license file for Chris Siebert's AccuracyCoin battery.
The lowercase directory is the path that the runtime harness expects;
the uppercase [`../AccuracyCoin/`](../AccuracyCoin/) directory holds the
upstream test catalog (TSV) that the diagnostic decoder needs, plus the
custom sub-test ROMs. It holds no copy of the battery ROM itself.

## Files

| File | Author | License |
|------|--------|---------|
| `AccuracyCoin.nes` | Chris Siebert (100thCoin) | MIT |
| `LICENSE` | Chris Siebert | MIT (full text) |

## Source

`https://github.com/100thCoin/AccuracyCoin` (main branch, commit `46199ae4`,
fetched 2026-09-19; previously `69c8860`, fetched 2026-09-11, and `71f57fb`,
fetched 2026-05-10). Repository LICENSE is the MIT License,
"Copyright (c) 2025 Chris Siebert".

The `69c8860` -> `46199ae4` window is two commits and **165 differing ROM
bytes**, and both commits are the same one-line defect in two places: a
missing `INC <ErrorCode` between sub-tests, which made two sub-tests of one
routine report the *same* failure code and so made `Fail(N)` ambiguous.
`9bc42d1e` fixed it in `TEST_MisalignedOAM2Addr`; `46199ae4` fixed it in
`TEST_FrozenOAM2Inc2` — the instance this project reported upstream. It
changes no verdict here, because the codes only distinguish *failures* and
the battery passes; re-extracting `SOURCE_CATALOG.tsv` from the new
`AccuracyCoin.asm` reproduces the committed TSV **byte-identically**, since
the insertion moves code and not the result-address map.

## What it is

AccuracyCoin is a single-NROM-cartridge battery of **144 NES accuracy
tests** (plus 5 print-only `DRAW` tests) spanning CPU, PPU, APU, bus, IRQ, NMI, dummy-read / dummy-write,
DMA, and mapper behaviour. It is interactive on real hardware (the user
navigates with D-Pad / A / Start), but our harness uses a fixed
button-press script that triggers "run all" and then reads the result
addresses out of CPU RAM directly.

Current pass rate (measured via the RAM-direct decoder), against a catalog
of 149 rows / 144 assigned tests: **100.00%** (144 of 144). The last two
gaps were both on the `Advanced Sprite Evaluation` page — "Misaligned OAM2
Address" closed in v2.6.17, and "Frozen OAM2 Increment" in v2.6.18 on the
rule that a `$2001` mask change during dot N must not act on dot N
(`docs/accuracy-ledger.md`). Floor: 0.60.
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
