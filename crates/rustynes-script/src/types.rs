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
    /// Per-frame lag verdicts for the played prefix, one `bool` per emulated
    /// frame: `lag[f] == true` is a lag frame, `false` otherwise. The Lua
    /// `tastudio.islag(f)` accessor maps this to `Some(true)` / `Some(false)`
    /// for an in-range frame and `nil` for an index past the end (the `.get(f)`
    /// miss), so the Lua-visible verdict is tri-state while this field is a
    /// dense `bool` vector over the played prefix.
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

/// v1.7.0 "Forge" Workstream E1 — a host-mediated IPC request a script issued
/// via the `comm.*` table.
///
/// The defining property: **the script never gets a raw socket.** It issues one
/// of these high-level, fully-marshalled requests, the **host** (the frontend's
/// [`crate`]-external `script_host`) owns the actual TCP / HTTP / WebSocket /
/// memory-mapped-file connection, performs the I/O off the emulator lock, and
/// feeds the marshalled bytes/strings back via [`CommResult`]. This preserves
/// the Lua sandbox guarantee (no `io` / `os` / `package` / net) — the VM only
/// ever sees plain Lua values.
///
/// `comm.*` is a NEW non-deterministic input/output source, so every variant is
/// gated like `emu.write`: when the host sets
/// [`crate::ScriptEngine::set_writes_locked`] (netplay / TAS replay / record /
/// RA-hardcore) the command is dropped at the source and never queued. The core
/// synthesis never sees a `CommCmd`.
///
/// Only present when the crate's `script-ipc` feature is enabled; with the
/// feature off the `comm` table is not installed and this type is unused.
#[cfg(feature = "script-ipc")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommCmd {
    /// `comm.socketServerSend(data)` — send a string over the host's configured
    /// outbound TCP socket. Fire-and-forget (no per-call result).
    SocketSend(Vec<u8>),
    /// `comm.httpGet(url)` — issue an HTTP GET. The host fulfils it and pushes a
    /// [`CommResult::Http`] tagged with `id`.
    HttpGet {
        /// Correlation id the host echoes back in the [`CommResult`].
        id: u64,
        /// Target URL (host-validated; the script never opens a connection).
        url: String,
    },
    /// `comm.httpPost(url, body)` — issue an HTTP POST with `body`.
    HttpPost {
        /// Correlation id the host echoes back in the [`CommResult`].
        id: u64,
        /// Target URL (host-validated).
        url: String,
        /// Request body.
        body: String,
    },
    /// `comm.ws_open(url)` — open a WebSocket. The host owns the connection and
    /// reports readiness via [`CommResult::WsState`].
    WsOpen {
        /// Correlation id the host echoes back.
        id: u64,
        /// WebSocket URL.
        url: String,
    },
    /// `comm.ws_send(text)` — send a text frame over the open WebSocket.
    WsSend(String),
    /// `comm.ws_close()` — close the open WebSocket.
    WsClose,
    /// `comm.mmfWrite(name, data)` — write `data` into the named memory-mapped
    /// file the host owns.
    MmfWrite {
        /// Host-side memory-mapped-file identifier.
        name: String,
        /// Bytes to write.
        data: Vec<u8>,
    },
    /// `comm.mmfRead(name, len)` — request up to `len` bytes from the named MMF;
    /// the host pushes a [`CommResult::Mmf`] tagged with `id`.
    MmfRead {
        /// Correlation id the host echoes back.
        id: u64,
        /// Host-side memory-mapped-file identifier.
        name: String,
        /// Max bytes to read.
        len: u32,
    },
}

/// v1.7.0 "Forge" Workstream E1 — a result the **host** injects back into the
/// engine in response to an asynchronous [`CommCmd`].
///
/// The host performs the I/O off the emulator lock, then calls
/// [`crate::ScriptEngine::push_comm_result`] to deliver the marshalled value. On
/// the next pump the engine surfaces it to the script (e.g. via a polled
/// `comm.receive()` queue). The script only ever sees these plain values, never
/// a connection handle.
#[cfg(feature = "script-ipc")]
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum CommResult {
    /// Response to an [`CommCmd::HttpGet`] / [`CommCmd::HttpPost`].
    Http {
        /// The correlation id from the request.
        id: u64,
        /// HTTP status code (0 = host-side transport error).
        status: u16,
        /// Response body (empty on error).
        body: String,
    },
    /// A WebSocket lifecycle / inbound-message event.
    WsState {
        /// The correlation id from [`CommCmd::WsOpen`].
        id: u64,
        /// `true` once the socket is open, `false` on close/error.
        open: bool,
        /// An inbound text frame, if this event carries one.
        message: Option<String>,
    },
    /// Response to a [`CommCmd::MmfRead`].
    Mmf {
        /// The correlation id from the request.
        id: u64,
        /// The bytes read (empty on error).
        data: Vec<u8>,
    },
}

/// v1.7.0 "Forge" Workstream E2 — a host-automation verb a script issued via the
/// `client.*` table.
///
/// Like [`ControlCmd`], these are **collected, never applied inline** — the host
/// stays the single owner of window / tool / capture / cheat state and drains
/// them after [`crate::ScriptEngine::on_frame`]. State-changing verbs
/// (`RebootCore`, `AddCheat`, `RemoveCheat`) are gated like `emu.write`:
/// dropped at the source under a locked session. The observational verbs
/// (`Screenshot`, `SetWindowSize`, `SpeedMode`, `FrameSkip`, `PauseAv` …) are
/// host-presentation only and never perturb the deterministic core.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ClientCmd {
    /// `client.opentool(name)` — open a named tool window (debugger, cheats,
    /// tastudio, …). The host maps the name; an unknown name is a no-op.
    OpenTool(String),
    /// `client.screenshot()` — capture the current framebuffer to a file.
    Screenshot,
    /// `client.screenshottoclipboard()` — capture to the system clipboard.
    ScreenshotToClipboard,
    /// `client.setwindowsize(scale)` — set the integer window scale.
    SetWindowSize(u32),
    /// `client.speedmode(pct)` — set the emulation speed as a percentage
    /// (`100` = realtime). Presentation-only; never alters per-frame output.
    SpeedMode(u32),
    /// `client.frameskip(n)` — set the render frame-skip count.
    FrameSkip(u32),
    /// `client.reboot_core()` — power-cycle the running ROM. Gated like
    /// `emu.write` (it perturbs the run).
    RebootCore,
    /// `client.pause_av()` — pause A/V recording.
    PauseAv,
    /// `client.unpause_av()` — resume A/V recording.
    UnpauseAv,
    /// `client.addcheat(code)` — add a Game Genie code. Gated like `emu.write`.
    AddCheat(String),
    /// `client.removecheat(code)` — remove a Game Genie code. Gated like
    /// `emu.write`.
    RemoveCheat(String),
}
