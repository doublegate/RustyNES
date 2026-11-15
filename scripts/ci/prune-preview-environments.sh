#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Delete the Docs7 preview environments whose branch is gone.
#
# WHY THIS EXISTS
# ===============
# The Docs7 GitHub App (Context7's documentation previews) deploys every pull
# request's branch to an environment named "Docs7 Preview: <branch>". GitHub
# creates an environment the first time a deployment names it and never deletes
# one, and Docs7 only marks its old deployments inactive, so every branch that
# ever had a pull request left a permanent environment behind: 33 of them by
# 2026-11-15, deleted by hand then, with 263 Docs7 deployments under them.
#
# Only "Docs7 Preview: <branch>" environments are touched, and only when <branch>
# no longer exists AND no open pull request has it as its head. "Docs7
# Production", "github-pages", "copilot" and anything else are never touched.
#
# THE TOKEN
# =========
# Deleting an environment needs the repository's Administration permission
# (write), which a workflow's GITHUB_TOKEN cannot be granted (it is not one of
# the `permissions:` keys), so this needs a fine-grained personal access token
# scoped to this repository with Administration: read and write, in
# ENV_CLEANUP_TOKEN (see .github/workflows/preview-env-cleanup.yml for why it is
# only ever used from the default branch). Listing needs no such permission.
#
# Usage: GH_TOKEN=<token> prune-preview-environments.sh <owner/repo> [--apply]
# Without --apply it prints what it would delete and changes nothing.
set -euo pipefail

repo="${1:?usage: prune-preview-environments.sh <owner/repo> [--apply]}"
apply="${2:-}"
prefix='Docs7 Preview: '

# Every listing is captured into a variable BEFORE it becomes an array: a plain
# assignment fails the script under `set -e` when `gh` fails on any page, where
# `mapfile < <(gh ...)` would hide the failure and leave a truncated list --
# and a truncated branch or pull-request list here means deleting a live
# preview (Antigravity on #666). Every environment, every page.
env_list="$(gh api --paginate "repos/$repo/environments?per_page=100" --jq '.environments[].name')"
mapfile -t envs <<< "$env_list"
if [ -z "$env_list" ]; then
  # A repository with Pages always has at least `github-pages`; zero means the
  # listing failed or the token cannot read environments, not that all is clean.
  echo "::error::no environments listed for $repo; refusing to report it clean" >&2
  exit 1
fi
pr_list="$(gh api --paginate "repos/$repo/pulls?state=open&per_page=100" --jq '.[].head.ref')"
open_heads=()
# An empty listing (no open pull requests) must give no heads, not one empty
# one (Antigravity on #666).
[ -z "$pr_list" ] || mapfile -t open_heads <<< "$pr_list"
# Every branch name in one paginated GraphQL query (100 per request), rather
# than a REST lookup per preview environment.
owner="${repo%%/*}" rname="${repo#*/}"
# shellcheck disable=SC2016  # $owner, $name and $endCursor are GraphQL variables, not shell ones.
branch_list="$(gh api graphql --paginate -F owner="$owner" -F name="$rname" -f query='
  query($owner: String!, $name: String!, $endCursor: String) {
    repository(owner: $owner, name: $name) {
      refs(refPrefix: "refs/heads/", first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes { name }
      }
    }
  }' --jq '.data.repository.refs.nodes[].name')"
mapfile -t branches <<< "$branch_list"
if [ -z "$branch_list" ]; then
  echo "::error::no branches listed for $repo; refusing to treat every preview as stale" >&2
  exit 1
fi

# Membership tests read the whole list (`grep -xF ... >/dev/null`, never `grep -q`):
# `-q` exits at the first match, the `printf` feeding it then dies of SIGPIPE
# once the list outgrows the pipe buffer, and under pipefail the pipeline fails
# -- a match reported as a miss (CodeRabbit on #666; reproduced with 20,000
# names).
kept=0 deleted=0 would=0 failed=0
for name in "${envs[@]}"; do
  case "$name" in "$prefix"*) ;; *) continue ;; esac
  branch="${name#"$prefix"}"
  if [ -z "$branch" ]; then
    echo "keep   $name (no branch name to check)"; kept=$((kept + 1)); continue
  fi
  if printf '%s\n' "${open_heads[@]}" | grep -xF -- "$branch" >/dev/null; then
    echo "keep   $name (an open pull request's head)"; kept=$((kept + 1)); continue
  fi
  if printf '%s\n' "${branches[@]}" | grep -xF -- "$branch" >/dev/null; then
    echo "keep   $name (the branch still exists)"; kept=$((kept + 1)); continue
  fi
  # Recheck just before acting (CodeRabbit on #666): the lists above are a
  # snapshot, and a branch recreated, or a pull request opened, since then
  # must not lose its preview. One branch lookup and one pull-request lookup
  # per candidate; there are few. A failed lookup counts as "still in use".
  enc_branch="$(python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=""))' "$branch")"
  # `gh api` exits non-zero on the 404 that means "gone", so its exit status
  # cannot be the test (under pipefail it discarded the status line, and every
  # candidate looked "could not be checked"); the response's status line is.
  status="$(gh api -i "repos/$repo/branches/$enc_branch" 2>/dev/null || true)"
  status="${status%%$'\n'*}"
  case "$status" in *" 404"*) ;; *)
    echo "keep   $name (the branch exists, or could not be checked, on a recheck)"; kept=$((kept + 1)); continue ;;
  esac
  # Every open pull request's head, whoever owns it: a `head=owner:branch`
  # filter misses a fork's `alice:topic`, whose preview Docs7 also names
  # "Docs7 Preview: topic" (CodeRabbit on #666).
  if ! heads_now="$(gh api --paginate "repos/$repo/pulls?state=open&per_page=100" --jq '.[].head.ref')" \
     || printf '%s\n' "$heads_now" | grep -xF -- "$branch" >/dev/null; then
    echo "keep   $name (an open pull request, or a failed check, on a recheck)"; kept=$((kept + 1)); continue
  fi
  enc_env="$(python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=""))' "$name")"
  if [ "$apply" = "--apply" ]; then
    # One failed delete is recorded and the run goes on to the rest, failing at
    # the end (Antigravity on #666), as prune-deployments.sh does.
    if gh api -X DELETE "repos/$repo/environments/$enc_env" --silent; then
      echo "delete $name"; deleted=$((deleted + 1))
    else
      echo "::warning::could not delete environment $name"; failed=$((failed + 1))
    fi
  else
    echo "would delete $name"; would=$((would + 1))
  fi
done
echo "kept $kept, deleted $deleted, would delete $would, failed $failed (of ${#envs[@]} environments)"
[ "$failed" -eq 0 ]
