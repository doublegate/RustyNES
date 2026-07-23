#!/usr/bin/env bash
#
# agy-review.sh -- headless GitHub PR reviewer driven by Antigravity CLI (`agy`).
#
# Runs on a SELF-HOSTED GitHub Actions runner that lives on a machine where `agy`
# is already logged in via Google OAuth. Because it uses the CLI's cached OAuth
# session (not an API key), every review is billed against your Google AI Ultra
# rate limits -- i.e. free under the subscription, no metered API spend.
#
# Flow: resolve PR -> `gh pr diff` -> build an adversarial-reviewer prompt
#       (+ repo style guide) -> `agy --print` under a PTY -> post via `gh pr comment`.
#
# See ../README.md for setup, the issue #76 PTY workaround, and the ToS caveat.
set -euo pipefail

# --- configuration (all env-overridable from the workflow) ---------------------
AGY_BIN="${AGY_BIN:-agy}"
command -v "$AGY_BIN" >/dev/null 2>&1 || AGY_BIN="$HOME/.local/bin/agy"
AGY_MODEL="${AGY_MODEL:-}"                 # empty = agy's configured default (Gemini 3.x Pro)
AGY_EFFORT="${AGY_EFFORT:-high}"           # low|medium|high
# Recorded BEFORE defaulting so the large-diff path can raise the timeout without ever
# overriding a value the caller asked for explicitly.
agy_print_timeout_explicit="${AGY_PRINT_TIMEOUT:+set}"
AGY_PRINT_TIMEOUT="${AGY_PRINT_TIMEOUT:-5m}"
# Separate budget for the on-disk handoff below. An inlined diff arrives with the prompt
# and 5m is ample; a handed-off diff has to be READ first, one tool call per part, before
# reasoning even starts. Timing that out would reproduce the failure this replaces -- an
# empty or partial review of a large PR -- so large PRs get proportionally longer.
AGY_PRINT_TIMEOUT_LARGE="${AGY_PRINT_TIMEOUT_LARGE:-25m}"
MAX_DIFF_BYTES="${MAX_DIFF_BYTES:-90000}"  # inline-embedding budget for the diff (~90 KB)
# Hard ceiling on the ASSEMBLED prompt. The prompt reaches agy as one argv string, and
# Linux caps a single argument at MAX_ARG_STRLEN = 32 * PAGE_SIZE = 128 KiB regardless of
# ARG_MAX; exceeding it fails the exec with E2BIG. Capping only the diff is not enough --
# the boilerplate and the style guide ride in the same string.
MAX_PROMPT_BYTES="${MAX_PROMPT_BYTES:-120000}"
# Above MAX_DIFF_BYTES the diff is HANDED OFF ON DISK instead of being truncated (see
# "hand the diff off on disk" below). agy is an agent with file-reading tools, so the
# argv ceiling bounds only what can be *inlined* -- it is not a bound on what can be
# reviewed. Parts are sized so each is a comfortable single read for the agent.
AGY_DIFF_PART_BYTES="${AGY_DIFF_PART_BYTES:-150000}"
# Work dir for the on-disk handoff. Kept INSIDE the checkout deliberately: it is then
# part of agy's own workspace, so the read needs no --add-dir and no sandbox exception.
# Untracked, gitignored (`.agy-review-work/`), and removed on exit.
AGY_WORK_DIR="${AGY_WORK_DIR:-.agy-review-work}"
STYLE_GUIDE="${STYLE_GUIDE:-.github/agy-review.md}"  # repo-relative; loaded if present
                                           # (dedicated name -- avoids colliding with GEMINI.md/AGENTS.md)
# Per-run log path. A fixed name would collide between concurrent jobs whenever
# RUNNER_TEMP is unset (local runs fall back to /tmp, which is shared).
LOG="${RUNNER_TEMP:-/tmp}/agy-review-${GITHUB_RUN_ID:-$$}.log"
AGY_LOCK="${AGY_LOCK:-$HOME/.gemini/antigravity-cli/.agy-review.lock}"
AGY_LOCK_WAIT="${AGY_LOCK_WAIT:-600}"      # seconds to wait for the agy lock before proceeding
AGY_RETRIES="${AGY_RETRIES:-3}"            # attempts to get a usable agy response
AGY_RETRY_DELAY="${AGY_RETRY_DELAY:-15}"   # base backoff seconds between retries (grows per attempt)
MARKER="<!-- antigravity-pr-review -->"

