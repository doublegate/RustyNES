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
MAX_DIFF_BYTES="${MAX_DIFF_BYTES:-200000}" # truncate very large diffs (~200 KB)
STYLE_GUIDE="${STYLE_GUIDE:-.github/agy-review.md}"  # repo-relative; loaded if present
                                           # (dedicated name -- avoids colliding with GEMINI.md/AGENTS.md)
CONV_DIR="${CONV_DIR:-$HOME/.gemini/antigravity-cli/conversations}"
LOG="${RUNNER_TEMP:-/tmp}/agy-review.log"
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
    [ -n "$is_pr" ] || { log "comment is not on a PR; skipping"; exit 0; }
    case "$body" in
      /agy-review*) : ;;
      *) log "comment is not an /agy-review command; skipping"; exit 0 ;;
    esac
    PR="$(jq -r '.issue.number' "$GITHUB_EVENT_PATH")"
    ;;
  *)
    PR="${1:-}"
    [ -n "$PR" ] || { log "unknown event; pass a PR number as \$1"; exit 1; }
    ;;
esac
log "reviewing ${REPO}#${PR}"

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

# --- run agy headless, under a PTY (works around agy issue #76: -p drops --------
#     stdout when stdout is not a TTY, e.g. piped/redirected/subprocess) ---------
flags=( --print-timeout "$AGY_PRINT_TIMEOUT" --sandbox --dangerously-skip-permissions )
[ -n "$AGY_MODEL" ]  && flags+=( --model "$AGY_MODEL" )
[ -n "$AGY_EFFORT" ] && flags+=( --effort "$AGY_EFFORT" )

out_file="$(mktemp)"
here="$(cd "$(dirname "$0")" && pwd)"
: > "$LOG"

if command -v unbuffer >/dev/null 2>&1; then
  log "running agy via unbuffer (allocates a PTY)"
  unbuffer "$AGY_BIN" "${flags[@]}" --print "$(cat "$prompt_file")" > "$out_file" 2>>"$LOG" || true
else
  log "unbuffer not found; falling back to script(1)"
  raw="$(mktemp)"
  AGY_BIN="$AGY_BIN" script -qfec "$here/_agy_print.sh '$prompt_file' ${flags[*]}" "$raw" >/dev/null 2>>"$LOG" || true
  col -b < "$raw" > "$out_file"
fi

# normalize CRs without sed -i (avoid in-place edit footguns)
tr -d '\r' < "$out_file" > "$out_file.clean" && mv "$out_file.clean" "$out_file"

# --- fallback: recover the answer from agy's conversation SQLite store ----------
#     (belt-and-suspenders for issue #76 on hosts where the PTY trick still
#     yields nothing). The schema is NOT officially documented and can change
#     between agy versions -- inspect with `sqlite3 <db> .schema` and adjust.
if ! have_text "$out_file"; then
  log "print output empty; trying SQLite conversation fallback"
  if command -v sqlite3 >/dev/null 2>&1 && [ -d "$CONV_DIR" ]; then
    db="$(ls -t "$CONV_DIR"/*.db 2>/dev/null | head -1 || true)"
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

if ! have_text "$out_file"; then
  log "no review output produced. Check $LOG and confirm 'agy -p \"hi\"' works for this user."
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
gh api "repos/${REPO}/issues/${PR}/comments" --paginate \
    --jq ".[] | select(.body | contains(\"${MARKER}\")) | .id" 2>/dev/null \
  | while read -r cid; do
      [ -n "$cid" ] && gh api -X DELETE "repos/${REPO}/issues/comments/${cid}" >/dev/null 2>&1 || true
    done

gh pr comment "$PR" --repo "$REPO" --body-file "$body_file"
log "posted review to ${REPO}#${PR}"
