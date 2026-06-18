#![allow(
    clippy::cast_lossless,
    clippy::cast_possible_truncation,
    clippy::cast_sign_loss,
    clippy::missing_const_for_fn,
    clippy::too_many_lines
)]
//! Watch / conditional-breakpoint / watchpoint panel (v1.6.0 "Studio"
//! Workstream C — C1 keystone + C4 free riders).
//!
//! Mesen2-class debugger depth, built entirely on the frontend's
//! [`super::expr`] evaluator + the core's existing `debug-hooks` per-frame
//! observational logs (`Nes::exec_log` / `Nes::accesses`). It hosts four
//! related tools:
//!
//! - **C1 conditional breakpoints** — an exec PC (or address range) + an
//!   optional [`Expr`] condition. When the program executes an address in range
//!   AND the condition is true, the hit is logged (break-on-condition).
//! - **C1 read/write/exec watchpoints** — an address range + an access class
//!   (read / write / exec) + an optional condition; logs every matching access.
//! - **C4 watch window** — a list of expressions evaluated once per frame and
//!   displayed (end-of-frame machine state).
//! - **C4 conditional trace** — a format string + a condition expression that
//!   filters which executed instructions get logged.
//!
//! # Observational contract (ADR 0010)
//!
//! All of this is **observational**: it replays the just-finished frame's exec /
//! access logs AFTER the frame, exactly like the Lua `onExec` / `onRead` /
//! `onWrite` hooks. It NEVER intercepts mid-instruction or mutates deterministic
//! state, so determinism / `AccuracyCoin` are unaffected and the feature-off
//! core build is byte-identical.
//!
//! One consequence of replay (shared with the Lua hooks): the `value` /
//! `address` / `isRead` / `isWrite` / `isExec` tokens are per-access accurate,
//! but the CPU-register / PPU (`a x y s p pc scanline cycle frame`) and `[addr]`
//! memory tokens reflect the machine's **end-of-frame** state, not the state at
//! the exact cycle of the logged access. This is documented in the panel UI.

use std::cell::RefCell;

use egui::Color32;
use rustynes_core::Nes;

use super::expr::{AccessContext, AccessKind, EvalContext, Expr};
use crate::symbols::SymbolMap;

/// Maximum hit-log rows retained (oldest dropped). Bounds memory on a noisy
/// watchpoint.
const HIT_LOG_CAP: usize = 512;

/// Maximum conditional-trace rows retained (oldest dropped).
const TRACE_CAP: usize = 1024;

/// The access class a watchpoint fires on.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum WatchKind {
    /// A CPU read in range.
    Read,
    /// A CPU write in range.
    Write,
    /// An instruction fetch (exec) in range.
    Exec,
}

impl WatchKind {
    const fn label(self) -> &'static str {
        match self {
            Self::Read => "R",
            Self::Write => "W",
            Self::Exec => "X",
        }
    }

    const fn access_kind(self) -> AccessKind {
        match self {
            Self::Read => AccessKind::Read,
            Self::Write => AccessKind::Write,
            Self::Exec => AccessKind::Exec,
        }
    }
}

/// A conditional breakpoint: an exec-PC range + an optional condition.
struct CondBreakpoint {
    enabled: bool,
    /// Inclusive low address.
    lo: u16,
    /// Inclusive high address (== `lo` for a single PC).
    hi: u16,
    /// The raw condition source (empty = unconditional).
    cond_src: String,
    /// The compiled condition, or `None` when `cond_src` is empty / invalid.
    cond: Option<Expr>,
    /// `true` when `cond_src` failed to parse (shown red in the UI).
    cond_error: bool,
    /// Cumulative hit count since added / last cleared.
    hits: u64,
}

/// A read/write/exec watchpoint: an address range + access class + optional
/// condition.
struct Watchpoint {
    enabled: bool,
    kind: WatchKind,
    lo: u16,
    hi: u16,
    cond_src: String,
    cond: Option<Expr>,
    cond_error: bool,
    hits: u64,
}

/// One watch-window row: a label-less expression evaluated each frame.
struct WatchRow {
    src: String,
    expr: Option<Expr>,
    error: bool,
}

