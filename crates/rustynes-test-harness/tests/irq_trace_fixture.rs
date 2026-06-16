//! M2-phase IRQ-timing tracing fixture (Track C1 pre-work).
//!
//! Drives `mmc3_test_2/4-scanline_timing` and `cpu_interrupts_v2/{1..5}`
//! to completion with the `irq-timing-trace` cargo feature enabled.
//! Each run dumps a full per-CPU-cycle trace to
//! `target/irq_trace/<rom>.full.csv` (diagnostic; not committed) plus a
//! filtered "IRQ-relevant cycles only" trace to
//! `crates/rustynes-test-harness/golden/irq_trace/<rom>.csv` (committed as
//! the baseline; future trace diffs use it as the empirical oracle the
//! four rolled-back IRQ-timing attempts could not be evaluated against).
//!
//! See ADR-0002 "Decision (revised, 2026-05-13)" → "Test fixture".
//!
//! # Running
//!
//! ```bash
//! cargo test -p rustynes-test-harness \
//!     --features test-roms,irq-timing-trace \
//!     --test irq_trace_fixture
//! ```
//!
//! # Why this is its own test file
//!
//! The fixture is heavy (~3-4 M records per ROM, ~160 MB peak across
//! the 6 tests).  We do *not* want it running on every
//! `cargo test --workspace` — it's a diagnostic tool, gated behind two
//! cargo features, and only needed when investigating IRQ-timing
//! changes.  Standard CI does not enable `irq-timing-trace`.

#![cfg(all(feature = "test-roms", feature = "irq-timing-trace"))]

use std::fs;
use std::path::PathBuf;

use rustynes_core::Nes;
use rustynes_core::irq_trace::IrqTrace;

/// Per-ROM linear-buffer cap.  Sized to comfortably hold the entire
/// run from `BOOT_FRAMES_TO_SKIP` to final-status detection for every
/// target ROM (max 126 frames - 10 boot ≈ 116 frames × 29 780 cycles ≈
/// 3.5 M records).  Records past the cap are silently dropped, but we
/// size this generously so that does not happen in practice.  Each
/// record is ~40 bytes → ~160 MB peak across all 6 fixture tests.
const MAX_RECORDS: usize = 4_000_000;

/// Frames to run BEFORE enabling the trace, to skip the boot phase where
/// rendering is off / CHR has not been banked / IRQs have not been
/// enabled.  Empirically `mmc3_test_2/4-scanline_timing` reaches its
/// rendering-on phase by frame ~15; `cpu_interrupts_v2/*` are faster.
const BOOT_FRAMES_TO_SKIP: u64 = 10;
/// Per-ROM frame cap.  Most blargg ROMs finish their first measurement
/// in 50-200 frames; 600 is the same ceiling the other test files use.
const MAX_FRAMES: u64 = 600;
// Optional Phase 1.3 lower-bound: when set via the
// `RUSTYNES_IRQ_TRACE_START_CYCLE` environment variable, run additional
// frames after `BOOT_FRAMES_TO_SKIP` until `bus.cycle() >= START_CYCLE`
// before arming the trace.  Mirrors the Mesen2 Lua script's
// `MESEN2_IRQ_TRACE_START_CYCLE` knob so both sides arm at the same
// CPU cycle and the cross-diff sees no boot/anchor offset.
fn rom_path(rel: &str) -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .join("tests")
        .join("roms")
        .join(rel)
}

fn workspace_root() -> PathBuf {
    let manifest = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    manifest
        .parent()
        .and_then(|p| p.parent())
        .expect("workspace root")
        .to_path_buf()
}

