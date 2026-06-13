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
| Save Slot ▸ | | Pick the active slot (0–9) used by `F1` / `F4` |
| Save to Slot ▸ | | Save directly to a chosen slot (0–9) |
| Load from Slot ▸ | | Load directly from a chosen slot (0–9) |
| Save States… | | Open the Save-States manager: a grid of slots, each with a thumbnail of the saved frame and its timestamp (or "Empty"), with per-slot Save / Load; the active slot is highlighted (native only) |
| Take Screenshot | | Write a PNG of the current frame (native only) |
| Copy Screenshot to Clipboard | | Copy the current frame to the system clipboard (native only) |
| Quit | `Esc` | Close the window cleanly |

### Emulation

| Item | Key | Notes |
|------|-----|-------|
| Pause / Resume | `Space` | Toggle emulation; disabled while a netplay session is active |
| Reset | `F2` | Warm reset |
| Power Cycle | `F3` | Cold boot |
| Frame Advance | `\` | Step exactly one frame — meant for use while paused |
| Fast Forward (hold Tab) | `Tab` | Hint only — hold `Tab` to run unthrottled (audio muted); there is no toggle |
| Speed ▸ | `=` / `-` / `0` | Pick an emulation-speed preset — 25% / 50% / 75% / 100% / 150% / 200% / 300% (the current one is checkmarked). `=` steps up, `-` steps down, `0` resets to 100%. The speed is transient (always launches at 100%); the status bar shows it when it is not 100%. Audio pitch-shifts naturally at non-100% speeds — distinct from the muted hold-`Tab` fast-forward |
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
| Settings… | | Open the tabbed Settings window (Display / Audio / Input / Advanced) |
| Theme ▸ | | Light / Dark / System |
| 8:7 Pixel Aspect | | Toggle NES-native pixel-aspect correction (default off) |
| Hide Overscan | | Crop the top and bottom 8 NES scanlines (the CRT-hidden overscan area); default off (`[graphics] hide_overscan`) |
| Fullscreen | `F11` | Toggle borderless fullscreen (native only) |
| Window Size ▸ | | Resize the window to an integer multiple of the NES resolution — 1x (100%) / 2x (200%) / 3x (300%) / 4x (400%) (native only) |
| Show FPS | | Toggle the FPS readout in the status bar |
| Pause When Unfocused | | Auto-pause emulation when the window loses focus, auto-resume when it regains focus (default off; never overrides a manual pause or a netplay session) |
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
- the run state — **Running**, **Paused** (yellow), or **Netplay** (blue),
- the active **emulation speed**, shown only when it is not 100%, and
- the **FPS** readout, right-aligned (hidden when View → Show FPS is off).

When emulation is paused the NES viewport dims (~40% black) with a large
centered **PAUSED** label; the menu bar and status bar stay at normal
brightness.

## Settings window

**View → Settings…** opens a tabbed dialog:

- **Display** — theme, 8:7 pixel-aspect, FPS readout, present mode, the
  NTSC filter, and the Hide Overscan toggle.
- **Audio** — sample rate, audio latency (`latency_ms`), the
  dynamic-rate-control toggle, a master **Volume** slider (0–100%) and a
  **Mute** checkbox, and six per-channel mute checkboxes (Pulse 1, Pulse 2,
  Triangle, Noise, DMC, Mapper Audio), all on by default.
- **Input** — the full rebind panel (the same one the debugger surfaces),
  plus an analog-stick **deadzone** slider (0.05–0.95) for the gamepads,
  applied live.
- **Advanced** — run-ahead depth, rewind sizing, and other developer-facing
  toggles.

The Video / Audio / Advanced sections each have a **Reset to Defaults** button
(two-click confirm) that restores that section's settings to their defaults and
re-applies them live.

Changes to the theme, pixel-aspect, FPS, volume/mute, per-channel mutes,
overscan, and deadzone apply live; the rest note where a restart is needed.

## First-run Welcome

On a brand-new install (no config file yet) a one-time **Welcome** modal appears
with a quick-start shortcut list. Dismiss it with **Get Started**, the close
button, or by clicking away — it never re-appears once shown.

## See also

- [Controls](./controls.md) — the full key list and how to rebind
- [Configuration](./configuration.md) — the `[ui]` config section behind these toggles
- [Debugger](./debugger.md) — the `` ` `` overlay tour
