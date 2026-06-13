#!/usr/bin/env python3
"""
process_raw_nesdev_pages.py — clean up wiki-infrastructure references
introduced by raw HTML downloads from
`download_missing_nesdev_pages.py`.

The 32 newly-downloaded `.xhtml` files contain raw MediaWiki output with
in-page references to wiki chrome that the original crawler-processed
files have stripped:

* `/wiki/PageName` — wiki article navigation links
* `/w/index.php?...` — MediaWiki action endpoints (edit/history/log/feed)
* `/w/load.php?...` — Resource Loader entrypoints (CSS/JS bundles)
* `/w/rest.php/...` — MediaWiki REST API
* Protocol-relative `//host/path` — third-party scripts (rare)

This script rewrites those references so the local mirror's verifier
no longer reports them as "unresolved local refs":

1. ``/wiki/PageName`` → ``PageName.xhtml`` when the locally-named file
   exists (using the same filename transform the crawler used:
   replace every non-`[A-Za-z0-9_]` char with ``_``). Otherwise
   rewrite to the absolute upstream URL so the browser can fetch
   the live wiki page.
2. ``/w/...`` → absolute upstream URL (this is the live wiki's
   action endpoint; clicking it goes off-site to the wiki, which
   is the natural behaviour for "edit this page" links).
3. ``//host/...`` → ``https://host/...``.
4. Any other ``/...`` absolute reference → upstream-absolute URL.

The upstream base is read from each file's ``<link rel="canonical">``
header, so ``mediawikiwiki_*.xhtml`` files (interwiki to mediawiki.org)
are rewritten against the right host automatically.

Detection: this script only touches HTML5 files (those starting with
``<!DOCTYPE html>``). The 2,769 crawler-processed XHTML 1.0 files start
with ``<?xml version="1.0" ?>`` and are skipped untouched.

Usage::

    python3 scripts/process_raw_nesdev_pages.py
    python3 scripts/process_raw_nesdev_pages.py --dry-run
    python3 scripts/process_raw_nesdev_pages.py --report PATH
"""

from __future__ import annotations

import argparse
import re
import sys
import urllib.parse
from collections import Counter
from pathlib import Path


CANONICAL_RE = re.compile(
    r'<link\s+rel="canonical"\s+href="([^"]+)"', re.IGNORECASE
)
ATTR_RE = re.compile(r'\b(src|href)="([^"]*)"')

# Drop upstream DNS prefetch / preconnect hints — these are performance
# optimizations meaningful only on the live wiki; in the mirror they
# render as malformed `<link href="host.tld">` tags (no scheme) that
# the browser ignores and the verifier reports as unresolved file refs.
DNS_HINT_RE = re.compile(
    r'<link\s+rel="(?:dns-prefetch|preconnect)"[^>]*>\s*', re.IGNORECASE
)

# Prefixes we do NOT touch (already off-site, in-page anchors, or
# special wiki schemes).
EXTERNAL_PREFIXES = (
    "http://",
    "https://",
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
    "mw-data:",  # MediaWiki virtual scheme for inline TemplateStyles
    "blob:",
)


def is_raw_html5(text: str) -> bool:
    """Return True iff the document looks like a raw MediaWiki HTML5
    download (vs. the crawler-processed XHTML 1.0 Transitional)."""
    head = text[:400].lstrip()
    return head.lower().startswith("<!doctype html>")


def derive_upstream_base(text: str) -> str:
    """Read the upstream base URL from the document's
    ``<link rel="canonical">`` header. Falls back to nesdev.org."""
    m = CANONICAL_RE.search(text)
    if m:
        parts = urllib.parse.urlparse(m.group(1))
        if parts.scheme and parts.netloc:
            return f"{parts.scheme}://{parts.netloc}"
    return "https://www.nesdev.org"


def page_path_to_local_filename(page_part: str) -> str:
    """Apply the crawler's filename transform:

    1. URL-decode (%2C → ``,`` etc.)
    2. Replace every non-``[A-Za-z0-9_]`` char with ``_``
    3. Append ``.xhtml``

    This is the inverse of looking at the existing local filenames'
    `<link rel="canonical">` values: the upstream URL's path part
    after `/wiki/` is run through the same regex to derive the
    crawler-generated filename.
    """
    decoded = urllib.parse.unquote(page_part)
    return re.sub(r"[^A-Za-z0-9_]", "_", decoded) + ".xhtml"


