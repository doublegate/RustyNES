//! Native Lua 5.4 backend (vendored [`mlua`]).
//!
//! This is the reference backend and the only one compiled on native targets
//! (behind the crate's default `mlua-backend` feature, which the frontend's
//! native `scripting` feature pulls in). It is byte-identical to the v1.1.0
//! engine — the code below was lifted verbatim from the original `lib.rs`, with
//! only the shared host-facing types (`ControlCmd` / `DrawCmd` / `ScriptError`
//! / `DEFAULT_INSTRUCTION_BUDGET`) hoisted into `crate::types`.
//!
//! See the module doc on [`crate::backend`] for the contract.

use std::cell::RefCell;
use std::collections::HashMap;

use mlua::{Function, HookTriggers, Lua, RegistryKey, StdLib, Table, VmState};
use rustynes_core::Nes;

use crate::backend::VmBackend;
use crate::tastudio::{self, TasState};
use crate::types::{
    ClientCmd, ControlCmd, DEFAULT_INSTRUCTION_BUDGET, DrawCmd, MAX_QUEUED_CMDS, ScriptError,
    TasCellDecor, TasCmd, TasSnapshot,
};
#[cfg(feature = "script-ipc")]
use crate::types::{CommCmd, CommResult};
use crate::{Shared, SharedCounter, SharedFlag};

/// Max inclusive address span a single value-modifying `emu.addMemoryCallback`
/// may cover. The implementation registers one Lua registry value per address,
/// so an unbounded range (up to 64K) would allocate 64K registry entries; this
/// caps it. A real watchpoint spans a handful of addresses (4 KiB is already far
/// beyond any legitimate scriptable-cheat); a whole-RAM watch belongs on the
/// observational `onWrite` hook, which stores one key for the whole range.
const MAX_MEMORY_CALLBACK_SPAN: u32 = 4096;

/// A stack-allocated bitset over the 16-bit CPU address space (8 KiB, no heap),
/// used to gate the hot per-frame replay loops: membership is an O(1) bit test
/// with no `RefCell` borrow or allocation per event (gemini #58).
struct AddrBits([u64; 1024]);

impl AddrBits {
    /// Build a bitset of the keys registered in `map` (one cheap borrow).
    fn from_keys(map: &AddrCallbacks) -> Self {
        let mut bits = [0u64; 1024];
        for &addr in map.borrow().keys() {
            // `>> 6` / `& 63` (not `/ 64` / `% 64`) — idiomatic + no div/mod even
            // in debug builds (gemini #59).
            bits[usize::from(addr >> 6)] |= 1u64 << (addr & 63);
        }
        Self(bits)
    }

    /// Build a bitset only if `map` has any entries (skips the 8 KiB zero-init
    /// when the corresponding callback type is unused — gemini/Copilot #59).
    fn from_keys_opt(map: &AddrCallbacks) -> Option<Self> {
        if map.borrow().is_empty() {
            None
        } else {
            Some(Self::from_keys(map))
        }
    }

    #[inline]
    fn contains(&self, addr: u16) -> bool {
        self.0[usize::from(addr >> 6)] & (1u64 << (addr & 63)) != 0
    }
}

/// A set of callbacks registered against CPU addresses (`onExec`/`onRead`/
/// `onWrite`), stored as Lua registry keys so they live entirely Rust-side —
/// **not** in a script-visible global. A script therefore cannot inspect,
/// clobber, or inject junk into the registry, which removes the whole class of
/// "malformed registry value crashes the host pump" issues at the source.
type AddrCallbacks = Shared<HashMap<u16, Vec<RegistryKey>>>;

/// Push `cmd` into a host-drained queue unless it is already at the per-frame
/// cap.
fn push_capped<T>(q: &Shared<Vec<T>>, cmd: T) {
    let mut q = q.borrow_mut();
    if q.len() < MAX_QUEUED_CMDS {
        q.push(cmd);
    }
}

/// Side-effect-free read of `len` CPU-space bytes starting at `addr` (wrapping
/// the 16-bit space). Shared by `emu.readRange` and `memory:read_range` (B1).
/// `len` is capped to the 64 KiB address space so an unbounded request can't OOM
/// the host; `wrapping_add` avoids a debug-build overflow panic.
fn read_range_cpu(nes_cell: &RefCell<&mut Nes>, addr: u32, len: u32) -> mlua::Result<Vec<u8>> {
    if len > 0x1_0000 {
        return Err(mlua::Error::RuntimeError(
            "read range length cannot exceed 65536".into(),
        ));
    }
    let mut out = Vec::with_capacity(len as usize);
    let mut nes = nes_cell.borrow_mut();
    for i in 0..len {
        #[allow(clippy::cast_possible_truncation)]
        let a = (addr.wrapping_add(i) & 0xFFFF) as u16;
        out.push(nes.peek(a));
    }
    Ok(out)
}

/// Resolve the registry-key callbacks registered at `addr` into live Lua
/// `Function` handles (empty when none). Collecting up front releases the
/// `RefCell` borrow before any callback runs, so a callback that registers a
/// new one can't trip a re-borrow.
fn fns_at(lua: &Lua, map: &AddrCallbacks, addr: u16) -> mlua::Result<Vec<Function>> {
    let borrow = map.borrow();
    borrow.get(&addr).map_or_else(
        || Ok(Vec::new()),
        |keys| {
            keys.iter()
                .map(|k| lua.registry_value::<Function>(k))
                .collect()
        },
    )
}

/// v1.7.0 "Forge" Workstream B (B3) — invoke every registered callback in a
/// flat event list, passing a single numeric arg. Collects the live `Function`
/// handles up front so the `RefCell` borrow is released before any callback
/// runs (a callback could register another). Shared by the `addEventCallback`
/// per-frame events and the `stateLoaded`/`stateSaved` synchronous events.
fn fire_event_list(lua: &Lua, list: &Shared<Vec<RegistryKey>>, arg: u64) -> mlua::Result<()> {
    let fns: Vec<Function> = list
        .borrow()
        .iter()
        .map(|k| lua.registry_value::<Function>(k))
        .collect::<mlua::Result<_>>()?;
    for f in fns {
        f.call::<()>(arg)?;
    }
    Ok(())
}

/// Drop the active driving coroutine (B2): clear the slot and free its Lua
/// registry value so a finished / errored driver is no longer resumed.
fn drop_driver(lua: &Lua, driver: &Shared<Option<RegistryKey>>) -> mlua::Result<()> {
    // Take the key, dropping the lock guard BEFORE the registry call (a
    // `MutexGuard` held across the `if let` body would needlessly hold the lock;
    // unlike `RefCell`'s `Ref`, clippy flags the held guard — and we never want
    // the lock held across re-entrant Lua work).
    let key = driver.borrow_mut().take();
    if let Some(key) = key {
        lua.remove_registry_value(key)?;
    }
    Ok(())
}

/// Resolve the `onFrame` registry keys into live `Function` handles (collected
/// up front so the `RefCell` borrow is released before any callback runs).
fn fns_for_frame(lua: &Lua, frame: &Shared<Vec<RegistryKey>>) -> mlua::Result<Vec<Function>> {
    frame
        .borrow()
        .iter()
        .map(|k| lua.registry_value::<Function>(k))
        .collect()
}

