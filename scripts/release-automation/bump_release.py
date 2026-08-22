#!/usr/bin/env python3
"""Move every release anchor to the next version, WITHOUT lying about it.

# The defect this exists to prevent

A first version of this script replaced `<marker><old_version>` with
`<marker><new_version>` and swapped the codename on that line. It was used to
cut v2.4.4 and it produced, in ten documents:

    **Current release: v2.4.4 "Ignition"** -- what the synthesiser accepts, and
    what the licence requires. A touchstone is a stone you rub gold against...

The version moved, the codename moved, and the DESCRIPTION still belonged to
v2.4.3 "Touchstone". That is worse than a stale version: a stale version is
merely old, and this is confidently wrong. `release_anchor_audit` passed --
correctly, because it checks that the anchor NAMES the workspace version, which
it did.

So the rule here is: an anchor whose prose describes a release is either demoted
properly or REPORTED. It is never left mechanically bumped.

# What it does

* Derives the new version and codename from the CHANGELOG's topmost dated
  section, so the CHANGELOG is the single source of truth and a typo in an
  argument cannot disagree with it.
* Derives the old version from `[workspace.package]`, and the old codename from
  the section below the new one.
* Classifies each anchor by SHAPE and applies the matching transform.
* Fails closed on any anchor it cannot classify, naming it, rather than falling
  back to the mechanical bump that caused the defect above.
* Checks afterwards that the old codename SURVIVES (demoted, not erased) and
  that no doubled bold marker was produced -- a bug that reached two separate
  release cuts.

Anchors come from the ANCHORS table in `release_anchor_audit.rs`. A second copy
of "where the version lives" is precisely the drift that audit exists to catch.
"""

from __future__ import annotations

import argparse
import difflib
import re
import subprocess
import sys
from dataclasses import dataclass
from pathlib import Path

AUDIT = "crates/rustynes-test-harness/tests/release_anchor_audit.rs"
CHANGELOG = "CHANGELOG.md"

# `## [2.4.5] - 2026-08-22 - "Compass" (theme)`
SECTION = re.compile(r'^## \[(?P<v>\d+\.\d+\.\d+)\] - (?P<d>\d{4}-\d{2}-\d{2}) - "(?P<c>[^"]+)"')

# Manifests that carry the version and cannot inherit it.
MANIFESTS = [
    ("Cargo.toml", 'version = "{v}"'),
    ("crates/rustynes-cosim/Cargo.toml", 'version = "{v}"'),
    ("crates/rustynes-libretro/rustynes_libretro.info", 'display_version = "v{v}"'),
]


@dataclass
class Release:
    version: str
    codename: str
    date: str


def run(cmd: list[str], cwd: Path) -> str:
    return subprocess.run(cmd, cwd=cwd, capture_output=True, text=True, check=True).stdout


def anchors(root: Path) -> list[tuple[str, str]]:
    src = (root / AUDIT).read_text()
    i = src.index("const ANCHORS")
    blk = src[i: src.index("\n];", i)]
    out = re.findall(r'path:\s*"((?:[^"\\]|\\.)*)".*?marker:\s*"((?:[^"\\]|\\.)*)"', blk, re.S)
    if not out:
        raise SystemExit(f"no anchors parsed from {AUDIT}; the table moved")
    return [(p, m.encode().decode("unicode_escape")) for p, m in out]


def changelog_releases(root: Path) -> list[Release]:
    rel = []
    for line in (root / CHANGELOG).read_text().splitlines():
        m = SECTION.match(line)
        if m:
            rel.append(Release(m["v"], m["c"], m["d"]))
    if len(rel) < 2:
        raise SystemExit("CHANGELOG needs at least two dated sections to demote between")
    return rel


