//! `AccuracyCoin` battery driver and result-page glyph decoder.
//!
//! Source: <https://github.com/100thCoin/AccuracyCoin> (Chris Siebert, MIT,
//! `Copyright (c) 2025`). The ROM ships under `tests/roms/accuracycoin/`.
//!
//! ## What this module does
//!
//! Drives a fresh [`Nes`] from power-on through the entire `AccuracyCoin`
//! battery and reports the per-status cell count from the on-screen result
//! grid. No memory-mapped status protocol exists (per upstream the ROM
//! reports state visually only) so we decode the framebuffer directly.
//!
//! Per the upstream README:
//!
//! > Pressing **Start** will run all tests on the ROM, and then draw a
//! > table showing the results of every test. Tests "print 'PASS' or 'FAIL'
//! > on screen, and in the event of a failure, this ROM also provides an
//! > error code." Tests with multiple acceptable behaviors display "a light
//! > blue number over it".
//!
//! ## Result grid encoding
//!
//! The result page renders a 10x16 cell grid (16 columns × 10 rows ≈ 160
//! cells) inside a white border at framebuffer coords `cols 42..210,
//! rows 60..147`. Each 8x8 cell is solid-colored:
//!
//! | RGB hex        | Meaning                              |
//! |----------------|--------------------------------------|
//! | `#64A0FF` blue | `PASS`                               |
//! | `#4F1000` red  | `FAIL` (error code rendered inside)  |
//! | `#DC834C` orange | `PARTIAL` (multi-acceptable test, "light blue number over it") |
//! | `#4C4C4C` gray | `not run` / no test assigned to slot |
//! | `#FFFFFF` white | border / labels                     |
//!
//! The five-color palette is exhaustive: the entire framebuffer at the
//! result page renders in these five and nothing else (verified during
//! harness development). That makes nearest-neighbor exact-pixel matching
//! sufficient — no fuzzy color-space tolerance needed.
//!
//! ## Pass-rate metric
//!
//! `(PASS + PARTIAL) / (PASS + FAIL + PARTIAL)` — partial counts as a pass
//! because the test ROM accepts the behavior as correct (it just notes the
//! variation). Gray cells are excluded from the denominator: they represent
//! grid slots with no test assigned (the grid has more cells than tests).
//!
//! ## Drive sequence
//!
//! 1. Boot, advance 300 frames (~5 s NES time) to clear the title splash
//!    and reach the main menu.
//! 2. Press `START` for 6 frames, release for 30 — this triggers
//!    "run all tests on the ROM".
//! 3. Sample the result grid every 600 frames (~10 s NES time). When the
//!    `not-run` count stabilizes for 1800 consecutive frames AND fewer than
//!    half the cells remain gray, the battery is complete.
//! 4. Decode and return the counts.
//!
//! See `docs/STATUS.md` `AccuracyCoin pass rate` for the current measured
//! rate and `docs/testing-strategy.md` Layer 3 for where this fits in the
//! six-layer testing pyramid.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_core::{Buttons, Nes};

/// Number of cells in the result grid (10 rows × 16 cols).
pub const GRID_CELLS: usize = 160;
/// Number of grid rows.
pub const GRID_ROWS: usize = 10;
/// Number of grid columns.
pub const GRID_COLS: usize = 16;
/// Row pixel origin (y-coordinate of the centre of the top row).
const CELL_Y0: usize = 67;
/// Row pixel stride (each cell is 8 px tall, contiguous).
const CELL_Y_STRIDE: usize = 8;
/// Column pixel origin.
const CELL_X0: usize = 49;
/// Column pixel stride.
const CELL_X_STRIDE: usize = 10;

/// Per-cell classification of the result grid.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum CellStatus {
    /// `#64A0FF` light blue: test passed.
    Pass,
    /// `#4F1000` dark red: test failed (the cell contains a hex error code
    /// glyph rendered in the same colour, which we do not currently decode).
    Fail,
    /// `#DC834C` orange: test with multiple acceptable behaviours
    /// (counts as a pass for pass-rate purposes; the ROM does not consider
    /// these failures).
    Partial,
    /// `#4C4C4C` dark gray: grid slot with no test assigned, or test not
    /// yet run.
    NotRun,
    /// Anything else (border white pixels caught by the sampler — should
    /// not occur if the result grid is being read; treat as not run).
    Unknown,
}

