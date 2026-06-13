import re
# reuse the verified 341-byte key from diff2004.py
txt=open("diff2004.py").read()
key_txt=txt.split('key_txt = """')[1].split('"""')[0]
key=[int(x,16) for x in key_txt.split()]
assert len(key)==341,len(key)
def load(path,sl_col,dot_col,val_col,addr_col=None,addr=None):
    d={}
    with open(path) as f:
        next(f)
        for line in f:
            p=line.strip().split(",")
            if len(p)<=max(sl_col,dot_col,val_col): continue
            if addr is not None and p[addr_col]!=addr: continue
            if p[sl_col]!="128": continue
            d[int(p[dot_col])]=int(p[val_col],16)
    return d
mesen=load("mesen_2004.csv",1,2,3)
rusty=load("reg_2004stress_new.csv",6,7,3,addr_col=2,addr="0x2004")
mk=[d for d in range(341) if d in mesen and mesen[d]!=key[d]]
print(f"Mesen vs AnswerKey1: {len(mk)} mismatches", mk[:20])
mr=[d for d in range(341) if d in mesen and d in rusty and mesen[d]!=rusty[d]]
print(f"Mesen vs RustyNES(new): {len(mr)} mismatches")
for d in mr[:30]:
    print(f"  dot{d}: mesen={mesen[d]:02X} rusty={rusty[d]:02X} key={key[d]:02X}")
