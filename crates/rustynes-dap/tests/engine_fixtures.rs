// SPDX-License-Identifier: GPL-3.0-or-later
//! The engine against both fixtures, without a protocol in between: stop at
//! a source breakpoint, read a parameter and a local, step a line, step
//! into and out of a call, and read a global and a caller frame's local.
//!
//! The values are the C program's, so they are an oracle independent of the
//! debugger: `add3(5)` sees `v == 5`, stores `r == 8`, and the loop then
//! calls `add3(8)`.

mod common;

use common::{CC65, Fixture, LLVM_MOS};
use rustynes_core::Nes;
use rustynes_dap::debuginfo::DebugInfo;
use rustynes_dap::engine::{Engine, StepKind, Stop};

fn engine(fx: Fixture) -> Engine {
    let rom = std::fs::read(fx.rom_path()).expect("fixture ROM");
    let dbg = std::fs::read(fx.debug_path()).expect("fixture debug file");
    let info = DebugInfo::load(&dbg, &common::dir()).expect("debug info loads");
    Engine::new(Nes::from_rom(&rom).expect("ROM loads"), Some(info))
}

fn file(e: &Engine) -> usize {
    e.info()
        .and_then(|i| i.file_for_path(&common::source().to_string_lossy()))
        .expect("dapdemo.c is in the debug info")
}

/// The value text of `name` in the innermost frame (or a global).
fn var(e: &mut Engine, frame: usize, name: &str) -> String {
    let stack = e.stack();
    let f = &stack[frame];
    if let Some(v) = e.frame_vars(f).into_iter().find(|v| v.name == name) {
        return e.value(&v, f.base).text;
    }
    let g = e
        .info()
        .and_then(|i| i.global_named(name))
        .cloned()
        .unwrap_or_else(|| panic!("no variable {name}"));
    e.value(&g, None).text
}

fn first_number(text: &str) -> u64 {
    text.split_whitespace()
        .next()
        .and_then(|t| t.parse().ok())
        .unwrap_or_else(|| panic!("not a number: {text}"))
}

fn line_now(e: &mut Engine) -> u32 {
    e.stack()[0].loc.map_or(0, |l| l.line)
}

fn never() -> impl FnMut() -> bool {
    let mut frames = 0u32;
    move || {
        frames += 1;
        assert!(
            frames < 600,
            "ran ten seconds of emulated time without stopping"
        );
        false
    }
}

fn breakpoint_step_and_read(fx: Fixture) {
    let mut e = engine(fx);
    let f = file(&e);
    let body = common::line("add3-body");
    let ret = common::line("add3-return");
    let got = e.set_source_breakpoints(f, &[body]).expect("armed");
    assert_eq!(got, vec![Some(body)], "{}: the body line has code", fx.name);

    assert_eq!(e.resume(&mut never()), Stop::Breakpoint, "{}", fx.name);
    assert_eq!(
        line_now(&mut e),
        body,
        "{}: stopped on the body line",
        fx.name
    );
    let stack = e.stack();
    assert_eq!(stack[0].name, "add3", "{}", fx.name);
    assert_eq!(
        stack[1].name, "main",
        "{}: the caller frame is main",
        fx.name
    );
    assert_eq!(
        stack[1].loc.map(|l| l.line),
        Some(common::line("main-call")),
        "{}: the caller frame is on the call line",
        fx.name
    );
    assert_eq!(
        e.peek(stack[1].pc),
        0x20,
        "{}: the caller's pc is its JSR",
        fx.name
    );
    assert_eq!(first_number(&var(&mut e, 0, "v")), 5, "{}: v", fx.name);
    assert_eq!(
        first_number(&var(&mut e, 1, "local")),
        5,
        "{}: main's local",
        fx.name
    );

    assert_eq!(
        e.step_line(StepKind::Over, &mut never()),
        Stop::Step,
        "{}",
        fx.name
    );
    assert_eq!(
        line_now(&mut e),
        ret,
        "{}: next goes to the return line",
        fx.name
    );
    assert_eq!(
        first_number(&var(&mut e, 0, "r")),
        8,
        "{}: r = v + 3",
        fx.name
    );

    assert_eq!(e.resume(&mut never()), Stop::Breakpoint, "{}", fx.name);
    assert_eq!(
        first_number(&var(&mut e, 0, "v")),
        8,
        "{}: second call",
        fx.name
    );
    assert_eq!(
        first_number(&var(&mut e, 0, "counter")),
        8,
        "{}: the global",
        fx.name
    );
}