/// A logged hit (from a conditional breakpoint or a watchpoint).
#[derive(Clone)]
struct HitRec {
    /// `"BP"`, `"R"`, `"W"`, or `"X"`.
    tag: &'static str,
    /// The frame the hit was observed in.
    frame: u64,
    /// The PC (for a breakpoint) or accessed address (for a watchpoint).
    addr: u16,
    /// The accessed byte (watchpoints only; `0` for an exec breakpoint).
    value: u8,
    /// `true` for a watchpoint hit that carried a value.
    has_value: bool,
}

/// Panel + replay state for the Watch / breakpoints / watchpoints tools.
pub struct WatchPanelState {
    /// Master arm switch — when off, the pump does nothing (logs stay disarmed).
    armed: bool,
    breakpoints: Vec<CondBreakpoint>,
    watchpoints: Vec<Watchpoint>,
    watch_rows: Vec<WatchRow>,
    /// Conditional trace: format string + condition.
    trace_enabled: bool,
    trace_format_src: String,
    trace_cond_src: String,
    trace_cond: Option<Expr>,
    trace_cond_error: bool,
    /// The most-recent conditional-trace rows (already formatted).
    trace_rows: std::collections::VecDeque<String>,
    /// The most-recent breakpoint / watchpoint hits.
    hits: std::collections::VecDeque<HitRec>,
    // --- UI input buffers ---
    bp_lo_text: String,
    bp_hi_text: String,
    bp_cond_text: String,
    wp_kind: WatchKind,
    wp_lo_text: String,
    wp_hi_text: String,
    wp_cond_text: String,
    watch_add_text: String,
}

impl Default for WatchPanelState {
    fn default() -> Self {
        Self {
            armed: true,
            breakpoints: Vec::new(),
            watchpoints: Vec::new(),
            watch_rows: Vec::new(),
            trace_enabled: false,
            trace_format_src: "{pc}: A={a} X={x} Y={y}".to_string(),
            trace_cond_src: String::new(),
            trace_cond: None,
            trace_cond_error: false,
            trace_rows: std::collections::VecDeque::new(),
            hits: std::collections::VecDeque::new(),
            bp_lo_text: String::new(),
            bp_hi_text: String::new(),
            bp_cond_text: String::new(),
            wp_kind: WatchKind::Write,
            wp_lo_text: String::new(),
            wp_hi_text: String::new(),
            wp_cond_text: String::new(),
            watch_add_text: String::new(),
        }
    }
}

/// Snapshot of the end-of-frame machine state + the access in flight, fed to the
/// expression evaluator during replay. CPU/PPU fields are end-of-frame (the
/// observational-replay limitation); `access` is the per-access record.
///
/// `nes` is held in a [`RefCell`] because the evaluator's [`EvalContext::peek`]
/// is `&self`, while the core's only side-effect-free CPU peek
/// ([`Nes::cpu_bus_peek`]) takes `&mut self` (it borrows the mapper, which
/// reads `&mut` — but, per its docs, advances no emulator-visible state, so the
/// peek stays observational and determinism is intact).
struct ReplayCtx<'a> {
    nes: RefCell<&'a mut Nes>,
    a: u8,
    x: u8,
    y: u8,
    s: u8,
    p: u8,
    pc: u16,
    scanline: i16,
    dot: u16,
    frame: u64,
    access: AccessContext,
}

impl EvalContext for ReplayCtx<'_> {
    fn a(&self) -> u8 {
        self.a
    }
    fn x(&self) -> u8 {
        self.x
    }
    fn y(&self) -> u8 {
        self.y
    }
    fn s(&self) -> u8 {
        self.s
    }
    fn p(&self) -> u8 {
        self.p
    }
    fn pc(&self) -> u16 {
        self.pc
    }
    fn scanline(&self) -> i16 {
        self.scanline
    }
    fn dot(&self) -> u16 {
        self.dot
    }
    fn frame(&self) -> u64 {
        self.frame
    }
    fn peek(&self, addr: u16) -> u8 {
        self.nes.borrow_mut().cpu_bus_peek(addr)
    }
    fn access(&self) -> AccessContext {
        self.access
    }
}

