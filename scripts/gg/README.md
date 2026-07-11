# Game Genie header-robust re-key

Tooling that regenerates
`crates/rustynes-frontend/src/genie_database_headerless.tsv` — the Game Genie
code catalog re-keyed to the **header-excluded** `rom_crc32`, which is what gives
the Cheats panel header-insensitive matching for all ~520 games (a re-headered
dump still matches, because the key depends only on PRG + CHR content, not the
16-byte iNES header).

## Why a re-key exists

The bulk catalog `genie_database_full.tsv` is keyed by the **full-file** No-Intro
CRC32 (header included), so it only matches a dump whose header is byte-identical
to No-Intro's. This tool takes the same codes and re-keys them to the
header-excluded content CRC that `game_db::rom_crc32` computes for any dump of the
game — so nomination works regardless of the header "flavor".

## Files

| File | Committed? | Purpose |
|------|-----------|---------|
| `gen_headerless_genie_db.py` | yes | The generator. Joins the cheat catalog to nes20db by normalized game name. |
| `alias_crcs.py` | yes | Manual name-alias table (game name → header-excluded CRC list) for the ~45 long-tail titles that fail the automatic name-join (Japanese titles, subtitle variants, article suffixes, etc.). |
| `nes20db.xml` | **no** (gitignored) | Build-time input: NewRisingSun's NES 2.0 database. Supplies each game's header-excluded `<rom crc32>`. |

## Regenerate

```bash
# 1. Fetch the NES 2.0 database (build-time input, not committed):
curl -L -o scripts/gg/nes20db.xml \
  https://raw.githubusercontent.com/Kreeblah/NES20Tool/master/nes20db/nes20db.xml

# 2. Regenerate the committed .tsv:
python3 scripts/gg/gen_headerless_genie_db.py
```

The generator prints coverage (games covered / uncovered, row count) and the
output's raw + gzip size. With the current inputs it covers **521/521 games**
(45 via the alias table), ~16.5k rows, ~244 KiB gzip.

## Data policy

`nes20db.xml` is a build-time input only and is never committed. The only
committed output is factual CRC32 dump identifiers + factual Game Genie codes —
non-copyrightable data, matching the project's policy (the same precedent as
`genie_database_full.tsv`, generated from libretro-database + the No-Intro DAT).
Commercial ROMs are never committed.
