def load(p):
    d={}
    with open(p) as f:
        next(f)
        for line in f:
            fr,cy=line.strip().split(',')
            d[int(fr)]=int(cy)
    return d
leg=load('/tmp/RustyNES_v2/leg_frames.csv')
r4=load('/tmp/RustyNES_v2/r4_frames.csv')
out=[]
frames=sorted(set(leg)&set(r4))
out.append("frames compared: %d"%len(frames))
# delta = r4_cycle - leg_cycle (cumulative). Per-frame length = cy[f]-cy[f-1].
prev=None
first_div=None
out.append("frame | leg_cyc | r4_cyc | cum_delta(r4-leg) | leg_len | r4_len | len_delta")
for f in frames:
    cum = r4[f]-leg[f]
    leg_len = leg[f]-leg[f-1] if (f-1) in leg else 0
    r4_len  = r4[f]-r4[f-1] if (f-1) in r4 else 0
    ld = r4_len-leg_len
    if first_div is None and cum!=0:
        first_div=f
    # print only frames where the per-frame length differs (the drift events)
    if ld!=0:
        out.append("%d | %d | %d | %d | %d | %d | %+d"%(f,leg[f],r4[f],cum,leg_len,r4_len,ld))
out.append("")
out.append("first frame with nonzero cumulative delta: %s"%first_div)
# summary: total drift at last common frame
lastf=frames[-1]
out.append("cumulative delta at frame %d: %d cycles"%(lastf, r4[lastf]-leg[lastf]))
# count of frames where r4 longer / shorter / equal
longer=sum(1 for f in frames if (f-1) in leg and (f-1) in r4 and (r4[f]-r4[f-1])>(leg[f]-leg[f-1]))
shorter=sum(1 for f in frames if (f-1) in leg and (f-1) in r4 and (r4[f]-r4[f-1])<(leg[f]-leg[f-1]))
out.append("frames r4-longer: %d  r4-shorter: %d"%(longer,shorter))
open('/tmp/RustyNES_v2/framediff_out.txt','w').write("\n".join(out)+"\n")
print("done; div frames:", sum(1 for f in frames if (f-1) in leg and (f-1) in r4 and (r4[f]-r4[f-1])!=(leg[f]-leg[f-1])))
