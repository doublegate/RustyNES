//! Build script (v3.7.1): embed the application icon into the Windows
//! executable.
//!
//! The winit window icon (`src/icon.rs`) is set at run time and covers only
//! an open window. Explorer, the taskbar's pinned entry and shortcuts read the
//! icon from the executable's own resources, so on a Windows target this
//! compiles `assets/RustyNES_Icon_Pack/windows/RustyNES.ico` (16 to 256 px)
//! into `rustynes.exe` with `winresource`, which drives the Windows SDK's
//! resource compiler.
//!
//! Every other target does nothing here. The icon is cosmetic, so a failure
//! (no resource compiler on the host) is a `cargo:warning` for a local
//! build. A release must not ship without it, though, so `release.yml` sets
//! `RUSTYNES_REQUIRE_EXE_ICON=1`, which turns the failure into a build
//! error.

const ICO: &str = "../../assets/RustyNES_Icon_Pack/windows/RustyNES.ico";

fn main() {
    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed={ICO}");
    println!("cargo:rerun-if-env-changed=RUSTYNES_REQUIRE_EXE_ICON");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }
    embed_windows_icon();
}

#[cfg(windows)]
fn embed_windows_icon() {
    let mut res = winresource::WindowsResource::new();
    res.set_icon(ICO);
    if let Err(e) = res.compile() {
        report(&format!(
            "could not embed the application icon ({ICO}): {e}"
        ));
    }
}

/// A Windows target built on a non-Windows host: `winresource` is not
/// available (it is a Windows-host build-dependency), so the icon cannot be
/// embedded.
#[cfg(not(windows))]
fn embed_windows_icon() {
    report("a Windows target built on a non-Windows host: the application icon is not embedded");
}

fn report(msg: &str) {
    assert!(
        std::env::var("RUSTYNES_REQUIRE_EXE_ICON").as_deref() != Ok("1"),
        "{msg} (RUSTYNES_REQUIRE_EXE_ICON=1)"
    );
    println!("cargo:warning={msg}");
}
