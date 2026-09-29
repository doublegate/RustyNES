#!/usr/bin/env python3
# SPDX-License-Identifier: GPL-3.0-or-later
"""Write a tiny battery-backed NES ROM that counts its own power-ons.

For checking a frontend's battery-save path without a commercial game
(mobile run sheet row A1; `docs/mobile-v2.9.3-run-sheet.md`). On every
power-on the program increments `$6000`, so byte 0 of the frontend's `.sav`
reads 1, 2, 3 ... across launches if, and only if, the save is written and
loaded back. The picture is the blank backdrop; there is nothing to play.

Written from nesdev's iNES and MMC1 pages. The board is MMC1 (mapper 1) with
the battery bit set and 32 KiB of PRG, both 16 KiB banks identical, so the
code runs whichever bank is mapped at `$C000`. The program resets MMC1's
shift register and writes 0 to `$E000`, whose bit 4 clear enables PRG-RAM,
instead of relying on a power-on value.

Usage: scripts/mobile-battery-counter-rom.py OUT.nes

The output is byte-for-byte deterministic. Checked on the host core
(`rustynes_core::Nes`, three boots carrying SRAM between them: 1, 2, 3) and
on the Android emulator (2026-09-28, the same count through the app).
"""
import sys

CODE = bytes(
    [
        0x78,  # SEI
        0xD8,  # CLD
        0xA2, 0xFF, 0x9A,  # LDX #$FF ; TXS
        0xA9, 0x80, 0x8D, 0x00, 0x80,  # LDA #$80 ; STA $8000 (reset the shift register)
        0xA9, 0x00,  # LDA #$00
    ]
    + [0x8D, 0x00, 0xE0] * 5  # STA $E000 x5: PRG bank register 0, PRG-RAM enabled
    + [
        0xEE, 0x00, 0x60,  # INC $6000 (the counter)
        0x4C, 0x00, 0x00,  # JMP to itself (address patched below)
    ]
)


def build() -> bytes:
    bank = bytearray([0xFF] * 0x4000)
    bank[: len(CODE)] = CODE
    loop = 0xC000 + len(CODE) - 3
    bank[len(CODE) - 2] = loop & 0xFF
    bank[len(CODE) - 1] = loop >> 8
    # NMI, RESET and IRQ all point at $C000; neither interrupt is enabled.
    for vector in (0x3FFA, 0x3FFC, 0x3FFE):
        bank[vector] = 0x00
        bank[vector + 1] = 0xC0
    # iNES: 2 x 16 KiB PRG, CHR-RAM, flags 6 = mapper 1 low nibble + battery.
    header = bytes([0x4E, 0x45, 0x53, 0x1A, 2, 0, 0x12, 0x00]) + bytes(8)
    return header + bytes(bank) + bytes(bank)


def main() -> int:
    if len(sys.argv) != 2:
        print(__doc__.strip().splitlines()[0], file=sys.stderr)
        print("usage: mobile-battery-counter-rom.py OUT.nes", file=sys.stderr)
        return 2
    with open(sys.argv[1], "wb") as out:
        out.write(build())
    return 0


if __name__ == "__main__":
    sys.exit(main())
