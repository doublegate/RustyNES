# RustyNES Desktop Build Guide

Complete instructions for building the RustyNES desktop application on Windows, macOS, and Linux.

## Prerequisites

### All Platforms

- **Rust toolchain** 1.86.0 or later
- **Git** for source control
- **CMake** 3.15+ (for some native dependencies)

### Windows

```powershell
# Install Rust via rustup
winget install Rustlang.Rust.MSVC

# Or download from https://rustup.rs

# Visual Studio Build Tools (required for native dependencies)
winget install Microsoft.VisualStudio.2022.BuildTools

# During installation, select:
# - Desktop development with C++
# - Windows 10/11 SDK
```

### macOS

```bash
# Install Xcode command line tools
xcode-select --install

# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install Homebrew dependencies
brew install cmake pkg-config
```

### Linux (Debian/Ubuntu)

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install development dependencies
sudo apt-get update
sudo apt-get install -y \
    build-essential \
    cmake \
    pkg-config \
    libgtk-3-dev \
    libxcb-render0-dev \
    libxcb-shape0-dev \
    libxcb-xfixes0-dev \
    libxkbcommon-dev \
    libssl-dev \
    libasound2-dev \
    libpulse-dev \
    libsdl2-dev
```

### Linux (Fedora)

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install development dependencies
sudo dnf install -y \
    gcc \
    cmake \
    pkg-config \
    gtk3-devel \
    libxcb-devel \
    libxkbcommon-devel \
    openssl-devel \
    alsa-lib-devel \
    pulseaudio-libs-devel \
    SDL2-devel
```

### Linux (Arch)

```bash
# Install Rust via rustup
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh

# Install development dependencies
sudo pacman -S --needed \
    base-devel \
    cmake \
    pkg-config \
    gtk3 \
    libxcb \
    libxkbcommon \
    openssl \
    alsa-lib \
    pulseaudio \
    sdl2
```

## Building

### Clone Repository

```bash
git clone https://github.com/doublegate/RustyNES.git
cd RustyNES
```

### Debug Build

```bash
# Build entire workspace
cargo build --workspace

# Build only desktop application
cargo build -p rustynes-desktop

# Run directly
cargo run -p rustynes-desktop
```

### Release Build

```bash
# Optimized release build
cargo build --release --workspace

# Build with all optimizations
RUSTFLAGS="-C target-cpu=native" cargo build --release -p rustynes-desktop
```

### Build with Features

```bash
# Build with all features enabled
cargo build -p rustynes-desktop --release --all-features

# Build with specific features
cargo build -p rustynes-desktop --release --features "lua,netplay,achievements"

# Build minimal version (no optional features)
cargo build -p rustynes-desktop --release --no-default-features
```

## Feature Flags

| Feature | Description | Default |
|---------|-------------|---------|
| `default` | Standard desktop features | Yes |
| `lua` | Lua 5.4 scripting support | No |
| `netplay` | GGPO rollback netcode | No |
| `achievements` | RetroAchievements integration | No |
| `debugger` | Integrated debugging tools | No |
| `expansion-audio` | VRC6/VRC7/N163/MMC5/FDS audio | Yes |

### Cargo.toml Features

```toml
[features]
default = ["expansion-audio", "rewind"]
lua = ["mlua"]
netplay = ["backroll", "tokio"]
achievements = ["rcheevos-sys"]
debugger = []
expansion-audio = []
rewind = []
all = ["lua", "netplay", "achievements", "debugger", "expansion-audio", "rewind"]
```

## Platform-Specific Configuration

### Windows

#### Audio Backend

```toml
# Cargo.toml - Windows audio
[target.'cfg(windows)'.dependencies]
cpal = { version = "0.15", features = ["wasapi"] }
```

#### High DPI Support

RustyNES automatically handles DPI scaling on Windows. The manifest includes:

```xml
<!-- rustynes-desktop/windows/app.manifest -->
<?xml version="1.0" encoding="UTF-8" standalone="yes"?>
<assembly xmlns="urn:schemas-microsoft-com:asm.v1" manifestVersion="1.0">
  <application xmlns="urn:schemas-microsoft-com:asm.v3">
    <windowsSettings>
      <dpiAware xmlns="http://schemas.microsoft.com/SMI/2005/WindowsSettings">true/pm</dpiAware>
      <dpiAwareness xmlns="http://schemas.microsoft.com/SMI/2016/WindowsSettings">permonitorv2,permonitor</dpiAwareness>
    </windowsSettings>
  </application>
</assembly>
```

