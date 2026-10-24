// SPDX-License-Identifier: GPL-3.0-or-later
//! The debugger engine: one [`Nes`], its debug information, breakpoints,
//! source-line stepping and a shadow call stack.
//!
//! Both front ends (the DAP server in [`crate::dap`] and the GDB stub in
//! [`crate::gdb`]) drive this and nothing else, so a behaviour pinned in one
//! front end's tests is the same behaviour in the other.
//!
//! # Breakpoints are the core's
//!
//! Execution breakpoints are the core's own (`Nes::add_breakpoint`, the
//! `debug-hooks` feature) — the same mechanism the frontend debugger, Lua and
//! `rustynes-mcp` use; nothing here re-implements a PC check. The engine keeps
//! *why* each address is armed and re-arms the core's list as the union:
//!
//! - **user** breakpoints (a source line's statement starts, a function's
//!   entry, a raw address from an instruction breakpoint or GDB `Z0`) stop
//!   and are reported;
//! - **frame hooks** (every function's entry, and the address where its frame
//!   base becomes valid) are recorded and execution continues without a stop;
//! - one **temporary** breakpoint carries a step over a `JSR` or a step out.
//!
//! The core stops `run_frame` *before* executing a breakpoint PC and skips
//! that PC once on the next call, so a hook costs one return from `run_frame`
//! and execution resumes exactly where it was: the emulated machine never
//! sees the debugger (no cycle is added, no register changed). That is what
//! keeps a debugging session on the determinism contract: the same ROM and
//! the same requests give the same stops.
//!
//! # The shadow call stack
//!
//! The 6502 has no frame pointer, and neither cc65 nor llvm-mos emits call
//! frame information an unwinder could use, so the engine records frames as
//! they are made: at a function's entry hook it pushes `(function, S)`, with
//! `S` the hardware stack pointer just after the `JSR`; at the function's
//! frame hook it samples the frame base into that entry. A frame whose entry
//! `S` is below the current `S` has returned (its `RTS` raised `S` past it)
//! and is pruned. The return address of a frame is the word its `JSR`
//! pushed, at `$0100 + S + 1`, so caller positions come from the real stack,
//! not from a model of it. Recursion and interrupts nest naturally; code
//! entered without a hooked entry (a hand-written assembler routine, a
//! library function) shows as a frame with no variables.

use std::collections::{BTreeSet, HashMap};

use rustynes_core::Nes;

use crate::debuginfo::{DebugInfo, Encoding, FrameBase, Function, Loc, VarLoc, Variable};

/// The most distinct addresses user breakpoints may arm (each source line
/// can have several statement starts).
pub const MAX_BREAKPOINT_ADDRS: usize = 256;
/// The most instructions one line step may execute before it gives up and
/// reports a stop where it is (a line that waits for an event that never
/// comes must not hang the session).
pub const MAX_STEP_INSTRUCTIONS: u64 = 5_000_000;
/// The most bytes one memory read returns.
pub const MAX_READ_BYTES: usize = 0x1_0000;
/// The deepest shadow stack kept (deeper is a runaway recursion or an
/// unbalanced stack; the oldest frames are dropped).
pub const MAX_FRAMES: usize = 128;

/// Why execution stopped.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum Stop {
    /// A user breakpoint at the current PC.
    Breakpoint,
    /// A step completed.
    Step,
    /// The client asked to pause.
    Pause,
    /// Launched and not yet run.
    Entry,
    /// The CPU executed a JAM (KIL) opcode; the program cannot continue.
    Jam,
    /// A line step ran [`MAX_STEP_INSTRUCTIONS`] without reaching a new line.
    StepLimit,
}

/// How a line step treats calls.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepKind {
    /// Step over calls (DAP `next`).
    Over,
    /// Step into calls that have source (DAP `stepIn`).
    In,
    /// Run until the current function returns (DAP `stepOut`).
    Out,
}

/// One recorded call.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct Shadow {
    func: usize,
    entry_s: u8,
    base: Option<u16>,
}

/// One frame of a stack trace.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct FrameView {
    /// The PC this frame is at: the current PC for the innermost frame, the
    /// address of the `JSR` for a caller.
    pub pc: u16,
    /// The function, as an index into [`DebugInfo::functions`].
    pub func: Option<usize>,
    /// Its frame base, when sampled.
    pub base: Option<u16>,
    /// The source line at `pc`.
    pub loc: Option<Loc>,
    /// The name to show.
    pub name: String,
}

