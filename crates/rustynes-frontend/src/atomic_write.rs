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
//!
//! # Two failures that were reported as success, and are not any more
//!
//! Found in review of the change that introduced this module, and worth recording
//! because both had the same shape: an error deliberately discarded at a call
//! site, under a comment explaining the *rest* of the operation.
//!
//! - **`set_permissions` (property 5).** Discarded with `let _ =`. The mode being
//!   applied is the mode the target **already had**, so a failure replaces a file
//!   at `0600` with one at the umask default — **wider than what it replaced** —
//!   and reports success. It now propagates, after removing the scratch file.
//! - **The parent-directory `sync_all` (property 6).** Both the `File::open` and
//!   the `sync_all` were discarded, so the entire durability barrier could be a
//!   no-op while the table above claimed "yes" for Unix. It now propagates —
//!   *except* for the two errnos that mean the filesystem does not offer the
//!   barrier (`EINVAL`, and `EBADF` on some network mounts), since failing a save
//!   outright on those mounts would be a worse answer than proceeding. `EIO` —
//!   the exact condition the sync exists to detect — no longer passes as success.
//!
//! Both decisions are **extracted into named functions** (`apply_mode_using`,
//! `directory_fsync_is_unsupported`) for the same reason `rename_with_retry_using`
//! is: on Unix neither failure can be arranged in a unit test against a file this
//! process just created and owns, so hard-wired call sites would leave both
//! propagation paths permanently unexercised. An unexercised error path is how the
//! swallowed versions survived review to begin with.
//!
//! # One orphan was not enough
//!
//! Property 7's collision handling was a **single** retry, on the reasoning that
//! advancing the counter cannot repeat a name within a process. True, and beside
//! the point: the collision comes from a *previous* process. A run that crashed
//! mid-session orphans one scratch file per save it made, and the OS reusing that
//! pid restarts the counter at zero -- so two orphans defeat one retry and the
//! save fails outright, for a reason the user cannot act on. It is now a loop
//! bounded at `SCRATCH_ATTEMPTS`; bounded rather than bare, because a directory
//! rejecting creation for a persistent reason would otherwise hang, and a hang is
//! a worse answer than an error.
//!
//! The exhaustion branch is driven through `open_fresh_scratch` rather than
//! through `write_atomic`, and that is not a stylistic choice. Reaching it the
//! other way means predicting the process-global `SCRATCH_SEQ` and planting a
//! decoy at every name the call will pick -- a prediction that **races**, because
//! `cargo test` runs in parallel and every sibling test calling `write_atomic`
//! consumes sequence values. Measured rather than assumed: a serialising mutex
//! over the three tests that *peek* at the counter still failed 2 runs in 5,
//! because the tests doing the consuming are precisely the ones that never look
//! at it. The single-decoy test had been latently flaky since it was written and
//! had simply never lost the race.

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

/// How many scratch names to try before giving up.
///
/// One per orphaned scratch file left by a crashed run whose pid the OS later
/// reused. Eight is far past any plausible orphan count and still terminates
/// promptly if the directory is rejecting creation for a persistent reason --
/// which is the case a bare `loop` would hang on.
const SCRATCH_ATTEMPTS: u32 = 8;

/// How many times to attempt the rename before giving up.
///
/// Only ever more than one on Windows (see `is_transient_rename_error`). Five
/// attempts sleep FOUR times — the loop returns on the fifth failure rather than
/// backing off after it — so the worst case is 10 + 20 + 40 + 80 = **150 ms**,
/// which is a long time in a UI frame and a short one against losing a save the
/// user believes happened. (This read 310 ms until review; that would be the
/// figure if a fifth sleep of 160 ms happened, and it does not.)
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
    // A BROKEN chain, followed by hand: `canonicalize` fails outright when the
    // final target does not exist, so it cannot resolve `link1 -> link2 ->
    // missing`. Following one level was enough for the dotfiles case that
    // motivated this and wrong for the general one -- it would replace `link2`
    // with a regular file rather than writing through to where the chain points.
    //
    // Bounded at `SYMLINK_DEPTH`, because a chain can be a CYCLE (`a -> b -> a`)
    // and `read_link` succeeds forever on one. On exhaustion the last resolved
    // path is returned rather than an error: this function's whole job is to pick
    // a write target, and a pathological chain should degrade to "write where you
    // were told", not fail a save.
    let mut cur = path.to_path_buf();
    for _ in 0..SYMLINK_DEPTH {
        let Ok(dest) = fs::read_link(&cur) else {
            break;
        };
        cur = if dest.is_absolute() {
            dest
        } else {
            match cur.parent() {
                Some(dir) => dir.join(dest),
                None => dest,
            }
        };
    }
    cur
}