#### Windows Build Script

```powershell
# build-windows.ps1
$ErrorActionPreference = "Stop"

Write-Host "Building RustyNES for Windows..."

# Set environment for release build
$env:RUSTFLAGS = "-C target-cpu=native"

# Build release
cargo build --release -p rustynes-desktop --features "all"

# Copy to dist folder
New-Item -ItemType Directory -Force -Path "dist/windows"
Copy-Item "target/release/rustynes-desktop.exe" "dist/windows/RustyNES.exe"

# Copy runtime dependencies if any
# Copy-Item "path/to/dll" "dist/windows/"

Write-Host "Build complete: dist/windows/RustyNES.exe"
```

### macOS

#### App Bundle Creation

```bash
#!/bin/bash
# build-macos.sh

set -e

echo "Building RustyNES for macOS..."

# Build universal binary (Intel + Apple Silicon)
cargo build --release -p rustynes-desktop --target x86_64-apple-darwin
cargo build --release -p rustynes-desktop --target aarch64-apple-darwin

# Create universal binary
mkdir -p target/universal-apple-darwin/release
lipo -create \
    target/x86_64-apple-darwin/release/rustynes-desktop \
    target/aarch64-apple-darwin/release/rustynes-desktop \
    -output target/universal-apple-darwin/release/rustynes-desktop

# Create app bundle
APP_NAME="RustyNES"
APP_DIR="dist/macos/${APP_NAME}.app"

mkdir -p "${APP_DIR}/Contents/MacOS"
mkdir -p "${APP_DIR}/Contents/Resources"

# Copy binary
cp target/universal-apple-darwin/release/rustynes-desktop "${APP_DIR}/Contents/MacOS/${APP_NAME}"

# Create Info.plist
cat > "${APP_DIR}/Contents/Info.plist" << 'EOF'
<?xml version="1.0" encoding="UTF-8"?>
<!DOCTYPE plist PUBLIC "-//Apple//DTD PLIST 1.0//EN" "http://www.apple.com/DTDs/PropertyList-1.0.dtd">
<plist version="1.0">
<dict>
    <key>CFBundleExecutable</key>
    <string>RustyNES</string>
    <key>CFBundleIdentifier</key>
    <string>com.rustynes.emulator</string>
    <key>CFBundleName</key>
    <string>RustyNES</string>
    <key>CFBundleVersion</key>
    <string>0.1.0</string>
    <key>CFBundleShortVersionString</key>
    <string>0.1.0</string>
    <key>CFBundleIconFile</key>
    <string>AppIcon</string>
    <key>CFBundlePackageType</key>
    <string>APPL</string>
    <key>LSMinimumSystemVersion</key>
    <string>10.15</string>
    <key>NSHighResolutionCapable</key>
    <true/>
    <key>CFBundleDocumentTypes</key>
    <array>
        <dict>
            <key>CFBundleTypeExtensions</key>
            <array>
                <string>nes</string>
            </array>
            <key>CFBundleTypeName</key>
            <string>NES ROM</string>
            <key>CFBundleTypeRole</key>
            <string>Viewer</string>
        </dict>
    </array>
</dict>
</plist>
EOF

# Copy icon (if exists)
if [ -f "assets/icon.icns" ]; then
    cp assets/icon.icns "${APP_DIR}/Contents/Resources/AppIcon.icns"
fi

echo "Build complete: ${APP_DIR}"
```

#### Code Signing (Optional)

```bash
# Sign the app bundle
codesign --force --deep --sign "Developer ID Application: Your Name" \
    dist/macos/RustyNES.app

# Notarize for distribution
xcrun notarytool submit dist/macos/RustyNES.app.zip \
    --apple-id "your@email.com" \
    --team-id "TEAM_ID" \
    --password "app-specific-password" \
    --wait

# Staple the ticket
xcrun stapler staple dist/macos/RustyNES.app
```

### Linux

#### AppImage Creation

