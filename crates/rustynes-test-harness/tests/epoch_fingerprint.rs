//! `T-EPOCH-FINGERPRINT` (v3.1.0, CI-02): the emulation epoch is enforced by
//! a test, not by memory.
//!
//! [`rustynes_core::EMULATION_EPOCH`] tells a movie or a netplay peer which
//! emulator *behaviour* it was made under (ADR 0045). Its rule is "raise it in
//! the same change as anything that alters a frame, a sample or a bus cycle
//! against the last release", and until v3.1.0 that rule was enforced by hand
//! only: a change that moved output and forgot the epoch shipped movies that
//! replay wrongly and netplay sessions that desync, with nothing saying why.
//!
//! This test runs a fixed panel of committed test ROMs and fingerprints what
//! each one produces: every frame's framebuffer, every audio sample, the end
//! RAM and the total CPU cycle count. The fingerprints are committed in
//! `golden/epoch_fingerprint.tsv` together with the epoch they belong to.
//!
//! The table also records `last_release_epoch`, the epoch the last release
//! shipped (set by hand at each release cut, like the release anchors):
//!
//! * Fingerprints match the table: pass (once the table names the current
//!   epoch).
//! * A fingerprint moved while `EMULATION_EPOCH` still equals the last
//!   release's: **fail**. Raise the epoch (adding its row to the table in
//!   `hardware_options.rs`), then re-bless.
//! * A fingerprint moved after the epoch was already raised this release: fail
//!   until re-blessed. A second behaviour change in one release needs a
//!   re-bless, not a second rise, because the rule is relative to the last
//!   release (ADR 0045).
//!
//! Re-bless with `RUSTYNES_BLESS_EPOCH_FINGERPRINT=1`. **Blessing refuses to
//! write moved fingerprints while the epoch equals the last release's** --
//! that refusal is the gate; a bless that accepted them would make the rule
//! optional again.
//!
//! The panel is chosen for reach, one subsystem per ROM, so a behaviour change
//! anywhere in the core is likely to move at least one fingerprint. It is not
//! a proof that every change is caught: a change that alters no output on
//! these seven ROMs passes, which is also when the epoch rule does not demand
//! a rise for them. The commercial snapshot suites (`external_*`) remain the
//! wider net, and the release checklist still names the rule.

#![cfg(feature = "test-roms")]

mod common;

use std::fmt::Write as _;
use std::fs;
use std::path::PathBuf;

use common::{fnv1a64, rom_path};
use rustynes_core::{Buttons, EMULATION_EPOCH, Nes};

/// One panel entry: a ROM, how long to run it, and whether to press START
/// after the boot frames (`AccuracyCoin`'s "run every test" menu entry).
struct Probe {
    rom: &'static str,
    frames: u32,
    press_start: bool,
    /// What it is there to catch, for the failure message.
    reach: &'static str,
}

const PANEL: &[Probe] = &[
    Probe {
        rom: "accuracycoin/AccuracyCoin.nes",
        frames: 3200,
        press_start: true,
        reach: "the whole AccuracyCoin battery: CPU, PPU, APU, DMA, bus",
    },
    Probe {
        rom: "blargg/apu_mixer/triangle.nes",
        frames: 600,
        press_start: false,
        reach: "a continuous tone through the mixer (audio)",
    },
    Probe {
        rom: "blargg/sprite_overflow_tests/3.Timing.nes",
        frames: 300,
        press_start: false,
        reach: "sprite evaluation and overflow timing",
    },
    Probe {
        rom: "nes-test-roms/dmc_tests/latency.nes",
        frames: 240,
        press_start: false,
        reach: "DMC fetch latency (audio)",
    },
    Probe {
        rom: "blargg/mmc3_test_2/4-scanline_timing.nes",
        frames: 120,
        press_start: false,
        reach: "MMC3 IRQ timing",
    },
    Probe {
        rom: "nes-test-roms/sprdma_and_dmc_dma/sprdma_and_dmc_dma.nes",
        frames: 120,
        press_start: false,
        reach: "OAM and DMC DMA overlap",
    },
    Probe {
        rom: "assorted/flowing_palette.nes",
        frames: 120,
        press_start: false,
        reach: "palette and emphasis output",
    },
];

