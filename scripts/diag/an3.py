rows=[]
with open('/tmp/RustyNES_v2/s2002.csv') as f:
    next(f)
    for line in f:
        p=line.strip().split(',')
        if len(p)<5: continue
        rows.append((int(p[0]),int(p[1]),int(p[2]),int(p[3]),int(p[4])))
out=[]
# The 4 test reads = reads on prerender (sl 261) with VBL(0x80) set.
pr=[r for r in rows if r[2]==261 and (r[4]&0x80)]
out.append("prerender(261) reads with VBL set (candidate test reads): %d"%len(pr))
for r in pr:
    out.append("  cyc=%d mc=%d sl=%d dot=%d val=0x%02X (V=%d S=%d O=%d)"%(
        r[0],r[1],r[2],r[3],r[4],(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1))
# ALL prerender reads (any val)
out.append("---ALL prerender(261) reads: %d---"%sum(1 for r in rows if r[2]==261))
for r in [r for r in rows if r[2]==261][:40]:
    out.append("  cyc=%d dot=%d val=0x%02X (V=%d S=%d O=%d)"%(
        r[0],r[3],r[4],(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1))
# Does sprite-0-hit EVER get set anywhere? scan whole list bit6.
any6=[r for r in rows if r[4]&0x40]
out.append("TOTAL reads with bit6(sprite0) set across entire run: %d"%len(any6))
# Does overflow get set? bit5
any5=[r for r in rows if r[4]&0x20]
out.append("TOTAL reads with bit5(overflow) set: %d"%len(any5))
import collections
out.append("  overflow-set reads by scanline: "+str(dict(collections.Counter(r[2] for r in any5))))
open('/tmp/RustyNES_v2/an3_out.txt','w').write("\n".join(out)+"\n")
print("OK")
