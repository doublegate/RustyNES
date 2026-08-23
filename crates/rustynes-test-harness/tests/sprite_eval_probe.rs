//! Diagnostic: what does the oracle's sprite evaluation do, dot by dot?
//!
//! DIAGNOSTIC, never a gate. `ppu-state-trace` encodes `RustyNES`'s own model of
//! the evaluation FSM, so it may explain a divergence and must never cause one --
//! see `rung3-ppu.md` in the sibling repository. The gate this supports is
//! `$2004` read data on the CPU bus, which is pin-observable; these fields are
//! not.
//!
//! **Unreleased work.** The sprite-evaluation step is in progress in the sibling
//! repository and is NOT part of any shipped release; the current release is
//! v2.5.5 "Raster". This probe exists because that in-progress gate went red on
//! essentially every `$2004` read (1,284 of 59,993 compared cycles) and two
//! plausible fixes moved the count by six in opposite directions. That is
//! guessing with a gate attached. The v2.5.3 probe beside this one settled its
//! question in one run by asking what the oracle actually does rather than what
//! it plausibly does; this asks the same, and the gate is now green.
//!
//! Two environment variables steer it. `EV_SL` selects the scanline (default
//! 55); `EV_ALL` prints every dot instead of only the dots where the state
//! changes. The second exists because the transition filter is right for reading
//! and wrong for diffing -- a suppressed line and an absent line are the same
//! thing to a comparison script, and the sub-cycle offset between the two sides
//! can only be recovered from a table with no holes in it.
//!
//! Frame indices do NOT correspond between the two sides. The oracle's counter
//! and a testbench's differ because the first `run_frame()` after power-on
//! advances zero cycles, and this ROM does not enable rendering until its third
//! frame -- so the oracle's frame 1 has `mask = 00` and nothing to compare at
//! all. Pick the frame by what it CONTAINS, never by its number.
#![cfg(feature = "ppu-state-trace")]

use std::path::{Path, PathBuf};

use rustynes_core::Nes;
use rustynes_core::rustynes_ppu::state_trace::{PpuStateTrace, PpuTraceConfig};

#[test]
#[ignore = "diagnostic, run explicitly"]
fn sprite_evaluation_dot_by_dot() {
    let rom_path = std::env::var("SPRITE_ROM").map_or_else(
        |_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .expect("workspace parent")
                .join("RustyNES_MiSTer/tb/roms/ppusprite.nes")
        },
        PathBuf::from,
    );
    let rom =
        std::fs::read(&rom_path).unwrap_or_else(|e| panic!("read {}: {e}", rom_path.display()));
    let mut nes =
        Nes::from_rom_with_power_on_seed(&rom, 0).unwrap_or_else(|e| panic!("load rom: {e:?}"));

    // The bus gate's first divergence is CPU cycle 34,919, which is frame 1 a
    // little past scanline 45. Capture that whole scanline: the question is
    // which of the three phases disagrees, so the window must contain all three.
    let cfg = PpuTraceConfig {
        frame_range: 0..=3,
        scanline_range: Some(
            std::env::var("EV_SL")
                .ok()
                .and_then(|s| s.parse::<i16>().ok())
                .unwrap_or(55)
                ..=std::env::var("EV_SL")
                    .ok()
                    .and_then(|s| s.parse::<i16>().ok())
                    .unwrap_or(55),
        ),
        dot_range: Some(0..=340),
    };
    nes.bus_mut()
        .ppu_mut()
        .enable_state_trace(PpuStateTrace::with_capacity(4096, cfg));

    // Gate on the FRAME COUNTER, never the call count.
    // Bounded, and the bound is not decoration. Gating on `Nes::frame()` is
    // right -- the call count lies, because the first `run_frame` after power-on
    // advances zero cycles -- but a ROM that never completes a frame would spin
    // here forever. The jam check catches a `JAM`/`KIL`; the cap catches
    // everything else.
    let mut guard = 0u32;
    while nes.frame() < 4 && !nes.is_jammed() {
        nes.run_frame();
        guard += 1;
        assert!(
            guard < 1000,
            "ran {guard} frames without reaching frame 4: the ROM is not \
             advancing, and no amount of further running will fix that"
        );
    }

    let trace = nes
        .bus_mut()
        .ppu_mut()
        .take_state_trace()
        .expect("state trace was enabled");
    let recs = trace.records();
    assert!(!recs.is_empty(), "no records captured; the window is wrong");

    let oam = nes.bus().ppu().oam();
    println!(
        "OAM Y bytes (sprite 0..15): {:?}",
        (0..16).map(|n| oam[n * 4]).collect::<Vec<_>>()
    );
    println!("captured {} records", recs.len());
    let mut prev = None;
    for r in recs {
        let key = (
            r.oam_bus_copybuffer,
            r.mask,
            r.sprite_eval_n,
            r.sprite_eval_m,
            r.sprite_eval_found,
            r.sprite_eval_sec_idx,
            r.sprite_eval_copying,
            r.sprite_eval_overflow_search,
            r.sprite_eval_done,
            r.sprite_eval_read_latch,
        );
        // Only transitions, so the interesting dots are not buried under 341
        // lines of unchanged state.
        if std::env::var("EV_ALL").is_ok() || prev.as_ref() != Some(&key) {
            println!(
                "  f{} dot{:>3}  bus={:02X} mask={:02X} n={:>2} m={} found={} sec={:>2} copy={} ovf={} done={} latch={:02X}",
                r.frame,
                r.dot,
                r.oam_bus_copybuffer,
                r.mask,
                r.sprite_eval_n,
                r.sprite_eval_m,
                r.sprite_eval_found,
                r.sprite_eval_sec_idx,
                u8::from(r.sprite_eval_copying),
                u8::from(r.sprite_eval_overflow_search),
                u8::from(r.sprite_eval_done),
                r.sprite_eval_read_latch,
            );
            prev = Some(key);
        }
    }
}
