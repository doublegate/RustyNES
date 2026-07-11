//! Tepples / Damian Yerrick `holy_mapperel` cartridge-PCB-assembly oracle —
//! the **mapper bank-reachability + IRQ regression net** (v2.1.5 "Regression
//! Net & Residual", primary item).
//!
//! Source: <https://github.com/pinobatch/holy-mapperel> v0.02 release
//! (`holy-mapperel-bin-0.02.7z`). The README's "Legal" section places the
//! ROMs under the **zlib license** (permissive redistribution). The license
//! provenance + per-file attribution is recorded in `tests/roms/LICENSES.md`;
//! the ROMs are vendored under `tests/roms/holy_mapperel/` and gated on the
//! default `--features test-roms`, so this suite runs in the same CI job as
//! `AccuracyCoin` / blargg / kevtris.
//!
//! # What this catches that `AccuracyCoin` / blargg do not
//!
//! Holy Mapperel is a manufacturing self-test: on power-on it (1) *detects*
//! which mapper it is running on purely from the console's mirroring +
//! bank-switching response (no header trust), (2) sizes PRG/CHR ROM/RAM by
//! walking bank tags, (3) proves every PRG and CHR bank is reachable, and (4)
//! exercises WRAM presence/protection and the mapper's IRQ (MMC3 / FME-7)
//! interval timer. `AccuracyCoin` and the blargg CPU/PPU corpora barely touch
//! mapper banking at all, and the 60-ROM commercial oracle is gitignored
//! (non-distributable) — so before this net a silent bank-reachability or
//! mapper-detection regression across the 172-family set could slip through CI
//! entirely. This net is deliberately a *sentinel*: it pins the exact,
//! known-good result screen per ROM and **fails loudly** on any deviation. It
//! promotes nothing and claims no new accuracy grade.
//!
//! # How the assertion works
//!
//! Holy Mapperel reports its verdict **visually** (on-screen text + Morse-coded
//! audio) — there is *no* blargg `$6000` status-byte protocol to poll (see
//! `docs/testing-strategy.md` §Layer 3). So, exactly as `visual_regression.rs`
//! does for the status-protocol-less demos, each ROM is driven to its settled
//! result screen and its framebuffer is fingerprinted with an FNV-1a 64-bit
//! hash pinned via `insta`. The determinism contract (same seed + ROM ⇒
//! byte-identical framebuffer, enforced cross-platform by
//! `nes_determinism_two_runs_match`) makes the hash a portable golden. Any
//! change to a mapper's detection, bank layout, RAM sizing, or IRQ handling
//! shifts the on-screen result and flips that ROM's hash → the combined
//! snapshot diff names the offending ROM. Two cheap structural guards run
//! *before* the hash so a hard failure surfaces with a readable message rather
//! than an opaque hash flip:
//!
//! * **Settled** — the framebuffer is byte-identical at frame [`PIN_FRAME`] and
//!   `PIN_FRAME + SETTLE_FRAMES`. A ROM that wedged in the 4 KiB Morse-code
//!   crash handler (a hard mapper fault) never reaches a static screen.
//! * **Non-blank** — the settled screen carries at least two distinct colours
//!   (the result screen is yellow text on a blue backdrop). A crash that blanks
//!   the display to the backdrop alone collapses to one colour.
//!
//! # Per-ROM expected outcome (verified 2026-07-11 by rendering each screen)
//!
//! The `expect` column is a *static, human-verified* label; the FNV-1a hash in
//! the snapshot is the *live* sentinel. A ROM whose label says `PASS 0000` but
//! whose hash changes is a genuine regression to investigate. All 17 committed
//! ROMs detect the correct mapper and prove full PRG/CHR bank reachability with
//! every RAM/ROM/IRQ sub-test `OK`; the "detailed result" 4-digit code is
//! `WRAM · PRG ROM · IRQ · CHR` where `0` is normal (README §"Displayed
//! result").
//!
//! | ROM | Detected | Detailed | Class |
//! |-----|----------|----------|-------|
//! | `M0_*` (×2)         | 000 NROM           | `0000` | PASS |
//! | `M1_P128K_C32K`     | 001 SJROM (MMC1)   | `1000` | WRAM residual |
//! | `M1_P128K_CR8K`     | 001 SNROM (MMC1)   | `5000` | WRAM residual |
//! | `M2_P128K_CR8K_V`   | 002 UNROM          | `0000` | PASS |
//! | `M3_P32K_C32K_H`    | 003 CNROM          | `0000` | PASS |
//! | `M4_*` (×3)         | 004 T[N/S]ROM MMC3 | `0000` | PASS (incl. IRQ) |
//! | `M7_P128K_CR8K`     | 007 ANROM (`AxROM`)  | `0000` | PASS |
//! | `M9_P128K_C64K`     | 009 PNROM (MMC2)   | `0000` | PASS |
//! | `M10_*` (×2)        | 010 F*ROM (MMC4)   | `0000` | PASS |
//! | `M34_P128K_CR8K_H`  | 034 BNROM          | `0000` | PASS (NES 2.0 dual-reg OK) |
//! | `M66_P64K_C16K_V`   | 066 MHROM (`GxROM`)  | `0000` | PASS |
//! | `M69_*` (×2)        | 069 J*ROM (FME-7)  | `1000` | WRAM residual (IRQ OK) |
//!
//! ## The WRAM-protection residual (`M1_*`, `M69_*`: nonzero WRAM nibble)
//!
//! Both `M1_*` and `M69_*` land a `1` in the WRAM nibble, but for **different
//! reasons** — the "`RustyNES` treats WRAM as always-enabled" story is accurate
//! for MMC1 only; FME-7 does model its RAM-enable bits and fails a narrower,
//! open-bus edge instead.
//!
//! **MMC1 (`M1_*` = `1000` / `5000`).** MMC1 provides a *software WRAM
//! write-protect* bit (`$E000` bit 4; SNROM adds a second `$A000` bit-4 layer).
//! `RustyNES` does **not** model it: `crates/rustynes-mappers/src/mmc1.rs`
//! reads and writes `$6000-$7FFF` `prg_ram` unconditionally, ignoring the
//! disable bit. Holy Mapperel's disable sub-checks therefore fail — `1000` on
//! SJROM (`$E000` layer = `MAPTEST_WRAMEN` `$10`), `5000` on SNROM (both
//! layers, `MAPTEST_WRAMEN2 $40 | MAPTEST_WRAMEN $10`). This is a deliberate,
//! widely-shared simplification: Holy Mapperel's own README notes FCEUX and
//! `PowerPak` omit it too, and modelling MMC1 RAM-disable is a notorious
//! game-compatibility hazard.
//!
//! **FME-7 (`M69_*` = `1000`).** FME-7 is *not* an "always-enabled WRAM" case.
//! `crates/rustynes-mappers/src/sprint3.rs` **does** model the command-`$8`
//! RAM-enable (bit 7, `$80`) and RAM-select (bit 6, `$40`) bits: it maps
//! PRG-RAM at `$6000-$7FFF` only when *both* are set, and maps a PRG-ROM bank
//! when RAM is deselected (bit 6 = 0). Its `1` nibble is a narrower gap in a
//! single register state. The FME-7 driver's WRAM test does three sub-checks:
//! write-then-read RAM (`$C0|$3F` = select + enable, **passes** — RAM works),
//! read the last ROM bank (`$00|$3F` = deselect, **passes** — ROM works), then
//! read the *RAM-selected-but-disabled* window (`$40|$3F` = bit 6 = 1, bit 7 =
//! 0) and require an **open-bus** byte `>= 3` (a `$7F` from the disconnected
//! bus). On real hardware that state drives neither the RAM nor the ROM chip,
//! so the read floats to open bus; `RustyNES` instead falls through to the
//! PRG-ROM bank (`value & $3F` = the last 8 KiB bank) and returns its bank tag
//! (`1`, which is `< 3`), so the driver sets `MAPTEST_WRAMEN` (`$10`) → WRAM
//! nibble `1`. No known FME-7 game reads `$6000-$7FFF` in that
//! RAM-selected-yet-disabled state, so closing the gap is not provably
//! byte-identical against the commercial oracle; it is left as a documented,
//! deferred open-bus edge rather than a speculative core change.
//!
//! Neither case is a bank-reachability defect — every bank is reachable and
//! every other sub-test is `0`, including the FME-7 IRQ nibble (`0` = the
//! interval-timer IRQ works). Both are recorded in `docs/accuracy-ledger.md`.
//! This net pins the honest current code (`1000` / `5000`) rather than
//! blind-passing — if either path is ever modelled, these hashes flip and force
//! a conscious re-bless.