log() { printf '[agy-review] %s\n' "$*" >&2; }
have_text() { [ -s "$1" ] && grep -q '[^[:space:]]' "$1"; }

REPO="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY not set}"

# --- resolve the PR number from the triggering event --------------------------
case "${GITHUB_EVENT_NAME:-}" in
  pull_request|pull_request_target)
    PR="$(jq -r '.pull_request.number' "$GITHUB_EVENT_PATH")"
    ;;
  issue_comment)
    is_pr="$(jq -r '.issue.pull_request // empty' "$GITHUB_EVENT_PATH")"
    body="$(jq -r '.comment.body // ""' "$GITHUB_EVENT_PATH")"
    assoc="$(jq -r '.comment.author_association // ""' "$GITHUB_EVENT_PATH")"
    [ -n "$is_pr" ] || { log "comment is not on a PR; skipping"; exit 0; }
    case "$body" in
      /agy-review*) : ;;
      *) log "comment is not an /agy-review command; skipping"; exit 0 ;;
    esac
    # Defense in depth: the workflow `if:` already gates on author_association, but this
    # script is also runnable by hand and by any future caller. Re-check here so the gate
    # cannot be lost by an edit to the workflow alone.
    case "$assoc" in
      OWNER|MEMBER|COLLABORATOR) : ;;
      *) log "comment author association '$assoc' lacks write access; skipping"; exit 0 ;;
    esac
    PR="$(jq -r '.issue.number' "$GITHUB_EVENT_PATH")"
    ;;
  *)
    PR="${1:-}"
    [ -n "$PR" ] || { log "unknown event; pass a PR number as \$1"; exit 1; }
    ;;
esac
log "reviewing ${REPO}#${PR}"

# Remove every temp file on exit. Pre-declared so the trap is safe under `set -u` even if the
# script exits before a given file is created.
diff_file= diff_err= meta_file= prompt_file= out_file= raw= body_file=
# Set when the large-diff fallback creates refs/agy/* so the trap can remove them.
agy_refs_created=
# Set when the on-disk diff handoff creates $AGY_WORK_DIR inside the checkout.
agy_work_created=
cleanup() {
  rm -f "$diff_file" "$diff_err" "$meta_file" "$prompt_file" "$out_file" "$raw" "$body_file"
  # Guarded on the flag, not on directory existence: this must never remove a path
  # that was already there when the script started.
  [ -n "$agy_work_created" ] && rm -rf "$AGY_WORK_DIR" || true
  if [ -n "$agy_refs_created" ] && [ -n "${PR:-}" ]; then
    git update-ref -d "refs/agy/pr-${PR}" 2>/dev/null || true
    git update-ref -d "refs/agy/base-${PR}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# --- metadata first, because the fork gate depends on it -----------------------
# FAIL-CLOSED. The old form fell back to `{}` when `gh pr view` failed, which was
# harmless when the only field read was the title -- it is not harmless now that
# isCrossRepository gates whether an untrusted diff reaches agy. A lookup failure must
# never be indistinguishable from "same-repo".
meta_file="$(mktemp)"
gh pr view "$PR" --repo "$REPO" --json title,isCrossRepository,baseRefName > "$meta_file" \
  || { log "gh pr view failed; refusing to review without knowing the PR's head repo"; exit 1; }

# THE FORK GATE (see the trust model at the agy invocation below). The workflow `if:`
# blocks fork PRs on the `pull_request` trigger, but it CANNOT do so on `issue_comment`:
# that payload carries no head-repo information at all, so a collaborator commenting
# `/agy-review` on a fork PR would otherwise schedule this job against an external diff.
# A trusted person typing the command does not make the DIFF trusted -- and the diff is
# what agy ingests, under --dangerously-skip-permissions, on the maintainer's machine.
# Enforced here because this is the first point where the answer is knowable.
# NOT `.isCrossRepository // empty` -- jq's `//` treats `false` as absent, so the
# alternative fires on exactly the same-repo case this gate is meant to admit, and every
# legitimate review would be refused. Read the raw value and match all three shapes.
is_fork="$(jq -r '.isCrossRepository' "$meta_file")"
case "$is_fork" in
  true)
    log "PR #${PR} is from a fork; refusing to run agy on an external diff"
    log "(review it by hand, or push the branch into this repo first)"
    exit 0
    ;;
  false) : ;;
  *) log "could not determine whether PR #${PR} is cross-repository; refusing"; exit 1 ;;
