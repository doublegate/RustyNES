//! The fast dot path must not change what the fetch trace records.
//!
//! A review of #450 raised this as a blocking correctness finding: that
//! `ppu-state-trace` disables `Ppu::tick_visible_render_fast` while
//! `ppu-fetch-trace` does not, so a build with only the fetch trace would run
//! the fast path and "bypass per-dot `read_vram` calls, silently dropping
//! background fetches".
//!
//! It does not, and the reason is where the hook was put. `read_vram` is the
//! single choke point every VRAM read passes through, and the fast path reaches
//! it by the same route the general path does — `fetch_nt`, `fetch_at`,
//! `fetch_bg_lo` and `fetch_bg_hi` all call it. `ppu-state-trace` has to
//! disable the fast path for a different reason: its hook fires per DOT, and the
//! fast path exists precisely to skip per-dot bookkeeping.
//!
//! That is an argument, and an argument is not evidence. This test is the
//! evidence, and it is here rather than in a reply so the property cannot
//! regress silently: run the same ROM with the fast path on and off, and require
//! byte-identical traces.
//!
//! This crate is excluded from the workspace and enables `ppu-fetch-trace`
//! unconditionally, so `cargo test` here is the only place this can run.

use rustynes_cosim::Oracle;

/// Resolved from `CARGO_MANIFEST_DIR` rather than written relative to the
/// working directory: `cargo test` runs from the package root, but a test
/// invoked another way does not, and a path that silently resolves to nothing
/// would make this test pass by skipping.
fn rom_path() -> std::path::PathBuf {
    std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("the cosim crate sits two levels below the repository root")
        .parent()
        .expect("the sibling repository is beside this one")
        .join("RustyNES_MiSTer/tb/roms/ppufetch.nes")
}

fn trace_with_fast_path(enabled: bool) -> (Vec<u8>, u64, u64) {
    let path = rom_path();
    let rom = std::fs::read(&path).unwrap_or_else(|e| panic!("{}: {e}", path.display()));
    let mut o = Oracle::new(&rom, 0).expect("ppufetch.nes must load");
    o.nes_mut().set_fast_dotloop(enabled);
    o.enable_fetch_trace(200_000);
    o.advance_frames(3);
    let hits = o.nes().bus().ppu().fast_path_hits;
    let (bytes, dropped) = o
        .take_fetch_trace()
        .expect("the trace was armed, so it must return");
    (bytes, dropped, hits)
}

#[test]
fn the_fast_dot_path_records_exactly_the_same_fetches() {
    let (fast, fast_dropped, fast_hits) = trace_with_fast_path(true);
    let (slow, slow_dropped, slow_hits) = trace_with_fast_path(false);

    // FIRST: prove the subject was reached. Without this the test passes when
    // the fast path is never entered, which is exactly how a check comes back
    // green having compared two runs of the same code. Measured: 11,755 dots
    // take it on this ROM with the flag on, and none with it off.
    assert!(
        fast_hits > 1000,
        "the fast path ran only {fast_hits} times — this test would then be \
         comparing the general path against itself and proving nothing"
    );
    assert_eq!(
        slow_hits, 0,
        "set_fast_dotloop(false) did not disable the fast path ({slow_hits} hits), \
         so both runs took the same route"
    );

    // Non-empty first. Two empty traces are byte-identical and prove nothing —
    // the same trap `fetch_diff.py` refuses in the DUT repository.
    assert!(
        fast.len() > 16 + 12 * 1000,
        "the fast-path trace holds only {} bytes; this ROM renders 24 scanlines \
         twice, so an almost-empty trace means the capture never armed",
        fast.len()
    );
    assert_eq!(
        fast_dropped, 0,
        "capacity was too small to compare anything"
    );
    assert_eq!(
        slow_dropped, 0,
        "capacity was too small to compare anything"
    );
    assert_eq!(
        fast.len(),
        slow.len(),
        "the fast path recorded {} bytes and the general path {} — the fast path \
         is dropping or adding fetches",
        fast.len(),
        slow.len()
    );
    assert!(
        fast == slow,
        "same ROM, same seed, same frames, different fetch traces: the fast dot \
         path changed what reaches `read_vram`"
    );
}
