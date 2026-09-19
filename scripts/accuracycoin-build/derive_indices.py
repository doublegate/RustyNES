#!/usr/bin/env python3
"""Derive (suite index, test index) for every AccuracyCoin catalog entry.

WHY THIS EXISTS. `build_sub_test_rom.py` needs a suite index and a test index,
and until now those were recorded by hand: `BUILD-PROVENANCE.tsv` carried TWO
rows for a directory of thirty-two ROMs, so thirty of them could not be rebuilt
at all without re-deriving indices nobody had written down.

WHY THE HAND-MAINTAINED MAP COULD NOT BE TRUSTED ANYWAY. The suite map in
`build_sub_test_rom.py`'s docstring lists twenty suites and stops at index 19,
with `PowerOnState` at 14 and `CPUBehavior2` at 19. Upstream's `TableTable` at
46199ae4 has TWENTY-TWO suites, `CPUBehavior2` at **14** and `PPUMisc` at 19 --
the suites were reordered at some point, so a rebuild driven by that docstring
would enter the wrong suite and the ROM would silently test something else.
A ROM that runs the wrong test and writes a plausible byte is the worst possible
failure here, because it looks like a result.

So the indices are derived from the ASM, which is the source, and the derivation
is VALIDATED against the two rows that were recorded by hand before it is used
for the thirty that were not.

Usage:
    derive_indices.py <upstream-src-dir> [--validate suite:test:name ...]
"""
from __future__ import annotations

import argparse
import re
import sys
from pathlib import Path


# Upstream's "this test is not in the all-test-result-table" marker. Five
# entries share it, so it is not an address and must never be used as a key.
DRAW_TEST_LABEL = "result_DrawTest"


def parse(asm: str):
    """Return (suites, results) from the upstream assembly.

    suites:  list of (suite_index, suite_label, [(test_index, test_name, result_label)])
    results: {result_label: address}
    """
    lines = asm.splitlines()

    # --- TableTable gives the suite ORDER, which is the suite index ----------
    order: list[str] = []
    in_tt = False
    for ln in lines:
        s = ln.strip()
        if s.startswith("TableTable:"):
            in_tt = True
            continue
        if in_tt:
            if s.startswith("EndTableTable:"):
                break
            m = re.match(r"\.(?:word|dw)\s+(Suite_[A-Za-z0-9_]+)", s)
            if m:
                order.append(m.group(1))
    if not order:
        sys.exit("derive_indices: TableTable parsed to nothing -- refusing to guess")

    # --- each suite's own `table` lines give the test order -----------------
    bodies: dict[str, list[tuple[str, str]]] = {}
    cur: str | None = None
    for ln in lines:
        m = re.match(r"^(Suite_[A-Za-z0-9_]+):", ln)
        if m:
            cur = m.group(1)
            bodies[cur] = []
            continue
        if cur is None:
            continue
        # A label at column 0 that is not a suite ends the block.
        if re.match(r"^[A-Za-z_][A-Za-z0-9_]*:", ln) and not ln.startswith("Suite_"):
            cur = None
            continue
        m = re.match(r'\s*table\s+"([^"]+)"\s*,\s*\$FF\s*,\s*(result_[A-Za-z0-9_]+)', ln)
        if m and cur:
            bodies[cur].append((m.group(1), m.group(2)))

    # --- result label -> address -------------------------------------------
    results: dict[str, int] = {}
    for ln in lines:
        m = re.match(r"^(result_[A-Za-z0-9_]+)\s*=\s*\$([0-9A-Fa-f]+)", ln)
        if m:
            results[m.group(1)] = int(m.group(2), 16)

    suites = []
    for idx, label in enumerate(order):
        tests = [(i, n, r) for i, (n, r) in enumerate(bodies.get(label, []))]
        suites.append((idx, label, tests))
    return suites, results


