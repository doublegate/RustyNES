# Controls

All keyboard and gamepad input is configurable. This page documents the
defaults and the two ways to change them.

## Default bindings

### Player 1

| NES button | Default key |
|------------|-------------|
| D-pad Up | `ArrowUp` |
| D-pad Down | `ArrowDown` |
| D-pad Left | `ArrowLeft` |
| D-pad Right | `ArrowRight` |
| A | `Z` |
| B | `X` |
| Select | `Right Shift` |
| Start | `Enter` |

### Player 2

| NES button | Default key |
|------------|-------------|
| D-pad Up | `W` |
| D-pad Down | `S` |
| D-pad Left | `A` |
| D-pad Right | `D` |
| A | `Q` |
| B | `E` |
| Select | `L` |
| Start | `P` |

Player 2 input is fully wired — these keys drive the second controller
(`$4017`). A second USB gamepad can also drive Player 2 (see
[Gamepads](#gamepads)).

### Players 3 & 4 (Four Score)

The Four Score 4-player adapter is **off by default**. Toggle it with the
**"Four Score (4-player)"** checkbox in the in-app input modal (open the
debugger with `~`, tick **Input**). While off, the emulator is byte-for-byte a
standard two-controller NES; while on, Players 3 and 4 are multiplexed onto
`$4016`/`$4017` for the handful of games that support it. Default keys:

| NES button | Player 3 | Player 4 |
|------------|----------|----------|
| D-pad Up    | `I` | `Numpad8` |
| D-pad Down  | `K` | `Numpad2` |
| D-pad Left  | `J` | `Numpad4` |
| D-pad Right | `L` | `Numpad6` |
| A      | `U` | `Numpad7` |
| B      | `O` | `Numpad9` |
| Select | `M` | `Numpad1` |
| Start  | `.` (Period) | `Numpad3` |

All four players are rebindable (keyboard and gamepad) in the same input
modal, and a third/fourth USB gamepad auto-binds to Players 3/4 — most
4-player setups use gamepads, so the keyboard defaults above are a fallback.

### System keys

| Action | Default key | Notes |
|--------|-------------|-------|
| Quit | `Esc` | Closes the window cleanly |
| Save state | `F1` | Writes to slot 0 for the current ROM |
| Load state | `F4` | Reads from slot 0 |
| Rewind | `F5` | Held — step back one frame per held tick |
| Reset | `F2` | Warm reset (same as the cartridge's RESET button) |
| Power cycle | `F3` | Cold boot (clears RAM, re-runs init) |
| Movie record | `F6` | Toggle TAS movie recording (start = power-on; stop = save `.rnm`) |
| Movie play | `F7` | Toggle TAS movie playback (start = open `.rnm`; stop = live input) |
| Movie branch | `F8` | Branch the current run into a new recording at this frame |
| Open ROM | `F12` | Open the file picker to load a different `.nes` ROM |
| Debugger overlay | `` ` `` (Backquote, the `~` key) | Toggles the egui overlay |

Esc cancels the in-app rebind capture too — if you click "rebind" in the
debugger and want to back out, press Esc instead of any other key.

## Two ways to rebind

### 1. In the running emulator (recommended)

1. Press `~` to open the debugger overlay.
2. Tick the **Input** checkbox in the top toolbar.
3. The "Input bindings" window lists every action — Player 1, Player 2,
   gamepad (per player), and system keys — with its current binding.
4. Click **rebind** next to the action you want to change.
5. The next key (or, for a gamepad row, the next pad button) you press
   becomes the new binding. Press `Esc` to cancel.
6. Click **Save to disk** to persist the change.

"Reset to defaults" reverts every binding to the table above. The change
is in-memory only until you also click "Save to disk".

### 2. Editing `config.toml` by hand

The config file lives at:

| OS | Path |
|----|------|
| Linux | `$XDG_CONFIG_HOME/RustyNES/config.toml` (or `~/.config/RustyNES/config.toml`) |
| macOS | `~/Library/Application Support/dev.DoubleGate.RustyNES/config.toml` |
| Windows | `%APPDATA%\DoubleGate\RustyNES\config\config.toml` |

A minimal example overriding only Player 1's A and B:

```toml
[input.player1]
a = "KeyA"
b = "KeyS"
```

All other fields fall back to the defaults documented above. A missing
file is fine — the emulator creates one with defaults the first time you
save settings from the in-app modal.

## Key names

Key names follow the `winit::keyboard::KeyCode` enum:

| Class | Examples |
|-------|----------|
| Letters | `KeyA`, `KeyB`, ... `KeyZ` |
| Digits (top row) | `Digit0` ... `Digit9` |
| Function keys | `F1` ... `F12` |
| Arrows | `ArrowUp`, `ArrowDown`, `ArrowLeft`, `ArrowRight` |
| Modifiers | `ShiftLeft`, `ShiftRight`, `ControlLeft`, `ControlRight`, `AltLeft`, `AltRight`, `SuperLeft`, `SuperRight` |
| Whitespace + punctuation | `Space`, `Enter`, `Tab`, `Backspace`, `Escape`, `Comma`, `Period`, `Slash`, `Backslash`, `Semicolon`, `Quote`, `Backquote`, `Minus`, `Equal`, `BracketLeft`, `BracketRight` |
| Numpad | `Numpad0` ... `Numpad9` |

Key names are case-sensitive and use the physical key layout (so `KeyA`
is the position of the `A` key on a US QWERTY keyboard regardless of
your OS-level keymap). A typo in `config.toml` logs a warning to stderr
and the binding is silently dropped — the rest of the file still loads.

## Held versus tap

- **D-pad and A/B/Select/Start** are held: held keys produce held NES
  buttons; releasing the key clears the bit.
- **Save / Load state, Reset, Power cycle, Debugger toggle** fire on
  key-down only.
- **Rewind** is held: while `F5` is held, the emulator walks backwards
  through the rewind ring one frame per redraw. Releasing resumes
  forward play.
- **Quit (Esc)** fires on key-down.

## TAS movies (record / playback)

RustyNES can record a *movie* — the per-frame controller input applied on
top of a reproducible start point — and replay it bit-for-bit. Because the
core is deterministic, a replayed movie re-derives every pixel and audio
sample exactly.

- **Record** (`F6`): press once to start. Recording power-cycles the
  console so the movie begins from a fresh boot (the most portable start
  point). Press `F6` again to stop; a file picker prompts for a `.rnm`
  save path (defaults to the `movies/` folder under your data directory —
  see [File locations](./file-locations.md)).
- **Play** (`F7`): press once to open a `.rnm` file. The console seeks to
  the movie's start point and replays it; the movie's input **overrides**
  your live keyboard and gamepad input until the movie ends (control then
  returns to live input automatically) or you press `F7` again to stop.
- **Branch** (`F8`): while watching a replay (or at any point during live
  play), press `F8` to begin recording a *new* movie from the current
  state. This embeds a save-state start point so you can diverge from a
  run and record your own continuation. Stop with `F6` to save the branch.

When the debugger overlay (`` ` ``) is open, the top toolbar shows a
read-only status indicator: **REC N frames** while recording, or **PLAY
n/N** during playback. The overlay never affects the recording — it is
purely an observer, so a movie recorded with the overlay open replays
identically with it closed.

A `.rnm` movie is tied to the ROM it was recorded against (by SHA-256);
loading a movie while a different ROM is running reports a mismatch and
declines to play.

> Web build note: TAS movie record/playback is native-only in this
> release. The browser build compiles the same machinery but the hotkeys
> are inert there pending browser file-download / storage support.

## Gamepads

USB / Bluetooth gamepads are supported via `gilrs`. The first pad your OS
reports drives Player 1; a second distinct pad drives Player 2. The default
layout is Xbox-style:

| NES button | Default pad input |
|------------|-------------------|
| A | South (A / cross) |
| B | West (X / square) |
| Start | Start |
| Select | Back / View / Select |
| D-pad | D-pad |
| D-pad (alt) | Left analog stick, past the deadzone |

Rebind pad buttons in the same in-app modal as the keyboard (open the
debugger with `~`, tick **Input**): each player has a row of gamepad
bindings — click **rebind** and press the pad button you want. The left
analog stick doubles as a D-pad once it deflects past `axis_deadzone`
(default 0.5). Bindings persist to the `[input.gamepad1]` /
`[input.gamepad2]` sections of `config.toml` (see
[Configuration](./configuration.md)). Gamepads are native-only — the
browser build is keyboard-only.

## See also

- [Configuration](./configuration.md) — the full `config.toml` reference
- [Save states and rewind](./save-states-and-rewind.md) — what F1 / F4 / F5 do
- [Debugger](./debugger.md) — the `~` overlay and the rebind panel
