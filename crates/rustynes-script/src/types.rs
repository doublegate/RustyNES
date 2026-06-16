//! Backend-agnostic host-facing types shared by every VM backend.
//!
//! These are the types the **host** (the frontend) sees and drains, regardless
//! of which Lua VM is compiled in.
//!
//! They carry no VM-specific data, so the mlua (native) and piccolo
//! (experimental wasm) backends both produce + consume the exact same
//! `ControlCmd` / `DrawCmd` / `ScriptError` surface. Keeping them in one place
//! is what lets `app.rs` use `rustynes_script::DrawCmd` etc. with no `#[cfg]`
//! on the backend.

/// Default per-frame VM-instruction budget (a runaway-loop backstop).
///
/// A callback that exceeds this is aborted — surfaced as [`ScriptError::Budget`]
/// on the piccolo backend and [`ScriptError::Lua`] on the mlua backend (an mlua
/// runtime error).
///
/// The host pumps the engine while holding the emulator lock (callbacks need
/// live `Nes` access), so this budget also bounds how long a runaway script can
/// stall emulation. 1M VM instructions is ~10 ms worst case — well above any
/// legitimate per-frame script (real HUD/watch logic is well under 10k
/// instructions/frame), but tight enough that a runaway is cut off within a
/// frame or two rather than freezing the emulator.
pub const DEFAULT_INSTRUCTION_BUDGET: u64 = 1_000_000;

/// Max control / draw commands queued per frame (drained by the host). A script
/// can't grow host memory without bound; excess commands in one frame are
/// dropped.
pub const MAX_QUEUED_CMDS: usize = 8192;

/// Errors from loading or running a script.
#[derive(Debug, thiserror::Error)]
pub enum ScriptError {
    /// The Lua chunk failed to load (syntax error), a callback raised, or the
    /// per-frame instruction budget was exceeded (a Lua runtime error). Carries
    /// the VM's message. (Used by the native mlua backend.)
    #[error("lua error: {0}")]
    Lua(String),
    /// The per-frame fuel/instruction budget was exceeded (the experimental
    /// piccolo backend surfaces the runaway-loop guard as its own variant so
    /// the host can distinguish a runaway from a genuine script error).
    #[error("script exceeded the per-frame instruction budget")]
    Budget,
}

#[cfg(feature = "mlua-backend")]
impl From<mlua::Error> for ScriptError {
    fn from(e: mlua::Error) -> Self {
        Self::Lua(e.to_string())
    }
}

/// A control action a script requested (`emu.pause` / `saveState` / ...).
///
/// Drained by the host after `ScriptEngine::on_frame` and applied to the
/// emulator. Collected (not applied inline) so the host stays the single owner
/// of emulator-control + can gate state-mutating actions.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ControlCmd {
    /// `emu.pause()` — request the host pause emulation.
    Pause,
    /// `emu.saveState(slot)` — save to a numbered slot.
    SaveState(u8),
    /// `emu.loadState(slot)` — load from a numbered slot.
    LoadState(u8),
    /// `emu.setInput(port, buttons)` — override a controller's button bitmask
    /// for the next frame (`port` 0/1; `buttons` is the standard NES bitmask).
    SetInput {
        /// Controller port (0 = P1, 1 = P2).
        port: u8,
        /// Standard NES button bitmask (A,B,Select,Start,Up,Down,Left,Right).
        buttons: u8,
    },
}

/// One overlay draw command (`emu.drawText` / `drawRect` / `drawPixel`).
///
/// Drained by the host each frame and rendered through the egui pass. Pixel
/// coordinates are in NES framebuffer space (256x240). `color` is `0xRRGGBBAA`.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DrawCmd {
    /// Text at `(x, y)`.
    Text {
        /// X (px).
        x: i32,
        /// Y (px).
        y: i32,
        /// `0xRRGGBBAA`.
        color: u32,
        /// The string.
        text: String,
    },
    /// Filled rectangle.
    Rect {
        /// X (px).
        x: i32,
        /// Y (px).
        y: i32,
        /// Width (px).
        w: i32,
        /// Height (px).
        h: i32,
        /// `0xRRGGBBAA`.
        color: u32,
    },
    /// A single pixel.
    Pixel {
        /// X (px).
        x: i32,
        /// Y (px).
        y: i32,
        /// `0xRRGGBBAA`.
        color: u32,
    },
}
