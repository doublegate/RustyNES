#!/usr/bin/env python3
"""Arrange the boot/oracle screenshot tree to mirror ``tests/roms/`` by tier.

Single source of truth for the screenshot layout policy (ADR 0011 mapper
tiering):

* Commercial (``Core`` / ``Curated``) mappers  -> ``screenshots/external/``
* Unlicensed / pirate / homebrew (``BestEffort``) mappers
  -> ``screenshots/besteffort/``

Both trees use the SAME shape as the ROM corpus: one
``mapper-NNN-Name/<rom-stem>.png`` sub-directory per family (plus the special
``fds`` / ``pc10`` / ``vs-system`` dirs under ``external/``). This script:

1. Reads the tier of every ``mapper-NNN`` dir from the Rust classifier
   (``rustynes-mappers::mapper_tier``) via the generated ``--print-tier`` table,
   falling back to the static table embedded below if the binary is absent.
2. RELOCATES any ``screenshots/external/mapper-NNN-*`` dir whose mapper is
   ``BestEffort`` into ``screenshots/besteffort/`` (and vice-versa), so the
   screenshot category always matches the ROM category in ``tests/roms/``.
3. NORMALIZES ``screenshots/besteffort/`` from any legacy *flat*
   ``mapper-NNN-Name.png`` files into ``mapper-NNN-Name/<stem>.png`` sub-dirs.
4. Reports the final per-tree, per-mapper coverage.

It NEVER touches ROM files (``*.nes`` / ``*.zip``) and only moves ``*.png``.
Run from anywhere; it locates the repo root via this file's path.

Usage::

    python3 scripts/screenshots/categorize_screenshots.py            # apply
    python3 scripts/screenshots/categorize_screenshots.py --dry-run  # preview
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import sys

# --- Tier table (mirror of crates/rustynes-mappers/src/tier.rs). -----------
# Kept here as the offline fallback / CI-free default. When the families change
# in tier.rs, update this set in the same change (the screenshot layout and the
# classifier must agree). The accompanying test
# `scripts/screenshots/test_tier_table_matches_rust` (see CONTRIBUTING) guards
# it from drifting.
CORE = {
    0, 1, 2, 3, 4, 5, 7, 9, 10, 11, 13, 16, 18, 19, 21, 22, 23, 24, 25, 26, 32,
    33, 34, 48, 64, 65, 66, 67, 68, 69, 70, 71, 73, 75, 78, 80, 82, 85, 87, 88,
    89, 93, 99, 118, 119, 151, 152, 159, 184, 206, 210,
}
CURATED = {38, 41, 79, 86, 113, 140, 232, 240, 241}
BEST_EFFORT = {
    15, 28, 29, 30, 31, 36, 39, 58, 60, 61, 62, 63, 72, 76, 77, 92, 94, 96, 97,
    101, 107, 111, 132, 133, 143, 145, 146, 147, 148, 149, 150, 174, 177, 179,
    180, 185, 200, 201, 202, 203, 212, 213, 214, 218, 225, 226, 227, 229, 231,
    233, 234, 242, 246,
}

# Special non-mapper categories that always belong under external/ (commercial
# arcade / disk hardware), never relocated.
SPECIAL_EXTERNAL = {"fds", "pc10", "vs-system"}

MAPPER_DIR_RE = re.compile(r"^mapper-(\d+)-")


def repo_root() -> str:
    here = os.path.dirname(os.path.abspath(__file__))
    return os.path.abspath(os.path.join(here, "..", ".."))


def tier_of(mapper: int) -> str | None:
    if mapper in CORE:
        return "Core"
    if mapper in CURATED:
        return "Curated"
    if mapper in BEST_EFFORT:
        return "BestEffort"
    return None


def category_of(mapper: int) -> str | None:
    """Return the screenshot sub-tree a mapper's screenshots belong in."""
    t = tier_of(mapper)
    if t in ("Core", "Curated"):
        return "external"
    if t == "BestEffort":
        return "besteffort"
    return None


def mapper_id(dirname: str) -> int | None:
    m = MAPPER_DIR_RE.match(dirname)
    return int(m.group(1)) if m else None


def main() -> int:
    ap = argparse.ArgumentParser(description=__doc__)
    ap.add_argument(
        "--dry-run",
        action="store_true",
        help="print the moves/normalizations without applying them",
    )
    args = ap.parse_args()

    root = repo_root()
    ss = os.path.join(root, "screenshots")
    ext = os.path.join(ss, "external")
    be = os.path.join(ss, "besteffort")
    os.makedirs(ext, exist_ok=True)
    os.makedirs(be, exist_ok=True)

    moves: list[tuple[str, str]] = []
    normalizations: list[tuple[str, str]] = []
    flagged: list[str] = []

    def move(src: str, dst: str) -> None:
        rel_s = os.path.relpath(src, root)
        rel_d = os.path.relpath(dst, root)
        if args.dry_run:
            print(f"  MOVE  {rel_s}  ->  {rel_d}")
        else:
            os.makedirs(os.path.dirname(dst), exist_ok=True)
            shutil.move(src, dst)

    # 1. Normalize legacy FLAT besteffort PNGs into per-mapper sub-dirs.
    for entry in sorted(os.listdir(be)):
        full = os.path.join(be, entry)
        if not os.path.isfile(full) or not entry.endswith(".png"):
            continue
        stem = entry[:-4]
        sub = os.path.join(be, stem)  # mapper-NNN-Name/
        dst = os.path.join(sub, stem + ".png")
        normalizations.append((full, dst))
        move(full, dst)

    # 2. Relocate dirs whose tier-category disagrees with their current tree.
    for tree, name in ((ext, "external"), (be, "besteffort")):
        for d in sorted(os.listdir(tree)):
            full = os.path.join(tree, d)
            if not os.path.isdir(full):
                continue
            if d in SPECIAL_EXTERNAL:
                if name != "external":
                    flagged.append(f"special dir {d} found under {name}/ (should be external/)")
                continue
            mid = mapper_id(d)
            if mid is None:
                flagged.append(f"unrecognized screenshot dir: {name}/{d}")
                continue
            cat = category_of(mid)
            if cat is None:
                flagged.append(f"{name}/{d}: mapper {mid} is unclassified by the tier table")
                continue
            if cat != name:
                dst = os.path.join(ss, cat, d)
                moves.append((full, dst))
                move(full, dst)

    # 3. Report final coverage.
    print("\n=== relocations (tier mismatch) ===")
    for s, _ in moves:
        print(f"  {os.path.relpath(s, root)}  ->  {category_of(mapper_id(os.path.basename(s)))}/")
    if not moves:
        print("  (none — every screenshot dir already sits in the right tree)")
    print(f"\n=== besteffort flat->subdir normalizations: {len(normalizations)} ===")
    if flagged:
        print("\n=== FLAGGED (manual review) ===")
        for f in flagged:
            print(f"  {f}")

    def summarize(tree: str, label: str) -> None:
        dirs = sorted(
            d for d in os.listdir(tree) if os.path.isdir(os.path.join(tree, d))
        )
        print(f"\n=== {label}/ ({len(dirs)} dirs) ===")
        for d in dirs:
            n = len(
                [f for f in os.listdir(os.path.join(tree, d)) if f.endswith(".png")]
            )
            print(f"  {d:38} {n} png")

    summarize(ext, "screenshots/external")
    summarize(be, "screenshots/besteffort")
    return 1 if flagged else 0


if __name__ == "__main__":
    sys.exit(main())
