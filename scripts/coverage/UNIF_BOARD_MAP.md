# UNIF board → iNES mapper table

UNIF (`.unf` / `.unif`) carries no mapper number — it identifies the cartridge
by a **board name** string in its `MAPR` chunk. To slot a UNIF ROM into the
`mapper-NNN-Board/` coverage tree, the board name must be resolved to its iNES
mapper number. `coverage.py` does that with the `UNIF_BOARD_MAP` dict; this
document is the human-readable record of it.

## Provenance

The table is sourced from two reference emulators' UNIF board tables, then
cross-checked against `docs/mappers.md` (the authoritative RustyNES board-name
column):

* **Mesen2** — its UNIF board → mapper mapping.
* **puNES** — `src/core/unif.c` board-name table.

Where the two references agree, the value is used directly. Where a board name
has ambiguous variants, the variant suffix is honored (see Sachen 8259 below).

## Sachen 8259 family (suffix-disambiguated — verified)

The Sachen 8259 protection chip ships in four pin-strap variants that the UNIF
board name distinguishes by suffix, mapping to **different** iNES mappers. This
is the one place a naive "8259 → one mapper" guess is wrong; both references
agree on the split:

| UNIF board | iNES mapper |
|---|---|
| `SACHEN-8259A` | 141 |
| `SACHEN-8259B` | 138 |
| `SACHEN-8259C` | 139 |
| `SACHEN-8259D` | 137 |

(RustyNES implements 137 = Sachen 8259D; the A/B/C variants resolve correctly
for staging/gap-fill reporting even when not yet implemented.)

## Coverage

`UNIF_BOARD_MAP` covers the board names that correspond to RustyNES's
implemented mapper families (Nintendo discrete, Konami VRC, Sunsoft, Irem,
Jaleco, Namco, Taito, Camerica/Codemasters, AVE, Sachen, and the common
homebrew/multicart boards). Board names with no implemented iNES equivalent
(many `BMC-*` multicart boards) are intentionally **not** in the table —
`coverage.py stage --unif` reports them as `unknown UNIF board (no iNES map)`
rather than guessing.

To extend it: add the `MAPR` board string and its iNES mapper number to the
`UNIF_BOARD_MAP` dict in `coverage.py`, cross-check against `docs/mappers.md`,
and note any suffix-variant split here.