/// A sandboxed Lua scripting engine bound to one emulator session.
pub struct MluaBackend {
    lua: Lua,
    /// Captured `print` / `emu.log` output, drained by the host for display.
    log: Shared<Vec<String>>,
    /// Control actions a script requested this frame (drained by the host).
    controls: Shared<Vec<ControlCmd>>,
    /// Overlay draw commands a script issued this frame (drained by the host).
    draws: Shared<Vec<DrawCmd>>,
    /// `onFrame` callbacks (Lua registry keys; Rust-side, not script-visible).
    frame_cbs: Shared<Vec<RegistryKey>>,
    /// `onExec(addr, fn)` callbacks, keyed by CPU address.
    exec_cbs: AddrCallbacks,
    /// `onRead(addr, fn)` callbacks, keyed by CPU address.
    read_cbs: AddrCallbacks,
    /// `onWrite(addr, fn)` callbacks, keyed by CPU address.
    write_cbs: AddrCallbacks,
    /// `onNmi(fn)` callbacks (Lua registry keys; Rust-side, not script-visible).
    /// Output-only; replayed once per NMI service entry in the interrupt log.
    nmi_cbs: Shared<Vec<RegistryKey>>,
    /// `onIrq(fn)` callbacks (Lua registry keys; Rust-side, not script-visible).
    /// Output-only; replayed once per IRQ/BRK service entry in the interrupt log.
    irq_cbs: Shared<Vec<RegistryKey>>,
    /// Per-frame instruction counter (reset each `on_frame`); the VM hook
    /// trips a Lua runtime error when it crosses `budget`.
    instr_count: SharedCounter,
    /// Per-frame instruction budget.
    budget: SharedCounter,
    /// When `true`, `emu.write` AND `emu.setInput` are silent no-ops
    /// (deterministic / locked session). Shared as a `SharedFlag` so both the
    /// per-frame-scoped `write` accessor and the persistent `setInput` prelude
    /// function read the live value — so `setInput` is gated identically to
    /// `write` (T-110-E2), not merely at the host.
    writes_locked: SharedFlag,
    /// v1.5.0 B4 — `emu:on_breakpoint(addr, fn)` callbacks, keyed by CPU
    /// address. Observational: replayed from the per-frame exec-PC log exactly
    /// like `onExec` (the host arms the exec log when any are registered), so a
    /// breakpoint never intercepts mid-instruction or mutates deterministic
    /// state — it reports the PC after the fact.
    breakpoint_cbs: AddrCallbacks,
    /// v1.5.0 B4 — `emu:pause_at_frame(n)` targets. Each `on_frame` whose frame
    /// count has reached a target queues a `ControlCmd::Pause` and drops it.
    /// Observational control (the host applies the pause); never mutates the
    /// deterministic run.
    pause_frames: Shared<Vec<u64>>,
    /// v1.5.0 B3 — in-memory save-state slots populated by `emu:save_state(slot)`
    /// (read-only `Nes::snapshot`, always allowed) and consumed by
    /// `emu:load_state(slot)` (`Nes::restore`, GATED like `emu.write`). Distinct
    /// from the host's on-disk numbered slots: these live in the script engine
    /// for the session and are never persisted, so a TAS/analysis script can
    /// checkpoint + roll back without touching the user's save files.
    state_slots: Shared<HashMap<u8, Vec<u8>>>,
    /// v1.5.0 B4 — `sym:name(addr)` lookup (`address -> label`), pushed by the
    /// host from the debugger's loaded symbols. Read-only; never deterministic.
    sym_by_addr: Shared<HashMap<u16, String>>,
    /// v1.5.0 B4 — `sym:addr(name)` reverse lookup (`label -> address`). Built
    /// alongside [`Self::sym_by_addr`]; last writer wins on a duplicate label.
    sym_by_name: Shared<HashMap<String, u16>>,
    /// v1.6.0 B2 — the active **driving** coroutine registered via `emu.run(fn)`,
    /// stored Rust-side as a Lua registry key (a `thread` handle; never
    /// script-visible). [`Self::on_frame`] resumes it exactly once per emulated
    /// frame; the coroutine yields control back to the host with
    /// `emu.frameadvance()` (a thin `coroutine.yield()`), and the host advances
    /// one frame before the next resume. `None` once no driver is registered or
    /// the driver has run to completion. The driver only *reads* and issues the
    /// same gated `emu.write` / `emu.setInput` effects as any callback, so it
    /// never perturbs the deterministic run beyond what the write-gate already
    /// allows.
    driver: Shared<Option<RegistryKey>>,
    /// v1.7.0 "Forge" Workstream B (B1/B2) — all shared state backing the
    /// `tastudio.*` Lua surface (the host-pushed editor snapshot, the queued
    /// editor commands, and the cell-query / event callbacks). Self-contained in
    /// `crate::tastudio` for clean merging.
    tas: TasState,
    /// v1.7.0 "Forge" Workstream B (B3) — `emu.addEventCallback(fn, type)`
    /// callbacks keyed by the host-facing event type. `onNmi`/`onIrq` keep their
    /// own dedicated lists above (the legacy API); these are the *additional*
    /// Mesen2-parity events fired from `on_frame`: `startFrame` (top of a pump),
    /// `endFrame` (after the frame's callbacks), `inputPolled` (the frame polled
    /// a controller). `stateLoaded`/`stateSaved` fire from the in-memory
    /// save-state path. All observational (output-only).
    event_start_frame: Shared<Vec<RegistryKey>>,
    /// `endFrame` event callbacks (B3).
    event_end_frame: Shared<Vec<RegistryKey>>,
    /// `inputPolled` event callbacks (B3).
    event_input_polled: Shared<Vec<RegistryKey>>,
    /// `stateLoaded` event callbacks (B3) — fired after `emu:load_state`.
    event_state_loaded: Shared<Vec<RegistryKey>>,
    /// `stateSaved` event callbacks (B3) — fired after `emu:save_state`.
    event_state_saved: Shared<Vec<RegistryKey>>,
    /// v1.7.0 "Forge" Workstream B (B3) — `emu.addMemoryCallback` *value-modifying*
    /// write callbacks, keyed by CPU address. Unlike the observational `onWrite`,
    /// a callback here may RETURN a replacement byte; the engine then pokes it
    /// back via the GATED `poke_ram` path (a scriptable cheat / watchpoint).
    /// Dropped under a locked session, exactly like `emu.write`.
    modify_write_cbs: AddrCallbacks,
    /// v1.7.0 "Forge" Workstream B (B3) — a per-script sandboxed data directory
    /// (`emu.getScriptDataFolder()`), pushed by the host (the clean
    /// persist-without-arbitrary-FS path). `None` until the host sets it.
    script_data_folder: Shared<Option<String>>,
    /// v1.7.0 "Forge" E2 — `client.*` host-automation verbs requested this frame
    /// (drained by the host). Collected, never applied inline — the host stays
    /// the single owner of window / tool / capture / cheat state and gates the
    /// mutators among them.
    clients: Shared<Vec<ClientCmd>>,
    /// v1.7.0 "Forge" E3 — the per-script `userdata.*` KV store (string→string).
    /// Script-local host memory, never emulator state; the host persists it
    /// across runs via [`MluaBackend::userdata_snapshot`] /
    /// [`MluaBackend::userdata_restore`].
    userdata: Shared<HashMap<String, String>>,
    /// v1.7.0 "Forge" E1 — host-mediated `comm.*` IPC requests issued this frame
    /// (drained by the host, which owns every connection). Gated like
    /// `emu.write`: a locked session never queues one. Only present (and only
    /// installed) under the `script-ipc` feature.
    #[cfg(feature = "script-ipc")]
    comm_out: Shared<Vec<CommCmd>>,
    /// v1.7.0 "Forge" E1 — host-fulfilled `CommResult`s the script polls via
    /// `comm.receive()`. The host pushes here off the emulator lock.
    #[cfg(feature = "script-ipc")]
    comm_in: Shared<std::collections::VecDeque<CommResult>>,
    /// v1.7.0 "Forge" E1 — monotonic correlation-id source for async `comm.*`
    /// requests (HTTP / WS / MMF-read), so a result is matched to its request.
    #[cfg(feature = "script-ipc")]
    comm_next_id: SharedCounter,
}

