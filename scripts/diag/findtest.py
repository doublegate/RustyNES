import sys
def load(p):
    r=[]
    for l in open(p).read().splitlines()[1:]:
        x=l.split(',')
        if len(x)<5: continue
        r.append((int(x[0]),int(x[1]),int(x[2]),int(x[3]),int(x[4])))  # cyc,mc,sl,dot,val
    return r
rows=load(sys.argv[1])
out=[]
out.append("rows=%d"%len(rows))
# The 4 exact-timed reads (ReadFrom2002WithExactTiming) are ~29550 cyc apart and
# each ends on a slightly later dot. Find reads whose FORWARD gap to next is in
# 29000..30500 — those are the isolated measurement reads (surrounded by a big
# clockslide). Also catch the LAST one (no big forward gap) by also taking reads
# whose BACKWARD gap is in that range.
iso=set()
for i in range(len(rows)):
    fg = rows[i+1][0]-rows[i][0] if i+1<len(rows) else 0
    bg = rows[i][0]-rows[i-1][0] if i>0 else 0
    if 29000<=fg<=30500 or 29000<=bg<=30500:
        iso.add(i)
out.append("isolated (exact-timed) reads: %d"%len(iso))
for i in sorted(iso):
    r=rows[i]
    out.append("  cyc=%d mc=%d sl=%d dot=%d val=0x%02X masked0xE0=0x%02X (V%d S%d O%d)"%(
        r[0],r[1],r[2],r[3],r[4],r[4]&0xE0,(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1))
open(sys.argv[2],'w').write("\n".join(out)+"\n")
print("wrote",len(out),"lines")
