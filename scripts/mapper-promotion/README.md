# `scripts/mapper-promotion/` — salvaged mapper tier-promotion research tooling

Ad-hoc Python/Rust scripts from the **v2.1.0 "Fathom" F3 mapper-tier promotion**
batch — the pass that moved 86 previously-`BestEffort` mapper families to
`Curated` by staging a cleanly-booting commercial ROM dump for each and wiring
it into a byte-identity boot-snapshot test (see `docs/adr/0011-mapper-tiering.md`
and `crates/rustynes-mappers/src/tier.rs`).

**These are ad-hoc, not maintained tooling** — the same status as `scripts/diag/`
and `scripts/gg/`. They assume a local, gitignored `tests/roms/external/` corpus
that is never committed, and several hardcode paths from the session that
produced them. Treat them as starting points for the next promotion batch, not
as turnkey utilities.

| Script | What it does |
|---|---|
| `mapper_scan.py` | Walks a ROM corpus (including `.zip`) and tallies iNES mapper IDs, to find which families have a candidate dump. |
| `scan.py` | Narrower corpus scan used to locate a specific mapper's ROMs by header. |
| `enumerate_staged.py` | Enumerates the ROMs already staged under `tests/roms/external/` and verifies each parses with the expected mapper ID. |
| `batch2.py` | Copies the second batch (30 GoodNES ROMs) into `tests/roms/external/`, one per family. |
| `gen_promotion.py` | Generates the `external_extended.rs` test blocks and the `tier.rs` ID-list edits for a promotion batch. |
| `convert_gg.py` | Game Genie code conversion helper retained from the same session. |
| `promo_tests.rs` / `promo_tests_2.rs` | Generated test bodies from the two batches, kept as the record of what was emitted. |

Never commit the ROM dumps these operate on. `tests/roms/external/` is gitignored
by design; only screenshots and `.snap` snapshots are committed.

See `docs/STATUS.md` for the current tier matrix, which is the authoritative
count — not anything printed by these scripts.
