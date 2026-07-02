//! Vs. `DualSystem` handshake trace probe (v2.0.0 beta.5 diagnostic).
//!
//! Boots a `DualSystem` dump through `VsDualSystem`, runs N frames, and
//! samples both CPUs' PCs at a fixed cadence — a PC histogram per console
//! shows exactly where each program is spinning when the inter-CPU
//! handshake deadlocks. NOT part of CI; diagnostic tool only.
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features commercial-roms \
//!     --bin vs_dual_trace -- <rom-path> [frames]
//! ```

use std::collections::HashMap;

use rustynes_core::VsDualSystem;

// Diagnostic tool: the trace phases (frame loop, dense instruction trace,
// histogram report) read best as one linear script; splitting them would
// thread a dozen locals through helper signatures for no clarity gain.
#[allow(clippy::too_many_lines)]
fn main() {
    let mut args = std::env::args().skip(1);
    let rom = args.next().expect("usage: vs_dual_trace <rom> [frames]");
    let frames: u64 = args.next().map_or(120, |f| f.parse().expect("frames"));

    let bytes = std::fs::read(&rom).expect("read rom");
    let mut dual = VsDualSystem::from_rom(&bytes).expect("dual construct");

    let mut main_pcs: HashMap<u16, u64> = HashMap::new();
    let mut sub_pcs: HashMap<u16, u64> = HashMap::new();

    let colours = |fb: &[u8]| {
        fb.chunks_exact(4)
            .map(|c| [c[0], c[1], c[2], c[3]])
            .collect::<std::collections::HashSet<_>>()
            .len()
    };

    for f in 0..frames {
        if f % 120 == 30 {
            dual.insert_coin(0);
            dual.insert_coin(2);
        }
        if f % 120 == 34 {
            dual.clear_coin();
        }
        dual.run_frame();
        *main_pcs.entry(dual.main().cpu().pc).or_insert(0) += 1;
        *sub_pcs.entry(dual.sub().cpu().pc).or_insert(0) += 1;
        if f % 300 == 299 {
            println!(
                "f={f}: main colours={} sub colours={}",
                colours(dual.main_framebuffer()),
                colours(dual.sub_framebuffer())
            );
        }
        if f < 8 || f % 60 == 0 {
            // Handshake mailbox: $6220 as seen from each side + $6200 row.
            let m6220 = dual.main_mut().bus_mut().debug_peek_cpu(0x6220);
            let s6220 = dual.sub_mut().bus_mut().debug_peek_cpu(0x6220);
            let m6200 = dual.main_mut().bus_mut().debug_peek_cpu(0x6200);
            let s6200 = dual.sub_mut().bus_mut().debug_peek_cpu(0x6200);
            println!(
                "f={f}: $6220 main={m6220:02X} sub={s6220:02X}  $6200 main={m6200:02X} sub={s6200:02X}"
            );
        }
    }

    // Dense trace: replicate the wrapper's lockstep + pump manually for a
    // few hundred instructions and log both CPUs' PC streams (compressed to
    // transitions), plus every comms level and WRAM write observed.
    println!("== dense instruction trace ==");
    let mut last_main = 0u16;
    let mut last_sub = 0u16;
    let pump = |main: &mut rustynes_core::Nes, sub: &mut rustynes_core::Nes| {
        if let Some(level) = main.bus_mut().take_vs_mainsub_edge() {
            println!("  MAIN bit1={}", u8::from(level));
            sub.bus_mut().set_vs_external_irq(!level);
        }
        if let Some(level) = sub.bus_mut().take_vs_mainsub_edge() {
            println!("  SUB  bit1={}", u8::from(level));
            main.bus_mut().set_vs_external_irq(!level);
        }
        for (off, val) in main.bus_mut().take_vs_dual_wram_writes() {
            println!("  MAIN wram[{off:03X}]={val:02X}");
            sub.bus_mut().apply_vs_dual_wram_write(off, val);
        }
        for (off, val) in sub.bus_mut().take_vs_dual_wram_writes() {
            println!("  SUB  wram[{off:03X}]={val:02X}");
            main.bus_mut().apply_vs_dual_wram_write(off, val);
        }
    };
    {
        let (main, sub) = dual.split_mut();
        for _ in 0..400 {
            main.step_instruction();
            pump(main, sub);
            let mpc = main.cpu().pc;
            if mpc != last_main {
                print!("M:{mpc:04X} ");
                last_main = mpc;
            }
            while !sub.is_jammed()
                && (main.cycle() > sub.cycle().saturating_add(5) || main.frame() > sub.frame())
            {
                sub.step_instruction();
                pump(main, sub);
                let spc = sub.cpu().pc;
                if spc != last_sub {
                    print!("s:{spc:04X} ");
                    last_sub = spc;
                }
            }
        }
        println!();
    }

    let top = |m: &HashMap<u16, u64>| {
        let mut v: Vec<_> = m.iter().map(|(pc, n)| (*pc, *n)).collect();
        v.sort_by_key(|&(_, n)| std::cmp::Reverse(n));
        v.truncate(8);
        v
    };
    println!("== after {frames} frames ==");
    println!(
        "main: frame={} cycle={} jammed={}",
        dual.main().frame(),
        dual.main().cycle(),
        dual.main().is_jammed()
    );
    println!(
        "sub:  frame={} cycle={} jammed={}",
        dual.sub().frame(),
        dual.sub().cycle(),
        dual.sub().is_jammed()
    );
    println!(
        "main top PCs (sampled at frame edges): {:04X?}",
        top(&main_pcs)
    );
    println!(
        "sub  top PCs (sampled at frame edges): {:04X?}",
        top(&sub_pcs)
    );
}
