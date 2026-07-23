//! User-editable runtime configuration.
//!
//! Per `to-dos/phase-5-frontend-tooling/sprint-2-save-rewind.md` T-52-006:
//! a TOML file under [`directories::ProjectDirs::config_dir()`] holds the
//! input bindings, rewind defaults, audio sample rate, etc. Missing keys
//! fall back to the bundled defaults; syntactically-invalid bytes log a
//! warning and the in-process config falls back to defaults too.
//!
//! Pre-v1.3.1 configs used a different schema (`[video] vsync`,
//! `[input.keyboard_p1]`, ...). Rather than silently discarding them,
//! [`Config::load_or_default`] detects the legacy schema, carries the
//! recognizable fields forward, backs up the original to `config.toml.bak`,
//! writes the upgraded file, and logs a loud (non-silent) summary. See
//! the private `Config::migrate_legacy` helper.
//!
//! The config file is read once at app startup. Writing it back is on
//! demand — the tabbed Settings window (`ui_shell.rs`) auto-saves the file
//! whenever the user changes a setting.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};

use directories::ProjectDirs;
use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Application identifier triple used by `directories` (qualifier, org, app).
const APP_QUALIFIER: &str = "dev";
const APP_ORG: &str = "DoubleGate";
const APP_NAME: &str = "RustyNES";

/// Errors raised by [`Config::load_from`] / [`Config::save`].
#[derive(Debug, Error)]
#[non_exhaustive]
pub enum ConfigError {
    /// I/O error reading or writing the config file.
    #[error("config I/O: {0}")]
    Io(#[from] io::Error),
    /// TOML parse error.
    #[error("config parse: {0}")]
    Parse(#[from] toml::de::Error),
    /// TOML serialization error.
    #[error("config serialize: {0}")]
    Serialize(#[from] toml::ser::Error),
}

/// Player-1 / player-2 input bindings, plus the system bindings (quit,
/// save state, load state, rewind, ...).
///
/// The `gamepad1` / `gamepad2` sections are `#[serde(default)]`, so a
/// pre-v1.6.0 config (which has no `[input.gamepad*]` tables) loads
/// unchanged and gets the default Xbox-style pad layout — byte-identical
/// behaviour for users who never open the rebind UI.
///
/// The `player3` / `player4`, `gamepad3` / `gamepad4`, and `four_score`
/// fields (v1.7.0) are likewise `#[serde(default)]`, so a pre-v1.7.0
/// config (no Four Score tables / flag) loads unchanged: `four_score`
/// defaults off and the P3/P4 maps stay dormant until the toggle is
/// enabled.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct InputConfig {
    /// Player 1 keyboard mapping.
    pub player1: PadBindings,
    /// Player 2 keyboard mapping.
    pub player2: PadBindings,
    /// Player 3 keyboard mapping (v1.7.0, Four Score). `#[serde(default)]`
    /// so older configs without an `[input.player3]` section get the
    /// default IJKL-cluster layout.
    #[serde(default = "PadBindings::default_player3")]
    pub player3: PadBindings,
    /// Player 4 keyboard mapping (v1.7.0, Four Score). `#[serde(default)]`
    /// so older configs without an `[input.player4]` section get the
    /// default numpad layout.
    #[serde(default = "PadBindings::default_player4")]
    pub player4: PadBindings,
    /// Player 1 gamepad mapping (v1.6.0). `#[serde(default)]` so older
    /// configs without an `[input.gamepad1]` section keep the legacy
    /// hardcoded Xbox layout.
    #[serde(default = "GamepadBindings::default_xbox")]
    pub gamepad1: GamepadBindings,
    /// Player 2 gamepad mapping (v1.6.0). Defaults to the same Xbox
    /// layout as player 1 — gilrs routes per physical device, so two
    /// pads with the same logical map work fine; the layout only
    /// matters once the player rebinds a button.
    #[serde(default = "GamepadBindings::default_xbox")]
    pub gamepad2: GamepadBindings,
    /// Player 3 gamepad mapping (v1.7.0, Four Score). Same default Xbox
    /// layout — gilrs routes per physical device, so the third distinct
    /// pad drives player 3.
    #[serde(default = "GamepadBindings::default_xbox")]
    pub gamepad3: GamepadBindings,
    /// Player 4 gamepad mapping (v1.7.0, Four Score). Same default Xbox
    /// layout; the fourth distinct pad drives player 4.
    #[serde(default = "GamepadBindings::default_xbox")]
    pub gamepad4: GamepadBindings,
    /// Whether the Four Score 4-player adapter is enabled (v1.7.0).
    /// `#[serde(default)]` (= `false`), so a pre-v1.7.0 config loads with
    /// the adapter off — `$4016`/`$4017` reads stay byte-identical to two
    /// controllers until the user ticks the toggle in the rebind UI.
    #[serde(default)]
    pub four_score: bool,
    /// Non-standard expansion device on the player-2 port (`$4017`) (v2.1.0).
    /// `#[serde(default)]` (= [`ExpansionDevice::None`]), so a pre-v2.1.0
    /// config loads with no device — `$4017` reads stay byte-identical to a
    /// standard controller until the user selects a device in the menu.
    #[serde(default)]
    pub expansion_device: ExpansionDevice,
    /// v2.8.0 Phase 3 — run-ahead depth (0-3, default 1): each visible
    /// frame, the emulator runs this many extra frames ahead with the
    /// freshly latched input and shows the future frame, removing the
    /// game's own internal input lag (most NES titles buffer input >= 1
    /// frame). The persistent timeline stays byte-identical to a plain
    /// run; auto-disabled during movies/netplay and budget-throttled on
    /// slow hosts. Native-only.
    #[serde(default = "default_run_ahead")]
    pub run_ahead: u32,
    /// v1.1.0 beta.1 (T-110-B2) — turbo/autofire on the A button: while held, A
    /// rapid-fires. Off by default (`false`) = byte-identical input. Applied
    /// where input meets the NES, keyed on the emulated frame number, so it is
    /// deterministic and rollback / TAS / netplay-safe.
    #[serde(default)]
    pub turbo_a: bool,
    /// v1.1.0 beta.1 (T-110-B2) — turbo/autofire on the B button (see
    /// [`Self::turbo_a`]).
    #[serde(default)]
    pub turbo_b: bool,
    /// v1.1.0 beta.1 (T-110-B2) — frames the turbo button holds each on/off
    /// state (clamped to >= 1; default 2, ≈ 15 Hz at 60 fps). Lower = faster.
    #[serde(default = "default_turbo_period")]
    pub turbo_period: u32,
    /// v1.5.0 "Lens" Workstream D4 — SNES-mouse reported sensitivity (0 = low,
    /// 1 = medium, 2 = high). This is the 2-bit field the mouse sends in its
    /// serial report (some titles read + cycle it). Default `0` (low) matches
    /// the previous hardcoded value, so the deterministic device report is
    /// byte-identical to a pre-D4 config. Clamped to `0..=2` on use.
    #[serde(default)]
    pub mouse_sensitivity: u8,
    /// v1.5.0 "Lens" Workstream D4 — frontend DPI multiplier applied to the
    /// host mouse motion BEFORE it is clamped + handed to the SNES mouse /
    /// Vaus paddle. `1.0` (default) is the previous 1:1 mapping, byte-identical.
    /// A larger value moves the pointer device faster per host-mouse pixel.
    /// Clamped to `0.1..=8.0` on use.
    #[serde(default = "default_pointer_scale")]
    pub pointer_scale: f32,
    /// v1.5.0 "Lens" Workstream D4 — Power Pad / Family Trainer mat layout
    /// variant. The NES Power Pad has two labelled sides (the "A" 12-button
    /// grid and the mirrored "B" side); selecting [`PowerPadLayout::SideB`]
    /// remaps the host-key mat mask to the B-side button numbering. Default
    /// [`PowerPadLayout::SideA`] is the previous fixed mapping (byte-identical).
    #[serde(default)]
    pub power_pad_layout: PowerPadLayout,
    /// System-level bindings.
    pub system: SystemBindings,
}

/// Serde default for [`InputConfig::pointer_scale`].
const fn default_pointer_scale() -> f32 {
    1.0
}

/// v1.5.0 "Lens" Workstream D4 — NES Power Pad / Family Trainer mat layout side.
///
/// The physical mat is labelled with an "A" side and a "B" side whose button
/// numbering is the left-right mirror of side A; some games expect one or the
/// other. The remap is a pure presentation/input mapping of the 12-bit mat
/// mask — no core change, and side A is byte-identical to the prior fixed
/// mapping.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum PowerPadLayout {
    /// Side A — the default 12-button grid (the previous fixed mapping).
    #[default]
    SideA,
    /// Side B — the left-right mirrored button numbering.
    SideB,
}

impl PowerPadLayout {
    /// Human label for the settings combo.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::SideA => "Side A (default)",
            Self::SideB => "Side B (mirrored)",
        }
    }

    /// Remap a side-A 12-bit mat mask (bit `i` = mat button `i+1`) for this
    /// layout. Side A is the identity; side B mirrors each of the three rows
    /// left-to-right (the four columns `0 1 2 3` become `3 2 1 0`). The mat is
    /// a 3-row × 4-column grid, so the per-row mirror is `row*4 + (3 - col)`.
    #[must_use]
    pub const fn remap_mask(self, mask: u16) -> u16 {
        match self {
            Self::SideA => mask,
            Self::SideB => {
                let mut out: u16 = 0;
                let mut i = 0usize;
                while i < 12 {
                    if mask & (1 << i) != 0 {
                        let row = i / 4;
                        let col = i % 4;
                        let dst = row * 4 + (3 - col);
                        out |= 1 << dst;
                    }
                    i += 1;
                }
                out
            }
        }
    }
}

/// Serde default for [`InputConfig::run_ahead`].
const fn default_run_ahead() -> u32 {
    1
}

/// Serde default for [`InputConfig::turbo_period`].
const fn default_turbo_period() -> u32 {
    2
}

/// Famicom Disk System (FDS) configuration (v2.2.0).
///
/// All fields are `#[serde(default)]`, so a pre-v2.2.0 config (with no
/// `[fds]` section) loads unchanged and behaves exactly as before — FDS
/// support never touches the standard cartridge (`.nes`) load path.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct FdsConfig {
    /// Filesystem path to the user-supplied 8 KiB FDS BIOS (`disksys.rom`).
    ///
    /// Nintendo IP — NEVER committed to this repo. When a `.fds` image is
    /// loaded and this is unset (or points at a missing / wrong-size file),
    /// the frontend prompts for it once via an `rfd` file dialog and persists
    /// the chosen path here. Native-only (no filesystem on wasm32).
    #[serde(default)]
    pub bios_path: Option<PathBuf>,
}

/// Netplay (v2.3.0) configuration — only the last-used host port + join
/// address are persisted, as conveniences pre-filled into the netplay panel.
///
/// All fields are `#[serde(default)]`, so a pre-v2.3.0 config (with no
/// `[netplay]` section) loads unchanged. Netplay is native-only (it drives a
/// UDP socket via `std::net`); the section is harmless on wasm32 (where the
/// netplay panel is a "native-only" note), so it stays in the shared `Config`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct NetplayConfig {
    /// Local UDP port the host binds (default 7000). Pre-filled into the
    /// netplay panel's "Host" port field.
    #[serde(default = "default_netplay_port")]
    pub host_port: u16,
    /// Last `host:port` typed into the netplay panel's "Join" field. Empty by
    /// default; persisted so a re-launch pre-fills the last peer.
    #[serde(default)]
    pub last_join_address: String,
    /// Number of players a host starts the session with (2..=4; v2.5.0). 3-4
    /// players use the Four Score adapter. Clamped into `2..=4` on use.
    /// Defaults to 2 so a pre-v2.5.0 config loads byte-identically.
    #[serde(default = "default_netplay_players")]
    pub num_players: u8,
    /// v2.7.0 — the **browser** netplay signaling-server URL (a `wss://...`
    /// WebSocket the wasm build connects to for the WebRTC offer/answer/ICE
    /// handshake). Empty by default (the user fills it in the wasm lobby, or sets
    /// it here). Native netplay ignores this (it uses UDP directly). See
    /// `docs/netplay-webrtc.md` + the `deploy/` bundle for hosting one.
    #[serde(default)]
    pub signaling_url: String,
    /// v2.7.0 — the ICE / STUN servers the **browser** WebRTC peer connection
    /// uses for NAT traversal. Defaults to the public list
    /// ([`rustynes_netplay::DEFAULT_STUN_SERVERS`]); a production deployment points
    /// these at its own `coturn` (STUN + TURN). Native netplay ignores this.
    #[serde(default = "default_stun_servers")]
    pub stun_servers: Vec<String>,
}

const fn default_netplay_port() -> u16 {
    7000
}

const fn default_netplay_players() -> u8 {
    2
}

fn default_stun_servers() -> Vec<String> {
    rustynes_netplay::DEFAULT_STUN_SERVERS
        .iter()
        .map(|s| (*s).to_string())
        .collect()
}

impl Default for NetplayConfig {
    fn default() -> Self {
        Self {
            host_port: default_netplay_port(),
            last_join_address: String::new(),
            num_players: default_netplay_players(),
            signaling_url: String::new(),
            stun_servers: default_stun_servers(),
        }
    }
}

