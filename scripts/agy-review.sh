#!/usr/bin/env bash
#
# agy-review.sh -- headless GitHub PR reviewer driven by Antigravity CLI (`agy`).
#
# Runs on a SELF-HOSTED GitHub Actions runner where `agy` is logged in via Google
# OAuth, so reviews draw on your Google AI Ultra rate limits (no metered API key).
#
# Flow: resolve PR -> `gh pr diff` -> adversarial-reviewer prompt (+ style guide)
#       -> `agy --print` under a PTY (argv-safe) -> post via `gh pr comment`.
#
# Security: the workflow gates who can trigger this (same-repo PRs only; comments
# only from OWNER/MEMBER/COLLABORATOR). This script re-checks the comment gate
# defensively. See ../README.md.
set -euo pipefail

# --- configuration (env-overridable from the workflow) ------------------------
AGY_BIN="${AGY_BIN:-agy}"
command -v "$AGY_BIN" >/dev/null 2>&1 || AGY_BIN="$HOME/.local/bin/agy"
AGY_MODEL="${AGY_MODEL:-}"                 # empty = agy's configured default
AGY_EFFORT="${AGY_EFFORT:-high}"           # low|medium|high
AGY_PRINT_TIMEOUT="${AGY_PRINT_TIMEOUT:-5m}"
MAX_DIFF_BYTES="${MAX_DIFF_BYTES:-200000}" # truncate very large diffs (bounds ARG_MAX too)
STYLE_GUIDE="${STYLE_GUIDE:-.github/agy-review.md}"
MARKER="<!-- antigravity-pr-review -->"

# --- temp files + cleanup trap (no leftover scratch on the runner) ------------
diff_file= meta_file= prompt_file= out_file= body_file= LOG=
cleanup() {
  local f
  for f in "$diff_file" "$meta_file" "$prompt_file" "$out_file" "$body_file" "$LOG"; do
    [ -n "$f" ] && rm -f "$f"
  done
}
trap cleanup EXIT
LOG="$(mktemp)"

log() { printf '[agy-review] %s\n' "$*" >&2; }
have_text() { [ -s "$1" ] && grep -q '[^[:space:]]' "$1"; }
die() { log "$*"; [ -s "$LOG" ] && { log "--- agy stderr ---"; cat "$LOG" >&2; }; exit 1; }

REPO="${GITHUB_REPOSITORY:?GITHUB_REPOSITORY not set}"

# --- resolve the PR number; re-verify the comment gate defensively ------------
case "${GITHUB_EVENT_NAME:-}" in
  pull_request|pull_request_target)
    PR="$(jq -r '.pull_request.number' "$GITHUB_EVENT_PATH")"
    ;;
  issue_comment)
    is_pr="$(jq -r '.issue.pull_request // empty' "$GITHUB_EVENT_PATH")"
    body="$(jq -r '.comment.body // ""' "$GITHUB_EVENT_PATH")"
    assoc="$(jq -r '.comment.author_association // ""' "$GITHUB_EVENT_PATH")"
    [ -n "$is_pr" ] || { log "comment not on a PR; skipping"; exit 0; }
    case "$body" in /agy-review*) : ;; *) log "not an /agy-review command; skipping"; exit 0 ;; esac
    case "$assoc" in
      OWNER|MEMBER|COLLABORATOR) : ;;
      *) log "comment author association '$assoc' lacks write access; skipping"; exit 0 ;;
    esac
    PR="$(jq -r '.issue.number' "$GITHUB_EVENT_PATH")"
    ;;
  *)
    PR="${1:-}"
    [ -n "$PR" ] || die "unknown event; pass a PR number as \$1"
    ;;
esac
log "reviewing ${REPO}#${PR}"

# --- fetch the diff + metadata ------------------------------------------------
diff_file="$(mktemp)"; meta_file="$(mktemp)"
gh pr diff "$PR" --repo "$REPO" > "$diff_file" || die "gh pr diff failed"
gh pr view "$PR" --repo "$REPO" --json title > "$meta_file" 2>/dev/null || echo '{}' > "$meta_file"
have_text "$diff_file" || { log "empty diff; nothing to review"; exit 0; }

truncated=""
if [ "$(wc -c < "$diff_file")" -gt "$MAX_DIFF_BYTES" ]; then
  head -c "$MAX_DIFF_BYTES" "$diff_file" > "$diff_file.cut" && mv "$diff_file.cut" "$diff_file"
  truncated=$'\n\n> Note: the diff was truncated to '"${MAX_DIFF_BYTES}"$' bytes for this review.'
  log "diff truncated to ${MAX_DIFF_BYTES} bytes"
fi

# --- build the prompt ---------------------------------------------------------
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
  [ -n "$style" ] && printf '\n--- PROJECT STYLE GUIDE (enforce these) ---\n%s\n' "$style"
  printf '\n--- UNIFIED DIFF ---\n'
  cat "$diff_file"
} > "$prompt_file"

# --- run agy headless under a PTY (works around agy issue #76: -p drops --------
#     stdout on a non-TTY). Both paths pass an argv ARRAY -- no shell string --
#     so env-provided flags cannot inject a command or word-split. -------------
flags=( --print-timeout "$AGY_PRINT_TIMEOUT" --sandbox --dangerously-skip-permissions )
[ -n "$AGY_MODEL" ]  && flags+=( --model "$AGY_MODEL" )
[ -n "$AGY_EFFORT" ] && flags+=( --effort "$AGY_EFFORT" )

out_file="$(mktemp)"
here="$(cd "$(dirname "$0")" && pwd)"
prompt="$(cat "$prompt_file")"
rc=0
if command -v unbuffer >/dev/null 2>&1; then
  unbuffer "$AGY_BIN" "${flags[@]}" --print "$prompt" > "$out_file" 2>>"$LOG" || rc=$?
elif command -v python3 >/dev/null 2>&1; then
  python3 "$here/_agy_pty.py" "$AGY_BIN" "${flags[@]}" --print "$prompt" > "$out_file" 2>>"$LOG" || rc=$?
else
  die "need 'unbuffer' (from the 'expect' package) or python3 for a PTY; neither found"
fi
[ "$rc" -eq 0 ] || log "agy exited non-zero ($rc); checking output anyway"
tr -d '\r' < "$out_file" > "$out_file.clean" && mv "$out_file.clean" "$out_file"

have_text "$out_file" || die "no review output from agy (rc=$rc). Confirm 'agy -p \"hi\"' works for this user."

# --- assemble the comment body ------------------------------------------------
body_file="$(mktemp)"
{
  printf '%s\n' "$MARKER"
  printf '## Antigravity review (Gemini via Ultra)\n\n'
  cat "$out_file"
  printf '%s' "$truncated"
  printf '\n\n<sub>Automated first-pass review by `agy` on a self-hosted runner -- not a human review.</sub>\n'
} > "$body_file"

# --- replace any prior review comment, then post fresh ------------------------
# Comment deletion is best-effort by design (a delete failure must not block the
# new review), so its errors are intentionally not fatal.
gh api "repos/${REPO}/issues/${PR}/comments" --paginate \
    --jq ".[] | select(.body | contains(\"${MARKER}\")) | .id" 2>/dev/null \
  | while read -r cid; do
      [ -n "$cid" ] && gh api -X DELETE "repos/${REPO}/issues/comments/${cid}" >/dev/null 2>&1 || true
    done

gh pr comment "$PR" --repo "$REPO" --body-file "$body_file"
log "posted review to ${REPO}#${PR}"
