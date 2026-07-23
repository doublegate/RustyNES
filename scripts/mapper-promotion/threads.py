import json,sys
d=json.load(sys.stdin)
ns=d["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"]
print(f"{len(ns)} threads")
for t in ns:
    cs=t["comments"]["nodes"]
    c=cs[0] if cs else {}
    a=(c.get("author") or {}).get("login")
    tid=t["id"]
    print(f"\n[TID {tid}] resolved={t['isResolved']} outdated={t['isOutdated']} path={t.get('path')} by={a}")
    print("  ", (c.get("body","") or "")[:280].replace("\n"," "))
