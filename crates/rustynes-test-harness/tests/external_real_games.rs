//! Commercial-ROM regression-prevention baselines.
//!
//! Boots every commercial ROM staged under `tests/roms/external/`
//! (gitignored — user-supplied dumps), runs a deterministic input
//! script, and asserts against an `insta` snapshot that records:
//!
//! - the ROM's SHA-256 (so a different dump fails with a clear
//!   diagnostic instead of a cryptic framebuffer-hash mismatch),
//! - the framebuffer FNV-1a 64-bit hash at one or more checkpoints,
//! - the cumulative CPU cycle count at the final frame,
//! - the FNV-1a 64-bit hash of the drained audio samples (raw f32 LE
//!   bytes) over the run,
//! - the count of audio samples produced.
//!
//! ## Why a separate file from the committed-ROM harnesses
//!
//! The 21 permissively-licensed test ROMs under `tests/roms/` are
//! committed; their snapshots live alongside the harness as canonical
//! reference. The 60 commercial ROMs at `tests/roms/external/` are
//! never committed (copyright); the snapshots are committed (emulator
//! output, not ROM bytes — no copyright concern), so a developer with
//! their own ROM dumps gets a working regression net.
//!
//! ## Structure
//!
//! One test function per ROM. Each test:
//!
//! 1. Builds an [`common::external::InputScript`] (default
//!    `IdleOnly { frames: 600 }`; the 3 grandfathered ROMs preserve
//!    their original `StartTap` 3-checkpoint coverage).
//! 2. Calls [`common::external::run_capture`] to run the script and
//!    return a [`common::external::CaptureResult`].
//! 3. Asserts the result against an `insta` snapshot via
//!    [`common::external::snapshot_text`].
//!
//! ## Feature gating
//!
//! ```text
//! cargo test -p rustynes-test-harness --features commercial-roms,test-roms \
//!     --test external_real_games -- --nocapture
//! ```
//!
//! Default `cargo test --workspace --features test-roms` does **not**
//! run this harness — `commercial-roms` is off by default so CI never
//! depends on non-distributable assets.
//!
//! ## Recapturing baselines
//!
//! ```bash
//! INSTA_UPDATE=auto RUSTYNES_DUMP_FRAMES=1 \
//!     cargo test -p rustynes-test-harness --features commercial-roms,test-roms \
//!     --test external_real_games -- --test-threads=1 --nocapture
//! # Visually inspect the PNGs at /tmp/rustynes-baseline-screenshots/external/
//! # Accept the .snap.new files with: cargo insta accept
//! ```
//!
//! Why `--test-threads=1`: the harness runs 60 ROMs back-to-back, each
//! constructing a fresh `Nes`. The parallel allocator pressure isn't
//! the issue; serializing keeps the PNG-dump filesystem path order
//! readable and stops insta snapshot diff order from depending on
//! thread scheduling.
//!
//! ## Per-ROM `#[ignore]` policy
//!
//! ROMs that boot to a black frame, halt the CPU, or render a
//! transparently-broken visual at the baseline frame are `#[ignore]`'d
//! with a `// reason: …` comment AND documented in the audit. This is
//! deliberate: committing a baseline of broken-emulator behavior locks
//! in the broken state. Better to leave the test red-but-acknowledged
//! and fix the underlying mapper/CPU/PPU bug.

#![cfg(feature = "commercial-roms")]
#![allow(clippy::doc_markdown)]
#![allow(clippy::too_many_lines)]

mod common;

use common::external::{InputScript, run_capture, snapshot_text};

/// Default script: 600 frames idle (no input). 10 seconds @ NTSC 60 Hz
/// is past the title-screen ramp-up + initial demo-loop cycle for
/// every commercial ROM tested. Title screens and attract-mode loops
/// are deterministic, so any regression in PPU rendering / scrolling /
/// sprite-eval / mapper-CHR-banking / audio-mixer surfaces as a
/// snapshot mismatch.
const DEFAULT_IDLE: InputScript = InputScript::IdleOnly { frames: 600 };

/// `StartTap` script preserved from the original 3-ROM oracle (commit
/// `3e53802`). Idle 120 → 1-frame START → idle 119 → free-run 360,
/// captures at frames 120 / 240 / 600. The 3 grandfathered ROMs
/// (Super Mario Bros. / Excitebike / Kid Icarus) retain this coverage
/// so the pre-FSM-fix regression-detection power is preserved.
const START_TAP_120_240_600: InputScript = InputScript::StartTap {
    idle_pre: 120,
    idle_post: 119,
    free_run: 360,
    checkpoints: &[120, 240, 600],
};

