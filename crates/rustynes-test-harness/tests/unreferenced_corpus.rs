//! The committed test ROMs that no test ran, gated by the strongest verdict
//! each one supports (v2.9.5 "Caliper").
//!
//! At v2.9.4, 59 committed `.nes` files under `tests/roms/` were referenced by
//! nothing in `crates/` or `scripts/`, counting a directory a test enumerates
//! as referenced. That inventory was measured, not taken from the plan: the
//! plan's own list (`scanline-a1`, `scrolltest`, `tvpassfail`, `window5`)
//! names ROMs that live in the gitignored `nes-test-roms/` working clone and
//! are not committed at all, so no CI test could run them.
//! Ten of the 59 are `extra/apu/apu_test_{1..10}`, which
//! `apu_frame_clock_coincidence.rs` gates. This file takes the other 49.
//!
//! Each ROM was run through every reader the harness has (the `$6000`
//! protocol, the on-screen decoder, the result-code decoder, and a settled
//! framebuffer) before this file decided how to gate it. They fall into four
//! kinds, and the kind decides the strength of the assertion:
//!
//! 1. **A real verdict (13 ROMs).** The eight `extra/ppu/ppu_spr_hit_*` ROMs and
//!    `instr_test-v3` singles 11-15 speak blargg's `$6000` protocol: status 0
//!    and a message ending in `Passed`. Asserted as a pass.
//! 2. **Holy Mapperel variants (23 ROMs).** Each prints the board it detected
//!    and a four-digit detail code, `WRAM · PRG ROM · IRQ · CHR`, where zero is
//!    normal (upstream README). The nametable carries the digits, so both are
//!    read as text and asserted. Every one reads `0000`.
//! 3. **A data report (1 ROM).** `extra/cpu/cpu_flag_concurrency.nes`
//!    (Joel Yliluoma) measures the frame-IRQ trigger timing and a BRK-IRQ
//!    collision table and asks the reader to send them in. It has no verdict,
//!    so its report is pinned as a snapshot. A change to it is a change in
//!    interrupt timing and must be looked at.
//! 4. **Visual or audio only (12 ROMs).** The nine audio ROMs in `extra/apu`
//!    and `ppu_color`, `ppu_palette` and `ppu_ntsc_torture` produce output meant
//!    for eyes and ears. They are pinned by framebuffer and audio hash. **These
//!    are regression pins, not accuracy verdicts**: a hash says the output did
//!    not change, never that it was right.

#![cfg(feature = "test-roms")]

mod common;

use std::fs;

use common::{rom_path, run_and_capture_full, snapshot_line_full};
use rustynes_core::Nes;
use rustynes_test_harness::run_nes_blargg;

/// Frames for the `$6000` ROMs. The slowest settles in under a third of it.
const BLARGG_FRAMES: u64 = 1200;

/// Frames before a Holy Mapperel result screen is read (as `holy_mapperel.rs`).
const HOLY_FRAMES: u64 = 600;

/// Frames for the pinned visual and audio ROMs.
const PIN_FRAMES: u64 = 600;

const BLARGG_PASS: &[&str] = &[
    "extra/ppu/ppu_spr_hit_basics.nes",
    "extra/ppu/ppu_spr_hit_alignment.nes",
    "extra/ppu/ppu_spr_hit_corners.nes",
    "extra/ppu/ppu_spr_hit_flip.nes",
    "extra/ppu/ppu_spr_hit_left_clip.nes",
    "extra/ppu/ppu_spr_hit_right_edge.nes",
    "extra/ppu/ppu_spr_hit_screen_bottom.nes",
    "extra/ppu/ppu_spr_hit_double_height.nes",
    "nes-test-roms/instr_test-v3/rom_singles/11-jmp_jsr.nes",
    "nes-test-roms/instr_test-v3/rom_singles/12-rts.nes",
    "nes-test-roms/instr_test-v3/rom_singles/13-rti.nes",
    "nes-test-roms/instr_test-v3/rom_singles/14-brk.nes",
    "nes-test-roms/instr_test-v3/rom_singles/15-special.nes",
];

