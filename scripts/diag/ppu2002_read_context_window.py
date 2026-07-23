def load(p):
    rows=[]
    with open(p) as f:
        next(f)
        for line in f:
            x=line.strip().split(',')
            if len(x)<5: continue
            rows.append((int(x[0]),int(x[1]),int(x[2]),int(x[3]),int(x[4])))
    return rows
leg=load('/tmp/RustyNES/leg_2002win.csv')
r4=load('/tmp/RustyNES/r4_2002win.csv')
lm={r[0]:r for r in leg}
rm={r[0]:r for r in r4}
out=[]
# Context around the 2 differing reads: 2367000..2367400
out.append("=== reads in 2366900..2367400 (around the divergence) ===")
out.append("cyc        | L: sl dot val      | R: sl dot val")
for c in sorted(set(lm)|set(rm)):
    if 2366900<=c<=2367400:
        l=lm.get(c); r=rm.get(c)
        ls="sl%d d%d 0x%02X"%(l[2],l[3],l[4]) if l else "----"
        rs="sl%d d%d 0x%02X"%(r[2],r[3],r[4]) if r else "----"
        mark=" <<<" if l and r and (l[2],l[3])!=(r[2],r[3]) else ""
        out.append("%d | %-18s | %-18s%s"%(c,ls,rs,mark))
# The last 25 reads before the result write (2390748) — the measurement reads + checks
out.append("")
out.append("=== last 30 reads before 2390748 (measurement + answer-check) ===")
tail=[r for r in leg if r[0]<=2390748][-30:]
for l in tail:
    c=l[0]; r=rm.get(c)
    rs="sl%d d%d 0x%02X"%(r[2],r[3],r[4]) if r else "MISSING"
    out.append("cyc=%d L sl%d d%d 0x%02X | R %s%s"%(c,l[2],l[3],l[4],rs," <<<" if r and (l[2],l[3],l[4])!=(r[2],r[3],r[4]) else ""))
# Find the 4 measurement reads: reads ~29781 apart. Look for reads where consecutive gap ~29781
out.append("")
out.append("=== reads with gap-to-next in 29000..30500 (exact-timed measurement reads) ===")
for i in range(len(leg)-1):
    g=leg[i+1][0]-leg[i][0]
    if 29000<=g<=30500:
        c=leg[i][0]; l=leg[i]; r=rm.get(c)
        rs="sl%d d%d 0x%02X"%(r[2],r[3],r[4]) if r else "MISS"
        out.append("cyc=%d L sl%d d%d 0x%02X | R %s%s"%(c,l[2],l[3],l[4],rs," <<<DIFF" if r and (l[2],l[3],l[4])!=(r[2],r[3],r[4]) else ""))
open('/tmp/RustyNES/ctx2002_out.txt','w').write("\n".join(out)+"\n")
print("done")
