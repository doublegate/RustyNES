#!/usr/bin/env python3
"""Per-index $2007 Stress evaluator.

Compares the STRESS2007 dump (the 341 per-dot bytes the test writes to
$0500-$0654, one per clockslide iteration / landing dot) against the
170-byte stable-(odd-Y)-dot answer key at scripts/diag/key2007.txt
(extracted from tests/roms/AccuracyCoin/sub-tests/ppu-misc-2007-stress.nes
at file offset 0x1B5A; the test only checks the odd-Y dots).

Usage: eval2007.py <dump-file> [-v]
  <dump-file>: a scan_dma_abort stdout capture containing a `STRESS2007=`
               hex line, or a file of whitespace-separated hex bytes
               (the last 341 tokens are used).
  -v: print the per-index mismatch map (index, got, want, landing Y dot).
"""
import os
import sys

HERE = os.path.dirname(os.path.abspath(__file__))
key = [int(x, 16) for x in open(os.path.join(HERE, "key2007.txt")).read().split()]

txt = open(sys.argv[1]).read()
data = None
for line in txt.splitlines():
    if "STRESS2007=" in line:
        h = line.split("STRESS2007=")[1].strip()
        if h and h != "(unset)":
            data = list(bytes.fromhex(h))
        break
if data is None:
    data = [int(x, 16) for x in txt.split()[-341:]]
if len(data) < 341:
    print("SHORT", len(data))
    sys.exit(1)

# The test tolerates a 1-dot rotation; align the same way upstream does.
if data[3] != 0xC0 and data[2] == 0xC0:
    data = [data[0x154]] + data[:-1]

verbose = "-v" in sys.argv[2:]
X = 0
Y = 0
match = 0
total = 0
while X < 0xAA:
    if Y & 1:
        total += 1
        if data[Y] == key[X]:
            match += 1
        elif verbose:
            print(f"  idx {X:3} (Y dot {Y:3}): got {data[Y]:02X} want {key[X]:02X}")
        X += 1
    Y += 1
print(f"stable matches: {match}/{total} = {100 * match // max(total, 1)}%  (data[3]={data[3]:02X})")
