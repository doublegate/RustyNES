//! AccuracyCoin test-name catalog + RAM-direct result decoder.
//!
//! Vendored from upstream `100thCoin/AccuracyCoin` (MIT licensed). The
//! list mirrors `AccuracyCoin.asm`'s 20 `Suite_*` pages: each page
//! contributes a header string + a sequence of `table "name", $FF,
//! result_addr, run_addr` macro entries. Total: 146 entries across 20
//! suites.
//!
//! ## Source of truth
//!
//! The authoritative list lives next to the ROM at
//! `tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv` as a 146-line
//! `(suite<TAB>name<TAB>result_addr)` file extracted from upstream
//! `AccuracyCoin.asm` by the recipe documented inline in
//! `tests/roms/AccuracyCoin/README.md` (walk each `Suite_*`/`table` block,
//! resolving `result_symbol` to its `result_X = $ADDR` definition).
//! This module embeds that file via `include_str!` and parses it
//! lazily so the in-code catalog cannot drift from the on-disk source.
//!
//! ## Result encoding (per `AccuracyCoin.asm` `TEST_Fail` / `TEST_Pass`
//! convention)
//!
//! Each test stores a single result byte at its `result_addr`:
//!
//! | Byte         | Meaning                                            |
//! |--------------|----------------------------------------------------|
//! | `$00`        | not run (the ROM's `result_Unimplemented` default) |
//! | `$01`        | PASS (clean — no overlay)                          |
//! | `(N<<2)\|$01` | PASS with code N (light-blue sprite overlay)      |
//! | `(N<<2)\|$02` | FAIL with error code N                            |
//! | `$FF`        | skipped (the ROM's "$C9 unique square" tile)       |
//!
//! The display routine at `AREROM_PageColumnLoop2` does:
//! `AND #$01 BEQ AREROM_PrintFail ... LDA result; AND #$FE BEQ ...` —
//! so the "pass with code" branch is `bit-0 set AND bits 7..2 non-zero`.
//! Five "Power On State" tests share `$03FF` (the ROM's `result_DrawTest`
//! sentinel) because the upstream display routine excludes them from
//! the result table (page 3 is print-only); they always read as `$00`
//! "not run" via this decoder.
//!
//! ## Source
//!
//! `https://github.com/100thCoin/AccuracyCoin` — MIT licensed.

#![cfg(feature = "test-roms")]
#![allow(dead_code, clippy::doc_markdown)]

use std::sync::OnceLock;

/// One entry in the AccuracyCoin test catalog.
#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CatalogEntry {
    /// Page (suite) the test belongs to. Matches the `Suite_*` header
    /// string in `AccuracyCoin.asm`.
    pub suite: String,
    /// Test name as displayed in the page menu.
    pub name: String,
    /// CPU-RAM byte address where this test writes its result. All
    /// addresses fall within CPU RAM (`$0000-$07FF`). The five
    /// "Power On State" tests share `$03FF` (the ROM excludes them
    /// from the result table).
    pub result_addr: u16,
}

/// Authoritative TSV embedded at compile time.
const RAW_TSV: &str = include_str!("../../../tests/roms/AccuracyCoin/SOURCE_CATALOG.tsv");

static CATALOG_CELL: OnceLock<Vec<CatalogEntry>> = OnceLock::new();

/// Per-test decoded status (the conceptual analogue of [`CellStatus`]
/// from [`super::accuracy_coin`] but keyed by test, not by grid cell).
///
/// [`CellStatus`]: super::accuracy_coin::CellStatus
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum TestStatus {
    /// Result byte was `$00` — test never wrote a result.
    NotRun,
    /// Result byte was `$01` — clean pass with no overlay.
    Pass,
    /// Result byte was `(N<<2)|$01` for `N != 0` — pass with the
    /// stored sprite-overlay code (the orange "partial" colour on the
    /// upstream summary screen).
    PassWithCode(u8),
    /// Result byte was `(N<<2)|$02` — failure with stored error code.
    Fail(u8),
    /// Result byte was `$FF` — test was skipped.
    Skipped,
    /// Any other value — unknown encoding.
    Unknown(u8),
}