```bash
#!/bin/bash
# build-appimage.sh

set -e

echo "Building RustyNES AppImage..."

# Build release
cargo build --release -p rustynes-desktop

# Create AppDir structure
APP_DIR="dist/AppDir"
mkdir -p "${APP_DIR}/usr/bin"
mkdir -p "${APP_DIR}/usr/share/applications"
mkdir -p "${APP_DIR}/usr/share/icons/hicolor/256x256/apps"

# Copy binary
cp target/release/rustynes-desktop "${APP_DIR}/usr/bin/rustynes"

# Create desktop file
cat > "${APP_DIR}/usr/share/applications/rustynes.desktop" << 'EOF'
[Desktop Entry]
Name=RustyNES
Comment=NES Emulator
Exec=rustynes %f
Icon=rustynes
Terminal=false
Type=Application
Categories=Game;Emulator;
MimeType=application/x-nes-rom;
EOF

# Copy icon
if [ -f "assets/icon.png" ]; then
    cp assets/icon.png "${APP_DIR}/usr/share/icons/hicolor/256x256/apps/rustynes.png"
fi

# Create AppRun
cat > "${APP_DIR}/AppRun" << 'EOF'
#!/bin/bash
SELF=$(readlink -f "$0")
HERE=${SELF%/*}
export PATH="${HERE}/usr/bin:${PATH}"
export LD_LIBRARY_PATH="${HERE}/usr/lib:${LD_LIBRARY_PATH}"
exec "${HERE}/usr/bin/rustynes" "$@"
EOF
chmod +x "${APP_DIR}/AppRun"

# Download appimagetool if not present
if [ ! -f "appimagetool" ]; then
    wget -O appimagetool \
        "https://github.com/AppImage/AppImageKit/releases/download/continuous/appimagetool-x86_64.AppImage"
    chmod +x appimagetool
fi

# Create AppImage
./appimagetool "${APP_DIR}" "dist/RustyNES-x86_64.AppImage"

echo "Build complete: dist/RustyNES-x86_64.AppImage"
```

#### Flatpak Manifest

```yaml
# com.rustynes.RustyNES.yaml
app-id: com.rustynes.RustyNES
runtime: org.freedesktop.Platform
runtime-version: '23.08'
sdk: org.freedesktop.Sdk
sdk-extensions:
  - org.freedesktop.Sdk.Extension.rust-stable
command: rustynes
finish-args:
  - --share=ipc
  - --socket=x11
  - --socket=wayland
  - --socket=pulseaudio
  - --device=all  # For gamepads
  - --filesystem=home:ro  # Read ROMs from home
modules:
  - name: rustynes
    buildsystem: simple
    build-options:
      append-path: /usr/lib/sdk/rust-stable/bin
      env:
        CARGO_HOME: /run/build/rustynes/cargo
    build-commands:
      - cargo --offline build --release -p rustynes-desktop
      - install -Dm755 target/release/rustynes-desktop /app/bin/rustynes
    sources:
      - type: dir
        path: .
      - cargo-sources.json
```

#### Debian Package

```bash
#!/bin/bash
# build-deb.sh

set -e

PKG_NAME="rustynes"
PKG_VERSION="0.1.0"
PKG_ARCH="amd64"
PKG_DIR="dist/${PKG_NAME}_${PKG_VERSION}_${PKG_ARCH}"

# Build release
cargo build --release -p rustynes-desktop

# Create package structure
mkdir -p "${PKG_DIR}/DEBIAN"
mkdir -p "${PKG_DIR}/usr/bin"
mkdir -p "${PKG_DIR}/usr/share/applications"
mkdir -p "${PKG_DIR}/usr/share/icons/hicolor/256x256/apps"

# Copy binary
cp target/release/rustynes-desktop "${PKG_DIR}/usr/bin/rustynes"

# Create control file
cat > "${PKG_DIR}/DEBIAN/control" << EOF
Package: ${PKG_NAME}
Version: ${PKG_VERSION}
Section: games
Priority: optional
Architecture: ${PKG_ARCH}
Depends: libc6, libgtk-3-0, libasound2, libpulse0
Maintainer: RustyNES Team <team@rustynes.org>
Description: High-accuracy NES emulator
 RustyNES is a cycle-accurate Nintendo Entertainment System
 emulator written in Rust, targeting 100% TASVideos accuracy.
EOF

# Create desktop file
cat > "${PKG_DIR}/usr/share/applications/rustynes.desktop" << 'EOF'
[Desktop Entry]
Name=RustyNES
Comment=NES Emulator
Exec=rustynes %f
Icon=rustynes
Terminal=false
Type=Application
Categories=Game;Emulator;
MimeType=application/x-nes-rom;
EOF

# Copy icon
cp assets/icon.png "${PKG_DIR}/usr/share/icons/hicolor/256x256/apps/rustynes.png"

# Build package
dpkg-deb --build "${PKG_DIR}"

echo "Build complete: ${PKG_DIR}.deb"
```

## Running

### Basic Usage

