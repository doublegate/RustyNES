#![allow(missing_docs)]
//! Criterion bench — `Cpu::step` throughput on a synthetic NOP image.
//!
//! Per Track B6 of the gap-analysis remediation plan. The point of this
//! bench is **baseline numbers** rather than tight micro-optimization
//! gates; the values populate `docs/performance.md` and the future
//! `mapper_dispatch` ADR (D1).
//!
//! Methodology: a minimal `Bus` impl returns `0xEA` (NOP) for every
//! read, swallows writes, and never asserts NMI/IRQ. The CPU then
//! executes pure-NOP code at PC=0, two cycles per instruction. Cache
//! pressure is therefore minimal; this is an upper bound on
//! instructions-per-second, not a representative workload.

use criterion::{criterion_group, criterion_main, Criterion};
use rustynes_cpu::{Bus, Cpu};
use std::hint::black_box;

/// Minimal bus: NOPs everywhere, no IRQ/NMI, no cycle callback work.
struct NopBus {
    cycles: u64,
}

impl NopBus {
    const fn new() -> Self {
        Self { cycles: 0 }
    }
}

impl Bus for NopBus {
    fn cpu_read(&mut self, _addr: u16) -> u8 {
        0xEA // NOP opcode
    }

    fn cpu_write(&mut self, _addr: u16, _value: u8) {}

    fn on_cpu_cycle(&mut self) {
        self.cycles += 1;
    }
}

fn bench_nop_loop(c: &mut Criterion) {
    c.bench_function("cpu_nop_step_x1000", |b| {
        b.iter_batched(
            || {
                let mut bus = NopBus::new();
                let mut cpu = Cpu::new();
                // Synthetic reset: set PC=0 and clear the I flag so IRQs would
                // service (they won't — NopBus never asserts).
                cpu.reset(&mut bus);
                // After reset PC reads from $FFFC/D = 0xEA / 0xEA = 0xEAEA;
                // every memory cell is NOP so it doesn't matter where we run.
                (cpu, bus)
            },
            |(mut cpu, mut bus)| {
                for _ in 0..1000 {
                    let cycles = cpu.step(&mut bus);
                    black_box(cycles);
                }
            },
            criterion::BatchSize::SmallInput,
        );
    });
}

criterion_group!(benches, bench_nop_loop);
criterion_main!(benches);