impl TestStatus {
    /// Decode a single result byte per the encoding documented in the
    /// module rustdoc.
    #[must_use]
    pub const fn from_byte(b: u8) -> Self {
        match b {
            0x00 => Self::NotRun,
            0x01 => Self::Pass,
            0xFF => Self::Skipped,
            _ => {
                let code = b >> 2;
                if b & 0x01 != 0 {
                    Self::PassWithCode(code)
                } else if b & 0x02 != 0 {
                    Self::Fail(code)
                } else {
                    Self::Unknown(b)
                }
            }
        }
    }

    /// `true` for `Pass` and `PassWithCode` — the ROM's "this counts
    /// toward `PostAllPassTally`" rule, derived from
    /// `AND #$01 BEQ AREROM_PrintFail`.
    #[must_use]
    pub const fn is_pass(self) -> bool {
        matches!(self, Self::Pass | Self::PassWithCode(_))
    }

    /// `true` for `Fail` and `Unknown` — anything that wasn't a pass,
    /// excluding `NotRun` and `Skipped` (which the ROM excludes from
    /// the denominator).
    #[must_use]
    pub const fn is_fail(self) -> bool {
        matches!(self, Self::Fail(_) | Self::Unknown(_))
    }
}

/// Return the catalog of all 146 AccuracyCoin tests, in `TableTable`
/// order.
///
/// The result is built once (on first call) and cached for the
/// lifetime of the process.
#[must_use]
pub fn catalog() -> &'static [CatalogEntry] {
    CATALOG_CELL
        .get_or_init(|| {
            RAW_TSV
                .lines()
                .map(str::trim_end)
                .filter(|line| !line.is_empty())
                .map(|line| {
                    let mut parts = line.splitn(3, '\t');
                    let suite = parts.next().expect("non-empty TSV row");
                    let name = parts.next().unwrap_or_else(|| {
                        panic!("AccuracyCoin TSV row missing TAB-separated name: {line:?}")
                    });
                    let addr_str = parts.next().unwrap_or_else(|| {
                        panic!("AccuracyCoin TSV row missing TAB-separated addr: {line:?}")
                    });
                    let addr_hex = addr_str
                        .strip_prefix("0x")
                        .or_else(|| addr_str.strip_prefix("0X"))
                        .unwrap_or_else(|| {
                            panic!("AccuracyCoin TSV addr lacks 0x prefix: {addr_str:?}")
                        });
                    let result_addr = u16::from_str_radix(addr_hex, 16).unwrap_or_else(|e| {
                        panic!("AccuracyCoin TSV addr parse: {addr_str:?}: {e}")
                    });
                    CatalogEntry {
                        suite: suite.to_owned(),
                        name: name.to_owned(),
                        result_addr,
                    }
                })
                .collect()
        })
        .as_slice()
}

/// Look up a catalog entry by zero-based `TableTable` index.
///
/// Returns `None` if `index >= 146`.
#[must_use]
pub fn entry(index: usize) -> Option<&'static CatalogEntry> {
    catalog().get(index)
}

/// Return the alphabetically-sorted list of unique suite names that
/// appear in the catalog.
#[must_use]
pub fn suites() -> Vec<&'static str> {
    let mut out: Vec<&'static str> = catalog().iter().map(|e| e.suite.as_str()).collect();
    out.sort_unstable();
    out.dedup();
    out
}

/// Count the entries belonging to a named suite.
#[must_use]
pub fn suite_size(suite: &str) -> usize {
    catalog().iter().filter(|e| e.suite == suite).count()
}

/// Decode the 146-entry result vector by reading each catalog entry's
/// [`CatalogEntry::result_addr`] from `ram` (which must be the NES's
/// 2 KiB CPU RAM borrowed via `Nes::bus().ram_bytes()`).
///
/// Returns `None` if `ram` is shorter than the largest result address
/// (i.e. not the full CPU RAM).
#[must_use]
pub fn decode_results(ram: &[u8]) -> Option<Vec<TestStatus>> {
    let max = catalog().iter().map(|e| e.result_addr as usize).max()?;
    if ram.len() <= max {
        return None;
    }
    Some(
        catalog()
            .iter()
            .map(|e| TestStatus::from_byte(ram[e.result_addr as usize]))
            .collect(),
    )
}