/// The non-standard input device attached to the player-2 port (`$4017`).
///
/// Mutually exclusive with the standard controller on that port (and with
/// the Four Score, which the real hardware also does not support alongside
/// these devices).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum ExpansionDevice {
    /// Standard controller (no overlay device). The default.
    #[default]
    None,
    /// NES Zapper light gun.
    Zapper,
    /// Arkanoid "Vaus" paddle.
    Vaus,
    /// NES Power Pad / Family Fun Fitness mat (12 buttons; keys per
    /// `input::POWER_PAD_KEYS`).
    PowerPad,
    /// SNES-style serial mouse (v1.2.0 Workstream D). Mouse motion drives the
    /// per-frame movement deltas; left/right mouse buttons map to the device
    /// buttons.
    SnesMouse,
    /// Famicom Family BASIC keyboard (v1.2.0 Workstream D). Host keys map to the
    /// 72-key matrix via `input::family_keyboard_index`.
    FamilyKeyboard,
    /// Bandai Family Trainer mat (v1.3.0 Workstream F1) — layout-equivalent to
    /// the Power Pad; reuses the same 12-button matrix scan (keys per
    /// `input::POWER_PAD_KEYS`).
    FamilyTrainer,
    /// Subor keyboard (v1.3.0 Workstream F1) — a Family BASIC keyboard
    /// work-alike; reuses the same 72-key matrix (keys per
    /// `input::family_keyboard_index`).
    SuborKeyboard,
    /// Konami Hyper Shot (v1.3.0 Workstream F1) — a 4-button (2-player Run/Jump)
    /// expansion controller (keys per `input::KONAMI_HYPER_SHOT_KEYS`).
    KonamiHyperShot,
    /// Bandai Hyper Shot / Exciting Boxing punching bag (v1.3.0 Workstream F1) —
    /// an 8-sensor expansion controller (keys per
    /// `input::BANDAI_HYPER_SHOT_KEYS`).
    BandaiHyperShot,
}

impl Default for InputConfig {
    fn default() -> Self {
        Self {
            player1: PadBindings::default_player1(),
            player2: PadBindings::default_player2(),
            player3: PadBindings::default_player3(),
            player4: PadBindings::default_player4(),
            gamepad1: GamepadBindings::default_xbox(),
            gamepad2: GamepadBindings::default_xbox(),
            gamepad3: GamepadBindings::default_xbox(),
            gamepad4: GamepadBindings::default_xbox(),
            four_score: false,
            expansion_device: ExpansionDevice::None,
            run_ahead: default_run_ahead(),
            turbo_a: false,
            turbo_b: false,
            turbo_period: default_turbo_period(),
            mouse_sensitivity: 0,
            pointer_scale: default_pointer_scale(),
            power_pad_layout: PowerPadLayout::default(),
            system: SystemBindings::default(),
        }
    }
}

/// Per-pad keyboard mapping.
///
/// Each value is a winit `KeyCode` name as a string (matches the
/// `Debug` representation of `winit::keyboard::KeyCode`, which is
/// stable). See [`crate::input::parse_keycode`] for the lookup.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PadBindings {
    /// D-pad up.
    pub up: String,
    /// D-pad down.
    pub down: String,
    /// D-pad left.
    pub left: String,
    /// D-pad right.
    pub right: String,
    /// A button.
    pub a: String,
    /// B button.
    pub b: String,
    /// Select.
    pub select: String,
    /// Start.
    pub start: String,
}

impl PadBindings {
    /// Reasonable player-1 defaults (arrows + Z/X + `RShift` + Enter).
    #[must_use]
    pub fn default_player1() -> Self {
        Self {
            up: "ArrowUp".into(),
            down: "ArrowDown".into(),
            left: "ArrowLeft".into(),
            right: "ArrowRight".into(),
            a: "KeyZ".into(),
            b: "KeyX".into(),
            select: "ShiftRight".into(),
            start: "Enter".into(),
        }
    }

    /// Player-2 defaults (WASD + Q/E + L/P).
    #[must_use]
    pub fn default_player2() -> Self {
        Self {
            up: "KeyW".into(),
            down: "KeyS".into(),
            left: "KeyA".into(),
            right: "KeyD".into(),
            a: "KeyQ".into(),
            b: "KeyE".into(),
            select: "KeyL".into(),
            start: "KeyP".into(),
        }
    }

    /// Player-3 defaults (v1.7.0, Four Score): the IJKL cluster +
    /// surrounding keys — I/K/J/L = D-pad, U = A, O = B, M = Select,
    /// `Period` = Start. Chosen to avoid clashing with the P1 (arrows +
    /// Z/X) and P2 (WASD + Q/E) layouts on a single keyboard.
    #[must_use]
    pub fn default_player3() -> Self {
        Self {
            up: "KeyI".into(),
            down: "KeyK".into(),
            left: "KeyJ".into(),
            right: "KeyL".into(),
            a: "KeyU".into(),
            b: "KeyO".into(),
            select: "KeyM".into(),
            start: "Period".into(),
        }
    }

    /// Player-4 defaults (v1.7.0, Four Score): the numpad — 8/2/4/6 =
    /// D-pad, 7 = A, 9 = B, 1 = Select, 3 = Start. Non-conflicting with
    /// the P1/P2/P3 layouts.
    #[must_use]
    pub fn default_player4() -> Self {
        Self {
            up: "Numpad8".into(),
            down: "Numpad2".into(),
            left: "Numpad4".into(),
            right: "Numpad6".into(),
            a: "Numpad7".into(),
            b: "Numpad9".into(),
            select: "Numpad1".into(),
            start: "Numpad3".into(),
        }
    }
}

/// Per-pad gamepad mapping (v1.6.0).
///
/// Each face/d-pad value is a `gilrs::Button` name as a string (matches
/// the `Debug` representation of `gilrs::Button`, e.g. `"South"`,
/// `"DPadUp"`, `"Start"`). See [`crate::input::parse_gamepad_button`]
/// for the lookup. The analog-stick D-pad emulation reads
/// [`Self::axis_deadzone`].
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GamepadBindings {
    /// D-pad up.
    pub up: String,
    /// D-pad down.
    pub down: String,
    /// D-pad left.
    pub left: String,
    /// D-pad right.
    pub right: String,
    /// A button.
    pub a: String,
    /// B button.
    pub b: String,
    /// Select.
    pub select: String,
    /// Start.
    pub start: String,
    /// Left-analog-stick deflection (absolute value, 0.0..=1.0) past
    /// which the stick is treated as a D-pad press. Defaults to 0.5.
    #[serde(default = "default_axis_deadzone")]
    pub axis_deadzone: f32,
}

const fn default_axis_deadzone() -> f32 {
    0.5
}

impl GamepadBindings {
    /// The legacy hardcoded Xbox-style layout (South=A, West=B,
    /// Start=Start, Select=Back/Select, `DPad`=D-pad). This is the serde
    /// default for both pads, so a config with no `[input.gamepad*]`
    /// section reproduces the pre-v1.6.0 behaviour exactly.
    #[must_use]
    pub fn default_xbox() -> Self {
        Self {
            up: "DPadUp".into(),
            down: "DPadDown".into(),
            left: "DPadLeft".into(),
            right: "DPadRight".into(),
            a: "South".into(),
            b: "West".into(),
            select: "Select".into(),
            start: "Start".into(),
            axis_deadzone: default_axis_deadzone(),
        }
    }
}

impl Default for GamepadBindings {
    fn default() -> Self {
        Self::default_xbox()
    }
}

/// System bindings (non-pad).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct SystemBindings {
    /// Quit (closes window cleanly).
    pub quit: String,
    /// Save state to the most-recent slot (slot 0).
    pub save_state: String,
    /// Load state from the most-recent slot (slot 0).
    pub load_state: String,
    /// Hold to rewind frame-by-frame.
    pub rewind: String,
    /// Hard reset (warm boot).
    pub reset: String,
    /// Power cycle (cold boot).
    pub power_cycle: String,
    /// Toggle the egui debug overlay (defaults to `~`, i.e. `Backquote`).
    #[serde(default = "default_debug_overlay")]
    pub debug_overlay: String,
    /// Open the rfd file-picker to load a different ROM (default `F12`).
    #[serde(default = "default_open_rom")]
    pub open_rom: String,
    /// Toggle TAS movie recording (default `F6`).
    #[serde(default = "default_movie_record")]
    pub movie_record: String,
    /// Toggle TAS movie playback (default `F7`).
    #[serde(default = "default_movie_play")]
    pub movie_play: String,
    /// Branch the current playback into a new recording (default `F8`).
    #[serde(default = "default_movie_branch")]
    pub movie_branch: String,
    /// Cycle the inserted Famicom Disk System disk side (default `F9`):
    /// eject -> side 1 -> side 2 -> ... -> wrap. Only active when an FDS
    /// game is loaded. `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_disk_swap")]
    pub disk_swap: String,
    /// Insert a Vs. System coin (default `F10`). Only active when a Vs. System
    /// game is loaded. `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_insert_coin")]
    pub insert_coin: String,
    /// v1.0.0 (BUG-2) — toggle borderless fullscreen (default `F11`).
    /// `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_fullscreen")]
    pub fullscreen: String,
    /// v1.0.0 — toggle the always-on menu bar (default `KeyM`). The keyboard
    /// path back to the menu bar after hiding it from the View menu.
    /// `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_toggle_menu_bar")]
    pub toggle_menu_bar: String,
    /// Hold to fast-forward (run unthrottled, audio muted) (default `Tab`).
    /// `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_fast_forward")]
    pub fast_forward: String,
    /// Step one frame while paused (default `Backslash`).
    /// `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_frame_advance")]
    pub frame_advance: String,
    /// v1.0.0 (UX3 BUG-1) — toggle pause/resume (default `Space`).
    /// `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_pause")]
    pub pause: String,
    /// v1.0.0 — step the emulation speed up one preset (default `Equal`).
    /// `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_speed_up")]
    pub speed_up: String,
    /// v1.0.0 — step the emulation speed down one preset (default `Minus`).
    /// `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_speed_down")]
    pub speed_down: String,
    /// v1.0.0 — reset the emulation speed to 100% (default `Digit0`).
    /// `#[serde(default)]` so older configs keep loading.
    #[serde(default = "default_speed_reset")]
    pub speed_reset: String,
}

fn default_debug_overlay() -> String {
    "Backquote".into()
}

fn default_open_rom() -> String {
    "F12".into()
}

fn default_movie_record() -> String {
    "F6".into()
}

fn default_movie_play() -> String {
    "F7".into()
}

fn default_movie_branch() -> String {
    "F8".into()
}

fn default_disk_swap() -> String {
    "F9".into()
}

fn default_insert_coin() -> String {
    "F10".into()
}

fn default_fullscreen() -> String {
    "F11".into()
}

fn default_toggle_menu_bar() -> String {
    "KeyM".into()
}

fn default_fast_forward() -> String {
    "Tab".into()
}

fn default_frame_advance() -> String {
    "Backslash".into()
}

fn default_pause() -> String {
    "Space".into()
}

fn default_speed_up() -> String {
    "Equal".into()
}

fn default_speed_down() -> String {
    "Minus".into()
}

fn default_speed_reset() -> String {
    "Digit0".into()
}

impl Default for SystemBindings {
    fn default() -> Self {
        Self {
            quit: "Escape".into(),
            save_state: "F1".into(),
            load_state: "F4".into(),
            rewind: "F5".into(),
            reset: "F2".into(),
            power_cycle: "F3".into(),
            debug_overlay: default_debug_overlay(),
            open_rom: default_open_rom(),
            movie_record: default_movie_record(),
            movie_play: default_movie_play(),
            movie_branch: default_movie_branch(),
            disk_swap: default_disk_swap(),
            insert_coin: default_insert_coin(),
            fullscreen: default_fullscreen(),
            toggle_menu_bar: default_toggle_menu_bar(),
            fast_forward: default_fast_forward(),
            frame_advance: default_frame_advance(),
            pause: default_pause(),
            speed_up: default_speed_up(),
            speed_down: default_speed_down(),
            speed_reset: default_speed_reset(),
        }
    }
}

/// Rewind capture configuration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
pub struct RewindConfig {
    /// Whether the rewind ring is enabled at startup.
    pub enabled: bool,
    /// Rewind window in seconds.
    pub max_seconds: u32,
    /// Keyframe period in frames.
    pub keyframe_period: u32,
}

impl Default for RewindConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            max_seconds: 60,
            keyframe_period: 60,
        }
    }
}

