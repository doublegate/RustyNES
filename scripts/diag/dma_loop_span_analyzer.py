import sys, collections
rows=[]
with open('/tmp/RustyNES_dmaloop_seed1.csv') as f:
    hdr=f.readline()
    for l in f:
        p=l.strip().split(',')
        if len(p)<3: continue
        try: cyc=int(p[0]); kind=int(p[2])
        except: continue
        held=p[3] if len(p)>3 else ''
        rows.append((cyc,kind,held))
# DMC span: from a halt(0) to the next get(1)
spans=collections.Counter()
periods=collections.Counter()
get_cyc=[]
last_halt=None
held_at_get=collections.Counter()
for cyc,kind,held in rows:
    if kind==0:
        last_halt=cyc
    elif kind==1:  # DMC get
        if last_halt is not None:
            spans[cyc-last_halt+1]+=1  # inclusive cycle count
            last_halt=None
        get_cyc.append(cyc)
        held_at_get[held]+=1
for a,b in zip(get_cyc,get_cyc[1:]):
    periods[b-a]+=1
print("total rows:",len(rows))
print("DMC gets:",len(get_cyc))
print("DMC span (halt->get inclusive cycles):",dict(sorted(spans.items())))
print("get-to-get period (top):",dict(sorted(periods.items(), key=lambda x:-x[1])[:8]))
print("held_addr at get (top):",dict(sorted(held_at_get.items(), key=lambda x:-x[1])[:6]))
