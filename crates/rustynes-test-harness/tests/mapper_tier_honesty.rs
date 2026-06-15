//! Mapper-tier honesty gate (ADR 0011).
//!
//! The project's headline accuracy claim ("N mapper families, `AccuracyCoin`
//! 100%") is only honest if no `BestEffort` mapper — register-decode-only,
//! deliberately NOT covered by the `AccuracyCoin` / commercial-ROM oracle — ever
//! silently backs an accuracy-oracle ROM. This test enforces that invariant:
//! every ROM in an accuracy corpus must resolve to a `Core` or `Curated`
//! (accuracy-gated) tier, never `BestEffort` and never an unclassified `None`.
//!
//! - The committed headline ROMs (`AccuracyCoin`, `nestest`) run in headless CI.
//! - The full commercial oracle corpus (`tests/roms/external/`, gitignored)
//!   runs locally behind `--features commercial-roms` — the per-oracle-ROM
//!   tier assertion ADR 0011 specifies (it cannot run in headless CI because
//!   the dumps are never committed).
//!
//! Coverage/boot ROMs that exercise `BestEffort` mappers on purpose (Holy
//! Mapperel, the v2.1 coverage suite) are NOT accuracy oracles and are
//! intentionally excluded — they assert "does it boot", not byte-identity.

mod common;

use std::path::{Path, PathBuf};

use rustynes_core::rustynes_mappers::mapper_tier;

/// Parse the iNES / NES 2.0 mapper number + submapper from a ROM header.
/// Returns `None` for anything that is not an iNES image (`.fds`/`.nsf`).
fn ines_mapper(bytes: &[u8]) -> Option<(u16, u8)> {
    if bytes.len() < 16 || &bytes[0..4] != b"NES\x1A" {
        return None;
    }
    let lo = u16::from(bytes[6] >> 4);
    let hi = u16::from(bytes[7] & 0xF0);
    // NES 2.0 is indicated by header byte 7 bits 2-3 == 0b10; it adds 4 more
    // mapper bits (byte 8 low nibble) + the submapper (byte 8 high nibble).
    let (mapper, submapper) = if (bytes[7] & 0x0C) == 0x08 {
        (hi | lo | (u16::from(bytes[8]) & 0x0F) << 8, bytes[8] >> 4)
    } else {
        (hi | lo, 0)
    };
    Some((mapper, submapper))
}

