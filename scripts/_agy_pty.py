#!/usr/bin/env python3
# _agy_pty.py -- argv-safe PTY wrapper used by agy-review.sh as a fallback when
# `unbuffer` (from the `expect` package) is unavailable.
#
# Runs argv[1:] under a pseudo-terminal so a program that only flushes stdout
# when attached to a TTY (agy --print, upstream issue #76) still emits its output
# when its stdout is captured. No shell is involved -- the command is passed as an
# argv list -- so caller-provided flags cannot inject a command or be word-split.
#
# Usage: _agy_pty.py <cmd> [args...]   (child's terminal output goes to stdout)
import os
import pty
import sys

if len(sys.argv) < 2:
    sys.stderr.write("usage: _agy_pty.py <cmd> [args...]\n")
    sys.exit(2)

# pty.spawn copies the child's PTY output onto our stdout (fd 1), which the
# caller redirects to a file. Returns the child's raw wait status.
status = pty.spawn(sys.argv[1:])
sys.exit(os.waitstatus_to_exitcode(status))
