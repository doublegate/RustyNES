#![allow(missing_docs)]
//! Criterion bench — `Box<dyn Mapper>::cpu_read` dispatch overhead.
//!
//! Per Track B6 of the gap-analysis remediation plan. Baseline numbers
//! that feed the D1 ADR ("mapper dispatch — `Box<dyn>` vs. monomorphized
//! `MapperEnum`").
//!
//! Methodology: build a representative spread of mappers via the
//! `rustynes_mappers::parse` path (using vendored test ROMs as a side-effect
//! of having real PRG/CHR bytes), then dispatch a uniformly-distributed
//! sequence of CPU reads at addresses spanning `$4020-$FFFF` against
//! each. The bench measures **dispatch + mapper-internal logic**
//! together — that's the bench you can meaningfully compare against an
//! equivalent monomorphized impl.
//!
//! This file vendors no ROMs of its own; it reads `tests/roms/*.nes`
//! relative to `$CARGO_MANIFEST_DIR`. If a path is missing the
//! corresponding row is silently skipped.

use std::path::PathBuf;

use criterion::{criterion_group, criterion_main, BenchmarkId, Criterion};
use rustynes_mappers::{parse, Mapper};
use std::hint::black_box;

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

fn build_mapper(rel: &str) -> Option<Box<dyn Mapper>> {
    let path = rom_path(rel);
    let bytes = std::fs::read(&path).ok()?;
    let (_cart, mapper) = parse(&bytes).ok()?;
    Some(mapper)
}

fn bench_dispatch(c: &mut Criterion) {
    // One ROM per mapper family we care to bench. Some are smoke-only ROMs
    // already in the corpus; using them here is fine — we never assert on
    // their bytes, only on dispatch latency.
    let mappers: &[(&str, &str)] = &[
        ("NROM (0)", "nestest/nestest.nes"),
        ("MMC1 (1)", "blargg/instr_test_v5/all_instrs.nes"),
        ("MMC3 (4)", "blargg/mmc3_test_2/1-clocking.nes"),
        ("MMC5 (5)", "mmc5/mapper_mmc5test_v1.nes"),
        ("M34 (34)", "holy_mapperel/M34_P128K_CR8K_H.nes"),
        ("FME-7 (69)", "holy_mapperel/M69_P128K_C64K_W8K.nes"),
    ];

    let mut group = c.benchmark_group("mapper_dispatch_cpu_read");
    for (label, rel) in mappers {
        let Some(mut mapper) = build_mapper(rel) else {
            continue;
        };
        group.bench_with_input(BenchmarkId::from_parameter(label), label, |b, _| {
            // 1024 reads sampled deterministically across $4020-$FFFF; this
            // mixes PRG-RAM, register, and ROM regions for the mappers that
            // see traffic at all of those.
            b.iter(|| {
                let mut sum: u32 = 0;
                let mut addr: u16 = 0x4020;
                for _ in 0..1024 {
                    sum = sum.wrapping_add(u32::from(mapper.cpu_read(black_box(addr))));
                    addr = addr.wrapping_add(0xB);
                    if addr < 0x4020 {
                        addr = 0x4020;
                    }
                }
                sum
            });
        });
    }
    group.finish();
}

criterion_group!(benches, bench_dispatch);
criterion_main!(benches);
