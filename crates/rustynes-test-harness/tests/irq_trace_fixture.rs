//! M2-phase IRQ-timing tracing fixture (Track C1 pre-work).
//!
//! Drives `mmc3_test_2/4-scanline_timing`, `cpu_interrupts_v2/{1..5}` and
//! `dmc_dma_during_read4/dma_2007_read` to completion with the
//! `irq-timing-trace` cargo feature enabled. Each run dumps a full
//! per-CPU-cycle trace to `target/irq_trace/<rom>.full.csv` (diagnostic; not
//! committed) and three filtered traces -- `<rom>.csv` (IRQ-relevant cycles),
//! `<rom>.svc.csv` (vector services) and `<rom>.dmc.csv` (DMC scheduler plus
//! IRQ) -- which are COMPARED, byte for byte, against the goldens committed
//! under `crates/rustynes-test-harness/golden/irq_trace/`.
//!
//! See ADR-0002 "Decision (revised, 2026-05-13)" → "Test fixture".
//!
//! # The goldens are a gate (v3.3.0)
//!
//! Until v3.3.0 this fixture WROTE the goldens on every run and compared
//! nothing against them, so they could not catch a regression however far
//! the timing moved -- and they had not been regenerated since v2.0.0's
//! betas, so every committed trace described a core that no longer existed,
//! while each run silently rewrote them in the working tree (where a broad
//! `git add` could sweep them into an unrelated commit). Now a run compares,
//! and on a difference it writes what it measured to
//! `target/irq_trace/<file>.actual` and fails, naming the first differing
//! line. Re-recording is a deliberate act: set `RUSTYNES_IRQ_TRACE_BLESS=1`,
//! and commit the result only with every difference attributed to an
//! intended change -- the same rule as any other golden in this repository.
//! A capture with a non-default `RUSTYNES_IRQ_TRACE_START_CYCLE` describes a
//! different window, so it is written to `target/irq_trace/` and never
//! compared or blessed.
//!
//! # Running
//!
//! ```bash
//! cargo test -p rustynes-test-harness \
//!     --features test-roms,irq-timing-trace \
//!     --test irq_trace_fixture
//! # re-record, after an intended timing change:
//! RUSTYNES_IRQ_TRACE_BLESS=1 cargo test -p rustynes-test-harness \
//!     --features test-roms,irq-timing-trace --test irq_trace_fixture
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
use std::sync::Mutex;

use rustynes_core::Nes;
use rustynes_core::irq_trace::{IrqTrace, ServiceKind};

/// Per-ROM linear-buffer cap: one record per CPU cycle from the arm point to
/// the stop condition, which must fit WHOLE -- `run_traced` refuses an
/// overflowed default-window capture, because a golden taken from a truncated
/// buffer describes a window that ends wherever the cap happened to fall.
///
/// It was 4,000,000, sized when the longest run was ~116 frames, and three
/// runs had outgrown it unnoticed (measured at v3.3.0, records needed):
/// `dma_2007_read` 17,570,497 (it never reports a final status and stops at
/// `MAX_FRAMES`), `5-branch_delays_irq` 11,048,565 and mmc3
/// `4-scanline_timing` 9,172,439. The next largest fits at 3,663,022. A
/// record is roughly 80 bytes (its `a12_events` `Vec` plus ~50 bytes of
/// fields), so the largest run peaks near 1.4 GB and the seven in parallel at
/// a few GB -- acceptable for a diagnostic fixture CI does not run.
const MAX_RECORDS: usize = 24_000_000;

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

