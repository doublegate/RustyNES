import csv, sys
def load(fn):
    rows=[]
    with open(fn) as f:
        r=csv.reader(f); next(r)
        for row in r:
            if len(row)<16: continue
            rows.append((int(row[0]),row[4],row[5],row[6],row[9],row[12]))  # cyc,acc,addr,data,in_dmc,put
    return rows
de=load('def_abort.csv'); r1=load('r1_abort.csv')
# landmark: the $4010=$0E immediately preceding the target $0500 write
def last_lm(rows, before_cyc):
    cand=[i for i in range(len(rows)) if rows[i][1]=='W' and rows[i][2]=='$4010' and rows[i][3]=='$0E' and rows[i][0]<before_cyc]
    return cand[-1] if cand else None
di=last_lm(de, 48014349); ri=last_lm(r1, 48759427)
print(f"default lm idx={di} cyc={de[di][0]}; r1 lm idx={ri} cyc={r1[ri][0]}")
# CPU accesses only (R/W)
def cpu(rows,start): return [rows[i] for i in range(start,len(rows)) if rows[i][1] in ('R','W')]
dc=cpu(de,di); rc=cpu(r1,ri)
base=rc[0][0]-dc[0][0]
print(f"base delta={base}; walking for first (addr,data,acc) divergence:")
n=min(len(dc),len(rc)); shown=0
for k in range(n):
    if (dc[k][2],dc[k][3],dc[k][1])!=(rc[k][2],rc[k][3],rc[k][1]):
        print(f"  k={k}: default {dc[k][1]} {dc[k][2]}={dc[k][3]} (cyc{dc[k][0]}) | r1 {rc[k][1]} {rc[k][2]}={rc[k][3]} (cyc{rc[k][0]})")
        shown+=1
        if shown>=14: break
print(f"first divergence shown; {shown} lines")
