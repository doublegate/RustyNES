# Milestone 11: Advanced CRT Shaders & Filters

**Phase:** 3 (Expansion)
**Duration:** Months 16-17 (2 months)
**Status:** Planned
**Target:** May 2027
**Prerequisites:** M6 MVP Complete (wgpu rendering established)

---

## Overview

Milestone 11 implements comprehensive CRT shader system with **12+ presets**, phosphor persistence simulation, scanline rendering, aperture grille effects, and user-customizable shader parameters. This milestone transforms the raw pixel-perfect output into authentic CRT television reproduction with historically accurate phosphor color temperatures and decay curves.

**Philosophy:**
- Authentic vintage experience (NTSC composite artifacts, phosphor glow)
- Modern enhancements (4K upscaling, HDR, integer scaling)
- User control (adjustable parameters, custom presets)
- Performance (60 FPS at 4K on mid-range GPUs)

---

## Goals

### Core Shader Features

- [ ] **CRT Shader Presets (12+)**
  - CRT-Royale (ultra-realistic, performance-intensive)
  - CRT-Lottes (balanced quality/performance)
  - CRT-EasyMode (fast, subtle scanlines)
  - CRT-Geom (curved screen geometry)
  - Trinitron Aperture Grille (Sony trinitron simulation)
  - Shadow Mask (RGB triad shadow mask)
  - NTSC Composite (artifact simulation)
  - Scanline-Sharp (minimal, crisp scanlines)
  - Phosphor-Glow (decay persistence only)
  - RGB-Sharp (pixel-perfect reference)
  - Vintage-TV (overscan, noise, warmth)
  - LCD-Grid (handheld LCD simulation)

- [ ] **Phosphor Persistence Simulation**
  - Temporal blur (multiple past frames)
  - Exponential decay curves
  - Per-channel RGB decay rates
  - Customizable persistence time (0-100ms)

- [ ] **Scanline Rendering**
  - Horizontal scanlines (TV raster pattern)
  - Vertical mask simulation (RGB triads)
  - Adjustable intensity (0-100%)
  - Interlaced mode (60i vs 60p)

- [ ] **Screen Geometry**
  - Curved screen (barrel distortion)
  - Overscan simulation
  - Aspect ratio correction (8:7 to 4:3)
  - Corner rounding

- [ ] **Color Processing**
  - NTSC color temperature (6500K-9300K)
  - Phosphor color tint (amber, green, blue)
  - Gamma correction (2.2-2.8)
  - Saturation/brightness/contrast controls

- [ ] **Advanced Effects**
  - Bloom/glow (bright pixel bleeding)
  - Halation (light halo around bright areas)
  - Screen reflections (ambient light simulation)
  - Vignetting (screen edge darkening)
  - NTSC composite artifacts (dot crawl, rainbow banding)

- [ ] **User Customization**
  - Per-preset parameter tweaking
  - Custom preset creation
  - Save/load presets (TOML files)
  - Real-time parameter adjustment (sliders)

---

## Architecture

### Shader Pipeline (wgpu)

```
┌────────────────────────────────────────────────┐
│  NES Framebuffer (256×240 RGB)                 │
│  ↓                                             │
│  Integer Scaling (1x-10x)                      │
│  ↓                                             │
│  Aspect Ratio Correction (8:7 → 4:3)           │
│  ↓                                             │
│  CRT Shader Pipeline (multi-pass)              │
│  ├─ Pass 1: Phosphor Persistence               │
│  ├─ Pass 2: Scanlines + Mask                   │
│  ├─ Pass 3: Screen Geometry                    │
│  ├─ Pass 4: Bloom/Halation                     │
│  └─ Pass 5: Color Grading                      │
│  ↓                                             │
│  Final Composite (1920×1080 or 3840×2160)      │
└────────────────────────────────────────────────┘
```

### Shader Implementation (wgpu WGSL)

**File:** `crates/rustynes-desktop/shaders/crt_royale.wgsl`

