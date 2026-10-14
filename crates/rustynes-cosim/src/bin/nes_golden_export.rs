//! Export `RustyNES` goldens for an HDL device-under-test to be compared against.
//!
//! # Usage
//!
//! ```text
//! nes_golden_export --rom <path> --out <dir> [--seed N] [--frames N]
//!                   [--boot-trace START..END] [--irq-trace CAP]
//!                   [--checkpoint-interval N]
//!                   [--four-score P1:P2:P3:P4] [--vaus POS:FIRE] [--genie CODE]...
//! ```
//!
//! The last three (v3.2.0) attach STIMULUS devices for the whole run -- a Four
//! Score with four pads, an Arkanoid paddle on `$4017`, Game Genie codes -- so
//! the `RustyNES_MiSTer` input and cheat gates have a golden to compare against.
//! Each is recorded in the manifest (`four_score`, `vaus`, `genie`) only when
//! given; see `DeviceArgs`.
//!
//! Writes, under `<dir>`:
//!
//! | file | format | consumed by |
//! |---|---|---|
//! | `<stem>.boot.bin` | `CpuBootTrace` binary | `cpu_boot_trace_diff` |
//! | `<stem>.irq.csv` | per-cycle IRQ/bus CSV | `scripts/irq_trace_cross_diff.py` |
//! | `<stem>.ckpt.bin` | rolling per-cycle hash checkpoints | `checkpoint_diff` |
//! | `<stem>.obs.bin` | full-capture observable stream, 16-byte records | the testbench's self-diff, and a window re-run |
//! | `<stem>.index_fb.bin` | 256x240 LE `u16` | the testbench's frame comparison |
//! | `<stem>.fetch.bin` | per-dot PPU bus addresses, 12-byte records | rung 3's background-fetch gate |
//! | `<stem>.ram.bin` | 2 KiB CPU work RAM | `accuracy_coin_catalog::decode_results` |
//! | `<stem>.ram_init.bin` | 2 KiB CPU work RAM **before** execution | a co-simulation testbench, so its flat memory starts where the oracle's does |
//! | `<stem>.manifest.txt` | provenance | humans, and the drift guard below |
//!
//! # The manifest is not decoration
//!
//! The determinism contract covers the framebuffer and audio. It says **nothing**
//! about trace-format stability, and `cpu_boot_trace` is at schema version 1 with
//! a history of being reshaped. So a routine `RustyNES` accuracy fix can change a
//! golden and turn the FPGA repository's CI red for a reason unrelated to its RTL.
//!
//! The manifest records the ROM SHA-256, the seed, the frame count and the
//! emulator version that produced the goldens, so a red diff can be attributed to
//! the right side of the boundary in one look rather than by bisecting two repos.

use std::path::{Path, PathBuf};

use rustynes_cosim::Oracle;
use sha2::{Digest, Sha256};

const INDEX_FB_LEN: usize = 256 * 240;
const RAM_LEN: usize = 2048;

/// The `--ppu-state-trace` window, as four named fields rather than a nest of
/// tuples. Named because the tuple form was genuinely unreadable -- three of its
/// four members are pairs and two of those are optional -- and because a struct
/// with `None` defaults keeps the test builders below to one line each.
///
/// `scanlines` is `i16` rather than `u16` because the pre-render line is -1.
#[cfg(feature = "ppu-state-trace")]
#[derive(Clone, Copy)]
struct PpuStateTraceArgs {
    capacity: usize,
    frames: (u32, u32),
    scanlines: Option<(i16, i16)>,
    dots: Option<(u16, u16)>,
}

struct Args {
    rom: PathBuf,
    out: PathBuf,
    seed: u64,
    frames: u32,
    /// Hold START on port 1 across a half-open frame window.
    ///
    /// Spelled `--press-start A:B` on the command line; the window itself is
    /// `A..B`, i.e. START is down for frames `A` through `B - 1`. The two
    /// notations were mixed here -- the field said `A..B` where a reader would
    /// take it for the CLI syntax -- so both are now stated.
    ///
    /// Several accuracy ROMs do not start on their own. `AccuracyCoin` sits on
    /// its title screen indefinitely, and a golden exported without this
    /// captures an idle menu -- which a DUT reproduces perfectly while running
    /// none of the tests.
    press_start: Option<(u64, u64)>,
    boot_trace: Option<(u64, u64)>,
    irq_trace: Option<usize>,
    /// Capacity for the per-dot PPU bus-address capture (rung 3's v2.5.4 gate).
    fetch_trace: Option<usize>,
    apu_trace: Option<usize>,
    /// v2.6.5 — the per-dot PPU state fixture. A **DIAGNOSTIC**, never a gate:
    /// `RustyNES_MiSTer/docs/rung3-ppu.md` fixes that partition, and these
    /// fields are this emulator's decomposition of the chip rather than
    /// pin-observable facts. Carried as (capacity, frames, scanlines, dots) so
    /// the window can be narrowed to the handful of dots a question is about.
    #[cfg(feature = "ppu-state-trace")]
    ppu_state_trace: Option<PpuStateTraceArgs>,
    checkpoint_interval: u64,
    /// v2.5.1 — rung 2's interrupt sweep. Instruction-indexed, not
    /// cycle-indexed: this side cannot assert a pin mid-instruction, so a
    /// cycle-indexed sweep would not be comparable. See `Oracle::run_with_injection`.
    inject_instructions: u64,
    inject_nmi_at: Option<u64>,
    inject_irq_at: Option<u64>,
    inject_hold: u64,
    /// v3.2.0 (item 8) -- the input devices and cheat codes attached for the
    /// whole run. See [`DeviceArgs`].
    devices: DeviceArgs,
    /// v3.2.0 — `--sprite-limit-off`: run with the v3.1.0 "Disable
    /// 8-sprite-per-scanline limit" option ON (`Nes::set_sprite_limit_disabled`).
    ///
    /// The `MiSTer` core gains the same option (its FB-16), and the only way to
    /// check a display option is to export the oracle's picture with it set.
    /// It is render-only (`docs/ppu-2c02.md`), so every CPU-visible artifact of
    /// such a run must be byte-identical to the same run without it; only the
    /// framebuffer differs. Recorded in the manifest when set (as
    /// `sprite_limit_off = true`), so a golden says which picture it is.
    sprite_limit_off: bool,
    /// v3.2.0 — `--fb-every-frame`: also write `<stem>.index_fb_frames.bin`,
    /// the index framebuffer after EACH frame of the run, concatenated
    /// (frames x 256 x 240 LE `u16`). The single `index_fb.bin` is the last
    /// frame only, so a display option that is right on the last frame and
    /// wrong on the others would pass a one-frame gate.
    fb_every_frame: bool,
}

/// v3.2.0 (item 8): the STIMULUS the `RustyNES_MiSTer` input and cheat gates
/// need on the oracle side -- a Four Score with four pads, an Arkanoid (Vaus)
/// paddle on port 2, and Game Genie codes.
///
/// Every one of these is a console INPUT the emulator already models (`Nes::set_four_score`,
/// `Nes::set_paddle`, `Nes::add_genie_code`); the exporter only had no way to
/// ask for them, so a DUT implementing the same device had nothing to be
/// compared against. They change no emulated behaviour of their own: with none
/// given, every artifact and every manifest line is byte-identical to the
/// exporter before this field existed (the manifest gains lines ONLY when one
/// is set, so the 132 committed manifests stay exactly what they are).
///
/// Held CONSTANT for the run, deliberately. A frame-windowed schedule like
/// `--press-start` would need the DUT's testbench to reproduce the frame
/// boundaries in CPU cycles; a constant input needs nothing reproduced, and a
/// stimulus ROM that reads the device every frame exercises every bit of it.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
struct DeviceArgs {
    /// `--four-score P1:P2:P3:P4` -- the adapter on, and the four pads' button
    /// bytes (bit order as `Buttons`: A, B, Select, Start, Up, Down, Left, Right
    /// from bit 0).
    four_score: Option<[u8; 4]>,
    /// `--vaus POS:FIRE` -- an Arkanoid controller on port 2 (`$4017`), its
    /// 8-bit knob position and whether the fire button is held.
    vaus: Option<(u8, bool)>,
    /// `--genie CODE`, repeatable -- 6- or 8-letter Game Genie codes, kept in
    /// the order given (the manifest records them in that order).
    genie: Vec<String>,
}

/// Parse `--four-score P1:P2:P3:P4`: four two-digit hex bytes.
///
/// Exactly two digits each, so `1:2:3:4` is refused rather than read as
/// `01:02:03:04` -- the manifest records the canonical form, and a recipe
/// regenerated from it must reproduce the same argument the export saw.
fn parse_four_score_spec(spec: &str) -> Option<[u8; 4]> {
    let parts: Vec<&str> = spec.split(':').collect();
    if parts.len() != 4 {
        return None;
    }
    let mut out = [0u8; 4];
    for (slot, p) in out.iter_mut().zip(&parts) {
        if p.len() != 2 {
            return None;
        }
        *slot = u8::from_str_radix(p, 16).ok()?;
    }
    Some(out)
}