impl MluaBackend {
    /// Install the persistent `emu` table: the callback registry, `emu.onFrame`,
    /// `emu.log`, and a `print` redirect. The live-`Nes` accessors (`read` /
    /// `write` / `cpu` / `frame` / `cycle`) are (re)bound per frame in
    /// [`Self::on_frame`] via a scope.
    #[allow(clippy::too_many_lines)] // one create_function per API entry.
    fn install_prelude(&self) -> Result<(), ScriptError> {
        let emu = self.lua.create_table()?;

        // emu.log(msg) — append to the host-visible buffer.
        let log = self.log.clone();
        let log_fn = self
            .lua
            .create_function(move |_, msg: mlua::Variadic<mlua::Value>| {
                let mut parts = Vec::with_capacity(msg.len());
                for v in msg.iter() {
                    parts.push(value_to_string(v));
                }
                log.borrow_mut().push(parts.join("\t"));
                Ok(())
            })?;
        emu.set("log", log_fn.clone())?;

        // Control commands (collected; the host applies + gates them). Each
        // queue is capped per frame so a script can't grow host memory without
        // bound (Copilot #47); excess commands in one frame are dropped.
        let controls = self.controls.clone();
        emu.set(
            "pause",
            self.lua.create_function(move |_, ()| {
                push_capped(&controls, ControlCmd::Pause);
                Ok(())
            })?,
        )?;
        let controls = self.controls.clone();
        emu.set(
            "saveState",
            self.lua.create_function(move |_, slot: u8| {
                push_capped(&controls, ControlCmd::SaveState(slot));
                Ok(())
            })?,
        )?;
        let controls = self.controls.clone();
        emu.set(
            "loadState",
            self.lua.create_function(move |_, slot: u8| {
                push_capped(&controls, ControlCmd::LoadState(slot));
                Ok(())
            })?,
        )?;
        let controls = self.controls.clone();
        let setinput_locked = self.writes_locked.clone();
        emu.set(
            "setInput",
            self.lua
                .create_function(move |_, (port, buttons): (u8, u8)| {
                    // Reject any port outside {0 = P1, 1 = P2} so an out-of-range
                    // value can never silently apply to the wrong player at the
                    // host's late-latch (which treats `port != 0` as P2).
                    if port > 1 {
                        return Err(mlua::Error::RuntimeError(format!(
                            "setInput: port must be 0 (P1) or 1 (P2), got {port}"
                        )));
                    }
                    // T-110-E2 — gated IDENTICALLY to `emu.write`: under a locked
                    // session (netplay / TAS replay / RA-hardcore) the command is
                    // dropped at the source, so it can never reach the host's
                    // late-latch and perturb a deterministic / replayed run.
                    if !setinput_locked.get() {
                        push_capped(&controls, ControlCmd::SetInput { port, buttons });
                    }
                    Ok(())
                })?,
        )?;

        // v1.6.0 B2 — Lua DRIVING primitives.
        //
        // `emu.frameadvance()` yields the running coroutine back to the host:
        // when the driving coroutine (registered via `emu.run`) calls it,
        // control returns to the host's per-frame pump, which advances exactly
        // one emulated frame and then resumes the coroutine on the next
        // `on_frame`. Calling it outside a coroutine raises Lua's own "attempt to
        // yield from outside a coroutine", surfaced to the host as a script
        // error — so a driving script must register itself with `emu.run`.
        //
        // It MUST be a pure-Lua function (a direct alias of `coroutine.yield`),
        // NOT a Rust `create_function`: a Rust closure is a C-call frame, and Lua
        // cannot yield across a C-call boundary ("attempt to yield across a
        // C-call boundary"). Defining it in Lua keeps the yield inside the Lua
        // stack, where coroutine resumption works. The host loads this chunk
        // directly (Rust-side `Lua::load`), so it is unaffected by the sandbox's
        // global stripping.
        let frameadvance: Function = self
            .lua
            .load("return function(...) return coroutine.yield(...) end")
            .eval()?;
        emu.set("frameadvance", frameadvance)?;

        // `emu.run(fn)` registers `fn` as the driving coroutine. The function
        // body typically loops, calling `emu.frameadvance()` between steps to
        // hand a frame to the emulator. Only one driver is active at a time
        // (a later `emu.run` replaces an earlier one). The coroutine handle is
        // stored Rust-side as a registry key (never script-visible), exactly
        // like the callback registries.
        let driver = self.driver.clone();
        emu.set(
            "run",
            self.lua.create_function(move |lua, f: Function| {
                let thread = lua.create_thread(f)?;
                let key = lua.create_registry_value(thread)?;
                // Replacing an existing driver drops its registry key (the old
                // coroutine is abandoned); the new one drives from now on. Take
                // the old key out and drop the lock guard before the registry
                // call (don't hold the lock across re-entrant Lua work).
                let old = driver.borrow_mut().replace(key);
                if let Some(old) = old {
                    lua.remove_registry_value(old)?;
                }
                Ok(())
            })?,
        )?;

        // Overlay draw commands (collected; the host renders them via egui).
        let draws = self.draws.clone();
        emu.set(
            "drawText",
            self.lua.create_function(
                move |_, (x, y, text, color): (i32, i32, String, Option<u32>)| {
                    push_capped(
                        &draws,
                        DrawCmd::Text {
                            x,
                            y,
                            color: color.unwrap_or(0xFFFF_FFFF),
                            text,
                        },
                    );
                    Ok(())
                },
            )?,
        )?;
        let draws = self.draws.clone();
        emu.set(
            "drawRect",
            self.lua.create_function(
                move |_, (x, y, w, h, color): (i32, i32, i32, i32, Option<u32>)| {
                    push_capped(
                        &draws,
                        DrawCmd::Rect {
                            x,
                            y,
                            w,
                            h,
                            color: color.unwrap_or(0xFFFF_FFFF),
                        },
                    );
                    Ok(())
                },
            )?,
        )?;
        let draws = self.draws.clone();
        emu.set(
            "drawPixel",
            self.lua
                .create_function(move |_, (x, y, color): (i32, i32, Option<u32>)| {
                    push_capped(
                        &draws,
                        DrawCmd::Pixel {
                            x,
                            y,
                            color: color.unwrap_or(0xFFFF_FFFF),
                        },
                    );
                    Ok(())
                })?,
        )?;

        // Callback registrars. The handles are stored Rust-side as Lua registry
        // keys (NOT in a script-visible global), so a script can register but
        // can never inspect / clobber / inject junk into the registry — the
        // whole "malformed registry value crashes the host" class is gone.
        let frame = self.frame_cbs.clone();
        emu.set(
            "onFrame",
            self.lua.create_function(move |lua, f: Function| {
                frame.borrow_mut().push(lua.create_registry_value(f)?);
                Ok(())
            })?,
        )?;
        // T-110-E1 — onNmi / onIrq: per-interrupt-service callbacks, replayed
        // each frame from the core's committed interrupt-service log (the same
        // Rust-side registry-key storage as onFrame, so a script can register
        // but never inspect / clobber the registry). Output-only: the callback
        // observes the service vector but cannot mutate emulation.
        for (name, list) in [("onNmi", &self.nmi_cbs), ("onIrq", &self.irq_cbs)] {
            let list = list.clone();
            emu.set(
                name,
                self.lua.create_function(move |lua, f: Function| {
                    list.borrow_mut().push(lua.create_registry_value(f)?);
                    Ok(())
                })?,
            )?;
        }
        // Address-keyed registrars (`emu.onExec`/`onRead`/`onWrite`, dot form).
        for (name, map) in [
            ("onExec", &self.exec_cbs),
            ("onRead", &self.read_cbs),
            ("onWrite", &self.write_cbs),
        ] {
            let map = map.clone();
            emu.set(
                name,
                self.lua
                    .create_function(move |lua, (addr, f): (i64, Function)| {
                        let key = lua.create_registry_value(f)?;
                        #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                        let addr = (addr & 0xFFFF) as u16;
                        map.borrow_mut().entry(addr).or_default().push(key);
                        Ok(())
                    })?,
            )?;
        }

        // v1.5.0 B4 — `emu:on_breakpoint(addr, fn)` (colon form; the leading
        // `self` table is ignored). Same `(addr, fn)` Rust-side registry-key
        // storage as `onExec`, replayed from the same per-frame exec-PC log — an
        // observational breakpoint that reports the PC after the frame, never an
        // intercept and never a state mutation.
        let bp_map = self.breakpoint_cbs.clone();
        emu.set(
            "on_breakpoint",
            self.lua.create_function(
                move |lua, (_this, addr, f): (mlua::Value, i64, Function)| {
                    let key = lua.create_registry_value(f)?;
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let addr = (addr & 0xFFFF) as u16;
                    bp_map.borrow_mut().entry(addr).or_default().push(key);
                    Ok(())
                },
            )?,
        )?;

        // v1.5.0 B4 — `emu:pause_at_frame(n)` (colon form; `self` ignored):
        // record a frame target; the next `on_frame` to reach it queues a Pause
        // control and drops the target.
        let pause_frames = self.pause_frames.clone();
        emu.set(
            "pause_at_frame",
            self.lua
                .create_function(move |_, (_this, n): (mlua::Value, i64)| {
                    let target = u64::try_from(n).unwrap_or(0);
                    // Capped (and the lock dropped promptly) like every other
                    // host queue, so a runaway loop can't grow host memory.
                    push_capped(&pause_frames, target);
                    Ok(())
                })?,
        )?;

        // v1.5.0 B4 — the `sym` table: read-only symbol-label queries against the
        // host-pushed debugger symbol map. `sym:name(addr)` / `sym:addr(name)`
        // (colon form; the leading `self` table is ignored). Both lookups are
        // pure reads of the engine-side maps — never deterministic state.
        let sym = self.lua.create_table()?;
        let by_addr = self.sym_by_addr.clone();
        sym.set(
            "name",
            self.lua
                .create_function(move |_, (_this, addr): (mlua::Value, i64)| {
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let addr = (addr & 0xFFFF) as u16;
                    Ok(by_addr.borrow().get(&addr).cloned())
                })?,
        )?;
        let by_name = self.sym_by_name.clone();
        sym.set(
            "addr",
            self.lua
                .create_function(move |_, (_this, name): (mlua::Value, String)| {
                    Ok(by_name.borrow().get(&name).copied())
                })?,
        )?;
        self.lua.globals().set("sym", sym)?;

        self.lua.globals().set("emu", &emu)?;

        // v1.6.0 B3 — the `joypad` table (colon-call). `joypad:set` mirrors
        // `emu.setInput` (the same gated `ControlCmd::SetInput` path); the
        // per-frame `joypad:get(port)` read is bound in the frame scope (it
        // needs the live `Nes`), exactly as `emu.read` is.
        let joypad = self.lua.create_table()?;
        {
            let controls = self.controls.clone();
            let locked = self.writes_locked.clone();
            joypad.set(
                "set",
                self.lua.create_function(
                    move |_, (_this, port, buttons): (mlua::Value, u8, u8)| {
                        // Gated identically to `emu.setInput`: dropped under a
                        // locked / replayed session.
                        if !locked.get() {
                            push_capped(&controls, ControlCmd::SetInput { port, buttons });
                        }
                        Ok(())
                    },
                )?,
            )?;
        }
        self.lua.globals().set("joypad", &joypad)?;

        // v1.7.0 "Forge" Workstream B (B1/B2) — install the persistent
        // `tastudio` table (queries read the host-pushed snapshot; mutators
        // queue gated `TasCmd`s; the B2 callbacks store Rust-side registry
        // keys). Self-contained in `crate::tastudio` for clean merging.
        tastudio::install(&self.lua, &self.tas, &self.writes_locked)?;

        // v1.7.0 "Forge" Workstream B (B3) — Mesen2-parity event + memory
        // callback registrars and a couple of "good-citizen" utilities. Kept in
        // their own clearly-named prelude section.
        self.install_lua_parity()?;

        // Redirect base `print` to the same sink.
        self.lua.globals().set("print", log_fn)?;
        Ok(())
    }

