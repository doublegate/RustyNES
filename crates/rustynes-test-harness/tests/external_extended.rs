//! Extended commercial-ROM regression baselines (compatibility survey).
//!
//! Companion to [`external_real_games`]: locks 39 newly-validated
//! commercial games into the same `insta`-snapshot regression net. Each
//! game was extracted to the gitignored `tests/roms/external/` tree and
//! visually verified to render correctly during a compatibility survey;
//! this harness snapshots their deterministic boot output so future
//! regressions are caught.
//!
//! Each snapshot records the ROM's SHA-256 (so a different dump fails
//! with a clear diagnostic) plus the framebuffer / audio FNV-1a hashes
//! and cumulative cycle count — emulator output only, never ROM bytes,
//! so the `.snap` files are safe to commit.
//!
//! ## Feature gating
//!
//! ```text
//! cargo test -p rustynes-test-harness --features commercial-roms,test-roms \
//!     --test external_extended -- --nocapture
//! ```
//!
//! Like [`external_real_games`], `commercial-roms` is off by default so
//! CI never depends on non-distributable assets.
//!
//! ## Recapturing baselines
//!
//! ```bash
//! INSTA_UPDATE=always \
//!     cargo test -p rustynes-test-harness --release \
//!     --features commercial-roms,test-roms \
//!     --test external_extended
//! # Accept the .snap.new files with: cargo insta accept
//! ```

#![cfg(feature = "commercial-roms")]
#![allow(clippy::doc_markdown)]
#![allow(clippy::too_many_lines)]

mod common;

use common::external::{run_capture, snapshot_text, InputScript};

/// Default script: 600 frames idle (no input), 10 s @ NTSC 60 Hz —
/// past the title-screen ramp-up for the bulk of these games.
const DEFAULT_IDLE: InputScript = InputScript::IdleOnly { frames: 600 };

/// Longer 1200-frame (20 s) idle script for games whose title / intro
/// sequence lands later than the default 600-frame budget (Crystalis
/// and StarTropics both have extended opening cinematics).
const LONG_IDLE: InputScript = InputScript::IdleOnly { frames: 1200 };

/// Shorter 300-frame (5 s) idle for games whose title screen is shown
/// EARLY then auto-advances to a black/demo attract loop (Burai Fighter
/// holds its title for ~5 s, then blanks before the 600-frame mark).
const TITLE_IDLE_300: InputScript = InputScript::IdleOnly { frames: 300 };

/// START tapped every 10 s through a long multi-stage intro (publisher
/// splash → story scrawl → title → menu) to reach the actual menu /
/// stage-select. Mega Man 4 & 6 land on their Robot-Master select grid
/// at the f2000 checkpoint (their f600 idle frame is only the story text).
const MENU_REPEAT_START: InputScript = InputScript::RepeatStartTap {
    warmup: 90,
    period: 600,
    taps: 4,
    free_run: 200,
    checkpoints: &[2000],
};

/// Helper: one-line test body. Snapshot name == test function name
/// (insta default).
fn check(rom_rel: &str, script: InputScript, snap_id: &str) {
    let result = run_capture(rom_rel, script);
    let text = snapshot_text(rom_rel, script, &result);
    insta::assert_snapshot!(snap_id, text);
}

// ============================================================
// Mapper 000 — NROM (6 ROMs)
// ============================================================

#[test]
fn extended_nrom_1942() {
    check(
        "mapper-000-NROM/1942.nes",
        DEFAULT_IDLE,
        "extended_nrom_1942",
    );
}

#[test]
fn extended_nrom_duck_hunt() {
    check(
        "mapper-000-NROM/Duck Hunt.nes",
        DEFAULT_IDLE,
        "extended_nrom_duck_hunt",
    );
}

#[test]
fn extended_nrom_galaga() {
    check(
        "mapper-000-NROM/Galaga - Demons of Death.nes",
        DEFAULT_IDLE,
        "extended_nrom_galaga",
    );
}

#[test]
fn extended_nrom_ice_hockey() {
    check(
        "mapper-000-NROM/Ice Hockey.nes",
        DEFAULT_IDLE,
        "extended_nrom_ice_hockey",
    );
}

#[test]
fn extended_nrom_kung_fu() {
    check(
        "mapper-000-NROM/Kung Fu.nes",
        DEFAULT_IDLE,
        "extended_nrom_kung_fu",
    );
}

#[test]
fn extended_nrom_pac_man() {
    check(
        "mapper-000-NROM/Pac-Man (Namco).nes",
        DEFAULT_IDLE,
        "extended_nrom_pac_man",
    );
}

