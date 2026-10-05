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
use crate::tests::DescView;
use std::os::raw::{c_uint, c_void};
use std::sync::atomic::{AtomicBool, AtomicI64, AtomicPtr, AtomicU32, Ordering::SeqCst};
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
/// How many times the core has called `SET_MEMORY_MAPS`.
static MAP_SETS: AtomicU32 = AtomicU32::new(0);
/// Whether the fake light gun on every port reports its trigger held.
static TRIGGER: AtomicBool = AtomicBool::new(false);
/// Every line the core wrote through the fake frontend's log interface.
static LOGGED: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// `input_poll` calls since the counter was last cleared.
static POLLS: AtomicU32 = AtomicU32::new(0);
/// `input_state` calls for `RETRO_DEVICE_JOYPAD` since last cleared.
static JOYPAD_READS: AtomicU32 = AtomicU32::new(0);
/// The port of every descriptor in the most recent `SET_INPUT_DESCRIPTORS`.
static DESCRIBED_PORTS: Mutex<Vec<u32>> = Mutex::new(Vec::new());
/// `(port, id, description)` of every descriptor in the most recent
/// `SET_INPUT_DESCRIPTORS`.
static DESCRIBED: Mutex<Vec<(u32, u32, String)>> = Mutex::new(Vec::new());
/// Keys the core declared with `SET_VARIABLES`.
static DECLARED_VARS: Mutex<Vec<String>> = Mutex::new(Vec::new());
/// How many times the core has called `SET_VARIABLES`.
static SET_VARIABLES_CALLS: AtomicU32 = AtomicU32::new(0);
/// Whether the fake video callback copies each frame into `LAST_FRAME`
/// (off by default: most tests do not look at the picture).
static KEEP_FRAME: AtomicBool = AtomicBool::new(false);
/// The last frame presented, as tightly packed XRGB8888 rows.
static LAST_FRAME: Mutex<Vec<u8>> = Mutex::new(Vec::new());

/// The RetroPad buttons held on each port, as a `RETRO_DEVICE_ID_JOYPAD_MASK`
/// bitmask (bit n = `RETRO_DEVICE_ID_JOYPAD_*` n).
static PADS: [AtomicU32; 4] = [const { AtomicU32::new(0) }; 4];

/// The disk-control interface version the fake frontend reports.
static DISK_VERSION: AtomicU32 = AtomicU32::new(1);
/// `SET_DISK_CONTROL_INTERFACE` (v0) calls since last cleared.
static DISK_V0_SETS: AtomicU32 = AtomicU32::new(0);
/// `SET_DISK_CONTROL_EXT_INTERFACE` calls since last cleared.
static DISK_EXT_SETS: AtomicU32 = AtomicU32::new(0);
/// The callbacks of the most recent `SET_DISK_CONTROL_EXT_INTERFACE`.
static DISK_EXT: Mutex<Option<DiskExt>> = Mutex::new(None);

/// The parts of a `retro_disk_control_ext_callback` the tests call or check.
#[derive(Clone, Copy)]
struct DiskExt {
    get_num_images: retro_get_num_images_t,
    get_image_label: retro_get_image_label_t,
    get_image_path: retro_get_image_path_t,
    set_initial_image: retro_set_initial_image_t,
}

/// What the synthetic BIOS writes over side A's disk-info block: the block
/// code and signature unchanged (so the image still parses), then this
/// marker, then zeros to the block's 56 bytes.
const DISK_MARKER: &[u8] = b"RUSTYNES-NL03-DISK-SAVE";

/// The synthetic 8 KiB FDS BIOS (the real `disksys.rom` is Nintendo's and is
/// never committed). At reset it saves to the disk the way a game does,
/// through the drive registers, and then idles:
///
/// ```text
/// $E000  LDA #$01 / STA $4023     disk I/O on
///        LDA #$00 / STA $4024     byte for head position 0
///        LDA #$60 / STA $4025     write mode, motor on, CRC control
///        LDX #200                 heads 1-200: the lead-in gap (not stored)
/// gap:   BIT $4030 / BPL gap      wait for the byte-transfer flag
///        LDA #$00 / STA $4024 / DEX / BNE gap
///        LDY #0                   heads 201-256: the disk-info block
/// pay:   BIT $4030 / BPL pay
///        LDA $E100,Y / STA $4024 / INY / CPY #56 / BNE pay
/// last:  BIT $4030 / BPL last     the last byte is on the disk
///        LDA #$00 / STA $4023     disk I/O off
/// idle:  JMP idle
/// $E080  RTI                      NMI / IRQ
/// $E100  the 56-byte block: $01 "*NINTENDO-HVC*" DISK_MARKER, zeros
/// ```
///
/// The drive stores the byte in `$4024` at each head position and sets
/// `$4030` bit 7; writing `$4024` clears it. After a 50,000-cycle spin-up and
/// 257 bytes at 149 cycles each the write is done within three frames. The
/// wire layout (a 200-byte lead-in gap, the `$80` start mark at 200, the
/// first block's payload from 201) is `rustynes-mappers`' `fds.rs`.
fn synthetic_bios() -> Vec<u8> {
    let mut bios = vec![0u8; 0x2000];
    let program: [u8; 0x3D] = [
        0xA9, 0x01, 0x8D, 0x23, 0x40, // LDA #$01, STA $4023
        0xA9, 0x00, 0x8D, 0x24, 0x40, // LDA #$00, STA $4024
        0xA9, 0x60, 0x8D, 0x25, 0x40, // LDA #$60, STA $4025
        0xA2, 0xC8, // $E00F LDX #200
        0x2C, 0x30, 0x40, 0x10, 0xFB, // $E011 BIT $4030, BPL $E011
        0xA9, 0x00, 0x8D, 0x24, 0x40, // LDA #$00, STA $4024
        0xCA, 0xD0, 0xF3, // DEX, BNE $E011
        0xA0, 0x00, // $E01E LDY #0
        0x2C, 0x30, 0x40, 0x10, 0xFB, // $E020 BIT $4030, BPL $E020
        0xB9, 0x00, 0xE1, 0x8D, 0x24, 0x40, // LDA $E100,Y, STA $4024
        0xC8, 0xC0, 0x38, 0xD0, 0xF0, // INY, CPY #56, BNE $E020
        0x2C, 0x30, 0x40, 0x10, 0xFB, // $E030 BIT $4030, BPL $E030
        0xA9, 0x00, 0x8D, 0x23, 0x40, // LDA #$00, STA $4023
        0x4C, 0x3A, 0xE0, // $E03A JMP $E03A
    ];
    bios[..program.len()].copy_from_slice(&program);
    bios[0x80] = 0x40; // $E080: RTI
    let block = &mut bios[0x100..0x100 + 56];
    block[0] = 0x01;
    block[1..15].copy_from_slice(b"*NINTENDO-HVC*");
    block[15..15 + DISK_MARKER.len()].copy_from_slice(DISK_MARKER);
    // NMI $E080, RESET $E000, IRQ $E080.
    bios[0x1FFA..].copy_from_slice(&[0x80, 0xE0, 0x00, 0xE0, 0x80, 0xE0]);
    bios
}

/// A fwNES disk image with `sides` sides, each opening with the disk-info
/// block signature (the only part the loader requires).
fn synthetic_disk(sides: u8) -> &'static [u8] {
    const SIDE: usize = 65_500;
    let mut disk = vec![0u8; 16 + usize::from(sides) * SIDE];
    disk[..4].copy_from_slice(b"FDS\x1A");
    disk[4] = sides;
    for s in 0..usize::from(sides) {
        let base = 16 + s * SIDE;
        disk[base] = 0x01;
        disk[base + 1..base + 15].copy_from_slice(b"*NINTENDO-HVC*");
    }
    Box::leak(disk.into_boxed_slice())
}

/// The fake frontend's directories, created once per test process under the
/// OS temporary directory, with the synthetic BIOS in the system one. The C
/// strings are what `GET_*_DIRECTORY` hand out; they live for the process.
struct Dirs {
    system: std::ffi::CString,
    save: std::ffi::CString,
    save_path: std::path::PathBuf,
}

fn dirs() -> &'static Dirs {
    static DIRS: std::sync::OnceLock<Dirs> = std::sync::OnceLock::new();
    DIRS.get_or_init(|| {
        let root = std::env::temp_dir()
            .join("RustyNES")
            .join(format!("libretro-abi-{}", std::process::id()));
        let system = root.join("system");
        let save = root.join("saves");
        std::fs::create_dir_all(&system).expect("create the system directory");
        std::fs::create_dir_all(&save).expect("create the save directory");
        std::fs::write(system.join("disksys.rom"), synthetic_bios()).expect("write the BIOS");
        let c = |p: &std::path::Path| {
            std::ffi::CString::new(p.to_str().expect("UTF-8 temp path")).expect("no NUL")
        };
        Dirs {
            system: c(&system),
            save: c(&save),
            save_path: save,
        }
    })
}
/// The value the fake frontend reports for `rustynes_four_score`.
static FOUR_SCORE_ON: AtomicBool = AtomicBool::new(false);
/// Whether the next `GET_VARIABLE_UPDATE` reports a change (then clears).
static VARS_CHANGED: AtomicBool = AtomicBool::new(false);
/// The descriptors of the most recent `SET_MEMORY_MAPS`, copied out.
static LAST_MAP: Mutex<Vec<Desc>> = Mutex::new(Vec::new());

