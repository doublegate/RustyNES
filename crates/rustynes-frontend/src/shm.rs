//! v3.5.0 (CR-02) — named OS shared memory, for the script `comm.mmf*` bridge
//! and the speedrun memory-export interface (`T-SPEEDRUN-MEMIF`).
//!
//! Until v3.5.0 the "memory-mapped file" a Lua script wrote with
//! `comm.mmfWrite` was a `HashMap` inside the emulator process, so nothing
//! outside the emulator could read it, which was the whole point of the API (a
//! bot, an overlay or a splitter in another process). This module is the real
//! thing: a region of memory the operating system names, that any process run
//! by the same user can open by that name.
//!
//! # The platform mapping
//!
//! | Platform | Create / open | Name | Lifetime |
//! | --- | --- | --- | --- |
//! | Linux, macOS, the BSDs | `shm_open` + `ftruncate` + `mmap` | `/<name>` (on Linux, a file in `/dev/shm`) | until `shm_unlink`; the creator unlinks it on drop |
//! | Windows | `CreateFileMappingW` (pagefile-backed) + `MapViewOfFile` | `<name>` in the session's `Local\` namespace | until the last handle closes |
//!
//! The OS-level name carries the user (see "Threat model"): on POSIX the
//! object for `<name>` is `/<name>.<euid in lowercase hex>`, on Windows it is
//! `Local\<name>`. [`os_object_name`] spells it.
//!
//! These are the same objects Python's `multiprocessing.shared_memory`
//! opens by name, which is what `scripts/memif/reference_splitter.py` (the
//! speedrun gate's reference reader) uses on all three platforms; a C, C# or
//! Rust tool reaches them through the same calls.
//!
//! # Names
//!
//! [`validate_name`] accepts 1 to [`MAX_NAME_LEN`] characters of
//! `[A-Za-z0-9_.-]`. The bound is macOS's: `PSHMNAMLEN` is 31 bytes including
//! the leading `/`, which must also hold the `.<uid>` suffix (at most nine
//! bytes), and a longer name fails there with `ENAMETOOLONG` while working on
//! Linux, which is the portability trap the single limit avoids. No `/` and no
//! `\` are accepted, so a name can never become a path. Callers that take a
//! name from a script add their own prefix before calling this module
//! (`script_host` uses `rnmf-`), so a script cannot open an arbitrary
//! shared-memory object belonging to another program.
//!
//! # Threat model
//!
//! Shared-memory names are a namespace other local users can write into
//! (POSIX `/dev/shm` is world-writable; Windows' `Global\` is shared). A
//! hostile local user who pre-creates the name `RustyNES` is about to use
//! ("name squatting") could otherwise read what a script or the memif block
//! publishes, inject data a script then reads, or block the name. Three rules
//! close that, and an object that fails any of them is refused with
//! `PermissionDenied` naming the reason, never adopted:
//!
//! 1. **The name is per user.** POSIX names end in `.<effective uid, hex>`;
//!    Windows names live in the session's `Local\` namespace. Two users never
//!    compute the same object name, so a squatter has to guess ours on purpose,
//!    which rule 2 then defeats.
//! 2. **An existing object is checked before it is used.** POSIX: `fstat` must
//!    show our effective uid as the owner and no group or other permission bits
//!    (`mode & 0o077 == 0`). Windows: the mapping's owner SID must be the
//!    process token's user SID. This holds for [`SharedRegion::open`] and for
//!    [`SharedRegion::create`] finding the name taken.
//! 3. **What we create is owner-only.** POSIX objects are created with
//!    `O_EXCL` and mode `0600`; Windows mappings get the descriptor
//!    `D:P(A;;GA;;;OW)` (a protected DACL granting access only to the owner).
//!
//! What remains: another user can still squat the name to make `RustyNES`'s
//! create fail (denial of service, reported as `PermissionDenied`), and
//! anything running as the same user can read and write the region; that is
//! the same user, whom no file permission separates either.
//!
//! # Concurrency
//!
//! A region is memory another PROCESS may read or write at any moment, which
//! is outside what Rust's memory model can describe. Every byte copy into or
//! out of the mapping is therefore volatile ([`SharedRegion::write_bytes`],
//! [`SharedRegion::read_bytes`]), and the layouts built on top of it
//! (`script_host`'s MMF header, the memif block) publish through a sequence
//! counter read and written with atomics ([`SharedRegion::atomic_u32`]): the
//! writer makes it odd, writes, makes it even; a reader that sees it odd, or
//! sees it change across its copy, retries. That is the established
//! single-writer seqlock, and it is the reason a reader can never return a
//! torn record as a whole one.
//!
//! This is one of the few modules with `unsafe` in the frontend (the crate the
//! project allows it in). Each block carries a `// SAFETY:` note.

// The OS calls and the raw mapping are `unsafe` by nature; the workspace lint
// asks for each to be justified, which every block below is (and
// `clippy::undocumented_unsafe_blocks` checks that it is).
#![allow(unsafe_code)]