/// Graphics configuration.
// `crt_scanline` is an `f32`, so this config is `PartialEq` only (not `Eq`).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct GraphicsConfig {
    /// wgpu present mode: `"Mailbox"` (default), `"Fifo"`, or
    /// `"Immediate"`. The native frontend paces frames on a wall clock,
    /// so `"Mailbox"` (no vsync gate) avoids the double-pacing beat that
    /// `"Fifo"` (vsync) produces against the display refresh. Falls back
    /// to `"Fifo"` automatically when the backend lacks the requested
    /// mode.
    pub present_mode: String,
    /// NTSC filter setting: `"off"`, `"composite"`, `"rgb"`, or
    /// `"composite-rt"`. Defaults to `"off"`. `"composite"` / `"rgb"` run the
    /// simplified Blargg-style blur as a wgsl post-pass between the PPU
    /// framebuffer texture and the letterbox blit. `"composite-rt"` (T-110-A1)
    /// runs the true composite `NES_NTSC` filter (Bisqwit algorithm): it
    /// reconstructs the analog signal from the PPU's palette-index framebuffer
    /// and demodulates it back to RGB, for genuine dot-crawl / fringing
    /// artifacts. All are presentation-only (no core / accuracy impact).
    #[serde(default = "default_ntsc_filter")]
    pub ntsc_filter: String,
    /// v2.8.0 Phase 2 — frame-pacing regime (the canonical display-sync
    /// matrix; native only, the wasm rAF loop has its own pacing):
    ///
    /// - `"auto"` (default): when the monitor's refresh rate is within 0.5%
    ///   of the ROM's nominal rate (NTSC 60.0988 / PAL 50.007), sync to the
    ///   display (`"display"` behavior); otherwise fall back to
    ///   `"wallclock"`.
    /// - `"display"`: Fifo (vsync) is the clock — exactly one emulated
    ///   frame per display refresh; the tiny speed bend (≤0.5%) is
    ///   invisible and the audio DRC absorbs the rate difference. Zero
    ///   judder on fixed ~60 Hz panels.
    /// - `"vrr"`: for G-Sync/FreeSync displays — Fifo + the wall-clock
    ///   pacer at the exact console rate; the variable-refresh display
    ///   follows the emulator. Best used fullscreen (compositors generally
    ///   engage VRR only for fullscreen surfaces).
    /// - `"wallclock"`: the pre-v2.8.0 behavior — wall-clock pacer +
    ///   the configured present mode (Mailbox default). Right for
    ///   high-refresh fixed panels (120/144 Hz) without VRR.
    #[serde(default = "default_pacing_mode")]
    pub pacing_mode: String,
    /// v2.8.0 Phase 2 — swapchain `desired_maximum_frame_latency` (1 or 2).
    /// 1 = lowest display latency, 2 (default) = more scheduling slack.
    #[serde(default = "default_max_frame_latency")]
    pub max_frame_latency: u32,
    /// v1.0.0 — crop the top + bottom 8 NES scanlines (the CRT-overscan
    /// region many games leave noisy / scrolling-garbage in). Default
    /// `false` so the default presentation is byte-identical (the full
    /// 256x240 framebuffer is blitted). When on, the letterbox blit samples
    /// only the inner 256x224 source rect — a presentation-layer UV change,
    /// no core / framebuffer change.
    #[serde(default)]
    pub hide_overscan: bool,
    /// v1.1.0 beta.1 — CRT / scanline post-process pass. Default `false` (the
    /// presentation is byte-identical when off). Mutually exclusive with the NTSC
    /// filter at render time (CRT wins when both are set). A presentation-layer
    /// wgsl pass; no core / framebuffer change.
    #[serde(default)]
    pub crt_filter: bool,
    /// v1.1.0 beta.1 — CRT scanline intensity (`0.0` = none .. `1.0` = strong),
    /// applied live. Default `0.5`.
    #[serde(default = "default_crt_scanline")]
    pub crt_scanline: f32,
    /// v1.1.0 beta.1 (T-110-A3) — path to a loaded `.pal` palette file (64-entry,
    /// 192-byte form; longer files use the first 64 colours). `None` (default) =
    /// the built-in palette, byte-identical presentation. A custom palette
    /// re-tints the displayed framebuffer only — no core / accuracy impact.
    #[serde(default)]
    pub palette_file: Option<std::path::PathBuf>,
    /// v1.2.0 C1 — live contrast knob for the true-composite (`"composite-rt"`,
    /// Bisqwit) NTSC filter. Picture contrast factor is `(contrast + 1)^2`.
    /// Default `0.0` ([`crate::ntsc_bisqwit::NtscKnobs::DEFAULT`]) is byte-identical
    /// to the previous hardcoded value. Output-only; no core / accuracy impact.
    #[serde(default = "default_ntsc_contrast")]
    pub ntsc_contrast: f32,
    /// v1.2.0 C1 — live saturation knob for the Bisqwit NTSC filter. Chroma gain
    /// factor is `(saturation + 1)^2`. Default `0.0` = byte-identical.
    #[serde(default = "default_ntsc_saturation")]
    pub ntsc_saturation: f32,
    /// v1.2.0 C1 — live brightness knob for the Bisqwit NTSC filter (additive luma
    /// offset). Default `0.0` = byte-identical.
    #[serde(default = "default_ntsc_brightness")]
    pub ntsc_brightness: f32,
    /// v1.2.0 C1 — live hue knob (degrees) for the Bisqwit NTSC filter, applied as
    /// a rotation of the demodulated (I, Q) vector. Default `0.0` = byte-identical.
    #[serde(default = "default_ntsc_hue")]
    pub ntsc_hue: f32,
    /// v1.2.0 C2 — the composable post-process shader stack. `#[serde(default)]`
    /// = an EMPTY stack, so a pre-C2 config (with no `[graphics.shader_stack]`
    /// key) loads byte-identically and the renderer takes the unchanged
    /// direct-blit / legacy-filter path. A non-empty stack engages the ping-pong
    /// shader executor. Presentation-only; no core / accuracy impact.
    #[serde(default)]
    pub shader_stack: crate::shader_pass::ShaderStackConfig,
    /// v1.2.0 C2 — saved named shader-stack presets (the CRT preset bank +
    /// user-saved stacks). `#[serde(default)]` = empty, so a pre-C2 config is
    /// byte-identical. Persisted under `[graphics.shader_presets]`.
    #[serde(default)]
    pub shader_presets: crate::shader_pass::ShaderPresetBank,
    /// v1.2.0 beta.2 (Workstream C3) — per-game HD-pack paths, keyed on the
    /// ROM SHA-256 (hex). When the loaded ROM's hash has an entry here AND the
    /// `hd-pack` feature is built in, the frontend loads the referenced pack
    /// (folder or `.zip`) and substitutes hi-res tiles at blit time. Empty by
    /// default and `#[serde(default)]`, so a pre-C3 config is byte-identical
    /// and the default presentation is unchanged. Presentation-only.
    #[serde(default)]
    pub hd_packs: std::collections::BTreeMap<String, std::path::PathBuf>,
    /// v1.5.0 "Lens" Workstream D1 — per-side overscan crop, in NES pixels. The
    /// legacy [`Self::hide_overscan`] toggle is the equivalent of an
    /// `8 px top + 8 px bottom` crop; this finer control lets the user trim each
    /// edge independently (WYSIWYG, live). Default `(0,0,0,0)` (the full
    /// 256x240 framebuffer) is byte-identical to today's presentation when
    /// `hide_overscan` is also off. Presentation-only; no core change.
    #[serde(default)]
    pub overscan: Overscan,
    /// v1.5.0 "Lens" Workstream D1 — named custom palette bank. Each entry is a
    /// user-saved 64-colour base palette (edited in the in-app palette editor or
    /// imported from a `.pal`). `#[serde(default)]` = empty, so a pre-D1 config
    /// loads byte-identically. The selected palette (if any) is named by
    /// [`Self::active_palette`]; an unselected / missing name uses the built-in
    /// palette. Presentation-only (re-tints the displayed framebuffer).
    #[serde(default)]
    pub palettes: PaletteBank,
    /// v1.5.0 "Lens" Workstream D1 — the name of the active entry in
    /// [`Self::palettes`], or `None` for the built-in palette (or the legacy
    /// [`Self::palette_file`]). `#[serde(default)]` = `None`, byte-identical.
    #[serde(default)]
    pub active_palette: Option<String>,
    /// v2.1.2 "Fathom" F1.4 — when `true`, the PPU base palette is *generated*
    /// from the composite waveform model (`rustynes_ppu::generate_base_palette`,
    /// tuned by [`Self::ntsc_palette`]) instead of the hand-authored built-in.
    /// It takes precedence over the named bank + legacy `.pal`. **Default
    /// `false`**, so the shipped presentation keeps the built-in palette and is
    /// byte-identical (the generated palette changes framebuffer output, hence
    /// it is opt-in). Presentation-only; no core / accuracy impact.
    #[serde(default)]
    pub ntsc_palette_enabled: bool,
    /// v2.1.2 "Fathom" F1.4 — parameters for the generated NTSC palette (used
    /// only when [`Self::ntsc_palette_enabled`]). `#[serde(default)]` = the
    /// neutral calibration, so a pre-F1.4 config loads unchanged.
    #[serde(default)]
    pub ntsc_palette: NtscPaletteConfig,
    /// v2.1.2 "Fathom" F2.1 — Vs. `DualSystem` two-screen layout: `"side-by-side"`
    /// (default, `512x240`) or `"stacked"` (`256x480`). Only used while a
    /// `DualSystem` cabinet is loaded; irrelevant to single-console presentation.
    #[serde(default = "default_dual_screen_layout")]
    pub dual_screen_layout: String,
}

fn default_dual_screen_layout() -> String {
    "side-by-side".to_string()
}

/// v2.1.2 "Fathom" F1.4 — serializable mirror of `rustynes_ppu::NtscPaletteParams`.
///
/// A config can't reference the core type directly and stay `Deserialize`-stable,
/// so this mirrors its fields; [`Self::to_params`] converts to the core type
/// consumed by `generate_base_palette`. `f64` fields, so `PartialEq` only.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq)]
pub struct NtscPaletteConfig {
    /// Chroma gain (`1.0` neutral, `0.0` grayscale).
    #[serde(default = "default_ntsc_pal_saturation")]
    pub saturation: f64,
    /// Global hue rotation in subcarrier-phase units (1 unit = 30°); `0.0` std.
    #[serde(default)]
    pub hue: f64,
    /// Luma contrast about mid-gray (`1.0` neutral).
    #[serde(default = "default_ntsc_pal_contrast")]
    pub contrast: f64,
    /// Overall luma gain (`1.0` neutral).
    #[serde(default = "default_ntsc_pal_brightness")]
    pub brightness: f64,
    /// Display gamma for the `f^(2.2/gamma)` correction (`1.8` default).
    #[serde(default = "default_ntsc_pal_gamma")]
    pub gamma: f64,
}

const fn default_ntsc_pal_saturation() -> f64 {
    1.0
}
const fn default_ntsc_pal_contrast() -> f64 {
    1.0
}
const fn default_ntsc_pal_brightness() -> f64 {
    1.0
}
const fn default_ntsc_pal_gamma() -> f64 {
    1.8
}

impl Default for NtscPaletteConfig {
    fn default() -> Self {
        Self {
            saturation: default_ntsc_pal_saturation(),
            hue: 0.0,
            contrast: default_ntsc_pal_contrast(),
            brightness: default_ntsc_pal_brightness(),
            gamma: default_ntsc_pal_gamma(),
        }
    }
}

impl NtscPaletteConfig {
    /// Convert to the core synthesizer's parameter type.
    #[must_use]
    pub const fn to_params(self) -> rustynes_core::rustynes_ppu::NtscPaletteParams {
        rustynes_core::rustynes_ppu::NtscPaletteParams {
            saturation: self.saturation,
            hue: self.hue,
            contrast: self.contrast,
            brightness: self.brightness,
            gamma: self.gamma,
        }
    }
}

/// v1.5.0 "Lens" Workstream D1 — per-side overscan crop in NES pixels.
///
/// The cropped image samples the inner `(256 - left - right) x (240 - top -
/// bottom)` source rect; the renderer letterboxes it to keep the aspect. All
/// zero (the default) is byte-identical to the uncropped presentation. The
/// per-side values are clamped so the visible region never collapses.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct Overscan {
    /// Scanlines cropped from the top edge (0..=112).
    #[serde(default)]
    pub top: u8,
    /// Scanlines cropped from the bottom edge (0..=112).
    #[serde(default)]
    pub bottom: u8,
    /// Columns cropped from the left edge (0..=120).
    #[serde(default)]
    pub left: u8,
    /// Columns cropped from the right edge (0..=120).
    #[serde(default)]
    pub right: u8,
}

impl Overscan {
    /// Clamp each side so at least 16 px of width + height always remain
    /// visible (mirrors the renderer's guard). Returns a copy.
    #[must_use]
    pub fn clamped(self) -> Self {
        // Capping each side independently already guarantees at least 16 px of
        // width + height remain visible (top + bottom <= 224 <= 240 - 16 and
        // left + right <= 240 <= 256 - 16), so no proportional trim is needed.
        Self {
            top: self.top.min(112),
            bottom: self.bottom.min(112),
            left: self.left.min(120),
            right: self.right.min(120),
        }
    }

    /// `true` when no side is cropped (the byte-identical default).
    #[must_use]
    pub const fn is_zero(self) -> bool {
        self.top == 0 && self.bottom == 0 && self.left == 0 && self.right == 0
    }
}

/// v1.5.0 "Lens" Workstream D1 — a named 64-colour base palette (RGB triples).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct CustomPalette {
    /// The 64 base colours, `[R, G, B]` each (the same form [`parse_pal`]
    /// produces). Emphasis is applied by the PPU LUT as for the built-in.
    pub colors: Vec<[u8; 3]>,
}

impl CustomPalette {
    /// Build from a fixed 64-entry base palette.
    #[must_use]
    pub fn from_base(base: [[u8; 3]; 64]) -> Self {
        Self {
            colors: base.to_vec(),
        }
    }

    /// Materialize the 64-entry base palette the core expects, padding with
    /// black / truncating if the stored vector is the wrong length (defensive
    /// against hand-edited configs).
    #[must_use]
    pub fn to_base(&self) -> [[u8; 3]; 64] {
        let mut base = [[0u8; 3]; 64];
        for (dst, src) in base.iter_mut().zip(self.colors.iter()) {
            *dst = *src;
        }
        base
    }
}

