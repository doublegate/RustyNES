/*
 * rustynes_ios.h — the hand-written C ABI for the RustyNES iOS host glue.
 *
 * This declares ONLY the hot glue the UniFFI-generated `NesController` cannot
 * express: the Metal surface lifecycle (Workstream B) and the CoreAudio sink
 * (Workstream C). The typed emulator control surface (load ROM, set input, run
 * frame, save state, movies, HD-pack, RA, netplay) is the generated Swift
 * `NesController` from `rustynes-mobile` — drive the core through that.
 *
 * The implementation lives in `crates/rustynes-ios/src/ffi.rs`. Handles are
 * opaque: an `*_init`/`*_new` returns a pointer (or NULL on failure), every
 * other call takes that pointer, and `*_destroy` frees it. The Swift side must
 * null its stored pointer immediately after a `*_destroy`.
 *
 * Included by the app's `RustyNES-Bridging-Header.h` and packaged into the
 * `RustyNESFFI.xcframework` headers by `scripts/build-ios-xcframework.sh`.
 */

#ifndef RUSTYNES_IOS_H
#define RUSTYNES_IOS_H

#include <stdint.h>
#include <stddef.h>

#ifdef __cplusplus
extern "C" {
#endif

/* Opaque handles. */
typedef struct RustyNesMetalGfx RustyNesMetalGfx;
typedef struct RustyNesAudioSink RustyNesAudioSink;

/* ---- Graphics (Workstream B) ---- */

/* Build the wgpu->Metal renderer for an MTKView (UIView*) at the drawable size.
 * Returns NULL on failure. `view` must outlive the renderer. */
RustyNesMetalGfx *rustynes_ios_gfx_init(void *view, uint32_t width, uint32_t height);

/* Reconfigure for a new drawable size (scene resize / rotation / Stage Manager). */
void rustynes_ios_gfx_resize(RustyNesMetalGfx *handle, uint32_t width, uint32_t height);

/* Upload + present one 256x240 RGBA frame (NesController.run_frame()'s buffer).
 * A length mismatch drops the frame. */
void rustynes_ios_gfx_render(RustyNesMetalGfx *handle, const uint8_t *fb, size_t len);

/* Upload + present one HD-pack composited RGBA frame at w*h (v1.9.5;
 * NesController.compositeHdFrame(), sized by hdpackDimensions()). w*h may exceed
 * 256x240 (up to a 10x upscale). A length mismatch (len != w*h*4) drops it. */
void rustynes_ios_gfx_render_hd(RustyNesMetalGfx *handle, const uint8_t *fb,
                                size_t len, uint32_t w, uint32_t h);

/* Set the video filter (0 none, 1 scanlines, 2 CRT, 3 NTSC, 4 Bisqwit) + params. */
void rustynes_ios_gfx_set_filter(RustyNesMetalGfx *handle, uint8_t filter,
                                 float p0, float p1, float p2, float p3);

/* Upload the palette-index frame (256*240*2 LE u16 bytes) + NTSC phase (Bisqwit). */
void rustynes_ios_gfx_set_index_frame(RustyNesMetalGfx *handle, const uint8_t *idx,
                                      size_t len, uint8_t phase);

/* Drop the renderer (releases the wgpu surface before the host releases the view). */
void rustynes_ios_gfx_destroy(RustyNesMetalGfx *handle);

/* ---- Audio (Workstream C) ---- */

/* Open the CoreAudio output sink. Returns NULL on failure. AVAudioSession setup
 * is the Swift side's responsibility; this only owns the cpal output stream. */
RustyNesAudioSink *rustynes_ios_audio_new(void);

/* Enqueue mono f32 samples (NesController.drain_audio()). */
void rustynes_ios_audio_push(RustyNesAudioSink *handle, const float *samples, size_t len);

/* The negotiated device sample rate (request it from NesController::new). 0 if NULL. */
uint32_t rustynes_ios_audio_sample_rate(RustyNesAudioSink *handle);

/* Publish the live audio-depth DSP config (v1.9.9): EQ / pan / reverb / crossfeed,
 * applied host-side after drain_audio. `eq` is up to 5 band gains (dB), `pan` up to
 * 6 per-channel positions (-1..=1); extra entries ignored, missing ones neutral.
 * `enabled == 0` (or a flat / centered config) is a bit-exact passthrough. The
 * `eq` / `pan` pointers may be NULL (treated as neutral). No-op on a NULL handle. */
void rustynes_ios_audio_set_depth(RustyNesAudioSink *handle, uint8_t enabled,
                                  const float *eq, size_t eq_len,
                                  const float *pan, size_t pan_len,
                                  float reverb_mix, float reverb_room, float crossfeed);

/* Pause / resume the output stream (scene background / audio interruption). */
void rustynes_ios_audio_pause(RustyNesAudioSink *handle);
void rustynes_ios_audio_resume(RustyNesAudioSink *handle);

/* Stop + drop the sink. */
void rustynes_ios_audio_destroy(RustyNesAudioSink *handle);

#ifdef __cplusplus
}
#endif

#endif /* RUSTYNES_IOS_H */
