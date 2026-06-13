//! Per-CPU-instruction boot-trace diff tool (Session-12 observability).
//!
//! Compares two binary
//! [`CpuBootTrace`](rustynes_core::cpu_boot_trace::CpuBootTrace) files
//! produced by either:
//!
//! * Our in-tree fixture (`cpu_boot_trace_fixture` integration test
//!   or any caller of [`Nes::enable_cpu_boot_trace`]).
//! * A Mesen2 Lua-script reference run
//!   (`scripts/mesen2_cpu_boot_trace.lua`) emitting the same binary
//!   schema.
//!
//! Reports the first divergence (which cycle, PC, opcode, field,
//! expected vs actual) and optionally every divergence within a
//! window.
//!
//! Exit codes:
//!
//! * `0` -- traces are equivalent under the chosen comparator.
//! * `1` -- divergence reported (normal).
//! * `2` -- parse / file-I/O error.

#![cfg(feature = "cpu-boot-trace")]

use std::env;
use std::fs;
use std::path::PathBuf;
use std::process::ExitCode;

use rustynes_core::cpu_boot_trace::{CpuBootRecord, CpuBootTrace};

const USAGE: &str = "cpu_boot_trace_diff - CPU boot-trace divergence reporter

USAGE:
    cpu_boot_trace_diff --reference <FILE> --actual <FILE> [FLAGS] [OPTIONS]

REQUIRED ARGS:
    --reference <FILE>     Path to the reference binary trace
                           (e.g. Mesen2 .bin produced by mesen2_cpu_boot_trace.lua)
    --actual    <FILE>     Path to the actual binary trace
                           (e.g. RustyNES .bin from the fixture)

FLAGS:
    --first-divergence     Stop after the first differing record (default)
    --all-divergences      Walk to the end of the shorter trace, listing
                           every differing record. Overrides --first-divergence.
    --align-by-cycle       Skip-ahead the shorter side to align records by
                           cycle counter rather than record index. Useful
                           when the two emulators emit different numbers
                           of records before a synchronization point.
    --context <N>          Print N records before and after each
                           divergence (default 5). Only with
                           --first-divergence.
    -h, --help             Print this help and exit.

OPTIONS:
    --max-reports <N>      With --all-divergences, cap report lines (default 20).
    --skip-fields <CSV>    Comma-separated field names to ignore (e.g.
                           `--skip-fields scanline,dot,flags`).

EXAMPLES:
    cpu_boot_trace_diff --reference mesen2.bin --actual ours.bin
    cpu_boot_trace_diff --reference mesen2.bin --actual ours.bin --align-by-cycle
    cpu_boot_trace_diff --reference mesen2.bin --actual ours.bin --all-divergences \\
        --max-reports 5 --skip-fields flags
";

#[derive(Debug)]
struct Args {
    reference: PathBuf,
    actual: PathBuf,
    mode: Mode,
    align_by_cycle: bool,
    context: usize,
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
    let mut align_by_cycle = false;
    let mut context = 5usize;
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
            "--align-by-cycle" => align_by_cycle = true,
            "--context" => {
                let v = argv.next().ok_or_else(|| "missing --context".to_string())?;
                context = v.parse().map_err(|e| format!("--context parse: {e}"))?;
            }
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
        align_by_cycle,
        context,
        max_reports,
        skip_fields,
    })
}

fn load_trace(path: &PathBuf) -> Result<CpuBootTrace, String> {
    let bytes = fs::read(path).map_err(|e| format!("read {}: {e}", path.display()))?;
    CpuBootTrace::from_binary(&bytes).map_err(|e| format!("parse {}: {e}", path.display()))
}

