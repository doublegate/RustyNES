import os, subprocess
KEY='02,C0,46,46,03,C0,06,06,04,C1,6C,6C,05,C1,60,60,06,C1,60,60,07,C1,06,06,08,C2,66,66,09,C2,66,66,0A,C2,24,24,0B,C2,66,66,0C,C3,66,66,0D,C3,64,64,0E,C3,60,60,0F,C3,60,60,10,C4,66,66,11,C4,66,66,12,C4,18,18,13,C4,0C,0C,14,C5,6C,6C,15,C5,60,60,16,C5,76,76,17,C5,72,72,18,C6,66,66,19,C6,7C,7C,1A,C6,3C,3C,1B,C6,7C,7C,1C,C7,3C,3C,1D,C7,7E,7E,1E,C7,66,66,1F,C7,66,66,00,C0,3C,3C,01,C0,18,18'
key=[int(x,16) for x in KEY.split(',')]
ROM='tests/roms/accuracycoin/AccuracyCoin.nes'
def run(offs):
    env=dict(os.environ); env['RUSTYNES_2007_OFFSET']=','.join(str(o) for o in offs)
    out=subprocess.run(['./target/release/scan_dma_abort',ROM,'30000'],capture_output=True,text=True,env=env).stdout
    line=[l for l in out.splitlines() if 'STRESS2007=' in l][0].split('STRESS2007=')[1].strip()
    data=list(bytes.fromhex(line))
    d=data[:]
    # the test rotates ±1 to align; emulate by trying both 0 and a 1-rotate, take best
    best=0
    for dd in (d, [d[-1]]+d[:-1]):
        st=[dd[y] for y in range(1,341,2)]
        m=sum(1 for i in range(min(len(key),len(st))) if st[i]==key[i])
        best=max(best,m)
    return best
offs=[0]*8
base=run(offs); print('baseline(all=8):',base,flush=True)
for p in range(8):
    bo,bm=offs[p],run(offs)
    for o in [-10,-8,-6,-4,-2,-1,0,1,2,3,4,6,8,10,12,16]:
        offs[p]=o; m=run(offs)
        if m>bm: bm,bo=m,o
    offs[p]=bo
    print(f'phase {p}: best offset={bo} -> total {bm}',flush=True)
print('FINAL offsets:',offs,'total:',run(offs),flush=True)
