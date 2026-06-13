#![allow(
    clippy::items_after_statements,
    clippy::too_many_lines,
    clippy::uninlined_format_args,
    clippy::cast_possible_truncation,
    clippy::cognitive_complexity,
    clippy::map_unwrap_or
)]
//! Correlated default-vs-R1 per-cycle DMC-DMA trace for the `AccuracyCoin`
//! `APU Registers and DMA` cluster regressions (R1 substrate, session 2).
//! Targets any result byte: `$045D` DMA + $4015 Read, `$0479` Explicit DMA
//! Abort, `$0478` Implicit DMA Abort.
//!
//! Built BOTH ways and diffed:
//!   default: `cargo run -p rustynes-test-harness --release --features irq-timing-trace --bin trace_dma_4015`
//!   R1:      `... --features irq-timing-trace,mc-r1-substrate --bin trace_dma_4015`
//!
//! Findings (see `docs/audit/v2.0-interleaved-dma-rewrite-plan-2026-06-03.md`):
//!   * DMA + $4015: under R1 the test-positioned DMC DMA does NOT land at the
//!     `BIT $4015` cycle (no `dmc_dma_cycle` in the window) — the BIT runs
//!     un-delayed and reads the frame-IRQ flag still SET. Refutes the earlier
//!     "frame-counter-vs-$4015-replay ordering" hypothesis.
//!   * Explicit/Implicit DMA Abort: the DMC DMA SPAN histogram is uniformly 1
//!     cycle SHORTER under R1 (default normal span = 4 cyc; R1 = 3). Since the
//!     abort tests measure DMA *duration*, the short span -> wrong durations ->
//!     `0x06`. Points at `dmc_dma_step_impl` modeling halt-only instead of the
//!     Mesen `_needHalt` + `_needDummyRead` two-cycle preamble.
//!
//! The bin emits, around each DMC DMA span + `STA $4015` abort write, a CSV
//! context window (blank-line-separated per disjoint window). Columns:
//! `cpu_cycle,frame,scanline,dot,access,addr,data,apu_irq_low,apu_irq_high,
//! in_dmc,dmc_pending,apu_phase,note`. Post-process span histograms in Python
//! over the `in_dmc` column.
//!
//! USAGE:
//!   `cargo run -p rustynes-test-harness --release --features irq-timing-trace
//!     [,mc-r1-substrate] --bin trace_dma_4015 --
//!     <rom.nes> <result-addr-hex> <max-frames> <ring-capacity> <output.csv>`

#[cfg(feature = "irq-timing-trace")]
mod inner {
    use std::env;
    use std::fmt::Write as _;
    use std::fs;
    use std::process::ExitCode;

    use rustynes_core::irq_trace::BusAccess;
    use rustynes_core::{Buttons, Nes};

    // AccuracyCoin result bytes (SOURCE_CATALOG.tsv):
    //   $045D DMA + $4015 Read | $0479 Explicit DMA Abort | $0478 Implicit DMA Abort.

