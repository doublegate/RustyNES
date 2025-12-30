//! Comprehensive ROM loading test.
//!
//! Tests loading of all available test ROMs to validate mapper support
//! and ROM parsing across the ported codebase.

use rustynes_core::Console;
use std::fs;
use std::path::PathBuf;

/// Get the workspace root directory.
fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .unwrap()
        .parent()
        .unwrap()
        .to_path_buf()
}

/// Find all .nes files in a directory recursively.
fn find_nes_files(dir: &PathBuf) -> Vec<PathBuf> {
    let mut files = Vec::new();

    if let Ok(entries) = fs::read_dir(dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                files.extend(find_nes_files(&path));
            } else if path.extension().is_some_and(|e| e == "nes") {
                files.push(path);
            }
        }
    }

    files.sort();
    files
}

#[derive(Debug)]
struct LoadResult {
    path: String,
    success: bool,
    error: Option<String>,
}

/// Test loading all CPU test ROMs.
#[test]
#[allow(clippy::cast_precision_loss)]
fn test_load_all_cpu_roms() {
    let root = workspace_root();
    let cpu_dir = root.join("test-roms/cpu");
    let files = find_nes_files(&cpu_dir);

    println!("\n=== CPU Test ROM Loading ===");
    println!("Found {} ROM files\n", files.len());

    let mut results = Vec::new();
    let mut success_count = 0;
    let mut fail_count = 0;

    for file in &files {
        let rom_data = match fs::read(file) {
            Ok(data) => data,
            Err(e) => {
                results.push(LoadResult {
                    path: file.file_name().unwrap().to_string_lossy().to_string(),
                    success: false,
                    error: Some(format!("Read error: {e}")),
                });
                fail_count += 1;
                continue;
            }
        };

        match Console::new(&rom_data) {
            Ok(console) => {
                let name = file.file_name().unwrap().to_string_lossy().to_string();
                println!("  [OK] {name} (mapper {})", console.mapper_number());
                results.push(LoadResult {
                    path: name,
                    success: true,
                    error: None,
                });
                success_count += 1;
            }
            Err(e) => {
                let name = file.file_name().unwrap().to_string_lossy().to_string();
                println!("  [FAIL] {name} - {e}");
                results.push(LoadResult {
                    path: name,
                    success: false,
                    error: Some(e.to_string()),
                });
                fail_count += 1;
            }
        }
    }

    let pct = if files.is_empty() {
        0.0
    } else {
        (success_count as f64 / files.len() as f64) * 100.0
    };
    println!(
        "\nCPU ROMs: {success_count}/{} loaded successfully ({pct:.1}%)",
        files.len()
    );

    // Report failures
    if fail_count > 0 {
        println!("\nFailed ROMs:");
        for r in results.iter().filter(|r| !r.success) {
            let err = r.error.as_deref().unwrap_or("Unknown");
            println!("  - {}: {err}", r.path);
        }
    }
}

/// Test loading all PPU test ROMs.
#[test]
#[allow(clippy::cast_precision_loss)]
fn test_load_all_ppu_roms() {
    let root = workspace_root();
    let ppu_dir = root.join("test-roms/ppu");
    let files = find_nes_files(&ppu_dir);

    println!("\n=== PPU Test ROM Loading ===");
    println!("Found {} ROM files\n", files.len());

    let mut success_count = 0;

    for file in &files {
        if let Ok(rom_data) = fs::read(file) {
            match Console::new(&rom_data) {
                Ok(console) => {
                    let name = file.file_name().unwrap().to_string_lossy().to_string();
                    println!("  [OK] {name} (mapper {})", console.mapper_number());
                    success_count += 1;
                }
                Err(e) => {
                    let name = file.file_name().unwrap().to_string_lossy().to_string();
                    println!("  [FAIL] {name} - {e}");
                }
            }
        }
    }

    let pct = if files.is_empty() {
        0.0
    } else {
        (success_count as f64 / files.len() as f64) * 100.0
    };
    println!(
        "\nPPU ROMs: {success_count}/{} loaded successfully ({pct:.1}%)",
        files.len()
    );
}

/// Test loading all APU test ROMs.
#[test]
#[allow(clippy::cast_precision_loss)]
fn test_load_all_apu_roms() {
    let root = workspace_root();
    let apu_dir = root.join("test-roms/apu");
    let files = find_nes_files(&apu_dir);

    println!("\n=== APU Test ROM Loading ===");
    println!("Found {} ROM files\n", files.len());

    let mut success_count = 0;

    for file in &files {
        if let Ok(rom_data) = fs::read(file) {
            match Console::new(&rom_data) {
                Ok(console) => {
                    let name = file.file_name().unwrap().to_string_lossy().to_string();
                    println!("  [OK] {name} (mapper {})", console.mapper_number());
                    success_count += 1;
                }
                Err(e) => {
                    let name = file.file_name().unwrap().to_string_lossy().to_string();
                    println!("  [FAIL] {name} - {e}");
                }
            }
        }
    }

    let pct = if files.is_empty() {
        0.0
    } else {
        (success_count as f64 / files.len() as f64) * 100.0
    };
    println!(
        "\nAPU ROMs: {success_count}/{} loaded successfully ({pct:.1}%)",
        files.len()
    );
}

/// Summary test that loads all ROMs across all categories.
#[test]
#[allow(clippy::cast_precision_loss)]
fn test_load_summary() {
    let root = workspace_root();
    let test_roms_dir = root.join("test-roms");

    if !test_roms_dir.exists() {
        println!("Test ROMs directory not found, skipping comprehensive test");
        return;
    }

    let all_files = find_nes_files(&test_roms_dir);

    println!("\n========================================");
    println!("  Comprehensive ROM Loading Summary");
    println!("========================================\n");
    println!("Total ROM files found: {}\n", all_files.len());

    let mut total_success = 0;
    let mut mapper_counts: std::collections::HashMap<u16, usize> = std::collections::HashMap::new();
    let mut unsupported_mappers: std::collections::HashMap<String, usize> =
        std::collections::HashMap::new();

    for file in &all_files {
        if let Ok(rom_data) = fs::read(file) {
            match Console::new(&rom_data) {
                Ok(console) => {
                    total_success += 1;
                    *mapper_counts.entry(console.mapper_number()).or_insert(0) += 1;
                }
                Err(e) => {
                    let err_str = e.to_string();
                    if err_str.contains("mapper") || err_str.contains("Mapper") {
                        *unsupported_mappers.entry(err_str).or_insert(0) += 1;
                    }
                }
            }
        }
    }

    let pct = if all_files.is_empty() {
        0.0
    } else {
        (total_success as f64 / all_files.len() as f64) * 100.0
    };

    println!("Results:");
    println!(
        "  - Successfully loaded: {total_success}/{} ({pct:.1}%)",
        all_files.len()
    );

    println!("\nMapper usage:");
    let mut mapper_vec: Vec<_> = mapper_counts.iter().collect();
    mapper_vec.sort_by_key(|(m, _)| *m);
    for (mapper, count) in mapper_vec {
        println!("  - Mapper {mapper}: {count} ROMs");
    }

    if !unsupported_mappers.is_empty() {
        println!("\nUnsupported mappers:");
        for (err, count) in &unsupported_mappers {
            println!("  - {err} ({count} ROMs)");
        }
    }

    println!("\n========================================\n");

    // Ensure at least some ROMs load
    assert!(total_success > 0, "At least some ROMs should load");
}
