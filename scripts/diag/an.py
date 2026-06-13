import collections
rows=[]
with open('/tmp/RustyNES_v2/s2002.csv') as f:
    h=next(f)
    for line in f:
        p=line.strip().split(',')
        if len(p)<5: continue
        # cpu_cycle,master_clock,scanline,dot,value,vbl,sprite0,overflow
        cyc=int(p[0]); mc=int(p[1]); sl=int(p[2]); dot=int(p[3]); val=int(p[4])
        rows.append((cyc,mc,sl,dot,val))
out=[]
out.append("total %d"%len(rows))
# value histogram
vh=collections.Counter(r[4] for r in rows)
out.append("val hist: "+", ".join("0x%02X:%d"%(v,vh[v]) for v in sorted(vh)))
# The 4 test reads: value&0xE0 varies, on prerender line. prerender NTSC=261.
# Find runs of 4 reads where successive cpu_cycle gaps are equal-ish AND dot steps +1,
# and at least one has sprite bits. Simpler: print all reads on sl 261 or 260 with their val.
near=[r for r in rows if r[2] in (260,261,-1,0)]
out.append("reads on sl in {260,261,-1,0}: %d"%len(near))
# group: print reads whose val has 0x60 OR 0x80 set AND sl near boundary
flag=[r for r in near if r[4]&0xE0]
out.append("...with any VSO bit: %d"%len(flag))
for r in flag[:60]:
    out.append("  cyc=%d mc=%d sl=%d dot=%d val=0x%02X (V=%d S=%d O=%d)"%(
        r[0],r[1],r[2],r[3],r[4],(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1))
# Also: find the 4-read test pattern = consecutive reads (in trace order) where AND 0xE0
# transitions E0->E0->80->00 or similar. Scan windows of 4 with strictly stepping dot.
out.append("---scanning for E0/80/00 transition windows (masked 0xE0)---")
for i in range(len(rows)-3):
    w=rows[i:i+4]
    masks=[r[4]&0xE0 for r in w]
    # test signature: starts 0xE0, ends 0x00, monotonic non-increasing, on prerender-ish
    if masks[0]==0xE0 and masks[3]==0x00 and masks[0]>=masks[1]>=masks[2]>=masks[3]:
        sls=[r[2] for r in w]; dots=[r[3] for r in w]
        out.append("  win@%d sls=%s dots=%s masks=%s"%(i,sls,dots,[hex(m) for m in masks]))
open('/tmp/RustyNES_v2/an_out.txt','w').write("\n".join(out)+"\n")
print("done",len(out))
