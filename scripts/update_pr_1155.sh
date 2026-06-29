#!/bin/bash
set -e

echo "======================================================"
echo " Updating Pull Request #1155 on libretro/docs         "
echo "======================================================"
echo ""

cd /tmp/docs

# The fork 'myfork' should already exist from the previous script, but we ensure it's there
echo "Pushing fixes to 'feat/add-rustynes-core-v2' branch on your fork..."
git remote add myfork "https://github.com/$(gh api user -q .login)/docs.git" 2>/dev/null || true

# Simply push to the existing branch. GitHub automatically updates the PR.
git push -u myfork feat/add-rustynes-core-v2

echo ""
echo "======================================================"
echo " All done! The PR has been successfully updated.      "
echo "======================================================"
