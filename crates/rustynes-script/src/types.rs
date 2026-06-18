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
    /// v1.7.0 "Forge" Workstream B (B3) — `emu.takeScreenshot()` — request the
    /// host write the current framebuffer to a PNG (the host owns the `png`
    /// encoder + the screenshot directory; the script crate stays dep-free).
    /// Read-only side effect (a file write), so it is not gated by the
    /// write-lock (a screenshot can't perturb deterministic state).
    Screenshot,
}

/// v1.7.0 "Forge" Workstream B (B1) — a `TAStudio` editor action a script
/// requested via the `tastudio.*` Lua API.
///
/// Drained by the host (the frontend, which owns the live `TasEditor`) after
/// [`crate::ScriptEngine::on_frame`] and applied to the editor. Collected (not
/// applied inline) for the same reason as [`ControlCmd`]: the script crate has
/// no reference to the frontend's `TasEditor`, so the host stays the single
/// owner of the editor and gates every state-mutating action.
///
/// Every variant here is a **mutator** and is gated IDENTICALLY to `emu.write`
/// at the source — under a locked session (netplay / TAS replay / RA-hardcore)
/// the queue is never appended to, so a script can't perturb a deterministic /
/// replayed run. (`BizHawk` `TAStudioLuaLibrary` model.)
#[derive(Debug, Clone, PartialEq, Eq)]
#[non_exhaustive]
pub enum TasCmd {
    /// `tastudio.setrecording(bool)` / `togglerecording()` — set the editor's
    /// recording mode (`None` toggles it).
    SetRecording(Option<bool>),
    /// `tastudio.setplayback(frame)` — seek the editor cursor to a frame.
    SetPlaybackFrame(usize),
    /// `tastudio.setplayback(markerName)` — seek to a named marker's frame.
    SetPlaybackMarker(String),
    /// `tastudio.setlag(frame, bool)` — override a frame's lag verdict.
    SetLag {
        /// The frame whose lag verdict to set.
        frame: usize,
        /// The new lag verdict.
        lag: bool,
    },
    /// `tastudio.setmarker(frame, text)` — set or rename a marker.
    SetMarker {
        /// The frame to mark.
        frame: usize,
        /// The marker label.
        text: String,
    },
    /// `tastudio.removemarker(frame)` — clear a frame's marker.
    RemoveMarker(usize),
    /// `tastudio.submitinputchange(frame, port, buttons)` /
    /// `applyinputchanges()` — apply one queued atomic input edit. The Lua
    /// `submitinputchange` stages edits engine-side; `applyinputchanges` flushes
    /// them as a batch of these commands so the host re-seeks at most once.
    SetInput {
        /// The frame to edit.
        frame: usize,
        /// Controller port (0 = P1, 1 = P2).
        port: u8,
        /// Standard NES button bitmask for that frame.
        buttons: u8,
    },
    /// `tastudio.loadbranch(index)` — restore a saved branch.
    LoadBranch(usize),
    /// `tastudio.setbranchtext(index, text)` — set a branch's annotation.
    SetBranchText {
        /// The branch index.
        index: usize,
        /// The annotation text.
        text: String,
    },
}

/// v1.7.0 "Forge" Workstream B (B1/B2) — a read-only `TAStudio` editor snapshot.
///
/// Pushed into the engine each frame so the `tastudio.*` query API (`engaged`,
/// `getseekframe`, `islag`, `hasstate`, `getbranches`, ...) resolves against
/// current editor state without the script crate ever referencing the
/// frontend's `TasEditor`.
///
/// The same host-push pattern as [`crate::ScriptEngine::set_symbols`]: read-only
/// on the script side, never deterministic emulator state. When the editor is
/// closed the host pushes the default ([`TasSnapshot::engaged`] is `false`), and
/// every query returns its empty / `nil` form.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TasSnapshot {
    /// `true` when the `TAStudio` editor is open (`tastudio.engaged()`).
    pub engaged: bool,
    /// Whether the editor is in recording mode (`tastudio.getrecording()`).
    pub recording: bool,
    /// The editor cursor / seek frame (`tastudio.getseekframe()`).
    pub seek_frame: usize,
    /// The current piano-roll selection as `(first, last)` inclusive frame
    /// range, or `None` when nothing is selected (`tastudio.getselection()`).
    pub selection: Option<(usize, usize)>,
    /// Per-frame lag verdicts for the played prefix (`tastudio.islag(f)`):
    /// `lag[f]` is `Some(true)` for a lag frame, `Some(false)` otherwise, and
    /// indices past the end read back `nil`.
    pub lag: Vec<bool>,
    /// Frames that currently hold a greenzone save-state (`tastudio.hasstate`).
    pub state_frames: Vec<usize>,
    /// `(frame, label)` markers in ascending frame order
    /// (`tastudio.getmarker`).
    pub markers: Vec<(usize, String)>,
    /// Per-branch `(frame, text)` metadata (`tastudio.getbranches` /
    /// `getbranchtext`).
    pub branches: Vec<TasBranchInfo>,
    /// The input log length, so `getbranchinput` / cursor math can bound-check.
    pub input_len: usize,
}

/// v1.7.0 "Forge" Workstream B (B1) — per-branch metadata in a [`TasSnapshot`].
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TasBranchInfo {
    /// The frame the branch was forked at.
    pub frame: usize,
    /// The branch's annotation text (`tastudio.getbranchtext`).
    pub text: String,
    /// The branch's input log as `(p1, p2)` button bitmasks per frame, so
    /// `tastudio.getbranchinput(index, frame)` resolves without a re-push.
    pub input: Vec<(u8, u8)>,
}

/// v1.7.0 "Forge" Workstream B (B2) — what a `tastudio.onqueryitem*` callback
/// returned for one piano-roll cell, drained by the host to paint the grid.
///
/// Pure overlay: the host renders these over the piano-roll `TableBuilder`; no
/// callback here can mutate emulator or editor state (the analysis-canvas
/// contract).
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct TasCellDecor {
    /// Background colour `0xRRGGBBAA` (`onqueryitembg`), or `None` to keep the
    /// default row colour.
    pub bg: Option<u32>,
    /// Replacement cell text (`onqueryitemtext`), or `None` for the default.
    pub text: Option<String>,
    /// An icon key (`onqueryitemicon`) the host's icon cache resolves, or
    /// `None`.
    pub icon: Option<String>,
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
