//! v2.0.0 "Timebase" beta.1 — five-counter coherence invariants.
//!
//! The shipping R1 core keeps FIVE cycle counters that all advance exactly
//! once (or by one region divider) per CPU cycle, at different points within
//! the cycle: `Cpu::master_clock` (master-clock units), `Cpu::cycles`,
//! `LockstepBus::cycle`, `LockstepBus::ppu_clock`, and `Apu::cpu_cycle`.
//! They are kept in lockstep by hand-written `+= 1` statements rather than
//! being derived from one source of truth — the exact substrate the v2.0.0
//! one-clock collapse (ADR 0002 + `to-dos/plans/v2.0.0-master-clock-plan.md`
//! Workstream A1) replaces.
//!
//! These tests are the collapse's PRE-CONDITION EVIDENCE and its permanent
//! regression guard: at every frame boundary the counters must sit in a
//! FIXED AFFINE relation —
//!
//! - `master_clock - cpu_divider * cycles` is constant (any DMA fold or
//!   split-half accounting error that advanced the master clock without a
//!   matching whole cycle, or vice versa, moves this residue);
//! - `bus.cycle - cpu.cycles` is constant (the bus-side per-cycle hook
//!   fires exactly once per CPU-emitted cycle, including every unified-DMA
//!   engine cycle);
//! - `apu.cpu_cycle - cpu.cycles` is constant (the APU tick — the RW-1
//!   `apu_phase`/`put_cycle` parity source — fires exactly once per cycle).
//!
//! The constants themselves are boot-sequence offsets (reset vector reads,
//! seeding) and are intentionally NOT pinned to specific values — only their
//! frame-over-frame stability is the invariant. Two workloads: `nestest`
//! (CPU/branch heavy, no DMA) and `AccuracyCoin` through its DMC+OAM DMA
//! window (the historical drift surface: the 17-rollback DMA axis).
//!
//! Per `docs/testing-strategy.md` §Layer 3 + the v2.0.0 plan §Verification.

#![cfg(feature = "test-roms")]

use std::fs;
use std::path::PathBuf;

use rustynes_core::{Buttons, Nes};

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

/// The three affine residues between the cycle counters, sampled at a frame
/// boundary (the run loop polls `frame_complete` between CPU instructions,
/// so `master_clock` sits at a whole-cycle boundary).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
struct CounterResidues {
    /// `master_clock - cpu_divider * cpu.cycles` (wrapping arithmetic — the
    /// residue is compared, never interpreted as a magnitude).
    mc_minus_div_cycles: u64,
    /// `bus.cycle - cpu.cycles`.
    bus_minus_cpu: u64,
    /// `apu.cpu_cycle - cpu.cycles`.
    apu_minus_cpu: u64,
}

const fn residues(nes: &Nes, cpu_divider: u64) -> CounterResidues {
    let cpu_cycles = nes.cpu().cycles;
    CounterResidues {
        mc_minus_div_cycles: nes
            .cpu()
            .master_clock()
            .wrapping_sub(cpu_divider.wrapping_mul(cpu_cycles)),
        bus_minus_cpu: nes.bus().cycle().wrapping_sub(cpu_cycles),
        apu_minus_cpu: nes.bus().apu().cpu_cycle().wrapping_sub(cpu_cycles),
    }
}

/// Run `frames` frames asserting the residues never move after the first
/// frame boundary. Returns the pinned residues for diagnostics.
fn assert_residues_stable(nes: &mut Nes, frames: u64, label: &str) -> CounterResidues {
    // NTSC AccuracyCoin / nestest: divider 12. Read it from the region so a
    // future PAL/Dendy workload keeps the math honest.
    let cpu_divider: u64 = match nes.region() {
        rustynes_core::Region::Pal => 16,
        rustynes_core::Region::Dendy => 15,
        rustynes_core::Region::Ntsc => 12,
    };
    nes.run_frame();
    let pinned = residues(nes, cpu_divider);
    for f in 1..frames {
        nes.run_frame();
        let now = residues(nes, cpu_divider);
        assert_eq!(
            now, pinned,
            "{label}: counter residues drifted at frame {f} (pinned {pinned:?}, now {now:?}) — \
             a cycle counter advanced out of lockstep with the others; the one-clock collapse \
             precondition does not hold"
        );
    }
    pinned
}

/// CPU/branch-heavy workload with zero DMA: the counters' baseline lockstep.
#[test]
fn one_clock_counter_residues_stable_nestest() {
    let bytes = fs::read(rom_path("nestest/nestest.nes")).expect("read nestest");
    let mut nes = Nes::from_rom(&bytes).expect("parse nestest");
    let pinned = assert_residues_stable(&mut nes, 240, "nestest");
    eprintln!("nestest pinned residues: {pinned:?}");
}

/// DMA-heavy workload: `AccuracyCoin` booted + started, run through the DMC+OAM
/// DMA battery window (the `$0477` result lands around frame ~1640 on the
/// current default). Every unified-DMA engine cycle must keep all five
/// counters in lockstep — this is the surface where the 17-rollback drift
/// history lived.
#[test]
fn one_clock_counter_residues_stable_accuracycoin_dma_window() {
    let bytes = fs::read(rom_path("accuracycoin/AccuracyCoin.nes")).expect("read AccuracyCoin");
    let mut nes = Nes::from_rom(&bytes).expect("parse AccuracyCoin");

    // Boot + press START to launch the battery (the
    // `accuracy_coin::run_battery_capturing_ram` protocol).
    for _ in 0..300 {
        nes.run_frame();
    }
    nes.set_buttons(0, Buttons::START);
    for _ in 0..6 {
        nes.run_frame();
    }
    nes.set_buttons(0, Buttons::empty());

    // Run through the DMA cluster (DMC+OAM result at ~frame 1640) with the
    // residue assertion on every frame.
    let pinned = assert_residues_stable(&mut nes, 1800, "AccuracyCoin DMA window");
    eprintln!("AccuracyCoin pinned residues: {pinned:?}");

    // Sanity: the DMC+OAM battery result must actually have been reached, or
    // the "stable through the DMA window" claim is vacuous.
    let ram = nes.bus().ram_bytes();
    assert_ne!(
        ram[0x0477], 0,
        "AccuracyCoin DMC+OAM result ($0477) never set — the DMA window was not exercised"
    );
}