esac

# --- fetch the diff ------------------------------------------------------------
# `gh pr diff` can fail for two very different reasons and they must not be
# conflated. A genuine error (auth, network, bad PR) is fatal. But GitHub's API
# also refuses any diff over **20,000 lines** with HTTP 406, and that is not an
# error condition -- it just means this PR is too large to review through the
# API. A reviewer that cannot see the diff must not claim a verdict, but it also
# must not fail the build: a red check that means "the PR was big" is noise that
# trains people to ignore the check. Skip cleanly instead.
#
# Note this is upstream of the MAX_DIFF_BYTES truncation below, which can only
# shrink a diff we already have.
diff_file="$(mktemp)"
diff_err="$(mktemp)"
if ! gh pr diff "$PR" --repo "$REPO" > "$diff_file" 2> "$diff_err"; then
  if grep -qi 'diff exceeded the maximum number of lines' "$diff_err"; then
    # GitHub's API refuses any diff over 20,000 lines with HTTP 406. That is a
    # limit of the *transport*, not a reason to skip the review -- a large PR is
    # precisely the one worth reviewing. Fall back to computing the diff locally,
    # which has no such ceiling; MAX_DIFF_BYTES below then trims it to the prompt
    # budget exactly as it does for any other large diff.
    #
    # SECURITY: this fetches the PR's objects but NEVER checks them out. The
    # working tree stays on the default branch, so the reviewer scripts and style
    # guide still come from `main` and a PR cannot rewrite its own reviewer. The
    # PR's content is treated exactly as the API diff was: read-only bytes that
    # become prompt text and are never executed. `refs/agy/*` are private
    # namespaces so this cannot clobber a real branch, and are deleted on exit.
    #
    # Auth goes through `http.extraheader` for this one command rather than a
    # persisted credential, keeping the repo-wide `persist-credentials: false`
    # rule intact.
    base_ref="$(jq -r '.baseRefName' "$meta_file")"
    if [ -z "$base_ref" ] || [ "$base_ref" = "null" ]; then
      log "diff exceeds the API limit and the base branch is unknown; cannot fall back"
      exit 1
    fi
    log "diff exceeds GitHub's 20,000-line API limit; falling back to a local git diff"
    pr_ref="refs/agy/pr-${PR}"
    base_local="refs/agy/base-${PR}"
    agy_refs_created=1
    #
    # Two auth shapes, tried in order, because the same script runs in two places.
    # `AUTHORIZATION: bearer` is the form Actions' GITHUB_TOKEN accepts and is tried
    # first, since that is the path that matters in CI. It does NOT work for a personal
    # token from `gh auth token` ("remote: invalid credentials"), so a hand-run on a
    # developer box falls through to a plain fetch using whatever credential helper git
    # is already configured with. Neither path persists anything into .git/config.
    fetch_refspecs=( "+refs/pull/${PR}/head:${pr_ref}" "+refs/heads/${base_ref}:${base_local}" )
    if [ -n "${GH_TOKEN:-}" ] \
       && git -c "http.extraheader=AUTHORIZATION: bearer ${GH_TOKEN}" fetch --no-tags --quiet \
              origin "${fetch_refspecs[@]}" 2>/dev/null; then
      :
    elif git fetch --no-tags --quiet origin "${fetch_refspecs[@]}"; then
      log "fetched PR refs using git's ambient credentials (token header not accepted)"
    else
      log "could not fetch PR #${PR} refs for the local diff fallback"
      exit 1
    fi
    merge_base="$(git merge-base "$base_local" "$pr_ref")" || {
      log "could not compute the merge base for PR #${PR}"; exit 1; }
    git diff "$merge_base" "$pr_ref" > "$diff_file" || {
      log "local git diff failed for PR #${PR}"; exit 1; }
    log "local diff: $(wc -l < "$diff_file") lines, $(wc -c < "$diff_file") bytes"
  else
    log "gh pr diff failed:"; sed 's/^/  /' "$diff_err" >&2
    exit 1
  fi
