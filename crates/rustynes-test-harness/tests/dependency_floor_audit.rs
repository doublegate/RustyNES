// SPDX-License-Identifier: GPL-3.0-or-later
//! Version floors that an advisory ignore depends on (v3.4.2).
//!
//! `deny.toml` and `.cargo/audit.toml` ignore some advisories by ID because
//! the affected crate is present only as a version the advisory names but
//! whose content is fixed. Neither tool can scope such an ignore to one
//! version (cargo-deny's `crate = "name@version"` form applies to yanked
//! crates only), so the ignore also mutes the advisory for every other
//! version of the crate. This test is the guard the ignore needs: it reads
//! `Cargo.lock` and fails if a resolve ever picks a version below the fix.
//!
//! * **RUSTSEC-2026-0381** (`webpki-roots`, distrusted emSign roots, fixed in
//!   1.0.10). The lock carries `webpki-roots` 0.26.11, a semver-trick shim
//!   whose whole library is `pub use parent::TLS_SERVER_ROOTS` with `parent`
//!   being `webpki-roots` 1.x, so the roots any build trusts are the 1.x
//!   ones. They must be 1.0.10 or later.

use std::path::PathBuf;

/// Every `version` that the lockfile `text` records for package `name`.
fn locked_versions(text: &str, name: &str) -> Vec<String> {
    let header = format!("name = \"{name}\"");
    let mut out = Vec::new();
    let mut lines = text.lines();
    while let Some(line) = lines.next() {
        if line == header
            && let Some(v) = lines.next().and_then(|l| l.strip_prefix("version = \""))
        {
            out.push(v.trim_end_matches('"').to_owned());
        }
    }
    out
}

/// `(major, minor, patch, release)` for ordering. `release` is 0 for a
/// pre-release (`1.0.10-alpha.1`) and 1 otherwise, so a pre-release sorts
/// BELOW its release, as semantic versioning orders them; build metadata
/// (`+...`) is ignored, as semantic versioning ignores it. A segment that is not a number is a
/// malformed lockfile and panics rather than reading as 0.
fn triple(v: &str) -> (u64, u64, u64, u8) {
    let no_build = v.split('+').next().unwrap_or(v);
    let (core, release) = match no_build.split_once('-') {
        Some((core, _)) => (core, 0),
        None => (no_build, 1),
    };
    let parts: Vec<u64> = core
        .split('.')
        .map(|x| {
            x.parse::<u64>()
                .unwrap_or_else(|_| panic!("malformed version {v:?} in Cargo.lock"))
        })
        .collect();
    assert_eq!(parts.len(), 3, "version {v:?} is not major.minor.patch");
    (parts[0], parts[1], parts[2], release)
}

/// The RUSTSEC-2026-0381 floor, applied to lockfile `text`.
fn webpki_roots_floor(text: &str) -> Result<(), String> {
    let versions = locked_versions(text, "webpki-roots");
    if versions.is_empty() {
        return Err("Cargo.lock has no webpki-roots: if it was dropped, remove \
                    RUSTSEC-2026-0381 from deny.toml and .cargo/audit.toml along with this test"
            .into());
    }
    if !versions.iter().any(|v| v.starts_with("1.")) {
        return Err(format!(
            "no webpki-roots 1.x in Cargo.lock ({versions:?}): the 0.26 shim re-exports 1.x, \
             so the shim changed; re-check RUSTSEC-2026-0381 before keeping the ignore"
        ));
    }
    for v in &versions {
        if v.starts_with("1.") {
            if triple(v) < (1, 0, 10, 1) {
                return Err(format!(
                    "webpki-roots {v} is below 1.0.10, the fix for RUSTSEC-2026-0381, which \
                     deny.toml and .cargo/audit.toml ignore by ID; run cargo update -p webpki-roots"
                ));
            }
        } else if v != "0.26.11" {
            return Err(format!(
                "webpki-roots {v} is neither 1.x nor the 0.26.11 shim the ignore was checked against"
            ));
        }
    }
    Ok(())
}

fn lock(rel: &str) -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .join(rel);
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
        .replace("\r\n", "\n")
}

#[test]
fn webpki_roots_1x_is_at_least_the_fixed_release() {
    if let Err(e) = webpki_roots_floor(&lock("Cargo.lock")) {
        panic!("{e}");
    }
}

/// `crates/rustynes-cosim` is outside the workspace and has its own lock,
/// which the floor above does not read. It carries no TLS stack today; if it
/// ever gains `webpki-roots`, that lock needs the same check.
#[test]
fn the_cosim_lock_has_no_webpki_roots() {
    let versions = locked_versions(&lock("crates/rustynes-cosim/Cargo.lock"), "webpki-roots");
    assert!(
        versions.is_empty(),
        "crates/rustynes-cosim/Cargo.lock now has webpki-roots {versions:?}; extend the \
         RUSTSEC-2026-0381 floor to it"
    );
}

/// The check must be able to fail. A doctored `Cargo.lock` cannot be used
/// for that (cargo refuses to build with a changed checksum), so the
/// mutants are lockfile fragments.
#[test]
fn the_floor_refuses_what_it_exists_to_refuse() {
    let lock = |a: &str, b: &str| {
        format!(
            "[[package]]\nname = \"webpki-roots\"\nversion = \"{a}\"\n\n[[package]]\nname = \"webpki-roots\"\nversion = \"{b}\"\n"
        )
    };
    assert!(webpki_roots_floor(&lock("0.26.11", "1.0.10")).is_ok());
    assert!(webpki_roots_floor(&lock("0.26.11", "1.1.0")).is_ok());
    assert!(
        webpki_roots_floor(&lock("0.26.11", "1.0.9")).is_err(),
        "1.0.9 is the vulnerable root set"
    );
    assert!(
        webpki_roots_floor(&lock("0.26.11", "1.0.10-rc.1")).is_err(),
        "a 1.0.10 pre-release is below 1.0.10"
    );
    assert!(
        webpki_roots_floor(&lock("0.26.10", "1.0.10")).is_err(),
        "an unchecked shim version"
    );
    assert!(
        webpki_roots_floor("[[package]]\nname = \"webpki-roots\"\nversion = \"0.26.11\"\n")
            .is_err()
    );
    assert!(webpki_roots_floor("").is_err());
}

#[test]
fn the_version_compare_orders_releases_numerically() {
    assert!(triple("1.0.10") > triple("1.0.9"));
    assert!(triple("1.1.0") > triple("1.0.99"));
    assert!(triple("0.26.11") < (1, 0, 10, 1));
    // A pre-release sorts below its release, and build metadata is ignored.
    assert!(triple("1.0.10-alpha.1") < (1, 0, 10, 1));
    assert!(triple("1.0.10+build.5") >= (1, 0, 10, 1));
}

#[test]
#[should_panic(expected = "malformed version")]
fn a_malformed_version_is_refused_not_read_as_zero() {
    let _ = triple("1.x.10");
}
