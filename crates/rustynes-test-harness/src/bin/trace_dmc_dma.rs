#![allow(clippy::too_many_lines, clippy::doc_markdown)]
//! Per-CPU-cycle DMC-DMA scheduler trace (Sprint 2.3 Step 3 oracle
//! generation — Path β trace-tooling foundation).
//!
//! Runs a chosen `.nes` (typically `tests/roms/AccuracyCoin/sub-tests/
//! apu-implicit-dma-abort.nes` or the full `AccuracyCoin` battery) under
//! the `irq-timing-trace` feature, then filters the per-cycle bus
//! trace to DMC-relevant events and emits a focused CSV that the
//! cross-diff tool correlates against a Mesen2 trace of the same ROM.
//!
//! # The four "compensating delays" this trace exposes
//!
//! Per Session-20's Finding 3 (`docs/audit/session-20-sprint1-dmc-abort-
//! investigation-2026-05-22.md`), the DMC scheduler has FOUR compensating
//! delays that interlock:
//!
//! - `dmc_dma_short` (1-bit; load vs. early-deliver-get path)
//! - `dmc_dma_cooldown` (post-delivery; 4 cycles after a load, 5 after
//!   an early-deliver-get)
//! - `dmc_abort_delay` (cycles-until-output → abort halt delay; 2 → 2,
//!   3 → 3, others → None)
//! - `dmc_dma_pending` + `in_dmc_dma` flag pair (scheduler state)
//!
//! Each shifts the DMA-fire cycle by ±1 relative to a canonical baseline.
//! Session-19's "naive Sprint 2.3 attempt" (cycle-2 PC dummy reads on
//! 23 implied opcodes) cascaded `Implicit DMA Abort` from PASS to FAIL
//! precisely because the four delays were tuned to the non-canonical
//! bus-quiet pre-2.3 baseline. Single-axis recalibration (this session's
//! `dmc_dma_cooldown ±1` iter 1+2) is insufficient — all four delays
//! must be recalibrated together against a Mesen2 cycle-precise oracle.
//!
//! # Output columns
//!
//! `cpu_cycle`, `ppu_frame`, `ppu_scanline`, `ppu_dot`, `m2_phase`,
//! `access`, `bus_addr`, `bus_data`, `dmc_pending_pre`, `dmc_pending_post`,
//! `dmc_dma_short`, `dmc_abort_pending`, `dmc_abort_delay`,
//! `dmc_cooldown`, `mapper_irq_low`, `apu_irq_low`.
//!
//! `m2_phase`:
//!   L = M2-low snapshot (`sub_dot` 0; immediately after CPU read enters cycle)
//!   H = M2-high snapshot (end of cycle; after PPU advances 3 dots)
//!
//! # Usage
//!
//! ```text
//! cargo run -p rustynes-test-harness --release --features irq-timing-trace \
//!   --bin trace_dmc_dma -- <rom.nes> <result-addr-hex> <max-frames> <output.csv>
//! ```
//!
//! Example:
//!
//! ```text
//! cargo run -p rustynes-test-harness --release --features irq-timing-trace \
//!   --bin trace_dmc_dma -- \
//!   tests/roms/AccuracyCoin/sub-tests/apu-reg-activation.nes \
//!   045C 200 /tmp/RustyNES_v2/dmc_rusty.csv
//! ```
//!
//! For the full AccuracyCoin battery (cascade-sentinel surface):
//!
//! ```text
//! cargo run -p rustynes-test-harness --release --features irq-timing-trace,test-roms \
//!   --bin trace_dmc_dma -- \
//!   --battery --start-frame 1500 \
//!   tests/roms/accuracycoin/AccuracyCoin.nes \
//!   0478 2000 /tmp/RustyNES_v2/dmc_acc.csv
//! ```
//!
//! `--battery` makes the binary press START at boot (matches the
//! AccuracyCoin protocol so the full battery starts running).
//!
//! `--start-frame N` runs N frames without tracing, then enables
//! tracing. Lets you target a specific test phase without filling
//! the 9M-cycle trace buffer with boot + menu.
//!
//! Then run `scripts/mesen2_dmc_dma_trace.lua` against Mesen2 on the
//! same ROM, producing `/tmp/RustyNES_v2/dmc_mesen2.csv`, and run
//! `scripts/dmc_dma_trace_cross_diff.py` to align + diff the two.

#[cfg(feature = "irq-timing-trace")]
mod inner {
    use std::env;
    use std::fmt::Write as _;
    use std::fs;
    use std::process::ExitCode;

    use rustynes_core::irq_trace::BusAccess;
    use rustynes_core::{Buttons, Nes};

    /// Filter: emit a row when ANY of these is true:
    /// - DMC DMA is pending (about to halt or actively running)
    /// - The bus access touches $4010-$4017 (DMC register window)
    /// - The bus access is a DMA fetch (`DmaRead`/`DmaWrite`)
    /// - APU IRQ status changes from one snapshot to the next
    fn is_interesting(r: &rustynes_core::irq_trace::CycleRecord, prev_apu_irq: bool) -> bool {
        if r.dmc_dma_pending_pre || r.dmc_dma_pending_post {
            return true;
        }
        if (0x4010..=0x4017).contains(&r.bus_addr) {
            return true;
        }
        if matches!(r.bus_access, BusAccess::DmaRead | BusAccess::DmaWrite) {
            return true;
        }
        let now_apu_irq = r.irq_pending_apu_at_low || r.irq_pending_apu_at_high;
        if now_apu_irq != prev_apu_irq {
            return true;
        }
        false
    }