/// A variable's value, rendered.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct Value {
    /// Text for the editor.
    pub text: String,
    /// The address read, when there is one (an editor's memory view).
    pub addr: Option<u16>,
}

/// The engine.
pub struct Engine {
    nes: Nes,
    info: Option<DebugInfo>,
    source_bps: HashMap<usize, BTreeSet<u16>>,
    function_bps: BTreeSet<u16>,
    addr_bps: BTreeSet<u16>,
    user: BTreeSet<u16>,
    entries: HashMap<u16, Vec<usize>>,
    hooks: HashMap<u16, Vec<usize>>,
    temp: Option<(u16, u8)>,
    frames: Vec<Shadow>,
    seen: Option<(u16, u64)>,
}

impl Engine {
    /// Wrap a machine and (optionally) its debug information. Frame hooks are
    /// armed at once, so frames are recorded from the first instruction.
    #[must_use]
    pub fn new(nes: Nes, info: Option<DebugInfo>) -> Self {
        let mut e = Self {
            nes,
            info,
            source_bps: HashMap::new(),
            function_bps: BTreeSet::new(),
            addr_bps: BTreeSet::new(),
            user: BTreeSet::new(),
            entries: HashMap::new(),
            hooks: HashMap::new(),
            temp: None,
            frames: Vec::new(),
            seen: None,
        };
        e.place_hooks();
        e.sync();
        e
    }

    /// The machine (read-only).
    #[must_use]
    pub const fn nes(&self) -> &Nes {
        &self.nes
    }

    /// The debug information, if any.
    #[must_use]
    pub const fn info(&self) -> Option<&DebugInfo> {
        self.info.as_ref()
    }

    /// A side-effect-free CPU-bus read (no register side effect: reading
    /// `$2002` here does not clear `VBlank`).
    pub fn peek(&mut self, addr: u16) -> u8 {
        self.nes.peek(addr)
    }

    /// `len` bytes from `addr` (wrapping at `$FFFF`), at most
    /// [`MAX_READ_BYTES`].
    pub fn read(&mut self, addr: u16, len: usize) -> Vec<u8> {
        (0..len.min(MAX_READ_BYTES))
            .map(|i| {
                // `i < 0x1_0000`, so the wrap is exactly the bus's.
                #[allow(clippy::cast_possible_truncation)]
                let a = addr.wrapping_add(i as u16);
                self.peek(a)
            })
            .collect()
    }

    fn peek16(&mut self, addr: u16) -> u16 {
        u16::from_le_bytes([self.peek(addr), self.peek(addr.wrapping_add(1))])
    }

    /// Place every function's entry and frame hook. A cc65 function whose
    /// first instruction is `JSR` to a runtime push routine (`pusha`,
    /// `pushax`, ...) has its frame hook moved past that `JSR`: cc65's
    /// `offs` values are relative to the soft-stack pointer after the
    /// register-passed parameter is pushed (module docs of
    /// [`crate::debuginfo`]).
    fn place_hooks(&mut self) {
        let Some(info) = self.info.as_mut() else {
            return;
        };
        for i in 0..info.functions.len() {
            let f = &info.functions[i];
            let low = f.low;
            if f.frame_base != FrameBase::None
                && f.frame_hook == low
                && !info.push_routines.is_empty()
            {
                let op = self.nes.peek(low);
                let target = u16::from_le_bytes([
                    self.nes.peek(low.wrapping_add(1)),
                    self.nes.peek(low.wrapping_add(2)),
                ]);
                if op == 0x20 && info.push_routines.contains(&target) {
                    info.functions[i].frame_hook = low.wrapping_add(3);
                }
            }
            let f = &info.functions[i];
            self.entries.entry(f.low).or_default().push(i);
            if f.frame_base != FrameBase::None {
                self.hooks.entry(f.frame_hook).or_default().push(i);
            }
        }
    }