/// One `retro_memory_descriptor`, copied out of a `SET_MEMORY_MAPS` call so
/// a test can inspect it after the call returns.
#[derive(Clone, Debug)]
struct Desc {
    flags: u64,
    ptr: usize,
    offset: usize,
    start: usize,
    select: usize,
    len: usize,
    addrspace: String,
}

/// Lock a harness `Mutex`, ignoring poison: every test resets what it reads.
fn held<T>(m: &Mutex<T>) -> MutexGuard<'_, T> {
    m.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
}

/// The fake frontend's `retro_log_printf_t`.
///
/// libretro declares it variadic, and stable Rust cannot DEFINE a variadic
/// function, so this is a fixed-arity stand-in for the one shape the core
/// calls: `(level, "[RustyNES] %s\n", text)`. It is handed out only on x86_64,
/// where both the System V and the Windows x64 conventions pass those three
/// arguments in the same registers whether or not the callee is variadic; on
/// other targets the fake frontend refuses the command instead.
#[cfg(target_arch = "x86_64")]
unsafe extern "C" fn log_printf(
    _level: retro_log_level,
    fmt: *const std::ffi::c_char,
    text: *const std::ffi::c_char,
) {
    // SAFETY: the core passes two NUL-terminated strings valid for the call.
    let (fmt, text) = unsafe { (CStr::from_ptr(fmt), CStr::from_ptr(text)) };
    assert_eq!(fmt, c"[RustyNES] %s\n", "the format must stay a fixed %s");
    LOGGED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .push(text.to_string_lossy().into_owned());
}

/// Copy the descriptors of a `SET_MEMORY_MAPS` call out of the core's
/// memory, which is only valid for the duration of the call.
fn copy_descriptors(map: &retro_memory_map) -> Vec<Desc> {
    let mut descs = Vec::new();
    for i in 0..map.num_descriptors as usize {
        // SAFETY: `descriptors` holds `num_descriptors` entries for the
        // duration of the call; a count of zero never reads it.
        let d = unsafe { &*map.descriptors.add(i) };
        let addrspace = if d.addrspace.is_null() {
            String::new()
        } else {
            // SAFETY: a non-null `addrspace` is a NUL-terminated name.
            unsafe { CStr::from_ptr(d.addrspace) }
                .to_string_lossy()
                .into_owned()
        };
        descs.push(Desc {
            flags: d.flags,
            ptr: d.ptr as usize,
            offset: d.offset,
            start: d.start,
            select: d.select,
            len: d.len,
            addrspace,
        });
    }
    descs
}

// One arm per environment command the fake frontend answers: the whole
// frontend's behaviour in one table reads better than split across helpers.
#[allow(clippy::too_many_lines)]
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
        #[cfg(target_arch = "x86_64")]
        RETRO_ENVIRONMENT_GET_LOG_INTERFACE => {
            if data.is_null() {
                return false;
            }
            // SAFETY: the core passes a `*mut retro_log_callback`. The
            // transmute gives the fixed-arity `log_printf` the variadic type
            // libretro declares; see its doc for why that holds on x86_64.
            unsafe {
                (*data.cast::<retro_log_callback>()).log = Some(std::mem::transmute::<
                    unsafe extern "C" fn(
                        retro_log_level,
                        *const std::ffi::c_char,
                        *const std::ffi::c_char,
                    ),
                    unsafe extern "C" fn(retro_log_level, *const std::ffi::c_char, ...),
                >(log_printf));
            }
            true
        }
        // A current frontend: joypads can be read as one bitmask.
        RETRO_ENVIRONMENT_GET_INPUT_BITMASKS => true,
        RETRO_ENVIRONMENT_GET_SYSTEM_DIRECTORY => {
            if data.is_null() {
                return false;
            }
            // SAFETY: the core passes a `*mut *const c_char`; the string is
            // a process-lifetime `CString`.
            unsafe { *data.cast::<*const std::ffi::c_char>() = dirs().system.as_ptr() };
            true
        }
        RETRO_ENVIRONMENT_GET_SAVE_DIRECTORY => {
            if data.is_null() {
                return false;
            }
            // SAFETY: as for the system directory.
            unsafe { *data.cast::<*const std::ffi::c_char>() = dirs().save.as_ptr() };
            true
        }
        RETRO_ENVIRONMENT_GET_DISK_CONTROL_INTERFACE_VERSION => {
            if data.is_null() {
                return false;
            }
            // SAFETY: the core passes a `*mut c_uint`.
            unsafe { *data.cast::<c_uint>() = DISK_VERSION.load(SeqCst) };
            true
        }
        RETRO_ENVIRONMENT_SET_DISK_CONTROL_INTERFACE => {
            DISK_V0_SETS.fetch_add(1, SeqCst);
            true
        }
        RETRO_ENVIRONMENT_SET_DISK_CONTROL_EXT_INTERFACE => {
            if data.is_null() || DISK_VERSION.load(SeqCst) == 0 {
                return false;
            }
            // SAFETY: the core passes a `*const retro_disk_control_ext_callback`
            // valid for the call; the function pointers in it are the core's
            // own `extern "C"` functions and live for the process.
            let ext = unsafe { &*data.cast::<retro_disk_control_ext_callback>() };
            *held(&DISK_EXT) = Some(DiskExt {
                get_num_images: ext.get_num_images,
                get_image_label: ext.get_image_label,
                get_image_path: ext.get_image_path,
                set_initial_image: ext.set_initial_image,
            });
            DISK_EXT_SETS.fetch_add(1, SeqCst);
            true
        }
        RETRO_ENVIRONMENT_SET_INPUT_DESCRIPTORS => {
            let mut ports = Vec::new();
            let mut named = Vec::new();
            let mut d = data.cast::<retro_input_descriptor>().cast_const();
            // SAFETY: the core passes an array terminated by an entry whose
            // `description` is null, valid for the duration of the call.
            unsafe {
                while !d.is_null() && !(*d).description.is_null() {
                    ports.push((*d).port);
                    let text = CStr::from_ptr((*d).description).to_string_lossy();
                    named.push(((*d).port, (*d).id, text.into_owned()));
                    d = d.add(1);
                }
            }
            *held(&DESCRIBED_PORTS) = ports;
            *held(&DESCRIBED) = named;
            true
        }
        RETRO_ENVIRONMENT_SET_VARIABLES => {
            let mut keys = Vec::new();
            let mut v = data.cast::<retro_variable>().cast_const();
            // SAFETY: an array terminated by a null `key`, valid for the call.
            unsafe {
                while !v.is_null() && !(*v).key.is_null() {
                    keys.push(CStr::from_ptr((*v).key).to_string_lossy().into_owned());
                    v = v.add(1);
                }
            }
            *held(&DECLARED_VARS) = keys;
            SET_VARIABLES_CALLS.fetch_add(1, SeqCst);
            true
        }
        RETRO_ENVIRONMENT_GET_VARIABLE => {
            if data.is_null() {
                return false;
            }
            // SAFETY: the core passes a `*mut retro_variable` whose `key` is a
            // NUL-terminated string; `value` is set to a `'static` literal.
            unsafe {
                let var = &mut *data.cast::<retro_variable>();
                if var.key.is_null() || CStr::from_ptr(var.key) != c"rustynes_four_score" {
                    return false;
                }
                var.value = if FOUR_SCORE_ON.load(SeqCst) {
                    c"enabled".as_ptr()
                } else {
                    c"disabled".as_ptr()
                };
            }
            true
        }
        RETRO_ENVIRONMENT_GET_VARIABLE_UPDATE => {
            if data.is_null() {
                return false;
            }
            // SAFETY: the core passes a `*mut bool`.
            unsafe { *data.cast::<bool>() = VARS_CHANGED.swap(false, SeqCst) };
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
            MAP_SETS.fetch_add(1, SeqCst);
            *held(&LAST_MAP) = copy_descriptors(map);
            true
        }
        // Every other command is refused, which is what a minimal frontend
        // does; the core has to cope with that anyway.
        _ => false,
    }
}

