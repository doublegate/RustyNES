//! Export `RustyNES` goldens for an HDL device-under-test to be compared against.
//!
//! # Usage
//!
//! ```text
//! nes_golden_export --rom <path> --out <dir> [--seed N] [--frames N]
//!                   [--boot-trace START..END] [--irq-trace CAP]
//!                   [--checkpoint-interval N]
//! ```
//!
//! Writes, under `<dir>`:
//!
//! | file | format | consumed by |
//! |---|---|---|
//! | `<stem>.boot.bin` | `CpuBootTrace` binary | `cpu_boot_trace_diff` |
//! | `<stem>.irq.csv` | per-cycle IRQ/bus CSV | `scripts/irq_trace_cross_diff.py` |
//! | `<stem>.ckpt.bin` | rolling per-cycle hash checkpoints | `checkpoint_diff` |
//! | `<stem>.obs.bin` | full-capture observable stream, 16-byte records | the testbench's self-diff, and a window re-run |
//! | `<stem>.index_fb.bin` | 256x240 LE `u16` | the testbench's frame comparison |
//! | `<stem>.fetch.bin` | per-dot PPU bus addresses, 12-byte records | rung 3's background-fetch gate |
//! | `<stem>.ram.bin` | 2 KiB CPU work RAM | `accuracy_coin_catalog::decode_results` |
//! | `<stem>.ram_init.bin` | 2 KiB CPU work RAM **before** execution | a co-simulation testbench, so its flat memory starts where the oracle's does |
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
    /// Capacity for the per-dot PPU bus-address capture (rung 3's v2.5.4 gate).
    fetch_trace: Option<usize>,
    checkpoint_interval: u64,
    /// v2.5.1 — rung 2's interrupt sweep. Instruction-indexed, not
    /// cycle-indexed: this side cannot assert a pin mid-instruction, so a
    /// cycle-indexed sweep would not be comparable. See `Oracle::run_with_injection`.
    inject_instructions: u64,
    inject_nmi_at: Option<u64>,
    inject_irq_at: Option<u64>,
    inject_hold: u64,
}

fn usage() -> ! {
    eprintln!(
        "usage: nes_golden_export --rom <path> --out <dir> [--seed N] [--frames N]\n\
         \x20                       [--boot-trace START..END] [--irq-trace CAP]\n\
         \x20                       [--checkpoint-interval N]"
    );
    std::process::exit(2)
}

fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (mut rom, mut out) = (None, None);
    let (mut seed, mut frames) = (0u64, 60u32);
    let (mut boot_trace, mut irq_trace) = (None, None);
    let mut fetch_trace: Option<usize> = None;
    let mut checkpoint_interval = rustynes_cosim::checkpoint::DEFAULT_INTERVAL;
    let (mut inject_instructions, mut inject_hold) = (0u64, 1u64);
    let (mut inject_nmi_at, mut inject_irq_at) = (None, None);

    let mut i = 0;
    // Every value-taking arm below advances `i` by TWO, not one: this loop has
    // no trailing increment, so stepping by one leaves `i` on the value, which
    // then falls through to `_ => usage()` and prints the help text as though
    // the FLAG were unknown. Stated once here rather than four times inline.
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
            "--inject-instructions" => {
                inject_instructions = need(i).parse().unwrap_or_else(|_| usage());
                i += 2;
            }
            "--inject-nmi-instr" => {
                inject_nmi_at = Some(need(i).parse().unwrap_or_else(|_| usage()));
                i += 2;
            }
            "--inject-irq-instr" => {
                inject_irq_at = Some(need(i).parse().unwrap_or_else(|_| usage()));
                i += 2;
            }
            "--inject-hold" => {
                inject_hold = need(i).parse().unwrap_or_else(|_| usage());
                i += 2;
            }
            "--irq-trace" => {
                irq_trace = Some(need(i).parse().unwrap_or_else(|_| usage()));
                i += 2;
            }
            "--fetch-trace" => {
                fetch_trace = Some(need(i).parse().unwrap_or_else(|_| usage()));
                i += 2;
            }
            "--checkpoint-interval" => {
                checkpoint_interval = need(i).parse().unwrap_or_else(|_| usage());
                if checkpoint_interval == 0 {
                    eprintln!("--checkpoint-interval must be non-zero");
                    usage();
                }
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
        fetch_trace,
        checkpoint_interval,
        inject_instructions,
        inject_nmi_at,
        inject_irq_at,
        inject_hold,
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

/// Write the three artifacts derived from the per-cycle trace, and return the
/// counts the manifest records.
///
/// One take, three artifacts. `Bus::take_irq_trace` **moves** the trace out, so
/// asking for the CSV and then the checkpoints would silently yield an
/// unarmed-looking `None` for whichever came second -- and `None` there is
/// indistinguishable from "the trace was never armed".
fn write_irq_artifacts(o: &mut Oracle, base: &Path, interval: u64) -> (usize, usize) {
    let Some(a) = o.take_irq_artifacts(interval) else {
        eprintln!("  WARNING: irq trace was armed but returned nothing");
        return (0, 0);
    };
    write(&suffixed(base, "irq.csv"), a.csv.as_bytes());

    // Written BEFORE the checkpoints, and outside the `Err` arm below, on
    // purpose. This is the full-capture stream: it is the only artifact the
    // checkpoint hashes can be independently re-derived from -- the CSV cannot,
    // because it carries neither `pc` nor `put_cycle_post` -- and it is what a
    // re-run of a located window consumes. An overflowed trace still holds real
    // records, and those are worth keeping even when hashing them would claim a
    // coverage they do not have.
    let observable_count = a.observables.len();
    write(
        &suffixed(base, "obs.bin"),
        &rustynes_cosim::checkpoint::observables_to_bytes(&a.observables),
    );

    match a.checkpoints {
        Ok(ck) => {
            write(
                &suffixed(base, "ckpt.bin"),
                &rustynes_cosim::checkpoint::to_bytes(&ck),
            );
            (ck.len(), observable_count)
        }
        // Refuse rather than emitting a short stream: a hash over a trace that
        // dropped records covers fewer cycles than it claims, and the DUT would
        // be blamed for our truncation.
        Err(e) => panic!("  ERROR: {e}"),
    }
}

/// Refuse an injection request that would silently produce a NON-INJECTED golden.
///
/// Every combination rejected here runs to completion and emits a plausible
/// artifact with no pin ever asserted -- and a sweep comparing two non-injected
/// runs agrees, reporting a pass for a stimulus that was never applied. That is
/// this programme's recurring failure mode, so it is refused at the boundary
/// rather than diagnosed later.
///
/// Returns the reason rather than exiting, so the rules are testable without a
/// process boundary.
fn injection_error(args: &Args) -> Option<String> {
    let pinned = args.inject_nmi_at.is_some() || args.inject_irq_at.is_some();
    if pinned && args.inject_instructions == 0 {
        return Some(
            "--inject-{nmi,irq}-instr requires a non-zero --inject-instructions; without it \
             the run falls back to a frame advance and no pin is ever asserted"
                .to_owned(),
        );
    }
    if args.inject_instructions == 0 {
        return None;
    }
    if !pinned {
        return Some(
            "--inject-instructions was given with neither --inject-nmi-instr nor \
             --inject-irq-instr; the run would assert no pin at all"
                .to_owned(),
        );
    }
    if args.inject_hold == 0 {
        return Some("--inject-hold must be non-zero; a zero hold never asserts a pin".to_owned());
    }
    for (name, at) in [("nmi", args.inject_nmi_at), ("irq", args.inject_irq_at)] {
        if let Some(k) = at
            && k >= args.inject_instructions
        {
            return Some(format!(
                "--inject-{name}-instr {k} is outside the {} instruction(s) this run executes; \
                 the pin would never be asserted",
                args.inject_instructions
            ));
        }
    }
    None
}

fn validate_injection(args: &Args) {
    if let Some(why) = injection_error(args) {
        eprintln!("{why}");
        usage();
    }
}

/// The manifest lines describing WHICH KIND OF RUN produced these goldens.
///
/// An injection run is not a frame run, and the manifest must not describe it as
/// one: `calls` counts executed INSTRUCTIONS there, while the shared field is
/// named `run_frame_calls`, and the pin positions and hold that produced the
/// stimulus were recorded nowhere at all. A DUT could not reproduce or audit the
/// golden from the artifact -- which is the manifest's entire job.
/// How much work this run performs, in the unit that run actually uses.
///
/// An injection run completes no frames, so describing it in frames is not a
/// rounding error -- it is the wrong unit, and it is what made the frame-count
/// warning fire on every correct sweep export.
/// Warn when a run did not do what was asked -- in the unit that run uses.
///
/// Both arms matter, and the second was briefly LOST. Gating the frame warning
/// on the run mode fixed a warning that fired on every correct injection export,
/// and in doing so removed the jam signal from injection runs entirely, because
/// that warning was the only thing checking them. A jammed CPU would then have
/// produced a short golden in silence -- the exact failure the frame warning
/// exists to prevent, moved rather than fixed.
fn warn_if_incomplete(args: &Args, o: &Oracle, calls: u64, frames_actual: u64) {
    if args.inject_instructions > 0 {
        if calls != args.inject_instructions {
            eprintln!(
                "  WARNING: requested {} instructions, executed {calls} (CPU jammed: {})",
                args.inject_instructions,
                o.nes().is_jammed()
            );
        }
    } else if frames_actual != u64::from(args.frames) {
        // Reachable when the CPU jams. Emit the goldens anyway -- a jammed ROM
        // is a legitimate thing to compare a DUT against -- but never let the
        // manifest claim a frame count that was not simulated.
        eprintln!(
            "  WARNING: requested {} frames, simulated {frames_actual} (CPU jammed: {})",
            args.frames,
            o.nes().is_jammed()
        );
    }
}

/// Write the per-dot PPU bus-address golden, when the trace was armed.
///
/// A DROPPED count is a TRUNCATED window, and a comparison over one that does
/// not know it is truncated claims a coverage it does not have. It is reported
/// loudly rather than folded into the manifest where nobody looks.
fn write_fetch_trace(o: &mut Oracle, base: &Path) {
    if let Some((bytes, dropped)) = o.take_fetch_trace() {
        write(&suffixed(base, "fetch.bin"), &bytes);
        if dropped > 0 {
            eprintln!(
                "  WARNING: fetch trace dropped {dropped} read(s) -- capacity too small, \
                 so the golden covers less than the run"
            );
        }
    }
}

fn run_scale(args: &Args) -> String {
    if args.inject_instructions > 0 {
        format!("{} instructions, injected", args.inject_instructions)
    } else {
        format!("{} frames", args.frames)
    }
}

fn run_mode_block(args: &Args, calls: u64, frames_actual: u64) -> String {
    if args.inject_instructions > 0 {
        format!(
            "run_mode     = instruction-injection\n\
             instr_req    = {}\n\
             instr_actual = {calls}\n\
             inject_nmi_at= {}\n\
             inject_irq_at= {}\n\
             inject_hold  = {}\n",
            args.inject_instructions,
            args.inject_nmi_at
                .map_or_else(|| "none".to_owned(), |v| v.to_string()),
            args.inject_irq_at
                .map_or_else(|| "none".to_owned(), |v| v.to_string()),
            args.inject_hold,
        )
    } else {
        format!(
            "run_mode     = frames\n\
             frames_req   = {}\n\
             frames_actual= {frames_actual}\n\
             run_frame_calls = {calls}\n",
            args.frames,
        )
    }
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

    // The power-on work RAM, captured BEFORE a single cycle runs.
    //
    // `Nes::from_rom_with_power_on_seed` fills the 2 KiB from a seeded PRNG, so
    // it is deterministic but NOT zero -- and a co-simulation testbench with
    // flat, zeroed memory therefore disagrees with the oracle on every read of
    // a location the program has not written. Those reads are real: the dummy
    // read of an un-indexed zero-page address is one, and it lands on unwritten
    // RAM constantly.
    //
    // Exported as a golden rather than reproduced in the testbench, because
    // reimplementing the oracle's PRNG in C++ is precisely the parallel
    // second implementation that drifts. The DUT loads these bytes; it does not
    // compute them.
    let ram_init = o.nes().bus().ram_bytes().to_vec();

    if let Some((start, end)) = args.boot_trace {
        // Capacity is the window, not the whole run: a bounded window is the
        // design, because a full AccuracyCoin run would be ~1 GB of records.
        let cap = usize::try_from(end.saturating_sub(start) + 1).unwrap_or(usize::MAX);
        o.enable_cpu_boot_trace(cap, start, end);
    }
    if let Some(cap) = args.irq_trace {
        o.enable_irq_trace(cap);
    }
    if let Some(cap) = args.fetch_trace {
        o.enable_fetch_trace(cap);
    }

    // `advance_frames`, not a `run_frame()` loop: the first call after power-on
    // is swallowed by the frame_complete latch the reset sequence leaves set, so
    // a bare loop emits an (N-1)-frame golden under a manifest claiming N.
    let frame_before = o.nes().frame();
    // The interrupt sweep runs INSTEAD of the frame advance: it steps a bounded
    // number of instructions with a pin asserted for part of the run, which is
    // the stimulus rung 2 compares. A frame advance would run past the window
    // and bury the divergence under thousands of unrelated cycles.
    validate_injection(&args);

    let calls = if args.inject_instructions > 0 {
        o.run_with_injection(
            args.inject_instructions,
            args.inject_nmi_at,
            args.inject_irq_at,
            args.inject_hold,
        )
    } else {
        o.advance_frames(u64::from(args.frames))
    };
    let frames_actual = o.nes().frame() - frame_before;
    let cycles = o.nes().cycle();
    // The frame check applies to a FRAME run only. An injection run steps
    // instructions and completes no frames at all, so this fired on every
    // correct sweep export -- "requested 1 frames, simulated 0", with nothing
    // wrong and the CPU not jammed. A warning that cries wolf on every valid run
    // is how a real one comes to be ignored, and this one was invisible to me
    // because the sweep script redirects stderr.
    warn_if_incomplete(&args, &o, calls, frames_actual);

    let base = args.out.join(&stem);
    println!("exporting goldens for {stem} ({}):", run_scale(&args));

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

    write_fetch_trace(&mut o, &base);

    // Checked BEFORE the write, and checked on `ram_init` specifically. The
    // assert below covers the post-run `ram`, so this buffer had no length check
    // at all -- a co-simulation testbench mirrors these bytes through $1FFF and
    // a short read would place them wrongly rather than fail, which is a
    // divergence at an unrelated address instead of an error here.
    assert_eq!(
        ram_init.len(),
        RAM_LEN,
        "unexpected power-on work RAM length"
    );
    write(&suffixed(&base, "ram_init.bin"), &ram_init);

    let ram = o.nes().bus().ram_bytes();
    assert_eq!(ram.len(), RAM_LEN, "unexpected work RAM length");
    write(&suffixed(&base, "ram.bin"), ram);

    if args.boot_trace.is_some() {
        match o.take_cpu_boot_trace_binary() {
            Some(b) => write(&suffixed(&base, "boot.bin"), &b),
            None => eprintln!("  WARNING: boot trace was armed but returned nothing"),
        }
    }
    let (checkpoint_count, observable_count) = if args.irq_trace.is_some() {
        write_irq_artifacts(&mut o, &base, args.checkpoint_interval)
    } else {
        (0, 0)
    };

    let mode_block = run_mode_block(&args, calls, frames_actual);

    let manifest = format!(
        "rom          = {}\n\
         rom_sha256   = {}\n\
         seed         = {}\n\
         {}\
         cpu_cycles   = {}\n\
         emulator     = rustynes {}\n\
         index_fb_len = {}\n\
         ram_len      = {}\n\
         ckpt_interval= {}\n\
         ckpt_count   = {}\n\
         obs_count    = {}\n",
        args.rom.display(),
        sha256_hex(&rom),
        args.seed,
        mode_block,
        cycles,
        env!("CARGO_PKG_VERSION"),
        INDEX_FB_LEN,
        RAM_LEN,
        args.checkpoint_interval,
        checkpoint_count,
        observable_count,
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