```wgsl
// Pass 1: Phosphor Persistence
@fragment
fn phosphor_persistence(
    @location(0) tex_coord: vec2<f32>,
    @location(1) frame_history: texture_2d_array<f32>
) -> @location(0) vec4<f32> {
    var color = vec4<f32>(0.0);

    // Sample current frame
    let current = textureSample(frame_history, sampler0, tex_coord, 0);

    // Sample previous frames with exponential decay
    for (var i = 1; i < PERSISTENCE_FRAMES; i++) {
        let prev = textureSample(frame_history, sampler0, tex_coord, i);
        let decay = exp(-f32(i) * DECAY_RATE);
        color += prev * decay;
    }

    color += current;
    return color / (1.0 + PERSISTENCE_FRAMES);
}

// Pass 2: Scanlines + Aperture Grille
@fragment
fn scanlines_mask(
    @location(0) tex_coord: vec2<f32>,
    @builtin(position) frag_coord: vec4<f32>
) -> @location(0) vec4<f32> {
    let color = textureSample(input_texture, sampler0, tex_coord);

    // Horizontal scanlines
    let scanline = sin(frag_coord.y * SCANLINE_FREQUENCY) * SCANLINE_INTENSITY;

    // Aperture grille (trinitron vertical mask)
    let mask_offset = fmod(frag_coord.x, 3.0);
    var mask = vec3<f32>(1.0);
    if (mask_offset < 1.0) {
        mask = vec3<f32>(1.0, 0.7, 0.7);  // Red sub-pixel
    } else if (mask_offset < 2.0) {
        mask = vec3<f32>(0.7, 1.0, 0.7);  // Green sub-pixel
    } else {
        mask = vec3<f32>(0.7, 0.7, 1.0);  // Blue sub-pixel
    }

    return vec4<f32>(color.rgb * (1.0 - scanline) * mask, color.a);
}

// Pass 3: Screen Geometry (barrel distortion)
@fragment
fn screen_geometry(
    @location(0) tex_coord: vec2<f32>
) -> @location(0) vec4<f32> {
    // Center coordinates (-1 to 1)
    let centered = (tex_coord - 0.5) * 2.0;

    // Barrel distortion
    let r2 = centered.x * centered.x + centered.y * centered.y;
    let distortion = 1.0 + CURVATURE * r2;
    let distorted = centered / distortion;

    // Convert back to texture coordinates
    let final_coord = (distorted * 0.5) + 0.5;

    // Overscan (black borders)
    if (final_coord.x < OVERSCAN || final_coord.x > (1.0 - OVERSCAN) ||
        final_coord.y < OVERSCAN || final_coord.y > (1.0 - OVERSCAN)) {
        return vec4<f32>(0.0, 0.0, 0.0, 1.0);
    }

    return textureSample(input_texture, sampler0, final_coord);
}

// Pass 4: Bloom/Halation
@fragment
fn bloom_halation(
    @location(0) tex_coord: vec2<f32>
) -> @location(0) vec4<f32> {
    let color = textureSample(input_texture, sampler0, tex_coord);

    // Extract bright pixels
    let luminance = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
    var bloom = vec3<f32>(0.0);

    if (luminance > BLOOM_THRESHOLD) {
        // Gaussian blur for bloom
        for (var x = -BLOOM_RADIUS; x <= BLOOM_RADIUS; x++) {
            for (var y = -BLOOM_RADIUS; y <= BLOOM_RADIUS; y++) {
                let offset = vec2<f32>(f32(x), f32(y)) * BLOOM_SPREAD;
                let sample = textureSample(input_texture, sampler0, tex_coord + offset);
                bloom += sample.rgb * gaussian(x, y);
            }
        }
    }

    return vec4<f32>(color.rgb + bloom * BLOOM_INTENSITY, color.a);
}

// Pass 5: Color Grading
@fragment
fn color_grading(
    @location(0) tex_coord: vec2<f32>
) -> @location(0) vec4<f32> {
    var color = textureSample(input_texture, sampler0, tex_coord);

    // Gamma correction
    color.rgb = pow(color.rgb, vec3<f32>(1.0 / GAMMA));

    // Phosphor color temperature
    color.rgb *= COLOR_TEMPERATURE_MATRIX;

    // Saturation
    let luminance = dot(color.rgb, vec3<f32>(0.299, 0.587, 0.114));
    color.rgb = mix(vec3<f32>(luminance), color.rgb, SATURATION);

    // Brightness/Contrast
    color.rgb = (color.rgb - 0.5) * CONTRAST + 0.5 + BRIGHTNESS;

    return color;
}
```

---

## Shader Preset Configurations

### CRT-Royale (Ultra-Realistic)

