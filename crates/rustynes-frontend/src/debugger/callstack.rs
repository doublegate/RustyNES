//! v1.7.0 "Forge" Workstream C (C1) — call-stack tracking + stepping verbs.
//!
//! A Mesen2-`CallstackManager`-class live 6502 call stack plus the execution-
//! control "step" verbs (step-over / step-out / run-to-NMI / run-to-IRQ /
//! step-scanline / step-frame), all built on the **current** PPU-dot scheduler
//! (NOT the future v2.0 timebase rewrite).
//!
//! ## How it works (output-only telemetry)
//!
//! Everything here rides the existing `debug-hooks` per-frame log-replay model
//! (the same machinery the Lua `onExec` / `onNmi` callbacks and the Watch panel
//! use):
//!
//! - The frame's executed program counters arrive as
//!   [`rustynes_core::Nes::exec_log`] (a `&[u16]`, in execution order).
//! - The frame's committed interrupt-service entries arrive as
//!   [`rustynes_core::Nes::interrupt_log`] (NMI vs IRQ/BRK + the vector).
//!
//! [`CallstackTracker::replay_frame`] walks the exec log once, peeking each
//! instruction's opcode through the read-only [`rustynes_core::Nes::cpu_bus_peek`]
//! bus peek, and maintains a deque of [`StackFrame`]s:
//!
//! - `JSR` (`$20`) pushes a frame (return address = `pc + 3`, target = the next
//!   executed PC).
//! - `RTS` (`$60`) / `RTI` (`$40`) pops the innermost matching frame.
//! - A *non-sequential* PC transition that the previous opcode does **not**
//!   explain (i.e. not a branch / jump / `JSR` / `RTS` / `RTI`) is treated as a
//!   hardware-interrupt entry; it is correlated against the per-frame
//!   interrupt-service log to label it NMI vs IRQ/BRK and pushes an interrupt
//!   frame (popped by the matching `RTI`).
//!
//! The tracker only ever *reads* the core (peeks + the per-frame logs), so it is
//! purely observational: the determinism contract and `AccuracyCoin` are
//! unaffected, and with the core's `debug-hooks` feature OFF (the headless
//! test/bench builds) the hot path is byte-identical.

use std::collections::VecDeque;

use rustynes_core::Nes;

use crate::symbols::SymbolMap;

/// The maximum call-stack depth the tracker retains (matches Mesen2's bound).
/// The 6502 stack is one page, so a sane program never nests this deep; the cap
/// just guards against a runaway recursion / corrupt stack from growing the
/// deque without bound.
const MAX_DEPTH: usize = 511;

/// Why a [`StackFrame`] was pushed.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum FrameKind {
    /// A `JSR` subroutine call.
    Subroutine,
    /// An NMI service entry (`$FFFA`).
    Nmi,
    /// An IRQ / `BRK` service entry (`$FFFE`).
    Irq,
}

impl FrameKind {
    /// A short tag for the call-stack list.
    const fn tag(self) -> &'static str {
        match self {
            Self::Subroutine => "JSR",
            Self::Nmi => "NMI",
            Self::Irq => "IRQ",
        }
    }
}

/// One entry in the live call stack.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct StackFrame {
    /// Why this frame was entered.
    pub kind: FrameKind,
    /// The address the call jumped *to* (the subroutine / handler entry point).
    pub target: u16,
    /// The address control returns to when this frame pops (the instruction
    /// after the `JSR`, or the interrupted instruction for an interrupt frame).
    pub return_addr: u16,
}

/// The execution-control "step" verbs. A request rides the per-frame exec-log
/// replay: the frontend arms it, runs frames until the tracker reports the
/// target reached, then pauses. None of these alter emulation — they only
/// observe the exec log to decide *when* to stop.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum StepRequest {
    /// Step over a `JSR` (run until control returns to the same stack depth or
    /// shallower — i.e. the call returns), otherwise a single instruction.
    Over,
    /// Step out of the current subroutine (run until the stack pops below the
    /// depth captured when the request was made).
    Out,
    /// Run until the next NMI service entry.
    RunToNmi,
    /// Run until the next IRQ / `BRK` service entry.
    RunToIrq,
    /// Run to the end of the current scanline (advance one PPU scanline).
    Scanline,
    /// Run to the end of the current frame (advance one PPU frame).
    Frame,
}

