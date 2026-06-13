//! Boot-and-screenshot smoke harness for Famicom Disk System (FDS) games with a
//! real `disksys.rom` BIOS (v2.6.0). NOT part of CI. Diagnostic only — the BIOS
//! and disk images are never committed.
//!
//! Walks a directory for `.fds` disk images, boots each with the supplied BIOS
//! via [`Nes::from_disk`], runs N frames (pulsing Start to advance the BIOS
//! disk-load + title prompts), dumps a 256x240 PNG per disk, and prints a
//! per-disk health line (colour count / PANIC), mirroring `coverage_smoke`.
//!
//! Usage:
//!
//! ```text
//! cargo run -p rustynes-test-harness --features commercial-roms --release \
//!     --bin fds_smoke -- <bios.rom> <fds-dir> <frames> <out-dir> [name-filter]
//! ```

use std::collections::HashSet;
use std::panic::{self, AssertUnwindSafe};
use std::path::{Path, PathBuf};

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

fn walk_fds(root: &Path, out: &mut Vec<PathBuf>) {
    let Ok(rd) = std::fs::read_dir(root) else {
        return;
    };
    for e in rd.flatten() {
        let p = e.path();
        if p.is_dir() {
            walk_fds(&p, out);
        } else if p.extension().is_some_and(|x| x == "fds") {
            out.push(p);
        }
    }
}

fn main() {
    let args: Vec<String> = std::env::args().collect();
    let bios_path = args
        .get(1)
        .map_or("tests/roms/external/fds/disksys-fcd.rom", |s| s.as_str());
    let fds_dir = args
        .get(2)
        .map_or("tests/roms/external/fds", |s| s.as_str());
    let frames: u64 = args.get(3).and_then(|s| s.parse().ok()).unwrap_or(900);
    let out_dir = args.get(4).map_or("/tmp/rustynes-fds", |s| s.as_str());
    let filter = args.get(5).cloned().unwrap_or_default();
    std::fs::create_dir_all(out_dir).expect("mkdir out");

    let bios = match std::fs::read(bios_path) {
        Ok(b) if b.len() == 8192 => b,
        Ok(b) => {
            eprintln!("BIOS {bios_path} is {} bytes, expected 8192", b.len());
            return;
        }
        Err(e) => {
            eprintln!("cannot read BIOS {bios_path}: {e}");
            return;
        }
    };
    let bios_name = Path::new(bios_path)
        .file_stem()
        .map_or_else(|| "bios".into(), |s| s.to_string_lossy().into_owned());

    let mut disks = Vec::new();
    walk_fds(Path::new(fds_dir), &mut disks);
    disks.sort();
    disks.retain(|p| filter.is_empty() || p.to_string_lossy().contains(&filter));

    let (mut ok, mut sus, mut panicked) = (0u32, 0u32, 0u32);
    for disk_path in &disks {
        let stem = disk_path
            .file_stem()
            .map_or_else(|| "disk".into(), |s| s.to_string_lossy().into_owned());
        let label = format!("{stem} [{bios_name}]");
        let disk = std::fs::read(disk_path).expect("read fds");
        let result = panic::catch_unwind(AssertUnwindSafe(|| -> Result<Vec<u8>, String> {
            let mut nes = Nes::from_disk(&disk, &bios).map_err(|e| e.to_string())?;
            for f in 0..frames {
                // The BIOS shows a "set disk / loading" sequence; pulse Start
                // every ~180 frames to advance the boot + any "push start" title.
                let btn = if f % 180 == 60 {
                    Buttons::START
                } else {
                    Buttons::empty()
                };
                nes.set_buttons(0, btn);
                nes.run_frame();
            }
            Ok(nes.framebuffer().to_vec())
        }));
        match result {
            Ok(Ok(fb)) => {
                let colours: HashSet<[u8; 4]> = fb
                    .chunks_exact(4)
                    .map(|c| [c[0], c[1], c[2], c[3]])
                    .collect();
                let n = colours.len();
                write_png(&Path::new(out_dir).join(format!("{label}.png")), &fb);
                if n <= 4 {
                    sus += 1;
                    println!("SUSPICIOUS  {n:3} colours  {label}");
                } else {
                    ok += 1;
                    println!("ok          {n:3} colours  {label}");
                }
            }
            Ok(Err(e)) => {
                panicked += 1;
                println!("CONSTRUCT-ERR {label}: {e}");
            }
            Err(_) => {
                panicked += 1;
                println!("PANIC       {label}");
            }
        }
    }
    println!(
        "\n{} disks: ok={ok} suspicious={sus} panic/err={panicked} -> {out_dir}",
        disks.len()
    );
}
