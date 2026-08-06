// SPDX-License-Identifier: GPL-3.0-or-later
//! v2.3.1 "Plumb Line" — a harness-free frame-cost probe.
//!
//! ## Why this exists
//!
//! The criterion `full_frame` bench is the project's headline number and the
//! input to both CI gates and the PGO promotion gate — but it is a poor thing to
//! *profile*. A `perf record` of the bench binary attributes roughly **17% of
//! samples to criterion itself**: `rayon` plumbing for its parallel analysis,
//! `libm`'s `exp` from the distribution fitting, and its sorts. That noise sits
//! on top of every per-function percentage and silently skews attribution when
//! deciding which hot path to attack next.
//!
//! This probe runs the same workload with **no criterion in the process image**:
//! load a ROM, run frames in a tight steady-state loop, report wall-clock cost.
//! Profile *this* binary and every sample belongs to the emulator.
//!
//! It is deliberately NOT a replacement for the criterion suite. Criterion still
//! owns adopt/reject verdicts because it does the statistics properly; this owns
//! profiling and quick iteration.
//!
//! ## Host-quiet reporting
//!
//! A performance verdict measured on a contended machine is worse than no
//! verdict, because it looks like data. The v2.3.0 P1 campaign hit exactly this:
//! its first profile ran at **39% criterion outliers** and the second, on a quiet
//! host, at 2% — same code, same binary. So this probe reports spread
//! (median / p99 / a robust MAD-based coefficient of variation) alongside the
//! headline number and prints an explicit verdict on whether the host looked
//! quiet enough to trust. It never hides a noisy measurement behind a mean.
//!
//! ## Usage
//!
//! ```text
//! frame_probe                       # default corpus, 600 frames each
//! frame_probe --frames 1800         # longer steady state
//! frame_probe --rom path/to.nes     # explicit ROM (repeatable)
//! frame_probe --warmup 120          # frames discarded before timing
//! ```
//!
//! Typical profiling use:
//!
//! ```text
//! cargo build --release -p rustynes-test-harness --bin frame_probe --features test-roms
//! perf record -F 1200 --call-graph=dwarf -- \
//!     target/release/frame_probe --rom tests/roms/nestest/nestest.nes --frames 3000
//! perf report --no-children
//! ```

use std::path::PathBuf;
use std::time::Instant;

use rustynes_core::Nes;

/// Default corpus: the two ROMs the criterion `full_frame` bench and both CI
/// gates use, so the probe's numbers are directly comparable to the gate's.
/// `nestest` is the CPU/bus-leaning workload; `flowing_palette` is the
/// render-heavy one.
const DEFAULT_CORPUS: &[&str] = &[
    "tests/roms/nestest/nestest.nes",
    "tests/roms/assorted/flowing_palette.nes",
];

/// Frames discarded before timing starts, so the measurement covers steady
/// state rather than boot, first-frame allocation, and cold caches.
const DEFAULT_WARMUP: u32 = 120;

/// Timed frames per ROM.
const DEFAULT_FRAMES: u32 = 600;

/// One NTSC frame at 60.0988 Hz, milliseconds — the deadline every reported
/// figure is measured against.
const NTSC_FRAME_MS: f64 = 16.639;

/// Above this robust coefficient of variation the host is too noisy for the
/// numbers to support an adopt/reject decision. Chosen against the measured
/// back-to-back noise floor of ~0.7% on a quiet host (see
/// `scripts/bench_relative_check.sh`), with headroom so ordinary desktop jitter
/// does not cry wolf.
const QUIET_CV_PCT: f64 = 2.5;

/// Per-ROM timing summary. All values are nanoseconds per emulated frame.
struct Summary {
    label: String,
    median: f64,
    p99: f64,
    min: f64,
    /// Robust coefficient of variation: `1.4826 * MAD / median`, as a percent.
    /// Median-absolute-deviation rather than stddev because a handful of
    /// scheduler preemptions should not dominate the spread estimate.
    cv_pct: f64,
    frames: u32,
}

