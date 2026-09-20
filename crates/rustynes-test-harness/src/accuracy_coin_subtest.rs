//! Identify an `AccuracyCoin` **sub-test ROM** by measurement, not by name.
//!
//! A sub-test ROM is the whole `AccuracyCoin` ROM with its boot path patched to
//! enter ONE catalog entry directly (`scripts/accuracycoin-build/build_sub_test_rom.py`).
//! The patch is a pair of immediates — `LDY #suite` / `LDX #test` — and those
//! index the `TableTable` **of the assembly embedded in that ROM**, not
//! upstream's table today. Upstream has reordered its suites and inserted tests
//! within them since the oldest of these ROMs was built, so resolving a legacy
//! ROM's encoded index against *any* suite map gives a confident wrong answer;
//! the two available maps disagree with each other and both disagree with the
//! ROM. `tests/roms/AccuracyCoin/sub-tests/BUILD-PROVENANCE.tsv` records the
//! worked example.
//!
//! So this module does not resolve an index. It runs the ROM and reads which
//! result byte it writes. A result address is the catalog's own key and is
//! stable across upstream reordering, which makes it the right identity to
//! record — and it is checkable, which a filename is not.
//!
//! The encoded indices are still extracted, as a **fingerprint of the file**:
//! they pin the bytes so a swapped or silently-rebuilt ROM is caught, while
//! carrying no claim about meaning.

#![cfg(feature = "test-roms")]

use std::path::Path;

use rustynes_core::Nes;

use crate::accuracy_coin_catalog as cat;

/// The manifest's column schema, as one string.
///
/// `subtest_identify --tsv` prints it (behind a `# `, so the file's parser
/// skips it) and `accuracycoin_subtest_provenance.rs` asserts the committed
/// file carries it. Single-sourced because the two drifted once: the generator
/// emitted nine fields in a different order while the parser required these
/// eight, so the command this repository documents for regenerating
/// `BUILD-PROVENANCE.tsv` produced something the gate rejects. A documented
/// command that does not work is worse than no documented command, because it
/// reads as reproducibility.
pub const MANIFEST_COLUMNS: [&str; 8] = [
    "rom",
    "enc_suite",
    "enc_test",
    "result_addr",
    "entry",
    "oracle_verdict",
    "settles_frame",
    "upstream_commit",
];

/// The result window.
///
/// Every scored catalog entry writes inside it. The `$03FF` omit-sentinel
/// ([`cat::RESULT_DRAW_TEST`]) deliberately sits below it, so a
/// Power-On-State ROM comes back unidentifiable rather than attributed to a
/// neighbour.
pub const WINDOW: std::ops::RangeInclusive<usize> = 0x0400..=0x04FF;

/// One address the ROM wrote, and when.
#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct Hit {
    /// The CPU-RAM address written, inside [`WINDOW`].
    pub addr: u16,
    /// The frame on which it first became non-zero.
    pub first_frame: u64,
    /// Its value at the end of the run, decoded by
    /// [`cat::TestStatus::from_byte`].
    pub final_byte: u8,
}

/// What a sub-test ROM turned out to be.
#[derive(Clone, Debug)]
pub struct Identified {
    /// `(suite, test)` as injected by the builder — a fingerprint of the file,
    /// meaningful only against the build embedded in this same ROM.
    pub encoded: Option<(u8, u8)>,
    /// Every address in [`WINDOW`] the ROM wrote, earliest first. The first is
    /// the entry it was built to run; any others are the ROM continuing past
    /// its target.
    pub hits: Vec<Hit>,
}

impl Identified {
    /// The entry the ROM runs, or `None` when it wrote nothing — which is
    /// reported as a refusal rather than as an empty result, because "I could
    /// not look" must not arrive in the same shape as an answer.
    #[must_use]
    pub fn primary(&self) -> Option<Hit> {
        self.hits.first().copied()
    }
}

