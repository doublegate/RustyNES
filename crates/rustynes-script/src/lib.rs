//! Sandboxed Lua scripting engine for `RustyNES`.
//!
//! Exposes a small, Mesen2 / FCEUX-style `emu` API to user scripts: read /
//! write CPU-bus memory, inspect CPU registers + the frame / cycle counters,
//! log messages, draw an overlay, drive a few control actions, and register
//! per-frame / per-access / per-interrupt callbacks. The engine is **driven by
//! the host** (the frontend), never the other way around: the host calls
//! [`ScriptEngine::on_frame`] once per emulated frame, which binds the
//! live-`Nes` accessors and invokes every registered callback.
//!
//! ## Two VM backends behind one [`VmBackend`] contract (v1.2.0 Workstream F4)
//!
//! The host-facing surface — the `emu.*` API, the [`ControlCmd`] / [`DrawCmd`]
//! / log queues, the callback registration + per-frame replay, and the
//! [`ScriptEngine::set_writes_locked`] write-gate — is the contract spelled out
//! by the [`VmBackend`] trait (see `backend.rs`). Exactly one backend is
//! compiled in, selected by a cargo feature (a `cfg`, not a runtime `dyn` —
//! piccolo's `gc-arena` `'gc` lifetime makes a trait object impractical):
//!
//! - **`mlua-backend` (default):** `mlua` (vendored **Lua 5.4**, C), the
//!   production path. Native-only. Behavior is unchanged from v1.1.0 and is the
//!   byte-identical reference the determinism contract is defined against. This
//!   is what the frontend's `scripting` feature uses.
//! - **`script-wasm` (EXPERIMENTAL, off by default):** `piccolo`, a pure-Rust
//!   Lua VM, so scripting can compile to `wasm32-unknown-unknown` with no C
//!   toolchain. It is **explicitly not byte-parity** with mlua (a different VM,
//!   with deferred writes and snapshot-based reads). That is acceptable because
//!   scripts are observational / overlay + gated writes and are **never** part
//!   of the framebuffer / audio determinism oracle. It hosts the observational
//!   subset piccolo supports cleanly; the per-access (`onExec`/`onRead`/
//!   `onWrite`) and per-interrupt (`onNmi`/`onIrq`) replay callbacks are a
//!   documented native-only limitation (registered, but never fired on wasm).
//!   See `docs/adr/0012-wasm-lua-piccolo-backend.md`.
//!
//! `script-wasm` wins if both features are enabled, because mlua's C cannot
//! link on wasm. `ScriptEngine` is a thin newtype over whichever backend is
//! compiled in, so downstream code (`rustynes-frontend`) uses the same name and
//! the same method set on both.
//!
//! ## Determinism + safety (both backends)
//!
//! - The default build does **not** pull this crate in (the frontend's
//!   `scripting` feature is off by default), so the shipped emulator is
//!   byte-identical and carries no Lua dependency unless scripting is enabled.
//! - **Sandbox:** only the safe, side-effect-free standard libraries load — no
//!   `io`, `os`, `package` / `require`, `debug`, and no `load` / `loadfile` /
//!   `dofile` / `loadstring`. `print` is kept but redirected to the captured
//!   log.
//! - **Budget guard:** a per-frame instruction budget aborts a callback that
//!   runs away (default [`DEFAULT_INSTRUCTION_BUDGET`]). On mlua this is a
//!   VM-instruction hook; on piccolo it is the VM's `Fuel` mechanism.
//! - **Write gating:** when the host sets [`ScriptEngine::set_writes_locked`]
//!   (netplay / TAS replay / RA-hardcore), `emu.write` AND `emu.setInput`
//!   become silent no-ops, so a script cannot perturb a deterministic / locked
//!   session — the same policy as the Game Genie / raw-RAM cheat path.

mod backend;
mod types;

pub use backend::VmBackend;
pub use types::{
    ClientCmd, ControlCmd, DEFAULT_INSTRUCTION_BUDGET, DrawCmd, ScriptError, TasBranchInfo,
    TasCellDecor, TasCmd, TasSnapshot,
};
#[cfg(feature = "script-ipc")]
pub use types::{CommCmd, CommResult};

use rustynes_core::Nes;

// Exactly one backend is compiled in. `script-wasm` (piccolo) wins when both
// features are on, because mlua's vendored C cannot link on wasm32. (On the
// frontend the two are further isolated: mlua sits in the `cfg(not(wasm32))`
// dep table and piccolo in the `cfg(wasm32)` one, so they never co-resolve.)
#[cfg(feature = "script-wasm")]
mod piccolo_backend;
#[cfg(feature = "script-wasm")]
use piccolo_backend::PiccoloBackend as Backend;

#[cfg(all(feature = "mlua-backend", not(feature = "script-wasm")))]
mod mlua_backend;
#[cfg(all(feature = "mlua-backend", not(feature = "script-wasm")))]
use mlua_backend::MluaBackend as Backend;

// v1.7.0 "Forge" Workstream B — the scriptable-`TAStudio` Lua surface, a
// self-contained native-only module on the mlua backend.
#[cfg(all(feature = "mlua-backend", not(feature = "script-wasm")))]
mod tastudio;

/// A sandboxed Lua scripting engine bound to one emulator session.
///
/// A thin facade over the compile-time-selected [`VmBackend`]; the public API
/// is identical for both the mlua (native) and piccolo (experimental wasm)
/// backends. The inherent methods below mirror the trait so callers never need
/// the trait in scope.
#[cfg(any(feature = "mlua-backend", feature = "script-wasm"))]
pub struct ScriptEngine {
    inner: Backend,
}

