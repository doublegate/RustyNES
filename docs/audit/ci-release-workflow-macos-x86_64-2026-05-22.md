# CI Release Workflow — macOS x86_64 Job Hang Investigation & Fix

**Date:** 2026-05-22
**Workflow:** `.github/workflows/release.yml`
**Affected matrix entry:** `build (x86_64-apple-darwin)`
**Outcome:** Fix shipped; pending re-validation via `workflow_dispatch` or next release tag.

---

## Summary

The `build (x86_64-apple-darwin)` matrix entry of the Release workflow has been
hanging indefinitely in the GitHub Actions runner queue for both of the last two
release-tag invocations (`v1.0.0-rc1`, `v1.0.0-rc2`). Root cause: the workflow
pins `runs-on: macos-13`, and the `macos-13` runner image was fully deprecated
on **2025-12-04** by GitHub. The job never allocates a runner, never enters the
"in progress" state, and never executes any steps — it just sits in the queue.
The prior `v1.0.0-rc1` invocation sat queued for ~24 hours before manual
cancellation; the `v1.0.0-rc2` invocation had been queued since
`2026-05-22T22:09:51Z` and was cancelled as part of this investigation.

The fix is a single matrix-OS swap from `macos-13` to `macos-15-intel`, the
official replacement label GitHub announced in the same deprecation issue, plus
a `timeout-minutes: 30` guardrail on the entire build job so any future
runner-allocation regression fails fast instead of squatting the release queue.

---

## Evidence

### Hung run inventory

| Run ID        | Tag           | Trigger | Queued at             | Cancelled at          | Wall-clock queued |
|---------------|---------------|---------|-----------------------|-----------------------|-------------------|
| `26012710060` | `v1.0.0-rc1`  | push    | `2026-05-18T03:58:27Z`| `2026-05-19T03:58:27Z`| ~24h              |
| `26314402806` | `v1.0.0-rc2`  | push    | `2026-05-22T22:09:51Z`| `2026-05-22` (this fix)| ~variable (cancelled mid-investigation) |

Both runs show the SAME signature on the failing job:

```
status: queued
steps: []            # the runner was never allocated, no step ever started
runnerName: null
completedAt: 0001-01-01T00:00:00Z  # sentinel "never completed"
```

while the other three matrix entries on the SAME runs (Linux x86_64, macOS
aarch64, Windows x86_64) all completed successfully in 2-6 minutes:

```
build (aarch64-apple-darwin)     — macos-14         — 2m 37s — success
build (x86_64-unknown-linux-gnu) — ubuntu-latest    — 5m 24s — success
build (x86_64-pc-windows-msvc)   — windows-latest   — 9m 23s — success
build (x86_64-apple-darwin)      — macos-13         — QUEUED — never executed
```

### What changed between the last green run and the hang

`git log -- .github/workflows/release.yml` returns exactly one commit
(`f9651a1 build(ci): release workflow + README badges`). The workflow YAML has
been UNCHANGED since the file was introduced. The change between green and
hang is therefore entirely external to this repository — namely, GitHub's
infrastructure-side decommissioning of the `macos-13` runner image.

### Comparison to the CI workflow

`.github/workflows/ci.yml` uses `macos-latest` for its macOS test job (which
GitHub has migrated to Apple Silicon). The most recent CI run on `main`
(`26314902113`, `2026-05-22T22:24:48Z`) completed the `test (macos-latest / stable)`
job successfully in ~1 minute. This rules out a project-side macOS build issue:
the CI macOS job (different OS label, different host arch) works fine. The
specific failure surface is exclusively the deprecated `macos-13` label used by
the Release workflow's x86_64 matrix entry.

### CHANGELOG corroboration

`CHANGELOG.md` line 1782 already documents the user's prior awareness of this
exact pattern from the `v1.0.0-rc1` run:

> "(`x86_64-apple-darwin`) was still queued for macOS runner capacity at
> [the time of the rc1 cut]"

That earlier observation was treated as a transient capacity blip; the present
investigation establishes that it is the permanent steady-state behavior for
`macos-13` post-deprecation.

---

## Root cause