def rewrite_attribute_value(
    value: str, upstream_base: str, local_files: set[str], stats: Counter
) -> str:
    """Rewrite one attribute value per the rules in the module docstring.
    Returns the new value (possibly unchanged)."""
    if not value or value.startswith("#"):
        return value
    if value.startswith(EXTERNAL_PREFIXES):
        return value
    # Protocol-relative
    if value.startswith("//"):
        stats["proto_relative"] += 1
        return "https:" + value
    # /wiki/PageName  (possibly with query/fragment)
    if value.startswith("/wiki/"):
        rest = value[len("/wiki/") :]
        # Split fragment + query
        base_path, has_frag, frag = rest.partition("#")
        base_path, has_q, query = base_path.partition("?")
        # If there's a query string, this is a wiki-side action (rare on
        # /wiki/ paths but possible) — always send off-site.
        if has_q:
            stats["wiki_query_offsite"] += 1
            return f"{upstream_base}{value}"
        local = page_path_to_local_filename(base_path)
        if local in local_files:
            new_val = local
            if has_frag:
                new_val += "#" + frag
            stats["wiki_local"] += 1
            return new_val
        # Falls back to upstream — the page isn't in the mirror.
        stats["wiki_offsite"] += 1
        return f"{upstream_base}{value}"
    # /w/* backend URLs (index.php, load.php, rest.php, ...)
    if value.startswith("/w/"):
        stats["w_backend"] += 1
        return f"{upstream_base}{value}"
    # Any other site-absolute path (resources, special slots, etc.)
    if value.startswith("/"):
        stats["other_absolute"] += 1
        return f"{upstream_base}{value}"
    return value


def rewrite_text(
    text: str, upstream_base: str, local_files: set[str]
) -> tuple[str, Counter]:
    """Apply tag-stripping and attribute-value rewriting to a full
    document."""
    stats: Counter[str] = Counter()

    # 1) Drop DNS-prefetch / preconnect link tags entirely.
    def drop_hint(match: re.Match[str]) -> str:
        stats["dns_hint_dropped"] += 1
        return ""

    text = DNS_HINT_RE.sub(drop_hint, text)

    # 2) Rewrite attribute values.
    def repl(match: re.Match[str]) -> str:
        attr, value = match.group(1), match.group(2)
        new_value = rewrite_attribute_value(value, upstream_base, local_files, stats)
        if new_value == value:
            return match.group(0)
        return f'{attr}="{new_value}"'

    return ATTR_RE.sub(repl, text), stats


def discover_target(explicit: str | None) -> Path:
    if explicit:
        return Path(explicit).resolve()
    here = Path(__file__).resolve().parent.parent
    return here / "nesdev_wiki"


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument("--target", help="Path to nesdev_wiki/")
    parser.add_argument("--dry-run", action="store_true",
                        help="Preview without writing.")
    parser.add_argument("--report", help="Write Markdown summary to PATH.")
    parser.add_argument("--verbose", "-v", action="store_true",
                        help="Print one line per modified file with per-rule counts.")
    args = parser.parse_args(argv)

    target = discover_target(args.target)
    if not target.is_dir():
        print(f"error: {target} is not a directory", file=sys.stderr)
        return 2

    local_files = {p.name for p in target.iterdir() if p.is_file()}
    total_seen = 0
    raw_seen = 0
    modified = 0
    total_stats: Counter[str] = Counter()
    per_file: dict[str, dict] = {}

    for xhtml in sorted(target.glob("*.xhtml")):
        total_seen += 1
        try:
            text = xhtml.read_text(encoding="utf-8")
        except (OSError, UnicodeDecodeError):
            continue
        if not is_raw_html5(text):
            continue
        raw_seen += 1
        upstream = derive_upstream_base(text)
        new_text, stats = rewrite_text(text, upstream, local_files)
        total_stats.update(stats)
        per_file[xhtml.name] = {"upstream": upstream, "stats": dict(stats)}

        if new_text != text:
            modified += 1
            if not args.dry_run:
                xhtml.write_text(new_text, encoding="utf-8")
            if args.verbose:
                print(f"  {'(dry) ' if args.dry_run else ''}"
                      f"{xhtml.name:<60} upstream={upstream}  {dict(stats)}")

    print(f"\nScanned {total_seen} .xhtml files ({raw_seen} HTML5 raw, "
          f"{total_seen - raw_seen} XHTML 1.0 untouched)")
    print(f"Modified: {modified}" + (" (dry-run)" if args.dry_run else ""))
    if total_stats:
        print("\nTotal rewrites by rule:")
        for k in sorted(total_stats, key=lambda x: -total_stats[x]):
            print(f"  {k:>22}: {total_stats[k]}")

    if args.report:
        lines = [
            "# Raw-HTML reference rewriter — report",
            "",
            f"Target: `{target}`",
            f"Files scanned: {total_seen} ({raw_seen} HTML5 raw)",
            f"Files modified: {modified}",
            "",
            "## Rewrites by rule",
            "",
        ]
        for k in sorted(total_stats, key=lambda x: -total_stats[x]):
            lines.append(f"- `{k}`: {total_stats[k]}")
        lines.append("")
        lines.append("## Per-file detail")
        lines.append("")
        for name in sorted(per_file):
            entry = per_file[name]
            if not entry["stats"]:
                continue
            lines.append(f"### `{name}`")
            lines.append("")
            lines.append(f"- Upstream base: `{entry['upstream']}`")
            for k in sorted(entry["stats"], key=lambda x: -entry["stats"][x]):
                lines.append(f"- `{k}`: {entry['stats'][k]}")
            lines.append("")
        Path(args.report).write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"\nReport: {Path(args.report).resolve()}")

    return 0


if __name__ == "__main__":
    sys.exit(main())
