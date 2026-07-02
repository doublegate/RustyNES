//! v2.0.0 beta.3 (A4 scoping) — warm-reset second-pass tracer.
//!
//! Runs a blargg `apu_reset`-protocol ROM to its first `$81`
//! ("press RESET") status, issues `Nes::reset()`, then dumps per-frame:
//! the CPU PC, the `$6000` status byte, and the head of the `$6004` text
//! buffer — so a wedged / crashed / looping second pass is directly
//! observable instead of inferred from stale RAM.
//!
//! Usage:
//!   `cargo run -p rustynes-test-harness --release --features test-roms \
//!      --bin reset_trace -- <rom.nes> [frames-after-reset]`

use std::env;
use std::fs;
use std::process::ExitCode;

use rustynes_core::Nes;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() < 2 {
        eprintln!("usage: {} <rom.nes> [frames-after-reset]", args[0]);
        return ExitCode::FAILURE;
    }
    let rom_path = &args[1];
    let after: u64 = args.get(2).and_then(|s| s.parse().ok()).unwrap_or(40);

    let bytes = fs::read(rom_path).expect("read ROM");
    let mut nes = Nes::from_rom(&bytes).expect("parse ROM");

    // Run to the first $81 (max 200 frames).
    let mut reached = false;
    for f in 0..200u64 {
        nes.run_frame();
        let status = nes.bus_mut().peek_cpu(0x6000);
        if status == 0x81 {
            println!("frame {f}: status=0x81 (press RESET) — issuing Nes::reset()");
            reached = true;
            break;
        }
    }
    if !reached {
        eprintln!("never reached $81");
        return ExitCode::FAILURE;
    }

    // Hold briefly like the harness, then reset.
    for _ in 0..6 {
        nes.run_frame();
    }
    nes.reset();

    // Trace the second pass.
    for f in 0..after {
        nes.run_frame();
        let pc = nes.cpu().pc;
        let status = nes.bus_mut().peek_cpu(0x6000);
        let mut text = [0u8; 24];
        for (i, slot) in text.iter_mut().enumerate() {
            *slot = nes
                .bus_mut()
                .peek_cpu(0x6004 + u16::try_from(i).unwrap_or(0));
        }
        let text_str: String = text
            .iter()
            .take_while(|&&b| b != 0)
            .map(|&b| {
                if b.is_ascii_graphic() || b == b' ' {
                    b as char
                } else {
                    '.'
                }
            })
            .collect();
        println!("+{f:3}: PC={pc:04X} status={status:02X} text={text_str:?}");
    }
    ExitCode::SUCCESS
}