/// Recover the `(suite, test)` immediates the builder injected.
///
/// The patched call site assembles to `LDY #suite / STY zp / JSR abs / JSR abs
/// / LDX #test / STX zp`, fourteen bytes with wildcards for the two zero-page
/// operands and the two absolute targets.
///
/// Returns `None` when the pattern matches zero times **or more than once**.
/// Two matches would mean the pattern is not the discriminator it is taken
/// for, and choosing one would be a coin toss wearing the shape of a
/// measurement. Measured across the 33-ROM corpus: every ROM matches exactly
/// once.
#[must_use]
pub fn encoded_indices(rom: &[u8]) -> Option<(u8, u8)> {
    let mut found: Option<(u8, u8)> = None;
    for w in rom.windows(14) {
        if w[0] == 0xA0
            && w[2] == 0x84
            && w[4] == 0x20
            && w[7] == 0x20
            && w[10] == 0xA2
            && w[12] == 0x86
        {
            if found.is_some() {
                return None;
            }
            found = Some((w[1], w[11]));
        }
    }
    found
}

/// Boot `rom` and observe which catalog result byte it writes.
///
/// Stops one second after the first byte appears.
///
/// A sub-test ROM reaches its verdict well inside the budget, and running on
/// only risks it wandering into neighbouring entries and muddying the
/// attribution.
///
/// # Errors
///
/// Returns `Err` when the bytes are not a loadable ROM.
pub fn identify(rom: &[u8], max_frames: u64) -> Result<Identified, String> {
    let mut nes = Nes::from_rom(rom).map_err(|e| format!("cannot parse ROM: {e:?}"))?;
    let mut hits: Vec<Hit> = Vec::new();
    for f in 0..max_frames {
        nes.run_frame();
        let ram = nes.bus().ram_bytes();
        for a in WINDOW {
            if ram[a] != 0 && !hits.iter().any(|h| h.addr as usize == a) {
                // `WINDOW` is `0x0400..=0x04FF`, so this cannot truncate --
                // asserted rather than allowed, so that widening the window
                // past `u16` becomes a panic rather than a silent wrap.
                let addr = u16::try_from(a).expect("WINDOW is inside u16");
                hits.push(Hit {
                    addr,
                    first_frame: f,
                    final_byte: 0,
                });
            }
        }
        if let Some(h) = hits.first()
            && f.saturating_sub(h.first_frame) >= 60
        {
            break;
        }
    }
    let ram = nes.bus().ram_bytes();
    for h in &mut hits {
        h.final_byte = ram[h.addr as usize];
    }
    hits.sort_by_key(|h| (h.first_frame, h.addr));
    Ok(Identified {
        encoded: encoded_indices(rom),
        hits,
    })
}

/// Read a ROM off disk and [`identify`] it.
///
/// # Errors
///
/// Returns `Err` when the file cannot be read or is not a loadable ROM.
pub fn identify_path(path: &Path, max_frames: u64) -> Result<Identified, String> {
    let bytes = std::fs::read(path).map_err(|e| format!("{}: {e}", path.display()))?;
    // The path goes on BOTH errors. `identify` cannot know it, so a parse
    // failure used to print a contextless "cannot parse ROM: ..." while the
    // read failure beside it named the file — and the caller loops over a whole
    // corpus, so the one message that matters is which file. Raised by the
    // Antigravity reviewer.
    identify(&bytes, max_frames).map_err(|e| format!("{}: {e}", path.display()))
}

/// Catalog `result_addr` -> entry name, for the scored entries.
///
/// # Errors
///
/// Returns `Err` if two scored entries claim the same address, which would
/// make every attribution through this map ambiguous.
pub fn scored_by_addr() -> Result<std::collections::HashMap<u16, &'static str>, String> {
    let mut m = std::collections::HashMap::new();
    for e in cat::catalog() {
        if !e.is_scored() {
            continue;
        }
        if let Some(prev) = m.insert(e.result_addr, e.name.as_str()) {
            return Err(format!(
                "catalog defect: ${:04X} is claimed by both {prev:?} and {:?}",
                e.result_addr, e.name
            ));
        }
    }
    Ok(m)
}
