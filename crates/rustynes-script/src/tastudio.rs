//! v1.7.0 "Forge" Workstream B (B1/B2) — the `tastudio.*` Lua surface.
//!
//! v1.6.0 built the `TAStudio` *editor* (the piano-roll model: input log +
//! greenzone + lag log + markers + branches, in `rustynes-frontend`); this
//! module makes it **programmable** — the `BizHawk` `TAStudioLuaLibrary` model —
//! so bots, generated-input TASes, and analysis mods can drive and annotate it.
//!
//! ## How it bridges two crates without a dependency cycle
//!
//! The live `TasEditor` lives in `rustynes-frontend`, which `rustynes-script`
//! must NOT depend on. So this surface uses the exact two host-mediated
//! patterns the rest of the engine already uses:
//!
//! - **Queries** (`engaged` / `getseekframe` / `islag` / `hasstate` /
//!   `getbranches` / ...) read a [`crate::TasSnapshot`] the host pushes each
//!   frame (the `set_symbols` host-push pattern). Read-only, never deterministic
//!   state.
//! - **Mutators** (`setrecording` / `setplayback` / `submitinputchange` /
//!   `applyinputchanges` / marker + branch edits) queue a [`crate::TasCmd`] the
//!   host drains and applies (the `ControlCmd` queue pattern). Every mutator is
//!   **gated IDENTICALLY to `emu.write`**: under a locked session
//!   (netplay / TAS replay / RA-hardcore, surfaced via `set_writes_locked`) the
//!   queue is never appended to, so a script cannot perturb a deterministic /
//!   replayed run.
//! - **B2 callbacks** (`onqueryitembg|text|icon` + `clearIconCache`,
//!   `ongreenzoneinvalidated`, `onbranchload`) are stored Rust-side as registry
//!   keys (never script-visible, like every other callback). The cell-query
//!   callbacks are pure overlay (they return a colour / text / icon the host
//!   paints over the grid; they cannot mutate state); the event callbacks are
//!   observational. The host invokes them through the
//!   [`crate::ScriptEngine::query_tas_cell`] / `fire_*` entry points.
//!
//! Everything here is native-only (the mlua backend), the same carve-out as the
//! dev/TAS `memory`/`cart`/`sym`/driving surface (ADR 0012): the experimental
//! piccolo wasm backend inherits the default no-ops on the [`crate::VmBackend`]
//! trait.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use mlua::{Function, Lua, RegistryKey, Table, Value};

use crate::types::{MAX_QUEUED_CMDS, TasCellDecor, TasCmd, TasSnapshot};

/// All shared engine-side state backing the `tastudio` Lua table. Held by the
/// mlua backend and cloned (`Rc`) into the table's closures + the host entry
/// points. Self-contained so the backend only stores one field for the whole
/// Workstream-B `TAStudio` surface.
#[derive(Clone)]
pub struct TasState {
    /// The host-pushed read-only editor snapshot (refreshed each frame).
    pub snapshot: Rc<RefCell<TasSnapshot>>,
    /// Editor actions queued this frame (drained + gated + applied by the host).
    pub commands: Rc<RefCell<Vec<TasCmd>>>,
    /// Staged input edits from `submitinputchange`, flushed to [`Self::commands`]
    /// as a batch by `applyinputchanges` (the `BizHawk` atomic-edit pattern), so
    /// the host re-seeks at most once per apply.
    pub pending_input: Rc<RefCell<Vec<TasCmd>>>,
    /// `onqueryitembg(fn)` callbacks (Lua registry keys; Rust-side).
    pub query_bg: Rc<RefCell<Vec<RegistryKey>>>,
    /// `onqueryitemtext(fn)` callbacks.
    pub query_text: Rc<RefCell<Vec<RegistryKey>>>,
    /// `onqueryitemicon(fn)` callbacks.
    pub query_icon: Rc<RefCell<Vec<RegistryKey>>>,
    /// `ongreenzoneinvalidated(fn)` callbacks (observational).
    pub greenzone_cbs: Rc<RefCell<Vec<RegistryKey>>>,
    /// `onbranchload(fn)` callbacks (observational).
    pub branch_load_cbs: Rc<RefCell<Vec<RegistryKey>>>,
    /// Set by `tastudio.clearIconCache()`, taken by the host.
    pub clear_icon_cache: Rc<Cell<bool>>,
}

