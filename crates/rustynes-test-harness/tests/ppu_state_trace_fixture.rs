//! Per-PPU-dot state-tracing fixture (Session-10 observability tooling).
//!
//! Drives `AccuracyCoin` through its boot + Start-press sequence with
//! the `ppu-state-trace` cargo feature enabled, then dumps a binary
//! [`PpuStateTrace`](rustynes_core::rustynes_ppu::state_trace::PpuStateTrace) to
//! `target/ppu_trace/accuracycoin_<window>.bin` for diff against a
//! Mesen2-emitted reference trace.
//!
//! See `docs/adr/0005-ppu-state-trace.md` for the design rationale,
//! `docs/ppu-trace-tooling.md` for usage, and `scripts/mesen2_ppu_trace.lua`
//! for the Mesen2-side reference generator.
//!
//! # Why this is its own test file
//!
//! Like `irq_trace_fixture.rs`, the trace is heavy (a 40-frame
//! visible-only capture is ~3.2 M records × 111 bytes ≈ 360 MB peak),
//! and we do not want it running on every `cargo test --workspace`.
//! It is gated behind TWO cargo features (`test-roms` + `ppu-state-trace`)
//! and is invoked explicitly:
//!
//! ```bash
//! cargo test -p rustynes-test-harness \
//!     --features test-roms,ppu-state-trace \
//!     --test ppu_state_trace_fixture
//! ```
//!
//! # Capture window
//!
//! The default window is frames 310..=320, visible-only. `AccuracyCoin`'s
//! test-runner Start press completes ~frame 306 (see
//! [`crate::accuracy_coin::run_battery_capturing_ram`]), so frame 310
//! lands on the first "tests are running" rendered frames — the
//! sprite-eval cascade investigation needs visibility on the
//! `INC $4014` test (catalog offset `0x0480`) and the `Arbitrary
//! Sprite zero` / `Misaligned OAM behavior` tests.
//!
//! Override the window via env vars for ad-hoc captures (the
//! integration test reads them on every run, so they can be set in CI
//! or locally without recompiling):
//!
//! * `RUSTYNES_PPU_TRACE_START_FRAME` (default `310`)
//! * `RUSTYNES_PPU_TRACE_END_FRAME` (default `320`)
//! * `RUSTYNES_PPU_TRACE_OUT` (default
//!   `target/ppu_trace/accuracycoin_default.bin`)
//! * `RUSTYNES_PPU_TRACE_SCANLINE_LO` / `_HI` (default `0` / `239`;
//!   visible-field default). Set both to `240` to capture
//!   `scanline 240` only — useful for diffing against Mesen2's
//!   per-frame `endFrame` Lua trace, which fires once per frame at
//!   PPU `(scanline=240, dot=0)`.
//! * `RUSTYNES_PPU_TRACE_DOT_LO` / `_HI` (default unset; full
//!   dot range when unset). Set both to `0` to filter to dot 0
//!   only, matching the Mesen2 Lua reference granularity.
//!
//! ### Per-frame comparison preset
//!
//! For diffing against Mesen2's per-frame Lua trace, use:
//! `RUSTYNES_PPU_TRACE_SCANLINE_LO=240 RUSTYNES_PPU_TRACE_SCANLINE_HI=240
//! RUSTYNES_PPU_TRACE_DOT_LO=0 RUSTYNES_PPU_TRACE_DOT_HI=0` — produces
//! one record per frame at the exact `(scanline, dot)` Mesen2's
//! endFrame callback fires at.

#![cfg(all(feature = "test-roms", feature = "ppu-state-trace"))]

use std::env;
use std::fs;
use std::path::PathBuf;

use rustynes_core::Nes;
use rustynes_core::rustynes_ppu::state_trace::{PpuStateTrace, PpuTraceConfig};

/// Per-fixture record cap. Sized at ~3 frames of visible-only capture
/// (≈ 720 k records) headroom past the default 10-frame window. Records
/// past the cap are silently dropped (`overflow` counter advances) —
/// generously sized so that doesn't happen in practice for the
/// documented default window.
const TRACE_CAPACITY: usize = 4_000_000;

/// Number of frames to advance through the `AccuracyCoin` splash
/// plus menu plus Start-press handshake before the trace turns on.
/// Mirrors the canonical battery driver in
/// [`crate::accuracy_coin::run_battery_capturing_ram`]: 300 splash
/// frames plus a 6-frame Start press, total 306.
const BOOT_FRAMES: u64 = 306;

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

fn rom_path() -> PathBuf {
    workspace_root()
        .join("tests")
        .join("roms")
        .join("accuracycoin")
        .join("AccuracyCoin.nes")
}

fn read_env_u32(key: &str, default: u32) -> u32 {
    env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn read_env_i16(key: &str, default: i16) -> i16 {
    env::var(key)
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default)
}

fn read_env_u16_opt(key: &str) -> Option<u16> {
    env::var(key).ok().and_then(|s| s.parse().ok())
}