    /// Re-arm the core's breakpoint list as the union of every reason.
    fn sync(&mut self) {
        self.user = self
            .source_bps
            .values()
            .flatten()
            .chain(&self.function_bps)
            .chain(&self.addr_bps)
            .copied()
            .collect();
        self.nes.clear_breakpoints();
        let mut all: BTreeSet<u16> = self.user.clone();
        all.extend(self.entries.keys());
        all.extend(self.hooks.keys());
        if let Some((a, _)) = self.temp {
            all.insert(a);
        }
        for a in all {
            self.nes.add_breakpoint(a);
        }
    }

    fn user_count(&self) -> usize {
        let mut set: BTreeSet<u16> = self.source_bps.values().flatten().copied().collect();
        set.extend(&self.function_bps);
        set.extend(&self.addr_bps);
        set.len()
    }

    /// Replace the breakpoints of one source file. Returns, per requested
    /// line, the line actually used and whether it was armed (`None`: the
    /// file has no code at or after that line).
    ///
    /// # Errors
    ///
    /// The request would arm more than [`MAX_BREAKPOINT_ADDRS`] addresses;
    /// nothing changes.
    pub fn set_source_breakpoints(
        &mut self,
        file: usize,
        lines: &[u32],
    ) -> Result<Vec<Option<u32>>, String> {
        let mut addrs = BTreeSet::new();
        let mut out = Vec::with_capacity(lines.len());
        for &line in lines {
            match self.info.as_ref().and_then(|i| i.resolve_line(file, line)) {
                Some((actual, a)) => {
                    addrs.extend(a);
                    out.push(Some(actual));
                }
                None => out.push(None),
            }
        }
        let mut total: BTreeSet<u16> = addrs.clone();
        for (f, a) in &self.source_bps {
            if *f != file {
                total.extend(a);
            }
        }
        total.extend(&self.function_bps);
        total.extend(&self.addr_bps);
        if total.len() > MAX_BREAKPOINT_ADDRS {
            return Err(format!(
                "at most {MAX_BREAKPOINT_ADDRS} breakpoint addresses"
            ));
        }
        self.source_bps.insert(file, addrs);
        self.sync();
        Ok(out)
    }

    /// Replace the function breakpoints. Returns each name's entry address.
    ///
    /// # Errors
    ///
    /// Too many addresses; nothing changes.
    pub fn set_function_breakpoints(&mut self, names: &[&str]) -> Result<Vec<Option<u16>>, String> {
        let found: Vec<Option<u16>> = names
            .iter()
            .map(|n| {
                self.info
                    .as_ref()
                    .and_then(|i| i.function_named(n))
                    .map(|f| f.low)
            })
            .collect();
        let set: BTreeSet<u16> = found.iter().flatten().copied().collect();
        self.replace_set(set, |e| &mut e.function_bps)?;
        Ok(found)
    }

    /// Replace the raw-address breakpoints (DAP instruction breakpoints).
    ///
    /// # Errors
    ///
    /// Too many addresses; nothing changes.
    pub fn set_address_breakpoints(&mut self, addrs: &[u16]) -> Result<(), String> {
        self.replace_set(addrs.iter().copied().collect(), |e| &mut e.addr_bps)
    }

    /// Add one raw-address breakpoint (GDB `Z0`).
    ///
    /// # Errors
    ///
    /// Too many addresses.
    pub fn add_address_breakpoint(&mut self, addr: u16) -> Result<(), String> {
        let mut set = self.addr_bps.clone();
        set.insert(addr);
        self.replace_set(set, |e| &mut e.addr_bps)
    }

    /// Remove one raw-address breakpoint (GDB `z0`).
    pub fn remove_address_breakpoint(&mut self, addr: u16) {
        self.addr_bps.remove(&addr);
        self.sync();
    }

    fn replace_set(
        &mut self,
        set: BTreeSet<u16>,
        field: fn(&mut Self) -> &mut BTreeSet<u16>,
    ) -> Result<(), String> {
        let old = std::mem::replace(field(self), set);
        if self.user_count() > MAX_BREAKPOINT_ADDRS {
            *field(self) = old;
            return Err(format!(
                "at most {MAX_BREAKPOINT_ADDRS} breakpoint addresses"
            ));
        }
        self.sync();
        Ok(())
    }

    /// The distinct addresses user breakpoints arm.
    #[must_use]
    pub const fn user_breakpoints(&self) -> &BTreeSet<u16> {
        &self.user
    }

