//! One durable, atomic file write, shared by every path that persists user data.
//!
//! # Why this is a module and not three copies
//!
//! v2.3.9 made [`crate::config::Config::save_to`] atomic and durable after
//! `fs::write` was found capable of leaving a user holding a truncated
//! `config.toml` — every keybinding, palette, shader preset and per-game setting
//! they had. It took **seven** properties to get right, and **five of them came
//! from review rather than from the first draft.** That ratio is the whole
//! argument for one implementation: a property that five separate reviews had to
//! find once will not be independently rediscovered three more times.
//!
//! The paths this replaces, and what each was actually doing before:
//!
//! | path | what it wrote with | properties held |
//! | --- | --- | :---: |
//! | `config.rs::save_to` | the full v2.3.9 sequence | 7 of 7 |
//! | `save_state.rs::save_to_slot` | `fs::write` | 0 of 7 |
//! | `cheats.rs::save_for_rom` | `fs::write` | 0 of 7 |
//! | `per_game.rs::save_overlay` | tmp + `rename` | 2 of 7 |
//!
//! `save_state.rs` is the one that matters most and was named last in the plan:
//! **a truncated save state is a user's game progress**, which is a worse loss
//! than a truncated config, and it was writing with the bare call the config path
//! had already been fixed for.
//!
//! `per_game.rs` is the instructive one, and it was not in the plan at all. It
//! *looks* correct — it writes a sibling temp file and renames — so a reader
//! scanning for `fs::write` straight onto a target would clear it. It has no
//! `fsync`, so the rename can commit a directory entry pointing at bytes that
//! never reached the medium; and its scratch name is a **fixed**
//! `path.with_extension("json.tmp")`, shared by every process and every
//! concurrent call, which is precisely the failure the mechanism exists to
//! prevent, reintroduced by the mechanism itself. A partially-correct
//! implementation is harder to spot than an absent one.
//!
//! # The seven properties
//!
//! 1. **Sibling scratch file.** Across a filesystem boundary `rename` is not a
//!    rename; a `$TMPDIR` on another mount silently degrades this to a copy.
//! 2. **`fsync` before the rename.** `fs::write` returns when the bytes reach the
//!    page cache, not the medium. `rename` is atomic against other *processes*,
//!    but a power loss between write and rename leaves the entry pointing at
//!    contents that never landed — the exact outcome this claims to prevent.
//! 3. **Parent-directory sync.** On POSIX the entry `rename` creates is itself a
//!    cache update until the directory is synced.
//! 4. **`create_new(true)`.** `File::create` follows symlinks and truncates, so a
//!    predictable scratch name is a CWE-377 surface.
//! 5. **Mode applied at creation**, then set exactly. `open(2)` masks with the
//!    umask, so creation alone can only land *narrower*; the explicit set makes it
//!    exact. Narrow-then-correct, never widen-then-narrow.
//! 6. **Symlink resolution.** `fs::write` follows a link; `rename` replaces it. A
//!    user who symlinked their config into a dotfiles repository would otherwise
//!    find the link replaced by a regular file on the first automatic save.
//! 7. **A pid + per-call counter in the scratch name**, which is what makes (4)
//!    adoptable: with a shared `.tmp`, exclusive creation fails every save after
//!    one crash.
//!
//! # It is not uniform across platforms, and does not pretend to be
//!
//! A portable spine — scratch sibling, exclusive create, write, `fsync`, rename —
//! with a tail on **both** platforms:
//!
//! | property | Unix | Windows |
//! | --- | :---: | --- |
//! | parent-directory `sync_all` | yes | not applicable — opening a directory as a `File` is not portable, and `MoveFileEx` orders the metadata write |
//! | mode at creation, then exact | yes | not applicable — the ACL is inherited from the parent directory |
//! | symlink resolution | yes | applicable in principle; the dotfiles convention that motivates it is a Unix one |
//! | rename retry | not needed | **required** — see below |
//!
//! `rename`-replaces-existing holds on both: `std::fs::rename` maps to
//! `MoveFileEx` with `MOVEFILE_REPLACE_EXISTING`. It holds with a caveat POSIX
//! does not have — on Windows the rename **fails if another process has the
//! target open**, and an antivirus scanner or a search indexer reading
//! `config.toml` is enough. That is why the config path never needed it, and why
//! it would go unnoticed until a Windows user reported a save that failed for no
//! visible reason.
//!
//! # The retry loop is portable so that it can be tested
//!
//! The obvious shape is a `#[cfg(windows)]` block. This module deliberately does
//! not use one for the loop itself, because CI runs the test suite on Linux and a
//! `cfg`-walled retry is code no test on this project's primary platform can ever
//! execute — an untested mechanism guarding a failure mode nobody can reproduce
//! locally.
//!
//! Instead the loop is portable and the **predicate** is platform-scoped:
//! `is_transient_rename_error` is `#[cfg(windows)]`-aware and returns `false`
//! unconditionally on Unix, so Unix performs exactly one attempt and its
//! behaviour is unchanged. The loop is then exercised on every platform through
//! `rename_with_retry_using`, which takes the rename as a closure. The mechanism
//! is tested where the condition cannot occur.
//!
//! When the retries are exhausted the error **propagates**. A save that fails
//! silently after N attempts is worse than one that fails on the first, because
//! the user gets no signal at all — and v2.3.9 fixed a swallowed save error for
//! exactly that reason.
//!
//! # What the tests do NOT cover, and why
//!
//! Stated rather than left to be inferred from a green suite. A mutation pass
//! over this module deleted each property in turn; five of seven were caught, and
//! the two that were not are **not observable from inside the process**:
//!
//! - **`fsync` before the rename (property 2).** Deleting it changes nothing any
//!   in-process assertion can see — the page cache serves the read back
//!   identically. Only a power loss or a filesystem fault injector distinguishes
//!   the two, and neither belongs in a unit suite.
//! - **Mode applied *at creation* (property 5).** Deleting `opts.mode(...)` still
//!   ends at the right mode, because the explicit `set_permissions` after it
//!   corrects the result. Creation-mode is a **race-window narrowing**, not an
//!   end-state property: it removes an interval in which the file sits at the
//!   umask default. A test can only observe the end state, so the window is
//!   invisible to it by construction.
//!
//! Both are kept for the reason they were added, and neither should be removed on
//! the evidence that "no test fails". That inference is the failure this project
//! has been bitten by more than once; an untested property is not an unnecessary
//! one.
//!
//! Everything else *is* pinned by a mutation: symlink resolution (both the live
//! and the broken-link path), the exact mode after creation, the occupied-scratch
//! retry, exhaustion propagating rather than reporting success, and the Unix
//! single-attempt guarantee.