// Every ROM above is COMMITTED. The panel first named four from the
// gitignored local aggregate `tests/roms/nes-test-roms/` (ny2011, spritecans,
// and the aggregate's copies of `4-scanline_timing` and `flowing_palette`),
// so it passed on the development host and could not run in CI's clean
// checkout (v3.1.0's release PR, #594). The two copies are byte-identical to
// the tracked files they now name; `ny2011` and `spritecans` have no tracked
// copy and were replaced by the nearest committed stimulus.

/// Frames to let a ROM boot before `AccuracyCoin`'s START press.
const BOOT_FRAMES: u32 = 300;
/// Frames START is held.
const START_FRAMES: u32 = 6;

#[derive(Debug, Clone, PartialEq, Eq)]
struct Fingerprint {
    rom: String,
    frames: u32,
    framebuffer: u64,
    audio: u64,
    ram: u64,
    cycles: u64,
}

impl Fingerprint {
    fn row(&self) -> String {
        format!(
            "{}\t{}\t{:016x}\t{:016x}\t{:016x}\t{}",
            self.rom, self.frames, self.framebuffer, self.audio, self.ram, self.cycles
        )
    }

    fn parse(line: &str) -> Option<Self> {
        let c: Vec<&str> = line.split('\t').collect();
        if c.len() != 6 {
            return None;
        }
        Some(Self {
            rom: c[0].to_string(),
            frames: c[1].parse().ok()?,
            framebuffer: u64::from_str_radix(c[2], 16).ok()?,
            audio: u64::from_str_radix(c[3], 16).ok()?,
            ram: u64::from_str_radix(c[4], 16).ok()?,
            cycles: c[5].parse().ok()?,
        })
    }
}

/// Run one probe and fingerprint it. Every frame's framebuffer is folded in,
/// not only the last, so a transient difference cannot hide behind a
/// converged final frame.
fn measure(p: &Probe) -> Fingerprint {
    let path = rom_path(p.rom);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let mut nes = Nes::from_rom(&bytes).unwrap_or_else(|e| panic!("parse {}: {e}", p.rom));
    let mut frame_hashes: Vec<u8> = Vec::with_capacity(p.frames as usize * 8);
    let mut audio: Vec<u8> = Vec::new();
    let mut step = |nes: &mut Nes| {
        nes.run_frame();
        frame_hashes.extend_from_slice(&fnv1a64(nes.framebuffer()).to_le_bytes());
        for s in nes.drain_audio() {
            audio.extend_from_slice(&s.to_le_bytes());
        }
    };
    let run = if p.press_start {
        for _ in 0..BOOT_FRAMES {
            step(&mut nes);
        }
        nes.set_buttons(0, Buttons::START);
        for _ in 0..START_FRAMES {
            step(&mut nes);
        }
        nes.set_buttons(0, Buttons::empty());
        BOOT_FRAMES + START_FRAMES
    } else {
        0
    };
    for _ in run..p.frames {
        step(&mut nes);
    }
    Fingerprint {
        rom: p.rom.to_string(),
        frames: p.frames,
        framebuffer: fnv1a64(&frame_hashes),
        audio: fnv1a64(&audio),
        ram: fnv1a64(nes.bus().ram_bytes()),
        cycles: nes.cycle(),
    }
}

fn table_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("golden/epoch_fingerprint.tsv")
}

/// The committed table: the epoch its rows describe, the epoch of the last
/// release, and the rows.
struct Table {
    epoch: u32,
    last_release_epoch: u32,
    rows: Vec<Fingerprint>,
}

fn read_table() -> Option<Table> {
    let text = fs::read_to_string(table_path()).ok()?;
    let (mut epoch, mut last) = (None, None);
    let mut rows = Vec::new();
    for line in text.lines() {
        if let Some(e) = line.strip_prefix("# epoch\t") {
            epoch = e.trim().parse().ok();
        } else if let Some(e) = line.strip_prefix("# last_release_epoch\t") {
            last = e.trim().parse().ok();
        } else if !line.starts_with('#') && !line.trim().is_empty() {
            rows.push(
                Fingerprint::parse(line)
                    .unwrap_or_else(|| panic!("malformed epoch_fingerprint.tsv row: {line:?}")),
            );
        }
    }
    Some(Table {
        epoch: epoch.expect("epoch_fingerprint.tsv has no `# epoch` line"),
        last_release_epoch: last.expect("epoch_fingerprint.tsv has no `# last_release_epoch` line"),
        rows,
    })
}