impl WatchPanelState {
    /// `true` when any breakpoint or watchpoint exists (used to decide whether
    /// the per-frame exec/access logs must be armed).
    #[must_use]
    pub fn needs_exec_log(&self) -> bool {
        self.armed
            && (self.breakpoints.iter().any(|b| b.enabled)
                || self
                    .watchpoints
                    .iter()
                    .any(|w| w.enabled && w.kind == WatchKind::Exec)
                || self.trace_enabled)
    }

    /// `true` when any read/write watchpoint exists (needs the bus-access log).
    #[must_use]
    pub fn needs_access_log(&self) -> bool {
        self.armed
            && self
                .watchpoints
                .iter()
                .any(|w| w.enabled && matches!(w.kind, WatchKind::Read | WatchKind::Write))
    }

    /// Per-frame pump (called from `App` after a frame is produced, under the
    /// emu lock). Arms the logs the active tools need, then replays the
    /// just-finished frame's exec / access logs to evaluate breakpoints,
    /// watchpoints, the watch window, and conditional trace.
    ///
    /// Purely observational: it only *reads* `nes`, exactly like the Lua
    /// `onExec` / `onRead` / `onWrite` replay (ADR 0010).
    pub fn pump(&mut self, nes: &mut Nes) {
        // Arm / disarm the core's per-frame logs for the NEXT frame based on
        // what the active tools need (mirrors the scripting engine's policy).
        let want_exec = self.needs_exec_log();
        let want_access = self.needs_access_log();
        nes.set_exec_logging(want_exec || nes.exec_logging());
        nes.set_access_logging(want_access);

        if !self.armed {
            return;
        }

        // Snapshot end-of-frame machine state + the frame's logs once (owned),
        // so the single `ReplayCtx` can hold the `&mut Nes` for the duration.
        let cpu = nes.cpu_snapshot();
        let ppu = nes.ppu_snapshot();
        let frame = ppu.frame;
        let exec_pcs: Vec<u16> = if want_exec {
            nes.exec_log().to_vec()
        } else {
            Vec::new()
        };
        let accesses: Vec<(bool, u16, u8)> = if want_access {
            nes.accesses()
                .iter()
                .map(|acc| (acc.write, acc.addr, acc.value))
                .collect()
        } else {
            Vec::new()
        };

        // One context for the whole replay; only `pc` + `access` change between
        // records. `self.breakpoints` / `self.watchpoints` (on `self`) and
        // `ctx` (which borrows `nes`) are disjoint, so the borrow checker lets
        // us evaluate against `&ctx` while mutating the lists' hit counters.
        let mut ctx = ReplayCtx {
            nes: RefCell::new(nes),
            a: cpu.a,
            x: cpu.x,
            y: cpu.y,
            s: cpu.s,
            p: cpu.p,
            pc: cpu.pc,
            scanline: ppu.scanline,
            dot: ppu.dot,
            frame,
            access: AccessContext::default(),
        };

        // --- C1: exec-driven (breakpoints + exec watchpoints) + C4 trace ---
        for &pc in &exec_pcs {
            ctx.pc = pc;
            ctx.access = AccessContext {
                value: 0,
                address: pc,
                kind: Some(AccessKind::Exec),
            };
            // Conditional breakpoints.
            for bp in &mut self.breakpoints {
                if !bp.enabled || pc < bp.lo || pc > bp.hi {
                    continue;
                }
                if bp.cond.as_ref().is_none_or(|e| e.eval_bool(&ctx)) {
                    bp.hits += 1;
                    push_hit(
                        &mut self.hits,
                        HitRec {
                            tag: "BP",
                            frame,
                            addr: pc,
                            value: 0,
                            has_value: false,
                        },
                    );
                }
            }
            // Exec watchpoints.
            for wp in &mut self.watchpoints {
                if !wp.enabled || wp.kind != WatchKind::Exec || pc < wp.lo || pc > wp.hi {
                    continue;
                }
                if wp.cond.as_ref().is_none_or(|e| e.eval_bool(&ctx)) {
                    wp.hits += 1;
                    push_hit(
                        &mut self.hits,
                        HitRec {
                            tag: WatchKind::Exec.label(),
                            frame,
                            addr: pc,
                            value: 0,
                            has_value: false,
                        },
                    );
                }
            }
            // C4 conditional trace.
            if self.trace_enabled && self.trace_cond.as_ref().is_none_or(|e| e.eval_bool(&ctx)) {
                let row = format_trace(&self.trace_format_src, &ctx);
                push_trace(&mut self.trace_rows, row);
            }
        }

        // --- C1: read/write watchpoints (bus-access log) ---
        // Restore `pc` to the end-of-frame value for the access pass (an access
        // expression that reads `pc` should see the machine state, not the last
        // exec PC).
        ctx.pc = cpu.pc;
        for (is_write, addr, value) in accesses {
            let want_kind = if is_write {
                WatchKind::Write
            } else {
                WatchKind::Read
            };
            ctx.access = AccessContext {
                value,
                address: addr,
                kind: Some(want_kind.access_kind()),
            };
            for wp in &mut self.watchpoints {
                if !wp.enabled || wp.kind != want_kind || addr < wp.lo || addr > wp.hi {
                    continue;
                }
                if wp.cond.as_ref().is_none_or(|e| e.eval_bool(&ctx)) {
                    wp.hits += 1;
                    push_hit(
                        &mut self.hits,
                        HitRec {
                            tag: want_kind.label(),
                            frame,
                            addr,
                            value,
                            has_value: true,
                        },
                    );
                }
            }
        }
    }

