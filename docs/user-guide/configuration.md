# Configuration

RustyNES reads a single TOML file on startup. Missing keys fall back
to documented defaults; a missing file is treated as "all defaults". A
parse error logs a warning to stderr and the in-process config falls
back to defaults — your save data is unaffected.

## Where the file lives

See [File locations](./file-locations.md) for the per-OS path. On Linux
it's typically `~/.config/RustyNES/config.toml`.

The file is **read** at startup and **written** whenever you change a UI
setting (theme, 8:7 pixel aspect, FPS readout, run-ahead), open a ROM
(updating the recent list), dismiss the first-run Welcome modal, or click
"Save to disk" in the rebind panel (see [Controls](./controls.md)). You can
also edit it by hand.

## Full schema with defaults

The complete `config.toml`, with every key set to its default:

```toml
[input.player1]
up = "ArrowUp"
down = "ArrowDown"
left = "ArrowLeft"
right = "ArrowRight"
a = "KeyZ"
b = "KeyX"
select = "ShiftRight"
start = "Enter"

[input.player2]
up = "KeyW"
down = "KeyS"
left = "KeyA"
right = "KeyD"
a = "KeyQ"
b = "KeyE"
select = "KeyL"
start = "KeyP"

[input.gamepad1]
up = "DPadUp"
down = "DPadDown"
left = "DPadLeft"
right = "DPadRight"
a = "South"
b = "West"
select = "Select"
start = "Start"
axis_deadzone = 0.5

[input.gamepad2]
up = "DPadUp"
down = "DPadDown"
left = "DPadLeft"
right = "DPadRight"
a = "South"
b = "West"
select = "Select"
start = "Start"
axis_deadzone = 0.5

[input.system]
quit = "Escape"
save_state = "F1"
load_state = "F4"
rewind = "F5"
reset = "F2"
power_cycle = "F3"
debug_overlay = "Backquote"
open_rom = "F12"
movie_record = "F6"
movie_play = "F7"
movie_branch = "F8"
disk_swap = "F9"
insert_coin = "F10"
fullscreen = "F11"
toggle_menu_bar = "KeyM"
fast_forward = "Tab"
frame_advance = "Backslash"

[rewind]
enabled = true
max_seconds = 60
keyframe_period = 60

[graphics]
present_mode = "Mailbox"
ntsc_filter = "off"

[audio]
sample_rate = 44100

[ui]
theme = "dark"                    # "light" | "dark" | "system"
pixel_aspect_correction = false   # 8:7 NES-native pixel aspect
show_fps = true                   # FPS readout in the status bar
pause_on_focus_loss = false       # auto-pause when the window loses focus

[recent_roms]
paths = []        # most-recently-opened ROM paths, newest first
max_entries = 10  # how many entries the File -> Recent list keeps

welcome_shown = false  # set to true after the first-run Welcome modal is dismissed
```

Every section is independently `#[serde(default)]`-d, so a file
containing only `[graphics]` and `ntsc_filter = "composite"` is perfectly
valid — the rest fills in from defaults.

## Section reference

### `[input.player1]` and `[input.player2]`

Eight string-typed fields each: `up`, `down`, `left`, `right`, `a`, `b`,
`select`, `start`. Each value is a `winit::keyboard::KeyCode` name. See
[Controls](./controls.md) for the table of accepted names.

Player 2 input is fully wired to the emulator core (the second controller
on `$4017`).

### `[input.gamepad1]` and `[input.gamepad2]`

