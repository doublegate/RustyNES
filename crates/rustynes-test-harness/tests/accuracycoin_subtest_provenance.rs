//! The gate on `tests/roms/AccuracyCoin/sub-tests/BUILD-PROVENANCE.tsv`.
//!
//! The manifest records, for each sub-test ROM, which catalog entry it runs —
//! a fact that was recovered by MEASUREMENT on 2026-09-19 after four releases
//! in which 31 of the 33 rows were empty and the filenames were the only
//! claim available. This test re-measures every row.
//!
//! ## Why the manifest needs a gate and not just a manifest
//!
//! The identity of a sub-test ROM cannot be read off its name, its filename
//! slug, or its encoded `(suite, test)` immediates — the reasoning is on
//! [`rustynes_test_harness::accuracy_coin_subtest`] and in the manifest's own
//! header. It can only be observed. An observation written into a file becomes
//! an assertion the moment it is committed, and an assertion nobody re-derives
//! is exactly the failure this project keeps recording. So the file is treated
//! as a prediction and the ROMs are asked again.
//!
//! What that catches, concretely: a ROM rebuilt against a newer upstream whose
//! suites moved (the index changes, the entry may not), a ROM swapped for
//! another by mistake, a ROM renamed to a name it does not run, and a catalog
//! re-sync that moves a result address.
//!
//! ## Fail-closed
//!
//! Zero rows examined is a FAILURE, not a pass. A filter matching nothing, a
//! moved directory and a clean sweep are otherwise the same output — which
//! this repository has been bitten by often enough to have a standing rule
//! about it.

#![cfg(feature = "test-roms")]

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use rustynes_test_harness::accuracy_coin_catalog as cat;
use rustynes_test_harness::accuracy_coin_subtest as sub;

/// Budget per ROM. The slowest in the corpus settles on frame 395; 900 leaves
/// room for one that is slower without letting a stuck ROM run unboundedly.
const FRAMES: u64 = 900;

fn dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join("tests/roms/AccuracyCoin/sub-tests")
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct Row {
    enc_suite: i32,
    enc_test: i32,
    result_addr: u16,
    entry: String,
    verdict: String,
    settles_frame: u64,
    upstream_commit: String,
}

fn parse_manifest(text: &str) -> BTreeMap<String, Row> {
    let mut out = BTreeMap::new();
    for line in text.lines() {
        if line.trim_start().starts_with('#') || line.trim().is_empty() {
            continue;
        }
        let f: Vec<&str> = line.split('\t').collect();
        assert!(
            f.len() == sub::MANIFEST_COLUMNS.len(),
            "malformed manifest row (expected {} tab-separated fields, got {}): {line:?}",
            sub::MANIFEST_COLUMNS.len(),
            f.len()
        );
        let addr = u16::from_str_radix(f[3].trim_start_matches("0x"), 16)
            .unwrap_or_else(|e| panic!("row {:?}: bad result_addr {:?}: {e}", f[0], f[3]));
        let prev = out.insert(
            f[0].to_string(),
            Row {
                enc_suite: f[1].parse().expect("enc_suite"),
                enc_test: f[2].parse().expect("enc_test"),
                result_addr: addr,
                entry: f[4].to_string(),
                verdict: f[5].to_string(),
                settles_frame: f[6].parse().expect("settles_frame"),
                upstream_commit: f[7].to_string(),
            },
        );
        assert!(prev.is_none(), "duplicate manifest row for {:?}", f[0]);
    }
    out
}