unsafe extern "C" fn video(data: *const c_void, width: c_uint, height: c_uint, pitch: usize) {
    if !KEEP_FRAME.load(SeqCst) || data.is_null() {
        return;
    }
    let len = pitch * height as usize;
    // SAFETY: libretro.h: `data` holds `height` rows of `pitch` bytes for
    // the duration of the call; only `width * 4` bytes of each row are
    // pixels (XRGB8888), and all of it is readable.
    let frame = unsafe { std::slice::from_raw_parts(data.cast::<u8>(), len) };
    let mut out = Vec::with_capacity(width as usize * 4 * height as usize);
    for row in frame.chunks(pitch) {
        out.extend_from_slice(&row[..width as usize * 4]);
    }
    *held(&LAST_FRAME) = out;
}
unsafe extern "C" fn audio_batch(_: *const i16, frames: usize) -> usize {
    frames
}
unsafe extern "C" fn audio_sample(_: i16, _: i16) {}
unsafe extern "C" fn input_poll() {
    POLLS.fetch_add(1, SeqCst);
}
unsafe extern "C" fn input_state(port: c_uint, device: c_uint, _: c_uint, id: c_uint) -> i16 {
    if device == RETRO_DEVICE_JOYPAD {
        JOYPAD_READS.fetch_add(1, SeqCst);
        if id == RETRO_DEVICE_ID_JOYPAD_MASK {
            let mask = PADS.get(port as usize).map_or(0, |p| p.load(SeqCst));
            // The bitmask is 16 bits wide; libretro returns it as `int16_t`.
            return (mask as u16).cast_signed();
        }
    }
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
    DISK_VERSION.store(1, SeqCst);
    SERIALIZE_BUFFER_GROWTHS.store(0, SeqCst);
    for pad in &PADS {
        pad.store(0, SeqCst);
    }
    KEEP_FRAME.store(false, SeqCst);
    guard
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
    // The real struct, which the vendored `rust-libretro-sys` defines by hand:
    // the crates.io binding reduced it to one opaque byte (libretro audit L-1.2,
    // `vendor/rust-libretro-sys/VENDORED.md`).
    let game = retro_game_info {
        path: std::ptr::null(),
        data: rom.as_ptr().cast(),
        size: rom.len(),
        meta: std::ptr::null(),
    };
    // SAFETY: `game` and the ROM bytes outlive the call.
    unsafe { rust_libretro::retro_load_game(&raw const game) }
}

/// The nestest image on disk, for loads that pass only a path.
const NESTEST_PATH: &CStr = match CStr::from_bytes_with_nul(
    concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../tests/roms/nestest/nestest.nes\0"
    )
    .as_bytes(),
) {
    Ok(path) => path,
    Err(_) => panic!("the path literal carries its own NUL"),
};

/// Load as a frontend that answers `GET_GAME_INFO_EXT` with a path but no
/// data, and passes the standard `retro_game_info` the same way.
fn load_path_only(path: &'static CStr) -> bool {
    let ext = Box::new(RetroGameInfoExt {
        full_path: path.as_ptr(),
        archive_path: std::ptr::null(),
        archive_file: std::ptr::null(),
        dir: std::ptr::null(),
        name: std::ptr::null(),
        ext: c"nes".as_ptr(),
        meta_data: std::ptr::null(),
        data: std::ptr::null(),
        size: 0,
        file_in_archive: false,
        persistent_data: false,
    });
    // Leaked on purpose: the frontend owns this for the rest of the process.
    EXT_INFO.store(Box::into_raw(ext), SeqCst);
    SUPPORT_EXT.store(true, SeqCst);
    let game = retro_game_info {
        path: path.as_ptr(),
        data: std::ptr::null(),
        size: 0,
        meta: std::ptr::null(),
    };
    // SAFETY: `game` and the path outlive the call.
    unsafe { rust_libretro::retro_load_game(&raw const game) }
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
///
/// Two defects stood behind that, and this test needs both fixed: the core had
/// no fallback, and the `rust-libretro-sys` binding reduced `retro_game_info` to
/// an opaque byte, so there was nothing to fall back to. The binding is now a
/// vendored, patched copy.
#[test]
fn a_frontend_without_game_info_ext_still_loads_the_game() {
    let _frontend = frontend();
    assert!(
        load(NESTEST, false),
        "retro_load_game must fall back to the standard retro_game_info"
    );
    run_frame();
    unload();
}

/// Review on #556 (agy). A frontend that answers `GET_GAME_INFO_EXT` with a
/// path and no data got an error, although the standard `retro_game_info`
/// it also passes names a file the core can read. The unusable EXT answer is
/// now treated as no answer.
#[test]
fn an_ext_answer_without_data_falls_back_to_the_game_path() {
    let _frontend = frontend();
    assert!(
        load_path_only(NESTEST_PATH),
        "an EXT answer with no data must fall through to retro_game_info::path"
    );
    run_frame();
    unload();
}

/// Review on #556 (agy). Every message the core writes goes to the frontend's
/// log interface when it offers one; before, all of them went to stderr,
/// which `RetroArch` does not put in its own log.
#[cfg(target_arch = "x86_64")]
#[test]
fn messages_reach_the_frontends_log_interface() {
    let _frontend = frontend();
    LOGGED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clear();
    assert!(load(NESTEST, true));
    unload();
    let logged = LOGGED
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner)
        .clone();
    assert!(
        logged.iter().any(|line| line.starts_with("Loading ")),
        "the load message must reach the frontend's log, got {logged:?}"
    );
}

/// libretro audit §2.5 (L-2.5). `rust-libretro`'s `retro_run` polls input
/// before calling `on_run`, and the core polled again; and it read each joypad
/// with 16 `input_state` calls where one bitmask read does. One frame of a
/// two-pad console must cost exactly one poll and two joypad reads.
#[test]
fn input_is_polled_once_and_read_as_one_bitmask_per_pad() {
    let _frontend = frontend();
    assert!(load(NESTEST, true));
    POLLS.store(0, SeqCst);
    JOYPAD_READS.store(0, SeqCst);
    run_frame();
    assert_eq!(POLLS.load(SeqCst), 1, "input polled more than once a frame");
    assert_eq!(
        JOYPAD_READS.load(SeqCst),
        2,
        "one bitmask read per pad on a frontend that supports bitmasks"
    );
    unload();
}

/// Executive-summary claim (L-S1). Only port 0 had input descriptors, so
/// RetroArch's remap menu had no button names for player 2, for a Vs.
/// cabinet's second console, or for Four Score players 3 and 4.
#[test]
fn every_port_has_input_descriptors() {
    let _frontend = frontend();
    let ports = held(&DESCRIBED_PORTS).clone();
    // Exactly 32: the list must END at a null description. With the
    // `input_descriptors!` macro's `""` terminator (non-null) a frontend, and
    // this loop, ran past the array; a count above 32 is that overrun.
    assert_eq!(
        ports.len(),
        32,
        "4 ports x 8 buttons, then a null terminator"
    );
    for port in 0..4 {
        assert_eq!(
            ports.iter().filter(|&&p| p == port).count(),
            8,
            "port {port} should describe all 8 NES buttons: {ports:?}"
        );
    }
}

/// Executive-summary claim (L-S1). The core models the Four Score adapter
/// (`Nes::set_four_score`), but nothing in the libretro core could enable it,
/// so four-player games were two-player in RetroArch. A core option turns it
/// on, and then players 3 and 4 are read every frame.
#[test]
fn the_four_score_option_reads_players_three_and_four() {
    let _frontend = frontend();
    assert!(
        held(&DECLARED_VARS)
            .iter()
            .any(|k| k == "rustynes_four_score"),
        "the core must declare the Four Score option"
    );
    FOUR_SCORE_ON.store(true, SeqCst);
    VARS_CHANGED.store(true, SeqCst);
    assert!(load(NESTEST, true));
    JOYPAD_READS.store(0, SeqCst);
    run_frame();
    let with_four_score = JOYPAD_READS.load(SeqCst);
    FOUR_SCORE_ON.store(false, SeqCst);
    VARS_CHANGED.store(true, SeqCst);
    JOYPAD_READS.store(0, SeqCst);
    run_frame();
    let without = JOYPAD_READS.load(SeqCst);
    unload();
    assert_eq!(with_four_score, 4, "Four Score on: all four pads are read");
    assert_eq!(without, 2, "Four Score off again: two pads, as before");
}

/// A UNIF image of the committed CC0 nestest cartridge, built here rather than
/// read from a fixture.
///
/// v2.8.1's first version of the UNIF test included a `.unif` from
/// `tests/roms/nes-test-roms/`, which is gitignored: it existed on the machine
/// that wrote the test and nowhere else, so CI could not compile it (Copilot,
/// #557). Building the image from nestest needs no third-party file. Layout per
/// `rustynes_mappers::unif`: the 32-byte header (`UNIF`, revision, zeros),
/// then `MAPR` (the board name, NUL-terminated), `PRG0`, `CHR0` and `MIRR`.
fn nestest_as_unif() -> &'static [u8] {
    const PRG: usize = 16 * 1024;
    const CHR: usize = 8 * 1024;
    let prg = &NESTEST[16..16 + PRG];
    let chr = &NESTEST[16 + PRG..16 + PRG + CHR];
    let mut unif = Vec::new();
    unif.extend_from_slice(b"UNIF");
    unif.extend_from_slice(&7u32.to_le_bytes());
    unif.extend_from_slice(&[0; 24]);
    for (id, data) in [
        (b"MAPR", &b"NES-NROM-128\0"[..]),
        (b"PRG0", prg),
        (b"CHR0", chr),
        (b"MIRR", &[NESTEST[6] & 1][..]),
    ] {
        unif.extend_from_slice(id);
        unif.extend_from_slice(&u32::try_from(data.len()).expect("small").to_le_bytes());
        unif.extend_from_slice(data);
    }
    // Leaked once per test run: `load` hands the frontend a `'static` buffer.
    Box::leak(unif.into_boxed_slice())
}

