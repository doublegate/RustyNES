def load(p):
    rows={}
    order=[]
    with open(p) as f:
        next(f)
        for line in f:
            x=line.strip().split(',')
            if len(x)<5: continue
            # cpu_cycle,master_clock,scanline,dot,value,(vbl,sprite0,overflow)
            cyc=int(x[0]); mc=int(x[1]); sl=int(x[2]); dot=int(x[3]); val=int(x[4])
            rows[cyc]=(mc,sl,dot,val)
            order.append(cyc)
    return rows,order

leg,lo=load('/tmp/RustyNES/leg_2002win.csv')
r4,ro=load('/tmp/RustyNES/r4_2002win.csv')

out=[]
out.append("legacy reads=%d  r4 reads=%d"%(len(leg),len(r4)))
# Diff by cpu_cycle
common=[c for c in lo if c in r4]
out.append("common cpu_cycles=%d"%len(common))
diffs=[]
for c in common:
    lmc,lsl,ldot,lval=leg[c]
    rmc,rsl,rdot,rval=r4[c]
    if (lsl,ldot,lval)!=(rsl,rdot,rval) or lmc!=rmc:
        diffs.append((c,(lmc,lsl,ldot,lval),(rmc,rsl,rdot,rval)))
out.append("rows differing (sl/dot/val/mc): %d"%len(diffs))
for c,l,r in diffs[:60]:
    out.append("  cyc=%d  L mc=%d sl=%d dot=%d val=0x%02X | R mc=%d sl=%d dot=%d val=0x%02X"%(
        c,l[0],l[1],l[2],l[3],r[0],r[1],r[2],r[3]))
# cpu_cycles only in one
onlyL=[c for c in lo if c not in r4]
onlyR=[c for c in ro if c not in leg]
out.append("cpu_cycles only in legacy: %d  only in r4: %d"%(len(onlyL),len(onlyR)))
for c in onlyL[:10]: out.append("  onlyL cyc=%d %s"%(c,leg[c]))
for c in onlyR[:10]: out.append("  onlyR cyc=%d %s"%(c,r4[c]))
# Show value-only diffs (the actual flag-timing failure)
valdiffs=[(c,l,r) for c,l,r in diffs if l[3]!=r[3]]
out.append("--- VALUE diffs (val differs): %d ---"%len(valdiffs))
for c,l,r in valdiffs[:60]:
    out.append("  cyc=%d sl(L=%d,R=%d) dot(L=%d,R=%d) val L=0x%02X R=0x%02X"%(
        c,l[1],r[1],l[2],r[2],l[3],r[3]))
open('/tmp/RustyNES/diff2002_out.txt','w').write("\n".join(out)+"\n")
print("done diffs=%d valdiffs=%d"%(len(diffs),len(valdiffs)))
