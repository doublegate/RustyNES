# Getting started

This page walks through downloading or building the `rustynes` binary
and loading your first ROM.

## System requirements

| Requirement | Detail |
|-------------|--------|
| OS | Linux, macOS, or Windows (64-bit) |
| CPU | Any x86_64 or aarch64 (Apple Silicon) made since ~2015 |
| GPU | Any GPU with Vulkan, Metal, D3D12, or OpenGL ES 3 support (basically anything from the last decade) |
| RAM | 256 MiB free; the rewind ring uses up to ~32 MiB by default |
| Audio | Any output device supported by the OS — PulseAudio / PipeWire / ALSA / CoreAudio / WASAPI all work |

The emulator is built on `winit`, `wgpu`, and `cpal`, so it inherits their
platform reach. On Linux both X11 and Wayland are supported through the
same binary.

## Installing

### Option 1: download a release

Pre-built binaries are published on the [GitHub Releases
page](https://github.com/doublegate/RustyNES/releases) for:

- `x86_64-unknown-linux-gnu`
- `aarch64-apple-darwin` (Apple Silicon native)
- `x86_64-pc-windows-msvc`

Each archive contains `rustynes` (or `rustynes.exe`), the licenses,
the changelog, and the README. Extract anywhere; the binary is a single
file with no install step.

### Option 2: build from source

Building from source requires Rust 1.96 (pinned via `rust-toolchain.toml`)
and a working system-library set for the windowing / audio stack.

**Linux build deps (Debian / Ubuntu):**

```bash
sudo apt-get install -y \
    libxkbcommon-dev libwayland-dev libxkbcommon-x11-dev \
    libasound2-dev libudev-dev
```

**Linux build deps (Arch / CachyOS):**

```bash
sudo pacman -S --needed libxkbcommon wayland alsa-lib systemd-libs
```

**macOS and Windows:** no extra setup beyond installing
[rustup](https://rustup.rs/).

**Build:**

```bash
git clone https://github.com/doublegate/RustyNES.git
cd RustyNES
cargo build --release -p rustynes-frontend
# Binary is at target/release/rustynes
```

## First launch

You can pass a ROM on the command line, or launch the binary bare and
load one from the menu:

```bash
rustynes path/to/game.nes   # open a ROM directly
rustynes                    # launch empty, then use File -> Open ROM (F12)
```

Once running, load a ROM any of three ways: **File → Open ROM…** (`F12`)
for a native file picker, **File → Open Recent** for a previously-opened
ROM, or simply **drag and drop** a `.nes` / `.fds` file onto the window.
On a brand-new install a one-time Welcome modal greets you with a
quick-start shortcut list.

On first launch the emulator:

1. Reads the iNES / NES 2.0 header to identify the mapper and region.
2. Allocates a window sized to 3x the NES native 256x240 resolution
   (so 768x720 by default).
3. Opens the system's default audio device at 44.1 kHz, falling back to
   whatever the device advertises if 44.1 kHz isn't supported.
4. Detects the cartridge region (NTSC, PAL, or Dendy) and paces the
   emulator at the matching real-hardware frame rate (60.0988 Hz for
   NTSC; 50.0070 Hz for PAL/Dendy).
5. Creates a config file with defaults at the standard location for
   your OS the first time you change a setting. See
   [File locations](./file-locations.md) for the exact paths.

## What you should see

A window opens with a **menu bar** along the top and a **status bar**
along the bottom framing the NES image; the game boots and sound starts
immediately. The emulator is paced by wall-clock time, so the game runs at
the correct speed even on high-refresh monitors (e.g. 144 Hz / 240 Hz)
without speeding up.

From here you can:

- open **View → Settings…** for the tabbed Display / Audio / Input / Advanced
  dialog (theme, 8:7 pixel aspect, NTSC filter, sample rate, rebinding),
- press `F11` (or **View → Fullscreen**) to go borderless fullscreen, and
  `Esc` to leave it,
- pick a theme under **View → Theme** (Light / Dark / System), and
- hide the menu bar with `M` if you want a clean view (press `M` again to
  bring it back).

If something doesn't work — silent audio, a black screen, wrong colors —
jump to [Troubleshooting](./troubleshooting.md).

## What's next

- [Controls](./controls.md) — change keys, set up two-player
- [Menu reference](./menus.md) — what every menu does
- [Save states and rewind](./save-states-and-rewind.md) — F1 / F4 / F5
- [Debugger](./debugger.md) — press `~` once a game is running
- [Configuration](./configuration.md) — full `config.toml` reference
