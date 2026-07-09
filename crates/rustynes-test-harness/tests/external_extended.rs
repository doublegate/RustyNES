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

use common::external::{InputScript, run_capture, snapshot_text};

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

// ============================================================
// v2.1.0 "Fathom" F3 — BestEffort -> Curated promotions.
// One representative staged commercial ROM per mapper, locked as a
// byte-identity boot-output snapshot (600-frame idle). Promoting these
// mappers to Curated (see rustynes-mappers::tier) is gated on this
// oracle evidence per ADR 0011.
// ============================================================

#[test]
fn extended_m15_100_in_1_contra_function_16_p1() {
    check(
        "mapper-015-Multicart15/100-in-1 Contra Function 16 [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m15_100_in_1_contra_function_16_p1",
    );
}

#[test]
fn extended_m28_witchnwiz_2021_02_22_nes_dev_v_1_0_0() {
    check(
        "mapper-028-Action53/witchnwiz_2021_02_22_nes_dev_v_1_0_0.nes",
        DEFAULT_IDLE,
        "extended_m28_witchnwiz_2021_02_22_nes_dev_v_1_0_0",
    );
}

#[test]
fn extended_m30_chu_liu_xiang() {
    check(
        "mapper-030-UNROM512/Chu Liu Xiang (Ch) (Wxn).nes",
        DEFAULT_IDLE,
        "extended_m30_chu_liu_xiang",
    );
}

#[test]
fn extended_m31_2a03puritans() {
    check(
        "mapper-031-INL-NSF/2a03puritans.nes",
        DEFAULT_IDLE,
        "extended_m31_2a03puritans",
    );
}

#[test]
fn extended_m36_3_in_1_supergun() {
    check(
        "mapper-036-TXC36/3 in 1 Supergun (Asia) (Ja) (Unl).nes",
        DEFAULT_IDLE,
        "extended_m36_3_in_1_supergun",
    );
}

#[test]
fn extended_m40_super_mario_bros_2() {
    check(
        "mapper-040-NTDEC2722/Super Mario Bros 2 (Lost Levels) (Unl).nes",
        DEFAULT_IDLE,
        "extended_m40_super_mario_bros_2",
    );
}

#[test]
fn extended_m58_116_in_1_p1() {
    check(
        "mapper-058-Multicart58/116-in-1 [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m58_116_in_1_p1",
    );
}

#[test]
fn extended_m60_4_in_1_p1() {
    check(
        "mapper-060-Multicart60/4-in-1 (Mapper 60) [p1].nes",
        DEFAULT_IDLE,
        "extended_m60_4_in_1_p1",
    );
}

#[test]
fn extended_m61_20_in_1_a1_p1() {
    check(
        "mapper-061-Multicart61/20-in-1 [a1][p1][!].nes",
        DEFAULT_IDLE,
        "extended_m61_20_in_1_a1_p1",
    );
}

#[test]
fn extended_m62_super_190_in_1() {
    check(
        "mapper-062-Multicart62/Super 190-in-1 (Asia) (En) (Unl) (Pirate).nes",
        DEFAULT_IDLE,
        "extended_m62_super_190_in_1",
    );
}

#[test]
fn extended_m63_255_in_1() {
    check(
        "mapper-063-NTDEC0324/255-in-1 (As) [!].nes",
        DEFAULT_IDLE,
        "extended_m63_255_in_1",
    );
}

#[test]
fn extended_m72_doraemon_world_3_by_kiku() {
    check(
        "mapper-072-Jaleco72/Doraemon World 3 by Kiku (Doraemon Hack).nes",
        DEFAULT_IDLE,
        "extended_m72_doraemon_world_3_by_kiku",
    );
}

#[test]
fn extended_m76_digital_devil_monogatari_megami_tensei() {
    check(
        "mapper-076-Namcot3446/Digital Devil Monogatari - Megami Tensei (J) [!].nes",
        DEFAULT_IDLE,
        "extended_m76_digital_devil_monogatari_megami_tensei",
    );
}

