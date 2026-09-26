//! Fuzz target for the save-state (`.rns`) deserializer — untrusted **file
//! input**.
//!
//! A save-state slot is arbitrary on-disk bytes (a user can hand-edit or
//! corrupt one, or load one another program wrote). Three parse entry points
//! must reject malformed input with a typed `SnapshotError`, never panic / OOB /
//! hang:
//!
//! - [`parse_header`] — the fixed header (magic, format version, section table
//!   offset). The lightest structural check.
//! - `Nes::extract_thumbnail` — parses the header then walks the section list
//!   to find the embedded thumbnail, without touching emulator state (a pure
//!   parse over the whole container, incl. every section's length prefix).
//! - `Nes::restore_quiet` — the full restore into a live `Nes`, which decodes
//!   every section (CPU/PPU/APU/mapper/WRAM/…) and their bounded length fields.
//!
//! The base `Nes` is a synthesized minimal cartridge so a *structurally valid*
//! fuzz case can actually reach the per-section decoders (not just bounce off
//! the magic check). Until v2.9.0 it was always NROM; bits 1-3 of the first
//! input byte now pick one of eight mappers (`BASES`), so the `MAP ` decoders
//! are in reach as well.
//!
//! # Why the target runs the machine, and patches a real snapshot (v2.7.0)
//!
//! Until v2.7.0 this target restored and stopped, and fed only raw bytes. It
//! could not find the save-state crashes the core audit located (IMP-01/02),
//! for two independent reasons:
//!
//! 1. **Every one of them is deferred.** An out-of-range PPU `spr_count` or
//!    APU `duty` restores without complaint; the panic is an out-of-bounds
//!    index on the NEXT tick. A target that never ticks never sees it. So a
//!    successful restore is now followed by `STEPS_AFTER_RESTORE`
//!    instructions.
//! 2. **Raw bytes almost never reach a field.** To corrupt one byte of the APU
//!    section the input must first reproduce the 16-byte header, the section
//!    framing and every preceding field; libFuzzer with no seed corpus (the
//!    `corpus/` tree is gitignored) essentially never gets there. So the first
//!    input byte now selects a mode: odd = the old raw-container mode; even =
//!    **patch mode**, where the rest of the input is a list of
//!    `(offset: u24 LE, value: u8)` patches applied to a REAL snapshot of the
//!    base machine, with offsets mapped around the framebuffer (see `base`).
//!    Structure survives and every field is one patch away.
//!
//! A hang (a resampler ratio that loops forever) surfaces as a libFuzzer
//! timeout rather than a crash; run with `-timeout=` to catch it.
//!
//! Run with:
//!     cargo install cargo-fuzz
//!     cargo +nightly fuzz run save_state
//!
//! Per `docs/testing-strategy.md` §Layer 5.

#![no_main]

use libfuzzer_sys::fuzz_target;
use rustynes_core::{Nes, parse_header};

/// The base machines a patch-mode case can start from: `(mapper, 16 KiB PRG
/// banks, 8 KiB CHR-ROM banks)`, CHR count 0 meaning 8 KiB of CHR-RAM.
///
/// v2.9.0 (re-audit NC-02): until then the only base was NROM, whose `MAP `
/// section is almost empty, so the mapper-state decoders -- where the
/// re-audit found a GTROM bank restored unvalidated and then indexed on the
/// next fetch, and a BS-5 DIP that overflowed a shift -- were unreachable
/// from this target. Index 0 is the original NROM, byte-identical, so every
/// pre-v2.9.0 input with mode bits 1-3 clear means what it meant. The rest
/// are a spread of mapper-state shapes: MMC1 (serial shift register), MMC3
/// (scanline IRQ), MMC5 (the largest state), VRC6 (expansion audio + CPU-cycle
/// IRQ), FME-7 (CPU-cycle IRQ + 5B audio), GTROM (the NC-02 board), BS-5 (the
/// NC-06 board). FDS is absent because it needs a BIOS image.
const BASES: [(u16, u8, u8); 8] = [
    (0, 1, 1),
    (1, 8, 2),
    (4, 8, 8),
    (5, 8, 8),
    (24, 8, 8),
    (69, 8, 8),
    (111, 8, 0),
    (286, 8, 8),
];

