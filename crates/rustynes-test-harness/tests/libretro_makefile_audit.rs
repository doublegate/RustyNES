// SPDX-License-Identifier: GPL-3.0-or-later
//! The libretro core's `Makefile`, exercised the way a packager or the
//! buildbot drives it (libretro audit §3.1, v2.8.1).
//!
//! Every case runs `make -n` (print, do not execute) in
//! `crates/rustynes-libretro/` and asserts on the `cargo` and `cp` lines it
//! would run. Nothing is built, so the test costs milliseconds; it is skipped
//! where `make` is not installed (a Windows runner without MSYS), because the
//! question it answers is about the recipe text, not about any one host.
//!
//! Each assertion pins a defect the audit reported and a dry run reproduced
//! before the fix:
//!
//! * `make PREFIX=/usr/local` — the packaging convention — overrode the
//!   Makefile's own `PREFIX` (the library-file prefix), so the copy read
//!   `target/release//usr/localrustynes.so`. The file prefix is now
//!   `LIB_PREFIX`, and `PREFIX` is left to the packager.
//! * `platform=win` (what the Makefile's own auto-detection assigns on
//!   MinGW) never reached the `windows` target branch, so it built the host
//!   triple and then copied a `.dll` that did not exist.
//! * `DEBUG=1` still built and copied the release core.
//! * `CARGO_TARGET_DIR` moved Cargo's output while the copy still read
//!   `../../target`.

use std::path::PathBuf;
use std::process::Command;

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../rustynes-libretro")
}

/// `make -n` in the libretro crate with `args` and `env`, or `None` when
/// `make` is not available on this host.
fn dry_run(args: &[&str], env: &[(&str, &str)]) -> Option<String> {
    let mut cmd = Command::new("make");
    cmd.arg("-n").args(args).current_dir(crate_dir());
    // The dry run must not see a caller's own overrides.
    cmd.env_remove("CARGO_TARGET_DIR")
        .env_remove("PREFIX")
        .env_remove("LIB_PREFIX")
        .env_remove("DEBUG")
        .env_remove("platform")
        .env_remove("ARCH");
    for (k, v) in env {
        cmd.env(k, v);
    }
    let out = cmd.output().ok()?;
    assert!(
        out.status.success(),
        "make -n {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    Some(String::from_utf8_lossy(&out.stdout).into_owned())
}

/// The single line of `recipe` that starts with `prefix`.
fn line<'a>(recipe: &'a str, prefix: &str) -> &'a str {
    let hits: Vec<&str> = recipe
        .lines()
        .filter(|l| l.trim_start().starts_with(prefix))
        .collect();
    assert_eq!(hits.len(), 1, "expected one `{prefix}` line in:\n{recipe}");
    hits[0].trim()
}

#[test]
fn a_packagers_prefix_does_not_reach_the_copy() {
    let Some(recipe) = dry_run(&["PREFIX=/usr/local"], &[]) else {
        return;
    };
    let cp = line(&recipe, "cp ");
    assert!(
        !cp.contains("/usr/local"),
        "PREFIX leaked into the copy: {cp}"
    );
    assert!(cp.contains("/release/librustynes."), "{cp}");
}

#[test]
fn platform_win_is_the_windows_target() {
    let Some(recipe) = dry_run(&["platform=win", "ARCH=x86_64"], &[]) else {
        return;
    };
    let cargo = line(&recipe, "cargo build");
    assert!(
        cargo.contains("--target x86_64-pc-windows-gnu"),
        "platform=win built the wrong target: {cargo}"
    );
    let cp = line(&recipe, "cp ");
    assert!(
        cp.contains("x86_64-pc-windows-gnu/release/rustynes.dll"),
        "{cp}"
    );
}

#[test]
fn debug_builds_and_copies_the_debug_core() {
    let Some(recipe) = dry_run(&["DEBUG=1"], &[]) else {
        return;
    };
    let cargo = line(&recipe, "cargo build");
    assert!(
        !cargo.contains("--release"),
        "DEBUG=1 built release: {cargo}"
    );
    let cp = line(&recipe, "cp ");
    assert!(cp.contains("/debug/"), "DEBUG=1 copied release: {cp}");
}

#[test]
fn the_copy_follows_cargo_target_dir() {
    let Some(recipe) = dry_run(&[], &[("CARGO_TARGET_DIR", "/build/out")]) else {
        return;
    };
    let cp = line(&recipe, "cp ");
    assert!(
        cp.starts_with("cp /build/out/release/"),
        "the copy ignored CARGO_TARGET_DIR: {cp}"
    );
}

#[test]
fn the_default_build_is_unchanged() {
    // The control: with nothing set, the recipe is the one every existing
    // user and the documentation rely on.
    let Some(recipe) = dry_run(&[], &[]) else {
        return;
    };
    assert_eq!(
        line(&recipe, "cargo build"),
        "cargo build -p rustynes-libretro --release"
    );
    let cp = line(&recipe, "cp ");
    assert!(
        cp.starts_with("cp ../../target/release/librustynes.")
            || cp.starts_with("cp ../../target/release/rustynes."),
        "{cp}"
    );
}
