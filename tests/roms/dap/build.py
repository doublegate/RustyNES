#!/usr/bin/env python3
# SPDX-License-Identifier: CC0-1.0
"""Rebuild the T-DEBUG-DAP fixture ROMs from dapdemo.c.

    python3 tests/roms/dap/build.py [--out DIR]

Builds dapdemo.c twice:

* cc65 (``cl65 -t nes -g``) -> ``dapdemo_cc65.nes`` + ``dapdemo_cc65.dbg``
  (ld65's ``--dbgfile`` debug info);
* the llvm-mos SDK (``mos-nes-nrom-clang -O0 -g``) -> ``dapdemo_llvm_mos.nes``
  + ``dapdemo_llvm_mos.elf`` (DWARF 5 in the ELF).

Tool discovery: ``cl65`` from ``$CC65_HOME/bin`` or ``PATH``;
``mos-nes-nrom-clang`` from ``$LLVM_MOS/bin`` or ``PATH``. A missing tool is an
error, never a skip: the CI job that runs this installs both at pinned
versions (see ``.github/workflows/ci.yml``, job ``dap-fixtures``).

The build runs in a temporary directory holding only a copy of dapdemo.c,
so the debug info records the source as the relative name ``dapdemo.c``
(llvm-mos additionally gets ``-fdebug-prefix-map`` so no build path is kept)
and the working tree is never littered with compiler intermediates. Two
builds on one machine give identical ROMs and an identical ELF; the cc65
``.dbg`` differs (it records cl65's temporary file name and source
modification times). The absolute paths ld65 records for cc65's own
runtime files (where cc65 was built or installed) are rewritten to
``cc65/<dir>/...`` by ``normalize_dbg``, so no machine's home directory is
kept. The tests never compare fixture bytes; they drive whatever this script
produced.
"""

import argparse
import hashlib
import os
import re
import shutil
import subprocess
import sys
import tempfile
from pathlib import Path

HERE = Path(__file__).resolve().parent
SOURCE = HERE / "dapdemo.c"


def find_tool(name: str, home_var: str) -> str:
    home = os.environ.get(home_var)
    if home:
        candidate = Path(home) / "bin" / name
        if candidate.is_file():
            return str(candidate)
    found = shutil.which(name)
    if found:
        return found
    sys.exit(f"build.py: {name} not found (set {home_var} or put it on PATH)")


def run(cmd: list[str], cwd: Path, env: dict[str, str]) -> None:
    print("+", " ".join(cmd), flush=True)
    subprocess.run(cmd, cwd=cwd, env=env, check=True)


# A path cc65 recorded for its own runtime: where its asminc, cfg, include
# and lib directories were on the machine that built cc65 or installed it.
_CC65_PATH = re.compile(r'name="(?:[A-Za-z]:)?/[^"]*?/(asminc|cfg|include|lib|libsrc)/')


def normalize_dbg(text: str) -> str:
    """Replace the absolute paths of cc65's own files in an ld65 debug file
    with ``cc65/<dir>/...``.

    ld65 records the include and library files it read by absolute path, which
    put the building machine's home directory into a committed fixture (and
    made two machines' builds differ for no reason the tests care about). The
    debugger never opens those files: only ``dapdemo.c`` is a source it maps.
    """
    return _CC65_PATH.sub(lambda m: f'name="cc65/{m.group(1)}/', text)


def sha256(path: Path) -> str:
    return hashlib.sha256(path.read_bytes()).hexdigest()


def main() -> int:
    parser = argparse.ArgumentParser(description=__doc__.splitlines()[0])
    parser.add_argument("--out", type=Path, default=HERE, help="output directory")
    args = parser.parse_args()
    out: Path = args.out.resolve()
    out.mkdir(parents=True, exist_ok=True)

    cl65 = find_tool("cl65", "CC65_HOME")
    mos = find_tool("mos-nes-nrom-clang", "LLVM_MOS")
    env = dict(os.environ)
    # cl65 locates its include, asminc, cfg and lib directories through
    # CC65_HOME when it was not built with fixed paths; derive it from the
    # binary when the caller did not set it.
    env.setdefault("CC65_HOME", str(Path(cl65).resolve().parent.parent / "share" / "cc65"))

    with tempfile.TemporaryDirectory(prefix="rustynes-dap-") as tmp_name:
        tmp = Path(tmp_name)
        shutil.copy(SOURCE, tmp / "dapdemo.c")

        run(
            [
                cl65,
                "-t",
                "nes",
                "-g",
                "-Wl",
                "--dbgfile,dapdemo_cc65.dbg",
                "-o",
                "dapdemo_cc65.nes",
                "dapdemo.c",
            ],
            tmp,
            env,
        )
        run(
            [
                mos,
                "-O0",
                "-g",
                f"-fdebug-prefix-map={tmp}=.",
                "-o",
                "dapdemo_llvm_mos.nes",
                "dapdemo.c",
            ],
            tmp,
            env,
        )
        produced = {
            "dapdemo_cc65.nes": tmp / "dapdemo_cc65.nes",
            "dapdemo_cc65.dbg": tmp / "dapdemo_cc65.dbg",
            "dapdemo_llvm_mos.nes": tmp / "dapdemo_llvm_mos.nes",
            "dapdemo_llvm_mos.elf": tmp / "dapdemo_llvm_mos.nes.elf",
        }
        for name, path in produced.items():
            if not path.is_file():
                sys.exit(f"build.py: the build did not produce {path.name}")
            if name.endswith(".dbg"):
                (out / name).write_text(normalize_dbg(path.read_text()))
            else:
                shutil.copy(path, out / name)
            print(f"{sha256(out / name)}  {name}")
    return 0


if __name__ == "__main__":
    sys.exit(main())
