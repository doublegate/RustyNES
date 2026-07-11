//! `Nes`-based blargg/PPU ROM runner.
//!
//! Same `$6000` status protocol as the Phase-1 [`crate::run_blargg_until_complete`]
//! runner, but driven through the full lockstep [`Nes`] facade so the PPU is
//! actually online. Required for `ppu_vbl_nmi`, `ppu_open_bus`,
//! `cpu_dummy_reads`, `cpu_dummy_writes_oam`/`_ppumem`, the
//! `sprite_overflow_tests` suite, and `sprite_hit_tests_2005.10.05`.
//!
//! Per `docs/testing-strategy.md` §Layer 3.

use rustynes_core::Nes;
use rustynes_core::rustynes_mappers::RomError;

/// Outcome of running a Nes-based test ROM.
#[derive(Debug)]
pub struct NesTestResult {
    /// Final status byte at `$6000`.
    pub status: u8,
    /// Result string from `$6004..` (terminated by 0).
    pub message: String,
    /// CPU cycles spent.
    pub cycles: u64,
    /// Number of frames rendered.
    pub frames: u64,
}

fn read_message(nes: &mut Nes) -> String {
    let mut out = String::new();
    let bus = nes.bus_mut();
    let mut i: u16 = 4;
    while i < 0x2000 {
        let b = bus.peek_cpu(0x6000 + i);
        if b == 0 {
            break;
        }
        if b.is_ascii() && (b == b'\n' || !b.is_ascii_control()) {
            out.push(b as char);
        } else {
            out.push('.');
        }
        i += 1;
    }
    out
}

/// Run a blargg-style ROM via the lockstep [`Nes`] until either the `$6000`
/// status byte transitions to a terminal code, the ROM JAMs, or `max_frames`
/// is reached.
///
/// The status protocol: `$6000` reports `0x80` (running), `0x81` (needs
/// reset), or any other byte for a final code (`0x00` is pass).
///
/// # Errors
///
/// Returns the underlying [`RomError`] if the bytes don't parse.
pub fn run_nes_blargg(rom_bytes: &[u8], max_frames: u64) -> Result<NesTestResult, RomError> {
    run_nes_blargg_inner(rom_bytes, max_frames, false)
}

/// Run a blargg-style ROM but **force the PAL region** before booting.
///
/// Many PAL test ROMs (e.g. the `pal_apu_tests` corpus) ship as plain iNES
/// 1.0 files with no NES-2.0 region byte, so [`Nes::from_rom`] would default
/// them to NTSC and they would fail on timing. This helper rewrites the
/// 16-byte header in a throwaway copy of the ROM bytes to mark NES 2.0
/// (header byte 7 bits 2-3 = `10`) and set the region nibble (header byte 12
/// bits 0-1 = `01` = PAL) so the core's existing region-aware construction
/// path selects PAL timing. The original ROM body is untouched.
///
/// This is test-harness-only ROM-header surgery — it does not modify any
/// chip code or the core's parsing logic.
///
/// # Errors
///
/// Returns the underlying [`RomError`] if the bytes don't parse, or if the
/// buffer is too short to hold a 16-byte header.
pub fn run_nes_blargg_pal(rom_bytes: &[u8], max_frames: u64) -> Result<NesTestResult, RomError> {
    run_nes_blargg_inner(rom_bytes, max_frames, true)
}

/// Rewrite a throwaway copy of `rom_bytes` to force **PAL region** selection.
///
/// Many PAL test ROMs (the `pal_apu_tests` corpus, the older blargg PAL
/// builds) ship as plain iNES 1.0 with no NES-2.0 region byte, so
/// [`Nes::from_rom`] would default them to NTSC. This stamps the 16-byte
/// header of a *copy* to mark NES 2.0 (byte 7 bits 2-3 = `10`) and set the
/// region nibble (byte 12 bits 0-1 = `01` = PAL) so the core's existing
/// region-aware construction path selects PAL dividers. The ROM body is
/// untouched, and the original slice is never mutated.
///
/// Returns `None` when the buffer is too short to hold a 16-byte header, in
/// which case callers fall back to the ROM as-is.
fn pal_forced_copy(rom_bytes: &[u8]) -> Option<Vec<u8>> {
    if rom_bytes.len() < 16 {
        return None;
    }
    let mut v = rom_bytes.to_vec();
    // NES 2.0 marker: header byte 7 bits 2-3 = 0b10 (preserve other bits).
    v[7] = (v[7] & 0xF3) | 0x08;
    // Region nibble: header byte 12 bits 0-1 = 0b01 = PAL.
    v[12] = (v[12] & 0xFC) | 0x01;
    Some(v)
}

