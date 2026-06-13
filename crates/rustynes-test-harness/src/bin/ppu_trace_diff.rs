//! Per-PPU-dot state-trace diff tool (Session-10 observability).
//!
//! Compares two binary [`PpuStateTrace`] files produced by either:
//!
//! * Our in-tree fixture (`ppu_state_trace_fixture` integration
//!   test or any caller of [`rustynes_ppu::Ppu::enable_state_trace`]).
//! * A Mesen2 Lua-script reference run
//!   (`scripts/mesen2_ppu_trace.lua`) emitting the same binary
//!   schema.
//!
//! Reports the first divergence (which `(frame, scanline, dot)`,
//! which field, expected vs actual) and optionally every
//! divergence within a window. Designed as the
//! "diff Mesen2 vs `RustyNES`" workflow described in
//! `docs/ppu-trace-tooling.md`.
//!
//! Build with the feature flag:
//!
//! ```bash
//! cargo build -p rustynes-test-harness --features ppu-state-trace \
//!     --bin ppu_trace_diff
//! ```
//!
//! Run:
//!
//! ```bash
//! ./target/debug/ppu_trace_diff \
//!     --reference /tmp/mesen2_inc_4014.bin \
//!     --actual    target/ppu_trace/accuracycoin_inc_4014.bin \
//!     --first-divergence
//! ```
//!
//! Exit codes:
//!
//! * `0` — traces are equivalent under the chosen comparator.
//! * `1` — divergence reported (normal).
//! * `2` — parse / file-I/O error.

#![cfg(feature = "ppu-state-trace")]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use rustynes_core::rustynes_ppu::state_trace::{PpuStateRecord, PpuStateTrace};

const USAGE: &str = "ppu_trace_diff - PPU state trace divergence reporter

USAGE:
    ppu_trace_diff --reference <FILE> --actual <FILE> [FLAGS] [OPTIONS]

REQUIRED ARGS:
    --reference <FILE>     Path to the reference binary trace
                           (e.g. Mesen2 .bin produced by mesen2_ppu_trace.lua)
    --actual    <FILE>     Path to the actual binary trace
                           (e.g. RustyNES .bin from the fixture)

FLAGS:
    --first-divergence     Stop after the first differing record (default)
    --all-divergences      Walk to the end of the shorter trace, listing
                           every differing record. Overrides --first-divergence.
    -h, --help             Print this help and exit.

OPTIONS:
    --max-reports <N>      With --all-divergences, cap report lines (default 20).
    --skip-fields <CSV>    Comma-separated field names to ignore in the
                           comparison (e.g. `--skip-fields oam_fnv1a64,t`).
                           Useful when one side intentionally omits a field.

EXAMPLES:
    ppu_trace_diff --reference mesen2.bin --actual ours.bin
    ppu_trace_diff --reference mesen2.bin --actual ours.bin --all-divergences \\
        --max-reports 5 --skip-fields oam_fnv1a64
";

#[derive(Debug)]
struct Args {
    reference: PathBuf,
    actual: PathBuf,
    mode: Mode,
    max_reports: usize,
    skip_fields: Vec<String>,
}

#[derive(Debug, Clone, Copy, Eq, PartialEq)]
enum Mode {
    First,
    All,
}

