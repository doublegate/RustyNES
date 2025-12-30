//! Performance Benchmarks for RustyNES Emulator
//!
//! This benchmark suite measures the performance of key emulation components:
//! - CPU instruction execution
//! - PPU frame rendering
//! - Full console frame stepping
//! - ROM loading and initialization

#![allow(missing_docs)]

use criterion::{BenchmarkId, Criterion, Throughput, black_box, criterion_group, criterion_main};
use rustynes_core::Console;
use std::fs;
use std::path::PathBuf;
use std::time::Duration;

/// Get the workspace root directory.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Load a test ROM if available.
fn load_test_rom(name: &str) -> Option<Vec<u8>> {
    let root = workspace_root();
    let path = root.join("test-roms").join(name);
    if path.exists() {
        fs::read(&path).ok()
    } else {
        None
    }
}

/// Create a minimal valid NES ROM for benchmarking.
fn create_minimal_rom() -> Vec<u8> {
    let mut rom = vec![0u8; 16 + 32768 + 8192]; // Header + 32KB PRG + 8KB CHR

    // iNES header
    rom[0] = 0x4E; // 'N'
    rom[1] = 0x45; // 'E'
    rom[2] = 0x53; // 'S'
    rom[3] = 0x1A; // EOF
    rom[4] = 2; // 32KB PRG-ROM (2 x 16KB)
    rom[5] = 1; // 8KB CHR-ROM
    rom[6] = 0x01; // Mapper 0, vertical mirroring

    // Reset vector at $FFFC-$FFFD points to $8000
    rom[16 + 0x7FFC] = 0x00; // Low byte
    rom[16 + 0x7FFD] = 0x80; // High byte

    // Simple program at $8000: infinite loop (JMP $8000)
    rom[16] = 0x4C; // JMP absolute
    rom[17] = 0x00; // Low byte
    rom[18] = 0x80; // High byte

    rom
}

/// Benchmark CPU instruction execution speed.
fn bench_cpu_instructions(c: &mut Criterion) {
    let rom_data = create_minimal_rom();
    let mut console = Console::new(&rom_data).expect("Failed to create console");
    console.power_on();

    let mut group = c.benchmark_group("cpu");
    group.throughput(Throughput::Elements(1));
    group.measurement_time(Duration::from_secs(5));

    // Benchmark single instruction steps
    group.bench_function("single_instruction", |b| {
        b.iter(|| {
            black_box(console.step());
        });
    });

    // Benchmark 1000 instructions
    group.bench_function("1000_instructions", |b| {
        b.iter(|| {
            for _ in 0..1000 {
                black_box(console.step());
            }
        });
    });

    group.finish();
}

/// Benchmark PPU frame rendering.
fn bench_ppu_frames(c: &mut Criterion) {
    let rom_data = create_minimal_rom();
    let mut console = Console::new(&rom_data).expect("Failed to create console");
    console.power_on();

    let mut group = c.benchmark_group("ppu");
    group.throughput(Throughput::Elements(1));
    group.measurement_time(Duration::from_secs(10));

    // Benchmark running to next frame
    group.bench_function("single_frame", |b| {
        b.iter(|| {
            console.step_frame();
            black_box(console.framebuffer());
        });
    });

    group.finish();
}

/// Benchmark full console operation with real ROMs.
fn bench_real_rom_execution(c: &mut Criterion) {
    // Try to load a real test ROM
    let rom_data = load_test_rom("cpu/nestest.nes").unwrap_or_else(create_minimal_rom);

    let mut console = Console::new(&rom_data).expect("Failed to create console");
    console.power_on();

    let mut group = c.benchmark_group("console");
    group.throughput(Throughput::Elements(1));
    group.measurement_time(Duration::from_secs(10));

    // Benchmark frame execution
    group.bench_function("nestest_frame", |b| {
        b.iter(|| {
            console.step_frame();
            black_box(console.framebuffer());
        });
    });

    // Benchmark 60 frames (1 second of emulation)
    group.bench_function("60_frames", |b| {
        b.iter(|| {
            for _ in 0..60 {
                console.step_frame();
            }
            black_box(console.framebuffer());
        });
    });

    group.finish();
}