/// Pass/fail verdict decoded from a blargg **on-screen** (PPU-reported) test
/// ROM.
///
/// The 2005-era blargg APU corpora (`blargg_apu_2005.07.30`, and the
/// PAL-calibrated `pal_apu_tests` rebuild) predate the standardized `$6000`
/// WRAM status protocol used by the newer suites (`apu_test`, `instr_test_v5`,
/// …). They are plain NROM with **no PRG-RAM**, so `$6000` is unmapped and
/// reads back `0` forever — a `$6000`-based runner therefore reports a vacuous
/// "pass" for every one of them regardless of the real outcome. These ROMs
/// instead report their verdict by rendering text to the nametable: the test
/// title on one line and `PASSED` or `FAILED: #<n>` beneath it. This verdict
/// is decoded from that on-screen text.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ScreenVerdict {
    /// The screen shows `PASSED`.
    Passed,
    /// The screen shows `FAILED: #<n>` — the sub-test index that failed.
    Failed(u32),
    /// Neither terminal word appeared within the frame budget (the ROM never
    /// settled — a hang, a blank screen, or a decode miss). Treated as a hard
    /// error by callers, never as a pass.
    Unresolved,
}

/// Outcome of running an on-screen (PPU-reported) blargg test ROM.
#[derive(Debug, Clone)]
pub struct ScreenTestResult {
    /// Decoded terminal verdict.
    pub verdict: ScreenVerdict,
    /// The trimmed, newline-joined nametable text at the settling frame — the
    /// human-readable title + verdict line, surfaced in assertion messages so
    /// a failure names itself.
    pub text: String,
    /// Frames run until the verdict resolved (or the budget was exhausted).
    pub frames: u64,
}

/// Decode the visible blargg text from the two on-screen nametables.
///
/// blargg's text engine writes tiles whose index equals the glyph's ASCII
/// code directly (CHR-RAM is uploaded with an ASCII-positioned font), so the
/// nametable byte at a cell *is* the character. Both physical nametables
/// (`vram[0x000..0x3C0]` and `vram[0x400..0x7C0]`, each a 32x30 grid) are
/// decoded and joined, since the ROM's mirroring may place the text in either
/// — non-printable tiles collapse to spaces and blank lines are dropped.
fn decode_screen_text(nes: &Nes) -> String {
    let vram = nes.vram();
    let mut out = String::new();
    for base in [0x000usize, 0x400usize] {
        // A nametable is 32 cols x 30 rows of tile indices (0x3C0 bytes),
        // followed by its attribute table (skipped).
        for row in 0..30usize {
            let mut line = String::new();
            for col in 0..32usize {
                let idx = base + row * 32 + col;
                let tile = vram.get(idx).copied().unwrap_or(0);
                let ch = if (0x20..0x7f).contains(&tile) {
                    tile as char
                } else {
                    ' '
                };
                line.push(ch);
            }
            let trimmed = line.trim_end();
            if !trimmed.is_empty() {
                out.push_str(trimmed);
                out.push('\n');
            }
        }
    }
    out
}

