import csv

# Mesen cell trace: cpu_cycle,kind,addr,value  (kinds R/W/H/D/A/G)
mes = []
with open('m_cell_walk.csv') as f:
    r = csv.reader(f); next(r)
    for row in r:
        if len(row)<4: continue
        cyc=int(row[0]); kind=row[1]; addr=int(row[2],16); val=int(row[3],16)
        mes.append((cyc,kind,addr,val))

# RustyNES full dump: cpu_cycle,frame,sl,dot,access,addr($hex),data($hex),...
# access R/W/r/w/I ; addr/data have $ prefix
rny=[]
with open('rusty_fulldump.csv') as f:
    r=csv.reader(f); next(r)
    for row in r:
        cyc=int(row[0]); acc=row[4]
        addr=int(row[5].lstrip('$'),16); data=int(row[6].lstrip('$'),16)
        indmc=row[9]
        rny.append((cyc,acc,addr,data,indmc))

# landmark: first W 4010 4E
def find_mes_landmark():
    for i,(c,k,a,v) in enumerate(mes):
        if k=='W' and a==0x4010 and v==0x4E: return i
    return None
def find_rny_landmark():
    for i,(c,acc,a,d,_) in enumerate(rny):
        if acc=='W' and a==0x4010 and d==0x4E: return i
    return None

mi=find_mes_landmark(); ri=find_rny_landmark()
print(f"Mesen landmark idx={mi} cyc={mes[mi][0]}  Rusty landmark idx={ri} cyc={rny[ri][0]}")

# Normalize kind: collapse Mesen H/D/A/G -> read 'r' (DMA), R->'R', W->'W'
def mnorm(k):
    if k in ('H','D','A','G'): return 'r'
    return k  # R or W
def rnorm(acc):
    if acc=='r': return 'r'
    if acc=='w': return 'W'  # dma write
    if acc=='I': return 'I'
    return acc  # R or W

# Walk both from landmark, report first structural divergences (addr or normalized-kind)
mlist=mes[mi:]; rlist=rny[ri:]
# Mesen has NO Idle rows; RustyNES emits I rows. To align, drop RustyNES I rows? 
# But I rows ARE cycles. Mesen logs a read every cycle (no idle). So an I in Rusty
# where Mesen has a read = the structural divergence we hunt. Keep both, align by index.
divs=0
n=min(len(mlist),len(rlist))
for off in range(n):
    mc,mk,ma,mv=mlist[off]
    rc,racc,ra,rd,indmc=rlist[off]
    mk2=mnorm(mk); rk2=rnorm(racc)
    if ma!=ra or mk2!=rk2:
        print(f"off={off:5d} | MES {mk}{('('+mk+')') if mk in 'HDAG' else ''} {ma:04X}={mv:02X} | RNY {racc} {ra:04X}={rd:02X} indmc={indmc}")
        divs+=1
        if divs>=40: 
            print("... (capped at 40)")
            break
print(f"total divergences shown (cap 40 of first {n} aligned cycles)")
