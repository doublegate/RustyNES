// SPDX-License-Identifier: GPL-3.0-or-later
//! Where does a `$2001` write to the `Frozen OAM2 Increment` stimulus actually
//! land?
//!
//! # The question
//!
//! `Frozen OAM2 Increment` is the project's single remaining `AccuracyCoin`
//! failure, and `KNOWN_FAILING` records a specific cause: the sprite-zero hit
//! the entry detects with needs an opaque background pixel, `v` sits at fine-Y
//! **3** where the test needs **2**, and that is "because a `$2001` enable on
//! dot 256 takes effect a dot early", firing the dot-256 vertical increment the
//! ROM says must not fire.
//!
//! **That diagnosis is now in question.** `SCROLL_GATE_LAG` gives the dot-256
//! increment its own `$2001` depth, and it changes no verdict at depths 0..3 —
//! confirmed live by mutation (making its block never increment costs 14
//! tests). If the enable really landed a dot early, depth 2 would have skipped
//! the increment and moved something. So the claim has to be measured rather
//! than repeated.
//!
//! # What the ROM states
//!
//! `TEST_FrozenOAM2Inc` annotates every write with the dot it expects:
//!
//! | sub-test | write | ROM's stated dot |
//! |---|---|---|
//! | 2 | disable | scanline 194, dot **242** |
//! | 2 | enable | dot **256** ("but the PPU's vertical scroll is NOT incremented") |
//! | 3 | disable | scanline 196, dot **325** |
//! | 3 | enable | scanline 199, dot **256** |
//! | 4 | disable | scanline 196, dot **340** |
//! | 4 | enable | scanline 199, dot **256** |
//!
//! Test 4 is the false-positive guard and brackets dot 339 from the other side:
//! disabling at 340 leaves the dot-339 reset already done, so the flag is clear
//! and NO hit is expected.
//!
//! # Reading the output
//!
//! **`ppu-state-trace` records are END-OF-DOT** — the hook reads state after the
//! dot's effects. A record at dot `D` carrying the new mask means the write took
//! effect during `D`, so the first dot that *acts* on the new value is `D`
//! itself for anything sampling later in the dot, and `D + 1` for anything that
//! already sampled. Both are printed rather than one being chosen, because
//! collapsing them is how a one-dot gap was once read as two.
//!
//! Diagnostic, not a gate. Run:
//! `cargo test -p rustynes-test-harness --release --features test-roms,ppu-state-trace --test frozen_oam2_probe -- --ignored --nocapture`
#![cfg(all(feature = "test-roms", feature = "ppu-state-trace"))]

use std::path::{Path, PathBuf};

use rustynes_core::Nes;
use rustynes_core::rustynes_ppu::state_trace::{PpuStateRecord, PpuStateTrace, PpuTraceConfig};

/// Rendering bits of PPUMASK: show-background | show-sprites.
const RENDER_BITS: u8 = 0x18;

/// The dots the ROM names, so the report can mark a match without the reader
/// holding them in their head.
const STATED_DOTS: [u16; 4] = [242, 256, 325, 340];

/// Read a numeric env override, refusing a malformed value rather than
/// silently falling back — a probe reporting a different window than its
/// operator believes is worse than no probe.
fn env_or<T: std::str::FromStr>(key: &str, default: T) -> T
where
    <T as std::str::FromStr>::Err: std::fmt::Display,
{
    std::env::var(key).map_or(default, |v| {
        v.parse()
            .unwrap_or_else(|e| panic!("{key}={v:?} is not valid ({e}); refusing to run"))
    })
}

/// Every transition of PPUMASK's rendering bits in the captured window.
///
/// `prev` MUST reset per frame: the window starts mid-frame, so carrying it
/// across frames makes each window's first record compare against the previous
/// frame's last and report a transition that never happened.
fn report_transitions(recs: &[PpuStateRecord]) -> Vec<(u32, i16, u16)> {
    let mut prev: Option<u8> = None;
    let mut prev_frame: Option<u32> = None;
    let mut found = Vec::new();
    for r in recs {
        if prev_frame != Some(r.frame) {
            prev = None;
            prev_frame = Some(r.frame);
        }
        let now = r.mask & RENDER_BITS;
        if let Some(p) = prev
            && p != now
        {
            found.push((r.frame, r.scanline, r.dot));
            let kind = if now == 0 { "DISABLE" } else { "ENABLE " };
            let stated = STATED_DOTS
                .iter()
                .find(|d| r.dot.abs_diff(**d) <= 2)
                .map_or_else(String::new, |d| {
                    let delta = i32::from(r.dot) - i32::from(*d);
                    format!("  <- ROM states {d} (delta {delta:+})")
                });
            println!(
                "  f{:<4} line{:>4} dot{:>4}  {kind}  {p:02X}->{now:02X}  \
                 effect-in-dot {} / first-full-dot {}  fineY={}{stated}",
                r.frame,
                r.scanline,
                r.dot,
                r.dot,
                r.dot + 1,
                r.v >> 12
            );
        }
        prev = Some(now);
    }
    found
}

