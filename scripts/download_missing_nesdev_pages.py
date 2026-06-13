#!/usr/bin/env python3
"""
download_missing_nesdev_pages.py — fetch the upstream wiki pages whose
local `.xhtml` copies are absent from the `nesdev_wiki/` mirror, so
existing cross-page links resolve.

Background
==========

The mirror lives at the project's gitignored `nesdev_wiki/` dir and
contains 2,769 `.xhtml` pages produced by an earlier crawl. The
companion `restore_nesdev_wiki_image_links.py --verify` pass reports a
handful of `.xhtml` filenames that other pages link to but which were
never captured (Category pages, MediaWiki: namespace, the per-mapper
icon templates, mediawikiwiki interwiki links, etc.).

This script:

1. Enumerates the missing `.xhtml` filenames via the same verifier.
2. Maps each filename back to its upstream URL using the rules
   empirically derived from the existing mirror's `<link rel="canonical">`
   tags. The crawler collapsed the upstream chars ``: . - / $``
   (and Unicode like ``µ``) into ``_`` -- a many-to-one transform,
   so ambiguous candidates fall back to MediaWiki's `opensearch`
   API to find the real title.
3. Downloads the HTML; saves it as `<filename>.xhtml`.
4. Reports successes / 404 / API misses.

After the download pass, run
``restore_nesdev_wiki_image_links.py`` once more on the corpus to
rewrite any `/w/images/...` or `../wiki-images/...` references in the
new pages, then the verifier again to confirm resolution.

`mediawikiwiki_*` filenames map to mediawiki.org (not nesdev.org)
because that interwiki prefix is the literal MediaWiki sister-wiki
reference. They share the same `Help:` / `Help_*` namespace handling.

Usage
=====

::

    python3 scripts/download_missing_nesdev_pages.py
    python3 scripts/download_missing_nesdev_pages.py --dry-run
    python3 scripts/download_missing_nesdev_pages.py --report PATH
"""

from __future__ import annotations

import argparse
import re
import sys
import time
import urllib.parse
import urllib.request
from collections import Counter
from pathlib import Path

# Known MediaWiki namespaces on nesdev.org. The crawler collapses the
# `:` separator into `_`, so filenames look like `<Namespace>_<Title>`.
NESDEV_NAMESPACES = {
    "Category",
    "User",
    "Talk",
    "Template",
    "Help",
    "File",
    "MediaWiki",
    "Special",
    "Project",
    "User_talk",
    "Category_talk",
    "Template_talk",
    "File_talk",
    "Help_talk",
    "MediaWiki_talk",
    "Project_talk",
}

# Interwiki prefixes — filenames carrying these point to a different
# wiki on a different host.
INTERWIKI_PREFIXES = {
    "mediawikiwiki": "https://www.mediawiki.org",
}

# MediaWiki namespaces on mediawiki.org we recognize for the
# interwiki path resolution.
MEDIAWIKI_NAMESPACES = {"Help", "Template", "Category", "MediaWiki", "Project"}

NESDEV_BASE = "https://www.nesdev.org"
USER_AGENT = (
    "RustyNES-v2 nesdev-mirror-restorer/1.0 "
    "(github.com/doublegate/RustyNES - repairing local archive)"
)


def split_namespace(stem: str) -> tuple[str | None, str]:
    """Try to split ``stem`` into (namespace, rest) where namespace is
    a recognized MediaWiki namespace. Returns (None, stem) on no match.
    Only the FIRST underscore boundary is considered.
    """
    # Try one-word namespaces.
    parts = stem.split("_", 1)
    if len(parts) == 2 and parts[0] in NESDEV_NAMESPACES:
        return parts[0], parts[1]
    # Try two-word talk namespaces (User_talk, Category_talk, ...).
    parts2 = stem.split("_", 2)
    if len(parts2) >= 2:
        two = f"{parts2[0]}_{parts2[1] if len(parts2) > 1 else ''}"
        if two in NESDEV_NAMESPACES:
            rest = parts2[2] if len(parts2) > 2 else ""
            return two, rest
    return None, stem


