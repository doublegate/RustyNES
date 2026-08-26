//! Decode an `AccuracyCoin` work-RAM dump into its status vector, and optionally
//! compare two of them **entry for entry**.
//!
//! # Why this exists
//!
//! Rung 5's acceptance is not "the DUT's RAM matches". It is that an end-to-end
//! `AccuracyCoin` run "produces a status vector that can be compared entry-for-entry
//! against the oracle's" — and a byte comparison of 2 KiB of work RAM cannot do
//! that. It answers a different question, and answers it badly in both
//! directions:
//!
//! * it reports a difference in any scratch byte the suite happens to leave
//!   lying around as though a test had failed, and
//! * it reports **success** for two runs that both sat on the title screen and
//!   ran nothing, which is exactly how the first co-simulation run of this ROM
//!   read as a pass.
//!
//! The status vector is the thing with meaning: 146 catalog entries, each a byte
//! the ROM writes at a known address. Decoding it turns "2048 bytes differ" into
//! "these tests disagree, and here is what each side said".
//!
//! # The anti-vacuity guard is the point
//!
//! `fb_diff.py` refuses a reference framebuffer with fewer than eight distinct
//! values, because a uniform frame cannot distinguish a working renderer from a
//! broken one. The RAM comparison had no such guard and read as a pass on an
//! idle menu.
//!
//! So this tool refuses too: a vector that is entirely `NotRun` is reported as
//! **vacuous**, with a non-zero exit, whichever side it came from. A run that
//! executed nothing is not a passing run, and it must not be possible to
//! mistake one for the other.

use std::path::PathBuf;
use std::process::ExitCode;

use rustynes_test_harness::accuracy_coin_catalog::{
    TestStatus, catalog, decode_results, summarise,
};

fn usage() -> ! {
    eprintln!(
        "usage: accuracycoin_status <ram.bin> [<other-ram.bin>]\n\
         \x20 one file  -- decode and summarise that run's status vector\n\
         \x20 two files -- compare them ENTRY FOR ENTRY (first = reference)"
    );
    std::process::exit(2)
}

fn read_ram(p: &PathBuf) -> Vec<u8> {
    std::fs::read(p).unwrap_or_else(|e| {
        eprintln!("read {}: {e}", p.display());
        std::process::exit(2)
    })
}

fn describe(s: TestStatus) -> String {
    match s {
        TestStatus::NotRun => "NotRun".into(),
        TestStatus::Pass => "Pass".into(),
        TestStatus::PassWithCode(n) => format!("Pass(code {n})"),
        TestStatus::Fail(n) => format!("Fail(code {n})"),
        TestStatus::Skipped => "Skipped".into(),
        TestStatus::Unknown(b) => format!("Unknown(${b:02X})"),
    }
}

/// A vector with no test result at all describes a run that executed nothing.
/// Reporting that as agreement is the failure this tool exists to prevent.
fn vacuous(v: &[TestStatus]) -> bool {
    v.iter().all(|s| matches!(s, TestStatus::NotRun))
}

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.is_empty() || args.len() > 2 {
        usage();
    }

    let decode = |p: &PathBuf| -> Vec<TestStatus> {
        let ram = read_ram(p);
        decode_results(&ram).unwrap_or_else(|| {
            eprintln!(
                "{} is {} bytes -- too short to hold the result vector; \
                 pass the full 2 KiB work RAM",
                p.display(),
                ram.len()
            );
            std::process::exit(2)
        })
    };

    let a = decode(&args[0]);
    let sum = summarise(&a);
    println!(
        "{}: total={} pass={} pass_with_code={} fail={} skipped={} not_run={} unknown={}",
        args[0].display(),
        sum.total,
        sum.pass,
        sum.pass_with_code,
        sum.fail,
        sum.skipped,
        sum.not_run,
        sum.unknown
    );

    if vacuous(&a) {
        eprintln!(
            "\nVACUOUS: every one of the {} entries is NotRun. This run executed no \
             tests -- AccuracyCoin sits on its title screen until START is pressed. \
             Re-export with --press-start; a comparison against this proves nothing.",
            a.len()
        );
        return ExitCode::from(3);
    }

    let Some(second) = args.get(1) else {
        // Single-file mode: list anything that is not a clean pass, so the
        // interesting entries are visible without diffing against anything.
        let names: Vec<_> = catalog()
            .iter()
            .zip(&a)
            .filter(|(_, s)| !matches!(s, TestStatus::Pass))
            .map(|(e, s)| format!("  {:<44} {}", e.name, describe(*s)))
            .collect();
        if names.is_empty() {
            println!("every catalog entry is a clean Pass.");
        } else {
            println!("\nentries that are not a clean Pass ({}):", names.len());
            for l in names {
                println!("{l}");
            }
        }
        return ExitCode::SUCCESS;
    };

    let b = decode(second);
    if vacuous(&b) {
        eprintln!(
            "\nVACUOUS: {} has every entry NotRun -- see above.",
            second.display()
        );
        return ExitCode::from(3);
    }

    let diffs: Vec<_> = catalog()
        .iter()
        .zip(a.iter().zip(b.iter()))
        .filter(|(_, (x, y))| x != y)
        .map(|(e, (x, y))| {
            format!(
                "  {:<44} ref={:<16} actual={}",
                e.name,
                describe(*x),
                describe(*y)
            )
        })
        .collect();

    if diffs.is_empty() {
        println!(
            "\nstatus vectors are IDENTICAL entry for entry across all {} entries.",
            a.len()
        );
        ExitCode::SUCCESS
    } else {
        println!("\n{} of {} entries differ:", diffs.len(), a.len());
        for d in &diffs {
            println!("{d}");
        }
        ExitCode::FAILURE
    }
}