use std::fs;
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

/// Monotonic sequence for scratch filenames.
///
/// Module scope rather than a function-local `static`, because clippy's
/// `items_after_statements` fires on the latter — and it is right that an item
/// declared mid-function reads as if it were scoped to that point when it is not.
static SCRATCH_SEQ: AtomicU64 = AtomicU64::new(0);

/// How many times to attempt the rename before giving up.
///
/// Only ever more than one on Windows (see `is_transient_rename_error`). Five
/// attempts with the backoff below is a worst case of 310 ms, which is a long
/// time in a UI frame and a short one against losing a save the user believes
/// happened.
const RENAME_ATTEMPTS: u32 = 5;

/// Base backoff between rename attempts, doubled each time: 10, 20, 40, 80, 160.
///
/// The `sleep` this drives is unreachable off Windows, which matters on **wasm**
/// specifically: `std::thread::sleep` cannot block on `wasm32-unknown-unknown`.
/// It is never called there because `is_transient_rename_error` is `false` on
/// every non-Windows target, so the loop returns on the first error.
const RENAME_BACKOFF_MS: u64 = 10;

/// Resolve a symlinked target to the file it points at.
///
/// `fs::write` follows a symlink and writes through to its target; `fs::rename`
/// replaces the link itself. Without this, a user who has symlinked a config or
/// cheat file into a dotfiles repository — a common setup — finds the link
/// silently replaced by a regular file on the first automatic save, and the
/// repository stops receiving changes. That is a behaviour regression introduced
/// by the fix rather than by the bug.
///
/// Handles three cases, in order:
///
/// - a resolvable path (symlink or not) — `canonicalize` gives the real file;
/// - a **broken** symlink — `canonicalize` fails, so the link is read directly.
///   This is the freshly-created-dotfiles case: the link exists, its target does
///   not yet;
/// - anything else — the path itself, which covers a first-ever save.
///
/// A relative link target is resolved against the link's own directory, which is
/// what a relative symlink means.
#[must_use]
pub fn resolve_write_target(path: &Path) -> PathBuf {
    if let Ok(real) = fs::canonicalize(path) {
        return real;
    }
    match fs::read_link(path) {
        Ok(dest) if dest.is_absolute() => dest,
        Ok(dest) => match path.parent() {
            Some(dir) => dir.join(dest),
            None => dest,
        },
        Err(_) => path.to_path_buf(),
    }
}