#[test]
fn extended_m77_napoleon_senki() {
    check(
        "mapper-077-Irem77/Napoleon Senki (J) [!].nes",
        DEFAULT_IDLE,
        "extended_m77_napoleon_senki",
    );
}

#[test]
fn extended_m92_moero_pro_soccer() {
    check(
        "mapper-092-JalecoJF19/Moero!! Pro Soccer (J).nes",
        DEFAULT_IDLE,
        "extended_m92_moero_pro_soccer",
    );
}

#[test]
fn extended_m94_senjou_no_ookami() {
    check(
        "mapper-094-UN1ROM/Senjou no Ookami (J) [!].nes",
        DEFAULT_IDLE,
        "extended_m94_senjou_no_ookami",
    );
}

#[test]
fn extended_m95_dragon_buster() {
    check(
        "mapper-095-Namcot3425/Dragon Buster (J) [!].nes",
        DEFAULT_IDLE,
        "extended_m95_dragon_buster",
    );
}

#[test]
fn extended_m96_oeka_kids_anpanman_no_hiragana_daisuki() {
    check(
        "mapper-096-Multicart96/Oeka Kids - Anpanman no Hiragana Daisuki (J).nes",
        DEFAULT_IDLE,
        "extended_m96_oeka_kids_anpanman_no_hiragana_daisuki",
    );
}

#[test]
fn extended_m97_kaiketsu_yanchamaru() {
    check(
        "mapper-097-Irem-TamSan/Kaiketsu Yanchamaru (J) [!].nes",
        DEFAULT_IDLE,
        "extended_m97_kaiketsu_yanchamaru",
    );
}

#[test]
fn extended_m101_urusei_yatsura_lum_no_wedding_bell_a1_t_fre() {
    check(
        "mapper-101-JalecoJF10/Urusei Yatsura - Lum no Wedding Bell (J) [a1][T+Fre].nes",
        DEFAULT_IDLE,
        "extended_m101_urusei_yatsura_lum_no_wedding_bell_a1_t_fre",
    );
}

#[test]
fn extended_m107_magic_dragon() {
    check(
        "mapper-107-MagicDragon/Magic Dragon (Unl).nes",
        DEFAULT_IDLE,
        "extended_m107_magic_dragon",
    );
}

// NOTE: mapper 111 (GTROM/Cheapocabra) is deliberately NOT oracle-promoted — the
// only available dump ("Ninja Ryukenden (Ch)") jams at boot (cycles=26), so it is
// not honest Curated-tier evidence. Mapper 111 stays BestEffort (see tier.rs).

#[test]
fn extended_m112_chik_bik_ji_jin_saam_gwok_ji() {
    check(
        "mapper-112-NTDEC-Asder/Chik Bik Ji Jin - Saam Gwok Ji (CN-20) (Asder) [!].nes",
        DEFAULT_IDLE,
        "extended_m112_chik_bik_ji_jin_saam_gwok_ji",
    );
}

#[test]
fn extended_m132_creatom() {
    check(
        "mapper-132-TXC132/Creatom (Spain) (Gluk Video) (Unl).zip",
        DEFAULT_IDLE,
        "extended_m132_creatom",
    );
}

#[test]
fn extended_m133_21_in_1_p1() {
    check(
        "mapper-133-SachenSA72008/21-in-1 [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m133_21_in_1_p1",
    );
}

#[test]
fn extended_m137_great_wall_the() {
    check(
        "mapper-137-Sachen8259D/Great Wall, The (Sachen) [!].nes",
        DEFAULT_IDLE,
        "extended_m137_great_wall_the",
    );
}

#[test]
fn extended_m143_dancing_blocks() {
    check(
        "mapper-143-SachenTCA01/Dancing Blocks (Sachen) [!].nes",
        DEFAULT_IDLE,
        "extended_m143_dancing_blocks",
    );
}

