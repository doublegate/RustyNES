//! C-ABI harness: drives the core through the exported `retro_*` functions,
//! the way a libretro frontend does (v2.8.0, libretro audit §1-§2).
//!
//! # Why a harness and not method calls
//!
//! Every defect this release fixes lives at the C boundary: which environment
//! commands a frontend answers, what it is handed back in `retro_serialize`'s
//! buffer, which memory maps it still holds after `retro_unload_game`, whether a
//! panic reaches it. A test that calls `RustyNesLibretro`'s methods directly
//! goes around exactly that boundary, and cannot construct the
//! `rust_libretro` contexts anyway. So these tests stand in for the frontend:
//! an environment callback that can refuse `GET_GAME_INFO_EXT` and records
//! every `SET_MEMORY_MAPS`, plus the video / audio / input callbacks.
//!
//! # The shared-instance constraint
//!
//! `rust_libretro` keeps the core in one process-global instance, created on
//! the first `retro_get_system_info` and never replaced. Every test in this
//! module therefore shares one core, so each takes [`LOCK`] for its whole body
//! and leaves the core unloaded when it finishes. Tests elsewhere in the crate
//! build their own `RustyNesLibretro` and never touch the global instance.

use super::*;
use std::os::raw::{c_uint, c_void};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, Ordering::SeqCst};
use std::sync::{Mutex, MutexGuard, Once};

/// The CC0 nestest image: a plain NROM cartridge with 2 KiB of WRAM mapped.
const NESTEST: &[u8] = include_bytes!("../../../tests/roms/nestest/nestest.nes");

/// Serializes every test in this module around the one global core.
static LOCK: Mutex<()> = Mutex::new(());
/// Runs the frontend's one-time setup exactly once per process.
static INIT: Once = Once::new();

/// Whether the fake frontend answers `RETRO_ENVIRONMENT_GET_GAME_INFO_EXT`.
static SUPPORT_EXT: AtomicBool = AtomicBool::new(true);
/// The `retro_game_info_ext` the fake frontend hands out when asked.
static EXT_INFO: AtomicPtr<RetroGameInfoExt> = AtomicPtr::new(std::ptr::null_mut());
/// Descriptor count of the most recent `SET_MEMORY_MAPS`, or -1 before any.
static LAST_MAP_LEN: AtomicI64 = AtomicI64::new(-1);
/// Whether the fake light gun on every port reports its trigger held.
static TRIGGER: AtomicBool = AtomicBool::new(false);

unsafe extern "C" fn environment(cmd: c_uint, data: *mut c_void) -> bool {
    match cmd {
        RETRO_ENVIRONMENT_GET_GAME_INFO_EXT => {
            let info = EXT_INFO.load(SeqCst);
            if !SUPPORT_EXT.load(SeqCst) || info.is_null() || data.is_null() {
                return false;
            }
            // SAFETY: the core passes a `*mut *const RetroGameInfoExt` for this
            // command, and `info` is a leaked `Box` that is never freed.
            unsafe { *data.cast::<*const RetroGameInfoExt>() = info };
            true
        }
        RETRO_ENVIRONMENT_SET_MEMORY_MAPS => {
            if data.is_null() {
                return false;
            }
            // SAFETY: the core passes a `*const retro_memory_map` for this
            // command, valid for the duration of the call.
            let map = unsafe { &*data.cast::<retro_memory_map>() };
            LAST_MAP_LEN.store(i64::from(map.num_descriptors), SeqCst);
            true
        }
        // Every other command is refused, which is what a minimal frontend
        // does; the core has to cope with that anyway.
        _ => false,
    }
}

unsafe extern "C" fn video(_: *const c_void, _: c_uint, _: c_uint, _: usize) {}
unsafe extern "C" fn audio_batch(_: *const i16, frames: usize) -> usize {
    frames
}
unsafe extern "C" fn audio_sample(_: i16, _: i16) {}
unsafe extern "C" fn input_poll() {}
unsafe extern "C" fn input_state(_port: c_uint, device: c_uint, _: c_uint, id: c_uint) -> i16 {
    i16::from(
        device == RETRO_DEVICE_LIGHTGUN
            && id == RETRO_DEVICE_ID_LIGHTGUN_TRIGGER
            && TRIGGER.load(SeqCst),
    )
}