/// Parse `--vaus POS:FIRE`: a two-digit hex knob position and `0` or `1`.
fn parse_vaus_spec(spec: &str) -> Option<(u8, bool)> {
    let (pos, fire) = spec.split_once(':')?;
    if pos.len() != 2 {
        return None;
    }
    let pos = u8::from_str_radix(pos, 16).ok()?;
    let fire = match fire {
        "0" => false,
        "1" => true,
        _ => return None,
    };
    Some((pos, fire))
}

/// Refuse a device request that would silently produce a golden describing
/// something other than what was asked. `None` when the request is coherent.
///
/// Each rule names a combination that would RUN and export a plausible golden:
///
/// - **Four Score with `--press-start`.** Both write port 1's buttons; the
///   press would overwrite the Four Score's pad 1 for its window and restore
///   zero after it, so the manifest's `four_score` line would be false for part
///   of the run.
/// - **Four Score with `--vaus`.** The `NESdev` "Arkanoid controller" page: "No
///   version is compatible with four player adapters." The oracle would let the
///   paddle take over port 2 regardless, producing a stream no console can.
/// - **Two Game Genie codes at one address.** The oracle keeps one code per
///   address, so the second REPLACES the first; the manifest would list a code
///   that was never active.
/// - **An undecodable code** is refused here rather than at `add_genie_code`,
///   so the message names the flag.
fn device_error(args: &Args) -> Option<String> {
    let d = &args.devices;
    if d.four_score.is_some() && args.press_start.is_some() {
        return Some(
            "--four-score and --press-start both drive port 1's buttons; the press \
             would overwrite pad 1 for part of the run"
                .to_owned(),
        );
    }
    if d.four_score.is_some() && d.vaus.is_some() {
        return Some(
            "--four-score with --vaus: the Arkanoid controller is not compatible with \
             four player adapters (NESdev \"Arkanoid controller\")"
                .to_owned(),
        );
    }
    let mut seen: Vec<u16> = Vec::with_capacity(d.genie.len());
    for code in &d.genie {
        let gc = match rustynes_core::GenieCode::new(code) {
            Ok(gc) => gc,
            Err(e) => return Some(format!("--genie {code:?}: {e}")),
        };
        if seen.contains(&gc.addr()) {
            return Some(format!(
                "--genie {code:?} patches ${:04X}, which an earlier --genie already \
                 patches; the oracle keeps one code per address, so the earlier one \
                 would silently stop applying",
                gc.addr()
            ));
        }
        seen.push(gc.addr());
    }
    None
}

/// Attach the requested devices and codes to a freshly constructed oracle.
///
/// Called before a single cycle runs, so the first strobe of the run already
/// sees them. Infallible once [`device_error`] has passed; the `Result` is the
/// genie decoder's, kept rather than unwrapped so a future path that skips the
/// validation fails with a message instead of a panic.
fn apply_devices(o: &mut Oracle, d: &DeviceArgs) -> Result<(), String> {
    if let Some(pads) = d.four_score {
        o.nes_mut().set_four_score(true);
        for (port, bits) in pads.iter().enumerate() {
            o.set_buttons(port, *bits);
        }
    }
    if let Some((pos, fire)) = d.vaus {
        // Port index 1 is `$4017`, where the NES (7-pin) Arkanoid controller
        // reports -- the Famicom version's `$4016`/`$4017` split is a different
        // device and is not what the MiSTer core models.
        o.nes_mut().set_paddle(1, pos, fire);
    }
    for code in &d.genie {
        o.nes_mut()
            .add_genie_code(code)
            .map_err(|e| format!("--genie {code:?}: {e}"))?;
    }
    Ok(())
}

/// Validate the device request, then attach it, or exit with the reason; and
/// set `--sprite-limit-off`.
///
/// One call in `main`, which sits at the pedantic line limit, for the same
/// reason [`arm_traces`] is one call. Both run before the first cycle. The
/// sprite option is configuration, carried across a power cycle, and
/// render-only, so setting it here changes the picture and nothing else (no
/// work RAM byte, no bus cycle).
fn attach_devices(o: &mut Oracle, args: &Args) {
    if args.sprite_limit_off {
        o.nes_mut().set_sprite_limit_disabled(true);
    }
    if let Some(why) = device_error(args) {
        eprintln!("{why}");
        usage();
    }
    if let Err(e) = apply_devices(o, &args.devices) {
        eprintln!("{e}");
        usage();
    }
}

/// The manifest lines recording the devices, or the empty string when none
/// was attached -- which is what keeps every existing manifest byte-identical.
///
/// Canonical forms (lower-case two-digit hex, upper-case codes) so the
/// recipe generator in `RustyNES_MiSTer` can turn each line straight back into
/// the flag that produced it.
fn device_block(d: &DeviceArgs) -> String {
    // Lines collected and joined rather than `write!`-ed into one `String`, so
    // there is no `fmt::Result` to discard (the reason `sha256_hex` is written
    // the way it is).
    let mut lines: Vec<String> = Vec::new();
    if let Some(p) = d.four_score {
        lines.push(format!(
            "four_score   = {:02x}:{:02x}:{:02x}:{:02x}\n",
            p[0], p[1], p[2], p[3]
        ));
    }
    if let Some((pos, fire)) = d.vaus {
        lines.push(format!("vaus         = {pos:02x}:{}\n", u8::from(fire)));
    }
    if !d.genie.is_empty() {
        let codes: Vec<String> = d.genie.iter().map(|c| c.to_ascii_uppercase()).collect();
        lines.push(format!("genie        = {}\n", codes.join(",")));
    }
    lines.concat()
}

/// The `--ppu-state-trace` capacity, refused outside a usable range.
///
/// `PPU_STATE_TRACE_MAX` is 40 million records — a whole 4500-frame `AccuracyCoin`
/// run is ~134 M dots, so this is deliberately below "capture everything": the
/// window flags exist because capturing everything is not the intended use, and
/// a cap that permits it invites the allocation that killed the exporter.
#[cfg(feature = "ppu-state-trace")]
const PPU_STATE_TRACE_MAX: usize = 40_000_000;

#[cfg(feature = "ppu-state-trace")]
fn check_pst_cap(cap: usize) -> usize {
    if cap == 0 {
        eprintln!(
            "--ppu-state-trace needs a non-zero capacity: 0 would write a header and no records, which reads as a successful capture"
        );
        std::process::exit(2);
    }
    if cap > PPU_STATE_TRACE_MAX {
        eprintln!(
            "--ppu-state-trace capacity {cap} exceeds {PPU_STATE_TRACE_MAX}; narrow the window with --pst-frames / --pst-scanlines / --pst-dots instead"
        );
        std::process::exit(2);
    }
    cap
}

fn usage() -> ! {
    eprintln!(
        "usage: nes_golden_export --rom <path> --out <dir> [--seed N] [--frames N]\n\
         \x20                       [--boot-trace START..END] [--irq-trace CAP]\n\
         \x20                       [--fetch-trace CAP] [--apu-trace CAP] [--checkpoint-interval N]\n\
         \x20                       [--inject-instructions N] [--inject-hold N]\n\
         \x20                       [--inject-nmi-at N] [--inject-irq-at N]\n\
         \x20                       [--press-start A:B] [--sprite-limit-off] [--fb-every-frame]\n\
         \x20                       [--four-score P1:P2:P3:P4] [--vaus POS:FIRE]\n\
         \x20                       [--genie CODE]...\n\
         \x20                       [--ppu-state-trace CAP --pst-frames A:B\n\
         \x20                        [--pst-scanlines A:B] [--pst-dots A:B]]"
    );
    std::process::exit(2)
}

/// Parse a `--press-start A:B` spec. `None` for anything this option should
/// refuse.
///
/// Split from the exiting wrapper below so a test can reach the DECISION. The
/// wrapper calls `usage()`, which calls `std::process::exit`, so a test of the
/// rejecting paths through it would take the test process with it -- and the
/// rejecting paths are the ones worth testing, since the whole point of this
/// option is that a silently-ignored press produces a golden of an idle title
/// screen, which is precisely the artifact it exists to stop being mistaken for
/// a run.
fn parse_press_start_spec(spec: &str) -> Option<(u64, u64)> {
    let (a, b) = spec.split_once(':')?;
    let a: u64 = a.parse().ok()?;
    let b: u64 = b.parse().ok()?;
    // An empty or inverted window is refused rather than clamped: `B == A` holds
    // START for zero frames, which is indistinguishable from not passing the
    // flag at all.
    (b > a).then_some((a, b))
}

