rows=[]
with open('/tmp/RustyNES/s2002.csv') as f:
    next(f)
    for line in f:
        p=line.strip().split(',')
        if len(p)<5: continue
        rows.append((int(p[0]),int(p[1]),int(p[2]),int(p[3]),int(p[4])))
out=[]
# The 4 test reads are ~29550 CPU cyc apart (ReadFrom2002WithExactTiming clockslide).
# Find reads whose NEXT read (in cyc) is 29000..30500 cyc later -> these are the
# isolated "exact timing" reads (the loop body is one big clockslide).
iso=[]
for i in range(1,len(rows)):
    gap=rows[i][0]-rows[i-1][0]
    if 29000<=gap<=30500:
        iso.append((rows[i-1],gap))
out.append("isolated reads (prev gap 29000-30500): %d"%len(iso))
for (r,g) in iso:
    out.append("  cyc=%d sl=%d dot=%d val=0x%02X (V=%d S=%d O=%d)  [+%d to next]"%(
        r[0],r[2],r[3],r[4],(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1,g))
# Also find reads followed ~29550 later (forward gap)
out.append("---reads with FORWARD gap 29000-30500 (the read itself is the exact-timed one)---")
seq=[]
for i in range(len(rows)-1):
    gap=rows[i+1][0]-rows[i][0]
    if 29000<=gap<=30500:
        r=rows[i]
        seq.append(r)
for r in seq:
    out.append("  cyc=%d sl=%d dot=%d val=0x%02X (V=%d S=%d O=%d)"%(
        r[0],r[2],r[3],r[4],(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1))
# What is our prerender line? max scanline seen:
out.append("max scanline seen: %d"%max(r[2] for r in rows))
out.append("scanlines with reads where vbl(0x80) set:")
vbl=[r for r in rows if r[4]&0x80]
import collections
c=collections.Counter(r[2] for r in vbl)
out.append("  "+str(dict(c)))
out.append("scanlines where sprite0(0x40) set:")
s0=[r for r in rows if r[4]&0x40]
c2=collections.Counter(r[2] for r in s0)
out.append("  "+str(dict(c2)))
open('/tmp/RustyNES/an2_out.txt','w').write("\n".join(out)+"\n")
print("done",len(out))