fi

if ! have_text "$diff_file"; then log "empty diff; nothing to review"; exit 0; fi

# --- decide how the diff reaches agy: inlined, or handed off on disk ------------
# A diff larger than the argv budget used to be TRUNCATED, which silently produced a
# review of the first ~90 KB while reading as a review of the whole PR. That is worse
# than no review: it is a confident verdict over an arbitrary prefix. The argv ceiling
# is a limit on what can be *inlined*, not on what agy can *read* -- it has file tools
# -- so large diffs are written into agy's own workspace and the prompt points at them.
# Truncation now happens only if the on-disk handoff itself cannot be set up.
truncated=""
diff_bytes="$(wc -c < "$diff_file")"
diff_parts=()
if [ "$diff_bytes" -le "$MAX_DIFF_BYTES" ]; then
  log "diff is ${diff_bytes} bytes; inlining it in the prompt"
else
  log "diff is ${diff_bytes} bytes; handing it off on disk (over the ${MAX_DIFF_BYTES}-byte inline budget)"
  rm -rf "$AGY_WORK_DIR"
  mkdir -p "$AGY_WORK_DIR"
  agy_work_created=1
  # Split rather than pointing at one 2+ MB file: a single read of that size is at the
  # mercy of whatever per-read cap the agent applies, and a silent cap is exactly the
  # failure this replaces. Fixed-size parts make coverage explicit -- the prompt lists
  # every part with its line count, so an incomplete read is visible in the output.
  split -b "$AGY_DIFF_PART_BYTES" -d -a 3 \
        --additional-suffix=.diff "$diff_file" "$AGY_WORK_DIR/pr-${PR}.part-"
  while IFS= read -r part; do diff_parts+=("$part"); done \
    < <(find "$AGY_WORK_DIR" -maxdepth 1 -name "pr-${PR}.part-*.diff" | sort)
  if [ "${#diff_parts[@]}" -eq 0 ]; then
    log "on-disk handoff produced no parts; falling back to truncating the diff"
    head -c "$MAX_DIFF_BYTES" "$diff_file" > "$diff_file.cut" && mv "$diff_file.cut" "$diff_file"
    truncated=$'\n\n> Note: the diff was truncated to '"${MAX_DIFF_BYTES}"$' bytes for this review.'
  else
    log "wrote ${#diff_parts[@]} diff part(s) to ${AGY_WORK_DIR}/"
    if [ -z "$agy_print_timeout_explicit" ]; then
      AGY_PRINT_TIMEOUT="$AGY_PRINT_TIMEOUT_LARGE"
      log "print timeout raised to ${AGY_PRINT_TIMEOUT} for the on-disk handoff"
    fi
  fi
fi

# --- build the prompt ----------------------------------------------------------
title="$(jq -r '.title // ""' "$meta_file")"
style=""; [ -f "$STYLE_GUIDE" ] && style="$(cat "$STYLE_GUIDE")"

