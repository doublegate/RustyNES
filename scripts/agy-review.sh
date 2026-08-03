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

# Helpers are defined FIRST: the configuration block below calls log() from the MAX_PROMPT_BYTES
# clamp, so log() must already exist. (Defined later, a clamp that fires would die with
# `log: command not found` under set -e instead of warning — a latent misconfiguration trap.)
log() { printf '[agy-review] %s\n' "$*" >&2; }
have_text() { [ -s "$1" ] && grep -q '[^[:space:]]' "$1"; }
# Guarding against a leak of agy's interactive Google OAuth login flow. When agy's cached
# session lapses on the runner, `--print` emits the login prompt (a live OAuth URL + "paste the
# authorization code here") instead of a review; that text is non-empty, so have_text() alone
# would treat it as a valid review and POST it — leaking the URL into a public PR comment (the
# incident this guards against).
#
# The guard keys on the ONE artifact that reliably distinguishes agy's login flow from any
# review: a live Google OAuth *authorization URL* (scheme + host + endpoint path). This choice
# is deliberate and is the convergence point of several rounds of agy's own adversarial review:
#   * It does NOT false-positive on reviewing the reviewer or on an OAuth-related PR. A code
#     review quotes this script's BARE regex ("accounts.google.com/o/oauth2", no scheme) or
#     discusses auth in prose; neither contains the "https://…/o/oauth2" URL form. (Earlier
#     signature/prompt-phrase heuristics wrongly flagged such reviews.)
#   * It needs NO "is this a review?" exemption, so it CANNOT be disarmed by a section header
#     (or the phrase "blocking issues") appearing in the same output as a URL — a false
#     NEGATIVE would be a leak. (Earlier "### Blocking issues" exemptions had exactly this hole,
#     and were also brittle to header case / trailing punctuation.)
# So this is both necessary (agy's login flow always prints the URL) and sufficient (nothing
# else legitimately carries a live authorization URL), with no heuristic that flips between
# false-positive and false-negative.
#
# The regex requires the scheme (`https?://`) and the host + `/o/oauth2` endpoint prefix, but
# deliberately NOT a trailing slash: agy round-4 flagged that a `.../o/oauth2/` requirement
# would miss a URL emitted as `.../o/oauth2?client_id=…` or bare `.../o/oauth2` (a leak). The
# scheme is what keeps a review's bare-pattern quote from matching, so dropping the trailing
# slash loses no safety.
OAUTH_URL_RE='https?://accounts\.google\.com/o/oauth2'

# The single guard, used at BOTH the retry-loop capture (Layer 1) and the assembled body before
# posting (Layer 3): true iff a text contains a live OAuth authorization URL. There is no
# separate heuristic and no "is this a review?" exemption — earlier a secondary "authentication
# failed or timed out" substring fallback was tried, but agy round-4 flagged it (correctly) as
# broad enough to abort a genuine review that merely discusses auth-timeout handling. The live
# URL is always present in a real login flow, so it alone is both necessary and sufficient; a
# lapse is detected AND a leak is blocked by the same unconditional check.
oauth_url_present() { [ -s "$1" ] && grep -qiE "$OAUTH_URL_RE" "$1"; }

# --- configuration (all env-overridable from the workflow) ---------------------
AGY_BIN="${AGY_BIN:-agy}"
command -v "$AGY_BIN" >/dev/null 2>&1 || AGY_BIN="$HOME/.local/bin/agy"
AGY_MODEL="${AGY_MODEL:-}"                 # empty = agy's configured default (Gemini 3.x Pro)
AGY_EFFORT="${AGY_EFFORT:-high}"           # low|medium|high
AGY_PRINT_TIMEOUT="${AGY_PRINT_TIMEOUT:-5m}"
AGY_DIFF_MODE="${AGY_DIFF_MODE:-auto}"     # auto|inline|file. A diff is passed to agy either inlined
                                           # in the --print prompt, or written to a FILE agy reads with
                                           # its own tools. `auto` inlines a diff that fits under the
                                           # arg-size budget and files anything larger (so large PRs are
                                           # never truncated); `inline`/`file` force one path.
MAX_DIFF_BYTES="${MAX_DIFF_BYTES:-5000000}" # sanity cap on a pathological diff (5 MB). No longer the
                                           # arg-size limit -- a large diff goes to agy as a file, not
                                           # as an argv value -- just a guard against a runaway diff.
