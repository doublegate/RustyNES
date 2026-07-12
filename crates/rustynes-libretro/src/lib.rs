//! RustyNES Libretro Core.
//!
//! This crate implements the C ABI boundary for the `rustynes-core` engine, exposing
//! the standard libretro lifecycle hooks (`retro_init`, `retro_load_game`, `retro_run`, etc.)
//! required by RetroArch and other compatible frontends.
//!
//! # Architecture
//!
//! The Libretro wrapper operates as a thin, safe facade over the `Nes` emulator struct.
//! Because the emulator guarantees strict cycle-accuracy (a lockstep master clock for the
//! CPU/PPU/APU) and strict determinism, this crate avoids mutating emulation flow.
//!
//! - **Video**: Native 256x240 framebuffers are handed off directly to `VideoContext`.
//!   A Vs. `DualSystem` cabinet (two cross-wired consoles) instead composes its two
//!   256x240 framebuffers into a single 512x240 side-by-side image, presented as a
//!   variable-width frame within the 512-wide `max_width` geometry advertised up front.
//! - **Audio**: Audio is drained per frame and interleaved (left/right) into a pooled
//!   buffer before pushing via `batch_audio_samples`. The accumulator relies on a
//!   pre-allocated array (or sufficient `Vec` capacity) to honor the hot-path allocation bans.
//! - **Input**: The Joypad API is polled each frame and bitmasked into `rustynes_core::Buttons`.
//!   For a `DualSystem` cabinet libretro ports 0/1 drive the MAIN console's P1/P2 and
//!   ports 2/3 drive the SUB console's P1/P2 (matching `VsDualSystem::set_buttons`).
//! - **Save States & Memory Maps**: Direct pointers to WRAM, SRAM, and VRAM are provided
//!   safely by isolating the memory accessors in the core. Save states serialize statically
//!   sized binary blobs natively through `Nes::snapshot_core_into` (single console) or
//!   `VsDualSystem::snapshot` (dual cabinet).
//!
//! # Vs. `DualSystem` present path (v2.1.10 "Web Parity")
//!
//! The core already models the four Vs. `DualSystem` arcade boards (`Emu::Dual`,
//! `rustynes_core::VsDualSystem`). This wrapper detects them at load through
//! [`rustynes_core::Emu::from_rom`] — which OR's the NES 2.0 header Vs. type with the
//! SHA-keyed `vs_db` — and, when a cabinet is loaded, steps BOTH consoles each
//! `retro_run`, composes their framebuffers side-by-side, and presents the 512x240
//! result. The deterministic core is untouched; the dual branch is purely a parallel
//! present/serialize path, exactly mirroring the desktop frontend's `emu.dual` branch.

// We allow `unsafe_code` globally for this crate because it is an FFI boundary
// wrapper (like `rustynes-frontend` and `rustynes-cheevos`). All raw pointers
// originate from or are delivered to the frontend environment.
#![allow(unsafe_code)]
#![allow(clippy::wildcard_imports)]
#![allow(clippy::cast_possible_truncation)]
#![allow(clippy::doc_markdown)]
#![allow(clippy::ptr_as_ptr)]
#![warn(missing_docs)]

use rust_libretro::{
    contexts::*,
    core::{Core, CoreOptions},
    retro_core,
    sys::*,
    types::*,
};
use rustynes_core::{Emu, Nes, VsDualSystem};
use std::ffi::CString;

/// NES native framebuffer width in pixels (one console).
const NES_W: usize = 256;
/// NES native framebuffer height in pixels.
const NES_H: usize = 240;
/// Composed width of a Vs. `DualSystem` side-by-side present (two consoles).
const DUAL_W: usize = NES_W * 2;

/// The central libretro core structure for RustyNES.
///
/// This struct holds the underlying cycle-accurate `Nes` emulator instance alongside
/// the operational buffers necessary to interface with libretro's batch APIs. It is
/// statically instantiated via the `retro_core!` macro.
pub struct RustyNesLibretro {
    /// The cycle-accurate RustyNES core instance (single-console carts).
    ///
    /// Exists as an `Option` because `retro_init` fires before `retro_load_game`.
    /// Mutually exclusive with [`Self::dual`]: exactly one is `Some` while a ROM is
    /// loaded, matching the desktop frontend's `emu.nes` / `emu.dual` invariant.
    nes: Option<Nes>,

