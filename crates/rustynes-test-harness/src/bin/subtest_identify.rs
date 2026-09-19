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
    /// What to write in the manifest's `upstream_commit` column. The tool
    /// cannot observe which `AccuracyCoin` commit a ROM was built from -- that
    /// is the one fact a measurement cannot supply -- so it is passed
    /// in, defaulting to the value the 31 legacy ROMs carry.
    upstream: String,
    roms: Vec<String>,
}

const USAGE: &str = "usage: subtest_identify [--frames N] [--tsv] \
     [--upstream-commit S] [--] <rom.nes>...";

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
    let mut upstream = "legacy-unrecorded".to_string();
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
            "--upstream-commit" => {
                let Some(v) = it.next() else {
                    eprintln!("subtest_identify: --upstream-commit needs a value");
                    return Err(ExitCode::from(2));
                };
                if v.is_empty() || v.contains('\t') {
                    eprintln!("subtest_identify: --upstream-commit must be non-empty and tab-free");
                    return Err(ExitCode::from(2));
                }
                upstream = v;
            }
            // Everything after `--` is a path, whatever it looks like. Without
            // this there is no way to name a file whose own name begins with
            // `-`, because the arm below refuses those. Raised by the
            // Antigravity reviewer.
            "--" => {
                roms.extend(it.by_ref());
                break;
            }
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
    Ok(Args {
        frames,
        tsv,
        upstream,
        roms,
    })
}

fn main() -> ExitCode {
    let Args {
        frames,
        tsv,
        upstream,
        roms,
    } = match parse_args(env::args().skip(1)) {
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
        // The MANIFEST schema, exactly: eight fields in the order
        // `accuracycoin_subtest_provenance.rs`'s `parse_manifest` reads them.
        // It used to be nine, in a different order, with `final_byte` and
        // `extra` added and `upstream_commit` missing -- so the command this
        // repository documents for regenerating BUILD-PROVENANCE.tsv produced
        // something the gate rejects. A documented command that does not work
        // is worse than no documented command, because it reads as
        // reproducibility. Raised by CodeRabbit.
        //
        // `final_byte` is not lost: `verdict` is its decode, which is the form
        // the manifest records. `extra` moves to stderr as a warning, because a
        // ROM writing more than one result address is worth SAYING rather than
        // filing in a column nobody parses.
        println!("# {}", sub::MANIFEST_COLUMNS.join("\t"));
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
            .map_or((-1, -1), |(s, t)| (i32::from(s), i32::from(t)));

        let Some(first) = id.primary() else {
            eprintln!(
                "{stem}: wrote NOTHING in $0400-$04FF in {frames} frames -- \
                 unidentifiable, not reported as a result"
            );
            bad += 1;
            continue;
        };
        // AN ADDRESS IN NO CATALOG ENTRY IS A REFUSAL, NOT A ROW. Writing a
        // sentinel name here and accepting the same sentinel in the gate would
        // let a wrong ROM produce provenance that is self-consistent and
        // identifies nothing -- the two halves agreeing because they share a
        // placeholder, not because anything was checked. Raised by CodeRabbit.
        let Some(name) = by_addr.get(&first.addr).copied() else {
            eprintln!(
                "{stem}: writes ${:04X}, which belongs to NO scored catalog \
                 entry -- refusing to emit a row for it",
                first.addr
            );
            bad += 1;
            continue;
        };
        let status = cat::TestStatus::from_byte(first.final_byte);
        let extra: Vec<String> = id
            .hits
            .iter()
            .skip(1)
            .map(|h| format!("${:04X}", h.addr))
            .collect();
        if !extra.is_empty() {
            eprintln!(
                "{stem}: also wrote {} -- the ROM did not stop at its target entry",
                extra.join(",")
            );
        }
        if tsv {
            println!(
                "{stem}\t{es}\t{et}\t0x{:04X}\t{name}\t{status:?}\t{}\t{upstream}",
                first.addr, first.first_frame
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