#[test]
fn extended_m145_sidewinder() {
    check(
        "mapper-145-SachenSA72007/Sidewinder (Sachen) [!].nes",
        DEFAULT_IDLE,
        "extended_m145_sidewinder",
    );
}

#[test]
fn extended_m146_galactic_crusader() {
    check(
        "mapper-146-Sachen-NINA/Galactic Crusader (Sachen) [!].zip",
        DEFAULT_IDLE,
        "extended_m146_galactic_crusader",
    );
}

#[test]
fn extended_m147_challenge_of_the_dragon() {
    check(
        "mapper-147-Sachen3018-JV001/Challenge of the Dragon (Sachen) [!].nes",
        DEFAULT_IDLE,
        "extended_m147_challenge_of_the_dragon",
    );
}

#[test]
fn extended_m148_av_hanafuda_club() {
    check(
        "mapper-148-SachenSA0037/AV Hanafuda Club (Japan) (Unl).nes",
        DEFAULT_IDLE,
        "extended_m148_av_hanafuda_club",
    );
}

#[test]
fn extended_m149_taiwan_mahjong_tai_wan_ma_que_16() {
    check(
        "mapper-149-SachenSA0036/Taiwan Mahjong - Tai Wan Ma Que 16 (Asia) (Ja) (Unl).nes",
        DEFAULT_IDLE,
        "extended_m149_taiwan_mahjong_tai_wan_ma_que_16",
    );
}

#[test]
fn extended_m150_auto_upturn() {
    check(
        "mapper-150-Sachen74LS374N/Auto-Upturn (Asia) (Ja) (PAL) (Unl).zip",
        DEFAULT_IDLE,
        "extended_m150_auto_upturn",
    );
}

#[test]
fn extended_m156_buzz_waldog() {
    check(
        "mapper-156-DIS23C01-DAOU/Buzz & Waldog (USA) (Unl) (Beta).nes",
        DEFAULT_IDLE,
        "extended_m156_buzz_waldog",
    );
}

#[test]
fn extended_m162_chong_wu_jin_hua_shi() {
    check(
        "mapper-162-WaixingFS304/Chong Wu Jin Hua Shi (Pet Evolve) (ES-1085) (Ch).nes",
        DEFAULT_IDLE,
        "extended_m162_chong_wu_jin_hua_shi",
    );
}

#[test]
fn extended_m177_mei_guo_fu_hao() {
    check(
        "mapper-177-Hengedianzi/Mei Guo Fu Hao (Ch).nes",
        DEFAULT_IDLE,
        "extended_m177_mei_guo_fu_hao",
    );
}

#[test]
fn extended_m178_da_hang_hai_vii() {
    check(
        "mapper-178-WaixingEdu/Da Hang Hai VII (Ch).nes",
        DEFAULT_IDLE,
        "extended_m178_da_hang_hai_vii",
    );
}

#[test]
fn extended_m180_crazy_climber() {
    check(
        "mapper-180-UNROM-Nichibutsu/Crazy Climber (J) [!].nes",
        DEFAULT_IDLE,
        "extended_m180_crazy_climber",
    );
}

#[test]
fn extended_m185_b_wings() {
    check(
        "mapper-185-CNROM-Lock/B-Wings (J) [!].nes",
        DEFAULT_IDLE,
        "extended_m185_b_wings",
    );
}

#[test]
fn extended_m200_1000_in_1_p1() {
    check(
        "mapper-200-Multicart200/1000-in-1 [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m200_1000_in_1_p1",
    );
}

#[test]
fn extended_m201_21_in_1_p1() {
    check(
        "mapper-201-Multicart201/21-in-1 (2006-CA) [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m201_21_in_1_p1",
    );
}

#[test]
fn extended_m202_150_in_1_p1() {
    check(
        "mapper-202-Multicart202/150-in-1 (Mapper 202) [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m202_150_in_1_p1",
    );
}

#[test]
fn extended_m203_35_in_1_happy_p1() {
    check(
        "mapper-203-Multicart203/35-in-1 Happy [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m203_35_in_1_happy_p1",
    );
}