/// v1.5.0 "Lens" Workstream D1 — the named-palette bank (`name -> palette`).
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct PaletteBank {
    /// Saved custom palettes, keyed by user-chosen name.
    #[serde(default)]
    pub palettes: std::collections::BTreeMap<String, CustomPalette>,
}

fn default_ntsc_filter() -> String {
    "off".into()
}

/// Serde default for [`GraphicsConfig::ntsc_contrast`] — Bisqwit's neutral value.
const fn default_ntsc_contrast() -> f32 {
    crate::ntsc_bisqwit::NtscKnobs::DEFAULT.contrast
}

/// Serde default for [`GraphicsConfig::ntsc_saturation`].
const fn default_ntsc_saturation() -> f32 {
    crate::ntsc_bisqwit::NtscKnobs::DEFAULT.saturation
}

/// Serde default for [`GraphicsConfig::ntsc_brightness`].
const fn default_ntsc_brightness() -> f32 {
    crate::ntsc_bisqwit::NtscKnobs::DEFAULT.brightness
}

/// Serde default for [`GraphicsConfig::ntsc_hue`].
const fn default_ntsc_hue() -> f32 {
    crate::ntsc_bisqwit::NtscKnobs::DEFAULT.hue
}

const fn default_crt_scanline() -> f32 {
    0.5
}

fn default_pacing_mode() -> String {
    "auto".into()
}

const fn default_max_frame_latency() -> u32 {
    2
}

impl Default for GraphicsConfig {
    fn default() -> Self {
        Self {
            // Default to `Mailbox` so the wall-clock frame pacer
            // (`App::pace_frames`, NTSC 60.098 Hz) is the single timing
            // authority. With `Fifo` (vsync) the surface ALSO gates on the
            // display's refresh; on a 60 Hz panel the two clocks beat and
            // drop/double one frame every ~10 s — the visible stutter.
            // `select_present_mode` transparently falls back to `Fifo`
            // when the backend does not advertise `Mailbox`.
            present_mode: "Mailbox".into(),
            ntsc_filter: default_ntsc_filter(),
            pacing_mode: default_pacing_mode(),
            max_frame_latency: default_max_frame_latency(),
            hide_overscan: false,
            crt_filter: false,
            crt_scanline: default_crt_scanline(),
            palette_file: None,
            ntsc_contrast: default_ntsc_contrast(),
            ntsc_saturation: default_ntsc_saturation(),
            ntsc_brightness: default_ntsc_brightness(),
            ntsc_hue: default_ntsc_hue(),
            shader_stack: crate::shader_pass::ShaderStackConfig::default(),
            shader_presets: crate::shader_pass::ShaderPresetBank::default(),
            hd_packs: std::collections::BTreeMap::new(),
            overscan: Overscan::default(),
            palettes: PaletteBank::default(),
            active_palette: None,
            ntsc_palette_enabled: false,
            ntsc_palette: NtscPaletteConfig::default(),
            dual_screen_layout: default_dual_screen_layout(),
        }
    }
}

/// Parse a `.pal` palette file into a 64-entry RGB base palette.
///
/// Accepts the common 192-byte (64 colours × 3) form; longer files (e.g. a 512-entry Mesen
/// palette) use the first 64 colours. Returns `None` if the file is too short.
#[must_use]
pub fn parse_pal(bytes: &[u8]) -> Option<[[u8; 3]; 64]> {
    if bytes.len() < 192 {
        return None;
    }
    let mut pal = [[0u8; 3]; 64];
    for (i, chunk) in bytes[..192].chunks_exact(3).enumerate() {
        pal[i] = [chunk[0], chunk[1], chunk[2]];
    }
    Some(pal)
}

/// Audio configuration.
//
// Not `Eq`: the v1.0.0 `volume` field is an `f32`, so the section is
// `PartialEq` only (matching `Config`, which already drops `Eq` for the
// gamepad deadzone). Not `Copy` since v1.7.0 H3: the `output_device` name is an
// owned `Option<String>`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
// Several independent on/off audio settings (mute / DRC / EQ enable / 20-band
// mode); they are unrelated config flags, not a state machine.
#[allow(clippy::struct_excessive_bools)]
pub struct AudioConfig {
    /// Preferred host sample rate (Hz). The actual rate may differ if
    /// CPAL refuses; the emulator and APU rebuild themselves at the
    /// negotiated rate.
    pub sample_rate: u32,
    /// v2.8.0 Phase 1 — buffered-audio latency target in milliseconds: the
    /// dynamic-rate-control servo holds the queue at this level and the
    /// start-gate waits for it before playback (clamped to 20..=250 at
    /// use). Lower = less audio latency; higher = more stall tolerance.
    #[serde(default = "default_audio_latency_ms")]
    pub latency_ms: u32,
    /// v2.8.0 Phase 1 — dynamic rate control on/off. On (default) nudges
    /// the output rate ±0.5% from queue occupancy so audio never underruns
    /// (silence gaps) or overruns (dropped-sample pops) from clock drift;
    /// off is a bit-exact passthrough of the core's samples.
    #[serde(default = "default_audio_drc")]
    pub drc: bool,
    /// v1.0.0 — master output volume (0.0..=1.0, default 1.0). Applied at the
    /// single cpal consume point (post-resampler, lock-free); the core's
    /// produced samples are byte-identical regardless. Clamped on load.
    /// Default 1.0 = today's sound exactly.
    #[serde(default = "default_audio_volume")]
    pub volume: f32,
    /// v1.0.0 — master mute. When `true` the output gain is forced to 0
    /// (independent of [`Self::volume`]). Default `false`.
    #[serde(default)]
    pub muted: bool,
    /// v1.0.0 — per-APU-channel enable mask (a UI playback overlay, NOT NES
    /// hardware state). Bit 0 = pulse 1, 1 = pulse 2, 2 = triangle, 3 = noise,
    /// 4 = DMC, 5 = external/mapper audio. A cleared bit mutes that channel.
    /// Default `0x3F` (all six on) is byte-identical to today's mixer output —
    /// the deterministic core audio is unchanged unless a channel is muted.
    #[serde(default = "default_audio_channel_mask")]
    pub channel_mask: u8,
    /// v1.1.0 beta.2 (T-110-D2) — enable the frontend graphic EQ output stage.
    /// `false` (default) bypasses it entirely, so the audio is byte-identical
    /// to a build without the EQ.
    #[serde(default)]
    pub eq_enabled: bool,
    /// v1.1.0 beta.2 — per-band EQ gains in dB (−12..=+12) for the five fixed
    /// bands (60 / 240 / 1k / 3.8k / 12k Hz). All-zero (the default) is a flat,
    /// identity response. Clamped on use. Only consulted when [`Self::eq_enabled`].
    #[serde(default = "default_audio_eq_bands")]
    pub eq_bands: [f32; 5],
    /// v1.4.0 Workstream C — per-APU-channel output gain (a UI mixing overlay,
    /// NOT NES hardware state), generalizing [`Self::channel_mask`]. Index 0 =
    /// pulse 1, 1 = pulse 2, 2 = triangle, 3 = noise, 4 = DMC, 5 = external /
    /// mapper expansion audio (VRC6/VRC7/MMC5/Namco-163/Sunsoft-5B). Each gain is
    /// clamped to `0.0..=2.0` on use.
    ///
    /// Default (all `1.0`, `default_audio_channel_gain`) is byte-identical to
    /// today's mixer output — `#[serde(default = ...)]` so a pre-v1.4.0 config
    /// (with no `[audio] channel_gain` key) loads unchanged and the deterministic
    /// core audio + the oracle stay byte-identical until the user moves a slider.
    #[serde(default = "default_audio_channel_gain")]
    pub channel_gain: [f32; 6],
    /// v2.1.3 — the APU analog output-filter model: `"nes"` (default; the NES
    /// front-loader's 90 + 440 Hz high-pass + 14 kHz low-pass — authentic and
    /// thinnest, byte-identical to earlier builds), `"famicom"` (a single 37 Hz
    /// high-pass — fuller bass), or `"clean"` (a ~10 Hz DC-block only — fullest,
    /// closest to Mesen2/FCEUX which omit the high-pass). Tonal only; channel
    /// content is unchanged and it never touches the save state. `#[serde(default)]`
    /// = `"nes"`, so a pre-v2.1.3 config loads byte-identical.
    #[serde(default = "default_audio_filter_model")]
    pub filter_model: String,
    /// v1.7.0 "Forge" H3 — select the 20-band graphic EQ (using [`Self::eq_bands_20`])
    /// instead of the classic 5-band voicing ([`Self::eq_bands`]). `false`
    /// (default) keeps the 5-band behaviour, so a pre-v1.7.0 config with no key
    /// loads byte-identical.
    #[serde(default)]
    pub eq_20_band: bool,
    /// v1.7.0 H3 — per-band gains in dB (−12..=+12) for the 20-band graphic EQ at
    /// the ISO third-octave centers. All-zero (the default) is flat (bypass) and
    /// byte-identical to a no-EQ build. Only consulted when [`Self::eq_enabled`]
    /// and [`Self::eq_20_band`].
    #[serde(default = "default_audio_eq_bands_20")]
    pub eq_bands_20: [f32; 20],
    /// v1.7.0 H3 — per-APU-channel stereo pan in `-1.0..=1.0` (−1 = hard left,
    /// 0 = center, +1 = hard right): index 0 = pulse 1 .. 5 = expansion. All
    /// `0.0` (the default) duplicates the mono master to L/R bit-for-bit, so the
    /// default output is byte-identical. (The frontend applies the average pan
    /// to the pre-mixed mono master; true per-channel panning awaits the v2.0
    /// core split.)
    #[serde(default = "default_audio_pan")]
    pub pan: [f32; 6],
    /// v1.7.0 H3 — reverb wet mix `0.0..=1.0`. `0.0` (default) = dry/bypass.
    #[serde(default)]
    pub reverb_mix: f32,
    /// v1.7.0 H3 — reverb room size `0.0..=1.0` (decay-time control).
    #[serde(default = "default_audio_reverb_room")]
    pub reverb_room: f32,
    /// v1.7.0 H3 — headphone crossfeed amount `0.0..=1.0`. `0.0` (default) =
    /// bypass.
    #[serde(default)]
    pub crossfeed: f32,
    /// v1.7.0 H3 — preferred output device name. `None` (default) = the system
    /// default device (today's behaviour). An unmatched / now-absent name also
    /// falls back to the default device.
    #[serde(default)]
    pub output_device: Option<String>,
    /// v1.7.0 H3 — master output volume multiplier `0.0..=1.0` applied across
    /// every context, on top of the per-context gains. `1.0` (default) is a
    /// no-op. (Distinct from [`Self::volume`], the existing single master
    /// slider; this is the per-context mixer's master leg.)
    #[serde(default = "default_audio_volume")]
    pub master_volume: f32,
    /// v1.7.0 H3 — gain applied while a game is running `0.0..=1.0`. `1.0`
    /// (default) is a no-op.
    #[serde(default = "default_audio_volume")]
    pub volume_game: f32,
    /// v1.7.0 H3 — gain applied while in a menu / no game is running
    /// `0.0..=1.0`. `1.0` (default) is a no-op.
    #[serde(default = "default_audio_volume")]
    pub volume_menu: f32,
}

/// Serde default for [`AudioConfig::eq_bands`] — flat (0 dB) across all bands.
const fn default_audio_eq_bands() -> [f32; 5] {
    [0.0; 5]
}

/// Serde default for [`AudioConfig::eq_bands_20`] — flat (0 dB) across 20 bands.
const fn default_audio_eq_bands_20() -> [f32; 20] {
    [0.0; 20]
}

/// Serde default for [`AudioConfig::pan`] — all center (`0.0`).
const fn default_audio_pan() -> [f32; 6] {
    [0.0; 6]
}

/// Serde default for [`AudioConfig::reverb_room`] — a medium room.
const fn default_audio_reverb_room() -> f32 {
    0.5
}

/// Serde default for [`AudioConfig::channel_gain`] — unity (`1.0`) across all
/// six channels (byte-identical to the un-scaled mix).
const fn default_audio_channel_gain() -> [f32; 6] {
    [1.0; 6]
}

/// Serde default for [`AudioConfig::channel_mask`] — all six channels enabled.
const fn default_audio_channel_mask() -> u8 {
    0x3F
}

/// Serde default for [`AudioConfig::filter_model`] — the authentic NES
/// front-loader chain (byte-identical to pre-v2.1.3 output).
fn default_audio_filter_model() -> String {
    "nes".to_string()
}

/// Map the `[audio] filter_model` config string to the core enum
/// (`"famicom"` / `"clean"`, anything else → `NesRf`).
#[must_use]
pub const fn parse_filter_model(s: &str) -> rustynes_core::rustynes_apu::FilterModel {
    use rustynes_core::rustynes_apu::FilterModel;
    // Case-insensitive so a hand-edited `"Clean"` / `"Famicom"` still resolves.
    if s.eq_ignore_ascii_case("famicom") {
        FilterModel::Famicom
    } else if s.eq_ignore_ascii_case("clean") {
        FilterModel::Clean
    } else {
        FilterModel::NesRf
    }
}

/// The canonical lowercase config token for a [`rustynes_core::rustynes_apu::FilterModel`]
/// (`"nes"` / `"famicom"` / `"clean"`). Round-trips with [`parse_filter_model`].
#[must_use]
pub const fn filter_model_token(model: rustynes_core::rustynes_apu::FilterModel) -> &'static str {
    use rustynes_core::rustynes_apu::FilterModel;
    match model {
        FilterModel::NesRf => "nes",
        FilterModel::Famicom => "famicom",
        FilterModel::Clean => "clean",
    }
}