MAX_PROMPT_BYTES="${MAX_PROMPT_BYTES:-125000}" # hard ceiling on the INLINED prompt: agy takes it as a
                                           # --print VALUE (not stdin), and a single execve argument
                                           # cannot exceed MAX_ARG_STRLEN (PAGE_SIZE*32 = 128 KiB on
                                           # Linux). Over it, execve fails with E2BIG before agy even
                                           # starts. In `auto` mode this is the inline/file threshold;
                                           # it also backstops the assembled prompt in every mode.
ARG_SIZE_CEILING=128000                     # hard cap: a configured MAX_PROMPT_BYTES above the
                                           # MAX_ARG_STRLEN-derived safe bound would defeat the guard
                                           # and re-expose E2BIG, so clamp any override down to it.
# Require a POSITIVE integer at or below the ceiling. The `-gt 0` half is load-bearing, not
# cosmetic: a negative override (e.g. MAX_PROMPT_BYTES=-1) satisfies `-le "$ARG_SIZE_CEILING"`,
# so without it the clamp is skipped and the `head -c "$MAX_PROMPT_BYTES"` prompt cap below runs
# as GNU `head -c -1` (print all-but-last-byte) — silently defeating the E2BIG backstop. A
# non-numeric value trips the `2>/dev/null` and is clamped too.
if ! { [ "$MAX_PROMPT_BYTES" -gt 0 ] && [ "$MAX_PROMPT_BYTES" -le "$ARG_SIZE_CEILING" ]; } 2>/dev/null; then
  log "MAX_PROMPT_BYTES='${MAX_PROMPT_BYTES}' invalid, non-positive, or above the ${ARG_SIZE_CEILING}-byte ceiling; clamping"
  MAX_PROMPT_BYTES="$ARG_SIZE_CEILING"
fi
STYLE_GUIDE="${STYLE_GUIDE:-.github/agy-review.md}"  # repo-relative; loaded if present
                                           # (dedicated name -- avoids colliding with GEMINI.md/AGENTS.md)
# Per-run log path. A FIXED name would collide between concurrent jobs whenever
# RUNNER_TEMP is unset (local runs fall back to /tmp, which is shared) -- and a
# predictable /tmp path is a symlink/file-tampering target. The run-id / PID
# suffix keeps it unique per run.
LOG="${RUNNER_TEMP:-/tmp}/agy-review-${GITHUB_RUN_ID:-$$}.log"
AGY_LOCK="${AGY_LOCK:-$HOME/.gemini/antigravity-cli/.agy-review.lock}"
AGY_LOCK_WAIT="${AGY_LOCK_WAIT:-600}"      # seconds to wait for the agy lock before proceeding
AGY_RETRIES="${AGY_RETRIES:-3}"            # attempts to get a usable agy response
AGY_RETRY_DELAY="${AGY_RETRY_DELAY:-15}"   # base backoff seconds between retries (grows per attempt)
MARKER="<!-- antigravity-pr-review -->"

# The jq program that picks which prior review comments to delete. Named, and exercised directly
# by `scripts/agy-review-selftest.sh`, because this filter has now been wrong TWICE in ways
# nothing observed: first the just-posted comment was not excluded (so it deleted itself), then
# jq's `--arg` was handed to `gh api`, which has no such flag (so the whole step died silently and
# stale comments accumulated). Both were invisible from the outside — the review still posted.
#
# The two `select`s that matter: the AUTHOR filter (without it, any user could put the marker in a
# comment and have this bot delete arbitrary comments) and the ID exclusion (without it, the run
# deletes the comment it just published).
SELECT_STALE_JQ='.[]
  | select(.user.type == "Bot" and .user.login == "github-actions[bot]")
  | select(.body | contains($marker))
  | select(.id != $new_id)
  | .id'
