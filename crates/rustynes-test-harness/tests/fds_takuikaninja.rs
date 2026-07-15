//! BIOS-gated smoke tests against `TakuikaNinja`'s FDS hardware-verification
//! probes (`FDS-Mirroring-Test`, `FDS-4023-Test`, `FDS-Audio-Registers`,
//! `FDS-4030D1-Addr` — see the `NESdev` Wiki "Emulator tests" page).
//!
//! None of these four ROMs carries an explicit permissive license (no
//! `LICENSE` file, nothing stated in any README as of this writing), so
//! unlike the rest of `tests/roms/` they are **not committed** — they live
//! gitignored under `tests/roms/external/fds-takuikaninja/`, the same
//! copyright-ambiguous staging convention used for commercial ROM dumps.
//! Every test here is a no-op skip (with a printed notice) unless both the
//! real FDS BIOS (`RUSTYNES_FDS_BIOS`, mirroring `fds.rs`) and the specific
//! probe disk are present on disk, so CI stays clean by default.
//!
//! These probe the exact `$4023`/mirroring behavior that
//! `crates/rustynes-mappers/src/fds.rs` already implements and unit-tests
//! independently (`clear_4023_stops_and_acks` et al.) — they are
//! regression-insurance against a second, hardware-verified oracle, not a
//! fix for a known gap. As with `fds_irq_tests_with_real_bios`, we assert
//! only that construction + a bounded run complete without panicking; these
//! ROMs render pass/fail state as on-screen text/register dumps, and
//! decoding that programmatically (framebuffer or nametable text-scrape) is
//! a follow-up once the exact screen layout is confirmed against real
//! hardware captures — asserting a specific outcome without that would
//! claim precision this harness does not actually verify.
//!
//! `FDS-4030D1-Addr` in particular probes the FDS DRAM-refresh-watchdog IRQ
//! (`$4030.D1`), which upstream research (and every current FDS emulator,
//! per the `NESdev` Wiki) explicitly notes as unimplemented/under-research;
//! `RustyNES` does not model it either, so that probe is expected to show
//! "not observed" (`XXXX`) rather than a specific timing value — this test
//! exists to track that honest residual, not to assert it away.
//!
//! Run all four (real BIOS + all four probe disks present):
//! ```text
//! RUSTYNES_FDS_BIOS=/path/to/disksys.rom \
//!   cargo test -p rustynes-test-harness --features test-roms --test fds_takuikaninja -- --nocapture
//! ```

#![cfg(feature = "test-roms")]

use rustynes_core::Nes;

const BIOS_LEN: usize = 0x2000;

/// Shared skip-gated runner: reads the BIOS + the named probe disk from
/// `tests/roms/external/fds-takuikaninja/`, constructs, runs `frames`
/// frames, and prints a diagnostic. Returns early (with an `eprintln!` skip
/// notice) if either file is absent — the CI convention already established
/// by `fds_irq_tests_with_real_bios`.
fn run_probe(test_name: &str, rom_filename: &str, frames: u64) {
    let Ok(bios_path) = std::env::var("RUSTYNES_FDS_BIOS") else {
        eprintln!(
            "SKIP {test_name}: set RUSTYNES_FDS_BIOS=/path/to/disksys.rom to run the \
             TakuikaNinja FDS probes (BIOS is never committed)."
        );
        return;
    };
    let bios = match std::fs::read(&bios_path) {
        Ok(b) if b.len() == BIOS_LEN => b,
        Ok(b) => {
            eprintln!(
                "SKIP {test_name}: BIOS at {bios_path} is {} bytes, expected {BIOS_LEN}.",
                b.len()
            );
            return;
        }
        Err(e) => {
            eprintln!("SKIP {test_name}: cannot read {bios_path}: {e}");
            return;
        }
    };

    let disk_path = format!(
        concat!(
            env!("CARGO_MANIFEST_DIR"),
            "/../../tests/roms/external/fds-takuikaninja/{}"
        ),
        rom_filename
    );
    let disk = match std::fs::read(&disk_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!(
                "SKIP {test_name}: cannot read {disk_path}: {e} (fetch it from the TakuikaNinja \
                 GitHub release and place it there — see this file's module doc)."
            );
            return;
        }
    };

    let mut nes = Nes::from_disk(&disk, &bios).expect("FDS construct with real BIOS");
    for _ in 0..frames {
        nes.run_frame();
    }
    eprintln!(
        "{test_name}: ran {frames} frames with real BIOS ({rom_filename}). Mapper debug: {:?}",
        nes.mapper_info()
    );
}

/// `FDS-Mirroring-Test`: `$4025.D3` write, `$4030.D3` read, and the
/// previously-undocumented `$4023.D0=0` nametable-arrangement reset.
#[test]
fn fds_mirroring_test_with_real_bios() {
    run_probe(
        "fds_mirroring_test_with_real_bios",
        "mirroring-test.fds",
        600,
    );
}

/// `FDS-4023-Test`: read-only register states ($4020-$409F) while toggling
/// bits 0/1 of `$4023`.
#[test]
fn fds_4023_test_with_real_bios() {
    run_probe("fds_4023_test_with_real_bios", "4023-test.fds", 600);
}

/// `FDS-Audio-Registers`: FDS audio register readback while toggling
/// `$4023.D1` during wavetable playback.
#[test]
fn fds_audio_registers_with_real_bios() {
    run_probe(
        "fds_audio_registers_with_real_bios",
        "audio-registers.fds",
        600,
    );
}

/// `FDS-4030D1-Addr`: DRAM-refresh-watchdog IRQ timing reported by
/// `$4030.D1`. Not modeled by `RustyNES` (nor, per upstream, by most current
/// FDS emulators) — this probe tracks that known, honest residual.
#[test]
fn fds_4030d1_addr_with_real_bios() {
    run_probe("fds_4030d1_addr_with_real_bios", "4030d1-addr.fds", 600);
}