/// How many broken-symlink hops to follow before giving up.
///
/// Bounded because a chain can be a cycle, on which `read_link` succeeds
/// indefinitely.
///
/// Matches Linux's own `MAXSYMLINKS` of 40 rather than picking a smaller number.
/// The earlier value of 8 was justified as "far past any real dotfiles
/// arrangement", which is true and beside the point: where the kernel would
/// resolve a chain and this function gives up, the two disagree about where the
/// file *is*, and the write lands somewhere the user did not mean. Costs one
/// `read_link` per level, and only on a chain already known to be broken.
const SYMLINK_DEPTH: usize = 40;

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
pub fn is_transient_rename_error(e: &io::Error) -> bool {
    // `cfg!`, not `#[cfg]`, and that is the whole point. `cfg!` is a compile-time
    // boolean in an ordinary expression, so the Windows predicate is PARSED,
    // TYPE-CHECKED and borrow-checked on every platform, while `&&` short-circuits
    // it away on non-Windows and the optimizer drops the branch entirely. Runtime
    // behaviour is identical to the `#[cfg]` form; what changes is that a Linux
    // `cargo check` now compiles the Windows logic.
    //
    // The `#[cfg]` form is what let a defect through: the body was `const` and
    // called `io::Error::kind`, which is not a `const fn`, so it was `E0015` on
    // Windows and invisible on Linux, where the branch was never compiled. PR runs
    // here are Linux-only and the full matrix runs on `main`, so it would have
    // turned `main` red after merge. Caught in review, not by a gate.
    cfg!(windows) && is_windows_sharing_violation(e)
}

