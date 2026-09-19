# `AccuracyCoin/` — Upstream Test Catalog (Uppercase Layout)

This directory mirrors the upstream `100thCoin/AccuracyCoin` repository
name (case-sensitive). It holds the **test-name catalog** that the
diagnostic decoder needs at compile time.

## Files

| File | Purpose |
|------|---------|
| `SOURCE_CATALOG.tsv` | 149-row TSV mapping `(suite, name) -> result-byte address`, extracted from upstream `AccuracyCoin.asm`'s `Suite_*` blocks by `scripts/accuracycoin-build/extract_catalog.py`. `include_str!`'d by `rustynes_test_harness::accuracy_coin_catalog`. |
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
# Fetch the upstream source into a scratch dir...
mkdir -p /tmp/accoin-src && cd /tmp/accoin-src
for f in AccuracyCoin.asm nesasm.exe Tiles.pcx Sprites.pcx; do
  curl -sLO "https://raw.githubusercontent.com/100thCoin/AccuracyCoin/main/$f"
done

# ...then come BACK. Both the builder path and `--out` are repo-relative, and
# the `cd` above leaves the shell in /tmp/accoin-src, where neither resolves.
cd "$(git -C ~/Code/OSS_Public-Projects/RustyNES rev-parse --show-toplevel)"

python3 scripts/accuracycoin-build/build_sub_test_rom.py /tmp/accoin-src \
    --suite 11 --test 1 --name "NMI Overlap BRK" \
    --out tests/roms/AccuracyCoin/sub-tests/nmi-overlap-brk.nes
