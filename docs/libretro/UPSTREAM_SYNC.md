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
2. `crates/rustynes-test-harness/tests/libretro_info_audit.rs` now pins the local `.info`'s `license`, `display_version`, and `supported_extensions` against the workspace manifest, so the local file cannot drift and the upstream sync is a **copy**, never a re-derivation. It cannot see the upstream repo — no test can — so the sync itself is still a human step.
3. **libretro `.info` files do not use SPDX.** They use short tokens and mark "or later" with a trailing `+`. Verified across all 316 core info files in `libretro-super`: `GPLv2` (100), `GPLv3` (64), `GPLv2+` (19), `GPLv3+` (5). RustyNES is GPL-3.0-**or-later**, so the correct token is **`GPLv3+`** — a bare `GPLv3` understates it as GPL-3.0-only. The audit encodes this mapping and fails with instructions if the license moves to something it has not been taught.

**Surfaces that must all be updated together on a license change:**

| Surface | Repo | Path |
| --- | --- | --- |
| Core metadata RetroArch reads | `libretro/libretro-super` | `dist/info/rustynes_libretro.info` |
| Public core docs page | `libretro/docs` | `docs/library/rustynes.md` (Author/License) |
| Local source of truth | this repo | `crates/rustynes-libretro/rustynes_libretro.info` |

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
