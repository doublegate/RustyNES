# Troubleshooting

Pragmatic answers to common problems. The egui debugger overlay (toggle
with `~`) is your friend here — the top toolbar's `fps:` readout alone
diagnoses about half of the issues on this page.

## "The ROM won't load — `UnsupportedMapper(N)`"

The cartridge uses an iNES mapper number that v1.0 doesn't implement.
See [Compatibility](./compatibility.md) for the list of 14 supported
mapper numbers.

If your ROM is a homebrew title that uses a less common mapper, please
file an issue with the mapper number, the ROM's SHA-256, and (if
applicable) a public link to the homebrew project. Adding a mapper is
tractable work; we triage by popularity / cultural impact.

## "ROM file not found"

The path on the command line doesn't resolve to an existing file. Things
to check:

- Did you `cd` to a different directory before launching?
- Does the file actually have a `.nes` extension (not `.nes.txt` from a
  bad download)?
- On Windows, is the path quoted if it contains spaces? (`rustynes
  "C:\My ROMs\game.nes"`)

The command is case-sensitive on Linux and macOS, case-insensitive on
Windows.

## "Black screen on launch"

The window opens but stays black. Likely causes:

1. **The ROM is corrupt or truncated.** Re-dump it. A 24,592-byte file
   is iNES header (16) + 16 KiB PRG, which is too small for almost any
   real game — a successful dump is typically tens to hundreds of KiB.
2. **The game expects a specific region.** A PAL game running at NTSC
   timing may run too fast and trip an early-init check that aborts to
   a black screen. Re-header the ROM with the correct NES 2.0 region
   byte.
3. **Your GPU driver can't open a wgpu surface.** Check stderr for a
   wgpu error message. Updating GPU drivers usually fixes it.

If the debugger overlay opens (press `~`) but only shows panels — no
game graphics under them — the renderer is alive but the emulator
isn't producing frames. Check the CPU panel: if PC isn't advancing, the
CPU is jammed. The flag row will show `JAMMED` in red.

## "No audio"

Symptom: the game runs visually but no sound plays. Stderr usually has
a `rustynes: audio disabled: <reason>` line at startup.

Common causes by OS:

| OS | Likely reason | Fix |
|----|---------------|-----|
| Linux | ALSA can't find a default output, or the PulseAudio / PipeWire compatibility layer isn't installed | `pactl list short sinks` to confirm a default sink exists; install `pipewire-alsa` or `pulseaudio-alsa` |
| macOS | Output device permissions or a different default device | System Settings -> Sound -> Output |
| Windows | Exclusive-mode lock on the device | Close other audio apps; in Sound settings disable "Allow apps to take exclusive control" for the default device |

If audio init succeeded but you still hear nothing, the device may be
muted at the OS level, or your default output may be a virtual sink
that's not connected to anything.

## "Audio crackles or stutters"

The bounded sample queue drops oldest samples when the emulator
out-produces the device — this is audible as crackles.

Common causes:

- **The emulator is briefly slow** — usually because the host system is
  under load. Check the debugger overlay's `fps:` readout: drops below
  the target (60.1 NTSC / 50.0 PAL) cause crackles.
- **The device's sample rate doesn't match the configured rate.** Try
  setting `[audio] sample_rate` to match your device's native rate
  (often 48000 on Linux/PipeWire, 44100 or 48000 on Windows).
- **Heavy use of the rewind / save-state load** — these stall the
  emulator briefly. Expected behavior.

Persistent crackles on a fast machine with the right sample rate are a
bug — please file an issue with your OS, audio backend, and the
device's reported sample rate.

## "The game runs too fast / too slow"

Open the debugger (`~`) and check the `fps:` readout in the top toolbar.

| You see | Likely cause | Fix |
|---------|--------------|-----|
| ~60 fps for an NTSC game | Correct | (Nothing) |
| ~50 fps for a PAL game | Correct | (Nothing) |
| ~60 fps for a PAL game (visually too fast) | iNES 1.0 header has no region byte; defaults to NTSC | Re-header the ROM as NES 2.0 with byte 12 bits 0-1 = 01 (PAL) |
| ~120-300 fps regardless of region | Wall-clock pacing isn't running (would only happen on a bug — please report) | (File an issue) |
| Below the target on a modern machine | System load, GPU driver issue, or audio device hijacking the main thread | Check other process load; try `[graphics] ntsc_filter = "off"`; try `[audio] sample_rate` matching your device |