impl StepRequest {
    /// A human label for the status bar.
    #[must_use]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Over => "step over",
            Self::Out => "step out",
            Self::RunToNmi => "run to NMI",
            Self::RunToIrq => "run to IRQ",
            Self::Scanline => "step scanline",
            Self::Frame => "step frame",
        }
    }

    /// `true` if this verb is satisfied by observing the per-frame *exec* log
    /// (so the exec log must be armed while it is pending).
    const fn needs_exec_log(self) -> bool {
        matches!(self, Self::Over | Self::Out)
    }

    /// `true` if this verb is satisfied by observing the per-frame *interrupt*
    /// log.
    const fn needs_interrupt_log(self) -> bool {
        matches!(self, Self::RunToNmi | Self::RunToIrq)
    }
}

/// The live call-stack tracker + pending step request.
///
/// Lives in the debugger overlay (frontend), driven once per frame from the
/// existing `pump_watchpoints` observational hook.
#[derive(Default)]
pub struct CallstackTracker {
    /// Current call stack, innermost frame last.
    stack: VecDeque<StackFrame>,
    /// A pending step request, plus the stack depth captured when it was made
    /// (used by step-over / step-out to know when control has returned).
    pending: Option<(StepRequest, usize)>,
    /// Set true for one pump after a step request is satisfied, so the app can
    /// pause emulation and surface a status line.
    satisfied: bool,
}

/// 6502 opcodes the tracker recognises by their fixed encoding.
const OP_BRK: u8 = 0x00;
const OP_JSR: u8 = 0x20;
const OP_RTI: u8 = 0x40;
const OP_RTS: u8 = 0x60;
const OP_JMP_ABS: u8 = 0x4C;
const OP_JMP_IND: u8 = 0x6C;

impl CallstackTracker {
    /// The current call stack, outermost first.
    #[must_use]
    pub fn frames(&self) -> impl ExactSizeIterator<Item = &StackFrame> + DoubleEndedIterator {
        self.stack.iter()
    }