/// A minimal cartridge image for `mapper` whose program spins in an infinite
/// loop after starting the APU and rendering -- enough to construct a real
/// `Nes` whose `restore` path can be exercised. For NROM (`synth(0, 1, 1)`)
/// this is exactly the image the target used before v2.9.0.
///
/// The program and all three vectors are written into EVERY 16 KiB bank, so
/// it runs whatever each mapper powers up with at `$C000-$FFFF`. A mapper
/// number above 255 gets a NES 2.0 header.
fn synth(mapper: u16, prg16: u8, chr8: u8) -> Vec<u8> {
    let prg_len = usize::from(prg16) * 16 * 1024;
    let chr_len = usize::from(chr8) * 8 * 1024;
    let mut bytes = Vec::with_capacity(16 + prg_len + chr_len);
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(prg16);
    bytes.push(chr8);
    let [lo, hi] = mapper.to_le_bytes();
    bytes.push((lo & 0x0F) << 4);
    if hi == 0 {
        bytes.push(lo & 0xF0);
        bytes.extend_from_slice(&[0u8; 8]);
    } else {
        // NES 2.0: byte 8 low nibble = mapper bits 8-11; byte 11 = 8 KiB
        // CHR-RAM when there is no CHR-ROM.
        bytes.push((lo & 0xF0) | 0x08);
        bytes.extend_from_slice(&[hi & 0x0F, 0, 0, if chr8 == 0 { 7 } else { 0 }, 0, 0, 0, 0]);
    }
    let mut prg = vec![0u8; 16 * 1024];
    // Reset vector -> $C000. The program starts all four tone channels, turns
    // rendering on, then spins in a `JMP` forever. Both halves matter to what
    // the fuzzer can reach:
    //
    // - A SILENT APU hides the channel-state panics: `Pulse::output` returns
    //   early on a zero length counter before it ever indexes the duty table.
    //   So every channel gets a halted length counter and constant volume 15,
    //   and pulse 1 gets the `$4001 = $08` sweep idiom (core ledger T-01).
    // - Rendering must be on, or the sprite-evaluation loops that an
    //   out-of-range `spr_count` overruns never execute. OAM powers up zeroed,
    //   so all 64 sprites sit on scanlines 1-8 and every one of those lines
    //   evaluates a full eight.
    let mut code: Vec<u8> = Vec::new();
    for (reg, val) in [
        (0x4015u16, 0x0Fu8), // enable pulse 1/2, triangle, noise
        (0x4000, 0xBF),      // pulse 1: duty 2, halt, constant volume 15
        (0x4001, 0x08),      // pulse 1: sweep off via negate + shift 0
        (0x4002, 0xFD),
        (0x4003, 0x08),
        (0x4004, 0xBF), // pulse 2
        (0x4006, 0xFD),
        (0x4007, 0x08),
        (0x4008, 0xFF), // triangle: control + linear reload 127
        (0x400A, 0xFD),
        (0x400B, 0x08),
        (0x400C, 0x3F), // noise: halt, constant volume 15
        (0x400E, 0x04),
        (0x400F, 0x08),
        (0x2001, 0x18), // PPUMASK: background + sprites
    ] {
        let [lo, hi] = reg.to_le_bytes();
        code.extend_from_slice(&[0xA9, val, 0x8D, lo, hi]); // LDA #val; STA reg
    }
    let spin = 0xC000u16 + u16::try_from(code.len()).expect("fits");
    let [lo, hi] = spin.to_le_bytes();
    code.extend_from_slice(&[0x4C, lo, hi]); // JMP spin
    prg[..code.len()].copy_from_slice(&code);
    let len = prg.len();
    prg[len - 4] = 0x00; // NMI  low
    prg[len - 3] = 0xC0; // NMI  high
    prg[len - 6] = 0x00; // reset low
    prg[len - 5] = 0xC0; // reset high
    prg[len - 2] = 0x00; // IRQ  low
    prg[len - 1] = 0xC0; // IRQ  high
    for _ in 0..prg16 {
        bytes.extend_from_slice(&prg);
    }
    bytes.extend_from_slice(&vec![0u8; chr_len]);
    bytes
}

/// CPU instructions run after an accepted restore. The loop is a 3-cycle
/// `JMP`, so this is about three scanlines -- enough to put the PPU through
/// sprite evaluation and the APU through its mixer, where the deferred
/// snapshot panics fire, at a small fraction of a frame's cost. A whole
/// `run_frame` per case measured ~15 executions per second.
const STEPS_AFTER_RESTORE: usize = 120;