fn render_table(last_release_epoch: u32, rows: &[Fingerprint]) -> String {
    let mut out = String::from(
        "# T-EPOCH-FINGERPRINT: what the panel in tests/epoch_fingerprint.rs produces\n\
         # under the epoch below. Generated; re-bless with\n\
         # RUSTYNES_BLESS_EPOCH_FINGERPRINT=1. `last_release_epoch` is the epoch\n\
         # the last release shipped, set by hand at each release cut: output may\n\
         # move only while EMULATION_EPOCH is above it.\n",
    );
    let _ = writeln!(out, "# epoch\t{EMULATION_EPOCH}");
    let _ = writeln!(out, "# last_release_epoch\t{last_release_epoch}");
    out.push_str("# rom\tframes\tframebuffer_fnv\taudio_fnv\tram_fnv\tcpu_cycles\n");
    for r in rows {
        out.push_str(&r.row());
        out.push('\n');
    }
    out
}

#[test]
fn output_moves_only_with_the_emulation_epoch() {
    let now: Vec<Fingerprint> = PANEL.iter().map(measure).collect();
    let bless = std::env::var_os("RUSTYNES_BLESS_EPOCH_FINGERPRINT").is_some();
    let table = read_table().expect(
        "golden/epoch_fingerprint.tsv is missing or unreadable; it needs a \
         `# last_release_epoch` line written by hand before the first bless",
    );
    assert!(
        table.last_release_epoch <= EMULATION_EPOCH,
        "last_release_epoch {} is above EMULATION_EPOCH {EMULATION_EPOCH}",
        table.last_release_epoch
    );

    let moved: Vec<String> = now
        .iter()
        .zip(PANEL)
        .filter(|(f, _)| !table.rows.contains(f))
        .map(|(f, p)| format!("  {} ({}): now {}", f.rom, p.reach, f.row()))
        .collect();
    // The rule (ADR 0045): output may differ from the LAST RELEASE only under
    // a raised epoch. Within one release, a second change after the rise
    // needs a re-bless, not a second rise.
    let raised = EMULATION_EPOCH > table.last_release_epoch;

    assert!(
        moved.is_empty() || raised,
        "emulated output changed but EMULATION_EPOCH is still {EMULATION_EPOCH}, \
             the epoch the last release shipped.\n\
             Raise it in crates/rustynes-core/src/hardware_options.rs (with a row \
             in its table saying what moved), then re-bless this table with \
             RUSTYNES_BLESS_EPOCH_FINGERPRINT=1. Moved:\n{}",
        moved.join("\n")
    );

    if bless {
        fs::write(table_path(), render_table(table.last_release_epoch, &now))
            .expect("write epoch_fingerprint.tsv");
        eprintln!(
            "blessed epoch_fingerprint.tsv at epoch {EMULATION_EPOCH} ({} rows moved)",
            moved.len()
        );
        return;
    }

    assert!(
        moved.is_empty(),
        "emulated output changed under epoch {EMULATION_EPOCH}, which is already \
         raised above the last release's {}; re-bless with \
         RUSTYNES_BLESS_EPOCH_FINGERPRINT=1. Moved:\n{}",
        table.last_release_epoch,
        moved.join("\n")
    );
    assert_eq!(
        table.epoch, EMULATION_EPOCH,
        "EMULATION_EPOCH is {EMULATION_EPOCH} but the fingerprint table records {}; \
         re-bless with RUSTYNES_BLESS_EPOCH_FINGERPRINT=1 so the table names the \
         epoch its hashes belong to",
        table.epoch
    );
    assert_eq!(
        table.rows.len(),
        PANEL.len(),
        "the table has {} rows for a panel of {}; re-bless after changing the panel",
        table.rows.len(),
        PANEL.len()
    );
}