#[test]
#[allow(clippy::too_many_lines)] // End-to-end fixture; readability beats decomposition.
fn accuracycoin_visible_window_emits_binary_trace() {
    use rustynes_core::Buttons;

    let start_frame = read_env_u32("RUSTYNES_PPU_TRACE_START_FRAME", 310);
    let end_frame = read_env_u32("RUSTYNES_PPU_TRACE_END_FRAME", 320);
    assert!(
        start_frame <= end_frame,
        "RUSTYNES_PPU_TRACE_START_FRAME ({start_frame}) must be <= END_FRAME ({end_frame})"
    );

    let out_path = env::var("RUSTYNES_PPU_TRACE_OUT").map_or_else(
        |_| {
            workspace_root()
                .join("target")
                .join("ppu_trace")
                .join("accuracycoin_default.bin")
        },
        PathBuf::from,
    );

    let bytes = fs::read(rom_path())
        .unwrap_or_else(|e| panic!("read AccuracyCoin.nes: {e} (path={:?})", rom_path()));
    let mut nes = Nes::from_rom(&bytes).expect("parse AccuracyCoin.nes (NROM)");

    // Boot: 300 splash frames + 6-frame Start press (matches
    // run_battery_capturing_ram). When `RUSTYNES_PPU_TRACE_RAW_BOOT=1`
    // is set, skip the splash + start-press pre-roll entirely and
    // capture from cold-boot frame 0 onward — used for Mesen2-vs-
    // RustyNES per-frame comparison runs where the Mesen2 Lua script
    // injects the Start press itself.
    let raw_boot = env::var("RUSTYNES_PPU_TRACE_RAW_BOOT")
        .ok()
        .filter(|s| s == "1" || s.eq_ignore_ascii_case("true"))
        .is_some();
    if raw_boot {
        println!("[ppu_state_trace_fixture] raw-boot mode: skipping splash + start-press");
    } else {
        for _ in 0..300 {
            nes.run_frame();
        }
        nes.set_buttons(0, Buttons::START);
        for _ in 0..6 {
            nes.run_frame();
        }
        nes.set_buttons(0, Buttons::empty());
        debug_assert_eq!(BOOT_FRAMES, 306);
    }

    // Enable tracing AFTER the Start press handshake — capturing the
    // 300-frame splash would just waste ~24 M records on
    // post-render scanlines with rendering disabled.
    //
    // Build the config: defaults capture every visible scanline
    // (0..=239) of every dot of every frame in the window. Env
    // vars `RUSTYNES_PPU_TRACE_SCANLINE_LO/HI` and
    // `RUSTYNES_PPU_TRACE_DOT_LO/HI` can narrow this further —
    // crucial for the per-frame Mesen2 reference trace mode,
    // which uses `SCANLINE_LO=SCANLINE_HI=240, DOT_LO=DOT_HI=0` to
    // emit a single record per frame at the Mesen2 endFrame point.
    let scan_lo = read_env_i16("RUSTYNES_PPU_TRACE_SCANLINE_LO", 0);
    let scan_hi = read_env_i16("RUSTYNES_PPU_TRACE_SCANLINE_HI", 239);
    let dot_lo = read_env_u16_opt("RUSTYNES_PPU_TRACE_DOT_LO");
    let dot_hi = read_env_u16_opt("RUSTYNES_PPU_TRACE_DOT_HI");
    let cfg = PpuTraceConfig {
        frame_range: start_frame..=end_frame,
        scanline_range: Some(scan_lo..=scan_hi),
        dot_range: match (dot_lo, dot_hi) {
            (Some(lo), Some(hi)) => Some(lo..=hi),
            (Some(lo), None) => Some(lo..=340),
            (None, Some(hi)) => Some(0..=hi),
            (None, None) => None,
        },
    };
    println!(
        "[ppu_state_trace_fixture] config: frames={}..={} scanlines={:?} dots={:?}",
        start_frame, end_frame, cfg.scanline_range, cfg.dot_range
    );
    let trace = PpuStateTrace::with_capacity(TRACE_CAPACITY, cfg);
    nes.bus_mut().ppu_mut().enable_state_trace(trace);

    // Run until we've passed the end-frame.  Each frame advances the
    // PPU's `frame` counter by 1; once it exceeds `end_frame + 1` we
    // know the buffer is fully populated.
    //
    // `end_frame + 2` upper bound is intentional: the `frame` counter
    // increments AT the start of the new frame, so to capture every
    // dot of `end_frame` we need to keep ticking past the start of
    // `end_frame + 1`.
    //
    // In raw-boot mode we also drive a Start press at user-specified
    // frames (defaults match the Mesen2 Lua script's defaults:
    // 300..=305) so the two emulators' input timing stays in lockstep.
    let start_press_lo = read_env_u32("RUSTYNES_PPU_TRACE_START_PRESS_LO", 300);
    let start_press_hi = read_env_u32("RUSTYNES_PPU_TRACE_START_PRESS_HI", 305);
    let frame_cap = u64::from(end_frame) + 2;
    while nes.bus().ppu().frame() < frame_cap {
        if raw_boot {
            let cur = nes.bus().ppu().frame();
            if cur >= u64::from(start_press_lo) && cur <= u64::from(start_press_hi) {
                nes.set_buttons(0, Buttons::START);
            } else {
                nes.set_buttons(0, Buttons::empty());
            }
        }
        nes.run_frame();
    }

    let trace = nes
        .bus_mut()
        .ppu_mut()
        .take_state_trace()
        .expect("trace was enabled above");
    println!(
        "[ppu_state_trace_fixture] captured frames={}..={} \
         records={} overflow={} (cap={})",
        start_frame,
        end_frame,
        trace.len(),
        trace.overflow(),
        TRACE_CAPACITY,
    );
    assert_eq!(
        trace.overflow(),
        0,
        "trace buffer overflowed — raise TRACE_CAPACITY or shrink the capture window"
    );

    // Some windows may legitimately produce zero records (e.g.
    // start > end via env-var typo); we surface this rather than
    // assert.  In the default window the `AccuracyCoin` test runner
    // has rendering ON throughout, so the visible-only filter
    // captures the full 240 scanlines * 341 dots * N frames.
    if trace.is_empty() {
        println!(
            "[ppu_state_trace_fixture] note: zero records captured in window \
             frames={start_frame}..={end_frame} (probably rendering disabled \
             or narrow scanline/dot filter)"
        );
    } else if env::var("RUSTYNES_PPU_TRACE_SCANLINE_LO").is_err()
        && env::var("RUSTYNES_PPU_TRACE_DOT_LO").is_err()
    {
        // Only apply the strict-visible-only sanity bound when the
        // caller is using the defaults. When env vars narrow the
        // scanline/dot range (e.g. for per-frame Mesen2-comparable
        // captures) the record count is intentionally tiny and the
        // bound becomes a hindrance.
        let expected_per_frame = 240usize * 341usize;
        let frames = (end_frame - start_frame + 1) as usize;
        // Sanity: the visible-only filter should produce roughly
        // frames * 240 scanlines * 341 dots. Permit a 5% margin to
        // absorb early/late partial-frame fragments.
        let expected = expected_per_frame * frames;
        let lo = expected * 95 / 100;
        let hi = expected * 105 / 100;
        assert!(
            (lo..=hi).contains(&trace.len()),
            "record count {} outside expected range {}..={} for {} frames \
             of visible-only capture",
            trace.len(),
            lo,
            hi,
            frames,
        );
    }

    // Write the binary trace.
    let out_dir = out_path
        .parent()
        .map_or_else(|| PathBuf::from("."), PathBuf::from);
    fs::create_dir_all(&out_dir)
        .unwrap_or_else(|e| panic!("create_dir_all {}: {e}", out_dir.display()));
    let binary = trace.to_binary();
    fs::write(&out_path, &binary).unwrap_or_else(|e| panic!("write {}: {e}", out_path.display()));
    println!(
        "[ppu_state_trace_fixture] wrote {} bytes to {}",
        binary.len(),
        out_path.display()
    );

    // Also write a small CSV preview (first 200 records) for
    // human inspection — large CSV exports are wasteful so we
    // truncate.
    let csv_path = out_path.with_extension("preview.csv");
    let csv = preview_csv(&trace, 200);
    fs::write(&csv_path, csv).unwrap_or_else(|e| panic!("write {}: {e}", csv_path.display()));
    println!(
        "[ppu_state_trace_fixture] wrote {}-record CSV preview to {}",
        trace.len().min(200),
        csv_path.display()
    );

    // Verify the binary roundtrips through the decoder so a future
    // capture is loadable by the diff tool.
    let parsed = PpuStateTrace::from_binary(&binary).expect("binary trace must parse");
    assert_eq!(parsed.len(), trace.len(), "roundtrip record count mismatch");
    if !trace.is_empty() {
        assert_eq!(
            parsed.records()[0],
            trace.records()[0],
            "roundtrip first-record mismatch"
        );
    }
}

/// Render the first `limit` records as CSV.  Used by the fixture to
/// emit a human-inspectable preview; full CSVs would be tens of MB
/// and rarely useful at that size.
fn preview_csv(trace: &PpuStateTrace, limit: usize) -> String {
    if trace.len() <= limit {
        return trace.to_csv();
    }
    // Build a sub-trace containing only the first `limit` records
    // and render that. We rebuild via the binary roundtrip path so
    // the column order is guaranteed identical to the full CSV.
    let cfg = trace.config().clone();
    let mut sub = PpuStateTrace::with_capacity(limit, cfg);
    for r in trace.records().iter().take(limit) {
        sub.maybe_push(r.clone());
    }
    sub.to_csv()
}
