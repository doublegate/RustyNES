// Diagnostic probe behind the v2.1.7 "Stepping" 2A03-revision / DMA-frontier
// work (ADR 0033): snapshot-hashes each DMA test ROM under Rp2A03G vs Rp2A03H
// to see whether the revision toggle perturbs the deterministic core on that
// ROM. Not wired into CI -- drop into `crates/rustynes-test-harness/tests/`
// (it expects a sibling `common` module, same as the other harness tests) and
// run with `cargo test --features test-roms -- --nocapture probe`.
#![cfg(feature = "test-roms")]
mod common;
use common::{fnv1a64, rom_path};
use std::fs;
use rustynes_core::{Cpu2A03Revision, Nes};
fn snaphash(name: &str, rev: Cpu2A03Revision, frames: u64) -> u64 {
    let bytes = fs::read(rom_path(name)).unwrap();
    let mut nes = Nes::from_rom(&bytes).unwrap();
    nes.set_cpu_2a03_revision(rev);
    for _ in 0..frames { nes.run_frame(); }
    fnv1a64(&nes.snapshot())
}
#[test]
fn probe() {
    for name in [
        "nes-test-roms/sprdma_and_dmc_dma/sprdma_and_dmc_dma.nes",
        "nes-test-roms/sprdma_and_dmc_dma/sprdma_and_dmc_dma_512.nes",
        "blargg/dmc_dma_during_read4/dma_2007_read.nes",
        "blargg/dmc_dma_during_read4/dma_4016_read.nes",
        "blargg/dmc_dma_during_read4/double_2007_read.nes",
        "blargg/dmc_dma_during_read4/read_write_2007.nes",
        "accuracycoin/AccuracyCoin.nes",
    ] {
        let g = snaphash(name, Cpu2A03Revision::Rp2A03G, 400);
        let h = snaphash(name, Cpu2A03Revision::Rp2A03H, 400);
        eprintln!("{}: G={:016x} H={:016x} differ={}", name, g, h, g!=h);
    }
}