/// The committed file must carry the generator's schema line.
///
/// Without this the two can drift silently: `subtest_identify --tsv` would keep
/// emitting one order while `parse_manifest` read another, and the only symptom
/// would be that the command this repository documents for regenerating the
/// file produces something the gate rejects. That is exactly what happened once
/// — nine fields out, eight expected, `upstream_commit` missing — and nothing
/// caught it, because the manifest had been assembled by hand.
#[test]
fn the_manifest_carries_the_generators_schema() {
    let text = std::fs::read_to_string(dir().join("BUILD-PROVENANCE.tsv")).expect("read manifest");
    let want = format!("# {}", sub::MANIFEST_COLUMNS.join("\t"));
    assert!(
        text.lines().any(|l| l == want),
        "BUILD-PROVENANCE.tsv does not carry the schema line {want:?}.\n           Regenerate it with `subtest_identify --tsv`, which prints it."
    );
}

#[test]
fn manifest_covers_exactly_the_roms_on_disk() {
    let d = dir();
    let text = std::fs::read_to_string(d.join("BUILD-PROVENANCE.tsv")).expect("read manifest");
    let rows = parse_manifest(&text);

    let mut on_disk: Vec<String> = std::fs::read_dir(&d)
        .expect("read sub-tests dir")
        .filter_map(Result::ok)
        .filter(|e| e.path().extension().is_some_and(|x| x == "nes"))
        .filter_map(|e| {
            e.path()
                .file_stem()
                .map(|s| s.to_string_lossy().into_owned())
        })
        .collect();
    on_disk.sort();

    assert!(
        !on_disk.is_empty(),
        "no .nes files under {} -- a corpus that cannot be found must FAIL, \
         not report a clean manifest",
        d.display()
    );

    let on_disk_set: std::collections::HashSet<&String> = on_disk.iter().collect();
    let missing: Vec<&String> = on_disk.iter().filter(|r| !rows.contains_key(*r)).collect();
    let orphan: Vec<&String> = rows.keys().filter(|r| !on_disk_set.contains(r)).collect();
    assert!(
        missing.is_empty() && orphan.is_empty(),
        "BUILD-PROVENANCE.tsv is out of step with the corpus.\n  \
         ROMs with no row: {missing:?}\n  rows with no ROM: {orphan:?}"
    );
}

#[test]
fn every_rom_still_runs_the_entry_its_row_records() {
    let d = dir();
    let text = std::fs::read_to_string(d.join("BUILD-PROVENANCE.tsv")).expect("read manifest");
    let rows = parse_manifest(&text);
    let by_addr = sub::scored_by_addr().expect("catalog addresses are unique");

    let mut examined = 0usize;
    let mut problems: Vec<String> = Vec::new();

    for (stem, row) in &rows {
        let path = d.join(format!("{stem}.nes"));
        let id = match sub::identify_path(&path, FRAMES) {
            Ok(i) => i,
            Err(e) => {
                problems.push(format!("{stem}: {e}"));
                continue;
            }
        };
        examined += 1;

        // The encoded immediates are a fingerprint of the FILE. They carry no
        // claim about which test they name -- that is the manifest's whole
        // subject -- but a change in them means the bytes changed.
        let (es, et) = id
            .encoded
            .map_or((-1, -1), |(s, t)| (i32::from(s), i32::from(t)));
        if (es, et) != (row.enc_suite, row.enc_test) {
            problems.push(format!(
                "{stem}: encoded indices are {es}/{et}, manifest records {}/{}",
                row.enc_suite, row.enc_test
            ));
        }

        let Some(first) = id.primary() else {
            problems.push(format!(
                "{stem}: wrote NOTHING in $0400-$04FF in {FRAMES} frames"
            ));
            continue;
        };
        if first.addr != row.result_addr {
            problems.push(format!(
                "{stem}: writes ${:04X} ({}), manifest records ${:04X} ({})",
                first.addr,
                by_addr
                    .get(&first.addr)
                    .copied()
                    .unwrap_or("no scored catalog entry"),
                row.result_addr,
                row.entry
            ));
            continue;
        }
        // An address in NO scored catalog entry is a problem in its own right,
        // not a row to compare against a placeholder. Accepting a sentinel on
        // both sides would let a wrong ROM and a wrong row agree because they
        // share the placeholder rather than because anything was checked --
        // which is why the generator now refuses to emit such a row at all.
        // Raised by CodeRabbit.
        let Some(name) = by_addr.get(&first.addr).copied() else {
            problems.push(format!(
                "{stem}: writes ${:04X}, which belongs to no scored catalog entry",
                first.addr
            ));
            continue;
        };
        if name != row.entry {
            problems.push(format!(
                "{stem}: ${:04X} is {name:?} in the catalog, manifest says {:?} \
                 -- a catalog re-sync moved it",
                first.addr, row.entry
            ));
        }
        let verdict = format!("{:?}", cat::TestStatus::from_byte(first.final_byte));
        if verdict != row.verdict {
            problems.push(format!(
                "{stem}: oracle verdict is {verdict}, manifest records {}",
                row.verdict
            ));
        }
        if first.first_frame != row.settles_frame {
            problems.push(format!(
                "{stem}: settles on frame {}, manifest records {}",
                first.first_frame, row.settles_frame
            ));
        }
    }

    assert!(
        examined > 0,
        "examined ZERO ROMs -- a run that looked at nothing must fail, not pass"
    );
    assert!(
        problems.is_empty(),
        "{} of {examined} sub-test ROM(s) disagree with BUILD-PROVENANCE.tsv:\n  {}",
        problems.len(),
        problems.join("\n  ")
    );
}