    /// v1.7.0 "Forge" Workstream B (B3) — install the Mesen2-parity surface:
    /// `emu.addEventCallback(fn, type)` (the full event enum), the
    /// value-modifying `emu.addMemoryCallback(fn, type, start[, end])`,
    /// `emu.takeScreenshot()`, and `emu.getScriptDataFolder()`. The per-frame
    /// `getScreenBuffer` / `setScreenBuffer` / `getPixel` / `getState` /
    /// `setState` accessors need the live `Nes`, so they are bound in the frame
    /// scope (in [`Self::on_frame`]), exactly like `emu.read` / `emu.write`.
    fn install_lua_parity(&self) -> Result<(), ScriptError> {
        let emu: Table = self.lua.globals().get("emu")?;

        // `emu.addEventCallback(fn, type)` — `type` is one of the Mesen2 event
        // names. `nmi`/`irq` route to the existing dedicated lists (so the new
        // API and the legacy `onNmi`/`onIrq` share one dispatch); the rest land
        // in their own per-event lists, fired from `on_frame`.
        let nmi = self.nmi_cbs.clone();
        let irq = self.irq_cbs.clone();
        let start = self.event_start_frame.clone();
        let end = self.event_end_frame.clone();
        let polled = self.event_input_polled.clone();
        let loaded = self.event_state_loaded.clone();
        let saved = self.event_state_saved.clone();
        emu.set(
            "addEventCallback",
            self.lua
                .create_function(move |lua, (f, ty): (Function, String)| {
                    let list = match ty.as_str() {
                        "nmi" => &nmi,
                        "irq" => &irq,
                        "startFrame" => &start,
                        "endFrame" => &end,
                        "inputPolled" => &polled,
                        "stateLoaded" => &loaded,
                        "stateSaved" => &saved,
                        other => {
                            return Err(mlua::Error::RuntimeError(format!(
                                "addEventCallback: unknown event type '{other}'"
                            )));
                        }
                    };
                    list.borrow_mut().push(lua.create_registry_value(f)?);
                    Ok(())
                })?,
        )?;

        // `emu.addMemoryCallback(fn, type, start[, end])` — a VALUE-MODIFYING
        // write callback over `[start, end]` (inclusive; `end` defaults to
        // `start`). `type` must be `"write"` (read/exec value-modify is not a
        // thing on a post-frame replay). The callback receives `(addr, value)`
        // and may RETURN a replacement byte; the engine pokes it back through
        // the GATED `poke_ram` path. This is the scriptable-cheat / scriptable-
        // watchpoint primitive; it is gated IDENTICALLY to `emu.write`.
        let modify = self.modify_write_cbs.clone();
        emu.set(
            "addMemoryCallback",
            self.lua.create_function(
                move |lua, (f, ty, start, end): (Function, String, i64, Option<i64>)| {
                    if ty != "write" {
                        return Err(mlua::Error::RuntimeError(format!(
                            "addMemoryCallback: only 'write' supports value-modify (got '{ty}')"
                        )));
                    }
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let lo = (start & 0xFFFF) as u16;
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let hi = (end.unwrap_or(start) & 0xFFFF) as u16;
                    // This registers one registry key PER address, so a huge span
                    // (up to 64K) would allocate 64K registry values. Reject an
                    // oversized range so a script can't force that allocation; a
                    // legitimate watchpoint covers a handful of addresses, and a
                    // whole-RAM watch should use the observational `onWrite` hook.
                    let span = u32::from(hi.max(lo) - lo) + 1;
                    if span > MAX_MEMORY_CALLBACK_SPAN {
                        return Err(mlua::Error::RuntimeError(format!(
                            "addMemoryCallback: range too large ({span} addresses; max \
                             {MAX_MEMORY_CALLBACK_SPAN}). Use a narrower span or the \
                             observational emu.addMemoryCallback 'onWrite' hook."
                        )));
                    }
                    // Register one shared key per address in the range so a hit
                    // at any address in the span fires the callback (BizHawk /
                    // Mesen2 range semantics). Clamp an inverted range to `lo`.
                    let key = lua.create_registry_value(f)?;
                    // The first address owns the real key; the rest reference it
                    // by cloning the Function out and re-registering, so each
                    // address has an independent registry value (a dropped one
                    // doesn't free another's). Cheap: ranges are tiny in practice.
                    let f0: Function = lua.registry_value(&key)?;
                    // Scope the lock guard to the fill loop so it is released
                    // before the final registry call (and never held across
                    // unrelated Lua work).
                    {
                        let mut map = modify.borrow_mut();
                        for a in lo..=hi.max(lo) {
                            let k = lua.create_registry_value(f0.clone())?;
                            map.entry(a).or_default().push(k);
                        }
                    }
                    lua.remove_registry_value(key)?;
                    Ok(())
                },
            )?,
        )?;

        // `emu.takeScreenshot()` — queue a host PNG write (the host owns the
        // encoder + the screenshot dir; the script crate stays dep-free). A
        // read-only side effect (a file write), so it is NOT write-gated — a
        // screenshot cannot perturb deterministic state.
        let controls = self.controls.clone();
        emu.set(
            "takeScreenshot",
            self.lua
                .create_function(move |_, _args: mlua::Variadic<mlua::Value>| {
                    push_capped(&controls, ControlCmd::Screenshot);
                    Ok(())
                })?,
        )?;

        // `emu.getScriptDataFolder()` — the host-pushed sandboxed data dir
        // (the clean persist-without-arbitrary-FS path), or `nil` if unset.
        let folder = self.script_data_folder.clone();
        emu.set(
            "getScriptDataFolder",
            self.lua
                .create_function(move |_, _args: mlua::Variadic<mlua::Value>| {
                    Ok(folder.borrow().clone())
                })?,
        )?;

        Ok(())
    }

    /// v1.7.0 "Forge" Workstream E — install the platform tables that turn
    /// `RustyNES` into a host for external bots / RL agents / randomizers:
    ///
    /// - **`client.*` (E2)** — host-automation verbs (open tools, screenshot,
    ///   window size, speed/frameskip, reboot, A/V pause, cheats). Collected as
    ///   [`ClientCmd`]s and drained by the host; the state-changing verbs
    ///   (`reboot_core`, `addcheat`, `removecheat`) are gated like `emu.write`.
    /// - **`userdata.*` (E3)** — a per-script string→string KV store the host
    ///   persists across runs. Pure host memory, never emulator state.
    /// - **`comm.*` (E1, `script-ipc` only)** — host-mediated TCP / HTTP / WS /
    ///   memory-mapped-file IPC. The script NEVER gets a raw socket: it queues a
    ///   [`CommCmd`] and the host owns the connection, marshalling plain values
    ///   back via [`CommResult`]. A new non-deterministic source, so every verb
    ///   is gated like `emu.write`.
    ///
    /// Kept in one self-contained method (separate from the v1.0–v1.6 `emu` /
    /// `joypad` / `sym` prelude) so the workstream merges cleanly.
    fn install_platform_tables(&self) -> Result<(), ScriptError> {
        self.install_client_table()?;
        self.install_userdata_table()?;
        #[cfg(feature = "script-ipc")]
        self.install_comm_table()?;
        Ok(())
    }

    /// E2 — the `client.*` host-automation table.
    fn install_client_table(&self) -> Result<(), ScriptError> {
        let client = self.lua.create_table()?;

        // Observational / presentation-only verbs (never perturb the core), so
        // they are NOT write-gated — like `emu.pause` / `emu.saveState`.
        let q = self.clients.clone();
        client.set(
            "opentool",
            self.lua.create_function(move |_, name: String| {
                push_capped(&q, ClientCmd::OpenTool(name));
                Ok(())
            })?,
        )?;
        let q = self.clients.clone();
        client.set(
            "screenshot",
            self.lua.create_function(move |_, ()| {
                push_capped(&q, ClientCmd::Screenshot);
                Ok(())
            })?,
        )?;
        let q = self.clients.clone();
        client.set(
            "screenshottoclipboard",
            self.lua.create_function(move |_, ()| {
                push_capped(&q, ClientCmd::ScreenshotToClipboard);
                Ok(())
            })?,
        )?;
        let q = self.clients.clone();
        client.set(
            "setwindowsize",
            self.lua.create_function(move |_, scale: u32| {
                push_capped(&q, ClientCmd::SetWindowSize(scale));
                Ok(())
            })?,
        )?;
        let q = self.clients.clone();
        client.set(
            "speedmode",
            self.lua.create_function(move |_, pct: u32| {
                push_capped(&q, ClientCmd::SpeedMode(pct));
                Ok(())
            })?,
        )?;
        let q = self.clients.clone();
        client.set(
            "frameskip",
            self.lua.create_function(move |_, n: u32| {
                push_capped(&q, ClientCmd::FrameSkip(n));
                Ok(())
            })?,
        )?;
        let q = self.clients.clone();
        client.set(
            "pause_av",
            self.lua.create_function(move |_, ()| {
                push_capped(&q, ClientCmd::PauseAv);
                Ok(())
            })?,
        )?;
        let q = self.clients.clone();
        client.set(
            "unpause_av",
            self.lua.create_function(move |_, ()| {
                push_capped(&q, ClientCmd::UnpauseAv);
                Ok(())
            })?,
        )?;

        // State-changing verbs — GATED like `emu.write`: dropped at the source
        // under a locked session (netplay / TAS replay / record / RA-hardcore),
        // so a script can never perturb a deterministic / replayed run.
        let q = self.clients.clone();
        let locked = self.writes_locked.clone();
        client.set(
            "reboot_core",
            self.lua.create_function(move |_, ()| {
                if !locked.get() {
                    push_capped(&q, ClientCmd::RebootCore);
                }
                Ok(())
            })?,
        )?;
        let q = self.clients.clone();
        let locked = self.writes_locked.clone();
        client.set(
            "addcheat",
            self.lua.create_function(move |_, code: String| {
                if !locked.get() {
                    push_capped(&q, ClientCmd::AddCheat(code));
                }
                Ok(())
            })?,
        )?;
        let q = self.clients.clone();
        let locked = self.writes_locked.clone();
        client.set(
            "removecheat",
            self.lua.create_function(move |_, code: String| {
                if !locked.get() {
                    push_capped(&q, ClientCmd::RemoveCheat(code));
                }
                Ok(())
            })?,
        )?;

        self.lua.globals().set("client", client)?;
        Ok(())
    }

    /// E3 — the `userdata.*` per-script KV store (`set` / `get` / `containskey`
    /// / `remove` / `keys`). String→string; lives in host memory and is
    /// persisted across runs by the host. Never touches emulator state, so it is
    /// not write-gated.
    fn install_userdata_table(&self) -> Result<(), ScriptError> {
        let userdata = self.lua.create_table()?;

        let kv = self.userdata.clone();
        userdata.set(
            "set",
            self.lua
                .create_function(move |_, (key, value): (String, String)| {
                    kv.borrow_mut().insert(key, value);
                    Ok(())
                })?,
        )?;
        let kv = self.userdata.clone();
        userdata.set(
            "get",
            self.lua
                .create_function(move |_, key: String| Ok(kv.borrow().get(&key).cloned()))?,
        )?;
        let kv = self.userdata.clone();
        userdata.set(
            "containskey",
            self.lua
                .create_function(move |_, key: String| Ok(kv.borrow().contains_key(&key)))?,
        )?;
        let kv = self.userdata.clone();
        userdata.set(
            "remove",
            self.lua.create_function(move |_, key: String| {
                Ok(kv.borrow_mut().remove(&key).is_some())
            })?,
        )?;
        let kv = self.userdata.clone();
        userdata.set(
            "keys",
            self.lua.create_function(move |lua, ()| {
                // Sorted for a stable, deterministic iteration order.
                let mut keys: Vec<String> = kv.borrow().keys().cloned().collect();
                keys.sort_unstable();
                lua.create_sequence_from(keys)
            })?,
        )?;

        self.lua.globals().set("userdata", userdata)?;
        Ok(())
    }