def candidate_url_nesdev(stem: str) -> str:
    """Build the most-likely upstream URL for a stem that targets
    nesdev.org. Underscores in the page TITLE portion are preserved
    (MediaWiki uses `_` for space; other punctuation in titles is
    literal in the URL).
    """
    namespace, title = split_namespace(stem)
    if namespace:
        path = f"{namespace}:{title}"
    else:
        path = stem
    return f"{NESDEV_BASE}/wiki/{urllib.parse.quote(path, safe=':/_')}"


def candidate_url_interwiki(stem: str) -> str | None:
    """If ``stem`` carries an interwiki prefix, build the corresponding
    URL. Returns None if no prefix matches.

    Example: ``mediawikiwiki_Help_Contents`` →
    ``https://www.mediawiki.org/wiki/Help:Contents``
    """
    for prefix, base in INTERWIKI_PREFIXES.items():
        marker = prefix + "_"
        if stem.startswith(marker):
            rest = stem[len(marker) :]
            # On the target wiki, split off a recognized namespace.
            parts = rest.split("_", 1)
            if len(parts) == 2 and parts[0] in MEDIAWIKI_NAMESPACES:
                page = f"{parts[0]}:{parts[1]}"
            else:
                page = rest
            return f"{base}/wiki/{urllib.parse.quote(page, safe=':/_')}"
    return None


def opensearch_resolve(stem: str, base: str = NESDEV_BASE) -> str | None:
    """Use the MediaWiki ``opensearch`` API to resolve an ambiguous
    filename stem to its real title. Returns the title's URL on a hit,
    or None.
    """
    # Replace underscores with spaces for the search query.
    search_term = stem.replace("_", " ")
    url = (
        f"{base}/w/api.php?action=opensearch"
        f"&search={urllib.parse.quote(search_term)}"
        f"&limit=1&format=json"
    )
    try:
        req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
        with urllib.request.urlopen(req, timeout=15) as resp:
            import json

            data = json.loads(resp.read().decode("utf-8", errors="replace"))
            if isinstance(data, list) and len(data) >= 4:
                # data[3] is the list of result URLs.
                urls = data[3]
                if urls:
                    return urls[0]
    except Exception as exc:
        print(f"  opensearch error for {stem!r}: {exc}", file=sys.stderr)
    return None


def fetch(url: str) -> tuple[int, bytes]:
    """HTTP GET. Returns (status_code, body_bytes).

    On 4xx responses the body is preserved — MediaWiki's 404 page for a
    redlink is a fully-rendered HTML page that says "this page doesn't
    exist", which is the correct content for a missing-subpage placeholder.
    """
    req = urllib.request.Request(url, headers={"User-Agent": USER_AGENT})
    try:
        with urllib.request.urlopen(req, timeout=30) as resp:
            return resp.status, resp.read()
    except urllib.error.HTTPError as e:
        try:
            body = e.read()
        except Exception:
            body = b""
        return e.code, body
    except Exception as e:
        print(f"  fetch error {url}: {e}", file=sys.stderr)
        return 0, b""


def digit_pair_dot_variants(url: str) -> list[str]:
    """Generate URL variants where each `_<d>_<d>_` digit pair becomes
    `_<d>.<d>_`. The crawler collapsed dots into underscores, so version
    numbers like ``4.0`` round-trip ambiguously. Returns the variants in
    "try first" order (more-specific first), excluding the original.
    """
    # Find all overlapping `_<digit>_<digit>_` boundaries that could be dots.
    # We apply one substitution at a time (the typical case is a single
    # version-number pair).
    pat = re.compile(r"_(\d)_(\d)_")
    variants = []
    for m in pat.finditer(url):
        candidate = url[: m.start()] + f"_{m.group(1)}.{m.group(2)}_" + url[m.end() :]
        if candidate != url:
            variants.append(candidate)
    return variants


def subpage_variants(url: str) -> list[str]:
    """Generate URL variants where the LAST underscore in the page title
    is treated as a subpage separator (``/``). MediaWiki subpages like
    ``INES_Mapper_074/Icon`` were collapsed by the crawler into
    ``INES_Mapper_074_Icon``.
    """
    # Operate only on the path portion. We don't replace underscores in
    # the namespace prefix (`Category:`, `User:` etc.) so we look at the
    # part AFTER the last `:` (if any).
    proto, _, rest = url.partition("://")
    host_and_path = rest
    last_slash = host_and_path.rfind("/")
    if last_slash < 0:
        return []
    title = host_and_path[last_slash + 1 :]
    prefix = host_and_path[: last_slash + 1]
    # Walk underscore positions from rightmost to leftmost — the rightmost
    # ones are most-likely subpage separators in practice.
    underscores = [i for i, ch in enumerate(title) if ch == "_"]
    variants = []
    for i in reversed(underscores):
        # Only flip if it's BELOW a colon-separator (don't break namespace).
        colon = title.rfind(":")
        if i <= colon:
            continue
        new_title = title[:i] + "/" + title[i + 1 :]
        variants.append(f"{proto}://{prefix}{new_title}")
    return variants


