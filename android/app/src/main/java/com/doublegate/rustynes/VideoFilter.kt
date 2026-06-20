package com.doublegate.rustynes

import android.graphics.RenderEffect
import android.graphics.RuntimeShader
import android.os.Build
import androidx.annotation.RequiresApi
import androidx.compose.ui.graphics.asComposeRenderEffect
import androidx.compose.ui.graphics.RenderEffect as ComposeRenderEffect

/**
 * Video post-processing filters (Workstream B/F), applied on the GPU via an AGSL
 * [RuntimeShader] over the Compose render path.
 *
 * This is the pragmatic, Compose-native shader path: it delivers GPU CRT/scanline
 * post-processing without a native wgpu `SurfaceView` rewrite. (The full
 * wgpu-on-`SurfaceView` renderer reusing the desktop WGSL NTSC/CRT/Bisqwit stack
 * remains the documented deeper-architecture option.) `RuntimeShader` is API 33+,
 * so the effect is skipped on older devices — the plain `Bitmap` blit is the
 * always-available fallback. The shader is post-processing only; it never touches
 * the emulated framebuffer/audio, so determinism is unaffected.
 */
enum class VideoFilter(val label: String) {
    None("None"),
    Scanlines("Scanlines"),
    Crt("CRT"),
    // v1.8.4: NTSC composite (LMP88959) — only on the native wgpu GPU renderer; the
    // AGSL/Bitmap path shows it unfiltered (no AGSL NTSC). Ordinals match the native
    // filter codes (0/1/2/3).
    Ntsc("NTSC"),
    // v1.8.5: Bisqwit composite NTSC — GPU-renderer only; needs the palette-index
    // frame fed each frame. Ordinal 4 matches the native code.
    Bisqwit("Bisqwit NTSC"),
    ;

    fun next(): VideoFilter = entries[(ordinal + 1) % entries.size]
}

// `content` receives the rasterised composable (the NES image); `size` is its
// pixel extent; `mode` selects scanlines (1) vs full CRT (2).
private const val AGSL_SOURCE = """
uniform shader content;
uniform float2 size;
uniform float mode;

half4 main(float2 coord) {
    half4 c = content.eval(coord);
    // ~240 soft scanlines mapped onto the output height.
    float ny = coord.y / size.y * 240.0;
    float scan = 0.78 + 0.22 * abs(sin(ny * 3.14159265));
    half3 rgb = c.rgb * scan;
    if (mode > 1.5) {
        // Subtle RGB aperture mask on a 3-pixel pitch.
        float m = mod(coord.x, 3.0);
        half3 mask = (m < 1.0) ? half3(1.06, 0.96, 0.96)
                   : (m < 2.0) ? half3(0.96, 1.06, 0.96)
                               : half3(0.96, 0.96, 1.06);
        rgb = rgb * mask;
        // Gentle vignette toward the edges.
        float2 uv = coord / size;
        float v = smoothstep(0.0, 0.35, uv.x) * smoothstep(0.0, 0.35, 1.0 - uv.x)
                * smoothstep(0.0, 0.35, uv.y) * smoothstep(0.0, 0.35, 1.0 - uv.y);
        rgb = rgb * (0.88 + 0.12 * v);
    }
    return half4(rgb, c.a);
}
"""

/**
 * Build the Compose [ComposeRenderEffect] for [filter] at the given pixel size,
 * or null for [VideoFilter.None]. Caller must guard on API 33+.
 */
@RequiresApi(Build.VERSION_CODES.TIRAMISU)
fun buildRenderEffect(filter: VideoFilter, width: Float, height: Float): ComposeRenderEffect? {
    // None has no effect; NTSC + Bisqwit are native-wgpu-renderer only (no AGSL).
    if (filter == VideoFilter.None ||
        filter == VideoFilter.Ntsc ||
        filter == VideoFilter.Bisqwit ||
        width <= 0f ||
        height <= 0f
    ) {
        return null
    }
    val shader = RuntimeShader(AGSL_SOURCE)
    shader.setFloatUniform("size", width, height)
    shader.setFloatUniform("mode", if (filter == VideoFilter.Crt) 2f else 1f)
    return RenderEffect.createRuntimeShaderEffect(shader, "content").asComposeRenderEffect()
}

/** Whether GPU video filters are available on this device (API 33+). */
val videoFiltersSupported: Boolean
    get() = Build.VERSION.SDK_INT >= Build.VERSION_CODES.TIRAMISU
