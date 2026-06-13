# File locations

RustyNES writes to two places: a **config directory** (one
`config.toml`) and a **data directory** (per-ROM save state slots). The
exact paths come from the `directories` crate, which follows each OS's
standard conventions.

The application identifier used by the resolver is:

| Field | Value |
|-------|-------|
| Qualifier | `dev` |
| Organization | `DoubleGate` |
| Application | `RustyNES` |

This produces the paths below.

## Linux

Follows the [XDG Base Directory
Specification](https://specifications.freedesktop.org/basedir-spec/basedir-spec-latest.html).

| Purpose | Path |
|---------|------|
| Config | `$XDG_CONFIG_HOME/RustyNES/config.toml` (commonly `~/.config/RustyNES/config.toml`) |
| Save states | `$XDG_DATA_HOME/RustyNES/saves/<rom_sha256_hex>/slot{0..9}.rns` (commonly `~/.local/share/RustyNES/saves/...`) |

If `$XDG_CONFIG_HOME` is unset, it falls back to `~/.config`. If
`$XDG_DATA_HOME` is unset, it falls back to `~/.local/share`.

## macOS

Follows the Apple Application Support layout.

| Purpose | Path |
|---------|------|
| Config | `~/Library/Application Support/dev.DoubleGate.RustyNES/config.toml` |
| Save states | `~/Library/Application Support/dev.DoubleGate.RustyNES/saves/<rom_sha256_hex>/slot{0..9}.rns` |

Note that macOS uses the same directory for config and data (Apple's
convention combines them under Application Support).

## Windows

Follows the standard `%APPDATA%` layout.

| Purpose | Path |
|---------|------|
| Config | `%APPDATA%\DoubleGate\RustyNES\config\config.toml` |
| Save states | `%APPDATA%\DoubleGate\RustyNES\data\saves\<rom_sha256_hex>\slot{0..9}.rns` |

`%APPDATA%` typically expands to `C:\Users\<username>\AppData\Roaming`.

## What lives where

### Config directory

A single file:

- `config.toml` — your keyboard bindings, rewind settings, audio sample
  rate, NTSC filter setting. See [Configuration](./configuration.md)
  for the full schema.

The file is read once at startup. It's written back whenever you change a
persisted setting from inside the emulator — "Save to disk" in the rebind
modal, the `[ui]` toggles (theme, 8:7 aspect, FPS), the run-ahead picker,
opening a ROM (updating the recent list), or dismissing the first-run
Welcome modal — see [Controls](./controls.md) and
[Configuration](./configuration.md).

The directory and file are created on demand. A pristine install has no
config files; the emulator runs from compiled-in defaults until you
change something.

### Data directory

A `saves/` subdirectory, then one directory per ROM (named by the ROM's
64-character lowercase hex SHA-256), then up to 10 slot files:

```
saves/
  3e9c1d...cafe/
    slot0.rns
    slot1.rns
    ...
  4f5a2b...beef/
    slot0.rns
```

Each `slotN.rns` is a self-contained save state — see
[Save states and rewind](./save-states-and-rewind.md) for the format and
behavior.

Per-ROM directories are created on demand, so a pristine data directory
is empty.

Alongside `saves/`, the data directory also gains sibling folders as you
use the corresponding features: `cheats/<rom_sha256>.toml` (Game Genie +
raw RAM cheats), `movies/` (`.rnm` TAS recordings), `fds-saves/`
(writable `.fds.sav` disk images), and — with the RetroAchievements
feature built — `ra-progress/`. Each is created on demand.

## Inspecting on your system

To see where your install actually writes (handy when something doesn't
work the way the table above suggests), you can verify the directory
empirically:

**Linux / macOS:**

```bash
# After saving a state at least once
find ~ -name 'slot0.rns' -path '*RustyNES*' 2>/dev/null
```

**Windows (PowerShell):**

```powershell
Get-ChildItem -Path $env:APPDATA -Recurse -Filter slot0.rns -ErrorAction SilentlyContinue
```

## Moving or backing up

- **The config file** is plain TOML — copy it to another machine
  freely.
- **Save data** is keyed by ROM SHA-256 — as long as the destination
  machine has the same ROM dump (same SHA-256), the slot files load
  there too.
- **Rewind state** is in-memory only; it doesn't persist between
  launches.

To start fresh, delete the relevant directory:

| Reset | Action |
|-------|--------|
| Default keybindings | Delete `config.toml` |
| All save states for one ROM | Delete that ROM's directory under `saves/` |
| All save states ever | Delete the `saves/` directory |
| Everything | Delete the whole `RustyNES` (Linux) / `dev.DoubleGate.RustyNES` (macOS) / `DoubleGate\RustyNES` (Windows) directory |

The emulator regenerates everything on the next launch / save.

## See also

- [Configuration](./configuration.md) — the schema of `config.toml`
- [Save states and rewind](./save-states-and-rewind.md) — what's in a `.rns` file
- [Controls](./controls.md) — the in-app rebind modal that writes to `config.toml`
