#!/usr/bin/env bash
# Install one apt package, bounded and retried.
#
# v2.3.9 A5b. The cross-compile gate provisions glibc headers for bindgen with a
# bare `apt-get update && apt-get install`. Both are network fetches with no
# timeout of their own, so when a mirror stalls the step hangs until the JOB
# timeout fires — 25 minutes for `libretro-cross` — and the run is reported as
# cancelled rather than as what it was.
#
# That is not hypothetical. During the v2.3.7 cut this hung four separate times
# across two PRs, always in a setup or provisioning step and never in a compile
# or test step: twice in `rust-setup`, once in the armhf provision, once in the
# aarch64 provision. Each cost 25-45 minutes and needed a manual re-run. The
# per-job `timeout-minutes` added in #400 bounded the damage correctly; nothing
# addressed the fragility underneath it.
#
# Two bounds, doing different jobs:
#
#   * `timeout` per command, so a stalled fetch fails in minutes rather than
#     consuming the job's entire budget. The job timeout is a backstop against a
#     hang; this is the thing that actually notices one.
#   * Three attempts with linear backoff, because the observed failure is
#     transient — a re-run has cleared it every time.
#
# Deliberately NOT a general-purpose apt wrapper: one package, from a workflow
# `env:` (never from event data, which is the injection vector the Actions
# security guidance warns about), and a hard failure if it is unset.
set -euo pipefail

if [ -z "${APT_PACKAGE:-}" ]; then
    echo "::error::APT_PACKAGE is unset; refusing to guess what to install" >&2
    exit 1
fi

# Bounds chosen from observed behaviour, not from taste: a healthy `update` on
# these runners is a few seconds and a healthy `install` well under a minute, so
# these are roughly an order of magnitude of headroom. Long enough that a merely
# slow mirror still succeeds; short enough that three full attempts fit inside
# the 25-minute job budget with room for the build that follows.
readonly UPDATE_TIMEOUT=180
readonly INSTALL_TIMEOUT=300
readonly ATTEMPTS=3

for attempt in $(seq 1 "$ATTEMPTS"); do
    if timeout "$UPDATE_TIMEOUT" sudo apt-get update -qq &&
        timeout "$INSTALL_TIMEOUT" sudo apt-get install -yq "$APT_PACKAGE"; then
        echo "Installed ${APT_PACKAGE} on attempt ${attempt}."
        exit 0
    fi
    # Reported per attempt rather than only on final failure: a run that
    # succeeded on attempt 3 looks identical to one that succeeded on attempt 1
    # in the job's conclusion, and the difference is the early warning that the
    # mirrors are degrading.
    echo "::warning::apt attempt ${attempt}/${ATTEMPTS} for ${APT_PACKAGE} failed or timed out"
    if [ "$attempt" -lt "$ATTEMPTS" ]; then
        sleep $((attempt * 15))
    fi
done

echo "::error::Could not install ${APT_PACKAGE} after ${ATTEMPTS} attempts" >&2
exit 1