    /// Evaluate the watch-window rows against the current (end-of-frame)
    /// machine state, returning `(source, value-string)` pairs for display.
    fn eval_watch_rows(&self, nes: &mut Nes) -> Vec<(String, String, bool)> {
        let cpu = nes.cpu_snapshot();
        let ppu = nes.ppu_snapshot();
        let ctx = ReplayCtx {
            nes: RefCell::new(nes),
            a: cpu.a,
            x: cpu.x,
            y: cpu.y,
            s: cpu.s,
            p: cpu.p,
            pc: cpu.pc,
            scanline: ppu.scanline,
            dot: ppu.dot,
            frame: ppu.frame,
            access: AccessContext::default(),
        };
        self.watch_rows
            .iter()
            .map(|row| {
                let display = if row.error {
                    ("parse error".to_string(), true)
                } else if let Some(e) = &row.expr {
                    let v = e.eval(&ctx);
                    (format!("{v} (${:X})", v & 0xFFFF_FFFF), false)
                } else {
                    (String::new(), false)
                };
                (row.src.clone(), display.0, display.1)
            })
            .collect()
    }
}

/// Render the Watch panel. `symbols` annotates breakpoint / hit addresses with
/// loaded labels.
pub fn show(
    ctx: &egui::Context,
    open: &mut bool,
    state: &mut WatchPanelState,
    nes: &mut Nes,
    symbols: &SymbolMap,
) {
    // Pre-compute the watch-row values before borrowing `state` mutably for the
    // UI (the eval needs `&mut Nes` + `&state`).
    let watch_values = state.eval_watch_rows(nes);

    egui::Window::new("Watch / Breakpoints")
        .open(open)
        .default_size([460.0, 520.0])
        .resizable(true)
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.checkbox(&mut state.armed, "Armed");
                ui.weak("(observational — replays the frame's exec/access logs)");
            });
            ui.separator();

            // --- Conditional breakpoints (C1) ---
            egui::CollapsingHeader::new("Conditional breakpoints")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.label("addr:");
                        ui.add(
                            egui::TextEdit::singleline(&mut state.bp_lo_text)
                                .desired_width(56.0)
                                .hint_text("$8000"),
                        );
                        ui.label("..");
                        ui.add(
                            egui::TextEdit::singleline(&mut state.bp_hi_text)
                                .desired_width(56.0)
                                .hint_text("(opt)"),
                        );
                        ui.label("if:");
                        ui.add(
                            egui::TextEdit::singleline(&mut state.bp_cond_text)
                                .desired_width(140.0)
                                .hint_text("a == 0 (opt)"),
                        );
                        if ui.button("Add").clicked() {
                            add_breakpoint(state);
                        }
                    });
                    let mut remove = None;
                    for (i, bp) in state.breakpoints.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut bp.enabled, "");
                            let range = if bp.lo == bp.hi {
                                format!("${:04X}", bp.lo)
                            } else {
                                format!("${:04X}..${:04X}", bp.lo, bp.hi)
                            };
                            ui.monospace(range);
                            if let Some(label) = symbols.label(bp.lo) {
                                ui.colored_label(Color32::from_rgb(0x90, 0xC0, 0xF0), label);
                            }
                            if !bp.cond_src.is_empty() {
                                let col = if bp.cond_error {
                                    Color32::from_rgb(0xE0, 0x50, 0x50)
                                } else {
                                    Color32::from_rgb(0xC0, 0xC0, 0x60)
                                };
                                ui.colored_label(col, format!("if {}", bp.cond_src));
                            }
                            ui.weak(format!("hits={}", bp.hits));
                            if ui.small_button("x").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        state.breakpoints.remove(i);
                    }
                });

            // --- Read/write/exec watchpoints (C1) ---
            egui::CollapsingHeader::new("Watchpoints (R/W/X)")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        egui::ComboBox::from_id_salt("wp_kind")
                            .selected_text(match state.wp_kind {
                                WatchKind::Read => "Read",
                                WatchKind::Write => "Write",
                                WatchKind::Exec => "Exec",
                            })
                            .show_ui(ui, |ui| {
                                ui.selectable_value(&mut state.wp_kind, WatchKind::Read, "Read");
                                ui.selectable_value(&mut state.wp_kind, WatchKind::Write, "Write");
                                ui.selectable_value(&mut state.wp_kind, WatchKind::Exec, "Exec");
                            });
                        ui.add(
                            egui::TextEdit::singleline(&mut state.wp_lo_text)
                                .desired_width(56.0)
                                .hint_text("$0300"),
                        );
                        ui.label("..");
                        ui.add(
                            egui::TextEdit::singleline(&mut state.wp_hi_text)
                                .desired_width(56.0)
                                .hint_text("(opt)"),
                        );
                        ui.add(
                            egui::TextEdit::singleline(&mut state.wp_cond_text)
                                .desired_width(120.0)
                                .hint_text("value!=0 (opt)"),
                        );
                        if ui.button("Add").clicked() {
                            add_watchpoint(state);
                        }
                    });
                    let mut remove = None;
                    for (i, wp) in state.watchpoints.iter_mut().enumerate() {
                        ui.horizontal(|ui| {
                            ui.checkbox(&mut wp.enabled, "");
                            ui.colored_label(Color32::from_rgb(0x80, 0xD0, 0xF0), wp.kind.label());
                            let range = if wp.lo == wp.hi {
                                format!("${:04X}", wp.lo)
                            } else {
                                format!("${:04X}..${:04X}", wp.lo, wp.hi)
                            };
                            ui.monospace(range);
                            if !wp.cond_src.is_empty() {
                                let col = if wp.cond_error {
                                    Color32::from_rgb(0xE0, 0x50, 0x50)
                                } else {
                                    Color32::from_rgb(0xC0, 0xC0, 0x60)
                                };
                                ui.colored_label(col, format!("if {}", wp.cond_src));
                            }
                            ui.weak(format!("hits={}", wp.hits));
                            if ui.small_button("x").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        state.watchpoints.remove(i);
                    }
                });

            // --- Watch window (C4) ---
            egui::CollapsingHeader::new("Watch window")
                .default_open(true)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.add(
                            egui::TextEdit::singleline(&mut state.watch_add_text)
                                .desired_width(220.0)
                                .hint_text("{$00} | [$0300] | a"),
                        );
                        if ui.button("Add").clicked() {
                            add_watch_row(state);
                        }
                    });
                    let mut remove = None;
                    for (i, (src, val, err)) in watch_values.iter().enumerate() {
                        ui.horizontal(|ui| {
                            ui.monospace(src);
                            ui.label("=");
                            if *err {
                                ui.colored_label(Color32::from_rgb(0xE0, 0x50, 0x50), val);
                            } else {
                                ui.monospace(val);
                            }
                            if ui.small_button("x").clicked() {
                                remove = Some(i);
                            }
                        });
                    }
                    if let Some(i) = remove {
                        state.watch_rows.remove(i);
                    }
                });

            // --- Conditional trace (C4) ---
            egui::CollapsingHeader::new("Conditional trace")
                .default_open(false)
                .show(ui, |ui| {
                    ui.horizontal(|ui| {
                        ui.checkbox(&mut state.trace_enabled, "Record");
                        if ui.button("Clear").clicked() {
                            state.trace_rows.clear();
                        }
                    });
                    ui.horizontal(|ui| {
                        ui.label("format:");
                        ui.add(
                            egui::TextEdit::singleline(&mut state.trace_format_src)
                                .desired_width(260.0)
                                .hint_text("{pc}: A={a}"),
                        );
                    });
                    ui.horizontal(|ui| {
                        ui.label("when:");
                        let resp = ui.add(
                            egui::TextEdit::singleline(&mut state.trace_cond_src)
                                .desired_width(220.0)
                                .hint_text("(opt) pc >= $8000"),
                        );
                        if resp.changed() {
                            recompile_trace_cond(state);
                        }
                    });
                    if state.trace_cond_error {
                        ui.colored_label(
                            Color32::from_rgb(0xE0, 0x50, 0x50),
                            "condition parse error",
                        );
                    }
                    ui.weak(
                        "Tokens: {a}{x}{y}{s}{p}{pc}{scanline}{cycle}{frame}, \
                         {[addr]}, {{addr}}.",
                    );
                    egui::ScrollArea::vertical()
                        .id_salt("trace_rows")
                        .max_height(120.0)
                        .auto_shrink([false, false])
                        .stick_to_bottom(true)
                        .show(ui, |ui| {
                            for r in &state.trace_rows {
                                ui.monospace(r);
                            }
                        });
                });

            ui.separator();

            // --- Hit log ---
            ui.horizontal(|ui| {
                ui.label(egui::RichText::new("Hits").strong());
                if ui.button("Clear").clicked() {
                    state.hits.clear();
                }
            });
            ui.weak(
                "Per-access tokens (value/address/isRead/isWrite/isExec) are \
                 exact; register/PPU/[addr] tokens reflect end-of-frame state \
                 (observational replay).",
            );
            egui::ScrollArea::vertical()
                .id_salt("hit_log")
                .auto_shrink([false, false])
                .stick_to_bottom(true)
                .show(ui, |ui| {
                    if state.hits.is_empty() {
                        ui.weak("(no hits — add a breakpoint/watchpoint and run)");
                    }
                    for h in &state.hits {
                        let label = symbols
                            .label(h.addr)
                            .map_or_else(String::new, |l| format!("  <{l}>"));
                        let line = if h.has_value {
                            format!(
                                "f{:<6} [{}] ${:04X} = ${:02X}{}",
                                h.frame, h.tag, h.addr, h.value, label
                            )
                        } else {
                            format!("f{:<6} [{}] ${:04X}{}", h.frame, h.tag, h.addr, label)
                        };
                        ui.monospace(line);
                    }
                });
        });
}

