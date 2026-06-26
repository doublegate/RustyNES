# Release notes (publish overrides)

This directory holds optional **maintainer-authored** GitHub Release notes that the
[`release-auto.yml`](../workflows/release-auto.yml) workflow publishes when a new
version goes final-green on `main`.

> Not to be confused with [`docs/release-notes/`](../../docs/release-notes/), which
> is the **engine-lineage history archive** (those are not RustyNES release notes).
> Publish overrides live here, under `.github/`, to keep the two separate.

## How automated releases work

1. A PR bumps the workspace version in `Cargo.toml` and moves the `CHANGELOG.md`
   `[Unreleased]` section to `[X.Y.Z]`.
2. The PR merges to `main`; CI runs on `main` and goes green (all review threads
   adjudicated, fixes landed).
3. `release-auto.yml` fires on that green CI run. If no `vX.Y.Z` tag exists yet, it
   resolves the notes + title, creates the tag and the GitHub Release, then invokes
   `release.yml` to build and attach the platform binaries (Linux x86_64, macOS
   aarch64, Windows x86_64).

The workflow is idempotent: if the version's tag already exists it is a clean
no-op, so it fires harmlessly on every `main` build.

## Where the notes come from

For version `X.Y.Z`, the release body is resolved in this order:

1. **`.github/release-notes/vX.Y.Z.md`** (preferred) — a hand-written,
   comprehensive, technically-detailed notes file. Commit it as part of the
   release PR for the richest result.
2. **The `## [X.Y.Z]` section of `CHANGELOG.md`** (fallback) — used automatically
   when no override file exists.
3. If **neither** exists, the workflow **fails loudly** — a release never ships
   with empty notes.

The release **title** is `RustyNES vX.Y.Z — <codename/theme>`, with the
codename/theme parsed from the `CHANGELOG.md` header line
(`## [X.Y.Z] - <date> - "<Codename>" (<theme>)`).

## Authoring an override file

Match the house style of the published releases (see any release at
<https://github.com/doublegate/RustyNES/releases>):

- Open with `# RustyNES vX.Y.Z — "<Codename>"` and a paragraph stating what the
  release is plus the byte-identical / AccuracyCoin framing.
- Per-area `## Section` blocks for substantial releases; a tighter flow for point
  releases.
- A `## Verification` summary, a `## Deferred` / carryover list where apt, and a
  closing `## Install` block (the binaries attached below + the web build + links
  to the relevant docs / ADRs + the license line).
- No emojis. These files are linted by markdownlint like the rest of the repo.

The `release.yml` build matrix only attaches artifacts; it never sets the release
body, so the notes resolved here are preserved.
