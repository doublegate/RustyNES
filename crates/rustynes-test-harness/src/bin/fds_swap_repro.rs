//! FDS disk-swap reproduction harness (T-101-002 — Kid Icarus side-B stall).
//!
//! `fds_smoke` boots a `.fds` and pulses Start but **never swaps disk sides**, so
//! it cannot exercise a game that needs side B (it just sits on the BIOS
//! "set next disk side" wait). This harness scripts an eject -> insert-side-N at a
//! configurable frame and logs the per-window framebuffer activity + the disk-side
//! state, so the stall — and whether the swap unblocks it — is visible headlessly.
//!
//! NOT part of CI. Diagnostic only; the BIOS and disk images are never committed.
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features test-roms,commercial-roms --release \
//!     --bin fds_swap_repro -- <bios.rom> <disk.fds> <total-frames> <out-dir> \
//!     [swap-at-frame=600] [swap-to-side=1] [eject-frames=8]
//! ```
//!
//! It prints a 60-frame timeline (`side=`, distinct-colour count, framebuffer
//! FNV-1a, and a `(static)` marker when the frame is unchanged from the previous
//! sample) plus EJECT / INSERT events, and dumps PNGs at boot, around the swap,
//! and at the end. Sweep `swap-at-frame` to find when the game wants side B; if no
//! timing unblocks it, the FDS device is not signalling the insert correctly.

// Diagnostic dev tool only: the one `u64 -> usize` cast is a tiny disk-side index
// from argv, where the pedantic cast lint cannot represent a real truncation.
#![allow(clippy::cast_possible_truncation)]

use std::collections::HashSet;
use std::path::Path;

use rustynes_core::{Buttons, Nes};

fn write_png(path: &Path, fb: &[u8]) {
    let file = std::fs::File::create(path).expect("create png");
    let w = std::io::BufWriter::new(file);
    let mut enc = png::Encoder::new(w, 256, 240);
    enc.set_color(png::ColorType::Rgba);
    enc.set_depth(png::BitDepth::Eight);
    let mut writer = enc.write_header().expect("png header");
    writer.write_image_data(fb).expect("png data");
}

/// FNV-1a 64-bit hash of the framebuffer (to spot a static / stalled screen).
fn fb_hash(fb: &[u8]) -> u64 {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for &b in fb {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    h
}

fn colour_count(fb: &[u8]) -> usize {
    fb.chunks_exact(4)
        .map(|c| [c[0], c[1], c[2], c[3]])
        .collect::<HashSet<[u8; 4]>>()
        .len()
}

fn arg_u64(args: &[String], i: usize, default: u64) -> u64 {
    args.get(i).and_then(|s| s.parse().ok()).unwrap_or(default)
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    if args.len() < 5 {
        eprintln!(
            "usage: {} <bios.rom> <disk.fds> <total-frames> <out-dir> \
             [swap-at-frame=600] [swap-to-side=1] [eject-frames=8]",
            args.first().map_or("fds_swap_repro", |s| s.as_str())
        );
        std::process::exit(2);
    }
    let bios_path = &args[1];
    let disk_path = &args[2];
    let total: u64 = args[3].parse().expect("total-frames must be a number");
    let out_dir = &args[4];
    let swap_at = arg_u64(&args, 5, 600);
    let swap_to = arg_u64(&args, 6, 1) as usize;
    let eject_frames = arg_u64(&args, 7, 8);

    let bios = std::fs::read(bios_path).expect("read BIOS");
    assert_eq!(bios.len(), 8192, "BIOS must be 8192 bytes");
    let disk = std::fs::read(disk_path).expect("read .fds");

    let mut nes = Nes::from_disk(&disk, &bios).expect("construct FDS");
    let sides = nes.disk_side_count();
    let stem = Path::new(disk_path)
        .file_stem()
        .map_or_else(|| "disk".into(), |s| s.to_string_lossy().into_owned());
    eprintln!(
        "{stem}: {sides} disk side(s); inserted={:?}; \
         swap at frame {swap_at} -> side {swap_to} (eject {eject_frames} frames)",
        nes.inserted_disk_side()
    );

    let insert_at = swap_at + eject_frames;
    let dump_frames = [
        60u64,
        swap_at.saturating_sub(1),
        insert_at + 1,
        swap_at + 200,
        total - 1,
    ];
    let mut last_hash = 0u64;

    for f in 0..total {
        // Pulse Start every 180 frames (offset 60) to advance the BIOS load +
        // any "push start" prompt — matches fds_smoke's cadence.
        let btn = if f % 180 == 60 {
            Buttons::START
        } else {
            Buttons::empty()
        };
        nes.set_buttons(0, btn);

        if f == swap_at {
            nes.set_disk_side(None);
            eprintln!("[f{f:5}] EJECT (side -> None)");
        }
        if f == insert_at {
            nes.set_disk_side(Some(swap_to));
            eprintln!("[f{f:5}] INSERT side {swap_to}");
        }

        nes.run_frame();

        if f % 60 == 0 || dump_frames.contains(&f) {
            let fb = nes.framebuffer();
            let h = fb_hash(fb);
            let c = colour_count(fb);
            let marker = if h == last_hash { " (static)" } else { "" };
            eprintln!(
                "[f{f:5}] side={:?} colours={c:3} fb={h:016x}{marker}",
                nes.inserted_disk_side()
            );
            last_hash = h;
        }
        if dump_frames.contains(&f) {
            write_png(
                &Path::new(out_dir).join(format!("{stem}_f{f}.png")),
                nes.framebuffer(),
            );
        }
    }
    eprintln!("done -> {out_dir}");
}