    /// A loaded Vs. `DualSystem` cabinet (two cross-wired consoles), when the ROM
    /// resolves to one of the four dual boards via [`Emu::from_rom`]. Boxed because
    /// `VsDualSystem` owns two full `Nes` instances. Mutually exclusive with
    /// [`Self::nes`]; drives the 512x240 side-by-side present path in `on_run`.
    dual: Option<Box<VsDualSystem>>,

    /// Intermediate buffer for left/right interleaved audio samples.
    ///
    /// Pre-allocated with capacity to hold multiple frames of audio, ensuring
    /// the hot `on_run` loop avoids any heap allocations.
    audio_buffer: Vec<i16>,

    /// Intermediate buffer for raw floating-point audio samples from the core.
    ///
    /// Pre-allocated to avoid heap allocations when draining audio batches.
    audio_float_buffer: Vec<f32>,

    /// Intermediate buffer for the video framebuffer.
    ///
    /// Pre-allocated to hold 256x240 RGBA8 pixels. Used to swap R and B channels
    /// to match the XRGB8888 libretro pixel format.
    video_buffer: Vec<u8>,

    /// Pre-computed save state size (constant for a given ROM and mapper).
    ///
    /// Stored upon ROM loading to satisfy libretro's `get_serialize_size` contract,
    /// guaranteeing the frontend allocates a precisely sized buffer.
    serialize_size: usize,

    /// Pre-allocated buffer for snapshot serialization.
    serialize_buffer: Vec<u8>,
}

impl Default for RustyNesLibretro {
    fn default() -> Self {
        Self {
            nes: None,
            dual: None,
            // 4096 samples comfortably holds ~85ms of audio at 48kHz,
            // well beyond the 16.6ms standard 60Hz frame delivery.
            audio_buffer: Vec::with_capacity(4096),
            audio_float_buffer: Vec::with_capacity(4096),
            // Sized for the widest present (512x240 Vs. DualSystem side-by-side)
            // so the hot path never reallocates even in dual mode.
            video_buffer: Vec::with_capacity(DUAL_W * NES_H * 4),
            serialize_size: 0,
            serialize_buffer: Vec::new(),
        }
    }
}

impl CoreOptions for RustyNesLibretro {}

#[repr(C)]
struct RetroGameInfoExt {
    full_path: *const std::os::raw::c_char,
    archive_path: *const std::os::raw::c_char,
    archive_file: *const std::os::raw::c_char,
    dir: *const std::os::raw::c_char,
    name: *const std::os::raw::c_char,
    ext: *const std::os::raw::c_char,
    meta_data: *const std::os::raw::c_char,
    data: *const std::os::raw::c_void,
    size: usize,
    file_in_archive: bool,
    persistent_data: bool,
}

/// Translate a libretro joypad snapshot for `port` into RustyNES `Buttons`.
///
/// Factored out of `on_run` so both the single-console and Vs. `DualSystem` present
/// paths share one mapping (ports 0/1 → main P1/P2, ports 2/3 → sub P1/P2 in dual).
fn joypad_to_buttons(ctx: &mut RunContext, port: u32) -> rustynes_core::Buttons {
    let jp = ctx.get_joypad_state(port, 0);
    let mut bt = rustynes_core::Buttons::empty();
    if jp.contains(JoypadState::A) {
        bt |= rustynes_core::Buttons::A;
    }
    if jp.contains(JoypadState::B) {
        bt |= rustynes_core::Buttons::B;
    }
    if jp.contains(JoypadState::SELECT) {
        bt |= rustynes_core::Buttons::SELECT;
    }
    if jp.contains(JoypadState::START) {
        bt |= rustynes_core::Buttons::START;
    }
    if jp.contains(JoypadState::UP) {
        bt |= rustynes_core::Buttons::UP;
    }
    if jp.contains(JoypadState::DOWN) {
        bt |= rustynes_core::Buttons::DOWN;
    }
    if jp.contains(JoypadState::LEFT) {
        bt |= rustynes_core::Buttons::LEFT;
    }
    if jp.contains(JoypadState::RIGHT) {
        bt |= rustynes_core::Buttons::RIGHT;
    }
    bt
}