def list_missing(target: Path) -> Counter[str]:
    """Re-discover missing local-pointing .xhtml refs (filters out
    gopher://, anchors, non-extension refs). Mirrors the verifier's
    logic but is fully self-contained here so this script doesn't
    depend on the restorer module API surface.
    """
    attr_pat = re.compile(r'\b(src|href)="([^"]*)"')
    external_prefixes = (
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
    )
    local_files = {p.name for p in target.iterdir() if p.is_file()}
    unresolved: Counter[str] = Counter()
    for xhtml in target.glob("*.xhtml"):
        try:
            text = xhtml.read_text(encoding="utf-8")
        except (UnicodeDecodeError, OSError):
            continue
        for m in attr_pat.finditer(text):
            value = m.group(2)
            if (
                not value
                or value.startswith("#")
                or value.startswith(external_prefixes)
            ):
                continue
            if value.startswith("/w/images/") or value.startswith("../wiki-images/"):
                continue
            local_path = value.split("#", 1)[0].split("?", 1)[0]
            if not local_path:
                continue
            decoded = urllib.parse.unquote(local_path)
            basename = Path(decoded).name
            if not basename or "." not in basename:
                # Non-file refs (e.g., bare identifiers in URL paths).
                continue
            if basename not in local_files:
                unresolved[basename] += 1
    return unresolved