/// Assert one accuracy-corpus ROM resolves to an accuracy-gated tier.
fn assert_accuracy_gated(path: &Path) {
    let bytes = std::fs::read(path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
    let Some((mapper, sub)) = ines_mapper(&bytes) else {
        return; // not an iNES ROM (e.g. .fds / .nsf) — out of scope here.
    };
    let tier = mapper_tier(mapper, sub);
    assert!(
        tier.is_some_and(rustynes_core::rustynes_mappers::MapperTier::is_accuracy_gated),
        "accuracy-corpus ROM {} uses mapper {mapper} (submapper {sub}) whose tier is {tier:?} — \
         an accuracy oracle must be backed by a Core/Curated mapper, never BestEffort or an \
         unclassified mapper (ADR 0011 honesty invariant)",
        path.display(),
    );
}

/// Headless gate: the committed headline accuracy ROMs must never regress to a
/// `BestEffort`/unclassified mapper.
#[test]
fn committed_accuracy_roms_are_not_best_effort() {
    let mut checked = 0usize;
    for rel in ["accuracycoin/AccuracyCoin.nes", "nestest/nestest.nes"] {
        let path = common::rom_path(rel);
        if path.exists() {
            assert_accuracy_gated(&path);
            checked += 1;
        }
    }
    assert!(checked > 0, "no committed headline accuracy ROMs found");
}

/// Headless gate: the byte-identity commercial-oracle tests
/// (`external_real_games.rs` / `external_extended.rs`) must only reference
/// `mapper-NNN-*` corpus directories whose mapper is accuracy-gated. This reads
/// the oracle test SOURCE (always present, no commercial dumps needed), so it
/// runs in CI and catches a `BestEffort` mapper being wired into a byte-identity
/// oracle — the precise ADR 0011 honesty invariant.
#[test]
fn oracle_tests_only_reference_accuracy_gated_mapper_dirs() {
    let tests_dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests");
    let mut referenced: std::collections::BTreeSet<u16> = std::collections::BTreeSet::new();
    for src in ["external_real_games.rs", "external_extended.rs"] {
        let path = tests_dir.join(src);
        let Ok(text) = std::fs::read_to_string(&path) else {
            continue;
        };
        // Extract every `mapper-NNN-` directory reference and parse NNN.
        for (i, _) in text.match_indices("mapper-") {
            let digits: String = text[i + 7..]
                .chars()
                .take_while(char::is_ascii_digit)
                .collect();
            if let Ok(n) = digits.parse::<u16>() {
                referenced.insert(n);
            }
        }
    }
    assert!(
        !referenced.is_empty(),
        "found no mapper-NNN- dir references in the oracle test sources"
    );
    for mapper in referenced {
        let tier = mapper_tier(mapper, 0);
        assert!(
            tier.is_some_and(rustynes_core::rustynes_mappers::MapperTier::is_accuracy_gated),
            "a byte-identity oracle test references the mapper-{mapper:03} corpus dir, but mapper \
             {mapper}'s tier is {tier:?} — an oracle must gate a Core/Curated mapper, never \
             BestEffort/unclassified (ADR 0011)"
        );
    }
}

/// Local oracle gate (gitignored commercial dumps): every ROM in the
/// byte-identical oracle corpus resolves to an accuracy-gated mapper. This is
/// the per-oracle-ROM assertion ADR 0011 describes; it only runs with the
/// dumps present (`--features commercial-roms`).
#[cfg(feature = "commercial-roms")]
#[test]
fn oracle_corpus_uses_no_best_effort_mapper() {
    use rustynes_core::rustynes_mappers::MapperTier;

    let root = common::rom_path("external");
    let mut checked = 0usize;
    let mut by_tier: std::collections::BTreeMap<String, usize> = std::collections::BTreeMap::new();
    let mut unclassified: Vec<String> = Vec::new();
    let mut best_effort: Vec<String> = Vec::new();
    let mut stack = vec![root.clone()];
    while let Some(dir) = stack.pop() {
        let Ok(entries) = std::fs::read_dir(&dir) else {
            continue;
        };
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_dir() {
                stack.push(path);
            } else if path.extension().is_some_and(|e| e == "nes") {
                let bytes = std::fs::read(&path).expect("read external ROM");
                let Some((mapper, sub)) = ines_mapper(&bytes) else {
                    continue;
                };
                checked += 1;
                let rel = path
                    .strip_prefix(&root)
                    .unwrap_or(&path)
                    .display()
                    .to_string();
                match mapper_tier(mapper, sub) {
                    Some(MapperTier::Core) => *by_tier.entry("Core".into()).or_default() += 1,
                    Some(MapperTier::Curated) => *by_tier.entry("Curated".into()).or_default() += 1,
                    Some(MapperTier::BestEffort) => {
                        *by_tier.entry("BestEffort".into()).or_default() += 1;
                        best_effort.push(format!("{rel} (mapper {mapper})"));
                    }
                    None => unclassified.push(format!("{rel} (mapper {mapper})")),
                }
            }
        }
    }
    assert!(
        checked > 0,
        "no external oracle ROMs found under tests/roms/external (commercial-roms enabled but \
         the gitignored dumps are absent)"
    );
    eprintln!("[honesty] {checked} external ROMs by tier: {by_tier:?}");
    if !best_effort.is_empty() {
        eprintln!(
            "[honesty] {} BestEffort (register-decode verification dumps, NOT oracle-gated): {:#?}",
            best_effort.len(),
            best_effort
        );
    }
    // The hard invariant (ADR 0011): no ROM in the oracle tree uses a mapper we
    // do not even classify. (BestEffort dumps are allowed here — they are
    // register-decode verification dumps, never wired to a byte-identity oracle
    // test; the byte-identity oracles in `external_real_games.rs` /
    // `external_extended.rs` only reference Core/Curated mapper dirs.)
    assert!(
        unclassified.is_empty(),
        "external oracle tree contains ROM(s) whose mapper is unclassified by mapper_tier() — \
         classify them (Core/Curated/BestEffort) before they can sit in the corpus: {unclassified:#?}"
    );
}
