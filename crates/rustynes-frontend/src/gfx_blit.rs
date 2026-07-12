//! v2.1.8 "Performance" (A2) — a vectorized software palette-index -> RGBA
//! blitter, kept **byte-identical** to the emulation core's own pixel-emit path.
//!
//! # What this is (and, importantly, what it is NOT)
//!
//! RustyNES's on-screen frame path is GPU-resident: the `#![no_std]` core
//! (`Ppu::emit_pixel`) writes the 256x240 RGBA8 framebuffer via a precomputed
//! `(emphasis << 6) | colour` -> RGBA lookup table (`build_rgba_lut`), the
//! frontend uploads that RGBA straight to a wgpu texture, and every display
//! filter (the LMP88959 / Bisqwit
//! NTSC composite ladders, the CRT / scanline pass, the hqx / xBRZ upscalers)
//! runs as a WGSL **fragment shader** in [`crate::gfx`] +
//! `rustynes_gfx_shaders`. None of those transforms execute on the CPU on the
//! shipped frame path, so there is deliberately no CPU NTSC ladder here to
//! vectorize — that work lives on the GPU by design.
//!
//! What *does* have a well-defined CPU form is the palette-index -> RGBA
//! conversion itself. The PPU keeps a parallel palette-index framebuffer
//! (`Ppu::index_framebuffer`, one `u16` per pixel, each value
//! `(emphasis << 6) | colour` in `0..512`) written in lockstep with the RGBA
//! framebuffer, under the exact contract asserted by the core:
//!
//! ```text
//! framebuffer[px*4 .. px*4+4] == rgba_lut[index_framebuffer[px]]
//! ```
//!
//! This module reproduces that mapping on the CPU as a standalone, reusable,
//! and rigorously validated routine: given the palette-index buffer plus the
//! same 512-entry LUT the core uses, it reconstructs a byte-identical RGBA
//! frame. That makes it a genuine oracle-checkable transform (the unit tests
//! assert equality against `rgba_lut[idx]` for the exact `build_rgba_lut`
//! table the PPU emits with) and a ready building block for any host that has
//! the index frame but wants to (re)colour it off the GPU — e.g. re-blitting a
//! captured index frame under a swapped palette without re-running the core.
//!
//! # The honest SIMD story
//!
//! The conversion is a **table gather** (`out[i] = lut[idx[i]]`), not per-lane
//! arithmetic. Portable SIMD (`wide` on desktop, `core::arch::wasm32` v128 on
//! wasm) has no hardware gather on the stable target-feature baselines we ship,
//! so the *load* side stays scalar; SIMD only widens the **store** side (one
//! 32-byte `u32x8` write instead of eight 4-byte writes). The gather therefore
//! dominates and the operation is memory-bandwidth bound: the vectorized path
//! is byte-identical to and, per the Criterion bench in `benches/gfx_blit.rs`,
//! within noise of the tight scalar-`u32` path (both comfortably beat the naive
//! per-pixel `copy_from_slice` baseline). We keep the scalar path as the
//! reference and [`blit`] adopts the vectorized path only where it measures a
//! win; correctness never depends on which lane width runs.
//!
//! # Determinism / core contract
//!
//! This is a pure, side-effect-free display transform over data the core
//! already produced. It touches neither the emulation core nor the determinism
//! contract (same seed + ROM + input => bit-identical framebuffer + audio); the
//! SIMD and scalar paths produce identical bytes, guarded by the SIMD==scalar
//! test below.

// The docs are dense with graphics acronyms (GPU, RGBA, WGSL, LMP88959) and the
// mixed-case product name RustyNES; backticking each would hurt readability, so
// take the same `doc_markdown` exemption the `rustynes-gfx-shaders` crate does.
#![allow(clippy::doc_markdown)]

// `wide::u32x8` powers the desktop `blit_simd` path only; on wasm32 that fn is
// `cfg`-compiled out (the wasm path uses `core::arch::wasm32` v128 intrinsics),
// so the import would be unused there.
#[cfg(not(target_arch = "wasm32"))]
use wide::u32x8;

/// NES visible framebuffer width in pixels.
pub const NES_W: usize = 256;
/// NES visible framebuffer height in pixels.
pub const NES_H: usize = 240;
/// Total visible pixels per frame (`NES_W * NES_H`).
pub const PIXELS: usize = NES_W * NES_H;
/// RGBA8 framebuffer length in bytes (`PIXELS * 4`).
pub const RGBA_LEN: usize = PIXELS * 4;

