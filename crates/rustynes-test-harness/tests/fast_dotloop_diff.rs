//! v2.1.8 "Performance" A1 — the differential byte-identity gate for the
//! specialized visible-scanline **fast dot path** (`Nes::set_fast_dotloop`).
//!
//! The PPU per-dot FSM (`Ppu::tick`) is the emulator's single hottest function
//! (~46% of a representative frame's self-time; `docs/performance.md`). The A1
//! optimization dispatches the common "clean" visible BG-render dots (a visible
//! scanline, dots `1..=256`, rendering stably enabled, no sub-dot disturbance)
//! to a straight-line handler (`Ppu::tick_visible_render_fast`) that runs the
//! *identical* helper sequence with the statically-dead event/bookkeeping
//! branches pruned. It is a pure internal speedup — the emulated output must not
//! move by a single bit.
//!
//! This suite is the hard contract for that claim. For each ROM in a corpus
//! spanning the accuracy-critical configurations (`nestest` a rendering-enabled
//! menu — where the fast path actually engages, `flowing_palette` a
//! rendering-DISABLED 64-colour backdrop-override demo — the guard-bail /
//! neutral case, `oam_stress` sprite-eval stress, `AccuracyCoin` the PPU-timing
//! gauntlet, and the Holy Mapperel MMC1/MMC3
//! banked boards), it runs the SAME scripted input twice — once with the fast
//! path OFF (the shipped exact path) and once ON — and asserts that EVERY
//! observable stream is bit-for-bit identical:
//!
//! * the RGBA framebuffer, every frame;
//! * the palette-index framebuffer (composite-filter input), every frame;
//! * the emitted audio samples, every frame;
//! * the cumulative CPU-cycle count; and
//! * the full serialized core snapshot (all internal PPU/CPU/APU/mapper state).
//!
//! A per-frame hash vector is compared so a divergence pinpoints the exact
//! frame. Any single-bit difference fails the gate — the fast path would then be
//! wrong for that case and must either widen its disturbance guard (fall back to
//! the exact path) or be dropped. This mirrors the byte-identity discipline the
//! `extra_scanlines` and OAM-decay knobs are held to.
//!
//! ## v2.2.3 P2 — the idle-line fast path
//!
//! P2 added a second specialization, `Ppu::tick_idle_line_fast`, covering
//! **idle lines** (post-render 240 plus vblank 242..=260 — everything that is
//! neither a render line nor the VBL-set line), where the whole per-dot body
//! provably reduces to three rendering-flag assignments. That is another 6,820
//! dots per NTSC frame on top of A1's 61,440.
//!
//! It sits behind the **`ppu-idle-line-fast` cargo feature, default OFF**: it is
//! byte-identical but measured below the >3% adoption bar (`docs/performance.md`
//! §P2). With the feature off the idle-line handler is compiled out entirely and
//! the tests below still pass — they then compare the general path against
//! itself for those dots, which is correct but proves nothing about P2. **To
//! actually exercise it:**
//!
//! ```text
//! cargo test -p rustynes-test-harness \
//!   --features test-roms,ppu-idle-line-fast --test fast_dotloop_diff
//! ```
//!
//! The corpus below already covers those dots incidentally — it hashes whole
//! frames, and every frame contains 20 idle lines. What it does NOT reliably
//! cover is the *interesting* case: PPU register writes landing ON an idle
//! line, which is exactly when the new guard must fall through to the exact
//! path (`$2001` arms `mask_write_delay`, `$2006` arms `copy_v_delay`, `$2007`
//! arms `ppudata_sm_countdown`). Vblank is when real games do most of their PPU
//! I/O, so getting this wrong would be both easy and catastrophic.
//!
//! `idle_line_fast_path_matches_exact_under_vblank_io` therefore drives a
//! purpose-built ROM that hammers `$2000`/`$2001`/`$2006`/`$2007` in a tight
//! loop for the length of vblank, guaranteeing those countdowns are live on
//! idle-line dots rather than hoping a corpus ROM happens to do it.

#![cfg(feature = "test-roms")]

mod common;

use common::{fnv1a64, rom_path};
use std::fs;

use rustynes_core::{Buttons, Nes, PpuRevision};

/// Scripted, deterministic input for frame `f`: Start on a 4-of-7 cycle (drives
/// title screens forward — Mesen2's `PGOHelper` trick) plus a rotating
/// d-pad/A/B mix so scrolling + sprite + collision render paths actually run
/// (which is exactly what the visible-scanline fast path accelerates). Identical
/// across the OFF and ON runs, so it can never itself introduce a difference.
fn buttons_for(f: u32) -> Buttons {
    let mut b = Buttons::empty();
    if f % 7 <= 3 {
        b |= Buttons::START;
    }
    match (f / 30) % 4 {
        0 => b |= Buttons::RIGHT | Buttons::A,
        1 => b |= Buttons::LEFT,
        2 => b |= Buttons::A | Buttons::B,
        _ => b |= Buttons::DOWN,
    }
    b
}