/// The `RUSTYNES_IRQ_TRACE_START_CYCLE` knob: the CPU cycle after which
/// recording starts, 0 (the default window) when unset or unparsable.
fn trace_start_cycle() -> u64 {
    std::env::var("RUSTYNES_IRQ_TRACE_START_CYCLE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(0)
}

/// `RUSTYNES_IRQ_TRACE_BLESS=1`: re-record the goldens instead of comparing.
fn bless() -> bool {
    std::env::var("RUSTYNES_IRQ_TRACE_BLESS").as_deref() == Ok("1")
}

/// Compares the traces one ROM produced against the committed goldens, or
/// re-records them under `RUSTYNES_IRQ_TRACE_BLESS=1`. Each entry is
/// `(file name, measured contents)`. All three files are checked before the
/// test fails, so one run reports every golden that moved, and each moved one
/// leaves its measurement at `target/irq_trace/<file>.actual` to diff.
fn check_goldens(slug: &str, files: &[(String, &str)]) {
    let golden_dir = workspace_root()
        .join("crates")
        .join("rustynes-test-harness")
        .join("golden")
        .join("irq_trace");
    let out_dir = workspace_root().join("target").join("irq_trace");
    fs::create_dir_all(&out_dir).expect("create target/irq_trace");
    // A later start cycle is a different capture window: it can be neither
    // compared with nor blessed over the default-window goldens.
    if trace_start_cycle() > 0 {
        for (name, actual) in files {
            fs::write(out_dir.join(name), actual).expect("write windowed trace csv");
        }
        return;
    }
    if bless() {
        fs::create_dir_all(&golden_dir).expect("create golden/irq_trace");
        for (name, actual) in files {
            fs::write(golden_dir.join(name), actual).expect("write golden trace csv");
        }
        return;
    }
    let mut moved = Vec::new();
    for (name, actual) in files {
        let golden = fs::read_to_string(golden_dir.join(name)).unwrap_or_default();
        if golden == *actual {
            continue;
        }
        let first = golden
            .lines()
            .zip(actual.lines())
            .position(|(g, a)| g != a)
            .unwrap_or_else(|| golden.lines().count().min(actual.lines().count()));
        let artifact = out_dir.join(format!("{name}.actual"));
        fs::write(&artifact, actual).expect("write actual trace csv");
        moved.push(format!(
            "{name}: first difference at line {} ({} golden lines, {} measured); measured trace in {}",
            first + 1,
            golden.lines().count(),
            actual.lines().count(),
            artifact.display()
        ));
    }
    assert!(
        moved.is_empty(),
        "[{slug}] the IRQ trace moved against the committed goldens. If the change is \
         intended, re-record with RUSTYNES_IRQ_TRACE_BLESS=1 and attribute every \
         difference before committing.\n  {}",
        moved.join("\n  ")
    );
}

/// Runs a single ROM with tracing enabled, writes the full per-CPU-cycle trace
/// to `target/irq_trace/<slug>.full.csv` (diagnostic; not committed), and
/// checks the three filtered traces against the goldens (`check_goldens`).
/// Returns the IRQ-filtered CSV for the well-formed assertion.
/// Serialises the captures. The harness runs these seven tests in parallel by
/// default, and with every run captured whole (`MAX_RECORDS`) the longest
/// three would hold their buffers at once: 4.5 GB peak RSS measured at v3.3.0
/// with all seven concurrent, enough to be killed on a smaller host (Codex on
/// #612). One capture at a time bounds the peak by the largest single run, so
/// the documented command needs no `--test-threads=1`. A poisoned lock (a
/// previous test panicked) is taken anyway: its holder held no shared state.
static CAPTURE: Mutex<()> = Mutex::new(());

fn run_traced(rel: &str, slug: &str) -> String {
    let _capture = CAPTURE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
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
    let start_cycle = trace_start_cycle();
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
    // The unfiltered CSV is the largest single allocation here (one line per
    // CPU cycle); free it before the filtered traces are built.
    drop(full_csv);
    // Filtered trace -> golden/irq_trace/<slug>.csv.
    //
    // Session-21 design decision: the IRQ-focused trace uses the same
    // `is_irq_event` filter as before (only IRQ-line / NMI / A12
    // transitions).  For DMC-active ROMs (the `dmc_dma_during_read4_*`
    // sentinel) a `*.dmc.csv` golden is written beside it using the wider
    // `is_dmc_or_irq_event` filter -- the DMC trace is what Sprint 1
    // iteration 2's diagnosis pass cross-diffs against Mesen2.  The two
    // filters are intentionally separate so the cpu_interrupts_v2/* goldens
    // stay tight (the OAM DMA window's `dma_cycles_owed` decrement is
    // per-cycle activity and would 100x the IRQ-only baselines).
    let filtered_csv = trace.to_csv_filtered(IrqTrace::is_irq_event);
    // Phase 1.2 sidecar: the vector-service event trace, `*.svc.csv`.
    // Schema is documented in
    // `crates/rustynes-core/src/irq_trace.rs::IrqTrace::service_events_to_csv`.
    // Each row is one IRQ or NMI vector fetch; cross-diffable against
    // Mesen2's `scripts/mesen2_irq_trace.lua` `irq_svc` / `nmi_svc` rows.
    assert_service_kinds_agree_with_mesen2(slug, &trace);
    let service_csv = trace.service_events_to_csv();
    // Session-21 sidecar: the DMC-focused filtered trace, `*.dmc.csv`.
    let dmc_csv = trace.to_csv_filtered(IrqTrace::is_dmc_or_irq_event);
    // A trace that hit `MAX_RECORDS` dropped its tail, so a golden compared
    // (or blessed) from it would describe a truncated run and could pass or
    // re-record a capture that never saw the cycles it claims to cover. The
    // default window is sized never to overflow; a windowed capture is written
    // to `target/` only, so it is reported above and not refused here.
    assert!(
        overflow == 0 || trace_start_cycle() > 0,
        "[{slug}] the trace dropped {overflow} records past MAX_RECORDS ({MAX_RECORDS}); \
         raise the cap before comparing or blessing"
    );
    check_goldens(
        slug,
        &[
            (format!("{slug}.csv"), filtered_csv.as_str()),
            (format!("{slug}.svc.csv"), service_csv.as_str()),
            (format!("{slug}.dmc.csv"), dmc_csv.as_str()),
        ],
    );
    filtered_csv
}

/// Round 2 audit CORE-R2-01 (v3.3.0): the service events' `kind` must say
/// NMI where an NMI was serviced. Until v3.3.0 the CPU passed
/// `vector == NMI_VECTOR` with the IRQ vector the unified dispatch always
/// enters with, so every service was labelled IRQ.
///
/// Checked against the Mesen2 trace for the same ROM, where one exists
/// (`golden/irq_trace/mesen2/<slug>.csv`, black-box oracle output): this
/// trace has an NMI-kind row if and only if Mesen2 logged an `nmi_svc`, and
/// every NMI-kind row fetched `$FFFA`. Counts are not compared, because the
/// two labels differ by design: `kind` names the request that started the
/// sequence, so an IRQ or BRK that an NMI hijacks is IRQ-kind with vector
/// `$FFFA` (see `ServiceEvent::vector`), where Mesen2 logs it as an NMI; and
/// the two captures cover different windows.
///
/// The hijacks themselves are pinned by count for the two ROMs built to
/// produce them, before the Mesen2 lookup so they hold without it (#611
/// review). The counts were measured at v3.3.0 (deterministic over repeated
/// runs). The `.svc.csv` goldens beside them now gate every service event too
/// (they were rewritten on every run, and stale since v2.0.0, until v3.3.0);
/// the counts stay because they say WHICH property a moved golden broke.
///
/// The counts and the Mesen2 comparison describe the default capture window.
/// A positive `RUSTYNES_IRQ_TRACE_START_CYCLE` starts recording later, after
/// services those two checks expect, so for such a capture only the per-event
/// check (an NMI-kind row fetched `$FFFA`) applies (#611 review).
fn assert_service_kinds_agree_with_mesen2(slug: &str, trace: &IrqTrace) {
    let events = trace.service_events();
    assert!(
        events
            .iter()
            .filter(|e| e.kind == ServiceKind::Nmi)
            .all(|e| e.vector == 0xFFFA),
        "[{slug}] an NMI-kind service fetched another vector"
    );
    if trace_start_cycle() > 0 {
        return;
    }
    let expected_hijacks = match slug {
        "cpu_interrupts_v2_2_nmi_and_brk" | "cpu_interrupts_v2_3_nmi_and_irq" => Some(5),
        _ => None,
    };
    if let Some(expected) = expected_hijacks {
        let hijacks = events
            .iter()
            .filter(|e| e.kind == ServiceKind::Irq && e.vector == 0xFFFA)
            .count();
        assert_eq!(
            hijacks, expected,
            "[{slug}] IRQ-kind service events that fetched $FFFA (NMI hijacks)"
        );
    }
    let mesen = workspace_root()
        .join("crates/rustynes-test-harness/golden/irq_trace/mesen2")
        .join(format!("{slug}.csv"));
    let Ok(mesen) = fs::read_to_string(&mesen) else {
        return;
    };
    let mesen_nmi = mesen
        .lines()
        .filter(|l| l.split(',').nth(4) == Some("nmi_svc"))
        .count();
    let ours_nmi: Vec<_> = events
        .iter()
        .filter(|e| e.kind == ServiceKind::Nmi)
        .collect();
    assert_eq!(
        ours_nmi.is_empty(),
        mesen_nmi == 0,
        "[{slug}] {} NMI-kind service events here, {mesen_nmi} nmi_svc in Mesen2",
        ours_nmi.len()
    );
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
