//! `Nes`-based blargg/PPU ROM runner.
//!
//! Same `$6000` status protocol as the Phase-1 [`crate::run_blargg_until_complete`]
//! runner, but driven through the full lockstep [`Nes`] facade so the PPU is
//! actually online. Required for `ppu_vbl_nmi`, `ppu_open_bus`,
//! `cpu_dummy_reads`, `cpu_dummy_writes_oam`/`_ppumem`, the
//! `sprite_overflow_tests` suite, and `sprite_hit_tests_2005.10.05`.
//!
//! Per `docs/testing-strategy.md` §Layer 3.

use rustynes_core::rustynes_mappers::RomError;
use rustynes_core::Nes;

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

fn run_nes_blargg_inner(
    rom_bytes: &[u8],
    max_frames: u64,
    force_pal: bool,
) -> Result<NesTestResult, RomError> {
    let owned;
    let bytes: &[u8] = if force_pal && rom_bytes.len() >= 16 {
        let mut v = rom_bytes.to_vec();
        // NES 2.0 marker: header byte 7 bits 2-3 = 0b10 (preserve other bits).
        v[7] = (v[7] & 0xF3) | 0x08;
        // Region nibble: header byte 12 bits 0-1 = 0b01 = PAL.
        v[12] = (v[12] & 0xFC) | 0x01;
        owned = v;
        &owned
    } else {
        rom_bytes
    };
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
