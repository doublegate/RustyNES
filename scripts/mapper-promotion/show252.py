import json
d=json.load(open("/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/6112789e-19c9-4d7b-9638-b241cdbc833d/scratchpad/t252.json"))
for t in d["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"]:
    if t["isResolved"]: continue
    c=t["comments"]["nodes"][0]
    print(f"\n=== TID {t['id']} | dbId {c['databaseId']} | {t['path']}:{t.get('line')} | by {c['author']['login']} ===")
    print(c["body"][:700])