use std::io;
use std::ptr::NonNull;
use std::sync::atomic::AtomicU32;

/// The longest accepted name, in bytes: macOS `PSHMNAMLEN` (31) less the
/// leading `/` and the `.<uid>` suffix (a `.` and up to eight hex digits).
pub const MAX_NAME_LEN: usize = 21;

/// The OS-level object name for `name` (see the module docs): what another
/// program passes to `shm_open` (POSIX) or `OpenFileMappingW` (Windows).
#[must_use]
pub fn os_object_name(name: &str) -> String {
    sys::os_name(name)
}

/// Check `name` against the portable rule (see the module docs).
///
/// # Errors
///
/// `InvalidInput`, naming the rule, for an empty or over-long name or one with
/// a character outside `[A-Za-z0-9_.-]`.
pub fn validate_name(name: &str) -> io::Result<()> {
    let ok = !name.is_empty()
        && name.len() <= MAX_NAME_LEN
        && name
            .bytes()
            .all(|b| b.is_ascii_alphanumeric() || matches!(b, b'_' | b'.' | b'-'));
    if ok {
        Ok(())
    } else {
        Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            format!(
                "shared-memory name {name:?} must be 1-{MAX_NAME_LEN} characters of A-Z a-z 0-9 _ . -"
            ),
        ))
    }
}

/// A mapped, named shared-memory region.
///
/// Dropping it unmaps the view and, on POSIX, unlinks the name if this value
/// created it ([`Self::created`]). A region opened with [`Self::open`] is never
/// unlinked: it belongs to whoever made it.
pub struct SharedRegion {
    ptr: NonNull<u8>,
    len: usize,
    name: String,
    created: bool,
    #[cfg(windows)]
    handle: windows_sys::Win32::Foundation::HANDLE,
}

// SAFETY: the region is plain shared memory with no thread affinity: the
// pointer stays valid until `Drop` unmaps it, and every access through this
// type is a volatile copy or an atomic, so moving it to another thread (the
// script-IPC worker, the emulation thread) cannot create an access the owning
// thread could not have made. The Windows `HANDLE` is a process-wide kernel
// object handle, valid from any thread.
unsafe impl Send for SharedRegion {}

impl std::fmt::Debug for SharedRegion {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SharedRegion")
            .field("name", &self.name)
            .field("len", &self.len)
            .field("created", &self.created)
            .finish_non_exhaustive()
    }
}

impl SharedRegion {
    /// Create the region `name` with at least `len` bytes, or open it if it
    /// already exists and is at least that large. A region this call creates
    /// is zero-filled by the OS.
    ///
    /// # Errors
    ///
    /// An invalid name, `len == 0`, an existing region smaller than `len`
    /// that cannot be grown (macOS sizes an object once; Windows never
    /// resizes a mapping), or any OS failure.
    pub fn create(name: &str, len: usize) -> io::Result<Self> {
        validate_name(name)?;
        if len == 0 {
            return Err(io::Error::new(
                io::ErrorKind::InvalidInput,
                "a shared-memory region needs at least one byte",
            ));
        }
        sys::create(name, len)
    }

    /// Open an existing region `name`; its length is whatever its creator made.
    ///
    /// # Errors
    ///
    /// `NotFound` when no region has that name, an invalid name, or any OS
    /// failure.
    pub fn open(name: &str) -> io::Result<Self> {
        validate_name(name)?;
        sys::open(name)
    }

    /// The mapped length in bytes. On Windows a region opened by name reports
    /// its creator's size rounded up to a page, which is all the OS records.
    #[must_use]
    pub const fn len(&self) -> usize {
        self.len
    }

    /// `true` for a zero-length mapping, which this type never produces.
    #[must_use]
    pub const fn is_empty(&self) -> bool {
        self.len == 0
    }

    /// The name the region was created or opened under (without any `/`).
    #[must_use]
    pub fn name(&self) -> &str {
        &self.name
    }

    /// Whether this value created the region (and so unlinks it on POSIX).
    #[must_use]
    pub const fn created(&self) -> bool {
        self.created
    }

    /// Copy `src` into the region at `offset`, byte by byte, volatile.
    ///
    /// # Panics
    ///
    /// If `offset + src.len()` exceeds [`Self::len`]: every caller computes
    /// its offsets from a fixed layout it has already sized the region for, so
    /// an overrun is a programming error, not an input.
    pub fn write_bytes(&self, offset: usize, src: &[u8]) {
        let end = offset.checked_add(src.len()).expect("offset overflow");
        assert!(end <= self.len, "write {offset}..{end} past {}", self.len);
        let base = self.ptr.as_ptr();
        for (i, &b) in src.iter().enumerate() {
            // SAFETY: `offset + i < end <= self.len`, so the address is inside
            // the live mapping, which is writable (both platforms map it
            // read-write). Volatile, because another process may observe it.
            unsafe { base.add(offset + i).write_volatile(b) };
        }
    }