fn parse_args() -> Result<Args, String> {
    let mut argv = env::args().skip(1);
    let mut reference: Option<PathBuf> = None;
    let mut actual: Option<PathBuf> = None;
    let mut mode = Mode::First;
    let mut max_reports = 20usize;
    let mut skip_fields: Vec<String> = Vec::new();

    while let Some(arg) = argv.next() {
        match arg.as_str() {
            "-h" | "--help" => {
                print!("{USAGE}");
                std::process::exit(0);
            }
            "--reference" => {
                reference =
                    Some(PathBuf::from(argv.next().ok_or_else(|| {
                        "missing value for --reference".to_string()
                    })?));
            }
            "--actual" => {
                actual = Some(PathBuf::from(
                    argv.next()
                        .ok_or_else(|| "missing value for --actual".to_string())?,
                ));
            }
            "--first-divergence" => mode = Mode::First,
            "--all-divergences" => mode = Mode::All,
            "--max-reports" => {
                let v = argv
                    .next()
                    .ok_or_else(|| "missing --max-reports".to_string())?;
                max_reports = v.parse().map_err(|e| format!("--max-reports parse: {e}"))?;
            }
            "--skip-fields" => {
                let v = argv
                    .next()
                    .ok_or_else(|| "missing --skip-fields".to_string())?;
                skip_fields = v.split(',').map(str::to_string).collect();
            }
            other => return Err(format!("unknown argument: {other}")),
        }
    }

    let reference = reference.ok_or_else(|| "--reference is required".to_string())?;
    let actual = actual.ok_or_else(|| "--actual is required".to_string())?;
    Ok(Args {
        reference,
        actual,
        mode,
        max_reports,
        skip_fields,
    })
}

