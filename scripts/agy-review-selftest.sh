#!/usr/bin/env bash
#
# agy-review-selftest.sh -- guards the comment-selection logic in `agy-review.sh`.
#
# Why this exists: that filter decides which PR comments the bot DELETES, and it has been wrong
# twice, both times invisibly.
#
#   1. The just-posted comment was not reliably excluded. `new_comment_id` came from re-querying
#      the comment list, which races GitHub's read replication; on a miss the exclusion became
#      `select(.id != null)`, true for every id, and the run deleted the review it had just
#      published.
#   2. jq's `--arg`/`--argjson` were handed to `gh api`, which has no such flags. It exited
#      non-zero, `2>/dev/null` hid the message, and `set -o pipefail` + `set -e` killed the script
#      AFTER posting — so stale comments silently accumulated and the job went red with nothing in
#      the log explaining why.
#
# Neither was catchable by looking at the review the bot posted: both times it posted fine. So the
# filter is tested here directly, offline, against fixtures — no network, no `gh`, no runner.
#
# Run: bash scripts/agy-review-selftest.sh

set -euo pipefail

SCRIPT_DIR="$(cd -- "$(dirname -- "${BASH_SOURCE[0]}")" && pwd)"

# Source the constants out of the reviewer without running it. `agy-review.sh` does its work at
# top level, so it cannot simply be sourced; the two values under test are lifted by pattern
# instead. That coupling is deliberate: if either declaration is renamed or reshaped, this test
# fails loudly rather than silently checking a stale copy of the filter.
extract_marker() {
  sed -n 's/^MARKER="\(.*\)"$/\1/p' "$SCRIPT_DIR/agy-review.sh" | head -n 1
}
extract_filter() {
  sed -n "/^SELECT_STALE_JQ='/,/'\$/p" "$SCRIPT_DIR/agy-review.sh" \
    | sed "1s/^SELECT_STALE_JQ='//; \$s/'\$//"
}

MARKER="$(extract_marker)"
FILTER="$(extract_filter)"

[ -n "$MARKER" ] || { echo "FAIL: could not extract MARKER from agy-review.sh" >&2; exit 1; }
[ -n "$FILTER" ] || { echo "FAIL: could not extract SELECT_STALE_JQ from agy-review.sh" >&2; exit 1; }

# A non-empty extraction is not the same as a COMPLETE one. The `sed` range above ends at the
# first line closing with a quote, so a filter whose body ever ends a line that way would be
# truncated — and a truncated jq program can still be valid and still return ids, which is the
# silent-wrong-answer this whole file exists to prevent. Two independent guards:
#
#   1. it must compile (a truncated program is usually, though not always, a syntax error);
#   2. it must END with the projection, which is what makes it a complete pipeline rather than a
#      prefix of one.
# The named args must be supplied here too: the filter references `$marker`/`$new_id`, and jq
# rejects an undefined variable at COMPILE time — so omitting them fails a perfectly good program.
if ! printf '[]' | jq --arg marker x --argjson new_id 0 "$FILTER" >/dev/null 2>&1; then
  echo "FAIL: extracted SELECT_STALE_JQ is not a valid jq program (truncated?):" >&2
  printf '%s\n' "$FILTER" >&2
  exit 1
fi
case "$(printf '%s' "$FILTER" | tr -d '[:space:]')" in
  *'|.id') : ;;
  *) echo "FAIL: extracted SELECT_STALE_JQ does not end in '| .id'; extraction truncated" >&2
     printf '%s\n' "$FILTER" >&2
     exit 1 ;;
esac

fixture() {
  cat <<JSON
[
  {"id": 111, "user": {"type": "Bot",  "login": "github-actions[bot]"}, "body": "$MARKER\nold review"},
  {"id": 222, "user": {"type": "Bot",  "login": "github-actions[bot]"}, "body": "$MARKER\nolder still"},
  {"id": 333, "user": {"type": "User", "login": "someone"},             "body": "$MARKER\nnot ours"},
  {"id": 444, "user": {"type": "Bot",  "login": "other-bot"},           "body": "$MARKER\nwrong bot"},
  {"id": 555, "user": {"type": "Bot",  "login": "github-actions[bot]"}, "body": "an ordinary bot comment"},
  {"id": 999, "user": {"type": "Bot",  "login": "github-actions[bot]"}, "body": "$MARKER\nJUST POSTED"}
]
JSON
}

# Ids as a single space-separated line, with no trailing space — so the expected values below read
# as what they are rather than carrying padding an assertion would have to mirror.
select_ids() {
  fixture | jq -r --arg marker "$MARKER" --argjson new_id "$1" "$FILTER" | sort -n | paste -sd' ' -
}

fails=0
check() {
  local name="$1" want="$2" got="$3"
  if [ "$got" = "$want" ]; then
    echo "  ok    $name"
  else
    echo "  FAIL  $name"
    echo "          want: [$want]"
    echo "          got:  [$got]"
    fails=$((fails + 1))
  fi
}

echo "agy-review comment-selection self-test"

# The whole point: the comment just published is never selected for deletion.
check "excludes the just-posted comment" "111 222" "$(select_ids 999)"

# The author filter is a security control, not tidiness: without it any user could paste the
# marker into a comment and have the bot delete comments on the next run.
check "ignores other users and other bots" "111 222" "$(select_ids 999)"

# A bot comment without the marker is somebody else's feature (a CI summary, a deploy note).
check "ignores bot comments without the marker" "111 222" "$(select_ids 999)"

# Regression #1, pinned: an unknown id must not select everything. The caller now refuses to run
# the delete at all in this case, but the filter itself is checked so the two guards are
# independent rather than one relying on the other.
check "an id of 0 still excludes nothing real" "111 222 999" "$(select_ids 0)"

# A different id in the set behaves the same way, so the exclusion is genuinely by value.
check "excludes whichever id it is given" "222 999" "$(select_ids 111)"

# Regression #2, pinned: `--arg`/`--argjson` belong to jq. If they are ever moved onto `gh api`
# again, that command exits non-zero — assert the flags are not passed to `gh api` in the script.
# Line continuations are folded first: `--arg` moved onto a continuation line would otherwise sit
# on a different physical line from `gh api`, and a line-by-line grep would report a false pass on
# exactly the mistake this check exists to catch.
if sed -e ':a' -e '/\\$/{N;s/\\\n//;ba' -e '}' "$SCRIPT_DIR/agy-review.sh" \
     | grep -qE 'gh api[^|]*--(arg|argjson)'; then
  echo "  FAIL  --arg/--argjson passed to \`gh api\` (jq flags; gh api rejects them)"
  fails=$((fails + 1))
else
  echo "  ok    --arg/--argjson are not passed to \`gh api\`"
fi

if [ "$fails" -ne 0 ]; then
  echo "$fails check(s) failed" >&2
  exit 1
fi
echo "all checks passed"