/// Serde default for [`AudioConfig::latency_ms`].
const fn default_audio_latency_ms() -> u32 {
    60
}

/// Serde default for [`AudioConfig::drc`].
const fn default_audio_drc() -> bool {
    true
}

/// Serde default for [`AudioConfig::volume`].
const fn default_audio_volume() -> f32 {
    1.0
}

impl Default for AudioConfig {
    fn default() -> Self {
        Self {
            sample_rate: 44_100,
            latency_ms: default_audio_latency_ms(),
            drc: default_audio_drc(),
            volume: default_audio_volume(),
            muted: false,
            channel_mask: default_audio_channel_mask(),
            eq_enabled: false,
            eq_bands: default_audio_eq_bands(),
            channel_gain: default_audio_channel_gain(),
            filter_model: default_audio_filter_model(),
            eq_20_band: false,
            eq_bands_20: default_audio_eq_bands_20(),
            pan: default_audio_pan(),
            reverb_mix: 0.0,
            reverb_room: default_audio_reverb_room(),
            crossfeed: 0.0,
            output_device: None,
            master_volume: default_audio_volume(),
            volume_game: default_audio_volume(),
            volume_menu: default_audio_volume(),
        }
    }
}

impl AudioConfig {
    /// v1.0.0 — the master output gain to apply at the cpal consume point:
    /// the clamped [`Self::volume`], or 0.0 when [`Self::muted`]. Default
    /// (volume 1.0, not muted) returns 1.0 = today's sound exactly.
    #[must_use]
    pub const fn effective_gain(&self) -> f32 {
        if self.muted {
            0.0
        } else {
            self.volume.clamp(0.0, 1.0)
        }
    }

    /// v1.7.0 H3 — the master gain folded with the per-context mixer legs:
    /// [`Self::effective_gain`] × `master_volume` × (`volume_game` when
    /// `in_game`, else `volume_menu`). All the new legs default to `1.0`, so the
    /// product equals [`Self::effective_gain`] exactly until the user moves a
    /// per-context slider — keeping the default output byte-identical.
    #[must_use]
    pub fn effective_gain_for(&self, in_game: bool) -> f32 {
        let context = if in_game {
            self.volume_game
        } else {
            self.volume_menu
        };
        (self.effective_gain() * self.master_volume.clamp(0.0, 1.0) * context.clamp(0.0, 1.0))
            .clamp(0.0, 1.0)
    }
}

/// egui visual theme for the desktop UX shell (menu bar, status bar, windows).
///
/// The v1.5.0 "Lens" accessibility pass extends the original light/dark/system
/// trio with two accessibility-oriented variants:
///
/// - [`AppTheme::HighContrast`] — a near-black background with near-white text
///   and saturated accents, raising every UI foreground/background pair past
///   the WCAG 2.1 AA 4.5:1 (and most past 7:1 AAA) contrast ratio for
///   low-vision users.
/// - [`AppTheme::Colorblind`] — a dark theme whose interactive accents are
///   drawn from a deuteranopia/protanopia-safe palette (Okabe-Ito), so the
///   selection/hover/active cues stay distinguishable for the most common
///   forms of red-green color vision deficiency.
///
/// Both are additive: the variants are appended after the originals, so any
/// existing config that stored `"light"`/`"dark"`/`"system"` deserializes
/// unchanged and the default is still [`AppTheme::Dark`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "lowercase")]
pub enum AppTheme {
    /// Light visuals.
    Light,
    /// Dark visuals (default).
    #[default]
    Dark,
    /// Follow the OS theme when the windowing system reports one (falls back
    /// to [`AppTheme::Dark`] when unknown).
    System,
    /// High-contrast dark theme (WCAG AA/AAA foreground/background pairs) for
    /// low-vision accessibility. (v1.5.0)
    #[serde(rename = "high-contrast")]
    HighContrast,
    /// Colorblind-safe dark theme using the Okabe-Ito palette for interactive
    /// accents (deuteranopia/protanopia-friendly). (v1.5.0)
    Colorblind,
}

impl AppTheme {
    /// Human-readable label for the settings combo box.
    #[must_use]
    pub const fn display_name(self) -> &'static str {
        match self {
            Self::Light => "Light",
            Self::Dark => "Dark",
            Self::System => "System",
            Self::HighContrast => "High Contrast",
            Self::Colorblind => "Colorblind-Safe",
        }
    }

    /// All themes in display order — single source of truth for the menu radio
    /// list and the Settings combo box so the two never drift apart.
    #[must_use]
    pub const fn all() -> [Self; 5] {
        [
            Self::Light,
            Self::Dark,
            Self::System,
            Self::HighContrast,
            Self::Colorblind,
        ]
    }
}

/// `[recent_roms]` section — the File -> Recent MRU list (v1.0.0).
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecentRoms {
    /// Most-recently-opened ROM paths, newest first.
    #[serde(default)]
    pub paths: Vec<PathBuf>,
    /// Maximum number of entries retained. Default 10.
    #[serde(default = "default_recent_max")]
    pub max_entries: usize,
}

/// Serde default for [`RecentRoms::max_entries`].
const fn default_recent_max() -> usize {
    10
}

impl Default for RecentRoms {
    fn default() -> Self {
        Self {
            paths: Vec::new(),
            max_entries: default_recent_max(),
        }
    }
}

impl RecentRoms {
    /// Insert `path` as the newest entry: de-duplicate, push to front, and
    /// truncate to `max_entries`.
    pub fn add(&mut self, path: PathBuf) {
        self.paths.retain(|p| p != &path);
        self.paths.insert(0, path);
        self.paths.truncate(self.max_entries.max(1));
    }
}

/// `[ui]` section — the desktop UX shell (theme, pixel aspect, FPS readout)
/// (v1.0.0).
//
// Not `Eq`: the v1.5.0 accessibility `zoom_factor` is an `f32`, so this section
// (and therefore the whole `Config` tree, which was already `PartialEq`-only)
// is `PartialEq` only.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
// v1.7.0 H4 — `show_lag_frames` pushed the visibility toggles past 3 bools; they
// are independent display flags, so a struct is clearer than packed bitflags.
#[allow(clippy::struct_excessive_bools)]
pub struct UiConfig {
    /// egui visual theme. Default [`AppTheme::Dark`].
    #[serde(default)]
    pub theme: AppTheme,
    /// Apply 8:7 pixel-aspect-ratio correction to the NES image. Default
    /// `false`, so the shipped output stays pixel-exact unless opted in.
    #[serde(default)]
    pub pixel_aspect_correction: bool,
    /// Show the FPS readout in the status bar. Default `true`.
    #[serde(default = "default_ui_show_fps")]
    pub show_fps: bool,
    /// v1.7.0 "Forge" Workstream H4 — show the lag-frame counter in the status
    /// bar (forward frames since ROM load in which the program polled no
    /// controller — a TAS/debug diagnostic). Default `false`, so the status bar
    /// is unchanged unless opted in. Pure observation; never affects emulation.
    #[serde(default)]
    pub show_lag_frames: bool,
    /// Auto-pause emulation when the window loses focus, auto-resume when it
    /// regains focus. Default `false` (no behavior change unless enabled). A
    /// manual user pause is never overridden, and this never auto-pauses
    /// during a netplay session.
    #[serde(default)]
    pub pause_on_focus_loss: bool,
    /// egui UI zoom factor (accessibility). The whole egui shell — menu bar,
    /// Settings, debugger panels, fonts — is scaled by this multiplier via
    /// `ctx.set_zoom_factor`. The emulated NES image is unaffected (it is a
    /// raw framebuffer blit, not egui content). Default `1.0` so the shipped
    /// UI is pixel-identical unless the user opts in. Clamped to
    /// [`UiConfig::ZOOM_MIN`]..=[`UiConfig::ZOOM_MAX`] on apply. (v1.5.0)
    #[serde(default = "default_ui_zoom_factor")]
    pub zoom_factor: f32,
    /// v1.7.0 "Forge" Workstream H5 — UI language (i18n). Selects the active
    /// string catalog (see [`crate::i18n`]). Defaults to
    /// [`Locale::English`](crate::i18n::Locale::English), so a config that omits
    /// the field (every pre-H5 config) renders the verbatim English UI, byte-
    /// identical to v1.6.0. Set from the Settings language picker.
    #[serde(default)]
    pub locale: crate::i18n::Locale,
}

/// Serde default for [`UiConfig::show_fps`].
const fn default_ui_show_fps() -> bool {
    true
}

/// Serde default for [`UiConfig::zoom_factor`].
const fn default_ui_zoom_factor() -> f32 {
    1.0
}

impl UiConfig {
    /// Minimum UI zoom factor exposed in the accessibility control.
    pub const ZOOM_MIN: f32 = 0.5;
    /// Maximum UI zoom factor exposed in the accessibility control.
    pub const ZOOM_MAX: f32 = 3.0;

    /// The configured zoom factor clamped to the supported range. Guards
    /// against a hand-edited config asking for a degenerate value, including
    /// a non-finite (`NaN`/`Infinity`) `zoom_factor` that would otherwise
    /// propagate into egui layout and panic; those fall back to the default.
    #[must_use]
    pub const fn clamped_zoom_factor(&self) -> f32 {
        if self.zoom_factor.is_finite() {
            self.zoom_factor.clamp(Self::ZOOM_MIN, Self::ZOOM_MAX)
        } else {
            default_ui_zoom_factor()
        }
    }
}

impl Default for UiConfig {
    fn default() -> Self {
        Self {
            theme: AppTheme::default(),
            pixel_aspect_correction: false,
            show_fps: default_ui_show_fps(),
            show_lag_frames: false,
            pause_on_focus_loss: false,
            zoom_factor: default_ui_zoom_factor(),
            locale: crate::i18n::Locale::default(),
        }
    }
}

/// A/V recording codec-depth options (v1.8.9), persisted so the Settings picker
/// round-trips through `config.toml`.
///
/// Deliberately plain (un-gated) strings / numbers — the picker UI works even in a
/// build without the `av-record` feature, and the arming path (gated) parses the
/// codec / preset ids via `AvRecordOptions::from_parts`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RecordingConfig {
    /// Video codec id: `h264` | `h265` | `vp9`.
    pub video_codec: String,
    /// Constant-quality factor (CRF), 0..=51 (lower = higher quality / bigger).
    pub crf: u8,
    /// x264 / x265 preset id: `ultrafast`..`slow` (ignored by VP9).
    pub preset: String,
    /// AAC audio bitrate in kbit/s.
    pub audio_bitrate_k: u32,
}

impl Default for RecordingConfig {
    fn default() -> Self {
        Self {
            video_codec: "h264".into(),
            crf: 18,
            preset: "veryfast".into(),
            audio_bitrate_k: 192,
        }
    }
}

/// Top-level config struct.
//
// Not `Eq`: `InputConfig` carries the `f32` gamepad `axis_deadzone`, so
// the whole tree is `PartialEq` only.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq)]
pub struct Config {
    /// Input bindings.
    #[serde(default)]
    pub input: InputConfig,
    /// Rewind defaults.
    #[serde(default)]
    pub rewind: RewindConfig,
    /// Graphics defaults.
    #[serde(default)]
    pub graphics: GraphicsConfig,
    /// Audio defaults.
    #[serde(default)]
    pub audio: AudioConfig,
    /// A/V recording codec depth (v1.8.9): encoder / CRF / preset / audio
    /// bitrate the Settings picker writes; read at arm time (`av-record`).
    #[serde(default)]
    pub recording: RecordingConfig,
    /// Famicom Disk System defaults (BIOS path) (v2.2.0).
    #[serde(default)]
    pub fds: FdsConfig,
    /// Netplay defaults (last host port + join address) (v2.3.0).
    #[serde(default)]
    pub netplay: NetplayConfig,
    /// v1.5.0 "Lens" Workstream D3 — grouped non-accuracy "enhancement" modes
    /// (sprite-limit disable, optional overclock). All off by default and
    /// never part of the determinism oracle / `AccuracyCoin`.
    #[serde(default)]
    pub enhancements: EnhancementsConfig,
    /// v2.1.4 F2.3 — the `[emulation]` section: optional *accuracy* toggles that
    /// change the deterministic core behaviour (distinct from `[enhancements]`,
    /// which never touches the oracle). All off by default = byte-identical.
    #[serde(default)]
    pub emulation: EmulationConfig,
    /// Vs. System arcade defaults (DIP switches) (v2.5.0).
    #[serde(default)]
    pub vs: VsConfig,
    /// `RetroAchievements` defaults (login token + hardcore) (v2.7.0).
    #[serde(default)]
    pub retroachievements: RetroAchievementsConfig,
    /// Desktop UX shell settings — theme, 8:7 pixel aspect, FPS readout (v1.0.0).
    #[serde(default)]
    pub ui: UiConfig,
    /// Recently-opened ROMs for the File -> Recent menu (v1.0.0).
    #[serde(default)]
    pub recent_roms: RecentRoms,
    /// `true` once the first-run Welcome modal has been dismissed. Defaults to
    /// `false` (so a brand-new install — which has no config file and thus gets
    /// `Config::default()` — shows the welcome), and is set to `true` + saved
    /// when the user dismisses it (v1.0.0).
    #[serde(default)]
    pub welcome_shown: bool,
}