/// Dump every field that decides this entry, dot by dot, around one transition.
///
/// The transition dot alone cannot settle the question. A record at end-of-dot
/// 255 means the new mask is in force from the START of dot 256 — which is
/// exactly the dot the ROM names, not a dot early. What discriminates is
/// whether the dot-256 vertical increment actually FIRES: the ROM states it
/// must not. `v`'s fine-Y is that increment's visible effect, so this prints it
/// per dot and marks the step.
fn dump_around(recs: &[PpuStateRecord], frame: u32, line: i16, centre: u16) {
    let lo = centre.saturating_sub(env_or("FROZEN_DUMP_BACK", 4u16));
    let hi = (centre + env_or("FROZEN_DUMP_FWD", 8u16)).min(340);
    println!("\n  --- f{frame} line {line} dots {lo}..={hi} (end-of-dot state) ---");
    let mut prev_fine: Option<u16> = None;
    for r in recs
        .iter()
        .filter(|r| r.frame == frame && r.scanline == line && (lo..=hi).contains(&r.dot))
    {
        let fine = r.v >> 12;
        let coarse_y = (r.v >> 5) & 0x1F;
        let step = match prev_fine {
            Some(p) if p != fine => "  <== VERTICAL INCREMENT",
            _ => "",
        };
        println!(
            "    dot {:>3}  mask {:02X}  v={:04X}  fineY={fine}  cY={coarse_y}{step}",
            r.dot,
            r.mask & RENDER_BITS,
            r.v
        );
        prev_fine = Some(fine);
    }
}

/// Does the sprite-zero hit the ROM is waiting on ever land, and what is the
/// sprite line-up that should produce it?
///
/// Separated from the mask report because the freeze can be PERFECT and the
/// detector still miss — which is how this entry failed before — and because
/// test 4 fails by producing a hit that must not happen, so the hit list is
/// the only place its failure is visible at all.
fn report_sprite_zero(recs: &[PpuStateRecord]) {
    let mut prev_hit = false;
    let mut prev_frame = None;
    for r in recs {
        if prev_frame != Some(r.frame) {
            prev_hit = false;
            prev_frame = Some(r.frame);
        }
        let hit = r.status & 0x40 != 0;
        if hit && !prev_hit {
            println!(
                "  SPRITE-ZERO HIT  f{} line {} dot {}",
                r.frame, r.scanline, r.dot
            );
        }
        prev_hit = hit;
    }
    println!("  -- sprite line-up latched at dot 321 --");
    for r in recs
        .iter()
        .filter(|r| r.dot == 321 && r.spr_count > 0 && (194..=202).contains(&r.scanline))
    {
        println!(
            "    f{} line {}  spr_count={} zero_in_line={}  x={:?}",
            r.frame, r.scanline, r.spr_count, r.spr_zero_in_line, r.spr_x
        );
    }
}

