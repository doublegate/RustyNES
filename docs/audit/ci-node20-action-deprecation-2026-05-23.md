# CI — Node 20 Actions Deprecation Bump (`actions/*@v4` → `@v5`, `softprops/action-gh-release@v2` → `@v3`)

**Date:** 2026-05-23
**Workflows:** `.github/workflows/ci.yml`, `.github/workflows/release.yml`
**Outcome:** Fix shipped; deprecation warnings expected to clear on the next workflow run.

---

## Summary

GitHub Actions has surfaced **"Node.js 20 actions are deprecated"** warnings on
recent CI and Release workflow invocations of this project. Node 20 hits
end-of-support in 2026; GitHub is migrating the action runtime to Node 24 via
the `@v5` series of `actions/*` first-party packages and equivalent major-version
bumps on third-party actions. This audit bumps every Node-20-pinned action used
by this project to its current Node-24 release, in a single workflow-only commit
with no observable behavior change at the call sites.

This is a sibling fix to the Session-22 `macos-13` → `macos-15-intel` migration
documented in `ci-release-workflow-macos-x86_64-2026-05-22.md` — both clear
deprecation warnings surfaced by upstream lifecycle events that are external to
this repository. Neither required a re-trigger of the workflow on user-paid
Actions minutes; the fix is pushed and verification is deferred to the next
naturally-occurring CI run on `main` or the next release tag.

---

## Inventory of `uses:` invocations (pre-fix)

| Workflow      | Job             | Step              | Action                              | Pin     | Runtime      |
|---------------|-----------------|-------------------|-------------------------------------|---------|--------------|
| `ci.yml`      | `fmt`           | checkout          | `actions/checkout`                  | `@v4`   | **Node 20**  |
| `ci.yml`      | `fmt`           | toolchain         | `dtolnay/rust-toolchain`            | `@stable` | composite  |
| `ci.yml`      | `clippy`        | checkout          | `actions/checkout`                  | `@v4`   | **Node 20**  |
| `ci.yml`      | `clippy`        | toolchain         | `dtolnay/rust-toolchain`            | `@stable` | composite  |
| `ci.yml`      | `clippy`        | cache             | `Swatinem/rust-cache`               | `@v2`   | Node 24 (v2.9.1) |
| `ci.yml`      | `test`          | checkout          | `actions/checkout`                  | `@v4`   | **Node 20**  |
| `ci.yml`      | `test`          | toolchain         | `dtolnay/rust-toolchain`            | `@master` | composite  |
| `ci.yml`      | `test`          | cache             | `Swatinem/rust-cache`               | `@v2`   | Node 24      |
| `ci.yml`      | `test-roms`     | checkout          | `actions/checkout`                  | `@v4`   | **Node 20**  |
| `ci.yml`      | `test-roms`     | toolchain         | `dtolnay/rust-toolchain`            | `@stable` | composite  |
| `ci.yml`      | `test-roms`     | cache             | `Swatinem/rust-cache`               | `@v2`   | Node 24      |
| `ci.yml`      | `doc`           | checkout          | `actions/checkout`                  | `@v4`   | **Node 20**  |
| `ci.yml`      | `doc`           | toolchain         | `dtolnay/rust-toolchain`            | `@stable` | composite  |
| `ci.yml`      | `doc`           | cache             | `Swatinem/rust-cache`               | `@v2`   | Node 24      |
| `ci.yml`      | `no_std_check`  | checkout          | `actions/checkout`                  | `@v4`   | **Node 20**  |
| `ci.yml`      | `no_std_check`  | toolchain         | `dtolnay/rust-toolchain`            | `@stable` | composite  |
| `ci.yml`      | `no_std_check`  | cache             | `Swatinem/rust-cache`               | `@v2`   | Node 24      |
| `release.yml` | `build` matrix  | checkout          | `actions/checkout`                  | `@v4`   | **Node 20**  |
| `release.yml` | `build` matrix  | toolchain         | `dtolnay/rust-toolchain`            | `@stable` | composite  |
| `release.yml` | `build` matrix  | cache             | `Swatinem/rust-cache`               | `@v2`   | Node 24      |
| `release.yml` | `build` matrix  | upload release    | `softprops/action-gh-release`       | `@v2`   | **Node 20**  |

