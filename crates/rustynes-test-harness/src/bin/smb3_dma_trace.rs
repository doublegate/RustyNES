//! SMB3 OAM-DMA per-cycle trace probe (v1.2.0 diagnostic).
//!
//! NOT part of CI. Diagnostic tool only; uses a gitignored external ROM dump.
//!
//! Replays the SMB3 `.rnm` movie into 1-1, then runs idle frames with the
//! per-CPU-cycle IRQ/bus trace (`irq-timing-trace`) armed. On a frame where
//! Mario is DROPPED from OAM (RAM shadow OAM has him, PPU OAM does not), it
//! dumps the OAM-DMA cycle stream (`r`/`w` records around the DMA) so the
//! exact skipped/mis-aligned writes are visible.
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness \
//!     --features commercial-roms,irq-timing-trace,debug-hooks \
//!     --bin smb3_dma_trace -- <rom-path> --movie <file.rnm>
//! ```

#![allow(clippy::cast_possible_truncation, clippy::cast_sign_loss)]

use rustynes_core::irq_trace::BusAccess;
use rustynes_core::{Buttons, Movie, MoviePlayer, Nes};

fn mario_in_buf(buf: &[u8]) -> bool {
    buf.chunks_exact(4)
        .any(|s| s[0] < 0xEF && matches!(s[1], 0x05 | 0x07))
}

