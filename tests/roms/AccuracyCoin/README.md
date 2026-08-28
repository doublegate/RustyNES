# `AccuracyCoin/` — Upstream Test Catalog (Uppercase Layout)

This directory mirrors the upstream `100thCoin/AccuracyCoin` repository
name (case-sensitive). It holds the **test-name catalog** that the
diagnostic decoder needs at compile time.

## Files

| File | Purpose |
|------|---------|
| `SOURCE_CATALOG.tsv` | 146-row TSV mapping `(suite, name) -> result-byte address`, extracted from upstream `AccuracyCoin.asm`'s `Suite_*` blocks. `include_str!`'d by `rustynes_test_harness::accuracy_coin_catalog`. |
| `sub-tests/*.nes` | Custom-built sub-test ROMs that boot directly into one target test (bypass menu + full-battery loop). Built by `scripts/accuracycoin-build/build_sub_test_rom.py`. Used to unblock the Session-22 Mesen2 wall-time oracle blocker. Inherits upstream MIT license. See `docs/audit/session-23-custom-accuracycoin-sub-test-roms-2026-05-22.md`. |

The runtime `.nes` ROM lives at [`../accuracycoin/AccuracyCoin.nes`](../accuracycoin/AccuracyCoin.nes)
(lowercase directory). The two directories exist because the runtime
harness loads the ROM from a workspace-root-relative path while the
compile-time `include_str!` reaches for a different one.

## Building another sub-test (v2.6.4)

`sub-tests/nmi-overlap-brk.nes` was added in v2.6.4 and is the first one built
after the original batch. The recipe, so the next one does not have to be
rediscovered:

```bash
mkdir -p /tmp/accoin-src && cd /tmp/accoin-src
for f in AccuracyCoin.asm nesasm.exe Tiles.pcx Sprites.pcx; do
  curl -sLO "https://raw.githubusercontent.com/100thCoin/AccuracyCoin/main/$f"
done
python3 scripts/accuracycoin-build/build_sub_test_rom.py /tmp/accoin-src \
    --suite 11 --test 1 --name "NMI Overlap BRK" \
    --out tests/roms/AccuracyCoin/sub-tests/nmi-overlap-brk.nes
```

`--suite` is the 0-based index into `TableTable` and `--test` the 0-based row
within that suite's `table "name", ...` lines; the builder's docstring carries
the suite map. It assembles through **wine + the upstream `nesasm.exe`**, which
is the upstream toolchain rather than a substitute.

**Two more were added in v2.6.5**, for the `$2007` state-machine cluster:

```bash
python3 scripts/accuracycoin-build/build_sub_test_rom.py /tmp/accoin-src \
    --suite 18 --test 7 --name "ALE + Read" \
    --out tests/roms/AccuracyCoin/sub-tests/ppu-misc-ale-read.nes
python3 scripts/accuracycoin-build/build_sub_test_rom.py /tmp/accoin-src \
    --suite 18 --test 8 --name "Hybrid Addresses" \
    --out tests/roms/AccuracyCoin/sub-tests/ppu-misc-hybrid-addresses.nes
```

Both report at their **catalog** addresses (`$0491`, `$0492`) — verified from a
RAM diff, not assumed — and both reach a verdict in **8.93M cycles** against the
full battery's 134M, which is the whole reason to build them.

**`/usr/local/bin/wine` may not be wine.** On the development machine it is a
symlink to **firejail**, which shadows the real binary at `/usr/bin/wine` on
`PATH`; the builder then assembles nothing and the failure does not name wine.
Run it as `PATH=/usr/bin:$PATH python3 scripts/...` if `wine --version` prints
anything other than a wine version.

Re-fetch `AccuracyCoin.asm` rather than reusing a local copy, and check it
matches: the builder rewrites one routine in the source it is handed, so a
source that already drifted produces a ROM that looks fine and tests something
else.

**`sub-tests/cpu-open-bus.nes` does not run `Open Bus`.** Measured in v2.6.4:
its verdict lands at **`$0407`**, which the catalog assigns to *Dummy write
cycles*, and `$0408` (`Open Bus`) is never written. It is off by one row of
`Suite_CPUBehavior` — a valid stimulus under the wrong name. It is kept as-is
rather than renamed, because a gate may already reference it; the correctly
built one is **`sub-tests/open-bus.nes`** (`--suite 0 --test 7`), added in the
same release and verified to report at `$0408`.

Two of the three ROMs used for v2.6.4's rung-5 work therefore report at their
catalog addresses and one does not, which is why the paragraph below says to
read the address out of a RAM diff.

**Why these matter for co-simulation.** The full battery is 17,868,316 CPU
cycles and needs a START press at a specific frame. A sub-test reaches its
verdict in **0.9M** (`iflag-latency`) to **4.5M** (`nmi-overlap-brk`) cycles from
boot, with no input at all — so a DUT iteration that took minutes takes seconds,
and the verdict byte names one assertion instead of one entry among 146. The
result addresses are **not** always the catalog's: `iflag-latency` and
`nmi-overlap-brk` report at their catalog addresses (`$0461`, `$0462`) and
`cpu-open-bus` reports at **`$0407`**, one below its catalog `$0408`. Read the
address out of a RAM diff rather than assuming.

## Catalog format

```text
<suite-name>\t<test-name>\t<ram-address-hex>
```

Each row maps one logical test to the CPU RAM byte that
AccuracyCoin's `TEST_Pass` / `TEST_Fail` macros write its `(N<<2)|bit`
status into. The decoder in `rustynes_test_harness::accuracy_coin_catalog`
parses the TSV at first access (`OnceLock`-lazy) and pairs it with the
post-battery 2 KiB RAM dump produced by
`accuracy_coin::run_battery_capturing_ram` to compute per-test
pass / fail breakdowns.

## Source

`https://github.com/100thCoin/AccuracyCoin` (main branch; re-synced to
upstream commit `71f57fb` in v2.0.1). Extraction recipe (inline — the
authoritative source is `AccuracyCoin.asm` itself, not a prose doc): walk
each `Suite_*` block, and for every `table "name", $FF, result_symbol,
TEST_addr` macro entry emit a `(suite, test-name, ram-addr)` triple,
resolving `result_symbol` to its `result_X = $ADDR` definition. The v2.0.1
re-sync added the two newest PPU tests ("ALE + Read" `$0491`, "Hybrid
Addresses" `$0492`), growing the catalog 144 -> 146 rows / 139 -> 141
assigned tests.

## License

MIT (same as the runtime ROM; full text in
[`../accuracycoin/LICENSE`](../accuracycoin/LICENSE)).

## Why not deduplicate?

Both directories are referenced by code:

- `crates/rustynes-test-harness/src/accuracy_coin.rs:176-177` — runtime ROM
  path: `tests/roms/accuracycoin/AccuracyCoin.nes`.
- `crates/rustynes-test-harness/src/accuracy_coin_catalog.rs:64` — compile-time
  TSV: `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv`.

Merging them would require renaming the source files in both crates and
regenerating the per-suite pass-rate baselines. Cost > benefit. The
two-directory layout is the canonical path going forward.