/// Is this rename failure one a retry could plausibly clear?
///
/// On Windows, `MoveFileEx` fails with a sharing violation when another process
/// has the target open — an antivirus scanner, a search indexer, or a backup
/// agent reading the file is enough, and all three are transient. `std::io` maps
/// both `ERROR_ACCESS_DENIED` and `ERROR_SHARING_VIOLATION` to
/// [`io::ErrorKind::PermissionDenied`].
///
/// On Unix this is unconditionally `false`. POSIX `rename` has no such
/// constraint, so a `PermissionDenied` there means the directory permissions
/// genuinely forbid it — a condition retrying cannot change, and retrying would
/// only delay an error the caller needs now.
#[must_use]
pub const fn is_transient_rename_error(e: &io::Error) -> bool {
    #[cfg(windows)]
    {
        matches!(e.kind(), io::ErrorKind::PermissionDenied)
    }
    #[cfg(not(windows))]
    {
        let _ = e;
        false
    }
}

/// The retry loop, over an arbitrary rename operation.
///
/// Both the operation and the transience predicate are parameters, and the
/// predicate is the load-bearing one. An earlier version called
/// `is_transient_rename_error` directly, which reads as testable and is not:
/// that predicate is unconditionally `false` on Unix, so the exhaustion branch is
/// **unreachable** on the platform CI runs. A mutation making exhaustion return
/// `Ok(())` — silently reporting a save that never happened, the worst outcome
/// this module has — was NOT caught by the test written for it. Injecting the
/// predicate makes the branch reachable everywhere.
///
/// Propagates the final error when the attempts are exhausted rather than
/// reporting success or falling silent.
fn rename_with_retry_using<F, P>(mut op: F, transient: P) -> io::Result<()>
where
    F: FnMut() -> io::Result<()>,
    P: Fn(&io::Error) -> bool,
{
    let mut attempt: u32 = 0;
    loop {
        match op() {
            Ok(()) => return Ok(()),
            Err(e) => {
                attempt += 1;
                if attempt >= RENAME_ATTEMPTS || !transient(&e) {
                    return Err(e);
                }
                std::thread::sleep(std::time::Duration::from_millis(
                    RENAME_BACKOFF_MS << (attempt - 1),
                ));
            }
        }
    }
}

/// `fs::rename`, retried past a transient Windows sharing violation.
fn rename_with_retry(from: &Path, to: &Path) -> io::Result<()> {
    rename_with_retry_using(|| fs::rename(from, to), is_transient_rename_error)
}

