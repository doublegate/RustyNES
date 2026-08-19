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

# Bounds RE-CALIBRATED against a real failure, because the first set was
# calibrated against a claim.
#
# The comment that stood here asserted that "a healthy `update` on these runners
# is a few seconds", which made 180s look like an order of magnitude of headroom.
# It was never measured. On 2026-08-19 this step failed on three consecutive PRs
# (#412, #415, #416) and the log says exactly where:
#
#   attempt 1  update killed at 180s, precisely the timeout
#   attempt 2  update killed at 180s again
#   attempt 3  update succeeded; install killed at 300s MID-DOWNLOAD, the log
#              ending inside `Get:20 gcc-13-aarch64-linux-gnu [21.1 MB]`
#
# No apt error anywhere in it. The provision was succeeding slowly and the
# wrapper converted that into a hard failure, three times over. A bound derived
# from an unmeasured premise is a gate that fires on healthy runs, which is worse
# than a loose one: it blocked three PRs while looking like infrastructure decay.
#
# `apt-get update` now runs ONLY after a direct install has failed. The runner
# image ships a populated package index, so an index refresh does not belong on
# the happy path -- it is the recovery step for the one case that needs it, an
# index stale enough that the requested version has moved and install 404s.
#
# THE SECOND CALIBRATION, from this change's own first CI run. Do not tune these
# numbers again without a `Fetched` line to point at.
#
# The narrowed package (see the workflow step) made the provision succeed, and
# the log gives the reason the old one could not:
#
#   Fetched 4201 kB in 4min 45s (14.7 kB/s)
#
# Fourteen point seven kilobytes per second. The mirror is degraded by roughly
# three orders of magnitude, which is why every bound derived from "healthy"
# behaviour was wrong -- and why the previous 40 MB package set was hopeless:
# at that rate it needed about 45 minutes, past the job budget entirely.
#
# Even at 4.2 MB it only just passed, and NOT the way the first draft of this
# comment assumed. The real sequence was:
#
#   attempt 1  install downloaded all 4201 kB (285s), then was killed at 300s
#              during dpkg unpack -- the download finished, the install did not
#   attempt 2  update, then install: NO re-download, because the archives were
#              already in /var/cache/apt/archives. Succeeded.
#
# So the run was rescued by apt's archive cache persisting across attempts. That
# is a real and useful property -- each attempt makes progress rather than
# starting over -- but it was undesigned and undocumented, which makes it the
# same defect class as the bound it rescued: behaviour nobody wrote down.
# Written down now, and no longer relied upon: INSTALL_TIMEOUT is sized so ONE
# attempt completes at the worst speed actually observed.
#
#   285s download at 14.7 kB/s + dpkg unpack, so 600s is ~2x the worst observed.
#
# ATTEMPTS drops to 2 to keep the worst case inside the job's 25-minute budget,
# stated as arithmetic rather than as "roughly an order of magnitude" -- that
# phrasing is what went unchecked the first time:
#
#   600 + 15 + (180 + 600) = 1395s, plus ~45s of surrounding steps
#
# The third attempt is not a loss worth arguing for: attempt 2 already retries
# with a refreshed index AND a warm download cache, which covers both the stale
# index and the slow mirror. A third would only repeat attempt 2.
readonly UPDATE_TIMEOUT=180
readonly INSTALL_TIMEOUT=600
readonly ATTEMPTS=2

# Elevation on the OUTSIDE, `timeout` on the inside. Review on #408 caught the
# ordering and it is not cosmetic: with `timeout` outermost the SIGTERM goes to
# the elevation helper, which may not forward it — leaving `apt-get` orphaned
# while still holding the dpkg lock, so every subsequent retry fails on the lock
# rather than on the original problem. A retry loop that guarantees its own
# retries fail is worse than no retry loop at all.
#
# `DEBIAN_FRONTEND=noninteractive` for the same class of reason: a package that
# prompts for configuration blocks on stdin that will never arrive in CI, burning
# the whole timeout budget waiting for a human who is not there. Passed through
# explicitly because the environment is scrubbed on elevation.
#
# Set via `env` rather than as a bare `VAR=value` argument to the elevation
# helper. Both work on a standard GitHub runner (`ALL=(ALL) NOPASSWD:ALL`
# implies the privilege), but the bare form additionally requires SETENV in
# sudoers, so on a stricter host it fails outright — and it fails by refusing to
# run at all, which would break the wrapper rather than degrade it. `env` is a
# plain command and needs no such privilege. (Review on #409; both reviewers
# raised it independently.)
apt_install() {
    sudo env DEBIAN_FRONTEND=noninteractive timeout "$INSTALL_TIMEOUT" \
        apt-get install -yq --no-install-recommends "$APT_PACKAGE"
}

apt_update() {
    sudo env DEBIAN_FRONTEND=noninteractive timeout "$UPDATE_TIMEOUT" apt-get update -qq
}

# `--no-install-recommends` is not a size micro-optimisation, it is the same
# argument as the bounds above: every byte downloaded is time spent inside a
# timeout. The recommends pulled here are packages the caller has already written
# down as unused -- the workflow step's own comment says the cross linker is
# unused because this gate is `cargo check` only.
for attempt in $(seq 1 "$ATTEMPTS"); do
    # Attempt 1 goes straight at the package. Later attempts refresh the index
    # first, since a stale index is the one failure a refresh actually fixes.
    if [ "$attempt" -eq 1 ]; then
        if apt_install; then
            echo "Installed ${APT_PACKAGE} on attempt ${attempt} (no index refresh needed)."
            exit 0
        fi
    elif apt_update && apt_install; then
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
