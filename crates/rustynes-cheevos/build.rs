//! Build script for `rustynes-cheevos`.
//!
//! Compiles the vendored RetroAchievements `rcheevos` C library (MIT, under
//! `vendor/rcheevos/`) into a single static archive linked into this crate.
//!
//! On `wasm32` targets the crate body is empty (`#![cfg(not(target_arch =
//! "wasm32"))]`), so this script early-returns without invoking `cc` — a wasm
//! workspace build must not require a C toolchain or the vendored sources.
//!
//! ## Vendored subset and excluded sources
//!
//! Included: `src/rc_client.c`, `src/rc_compat.c`, `src/rc_util.c`,
//! `src/rc_version.c`, all of `src/rcheevos/` (condition VM, runtime, rich
//! presence, leaderboards), all of `src/rapi/` (request/response codecs), and
//! the NES-relevant `src/rhash/` files (`hash.c`, `hash_rom.c`, `md5.c`).
//!
//! Excluded (and disabled via compile defines) because NES needs none of them
//! and they pull external deps (zlib) or large encrypted/disc machinery:
//!   - `hash_disc.c`, `cdreader.c`  -> disabled by `RC_HASH_NO_DISC`
//!   - `hash_encrypted.c`, `aes.c`  -> disabled by `RC_HASH_NO_ENCRYPTED`
//!   - `hash_zip.c`                 -> disabled by `RC_HASH_NO_ZIP`
//!   - `rc_libretro.c`, `rc_client_external*.c`, `rc_client_raintegration*.c`
//!     (never copied into `vendor/`)
//!
//! `RC_DISABLE_LUA` drops the optional Lua rich-presence path. `RC_STATIC`
//! selects static-archive linkage. `RC_CLIENT_SUPPORTS_HASH` enables
//! `rc_client_begin_identify_and_load_game` (we pass raw ROM bytes and let
//! rcheevos hash them internally via `hash_rom.c`).

use std::env;
use std::path::{Path, PathBuf};

fn main() {
    // wasm: empty crate, no C toolchain. Must not invoke cc.
    if env::var("CARGO_CFG_TARGET_ARCH").as_deref() == Ok("wasm32") {
        return;
    }

    let manifest_dir = PathBuf::from(env::var("CARGO_MANIFEST_DIR").unwrap());
    let vendor = manifest_dir.join("vendor/rcheevos");
    let src = vendor.join("src");
    let include = vendor.join("include");

    // Rebuild triggers.
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=static_asserts.c");
    println!("cargo:rerun-if-changed={}", vendor.display());

    // Collect every vendored .c file, then drop the disc/zip/encrypted handlers
    // that the NO_* defines turn off (defensive: they should not be in vendor/,
    // but exclude by name so a stray copy can't break the link).
    let excluded = [
        "hash_disc.c",
        "hash_zip.c",
        "hash_encrypted.c",
        "cdreader.c",
        "aes.c",
        "rc_libretro.c",
        "rc_client_external.c",
        "rc_client_raintegration.c",
    ];

    let mut files: Vec<PathBuf> = Vec::new();
    collect_c_files(&src, &excluded, &mut files);
    files.sort();
    assert!(
        !files.is_empty(),
        "no vendored rcheevos .c files found under {}",
        src.display()
    );

    let mut build = cc::Build::new();
    build
        .include(&include)
        .include(&src)
        .define("RC_STATIC", None)
        .define("RC_CLIENT_SUPPORTS_HASH", None)
        .define("RC_DISABLE_LUA", None)
        .define("RC_HASH_NO_DISC", None)
        .define("RC_HASH_NO_ENCRYPTED", None)
        .define("RC_HASH_NO_ZIP", None)
        .warnings(false)
        .files(&files);

    // The ABI-guard TU (C `_Static_assert`s mirroring src/ffi.rs sizes).
    build.file(manifest_dir.join("static_asserts.c"));

    build.compile("rcheevos");
}

fn collect_c_files(dir: &Path, excluded: &[&str], out: &mut Vec<PathBuf>) {
    let entries =
        std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read_dir {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.unwrap().path();
        if path.is_dir() {
            collect_c_files(&path, excluded, out);
        } else if path.extension().and_then(|s| s.to_str()) == Some("c") {
            let name = path.file_name().and_then(|s| s.to_str()).unwrap_or("");
            if !excluded.contains(&name) {
                out.push(path);
            }
        }
    }
}
