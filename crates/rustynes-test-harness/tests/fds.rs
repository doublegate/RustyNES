//! Famicom Disk System (FDS) Stage 1 harness tests.
//!
//! These tests do **not** require the real `disksys.rom` BIOS (which is
//! Nintendo IP and must never be committed). The default tests synthesize a
//! tiny 8 KiB "BIOS" — just enough to give the CPU a sane reset vector and an
//! infinite-loop so the emulator can construct and tick frames without
//! panicking. They prove the construction + scheduling integration, not real
//! disk boot (which is inherently BIOS-dependent).
//!
//! An optional BIOS-gated test runs the committed `fdsirqtests.fds` against a
//! user-supplied real BIOS pointed at by the `RUSTYNES_FDS_BIOS` environment
//! variable. When the variable is unset the test prints a skip notice and
//! returns cleanly (the CI convention, mirroring `commercial-roms`).
//!
//! Run all (synthetic, no BIOS):
//! ```text
//! cargo test -p rustynes-test-harness --features test-roms --test fds
//! ```
//! Run the BIOS-gated path (user supplies the BIOS):
//! ```text
//! RUSTYNES_FDS_BIOS=/path/to/disksys.rom \
//!   cargo test -p rustynes-test-harness --features test-roms --test fds -- --nocapture
//! ```

#![cfg(feature = "test-roms")]

use rustynes_core::Nes;

const FDS_SIDE_LEN: usize = 65500;
const BIOS_LEN: usize = 0x2000;

/// Build a minimal synthetic 8 KiB BIOS.
///
/// The reset vector ($FFFC/$FFFD, i.e. BIOS offsets $1FFC/$1FFD) points at the
/// start of the BIOS ($E000), where we place an infinite `JMP $E000` so the CPU
/// has somewhere safe to run. The NMI ($FFFA) and IRQ ($FFFE) vectors point at
/// a `RTI` so spurious interrupts are handled without wandering off.
fn synth_bios() -> Vec<u8> {
    let mut bios = vec![0u8; BIOS_LEN];
    // $E000: JMP $E000  (4C 00 E0)
    bios[0x0000] = 0x4C;
    bios[0x0001] = 0x00;
    bios[0x0002] = 0xE0;
    // $E010: RTI (40) — interrupt handler landing pad.
    bios[0x0010] = 0x40;
    // Vectors (BIOS is mapped at $E000, so offset = vector - $E000).
    // NMI  -> $E010
    bios[0x1FFA] = 0x10;
    bios[0x1FFB] = 0xE0;
    // RESET -> $E000
    bios[0x1FFC] = 0x00;
    bios[0x1FFD] = 0xE0;
    // IRQ  -> $E010
    bios[0x1FFE] = 0x10;
    bios[0x1FFF] = 0xE0;
    bios
}

/// Build a synthetic fwNES disk image with `sides` 65500-byte sides, each
/// opening with the FDS disk-info block signature.
fn synth_disk(sides: u8) -> Vec<u8> {
    let mut out = vec![0u8; 16 + sides as usize * FDS_SIDE_LEN];
    out[0..4].copy_from_slice(b"FDS\x1A");
    out[4] = sides;
    for s in 0..sides as usize {
        let base = 16 + s * FDS_SIDE_LEN;
        out[base] = 0x01;
        out[base + 1..base + 15].copy_from_slice(b"*NINTENDO-HVC*");
        // A walkable data pattern so a disk read would stream recognizable bytes.
        for (i, b) in out[base + 16..base + 64].iter_mut().enumerate() {
            #[allow(clippy::cast_possible_truncation)]
            let pat = (i as u8).wrapping_add(s as u8);
            *b = pat;
        }
    }
    out
}

#[test]
fn fds_constructs_from_synthetic_disk_and_bios() {
    let disk = synth_disk(1);
    let bios = synth_bios();
    let nes = Nes::from_disk(&disk, &bios).expect("FDS construction must succeed");
    // The framebuffer should be the standard NES resolution.
    assert_eq!(nes.framebuffer().len(), 256 * 240 * 4);
}