/// Minimal opcode disassembler: returns a short mnemonic + operand
/// description.  Covers the ~150 official + common unofficial opcodes
/// that boot-time RESET routines actually execute.  Unknown opcodes
/// fall through to `???`.
#[allow(clippy::too_many_lines)] // Big tabular opcode dispatch; readability beats decomposition.
fn disasm(rec: &CpuBootRecord) -> String {
    // Tabular dispatch: (mnemonic, addr-mode-bytes).
    let m = match rec.opcode {
        0x00 => ("BRK", 0),
        0x01 => ("ORA (zp,X)", 1),
        0x05 => ("ORA zp", 1),
        0x06 => ("ASL zp", 1),
        0x08 => ("PHP", 0),
        0x09 => ("ORA #", 1),
        0x0A => ("ASL A", 0),
        0x0D => ("ORA abs", 2),
        0x0E => ("ASL abs", 2),
        0x10 => ("BPL rel", 1),
        0x11 => ("ORA (zp),Y", 1),
        0x15 => ("ORA zp,X", 1),
        0x16 => ("ASL zp,X", 1),
        0x18 => ("CLC", 0),
        0x19 => ("ORA abs,Y", 2),
        0x1D => ("ORA abs,X", 2),
        0x1E => ("ASL abs,X", 2),
        0x20 => ("JSR abs", 2),
        0x21 => ("AND (zp,X)", 1),
        0x24 => ("BIT zp", 1),
        0x25 => ("AND zp", 1),
        0x26 => ("ROL zp", 1),
        0x28 => ("PLP", 0),
        0x29 => ("AND #", 1),
        0x2A => ("ROL A", 0),
        0x2C => ("BIT abs", 2),
        0x2D => ("AND abs", 2),
        0x2E => ("ROL abs", 2),
        0x30 => ("BMI rel", 1),
        0x31 => ("AND (zp),Y", 1),
        0x35 => ("AND zp,X", 1),
        0x36 => ("ROL zp,X", 1),
        0x38 => ("SEC", 0),
        0x39 => ("AND abs,Y", 2),
        0x3D => ("AND abs,X", 2),
        0x3E => ("ROL abs,X", 2),
        0x40 => ("RTI", 0),
        0x41 => ("EOR (zp,X)", 1),
        0x45 => ("EOR zp", 1),
        0x46 => ("LSR zp", 1),
        0x48 => ("PHA", 0),
        0x49 => ("EOR #", 1),
        0x4A => ("LSR A", 0),
        0x4C => ("JMP abs", 2),
        0x4D => ("EOR abs", 2),
        0x4E => ("LSR abs", 2),
        0x50 => ("BVC rel", 1),
        0x51 => ("EOR (zp),Y", 1),
        0x55 => ("EOR zp,X", 1),
        0x56 => ("LSR zp,X", 1),
        0x58 => ("CLI", 0),
        0x59 => ("EOR abs,Y", 2),
        0x5D => ("EOR abs,X", 2),
        0x5E => ("LSR abs,X", 2),
        0x60 => ("RTS", 0),
        0x61 => ("ADC (zp,X)", 1),
        0x65 => ("ADC zp", 1),
        0x66 => ("ROR zp", 1),
        0x68 => ("PLA", 0),
        0x69 => ("ADC #", 1),
        0x6A => ("ROR A", 0),
        0x6C => ("JMP (abs)", 2),
        0x6D => ("ADC abs", 2),
        0x6E => ("ROR abs", 2),
        0x70 => ("BVS rel", 1),
        0x71 => ("ADC (zp),Y", 1),
        0x75 => ("ADC zp,X", 1),
        0x76 => ("ROR zp,X", 1),
        0x78 => ("SEI", 0),
        0x79 => ("ADC abs,Y", 2),
        0x7D => ("ADC abs,X", 2),
        0x7E => ("ROR abs,X", 2),
        0x81 => ("STA (zp,X)", 1),
        0x84 => ("STY zp", 1),
        0x85 => ("STA zp", 1),
        0x86 => ("STX zp", 1),
        0x88 => ("DEY", 0),
        0x8A => ("TXA", 0),
        0x8C => ("STY abs", 2),
        0x8D => ("STA abs", 2),
        0x8E => ("STX abs", 2),
        0x90 => ("BCC rel", 1),
        0x91 => ("STA (zp),Y", 1),
        0x94 => ("STY zp,X", 1),
        0x95 => ("STA zp,X", 1),
        0x96 => ("STX zp,Y", 1),
        0x98 => ("TYA", 0),
        0x99 => ("STA abs,Y", 2),
        0x9A => ("TXS", 0),
        0x9D => ("STA abs,X", 2),
        0xA0 => ("LDY #", 1),
        0xA1 => ("LDA (zp,X)", 1),
        0xA2 => ("LDX #", 1),
        0xA4 => ("LDY zp", 1),
        0xA5 => ("LDA zp", 1),
        0xA6 => ("LDX zp", 1),
        0xA8 => ("TAY", 0),
        0xA9 => ("LDA #", 1),
        0xAA => ("TAX", 0),
        0xAC => ("LDY abs", 2),
        0xAD => ("LDA abs", 2),
        0xAE => ("LDX abs", 2),
        0xB0 => ("BCS rel", 1),
        0xB1 => ("LDA (zp),Y", 1),
        0xB4 => ("LDY zp,X", 1),
        0xB5 => ("LDA zp,X", 1),
        0xB6 => ("LDX zp,Y", 1),
        0xB8 => ("CLV", 0),
        0xB9 => ("LDA abs,Y", 2),
        0xBA => ("TSX", 0),
        0xBC => ("LDY abs,X", 2),
        0xBD => ("LDA abs,X", 2),
        0xBE => ("LDX abs,Y", 2),
        0xC0 => ("CPY #", 1),
        0xC1 => ("CMP (zp,X)", 1),
        0xC4 => ("CPY zp", 1),
        0xC5 => ("CMP zp", 1),
        0xC6 => ("DEC zp", 1),
        0xC8 => ("INY", 0),
        0xC9 => ("CMP #", 1),
        0xCA => ("DEX", 0),
        0xCC => ("CPY abs", 2),
        0xCD => ("CMP abs", 2),
        0xCE => ("DEC abs", 2),
        0xD0 => ("BNE rel", 1),
        0xD1 => ("CMP (zp),Y", 1),
        0xD5 => ("CMP zp,X", 1),
        0xD6 => ("DEC zp,X", 1),
        0xD8 => ("CLD", 0),
        0xD9 => ("CMP abs,Y", 2),
        0xDD => ("CMP abs,X", 2),
        0xDE => ("DEC abs,X", 2),
        0xE0 => ("CPX #", 1),
        0xE1 => ("SBC (zp,X)", 1),
        0xE4 => ("CPX zp", 1),
        0xE5 => ("SBC zp", 1),
        0xE6 => ("INC zp", 1),
        0xE8 => ("INX", 0),
        0xE9 => ("SBC #", 1),
        0xEA => ("NOP", 0),
        0xEC => ("CPX abs", 2),
        0xED => ("SBC abs", 2),
        0xEE => ("INC abs", 2),
        0xF0 => ("BEQ rel", 1),
        0xF1 => ("SBC (zp),Y", 1),
        0xF5 => ("SBC zp,X", 1),
        0xF6 => ("INC zp,X", 1),
        0xF8 => ("SED", 0),
        0xF9 => ("SBC abs,Y", 2),
        0xFD => ("SBC abs,X", 2),
        0xFE => ("INC abs,X", 2),
        _ => ("???", 0),
    };
    match m.1 {
        0 => m.0.to_string(),
        1 => format!("{} ${:02X}", m.0, rec.op1),
        2 => format!("{} ${:02X}{:02X}", m.0, rec.op2, rec.op1),
        _ => unreachable!(),
    }
}