#[test]
#[ignore = "diagnostic, run explicitly"]
fn frozen_oam2_rendering_transition_dots() {
    let rom_path = std::env::var("FROZEN_ROM").map_or_else(
        |_| {
            Path::new(env!("CARGO_MANIFEST_DIR"))
                .ancestors()
                .nth(2)
                .expect("workspace root")
                .join(
                    "tests/roms/AccuracyCoin/sub-tests/\
                     advanced-sprite-eval-frozen-oam2-increment.nes",
                )
        },
        PathBuf::from,
    );
    let rom =
        std::fs::read(&rom_path).unwrap_or_else(|e| panic!("read {}: {e}", rom_path.display()));
    let mut nes =
        Nes::from_rom_with_power_on_seed(&rom, 0).unwrap_or_else(|e| panic!("load rom: {e:?}"));

    // Whole scanlines, so a transition cannot fall outside a guessed dot window
    // — the ROM's four stated dots span 242..=340 but the writes that produce
    // them start earlier, and a probe armed to the wrong window reads the wrong
    // answer. Frames are bounded instead: the sub-test reaches its verdict at
    // frame 63.
    let frames: u32 = env_or("FROZEN_FRAMES", 80);
    let lo_line: i16 = env_or("FROZEN_LINE_LO", 185);
    let hi_line: i16 = env_or("FROZEN_LINE_HI", 210);
    println!("ROM    : {}", rom_path.display());
    println!("window : frames 0..={frames}, scanlines {lo_line}..={hi_line}, all dots");
    println!("records are END-OF-DOT: a record at dot D carrying the new mask");
    println!("means the write took effect DURING dot D.\n");

    // Arm the scroll gate when the study feature is compiled in. This is the
    // "run the mutation against the gate that can SEE it" check: the earlier
    // `SCROLL_GATE_LAG` refutation was measured on the BATTERY VERDICT, which
    // cannot distinguish "the knob suppressed the increment and something else
    // still fails" from "the knob is inert". This probe reads the increment.
    #[cfg(feature = "phi2-write-sweep")]
    {
        use core::sync::atomic::Ordering::Relaxed;
        let lag: u8 = env_or("FROZEN_SCROLL_LAG", u8::MAX);
        rustynes_core::rustynes_ppu::SCROLL_GATE_LAG.store(lag, Relaxed);
        let oam2: u8 = env_or("FROZEN_OAM2_LAG", 0u8);
        rustynes_core::rustynes_ppu::OAM2_GATE_LAG.store(oam2, Relaxed);
        let rend: u8 = env_or("FROZEN_RENDER_LAG", 1u8);
        rustynes_core::rustynes_ppu::RENDER_GATE_LAG.store(rend, Relaxed);
        println!("OAM2_GATE_LAG = {oam2}, RENDER_GATE_LAG = {rend}");
        if lag == u8::MAX {
            println!("SCROLL_GATE_LAG = follow-render-gate (shipped)\n");
        } else {
            println!("SCROLL_GATE_LAG = {lag}\n");
        }
    }

    let cfg = PpuTraceConfig {
        frame_range: 0..=frames,
        scanline_range: Some(lo_line..=hi_line),
        dot_range: None,
    };
    // Capacity DERIVED from the window, never a literal: a trace that silently
    // stops recording partway looks exactly like "the write never happened",
    // and a literal is how this project has mis-sized a capture before.
    let lines = usize::try_from(hi_line - lo_line + 1).expect("hi >= lo");
    let cap = (frames as usize + 1) * lines * 341 + 1024;
    println!("trace capacity: {cap} records");
    nes.bus_mut()
        .ppu_mut()
        .enable_state_trace(PpuStateTrace::with_capacity(cap, cfg));

    // Gate on the FRAME COUNTER, never the call count: the first `run_frame`
    // after power-on advances zero cycles.
    let start = nes.frame();
    while nes.frame() - start < u64::from(frames) {
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
        "the capture window produced NO records — the probe measured nothing, \
         which reads exactly like 'the write never happened'"
    );
    println!("captured {} records", recs.len());

    // The verdict, read from the ROM's own status byte. Suppressing the dot-256
    // increment is only interesting if it CLOSES the entry: `$01` is a clean
    // pass, and `(ErrorCode << 2) | 2` names the failing sub-test one-based, so
    // `$0A` is Fail(2). Scanned rather than read at a catalog address, because
    // the two differ.
    let ram = nes.bus().ram_bytes().to_vec();
    let verdict: Vec<String> = (0x0400..0x0500_usize)
        .filter(|a| ram[*a] != 0)
        .map(|a| {
            let b = ram[a];
            let name = match b {
                0x01 => "PASS".to_owned(),
                0xFF => "Skipped".to_owned(),
                _ if b & 3 == 2 => format!("Fail({})", b >> 2),
                _ => format!("raw {b:02X}"),
            };
            format!("${a:04X} = {b:02X} ({name})")
        })
        .collect();
    println!("\nVERDICT: {}", verdict.join(", "));

    report_sprite_zero(recs);

    let found = report_transitions(recs);
    println!("\n{} rendering transition(s) in the window", found.len());
    for (frame, line, dot) in &found {
        dump_around(recs, *frame, *line, *dot);
    }
    assert!(
        !found.is_empty(),
        "no PPUMASK rendering transition in the window at all — widen \
         FROZEN_LINE_LO/HI or FROZEN_FRAMES rather than reading this as 'the \
         ROM does not toggle rendering'"
    );
}
