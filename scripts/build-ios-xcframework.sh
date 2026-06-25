#!/usr/bin/env bash
# build-ios-xcframework.sh — assemble RustyNESFFI.xcframework + the generated
# Swift bindings for the v1.9.0 "Sunrise" iOS/iPadOS host.
#
# What it produces (all under ios/):
#   - ios/RustyNESFFI.xcframework  : the device + simulator static slices, each
#                                    carrying the C headers + the modulemap.
#   - ios/Generated/RustyNESCore.swift : the UniFFI-generated Swift surface
#                                    (the `NesController` class et al.).
#   - ios/RustyNES.xcodeproj       : regenerated from ios/project.yml (if
#                                    xcodegen is installed).
#
# A Rust `staticlib` is self-contained: `librustynes_ios.a` already bundles
# `rustynes-mobile` + `rustynes-core`, so the xcframework links exactly ONE
# archive per slice. The Swift bindings are generated from the device `.a`.
#
# Run on macOS with Xcode + the iOS Rust targets installed. This script does NOT
# sign or upload — that is fastlane's job (see fastlane/Fastfile).
set -euo pipefail

# --- locate the repo root (this script lives in scripts/) -------------------
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
ROOT="$(cd "${SCRIPT_DIR}/.." && pwd)"
cd "${ROOT}"

CRATE="rustynes-ios"
LIB="librustynes_ios.a"
IOS_DIR="${ROOT}/ios"
BUILD_DIR="${IOS_DIR}/build"
HEADERS_DIR="${BUILD_DIR}/headers"
XCFRAMEWORK="${IOS_DIR}/RustyNESFFI.xcframework"
GENERATED_DIR="${IOS_DIR}/Generated"

# Device + the two simulator ABIs (Apple-silicon sim + Intel sim).
TARGET_DEVICE="aarch64-apple-ios"
TARGET_SIM_ARM="aarch64-apple-ios-sim"
TARGET_SIM_X86="x86_64-apple-ios"

echo "==> Installing iOS Rust targets"
rustup target add "${TARGET_DEVICE}" "${TARGET_SIM_ARM}" "${TARGET_SIM_X86}"

echo "==> Building ${CRATE} (release) for each iOS ABI"
cargo build --release -p "${CRATE}" --target "${TARGET_DEVICE}"
cargo build --release -p "${CRATE}" --target "${TARGET_SIM_ARM}"
cargo build --release -p "${CRATE}" --target "${TARGET_SIM_X86}"

DEVICE_LIB="${ROOT}/target/${TARGET_DEVICE}/release/${LIB}"
SIM_ARM_LIB="${ROOT}/target/${TARGET_SIM_ARM}/release/${LIB}"
SIM_X86_LIB="${ROOT}/target/${TARGET_SIM_X86}/release/${LIB}"

echo "==> Lipo-ing the two simulator slices into one universal archive"
mkdir -p "${BUILD_DIR}"
SIM_LIB="${BUILD_DIR}/${LIB}"
lipo -create "${SIM_ARM_LIB}" "${SIM_X86_LIB}" -output "${SIM_LIB}"

echo "==> Generating Swift bindings from the device archive (UniFFI 0.31)"
# UniFFI's library mode reads the metadata baked into the staticlib and emits the
# Swift surface + the FFI C header + the modulemap. Generated from the device .a;
# the surface is ABI-identical across slices.
rm -rf "${HEADERS_DIR}"
mkdir -p "${HEADERS_DIR}"
cargo run -q -p rustynes-mobile --bin uniffi-bindgen -- \
  generate \
  --library "${DEVICE_LIB}" \
  --language swift \
  --out-dir "${HEADERS_DIR}"

# UniFFI emits `rustynes_mobileFFI.modulemap`; the xcframework wants it named
# `module.modulemap` so the slice is importable as a Clang module.
mv "${HEADERS_DIR}/rustynes_mobileFFI.modulemap" "${HEADERS_DIR}/module.modulemap"

# The hand-written iOS glue header (the Metal/audio FFI) ships alongside the
# UniFFI header so the app's bridging header can include it.
cp "${ROOT}/crates/${CRATE}/include/rustynes_ios.h" "${HEADERS_DIR}/rustynes_ios.h"

# The modulemap must expose BOTH headers (the UniFFI FFI header AND the hand-
# written glue header) so the linked binary's full C surface is a single module.
# UniFFI's generated modulemap references only `rustynes_mobileFFI.h`; append the
# glue header unless it is already present.
if ! grep -q 'rustynes_ios.h' "${HEADERS_DIR}/module.modulemap"; then
  cat >> "${HEADERS_DIR}/module.modulemap" <<'MODMAP'

// The hand-written RustyNES iOS Metal/audio C glue (rustynes-ios). Added by
// scripts/build-ios-xcframework.sh so the linked staticlib's whole C surface is
// one importable Clang module.
module RustyNESHostGlue {
    header "rustynes_ios.h"
    export *
}
MODMAP
fi

echo "==> Moving the generated Swift surface into ios/Generated/"
mkdir -p "${GENERATED_DIR}"
# UniFFI names the file after the crate: rustynes_mobile.swift. Check it into the
# app target as RustyNESCore.swift (a stable, app-friendly name).
mv "${HEADERS_DIR}/rustynes_mobile.swift" "${GENERATED_DIR}/RustyNESCore.swift"

echo "==> Assembling RustyNESFFI.xcframework"
rm -rf "${XCFRAMEWORK}"
xcodebuild -create-xcframework \
  -library "${DEVICE_LIB}" -headers "${HEADERS_DIR}" \
  -library "${SIM_LIB}" -headers "${HEADERS_DIR}" \
  -output "${XCFRAMEWORK}"

echo "==> Generating the Xcode project from ios/project.yml"
# XcodeGen authors a clean .pbxproj from the declarative spec. Guarded so the
# binding/xcframework steps above still succeed on a host without xcodegen
# (CI installs it via Homebrew before invoking this script).
if command -v xcodegen >/dev/null 2>&1; then
  xcodegen generate --spec "${IOS_DIR}/project.yml"
else
  echo "    (xcodegen not found; skipping project generation — install with 'brew install xcodegen')"
fi

echo "==> Done."
echo "    xcframework : ${XCFRAMEWORK}"
echo "    swift surface: ${GENERATED_DIR}/RustyNESCore.swift"
echo "    open ios/RustyNES.xcodeproj in Xcode to build/run, or 'fastlane ios beta' to ship."
