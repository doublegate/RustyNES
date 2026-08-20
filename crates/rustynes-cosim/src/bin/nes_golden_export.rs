//! Export `RustyNES` goldens for an HDL device-under-test to be compared against.
//!
//! # Usage
//!
//! ```text
//! nes_golden_export --rom <path> --out <dir> [--seed N] [--frames N]
//!                   [--boot-trace START..END] [--irq-trace CAP]
//! ```
//!
//! Writes, under `<dir>`:
//!
//! | file | format | consumed by |
//! |---|---|---|
//! | `<stem>.boot.bin` | `CpuBootTrace` binary | `cpu_boot_trace_diff` |
//! | `<stem>.irq.csv` | per-cycle IRQ/bus CSV | `scripts/irq_trace_cross_diff.py` |
//! | `<stem>.index_fb.bin` | 256x240 LE `u16` | the testbench's frame comparison |
//! | `<stem>.ram.bin` | 2 KiB CPU work RAM | `accuracy_coin_catalog::decode_results` |
//! | `<stem>.manifest.txt` | provenance | humans, and the drift guard below |
//!
//! # The manifest is not decoration
//!
//! The determinism contract covers the framebuffer and audio. It says **nothing**
//! about trace-format stability, and `cpu_boot_trace` is at schema version 1 with
//! a history of being reshaped. So a routine `RustyNES` accuracy fix can change a
//! golden and turn the FPGA repository's CI red for a reason unrelated to its RTL.
//!
//! The manifest records the ROM SHA-256, the seed, the frame count and the
//! emulator version that produced the goldens, so a red diff can be attributed to
//! the right side of the boundary in one look rather than by bisecting two repos.

use std::path::{Path, PathBuf};

use rustynes_cosim::Oracle;
use sha2::{Digest, Sha256};

const INDEX_FB_LEN: usize = 256 * 240;
const RAM_LEN: usize = 2048;

struct Args {
    rom: PathBuf,
    out: PathBuf,
    seed: u64,
    frames: u32,
    boot_trace: Option<(u64, u64)>,
    irq_trace: Option<usize>,
}

fn usage() -> ! {
    eprintln!(
        "usage: nes_golden_export --rom <path> --out <dir> [--seed N] [--frames N]\n\
         \x20                       [--boot-trace START..END] [--irq-trace CAP]"
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (mut rom, mut out) = (None, None);
    let (mut seed, mut frames) = (0u64, 60u32);
    let (mut boot_trace, mut irq_trace) = (None, None);

    let mut i = 0;
    while i < argv.len() {
        let need = |i: usize| -> &String { argv.get(i + 1).unwrap_or_else(|| usage()) };
        match argv[i].as_str() {
            "--rom" => {
                rom = Some(PathBuf::from(need(i)));
                i += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(need(i)));
                i += 2;
            }
            "--seed" => {
                seed = need(i).parse().unwrap_or_else(|_| usage());
                i += 2;
            }
            "--frames" => {
                frames = need(i).parse().unwrap_or_else(|_| usage());
                i += 2;
            }
            "--boot-trace" => {
                let v = need(i);
                let (a, b) = v.split_once("..").unwrap_or_else(|| usage());
                boot_trace = Some((
                    a.parse().unwrap_or_else(|_| usage()),
                    b.parse().unwrap_or_else(|_| usage()),
                ));
                i += 2;
            }
            "--irq-trace" => {
                irq_trace = Some(need(i).parse().unwrap_or_else(|_| usage()));
                i += 2;
            }
            _ => usage(),
        }
    }
    Args {
        rom: rom.unwrap_or_else(|| usage()),
        out: out.unwrap_or_else(|| usage()),
        seed,
        frames,
        boot_trace,
        irq_trace,
    }
}

/// SHA-256 of the ROM, so a golden can be tied to the exact input that made it.
///
/// `sha2` is already a workspace dependency, used by `rustynes-core` (which this
/// crate depends on), `rustynes-frontend` and `rustynes-test-harness` -- so this
/// is reuse, not a new dependency to justify.
fn sha256_hex(data: &[u8]) -> String {
    // Built by hand rather than through `write!`, so there is no `fmt::Result` to
    // discard. Review flagged the discarded result, and it was right that `let _ =`
    // on a fallible call is against the project's rules even where the call cannot
    // fail into a `String`.
    //
    // NOT `format!("{:x}", Sha256::digest(data))`, which review also suggested:
    // sha2 0.11 returns `hybrid_array::Array<u8, _>`, which does not implement
    // `LowerHex`. Checked against a scratch crate rather than assumed --
    // `the trait bound Array<u8, ...>: LowerHex is not satisfied`.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(data);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    out
}

/// `<base>.<suffix>`, by APPENDING rather than by `Path::with_extension`.
///
/// `with_extension` replaces everything after the last dot, so a ROM named
/// `Super Mario Bros. 3.nes` has a stem of `Super Mario Bros. 3` and
/// `with_extension("ram.bin")` yields `Super Mario Bros.ram.bin` -- the frame
/// number silently eaten. Verified, not assumed: that is the literal output.
/// Dots in NES filenames are common enough that this would have corrupted real
/// golden sets. Found in review.
fn suffixed(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(".");
    s.push(suffix);
    PathBuf::from(s)
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    println!("  wrote {} ({} bytes)", path.display(), bytes.len());
}

