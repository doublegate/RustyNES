//! v3.5.1 — reading a file the user named, with a bound.
//!
//! Every ROM, patch, BIOS, movie, script, palette, splits and cheat file the
//! desktop frontend opens is a path the user picked, dropped, or left in a
//! recent-files list, and until v3.5.1 each was read with a bare
//! [`std::fs::read`]. Two kinds of path turn that into a hang or an
//! allocation failure on the window thread (`T-V350-REVIEW-RESIDUE`,
//! Antigravity on #624):
//!
//! - **a file that is not a regular file.** A FIFO blocks in `open` until a
//!   writer appears, which may be never; a character device such as
//!   `/dev/zero` (named `game.nes`, or reached through a symlink) streams
//!   without end. Neither has a meaningful length, and `metadata().len()`
//!   reports 0 for both, so a length check alone passes them.
//! - **a file larger than anything the frontend can use.** The bound is
//!   applied to the read, not taken from the metadata, so a file that grows
//!   while it is read is still cut off.
//!
//! `read` refuses the first kind from the metadata (following symlinks,
//! since the target is what gets read) before opening anything, and reads at
//! most `limit + 1` bytes of the second, refusing if the extra byte arrived.
//! The bundle loader's own `read_bounded` predates this and applies the same
//! read-side bound with its own error type.

use std::io::{self, Read as _};
use std::path::Path;

/// The largest file [`read`] accepts by default: 64 MiB.
///
/// Far above any NES image (NES 2.0 can describe more, but no dump in any preservation set
/// approaches it), any movie a session records, and any script or palette,
/// and small enough that a refusal arrives before memory runs out.
pub const MAX_USER_FILE: u64 = 64 * 1024 * 1024;

/// Read `path` whole, refusing a non-regular file and anything larger than
/// `limit` bytes. The error says which, naming the limit.
///
/// # Errors
///
/// The underlying I/O error; `InvalidInput` for a path that is not a regular
/// file; `InvalidData` for a file over `limit`.
pub fn read_limited(path: &Path, limit: u64) -> io::Result<Vec<u8>> {
    let meta = std::fs::metadata(path)?;
    if !meta.is_file() {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!("{} is not a regular file", path.display()),
        ));
    }
    if meta.len() > limit {
        return Err(too_large(path, limit));
    }
    let mut bytes = Vec::new();
    std::fs::File::open(path)?
        .take(limit.saturating_add(1))
        .read_to_end(&mut bytes)?;
    if bytes.len() as u64 > limit {
        return Err(too_large(path, limit));
    }
    Ok(bytes)
}

/// [`read_limited`] at [`MAX_USER_FILE`].
///
/// # Errors
///
/// As [`read_limited`].
pub fn read(path: &Path) -> io::Result<Vec<u8>> {
    read_limited(path, MAX_USER_FILE)
}

/// [`read`], decoded as UTF-8 (`InvalidData` when it is not).
///
/// # Errors
///
/// As [`read_limited`], plus `InvalidData` for text that is not UTF-8.
pub fn read_to_string(path: &Path) -> io::Result<String> {
    String::from_utf8(read(path)?).map_err(|e| io::Error::new(io::ErrorKind::InvalidData, e))
}

fn too_large(path: &Path, limit: u64) -> io::Error {
    io::Error::new(
        io::ErrorKind::InvalidData,
        format!(
            "{} is larger than {} MiB, more than any file this can open",
            path.display(),
            limit / (1024 * 1024)
        ),
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_regular_file_reads_whole() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("a.nes");
        std::fs::write(&p, b"NES\x1a payload").unwrap();
        assert_eq!(read(&p).unwrap(), b"NES\x1a payload");
        assert_eq!(read_to_string(&p).unwrap(), "NES\u{1a} payload");
    }

    #[test]
    fn a_file_over_the_limit_is_refused_and_one_at_it_is_not() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("big.nes");
        std::fs::write(&p, [0u8; 17]).unwrap();
        let e = read_limited(&p, 16).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidData, "{e}");
        assert_eq!(read_limited(&p, 17).unwrap().len(), 17);
    }

    #[test]
    fn a_directory_is_not_a_regular_file() {
        let dir = tempfile::tempdir().unwrap();
        let e = read(dir.path()).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "{e}");
    }

    /// The case the module exists for: a device that reports length 0 and
    /// never ends. Refused from the metadata, so the test cannot hang.
    #[cfg(unix)]
    #[test]
    fn a_device_is_refused_before_it_is_read() {
        let e = read(Path::new("/dev/zero")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "{e}");
        let dir = tempfile::tempdir().unwrap();
        let link = dir.path().join("game.nes");
        std::os::unix::fs::symlink("/dev/zero", &link).unwrap();
        let e = read(&link).unwrap_err();
        assert_eq!(
            e.kind(),
            io::ErrorKind::InvalidInput,
            "through a symlink: {e}"
        );
    }

    /// A FIFO would block `open` until a writer appears; it is refused
    /// before `open` is reached.
    #[cfg(unix)]
    #[test]
    fn a_fifo_is_refused_without_blocking() {
        let dir = tempfile::tempdir().unwrap();
        let fifo = dir.path().join("pipe.nes");
        let made = std::process::Command::new("mkfifo")
            .arg(&fifo)
            .status()
            .expect("mkfifo runs");
        assert!(made.success(), "mkfifo {}", fifo.display());
        let e = read(&fifo).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::InvalidInput, "{e}");
    }
}
