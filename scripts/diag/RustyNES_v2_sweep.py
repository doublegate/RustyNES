import sys,collections
rows=[]
with open('/tmp/RustyNES_sweepdma.csv') as f:
    f.readline()
    for l in f:
        p=l.strip().split(',')
        if len(p)<4: continue
        try: cyc=int(p[0]); kind=int(p[2]); held=int(p[3],16)
        except: continue
        rows.append((cyc,kind,held))
# halts = kind 0
halts=[(c,h) for c,k,h in rows if k==0]
print("total halts captured:",len(halts))
# held_addr histogram
hist=collections.Counter(h for c,h in halts)
print("halt held_addr histogram (top12):")
for a,n in hist.most_common(12): print(f"  ${a:04X}: {n}")
# any $4000-page halts?
n4000=sum(1 for c,h in halts if 0x4000<=h<=0x401f)
print(f"halts on $4000-$401F: {n4000}")
# SWEEP: consecutive halt held_addr sequence (first 40) + cpu_cycle delta
print("first 40 consecutive halts (cyc_delta, held_addr):")
prev=None
for c,h in halts[:40]:
    d = c-prev if prev else 0
    prev=c
    print(f"  +{d:4d}  ${h:04X}")
# period distribution
deltas=collections.Counter()
prev=None
for c,h in halts:
    if prev: deltas[c-prev]+=1
    prev=c
print("halt-to-halt cpu_cycle period (top8):", deltas.most_common(8))
