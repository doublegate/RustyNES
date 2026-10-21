//! v3.5.0 (`FE-05`) — what a dropped file is, decided in one place.
//!
//! The desktop has accepted dropped ROMs since v1.0.0 (winit delivers
//! `WindowEvent::DroppedFile` with a path). The browser builds could not:
//! winit's web backend delivers no file-drop event, so until v3.5.0 a ROM
//! dragged onto the page was simply opened by the browser as a download,
//! and the `<input type="file">` picker was the only way in. Both browser
//! builds now listen for the DOM `dragover` / `drop` events themselves
//! (`wasm_winit::install_drop_loader`, `wasm::install_drop_loader`) and
//! route the dropped file by [`classify`].
//!
//! The decision is pure and target-agnostic so the native unit tests pin
//! exactly the rule the browser runs; the DOM half (event wiring, reading
//! the file) cannot run outside a browser and is the manual check recorded
//! in `docs/frontend.md`.
//!
//! The rule, by file name (case-insensitive extension) and size:
//!
//! | dropped file | route |
//! |---|---|
//! | `.nes`, `.fds`, `.unf`, `.unif` | a ROM: the same path as the picker |
//! | `.rnm` | a movie: the same path as the F7 movie picker |
//! | `.rom` / `.bin` of exactly 8 KiB | the FDS BIOS (`disksys.rom`) |
//! | anything else | refused, with the name logged |
//!
//! The ROM list is the desktop drop handler's list, which it now reads from
//! [`is_rom_name`] too, so the two cannot drift. It is deliberately
//! narrower than the desktop's File → Open (no `.zip`, no `.nsf`): the
//! browser load path extracts no archive and plays no NSF, and a file the
//! picker's `accept` list would not offer should not get in by dragging.
//! The BIOS rule keys on the size as well as the extension because `.bin`
//! is a generic name: only an 8 KiB file can be the 8 KiB BIOS, and the
//! core refuses any other size anyway (`FDS_BIOS_LEN`).

/// The size of the Famicom Disk System BIOS (`disksys.rom`), the one size a
/// dropped `.rom` / `.bin` is taken as the BIOS at.
pub const FDS_BIOS_LEN: usize = 0x2000;

/// What to do with a dropped file. See the module table.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DropKind {
    /// A cartridge or disk image: load it as the picker would.
    Rom,
    /// A `.rnm` movie: play it as the movie picker would.
    Movie,
    /// The 8 KiB FDS BIOS: install it as the BIOS upload would.
    FdsBios,
    /// Not something a drop can load; the caller logs the name.
    Unsupported,
}

/// The lower-cased extension of `name` (after its last `.`), if any.
fn extension(name: &str) -> Option<String> {
    let (stem, ext) = name.rsplit_once('.')?;
    // A dotfile (`.nes` alone) has no stem; it is a name, not an extension.
    if stem.is_empty() || stem.ends_with(['/', '\\']) {
        return None;
    }
    Some(ext.to_ascii_lowercase())
}

/// Whether `name` carries one of the ROM extensions a drop accepts
/// (`.nes`, `.fds`, `.unf`, `.unif`, case-insensitive).
#[must_use]
pub fn is_rom_name(name: &str) -> bool {
    matches!(
        extension(name).as_deref(),
        Some("nes" | "fds" | "unf" | "unif")
    )
}

/// Route a dropped file by its `name` and byte length `len`.
#[must_use]
pub fn classify(name: &str, len: usize) -> DropKind {
    if is_rom_name(name) {
        return DropKind::Rom;
    }
    match extension(name).as_deref() {
        Some("rnm") => DropKind::Movie,
        Some("rom" | "bin") if len == FDS_BIOS_LEN => DropKind::FdsBios,
        _ => DropKind::Unsupported,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_extensions_route_as_roms_in_any_case() {
        for name in [
            "Super Mario Bros.nes",
            "zelda.FDS",
            "board.unf",
            "board.Unif",
            "a.b.c.NES",
        ] {
            assert_eq!(classify(name, 40_976), DropKind::Rom, "{name}");
            assert!(is_rom_name(name), "{name}");
        }
    }

    #[test]
    fn a_movie_routes_as_a_movie() {
        assert_eq!(classify("run.rnm", 123), DropKind::Movie);
        assert_eq!(classify("RUN.RNM", 0), DropKind::Movie);
    }

    #[test]
    fn only_an_8_kib_rom_or_bin_is_the_bios() {
        assert_eq!(classify("disksys.rom", 8192), DropKind::FdsBios);
        assert_eq!(classify("DISKSYS.BIN", 8192), DropKind::FdsBios);
        assert_eq!(classify("disksys.rom", 8191), DropKind::Unsupported);
        assert_eq!(classify("disksys.rom", 8193), DropKind::Unsupported);
        assert_eq!(classify("firmware.bin", 16_384), DropKind::Unsupported);
        // The size alone does not make a BIOS.
        assert_eq!(classify("notes.txt", 8192), DropKind::Unsupported);
    }

    #[test]
    fn what_the_browser_cannot_load_is_refused() {
        // `.zip` and `.nsf` open on the desktop's File menu, not in a browser.
        for name in ["game.zip", "tune.nsf", "patch.ips", "readme", ".nes", "nes"] {
            assert_eq!(classify(name, 40_976), DropKind::Unsupported, "{name}");
            assert!(!is_rom_name(name), "{name}");
        }
        // A directory component ending in a dot is not an extension.
        assert!(!is_rom_name("dir/.nes"));
    }
}