/// Copy one RGBA8 NES scanline into an XRGB8888 destination scanline, swapping the
/// R and B channels (RGBA in memory → B G R X for libretro's XRGB8888). `dst` and
/// `src` must each be at least `NES_W * 4` bytes.
#[inline]
fn blit_scanline_rgba_to_xrgb(dst: &mut [u8], src: &[u8]) {
    for x in 0..NES_W {
        let d = x * 4;
        let s = x * 4;
        dst[d] = src[s + 2]; // B
        dst[d + 1] = src[s + 1]; // G
        dst[d + 2] = src[s]; // R
        dst[d + 3] = src[s + 3]; // X (alpha byte, ignored by XRGB8888)
    }
}

impl RustyNesLibretro {
    /// Convert `produced` mono `f32` samples from `audio_float_buffer` into
    /// interleaved stereo `i16` and push them. Shared by both present paths so the
    /// audio scaling / interleave logic lives in exactly one place. The buffers are
    /// pre-allocated, so this stays allocation-free on the hot path.
    fn push_audio(&mut self, ctx: &mut RunContext, produced: usize) {
        self.audio_buffer.clear();
        for &sample in &self.audio_float_buffer[..produced] {
            // RustyNES APU outputs bipolar ~[-0.5, 0.5], so we scale by 65535.0.
            let s16 = (sample * 65535.0).clamp(-32768.0, 32767.0) as i16;
            // Duplicate the sample for stereo interleaving (Left, Right).
            self.audio_buffer.push(s16);
            self.audio_buffer.push(s16);
        }
        rust_libretro::contexts::AudioContext::from(&mut *ctx)
            .batch_audio_samples(&self.audio_buffer);
    }

    /// The classic single-console present path: 256x240 XRGB8888 + one audio stream.
    fn run_single(&mut self, ctx: &mut RunContext) {
        ctx.poll_input();
        // Port 0 → Player 1, Port 1 → Player 2.
        let b0 = joypad_to_buttons(ctx, 0);
        let b1 = joypad_to_buttons(ctx, 1);
        {
            let nes = self.nes.as_mut().expect("run_single: nes present");
            nes.set_buttons(0, b0);
            nes.set_buttons(1, b1);
            // Advance the emulator clock by precisely one frame (the lockstep routine
            // that drives CPU/PPU/APU progression). The returned framebuffer borrow is
            // dropped immediately; we re-read it below via `framebuffer()` to keep the
            // video copy disjoint from the audio drain.
            nes.run_frame();
        }
        self.video_buffer.clear();
        self.video_buffer
            .extend_from_slice(self.nes.as_ref().expect("nes present").framebuffer());
        for chunk in self.video_buffer.chunks_exact_mut(4) {
            chunk.swap(0, 2); // RGBA8 → XRGB8888 (in-memory B G R X).
        }
        ctx.draw_frame(&self.video_buffer, NES_W as u32, NES_H as u32, NES_W * 4);

        self.audio_float_buffer.resize(4096, 0.0);
        let produced = self
            .nes
            .as_mut()
            .expect("nes present")
            .drain_audio_into(&mut self.audio_float_buffer);
        self.push_audio(ctx, produced);
    }

    /// The Vs. `DualSystem` present path: step BOTH consoles, compose their two
    /// 256x240 framebuffers side-by-side into a single 512x240 XRGB8888 image, and
    /// present it. Only the MAIN console's audio is played (one stream, matching the
    /// desktop frontend); the SUB console's APU ring is drained-and-discarded to keep
    /// it bounded. The deterministic core is untouched — this is a parallel present.
    fn run_dual(&mut self, ctx: &mut RunContext) {
        ctx.poll_input();
        // Ports 0/1 drive the MAIN console's P1/P2; ports 2/3 the SUB console's.
        let buttons = [
            joypad_to_buttons(ctx, 0),
            joypad_to_buttons(ctx, 1),
            joypad_to_buttons(ctx, 2),
            joypad_to_buttons(ctx, 3),
        ];
        {
            let dual = self.dual.as_mut().expect("run_dual: dual present");
            for (port, btn) in buttons.into_iter().enumerate() {
                dual.set_buttons(port, btn);
            }
            dual.run_frame();
        }
        self.compose_dual();
        ctx.draw_frame(&self.video_buffer, DUAL_W as u32, NES_H as u32, DUAL_W * 4);

        // Main console audio (the presented stream).
        self.audio_float_buffer.resize(4096, 0.0);
        let produced = self
            .dual
            .as_mut()
            .expect("dual present")
            .main_mut()
            .drain_audio_into(&mut self.audio_float_buffer);
        self.push_audio(ctx, produced);

        // Bound the SUB console's APU buffer even though its audio is not played:
        // drain it into a small stack scratch, looping until a partial fill signals
        // the ring is empty. Stack-allocated, so no heap traffic on the hot path.
        let mut scratch = [0.0f32; 1024];
        let dual = self.dual.as_mut().expect("dual present");
        while dual.sub_mut().drain_audio_into(&mut scratch) == scratch.len() {}
    }

