//! Comprehensive test ROM runner for RustyNES validation
//!
//! This tool runs all test ROMs and generates a detailed report.

use rustynes_core::Console;
use std::fs;
use std::path::{Path, PathBuf};
use std::time::Instant;

#[derive(Debug, Clone)]
pub struct TestResult {
    pub rom_name: String,
    pub rom_path: PathBuf,
    pub status: TestStatus,
    pub result_code: Option<u8>,
    pub execution_time_ms: u64,
    pub cycles: u64,
    pub error_message: Option<String>,
    pub category: String,
}

#[derive(Debug, Clone, PartialEq)]
pub enum TestStatus {
    Pass,
    Fail,
    Timeout,
    LoadError,
    NotRun,
    Ignored,
}

impl TestStatus {
    pub fn as_str(&self) -> &str {
        match self {
            TestStatus::Pass => "PASS",
            TestStatus::Fail => "FAIL",
            TestStatus::Timeout => "TIMEOUT",
            TestStatus::LoadError => "LOAD_ERROR",
            TestStatus::NotRun => "NOT_RUN",
            TestStatus::Ignored => "IGNORED",
        }
    }
}

pub struct TestRunner {
    test_rom_dir: PathBuf,
    timeout_frames: u32,
}

impl TestRunner {
    pub fn new(test_rom_dir: PathBuf) -> Self {
        Self {
            test_rom_dir,
            timeout_frames: 600, // 10 seconds at 60fps
        }
    }

    /// Run a single test ROM
    pub fn run_test(&self, rom_path: &Path, category: &str) -> TestResult {
        let rom_name = rom_path
            .file_name()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();

        println!("  Testing: {}", rom_name);

        let start_time = Instant::now();

        // Load ROM
        let rom_data = match fs::read(rom_path) {
            Ok(data) => data,
            Err(e) => {
                return TestResult {
                    rom_name,
                    rom_path: rom_path.to_path_buf(),
                    status: TestStatus::LoadError,
                    result_code: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                    cycles: 0,
                    error_message: Some(format!("Failed to read ROM: {}", e)),
                    category: category.to_string(),
                };
            }
        };

        // Create console
        let mut console = match Console::from_rom_bytes(&rom_data) {
            Ok(c) => c,
            Err(e) => {
                return TestResult {
                    rom_name,
                    rom_path: rom_path.to_path_buf(),
                    status: TestStatus::LoadError,
                    result_code: None,
                    execution_time_ms: start_time.elapsed().as_millis() as u64,
                    cycles: 0,
                    error_message: Some(format!("Failed to create console: {}", e)),
                    category: category.to_string(),
                };
            }
        };

        // Run test ROM
        let mut frames = 0;
        let mut result_code: Option<u8> = None;

        while frames < self.timeout_frames {
            // Step one frame
            console.step_frame();
            frames += 1;

            // Check result at $6000
            // Note: This requires Console to expose a way to read memory
            // For now, we'll assume test completes when it stabilizes
            // TODO: Add Console::read_memory(addr: u16) -> u8 method

            // Temporary: Run for full timeout
        }

        let execution_time_ms = start_time.elapsed().as_millis() as u64;
        let cycles = console.cycles();

        // Determine status based on result code
        let status = if frames >= self.timeout_frames {
            TestStatus::Timeout
        } else if let Some(code) = result_code {
            if code == 0x00 {
                TestStatus::Pass
            } else {
                TestStatus::Fail
            }
        } else {
            TestStatus::NotRun
        };

        TestResult {
            rom_name,
            rom_path: rom_path.to_path_buf(),
            status,
            result_code,
            execution_time_ms,
            cycles,
            error_message: None,
            category: category.to_string(),
        }
    }

    /// Run all test ROMs in a category
    pub fn run_category(&self, category: &str) -> Vec<TestResult> {
        let category_path = self.test_rom_dir.join(category);

        println!("\n=== Testing {} ROMs ===", category.to_uppercase());

        let mut results = Vec::new();

        // Get all .nes files in category
        if let Ok(entries) = fs::read_dir(&category_path) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("nes") {
                    let result = self.run_test(&path, category);
                    println!("    {} - {}", result.rom_name, result.status.as_str());
                    results.push(result);
                }
            }
        }

        results
    }

    /// Run all test ROMs across all categories
    pub fn run_all(&self) -> Vec<TestResult> {
        let mut all_results = Vec::new();

        // Test each category
        for category in &["cpu", "ppu", "apu", "mappers"] {
            let results = self.run_category(category);
            all_results.extend(results);
        }

        all_results
    }
}

