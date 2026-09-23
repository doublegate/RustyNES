# SPDX-License-Identifier: GPL-3.0-or-later
"""MkDocs hook: point links that leave ``docs/`` at the repository instead.

The documents under ``docs/`` are the project's specs, read in two places: on
GitHub, where a relative link such as ``../to-dos/plans/x.md`` or
``../crates/rustynes-core/src/save_state.rs`` resolves to the file beside the
document, and in the MkDocs handbook, where anything outside ``docs_dir`` does
not exist. MkDocs reported every such link as a WARNING on every Pages build
(13 of them on 2026-09-23), and on the published site each one was a dead link.

Rewriting the source would break the GitHub reading, which is the primary one.
So the rewrite happens at build time instead: a relative link whose target
resolves OUTSIDE ``docs_dir`` -- or to a docs file the build excludes, such as
an ADR -- becomes ``<repo_url>/blob/main/<path>``, keeping any ``#fragment``.
Links to published pages, links to files that do not exist, absolute URLs,
anchors and mail links are untouched, so MkDocs still validates and reports
every one of them as before.

Enabled by ``hooks:`` in ``mkdocs.yml``.
"""

from __future__ import annotations

import posixpath
import re

# `[text](target)` and `[text](target "title")`, excluding images' leading `!`
# only in the sense that images are rewritten too: an image outside docs/ is
# equally absent from the site. Nested brackets in link text are not supported,
# which matches the corpus.
_LINK = re.compile(r"(\]\()(?P<target>[^)\s]+)(?P<rest>(?:\s+\"[^\"]*\")?\))")

_SKIP_PREFIXES = ("http://", "https://", "mailto:", "#", "/")


def _rewrite(target: str, page_dir: str, repo_url: str, files) -> str | None:
    """Return the repository URL for ``target``, or None to leave it alone."""
    if target.startswith(_SKIP_PREFIXES) or "://" in target:
        return None
    path, sep, fragment = target.partition("#")
    if not path:
        return None
    # `page_dir` is relative to docs_dir; resolve against the repository root,
    # where docs_dir is `docs/`.
    resolved = posixpath.normpath(posixpath.join("docs", page_dir, path))
    if resolved.startswith(".."):
        return None  # escapes the repository too; leave it for MkDocs to report
    if resolved.startswith("docs/"):
        # Inside docs/: leave it for MkDocs to validate UNLESS the target is a
        # file the build excludes (`exclude_docs` keeps the ADRs, audit logs and
        # archives out of the handbook), which is as absent from the site as a
        # file outside docs/ -- 16 such links to ADRs on 2026-09-23.
        # A target that does not exist at all is left alone too: rewriting it
        # would turn a broken link MkDocs reports into a dead GitHub URL that
        # nothing reports.
        in_site = files.get_file_from_path(resolved[len("docs/") :])
        if in_site is None or not _is_excluded(in_site):
            return None
    elif resolved == "docs":
        return None
    return f"{repo_url.rstrip('/')}/blob/main/{resolved}{sep}{fragment}"


def _is_excluded(file) -> bool:
    """True for a docs file MkDocs will not publish (`exclude_docs`, drafts)."""
    inclusion = getattr(file, "inclusion", None)
    return inclusion is not None and inclusion.is_excluded()


def on_page_markdown(markdown, page, config, files):
    repo_url = config.get("repo_url")
    if not repo_url:
        return markdown
    page_dir = posixpath.dirname(page.file.src_uri)

    def sub(match: re.Match) -> str:
        new = _rewrite(match.group("target"), page_dir, repo_url, files)
        if new is None:
            return match.group(0)
        return f"{match.group(1)}{new}{match.group('rest')}"

    return _LINK.sub(sub, markdown)