/// libretro audit §3.4 (L-3.4b). The core declares `unf|unif` from v2.8.1;
/// this pins that it also LOADS one through the C ABI, so it does not declare
/// a format its load path refuses.
#[test]
fn a_unif_image_loads_through_the_c_abi() {
    let unif = nestest_as_unif();
    let _frontend = frontend();
    assert!(unif.starts_with(b"UNIF"), "the image is a UNIF container");
    assert!(load(unif, false), "a UNIF image must load");
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

/// Set the version byte of the LAST `CPU ` section in a serialized state. In
/// a single-console state that is the console's CPU section; in a dual
/// cabinet's container it is the SUB console's, which is decoded after the
/// main console has been applied.
fn reject_last_cpu_section(state: &mut [u8]) {
    let at = state
        .windows(4)
        .rposition(|w| w == b"CPU ")
        .expect("a CPU section");
    state[at + 4] = state[at + 4].wrapping_add(1);
}

/// Serialize, run on, try to unserialize the EARLY state with its last CPU
/// section rejected, and report (the call's result, whether a fresh serialize
/// equals the one taken just before the call).
fn a_rejected_unserialize(rom: &'static [u8]) -> (bool, bool) {
    assert!(load(rom, true));
    for _ in 0..10 {
        run_frame();
    }
    let size = serialize_size();
    let mut early = vec![0_u8; size];
    assert!(serialize(&mut early));
    for _ in 0..50 {
        run_frame();
    }
    let mut reference = vec![0_u8; size];
    assert!(serialize(&mut reference));
    reject_last_cpu_section(&mut early);
    let accepted = unserialize(&early);
    let mut after = vec![0_u8; size];
    assert!(serialize(&mut after));
    unload();
    (accepted, after == reference)
}

/// v2.9.0 re-audit NL-01 (core NC-03 / NC-05). `retro_unserialize` receives
/// every state a user loads — a slot file, a state from an older core, a
/// corrupt one — and routed it through `Nes::restore_quiet`, which had no
/// rollback: a state rejected at its CPU section returned `false` and left
/// the bus, PPU, APU and mapper from the file under the running game's CPU,
/// and RetroArch kept emulating that machine. For the Vs. `DualSystem`
/// cabinet a rejected SUB block left the main console restored. Both must
/// now return `false` AND leave the machine exactly as it was.
#[test]
fn a_rejected_unserialize_leaves_the_machine_as_it_was() {
    let _frontend = frontend();
    let (accepted, unchanged) = a_rejected_unserialize(NESTEST);
    assert!(
        !accepted,
        "a state with a rejected CPU section must not load"
    );
    assert!(unchanged, "a rejected unserialize changed the console");

    // nestest's PRG/CHR under a NES 2.0 Vs. DualSystem header (console type
    // Vs. System, byte-13 hardware type 5), as the re-audit's demonstration
    // built it. Leaked: `load` hands the bytes to the core as a frontend would.
    let mut dual = NESTEST.to_vec();
    dual[7] = 0x08 | 0x01;
    dual[13] = 0x50;
    let dual: &'static [u8] = Box::leak(dual.into_boxed_slice());
    let (accepted, unchanged) = a_rejected_unserialize(dual);
    assert!(
        !accepted,
        "a dual state with a rejected sub block must not load"
    );
    assert!(unchanged, "a rejected dual unserialize changed the cabinet");
}

/// v2.9.9 re-audit NL-11. v2.9.8 made every state from v2.9.7 and earlier
/// unloadable (`.rns` container epoch 3, ADR 0042), and the CHANGELOG promises
/// such a state is "refused with a clear error". `retro_unserialize` refused
/// it, but dropped the `SnapshotError` and returned a bare `false`, so the
/// frontend's log received nothing and the user saw only the frontend's own
/// generic failure. The refusal must now reach the log with the reason (the
/// format, and that older states have to be re-recorded), and a refusal for
/// any other reason must say why too. The machine stays as it was in both
/// cases (`a_rejected_unserialize_leaves_the_machine_as_it_was`).
/// NL-12 (v2.9.9 libretro re-audit): a serialize/unserialize round trip in
/// the middle of a run leaves the machine exactly where a run that never
/// restored is, so the next state serializes to the same bytes. Until the
/// APU section carried the resampler's synthesis state (APU v5) the two
/// differed in 24 bytes of filter state, which RetroArch's netplay CRC check
/// would report as a desync.
#[test]
fn a_mid_run_round_trip_serializes_like_a_straight_run() {
    let _frontend = frontend();
    assert!(load(NESTEST, true));
    let size = serialize_size();
    for _ in 0..10 {
        run_frame();
    }
    let mut mid = vec![0_u8; size];
    assert!(serialize(&mut mid));
    assert!(unserialize(&mid));
    for _ in 0..10 {
        run_frame();
    }
    let mut restored = vec![0_u8; size];
    assert!(serialize(&mut restored));
    unload();

    assert!(load(NESTEST, true));
    for _ in 0..20 {
        run_frame();
    }
    let mut straight = vec![0_u8; size];
    assert!(serialize(&mut straight));
    unload();
    assert!(
        restored == straight,
        "a round trip at frame 10 changed the state at frame 20"
    );
}

#[cfg(target_arch = "x86_64")]
#[test]
fn a_refused_unserialize_logs_the_reason() {
    use rustynes_core::save_state::MIN_FORMAT_VERSION;
    let _frontend = frontend();
    assert!(load(NESTEST, true));
    for _ in 0..10 {
        run_frame();
    }
    let size = serialize_size();
    let mut state = vec![0_u8; size];
    assert!(serialize(&mut state));
    // The container header: 8-byte magic, then the little-endian format.
    let mut old = state.clone();
    old[8..10].copy_from_slice(&(MIN_FORMAT_VERSION - 1).to_le_bytes());
    held(&LOGGED).clear();
    let old_accepted = unserialize(&old);
    let old_logged = held(&LOGGED).clone();

    let mut damaged = state;
    reject_last_cpu_section(&mut damaged);
    held(&LOGGED).clear();
    let damaged_accepted = unserialize(&damaged);
    let damaged_logged = held(&LOGGED).clone();
    unload();

    assert!(!old_accepted, "a pre-v2.9.8 state must not load");
    let old_line = old_logged.join("\n");
    assert!(
        old_line.contains("older RustyNES")
            && old_line.contains(&format!("format {}", MIN_FORMAT_VERSION - 1))
            && old_line.contains("re-record"),
        "an old state's refusal must name the format and the remedy, got {old_logged:?}"
    );
    assert!(!damaged_accepted, "a damaged state must not load");
    assert!(
        damaged_logged
            .iter()
            .any(|line| line.contains("save state refused") && line.contains("CPU ")),
        "any other refusal must carry the core's reason, got {damaged_logged:?}"
    );
}

/// v2.9.1 (NL-09): a Vs. `DualSystem` state now leaves `retro_serialize`
/// through `VsDualSystem::snapshot_into`. The test above only shows a state
/// being REFUSED; this one shows the frontend's own state coming back. Save
/// at frame 10, run 50 more, load the save, and a fresh save must equal it
/// byte for byte. Both consoles run the same program here, so this proves the
/// container's framing and lengths, not the order of the two blocks -- the
/// core's `snapshot_into_round_trips_into_a_fresh_cabinet` owns that.
#[test]
fn a_dual_cabinet_state_round_trips_through_the_abi() {
    let _frontend = frontend();
    let mut dual = NESTEST.to_vec();
    dual[7] = 0x08 | 0x01;
    dual[13] = 0x50;
    let dual: &'static [u8] = Box::leak(dual.into_boxed_slice());
    assert!(load(dual, true));
    for _ in 0..10 {
        run_frame();
    }
    let size = serialize_size();
    let mut saved = vec![0_u8; size];
    assert!(serialize(&mut saved));
    for _ in 0..50 {
        run_frame();
    }
    assert!(unserialize(&saved), "the cabinet's own state must load");
    let mut again = vec![0_u8; size];
    assert!(serialize(&mut again));
    unload();
    assert!(
        saved == again,
        "a loaded dual state must serialize back to itself"
    );
}

/// libretro audit §1.1 (L-1.1). A panic inside a frame used to cross the
/// `extern "C"` boundary and abort the frontend: in this test process it would
/// abort the test binary, which is what the mutation that removes
/// `contained` shows. Contained, `retro_run` returns, the core refuses further
/// emulation (poisoned), and the console is kept rather than dropped, so the
/// WRAM pointer the frontend holds still points at live memory. Reloading the
/// game clears the poison.
#[test]
fn a_panic_inside_a_frame_is_contained_and_the_memory_stays_valid() {
    let _frontend = frontend();
    assert!(load(NESTEST, true));
    run_frame();
    let ram = system_ram();
    INJECT_PANIC_IN_RUN.store(true, SeqCst);
    run_frame();
    let mut buf = vec![0_u8; serialize_size()];
    assert!(
        !serialize(&mut buf),
        "a poisoned core must refuse to serialize"
    );
    // The report reaches the frontend's log and names the fault itself, not
    // only the callback it happened in (review on #556).
    #[cfg(target_arch = "x86_64")]
    {
        let logged = LOGGED
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner)
            .clone();
        assert!(
            logged
                .iter()
                .any(|line| line.contains("internal error in retro_run")
                    && line.contains("injected by the C-ABI harness")),
            "the contained panic and its message must be logged, got {logged:?}"
        );
    }
    assert_eq!(
        system_ram(),
        ram,
        "the console must be kept after a panic, not dropped under the frontend"
    );
    unload();
    assert!(
        load(NESTEST, true),
        "a reload must work after a contained panic"
    );
    run_frame();
    let mut buf = vec![0_u8; serialize_size()];
    assert!(serialize(&mut buf), "reloading must clear the poison");
    unload();
}