fn add_breakpoint(state: &mut WatchPanelState) {
    let Some(lo) = parse_hex16(&state.bp_lo_text) else {
        return;
    };
    let hi = parse_hex16(&state.bp_hi_text).unwrap_or(lo);
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    let (cond, cond_error) = compile_opt(&state.bp_cond_text);
    state.breakpoints.push(CondBreakpoint {
        enabled: true,
        lo,
        hi,
        cond_src: state.bp_cond_text.trim().to_string(),
        cond,
        cond_error,
        hits: 0,
    });
    state.bp_lo_text.clear();
    state.bp_hi_text.clear();
    state.bp_cond_text.clear();
}

fn add_watchpoint(state: &mut WatchPanelState) {
    let Some(lo) = parse_hex16(&state.wp_lo_text) else {
        return;
    };
    let hi = parse_hex16(&state.wp_hi_text).unwrap_or(lo);
    let (lo, hi) = (lo.min(hi), lo.max(hi));
    let (cond, cond_error) = compile_opt(&state.wp_cond_text);
    state.watchpoints.push(Watchpoint {
        enabled: true,
        kind: state.wp_kind,
        lo,
        hi,
        cond_src: state.wp_cond_text.trim().to_string(),
        cond,
        cond_error,
        hits: 0,
    });
    state.wp_lo_text.clear();
    state.wp_hi_text.clear();
    state.wp_cond_text.clear();
}