readonly SELECT_STALE_JQ

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
    # script is also runnable by hand and by any future caller, so re-check here that the
    # commenter has write access. A stranger's `/agy-review` must never schedule an agy
    # run on the self-hosted host — losing this to a workflow-only gate would be silent.
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
diff_file= diff_err= meta_file= prompt_file= out_file= raw= body_file= agy_diff_file= agy_work_dir=
# Set to 1 if agy printed its interactive OAuth login flow instead of a review (lapsed session);
# gates the no-post abort below. Pre-declared so `${auth_failed:-0}` is set-u-safe on every path.
auth_failed=0
# Set when the large-diff fallback below creates refs/agy/* so the trap can remove them.
agy_refs_created=
cleanup() {
  rm -f "$diff_file" "$diff_err" "$meta_file" "$prompt_file" "$out_file" "$raw" "$body_file" "$agy_diff_file"
  # Remove the gitignored diff-handoff scratch dir once its file is gone. `rmdir` only unlinks an
  # empty dir, so a concurrent run's file (a different $$) is never clobbered; a non-empty dir is
  # gitignored and harmless if left behind.
  [ -n "$agy_work_dir" ] && rmdir "$agy_work_dir" 2>/dev/null || true
  if [ -n "$agy_refs_created" ] && [ -n "${PR:-}" ]; then
    git update-ref -d "refs/agy/pr-${PR}" 2>/dev/null || true
    git update-ref -d "refs/agy/base-${PR}" 2>/dev/null || true
  fi
}
trap cleanup EXIT

# --- fetch metadata first, because the fork gate + the large-diff fallback depend on it ---
# FAIL-CLOSED. A `{}` fallback was harmless when the only field read was the title,
# but is NOT harmless now that isCrossRepository gates whether an untrusted diff
# reaches agy: a lookup failure must never be indistinguishable from "same-repo".
diff_file="$(mktemp)"; meta_file="$(mktemp)"; diff_err="$(mktemp)"
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

# --- fetch the diff, with a fallback for a diff over GitHub's API line limit -----
# `gh pr diff` can fail for two very different reasons and they must not be
# conflated. A genuine error (auth, network, bad PR) is fatal. But GitHub's API
# also refuses any diff over 20,000 lines with HTTP 406, and that is not an error
# -- it just means the PR is too large to fetch through the API. A large PR is
# exactly the one worth reviewing, so fall back to computing the diff locally.
#
# SECURITY: this fetches the PR's objects but NEVER checks them out. The working
# tree is untouched, and the PR content is treated exactly as the API diff was --
# read-only bytes that become prompt text and are never executed. So the fallback
# does not widen the trust boundary the workflow already sets: whatever governs
# whether a given PR's diff is allowed to reach agy at all (the workflow `if:`
# author-association gate; see the trust model at the agy invocation below) is
# unchanged, and this only changes HOW an already-permitted diff is obtained when
# it is too large for the API. `refs/agy/*` are private namespaces (cannot clobber
# a real branch) and are removed on exit. Auth goes through `http.extraheader` for
# the one fetch rather than a persisted credential, so a `persist-credentials:
# false` checkout stays intact; a hand-run without GH_TOKEN falls through to git's
# ambient credential helper.
# ($diff_err was allocated alongside $diff_file / $meta_file above, so the cleanup
# trap never references it before it exists.)
if ! gh pr diff "$PR" --repo "$REPO" > "$diff_file" 2>"$diff_err"; then
  if grep -qi 'diff exceeded the maximum number of lines' "$diff_err"; then
    base_ref="$(jq -r '.baseRefName // empty' "$meta_file")"
    if [ -z "$base_ref" ] || [ "$base_ref" = "null" ]; then
      log "diff exceeds the API limit and the base branch is unknown; cannot fall back"
      exit 1
    fi
    log "diff exceeds GitHub's 20,000-line API limit; falling back to a local git diff"
    pr_ref="refs/agy/pr-${PR}"
    base_local="refs/agy/base-${PR}"
    agy_refs_created=1
    fetch_refspecs=( "+refs/pull/${PR}/head:${pr_ref}" "+refs/heads/${base_ref}:${base_local}" )
    # `bearer` is what Actions' GITHUB_TOKEN accepts; a personal token from
    # `gh auth token` is rejected ("remote: invalid credentials"), so a hand-run
    # falls through to git's ambient credentials. Neither path persists anything.
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

truncated=""
if [ "$(wc -c < "$diff_file")" -gt "$MAX_DIFF_BYTES" ]; then
  head -c "$MAX_DIFF_BYTES" "$diff_file" > "$diff_file.cut" && mv "$diff_file.cut" "$diff_file"
  truncated=$'\n\n> Note: the diff exceeded '"${MAX_DIFF_BYTES}"$' bytes and was truncated for this review.'
  log "diff truncated to the ${MAX_DIFF_BYTES}-byte sanity cap"
