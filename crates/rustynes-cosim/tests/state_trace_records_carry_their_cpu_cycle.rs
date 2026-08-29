//! The per-dot PPU state trace must stamp each record with the CPU cycle it
//! belongs to.
//!
//! Schema 4 added `cpu_cycle` for one reason: without it a record can only be
//! located by `frame`/`scanline`/`dot`, and none of those is comparable across
//! two consoles on its own. Diagnosing a single `AccuracyCoin` entry produced
//! three wrong conclusions in a row for exactly that reason — frames had to be
//! matched by what they CONTAIN, dots by a relationship measured separately on
//! each side, and cycles not at all.
//!
//! A field that is present but always zero would reinstate the whole problem
//! while looking fixed, which is why this test asserts the VALUES rather than
//! the column's existence. It is here rather than in `rustynes-ppu` because the
//! PPU cannot populate it: the bus stamps it once per CPU cycle, so only an
//! assembled console exercises the path.
#![cfg(feature = "ppu-state-trace")]

use rustynes_cosim::Oracle;

/// `nestest.nes`, committed in this repository under `tests/roms/`. Resolved
/// from `CARGO_MANIFEST_DIR` so the test cannot pass by failing to find its
/// subject when run from a different working directory.
fn rom() -> Vec<u8> {
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../tests/roms/nestest/nestest.nes");
    std::fs::read(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

#[test]
fn every_record_carries_a_cpu_cycle_that_advances_with_the_run() {
    let mut o = Oracle::new(&rom(), 0).expect("parse rom");
    // Two whole frames, unfiltered by scanline: enough dots that a stuck value
    // cannot hide behind a narrow window.
    o.enable_ppu_state_trace(200_000, 2..=3, None, None);
    o.advance_frames(5);

    let (csv, dropped) = o
        .take_ppu_state_trace_csv()
        .expect("the trace was armed, so it must produce a table");
    // A truncated capture would make every assertion below a statement about a
    // narrower window than the one asked for, so this is checked rather than
    // assumed -- the capacity is deliberately far above what five frames of two
    // scanlines can produce.
    assert_eq!(
        dropped, 0,
        "the trace dropped {dropped} record(s): the window is narrower than it asks for"
    );
    let mut lines = csv.lines();
    let header = lines.next().expect("header");
    let idx = header
        .split(',')
        .position(|c| c == "cpu_cycle")
        .expect("header names cpu_cycle");

    let cycles: Vec<u64> = lines
        .map(|l| {
            l.split(',')
                .nth(idx)
                .expect("row has a cpu_cycle column")
                .parse()
                .expect("cpu_cycle parses as a number")
        })
        .collect();

    assert!(
        !cycles.is_empty(),
        "the window produced no records, so this test asserts nothing"
    );

    // NOT merely non-zero. A field wired to a constant would pass that, and the
    // failure this guards is precisely a value that looks present and says
    // nothing.
    assert!(
        cycles.iter().any(|&c| c != 0),
        "every record carries cpu_cycle = 0, so the bus is not stamping it"
    );
    let (lo, hi) = (cycles[0], *cycles.last().expect("non-empty"));
    assert!(
        hi > lo,
        "cpu_cycle does not advance across the window: first {lo}, last {hi}"
    );

    // Monotonic, and never running ahead of the dots it labels: a record is
    // stamped BEFORE its cycle's dots are ticked, so consecutive records step
    // by 0 or 1 and never jump.
    for w in cycles.windows(2) {
        let step = w[1] - w[0];
        assert!(
            step <= 1,
            "cpu_cycle jumped by {step} between consecutive dots ({} -> {}); \
             each CPU cycle covers three dots, so the step is 0 or 1",
            w[0],
            w[1]
        );
    }
}