    /// Copy `dst.len()` bytes out of the region at `offset`, volatile.
    ///
    /// # Panics
    ///
    /// If the range exceeds [`Self::len`] (see [`Self::write_bytes`]).
    pub fn read_bytes(&self, offset: usize, dst: &mut [u8]) {
        let end = offset.checked_add(dst.len()).expect("offset overflow");
        assert!(end <= self.len, "read {offset}..{end} past {}", self.len);
        let base = self.ptr.as_ptr();
        for (i, d) in dst.iter_mut().enumerate() {
            // SAFETY: in bounds of the live mapping, as in `write_bytes`.
            *d = unsafe { base.add(offset + i).read_volatile() };
        }
    }

    /// The `u32` at `offset`, as an atomic shared with every other mapper.
    ///
    /// # Panics
    ///
    /// If `offset` is not 4-aligned or `offset + 4` exceeds [`Self::len`].
    #[must_use]
    // The `u8` -> `u32` pointer cast is aligned: see the SAFETY note.
    #[allow(clippy::cast_ptr_alignment)]
    pub fn atomic_u32(&self, offset: usize) -> &AtomicU32 {
        assert!(offset.is_multiple_of(4), "unaligned atomic at {offset}");
        assert!(
            offset + 4 <= self.len,
            "atomic at {offset} past {}",
            self.len
        );
        // SAFETY: the mapping base is page-aligned (both `mmap` and
        // `MapViewOfFile` return allocation-granularity addresses) and `offset`
        // is a multiple of 4, so the pointer is aligned for `AtomicU32`; it is
        // in bounds (checked above) and valid for as long as `&self` is,
        // because only `Drop` unmaps it. Every other access to these four
        // bytes, in this process, is through this same atomic.
        unsafe { AtomicU32::from_ptr(self.ptr.as_ptr().add(offset).cast::<u32>()) }
    }
}

impl Drop for SharedRegion {
    fn drop(&mut self) {
        sys::close(self);
    }
}

#[cfg(unix)]
mod sys {
    //! POSIX shared memory: `shm_open` / `ftruncate` / `mmap`.
    use super::{NonNull, SharedRegion, io};
    use std::ffi::CString;

    /// `/<name>.<euid hex>`: the per-user object name (threat-model rule 1).
    pub(super) fn os_name(name: &str) -> String {
        // SAFETY: `geteuid` has no preconditions and cannot fail.
        let uid = unsafe { libc::geteuid() };
        format!("/{name}.{uid:x}")
    }

    fn c_name(name: &str) -> io::Result<CString> {
        // `validate_name` already refused NUL, so this cannot fail; map it
        // anyway rather than unwrap.
        CString::new(os_name(name)).map_err(|e| io::Error::new(io::ErrorKind::InvalidInput, e))
    }

    /// The rule 2 predicate: owned by `me`, and no group or other bits. Split
    /// out so a test can exercise the uid half, which needs a second user to
    /// reach through the OS.
    // The mask reads as the permission bits it tests; `trailing_zeros` would not.
    #[allow(clippy::verbose_bit_mask)]
    pub(super) const fn owner_ok(uid: libc::uid_t, mode: u32, me: libc::uid_t) -> bool {
        uid == me && mode & 0o077 == 0
    }