/// Some titles need TWO START presses to reach gameplay (e.g. Kid Icarus:
/// title → story-intro menu → game; Excitebike: title → mode-select → game),
/// so a single `StartTap` leaves the f600 screenshot on the menu. Idle 120
/// (title) → START → 120 idle (menu) → START → free-run; captures f120 (title)
/// / f240 (menu) / f600 (gameplay). total = 600. The 2nd tap lands at frame
/// 242, so f120 + f240 stay identical to the single-tap script.
const DOUBLE_START_120_240_600: InputScript = InputScript::DoubleStartTap {
    idle_pre: 120,
    gap: 120,
    idle_post: 0,
    free_run: 358,
    checkpoints: &[120, 240, 600],
};

/// START tapped every 10 s through a long multi-stage intro to reach the
/// menu. Bandit Kings of Ancient China sits on the KOEI publisher splash
/// at f600; its f2000 checkpoint lands on the main strategy menu.
const MENU_REPEAT_START: InputScript = InputScript::RepeatStartTap {
    warmup: 90,
    period: 600,
    taps: 4,
    free_run: 200,
    checkpoints: &[2000],
};

/// `StartTap` script for ROMs with a long pre-menu intro sequence
/// (~60+ s) where the default 600-frame `IdleOnly` baseline lands on
/// an animated demo frame that obscures the actual title-or-menu
/// state. Idle 3600 (60 s @ 60 Hz) → 1-frame START → idle 60 →
/// free-run 240; captures at the post-START +60 (frame 3661) and
/// +300 (frame 3961) marks, which both land on the post-intro menu.
///
/// Used by Mr. Gimmick (FME-7) + Tiny Toon Adventures 2 (MMC3) per
/// the T-60-003 investigation (2026-05-17). Diagnostic harness at
/// `tests/ignored_roms_diagnostic.rs` measured 10 distinct
/// framebuffer states across frames 60..3600 for both ROMs (game
/// IS running) but the f600 capture landed on a uniform-palette
/// animation tick that matched the well-known "audio-test rendering-
/// disabled" hash, hence the pre-fix false-positive `#[ignore]`.
const LONG_INTRO_START_3600: InputScript = InputScript::StartTap {
    idle_pre: 3600,
    idle_post: 60,
    free_run: 240,
    checkpoints: &[3661, 3901],
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
fn external_nrom_super_mario_bros() {
    check(
        "mapper-000-NROM/Super Mario Bros.nes",
        START_TAP_120_240_600,
        "external_nrom_super_mario_bros",
    );
}

#[test]
fn external_nrom_excitebike() {
    check(
        "mapper-000-NROM/Excitebike.nes",
        DOUBLE_START_120_240_600,
        "external_nrom_excitebike",
    );
}

#[test]
fn external_nrom_donkey_kong() {
    check(
        "mapper-000-NROM/Donkey Kong.nes",
        DEFAULT_IDLE,
        "external_nrom_donkey_kong",
    );
}

#[test]
fn external_nrom_balloon_fight() {
    check(
        "mapper-000-NROM/Balloon Fight.nes",
        DEFAULT_IDLE,
        "external_nrom_balloon_fight",
    );
}

#[test]
fn external_nrom_ice_climber() {
    check(
        "mapper-000-NROM/Ice Climber.nes",
        DEFAULT_IDLE,
        "external_nrom_ice_climber",
    );
}

#[test]
fn external_nrom_gyromite() {
    check(
        "mapper-000-NROM/Gyromite.nes",
        DEFAULT_IDLE,
        "external_nrom_gyromite",
    );
}

// ============================================================
// Mapper 001 — MMC1 (7 ROMs)
// ============================================================

#[test]
fn external_mmc1_kid_icarus() {
    check(
        "mapper-001-MMC1/Kid Icarus.nes",
        DOUBLE_START_120_240_600,
        "external_mmc1_kid_icarus",
    );
}

#[test]
fn external_mmc1_legend_of_zelda() {
    check(
        "mapper-001-MMC1/Legend of Zelda, The.nes",
        DEFAULT_IDLE,
        "external_mmc1_legend_of_zelda",
    );
}

#[test]
fn external_mmc1_metroid() {
    check(
        "mapper-001-MMC1/Metroid.nes",
        DEFAULT_IDLE,
        "external_mmc1_metroid",
    );
}

#[test]
fn external_mmc1_final_fantasy() {
    check(
        "mapper-001-MMC1/Final Fantasy.nes",
        DEFAULT_IDLE,
        "external_mmc1_final_fantasy",
    );
}

#[test]
fn external_mmc1_mega_man_2() {
    check(
        "mapper-001-MMC1/Mega Man 2.nes",
        DEFAULT_IDLE,
        "external_mmc1_mega_man_2",
    );
}

#[test]
fn external_mmc1_castlevania_2() {
    check(
        "mapper-001-MMC1/Castlevania II - Simon's Quest.nes",
        DEFAULT_IDLE,
        "external_mmc1_castlevania_2",
    );
}

#[test]
fn external_mmc1_ninja_gaiden() {
    check(
        "mapper-001-MMC1/Ninja Gaiden.nes",
        DEFAULT_IDLE,
        "external_mmc1_ninja_gaiden",
    );
}

// ============================================================
// Mapper 002 — UxROM (4 ROMs)
// ============================================================

#[test]
fn external_uxrom_castlevania() {
    check(
        "mapper-002-UxROM/Castlevania.nes",
        DEFAULT_IDLE,
        "external_uxrom_castlevania",
    );
}

#[test]
fn external_uxrom_mega_man() {
    check(
        "mapper-002-UxROM/Mega Man.nes",
        DEFAULT_IDLE,
        "external_uxrom_mega_man",
    );
}

#[test]
fn external_uxrom_contra() {
    check(
        "mapper-002-UxROM/Contra.nes",
        DEFAULT_IDLE,
        "external_uxrom_contra",
    );
}

#[test]
fn external_uxrom_ducktales() {
    check(
        "mapper-002-UxROM/Disney's DuckTales.nes",
        DEFAULT_IDLE,
        "external_uxrom_ducktales",
    );
}

// ============================================================
// Mapper 003 — CNROM (3 ROMs)
// ============================================================

#[test]
fn external_cnrom_arkanoid() {
    check(
        "mapper-003-CNROM/Arkanoid.nes",
        DEFAULT_IDLE,
        "external_cnrom_arkanoid",
    );
}

#[test]
fn external_cnrom_gradius() {
    check(
        "mapper-003-CNROM/Gradius.nes",
        DEFAULT_IDLE,
        "external_cnrom_gradius",
    );
}

#[test]
fn external_cnrom_paperboy() {
    check(
        "mapper-003-CNROM/Paperboy.nes",
        DEFAULT_IDLE,
        "external_cnrom_paperboy",
    );
}

// ============================================================
// Mapper 004 — MMC3 (7 ROMs)
// ============================================================

#[test]
fn external_mmc3_super_mario_bros_3() {
    check(
        "mapper-004-MMC3/Super Mario Bros. 3.nes",
        DEFAULT_IDLE,
        "external_mmc3_super_mario_bros_3",
    );
}

#[test]
fn external_mmc3_super_mario_bros_2() {
    check(
        "mapper-004-MMC3/Super Mario Bros. 2.nes",
        DEFAULT_IDLE,
        "external_mmc3_super_mario_bros_2",
    );
}

#[test]
fn external_mmc3_mega_man_3() {
    check(
        "mapper-004-MMC3/Mega Man 3.nes",
        DEFAULT_IDLE,
        "external_mmc3_mega_man_3",
    );
}

#[test]
fn external_mmc3_kirbys_adventure() {
    check(
        "mapper-004-MMC3/Kirby's Adventure.nes",
        DEFAULT_IDLE,
        "external_mmc3_kirbys_adventure",
    );
}

#[test]
fn external_mmc3_ninja_gaiden_2() {
    check(
        "mapper-004-MMC3/Ninja Gaiden II - The Dark Sword of Chaos.nes",
        DEFAULT_IDLE,
        "external_mmc3_ninja_gaiden_2",
    );
}

#[test]
fn external_mmc3_tmnt3() {
    check(
        "mapper-004-MMC3/Teenage Mutant Ninja Turtles III - The Manhattan Project.nes",
        DEFAULT_IDLE,
        "external_mmc3_tmnt3",
    );
}

#[test]
fn external_mmc3_tiny_toon_adventures_2() {
    // T-60-003 (2026-05-17): formerly #[ignore]'d; diagnostic
    // confirmed the game runs fine but its intro is ~60 s long. The
    // LONG_INTRO_START_3600 script taps START at frame 3600 and
    // captures the post-intro WACKYLAND menu screen at +60 / +300.
    check(
        "mapper-004-MMC3/Tiny Toon Adventures 2 - Trouble in Wackyland.nes",
        LONG_INTRO_START_3600,
        "external_mmc3_tiny_toon_adventures_2",
    );
}

// ============================================================
// Mapper 005 — MMC5 (3 ROMs)
// ============================================================

#[test]
fn external_mmc5_castlevania_3() {
    check(
        "mapper-005-MMC5/Castlevania III - Dracula's Curse.nes",
        DEFAULT_IDLE,
        "external_mmc5_castlevania_3",
    );
}

#[test]
fn external_mmc5_bandit_kings_of_ancient_china() {
    check(
        "mapper-005-MMC5/Bandit Kings of Ancient China.nes",
        MENU_REPEAT_START,
        "external_mmc5_bandit_kings_of_ancient_china",
    );
}

#[test]
fn external_mmc5_uchuu_keibitai_sdf() {
    check(
        "mapper-005-MMC5/Uchuu Keibitai SDF (Japan).nes",
        DEFAULT_IDLE,
        "external_mmc5_uchuu_keibitai_sdf",
    );
}

// ============================================================
// Mapper 007 — AxROM (4 ROMs)
// ============================================================

#[test]
fn external_axrom_battletoads() {
    check(
        "mapper-007-AxROM/Battletoads.nes",
        DEFAULT_IDLE,
        "external_axrom_battletoads",
    );
}

#[test]
fn external_axrom_marble_madness() {
    check(
        "mapper-007-AxROM/Marble Madness.nes",
        DEFAULT_IDLE,
        "external_axrom_marble_madness",
    );
}

#[test]
fn external_axrom_cobra_triangle() {
    check(
        "mapper-007-AxROM/Cobra Triangle.nes",
        DEFAULT_IDLE,
        "external_axrom_cobra_triangle",
    );
}

#[test]
fn external_axrom_solstice() {
    check(
        "mapper-007-AxROM/Solstice - The Quest for the Staff of Demnos.nes",
        DEFAULT_IDLE,
        "external_axrom_solstice",
    );
}

// ============================================================
// Mapper 009 — MMC2 (2 ROMs)
// ============================================================

#[test]
fn external_mmc2_punch_out() {
    check(
        "mapper-009-MMC2/Punch-Out!!.nes",
        DEFAULT_IDLE,
        "external_mmc2_punch_out",
    );
}

#[test]
fn external_mmc2_mike_tyson_punch_out() {
    check(
        "mapper-009-MMC2/Mike Tyson's Punch-Out!!.nes",
        DEFAULT_IDLE,
        "external_mmc2_mike_tyson_punch_out",
    );
}

// ============================================================
// Mapper 010 — MMC4 (3 ROMs)
// ============================================================

#[test]
fn external_mmc4_famicom_wars() {
    check(
        "mapper-010-MMC4/Famicom Wars (Japan) (En) (1.11) (Good dump + Title screen).nes",
        DEFAULT_IDLE,
        "external_mmc4_famicom_wars",
    );
}

#[test]
fn external_mmc4_fire_emblem() {
    check(
        "mapper-010-MMC4/Fire Emblem - Ankoku Ryuu to Hikari no Tsurugi (Japan) (En) (1.0) (Official names).nes",
        DEFAULT_IDLE,
        "external_mmc4_fire_emblem",
    );
}

#[test]
fn external_mmc4_fire_emblem_gaiden() {
    // T-60-003c (2026-05-17): un-ignored after the MMC4 WRAM bug fix
    // (same root cause as VRC2/4/6 — $6000-$7FFF was returning 0
    // instead of prg_ram contents). Post-fix the title screen
    // ("Fire Emblem" with sword + green emblem + ©1991 Nintendo)
    // renders cleanly.
    check(
        "mapper-010-MMC4/Fire Emblem Gaiden (Japan) (En) (1.01).nes",
        DEFAULT_IDLE,
        "external_mmc4_fire_emblem_gaiden",
    );
}

// ============================================================
// Mapper 019 — Namco 163 (4 ROMs)
// ============================================================

#[test]
fn external_namco163_famista_90() {
    check(
        "mapper-019-Namco163/Famista '90 (Japan) (En) (0.91).nes",
        DEFAULT_IDLE,
        "external_namco163_famista_90",
    );
}

#[test]
fn external_namco163_famista_91() {
    check(
        "mapper-019-Namco163/Famista '91 (Japan) (En) (0.99).nes",
        DEFAULT_IDLE,
        "external_namco163_famista_91",
    );
}

#[test]
fn external_namco163_final_lap() {
    check(
        "mapper-019-Namco163/Final Lap (Japan).nes",
        DEFAULT_IDLE,
        "external_namco163_final_lap",
    );
}

#[test]
fn external_namco163_mappy_kids() {
    check(
        "mapper-019-Namco163/Mappy Kids (Japan) (En) (1.0).nes",
        DEFAULT_IDLE,
        "external_namco163_mappy_kids",
    );
}

// ============================================================
// Mapper 021 — VRC2/VRC4 variant a (1 ROM)
// ============================================================

#[test]
fn external_vrc4_wai_wai_world_2() {
    check(
        "mapper-021-VRC2-VRC4/Wai Wai World 2 - SOS!! Parsley Jou (Japan) (En) (1.01) (2018 update).nes",
        DEFAULT_IDLE,
        "external_vrc4_wai_wai_world_2",
    );
}

// ============================================================
// Mapper 022 — VRC2a (1 ROM)
// ============================================================

#[test]
fn external_vrc2a_twinbee_3() {
    check(
        "mapper-022-VRC2/TwinBee 3 - Poko Poko Daimaou (Japan) (En) (1.01).nes",
        DEFAULT_IDLE,
        "external_vrc2a_twinbee_3",
    );
}

// ============================================================
// Mapper 023 — VRC2b/VRC4e/4f (4 ROMs)
// ============================================================

#[test]
fn external_vrc4_akumajou_special() {
    check(
        "mapper-023-VRC2-VRC4/Akumajou Special - Boku Dracula-kun (Japan) (En) (1.0).nes",
        DEFAULT_IDLE,
        "external_vrc4_akumajou_special",
    );
}

#[test]
fn external_vrc4_crisis_force() {
    check(
        "mapper-023-VRC2-VRC4/Crisis Force (Japan) (En) (1.0).nes",
        DEFAULT_IDLE,
        "external_vrc4_crisis_force",
    );
}

#[test]
fn external_vrc4_ganbare_goemon_2() {
    // T-60-003b/c (2026-05-17): un-ignored after the VRC4 WRAM bug
    // fix. The mapper was returning 0 from $6000-$7FFF reads instead
    // of the prg_ram contents; Konami's Goemon series reads save-data
    // magic from this region at boot, so the game was stuck-at-
    // uniform-gray hash 89ee4c476c97a325. Post-fix the title screen
    // ("Konami / GAMBARE GOEMON 2 / Konami 1989") renders cleanly.
    check(
        "mapper-023-VRC2-VRC4/Ganbare Goemon 2 (Japan) (En) (1.02).nes",
        DEFAULT_IDLE,
        "external_vrc4_ganbare_goemon_2",
    );
}

#[test]
fn external_vrc4_wai_wai_world() {
    check(
        "mapper-023-VRC2-VRC4/Wai Wai World (Japan) (En) (2.2).nes",
        DEFAULT_IDLE,
        "external_vrc4_wai_wai_world",
    );
}

// ============================================================
// Mapper 024 — VRC6a (1 ROM)
// ============================================================

#[test]
fn external_vrc6a_castlevania_3_retranslation() {
    check(
        "mapper-024-VRC6/Castlevania III - Dracula's Curse - Retranslation and Improved Controls (Castlevania III - Dracula's Curse modification) (7.1 & 1.4).nes",
        DEFAULT_IDLE,
        "external_vrc6a_castlevania_3_retranslation",
    );
}

// ============================================================
// Mapper 025 — VRC4 variant (1 ROM)
// ============================================================

#[test]
fn external_vrc4_ganbare_goemon_gaiden() {
    check(
        "mapper-025-VRC2-VRC4/Ganbare Goemon Gaiden - Kieta Ougon Kiseru (Japan) (En) (0.99c).nes",
        DEFAULT_IDLE,
        "external_vrc4_ganbare_goemon_gaiden",
    );
}

// ============================================================
// Mapper 026 — VRC6b (2 ROMs)
// ============================================================

#[test]
fn external_vrc6b_esper_dream_2() {
    // T-60-003b (2026-05-17): un-ignored after the VRC6 WRAM bug fix
    // (matches the VRC4 fix above; the VRC6b pinout-decoder hypothesis
    // was wrong — actual bug was missing $6000-$7FFF WRAM read/write
    // path on the VRC6 mapper). Post-fix title screen ("Esper Dream
    // 2" pink logo + New Game/Load Game menu + ©1992 Konami) renders.
    check(
        "mapper-026-VRC6/Esper Dream 2 - Aratanaru Tatakai (Japan) (En) (1.0).nes",
        DEFAULT_IDLE,
        "external_vrc6b_esper_dream_2",
    );
}

#[test]
fn external_vrc6b_madara() {
    // T-60-003b (2026-05-17): un-ignored after the VRC6 WRAM bug fix
    // (same root cause as Esper Dream 2 above). Post-fix the title
    // screen ("MADARA" logo + ©KONAMI 1990 + "PUSH START KEY")
    // renders cleanly.
    check(
        "mapper-026-VRC6/Mouryou Senki Madara (Japan) (En) (1.0).nes",
        DEFAULT_IDLE,
        "external_vrc6b_madara",
    );
}

// ============================================================
// Mapper 066 — GxROM (2 ROMs)
// ============================================================

#[test]
fn external_gxrom_doraemon() {
    check(
        "mapper-066-GxROM/Doraemon (Japan) (En) (1.0).nes",
        DEFAULT_IDLE,
        "external_gxrom_doraemon",
    );
}

#[test]
fn external_gxrom_thunder_and_lightning() {
    check(
        "mapper-066-GxROM/Thunder & Lightning.nes",
        DEFAULT_IDLE,
        "external_gxrom_thunder_and_lightning",
    );
}

// ============================================================
// Mapper 069 — Sunsoft FME-7 / 5B (2 ROMs)
// ============================================================

#[test]
fn external_fme7_mr_gimmick() {
    // T-60-003 (2026-05-17): formerly #[ignore]'d. Mr. Gimmick! has
    // a notoriously long FME-7 splash + Sunsoft logo + animated
    // intro that exceeded the default 600-frame budget. Diagnostic
    // confirmed 10 distinct framebuffer states across frames 60..
    // 3600 (game IS running). LONG_INTRO_START_3600 taps START at
    // frame 3600 and captures the post-intro "GIMMICK! / START /
    // CONTINUE" menu screen at +60 / +300.
    check(
        "mapper-069-FME7-Sunsoft5B/Mr. Gimmick.nes",
        LONG_INTRO_START_3600,
        "external_fme7_mr_gimmick",
    );
}

#[test]
fn external_fme7_batman_return_of_the_joker() {
    check(
        "mapper-069-FME7-Sunsoft5B/Batman - Return of the Joker.nes",
        DEFAULT_IDLE,
        "external_fme7_batman_return_of_the_joker",
    );
}

// ============================================================
// Mapper 075 — VRC1 (2 ROMs)
// ============================================================

#[test]
fn external_vrc1_ganbare_goemon() {
    // v2.0 Tier 1.1 (2026-06-03): the `audio_fnv1a64` checkpoint was
    // re-baselined when `ppu-2002-read-end-flags` was promoted to default.
    // Goemon reads `$2002` within 1 dot of the pre-render flag-clear and
    // consumes the returned sprite-0/overflow bits in its audio/RNG path.
    // The two-point read sample masks those bits (the AccuracyCoin
    // `$2002 flag timing` answer-key author proves real hardware returns
    // exactly this masked byte), so the prior golden captured the
    // LESS-accurate single-point behavior. Framebuffer hash + cycle count
    // are byte-identical; only the audio hash shifted. See
    // `docs/audit/v2.0-pivot-port-log-2026-06-03.md`.
    check(
        "mapper-075-VRC1/Ganbare Goemon! Karakuri Douchuu (Japan) (En) (1.01).nes",
        DEFAULT_IDLE,
        "external_vrc1_ganbare_goemon",
    );
}

#[test]
fn external_vrc1_king_kong_2() {
    check(
        "mapper-075-VRC1/King Kong 2 - Ikari no Megaton Punch (Japan) (En) (Rev-A).nes",
        DEFAULT_IDLE,
        "external_vrc1_king_kong_2",
    );
}

// ============================================================
// Mapper 085 — VRC7 (1 ROM)
// ============================================================
//
// Per ADR-0004, the VRC7 FM (OPLL) synthesizer is deferred —
// `Mapper::mix_audio` returns 0. The mapper banking + IRQ +
// register-surface latching that DID land is what this baseline
// guards. The audio FNV hash locks in the silence; when the FM synth
// eventually lands, this baseline is expected to flip and the
// snapshot needs re-capture.

#[test]
fn external_vrc7_lagrange_point() {
    check(
        "mapper-085-VRC7/Lagrange Point (Japan) (En) (1.01).nes",
        DEFAULT_IDLE,
        "external_vrc7_lagrange_point",
    );
}
