//! Pins `rustynes-cosim`'s manifest against the workspace it deliberately left.
//!
//! # Why this exists
//!
//! `crates/rustynes-cosim` is **excluded** from the workspace. It has to be:
//! it enables `cpu-boot-trace` and `irq-timing-trace` on `rustynes-core`, and
//! cargo unifies features across a workspace build, so as a member it made
//! `cargo build --workspace` compile the core ONCE with the union. Measured
//! through `--message-format=json`, that union was
//! `['cpu-boot-trace', 'debug-hooks', 'default', 'hd-pack', 'irq-timing-trace', 'std']`.
//!
//! That is not a performance footnote. `irq-timing-trace` selects a **different**
//! `for sub_dot in 0..3` loop in `Bus::tick_one_cpu_cycle`, so CI's
//! `cargo test --workspace --release --features test-roms` — the accuracy battery
//! — was validating a scheduler no user runs. Same shape as the v2.3.4 defect
//! where the coverage harness tested a load path no user runs. (The cost is
//! +1.2% to +1.9% on `full_frame`, which is *below* this project's 3% bar; the
//! reason to fix it was never the percentage.)
//!
//! # What exclusion costs, and why that cost is pinned here
//!
//! An excluded package cannot use `field.workspace = true`, so its version,
//! edition, rust-version, license and lint tables are **duplicated**. Duplication
//! that nothing checks is duplication that drifts — and a crate quietly holding
//! itself to weaker lints than the rest of the project is exactly the kind of
//! erosion nobody notices until it matters.
//!
//! So every duplicated value is asserted equal to the workspace's here. The
//! exclusion is asserted too: re-adding the crate to `members` fails this test
//! rather than silently reintroducing the unified build.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    // `CARGO_MANIFEST_DIR` is `crates/rustynes-test-harness`.
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .parent()
        .and_then(Path::parent)
        .expect("repo root is two levels above this crate")
        .to_path_buf()
}