    const fn pc(&self) -> u16 {
        self.nes.cpu().pc
    }

    const fn s(&self) -> u8 {
        self.nes.cpu().s
    }

    /// Record the frame hooks at the current PC, once per arrival (the
    /// same PC at the same CPU cycle count is the same arrival).
    fn arrive(&mut self) {
        let pc = self.pc();
        let key = (pc, self.nes.cpu().cycles);
        if self.seen == Some(key) {
            return;
        }
        self.seen = Some(key);
        if let Some(funcs) = self.entries.get(&pc).cloned() {
            let s = self.s();
            self.prune();
            for func in funcs {
                if self.frames.len() >= MAX_FRAMES {
                    self.frames.remove(0);
                }
                self.frames.push(Shadow {
                    func,
                    entry_s: s,
                    base: None,
                });
            }
        }
        if let Some(funcs) = self.hooks.get(&pc).cloned() {
            for func in funcs {
                let base = self.frame_base_now(func);
                if let Some(top) = self.frames.last_mut()
                    && top.func == func
                    && top.base.is_none()
                {
                    top.base = base;
                }
            }
        }
    }

    fn frame_base_now(&mut self, func: usize) -> Option<u16> {
        let fb = self.info.as_ref()?.functions.get(func)?.frame_base;
        match fb {
            FrameBase::None => None,
            FrameBase::ZpPointer(zp) => {
                let zp = u16::from(zp);
                Some(u16::from_le_bytes([
                    self.peek(zp),
                    self.peek((zp + 1) & 0xFF),
                ]))
            }
        }
    }

    /// Drop frames that have returned: their entry `S` is below the current
    /// `S`.
    fn prune(&mut self) {
        let s = self.s();
        while self.frames.last().is_some_and(|f| f.entry_s < s) {
            self.frames.pop();
        }
    }

    fn exec_one(&mut self) {
        self.arrive();
        self.nes.step_instruction();
        self.arrive();
    }

    /// Execute exactly one instruction.
    pub fn step_instruction(&mut self) -> Stop {
        if self.nes.cpu().is_jammed() {
            return Stop::Jam;
        }
        self.exec_one();
        if self.nes.cpu().is_jammed() {
            Stop::Jam
        } else {
            Stop::Step
        }
    }

    /// Run until a user breakpoint, the temporary breakpoint, a JAM, or
    /// `poll` returns `true` (it is called whenever the core returns from a
    /// frame or a hook, which is at least once per emulated frame). The
    /// instruction at the current PC is executed first, so continuing from
    /// a breakpoint does not stop on it again.
    pub fn resume(&mut self, poll: &mut dyn FnMut() -> bool) -> Stop {
        if self.nes.cpu().is_jammed() {
            return Stop::Jam;
        }
        self.exec_one();
        loop {
            self.arrive();
            if self.nes.cpu().is_jammed() {
                return Stop::Jam;
            }
            let pc = self.pc();
            if let Some((t, min_s)) = self.temp
                && t == pc
                && self.s() >= min_s
            {
                return Stop::Step;
            }
            if self.user.contains(&pc) {
                return Stop::Breakpoint;
            }
            self.nes.run_frame();
            let hit = self.nes.take_break_hit().is_some();
            if !hit && poll() {
                return Stop::Pause;
            }
            if hit && poll() {
                // A pause request arriving while hooks keep firing is still
                // honoured promptly.
                self.arrive();
                if !self.user.contains(&self.pc()) {
                    return Stop::Pause;
                }
            }
        }
    }

    fn run_to(&mut self, addr: u16, min_s: u8, poll: &mut dyn FnMut() -> bool) -> Stop {
        self.temp = Some((addr, min_s));
        self.sync();
        let stop = self.resume(poll);
        self.temp = None;
        self.sync();
        stop
    }

    fn has_source(&self, addr: u16) -> bool {
        self.info
            .as_ref()
            .is_some_and(|i| i.stmt_at(addr).is_some() || i.line_at(addr).is_some())
    }