    /// E1 — the host-mediated `comm.*` IPC table (`script-ipc` only).
    ///
    /// Every entry queues a [`CommCmd`] (the host owns the connection and does
    /// the I/O) or polls the host-injected [`CommResult`] inbox via
    /// `comm.receive()`. The script never sees a socket handle, so the sandbox's
    /// no-`io`/`os`/net guarantee is preserved. All verbs are GATED like
    /// `emu.write` (dropped under a locked session) because IPC is a new
    /// non-deterministic source.
    #[cfg(feature = "script-ipc")]
    #[allow(clippy::too_many_lines)] // one create_function per comm.* entry.
    fn install_comm_table(&self) -> Result<(), ScriptError> {
        let comm = self.lua.create_table()?;

        // A fresh async correlation id (host echoes it back in the CommResult).
        let next_id = self.comm_next_id.clone();
        let alloc_id = move || -> u64 {
            let id = next_id.get();
            next_id.set(id.wrapping_add(1));
            id
        };

        let out = self.comm_out.clone();
        let locked = self.writes_locked.clone();
        comm.set(
            "socketServerSend",
            self.lua.create_function(move |_, data: mlua::String| {
                if !locked.get() {
                    push_capped(&out, CommCmd::SocketSend(data.as_bytes().to_vec()));
                }
                Ok(())
            })?,
        )?;
        let out = self.comm_out.clone();
        let locked = self.writes_locked.clone();
        let mk = alloc_id.clone();
        comm.set(
            "httpGet",
            self.lua.create_function(move |_, url: String| {
                if locked.get() {
                    return Ok(0u64);
                }
                let id = mk();
                push_capped(&out, CommCmd::HttpGet { id, url });
                Ok(id)
            })?,
        )?;
        let out = self.comm_out.clone();
        let locked = self.writes_locked.clone();
        let mk = alloc_id.clone();
        comm.set(
            "httpPost",
            self.lua
                .create_function(move |_, (url, body): (String, String)| {
                    if locked.get() {
                        return Ok(0u64);
                    }
                    let id = mk();
                    push_capped(&out, CommCmd::HttpPost { id, url, body });
                    Ok(id)
                })?,
        )?;
        let out = self.comm_out.clone();
        let locked = self.writes_locked.clone();
        let mk = alloc_id.clone();
        comm.set(
            "ws_open",
            self.lua.create_function(move |_, url: String| {
                if locked.get() {
                    return Ok(0u64);
                }
                let id = mk();
                push_capped(&out, CommCmd::WsOpen { id, url });
                Ok(id)
            })?,
        )?;
        let out = self.comm_out.clone();
        let locked = self.writes_locked.clone();
        comm.set(
            "ws_send",
            self.lua.create_function(move |_, text: String| {
                if !locked.get() {
                    push_capped(&out, CommCmd::WsSend(text));
                }
                Ok(())
            })?,
        )?;
        let out = self.comm_out.clone();
        let locked = self.writes_locked.clone();
        comm.set(
            "ws_close",
            self.lua.create_function(move |_, ()| {
                if !locked.get() {
                    push_capped(&out, CommCmd::WsClose);
                }
                Ok(())
            })?,
        )?;
        let out = self.comm_out.clone();
        let locked = self.writes_locked.clone();
        comm.set(
            "mmfWrite",
            self.lua
                .create_function(move |_, (name, data): (String, mlua::String)| {
                    if !locked.get() {
                        push_capped(
                            &out,
                            CommCmd::MmfWrite {
                                name,
                                data: data.as_bytes().to_vec(),
                            },
                        );
                    }
                    Ok(())
                })?,
        )?;
        let out = self.comm_out.clone();
        let locked = self.writes_locked.clone();
        let mk = alloc_id;
        comm.set(
            "mmfRead",
            self.lua
                .create_function(move |_, (name, len): (String, u32)| {
                    if locked.get() {
                        return Ok(0u64);
                    }
                    let id = mk();
                    push_capped(&out, CommCmd::MmfRead { id, name, len });
                    Ok(id)
                })?,
        )?;

        // `comm.receive()` — pop the oldest host-injected result (or nil). The
        // host fulfils the async requests off the emulator lock and pushes a
        // `CommResult`; the script polls it here. Returns a small Lua table the
        // script destructures (`{kind=..., id=..., status=..., body=..., ...}`).
        let inbox = self.comm_in.clone();
        comm.set(
            "receive",
            self.lua.create_function(move |lua, ()| {
                let next = inbox.borrow_mut().pop_front();
                match next {
                    None => Ok(mlua::Value::Nil),
                    Some(result) => {
                        let t = lua.create_table()?;
                        match result {
                            CommResult::Http { id, status, body } => {
                                t.set("kind", "http")?;
                                t.set("id", id)?;
                                t.set("status", status)?;
                                t.set("body", body)?;
                            }
                            CommResult::WsState { id, open, message } => {
                                t.set("kind", "ws")?;
                                t.set("id", id)?;
                                t.set("open", open)?;
                                t.set("message", message)?;
                            }
                            CommResult::Mmf { id, data } => {
                                t.set("kind", "mmf")?;
                                t.set("id", id)?;
                                t.set("data", lua.create_string(&data)?)?;
                            }
                        }
                        Ok(mlua::Value::Table(t))
                    }
                }
            })?,
        )?;

        self.lua.globals().set("comm", comm)?;
        Ok(())
    }

    /// Install the per-frame instruction-budget hook (and reset the counter).
    fn arm_hook(&self) -> Result<(), ScriptError> {
        self.instr_count.set(0);
        let count = self.instr_count.clone();
        let budget = self.budget.clone();
        // mlua 0.11 makes `set_hook` fallible. The instruction-budget hook is
        // the sandbox's runaway-script guard, so a failed install is surfaced
        // as an error rather than silently leaving scripts uncapped.
        self.lua.set_hook(
            HookTriggers::new().every_nth_instruction(10_000),
            move |_lua, _debug| {
                let n = count.get() + 10_000;
                count.set(n);
                if n > budget.get() {
                    Err(mlua::Error::RuntimeError(
                        "script exceeded the per-frame instruction budget".into(),
                    ))
                } else {
                    Ok(VmState::Continue)
                }
            },
        )?;
        Ok(())
    }
}

impl VmBackend for MluaBackend {
    fn new() -> Result<Self, ScriptError> {
        // Only the pure, side-effect-free standard libraries.
        let lua = Lua::new_with(
            StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::COROUTINE,
            mlua::LuaOptions::default(),
        )?;

        let log: Shared<Vec<String>> = Shared::new(Vec::new());
        let controls: Shared<Vec<ControlCmd>> = Shared::new(Vec::new());
        let draws: Shared<Vec<DrawCmd>> = Shared::new(Vec::new());
        let instr_count = SharedCounter::new(0u64);
        let budget = SharedCounter::new(DEFAULT_INSTRUCTION_BUDGET);

        // Remove the unsafe base globals the sandbox must not expose.
        {
            let g = lua.globals();
            for name in [
                "load",
                "loadfile",
                "dofile",
                "loadstring",
                "collectgarbage",
                "require",
                "package",
                "io",
                "os",
                "debug",
            ] {
                g.set(name, mlua::Value::Nil)?;
            }
        }

        let engine = Self {
            lua,
            log,
            controls,
            draws,
            frame_cbs: Shared::new(Vec::new()),
            exec_cbs: Shared::new(HashMap::new()),
            read_cbs: Shared::new(HashMap::new()),
            write_cbs: Shared::new(HashMap::new()),
            nmi_cbs: Shared::new(Vec::new()),
            irq_cbs: Shared::new(Vec::new()),
            instr_count,
            budget,
            writes_locked: SharedFlag::new(false),
            breakpoint_cbs: Shared::new(HashMap::new()),
            pause_frames: Shared::new(Vec::new()),
            state_slots: Shared::new(HashMap::new()),
            sym_by_addr: Shared::new(HashMap::new()),
            sym_by_name: Shared::new(HashMap::new()),
            driver: Shared::new(None),
            tas: TasState::new(),
            event_start_frame: Shared::new(Vec::new()),
            event_end_frame: Shared::new(Vec::new()),
            event_input_polled: Shared::new(Vec::new()),
            event_state_loaded: Shared::new(Vec::new()),
            event_state_saved: Shared::new(Vec::new()),
            modify_write_cbs: Shared::new(HashMap::new()),
            script_data_folder: Shared::new(None),
            clients: Shared::new(Vec::new()),
            userdata: Shared::new(HashMap::new()),
            #[cfg(feature = "script-ipc")]
            comm_out: Shared::new(Vec::new()),
            #[cfg(feature = "script-ipc")]
            comm_in: Shared::new(std::collections::VecDeque::new()),
            #[cfg(feature = "script-ipc")]
            comm_next_id: SharedCounter::new(1),
        };
        engine.install_prelude()?;
        engine.install_platform_tables()?;
        Ok(engine)
    }

    fn set_instruction_budget(&self, budget: u64) {
        self.budget.set(budget);
    }

    fn set_writes_locked(&self, locked: bool) {
        self.writes_locked.set(locked);
    }

    fn set_symbols(&self, pairs: &[(u16, String)]) {
        // Scope both lock guards to the rebuild so they release as soon as the
        // map is repopulated (clippy flags a `MutexGuard` held to scope end).
        {
            let mut by_addr = self.sym_by_addr.borrow_mut();
            by_addr.clear();
            // Pre-allocate for the incoming pairs so the inserts below don't
            // rehash as the map grows.
            by_addr.reserve(pairs.len());
            for (addr, name) in pairs {
                by_addr.insert(*addr, name.clone());
            }
        }
        let mut by_name = self.sym_by_name.borrow_mut();
        by_name.clear();
        by_name.reserve(pairs.len());
        for (addr, name) in pairs {
            by_name.insert(name.clone(), *addr);
        }
    }

    fn drain_log(&self) -> Vec<String> {
        std::mem::take(&mut self.log.borrow_mut())
    }

    fn drain_controls(&self) -> Vec<ControlCmd> {
        std::mem::take(&mut self.controls.borrow_mut())
    }

    fn drain_draws(&self) -> Vec<DrawCmd> {
        std::mem::take(&mut self.draws.borrow_mut())
    }