/// Parse a `--press-start A:B` spec, or exit.
fn parse_press_start(spec: &str) -> (u64, u64) {
    parse_press_start_spec(spec).unwrap_or_else(|| {
        eprintln!("--press-start A:B needs two frame numbers with B > A (got {spec:?})");
        usage()
    })
}

// A flat match over CLI flags, two lines past the limit since `--press-start`
// was added. Allowed rather than split: every arm assigns one of a dozen `mut`
// locals, so a helper would have to take them all by `&mut` and would be harder
// to read than the table it replaced -- and the two parses with real logic
// (`parse_press_start`, and the trace ranges) are already extracted. Same
// judgement as `Bus::unified_dma_cycle_impl` in the core.
#[allow(clippy::too_many_lines)]
fn parse_args() -> Args {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let (mut rom, mut out) = (None, None);
    let (mut seed, mut frames) = (0u64, 60u32);
    let (mut boot_trace, mut irq_trace) = (None, None);
    let mut fetch_trace: Option<usize> = None;
    let mut apu_trace: Option<usize> = None;
    #[cfg(feature = "ppu-state-trace")]
    let (mut pst_cap, mut pst_frames) = (None::<usize>, None::<(u32, u32)>);
    #[cfg(feature = "ppu-state-trace")]
    let (mut pst_scanlines, mut pst_dots) = (None::<(i16, i16)>, None::<(u16, u16)>);
    let mut checkpoint_interval = rustynes_cosim::checkpoint::DEFAULT_INTERVAL;
    let (mut inject_instructions, mut inject_hold) = (0u64, 1u64);
    let (mut inject_nmi_at, mut inject_irq_at) = (None, None);
    let mut press_start: Option<(u64, u64)> = None;
    let mut devices = DeviceArgs::default();
    let (mut sprite_limit_off, mut fb_every_frame) = (false, false);

    let mut i = 0;
    // Every value-taking arm below advances `i` by TWO, not one: this loop has
    // no trailing increment, so stepping by one leaves `i` on the value, which
    // then falls through to `_ => usage()` and prints the help text as though
    // the FLAG were unknown. Stated once here rather than four times inline.
    while i < argv.len() {
        let need = |i: usize| -> &String { argv.get(i + 1).unwrap_or_else(|| usage()) };
        match argv[i].as_str() {
            "--rom" => {
                rom = Some(PathBuf::from(need(i)));
                i += 2;
            }
            "--out" => {
                out = Some(PathBuf::from(need(i)));
                i += 2;
            }
            "--seed" => {
                seed = need(i).parse().unwrap_or_else(|_| usage());
                i += 2;
            }
            "--press-start" => {
                // `A:B` -- hold START on port 1 from frame A until frame B.
                // See `parse_press_start` for why a bad spec exits.
                press_start = Some(parse_press_start(need(i)));
                // Omitting this `i += 2` (see above: no trailing increment)
                // spun the parser at 100% CPU for thirteen minutes, never
                // reaching the simulation. The tell was the output directory
                // never being created.
                i += 2;
            }
            "--frames" => {
                frames = need(i).parse().unwrap_or_else(|_| usage());
                // Rejected at the boundary. A zero-frame run never reaches a
                // frame boundary, so an armed APU trace is never drained, and
                // `write_apu_trace`'s emptiness invariant then fires with a
                // message blaming "some run path" for what is plain invalid
                // input. A panic is the wrong report for a bad argument.
                if frames == 0 {
                    eprintln!("--frames must be at least 1 (got 0)");
                    usage();
                }
                i += 2;
            }
            "--boot-trace" => {
                let v = need(i);
                let (a, b) = v.split_once("..").unwrap_or_else(|| usage());
                boot_trace = Some((
                    a.parse().unwrap_or_else(|_| usage()),
                    b.parse().unwrap_or_else(|_| usage()),
                ));
                i += 2;
            }
            "--inject-instructions" => {
                inject_instructions = need(i).parse().unwrap_or_else(|_| usage());
                i += 2;
            }
            "--inject-nmi-instr" => {
                inject_nmi_at = Some(need(i).parse().unwrap_or_else(|_| usage()));
                i += 2;
            }
            "--inject-irq-instr" => {
                inject_irq_at = Some(need(i).parse().unwrap_or_else(|_| usage()));
                i += 2;
            }
            "--inject-hold" => {
                inject_hold = need(i).parse().unwrap_or_else(|_| usage());
                i += 2;
            }
            "--irq-trace" => {
                irq_trace = Some(need(i).parse().unwrap_or_else(|_| usage()));
                i += 2;
            }
            "--fetch-trace" => {
                fetch_trace = Some(parse_fetch_cap(need(i)));
                i += 2;
            }
            "--apu-trace" => {
                apu_trace = Some(parse_apu_cap(need(i)));
                i += 2;
            }
            #[cfg(not(feature = "ppu-state-trace"))]
            "--ppu-state-trace" | "--pst-frames" | "--pst-scanlines" | "--pst-dots" => {
                // REFUSED, not ignored. A control build that accepted the flag
                // and produced no `ppu_state.csv` would read as "the window held
                // no dots" -- the same confusion the armed-but-empty warning
                // exists to prevent, one level up.
                eprintln!(
                    "{} needs a build with --features ppu-state-trace \
                     (it is off by default; see Cargo.toml)",
                    argv[i]
                );
                usage()
            }
            #[cfg(feature = "ppu-state-trace")]
            "--ppu-state-trace" => {
                // BOUNDED AT BOTH ENDS. Zero produced a header-only CSV that
                // looks like a successful capture, and an arbitrary `usize`
                // reached `Vec::with_capacity` and aborted the exporter rather
                // than reporting anything. A diagnostic that can only be empty
                // or fatal is worse than one that refuses the argument.
                let cap: usize = need(i).parse().unwrap_or_else(|_| usage());
                pst_cap = Some(check_pst_cap(cap));
                i += 2;
            }
            #[cfg(feature = "ppu-state-trace")]
            "--pst-frames" => {
                pst_frames = Some(parse_pair_u32(need(i)).unwrap_or_else(|| usage()));
                i += 2;
            }
            #[cfg(feature = "ppu-state-trace")]
            "--pst-scanlines" => {
                pst_scanlines = Some(parse_pair_i16(need(i)).unwrap_or_else(|| usage()));
                i += 2;
            }
            #[cfg(feature = "ppu-state-trace")]
            "--pst-dots" => {
                pst_dots = Some(parse_pair_u16(need(i)).unwrap_or_else(|| usage()));
                i += 2;
            }
            // v3.2.0 (item 8). A malformed spec EXITS rather than being
            // ignored: an unparsed device is a golden of a console with nothing
            // plugged in, under a recipe claiming otherwise.
            "--four-score" => {
                devices.four_score = Some(parse_four_score_spec(need(i)).unwrap_or_else(|| {
                    eprintln!("--four-score wants P1:P2:P3:P4, four two-digit hex bytes");
                    usage()
                }));
                i += 2;
            }
            "--vaus" => {
                devices.vaus = Some(parse_vaus_spec(need(i)).unwrap_or_else(|| {
                    eprintln!("--vaus wants POS:FIRE, a two-digit hex position and 0 or 1");
                    usage()
                }));
                i += 2;
            }
            "--genie" => {
                devices.genie.push(need(i).clone());
                i += 2;
            }
            // The two flags that take no value advance `i` by ONE.
            "--sprite-limit-off" => {
                sprite_limit_off = true;
                i += 1;
            }
            "--fb-every-frame" => {
                fb_every_frame = true;
                i += 1;
            }
            "--checkpoint-interval" => {
                checkpoint_interval = need(i).parse().unwrap_or_else(|_| usage());
                if checkpoint_interval == 0 {
                    eprintln!("--checkpoint-interval must be non-zero");
                    usage();
                }
                i += 2;
            }
            _ => usage(),
        }
    }
    Args {
        rom: rom.unwrap_or_else(|| usage()),
        out: out.unwrap_or_else(|| usage()),
        seed,
        frames,
        press_start,
        boot_trace,
        irq_trace,
        fetch_trace,
        apu_trace,
        // REFUSED rather than defaulted: a frameless window would record every
        // frame of the run, which at ~46 kB a frame is gigabytes for the
        // 4500-frame goldens -- and the whole point of this trace is a handful
        // of dots. An option that silently means "everything" is how a
        // diagnostic becomes an out-of-disk.
        #[cfg(feature = "ppu-state-trace")]
        ppu_state_trace: match (pst_cap, pst_frames) {
            (Some(capacity), Some(frames)) => Some(PpuStateTraceArgs {
                capacity,
                frames,
                scanlines: pst_scanlines,
                dots: pst_dots,
            }),
            (None, None) => None,
            _ => {
                eprintln!("--ppu-state-trace and --pst-frames must be given together");
                usage()
            }
        },
        checkpoint_interval,
        inject_instructions,
        inject_nmi_at,
        inject_irq_at,
        inject_hold,
        devices,
        sprite_limit_off,
        fb_every_frame,
    }
}

