data_hex="00030303C0C01C0606060404C1C16C1C1C1C0505C1C17C6C6C6C0606C1C17C7C7C7C0707C1C10C7C7C7C0808C2C23C0C0C0C0909C2C2663C3C3C0A0AC2C2666666660B0BC2C27C6666660C0CC3C3607C7C7C0D0DC3C3666060600E0EC3C37C6666660F0FC3C37C7C7C7C1010C4C4607C7C7C1111C4C47E6060601212C4C4187E7E7E1313C4C40C1818181414C5C5780C0C0C1515C5C5607878781616C5C57E6060601717C5C57A7E7E7E1818C6C6667A7A7A1919C6C6666666661A1AC6C6666666661B1BC6C6666666661C1CC7C7606666661D1DC7C7186060601E1EC7C7661818181F1FC7C7666666660000C0C0666666660101C0C0386666660202C0C0463838383838383C3C3C3C3C3C3C3C6666666666666666666666666666666666666666667E7E7E7E7E7E7E7E3C3C3C3C3C3C3C3C7C7C7C7C7C7C7C7C3C3C3C3C3C3C3C3C0101C0C0186666660202C0C006181803030303"
data=[int(data_hex[i:i+2],16) for i in range(0,len(data_hex),2)]
# reconstruct fetch_bus[d] = data[d-6] (offset 6 used in dump)
# so to sample at offset o for read at dot D: val = data[D + o - 6]
fullkey=[0x02,0xC0,0x46,0x46,0x03,0xC0,0x06,0x06, 0x04,0xC1,0x6C,0x6C,0x05,0xC1,0x60,0x60,
0x06,0xC1,0x60,0x60,0x07,0xC1,0x06,0x06, 0x08,0xC2,0x66,0x66,0x09,0xC2,0x66,0x66,
0x0A,0xC2,0x24,0x24,0x0B,0xC2,0x66,0x66, 0x0C,0xC3,0x66,0x66,0x0D,0xC3,0x64,0x64,
0x0E,0xC3,0x60,0x60,0x0F,0xC3,0x60,0x60, 0x10,0xC4,0x66,0x66,0x11,0xC4,0x66,0x66,
0x12,0xC4,0x18,0x18,0x13,0xC4,0x0C,0x0C, 0x14,0xC5,0x6C,0x6C,0x15,0xC5,0x60,0x60,
0x16,0xC5,0x76,0x76,0x17,0xC5,0x72,0x72, 0x18,0xC6,0x66,0x66,0x19,0xC6,0x7C,0x7C,
0x1A,0xC6,0x3C,0x3C,0x1B,0xC6,0x7C,0x7C]
# key index j -> read dot = 1+2j. phase = (1+2j)&7 in {1,3,5,7}
# For each phase find offset maximizing matches over its j's.
from collections import defaultdict
byphase=defaultdict(list)
for j in range(len(fullkey)):
    d=1+2*j
    byphase[d&7].append((j,d))
def val(D,o):
    idx=D+o-6
    return data[idx] if 0<=idx<len(data) else None
for ph in sorted(byphase):
    best=None
    for o in range(-4,12):
        m=sum(1 for (j,d) in byphase[ph] if val(d,o)==fullkey[j])
        if best is None or m>best[1]: best=(o,m)
    print(f"phase {ph}: best offset {best[0]:+d} -> {best[1]}/{len(byphase[ph])}")
# total with per-phase best
tot=0;mt=0
bestoff={}
for ph in sorted(byphase):
    b=max(range(-4,12),key=lambda o:sum(1 for (j,d) in byphase[ph] if val(d,o)==fullkey[j]))
    bestoff[ph]=b
for j in range(len(fullkey)):
    d=1+2*j; o=bestoff[d&7]; tot+=1
    if val(d,o)==fullkey[j]: mt+=1
print("per-phase-best total:",mt,"/",tot, "offsets",bestoff)

print("\n-- reconstructed fetch_bus per dot, dots 0..40 --")
def fb(d):
    idx=d-6
    return data[idx] if 0<=idx<len(data) else None
for d in range(0,40):
    print(d, f"{fb(d):02X}" if fb(d) is not None else "--", end="   ")
    if d%8==7: print()
print("\n-- key first 16 (NT,AT,PL,PH per tile) --", [f"{k:02X}" for k in fullkey[:16]])
