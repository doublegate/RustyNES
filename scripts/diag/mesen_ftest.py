import collections
# Find the $2002 flag-timing test in Mesen's trace.
# The test (TEST_2002FlagTiming) does ReadFrom2002WithExactTiming 4x, each read
# from a DISTINCT pc (the LDY $2002 inside that subroutine), landing near the
# pre-render line (Mesen sl=-1) / line 0, stepping one dot each. It runs ONCE,
# late in the battery. Strategy: for each non-F91C pc, find reads whose
# scanline is in {-1,0,1} (pre-render/early) — those are the flag-timing/sprite-eval
# tests. Then within each such pc, list the value sequence.
rows=[]
with open('/tmp/RustyNES_v2/mesen_2002.csv') as f:
    f.readline()
    for line in f:
        x=line.strip().split(',')
        if len(x)<9: continue
        rows.append((int(x[0]),int(x[1]),int(x[2]),int(x[3]),x[4],int(x[5],16),int(x[6]),int(x[7]),int(x[8])))
out=[]
# reads on pre-render (-1) or line 0/1, NOT from the F91C vbl-poll
near=[r for r in rows if r[2] in (-1,0,1) and r[4]!='F91C']
out.append("non-F91C reads on sl in {-1,0,1}: %d"%len(near))
pcc=collections.Counter(r[4] for r in near)
out.append("their PCs: %s"%pcc.most_common(15))
# The exact-timed flag CLEAR test: 4 reads stepping dots, values masked 0xE0
# Show the per-pc value sequences for the candidate PCs (low count, sl -1/0)
for pc,_ in pcc.most_common(8):
    rs=[r for r in near if r[4]==pc]
    out.append("--- pc=%s (%d reads) ---"%(pc,len(rs)))
    for r in rs[:12]:
        out.append("  cyc=%d sl=%d dot=%d val=0x%02X E0=0x%02X (V%d S%d O%d)"%(
            r[0],r[2],r[3],r[5],r[5]&0xE0,r[6],r[7],r[8]))
open('/tmp/RustyNES_v2/mesen_ftest_out.txt','w').write("\n".join(out)+"\n")
print("done; near=%d"%len(near))