#[test]
fn extended_m212_100_in_1_p1() {
    check(
        "mapper-212-Multicart212/100-in-1 (MG109) [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m212_100_in_1_p1",
    );
}

#[test]
fn extended_m213_9999999_in_1_p2() {
    check(
        "mapper-213-Multicart213/9999999-in-1 [p2].nes",
        DEFAULT_IDLE,
        "extended_m213_9999999_in_1_p2",
    );
}

#[test]
fn extended_m214_super_gun_20_in_1_p1() {
    check(
        "mapper-214-Multicart214/Super Gun 20-in-1 [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m214_super_gun_20_in_1_p1",
    );
}

#[test]
fn extended_m218_magic_floor_by_martin_korth() {
    check(
        "mapper-218-MagicFloor/Magic Floor by Martin Korth (2012) (PC10 Version) (PD).nes",
        DEFAULT_IDLE,
        "extended_m218_magic_floor_by_martin_korth",
    );
}

#[test]
fn extended_m225_110_in_1() {
    check(
        "mapper-225-ColorDreams72in1/110 in 1 (Asia) (En) (Unl) (Pirate).nes",
        DEFAULT_IDLE,
        "extended_m225_110_in_1",
    );
}

#[test]
fn extended_m226_76_in_1_p1() {
    check(
        "mapper-226-BMC-76in1/76-in-1 [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m226_76_in_1_p1",
    );
}

#[test]
fn extended_m227_295_in_1_p1() {
    check(
        "mapper-227-BMC-1200in1/295-in-1 [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m227_295_in_1_p1",
    );
}

#[test]
fn extended_m229_31_in_1_p1() {
    check(
        "mapper-229-BMC-31in1/31-in-1 [p1].nes",
        DEFAULT_IDLE,
        "extended_m229_31_in_1_p1",
    );
}

#[test]
fn extended_m231_20_in_1_p1() {
    check(
        "mapper-231-BMC-20in1/20-in-1 [p1][!].nes",
        DEFAULT_IDLE,
        "extended_m231_20_in_1_p1",
    );
}

#[test]
fn extended_m233_super_22_in_1_p1() {
    check(
        "mapper-233-BMC-42in1/Super 22-in-1 [p1].nes",
        DEFAULT_IDLE,
        "extended_m233_super_22_in_1_p1",
    );
}

#[test]
fn extended_m234_maxi_15() {
    check(
        "mapper-234-Maxi15/Maxi 15 (AVE) [!].nes",
        DEFAULT_IDLE,
        "extended_m234_maxi_15",
    );
}

#[test]
fn extended_m242_dragon_quest_viii() {
    check(
        "mapper-242-Waixing43in1/Dragon Quest VIII (ES-1077) (Ch) [!].nes",
        DEFAULT_IDLE,
        "extended_m242_dragon_quest_viii",
    );
}

#[test]
fn extended_m244_asmik_kun_land_t1() {
    check(
        "mapper-244-Decathlon/Asmik-kun Land (J) [t1].nes",
        DEFAULT_IDLE,
        "extended_m244_asmik_kun_land_t1",
    );
}

#[test]
fn extended_m246_feng_shen_bang() {
    check(
        "mapper-246-FongShenBang/Feng Shen Bang (Asia) (Ja) (Unl).nes",
        DEFAULT_IDLE,
        "extended_m246_feng_shen_bang",
    );
}

#[test]
fn extended_m250_queen_bee_v() {
    check(
        "mapper-250-Nitra/Queen Bee V (Unl) [!].nes",
        DEFAULT_IDLE,
        "extended_m250_queen_bee_v",
    );
}

// ============================================================
// v2.1.0 "Fathom" F3 (batch 2) — GoodNES-sourced BestEffort -> Curated.
// Sachen/Waixing/Kaiser/JY-Company/pirate-multicart boards, one clean
// dump each, byte-identity boot snapshot (ADR 0011).
// ============================================================

