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
//! The base `Nes` is a synthesized minimal NROM so a *structurally valid* fuzz
//! case can actually reach the per-section decoders (not just bounce off the
//! magic check).
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

/// A minimal iNES NROM (16 KiB PRG that spins in an infinite loop + 8 KiB CHR),
/// enough to construct a real `Nes` whose `restore` path can be exercised.
fn synth_nrom() -> Vec<u8> {
    let mut bytes = Vec::with_capacity(16 + 16 * 1024 + 8 * 1024);
    bytes.extend_from_slice(b"NES\x1A");
    bytes.push(1); // 1 x 16 KiB PRG
    bytes.push(1); // 1 x 8 KiB CHR
    bytes.push(0);
    bytes.push(0);
    bytes.extend_from_slice(&[0u8; 8]);
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
    bytes.extend_from_slice(&prg);
    bytes.extend_from_slice(&vec![0u8; 8 * 1024]);
    bytes
}

/// CPU instructions run after an accepted restore. The loop is a 3-cycle
/// `JMP`, so this is about three scanlines -- enough to put the PPU through
/// sprite evaluation and the APU through its mixer, where the deferred
/// snapshot panics fire, at a small fraction of a frame's cost. A whole
/// `run_frame` per case measured ~15 executions per second.
const STEPS_AFTER_RESTORE: usize = 120;

/// The base snapshot, and where its framebuffer sits inside it.
struct Base {
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
fn base() -> &'static Base {
    static BASE: std::sync::OnceLock<Base> = std::sync::OnceLock::new();
    BASE.get_or_init(|| {
        let mut nes = Nes::from_rom(&synth_nrom()).expect("synthetic NROM loads");
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
        Base { blob, fb }
    })
}

fuzz_target!(|data: &[u8]| {
    let Some((&mode, rest)) = data.split_first() else {
        return;
    };
    let patched;
    let state: &[u8] = if mode & 1 == 0 {
        // Patch mode: `(offset u24 LE, value)` quads over a real snapshot.
        let base = base();
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

    // 3. Full restore into a live NROM Nes. A malformed state must return an
    //    error and leave the Nes usable, not panic or corrupt memory. A state
    //    that is ACCEPTED must then run: the snapshot crashes the core audit
    //    found all panic on the first tick after restore, not during it.
    if let Ok(mut nes) = Nes::from_rom(&synth_nrom()) {
        if nes.restore_quiet(state).is_ok() {
            for _ in 0..STEPS_AFTER_RESTORE {
                nes.step_instruction();
            }
        }
    }
});
