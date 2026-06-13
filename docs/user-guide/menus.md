# Menu reference

The desktop frontend frames the NES image with an always-on **menu bar**
across the top and a **status bar** along the bottom. The egui debugger is a
separate overlay toggled with `` ` `` — the menu bar and status bar are always
present (unless you hide the menu bar with `M` or View → Show Menu Bar).

Every menu entry shows its keyboard accelerator on the right where one exists;
the hotkey and the menu item do exactly the same thing.

## Menu bar

### File

| Item | Key | Notes |
|------|-----|-------|
| Open ROM… | `F12` | Native file picker for a `.nes` / `.fds` ROM |
| Open Recent ▸ | | The recently-opened ROMs; missing files are greyed out. "Clear Recent" empties the list |
| Swap Disk Side | `F9` | FDS only — cycle the inserted disk side |
| Save State | `F1` | Save to the active slot |
| Load State | `F4` | Load from the active slot |
| Save Slot ▸ | | Pick the active slot (1–8) used by `F1` / `F4` |
| Save to Slot ▸ | | Save directly to a chosen slot (1–8) |
| Load from Slot ▸ | | Load directly from a chosen slot (1–8) |
| Take Screenshot | | Write a PNG of the current frame (native only) |
| Quit | `Esc` | Close the window cleanly |

### Emulation

| Item | Key | Notes |
|------|-----|-------|
| Pause / Resume | | Toggle emulation; disabled while a netplay session is active |
| Reset | `F2` | Warm reset |
| Power Cycle | `F3` | Cold boot |
| Run-Ahead ▸ | | Choose the run-ahead depth, 0–3 frames |
| Region | | Read-only NTSC / PAL / Dendy label |
| Vs. Insert Coin | `F10` | Vs. System games only — insert a coin into acceptor #1 |

### Tools

These open as floating windows directly — you do **not** need the debugger
overlay for them.

| Item | Key | Notes |
|------|-----|-------|
| Cheats… | | Game Genie and raw RAM cheats |
| Movies (TAS) ▸ | `F6` / `F7` / `F8` | Record / Play / Branch a TAS movie |
| Netplay… | | Host or join a rollback session (native only) |
| RetroAchievements… | | Login, achievements, leaderboards (native only, opt-in feature) |
| Performance Monitor | | Frame-timing, audio-queue, and pacing telemetry |

### View

| Item | Key | Notes |
|------|-----|-------|
| Settings… | | Open the tabbed Settings window (Video / Audio / Input / Advanced) |
| Theme ▸ | | Light / Dark / System |
| 8:7 Pixel Aspect | | Toggle NES-native pixel-aspect correction (default off) |
| Fullscreen | `F11` | Toggle borderless fullscreen (native only) |
| Show FPS | | Toggle the FPS readout in the status bar |
| Show Menu Bar | `M` | Hide / show the menu bar itself |

### Debug

| Item | Key | Notes |
|------|-----|-------|
| Show Debugger | `` ` `` | Toggle the egui debugger overlay |
| CPU / PPU / APU / Memory / OAM / Mapper | | Open a specific inspection panel (forces the overlay visible) |

### Help

| Item | Notes |
|------|-------|
| Keyboard Shortcuts | The full default-bindings table |
| About | Version, license, and project link |

## Status bar

The bottom bar shows the current state at a glance:

- the loaded **ROM name** (or "No ROM loaded"),
- the detected **region** (NTSC / PAL / Dendy),
- the **mapper** name,
- the active **run-ahead** depth (when non-zero),
- the run state — **Running**, **Paused** (yellow), or **Netplay** (blue), and
- the **FPS** readout, right-aligned (hidden when View → Show FPS is off).

When emulation is paused a translucent **PAUSED** overlay is drawn over the NES
image.

## Settings window

**View → Settings…** opens a tabbed dialog:

- **Video** — theme, 8:7 pixel-aspect, FPS readout, present mode, and the NTSC
  filter.
- **Audio** — sample rate and audio-latency / dynamic-rate-control options.
- **Input** — the full rebind panel (the same one the debugger surfaces).
- **Advanced** — rewind and other developer-facing toggles.

Changes to the theme, pixel-aspect, and FPS toggles apply live; the rest note
where a restart is needed.

## First-run Welcome

On a brand-new install (no config file yet) a one-time **Welcome** modal appears
with a quick-start shortcut list. Dismiss it with **Get Started**, the close
button, or by clicking away — it never re-appears once shown.

## See also

- [Controls](./controls.md) — the full key list and how to rebind
- [Configuration](./configuration.md) — the `[ui]` config section behind these toggles
- [Debugger](./debugger.md) — the `` ` `` overlay tour