fn step_in_and_out(fx: Fixture) {
    let mut e = engine(fx);
    let f = file(&e);
    let call = common::line("main-call");
    let store = common::line("main-store");
    let body = common::line("add3-body");
    e.set_source_breakpoints(f, &[call]).expect("armed");
    assert_eq!(e.resume(&mut never()), Stop::Breakpoint, "{}", fx.name);
    assert_eq!(line_now(&mut e), call, "{}", fx.name);
    e.set_source_breakpoints(f, &[]).expect("cleared");

    assert_eq!(
        e.step_line(StepKind::In, &mut never()),
        Stop::Step,
        "{}",
        fx.name
    );
    assert_eq!(
        line_now(&mut e),
        body,
        "{}: stepIn lands on add3's first body line",
        fx.name
    );
    assert_eq!(
        first_number(&var(&mut e, 0, "v")),
        5,
        "{}: v after stepIn",
        fx.name
    );

    assert_eq!(
        e.step_line(StepKind::Out, &mut never()),
        Stop::Step,
        "{}",
        fx.name
    );
    assert_eq!(
        e.stack()[0].name,
        "main",
        "{}: stepOut returns to main",
        fx.name
    );
    let l = line_now(&mut e);
    assert!(
        l == call || l == store,
        "{}: back in main on line {l}",
        fx.name
    );

    // From the call line, `next` steps over add3 to the store.
    let mut e = engine(fx);
    e.set_source_breakpoints(f, &[call]).expect("armed");
    assert_eq!(e.resume(&mut never()), Stop::Breakpoint, "{}", fx.name);
    e.set_source_breakpoints(f, &[]).expect("cleared");
    assert_eq!(
        e.step_line(StepKind::Over, &mut never()),
        Stop::Step,
        "{}",
        fx.name
    );
    assert_eq!(
        line_now(&mut e),
        store,
        "{}: next steps over the call",
        fx.name
    );
    assert_eq!(
        first_number(&var(&mut e, 0, "local")),
        8,
        "{}: local = add3(5)",
        fx.name
    );
}

#[test]
fn cc65_breakpoint_step_and_read() {
    breakpoint_step_and_read(CC65);
}

#[test]
fn llvm_mos_breakpoint_step_and_read() {
    breakpoint_step_and_read(LLVM_MOS);
}

#[test]
fn cc65_step_in_and_out() {
    step_in_and_out(CC65);
}

#[test]
fn llvm_mos_step_in_and_out() {
    step_in_and_out(LLVM_MOS);
}

#[test]
fn a_breakpoint_on_a_line_without_code_moves_forward() {
    for fx in [CC65, LLVM_MOS] {
        let mut e = engine(fx);
        let f = file(&e);
        // The line before the body is the declaration `unsigned char r;`.
        let decl = common::line("add3-body") - 1;
        let got = e.set_source_breakpoints(f, &[decl]).expect("armed");
        assert_eq!(got, vec![Some(common::line("add3-body"))], "{}", fx.name);
    }
}

#[test]
fn the_stop_does_not_perturb_the_machine() {
    // Run to the second hit of the body line through the engine (source
    // breakpoints, every frame hook, the step off the first stop) and through
    // the bare core with one exec breakpoint and nothing else: the two
    // machines must agree byte for byte. The hooks and the bookkeeping add no
    // cycle and change no state.
    for fx in [CC65, LLVM_MOS] {
        let mut e = engine(fx);
        let f = file(&e);
        e.set_source_breakpoints(f, &[common::line("add3-body")])
            .expect("armed");
        let addr = *e.user_breakpoints().iter().next().expect("one address");
        assert_eq!(e.resume(&mut never()), Stop::Breakpoint);
        assert_eq!(e.resume(&mut never()), Stop::Breakpoint);
        // Everything a client can look at, including the PPU and APU
        // registers (a real `$2002` read would clear VBlank): reads are
        // side-effect free.
        let _ = e.read(0x0000, 0x1_0000);
        let stack = e.stack();
        for f in &stack {
            for v in e.frame_vars(f) {
                let _ = e.value(&v, f.base);
            }
        }

        let rom = std::fs::read(fx.rom_path()).expect("ROM");
        let mut plain = Nes::from_rom(&rom).expect("ROM loads");
        plain.add_breakpoint(addr);
        let mut hits = 0;
        for _ in 0..600 {
            plain.run_frame();
            if plain.take_break_hit().is_some() {
                hits += 1;
                if hits == 2 {
                    break;
                }
            }
        }
        assert_eq!(hits, 2, "{}: the bare core hits twice", fx.name);
        assert_eq!(
            plain.cpu().cycles,
            e.nes().cpu().cycles,
            "{}: same cycle",
            fx.name
        );
        assert!(
            plain.snapshot() == e.nes().snapshot(),
            "{}: identical state",
            fx.name
        );
    }
}