/// Nearest-rank percentile of an already-sorted slice.
///
/// Integer arithmetic rather than a float `ceil`, so there is no cast in either
/// direction: `rank = ceil(q_num * len / q_den)` computed exactly. `q_num/q_den`
/// is the quantile as a rational (e.g. 99/100 for p99), which is all this probe
/// ever needs and avoids the truncation/precision lints entirely.
fn percentile(sorted: &[f64], q_num: usize, q_den: usize) -> f64 {
    if sorted.is_empty() {
        return 0.0;
    }
    let rank = (q_num * sorted.len()).div_ceil(q_den);
    sorted[rank.saturating_sub(1).min(sorted.len() - 1)]
}

fn summarize(label: String, mut samples: Vec<f64>) -> Summary {
    samples.sort_by(f64::total_cmp);
    let median = percentile(&samples, 1, 2);
    let mut dev: Vec<f64> = samples.iter().map(|s| (s - median).abs()).collect();
    dev.sort_by(f64::total_cmp);
    let mad = percentile(&dev, 1, 2);
    let cv_pct = if median > 0.0 {
        1.4826 * mad / median * 100.0
    } else {
        0.0
    };
    Summary {
        label,
        median,
        p99: percentile(&samples, 99, 100),
        min: samples.first().copied().unwrap_or(0.0),
        cv_pct,
        frames: u32::try_from(samples.len()).unwrap_or(u32::MAX),
    }
}

/// Time `frames` steady-state frames of one ROM, returning per-frame ns.
fn probe(bytes: &[u8], warmup: u32, frames: u32) -> Result<Vec<f64>, String> {
    let mut nes = Nes::from_rom(bytes).map_err(|e| format!("{e:?}"))?;
    for _ in 0..warmup {
        nes.run_frame();
    }
    let mut samples = Vec::with_capacity(frames as usize);
    for _ in 0..frames {
        let t0 = Instant::now();
        let fb = nes.run_frame();
        // Keep the frame observably used so the optimizer cannot elide the work.
        // `Nes::framebuffer()` is a borrow, so this costs a length read.
        std::hint::black_box(fb.len());
        // `as_secs_f64() * 1e9` rather than `as_nanos() as f64`: a frame is far
        // below the f64-exact integer range either way, but this keeps the cast
        // lints satisfied without an allow.
        samples.push(t0.elapsed().as_secs_f64() * 1.0e9);
    }
    Ok(samples)
}

/// Parse a `u32` CLI count, exiting with a usage error rather than falling back
/// to a default. `require_positive` additionally rejects zero.
///
/// A measurement tool must not quietly substitute a different input than the one
/// it was asked for — the number it prints would then describe a run the caller
/// never requested. Concretely, `--frames 0` previously parsed, produced an
/// empty sample set, and reported a 0.00% CV ("host: QUIET"), a 0 ms median and
/// an infinite realtime multiplier: a confident-looking measurement of nothing.
/// Exit code 2 marks a usage error, distinct from a probe that ran.
fn parse_count(value: Option<&str>, flag: &str, require_positive: bool) -> u32 {
    let Some(raw) = value else {
        eprintln!("frame_probe: {flag} requires a value");
        std::process::exit(2);
    };
    let Ok(n) = raw.parse::<u32>() else {
        eprintln!("frame_probe: {flag} expects a non-negative integer, got {raw:?}");
        std::process::exit(2);
    };
    if require_positive && n == 0 {
        eprintln!("frame_probe: {flag} must be greater than zero");
        std::process::exit(2);
    }
    n
}

/// Workspace root, resolved from the **compile-time** manifest directory.
///
/// This is only used to locate the DEFAULT ROM corpus. `CARGO_MANIFEST_DIR` is
/// baked in at build time, so a binary copied away from its build tree resolves
/// to a path that no longer exists — which is why the default-corpus loop below
/// reports each missing ROM by path and then exits non-zero with
/// `no ROMs measured`, rather than silently measuring an empty set. Pass
/// `--rom <path>` explicitly when running a relocated binary.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root is two levels above the crate manifest")
        .to_path_buf()
}

