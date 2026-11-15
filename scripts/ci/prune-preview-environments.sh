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

# Every environment, every page. --paginate with --jq emits one name per line.
mapfile -t envs < <(gh api --paginate "repos/$repo/environments?per_page=100" --jq '.environments[].name')
if [ "${#envs[@]}" -eq 0 ]; then
  # A repository with Pages always has at least `github-pages`; zero means the
  # listing failed or the token cannot read environments, not that all is clean.
  echo "::error::no environments listed for $repo; refusing to report it clean" >&2
  exit 1
fi
mapfile -t open_heads < <(gh api --paginate "repos/$repo/pulls?state=open&per_page=100" --jq '.[].head.ref')
# Every branch name in one paginated GraphQL query (100 per request), rather
# than a REST lookup per preview environment.
owner="${repo%%/*}" rname="${repo#*/}"
# shellcheck disable=SC2016  # $owner, $name and $endCursor are GraphQL variables, not shell ones.
mapfile -t branches < <(gh api graphql --paginate -F owner="$owner" -F name="$rname" -f query='
  query($owner: String!, $name: String!, $endCursor: String) {
    repository(owner: $owner, name: $name) {
      refs(refPrefix: "refs/heads/", first: 100, after: $endCursor) {
        pageInfo { hasNextPage endCursor }
        nodes { name }
      }
    }
  }' --jq '.data.repository.refs.nodes[].name')
if [ "${#branches[@]}" -eq 0 ]; then
  echo "::error::no branches listed for $repo; refusing to treat every preview as stale" >&2
  exit 1
fi

kept=0 deleted=0 would=0
for name in "${envs[@]}"; do
  case "$name" in "$prefix"*) ;; *) continue ;; esac
  branch="${name#"$prefix"}"
  if printf '%s\n' "${open_heads[@]}" | grep -qxF -- "$branch"; then
    echo "keep   $name (an open pull request's head)"; kept=$((kept + 1)); continue
  fi
  if printf '%s\n' "${branches[@]}" | grep -qxF -- "$branch"; then
    echo "keep   $name (the branch still exists)"; kept=$((kept + 1)); continue
  fi
  enc_env="$(python3 -c 'import sys, urllib.parse; print(urllib.parse.quote(sys.argv[1], safe=""))' "$name")"
  if [ "$apply" = "--apply" ]; then
    gh api -X DELETE "repos/$repo/environments/$enc_env" --silent
    echo "delete $name"; deleted=$((deleted + 1))
  else
    echo "would delete $name"; would=$((would + 1))
  fi
done
echo "kept $kept, deleted $deleted, would delete $would (of ${#envs[@]} environments)"
