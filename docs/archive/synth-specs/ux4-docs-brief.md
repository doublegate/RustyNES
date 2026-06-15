# #4 feature facts for the docs update (authoritative — match docs to THIS)

These 11 frontend features just shipped in RustyNES v1.0.0 (`/home/parobek/Code/OSS_Public-Projects/RustyNES`).
Update the user-guide docs to reflect them exactly. No emojis. Read each file before editing.

## New default keybinds ([input.system], all rebindable)

- **Speed Up = `=`** (Equal), **Speed Down = `-`** (Minus), **Speed Reset = `0`** (Digit0).
  (These step through / reset the emulation-speed presets.)
- No new keybind for clipboard-screenshot (menu-only) or the other features.
- (Unchanged existing keys: Space=Pause, Tab=fast-forward hold, \\=frame-advance, F1/F4 save/load,
  F5 rewind, F2/F3 reset/power, F11 fullscreen, M=menu bar, ~=debugger, F12 open, F6/F7/F8 TAS,
  F9 disk swap, F10 insert coin.)

## The features

1. **Master volume** — Settings -> Audio tab: a Volume slider (0–100%) + a Mute checkbox.
   Config: `[audio] volume` (float 0.0–1.0, default 1.0) + `[audio] muted` (bool, default false).
2. **Per-APU-channel mute** — Settings -> Audio tab: six checkboxes (Pulse 1, Pulse 2, Triangle,
   Noise, DMC, Mapper Audio), all on by default. Config: `[audio] channel_mask` (default all-on).
   A studio/debug mute; does not affect accuracy (a playback overlay).
3. **Emulation-speed presets** — Emulation -> Speed submenu: 25% / 50% / 75% / 100% / 150% / 200% /
   300% (current one checkmarked). Keys `=` / `-` / `0` (up/down/reset). The speed is transient
   (always launches at 100%, not persisted). The status bar shows the speed when it is not 100%.
   Audio pitch-shifts naturally at non-100% speeds (slow-mo at 50%, faster/higher at 200%); separate
   from the hold-Tab fast-forward (which mutes).
4. **Save-state thumbnails** — File -> Save States… opens a Save-States manager window: a grid of the
   save slots, each showing a thumbnail of the saved frame + the save timestamp (or "Empty"), with
   Save / Load per slot; the active slot is highlighted. Native-only.
5. **Overscan crop** — View -> Hide Overscan (and a Video settings-tab toggle): crops the top and
   bottom 8 NES scanlines (the CRT-hidden overscan). Config: `[graphics] hide_overscan` (bool,
   default false). Off by default = the full 256x240 image as before.
6. **Pause-screen dimming** — when paused, the emulated viewport dims (~40% black) with a large
   centered "PAUSED" label (the menu/status bars stay normal).
7. **Reset to Defaults** — each Settings section (Video / Audio / Advanced) has a Reset-to-Defaults
   button (two-click confirm) restoring that section to its defaults and re-applying live.
8. **Controller hot-plug toast** — a status message ("Controller connected/disconnected") when a
   gamepad is plugged or unplugged.
9. **Deadzone slider** — Settings -> Input tab: an analog-stick deadzone slider (0.05–0.95) for the
   gamepads (exposes the existing `axis_deadzone` config, applied live).
10. **Screenshot to clipboard** — File -> Copy Screenshot to Clipboard: copies the current frame to
    the system clipboard (in addition to the existing Take Screenshot -> PNG file). Native-only.
11. **Live FPS / frame-time graph** — the Performance panel now includes a rolling frame-time
    sparkline (presented vs produced, with a 16.64 ms NTSC-deadline reference line), beyond the
    existing numeric readout.

## Settings window tabs (current): Display / Audio / Input / Advanced