**Net Node-20 invocations:** 8 (= 7× `actions/checkout@v4` + 1× `softprops/action-gh-release@v2`).
**Already-Node-24 invocations:** 6× `Swatinem/rust-cache@v2` (the `v2` major tag advances Node runtimes internally — `v2.9.1` ships `using: node24`).
**Composite (not Node-runtime-affected):** 7× `dtolnay/rust-toolchain` (composite shell action, no Node).

The `dtolnay/rust-toolchain` and `Swatinem/rust-cache` invocations are intentionally
NOT touched by this fix: they are either composite (no Node runtime to migrate)
or have already migrated to Node 24 within their existing major-version tag.
Bumping them would be churn for no warning-reduction benefit.

---

## Per-action upgrade research

### `actions/checkout`

| Tag        | Date       | Node runtime |
|------------|------------|--------------|
| `v4.3.1`   | 2025-11-17 | Node 20      |
| `v5.0.0`   | 2025-08-11 | **Node 24**  |
| `v5.0.1`   | 2025-11-17 | Node 24      |
| `v6.0.0`   | 2025-11-20 | Node 24      |
| `v6.0.2`   | 2026-01-09 | Node 24 (latest) |

Source: `gh release list --repo actions/checkout`, `gh api repos/actions/checkout/contents/action.yml?ref=<tag>`.

**Chosen target: `@v5`** (the minimum-risk Node 24 floor). v5.0.0 release notes
state: "Update actions checkout to use node 24" — purely a runtime swap with no
input-surface changes; the `compare/v4...v5.0.0` diff is dominated by `dist/`
rebuild and a `@types/node` bump. v6 additionally changes credential persistence
(`actions/checkout#2286` — "Persist creds to a separate file") and adds a v6-beta
graduation commit; orthogonal to the Node-20 deprecation surface and not needed
for this fix. Pinning at `@v5` rather than `@v5.0.1` matches the floating-major-tag
convention used elsewhere in these workflows (`@v2` for `Swatinem/rust-cache`,
`@v3` for the new `softprops/action-gh-release` pin).

Release notes URL: <https://github.com/actions/checkout/releases/tag/v5.0.0>.

**Breaking-change audit at our call sites:** none. All 7 invocations of
`actions/checkout` in this repo are the bare default-input form — no `path:`,
no `submodules:`, no `token:`, no `ref:`, no `fetch-depth:`. The v4 → v5
behavioral diff is the Node runtime swap; default-input behavior is unchanged.

### `softprops/action-gh-release`

| Tag        | Date       | Node runtime |
|------------|------------|--------------|
| `v2.6.2`   | 2026-04-12 | Node 20      |
| `v3.0.0`   | 2026-04-12 | **Node 24**  |

Source: `gh release list --repo softprops/action-gh-release`,
`gh api repos/softprops/action-gh-release/contents/action.yml?ref=<tag>`.

**Chosen target: `@v3`** (the only Node 24 line). v3.0.0 release notes verbatim:

> `3.0.0` is a major release that moves the action runtime from Node 20 to
> Node 24. Use `v3` on GitHub-hosted runners and self-hosted fleets that already
> support the Node 24 Actions runtime. If you still need the last
> Node 20-compatible line, stay on `v2.6.2`.
> ## What's Changed
> ### Other Changes 🔄
> * Move the action runtime and bundle target to Node 24
> * Update `@types/node` to the Node 24 line and allow future Dependabot updates
> * Keep the floating major tag on `v3`; `v2` remains pinned to the latest `2.x` release

Release notes URL: <https://github.com/softprops/action-gh-release/releases/tag/v3.0.0>.

**Breaking-change audit at our call site:** none. Our single invocation passes
five inputs — `tag_name`, `files`, `generate_release_notes`, `body`,
`fail_on_unmatched_files` — all of which were present and unchanged in v2.6.2
and remain present in v3.0.0. The `body` string and `GITHUB_TOKEN` env var
surface are identical. The v2 → v3 diff is exclusively the Node 20 → Node 24
runtime swap.

### Actions deliberately not touched

- **`Swatinem/rust-cache@v2`** — the major-version `v2` tag rolls through point
  releases; `v2.9.1` (latest, 2026-03-12) ships `using: node24` in its
  `action.yml`. No upgrade necessary; the deprecation warning will not fire on
  this action.