The previous frontend MVP had a real "runs at 144 / 60 = 2.4x speed on a
high-refresh monitor" bug. That's fixed in the current release —
emulation is paced by wall-clock time, so any monitor refresh rate works.

## "Save state load did nothing / corrupted state"

`F4` loads slot 0 for the current ROM. Failures log to stderr; the
running emulator state is unchanged.

| stderr says | Meaning | Fix |
|-------------|---------|-----|
| `load state failed: save-state I/O at <path>: No such file` | You haven't saved into this slot yet for this ROM (or the SHA-256 changed) | Save first (`F1`), then load |
| `restore failed: ...` (any other variant) | The slot file is corrupt or from an incompatible version | Delete the slot file from `<data_dir>/saves/<rom_sha256>/slot0.rns` and re-save |

Each ROM has its own save directory keyed by SHA-256 — if you re-dumped
the ROM (or downloaded a different copy), the new SHA-256 means a fresh
save directory. The old slot files remain untouched in the original
directory.

See [Save states and rewind](./save-states-and-rewind.md) for the file
layout and [File locations](./file-locations.md) for `<data_dir>` per
OS.

## "Rewind doesn't go back"

Symptom: hold `F5`, nothing happens.

- **Check `config.toml`.** If you have `[rewind] enabled = false`, the
  ring isn't capturing and `F5` is a no-op.
- **You may be at the start of the ring.** The ring fills as the
  emulator runs; if you press `F5` immediately on launch, there's
  nothing to rewind to. Play for a few seconds first.
- **The rewind key may be remapped.** Check `[input.system] rewind` in
  `config.toml`, or open the **Input** debugger panel.

## "Wrong colors / weird visual artifacts"

| Symptom | Likely cause |
|---------|--------------|
| Whole image is too dark / too bright | The NTSC filter is on (`[graphics] ntsc_filter`) — set to `"off"` for pure-pixel output |
| Faint orange/teal fringe along pixel edges | Same — that's the simplified chroma fringe (deliberate when filter is on) |
| Visible scanlines | Same — 15% darkening on alternating lines (deliberate when filter is on) |
| Wrong palette colors | The NES has 64 colors; ours match Mesen's defaults. If a specific game looks "off" compared to real hardware, this is interesting — please file an issue with a screenshot |
| Sprite flicker that real hardware doesn't have | Likely a real PPU accuracy edge case. The sprite-evaluation pipeline passes the blargg suite but residual scanline-precision gaps exist — see [Compatibility](./compatibility.md) |

## "The window is too small / too big"

You can resize the window freely — drag the edges or maximize. The
emulator preserves the NES aspect ratio with letterboxing or
pillarboxing as needed.

There is no fullscreen toggle yet. Use your window manager's shortcut:

- Linux: `F11` on GNOME / KDE / most desktops
- macOS: green traffic-light button, or `Ctrl+Cmd+F`
- Windows: `Win+Up` to maximize; `F11` works in some configurations

## "I lost my settings"

Likely you (or an installer) wrote a malformed `config.toml`. On the
next launch the emulator falls back to defaults and the malformed file
is left untouched. Either fix the file by hand (it's plain TOML) or
delete it and let the emulator regenerate one when you save settings
from the in-app rebind modal.

See [File locations](./file-locations.md) for the path.

## "I want to share my setup"

The config file is plain TOML, no machine-specific paths inside, so it's
safe to copy. Save data is keyed by ROM SHA-256, so as long as the
recipient has the same dump of the ROM, your save states load on their
machine.

## Filing a bug

When reporting a bug, please include:

- Your OS and CPU architecture.
- The ROM's SHA-256 (run `sha256sum your.nes` on Linux/macOS, or
  `certutil -hashfile your.nes SHA256` on Windows).
- The mapper number (the `Mapper` panel in the debugger shows it).
- A screenshot or short capture if visual.
- Stderr output from the run (run from a terminal, copy/paste anything
  prefixed with `rustynes:`).
- For save state issues: the contents of `<data_dir>/saves/<rom_sha256_hex>/`.

Issues go to <https://github.com/doublegate/RustyNES/issues>.

## See also

- [Compatibility](./compatibility.md) — what's known to work and what isn't
- [Debugger](./debugger.md) — the overlay panels referenced throughout this page
- [File locations](./file-locations.md) — paths to inspect when something looks wrong