```bash
# Run with GUI (file picker)
cargo run -p rustynes-desktop --release

# Run with ROM file
cargo run -p rustynes-desktop --release -- game.nes

# Run with options
cargo run -p rustynes-desktop --release -- \
    --scale 3 \
    --filter crt \
    --audio-backend pulseaudio \
    game.nes
```

### Command Line Options

```
USAGE:
    rustynes-desktop [OPTIONS] [ROM]

ARGS:
    <ROM>    Path to NES ROM file (.nes)

OPTIONS:
    -s, --scale <SCALE>          Window scale factor (1-6) [default: 2]
    -f, --fullscreen             Start in fullscreen mode
    --filter <FILTER>            Video filter [possible values: none, crt, hq2x, xbrz]
    --audio-backend <BACKEND>    Audio backend [possible values: auto, alsa, pulseaudio, wasapi, coreaudio]
    --audio-latency <MS>         Audio latency in milliseconds [default: 50]
    --no-vsync                   Disable vertical sync
    --speed <SPEED>              Emulation speed multiplier [default: 1.0]
    --region <REGION>            Force region [possible values: auto, ntsc, pal, dendy]
    --controller-db <PATH>       Path to gamecontrollerdb.txt
    --lua-script <PATH>          Run Lua script on startup
    --connect <ADDRESS>          Connect to netplay session
    --host <PORT>                Host netplay session
    -h, --help                   Print help information
    -V, --version                Print version information
```

### Environment Variables

```bash
# Force specific audio backend
export RUSTYNES_AUDIO_BACKEND=pulseaudio

# Set save directory
export RUSTYNES_SAVE_DIR=~/.local/share/rustynes/saves

# Enable debug logging
export RUST_LOG=rustynes=debug

# Disable GPU acceleration (force software rendering)
export RUSTYNES_SOFTWARE_RENDER=1
```

## Troubleshooting

### Common Issues

#### Audio Not Working (Linux)

```bash
# Check PulseAudio is running
pulseaudio --check

# Or with PipeWire
pactl info

# Verify ALSA devices
aplay -l
```

#### No Gamepad Detected

```bash
# Linux: Check udev rules
ls -la /dev/input/js*

# Install gamepad udev rules if needed
sudo cp /path/to/50-gamepad.rules /etc/udev/rules.d/
sudo udevadm control --reload-rules
```

#### High CPU Usage

```bash
# Enable VSync
rustynes-desktop --vsync game.nes

# Or lower framerate cap
rustynes-desktop --fps-limit 60 game.nes
```

#### Blurry Graphics

```bash
# Use integer scaling
rustynes-desktop --integer-scale game.nes

# Or specific scale factor
rustynes-desktop --scale 3 game.nes
```

### Debug Build

```bash
# Build with debug symbols
cargo build -p rustynes-desktop

# Run with logging
RUST_LOG=debug cargo run -p rustynes-desktop -- game.nes

# Run with backtrace on panic
RUST_BACKTRACE=1 cargo run -p rustynes-desktop -- game.nes
```

## Performance Optimization

### Compiler Flags

```bash
# Native CPU optimization
RUSTFLAGS="-C target-cpu=native" cargo build --release

# Maximum optimization (slower compile)
RUSTFLAGS="-C opt-level=3 -C lto=fat -C codegen-units=1" cargo build --release
```

### Profile-Guided Optimization

```bash
# Build with instrumentation
RUSTFLAGS="-Cprofile-generate=/tmp/pgo-data" cargo build --release -p rustynes-desktop

# Run workload to collect data
./target/release/rustynes-desktop benchmark_rom.nes

# Build with profile data
RUSTFLAGS="-Cprofile-use=/tmp/pgo-data" cargo build --release -p rustynes-desktop
```

## Development

### Running Tests

```bash
# All tests
cargo test --workspace

# Desktop-specific tests
cargo test -p rustynes-desktop

# With output
cargo test -p rustynes-desktop -- --nocapture
```

### Benchmarks

```bash
# Run benchmarks
cargo bench -p rustynes-core

# Specific benchmark
cargo bench -p rustynes-core -- cpu_
```

### Documentation

```bash
# Generate and open docs
cargo doc --workspace --no-deps --open
```

## Distribution Checklist

- [ ] Build release binaries for all target platforms
- [ ] Test on clean systems without development tools
- [ ] Verify all features work correctly
- [ ] Check license compliance for dependencies
- [ ] Create installer/package for each platform
- [ ] Sign binaries (Windows/macOS)
- [ ] Test auto-update mechanism if applicable
- [ ] Prepare release notes
- [ ] Upload to distribution channels