#[cfg(any(feature = "mlua-backend", feature = "script-wasm"))]
impl ScriptEngine {
    /// Build a fresh sandboxed engine (no script loaded yet).
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError`] if the sandbox prelude fails to install.
    pub fn new() -> Result<Self, ScriptError> {
        Ok(Self {
            inner: Backend::new()?,
        })
    }

    /// Load (and execute the top level of) a Lua script. Top-level code
    /// typically registers callbacks via `emu.onFrame(...)`.
    ///
    /// Takes `&mut self`: the piccolo backend's VM (`Lua::try_enter`) needs
    /// `&mut`. (The mlua backend's underlying load is `&self`-friendly, but the
    /// trait unifies on `&mut self` so both backends share the facade.)
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError`] on a syntax or top-level runtime error, or if the
    /// load exceeded the per-frame instruction budget.
    pub fn load(&mut self, src: &str) -> Result<(), ScriptError> {
        self.inner.load(src)
    }

    /// Run one emulated frame's worth of scripting: bind the live-`Nes`
    /// accessors and invoke every registered `onFrame` callback.
    ///
    /// # Errors
    ///
    /// Returns [`ScriptError`] if a callback raises or busts the budget.
    pub fn on_frame(&mut self, nes: &mut Nes) -> Result<(), ScriptError> {
        self.inner.on_frame(nes)
    }

    /// Set the per-frame VM-instruction / fuel budget (runaway-loop guard).
    pub fn set_instruction_budget(&self, budget: u64) {
        self.inner.set_instruction_budget(budget);
    }

    /// Gate `emu.write` AND `emu.setInput`: when `true` (netplay / TAS replay /
    /// RA-hardcore) both are silently dropped so a script cannot perturb a
    /// locked / replayed session.
    pub fn set_writes_locked(&self, locked: bool) {
        self.inner.set_writes_locked(locked);
    }

    /// v1.5.0 Workstream B (B4) — push the host's loaded debugger symbols
    /// (`address -> label`) into the engine so a script's `sym:addr(name)` /
    /// `sym:name(addr)` queries resolve against them. Read-only; never perturbs
    /// deterministic state. A no-op on the experimental piccolo backend.
    pub fn set_symbols(&self, pairs: &[(u16, String)]) {
        self.inner.set_symbols(pairs);
    }

    /// Drain captured log / `print` output (oldest first).
    #[must_use]
    pub fn drain_log(&self) -> Vec<String> {
        self.inner.drain_log()
    }

    /// Drain the control actions requested since the last call. The host
    /// applies (and gates) them after [`Self::on_frame`].
    #[must_use]
    pub fn drain_controls(&self) -> Vec<ControlCmd> {
        self.inner.drain_controls()
    }

    /// Drain the overlay draw commands issued this frame (host renders them).
    #[must_use]
    pub fn drain_draws(&self) -> Vec<DrawCmd> {
        self.inner.drain_draws()
    }

    /// `true` if any `onExec` callback is registered — the host should enable
    /// [`rustynes_core::Nes::set_exec_logging`]. Always `false` on the piccolo
    /// backend (per-access callbacks are native-only; ADR 0012).
    #[must_use]
    pub fn needs_exec_log(&self) -> bool {
        self.inner.needs_exec_log()
    }

    /// `true` if any `onRead`/`onWrite` callback is registered — the host should
    /// enable [`rustynes_core::Nes::set_access_logging`]. Always `false` on the
    /// piccolo backend (native-only; ADR 0012).
    #[must_use]
    pub fn needs_access_log(&self) -> bool {
        self.inner.needs_access_log()
    }

    /// `true` if any `onNmi`/`onIrq` callback is registered — the host should
    /// enable [`rustynes_core::Nes::set_interrupt_logging`]. Always `false` on
    /// the piccolo backend (native-only; ADR 0012).
    #[must_use]
    pub fn needs_interrupt_log(&self) -> bool {
        self.inner.needs_interrupt_log()
    }

    /// Number of registered `onFrame` callbacks (for the host UI / tests).
    #[must_use]
    pub fn frame_callback_count(&self) -> usize {
        self.inner.frame_callback_count()
    }

    // ---- v1.7.0 "Forge" Workstream B — scriptable TAStudio + Lua parity ----

    /// B1 — push a read-only snapshot of the host's live `TAStudio` editor so
    /// the `tastudio.*` query API resolves against current editor state. The
    /// host pushes this each frame before [`Self::on_frame`]. A no-op on the
    /// piccolo backend (the dev/TAS surface is native-only; ADR 0012).
    pub fn set_tas_snapshot(&self, snapshot: TasSnapshot) {
        self.inner.set_tas_snapshot(snapshot);
    }

    /// B1 — drain the `TAStudio` editor actions a script requested this frame
    /// (`tastudio.*` mutators); the host applies + gates them. Always empty on
    /// the piccolo backend.
    #[must_use]
    pub fn drain_tas_commands(&self) -> Vec<TasCmd> {
        self.inner.drain_tas_commands()
    }

    /// B2 — the per-cell decoration a script's `onqueryitem*` callbacks produce
    /// for piano-roll cell `(frame, column)`. The default (no decoration) when
    /// no callback is registered or on the piccolo backend.
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises.
    pub fn query_tas_cell(&self, frame: usize, column: u32) -> Result<TasCellDecor, ScriptError> {
        self.inner.query_tas_cell(frame, column)
    }

    /// B2 — whether a script asked to clear the piano-roll icon cache
    /// (`tastudio.clearIconCache()`) since the last drain.
    #[must_use]
    pub fn take_clear_icon_cache(&self) -> bool {
        self.inner.take_clear_icon_cache()
    }

    /// B2 — invoke the registered `ongreenzoneinvalidated(fn)` callbacks with
    /// the first invalidated frame (the host calls this after an edit).
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises.
    pub fn fire_greenzone_invalidated(&self, first_frame: usize) -> Result<(), ScriptError> {
        self.inner.fire_greenzone_invalidated(first_frame)
    }

    /// B2 — invoke the registered `onbranchload(fn)` callbacks with the loaded
    /// branch index (the host calls this after a branch loads).
    ///
    /// # Errors
    /// Returns [`ScriptError`] if a callback raises.
    pub fn fire_branch_load(&self, index: usize) -> Result<(), ScriptError> {
        self.inner.fire_branch_load(index)
    }

    /// B2 — `true` if any `tastudio.onqueryitem*` callback is registered, so the
    /// host knows to call [`Self::query_tas_cell`] while painting the grid.
    #[must_use]
    pub fn needs_tas_cell_query(&self) -> bool {
        self.inner.needs_tas_cell_query()
    }

    /// B3 — set the per-script sandboxed data directory returned by
    /// `emu.getScriptDataFolder()` (`None` clears it). A no-op on the piccolo
    /// backend.
    pub fn set_script_data_folder(&self, path: Option<String>) {
        self.inner.set_script_data_folder(path);
    }

    /// v1.7.0 "Forge" Workstream E2 — drain the `client.*` automation verbs a
    /// script requested this frame. The host applies (and gates the mutators
    /// among) them after [`Self::on_frame`], exactly like [`Self::drain_controls`].
    #[must_use]
    pub fn drain_clients(&self) -> Vec<ClientCmd> {
        self.inner.drain_clients()
    }

    /// v1.7.0 "Forge" Workstream E1 — drain the host-mediated `comm.*` IPC
    /// requests a script issued this frame. The host (which owns every
    /// connection) performs the I/O off the emulator lock and feeds results back
    /// via [`Self::push_comm_result`]. Empty unless the `script-ipc` feature is
    /// on AND the host has not locked writes.
    #[cfg(feature = "script-ipc")]
    #[must_use]
    pub fn drain_comm(&self) -> Vec<CommCmd> {
        self.inner.drain_comm()
    }

    /// v1.7.0 "Forge" Workstream E1 — deliver a host-fulfilled [`CommResult`]
    /// back to the engine. Surfaced to the script on the next pump via the
    /// polled `comm.receive()` queue. The host calls this off the emulator lock.
    #[cfg(feature = "script-ipc")]
    pub fn push_comm_result(&self, result: CommResult) {
        self.inner.push_comm_result(result);
    }

    /// v1.7.0 "Forge" Workstream E3 — snapshot the per-script `userdata.*` KV
    /// store as `(key, value)` string pairs (sorted by key for determinism). The
    /// host persists this into save-states / on-disk so the store survives across
    /// runs. The KV store is script-local host memory, never emulator state, so
    /// snapshotting it never perturbs the deterministic core.
    #[must_use]
    pub fn userdata_snapshot(&self) -> Vec<(String, String)> {
        self.inner.userdata_snapshot()
    }

    /// v1.7.0 "Forge" Workstream E3 — replace the `userdata.*` KV store from a
    /// snapshot (the host restores it from a save-state / disk on script load).
    pub fn userdata_restore(&self, pairs: &[(String, String)]) {
        self.inner.userdata_restore(pairs);
    }
}

#[cfg(all(test, any(feature = "mlua-backend", feature = "script-wasm")))]
mod tests {
    use super::*;