fi

# --- build the prompt ----------------------------------------------------------
title="$(jq -r '.title // ""' "$meta_file")"
# Bound the style guide so it can never fill the whole arg budget and crowd out the diff -- or, in
# file mode, the file pointer that the MAX_PROMPT_BYTES guard would otherwise truncate away, leaving
# agy with no diff at all. Reserve headroom for the instructions, the diff header, and the pointer.
# head -c is byte-accurate (a shell substring is by character, which is wrong for multi-byte UTF-8).
style=""
if [ -f "$STYLE_GUIDE" ]; then
  style_cap=$(( MAX_PROMPT_BYTES - 8192 )); [ "$style_cap" -lt 0 ] && style_cap=0
  style="$(head -c "$style_cap" "$STYLE_GUIDE")"
  [ "$(wc -c < "$STYLE_GUIDE")" -gt "$style_cap" ] && log "STYLE_GUIDE capped to ${style_cap} bytes so the diff / file pointer always fits under the arg budget"
fi

# The instruction HEAD (everything except the diff body). agy takes the whole prompt as one --print
# argv value, capped at MAX_ARG_STRLEN (128 KiB), so a diff that would push the prompt over the budget
# is handed to agy as a FILE it reads with its own tools instead of being inlined or truncated. Small
# diffs still inline (the proven path); only a large PR takes the file path -- and a large PR used to
# fail outright with E2BIG, so the file path can only improve on that.
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
} > "$prompt_file"

# Decide inline vs file. Budget: keep the whole argv prompt under MAX_PROMPT_BYTES (itself clamped
# below the 128 KiB ceiling), reserving a small margin for the diff header. `file` mode writes the
# diff into agy's working directory (the repo checkout) and points the prompt at it by name.
head_bytes="$(wc -c < "$prompt_file")"
diff_bytes="$(wc -c < "$diff_file")"
inline_budget=$(( MAX_PROMPT_BYTES - head_bytes - 512 ))   # margin covers the diff header + file notice
[ "$inline_budget" -lt 0 ] && inline_budget=0             # a huge STYLE_GUIDE can exceed it: file the diff
use_file=0
case "$AGY_DIFF_MODE" in
  file)   use_file=1 ;;
  inline) use_file=0 ;;
  *)      [ "$diff_bytes" -gt "$inline_budget" ] && use_file=1 ;;
esac

if [ "$use_file" = "1" ]; then
  # agy's CWD is the repo checkout, so a file written under it is readable by its file tool. Write it
  # into a dedicated, gitignored scratch dir (`.agy-review-work/`, listed in .gitignore) rather than
  # `.git/` (a sandbox may deny tool access to the hidden `.git` dir) or the repo root (an untracked
  # `.patch` there pollutes `git status` if agy inspects repo state). Being gitignored, the dir never
  # shows as working-tree pollution; the EXIT trap removes the file and the (now-empty) dir.
  #
  # The prompt hands agy an ABSOLUTE path, and the run below adds "$PWD" to agy's workspace via
  # --add-dir. Both are load-bearing and were proven on the live runner: agy's sandboxed file tool
  # does NOT resolve a relative path against the shell CWD (it uses its own workspace root), so a
  # relative `.agy-review-work/...` comes back "does not exist on the filesystem" — the exact
  # large-diff review failure this fixes. An absolute path resolves on its own, and --add-dir makes
  # the checkout part of the sandbox workspace; using both is belt-and-suspenders.
  agy_work_dir="$PWD/.agy-review-work"
  mkdir -p "$agy_work_dir"
  diff_name=".agy-review-work/agy-review-diff.$$.patch"
  agy_diff_file="$PWD/$diff_name"
  cp "$diff_file" "$agy_diff_file"
  {
    printf '\n--- UNIFIED DIFF (in a file) ---\n'
    printf 'The full unified diff for this PR is in the file at the absolute path `%s`\n' "$agy_diff_file"
    printf '(it is too large to inline). Read that file IN FULL with your file-reading tool first, then\n'
    printf 'produce the review above from its actual contents. Do not review from the PR title alone.\n'
  } >> "$prompt_file"
  log "diff is ${diff_bytes} bytes (> ${inline_budget}-byte inline budget); handing it to agy as ${agy_diff_file}"