/// SHA-256 of the ROM, so a golden can be tied to the exact input that made it.
///
/// `sha2` is already a workspace dependency, used by `rustynes-core` (which this
/// crate depends on), `rustynes-frontend` and `rustynes-test-harness` -- so this
/// is reuse, not a new dependency to justify.
fn sha256_hex(data: &[u8]) -> String {
    // Built by hand rather than through `write!`, so there is no `fmt::Result` to
    // discard. Review flagged the discarded result, and it was right that `let _ =`
    // on a fallible call is against the project's rules even where the call cannot
    // fail into a `String`.
    //
    // NOT `format!("{:x}", Sha256::digest(data))`, which review also suggested:
    // sha2 0.11 returns `hybrid_array::Array<u8, _>`, which does not implement
    // `LowerHex`. Checked against a scratch crate rather than assumed --
    // `the trait bound Array<u8, ...>: LowerHex is not satisfied`.
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let digest = Sha256::digest(data);
    let mut out = String::with_capacity(digest.len() * 2);
    for b in digest {
        out.push(char::from(HEX[usize::from(b >> 4)]));
        out.push(char::from(HEX[usize::from(b & 0x0F)]));
    }
    out
}

/// `<base>.<suffix>`, by APPENDING rather than by `Path::with_extension`.
///
/// `with_extension` replaces everything after the last dot, so a ROM named
/// `Super Mario Bros. 3.nes` has a stem of `Super Mario Bros. 3` and
/// `with_extension("ram.bin")` yields `Super Mario Bros.ram.bin` -- the frame
/// number silently eaten. Verified, not assumed: that is the literal output.
/// Dots in NES filenames are common enough that this would have corrupted real
/// golden sets. Found in review.
fn suffixed(base: &Path, suffix: &str) -> PathBuf {
    let mut s = base.as_os_str().to_os_string();
    s.push(".");
    s.push(suffix);
    PathBuf::from(s)
}

fn write(path: &Path, bytes: &[u8]) {
    std::fs::write(path, bytes).unwrap_or_else(|e| panic!("write {}: {e}", path.display()));
    println!("  wrote {} ({} bytes)", path.display(), bytes.len());
}

/// Write the three artifacts derived from the per-cycle trace, and return the
/// counts the manifest records.
///
/// One take, three artifacts. `Bus::take_irq_trace` **moves** the trace out, so
/// asking for the CSV and then the checkpoints would silently yield an
/// unarmed-looking `None` for whichever came second -- and `None` there is
/// indistinguishable from "the trace was never armed".
fn write_irq_artifacts(o: &mut Oracle, base: &Path, interval: u64) -> (usize, usize) {
    let Some(a) = o.take_irq_artifacts(interval) else {
        eprintln!("  WARNING: irq trace was armed but returned nothing");
        return (0, 0);
    };

    // Written BEFORE the checkpoints, and outside the `Err` arm below, on
    // purpose. This is the full-capture stream: it is the only artifact the
    // checkpoint hashes can be independently re-derived from -- the CSV cannot,
    // because it carries neither `pc` nor `put_cycle_post` -- and it is what a
    // re-run of a located window consumes. An overflowed trace still holds real
    // records, and those are worth keeping even when hashing them would claim a
    // coverage they do not have.
    let observable_count = a.observables.len();
    write(
        &suffixed(base, "obs.bin"),
        &rustynes_cosim::checkpoint::observables_to_bytes(&a.observables),
    );

    match a.checkpoints {
        Ok(ck) => {
            // The CSV is written only here, unlike `obs.bin` above: it carries
            // no overflow marker, so a CSV from an overflowed trace would be a
            // truncated file indistinguishable from a complete one.
            write(&suffixed(base, "irq.csv"), a.csv.as_bytes());
            write(
                &suffixed(base, "ckpt.bin"),
                &rustynes_cosim::checkpoint::to_bytes(&ck),
            );
            (ck.len(), observable_count)
        }
        // Refuse rather than emitting a short stream: a hash over a trace that
        // dropped records covers fewer cycles than it claims, and the DUT would
        // be blamed for our truncation. First remove any CSV and checkpoints a
        // previous run left under this stem (v3.0.1): they look complete, and
        // beside this run's `obs.bin` they would read as its output.
        Err(e) => {
            for stale in [suffixed(base, "irq.csv"), suffixed(base, "ckpt.bin")] {
                match std::fs::remove_file(&stale) {
                    Ok(()) => eprintln!("  removed stale {}", stale.display()),
                    Err(err) if err.kind() == std::io::ErrorKind::NotFound => {}
                    Err(err) => eprintln!("  WARNING: could not remove {}: {err}", stale.display()),
                }
            }
            panic!("  ERROR: {e}")
        }
    }
}

/// Refuse an injection request that would silently produce a NON-INJECTED golden.
///
/// Every combination rejected here runs to completion and emits a plausible
/// artifact with no pin ever asserted -- and a sweep comparing two non-injected
/// runs agrees, reporting a pass for a stimulus that was never applied. That is
/// this programme's recurring failure mode, so it is refused at the boundary
/// rather than diagnosed later.
///
/// Returns the reason rather than exiting, so the rules are testable without a
/// process boundary.
fn injection_error(args: &Args) -> Option<String> {
    // The APU trace is drained at FRAME boundaries, in `advance_frames`, and an
    // injection run steps instructions and completes no frames -- so the two
    // together would write a well-formed, EMPTY `.apu.bin` with a dropped count
    // of zero. That is the exact shape this project keeps catching: a golden
    // whose emptiness is indistinguishable from a run that produced nothing.
    // Refused here rather than papered over with a warning, because a warning
    // on stderr is a signal that may never arrive.
    if args.apu_trace.is_some() && args.inject_instructions > 0 {
        return Some(
            "--apu-trace needs a frame run: the channel-level trace is drained at \
             frame boundaries, and an injection run completes no frames, so the \
             golden would be silently empty"
                .to_owned(),
        );
    }
    let pinned = args.inject_nmi_at.is_some() || args.inject_irq_at.is_some();
    if pinned && args.inject_instructions == 0 {
        return Some(
            "--inject-{nmi,irq}-instr requires a non-zero --inject-instructions; without it \
             the run falls back to a frame advance and no pin is ever asserted"
                .to_owned(),
        );
    }
    if args.inject_instructions == 0 {
        return None;
    }
    if !pinned {
        return Some(
            "--inject-instructions was given with neither --inject-nmi-instr nor \
             --inject-irq-instr; the run would assert no pin at all"
                .to_owned(),
        );
    }
    if args.inject_hold == 0 {
        return Some("--inject-hold must be non-zero; a zero hold never asserts a pin".to_owned());
    }
    for (name, at) in [("nmi", args.inject_nmi_at), ("irq", args.inject_irq_at)] {
        if let Some(k) = at
            && k >= args.inject_instructions
        {
            return Some(format!(
                "--inject-{name}-instr {k} is outside the {} instruction(s) this run executes; \
                 the pin would never be asserted",
                args.inject_instructions
            ));
        }
    }
    None
}

fn validate_injection(args: &Args) {
    if let Some(why) = injection_error(args) {
        eprintln!("{why}");
        usage();
    }
}

/// The manifest lines describing WHICH KIND OF RUN produced these goldens.
///
/// An injection run is not a frame run, and the manifest must not describe it as
/// one: `calls` counts executed INSTRUCTIONS there, while the shared field is
/// named `run_frame_calls`, and the pin positions and hold that produced the
/// stimulus were recorded nowhere at all. A DUT could not reproduce or audit the
/// golden from the artifact -- which is the manifest's entire job.
/// How much work this run performs, in the unit that run actually uses.
///
/// An injection run completes no frames, so describing it in frames is not a
/// rounding error -- it is the wrong unit, and it is what made the frame-count
/// warning fire on every correct sweep export.
/// Warn when a run did not do what was asked -- in the unit that run uses.
///
/// Both arms matter, and the second was briefly LOST. Gating the frame warning
/// on the run mode fixed a warning that fired on every correct injection export,
/// and in doing so removed the jam signal from injection runs entirely, because
/// that warning was the only thing checking them. A jammed CPU would then have
/// produced a short golden in silence -- the exact failure the frame warning
/// exists to prevent, moved rather than fixed.
fn warn_if_incomplete(args: &Args, o: &Oracle, calls: u64, frames_actual: u64) {
    if args.inject_instructions > 0 {
        if calls != args.inject_instructions {
            eprintln!(
                "  WARNING: requested {} instructions, executed {calls} (CPU jammed: {})",
                args.inject_instructions,
                o.nes().is_jammed()
            );
        }
    } else if frames_actual != u64::from(args.frames) {
        // Reachable when the CPU jams. Emit the goldens anyway -- a jammed ROM
        // is a legitimate thing to compare a DUT against -- but never let the
        // manifest claim a frame count that was not simulated.
        eprintln!(
            "  WARNING: requested {} frames, simulated {frames_actual} (CPU jammed: {})",
            args.frames,
            o.nes().is_jammed()
        );
    }
}