    /// Compose the dual cabinet's two framebuffers into `video_buffer` as a 512x240
    /// XRGB8888 image (MAIN on the left, SUB on the right). Pre-sized once so the
    /// per-frame `resize` never reallocates.
    fn compose_dual(&mut self) {
        self.video_buffer.clear();
        self.video_buffer.resize(DUAL_W * NES_H * 4, 0);
        let dual = self.dual.as_ref().expect("compose_dual: dual present");
        let main = dual.main_framebuffer();
        let sub = dual.sub_framebuffer();
        for y in 0..NES_H {
            let src = y * NES_W * 4;
            let dst = y * DUAL_W * 4;
            // Left half ← MAIN, right half ← SUB. Each half is one NES scanline.
            let (left, right) = self.video_buffer[dst..dst + DUAL_W * 4].split_at_mut(NES_W * 4);
            blit_scanline_rgba_to_xrgb(left, &main[src..src + NES_W * 4]);
            blit_scanline_rgba_to_xrgb(right, &sub[src..src + NES_W * 4]);
        }
    }
}

impl Core for RustyNesLibretro {
    fn get_info(&self) -> SystemInfo {
        SystemInfo {
            library_name: CString::new("RustyNES").unwrap(),
            library_version: CString::new(env!("CARGO_PKG_VERSION")).unwrap(),
            valid_extensions: CString::new("nes|fds").unwrap(),
            need_fullpath: false,
            block_extract: false,
        }
    }

    fn on_get_av_info(&mut self, _ctx: &mut GetAvInfoContext) -> retro_system_av_info {
        // Return standard NTSC geometries. The core runs internally at ~60.0988 FPS
        // for NTSC standard. We match the internal audio mixing rate of 44.1kHz.
        retro_system_av_info {
            geometry: retro_game_geometry {
                base_width: 256,
                base_height: 240,
                // max_width is 512 so a Vs. DualSystem cabinet's 512x240 side-by-side
                // present fits without a geometry renegotiation: RetroArch honors a
                // per-frame width up to max_width, so a single-console 256x240 frame and
                // a dual 512x240 frame both draw correctly against the same AV info.
                max_width: DUAL_W as u32,
                max_height: 240,
                aspect_ratio: 0.0,
            },
            timing: retro_system_timing {
                fps: 60.0988,
                sample_rate: 44100.0,
            },
        }
    }

    fn on_set_environment(&mut self, _initial: bool, ctx: &mut SetEnvironmentContext) {
        // SAFETY: The Libretro API guarantees that the context provides a valid environment
        // callback pointer. `set_pixel_format` and `set_input_descriptors` invoke safe FFI
        // abstractions over that valid pointer.
        unsafe {
            let generic_ctx: GenericContext = (&*ctx).into();
            let cb = *generic_ctx.environment_callback();

            // XRGB8888 is preferred by RustyNES because it maps well to standard 32-bit GPU textures.
            if !rust_libretro::environment::set_pixel_format(cb, PixelFormat::XRGB8888) {
                eprintln!(
                    "[RustyNES] Error: Frontend rejected XRGB8888 pixel format. Colors will be broken."
                );
            }

            // Register standardized controller layouts for the frontend to bind against.
            let descriptors = rust_libretro::input_descriptors!(
                { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_A, "NES A Button" },
                { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_B, "NES B Button" },
                { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_SELECT, "Select" },
                { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_START, "Start" },
                { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_UP, "D-Pad Up" },
                { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_DOWN, "D-Pad Down" },
                { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_LEFT, "D-Pad Left" },
                { 0, RETRO_DEVICE_JOYPAD, 0, RETRO_DEVICE_ID_JOYPAD_RIGHT, "D-Pad Right" }
            );
            rust_libretro::environment::set_input_descriptors(cb, &descriptors);
        }
    }

