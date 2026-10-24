// SPDX-License-Identifier: GPL-3.0-or-later
//! Shared fixture helpers for the `rustynes-dap` tests.
//!
//! The fixtures are `tests/roms/dap/`: `dapdemo.c` built with cc65 and with
//! llvm-mos by `build.py`. `RUSTYNES_DAP_FIXTURES` points the tests at a
//! freshly built copy instead (the CI `dap-fixtures` job rebuilds them with
//! pinned toolchains and runs the suite against its own build).

#![allow(dead_code)]

use std::path::PathBuf;

/// The fixture directory.
#[must_use]
pub fn dir() -> PathBuf {
    std::env::var_os("RUSTYNES_DAP_FIXTURES").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/roms/dap"),
        PathBuf::from,
    )
}

/// The source file (always the committed one: the build copies it).
#[must_use]
pub fn source() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../tests/roms/dap/dapdemo.c")
}

/// The 1-based line carrying `BP:<marker>` in `dapdemo.c`.
///
/// # Panics
///
/// The marker is missing (the fixture was edited without the tests).
#[must_use]
pub fn line(marker: &str) -> u32 {
    let text = std::fs::read_to_string(source()).expect("dapdemo.c");
    let tag = format!("BP:{marker} ");
    let tag_end = format!("BP:{marker}*/");
    let n = text
        .lines()
        .position(|l| l.contains(&tag) || l.replace(' ', "").contains(&tag_end))
        .unwrap_or_else(|| panic!("no BP:{marker} in dapdemo.c"));
    u32::try_from(n + 1).expect("line fits")
}

/// One toolchain's fixture.
#[derive(Clone, Copy, Debug)]
pub struct Fixture {
    /// Name for messages.
    pub name: &'static str,
    /// The `.nes` file name.
    pub rom: &'static str,
    /// The debug file name.
    pub debug: &'static str,
}

/// cc65.
pub const CC65: Fixture = Fixture {
    name: "cc65",
    rom: "dapdemo_cc65.nes",
    debug: "dapdemo_cc65.dbg",
};

/// llvm-mos.
pub const LLVM_MOS: Fixture = Fixture {
    name: "llvm-mos",
    rom: "dapdemo_llvm_mos.nes",
    debug: "dapdemo_llvm_mos.elf",
};

impl Fixture {
    /// The ROM path.
    #[must_use]
    pub fn rom_path(self) -> PathBuf {
        dir().join(self.rom)
    }

    /// The debug-file path.
    #[must_use]
    pub fn debug_path(self) -> PathBuf {
        dir().join(self.debug)
    }
}