- **`dtolnay/rust-toolchain@stable`** and **`@master`** — composite shell action;
  no Node runtime. Not in the deprecation surface.

### Actions remaining on Node 20 with no Node 24 release yet

None. All Node-20-affected actions used by this repository have a current Node 24
release available.

### Deprecated-entirely actions (`actions-rs/*`)

None used by this repository. The `dtolnay/rust-toolchain` + `Swatinem/rust-cache`
combination is already the canonical post-`actions-rs/*` replacement set and
remains current.

---

## Applied changes

Two files modified; one new audit file (this document) created. No code, test,
or non-workflow documentation changed.

### `.github/workflows/ci.yml` — 6 line edits

```diff
@@ jobs.fmt @@
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5

@@ jobs.clippy @@
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5

@@ jobs.test @@
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5

@@ jobs.test-roms @@
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5

@@ jobs.doc @@
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5

@@ jobs.no_std_check @@
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5
```

### `.github/workflows/release.yml` — 3 edits (1 `uses:`, 1 `uses:`, 1 comment refresh)

```diff
@@ file header comment @@
-# softprops/action-gh-release@v2 is used; v1 was deprecated mid-2025 and emits
-# a warning. Permissions are minimum-necessary (contents: write only).
+# softprops/action-gh-release@v3 is used; v1 was deprecated mid-2025 and v2
+# (Node 20) joined the Node-20-deprecated set in 2026 — v3 moves the action
+# runtime to Node 24 with no input-surface changes. Permissions are
+# minimum-necessary (contents: write only).

@@ jobs.build matrix, checkout step @@
-      - uses: actions/checkout@v4
+      - uses: actions/checkout@v5

@@ jobs.build matrix, upload release step @@
-        uses: softprops/action-gh-release@v2
+        uses: softprops/action-gh-release@v3
```

---

## YAML validation

```bash
python3 -c "import yaml; d=yaml.safe_load(open('.github/workflows/ci.yml').read()); print('ci.yml:', 'OK' if d else 'FAIL')"
# ci.yml: OK

python3 -c "import yaml; d=yaml.safe_load(open('.github/workflows/release.yml').read()); print('release.yml:', 'OK' if d else 'FAIL')"
# release.yml: OK
```

Both files parse cleanly.

---

## Re-validation plan

No `workflow_dispatch` re-trigger was performed as part of this fix; per project
convention (mirrored from the Session-22 macOS fix), spending Actions minutes on
a synthetic re-run is the user's product call. The natural verification path is:

1. **The next push to `main`** (CI workflow) will exercise all 6 `actions/checkout@v5`
   invocations in the 6 jobs of `ci.yml`. If the Node 20 warning re-appears, the
   upgrade was incomplete; if it disappears, the bump cleared the surface.

2. **The next release tag** (Release workflow) will exercise the `actions/checkout@v5`
   + `softprops/action-gh-release@v3` invocations together. Independent of the
   Node 24 migration, this also re-validates the Session-22 `macos-15-intel` runner
   swap for the `x86_64-apple-darwin` matrix entry.

Neither verification path costs anything beyond Actions minutes that would be
spent on naturally-occurring runs.

---

## Follow-up items (out of scope)

1. **`actions/checkout` minor-version drift.** This fix pins the floating-major
   `@v5` tag. If a future security advisory necessitates pinning to a specific
   tag (`@v5.0.1` or higher), that's a separate, narrower fix.

2. **Future Node 24 deprecation.** Node 24 will itself be deprecated at some
   future date (likely 2028+). When that surface fires, this same audit pattern
   should be repeated: identify every `uses:` with a Node-deprecated runtime,
   bump to the new major, document.

3. **Dependabot for Actions.** Not currently enabled for `.github/workflows/`.
   Enabling `package-ecosystem: github-actions` in `.github/dependabot.yml`
   would surface action-version upgrades as PRs automatically and pre-empt
   future manual rounds of this audit. Defer to user product decision.
   **Status: addressed in follow-up below.**

---

## Follow-up: Dependabot config landed (2026-05-23)

The Dependabot follow-up flagged above shipped as `.github/dependabot.yml`
in a separate commit on `main` the same day. Decision: **comprehensive** — both
`github-actions` and `cargo` ecosystems are tracked, not the narrow
`github-actions`-only scope.