#![cfg(feature = "test-roms")]

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use rustynes_core::Nes;

/// Frame at which the result screen is fingerprinted. Holy Mapperel buzzes the
/// speaker through the CHR-RAM pattern sweep, then draws a static result
/// screen; empirically every committed ROM is settled well before this (~10 s
/// of NES time — a generous margin past the longest CHR-RAM sweep).
const PIN_FRAME: u64 = 600;

/// Extra frames advanced past [`PIN_FRAME`] to prove the screen is static (the
/// test finished and did not wedge in the Morse-code crash redraw loop).
const SETTLE_FRAMES: u64 = 60;

/// The committed subset is 17 ROMs; guard against a corpus that was silently
/// trimmed (a shrunk net catches less).
const MIN_COMMITTED_ROMS: usize = 17;

/// FNV-1a 64-bit hash of a framebuffer — a tiny, dependency-free, byte-exact,
/// cross-platform-stable fingerprint (the same primitive `visual_regression.rs`
/// pins). The determinism contract guarantees an identical hash on every host.
fn fnv1a64(fb: &[u8]) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for &b in fb {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Number of distinct RGBA colours in a framebuffer. A settled result screen
/// has two (text + backdrop); a blanked crash screen collapses to one.
fn distinct_colors(fb: &[u8]) -> usize {
    let mut px: Vec<u32> = fb
        .chunks_exact(4)
        .map(|p| u32::from_le_bytes([p[0], p[1], p[2], p[3]]))
        .collect();
    px.sort_unstable();
    px.dedup();
    px.len()
}

/// Directory holding the committed, zlib-licensed Holy Mapperel ROM subset.
fn rom_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join("holy_mapperel")
}

