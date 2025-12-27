# Building RustyNES

**Table of Contents**

- [Prerequisites](#prerequisites)
- [Toolchain Setup](#toolchain-setup)
- [Building](#building)
- [Feature Flags](#feature-flags)
- [Platform-Specific Instructions](#platform-specific-instructions)
- [Cross-Compilation](#cross-compilation)
- [WebAssembly Build](#webassembly-build)
- [Troubleshooting](#troubleshooting)

---

## Prerequisites

### Required

- **Rust** 1.86+ (stable)
- **Cargo** (included with Rust)

### Optional (Platform-Dependent)

- **Linux**: SDL2 development libraries, libasound2-dev
- **macOS**: Xcode Command Line Tools
- **Windows**: Visual Studio 2019+ or MinGW-w64

---

## Toolchain Setup

### Install Rust

```bash
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**Or visit**: <https://rustup.rs>

### Verify Installation

```bash
rustc --version  # Should be 1.86.0 or higher
cargo --version
```

### Update Rust

```bash
rustup update stable
```

---

## Building

### Debug Build (Development)

```bash
cargo build
```

**Output**: `target/debug/rustynes`

**Characteristics**:

- Debug symbols included
- No optimizations
- Fast compilation
- Slower execution (2-5x slower than release)

### Release Build (Production)

```bash
cargo build --release
```

**Output**: `target/release/rustynes`

**Characteristics**:

- Optimized for speed
- No debug symbols (unless configured)
- Slower compilation
- Fast execution

### Run Directly

```bash
cargo run --release -- path/to/rom.nes
```

---

## Feature Flags

RustyNES uses Cargo features for optional functionality:

### Available Features

| Feature | Description | Default |
|---------|-------------|---------|
| `desktop` | Desktop GUI (egui) | Yes |
| `audio` | Audio output (SDL2/cpal) | Yes |
| `debugger` | Built-in debugger | Yes |
| `netplay` | GGPO netplay | No |
| `tas` | TAS recording/playback | No |
| `lua` | Lua scripting (mlua) | No |
| `retroachievements` | RetroAchievements | No |
| `wasm` | WebAssembly support | No |

### Build Examples

**Minimal (headless emulation core)**:

```bash
cargo build --release --no-default-features
```

**With TAS support**:

```bash
cargo build --release --features tas
```

**Full featured**:

```bash
cargo build --release --all-features
```

---

## Platform-Specific Instructions

### Linux

**Install Dependencies**:

**Debian/Ubuntu**:

```bash
sudo apt-get install libsdl2-dev libasound2-dev pkg-config
```

**Fedora**:

```bash
sudo dnf install SDL2-devel alsa-lib-devel
```

**Arch**:

```bash
sudo pacman -S sdl2 alsa-lib
```

**Build**:

```bash
cargo build --release
```

### macOS

**Install Dependencies**:

```bash
brew install sdl2
```

**Build**:

```bash
cargo build --release
```

**Apple Silicon Note**:

```bash
# If SDL2 issues occur on ARM64:
export LIBRARY_PATH="$LIBRARY_PATH:$(brew --prefix)/lib"
cargo build --release
```

### Windows

**Option 1: Visual Studio**

1. Install Visual Studio 2019+ with C++ tools
2. Build:

```powershell
cargo build --release
```

**Option 2: MinGW-w64**

1. Install MSYS2 from <https://www.msys2.org/>
2. Install toolchain:

```bash
pacman -S mingw-w64-x86_64-rust mingw-w64-x86_64-SDL2
```
1. Build:

```bash
cargo build --release
```

---

## Cross-Compilation

### Linux to Windows

**Install cross target**:

```bash
rustup target add x86_64-pc-windows-gnu
sudo apt-get install mingw-w64
```

**Build**:

```bash
cargo build --release --target x86_64-pc-windows-gnu
```

### Linux to macOS

**Using osxcross** (advanced):

```bash
# See: https://github.com/tpoechtrager/osxcross
rustup target add x86_64-apple-darwin
cargo build --release --target x86_64-apple-darwin
```

---

## WebAssembly Build

**Install wasm32 target**:

```bash
rustup target add wasm32-unknown-unknown
cargo install wasm-bindgen-cli
```

**Build WASM**:

```bash
cargo build --release --target wasm32-unknown-unknown --features wasm
wasm-bindgen --out-dir web/pkg --target web target/wasm32-unknown-unknown/release/rustynes.wasm
```

**Serve**:

```bash
cd web
python3 -m http.server 8080
# Visit http://localhost:8080
```

---

## Troubleshooting

### "SDL2 not found"

**Linux**:

```bash
sudo apt-get install libsdl2-dev
```

**macOS**:

```bash
brew install sdl2
export LIBRARY_PATH="$LIBRARY_PATH:$(brew --prefix)/lib"
```

**Windows**: Download SDL2 development libraries from <https://libsdl.org>

### "linker 'cc' not found"

**Install C compiler**:

```bash
# Debian/Ubuntu
sudo apt-get install build-essential

# macOS
xcode-select --install

# Windows
# Install Visual Studio or MinGW-w64
```

### Slow Debug Builds

**Use release mode** for testing:

```bash
cargo run --release
```

### Out of Memory During Compilation

**Reduce parallelism**:

```bash
cargo build --release -j 2
```

---

## References

- [Rust Installation Guide](https://www.rust-lang.org/tools/install)
- [Cargo Book](https://doc.rust-lang.org/cargo/)
- [Cross-Compilation Guide](https://rust-lang.github.io/rustup/cross-compilation.html)

---

**Related Documents**:

- [CONTRIBUTING.md](CONTRIBUTING.md) - Development guidelines
- [TESTING.md](TESTING.md) - Running tests
- [DEBUGGING.md](DEBUGGING.md) - Debugging tools