/// Field-level diff between two records.
fn diff_record(
    r: &CpuBootRecord,
    a: &CpuBootRecord,
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
    cmp_field!("cycle", cycle, "{}");
    cmp_field!("frame", frame, "{}");
    cmp_field!("scanline", scanline, "{}");
    cmp_field!("dot", dot, "{}");
    cmp_field!("pc", pc, "${:04X}");
    cmp_field!("a", a, "${:02X}");
    cmp_field!("x", x, "${:02X}");
    cmp_field!("y", y, "${:02X}");
    cmp_field!("p", p, "${:02X}");
    cmp_field!("s", s, "${:02X}");
    cmp_field!("opcode", opcode, "${:02X}");
    cmp_field!("op1", op1, "${:02X}");
    cmp_field!("op2", op2, "${:02X}");
    cmp_field!("flags", flags, "${:02X}");
    diffs
}

fn print_record(rec: &CpuBootRecord, label: &str) {
    println!(
        "  {label:<6} cyc={:<7} PC=${:04X} A=${:02X} X=${:02X} Y=${:02X} P=${:02X} S=${:02X}  {}",
        rec.cycle,
        rec.pc,
        rec.a,
        rec.x,
        rec.y,
        rec.p,
        rec.s,
        disasm(rec)
    );
}