    fn on_load_game(
        &mut self,
        _game: Option<retro_game_info>,
        ctx: &mut LoadGameContext,
    ) -> Result<(), Box<dyn std::error::Error>> {
        // We use `GET_GAME_INFO_EXT` directly via the raw environment callback.
        let ext_info = unsafe {
            let generic_ctx: GenericContext = (&*ctx).into();
            let cb = generic_ctx.environment_callback().unwrap();
            let mut ptr: *const RetroGameInfoExt = std::ptr::null();

            // SAFETY: `cb` is a valid function pointer supplied by the libretro frontend
            // via the environment context. If the callback returns true, the spec guarantees
            // `ptr` is set to a valid, aligned, frontend-owned `RetroGameInfoExt` whose
            // lifetime is at least as long as this `on_load_game` invocation.
            // `as_ref()` returns `None` if `ptr` remains null (a spec-violating frontend
            // that returns `true` without setting the pointer), ensuring we never produce
            // a reference from an invalid address.
            if cb(
                rust_libretro::sys::RETRO_ENVIRONMENT_GET_GAME_INFO_EXT,
                std::ptr::addr_of_mut!(ptr).cast::<std::os::raw::c_void>(),
            ) {
                ptr.as_ref()
            } else {
                None
            }
        }
        .ok_or("Frontend does not support get_game_info_ext")?;

        let rom_data = if ext_info.data.is_null() {
            return Err("ext_info data pointer is NULL. The frontend did not load the ROM into memory (need_fullpath is false).".into());
        } else {
            eprintln!("[RustyNES] ext_info data is valid. Size: {}", ext_info.size);
            // SAFETY: `data` is non-null (checked above). The libretro spec guarantees
            // the pointer references a valid, contiguous byte slice of exactly `size`
            // bytes, owned by the frontend for the duration of this call.
            let slice =
                unsafe { std::slice::from_raw_parts(ext_info.data.cast::<u8>(), ext_info.size) };
            slice.to_vec()
        };

        // `Emu::from_rom` picks the right shape for the cart: a `VsDualSystem` for the
        // four Vs. DualSystem boards (detected via the NES 2.0 header Vs. type OR the
        // SHA-keyed `vs_db`), else a standard single `Nes`. This is the SAME detection
        // the desktop frontend uses, so the libretro core presents dual cabinets
        // identically (two consoles side-by-side) instead of booting a single console
        // that would hang waiting on its cross-wired partner.
        let emu = match Emu::from_rom(&rom_data) {
            Ok(e) => e,
            Err(e) => {
                eprintln!("[RustyNES] Failed to parse ROM: {e:?}");
                return Err(format!("Failed to load ROM: {e:?}").into());
            }
        };

        // Save state sizes in RustyNES are strictly deterministic for a given ROM image.
        // We evaluate the snapshot footprint once during initialization to satisfy
        // libretro's serialization size querying contract. Clear whichever emulator
        // shape a prior load left behind so the two Options stay mutually exclusive.
        match emu {
            Emu::Single(nes) => {
                let nes = *nes;
                let mut tmp = Vec::new();
                nes.snapshot_core_into(&mut tmp);
                self.serialize_size = tmp.len();
                self.nes = Some(nes);
                self.dual = None;
                eprintln!("[RustyNES] Loaded single-console cart.");
            }
            Emu::Dual(dual) => {
                // The dual snapshot is a self-describing blob of both consoles; size it
                // once here (it is deterministic for a given ROM, like the single case).
                self.serialize_size = dual.snapshot().len();
                self.dual = Some(dual);
                self.nes = None;
                eprintln!("[RustyNES] Loaded Vs. DualSystem cabinet (512x240 side-by-side).");
            }
        }
        Ok(())
    }