fn read_page(nes: &mut Nes, page: u8) -> [u8; 256] {
    let base = u16::from(page) << 8;
    let mut out = [0u8; 256];
    for (i, b) in out.iter_mut().enumerate() {
        *b = nes.cpu_bus_peek(base + i as u16);
    }
    out
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let rom_path = &args[1];
    let movie_path = args
        .iter()
        .position(|a| a == "--movie")
        .and_then(|i| args.get(i + 1).cloned())
        .expect("--movie required");

    let bytes = std::fs::read(rom_path).expect("read rom");
    let mut nes = Nes::from_rom(&bytes).expect("parse rom");

    let blob = std::fs::read(&movie_path).expect("read movie");
    let movie = Movie::deserialize(&blob).expect("parse movie");
    movie.seek_to_start(&mut nes).expect("seek movie");
    let mut player = MoviePlayer::new(&movie);
    while player.apply_next(&mut nes) {
        nes.run_frame();
    }
    eprintln!("movie replay complete; running traced idle frames");

    // Run idle frames; on the first few DROP frames, dump the DMA cycle stream.
    let mut dumped = 0u32;
    for f in 0..120usize {
        nes.set_buttons(0, Buttons::empty());

        // Shadow OAM (page 2) at frame start = what the NMI will DMA in.
        let ram = read_page(&mut nes, 0x02);
        let ram_mario = mario_in_buf(&ram);

        nes.bus_mut().enable_irq_trace(2_000_000);
        nes.run_frame();
        let trace = nes.bus_mut().take_irq_trace().expect("trace enabled");

        let oam = nes.oam();
        let oam_mario = mario_in_buf(&oam);

        // For EVERY frame, report which OAM rows (8-byte groups) now equal
        // OAM[0..8] — the corruption signature — and whether Mario dropped.
        {
            let row0 = &oam[0..8];
            let corrupted: Vec<usize> = (1..32)
                .filter(|&i| &oam[i * 8..i * 8 + 8] == row0)
                .collect();
            eprintln!(
                "[rows] f{f} drop={} corrupted_rows(==row0)={corrupted:?}",
                u8::from(ram_mario && !oam_mario)
            );
        }

        if ram_mario && !oam_mario && dumped < 3 {
            dumped += 1;
            eprintln!("\n=== DROP FRAME f{f}: dumping OAM-DMA cycle stream ===");
            // Collect all DMA write records (BusAccess::DmaWrite -> $2004) and
            // DMA read records, in order, with a running write index. We expect
            // 256 writes to OAM; if fewer (or a gap), we see exactly where.
            let recs = trace.records();
            // Find the OAM DMA window: the contiguous run containing the
            // DmaWrite($2004) records. Print every DMA cycle in that window
            // plus a little context, and flag the byte index of each write.
            let mut write_idx = 0u32;
            let mut in_dma = false;
            let mut printed = 0u32;
            let mut last_was_dma = false;
            for r in recs {
                let is_dma = matches!(r.bus_access, BusAccess::DmaRead | BusAccess::DmaWrite);
                if is_dma && !in_dma {
                    in_dma = true;
                    eprintln!(
                        "  -- DMA start @ cyc{} sl{} dot{} --",
                        r.cpu_cycle, r.ppu_scanline, r.ppu_dot
                    );
                }
                if in_dma {
                    let tag = match r.bus_access {
                        BusAccess::DmaWrite => {
                            let t = format!("W#{write_idx:3} -> $2004 = {:02X}", r.bus_data);
                            write_idx += 1;
                            t
                        }
                        BusAccess::DmaRead => {
                            format!("R         $ {:04X} = {:02X}", r.bus_addr, r.bus_data)
                        }
                        BusAccess::Idle => "Idle".into(),
                        other => format!("{other:?} ${:04X}={:02X}", r.bus_addr, r.bus_data),
                    };
                    // Print the cycles around the corruption window (writes
                    // 36..52, i.e. OAM offsets 36..52 covering the stale 40..47)
                    // plus DMA start/end markers, to keep output readable.
                    let near_gap = write_idx >= 35 && write_idx <= 54;
                    if near_gap || printed < 6 {
                        eprintln!(
                            "    cyc{} sl{} dot{} dmc{} owed{} : {tag}",
                            r.cpu_cycle,
                            r.ppu_scanline,
                            r.ppu_dot,
                            u8::from(r.in_dmc_dma),
                            r.dma_cycles_owed
                        );
                        printed += 1;
                    }
                    last_was_dma = true;
                } else if last_was_dma {
                    // Just exited the DMA run.
                    eprintln!("  -- DMA end: total writes = {write_idx} --");
                    break;
                }
            }
            eprintln!("  total OAM writes counted = {write_idx} (expect 256)");

            // Now scan the WHOLE frame for any OAM-affecting access AFTER the
            // DMA: a second DMA, or $2003/$2004 writes/reads. These are the only
            // ways OAM[40] could change after the DMA wrote 0x51 there.
            let mut second_dma_writes = 0u32;
            let mut dma_runs = 0u32;
            let mut prev_dma = false;
            let mut w2003 = 0u32;
            let mut w2004 = 0u32;
            for r in recs {
                let is_w = matches!(r.bus_access, BusAccess::DmaWrite);
                let is_dma = matches!(r.bus_access, BusAccess::DmaRead | BusAccess::DmaWrite);
                if is_dma && !prev_dma {
                    dma_runs += 1;
                }
                prev_dma = is_dma;
                if dma_runs >= 2 && is_w {
                    second_dma_writes += 1;
                }
                // Normal CPU writes to PPU regs are BusAccess::Write at $2003/$2004.
                if matches!(r.bus_access, BusAccess::Write) {
                    match r.bus_addr {
                        0x2003 => w2003 += 1,
                        0x2004 => w2004 += 1,
                        _ => {}
                    }
                }
            }
            eprintln!(
                "  whole-frame: DMA runs={dma_runs} second-DMA-writes={second_dma_writes} cpu_$2003_writes={w2003} cpu_$2004_writes={w2004}"
            );
            // What does OAM[40] hold right now (post-frame)?
            eprintln!("  post-frame OAM[40..48] = {:02X?}", &oam[40..48]);
            // The OAM-corruption hypothesis: OAM[40..48] (row 5) was overwritten
            // with OAM[0..8]. Confirm they now match.
            eprintln!(
                "  post-frame OAM[ 0.. 8] = {:02X?}  (corruption copies these over row 5)",
                &oam[0..8]
            );
            eprintln!(
                "  => OAM[40..48] == OAM[0..8] ? {}",
                oam[40..48] == oam[0..8]
            );
        }
    }
}
