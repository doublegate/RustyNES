# Libretro Upstream Sync Guide

This document outlines the standard operating procedure for pushing future updates from RustyNES to the upstream Libretro organization (specifically the `libretro-super` and `docs` repositories).

It uses the **"Re-fork"** method, which is ideal for infrequent, atomic updates where you prefer to delete your forks between contributions to keep your GitHub workspace clean.

## Repositories to Update

When RustyNES introduces new capabilities that require upstream awareness (like new supported extensions, features, or metadata changes), you may need to update one or both of the following upstream repositories:

1. **[libretro-super](https://github.com/libretro/libretro-super)**: Contains the build recipes used by the Libretro buildbot network, as well as the `.info` files that RetroArch uses to identify the core's metadata, supported extensions, and capabilities. *(Note: Do not submit PRs to `libretro-core-info`, as it is merely an automated mirror of the info files stored in `libretro-super`)*.
2. **[docs](https://github.com/libretro/docs)**: Contains the official Libretro documentation. You will need to update this repo when introducing the core, changing usage instructions, or adding new supported features.

## Step-by-Step "Re-fork" Workflow

### 1. Re-fork the Upstream Repositories

Since you delete your forks after a successful merge, start by navigating to the upstream repositories on GitHub and clicking **Fork**. This guarantees that your new fork is created from the absolute latest upstream `master` branch.

### 2. Clone and Branch Locally

Clone your newly created forks to your local machine and immediately check out a feature branch.

```bash
git clone https://github.com/YOUR-USERNAME/libretro-super.git
cd libretro-super
git checkout -b update-rustynes-recipe
```

### 3. Apply the Updates

Make the necessary changes.

**Critical Lessons Learned for Libretro PRs:**

- **Strict Alphabetical Ordering:** When modifying recipe lists (e.g., `recipes/apple/crates.conf`) or the `docs` repository menus/sidebars, ensure that the `rustynes_libretro` entry is placed in strict alphabetical order relative to the other cores.
- **Professionalism:** Maintain a professional, direct, and concise "core submission style" when drafting your PR descriptions.
- **Info File Validation:** When updating the `rustynes_libretro.info` file (submitted to `libretro-super`, not the mirror), verify that `supported_extensions` matches the exact list supported by the `rustynes-libretro` crate.

#### The `.info` file upstream is a SEPARATE COPY, and it went stale for eleven days

**This is the failure this section exists to prevent, and it already happened once.**

RetroArch does **not** read this repo's `crates/rustynes-libretro/rustynes_libretro.info`. It reads `dist/info/rustynes_libretro.info` from `libretro/libretro-super`, which the buildbot republishes and the frontend downloads. The two files are unrelated as far as any tooling is concerned — nothing syncs them, and nothing in either repo compares them.

So when v2.2.9 relicensed RustyNES from MIT/Apache-2.0 to **GPL-3.0-or-later** (ADR 0036), the change reached `Cargo.toml`, `NOTICE`, `deny.toml`, the SPDX headers, `docs/originality-and-provenance.md`, the README, and the local `.info` — and **not** the upstream copy. RetroArch went on advertising a GPL-3.0-or-later emulator as "MIT OR Apache-2.0", alongside a `display_version` of v2.2.1, until a user noticed.

Given that RustyNES's license is itself the outcome of a corrected provenance failure, a frontend misreporting it is a compliance problem, not a cosmetic one.

**Therefore:**

1. **A license change is a mandatory upstream-sync trigger**, on the same footing as a release. It is not a documentation-only change.

   **A version bump alone is not.** Maintainer decision at the v2.3.6 cut: upstream syncs are batched to MINOR releases, so the next one is **v2.4.0**. The upstream `dist/info/rustynes_libretro.info` therefore reads `display_version = "v2.3.5"` through the v2.3.6-v2.3.9 line, deliberately and not by oversight. The distinction that makes this safe is the one this whole document exists for: a stale `display_version` misreports a number, whereas a stale `license` misreports the terms under which the software is distributed — which is what actually went wrong in v2.2.9. Anything touching `license`, `supported_extensions`, or the core's declared capabilities still syncs immediately, regardless of where the version line sits.
2. `crates/rustynes-test-harness/tests/libretro_info_audit.rs` now pins the local `.info` against **two different sources of truth**, one per field, so the local file cannot drift and the upstream sync is a **copy**, never a re-derivation:
   - `license` and `display_version` — against `[workspace.package]` in the root `Cargo.toml`.
   - `supported_extensions` — against the **core's own** `retro_get_system_info` declaration in `crates/rustynes-libretro/src/lib.rs`, not the manifest, because that is where the list the core will actually load is defined. A literal repeated in the test would be a second copy of the fact rather than an audit of it.

   The audit cannot see the upstream repo — no test can — so the sync itself is still a human step.
3. **libretro `.info` files do not use SPDX.** They use short tokens and mark "or later" with a trailing `+`. Verified across all 316 core info files in `libretro-super`: `GPLv2` (100), `GPLv3` (64), `GPLv2+` (19), `GPLv3+` (5). RustyNES is GPL-3.0-**or-later**, so the correct token is **`GPLv3+`** — a bare `GPLv3` understates it as GPL-3.0-only. The audit encodes this mapping and fails with instructions if the license moves to something it has not been taught.

**Surfaces that must all be updated together:**

| Surface | Repo | Path |
| --- | --- | --- |
| Core metadata RetroArch reads | `libretro/libretro-super` | `dist/info/rustynes_libretro.info` |
| Public core docs page | `libretro/docs` | `docs/library/rustynes.md` |
| Local source of truth | this repo | `crates/rustynes-libretro/rustynes_libretro.info` |

**Every advertised field is in scope, not only the license.** The license is what
drifted, but nothing about the failure was license-specific — the same gap
applies to every field, and `display_version` had drifted too (stuck at v2.2.1).
Treat a change to any of these as requiring an upstream sync:

| Field | Syncs to `libretro-super` | Syncs to `libretro/docs` | Locally audited? |
| --- | :---: | :---: | --- |
| `license` | yes | yes (Author/License) | yes — vs `[workspace.package]` |
| `display_version` | yes | no | yes — vs `[workspace.package]` |
| `supported_extensions` | yes | yes (Extensions) | yes — vs the core's `retro_get_system_info` |
| `disk_control`, `savestate`, `cheats`, `core_options`, and the other capability flags | yes | yes (Features table) | no — assert by hand against the crate |
| mapper count / `description` | yes | no | no |
| `firmware*`, `database` | yes | yes (Databases / BIOS) | no |

The audited rows fail the test suite the moment they drift. The unaudited rows
are the ones to check by hand at release time — capability flags especially, since
advertising a capability the core lacks is worse than omitting one it has. That
exact defect shipped once already: `disk_control` was `false` while the FDS Disk
Control interface had been wired for months, hiding multi-disk swapping from
RetroArch's Quick Menu until v2.2.4 corrected it.

#### iOS / iPadOS / tvOS availability is a THIRD repo, and a hardcoded list

Being on the buildbot is **necessary but not sufficient** for Apple platforms. RustyNES has had a valid `ios-arm64` core on the buildbot for some time — a 1.3 MiB arm64 Mach-O exporting all 51 `retro_*` symbols, disk-control included — and it still does not appear in RetroArch on iOS or iPadOS.

iOS cannot download cores; Apple prohibits fetching executable code. The App Store build therefore **bundles** a fixed set, chosen by `pkg/apple/update-cores.sh` in [`libretro/RetroArch`](https://github.com/libretro/RetroArch). That script holds two lists:

| list | how it is populated | contains RustyNES? |
| --- | --- | --- |
| `allcores` | fetched dynamically from the buildbot directory listing | **yes**, automatically |
| `appstore_cores` | hardcoded array in the script | **no** |

The iOS and tvOS App Store build phases run `rm -f ${SRCROOT}/<platform>/modules/*.dylib` followed by `./update-cores.sh appstore` — so only the **hardcoded** list survives into the bundle. Being in the dynamic `allcores` buys nothing for App Store builds.

**The fix is a one-line PR to `libretro/RetroArch`** adding `rustynes` to `appstore_cores`. The same array feeds iOS, tvOS, and the macOS App Store build, so one entry covers all three. Cores are added there by explicit PR — historically by the Apple maintainer, and also by core authors (`pd777` was added by its own author), so a submission from us is the established route rather than an imposition.

**Alphabetical ordering is mandatory** (see the Strict Alphabetical Ordering note above — it applies to this array too). `rustynes` sorts between `reminiscence` and `sameboy`:

```sh
    reminiscence
    rustynes        # <- insert here
    sameboy
```

Check the surrounding lines at submission time rather than trusting this snippet; the array grows, and a misordered entry is the most common review comment on these PRs.

**Licensing note for the maintainer, not a blocker.** Adding RustyNES to `appstore_cores` means a GPL-3.0-or-later work gets distributed through the App Store, which carries the long-standing GPL-vs-App-Store-terms tension. RetroArch has already made this call for itself — RetroArch is GPLv3 and ships there, as do the GPLv3 cores `mesen` and `bsnes_hd_beta` — so there is clear precedent. It is still the copyright holder's decision to make deliberately rather than by default.

### 4. Commit and Push

Stage and commit your changes using clear, conventional commit messages.

```bash
git add .
git commit -m "Update RustyNES core recipe and dependencies"
git push origin update-rustynes-recipe
```

### 5. Submit the Pull Request

Go to the original upstream repository on GitHub and open a Pull Request comparing your `update-rustynes-recipe` branch against their `master` branch.

Provide a clear description of what changed in RustyNES to warrant the update.

### 6. Delete the Fork (Cleanup)

Once the Libretro maintainers accept and merge your PR(s) into their upstream `master` branch, your commits become a permanent part of their history.

At this point, you can safely navigate to your repository settings on GitHub and **delete the fork**. When you need to make another update in the future, simply return to Step 1.

---

## Pending sync — measured 2026-08-20, against upstream `master`

v2.4.0 item A. The obligation this file exists to discharge, with the **actual
diff** rather than a description of one, so the human step is a copy and not a
re-derivation. That is the same reason `libretro_info_audit.rs` exists: the v2.3.5
incident happened because a re-derivation was asked of a human and not performed.

Both surfaces were fetched read-only and compared. The result is smaller than
expected, and the shape is worth recording.

### 1. `libretro/libretro-super` — `dist/info/rustynes_libretro.info`

**One line.** Everything else is already in sync — including `license = "GPLv3+"`,
which `libretro-super#2069` landed on 2026-08-16, and the description's
`174 mapper families`.

```diff
-display_version = "v2.3.5"
+display_version = "v2.3.9"
```

Confirmed by diffing the upstream file against this repo's copy: two changed
lines total, which is the one field and its counterpart.

That the licence is already correct upstream is the part worth noting. The v2.3.5
release found `.info` advertising MIT/Apache-2.0 eleven days after the GPL
relicense; that specific failure is closed, and what remains is ordinary version
drift of four releases.

### 2. `libretro/docs` — `docs/library/rustynes.md`

**Still wrong, and it is the licence again.** The page reads:

```markdown
The RustyNES core is licensed under

- MIT OR Apache-2.0
```

RustyNES has been **GPL-3.0-or-later** since v2.2.9 (ADR 0036), as a derivative
work of GPL emulators. `libretro/docs#1180` is open against exactly this and was
filed at the time; it has not been actioned upstream.

This is the surface the v2.3.5 work did **not** reach, and it is the one a user
reads before the `.info`. The correction is:

```diff
 The RustyNES core is licensed under

-- MIT OR Apache-2.0
+- GPL-3.0-or-later
```

### Status — both filed, 2026-08-20

- **`libretro-super#2074`** — opened today. One line, `display_version` v2.3.5 ->
  v2.3.9. Verified before pushing: the branch's `dist/info/rustynes_libretro.info`
  is now **byte-identical** to this repository's copy, which is the property
  `libretro_info_audit.rs` exists to make possible — the sync is a copy rather
  than a re-derivation.
- **`libretro/docs#1180`** — **already open since 2026-08-16**, and it is a PULL
  REQUEST, not an issue. Re-verified today: `OPEN`, `MERGEABLE / CLEAN`, `+1/-1`,
  **zero comments** — correct, still applicable, simply unreviewed upstream. A
  second PR would be a duplicate.

  Worth recording the misreading that nearly produced one: `gh api
  repos/libretro/docs/issues/1180` returns the PR, because GitHub's *issues*
  endpoint serves pull requests too. That is what made an earlier pass here
  describe it as "an open issue" and conclude the docs fix still needed filing.
  Use `gh pr view` when the question is whether a change is already proposed.

### Why these are opened by hand rather than by tooling

Both are pull requests against third-party repositories — outward-facing actions
on projects this one does not own, so they are a maintainer decision rather than
something a sync script should perform. It is also why the audit in
`crates/rustynes-test-harness/tests/libretro_info_audit.rs` deliberately cannot
see upstream: a test that could would be a test that silently disagreed with a
repository nobody here controls.
