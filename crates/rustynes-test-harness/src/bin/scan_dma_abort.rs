#![allow(
    clippy::redundant_closure,
    clippy::redundant_closure_for_method_calls,
    clippy::indexing_slicing,
    clippy::print_literal,
    clippy::uninlined_format_args,
    clippy::too_many_lines,
    clippy::items_after_statements,
    clippy::cast_possible_truncation,
    clippy::cognitive_complexity
)]
//! Probe binary: scan the full `AccuracyCoin.nes` battery and record
//! which frame each of the DMA-test result addresses gets set, so the
//! DMC-trace fixture can target a tight cycle window for cascade-
//! sentinel surfaces.
//!
//! No-features fast-path — does not enable any trace, just runs the
//! emulator and reads RAM each frame.
//!
//! Usage:
//!   `cargo run -p rustynes-test-harness --release --bin scan_dma_abort
//!     -- <rom.nes> <max-frames>`

use std::env;
use std::fs;
use std::process::ExitCode;

use rustynes_core::{Buttons, Nes};

const TARGETS: &[(&str, u16)] = &[
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

fn main() -> ExitCode {
    let args: Vec<String> = env::args().collect();
    if args.len() != 3 {
        eprintln!("usage: {} <rom.nes> <max-frames>", args[0]);
        return ExitCode::from(2);
    }
    let rom_path = &args[1];
    let max_frames: u64 = args[2].parse().expect("parse max-frames");

    // Φ-2 V-axis: set the cold-boot extra master-clocks BEFORE from_rom (reset reads
    // COLDBOOT_EXTRA_MC) to sweep the CycleCount parity vs CheckDMATiming Y.

    if let Ok(v) = env::var("RUSTYNES_SUBPOS_DELAY")
        && let Ok(n) = v.parse::<i32>()
    {
        rustynes_core::rustynes_apu::SUBPOS_DELAY.store(n, core::sync::atomic::Ordering::Relaxed);
        println!("  SUBPOS_DELAY = {n}");
    }

    if let Ok(v) = env::var("RUSTYNES_REENABLE_BUMP")
        && let Ok(n) = v.parse::<i32>()
    {
        rustynes_core::rustynes_apu::REENABLE_BUMP.store(n, core::sync::atomic::Ordering::Relaxed);
        println!("  REENABLE_BUMP = {n}");
    }

    // W2 ($2007 Stress): PPU-dot countdown from the $2007 read to the PPUDATA
    // state machine's data_buffer reload (TriCNES latch cascade; default 6).
    if let Ok(v) = env::var("RUSTYNES_2007_DELAY")
        && let Ok(n) = v.parse::<u32>()
    {
        rustynes_core::rustynes_ppu::read2007_diag::RENDER_BUFFER_DOT_DELAY
            .store(n, core::sync::atomic::Ordering::Relaxed);
        println!("  RENDER_BUFFER_DOT_DELAY = {n}");
    }

    // W2 sub-knob: 1 (default) = defer the $2007 v-glitch increment to the
    // TStep landing dot; 0 = legacy immediate increment at read time.
    if let Ok(v) = env::var("RUSTYNES_2007_VINC")
        && let Ok(n) = v.parse::<u32>()
    {
        rustynes_core::rustynes_ppu::read2007_diag::RENDER_BUFFER_DEFER_V_INC
            .store(n, core::sync::atomic::Ordering::Relaxed);
        println!("  RENDER_BUFFER_DEFER_V_INC = {n}");
    }

    let bytes = fs::read(rom_path).expect("read ROM");
    let mut nes = Nes::from_rom(&bytes).expect("parse ROM");

    // Boot + press START to launch the battery (matches the
    // accuracy_coin::run_battery_capturing_ram protocol).
    for _ in 0..300 {
        nes.run_frame();
    }
    nes.set_buttons(0, Buttons::START);
    for _ in 0..6 {
        nes.run_frame();
    }
    nes.set_buttons(0, Buttons::empty());

    let mut seen: Vec<Option<(u64, u8)>> = vec![None; TARGETS.len()];
    let mut last_seen_frame: Option<u64> = None;
    // Capture zero-page $50 (DMC+OAM's `STY <$50` = CheckDMATiming's measured Y)
    // at the frame the DMC+OAM result ($0477) is first set. The whole abort
    // cluster gates on `CheckDMATiming; CPY #4` — so this Y is the (b) target.
    let mut check_dma_timing_y: Option<u8> = None;

    // accuracycoin-100 Phase 2: capture the per-iteration abort-duration sweep
    // (CalculateDMADuration Y written to $500,X / $520,X / $540,X) at the frame
    // each abort result byte is first set, to compare against Key1/Key2/Key3.
    // Implicit/Explicit Abort gate on these sweeps, not just the $0478/$0479
    // pass byte.
    let mut sweep_implicit: Option<[u8; 16]> = None; // $500,X, Key1 00x10,01,01,00x4
    let mut sweep_explicit: Option<[u8; 16]> = None; // $520,X, Key2

    // $2007 Stress ($048E): the per-dot read-buffer data at $500-$654 (341 bytes).
    let mut stress2007: Option<[u8; 341]> = None;

    let mut prev_zp: [u8; 32] = [0u8; 32];
    for f in 306..max_frames {
        // Snapshot $50-$6F BEFORE running the frame so the DMC+OAM sweep is
        // captured uncontaminated by the next test (Explicit overwrites $50-$5F
        // within ~1 frame of $0477 being set).
        let pre_zp = prev_zp;
        nes.run_frame();
        let ram = nes.bus().ram_bytes();
        prev_zp.copy_from_slice(&ram[0x0050..0x0070]);
        if check_dma_timing_y.is_none() && ram[0x0477] != 0 {
            check_dma_timing_y = Some(ram[0x0050]);
            // DMC+OAM sweeps live in zero-page $50-$5F (Loop1) + $60-$6F (Loop2).
            // Use BOTH this-frame and previous-frame snapshots (the result byte
            // can be set the same frame the next test starts overwriting $50).
            println!("  DMCOAM $50-$5F(now)  = {:02X?}", &ram[0x0050..0x0060]);
            println!("  DMCOAM $50-$5F(prev) = {:02X?}", &pre_zp[..16]);
            println!("  DMCOAM $60-$6F(now)  = {:02X?}", &ram[0x0060..0x0070]);
            println!("  DMCOAM $60-$6F(prev) = {:02X?}", &pre_zp[16..]);
            println!("  DMCOAM KEY  $50-$5F = [04,03,04,03,04,03,02,01,02,01,02,01,02,01,02,01]");
            println!("  DMCOAM KEY  $60-$6F = [02,01,02,01,02,00,01,02,03,03,04,03,04,03,04,03]");
        }
        if stress2007.is_none() && ram[0x048E] != 0 {
            let mut s = [0u8; 341];
            s.copy_from_slice(&ram[0x0500..0x0500 + 341]);
            stress2007 = Some(s);
        }
        if sweep_implicit.is_none() && ram[0x0478] != 0 {
            let mut s = [0u8; 16];
            s.copy_from_slice(&ram[0x0500..0x0510]);
            sweep_implicit = Some(s);
            let mut s5 = [0u8; 16];
            s5.copy_from_slice(&ram[0x0520..0x0530]);
            let mut s4 = [0u8; 16];
            s4.copy_from_slice(&ram[0x0540..0x0550]);
            println!("  IMPLICIT $500 = {:02X?}", s);
            println!("  IMPLICIT $520 = {:02X?}", s5);
            println!("  IMPLICIT $540 = {:02X?}", s4);
            println!("  KEY1     $500 = [00,00,00,00,00,00,00,00,00,00,01,01,00,00,00,00]");
            println!("  KEY2     $520 = [00,00,00,00,00,00,00,00,00,00,01,00,00,00,00,00]");
            println!("  KEY3     $540 = [00,00,00,00,00,00,00,00,00,00,04,04,04,04,04,04]");
        }
        if sweep_explicit.is_none() && ram[0x0479] != 0 {
            let mut s = [0u8; 16];
            s.copy_from_slice(&ram[0x0520..0x0530]);
            sweep_explicit = Some(s);
            // Explicit Abort sweep lives in zero-page $50-$5F (Loop1).
            let mut ze = [0u8; 16];
            ze.copy_from_slice(&ram[0x0050..0x0060]);
            println!("  EXPLICIT $50-$5F = {:02X?}", ze);
            println!("  EXPLICIT KEY     = [04,04,04,04,04,04,03,04,01,01,00,00,00,00,00,00]");
        }
        for (i, (name, addr)) in TARGETS.iter().enumerate() {
            let v = ram[*addr as usize];
            if v != 0 && seen[i].is_none() {
                seen[i] = Some((f, v));
                last_seen_frame = Some(f);
                println!("  frame {f:>5}: {name} (${addr:04X}) = 0x{v:02X}");
            }
        }
        if seen.iter().all(|s| s.is_some()) {
            break;
        }
        // Early-exit only AFTER we've seen at least one address set, then
        // 6000 frames (~100 s NES time) of no further change.
        if let Some(lsf) = last_seen_frame
            && f - lsf > 6000
        {
            break;
        }
    }
    println!();

    println!(
        "CheckDMATiming Y (normal DMA, must == 4 to pass the abort gate): {}",
        check_dma_timing_y.map_or_else(|| "(unset)".to_string(), |y| format!("{y}"))
    );
    let fmt_sweep = |s: Option<[u8; 16]>| {
        s.map_or_else(
            || "(unset)".to_string(),
            |a| {
                a.iter()
                    .map(|b| format!("{b:02X}"))
                    .collect::<Vec<_>>()
                    .join(" ")
            },
        )
    };
    println!("Implicit Abort sweep $500 (Key1 = 00 00 00 00 00 00 00 00 00 00 01 01 00 00 00 00):");
    println!("  {}", fmt_sweep(sweep_implicit));
    println!("Explicit Abort sweep $520 (Key2):");
    println!("  {}", fmt_sweep(sweep_explicit));
    println!();

    println!("$2007 Stress data $500-$654 (341 bytes, for the odd-Y/key dot diagnostic):");
    match stress2007 {
        Some(d) => {
            print!("  STRESS2007=");
            for b in &d {
                print!("{b:02X}");
            }
            println!();
        }
        None => println!("  STRESS2007=(unset)"),
    }
    println!();

    println!("{:<25} {:>10} {:>6}  {}", "TEST", "FRAME", "RESULT", "ADDR");
    for (i, (name, addr)) in TARGETS.iter().enumerate() {
        // `i` ranges over `TARGETS`, and `seen` is sized `TARGETS.len()`, so
        // `get(i)` always yields `Some`; `flatten` recovers the inner option.
        match seen.get(i).copied().flatten() {
            Some((f, v)) => println!("{:<25} {:>10} 0x{:02X}  ${:04X}", name, f, v, addr),
            None => println!("{:<25} {:>10} {:>6}  ${:04X}", name, "(unset)", "-", addr),
        }
    }
    ExitCode::from(0)
}