    /// Threat-model rule 2: refuse an existing object that is not ours and
    /// owner-only. `name` is for the message.
    fn check_owner(fd: libc::c_int, name: &str) -> io::Result<()> {
        // SAFETY: an all-zero `stat` is a valid value of the plain C struct;
        // `fstat` only writes it.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `fd` is an open descriptor owned by the caller, and `st` is
        // a valid, writable `stat`.
        if unsafe { libc::fstat(fd, &raw mut st) } != 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: `geteuid` has no preconditions and cannot fail.
        let me = unsafe { libc::geteuid() };
        // `st_mode` is a u32 on Linux and a u16 on macOS.
        #[allow(clippy::useless_conversion)]
        let mode = u32::from(st.st_mode) & 0o777;
        if !owner_ok(st.st_uid, mode, me) {
            return Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing shared-memory object {}: owned by uid {} with mode {mode:03o}, \
                     expected uid {me} and no group/other access (another user may have \
                     created it)",
                    os_name(name),
                    st.st_uid
                ),
            ));
        }
        Ok(())
    }

    /// `shm_open`, papering over the variadic macOS prototype.
    fn shm_open(name: &CString, oflag: libc::c_int) -> libc::c_int {
        // The Apple prototype is variadic, where a `mode_t` (u16 there) is
        // promoted to an int, so the mode goes as `c_uint`; elsewhere it is a
        // plain `mode_t` parameter.
        #[cfg(target_vendor = "apple")]
        let mode: libc::c_uint = 0o600;
        #[cfg(not(target_vendor = "apple"))]
        let mode: libc::mode_t = 0o600;
        // SAFETY: `name` is a valid NUL-terminated string that outlives the
        // call; `shm_open` reads it and returns a new descriptor or -1.
        unsafe { libc::shm_open(name.as_ptr(), oflag, mode) }
    }

    fn file_len(fd: libc::c_int) -> io::Result<usize> {
        // SAFETY: an all-zero `stat` is a valid value of the plain C struct;
        // `fstat` only writes it.
        let mut st: libc::stat = unsafe { std::mem::zeroed() };
        // SAFETY: `fd` is an open descriptor owned by the caller, and `st` is
        // a valid, writable `stat`.
        if unsafe { libc::fstat(fd, &raw mut st) } != 0 {
            return Err(io::Error::last_os_error());
        }
        usize::try_from(st.st_size)
            .map_err(|_| io::Error::new(io::ErrorKind::InvalidData, "negative size"))
    }

    fn close_fd(fd: libc::c_int) {
        // SAFETY: `fd` is an open descriptor this module owns; it is closed
        // exactly once. The mapping, if any, stays valid after the close.
        unsafe { libc::close(fd) };
    }

    fn map(fd: libc::c_int, len: usize) -> io::Result<NonNull<u8>> {
        // SAFETY: a fresh shared read-write mapping of `len` bytes of the
        // object `fd` names, at an address the kernel chooses (null hint);
        // nothing else in this process aliases it.
        let p = unsafe {
            libc::mmap(
                std::ptr::null_mut(),
                len,
                libc::PROT_READ | libc::PROT_WRITE,
                libc::MAP_SHARED,
                fd,
                0,
            )
        };
        if p == libc::MAP_FAILED {
            return Err(io::Error::last_os_error());
        }
        NonNull::new(p.cast::<u8>()).ok_or_else(|| io::Error::other("mmap returned null"))
    }

    pub(super) fn create(name: &str, len: usize) -> io::Result<SharedRegion> {
        let c = c_name(name)?;
        // Exclusive first, so a region someone else made is never unlinked by
        // us later; fall back to opening it.
        let mut created = true;
        let mut fd = shm_open(&c, libc::O_RDWR | libc::O_CREAT | libc::O_EXCL);
        if fd < 0 {
            let err = io::Error::last_os_error();
            if err.raw_os_error() != Some(libc::EEXIST) {
                return Err(err);
            }
            created = false;
            fd = shm_open(&c, libc::O_RDWR);
            if fd < 0 {
                return Err(io::Error::last_os_error());
            }
            if let Err(e) = check_owner(fd, name) {
                close_fd(fd);
                return Err(e);
            }
        }
        let have = match file_len(fd) {
            Ok(n) => n,
            Err(e) => {
                close_fd(fd);
                return Err(e);
            }
        };
        if have < len {
            let Ok(want) = libc::off_t::try_from(len) else {
                close_fd(fd);
                return Err(io::Error::new(io::ErrorKind::InvalidInput, "too large"));
            };
            // SAFETY: `fd` is open read-write; `ftruncate` sets its size.
            // macOS refuses to resize an object that already has a size, and
            // the error is returned to the caller.
            if unsafe { libc::ftruncate(fd, want) } != 0 {
                let e = io::Error::last_os_error();
                close_fd(fd);
                if created {
                    // SAFETY: as in `close`; we made it, so we remove it.
                    unsafe { libc::shm_unlink(c.as_ptr()) };
                }
                return Err(e);
            }
        }
        let mapped = have.max(len);
        let ptr = map(fd, mapped);
        close_fd(fd);
        let ptr = match ptr {
            Ok(p) => p,
            Err(e) => {
                if created {
                    // SAFETY: as in `close`.
                    unsafe { libc::shm_unlink(c.as_ptr()) };
                }
                return Err(e);
            }
        };
        Ok(SharedRegion {
            ptr,
            len: mapped,
            name: name.to_owned(),
            created,
        })
    }

    pub(super) fn open(name: &str) -> io::Result<SharedRegion> {
        let c = c_name(name)?;
        let fd = shm_open(&c, libc::O_RDWR);
        if fd < 0 {
            return Err(io::Error::last_os_error());
        }
        if let Err(e) = check_owner(fd, name) {
            close_fd(fd);
            return Err(e);
        }
        let len = file_len(fd);
        let len = match len {
            Ok(0) => Err(io::Error::new(
                io::ErrorKind::InvalidData,
                "the region has no size yet",
            )),
            other => other,
        };
        let res = len.and_then(|len| map(fd, len).map(|p| (p, len)));
        close_fd(fd);
        let (ptr, len) = res?;
        Ok(SharedRegion {
            ptr,
            len,
            name: name.to_owned(),
            created: false,
        })
    }

    pub(super) fn close(region: &SharedRegion) {
        // SAFETY: `ptr`/`len` are exactly the mapping `map` returned, and
        // `Drop` runs once, so it is unmapped once; no reference into it
        // outlives `&self` (every accessor borrows `self`).
        unsafe { libc::munmap(region.ptr.as_ptr().cast(), region.len) };
        if region.created
            && let Ok(c) = c_name(&region.name)
        {
            // SAFETY: a valid C string naming the object this value created.
            unsafe { libc::shm_unlink(c.as_ptr()) };
        }
    }
}

