def load(p):
    rows=[]
    with open(p) as f:
        next(f)
        for line in f:
            x=line.strip().split(',')
            if len(x)<5: continue
            rows.append((int(x[0]),int(x[1]),int(x[2]),int(x[3]),int(x[4])))
    return rows
leg=load('/tmp/RustyNES_v2/leg_win.csv')
r4=load('/tmp/RustyNES_v2/r4_win.csv')
out=[]
out.append("legacy reads=%d  r4 reads=%d"%(len(leg),len(r4)))

# The 4 measurement reads: ReadFrom2002WithExactTiming, consecutive reads ~? apart,
# each landing one dot later, near pre-render line (sl 261) crossing dot 1.
# Find the LAST ~12 reads before each build's transition cycle (the measurement+check).
def tail_before(rows, tcyc, n=16):
    pre=[r for r in rows if r[0] < tcyc]
    return pre[-n:]

legt=tail_before(leg, 105778724)
r4t =tail_before(r4, 104530002)
out.append("")
out.append("=== legacy: last 16 reads before transition 105778724 ===")
for r in legt:
    out.append("  cyc=%d sl=%d dot=%d val=0x%02X (V%d S%d O%d)"%(r[0],r[2],r[3],r[4],(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1))
out.append("")
out.append("=== r4: last 16 reads before transition 104530002 ===")
for r in r4t:
    out.append("  cyc=%d sl=%d dot=%d val=0x%02X (V%d S%d O%d)"%(r[0],r[2],r[3],r[4],(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1))

# Find the measurement-read group: reads on sl 261 (pre-render) with V/S/O bits,
# consecutive in trace, stepping dot. Show ALL sl-261 reads near each transition.
def pr_reads(rows, tcyc, lo=20000):
    return [r for r in rows if r[2]==261 and tcyc-lo<=r[0]<=tcyc]
out.append("")
out.append("=== legacy sl-261 reads within 20k cyc before transition ===")
for r in pr_reads(leg,105778724):
    out.append("  cyc=%d dot=%d val=0x%02X (V%d S%d O%d)"%(r[0],r[3],r[4],(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1))
out.append("=== r4 sl-261 reads within 20k cyc before transition ===")
for r in pr_reads(r4,104530002):
    out.append("  cyc=%d dot=%d val=0x%02X (V%d S%d O%d)"%(r[0],r[3],r[4],(r[4]>>7)&1,(r[4]>>6)&1,(r[4]>>5)&1))
open('/tmp/RustyNES_v2/align_out.txt','w').write("\n".join(out)+"\n")
print("done leg=%d r4=%d"%(len(leg),len(r4)))