fn read(rel: &str) -> String {
    let p = repo_root().join(rel);
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// A `key = value` line from the given table, with comments stripped.
///
/// Deliberately not a TOML parser: the workspace root already avoids one in
/// `release_anchor_audit.rs`, for the same reason — the values under test are
/// simple, and a dependency whose job is to check a manifest is a strange thing
/// to add to a manifest.
fn field_in_table(text: &str, table: &str, key: &str) -> Option<String> {
    let mut in_table = false;
    for raw in text.lines() {
        let line = raw.split('#').next().unwrap_or("").trim();
        if line.starts_with('[') {
            in_table = line == table;
            continue;
        }
        if !in_table {
            continue;
        }
        if let Some(rest) = line.strip_prefix(key)
            && let Some(v) = rest.trim_start().strip_prefix('=')
        {
            return Some(v.trim().trim_matches('"').to_owned());
        }
    }
    None
}

/// Every value the excluded crate has to spell out must still match the
/// workspace's. Drift here is silent by construction, which is what makes it
/// worth a test.
#[test]
fn the_excluded_crate_still_matches_the_workspace_package_fields() {
    let root = read("Cargo.toml");
    let cosim = read("crates/rustynes-cosim/Cargo.toml");

    for key in [
        "version",
        "edition",
        "rust-version",
        "license",
        "repository",
    ] {
        let want = field_in_table(&root, "[workspace.package]", key)
            .unwrap_or_else(|| panic!("workspace.package has no `{key}` — did the table move?"));
        let got = field_in_table(&cosim, "[package]", key)
            .unwrap_or_else(|| panic!("rustynes-cosim has no explicit `{key}`"));
        assert_eq!(
            got, want,
            "rustynes-cosim `{key}` = {got:?} but the workspace says {want:?}; \
             the crate is excluded, so nothing inherits this for you"
        );
    }
}

/// The crate must not quietly hold itself to weaker lints than the project.
#[test]
fn the_excluded_crate_carries_the_same_lints() {
    let root = read("Cargo.toml");
    let cosim = read("crates/rustynes-cosim/Cargo.toml");

    for (root_table, crate_table, keys) in [
        (
            "[workspace.lints.rust]",
            "[lints.rust]",
            &["unsafe_code", "missing_docs"][..],
        ),
        (
            "[workspace.lints.clippy]",
            "[lints.clippy]",
            &["pedantic", "nursery", "undocumented_unsafe_blocks"][..],
        ),
    ] {
        for key in keys {
            let want = field_in_table(&root, root_table, key)
                .unwrap_or_else(|| panic!("{root_table} has no `{key}`"));
            let got = field_in_table(&cosim, crate_table, key).unwrap_or_else(|| {
                panic!(
                    "rustynes-cosim's {crate_table} is missing `{key}`, which the workspace sets"
                )
            });
            // Normalized: the workspace writes `nursery  = {...}` with padding.
            assert_eq!(
                got.replace(' ', ""),
                want.replace(' ', ""),
                "lint `{key}` differs between the workspace and the excluded crate"
            );
        }
    }
}

/// The exclusion itself, asserted — this is the property everything above exists
/// to protect, and it is one careless edit away from being undone.
#[test]
fn rustynes_cosim_is_excluded_from_the_workspace() {
    let root = read("Cargo.toml");
    let stripped: String = root
        .lines()
        .map(|l| l.split('#').next().unwrap_or(""))
        .collect::<Vec<_>>()
        .join("\n");

    let members_start = stripped
        .find("members = [")
        .expect("workspace has no `members` list");
    let members_end = stripped[members_start..]
        .find(']')
        .expect("unterminated `members` list")
        + members_start;
    assert!(
        !stripped[members_start..members_end].contains("rustynes-cosim"),
        "rustynes-cosim is back in `members`; a --workspace build will unify its \
         trace features onto rustynes-core and the accuracy battery will start \
         validating the instrumented scheduler again"
    );
    assert!(
        stripped.contains("exclude = [") && stripped.contains("crates/rustynes-cosim"),
        "rustynes-cosim must be listed in the workspace `exclude`"
    );
}

/// The excluded crate's lockfile must be **tracked**.
///
/// `.gitignore` carries a bare `Cargo.lock`, which matches at any depth, and a
/// `!/Cargo.lock` re-include that names only the workspace root. That pairing
/// was written when there was exactly one lockfile. Excluding
/// `rustynes-cosim` from the workspace gave it its own resolve and its own
/// lockfile, which the bare rule then silently ignored -- so CI re-resolved its
/// dependency graph on every run.
///
/// That matters more here than for an ordinary crate: this one emits the
/// goldens an external NES implementation is verified against, and its manifest
/// records the emulator version rather than the resolve. A dependency moving
/// underneath it would be invisible in exactly the artifact whose job is to
/// establish provenance.
#[test]
fn the_excluded_crates_lockfile_is_tracked() {
    let root = repo_root();
    let lock = root.join("crates/rustynes-cosim/Cargo.lock");
    assert!(
        lock.is_file(),
        "crates/rustynes-cosim/Cargo.lock is missing; an excluded package has \
         its own resolve and the lockfile must be committed with it"
    );

    let out = std::process::Command::new("git")
        .arg("-C")
        .arg(&root)
        .args([
            "ls-files",
            "--error-unmatch",
            "crates/rustynes-cosim/Cargo.lock",
        ])
        .output();

    // A source tarball with no .git is a legitimate way to build this; skip
    // rather than fail, and say so, so a skip is never mistaken for a pass.
    let Ok(out) = out else {
        eprintln!("skipping: git is unavailable");
        return;
    };
    if !root.join(".git").exists() {
        eprintln!("skipping: not a git checkout");
        return;
    }
    assert!(
        out.status.success(),
        "crates/rustynes-cosim/Cargo.lock exists but is NOT tracked by git -- \
         check the `!/crates/rustynes-cosim/Cargo.lock` re-include in .gitignore.\n\
         stderr: {}",
        String::from_utf8_lossy(&out.stderr)
    );
}
