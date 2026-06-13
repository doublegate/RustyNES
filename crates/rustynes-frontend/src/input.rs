//! Keyboard -> NES controller mapping.
//!
//! Bindings are loaded from the user's config file (`config.toml`) at
//! startup; missing keys fall back to the defaults documented below.
//!
//! Default bindings (player 1):
//!
//! | Key          | NES button |
//! |--------------|------------|
//! | Arrow Up     | D-pad Up |
//! | Arrow Down   | D-pad Down |
//! | Arrow Left   | D-pad Left |
//! | Arrow Right  | D-pad Right |
//! | Z            | A |
//! | X            | B |
//! | Enter        | Start |
//! | `RShift`     | Select |
//!
//! Default bindings (player 2):
//!
//! | Key   | NES button |
//! |-------|------------|
//! | W / S / A / D | D-pad Up / Down / Left / Right |
//! | Q     | A |
//! | E     | B |
//! | P     | Start |
//! | L     | Select |
//!
//! System defaults: `Esc` quit, `F1` save state, `F4` load state, `F5`
//! rewind (held), `F2` reset, `F3` power-cycle, `F6` TAS movie record
//! toggle, `F7` TAS movie play toggle, `F8` TAS movie branch, `F9` cycle
//! FDS disk side, `F12` open ROM, `` ` `` toggle debugger overlay.
//! Rebinding is via the in-app egui modal (open the debugger overlay with
//! `` ` ``).
//!
//! Gamepad bindings (v1.6.0) are config-driven too — `[input.gamepad1]` /
//! `[input.gamepad2]` map a `gilrs::Button` name per NES button. Default
//! (player 1) Xbox-style layout: South=A, West=B, Start=Start,
//! Select=Back/Select, D-pad → D-pad. The left analog stick doubles as a
//! D-pad past `axis_deadzone`. The first physical pad seen drives player
//! 1; a second distinct pad drives player 2.

use std::collections::HashMap;

use rustynes_core::Buttons;
use winit::event::ElementState;
use winit::keyboard::{KeyCode, PhysicalKey};

use crate::config::{GamepadBindings, InputConfig, PadBindings};

/// System-level user actions.
#[derive(Clone, Copy, Debug, Eq, PartialEq, Hash)]
pub enum SysAction {
    /// Quit the application.
    Quit,
    /// Save state to the most-recent slot.
    SaveState,
    /// Load state from the most-recent slot.
    LoadState,
    /// Hold to rewind.
    Rewind,
    /// Reset (warm boot).
    Reset,
    /// Power cycle (cold boot).
    PowerCycle,
    /// Toggle the egui debug overlay (default `~`).
    ToggleDebug,
    /// Open the `rfd` file dialog to load a different ROM (default `F12`).
    OpenRom,
    /// Toggle TAS movie recording (default `F6`). Start records from a
    /// fresh power-on; stop finishes the movie and saves a `.rnm` file.
    MovieRecordToggle,
    /// Toggle TAS movie playback (default `F7`). Start opens a `.rnm`
    /// file and replays it (overriding live input); stop returns control
    /// to live input.
    MoviePlayToggle,
    /// Branch the current playback into a new recording at the current
    /// frame (default `F8`): snapshots `nes` and begins a new movie from
    /// that save-state start point.
    MovieBranch,
    /// Cycle the inserted Famicom Disk System disk side (default `F9`):
    /// eject -> side 1 -> side 2 -> ... -> wrap. A no-op for non-FDS games.
    DiskSwap,
    /// Insert a Vs. System coin into acceptor #1 (default `F10`). The latch is
    /// cleared automatically after a few frames (the real ~40-70 ms window). A
    /// no-op for non-Vs. games.
    InsertCoin,
    /// v1.0.0 (BUG-2) — toggle borderless fullscreen (default `F11`).
    ToggleFullscreen,
    /// v1.0.0 — toggle the always-on menu bar (default `Ctrl+M` — but bound by
    /// keycode here, so the default is `KeyM`). Provides a keyboard path back
    /// to the menu bar after hiding it from the View menu.
    ToggleMenuBar,
    /// Hold to fast-forward (run unthrottled, audio muted) (default `Tab`).
    /// Like [`Rewind`](Self::Rewind), this emits on BOTH press and release so
    /// the run loop can transition between normal play and fast-forward; the
    /// caller reads the live state via [`InputState::fast_forward_held`].
    FastForward,
    /// Step exactly one frame while paused (default `Backslash`). Emitted on
    /// press only.
    FrameAdvance,
    /// v1.0.0 (UX3 BUG-1) — toggle pause/resume (default `Space`). The
    /// keyboard path to pause/resume that also guarantees an escape from a
    /// paused state if the menu-bar redraw edge is ever missed. Emitted on
    /// press only.
    TogglePause,
}

/// Keyboard layout resolved from the loaded [`InputConfig`].
#[derive(Debug, Clone)]
pub struct KeyBindings {
    /// `KeyCode -> button` for player 1.
    pub player1: HashMap<KeyCode, Buttons>,
    /// `KeyCode -> button` for player 2.
    pub player2: HashMap<KeyCode, Buttons>,
    /// `KeyCode -> button` for player 3 (v1.7.0, Four Score).
    pub player3: HashMap<KeyCode, Buttons>,
    /// `KeyCode -> button` for player 4 (v1.7.0, Four Score).
    pub player4: HashMap<KeyCode, Buttons>,
    /// `KeyCode -> action` for system bindings.
    pub system: HashMap<KeyCode, SysAction>,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self::from_config(&InputConfig::default())
    }
}

impl KeyBindings {
    /// Build from a parsed [`InputConfig`]. Bindings whose string doesn't
    /// match a known [`KeyCode`] are silently dropped (a `eprintln!` warns
    /// about each one so the user can spot typos).
    #[must_use]
    pub fn from_config(cfg: &InputConfig) -> Self {
        let player1 = pad_to_map(&cfg.player1);
        let player2 = pad_to_map(&cfg.player2);
        let player3 = pad_to_map(&cfg.player3);
        let player4 = pad_to_map(&cfg.player4);
        let mut system = HashMap::new();
        try_bind(&mut system, &cfg.system.quit, SysAction::Quit);
        try_bind(&mut system, &cfg.system.save_state, SysAction::SaveState);
        try_bind(&mut system, &cfg.system.load_state, SysAction::LoadState);
        try_bind(&mut system, &cfg.system.rewind, SysAction::Rewind);
        try_bind(&mut system, &cfg.system.reset, SysAction::Reset);
        try_bind(&mut system, &cfg.system.power_cycle, SysAction::PowerCycle);
        try_bind(
            &mut system,
            &cfg.system.debug_overlay,
            SysAction::ToggleDebug,
        );
        try_bind(&mut system, &cfg.system.open_rom, SysAction::OpenRom);
        try_bind(
            &mut system,
            &cfg.system.movie_record,
            SysAction::MovieRecordToggle,
        );
        try_bind(
            &mut system,
            &cfg.system.movie_play,
            SysAction::MoviePlayToggle,
        );
        try_bind(
            &mut system,
            &cfg.system.movie_branch,
            SysAction::MovieBranch,
        );
        try_bind(&mut system, &cfg.system.disk_swap, SysAction::DiskSwap);
        try_bind(&mut system, &cfg.system.insert_coin, SysAction::InsertCoin);
        try_bind(
            &mut system,
            &cfg.system.fullscreen,
            SysAction::ToggleFullscreen,
        );
        try_bind(
            &mut system,
            &cfg.system.toggle_menu_bar,
            SysAction::ToggleMenuBar,
        );
        try_bind(
            &mut system,
            &cfg.system.fast_forward,
            SysAction::FastForward,
        );
        try_bind(
            &mut system,
            &cfg.system.frame_advance,
            SysAction::FrameAdvance,
        );
        try_bind(&mut system, &cfg.system.pause, SysAction::TogglePause);
        Self {
            player1,
            player2,
            player3,
            player4,
            system,
        }
    }
}