fn main() {
    let mut frames = DEFAULT_FRAMES;
    let mut warmup = DEFAULT_WARMUP;
    let mut roms: Vec<PathBuf> = Vec::new();

    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        match arg.as_str() {
            // Reject rather than silently fall back to the default. `--frames 0`
            // used to be accepted and produced an empty sample set, which then
            // reported a 0.00% CV ("host: QUIET"), a 0 ms median, and an
            // infinite realtime multiplier — a confident-looking measurement of
            // nothing, which is the exact failure mode this probe exists to
            // avoid. A typo'd `--frames 60O` deserves the same treatment.
            "--frames" => frames = parse_count(args.next().as_deref(), "--frames", true),
            // Warmup MAY legitimately be zero, so only the parse is enforced.
            "--warmup" => warmup = parse_count(args.next().as_deref(), "--warmup", false),
            "--rom" => {
                if let Some(p) = args.next() {
                    roms.push(PathBuf::from(p));
                }
            }
            "--help" | "-h" => {
                println!(
                    "frame_probe [--frames N] [--warmup N] [--rom PATH]...\n\n\
                     Harness-free steady-state frame cost. Profile this binary\n\
                     instead of the criterion bench so samples are not diluted by\n\
                     criterion's own rayon/exp/sort work (~17% of the bench profile)."
                );
                return;
            }
            other => eprintln!("frame_probe: ignoring unknown argument {other:?}"),
        }
    }

    let root = workspace_root();
    if roms.is_empty() {
        roms = DEFAULT_CORPUS.iter().map(|r| root.join(r)).collect();
    }

    println!("frame_probe — {frames} timed frames/ROM after {warmup} warmup frames\n");

    let mut summaries = Vec::new();
    for rom in &roms {
        let label = rom
            .file_stem()
            .map_or_else(|| rom.display().to_string(), |s| s.to_string_lossy().into());
        let bytes = match std::fs::read(rom) {
            Ok(b) => b,
            Err(e) => {
                eprintln!("skip {}: {e}", rom.display());
                continue;
            }
        };
        match probe(&bytes, warmup, frames) {
            Ok(samples) => summaries.push(summarize(label, samples)),
            Err(e) => eprintln!("skip {}: {e}", rom.display()),
        }
    }

    if summaries.is_empty() {
        eprintln!("frame_probe: no ROMs measured");
        std::process::exit(1);
    }

    println!(
        "{:<24} {:>11} {:>11} {:>11} {:>8}",
        "workload", "median ms", "p99 ms", "min ms", "CV %"
    );
    for s in &summaries {
        println!(
            "{:<24} {:>11.4} {:>11.4} {:>11.4} {:>8.2}",
            s.label,
            s.median / 1.0e6,
            s.p99 / 1.0e6,
            s.min / 1.0e6,
            s.cv_pct
        );
    }

    // Host-quiet verdict. Reported, never silently folded into the numbers.
    let worst = summaries.iter().fold(0.0_f64, |a, s| a.max(s.cv_pct));
    println!();
    if worst <= QUIET_CV_PCT {
        println!(
            "host: QUIET (worst CV {worst:.2}% <= {QUIET_CV_PCT:.2}%) — numbers are usable for an A/B"
        );
    } else {
        println!(
            "host: NOISY (worst CV {worst:.2}% > {QUIET_CV_PCT:.2}%) — do NOT base an adopt/reject \
             decision on this run; close other work and re-measure"
        );
    }

    // NTSC frame budget context, so the number always carries its meaning.
    for s in &summaries {
        let ms = s.median / 1.0e6;
        println!(
            "  {:<22} {:>6.2}x realtime, {:>5.1}% of the {NTSC_FRAME_MS} ms NTSC budget ({} frames)",
            s.label,
            NTSC_FRAME_MS / ms,
            ms / NTSC_FRAME_MS * 100.0,
            s.frames
        );
    }
}
