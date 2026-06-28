#!/bin/bash
set -e

echo "======================================================"
echo " Submitting Pull Requests to upstream Libretro repos  "
echo "======================================================"
echo ""

# 1. libretro-super
echo "[1/2] Preparing libretro-super integration..."
cd /tmp/libretro-super

# Fork if necessary (gh handles this intelligently if it already exists)
echo "Forking libretro/libretro-super under your account..."
gh repo fork libretro/libretro-super --clone=false

# Wait a second for github's backend to register the fork
sleep 3

# Add the fork as a remote and push the branch
echo "Pushing 'feat/add-rustynes-core' branch to your fork..."
git remote add myfork "https://github.com/$(gh api user -q .login)/libretro-super.git" || true
git push -u myfork feat/add-rustynes-core --force

echo "Opening Pull Request against libretro/libretro-super..."
gh pr create \
    --repo libretro/libretro-super \
    --head "$(gh api user -q .login):feat/add-rustynes-core" \
    --base master \
    --title "Add RustyNES core integration" \
    --body "This PR integrates the RustyNES libretro core into the buildbot infrastructure.
It adds the \`rustynes_libretro.info\` metadata file and registers the core across standard desktop targets (Linux, Windows, OSX)."

echo ""
echo "[2/2] Preparing libretro-docs integration..."
cd /tmp/docs

# Fork if necessary
echo "Forking libretro/docs under your account..."
gh repo fork libretro/docs --clone=false
sleep 3

# Add the fork as a remote and push the branch
echo "Pushing 'feat/add-rustynes-core' branch to your fork..."
git remote add myfork "https://github.com/$(gh api user -q .login)/docs.git" || true
git push -u myfork feat/add-rustynes-core --force

echo "Opening Pull Request against libretro/docs..."
gh pr create \
    --repo libretro/docs \
    --head "$(gh api user -q .login):feat/add-rustynes-core" \
    --base master \
    --title "docs: Add RustyNES core documentation page" \
    --body "This PR adds the standard documentation page for the new RustyNES core and hooks it into the \`mkdocs.yml\` navigation tree under the Nintendo Entertainment System section."

echo ""
echo "======================================================"
echo " All done! Both PRs have been submitted successfully. "
echo "======================================================"