Eight string-typed fields each (`up`, `down`, `left`, `right`, `a`, `b`,
`select`, `start`), where each value is a `gilrs::Button` name (e.g.
`"South"`, `"West"`, `"DPadUp"`, `"Start"`, `"Select"`), plus a numeric
`axis_deadzone` (0.0..=1.0, default `0.5`) controlling how far the left
analog stick must deflect before it counts as a D-pad press. The default
is the Xbox-style layout in [Controls](./controls.md#gamepads). The first
physical pad your OS reports drives Player 1; a second distinct pad drives
Player 2. Gamepads are native-only (no `gilrs` on the web build). Both
sections are `#[serde(default)]`, so a pre-v1.6.0 config without them loads
unchanged with the default layout.

### Players 3 & 4 + Four Score (v1.7.0)

`[input.player3]` / `[input.player4]` (keyboard) and `[input.gamepad3]` /
`[input.gamepad4]` (gamepad) mirror the Players 1/2 sections above, and a
top-level `four_score` key in the `[input]` table (default `false`) enables the
Four Score 4-player adapter. All are `#[serde(default)]`, so a pre-v1.7.0
config loads unchanged. Defaults + the in-app toggle are described under
[Controls → Players 3 & 4](./controls.md#players-3--4-four-score).

### `[input.system]`

String-typed fields, each a single key bound to one system action. All
fields beyond the original seven are `#[serde(default)]`, so a pre-v1.0.0
config loads unchanged and fills the new binds in:

| Field | Default | Action |
|-------|---------|--------|
| `quit` | `Escape` | Close the window (or leave fullscreen first) |
| `save_state` | `F1` | Save to the active slot of the current ROM |
| `load_state` | `F4` | Load the active slot |
| `rewind` | `F5` | Hold to walk backwards through the rewind ring |
| `reset` | `F2` | Warm reset |
| `power_cycle` | `F3` | Cold boot |
| `debug_overlay` | `Backquote` | Toggle the debugger overlay |
| `open_rom` | `F12` | Open the ROM file picker |
| `movie_record` | `F6` | Toggle TAS movie recording |
| `movie_play` | `F7` | Toggle TAS movie playback |
| `movie_branch` | `F8` | Branch the current run into a new recording |
| `disk_swap` | `F9` | Cycle the FDS disk side (FDS games only) |
| `insert_coin` | `F10` | Insert a Vs. System coin (Vs. games only) |
| `fullscreen` | `F11` | Toggle borderless fullscreen (native only) |
| `toggle_menu_bar` | `KeyM` | Show / hide the menu bar |
| `fast_forward` | `Tab` | Hold to run the emulator unthrottled (audio muted) |
| `frame_advance` | `Backslash` | Press to step one frame (for use while paused) |

### `[rewind]`

```toml
[rewind]
enabled = true        # default
max_seconds = 60      # default — rewind window length in seconds
keyframe_period = 60  # default — frames per LZ4 keyframe (rest are XOR deltas)
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `enabled` | bool | `true` | Set to `false` to disable rewind capture at startup |
| `max_seconds` | u32 | `60` | Upper bound on the rewind window. Memory is also capped at 32 MiB regardless of this value |
| `keyframe_period` | u32 | `60` | How often a full LZ4-compressed keyframe is stored (1/sec at NTSC). Smaller = faster step-back, more memory |

Disabling rewind reclaims the memory the ring would have used and skips
the per-frame snapshot cost. Save states (F1 / F4) work either way; they
are independent of the rewind ring.

### `[graphics]`

```toml
[graphics]
present_mode = "Fifo"   # default
ntsc_filter = "off"     # default
```

| Field | Type | Default | Accepted values | Notes |
|-------|------|---------|-----------------|-------|
| `present_mode` | string | `"Mailbox"` | `"Fifo"`, `"Mailbox"` | Wired since v1.3.1. `"Mailbox"` lets the wall-clock frame pacer own timing (avoids the vsync double-pacing beat); falls back to `"Fifo"` automatically when the backend doesn't advertise Mailbox |
| `ntsc_filter` | string | `"off"` | `"off"`, `"composite"`, `"rgb"` | `"off"` and `"composite"` are the meaningful settings in v1.0 (see below) |

The NTSC filter (when set to anything other than `"off"`) runs a
simplified Blargg-style wgsl post-pass between the PPU framebuffer and
the letterbox blit: a 5-tap horizontal blur, 15% scanline darkening on
alternating lines, and a subtle chroma fringe along strong luma edges.
It's deliberately marked "simple" — a bit-exact `nes_ntsc` port is a
follow-up. See [Display and audio](./display-and-audio.md) for a
side-by-side description.

The `"rgb"` value is accepted by the parser but currently produces the
same output as `"composite"`; a distinct RGB-pixel-shaped path is
planned for a later release.

### `[audio]`

```toml
[audio]
sample_rate = 44100  # default
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `sample_rate` | u32 | `44100` | Preferred sample rate in Hz. The negotiated rate may differ if the audio device refuses 44.1 kHz; the APU is rebuilt at whatever rate the device opens at, so audio still sounds correct |

The APU emits at the negotiated rate directly via band-limited synthesis
(blip_buf-style), so there is no software resampler in the pipeline — no
quality loss from rate conversion.

### `[ui]`

The desktop UX shell settings, surfaced under **View → Settings… → Video**
(and the View menu). All three apply live.

```toml
[ui]
theme = "dark"
pixel_aspect_correction = false
show_fps = true
pause_on_focus_loss = false
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `theme` | string | `"dark"` | `"light"`, `"dark"`, or `"system"` (follow the OS theme, falling back to dark) |
| `pixel_aspect_correction` | bool | `false` | Apply 8:7 NES-native pixel-aspect correction. Off by default so the shipped image stays pixel-exact |
| `show_fps` | bool | `true` | Show the FPS readout in the status bar |
| `pause_on_focus_loss` | bool | `false` | Auto-pause emulation when the window loses focus, auto-resume on regaining it. Never overrides a manual pause and never auto-pauses during a netplay session |

### `[recent_roms]`

The File → Open Recent MRU list. Managed by the emulator; rarely hand-edited.

```toml
[recent_roms]
paths = []
max_entries = 10
```

| Field | Type | Default | Notes |
|-------|------|---------|-------|
| `paths` | array of strings | `[]` | Most-recently-opened ROM paths, newest first. Missing files are greyed out in the menu |
| `max_entries` | usize | `10` | How many entries the list retains |

### `welcome_shown`

A top-level boolean (default `false`). Set to `true` and saved the first
time the Welcome modal is shown, so it never re-appears. Set it back to
`false` to see the first-run modal again.

## Reload behavior

The config file is read **once** at startup, so hand-edits during a session
take effect on the next launch. Changes you make from inside the running
emulator are written back immediately and applied without a restart: the
rebind panel ("Save to disk"), the `[ui]` toggles (theme, 8:7, FPS), the
run-ahead picker, and the recent-ROMs list all persist as you change them.

## Backing up your config

The file is plain TOML and safe to copy to another machine. Save data
(slot files in `<data_dir>/saves/<rom_sha256>/slot*.rns`) is a separate
directory; the config has no references to save files, so you can move
each independently. Game Genie cheats are likewise per-ROM, in
`<data_dir>/cheats/<rom_sha256>.toml`.

See [File locations](./file-locations.md) for everywhere the emulator
writes.

## See also

- [Controls](./controls.md) — full key-name table + the in-app rebind flow
- [Menu reference](./menus.md) — the menus and Settings window behind these keys
- [File locations](./file-locations.md) — paths per OS
- [Save states and rewind](./save-states-and-rewind.md) — how `[rewind]` is consumed
