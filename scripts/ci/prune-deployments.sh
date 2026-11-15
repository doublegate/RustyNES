#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Delete superseded deployments, keeping the newest one of every environment.
#
# WHY THIS EXISTS
# ===============
# GitHub keeps every deployment forever. Each Docs7 preview of each pull
# request push, each GitHub Pages publish and each Copilot agent run adds one,
# and a superseded deployment is only marked inactive. By 2026-11-15 this
# repository held 668 (263 from Docs7, 405 from Pages and Copilot), and each
# preview branch's deployments also kept its environment alive.
#
# WHAT IS KEPT, AND WHAT GOES
# ===========================
# - KEPT: the newest deployment of every environment that still exists, whatever
#   its state, AND its newest SUCCESS deployment, which is the live one while a
#   newer deployment is in flight or after it failed (the live Pages site, Docs7
#   Production, an open pull request's preview). A deployment in a transient
#   state (queued, pending, in progress, waiting) is never touched anywhere.
# - DELETED: every other deployment. GitHub only deletes an inactive deployment,
#   so one in another state (an older `success`, a `failure` or an `error`) is
#   first marked `inactive`; that is safe because a newer SUCCESS deployment of
#   the same environment is kept, or the environment itself is gone.
#
# Needs `deployments: write` (GITHUB_TOKEN can have it). Deployments and their
# latest status come from GraphQL, 100 per request: the REST list carries no
# state, and one REST call per deployment took several minutes for 668.
#
# Usage: GH_TOKEN=<token> prune-deployments.sh <owner/repo> [--apply]
# Without --apply it prints the counts it would act on and changes nothing.
set -euo pipefail

repo="${1:?usage: prune-deployments.sh <owner/repo> [--apply]}"
apply="${2:-}"
owner="${repo%%/*}" name="${repo#*/}"

# Captured into a variable first, so a failure on any page fails the script
# (`mapfile < <(gh ...)` would hide it and leave a truncated list; Antigravity
# on #666). `rows` below is captured the same way.
env_list="$(gh api --paginate "repos/$repo/environments?per_page=100" --jq '.environments[].name')"
mapfile -t envs <<< "$env_list"
if [ -z "$env_list" ]; then
  echo "::error::no environments listed for $repo; refusing to guess which deployments are live" >&2
  exit 1
fi

# id <TAB> environment <TAB> createdAt <TAB> latest state <TAB> node id, every deployment.
# shellcheck disable=SC2016  # $owner, $name and $endCursor are GraphQL variables, not shell ones.
rows="$(gh api graphql --paginate -F owner="$owner" -F name="$name" -f query='
  query($owner: String!, $name: String!, $endCursor: String) {
    repository(owner: $owner, name: $name) {
      deployments(first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes { id databaseId environment createdAt latestStatus { state } }
      }
    }
  }' --jq '.data.repository.deployments.nodes[] | [.databaseId, .environment, .createdAt, (.latestStatus.state // "NONE"), .id] | @tsv')"
total="$(printf '%s\n' "$rows" | grep -c . || true)"
if [ "$total" -eq 0 ]; then
  echo "no deployments in $repo"; exit 0
fi

# Kept, for each EXISTING environment: its newest deployment AND its newest
# SUCCESS deployment. The two differ while a newer deployment is still in
# flight or after it failed, and then the older SUCCESS one is the LIVE one:
# keeping only the newest marked the live deployment inactive and deleted it
# (Antigravity on #666). And anywhere, a deployment in a transient state is
# never touched: it is still running.
keep_ids="$(printf '%s\n' "$rows" | python3 -c '
import sys
envs = set(sys.argv[1:])
transient = {"IN_PROGRESS", "QUEUED", "PENDING", "WAITING", "NONE"}
newest, newest_ok, keep = {}, {}, set()
for line in sys.stdin:
    did, env, created, state, _ = line.rstrip("\n").split("\t")
    if state in transient:
        keep.add(did)
    if env not in envs:
        continue
    if env not in newest or created > newest[env][0]:
        newest[env] = (created, did)
    if state == "SUCCESS" and (env not in newest_ok or created > newest_ok[env][0]):
        newest_ok[env] = (created, did)
keep |= {did for _, did in newest.values()} | {did for _, did in newest_ok.values()}
print("\n".join(sorted(keep)))
' "${envs[@]}")"

# Mark the not-yet-inactive ones inactive (REST, one call each: they are few,
# since GitHub marks a superseded deployment inactive itself), then delete in
# batches of GraphQL `deleteDeployment` mutations, 50 aliased per request: 668
# deletions took about ten minutes one REST call at a time.
kept=0 marked=0 would=0 failed=0
del_ids=()
while IFS=$'\t' read -r did env created state nid; do
  if printf '%s\n' "$keep_ids" | grep -qxF -- "$did"; then
    kept=$((kept + 1)); continue
  fi
  if [ "$apply" != "--apply" ]; then
    would=$((would + 1)); continue
  fi
  if [ "$state" != "INACTIVE" ]; then
    if gh api -X POST "repos/$repo/deployments/$did/statuses" -f state=inactive --silent; then
      marked=$((marked + 1))
    else
      echo "::warning::could not mark deployment $did ($env, $created) inactive"; failed=$((failed + 1)); continue
    fi
  fi
  del_ids+=("$nid")
done <<< "$rows"

deleted=0
for ((i = 0; i < ${#del_ids[@]}; i += 50)); do
  chunk=("${del_ids[@]:i:50}")
  # The request goes in as a JSON body on stdin, not `-f query=...`: a 50-mutation
  # query is over 4 KiB, past the per-argument limit some hosts impose on argv
  # ("too long argument ... arg-max-len (4128)" here, 2026-11-15).
  # A GraphQL error exits 0 with an `errors` array, so count what succeeded
  # from the response, not from the exit status.
  out="$(python3 -c '
import json, sys
ids = sys.argv[1:]
body = " ".join(f"d{n}: deleteDeployment(input: {{id: \"{i}\"}}) {{ clientMutationId }}" for n, i in enumerate(ids))
print(json.dumps({"query": "mutation { " + body + " }"}))
' "${chunk[@]}" | gh api graphql --input - 2>&1 || true)"
  ok="$(printf '%s' "$out" | python3 -c '
import json, sys
try:
    d = json.load(sys.stdin)
except Exception:
    print(0); sys.exit()
data = d.get("data") or {}
print(sum(1 for v in data.values() if v is not None))
')"
  deleted=$((deleted + ok))
  if [ "$ok" -ne "${#chunk[@]}" ]; then
    failed=$((failed + ${#chunk[@]} - ok))
    echo "::warning::batch at $i: deleted $ok of ${#chunk[@]}: $(printf '%s' "$out" | head -c 400)"
  fi
done
echo "of $total deployments: kept $kept (the newest of each environment), deleted $deleted (marked inactive first: $marked), would delete $would, failed $failed"
[ "$failed" -eq 0 ]