fn main() {
    let args = parse_args();
    let rom =
        std::fs::read(&args.rom).unwrap_or_else(|e| panic!("read {}: {e}", args.rom.display()));
    let stem = args
        .rom
        .file_stem()
        .map_or_else(|| "rom".to_owned(), |s| s.to_string_lossy().into_owned());
    std::fs::create_dir_all(&args.out).expect("create out dir");

    let mut o = Oracle::new(&rom, args.seed).unwrap_or_else(|e| panic!("parse rom: {e}"));
    if let Some((start, end)) = args.boot_trace {
        // Capacity is the window, not the whole run: a bounded window is the
        // design, because a full AccuracyCoin run would be ~1 GB of records.
        let cap = usize::try_from(end.saturating_sub(start) + 1).unwrap_or(usize::MAX);
        o.enable_cpu_boot_trace(cap, start, end);
    }
    if let Some(cap) = args.irq_trace {
        o.enable_irq_trace(cap);
    }

    // `advance_frames`, not a `run_frame()` loop: the first call after power-on
    // is swallowed by the frame_complete latch the reset sequence leaves set, so
    // a bare loop emits an (N-1)-frame golden under a manifest claiming N.
    let frame_before = o.nes().frame();
    let calls = o.advance_frames(u64::from(args.frames));
    let frames_actual = o.nes().frame() - frame_before;
    let cycles = o.nes().cycle();
    if frames_actual != u64::from(args.frames) {
        // Reachable when the CPU jams. Emit the goldens anyway -- a jammed ROM
        // is a legitimate thing to compare a DUT against -- but never let the
        // manifest claim a frame count that was not simulated.
        eprintln!(
            "  WARNING: requested {} frames, simulated {frames_actual} (CPU jammed: {})",
            args.frames,
            o.nes().is_jammed()
        );
    }

    let base = args.out.join(&stem);
    println!("exporting goldens for {} ({} frames):", stem, args.frames);

    let fb = o.nes().index_framebuffer();
    assert_eq!(
        fb.len(),
        INDEX_FB_LEN,
        "unexpected index framebuffer length"
    );
    let mut fb_bytes = Vec::with_capacity(INDEX_FB_LEN * 2);
    for px in fb {
        fb_bytes.extend_from_slice(&px.to_le_bytes());
    }
    write(&suffixed(&base, "index_fb.bin"), &fb_bytes);

    let ram = o.nes().bus().ram_bytes();
    assert_eq!(ram.len(), RAM_LEN, "unexpected work RAM length");
    write(&suffixed(&base, "ram.bin"), ram);

    if args.boot_trace.is_some() {
        match o.take_cpu_boot_trace_binary() {
            Some(b) => write(&suffixed(&base, "boot.bin"), &b),
            None => eprintln!("  WARNING: boot trace was armed but returned nothing"),
        }
    }
    if args.irq_trace.is_some() {
        match o.take_irq_trace_csv() {
            Some(csv) => write(&suffixed(&base, "irq.csv"), csv.as_bytes()),
            None => eprintln!("  WARNING: irq trace was armed but returned nothing"),
        }
    }

    let manifest = format!(
        "rom          = {}\n\
         rom_sha256   = {}\n\
         seed         = {}\n\
         frames_req   = {}\n\
         frames_actual= {}\n\
         run_frame_calls = {}\n\
         cpu_cycles   = {}\n\
         emulator     = rustynes {}\n\
         index_fb_len = {}\n\
         ram_len      = {}\n",
        args.rom.display(),
        sha256_hex(&rom),
        args.seed,
        args.frames,
        frames_actual,
        calls,
        cycles,
        env!("CARGO_PKG_VERSION"),
        INDEX_FB_LEN,
        RAM_LEN,
    );
    write(&suffixed(&base, "manifest.txt"), manifest.as_bytes());
    println!("done; {cycles} CPU cycles simulated");
}

#[cfg(test)]
mod tests {
    use super::{sha256_hex, suffixed};
    use std::path::Path;

    /// A dot in the ROM name must not eat part of the golden's filename.
    ///
    /// `Path::with_extension` replaces everything after the LAST dot, so
    /// `Super Mario Bros. 3` became `Super Mario Bros.ram.bin` -- the frame number
    /// silently gone. NES filenames contain dots routinely, so this would have
    /// corrupted real golden sets rather than being a theoretical edge. Found in
    /// review; the old behaviour is asserted against explicitly so a well-meaning
    /// "simplify this to `with_extension`" is caught.
    #[test]
    fn a_dotted_rom_name_keeps_its_whole_stem() {
        let base = Path::new("/out/Super Mario Bros. 3");
        assert_eq!(
            suffixed(base, "ram.bin"),
            Path::new("/out/Super Mario Bros. 3.ram.bin")
        );
        assert_ne!(
            suffixed(base, "ram.bin"),
            base.with_extension("ram.bin"),
            "if these agree, `with_extension` stopped truncating and this test is moot"
        );
    }

    #[test]
    fn an_undotted_name_is_unaffected() {
        assert_eq!(
            suffixed(Path::new("/out/nestest"), "boot.bin"),
            Path::new("/out/nestest.boot.bin")
        );
    }

    /// Pinned against an independently-known digest, not against our own output.
    #[test]
    fn sha256_matches_the_known_empty_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}