```toml
# presets/crt-royale.toml

[preset]
name = "CRT-Royale"
description = "Ultra-realistic CRT with phosphor persistence, aperture grille, and bloom"
performance = "high"  # Requires GPU with 4GB+ VRAM

[phosphor]
persistence_frames = 3
decay_rate = 0.25
rgb_decay = [0.95, 0.97, 0.99]  # Red decays faster

[scanlines]
frequency = 2.0
intensity = 0.35
interlaced = false

[mask]
type = "aperture_grille"  # trinitron-style
strength = 0.5

[geometry]
curvature = 0.08
overscan = 0.05
corner_radius = 0.02

[color]
temperature = 6500  # K
gamma = 2.4
saturation = 1.05
brightness = 0.0
contrast = 1.1

[effects]
bloom_threshold = 0.7
bloom_radius = 8
bloom_intensity = 0.3
halation = 0.2
vignette = 0.15
```

### CRT-Lottes (Balanced)

```toml
# presets/crt-lottes.toml

[preset]
name = "CRT-Lottes"
description = "Balanced CRT shader with good quality and performance"
performance = "medium"

[phosphor]
persistence_frames = 2
decay_rate = 0.35
rgb_decay = [1.0, 1.0, 1.0]  # Equal decay

[scanlines]
frequency = 2.0
intensity = 0.25
interlaced = false

[mask]
type = "shadow_mask"  # RGB triad
strength = 0.3

[geometry]
curvature = 0.04
overscan = 0.03
corner_radius = 0.01

[color]
temperature = 6500
gamma = 2.2
saturation = 1.0
brightness = 0.0
contrast = 1.0

[effects]
bloom_threshold = 0.8
bloom_radius = 4
bloom_intensity = 0.15
halation = 0.1
vignette = 0.1
```

### Scanline-Sharp (Minimal)

```toml
# presets/scanline-sharp.toml

[preset]
name = "Scanline-Sharp"
description = "Minimal scanlines with sharp pixels"
performance = "low"  # Very fast

[phosphor]
persistence_frames = 0
decay_rate = 0.0
rgb_decay = [1.0, 1.0, 1.0]

[scanlines]
frequency = 2.0
intensity = 0.15
interlaced = false

[mask]
type = "none"
strength = 0.0

[geometry]
curvature = 0.0
overscan = 0.0
corner_radius = 0.0

[color]
temperature = 6500
gamma = 2.2
saturation = 1.0
brightness = 0.0
contrast = 1.0

[effects]
bloom_threshold = 1.0  # Disabled
bloom_radius = 0
bloom_intensity = 0.0
halation = 0.0
vignette = 0.0
```

---

## Implementation Plan

### Sprint 1: Shader Infrastructure

**Duration:** 2 weeks

- [ ] wgpu multi-pass render pipeline
- [ ] Frame history buffer (for phosphor persistence)
- [ ] Shader parameter system (uniform buffers)
- [ ] Real-time shader reloading (development)
- [ ] Performance profiling (GPU timers)

### Sprint 2: Core Shaders

**Duration:** 3 weeks

- [ ] Phosphor persistence shader
- [ ] Scanline shader
- [ ] Aperture grille/shadow mask shader
- [ ] Screen geometry shader
- [ ] Bloom/halation shader
- [ ] Color grading shader

### Sprint 3: Presets & UI

**Duration:** 2 weeks

- [ ] 12+ preset configurations (TOML)
- [ ] Preset selector in Settings UI
- [ ] Real-time parameter sliders
- [ ] Custom preset creation
- [ ] Preset import/export

### Sprint 4: NTSC Composite

**Duration:** 1 week

- [ ] NTSC composite artifact simulation
- [ ] Dot crawl effect
- [ ] Color bleeding
- [ ] Rainbow banding

---

## Acceptance Criteria

### Functionality

- [ ] 12+ CRT presets functional
- [ ] Phosphor persistence works (temporal blur)
- [ ] Scanlines render correctly
- [ ] Screen geometry (curvature) works
- [ ] Bloom/halation effects functional
- [ ] Real-time parameter adjustment

### Performance

- [ ] 60 FPS at 1080p (CRT-Royale on GTX 1660)
- [ ] 60 FPS at 4K (CRT-Lottes on RTX 3060)
- [ ] <2ms shader overhead (minimal presets)
- [ ] <8ms shader overhead (maximal presets)

### User Experience

- [ ] Easy preset switching (dropdown)
- [ ] Intuitive parameter sliders
- [ ] Live preview (no restart required)
- [ ] Custom preset creation workflow
- [ ] Clear performance warnings (if GPU insufficient)

---

## Dependencies