/// v1.5.0 "Lens" Workstream D3 — the `[enhancements]` section: non-accuracy
/// "improvement" modes, grouped together.
///
/// Mirrors `GeraNES`' Improvements window / Mesen2's emulation enhancements.
/// Every field is off / neutral by default, so a pre-D3 config is
/// byte-identical, and **none of these are ever applied while the determinism
/// oracle / `AccuracyCoin` / TAS / netplay paths run** — they are explicitly
/// out-of-oracle enhancement modes.
///
/// NOTE (v1.5.0): the cycle-accurate core does not yet expose hooks to disable
/// the 8-sprite-per-scanline limit or to overclock the PPU/CPU (both require a
/// core synthesis change, deferred to the v2.0 fractional-master-clock
/// refactor, ADR 0002). These flags persist the user's *intent* and are
/// surfaced in the UI as experimental / staged; the frontend applies only the
/// portions that are achievable without a core change. They never affect the
/// deterministic core output today.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct EnhancementsConfig {
    /// Disable the hardware 8-sprite-per-scanline limit (removes sprite
    /// flicker). Off by default = accurate hardware behaviour. **Staged**: the
    /// current cycle-accurate core has no no-sprite-limit hook, so this is
    /// persisted + surfaced but inert until the v2.0 core pass (ADR 0002).
    #[serde(default)]
    pub disable_sprite_limit: bool,
    /// Optional overclock: extra emulated PPU scanlines inserted in the
    /// vblank, reducing per-scanline slowdown in some games (Mesen2's
    /// "additional scanlines" enhancement). `0` (default) = stock timing.
    /// **Staged**: no core hook yet (v2.0, ADR 0002); persisted + surfaced
    /// only. Clamped to `0..=80` on use.
    #[serde(default)]
    pub overclock_scanlines: u16,
}

/// v2.1.4 F2.3 — the `[emulation]` section: optional **accuracy** toggles.
///
/// Unlike [`EnhancementsConfig`] (non-accuracy "improvement" modes that are never
/// applied while the oracle runs), these make emulation *more* faithful to real
/// hardware and DO feed the deterministic core when enabled. Every *accuracy*
/// field is off/neutral by default, so a pre-v2.1.4 config (no `[emulation]`
/// section) loads **byte-identical** to today's behaviour.
///
/// [`Self::fast_dotloop`] is the one field that is NOT an accuracy knob: it
/// selects a PPU code path that produces the identical frame either way, and it
/// defaults **on**. It lives here because it is pushed into the core through the
/// same `apply_ppu_hardware_config` path as the rest, not because it changes
/// emulation.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
// These are independent, orthogonal accuracy toggles (each maps to a distinct
// core knob), not a state machine — a bitfield/enum would only obscure the
// serde config surface. v2.1.7 P5 pushed the count past the 3-bool lint.
#[allow(clippy::struct_excessive_bools)]
pub struct EmulationConfig {
    /// Model OAM decay: the 2C02's dynamic sprite RAM loses un-refreshed rows to a
    /// fixed garbage pattern when rendering is disabled for a while (à la Mesen2's
    /// `EnableOamDecay`). **Off by default** = byte-identical to a decay-free core;
    /// `AccuracyCoin`, the commercial oracle, and the visual regression suites are
    /// unaffected. NTSC/Dendy-only (PAL's refresh cadence masks decay). Deterministic
    /// when on (driven off the PPU's monotonic dot counter). Pushed into the core on
    /// ROM load / power-cycle / a Settings change via `Nes::set_oam_decay`.
    #[serde(default)]
    pub oam_decay: bool,

    /// v2.1.7 P5 — model the earlier `Rp2c02G` 2C02 die: when `true`, an OAMADDR
    /// (`$2003`) write while rendering is active corrupts one OAM row (the *Huge
    /// Insect* glitch). **Off by default** (the `Rp2c02H` revision), which is
    /// byte-identical. Pushed into the core via `Nes::set_ppu_revision`.
    #[serde(default)]
    pub ppu_oamaddr_corruption: bool,

    /// v2.1.7 P5 — load the canonical "blargg" power-up palette-RAM dump instead
    /// of the all-zero default, for software that samples uninitialized palette
    /// RAM. **Off by default** = all-zero power-up palette (byte-identical).
    /// Pushed into the core via `Nes::set_power_up_palette`.
    #[serde(default)]
    pub blargg_power_up_palette: bool,

    /// v2.1.7 P5 — randomize the 2 KiB work RAM at power-on from
    /// [`Self::power_on_ram_seed`] (deterministic `xorshift64`), surfacing
    /// software that reads uninitialized RAM (*Final Fantasy* RNG seed, *River
    /// City Ransom*, *Cybernoid*). **Off by default** = all-zero work RAM
    /// (byte-identical). Pushed into the core via `Nes::set_power_on_ram`.
    #[serde(default)]
    pub randomize_power_on_ram: bool,

    /// v2.1.7 P5 — the deterministic seed used when [`Self::randomize_power_on_ram`]
    /// is set. Same seed ⇒ identical power-on RAM (the determinism contract).
    /// Ignored when randomization is off.
    #[serde(default)]
    pub power_on_ram_seed: u64,

    /// v2.1.8 A1 / v2.2.3 — use the specialized visible-scanline fast dot path
    /// (`Nes::set_fast_dotloop`). **On by default**, and unlike every other
    /// field here it is **not an accuracy knob**: the fast path runs the same
    /// helper sequence as the general path with statically-dead branches
    /// pruned, so it emits the identical framebuffer, audio and cycle count —
    /// pinned bit-for-bit every frame by `fast_dotloop_diff`. It is ~11% faster
    /// on rendering-heavy content and neutral when rendering is disabled.
    ///
    /// Exposed as a setting only as an escape hatch: if a future change ever
    /// made the two paths disagree, turning this off selects the
    /// fully-general per-dot path without a rebuild. There is no accuracy
    /// reason to disable it.
    ///
    /// Defaulted through `default_fast_dotloop` rather than `#[serde(default)]`
    /// so an existing config written before this key existed loads as `true`
    /// (the shipped default) instead of silently opting the user out.
    #[serde(default = "default_fast_dotloop")]
    pub fast_dotloop: bool,
}

/// Serde + [`Default`] value for [`EmulationConfig::fast_dotloop`] — `true`.
///
/// A named function because `bool`'s `Default` is `false`, which would both
/// deserialize a pre-v2.2.3 config to the wrong value and make
/// `EmulationConfig::default()` disagree with the core's own default.
const fn default_fast_dotloop() -> bool {
    true
}

impl Default for EmulationConfig {
    /// Hand-written rather than derived: every accuracy field is `false`/`0`
    /// (the byte-identical baseline), but [`Self::fast_dotloop`] must default
    /// to `true` to match the core, which `#[derive(Default)]` cannot express.
    fn default() -> Self {
        Self {
            oam_decay: false,
            ppu_oamaddr_corruption: false,
            blargg_power_up_palette: false,
            randomize_power_on_ram: false,
            power_on_ram_seed: 0,
            fast_dotloop: default_fast_dotloop(),
        }
    }
}

/// `[retroachievements]` section (v2.7.0).
///
/// `RetroAchievements` is native-only and gated behind the default-OFF
/// `retroachievements` cargo feature; this config section is always present in
/// the shared `Config` (so the serde shape is target-agnostic) but only
/// consulted when the feature is compiled in. All fields are
/// `#[serde(default)]`, so a pre-v2.7.0 config (with no `[retroachievements]`
/// section) loads unchanged.
///
/// Only the login **token** is persisted, never the password: a password login
/// returns a token (see `RaUser::token`) which is written back here so a
/// re-launch logs in without re-entering the password.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RetroAchievementsConfig {
    /// Master enable. When `true` and a saved [`Self::token`] is present, the
    /// app logs in automatically at startup. Default `false`.
    #[serde(default)]
    pub enabled: bool,
    /// The `RetroAchievements` username. Persisted so the token login can be
    /// retried on the next launch.
    #[serde(default)]
    pub username: String,
    /// The login **token** returned by a successful login (NOT the password).
    /// Persisted after a successful password or token login.
    #[serde(default)]
    pub token: String,
    /// Hardcore mode (no save-state load / rewind / cheats / RAM-watch).
    /// Defaults to `true`, matching the `RetroAchievements` convention.
    #[serde(default = "default_ra_hardcore")]
    pub hardcore: bool,
    /// The `RetroAchievements` host base URL. Default
    /// `https://retroachievements.org`.
    #[serde(default = "default_ra_host")]
    pub host: String,
}

const fn default_ra_hardcore() -> bool {
    true
}

fn default_ra_host() -> String {
    "https://retroachievements.org".to_string()
}

impl Default for RetroAchievementsConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            username: String::new(),
            token: String::new(),
            hardcore: default_ra_hardcore(),
            host: default_ra_host(),
        }
    }
}

/// `[vs]` section — Vs. System arcade hardware settings. Only consulted when a
/// Vs. System game is loaded; a normal NES game ignores it entirely.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq, Default)]
pub struct VsConfig {
    /// 8-bit DIP-switch bank (switch 1 = bit 0 .. switch 8 = bit 7). Read
    /// through the upper bits of `$4016`/`$4017`. Game-specific (difficulty,
    /// lives, free-play, etc.); see the game's manual. Default 0.
    #[serde(default)]
    pub dip: u8,
    /// True when the user has explicitly chosen a DIP value (so the per-game
    /// database must NOT override it). Defaults to `false` so existing configs
    /// — and games not in the DB — keep working: the precedence is explicit
    /// config dip > per-game DB default > 0. The settings/rebind UI sets this
    /// when the user edits the DIP field.
    #[serde(default)]
    pub dip_set: bool,
}

impl Config {
    /// Resolve the standard config-file path (e.g. `~/.config/rustynes/config.toml`).
    #[must_use]
    pub fn default_path() -> Option<PathBuf> {
        ProjectDirs::from(APP_QUALIFIER, APP_ORG, APP_NAME)
            .map(|d| d.config_dir().join("config.toml"))
    }

    /// Resolve the standard data dir (used for save-state slots).
    #[must_use]
    pub fn default_data_dir() -> Option<PathBuf> {
        ProjectDirs::from(APP_QUALIFIER, APP_ORG, APP_NAME).map(|d| d.data_dir().to_path_buf())
    }

    /// Load from the default path. Missing file -> defaults; a pre-v1.3.1
    /// (legacy-schema) config is migrated in place (with a backup) and the
    /// migrated value used; an unreadable / syntactically-invalid file logs
    /// a warning and falls back to defaults.
    #[must_use]
    pub fn load_or_default() -> Self {
        let Some(path) = Self::default_path() else {
            return Self::default();
        };
        let bytes = match fs::read_to_string(&path) {
            Ok(b) => b,
            Err(ref e) if e.kind() == io::ErrorKind::NotFound => return Self::default(),
            Err(e) => {
                eprintln!(
                    "rustynes: config {} unreadable, using defaults: {e}",
                    path.display()
                );
                return Self::default();
            }
        };

        // Detect + migrate a legacy schema BEFORE the strict parse, so an
        // old config (which would otherwise hard-fail to parse, or silently
        // have its unknown sections ignored) carries its recognizable
        // settings forward instead of being silently discarded.
        if let Some((migrated, notes)) = Self::migrate_legacy(&bytes) {
            migrated.apply_migration(&path, &bytes, &notes);
            return migrated;
        }

        match toml::from_str::<Self>(&bytes) {
            Ok(cfg) => cfg,
            Err(e) => {
                eprintln!(
                    "rustynes: config {} unreadable, using defaults: {e}",
                    path.display()
                );
                Self::default()
            }
        }
    }

    /// Best-effort migration of a pre-v1.3.1 config schema.
    ///
    /// Older builds used `[video] vsync`, `[input.keyboard_p1]` /
    /// `[input.keyboard_p2]`, etc. Such files either failed the strict
    /// parse (→ silent fallback to defaults) or had their unknown sections
    /// silently ignored — in both cases the user's settings were lost.
    /// This carries the recognizable legacy fields into the current schema.
    ///
    /// Returns `Some((config, notes))` when the input is recognized as
    /// legacy (and was migrated), or `None` when the input is already
    /// current-schema (or is not valid TOML at all, in which case the
    /// caller's strict parse reports the error). `notes` are human-readable
    /// descriptions of what was carried over, for the migration warning.
    fn migrate_legacy(bytes: &str) -> Option<(Self, Vec<String>)> {
        let value: toml::Value = toml::from_str(bytes).ok()?;
        let table = value.as_table()?;

        // Legacy markers: a `[video]` section (replaced by `[graphics]`) or
        // `[input.keyboard_pN]` sub-tables (renamed to `[input.playerN]`).
        let has_video = table.contains_key("video");
        let legacy_input = table
            .get("input")
            .and_then(toml::Value::as_table)
            .is_some_and(|i| i.contains_key("keyboard_p1") || i.contains_key("keyboard_p2"));
        if !has_video && !legacy_input {
            return None;
        }

        let mut cfg = Self::default();
        let mut notes = Vec::new();

        // [video] vsync (bool) -> [graphics] present_mode.
        if let Some(vsync) = table
            .get("video")
            .and_then(|v| v.get("vsync"))
            .and_then(toml::Value::as_bool)
        {
            cfg.graphics.present_mode = if vsync { "Fifo" } else { "Mailbox" }.into();
            notes.push(format!(
                "[video] vsync = {vsync} -> [graphics] present_mode = \"{}\"",
                cfg.graphics.present_mode
            ));
        }

        // Legacy keyboard bindings: [input.keyboard_pN] -> [input.playerN].
        // The field names (up/down/.../start) are unchanged, so the legacy
        // sub-table deserializes directly into `PadBindings`. The keycode
        // *values*, however, are old winit-0.29 `VirtualKeyCode` Debug
        // names (`Up`, `Z`, `Return`, `RShift`, ...); canonicalize each to
        // its current winit-0.30 `KeyCode` spelling so the written file is
        // clean. (`parse_keycode` accepts the legacy names as aliases too,
        // so this is cosmetic — but it keeps migrated files current.)
        if let Some(input) = table.get("input").and_then(toml::Value::as_table) {
            if let Some(p1) = input
                .get("keyboard_p1")
                .and_then(|v| v.clone().try_into::<PadBindings>().ok())
            {
                cfg.input.player1 = canonicalize_pad(&p1);
                notes.push("[input.keyboard_p1] -> [input.player1]".into());
            }
            if let Some(p2) = input
                .get("keyboard_p2")
                .and_then(|v| v.clone().try_into::<PadBindings>().ok())
            {
                cfg.input.player2 = canonicalize_pad(&p2);
                notes.push("[input.keyboard_p2] -> [input.player2]".into());
            }
        }

        // [audio] sample_rate is unchanged in name; preserve it if valid.
        if let Some(sr) = table
            .get("audio")
            .and_then(|a| a.get("sample_rate"))
            .and_then(toml::Value::as_integer)
            .and_then(|n| u32::try_from(n).ok())
        {
            cfg.audio.sample_rate = sr;
            notes.push(format!("[audio] sample_rate = {sr} preserved"));
        }

        Some((cfg, notes))
    }