### Per-ecosystem configuration

| Ecosystem        | Schedule  | Versioning      | Groups                                                              | PR cap |
|------------------|-----------|-----------------|---------------------------------------------------------------------|--------|
| `github-actions` | weekly    | (default)       | `actions-core` (`actions/*`), `rust-tooling` (`dtolnay/*`, `Swatinem/*`, `taiki-e/*`), `release-tooling` (`softprops/*`) | 5      |
| `cargo`          | monthly   | `lockfile-only` | `patch-and-minor` (rolls up semver-compatible bumps); majors stay individual | 5      |

### Rationale

- **`github-actions` weekly.** Low-volume by nature (4 distinct actions across
  8 invocations across `ci.yml` + `release.yml`). Grouping by upstream owner
  means a Node-runtime bump that touches multiple `actions/*` arrives as a
  single PR — the exact ergonomic this audit's manual round optimized for.

- **`cargo` monthly + `lockfile-only`.** The workspace has 17 direct
  `[workspace.dependencies]` entries plus a much larger transitive surface
  (wgpu + winit + egui each pull in 100+ crates). Monthly cadence dampens PR
  volume; `lockfile-only` ensures Cargo.toml manifest edits (the major-line
  pins like `wgpu = "22"`, `bitflags = "2.6"`) stay human-driven. Patches +
  minors group into one batched PR per month; majors stay individual because
  they are likely breaking and want manual review against the 537-strict-test
  suite + 60-ROM commercial-ROM oracle.

- **`rust-toolchain.toml` deliberately untracked.** Dependabot does not have a
  native `rust-toolchain.toml` ecosystem. The project's pinned 1.86.0 channel
  is required for `edition2024` transitive deps in the frontend stack
  (`icu_*`, `idna_adapter` via wgpu / winit / egui). Toolchain bumps remain
  manual.

### Motivating events

This config closes the loop on two manual migrations:

- **Session-22 (2026-05-22):** `macos-13` GitHub runner label fully
  deprecated; release workflow's `x86_64-apple-darwin` matrix entry was
  manually swapped to `macos-15-intel`. See
  `docs/audit/ci-release-workflow-macos-x86_64-2026-05-22.md`.
- **Session-24 (2026-05-23, this audit):** Node 20 actions deprecated;
  `actions/checkout@v4` + `softprops/action-gh-release@v2` manually bumped to
  Node 24 majors (`@v5` and `@v3` respectively). Documented above.

Both manual rounds would have arrived as Dependabot PRs under the new config
weeks before the deprecation warnings fired. The first natural Dependabot
trigger is Monday 2026-05-25 06:00 UTC for `github-actions`; first `cargo`
run is 2026-06-01 06:00 UTC.

### References

- Dependabot config docs:
  <https://docs.github.com/en/code-security/dependabot/dependabot-version-updates/configuration-options-for-the-dependabot.yml-file>
- Sibling audit (Session-22 macOS): `docs/audit/ci-release-workflow-macos-x86_64-2026-05-22.md`

---

## References

- GitHub Node 20 deprecation context: see release notes for the major-version
  bumps below; GitHub has not (yet) published a single consolidated deprecation
  announcement issue for Node 20 the way they did for Node 16
  (`actions/runner#3373`). The per-action release notes are the authoritative
  source.
- `actions/checkout@v5.0.0` release notes (Node 20 → Node 24):
  <https://github.com/actions/checkout/releases/tag/v5.0.0>
- `actions/checkout@v6.0.0` release notes (credential persistence change,
  orthogonal to this fix; documented for completeness):
  <https://github.com/actions/checkout/releases/tag/v6.0.0>
- `softprops/action-gh-release@v3.0.0` release notes (Node 20 → Node 24):
  <https://github.com/softprops/action-gh-release/releases/tag/v3.0.0>
- `Swatinem/rust-cache@v2.9.1` `action.yml` (`using: node24` confirmation, no
  bump needed):
  <https://github.com/Swatinem/rust-cache/blob/v2.9.1/action.yml>
- Sibling audit doc — Session-22 `macos-13` → `macos-15-intel`:
  `docs/audit/ci-release-workflow-macos-x86_64-2026-05-22.md`