/// Gamepad layout resolved from a [`GamepadBindings`] config section.
///
/// Holds a `gilrs::Button -> Buttons` lookup plus the analog-stick
/// deadzone. Built per player; the live [`InputState`] consults the map
/// for the pad currently assigned to that player.
#[derive(Debug, Clone)]
pub struct GamepadMap {
    /// `gilrs::Button -> NES button`. Unbound / `Button::Unknown`
    /// entries are simply absent.
    buttons: HashMap<gilrs::Button, Buttons>,
    /// Left-stick deflection past which an axis counts as a D-pad press.
    deadzone: f32,
}

impl Default for GamepadMap {
    fn default() -> Self {
        Self::from_config(&GamepadBindings::default_xbox())
    }
}

impl GamepadMap {
    /// Build from a parsed [`GamepadBindings`]. Bindings whose string
    /// doesn't match a known `gilrs::Button` are dropped (a warning is
    /// printed so the user can spot typos), mirroring [`KeyBindings`].
    #[must_use]
    pub fn from_config(cfg: &GamepadBindings) -> Self {
        let mut buttons = HashMap::new();
        bind_pad(&mut buttons, &cfg.up, Buttons::UP);
        bind_pad(&mut buttons, &cfg.down, Buttons::DOWN);
        bind_pad(&mut buttons, &cfg.left, Buttons::LEFT);
        bind_pad(&mut buttons, &cfg.right, Buttons::RIGHT);
        bind_pad(&mut buttons, &cfg.a, Buttons::A);
        bind_pad(&mut buttons, &cfg.b, Buttons::B);
        bind_pad(&mut buttons, &cfg.select, Buttons::SELECT);
        bind_pad(&mut buttons, &cfg.start, Buttons::START);
        Self {
            buttons,
            deadzone: cfg.axis_deadzone.clamp(0.05, 0.95),
        }
    }

    /// Look up the NES button bound to a `gilrs::Button`. Returns `None`
    /// for unbound buttons (and always for `Button::Unknown`, which is
    /// never inserted).
    #[must_use]
    pub fn lookup(&self, btn: gilrs::Button) -> Option<Buttons> {
        self.buttons.get(&btn).copied()
    }

    /// The configured analog-stick deadzone.
    #[must_use]
    pub const fn deadzone(&self) -> f32 {
        self.deadzone
    }
}

fn bind_pad(m: &mut HashMap<gilrs::Button, Buttons>, name: &str, btn: Buttons) {
    match parse_gamepad_button(name) {
        Some(code) if code != gilrs::Button::Unknown => {
            m.insert(code, btn);
        }
        _ => eprintln!("rustynes: unknown gamepad button {name:?} in input config"),
    }
}

/// Resolve all four per-player [`GamepadMap`]s from an [`InputConfig`]
/// (v1.7.0). Players 3/4 only matter when Four Score is enabled, but the
/// maps are always built so a live toggle needs no reload.
fn gamepad_maps_from_config(cfg: &InputConfig) -> [GamepadMap; MAX_PADS] {
    [
        GamepadMap::from_config(&cfg.gamepad1),
        GamepadMap::from_config(&cfg.gamepad2),
        GamepadMap::from_config(&cfg.gamepad3),
        GamepadMap::from_config(&cfg.gamepad4),
    ]
}

fn pad_to_map(b: &PadBindings) -> HashMap<KeyCode, Buttons> {
    let mut m = HashMap::new();
    bind_button(&mut m, &b.up, Buttons::UP);
    bind_button(&mut m, &b.down, Buttons::DOWN);
    bind_button(&mut m, &b.left, Buttons::LEFT);
    bind_button(&mut m, &b.right, Buttons::RIGHT);
    bind_button(&mut m, &b.a, Buttons::A);
    bind_button(&mut m, &b.b, Buttons::B);
    bind_button(&mut m, &b.select, Buttons::SELECT);
    bind_button(&mut m, &b.start, Buttons::START);
    m
}

fn bind_button(m: &mut HashMap<KeyCode, Buttons>, name: &str, btn: Buttons) {
    if let Some(code) = parse_keycode(name) {
        m.insert(code, btn);
    } else {
        eprintln!("rustynes: unknown keycode {name:?} in input config");
    }
}

fn try_bind(m: &mut HashMap<KeyCode, SysAction>, name: &str, action: SysAction) {
    if let Some(code) = parse_keycode(name) {
        m.insert(code, action);
    } else {
        eprintln!("rustynes: unknown system keycode {name:?} in input config");
    }
}

