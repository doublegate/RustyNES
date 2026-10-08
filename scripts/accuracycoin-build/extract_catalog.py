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
  * Since upstream `f5f41dc2` (2026-10) the unofficial-opcode suites use two
    byte-saving variants. `tblf1 "prefix", str_X, $FF, result, TEST` stores a
    one-byte token for a common word, and `tblf2 "prefix", str_X, ",X", $FF,
    result, TEST` appends a suffix after it. The ROM prints the token from
    `PrintTextSpecialStrings` (`" indirect"`, `" zeropage"`, ...), indexed by
    the token's low two bits, so the name is rebuilt from that table, not
    from a word list kept here.
  * Any other row-shaped line inside a suite aborts the run. A new macro
    must fail loudly: this script once returned 85 of 151 rows without
    complaint, because it only checked that the total was non-zero.
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
# `tblf1 "$07   SLO", str_ZeroPage, $FF, result_X, TEST_X`
RE_TBLF1_ROW = re.compile(
    r'^\s*tblf1\s+"([^"]*)"\s*,\s*(str_[A-Za-z0-9_]+)\s*,\s*\$FF\s*,'
    r"\s*(result_[A-Za-z0-9_]+)\s*,",
    re.M,
)
# `tblf2 "$03   SLO", str_Indirect, ",X", $FF, result_X, TEST_X`
RE_TBLF2_ROW = re.compile(
    r'^\s*tblf2\s+"([^"]*)"\s*,\s*(str_[A-Za-z0-9_]+)\s*,\s*"([^"]*)"\s*,'
    r"\s*\$FF\s*,\s*(result_[A-Za-z0-9_]+)\s*,",
    re.M,
)
# Any line opening with a row macro (`table`, `tblf1`, `tblfN`, ...), whatever
# its first argument is. Used only to prove every such line was read by one of
# the patterns above. Keyed on the macro FAMILY rather than on a quoted first
# argument (PR #594 review): a future `tblf3 str_Name, $FF, result_X, ...` row
# would have slipped past a quote-anchored check and silently shifted every
# later index. `table`/`tbl` because the 6502 mnemonics that start with `t`
# (`tax`, `tay`, `tsx`, `txa`, `txs`, `tya`) must not match.
RE_ANY_ROW = re.compile(r"^\s*(table|tbl[a-z0-9]*)\s+\S", re.M)
RE_TOKEN_DEF = re.compile(r"^(str_[A-Za-z0-9_]+)\s*=\s*\$([0-9A-Fa-f]+)", re.M)
RE_SPECIAL_STRINGS = re.compile(
    r'^PrintTextSpecialStrings:\s*\n((?:\s*\.byte\s+"[^"]*"\s*\n?)+)', re.M
)


def special_words(asm: str) -> dict[str, str]:
    """Map each `str_X` token to the word the ROM prints for it.

    The ROM's `PrintTextSpecialString` takes the token's low two bits as an
    index into `PrintTextSpecialStrings`. Each entry carries a leading space
    that separates it from the prefix; `parse` re-adds it as one space.
    """
    tokens = {m.group(1): int(m.group(2), 16) for m in RE_TOKEN_DEF.finditer(asm)}
    if not tokens:
        return {}
    block = RE_SPECIAL_STRINGS.search(asm)
    if not block:
        raise SystemExit("`str_X` tokens are defined but no PrintTextSpecialStrings table exists")
    words = re.findall(r'"([^"]*)"', block.group(1))
    out = {}
    for name, value in tokens.items():
        idx = value & 0x3
        if idx >= len(words):
            raise SystemExit(f"{name} = ${value:02X} indexes past PrintTextSpecialStrings")
        out[name] = words[idx].strip()
    return out


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

    words = special_words(asm)

    def word(token: str) -> str:
        if token not in words:
            raise SystemExit(f"row uses {token}, which has no `{token} = $NN` definition")
        return words[token]

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

        found: list[tuple[int, str, str]] = []
        for m in RE_TABLE_ROW.finditer(block):
            found.append((m.start(), m.group(1), m.group(2)))
        for m in RE_TBLF1_ROW.finditer(block):
            found.append((m.start(), f"{m.group(1)} {word(m.group(2))}", m.group(3)))
        for m in RE_TBLF2_ROW.finditer(block):
            found.append(
                (m.start(), f"{m.group(1)} {word(m.group(2))}{m.group(3)}", m.group(4))
            )
        found.sort()
        # Every row-shaped line must have been read by exactly one pattern.
        candidates = [m for m in RE_ANY_ROW.finditer(block)]
        if len(candidates) != len(found):
            parsed = {pos for pos, _, _ in found}
            missed = [
                block[m.start() : block.find("\n", m.start())].strip()
                for m in candidates
                if m.start() not in parsed
            ]
            raise SystemExit(f"{label} has row lines this script cannot read: {missed}")
        if not found:
            raise SystemExit(f"{label} yielded zero rows")

        for _, name, symbol in found:
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

