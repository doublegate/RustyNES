//! Diagnostic: what does the oracle's `v` do across the v2.5.3 window?
//!
//! DIAGNOSTIC, never a gate. `ppu-state-trace` encodes `RustyNES`'s own model, so
//! it may explain a divergence and must never cause one -- see
//! `rung3-ppu.md` in the sibling repository. This exists because the v2.5.3 gate was
//! down to one coarse-X increment over a provably identical bus, and the only
//! remaining question was what the oracle's `v` and its rendering enable actually
//! do dot by dot.
//!
//! It found the answer immediately: the oracle increments at dots 112, 120, 128,
//! 136, 144 **and 152**, with `mask` still `$08` at 152. The sixth increment was
//! at the END of the window, not the start -- where three hypotheses had been
//! looking. `ppu2c02.sv` was applying a `$2001` write immediately; toggling
//! rendering takes effect about three dots after the write.
//!
//! Kept because the next PPU divergence will ask the same question, and because
//! it demonstrates the partition working: a diagnostic explained the failure and
//! never became a gate.
#![cfg(feature = "ppu-state-trace")]
// NOTE, recorded rather than fixed here: building with `--features
// ppu-state-trace` surfaces TWO pre-existing clippy findings in
// `rustynes-ppu` -- `tick_visible_render_fast` unused under the feature, and a
// collapsible `if` in `state_trace.rs`. No CI invocation enables this feature,
// so like `cpu-boot-trace` and `irq-timing-trace` before it, the gated code has
// never been linted (see AGENTS.md).
//
// They are NOT fixed in this release on purpose: `rustynes-ppu` is the emulation
// core, and touching it would turn v2.5.3 from "core untouched, AccuracyCoin
// inherited" into "core changed, AccuracyCoin re-verified". That is a real
// difference this project insists on stating, and it is not worth blurring for
// two lint findings that reach no shipped build.

use std::path::{Path, PathBuf};

use rustynes_core::Nes;
use rustynes_core::rustynes_ppu::state_trace::{PpuStateTrace, PpuTraceConfig};

#[test]
#[ignore = "diagnostic, run explicitly"]
fn scroll_window_v_progression() {
    // DERIVED, not a hand-counted `../`. The default is the sibling repository,
    // and an integration test's working directory is the PACKAGE root, not the
    // workspace root -- so a literal relative path needs three levels and reads
    // like a typo at any of them. `CARGO_MANIFEST_DIR` removes the guess.
    let rom_path = std::env::var("SCROLL_ROM").map_or_else(
        |_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(3)
                .expect("workspace parent")
                .join("RustyNES_MiSTer/tb/roms/ppuscroll.nes")
        },
        PathBuf::from,
    );
    let rom =
        std::fs::read(&rom_path).unwrap_or_else(|e| panic!("read {}: {e}", rom_path.display()));
    let mut nes =
        Nes::from_rom_with_power_on_seed(&rom, 0).unwrap_or_else(|e| panic!("load rom: {e:?}"));

    // The window the bus gate localised: frame 2, line 142, dots 90..200.
    let cfg = PpuTraceConfig {
        frame_range: 0..=3,
        scanline_range: Some(142..=142),
        dot_range: Some(90..=200),
    };
    nes.bus_mut()
        .ppu_mut()
        .enable_state_trace(PpuStateTrace::with_capacity(4096, cfg));

    // Gate on the FRAME COUNTER, never the call count: the first `run_frame`
    // after power-on advances zero cycles.
    while nes.frame() < 4 && !nes.is_jammed() {
        nes.run_frame();
    }

    let trace = nes
        .bus_mut()
        .ppu_mut()
        .take_state_trace()
        .expect("state trace was enabled");
    let recs = trace.records();
    assert!(!recs.is_empty(), "no records captured; the window is wrong");

    println!("captured {} records", recs.len());
    let mut last_v = None;
    for r in recs {
        let rendering = (r.mask & 0x18) != 0;
        let changed = last_v != Some(r.v);
        if changed || rendering {
            println!(
                "  f{} line{} dot{:>3}  v={:04X}  mask={:02X} rendering={}{}",
                r.frame,
                r.scanline,
                r.dot,
                r.v,
                r.mask,
                u8::from(rendering),
                if changed { "  <- v changed" } else { "" }
            );
        }
        last_v = Some(r.v);
    }
}