/// `retro_deinit` releases the core's buffers (libretro audit §1.4: the
/// instance itself is never freed by `rust-libretro`), and the core still
/// works after a fresh `retro_init`, which a frontend reusing a loaded library
/// does.
#[test]
fn the_core_works_again_after_deinit_and_init() {
    let _frontend = frontend();
    // SAFETY: plain lifecycle calls, in the order a frontend makes them.
    unsafe {
        rust_libretro::retro_deinit();
        // libretro.h: `retro_set_environment` comes before `retro_init`, and
        // a frontend re-initialising a loaded library repeats it. It is also
        // where the core re-fetches the log interface `retro_deinit` cleared,
        // so leaving it out would make the log test depend on test order.
        rust_libretro::retro_set_environment(Some(environment));
        rust_libretro::retro_init();
    }
    assert!(load(NESTEST, true));
    run_frame();
    let mut buf = vec![0_u8; serialize_size()];
    assert!(serialize(&mut buf));
    unload();
}

/// `retro_deinit` then the calls a frontend reusing the library makes before
/// its next load, so the shared core is usable by the next test.
fn deinit_and_reinit() {
    // SAFETY: plain lifecycle calls, in the order libretro.h documents
    // (`retro_set_environment` before `retro_init`).
    unsafe {
        rust_libretro::retro_deinit();
        rust_libretro::retro_set_environment(Some(environment));
        rust_libretro::retro_init();
    }
    for port in 0..4 {
        set_port(port, RETRO_DEVICE_JOYPAD);
    }
}

/// v2.9.2 audit AUD-16. libretro.h says `retro_unload_game` is "Called
/// before retro_deinit", but `on_deinit` already caters for a frontend that
/// skips it (it writes the FDS disk and drops the consoles). That path freed
/// WRAM / SRAM / CIRAM while the frontend still held the memory-map
/// descriptors pointing into them, the use-after-free `on_unload_game`
/// withdraws the maps to prevent (L-1.3). A conforming frontend, which
/// unloads first, must see no extra `SET_MEMORY_MAPS` at deinit.
#[test]
fn deinit_without_unload_withdraws_the_memory_maps() {
    let _frontend = frontend();
    assert!(load(NESTEST, true));
    assert!(
        LAST_MAP_LEN.load(SeqCst) >= 2,
        "loading registers at least WRAM and CIRAM"
    );
    deinit_and_reinit();
    let left = LAST_MAP_LEN.load(SeqCst);

    // The conforming order: unload withdraws, deinit sends nothing more.
    assert!(load(NESTEST, true));
    unload();
    let sets = MAP_SETS.load(SeqCst);
    deinit_and_reinit();
    let sets_at_deinit = MAP_SETS.load(SeqCst) - sets;

    assert_eq!(
        left, 0,
        "retro_deinit without retro_unload_game must leave the frontend holding no descriptors"
    );
    assert_eq!(
        sets_at_deinit, 0,
        "after retro_unload_game, retro_deinit has nothing left to withdraw"
    );
}

/// v2.9.2 audit AUD-17. `retro_serialize_size` is answered from a snapshot
/// taken at load, and that snapshot used to go into a throwaway `Vec`, so the
/// first `retro_serialize` -- the first run-ahead or rollback frame -- grew
/// `serialize_buffer` from nothing (about 260 KB, 645 KB for a cabinet)
/// during gameplay. Sized into `serialize_buffer` itself, with the
/// expansion-device headroom reserved, no serialize reallocates it: not the
/// first, and not after a Zapper grows the state on both ports.
#[test]
fn no_serialize_reallocates_the_buffer_sized_at_load() {
    let _frontend = frontend();
    let growths = || SERIALIZE_BUFFER_GROWTHS.load(SeqCst);

    assert!(load(NESTEST, true));
    let before = growths();
    let _ = state();
    let first = growths() - before;
    for port in 0..2 {
        set_port(port, RETRO_DEVICE_LIGHTGUN);
    }
    run_frame(); // attaches both Zappers: the largest single-console state
    let _ = state();
    let with_zappers = growths() - before - first;
    unload();

    let mut dual = NESTEST.to_vec();
    dual[7] = 0x08 | 0x01;
    dual[13] = 0x50;
    assert!(load(Box::leak(dual.into_boxed_slice()), true));
    let before = growths();
    let _ = state();
    let dual_first = growths() - before;
    unload();

    assert_eq!(first, 0, "the first serialize reallocated serialize_buffer");
    assert_eq!(
        with_zappers, 0,
        "a Zapper on both ports outgrew the reserved headroom"
    );
    assert_eq!(
        dual_first, 0,
        "a cabinet's first serialize reallocated serialize_buffer"
    );
}

/// A copy of nestest with its header edited by `edit`, leaked so `load` can
/// hand the frontend a `'static` buffer (one small leak per test).
fn nestest_with(edit: impl FnOnce(&mut Vec<u8>)) -> &'static [u8] {
    let mut rom = NESTEST.to_vec();
    edit(&mut rom);
    Box::leak(rom.into_boxed_slice())
}

/// nestest's PRG and CHR under an MMC1 NES 2.0 header whose byte 10 is
/// `prg_ram_byte` (low nibble volatile, high nibble battery-backed PRG-RAM,
/// each `64 << n` bytes), with the battery bit when `battery`. `0x7A` gives
/// 64 KiB + 8 KiB = 73,728 bytes, the size three commercial MMC1 dumps
/// expose (libretro re-audit NL-06); `0x0A` gives 64 KiB; `0x09` 32 KiB.
fn mmc1_with_prg_ram(prg_ram_byte: u8, battery: bool) -> &'static [u8] {
    let prg = &NESTEST[16..16 + 16 * 1024];
    let chr = &NESTEST[16 + 16 * 1024..16 + 24 * 1024];
    let mut rom = vec![0u8; 16];
    rom[..4].copy_from_slice(b"NES\x1A");
    rom[4] = 8; // 128 KiB PRG: nestest's 16 KiB, eight times
    rom[5] = 1; // 8 KiB CHR
    rom[6] = 0x10 | if battery { 0x02 } else { 0 }; // mapper 1
    rom[7] = 0x08; // NES 2.0
    rom[10] = prg_ram_byte;
    for _ in 0..8 {
        rom.extend_from_slice(prg);
    }
    rom.extend_from_slice(chr);
    Box::leak(rom.into_boxed_slice())
}

/// The CPU-space (blank `addrspace`) descriptor that starts at `$6000`.
fn cpu_window(descs: &[Desc]) -> Option<Desc> {
    descs
        .iter()
        .find(|d| d.addrspace.is_empty() && d.start == 0x6000)
        .cloned()
}

fn memory_size(id: c_uint) -> usize {
    // SAFETY: a plain query.
    unsafe { rust_libretro::retro_get_memory_size(id) }
}

fn memory_data(id: c_uint) -> *mut c_void {
    // SAFETY: a plain query; the pointer is only compared, never dereferenced.
    unsafe { rust_libretro::retro_get_memory_data(id) }
}