    fn needs_exec_log(&self) -> bool {
        // `on_breakpoint` replays from the same per-frame exec-PC log as
        // `onExec`, so either kind of registration arms it (B4).
        !self.exec_cbs.borrow().is_empty() || !self.breakpoint_cbs.borrow().is_empty()
    }

    fn needs_access_log(&self) -> bool {
        // The value-modifying write callbacks (B3) replay from the same bus
        // access log, so they arm it too.
        !self.read_cbs.borrow().is_empty()
            || !self.write_cbs.borrow().is_empty()
            || !self.modify_write_cbs.borrow().is_empty()
    }

    fn needs_interrupt_log(&self) -> bool {
        !self.nmi_cbs.borrow().is_empty() || !self.irq_cbs.borrow().is_empty()
    }

    fn load(&mut self, src: &str) -> Result<(), ScriptError> {
        self.arm_hook()?;
        let r = self.lua.load(src).exec().map_err(ScriptError::from);
        self.lua.remove_hook();
        r
    }

    #[allow(clippy::too_many_lines)] // scoped accessor binding + replay loops.
    fn on_frame(&mut self, nes: &mut Nes) -> Result<(), ScriptError> {
        let frame = nes.frame();
        let cycle = nes.cycle();
        let writes_locked = self.writes_locked.get();

        // Snapshot the just-finished frame's exec PCs + bus accesses (owned, so
        // they don't tie up the `nes` borrow inside the scope) for the
        // onExec / onRead / onWrite replay. `exec_log` is the dedicated
        // per-frame log (cleared each frame) — NOT the rolling trace buffer, so
        // there are no stale / duplicate PCs (gemini #47). Both are empty unless
        // the matching callbacks are registered (so the gate is free when off).
        let want_exec = self.needs_exec_log();
        let want_access = self.needs_access_log();
        let want_interrupt = self.needs_interrupt_log();
        let exec_pcs: Vec<u16> = if want_exec {
            nes.exec_log().to_vec()
        } else {
            Vec::new()
        };
        let accesses: Vec<(bool, u16, u8)> = if want_access {
            nes.accesses()
                .iter()
                .map(|a| (a.write, a.addr, a.value))
                .collect()
        } else {
            Vec::new()
        };
        // Snapshot this frame's committed interrupt services (owned, so the
        // `nes` borrow is free inside the scope). Each is `(is_nmi, vector)`;
        // replayed through onNmi / onIrq below. Empty unless onNmi/onIrq exist.
        let interrupts: Vec<(bool, u16)> = if want_interrupt {
            nes.interrupt_log()
                .iter()
                .map(|i| (i.is_nmi, i.vector))
                .collect()
        } else {
            Vec::new()
        };

        // v1.5.0 B2 — read-only cart / system metadata, captured once before the
        // scope (cheap; all `const`/O(1) on the core). `sha256` is the lowercase
        // hex of the ROM's SHA-256 (matches the host's save-state directory key).
        let cart_mapper_id = nes.mapper_id();
        let cart_prg_size = nes.prg_rom_len() as u64;
        let cart_chr_size = nes.chr_rom_len() as u64;
        let cart_region: &'static str = match nes.region() {
            rustynes_core::Region::Pal => "PAL",
            rustynes_core::Region::Dendy => "Dendy",
            rustynes_core::Region::Ntsc => "NTSC",
        };
        let cart_sha256 = {
            let mut s = String::with_capacity(64);
            for b in nes.rom_sha256() {
                use core::fmt::Write as _;
                let _ = write!(s, "{b:02x}");
            }
            s
        };

        // v1.7.0 "Forge" Workstream B (B3) — whether the just-finished frame
        // polled a controller (drives the `inputPolled` event). Read before
        // `nes` is moved into the `RefCell`.
        let input_was_polled = nes.was_input_polled_this_frame();

        let nes_cell = RefCell::new(nes);
        let lua = &self.lua;
        // Rust-side callback registries (clones of the `Rc`s) — used inside the
        // scope without aliasing `self`.
        let frame_cbs = self.frame_cbs.clone();
        let exec_cbs = self.exec_cbs.clone();
        let read_cbs = self.read_cbs.clone();
        let write_cbs = self.write_cbs.clone();
        let nmi_cbs = self.nmi_cbs.clone();
        let irq_cbs = self.irq_cbs.clone();
        let breakpoint_cbs = self.breakpoint_cbs.clone();
        let state_slots = self.state_slots.clone();
        let controls = self.controls.clone();
        let driver = self.driver.clone();
        // v1.7.0 "Forge" Workstream B (B3) — the additional event lists +
        // the value-modifying write callbacks, cloned for use inside the scope.
        let event_start_frame = self.event_start_frame.clone();
        let event_end_frame = self.event_end_frame.clone();
        let event_input_polled = self.event_input_polled.clone();
        let event_state_loaded = self.event_state_loaded.clone();
        let event_state_saved = self.event_state_saved.clone();
        let modify_write_cbs = self.modify_write_cbs.clone();
        // v1.5.0 B4 — drain the pause-at-frame targets reached this frame, OUTSIDE
        // the scope (no `nes` access): each reached target queues a Pause control.
        {
            let mut pf = self.pause_frames.borrow_mut();
            if !pf.is_empty() {
                pf.retain(|&target| {
                    if frame >= target {
                        push_capped(&controls, ControlCmd::Pause);
                        false
                    } else {
                        true
                    }
                });
            }
        }

