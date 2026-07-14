#!/usr/bin/env python3
"""Verify the zlib crc32_combine identity for RustyNES Game Genie re-keying.

Identity under test:
    crc32(A ++ B) == crc32_combine(crc32(A), crc32(B), len(B))
Applied to NES ROMs:
    rom_crc32 (header-excluded) == crc32(PRG ++ CHR)
                                == crc32_combine(prgCRC, chrCRC, chrLen)
    and when CHR len == 0 (CHR-RAM): rom_crc32 == prgCRC
"""
import zlib

# ---- Pure-Python crc32_combine (matches zlib's algorithm) ----------------
GF2_DIM = 32

def gf2_matrix_times(mat, vec):
    summ = 0
    i = 0
    while vec:
        if vec & 1:
            summ ^= mat[i]
        vec >>= 1
        i += 1
    return summ

def gf2_matrix_square(square, mat):
    for n in range(GF2_DIM):
        square[n] = gf2_matrix_times(mat, mat[n])

def crc32_combine(crc1, crc2, len2):
    if len2 == 0:
        return crc1
    even = [0] * GF2_DIM
    odd = [0] * GF2_DIM
    # put operator for one zero bit in odd
    odd[0] = 0xEDB88320  # CRC-32 polynomial
    row = 1
    for n in range(1, GF2_DIM):
        odd[n] = row
        row <<= 1
    gf2_matrix_square(even, odd)   # even = odd^2 -> operator for 2 zero bits
    gf2_matrix_square(odd, even)   # odd = even^2 -> operator for 4 zero bits
    while True:
        gf2_matrix_square(even, odd)
        if len2 & 1:
            crc1 = gf2_matrix_times(even, crc1)
        len2 >>= 1
        if len2 == 0:
            break
        gf2_matrix_square(odd, even)
        if len2 & 1:
            crc1 = gf2_matrix_times(odd, crc1)
        len2 >>= 1
        if len2 == 0:
            break
    crc1 ^= crc2
    return crc1

# ---- Sanity: synthetic proof against zlib -------------------------------
def selftest():
    import os
    ok = True
    for trial in range(2000):
        la = (trial * 37) % 5000
        lb = (trial * 101 + 13) % 5000
        A = os.urandom(la)
        B = os.urandom(lb)
        expect = zlib.crc32(A + B) & 0xFFFFFFFF
        ca = zlib.crc32(A) & 0xFFFFFFFF
        cb = zlib.crc32(B) & 0xFFFFFFFF
        got = crc32_combine(ca, cb, len(B)) & 0xFFFFFFFF
        if got != expect:
            print(f"MISMATCH la={la} lb={lb} got={got:08X} exp={expect:08X}")
            ok = False
            break
    print("SYNTHETIC crc32_combine self-test:", "PASS (2000 random trials)" if ok else "FAIL")
    return ok

if __name__ == "__main__":
    selftest()