#[test]
fn blargg_protocol_roms_pass() {
    let mut failures = Vec::new();
    for rel in BLARGG_PASS {
        let bytes = fs::read(rom_path(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let r = run_nes_blargg(&bytes, BLARGG_FRAMES).expect("rom must parse + run");
        // Status 0 alone is not a pass on a ROM without PRG-RAM, where `$6000`
        // reads back 0 unmapped; the message is what proves the protocol ran.
        if r.status != 0 || !r.message.contains("Passed") {
            failures.push(format!(
                "{rel}: status={} message={:?}",
                r.status, r.message
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `(ROM file stem, detected-board digits as printed)`. The detection is the
/// ROM's own finding from the board's behaviour, and the header's mapper
/// number is what it should find, with one exception documented below.
const HOLY: &[(&str, &str)] = &[
    ("mapper_holymapperel_0_P32K_C8K_V", "000"),
    ("mapper_holymapperel_1_P128K", "001"),
    ("mapper_holymapperel_1_P128K_C128K", "001"),
    ("mapper_holymapperel_1_P128K_C128K_S8K", "001"),
    ("mapper_holymapperel_1_P128K_C128K_W8K", "001"),
    ("mapper_holymapperel_1_P128K_C32K_S8K", "001"),
    ("mapper_holymapperel_1_P128K_C32K_W8K", "001"),
    ("mapper_holymapperel_1_P512K_CR8K_S32K", "001"),
    ("mapper_holymapperel_1_P512K_CR8K_S8K", "001"),
    ("mapper_holymapperel_1_P512K_S32K", "001"),
    ("mapper_holymapperel_1_P512K_S8K", "001"),
    ("mapper_holymapperel_2_P128K_V", "002"),
    ("mapper_holymapperel_4_P128K", "004"),
    ("mapper_holymapperel_7_P128K", "007"),
    ("mapper_holymapperel_11_P64K_C64K_V", "011"),
    // NES 2.0 mapper 11 with 32 KiB of CHR-RAM. Color Dreams is documented
    // with CHR-ROM only (`nesdev_wiki/Color_Dreams`), upstream Holy Mapperel
    // builds no such image, and with its CHR fixed the board behaves as BNROM,
    // which is what the ROM reports. Recorded as observed, not judged.
    ("mapper_holymapperel_11_P64K_CR32K_V", "034"),
    ("mapper_holymapperel_28_P512K", "028"),
    ("mapper_holymapperel_28_P512K_CR32K", "028"),
    ("mapper_holymapperel_34_P128K_H", "034"),
    ("mapper_holymapperel_78.3_P128K_C64K", "078"),
    ("mapper_holymapperel_118_P128K_C64K", "118"),
    ("mapper_holymapperel_180_P128K_H", "180"),
    ("mapper_holymapperel_180_P128K_CR8K_H", "180"),
];

/// The nametable rows as text. Holy Mapperel's font puts the digits at their
/// ASCII tile indices, which is all this reads; letters decode as blanks.
fn nametable_rows(nes: &Nes) -> Vec<String> {
    let vram = nes.vram();
    (0..30usize)
        .map(|row| {
            (0..32usize)
                .map(|col| {
                    let t = vram.get(row * 32 + col).copied().unwrap_or(0);
                    if (0x20..0x7f).contains(&t) {
                        t as char
                    } else {
                        ' '
                    }
                })
                .collect::<String>()
                .trim()
                .to_string()
        })
        .filter(|r| !r.is_empty())
        .collect()
}

#[test]
fn holy_mapperel_variants_detect_and_report_0000() {
    let mut failures = Vec::new();
    for (stem, detected) in HOLY {
        let rel = format!("extra/mappers/{stem}.nes");
        let bytes = fs::read(rom_path(&rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
        let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rel}: {e:?}"));
        for _ in 0..HOLY_FRAMES {
            nes.run_frame();
        }
        let rows = nametable_rows(&nes);
        let board = rows
            .first()
            .map_or("", |r| r.split_whitespace().next().unwrap_or(""));
        // The detail line is the one row of the form `...: NNNN`.
        let detail = rows
            .iter()
            .find_map(|r| r.rsplit_once(": ").map(|(_, d)| d.trim().to_string()))
            .unwrap_or_default();
        if board != *detected || detail != "0000" {
            failures.push(format!(
                "{stem}: detected {board:?} (want {detected:?}), detail {detail:?} (want \"0000\")\n{}",
                rows.join("\n")
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n\n"));
}

#[test]
fn cpu_flag_concurrency_report_is_pinned() {
    let rel = "extra/cpu/cpu_flag_concurrency.nes";
    let bytes = fs::read(rom_path(rel)).unwrap_or_else(|e| panic!("read {rel}: {e}"));
    let r = run_nes_blargg(&bytes, BLARGG_FRAMES).expect("rom must parse + run");
    // Strip the ANSI colour codes the ROM writes for a terminal, so the pin is
    // the report and not its colouring. The harness has already replaced each
    // ESC with `.`, so a code arrives as `.[0;37m`.
    let mut report = String::new();
    let mut rest = r.message.as_str();
    while let Some(i) = rest.find(".[") {
        report.push_str(&rest[..i]);
        let tail = &rest[i + 2..];
        match tail.find('m') {
            Some(j) if tail[..j].chars().all(|c| c.is_ascii_digit() || c == ';') => {
                rest = &tail[j + 1..];
            }
            _ => {
                report.push_str(".[");
                rest = tail;
            }
        }
    }
    report.push_str(rest);
    // Trailing spaces are the ROM's column padding. The repository's
    // whitespace hook strips them from a committed snapshot, so strip them here
    // too, or the pin would disagree with its own file.
    let report: String = report
        .lines()
        .map(str::trim_end)
        .collect::<Vec<_>>()
        .join("\n");
    assert!(
        report.contains("Finding APU IRQ timings"),
        "the report never reached its measurement: {report:?}"
    );
    insta::assert_snapshot!("cpu_flag_concurrency_report", report);
}

const PINNED: &[&str] = &[
    "extra/apu/apu_dmc_pitch.nes",
    "extra/apu/apu_env.nes",
    "extra/apu/apu_lin_ctr.nes",
    "extra/apu/apu_noise_pitch.nes",
    "extra/apu/apu_phase_reset.nes",
    "extra/apu/apu_square_pitch.nes",
    "extra/apu/apu_sweep_cutoff.nes",
    "extra/apu/apu_sweep_sub.nes",
    "extra/apu/apu_triangle_pitch.nes",
    "extra/ppu/ppu_color.nes",
    "extra/ppu/ppu_ntsc_torture.nes",
    "extra/ppu/ppu_palette.nes",
];

/// A REGRESSION PIN, not an accuracy verdict: see the module docs, kind 4.
#[test]
fn visual_and_audio_roms_are_pinned() {
    let mut lines = Vec::new();
    for rel in PINNED {
        let (fb, cycles, samples, audio) = run_and_capture_full("unreferenced", rel, PIN_FRAMES);
        assert!(samples > 0, "{rel}: produced no audio at all");
        lines.push(snapshot_line_full(
            rel, PIN_FRAMES, fb, cycles, samples, audio,
        ));
    }
    insta::assert_snapshot!("unreferenced_visual_audio_pins", lines.join("\n"));
}