/// The 512-entry `(emphasis << 6) | colour` -> RGBA8 lookup table.
///
/// Exactly the shape the PPU emits with (`build_rgba_lut` /
/// `build_rgba_lut_from_base`, both re-exported from `rustynes_core::rustynes_ppu`).
pub type RgbaLut = [[u8; 4]; 512];

/// Pack the byte-quad LUT into a native-endian `u32` view once per call.
///
/// Reading each `[r, g, b, a]` with [`u32::from_ne_bytes`] and later storing it
/// back with the same native endianness round-trips the four bytes unchanged on
/// the same machine, so the packed form is byte-identical to the `[u8; 4]`
/// source — it just lets the hot loop move a pixel with one 32-bit store
/// instead of a four-byte slice copy. 512 entries = 2 KiB, trivial against the
/// 240 KiB output it feeds.
#[inline]
fn pack_lut_u32(lut: &RgbaLut) -> [u32; 512] {
    let mut out = [0u32; 512];
    for (dst, src) in out.iter_mut().zip(lut.iter()) {
        *dst = u32::from_ne_bytes(*src);
    }
    out
}

/// Scalar **reference** blitter: `out[px*4 .. px*4+4] = lut[idx[px] & 0x1FF]`.
///
/// This mirrors `Ppu::emit_pixel` one-for-one (a bounds-checked
/// four-byte slice copy per pixel) and is the definition every other path is
/// validated against. The `& 0x1FF` mask is defensive: `index_framebuffer`
/// values are always `< 512` by construction, so it is a no-op for valid input
/// while making a malformed index safe (parse-don't-panic at the boundary)
/// rather than an out-of-bounds panic.
///
/// # Panics
/// Panics if `indices.len() < PIXELS` or `out.len() < RGBA_LEN`.
pub fn blit_scalar(indices: &[u16], lut: &RgbaLut, out: &mut [u8]) {
    assert!(indices.len() >= PIXELS, "index framebuffer too short");
    assert!(out.len() >= RGBA_LEN, "rgba output too short");
    for (px, &idx) in indices[..PIXELS].iter().enumerate() {
        let rgba = lut[(idx & 0x1FF) as usize];
        let off = px * 4;
        out[off..off + 4].copy_from_slice(&rgba);
    }
}

/// Tight scalar-`u32` blitter — the low end of the "adopt only if it wins" ladder.
///
/// Identical output to [`blit_scalar`], but gathers the packed `u32` LUT entry
/// and writes it as a single 32-bit store, which the reference four-byte
/// `copy_from_slice` cannot always be lowered to.
///
/// # Panics
/// Panics if `indices.len() < PIXELS` or `out.len() < RGBA_LEN`.
pub fn blit_u32(indices: &[u16], lut: &RgbaLut, out: &mut [u8]) {
    assert!(indices.len() >= PIXELS, "index framebuffer too short");
    assert!(out.len() >= RGBA_LEN, "rgba output too short");
    let lut32 = pack_lut_u32(lut);
    for (px, &idx) in indices[..PIXELS].iter().enumerate() {
        let rgba = lut32[(idx & 0x1FF) as usize];
        let off = px * 4;
        out[off..off + 4].copy_from_slice(&rgba.to_ne_bytes());
    }
}

/// Desktop portable-SIMD blitter (`wide::u32x8`).
///
/// Scalar 8-wide gather from the packed LUT, then one 32-byte vector store;
/// byte-identical to [`blit_scalar`]. The tail (`PIXELS % 8`, which is 0 here
/// since `PIXELS = 61 440` is a multiple of 8, but handled for generality)
/// falls back to scalar `u32` stores.
///
/// # Panics
/// Panics if `indices.len() < PIXELS` or `out.len() < RGBA_LEN`.
#[cfg(not(target_arch = "wasm32"))]
pub fn blit_simd(indices: &[u16], lut: &RgbaLut, out: &mut [u8]) {
    assert!(indices.len() >= PIXELS, "index framebuffer too short");
    assert!(out.len() >= RGBA_LEN, "rgba output too short");
    let lut32 = pack_lut_u32(lut);
    let idx = &indices[..PIXELS];
    let chunks = PIXELS / 8;
    for c in 0..chunks {
        let base = c * 8;
        // Scalar gather (no portable hardware gather on the stable baseline).
        let lanes = [
            lut32[(idx[base] & 0x1FF) as usize],
            lut32[(idx[base + 1] & 0x1FF) as usize],
            lut32[(idx[base + 2] & 0x1FF) as usize],
            lut32[(idx[base + 3] & 0x1FF) as usize],
            lut32[(idx[base + 4] & 0x1FF) as usize],
            lut32[(idx[base + 5] & 0x1FF) as usize],
            lut32[(idx[base + 6] & 0x1FF) as usize],
            lut32[(idx[base + 7] & 0x1FF) as usize],
        ];
        // One 256-bit store of the eight packed RGBA pixels.
        let v = u32x8::new(lanes);
        let off = base * 4;
        out[off..off + 32].copy_from_slice(bytemuck::cast_slice(&v.to_array()));
    }
    for px in (chunks * 8)..PIXELS {
        let rgba = lut32[(idx[px] & 0x1FF) as usize];
        let off = px * 4;
        out[off..off + 4].copy_from_slice(&rgba.to_ne_bytes());
    }
}