impl TasState {
    /// Fresh, empty state (no editor pushed, no callbacks, no queued commands).
    pub fn new() -> Self {
        Self {
            snapshot: Rc::new(RefCell::new(TasSnapshot::default())),
            commands: Rc::new(RefCell::new(Vec::new())),
            pending_input: Rc::new(RefCell::new(Vec::new())),
            query_bg: Rc::new(RefCell::new(Vec::new())),
            query_text: Rc::new(RefCell::new(Vec::new())),
            query_icon: Rc::new(RefCell::new(Vec::new())),
            greenzone_cbs: Rc::new(RefCell::new(Vec::new())),
            branch_load_cbs: Rc::new(RefCell::new(Vec::new())),
            clear_icon_cache: Rc::new(Cell::new(false)),
        }
    }

    /// `true` if any cell-query callback is registered.
    pub fn needs_cell_query(&self) -> bool {
        !self.query_bg.borrow().is_empty()
            || !self.query_text.borrow().is_empty()
            || !self.query_icon.borrow().is_empty()
    }
}

/// Push a TAS command into the host-drained queue unless it is at the per-frame
/// cap (the same backstop as the `emu.*` control queue).
fn push_capped(q: &Rc<RefCell<Vec<TasCmd>>>, cmd: TasCmd) {
    let mut q = q.borrow_mut();
    if q.len() < MAX_QUEUED_CMDS {
        q.push(cmd);
    }
}

/// Register a colon-call callback `fn` (the leading `self`/table arg ignored)
/// into a Rust-side registry-key list, so a script can register but never
/// inspect / clobber the registry — the same hardening as every `emu.*`
/// callback.
fn install_cb_registrar(
    lua: &Lua,
    table: &Table,
    name: &str,
    list: &Rc<RefCell<Vec<RegistryKey>>>,
) -> mlua::Result<()> {
    let list = Rc::clone(list);
    table.set(
        name,
        lua.create_function(move |lua, (_this, f): (Value, Function)| {
            list.borrow_mut().push(lua.create_registry_value(f)?);
            Ok(())
        })?,
    )
}