fn add_watch_row(state: &mut WatchPanelState) {
    let src = state.watch_add_text.trim();
    if src.is_empty() {
        return;
    }
    let expr = Expr::parse(src).ok();
    let error = expr.is_none();
    state.watch_rows.push(WatchRow {
        src: src.to_string(),
        expr,
        error,
    });
    state.watch_add_text.clear();
}

fn recompile_trace_cond(state: &mut WatchPanelState) {
    let (cond, error) = compile_opt(&state.trace_cond_src);
    state.trace_cond = cond;
    state.trace_cond_error = error;
}

/// Compile an optional condition. Empty source → `(None, false)`; a parse
/// failure → `(None, true)`.
fn compile_opt(src: &str) -> (Option<Expr>, bool) {
    let trimmed = src.trim();
    if trimmed.is_empty() {
        return (None, false);
    }
    let compiled = Expr::parse(trimmed).ok();
    let error = compiled.is_none();
    (compiled, error)
}

fn parse_hex16(s: &str) -> Option<u16> {
    let t = s.trim().trim_start_matches('$').trim_start_matches("0x");
    if t.is_empty() {
        return None;
    }
    u16::from_str_radix(t, 16).ok()
}

fn push_hit(hits: &mut std::collections::VecDeque<HitRec>, rec: HitRec) {
    if hits.len() >= HIT_LOG_CAP {
        hits.pop_front();
    }
    hits.push_back(rec);
}