def workspace_version(root: Path) -> str:
    t = (root / "Cargo.toml").read_text()
    seg = t[t.index("[workspace.package]"):]
    m = re.search(r'^version\s*=\s*"([^"]+)"', seg, re.M)
    if not m:
        raise SystemExit("[workspace.package] has no version")
    return m.group(1).split("-")[0].split("+")[0]


# ---- shapes -------------------------------------------------------------
#
# Only the shapes this repository's anchors actually use. Anything else is
# reported rather than guessed at: a wrong guess here is the exact class of
# defect the module docstring describes.

def classify(line: str, marker: str, old: Release) -> str:
    """BARE | DASH | PAREN | CHAIN | UNKNOWN, judged on the OLD text."""
    at = line.find(marker + old.version)
    if at < 0:
        return "ABSENT"
    rest = line[at + len(marker) + len(old.version):]
    if f'"{old.codename}"' not in rest[:40]:
        # No codename beside the version: nothing describes a release here.
        return "BARE"
    tail = rest.split(f'"{old.codename}"', 1)[1]
    tail = re.sub(r'^\*{0,2}\s*(?:\(\d{4}-\d{2}-\d{2}\))?\s*', '', tail)
    if tail.startswith((", on ", " released — ", " released -- ")):
        return "CHAIN"
    if tail.startswith(("—", "--", "-")):
        return "DASH"
    if tail.startswith("("):
        return "PAREN"
    return "UNKNOWN"


def demote(line: str, marker: str, old: Release, new: Release, lead: str) -> str:
    """Rewrite one anchor line: new release in front, old one demoted behind it."""
    at = line.find(marker + old.version)
    head = line[:at + len(marker)]
    rest = line[at + len(marker):]

    # Split `<ver> "<code>"[**][ (date)]` from the description that follows.
    m = re.match(
        r'(?P<ver>' + re.escape(old.version) + r')'
        r'(?P<mid>\s*"' + re.escape(old.codename) + r'")'
        r'(?P<bold>\*{0,2})'
        r'(?P<date>\s*\(\d{4}-\d{2}-\d{2}\))?',
        rest)
    if not m:
        raise ValueError("shape changed under us")
    desc = rest[m.end():]
    date_new = f" ({new.date})" if m["date"] else ""
    date_old = m["date"] or ""

    shape = classify(line, marker, old)
    if shape == "DASH":
        sep = re.match(r'\s*(—|--|-)\s*', desc)
        body = desc[sep.end():]
        return (f'{head}{new.version} "{new.codename}"{m["bold"]}{date_new} — {lead} '
                f'Built on **v{old.version} "{old.codename}"**{date_old} — {body}')
    if shape == "PAREN":
        # `desc` begins with the SPACE before the paren, so slicing [1:]
        # removes the space and leaves the paren. Caught by the selftest.
        inner = desc.lstrip()[1:]             # drop the opening paren
        return (f'{head}{new.version} "{new.codename}"{m["bold"]}{date_new} ({lead.rstrip(". ")}, '
                f'on **v{old.version} "{old.codename}"**{date_old} — {inner}')
    raise ValueError(f"no demotion rule for shape {shape}")



# ---- selftest -----------------------------------------------------------
#
# The original script shipped untested and produced ten confidently-wrong
# anchors. These pin the classifier and the demotion against the real shapes
# this repository uses, including the two that must FAIL.