    /// Step by source line.
    pub fn step_line(&mut self, kind: StepKind, poll: &mut dyn FnMut() -> bool) -> Stop {
        if kind == StepKind::Out {
            return self.step_out(poll);
        }
        let Some(info) = self.info.as_ref() else {
            return self.step_instruction();
        };
        let start_pc = self.pc();
        let start = info.line_at(start_pc);
        let start_s = self.s();
        let mut executed: u64 = 0;
        loop {
            if self.nes.cpu().is_jammed() {
                return Stop::Jam;
            }
            let pc = self.pc();
            let s_before = self.s();
            if self.peek(pc) == 0x20 {
                let target = self.peek16(pc.wrapping_add(1));
                if kind == StepKind::Over || !self.has_source(target) {
                    match self.run_to(pc.wrapping_add(3), s_before, poll) {
                        Stop::Step => {}
                        other => return other,
                    }
                } else {
                    self.exec_one();
                    // Into a function with source: finish its prologue, so
                    // the stop is on its first body line with the frame base
                    // sampled (what a C programmer means by "step into").
                    let hook = self
                        .function_at(target)
                        .filter(|f| f.low == target && f.frame_hook != target)
                        .map(|f| f.frame_hook);
                    if let Some(hook) = hook
                        && self
                            .info
                            .as_ref()
                            .is_some_and(|i| i.stmt_at(hook).is_some())
                    {
                        return match self.run_to(hook, 0, poll) {
                            Stop::Step => Stop::Step,
                            other => other,
                        };
                    }
                }
            } else {
                self.exec_one();
            }
            executed += 1;
            if self.nes.cpu().is_jammed() {
                return Stop::Jam;
            }
            let pc = self.pc();
            if self.user.contains(&pc) {
                return Stop::Breakpoint;
            }
            let info = self.info.as_ref().unwrap_or_else(|| unreachable!());
            if let Some(loc) = info.stmt_at(pc)
                && (Some(loc) != start || pc == start_pc || self.s() > start_s)
            {
                return Stop::Step;
            }
            if executed >= MAX_STEP_INSTRUCTIONS {
                return Stop::StepLimit;
            }
            if executed.is_multiple_of(30_000) && poll() {
                return Stop::Pause;
            }
        }
    }

    fn step_out(&mut self, poll: &mut dyn FnMut() -> bool) -> Stop {
        self.prune();
        let pc = self.pc();
        let inner = self
            .info
            .as_ref()
            .and_then(|i| i.function_at(pc))
            .map(|(i, _)| i);
        let Some(top) = self
            .frames
            .last()
            .copied()
            .filter(|t| Some(t.func) == inner)
        else {
            // No recorded frame for this code: fall back to one instruction,
            // which is honest about what is known.
            return self.step_instruction();
        };
        let ret = self.pushed_return(top.entry_s).wrapping_add(1);
        self.run_to(ret, top.entry_s.wrapping_add(1), poll)
    }

    /// The word a `JSR` pushed for a frame whose entry stack pointer is
    /// `entry_s`: the `JSR`'s address plus 2, low byte at `$0100 + S + 1`
    /// (the page wraps, as the 6502's does).
    fn pushed_return(&mut self, entry_s: u8) -> u16 {
        let lo = self.peek(0x0100 | u16::from(entry_s.wrapping_add(1)));
        let hi = self.peek(0x0100 | u16::from(entry_s.wrapping_add(2)));
        u16::from_le_bytes([lo, hi])
    }

    /// The call stack, innermost first.
    pub fn stack(&mut self) -> Vec<FrameView> {
        self.prune();
        let pc = self.pc();
        let inner = self
            .info
            .as_ref()
            .and_then(|i| i.function_at(pc))
            .map(|(i, _)| i);
        let frames = self.frames.clone();
        let mut out = Vec::new();
        let mut rest = frames.len();
        let mut callee: Option<Shadow> = None;
        if let Some(top) = frames.last()
            && Some(top.func) == inner
        {
            out.push(self.view(pc, Some(top.func), top.base));
            callee = Some(*top);
            rest -= 1;
        } else {
            out.push(self.view(pc, inner, None));
        }
        for sh in frames[..rest].iter().rev() {
            let call_pc = match callee {
                Some(c) => self.pushed_return(c.entry_s).wrapping_sub(2),
                None => self.info.as_ref().map_or(0, |i| i.functions[sh.func].low),
            };
            out.push(self.view(call_pc, Some(sh.func), sh.base));
            callee = Some(*sh);
        }
        out
    }

