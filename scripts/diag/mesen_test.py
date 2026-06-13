import collections
# Mesen schema: cpu_cycle,mc,scanline,cycle(dot),pc,value,vbl,sprite0,overflow
rows=[]
with open('/tmp/RustyNES_v2/mesen_2002.csv') as f:
    h=f.readline()
    for line in f:
        x=line.strip().split(',')
        if len(x)<9: continue
        cyc=int(x[0]); mc=int(x[1]); sl=int(x[2]); dot=int(x[3])
        pc=x[4]; val=int(x[5],16); vbl=int(x[6]); s0=int(x[7]); ov=int(x[8])
        rows.append((cyc,mc,sl,dot,pc,val,vbl,s0,ov))
out=[]
out.append("mesen $2002 reads: %d"%len(rows))
out.append("header was: %s"%h.strip())
# PC histogram — the flag-timing test reads come from one/few PCs (ReadFrom2002WithExactTiming)
pcc=collections.Counter(r[4] for r in rows)
out.append("top PCs by count: %s"%pcc.most_common(12))
# The flag-timing test: reads where the value has sprite bits (0x60) AND vbl, near a scanline boundary.
# Find reads with sprite0 set on the pre-render-ish lines.
spr=[r for r in rows if r[7]==1]  # sprite0 set
out.append("reads with sprite0 set: %d"%len(spr))
# their scanline distribution
slc=collections.Counter(r[2] for r in spr)
out.append("sprite0-set reads by scanline (top): %s"%slc.most_common(10))
open('/tmp/RustyNES_v2/mesen_test_out.txt','w').write("\n".join(out)+"\n")
print("done")