/// Install the persistent `tastudio` global table (the colon-call convention,
/// like `memory:` / `cart:` / `sym:`). All accessors close over `state` + the
/// shared `writes_locked` cell; the query accessors read the host-pushed
/// snapshot, the mutators queue gated [`TasCmd`]s.
#[allow(clippy::too_many_lines)] // one create_function per API entry.
pub fn install(lua: &Lua, state: &TasState, writes_locked: &Rc<Cell<bool>>) -> mlua::Result<()> {
    let t = lua.create_table()?;

    // ---- Queries (read the host-pushed snapshot; never deterministic) ----

    let snap = Rc::clone(&state.snapshot);
    t.set(
        "engaged",
        lua.create_function(move |_, _this: Value| Ok(snap.borrow().engaged))?,
    )?;
    let snap = Rc::clone(&state.snapshot);
    t.set(
        "getrecording",
        lua.create_function(move |_, _this: Value| Ok(snap.borrow().recording))?,
    )?;
    let snap = Rc::clone(&state.snapshot);
    t.set(
        "getseekframe",
        lua.create_function(move |_, _this: Value| {
            Ok(u64::try_from(snap.borrow().seek_frame).unwrap_or(0))
        })?,
    )?;
    // `getselection()` -> (first, last) or (nil, nil) when nothing is selected.
    let snap = Rc::clone(&state.snapshot);
    t.set(
        "getselection",
        lua.create_function(move |_, _this: Value| {
            Ok(snap
                .borrow()
                .selection
                .map_or((None, None), |(a, b)| (Some(a as u64), Some(b as u64))))
        })?,
    )?;
    // `islag(frame)` -> bool, or nil for a frame not yet emulated.
    let snap = Rc::clone(&state.snapshot);
    t.set(
        "islag",
        lua.create_function(move |_, (_this, frame): (Value, usize)| {
            Ok(snap.borrow().lag.get(frame).copied())
        })?,
    )?;
    // `hasstate(frame)` -> bool (a greenzone save-state exists at `frame`).
    let snap = Rc::clone(&state.snapshot);
    t.set(
        "hasstate",
        lua.create_function(move |_, (_this, frame): (Value, usize)| {
            Ok(snap.borrow().state_frames.contains(&frame))
        })?,
    )?;
    // `getmarker(frame)` -> label or nil.
    let snap = Rc::clone(&state.snapshot);
    t.set(
        "getmarker",
        lua.create_function(move |_, (_this, frame): (Value, usize)| {
            Ok(snap
                .borrow()
                .markers
                .iter()
                .find(|(f, _)| *f == frame)
                .map(|(_, l)| l.clone()))
        })?,
    )?;
    // `getbranches()` -> array of { frame=, text= } in branch order.
    let snap = Rc::clone(&state.snapshot);
    t.set(
        "getbranches",
        lua.create_function(move |lua, _this: Value| {
            let s = snap.borrow();
            let arr = lua.create_table()?;
            for (i, b) in s.branches.iter().enumerate() {
                let entry = lua.create_table()?;
                entry.set("frame", u64::try_from(b.frame).unwrap_or(0))?;
                entry.set("text", b.text.clone())?;
                arr.set(i + 1, entry)?;
            }
            Ok(arr)
        })?,
    )?;
    // `getbranchtext(index)` -> text or nil (1-based, matching Lua arrays).
    let snap = Rc::clone(&state.snapshot);
    t.set(
        "getbranchtext",
        lua.create_function(move |_, (_this, index): (Value, usize)| {
            Ok(snap
                .borrow()
                .branches
                .get(index.wrapping_sub(1))
                .map(|b| b.text.clone()))
        })?,
    )?;
    // `getbranchinput(index, frame)` -> (p1, p2) button bitmasks or (nil, nil).
    let snap = Rc::clone(&state.snapshot);
    t.set(
        "getbranchinput",
        lua.create_function(move |_, (_this, index, frame): (Value, usize, usize)| {
            Ok(snap
                .borrow()
                .branches
                .get(index.wrapping_sub(1))
                .and_then(|b| b.input.get(frame).copied())
                .map_or((None, None), |(p1, p2)| (Some(p1), Some(p2))))
        })?,
    )?;

    // ---- Mutators (queue a gated TasCmd; dropped under a locked session) ----

    let cmds = Rc::clone(&state.commands);
    let locked = Rc::clone(writes_locked);
    t.set(
        "setrecording",
        lua.create_function(move |_, (_this, on): (Value, bool)| {
            if !locked.get() {
                push_capped(&cmds, TasCmd::SetRecording(Some(on)));
            }
            Ok(())
        })?,
    )?;
    let cmds = Rc::clone(&state.commands);
    let locked = Rc::clone(writes_locked);
    t.set(
        "togglerecording",
        lua.create_function(move |_, _this: Value| {
            if !locked.get() {
                push_capped(&cmds, TasCmd::SetRecording(None));
            }
            Ok(())
        })?,
    )?;
    // `setplayback(target)` accepts a frame number OR a marker name (string).
    let cmds = Rc::clone(&state.commands);
    let locked = Rc::clone(writes_locked);
    t.set(
        "setplayback",
        lua.create_function(move |_, (_this, target): (Value, Value)| {
            if !locked.get() {
                let cmd = match target {
                    Value::Integer(n) => {
                        Some(TasCmd::SetPlaybackFrame(usize::try_from(n).unwrap_or(0)))
                    }
                    Value::Number(n) if n >= 0.0 =>
                    {
                        #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                        Some(TasCmd::SetPlaybackFrame(n as usize))
                    }
                    Value::String(s) => Some(TasCmd::SetPlaybackMarker(s.to_string_lossy())),
                    _ => None,
                };
                if let Some(cmd) = cmd {
                    push_capped(&cmds, cmd);
                }
            }
            Ok(())
        })?,
    )?;
    let cmds = Rc::clone(&state.commands);
    let locked = Rc::clone(writes_locked);
    t.set(
        "setlag",
        lua.create_function(move |_, (_this, frame, lag): (Value, usize, bool)| {
            if !locked.get() {
                push_capped(&cmds, TasCmd::SetLag { frame, lag });
            }
            Ok(())
        })?,
    )?;
    let cmds = Rc::clone(&state.commands);
    let locked = Rc::clone(writes_locked);
    t.set(
        "setmarker",
        lua.create_function(move |_, (_this, frame, text): (Value, usize, String)| {
            if !locked.get() {
                push_capped(&cmds, TasCmd::SetMarker { frame, text });
            }
            Ok(())
        })?,
    )?;
    let cmds = Rc::clone(&state.commands);
    let locked = Rc::clone(writes_locked);
    t.set(
        "removemarker",
        lua.create_function(move |_, (_this, frame): (Value, usize)| {
            if !locked.get() {
                push_capped(&cmds, TasCmd::RemoveMarker(frame));
            }
            Ok(())
        })?,
    )?;
    // `submitinputchange(frame, port, buttons)` STAGES an edit; it does NOT
    // touch the editor until `applyinputchanges()` flushes the batch. The stage
    // is gated too (no staging under a locked session), so a locked session
    // never accumulates pending edits.
    let pending = Rc::clone(&state.pending_input);
    let locked = Rc::clone(writes_locked);
    t.set(
        "submitinputchange",
        lua.create_function(
            move |_, (_this, frame, port, buttons): (Value, usize, u8, u8)| {
                if !locked.get() {
                    let mut p = pending.borrow_mut();
                    if p.len() < MAX_QUEUED_CMDS {
                        p.push(TasCmd::SetInput {
                            frame,
                            port,
                            buttons,
                        });
                    }
                }
                Ok(())
            },
        )?,
    )?;
    // `applyinputchanges()` flushes the staged edits into the host command queue
    // as one batch (still gated — under a lock the staged list is empty anyway).
    let pending = Rc::clone(&state.pending_input);
    let cmds = Rc::clone(&state.commands);
    let locked = Rc::clone(writes_locked);
    t.set(
        "applyinputchanges",
        lua.create_function(move |_, _this: Value| {
            if !locked.get() {
                let batch = std::mem::take(&mut *pending.borrow_mut());
                for cmd in batch {
                    push_capped(&cmds, cmd);
                }
            }
            Ok(())
        })?,
    )?;
    let cmds = Rc::clone(&state.commands);
    let locked = Rc::clone(writes_locked);
    t.set(
        "loadbranch",
        lua.create_function(move |_, (_this, index): (Value, usize)| {
            if !locked.get() {
                push_capped(&cmds, TasCmd::LoadBranch(index));
            }
            Ok(())
        })?,
    )?;
    let cmds = Rc::clone(&state.commands);
    let locked = Rc::clone(writes_locked);
    t.set(
        "setbranchtext",
        lua.create_function(move |_, (_this, index, text): (Value, usize, String)| {
            if !locked.get() {
                push_capped(&cmds, TasCmd::SetBranchText { index, text });
            }
            Ok(())
        })?,
    )?;

    // ---- B2 callbacks (Rust-side registry keys; not script-visible) ----

    install_cb_registrar(lua, &t, "onqueryitembg", &state.query_bg)?;
    install_cb_registrar(lua, &t, "onqueryitemtext", &state.query_text)?;
    install_cb_registrar(lua, &t, "onqueryitemicon", &state.query_icon)?;
    install_cb_registrar(lua, &t, "ongreenzoneinvalidated", &state.greenzone_cbs)?;
    install_cb_registrar(lua, &t, "onbranchload", &state.branch_load_cbs)?;

    let clear = Rc::clone(&state.clear_icon_cache);
    t.set(
        "clearIconCache",
        lua.create_function(move |_, _this: Value| {
            clear.set(true);
            Ok(())
        })?,
    )?;

    lua.globals().set("tastudio", &t)?;
    Ok(())
}

