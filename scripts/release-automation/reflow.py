#!/usr/bin/env python3
"""Unwrap hard-wrapped markdown paragraphs/bullets into single full-width lines.

Preserves: blank lines, ATX headings (#...), horizontal rules (--- / ***),
fenced code blocks (```), tables (| ...), blockquotes (> ...), and raw HTML lines.
Joins wrapped continuation lines within a paragraph or a single list item.
"""
import sys

STRUCT_PREFIXES = ("#", ">", "|")


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
    buf = []  # accumulated logical line pieces
    in_code = False

    def flush():
        if buf:
            first = buf[0]
            indent = first[: len(first) - len(first.lstrip())]
            joined = indent + " ".join(p.strip() for p in buf)
            out.append(joined)
            buf.clear()

    for ln in lines:
        stripped = ln.strip()
        # Fenced code block toggle.
        if stripped.startswith(("```", "~~~")):
            flush()
            out.append(ln)
            in_code = not in_code
            continue
        if in_code:
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
                buf.append(ln.rstrip())  # seed with the full `> ...` line
            continue
        if is_struct(ln):
            # A structural line starts its own logical line. For a list item we
            # still want to absorb ITS wrapped continuations, so seed the buffer.
            flush()
            if is_list_item(ln):
                buf.append(ln.rstrip())
            else:
                out.append(ln.rstrip())
            continue
        # Plain text: continuation of the current paragraph/list item, or a new
        # paragraph if the buffer is empty.
        buf.append(ln.rstrip())
    flush()
    # Collapse any accidental >1 consecutive blank lines to a single blank.
    res = []
    for ln in out:
        if ln == "" and res and res[-1] == "":
            continue
        res.append(ln)
    return "\n".join(res).rstrip() + "\n"


if __name__ == "__main__":
    sys.stdout.write(reflow(sys.stdin.read()))