str_Indirect = $F0
str_ZeroPage = $F1

PrintTextSpecialStrings:
\t.byte " indirect"
\t.byte " zeropage"
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

    # The compressed row macros (upstream 03757ce8 / f5f41dc2, 2026-10-07):
    # `tblf1` and `tblf2` substitute a one-byte token for a common word, and
    # `tblf2` appends a suffix. The name is rebuilt exactly as the ROM shows it.
    compressed = SELF_TEST_ASM.replace(
        '\ttable "Gamma Test",  $FF, result_Gamma, TEST_Gamma',
        '\ttblf2 "$03   SLO", str_Indirect, ",X", $FF, result_Gamma, TEST_Gamma\n'
        '\ttblf1 "$07   SLO", str_ZeroPage,       $FF, result_Alpha, TEST_Alpha',
    )
    rows = parse(compressed)
    assert rows[:2] == [
        ("Second Page", "$03   SLO indirect,X", 0x0403),
        ("Second Page", "$07   SLO zeropage", 0x0401),
    ], rows

    # FAIL CLOSED on a row shape the parser does not know: a macro it cannot
    # read must abort, never vanish. Until v3.1.0 this script dropped all 66
    # compressed rows silently, because only "zero rows in total" was checked.
    unknown = SELF_TEST_ASM.replace("\ttable \"Gamma Test\"", "\ttblf9 \"Gamma Test\"")
    try:
        parse(unknown)
    except SystemExit as exc:
        assert "tblf9" in str(exc), exc
    else:
        raise AssertionError("an unknown row macro was dropped silently")

    # ...including one whose FIRST argument is a token rather than a string,
    # which the detector used to require (PR #594 review).
    tokenfirst = SELF_TEST_ASM.replace(
        '\ttable "Gamma Test",  $FF, result_Gamma, TEST_Gamma',
        '\ttable "Gamma Test",  $FF, result_Gamma, TEST_Gamma\n'
        "\ttblf3 str_ZeroPage, $FF, result_Alpha, TEST_Alpha",
    )
    try:
        parse(tokenfirst)
    except SystemExit as exc:
        assert "tblf3" in str(exc), exc
    else:
        raise AssertionError("a token-first row macro was dropped silently")

    # ...and a suite that yields no rows at all must abort.
    empty = SELF_TEST_ASM.replace('\ttable "Gamma Test",  $FF, result_Gamma, TEST_Gamma\n', "")
    try:
        parse(empty)
    except SystemExit as exc:
        assert "Suite_Second" in str(exc), exc
    else:
        raise AssertionError("a suite with zero rows was accepted")

    # An unknown word token must abort rather than guess a word.
    badtok = compressed.replace(", str_ZeroPage,", ", str_Mystery,")
    try:
        parse(badtok)
    except SystemExit as exc:
        assert "str_Mystery" in str(exc), exc
    else:
        raise AssertionError("an unknown word token was accepted")

    print("extract_catalog self-test: ok (9 cases)")
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
