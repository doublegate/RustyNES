#!/usr/bin/env python3
"""
restore_nesdev_wiki_image_links.py — rewrite broken image links in a
gitignored `nesdev_wiki/` local mirror so they point at the sibling
image files instead of the MediaWiki absolute paths.

The crawl that produced `nesdev_wiki/` deposited 2,769 `.xhtml` pages
alongside ~580 image files at the top level, but left the in-page
`<img src="...">` and `<a href="...">` attributes pointing at three
broken URL shapes:

  1. ``src="../wiki-images/<Name>"``                                 (relative — sibling dir does not exist)
  2. ``href="../wiki-images/<Name>"``                                (same; image-page anchor)
  3. ``src="/w/images/default/<X>/<XX>/<Name>"``                     (absolute MediaWiki path, full-size)
  4. ``src="/w/images/default/thumb/<X>/<XX>/<Name>/<NNNpx>-<Name>"`` (absolute MediaWiki thumb URL)

Local files live at the top of `nesdev_wiki/` with filenames that
match the basename in each URL (the trailing component for cases 1-3;
the second-to-last component for case 4, since the final component is
a `NNNpx-` prefixed thumbnail copy that is NOT on disk). The mapping
is therefore: each broken URL → URL-decoded basename → sibling file.

Usage::

    python3 scripts/restore_nesdev_wiki_image_links.py [--target DIR]
                                                         [--dry-run]
                                                         [--no-backup]
                                                         [--verbose]
                                                         [--report PATH]

The default target is `nesdev_wiki/` next to this script (resolved
from the script's grandparent — i.e., the repo root). The default
mode writes in-place with a `.bak` backup file per modified .xhtml;
pass `--no-backup` to skip backups (irreversible). Use `--dry-run`
to preview without writing.

References that resolve to a basename which is NOT present locally
are left alone and counted under "misses" (per-page detail in the
report). This preserves diagnostic value: a future image-download
pass can be driven from the misses report.
"""

from __future__ import annotations

import argparse
import os
import re
import shutil
import sys
import tempfile
import urllib.parse
from collections import Counter
from dataclasses import dataclass, field
from pathlib import Path

# ---------------------------------------------------------------------------
# Pattern table — order matters. The thumb pattern is tried first so the
# inner full-size pattern does not steal a thumb URL prematurely.
#
# Each pattern captures the URL-encoded basename of the image whose local
# sibling file we want to point at. For thumb URLs that is the second-to-
# last path component; for everything else it is the trailing component.
# ---------------------------------------------------------------------------

PATTERNS: list[tuple[str, re.Pattern[str]]] = [
    # 1) /w/images/default/thumb/<X>/<XX>/<Name>/<NNNpx>-<Name> in `src=`
    #    The captured group is <Name> (the basename of the full-size image,
    #    not the thumb file itself which is absent on disk).
    (
        "thumb_src",
        re.compile(
            r'\bsrc="/w/images/default/thumb/[^/"]+/[^/"]+/([^/"]+)/[^"]+"'
        ),
    ),
    # 2) /w/images/default/<X>/<XX>/<Name> in `src=` (non-thumb, full-size)
    (
        "full_src",
        re.compile(r'\bsrc="/w/images/default/[^/"]+/[^/"]+/([^/"]+)"'),
    ),
    # 3) ../wiki-images/<Name> in `src=`
    (
        "wiki_src",
        re.compile(r'\bsrc="\.\./wiki-images/([^"]+)"'),
    ),
    # 4) ../wiki-images/<Name> in `href=` (image-page anchor wrap)
    (
        "wiki_href",
        re.compile(r'\bhref="\.\./wiki-images/([^"]+)"'),
    ),
]


@dataclass
class Stats:
    files_scanned: int = 0
    files_modified: int = 0
    rewrites_by_pattern: Counter = field(default_factory=Counter)
    misses_by_pattern: Counter = field(default_factory=Counter)
    missing_basenames: Counter = field(default_factory=Counter)
    write_errors: int = 0


def decode_basename(raw: str) -> str:
    """URL-decode a basename captured from one of the broken URL shapes.

    ``urllib.parse.unquote`` handles the common encoded chars
    (``%2C`` → ``,``, ``%20`` → space, etc.) which the wiki crawl
    propagated into URLs but NOT into local filenames.
    """
    return urllib.parse.unquote(raw)