prompt_file="$(mktemp)"
{
  cat <<EOF
You are an adversarial code reviewer doing a first-pass review of a GitHub pull request.
Act as a skeptical senior engineer, not the author. Be concise, specific, and honest.

Output (GitHub-flavored Markdown, no preamble):
1. A one-sentence summary of what the PR does.
2. "### Blocking issues" -- correctness, security, data-loss, or breaking-change
   problems only. Write "None found." if there are none.
3. "### Suggestions" -- non-blocking improvements; cite file and line where you can.
4. "### Nitpicks" -- optional, keep terse.
Do not praise. Focus on what could be wrong. If the change is trivial, say so briefly.

PR title: ${title}
EOF
  if [ -n "$style" ]; then
    printf '\n--- PROJECT STYLE GUIDE (enforce these) ---\n%s\n' "$style"
  fi
  if [ "${#diff_parts[@]}" -gt 0 ]; then
    # On-disk handoff. The diff is too large to inline, so it is in your workspace.
    # State the part count and per-part line counts explicitly: that turns "did the
    # reviewer actually read all of it" from an assumption into something checkable.
    printf '\n--- UNIFIED DIFF (ON DISK -- YOU MUST READ IT) ---\n'
    printf 'This PR is too large to inline (%s bytes). The complete unified diff has been\n' "$diff_bytes"
    printf 'written into your workspace, split into %s sequential parts:\n\n' "${#diff_parts[@]}"
    for part in "${diff_parts[@]}"; do
      printf '  %s  (%s lines, %s bytes)\n' "$part" "$(wc -l < "$part")" "$(wc -c < "$part")"
    done
    cat <<'EOF'

Read EVERY part, in order, before writing anything. They are consecutive slices of one
file, so a hunk header can be split across a part boundary -- treat the concatenation as
the diff, not each part as a standalone unit.

Do not review from filenames, paths, or part sizes. Do not extrapolate from a sample. If
you could not read some part, say so explicitly at the top of your review and scope your
verdict to what you did read -- an honest partial review is useful, a confident review of
an unread diff is not.

Begin your output with a line of the form:
  <!-- coverage: read N/M parts -->
EOF
  else
    printf '\n--- UNIFIED DIFF ---\n'
    cat "$diff_file"
  fi
} > "$prompt_file"

# Enforce the single-argument ceiling on the WHOLE prompt (see MAX_PROMPT_BYTES above).
# MAX_DIFF_BYTES alone cannot guarantee this: a long style guide can push the assembled
# prompt past 128 KiB even with a modest diff, and the exec then fails with E2BIG.
if [ "$(wc -c < "$prompt_file")" -gt "$MAX_PROMPT_BYTES" ]; then
  head -c "$MAX_PROMPT_BYTES" "$prompt_file" > "$prompt_file.cut" && mv "$prompt_file.cut" "$prompt_file"
  truncated=$'\n\n> Note: the review prompt was truncated to '"${MAX_PROMPT_BYTES}"$' bytes (single-argument limit).'
  log "prompt truncated to ${MAX_PROMPT_BYTES} bytes"
fi

# Escape hatch for verifying the diff-acquisition and prompt-assembly path (including the
# large-PR fallbacks) without spending an agy run or posting to the PR. Prints the
# assembled prompt to stdout and stops before the agy invocation.
if [ -n "${AGY_DRY_RUN:-}" ]; then
  log "AGY_DRY_RUN set: printing the assembled prompt and exiting before agy runs"
  log "prompt: $(wc -c < "$prompt_file") bytes; diff parts: ${#diff_parts[@]}"
  cat "$prompt_file"
  exit 0
fi

# --- run agy headless, under a PTY (works around agy issue #76: -p drops --------
#     stdout when stdout is not a TTY, e.g. piped/redirected/subprocess) ---------
flags=( --print-timeout "$AGY_PRINT_TIMEOUT" --sandbox --dangerously-skip-permissions )
[ -n "$AGY_MODEL" ]  && flags+=( --model "$AGY_MODEL" )
[ -n "$AGY_EFFORT" ] && flags+=( --effort "$AGY_EFFORT" )

