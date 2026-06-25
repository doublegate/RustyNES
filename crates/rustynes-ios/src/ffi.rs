//! The C-ABI seam (v1.9.0 "Sunrise"). These `extern "C"` functions are the only
//! hand-written FFI the SwiftUI app calls directly — the *typed* control surface
//! (load ROM, set input, run frame, save state, …) is the UniFFI-generated
//! `NesController` from `rustynes-mobile`. This module is the iOS analogue of the
//! Android `jni_glue`: the Metal surface lifecycle and the audio sink, reached
//! over opaque `*mut` handles the Swift `RustyNES-Bridging-Header.h` declares.
//!
//! Handle discipline (mirrors the Android `jlong` boxed-pointer pattern):
//! `*_new`/`*_init` return `Box::into_raw` (or null on failure), every other call
//! null-checks and dereferences the live `Box`, and `*_destroy` reclaims it. The
//! Swift side nulls its stored pointer immediately after `*_destroy`.

use core::ffi::c_void;

use crate::audio::AudioSink;
use crate::gfx_metal::MetalGfx;

// ---- Graphics (Workstream B) ----------------------------------------------

/// `rustynes_ios_gfx_init(view, width, height) -> *mut MetalGfx` — build the wgpu
/// Metal renderer for the `MTKView` (`UIView`) pointer at the drawable size.
/// Returns null on failure.
///
/// # Safety
/// `view` must be a live `UIView` (`MTKView`) retained by the caller for the
/// renderer's whole lifetime (until `rustynes_ios_gfx_destroy`).
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_gfx_init(
    view: *mut c_void,
    width: u32,
    height: u32,
) -> *mut MetalGfx {
    match MetalGfx::new(view, width, height) {
        Ok(gfx) => Box::into_raw(Box::new(gfx)),
        Err(e) => {
            log::error!("rustynes_ios_gfx_init failed: {e}");
            core::ptr::null_mut()
        }
    }
}

/// `rustynes_ios_gfx_resize(handle, width, height)` — reconfigure for a new
/// drawable size (scene resize / Stage-Manager / rotation).
///
/// # Safety
/// `handle` must be a live value returned by `rustynes_ios_gfx_init`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_gfx_resize(handle: *mut MetalGfx, width: u32, height: u32) {
    if handle.is_null() {
        return;
    }
    // SAFETY: live handle between init and destroy (caller contract).
    let gfx = unsafe { &mut *handle };
    gfx.resize(width, height);
}

/// `rustynes_ios_gfx_render(handle, fb, len)` — upload + present one 256×240 RGBA
/// frame (`fb` is `NesController.run_frame()`'s buffer). A length mismatch drops
/// the frame (presentation-only, determinism untouched).
///
/// # Safety
/// `handle` must be live; `fb` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_gfx_render(handle: *mut MetalGfx, fb: *const u8, len: usize) {
    if handle.is_null() || fb.is_null() {
        return;
    }
    // SAFETY: live handle (see above).
    let gfx = unsafe { &mut *handle };
    let dst = gfx.frame_buf_mut();
    if len != dst.len() {
        return;
    }
    // SAFETY: `fb` points to `len` readable bytes (caller contract); `len`
    // equals `dst.len()`, so the copy stays in bounds on both sides.
    let src = unsafe { core::slice::from_raw_parts(fb, len) };
    dst.copy_from_slice(src);
    gfx.render();
}

/// `rustynes_ios_gfx_set_filter(handle, filter, p0..p3)` — 0 none / 1 scanlines /
/// 2 CRT / 3 NTSC / 4 Bisqwit, plus the filter-specific shader params.
///
/// # Safety
/// `handle` must be a live value returned by `rustynes_ios_gfx_init`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_gfx_set_filter(
    handle: *mut MetalGfx,
    filter: u8,
    p0: f32,
    p1: f32,
    p2: f32,
    p3: f32,
) {
    if handle.is_null() {
        return;
    }
    // SAFETY: live handle (see above).
    let gfx = unsafe { &mut *handle };
    gfx.set_filter(filter, [p0, p1, p2, p3]);
}