    pub fn run() -> ExitCode {
        let raw: Vec<String> = env::args().collect();
        let mut battery = false;
        let mut start_frame: u64 = 0;
        let mut buffer_cycles: usize = 9_000_000;
        let mut positional: Vec<String> = Vec::new();
        let mut i = 1;
        while i < raw.len() {
            match raw[i].as_str() {
                "--battery" => battery = true,
                "--start-frame" => {
                    i += 1;
                    start_frame = raw
                        .get(i)
                        .and_then(|s| s.parse().ok())
                        .expect("--start-frame N");
                }
                "--buffer-cycles" => {
                    i += 1;
                    buffer_cycles = raw
                        .get(i)
                        .and_then(|s| s.parse().ok())
                        .expect("--buffer-cycles N");
                }
                other => positional.push(other.to_string()),
            }
            i += 1;
        }
        if positional.len() != 4 {
            eprintln!(
                "usage: {} [--battery] [--start-frame N] [--buffer-cycles N] \
                 <rom.nes> <result-addr-hex> <max-frames> <output.csv>",
                raw[0]
            );
            return ExitCode::from(2);
        }
        let rom_path = &positional[0];
        let addr = u16::from_str_radix(positional[1].trim_start_matches('$'), 16)
            .expect("parse hex result address");
        let max_frames: u64 = positional[2].parse().expect("parse max-frames");
        let out_path = &positional[3];

        let bytes = fs::read(rom_path).expect("read ROM");
        let mut nes = Nes::from_rom(&bytes).expect("parse ROM");

        // Boot + START strobe (matches accuracy_coin protocol).
        if battery {
            for _ in 0..300 {
                nes.run_frame();
            }
            nes.set_buttons(0, Buttons::START);
            for _ in 0..6 {
                nes.run_frame();
            }
            nes.set_buttons(0, Buttons::empty());
        }
        // Skip frames before enabling the trace buffer.
        for _ in 0..start_frame {
            nes.run_frame();
        }
        nes.bus_mut().enable_irq_trace(buffer_cycles);

        let mut first_set_frame: Option<u64> = None;
        for f in 0..max_frames {
            nes.run_frame();
            let v = nes.bus().ram_bytes()[addr as usize];
            if first_set_frame.is_none() && v != 0 {
                first_set_frame = Some(f);
                // Stop immediately on result-write -- matches the Mesen2
                // Lua's stop-on-first-result behavior so the row counts
                // are comparable across emulators.
                break;
            }
        }
        let final_val = nes.bus().ram_bytes()[addr as usize];
        let trace = nes.bus_mut().take_irq_trace().expect("trace enabled above");
        let records = trace.records();

        let mut out = String::new();
        out.push_str(
            "cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,m2_phase,access,bus_addr,bus_data,\
             dmc_pending_pre,dmc_pending_post,dmc_dma_short,dmc_abort_pending,\
             dmc_abort_delay,dmc_cooldown,mapper_irq_low,apu_irq_low\n",
        );
        let mut rows = 0u64;
        let mut prev_apu_irq = false;
        for r in records {
            if !is_interesting(r, prev_apu_irq) {
                prev_apu_irq = r.irq_pending_apu_at_low || r.irq_pending_apu_at_high;
                continue;
            }
            prev_apu_irq = r.irq_pending_apu_at_low || r.irq_pending_apu_at_high;

            let m2_phase = if (r.cpu_cycle & 1) == 0 { 'L' } else { 'H' };
            let access = match r.bus_access {
                BusAccess::Read => "R",
                BusAccess::Write => "W",
                BusAccess::DmaRead => "r",
                BusAccess::DmaWrite => "w",
                BusAccess::Idle => "I",
            };
            let _ = writeln!(
                &mut out,
                "{},{},{},{},{},{},${:04X},${:02X},{},{},{},{},{},{},{},{}",
                r.cpu_cycle,
                r.ppu_frame,
                r.ppu_scanline,
                r.ppu_dot,
                m2_phase,
                access,
                r.bus_addr,
                r.bus_data,
                u8::from(r.dmc_dma_pending_pre),
                u8::from(r.dmc_dma_pending_post),
                u8::from(r.dmc_dma_short_post),
                u8::from(r.dmc_abort_pending_post),
                r.dmc_abort_delay_post,
                r.dmc_dma_cooldown_post,
                u8::from(r.irq_pending_mapper_at_low),
                u8::from(r.irq_pending_apu_at_low),
            );
            rows += 1;
        }
        fs::write(out_path, &out).expect("write output");

        println!(
            "rom={rom_path} addr=${addr:04X} final=0x{final_val:02X} \
             first_set_frame={first_set_frame:?} \
             trace_rows={rows} trace_records={} written_to={out_path}",
            records.len(),
        );
        ExitCode::from(0)
    }
}

#[cfg(feature = "irq-timing-trace")]
fn main() -> std::process::ExitCode {
    inner::run()
}

#[cfg(not(feature = "irq-timing-trace"))]
fn main() {
    eprintln!(
        "trace_dmc_dma requires the `irq-timing-trace` cargo feature. \
         Re-run with --features irq-timing-trace."
    );
    std::process::exit(2);
}