GitHub officially deprecated `macos-13` and `macos-13-arm64` runner images
between **2025-09-22** (deprecation start) and **2025-12-04** (full removal).
The current date is **2026-05-22** — over five months past full removal.
Workflows using the `macos-13` label are no longer scheduled onto runners and
sit in the queue until cancelled or until the run-level 6-hour ceiling expires
(GitHub's per-run default; jobs without `timeout-minutes` set inherit this).

Authoritative sources:

1. **actions/runner-images#13046** — "[macOS] The macOS 13 Ventura based runner
   images will begin deprecation on September 22nd and will be fully unsupported
   by December 4th for GitHub and ADO". Key quote:
   > "Workflows using the `macos-13` image label will be terminated with an error."

   URL: <https://github.com/actions/runner-images/issues/13046>

2. **actions/runner-images#13045** — "[macOS] The additional macOS 15 Sonoma
   Intel-based image will be available in GitHub Actions". Announces the
   replacement label:
   > "For users that require the x86_64 (Intel) environment, we are
   > introducing a new label to migrate to: `macos-15-intel`. The new label
   > will run on macOS 15 and will be available from now until August 2027.
   > This will be the last available x86_64 image from Actions, and after that
   > date the x86_64 architecture will not be supported on GitHub Actions."

   URL: <https://github.com/actions/runner-images/issues/13045>

3. **PR actions/runner-images#13413** (merged 2025-12-15) — "Post macos 13
   deprecation to readmes" — codifies removal in the upstream runner-image
   READMEs after the December 4th cutoff.

---

## Fix applied

File: `.github/workflows/release.yml`. Two changes in the same `jobs.build`
block.

### Change 1: matrix OS swap for the Intel target

```diff
-          - os: macos-13
+          - os: macos-15-intel
             target: x86_64-apple-darwin
```

`macos-15-intel` runs macOS 15 Sonoma on Intel hardware. Per
`actions/runner-images#13045`, this is the official, intended, and ONLY
remaining x86_64 macOS image. It is supported through August 2027, at which
point the project will need to either (a) drop the Intel macOS binary target,
(b) adopt a cross-compile strategy via `cargo-zigbuild` from a Linux host, or
(c) cross-compile from `macos-14` arm64 with `rustup target add
x86_64-apple-darwin` and accept that no `cargo test --target
x86_64-apple-darwin` is possible without Rosetta-2 emulation.

The Rust toolchain target `x86_64-apple-darwin` remains
**Tier 1 with host tools** per <https://doc.rust-lang.org/rustc/platform-support.html>,
so the source-of-truth compile path is unaffected.

### Change 2: `timeout-minutes: 30` on the build job

```diff
   build:
     name: build (${{ matrix.target }})
     runs-on: ${{ matrix.os }}
+    timeout-minutes: 30
```

Historical wall-clock for the three working matrix entries on `v1.0.0-rc1`:

| Job                              | Duration |
|----------------------------------|----------|
| `build (aarch64-apple-darwin)`   | ~2m 37s  |
| `build (x86_64-unknown-linux-gnu)`| ~5m 24s |
| `build (x86_64-pc-windows-msvc)` | ~9m 23s  |

30 minutes is a 3-12x safety margin over observed durations and ~3-6x what a
cold-cache `cargo build --release -p nes-frontend` typically needs on a busy
Intel macOS runner. It is tight enough to fail fast on a future runner-queue
regression (the failure mode that prompted this investigation) but loose enough
to not regress on legitimate cold-cache compilation. Without this guard, a
queued-but-never-scheduled job inherits the workflow-level 6-hour ceiling and
blocks the release queue for users polling on the GitHub Release publication.

---

## Runs cancelled during this investigation

| Run ID        | Tag           | Reason                                                          |
|---------------|---------------|------------------------------------------------------------------|
| `26314402806` | `v1.0.0-rc2`  | Stuck in queue with `macos-13` runner unavailable; freeing slot. |

The prior `26012710060` (`v1.0.0-rc1`) was already cancelled by the user
~24 hours into the queue wait, before this investigation began.

---

## Re-validation plan

Two options for verifying the fix end-to-end:

1. **`workflow_dispatch`** — the workflow already accepts a manual trigger with
   a `tag` input parameter:
   ```bash
   gh workflow run Release -f tag=v1.0.0-rc2
   ```
   This will execute the full build matrix without publishing a new tag. Note
   that the `softprops/action-gh-release@v2` step will attempt to upload assets
   to the existing `v1.0.0-rc2` Release; if the artifact name collides with the
   already-uploaded asset from the now-cancelled `v1.0.0-rc2` run, the upload
   will fail unless `fail_on_unmatched_files: true` is paired with a clean
   slate. The aarch64-apple-darwin, Linux, and Windows artifacts ARE already
   uploaded from the cancelled run; only the x86_64-apple-darwin asset is
   missing. The retry is therefore additive for the missing target.

2. **Next release tag** — when the project moves to `v1.0.0` final, the
   tag-push trigger will exercise this matrix on the new commit. Lower
   confidence (longer feedback loop) but no risk of artifact collision.

This audit recommends option 1 once the user confirms they want to spend the
Actions minutes; option 2 is the no-cost fallback.

---

## Follow-up items (not in scope of this fix)

1. **August 2027 deadline.** When `macos-15-intel` is removed, the project must
   either drop the `x86_64-apple-darwin` matrix entry entirely or move to
   `cargo-zigbuild` cross-compilation from `ubuntu-latest`. Track as a v1.x
   release-engineering item.

2. **`continue-on-error: true` consideration.** Not adopted in this fix. Adding
   it to the macOS matrix entries would mean a hung or broken Intel macOS build
   doesn't block the release of the other three platforms. Pro: faster
   release-cut. Con: silently shipping a release with missing platform binaries
   is a UX regression and contradicts the release notes' "Targets: ..." line.
   Defer to user product decision.

3. **`fail_on_unmatched_files: true` interplay with partial re-runs.** If a
   workflow_dispatch re-run is used to fill in the missing macOS-Intel asset
   for `v1.0.0-rc2`, the OTHER three jobs in the matrix will all attempt to
   re-upload assets that already exist on the Release. `softprops/action-gh-release@v2`
   handles this by default (existing assets are overwritten), but it is worth
   verifying once. Not blocking this fix.

---

## References

- GitHub issue announcing `macos-13` deprecation:
  <https://github.com/actions/runner-images/issues/13046>
- GitHub issue announcing `macos-15-intel` replacement:
  <https://github.com/actions/runner-images/issues/13045>
- GitHub PR removing `macos-13` from runner-image READMEs:
  <https://github.com/actions/runner-images/pull/13413>
- Rust platform-support for `x86_64-apple-darwin` (Tier 1 with host tools):
  <https://doc.rust-lang.org/rustc/platform-support.html>
- This project's CHANGELOG entry that anticipated the issue but didn't
  diagnose the root cause: `CHANGELOG.md` line 1782 ("`x86_64-apple-darwin`
  was still queued for macOS runner capacity").