/// Parse and validate `--fetch-trace`'s capacity.
///
/// Validated HERE, at the boundary, rather than left to clamp silently inside
/// the trace. `FetchTrace` caps its own storage so no argument can make it
/// allocate without bound -- but a clamp the caller never learns about produces
/// a golden covering less than the run, and the whole point of the drop counter
/// is that such a golden must not pass unnoticed. Refusing the argument says so
/// before a single cycle is simulated.
fn parse_fetch_cap(raw: &str) -> usize {
    let cap: usize = raw.parse().unwrap_or_else(|_| usage());
    if cap == 0 || cap > rustynes_core::rustynes_ppu::fetch_trace::MAX_CAPACITY {
        eprintln!(
            "--fetch-trace must be between 1 and {} (got {cap}); a window \
             needing more than that wants to be shorter, not buffered \
             larger -- three frames of a rendering ROM is under ten thousand",
            rustynes_core::rustynes_ppu::fetch_trace::MAX_CAPACITY
        );
        usage();
    }
    cap
}

/// Parse and validate `--apu-trace`'s capacity.
///
/// Extracted from `parse_args` to keep that function under the line limit, and
/// validated at the boundary rather than clamped later: the cap is in RECORDS
/// and one record is one CPU cycle, so a 24-frame run wants ~715,000. A
/// capacity smaller than the run yields a well-formed SHORTER file, which is
/// indistinguishable from a shorter run -- the failure this project keeps
/// catching, and the reason the drop counter exists at all.
fn parse_apu_cap(raw: &str) -> usize {
    let cap: usize = raw.parse().unwrap_or_else(|_| usage());
    if cap == 0 || cap > rustynes_cosim::MAX_APU_TRACE_CAPACITY {
        eprintln!(
            "--apu-trace must be between 1 and {} records (got {cap}); one \
             record is one CPU cycle, so a 24-frame run wants ~715,000. An \
             unbounded value reaches Vec::with_capacity directly and aborts \
             the process in the allocator.",
            rustynes_cosim::MAX_APU_TRACE_CAPACITY
        );
        usage();
    }
    cap
}

/// Parse an inclusive `A:B` range. Returns `None` for anything malformed or
/// inverted, so the caller can refuse rather than silently widen the window.
///
/// Three monomorphic wrappers rather than one generic: the three ranges have
/// three different element types (`u32` frames, `i16` scanlines carrying the
/// `-1` pre-render line, `u16` dots), and a generic over `FromStr + PartialOrd`
/// buys nothing at three call sites.
/// Arm the per-dot PPU state fixture if it was requested.
///
/// A free function with a no-op twin rather than a `cfg` block inside `main`:
/// `main` is already at the pedantic line limit, and a `cfg`-gated block there
/// makes its length depend on which features are enabled -- so the lint would
/// fire in one configuration and not the other.
#[cfg(feature = "ppu-state-trace")]
fn arm_ppu_state_trace(o: &mut Oracle, args: &Args) {
    if let Some(w) = args.ppu_state_trace {
        let (f0, f1) = w.frames;
        o.enable_ppu_state_trace(
            w.capacity,
            f0..=f1,
            w.scanlines.map(|(a, b)| a..=b),
            w.dots.map(|(a, b)| a..=b),
        );
    }
}

/// The control build's twin. Present so `main` reads identically either way;
/// the argument parser still REFUSES the flags rather than ignoring them, so a
/// control build cannot silently accept a request it cannot serve.
#[cfg(not(feature = "ppu-state-trace"))]
const fn arm_ppu_state_trace(_o: &mut Oracle, _args: &Args) {}

/// Arm every trace the arguments asked for.
///
/// One place rather than five blocks in `main`: they are the same kind of step,
/// `main` sits at the pedantic line limit, and keeping them together means
/// adding a sixth trace does not push an unrelated function over it.
fn arm_traces(o: &mut Oracle, args: &Args) {
    if let Some((start, end)) = args.boot_trace {
        // Capacity is the window, not the whole run: a bounded window is the
        // design, because a full AccuracyCoin run would be ~1 GB of records.
        let cap = usize::try_from(end.saturating_sub(start) + 1).unwrap_or(usize::MAX);
        o.enable_cpu_boot_trace(cap, start, end);
    }
    if let Some(cap) = args.irq_trace {
        o.enable_irq_trace(cap);
    }
    if let Some(cap) = args.fetch_trace {
        o.enable_fetch_trace(cap);
    }
    if let Some(cap) = args.apu_trace {
        o.enable_apu_trace(cap);
    }
    arm_ppu_state_trace(o, args);
}

#[cfg(feature = "ppu-state-trace")]
fn parse_pair_u32(raw: &str) -> Option<(u32, u32)> {
    let (a, b) = raw.split_once(':')?;
    let (a, b) = (a.parse::<u32>().ok()?, b.parse::<u32>().ok()?);
    (a <= b).then_some((a, b))
}

#[cfg(feature = "ppu-state-trace")]
fn parse_pair_i16(raw: &str) -> Option<(i16, i16)> {
    let (a, b) = raw.split_once(':')?;
    let (a, b) = (a.parse::<i16>().ok()?, b.parse::<i16>().ok()?);
    (a <= b).then_some((a, b))
}

#[cfg(feature = "ppu-state-trace")]
fn parse_pair_u16(raw: &str) -> Option<(u16, u16)> {
    let (a, b) = raw.split_once(':')?;
    let (a, b) = (a.parse::<u16>().ok()?, b.parse::<u16>().ok()?);
    (a <= b).then_some((a, b))
}

/// Write rung 4's per-CPU-cycle channel-level golden, when armed.
///
/// Fails loudly on a dropped record for the same reason `write_fetch_trace`
/// does: a truncated golden is not a smaller golden. The comparator sees a
/// length mismatch and reports a divergence whose real cause is an export-side
/// capacity, which is a divergence about the wrong thing.
fn write_apu_trace(o: &mut Oracle, base: &Path) {
    if let Some((bytes, dropped)) = o.take_apu_trace() {
        // ARMED BUT EMPTY is a defect, not a short run. `injection_error`
        // already refuses the one combination known to produce it, and this is
        // the backstop for the ones nobody has thought of yet: any future path
        // that arms the trace and never drains it would otherwise ship a
        // 0-byte golden and report success, because `dropped` stays 0.
        //
        // Defence in depth, deliberately -- the guard upstream states the rule
        // and this states the invariant, and the two fail for different
        // reasons. Raised in review after the upstream guard was already in.
        assert!(
            !bytes.is_empty(),
            "the APU trace was armed and produced no records -- a 0-byte golden \
             reports success while covering nothing. Some run path armed the \
             trace without draining it at a frame boundary."
        );
        // Checked BEFORE writing, not after. Writing a golden and then exiting
        // non-zero leaves a truncated `.bin` on disk that looks like every
        // other golden, and the next run of a gate against it compares a window
        // shorter than the manifest claims. The exit code fails a pipeline; the
        // file outlives it.
        if dropped > 0 {
            eprintln!(
                "  ERROR: apu trace dropped {dropped} record(s) -- --apu-trace \
                 capacity is too small, so the golden covers less than the run. \
                 No file written."
            );
            std::process::exit(1);
        }
        write(&suffixed(base, "apu.bin"), &bytes);
    }
}

/// Write the per-dot PPU state fixture if it was armed.
///
/// Written LAST and unconditionally reported: a diagnostic that silently
/// produced nothing would be read as "the window held no dots" rather than "the
/// trace was never armed", which is the exact confusion this project keeps
/// paying for.
///
/// Extracted from `main` for the same reason as `arm_ppu_state_trace`, and with
/// the same no-op twin, so `main`'s length does not depend on the feature set.
#[cfg(feature = "ppu-state-trace")]
fn write_ppu_state_trace(o: &mut Oracle, args: &Args, base: &Path) {
    if args.ppu_state_trace.is_none() {
        return;
    }
    match o.take_ppu_state_trace_csv() {
        Some((csv, dropped)) => {
            let rows = csv.lines().count().saturating_sub(1);
            write(&suffixed(base, "ppu_state.csv"), csv.as_bytes());
            println!("  ppu_state.csv: {rows} dot records (DIAGNOSTIC, not a gate)");
            if dropped != 0 {
                // Loud, and on stderr: the file exists and parses, so nothing
                // downstream can tell it is a narrower window than the flags
                // asked for unless this says so here.
                eprintln!(
                    "  WARNING: the PPU state trace hit its capacity and DROPPED {dropped} record(s). \
                     The window is narrower than --pst-frames/--pst-scanlines/--pst-dots requested; \
                     raise --ppu-state-trace or narrow the window."
                );
            }
        }
        None => eprintln!("  WARNING: PPU state trace was armed but returned nothing"),
    }
}