    fn on_run(&mut self, ctx: &mut RunContext, _delta_us: Option<i64>) {
        // Two mutually-exclusive shapes: a single console, or a Vs. DualSystem
        // cabinet. The dual branch steps both consoles and presents a 512x240
        // side-by-side image; otherwise the classic single-console 256x240 path runs.
        if self.dual.is_some() {
            self.run_dual(ctx);
        } else if self.nes.is_some() {
            self.run_single(ctx);
        }
    }

    fn get_memory_data(
        &mut self,
        id: std::os::raw::c_uint,
        _ctx: &mut GetMemoryDataContext,
    ) -> *mut std::os::raw::c_void {
        // Expose zero-cost direct memory maps for RetroAchievements and cheat engines.
        // Memory boundary enforcement remains safe within the `rustynes_core` design.
        // In dual mode the MAIN console is the one RetroAchievements / cheats target
        // (its memory map is where gameplay state lives); expose it, matching the
        // single-console mapping. `main_mut()` yields the MAIN `Nes`.
        let nes = match (self.nes.as_mut(), self.dual.as_mut()) {
            (Some(nes), _) => nes,
            (None, Some(dual)) => dual.main_mut(),
            (None, None) => return std::ptr::null_mut(),
        };
        match id {
            RETRO_MEMORY_SAVE_RAM => {
                let sram = nes.sram_mut();
                if sram.is_empty() {
                    std::ptr::null_mut()
                } else {
                    sram.as_mut_ptr().cast::<std::os::raw::c_void>()
                }
            }
            RETRO_MEMORY_SYSTEM_RAM => nes.wram_mut().as_mut_ptr().cast::<std::os::raw::c_void>(),
            RETRO_MEMORY_VIDEO_RAM => nes.vram_mut().as_mut_ptr().cast::<std::os::raw::c_void>(),
            _ => std::ptr::null_mut(),
        }
    }

    fn get_memory_size(
        &mut self,
        id: std::os::raw::c_uint,
        _ctx: &mut GetMemorySizeContext,
    ) -> usize {
        // Mirror `get_memory_data`: the MAIN console in dual mode.
        let nes = match (self.nes.as_ref(), self.dual.as_ref()) {
            (Some(nes), _) => nes,
            (None, Some(dual)) => dual.main(),
            (None, None) => return 0,
        };
        match id {
            RETRO_MEMORY_SAVE_RAM => nes.sram().len(),
            RETRO_MEMORY_SYSTEM_RAM => nes.wram().len(),
            RETRO_MEMORY_VIDEO_RAM => nes.vram().len(),
            _ => 0,
        }
    }

    fn get_serialize_size(&mut self, _ctx: &mut GetSerializeSizeContext) -> usize {
        self.serialize_size
    }

    fn on_serialize(&mut self, slice: &mut [u8], _ctx: &mut SerializeContext) -> bool {
        // Generates the deterministic binary blob representing the console hardware
        // state. Single console → `snapshot_core_into`; a Vs. DualSystem cabinet →
        // `VsDualSystem::snapshot` (a self-describing blob of BOTH consoles).
        if let Some(nes) = self.nes.as_ref() {
            self.serialize_buffer.clear();
            nes.snapshot_core_into(&mut self.serialize_buffer);
        } else if let Some(dual) = self.dual.as_ref() {
            self.serialize_buffer = dual.snapshot();
        } else {
            return false;
        }
        if slice.len() >= self.serialize_buffer.len() {
            slice[..self.serialize_buffer.len()].copy_from_slice(&self.serialize_buffer);
            return true;
        }
        false
    }

    fn on_unserialize(&mut self, slice: &mut [u8], _ctx: &mut UnserializeContext) -> bool {
        // Restores the cycle-accurate lockstep hardware state from the serialized blob.
        if let Some(nes) = self.nes.as_mut() {
            return nes.restore_quiet(slice).is_ok();
        }
        if let Some(dual) = self.dual.as_mut() {
            return dual.restore(slice).is_ok();
        }
        false
    }
}

retro_core!(RustyNesLibretro {
    nes: None,
    dual: None,
    audio_buffer: Vec::with_capacity(4096),
    audio_float_buffer: Vec::with_capacity(4096),
    video_buffer: Vec::with_capacity(DUAL_W * NES_H * 4),
    serialize_size: 0,
    serialize_buffer: Vec::new(),
});