/// WebAssembly SIMD blitter (`core::arch::wasm32` `v128`), compiled only under
/// the `+simd128` target feature.
///
/// Scalar 4-wide gather from the packed LUT, then one 16-byte `v128` store;
/// byte-identical to [`blit_scalar`]. Non-`simd128` wasm builds fall back to
/// [`blit_u32`] through [`blit`].
///
/// # Panics
/// Panics if `indices.len() < PIXELS` or `out.len() < RGBA_LEN`.
#[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
#[allow(unsafe_code)] // one `v128_store`; justified by the SAFETY block below.
pub fn blit_simd_wasm(indices: &[u16], lut: &RgbaLut, out: &mut [u8]) {
    use core::arch::wasm32::{u32x4, v128_store};

    assert!(indices.len() >= PIXELS, "index framebuffer too short");
    assert!(out.len() >= RGBA_LEN, "rgba output too short");
    let lut32 = pack_lut_u32(lut);
    let idx = &indices[..PIXELS];
    let chunks = PIXELS / 4;
    for c in 0..chunks {
        let base = c * 4;
        let v = u32x4(
            lut32[(idx[base] & 0x1FF) as usize],
            lut32[(idx[base + 1] & 0x1FF) as usize],
            lut32[(idx[base + 2] & 0x1FF) as usize],
            lut32[(idx[base + 3] & 0x1FF) as usize],
        );
        let off = base * 4;
        // SAFETY: `off + 16 <= RGBA_LEN` because `base = c*4 <= PIXELS-4` so
        // `off <= (PIXELS-4)*4 = RGBA_LEN - 16`; the store writes exactly the
        // 16 bytes `out[off..off+16]`, which the length assertion above proved
        // are in bounds and which is a `[u8]` (1-byte aligned, as `v128_store`
        // requires — it performs an unaligned store).
        unsafe {
            v128_store(out.as_mut_ptr().add(off).cast(), v);
        }
    }
    for px in (chunks * 4)..PIXELS {
        let rgba = lut32[(idx[px] & 0x1FF) as usize];
        let off = px * 4;
        out[off..off + 4].copy_from_slice(&rgba.to_ne_bytes());
    }
}

/// Convert a palette-index framebuffer to RGBA8 (the recommended entry point).
///
/// Dispatches to [`blit_u32`] on every target. The output is byte-identical to
/// [`blit_scalar`] (hence to the core's own `Ppu::framebuffer`).
///
/// **Why not the SIMD paths here?** The conversion is a memory-bound LUT gather,
/// so the Criterion bench (`benches/gfx_blit.rs`, see `docs/performance.md`)
/// measures the `wide::u32x8` path *within noise of — in fact marginally slower
/// than* — the scalar-`u32` path: all three variants land at ~12 µs / ~19 GiB/s
/// for a full 256x240 frame. Per RustyNES's "adopt only on a measured > 3% win"
/// discipline, the default hot path stays scalar-`u32`. The SIMD variants
/// ([`blit_simd`] / `blit_simd_wasm`) remain byte-identical, validated, and
/// directly callable for a host that wants them; they are not dead — the
/// SIMD==scalar tests and the bench exercise them.
///
/// # Panics
/// Panics if `indices.len() < PIXELS` or `out.len() < RGBA_LEN`.
#[inline]
pub fn blit(indices: &[u16], lut: &RgbaLut, out: &mut [u8]) {
    blit_u32(indices, lut, out);
}