fn push_trace(rows: &mut std::collections::VecDeque<String>, row: String) {
    if rows.len() >= TRACE_CAP {
        rows.pop_front();
    }
    rows.push_back(row);
}

/// Expand a trace format string. Tokens are `{name}` for each evaluator token
/// (`a x y s p pc scanline cycle frame value address`), `{[addr]}` for a
/// one-byte peek, and `{{addr}}` for a two-byte word peek. Numeric values are
/// shown in hex. An unknown token is left verbatim.
fn format_trace(fmt: &str, ctx: &dyn EvalContext) -> String {
    use core::fmt::Write as _;

    let mut out = String::with_capacity(fmt.len() + 16);
    let chars: Vec<char> = fmt.chars().collect();
    let mut i = 0;
    while i < chars.len() {
        if chars[i] != '{' {
            out.push(chars[i]);
            i += 1;
            continue;
        }
        // A `{{` opens a 16-bit word peek `{{addr}}`; a single `{` opens a token
        // `{expr}` (evaluated as the byte/word/value the expression yields).
        let is_word = i + 1 < chars.len() && chars[i + 1] == '{';
        let body_start = if is_word { i + 2 } else { i + 1 };
        let close = if is_word {
            find_double_close(&chars, body_start)
        } else {
            chars[body_start..]
                .iter()
                .position(|&c| c == '}')
                .map(|p| body_start + p)
        };
        let Some(close) = close else {
            // No matching close — emit the brace literally and move on.
            out.push('{');
            i += 1;
            continue;
        };
        let inner: String = chars[body_start..close].iter().collect();
        let src = if is_word {
            format!("{{{inner}}}") // wrap in {} so the evaluator does a word peek
        } else {
            inner
        };
        if let Ok(e) = Expr::parse(&src) {
            let v = e.eval(ctx);
            let _ = write!(out, "${:X}", v & 0xFFFF_FFFF);
            i = close + if is_word { 2 } else { 1 };
        } else {
            // Unparseable token — emit the brace literally and continue past it.
            out.push('{');
            i += 1;
        }
    }
    out
}

