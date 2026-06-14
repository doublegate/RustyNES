//! FDS read-stream trace harness (T-101-002 — Kid Icarus side-B ERR.07).
//!
//! Boots a `.fds` with the real BIOS, enables the `Nes` FDS trace, scripts an
//! eject -> insert-side-N swap, then drains the `$4031` disk-byte stream (plus
//! `$4025` control writes + side changes) and diffs the side-N disk-info block as
//! the BIOS actually read it against the KNOWN-CORRECT raw `.fds` block. The
//! `ERR.07` ("wrong side number") fires when the BIOS's side-number byte read does
//! not match, so the diff pinpoints the exact byte where the read diverges —
//! no second emulator required.
//!
//! NOT part of CI. Diagnostic only; the BIOS and disk images are never committed.
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features test-roms,commercial-roms --release \
//!     --bin fds_trace -- <bios.rom> <disk.fds> <total-frames> [swap-at=600] [swap-to=1]
//! ```

// Diagnostic dev tool: the casts are tiny disk-side / counter values from argv,
// and `main` is a single linear analysis pass — the pedantic lints can't represent
// a real bug here.
#![allow(
    clippy::too_many_lines,
    clippy::cast_possible_truncation,
    clippy::cast_possible_wrap
)]

use rustynes_core::{Buttons, Nes};

const FDS_SIDE_LEN: usize = 65500;
const INFO_BLOCK_LEN: usize = 56;