/// Classify decoded on-screen text into a terminal [`ScreenVerdict`], or
/// `None` while the ROM is still running (no terminal word yet).
fn classify_screen(text: &str) -> Option<ScreenVerdict> {
    // `FAILED` is checked first: a `FAILED: #n` line and a stray `PASSED`
    // never co-occur, but ordering makes the intent explicit.
    if let Some(pos) = text.find("FAILED") {
        // Parse the sub-test index after the first '#' on the FAILED line.
        let code = text[pos..]
            .split('#')
            .nth(1)
            .and_then(|s| {
                let digits: String = s.chars().take_while(char::is_ascii_digit).collect();
                digits.parse::<u32>().ok()
            })
            .unwrap_or(0);
        return Some(ScreenVerdict::Failed(code));
    }
    if text.contains("PASSED") {
        return Some(ScreenVerdict::Passed);
    }
    None
}

/// Run a blargg **on-screen-reporting** test ROM and decode its rendered
/// `PASSED` / `FAILED: #<n>` verdict, optionally forcing PAL region.
///
/// This is the honest oracle for the 2005-era blargg APU corpora (see
/// [`ScreenVerdict`] for why the `$6000` runners cannot be trusted on them).
/// The machine is stepped a frame at a time; as soon as the nametable text
/// resolves to a terminal verdict the run returns early (these ROMs write the
/// verdict line only once, at the very end, so the first appearance is
/// stable). If `max_frames` elapse with neither word on screen the verdict is
/// [`ScreenVerdict::Unresolved`] — never a silent pass.
///
/// When `force_pal` is set the ROM header is stamped for PAL region in a
/// throwaway copy (the same header stamp `run_nes_blargg_pal` uses); the
/// original bytes are untouched.
///
/// # Errors
///
/// Returns the underlying [`RomError`] if the bytes don't parse.
pub fn run_nes_screen(
    rom_bytes: &[u8],
    max_frames: u64,
    force_pal: bool,
) -> Result<ScreenTestResult, RomError> {
    // `owned` holds the PAL-stamped copy (if any) for the borrow's lifetime;
    // when absent (NTSC, or a sub-16-byte buffer) we run the original bytes.
    let owned = force_pal.then(|| pal_forced_copy(rom_bytes)).flatten();
    let bytes: &[u8] = owned.as_deref().unwrap_or(rom_bytes);
    let mut nes = Nes::from_rom(bytes)?;
    let mut frames = 0u64;
    while frames < max_frames {
        nes.run_frame();
        frames += 1;
        let text = decode_screen_text(&nes);
        if let Some(verdict) = classify_screen(&text) {
            return Ok(ScreenTestResult {
                verdict,
                text,
                frames,
            });
        }
    }
    let text = decode_screen_text(&nes);
    Ok(ScreenTestResult {
        verdict: ScreenVerdict::Unresolved,
        text,
        frames,
    })
}

fn run_nes_blargg_inner(
    rom_bytes: &[u8],
    max_frames: u64,
    force_pal: bool,
) -> Result<NesTestResult, RomError> {
    // `owned` holds the PAL-stamped copy (if any) for the borrow's lifetime;
    // when absent (NTSC, or a sub-16-byte buffer) we run the original bytes.
    let owned = force_pal.then(|| pal_forced_copy(rom_bytes)).flatten();
    let bytes: &[u8] = owned.as_deref().unwrap_or(rom_bytes);
    let mut nes = Nes::from_rom(bytes)?;
    let magic = [b'D', b'E', b'B', 0];
    let mut started = false;
    let mut frames = 0u64;
    while frames < max_frames {
        nes.run_frame();
        frames += 1;
        let m = {
            let bus = nes.bus_mut();
            [
                bus.peek_cpu(0x6001),
                bus.peek_cpu(0x6002),
                bus.peek_cpu(0x6003),
            ]
        };
        if !started {
            if m == magic[..3] {
                started = true;
            } else {
                continue;
            }
        }
        let status = nes.bus_mut().peek_cpu(0x6000);
        match status {
            0x80 => {}
            0x81 => {
                for _ in 0..6 {
                    nes.run_frame();
                    frames += 1;
                }
                nes.reset();
            }
            code => {
                let cycles = nes.cycle();
                let message = read_message(&mut nes);
                return Ok(NesTestResult {
                    status: code,
                    message,
                    cycles,
                    frames,
                });
            }
        }
    }
    let cycles = nes.cycle();
    let status = nes.bus_mut().peek_cpu(0x6000);
    let message = read_message(&mut nes);
    Ok(NesTestResult {
        status,
        message,
        cycles,
        frames,
    })
}