#[test]
fn extended_m35_warioland_ii() {
    check(
        "mapper-035-JYCompany35/Warioland II (Unl).zip",
        DEFAULT_IDLE,
        "extended_m35_warioland_ii",
    );
}

#[test]
fn extended_m42_ai_senshi_nicol() {
    check(
        "mapper-042-BioMiracleFDS/Ai Senshi Nicol (FDS Conversion) [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m42_ai_senshi_nicol",
    );
}

#[test]
fn extended_m44_super_big_7_in_1() {
    check(
        "mapper-044-SuperBig7in1/Super Big 7-in-1 [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m44_super_big_7_in_1",
    );
}

#[test]
fn extended_m46_rumblestation_15_in_1() {
    check(
        "mapper-046-RumbleStation/RumbleStation 15-in-1 (Unl).zip",
        DEFAULT_IDLE,
        "extended_m46_rumblestation_15_in_1",
    );
}

#[test]
fn extended_m49_super_hik_4_in_1() {
    check(
        "mapper-049-SuperHIK4in1/Super HIK 4-in-1 [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m49_super_hik_4_in_1",
    );
}

// NOTE: mapper 50 (Alibaba/SMB2j FDS-conversion) is deliberately NOT oracle-
// promoted — the only available dump ("Super Mario Bros. (Alt Levels)") halts
// after ~18 frames (cycles=551855), so it is not honest Curated-tier evidence.
// Mapper 50 stays BestEffort (see tier.rs).

#[test]
fn extended_m51_11_in_1_ball_games() {
    check(
        "mapper-051-BallGames11in1/11-in-1 Ball Games [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m51_11_in_1_ball_games",
    );
}

#[test]
fn extended_m52_2_in_1_1996_super_hik_gold_card() {
    check(
        "mapper-052-MarioParty7in1/2-in-1 - 1996 Super HIK Gold Card (NT-803) [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m52_2_in_1_1996_super_hik_gold_card",
    );
}

#[test]
fn extended_m56_super_mario_bros_3() {
    check(
        "mapper-056-KaiserKS202/Super Mario Bros. 3 (J) (PRG1) [p2][!].zip",
        DEFAULT_IDLE,
        "extended_m56_super_mario_bros_3",
    );
}

#[test]
fn extended_m57_54_in_1() {
    check(
        "mapper-057-BMC-GKA/54-in-1 (Game Star - GK-54) [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m57_54_in_1",
    );
}

#[test]
fn extended_m90_1997_super_hik_4_in_1() {
    check(
        "mapper-090-JYCompany90/1997 Super HIK 4-in-1 (JY-052) [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m90_1997_super_hik_4_in_1",
    );
}

#[test]
fn extended_m115_av_jiu_ji_ma_jiang_2() {
    check(
        "mapper-115-KashengSFC03/AV Jiu Ji Ma Jiang 2 (Unl) [!].zip",
        DEFAULT_IDLE,
        "extended_m115_av_jiu_ji_ma_jiang_2",
    );
}

#[test]
fn extended_m120_tobidase_daisakusen() {
    check(
        "mapper-120-TobidaseFDS/Tobidase Daisakusen (FDS Conversion).zip",
        DEFAULT_IDLE,
        "extended_m120_tobidase_daisakusen",
    );
}

#[test]
fn extended_m134_2_in_1_family_kid_aladdin_4() {
    check(
        "mapper-134-BMC-T4A54A/2-in-1 - Family Kid & Aladdin 4 (Ch) [!].zip",
        DEFAULT_IDLE,
        "extended_m134_2_in_1_family_kid_aladdin_4",
    );
}

#[test]
fn extended_m136_mei_loi_siu_ji() {
    check(
        "mapper-136-SachenTCU02/Mei Loi Siu Ji (Metal Fighter) (Sachen) [!].zip",
        DEFAULT_IDLE,
        "extended_m136_mei_loi_siu_ji",
    );
}

#[test]
fn extended_m138_silver_eagle() {
    check(
        "mapper-138-Sachen8259B/Silver Eagle (Sachen) [!].zip",
        DEFAULT_IDLE,
        "extended_m138_silver_eagle",
    );
}