else
  # Forced inline (AGY_DIFF_MODE=inline) on an over-budget diff: the MAX_PROMPT_BYTES guard below
  # still prevents E2BIG by truncating, but warn since `auto` would have filed it in full instead.
  [ "$diff_bytes" -gt "$inline_budget" ] && log "warning: forced inline with a ${diff_bytes}-byte diff over the ${inline_budget}-byte budget; the prompt will be truncated -- use AGY_DIFF_MODE=auto to file it in full"
  { printf '\n--- UNIFIED DIFF ---\n'; cat "$diff_file"; } >> "$prompt_file"
  log "diff is ${diff_bytes} bytes; inlined into the prompt"
fi

# --- guard the argv size (E2BIG) -----------------------------------------------
# agy takes the prompt as a --print VALUE, so the whole prompt is one execve argument
# and must stay under MAX_ARG_STRLEN (128 KiB). MAX_DIFF_BYTES bounds the diff, but the
# boilerplate + style guide ride on top, so cap the assembled prompt as a hard backstop.
if [ "$(wc -c < "$prompt_file")" -gt "$MAX_PROMPT_BYTES" ]; then
  head -c "$MAX_PROMPT_BYTES" "$prompt_file" > "$prompt_file.cut" && mv "$prompt_file.cut" "$prompt_file"
  truncated+=$'\n\n> Note: the review prompt was capped to '"${MAX_PROMPT_BYTES}"$' bytes (execve arg-size limit).'
  log "prompt capped to ${MAX_PROMPT_BYTES} bytes (execve arg-size ceiling)"
fi
# Byte truncation (here or in the MAX_DIFF_BYTES cap above) can slice a multi-byte UTF-8 sequence.
# agy is a Rust binary and std::env::args() PANICS on a non-UTF-8 argument, which would reintroduce
# an instant startup failure -- exactly the class of bug this guard exists to prevent. Drop any
# invalid/partial sequences with iconv (glibc + macOS ship it), and fall back to python3 (present on
# every CI runner) if iconv is absent, so a no-iconv host can't leave a split sequence in the prompt.
# Explicit branches, not `&& mv || rm`: that idiom masks an mv failure and would then feed agy the
# original (possibly split) bytes. A successful sanitize must replace the file; a failed mv is fatal
# (set -e); a failed/absent sanitizer leaves the original and we proceed (it may already be clean, and
# any residual invalid byte now surfaces via the captured stderr rather than a silent instant crash).
# The `[ -s ... ]` guard is deliberate: the prompt is always non-empty here (boilerplate at
# minimum), so a sanitizer that exits 0 but emits ZERO bytes is a malfunction, and mv-ing that
# empty file over $prompt_file would wipe the prompt and hand agy nothing. Replace only on a
# non-empty result; otherwise discard it and proceed with the original (already likely clean).
if command -v iconv >/dev/null 2>&1; then
  if iconv -c -f UTF-8 -t UTF-8 "$prompt_file" > "$prompt_file.utf8" 2>/dev/null && [ -s "$prompt_file.utf8" ]; then
    mv "$prompt_file.utf8" "$prompt_file"
  else
    rm -f "$prompt_file.utf8"
  fi
elif command -v python3 >/dev/null 2>&1; then
  if python3 -c 'import sys; sys.stdout.buffer.write(open(sys.argv[1],"rb").read().decode("utf-8","ignore").encode("utf-8"))' \
       "$prompt_file" > "$prompt_file.utf8" 2>/dev/null && [ -s "$prompt_file.utf8" ]; then
    mv "$prompt_file.utf8" "$prompt_file"
  else
    rm -f "$prompt_file.utf8"
  fi
fi

# Escape hatch for verifying the diff-acquisition and prompt-assembly path (including
# the large-PR API-limit fallback and the inline/file decision) without spending an agy
# run or posting to the PR. Prints the assembled prompt to stdout and stops before agy.
if [ -n "${AGY_DRY_RUN:-}" ]; then
  log "AGY_DRY_RUN set: printing the assembled prompt and exiting before agy runs"
  if [ "${use_file:-0}" = "1" ]; then
    log "prompt: $(wc -c < "$prompt_file") bytes (diff handed off on disk as ${agy_diff_file:-?}, $(wc -c < "$diff_file") bytes)"
  else
    log "prompt: $(wc -c < "$prompt_file") bytes (diff inlined, $(wc -c < "$diff_file") bytes)"
  fi
  cat "$prompt_file"
  exit 0