/// The Windows half of the predicate, compiled on **every** platform.
///
/// Two reasons, and the second is why it exists at all.
///
/// It is not `const`: `io::Error::kind` is not a `const fn`, so a `const`
/// wrapper is `E0015: cannot call non-const method` — but only where the body is
/// compiled. `is_transient_rename_error` was `const` with the call behind
/// `#[cfg(windows)]`, so it compiled cleanly on Linux and would have failed the
/// Windows job **after merge**: PR runs are Linux-only here, and the full matrix
/// runs on `main`. Caught in review rather than by a gate.
///
/// And it is NOT `cfg`-gated, which is the actual fix. Windows-only code behind a
/// `#[cfg]` is never type-checked by a Linux PR build, so any error in it is
/// invisible until the platform that compiles it runs — which is exactly what
/// happened. Reached through `cfg!(windows) && …` instead, the logic is compiled
/// everywhere and exercised by the test below on whatever platform the suite runs.
fn is_windows_sharing_violation(e: &io::Error) -> bool {
    // `MoveFileEx` reports both `ERROR_ACCESS_DENIED` and `ERROR_SHARING_VIOLATION`
    // as `PermissionDenied` through `std::io`, and only the second is transient —
    // std does not distinguish them, so the retry covers both.
    matches!(e.kind(), io::ErrorKind::PermissionDenied)
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

/// Apply `mode` to `path` through `op`.
///
/// The indirection exists so a test can reach the failure branch. On Unix
/// `set_permissions` on a file this process just created and owns does not fail
/// for any cause a unit test can arrange, so a hard-wired call would leave the
/// propagation path permanently unexercised -- and an unexercised error path is
/// how the swallowed version survived review in the first place.
#[cfg(unix)]
fn apply_mode_using<F>(path: &Path, mode: u32, op: F) -> io::Result<()>
where
    F: Fn(&Path, u32) -> io::Result<()>,
{
    op(path, mode)
}

/// The real mode application: `chmod` to exactly `mode`.
#[cfg(unix)]
fn set_permissions_mode(path: &Path, mode: u32) -> io::Result<()> {
    use std::os::unix::fs::PermissionsExt as _;
    fs::set_permissions(path, fs::Permissions::from_mode(mode))
}

/// Open a scratch file, advancing past names a crashed run left behind.
///
/// `open` must pick a **fresh** name each call and return the opened file. The
/// loop retries only `AlreadyExists`; every other error propagates on the first
/// occurrence.
///
/// Bounded at `SCRATCH_ATTEMPTS`, because a bare `loop` would hang on a directory
/// that rejects creation for some persistent reason -- and a hang is a worse
/// answer than an error.
///
/// # Why this is a separate function
///
/// The single retry it replaced was correct for one orphan and wrong for two, and
/// review found that rather than a test, because the only way to exercise the
/// exhaustion branch through `write_atomic` is to predict the process-global
/// `SCRATCH_SEQ` and plant a decoy at every name it will pick. That prediction
/// **races**: `cargo test` runs in parallel and any sibling test calling
/// `write_atomic` consumes sequence values, so the decoys land at names nothing
/// asks for. Measured, not theorised -- a serialising mutex over the three tests
/// that peek at the counter still failed 2 runs in 5, because the tests doing the
/// consuming are the ones that never look at it.
///
/// Driving the loop directly removes the global from the test entirely. Same
/// reasoning as `rename_with_retry_using` two definitions up.
fn open_fresh_scratch<T, F>(mut open: F) -> io::Result<T>
where
    F: FnMut() -> io::Result<T>,
{
    let mut attempt: u32 = 0;
    loop {
        match open() {
            Err(e) if e.kind() == io::ErrorKind::AlreadyExists => {
                attempt += 1;
                if attempt >= SCRATCH_ATTEMPTS {
                    return Err(e);
                }
            }
            other => return other,
        }
    }
}

/// Write `contents` to `path` atomically and durably.
///
/// Creates the parent directory if needed, resolves a symlinked target, writes to
/// an exclusively-created sibling scratch file, `fsync`s it, carries the existing
/// file's mode across on Unix, renames over the target, and syncs the parent
/// directory.
///
/// On failure **before the rename** the scratch file is removed and the existing
/// file is left untouched — a stale-but-valid file is the one worth keeping.
///
/// # One failure happens AFTER the target is replaced
///
/// The parent-directory sync is the last step, and it necessarily runs *after*
/// the rename it exists to make durable. So a `sync_parent_dir` failure returns
/// `Err` from a call in which **the target was successfully replaced** — the new
/// contents are on disk and were `fsync`ed before the rename; what is uncertain
/// is whether the directory entry survives a power loss.
///
/// This is stated rather than hidden because the two obvious alternatives are
/// both worse. Swallowing it is the defect review found in the first version of
/// this module: a genuine `EIO` from the storage layer reported as success.
/// Rolling the rename back would mean writing the old contents again, turning a
/// durability warning into a second full write that can itself fail.
///
/// Callers that distinguish "not written" from "written, durability unconfirmed"
/// should treat an error from this function as the latter only when the target's
/// mtime has advanced; none in this repository need to, since all of them
/// surface the error to the user rather than acting on it.
///
/// # Errors
///
/// Returns the underlying [`io::Error`] from any step. A rename that fails after
/// `RENAME_ATTEMPTS` transient failures returns the last error rather than
/// succeeding quietly. A post-rename sync failure is wrapped so its message says
/// the data was written; see `post_rename_sync_error`.
///
/// (A plain code span, not an intra-doc link: `write_atomic` is public and that function
/// is private, which `rustdoc::private-intra-doc-links` rejects under `-D warnings`. Same
/// rule this repo already applies to feature-gated dependency names.)
pub fn write_atomic(path: &Path, contents: &[u8]) -> io::Result<()> {
    write_atomic_with(path, contents, scratch_name, sync_parent_dir)
}

/// `write_atomic`, with the scratch-name source and the parent sync injected.
///
/// The seam exists for one specific test. The exhaustion branch -- where every
/// candidate name is occupied and nothing of ours is ever created -- is where the
/// cleanup must NOT delete anything, and reaching it through the real
/// `scratch_name` means predicting the process-global `SCRATCH_SEQ` and planting
/// a decoy at every name it will pick. That prediction races every parallel test
/// that calls `write_atomic` (see `open_fresh_scratch`).
///
/// The parent sync is injected for the same reason and a different failure: it
/// runs AFTER the rename, so its error is returned from a call in which the
/// target was already replaced. Asserting that contract needs a sync that fails
/// on demand, and the real one only fails on a directory that is writable but not
/// readable -- not something a unit test should be arranging.
///
/// It was not obvious the first seam was needed. The first attempt at the test forced
/// a failure by writing to a directory, which fails at the *rename* -- a branch
/// where the scratch file genuinely is ours -- so it exercised the wrong path
/// entirely and passed against both the fix and the defect. Two mutations
/// reported NOT CAUGHT, which is the only reason that was noticed.
///
/// # Errors
///
/// As [`write_atomic`].
fn write_atomic_with<N, S>(
    path: &Path,
    contents: &[u8],
    mut scratch: N,
    sync_parent: S,
) -> io::Result<()>
where
    N: FnMut(&Path) -> PathBuf,
    S: Fn(&Path) -> io::Result<()>,
{
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

    let mut tmp: Option<PathBuf> = None;

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
        // Advance past an occupied scratch name, up to `SCRATCH_ATTEMPTS` times.
        // A crashed run can orphan scratch files, the OS later reuses that pid,
        // and the new run's counter restarts at zero -- so its first save picks a
        // name that already exists. Advancing cannot repeat a name within a
        // process, since the counter only increases.
        //
        // This was a single retry, which review correctly showed is not enough:
        // a crashed run that orphaned BOTH `.pid.0.tmp` and `.pid.1.tmp` defeats
        // it, and the save fails outright for a reason the user cannot act on. A
        // bounded loop costs one `open` per orphan and is bounded so a directory
        // that rejects creation for some other persistent reason still terminates.
        //
        // `tmp` is assigned ONLY on a successful create, which is load-bearing
        // rather than tidy: on exhaustion the last name tried is a file that
        // already existed and belongs to somebody else -- an orphan, or a scratch
        // file a colliding instance is actively writing. Assigning it and then
        // running the cleanup below would delete that file. Review caught this;
        // the bug predated the loop (a single retry could reach it too) and the
        // loop widened it from one chance to eight.
        let (created, mut f) = open_fresh_scratch(|| {
            let candidate = scratch(&target);
            opts.open(&candidate).map(|f| (candidate, f))
        })?;
        tmp = Some(created);
        f.write_all(contents)?;
        f.sync_all()
    })();
    if let Err(e) = write_result {
        // Only if this process created it. `None` means the scratch open never
        // succeeded, so there is nothing of ours to remove and anything at those
        // names belongs to someone else.
        //
        // Best-effort past that: if the write failed because the disk is full,
        // the remove may fail too, and the original file is still intact.
        if let Some(t) = &tmp {
            let _ = fs::remove_file(t);
        }
        return Err(e);
    }
    // Past this point the create succeeded, so the scratch file is ours.
    let tmp = tmp.expect("the scratch file exists once the write succeeded");

    // The exact mode, after creation. `open(2)` masks the requested mode with the
    // umask, so creation alone can land narrower; this makes it exact.
    //
    // The error PROPAGATES, and the scratch file is removed first. Swallowing it
    // was wrong in the one direction that matters: the mode being restored is the
    // mode the file already had, so a target at 0600 whose `set_permissions`
    // fails is replaced by one at the umask default -- which is WIDER. Reporting
    // success there hands the caller a file with weaker permissions than the one
    // it replaced, and says nothing. Found in review of the PR that introduced it.
    #[cfg(unix)]
    if let Some(mode) = existing_mode
        && let Err(e) = apply_mode_using(&tmp, mode, set_permissions_mode)
    {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    if let Err(e) = rename_with_retry(&tmp, &target) {
        let _ = fs::remove_file(&tmp);
        return Err(e);
    }

    sync_parent(&target).map_err(|e| post_rename_sync_error(&e))
}

