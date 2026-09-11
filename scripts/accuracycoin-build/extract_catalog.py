#!/usr/bin/env python3
"""Extract `SOURCE_CATALOG.tsv` from upstream `AccuracyCoin.asm`.

The catalog maps `(suite, test-name) -> result-byte RAM address`, which is
what `rustynes_test_harness::accuracy_coin_catalog` decodes a battery run
with. Until v2.6.17 the extraction recipe lived only as prose in
`tests/roms/AccuracyCoin/README.md`; prose cannot be audited and cannot be
re-run, so an upstream re-sync meant re-deriving the rules by hand. This is
that recipe as a program.

Authoritative source is `AccuracyCoin.asm`, never a doc:

  * `TableTable:` .. `EndTableTable:` lists `.word Suite_X` in **display
    order**. That ordering is the catalog's ordering — the ROM's own result
    page walks it, so a catalog in file order would disagree with the ROM
    whenever the two diverge.
  * Each `Suite_X:` block opens with `.byte "Display Name", $FF` and then
    carries `table "test name", $FF, result_symbol, TEST_entrypoint` rows
    until a bare `.byte $FF` terminator.
  * `result_symbol` resolves through a top-level `result_X = $ADDR`
    definition.

Usage:
    python3 scripts/accuracycoin-build/extract_catalog.py <path-to-AccuracyCoin.asm> [--out FILE]
    python3 scripts/accuracycoin-build/extract_catalog.py --self-test
"""

from __future__ import annotations

import argparse
import re
import sys

# `result_Foo = $0491` / `result_Foo = $12`. Anchored at column 0 because the
# in-suite `table` rows also mention the symbol and must not match here.
RE_RESULT_DEF = re.compile(r"^(result_[A-Za-z0-9_]+)\s*=\s*\$([0-9A-Fa-f]+)", re.M)
RE_TABLETABLE = re.compile(r"^TableTable:(.*?)^EndTableTable:", re.S | re.M)
RE_SUITE_WORD = re.compile(r"^\s*\.word\s+(Suite_[A-Za-z0-9_]+)\s*$", re.M)
RE_TABLE_ROW = re.compile(
    r'^\s*table\s+"([^"]*)"\s*,\s*\$FF\s*,\s*(result_[A-Za-z0-9_]+)\s*,', re.M
)
RE_SUITE_HEADER = re.compile(r'^\s*\.byte\s+"([^"]*)"\s*,\s*\$FF', re.M)


def parse(asm: str) -> list[tuple[str, str, int]]:
    """Return `(suite, name, result_addr)` triples in TableTable order."""
    results = {m.group(1): int(m.group(2), 16) for m in RE_RESULT_DEF.finditer(asm)}
    if not results:
        raise SystemExit("no `result_X = $ADDR` definitions found — wrong file?")

    tt = RE_TABLETABLE.search(asm)
    if not tt:
        raise SystemExit("no TableTable:/EndTableTable: block found")
    order = RE_SUITE_WORD.findall(tt.group(1))
    if not order:
        raise SystemExit("TableTable block contained no `.word Suite_*` rows")

    rows: list[tuple[str, str, int]] = []
    for label in order:
        start = asm.find(f"\n{label}:")
        if start < 0:
            raise SystemExit(f"TableTable names {label} but no `{label}:` block exists")
        # The block ends at the next top-level label, so slice to the next
        # `\nSuite_` or, for the final suite, to the end of the source.
        nxt = asm.find("\nSuite_", start + 1)
        block = asm[start : nxt if nxt > 0 else len(asm)]

        header = RE_SUITE_HEADER.search(block)
        if not header:
            raise SystemExit(f"{label} has no `.byte \"name\", $FF` display header")
        suite = header.group(1)

        for name, symbol in RE_TABLE_ROW.findall(block):
            if symbol not in results:
                raise SystemExit(
                    f'{label} / "{name}" references {symbol}, which has no '
                    f"`{symbol} = $ADDR` definition"
                )
            rows.append((suite, name, results[symbol]))

    if not rows:
        raise SystemExit("parsed zero test rows — the `table` macro shape changed?")
    return rows


def render(rows: list[tuple[str, str, int]]) -> str:
    return "".join(f"{s}\t{n}\t0x{a:04X}\n" for s, n, a in rows)


SELF_TEST_ASM = """
result_Alpha = $0401
result_Beta  = $0402
result_Gamma = $0403

TableTable:
\t.word Suite_Second
\t.word Suite_First
EndTableTable:

Suite_First:
\t.byte "First Page", $FF
\ttable "Alpha Test",  $FF, result_Alpha, TEST_Alpha
\ttable "Beta Test",   $FF, result_Beta,  TEST_Beta
\t.byte $FF

Suite_Second:
\t.byte "Second Page", $FF
\ttable "Gamma Test",  $FF, result_Gamma, TEST_Gamma
\t.byte $FF
"""


def self_test() -> int:
    rows = parse(SELF_TEST_ASM)
    # TableTable order wins over file order: Second precedes First.
    assert rows == [
        ("Second Page", "Gamma Test", 0x0403),
        ("First Page", "Alpha Test", 0x0401),
        ("First Page", "Beta Test", 0x0402),
    ], rows
    assert render(rows[:1]) == "Second Page\tGamma Test\t0x0403\n"

    # An unresolvable result symbol must abort, never silently drop the row.
    broken = SELF_TEST_ASM.replace("result_Gamma, TEST_Gamma", "result_Missing, TEST_Gamma")
    try:
        parse(broken)
    except SystemExit as exc:
        assert "result_Missing" in str(exc), exc
    else:
        raise AssertionError("an undefined result symbol was accepted")

    # A suite named by TableTable but absent from the source must abort.
    broken = SELF_TEST_ASM.replace("Suite_First:", "Suite_Elsewhere:")
    try:
        parse(broken)
    except SystemExit as exc:
        assert "Suite_First" in str(exc), exc
    else:
        raise AssertionError("a missing suite block was accepted")

    # A `table` row inside a suite must not be mistaken for a result definition.
    assert "result_Alpha" in {m.group(1) for m in RE_RESULT_DEF.finditer(SELF_TEST_ASM)}
    assert len(RE_RESULT_DEF.findall(SELF_TEST_ASM)) == 3

    print("extract_catalog self-test: ok (4 cases)")
    return 0


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument("asm", nargs="?", help="path to upstream AccuracyCoin.asm")
    ap.add_argument("--out", help="write TSV here instead of stdout")
    ap.add_argument("--self-test", action="store_true", help="run the parser self-test")
    args = ap.parse_args()

    if args.self_test:
        return self_test()
    if not args.asm:
        ap.error("an AccuracyCoin.asm path is required (or --self-test)")

    with open(args.asm, encoding="utf-8", errors="replace") as fh:
        rows = parse(fh.read())

    tsv = render(rows)
    if args.out:
        with open(args.out, "w", encoding="utf-8") as fh:
            fh.write(tsv)
        suites = len({s for s, _, _ in rows})
        print(f"wrote {args.out}: {len(rows)} rows across {suites} suites")
    else:
        sys.stdout.write(tsv)
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
