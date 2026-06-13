#!/usr/bin/env bash
# check-mapper.sh — verify an iNES file's mapper number
# Usage: check-mapper.sh <rom.nes>
# Output: "<mapper>\t<submapper>\t<ines2>\t<file>"
set -euo pipefail
f="$1"
if [[ ! -f "$f" ]]; then
  echo "MISSING\t-\t-\t$f"
  exit 1
fi
# Read header bytes 6, 7, 8
b6=$(dd if="$f" bs=1 skip=6 count=1 2>/dev/null | od -An -tu1 | tr -d ' \n')
b7=$(dd if="$f" bs=1 skip=7 count=1 2>/dev/null | od -An -tu1 | tr -d ' \n')
b8=$(dd if="$f" bs=1 skip=8 count=1 2>/dev/null | od -An -tu1 | tr -d ' \n')
magic=$(dd if="$f" bs=1 count=4 2>/dev/null | od -An -tx1 | tr -d ' \n')
if [[ "$magic" != "4e45531a" ]]; then
  echo "BAD-MAGIC\t-\t-\t$f"
  exit 1
fi
mapper_lo=$(( b6 >> 4 ))
mapper_hi=$(( b7 & 0xF0 ))
mapper=$(( mapper_hi | mapper_lo ))
# iNES 2.0 detection
ines2_flag=$(( (b7 >> 2) & 0x03 ))
if [[ "$ines2_flag" == "2" ]]; then
  ines2="NES2.0"
  submapper=$(( b8 >> 4 ))
  mapper_ext=$(( (b8 & 0x0F) << 8 ))
  mapper=$(( mapper_ext | mapper ))
else
  ines2="iNES1"
  submapper="-"
fi
printf "%d\t%s\t%s\t%s\n" "$mapper" "$submapper" "$ines2" "$f"