impl CellStatus {
    /// Classify a single RGB pixel into a [`CellStatus`].
    #[must_use]
    pub const fn from_rgb(r: u8, g: u8, b: u8) -> Self {
        match (r, g, b) {
            (0x64, 0xA0, 0xFF) => Self::Pass,
            (0x4F, 0x10, 0x00) => Self::Fail,
            (0xDC, 0x83, 0x4C) => Self::Partial,
            (0x4C, 0x4C, 0x4C) => Self::NotRun,
            _ => Self::Unknown,
        }
    }

    /// Treat partial passes as passes for the headline metric.
    #[must_use]
    pub const fn is_pass(self) -> bool {
        matches!(self, Self::Pass | Self::Partial)
    }
}

/// Aggregated result of one battery run.
#[derive(Clone, Copy, Debug, Default)]
pub struct BatteryResult {
    /// Number of cells classified as `Pass` (#64A0FF).
    pub pass: u32,
    /// Number of cells classified as `Fail` (#4F1000).
    pub fail: u32,
    /// Number of cells classified as `Partial` (#DC834C).
    pub partial: u32,
    /// Number of cells classified as `NotRun` (#4C4C4C) — empty grid slots.
    pub not_run: u32,
    /// Sanity-check overflow bucket (border whites caught by sampler).
    pub other: u32,
    /// Number of frames the harness ran for.
    pub frames: u64,
}

impl BatteryResult {
    /// Number of cells with a definite pass/fail/partial verdict — the
    /// denominator of the pass-rate metric.
    #[must_use]
    pub const fn assigned(&self) -> u32 {
        self.pass + self.fail + self.partial
    }

    /// `(pass + partial) / assigned`, in `[0.0, 1.0]`. Returns `0.0` if no
    /// cells were assigned (e.g. the harness never reached the result page).
    #[must_use]
    pub fn pass_rate(&self) -> f64 {
        let denom = self.assigned();
        if denom == 0 {
            return 0.0;
        }
        f64::from(self.pass + self.partial) / f64::from(denom)
    }
}

/// Locate the `AccuracyCoin.nes` ROM file inside the workspace.
///
/// # Panics
///
/// Panics if the workspace layout doesn't match the expected structure
/// (`<workspace>/tests/roms/accuracycoin/AccuracyCoin.nes`).
#[must_use]
pub fn rom_path() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root (CARGO_MANIFEST_DIR/../..)")
        .join("tests")
        .join("roms")
        .join("accuracycoin")
        .join("AccuracyCoin.nes")
}

/// Read the result grid out of a 256×240 RGBA framebuffer.
///
/// Samples one pixel per cell at the cell centre. The five-colour palette
/// is exhaustive (verified empirically) so exact pixel matching suffices.
#[must_use]
pub fn classify_grid(framebuffer: &[u8]) -> [CellStatus; GRID_CELLS] {
    assert_eq!(
        framebuffer.len(),
        256 * 240 * 4,
        "framebuffer must be 256x240 RGBA8"
    );
    let mut out = [CellStatus::NotRun; GRID_CELLS];
    for r in 0..GRID_ROWS {
        let y = CELL_Y0 + r * CELL_Y_STRIDE;
        for c in 0..GRID_COLS {
            let x = CELL_X0 + c * CELL_X_STRIDE;
            let i = (y * 256 + x) * 4;
            out[r * GRID_COLS + c] =
                CellStatus::from_rgb(framebuffer[i], framebuffer[i + 1], framebuffer[i + 2]);
        }
    }
    out
}

