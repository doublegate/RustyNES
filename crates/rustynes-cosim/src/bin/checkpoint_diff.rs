//! Compare two checkpoint streams and locate the window a divergence is in.
//!
//! ```text
//! checkpoint_diff <reference.ckpt.bin> <candidate.ckpt.bin>
//! ```
//!
//! Exit status is the whole interface, because this runs in CI on both sides of
//! the boundary:
//!
//! | code | meaning |
//! |---|---|
//! | `0` | every checkpoint agreed |
//! | `1` | the streams diverged; the window is printed |
//! | `2` | usage or I/O error |
//! | `3` | **inconclusive** -- the comparison could not be made |
//!
//! `3` is separate from `0` on purpose and it is the point of the tool. A
//! truncated run, a DUT that stopped early, and two runs at different intervals
//! all produce "no divergence was found", and reporting that as agreement is
//! this project's recurring failure: an absence of signal read as a pass. A
//! green CI job must mean the streams were compared and matched, never that
//! there was nothing to compare.

use std::process::ExitCode;

use rustynes_cosim::checkpoint::{Comparison, InconclusiveReason, compare, from_bytes};

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let [reference, candidate] = argv.as_slice() else {
        eprintln!("usage: checkpoint_diff <reference.ckpt.bin> <candidate.ckpt.bin>");
        return ExitCode::from(2);
    };

    let load = |p: &str| match std::fs::read(p) {
        Ok(b) => from_bytes(&b).map_err(|e| format!("{p}: {e}")),
        Err(e) => Err(format!("{p}: {e}")),
    };
    let (a, b) = match (load(reference), load(candidate)) {
        (Ok(a), Ok(b)) => (a, b),
        (Err(e), _) | (_, Err(e)) => {
            eprintln!("error: {e}");
            return ExitCode::from(2);
        }
    };

    match compare(&a, &b) {
        Comparison::Identical { checkpoints } => {
            println!("checkpoints match: {checkpoints} compared, 0 divergences");
            ExitCode::SUCCESS
        }
        Comparison::Diverged(d) => {
            println!(
                "DIVERGED at checkpoint {}\n  \
                 window to re-run with full capture: cycles ({}, {}]  ({} cycles)\n  \
                 reference hash = {:#018x}\n  \
                 candidate hash = {:#018x}",
                d.index,
                d.after_cycle,
                d.through_cycle,
                d.window_len(),
                d.reference_hash,
                d.candidate_hash,
            );
            ExitCode::from(1)
        }
        Comparison::Inconclusive { reason } => {
            // Spelled out rather than printed as a debug struct: the operator
            // reading this in a CI log needs to know it is NOT a pass.
            let detail = match reason {
                InconclusiveReason::Empty => "one side produced no checkpoints at all".to_owned(),
                InconclusiveReason::LengthMismatch {
                    reference: r,
                    candidate: c,
                } => format!(
                    "the streams agree as far as they overlap, then one ends \
                     (reference {r} checkpoints, candidate {c}) -- one side stopped early"
                ),
                InconclusiveReason::UnalignedCycles { index } => format!(
                    "the cycle axes disagree at checkpoint {index}, so the hashes cover \
                     different spans and cannot be compared -- different intervals, or one \
                     side truncated mid-window"
                ),
            };
            println!("INCONCLUSIVE (this is not a pass): {detail}");
            ExitCode::from(3)
        }
    }
}