// ============================================================
// Mapper 001 — MMC1 (11 ROMs)
// ============================================================

#[test]
fn extended_mmc1_bionic_commando() {
    check(
        "mapper-001-MMC1/Bionic Commando.nes",
        DEFAULT_IDLE,
        "extended_mmc1_bionic_commando",
    );
}

#[test]
fn extended_mmc1_blaster_master() {
    check(
        "mapper-001-MMC1/Blaster Master.nes",
        DEFAULT_IDLE,
        "extended_mmc1_blaster_master",
    );
}

#[test]
fn extended_mmc1_bubble_bobble() {
    check(
        "mapper-001-MMC1/Bubble Bobble.nes",
        DEFAULT_IDLE,
        "extended_mmc1_bubble_bobble",
    );
}

#[test]
fn extended_mmc1_double_dragon() {
    check(
        "mapper-001-MMC1/Double Dragon.nes",
        DEFAULT_IDLE,
        "extended_mmc1_double_dragon",
    );
}

#[test]
fn extended_mmc1_dr_mario() {
    check(
        "mapper-001-MMC1/Dr. Mario.nes",
        DEFAULT_IDLE,
        "extended_mmc1_dr_mario",
    );
}

#[test]
fn extended_mmc1_dragon_warrior() {
    check(
        "mapper-001-MMC1/Dragon Warrior.nes",
        DEFAULT_IDLE,
        "extended_mmc1_dragon_warrior",
    );
}

#[test]
fn extended_mmc1_maniac_mansion() {
    check(
        "mapper-001-MMC1/Maniac Mansion.nes",
        DEFAULT_IDLE,
        "extended_mmc1_maniac_mansion",
    );
}

#[test]
fn extended_mmc1_rad_racer() {
    check(
        "mapper-001-MMC1/Rad Racer.nes",
        DEFAULT_IDLE,
        "extended_mmc1_rad_racer",
    );
}

#[test]
fn extended_mmc1_tecmo_bowl() {
    check(
        "mapper-001-MMC1/Tecmo Bowl.nes",
        DEFAULT_IDLE,
        "extended_mmc1_tecmo_bowl",
    );
}

#[test]
fn extended_mmc1_tetris() {
    check(
        "mapper-001-MMC1/Tetris.nes",
        DEFAULT_IDLE,
        "extended_mmc1_tetris",
    );
}

#[test]
fn extended_mmc1_zelda_ii() {
    check(
        "mapper-001-MMC1/Zelda II - The Adventure of Link.nes",
        DEFAULT_IDLE,
        "extended_mmc1_zelda_ii",
    );
}

// ============================================================
// Mapper 002 — UxROM (5 ROMs)
// ============================================================

#[test]
fn extended_uxrom_1943() {
    check(
        "mapper-002-UxROM/1943 - The Battle of Midway.nes",
        DEFAULT_IDLE,
        "extended_uxrom_1943",
    );
}

#[test]
fn extended_uxrom_gun_smoke() {
    check(
        "mapper-002-UxROM/Gun.Smoke.nes",
        DEFAULT_IDLE,
        "extended_uxrom_gun_smoke",
    );
}

#[test]
fn extended_uxrom_jackal() {
    check(
        "mapper-002-UxROM/Jackal.nes",
        DEFAULT_IDLE,
        "extended_uxrom_jackal",
    );
}

#[test]
fn extended_uxrom_life_force() {
    check(
        "mapper-002-UxROM/Life Force.nes",
        DEFAULT_IDLE,
        "extended_uxrom_life_force",
    );
}

#[test]
fn extended_uxrom_rygar() {
    check(
        "mapper-002-UxROM/Rygar.nes",
        DEFAULT_IDLE,
        "extended_uxrom_rygar",
    );
}

// ============================================================
// Mapper 004 — MMC3 (10 ROMs)
// ============================================================

#[test]
fn extended_mmc3_bad_dudes() {
    check(
        "mapper-004-MMC3/Bad Dudes.nes",
        DEFAULT_IDLE,
        "extended_mmc3_bad_dudes",
    );
}

#[test]
fn extended_mmc3_bucky_ohare() {
    check(
        "mapper-004-MMC3/Bucky O'Hare.nes",
        DEFAULT_IDLE,
        "extended_mmc3_bucky_ohare",
    );
}

#[test]
fn extended_mmc3_burai_fighter() {
    check(
        "mapper-004-MMC3/Burai Fighter.nes",
        TITLE_IDLE_300,
        "extended_mmc3_burai_fighter",
    );
}