/// Aggregated counts derived from a decoded results vector.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct RamResultSummary {
    /// Total number of catalog entries (always 146 if the catalog is
    /// fully loaded).
    pub total: u32,
    /// Tests that wrote `$01` (clean pass).
    pub pass: u32,
    /// Tests that wrote `(N<<2)|1`, `N != 0` (pass with overlay code).
    pub pass_with_code: u32,
    /// Tests that wrote `(N<<2)|2` (failure with error code).
    pub fail: u32,
    /// Tests that wrote `$FF` (skipped by the ROM).
    pub skipped: u32,
    /// Tests that wrote `$00` (never executed — includes the five
    /// `Power On State` visual-inspection tests that share `$03FF`).
    pub not_run: u32,
    /// Tests that wrote a byte with neither bit 0 nor bit 1 set,
    /// excluding `$00` and `$FF` (corrupted / unexpected encoding).
    pub unknown: u32,
}

impl RamResultSummary {
    /// Headline pass rate: `(pass + pass_with_code) / (pass +
    /// pass_with_code + fail + unknown)`. Excludes `not_run` and
    /// `skipped` from the denominator, matching the ROM's
    /// `PostAllPassTally / PostAllTestTally` ratio (see
    /// `AccuracyCoin.asm` line 1042-1047).
    #[must_use]
    pub fn pass_rate(&self) -> f64 {
        let num = self.pass + self.pass_with_code;
        let denom = num + self.fail + self.unknown;
        if denom == 0 {
            0.0
        } else {
            f64::from(num) / f64::from(denom)
        }
    }

    /// Total tests with a definite pass/fail verdict (the denominator
    /// of [`Self::pass_rate`]).
    #[must_use]
    pub const fn assigned(&self) -> u32 {
        self.pass + self.pass_with_code + self.fail + self.unknown
    }
}

/// Roll a decoded results vector into bucket counts.
#[must_use]
pub fn summarise(statuses: &[TestStatus]) -> RamResultSummary {
    let mut s = RamResultSummary {
        total: u32::try_from(statuses.len()).unwrap_or(u32::MAX),
        ..RamResultSummary::default()
    };
    for status in statuses {
        match status {
            TestStatus::Pass => s.pass += 1,
            TestStatus::PassWithCode(_) => s.pass_with_code += 1,
            TestStatus::Fail(_) => s.fail += 1,
            TestStatus::Skipped => s.skipped += 1,
            TestStatus::NotRun => s.not_run += 1,
            TestStatus::Unknown(_) => s.unknown += 1,
        }
    }
    s
}