    /// Whether the call stack is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.stack.is_empty()
    }

    /// Drop the whole stack (on reset / power-cycle / save-state load, where the
    /// reconstructed stack would otherwise be stale).
    pub fn clear(&mut self) {
        self.stack.clear();
        self.pending = None;
        self.satisfied = false;
    }

    /// Queue a step request. The app keeps running frames until [`Self::take_satisfied`]
    /// reports the verb is done, then pauses.
    pub fn request_step(&mut self, req: StepRequest) {
        self.pending = Some((req, self.stack.len()));
        self.satisfied = false;
    }

    /// Whether a step request is in flight (the app keeps the emulator running
    /// while one is).
    #[must_use]
    pub const fn step_pending(&self) -> bool {
        self.pending.is_some()
    }

    /// Take the "step satisfied this pump" edge (resets it). The app pauses when
    /// this returns `true`.
    pub fn take_satisfied(&mut self) -> bool {
        std::mem::take(&mut self.satisfied)
    }

    /// Whether the tracker wants the core's per-frame exec log armed (it does
    /// while a panel is open *or* an exec-driven step is pending, so the next
    /// frame is captured).
    #[must_use]
    pub fn wants_exec_log(&self, panel_open: bool) -> bool {
        panel_open || self.pending.is_some_and(|(req, _)| req.needs_exec_log())
    }

    /// Whether the tracker wants the core's per-frame interrupt log armed.
    #[must_use]
    pub fn wants_interrupt_log(&self, panel_open: bool) -> bool {
        panel_open
            || self
                .pending
                .is_some_and(|(req, _)| req.needs_interrupt_log())
    }

    /// Replay the just-finished frame's exec + interrupt logs to rebuild the
    /// live call stack and evaluate any pending step request. Observational —
    /// `nes` is only read (peeks + the per-frame logs).
    pub fn replay_frame(&mut self, nes: &mut Nes) {
        // Snapshot the logs into owned buffers so we can hold `&mut nes` for the
        // opcode peeks below without overlapping borrows.
        let exec: Vec<u16> = nes.exec_log().to_vec();
        let interrupts: Vec<(bool, u16)> = nes
            .interrupt_log()
            .iter()
            .map(|rec| (rec.is_nmi, rec.vector))
            .collect();
        if exec.is_empty() {
            return;
        }

        // The interrupt-service log is consumed in order: each detected hardware
        // interrupt entry pulls the next record to label NMI vs IRQ.
        let mut next_irq = 0usize;

        for win in exec.windows(2) {
            let pc = win[0];
            let next = win[1];
            let opcode = nes.cpu_bus_peek(pc);

            match opcode {
                OP_JSR => {
                    // `JSR $nnnn` is 3 bytes; control returns to pc+3. The call
                    // target is simply the next executed PC.
                    self.push(StackFrame {
                        kind: FrameKind::Subroutine,
                        target: next,
                        return_addr: pc.wrapping_add(3),
                    });
                }
                OP_RTS | OP_RTI => {
                    self.stack.pop_back();
                }
                _ => {
                    // A non-sequential transition the opcode does not itself
                    // explain is a hardware-interrupt entry. (`JMP`, branches,
                    // and the call/return opcodes above are handled / excluded.)
                    if next != fallthrough(pc, opcode) && !is_control_flow(opcode) {
                        let (kind, ret) = match interrupts.get(next_irq) {
                            Some(&(true, _)) => (FrameKind::Nmi, pc),
                            Some(&(false, _)) => (FrameKind::Irq, pc),
                            // No matching service record (the heuristic fired on
                            // a jump the decoder missed): skip — don't invent a
                            // frame.
                            None => continue,
                        };
                        next_irq += 1;
                        self.push(StackFrame {
                            kind,
                            target: next,
                            return_addr: ret,
                        });
                    }
                }
            }

            self.evaluate_pending(opcode);
        }

        // Scanline / Frame steps are satisfied by simply having advanced (the
        // app advances exactly one scanline / frame for them), so they resolve
        // the moment they are pumped after the advance.
        if let Some((StepRequest::Scanline | StepRequest::Frame, _)) = self.pending {
            self.pending = None;
            self.satisfied = true;
        }
    }

    /// Push a frame, enforcing the depth cap (drop the oldest on overflow so a
    /// runaway can't grow the deque unbounded).
    fn push(&mut self, frame: StackFrame) {
        if self.stack.len() >= MAX_DEPTH {
            self.stack.pop_front();
        }
        self.stack.push_back(frame);
    }

    /// Resolve a pending exec/interrupt-driven step against the latest opcode +
    /// the current stack depth.
    fn evaluate_pending(&mut self, opcode: u8) {
        let Some((req, depth)) = self.pending else {
            return;
        };
        let done = match req {
            // Step-over completes when control returns to the request depth or
            // shallower; step-out when it drops strictly below it.
            StepRequest::Over => self.stack.len() <= depth,
            StepRequest::Out => self.stack.len() < depth,
            // Run-to-NMI / run-to-IRQ complete when the matching interrupt frame
            // is pushed (it becomes the innermost frame).
            StepRequest::RunToNmi => {
                matches!(self.stack.back(), Some(f) if f.kind == FrameKind::Nmi)
            }
            StepRequest::RunToIrq => {
                matches!(self.stack.back(), Some(f) if f.kind == FrameKind::Irq)
            }
            StepRequest::Scanline | StepRequest::Frame => false,
        };
        // Step-over of a non-`JSR` instruction is a plain single-instruction
        // step: any instruction observed at request depth satisfies it.
        let single = req == StepRequest::Over && opcode != OP_JSR;
        if done || single {
            self.pending = None;
            self.satisfied = true;
        }
    }
}