fi

# --- run agy headless, under a PTY (works around agy issue #76: -p drops --------
#     stdout when stdout is not a TTY, e.g. piped/redirected/subprocess) ---------
flags=( --print-timeout "$AGY_PRINT_TIMEOUT" --sandbox --dangerously-skip-permissions )
# In file-handoff mode agy must read the on-disk diff, and --sandbox otherwise confines its file
# tool to its own workspace root (NOT the shell CWD) — so add the checkout to the workspace. Only in
# file mode: an inline review reads nothing from disk, so it keeps zero filesystem access (narrower
# prompt-injection surface for the common path). Proven necessary on the live runner: without this
# (and the absolute path above) the large-diff handoff file reads back as "does not exist".
[ "${use_file:-0}" = "1" ] && flags+=( --add-dir "$PWD" )

# Strip the repo tokens from agy's own environment. agy runs under
# --dangerously-skip-permissions and ingests an untrusted diff (prompt-injection
# surface); `gh` and the large-diff fetch run in THIS script, before and after, and
# agy has no use for GH_TOKEN / GITHUB_TOKEN — so it should not inherit them.
agy_env=( env -u GH_TOKEN -u GITHUB_TOKEN )
[ -n "$AGY_MODEL" ]  && flags+=( --model "$AGY_MODEL" )
[ -n "$AGY_EFFORT" ] && flags+=( --effort "$AGY_EFFORT" )

out_file="$(mktemp)"
here="$(cd "$(dirname "$0")" && pwd)"
: > "$LOG"

# Serialize agy across concurrent review jobs on this host. agy runs a SINGLETON
# local language-server + conversation store per user, so two `--print` calls at
# once collide (one reports the backend "unavailable"). flock makes jobs queue
# instead of failing. FAIL CLOSED: if flock is missing, or the lock can't be
# taken/times out, exit rather than let two agy processes race each other --
# a fail-open here made the exact collision this lock exists to prevent still
# reachable (one run can burn the whole ${AGY_RETRIES}x${AGY_LOCK_WAIT}s wait).
command -v flock >/dev/null 2>&1 || {
  log "flock is required to serialize agy; refusing to run unserialized"
  exit 1
}
# Create the lock dir first: a failed `exec 9>` redirection is a FATAL shell error (it aborts
# before the `|| log` fallback can run), so ensure the parent exists on a fresh runner. `>>` opens
# for append rather than truncating the lockfile — flock uses the fd, not the contents.
# Validated before use: an empty `AGY_LOCK` (an env override set to "") would make `dirname`
# yield "." and the redirection below fail with an obscure shell error, at the one point where a
# clear message matters -- this is the guard that keeps two agy runs off each other.
if [ -z "$AGY_LOCK" ]; then
  log "AGY_LOCK is empty; refusing to run unserialized"
  exit 1
fi
mkdir -p "$(dirname "$AGY_LOCK")"
exec 9>>"$AGY_LOCK"
flock -w "$AGY_LOCK_WAIT" 9 || {
  log "agy lock timed out after ${AGY_LOCK_WAIT}s"
  exit 1
}