/// Invoke the registered `onqueryitem*` callbacks for one cell `(frame, column)`
/// and fold their returns into a [`TasCellDecor`] the host paints. Pure
/// overlay: callbacks return a value, they never mutate state. The last
/// non-`nil` return wins for each facet (so a later registration can override).
pub fn query_cell(
    lua: &Lua,
    state: &TasState,
    frame: usize,
    column: u32,
) -> mlua::Result<TasCellDecor> {
    let mut decor = TasCellDecor::default();
    let f = u64::try_from(frame).unwrap_or(0);
    // bg -> u32 colour (0xRRGGBBAA).
    for key in state.query_bg.borrow().iter() {
        let cb: Function = lua.registry_value(key)?;
        if let Some(c) = cb.call::<Option<u32>>((f, column))? {
            decor.bg = Some(c);
        }
    }
    // text -> string.
    for key in state.query_text.borrow().iter() {
        let cb: Function = lua.registry_value(key)?;
        if let Some(s) = cb.call::<Option<String>>((f, column))? {
            decor.text = Some(s);
        }
    }
    // icon -> string key.
    for key in state.query_icon.borrow().iter() {
        let cb: Function = lua.registry_value(key)?;
        if let Some(s) = cb.call::<Option<String>>((f, column))? {
            decor.icon = Some(s);
        }
    }
    Ok(decor)
}

/// Invoke every registered callback in `list`, passing a single `usize` arg
/// (`ongreenzoneinvalidated(firstFrame)` / `onbranchload(index)`). Observational.
pub fn fire_event(lua: &Lua, list: &Rc<RefCell<Vec<RegistryKey>>>, arg: usize) -> mlua::Result<()> {
    // Collect the handles up front so the RefCell borrow is released before any
    // callback runs (a callback could register another).
    let fns: Vec<Function> = list
        .borrow()
        .iter()
        .map(|k| lua.registry_value::<Function>(k))
        .collect::<mlua::Result<_>>()?;
    let a = u64::try_from(arg).unwrap_or(0);
    for f in fns {
        f.call::<()>(a)?;
    }
    Ok(())
}