/// Roll up a classified grid into bucket counts.
#[must_use]
pub fn tally(cells: &[CellStatus]) -> (u32, u32, u32, u32, u32) {
    let mut pass = 0;
    let mut fail = 0;
    let mut partial = 0;
    let mut not_run = 0;
    let mut other = 0;
    for c in cells {
        match c {
            CellStatus::Pass => pass += 1,
            CellStatus::Fail => fail += 1,
            CellStatus::Partial => partial += 1,
            CellStatus::NotRun => not_run += 1,
            CellStatus::Unknown => other += 1,
        }
    }
    (pass, fail, partial, not_run, other)
}

/// Drive `AccuracyCoin` from power-on through "press Start, run all tests",
/// poll the result grid until stable, and return the counts.
///
/// The frame budget is shared between the menu-wait, the battery itself,
/// and the post-battery stability check. The default (`72_000` frames ≈
/// 20 minutes NES time) is more than 3x what the battery currently takes
/// to complete on `RustyNES` — it exists to bound CI wall time, not to
/// time-out a legitimate run.
///
/// # Panics
///
/// Panics if the ROM doesn't parse or load. Callers driving novel input
/// sequences should construct the [`Nes`] manually instead.
pub fn run_battery() -> BatteryResult {
    run_battery_with_budget(72_000)
}

/// Run the battery with a caller-chosen frame budget.
///
/// See [`run_battery`] for the contract. This entry point exists so the
/// integration test can use a tighter budget when iterating locally.
///
/// # Panics
///
/// Panics if the ROM doesn't parse or load.
#[must_use]
pub fn run_battery_with_budget(max_frames: u64) -> BatteryResult {
    let (fb_result, _ram) = run_battery_capturing_ram(max_frames);
    fb_result
}

/// Run the battery and return BOTH the framebuffer-decoded counts
/// AND a copy of the post-run 2 KiB CPU RAM.
///
/// The RAM is the authoritative source of per-test results: each
/// test stores its result byte at a fixed RAM address (see
/// [`super::accuracy_coin_catalog`] for the address table and
/// encoding). The framebuffer-decoded [`BatteryResult`] is retained
/// for backward-compatibility (and as a cross-check), but new
/// diagnostic tooling should prefer the RAM-direct path because it
/// (a) is independent of the result-grid display layout, (b) decodes
/// per-test names + error codes, and (c) covers all 144 tests rather
/// than the subset visible on the summary screen.
///
/// # Panics
///
/// Panics if the ROM doesn't parse or load.
#[must_use]
pub fn run_battery_capturing_ram(max_frames: u64) -> (BatteryResult, Vec<u8>) {
    // v2.0 Phase 6 (mc-ppu-subpos): allow sweeping the analog `$2001` write
    // delay + the `$2007` fetch-buffer per-phase offsets at runtime (no
    // rebuild). `RUSTYNES_MASK_DELAY` sets the PPUMASK write delay (dots);
    // `RUSTYNES_FETCH_OFF` sets the 8 comma-separated `$2007` fetch offsets.
    if let Ok(v) = std::env::var("RUSTYNES_MASK_DELAY")
        && let Ok(d) = v.trim().parse::<u8>()
    {
        rustynes_core::rustynes_ppu::MASK_WRITE_DELAY
            .store(d, std::sync::atomic::Ordering::Relaxed);
    }
    let bytes = fs::read(rom_path())
        .unwrap_or_else(|e| panic!("read AccuracyCoin.nes: {e} (path={})", rom_path().display()));
    let mut nes = Nes::from_rom(&bytes).expect("parse AccuracyCoin.nes (NROM)");

    // 1) Wait for the title splash + menu to render.
    for _ in 0..300 {
        nes.run_frame();
    }
    // 2) Press Start (run all tests). 6-frame press is plenty — NES strobe
    //    polls happen every frame, and the ROM debounces internally.
    nes.set_buttons(0, Buttons::START);
    for _ in 0..6 {
        nes.run_frame();
    }
    nes.set_buttons(0, Buttons::empty());
    // 3) Run until the not-run cell count is stable. Sample every 600
    //    frames (~10 s NES time); after 1800 stable frames we assume the
    //    battery is complete. Battery typically finishes ~4200 frames in.
    let mut frames = 306u64;
    let mut last_not_run = u32::MAX;
    let mut stable_frames = 0u64;
    let mut last_result = BatteryResult::default();
    while frames < max_frames {
        nes.run_frame();
        frames += 1;
        if !frames.is_multiple_of(600) {
            continue;
        }
        let cells = classify_grid(nes.framebuffer());
        let (pass, fail, partial, not_run, other) = tally(&cells);
        last_result = BatteryResult {
            pass,
            fail,
            partial,
            not_run,
            other,
            frames,
        };
        if not_run == last_not_run {
            stable_frames += 600;
            if stable_frames >= 1800 && (not_run as usize) < GRID_CELLS {
                break;
            }
        } else {
            stable_frames = 0;
            last_not_run = not_run;
        }
    }
    if last_result.frames == 0 {
        // Polling never fired (max_frames < 600). Sample once at end.
        let cells = classify_grid(nes.framebuffer());
        let (pass, fail, partial, not_run, other) = tally(&cells);
        last_result = BatteryResult {
            pass,
            fail,
            partial,
            not_run,
            other,
            frames,
        };
    }
    let ram = nes.bus().ram_bytes().to_vec();
    (last_result, ram)
}

