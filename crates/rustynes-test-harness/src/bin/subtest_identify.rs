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

fn main() -> ExitCode {
    let mut frames: u64 = 900;
    let mut tsv = false;
    let mut roms: Vec<String> = Vec::new();
    let mut args = env::args().skip(1);
    while let Some(a) = args.next() {
        match a.as_str() {
            "--frames" => {
                frames = args
                    .next()
                    .and_then(|v| v.parse().ok())
                    .expect("--frames needs a number");
            }
            "--tsv" => tsv = true,
            other => roms.push(other.to_string()),
        }
    }
    if roms.is_empty() {
        eprintln!("usage: subtest_identify [--frames N] [--tsv] <rom.nes>...");
        return ExitCode::from(2);
    }

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