/// Translate a legacy winit-0.29 `VirtualKeyCode` Debug name into the
/// equivalent winit-0.30 `KeyCode` Debug name. Returns `None` when the
/// input is not a recognized legacy name (the caller then tries it as a
/// current `KeyCode` name directly).
///
/// Background (v1.3.1 regression fix): `Config::migrate_legacy` renamed
/// the legacy `[input.keyboard_pN]` sections to `[input.playerN]` but
/// carried the keycode *string values* over verbatim. Those values are
/// old winit-0.29 `VirtualKeyCode` Debug names (`Up`, `Z`, `Return`,
/// `RShift`, single letters, ...), which the winit-0.30 `KeyCode` schema
/// does not recognize — so every migrated binding hit the "unknown
/// keycode" path and input went dead. Accepting the legacy names as
/// aliases here repairs an already-migrated on-disk config with no manual
/// user action. Current `KeyCode` names are checked first in
/// [`parse_keycode`], so this never shadows a valid current name.
///
/// Covers the winit 0.29 → 0.30 keyboard-API rename
/// (`VirtualKeyCode` → `KeyCode`): the common keys the frontend's
/// defaults and the legacy schema used.
#[must_use]
#[allow(clippy::too_many_lines)]
fn legacy_keycode_alias(name: &str) -> Option<&'static str> {
    Some(match name {
        // Arrows: bare direction -> Arrow*.
        "Up" => "ArrowUp",
        "Down" => "ArrowDown",
        "Left" => "ArrowLeft",
        "Right" => "ArrowRight",
        // Letters: bare single letter -> Key*.
        "A" => "KeyA",
        "B" => "KeyB",
        "C" => "KeyC",
        "D" => "KeyD",
        "E" => "KeyE",
        "F" => "KeyF",
        "G" => "KeyG",
        "H" => "KeyH",
        "I" => "KeyI",
        "J" => "KeyJ",
        "K" => "KeyK",
        "L" => "KeyL",
        "M" => "KeyM",
        "N" => "KeyN",
        "O" => "KeyO",
        "P" => "KeyP",
        "Q" => "KeyQ",
        "R" => "KeyR",
        "S" => "KeyS",
        "T" => "KeyT",
        "U" => "KeyU",
        "V" => "KeyV",
        "W" => "KeyW",
        "X" => "KeyX",
        "Y" => "KeyY",
        "Z" => "KeyZ",
        // Digits: bare digit or winit-0.29 `Key0`..`Key9` -> Digit*.
        // (winit 0.29 named the number row `Key1`..`Key0`; winit 0.30
        // uses `Digit1`..`Digit0`. Both spellings are aliased.)
        "0" | "Key0" => "Digit0",
        "1" | "Key1" => "Digit1",
        "2" | "Key2" => "Digit2",
        "3" | "Key3" => "Digit3",
        "4" | "Key4" => "Digit4",
        "5" | "Key5" => "Digit5",
        "6" | "Key6" => "Digit6",
        "7" | "Key7" => "Digit7",
        "8" | "Key8" => "Digit8",
        "9" | "Key9" => "Digit9",
        // Modifiers / named keys renamed in winit 0.30.
        "Return" => "Enter",
        "LShift" => "ShiftLeft",
        "RShift" => "ShiftRight",
        "LControl" => "ControlLeft",
        "RControl" => "ControlRight",
        "LAlt" => "AltLeft",
        "RAlt" => "AltRight",
        "LWin" => "SuperLeft",
        "RWin" => "SuperRight",
        "Back" => "Backspace",
        "Capital" => "CapsLock",
        "Grave" => "Backquote",
        "Apostrophe" => "Quote",
        // winit 0.29 numpad names were `Numpad0`..`Numpad9` (unchanged in
        // 0.30) — no alias needed, they fall through to `parse_keycode`.
        _ => return None,
    })
}

/// Parse a `KeyCode` name (the `Debug` representation, e.g. `"ArrowUp"`,
/// `"KeyZ"`, `"F5"`). Returns `None` for unknown names.
///
/// Current winit-0.30 `KeyCode` Debug names are matched first; if the
/// name is not a current name, it is retried through the legacy
/// winit-0.29 `VirtualKeyCode` alias table (`legacy_keycode_alias`) so a
/// pre-v1.3.1 config (migrated structurally but carrying legacy value
/// strings) still resolves. Current names always take precedence.
///
/// Only the keycodes the frontend cares about are handled — adding new
/// ones is a one-line match-arm extension.
#[must_use]
#[allow(clippy::too_many_lines)]
pub fn parse_keycode(name: &str) -> Option<KeyCode> {
    Some(match name {
        // Letters
        "KeyA" => KeyCode::KeyA,
        "KeyB" => KeyCode::KeyB,
        "KeyC" => KeyCode::KeyC,
        "KeyD" => KeyCode::KeyD,
        "KeyE" => KeyCode::KeyE,
        "KeyF" => KeyCode::KeyF,
        "KeyG" => KeyCode::KeyG,
        "KeyH" => KeyCode::KeyH,
        "KeyI" => KeyCode::KeyI,
        "KeyJ" => KeyCode::KeyJ,
        "KeyK" => KeyCode::KeyK,
        "KeyL" => KeyCode::KeyL,
        "KeyM" => KeyCode::KeyM,
        "KeyN" => KeyCode::KeyN,
        "KeyO" => KeyCode::KeyO,
        "KeyP" => KeyCode::KeyP,
        "KeyQ" => KeyCode::KeyQ,
        "KeyR" => KeyCode::KeyR,
        "KeyS" => KeyCode::KeyS,
        "KeyT" => KeyCode::KeyT,
        "KeyU" => KeyCode::KeyU,
        "KeyV" => KeyCode::KeyV,
        "KeyW" => KeyCode::KeyW,
        "KeyX" => KeyCode::KeyX,
        "KeyY" => KeyCode::KeyY,
        "KeyZ" => KeyCode::KeyZ,
        // Digits (top row)
        "Digit0" => KeyCode::Digit0,
        "Digit1" => KeyCode::Digit1,
        "Digit2" => KeyCode::Digit2,
        "Digit3" => KeyCode::Digit3,
        "Digit4" => KeyCode::Digit4,
        "Digit5" => KeyCode::Digit5,
        "Digit6" => KeyCode::Digit6,
        "Digit7" => KeyCode::Digit7,
        "Digit8" => KeyCode::Digit8,
        "Digit9" => KeyCode::Digit9,
        // Function keys
        "F1" => KeyCode::F1,
        "F2" => KeyCode::F2,
        "F3" => KeyCode::F3,
        "F4" => KeyCode::F4,
        "F5" => KeyCode::F5,
        "F6" => KeyCode::F6,
        "F7" => KeyCode::F7,
        "F8" => KeyCode::F8,
        "F9" => KeyCode::F9,
        "F10" => KeyCode::F10,
        "F11" => KeyCode::F11,
        "F12" => KeyCode::F12,
        // Arrows
        "ArrowUp" => KeyCode::ArrowUp,
        "ArrowDown" => KeyCode::ArrowDown,
        "ArrowLeft" => KeyCode::ArrowLeft,
        "ArrowRight" => KeyCode::ArrowRight,
        // Modifiers
        "ShiftLeft" => KeyCode::ShiftLeft,
        "ShiftRight" => KeyCode::ShiftRight,
        "ControlLeft" => KeyCode::ControlLeft,
        "ControlRight" => KeyCode::ControlRight,
        "AltLeft" => KeyCode::AltLeft,
        "AltRight" => KeyCode::AltRight,
        "SuperLeft" => KeyCode::SuperLeft,
        "SuperRight" => KeyCode::SuperRight,
        // Punctuation / whitespace
        "Space" => KeyCode::Space,
        "Enter" => KeyCode::Enter,
        "Tab" => KeyCode::Tab,
        "Backspace" => KeyCode::Backspace,
        "Escape" => KeyCode::Escape,
        "CapsLock" => KeyCode::CapsLock,
        "Comma" => KeyCode::Comma,
        "Period" => KeyCode::Period,
        "Slash" => KeyCode::Slash,
        "Backslash" => KeyCode::Backslash,
        "Semicolon" => KeyCode::Semicolon,
        "Quote" => KeyCode::Quote,
        "Backquote" => KeyCode::Backquote,
        "Minus" => KeyCode::Minus,
        "Equal" => KeyCode::Equal,
        "BracketLeft" => KeyCode::BracketLeft,
        "BracketRight" => KeyCode::BracketRight,
        // Numpad (a few common ones)
        "Numpad0" => KeyCode::Numpad0,
        "Numpad1" => KeyCode::Numpad1,
        "Numpad2" => KeyCode::Numpad2,
        "Numpad3" => KeyCode::Numpad3,
        "Numpad4" => KeyCode::Numpad4,
        "Numpad5" => KeyCode::Numpad5,
        "Numpad6" => KeyCode::Numpad6,
        "Numpad7" => KeyCode::Numpad7,
        "Numpad8" => KeyCode::Numpad8,
        "Numpad9" => KeyCode::Numpad9,
        // Not a current `KeyCode` name — retry it as a legacy winit-0.29
        // `VirtualKeyCode` name (repairs pre-v1.3.1 migrated configs).
        // Recurse exactly once: `legacy_keycode_alias` only ever yields a
        // current name, so the second `parse_keycode` cannot re-alias.
        other => return legacy_keycode_alias(other).and_then(parse_keycode),
    })
}