#[cfg(not(feature = "ppu-state-trace"))]
const fn write_ppu_state_trace(_o: &mut Oracle, _args: &Args, _base: &Path) {}

/// Write the per-dot PPU bus-address golden, when the trace was armed.
///
/// A DROPPED count is a TRUNCATED window, and a comparison over one that does
/// not know it is truncated claims a coverage it does not have. It is reported
/// loudly rather than folded into the manifest where nobody looks.
fn write_fetch_trace(o: &mut Oracle, base: &Path) {
    if let Some((bytes, dropped)) = o.take_fetch_trace() {
        write(&suffixed(base, "fetch.bin"), &bytes);
        if dropped > 0 {
            // FAILS, rather than warning. A truncated golden is not a smaller
            // golden: the DUT captures the whole run, so the comparator sees a
            // length mismatch and reports a divergence whose real cause is an
            // export-side capacity. Worse, this project has already been bitten
            // by a warning that fired on every run and was never seen, because
            // the caller redirected stderr -- so a warning here is a signal that
            // may not arrive. The file is written FIRST and kept: its records
            // are real, and a partial capture is still worth inspecting by hand.
            eprintln!(
                "  ERROR: fetch trace dropped {dropped} read(s) -- --fetch-trace \
                 capacity is too small, so the golden covers less than the run. \
                 The truncated file was written; re-run with a larger capacity."
            );
            std::process::exit(1);
        }
    }
}

fn run_scale(args: &Args) -> String {
    if args.inject_instructions > 0 {
        format!("{} instructions, injected", args.inject_instructions)
    } else {
        format!("{} frames", args.frames)
    }
}

fn run_mode_block(args: &Args, calls: u64, frames_actual: u64) -> String {
    if args.inject_instructions > 0 {
        format!(
            "run_mode     = instruction-injection\n\
             instr_req    = {}\n\
             instr_actual = {calls}\n\
             inject_nmi_at= {}\n\
             inject_irq_at= {}\n\
             inject_hold  = {}\n",
            args.inject_instructions,
            args.inject_nmi_at
                .map_or_else(|| "none".to_owned(), |v| v.to_string()),
            args.inject_irq_at
                .map_or_else(|| "none".to_owned(), |v| v.to_string()),
            args.inject_hold,
        )
    } else {
        // `press_start` is recorded, and its absence is recorded too.
        //
        // The manifest exists so a golden's provenance is recoverable from the
        // golden. A controller press changes what the ROM EXECUTES -- an
        // AccuracyCoin export without one captures an idle title screen and
        // with one captures 88 test results -- so a manifest that omits it
        // describes two completely different runs identically.
        //
        // Found by needing it: the shipped `AccuracyCoin` golden plainly
        // contains a pressed run (80 clean passes), and its own manifest could
        // not say what window produced them.
        format!(
            "run_mode     = frames\n\
             frames_req   = {}\n\
             frames_actual= {frames_actual}\n\
             run_frame_calls = {calls}\n\
             press_start  = {}\n",
            args.frames,
            args.press_start
                .map_or_else(|| "none".to_owned(), |(a, b)| format!("{a}:{b}")),
        )
    }
}

/// Advance `total` frames, optionally holding START across `[a, b)`.
///
/// Split out of `main` so it stays under the line limit, and because the
/// segmentation is a statement about the run rather than about argument
/// handling: the press lands on the same frames the oracle's own
/// `AccuracyCoin` runner uses -- idle, hold START, release, run on.
///
/// `advance_frames` is used for each leg rather than a `run_frame()` loop, for
/// the reason that function documents: the first call after power-on completes
/// no frame, so a leg counted in calls would be a leg short.
fn run_frames_with_optional_press(
    o: &mut Oracle,
    total: u64,
    press_start: Option<(u64, u64)>,
) -> u64 {
    const START: u8 = 1 << 3; // Buttons::START
    let Some((a, b)) = press_start else {
        return o.advance_frames(total);
    };
    // Clamped, so a window past the end of the run shortens rather than
    // underflowing `b - a` or `total - b`.
    let a = a.min(total);
    let b = b.min(total);
    let mut calls = o.advance_frames(a);
    o.set_buttons(0, START);
    calls += o.advance_frames(b - a);
    o.set_buttons(0, 0);
    calls += o.advance_frames(total - b);
    calls
}

/// Advance the run one frame at a time, writing the index framebuffer after
/// each frame to `<base>.index_fb_frames.bin` (v3.2.0, `--fb-every-frame`).
///
/// The same `run_frame()` calls `advance_frames(total)` would make, split at
/// each frame boundary, so the run and every other artifact are unchanged.
/// Refused with `--press-start` rather than combined with it: no recipe needs
/// both, and an unexercised combination is a place a golden goes quietly wrong.
fn run_frames_capturing_each(o: &mut Oracle, args: &Args, base: &Path) -> u64 {
    if args.press_start.is_some() {
        eprintln!("--fb-every-frame is not supported together with --press-start");
        usage();
    }
    let mut calls = 0u64;
    let mut bytes = Vec::with_capacity(INDEX_FB_LEN * 2 * args.frames as usize);
    for _ in 0..args.frames {
        calls += o.advance_frames(1);
        let fb = o.nes().index_framebuffer();
        assert_eq!(
            fb.len(),
            INDEX_FB_LEN,
            "unexpected index framebuffer length"
        );
        for px in fb {
            bytes.extend_from_slice(&px.to_le_bytes());
        }
    }
    write(&suffixed(base, "index_fb_frames.bin"), &bytes);
    calls
}

fn main() {
    let args = parse_args();
    let rom =
        std::fs::read(&args.rom).unwrap_or_else(|e| panic!("read {}: {e}", args.rom.display()));
    let stem = args
        .rom
        .file_stem()
        .map_or_else(|| "rom".to_owned(), |s| s.to_string_lossy().into_owned());
    std::fs::create_dir_all(&args.out).expect("create out dir");

    let mut o = Oracle::new(&rom, args.seed).unwrap_or_else(|e| panic!("parse rom: {e}"));
    attach_devices(&mut o, &args);

    // The power-on work RAM, captured BEFORE a single cycle runs.
    //
    // `Nes::from_rom_with_power_on_seed` fills the 2 KiB from a seeded PRNG, so
    // it is deterministic but NOT zero -- and a co-simulation testbench with
    // flat, zeroed memory therefore disagrees with the oracle on every read of
    // a location the program has not written. Those reads are real: the dummy
    // read of an un-indexed zero-page address is one, and it lands on unwritten
    // RAM constantly.
    //
    // Exported as a golden rather than reproduced in the testbench, because
    // reimplementing the oracle's PRNG in C++ is precisely the parallel
    // second implementation that drifts. The DUT loads these bytes; it does not
    // compute them.
    let ram_init = o.nes().bus().ram_bytes().to_vec();

    arm_traces(&mut o, &args);

    // `advance_frames`, not a `run_frame()` loop: the first call after power-on
    // is swallowed by the frame_complete latch the reset sequence leaves set, so
    // a bare loop emits an (N-1)-frame golden under a manifest claiming N.
    let frame_before = o.nes().frame();
    // The interrupt sweep runs INSTEAD of the frame advance: it steps a bounded
    // number of instructions with a pin asserted for part of the run, which is
    // the stimulus rung 2 compares. A frame advance would run past the window
    // and bury the divergence under thousands of unrelated cycles.
    validate_injection(&args);

    let calls = if args.inject_instructions > 0 {
        o.run_with_injection(
            args.inject_instructions,
            args.inject_nmi_at,
            args.inject_irq_at,
            args.inject_hold,
        )
    } else if args.fb_every_frame {
        run_frames_capturing_each(&mut o, &args, &args.out.join(&stem))
    } else {
        run_frames_with_optional_press(&mut o, u64::from(args.frames), args.press_start)
    };
    let frames_actual = o.nes().frame() - frame_before;
    let cycles = o.nes().cycle();
    // The frame check applies to a FRAME run only. An injection run steps
    // instructions and completes no frames at all, so this fired on every
    // correct sweep export -- "requested 1 frames, simulated 0", with nothing
    // wrong and the CPU not jammed. A warning that cries wolf on every valid run
    // is how a real one comes to be ignored, and this one was invisible to me
    // because the sweep script redirects stderr.
    warn_if_incomplete(&args, &o, calls, frames_actual);

    let base = args.out.join(&stem);
    println!("exporting goldens for {stem} ({}):", run_scale(&args));

    let fb = o.nes().index_framebuffer();
    assert_eq!(
        fb.len(),
        INDEX_FB_LEN,
        "unexpected index framebuffer length"
    );
    let mut fb_bytes = Vec::with_capacity(INDEX_FB_LEN * 2);
    for px in fb {
        fb_bytes.extend_from_slice(&px.to_le_bytes());
    }
    write(&suffixed(&base, "index_fb.bin"), &fb_bytes);

    write_fetch_trace(&mut o, &base);
    write_ppu_state_trace(&mut o, &args, &base);
    write_apu_trace(&mut o, &base);

    // Checked BEFORE the write, and checked on `ram_init` specifically. The
    // assert below covers the post-run `ram`, so this buffer had no length check
    // at all -- a co-simulation testbench mirrors these bytes through $1FFF and
    // a short read would place them wrongly rather than fail, which is a
    // divergence at an unrelated address instead of an error here.
    assert_eq!(
        ram_init.len(),
        RAM_LEN,
        "unexpected power-on work RAM length"
    );
    write(&suffixed(&base, "ram_init.bin"), &ram_init);

    let ram = o.nes().bus().ram_bytes();
    assert_eq!(ram.len(), RAM_LEN, "unexpected work RAM length");
    write(&suffixed(&base, "ram.bin"), ram);

    if args.boot_trace.is_some() {
        match o.take_cpu_boot_trace_binary() {
            Some(b) => write(&suffixed(&base, "boot.bin"), &b),
            None => eprintln!("  WARNING: boot trace was armed but returned nothing"),
        }
    }
    let (checkpoint_count, observable_count) = if args.irq_trace.is_some() {
        write_irq_artifacts(&mut o, &base, args.checkpoint_interval)
    } else {
        (0, 0)
    };

    // The device lines ride in the mode block: they describe the run, and an
    // empty block leaves every pre-v3.2.0 manifest byte-identical.
    let mode_block = run_mode_block(&args, calls, frames_actual) + &device_block(&args.devices);

    let manifest = format!(
        "rom          = {}\n\
         rom_sha256   = {}\n\
         seed         = {}\n\
         {}\
         cpu_cycles   = {}\n\
         emulator     = rustynes {}\n\
         index_fb_len = {}\n\
         ram_len      = {}\n\
         ckpt_interval= {}\n\
         ckpt_count   = {}\n\
         obs_count    = {}\n\
         {}",
        args.rom.display(),
        sha256_hex(&rom),
        args.seed,
        mode_block,
        cycles,
        env!("CARGO_PKG_VERSION"),
        INDEX_FB_LEN,
        RAM_LEN,
        args.checkpoint_interval,
        checkpoint_count,
        observable_count,
        // Written only when set, as the device lines are, so every manifest
        // from a run without the option stays byte-identical to the
        // exporter before it; `RustyNES_MiSTer`'s Makefile looks for `true`.
        if args.sprite_limit_off {
            "sprite_limit_off = true\n"
        } else {
            ""
        },
    );
    write(&suffixed(&base, "manifest.txt"), manifest.as_bytes());
    println!("done; {cycles} CPU cycles simulated");
}

