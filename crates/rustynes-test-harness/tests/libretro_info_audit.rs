//! Standing audit — the libretro `.info` metadata must not drift from the
//! workspace it describes.
//!
//! **Why this exists.** `RustyNES` was relicensed MIT/Apache-2.0 ->
//! GPL-3.0-or-later in v2.2.9 (ADR 0036) as a derivative work of GPL emulators.
//! The relicense propagated to `Cargo.toml`, `NOTICE`, `deny.toml`, the SPDX
//! headers, `docs/originality-and-provenance.md`, the README, and this repo's
//! own copy of `rustynes_libretro.info` — but the copy `RetroArch` actually reads
//! lives **upstream**, in `libretro/libretro-super`'s `dist/info/`, and nobody
//! updated it. For eleven days `RetroArch` told every user that a GPL-3.0-or-later
//! emulator was "MIT OR Apache-2.0".
//!
//! Nothing in the repo could have caught that, because nothing compared this
//! file against the workspace. This audit is that comparison. It cannot reach
//! into the upstream repo — no test can — but it guarantees the local file is
//! always correct, so a sync is a copy rather than a re-derivation, and it fails
//! loudly at the moment the workspace moves rather than eleven days later.
//!
//! Modelled on `snapshot_schema_audit.rs`, which exists for the same reason: a
//! field-vs-schema gap that no behavioural test could see.
//!
//! **License-token mapping.** libretro `.info` files do not use SPDX. They use
//! short tokens, and mark "or later" with a trailing `+` — `GPLv2+`, `LGPLv2.1+`
//! (verified against all 316 core info files in `libretro/libretro-super`:
//! `GPLv2` x100, `GPLv3` x64, `GPLv2+` x19, `GPLv3+` x5). So GPL-3.0-**or-later**
//! is `GPLv3+`; a bare `GPLv3` would understate it as GPL-3.0-only.

use std::path::{Path, PathBuf};

/// Workspace root, derived from this crate's manifest dir rather than the CWD
/// (which differs between `cargo test` and a direct binary invocation).
fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .expect("crates/<crate>/ is two levels below the workspace root")
        .to_path_buf()
}

fn info_file() -> String {
    let p = workspace_root().join("crates/rustynes-libretro/rustynes_libretro.info");
    std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("read {}: {e}", p.display()))
}

/// Pull a `key = "value"` field out of the `.info` file.
fn field(info: &str, key: &str) -> String {
    info.lines()
        .find_map(|l| {
            let (k, v) = l.split_once('=')?;
            (k.trim() == key).then(|| v.trim().trim_matches('"').to_owned())
        })
        .unwrap_or_else(|| panic!("`.info` has no `{key}` field"))
}

/// Pull a `key = "value"` out of the workspace `[workspace.package]` table.
///
/// Deliberately table-scoped rather than a first-match scan: the workspace
/// manifest carries `version`/`license` keys in more than one place, and the
/// first match is not necessarily the one that governs the crates. This is the
/// same trap the CI toolchain resolver hit when it matched the first `channel`
/// key anywhere in `rust-toolchain.toml`.
fn workspace_package_field(key: &str) -> String {
    let manifest = std::fs::read_to_string(workspace_root().join("Cargo.toml"))
        .expect("read workspace Cargo.toml");
    let mut in_table = false;
    for line in manifest.lines() {
        let t = line.trim();
        if t.starts_with('[') {
            in_table = t == "[workspace.package]";
            continue;
        }
        if !in_table {
            continue;
        }
        if let Some((k, v)) = t.split_once('=')
            && k.trim() == key
        {
            return v.trim().trim_matches('"').to_owned();
        }
    }
    panic!("`[workspace.package]` has no `{key}` key");
}

/// SPDX identifier -> the libretro `.info` token that means the same thing.
fn libretro_license_token(spdx: &str) -> &'static str {
    match spdx {
        "GPL-3.0-or-later" => "GPLv3+",
        "GPL-3.0-only" => "GPLv3",
        "GPL-2.0-or-later" => "GPLv2+",
        "GPL-2.0-only" => "GPLv2",
        "MIT" => "MIT",
        other => panic!(
            "no libretro token mapping for SPDX `{other}`. RustyNES's license \
             changed and this audit was not taught the new mapping -- add it \
             here, then update `crates/rustynes-libretro/rustynes_libretro.info` \
             AND open a PR against `libretro/libretro-super`'s `dist/info/`, \
             which is the copy RetroArch actually reads."
        ),
    }
}

/// The `.info` license must state the workspace license, in libretro's dialect.
///
/// This is the assertion whose absence let `RetroArch` advertise the wrong license
/// for eleven days.
#[test]
fn libretro_info_license_matches_the_workspace_license() {
    let spdx = workspace_package_field("license");
    let expected = libretro_license_token(&spdx);
    let actual = field(&info_file(), "license");

    assert_eq!(
        actual, expected,
        "`rustynes_libretro.info` says license=\"{actual}\" but the workspace is \
         SPDX `{spdx}`, which is libretro token `{expected}`.\n\
         \n\
         NOTE: fixing the file in THIS repo is only half the job. RetroArch reads \
         `dist/info/rustynes_libretro.info` from `libretro/libretro-super`, not \
         from here -- see docs/libretro/UPSTREAM_SYNC.md."
    );
}

/// The advertised `display_version` must be the version this tree actually is.
///
/// A stale version here is how the upstream copy ended up claiming v2.2.1 long
/// after v2.3.x shipped.
#[test]
fn libretro_info_display_version_matches_the_workspace_version() {
    let version = workspace_package_field("version");
    let actual = field(&info_file(), "display_version");

    assert_eq!(
        actual,
        format!("v{version}"),
        "`rustynes_libretro.info` advertises display_version=\"{actual}\" but the \
         workspace is at {version}. Bump the `.info` in the same change as the \
         version, and re-sync `libretro/libretro-super`."
    );
}

/// `supported_extensions` must match what the core will actually load.
///
/// `UPSTREAM_SYNC.md` already asks for this by hand; asking a human to diff two
/// lists on every release is how the license drifted in the first place.
#[test]
fn libretro_info_supported_extensions_are_the_ones_the_core_loads() {
    let actual = field(&info_file(), "supported_extensions");
    assert_eq!(
        actual, "nes|fds",
        "`supported_extensions` changed. If the core genuinely gained or lost a \
         container, update this assertion AND the upstream info file; if it did \
         not, the `.info` is wrong."
    );
}