#[test]
fn extended_mmc3_crystalis() {
    // Crystalis has an extended opening cinematic; the title/menu lands
    // past the default 600-frame budget, so use the 1200-frame script.
    check(
        "mapper-004-MMC3/Crystalis.nes",
        LONG_IDLE,
        "extended_mmc3_crystalis",
    );
}

#[test]
fn extended_mmc3_felix_the_cat() {
    check(
        "mapper-004-MMC3/Felix the Cat.nes",
        DEFAULT_IDLE,
        "extended_mmc3_felix_the_cat",
    );
}

#[test]
fn extended_mmc3_mega_man_4() {
    check(
        "mapper-004-MMC3/Mega Man 4.nes",
        MENU_REPEAT_START,
        "extended_mmc3_mega_man_4",
    );
}

#[test]
fn extended_mmc3_mega_man_5() {
    check(
        "mapper-004-MMC3/Mega Man 5.nes",
        DEFAULT_IDLE,
        "extended_mmc3_mega_man_5",
    );
}

#[test]
fn extended_mmc3_mega_man_6() {
    check(
        "mapper-004-MMC3/Mega Man 6.nes",
        MENU_REPEAT_START,
        "extended_mmc3_mega_man_6",
    );
}

#[test]
fn extended_mmc3_rampage() {
    check(
        "mapper-004-MMC3/Rampage.nes",
        DEFAULT_IDLE,
        "extended_mmc3_rampage",
    );
}

#[test]
fn extended_mmc3_startropics() {
    // StarTropics opens with a long intro sequence; the title/menu lands
    // past the default 600-frame budget, so use the 1200-frame script.
    check(
        "mapper-004-MMC3/StarTropics.nes",
        LONG_IDLE,
        "extended_mmc3_startropics",
    );
}

// ============================================================
// Mapper 007 — AxROM (2 ROMs)
// ============================================================

#[test]
fn extended_axrom_battletoads_double_dragon() {
    check(
        "mapper-007-AxROM/Battletoads & Double Dragon - The Ultimate Team.nes",
        DEFAULT_IDLE,
        "extended_axrom_battletoads_double_dragon",
    );
}

#[test]
fn extended_axrom_wizards_and_warriors() {
    check(
        "mapper-007-AxROM/Wizards & Warriors.nes",
        DEFAULT_IDLE,
        "extended_axrom_wizards_and_warriors",
    );
}

// ============================================================
// Mapper 071 — Camerica (4 ROMs)
// ============================================================

#[test]
fn extended_camerica_bee_52() {
    check(
        "mapper-071-Camerica/Bee 52.nes",
        DEFAULT_IDLE,
        "extended_camerica_bee_52",
    );
}

#[test]
fn extended_camerica_firehawk() {
    check(
        "mapper-071-Camerica/Firehawk.nes",
        DEFAULT_IDLE,
        "extended_camerica_firehawk",
    );
}

#[test]
fn extended_camerica_mig_29() {
    check(
        "mapper-071-Camerica/MiG 29 - Soviet Fighter.nes",
        DEFAULT_IDLE,
        "extended_camerica_mig_29",
    );
}

#[test]
fn extended_camerica_micro_machines() {
    check(
        "mapper-071-Camerica/Micro Machines.nes",
        DEFAULT_IDLE,
        "extended_camerica_micro_machines",
    );
}

// ============================================================
// Mapper 206 — Namcot 118 (1 ROM)
// ============================================================

#[test]
fn extended_namcot118_gauntlet() {
    check(
        "mapper-206-Namcot118/Gauntlet.nes",
        DEFAULT_IDLE,
        "extended_namcot118_gauntlet",
    );
}

// ============================================================
// Mapper 033 — Taito TC0190 (1 ROM, v2.6.0)
// ============================================================

#[test]
fn extended_taito33_don_doko_don() {
    check(
        "mapper-033-TaitoTC0190/Don Doko Don.nes",
        DEFAULT_IDLE,
        "extended_taito33_don_doko_don",
    );
}

// ============================================================
// Mapper 093 — Sunsoft-3R (1 ROM, v2.6.0)
// ============================================================

#[test]
fn extended_sunsoft3r_shanghai() {
    check(
        "mapper-093-Sunsoft3R/Shanghai.nes",
        DEFAULT_IDLE,
        "extended_sunsoft3r_shanghai",
    );
}