### Prerequisites

- **M6 MVP Complete:** wgpu rendering established
- **GPU Requirements:** Vulkan 1.1+ or Metal 2.0+

### Crate Dependencies

```toml
# crates/rustynes-desktop/Cargo.toml

[dependencies.wgpu]
version = "0.19"
features = ["spirv"]  # WGSL support

[dependencies.bytemuck]
version = "1.14"
features = ["derive"]  # Shader uniforms

[dependencies.toml]
version = "0.8"  # Preset configuration
```

---

## Related Documentation

- [M6-S2-wgpu-rendering.md](../../phase-1-mvp/milestone-6-gui/M6-S2-wgpu-rendering.md) - wgpu rendering foundation
- [M15 Advanced Shader Pipeline](../../phase-4-polish/milestone-15-video-filters/README.md) - Future shader enhancements
- wgpu Shader Tutorial: https://sotrh.github.io/learn-wgpu/

---

## Success Criteria

1. 12+ CRT shader presets implemented
2. Phosphor persistence simulates authentic CRT glow
3. Scanlines and aperture grille render accurately
4. Screen geometry (curvature) works without distortion artifacts
5. Bloom/halation effects enhance visual quality
6. 60 FPS at 1080p on mid-range GPUs (GTX 1660)
7. User-customizable presets (save/load TOML)
8. Real-time parameter adjustment (no restart)
9. NTSC composite artifacts optional (for authenticity)
10. M11 milestone marked as ✅ COMPLETE

---

**Milestone Status:** ⏳ PLANNED
**Blocked By:** M6 MVP Complete
**Next Milestone:** M12 (Expansion Audio - VRC6, MMC5, Namco 163, FDS)

---

## Design Notes

### Why CRT Shaders?

**Historical Accuracy:**
- NES games were designed for CRT televisions
- Dithering patterns relied on phosphor blending
- Scanlines were expected visual characteristic
- Many effects look "wrong" on pixel-perfect displays

**Visual Quality:**
- Phosphor glow softens harsh pixels
- Scanlines add depth and texture
- Bloom enhances bright highlights
- Curvature simulates vintage TVs

**User Choice:**
- Some users prefer pixel-perfect
- Others want authentic CRT experience
- Presets accommodate both preferences

### Performance Considerations

**Multi-Pass Rendering:**
- Each pass requires texture copy (GPU memory bandwidth)
- Minimize passes (combine where possible)
- Use lower-resolution intermediate textures (bloom)

**Phosphor Persistence:**
- Requires frame history buffer (3-5 frames)
- Memory: 256×240×3 bytes × 5 frames = ~900 KB (negligible)
- Bandwidth: Critical on integrated GPUs

**Target Hardware:**
- GTX 1660 (6GB VRAM) - CRT-Royale at 1080p 60 FPS
- RTX 3060 (12GB VRAM) - CRT-Royale at 4K 60 FPS
- Integrated GPUs - CRT-Lottes at 1080p 60 FPS

---

## Future Enhancements (Phase 4 M15)

Advanced features deferred to M15 (Advanced Shader Pipeline):

1. **Custom Shader Language (DSL):**
   - User-friendly shader creation
   - Visual node editor
   - Preset sharing community

2. **Advanced NTSC Simulation:**
   - Full NTSC composite video pipeline
   - RF modulation artifacts
   - TV tuner simulation

3. **HDR Support:**
   - Wide color gamut (Rec. 2020)
   - Peak brightness mapping (1000-4000 nits)
   - Authentic phosphor luminance

4. **AI Upscaling:**
   - Machine learning-based upscaling (waifu2x)
   - Detail enhancement
   - Anti-aliasing

---

## References

### CRT Shader Resources

- **LibRetro Slang Shaders:** https://github.com/libretro/slang-shaders
- **CRT-Royale Documentation:** https://github.com/libretro/slang-shaders/tree/master/crt/crt-royale
- **NTSC Composite Guide:** https://www.nesdev.org/wiki/NTSC_video

### wgpu Resources

- **wgpu Tutorial:** https://sotrh.github.io/learn-wgpu/
- **WGSL Reference:** https://www.w3.org/TR/WGSL/
- **wgpu Examples:** https://github.com/gfx-rs/wgpu/tree/trunk/examples

---

**Migration Note:** WebAssembly features originally planned for M11 have been moved to future milestones (browser deployment, PWA support). M11 now focuses exclusively on advanced CRT shader system.