# TRUST MODEL -- read this before changing the trigger conditions.
#
# `--dangerously-skip-permissions` is REQUIRED for headless operation: without it agy
# blocks on an interactive approval prompt that no one is present to answer, and the run
# burns --print-timeout and exits empty. What it removes is the approval gate, so agy
# could act on instructions embedded in the diff it is reviewing (prompt injection).
#
# `--sandbox` is NOT a security boundary and must not be treated as one. Upstream
# antigravity-cli#36 reports that --dangerously-skip-permissions can auto-approve the
# very prompts needed to escape the sandbox, and there is a published prompt-injection
# -> RCE/sandbox-escape writeup against the CLI. It is kept for defense in depth only.
#
# The ACTUAL boundary is "agy only ever sees a same-repo diff", and it takes TWO checks
# because no single one covers both triggers:
#   * the workflow `if:` rejects fork PRs on `pull_request`, and requires
#     OWNER/MEMBER/COLLABORATOR on `issue_comment`;
#   * the isCrossRepository check above rejects fork PRs on the `issue_comment` path,
#     which the workflow cannot do -- that payload carries no head-repo field, so
#     `/agy-review` on a fork PR would otherwise feed an external diff straight in.
#     Authorizing the COMMENTER is not the same as trusting the DIFF, and the diff is
#     what agy ingests.
# With both in place, a diff reaching agy was pushed to a branch of this repository by
# someone who already holds write access -- and therefore has far more direct means
# available than prompt injection. Those two checks are the whole defense: weaken either
# (add `pull_request_target`, drop the fork check, let the metadata lookup fail open) and
# this becomes remote code execution on the maintainer's machine.
#
# Within that model, two cheap reductions are still worth having:
#   * agy runs with GH_TOKEN/GITHUB_TOKEN removed from its environment -- `gh` runs in
#     this script, before and after, and agy has no use for the token.
#   * the conversation-store fallback is gone (see the loop below), so nothing from the
#     shared per-user agy state can be copied into a public comment.
# Neither isolates the host. Doing that properly needs an ephemeral account or VM, which
# is incompatible with the OAuth session in $HOME that makes these reviews free under
# Ultra; if this is ever opened to untrusted diffs, that isolation becomes mandatory.
agy_env=( env -u GH_TOKEN -u GITHUB_TOKEN )

out_file="$(mktemp)"
here="$(cd "$(dirname "$0")" && pwd)"
: > "$LOG"

# Serialize agy across concurrent review jobs on this host. agy runs a SINGLETON
# local language-server + conversation store per user, so two `--print` calls at
# once collide (one reports the backend "unavailable"). flock makes jobs queue
# instead of failing. Best-effort: if the lock can't be taken, proceed anyway.
if command -v flock >/dev/null 2>&1; then
  exec 9>"$AGY_LOCK" 2>/dev/null \
    && flock -w "$AGY_LOCK_WAIT" 9 \
    || log "agy lock unavailable or timed out (${AGY_LOCK_WAIT}s); proceeding unserialized"
fi