/// The PC that would follow `pc` if the instruction there fell through linearly
/// (used to detect interrupt entry). Uses the opcode's encoded length,
/// defaulting to 1 for the implied / accumulator forms.
fn fallthrough(pc: u16, opcode: u8) -> u16 {
    pc.wrapping_add(u16::from(opcode_len(opcode)))
}

/// The encoded byte length of a 6502 instruction by opcode. Covers the official
/// plus common unofficial encodings; an unknown opcode defaults to 1 (the safe
/// minimum for the fall-through interrupt heuristic).
fn opcode_len(opcode: u8) -> u8 {
    match opcode {
        // 3-byte: absolute / absolute-indexed / indirect + JSR/JMP-abs.
        0x0C | 0x0D | 0x0E | 0x0F | 0x19 | 0x1B | 0x1C | 0x1D | 0x1E | 0x1F | 0x20 | 0x2C
        | 0x2D | 0x2E | 0x2F | 0x39 | 0x3B | 0x3C | 0x3D | 0x3E | 0x3F | 0x4C | 0x4D | 0x4E
        | 0x4F | 0x59 | 0x5B | 0x5C | 0x5D | 0x5E | 0x5F | 0x6C | 0x6D | 0x6E | 0x6F | 0x79
        | 0x7B | 0x7C | 0x7D | 0x7E | 0x7F | 0x8C | 0x8D | 0x8E | 0x8F | 0x99 | 0x9B | 0x9C
        | 0x9D | 0x9E | 0x9F | 0xAC | 0xAD | 0xAE | 0xAF | 0xB9 | 0xBB | 0xBC | 0xBD | 0xBE
        | 0xBF | 0xCC | 0xCD | 0xCE | 0xCF | 0xD9 | 0xDB | 0xDC | 0xDD | 0xDE | 0xDF | 0xEC
        | 0xED | 0xEE | 0xEF | 0xF9 | 0xFB | 0xFC | 0xFD | 0xFE | 0xFF => 3,
        // 2-byte: immediate / zero-page / zp-indexed / (ind,X)/(ind),Y / relative.
        0x01 | 0x03 | 0x04 | 0x05 | 0x06 | 0x07 | 0x09 | 0x0B | 0x10 | 0x11 | 0x13 | 0x14
        | 0x15 | 0x16 | 0x17 | 0x21 | 0x23 | 0x24 | 0x25 | 0x26 | 0x27 | 0x29 | 0x2B | 0x30
        | 0x31 | 0x33 | 0x34 | 0x35 | 0x36 | 0x37 | 0x41 | 0x43 | 0x44 | 0x45 | 0x46 | 0x47
        | 0x49 | 0x4B | 0x50 | 0x51 | 0x53 | 0x54 | 0x55 | 0x56 | 0x57 | 0x61 | 0x63 | 0x64
        | 0x65 | 0x66 | 0x67 | 0x69 | 0x6B | 0x70 | 0x71 | 0x73 | 0x74 | 0x75 | 0x76 | 0x77
        | 0x80 | 0x81 | 0x82 | 0x83 | 0x84 | 0x85 | 0x86 | 0x87 | 0x89 | 0x90 | 0x91 | 0x93
        | 0x94 | 0x95 | 0x96 | 0x97 | 0xA0 | 0xA1 | 0xA2 | 0xA3 | 0xA4 | 0xA5 | 0xA6 | 0xA7
        | 0xA9 | 0xAB | 0xB0 | 0xB1 | 0xB3 | 0xB4 | 0xB5 | 0xB6 | 0xB7 | 0xC0 | 0xC1 | 0xC2
        | 0xC3 | 0xC4 | 0xC5 | 0xC6 | 0xC7 | 0xC9 | 0xCB | 0xD0 | 0xD1 | 0xD3 | 0xD4 | 0xD5
        | 0xD6 | 0xD7 | 0xE0 | 0xE1 | 0xE2 | 0xE3 | 0xE4 | 0xE5 | 0xE6 | 0xE7 | 0xE9 | 0xEB
        | 0xF0 | 0xF1 | 0xF3 | 0xF4 | 0xF5 | 0xF6 | 0xF7 => 2,
        // Everything else (implied / accumulator / the KIL/JAM opcodes) is 1.
        _ => 1,
    }
}

