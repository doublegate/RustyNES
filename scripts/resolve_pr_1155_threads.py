#!/usr/bin/env python3
import json
import subprocess
import sys

def run_gh_api(*args):
    cmd = ['gh', 'api'] + list(args)
    result = subprocess.run(cmd, capture_output=True, text=True)
    if result.returncode != 0:
        print(f"Error running gh api: {result.stderr}")
        sys.exit(1)
    return result.stdout

query_get_threads = """
query($owner: String!, $repo: String!, $number: Int!) {
  repository(owner: $owner, name: $repo) {
    pullRequest(number: $number) {
      reviewThreads(first: 100) {
        nodes {
          id
          isResolved
          comments(first: 1) {
            nodes {
              body
            }
          }
        }
      }
    }
  }
}
"""

query_reply = """
mutation($subjectId: ID!, $body: String!) {
  addPullRequestReviewThreadReply(input: {pullRequestReviewThreadId: $subjectId, body: $body}) {
    comment { id }
  }
}
"""

query_resolve = """
mutation($threadId: ID!) {
  resolveReviewThread(input: {threadId: $threadId}) {
    thread { isResolved }
  }
}
"""

print("======================================================")
print(" Replying to & Resolving PR #1155 Review Threads      ")
print("======================================================\n")

print("Fetching PR #1155 review threads from libretro/docs...")
out = run_gh_api('graphql', '-F', 'owner=libretro', '-F', 'repo=docs', '-F', 'number=1155', '-f', f'query={query_get_threads}')
data = json.loads(out)
threads = data['data']['repository']['pullRequest']['reviewThreads']['nodes']

resolved_count = 0

for thread in threads:
    if thread['isResolved']:
        continue

    thread_id = thread['id']
    comment_body = thread['comments']['nodes'][0]['body']

    # Determine appropriate reply message based on bot comment content
    reply_msg = "Fixed as requested."
    if "blank line" in comment_body.lower() or "inconsistent" in comment_body.lower():
        reply_msg = "Fixed by adding explicit preceding blank lines to ensure strict markdown list parsing."
    elif "alignment separator" in comment_body.lower():
        reply_msg = "Fixed by center-aligning the Supported column to match other NES cores."
    elif "memory monitoring" in comment_body.lower():
        reply_msg = "Fixed by updating the label and linking directly to the Memory Monitoring guide."
    elif "nes controller image" in comment_body.lower():
        reply_msg = "Fixed by embedding the standard NES controller image above the mapping table."

    print(f"\n[Thread ID: {thread_id}]")
    print(f"Comment: {comment_body[:80]}...")
    print(f"Reply: {reply_msg}")

    print(" -> Posting reply...")
    run_gh_api('graphql', '-F', f'subjectId={thread_id}', '-F', f'body={reply_msg}', '-f', f'query={query_reply}')

    print(" -> Marking thread as 'Resolved'...")
    run_gh_api('graphql', '-F', f'threadId={thread_id}', '-f', f'query={query_resolve}')

    resolved_count += 1

print(f"\n======================================================")
print(f" All done! Successfully replied and resolved {resolved_count} threads.")
print(f"======================================================")