    pub fn run() -> ExitCode {
        let args: Vec<String> = env::args().collect();
        if args.len() != 6 {
            eprintln!(
                "usage: {} <rom.nes> <result-addr-hex> <max-frames> <ring-capacity> <output.csv>",
                args[0]
            );
            return ExitCode::from(2);
        }
        let rom_path = &args[1];
        let result_addr = u16::from_str_radix(args[2].trim_start_matches('$'), 16)
            .expect("parse result-addr hex");
        let max_frames: u64 = args[3].parse().expect("parse max-frames");
        let ring_capacity: usize = args[4].parse().expect("parse ring-capacity");
        let out_path = &args[5];

        let bytes = fs::read(rom_path).expect("read ROM");

        // `IrqTrace::push` is a BOUNDED buffer (keeps the FIRST `capacity`
        // records, drops the rest) — NOT a ring. So we must enable it right
        // before the result-set window. Two deterministic passes (same seed):
        //   pass 1: find the frame where `result_addr` is first written;
        //   pass 2: fresh ROM, enable the trace `MARGIN` frames before that.
        // `MARGIN` = frames before the result-set frame to begin tracing.
        // Default 12 (the tight result window); override via
        // `RUSTYNES_TRACE_MARGIN` to capture the abort-test ENTRY (the first
        // DMASync spin), which lands well before the result write.
        let margin: u64 = std::env::var("RUSTYNES_TRACE_MARGIN")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(12);

        let boot = |bytes: &[u8]| {
            let mut nes = Nes::from_rom(bytes).expect("parse ROM");
            for _ in 0..300 {
                nes.run_frame();
            }
            nes.set_buttons(0, Buttons::START);
            for _ in 0..6 {
                nes.run_frame();
            }
            nes.set_buttons(0, Buttons::empty());
            nes
        };

        // Pass 1 — locate the result-set frame.
        let mut nes = boot(&bytes);
        let mut set_frame: Option<u64> = None;
        for f in 306..max_frames {
            nes.run_frame();
            if nes.bus().ram_bytes()[result_addr as usize] != 0 {
                set_frame = Some(f);
                break;
            }
        }
        // BP/wedge diagnostic: if the result is never set (the DMASync wedge
        // hangs the battery), still capture the spin by tracing the LAST `margin`
        // frames of the run instead of panicking.
        let set_frame = set_frame.unwrap_or_else(|| {
            eprintln!("(result never set — WEDGE; tracing the spin at end-of-run)");
            max_frames.saturating_sub(2)
        });
        let trace_start = set_frame.saturating_sub(margin);

        // Pass 2 — re-run, enabling the trace at `trace_start`.
        let mut nes = boot(&bytes);
        for f in 306..trace_start {
            nes.run_frame();
            let _ = f;
        }
        nes.bus_mut().enable_irq_trace(ring_capacity);
        for _ in trace_start..=(set_frame + 2) {
            nes.run_frame();
        }

        let final_val = nes.bus().ram_bytes()[result_addr as usize];
        let trace = nes.bus_mut().take_irq_trace().expect("trace enabled above");
        let records = trace.records();

        // Anchor on each DMC DMA span (`in_dmc_dma`) and each `STA $4015` abort
        // write — the events these tests measure (DMA duration / abort). The
        // CTX window then captures any nearby `BIT $4015` read too. Anchoring on
        // R $4015 instead would explode on DMASync's `LDA $4015` spin loop.
        const CTX: u64 = 24;
        let anchors: Vec<u64> = records
            .iter()
            .filter(|r| {
                r.in_dmc_dma || (r.bus_addr == 0x4015 && matches!(r.bus_access, BusAccess::Write))
            })
            .map(|r| r.cpu_cycle)
            .collect();
        // DC-0 full-dump mode (`RUSTYNES_FULL_RANGE="lo,hi"`): bypass the
        // ±CTX-around-DMC disjoint windows and emit EVERY record in the
        // contiguous cpu_cycle range [lo,hi], so the CalculateDMADuration
        // 575-cycle walk (whose `$4000` reads are far from any DMC span) is
        // fully visible — to verify the loop period vs Mesen.
        let full_range: Option<(u64, u64)> =
            std::env::var("RUSTYNES_FULL_RANGE").ok().and_then(|s| {
                let mut it = s.split(',');
                Some((
                    it.next()?.trim().parse().ok()?,
                    it.next()?.trim().parse().ok()?,
                ))
            });
        let in_window = |c: u64| {
            if let Some((lo, hi)) = full_range {
                return c >= lo && c <= hi;
            }
            anchors
                .iter()
                .any(|&a| c >= a.saturating_sub(CTX) && c <= a + CTX)
        };

        // Abort-setup cross-diff mode (`RUSTYNES_ABORT_SETUP=1`): instead of the
        // windowed view, emit the per-iteration setup landmarks across the whole
        // trace — `$4010-$4013` writes (DMC rate/sample setup), `$4015` writes,
        // `$4000-$401F` reads (the DMASync spin + DMA conflict), and DMC-DMA
        // starts — so default vs R1 can be aligned on the `$4010` rate-change.
        let setup_mode = std::env::var("RUSTYNES_ABORT_SETUP").is_ok();
        let setup_interesting = |r: &rustynes_core::irq_trace::CycleRecord| -> bool {
            let dma_start = r.in_dmc_dma; // first/all DMA cycles
            let w4010 =
                (0x4010..=0x4013).contains(&r.bus_addr) && matches!(r.bus_access, BusAccess::Write);
            let w4015 = r.bus_addr == 0x4015 && matches!(r.bus_access, BusAccess::Write);
            let r400x = (0x4000..=0x401F).contains(&r.bus_addr)
                && matches!(r.bus_access, BusAccess::Read | BusAccess::DmaRead);
            dma_start || w4010 || w4015 || r400x
        };

        // === ANCHOR MODE (TriCNES cross-diff) ===
        // `RUSTYNES_ANCHOR="<addr_hex>[,<pre>,<post>,<occurrence>]"` emits a
        // relative-cycle window around the CPU WRITE to <addr_hex> (e.g.
        // `054A` = the X=10 `STA $540,X` store of the Implicit-DMA-Abort
        // CalculateDMADuration sweep). Columns are aligned with the TriCNES
        // per-cycle log so `/tmp/xdiff.py` can diff them relative-cycle by
        // relative-cycle. `occurrence` = `last` (default) or a 0-based index.
        if let Ok(spec) = std::env::var("RUSTYNES_ANCHOR") {
            let mut it = spec.split(',');
            let addr = u16::from_str_radix(
                it.next().unwrap_or("054A").trim().trim_start_matches('$'),
                16,
            )
            .expect("parse anchor addr hex");
            let pre: u64 = it
                .next()
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(2000);
            let post: u64 = it.next().and_then(|s| s.trim().parse().ok()).unwrap_or(200);
            let occ = it
                .next()
                .map(|s| s.trim().to_string())
                .unwrap_or_else(|| "last".to_string());

            // All writes to the anchor addr (cycle, pc, data). $054A is
            // written both by the per-frame NMI clear (pc≈F0D5) AND the Loop3
            // measure store `STA $540,X` (the real anchor). `RUSTYNES_ANCHOR_PC`
            // (hex opcode addr, e.g. DC52) disambiguates to the measure store.
            let pc_filter: Option<u16> = std::env::var("RUSTYNES_ANCHOR_PC")
                .ok()
                .and_then(|s| u16::from_str_radix(s.trim().trim_start_matches('$'), 16).ok());
            let writes: Vec<(u64, u16, u8)> = records
                .iter()
                .filter(|r| r.bus_addr == addr && matches!(r.bus_access, BusAccess::Write))
                .filter(|r| pc_filter.is_none_or(|p| r.pc == p))
                .map(|r| (r.cpu_cycle, r.pc, r.bus_data))
                .collect();
            eprintln!(
                "ANCHOR ${addr:04X} (pc_filter={pc_filter:04X?}): {} write(s):",
                writes.len(),
            );
            for (c, p, d) in &writes {
                eprintln!("    cyc={c} pc={p:04X} data={d:02X}");
            }
            let anchor_cyc = match occ.as_str() {
                "last" => {
                    writes
                        .last()
                        .expect("no write to anchor addr in trace window")
                        .0
                }
                "first" => {
                    writes
                        .first()
                        .expect("no write to anchor addr in trace window")
                        .0
                }
                idx => writes[idx.parse::<usize>().expect("parse occurrence index")].0,
            };
            let lo = anchor_cyc.saturating_sub(pre);
            let hi = anchor_cyc + post;

            let mut aout = String::from(
                "rel,cpu_cycle,pc,access,addr,data,in_dmc,put_cycle,apu_phase,dmc_pending,\
                 dtimer,dbits,dsil,dbuf,cooldown,note\n",
            );
            for r in records {
                if r.cpu_cycle < lo || r.cpu_cycle > hi {
                    continue;
                }
                let access = match r.bus_access {
                    BusAccess::Read => "R",
                    BusAccess::Write => "W",
                    BusAccess::DmaRead => "r",
                    BusAccess::DmaWrite => "w",
                    BusAccess::Idle => "I",
                };
                let note = if r.cpu_cycle == anchor_cyc {
                    "ANCHOR"
                } else if r.bus_addr == 0x4000 && matches!(r.bus_access, BusAccess::Read) {
                    if r.bus_data == 0 {
                        "R4000=00_CATCH"
                    } else {
                        "R4000"
                    }
                } else if r.in_dmc_dma {
                    "dmc_dma"
                } else {
                    "-"
                };
                let rel = i64::try_from(r.cpu_cycle).unwrap_or(0)
                    - i64::try_from(anchor_cyc).unwrap_or(0);
                let _ = writeln!(
                    &mut aout,
                    "{},{},{:04X},{},{:04X},{:02X},{},{},{},{},{},{},{},{},{},{}",
                    rel,
                    r.cpu_cycle,
                    r.pc,
                    access,
                    r.bus_addr,
                    r.bus_data,
                    i32::from(r.in_dmc_dma),
                    i32::from(r.put_cycle_post),
                    i32::from(r.apu_phase_post),
                    i32::from(r.dmc_dma_pending_post),
                    r.dmc_timer_post,
                    r.dmc_bits_remaining_post,
                    i32::from(r.dmc_silence_post),
                    i32::from(r.dmc_buffer_full_post),
                    r.dmc_dma_cooldown_post,
                    note,
                );
            }
            fs::write(out_path, &aout).expect("write anchor output");
            println!(
                "ANCHOR mode: addr=${addr:04X} occ={occ} anchor_cyc={anchor_cyc} \
                 window=[{lo},{hi}] final=0x{final_val:02X} out={out_path}",
            );
            return ExitCode::from(0);
        }

        let mut out = String::new();
        out.push_str(
            "cpu_cycle,pc,frame,scanline,dot,access,addr,data,apu_irq_low,apu_irq_high,in_dmc,dmc_pending,apu_phase,put_cycle,short_load,abort_pend,abort_dly,note\n",
        );
        let mut rows = 0u64;
        let mut prev_cycle: Option<u64> = None;
        for r in records {
            if setup_mode {
                if !setup_interesting(r) {
                    continue;
                }
            } else if !in_window(r.cpu_cycle) {
                continue;
            }
            // Blank line between disjoint windows (separates loop iterations).
            if let Some(pc) = prev_cycle {
                if r.cpu_cycle > pc + 1 {
                    out.push('\n');
                }
            }
            prev_cycle = Some(r.cpu_cycle);
            let access = match r.bus_access {
                BusAccess::Read => "R",
                BusAccess::Write => "W",
                BusAccess::DmaRead => "r",
                BusAccess::DmaWrite => "w",
                BusAccess::Idle => "I",
            };
            // Annotate the salient events: the CPU BIT read, a DMC DMA $4015
            // re-read (the CLEAR), and the flag bit6 visible on any $4015 read.
            let note = if r.bus_addr == 0x4015 && matches!(r.bus_access, BusAccess::Read) {
                if r.bus_data & 0x40 != 0 {
                    "CPU_BIT flag=SET"
                } else {
                    "CPU_BIT flag=CLR"
                }
            } else if r.bus_addr == 0x4015 && matches!(r.bus_access, BusAccess::Write) {
                "STA_$4015 (abort/enable)"
            } else if r.bus_addr == 0x4015 && matches!(r.bus_access, BusAccess::DmaRead) {
                "DMA_re-read_$4015"
            } else if r.in_dmc_dma {
                "dmc_dma_cycle"
            } else {
                "-"
            };
            let _ = writeln!(
                &mut out,
                "{},{:04X},{},{},{},{},${:04X},${:02X},{},{},{},{},{},{},{},{},{},{}",
                r.cpu_cycle,
                r.pc,
                r.ppu_frame,
                r.ppu_scanline,
                r.ppu_dot,
                access,
                r.bus_addr,
                r.bus_data,
                i32::from(r.irq_pending_apu_at_low),
                i32::from(r.irq_pending_apu_at_high),
                i32::from(r.in_dmc_dma),
                i32::from(r.dmc_dma_pending_post),
                i32::from(r.apu_phase_post),
                i32::from(r.put_cycle_post),
                i32::from(r.dmc_dma_short_post),
                i32::from(r.dmc_abort_pending_post),
                r.dmc_abort_delay_post,
                note,
            );
            rows += 1;
        }
        fs::write(out_path, &out).expect("write output");

        println!(
            "rom={rom_path} result=${result_addr:04X} final=0x{final_val:02X} \
             set_frame={set_frame} trace_start={trace_start} trace_records={} \
             emitted_rows={rows} out={out_path}",
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
        "trace_dma_4015 requires the `irq-timing-trace` cargo feature. \
         Re-run with --features irq-timing-trace."
    );
    std::process::exit(2);
}
