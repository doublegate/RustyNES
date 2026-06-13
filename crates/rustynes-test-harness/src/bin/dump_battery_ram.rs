#![allow(
    clippy::map_unwrap_or,
    clippy::indexing_slicing,
    clippy::uninlined_format_args,
    clippy::cast_possible_wrap
)]
//! Dump post-battery RAM bytes for the DMA-test address range, so we
//! can correlate the upstream catalog addresses with what actually
//! gets set in our emulator.

use std::env;
use std::process::ExitCode;

use rustynes_test_harness::accuracy_coin;

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    let max_frames: u64 = args
        .get(1)
        .map(|s| s.parse().expect("parse max-frames"))
        .unwrap_or(72_000);

    // W2 ($2007 Stress): PPU-dot countdown from the $2007 read to the PPUDATA
    // state machine's data_buffer reload (default 6).
    if let Ok(v) = std::env::var("RUSTYNES_2007_DELAY") {
        if let Ok(n) = v.parse::<u32>() {
            rustynes_core::rustynes_ppu::read2007_diag::RENDER_BUFFER_DOT_DELAY
                .store(n, core::sync::atomic::Ordering::Relaxed);
        }
    }
    // W2 sub-knob: 1 (default) = defer the $2007 v-glitch increment to the
    // TStep landing dot; 0 = legacy immediate increment at read time.
    if let Ok(v) = std::env::var("RUSTYNES_2007_VINC") {
        if let Ok(n) = v.parse::<u32>() {
            rustynes_core::rustynes_ppu::read2007_diag::RENDER_BUFFER_DEFER_V_INC
                .store(n, core::sync::atomic::Ordering::Relaxed);
        }
    }
    let (result, ram) = accuracy_coin::run_battery_capturing_ram(max_frames);
    println!(
        "battery: pass={} fail={} partial={} not_run={} other={}",
        result.pass, result.fail, result.partial, result.not_run, result.other,
    );
    println!("---");

    let targets: &[(&str, u16)] = &[
        ("DMA + Open Bus", 0x046C),
        ("DMA + $2002 Read", 0x0488),
        ("DMA + $2007 Read", 0x044C),
        ("DMA + $2007 Write", 0x044F),
        ("DMA + $4015 Read", 0x045D),
        ("DMA + $4016 Read", 0x045E),
        ("DMC DMA Bus Conflicts", 0x046B),
        ("DMC DMA + OAM DMA", 0x0477),
        ("Explicit DMA Abort", 0x0479),
        ("Implicit DMA Abort", 0x0478),
        ("Implied Dummy Reads", 0x046D),
        ("APU Reg Activation", 0x045C),
    ];

    for (name, addr) in targets {
        let v = ram[*addr as usize];
        println!("  ${addr:04X} = 0x{v:02X}  {name}");
    }
    // $2007 Stress: the result byte + the per-dot $500 array (340 raw, the
    // odd-Y stable bytes are the 170 compared against TEST_2007StressTest_Key).
    println!("---");
    println!(
        "  $048E = 0x{:02X}  $2007 Stress Test (result)",
        ram[0x048E]
    );
    println!("  $500 array (raw, 64 of 340):");
    for row in 0..8 {
        let base = 0x0500 + row * 8;
        let bytes: Vec<String> = (0..8).map(|i| format!("{:02X}", ram[base + i])).collect();
        println!("    ${base:04X}: {}", bytes.join(" "));
    }
    // $2007 read-landing diagnostic (gated mc-ppu-2007-render-buffer).
    {
        use core::sync::atomic::Ordering::Relaxed;
        use rustynes_core::rustynes_ppu::read2007_diag;
        let n = (read2007_diag::IDX.load(Relaxed) as usize).min(1024);
        println!("--- $2007 reads landing (n={n}): scanline,dot,render_en,is_render ---");
        for i in 0..n.min(40) {
            let p = read2007_diag::LOG[i].load(Relaxed);
            let sl = ((p >> 18) & 0x1FF) as i32 - 1;
            let dot = (p >> 5) & 0x1FFF;
            let ren = (p >> 1) & 1;
            let isr = p & 1;
            println!("  [{i:3}] scanline={sl:>4} dot={dot:>3} render_en={ren} is_render={isr}");
        }
    }
    ExitCode::from(0)
}