/// Generate markdown report from test results
pub fn generate_report(results: &[TestResult]) -> String {
    let mut report = String::new();

    // Header
    report.push_str("# RustyNES Test ROM Validation Results\n\n");
    report.push_str(&format!("**Date**: {}\n", chrono::Local::now().format("%Y-%m-%d %H:%M:%S")));
    report.push_str(&format!("**Total Test ROMs**: {}\n\n", results.len()));

    // Executive Summary
    report.push_str("## Executive Summary\n\n");

    let total = results.len();
    let passed = results.iter().filter(|r| r.status == TestStatus::Pass).count();
    let failed = results.iter().filter(|r| r.status == TestStatus::Fail).count();
    let timeout = results.iter().filter(|r| r.status == TestStatus::Timeout).count();
    let load_error = results.iter().filter(|r| r.status == TestStatus::LoadError).count();
    let ignored = results.iter().filter(|r| r.status == TestStatus::Ignored).count();

    let pass_rate = if total > 0 {
        (passed as f64 / total as f64) * 100.0
    } else {
        0.0
    };

    report.push_str(&format!("| Metric | Count | Percentage |\n"));
    report.push_str(&format!("|--------|-------|------------|\n"));
    report.push_str(&format!("| **Total Tests** | {} | 100.0% |\n", total));
    report.push_str(&format!("| **Passed** | {} | {:.1}% |\n", passed, pass_rate));
    report.push_str(&format!("| **Failed** | {} | {:.1}% |\n", failed, (failed as f64 / total as f64) * 100.0));
    report.push_str(&format!("| **Timeout** | {} | {:.1}% |\n", timeout, (timeout as f64 / total as f64) * 100.0));
    report.push_str(&format!("| **Load Error** | {} | {:.1}% |\n", load_error, (load_error as f64 / total as f64) * 100.0));
    report.push_str(&format!("| **Ignored** | {} | {:.1}% |\n\n", ignored, (ignored as f64 / total as f64) * 100.0));

    // Category breakdown
    report.push_str("## Results by Category\n\n");

    for category in &["cpu", "ppu", "apu", "mappers"] {
        let category_results: Vec<_> = results.iter().filter(|r| r.category == *category).collect();

        if category_results.is_empty() {
            continue;
        }

        let cat_total = category_results.len();
        let cat_passed = category_results.iter().filter(|r| r.status == TestStatus::Pass).count();
        let cat_pass_rate = (cat_passed as f64 / cat_total as f64) * 100.0;

        report.push_str(&format!("### {} Tests ({}/{}  - {:.1}%)\n\n",
            category.to_uppercase(), cat_passed, cat_total, cat_pass_rate));

        report.push_str("| Test ROM | Status | Time (ms) | Cycles | Notes |\n");
        report.push_str("|----------|--------|-----------|--------|-------|\n");

        for result in category_results {
            let notes = result.error_message.as_ref()
                .map(|m| m.as_str())
                .unwrap_or("");

            report.push_str(&format!("| {} | {} | {} | {} | {} |\n",
                result.rom_name,
                result.status.as_str(),
                result.execution_time_ms,
                result.cycles,
                notes
            ));
        }

        report.push_str("\n");
    }

    // Failure Analysis
    report.push_str("## Failure Analysis\n\n");

    let failures: Vec<_> = results.iter()
        .filter(|r| r.status == TestStatus::Fail || r.status == TestStatus::Timeout)
        .collect();

    if failures.is_empty() {
        report.push_str("No failures detected!\n\n");
    } else {
        report.push_str(&format!("{} tests failed or timed out:\n\n", failures.len()));

        for failure in failures {
            report.push_str(&format!("### {}\n\n", failure.rom_name));
            report.push_str(&format!("- **Status**: {}\n", failure.status.as_str()));
            report.push_str(&format!("- **Category**: {}\n", failure.category));
            if let Some(ref msg) = failure.error_message {
                report.push_str(&format!("- **Error**: {}\n", msg));
            }
            if let Some(code) = failure.result_code {
                report.push_str(&format!("- **Result Code**: 0x{:02X}\n", code));
            }
            report.push_str("\n");
        }
    }

    report
}
