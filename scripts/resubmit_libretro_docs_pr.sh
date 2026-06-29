#!/bin/bash
set -e

echo "======================================================"
echo " Resubmitting Pull Request to libretro/docs           "
echo "======================================================"
echo ""

echo "Closing original PR #1154 on libretro/docs..."
gh pr close 1154 --repo libretro/docs || echo "(PR might already be closed, continuing...)"

cd /tmp/docs

# Wait a second for github's backend to process
sleep 2

USER_LOGIN=$(gh api user -q .login)
echo "Pushing new 'feat/add-rustynes-core-v2' branch to your fork..."
git remote remove myfork 2>/dev/null || true
git remote add myfork "https://github.com/${USER_LOGIN}/docs.git"
git push -u myfork feat/add-rustynes-core-v2 --force

echo "Opening new Pull Request against libretro/docs..."
gh pr create \
    --repo libretro/docs \
    --head "${USER_LOGIN}:feat/add-rustynes-core-v2" \
    --base master
    --title "docs: Add RustyNES core documentation page" \
    --body "This PR adds the standard documentation page for the new RustyNES core and hooks it into the \`mkdocs.yml\` navigation tree under the Nintendo Entertainment System section.

*(Note: This replaces PR #1154, addressing all automated review feedback regarding page titles, database linking, softpatching links, and markdown lint whitespace issues).*
"

echo ""
echo "======================================================"
echo " All done! The PR has been successfully resubmitted.  "
echo "======================================================"