/// Two ROMs may not claim the same catalog entry without the manifest saying
/// so in as many words.
///
/// This is not tidiness. `ppu-misc-2004-stress.nes` does not run `$2004 Stress
/// Test`; it runs `$2007 Stress Test`, the same entry as
/// `ppu-misc-2007-stress.nes` — so the corpus looks like it covers an entry it
/// does not cover. The pair is allow-listed here BY NAME, with the effect
/// stated, so that a *new* collision fails instead of joining a category that
/// already has members.
#[test]
fn entry_collisions_are_the_two_that_are_documented() {
    let d = dir();
    let text = std::fs::read_to_string(d.join("BUILD-PROVENANCE.tsv")).expect("read manifest");
    let rows = parse_manifest(&text);

    let known: &[(&str, &str)] = &[
        // Misnamed since before the manifest existed; `$2004 Stress Test`
        // ($048C) consequently has NO sub-test ROM.
        ("ppu-misc-2004-stress", "ppu-misc-2007-stress"),
        // A rebuild, not a variant: same encoded index, address, verdict and
        // settling frame.
        ("implied-dummy-reads", "implied-dummy-reads-v2"),
    ];

    let mut by_entry: BTreeMap<&str, Vec<&str>> = BTreeMap::new();
    for (stem, row) in &rows {
        by_entry.entry(&row.entry).or_default().push(stem);
    }
    let mut unexpected = Vec::new();
    for (entry, stems) in &by_entry {
        if stems.len() < 2 {
            continue;
        }
        let matched = known
            .iter()
            .any(|(a, b)| stems.len() == 2 && stems.contains(a) && stems.contains(b));
        if !matched {
            unexpected.push(format!("{entry:?} is claimed by {stems:?}"));
        }
    }
    assert!(
        unexpected.is_empty(),
        "undocumented sub-test collision(s) -- a corpus that looks broader than \
         it is:\n  {}",
        unexpected.join("\n  ")
    );

    // And the allow-list may not rot: each documented pair must still BE a
    // collision. One that stops colliding means a ROM was rebuilt or renamed
    // and the note here is now describing nothing.
    for (a, b) in known {
        let ra = rows
            .get(*a)
            .unwrap_or_else(|| panic!("{a} missing from manifest"));
        let rb = rows
            .get(*b)
            .unwrap_or_else(|| panic!("{b} missing from manifest"));
        assert_eq!(
            ra.entry, rb.entry,
            "{a} and {b} no longer share an entry; remove them from the \
             allow-list rather than leaving a note that describes nothing"
        );
    }
}