#[cfg(windows)]
mod sys {
    //! Windows named file mappings over the paging file.
    use super::{NonNull, SharedRegion, io};
    use windows_sys::Win32::Foundation::{
        CloseHandle, ERROR_ALREADY_EXISTS, ERROR_SUCCESS, GetLastError, HANDLE,
        INVALID_HANDLE_VALUE, LocalFree,
    };
    use windows_sys::Win32::Security::Authorization::{
        ConvertStringSecurityDescriptorToSecurityDescriptorW, GetSecurityInfo, SDDL_REVISION_1,
        SE_KERNEL_OBJECT,
    };
    use windows_sys::Win32::Security::{
        EqualSid, GetTokenInformation, OWNER_SECURITY_INFORMATION, PSECURITY_DESCRIPTOR, PSID,
        SECURITY_ATTRIBUTES, TOKEN_QUERY, TOKEN_USER, TokenUser,
    };
    use windows_sys::Win32::System::Memory::{
        CreateFileMappingW, FILE_MAP_ALL_ACCESS, MEMORY_BASIC_INFORMATION,
        MEMORY_MAPPED_VIEW_ADDRESS, MapViewOfFile, OpenFileMappingW, PAGE_READWRITE,
        UnmapViewOfFile, VirtualQuery,
    };
    use windows_sys::Win32::System::Threading::{GetCurrentProcess, OpenProcessToken};

    /// `Local\<name>`: the session-local object name (threat-model rule 1).
    pub(super) fn os_name(name: &str) -> String {
        format!("Local\\{name}")
    }

    fn wide(name: &str) -> Vec<u16> {
        os_name(name)
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect()
    }

    fn close_handle(h: HANDLE) {
        // SAFETY: `h` is a live handle this module owns, closed once.
        unsafe { CloseHandle(h) };
    }

    /// Threat-model rule 2: the mapping's owner SID must be this process's
    /// user SID.
    fn check_owner(mapping: HANDLE, name: &str) -> io::Result<()> {
        let mut token: HANDLE = std::ptr::null_mut();
        // SAFETY: the pseudo-handle of the current process, a query-only
        // token request, and a valid out-pointer.
        if unsafe { OpenProcessToken(GetCurrentProcess(), TOKEN_QUERY, &raw mut token) } == 0 {
            return Err(io::Error::last_os_error());
        }
        // TOKEN_USER plus the SID it points into; 256 bytes is ample (a SID is
        // at most 68 bytes). `u64` storage keeps it pointer-aligned.
        let mut buf = [0u64; 32];
        let mut got = 0u32;
        // SAFETY: `token` is live; `buf` is writable for the length passed.
        let ok = unsafe {
            GetTokenInformation(
                token,
                TokenUser,
                buf.as_mut_ptr().cast(),
                u32::try_from(std::mem::size_of_val(&buf)).unwrap_or(u32::MAX),
                &raw mut got,
            )
        };
        close_handle(token);
        if ok == 0 {
            return Err(io::Error::last_os_error());
        }
        // SAFETY: GetTokenInformation(TokenUser) succeeded, so `buf` begins
        // with a TOKEN_USER whose SID points inside `buf`, which outlives this
        // read; `buf` is 8-aligned.
        let me: PSID = unsafe { (*buf.as_ptr().cast::<TOKEN_USER>()).User.Sid };
        let mut owner: PSID = std::ptr::null_mut();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: `mapping` is a live kernel-object handle; only the owner is
        // requested; the descriptor it allocates is freed below.
        let rc = unsafe {
            GetSecurityInfo(
                mapping,
                SE_KERNEL_OBJECT,
                OWNER_SECURITY_INFORMATION,
                &raw mut owner,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &raw mut sd,
            )
        };
        if rc != ERROR_SUCCESS {
            return Err(io::Error::from_raw_os_error(
                i32::try_from(rc).unwrap_or(i32::MAX),
            ));
        }
        // SAFETY: both SIDs are valid: `owner` points into `sd` (still
        // allocated), `me` into `buf`.
        let same = unsafe { EqualSid(owner, me) } != 0;
        // SAFETY: `sd` was allocated by GetSecurityInfo with LocalAlloc.
        unsafe { LocalFree(sd) };
        if same {
            Ok(())
        } else {
            Err(io::Error::new(
                io::ErrorKind::PermissionDenied,
                format!(
                    "refusing shared-memory object {}: it is owned by another user",
                    os_name(name)
                ),
            ))
        }
    }