/// Run a single ROM with tracing enabled.  Writes the full per-CPU-cycle
/// trace to `target/irq_trace/<slug>.full.csv` (diagnostic; not
/// committed) and the IRQ-event-filtered trace to
/// `crates/rustynes-test-harness/golden/irq_trace/<slug>.csv` (committed).
/// Returns the filtered CSV string for the well-formed assertion.
fn run_traced(rel: &str, slug: &str) -> String {
    let path = rom_path(rel);
    let bytes = fs::read(&path).unwrap_or_else(|e| panic!("read {}: {}", path.display(), e));
    let mut nes = Nes::from_rom(&bytes).expect("rom must parse");
    // Boot phase: run for `BOOT_FRAMES_TO_SKIP` frames without tracing
    // so the IRQ-active phase starts near the beginning of the captured
    // window.  The trace's linear capacity (`MAX_RECORDS`) is sized
    // generously past the IRQ-relevant window, so post-failure activity
    // is also captured if it exists.
    for _ in 0..BOOT_FRAMES_TO_SKIP {
        nes.run_frame();
    }
    // Phase 1.3 of Track C1 attempt 14: optionally skip further frames
    // until the CPU cycle counter passes a user-supplied threshold.  Set
    // `RUSTYNES_IRQ_TRACE_START_CYCLE=250000` to match the Mesen2 Lua
    // script's `MESEN2_IRQ_TRACE_START_CYCLE` knob; the resulting trace
    // omits the boot/anchor offset region and exposes only in-test-loop
    // divergence.  Default 0 = behavior is byte-identical to the
    // Phase A baseline.
    if let Ok(s) = std::env::var("RUSTYNES_IRQ_TRACE_START_CYCLE") {
        let start_cycle: u64 = s.parse().unwrap_or(0);
        if start_cycle > 0 {
            while nes.bus().cycle() < start_cycle {
                nes.run_frame();
            }
            eprintln!(
                "[{slug}] post-START_CYCLE skip arrived at cycle={} (target {})",
                nes.bus().cycle(),
                start_cycle
            );
        }
    }
    nes.bus_mut().enable_irq_trace(MAX_RECORDS);
    // The blargg test-runner ROMs write `$DE $B0 $61` to `$6001-$6003`
    // on this build (different from `nes_runner.rs`'s `[b'D', b'E', b'B']`
    // — open-bus / mapper-RAM init difference).  Use that as the
    // "test running" marker; final status appears at `$6000` thereafter.
    let magic_target: [u8; 3] = [0xDE, 0xB0, 0x61];
    let mut break_reason = "max_frames";
    let mut break_frame = MAX_FRAMES;
    let mut prev_status = 0u8;
    for frame in BOOT_FRAMES_TO_SKIP..MAX_FRAMES {
        nes.run_frame();
        let status = nes.bus_mut().peek_cpu(0x6000);
        let magic = [
            nes.bus_mut().peek_cpu(0x6001),
            nes.bus_mut().peek_cpu(0x6002),
            nes.bus_mut().peek_cpu(0x6003),
        ];
        // Stop the moment the ROM transitions out of "running" ($80) or
        // "needs reset" ($81) into a final status byte (gated on the
        // magic bytes being present so we don't catch boot zeros).
        if magic == magic_target && status != 0x80 && status != 0x81 && prev_status != status {
            break_reason = "final_status_transition";
            break_frame = frame;
            break;
        }
        prev_status = status;
    }
    eprintln!("[{slug}] ran {break_frame} frames (reason: {break_reason})");
    let ppu_regs = nes.bus().ppu().debug_registers();
    let final_status = nes.bus_mut().peek_cpu(0x6000);
    let trace = nes
        .bus_mut()
        .take_irq_trace()
        .expect("trace was enabled above");
    let events_in_memory: usize = trace.records().iter().map(|r| r.a12_events.len()).sum();
    let kept = trace.len();
    let overflow = trace.overflow();
    let a12 = trace.notify_a12_count;
    let with_a12 = trace.records_with_a12_count;
    let svc = trace.notify_irq_service_count;
    let svc_len = trace.service_events().len();
    let ctrl = ppu_regs[0];
    let mask = ppu_regs[1];
    let status = ppu_regs[2];
    eprintln!(
        "[{slug}] kept={kept} overflow={overflow} notify_a12={a12} \
         records_with_a12={with_a12} events_in_memory={events_in_memory} \
         notify_irq_service={svc} service_events_len={svc_len} \
         PPU end ctrl=${ctrl:02X} mask=${mask:02X} status=${status:02X} \
         $6000=${final_status:02X}",
    );
    // Full trace -> target/irq_trace/<slug>.full.csv (diagnostic; not
    // committed).
    let full_csv = trace.to_csv();
    let out_dir = workspace_root().join("target").join("irq_trace");
    fs::create_dir_all(&out_dir).expect("create target/irq_trace");
    fs::write(out_dir.join(format!("{slug}.full.csv")), &full_csv).expect("write full trace csv");
    // Filtered trace -> crates/rustynes-test-harness/golden/irq_trace/<slug>.csv
    // (committed as the baseline that future coordinated-change attempts
    // diff against).
    //
    // Session-21 design decision: the IRQ-focused trace uses the same
    // `is_irq_event` filter as before (only IRQ-line / NMI / A12
    // transitions).  ROMs without DMC activity produce byte-identical
    // (modulo the new column suffixes) filtered output to the
    // pre-Session-21 baseline.  For DMC-active ROMs (the new
    // `dmc_dma_during_read4_*` sentinel) we additionally write a
    // `*.dmc.csv` golden file using the wider `is_dmc_or_irq_event`
    // filter — the DMC trace is what Sprint 1 iteration 2's diagnosis
    // pass cross-diffs against Mesen2.  The two filters are
    // intentionally separate so committing this branch does not
    // explode the cpu_interrupts_v2/* golden files (the OAM DMA
    // window's `dma_cycles_owed` decrement is per-cycle activity and
    // would 100x the previously-tight IRQ-only baselines).
    let filtered_csv = trace.to_csv_filtered(IrqTrace::is_irq_event);
    let golden_dir = workspace_root()
        .join("crates")
        .join("rustynes-test-harness")
        .join("golden")
        .join("irq_trace");
    fs::create_dir_all(&golden_dir).expect("create golden/irq_trace");
    fs::write(golden_dir.join(format!("{slug}.csv")), &filtered_csv)
        .expect("write golden trace csv");
    // Phase 1.2 sidecar: write the vector-service event trace as a
    // separate `*.svc.csv` golden file.  Schema is documented in
    // `crates/rustynes-core/src/irq_trace.rs::IrqTrace::service_events_to_csv`.
    // Each row is one IRQ or NMI vector fetch; cross-diffable against
    // Mesen2's `scripts/mesen2_irq_trace.lua` `irq_svc` / `nmi_svc` rows.
    let service_csv = trace.service_events_to_csv();
    fs::write(golden_dir.join(format!("{slug}.svc.csv")), &service_csv)
        .expect("write golden service-events csv");
    // Session-21 sidecar: DMC-focused filtered trace.  Uses
    // `is_dmc_or_irq_event` to retain DMC scheduler state changes (in
    // addition to IRQ-line transitions).  Written ALONGSIDE the
    // IRQ-focused trace so the per-cycle DMC visibility is captured in
    // the committed golden surface without bloating the historical
    // IRQ-only files.  Sprint 1 iteration 2 cross-diffs against this
    // sidecar.
    let dmc_csv = trace.to_csv_filtered(IrqTrace::is_dmc_or_irq_event);
    fs::write(golden_dir.join(format!("{slug}.dmc.csv")), &dmc_csv)
        .expect("write golden DMC trace csv");
    filtered_csv
}