/// FNV-1a 64-bit over a stream of bytes (identical algorithm/constants to
/// [`fnv1a64`], but folding an iterator so callers never materialize a `Vec`).
fn fnv1a64_stream(bytes: impl Iterator<Item = u8>) -> u64 {
    let mut h: u64 = 0xCBF2_9CE4_8422_2325;
    for b in bytes {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01B3);
    }
    h
}

/// Fold one frame's every observable output into a single 64-bit hash: the RGBA
/// framebuffer, the palette-index framebuffer, and the audio drained this frame.
/// The `u16` index buffer and `f32` samples are hashed by folding their
/// little-endian bytes directly (no per-frame `Vec` allocation — this runs on
/// every frame of every corpus ROM, twice per ROM).
fn frame_hash(nes: &Nes, audio: &[f32]) -> u64 {
    let mut h = fnv1a64(nes.framebuffer());
    h ^= fnv1a64_stream(nes.index_framebuffer().iter().flat_map(|v| v.to_le_bytes()))
        .rotate_left(17);
    h ^= fnv1a64_stream(audio.iter().flat_map(|s| s.to_le_bytes())).rotate_left(33);
    h
}

struct Capture {
    /// One combined hash per frame (framebuffer + index buffer + audio).
    per_frame: Vec<u64>,
    /// Cumulative CPU cycles across the whole run.
    cpu_cycles: u64,
    /// Full serialized core state at the end of the run.
    snapshot: Vec<u8>,
}

/// Run `rom` for `frames` frames with the fast dot path `fast` (on/off) and the
/// given PPU die `revision`, feeding the scripted input, and capture every
/// observable stream.
fn capture(rom: &str, frames: u32, fast: bool, revision: PpuRevision) -> Capture {
    let path = rom_path(rom);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {rom}: {e:?}"));
    nes.set_ppu_revision(revision);
    nes.set_fast_dotloop(fast);
    assert_eq!(nes.fast_dotloop(), fast, "fast_dotloop knob did not stick");

    let start = nes.cycle();
    let mut per_frame = Vec::with_capacity(frames as usize);
    for f in 0..frames {
        nes.set_buttons(0, buttons_for(f));
        nes.run_frame();
        let audio = nes.drain_audio();
        per_frame.push(frame_hash(&nes, &audio));
    }
    Capture {
        per_frame,
        cpu_cycles: nes.cycle().wrapping_sub(start),
        snapshot: nes.snapshot(),
    }
}

/// The core differential assertion for one ROM under one PPU revision: OFF
/// (exact path) and ON (fast path) must agree bit-for-bit on every stream,
/// every frame.
fn assert_byte_identical(rom: &str, frames: u32, revision: PpuRevision) {
    let exact = capture(rom, frames, false, revision);
    let fast = capture(rom, frames, true, revision);

    assert_eq!(
        exact.per_frame.len(),
        fast.per_frame.len(),
        "{rom} [{revision:?}]: frame count differs"
    );
    // Pinpoint the FIRST diverging frame for a useful failure message.
    for (i, (a, b)) in exact
        .per_frame
        .iter()
        .zip(fast.per_frame.iter())
        .enumerate()
    {
        assert_eq!(
            a, b,
            "{rom} [{revision:?}]: fast dot path diverged at frame {i} \
             (framebuffer / index buffer / audio hash mismatch) — \
             the fast path is NOT byte-identical for this case"
        );
    }
    assert_eq!(
        exact.cpu_cycles, fast.cpu_cycles,
        "{rom} [{revision:?}]: cumulative CPU-cycle count differs (fast path changed timing)"
    );
    assert_eq!(
        fnv1a64(&exact.snapshot),
        fnv1a64(&fast.snapshot),
        "{rom} [{revision:?}]: final core snapshot differs (fast path changed internal state)"
    );
}