/// Pretty-print the list of failing tests (and unknown-encoding tests)
/// for diagnostic output. Each line: `<suite> :: <name> [error N]`.
#[must_use]
pub fn failing_tests(statuses: &[TestStatus]) -> Vec<String> {
    catalog()
        .iter()
        .zip(statuses.iter())
        .filter_map(|(entry, status)| match *status {
            TestStatus::Fail(code) => {
                Some(format!("{} :: {} [error {code}]", entry.suite, entry.name))
            }
            TestStatus::Unknown(b) => Some(format!(
                "{} :: {} [unknown encoding 0x{b:02X}]",
                entry.suite, entry.name
            )),
            _ => None,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_has_exactly_146_entries() {
        assert_eq!(catalog().len(), 146, "AccuracyCoin catalog size drifted");
    }

    #[test]
    fn catalog_addresses_lie_within_cpu_ram() {
        for e in catalog() {
            assert!(
                e.result_addr < 0x0800,
                "result_addr 0x{:04X} for {:?} outside CPU RAM",
                e.result_addr,
                e.name
            );
        }
    }

    #[test]
    fn catalog_contains_known_test_names() {
        let names: Vec<&str> = catalog().iter().map(|e| e.name.as_str()).collect();
        assert!(names.contains(&"ROM is not writable"));
        assert!(names.contains(&"PC Wraparound"));
        assert!(names.contains(&"NMI Overlap BRK"));
        assert!(names.contains(&"NMI Overlap IRQ"));
        assert!(names.contains(&"$03   SLO indirect,X"));
        assert!(names.contains(&"Internal Data Bus"));
        assert!(names.contains(&"$2007 Stress Test"));
    }

    #[test]
    fn catalog_has_20_suites() {
        let expected = [
            "CPU Behavior",
            "Addressing mode wraparound",
            "Unofficial Instructions: SLO",
            "Unofficial Instructions: RLA",
            "Unofficial Instructions: SRE",
            "Unofficial Instructions: RRA",
            "Unofficial Instructions: *AX",
            "Unofficial Instructions: DCP",
            "Unofficial Instructions: ISC",
            "Unofficial Instructions: SH*",
            "Unofficial Immediates",
            "CPU Interrupts",
            "APU Registers and DMA tests",
            "APU Tests",
            "Power On State",
            "PPU Behavior",
            "PPU VBlank Timing",
            "Sprite Evaluation",
            "PPU Misc.",
            "CPU Behavior 2",
        ];
        for suite in expected {
            assert!(
                suite_size(suite) > 0,
                "AccuracyCoin suite missing entries: {suite:?}"
            );
        }
        assert_eq!(suites().len(), 20);
    }

    #[test]
    fn entry_by_index_is_zero_based() {
        let first = entry(0).expect("index 0 present");
        assert_eq!(first.name, "ROM is not writable");
        let last = entry(145).expect("index 145 present");
        assert_eq!(last.name, "Internal Data Bus");
        assert!(entry(146).is_none());
    }

    #[test]
    fn test_status_decodes_known_bytes() {
        assert_eq!(TestStatus::from_byte(0x00), TestStatus::NotRun);
        assert_eq!(TestStatus::from_byte(0x01), TestStatus::Pass);
        assert_eq!(TestStatus::from_byte(0xFF), TestStatus::Skipped);
        // "Pass with code 4" — 4 << 2 | 1 = 17 (0x11). The ROM's
        // `LDA #17 ; Pass, "code 4"` line at AccuracyCoin.asm:1333.
        assert_eq!(TestStatus::from_byte(17), TestStatus::PassWithCode(4));
        // Fail with code 5 — 5 << 2 | 2 = 22 (0x16).
        assert_eq!(TestStatus::from_byte(22), TestStatus::Fail(5));
        // Bit 0 wins over bit 1 if both are set.
        assert_eq!(TestStatus::from_byte(0x03), TestStatus::PassWithCode(0));
        // No bits set, non-$00, non-$FF — unknown.
        assert_eq!(TestStatus::from_byte(0x04), TestStatus::Unknown(0x04));
    }

    #[test]
    fn decode_results_reads_each_addr() {
        let mut ram = vec![0u8; 2048];
        // Mark the first three catalog entries: pass / fail(3) / not-run.
        let e0 = entry(0).unwrap();
        let e1 = entry(1).unwrap();
        ram[e0.result_addr as usize] = 0x01;
        ram[e1.result_addr as usize] = (3 << 2) | 0x02; // fail code 3
        let statuses = decode_results(&ram).expect("decode");
        assert_eq!(statuses.len(), 146);
        assert_eq!(statuses[0], TestStatus::Pass);
        assert_eq!(statuses[1], TestStatus::Fail(3));
        // The five Power On State tests share $03FF (left at 0x00).
        for (i, e) in catalog().iter().enumerate() {
            if e.suite == "Power On State" {
                assert_eq!(statuses[i], TestStatus::NotRun);
            }
        }
    }

    #[test]
    fn summarise_excludes_not_run_and_skipped() {
        let statuses = [
            TestStatus::Pass,
            TestStatus::Pass,
            TestStatus::PassWithCode(2),
            TestStatus::Fail(7),
            TestStatus::NotRun,
            TestStatus::Skipped,
            TestStatus::Unknown(0x10),
        ];
        let s = summarise(&statuses);
        // 2 pass + 1 pwc + 1 fail + 1 unknown = 5 assigned
        assert_eq!(s.assigned(), 5);
        // (2 + 1) / 5 = 0.60
        assert!((s.pass_rate() - 0.60).abs() < 1e-9);
    }
}