def main() -> int:
    ap = argparse.ArgumentParser()
    ap.add_argument("src", type=Path, help="upstream AccuracyCoin source directory")
    ap.add_argument(
        "--validate",
        action="append",
        default=[],
        metavar="SUITE:TEST:NAME",
        help="assert a known (suite, test) -> name mapping before trusting the rest",
    )
    args = ap.parse_args()

    asm_path = args.src / "AccuracyCoin.asm"
    if not asm_path.is_file():
        sys.exit(f"derive_indices: missing {asm_path}")
    suites, results = parse(asm_path.read_text(encoding="utf-8", errors="replace"))

    # A FLAT LIST, NOT A MAP KEYED BY ADDRESS. Keying by address silently drops
    # every entry after the first at a shared one, and this corpus has such a
    # set: all five `Suite_PowerOnState` tests name `result_DrawTest`, whose
    # definition carries upstream's own explanation --
    #
    #     result_DrawTest = $03FF  ; page 3 omits the test from the
    #                                all-test-result-table.
    #
    # So $3FF is a SENTINEL meaning "this test has no result byte", not a
    # location. The map form emitted 145 rows for a 149-entry catalog and said
    # nothing; the four missing ones were `CPU RAM`, `CPU Registers`,
    # `PPU RAM` and `Palette RAM`, which is precisely the kind of silent
    # shortfall this script exists to replace. Raised in review as a
    # hypothetical ("if multiple tests happen to share..."); measured here, it
    # was already happening.
    rows: list[tuple[int, int, int, str, str, bool]] = []
    total = 0
    for sidx, slabel, tests in suites:
        for tidx, name, rlabel in tests:
            total += 1
            addr = results.get(rlabel)
            if addr is None:
                print(f"  warning: {rlabel} has no address definition", file=sys.stderr)
                continue
            rows.append((addr, sidx, tidx, slabel, name, rlabel == DRAW_TEST_LABEL))

    # The whole point of the list is that nothing is dropped, so say so rather
    # than trusting it. A future sentinel, or a parse that loses a table, fails
    # here instead of quietly emitting a short catalog.
    if len(rows) != total:
        sys.exit(
            f"derive_indices: {total} catalog entries but {len(rows)} rows -- "
            f"{total - len(rows)} lost. A short catalog must not be emitted."
        )

    # Validation BEFORE use. A derivation that cannot reproduce a
    # hand-recorded answer is not permitted to supply the ones nobody recorded.
    for spec in args.validate:
        try:
            want_s, want_t, want_name = spec.split(":", 2)
            int(want_s), int(want_t)
        except ValueError:
            sys.exit(
                f"derive_indices: --validate {spec!r} is not SUITE:TEST:NAME "
                f"(e.g. --validate 21:3:'Misaligned OAM2 Address')"
            )
        hit = [
            (s, t, sl, n)
            for (_a, s, t, sl, n, _d) in rows
            if s == int(want_s) and t == int(want_t)
        ]
        if not hit:
            sys.exit(f"derive_indices: VALIDATION FAILED -- no entry at suite {want_s} test {want_t}")
        got = hit[0][3]
        if got != want_name:
            sys.exit(
                f"derive_indices: VALIDATION FAILED -- suite {want_s} test {want_t} "
                f"is {got!r}, expected {want_name!r}. The derivation disagrees with a "
                f"hand-recorded answer, so it must not be used for the rest."
            )
        print(f"validated: suite {want_s} test {want_t} = {got}", file=sys.stderr)

    drawtest = sum(1 for r in rows if r[5])
    print(
        f"# {len(suites)} suites, {total} catalog entries, {len(rows)} rows "
        f"({drawtest} with no result byte)",
        file=sys.stderr,
    )
    # Sorted by address, then by (suite, test) so the sentinel group has a
    # stable order rather than whatever the dict happened to hold last.
    print("addr\tsuite\ttest\thas_result\tsuite_label\ttest_name")
    for addr, s, t, sl, n, is_draw in sorted(rows, key=lambda r: (r[0], r[1], r[2])):
        # `has_result` is the column a consumer must read before treating
        # `addr` as somewhere to look for a verdict. For the sentinel group
        # there is no byte at $3FF to read, and a caller that assumed there was
        # would find whatever the last test wrote there.
        print(f"0x{addr:03X}\t{s}\t{t}\t{'yes' if not is_draw else 'no'}\t{sl}\t{n}")
    return 0


if __name__ == "__main__":
    raise SystemExit(main())