# Retry the whole agy attempt on empty/failed output: transient "agy is down"
# (backend rate-limit / local-server contention) usually clears within seconds.
# The flock (above) is held across all attempts, released after the loop.
for (( attempt=1; attempt<=AGY_RETRIES; attempt++ )); do
  : > "$out_file"   # clear any partial output from a prior attempt

  rc=0
  if command -v unbuffer >/dev/null 2>&1; then
    log "running agy via unbuffer (allocates a PTY) [attempt ${attempt}/${AGY_RETRIES}]"
    "${agy_env[@]}" unbuffer "$AGY_BIN" "${flags[@]}" --print "$(cat "$prompt_file")" \
      > "$out_file" 2>>"$LOG" || rc=$?
  else
    log "unbuffer not found; falling back to script(1) [attempt ${attempt}/${AGY_RETRIES}]"
    raw="$(mktemp)"
    # `script -c` takes a COMMAND STRING and runs it through `sh -c`, so every word must be
    # quoted for that inner shell. Interpolating `${flags[*]}` raw (as the upstream template
    # does) is a command-injection surface: any space or metacharacter in AGY_MODEL /
    # AGY_EFFORT / AGY_PRINT_TIMEOUT -- all env-settable -- becomes shell syntax. `printf %q`
    # emits a shell-safe rendering of each element, so the string is exactly the argv we mean.
    cmd="$(printf '%q ' "$here/_agy_print.sh" "$prompt_file" "${flags[@]}")"
    "${agy_env[@]}" AGY_BIN="$AGY_BIN" script -qfec "$cmd" "$raw" >/dev/null 2>>"$LOG" || rc=$?
    col -b < "$raw" > "$out_file"
  fi
  # Not fatal on its own -- agy can exit non-zero and still have printed a usable review,
  # and the retry loop below is the real gate. But record it: a silent `|| true` hid the
  # difference between "agy failed" and "agy produced nothing".
  [ "$rc" -eq 0 ] || log "agy exited non-zero (${rc}); checking output anyway (see $LOG)"

  # normalize CRs without sed -i (avoid in-place edit footguns)
  tr -d '\r' < "$out_file" > "$out_file.clean" && mv "$out_file.clean" "$out_file"

  # --- NO conversation-store fallback, deliberately -----------------------------
  #     The upstream template recovers the answer from agy's SQLite conversation store
  #     when the PTY trick yields nothing (belt-and-suspenders for agy issue #76). That
  #     store is SHARED per-user: `$HOME/.gemini/antigravity-cli/conversations` holds
  #     every session on the host, including the owner's interactive chats and reviews
  #     for other repos. Reading it and posting the result to a PUBLIC pull request
  #     comment risks publishing an unrelated conversation.
  #
  #     Narrowing by mtime is not sufficient -- it bounds a time window, not ownership,
  #     and the flock above serializes review JOBS, not the owner's own agy usage. The
  #     only sound version of this fallback needs a conversation ID (or a private store)
  #     tied to THIS invocation, and agy exposes neither: the OAuth session lives in
  #     $HOME, so it cannot be relocated per run without losing the login that makes
  #     these reviews free under Ultra.
  #
  #     So it fails closed instead: no usable stdout means retry, then fail the job.
  #     Losing a review is recoverable (`/agy-review` re-runs it); publishing someone
  #     else's session into a public comment is not.

  have_text "$out_file" && break
  if [ "$attempt" -lt "$AGY_RETRIES" ]; then
    delay=$(( AGY_RETRY_DELAY * attempt ))
    log "no usable output (attempt ${attempt}/${AGY_RETRIES}); retrying in ${delay}s"
    sleep "$delay"
  fi
done
exec 9>&- 2>/dev/null || true    # release the agy lock so the next queued job proceeds

if ! have_text "$out_file"; then
  log "no review output after ${AGY_RETRIES} attempt(s). Check $LOG and confirm 'agy -p \"hi\"' works for this user."
  exit 1
fi

# --- assemble the comment body -------------------------------------------------
body_file="$(mktemp)"
{
  printf '%s\n' "$MARKER"
  printf '## Antigravity review (Gemini via Ultra)\n\n'
  cat "$out_file"
  printf '%s' "$truncated"
  printf '\n\n<sub>Automated first-pass review by `agy` on a self-hosted runner -- not a human review.</sub>\n'
} > "$body_file"

# --- replace any prior review comment, then post fresh -------------------------
# A failed delete is logged, not swallowed: silently ignoring it would let a transient API/perms
# error leave the old comment in place AND post a new one, so runs accumulate duplicates.
#
# The author filter is a correctness requirement, not a nicety: the marker is plain text in a
# public comment, so matching on the marker ALONE lets anyone who pastes it into a comment have
# this workflow delete that comment on the next run -- the job holds `issues: write`, so the
# delete succeeds. Restricting to the bot that posts these comments keeps deletion to our own.
gh api "repos/${REPO}/issues/${PR}/comments" --paginate \
    --jq ".[] | select(.user.type == \"Bot\" and .user.login == \"github-actions[bot]\") | select(.body | contains(\"${MARKER}\")) | .id" 2>/dev/null \
  | while read -r cid; do
      [ -n "$cid" ] || continue
      if ! gh api -X DELETE "repos/${REPO}/issues/comments/${cid}" >/dev/null 2>&1; then
        log "warning: could not delete prior review comment ${cid}; a duplicate may result"
      fi
    done

gh pr comment "$PR" --repo "$REPO" --body-file "$body_file"
log "posted review to ${REPO}#${PR}"