fn load_trace(path: &PathBuf) -> Result<PpuStateTrace, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    PpuStateTrace::from_binary(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Field-level diff between two records.
///
/// Returns a list of `(field_name, ref_repr, actual_repr)` tuples,
/// one per disagreement.  Empty list ⇒ records are byte-equal.
#[allow(clippy::too_many_lines, clippy::cognitive_complexity)] // Per-field comparison is tabular.
fn diff_record(
    r: &PpuStateRecord,
    a: &PpuStateRecord,
    skip: &[String],
) -> Vec<(&'static str, String, String)> {
    let skip = |f: &str| skip.iter().any(|s| s == f);
    let mut diffs: Vec<(&'static str, String, String)> = Vec::new();

    macro_rules! cmp_field {
        ($name:literal, $field:ident, $fmt:literal) => {
            if !skip($name) && r.$field != a.$field {
                diffs.push(($name, format!($fmt, r.$field), format!($fmt, a.$field)));
            }
        };
    }
    macro_rules! cmp_array {
        ($name:literal, $field:ident) => {
            if !skip($name) && r.$field != a.$field {
                diffs.push((
                    $name,
                    format!("{:02X?}", r.$field),
                    format!("{:02X?}", a.$field),
                ));
            }
        };
    }

    cmp_field!("frame", frame, "{}");
    cmp_field!("scanline", scanline, "{}");
    cmp_field!("dot", dot, "{}");
    cmp_field!("ctrl", ctrl, "${:02X}");
    cmp_field!("mask", mask, "${:02X}");
    cmp_field!("status", status, "${:02X}");
    cmp_field!("oam_addr", oam_addr, "${:02X}");
    cmp_field!("v", v, "${:04X}");
    cmp_field!("t", t, "${:04X}");
    cmp_field!("fine_x", fine_x, "{}");
    cmp_field!("w_toggle", w_toggle, "{}");
    cmp_field!("sprite_eval_n", sprite_eval_n, "{}");
    cmp_field!("sprite_eval_m", sprite_eval_m, "{}");
    cmp_field!("sprite_eval_found", sprite_eval_found, "{}");
    cmp_field!("sprite_eval_sec_idx", sprite_eval_sec_idx, "{}");
    cmp_field!("sprite_eval_copying", sprite_eval_copying, "{}");
    cmp_field!(
        "sprite_eval_overflow_search",
        sprite_eval_overflow_search,
        "{}"
    );
    cmp_field!("sprite_eval_done", sprite_eval_done, "{}");
    cmp_field!("sprite_eval_read_latch", sprite_eval_read_latch, "${:02X}");
    cmp_field!("spr_count", spr_count, "{}");
    cmp_field!("spr_zero_in_line", spr_zero_in_line, "{}");
    cmp_array!("spr_shift_lo", spr_shift_lo);
    cmp_array!("spr_shift_hi", spr_shift_hi);
    cmp_array!("spr_attr", spr_attr);
    cmp_array!("spr_x", spr_x);
    cmp_field!("bg_shift_lo", bg_shift_lo, "${:04X}");
    cmp_field!("bg_shift_hi", bg_shift_hi, "${:04X}");
    cmp_field!("at_shift_lo", at_shift_lo, "${:04X}");
    cmp_field!("at_shift_hi", at_shift_hi, "${:04X}");
    cmp_field!("nt_latch", nt_latch, "${:02X}");
    cmp_field!("at_latch", at_latch, "${:02X}");
    cmp_field!("bg_lo_latch", bg_lo_latch, "${:02X}");
    cmp_field!("bg_hi_latch", bg_hi_latch, "${:02X}");
    cmp_array!("secondary_oam", secondary_oam);
    cmp_field!("oam_fnv1a64", oam_fnv1a64, "${:016X}");
    cmp_field!("nmi_line", nmi_line, "{}");

    diffs
}

fn report_record_diff(r: &PpuStateRecord, a: &PpuStateRecord, diffs: &[(&str, String, String)]) {
    if diffs.is_empty() {
        return;
    }
    println!(
        "[diff @ frame={} scanline={} dot={}]",
        r.frame, r.scanline, r.dot
    );
    println!(
        "  (anchor: ref(frame={},scanline={},dot={}) vs actual(frame={},scanline={},dot={}))",
        r.frame, r.scanline, r.dot, a.frame, a.scanline, a.dot
    );
    for (name, lhs, rhs) in diffs {
        println!("    {name:30} ref={lhs:<20} actual={rhs}");
    }
}

fn run(args: &Args) -> Result<bool, String> {
    let reference = load_trace(&args.reference)?;
    let actual = load_trace(&args.actual)?;
    println!(
        "Loaded reference: {} records from {}",
        reference.len(),
        args.reference.display()
    );
    println!(
        "Loaded actual:    {} records from {}",
        actual.len(),
        args.actual.display()
    );

    // First check: anchor mismatch (different starting frame/
    // scanline/dot suggests capture windows are misaligned).
    if let (Some(rf), Some(af)) = (reference.records().first(), actual.records().first()) {
        if (rf.frame, rf.scanline, rf.dot) != (af.frame, af.scanline, af.dot) {
            println!(
                "WARNING: reference starts at ({},{},{}) but actual starts at ({},{},{}); \
                 alignment by record index may be misleading. Consider re-running both \
                 captures with identical PpuTraceConfig windows.",
                rf.frame, rf.scanline, rf.dot, af.frame, af.scanline, af.dot
            );
        }
    }

    let r_recs = reference.records();
    let a_recs = actual.records();
    let len = r_recs.len().min(a_recs.len());
    if len == 0 {
        println!("Both traces empty (or one is). Nothing to compare.");
        return Ok(true);
    }
    let mut reports = 0usize;
    let mut any_diff = false;
    for i in 0..len {
        let r = &r_recs[i];
        let a = &a_recs[i];
        let diffs = diff_record(r, a, &args.skip_fields);
        if diffs.is_empty() {
            continue;
        }
        any_diff = true;
        report_record_diff(r, a, &diffs);
        reports += 1;
        if args.mode == Mode::First {
            println!("(stopping at first divergence; pass --all-divergences to walk further)");
            return Ok(false);
        }
        if reports >= args.max_reports {
            println!("(stopping after {reports} report(s); raise --max-reports to see more)");
            return Ok(false);
        }
    }
    if r_recs.len() != a_recs.len() {
        println!(
            "Length mismatch: reference={} actual={} (compared first {} records)",
            r_recs.len(),
            a_recs.len(),
            len
        );
        any_diff = true;
    }
    if !any_diff {
        println!(
            "All {len} records match under the chosen comparator (skip-fields: {:?}).",
            args.skip_fields
        );
    }
    Ok(!any_diff)
}

fn main() -> ExitCode {
    let args = match parse_args() {
        Ok(a) => a,
        Err(e) => {
            eprintln!("error: {e}\n\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&args) {
        Ok(true) => ExitCode::from(0),
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("error: {e}");
            ExitCode::from(2)
        }
    }
}