    /// A minimal NROM ROM whose reset vector loops `JMP $C000`.
    fn synth_rom() -> Vec<u8> {
        let mut bytes = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
        bytes.resize(16, 0);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0] = 0x4C; // JMP $C000
        prg[1] = 0x00;
        prg[2] = 0xC0;
        let len = prg.len();
        prg[len - 4] = 0x00; // reset vector lo
        prg[len - 3] = 0xC0; // reset vector hi
        bytes.extend_from_slice(&prg);
        bytes.resize(16 + 16 * 1024 + 8 * 1024, 0); // 8 KiB CHR
        bytes
    }

    #[test]
    fn loads_and_runs_on_frame_callback() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            count = 0
            emu.onFrame(function() count = count + 1; emu.log('tick ' .. count) end)
            ",
        )
        .expect("load");
        assert_eq!(eng.frame_callback_count(), 1);
        for _ in 0..3 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
        }
        let log = eng.drain_log();
        assert_eq!(log, vec!["tick 1", "tick 2", "tick 3"]);
    }

    #[test]
    fn memory_read_and_write_round_trip() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                emu.write(0x10, 0x42)
                emu.log('mem10=' .. emu.read(0x10))
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        // The mlua backend writes immediately, so the SAME-frame `emu.read` sees
        // 0x42 and logs `mem10=66`. The piccolo backend DEFERS writes until after
        // callbacks (it can't hold a live `&mut Nes` mid-callback), so the
        // same-frame read returns the pre-write value — a documented divergence
        // (ADR 0012); the deferred write still lands (asserted below). The log
        // assertion is therefore the mlua reference.
        #[cfg(not(feature = "script-wasm"))]
        assert_eq!(eng.drain_log(), vec!["mem10=66"]);
        assert_eq!(nes.peek(0x10), 0x42);
    }

    #[test]
    fn writes_are_gated_when_locked() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_writes_locked(true);
        eng.load("emu.onFrame(function() emu.write(0x20, 0x99) end)")
            .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(nes.peek(0x20), 0x00, "write must be dropped when locked");
    }

    #[test]
    fn cpu_state_is_exposed() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load("emu.onFrame(function() emu.log('pc=' .. emu.cpu().pc) end)")
            .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(eng.drain_log()[0].starts_with("pc="));
    }

    #[test]
    fn sandbox_blocks_io_os_and_loaders() {
        let mut eng = ScriptEngine::new().expect("engine");
        for probe in [
            "return io.open('/etc/passwd')",
            "return os.execute('echo hi')",
            "return require('os')",
            "return load('return 1')",
            "return dofile('/etc/passwd')",
            "return package.path",
        ] {
            assert!(eng.load(probe).is_err(), "sandbox must reject: {probe}");
        }
    }

    #[test]
    fn runaway_loop_hits_the_budget() {
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_instruction_budget(100_000);
        let err = eng.load("while true do end").unwrap_err();
        // mlua surfaces it as a runtime `Lua` error; piccolo as `Budget`.
        assert!(matches!(err, ScriptError::Lua(_) | ScriptError::Budget));
    }

    /// NROM whose boot loop writes `$2000` each iteration:
    /// `LDA #$80; STA $2000; JMP $C000`.
    #[cfg(not(feature = "script-wasm"))]
    fn synth_writing_rom() -> Vec<u8> {
        let mut bytes = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
        bytes.resize(16, 0);
        let mut prg = vec![0u8; 16 * 1024];
        prg[0..8].copy_from_slice(&[0xA9, 0x80, 0x8D, 0x00, 0x20, 0x4C, 0x00, 0xC0]);
        let len = prg.len();
        prg[len - 4] = 0x00; // reset vector -> $C000
        prg[len - 3] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.resize(16 + 16 * 1024 + 8 * 1024, 0);
        bytes
    }

    // The per-access (`onExec`/`onRead`/`onWrite`) and per-interrupt
    // (`onNmi`/`onIrq`) replay callbacks are native-only (a documented piccolo
    // limitation, ADR 0012), so these tests run on the mlua backend only.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn on_write_fires_from_the_access_log() {
        let mut nes = Nes::from_rom(&synth_writing_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            hits = 0
            emu.onWrite(0x2000, function(addr, val) hits = hits + 1 end)
            emu.onFrame(function() emu.log('hits=' .. hits) end)
            ",
        )
        .expect("load");
        assert!(eng.needs_access_log());
        assert!(!eng.needs_exec_log());
        // The host enables the access log per `needs_access_log`.
        nes.set_access_logging(true);
        let mut saw = false;
        for _ in 0..4 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
            if eng.drain_log().iter().any(|l| l != "hits=0") {
                saw = true;
                break;
            }
        }
        assert!(saw, "onWrite($2000) should fire from the bus-access log");
    }

    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn on_exec_fires_from_the_exec_log() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        // The boot loop sits at $C000 (JMP $C000); onExec there must fire.
        eng.load(
            r"
            seen = false
            emu.onExec(0xC000, function(pc) seen = true end)
            emu.onFrame(function() if seen then emu.log('exec') end end)
            ",
        )
        .expect("load");
        assert!(eng.needs_exec_log());
        nes.set_exec_logging(true);
        let mut saw = false;
        for _ in 0..4 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
            if eng.drain_log().contains(&"exec".to_owned()) {
                saw = true;
                break;
            }
        }
        assert!(saw, "onExec($C000) should fire from the exec log");
    }

    #[test]
    fn callback_registry_is_not_script_visible() {
        // The architectural fix: callbacks live Rust-side, so `__rustynes` is
        // NOT a script-visible global. A script cannot inspect, clobber, or
        // inject junk into the registry — the entire "malformed registry value
        // crashes the host" class is gone by construction.
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            assert(__rustynes == nil, 'registry must not be a global')
            -- Even hostile writes only create an unused global; the real
            -- registry is untouched.
            __rustynes = { frame = { 'junk' } }
            emu.onFrame(function() emu.log('ok') end)
            ",
        )
        .expect("load");
        assert_eq!(eng.frame_callback_count(), 1);
        for _ in 0..3 {
            nes.run_frame();
            eng.on_frame(&mut nes)
                .expect("script junk can never reach the host pump");
        }
        assert!(eng.drain_log().contains(&"ok".to_owned()));
    }

    /// NROM whose boot loop re-enables the vblank NMI each iteration then
    /// re-loops; the NMI handler is a bare `RTI`. Re-writing `$2000` every
    /// iteration matters: the PPU ignores register writes during the ~30k-cycle
    /// post-reset warmup, so a single boot-time `STA $2000` would be dropped and
    /// no NMI would ever fire. Once warmup passes the PPU fires one NMI per
    /// frame, so the interrupt-service log records an NMI ($FFFA) each frame.
    /// `loop: LDA #$80; STA $2000; JMP loop` ($C000).
    #[cfg(not(feature = "script-wasm"))]
    fn synth_nmi_rom() -> Vec<u8> {
        let mut bytes = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
        bytes.resize(16, 0);
        let mut prg = vec![0u8; 16 * 1024];
        // $C000: LDA #$80 ; STA $2000 ; JMP $C000.
        prg[0..8].copy_from_slice(&[0xA9, 0x80, 0x8D, 0x00, 0x20, 0x4C, 0x00, 0xC0]);
        // $C008: RTI (the NMI handler).
        prg[8] = 0x40;
        let len = prg.len();
        prg[len - 4] = 0x00; // reset vector -> $C000
        prg[len - 3] = 0xC0;
        prg[len - 6] = 0x08; // NMI vector ($FFFA) -> $C008
        prg[len - 5] = 0xC0;
        bytes.extend_from_slice(&prg);
        bytes.resize(16 + 16 * 1024 + 8 * 1024, 0);
        bytes
    }

    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn on_nmi_fires_from_the_interrupt_log() {
        let mut nes = Nes::from_rom(&synth_nmi_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            nmis = 0
            last_vec = 0
            emu.onNmi(function(vector) nmis = nmis + 1; last_vec = vector end)
            emu.onFrame(function() emu.log('nmis=' .. nmis .. ' vec=' .. last_vec) end)
            ",
        )
        .expect("load");
        assert!(eng.needs_interrupt_log());
        // The host enables the interrupt log per `needs_interrupt_log`.
        nes.set_interrupt_logging(true);
        let mut saw = false;
        for _ in 0..6 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
            if eng
                .drain_log()
                .iter()
                .any(|l| l.starts_with("nmis=") && !l.contains("nmis=0 "))
            {
                saw = true;
                break;
            }
        }
        assert!(saw, "onNmi should fire from the committed interrupt log");
    }

    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn on_irq_does_not_fire_without_an_irq_source() {
        // The NMI ROM never raises an IRQ, so onIrq must stay silent even with
        // the interrupt log armed (proving the dispatch keys on `is_nmi`).
        let mut nes = Nes::from_rom(&synth_nmi_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            irqs = 0
            emu.onIrq(function(vector) irqs = irqs + 1 end)
            emu.onFrame(function() emu.log('irqs=' .. irqs) end)
            ",
        )
        .expect("load");
        nes.set_interrupt_logging(true);
        for _ in 0..6 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
        }
        assert!(
            eng.drain_log().iter().all(|l| l == "irqs=0"),
            "onIrq must not fire when no IRQ is serviced"
        );
    }

    #[test]
    fn set_input_is_gated_when_locked() {
        // T-110-E2 determinism guard: under a locked session (netplay / TAS
        // replay / RA-hardcore — all surfaced via `set_writes_locked`) a script
        // `emu.setInput` is dropped at the source, so NO `SetInput` control
        // command ever reaches the host.
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_writes_locked(true);
        eng.load("emu.onFrame(function() emu.setInput(0, 0x81) end)")
            .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            eng.drain_controls().is_empty(),
            "setInput must be a no-op (no queued ControlCmd) when writes are locked"
        );

        // And the inverse: unlocked, the same call DOES queue a SetInput.
        eng.set_writes_locked(false);
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            eng.drain_controls().contains(&ControlCmd::SetInput {
                port: 0,
                buttons: 0x81
            }),
            "setInput must queue a SetInput command when unlocked"
        );
    }

    #[test]
    fn control_and_draw_commands_are_queued() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                emu.pause()
                emu.saveState(2)
                emu.setInput(0, 0x81)
                emu.drawText(10, 20, 'HP: 3', 0xFF0000FF)
                emu.drawRect(0, 0, 8, 8)
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");

        let controls = eng.drain_controls();
        assert!(controls.contains(&ControlCmd::Pause));
        assert!(controls.contains(&ControlCmd::SaveState(2)));
        assert!(controls.contains(&ControlCmd::SetInput {
            port: 0,
            buttons: 0x81
        }));
        let draws = eng.drain_draws();
        assert_eq!(draws.len(), 2);
        assert!(matches!(&draws[0], DrawCmd::Text { text, .. } if text == "HP: 3"));
        // Drained — a second drain is empty.
        assert!(eng.drain_controls().is_empty());
    }

    // ----- v1.5.0 Workstream B: Lua dev/TAS API depth (native-only / mlua) -----
    // The `memory` / `cart` / `sym` tables + in-memory save-state slots +
    // `on_breakpoint` / `pause_at_frame` are installed only on the mlua backend
    // (the dev/TAS surface is native-only, the same carve-out as the per-access
    // and per-interrupt callbacks; ADR 0012). So these tests run on mlua only.

    /// B1 — `memory:peek` / `poke` / `read_range` / `write_range` (CPU space)
    /// round-trip through the live `Nes`. Reads use the side-effect-free path.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn memory_table_cpu_round_trip() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                memory:poke(0x30, 0x12)
                memory:write_range(0x31, { 0x34, 0x56 })
                local b = memory:peek(0x30)
                local r = memory:read_range(0x30, 3)
                emu.log('m=' .. b .. ' a=' .. r[1] .. ',' .. r[2] .. ',' .. r[3])
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(eng.drain_log(), vec!["m=18 a=18,52,86"]);
        assert_eq!(nes.peek(0x30), 0x12);
        assert_eq!(nes.peek(0x31), 0x34);
        assert_eq!(nes.peek(0x32), 0x56);
    }

    /// B1 — `memory:peek` is side-effect-free: a script peek of `$2002` must NOT
    /// clear the VBL flag (the debug-peek path), so a subsequent CPU read still
    /// sees it. We assert the engine peek matches the core's own `peek` (both
    /// the side-effect-free path) before and after, i.e. peeking is idempotent.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn memory_peek_is_side_effect_free() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                local a = memory:peek(0x2002)
                local b = memory:peek(0x2002)
                emu.log('eq=' .. tostring(a == b))
            end)
            ",
        )
        .expect("load");
        // Run several frames so the PPU has set/!set VBL at various points; the
        // two back-to-back peeks must always agree (no latch was consumed).
        for _ in 0..4 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
        }
        assert!(eng.drain_log().iter().all(|l| l == "eq=true"));
    }

    /// B1 — `memory:poke` / `write_range` are GATED identically to `emu.write`:
    /// under a locked session they are silent no-ops.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn memory_poke_and_write_range_gated_when_locked() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_writes_locked(true);
        eng.load(
            r"
            emu.onFrame(function()
                memory:poke(0x40, 0x99)
                memory:write_range(0x41, { 0xAA, 0xBB })
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(nes.peek(0x40), 0x00, "poke must be dropped when locked");
        assert_eq!(
            nes.peek(0x41),
            0x00,
            "write_range must be dropped when locked"
        );
        assert_eq!(nes.peek(0x42), 0x00);
    }

    /// v1.6.0 B3 — sized reads (`read_u16_le` / `read_u16_be`) compose two CPU
    /// `peek`s, and `read_oam` exposes the sprite-RAM domain. Observational, so
    /// they never perturb the deterministic run.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn memory_sized_reads_and_oam_domain() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                memory:poke(0x10, 0x34)
                memory:poke(0x11, 0x12)
                emu.log('le=' .. memory:read_u16_le(0x10))
                emu.log('be=' .. memory:read_u16_be(0x10))
                emu.log('oam=' .. memory:read_oam(0))
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        let log = eng.drain_log();
        // $34 (lo) | $12 (hi) << 8 = $1234 = 4660 little-endian; $3412 = 13330 big.
        assert!(log.contains(&"le=4660".to_string()), "LE word: got {log:?}");
        assert!(
            log.contains(&"be=13330".to_string()),
            "BE word: got {log:?}"
        );
        assert!(
            log.iter().any(|l| l.starts_with("oam=")),
            "read_oam returns a byte: got {log:?}"
        );
    }

    /// v1.6.0 B3 — `joypad:get(port)` reads the latched standard-controller
    /// bitmask (side-effect-free), and `joypad:set` queues the same gated
    /// `ControlCmd::SetInput` as `emu.setInput` (a silent no-op when locked).
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn joypad_get_reads_latched_and_set_is_gated() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                emu.log('p1=' .. joypad:get(0))
                joypad:set(1, 0xC1)
            end)
            ",
        )
        .expect("load");
        // A | START = bit0 | bit3 = 0x09 = 9.
        nes.set_buttons(0, rustynes_core::Buttons::A | rustynes_core::Buttons::START);
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        let log = eng.drain_log();
        assert!(
            log.contains(&"p1=9".to_string()),
            "joypad:get reads A|START: {log:?}"
        );
        let controls = eng.drain_controls();
        assert!(
            controls.iter().any(|c| matches!(
                c,
                ControlCmd::SetInput {
                    port: 1,
                    buttons: 0xC1
                }
            )),
            "joypad:set queues SetInput: {controls:?}"
        );
        // Gated under a locked session, exactly like emu.setInput.
        eng.set_writes_locked(true);
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            !eng.drain_controls()
                .iter()
                .any(|c| matches!(c, ControlCmd::SetInput { .. })),
            "joypad:set must be dropped when locked"
        );
    }

    /// B2 — `cart:` read-only queries surface the loaded ROM's metadata. The
    /// synthetic NROM is a 16 KiB-PRG / 8 KiB-CHR mapper-0 NTSC cart.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn cart_queries_report_metadata() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                emu.log('id=' .. cart:mapper_id()
                    .. ' prg=' .. cart:prg_size()
                    .. ' chr=' .. cart:chr_size()
                    .. ' region=' .. cart:region()
                    .. ' shalen=' .. #cart:sha256())
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(
            eng.drain_log(),
            vec![format!(
                "id=0 prg={} chr={} region=NTSC shalen=64",
                16 * 1024,
                8 * 1024
            )]
        );
    }

    /// B3 — in-memory `emu:save_state` / `load_state` round-trips emulator state.
    /// The boot loop writes `$80` to `$2000` each iteration; we checkpoint RAM
    /// byte `$05` (poked by the script), advance, then roll back and confirm the
    /// restore took.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn save_and_load_state_slots_round_trip() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            phase = 0
            emu.onFrame(function()
                phase = phase + 1
                if phase == 1 then
                    memory:poke(0x05, 0x77)
                    emu:save_state(1)
                elseif phase == 2 then
                    memory:poke(0x05, 0x11)   -- clobber
                elseif phase == 3 then
                    local ok = emu:load_state(1)
                    emu.log('restored=' .. tostring(ok) .. ' v=' .. memory:peek(0x05))
                end
            end)
            ",
        )
        .expect("load");
        for _ in 0..3 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
        }
        assert_eq!(eng.drain_log(), vec!["restored=true v=119"]);
        assert_eq!(nes.peek(0x05), 0x77, "load_state must restore the snapshot");
    }

    /// B3 — `emu:load_state` is GATED like `emu.write`: under a locked session a
    /// restore is a silent no-op (returns `false`, state untouched). A `save` is
    /// always allowed (it is a read-only snapshot).
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn load_state_gated_when_locked() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            phase = 0
            emu.onFrame(function()
                phase = phase + 1
                if phase == 1 then
                    memory:poke(0x06, 0x55)
                    emu:save_state(2)     -- save still works under lock
                elseif phase == 2 then
                    local ok = emu:load_state(2)
                    emu.log('locked_restore=' .. tostring(ok))
                end
            end)
            ",
        )
        .expect("load");
        // Lock writes (netplay / TAS-replay / RA-hardcore analog). `poke` is then
        // dropped too, so RAM never changes; the key assertion is `load_state`
        // returns false and is inert.
        eng.set_writes_locked(true);
        for _ in 0..2 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
        }
        assert_eq!(eng.drain_log(), vec!["locked_restore=false"]);
    }

    /// B4 — `sym:addr(name)` / `sym:name(addr)` resolve against the host-pushed
    /// symbol table (read-only). An unknown lookup returns `nil`.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn symbol_queries_resolve_host_table() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_symbols(&[(0xC000, "main".to_owned()), (0x0010, "player_x".to_owned())]);
        eng.load(
            r"
            emu.onFrame(function()
                emu.log('main=' .. (sym:addr('main') or -1)
                    .. ' name=' .. (sym:name(0x10) or 'nil')
                    .. ' miss=' .. tostring(sym:addr('nope')))
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(
            eng.drain_log(),
            vec![format!("main={} name=player_x miss=nil", 0xC000)]
        );
    }

    /// B4 — `emu:on_breakpoint(addr, fn)` fires observationally from the per-frame
    /// exec-PC log (the boot loop sits at `$C000`), and arms the exec log.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn on_breakpoint_fires_from_the_exec_log() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            hit = false
            emu:on_breakpoint(0xC000, function(pc) hit = true end)
            emu.onFrame(function() if hit then emu.log('bp') end end)
            ",
        )
        .expect("load");
        assert!(eng.needs_exec_log(), "on_breakpoint must arm the exec log");
        nes.set_exec_logging(true);
        let mut saw = false;
        for _ in 0..4 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
            if eng.drain_log().contains(&"bp".to_owned()) {
                saw = true;
                break;
            }
        }
        assert!(saw, "on_breakpoint($C000) should fire from the exec log");
    }

    /// B4 — `emu:pause_at_frame(n)` queues exactly one `Pause` control when the
    /// emulated frame count reaches `n`, then never again.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn pause_at_frame_queues_one_pause() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        // Target a frame a couple of pumps out (the first pump sees frame >= 1).
        let target = nes.frame() + 2;
        eng.load(&format!(
            "emu.onFrame(function() if emu.frame == {target} then emu:pause_at_frame({target}) end end)"
        ))
        .expect("load");
        let mut pauses = 0;
        for _ in 0..5 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
            pauses += eng
                .drain_controls()
                .iter()
                .filter(|c| matches!(c, ControlCmd::Pause))
                .count();
        }
        assert_eq!(pauses, 1, "pause_at_frame must queue exactly one Pause");
    }

    /// B5 — every bundled example script loads + pumps a few frames without a
    /// load or runtime error (so a doc-referenced example never bit-rots against
    /// the API). Embedded at compile time relative to this crate.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn bundled_example_scripts_load_and_run() {
        const EXAMPLES: &[(&str, &str)] = &[
            (
                "memory_scanner.lua",
                include_str!("../../../examples/scripts/memory_scanner.lua"),
            ),
            (
                "tas_frame_analysis.lua",
                include_str!("../../../examples/scripts/tas_frame_analysis.lua"),
            ),
            (
                "game_state_tracker.lua",
                include_str!("../../../examples/scripts/game_state_tracker.lua"),
            ),
            (
                "driving_loop.lua",
                include_str!("../../../examples/scripts/driving_loop.lua"),
            ),
        ];
        for (name, src) in EXAMPLES {
            let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
            let mut eng = ScriptEngine::new().expect("engine");
            eng.set_symbols(&[(0x0010, "player_x".to_owned())]);
            eng.load(src).unwrap_or_else(|e| panic!("{name} load: {e}"));
            for _ in 0..3 {
                nes.run_frame();
                eng.on_frame(&mut nes)
                    .unwrap_or_else(|e| panic!("{name} on_frame: {e}"));
                let _ = eng.drain_controls();
                let _ = eng.drain_draws();
                let _ = eng.drain_log();
            }
        }
    }

    // ----- v1.6.0 Workstream B2: Lua driving primitives (native-only / mlua) --
    // `emu.run(fn)` + `emu.frameadvance()` let a script drive the emulator a
    // frame at a time (the FCEUX / BizHawk model). Driving is hosted on the
    // mlua backend (the dev/TAS surface is native-only; ADR 0012).

    /// A driving coroutine resumes exactly once per `on_frame`, picking up after
    /// the `emu.frameadvance()` it yielded on. So N frames advance the loop N
    /// steps (one log line per frame here).
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn driving_coroutine_resumes_once_per_frame() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.run(function()
                local n = 0
                while true do
                    n = n + 1
                    emu.log('step ' .. n)
                    emu.frameadvance()
                end
            end)
            ",
        )
        .expect("load");
        // Driving needs no onFrame callback registered.
        assert_eq!(eng.frame_callback_count(), 0);
        for _ in 0..3 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
        }
        assert_eq!(eng.drain_log(), vec!["step 1", "step 2", "step 3"]);
    }

    /// A driver that returns (finishes) stops being resumed — no further log
    /// output, and no error.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn driving_coroutine_stops_when_finished() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.run(function()
                emu.log('a')
                emu.frameadvance()
                emu.log('b')
                -- returns here: the driver is done.
            end)
            ",
        )
        .expect("load");
        for _ in 0..4 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
        }
        // Resume 1 logs 'a' then yields; resume 2 logs 'b' then returns; resumes
        // 3 and 4 see no driver. No duplicate / runaway output.
        assert_eq!(eng.drain_log(), vec!["a", "b"]);
    }

    /// A driver's `emu.setInput` is gated identically to `emu.write`: dropped
    /// under a locked session, queued when unlocked.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn driving_set_input_is_gated_when_locked() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_writes_locked(true);
        eng.load(
            r"
            emu.run(function()
                while true do
                    emu.setInput(0, 0x81)
                    emu.frameadvance()
                end
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            eng.drain_controls().is_empty(),
            "driver setInput must be a no-op when writes are locked"
        );

        eng.set_writes_locked(false);
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            eng.drain_controls().contains(&ControlCmd::SetInput {
                port: 0,
                buttons: 0x81
            }),
            "driver setInput must queue a SetInput command when unlocked"
        );
    }

    /// A later `emu.run` replaces an earlier driver (only one drives at a time).
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn driving_run_replaces_previous_driver() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.run(function()
                while true do emu.log('first'); emu.frameadvance() end
            end)
            emu.run(function()
                while true do emu.log('second'); emu.frameadvance() end
            end)
            ",
        )
        .expect("load");
        for _ in 0..2 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
        }
        // Only the second driver runs.
        assert_eq!(eng.drain_log(), vec!["second", "second"]);
    }

    // ===== v1.7.0 "Forge" Workstream B — scriptable TAStudio + Lua parity =====
    // All native-only (mlua), the same carve-out as the dev/TAS surface above.

    /// B1 — the `tastudio.*` query API reads the host-pushed [`TasSnapshot`].
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn tastudio_queries_read_host_snapshot() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_tas_snapshot(TasSnapshot {
            engaged: true,
            recording: true,
            seek_frame: 42,
            selection: Some((10, 20)),
            lag: vec![false, true, false],
            state_frames: vec![0, 60],
            markers: vec![(5, "start".to_owned()), (60, "boss".to_owned())],
            branches: vec![TasBranchInfo {
                frame: 30,
                text: "alt route".to_owned(),
                input: vec![(0x01, 0x00), (0x02, 0x80)],
            }],
            input_len: 100,
        });
        eng.load(
            r"
            emu.onFrame(function()
                local first, last = tastudio:getselection()
                emu.log('eng=' .. tostring(tastudio:engaged())
                    .. ' rec=' .. tostring(tastudio:getrecording())
                    .. ' seek=' .. tastudio:getseekframe()
                    .. ' sel=' .. first .. ',' .. last
                    .. ' lag1=' .. tostring(tastudio:islag(1))
                    .. ' lag9=' .. tostring(tastudio:islag(9))
                    .. ' hs60=' .. tostring(tastudio:hasstate(60))
                    .. ' hs61=' .. tostring(tastudio:hasstate(61))
                    .. ' mk5=' .. tastudio:getmarker(5)
                    .. ' br1=' .. tastudio:getbranchtext(1)
                    .. ' brn=' .. tostring(tastudio:getbranches()[1].frame))
                local p1, p2 = tastudio:getbranchinput(1, 1)
                emu.log('bi=' .. p1 .. ',' .. p2)
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        let log = eng.drain_log();
        assert!(
            log[0].contains("eng=true rec=true seek=42 sel=10,20 lag1=true lag9=nil"),
            "snapshot reads: {log:?}"
        );
        assert!(log[0].contains("hs60=true hs61=false mk5=start br1=alt route brn=30"));
        assert_eq!(log[1], "bi=2,128");
    }

    /// B1 — every `tastudio.*` mutator queues a [`TasCmd`] when unlocked, and is
    /// a silent no-op (NO queued command) under a locked session — gated exactly
    /// like `emu.write`.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn tastudio_mutators_queue_and_gate_like_emu_write() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                tastudio:setrecording(true)
                tastudio:setplayback(100)
                tastudio:setplayback('boss')
                tastudio:setlag(7, true)
                tastudio:setmarker(9, 'here')
                tastudio:removemarker(3)
                tastudio:submitinputchange(0, 0, 0x81)
                tastudio:submitinputchange(1, 1, 0x42)
                tastudio:applyinputchanges()
                tastudio:loadbranch(2)
                tastudio:setbranchtext(2, 'alt')
            end)
            ",
        )
        .expect("load");

        // Unlocked: each call queues its command (the two submits flush as a
        // batch on applyinputchanges).
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        let cmds = eng.drain_tas_commands();
        assert!(cmds.contains(&TasCmd::SetRecording(Some(true))));
        assert!(cmds.contains(&TasCmd::SetPlaybackFrame(100)));
        assert!(cmds.contains(&TasCmd::SetPlaybackMarker("boss".to_owned())));
        assert!(cmds.contains(&TasCmd::SetLag {
            frame: 7,
            lag: true
        }));
        assert!(cmds.contains(&TasCmd::SetMarker {
            frame: 9,
            text: "here".to_owned()
        }));
        assert!(cmds.contains(&TasCmd::RemoveMarker(3)));
        assert!(cmds.contains(&TasCmd::SetInput {
            frame: 0,
            port: 0,
            buttons: 0x81
        }));
        assert!(cmds.contains(&TasCmd::SetInput {
            frame: 1,
            port: 1,
            buttons: 0x42
        }));
        assert!(cmds.contains(&TasCmd::LoadBranch(2)));
        assert!(cmds.contains(&TasCmd::SetBranchText {
            index: 2,
            text: "alt".to_owned()
        }));

        // Locked: NOTHING queues (every mutator is gated like emu.write).
        eng.set_writes_locked(true);
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            eng.drain_tas_commands().is_empty(),
            "all tastudio mutators must be no-ops when locked"
        );
    }

    /// B1 — `submitinputchange` STAGES; nothing reaches the host queue until
    /// `applyinputchanges()` flushes the batch (the `BizHawk` atomic-edit pattern).
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn tastudio_submit_is_atomic_until_apply() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            phase = 0
            emu.onFrame(function()
                phase = phase + 1
                if phase == 1 then
                    tastudio:submitinputchange(5, 0, 0x01)
                    tastudio:submitinputchange(6, 0, 0x02)
                    -- no apply yet
                elseif phase == 2 then
                    tastudio:applyinputchanges()
                end
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            eng.drain_tas_commands().is_empty(),
            "staged edits must not reach the host before apply"
        );
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        let cmds = eng.drain_tas_commands();
        assert_eq!(cmds.len(), 2, "apply flushes both staged edits: {cmds:?}");
    }

    /// B2 — `onqueryitem*` callbacks paint a piano-roll cell; the host queries
    /// via [`ScriptEngine::query_tas_cell`]. Pure overlay (returns decoration).
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn tastudio_cell_query_callbacks_decorate() {
        let mut eng = ScriptEngine::new().expect("engine");
        assert!(!eng.needs_tas_cell_query());
        eng.load(
            r"
            tastudio:onqueryitembg(function(frame, col)
                if frame == 5 then return 0xFF0000FF end
            end)
            tastudio:onqueryitemtext(function(frame, col)
                if frame == 5 and col == 1 then return 'X' end
            end)
            tastudio:onqueryitemicon(function(frame, col)
                if frame == 5 then return 'star' end
            end)
            ",
        )
        .expect("load");
        assert!(eng.needs_tas_cell_query());
        let decor = eng.query_tas_cell(5, 1).expect("query");
        assert_eq!(decor.bg, Some(0xFF00_00FF));
        assert_eq!(decor.text.as_deref(), Some("X"));
        assert_eq!(decor.icon.as_deref(), Some("star"));
        // A different cell gets no decoration.
        let none = eng.query_tas_cell(6, 1).expect("query");
        assert_eq!(none, TasCellDecor::default());
    }

    /// B2 — `clearIconCache()` raises the host-drained flag once.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn tastudio_clear_icon_cache_flag() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load("emu.onFrame(function() tastudio:clearIconCache() end)")
            .expect("load");
        assert!(!eng.take_clear_icon_cache());
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            eng.take_clear_icon_cache(),
            "clearIconCache raises the flag"
        );
        assert!(!eng.take_clear_icon_cache(), "flag is taken (one-shot)");
    }

    /// B2 — `ongreenzoneinvalidated` / `onbranchload` event callbacks fire from
    /// the host entry points with the right argument.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn tastudio_event_callbacks_fire() {
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            gz = -1
            bl = -1
            tastudio:ongreenzoneinvalidated(function(frame) gz = frame end)
            tastudio:onbranchload(function(idx) bl = idx end)
            emu.onFrame(function() emu.log('gz=' .. gz .. ' bl=' .. bl) end)
            ",
        )
        .expect("load");
        eng.fire_greenzone_invalidated(37).expect("gz");
        eng.fire_branch_load(2).expect("bl");
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(eng.drain_log(), vec!["gz=37 bl=2"]);
    }

    /// B3 — `getScreenBuffer` / `getPixel` read the framebuffer; `setScreenBuffer`
    /// paints output and is GATED like `emu.write` (no-op when locked).
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn parity_screen_buffer_get_set_and_gate() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                local buf = emu.getScreenBuffer()
                emu.log('len=' .. #buf)
                emu.log('px=' .. tostring(emu.getPixel(0, 0)))
                emu.log('oob=' .. tostring(emu.getPixel(999, 0)))
                -- paint the whole frame opaque red (0xRRGGBBAA).
                local red = {}
                for i = 1, 256 * 240 do red[i] = 0xFF0000FF end
                emu:setScreenBuffer(red)
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        let log = eng.drain_log();
        assert_eq!(log[0], format!("len={}", 256 * 240));
        assert!(log[1].starts_with("px="));
        assert_eq!(log[2], "oob=nil");
        // The paint landed: top-left pixel is now opaque red.
        assert_eq!(&nes.framebuffer()[0..4], &[0xFF, 0x00, 0x00, 0xFF]);

        // Locked: setScreenBuffer is a no-op. Repaint green, but locked.
        nes.run_frame(); // a fresh frame repaints the framebuffer
        eng.set_writes_locked(true);
        eng.load("emu.onFrame(function() local g = {}; for i=1,256*240 do g[i]=0x00FF00FF end; emu:setScreenBuffer(g) end)").expect("load2");
        let before = nes.framebuffer()[0..4].to_vec();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(
            &nes.framebuffer()[0..4],
            before.as_slice(),
            "setScreenBuffer must be a no-op when locked"
        );
    }

    /// B3 — `getState` returns a structured CPU/system map; `setState` writes
    /// back the register file and is GATED like `emu.write`.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn parity_get_set_state_and_gate() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                local s = emu:getState()
                emu.log('has_pc=' .. tostring(s.pc ~= nil)
                    .. ' has_a=' .. tostring(s.a ~= nil)
                    .. ' region=' .. s.region
                    .. ' fc=' .. tostring(s.frameCount ~= nil))
                emu:setState({ a = 0x55, x = 0x66 })
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        let log = eng.drain_log();
        assert!(
            log[0].contains("has_pc=true has_a=true region=NTSC fc=true"),
            "getState map: {log:?}"
        );
        assert_eq!(nes.cpu().a, 0x55, "setState wrote A");
        assert_eq!(nes.cpu().x, 0x66, "setState wrote X");

        // Locked: setState is a no-op.
        eng.set_writes_locked(true);
        eng.load("emu.onFrame(function() emu:setState({ a = 0x11 }) end)")
            .expect("load2");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(nes.cpu().a, 0x55, "setState must be a no-op when locked");
    }

    /// B3 — the value-modifying write callback intercepts a RAM write and pokes a
    /// replacement byte; the poke is GATED like `emu.write`.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn parity_value_modifying_write_callback_and_gate() {
        // NROM whose boot loop writes $33 to $0000 (zero-page RAM) each frame:
        // `LDA #$33; STA $0000; JMP $C000`.
        fn synth_ram_writer() -> Vec<u8> {
            let mut bytes = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
            bytes.resize(16, 0);
            let mut prg = vec![0u8; 16 * 1024];
            prg[0..8].copy_from_slice(&[0xA9, 0x33, 0x8D, 0x00, 0x00, 0x4C, 0x00, 0xC0]);
            let len = prg.len();
            prg[len - 4] = 0x00;
            prg[len - 3] = 0xC0;
            bytes.extend_from_slice(&prg);
            bytes.resize(16 + 16 * 1024 + 8 * 1024, 0);
            bytes
        }
        let mut nes = Nes::from_rom(&synth_ram_writer()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.addMemoryCallback(function(addr, value)
                -- Intercept the write to $0000, force the stored byte to $99.
                if value == 0x33 then return 0x99 end
            end, 'write', 0x0000)
            ",
        )
        .expect("load");
        assert!(
            eng.needs_access_log(),
            "modify callback arms the access log"
        );
        nes.set_access_logging(true);
        // Run a few frames so the CPU clears reset warmup and the boot loop's
        // `STA $0000` actually executes (a write into the access log).
        let mut saw = false;
        for _ in 0..4 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
            if nes.peek(0x0000) == 0x99 {
                saw = true;
                break;
            }
        }
        assert!(saw, "value-modify poked the replacement");

        // Locked: the modify poke is dropped; the original write stands.
        eng.set_writes_locked(true);
        nes.run_frame(); // writes $33 again
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(
            nes.peek(0x0000),
            0x33,
            "value-modify poke must be dropped when locked"
        );
    }

    /// B3 — the full `addEventCallback` enum: `startFrame`/`endFrame`/
    /// `inputPolled` fire from the per-frame pump; an unknown type errors.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn parity_event_callbacks_fire_and_reject_unknown() {
        // A polling ROM so `inputPolled` fires (`LDA $4016; JMP $C000`).
        fn synth_polling() -> Vec<u8> {
            let mut bytes = vec![b'N', b'E', b'S', 0x1A, 1, 1, 0, 0];
            bytes.resize(16, 0);
            let mut prg = vec![0u8; 16 * 1024];
            prg[0..6].copy_from_slice(&[0xAD, 0x16, 0x40, 0x4C, 0x00, 0xC0]);
            let len = prg.len();
            prg[len - 4] = 0x00;
            prg[len - 3] = 0xC0;
            bytes.extend_from_slice(&prg);
            bytes.resize(16 + 16 * 1024 + 8 * 1024, 0);
            bytes
        }
        let mut nes = Nes::from_rom(&synth_polling()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            sf = 0; ef = 0; ip = 0
            emu.addEventCallback(function() sf = sf + 1 end, 'startFrame')
            emu.addEventCallback(function() ef = ef + 1 end, 'endFrame')
            emu.addEventCallback(function() ip = ip + 1 end, 'inputPolled')
            emu.onFrame(function() emu.log('sf=' .. sf .. ' ef=' .. ef .. ' ip=' .. ip) end)
            ",
        )
        .expect("load");
        let mut polled = false;
        for _ in 0..6 {
            nes.run_frame();
            eng.on_frame(&mut nes).expect("on_frame");
            let log = eng.drain_log();
            if log.iter().any(|l| !l.ends_with("ip=0")) {
                polled = true;
            }
        }
        assert!(polled, "inputPolled should fire on a polling ROM");

        // An unknown event type is a load-time error.
        let mut eng2 = ScriptEngine::new().expect("engine");
        assert!(
            eng2.load("emu.addEventCallback(function() end, 'bogus')")
                .is_err(),
            "unknown addEventCallback type must error"
        );
    }

    /// B3 — `takeScreenshot()` queues a `Screenshot` control (NOT gated — a
    /// screenshot is a read-only side effect), and `getScriptDataFolder()`
    /// returns the host-pushed path or nil.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn parity_screenshot_and_script_data_folder() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_script_data_folder(Some("/tmp/rustynes/scripts".to_owned()));
        eng.load(
            r"
            emu.onFrame(function()
                emu.takeScreenshot()
                emu.log('dir=' .. tostring(emu.getScriptDataFolder()))
            end)
            ",
        )
        .expect("load");
        // Even locked, a screenshot is allowed (read-only side effect).
        eng.set_writes_locked(true);
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            eng.drain_controls().contains(&ControlCmd::Screenshot),
            "takeScreenshot queues a Screenshot control even when locked"
        );
        assert_eq!(eng.drain_log(), vec!["dir=/tmp/rustynes/scripts"]);
    }

    /// `emu.frameadvance()` outside a coroutine raises (Lua's "yield from outside
    /// a coroutine"), surfaced to the host as a script error rather than a panic.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn frameadvance_outside_coroutine_errors() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        // Calling frameadvance from an onFrame callback (not a driving coroutine)
        // must surface an error, not crash the host pump.
        eng.load("emu.onFrame(function() emu.frameadvance() end)")
            .expect("load");
        nes.run_frame();
        assert!(
            eng.on_frame(&mut nes).is_err(),
            "frameadvance outside a coroutine must surface a script error"
        );
    }

    // ====================================================================
    // v1.7.0 "Forge" Workstream E — host IPC / automation.
    // ====================================================================

    /// E2 — observational `client.*` verbs queue a `ClientCmd` the host drains;
    /// they are NOT write-gated (presentation-only, never perturb the core).
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn client_observational_verbs_queue() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                client.screenshot()
                client.setwindowsize(3)
                client.speedmode(200)
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        let cmds = eng.drain_clients();
        assert_eq!(
            cmds,
            vec![
                ClientCmd::Screenshot,
                ClientCmd::SetWindowSize(3),
                ClientCmd::SpeedMode(200),
            ]
        );
    }

    /// E2 — the state-changing `client.*` verbs (`reboot_core` / `addcheat` /
    /// `removecheat`) are gated EXACTLY like `emu.write`: dropped at the source
    /// when the host has locked writes, while the observational verbs still run.
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn client_mutators_gated_when_locked() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_writes_locked(true);
        eng.load(
            r"
            emu.onFrame(function()
                client.reboot_core()
                client.addcheat('SXIOPO')
                client.removecheat('SXIOPO')
                client.screenshot()
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        // Only the observational `screenshot` survives the lock.
        assert_eq!(
            eng.drain_clients(),
            vec![ClientCmd::Screenshot],
            "client mutators must be dropped when writes are locked"
        );
    }

    /// E3 — the `userdata.*` KV store round-trips through Lua, and the host can
    /// snapshot/restore it (persistence across runs / into save-states).
    #[cfg(not(feature = "script-wasm"))]
    #[test]
    fn userdata_kv_round_trips_and_snapshots() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            emu.onFrame(function()
                userdata.set('seed', '42')
                userdata.set('best', 'world1-1')
                emu.log('has=' .. tostring(userdata.containskey('seed')))
                emu.log('seed=' .. userdata.get('seed'))
                userdata.remove('best')
                emu.log('best=' .. tostring(userdata.get('best')))
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(eng.drain_log(), vec!["has=true", "seed=42", "best=nil"]);
        // The host snapshot mirrors the live store (sorted, `best` removed).
        assert_eq!(
            eng.userdata_snapshot(),
            vec![("seed".to_string(), "42".to_string())]
        );

        // A fresh engine restored from a snapshot sees the persisted values.
        let mut eng2 = ScriptEngine::new().expect("engine");
        eng2.userdata_restore(&[("seed".to_string(), "99".to_string())]);
        eng2.load("emu.onFrame(function() emu.log('r=' .. userdata.get('seed')) end)")
            .expect("load");
        nes.run_frame();
        eng2.on_frame(&mut nes).expect("on_frame");
        assert_eq!(eng2.drain_log(), vec!["r=99"]);
    }

    /// E1 (`script-ipc`) — a `comm.*` request queues a host-owned `CommCmd`, and
    /// the host's fulfilled `CommResult` is surfaced back via `comm.receive()`.
    /// The script only ever sees marshalled plain values — never a socket.
    #[cfg(all(feature = "script-ipc", not(feature = "script-wasm")))]
    #[test]
    fn comm_request_queues_and_result_polls_back() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.load(
            r"
            req = nil
            emu.onFrame(function()
                if req == nil then
                    req = comm.httpGet('http://localhost/state')
                else
                    local r = comm.receive()
                    if r ~= nil then emu.log('got ' .. r.status .. ' ' .. r.body) end
                end
            end)
            ",
        )
        .expect("load");
        // Frame 1: the script issues the request; the host owns the connection.
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        let out = eng.drain_comm();
        let id = match out.as_slice() {
            [CommCmd::HttpGet { id, url }] => {
                assert_eq!(url, "http://localhost/state");
                *id
            }
            other => panic!("expected one HttpGet, got {other:?}"),
        };
        // The host fulfils it off the emu lock and injects the result.
        eng.push_comm_result(CommResult::Http {
            id,
            status: 200,
            body: "ok".to_string(),
        });
        // Frame 2: the script polls the marshalled result.
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert_eq!(eng.drain_log(), vec!["got 200 ok"]);
    }

    /// E1 — `comm.*` is gated EXACTLY like `emu.write`: under a locked session
    /// (netplay / TAS replay / RA-hardcore) every IPC verb is a no-op, so no
    /// `CommCmd` is queued and the host never opens a connection.
    #[cfg(all(feature = "script-ipc", not(feature = "script-wasm")))]
    #[test]
    fn comm_is_a_no_op_when_locked() {
        let mut nes = Nes::from_rom(&synth_rom()).expect("rom");
        let mut eng = ScriptEngine::new().expect("engine");
        eng.set_writes_locked(true);
        eng.load(
            r"
            emu.onFrame(function()
                comm.socketServerSend('x')
                comm.httpGet('http://localhost/')
                comm.ws_open('ws://localhost/')
                comm.ws_send('hi')
                comm.mmfWrite('m', 'data')
                comm.mmfRead('m', 4)
            end)
            ",
        )
        .expect("load");
        nes.run_frame();
        eng.on_frame(&mut nes).expect("on_frame");
        assert!(
            eng.drain_comm().is_empty(),
            "comm.* must queue nothing when writes are locked"
        );
    }

    /// E1 — the host-mediated design preserves the sandbox: even with the `comm`
    /// table present, the script still cannot reach any RAW networking / OS
    /// surface (`io` / `os` / `package` / `require` / loaders stay stripped). The
    /// only IPC path is the host-owned `comm.*` marshalling.
    #[cfg(all(feature = "script-ipc", not(feature = "script-wasm")))]
    #[test]
    fn comm_does_not_open_the_raw_net_sandbox() {
        let mut eng = ScriptEngine::new().expect("engine");
        for probe in [
            "return io.open('/etc/passwd')",
            "return os.execute('curl http://x')",
            "return require('socket')",
            "return package.loadlib('libc.so', 'connect')",
            "return load('return io')",
        ] {
            assert!(
                eng.load(probe).is_err(),
                "comm must not open a raw-net escape: {probe}"
            );
        }
    }
}
