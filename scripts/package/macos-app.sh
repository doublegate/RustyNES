#!/usr/bin/env bash
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Wrap the release `rustynes` binary in a macOS application bundle (v3.7.1).
#
#   scripts/package/macos-app.sh <rustynes-binary> <version> <out-dir>
#
# writes <out-dir>/RustyNES.app:
#
#   RustyNES.app/Contents/Info.plist
#   RustyNES.app/Contents/MacOS/rustynes        (the binary, unchanged)
#   RustyNES.app/Contents/Resources/RustyNES.icns
#
# WHY. A bare executable has no icon on macOS (winit's window icon is a no-op
# there): Finder and the Dock take the icon from a bundle's Info.plist
# `CFBundleIconFile`. The icon is the pack's (`assets/RustyNES_Icon_Pack/
# macos/RustyNES.icns`).
#
# SIGNING. The bundle is signed AD HOC (`codesign -s -`) on macOS. Apple
# Silicon runs only signed code, and the linker's ad-hoc signature covers the
# binary but not the bundle around it, so the bundle is re-signed as a whole.
# Ad hoc is not a Developer ID and the app is not notarized. A downloaded copy
# is quarantined, so Gatekeeper still refuses it at first launch until the
# user allows it in System Settings > Privacy & Security, or clears the flag:
#   xattr -dr com.apple.quarantine RustyNES.app
# The release notes say so. The bare binary stays in the archive beside the
# bundle, for terminal use.
#
# On a non-macOS host the bundle is assembled but not signed or linted
# (`codesign` and `plutil` are macOS tools), which is enough to check its
# layout.
set -euo pipefail

bin="${1:?usage: macos-app.sh <rustynes-binary> <version> <out-dir>}"
version="${2:?usage: macos-app.sh <rustynes-binary> <version> <out-dir>}"
out="${3:?usage: macos-app.sh <rustynes-binary> <version> <out-dir>}"
version="${version#v}"
# Strict, because the version is written into the Info.plist XML below: a
# glob such as [0-9]*.[0-9]*.[0-9]* would let `<` or `&` through.
if ! [[ "$version" =~ ^[0-9]+\.[0-9]+\.[0-9]+$ ]]; then
  echo "macos-app.sh: version '$version' is not X.Y.Z" >&2
  exit 2
fi

root="$(cd "$(dirname "${BASH_SOURCE[0]}")/../.." && pwd)"
icns="$root/assets/RustyNES_Icon_Pack/macos/RustyNES.icns"
[ -f "$bin" ] || { echo "macos-app.sh: no binary at $bin" >&2; exit 2; }
[ -f "$icns" ] || { echo "macos-app.sh: no icon at $icns" >&2; exit 2; }

app="$out/RustyNES.app"
rm -rf "$app"
mkdir -p "$app/Contents/MacOS" "$app/Contents/Resources"
cp "$bin" "$app/Contents/MacOS/rustynes"
chmod +x "$app/Contents/MacOS/rustynes"
cp "$icns" "$app/Contents/Resources/RustyNES.icns"

# LSMinimumSystemVersion 11.0: the first macOS on Apple Silicon, the only
# macOS architecture the release ships (ADR 0009). NSHighResolutionCapable so
# the window is not drawn at 1x and scaled on a Retina display.
cat > "$app/Contents/Info.plist" <<PLIST
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
  <key>CFBundleDevelopmentRegion</key>
  <string>en</string>
  <key>CFBundleDisplayName</key>
  <string>RustyNES</string>
  <key>CFBundleExecutable</key>
  <string>rustynes</string>
  <key>CFBundleIconFile</key>
  <string>RustyNES</string>
  <key>CFBundleIdentifier</key>
  <string>com.doublegate.rustynes</string>
  <key>CFBundleInfoDictionaryVersion</key>
  <string>6.0</string>
  <key>CFBundleName</key>
  <string>RustyNES</string>
  <key>CFBundlePackageType</key>
  <string>APPL</string>
  <key>CFBundleShortVersionString</key>
  <string>${version}</string>
  <key>CFBundleVersion</key>
  <string>${version}</string>
  <key>LSApplicationCategoryType</key>
  <string>public.app-category.games</string>
  <key>LSMinimumSystemVersion</key>
  <string>11.0</string>
  <key>NSHighResolutionCapable</key>
  <true/>
  <key>NSHumanReadableCopyright</key>
  <string>GPL-3.0-or-later. See LICENSE and NOTICE.</string>
</dict>
</plist>
PLIST

if [ "$(uname -s)" = Darwin ]; then
  plutil -lint "$app/Contents/Info.plist"
  codesign --force --sign - "$app"
  codesign --verify --strict --verbose=2 "$app"
else
  echo "macos-app.sh: not on macOS; $app assembled, not signed or linted"
fi
echo "macos-app.sh: wrote $app ($version)"