fn report_record_diff(r: &CpuBootRecord, _a: &CpuBootRecord, diffs: &[(&str, String, String)]) {
    if diffs.is_empty() {
        return;
    }
    println!(
        "[diff @ cycle={} PC=${:04X} frame={} scanline={} dot={}] {}",
        r.cycle,
        r.pc,
        r.frame,
        r.scanline,
        r.dot,
        disasm(r)
    );
    for (name, lhs, rhs) in diffs {
        println!("    {name:8} ref={lhs:<12} actual={rhs}");
    }
}

/// Align actual's first index with reference's first index by cycle
/// counter.  Returns `(ref_start_idx, actual_start_idx)`.  If the
/// first common cycle isn't found in either trace, returns `(0, 0)`.
fn align_by_cycle(reference: &CpuBootTrace, actual: &CpuBootTrace) -> (usize, usize) {
    let rf = reference.records();
    let af = actual.records();
    if rf.is_empty() || af.is_empty() {
        return (0, 0);
    }
    let first_ref_cycle = rf[0].cycle;
    let first_actual_cycle = af[0].cycle;
    // Find the first index in `actual` where cycle >= first_ref_cycle.
    let actual_start = af
        .iter()
        .position(|r| r.cycle >= first_ref_cycle)
        .unwrap_or(af.len());
    // And first index in `ref` where cycle >= first_actual_cycle.
    let ref_start = rf
        .iter()
        .position(|r| r.cycle >= first_actual_cycle)
        .unwrap_or(rf.len());
    if first_ref_cycle <= first_actual_cycle {
        (ref_start, 0)
    } else {
        (0, actual_start)
    }
}

#[allow(clippy::too_many_lines)] // End-to-end driver; readability beats decomposition.
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

    let r_recs = reference.records();
    let a_recs = actual.records();
    if let (Some(rf), Some(af)) = (r_recs.first(), a_recs.first()) {
        if rf.cycle != af.cycle && !args.align_by_cycle {
            println!(
                "NOTE: reference starts at cycle {} but actual starts at cycle {}; \
                 pass --align-by-cycle to skip-ahead.",
                rf.cycle, af.cycle
            );
        }
    }

    let (r_start, a_start) = if args.align_by_cycle {
        align_by_cycle(&reference, &actual)
    } else {
        (0, 0)
    };
    if args.align_by_cycle {
        println!(
            "Aligned by cycle: ref starts at idx {} (cyc {}), actual starts at idx {} (cyc {})",
            r_start,
            r_recs.get(r_start).map_or(0, |r| r.cycle),
            a_start,
            a_recs.get(a_start).map_or(0, |r| r.cycle),
        );
    }

    let r_view = &r_recs[r_start..];
    let a_view = &a_recs[a_start..];
    let len = r_view.len().min(a_view.len());
    if len == 0 {
        println!("Both traces empty (or one is). Nothing to compare.");
        return Ok(true);
    }
    let mut reports = 0usize;
    let mut any_diff = false;
    for i in 0..len {
        let r = &r_view[i];
        let a = &a_view[i];
        let diffs = diff_record(r, a, &args.skip_fields);
        if diffs.is_empty() {
            continue;
        }
        any_diff = true;
        report_record_diff(r, a, &diffs);
        if args.mode == Mode::First {
            // Print context window.
            let ctx = args.context.min(i);
            println!("  -- {ctx} instructions before divergence --");
            let lo = i.saturating_sub(ctx);
            for j in lo..i {
                print_record(&r_view[j], "ref");
                print_record(&a_view[j], "actual");
                println!();
            }
            println!("  -- divergence point --");
            print_record(r, "ref");
            print_record(a, "actual");
            println!();
            let hi = (i + args.context + 1).min(len);
            println!("  -- up to {} instructions after divergence --", hi - i - 1);
            for j in (i + 1)..hi {
                print_record(&r_view[j], "ref");
                print_record(&a_view[j], "actual");
                println!();
            }
            println!("(stopping at first divergence; pass --all-divergences to walk further)");
            return Ok(false);
        }
        reports += 1;
        if reports >= args.max_reports {
            println!("(stopping after {reports} report(s); raise --max-reports to see more)");
            return Ok(false);
        }
    }
    if r_view.len() != a_view.len() {
        println!(
            "Length mismatch: reference={} actual={} (compared first {} aligned records)",
            r_view.len(),
            a_view.len(),
            len
        );
        any_diff = true;
    }
    if !any_diff {
        println!(
            "All {len} aligned records match under the chosen comparator (skip-fields: {:?}).",
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
