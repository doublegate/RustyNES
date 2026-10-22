#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""The reference reader of RustyNES's memory-export interface (layout v1).

Written from docs/memory-export-interface.md ALONE, as a third-party
splitter would be: every offset below is copied from that page's table, and
nothing here imports, parses or depends on RustyNES's source. It is the
reader half of T-SPEEDRUN-MEMIF's gate (scripts/memif/two_build_check.sh runs
it, unchanged, against two different builds).

Standard library only (multiprocessing.shared_memory, which opens POSIX
shared memory on Linux and macOS and named file mappings on Windows).

Usage:
  reference_splitter.py [--name rustynes-memif] [--timeout 30] [--once]

It waits for the block, then follows it until the timer reports "finished",
and prints one JSON document: the splits it saw, read from the block's split
table, plus the frame and master clock. --once prints the current state and
exits. Exit status: 0 finished (or --once), 2 timed out, 1 a malformed block.
"""

import argparse
import json
import os
import struct
import sys
import time
from multiprocessing import shared_memory

# --- docs/memory-export-interface.md, "Layout version 1" -------------------
MAGIC = b"RNESMEMF"
OFF_VERSION = 0x0008
OFF_RAM_OFFSET = 0x000C
OFF_LAYOUT_SIZE = 0x0010
OFF_SEQ = 0x0014
OFF_PUBLISHES = 0x0018
OFF_FRAME = 0x0020
OFF_MASTER_CLOCK = 0x0028
OFF_HZ_NUM = 0x0030
OFF_HZ_DEN = 0x0034
OFF_REGION = 0x0038
OFF_FLAGS = 0x0039
OFF_TIMER_STATE = 0x003A
OFF_SPLITS_DONE = 0x003C
OFF_SPLITS_DEFINED = 0x0040
OFF_EPOCH = 0x0044
OFF_TIMER_START = 0x0048
OFF_TIMER_ELAPSED = 0x0050
OFF_ROM_SHA256 = 0x0060
OFF_CPU_RAM = 0x0100
OFF_SPLITS = 0x2900
LAYOUT_SIZE = 0x2B00
FLAG_GAME = 1
FLAG_TIMER = 2
STATES = {0: "idle", 1: "running", 2: "finished"}
REGIONS = {0: "NTSC", 1: "PAL", 2: "Dendy"}
# ---------------------------------------------------------------------------


def os_name(name):
    """The per-user OS object name (the page's "Opening it" table)."""
    if os.name == "nt":
        return "Local\\" + name
    # Python adds the leading "/" itself.
    return f"{name}.{os.geteuid():x}"


def owned_by_me(shm):
    """POSIX: the block must be ours and owner-only, or another local user may
    have planted it (the page's threat model). Windows: Local\\ is per session."""
    if os.name == "nt":
        return True
    st = os.fstat(shm._fd)
    return st.st_uid == os.geteuid() and st.st_mode & 0o077 == 0


def open_block(name, timeout):
    name = os_name(name)
    deadline = time.monotonic() + timeout
    while True:
        try:
            try:
                return shared_memory.SharedMemory(name=name, track=False)
            except TypeError:  # Python < 3.13 has no `track`
                shm = shared_memory.SharedMemory(name=name)
                try:
                    # Keep the resource tracker from unlinking a block this
                    # process did not create when it exits.
                    from multiprocessing import resource_tracker

                    resource_tracker.unregister(shm._name, "shared_memory")
                except Exception:  # noqa: BLE001 - best effort, POSIX only
                    pass
                return shm
        except FileNotFoundError:
            if time.monotonic() > deadline:
                return None
            time.sleep(0.01)


def u32(b, at):
    return struct.unpack_from("<I", b, at)[0]


def u64(b, at):
    return struct.unpack_from("<Q", b, at)[0]


def snapshot(buf, deadline):
    """One consistent copy (the sequence-counter protocol of the page), or None
    once `deadline` passes: a producer that dies mid-write leaves the counter
    odd for good, and retrying forever would outlive --timeout."""
    while True:
        s1 = u32(buf, OFF_SEQ)
        if s1 & 1:
            if time.monotonic() > deadline:
                return None
            time.sleep(0)
            continue
        copy = bytes(buf[:LAYOUT_SIZE])
        if u32(buf, OFF_SEQ) == s1:
            return copy
        if time.monotonic() > deadline:
            return None


def decode(b):
    if b[:8] != MAGIC:
        raise ValueError(f"bad magic {b[:8]!r}")
    if u32(b, OFF_VERSION) != 1:
        raise ValueError(f"layout version {u32(b, OFF_VERSION)}, this reader knows 1")
    if u32(b, OFF_RAM_OFFSET) != OFF_CPU_RAM or u32(b, OFF_LAYOUT_SIZE) != LAYOUT_SIZE:
        raise ValueError("unexpected sizes")
    flags = b[OFF_FLAGS]
    done = u32(b, OFF_SPLITS_DONE)
    return {
        "publishes": u64(b, OFF_PUBLISHES),
        "game_loaded": bool(flags & FLAG_GAME),
        "timer_defined": bool(flags & FLAG_TIMER),
        "frame": u64(b, OFF_FRAME),
        "master_clock": u64(b, OFF_MASTER_CLOCK),
        "master_clock_hz": [u32(b, OFF_HZ_NUM), u32(b, OFF_HZ_DEN)],
        "region": REGIONS.get(b[OFF_REGION], "?"),
        "epoch": u32(b, OFF_EPOCH),
        "rom_sha256": b[OFF_ROM_SHA256 : OFF_ROM_SHA256 + 32].hex(),
        "timer_state": STATES.get(b[OFF_TIMER_STATE], "?"),
        "splits_done": done,
        "splits_defined": u32(b, OFF_SPLITS_DEFINED),
        "timer_start": u64(b, OFF_TIMER_START),
        "elapsed_cycles": u64(b, OFF_TIMER_ELAPSED),
        "split_elapsed_cycles": [u64(b, OFF_SPLITS + 8 * i) for i in range(min(done, 64))],
        "ram_0000_000f": b[OFF_CPU_RAM : OFF_CPU_RAM + 16].hex(),
    }


def main():
    ap = argparse.ArgumentParser(description=__doc__.split("\n\n")[0])
    ap.add_argument("--name", default="rustynes-memif")
    ap.add_argument("--timeout", type=float, default=30.0)
    ap.add_argument("--once", action="store_true")
    args = ap.parse_args()

    shm = open_block(args.name, args.timeout)
    if shm is None:
        print(f"no block named {args.name!r} within {args.timeout}s", file=sys.stderr)
        return 2
    try:
        if not owned_by_me(shm):
            print("refusing the block: not owned by this user with mode 0600", file=sys.stderr)
            return 1
        deadline = time.monotonic() + args.timeout
        seen_running = False
        while True:
            copy = snapshot(shm.buf, deadline)
            if copy is None:
                print("timed out: the block never held a consistent copy", file=sys.stderr)
                return 2
            try:
                state = decode(copy)
            except ValueError as e:
                print(f"malformed block: {e}", file=sys.stderr)
                return 1
            if args.once:
                print(json.dumps(state, indent=2))
                return 0
            seen_running |= state["timer_state"] == "running"
            if state["timer_state"] == "finished":
                state["saw_running"] = seen_running
                print(json.dumps(state, indent=2))
                return 0
            if time.monotonic() > deadline:
                print(json.dumps(state, indent=2))
                print("timed out before the run finished", file=sys.stderr)
                return 2
            time.sleep(0.001)
    finally:
        shm.close()


if __name__ == "__main__":
    sys.exit(main())