/// Asserts the trace's shape is plausible: monotonically increasing CPU
/// cycles and a CSV header line.  Empty filtered traces are allowed —
/// NROM ROMs with both pattern tables at `$0000` and APU IRQs disabled
/// will legitimately produce a header-only filtered trace.
fn assert_trace_well_formed(csv: &str) {
    let mut lines = csv.lines();
    let header = lines.next().expect("non-empty trace");
    assert!(
        header.starts_with("cpu_cycle,"),
        "trace CSV header malformed: {header}"
    );
    let mut prev: i64 = -1;
    for line in lines {
        let first = line.split(',').next().unwrap_or("");
        let cyc: i64 = first
            .parse()
            .unwrap_or_else(|_| panic!("bad cycle: {line}"));
        assert!(cyc > prev, "trace cycles not monotonic: {prev} -> {cyc}");
        prev = cyc;
    }
}

#[test]
fn mmc3_test_2_4_scanline_timing_baseline_trace() {
    let csv = run_traced(
        "blargg/mmc3_test_2/4-scanline_timing.nes",
        "mmc3_test_2_4_scanline_timing",
    );
    assert_trace_well_formed(&csv);
}

#[test]
fn cpu_interrupts_v2_2_nmi_and_brk_baseline_trace() {
    let csv = run_traced(
        "blargg/cpu_interrupts_v2/2-nmi_and_brk.nes",
        "cpu_interrupts_v2_2_nmi_and_brk",
    );
    assert_trace_well_formed(&csv);
}