    /// Map the whole of `handle` and report the view's size.
    fn map_all(handle: HANDLE) -> io::Result<(NonNull<u8>, usize)> {
        // SAFETY: `handle` is a live file-mapping handle; a zero length maps
        // the whole object read-write.
        let view = unsafe { MapViewOfFile(handle, FILE_MAP_ALL_ACCESS, 0, 0, 0) };
        let Some(ptr) = NonNull::new(view.Value.cast::<u8>()) else {
            return Err(io::Error::last_os_error());
        };
        // SAFETY: an all-zero MEMORY_BASIC_INFORMATION is a valid value of
        // the plain C struct; VirtualQuery only writes it.
        let mut info: MEMORY_BASIC_INFORMATION = unsafe { std::mem::zeroed() };
        // SAFETY: `ptr` is the base of the view just mapped; `info` is a
        // valid, writable buffer of the size passed.
        let got = unsafe {
            VirtualQuery(
                ptr.as_ptr().cast(),
                &raw mut info,
                std::mem::size_of::<MEMORY_BASIC_INFORMATION>(),
            )
        };
        if got == 0 {
            let e = io::Error::last_os_error();
            // SAFETY: unmapping the view mapped above, once.
            unsafe {
                UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                    Value: ptr.as_ptr().cast(),
                })
            };
            return Err(e);
        }
        Ok((ptr, info.RegionSize))
    }

    pub(super) fn create(name: &str, len: usize) -> io::Result<SharedRegion> {
        let w = wide(name);
        let len64 = len as u64;
        #[allow(clippy::cast_possible_truncation)] // the high and low halves.
        let (hi, lo) = ((len64 >> 32) as u32, len64 as u32);
        // Threat-model rule 3: a protected DACL granting access to the owner
        // only.
        let sddl: Vec<u16> = "D:P(A;;GA;;;OW)"
            .encode_utf16()
            .chain(std::iter::once(0))
            .collect();
        let mut sd: PSECURITY_DESCRIPTOR = std::ptr::null_mut();
        // SAFETY: a NUL-terminated SDDL string and a valid out-pointer; the
        // descriptor is LocalAlloc'd and freed below.
        if unsafe {
            ConvertStringSecurityDescriptorToSecurityDescriptorW(
                sddl.as_ptr(),
                SDDL_REVISION_1,
                &raw mut sd,
                std::ptr::null_mut(),
            )
        } == 0
        {
            return Err(io::Error::last_os_error());
        }
        let attrs = SECURITY_ATTRIBUTES {
            nLength: u32::try_from(std::mem::size_of::<SECURITY_ATTRIBUTES>()).unwrap_or(u32::MAX),
            lpSecurityDescriptor: sd,
            bInheritHandle: 0,
        };
        // SAFETY: a pagefile-backed mapping (INVALID_HANDLE_VALUE) with the
        // owner-only descriptor above, named by a NUL-terminated UTF-16 string
        // that outlives the call.
        let handle = unsafe {
            CreateFileMappingW(
                INVALID_HANDLE_VALUE,
                &raw const attrs,
                PAGE_READWRITE,
                hi,
                lo,
                w.as_ptr(),
            )
        };
        // SAFETY: read immediately after the call that set it.
        let existed = !handle.is_null() && unsafe { GetLastError() } == ERROR_ALREADY_EXISTS;
        let create_err = handle.is_null().then(io::Error::last_os_error);
        // SAFETY: `sd` came from the conversion above; the mapping copied it.
        unsafe { LocalFree(sd) };
        if let Some(e) = create_err {
            return Err(e);
        }
        // Rule 2: a name someone already holds is only used if it is ours.
        if existed && let Err(e) = check_owner(handle, name) {
            close_handle(handle);
            return Err(e);
        }
        let (ptr, size) = match map_all(handle) {
            Ok(v) => v,
            Err(e) => {
                // SAFETY: closing the handle created above, once.
                unsafe { CloseHandle(handle) };
                return Err(e);
            }
        };
        let region = SharedRegion {
            ptr,
            len: if existed { size } else { len },
            name: name.to_owned(),
            created: !existed,
            handle,
        };
        if region.len < len {
            // An existing mapping is never resized; the caller sees why.
            return Err(io::Error::new(
                io::ErrorKind::InvalidData,
                format!(
                    "region {name} exists with {} bytes, {len} needed",
                    region.len
                ),
            ));
        }
        Ok(region)
    }

    pub(super) fn open(name: &str) -> io::Result<SharedRegion> {
        let w = wide(name);
        // SAFETY: a NUL-terminated UTF-16 name that outlives the call; the
        // handle is not inheritable.
        let handle = unsafe { OpenFileMappingW(FILE_MAP_ALL_ACCESS, 0, w.as_ptr()) };
        if handle.is_null() {
            return Err(io::Error::last_os_error());
        }
        if let Err(e) = check_owner(handle, name) {
            close_handle(handle);
            return Err(e);
        }
        match map_all(handle) {
            Ok((ptr, len)) => Ok(SharedRegion {
                ptr,
                len,
                name: name.to_owned(),
                created: false,
                handle,
            }),
            Err(e) => {
                // SAFETY: closing the handle opened above, once.
                unsafe { CloseHandle(handle) };
                Err(e)
            }
        }
    }

    pub(super) fn close(region: &SharedRegion) {
        // SAFETY: the view and handle are the ones `create`/`open` produced,
        // released once from `Drop`; no reference into the view outlives
        // `&self`.
        unsafe {
            UnmapViewOfFile(MEMORY_MAPPED_VIEW_ADDRESS {
                Value: region.ptr.as_ptr().cast(),
            });
            CloseHandle(region.handle);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::Ordering;

    /// A name no other test (or a parallel `cargo test`) uses.
    fn unique(tag: &str) -> String {
        use std::sync::atomic::AtomicUsize;
        static N: AtomicUsize = AtomicUsize::new(0);
        let n = N.fetch_add(1, Ordering::Relaxed);
        format!("rt{}{tag}{n}", std::process::id() % 100_000)
    }

    #[test]
    fn names_follow_the_portable_rule() {
        for good in ["a", "rustynes-memif", "x_y.z-9", &"n".repeat(MAX_NAME_LEN)] {
            assert!(validate_name(good).is_ok(), "{good}");
        }
        for bad in [
            "",
            "a/b",
            "..\\x",
            "sp ace",
            "nul\0",
            &"n".repeat(MAX_NAME_LEN + 1),
            "é",
        ] {
            assert!(validate_name(bad).is_err(), "{bad:?}");
        }
    }

    /// CR-02's host test, in-process half: a second, independent mapping of
    /// the same NAME, opened on another thread, sees what the first wrote.
    /// Two mappings of one object are two different addresses, so this is the
    /// OS sharing the pages, not a pointer being passed.
    #[test]
    fn a_second_mapping_opened_by_name_sees_the_bytes() {
        let name = unique("a");
        let w = SharedRegion::create(&name, 4096).expect("create");
        assert!(w.created());
        w.write_bytes(100, b"hello, shared world");
        w.atomic_u32(0).store(0xC0FF_EE01, Ordering::Release);
        let n2 = name;
        let got = std::thread::spawn(move || {
            let r = SharedRegion::open(&n2).expect("open by name");
            assert!(!r.created());
            assert!(r.len() >= 4096);
            let mut buf = [0u8; 19];
            r.read_bytes(100, &mut buf);
            (buf, r.atomic_u32(0).load(Ordering::Acquire))
        })
        .join()
        .unwrap();
        assert_eq!(&got.0, b"hello, shared world");
        assert_eq!(got.1, 0xC0FF_EE01);
    }

    /// A creator unlinks on drop (POSIX); afterwards the name is gone. On
    /// Windows the mapping dies with its last handle, which is the same
    /// outcome here.
    #[test]
    fn the_creator_removes_the_name() {
        let name = unique("b");
        drop(SharedRegion::create(&name, 64).unwrap());
        assert_eq!(
            SharedRegion::open(&name).unwrap_err().kind(),
            io::ErrorKind::NotFound
        );
    }

    /// Creating an existing name opens it rather than replacing it, keeps its
    /// contents, and does not take ownership of it.
    #[test]
    fn create_on_an_existing_name_opens_it() {
        let name = unique("c");
        let first = SharedRegion::create(&name, 4096).unwrap();
        first.write_bytes(0, &[7, 8, 9]);
        let second = SharedRegion::create(&name, 16).unwrap();
        assert!(!second.created());
        let mut b = [0u8; 3];
        second.read_bytes(0, &mut b);
        assert_eq!(b, [7, 8, 9]);
    }

    #[test]
    fn opening_a_missing_name_is_not_found() {
        let e = SharedRegion::open(&unique("missing")).unwrap_err();
        assert_eq!(e.kind(), io::ErrorKind::NotFound);
    }

    /// Threat-model rule 1: the OS name is per user.
    #[test]
    fn the_os_name_carries_the_user() {
        let n = os_object_name("rustynes-memif");
        #[cfg(unix)]
        {
            // SAFETY: `geteuid` has no preconditions.
            let uid = unsafe { libc::geteuid() };
            assert_eq!(n, format!("/rustynes-memif.{uid:x}"));
            // The longest name plus the longest suffix fits macOS's 31 bytes.
            const { assert!(1 + MAX_NAME_LEN + 1 + 8 <= 31) };
        }
        #[cfg(windows)]
        assert_eq!(n, "Local\\rustynes-memif");
    }

    /// Threat-model rule 2 (the review finding on d279e0b0): an object that
    /// already exists under our name but is readable or writable by others,
    /// as one a squatter pre-created would be, is refused by both `open` and
    /// `create`, with a message saying why, and never adopted. Creating an
    /// object owned by ANOTHER uid needs root, so the test plants a wrong-MODE
    /// object owned by us, which the same check refuses.
    #[cfg(unix)]
    #[test]
    fn a_squatted_object_with_open_permissions_is_refused() {
        use std::ffi::CString;
        let name = unique("sq");
        let c = CString::new(os_object_name(&name)).unwrap();
        // SAFETY: a valid C string; a new object, mode 0666 (umask may clear
        // some bits; the fchmod below sets them regardless).
        let fd = unsafe {
            libc::shm_open(
                c.as_ptr(),
                libc::O_RDWR | libc::O_CREAT | libc::O_EXCL,
                0o666 as libc::c_uint,
            )
        };
        assert!(fd >= 0, "plant: {}", io::Error::last_os_error());
        // SAFETY: `fd` is the descriptor just opened.
        assert_eq!(unsafe { libc::ftruncate(fd, 4096) }, 0);
        // macOS does not implement `fchmod` on a POSIX shared-memory
        // descriptor (it fails; found when CI first ran this test there, #625),
        // so on Apple the plant keeps the mode `shm_open` gave it: 0666 less
        // the umask, 0644 under the usual 022, which others can read and the
        // check must refuse. The expected message is built from the mode
        // actually read back rather than assumed.
        #[cfg(not(target_vendor = "apple"))]
        // SAFETY: `fd` is the descriptor just opened.
        assert_eq!(unsafe { libc::fchmod(fd, 0o666) }, 0);
        // `st_mode` is a u32 on Linux and a u16 on macOS, as in `check_owner`.
        #[allow(clippy::useless_conversion)]
        // SAFETY: `fd` is open; `st` is a valid out-pointer for `fstat`.
        let mode = unsafe {
            let mut st: libc::stat = std::mem::zeroed();
            assert_eq!(libc::fstat(fd, &raw mut st), 0);
            libc::close(fd);
            u32::from(st.st_mode) & 0o777
        };
        assert_ne!(
            mode & 0o077,
            0,
            "the plant is mode {mode:03o}: this process's umask leaves the \
             object private, so no squatted object can be planted to test"
        );
        let expected = format!("mode {mode:03o}");
        for err in [
            SharedRegion::open(&name).unwrap_err(),
            SharedRegion::create(&name, 4096).unwrap_err(),
        ] {
            assert_eq!(err.kind(), io::ErrorKind::PermissionDenied, "{err}");
            assert!(err.to_string().contains(&expected), "{err}");
        }
        // Clean up the plant; then the normal path works again.
        // SAFETY: a valid C string naming the object planted above.
        unsafe { libc::shm_unlink(c.as_ptr()) };
        let r = SharedRegion::create(&name, 4096).expect("a fresh create works");
        assert!(r.created());
        assert!(SharedRegion::open(&name).is_ok(), "and so does an open");
    }

    /// Rule 2's predicate directly: the uid half cannot be reached through
    /// the OS without a second account (only root can plant an object owned
    /// by another uid), so it is pinned here.
    #[cfg(unix)]
    #[test]
    fn the_owner_rule() {
        use super::sys::owner_ok;
        assert!(owner_ok(1000, 0o600, 1000));
        assert!(owner_ok(1000, 0o400, 1000));
        assert!(!owner_ok(1001, 0o600, 1000), "another user's object");
        assert!(!owner_ok(0, 0o600, 1000), "root's object");
        assert!(!owner_ok(1000, 0o640, 1000), "group-readable");
        assert!(!owner_ok(1000, 0o602, 1000), "other-writable");
    }

    /// The env var a child test process reads its region name from.
    const CHILD_ENV: &str = "RUSTYNES_SHM_CHILD_TEST";

    /// Not a test on its own: the child half of
    /// `another_process_reads_the_region_by_name`, run as a separate process.
    #[test]
    #[ignore = "child half of another_process_reads_the_region_by_name"]
    fn shm_child_process_reader() {
        let Ok(name) = std::env::var(CHILD_ENV) else {
            return;
        };
        let r = SharedRegion::open(&name).expect("child: open by name");
        let mut buf = [0u8; 16];
        r.read_bytes(0, &mut buf);
        // Print what this process saw; the parent compares it.
        println!("CHILD-SAW:{}", String::from_utf8_lossy(&buf));
        // And write back, so the parent sees a write from another process too.
        r.write_bytes(16, b"from-the-child!!");
    }

    /// CR-02's host test, cross-process half: the test binary re-runs itself
    /// as a child that opens the region by NAME, prints what it read and
    /// writes a reply the parent then reads.
    #[test]
    fn another_process_reads_the_region_by_name() {
        let name = unique("p");
        let region = SharedRegion::create(&name, 4096).unwrap();
        region.write_bytes(0, b"written-by-parnt");
        let out = std::process::Command::new(std::env::current_exe().unwrap())
            .args([
                "--exact",
                "shm::tests::shm_child_process_reader",
                "--ignored",
                "--nocapture",
                "--test-threads=1",
            ])
            .env(CHILD_ENV, &name)
            .output()
            .expect("spawn the child");
        let stdout = String::from_utf8_lossy(&out.stdout);
        assert!(out.status.success(), "child failed: {stdout}");
        assert!(
            stdout.contains("CHILD-SAW:written-by-parnt"),
            "the child did not read the parent's bytes: {stdout}"
        );
        let mut reply = [0u8; 16];
        region.read_bytes(16, &mut reply);
        assert_eq!(&reply, b"from-the-child!!");
    }
}