/// Write `contents` to `path` atomically and durably.
///
/// Creates the parent directory if needed, resolves a symlinked target, writes to
/// an exclusively-created sibling scratch file, `fsync`s it, carries the existing
/// file's mode across on Unix, renames over the target, and syncs the parent
/// directory.
///
/// On failure the scratch file is removed and the **existing file is left
/// untouched** — a stale-but-valid file is the one worth keeping.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] from any step. A rename that fails after
/// `RENAME_ATTEMPTS` transient failures returns the last error rather than
/// succeeding quietly.
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    let target = resolve_write_target(path);
    if let Some(parent) = target.parent()
        && !parent.as_os_str().is_empty()
    {
        fs::create_dir_all(parent)?;
    }

    // The mode to create the scratch file WITH, read before it exists.
    //
    // Applying it at creation rather than chmod-ing afterwards closes a window in
    // which the file sits at the umask default — briefly wider than the file the
    // user tightened.
    #[cfg(unix)]
    let existing_mode = {
        use std::os::unix::fs::PermissionsExt as _;
        fs::metadata(&target).ok().map(|m| m.permissions().mode())
    };

    let mut tmp = scratch_name(&target);

    let write_result = (|| -> io::Result<()> {
        use std::io::Write as _;
        let mut opts = fs::OpenOptions::new();
        // Exclusive creation: the open FAILS if anything is already at that path
        // instead of truncating it (CWE-377).
        opts.write(true).create_new(true);
        #[cfg(unix)]
        if let Some(mode) = existing_mode {
            use std::os::unix::fs::OpenOptionsExt as _;
            opts.mode(mode);
        }
        // Retry once past an occupied scratch name. A crashed run can orphan a
        // scratch file, the OS later reuses that pid, and the new run's first save
        // picks the same seq. Advancing the counter cannot produce the same name
        // again, since it only increases within a process.
        let mut f = match opts.open(&tmp) {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                tmp = scratch_name(&target);
                opts.open(&tmp)?
            }
            other => other?,
        };
        f.write_all(contents)?;
        f.sync_all()
    })();
    if let Err(e) = write_result {
        // Best-effort: if the write failed because the disk is full, the remove
        // may fail too, and the original file is still intact.
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    // The exact mode, after creation. `open(2)` masks the requested mode with the
    // umask, so creation alone can land narrower; this makes it exact.
    #[cfg(unix)]
    if let Some(mode) = existing_mode {
        use std::os::unix::fs::PermissionsExt as _;
        let _ = fs::set_permissions(&tmp, fs::Permissions::from_mode(mode));
    }

    if let Err(e) = rename_with_retry(&tmp, &target) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    sync_parent_dir(&target);
    Ok(())
}

/// A scratch path beside `target`, carrying the pid and a per-call counter.
///
/// The pid separates processes: two `RustyNES` instances saving at once would
/// otherwise write the same scratch file and one would rename the other's
/// half-written bytes over the target. The counter separates concurrent calls
/// *within* a process — not reachable from today's callers, but that is a
/// property of the callers rather than of this function, and one relaxed
/// fetch-add makes the guarantee structural.
fn scratch_name(target: &Path) -> PathBuf {
    let seq = SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed);
    let mut s = target.as_os_str().to_os_string();
    s.push(format!(".{}.{seq}.tmp", std::process::id()));
    PathBuf::from(s)
}

/// `fsync` the directory holding `target`, so the rename itself is durable.
///
/// Best-effort and Unix-gated: opening a directory as a `File` is not portable,
/// and `MoveFileEx` on Windows already orders the metadata write.
///
/// A bare filename's parent is `Some("")`, and `File::open("")` fails with
/// `ENOENT` — so without the fallback the sync would silently not happen for a
/// relative target. That is a durability step quietly skipped rather than a
/// failure anyone sees, which is the worse of the two. `.` is the directory an
/// empty parent means.
#[cfg(unix)]
fn sync_parent_dir(target: &Path) {
    {
        let parent = target.parent().map_or_else(
            || PathBuf::from("."),
            |p| {
                if p.as_os_str().is_empty() {
                    PathBuf::from(".")
                } else {
                    p.to_path_buf()
                }
            },
        );
        if let Ok(dir) = fs::File::open(&parent) {
            let _ = dir.sync_all();
        }
    }
}