/// Diagnostic-only `Sprite 0 Hit` test-entry probe.
///
/// Returns a `(frame, PPUMASK_COPY, PPUSTATUS)` tuple sampled at the
/// moment the per-test result byte at `$0457` (Sprite 0 Hit behavior)
/// first becomes non-zero. This is read-only -- the production runner
/// is unchanged.
///
/// Used by the Cascade A diagnostic in `tests/accuracycoin.rs` to
/// confirm whether the test ROM's "BG bit assumed already set"
/// assumption is honored on our emulator at the moment the Sprite 0
/// Hit test runs.
///
/// # Panics
///
/// Panics if the ROM doesn't parse or load.
#[must_use]
pub fn capture_sprite_zero_hit_test_entry_state(max_frames: u64) -> Option<SpriteZeroHitProbe> {
    let bytes = fs::read(rom_path())
        .unwrap_or_else(|e| panic!("read AccuracyCoin.nes: {e} (path={})", rom_path().display()));
    let mut nes = Nes::from_rom(&bytes).expect("parse AccuracyCoin.nes (NROM)");

    for _ in 0..300 {
        nes.run_frame();
    }
    nes.set_buttons(0, Buttons::START);
    for _ in 0..6 {
        nes.run_frame();
    }
    nes.set_buttons(0, Buttons::empty());

    let mut frames = 306u64;
    let mut prev_byte: u8 = 0;
    let mut prior_ppumask_copy: u8 = 0;
    let mut prior_ppu_oam_addr: u8 = 0;
    let mut prior_ppu_ctrl: u8 = 0;
    let mut prior_ppu_v: u16 = 0;
    let mut prior_status: u8 = 0;
    while frames < max_frames {
        nes.run_frame();
        frames += 1;
        let ram = nes.bus().ram_bytes();
        let cur_byte = ram[0x0457];
        let ppumask_copy = ram[0x00F1];
        if cur_byte != prev_byte && prev_byte == 0 {
            return Some(SpriteZeroHitProbe {
                frame: frames,
                prior_ppumask_copy,
                prior_ppu_oam_addr,
                prior_ppu_ctrl,
                prior_ppu_v,
                prior_status,
            });
        }
        prior_ppumask_copy = ppumask_copy;
        let snap = nes.ppu_snapshot();
        prior_ppu_oam_addr = snap.oam_addr;
        prior_ppu_ctrl = snap.ctrl;
        prior_ppu_v = snap.v;
        prior_status = snap.status;
        prev_byte = cur_byte;
    }
    None
}