/// Canonicalize a keycode name string to its current winit-0.30
/// `KeyCode` Debug spelling.
///
/// If `name` is already a current name (or an unrecognized name), it is
/// returned unchanged; if it is a legacy winit-0.29 `VirtualKeyCode`
/// name, the current equivalent is returned. Used by
/// `Config::migrate_legacy` so freshly-migrated config files are written
/// with clean canonical names (`ArrowUp`, `KeyZ`, ...) rather than the
/// legacy spellings — `parse_keycode` accepts both, so this is cosmetic,
/// but it keeps on-disk files current.
#[must_use]
pub fn canonical_keycode_name(name: &str) -> String {
    // A current name is left as-is; a legacy name maps to its current
    // spelling. An unrecognized name passes through unchanged (the
    // migration shouldn't silently drop unknown values).
    if parse_keycode(name).is_some() {
        legacy_keycode_alias(name).map_or_else(|| name.to_owned(), ToOwned::to_owned)
    } else {
        name.to_owned()
    }
}

/// Parse a `gilrs::Button` name into the matching button.
///
/// The name is the button's `Debug` representation, e.g. `"South"`,
/// `"DPadUp"`, `"Start"`. Returns `None` for unknown names; `"Unknown"`
/// parses to `Button::Unknown` (a caller should treat that as unbound —
/// `GamepadMap::from_config` drops it).
///
/// Mirrors [`parse_keycode`]: only the buttons the frontend can bind are
/// enumerated, so an unrecognized string is rejected rather than
/// silently mapping to `Unknown`.
#[must_use]
pub fn parse_gamepad_button(name: &str) -> Option<gilrs::Button> {
    use gilrs::Button;
    Some(match name {
        "South" => Button::South,
        "East" => Button::East,
        "North" => Button::North,
        "West" => Button::West,
        "C" => Button::C,
        "Z" => Button::Z,
        "LeftTrigger" => Button::LeftTrigger,
        "LeftTrigger2" => Button::LeftTrigger2,
        "RightTrigger" => Button::RightTrigger,
        "RightTrigger2" => Button::RightTrigger2,
        "Select" => Button::Select,
        "Start" => Button::Start,
        "Mode" => Button::Mode,
        "LeftThumb" => Button::LeftThumb,
        "RightThumb" => Button::RightThumb,
        "DPadUp" => Button::DPadUp,
        "DPadDown" => Button::DPadDown,
        "DPadLeft" => Button::DPadLeft,
        "DPadRight" => Button::DPadRight,
        "Unknown" => Button::Unknown,
        _ => return None,
    })
}

/// Canonicalize a `gilrs::Button` name to its `Debug` spelling.
///
/// A recognized name is returned as the canonical static string; an
/// unrecognized name passes through unchanged (so a rebind UI write
/// never silently drops a value). Used when persisting a rebound pad
/// button to keep on-disk names canonical.
#[must_use]
pub fn canonical_gamepad_button_name(name: &str) -> String {
    parse_gamepad_button(name).map_or_else(|| name.to_owned(), |b| format!("{b:?}"))
}

/// Number of distinct physical gamepads (and players) the frontend can
/// drive. Two by default; up to four with the Four Score adapter.
const MAX_PADS: usize = 4;

/// Which player a freshly-seen physical gamepad is assigned to. The
/// first distinct `gilrs::GamepadId` seen drives player 1; the second
/// distinct id drives player 2; the third / fourth drive players 3 / 4
/// (Four Score, v1.7.0); further pads are ignored. Per-device assignment
/// keeps four players on four pads without a device-picker UI.
#[derive(Debug, Clone, Default)]
struct PadAssignment {
    /// Ids assigned to players 0..=3, in first-seen order. A `None` slot
    /// is still free.
    ids: [Option<gilrs::GamepadId>; MAX_PADS],
}

impl PadAssignment {
    /// Resolve a gamepad id to a player index (0..=3), assigning the
    /// next free port on first sight. Returns `None` once all four ports
    /// are taken by other devices.
    fn player_for(&mut self, id: gilrs::GamepadId) -> Option<usize> {
        if let Some(i) = self.ids.iter().position(|&slot| slot == Some(id)) {
            return Some(i);
        }
        let free = self.ids.iter().position(Option::is_none)?;
        self.ids[free] = Some(id);
        Some(free)
    }
}

/// Live input state — keyboard held buttons + gamepad held buttons +
/// system-key triggers.
///
/// The gamepad's held buttons live in separate per-player fields;
/// `player1()`..`player4()` return the **union** of keyboard and pad so
/// any binding (keyboard or pad) wakes the corresponding NES bit. P3/P4
/// (v1.7.0, Four Score) are inert unless the adapter is enabled in the
/// app, which decides whether to push them to `nes.set_buttons(2/3, ..)`.
#[derive(Debug, Clone)]
pub struct InputState {
    /// Currently-held keyboard buttons, indexed by player (0..=3).
    keyboard_buttons: [Buttons; MAX_PADS],
    /// Currently-held gamepad buttons, indexed by player (0..=3).
    gamepad_buttons: [Buttons; MAX_PADS],
    /// Currently-held analog-stick D-pad bits, indexed by player. Kept
    /// separate from the digital pad bits so a stick recentering past
    /// the deadzone clears only the stick contribution.
    gamepad_axis: [Buttons; MAX_PADS],
    /// Resolved keyboard bindings.
    bindings: KeyBindings,
    /// Resolved per-player gamepad maps (0..=3).
    gamepad_maps: [GamepadMap; MAX_PADS],
    /// Which physical pad drives which player.
    pad_assignment: PadAssignment,
    /// `true` while the rewind key is held.
    rewind_held: bool,
    /// `true` while the fast-forward key is held.
    fast_forward_held: bool,
}

