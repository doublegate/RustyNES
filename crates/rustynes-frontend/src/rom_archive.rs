//! v3.6.0 — a ROM inside a `.zip` archive, for every frontend.
//!
//! The desktop has opened ROMs straight out of `.zip` archives since v1.2.0
//! (Workstream B), and until v3.6.0 that code lived in `app.rs` behind
//! `cfg(not(target_arch = "wasm32"))`, with the `zip` crate in the native-only
//! dependency table, on the reasoning that "the wasm load path receives raw
//! bytes from the browser file picker, so it never needs zip". It does: the
//! bytes are whatever file the user picked, and a dump kept zipped (the usual
//! way ROM sets are kept) reached `Nes::from_rom` as an archive and failed to
//! parse, while the picker's `accept` list did not even offer it. The browser
//! builds now unpack it here, and so does the desktop, through the same code.
//!
//! The browser has no file name to go on once a `FileReader` has finished
//! (the bytes arrive alone), so an archive is recognised by its signature
//! ([`is_zip`](crate::rom_archive::is_zip)) rather than its extension; the desktop keeps keying on the
//! `.zip` extension it has always used.
//!
//! The `zip` crate is built with `default-features = false` and `deflate`
//! only: stored and deflated entries, which is what ROM archives use, through
//! the pure-Rust `miniz_oxide` backend (no C, so it builds for wasm32 as it
//! does natively), and none of the AES / zstd / bzip2 extras.

use std::io::Read as _;

/// The largest archive entry this unpacks: 64 MiB.
///
/// The same bound `bounded_read` applies to a bare file. NES images are at most a few MiB, so
/// anything larger is a corrupt archive or a zip bomb. Both the entry's
/// DECLARED size and the bytes actually read are held to it, since the
/// declared size can lie (Gemini security-high + Copilot, PR #74).
pub const MAX_ENTRY_BYTES: u64 = 64 * 1024 * 1024;

/// The entry extensions the desktop loads from an archive.
pub const DESKTOP_ROM_EXTENSIONS: &[&str] = &["nes", "fds", "nsf", "unf", "unif"];

/// The entry extensions the browser builds load from an archive: the
/// desktop's less `.nsf`, which the browser load path does not play (the same
/// list as `drop_route::is_rom_name`'s images).
pub const WEB_ROM_EXTENSIONS: &[&str] = &["nes", "fds", "unf", "unif"];

/// Whether `bytes` begin like a ZIP archive.
///
/// That is, with a local-file header (`PK\x03\x04`) or the
/// end-of-central-directory record an empty archive consists of
/// (`PK\x05\x06`). No NES, FDS, NSF or UNIF image begins with either: their
/// magics are `NES\x1A`, `FDS\x1A` / `\x01*NINTENDO-HVC*`, `NESM\x1A` and
/// `UNIF`.
#[must_use]
pub fn is_zip(bytes: &[u8]) -> bool {
    bytes.starts_with(b"PK\x03\x04") || bytes.starts_with(b"PK\x05\x06")
}

/// Extract the first ROM entry of a `.zip` archive.
///
/// The first entry whose extension is one of `extensions` (case-insensitive),
/// as its base file name and bytes. `None` if the archive is unreadable, holds no such entry, or the entry is
/// larger than [`MAX_ENTRY_BYTES`].
#[must_use]
pub fn extract_rom(zip_bytes: &[u8], extensions: &[&str]) -> Option<(String, Vec<u8>)> {
    let mut archive = zip::ZipArchive::new(std::io::Cursor::new(zip_bytes)).ok()?;
    let idx = (0..archive.len()).find(|&i| {
        // zip 9: an entry whose name decodes as neither UTF-8 nor CP437 is
        // skipped, the same as an entry that cannot be read at all.
        archive.by_index(i).is_ok_and(|f| {
            f.name().is_ok_and(|n| {
                std::path::Path::new(n.as_ref())
                    .extension()
                    .is_some_and(|e| extensions.iter().any(|x| e.eq_ignore_ascii_case(x)))
            })
        })
    })?;
    let file = archive.by_index(idx).ok()?;
    if file.size() > MAX_ENTRY_BYTES {
        return None;
    }
    let name = file
        .name()
        .ok()
        .and_then(|n| {
            std::path::Path::new(n.as_ref())
                .file_name()
                .map(|f| f.to_string_lossy().into_owned())
        })
        .unwrap_or_else(|| "rom".to_string());
    // `size()` is the archive's own claim, so it bounds nothing: capped at the
    // entry limit, a lying header cannot ask for a multi-gigabyte buffer
    // before `take` is reached (Antigravity on #631; the closed-PR sweep).
    let cap = usize::try_from(file.size().min(MAX_ENTRY_BYTES)).unwrap_or(0);
    let mut out = Vec::with_capacity(cap);
    file.take(MAX_ENTRY_BYTES + 1).read_to_end(&mut out).ok()?;
    if out.len() as u64 > MAX_ENTRY_BYTES {
        return None;
    }
    Some((name, out))
}

