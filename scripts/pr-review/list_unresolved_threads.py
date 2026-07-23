import json,sys
d=json.load(sys.stdin)
for t in d["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"]:
    if t["isResolved"]: continue
    c=t["comments"]["nodes"][0]
    print(f"TID={t['id']} dbId={c['databaseId']} {t.get('path')}:{t.get('line')} by={c['author']['login']}")
    print("  ", (c["body"] or "")[:650].replace("\n"," "))
    print()
