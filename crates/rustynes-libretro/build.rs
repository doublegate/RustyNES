//! Build-time check that the libretro core can contain a panic.
//!
//! The core's callbacks stop a panic at the C boundary with `catch_unwind`
//! (libretro audit L-1.1, v2.8.0), which does nothing when the core is built
//! with `panic = "abort"`: the panic ends the process that loaded it, which is
//! `RetroArch`. The workspace `[profile.release]` aborts, for the desktop and
//! web builds, and Cargo cannot set `panic` per package, so the supported
//! paths override it with `CARGO_PROFILE_RELEASE_PANIC=unwind` (this crate's
//! `Makefile`, `.gitlab-ci.yml`, and CI's `libretro-cross` gate).
//!
//! A direct `cargo build --release` bypasses all three, and because this
//! crate is the workspace's only `default-members` entry, a bare
//! `cargo build --release` at the root builds exactly this core that way.
//! This script turns that into a visible warning (a heuristic: see `main`) rather than a silently
//! weaker artifact. It is a warning and not an error because
//! `cargo build --release --workspace`, a documented command, also builds
//! this crate, and failing it would break the desktop build for a core that
//! command does not ship.

fn main() {
    println!("cargo::rerun-if-env-changed=CARGO_PROFILE_RELEASE_PANIC");
    // Not `CARGO_CFG_PANIC`: Cargo derives that from the target's DEFAULT
    // strategy, not the profile, and reports "unwind" for an aborting release
    // build (measured 2026-09-25). The profile's own `panic` is not visible to
    // a build script at all, so this checks for the override every supported
    // path sets. The exact answer is `cfg!(panic = "abort")` inside the crate,
    // which `on_load_game` logs to the frontend.
    let release = std::env::var("PROFILE").as_deref() == Ok("release");
    let unwind = std::env::var("CARGO_PROFILE_RELEASE_PANIC").as_deref() == Ok("unwind");
    if release && !unwind {
        println!(
            "cargo::warning=rustynes-libretro: CARGO_PROFILE_RELEASE_PANIC=unwind is not set, \
             so this release build uses the workspace's panic = \"abort\" and an internal \
             error will close the frontend instead of stopping the game. Build the core with \
             `make` in crates/rustynes-libretro, or set CARGO_PROFILE_RELEASE_PANIC=unwind."
        );
    }
}