/// Static, human-verified expectation for a ROM stem (see the module table).
/// New ROMs dropped into the corpus that are not yet classified render as
/// `UNVERIFIED`, which — together with their new snapshot line — forces a
/// visual check + conscious re-bless before they can go green.
fn expect_label(stem: &str) -> &'static str {
    match stem {
        "M0_P32K_CR32K_V" | "M0_P32K_CR8K_V" => "PASS NROM(000) detail=0000",
        "M1_P128K_C32K" => "WRAM-RESIDUAL SJROM/MMC1(001) detail=1000",
        "M1_P128K_CR8K" => "WRAM-RESIDUAL SNROM/MMC1(001) detail=5000",
        "M2_P128K_CR8K_V" => "PASS UNROM(002) detail=0000",
        "M3_P32K_C32K_H" => "PASS CNROM(003) detail=0000",
        "M4_P128K_CR32K" | "M4_P128K_CR8K" => "PASS TNROM/MMC3(004) detail=0000",
        "M4_P256K_C256K" => "PASS TSROM/MMC3(004) detail=0000",
        "M7_P128K_CR8K" => "PASS ANROM/AxROM(007) detail=0000",
        "M9_P128K_C64K" => "PASS PNROM/MMC2(009) detail=0000",
        "M10_P128K_C64K_S8K" | "M10_P128K_C64K_W8K" => "PASS FxROM/MMC4(010) detail=0000",
        "M34_P128K_CR8K_H" => "PASS BNROM(034) detail=0000",
        "M66_P64K_C16K_V" => "PASS MHROM/GxROM(066) detail=0000",
        "M69_P128K_C64K_S8K" | "M69_P128K_C64K_W8K" => "WRAM-RESIDUAL J*ROM/FME-7(069) detail=1000",
        _ => "UNVERIFIED (new ROM — render + classify before blessing)",
    }
}