/// Benchmark ROM loading and initialization.
fn bench_rom_loading(c: &mut Criterion) {
    let rom_data = create_minimal_rom();

    let mut group = c.benchmark_group("initialization");
    group.throughput(Throughput::Elements(1));
    group.measurement_time(Duration::from_secs(5));

    // Benchmark console creation
    group.bench_function("console_new", |b| {
        b.iter(|| {
            let console = Console::new(black_box(&rom_data)).expect("Failed to create console");
            black_box(console);
        });
    });

    // Benchmark power-on sequence
    group.bench_function("power_on", |b| {
        let mut console = Console::new(&rom_data).expect("Failed to create console");
        b.iter(|| {
            console.power_on();
            black_box(console.total_cycles());
        });
    });

    // Benchmark reset
    group.bench_function("reset", |b| {
        let mut console = Console::new(&rom_data).expect("Failed to create console");
        console.power_on();
        b.iter(|| {
            console.reset();
            black_box(console.total_cycles());
        });
    });

    group.finish();
}

/// Benchmark with different ROM sizes.
fn bench_mapper_variations(c: &mut Criterion) {
    let mut group = c.benchmark_group("mappers");
    group.measurement_time(Duration::from_secs(5));

    // Test with minimal ROM (NROM)
    let rom_data = create_minimal_rom();
    let mut console = Console::new(&rom_data).expect("Failed to create console");
    console.power_on();

    group.bench_with_input(BenchmarkId::new("frame", "NROM"), &(), |b, ()| {
        b.iter(|| {
            console.step_frame();
            black_box(console.framebuffer());
        });
    });

    // Try different test ROMs if available
    if let Some(rom_data) = load_test_rom("mappers/mapper_holymapperel_1_P128K.nes")
        && let Ok(mut console) = Console::new(&rom_data)
    {
        console.power_on();
        group.bench_with_input(BenchmarkId::new("frame", "MMC1"), &(), |b, ()| {
            b.iter(|| {
                console.step_frame();
                black_box(console.framebuffer());
            });
        });
    }

    if let Some(rom_data) = load_test_rom("mappers/mapper_holymapperel_4_P128K.nes")
        && let Ok(mut console) = Console::new(&rom_data)
    {
        console.power_on();
        group.bench_with_input(BenchmarkId::new("frame", "MMC3"), &(), |b, ()| {
            b.iter(|| {
                console.step_frame();
                black_box(console.framebuffer());
            });
        });
    }

    group.finish();
}

/// Benchmark memory access patterns.
fn bench_memory_access(c: &mut Criterion) {
    let rom_data = create_minimal_rom();
    let mut console = Console::new(&rom_data).expect("Failed to create console");
    console.power_on();

    let mut group = c.benchmark_group("memory");
    group.throughput(Throughput::Bytes(1));
    group.measurement_time(Duration::from_secs(5));

    // Benchmark memory peek
    group.bench_function("peek_ram", |b| {
        b.iter(|| {
            black_box(console.peek_memory(black_box(0x0000)));
        });
    });

    group.bench_function("peek_prg", |b| {
        b.iter(|| {
            black_box(console.peek_memory(black_box(0x8000)));
        });
    });

    // Benchmark sequential memory reads
    group.bench_function("peek_sequential_256", |b| {
        b.iter(|| {
            for addr in 0..256u16 {
                black_box(console.peek_memory(addr));
            }
        });
    });

    group.finish();
}

criterion_group!(
    benches,
    bench_cpu_instructions,
    bench_ppu_frames,
    bench_real_rom_execution,
    bench_rom_loading,
    bench_mapper_variations,
    bench_memory_access,
);
criterion_main!(benches);