/// Corpus spanning the accuracy-critical configurations. `frames` is sized to
/// get each ROM well past its boot/blank period and into steady-state rendering
/// where the fast path is exercised, while keeping the test brisk.
const CORPUS: &[(&str, u32)] = &[
    // Rendering-ENABLED near-static menu (BG fetch + sprite eval active) — the
    // case where the fast path actually engages.
    ("nestest/nestest.nes", 180),
    // Rendering-DISABLED 64-colour backdrop-override demo: the fast path never
    // engages (the guard bails at `rendering_enabled()`), so this pins the
    // neutral / guard-bail case as byte-identical too.
    ("assorted/flowing_palette.nes", 180),
    // Sprite-evaluation stress (OAM / secondary-OAM / overflow paths).
    ("assorted/oam_stress.nes", 180),
    // The PPU-timing gauntlet: sprite-0 hit, $2007 stress, ALE + Read, etc.
    ("accuracycoin/AccuracyCoin.nes", 240),
    // Banked MMC1 board (mapper 1) — A12/CHR-bank interaction with rendering.
    ("holy_mapperel/M1_P128K_CR8K.nes", 180),
    // Banked MMC3 board (mapper 4) — the dot-260 A12 IRQ path under rendering.
    ("holy_mapperel/M4_P128K_CR8K.nes", 180),
    // A mid-frame raster demo (mapper 1) exercising mid-scanline scroll writes,
    // which MUST force the exact path (disturbance guard).
    ("nes-test-roms/scanline/scanline.nes", 180),
];

#[test]
fn fast_dotloop_is_byte_identical_across_corpus() {
    for &(rom, frames) in CORPUS {
        assert_byte_identical(rom, frames, PpuRevision::Rp2c02H);
    }
}

/// v2.1.7 P5 (#280) added the opt-in `Rp2c02G` die revision, whose only per-dot
/// effect is that an OAMADDR (`$2003`) write during rendering ARMS
/// `oam_corruption_pending`. That armed/pending state is one of the
/// disturbances the fast-path dispatch guard tests (`!oam_corruption_pending`),
/// so the fast path must drop to the exact path the instant a `$2003`-write
/// corruption is armed and let the exact path arm/commit it. This re-runs the
/// OAM-exercising corpus with the corruption-modelling revision enabled to
/// PROVE fast == exact even through #280's corruption paths.
#[test]
fn fast_dotloop_is_byte_identical_under_oamaddr_corruption_revision() {
    // The OAM / sprite-heavy members of the corpus — the ones most likely to
    // drive OAMADDR (`$2003`) writes during rendering and thus actually arm
    // #280's corruption on `Rp2c02G`.
    for &(rom, frames) in &[
        ("assorted/oam_stress.nes", 180u32),
        ("accuracycoin/AccuracyCoin.nes", 240),
        ("nestest/nestest.nes", 180),
        ("nes-test-roms/scanline/scanline.nes", 180),
    ] {
        assert_byte_identical(rom, frames, PpuRevision::Rp2c02G);
    }
}

/// Build an NROM cartridge whose 6502 code does nothing but hammer the PPU
/// registers throughout vertical blank.
///
/// This exists because the P2 idle-line fast path must fall back to the exact
/// path the moment a `$2001` / `$2006` / `$2007` write arms one of the three
/// sub-dot countdowns, and vblank is precisely when real software issues those
/// writes. Relying on a corpus ROM to *happen* to land such a write on an idle
/// line would make the coverage accidental; this makes it structural.
///
/// The program:
///
/// ```text
///   reset:  LDA #$00 / STA $2000 / STA $2001     ; NMI off, rendering off
///   vwait:  LDA $2002 / BPL vwait                ; spin until the VBL flag sets
///           LDX #$64                             ; 100 bursts ~ most of vblank
///   burst:  LDA #$1E / STA $2001                 ; arms mask_write_delay
///           LDA #$20 / STA $2006                 ; address high
///           LDA #$00 / STA $2006                 ; address low -> copy_v_delay
///           LDA $2007                            ; arms ppudata_sm_countdown
///           LDA #$00 / STA $2000 / STA $2001     ; back to a quiet state
///           DEX / BNE burst
///           JMP vwait
/// ```
///
/// Each burst is ~30 CPU cycles (~90 dots), so 100 of them span roughly 26
/// scanlines — the whole NTSC vblank. Reading `$2002` clears the VBL flag, so
/// the outer loop re-arms once per frame. `$2000` is only ever written `$00`,
/// keeping NMI disabled so the run stays a pure vblank-I/O torture loop.
fn vblank_io_torture_rom() -> Vec<u8> {
    // 16 KiB PRG mapped at $C000; index 0 == $C000.
    let mut prg = vec![0u8; 16 * 1024];
    let code: &[u8] = &[
        0xA9, 0x00, // $C000 LDA #$00
        0x8D, 0x00, 0x20, // $C002 STA $2000
        0x8D, 0x01, 0x20, // $C005 STA $2001
        0xAD, 0x02, 0x20, // $C008 vwait: LDA $2002
        0x10, 0xFB, // $C00B BPL vwait  ($C00D - 5 = $C008)
        0xA2, 0x64, // $C00D LDX #$64
        0xA9, 0x1E, // $C00F burst: LDA #$1E
        0x8D, 0x01, 0x20, // $C011 STA $2001
        0xA9, 0x20, // $C014 LDA #$20
        0x8D, 0x06, 0x20, // $C016 STA $2006
        0xA9, 0x00, // $C019 LDA #$00
        0x8D, 0x06, 0x20, // $C01B STA $2006
        0xAD, 0x07, 0x20, // $C01E LDA $2007
        0xA9, 0x00, // $C021 LDA #$00
        0x8D, 0x00, 0x20, // $C023 STA $2000
        0x8D, 0x01, 0x20, // $C026 STA $2001
        0xCA, // $C029 DEX
        0xD0, 0xE3, // $C02A BNE burst ($C02C - 29 = $C00F)
        0x4C, 0x08, 0xC0, // $C02C JMP vwait
        0x40, // $C02F RTI (NMI/IRQ landing pad)
    ];
    prg[..code.len()].copy_from_slice(code);
    // Vectors: NMI/IRQ -> the RTI at $C02F, RESET -> $C000.
    prg[0x3FFA] = 0x2F;
    prg[0x3FFB] = 0xC0;
    prg[0x3FFC] = 0x00;
    prg[0x3FFD] = 0xC0;
    prg[0x3FFE] = 0x2F;
    prg[0x3FFF] = 0xC0;

    let mut rom = Vec::with_capacity(16 + prg.len() + 8192);
    rom.extend_from_slice(b"NES\x1A");
    rom.push(1); // 1 x 16 KiB PRG
    rom.push(1); // 1 x 8 KiB CHR
    rom.extend_from_slice(&[0u8; 10]); // flags 6..15: NROM, horizontal mirroring
    rom.extend_from_slice(&prg);
    rom.extend_from_slice(&[0u8; 8192]); // CHR
    rom
}