    /// Back up the legacy config, write the upgraded one, and log a loud
    /// (non-silent) summary of the migration. All steps are best-effort:
    /// failures are reported but never fatal (the migrated config is still
    /// used in-memory for the session).
    fn apply_migration(&self, path: &Path, original: &str, notes: &[String]) {
        // `config.toml` -> `config.toml.bak` (append, don't replace the ext).
        let mut backup_os = path.as_os_str().to_os_string();
        backup_os.push(".bak");
        let backup = PathBuf::from(backup_os);

        let backup_ok = fs::write(&backup, original).is_ok();
        let write_ok = self.save_to(path).is_ok();

        eprintln!(
            "rustynes: migrated an outdated config schema at {}",
            path.display()
        );
        for note in notes {
            eprintln!("rustynes:   carried over {note}");
        }
        if backup_ok {
            eprintln!("rustynes:   original backed up to {}", backup.display());
        }
        if write_ok {
            eprintln!(
                "rustynes:   upgraded config written; any settings not listed above were reset to defaults"
            );
        } else {
            eprintln!(
                "rustynes:   could not write the upgraded config; using the migrated settings in-memory for this session only"
            );
        }
    }

    /// Load from an explicit path.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] on I/O or parse failure.
    pub fn load_from(path: &Path) -> Result<Self, ConfigError> {
        let bytes = fs::read_to_string(path)?;
        let cfg: Self = toml::from_str(&bytes)?;
        Ok(cfg)
    }

    /// Save to the default path, creating parent directories if missing.
    //
    // Used by the planned Sprint 5-3 egui modal ("Save Settings" button)
    // and exercised by the unit tests; the bin currently doesn't write
    // configs from code.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] on I/O or serialization failure.
    #[allow(dead_code)]
    pub fn save(&self) -> Result<(), ConfigError> {
        let Some(path) = Self::default_path() else {
            return Ok(());
        };
        self.save_to(&path)
    }

    /// Save to an explicit path.
    ///
    /// # Errors
    ///
    /// Returns [`ConfigError`] on I/O or serialization failure.
    #[allow(dead_code)]
    pub fn save_to(&self, path: &Path) -> Result<(), ConfigError> {
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent)?;
        }
        let s = toml::to_string_pretty(self)?;
        fs::write(path, s)?;
        Ok(())
    }
}