/// Label a parent-sync failure as post-rename, so the message cannot be read as
/// "nothing was written".
///
/// The kind is preserved so callers matching on [`io::ErrorKind`] still see the
/// real cause; only the message gains the qualifier. Separated from the call site
/// so a test can assert the wording without needing a directory that is writable
/// but not readable.
fn post_rename_sync_error(e: &io::Error) -> io::Error {
    io::Error::new(
        e.kind(),
        format!(
            "the file was written and renamed into place, but its directory entry \
             could not be synced, so the rename may not survive a power loss: {e}"
        ),
    )
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
fn sync_parent_dir(target: &Path) -> io::Result<()> {
    let parent = target
        .parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or_else(|| Path::new("."));
    let dir = fs::File::open(parent)?;
    match dir.sync_all() {
        Ok(()) => Ok(()),
        Err(e) if directory_fsync_is_unsupported(&e) => Ok(()),
        Err(e) => Err(e),
    }
}

/// True when a directory `fsync` failure means "this filesystem does not offer
/// the barrier", rather than "the write failed".
///
/// `fsync` on a directory is a POSIX-*blessed* idiom, not a POSIX-*guaranteed*
/// one. Some filesystems answer `EINVAL`; a few network mounts answer `EBADF`
/// because the descriptor was not opened writable. Neither means the rename
/// failed to commit, so treating them as write failures would break saving
/// outright on those mounts -- on a path that otherwise fully succeeded.
///
/// Every OTHER error propagates. **That is the change**: previously all of them
/// were discarded, so a genuine `EIO` from the storage layer -- the exact
/// condition the sync exists to detect -- was reported to the caller as success.
///
/// Extracted rather than written inline as a `matches!` guard for the reason
/// `rename_with_retry_using` was: a decision buried in a call site that cannot be
/// reached from a test is a decision nothing checks. Here the excusable and
/// non-excusable cases are both one function call away.
#[cfg(unix)]
pub fn directory_fsync_is_unsupported(e: &io::Error) -> bool {
    // `EBADF` is 9 on every Unix ABI RustyNES builds for. Named rather than
    // written inline so the comparison reads as a decision, and spelled out
    // rather than pulled from `libc`, which this crate does not depend on.
    const EBADF: i32 = 9;
    // `Unsupported` covers `ENOTSUP` / `EOPNOTSUPP`, which some network
    // filesystems return here instead of `EINVAL`. Added on review: the set is a
    // list of ways a filesystem says "no barrier available", and leaving one out
    // fails a save on that mount for a path that otherwise fully succeeded.
    matches!(
        e.kind(),
        io::ErrorKind::InvalidInput | io::ErrorKind::Unsupported
    ) || e.raw_os_error() == Some(EBADF)
}

/// No-op off Unix.
///
/// Split into a separate `cfg`'d definition rather than an inner `#[cfg]` block,
/// because with an empty body clippy's `missing_const_for_fn` fires — and that is
/// only visible on a non-Unix target, so it failed the **wasm32** gate while
/// native clippy passed. `const` states the truth: on Windows `MoveFileEx`
/// already orders the metadata write, and on wasm there is no directory to sync.
///
/// The `unnecessary_wraps` allow is the same story a second time. Once the Unix
/// arm started propagating its `sync_all` failure, this arm had to match its
/// signature — and an always-`Ok` return is exactly what that lint objects to, on
/// non-Unix targets only. Clippy's suggested fix (return `()`) would break the
/// parity the shared call site depends on, since `write_atomic` ends in
/// `sync_parent_dir(&target)` as its tail expression. Caught by the wasm32 gate
/// again, which native clippy cannot reach.
#[cfg(not(unix))]
#[allow(
    clippy::unnecessary_wraps,
    reason = "signature parity with the Unix arm, which genuinely can fail"
)]
const fn sync_parent_dir(_target: &Path) -> io::Result<()> {
    Ok(())
}

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

    /// TWO occupied scratch names must not cost the save either.
    ///
    /// The single retry this replaced survived one decoy and failed on two --
    /// which is the real scenario, not a contrived one: a run that crashed
    /// mid-session orphans one scratch file per save it had made, and the OS
    /// reusing that pid restarts the counter at zero. Found in review.
    #[test]
    fn two_occupied_scratch_names_do_not_lose_the_write() {
        let d = tempdir();
        let p = d.join("f.txt");
        let next = SCRATCH_SEQ.load(Ordering::Relaxed);
        for offset in 0..2 {
            let mut decoy = p.as_os_str().to_os_string();
            decoy.push(format!(".{}.{}.tmp", std::process::id(), next + offset));
            fs::write(PathBuf::from(decoy), b"decoy").expect("plant decoy");
        }
        write_atomic(&p, b"payload").expect("write should survive two collisions");
        assert_eq!(fs::read(&p).unwrap(), b"payload");
    }

    /// ...but the loop is BOUNDED: it gives up rather than spinning.
    ///
    /// Driven directly rather than through `write_atomic`, because reaching the
    /// exhaustion branch that way means predicting the process-global
    /// `SCRATCH_SEQ`, and that prediction races with every parallel test that
    /// calls `write_atomic`. See `open_fresh_scratch`.
    #[test]
    fn the_scratch_loop_is_bounded_and_reports_failure() {
        let mut calls = 0u32;
        let r: io::Result<()> = open_fresh_scratch(|| {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::AlreadyExists))
        });
        let e = r.expect_err("the scratch loop must give up rather than spin");
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);
        assert_eq!(
            calls, SCRATCH_ATTEMPTS,
            "the loop must try exactly SCRATCH_ATTEMPTS names before giving up"
        );
    }

    /// The loop must succeed as soon as a name is free, not keep going.
    #[test]
    fn the_scratch_loop_stops_at_the_first_free_name() {
        let mut calls = 0u32;
        let got = open_fresh_scratch(|| {
            calls += 1;
            if calls < 3 {
                Err(io::Error::from(io::ErrorKind::AlreadyExists))
            } else {
                Ok(calls)
            }
        })
        .expect("should succeed on the third name");
        assert_eq!(got, 3);
        assert_eq!(calls, 3, "the loop kept going past a free name");
    }

    /// Only `AlreadyExists` is retried. Anything else is a real failure and must
    /// surface on the first occurrence rather than after eight pointless tries.
    #[test]
    fn the_scratch_loop_does_not_retry_other_errors() {
        let mut calls = 0u32;
        let r: io::Result<()> = open_fresh_scratch(|| {
            calls += 1;
            Err(io::Error::from(io::ErrorKind::PermissionDenied))
        });
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
        assert_eq!(calls, 1, "a non-transient error was retried");
    }

    /// A bare relative filename must still get its parent synced.
    ///
    /// `Path::new("f.txt").parent()` is `Some("")`, not `None`, and
    /// `File::open("")` fails with `ENOENT`. The fallback to `.` was already
    /// documented as load-bearing, but nothing tested it — a mutation deleting
    /// the filter passed the whole suite.
    ///
    /// It matters MORE now than when it was written. While the sync was
    /// best-effort, losing the fallback meant a durability step quietly skipped.
    /// Now that it propagates, losing it means `write_atomic` **fails outright**
    /// for any relative target, which is a working call site turned into an error.
    ///
    /// Called directly rather than through `write_atomic`, because reaching it
    /// that way needs a relative target and therefore a `set_current_dir` — which
    /// is process-global and races the parallel suite, the same trap
    /// `open_fresh_scratch` documents.
    #[cfg(unix)]
    #[test]
    fn a_bare_relative_target_syncs_the_current_directory() {
        sync_parent_dir(Path::new("f.txt"))
            .expect("an empty parent must fall back to `.`, not open \"\"");
        sync_parent_dir(Path::new("./f.txt")).expect("an explicit `.` parent must work");
    }

    /// The Windows sharing-violation predicate, exercised on EVERY platform.
    ///
    /// This is the point of routing through `cfg!(windows) && …` rather than
    /// `#[cfg(windows)]`: the Windows logic is compiled and testable on Linux, so
    /// a defect in it fails here instead of on `main` after merge. The previous
    /// form hid an `E0015` (a `const fn` calling the non-const
    /// `io::Error::kind`) that no PR run could have seen.
    #[test]
    fn the_windows_sharing_violation_predicate_is_checked_on_every_platform() {
        assert!(
            is_windows_sharing_violation(&io::Error::from(io::ErrorKind::PermissionDenied)),
            "MoveFileEx reports a sharing violation as PermissionDenied; it must retry"
        );
        for kind in [
            io::ErrorKind::NotFound,
            io::ErrorKind::AlreadyExists,
            io::ErrorKind::InvalidInput,
        ] {
            assert!(
                !is_windows_sharing_violation(&io::Error::from(kind)),
                "{kind:?} is not transient; retrying it would only delay the error"
            );
        }
    }

    /// ...and on Unix the public predicate stays unconditionally false, so the
    /// single-attempt guarantee holds. `cfg!` must not have changed that.
    #[cfg(unix)]
    #[test]
    fn unix_never_treats_a_rename_error_as_transient() {
        for kind in [
            io::ErrorKind::PermissionDenied,
            io::ErrorKind::NotFound,
            io::ErrorKind::AlreadyExists,
        ] {
            assert!(
                !is_transient_rename_error(&io::Error::from(kind)),
                "POSIX rename has no sharing violation; {kind:?} must not be retried"
            );
        }
    }

    /// A post-rename sync failure must not read as "nothing was written".
    ///
    /// The parent-directory sync is the last step and necessarily runs *after*
    /// the rename it exists to make durable, so its failure returns `Err` from a
    /// call in which the target WAS replaced. That contradicts the plain reading
    /// of "on failure the existing file is left untouched", which is why the
    /// message now says what actually happened. Found in review.
    #[test]
    fn a_post_rename_sync_failure_says_the_data_was_written() {
        let e = post_rename_sync_error(&io::Error::from_raw_os_error(5));
        let msg = e.to_string();
        assert!(
            msg.contains("was written"),
            "the message must not imply the write was skipped: {msg}"
        );
        assert!(
            msg.contains("power loss"),
            "the message must name what is actually uncertain: {msg}"
        );
        assert_eq!(
            e.kind(),
            io::Error::from_raw_os_error(5).kind(),
            "the underlying kind must survive, or callers matching on it break"
        );
    }

    /// `write_atomic` must APPLY the post-rename label, not merely define it.
    ///
    /// The first test for this called `post_rename_sync_error` directly, which
    /// asserts the helper behaves and says nothing about whether the call site
    /// uses it — a mutation deleting the `map_err` came back NOT CAUGHT. Same
    /// lesson as the scratch-cleanup test two definitions up, in a new shape:
    /// testing a helper is not testing the code that was supposed to call it.
    ///
    /// Also pins the contract the label exists to describe: on a post-rename sync
    /// failure the call returns `Err` **and the target holds the new contents**.
    #[test]
    fn a_post_rename_sync_failure_still_leaves_the_new_contents_in_place() {
        let d = tempdir();
        let p = d.join("f.txt");
        fs::write(&p, b"old").expect("seed");

        let e = write_atomic_with(&p, b"new", scratch_name, |_| {
            Err(io::Error::from_raw_os_error(5)) // EIO
        })
        .expect_err("a failing parent sync must not report success");

        assert!(
            e.to_string().contains("was written"),
            "write_atomic did not apply the post-rename label: {e}"
        );
        assert_eq!(
            fs::read(&p).expect("target must exist"),
            b"new",
            "the rename had already happened; the target must hold the NEW bytes"
        );
    }

    /// A broken symlink CHAIN must resolve to its end, not to the middle.
    ///
    /// `canonicalize` cannot help here — it fails outright when the final target
    /// does not exist — so the chain is followed by hand. Following one level
    /// replaced `link2` with a regular file instead of writing through to where
    /// the chain points. Raised in review three times before it was fixed.
    #[cfg(unix)]
    #[test]
    fn a_broken_symlink_chain_resolves_to_its_end() {
        use std::os::unix::fs::symlink;
        let d = tempdir();
        let missing = d.join("final.txt");
        let link2 = d.join("link2");
        let link1 = d.join("link1");
        symlink(&missing, &link2).expect("link2 -> final");
        symlink(&link2, &link1).expect("link1 -> link2");

        write_atomic(&link1, b"payload").expect("write through the chain");

        assert_eq!(
            fs::read(&missing).expect("the chain's end must hold the payload"),
            b"payload"
        );
        assert!(
            fs::symlink_metadata(&link2)
                .expect("link2 must survive")
                .is_symlink(),
            "the intermediate link was replaced by a regular file"
        );
    }

    /// A symlink CYCLE must terminate rather than spin.
    ///
    /// `read_link` succeeds forever on `a -> b -> a`, so the resolver is bounded.
    /// It degrades to a write target rather than an error: picking where to write
    /// is this function's whole job, and a pathological chain should not fail a
    /// save.
    #[cfg(unix)]
    #[test]
    fn a_symlink_cycle_terminates() {
        use std::os::unix::fs::symlink;
        let d = tempdir();
        let a = d.join("a");
        let b = d.join("b");
        symlink(&b, &a).expect("a -> b");
        symlink(&a, &b).expect("b -> a");
        // The assertion is that this RETURNS at all.
        let resolved = resolve_write_target(&a);
        assert!(
            resolved == a || resolved == b,
            "a cycle must resolve to one of its own links, got {resolved:?}"
        );
    }

    /// A failed write must not delete a file it did not create.
    ///
    /// On exhaustion the last name tried is one that ALREADY EXISTED — an orphan
    /// from a crashed run, or a scratch file a colliding instance is actively
    /// writing. The cleanup used to remove it unconditionally, so a save that
    /// failed took another process's in-progress data with it. Found in review;
    /// the defect predated the bounded loop, which widened it from one chance to
    /// eight.
    ///
    /// Driven through `write_atomic_with` with a fixed name generator, so every
    /// attempt collides and the exhaustion branch is reached deterministically.
    /// An earlier version of this test forced failure by writing to a directory
    /// and passed against the defect, because that fails at the *rename*, where
    /// the scratch file genuinely is ours.
    #[test]
    fn a_failed_write_does_not_delete_a_file_it_did_not_create() {
        const OTHERS: &[u8] = b"someone elses in-progress data";

        let d = tempdir();
        let p = d.join("f.txt");
        let occupied = d.join("occupied.tmp");
        fs::write(&occupied, OTHERS).expect("plant");

        let taken = occupied.clone();
        let e = write_atomic_with(&p, b"payload", |_| taken.clone(), sync_parent_dir)
            .expect_err("every candidate name was occupied; this must fail");
        assert_eq!(e.kind(), io::ErrorKind::AlreadyExists);

        assert_eq!(
            fs::read(&occupied).expect("the other process's file must still exist"),
            OTHERS,
            "a failed write deleted a file it did not create"
        );
        assert!(!p.exists(), "a failed write must not create the target");
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

        // ...and it leaves NO scratch file behind.
        //
        // Scope, stated precisely, because a mutation established it: this reaches
        // the RENAME-failure cleanup, not the write-failure one. The scratch create,
        // write and `fsync` all succeed here; it is renaming a file over a directory
        // that fails. Removing that cleanup is caught; removing the write-failure
        // cleanup is NOT, and this test should not be read as covering it.
        //
        // Review suggested a `Drop` guard on the grounds that an early return via
        // `?` orphans the temporary. It does not: the `?` returns from the CLOSURE,
        // so `write_result` is `Err` and its cleanup runs — the same shape as the
        // branch asserted here. Only a PANIC can orphan a scratch file, and the
        // bounded scratch loop exists so that is survivable.
        // Matched on OUR scratch prefix rather than the `.tmp` extension: it is
        // what `scratch_name` actually produces, it cannot collide with a `.tmp`
        // this test did not create, and it sidesteps clippy's case-sensitivity
        // lint on extension comparisons -- which is a fair complaint about
        // matching an extension and irrelevant to matching a generated name.
        let ours = format!(".{}.", std::process::id());
        let strays: Vec<_> = fs::read_dir(&d)
            .expect("read tempdir")
            .filter_map(Result::ok)
            .map(|e| e.file_name().to_string_lossy().into_owned())
            .filter(|n| n.contains(&ours))
            .collect();
        assert!(
            strays.is_empty(),
            "a failed write orphaned scratch file(s): {strays:?}"
        );
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

    /// A failing mode application must PROPAGATE, not be reported as success.
    ///
    /// The direction matters and is why this was a blocking finding rather than a
    /// nitpick: the mode being applied is the mode the target *already had*, so a
    /// swallowed failure replaces a 0600 file with one at the umask default --
    /// **wider** than what it replaced -- and tells the caller nothing.
    #[cfg(unix)]
    #[test]
    fn a_failing_mode_application_propagates() {
        let dir = tempdir();
        let p = dir.join("m");
        let r = apply_mode_using(&p, 0o600, |_, _| {
            Err(io::Error::new(io::ErrorKind::PermissionDenied, "denied"))
        });
        assert!(r.is_err(), "a failing mode op reported success");
        assert_eq!(r.unwrap_err().kind(), io::ErrorKind::PermissionDenied);
    }

    /// ...and the succeeding case still succeeds, so the test above is not
    /// passing merely because the helper always fails.
    #[cfg(unix)]
    #[test]
    fn a_succeeding_mode_application_is_ok() {
        let dir = tempdir();
        let p = dir.join("m");
        fs::write(&p, b"x").expect("seed");
        apply_mode_using(&p, 0o600, set_permissions_mode).expect("real chmod failed");
    }

    /// The two excusable directory-fsync errors are excused, and nothing else is.
    ///
    /// `EIO` is the case that matters: it is the exact condition the parent-dir
    /// sync exists to detect, and the previous code discarded it along with
    /// everything else.
    #[cfg(unix)]
    #[test]
    fn only_the_unsupported_directory_fsync_errors_are_excused() {
        let einval = io::Error::from(io::ErrorKind::InvalidInput);
        assert!(
            directory_fsync_is_unsupported(&einval),
            "EINVAL must be excused -- some filesystems answer it for dir fsync"
        );
        let ebadf = io::Error::from_raw_os_error(9);
        assert!(
            directory_fsync_is_unsupported(&ebadf),
            "EBADF must be excused -- some network mounts answer it"
        );
        let unsupported = io::Error::from(io::ErrorKind::Unsupported);
        assert!(
            directory_fsync_is_unsupported(&unsupported),
            "ENOTSUP/EOPNOTSUPP must be excused -- some network filesystems \
             answer it instead of EINVAL"
        );

        for (errno, name) in [(5, "EIO"), (28, "ENOSPC"), (13, "EACCES")] {
            let e = io::Error::from_raw_os_error(errno);
            assert!(
                !directory_fsync_is_unsupported(&e),
                "{name} must PROPAGATE -- it is a real write failure, not an \
                 unsupported barrier"
            );
        }
    }
}
