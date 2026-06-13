# Commercial-ROM compatibility-survey tooling

Developer-local helpers used to build the v2.4.0 **99-title commercial-ROM
compatibility survey** (the 39-title `external_extended` oracle) and to extract
the mapper-119 ROMs. They operate on a **No-Intro NES set** the developer owns
and populate the **gitignored** `tests/roms/external/` tree — **no commercial
ROMs are ever committed**.

> ⚠️ Both scripts hardcode the developer's local set path
> (`~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)/`). Edit the
> `DB`/`DIRS` constants for your own layout.

| Script | What it does |
|---|---|
| `rom_discover.py` | For a curated list of tricky-to-emulate + best-seller candidates, finds each game's `.zip` (top-level + `Homebrew & Unlicensed/` + `Japan/`), reads the iNES mapper byte, and reports `mapper N [OK/UNSUP]` against the 38→39 supported-mapper set. Read-only — picks the final extract set without touching the project. |
| `rom_extract.py` | Extracts the curated set's `.nes` out of the zips into `tests/roms/external/mapper-NNN-FAMILY/` (auto mapper-dir naming via the iNES header). The destination is gitignored. |

## Workflow (extending the survey)

```bash
# 1. curate: edit the CANDIDATES list, then see what's available + its mapper
python3 scripts/rom-survey/rom_discover.py

# 2. extract the chosen set into the gitignored external/ tree
python3 scripts/rom-survey/rom_extract.py

# 3. boot-and-screenshot survey (the committed diagnostic bin) + visually review
cargo run -p rustynes-test-harness --features commercial-roms --release \
    --bin coverage_smoke -- tests/roms/external 280 /tmp/rustynes-coverage
# montage /tmp/rustynes-coverage/*.png and inspect — the oracle is a regression
# gate, not a correctness check, so VISUAL review is what catches rendering bugs.

# 4. lock verified games as regression tests in
#    crates/rustynes-test-harness/tests/external_extended.rs (+ commit the .snap hashes)
```

See `docs/release-notes/v2.4.0.md` for the survey that found the VRC7/VRC2
rendering bugs, and `docs/SALVAGE_MANIFEST.md` for the salvage provenance.
