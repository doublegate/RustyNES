# Coverage tooling — `coverage.py`

One stdlib-only Python CLI for RustyNES's mapper / ROM / screenshot coverage
work. It replaces the old scatter of single-purpose helpers
(`scripts/rom-survey/`, the Python/Bash scripts under `scripts/screenshots/`,
and several throwaway `/tmp` scripts) with a single subcommand-driven entry
point.

> **Developer-local tooling.** This operates on a No-Intro / GoodNES NES set the
> developer owns (default root
> `~/Dropbox/ROMs/Nintendo Entertainment System - Famicom (2020)/`) and only ever
> writes into the **gitignored** `tests/roms/external/` tree. **No commercial
> Nintendo ROM is ever committed** — only the emulator's *output* (screenshots
> and `.snap` hashes) is, and that is done by the Rust harness, not this tool.

## The one pipeline

```text
index  ->  survey  ->  discover  ->  stage  ->  [Rust capture]  ->  categorize  ->  montage / report
```

1. **`index`** — walk the library once, parse every iNES / NES 2.0 / UNIF
   header, and cache a `mapper -> [titles]` JSON (`.library-index.json`,
   gitignored). ~25k headers; cached so the later steps are instant.
2. **`survey`** — per-mapper `avail` (library) vs `staged`
   (`tests/roms/external/`) vs `shots` (committed screenshots), flagging every
   implemented mapper under `--target` (default 5).
3. **`discover`** — list header-verified candidate titles per mapper (prefers
   clean `[!]` dumps and distinct base names).
4. **`stage`** — extract `>=N` distinct header-verified ROMs per mapper into
   `tests/roms/external/mapper-NNN-Board/`. **Dry-run by default**; pass
   `--execute` to write. `--unif` adds UNIF gap-fillers for boards without an
   iNES dump.
5. **Rust capture** — boot every staged ROM and emit screenshots / snapshot
   hashes. This lives in the Rust test harness, **not** here:
   `crates/rustynes-test-harness/tests/external_real_games.rs` (run with
   `--features commercial-roms,test-roms`). A data-driven `external_coverage.rs`
   variant is being developed on the `feat/coverage-harness` branch — point
   `stage` at it once it lands.
6. **`categorize`** — tier-split the screenshot tree so the screenshot category
   always matches the ROM tier (ADR 0011): `Core`/`Curated` → `screenshots/external/`,
   `BestEffort` → `screenshots/besteffort/`. Reads the live tier table from
   `crates/rustynes-mappers/src/tier.rs`.
7. **`montage` / `report`** — build the showcase montage (ImageMagick) and print
   a one-shot coverage summary.

## Subcommands

| Subcommand | What it does | Absorbed from |
|---|---|---|
| `index` | Build/cache the library `mapper -> [titles]` JSON. `--root PATH` (repeatable) adds locations; `--refresh` forces a re-scan; trailing mapper numbers print a per-mapper title sample. | `/tmp/RustyNES/mapper_index.py`, `scan_roots.py` (`.unf` board parse + `.7z` via the `7z` CLI) |
| `survey` | `avail`/`staged`/`shots` report with tier column; flags mappers `< --target` (default 5). | `/tmp/RustyNES/survey.py` |
| `discover` | Header-verified candidate titles per mapper; `--mapper N` (repeatable), `--count N`, `--all` (include unimplemented mappers seen in the library). | `scripts/rom-survey/rom_discover.py` (generalized from a hardcoded title list to every mapper) |
| `stage` | Extract `>=--ines N` distinct header-verified ROMs per mapper into `tests/roms/external/mapper-NNN-Board/`. `--unif` gap-filler, `--target N`, `--mapper N`, `--force`. **`--dry-run` is the DEFAULT**; `--execute` writes. | `scripts/rom-survey/rom_extract.py` + `/tmp/RustyNES/stage_rom.py` (header-verify) |
| `categorize` | Tier-split the screenshot tree per ADR 0011. `--dry-run` previews. | `scripts/screenshots/categorize_screenshots.py` |
| `montage` | Build `screenshots/montage.png` from the committed external screenshots (ImageMagick `montage`). | `scripts/screenshots/build_montage.sh` (+ the staging logic of `organize_screenshots.sh`) |
| `report` | One-shot summary: index stats, per-tier coverage at `--target`, fill candidates. | new (synthesis) |

## Examples

```bash
python3 scripts/coverage/coverage.py index --refresh
python3 scripts/coverage/coverage.py survey --target 5
python3 scripts/coverage/coverage.py discover --mapper 33 --count 5
python3 scripts/coverage/coverage.py stage --ines 5 --target 5          # dry-run preview
python3 scripts/coverage/coverage.py stage --ines 5 --execute           # actually write
python3 scripts/coverage/coverage.py stage --unif --execute             # gap-fill with UNIFs
python3 scripts/coverage/coverage.py categorize --dry-run
python3 scripts/coverage/coverage.py montage
python3 scripts/coverage/coverage.py report

# point at an extra library root (e.g. a second collection)
python3 scripts/coverage/coverage.py survey --root /mnt/roms/nes-extra
```

## Tier source of truth

`categorize` / `survey` / `report` read the Core / Curated / BestEffort tier sets
**live** from `crates/rustynes-mappers/src/tier.rs` (the Rust classifier, ADR
0011). If that file can't be parsed, the tool falls back to an embedded copy of
the same three id-sets. When the tier match-arms change in `tier.rs`, the tool
picks it up automatically — no second table to keep in sync.

## Board / UNIF tables

* `mapper -> board-name` (used to name the `mapper-NNN-Board/` dirs) is the
  `FAMILY` table inside `coverage.py`, cross-checked against the authoritative
  board-name column in `docs/mappers.md`.
* `UNIF board -> iNES mapper` (used to slot a `.unf` into a mapper dir) is
  `UNIF_BOARD_MAP` in `coverage.py` — see [`UNIF_BOARD_MAP.md`](UNIF_BOARD_MAP.md)
  for the full table and provenance.

## Test ROMs vs library ROMs

Two distinct concepts:

* **Library ROMs** (this tool): the developer's commercial No-Intro/GoodNES set.
  Staged into the **gitignored** `tests/roms/external/`; never committed.
* **Test ROMs** (`scripts/release-automation/stage_roms.sh`): the
  permissively-licensed CC0 / public-domain suites vendored under
  `tests/roms/nes-test-roms/`. That script force-adds those files *past* the
  gitignore so they can be **committed** as the accuracy spec. It is unrelated
  to `coverage.py` — leave it be.