/// `rustynes_ios_gfx_set_index_frame(handle, idx, len, phase)` — upload the
/// palette-index frame (`256*240*2` LE `u16` bytes) + NTSC phase for the Bisqwit
/// pass. Only called while that filter is active.
///
/// # Safety
/// `handle` must be live; `idx` must point to `len` readable bytes.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_gfx_set_index_frame(
    handle: *mut MetalGfx,
    idx: *const u8,
    len: usize,
    phase: u8,
) {
    if handle.is_null() || idx.is_null() {
        return;
    }
    // SAFETY: live handle (see above).
    let gfx = unsafe { &mut *handle };
    // SAFETY: `idx` points to `len` readable bytes (caller contract).
    let bytes = unsafe { core::slice::from_raw_parts(idx, len) };
    gfx.set_index_frame(bytes, phase);
}

/// `rustynes_ios_gfx_destroy(handle)` — drop the renderer (releases the wgpu
/// surface before the host releases the `UIView`).
///
/// # Safety
/// `handle` must be a live value from `rustynes_ios_gfx_init`, not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_gfx_destroy(handle: *mut MetalGfx) {
    if handle.is_null() {
        return;
    }
    // SAFETY: reclaim the `Box` created in `rustynes_ios_gfx_init`; the Swift side
    // nulls its handle immediately after this call.
    drop(unsafe { Box::from_raw(handle) });
}

// ---- Audio (Workstream C) -------------------------------------------------

/// `rustynes_ios_audio_new() -> *mut AudioSink` — open the CoreAudio output sink.
/// Returns null on failure.
#[unsafe(no_mangle)]
pub extern "C" fn rustynes_ios_audio_new() -> *mut AudioSink {
    match AudioSink::new() {
        Ok(sink) => Box::into_raw(Box::new(sink)),
        Err(e) => {
            log::error!("rustynes_ios_audio_new failed: {e}");
            core::ptr::null_mut()
        }
    }
}

/// `rustynes_ios_audio_push(handle, samples, len)` — enqueue mono `f32` samples
/// (`NesController.drain_audio()`).
///
/// # Safety
/// `handle` must be live; `samples` must point to `len` readable `f32`s.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_audio_push(
    handle: *mut AudioSink,
    samples: *const f32,
    len: usize,
) {
    if handle.is_null() || samples.is_null() {
        return;
    }
    // SAFETY: live handle (caller contract).
    let sink = unsafe { &*handle };
    // SAFETY: `samples` points to `len` readable `f32`s (caller contract).
    let s = unsafe { core::slice::from_raw_parts(samples, len) };
    sink.push(s);
}

/// `rustynes_ios_audio_sample_rate(handle) -> u32` — the negotiated device rate
/// (request it from `NesController::new` so the core resamples to it). 0 if null.
///
/// # Safety
/// `handle` must be a live value returned by `rustynes_ios_audio_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_audio_sample_rate(handle: *mut AudioSink) -> u32 {
    if handle.is_null() {
        return 0;
    }
    // SAFETY: live handle (caller contract).
    let sink = unsafe { &*handle };
    sink.sample_rate()
}

/// `rustynes_ios_audio_pause(handle)` — pause output (scene background / audio
/// interruption begin).
///
/// # Safety
/// `handle` must be a live value returned by `rustynes_ios_audio_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_audio_pause(handle: *mut AudioSink) {
    if handle.is_null() {
        return;
    }
    // SAFETY: live handle (caller contract).
    let sink = unsafe { &*handle };
    sink.pause();
}

/// `rustynes_ios_audio_resume(handle)` — resume output (scene foreground /
/// interruption end).
///
/// # Safety
/// `handle` must be a live value returned by `rustynes_ios_audio_new`.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_audio_resume(handle: *mut AudioSink) {
    if handle.is_null() {
        return;
    }
    // SAFETY: live handle (caller contract).
    let sink = unsafe { &*handle };
    sink.resume();
}

/// `rustynes_ios_audio_destroy(handle)` — stop + drop the sink.
///
/// # Safety
/// `handle` must be a live value from `rustynes_ios_audio_new`, not used after.
#[unsafe(no_mangle)]
pub unsafe extern "C" fn rustynes_ios_audio_destroy(handle: *mut AudioSink) {
    if handle.is_null() {
        return;
    }
    // SAFETY: reclaim the `Box` created in `rustynes_ios_audio_new`.
    drop(unsafe { Box::from_raw(handle) });
}