/// libretro re-audit NL-02. RetroArch writes `RETRO_MEMORY_SAVE_RAM` to a
/// `.srm` and loads it back after `retro_load_game`, so exposing work RAM
/// without a battery gave 581 images of the local corpus a save file the
/// hardware never had, and restored stale RAM on the next boot. nestest's
/// header has no battery bit: it must expose no save RAM, and no descriptor
/// may carry the `SAVE_RAM` flag. With the bit set, the same 8 KiB is save RAM.
#[test]
fn save_ram_is_exposed_only_for_a_battery_backed_cartridge() {
    let _frontend = frontend();

    assert!(load(NESTEST, true));
    let descs = held(&LAST_MAP).clone();
    let (size, data) = (
        memory_size(RETRO_MEMORY_SAVE_RAM),
        memory_data(RETRO_MEMORY_SAVE_RAM),
    );
    unload();
    assert_eq!(size, 0, "a cartridge without a battery has no save RAM");
    assert!(data.is_null(), "and no save-RAM pointer");
    assert!(
        descs
            .iter()
            .all(|d| d.flags & u64::from(RETRO_MEMDESC_SAVE_RAM) == 0),
        "no descriptor may be flagged SAVE_RAM: {descs:?}"
    );
    // The work RAM stays visible to cheats and RetroAchievements, as plain
    // memory at $6000.
    let window = cpu_window(&descs).expect("volatile PRG-RAM is still described at $6000");
    assert_eq!(window.len, 0x2000);

    let battery = nestest_with(|rom| rom[6] |= 0x02);
    assert!(load(battery, true));
    let descs = held(&LAST_MAP).clone();
    let size = memory_size(RETRO_MEMORY_SAVE_RAM);
    let data = memory_data(RETRO_MEMORY_SAVE_RAM);
    unload();
    assert_eq!(size, 0x2000, "a battery-backed cartridge exposes its 8 KiB");
    assert!(!data.is_null());
    let window = cpu_window(&descs).expect("save RAM is described at $6000");
    assert_ne!(
        window.flags & u64::from(RETRO_MEMDESC_SAVE_RAM),
        0,
        "and flagged as save RAM"
    );
}

/// libretro re-audit NL-06. The save-RAM descriptor was
/// `{start: $6000, select: 0, len: sram_len}` whatever the size: 64 KiB
/// claimed `$6000-$15FFF`, past the 16-bit bus; 32 KiB claimed PRG-ROM at
/// `$8000-$DFFF`; and 73,728 bytes broke libretro.h's power-of-two rule.
/// Driven through the C ABI at all three sizes.
#[test]
fn save_ram_descriptors_have_the_shape_libretro_h_requires() {
    let _frontend = frontend();
    for (byte10, expect) in [(0x7A_u8, 73_728_usize), (0x0A, 65_536), (0x09, 32_768)] {
        let rom = mmc1_with_prg_ram(byte10, true);
        assert!(load(rom, true), "the synthetic MMC1 image loads");
        let descs: Vec<DescView> = held(&LAST_MAP).iter().map(DescView::from).collect();
        let size = memory_size(RETRO_MEMORY_SAVE_RAM);
        unload();
        assert_eq!(size, expect, "the whole buffer stays save RAM for the .srm");
        crate::tests::assert_descriptor_shape(&descs, expect, false);
    }
}

impl From<&Desc> for DescView {
    fn from(d: &Desc) -> Self {
        Self {
            ptr: d.ptr,
            offset: d.offset,
            start: d.start,
            select: d.select,
            len: d.len,
            addrspace: d.addrspace.clone(),
        }
    }
}

/// libretro re-audit NL-10. libretro.h, `retro_serialize`: "If failed, or
/// size is lower than retro_serialize_size(), it should return false". The
/// core succeeded whenever the state itself fitted, and the state is usually
/// 26 bytes smaller than the reported size (the expansion-device headroom),
/// so a buffer one byte short still returned true.
#[test]
fn serialize_refuses_a_buffer_smaller_than_it_asked_for() {
    let _frontend = frontend();
    assert!(load(NESTEST, true));
    run_frame();
    let size = serialize_size();
    let mut short = vec![0_u8; size - 1];
    let refused = !serialize(&mut short);
    let mut exact = vec![0_u8; size];
    let accepted = serialize(&mut exact);
    unload();
    assert!(
        refused,
        "a buffer below retro_serialize_size must be refused"
    );
    assert!(
        accepted,
        "a buffer of exactly retro_serialize_size must work"
    );
}

/// libretro re-audit NL-05. `rust-libretro` declares core options only on the
/// first `retro_set_environment` the process ever sees, and its instance is
/// never reset (L-1.4). A frontend that keeps the library loaded, calls
/// `retro_deinit`, and starts again with `retro_set_environment` +
/// `retro_init` got no `SET_VARIABLES`, so a frontend that drops its option
/// set at deinit had no Four Score option in the second session. Each
/// init cycle must declare the options once; a repeated
/// `retro_set_environment` inside one cycle does not need to.
#[test]
fn core_options_are_declared_again_after_deinit_and_init() {
    let _frontend = frontend();
    let before = SET_VARIABLES_CALLS.load(SeqCst);
    // SAFETY: plain lifecycle calls, in the order a frontend that reuses a
    // loaded library makes them (libretro.h: set_environment before init).
    unsafe {
        rust_libretro::retro_deinit();
        rust_libretro::retro_set_environment(Some(environment));
        rust_libretro::retro_init();
    }
    let after_cycle = SET_VARIABLES_CALLS.load(SeqCst);
    // SAFETY: as above; a second environment inside the same cycle.
    unsafe { rust_libretro::retro_set_environment(Some(environment)) };
    let after_repeat = SET_VARIABLES_CALLS.load(SeqCst);
    assert_eq!(
        after_cycle,
        before + 1,
        "a new init cycle must declare the core options"
    );
    assert!(
        held(&DECLARED_VARS)
            .iter()
            .any(|k| k == "rustynes_four_score"),
        "and the declaration must carry the Four Score option"
    );
    assert_eq!(
        after_repeat, after_cycle,
        "a repeated set_environment within one cycle declares nothing new"
    );
}

/// Re-send the environment, as a frontend does, with the disk-control
/// counters cleared first.
fn resend_environment() {
    DISK_V0_SETS.store(0, SeqCst);
    DISK_EXT_SETS.store(0, SeqCst);
    *held(&DISK_EXT) = None;
    // SAFETY: a plain lifecycle call; repeating it is permitted.
    unsafe { rust_libretro::retro_set_environment(Some(environment)) };
}

/// Ask the registered `get_image_label` for side `index` into a `len`-byte
/// buffer pre-filled with `0xFF`, and return what it reported and wrote.
fn label(ext: DiskExt, index: c_uint, len: usize) -> (bool, Vec<u8>) {
    let get = ext.get_image_label.expect("get_image_label is registered");
    let mut buf = vec![0xFF_u8; len.max(1)];
    // SAFETY: `buf` is valid for `len` bytes of writes for the call.
    let ok = unsafe { get(index, buf.as_mut_ptr().cast(), len) };
    (ok, buf)
}

/// libretro re-audit NL-07. The core registered only the v0 disk-control
/// interface, which has no `get_image_label`, so the "Side A" / "Side B"
/// labels it computes never reached a frontend even when that frontend
/// reported the extended interface. With the extended interface, the labels
/// must arrive NUL-terminated (the `rust-libretro` label copy wrote no
/// terminator: libretro audit L-1.5), a short buffer must be truncated
/// rather than overrun, and an index past the last side must be refused.
/// `set_initial_image` and `get_image_path` stay NULL, so the frontend does
/// not try to boot a remembered side (an FDS game boots from side A).
#[test]
fn disk_labels_reach_a_frontend_with_the_extended_interface() {
    let _frontend = frontend();
    resend_environment();
    assert_eq!(
        DISK_EXT_SETS.load(SeqCst),
        1,
        "version 1: the extended interface"
    );
    assert_eq!(DISK_V0_SETS.load(SeqCst), 0, "and not the v0 one as well");
    let ext = held(&DISK_EXT).expect("the extended callbacks were handed over");
    assert!(ext.set_initial_image.is_none() && ext.get_image_path.is_none());

    assert!(load(synthetic_disk(2), true), "a two-sided disk loads");
    let sides = ext.get_num_images.expect("get_num_images is registered");
    // SAFETY: a plain query through the registered callback.
    assert_eq!(unsafe { sides() }, 2);
    let (ok, buf) = label(ext, 0, 32);
    assert!(ok);
    assert_eq!(CStr::from_bytes_until_nul(&buf).ok(), Some(c"Side A"));
    let (ok, buf) = label(ext, 1, 32);
    assert!(ok);
    assert_eq!(CStr::from_bytes_until_nul(&buf).ok(), Some(c"Side B"));
    let (ok, buf) = label(ext, 0, 4);
    assert!(ok, "a short buffer gets a truncated label");
    assert_eq!(&buf[..4], b"Sid\0", "truncated, and still terminated");
    assert!(
        !label(ext, 2, 32).0,
        "index 2 of a two-sided disk is invalid"
    );
    assert!(
        !label(ext, 0, 0).0,
        "a zero-length buffer cannot hold a label"
    );
    let get = ext.get_image_label.expect("registered");
    // SAFETY: a null buffer, which the callback must refuse without writing.
    assert!(!unsafe { get(0, std::ptr::null_mut(), 32) });
    unload();

    // A frontend without the extended interface still gets the v0 one.
    DISK_VERSION.store(0, SeqCst);
    resend_environment();
    assert_eq!(DISK_V0_SETS.load(SeqCst), 1, "version 0: the v0 interface");
    assert_eq!(DISK_EXT_SETS.load(SeqCst), 0);
}

/// Serialize the loaded game into a fresh buffer of the reported size.
fn state() -> Vec<u8> {
    let mut buf = vec![0_u8; serialize_size()];
    assert!(serialize(&mut buf), "serialize");
    buf
}