/// Snapshot captured by [`capture_sprite_zero_hit_test_entry_state`].
#[derive(Debug, Clone)]
pub struct SpriteZeroHitProbe {
    /// Frame at which the result byte transitioned from zero.
    pub frame: u64,
    /// `RAM[$00F1]` at the previous frame (PPUMASK shadow).
    pub prior_ppumask_copy: u8,
    /// PPU OAMADDR at the previous frame.
    pub prior_ppu_oam_addr: u8,
    /// PPU PPUCTRL at the previous frame.
    pub prior_ppu_ctrl: u8,
    /// PPU `v` (current VRAM address) at the previous frame.
    pub prior_ppu_v: u16,
    /// PPU PPUSTATUS at the previous frame.
    pub prior_status: u8,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cell_status_classifies_known_colours() {
        assert_eq!(CellStatus::from_rgb(0x64, 0xA0, 0xFF), CellStatus::Pass);
        assert_eq!(CellStatus::from_rgb(0x4F, 0x10, 0x00), CellStatus::Fail);
        assert_eq!(CellStatus::from_rgb(0xDC, 0x83, 0x4C), CellStatus::Partial);
        assert_eq!(CellStatus::from_rgb(0x4C, 0x4C, 0x4C), CellStatus::NotRun);
        assert_eq!(CellStatus::from_rgb(0xFF, 0xFF, 0xFF), CellStatus::Unknown);
        assert_eq!(CellStatus::from_rgb(0x00, 0x00, 0x00), CellStatus::Unknown);
    }

    #[test]
    fn partial_counts_as_pass() {
        assert!(CellStatus::Pass.is_pass());
        assert!(CellStatus::Partial.is_pass());
        assert!(!CellStatus::Fail.is_pass());
        assert!(!CellStatus::NotRun.is_pass());
        assert!(!CellStatus::Unknown.is_pass());
    }

    #[test]
    fn pass_rate_excludes_not_run() {
        let r = BatteryResult {
            pass: 73,
            fail: 26,
            partial: 9,
            not_run: 52,
            other: 0,
            frames: 0,
        };
        assert_eq!(r.assigned(), 108);
        let pct = r.pass_rate();
        // (73 + 9) / 108 = 0.7593
        assert!((pct - 0.7593).abs() < 0.0001, "got {pct}");
    }

    #[test]
    fn pass_rate_handles_empty_result() {
        let r = BatteryResult::default();
        assert!(r.pass_rate().abs() < f64::EPSILON);
    }

    #[test]
    fn tally_sums_all_buckets() {
        let cells = [
            CellStatus::Pass,
            CellStatus::Pass,
            CellStatus::Fail,
            CellStatus::Partial,
            CellStatus::NotRun,
            CellStatus::Unknown,
        ];
        let (p, f, pa, n, o) = tally(&cells);
        assert_eq!((p, f, pa, n, o), (2, 1, 1, 1, 1));
    }

    #[test]
    fn classify_grid_reads_160_cells_from_synthetic_buffer() {
        // Synthetic framebuffer: all gray except one PASS cell at row 3, col 5.
        let mut fb = vec![0u8; 256 * 240 * 4];
        // Fill with gray (#4C4C4C).
        for chunk in fb.chunks_mut(4) {
            chunk[0] = 0x4C;
            chunk[1] = 0x4C;
            chunk[2] = 0x4C;
            chunk[3] = 0xFF;
        }
        // Paint a 4x4 PASS patch at sample position (row 3, col 5).
        let y = CELL_Y0 + 3 * CELL_Y_STRIDE;
        let x = CELL_X0 + 5 * CELL_X_STRIDE;
        for dy in 0..4 {
            for dx in 0..4 {
                let i = ((y + dy) * 256 + (x + dx)) * 4;
                fb[i] = 0x64;
                fb[i + 1] = 0xA0;
                fb[i + 2] = 0xFF;
            }
        }
        let cells = classify_grid(&fb);
        assert_eq!(cells.len(), GRID_CELLS);
        for (i, &c) in cells.iter().enumerate() {
            let r = i / GRID_COLS;
            let col = i % GRID_COLS;
            if r == 3 && col == 5 {
                assert_eq!(c, CellStatus::Pass);
            } else {
                assert_eq!(c, CellStatus::NotRun, "cell ({r}, {col})");
            }
        }
    }
}