fn arg_u64(args: &[String], i: usize, default: u64) -> u64 {
    args.get(i).and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 4 {
        eprintln!(
            "usage: {} <bios.rom> <disk.fds> <total-frames> [swap-at=600] [swap-to=1]",
            args.first().map_or("fds_trace", |s| s.as_str())
        );
        std::process::exit(2);
    }
    let bios = std::fs::read(&args[1]).expect("read BIOS");
    let disk = std::fs::read(&args[2]).expect("read .fds");
    let total: u64 = args[3].parse().expect("total-frames");
    let swap_at = arg_u64(&args, 4, 600);
    let swap_to = arg_u64(&args, 5, 1) as usize;
    let insert_at = swap_at + 8;

    // The known-correct expected disk-info block for the swapped-in side, straight
    // from the raw .fds (header-less form; each side is FDS_SIDE_LEN bytes).
    // A headered `.fds` starts with "FDS\x1a" + a 16-byte header; require the
    // full header before slicing it off so a short/corrupt file can't panic.
    let body = if disk.len() >= 16 && &disk[..3] == b"FDS" {
        &disk[16..]
    } else {
        &disk[..]
    };
    let side_base = swap_to * FDS_SIDE_LEN;
    let expected: Vec<u8> = body
        .get(side_base..side_base + INFO_BLOCK_LEN)
        .map(<[u8]>::to_vec)
        .unwrap_or_default();
    println!(
        "expected side {swap_to} disk-info block (raw .fds): side#@0x15={} disk#@0x16={}",
        expected.get(0x15).copied().unwrap_or(0xEE),
        expected.get(0x16).copied().unwrap_or(0xEE),
    );

    let mut nes = Nes::from_disk(&disk, &bios).expect("construct FDS");
    nes.enable_fds_trace();

    for f in 0..total {
        let btn = if f % 180 == 60 {
            Buttons::START
        } else {
            Buttons::empty()
        };
        nes.set_buttons(0, btn);
        if f == swap_at {
            nes.set_disk_side(None);
        }
        if f == insert_at {
            nes.set_disk_side(Some(swap_to));
        }
        nes.run_frame();
    }

    let trace = nes.take_fds_trace();
    println!("trace records: {}", trace.len());

    // Head-motion analysis: how the disk position advances + wraps. A monotonic
    // climb that never wraps = stuck mid-disk; many rewinds-to-0 = motor cycling;
    // end-of-head sets = the head reached the inner track (disk-loop point).
    let reads: Vec<&_> = trace.iter().filter(|r| r.kind == 0).collect();
    let max_head = reads.iter().map(|r| r.head).max().unwrap_or(0);
    let rewinds = reads.windows(2).filter(|w| w[1].head < w[0].head).count();
    let rewinds_to_0 = reads
        .windows(2)
        .filter(|w| w[1].head == 0 && w[0].head > 100)
        .count();
    let eoh = reads.iter().filter(|r| (r.status & 0x40) != 0).count();
    println!(
        "head: max={max_head}  rewinds(any backward)={rewinds}  rewinds-to-0={rewinds_to_0}  \
         end-of-head reads={eoh}",
    );

    // CRC-error analysis: a wrong synthesized CRC makes the BIOS retry forever
    // (status bit 4 = $4030.D4). Count them + show the head positions of the first
    // few — a recurring head position is the block the BIOS keeps rejecting.
    let crc_errs: Vec<(u32, i8)> = trace
        .iter()
        .filter(|r| r.kind == 0 && (r.status & 0x10) != 0)
        .map(|r| (r.head, r.side))
        .collect();
    println!(
        "$4031 reads with CRC-error ($4030.D4) set: {}",
        crc_errs.len()
    );
    if !crc_errs.is_empty() {
        let mut heads: Vec<u32> = crc_errs.iter().map(|(h, _)| *h).collect();
        heads.sort_unstable();
        heads.dedup();
        println!(
            "  distinct head positions flagged CRC-error: {} -> {:?}",
            heads.len(),
            &heads[..heads.len().min(12)]
        );
    }

    // Side changes + control writes, in order (context for the read stream).
    for r in &trace {
        match r.kind {
            1 => println!(
                "  $4025<={:08b} (motor {}, reset {}, read {}) side={} head={}",
                r.value,
                if r.value & 0x02 == 0 { "ON" } else { "off" },
                r.value & 0x01,
                (r.value & 0x04) >> 2,
                r.side,
                r.head
            ),
            2 => println!(
                "  SET SIDE -> {} (head reset)",
                if r.value == 0xFF {
                    "EJECT".to_string()
                } else {
                    r.value.to_string()
                }
            ),
            _ => {}
        }
    }

    // The $4031 byte stream the BIOS read while side `swap_to` was inserted.
    let side_stream: Vec<u8> = trace
        .iter()
        .filter(|r| r.kind == 0 && r.side == swap_to as i8)
        .map(|r| r.value)
        .collect();
    println!(
        "\n$4031 bytes read while side {swap_to} inserted: {}",
        side_stream.len()
    );

    // Find the disk-info block the BIOS read: the run beginning 0x01 *NINTENDO-HVC*.
    let magic = b"*NINTENDO-HVC*";
    let found = side_stream
        .windows(15)
        .position(|w| w[0] == 0x01 && &w[1..15] == magic);
    match found {
        None => {
            println!(
                "DIVERGENCE: the BIOS never read a well-formed disk-info block from side \
                 {swap_to} (no `01 *NINTENDO-HVC*` in the side-{swap_to} read stream).\n\
                 First 32 bytes read on side {swap_to}: {:02x?}",
                &side_stream[..side_stream.len().min(32)]
            );
        }
        Some(p) => {
            let got = &side_stream[p..(p + INFO_BLOCK_LEN).min(side_stream.len())];
            println!("disk-info block found at read offset {p}.");
            let mut diverged = false;
            for i in 0..INFO_BLOCK_LEN.min(got.len()).min(expected.len()) {
                if got[i] != expected[i] {
                    println!(
                        "  DIVERGENCE at block byte 0x{i:02x}: BIOS read {:02x}, expected {:02x}{}",
                        got[i],
                        expected[i],
                        if i == 0x15 {
                            "  <- side# (ERR.07 source)"
                        } else {
                            ""
                        }
                    );
                    diverged = true;
                }
            }
            if expected.is_empty() || got.is_empty() {
                // No reference bytes for this side (e.g. an invalid side index):
                // the byte-compare loop ran zero times, so don't claim a MATCH.
                println!(
                    "  side {swap_to} has no reference disk-info block in the raw .fds \
                     (expected={} got={} bytes); cannot compare.",
                    expected.len(),
                    got.len()
                );
            } else if !diverged {
                println!(
                    "  side {swap_to} disk-info block read MATCHES the raw .fds exactly \
                     (side#=0x{:02x}); the ERR.07 side# check is satisfied here — the \
                     mismatch must be against a DIFFERENT expected side# (the BIOS's boot \
                     reset-check expects side 0), not a corrupted read.",
                    got.get(0x15).copied().unwrap_or(0xEE)
                );
            }
        }
    }
}