def main(argv: list[str] | None = None) -> int:
    parser = argparse.ArgumentParser(
        description=__doc__, formatter_class=argparse.RawDescriptionHelpFormatter
    )
    parser.add_argument(
        "--target",
        help="Path to the nesdev_wiki/ directory.",
    )
    parser.add_argument(
        "--dry-run",
        action="store_true",
        help="Print the URL we'd fetch for each missing file, but don't fetch.",
    )
    parser.add_argument(
        "--report",
        help="Write a Markdown summary to this path after the run.",
    )
    parser.add_argument(
        "--delay-ms",
        type=int,
        default=200,
        help="Inter-request delay in ms (default: 200 — polite throttle).",
    )
    args = parser.parse_args(argv)

    if args.target:
        target = Path(args.target).resolve()
    else:
        target = Path(__file__).resolve().parent.parent / "nesdev_wiki"
    if not target.is_dir():
        print(f"error: {target} not a directory", file=sys.stderr)
        return 2

    missing = list_missing(target)
    print(f"Found {len(missing)} unique missing .xhtml filenames "
          f"({sum(missing.values())} references)")

    if not missing:
        return 0

    results: dict[str, dict] = {}
    for stem_with_ext, ref_count in sorted(
        missing.items(), key=lambda kv: (-kv[1], kv[0])
    ):
        if not stem_with_ext.endswith(".xhtml"):
            continue
        stem = stem_with_ext[: -len(".xhtml")]

        # Strategy: try interwiki first (only matches if prefix present),
        # else build a candidate nesdev URL with the most-likely namespace.
        # On 404, fall back to opensearch to resolve underscore ambiguity.
        url = candidate_url_interwiki(stem)
        base_host_for_search = NESDEV_BASE
        if url and "mediawiki.org" in url:
            base_host_for_search = "https://www.mediawiki.org"
        if not url:
            url = candidate_url_nesdev(stem)

        if args.dry_run:
            results[stem_with_ext] = {
                "url": url,
                "status": "dry-run",
                "refs": ref_count,
            }
            print(f"  [dry] {stem_with_ext:<60} -> {url}")
            continue

        original_url = url
        status, body = fetch(url)

        # Build the fallback URL chain — try in order until one returns 200.
        if status != 200:
            fallbacks: list[str] = []
            # 1) digit-pair-to-dot — handles version numbers like 4.0
            fallbacks.extend(digit_pair_dot_variants(url))
            # 2) subpage — last underscore → `/`
            fallbacks.extend(subpage_variants(url))
            # 3) opensearch — decodes underscore ambiguity via search.
            search_host = base_host_for_search
            search_stem = stem
            if stem.startswith("mediawikiwiki_"):
                search_stem = stem[len("mediawikiwiki_") :]
            resolved = opensearch_resolve(search_stem, search_host)
            if resolved and resolved not in fallbacks:
                fallbacks.append(resolved)

            for fb in fallbacks:
                s2, b2 = fetch(fb)
                if s2 == 200 and b2:
                    url = fb
                    status = s2
                    body = b2
                    break

        outcome: dict
        if status == 200 and body:
            dest = target / stem_with_ext
            dest.write_bytes(body)
            outcome = {
                "url": url,
                "status": 200,
                "bytes": len(body),
                "refs": ref_count,
            }
            print(
                f"  OK   {stem_with_ext:<60} -> {url}  ({len(body):,} B, "
                f"{ref_count} refs)"
            )
        elif body:
            # 404 (or other) WITH body — MediaWiki redlinks return a
            # useful "page doesn't exist" HTML page. Save it as the
            # local placeholder so the cross-reference resolves and
            # the user sees the wiki's official "create this page"
            # UI when they click through. Use the ORIGINAL natural
            # URL (the redlink target) for the recorded source.
            dest = target / stem_with_ext
            dest.write_bytes(body)
            outcome = {
                "url": original_url,
                "status": status,
                "bytes": len(body),
                "refs": ref_count,
                "redlink": True,
            }
            print(
                f"  RED  {stem_with_ext:<60} -> {original_url}  "
                f"(HTTP {status} redlink placeholder, {len(body):,} B, "
                f"{ref_count} refs)"
            )
        else:
            outcome = {
                "url": url,
                "status": status,
                "bytes": 0,
                "refs": ref_count,
            }
            print(f"  FAIL {stem_with_ext:<60} -> {url}  (HTTP {status})")

        results[stem_with_ext] = outcome
        time.sleep(max(args.delay_ms, 0) / 1000.0)

    # --- summary -------------------------------------------------------
    ok = [n for n, r in results.items() if r["status"] == 200]
    redlinks = [n for n, r in results.items() if r.get("redlink")]
    fail = [
        n
        for n, r in results.items()
        if r["status"] != 200 and not r.get("redlink") and r["status"] != "dry-run"
    ]
    total_bytes = sum(r.get("bytes", 0) for r in results.values())
    print()
    print(f"Downloaded: {len(ok)} pages, {total_bytes:,} bytes total")
    if redlinks:
        print(f"Redlinks:   {len(redlinks)} pages (saved as upstream's "
              f"`page does not exist` placeholder)")
        for n in redlinks:
            print(f"  - {n} (HTTP {results[n]['status']})")
    if fail:
        print(f"Failed:     {len(fail)} pages")
        for n in fail:
            print(f"  - {n} (HTTP {results[n]['status']})")

    if args.report:
        lines = [
            "# Missing nesdev_wiki pages — download report",
            "",
            f"Target: `{target}`",
            f"Unique missing filenames at start: {len(missing)}",
            f"References at start: {sum(missing.values())}",
            f"Downloaded: {len(ok)}",
            f"Failed: {len(fail)}",
            f"Total bytes downloaded: {total_bytes:,}",
            "",
            "## Downloaded",
            "",
        ]
        for n in sorted(ok):
            r = results[n]
            lines.append(f"- `{n}` ({r['refs']} refs) — {r['bytes']:,} B "
                         f"from `{r['url']}`")
        if fail:
            lines += ["", "## Failed", ""]
            for n in sorted(fail):
                r = results[n]
                lines.append(
                    f"- `{n}` ({r['refs']} refs) — HTTP {r['status']} "
                    f"from `{r['url']}`"
                )
        Path(args.report).write_text("\n".join(lines) + "\n", encoding="utf-8")
        print(f"\nReport: {Path(args.report).resolve()}")

    # Redlinks are an EXPECTED outcome for references to pages that
    # don't exist on upstream — the placeholder body still resolves the
    # local-cross-reference correctly. Only treat unrecoverable failures
    # as a non-zero exit.
    return 0 if not fail else 1


if __name__ == "__main__":
    sys.exit(main())