/// Take the lock and make sure the frontend's one-time setup has run.
fn frontend() -> MutexGuard<'static, ()> {
    // A test that panicked while holding the lock poisons it; the state it
    // protects is reset by each test, so the poison carries no information.
    let guard = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    INIT.call_once(|| {
        // SAFETY: the calls a libretro frontend makes before loading content,
        // in the order libretro.h documents. `retro_get_system_info` first:
        // it is what creates `rust_libretro`'s global instance.
        unsafe {
            let mut info = std::mem::zeroed::<retro_system_info>();
            rust_libretro::retro_get_system_info(&raw mut info);
            rust_libretro::retro_set_environment(Some(environment));
            rust_libretro::retro_set_video_refresh(Some(video));
            rust_libretro::retro_set_audio_sample(Some(audio_sample));
            rust_libretro::retro_set_audio_sample_batch(Some(audio_batch));
            rust_libretro::retro_set_input_poll(Some(input_poll));
            rust_libretro::retro_set_input_state(Some(input_state));
            rust_libretro::retro_init();
        }
    });
    SUPPORT_EXT.store(true, SeqCst);
    TRIGGER.store(false, SeqCst);
    guard
}

/// libretro.h's `struct retro_game_info`, laid out as a frontend passes it.
///
/// Defined here because `rust-libretro-sys` 0.3.2 cannot express it: libretro.h
/// forward-declares `struct retro_game_info;` (line 2879) before the definition
/// (line 3661), and bindgen emits an opaque one-byte placeholder
/// (`_address: u8`). `rust_libretro::retro_load_game` then copies that byte
/// into `Option<retro_game_info>`, so the `game` argument a core receives
/// carries no path, data or size at all.
#[repr(C)]
struct StdGameInfo {
    path: *const std::os::raw::c_char,
    data: *const c_void,
    size: usize,
    meta: *const std::os::raw::c_char,
}

/// Load `rom` as a frontend would, answering `GET_GAME_INFO_EXT` or not.
fn load(rom: &'static [u8], answer_ext: bool) -> bool {
    let ext = Box::new(RetroGameInfoExt {
        full_path: std::ptr::null(),
        archive_path: std::ptr::null(),
        archive_file: std::ptr::null(),
        dir: std::ptr::null(),
        name: std::ptr::null(),
        ext: c"nes".as_ptr(),
        meta_data: std::ptr::null(),
        data: rom.as_ptr().cast(),
        size: rom.len(),
        file_in_archive: false,
        persistent_data: false,
    });
    // Leaked on purpose: the frontend owns this for the rest of the process.
    EXT_INFO.store(Box::into_raw(ext), SeqCst);
    SUPPORT_EXT.store(answer_ext, SeqCst);
    let game = StdGameInfo {
        path: std::ptr::null(),
        data: rom.as_ptr().cast(),
        size: rom.len(),
        meta: std::ptr::null(),
    };
    // SAFETY: `StdGameInfo` has libretro.h's `struct retro_game_info` layout
    // (see its doc), and `game` and the ROM bytes outlive the call.
    unsafe { rust_libretro::retro_load_game(std::ptr::from_ref(&game).cast()) }
}

fn unload() {
    // SAFETY: a plain lifecycle call with no arguments.
    unsafe { rust_libretro::retro_unload_game() };
    for port in 0..4 {
        set_port(port, RETRO_DEVICE_JOYPAD);
    }
}

fn run_frame() {
    // SAFETY: a plain lifecycle call with no arguments.
    unsafe { rust_libretro::retro_run() };
}

fn serialize_size() -> usize {
    // SAFETY: a plain lifecycle call with no arguments.
    unsafe { rust_libretro::retro_serialize_size() }
}

fn serialize(buf: &mut [u8]) -> bool {
    // SAFETY: `buf` is valid for writes of `buf.len()` bytes for the call.
    unsafe { rust_libretro::retro_serialize(buf.as_mut_ptr().cast(), buf.len()) }
}

fn unserialize(buf: &[u8]) -> bool {
    // SAFETY: `buf` is valid for reads of `buf.len()` bytes for the call.
    unsafe { rust_libretro::retro_unserialize(buf.as_ptr().cast(), buf.len()) }
}

fn system_ram() -> *mut c_void {
    // SAFETY: a plain query; the pointer is only compared, never dereferenced.
    unsafe { rust_libretro::retro_get_memory_data(RETRO_MEMORY_SYSTEM_RAM) }
}