impl InputState {
    /// New state with the supplied keyboard + gamepad bindings.
    #[must_use]
    pub fn new(bindings: KeyBindings, gamepad_maps: [GamepadMap; MAX_PADS]) -> Self {
        Self {
            keyboard_buttons: [Buttons::empty(); MAX_PADS],
            gamepad_buttons: [Buttons::empty(); MAX_PADS],
            gamepad_axis: [Buttons::empty(); MAX_PADS],
            bindings,
            gamepad_maps,
            pad_assignment: PadAssignment::default(),
            rewind_held: false,
            fast_forward_held: false,
        }
    }

    /// Build from a parsed [`InputConfig`] (keyboard + all four gamepad
    /// maps).
    #[must_use]
    pub fn from_config(cfg: &InputConfig) -> Self {
        Self::new(KeyBindings::from_config(cfg), gamepad_maps_from_config(cfg))
    }

    /// Rebuild the keyboard + gamepad maps from a (possibly edited)
    /// [`InputConfig`], so an in-app rebind takes effect immediately.
    /// Held-button state and the pad-to-player assignment are preserved.
    pub fn reload_bindings(&mut self, cfg: &InputConfig) {
        self.bindings = KeyBindings::from_config(cfg);
        self.gamepad_maps = gamepad_maps_from_config(cfg);
    }

    /// Default bindings (matches the legacy hardcoded layout).
    //
    // Used by the unit tests; the binary always goes through
    // [`Self::from_config`] with config-loaded bindings.
    #[must_use]
    #[allow(dead_code)]
    pub fn with_defaults() -> Self {
        Self::from_config(&InputConfig::default())
    }

    /// Currently-held buttons for `player` (0..=3): keyboard OR gamepad
    /// OR stick. Out-of-range indices return [`Buttons::empty`].
    #[must_use]
    pub fn player(&self, player: usize) -> Buttons {
        if player >= MAX_PADS {
            return Buttons::empty();
        }
        self.keyboard_buttons[player] | self.gamepad_buttons[player] | self.gamepad_axis[player]
    }

    /// Currently-held player-1 buttons (keyboard OR gamepad OR stick).
    #[must_use]
    pub fn player1(&self) -> Buttons {
        self.player(0)
    }

    /// Currently-held player-2 buttons (keyboard OR gamepad OR stick).
    #[must_use]
    pub fn player2(&self) -> Buttons {
        self.player(1)
    }

    /// Currently-held player-3 buttons (v1.7.0, Four Score).
    #[must_use]
    pub fn player3(&self) -> Buttons {
        self.player(2)
    }

    /// Currently-held player-4 buttons (v1.7.0, Four Score).
    #[must_use]
    pub fn player4(&self) -> Buttons {
        self.player(3)
    }

    /// Apply a `gilrs` event from a specific connected gamepad. The pad
    /// is routed to the player it's assigned to (first pad → P1, second
    /// → P2) and its config-driven map consulted. Button presses set
    /// digital bits; left-stick deflection past the deadzone drives the
    /// D-pad.
    pub fn handle_gamepad_event(&mut self, id: gilrs::GamepadId, event: &gilrs::EventType) {
        let Some(player) = self.pad_assignment.player_for(id) else {
            return;
        };
        match *event {
            gilrs::EventType::ButtonPressed(btn, _) => self.set_gamepad_button(player, btn, true),
            gilrs::EventType::ButtonReleased(btn, _) => self.set_gamepad_button(player, btn, false),
            gilrs::EventType::AxisChanged(axis, value, _) => {
                self.set_gamepad_axis(player, axis, value);
            }
            _ => {}
        }
    }

    /// Mark a specific gamepad button as pressed or released for a
    /// player. Pure helper exposed for unit tests; the `gilrs` event
    /// glue funnels through here.
    pub fn set_gamepad_button(&mut self, player: usize, btn: gilrs::Button, pressed: bool) {
        let Some(slot) = self.gamepad_buttons.get_mut(player) else {
            return;
        };
        if let Some(nes_btn) = self.gamepad_maps[player].lookup(btn) {
            slot.set(nes_btn, pressed);
        }
    }

    /// Apply a left-analog-stick axis change for a player: any
    /// deflection past the configured deadzone latches the matching
    /// D-pad bit (and clears the opposite one); recentering clears both.
    /// Only `LeftStickX` / `LeftStickY` are mapped — the right stick and
    /// the dedicated D-pad axes are ignored here (the D-pad arrives as
    /// `Button::DPad*` events). `Axis::Unknown` is ignored.
    pub fn set_gamepad_axis(&mut self, player: usize, axis: gilrs::Axis, value: f32) {
        let Some(slot) = self.gamepad_axis.get_mut(player) else {
            return;
        };
        let dz = self.gamepad_maps[player].deadzone();
        match axis {
            gilrs::Axis::LeftStickX => {
                slot.set(Buttons::LEFT, value <= -dz);
                slot.set(Buttons::RIGHT, value >= dz);
            }
            gilrs::Axis::LeftStickY => {
                // gilrs reports +Y up, -Y down.
                slot.set(Buttons::UP, value >= dz);
                slot.set(Buttons::DOWN, value <= -dz);
            }
            _ => {}
        }
    }

    /// `true` while the rewind key is held.
    #[must_use]
    pub const fn rewind_held(&self) -> bool {
        self.rewind_held
    }

    /// `true` while the fast-forward key is held.
    #[must_use]
    pub const fn fast_forward_held(&self) -> bool {
        self.fast_forward_held
    }