// ============================================================
// Mapper 152 — Bandai 74161/161 1-screen (1 ROM, v2.6.0)
// ============================================================

#[test]
fn extended_bandai152_arkanoid_ii() {
    check(
        "mapper-152-Bandai74161/Arkanoid II.nes",
        DEFAULT_IDLE,
        "extended_bandai152_arkanoid_ii",
    );
}

// ============================================================
// Mapper 032 — Irem G-101 (2 ROMs, v2.6.0)
// ============================================================

#[test]
fn extended_iremg101_yancha_maru_2() {
    check(
        "mapper-032-IremG101/Kaiketsu Yancha Maru 2 - Karakuri Land (Japan).nes",
        DEFAULT_IDLE,
        "extended_iremg101_yancha_maru_2",
    );
}

#[test]
fn extended_iremg101_major_league() {
    check(
        "mapper-032-IremG101/Major League (Japan).nes",
        DEFAULT_IDLE,
        "extended_iremg101_major_league",
    );
}

// ============================================================
// Mapper 048 — Taito TC0690 (1 ROM, v2.6.0)
// ============================================================

#[test]
fn extended_taito48_don_doko_don_2() {
    check(
        "mapper-048-TaitoTC0690/Don Doko Don 2 (Japan).nes",
        DEFAULT_IDLE,
        "extended_taito48_don_doko_don_2",
    );
}

// ============================================================
// Mapper 087 — Jaleco/Konami CNROM-style (2 ROMs, v2.6.0)
// ============================================================

#[test]
fn extended_jaleco87_choplifter() {
    check(
        "mapper-087-Jaleco87/Choplifter (Japan) (En) (Rev 1).nes",
        DEFAULT_IDLE,
        "extended_jaleco87_choplifter",
    );
}

#[test]
fn extended_jaleco87_argus() {
    check(
        "mapper-087-Jaleco87/Argus (Japan).nes",
        DEFAULT_IDLE,
        "extended_jaleco87_argus",
    );
}

// ============================================================
// Mapper 184 — Sunsoft-1 (2 ROMs, v2.6.0)
// ============================================================

#[test]
fn extended_sunsoft1_atlantis_no_nazo() {
    check(
        "mapper-184-Sunsoft1/Atlantis no Nazo (Japan).nes",
        DEFAULT_IDLE,
        "extended_sunsoft1_atlantis_no_nazo",
    );
}

#[test]
fn extended_sunsoft1_madoola() {
    check(
        "mapper-184-Sunsoft1/Madoola no Tsubasa (Japan).nes",
        DEFAULT_IDLE,
        "extended_sunsoft1_madoola",
    );
}

// ============================================================
// Mapper 080 — Taito X1-005 (1 ROM, v2.6.0)
// ============================================================

#[test]
fn extended_taito_x1_005_kyoto_ryuu() {
    check(
        "mapper-080-TaitoX1-005/Kyoto Ryuu no Tera Satsujin Jiken (Japan).nes",
        DEFAULT_IDLE,
        "extended_taito_x1_005_kyoto_ryuu",
    );
}

// ============================================================
// Mapper 082 — Taito X1-017 (1 ROM, v2.6.0)
// ============================================================

#[test]
fn extended_taito_x1_017_stadium_iii() {
    check(
        "mapper-082-TaitoX1-017/Kyuukyoku Harikiri Stadium III (Japan).nes",
        DEFAULT_IDLE,
        "extended_taito_x1_017_stadium_iii",
    );
}

// ============================================================
// PlayChoice-10 — 2C03 RGB PPU (1 ROM, v2.6.0)
//
// A clean iNES-1.0 PC10 arcade dump (byte 7 == 0x02) routed through the
// 2C03 RGB palette by the clean-byte arcade detection in rustynes-mappers::parse.
// This snapshot exercises the NEW RGB output path; it is not a regression
// of any prior baseline.
// ============================================================

/// PC10 Power Blade reaches its RGB title screen after the boot ramp;
/// a single START tap leaves the title for the LEVEL-select menu. The
/// script runs 321 frames total (120 warmup + 1 tap + 200 free-run), so
/// the f320 checkpoint lands on the rendered RGB title/menu.
const PC10_TITLE: InputScript = InputScript::RepeatStartTap {
    warmup: 120,
    period: 600,
    taps: 1,
    free_run: 200,
    checkpoints: &[320],
};

#[test]
fn extended_pc10_power_blade_rgb() {
    check(
        "pc10/PC10 Power Blade.nes",
        PC10_TITLE,
        "extended_pc10_power_blade_rgb",
    );
}
