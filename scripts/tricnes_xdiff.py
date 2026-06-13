#!/usr/bin/env python3
"""Per-cycle cross-diff: RustyNES (anchor CSV) vs TriCNES (XC stderr log).

Primary ROM landmark: the Implicit-DMA-Abort Loop3 `STA $540,X` measure store
for X=10 (each stream's rel-0 in its own capture). Because the buggy RustyNES
run catches `$00` on slide 0 (Y=0, SHORT CalculateDMADuration) while TriCNES
catches on slide 4 (Y=4, LONG), the two calls differ in length, so we RE-ANCHOR
on a SECONDARY shared landmark — the CalculateDMADuration ENTRY ($DA03, the
`LDY #0`) — which both streams reach identically. From there we walk FORWARD and
diff (access,addr) cycle-by-cycle to pin where the DMC GET / $4000 read stream
first diverges by 4 cycles.

PC is NOT directly comparable (RustyNES latches the opcode-fetch PC; TriCNES
reports mid-instruction PC), so alignment is on (access, addr) + relative cycle.

Usage:
  /tmp/xdiff.py <rusty_anchor.csv> <tri_xc.txt> <tri_store_cyc>
                [reanchor_addr=DA03] [lo=-5] [hi=70]
"""
import csv
import sys


def load_rusty(path):
    out = []
    for r in csv.DictReader(open(path)):
        out.append(dict(cyc=int(r["cpu_cycle"]), rel0=int(r["rel"]),
                        pc=r["pc"], acc=r["access"].upper(), addr=r["addr"],
                        data=r["data"], dmc=r["in_dmc"], put=r["put_cycle"]))
    return out


def load_tri(path, store_cyc):
    out = []
    for line in open(path):
        if not line.startswith("XC "):
            continue
        d = dict(kv.split("=") for kv in line.split()[1:])
        cyc = int(d["cyc"])
        out.append(dict(cyc=cyc, rel0=cyc - store_cyc, pc=d["pc"],
                        acc=d["acc"].upper(), addr=d["addr"], data=d["data"],
                        dmc=d["dmc"], put=d["put"]))
    return out


def reanchor(rows, addr):
    """Cyc of the last addr-read with rel0<=0 (the X=10 CalculateDMADuration
    entry, since later iterations/calls all have rel0>0 past the store)."""
    cand = [r for r in rows if r["addr"] == addr and r["acc"] == "R"
            and r["rel0"] <= 0]
    if not cand:
        raise SystemExit(f"reanchor addr {addr} not found in window")
    return max(cand, key=lambda r: r["cyc"])["cyc"]


def fmt(r):
    if r is None:
        return f"{'·':>9} {'--':<2} {'----':<4} {'--':<2} {'-':<1}{'-':<1}"
    return f"{r['cyc']:>9} {r['acc']:<2} {r['addr']:<4} {r['data']:<2} d{r['dmc']}p{r['put']}"


def main():
    rusty_path, tri_path, store_cyc = sys.argv[1], sys.argv[2], int(sys.argv[3])
    raddr = sys.argv[4] if len(sys.argv) > 4 else "DA03"
    lo = int(sys.argv[5]) if len(sys.argv) > 5 else -5
    hi = int(sys.argv[6]) if len(sys.argv) > 6 else 70

    R = load_rusty(rusty_path)
    T = load_tri(tri_path, store_cyc)
    rz = reanchor(R, raddr)
    tz = reanchor(T, raddr)
    print(f"# RustyNES rows={len(R)} TriCNES rows={len(T)}")
    print(f"# RE-ANCHOR on CalculateDMADuration entry ${raddr}: "
          f"RustyNES cyc={rz}  TriCNES cyc={tz}  (boot offset at entry "
          f"= {tz - rz})")
    Rm = {r["cyc"] - rz: r for r in R}
    Tm = {t["cyc"] - tz: t for t in T}
    print(f"#{'e':>4} | {'RustyNES  cyc acc addr dt dp':<31}| "
          f"{'TriCNES   cyc acc addr dt dp':<31}| flag")
    print("#" + "-" * 78)
    first_div = None
    div_count = 0
    for e in range(lo, hi + 1):
        r, t = Rm.get(e), Tm.get(e)
        flag = ""
        if r and t and (r["acc"] != t["acc"] or r["addr"] != t["addr"]):
            flag = "<<DIFF"
            div_count += 1
            if first_div is None:
                first_div = e
        elif (r is None) != (t is None):
            flag = "<<GAP"
        # annotate salient rows
        ann = []
        for tag, row in (("R", r), ("T", t)):
            if row and row["addr"] == "4000" and row["acc"] == "R":
                ann.append(f"{tag}:$4000={row['data']}"
                           + ("=CATCH" if row["data"] == "00" else ""))
            if row and row["addr"].startswith("FF") and row["acc"] == "R" \
                    and row["dmc"] == "1":
                ann.append(f"{tag}:GET_{row['addr']}")
        print(f" {e:>4} | {fmt(r):<31}| {fmt(t):<31}| {flag} {' '.join(ann)}")
    print("#" + "-" * 78)
    if first_div is not None:
        print(f"# FIRST (acc,addr) divergence at entry-rel e = {first_div}; "
              f"{div_count} differing cycles in [{lo},{hi}]")
    # GET / catch positions (entry-relative)
    def hits(M, pred):
        return sorted(e for e, x in M.items() if lo <= e <= hi and pred(x))
    g = lambda x: x["addr"].startswith("FF") and x["acc"] == "R" and x["dmc"] == "1"
    c = lambda x: x["addr"] == "4000" and x["acc"] == "R" and x["data"] == "00"
    l4 = lambda x: x["addr"] == "4000" and x["acc"] == "R"
    print(f"# DMC GET (R $FFxx) entry-rel: RustyNES={hits(Rm,g)} TriCNES={hits(Tm,g)}")
    print(f"# LDA $4000 reads  entry-rel: RustyNES={hits(Rm,l4)} TriCNES={hits(Tm,l4)}")
    print(f"# $4000==$00 catch entry-rel: RustyNES={hits(Rm,c)} TriCNES={hits(Tm,c)}")


if __name__ == "__main__":
    main()