/// Rewrite every keycode value of a [`PadBindings`] to its canonical
/// current winit-0.30 `KeyCode` spelling. Used by [`Config::migrate_legacy`]
/// to clean up legacy winit-0.29 `VirtualKeyCode` value strings carried in
/// from a pre-v1.3.1 config.
fn canonicalize_pad(pad: &PadBindings) -> PadBindings {
    use crate::input::canonical_keycode_name;
    PadBindings {
        up: canonical_keycode_name(&pad.up),
        down: canonical_keycode_name(&pad.down),
        left: canonical_keycode_name(&pad.left),
        right: canonical_keycode_name(&pad.right),
        a: canonical_keycode_name(&pad.a),
        b: canonical_keycode_name(&pad.b),
        select: canonical_keycode_name(&pad.select),
        start: canonical_keycode_name(&pad.start),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    #[test]
    fn parse_pal_reads_64_colours_and_rejects_short() {
        // A 192-byte file → 64 RGB triples, in order.
        let mut bytes = Vec::with_capacity(192);
        for i in 0..64u8 {
            bytes.extend_from_slice(&[i, i.wrapping_add(1), i.wrapping_add(2)]);
        }
        let pal = parse_pal(&bytes).expect("192-byte .pal parses");
        assert_eq!(pal[0], [0, 1, 2]);
        assert_eq!(pal[63], [63, 64, 65]);
        // A longer file (e.g. 512-entry) uses the first 64 colours.
        bytes.extend_from_slice(&[0xAA; 300]);
        assert_eq!(parse_pal(&bytes).unwrap()[0], [0, 1, 2]);
        // Too short → None.
        assert!(parse_pal(&[0u8; 191]).is_none());
    }

    #[test]
    fn ntsc_palette_defaults_off_and_neutral() {
        // Default = disabled + neutral params, so the shipped presentation keeps
        // the built-in palette (byte-identical).
        let g = GraphicsConfig::default();
        assert!(
            !g.ntsc_palette_enabled,
            "generated palette must default off"
        );
        let p = g.ntsc_palette.to_params();
        assert!((p.saturation - 1.0).abs() < f64::EPSILON);
        assert!((p.hue - 0.0).abs() < f64::EPSILON);
        assert!((p.contrast - 1.0).abs() < f64::EPSILON);
        assert!((p.brightness - 1.0).abs() < f64::EPSILON);
        assert!((p.gamma - 1.8).abs() < f64::EPSILON);
    }

    #[test]
    fn defaults_round_trip_through_toml() {
        let cfg = Config::default();
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn missing_keys_use_defaults() {
        // Empty TOML -> all defaults.
        let cfg: Config = toml::from_str("").unwrap();
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn shortcut_registry_defaults_are_byte_identical() {
        // v1.2.0 Workstream H2 — the system-bindings section *is* the
        // remappable shortcut registry the menu reads. Every field carries a
        // `#[serde(default)]` returning today's hardcoded hotkey, so a config
        // with NO `[input.system]` section (and an empty one) must reproduce
        // the exact bindings the app shipped with — i.e. a pre-H2 / default
        // build is byte-identical. This guards the H2 promise directly.
        let expected = SystemBindings::default();

        // (a) a config file with no `[input.system]` at all — the real
        // pre-H2 / default-build case (`InputConfig::system` is
        // `#[serde(default)]`). This is the byte-identical guarantee.
        let no_section: Config = toml::from_str("").unwrap();
        assert_eq!(no_section.input.system, expected);

        // (b) a `[input.system]` table that supplies ONLY the original
        // (pre-1.0) required fields and omits every newer hotkey parses to
        // today's defaults for the omitted keys — i.e. an on-disk config
        // written before those keys existed upgrades silently. Parsed as a bare
        // `SystemBindings` because the surrounding `InputConfig::player1/2/system`
        // are themselves required fields.
        let legacy_section: SystemBindings = toml::from_str(
            "quit = \"Escape\"\n\
             save_state = \"F1\"\n\
             load_state = \"F4\"\n\
             rewind = \"F5\"\n\
             reset = \"F2\"\n\
             power_cycle = \"F3\"\n",
        )
        .unwrap();
        assert_eq!(legacy_section, expected);

        // (c) pin the literal default key strings so an accidental rebinding of
        // a default in a future edit is caught here (the registry serialises
        // these verbatim, which is what the menu accelerator labels show).
        assert_eq!(expected.quit, "Escape");
        assert_eq!(expected.save_state, "F1");
        assert_eq!(expected.load_state, "F4");
        assert_eq!(expected.rewind, "F5");
        assert_eq!(expected.reset, "F2");
        assert_eq!(expected.power_cycle, "F3");
        assert_eq!(expected.debug_overlay, "Backquote");
        assert_eq!(expected.open_rom, "F12");
        assert_eq!(expected.movie_record, "F6");
        assert_eq!(expected.movie_play, "F7");
        assert_eq!(expected.movie_branch, "F8");
        assert_eq!(expected.disk_swap, "F9");
        assert_eq!(expected.insert_coin, "F10");
        assert_eq!(expected.fullscreen, "F11");
        assert_eq!(expected.toggle_menu_bar, "KeyM");
        assert_eq!(expected.fast_forward, "Tab");
        assert_eq!(expected.frame_advance, "Backslash");
        assert_eq!(expected.pause, "Space");
        assert_eq!(expected.speed_up, "Equal");
        assert_eq!(expected.speed_down, "Minus");
        assert_eq!(expected.speed_reset, "Digit0");

        // (d) serialising then re-parsing the registry is a fixed point.
        let s = toml::to_string_pretty(&expected).unwrap();
        let back: SystemBindings = toml::from_str(&s).unwrap();
        assert_eq!(back, expected);
    }

    #[test]
    fn movie_bindings_default_to_f6_f7_f8() {
        // v1.4.0 Sprint 4.2: the TAS movie keys default to F6/F7/F8 and
        // round-trip through TOML.
        let cfg = Config::default();
        assert_eq!(cfg.input.system.movie_record, "F6");
        assert_eq!(cfg.input.system.movie_play, "F7");
        assert_eq!(cfg.input.system.movie_branch, "F8");
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn system_bindings_without_movie_keys_fall_back_to_defaults() {
        // A v1.3.x-era `[input.system]` block lacks the movie keys; the
        // `#[serde(default)]` attributes must fill them in so an older
        // on-disk config keeps loading without manual edits.
        let toml_str = r#"
quit = "Escape"
save_state = "F1"
load_state = "F4"
rewind = "F5"
reset = "F2"
power_cycle = "F3"
debug_overlay = "Backquote"
open_rom = "F12"
"#;
        let sys: SystemBindings = toml::from_str(toml_str).unwrap();
        assert_eq!(sys.movie_record, "F6");
        assert_eq!(sys.movie_play, "F7");
        assert_eq!(sys.movie_branch, "F8");
        // The pre-existing keys still parse.
        assert_eq!(sys.open_rom, "F12");
    }

    #[test]
    fn partial_overrides_apply() {
        let toml_str = r#"
[input.player1]
up = "KeyI"
down = "KeyK"
left = "KeyJ"
right = "KeyL"
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

[input.system]
quit = "Escape"
save_state = "F1"
load_state = "F4"
rewind = "F5"
reset = "F2"
power_cycle = "F3"
debug_overlay = "Backquote"

[rewind]
enabled = false
max_seconds = 30
keyframe_period = 60

[graphics]
present_mode = "Mailbox"

[audio]
sample_rate = 48000
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        assert_eq!(cfg.input.player1.up, "KeyI");
        assert!(!cfg.rewind.enabled);
        assert_eq!(cfg.audio.sample_rate, 48_000);
        assert_eq!(cfg.graphics.present_mode, "Mailbox");
    }

    #[test]
    fn save_then_load_round_trips_through_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.input.player1.up = "KeyW".into();
        cfg.audio.sample_rate = 48_000;
        cfg.save_to(&path).unwrap();
        let back = Config::load_from(&path).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn load_missing_file_returns_io_not_found() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("missing.toml");
        let err = Config::load_from(&path).unwrap_err();
        assert!(matches!(err, ConfigError::Io(_)));
    }

    #[test]
    fn legacy_video_and_keyboard_schema_migrates() {
        // Pre-v1.3.1 schema: [video] vsync + [input.keyboard_p1].
        let legacy = r#"
[video]
vsync = true

[input.keyboard_p1]
up = "KeyI"
down = "KeyK"
left = "KeyJ"
right = "KeyL"
a = "KeyZ"
b = "KeyX"
select = "ShiftRight"
start = "Enter"

[audio]
sample_rate = 48000
"#;
        let (cfg, notes) = Config::migrate_legacy(legacy).expect("legacy schema must be detected");
        // [video] vsync = true -> Fifo.
        assert_eq!(cfg.graphics.present_mode, "Fifo");
        // keyboard_p1 carried into player1.
        assert_eq!(cfg.input.player1.up, "KeyI");
        assert_eq!(cfg.input.player1.left, "KeyJ");
        // sample_rate preserved.
        assert_eq!(cfg.audio.sample_rate, 48_000);
        assert!(notes.iter().any(|n| n.contains("present_mode")));
        assert!(notes.iter().any(|n| n.contains("player1")));
        // The migrated value is valid current-schema TOML.
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn legacy_keycode_values_are_canonicalized_and_resolve() {
        use crate::input::parse_keycode;
        // Pre-v1.3.1 schema with the LEGACY default layout: the keycode
        // VALUES are old winit-0.29 `VirtualKeyCode` Debug names. This is
        // the shape the user's broken config has.
        let legacy = r#"
[input.keyboard_p1]
up = "Up"
down = "Down"
left = "Left"
right = "Right"
a = "Z"
b = "X"
select = "RShift"
start = "Return"

[input.keyboard_p2]
up = "W"
down = "S"
left = "A"
right = "D"
a = "Q"
b = "E"
select = "L"
start = "P"
"#;
        let (cfg, _) = Config::migrate_legacy(legacy).expect("legacy schema must be detected");
        // Canonicalized to current winit-0.30 names...
        assert_eq!(cfg.input.player1.up, "ArrowUp");
        assert_eq!(cfg.input.player1.a, "KeyZ");
        assert_eq!(cfg.input.player1.start, "Enter");
        assert_eq!(cfg.input.player1.select, "ShiftRight");
        assert_eq!(cfg.input.player2.up, "KeyW");
        assert_eq!(cfg.input.player2.start, "KeyP");
        // ...and EVERY migrated keycode value resolves via parse_keycode.
        for v in [
            &cfg.input.player1.up,
            &cfg.input.player1.down,
            &cfg.input.player1.left,
            &cfg.input.player1.right,
            &cfg.input.player1.a,
            &cfg.input.player1.b,
            &cfg.input.player1.select,
            &cfg.input.player1.start,
            &cfg.input.player2.up,
            &cfg.input.player2.down,
            &cfg.input.player2.left,
            &cfg.input.player2.right,
            &cfg.input.player2.a,
            &cfg.input.player2.b,
            &cfg.input.player2.select,
            &cfg.input.player2.start,
        ] {
            assert!(
                parse_keycode(v).is_some(),
                "migrated keycode {v:?} must resolve"
            );
        }
    }

    #[test]
    fn legacy_vsync_false_maps_to_mailbox() {
        let (cfg, _) = Config::migrate_legacy("[video]\nvsync = false\n").unwrap();
        assert_eq!(cfg.graphics.present_mode, "Mailbox");
    }

    #[test]
    fn current_schema_is_not_treated_as_legacy() {
        // A current-schema file (even a fully-populated one) must NOT trigger
        // migration.
        let current = toml::to_string_pretty(&Config::default()).unwrap();
        assert!(Config::migrate_legacy(&current).is_none());
        // Empty file is current-schema (all defaults), not legacy.
        assert!(Config::migrate_legacy("").is_none());
    }

    #[test]
    fn invalid_toml_is_not_migrated() {
        // Syntactically-broken TOML cannot be migrated (the caller's strict
        // parse reports the error and falls back to defaults).
        assert!(Config::migrate_legacy("this is = = not toml").is_none());
    }

    #[test]
    fn config_without_gamepad_sections_yields_default_xbox_layout() {
        use crate::input::GamepadMap;
        use gilrs::Button;
        use rustynes_core::Buttons;
        // v1.6.0 back-compat: a pre-v1.6.0 current-schema config has no
        // `[input.gamepad1]` / `[input.gamepad2]` tables. The `#[serde(default)]`
        // attributes must supply the legacy Xbox layout so behaviour is
        // byte-identical for users who never open the rebind UI.
        let toml_str = r#"
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

[input.system]
quit = "Escape"
save_state = "F1"
load_state = "F4"
rewind = "F5"
reset = "F2"
power_cycle = "F3"
debug_overlay = "Backquote"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        // The gamepad sections defaulted to the Xbox layout...
        assert_eq!(cfg.input.gamepad1, GamepadBindings::default_xbox());
        assert_eq!(cfg.input.gamepad2, GamepadBindings::default_xbox());
        // ...and the resolved per-player map matches the pre-v1.6.0
        // hardcoded `gamepad_button_to_nes` behaviour exactly.
        let map = GamepadMap::from_config(&cfg.input.gamepad1);
        assert_eq!(map.lookup(Button::South), Some(Buttons::A));
        assert_eq!(map.lookup(Button::West), Some(Buttons::B));
        assert_eq!(map.lookup(Button::Start), Some(Buttons::START));
        assert_eq!(map.lookup(Button::Select), Some(Buttons::SELECT));
        assert_eq!(map.lookup(Button::DPadUp), Some(Buttons::UP));
        assert_eq!(map.lookup(Button::DPadDown), Some(Buttons::DOWN));
        assert_eq!(map.lookup(Button::DPadLeft), Some(Buttons::LEFT));
        assert_eq!(map.lookup(Button::DPadRight), Some(Buttons::RIGHT));
        // Unmapped buttons stay unmapped (North/East/triggers).
        assert_eq!(map.lookup(Button::North), None);
    }

    #[test]
    fn config_without_four_score_sections_loads_unchanged() {
        // v1.7.0 back-compat: a pre-v1.7.0 config has no `[input.player3]`
        // / `[input.player4]` / `[input.gamepad3]` / `[input.gamepad4]`
        // tables and no `four_score` flag. The `#[serde(default)]`
        // attributes must supply the default P3/P4 layouts + Four Score
        // off, so behaviour is byte-identical for existing users.
        let toml_str = r#"
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
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        // Four Score off by default — adapter stays dormant.
        assert!(!cfg.input.four_score);
        // P3/P4 keyboard + gamepad maps defaulted in.
        assert_eq!(cfg.input.player3, PadBindings::default_player3());
        assert_eq!(cfg.input.player4, PadBindings::default_player4());
        assert_eq!(cfg.input.gamepad3, GamepadBindings::default_xbox());
        assert_eq!(cfg.input.gamepad4, GamepadBindings::default_xbox());
        // The pre-existing P1/P2 sections are untouched.
        assert_eq!(cfg.input.player1, PadBindings::default_player1());
        assert_eq!(cfg.input.player2, PadBindings::default_player2());
        // The whole tree round-trips, and matches a fresh default (the
        // legacy file is behaviourally identical to today's default).
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn fds_config_defaults_to_no_bios_and_round_trips() {
        // v2.2.0: a fresh config has no FDS BIOS path, and the disk-swap
        // system key defaults to F9. Both must round-trip through TOML.
        let cfg = Config::default();
        assert_eq!(cfg.fds.bios_path, None);
        assert_eq!(cfg.input.system.disk_swap, "F9");
        let s = toml::to_string_pretty(&cfg).unwrap();
        let back: Config = toml::from_str(&s).unwrap();
        assert_eq!(cfg, back);
    }

    #[test]
    fn emulation_fast_dotloop_defaults_on_for_pre_v2_2_3_configs() {
        // The subtle case this guards: `fast_dotloop` is the one `[emulation]`
        // field whose default is `true`, so a plain `#[serde(default)]` (bool ->
        // false) would silently opt every existing user OUT of the fast path on
        // upgrade — a ~11% slowdown, invisible because both paths render
        // identically. Cover all three shapes an on-disk config can take.

        // 1. A pre-v2.2.3 `[emulation]` section that predates the key.
        let older: EmulationConfig = toml::from_str("oam_decay = true\n").unwrap();
        assert!(older.fast_dotloop, "missing key must default ON");
        assert!(older.oam_decay, "sibling keys still parse");

        // 2. No `[emulation]` section at all (pre-v2.1.4).
        let absent: EmulationConfig = toml::from_str("").unwrap();
        assert!(absent.fast_dotloop, "empty section must default ON");

        // 3. An explicit opt-out must survive a round trip — the escape hatch
        //    is worthless if it silently reverts.
        let off = EmulationConfig {
            fast_dotloop: false,
            ..EmulationConfig::default()
        };
        let back: EmulationConfig = toml::from_str(&toml::to_string_pretty(&off).unwrap()).unwrap();
        assert!(!back.fast_dotloop, "explicit opt-out must round-trip");

        // 4. `Default` must agree with serde, and with the core's own default.
        assert!(EmulationConfig::default().fast_dotloop);
        // The accuracy knobs stay off — promotion must not have disturbed them.
        let d = EmulationConfig::default();
        assert!(!d.oam_decay);
        assert!(!d.ppu_oamaddr_corruption);
        assert!(!d.blargg_power_up_palette);
        assert!(!d.randomize_power_on_ram);
        assert_eq!(d.power_on_ram_seed, 0);
    }

    #[test]
    fn config_without_fds_section_loads_unchanged() {
        // v2.2.0 back-compat: a pre-v2.2.0 config has no `[fds]` section and
        // no `disk_swap` system key. The `#[serde(default)]` attributes must
        // supply both so an older on-disk config keeps loading byte-identically.
        let toml_str = r#"
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

[input.system]
quit = "Escape"
save_state = "F1"
load_state = "F4"
rewind = "F5"
reset = "F2"
power_cycle = "F3"
debug_overlay = "Backquote"
"#;
        let cfg: Config = toml::from_str(toml_str).unwrap();
        // No `[fds]` section -> default (no BIOS path).
        assert_eq!(cfg.fds, FdsConfig::default());
        assert!(cfg.fds.bios_path.is_none());
        // No `disk_swap` key -> the F9 default.
        assert_eq!(cfg.input.system.disk_swap, "F9");
        // The rest of the (pre-v2.2.0) config is behaviourally unchanged.
        assert_eq!(cfg, Config::default());
    }

    #[test]
    fn fds_bios_path_persists_through_disk() {
        let tmp = TempDir::new().unwrap();
        let path = tmp.path().join("config.toml");
        let mut cfg = Config::default();
        cfg.fds.bios_path = Some(PathBuf::from("/home/user/disksys.rom"));
        cfg.save_to(&path).unwrap();
        let back = Config::load_from(&path).unwrap();
        assert_eq!(
            back.fds.bios_path,
            Some(PathBuf::from("/home/user/disksys.rom"))
        );
    }

    #[test]
    fn gamepad_default_deadzone_is_half() {
        let cfg = Config::default();
        assert!((cfg.input.gamepad1.axis_deadzone - 0.5).abs() < f32::EPSILON);
        // An older-schema gamepad table lacking `axis_deadzone` fills it
        // in via the field-level `#[serde(default)]`.
        let pad: GamepadBindings = toml::from_str(
            r#"
up = "DPadUp"
down = "DPadDown"
left = "DPadLeft"
right = "DPadRight"
a = "South"
b = "West"
select = "Select"
start = "Start"
"#,
        )
        .unwrap();
        assert!((pad.axis_deadzone - 0.5).abs() < f32::EPSILON);
    }

    // v1.5.0 "Lens" Workstream E (accessibility) — UI zoom + extended themes.

    #[test]
    fn ui_zoom_factor_defaults_to_one_and_round_trips() {
        let cfg = Config::default();
        assert!((cfg.ui.zoom_factor - 1.0).abs() < f32::EPSILON);
        // A config that predates the field (no `zoom_factor` key) fills it via
        // the field-level `#[serde(default)]`, staying byte-identical at 1.0.
        let ui: UiConfig = toml::from_str("theme = \"dark\"\n").unwrap();
        assert!((ui.zoom_factor - 1.0).abs() < f32::EPSILON);
        // Round-trips a non-default value.
        let mut cfg2 = Config::default();
        cfg2.ui.zoom_factor = 1.5;
        let back: Config = toml::from_str(&toml::to_string_pretty(&cfg2).unwrap()).unwrap();
        assert!((back.ui.zoom_factor - 1.5).abs() < f32::EPSILON);
    }

    #[test]
    fn ui_zoom_factor_clamps_to_supported_range() {
        let with_zoom = |z: f32| UiConfig {
            zoom_factor: z,
            ..UiConfig::default()
        };
        assert!((with_zoom(99.0).clamped_zoom_factor() - UiConfig::ZOOM_MAX).abs() < f32::EPSILON);
        assert!((with_zoom(0.01).clamped_zoom_factor() - UiConfig::ZOOM_MIN).abs() < f32::EPSILON);
        assert!((with_zoom(1.25).clamped_zoom_factor() - 1.25).abs() < f32::EPSILON);
    }

    #[test]
    fn ui_zoom_factor_non_finite_falls_back_to_default() {
        // A hand-edited config can put a non-finite value in `zoom_factor`;
        // `NaN`/`Infinity` would propagate into egui layout and panic, so they
        // must fall back to the 1.0 default rather than being clamped.
        let with_zoom = |z: f32| UiConfig {
            zoom_factor: z,
            ..UiConfig::default()
        };
        assert!((with_zoom(f32::NAN).clamped_zoom_factor() - 1.0).abs() < f32::EPSILON);
        assert!((with_zoom(f32::INFINITY).clamped_zoom_factor() - 1.0).abs() < f32::EPSILON);
        assert!((with_zoom(f32::NEG_INFINITY).clamped_zoom_factor() - 1.0).abs() < f32::EPSILON);
    }

    #[test]
    fn extended_themes_serialize_with_stable_keys() {
        // The two original-trip keys are unchanged, and the new accessibility
        // variants use stable, hand-chosen keys (one hyphenated). Guarding the
        // wire format means an old config never silently maps to a new variant.
        for (theme, key) in [
            (AppTheme::Light, "light"),
            (AppTheme::Dark, "dark"),
            (AppTheme::System, "system"),
            (AppTheme::HighContrast, "high-contrast"),
            (AppTheme::Colorblind, "colorblind"),
        ] {
            let toml = toml::to_string(&UiConfig {
                theme,
                ..UiConfig::default()
            })
            .unwrap();
            assert!(
                toml.contains(&format!("theme = \"{key}\"")),
                "theme {theme:?} should serialize as {key:?}; got:\n{toml}"
            );
            let back: UiConfig = toml::from_str(&toml).unwrap();
            assert_eq!(back.theme, theme);
        }
        // `AppTheme::all()` covers every variant exactly once.
        assert_eq!(AppTheme::all().len(), 5);
    }
}