# Retry the whole agy attempt on empty/failed output: transient "agy is down"
# (backend rate-limit / local-server contention) usually clears within seconds.
# The flock (above) is held across all attempts, released after the loop.
for (( attempt=1; attempt<=AGY_RETRIES; attempt++ )); do
  : > "$out_file"   # clear any partial output from a prior attempt

  if command -v unbuffer >/dev/null 2>&1; then
    log "running agy via unbuffer (allocates a PTY) [attempt ${attempt}/${AGY_RETRIES}]"
    "${agy_env[@]}" unbuffer "$AGY_BIN" "${flags[@]}" --print "$(cat "$prompt_file")" > "$out_file" 2>>"$LOG" || true
  else
    log "unbuffer not found; falling back to script(1) [attempt ${attempt}/${AGY_RETRIES}]"
    raw="$(mktemp)"
    # `script -c` runs its command through `sh -c`. Build that command with `printf '%q '`
    # so every argument -- including the flag values, which come from env vars
    # (AGY_MODEL / AGY_EFFORT / AGY_PRINT_TIMEOUT) -- is shell-escaped for the inner shell.
    # A raw `${flags[*]}` here would let a shell metacharacter in any of those be evaluated
    # by `sh -c` (command injection); `%q` quotes each token exactly.
    cmd="$(printf '%q ' "$here/_agy_print.sh" "$prompt_file" "${flags[@]}")"
    "${agy_env[@]}" AGY_BIN="$AGY_BIN" script -qfec "$cmd" "$raw" >/dev/null 2>>"$LOG" || true
    col -b < "$raw" > "$out_file"
    rm -f "$raw"   # each retry makes a fresh $raw; the EXIT trap only holds the last one
  fi

  # normalize CRs without sed -i (avoid in-place edit footguns)
  tr -d '\r' < "$out_file" > "$out_file.clean" && mv "$out_file.clean" "$out_file"

  # If the capture carries a live OAuth authorization URL, agy's cached Google session has lapsed
  # on this runner and there is NO review — just its interactive login flow (the OAuth URL + a
  # "paste the authorization code" prompt). That must never reach a public PR comment (noise,
  # phishing-shaped, and it advertises that the runner's auth dropped). Treat it as a hard,
  # NON-retryable failure: re-auth is a human action on the runner host, so retrying only thrashes
  # the backoff for a minute. Blank the capture so no later path (have_text / body assembly) can
  # ever post it, flag it, and stop.
  if oauth_url_present "$out_file"; then
    log "agy is NOT authenticated on this runner: it printed its interactive Google OAuth login flow instead of a review. Refusing to post it (it carries an OAuth URL). Re-authenticate agy on the runner host, then re-run with '/agy-review'."
    : > "$out_file"
    auth_failed=1
    break
  fi

  # NOTE: there is deliberately no SQLite-conversation-store fallback here. agy's
  # store is keyed by mtime, not session, and on a shared/multi-user runner the
  # most-recent `.db` can belong to an UNRELATED concurrent local `agy` session --
  # reading it would post that session's output into a public PR comment (data
  # leak). The PTY path above plus the retry loop cover agy issue #76 without it.

  have_text "$out_file" && break
  if [ "$attempt" -lt "$AGY_RETRIES" ]; then
    delay=$(( AGY_RETRY_DELAY * attempt ))
    log "no usable output (attempt ${attempt}/${AGY_RETRIES}); retrying in ${delay}s"
    sleep "$delay"
  fi
done
exec 9>&- 2>/dev/null || true    # release the agy lock so the next queued job proceeds

# Lapsed-auth abort takes precedence over the generic empty-output path: it is a specific,
# actionable cause (re-auth agy on the runner), not a transient backend blip, and we already
# blanked $out_file above so nothing is postable. Exit non-zero WITHOUT posting anything.
if [ "${auth_failed:-0}" = "1" ]; then
  log "aborting without posting: agy requires re-authentication on the runner host (no review was produced)."
  exit 1
fi

if ! have_text "$out_file"; then
  log "no review output after ${AGY_RETRIES} attempt(s). Check $LOG and confirm 'agy -p \"hi\"' works for this user."
  # Surface agy's stderr into the job log. RUNNER_TEMP is wiped between jobs, so a bare
  # `exit 1` otherwise leaves the real cause invisible in CI (E2BIG, auth, backend, ...).
  if [ -s "$LOG" ]; then
    # Bound the dump to the last lines: GitHub Actions auto-masks registered secrets (incl.
    # GITHUB_TOKEN) in logs, and this is agy's own diagnostic stream, but a bounded tail avoids
    # publishing an unbounded volume of stderr (which could echo prompt/diff content) into CI.
    log "----- captured agy stderr ($LOG), last ${AGY_LOG_TAIL_LINES:-60} lines (secrets auto-masked) -----"
    tail -n "${AGY_LOG_TAIL_LINES:-60}" "$LOG" | sed 's/^/[agy] /' >&2 || true
    log "----- end agy stderr -----"
  else
    log "(agy stderr log is empty -- agy likely failed before writing, e.g. execve E2BIG on an oversized prompt)"
  fi
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

