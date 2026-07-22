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
AGY_PRINT_TIMEOUT="${AGY_PRINT_TIMEOUT:-5m}"
MAX_DIFF_BYTES="${MAX_DIFF_BYTES:-90000}"  # truncate very large diffs (~90 KB)
# Hard ceiling on the ASSEMBLED prompt. The prompt reaches agy as one argv string, and
# Linux caps a single argument at MAX_ARG_STRLEN = 32 * PAGE_SIZE = 128 KiB regardless of
# ARG_MAX; exceeding it fails the exec with E2BIG. Capping only the diff is not enough --
# the boilerplate and the style guide ride in the same string.
MAX_PROMPT_BYTES="${MAX_PROMPT_BYTES:-120000}"
STYLE_GUIDE="${STYLE_GUIDE:-.github/agy-review.md}"  # repo-relative; loaded if present
                                           # (dedicated name -- avoids colliding with GEMINI.md/AGENTS.md)
CONV_DIR="${CONV_DIR:-$HOME/.gemini/antigravity-cli/conversations}"
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
diff_file= meta_file= prompt_file= out_file= raw= body_file=
trap 'rm -f "$diff_file" "$meta_file" "$prompt_file" "$out_file" "$raw" "$body_file"' EXIT

# --- fetch the diff + metadata -------------------------------------------------
diff_file="$(mktemp)"; meta_file="$(mktemp)"
gh pr diff "$PR" --repo "$REPO" > "$diff_file" || { log "gh pr diff failed"; exit 1; }
gh pr view "$PR" --repo "$REPO" --json title > "$meta_file" 2>/dev/null || echo '{}' > "$meta_file"

if ! have_text "$diff_file"; then log "empty diff; nothing to review"; exit 0; fi

truncated=""
if [ "$(wc -c < "$diff_file")" -gt "$MAX_DIFF_BYTES" ]; then
  head -c "$MAX_DIFF_BYTES" "$diff_file" > "$diff_file.cut" && mv "$diff_file.cut" "$diff_file"
  truncated=$'\n\n> Note: the diff was truncated to '"${MAX_DIFF_BYTES}"$' bytes for this review.'
  log "diff truncated to ${MAX_DIFF_BYTES} bytes"
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
  printf '\n--- UNIFIED DIFF ---\n'
  cat "$diff_file"
} > "$prompt_file"

# Enforce the single-argument ceiling on the WHOLE prompt (see MAX_PROMPT_BYTES above).
# MAX_DIFF_BYTES alone cannot guarantee this: a long style guide can push the assembled
# prompt past 128 KiB even with a modest diff, and the exec then fails with E2BIG.
if [ "$(wc -c < "$prompt_file")" -gt "$MAX_PROMPT_BYTES" ]; then
  head -c "$MAX_PROMPT_BYTES" "$prompt_file" > "$prompt_file.cut" && mv "$prompt_file.cut" "$prompt_file"
  truncated=$'\n\n> Note: the review prompt was truncated to '"${MAX_PROMPT_BYTES}"$' bytes (single-argument limit).'
  log "prompt truncated to ${MAX_PROMPT_BYTES} bytes"
fi

# --- run agy headless, under a PTY (works around agy issue #76: -p drops --------
#     stdout when stdout is not a TTY, e.g. piped/redirected/subprocess) ---------
flags=( --print-timeout "$AGY_PRINT_TIMEOUT" --sandbox --dangerously-skip-permissions )
[ -n "$AGY_MODEL" ]  && flags+=( --model "$AGY_MODEL" )
[ -n "$AGY_EFFORT" ] && flags+=( --effort "$AGY_EFFORT" )

# `--dangerously-skip-permissions` is REQUIRED for headless operation -- without it agy
# blocks on an interactive approval prompt that no one is there to answer, and the run
# just times out. What it removes is the approval gate, so the agent could act on
# instructions embedded in the (attacker-controlled) diff it is reviewing. The exposure
# that actually matters is the GitHub token in this job's environment, so agy is launched
# WITHOUT it: `gh` calls happen in this script, before and after, and agy never needs it.
# `--sandbox` still confines filesystem and network access.
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
  # Reference instant for the SQLite fallback below: only a conversation DB modified at
  # or after this point can plausibly belong to this attempt.
  attempt_start="$(date +%s)"

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

  # --- fallback: recover the answer from agy's conversation SQLite store --------
  #     (belt-and-suspenders for issue #76 on hosts where the PTY trick still
  #     yields nothing). The schema is NOT officially documented and can change
  #     between agy versions -- inspect with `sqlite3 <db> .schema` and adjust.
  #
  #     SCOPED TO THIS ATTEMPT ON PURPOSE. Taking the globally newest DB (the upstream
  #     template's `ls -t | head -1`) means that when agy fails BEFORE creating a
  #     conversation, this posts the last assistant message from whatever unrelated
  #     session ran on this host -- another repo's review, or the owner's interactive
  #     chat -- straight into a public PR comment. `find -newermt` restricts the search
  #     to a DB this attempt actually touched; if there is none, the fallback yields
  #     nothing and the retry/failure path takes over. (`find` also avoids the `ls`
  #     parsing that shellcheck SC2012 flags.)
  if ! have_text "$out_file"; then
    log "print output empty; trying SQLite conversation fallback"
    if command -v sqlite3 >/dev/null 2>&1 && [ -d "$CONV_DIR" ]; then
      db="$(find "$CONV_DIR" -maxdepth 1 -name '*.db' -newermt "@$(( attempt_start - 1 ))" \
              -printf '%T@ %p\n' 2>/dev/null | sort -rn | head -1 | cut -d' ' -f2- || true)"
      if [ -z "${db:-}" ]; then
        log "no conversation DB from this attempt; skipping fallback (refusing to read an unrelated session)"
      fi
      if [ -n "${db:-}" ]; then
        for q in \
          "SELECT text FROM messages WHERE role='assistant' ORDER BY rowid DESC LIMIT 1;" \
          "SELECT content FROM messages WHERE role='assistant' ORDER BY rowid DESC LIMIT 1;" \
          "SELECT body FROM message WHERE role='assistant' ORDER BY rowid DESC LIMIT 1;"; do
          sqlite3 "$db" "$q" > "$out_file" 2>/dev/null && have_text "$out_file" && break
        done
      fi
    fi
  fi

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