#[test]
fn extended_m139_final_combat() {
    check(
        "mapper-139-Sachen8259C/Final Combat (Sachen-JAP) [!].zip",
        DEFAULT_IDLE,
        "extended_m139_final_combat",
    );
}

#[test]
fn extended_m141_po_po_team() {
    check(
        "mapper-141-Sachen8259A/Po Po Team (Sachen) [!].zip",
        DEFAULT_IDLE,
        "extended_m141_po_po_team",
    );
}

#[test]
fn extended_m142_pipe_5() {
    check(
        "mapper-142-KaiserKS7032/Pipe 5 (Sachen) [!].zip",
        DEFAULT_IDLE,
        "extended_m142_pipe_5",
    );
}

#[test]
fn extended_m164_digital_dragon() {
    check(
        "mapper-164-WaixingFinalFantasy/Digital Dragon (Ch) [!].zip",
        DEFAULT_IDLE,
        "extended_m164_digital_dragon",
    );
}

#[test]
fn extended_m176_12_in_1_console_tv_game_cartridge() {
    check(
        "mapper-176-WaixingFK23C/12-in-1 Console TV Game Cartridge (Unl) [!].zip",
        DEFAULT_IDLE,
        "extended_m176_12_in_1_console_tv_game_cartridge",
    );
}

#[test]
fn extended_m189_mario_fighter_iii() {
    check(
        "mapper-189-TXC-MMC3/Mario Fighter III (Unl) [!].zip",
        DEFAULT_IDLE,
        "extended_m189_mario_fighter_iii",
    );
}

#[test]
fn extended_m193_war_in_the_gulf() {
    check(
        "mapper-193-NTDEC-TC112/War in The Gulf (B) (Unl) [!].zip",
        DEFAULT_IDLE,
        "extended_m193_war_in_the_gulf",
    );
}

#[test]
fn extended_m204_64_in_1() {
    check(
        "mapper-204-BMC-64in1/64-in-1 [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m204_64_in_1",
    );
}

#[test]
fn extended_m205_4_in_1() {
    check(
        "mapper-205-BMC-JC016/4-in-1 (K-3131GS, GN-45) [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m205_4_in_1",
    );
}

#[test]
fn extended_m209_mike_tyson_s_punch_out() {
    check(
        "mapper-209-JYCompany209/Mike Tyson's Punch-Out!! (Unl) [!].zip",
        DEFAULT_IDLE,
        "extended_m209_mike_tyson_s_punch_out",
    );
}

#[test]
fn extended_m211_2_in_1_donkey_kong_country_4_jungle_book_2() {
    check(
        "mapper-211-JYCompany211/2-in-1 - Donkey Kong Country 4 + Jungle Book 2 (Unl) [!].zip",
        DEFAULT_IDLE,
        "extended_m211_2_in_1_donkey_kong_country_4_jungle_book_2",
    );
}

#[test]
fn extended_m221_1000_in_1() {
    check(
        "mapper-221-NTDEC-N625092/1000-in-1 (JPx72) [p1][!].zip",
        DEFAULT_IDLE,
        "extended_m221_1000_in_1",
    );
}

#[test]
fn extended_m245_di_guo_shi_dai() {
    check(
        "mapper-245-WaixingMMC3/Di Guo Shi Dai (Age of Empires) (ES-1070) (Ch).zip",
        DEFAULT_IDLE,
        "extended_m245_di_guo_shi_dai",
    );
}

#[test]
fn extended_m253_dragon_ball_z_kyoushuu_saiya_jin_qi_long_zhu() {
    check(
        "mapper-253-WaixingVRC4-DBZ/Dragon Ball Z - Kyoushuu! Saiya Jin Qi Long Zhu (ES-1064) (Ch).zip",
        DEFAULT_IDLE,
        "extended_m253_dragon_ball_z_kyoushuu_saiya_jin_qi_long_zhu",
    );
}
