# AnswerKey1 from AccuracyCoin.asm (341 bytes)
key_txt = """
7F FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF
FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF
FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF
FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF
FF FF FF FB FB F7 F7 F3 F3 EF EF EB EB E7 E7 E3
E3 DF DF DB DB D7 D7 D3 D3 CF CF CB CB C7 C7 C3
C3 BF BF BB BB B7 B7 B3 B3 AF AF AB AB A7 A7 A3
A3 9F 9F 9B 9B 97 97 93 93 8F 8F 8B 8B 87 87 83
83 7F 7F 7E 7E 61 61 7C 7C 7B 7B 7A 7A 61 61 78
78 77 77 73 73 6F 6F 6B 6B 67 67 63 63 5F 5F 5B
5B 57 57 53 53 4F 4F 4B 4B 47 47 43 43 3F 3F 3B
3B 37 37 33 33 2F 2F 2B 2B 27 27 23 23 1F 1F 1B
1B 17 17 13 13 0F 0F 0B 0B 07 07 03 03 FF 03 FB
03 F7 03 F3 03 EF 03 EB 03 E7 03 E3 03 DF 03 DB
03 D7 03 D3 03 CF 03 CB 03 C7 03 C3 03 BF 03 BB
03 B7 03 B3 03 AF 03 AB 03 A7 03 A3 03 9F 03 9B
03 7F 7E 61 7C 7C 7C 7C 7C 7B 7A 61 78 78 78 78
78 03 FF FF FF FF FF FF FF FF FF FF FF FF FF FF
FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF
FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF FF
FF 7F 7F 7F 7F 7F 7F 7F 7F 7F 7F 7F 7F 7F 7F 7F
7F 7F 7F 7F 7F
"""
key = [int(x,16) for x in key_txt.split()]
assert len(key)==341, len(key)

# Read trace: dot -> val for scanline 128
dot2val = {}
with open("reg_2004stress.csv") as f:
    next(f)
    for line in f:
        p = line.rstrip("\n").split(",")
        if len(p)<8: continue
        if p[2]!="0x2004": continue
        if p[6]!="128": continue
        dot = int(p[7]); val = int(p[3],16)
        # the read END dot = p7; AccuracyCoin indexes by the dot the read ENDS on, slot 0..340
        dot2val[dot]=val

mism=[]
for d in range(341):
    rv = dot2val.get(d, None)
    if rv is None: continue
    if rv != key[d]:
        mism.append((d, key[d], rv))
print(f"captured {len(dot2val)}/341 dots; {len(mism)} mismatches")
for d,k,r in mism:
    print(f"  dot {d:3d}: key={k:02X} rusty={r:02X}")
