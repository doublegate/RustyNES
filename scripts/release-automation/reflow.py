#!/usr/bin/env python3
"""Unwrap hard-wrapped markdown paragraphs/bullets into single full-width lines.

Preserves: blank lines (including runs), ATX headings (#...), horizontal rules
(--- / ***), fenced code blocks, tables (| ...), blockquotes (> ...), raw HTML
lines, and **hard breaks** (a line ending in two or more spaces).
Joins wrapped continuation lines within a paragraph or a single list item.

# Hard breaks

A markdown hard break is encoded as two trailing spaces, and it MEANS "end the
line here". Joining across one changes the rendered output, so a hard-break line
terminates its logical line and keeps its two spaces. The original version
`strip()`ed every buffered piece and silently destroyed them.

# Fences

A fenced block is closed only by a fence of the SAME character and at least the
same length, per CommonMark -- so ``` does not close a ````-opened block, and a
tilde fence does not close a backtick one. Fences nested inside a blockquote or
list item are NOT recognized; such a block's lines are treated as ordinary text.
That is a known limit, not an oversight: handling container prefixes properly
needs a real block parser, and release notes do not use the construct.

Requires Python 3.9+ (`str.removeprefix`). CI runs 3.12.
"""
import re
import sys

STRUCT_PREFIXES = ("#", ">", "|")

# An opening/closing fence: optional indent, then 3+ backticks or 3+ tildes.
FENCE_RE = re.compile(r"^[ \t]*(`{3,}|~{3,})")


def is_list_item(s):
    t = s.lstrip()
    if t[:2] in ("- ", "* ", "+ "):
        return True
    # ordered "N. " or "N) "
    i = 0
    while i < len(t) and t[i].isdigit():
        i += 1
    return i > 0 and i < len(t) and t[i] in ".)" and t[i + 1 : i + 2] == " "


def is_hr(s):
    t = s.strip()
    return len(t) >= 3 and set(t) <= {"-", "*", "_"} and len(set(t)) == 1


def is_struct(s):
    t = s.lstrip()
    if not t:
        return False
    if t[0] in STRUCT_PREFIXES:
        return True
    if t.startswith("<") and t.rstrip().endswith(">"):
        return True  # standalone HTML line
    return is_hr(s) or is_list_item(s)


def reflow(text):
    lines = text.split("\n")
    out = []
    buf = []  # accumulated logical line pieces (trailing space PRESERVED)
    fence = None  # (char, run_length) of the open fence, or None

    def flush():
        """Emit the buffered pieces as one logical line, splitting at hard breaks."""
        if not buf:
            return

        def indent_of(line):
            return line[: len(line) - len(line.lstrip())]

        parts = []  # (indent, text)
        cur = []
        cur_indent = indent_of(buf[0])
        for piece in buf:
            if not cur:
                # Each emitted line keeps the indent of the piece that STARTS
                # it, not the first piece of the whole buffer. After a hard
                # break inside a list item the continuation must keep the item's
                # content indent, or markdown ends the list item there.
                cur_indent = indent_of(piece)
            cur.append(piece.strip())
            # Two-or-more trailing spaces on a non-empty line = hard break: end
            # the logical line here and keep the marker so it still renders.
            if piece.strip() and piece.endswith("  "):
                parts.append((cur_indent, " ".join(cur) + "  "))
                cur = []
        if cur:
            parts.append((cur_indent, " ".join(cur)))
        for ind, text in parts:
            out.append(ind + text)
        buf.clear()

    for ln in lines:
        stripped = ln.strip()
        # Fenced code block. Only a fence of the same character and at least the
        # same length closes an open one (CommonMark), so a ``` line inside a
        # ````-opened block stays content.
        m = FENCE_RE.match(ln)
        if m:
            marker = m.group(1)
            ch, run = marker[0], len(marker)
            if fence is None:
                flush()
                out.append(ln)
                fence = (ch, run)
                continue
            if ch == fence[0] and run >= fence[1]:
                out.append(ln)
                fence = None
                continue
            # A non-matching fence inside an open block is just content.
        if fence is not None:
            out.append(ln)
            continue
        if stripped == "":
            flush()
            out.append("")
            continue
        # Blockquote: join consecutive `>` lines into one full-width `> ...` line.
        if stripped.startswith(">"):
            content = ln.lstrip()[1:]
            content = content.removeprefix(" ")
            if buf and buf[0].lstrip().startswith(">"):
                buf.append(content)  # continuation of the current blockquote
            else:
                flush()
                buf.append(ln)  # seed with the full `> ...` line
            continue
        if is_struct(ln):
            # A structural line starts its own logical line. For a list item we
            # still want to absorb ITS wrapped continuations, so seed the buffer.
            flush()
            if is_list_item(ln):
                buf.append(ln)
            else:
                out.append(ln.rstrip())
            continue
        # Plain text: continuation of the current paragraph/list item, or a new
        # paragraph if the buffer is empty.
        buf.append(ln)
    flush()
    # Blank-line RUNS are preserved. The original collapsed them, which
    # contradicted this module's own "preserves blank lines" contract; markdown
    # renders one blank the same as three, so collapsing bought nothing and cost
    # fidelity to the author's file.
    return "\n".join(out).rstrip() + "\n"


if __name__ == "__main__":
    sys.stdout.write(reflow(sys.stdin.read()))