def rewrite_text(text: str, target_dir: Path, stats: Stats, source_path: Path) -> str:
    """Apply all four pattern rewrites to `text`. Leaves unmatched
    references alone; tallies misses (no local file exists) into stats.

    Returns the rewritten text (possibly unchanged).
    """

    def make_sub(pattern_name: str):
        def sub(match: re.Match[str]) -> str:
            raw_basename = match.group(1)
            basename = decode_basename(raw_basename)
            local_path = target_dir / basename
            attr_name = "src" if match.group(0).startswith('src=') else "href"
            if local_path.is_file():
                stats.rewrites_by_pattern[pattern_name] += 1
                return f'{attr_name}="{basename}"'
            stats.misses_by_pattern[pattern_name] += 1
            stats.missing_basenames[basename] += 1
            return match.group(0)

        return sub

    new_text = text
    for name, pat in PATTERNS:
        new_text = pat.sub(make_sub(name), new_text)
    return new_text


def write_atomic(path: Path, text: str) -> None:
    """Write `text` to `path` atomically via a sibling temp file +
    `os.replace` (POSIX rename is atomic within a single filesystem).
    """
    fd, tmp = tempfile.mkstemp(
        dir=str(path.parent), prefix=f".{path.name}.", suffix=".tmp"
    )
    try:
        with os.fdopen(fd, "w", encoding="utf-8") as out:
            out.write(text)
        os.replace(tmp, path)
    except Exception:
        # Clean up the temp file on failure; surface the error.
        try:
            os.unlink(tmp)
        except OSError:
            pass
        raise


def process_one(
    path: Path, target_dir: Path, stats: Stats, *, dry_run: bool, backup: bool, verbose: bool
) -> bool:
    """Rewrite `path` in place (or simulate, if dry-run). Returns True
    iff at least one rewrite was applied.
    """
    stats.files_scanned += 1
    try:
        original = path.read_text(encoding="utf-8")
    except (UnicodeDecodeError, OSError) as exc:
        print(f"WARN read failed {path}: {exc}", file=sys.stderr)
        stats.write_errors += 1
        return False

    rewritten = rewrite_text(original, target_dir, stats, path)
    if rewritten == original:
        return False

    stats.files_modified += 1
    if verbose:
        delta = sum(1 for _ in re.finditer(r'(src|href)="', original)) - sum(
            1 for _ in re.finditer(r'(src|href)="', rewritten)
        )
        # delta is not very meaningful (attribute count is preserved);
        # show byte delta instead.
        print(
            f"REWRITE {path.name}  (-{len(original) - len(rewritten)} bytes)"
        )

    if dry_run:
        return True

    if backup:
        bak = path.with_suffix(path.suffix + ".bak")
        if not bak.exists():
            shutil.copy2(path, bak)
    try:
        write_atomic(path, rewritten)
    except OSError as exc:
        print(f"WARN write failed {path}: {exc}", file=sys.stderr)
        stats.write_errors += 1
        return False
    return True


def discover_target(explicit: str | None) -> Path:
    """Resolve the `nesdev_wiki/` directory we operate on.

    With `--target`, use the supplied path. Otherwise infer from the
    script's location: walk up to the repo root (parent of `scripts/`)
    and look for `nesdev_wiki/` there.
    """
    if explicit:
        return Path(explicit).resolve()
    script = Path(__file__).resolve()
    repo_root = script.parent.parent  # scripts/ -> repo root
    candidate = repo_root / "nesdev_wiki"
    if candidate.is_dir():
        return candidate
    print(
        f"error: cannot locate nesdev_wiki/ relative to {script} — pass --target",
        file=sys.stderr,
    )
    sys.exit(2)


def write_report(report_path: Path, stats: Stats, target: Path) -> None:
    """Emit a per-run summary to `report_path`."""
    lines: list[str] = []
    lines.append(f"# nesdev_wiki image-link restoration report")
    lines.append(f"")
    lines.append(f"Target directory: `{target}`")
    lines.append(f"Files scanned: {stats.files_scanned}")
    lines.append(f"Files modified: {stats.files_modified}")
    lines.append(f"Write errors: {stats.write_errors}")
    lines.append(f"")
    lines.append(f"## Rewrites by pattern")
    lines.append(f"")
    for name, _ in PATTERNS:
        lines.append(
            f"- `{name}`: rewrote {stats.rewrites_by_pattern[name]}, "
            f"missed {stats.misses_by_pattern[name]}"
        )
    lines.append(f"")
    lines.append(f"## Missing image files ({len(stats.missing_basenames)} unique)")
    lines.append(f"")
    lines.append("These references resolved to a local basename that does not")
    lines.append("exist on disk. They were left untouched. Counts indicate how")
    lines.append("many .xhtml pages referenced each missing file.")
    lines.append(f"")
    for basename, count in sorted(
        stats.missing_basenames.items(), key=lambda kv: (-kv[1], kv[0])
    ):
        lines.append(f"- {count:>4}× `{basename}`")
    report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")