fn contains(haystack: &[u8], needle: &[u8]) -> bool {
    haystack.windows(needle.len()).any(|w| w == needle)
}

/// libretro re-audit NL-03. An FDS game saves by writing to its disk, and
/// nothing in the libretro core ever read the written disk back out, so
/// every in-game save was lost when the game closed. The synthetic BIOS
/// writes a marker into side A through the drive registers, exactly as a
/// game's save routine does. The written disk must reach
/// `<save dir>/RustyNES/<hash of the original image>.fds.sav` when the game
/// is unloaded, and again a second after a write while it runs; and the next
/// load of the same (original) image must boot the saved disk. The FDS
/// snapshot carries the disk contents, so a save state shows which disk is
/// in the drive.
#[test]
fn fds_disk_writes_survive_closing_the_game() {
    let _frontend = frontend();
    let disk = synthetic_disk(1);
    let sha = Nes::from_disk(disk, &synthetic_bios())
        .expect("synthetic disk")
        .rom_sha256()
        .iter()
        .fold(String::new(), |mut s, b| {
            use std::fmt::Write as _;
            let _ = write!(s, "{b:02x}");
            s
        });
    let file = dirs()
        .save_path
        .join("RustyNES")
        .join(format!("{sha}.fds.sav"));
    let _ = std::fs::remove_file(&file);
    let written = |file: &std::path::Path| {
        std::fs::read(file)
            .ok()
            .is_some_and(|b| b.get(15..15 + DISK_MARKER.len()) == Some(DISK_MARKER))
    };

    // A clean boot: nothing written yet.
    assert!(load(disk, true));
    assert!(
        !contains(&state(), DISK_MARKER),
        "the original disk is clean"
    );
    // Unloading after the write: the disk is saved.
    for _ in 0..5 {
        run_frame();
    }
    assert!(contains(&state(), DISK_MARKER), "the BIOS wrote the disk");
    assert!(!file.exists(), "not yet: the write is under a second old");
    unload();
    assert!(
        written(&file),
        "unloading must save the written disk to {}",
        file.display()
    );

    // While running: a second after the write, without unloading.
    std::fs::remove_file(&file).expect("remove the save");
    assert!(load(disk, true));
    for _ in 0..70 {
        run_frame();
    }
    assert!(
        written(&file),
        "a written disk must be saved within a second"
    );
    unload();

    // The next boot of the original image boots the saved disk. No frame
    // runs, so the BIOS has not written anything this time.
    assert!(load(disk, true));
    let restored = contains(&state(), DISK_MARKER);
    unload();
    assert!(restored, "the saved disk must be the one in the drive");
    let _ = std::fs::remove_file(&file);
}

/// A 16 KiB NROM Vs. System cartridge (NES 2.0, console type 1) whose
/// program copies `$4016` to `$0000` and `$4017` to `$0001` forever, so the
/// coin, service and DIP bits the Vs. panel drives are visible in WRAM.
/// `dual` marks it a Vs. `DualSystem` board (byte 13 hardware type 5).
///
/// ```text
/// $C000  LDA #$01 / STA $4016 / LDA #$00 / STA $4016   strobe the pads
///        LDA $4016 / STA $00 / LDA $4017 / STA $01
///        JMP $C000
/// ```
fn vs_probe_rom(dual: bool) -> &'static [u8] {
    let mut rom = vec![0u8; 16 + 16 * 1024 + 8 * 1024];
    rom[..4].copy_from_slice(b"NES\x1A");
    rom[4] = 1; // 16 KiB PRG
    rom[5] = 1; // 8 KiB CHR
    rom[7] = 0x08 | 0x01; // NES 2.0, Vs. System
    rom[13] = if dual { 0x50 } else { 0x00 };
    let program = [
        0xA9, 0x01, 0x8D, 0x16, 0x40, 0xA9, 0x00, 0x8D, 0x16, 0x40, // strobe
        0xAD, 0x16, 0x40, 0x85, 0x00, // LDA $4016, STA $00
        0xAD, 0x17, 0x40, 0x85, 0x01, // LDA $4017, STA $01
        0x4C, 0x00, 0xC0, // JMP $C000
    ];
    rom[16..16 + program.len()].copy_from_slice(&program);
    // NMI, RESET, IRQ -> $C000 (the PRG is mirrored at $8000 and $C000).
    rom[16 + 0x3FFA..16 + 0x4000].copy_from_slice(&[0x00, 0xC0, 0x00, 0xC0, 0x00, 0xC0]);
    Box::leak(rom.into_boxed_slice())
}

/// The first two bytes of the loaded game's (main console's) WRAM.
fn wram_head() -> (u8, u8) {
    let ram = system_ram().cast::<u8>();
    assert!(!ram.is_null());
    // SAFETY: WRAM is 2 KiB and stays allocated while the game is loaded.
    unsafe { (*ram, *ram.add(1)) }
}

const L: u32 = 1 << RETRO_DEVICE_ID_JOYPAD_L;
const R: u32 = 1 << RETRO_DEVICE_ID_JOYPAD_R;

/// libretro re-audit NL-08. The core modelled the Vs. System coin acceptors
/// and service button (`Nes::insert_coin`, `set_vs_service`), and the
/// desktop bound them to keys, but no libretro input reached them: a Vs.
/// game waiting for a credit could not be given one. RetroPad L now drops a
/// coin (port 1: acceptor 1, port 2: acceptor 2) as a pulse of a few frames,
/// however long it is held, and R holds the service button; both are named
/// in the input descriptors while a Vs. cartridge is loaded.
#[test]
fn vs_system_coins_and_service_reach_the_game() {
    let _frontend = frontend();
    assert!(load(vs_probe_rom(false), true));
    let named = held(&DESCRIBED).clone();
    run_frame();
    let (idle, _) = wram_head();
    assert_eq!(
        idle & 0x64,
        0,
        "no coin and no service at rest: {idle:#04x}"
    );

    PADS[0].store(L, SeqCst);
    run_frame();
    let (coin, _) = wram_head();
    for _ in 0..6 {
        run_frame();
    }
    let (held_long, _) = wram_head();
    PADS[0].store(0, SeqCst);
    PADS[1].store(L, SeqCst);
    run_frame();
    let (coin2, _) = wram_head();
    PADS[1].store(0, SeqCst);
    for _ in 0..6 {
        run_frame();
    }
    PADS[0].store(R, SeqCst);
    run_frame();
    let (service, _) = wram_head();
    PADS[0].store(0, SeqCst);
    run_frame();
    let (released, _) = wram_head();
    unload();
    let described_after_unload = held(&DESCRIBED).clone();

    assert_ne!(
        coin & 0x20,
        0,
        "port 1 L drops a coin in acceptor 1: {coin:#04x}"
    );
    assert_eq!(
        held_long & 0x20,
        0,
        "a coin is a pulse, not a level: {held_long:#04x}"
    );
    assert_ne!(
        coin2 & 0x40,
        0,
        "port 2 L drops a coin in acceptor 2: {coin2:#04x}"
    );
    assert_ne!(service & 0x04, 0, "port 1 R holds service: {service:#04x}");
    assert_eq!(released & 0x04, 0, "and releases it: {released:#04x}");
    for (port, id) in [
        (0, RETRO_DEVICE_ID_JOYPAD_L),
        (1, RETRO_DEVICE_ID_JOYPAD_L),
        (0, RETRO_DEVICE_ID_JOYPAD_R),
    ] {
        assert!(
            named.iter().any(|(p, i, _)| *p == port && *i == id),
            "port {port} id {id} must be described for a Vs. cartridge: {named:?}"
        );
    }
    assert!(
        described_after_unload
            .iter()
            .all(|(_, id, _)| *id != RETRO_DEVICE_ID_JOYPAD_L),
        "the Vs. names go away with the cartridge"
    );
}

/// The same inputs on a Vs. `DualSystem` cabinet: ports 1-2 are the main
/// console's panel (acceptors 1-2, service), ports 3-4 the sub console's.
/// The main console's WRAM is the one the frontend sees.
#[test]
fn vs_dual_system_coins_reach_the_main_console() {
    let _frontend = frontend();
    assert!(load(vs_probe_rom(true), true));
    run_frame();
    PADS[0].store(L, SeqCst);
    run_frame();
    let (coin, _) = wram_head();
    PADS[0].store(0, SeqCst);
    // A coin on the SUB console's panel does not reach the main one.
    for _ in 0..6 {
        run_frame();
    }
    PADS[2].store(L, SeqCst);
    run_frame();
    let (sub_coin, _) = wram_head();
    unload();
    assert_ne!(coin & 0x20, 0, "port 1 L: main acceptor 1: {coin:#04x}");
    assert_eq!(
        sub_coin & 0x60,
        0,
        "port 3 L is the sub console's: {sub_coin:#04x}"
    );
}