/// Run a blargg ROM that uses the **`0x81` "needs reset" protocol** with the
/// canonical magic signature `$DE $B0 $61` at `$6001..$6003`.
///
/// The blargg APU-reset suite (`apu_reset/*.nes`) reports `$6000 = 0x81`
/// ("Press RESET") at a precise cycle, expects the host to soft-reset the
/// machine, and then validates the post-reset register state. Detecting the
/// `0x81` transition and issuing [`Nes::reset`] is essential — without it the
/// ROM sits at `0x81` forever. This runner watches for the canonical magic
/// (the [`run_nes_blargg`] runner predates the magic fix and never trips
/// `started`, so its `0x81` path is dead — it is kept byte-compatible for the
/// existing terminal-`0x00` corpora rather than changed here).
///
/// A bounded number of soft resets are honoured; a runaway reset loop
/// terminates with the last observed status so a regression surfaces loudly.
///
/// # Errors
///
/// Returns the underlying [`RomError`] if the bytes don't parse.
pub fn run_nes_blargg_reset(rom_bytes: &[u8], max_frames: u64) -> Result<NesTestResult, RomError> {
    /// Canonical blargg test-ROM magic signature at `$6001..$6003`.
    const MAGIC: [u8; 3] = [0xDE, 0xB0, 0x61];
    /// Guard against a pathological reset loop (the `apu_reset` ROMs request
    /// at most two resets: one at power, one at reset).
    const MAX_RESETS: u32 = 8;

    let mut nes = Nes::from_rom(rom_bytes)?;
    let mut started = false;
    let mut resets = 0u32;
    let mut frames = 0u64;
    // Stale-status guard (v2.0.0 beta.3): the `$6000` status byte and the
    // `$6001-$6003` magic live in WRAM, which SURVIVES a soft reset — so for
    // the first frames after `nes.reset()` they still read the PRE-reset
    // `$81` until the ROM's post-reset path reaches `std_reset` and rewrites
    // them. Re-detecting that stale `$81` immediately re-resets the ROM
    // mid-measurement every ~7 frames (observed wedging `4017_timing` at
    // MAX_RESETS once the A4 reset sequence lengthened the second-pass
    // measurement window). After a reset, ignore `$81` until the status has
    // read something else at least once — a genuine new prompt always
    // rewrites a fresh `$81` after a `$80` running phase.
    let mut stale_after_reset = false;
    while frames < max_frames {
        nes.run_frame();
        frames += 1;
        let m = {
            let bus = nes.bus_mut();
            [
                bus.peek_cpu(0x6001),
                bus.peek_cpu(0x6002),
                bus.peek_cpu(0x6003),
            ]
        };
        if !started {
            if m == MAGIC {
                started = true;
            } else {
                continue;
            }
        }
        let status = nes.bus_mut().peek_cpu(0x6000);
        if stale_after_reset {
            if status == 0x81 {
                continue;
            }
            stale_after_reset = false;
        }
        match status {
            0x80 => {}
            0x81 => {
                // Hold "RESET pressed" briefly, then issue the soft reset the
                // ROM is waiting on. blargg's protocol expects the reset to
                // land after a short delay (it polls `$6000` post-reset).
                for _ in 0..6 {
                    nes.run_frame();
                    frames += 1;
                }
                nes.reset();
                started = false; // re-detect magic after the reset re-inits it
                stale_after_reset = true;
                resets += 1;
                if resets > MAX_RESETS {
                    break;
                }
            }
            code => {
                let cycles = nes.cycle();
                let message = read_message(&mut nes);
                return Ok(NesTestResult {
                    status: code,
                    message,
                    cycles,
                    frames,
                });
            }
        }
    }
    let cycles = nes.cycle();
    let status = nes.bus_mut().peek_cpu(0x6000);
    let message = read_message(&mut nes);
    Ok(NesTestResult {
        status,
        message,
        cycles,
        frames,
    })
}
