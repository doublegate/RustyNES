# ADR 0009 — Drop the x86_64-apple-darwin release-binary target

**Status:** Accepted.
**Date:** 2026-05-25
**Author:** RustyNES v2 maintainers
**Supersedes:** None. Resolves the "macOS x86_64 runner sunset" forward
reminder tracked in `to-dos/ROADMAP.md` → "Release engineering (v1.x)".

## Context

The release workflow (`.github/workflows/release.yml`) builds the
`rustynes-v2` binary for four targets: `x86_64-unknown-linux-gnu`,
`x86_64-apple-darwin`, `aarch64-apple-darwin`, and
`x86_64-pc-windows-msvc`. The Intel macOS binary is built **natively** on a
GitHub-hosted Intel macOS runner.

That runner is going away. GitHub has announced (`actions/runner-images#13045`)
that `macos-15-intel` — the replacement for the already-removed `macos-13`
label, and the **last** x86_64 macOS image GitHub will offer — is scheduled
for decommission in **August 2027**. After that date there is no first-party
way to *natively* build or test an Intel macOS binary in CI.

The earlier `macos-13` → `macos-15-intel` migration (Session-22, commit
`a9333ba`) bought time but did not remove the underlying cliff; this ADR
removes it.

## Options considered

1. **Keep `macos-15-intel` until it disappears.** Carries a dated time-bomb:
   the release workflow silently loses a target in August 2027, and the
   runner has a history of multi-hour queue hangs (the reason the 30-minute
   `timeout-minutes` guard exists). Rejected — it just defers the decision.

2. **Cross-compile `x86_64-apple-darwin` from Linux via `cargo-zigbuild`.**
   Preserves the artifact past the runner sunset, but requires installing
   `zig` + vendoring a macOS SDK (`SDKROOT`) so the Apple frameworks that
   wgpu / winit / cpal link against resolve, and pinning a known-good
   Rust + zig pair (cargo-zigbuild has had `iconv`/SDK linker regressions on
   specific version combos). It also means the produced binary can no longer
   be *run* in CI (it is cross-built on Linux), so it could not participate in
   the multi-OS smoke test. ~3–5 engineer-days of plumbing + ongoing
   maintenance for a shrinking audience. Deferred — revisit only if Intel-Mac
   demand materialises.

3. **Drop `x86_64-apple-darwin` from the release matrix (CHOSEN).** Ship
   Linux x86_64, macOS aarch64, and Windows x86_64 prebuilt binaries; Intel
   Mac users build from source (`cargo build --release -p nes-frontend`).
   Apple Silicon shipped in 2020 and is the overwhelming majority of active
   macOS hardware; the binary is a convenience, not the only install path
   (the project is `cargo`-buildable on any host).

## Decision

Remove the `macos-15-intel` / `x86_64-apple-darwin` matrix entry from
`.github/workflows/release.yml` now (v1.6.0). Keep the `aarch64-apple-darwin`
(`macos-14`) entry. Document the source-build path for Intel Mac users in the
release notes and README.

This is reversible: re-adding the entry (or switching to Option 2) is a
matrix edit if demand surfaces.

## Consequences

- **Positive:** the August-2027 deprecation cliff is removed today; the
  release matrix is one job lighter and faster; no `cargo-zigbuild`/SDK
  maintenance burden.
- **Negative:** Intel-Mac users no longer get a prebuilt binary and must
  build from source. This is a real (if small and shrinking) capability drop;
  it is called out in the v1.6.0 release notes.
- **Neutral:** native development and CI on Intel Macs are unaffected — only
  the *release-artifact* target is dropped, not support for the platform.

## References

- `actions/runner-images#13045` — `macos-15-intel` August-2027 removal.
- `actions/runner-images#13046` — prior `macos-13` removal (2025-12-04).
- `docs/audit/ci-release-workflow-macos-x86_64-2026-05-22.md` — the Session-22
  `macos-13` → `macos-15-intel` migration this ADR follows.
- `docs/audit/gap-analysis-remediation-plan-2026-05-25.md` §2 (Milestone
  v1.6.0, task 0) — the planning context.