/// Drive the Vs. probe cartridge for `real` presented frames with L held on
/// port 1 for frames 5 and 6, the way `RetroArch` run-ahead of `ahead` frames
/// drives a core: each presented frame is one `retro_run`, a
/// `retro_serialize`, `ahead` speculative `retro_run`s and a
/// `retro_unserialize` back. Returns the coin bit (`$00 & $20`) the game read
/// in each presented frame, as `1` / `.`.
fn vs_coin_under_run_ahead(ahead: usize, real: usize) -> String {
    assert!(load(vs_probe_rom(false), true));
    let mut buf = vec![0_u8; serialize_size()];
    let mut seen = String::new();
    for frame in 0..real {
        PADS[0].store(if (5..=6).contains(&frame) { L } else { 0 }, SeqCst);
        run_frame();
        let (b0, _) = wram_head();
        seen.push(if b0 & 0x20 == 0 { '.' } else { '1' });
        if ahead > 0 {
            assert!(serialize(&mut buf));
            for _ in 0..ahead {
                run_frame();
            }
            assert!(unserialize(&buf));
        }
    }
    PADS[0].store(0, SeqCst);
    unload();
    seen
}

/// v2.9.9 re-audit NL-13. The Vs. coin pulse was counted in `retro_run`
/// calls, not emulated frames, and the latch it drives is host input that a
/// save state does not carry. Run-ahead (and preemptive frames, rewind,
/// netplay rollback) call `retro_run` more than once per presented frame, so
/// the 3-frame (50 ms) pulse shrank to 2 frames at run-ahead 1 and to 1
/// frame (17 ms) at run-ahead 2 -- under the 40-70 ms the real coin switch
/// closes for. The pulse is now timed against the console's own frame
/// counter, which a restore rewinds, so it is the same three frames whatever
/// the run-ahead setting.
#[test]
fn the_vs_coin_pulse_is_three_emulated_frames_under_run_ahead() {
    let _frontend = frontend();
    let expected = ".....111............";
    for ahead in 0..=2 {
        assert_eq!(
            vs_coin_under_run_ahead(ahead, expected.len()),
            expected,
            "coin bit per presented frame at run-ahead {ahead}"
        );
    }
}

/// NL-13's rollback half. A frontend that restores a state from before the
/// coin went in and replays (netplay rollback, rewind) must see the coin
/// where the REPLAYED input puts it. Counting calls, the latch kept whatever
/// the last call before the restore left in it -- host input is not in the
/// state -- so the replay began with a coin nobody had inserted yet; and a
/// panel that remembered the press across the restore would latch it on the
/// original frame even when the replayed input presses L later (a netplay
/// peer's corrected input).
#[test]
fn a_rollback_across_the_vs_coin_follows_the_replayed_input() {
    let _frontend = frontend();
    assert!(load(vs_probe_rom(false), true));
    let mut before = vec![0_u8; serialize_size()];
    let play = |from: usize, to: usize, press: std::ops::RangeInclusive<usize>| {
        let mut seen = String::new();
        for frame in from..to {
            PADS[0].store(if press.contains(&frame) { L } else { 0 }, SeqCst);
            run_frame();
            seen.push(if wram_head().0 & 0x20 == 0 { '.' } else { '1' });
        }
        seen
    };
    let lead_in = play(0, 4, 5..=6);
    assert!(serialize(&mut before));
    // The first timeline presses L on frames 5 and 6. Roll back to the end
    // of frame 3 with L still held, and replay with L on frames 7 and 8.
    let first = play(4, 7, 5..=6);
    assert!(unserialize(&before));
    let replay = play(4, 12, 7..=8);
    PADS[0].store(0, SeqCst);
    unload();
    assert_eq!(lead_in, "....");
    assert_eq!(first, ".11", "the first timeline's coin goes in on frame 5");
    assert_eq!(
        replay, "...111..",
        "the replayed timeline's coin goes in on frame 7, and only there"
    );
}

/// #583 review (CodeRabbit). A restore to the very frame the press was made
/// on is still a restore: the frontend saved before frame N, the player
/// pressed L during frame N, and the replay of frame N has no press (a
/// netplay peer's corrected input, a rewind). The rollback test above only
/// covered restores to frames BEFORE the press, and a record whose start was
/// the restored frame survived, so the replay latched a coin nobody pressed.
#[test]
fn a_restore_to_the_press_frame_drops_a_coin_the_replay_does_not_press() {
    let _frontend = frontend();
    assert!(load(vs_probe_rom(false), true));
    let mut at_press = vec![0_u8; serialize_size()];
    let play = |from: usize, to: usize, press: Option<usize>| {
        let mut seen = String::new();
        for frame in from..to {
            PADS[0].store(if press == Some(frame) { L } else { 0 }, SeqCst);
            run_frame();
            seen.push(if wram_head().0 & 0x20 == 0 { '.' } else { '1' });
        }
        seen
    };
    let lead_in = play(0, 5, None);
    assert!(serialize(&mut at_press));
    let first = play(5, 6, Some(5));
    assert!(unserialize(&at_press));
    let replay = play(5, 10, None);
    PADS[0].store(0, SeqCst);
    unload();
    assert_eq!(lead_in, ".....");
    assert_eq!(first, "1", "the first timeline's coin goes in on frame 5");
    assert_eq!(replay, ".....", "the replay never presses L, so no coin");
}

/// `frames` frames of `nes` with no input, as XRGB8888 (the core's R/B swap).
fn oracle_frame(mut nes: Nes, frames: u32) -> Vec<u8> {
    for _ in 0..frames {
        nes.run_frame();
    }
    let mut out = nes.framebuffer().to_vec();
    for px in out.chunks_exact_mut(4) {
        px.swap(0, 2);
    }
    out
}

/// libretro re-audit NL-08, the palette through the load path. The core
/// loaded Vs. dumps through `Emu::from_rom` alone, so an iNES 1.0 dump
/// rendered with the parser's default 2C03 palette instead of the one the
/// Vs. database names. For the first single-console dump under `tests/roms`
/// that the database lists with a different PPU, the frame the core presents
/// after `FRAMES` frames must equal a console built with the database
/// applied, and differ from one without it. The committed corpus has no Vs.
/// dump, so in CI this finds none and checks nothing; locally it runs on the
/// gitignored `external/` dumps.
#[test]
fn a_vs_dump_in_the_database_renders_with_its_palette() {
    const FRAMES: u32 = 300;
    let root = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../../tests/roms");
    let mut paths = Vec::new();
    crate::tests::walk_nes(&root, &mut paths);
    paths.sort();
    let pick = paths.iter().find_map(|path| {
        let bytes = std::fs::read(path).ok()?;
        let nes = Nes::from_rom(&bytes).ok()?;
        let entry = rustynes_core::vs_db::lookup(&nes)?;
        (!entry.dual_system && nes.is_vs_system()).then_some((path.clone(), bytes, entry))
    });
    let Some((path, bytes, entry)) = pick else {
        eprintln!("no Vs. database dump under tests/roms; skipped");
        return;
    };
    let with_db = {
        let mut nes = Nes::from_rom(&bytes).expect("loads");
        nes.set_vs_ppu_type(entry.vs_ppu_type);
        nes.set_vs_dip(entry.vs_dip);
        oracle_frame(nes, FRAMES)
    };
    let without = oracle_frame(Nes::from_rom(&bytes).expect("loads"), FRAMES);

    let _frontend = frontend();
    KEEP_FRAME.store(true, SeqCst);
    assert!(load(Box::leak(bytes.into_boxed_slice()), true));
    for _ in 0..FRAMES {
        run_frame();
    }
    let presented = held(&LAST_FRAME).clone();
    unload();
    KEEP_FRAME.store(false, SeqCst);

    assert_ne!(
        with_db,
        without,
        "{}: pick a dump whose palette differs",
        path.display()
    );
    assert!(
        presented == with_db,
        "{}: the core must present the database's palette",
        path.display()
    );
}

/// v2.9.8 — the core applies the game database's load-time corrections,
/// through the same two functions every platform calls
/// (`rustynes_gamedb::correct_rom` / `correct_console`).
///
/// Until v2.9.8 the core handed the frontend's bytes straight to
/// `Emu::from_rom`, so a RetroArch user got none of the database's mapper,
/// submapper or region fixes. The image is one the database matches as
/// Gradius (Europe) with every corrected field wrong: uncorrected it is NROM
/// with 32 KiB of CHR, which NROM refuses, so `retro_load_game` failed;
/// corrected it is CNROM at PAL timing, and the frontend must be told PAL.
/// (The mirroring half is pinned below the C ABI, by
/// `tests::a_cartridge_gets_the_game_database_mirroring`: no libretro call
/// reports a nametable arrangement.)
#[test]
fn a_cartridge_gets_the_game_database_corrections() {
    let rom = rustynes_gamedb::test_support::gradius_europe_with_a_wrong_header();
    let _frontend = frontend();
    assert!(
        load(Box::leak(rom.into_boxed_slice()), true),
        "the corrected image must load (uncorrected, NROM refuses it)"
    );
    // SAFETY: a plain query with no arguments, made while a game is loaded.
    let region = unsafe { rust_libretro::retro_get_region() };
    run_frame();
    unload();
    assert_eq!(
        region, RETRO_REGION_PAL,
        "the row's PAL region reaches the frontend"
    );
}
