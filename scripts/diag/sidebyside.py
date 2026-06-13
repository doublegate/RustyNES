import csv
mes=[]
with open('m_cell_walk.csv') as f:
    r=csv.reader(f); next(r)
    for row in r:
        if len(row)<4: continue
        mes.append((int(row[0]),row[1],int(row[2],16),int(row[3],16)))
rny=[]
with open('rusty_fulldump.csv') as f:
    r=csv.reader(f); next(r)
    for row in r:
        rny.append((int(row[0]),row[4],int(row[5].lstrip('$'),16),int(row[6].lstrip('$'),16),row[9]))
mi=next(i for i,x in enumerate(mes) if x[1]=='W' and x[2]==0x4010 and x[3]==0x4E)
ri=next(i for i,x in enumerate(rny) if x[1]=='W' and x[2]==0x4010 and x[3]==0x4E)
ml=mes[mi:]; rl=rny[ri:]
print(f"{'off':>4} | {'MESEN':>16} | {'RUSTYNES':>18}")
for off in range(34,52):
    mc,mk,ma,mv=ml[off]
    rc,racc,ra,rd,ind=rl[off]
    mtag=f"{mk} {ma:04X}={mv:02X}"
    rtag=f"{racc} {ra:04X}={rd:02X}" + (" DMA" if ind=='1' else "")
    flag="  <-- DIFF" if (ma!=ra) else ""
    print(f"{off:>4} | {mtag:>16} | {rtag:>18}{flag}")