fn set_port(port: c_uint, device: c_uint) {
    // SAFETY: a plain lifecycle call; ports beyond the console's four are
    // ignored by the core.
    unsafe { rust_libretro::retro_set_controller_port_device(port, device) };
}

/// libretro audit §1.2 (L-1.2). A frontend that implements only the standard
/// `retro_load_game(const struct retro_game_info *)` refuses
/// `GET_GAME_INFO_EXT`, and the core must still load from the `game` it was
/// handed. Before v2.8.0 it returned "Frontend does not support
/// get_game_info_ext" and loaded nothing.
#[test]
#[ignore = "red: blocked on rust-libretro 0.3.2, whose opaque retro_game_info hands the core no path/data/size; needs the maintainer's vendor-or-upstream decision (libretro ledger L-1.2)"]
fn a_frontend_without_game_info_ext_still_loads_the_game() {
    let _frontend = frontend();
    assert!(
        load(NESTEST, false),
        "retro_load_game must fall back to the standard retro_game_info"
    );
    run_frame();
    unload();
}

/// libretro audit §2.1 and §2.2 (L-2.1, L-2.2). `retro_serialize_size` is read
/// once, but a Zapper plugged in mid-game grows the snapshot, so the frontend's
/// buffer was too small and every save state, rewind and run-ahead frame
/// failed. With headroom reserved, the state fits, the unused tail is zeroed
/// (the buffer is pre-filled with 0xAA to prove it), and the padded buffer
/// restores.
#[test]
#[ignore = "red until the v2.8.0 serialize_size headroom and zeroed tail land (libretro ledger L-2.1)"]
fn save_states_fit_with_a_zapper_on_both_ports_and_restore_padded() {
    let _frontend = frontend();
    assert!(load(NESTEST, true));
    for port in 0..2 {
        set_port(port, RETRO_DEVICE_LIGHTGUN);
    }
    TRIGGER.store(true, SeqCst);
    run_frame(); // attaches both Zappers to the bus
    let mut buf = vec![0xAA_u8; serialize_size()];
    let saved = serialize(&mut buf);
    assert!(
        saved,
        "a Zapper on both ports must fit in retro_serialize_size"
    );
    assert!(
        !buf.ends_with(&[0xAA]),
        "the unused tail must be zeroed, not left as the frontend's bytes"
    );
    run_frame();
    let restored = unserialize(&buf);
    assert!(
        restored,
        "a state padded out to retro_serialize_size must restore"
    );
    unload();
}

/// libretro audit §1.3 (L-1.3). The core hands the frontend raw pointers into
/// its WRAM, SRAM and CIRAM. RetroArch keeps those descriptors until the whole
/// core is torn down (`runloop_system_info_free`, reached from
/// `uninit_libretro_symbols`), not when content is closed, so after
/// `retro_unload_game` they pointed at freed memory. The core now replaces
/// them with an empty map first; RetroArch's handler frees the old
/// descriptors before reading the new count, so the empty map clears them.
#[test]
#[ignore = "red until the v2.8.0 unload-time SET_MEMORY_MAPS deregistration lands (libretro ledger L-1.3)"]
fn unloading_withdraws_the_memory_maps() {
    let _frontend = frontend();
    assert!(load(NESTEST, true));
    assert!(
        LAST_MAP_LEN.load(SeqCst) >= 2,
        "loading registers at least WRAM and CIRAM"
    );
    unload();
    assert_eq!(
        LAST_MAP_LEN.load(SeqCst),
        0,
        "retro_unload_game must leave the frontend holding no descriptors"
    );
}

/// The memory-map pointers must stay valid across a restore: a restore that
/// reallocated WRAM would leave every registered descriptor dangling while
/// the game kept running. Pinned so a future snapshot change cannot do that
/// silently.
#[test]
fn registered_memory_stays_put_across_a_restore() {
    let _frontend = frontend();
    assert!(load(NESTEST, true));
    run_frame();
    let before = system_ram();
    let mut buf = vec![0_u8; serialize_size()];
    assert!(serialize(&mut buf));
    run_frame();
    assert!(unserialize(&buf));
    let after = system_ram();
    assert!(!before.is_null());
    assert_eq!(before, after, "WRAM moved during a restore");
    unload();
}
