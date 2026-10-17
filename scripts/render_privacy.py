#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Render PRIVACY.md to the standalone Pages page at /RustyNES/privacy/.

v3.4.0. The privacy policy has one source, `PRIVACY.md` at the repository root
(linked from the README and the apps). GitHub Pages serves the same text at
https://doublegate.github.io/RustyNES/privacy/ so that the apps and the
RetroAchievements registration can link a stable, human-readable page. This
script is the bridge: `web.yml` runs it while assembling the Pages artifact,
and on pull requests it runs too (to a scratch path) so a policy that does not
render fails review rather than the deploy.

It uses Python-Markdown, which the same job already installs as a dependency
of MkDocs, so it adds nothing to the build. Usage:

    python3 scripts/render_privacy.py PRIVACY.md _site/privacy/index.html
"""

from __future__ import annotations

import html
import sys
from pathlib import Path

import markdown

PAGE = """<!doctype html>
<html lang="en">
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<title>{title}</title>
<link rel="canonical" href="https://doublegate.github.io/RustyNES/privacy/">
<style>
:root {{ color-scheme: light dark; --fg: #1d1d1f; --bg: #ffffff; --link: #0b62c4; --rule: #d0d0d5; }}
@media (prefers-color-scheme: dark) {{
  :root {{ --fg: #e8e8ea; --bg: #141416; --link: #6cb4ff; --rule: #3a3a40; }}
}}
body {{ margin: 0; background: var(--bg); color: var(--fg);
  font: 16px/1.6 system-ui, -apple-system, "Segoe UI", sans-serif; }}
main {{ max-width: 46rem; margin: 0 auto; padding: 2rem 1rem 4rem; }}
a {{ color: var(--link); }}
h1, h2, h3 {{ line-height: 1.25; }}
h2 {{ border-top: 1px solid var(--rule); padding-top: 1.2rem; margin-top: 2rem; }}
code {{ font-size: 0.92em; }}
footer {{ margin-top: 3rem; font-size: 0.9em; opacity: 0.8; }}
</style>
</head>
<body>
<main>
{body}
<footer><a href="../">RustyNES</a> &middot;
<a href="https://github.com/doublegate/RustyNES/blob/main/PRIVACY.md">Source and history</a></footer>
</main>
</body>
</html>
"""


def render(src: str) -> str:
    first = next((ln for ln in src.splitlines() if ln.startswith("# ")), "# Privacy policy")
    title = first[2:].strip()
    body = markdown.markdown(src, extensions=["tables", "sane_lists"], output_format="html")
    return PAGE.format(title=html.escape(title), body=body)


def main(argv: list[str]) -> int:
    if len(argv) != 3:
        print(__doc__, file=sys.stderr)
        return 2
    src = Path(argv[1]).read_text(encoding="utf-8")
    out = Path(argv[2])
    page = render(src)
    # A policy that rendered to almost nothing is a broken build, not a page.
    if "<h2" not in page or len(page) < 2000:
        print("error: PRIVACY.md rendered to an implausibly small page", file=sys.stderr)
        return 1
    out.parent.mkdir(parents=True, exist_ok=True)
    out.write_text(page, encoding="utf-8")
    print(f"wrote {out} ({len(page)} bytes)")
    return 0


if __name__ == "__main__":
    sys.exit(main(sys.argv))
