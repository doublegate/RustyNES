import json
d=json.load(open("/tmp/claude-1000/-home-parobek-Code-OSS-Public-Projects-RustyNES/6112789e-19c9-4d7b-9638-b241cdbc833d/scratchpad/t251.json"))
for t in d["data"]["repository"]["pullRequest"]["reviewThreads"]["nodes"]:
    c=t["comments"]["nodes"][0]
    print(t["id"], c["databaseId"], t["path"])