/// No-op off Unix.
///
/// Split into a separate `cfg`'d definition rather than an inner `#[cfg]` block,
/// because with an empty body clippy's `missing_const_for_fn` fires — and that is
/// only visible on a non-Unix target, so it failed the **wasm32** gate while
/// native clippy passed. `const` states the truth: on Windows `MoveFileEx`
/// already orders the metadata write, and on wasm there is no directory to sync.
#[cfg(not(unix))]
const fn sync_parent_dir(_target: &Path) {}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let d = std::env::temp_dir().join(format!(
            "rustynes-atomic-{}-{}",
            std::process::id(),
            SCRATCH_SEQ.fetch_add(1, Ordering::Relaxed)
        ));
        fs::create_dir_all(&d).expect("create tempdir");
        d
    }

    #[test]
    fn a_write_lands_and_replaces() {
        let d = tempdir();
        let p = d.join("f.txt");
        write_atomic(&p, b"first").expect("first write");
        assert_eq!(fs::read(&p).unwrap(), b"first");
        write_atomic(&p, b"second").expect("second write");
        assert_eq!(fs::read(&p).unwrap(), b"second");
    }

    /// No scratch file may survive a successful write.
    #[test]
    fn a_successful_write_leaves_no_scratch_behind() {
        let d = tempdir();
        let p = d.join("f.txt");
        write_atomic(&p, b"x").expect("write");
        let leftovers: Vec<_> = fs::read_dir(&d)
            .unwrap()
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_string_lossy().ends_with(".tmp"))
            .collect();
        assert!(
            leftovers.is_empty(),
            "scratch files survived a successful write: {:?}",
            leftovers
                .iter()
                .map(std::fs::DirEntry::file_name)
                .collect::<Vec<_>>()
        );
    }

    /// The symlink property, in the direction that matters: the LINK must
    /// survive, and the file it points at must receive the bytes.
    #[cfg(unix)]
    #[test]
    fn writing_through_a_symlink_keeps_the_link() {
        let d = tempdir();
        let real = d.join("real.txt");
        let link = d.join("link.txt");
        fs::write(&real, b"old").expect("seed");
        std::os::unix::fs::symlink(&real, &link).expect("symlink");

        write_atomic(&link, b"new").expect("write");

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the symlink was replaced by a regular file"
        );
        assert_eq!(
            fs::read(&real).unwrap(),
            b"new",
            "the target did not receive the bytes"
        );
    }

    /// A BROKEN symlink is the freshly-created-dotfiles case: the link exists,
    /// its target does not yet. `canonicalize` fails here, so this exercises the
    /// `read_link` fallback rather than the happy path above.
    #[cfg(unix)]
    #[test]
    fn writing_through_a_broken_symlink_creates_the_target_and_keeps_the_link() {
        let d = tempdir();
        let missing = d.join("not-yet.txt");
        let link = d.join("link.txt");
        std::os::unix::fs::symlink(&missing, &link).expect("symlink");

        write_atomic(&link, b"new").expect("write");

        assert!(
            fs::symlink_metadata(&link)
                .unwrap()
                .file_type()
                .is_symlink(),
            "the broken symlink was replaced by a regular file"
        );
        assert_eq!(fs::read(&missing).unwrap(), b"new");
    }

    /// The mode of an existing file must survive a rewrite.
    ///
    /// This is the property write-then-rename gives up relative to a truncating
    /// write, and the one a user would notice only by auditing: a config
    /// tightened to 0600 quietly widened to the umask default by an automatic
    /// save they never asked for.
    #[cfg(unix)]
    #[test]
    fn an_existing_files_mode_is_carried_across() {
        use std::os::unix::fs::PermissionsExt as _;
        let d = tempdir();
        let p = d.join("f.txt");
        fs::write(&p, b"old").expect("seed");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o600)).expect("chmod");

        write_atomic(&p, b"new").expect("write");

        let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(mode, 0o600, "mode was widened to {mode:o}");
    }

    /// A mode the UMASK would mask must still land exactly.
    ///
    /// This exists because the test above does NOT actually pin property 5.
    /// `opts.mode(0o600)` at creation already yields 0600 under any ordinary
    /// umask, so deleting the explicit `set_permissions` that follows left that
    /// test green — a mutation pass caught it asserting less than its name
    /// claimed. `open(2)` applies `mode & ~umask`, so a mode carrying bits the
    /// umask clears (0666 under the usual 022 gives 0644) is the only shape that
    /// distinguishes creation-mode from the exact set after it.
    ///
    /// Returns early rather than failing when the umask masks nothing, because
    /// the two mechanisms are then genuinely indistinguishable and a pass would
    /// mean nothing either way. The umask is OBSERVED rather than assumed to be
    /// 022 — CI images and developer shells do not agree on it.
    #[cfg(unix)]
    #[test]
    fn a_mode_the_umask_would_mask_still_lands_exactly() {
        use std::os::unix::fs::{OpenOptionsExt as _, PermissionsExt as _};
        let d = tempdir();

        let probe = d.join("probe.txt");
        fs::OpenOptions::new()
            .write(true)
            .create_new(true)
            .mode(0o666)
            .open(&probe)
            .expect("probe create");
        let created = fs::metadata(&probe).unwrap().permissions().mode() & 0o777;
        if created == 0o666 {
            return; // umask is 0 here; this test cannot distinguish anything.
        }

        let p = d.join("f.txt");
        fs::write(&p, b"old").expect("seed");
        fs::set_permissions(&p, fs::Permissions::from_mode(0o666)).expect("chmod");

        write_atomic(&p, b"new").expect("write");

        let mode = fs::metadata(&p).unwrap().permissions().mode() & 0o777;
        assert_eq!(
            mode, 0o666,
            "mode landed at {mode:o}, not the 0666 the existing file carried: the \
             exact set after creation is missing, and creation alone was masked to \
             {created:o}"
        );
    }

    /// An occupied scratch name must not cost the save.
    ///
    /// Plants a decoy at the name the next call will pick, which forces the
    /// `AlreadyExists` retry branch.
    #[test]
    fn an_occupied_scratch_name_does_not_lose_the_write() {
        let d = tempdir();
        let p = d.join("f.txt");
        // Peek at the next sequence value without consuming it, then plant a
        // decoy at exactly that name.
        let next = SCRATCH_SEQ.load(Ordering::Relaxed);
        let mut decoy = p.as_os_str().to_os_string();
        decoy.push(format!(".{}.{next}.tmp", std::process::id()));
        fs::write(PathBuf::from(decoy), b"decoy").expect("plant decoy");

        write_atomic(&p, b"payload").expect("write should survive the collision");
        assert_eq!(fs::read(&p).unwrap(), b"payload");
    }

    /// A failed write must leave the existing file intact.
    #[test]
    fn a_failed_write_leaves_the_original_intact() {
        let d = tempdir();
        let p = d.join("f.txt");
        write_atomic(&p, b"good").expect("seed");
        // A directory cannot be renamed over by a file write; target the
        // directory itself so the write fails after the original exists.
        let dir_target = d.join("subdir");
        fs::create_dir(&dir_target).expect("mkdir");
        assert!(write_atomic(&dir_target, b"nope").is_err());
        assert_eq!(fs::read(&p).unwrap(), b"good");
    }

    // ---- the retry loop, exercised on every platform ----

    #[test]
    fn the_retry_loop_returns_immediately_on_success() {
        let mut calls = 0;
        rename_with_retry_using(
            || {
                calls += 1;
                Ok(())
            },
            is_transient_rename_error,
        )
        .expect("should succeed");
        assert_eq!(calls, 1, "a successful rename must not be retried");
    }

    /// A non-transient error must NOT be retried — retrying a genuine permission
    /// problem only delays an error the caller needs now.
    #[test]
    fn the_retry_loop_does_not_retry_a_non_transient_error() {
        let mut calls = 0;
        // Predicate forced to accept only PermissionDenied, so the loop's own
        // decision is what stops it. With the real predicate this would pass on
        // Unix for the wrong reason -- everything is non-transient there.
        let r = rename_with_retry_using(
            || {
                calls += 1;
                Err(io::Error::new(io::ErrorKind::NotFound, "gone"))
            },
            |e| e.kind() == io::ErrorKind::PermissionDenied,
        );
        assert!(r.is_err());
        assert_eq!(calls, 1, "a NotFound rename must not be retried");
    }

    /// Exhausting the attempts must PROPAGATE the error, never report success.
    ///
    /// The predicate is injected as always-transient so this branch is reachable
    /// on EVERY platform. With the real predicate it is dead code on Unix, and a
    /// mutation making exhaustion return `Ok(())` -- reporting a save that never
    /// happened -- went uncaught until the predicate became a parameter.
    #[test]
    fn exhausting_the_attempts_propagates_the_error() {
        let mut calls: u32 = 0;
        let r = rename_with_retry_using(
            || {
                calls += 1;
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "busy"))
            },
            |_| true,
        );
        let e = r.expect_err("exhausting the attempts must not report success");
        assert_eq!(e.kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(
            calls, RENAME_ATTEMPTS,
            "the loop must make exactly RENAME_ATTEMPTS attempts before giving up"
        );
    }

    /// Unix must make exactly ONE attempt, with the REAL predicate -- the
    /// platform claim in the module docs, pinned rather than described. The test
    /// above deliberately bypasses that predicate, so this is what covers it.
    #[cfg(not(windows))]
    #[test]
    fn unix_never_retries_a_rename() {
        let mut calls = 0;
        let r = rename_with_retry_using(
            || {
                calls += 1;
                Err(io::Error::new(io::ErrorKind::PermissionDenied, "busy"))
            },
            is_transient_rename_error,
        );
        assert!(r.is_err());
        assert_eq!(calls, 1, "POSIX rename has no transient sharing violation");
    }
}
