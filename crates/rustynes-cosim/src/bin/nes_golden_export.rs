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
    /// Hold START on port 1 across a half-open frame window.
    ///
    /// Spelled `--press-start A:B` on the command line; the window itself is
    /// `A..B`, i.e. START is down for frames `A` through `B - 1`. The two
    /// notations were mixed here -- the field said `A..B` where a reader would
    /// take it for the CLI syntax -- so both are now stated.
    ///
    /// Several accuracy ROMs do not start on their own. `AccuracyCoin` sits on
    /// its title screen indefinitely, and a golden exported without this
    /// captures an idle menu -- which a DUT reproduces perfectly while running
    /// none of the tests.
    press_start: Option<(u64, u64)>,
    boot_trace: Option<(u64, u64)>,
    irq_trace: Option<usize>,
    /// Capacity for the per-dot PPU bus-address capture (rung 3's v2.5.4 gate).
    fetch_trace: Option<usize>,
    apu_trace: Option<usize>,
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
         \x20                       [--fetch-trace CAP] [--apu-trace CAP] [--checkpoint-interval N]\n\
         \x20                       [--inject-instructions N] [--inject-hold N]\n\
         \x20                       [--inject-nmi-at N] [--inject-irq-at N]\n\
         \x20                       [--press-start A:B]"
    );
    std::process::exit(2)
}

/// Parse a `--press-start A:B` spec. `None` for anything this option should
/// refuse.
///
/// Split from the exiting wrapper below so a test can reach the DECISION. The
/// wrapper calls `usage()`, which calls `std::process::exit`, so a test of the
/// rejecting paths through it would take the test process with it -- and the
/// rejecting paths are the ones worth testing, since the whole point of this
/// option is that a silently-ignored press produces a golden of an idle title
/// screen, which is precisely the artifact it exists to stop being mistaken for
/// a run.
fn parse_press_start_spec(spec: &str) -> Option<(u64, u64)> {
    let (a, b) = spec.split_once(':')?;
    let a: u64 = a.parse().ok()?;
    let b: u64 = b.parse().ok()?;
    // An empty or inverted window is refused rather than clamped: `B == A` holds
    // START for zero frames, which is indistinguishable from not passing the
    // flag at all.
    (b > a).then_some((a, b))
}

/// Parse a `--press-start A:B` spec, or exit.
fn parse_press_start(spec: &str) -> (u64, u64) {
    parse_press_start_spec(spec).unwrap_or_else(|| {
        eprintln!("--press-start A:B needs two frame numbers with B > A (got {spec:?})");
        usage()
    })
}