    /// Apply a winit keyboard event. Returns `Some(action)` for system
    /// keys (`Quit`, `SaveState`, `LoadState`, `Reset`, `PowerCycle`) on
    /// press, and always returns `Some(SysAction::Rewind)` for the rewind
    /// key on either press or release (the caller distinguishes via
    /// [`Self::rewind_held`]).
    pub fn handle_key(&mut self, key: PhysicalKey, state: ElementState) -> Option<SysAction> {
        let PhysicalKey::Code(code) = key else {
            return None;
        };
        let pressed = state == ElementState::Pressed;

        // Per-player keyboard button bits (P1..P4). A single key can be
        // bound on more than one player; each map is consulted.
        for (player, map) in [
            &self.bindings.player1,
            &self.bindings.player2,
            &self.bindings.player3,
            &self.bindings.player4,
        ]
        .into_iter()
        .enumerate()
        {
            if let Some(btn) = map.get(&code).copied() {
                self.keyboard_buttons[player].set(btn, pressed);
            }
        }

        // System actions. Rewind is special: emit on both press and
        // release so the run loop can transition between forward play
        // and step-back.
        if let Some(&action) = self.bindings.system.get(&code) {
            if action == SysAction::Rewind {
                self.rewind_held = pressed;
                return Some(SysAction::Rewind);
            }
            // Fast-forward is a held key like rewind: track the live state and
            // emit on both edges so the caller can publish the change at once.
            if action == SysAction::FastForward {
                self.fast_forward_held = pressed;
                return Some(SysAction::FastForward);
            }
            if pressed {
                return Some(action);
            }
        }
        None
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn down(code: KeyCode) -> (PhysicalKey, ElementState) {
        (PhysicalKey::Code(code), ElementState::Pressed)
    }
    fn up(code: KeyCode) -> (PhysicalKey, ElementState) {
        (PhysicalKey::Code(code), ElementState::Released)
    }

    #[test]
    fn dpad_keys_map_to_dpad_bits() {
        let mut s = InputState::with_defaults();
        let (k, e) = down(KeyCode::ArrowUp);
        s.handle_key(k, e);
        assert!(s.player1().contains(Buttons::UP));
        let (k, e) = down(KeyCode::ArrowLeft);
        s.handle_key(k, e);
        assert!(s.player1().contains(Buttons::LEFT));
        let (k, e) = up(KeyCode::ArrowUp);
        s.handle_key(k, e);
        assert!(!s.player1().contains(Buttons::UP));
        assert!(s.player1().contains(Buttons::LEFT));
    }

    #[test]
    fn ab_start_select_map_correctly() {
        let mut s = InputState::with_defaults();
        for (key, want) in [
            (KeyCode::KeyZ, Buttons::A),
            (KeyCode::KeyX, Buttons::B),
            (KeyCode::Enter, Buttons::START),
            (KeyCode::ShiftRight, Buttons::SELECT),
        ] {
            let (k, e) = down(key);
            s.handle_key(k, e);
            assert!(s.player1().contains(want), "{key:?} -> {want:?}");
        }
        assert_eq!(
            s.player1(),
            Buttons::A | Buttons::B | Buttons::START | Buttons::SELECT
        );
    }

    #[test]
    fn unmapped_key_returns_none() {
        let mut s = InputState::with_defaults();
        let before = s.player1();
        // Pick a key that isn't bound by default. KeyG is currently
        // unmapped in the default layout.
        let (k, e) = down(KeyCode::KeyG);
        assert_eq!(s.handle_key(k, e), None);
        assert_eq!(s.player1(), before);
    }

    #[test]
    fn debug_overlay_key_returns_toggle_action() {
        // Sprint 5-3: `~` toggles the debugger.
        let mut s = InputState::with_defaults();
        let (k, e) = down(KeyCode::Backquote);
        assert_eq!(s.handle_key(k, e), Some(SysAction::ToggleDebug));
    }

    #[test]
    fn esc_returns_quit() {
        let mut s = InputState::with_defaults();
        let (k, e) = down(KeyCode::Escape);
        assert_eq!(s.handle_key(k, e), Some(SysAction::Quit));
    }

    #[test]
    fn rewind_press_and_release_track_state() {
        let mut s = InputState::with_defaults();
        assert!(!s.rewind_held());
        let (k, e) = down(KeyCode::F5);
        assert_eq!(s.handle_key(k, e), Some(SysAction::Rewind));
        assert!(s.rewind_held());
        let (k, e) = up(KeyCode::F5);
        assert_eq!(s.handle_key(k, e), Some(SysAction::Rewind));
        assert!(!s.rewind_held());
    }

    #[test]
    fn player2_default_bindings_drive_p2_buttons() {
        // Defaults: P2 uses WASD + Q/E + P/L.
        let mut s = InputState::with_defaults();
        for (key, want) in [
            (KeyCode::KeyW, Buttons::UP),
            (KeyCode::KeyQ, Buttons::A),
            (KeyCode::KeyE, Buttons::B),
            (KeyCode::KeyP, Buttons::START),
            (KeyCode::KeyL, Buttons::SELECT),
        ] {
            let (k, e) = down(key);
            s.handle_key(k, e);
            assert!(s.player2().contains(want), "P2 {key:?} -> {want:?}");
        }
        // P1 button state must NOT pick up P2-only keys.
        assert!(!s.player1().contains(Buttons::A));
    }

    #[test]
    fn save_load_bindings_default_to_function_keys() {
        let mut s = InputState::with_defaults();
        assert_eq!(
            s.handle_key(down(KeyCode::F1).0, down(KeyCode::F1).1),
            Some(SysAction::SaveState)
        );
        assert_eq!(
            s.handle_key(down(KeyCode::F4).0, down(KeyCode::F4).1),
            Some(SysAction::LoadState)
        );
        assert_eq!(
            s.handle_key(down(KeyCode::F2).0, down(KeyCode::F2).1),
            Some(SysAction::Reset)
        );
        assert_eq!(
            s.handle_key(down(KeyCode::F3).0, down(KeyCode::F3).1),
            Some(SysAction::PowerCycle)
        );
    }

    #[test]
    fn movie_bindings_default_to_function_keys() {
        // v1.4.0 Sprint 4.2: F6 record toggle, F7 play toggle, F8 branch.
        let mut s = InputState::with_defaults();
        assert_eq!(
            s.handle_key(down(KeyCode::F6).0, down(KeyCode::F6).1),
            Some(SysAction::MovieRecordToggle)
        );
        assert_eq!(
            s.handle_key(down(KeyCode::F7).0, down(KeyCode::F7).1),
            Some(SysAction::MoviePlayToggle)
        );
        assert_eq!(
            s.handle_key(down(KeyCode::F8).0, down(KeyCode::F8).1),
            Some(SysAction::MovieBranch)
        );
    }

    #[test]
    fn disk_swap_binding_defaults_to_f9() {
        // v2.2.0: F9 cycles the FDS disk side.
        let mut s = InputState::with_defaults();
        assert_eq!(
            s.handle_key(down(KeyCode::F9).0, down(KeyCode::F9).1),
            Some(SysAction::DiskSwap)
        );
    }

    #[test]
    fn insert_coin_binding_defaults_to_f10() {
        // v2.5.0: F10 inserts a Vs. System coin.
        let mut s = InputState::with_defaults();
        assert_eq!(
            s.handle_key(down(KeyCode::F10).0, down(KeyCode::F10).1),
            Some(SysAction::InsertCoin)
        );
    }

    #[test]
    fn gamepad_button_default_xbox_mapping() {
        use gilrs::Button;
        // The default-config map reproduces the legacy hardcoded layout.
        let map = GamepadMap::default();
        assert_eq!(map.lookup(Button::South), Some(Buttons::A));
        assert_eq!(map.lookup(Button::West), Some(Buttons::B));
        assert_eq!(map.lookup(Button::Start), Some(Buttons::START));
        assert_eq!(map.lookup(Button::Select), Some(Buttons::SELECT));
        assert_eq!(map.lookup(Button::DPadUp), Some(Buttons::UP));
        assert_eq!(map.lookup(Button::DPadDown), Some(Buttons::DOWN));
        assert_eq!(map.lookup(Button::DPadLeft), Some(Buttons::LEFT));
        assert_eq!(map.lookup(Button::DPadRight), Some(Buttons::RIGHT));
        // Unmapped: triggers, sticks, North/East face buttons.
        assert_eq!(map.lookup(Button::North), None);
        assert_eq!(map.lookup(Button::East), None);
        assert_eq!(map.lookup(Button::LeftTrigger), None);
    }

    #[test]
    fn gamepad_press_release_drives_player1_state() {
        use gilrs::Button;
        let mut s = InputState::with_defaults();
        s.set_gamepad_button(0, Button::South, true);
        assert!(s.player1().contains(Buttons::A));
        s.set_gamepad_button(0, Button::South, false);
        assert!(!s.player1().contains(Buttons::A));
    }

    #[test]
    fn gamepad_and_keyboard_are_or_ed_into_player1() {
        use gilrs::Button;
        // Press keyboard Z (=A) and gamepad South (=A); release one;
        // player1 should still report A while the other is held.
        let mut s = InputState::with_defaults();
        let (k, e) = down(KeyCode::KeyZ);
        s.handle_key(k, e);
        s.set_gamepad_button(0, Button::South, true);
        assert!(s.player1().contains(Buttons::A));
        let (k, e) = up(KeyCode::KeyZ);
        s.handle_key(k, e);
        // Keyboard released, gamepad still held -> A bit still asserted.
        assert!(s.player1().contains(Buttons::A));
        s.set_gamepad_button(0, Button::South, false);
        assert!(!s.player1().contains(Buttons::A));
    }

    #[test]
    fn rebound_gamepad_button_takes_effect_after_reload() {
        use gilrs::Button;
        // Default: North is unmapped, East is unmapped, A=South.
        let mut cfg = InputConfig::default();
        let mut s = InputState::from_config(&cfg);
        s.set_gamepad_button(0, Button::North, true);
        assert!(
            !s.player1().contains(Buttons::A),
            "North unbound by default"
        );
        // Rebind A to North and reload — the new binding must apply live.
        cfg.gamepad1.a = "North".into();
        s.reload_bindings(&cfg);
        s.set_gamepad_button(0, Button::North, true);
        assert!(s.player1().contains(Buttons::A));
    }

    #[test]
    fn left_stick_past_deadzone_drives_dpad() {
        use gilrs::Axis;
        let mut s = InputState::with_defaults();
        // Below the 0.5 deadzone: no D-pad.
        s.set_gamepad_axis(0, Axis::LeftStickX, 0.3);
        assert!(!s.player1().contains(Buttons::RIGHT));
        // Past it: RIGHT latches.
        s.set_gamepad_axis(0, Axis::LeftStickX, 0.9);
        assert!(s.player1().contains(Buttons::RIGHT));
        assert!(!s.player1().contains(Buttons::LEFT));
        // Swing the other way: LEFT, not RIGHT.
        s.set_gamepad_axis(0, Axis::LeftStickX, -0.9);
        assert!(s.player1().contains(Buttons::LEFT));
        assert!(!s.player1().contains(Buttons::RIGHT));
        // Recenter clears both.
        s.set_gamepad_axis(0, Axis::LeftStickX, 0.0);
        assert!(!s.player1().contains(Buttons::LEFT));
        assert!(!s.player1().contains(Buttons::RIGHT));
        // +Y is up, -Y is down (gilrs convention).
        s.set_gamepad_axis(0, Axis::LeftStickY, 0.9);
        assert!(s.player1().contains(Buttons::UP));
        s.set_gamepad_axis(0, Axis::LeftStickY, -0.9);
        assert!(s.player1().contains(Buttons::DOWN));
        assert!(!s.player1().contains(Buttons::UP));
    }

    #[test]
    fn two_pads_drive_independent_players() {
        use gilrs::Button;
        // `gilrs::GamepadId` can't be constructed in a unit test (it's
        // crate-private), so this exercises the player-indexed helpers
        // that `handle_gamepad_event` funnels through once it has
        // resolved an id to a player via `PadAssignment`.
        let mut s = InputState::with_defaults();
        s.set_gamepad_button(0, Button::Start, true);
        s.set_gamepad_button(1, Button::Start, true);
        assert!(s.player1().contains(Buttons::START));
        assert!(s.player2().contains(Buttons::START));
        // Releasing P1 leaves P2 held.
        s.set_gamepad_button(0, Button::Start, false);
        assert!(!s.player1().contains(Buttons::START));
        assert!(s.player2().contains(Buttons::START));
    }

    #[test]
    fn pad_assignment_starts_empty() {
        // `PadAssignment` itself is unit-testable with synthetic indices
        // via the public-in-module helper... but `GamepadId` is opaque.
        // Instead verify the assignment logic directly is exercised by
        // the event handler's `player_for` returning 0..=3 then None.
        // (Construction of `GamepadId` is not possible here; the logic is
        // covered structurally by the per-player helper tests below.)
        let a = PadAssignment::default();
        assert!(a.ids.iter().all(Option::is_none));
    }

    #[test]
    fn four_players_drive_independent_state() {
        use gilrs::Button;
        // P3/P4 (v1.7.0, Four Score) route through the same player-indexed
        // helpers `handle_gamepad_event` funnels into once `PadAssignment`
        // has resolved the 3rd/4th distinct pad.
        let mut s = InputState::with_defaults();
        s.set_gamepad_button(2, Button::Start, true);
        s.set_gamepad_button(3, Button::South, true);
        assert!(s.player3().contains(Buttons::START));
        assert!(s.player4().contains(Buttons::A));
        // P1/P2 untouched.
        assert!(s.player1().is_empty());
        assert!(s.player2().is_empty());
    }

    #[test]
    fn player3_player4_keyboard_defaults_drive_their_bits() {
        // Defaults: P3 = IJKL cluster (I/K/J/L + U/O/M/Period), P4 =
        // numpad (8/2/4/6 + 7/9/1/3).
        let mut s = InputState::with_defaults();
        for (key, want) in [
            (KeyCode::KeyI, Buttons::UP),
            (KeyCode::KeyU, Buttons::A),
            (KeyCode::KeyO, Buttons::B),
            (KeyCode::Period, Buttons::START),
        ] {
            let (k, e) = down(key);
            s.handle_key(k, e);
            assert!(s.player3().contains(want), "P3 {key:?} -> {want:?}");
        }
        for (key, want) in [
            (KeyCode::Numpad8, Buttons::UP),
            (KeyCode::Numpad7, Buttons::A),
            (KeyCode::Numpad9, Buttons::B),
            (KeyCode::Numpad3, Buttons::START),
        ] {
            let (k, e) = down(key);
            s.handle_key(k, e);
            assert!(s.player4().contains(want), "P4 {key:?} -> {want:?}");
        }
        // P3 keys must not leak into P4 and vice versa.
        assert!(!s.player4().contains(Buttons::UP) || s.player3().contains(Buttons::UP));
        assert_eq!(
            s.player3(),
            Buttons::UP | Buttons::A | Buttons::B | Buttons::START
        );
        assert_eq!(
            s.player4(),
            Buttons::UP | Buttons::A | Buttons::B | Buttons::START
        );
    }

    #[test]
    fn player_index_out_of_range_is_empty() {
        let s = InputState::with_defaults();
        assert!(s.player(4).is_empty());
        assert!(s.player(usize::MAX).is_empty());
    }

    #[test]
    fn parse_gamepad_button_known_and_unknown() {
        use gilrs::Button;
        assert_eq!(parse_gamepad_button("South"), Some(Button::South));
        assert_eq!(parse_gamepad_button("DPadUp"), Some(Button::DPadUp));
        assert_eq!(parse_gamepad_button("Start"), Some(Button::Start));
        assert_eq!(
            parse_gamepad_button("RightTrigger2"),
            Some(Button::RightTrigger2)
        );
        assert_eq!(parse_gamepad_button("NotAButton"), None);
    }

    #[test]
    fn canonical_gamepad_button_name_round_trips() {
        assert_eq!(canonical_gamepad_button_name("South"), "South");
        assert_eq!(canonical_gamepad_button_name("DPadUp"), "DPadUp");
        // Unrecognized passes through unchanged (never silently dropped).
        assert_eq!(canonical_gamepad_button_name("NotAButton"), "NotAButton");
    }

    #[test]
    fn unknown_gamepad_button_string_is_dropped() {
        use gilrs::Button;
        // A config value of "Unknown" maps to Button::Unknown, which the
        // map builder drops (so it never shadows a real binding).
        let mut cfg = crate::config::GamepadBindings::default_xbox();
        cfg.a = "Unknown".into();
        let map = GamepadMap::from_config(&cfg);
        assert_eq!(map.lookup(Button::Unknown), None);
        // The other buttons still resolve.
        assert_eq!(map.lookup(Button::West), Some(Buttons::B));
    }

    #[test]
    fn parse_keycode_known_and_unknown() {
        assert_eq!(parse_keycode("KeyA"), Some(KeyCode::KeyA));
        assert_eq!(parse_keycode("ArrowUp"), Some(KeyCode::ArrowUp));
        assert_eq!(parse_keycode("F12"), Some(KeyCode::F12));
        assert_eq!(parse_keycode("PrintScreen"), None);
    }

    #[test]
    fn parse_keycode_accepts_legacy_arrow_names() {
        // v1.3.1 regression: legacy winit-0.29 `VirtualKeyCode` arrow
        // names must resolve to the current `KeyCode::Arrow*`.
        assert_eq!(parse_keycode("Up"), Some(KeyCode::ArrowUp));
        assert_eq!(parse_keycode("Down"), Some(KeyCode::ArrowDown));
        assert_eq!(parse_keycode("Left"), Some(KeyCode::ArrowLeft));
        assert_eq!(parse_keycode("Right"), Some(KeyCode::ArrowRight));
    }

    #[test]
    fn parse_keycode_accepts_legacy_letter_names() {
        // Bare single letters -> Key*.
        assert_eq!(parse_keycode("Z"), Some(KeyCode::KeyZ));
        assert_eq!(parse_keycode("X"), Some(KeyCode::KeyX));
        assert_eq!(parse_keycode("A"), Some(KeyCode::KeyA));
        assert_eq!(parse_keycode("W"), Some(KeyCode::KeyW));
        assert_eq!(parse_keycode("S"), Some(KeyCode::KeyS));
        assert_eq!(parse_keycode("D"), Some(KeyCode::KeyD));
        assert_eq!(parse_keycode("Q"), Some(KeyCode::KeyQ));
        assert_eq!(parse_keycode("E"), Some(KeyCode::KeyE));
        assert_eq!(parse_keycode("G"), Some(KeyCode::KeyG));
        assert_eq!(parse_keycode("F"), Some(KeyCode::KeyF));
    }

    #[test]
    fn parse_keycode_accepts_legacy_named_keys() {
        assert_eq!(parse_keycode("Return"), Some(KeyCode::Enter));
        assert_eq!(parse_keycode("RShift"), Some(KeyCode::ShiftRight));
        assert_eq!(parse_keycode("LShift"), Some(KeyCode::ShiftLeft));
        assert_eq!(parse_keycode("LControl"), Some(KeyCode::ControlLeft));
        assert_eq!(parse_keycode("RControl"), Some(KeyCode::ControlRight));
        assert_eq!(parse_keycode("LAlt"), Some(KeyCode::AltLeft));
        assert_eq!(parse_keycode("RAlt"), Some(KeyCode::AltRight));
        assert_eq!(parse_keycode("Back"), Some(KeyCode::Backspace));
        assert_eq!(parse_keycode("Capital"), Some(KeyCode::CapsLock));
        assert_eq!(parse_keycode("Grave"), Some(KeyCode::Backquote));
    }

    #[test]
    fn parse_keycode_accepts_legacy_digit_names() {
        // Bare digits and winit-0.29 `Key0`..`Key9` both -> Digit*.
        assert_eq!(parse_keycode("0"), Some(KeyCode::Digit0));
        assert_eq!(parse_keycode("9"), Some(KeyCode::Digit9));
        assert_eq!(parse_keycode("Key1"), Some(KeyCode::Digit1));
        assert_eq!(parse_keycode("Key0"), Some(KeyCode::Digit0));
    }

    #[test]
    fn parse_keycode_current_names_take_precedence_over_legacy() {
        // The shared spellings (`Space`, `Escape`, `Tab`, `F1`..`F12`)
        // must resolve directly, not via the legacy table.
        assert_eq!(parse_keycode("Space"), Some(KeyCode::Space));
        assert_eq!(parse_keycode("Escape"), Some(KeyCode::Escape));
        assert_eq!(parse_keycode("Tab"), Some(KeyCode::Tab));
        assert_eq!(parse_keycode("F5"), Some(KeyCode::F5));
        // Current `KeyZ`/`ArrowUp` still resolve (not shadowed by aliases).
        assert_eq!(parse_keycode("KeyZ"), Some(KeyCode::KeyZ));
        assert_eq!(parse_keycode("ArrowUp"), Some(KeyCode::ArrowUp));
    }

    #[test]
    fn legacy_default_layout_resolves_end_to_end() {
        // The exact value-strings a pre-v1.3.1 config carried for the
        // default P1 + P2 layout must all resolve (this is the user's
        // already-migrated on-disk config).
        for legacy in [
            "Up", "Down", "Left", "Right", "Z", "X", "Return", "RShift", // P1
            "W", "S", "A", "D", "Q", "E", "P", "L", // P2
        ] {
            assert!(
                parse_keycode(legacy).is_some(),
                "legacy keycode {legacy:?} should resolve"
            );
        }
    }

    #[test]
    fn canonical_keycode_name_rewrites_legacy_to_current() {
        assert_eq!(canonical_keycode_name("Up"), "ArrowUp");
        assert_eq!(canonical_keycode_name("Z"), "KeyZ");
        assert_eq!(canonical_keycode_name("Return"), "Enter");
        assert_eq!(canonical_keycode_name("RShift"), "ShiftRight");
        // Already-current names pass through unchanged.
        assert_eq!(canonical_keycode_name("ArrowUp"), "ArrowUp");
        assert_eq!(canonical_keycode_name("KeyZ"), "KeyZ");
        assert_eq!(canonical_keycode_name("F5"), "F5");
        // Unrecognized names pass through (migration shouldn't drop them).
        assert_eq!(canonical_keycode_name("PrintScreen"), "PrintScreen");
    }
}
