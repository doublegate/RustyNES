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
  sed -n "/^SELECT_OURS_JQ='/,/'\$/p" "$SCRIPT_DIR/agy-review.sh" \
    | sed "1s/^SELECT_OURS_JQ='//; \$s/'\$//"
}

MARKER="$(extract_marker)"
FILTER="$(extract_filter)"

[ -n "$MARKER" ] || { echo "FAIL: could not extract MARKER from agy-review.sh" >&2; exit 1; }
[ -n "$FILTER" ] || { echo "FAIL: could not extract SELECT_OURS_JQ from agy-review.sh" >&2; exit 1; }

# The REAL body-format implementation, sourced rather than reimplemented. The first version of
# these checks inlined its own copy of the `awk` pipeline, and a mutation deleting the marker
# strip from the script came back NOT CAUGHT -- a test that reimplements its subject agrees with
# itself forever. `agy-review.sh` cannot be sourced (it works at top level), which is exactly
# why the format lives in its own file.
# shellcheck source=scripts/_agy_comment_body.sh
. "$SCRIPT_DIR/_agy_comment_body.sh"
[ -n "${AGY_ARCHIVE_START:-}" ] || { echo "FAIL: _agy_comment_body.sh defined no AGY_ARCHIVE_START" >&2; exit 1; }
[ -n "${AGY_ARCHIVE_END:-}" ]   || { echo "FAIL: _agy_comment_body.sh defined no AGY_ARCHIVE_END" >&2; exit 1; }

# The script must actually USE the shared helper, or these checks test a file nothing runs.
grep -q '_agy_comment_body.sh' "$SCRIPT_DIR/agy-review.sh" \
  || { echo "FAIL: agy-review.sh does not source _agy_comment_body.sh" >&2; exit 1; }
for fn in agy_body_head agy_body_archive agy_drop_oldest_round; do
  grep -q "$fn" "$SCRIPT_DIR/agy-review.sh" \
    || { echo "FAIL: agy-review.sh does not call $fn" >&2; exit 1; }
done

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
if ! printf '[]' | jq --arg marker x "$FILTER" >/dev/null 2>&1; then
  echo "FAIL: extracted SELECT_OURS_JQ is not a valid jq program (truncated?):" >&2
  printf '%s\n' "$FILTER" >&2
  exit 1
fi
case "$(printf '%s' "$FILTER" | tr -d '[:space:]')" in
  *'|.id//empty') : ;;
  *) echo "FAIL: extracted SELECT_OURS_JQ does not end in '| .id // empty'; extraction truncated" >&2
     printf '%s\n' "$FILTER" >&2
     exit 1 ;;
esac

fixture() {
  cat <<JSON
[
  {"id":  50, "user": {"type": "User", "login": "someone"},             "body": "$MARKER\nimpostor, posted FIRST"},
  {"id":  60, "user": {"type": "Bot",  "login": "other-bot"},           "body": "$MARKER\nwrong bot, also early"},
  {"id":  70, "user": {"type": "Bot",  "login": "github-actions[bot]"}, "body": "an ordinary bot comment"},
  {"id": 111, "user": {"type": "Bot",  "login": "github-actions[bot]"}, "body": "$MARKER\nour canonical thread"},
  {"id": 222, "user": {"type": "Bot",  "login": "github-actions[bot]"}, "body": "$MARKER\na later duplicate"}
]
JSON
}

# Ids as a single space-separated line, with no trailing space — so the expected values below read
# as what they are rather than carrying padding an assertion would have to mirror.
select_id() {
  fixture | jq -r --arg marker "$MARKER" "$FILTER"
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

# The whole point: exactly ONE comment is chosen, and it is the OLDEST of ours -- so the canonical
# thread keeps growing rather than a new one starting whenever a duplicate exists.
check "selects the oldest of OUR OWN marked comments" "111" "$(select_id)"

# The author filter is a security control, not tidiness: without it any user could paste the
# marker (invisible when rendered) into a comment and have the bot EDIT it -- previously, delete it.
check "never selects another user or another bot" "111" "$(select_id)"

# A bot comment without the marker is somebody else's feature (a CI summary, a deploy note).
check "ignores bot comments without the marker" "111" "$(select_id)"

# No comment of ours yet: the filter must yield EMPTY, not `null`. The caller tests the result with
# `[ -n ... ]`, and the string "null" is non-empty -- it would be used as a comment id and every
# edit would 404 into the fallback, silently reverting to the post-fresh path this replaced.
check "yields empty when we have no comment yet" "" \
  "$(printf '[{"id":1,"user":{"type":"User","login":"a"},"body":"hi"}]' \
     | jq -r --arg marker "$MARKER" "$FILTER")"

check "yields empty for an empty comment list" "" \
  "$(printf '[]' | jq -r --arg marker "$MARKER" "$FILTER")"

# --- the archive round-trip ----------------------------------------------------
# A body split into head + archive and reassembled must lose neither. The failure this guards is
# not a crash: an `awk` that mismatched the sentinels would produce a body that renders fine and
# quietly contains only the newest round, which is indistinguishable from a first review.
round_trip_body="$(printf '%s\nNEWEST ROUND\n%s\n<details>\nOLDER ROUND\n</details>\n%s\n' \
  "$MARKER" "$AGY_ARCHIVE_START" "$AGY_ARCHIVE_END")"

head_part="$(printf '%s\n' "$round_trip_body" | agy_body_head "$MARKER")"
archive_part="$(printf '%s\n' "$round_trip_body" | agy_body_archive)"

check "the head split drops the marker and stops at the archive" "NEWEST ROUND" "$head_part"
check "the archive split returns only the archived rounds" \
  "$(printf '<details>\nOLDER ROUND\n</details>')" "$archive_part"

# A body with NO archive yet (the first round) must split to a head and an empty archive, not to
# an empty head -- the first re-review is exactly when this path runs for the first time.
first_body="$(printf '%s\nFIRST ROUND\n' "$MARKER")"
check "a body with no archive still yields its head" "FIRST ROUND" \
  "$(printf '%s\n' "$first_body" | agy_body_head "$MARKER")"
check "a body with no archive yields an empty archive" "" \
  "$(printf '%s\n' "$first_body" | agy_body_archive)"

# The head must NOT carry the marker forward: an archived round that still contains it would
# make every future run's `contains($marker)` match inside the archive, and the split would
# then cut at the wrong place. A mutation removing the strip must fail here.
check "the head never carries the marker into the archive" "" \
  "$(printf '%s\nX\n' "$MARKER" | agy_body_head "$MARKER" | grep -F -x "$MARKER" || true)"

# Trimming must terminate: with no `<details>` left, dropping fails rather than looping.
check "dropping from an empty archive fails rather than spinning" "1" \
  "$(printf 'no rounds here\n' | agy_drop_oldest_round >/dev/null 2>&1; echo $?)"
check "dropping removes the OLDEST round, keeping the newest" \
  "$(printf '<details>\nNEW\n</details>')" \
  "$(printf '<details>\nNEW\n</details>\n<details>\nOLD\n</details>\n' | agy_drop_oldest_round)"

# The script must not delete comments any more. A reintroduced DELETE is the regression that
# would silently restore the destructive behaviour this design replaced.
if grep -qE 'gh api +-X +DELETE' "$SCRIPT_DIR/agy-review.sh"; then
  echo "  FAIL  agy-review.sh deletes comments again; the archive design forbids it"
  fails=$((fails + 1))
else
  echo "  ok    agy-review.sh never deletes a comment"
fi

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
