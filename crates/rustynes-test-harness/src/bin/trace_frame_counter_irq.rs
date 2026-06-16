//! Per-CPU-cycle frame-counter-IRQ trace (Sprint 2 iteration 3 oracle generation).
//!
//! Runs `tests/roms/AccuracyCoin/sub-tests/frame-counter-irq.nes` under
//! the `irq-timing-trace` feature, then filters the per-cycle bus
//! trace to `$4015` reads/writes + `$4017` writes and emits a focused
//! CSV that the cross-diff tool can correlate against a Mesen2 trace
//! of the same ROM.
//!
//! Output columns:
//!   `cpu_cycle`, `ppu_frame`, `ppu_scanline`, `ppu_dot`, `m2_phase`,
//!   access, `bus_addr`, `bus_data`, `irq_pending`.
//!
//! `m2_phase`:
//!   L = `$4015`/`$4017` access observed at the M2-low snapshot of the cycle
//!   H = `$4015`/`$4017` access observed at the M2-high snapshot (end of cycle)
//!
//! `irq_pending` is the bus-data bit 6 for `$4015` READS (the
//! frame-counter IRQ flag exposed on the bus) and -1 for writes
//! / `$4017` reads (which never carry the FC IRQ bit).
//!
//! See `docs/audit/session-25-sprint2-iter3-frame-counter-irq-2026-05-23.md`.
//!
//! USAGE:
//!   `cargo run -p rustynes-test-harness --release --features irq-timing-trace
//!     --bin trace_frame_counter_irq --
//!     <rom.nes> <result-addr-hex> <max-frames> <output.csv>`

#[cfg(feature = "irq-timing-trace")]
mod inner {
    use std::env;
    use std::fmt::Write as _;
    use std::fs;
    use std::process::ExitCode;

    use rustynes_core::Nes;
    use rustynes_core::irq_trace::BusAccess;

    pub fn run() -> ExitCode {
        let args: Vec<String> = env::args().collect();
        if args.len() != 5 {
            eprintln!(
                "usage: {} <rom.nes> <result-addr-hex> <max-frames> <output.csv>",
                args[0]
            );
            return ExitCode::from(2);
        }
        let rom_path = &args[1];
        let addr = u16::from_str_radix(args[2].trim_start_matches('$'), 16)
            .expect("parse hex result address");
        let max_frames: u64 = args[3].parse().expect("parse max-frames");
        let out_path = &args[4];

        let bytes = fs::read(rom_path).expect("read ROM");
        let mut nes = Nes::from_rom(&bytes).expect("parse ROM");

        nes.bus_mut().enable_irq_trace(3_500_000);

        let mut first_set_frame: Option<u64> = None;
        let mut stable_frames: u64 = 0;
        let mut last_val: u8 = 0;
        for f in 0..max_frames {
            nes.run_frame();
            let v = nes.bus().ram_bytes()[addr as usize];
            if first_set_frame.is_none() && v != 0 {
                first_set_frame = Some(f);
                last_val = v;
            }
            if first_set_frame.is_some() {
                if v == last_val {
                    stable_frames += 1;
                    if stable_frames >= 60 {
                        break;
                    }
                } else {
                    stable_frames = 0;
                    last_val = v;
                }
            }
        }
        let final_val = nes.bus().ram_bytes()[addr as usize];
        let trace = nes.bus_mut().take_irq_trace().expect("trace enabled above");
        let records = trace.records();

        let mut out = String::new();
        out.push_str(
            "cpu_cycle,ppu_frame,ppu_scanline,ppu_dot,m2_phase,access,bus_addr,bus_data,irq_pending\n",
        );
        let mut rows = 0u64;
        for r in records {
            // Capture $4015 reads/writes, $4017 writes, the result-addr
            // write, and any ErrorCode ($0010) write so we can trace the
            // FAIL -> result pipeline end-to-end.
            let interesting = r.bus_addr == 0x4015
                || (r.bus_addr == 0x4017 && matches!(r.bus_access, BusAccess::Write))
                || (r.bus_addr == addr && matches!(r.bus_access, BusAccess::Write))
                || (r.bus_addr == 0x0010 && matches!(r.bus_access, BusAccess::Write));
            if !interesting {
                continue;
            }
            // The bus_access record reflects end-of-cycle (M2-high) state.
            // M2 phase derives from cycle parity (per Mesen2 master-clock
            // convention `GetMasterClock() & 0x01`).
            let m2_phase = if (r.cpu_cycle & 1) == 0 { 'L' } else { 'H' };
            let access = match r.bus_access {
                BusAccess::Read => "R",
                BusAccess::Write => "W",
                BusAccess::DmaRead => "r",
                BusAccess::DmaWrite => "w",
                BusAccess::Idle => "I",
            };
            let irq_pending: i32 =
                if r.bus_addr == 0x4015 && matches!(r.bus_access, BusAccess::Read) {
                    i32::from((r.bus_data & 0x40) != 0)
                } else {
                    -1
                };
            let _ = writeln!(
                &mut out,
                "{},{},{},{},{},{},${:04X},${:02X},{}",
                r.cpu_cycle,
                r.ppu_frame,
                r.ppu_scanline,
                r.ppu_dot,
                m2_phase,
                access,
                r.bus_addr,
                r.bus_data,
                irq_pending,
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
        "trace_frame_counter_irq requires the `irq-timing-trace` cargo \
         feature. Re-run with --features irq-timing-trace."
    );
    std::process::exit(2);
}
