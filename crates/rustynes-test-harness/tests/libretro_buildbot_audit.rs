// SPDX-License-Identifier: GPL-3.0-or-later
//! The libretro buildbot recipe (`.gitlab-ci.yml`), checked as text (v3.4.2).
//!
//! The buildbot is a third CI system, on libretro's GitLab. Its mirror builds
//! every pushed branch, so a recipe change CAN be checked before merge, but
//! only by fetching the job's artifact and inspecting it: a green job proves
//! that the core linked, not that it loads. Nothing in this repository's own
//! CI builds for macOS that way, so a regression here surfaced only when a
//! user reported a core that would not load. This audit pins the one setting
//! whose absence did exactly that.
//!
//! **The macOS arm64 core must not be stripped after linking.** libretro's
//! `rust-apple.yml` template runs `$STRIP -S` on the dylib when
//! `STRIP_CORE_LIB` is 1, and the macOS templates set it to 1. On arm64 the
//! linker has already ad-hoc signed the library; the image's strip (osxcross,
//! on Linux) rewrites the file without re-signing it, and macOS on Apple
//! Silicon kills any process that loads arm64 code whose signature does not
//! match. `RetroArch` died the moment it loaded the core. Measured on the
//! buildbot's 2026-10-20 build: 101 of 538 `CodeDirectory` page hashes
//! mismatched, while natively built C cores from the same server verified
//! clean.

use std::path::PathBuf;

fn gitlab_ci() -> String {
    let p = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../.gitlab-ci.yml");
    // Checkouts on Windows are CRLF; every match below is line-based.
    std::fs::read_to_string(&p)
        .unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
        .replace("\r\n", "\n")
}

/// The body of the top-level job `name`: every line after `name:` up to the
/// next line that starts a new top-level key or comment block.
fn job(text: &str, name: &str) -> Vec<String> {
    let header = format!("{name}:");
    let mut lines = text.lines().skip_while(|l| *l != header);
    assert!(
        lines.next().is_some(),
        ".gitlab-ci.yml has no top-level job `{name}`"
    );
    lines
        .take_while(|l| l.is_empty() || l.starts_with(' '))
        .map(str::to_owned)
        .collect()
}

#[test]
fn the_macos_arm64_core_is_not_stripped_after_signing() {
    let text = gitlab_ci();
    let body = job(&text, "libretro-build-osx-arm64");
    assert!(
        body.iter().any(|l| l.trim() == "STRIP_CORE_LIB: \"0\""),
        "libretro-build-osx-arm64 must set STRIP_CORE_LIB: \"0\": the template's \
         post-link strip invalidates the arm64 ad-hoc signature and macOS kills \
         RetroArch when it loads the core. Job body:\n{}",
        body.join("\n")
    );
    // The setting must sit in the job's own `variables:`, where it outranks
    // the template's `STRIP_CORE_LIB: 1`, not in a comment or a script line.
    let vars = body
        .iter()
        .position(|l| l == "  variables:")
        .expect("libretro-build-osx-arm64 has a variables: block");
    let in_vars = body[vars + 1..]
        .iter()
        .take_while(|l| l.is_empty() || l.starts_with("    "))
        .any(|l| l.trim() == "STRIP_CORE_LIB: \"0\"");
    assert!(
        in_vars,
        "STRIP_CORE_LIB: \"0\" must be in the job's variables: block"
    );
}

#[test]
fn the_job_parser_finds_the_jobs_it_is_asked_for() {
    // A guard on the helper: had `job` returned an empty body for a real job,
    // the assertion above would report a missing setting rather than a broken
    // parser, and a rename of the job would read as the fix being reverted.
    let text = gitlab_ci();
    for name in ["libretro-build-osx-arm64", "libretro-build-osx-x64"] {
        let body = job(&text, name);
        assert!(
            body.iter().any(|l| l.trim() == "extends:"),
            "{name}'s body was not found intact: {body:?}"
        );
    }
}
