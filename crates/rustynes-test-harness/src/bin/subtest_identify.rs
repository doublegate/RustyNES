//! Identify, by MEASUREMENT, which `AccuracyCoin` catalog entry a sub-test ROM runs.
//!
//! The reasoning — why a sub-test ROM's encoded `(suite, test)` cannot be
//! resolved against any suite map, and why the result address is the right
//! identity — lives on `rustynes_test_harness::accuracy_coin_subtest`, beside
//! the code both this binary and
//! `tests/accuracycoin_subtest_provenance.rs` call. It is deliberately not
//! restated here: a second copy would be correct on the day it was written.
//!
//! This is the generator for
//! `tests/roms/AccuracyCoin/sub-tests/BUILD-PROVENANCE.tsv`; that test is the
//! gate that re-measures it.
//!
//! ## Usage
//!
//! ```text
//! cargo run -p rustynes-test-harness --release --features test-roms \
//!   --bin subtest_identify -- [--frames N] [--tsv] <rom.nes>...
//! ```

use std::env;
use std::path::Path;
use std::process::ExitCode;

use rustynes_test_harness::accuracy_coin_catalog as cat;
use rustynes_test_harness::accuracy_coin_subtest as sub;

/// What the command line asked for.
struct Args {
    frames: u64,
    tsv: bool,
    roms: Vec<String>,
}

const USAGE: &str = "usage: subtest_identify [--frames N] [--tsv] <rom.nes>...";

/// Parse the command line, or print why it could not be parsed.
///
/// Extracted from `main` so the failure paths are one small function rather
/// than a third of the binary, and so a bad operand ends the run with a message
/// instead of a panic and a backtrace. The operator is not an untrusted-input
/// boundary, but a tool that aborts unreadably on a typo is one nobody trusts
/// the output of either. Raised by the Antigravity reviewer.
fn parse_args<I: Iterator<Item = String>>(mut it: I) -> Result<Args, ExitCode> {
    let mut frames: u64 = 900;
    let mut tsv = false;
    let mut roms: Vec<String> = Vec::new();
    while let Some(a) = it.next() {
        match a.as_str() {
            "--frames" => {
                let Some(v) = it.next() else {
                    eprintln!("subtest_identify: --frames needs a number");
                    return Err(ExitCode::from(2));
                };
                match v.parse::<u64>() {
                    Ok(n) if n > 0 => frames = n,
                    Ok(_) => {
                        eprintln!("subtest_identify: --frames must be at least 1");
                        return Err(ExitCode::from(2));
                    }
                    Err(e) => {
                        eprintln!("subtest_identify: --frames {v:?} is not a number: {e}");
                        return Err(ExitCode::from(2));
                    }
                }
            }
            "--tsv" => tsv = true,
            // An unrecognised flag is REFUSED rather than taken as a path. A
            // mistyped `--frame 10` would otherwise be read as two ROMs, and
            // the run would report two unidentifiable files instead of naming
            // the actual mistake.
            other if other.starts_with('-') => {
                eprintln!("subtest_identify: unknown option {other:?}");
                eprintln!("{USAGE}");
                return Err(ExitCode::from(2));
            }
            other => roms.push(other.to_string()),
        }
    }
    if roms.is_empty() {
        eprintln!("{USAGE}");
        return Err(ExitCode::from(2));
    }
    Ok(Args { frames, tsv, roms })
}

fn main() -> ExitCode {
    let Args { frames, tsv, roms } = match parse_args(env::args().skip(1)) {
        Ok(a) => a,
        Err(code) => return code,
    };

    let by_addr = match sub::scored_by_addr() {
        Ok(m) => m,
        Err(e) => {
            eprintln!("{e}");
            return ExitCode::from(3);
        }
    };

    if tsv {
        println!(
            "rom\tenc_suite\tenc_test\tresult_addr\tfinal_byte\tstatus\tfirst_frame\tentry\textra"
        );
    }
    let mut bad = 0u32;
    for path in &roms {
        let p = Path::new(path);
        let id = match sub::identify_path(p, frames) {
            Ok(i) => i,
            Err(e) => {
                eprintln!("{e}");
                bad += 1;
                continue;
            }
        };
        let stem = p
            .file_stem()
            .map_or_else(|| path.clone(), |s| s.to_string_lossy().into_owned());
        let (es, et) = id
            .encoded
            .map_or((-1i32, -1i32), |(s, t)| (i32::from(s), i32::from(t)));

        let Some(first) = id.primary() else {
            eprintln!(
                "{stem}: wrote NOTHING in $0400-$04FF in {frames} frames -- \
                 unidentifiable, not reported as a result"
            );
            bad += 1;
            continue;
        };
        let name = by_addr
            .get(&first.addr)
            .copied()
            .unwrap_or("<address in no catalog entry>");
        let status = cat::TestStatus::from_byte(first.final_byte);
        let extra: Vec<String> = id
            .hits
            .iter()
            .skip(1)
            .map(|h| format!("${:04X}", h.addr))
            .collect();
        if tsv {
            println!(
                "{stem}\t{es}\t{et}\t0x{:04X}\t0x{:02X}\t{status:?}\t{}\t{name}\t{}",
                first.addr,
                first.final_byte,
                first.first_frame,
                if extra.is_empty() {
                    "-".to_string()
                } else {
                    extra.join(",")
                }
            );
        } else {
            println!(
                "{stem:<46} enc={es}/{et:<3} ${:04X} = {name:<26} byte=0x{:02X} {status:?} \
                 frame={}{}",
                first.addr,
                first.final_byte,
                first.first_frame,
                if extra.is_empty() {
                    String::new()
                } else {
                    format!("  also-wrote={}", extra.join(","))
                }
            );
        }
    }
    if bad > 0 {
        eprintln!("{bad} ROM(s) could not be identified");
        return ExitCode::from(1);
    }
    ExitCode::SUCCESS
}