/// `true` if `opcode` is a control-flow instruction whose target need not be
/// the linear fall-through (so a non-sequential PC after it is NOT an
/// interrupt). Covers `JMP` (abs + indirect), `JSR`, `RTS`, `RTI`, `BRK`, and
/// the relative branches.
fn is_control_flow(opcode: u8) -> bool {
    matches!(
        opcode,
        OP_BRK | OP_JSR | OP_RTI | OP_RTS | OP_JMP_ABS | OP_JMP_IND
    ) || is_branch(opcode)
}

/// `true` for the eight relative-branch opcodes (`BPL`/`BMI`/`BVC`/`BVS`/`BCC`/
/// `BCS`/`BNE`/`BEQ`), whose taken target is not the linear fall-through.
const fn is_branch(opcode: u8) -> bool {
    matches!(
        opcode,
        0x10 | 0x30 | 0x50 | 0x70 | 0x90 | 0xB0 | 0xD0 | 0xF0
    )
}

/// Render the Call Stack collapsing section inside the CPU panel. Self-contained
/// so it merges cleanly with the parallel panel work; reads the tracker + the
/// loaded labels and emits the queued step requests back to the caller.
///
/// Returns a step request the user clicked (the app queues it + keeps the
/// emulator running until satisfied).
pub fn show_callstack_section(
    ui: &mut egui::Ui,
    tracker: &CallstackTracker,
    symbols: &SymbolMap,
) -> Option<StepRequest> {
    let mut requested = None;
    egui::CollapsingHeader::new("Call stack")
        .default_open(false)
        .show(ui, |ui| {
            ui.horizontal_wrapped(|ui| {
                for req in [
                    StepRequest::Over,
                    StepRequest::Out,
                    StepRequest::RunToNmi,
                    StepRequest::RunToIrq,
                    StepRequest::Scanline,
                    StepRequest::Frame,
                ] {
                    if ui.button(req.label()).clicked() {
                        requested = Some(req);
                    }
                }
            });
            ui.weak(
                "Step verbs ride the debug-hooks exec log; output-only \
                 (emulation is unaffected).",
            );
            ui.separator();
            if tracker.is_empty() {
                ui.weak("(call stack empty)");
            } else {
                // Innermost frame first (top of the listing), like a debugger.
                for (i, frame) in tracker.frames().rev().enumerate() {
                    ui.horizontal(|ui| {
                        ui.monospace(format!("#{i} {} ${:04X}", frame.kind.tag(), frame.target));
                        if let Some(label) = symbols.label(frame.target) {
                            ui.colored_label(egui::Color32::from_rgb(0x90, 0xC0, 0xF0), label);
                        }
                        ui.weak(format!("ret ${:04X}", frame.return_addr));
                    });
                }
            }
        });
    requested
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn opcode_lengths_match_known_encodings() {
        assert_eq!(opcode_len(OP_JSR), 3, "JSR is absolute (3 bytes)");
        assert_eq!(opcode_len(OP_RTS), 1, "RTS is implied (1 byte)");
        assert_eq!(opcode_len(OP_RTI), 1, "RTI is implied (1 byte)");
        assert_eq!(opcode_len(0xA9), 2, "LDA #imm is 2 bytes");
        assert_eq!(opcode_len(0xAD), 3, "LDA abs is 3 bytes");
        assert_eq!(opcode_len(0xEA), 1, "NOP is 1 byte");
    }

    #[test]
    fn control_flow_classification() {
        assert!(is_control_flow(OP_JSR));
        assert!(is_control_flow(OP_RTS));
        assert!(is_control_flow(OP_JMP_ABS));
        assert!(is_control_flow(0xD0), "BNE is a branch");
        assert!(!is_control_flow(0xA9), "LDA #imm is not control flow");
        assert!(!is_control_flow(0xEA), "NOP is not control flow");
    }

    #[test]
    fn step_request_log_needs() {
        assert!(StepRequest::Over.needs_exec_log());
        assert!(StepRequest::Out.needs_exec_log());
        assert!(!StepRequest::Over.needs_interrupt_log());
        assert!(StepRequest::RunToNmi.needs_interrupt_log());
        assert!(StepRequest::RunToIrq.needs_interrupt_log());
        assert!(!StepRequest::RunToNmi.needs_exec_log());
    }

    #[test]
    fn fresh_tracker_is_empty_and_idle() {
        let t = CallstackTracker::default();
        assert_eq!(t.frames().len(), 0);
        assert!(!t.step_pending());
        assert!(!t.wants_exec_log(false));
        assert!(t.wants_exec_log(true), "an open panel wants the exec log");
    }

    #[test]
    fn pending_step_drives_log_arming() {
        let mut t = CallstackTracker::default();
        t.request_step(StepRequest::Over);
        assert!(t.step_pending());
        assert!(t.wants_exec_log(false), "step-over needs the exec log");
        assert!(!t.wants_interrupt_log(false));

        let mut t2 = CallstackTracker::default();
        t2.request_step(StepRequest::RunToNmi);
        assert!(t2.wants_interrupt_log(false));
        assert!(!t2.wants_exec_log(false));
    }

    #[test]
    fn clear_resets_everything() {
        let mut t = CallstackTracker::default();
        t.push(StackFrame {
            kind: FrameKind::Subroutine,
            target: 0x8000,
            return_addr: 0xC003,
        });
        t.request_step(StepRequest::Out);
        t.clear();
        assert_eq!(t.frames().len(), 0);
        assert!(!t.step_pending());
    }

    #[test]
    fn depth_cap_drops_oldest() {
        let mut t = CallstackTracker::default();
        for i in 0..(MAX_DEPTH + 10) {
            t.push(StackFrame {
                kind: FrameKind::Subroutine,
                target: i as u16,
                return_addr: 0,
            });
        }
        assert_eq!(t.frames().len(), MAX_DEPTH);
        // The oldest (0) was dropped; the most-recent is preserved.
        assert_eq!(t.frames().last().unwrap().target, (MAX_DEPTH + 9) as u16);
    }

    #[test]
    fn evaluate_step_over_returns_at_depth() {
        let mut t = CallstackTracker::default();
        // Simulate being one level deep, then request step-over at that depth.
        t.push(StackFrame {
            kind: FrameKind::Subroutine,
            target: 0x9000,
            return_addr: 0x8003,
        });
        t.request_step(StepRequest::Over);
        // A non-JSR instruction at the request depth is a single step → done.
        t.evaluate_pending(0xEA);
        assert!(!t.step_pending());
        assert!(t.satisfied);
    }

    #[test]
    fn evaluate_step_out_waits_for_pop() {
        let mut t = CallstackTracker::default();
        t.push(StackFrame {
            kind: FrameKind::Subroutine,
            target: 0x9000,
            return_addr: 0x8003,
        });
        t.request_step(StepRequest::Out);
        // Still at the request depth → not done.
        t.evaluate_pending(0xEA);
        assert!(t.step_pending(), "step-out waits until the frame pops");
        // Pop below the request depth → done.
        t.stack.pop_back();
        t.evaluate_pending(0x60);
        assert!(!t.step_pending());
        assert!(t.satisfied);
    }
}