/// Find the index of the first `}}` at or after `from`, returning the index of
/// the first `}` of the pair.
fn find_double_close(chars: &[char], from: usize) -> Option<usize> {
    let mut i = from;
    while i + 1 < chars.len() {
        if chars[i] == '}' && chars[i + 1] == '}' {
            return Some(i);
        }
        i += 1;
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A minimal evaluator context with a tiny memory window for the
    /// format-string tests (mirrors the expr-module fake, kept local).
    struct FakeCtx {
        a: u8,
        x: u8,
        mem: std::collections::HashMap<u16, u8>,
    }

    impl EvalContext for FakeCtx {
        fn a(&self) -> u8 {
            self.a
        }
        fn x(&self) -> u8 {
            self.x
        }
        fn y(&self) -> u8 {
            0
        }
        fn s(&self) -> u8 {
            0
        }
        fn p(&self) -> u8 {
            0
        }
        fn pc(&self) -> u16 {
            0xC000
        }
        fn scanline(&self) -> i16 {
            10
        }
        fn dot(&self) -> u16 {
            20
        }
        fn frame(&self) -> u64 {
            5
        }
        fn peek(&self, addr: u16) -> u8 {
            self.mem.get(&addr).copied().unwrap_or(0)
        }
        fn access(&self) -> AccessContext {
            AccessContext::default()
        }
    }

    fn ctx() -> FakeCtx {
        let mut mem = std::collections::HashMap::new();
        mem.insert(0x0010, 0x34);
        mem.insert(0x0011, 0x12);
        FakeCtx {
            a: 0x42,
            x: 0x05,
            mem,
        }
    }

    #[test]
    fn format_trace_substitutes_tokens() {
        let c = ctx();
        assert_eq!(format_trace("a={a}", &c), "a=$42");
        assert_eq!(format_trace("{pc}: x={x}", &c), "$C000: x=$5");
        assert_eq!(
            format_trace("sl={scanline} dot={cycle}", &c),
            "sl=$A dot=$14"
        );
    }

    #[test]
    fn format_trace_byte_and_word_peeks() {
        let c = ctx();
        assert_eq!(format_trace("[{[$10]}]", &c), "[$34]");
        // `{{addr}}` is a 16-bit little-endian word peek.
        assert_eq!(format_trace("w={{$10}}", &c), "w=$1234");
    }

    #[test]
    fn format_trace_literal_text_and_bad_tokens() {
        let c = ctx();
        assert_eq!(format_trace("hello", &c), "hello");
        // An unparseable token keeps the brace literal rather than crashing.
        assert_eq!(format_trace("{nope}!", &c), "{nope}!");
        // An unterminated brace (no closing `}`) is emitted literally, and the
        // remaining text is passed through verbatim.
        assert_eq!(format_trace("{a", &c), "{a");
    }

    #[test]
    fn parse_hex16_accepts_common_forms() {
        assert_eq!(parse_hex16("$8000"), Some(0x8000));
        assert_eq!(parse_hex16("0x1F"), Some(0x1F));
        assert_eq!(parse_hex16("  C000 "), Some(0xC000));
        assert_eq!(parse_hex16(""), None);
        assert_eq!(parse_hex16("$"), None);
        assert_eq!(parse_hex16("xyz"), None);
    }

    #[test]
    fn compile_opt_handles_empty_and_invalid() {
        assert!(compile_opt("").0.is_none());
        assert!(!compile_opt("").1); // empty is NOT an error
        assert!(compile_opt("a == 0").0.is_some());
        assert!(!compile_opt("a == 0").1);
        assert!(compile_opt("a ==").0.is_none());
        assert!(compile_opt("a ==").1); // a real parse error
    }

    #[test]
    fn needs_logs_reflect_active_tools() {
        let mut s = WatchPanelState::default();
        assert!(!s.needs_exec_log());
        assert!(!s.needs_access_log());

        // Add a write watchpoint → needs the access log, not the exec log.
        s.wp_kind = WatchKind::Write;
        s.wp_lo_text = "$0300".to_string();
        add_watchpoint(&mut s);
        assert!(!s.needs_exec_log());
        assert!(s.needs_access_log());

        // Add an exec breakpoint → now also needs the exec log.
        s.bp_lo_text = "$8000".to_string();
        add_breakpoint(&mut s);
        assert!(s.needs_exec_log());

        // Disarm → nothing is needed.
        s.armed = false;
        assert!(!s.needs_exec_log());
        assert!(!s.needs_access_log());
    }
}