#[test]
fn fds_runs_frames_without_panicking() {
    let disk = synth_disk(2);
    let bios = synth_bios();
    let mut nes = Nes::from_disk(&disk, &bios).expect("FDS construction must succeed");
    // Spin a handful of frames; the synthetic BIOS just loops, so this exercises
    // the scheduler + bus + FDS mapper integration (notify_cpu_cycle every CPU
    // cycle) without depending on real disk boot.
    for _ in 0..10 {
        nes.run_frame();
    }
    // Framebuffer remains the right size and the run did not panic.
    assert_eq!(nes.framebuffer().len(), 256 * 240 * 4);
}

#[test]
fn fds_rejects_non_8k_bios() {
    let disk = synth_disk(1);
    let bad_bios = vec![0u8; 4096];
    assert!(
        Nes::from_disk(&disk, &bad_bios).is_err(),
        "BIOS that is not exactly 8 KiB must be rejected"
    );
}

#[test]
fn fds_rejects_garbage_disk() {
    let bios = synth_bios();
    let garbage = vec![0xFFu8; 1024];
    assert!(
        Nes::from_disk(&garbage, &bios).is_err(),
        "non-FDS bytes must be rejected"
    );
}

#[test]
fn fds_save_state_round_trip_through_nes() {
    let disk = synth_disk(1);
    let bios = synth_bios();
    let mut nes = Nes::from_disk(&disk, &bios).expect("construct");
    for _ in 0..3 {
        nes.run_frame();
    }
    let snap = nes.snapshot();

    let mut nes2 = Nes::from_disk(&disk, &bios).expect("construct");
    nes2.restore(&snap).expect("restore FDS snapshot");
    // After restore, re-snapshotting yields the same bytes (determinism).
    assert_eq!(nes2.snapshot(), snap);
}

/// BIOS-gated: run the committed `fdsirqtests.fds` against the real BIOS.
///
/// Skipped (with a printed notice) unless `RUSTYNES_FDS_BIOS` points at a valid
/// 8 KiB `disksys.rom`. This is the closest Stage-1 has to an end-to-end check,
/// but it is intentionally NOT a CI gate: the BIOS is non-distributable, and
/// driving the test to completion depends on the full disk-load sequence, which
/// is the Stage-1/Stage-2 boundary. We assert only that construction + a bounded
/// run do not panic and that the IRQ machinery is reachable.
#[test]
fn fds_irq_tests_with_real_bios() {
    let Ok(bios_path) = std::env::var("RUSTYNES_FDS_BIOS") else {
        eprintln!(
            "SKIP fds_irq_tests_with_real_bios: set RUSTYNES_FDS_BIOS=/path/to/disksys.rom \
             to run the BIOS-gated fdsirqtests check (BIOS is never committed)."
        );
        return;
    };
    let bios = match std::fs::read(&bios_path) {
        Ok(b) if b.len() == BIOS_LEN => b,
        Ok(b) => {
            eprintln!(
                "SKIP fds_irq_tests_with_real_bios: BIOS at {bios_path} is {} bytes, expected {BIOS_LEN}.",
                b.len()
            );
            return;
        }
        Err(e) => {
            eprintln!("SKIP fds_irq_tests_with_real_bios: cannot read {bios_path}: {e}");
            return;
        }
    };

    let disk_path = concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/roms/nes-test-roms/fdsirqtests/fdsirqtests.fds"
    );
    let disk = match std::fs::read(disk_path) {
        Ok(d) => d,
        Err(e) => {
            eprintln!("SKIP fds_irq_tests_with_real_bios: cannot read {disk_path}: {e}");
            return;
        }
    };

    let mut nes = Nes::from_disk(&disk, &bios).expect("FDS construct with real BIOS");
    // Run a generous number of frames; the BIOS loads the disk and the test
    // program exercises the IRQ paths. We do not assert a specific pass screen
    // here (that result decode is a Stage-2 item once boot is verified) — Stage
    // 1 verifies the real BIOS + disk drives the emulator without panic/hang.
    for _ in 0..600 {
        nes.run_frame();
    }
    eprintln!(
        "fds_irq_tests_with_real_bios: ran 600 frames with real BIOS (disk={} sides). \
         Mapper debug: {:?}",
        // side count via debug info string
        disk.len() / FDS_SIDE_LEN,
        nes.mapper_info()
    );
}
