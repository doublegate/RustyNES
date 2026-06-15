# `AccuracyCoin/` — Upstream Test Catalog (Uppercase Layout)

This directory mirrors the upstream `100thCoin/AccuracyCoin` repository
name (case-sensitive). It holds the **test-name catalog** that the
diagnostic decoder needs at compile time.

## Files

| File | Purpose |
|------|---------|
| `SOURCE_CATALOG.tsv` | 144-row TSV mapping `(suite, name) -> result-byte address`, extracted from upstream `AccuracyCoin.asm`'s `Suite_*` blocks. `include_str!`'d by `nes-test-harness::accuracy_coin_catalog`. |
| `sub-tests/*.nes` | Custom-built sub-test ROMs that boot directly into one target test (bypass menu + full-battery loop). Built by `scripts/accuracycoin-build/build_sub_test_rom.py`. Used to unblock the Session-22 Mesen2 wall-time oracle blocker. Inherits upstream MIT license. See `docs/audit/session-23-custom-accuracycoin-sub-test-roms-2026-05-22.md`. |

The runtime `.nes` ROM lives at [`../accuracycoin/AccuracyCoin.nes`](../accuracycoin/AccuracyCoin.nes)
(lowercase directory). The two directories exist because the runtime
harness loads the ROM from a workspace-root-relative path while the
compile-time `include_str!` reaches for a different one.

## Catalog format

```text
<suite-name>\t<test-name>\t<ram-address-hex>
```

Each row maps one logical test to the CPU RAM byte that
AccuracyCoin's `TEST_Pass` / `TEST_Fail` macros write its `(N<<2)|bit`
status into. The decoder in `nes-test-harness::accuracy_coin_catalog`
parses the TSV at first access (`OnceLock`-lazy) and pairs it with the
post-battery 2 KiB RAM dump produced by
`accuracy_coin::run_battery_capturing_ram` to compute per-test
pass / fail breakdowns.

## Source

`https://github.com/100thCoin/AccuracyCoin` (main branch). The TSV was
extracted from `AccuracyCoin.asm` by walking each `Suite_*` block and
emitting `(suite, test-name, ram-addr)` triples. See
`docs/testing-strategy.md` for the extraction recipe.

## License

MIT (same as the runtime ROM; full text in
[`../accuracycoin/LICENSE`](../accuracycoin/LICENSE)).

## Why not deduplicate?

Both directories are referenced by code:

- `crates/nes-test-harness/src/accuracy_coin.rs:176-177` — runtime ROM
  path: `tests/roms/accuracycoin/AccuracyCoin.nes`.
- `crates/nes-test-harness/src/accuracy_coin_catalog.rs:64` — compile-time
  TSV: `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv`.

Merging them would require renaming the source files in both crates and
regenerating the per-suite pass-rate baselines. Cost > benefit. The
two-directory layout is the canonical path going forward.