# Final hard guard — the last line of defense, and UNCONDITIONAL, run BEFORE anything is
# deleted or posted. Layer 1 (the retry loop) already rejects a lapsed-session capture, but
# a public PR comment must NEVER carry a live OAuth authorization URL, whatever any upstream
# change does to the body — and with no "looks like a review" exemption that a header
# alongside a URL could disarm. A genuine review that merely discusses auth or quotes this
# script's bare regex has no live URL and posts normally; only an actual authorization URL
# blocks the post.
if oauth_url_present "$body_file"; then
  log "refusing to post: the assembled comment body contains a live OAuth authorization URL. Re-authenticate agy on the runner host."
  exit 1
fi

# --- post fresh, THEN replace any prior review comment --------------------------
# Publish-before-delete, deliberately: if this ordering were reversed and posting failed
# afterward (a transient gh/API error), the PR would be left with NO review comment at all
# instead of the still-valid prior one. Posting first means a failure here can only ever
# leave a harmless duplicate, never a silent loss of the last review.
# The posted comment's id comes from the POST itself, not from a read-back. `gh pr comment`
# prints the new comment's URL, whose trailing `#issuecomment-<id>` is authoritative the instant
# it returns. Re-querying the comment list to find "the newest one with our marker" raced with
# GitHub's own read replication: right after posting, the list can still omit it, and then the
# exclusion below matched nothing and the script deleted the comment it had just published --
# turning publish-before-delete into publish-then-destroy, the exact failure the ordering exists
# to prevent.
if ! post_output="$(gh pr comment "$PR" --repo "$REPO" --body-file "$body_file" 2>&1)"; then
  # Nothing is deleted when the post fails: the prior review comment is still the best
  # information the PR has, and removing it would leave no review at all.
  log "failed to post review to ${REPO}#${PR}: ${post_output}"
  exit 1
fi
log "posted review to ${REPO}#${PR}"
new_comment_id="$(printf '%s\n' "$post_output" | sed -n 's/.*#issuecomment-\([0-9][0-9]*\).*/\1/p' | tail -n 1)"

# A failed delete is logged, not swallowed: silently ignoring it would let a transient API/perms
# error leave the old comment in place alongside the new one, so runs accumulate duplicates.
# The author filter is load-bearing, not cosmetic: without it, ANY user could put the
# marker (an HTML comment) in a PR comment and have this bot delete arbitrary comments on
# the next run. Only ever delete OUR OWN bot's prior review comments -- and only ones from
# BEFORE this run (the just-posted comment's own id is excluded so it can never delete itself).
if [ -z "$new_comment_id" ]; then
  # FAIL CLOSED. Without a known id there is no way to tell the new comment from the old ones,
  # and the safe direction is unambiguous: a leftover duplicate is noise, deleting the review
  # that was just posted is data loss.
  log "warning: could not determine the posted comment id; leaving prior review comments in place"
else
  # `--arg`/`--argjson` rather than shell interpolation into the filter: the marker is an HTML
  # comment today, but a quote or a backslash in it would otherwise break the jq program itself
  # rather than simply not matching.
  #
  # Those are JQ flags, so the JSON is fetched raw and piped into a real `jq` — `gh api` has no
  # `--arg`/`--argjson` of its own and rejects them. Handing them to `gh api --jq` made it exit
  # non-zero on every run; with the old `2>/dev/null` swallowing the message and `set -o pipefail`
  # in force, the script then died *after* posting, so the stale comments were never deleted and
  # the job went red for a reason nothing printed. stderr is kept this time for exactly that
  # reason. (`--paginate` without `--jq` emits one JSON array per page; `jq` reads that stream
  # fine, applying `.[]` to each.)
  stale_ids="$(
    gh api "repos/${REPO}/issues/${PR}/comments" --paginate \
      | jq -r --arg marker "$MARKER" --argjson new_id "$new_comment_id" "$SELECT_STALE_JQ"
  )" || {
    log "warning: could not list prior review comments; leaving them in place"
    stale_ids=""
  }
  while read -r cid; do
    [ -n "$cid" ] || continue
    if ! gh api -X DELETE "repos/${REPO}/issues/comments/${cid}" >/dev/null 2>&1; then
      log "warning: could not delete prior review comment ${cid}; a duplicate may result"
    fi
  done <<< "$stale_ids"
fi

# The delete loop above is the last real work; end on a defined status so a stray non-zero from
# it can never be mistaken for "the review failed" once the comment is already published.
exit 0
