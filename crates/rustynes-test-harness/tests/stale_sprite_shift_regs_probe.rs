//! Diagnostic: on which DOT does rendering actually transition for
//! `Stale Sprite Shift Regs` test 5, at each write placement?
//!
//! DIAGNOSTIC, never a gate. `ppu-state-trace` encodes `RustyNES`'s own model, so
//! it may explain a divergence and must never cause one.
//!
//! # Why this exists
//!
//! `Advanced Sprite Evaluation :: Stale Sprite Shift Regs` test 5 asks *"What
//! if we prepare the sprite counter but don't let the counter activate on dot
//! 339?"* and states the rule it checks:
//!
//! > "If the ppu is rendering on dot 339, then the shifter counters are set to
//! > 'counting'. If rendering was not enabled on dot 339, the shifter counters
//! > will be in whatever state they were previously in, which is likely
//! > 'halted'."
//!
//! The ROM also states its own timing model, which is the reason this probe is
//! worth running: `STA $2001` "begins on scanline 3, dot 325. Accounting for
//! the delay, rendering should be disabled around dot 334 or 335."
//!
//! THREE hypotheses about this test have already been refuted by patching
//! (gate quantisation, one-stage-less, the rendering-enable lag). This project's
//! own rule says the threshold for questioning the approach rather than reaching
//! for a fourth guess has been passed, so this MEASURES instead: it reports the
//! dot at which `mask`'s rendering bits actually change, and therefore whether
//! the effective gate falls before or after dot 339.
//!
//! That single fact decides whether the entry is a compensation for the write
//! placement (it closes when the placement moves) or an independent defect (it
//! does not, and phi2 buys nothing here).
//!
//! # Reading the output
//!
//! `rendering_enabled_delayed` is a ONE-dot pipeline, so the effective gate is
//! the reported mask-transition dot **+ 1**. A disable whose effect lands at
//! 339 or earlier halts the counters; later leaves them counting.
#![cfg(feature = "ppu-state-trace")]

use std::path::{Path, PathBuf};

use rustynes_core::Nes;
use rustynes_core::rustynes_ppu::state_trace::{PpuStateTrace, PpuTraceConfig};

/// Rendering bits of PPUMASK: show-background | show-sprites.
const RENDER_BITS: u8 = 0x18;

#[test]
#[ignore = "diagnostic, run explicitly"]
fn stale_sprite_shift_regs_rendering_transition_dot() {
    // DERIVED from the manifest dir rather than a hand-counted `../` chain: an
    // integration test's working directory is the PACKAGE root, not the
    // workspace root.
    let rom_path = std::env::var("STALE_ROM").map_or_else(
        |_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .expect("workspace root")
                .join("tests/roms/AccuracyCoin/sub-tests/ppu-misc-stale-sprite-shift-regs.nes")
        },
        PathBuf::from,
    );
    let rom =
        std::fs::read(&rom_path).unwrap_or_else(|e| panic!("read {}: {e}", rom_path.display()));
    let mut nes =
        Nes::from_rom_with_power_on_seed(&rom, 0).unwrap_or_else(|e| panic!("load rom: {e:?}"));

    // Optional placement override, so the SAME probe reports both
    // configurations. Absent = the shipped placement; a malformed value is
    // refused rather than silently ignored, because a probe that reports the
    // shipped path under a name claiming otherwise is worse than no probe.
    #[cfg(feature = "phi2-write-sweep")]
    if let Ok(v) = std::env::var("STALE_WRITE_OFFSET") {
        let n: u8 = v.parse().unwrap_or_else(|e| {
            panic!("STALE_WRITE_OFFSET={v:?} is not a u8 ({e}); refusing to run")
        });
        rustynes_core::rustynes_cpu::WRITE_PHI_OFFSET
            .store(n, core::sync::atomic::Ordering::Relaxed);
        let r: u8 = std::env::var("STALE_READ_OFFSET")
            .ok()
            .map(|v| v.parse().expect("STALE_READ_OFFSET is not a u8"))
            .unwrap_or(0);
        rustynes_core::rustynes_cpu::READ_PHI_OFFSET
            .store(r, core::sync::atomic::Ordering::Relaxed);
        println!("WRITE_PHI_OFFSET = {n}, READ_PHI_OFFSET = {r}");
    }
    #[cfg(not(feature = "phi2-write-sweep"))]
    println!("WRITE_PHI_OFFSET = 0 (shipped placement; feature off)");

    // The ROM names scanline 3 for test 5's writes. Capture the tail of that
    // line across a generous frame window rather than guessing the frame -- a
    // probe armed to the wrong frame reads the wrong answer, which this project
    // has already paid for once.
    let frames: u32 = std::env::var("STALE_FRAMES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(400);
    let cfg = PpuTraceConfig {
        frame_range: 0..=frames,
        scanline_range: Some(3..=3),
        dot_range: Some(300..=340),
    };
    nes.bus_mut()
        .ppu_mut()
        .enable_state_trace(PpuStateTrace::with_capacity(1 << 20, cfg));

    // Gate on the FRAME COUNTER, never the call count: the first `run_frame`
    // after power-on advances zero cycles.
    while nes.frame() <= u64::from(frames) && !nes.is_jammed() {
        nes.run_frame();
    }

    let trace = nes
        .bus_mut()
        .ppu_mut()
        .take_state_trace()
        .expect("state trace was enabled");
    let recs = trace.records();
    assert!(
        !recs.is_empty(),
        "no records captured -- the window is wrong, which is NOT the same as \
         'the write never happened'"
    );
    println!(
        "captured {} records on scanline 3, dots 300..=340",
        recs.len()
    );

    // Report every transition of the rendering bits within the window.
    // `prev` MUST reset per frame. The window starts at dot 300, so carrying it
    // across frames makes every window's first record compare against the
    // previous frame's last record and report a transition at dot 300 that
    // never happened -- an artifact of the instrument, not the console.
    let mut prev: Option<u8> = None;
    let mut prev_frame: Option<u32> = None;
    let mut transitions = 0usize;
    for r in recs {
        if prev_frame != Some(r.frame) {
            prev = None;
            prev_frame = Some(r.frame);
        }
        let now = r.mask & RENDER_BITS;
        if let Some(p) = prev
            && p != now
        {
            transitions += 1;
            let effective = r.dot + 1;
            println!(
                "  f{:<4} line{} dot{:>3}  mask {:02X} -> {:02X}  ({})  effective gate dot {} -> {} dot 339",
                r.frame,
                r.scanline,
                r.dot,
                p,
                now,
                if now == 0 { "DISABLE" } else { "ENABLE" },
                effective,
                if effective <= 339 {
                    "at or BEFORE"
                } else {
                    "AFTER"
                }
            );
        }
        prev = Some(now);
    }
    println!("rendering-bit transitions in window: {transitions}");
    assert!(
        transitions > 0,
        "ZERO transitions in the window. That is a finding about the PROBE, not \
         about the console -- widen the scanline/dot/frame window before reading \
         anything into it."
    );
}