#[cfg(test)]
mod tests {
    #[test]
    fn press_start_spec_accepts_a_well_formed_window() {
        assert_eq!(super::parse_press_start_spec("2:5"), Some((2, 5)));
        assert_eq!(super::parse_press_start_spec("0:1"), Some((0, 1)));
        // No upper bound is imposed here -- `run_frames_with_optional_press`
        // clamps to the run length, so a window past the end shortens rather
        // than being rejected at parse time.
        assert_eq!(
            super::parse_press_start_spec("100:4000000000"),
            Some((100, 4_000_000_000))
        );
    }

    #[test]
    fn press_start_spec_refuses_an_empty_or_inverted_window() {
        // `B == A` is the one most likely to be typed by accident, and it holds
        // START for zero frames -- indistinguishable from omitting the flag.
        assert_eq!(super::parse_press_start_spec("5:5"), None);
        assert_eq!(super::parse_press_start_spec("9:2"), None);
    }

    #[test]
    fn press_start_spec_refuses_malformed_input() {
        for bad in ["", "5", "5:", ":5", "a:5", "5:b", "5:5:5", "-1:5", "5 : 9"] {
            assert_eq!(
                super::parse_press_start_spec(bad),
                None,
                "should have refused {bad:?}"
            );
        }
    }

    /// `--apu-trace` with instruction injection must be REFUSED, not silently
    /// emptied.
    ///
    /// The trace drains at frame boundaries and an injection run completes no
    /// frames, so the combination would write a well-formed empty golden whose
    /// dropped count is zero -- indistinguishable from a run that produced
    /// nothing. This calls the real validator rather than restating its rule.
    #[test]
    fn apu_trace_with_injection_is_refused() {
        // Built explicitly rather than from a `Default`: `Args` has no
        // default and giving it one would invent a "valid" argument set that
        // no invocation produces.
        let base = || Args {
            #[cfg(feature = "ppu-state-trace")]
            ppu_state_trace: None,
            rom: std::path::PathBuf::from("/dev/null"),
            out: std::path::PathBuf::from("/tmp"),
            seed: 0,
            frames: 1,
            press_start: None,
            boot_trace: None,
            irq_trace: None,
            fetch_trace: None,
            apu_trace: None,
            checkpoint_interval: 4096,
            inject_instructions: 0,
            inject_nmi_at: None,
            inject_irq_at: None,
            inject_hold: 1,
            devices: super::DeviceArgs::default(),
            sprite_limit_off: false,
            fb_every_frame: false,
        };

        let mut args = base();
        args.apu_trace = Some(1000);
        args.inject_instructions = 8;
        args.inject_nmi_at = Some(2);
        let err = injection_error(&args).expect("the combination must be refused");
        assert!(
            err.contains("--apu-trace"),
            "message names the option: {err}"
        );

        // Neither half alone is refused for this reason: a frame run with the
        // trace armed is the normal case, and an injection run without it is
        // rung 2's sweep.
        let mut frames_only = base();
        frames_only.apu_trace = Some(1000);
        assert!(injection_error(&frames_only).is_none());

        let mut sweep_only = base();
        sweep_only.inject_instructions = 8;
        sweep_only.inject_nmi_at = Some(2);
        assert!(injection_error(&sweep_only).is_none());
    }

    use super::{Args, injection_error, sha256_hex, suffixed};
    use std::path::Path;

    /// A dot in the ROM name must not eat part of the golden's filename.
    ///
    /// `Path::with_extension` replaces everything after the LAST dot, so
    /// `Super Mario Bros. 3` became `Super Mario Bros.ram.bin` -- the frame number
    /// silently gone. NES filenames contain dots routinely, so this would have
    /// corrupted real golden sets rather than being a theoretical edge. Found in
    /// review; the old behaviour is asserted against explicitly so a well-meaning
    /// "simplify this to `with_extension`" is caught.
    #[test]
    fn a_dotted_rom_name_keeps_its_whole_stem() {
        let base = Path::new("/out/Super Mario Bros. 3");
        assert_eq!(
            suffixed(base, "ram.bin"),
            Path::new("/out/Super Mario Bros. 3.ram.bin")
        );
        assert_ne!(
            suffixed(base, "ram.bin"),
            base.with_extension("ram.bin"),
            "if these agree, `with_extension` stopped truncating and this test is moot"
        );
    }

    #[test]
    fn an_undotted_name_is_unaffected() {
        assert_eq!(
            suffixed(Path::new("/out/nestest"), "boot.bin"),
            Path::new("/out/nestest.boot.bin")
        );
    }

    /// An overflowed trace must not leave a truncated `irq.csv` behind.
    ///
    /// The CSV carries no overflow marker (`IrqTrace::to_csv` has no such
    /// field), so a CSV written before the refusal below is indistinguishable
    /// from a complete one. `obs.bin` is kept on overflow deliberately and says
    /// why; the CSV has no such rationale, so it is written only once the
    /// checkpoints are known to be good.
    #[test]
    fn an_overflowed_trace_writes_no_irq_csv() {
        // A 16 KiB NROM whose reset vector points at `JMP $8000`.
        let mut rom = vec![0u8; 16 + 16384 + 8192];
        rom[..4].copy_from_slice(b"NES\x1a");
        rom[4] = 1;
        rom[5] = 1;
        rom[16..19].copy_from_slice(&[0x4C, 0x00, 0x80]);
        let vec_base = 16 + 16384 - 6;
        rom[vec_base..vec_base + 6].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x80]);

        let mut o = rustynes_cosim::Oracle::new(&rom, 0).expect("rom");
        o.enable_irq_trace(64); // far below one frame of CPU cycles
        o.advance_frames(1);

        let dir = std::env::temp_dir().join(format!(
            "rustynes-cosim-overflow-csv-{}",
            std::process::id()
        ));
        std::fs::create_dir_all(&dir).expect("temp dir");
        let base = dir.join("overflow");
        // A previous, SUCCESSFUL run with the same stem (v3.0.1, Copilot on
        // #591): its complete-looking CSV and checkpoints must not survive a
        // failed rerun, or they read as that run's output.
        std::fs::write(suffixed(&base, "irq.csv"), b"stale csv").expect("stale csv");
        std::fs::write(suffixed(&base, "ckpt.bin"), b"stale ckpt").expect("stale ckpt");
        let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
            super::write_irq_artifacts(&mut o, &base, 4096)
        }));
        let csv_exists = suffixed(&base, "irq.csv").exists();
        let ckpt_exists = suffixed(&base, "ckpt.bin").exists();
        let _ = std::fs::remove_dir_all(&dir);
        assert!(result.is_err(), "an overflowed trace must still refuse");
        assert!(
            !csv_exists,
            "an irq.csv (truncated, or a previous run's) was left on disk"
        );
        assert!(!ckpt_exists, "a previous run's ckpt.bin was left on disk");
    }

    /// Pinned against an independently-known digest, not against our own output.
    #[test]
    fn sha256_matches_the_known_empty_digest() {
        assert_eq!(
            sha256_hex(b""),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855"
        );
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
    }
}