def selftest() -> int:
    old = Release("2.4.4", "Ignition", "2026-08-22")
    new = Release("2.4.5", "Compass", "2026-08-22")
    LEAD = "the core reaches memory."
    ok = True

    def check(name, got, want):
        nonlocal ok
        if got != want:
            ok = False
            print(f"  FAIL {name}\n    got  {got!r}\n    want {want!r}")
        else:
            print(f"  ok   {name}")

    cases = [
        ("bare applies-to", "**Applies to:** RustyNES v", "**Applies to:** RustyNES v2.4.4", "BARE"),
        ("bare badge", "badge/version-v", "badge/version-v2.4.4-blue.svg", "BARE"),
        ("dash, dated", "**Current release: v",
         '**Current release: v2.4.4 "Ignition"** (2026-08-22) — the first real RTL.', "DASH"),
        ("dash, undated", "**Current release: v",
         '**Current release: v2.4.4 "Ignition"** — the first real RTL.', "DASH"),
        ("paren", "the current release is **v",
         'the current release is **v2.4.4 "Ignition"** (the first real RTL)', "PAREN"),
        ("chain", "The current release is **v",
         'The current release is **v2.4.4 "Ignition"**, on **v2.4.3 "Touchstone"**', "CHAIN"),
        ("unknown shape", "**Current release: v",
         '**Current release: v2.4.4 "Ignition"** blah blah', "UNKNOWN"),
    ]
    for name, marker, line, want in cases:
        check(f"classify {name}", classify(line, marker, old), want)

    # Demotion must put the NEW lead in front and keep the OLD release named.
    line = '**Current release: v2.4.4 "Ignition"** (2026-08-22) — the first real RTL.'
    got = demote(line, "**Current release: v", old, new, LEAD)
    check("demote dash",
          got,
          '**Current release: v2.4.5 "Compass"** (2026-08-22) — the core reaches memory. '
          'Built on **v2.4.4 "Ignition"** (2026-08-22) — the first real RTL.')

    # The regression that matters: the old description must NOT end up attached
    # to the new codename.
    if "Compass" in got and got.index("Compass") > got.index("first real RTL"):
        ok = False
        print("  FAIL new codename trails the old description")
    else:
        print("  ok   new codename precedes the old description")
    if "Ignition" not in got:
        ok = False
        print("  FAIL demoted release was erased rather than demoted")
    else:
        print("  ok   demoted release survives")

    line_p = 'the current release is **v2.4.4 "Ignition"** (the first real RTL)'
    gp = demote(line_p, "the current release is **v", old, new, LEAD)
    check("demote paren", gp,
          'the current release is **v2.4.5 "Compass"** (the core reaches memory, '
          'on **v2.4.4 "Ignition"** — the first real RTL)')

    # An unclassifiable line must raise, never be bumped mechanically.
    try:
        demote('**Current release: v2.4.4 "Ignition"** blah', "**Current release: v", old, new, LEAD)
        ok = False
        print("  FAIL unknown shape was demoted instead of refused")
    except ValueError:
        print("  ok   unknown shape refused")

    print("\nselftest: " + ("all checks passed" if ok else "FAILURES"))
    return 0 if ok else 1


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__,
                                 formatter_class=argparse.RawDescriptionHelpFormatter)
    ap.add_argument("--selftest", action="store_true",
                    help="run the shape tests and exit")
    ap.add_argument("--lead",
                    help="one sentence describing the NEW release; inserted before the demotion")
    ap.add_argument("--apply", action="store_true",
                    help="write the changes (default is a dry run showing a diff)")
    ap.add_argument("--root", default=".", type=Path)
    args = ap.parse_args()
    if args.selftest:
        return selftest()
    if not args.lead:
        ap.error("--lead is required (or use --selftest)")
    root = args.root.resolve()

    rels = changelog_releases(root)
    new, old = rels[0], rels[1]
    ws = workspace_version(root)
    if ws != old.version:
        raise SystemExit(
            f"[workspace.package] is {ws} but the CHANGELOG's second section is "
            f"{old.version}. Bump the CHANGELOG first, or the manifest is already moved.")
    print(f"{old.version} \"{old.codename}\"  ->  {new.version} \"{new.codename}\"\n")

    edits: dict[Path, str] = {}
    unknown: list[str] = []
    counts = {"BARE": 0, "DASH": 0, "PAREN": 0, "CHAIN": 0}

    for path, marker in anchors(root):
        p = root / path
        text = edits.get(p, p.read_text())
        out = []
        for line in text.split("\n"):
            if marker + old.version in line:
                shape = classify(line, marker, old)
                if shape == "BARE":
                    line = line.replace(marker + old.version, marker + new.version)
                    counts["BARE"] += 1
                elif shape in ("DASH", "PAREN"):
                    line = demote(line, marker, old, new, args.lead)
                    counts[shape] += 1
                elif shape == "CHAIN":
                    # `..., on **v2.4.3 ...` -- the previous release is already
                    # named behind this one, so extend the chain rather than
                    # replacing it.
                    line = line.replace(
                        f'{marker}{old.version} "{old.codename}"',
                        f'{marker}{new.version} "{new.codename}"', 1)
                    line = re.sub(r'(,\s*on\s*)', rf'\1**v{old.version} "{old.codename}"** and ',
                                  line, count=1)
                    line = re.sub(r'( released\s*—\s*[^,]*,\s*on\s*)',
                                  rf'\1v{old.version} "{old.codename}" and ', line, count=1)
                    counts["CHAIN"] += 1
                else:
                    unknown.append(f"{path}: {shape}: {line[:100]}")
            out.append(line)
        edits[p] = "\n".join(out)

    for path, pat in MANIFESTS:
        p = root / path
        text = edits.get(p, p.read_text())
        want = pat.format(v=old.version)
        if text.count(want) != 1:
            unknown.append(f"{path}: expected exactly one {want!r}, found {text.count(want)}")
            continue
        edits[p] = text.replace(want, pat.format(v=new.version), 1)

    if unknown:
        print("REFUSING to write. These anchors were not classified:\n", file=sys.stderr)
        for u in unknown:
            print(f"  {u}", file=sys.stderr)
        print("\nA mechanical bump here would leave the new version beside the OLD "
              "release's description, which is the defect this script exists to "
              "prevent. Handle them explicitly, then re-run.", file=sys.stderr)
        return 1

    problems = []
    for p, text in edits.items():
        rel = p.relative_to(root)
        # The doubled-bold-marker bug, which reached two separate release cuts:
        # `**Current release: ` + `**v2.4.5 ...` yields `**Current release: **v`.
        for bad in re.findall(r'\*\*[^*\n]{0,40}:\s*\*\*v\d', text):
            problems.append(f"{rel}: doubled bold marker: {bad!r}")
        if p.suffix == ".md" and new.codename in text and old.codename not in text:
            problems.append(f"{rel}: {old.codename!r} vanished -- demoted releases must survive, "
                            f"not be overwritten")
    if problems:
        print("REFUSING to write:\n", file=sys.stderr)
        for pr in problems:
            print(f"  {pr}", file=sys.stderr)
        return 1

    print(f"classified: {counts['BARE']} bare, {counts['DASH']} dash, "
          f"{counts['PAREN']} paren, {counts['CHAIN']} chain\n")
    for p, text in sorted(edits.items()):
        before = p.read_text()
        if before == text:
            continue
        rel = p.relative_to(root)
        if args.apply:
            p.write_text(text)
            print(f"  wrote {rel}")
        else:
            d = list(difflib.unified_diff(before.split("\n"), text.split("\n"),
                                          f"a/{rel}", f"b/{rel}", lineterm="", n=0))
            print("\n".join(d[:14]))
    if not args.apply:
        print("\n(dry run -- pass --apply to write)")
        return 0

    try:
        run(["cargo", "metadata", "--manifest-path",
             "crates/rustynes-cosim/Cargo.toml", "--format-version", "1"], root)
        print("  refreshed crates/rustynes-cosim/Cargo.lock")
    except Exception as e:                                    # noqa: BLE001
        print(f"  WARNING: could not refresh the cosim lockfile: {e}", file=sys.stderr)
    print("\nNow: add the VERSION-PLAN row, write "
          f".github/release-notes/v{new.version}.md, and run the audits.")
    return 0


if __name__ == "__main__":
    sys.exit(main())