```

`--suite` is the 0-based index into `TableTable` and `--test` the 0-based row
within that suite's `table "name", ...` lines. It assembles through **wine + the
upstream `nesasm.exe`**, which is the upstream toolchain rather than a
substitute.

> **The indices in the recipes on this page are relative to the upstream source
> of their day, and several are stale.** Upstream has reordered its suites and
> inserted tests within them since; the builder's docstring keeps the old map
> struck through for exactly that reason. `--suite 18 --test 7` built
> `ALE + Read` in v2.6.5 and does not today — at `46199ae4` that slot is
> `OAM Corruption`, and `ALE + Read` is suite 20, test 3. **Derive the indices,
> never transcribe them**: `python3 scripts/accuracycoin-build/derive_indices.py
> <src>` reads them out of the assembly, and the builder cross-checks
> `--suite`/`--test`/`--name` against it before writing a ROM. The recipes are
> kept as a record of how each existing ROM was produced, not as commands to
> re-run.

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

### Which entry each sub-test ROM runs is MEASURED, and recorded

`sub-tests/BUILD-PROVENANCE.tsv` carries a row per ROM: the encoded
`(suite, test)` immediates, the **result address it writes**, the catalog entry
that address belongs to, and the oracle's verdict. The address column is the
identity — it is the catalog's own key and is stable across upstream
reorderings, which is what the encoded index is not.

It is produced by running the ROMs, not by reading their names:

```bash
cargo run -p rustynes-test-harness --release --features test-roms \
    --bin subtest_identify -- --tsv tests/roms/AccuracyCoin/sub-tests/*.nes
```

and `crates/rustynes-test-harness/tests/accuracycoin_subtest_provenance.rs`
re-measures every row, so a swapped, rebuilt or renamed ROM fails rather than
being trusted. The whole 33-ROM sweep is ~16 s.

**`sub-tests/cpu-open-bus.nes` does not run `Open Bus`.** Measured in v2.6.4 and
re-measured on 2026-09-19: its verdict lands at **`$0407`**, which the catalog
assigns to *Dummy write cycles*. It is a valid stimulus under the wrong name,
kept as-is rather than renamed because a gate may already reference it; the
correctly built one is **`sub-tests/open-bus.nes`**, verified to report at
`$0408`.

**`sub-tests/ppu-misc-2004-stress.nes` does not run `$2004 Stress Test`** — found
by that sweep. It writes `$048E`, which is *`$2007 Stress Test`*, the same entry
as `ppu-misc-2007-stress.nes`. So the corpus holds two ROMs for one entry and
**none** for `$2004 Stress Test` (`$048C`) while appearing to hold one. Neither
is registered in rung 5, so nothing relied on the name.

**Why these matter for co-simulation.** The full battery is 17,868,316 CPU
cycles and needs a START press at a specific frame. A sub-test reaches its
verdict in **0.9M** (`iflag-latency`) to **4.5M** (`nmi-overlap-brk`) cycles from
boot, with no input at all — so a DUT iteration that took minutes takes seconds,
and the verdict byte names one assertion instead of one entry among 146. Which
address a ROM writes is in `BUILD-PROVENANCE.tsv`, measured — do not infer it
from the filename.

This file used to describe those addresses as "not always the catalog's". That
is the wrong way round, and the provenance sweep settles it: **every one of the 33
ROMs writes the catalog address of some entry, exactly.** What varies is whether
that entry is the one the *filename* names.

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
upstream commit `46199ae4` on 2026-09-19; previously `69c8860`, 2026-09-11,
and `71f57fb` in v2.0.1). The `46199ae4` re-sync left this catalog
**byte-identical** — re-running `extract_catalog.py` against the new
`AccuracyCoin.asm` reproduces the committed TSV exactly, because the two
upstream commits insert `INC <ErrorCode` instructions, which move code
without moving any result address.

**The extraction is a script, not a recipe.** It used to be the prose
paragraph that stood here — walk each `Suite_*` block, emit a triple per
`table` macro row, resolve `result_symbol` through its `result_X = $ADDR`
definition. That is accurate and it could not be re-run or audited, so each
re-sync re-derived it by hand. It is now
`scripts/accuracycoin-build/extract_catalog.py`:

```bash
python3 scripts/accuracycoin-build/extract_catalog.py --self-test
python3 scripts/accuracycoin-build/extract_catalog.py /path/to/AccuracyCoin.asm \
    --out tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv
```

Two things the script settles that the prose did not. It orders rows by
upstream's `TableTable`, which is the ROM's own display order, rather than
by position in the file — the two can disagree. And running it against the
asm at `71f57fb` reproduced the committed 146-row TSV **byte-for-byte except
one row**: the hand extraction had filed "Attributes As Tiles" under
`PPU Misc.` where upstream has it in `Suite_PPUBehavior`. Same result
address, so no verdict was ever wrong — only the per-suite breakdown was.

The **2026-09 re-sync** grew the catalog 146 -> 149 rows / 141 -> 144
assigned tests across 20 -> 22 suites. Upstream removed nothing; it added
three tests (`Frozen OAM2 Increment` `$0493`, `Misaligned OAM DMA` `$0494`,
`Misaligned OAM2 Address` `$0495`) and added two pages, `Advanced Background
Evaluation` and `Advanced Sprite Evaluation`, which re-home eleven existing
PPU tests out of `PPU Misc.`, `PPU Behavior` and `Sprite Evaluation`. A
re-sync is therefore not an append: a suite-keyed baseline must be
regenerated, not extended.

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

### Sub-tests the harness cannot isolate

Two sub-test ROMs build correctly and are **deliberately unregistered**, because
the ORACLE does not pass them. `subtest_verdict.py` refuses a comparison whose
oracle side is not a pass — correctly, since a ROM the reference implementation
fails cannot adjudicate anything about the DUT.

| ROM | suite/test | oracle verdict | why |
|---|---|---|---|
| `sprite-eval-arbitrary-sprite-zero.nes` | — | Fail | pre-existing; see the rung-5 notes |
| `sprite-zero-hit-behavior.nes` | 17 / 1 | `$457 = $06`, Fail(test 1) | the streamlined boot omits state the full battery establishes |

For `sprite-zero-hit-behavior` the failure is the FIRST assertion — "does a
sprite zero hit occur in a situation in which it should" — so the test never
gets as far as the behaviour it exists to check. `TEST_Sprite0Hit_Behavior`
expects "a solid white square … placed at VRAM address $2001" and a sprite zero
overlapping it; the builder replaces `AutomaticallyRunEveryTestInROM` with a
runner that calls `LoadSuiteMenuNoRendering` and `RunTest` once, which does not
reproduce everything the full battery has done to VRAM and the pattern tables by
the time this entry runs.

The ROM and its golden are kept rather than deleted: they are the evidence that
the isolation was attempted and why it does not work, and the entry has to be
debugged against the full 4500-frame battery instead.