/// Collect the committed `.nes` ROMs, sorted for a deterministic report order.
/// This is the data-driven pivot: dropping a new Holy Mapperel ROM into the
/// directory auto-enrolls it in the net (a new snapshot line + a forced
/// `UNVERIFIED` classification).
fn committed_roms() -> Vec<PathBuf> {
    let mut roms: Vec<PathBuf> = fs::read_dir(rom_dir())
        .expect("holy_mapperel rom dir")
        // `.expect` (not `.flatten`) so a directory-read error fails the net loudly
        // rather than silently dropping a ROM — a skipped mapper ROM would be a
        // silent regression-net bypass.
        .map(|e| e.expect("read holy_mapperel dir entry").path())
        .filter(|p| p.extension().is_some_and(|x| x.eq_ignore_ascii_case("nes")))
        .collect();
    roms.sort();
    roms
}

/// The mapper bank-reachability + IRQ regression net.
///
/// Runs every committed Holy Mapperel ROM to its settled result screen,
/// asserts the two structural guards (settled + non-blank) with a ROM-named
/// message, then pins one combined `insta` snapshot of every ROM's framebuffer
/// hash. A mapper regression flips exactly the affected ROM's hash line; a new
/// ROM adds a line — both fail CI until consciously re-blessed.
#[test]
fn holy_mapperel_bank_reachability_regression_net() {
    let roms = committed_roms();
    assert!(
        roms.len() >= MIN_COMMITTED_ROMS,
        "expected the committed Holy Mapperel subset (>= {MIN_COMMITTED_ROMS} ROMs), found {} — \
         has the corpus been trimmed?",
        roms.len()
    );

    let mut report = String::new();
    // Any ROM whose stem is not yet classified in `expect_label` renders as the
    // `UNVERIFIED` sentinel. Collecting the offenders here lets the test fail
    // *loudly* below rather than merely printing the label into the pinned
    // snapshot — otherwise a `cargo insta accept` after a new ROM drops in would
    // silently bless an unclassified mapper ROM and let it go green in CI.
    let mut unverified: Vec<String> = Vec::new();
    for path in &roms {
        let stem = path
            .file_stem()
            .and_then(|s| s.to_str())
            .expect("utf-8 rom stem");
        let label = expect_label(stem);
        if label.starts_with("UNVERIFIED") {
            unverified.push(stem.to_owned());
        }
        let bytes = fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("{stem} must parse: {e:?}"));

        for _ in 0..PIN_FRAME {
            nes.run_frame();
        }
        let pinned = fnv1a64(nes.framebuffer());
        let colors = distinct_colors(nes.framebuffer());

        // Structural guard 1: non-blank (a hard Morse-crash blanks the screen).
        assert!(
            colors >= 2,
            "{stem}: result screen is blank ({colors} colour) at frame {PIN_FRAME} — \
             mapper detection or bank test crashed into the Morse-code handler"
        );

        // Structural guard 2: settled (the test finished, not mid-buzz/redraw).
        for _ in 0..SETTLE_FRAMES {
            nes.run_frame();
        }
        let settled = fnv1a64(nes.framebuffer());
        assert_eq!(
            pinned,
            settled,
            "{stem}: framebuffer changed between frame {PIN_FRAME} and {} — result \
             screen never settled (still buzzing, animating, or wedged)",
            PIN_FRAME + SETTLE_FRAMES
        );

        writeln!(
            report,
            "{stem:<20} colors={colors} fnv1a64={pinned:016x}  [{label}]"
        )
        .expect("write report line");
    }

    // Fail loudly on any still-`UNVERIFIED` ROM. The `UNVERIFIED` classification
    // is documented as a CI-blocking gate (see the module preamble +
    // `expect_label`): a newly-dropped ROM must be rendered and given an
    // explicit `expect_label` entry before it can go green. Asserting here — not
    // merely printing the label into the snapshot — is what makes the gate real;
    // it trips even after a snapshot re-bless, so an unclassified mapper ROM
    // cannot slip through CI.
    assert!(
        unverified.is_empty(),
        "unclassified Holy Mapperel ROM(s) present: {unverified:?} — render each \
         and add an explicit `expect_label` classification before blessing; the \
         `UNVERIFIED` sentinel must never pass CI",
    );

    // The combined golden sentinel: one snapshot, one line per ROM. A mapper
    // regression diffs exactly the affected line; a newly-added ROM appends a
    // line. Both fail until re-blessed.
    insta::assert_snapshot!("holy_mapperel_bank_reachability", report);
}