/// v2.2.3 P2 — the idle-line fast path must be byte-identical even when PPU
/// register writes land ON idle lines and arm the sub-dot countdowns the
/// dispatch guard tests.
///
/// Drives [`vblank_io_torture_rom`] through both paths and compares every
/// observable stream, exactly as [`assert_byte_identical`] does for the corpus.
/// A regression here means the idle-line guard is too permissive — it took the
/// fast path while `mask_write_delay` / `copy_v_delay` / `ppudata_sm_countdown`
/// had real work pending, and silently dropped it.
#[test]
fn idle_line_fast_path_matches_exact_under_vblank_io() {
    const FRAMES: u32 = 120;
    let bytes = vblank_io_torture_rom();

    let run = |fast: bool| {
        let mut nes = Nes::from_rom(&bytes).expect("synthetic NROM parses");
        nes.set_fast_dotloop(fast);
        assert_eq!(nes.fast_dotloop(), fast, "fast_dotloop knob did not stick");
        let start = nes.cycle();
        let mut hashes = Vec::with_capacity(FRAMES as usize);
        for _ in 0..FRAMES {
            nes.run_frame();
            let audio = nes.drain_audio();
            hashes.push(frame_hash(&nes, &audio));
        }
        (hashes, nes.cycle().wrapping_sub(start), nes.snapshot())
    };

    let (exact_h, exact_cycles, exact_snap) = run(false);
    let (fast_h, fast_cycles, fast_snap) = run(true);

    for (i, (a, b)) in exact_h.iter().zip(fast_h.iter()).enumerate() {
        assert_eq!(
            a, b,
            "vblank-I/O torture: idle-line fast path diverged at frame {i} — \
             a PPU register write landing on an idle line was not handled \
             identically by the two paths"
        );
    }
    assert_eq!(
        exact_cycles, fast_cycles,
        "vblank-I/O torture: cumulative CPU-cycle count differs"
    );
    assert_eq!(
        fnv1a64(&exact_snap),
        fnv1a64(&fast_snap),
        "vblank-I/O torture: final core snapshot differs"
    );
}

/// Sanity: setting the knob OFF must be byte-identical to never touching it at
/// all (the stock path the whole oracle uses). Guards against the field's mere
/// presence perturbing anything.
#[test]
fn fast_dotloop_off_equals_untouched() {
    let rom = "assorted/flowing_palette.nes";
    let path = rom_path(rom);
    let bytes = fs::read(&path).unwrap();

    let untouched = {
        let mut nes = Nes::from_rom(&bytes).unwrap();
        let mut hashes = Vec::new();
        for f in 0..120 {
            nes.set_buttons(0, buttons_for(f));
            nes.run_frame();
            let audio = nes.drain_audio();
            hashes.push(frame_hash(&nes, &audio));
        }
        (hashes, nes.snapshot())
    };
    let off = capture(rom, 120, false, PpuRevision::Rp2c02H);

    assert_eq!(
        untouched.0, off.per_frame,
        "{rom}: OFF != untouched (per-frame)"
    );
    assert_eq!(
        fnv1a64(&untouched.1),
        fnv1a64(&off.snapshot),
        "{rom}: OFF != untouched (snapshot)"
    );
}