/// v3.2.0 (item 8): the device and cheat stimulus flags.
///
/// The rules are tested through the REAL parsers and validator, and the
/// attach path through a real `Oracle` running a real ROM -- a test that only
/// checked the parsed `DeviceArgs` would pass while `apply_devices` attached
/// nothing, which is the "armed but empty" shape this exporter refuses
/// everywhere else.
#[cfg(test)]
mod device_tests {
    use super::{
        Args, DeviceArgs, apply_devices, device_block, device_error, parse_four_score_spec,
        parse_vaus_spec,
    };

    fn args(devices: DeviceArgs) -> Args {
        Args {
            #[cfg(feature = "ppu-state-trace")]
            ppu_state_trace: None,
            rom: std::path::PathBuf::from("/dev/null"),
            out: std::path::PathBuf::from("/tmp"),
            seed: 0,
            frames: 1,
            press_start: None,
            boot_trace: None,
            irq_trace: None,
            fetch_trace: None,
            apu_trace: None,
            checkpoint_interval: 4096,
            inject_instructions: 0,
            inject_nmi_at: None,
            inject_irq_at: None,
            inject_hold: 1,
            devices,
            sprite_limit_off: false,
            fb_every_frame: false,
        }
    }

    #[test]
    fn four_score_spec_takes_exactly_four_two_digit_hex_bytes() {
        assert_eq!(
            parse_four_score_spec("01:82:44:a8"),
            Some([0x01, 0x82, 0x44, 0xA8])
        );
        assert_eq!(
            parse_four_score_spec("FF:00:0f:F0"),
            Some([0xFF, 0x00, 0x0F, 0xF0])
        );
        for bad in [
            "",
            "01:82:44",
            "01:82:44:a8:00",
            "1:82:44:a8",
            "001:82:44:a8",
            "0g:82:44:a8",
        ] {
            assert_eq!(parse_four_score_spec(bad), None, "accepted {bad:?}");
        }
    }

    #[test]
    fn vaus_spec_takes_a_position_and_a_fire_bit() {
        assert_eq!(parse_vaus_spec("a7:1"), Some((0xA7, true)));
        assert_eq!(parse_vaus_spec("00:0"), Some((0x00, false)));
        for bad in ["", "a7", "a7:2", "a7:", "7:1", "a7:1:0", "zz:1"] {
            assert_eq!(parse_vaus_spec(bad), None, "accepted {bad:?}");
        }
    }

    #[test]
    fn incoherent_device_requests_are_refused() {
        let fs = Some([1, 2, 3, 4]);
        let mut a = args(DeviceArgs {
            four_score: fs,
            ..DeviceArgs::default()
        });
        assert!(device_error(&a).is_none(), "a Four Score alone is coherent");
        a.press_start = Some((1, 2));
        assert!(
            device_error(&a).is_some_and(|e| e.contains("--press-start")),
            "Four Score + press-start"
        );

        let both = args(DeviceArgs {
            four_score: fs,
            vaus: Some((0x80, false)),
            genie: Vec::new(),
        });
        assert!(device_error(&both).is_some_and(|e| e.contains("four player")));

        let bad_code = args(DeviceArgs {
            genie: vec!["WXIOPO".to_owned()],
            ..DeviceArgs::default()
        });
        assert!(device_error(&bad_code).is_some_and(|e| e.contains("WXIOPO")));

        // SXIOPO and SXIOPOZZ decode to the SAME address ($91D9); the second
        // would replace the first inside the oracle.
        let dup = args(DeviceArgs {
            genie: vec!["SXIOPO".to_owned(), "SXIOPOZZ".to_owned()],
            ..DeviceArgs::default()
        });
        assert!(device_error(&dup).is_some_and(|e| e.contains("$91D9")));

        let ok = args(DeviceArgs {
            genie: vec!["SXIOPO".to_owned(), "YYKPOYZZ".to_owned()],
            vaus: Some((0x10, true)),
            ..DeviceArgs::default()
        });
        assert!(device_error(&ok).is_none());
    }

    #[test]
    fn no_device_adds_no_manifest_line_and_each_device_adds_its_own() {
        assert_eq!(device_block(&DeviceArgs::default()), "");
        let d = DeviceArgs {
            four_score: Some([0x01, 0x82, 0x44, 0xA8]),
            vaus: None,
            genie: vec!["sxiopo".to_owned(), "YYKPOYZZ".to_owned()],
        };
        assert_eq!(
            device_block(&d),
            "four_score   = 01:82:44:a8\ngenie        = SXIOPO,YYKPOYZZ\n"
        );
        let v = DeviceArgs {
            vaus: Some((0xA7, true)),
            ..DeviceArgs::default()
        };
        assert_eq!(device_block(&v), "vaus         = a7:1\n");
    }

    /// A 16 KiB NROM that strobes the pads and stores the first `$4016` and
    /// `$4017` reads at `$00`/`$01`, then spins.
    fn strobe_rom() -> Vec<u8> {
        let mut rom = vec![0u8; 16 + 16384 + 8192];
        rom[..4].copy_from_slice(b"NES\x1a");
        rom[4] = 1;
        rom[5] = 1;
        let code = [
            0xA9, 0x01, 0x8D, 0x16, 0x40, // LDA #1 ; STA $4016
            0xA9, 0x00, 0x8D, 0x16, 0x40, // LDA #0 ; STA $4016
            0xAD, 0x16, 0x40, 0x85, 0x00, // LDA $4016 ; STA $00
            0xAD, 0x17, 0x40, 0x85, 0x01, // LDA $4017 ; STA $01
            0x4C, 0x14, 0x80, // JMP $8014 (itself)
        ];
        rom[16..16 + code.len()].copy_from_slice(&code);
        let v = 16 + 16384 - 6;
        rom[v..v + 6].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x80]);
        rom
    }

    fn run(d: &DeviceArgs) -> (u8, u8, rustynes_cosim::Oracle) {
        let mut o = rustynes_cosim::Oracle::new(&strobe_rom(), 0).expect("rom");
        apply_devices(&mut o, d).expect("attach");
        o.advance_frames(2);
        let ram = o.nes().bus().ram_bytes();
        (ram[0], ram[1], o)
    }

    #[test]
    fn the_devices_actually_reach_the_emulated_ports() {
        // Control: nothing attached. Open bus $40 from the operand's high byte,
        // D0 = 0 from two unpressed pads.
        let (p1, p2, _) = run(&DeviceArgs::default());
        assert_eq!((p1, p2), (0x40, 0x40));

        // Four Score: pad 1's A (bit 0) and pad 2's A are the first bits out.
        let (p1, p2, o) = run(&DeviceArgs {
            four_score: Some([0x01, 0x01, 0x00, 0x00]),
            ..DeviceArgs::default()
        });
        assert!(o.nes().four_score());
        assert_eq!((p1, p2), (0x41, 0x41));

        // Vaus on $4017: knob $A7 has its MSb set, which goes out INVERTED on
        // D4 (so 0), and fire held is D3 = 1; D0 is not driven. $4016 is
        // still the standard pad.
        let (p1, p2, _) = run(&DeviceArgs {
            vaus: Some((0xA7, true)),
            ..DeviceArgs::default()
        });
        assert_eq!((p1, p2), (0x40, 0x48));
        let (_, p2, _) = run(&DeviceArgs {
            vaus: Some((0x27, false)),
            ..DeviceArgs::default()
        });
        assert_eq!(p2, 0x50, "MSb clear goes out as D4 = 1");

        // Game Genie: every code is active, none silently dropped.
        let (_, _, o) = run(&DeviceArgs {
            genie: vec!["SXIOPO".to_owned(), "YYKPOYZZ".to_owned()],
            ..DeviceArgs::default()
        });
        assert_eq!(o.nes().genie_codes().count(), 2);
    }
}
