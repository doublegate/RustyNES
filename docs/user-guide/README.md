# RustyNES — User Guide

End-user documentation for running NES games with the `rustynes` binary.

If you've used Mesen, Nestopia, or FCEUX before, the model here will be
familiar: one ROM per window, cycle-accurate emulation, a menu bar and
status bar framing the picture, F-key save states, hold-to-rewind, and a
`~` debugger overlay. Settings live in a TOML file under your OS's standard
config directory (and most of them are also reachable from View → Settings…).

If you're a developer or contributor looking for implementation specs (CPU,
PPU, APU, scheduler, testing strategy, etc.), see the rest of [`../`](../)
instead. This subdirectory only covers running the emulator.

## Table of contents

| Page | What it covers |
|------|----------------|
| [Getting started](./getting-started.md) | Install, system requirements, first launch, loading a ROM |
| [Controls](./controls.md) | Default keyboard layout + how to rebind keys |
| [Menu reference](./menus.md) | What every menu-bar / status-bar / Settings entry does |
| [Configuration](./configuration.md) | The `config.toml` schema, every key documented with defaults |
| [Save states and rewind](./save-states-and-rewind.md) | F1 / F4 / F5, the 10 slots per ROM, where files live |
| [Debugger](./debugger.md) | The `~` overlay tour: CPU, PPU, OAM, APU, memory, mapper, input panels, fps counter |
| [Display and audio](./display-and-audio.md) | NTSC filter, aspect ratio, audio sample rate, region detection |
| [Compatibility](./compatibility.md) | Supported mappers, known accuracy gaps, ROM-format support |
| [Troubleshooting](./troubleshooting.md) | FAQ: no audio, wrong fps, black screen, save state errors |
| [File locations](./file-locations.md) | Per-OS paths for config, saves, rewind state |

## Quick reference (cheatsheet)

| Key             | Action |
|-----------------|--------|
| Arrow keys      | D-pad |
| Z / X           | A / B |
| Enter           | Start |
| Right Shift     | Select |
| Space           | Pause / Resume |
| F1 / F4         | Save state / Load state (active slot) |
| F2 / F3         | Reset / Power cycle |
| F5 (hold)       | Rewind |
| F6 / F7 / F8    | TAS movie record / play / branch |
| F9 / F10        | Swap FDS disk side / Insert Vs. coin |
| F11             | Fullscreen |
| F12             | Open ROM |
| M               | Toggle the menu bar |
| `~` (Backquote) | Toggle the debugger overlay |
| Esc             | Quit (or exit fullscreen) |

All keys are rebindable. See [Controls](./controls.md) and the
[Menu reference](./menus.md).

## A note on ROM legality

RustyNES does not ship commercial Nintendo ROMs and will not. Use only
ROMs you have legally obtained — your own cartridge dumps, or homebrew /
public-domain ROMs from sources such as the [NESdev wiki test ROM
collection](https://www.nesdev.org/wiki/Emulator_tests).

If you don't have a way to dump your own cartridges, the homebrew scene
has produced a sizeable library of free, original games.
