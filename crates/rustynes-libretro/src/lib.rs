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
//! - **Audio**: Audio is drained per frame and interleaved (left/right) into a pooled
//!   buffer before pushing via `batch_audio_samples`. The accumulator relies on a
//!   pre-allocated array (or sufficient `Vec` capacity) to honor the hot-path allocation bans.
//! - **Input**: The Joypad API is polled each frame and bitmasked into `rustynes_core::Buttons`.
//! - **Save States & Memory Maps**: Direct pointers to WRAM, SRAM, and VRAM are provided
//!   safely by isolating the memory accessors in the core. Save states serialize statically
//!   sized binary blobs natively through `Nes::snapshot_core_into`.

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
use rustynes_core::Nes;
use std::ffi::CString;

/// The central libretro core structure for RustyNES.
///
/// This struct holds the underlying cycle-accurate `Nes` emulator instance alongside
/// the operational buffers necessary to interface with libretro's batch APIs. It is
/// statically instantiated via the `retro_core!` macro.
pub struct RustyNesLibretro {
    /// The cycle-accurate RustyNES core instance.
    ///
    /// Exists as an `Option` because `retro_init` fires before `retro_load_game`.
    nes: Option<Nes>,

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
            // 4096 samples comfortably holds ~85ms of audio at 48kHz,
            // well beyond the 16.6ms standard 60Hz frame delivery.
            audio_buffer: Vec::with_capacity(4096),
            audio_float_buffer: Vec::with_capacity(4096),
            video_buffer: Vec::with_capacity(256 * 240 * 4),
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
                max_width: 256,
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

        let nes = match Nes::from_rom(&rom_data) {
            Ok(n) => n,
            Err(e) => {
                eprintln!("[RustyNES] Failed to parse ROM: {e:?}");
                return Err(format!("Failed to load ROM: {e:?}").into());
            }
        };

        // Save state sizes in RustyNES are strictly deterministic for a given ROM image.
        // We evaluate the snapshot footprint once during initialization to satisfy
        // libretro's serialization size querying contract.
        let mut tmp = Vec::new();
        nes.snapshot_core_into(&mut tmp);
        self.serialize_size = tmp.len();

        self.nes = Some(nes);
        Ok(())
    }

    fn on_run(&mut self, ctx: &mut RunContext, _delta_us: Option<i64>) {
        if let Some(nes) = self.nes.as_mut() {
            ctx.poll_input();

            // Map the Libretro joypad state into the native RustyNES controller bitmasks.
            // Port 0 maps to Player 1, Port 1 maps to Player 2.
            let mut apply_input = |port: usize| {
                let jp = ctx.get_joypad_state(port as u32, 0);
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
                nes.set_buttons(port, bt);
            };

            apply_input(0);
            apply_input(1);

            // Advance the emulator clock by precisely one frame (PPU dots).
            // This is the core lockstep routine triggering CPU/APU progression.
            let framebuffer = nes.run_frame();

            self.video_buffer.clear();
            self.video_buffer.extend_from_slice(framebuffer);
            for chunk in self.video_buffer.chunks_exact_mut(4) {
                chunk.swap(0, 2); // Swap R and B to convert RGBA8 to XRGB8888 (in-memory B G R X)
            }

            // The generated framebuffer is exactly 256x240 and formatted as XRGB8888.
            ctx.draw_frame(&self.video_buffer, 256, 240, 256 * 4);

            // Drain synthesized audio. RustyNES produces `f32` floats which we scale
            // to the standard signed 16-bit integer expected by the frontend.
            // The audio buffers are pre-allocated; draining and converting will not
            // trigger heap allocations on this critical hot path.
            self.audio_buffer.clear();
            self.audio_float_buffer.resize(4096, 0.0);
            let produced = nes.drain_audio_into(&mut self.audio_float_buffer);
            for &sample in &self.audio_float_buffer[..produced] {
                // RustyNES APU outputs bipolar ~[-0.5, 0.5], so we scale by 65535.0
                let s16 = (sample * 65535.0).clamp(-32768.0, 32767.0) as i16;
                // Duplicate the sample for stereo interleaving (Left, Right)
                self.audio_buffer.push(s16);
                self.audio_buffer.push(s16);
            }

            rust_libretro::contexts::AudioContext::from(&mut *ctx)
                .batch_audio_samples(&self.audio_buffer);
        }
    }

    fn get_memory_data(
        &mut self,
        id: std::os::raw::c_uint,
        _ctx: &mut GetMemoryDataContext,
    ) -> *mut std::os::raw::c_void {
        // Expose zero-cost direct memory maps for RetroAchievements and cheat engines.
        // Memory boundary enforcement remains safe within the `rustynes_core` design.
        self.nes
            .as_mut()
            .map_or(std::ptr::null_mut(), |nes| match id {
                RETRO_MEMORY_SAVE_RAM => {
                    let sram = nes.sram_mut();
                    if sram.is_empty() {
                        std::ptr::null_mut()
                    } else {
                        sram.as_mut_ptr().cast::<std::os::raw::c_void>()
                    }
                }
                RETRO_MEMORY_SYSTEM_RAM => {
                    nes.wram_mut().as_mut_ptr().cast::<std::os::raw::c_void>()
                }
                RETRO_MEMORY_VIDEO_RAM => {
                    nes.vram_mut().as_mut_ptr().cast::<std::os::raw::c_void>()
                }
                _ => std::ptr::null_mut(),
            })
    }

    fn get_memory_size(
        &mut self,
        id: std::os::raw::c_uint,
        _ctx: &mut GetMemorySizeContext,
    ) -> usize {
        self.nes.as_ref().map_or(0, |nes| match id {
            RETRO_MEMORY_SAVE_RAM => nes.sram().len(),
            RETRO_MEMORY_SYSTEM_RAM => nes.wram().len(),
            RETRO_MEMORY_VIDEO_RAM => nes.vram().len(),
            _ => 0,
        })
    }

    fn get_serialize_size(&mut self, _ctx: &mut GetSerializeSizeContext) -> usize {
        self.serialize_size
    }

    fn on_serialize(&mut self, slice: &mut [u8], _ctx: &mut SerializeContext) -> bool {
        // Generates the deterministic binary blob representing the console hardware state.
        if let Some(nes) = self.nes.as_ref() {
            self.serialize_buffer.clear();
            nes.snapshot_core_into(&mut self.serialize_buffer);
            if slice.len() >= self.serialize_buffer.len() {
                slice[..self.serialize_buffer.len()].copy_from_slice(&self.serialize_buffer);
                return true;
            }
        }
        false
    }

    fn on_unserialize(&mut self, slice: &mut [u8], _ctx: &mut UnserializeContext) -> bool {
        // Restores the cycle-accurate lockstep hardware state from the serialized blob.
        if let Some(nes) = self.nes.as_mut() {
            return nes.restore_quiet(slice).is_ok();
        }
        false
    }
}

retro_core!(RustyNesLibretro {
    nes: None,
    audio_buffer: Vec::with_capacity(4096),
    audio_float_buffer: Vec::with_capacity(4096),
    video_buffer: Vec::with_capacity(256 * 240 * 4),
    serialize_size: 0,
    serialize_buffer: Vec::new(),
});