        self.instr_count.set(0);
        let count = self.instr_count.clone();
        let budget = self.budget.clone();
        // mlua 0.11: `set_hook` is fallible. Surface a failed install as an
        // error rather than silently running the frame's callbacks without the
        // runaway-script budget guard (see arm_hook).
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(10_000),
            move |_lua, _debug| {
                let n = count.get() + 10_000;
                count.set(n);
                if n > budget.get() {
                    Err(mlua::Error::RuntimeError(
                        "script exceeded the per-frame instruction budget".into(),
                    ))
                } else {
                    Ok(VmState::Continue)
                }
            },
        )?;

        let result = lua.scope(|scope| {
            let emu: Table = lua.globals().get("emu")?;

            let read =
                scope.create_function(|_, addr: u16| Ok(nes_cell.borrow_mut().peek(addr)))?;
            emu.set("read", read)?;

            // v1.6.0 B3 — bind the per-frame `joypad:get(port)` read against the
            // live `Nes` (`joypad:set` was installed on the table in the prelude).
            // Returns the latched standard-controller bitmask (port 0=P1 .. 3),
            // side-effect-free (reads the latch, not the shift register).
            let joypad: Table = lua.globals().get("joypad")?;
            joypad.set(
                "get",
                scope.create_function(|_, (_this, port): (mlua::Value, u16)| {
                    Ok(nes_cell.borrow().controller_buttons((port & 0x03) as usize))
                })?,
            )?;

            let read_range = scope.create_function(|_, (addr, len): (u32, u32)| {
                read_range_cpu(&nes_cell, addr, len)
            })?;
            emu.set("readRange", read_range)?;

            // v1.7.0 "Forge" Workstream B (B3) — Mesen2-parity framebuffer +
            // structured-state accessors. They need the live `Nes`, so they are
            // bound here in the frame scope like `emu.read` / `emu.write`.

            // `emu.getScreenBuffer()` -> a flat array (1-based) of the 256x240
            // RGBA8 pixels packed as 0xRRGGBBAA ints. Read-only.
            emu.set(
                "getScreenBuffer",
                scope.create_function(|lua, _args: mlua::Variadic<mlua::Value>| {
                    let nes = nes_cell.borrow();
                    let fb = nes.framebuffer();
                    let t = lua.create_table_with_capacity(fb.len() / 4, 0)?;
                    for (i, px) in fb.chunks_exact(4).enumerate() {
                        let argb = (u32::from(px[0]) << 24)
                            | (u32::from(px[1]) << 16)
                            | (u32::from(px[2]) << 8)
                            | u32::from(px[3]);
                        t.set(i + 1, argb)?;
                    }
                    Ok(t)
                })?,
            )?;
            // `emu.getPixel(x, y)` -> the single 0xRRGGBBAA pixel, or nil if out
            // of the 256x240 frame.
            emu.set(
                "getPixel",
                scope.create_function(|_, (x, y): (i64, i64)| {
                    if !(0..256).contains(&x) || !(0..240).contains(&y) {
                        return Ok(None);
                    }
                    #[allow(clippy::cast_sign_loss, clippy::cast_possible_truncation)]
                    let off = ((y as usize) * 256 + (x as usize)) * 4;
                    let nes = nes_cell.borrow();
                    let fb = nes.framebuffer();
                    Ok(fb.get(off..off + 4).map(|px| {
                        (u32::from(px[0]) << 24)
                            | (u32::from(px[1]) << 16)
                            | (u32::from(px[2]) << 8)
                            | u32::from(px[3])
                    }))
                })?,
            )?;
            // `emu:setScreenBuffer(t)` — paint output only (the display
            // framebuffer the frontend presents). GATED like `emu.write`: a
            // no-op under a locked / replayed session. `t` is the same flat
            // 0xRRGGBBAA array `getScreenBuffer` returns; a short table leaves
            // the tail untouched. Output-only — never a register / latch.
            emu.set(
                "setScreenBuffer",
                scope.create_function(|_, (_this, t): (mlua::Value, Vec<u32>)| {
                    if !writes_locked {
                        let mut rgba = Vec::with_capacity(t.len() * 4);
                        for argb in t {
                            // 0xRRGGBBAA -> [R, G, B, A] (big-endian byte order).
                            rgba.extend_from_slice(&argb.to_be_bytes());
                        }
                        nes_cell.borrow_mut().debug_set_framebuffer(&rgba);
                    }
                    Ok(())
                })?,
            )?;
            // `emu:getState()` -> a structured field map (Mesen2 L4): the CPU
            // register file + frame/cycle/region. Read-only.
            emu.set(
                "getState",
                scope.create_function(|lua, _this: mlua::Value| {
                    let nes = nes_cell.borrow();
                    let c = nes.cpu();
                    let t = lua.create_table()?;
                    t.set("a", c.a)?;
                    t.set("x", c.x)?;
                    t.set("y", c.y)?;
                    t.set("s", c.s)?;
                    t.set("p", c.p.bits())?;
                    t.set("pc", c.pc)?;
                    t.set("frameCount", frame)?;
                    t.set("cycle", cycle)?;
                    t.set("region", cart_region)?;
                    Ok(t)
                })?,
            )?;
            // `emu:setState(t)` — write back the CPU register file from a state
            // map (the writable subset of `getState`). GATED like `emu.write`:
            // a no-op under a locked / replayed session. Missing fields keep
            // their current value (read from the live CPU first).
            emu.set(
                "setState",
                scope.create_function(|_, (_this, t): (mlua::Value, Table)| {
                    if !writes_locked {
                        // Read the current register file, then overlay only the
                        // fields the state table actually provides (a partial
                        // `setState` leaves the rest untouched).
                        let (acc, idx_x, idx_y, sp, status, pc) = {
                            let nes = nes_cell.borrow();
                            let c = nes.cpu();
                            (c.a, c.x, c.y, c.s, c.p.bits(), c.pc)
                        };
                        let acc = t.get::<u8>("a").unwrap_or(acc);
                        let idx_x = t.get::<u8>("x").unwrap_or(idx_x);
                        let idx_y = t.get::<u8>("y").unwrap_or(idx_y);
                        let sp = t.get::<u8>("s").unwrap_or(sp);
                        let status = t.get::<u8>("p").unwrap_or(status);
                        let pc = t.get::<u16>("pc").unwrap_or(pc);
                        nes_cell
                            .borrow_mut()
                            .debug_set_cpu_state(acc, idx_x, idx_y, sp, status, pc);
                    }
                    Ok(())
                })?,
            )?;

            let write = scope.create_function(|_, (addr, val): (u16, u8)| {
                if !writes_locked {
                    nes_cell.borrow_mut().poke_ram(addr, val);
                }
                Ok(())
            })?;
            emu.set("write", write)?;

            let cpu = scope.create_function(|lua, ()| {
                let nes = nes_cell.borrow();
                let c = nes.cpu();
                let t = lua.create_table()?;
                t.set("a", c.a)?;
                t.set("x", c.x)?;
                t.set("y", c.y)?;
                t.set("s", c.s)?;
                t.set("p", c.p.bits())?;
                t.set("pc", c.pc)?;
                Ok(t)
            })?;
            emu.set("cpu", cpu)?;

            emu.set("frame", frame)?;
            emu.set("cycle", cycle)?;

            // v1.5.0 B1 — the `memory` table: explicit CPU + PPU space access.
            // Colon-call form (`memory:peek(addr)`); the leading `self` table is
            // ignored. Reads use the side-effect-free debug-peek path ($2002 does
            // NOT clear VBL, $2007 does NOT advance the read buffer), so observing
            // memory never perturbs the deterministic run. `poke`/`write_range`
            // are GATED identically to `emu.write` (system RAM only, dropped under
            // a locked / replayed session).
            let memory = lua.create_table()?;
            memory.set(
                "peek",
                scope.create_function(|_, (_this, addr): (mlua::Value, u16)| {
                    Ok(nes_cell.borrow_mut().peek(addr))
                })?,
            )?;
            memory.set(
                "peek_ppu",
                scope.create_function(|_, (_this, addr): (mlua::Value, u16)| {
                    // PPU bus is $0000-$3FFF (mirrored to $4000); mask to 14 bits.
                    Ok(nes_cell.borrow_mut().ppu_bus_peek(addr & 0x3FFF))
                })?,
            )?;
            memory.set(
                "read_range",
                scope.create_function(|_, (_this, addr, len): (mlua::Value, u32, u32)| {
                    read_range_cpu(&nes_cell, addr, len)
                })?,
            )?;
            memory.set(
                "read_range_ppu",
                scope.create_function(|_, (_this, addr, len): (mlua::Value, u32, u32)| {
                    if len > 0x4000 {
                        return Err(mlua::Error::RuntimeError(
                            "memory:read_range_ppu length cannot exceed 16384".into(),
                        ));
                    }
                    let mut out = Vec::with_capacity(len as usize);
                    let mut nes = nes_cell.borrow_mut();
                    for i in 0..len {
                        #[allow(clippy::cast_possible_truncation)]
                        let a = (addr.wrapping_add(i) & 0x3FFF) as u16;
                        out.push(nes.ppu_bus_peek(a));
                    }
                    Ok(out)
                })?,
            )?;
            // v1.6.0 B3 — sized reads. Two CPU-bus `peek`s composed little- or
            // big-endian; observational (peek never perturbs the run), the
            // common TAS-script need for 16-bit values (positions, timers).
            memory.set(
                "read_u16_le",
                scope.create_function(|_, (_this, addr): (mlua::Value, u16)| {
                    let mut nes = nes_cell.borrow_mut();
                    Ok(u16::from_le_bytes([
                        nes.peek(addr),
                        nes.peek(addr.wrapping_add(1)),
                    ]))
                })?,
            )?;
            memory.set(
                "read_u16_be",
                scope.create_function(|_, (_this, addr): (mlua::Value, u16)| {
                    let mut nes = nes_cell.borrow_mut();
                    Ok(u16::from_be_bytes([
                        nes.peek(addr),
                        nes.peek(addr.wrapping_add(1)),
                    ]))
                })?,
            )?;
            // v1.6.0 B3 — OAM domain (sprite memory). The third read domain
            // alongside CPU (`peek`) and PPU (`peek_ppu`). `Nes::oam_byte` reads
            // one byte without copying the whole 256-byte array, so iterating
            // OAM in a script doesn't pay a full snapshot per access. Index
            // wraps to 8 bits.
            memory.set(
                "read_oam",
                scope.create_function(|_, (_this, index): (mlua::Value, u16)| {
                    #[allow(clippy::cast_possible_truncation)]
                    Ok(nes_cell.borrow().oam_byte((index & 0xFF) as u8))
                })?,
            )?;
            memory.set(
                "poke",
                scope.create_function(|_, (_this, addr, val): (mlua::Value, u16, u8)| {
                    if !writes_locked {
                        nes_cell.borrow_mut().poke_ram(addr, val);
                    }
                    Ok(())
                })?,
            )?;
            memory.set(
                "write_range",
                scope.create_function(|_, (_this, addr, bytes): (mlua::Value, u32, Vec<u8>)| {
                    if !writes_locked {
                        let mut nes = nes_cell.borrow_mut();
                        for (i, b) in bytes.iter().enumerate() {
                            #[allow(clippy::cast_possible_truncation)]
                            let a = (addr.wrapping_add(i as u32) & 0xFFFF) as u16;
                            nes.poke_ram(a, *b);
                        }
                    }
                    Ok(())
                })?,
            )?;
            lua.globals().set("memory", &memory)?;

            // v1.5.0 B2 — the `cart` table: read-only cart / system queries
            // (colon form; `self` ignored). All values are captured pre-scope,
            // so these are cheap constant returns that never touch the core.
            let cart = lua.create_table()?;
            cart.set(
                "mapper_id",
                lua.create_function(move |_, _this: mlua::Value| Ok(cart_mapper_id))?,
            )?;
            cart.set(
                "prg_size",
                lua.create_function(move |_, _this: mlua::Value| Ok(cart_prg_size))?,
            )?;
            cart.set(
                "chr_size",
                lua.create_function(move |_, _this: mlua::Value| Ok(cart_chr_size))?,
            )?;
            cart.set(
                "region",
                lua.create_function(move |_, _this: mlua::Value| Ok(cart_region))?,
            )?;
            cart.set("frame", frame)?;
            {
                let sha = cart_sha256.clone();
                cart.set(
                    "sha256",
                    lua.create_function(move |_, _this: mlua::Value| Ok(sha.clone()))?,
                )?;
            }
            lua.globals().set("cart", &cart)?;

            // v1.5.0 B3 — in-memory save-state slots on `emu`. `save_state(slot)`
            // is a read-only `Nes::snapshot` (always allowed). `load_state(slot)`
            // applies a stored snapshot via `Nes::restore` and is GATED IDENTICALLY
            // to `emu.write`: under a locked / replayed session it is a silent
            // no-op, so a deterministic / netplay / RA-hardcore run is unperturbed.
            // Distinct from the host's on-disk numbered slots (`emu.saveState`).
            emu.set(
                "save_state",
                scope.create_function(|lua, (_this, slot): (mlua::Value, u8)| {
                    let blob = nes_cell.borrow().snapshot();
                    state_slots.borrow_mut().insert(slot, blob);
                    // v1.7.0 B3 — fire `stateSaved(slot)` (observational; a save
                    // is a read-only snapshot, so it is always allowed).
                    fire_event_list(lua, &event_state_saved, u64::from(slot))?;
                    Ok(())
                })?,
            )?;
            emu.set(
                "load_state",
                scope.create_function(|lua, (_this, slot): (mlua::Value, u8)| {
                    if writes_locked {
                        return Ok(false);
                    }
                    let blob = state_slots.borrow().get(&slot).cloned();
                    // A restore failure (e.g. an empty slot mid-session) is
                    // surfaced as `false`, never a host crash.
                    let ok = blob.is_some_and(|blob| nes_cell.borrow_mut().restore(&blob).is_ok());
                    // v1.7.0 B3 — fire `stateLoaded(slot)` only on a successful
                    // restore (observational).
                    if ok {
                        fire_event_list(lua, &event_state_loaded, u64::from(slot))?;
                    }
                    Ok(ok)
                })?,
            )?;

            // v1.7.0 B3 — fire the `startFrame` event (Mesen2 parity), before
            // the onFrame callbacks. The arg is the frame count.
            if !event_start_frame.borrow().is_empty() {
                fire_event_list(lua, &event_start_frame, frame)?;
            }

            // Invoke every registered onFrame callback (from the Rust-side
            // registry — scripts cannot touch or corrupt it).
            for f in fns_for_frame(lua, &frame_cbs)? {
                f.call::<()>(())?;
            }

            // v1.6.0 B2 — resume the driving coroutine (if any) exactly once.
            // It runs until it next calls `emu.frameadvance()` (a
            // `coroutine.yield()`) or returns. On `Resumable`/`Running` it
            // yielded — we keep the handle for the next frame; on `Dead`/error
            // it finished — we drop the handle so a finished driver stops being
            // resumed. The driver shares the same gated `emu.write` /
            // `emu.setInput` / `load_state` accessors bound in this scope, so it
            // can never perturb the deterministic run beyond the write-gate.
            let driver_key = driver.borrow().as_ref().map(|k| {
                // Resolve the live thread handle from its registry key.
                lua.registry_value::<mlua::Thread>(k)
            });
            if let Some(thread_res) = driver_key {
                let thread = thread_res?;
                // A fresh coroutine resumes from its top; a yielded one resumes
                // where `frameadvance` left off. No values are passed in.
                let step = thread.resume::<()>(());
                match step {
                    Ok(()) => {
                        // Still resumable? keep it; finished? drop it.
                        if thread.status() != mlua::ThreadStatus::Resumable {
                            drop_driver(lua, &driver)?;
                        }
                    }
                    Err(e) => {
                        // A driver that raised is finished (and the error is
                        // surfaced to the host like any callback error).
                        drop_driver(lua, &driver)?;
                        return Err(e);
                    }
                }
            }

            // Replay this frame's committed interrupt services through
            // onNmi(vector) / onIrq(vector). Output-only; in service order.
            // `fns_for_frame` works for these flat Vec<RegistryKey> lists too.
            if !interrupts.is_empty() {
                for (is_nmi, vector) in &interrupts {
                    let list = if *is_nmi { &nmi_cbs } else { &irq_cbs };
                    for f in fns_for_frame(lua, list)? {
                        f.call::<()>(*vector)?;
                    }
                }
            }

            // Gate the hot replay loops on a stack-allocated bitset of the
            // registered addresses (covers the full 16-bit space in 8 KiB, no
            // heap allocation, no per-event `RefCell` borrow). Built only when
            // the matching loop will run, so there is zero cost unless a script
            // registers the corresponding callback (gemini #47/#57/#58). The
            // loops run ~15k (exec) + ~60k (access) times per frame.

            // Replay this frame's exec PCs through onExec(addr).
            if !exec_pcs.is_empty() {
                let active = AddrBits::from_keys(&exec_cbs);
                for pc in &exec_pcs {
                    if active.contains(*pc) {
                        for f in fns_at(lua, &exec_cbs, *pc)? {
                            f.call::<()>(*pc)?;
                        }
                    }
                }
            }

            // v1.5.0 B4 — replay this frame's exec PCs through on_breakpoint(pc).
            // Same observational exec-log replay as onExec, kept on a separate map
            // so a script can use breakpoints and onExec independently. Built only
            // when a breakpoint is registered (zero cost otherwise).
            if !exec_pcs.is_empty() && !breakpoint_cbs.borrow().is_empty() {
                let active = AddrBits::from_keys(&breakpoint_cbs);
                for pc in &exec_pcs {
                    if active.contains(*pc) {
                        for f in fns_at(lua, &breakpoint_cbs, *pc)? {
                            f.call::<()>(*pc)?;
                        }
                    }
                }
            }

            // Replay this frame's bus accesses through onRead/onWrite(addr, value).
            // Build each bitset only for a callback type that's actually in use
            // (most scripts watch only reads OR writes) — gemini/Copilot #59.
            if !accesses.is_empty() {
                let active_read = AddrBits::from_keys_opt(&read_cbs);
                let active_write = AddrBits::from_keys_opt(&write_cbs);
                // Resolve the `Option` discriminant once (these are 8 KiB enums);
                // the per-access check is then a cheap `Option<&AddrBits>` pointer
                // test rather than re-reading the discriminant 60k times (gemini #61).
                let (active_read, active_write) = (active_read.as_ref(), active_write.as_ref());
                for (is_write, addr, value) in &accesses {
                    let (active, map) = if *is_write {
                        (active_write, &write_cbs)
                    } else {
                        (active_read, &read_cbs)
                    };
                    if active.is_some_and(|a| a.contains(*addr)) {
                        for f in fns_at(lua, map, *addr)? {
                            f.call::<()>((*addr, *value))?;
                        }
                    }
                }
            }

            // v1.7.0 B3 — replay this frame's WRITES through the VALUE-MODIFYING
            // memory callbacks (`emu.addMemoryCallback(fn,"write",...)`). Unlike
            // the observational `onWrite` above, a callback here may RETURN a
            // replacement byte; we poke it back through the GATED `poke_ram`
            // path (the scriptable-cheat / scriptable-watchpoint primitive). The
            // poke is the mutation, so it is gated IDENTICALLY to `emu.write`:
            // under a locked / replayed session every poke is dropped, so a
            // value-modify callback can't perturb a deterministic run. (Reads
            // are skipped — a post-frame replay can't retroactively change a read
            // the CPU already consumed.)
            // Check `!writes_locked` FIRST: the poke is this loop's only effect
            // and it is gated like `emu.write`, so under a locked / replayed
            // session the entire replay (and every callback invocation) is
            // skipped — no wasted callback work and the gate short-circuits.
            if !writes_locked && !accesses.is_empty() && !modify_write_cbs.borrow().is_empty() {
                let active = AddrBits::from_keys(&modify_write_cbs);
                for (is_write, addr, value) in &accesses {
                    if *is_write && active.contains(*addr) {
                        for f in fns_at(lua, &modify_write_cbs, *addr)? {
                            // The callback returns a new byte (or nil to leave
                            // the value unchanged); poke it back through the
                            // gated path (we're already in the unlocked branch).
                            if let Some(new_val) = f.call::<Option<u8>>((*addr, *value))? {
                                nes_cell.borrow_mut().poke_ram(*addr, new_val);
                            }
                        }
                    }
                }
            }

            // v1.7.0 B3 — fire `inputPolled` (if the frame polled a controller)
            // then `endFrame`, after all per-frame work (Mesen2 parity).
            if input_was_polled && !event_input_polled.borrow().is_empty() {
                fire_event_list(lua, &event_input_polled, frame)?;
            }
            if !event_end_frame.borrow().is_empty() {
                fire_event_list(lua, &event_end_frame, frame)?;
            }
            Ok(())
        });

        lua.remove_hook();
        result.map_err(ScriptError::from)
    }

    fn frame_callback_count(&self) -> usize {
        self.frame_cbs.borrow().len()
    }

    // ---- v1.7.0 "Forge" Workstream B ----

    fn set_tas_snapshot(&self, snapshot: TasSnapshot) {
        *self.tas.snapshot.borrow_mut() = snapshot;
    }

    fn drain_tas_commands(&self) -> Vec<TasCmd> {
        std::mem::take(&mut self.tas.commands.borrow_mut())
    }

    fn query_tas_cell(&self, frame: usize, column: u32) -> Result<TasCellDecor, ScriptError> {
        tastudio::query_cell(&self.lua, &self.tas, frame, column).map_err(ScriptError::from)
    }

    fn take_clear_icon_cache(&self) -> bool {
        self.tas.clear_icon_cache.replace(false)
    }

    fn fire_greenzone_invalidated(&self, first_frame: usize) -> Result<(), ScriptError> {
        tastudio::fire_event(&self.lua, &self.tas.greenzone_cbs, first_frame)
            .map_err(ScriptError::from)
    }

    fn fire_branch_load(&self, index: usize) -> Result<(), ScriptError> {
        tastudio::fire_event(&self.lua, &self.tas.branch_load_cbs, index).map_err(ScriptError::from)
    }

    fn needs_tas_cell_query(&self) -> bool {
        self.tas.needs_cell_query()
    }

    fn set_script_data_folder(&self, path: Option<String>) {
        *self.script_data_folder.borrow_mut() = path;
    }

    // v1.7.0 "Forge" Workstream E — host IPC / automation drains.

    fn drain_clients(&self) -> Vec<ClientCmd> {
        std::mem::take(&mut self.clients.borrow_mut())
    }

    #[cfg(feature = "script-ipc")]
    fn drain_comm(&self) -> Vec<CommCmd> {
        std::mem::take(&mut self.comm_out.borrow_mut())
    }

    #[cfg(feature = "script-ipc")]
    fn push_comm_result(&self, result: CommResult) {
        let mut inbox = self.comm_in.borrow_mut();
        // Bound the inbox so a host (or a script that never drains it) can't grow
        // memory without limit; drop the oldest on overflow.
        if inbox.len() >= MAX_QUEUED_CMDS {
            inbox.pop_front();
        }
        inbox.push_back(result);
    }

    fn userdata_snapshot(&self) -> Vec<(String, String)> {
        let mut pairs: Vec<(String, String)> = self
            .userdata
            .borrow()
            .iter()
            .map(|(k, v)| (k.clone(), v.clone()))
            .collect();
        // Sorted by key so the persisted/save-state blob is deterministic.
        pairs.sort_unstable_by(|a, b| a.0.cmp(&b.0));
        pairs
    }

    fn userdata_restore(&self, pairs: &[(String, String)]) {
        let mut kv = self.userdata.borrow_mut();
        kv.clear();
        for (k, v) in pairs {
            kv.insert(k.clone(), v.clone());
        }
    }
}

/// Best-effort `Value` -> display string for the log sink.
fn value_to_string(v: &mlua::Value) -> String {
    match v {
        mlua::Value::String(s) => s.to_string_lossy(),
        mlua::Value::Integer(i) => i.to_string(),
        mlua::Value::Number(n) => n.to_string(),
        mlua::Value::Boolean(b) => b.to_string(),
        mlua::Value::Nil => "nil".to_owned(),
        // Tables / functions / userdata: render `tostring`-style ("table",
        // "function", ...) rather than a noisy `{:?}` debug dump (L4).
        other => other.type_name().to_owned(),
    }
}