#[cfg(test)]
mod tests {
    use super::*;
    use rustynes_core::rustynes_ppu::{PpuPalette, build_rgba_lut};

    /// A representative frame corpus of palette indices: every one of the 512
    /// `(emphasis << 6) | colour` values in order (tiling the frame), plus a
    /// deterministic pseudo-random overlay, so the tests exercise the whole LUT
    /// domain and unaligned access patterns without any RNG nondeterminism.
    #[allow(clippy::cast_possible_truncation)] // both operands are `% 512`, so < 512.
    fn corpus() -> Vec<u16> {
        let mut v = vec![0u16; PIXELS];
        // Deterministic xorshift so the corpus is stable across runs/platforms.
        let mut s: u32 = 0x9E37_79B9;
        for (i, slot) in v.iter_mut().enumerate() {
            if i % 3 == 0 {
                // Sweep the full 0..512 domain.
                *slot = (i % 512) as u16;
            } else {
                s ^= s << 13;
                s ^= s >> 17;
                s ^= s << 5;
                *slot = (s % 512) as u16;
            }
        }
        v
    }

    /// The scalar reference reproduces the exact core contract
    /// `out[px] == rgba_lut[idx[px]]` for the table the PPU emits with.
    #[test]
    fn scalar_matches_core_lut_contract() {
        let lut = build_rgba_lut(PpuPalette::Composite2C02);
        let idx = corpus();
        let mut out = vec![0u8; RGBA_LEN];
        blit_scalar(&idx, &lut, &mut out);
        for (px, &i) in idx.iter().enumerate() {
            let off = px * 4;
            assert_eq!(
                &out[off..off + 4],
                &lut[(i & 0x1FF) as usize],
                "pixel {px}: blit output must equal rgba_lut[index], the exact \
                 core emit-pixel contract"
            );
        }
    }

    /// SIMD == scalar, byte-for-byte, over the representative corpus — the
    /// load-bearing guarantee that the vectorized path never diverges from the
    /// reference. Run for the default composite palette AND an RGB (2C05) table
    /// so both LUT shapes are covered.
    #[test]
    fn simd_equals_scalar_byte_identical() {
        let idx = corpus();
        for palette in [PpuPalette::Composite2C02, PpuPalette::Rgb2C05] {
            let lut = build_rgba_lut(palette);
            let mut a = vec![0u8; RGBA_LEN];
            let mut b = vec![0u8; RGBA_LEN];
            let mut c = vec![0u8; RGBA_LEN];
            blit_scalar(&idx, &lut, &mut a);
            blit_u32(&idx, &lut, &mut b);
            blit(&idx, &lut, &mut c);
            assert_eq!(
                a, b,
                "u32 path must be byte-identical to scalar ({palette:?})"
            );
            assert_eq!(
                a, c,
                "dispatched path must be byte-identical to scalar ({palette:?})"
            );

            // Validate the target's portable-SIMD path directly (the dispatcher
            // deliberately routes through `blit_u32`, so assert the SIMD variant
            // here explicitly — it stays a byte-identical, callable alternative).
            #[cfg(not(target_arch = "wasm32"))]
            {
                let mut d = vec![0u8; RGBA_LEN];
                blit_simd(&idx, &lut, &mut d);
                assert_eq!(
                    a, d,
                    "wide::u32x8 SIMD must be byte-identical to scalar ({palette:?})"
                );
            }
            #[cfg(all(target_arch = "wasm32", target_feature = "simd128"))]
            {
                let mut d = vec![0u8; RGBA_LEN];
                blit_simd_wasm(&idx, &lut, &mut d);
                assert_eq!(
                    a, d,
                    "wasm v128 SIMD must be byte-identical to scalar ({palette:?})"
                );
            }
        }
    }

    /// The dispatcher and the tight scalar path agree with the reference on an
    /// all-zero (backdrop) frame — the common menu / vblank case.
    #[test]
    fn zero_frame_is_backdrop() {
        let lut = build_rgba_lut(PpuPalette::Composite2C02);
        let idx = vec![0u16; PIXELS];
        let mut out = vec![0xABu8; RGBA_LEN];
        blit(&idx, &lut, &mut out);
        let backdrop = lut[0];
        for off in (0..RGBA_LEN).step_by(4) {
            assert_eq!(&out[off..off + 4], &backdrop);
        }
    }
}