// A flat match over CLI flags, two lines past the limit since `--press-start`
// was added. Allowed rather than split: every arm assigns one of a dozen `mut`
// locals, so a helper would have to take them all by `&mut` and would be harder
// to read than the table it replaced -- and the two parses with real logic
// (`parse_press_start`, and the trace ranges) are already extracted. Same
// judgement as `Bus::unified_dma_cycle_impl` in the core.
#[allow(clippy::too_many_lines)]
fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (mut rom, mut out) = (None, None);
    let (mut seed, mut frames) = (0u64, 60u32);
    let (mut boot_trace, mut irq_trace) = (None, None);
    let mut fetch_trace: Option<usize> = None;
    let mut apu_trace: Option<usize> = None;
    let mut checkpoint_interval = rustynes_cosim::checkpoint::DEFAULT_INTERVAL;
    let (mut inject_instructions, mut inject_hold) = (0u64, 1u64);
    let (mut inject_nmi_at, mut inject_irq_at) = (None, None);
    let mut press_start: Option<(u64, u64)> = None;

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
            "--press-start" => {
                // `A:B` -- hold START on port 1 from frame A until frame B.
                // See `parse_press_start` for why a bad spec exits.
                press_start = Some(parse_press_start(need(i)));
                // Omitting this `i += 2` (see above: no trailing increment)
                // spun the parser at 100% CPU for thirteen minutes, never
                // reaching the simulation. The tell was the output directory
                // never being created.
                i += 2;
            }
            "--frames" => {
                frames = need(i).parse().unwrap_or_else(|_| usage());
                // Rejected at the boundary. A zero-frame run never reaches a
                // frame boundary, so an armed APU trace is never drained, and
                // `write_apu_trace`'s emptiness invariant then fires with a
                // message blaming "some run path" for what is plain invalid
                // input. A panic is the wrong report for a bad argument.
                if frames == 0 {
                    eprintln!("--frames must be at least 1 (got 0)");
                    usage();
                }
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
                fetch_trace = Some(parse_fetch_cap(need(i)));
                i += 2;
            }
            "--apu-trace" => {
                apu_trace = Some(parse_apu_cap(need(i)));
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
        press_start,
        boot_trace,
        irq_trace,
        fetch_trace,
        apu_trace,
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
    // The APU trace is drained at FRAME boundaries, in `advance_frames`, and an
    // injection run steps instructions and completes no frames -- so the two
    // together would write a well-formed, EMPTY `.apu.bin` with a dropped count
    // of zero. That is the exact shape this project keeps catching: a golden
    // whose emptiness is indistinguishable from a run that produced nothing.
    // Refused here rather than papered over with a warning, because a warning
    // on stderr is a signal that may never arrive.
    if args.apu_trace.is_some() && args.inject_instructions > 0 {
        return Some(
            "--apu-trace needs a frame run: the channel-level trace is drained at \
             frame boundaries, and an injection run completes no frames, so the \
             golden would be silently empty"
                .to_owned(),
        );
    }
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

/// Parse and validate `--fetch-trace`'s capacity.
///
/// Validated HERE, at the boundary, rather than left to clamp silently inside
/// the trace. `FetchTrace` caps its own storage so no argument can make it
/// allocate without bound -- but a clamp the caller never learns about produces
/// a golden covering less than the run, and the whole point of the drop counter
/// is that such a golden must not pass unnoticed. Refusing the argument says so
/// before a single cycle is simulated.
fn parse_fetch_cap(raw: &str) -> usize {
    let cap: usize = raw.parse().unwrap_or_else(|_| usage());
    if cap == 0 || cap > rustynes_core::rustynes_ppu::fetch_trace::MAX_CAPACITY {
        eprintln!(
            "--fetch-trace must be between 1 and {} (got {cap}); a window \
             needing more than that wants to be shorter, not buffered \
             larger -- three frames of a rendering ROM is under ten thousand",
            rustynes_core::rustynes_ppu::fetch_trace::MAX_CAPACITY
        );
        usage();
    }
    cap
}

/// Parse and validate `--apu-trace`'s capacity.
///
/// Extracted from `parse_args` to keep that function under the line limit, and
/// validated at the boundary rather than clamped later: the cap is in RECORDS
/// and one record is one CPU cycle, so a 24-frame run wants ~715,000. A
/// capacity smaller than the run yields a well-formed SHORTER file, which is
/// indistinguishable from a shorter run -- the failure this project keeps
/// catching, and the reason the drop counter exists at all.
fn parse_apu_cap(raw: &str) -> usize {
    let cap: usize = raw.parse().unwrap_or_else(|_| usage());
    if cap == 0 || cap > rustynes_cosim::MAX_APU_TRACE_CAPACITY {
        eprintln!(
            "--apu-trace must be between 1 and {} records (got {cap}); one \
             record is one CPU cycle, so a 24-frame run wants ~715,000. An \
             unbounded value reaches Vec::with_capacity directly and aborts \
             the process in the allocator.",
            rustynes_cosim::MAX_APU_TRACE_CAPACITY
        );
        usage();
    }
    cap
}

/// Write rung 4's per-CPU-cycle channel-level golden, when armed.
///
/// Fails loudly on a dropped record for the same reason `write_fetch_trace`
/// does: a truncated golden is not a smaller golden. The comparator sees a
/// length mismatch and reports a divergence whose real cause is an export-side
/// capacity, which is a divergence about the wrong thing.
fn write_apu_trace(o: &mut Oracle, base: &Path) {
    if let Some((bytes, dropped)) = o.take_apu_trace() {
        // ARMED BUT EMPTY is a defect, not a short run. `injection_error`
        // already refuses the one combination known to produce it, and this is
        // the backstop for the ones nobody has thought of yet: any future path
        // that arms the trace and never drains it would otherwise ship a
        // 0-byte golden and report success, because `dropped` stays 0.
        //
        // Defence in depth, deliberately -- the guard upstream states the rule
        // and this states the invariant, and the two fail for different
        // reasons. Raised in review after the upstream guard was already in.
        assert!(
            !bytes.is_empty(),
            "the APU trace was armed and produced no records -- a 0-byte golden \
             reports success while covering nothing. Some run path armed the \
             trace without draining it at a frame boundary."
        );
        // Checked BEFORE writing, not after. Writing a golden and then exiting
        // non-zero leaves a truncated `.bin` on disk that looks like every
        // other golden, and the next run of a gate against it compares a window
        // shorter than the manifest claims. The exit code fails a pipeline; the
        // file outlives it.
        if dropped > 0 {
            eprintln!(
                "  ERROR: apu trace dropped {dropped} record(s) -- --apu-trace \
                 capacity is too small, so the golden covers less than the run. \
                 No file written."
            );
            std::process::exit(1);
        }
        write(&suffixed(base, "apu.bin"), &bytes);
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
            // FAILS, rather than warning. A truncated golden is not a smaller
            // golden: the DUT captures the whole run, so the comparator sees a
            // length mismatch and reports a divergence whose real cause is an
            // export-side capacity. Worse, this project has already been bitten
            // by a warning that fired on every run and was never seen, because
            // the caller redirected stderr -- so a warning here is a signal that
            // may not arrive. The file is written FIRST and kept: its records
            // are real, and a partial capture is still worth inspecting by hand.
            eprintln!(
                "  ERROR: fetch trace dropped {dropped} read(s) -- --fetch-trace \
                 capacity is too small, so the golden covers less than the run. \
                 The truncated file was written; re-run with a larger capacity."
            );
            std::process::exit(1);
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

/// Advance `total` frames, optionally holding START across `[a, b)`.
///
/// Split out of `main` so it stays under the line limit, and because the
/// segmentation is a statement about the run rather than about argument
/// handling: the press lands on the same frames the oracle's own
/// `AccuracyCoin` runner uses -- idle, hold START, release, run on.
///
/// `advance_frames` is used for each leg rather than a `run_frame()` loop, for
/// the reason that function documents: the first call after power-on completes
/// no frame, so a leg counted in calls would be a leg short.
fn run_frames_with_optional_press(
    o: &mut Oracle,
    total: u64,
    press_start: Option<(u64, u64)>,
) -> u64 {
    const START: u8 = 1 << 3; // Buttons::START
    let Some((a, b)) = press_start else {
        return o.advance_frames(total);
    };
    // Clamped, so a window past the end of the run shortens rather than
    // underflowing `b - a` or `total - b`.
    let a = a.min(total);
    let b = b.min(total);
    let mut calls = o.advance_frames(a);
    o.set_buttons(0, START);
    calls += o.advance_frames(b - a);
    o.set_buttons(0, 0);
    calls += o.advance_frames(total - b);
    calls
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
    if let Some(cap) = args.apu_trace {
        o.enable_apu_trace(cap);
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
        run_frames_with_optional_press(&mut o, u64::from(args.frames), args.press_start)
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
    write_apu_trace(&mut o, &base);

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
    #[test]
    fn press_start_spec_accepts_a_well_formed_window() {
        assert_eq!(super::parse_press_start_spec("2:5"), Some((2, 5)));
        assert_eq!(super::parse_press_start_spec("0:1"), Some((0, 1)));
        // No upper bound is imposed here -- `run_frames_with_optional_press`
        // clamps to the run length, so a window past the end shortens rather
        // than being rejected at parse time.
        assert_eq!(
            super::parse_press_start_spec("100:4000000000"),
            Some((100, 4_000_000_000))
        );
    }

    #[test]
    fn press_start_spec_refuses_an_empty_or_inverted_window() {
        // `B == A` is the one most likely to be typed by accident, and it holds
        // START for zero frames -- indistinguishable from omitting the flag.
        assert_eq!(super::parse_press_start_spec("5:5"), None);
        assert_eq!(super::parse_press_start_spec("9:2"), None);
    }

    #[test]
    fn press_start_spec_refuses_malformed_input() {
        for bad in ["", "5", "5:", ":5", "a:5", "5:b", "5:5:5", "-1:5", "5 : 9"] {
            assert_eq!(
                super::parse_press_start_spec(bad),
                None,
                "should have refused {bad:?}"
            );
        }
    }

    /// `--apu-trace` with instruction injection must be REFUSED, not silently
    /// emptied.
    ///
    /// The trace drains at frame boundaries and an injection run completes no
    /// frames, so the combination would write a well-formed empty golden whose
    /// dropped count is zero -- indistinguishable from a run that produced
    /// nothing. This calls the real validator rather than restating its rule.
    #[test]
    fn apu_trace_with_injection_is_refused() {
        // Built explicitly rather than from a `Default`: `Args` has no
        // default and giving it one would invent a "valid" argument set that
        // no invocation produces.
        let base = || Args {
            rom: std::path::PathBuf::from("/dev/null"),
            out: std::path::PathBuf::from("/tmp"),
            seed: 0,
            frames: 1,
            press_start: None,
            boot_trace: None,
            irq_trace: None,
            fetch_trace: None,
            apu_trace: None,
            checkpoint_interval: 4096,
            inject_instructions: 0,
            inject_nmi_at: None,
            inject_irq_at: None,
            inject_hold: 1,
        };

        let mut args = base();
        args.apu_trace = Some(1000);
        args.inject_instructions = 8;
        args.inject_nmi_at = Some(2);
        let err = injection_error(&args).expect("the combination must be refused");
        assert!(
            err.contains("--apu-trace"),
            "message names the option: {err}"
        );

        // Neither half alone is refused for this reason: a frame run with the
        // trace armed is the normal case, and an injection run without it is
        // rung 2's sweep.
        let mut frames_only = base();
        frames_only.apu_trace = Some(1000);
        assert!(injection_error(&frames_only).is_none());

        let mut sweep_only = base();
        sweep_only.inject_instructions = 8;
        sweep_only.inject_nmi_at = Some(2);
        assert!(injection_error(&sweep_only).is_none());
    }

    use super::{Args, injection_error, sha256_hex, suffixed};
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
