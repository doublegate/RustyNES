# Libretro Upstream Sync Guide

This document outlines the standard operating procedure for pushing future updates from RustyNES to the upstream Libretro organization (specifically the `libretro-super` and `libretro-core-info` repositories).

It uses the **"Re-fork"** method, which is ideal for infrequent, atomic updates where you prefer to delete your forks between contributions to keep your GitHub workspace clean.

## Repositories to Update

When RustyNES introduces new capabilities that require upstream awareness (like new supported extensions, features, or metadata changes), you may need to update one or both of the following upstream repositories:

1. **[libretro-super](https://github.com/libretro/libretro-super)**: Contains the build recipes used by the Libretro buildbot network.
2. **[libretro-core-info](https://github.com/libretro/libretro-core-info)**: Contains the `.info` files that RetroArch uses to identify the core's metadata, supported extensions, and capabilities.

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

- **Strict Alphabetical Ordering:** When modifying recipe lists (e.g., `recipes/apple/crates.conf`), ensure that the `rustynes_libretro` entry is placed in strict alphabetical order relative to the other cores.
- **Professionalism:** Maintain a professional, direct, and concise "core submission style" when drafting your PR descriptions.
- **Info File Validation:** When updating the `rustynes_libretro.info` file, verify that `supported_extensions` matches the exact list supported by the `rustynes-libretro` crate.

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