    fn view(&self, pc: u16, func: Option<usize>, base: Option<u16>) -> FrameView {
        let info = self.info.as_ref();
        let name = func
            .and_then(|f| info.map(|i| i.functions[f].name.clone()))
            .unwrap_or_else(|| format!("${pc:04X}"));
        FrameView {
            pc,
            func,
            base,
            loc: info.and_then(|i| i.line_at(pc)),
            name,
        }
    }

    /// The variables of a frame.
    #[must_use]
    pub fn frame_vars(&self, frame: &FrameView) -> Vec<Variable> {
        frame
            .func
            .and_then(|f| self.info.as_ref().map(|i| i.functions[f].vars.clone()))
            .unwrap_or_default()
    }

    /// Render a variable's value in a frame (frame base `base`).
    pub fn value(&mut self, var: &Variable, base: Option<u16>) -> Value {
        let addr = match &var.loc {
            VarLoc::Absolute(a) => *a,
            VarLoc::Frame(off) => match base {
                // Two's-complement add, wrapping like the 16-bit pointer.
                #[allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]
                Some(b) => b.wrapping_add(*off as u16),
                None => {
                    return Value {
                        text: "<frame base not yet set: stop after the prologue>".into(),
                        addr: None,
                    };
                }
            },
            VarLoc::Unavailable(why) => {
                return Value {
                    text: format!("<{why}>"),
                    addr: None,
                };
            }
        };
        let bytes = self.read(addr, usize::from(var.ty.size.min(64)));
        Value {
            text: render(&bytes, var.ty.encoding),
            addr: Some(addr),
        }
    }

    /// The function at `pc`, if any.
    #[must_use]
    pub fn function_at(&self, pc: u16) -> Option<&Function> {
        self.info.as_ref()?.function_at(pc).map(|(_, f)| f)
    }
}

/// Render little-endian bytes per their encoding.
fn render(bytes: &[u8], enc: Encoding) -> String {
    let unsigned = || {
        bytes
            .iter()
            .rev()
            .fold(0u64, |acc, &b| (acc << 8) | u64::from(b))
    };
    match enc {
        _ if bytes.is_empty() => "<empty>".into(),
        Encoding::Unsigned | Encoding::Unknown if bytes.len() <= 8 => {
            let v = unsigned();
            format!("{v} (0x{v:0w$X})", w = bytes.len() * 2)
        }
        Encoding::Signed if bytes.len() <= 8 => {
            let bits = bytes.len() * 8;
            let v = unsigned();
            // Sign-extend from `bits`.
            #[allow(clippy::cast_possible_wrap)]
            let s = if bits < 64 {
                ((v << (64 - bits)) as i64) >> (64 - bits)
            } else {
                v as i64
            };
            format!("{s}")
        }
        Encoding::Bool => (if bytes.iter().any(|&b| b != 0) {
            "true"
        } else {
            "false"
        })
        .into(),
        Encoding::Pointer if bytes.len() == 2 => format!("${:04X}", unsigned()),
        _ => bytes
            .iter()
            .map(|b| format!("{b:02X}"))
            .collect::<Vec<_>>()
            .join(" "),
    }
}

#[cfg(test)]
mod tests {
    use super::render;
    use crate::debuginfo::Encoding;

    #[test]
    fn renders_each_encoding() {
        assert_eq!(render(&[5], Encoding::Unknown), "5 (0x05)");
        assert_eq!(render(&[0x34, 0x12], Encoding::Unsigned), "4660 (0x1234)");
        assert_eq!(render(&[0xFF], Encoding::Signed), "-1");
        assert_eq!(render(&[0xFE, 0xFF], Encoding::Signed), "-2");
        assert_eq!(render(&[0x7F], Encoding::Signed), "127");
        assert_eq!(render(&[0], Encoding::Bool), "false");
        assert_eq!(render(&[2], Encoding::Bool), "true");
        assert_eq!(render(&[0x00, 0x60], Encoding::Pointer), "$6000");
        assert_eq!(render(&[1, 2, 3], Encoding::Bytes), "01 02 03");
        assert_eq!(render(&[], Encoding::Unsigned), "<empty>");
    }
}