#[test]
fn cpu_interrupts_v2_3_nmi_and_irq_baseline_trace() {
    let csv = run_traced(
        "blargg/cpu_interrupts_v2/3-nmi_and_irq.nes",
        "cpu_interrupts_v2_3_nmi_and_irq",
    );
    assert_trace_well_formed(&csv);
}

#[test]
fn cpu_interrupts_v2_4_irq_and_dma_baseline_trace() {
    let csv = run_traced(
        "blargg/cpu_interrupts_v2/4-irq_and_dma.nes",
        "cpu_interrupts_v2_4_irq_and_dma",
    );
    assert_trace_well_formed(&csv);
}

#[test]
fn cpu_interrupts_v2_5_branch_delays_irq_baseline_trace() {
    let csv = run_traced(
        "blargg/cpu_interrupts_v2/5-branch_delays_irq.nes",
        "cpu_interrupts_v2_5_branch_delays_irq",
    );
    assert_trace_well_formed(&csv);
}

/// Control ROM: `1-cli_latency` is the only `cpu_interrupts_v2` sub-ROM
/// currently passing strictly.  Its trace is the "what success looks
/// like" reference — any coordinated-change attempt that breaks this
/// trace's IRQ-sampling pattern has regressed sub-ROM 1.
#[test]
fn cpu_interrupts_v2_1_cli_latency_passing_trace() {
    let csv = run_traced(
        "blargg/cpu_interrupts_v2/1-cli_latency.nes",
        "cpu_interrupts_v2_1_cli_latency",
    );
    assert_trace_well_formed(&csv);
}

/// Session-21 (Sprint 1 iteration 2 prereq): DMC-active baseline trace.
///
/// `dmc_dma_during_read4/dma_2007_read.nes` is the canonical DMC DMA
/// regression sentinel — the strict-pass 5-of-5 suite that documents
/// the DMC scheduler's contract against `$2007` reads halted by DMA.
/// Its trace covers DMC DMA halt → dummy → align → get cycles plus the
/// 2A03 register-conflict path (DMC sample fetches forwarded to PPU
/// `$2007` re-reads).  Cross-diffing this trace against Mesen2 is the
/// oracle for Sprint 1 iteration 2's DMC scheduler calibration audit
/// (the work that rolled back in Sessions-19 + 20 because the
/// `dmc_abort_delay` / `dmc_dma_cooldown` / `dmc_dma_short` constants
/// were calibrated to a bus-quiet implied-opcode T2 baseline that
/// Mesen2 does not share).
#[test]
fn dmc_dma_during_read4_dma_2007_read_baseline_trace() {
    let csv = run_traced(
        "blargg/dmc_dma_during_read4/dma_2007_read.nes",
        "dmc_dma_during_read4_dma_2007_read",
    );
    assert_trace_well_formed(&csv);
}
