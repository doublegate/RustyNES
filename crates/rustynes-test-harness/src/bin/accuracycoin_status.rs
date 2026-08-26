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

use std::path::{Path, PathBuf};
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

fn read_ram(p: &Path) -> Vec<u8> {
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

/// Entries that are `NotRun` on **both** sides.
///
/// The vacuity guard above catches an all-`NotRun` vector. It does not catch the
/// PARTIAL case, and v2.6.4 walked straight into it: this tool reported
/// "IDENTICAL entry for entry across all 146 entries" while **58 of the 146**
/// were `NotRun` on both sides, because the run window reached the CPU suites
/// and stopped. Two consoles agreeing about a test neither executed is not
/// evidence about that test, and the suites that went unasked were the APU and
/// PPU ones -- exactly what the preceding rungs exist for.
///
/// A comparison is only the rung-5 gate when the whole catalog EXECUTED, so this
/// is reported on every two-file run and refused when non-zero.
fn both_not_run(a: &[TestStatus], b: &[TestStatus]) -> usize {
    a.iter()
        .zip(b)
        .filter(|(x, y)| **x == TestStatus::NotRun && **y == TestStatus::NotRun)
        .count()
}

/// Entries that BOTH sides executed — neither is `NotRun`.
///
/// Not the complement of [`both_not_run`], and the difference is the whole
/// point. `len - both_not_run` counts entries executed on **at least one** side,
/// which is a different and much weaker statement: with the reference complete
/// and the DUT stalled after five entries, `both_not_run` is zero and that
/// subtraction claims all 146 ran on both sides. Caught in review of the change
/// that introduced it, on a release whose subject is a count that described a
/// set it did not measure.
fn executed_on_both(a: &[TestStatus], b: &[TestStatus]) -> usize {
    a.iter()
        .zip(b)
        .filter(|(x, y)| **x != TestStatus::NotRun && **y != TestStatus::NotRun)
        .count()
}

/// The coverage sentence, built rather than printed, so a test can read it.
///
/// The v2.6.4 review found this line claiming `len - both_not_run` entries had
/// "executed on both sides" — which is the count executed on **at least one**.
/// Fixing the arithmetic was not enough: a mutation reverting the line came back
/// NOT CAUGHT, because the tests asserted on the predicates and nothing reached
/// the message. The defect was in the sentence, so the sentence is what a test
/// has to be able to see.
fn coverage_line(a: &[TestStatus], b: &[TestStatus]) -> String {
    let dead = both_not_run(a, b);
    let both = executed_on_both(a, b);
    format!(
        "coverage: {both} of {} entries executed on both sides \
         ({dead} on neither, {} on one side only)",
        a.len(),
        a.len() - both - dead
    )
}

/// Report how much of the catalog the two runs actually EXECUTED, and refuse a
/// partial comparison.
///
/// Returns `Some(exit)` when the comparison must not proceed. Split out of
/// `main` so it can be tested directly: the property it enforces is the one
/// v2.6.4 found missing, and a check that only exists inside `main` is a check
/// no test can reach.
fn coverage_gate(a: &[TestStatus], b: &[TestStatus]) -> Option<ExitCode> {
    // Printed unconditionally so the number is visible even on a clean run -- a
    // reader who sees only "identical" has no way to tell how much of the
    // catalog that sentence covers.
    let dead = both_not_run(a, b);
    println!("\n{}", coverage_line(a, b));
    if dead == 0 {
        return None;
    }
    eprintln!(
        "\nPARTIAL: {dead} entries are NotRun on BOTH sides, so this comparison \
         says nothing about them. AccuracyCoin needs a long enough window to \
         reach the whole catalog -- 4500 frames executes all 146, where 600 \
         reaches only the CPU suites. Re-export the golden with more --frames; \
         agreement over a subset is not the rung-5 gate."
    );
    Some(ExitCode::from(4))
}

fn main() -> ExitCode {
    let args: Vec<PathBuf> = std::env::args_os().skip(1).map(PathBuf::from).collect();
    if args.is_empty() || args.len() > 2 {
        usage();
    }

    let decode = |p: &Path| -> Vec<TestStatus> {
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

    if let Some(code) = coverage_gate(&a, &b) {
        return code;
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

#[cfg(test)]
mod tests {
    use super::{both_not_run, coverage_gate, coverage_line, describe, executed_on_both, vacuous};
    use rustynes_test_harness::accuracy_coin_catalog::{TestStatus, catalog, decode_results};

    /// The SENTENCE, not the predicates behind it. Reverting the line to the
    /// subtraction the review flagged came back NOT CAUGHT while the tests only
    /// reached `both_not_run` and `executed_on_both` — the defect was in the
    /// message, so the message is what this asserts on.
    #[test]
    fn the_coverage_sentence_reports_what_it_claims() {
        let n = catalog().len();
        let reference = vec![TestStatus::Pass; n];
        let mut dut = vec![TestStatus::NotRun; n];
        for e in dut.iter_mut().take(5) {
            *e = TestStatus::Pass;
        }
        let line = coverage_line(&reference, &dut);
        assert!(
            line.starts_with(&format!(
                "coverage: 5 of {n} entries executed on both sides"
            )),
            "the sentence must say FIVE, not {n}: {line}"
        );
        assert!(
            line.contains("(0 on neither,"),
            "nothing is unrun on both sides here: {line}"
        );
        assert!(
            line.contains(&format!("{} on one side only)", n - 5)),
            "the rest ran on exactly one side: {line}"
        );
    }

    /// `len - both_not_run` is NOT the number both sides executed, and the
    /// v2.6.4 review caught the coverage line claiming it was. With the
    /// reference complete and the DUT stalled, the two differ by the whole run.
    #[test]
    fn executed_on_both_is_not_the_complement_of_both_not_run() {
        let n = catalog().len();
        let reference = vec![TestStatus::Pass; n];
        let mut dut = vec![TestStatus::NotRun; n];
        for e in dut.iter_mut().take(5) {
            *e = TestStatus::Pass;
        }

        assert_eq!(
            both_not_run(&reference, &dut),
            0,
            "the reference ran everything, so nothing is unrun on BOTH sides"
        );
        assert_eq!(
            executed_on_both(&reference, &dut),
            5,
            "only five entries ran on both sides"
        );
        assert_ne!(
            n - both_not_run(&reference, &dut),
            executed_on_both(&reference, &dut),
            "the subtraction the review flagged would have reported all of them"
        );
    }

    /// The refusal itself, not just its predicate. Reached directly because a
    /// check that only exists inside `main` is a check no test can reach --
    /// which is how the missing property got missed in the first place.
    #[test]
    fn a_partial_comparison_is_refused_and_a_full_one_is_not() {
        let n = catalog().len();
        let full = vec![TestStatus::Pass; n];
        assert!(
            coverage_gate(&full, &full).is_none(),
            "a comparison covering the whole catalog must proceed"
        );

        let mut a = vec![TestStatus::Pass; n];
        let mut b = vec![TestStatus::Pass; n];
        a[0] = TestStatus::NotRun;
        b[0] = TestStatus::NotRun;
        assert!(
            coverage_gate(&a, &b).is_some(),
            "one entry neither side ran is enough to refuse: agreement over a \
             subset is not the gate"
        );
    }

    /// The PARTIAL-coverage guard, added in v2.6.4 after the tool reported
    /// "IDENTICAL entry for entry across all 146 entries" over a comparison in
    /// which 58 of those entries were `NotRun` on both sides.
    #[test]
    fn entries_not_run_on_both_sides_are_counted() {
        let n = catalog().len();
        let mut a = vec![TestStatus::Pass; n];
        let mut b = vec![TestStatus::Pass; n];
        assert_eq!(both_not_run(&a, &b), 0, "two full vectors hide nothing");

        a[3] = TestStatus::NotRun;
        b[3] = TestStatus::NotRun;
        assert_eq!(both_not_run(&a, &b), 1, "one entry neither side ran");

        // NotRun on ONE side is a real disagreement, not dead coverage -- it is
        // the case the acceptance wording was written for and must not be
        // absorbed into the partial count.
        a[4] = TestStatus::NotRun;
        assert_eq!(
            both_not_run(&a, &b),
            1,
            "a one-sided NotRun is a difference, not missing coverage"
        );
    }

    /// `Skipped` is a result the ROM wrote, so a pair of them is coverage, not
    /// its absence -- the same distinction the vacuity guard draws.
    #[test]
    fn skipped_on_both_sides_is_not_missing_coverage() {
        let n = catalog().len();
        let a = vec![TestStatus::Skipped; n];
        let b = vec![TestStatus::Skipped; n];
        assert_eq!(
            both_not_run(&a, &b),
            0,
            "Skipped means the ROM reached the entry and declined it"
        );
    }

    /// The guard this tool exists for. A vector of nothing but `NotRun`
    /// describes a run that executed no tests, and reporting two of those as
    /// agreement is the failure mode the whole binary is built to refuse.
    #[test]
    fn an_all_not_run_vector_is_vacuous() {
        let v = vec![TestStatus::NotRun; catalog().len()];
        assert!(vacuous(&v), "a vector of only NotRun must be vacuous");
    }

    /// The other half, and the half a mutation to `all` would break silently:
    /// **one** real result is enough to make a vector non-vacuous. Without this
    /// an `any`-for-`all` swap still passes the test above.
    #[test]
    fn one_real_result_is_enough_to_be_non_vacuous() {
        let mut v = vec![TestStatus::NotRun; catalog().len()];
        v[0] = TestStatus::Pass;
        assert!(!vacuous(&v), "a single Pass must defeat the vacuity guard");

        let mut v = vec![TestStatus::NotRun; catalog().len()];
        *v.last_mut().expect("catalog is non-empty") = TestStatus::Fail(7);
        assert!(
            !vacuous(&v),
            "a single Fail must defeat the guard too -- a run that executed \
             tests and failed them is a real run"
        );
    }

    /// `Skipped` is a verdict the ROM writes deliberately (`$FF`), not an
    /// absence. A vector of skips is a run that happened, so it must NOT be
    /// refused as vacuous -- only `NotRun` means "never executed".
    #[test]
    fn skipped_is_not_the_same_as_never_run() {
        let v = vec![TestStatus::Skipped; catalog().len()];
        assert!(
            !vacuous(&v),
            "Skipped is a result the ROM wrote; only NotRun is an absence"
        );
    }

    /// An all-zero work RAM is what a run that never left the title screen
    /// actually looks like on disk, and it must decode to a vacuous vector.
    /// This pins the guard to the real input rather than to a hand-built
    /// vector -- `$00` is `NotRun`, and that link is what makes the guard fire.
    #[test]
    fn a_blank_work_ram_decodes_to_a_vacuous_vector() {
        let ram = vec![0u8; 2048];
        let v = decode_results(&ram).expect("2 KiB is long enough for the catalog");
        assert_eq!(v.len(), catalog().len());
        assert!(
            vacuous(&v),
            "blank work RAM is an idle run, not a passing one"
        );
    }

    /// Both decoded vectors are `catalog().len()` by construction, because
    /// `decode_results` maps over the catalog. The comparison in `main` zips
    /// three iterators and `zip` truncates silently, so that equal-length
    /// property is what keeps it from reporting agreement over a prefix while
    /// claiming the full count. Pinned here so a future change to
    /// `decode_results` that returns a shorter vector fails loudly.
    #[test]
    fn decoded_vectors_are_always_catalog_length() {
        for len in [2048usize, 4096] {
            let ram = vec![0u8; len];
            let v = decode_results(&ram).expect("long enough");
            assert_eq!(
                v.len(),
                catalog().len(),
                "decode_results must return one entry per catalog entry"
            );
        }
    }

    /// A short dump is refused rather than decoded into a short vector, which
    /// is what would make the `zip` above truncate.
    #[test]
    fn a_short_dump_is_refused() {
        assert!(decode_results(&[0u8; 8]).is_none());
    }

    /// The codes are what a reader acts on, so a status must not render as a
    /// bare variant name that drops its code.
    #[test]
    fn describe_carries_the_code() {
        assert_eq!(describe(TestStatus::Pass), "Pass");
        assert_eq!(describe(TestStatus::NotRun), "NotRun");
        assert_eq!(describe(TestStatus::Skipped), "Skipped");
        assert_eq!(describe(TestStatus::PassWithCode(1)), "Pass(code 1)");
        assert_eq!(describe(TestStatus::Fail(7)), "Fail(code 7)");
        assert_eq!(describe(TestStatus::Unknown(0xAB)), "Unknown($AB)");
    }
}