# ---------------------------------------------------------------------------
# Verify mode — scan src=/href= attributes and report unresolved local refs.
# ---------------------------------------------------------------------------

# Match `src="..."` and `href="..."` capturing the URL value.
_ATTR_REF = re.compile(r'\b(src|href)="([^"]*)"')

# Reference prefixes we consider "external / not a local file":
_EXTERNAL_PREFIXES = (
    "http://",
    "https://",
    "//",
    "data:",
    "mailto:",
    "tel:",
    "ftp://",
    "javascript:",
    "irc://",
    "ircs://",
    "gopher://",
    "magnet:",
    "ssh://",
    "git://",
    "mw-data:",  # MediaWiki virtual scheme for inline TemplateStyles —
                  # not a real fetchable resource; resolved client-side.
    "blob:",
)


def is_local_ref(value: str) -> bool:
    """Return True iff `value` should resolve to a sibling file."""
    if not value:
        return False
    if value.startswith("#"):
        return False  # in-page anchor
    if value.startswith(_EXTERNAL_PREFIXES):
        return False
    return True


def verify_mode(target: Path, report_path_arg: str | None) -> int:
    """Scan every .xhtml file for local-pointing `src=`/`href=` attributes
    whose decoded basename does not exist in `target`. Also flag any
    surviving broken-URL shapes (`/w/images/`, `../wiki-images/`).
    """
    xhtml_files = sorted(p for p in target.glob("*.xhtml") if p.is_file())
    if not xhtml_files:
        print(f"error: no *.xhtml files under {target}", file=sys.stderr)
        return 2

    unresolved: Counter[str] = Counter()
    leftover_broken: Counter[str] = Counter()
    total_refs = 0
    local_refs = 0
    by_extension: Counter[str] = Counter()

    # Cache sibling files for O(1) existence check (case-sensitive).
    local_files = {p.name for p in target.iterdir() if p.is_file()}

    for xhtml in xhtml_files:
        try:
            text = xhtml.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for match in _ATTR_REF.finditer(text):
            value = match.group(2)
            total_refs += 1
            # Surviving broken-URL shapes (the restore script should have
            # caught all of these; any leftover is a real diagnostic).
            if value.startswith("/w/images/") or value.startswith("../wiki-images/"):
                leftover_broken[value] += 1
                continue
            if not is_local_ref(value):
                continue
            # Strip any in-page fragment.
            local_path = value.split("#", 1)[0].split("?", 1)[0]
            if not local_path:
                continue
            local_refs += 1
            decoded = urllib.parse.unquote(local_path)
            # Only the final path component is the sibling-file basename;
            # local wiki-internal links use plain `Page_Name.xhtml`.
            basename = Path(decoded).name
            if not basename:
                continue
            # Filenames cannot contain `:` on most file systems; a `:`
            # in the basename indicates a URL scheme component (e.g.,
            # `mw-data:TemplateStyles:rNNN`) we already gate via the
            # external-prefix list elsewhere — skip defensively here too.
            if ":" in basename:
                continue
            # File refs must have an extension. Bareword identifiers
            # (`auth.wikimedia.org` is the upstream's literal href; not
            # a local file) and namespace tokens slip through if we
            # don't require a `.`. We DO require it AND a non-empty
            # suffix to avoid the hostname-as-path false positive.
            ext = Path(basename).suffix.lower()
            if not ext:
                continue
            by_extension[ext] += 1
            if basename not in local_files:
                unresolved[basename] += 1

    # --- Console summary -----------------------------------------------
    print()
    print(f"Verified {len(xhtml_files)} .xhtml files in {target}")
    print(f"Total src=/href= attribute count: {total_refs}")
    print(f"Local-pointing references:         {local_refs}")
    print(f"Unresolved local references:       {sum(unresolved.values())} "
          f"({len(unresolved)} unique basenames)")
    print(f"Leftover broken-URL refs:          {sum(leftover_broken.values())} "
          f"({len(leftover_broken)} unique URLs)")
    print()
    print("By extension (top 12):")
    for ext, count in by_extension.most_common(12):
        print(f"  {ext or '<no-ext>':>10}: {count}")
    if unresolved:
        print()
        print(f"Top 25 unresolved local basenames (count × name):")
        for basename, count in sorted(
            unresolved.items(), key=lambda kv: (-kv[1], kv[0])
        )[:25]:
            print(f"  {count:>4}× {basename}")
    if leftover_broken:
        print()
        print(f"Leftover broken URL shapes (top 5):")
        for url, count in sorted(
            leftover_broken.items(), key=lambda kv: (-kv[1], kv[0])
        )[:5]:
            print(f"  {count:>4}× {url}")

    if report_path_arg:
        lines = [
            "# nesdev_wiki verify report",
            "",
            f"Target: `{target}`",
            f"Files scanned: {len(xhtml_files)}",
            f"Total attribute references: {total_refs}",
            f"Local-pointing references: {local_refs}",
            f"Unresolved local references: {sum(unresolved.values())} "
            f"({len(unresolved)} unique basenames)",
            f"Leftover broken-URL refs: {sum(leftover_broken.values())} "
            f"({len(leftover_broken)} unique URLs)",
            "",
            "## By extension",
            "",
        ]
        for ext, count in by_extension.most_common():
            lines.append(f"- `{ext or '<no-ext>'}`: {count}")
        lines.append("")
        if unresolved:
            lines.append(f"## Unresolved local basenames ({len(unresolved)} unique)")
            lines.append("")
            for basename, count in sorted(
                unresolved.items(), key=lambda kv: (-kv[1], kv[0])
            ):
                lines.append(f"- {count:>4}× `{basename}`")
            lines.append("")
        if leftover_broken:
            lines.append(f"## Leftover broken-URL references ({len(leftover_broken)} unique)")
            lines.append("")
            for url, count in sorted(
                leftover_broken.items(), key=lambda kv: (-kv[1], kv[0])
            ):
                lines.append(f"- {count:>4}× `{url}`")
            lines.append("")
        report_path = Path(report_path_arg).resolve()
        report_path.write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"\nReport: {report_path}")

    return 0 if not unresolved and not leftover_broken else 1


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__,
        formatter_class=argparse.RawDescriptionHelpFormatter,
    )
    parser.add_argument(
        "--target",
        help="Path to the nesdev_wiki/ directory (default: inferred from script location).",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Preview rewrites without writing any files.",
    )
    parser.add_argument(
        "--no-backup",
        action="store_true",
        help="Skip the per-file .bak backup. Irreversible without git history.",
    )
    parser.add_argument(
        "--verbose",
        "-v",
        action="store_true",
        help="Print one line per modified file.",
    )
    parser.add_argument(
        "--report",
        help="Write a Markdown summary to this path after the run.",
    )
    parser.add_argument(
        "--verify",
        action="store_true",
        help="Verify-only mode: do not rewrite anything; instead scan all "
        ".xhtml files for local-pointing src=/href= attributes whose target "
        "does NOT resolve to a sibling file. Reports misses + leftover "
        "broken-URL shapes. Useful after a `restore` run + image download.",
    )
    args = parser.parse_args(argv)

    if args.verify:
        return verify_mode(discover_target(args.target), args.report)

    target = discover_target(args.target)
    if not target.is_dir():
        print(f"error: {target} is not a directory", file=sys.stderr)
        return 2

    xhtml_files = sorted(p for p in target.glob("*.xhtml") if p.is_file())
    if not xhtml_files:
        print(f"error: no *.xhtml files under {target}", file=sys.stderr)
        return 2

    stats = Stats()
    backup = not args.no_backup
    for path in xhtml_files:
        process_one(
            path,
            target,
            stats,
            dry_run=args.dry_run,
            backup=backup,
            verbose=args.verbose,
        )

    # --- Console summary -----------------------------------------------
    print()
    print(f"Scanned {stats.files_scanned} .xhtml files in {target}")
    print(f"Modified {stats.files_modified} files" + (" (dry-run)" if args.dry_run else ""))
    if stats.write_errors:
        print(f"  {stats.write_errors} write errors")
    print()
    print("Rewrites by pattern:")
    for name, _ in PATTERNS:
        print(
            f"  {name:>10}  rewrites={stats.rewrites_by_pattern[name]:>5} "
            f" misses={stats.misses_by_pattern[name]:>4}"
        )
    print()
    print(f"Unique missing image basenames: {len(stats.missing_basenames)}")
    if stats.missing_basenames and args.verbose:
        print("Top 10 missing:")
        for basename, count in sorted(
            stats.missing_basenames.items(), key=lambda kv: (-kv[1], kv[0])
        )[:10]:
            print(f"  {count:>4}× {basename}")

    if args.report:
        report_path = Path(args.report).resolve()
        write_report(report_path, stats, target)
        print(f"\nReport: {report_path}")

    return 0 if stats.write_errors == 0 else 1


if __name__ == "__main__":
    sys.exit(main())