/// The base snapshot, the image it came from, and where its framebuffer sits
/// inside it.
struct Base {
    /// The cartridge image; each case restores into a fresh `Nes` built from it.
    rom: Vec<u8>,
    /// A real `.rns` snapshot of the base machine, taken on scanline 0 so that
    /// the steps after a restore land on the sprite-heavy lines 1-3.
    blob: Vec<u8>,
    /// Byte range of the PPU framebuffer copy inside `blob`.
    fb: core::ops::Range<usize>,
}

/// Build the base once.
///
/// The PPU section carries the whole 245,760-byte RGBA framebuffer, ~95% of
/// the blob. Patch offsets therefore skip it: an offset taken modulo the
/// FULL length almost never lands behind the framebuffer, and a `u16` one
/// could not reach it at all -- the APU
/// section among it -- which is exactly how the first version of this patch
/// mode left every IMP-02 field unreachable (caught in review on #546).
/// Pixels are output, not state, and corrupting them exercises nothing.
fn base(index: usize) -> &'static Base {
    static BASE: [std::sync::OnceLock<Base>; BASES.len()] =
        [const { std::sync::OnceLock::new() }; BASES.len()];
    BASE[index].get_or_init(|| {
        let (mapper, prg16, chr8) = BASES[index];
        let rom = synth(mapper, prg16, chr8);
        let mut nes = Nes::from_rom(&rom)
            .unwrap_or_else(|e| panic!("synthetic mapper-{mapper} image loads: {e}"));
        for _ in 0..3 {
            nes.run_frame();
        }
        // Run to the pre-render line, then onto the first visible line.
        let line = |n: &Nes| n.bus().ppu().scanline();
        while line(&nes) != -1 && line(&nes) != 261 {
            nes.step_instruction();
        }
        while line(&nes) != 0 {
            nes.step_instruction();
        }
        let blob = nes.snapshot();
        let fb_bytes = nes.framebuffer();
        let start = blob
            .windows(fb_bytes.len())
            .position(|w| w == fb_bytes)
            .expect("the framebuffer is stored verbatim in the snapshot");
        let fb = start..start + fb_bytes.len();
        // The non-framebuffer state is itself larger than 64 KiB (measured:
        // a u16 offset tripped this assert on the first execution), which is
        // why a patch carries a 24-bit offset.
        assert!(
            blob.len() - fb.len() <= 1 << 24,
            "every non-framebuffer byte must stay reachable by a 24-bit offset"
        );
        Base { rom, blob, fb }
    })
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    // Bits 1-3 of the mode byte pick the base machine (`BASES`); both modes
    // restore into that machine, so raw inputs reach its mapper decoder too.
    let base = base(usize::from(mode >> 1) % BASES.len());
    let patched;
    let state: &[u8] = if mode & 1 == 0 {
        // Patch mode: `(offset u24 LE, value)` quads over a real snapshot.
        let mut s = base.blob.clone();
        let state_len = s.len() - base.fb.len();
        for p in rest.chunks_exact(4) {
            // A 24-bit offset into the blob with the framebuffer cut out.
            let raw = u32::from_le_bytes([p[0], p[1], p[2], 0]);
            let mut at = usize::try_from(raw).expect("24 bits fit") % state_len;
            if at >= base.fb.start {
                at += base.fb.len();
            }
            s[at] = p[3];
        }
        patched = s;
        &patched
    } else {
        rest
    };

    // 1. Pure header parse — must never panic on any byte slice.
    let _ = parse_header(state);

    // 2. Whole-container thumbnail walk (header + every section length prefix).
    let _ = Nes::extract_thumbnail(state);

    // 3. Full restore into a live Nes of the base's mapper. A malformed state
    //    must return an error and leave the Nes usable, not panic or corrupt
    //    memory. A state that is ACCEPTED must then run: the snapshot crashes
    //    the core audit found all panic on the first tick after restore, not
    //    during it. Since v2.9.0 a REJECTED one runs too: a failed restore
    //    promises the machine is unchanged, so it must keep running.
    if let Ok(mut nes) = Nes::from_rom(&base.rom) {
        let _ = nes.restore_quiet(state);
        for _ in 0..STEPS_AFTER_RESTORE {
            nes.step_instruction();
        }
    }
});