/// Unpack a picked or dropped file for the browser load paths.
///
/// `bytes` unchanged if they are not an archive, else the archive's first [`WEB_ROM_EXTENSIONS`] entry. The `Ok` carries
/// the entry's name when one was extracted, for the log; the `Err` is a
/// message fit to show the user.
///
/// # Errors
///
/// An archive with no loadable image in it, or one that cannot be read.
pub fn unpack_for_web(bytes: Vec<u8>) -> Result<(Vec<u8>, Option<String>), String> {
    if !is_zip(&bytes) {
        return Ok((bytes, None));
    }
    extract_rom(&bytes, WEB_ROM_EXTENSIONS)
        .map(|(name, rom)| (rom, Some(name)))
        .ok_or_else(|| {
            "No .nes, .fds, .unf or .unif image found inside the .zip archive \
             (or the archive could not be read)"
                .to_string()
        })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write as _;

    fn archive(entries: &[(&str, &[u8])], method: zip::CompressionMethod) -> Vec<u8> {
        let mut buf = Vec::new();
        {
            let mut w = zip::ZipWriter::new(std::io::Cursor::new(&mut buf));
            let opts = zip::write::SimpleFileOptions::default().compression_method(method);
            for (name, data) in entries {
                w.start_file(*name, opts).unwrap();
                w.write_all(data).unwrap();
            }
            w.finish().unwrap();
        }
        buf
    }

    const NES: &[u8] = b"NES\x1a\x01\x01\x00\x00\x00\x00\x00\x00\x00\x00\x00\x00rest";

    #[test]
    fn a_deflated_entry_is_found_under_any_case_and_any_folder() {
        let zip = archive(
            &[("readme.txt", b"hi"), ("sets/Game (U).NES", NES)],
            zip::CompressionMethod::Deflated,
        );
        assert!(is_zip(&zip));
        let (name, rom) = extract_rom(&zip, WEB_ROM_EXTENSIONS).expect("entry");
        assert_eq!(name, "Game (U).NES");
        assert_eq!(rom, NES);
    }

    #[test]
    fn the_web_path_unpacks_an_archive_and_passes_a_bare_image_through() {
        let zip = archive(&[("g.nes", NES)], zip::CompressionMethod::Stored);
        let (rom, name) = unpack_for_web(zip).expect("unpacked");
        assert_eq!(rom, NES);
        assert_eq!(name.as_deref(), Some("g.nes"));
        let (rom, name) = unpack_for_web(NES.to_vec()).expect("bare");
        assert_eq!(rom, NES);
        assert_eq!(name, None, "a bare image is not an archive");
    }

    #[test]
    fn an_archive_without_a_loadable_image_is_an_error_not_a_bad_rom() {
        // An NSF only: the desktop takes it, the browser does not play NSF.
        let zip = archive(&[("tune.nsf", b"NESM\x1a")], zip::CompressionMethod::Stored);
        assert!(unpack_for_web(zip.clone()).is_err());
        assert!(extract_rom(&zip, DESKTOP_ROM_EXTENSIONS).is_some());
        // Not an archive at all past its signature.
        assert!(unpack_for_web(b"PK\x03\x04garbage".to_vec()).is_err());
    }

    #[test]
    fn no_rom_magic_looks_like_an_archive() {
        for magic in [
            &b"NES\x1a"[..],
            b"FDS\x1a",
            b"\x01*NINTENDO-HVC*",
            b"NESM\x1a",
            b"UNIF",
        ] {
            assert!(!is_zip(magic), "{magic:?}");
        }
        assert!(
            is_zip(b"PK\x05\x06"),
            "an empty archive is still an archive"
        );
    }
}
