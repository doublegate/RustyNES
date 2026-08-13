// SPDX-License-Identifier: GPL-3.0-or-later
//! v2.3.3 — the frontend's two absolute clock readings, in one place.
//!
//! `Instant` is the right type for durations and is what the rest of the
//! frontend uses. It is deliberately opaque, though, so its absolute value
//! cannot be read — and two of this campaign's instruments need exactly that:
//!
//! * **`monotonic_now_ns`** anchors the per-frame trace. `wp_presentation`
//!   stamps presentations in the clock named by its `clock_id` event
//!   (1 = `CLOCK_MONOTONIC`), which is the same clock `Instant` rides on Linux;
//!   one absolute reading at the trace origin converts the whole series, and
//!   without it the compositor rows and the process rows are unjoinable.
//!   It is also the payload of the display tick, which is how the winit->emu
//!   hop is measured across a thread boundary where no `Instant` can travel
//!   meaningfully.
//! * **`thread_cpu_now_ns`** separates work from descheduling. Differenced
//!   across a span, `wall - cpu` is time the thread was not running — the
//!   distinction that showed the 9-32 ms `rwork` tail was a `cargo build`
//!   competing for the host, not the frontend doing anything.
//!
//! **Why this module exists at all:** both readings were first written as
//! private associated functions on `App`, and F15 needed the monotonic one on
//! the emulation thread as well. Copying a `clock_gettime` call is copying an
//! `unsafe` block and its `// SAFETY:` justification, which is precisely what
//! the project's rule against scattering `unsafe` forbids. One site, two
//! callers.
//!
//! Read through `libc` rather than a new dependency: it is already a direct dep
//! behind the default-on `emu-thread` feature (for the thread's `rtprio` call).
//! With that feature off, or off Unix, both functions return `None` and every
//! consumer degrades to saying the measurement is unavailable rather than
//! substituting a wrong one.

/// `CLOCK_MONOTONIC` in nanoseconds, or `None` where unavailable.
#[cfg(all(not(target_arch = "wasm32"), unix, feature = "emu-thread"))]
#[must_use]
pub fn monotonic_now_ns() -> Option<u64> {
    read(libc::CLOCK_MONOTONIC)
}

/// This thread's consumed CPU time in nanoseconds (`CLOCK_THREAD_CPUTIME_ID`),
/// or `None` where unavailable.
///
/// Thread-local by definition: the value belongs to whichever thread calls it,
/// which is what makes `wall - cpu` meaningful for that thread alone. Calling
/// it from a different thread than the one whose span is being measured yields
/// a number that looks plausible and means nothing.
#[cfg(all(not(target_arch = "wasm32"), unix, feature = "emu-thread"))]
#[must_use]
pub fn thread_cpu_now_ns() -> Option<u64> {
    read(libc::CLOCK_THREAD_CPUTIME_ID)
}

/// The single `clock_gettime` call site.
#[cfg(all(not(target_arch = "wasm32"), unix, feature = "emu-thread"))]
fn read(clock: libc::clockid_t) -> Option<u64> {
    let mut ts = libc::timespec {
        tv_sec: 0,
        tv_nsec: 0,
    };
    // SAFETY: `clock_gettime` writes only through the supplied pointer, which is
    // a live, correctly-typed, stack-allocated `timespec` owned here for the
    // duration of the call. Both `CLOCK_MONOTONIC` and `CLOCK_THREAD_CPUTIME_ID`
    // are unconditionally available on Linux, and the parameter is private to
    // this module so no caller can pass an arbitrary id. The return value is
    // checked; on failure `ts` is left as initialised above and discarded.
    #[allow(unsafe_code)]
    let rc = unsafe { libc::clock_gettime(clock, &raw mut ts) };
    if rc != 0 {
        return None;
    }
    u64::try_from(ts.tv_sec)
        .ok()?
        .checked_mul(1_000_000_000)?
        .checked_add(u64::try_from(ts.tv_nsec).ok()?)
}

/// No monotonic clock reachable: consumers record the measurement as
/// unavailable rather than substituting a wrong one.
#[cfg(not(all(not(target_arch = "wasm32"), unix, feature = "emu-thread")))]
#[must_use]
pub const fn monotonic_now_ns() -> Option<u64> {
    None
}

/// No thread-CPU clock reachable; the work/deschedule split is not reported.
#[cfg(not(all(not(target_arch = "wasm32"), unix, feature = "emu-thread")))]
#[must_use]
pub const fn thread_cpu_now_ns() -> Option<u64> {
    None
}
