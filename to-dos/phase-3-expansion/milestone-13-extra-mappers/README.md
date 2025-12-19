# Milestone 13: HTPC Mode & Additional Mappers

**Phase:** 3 (Expansion)
**Duration:** Months 14-17 (4 months)
**Status:** Planned
**Target:** May 2027
**Prerequisites:** M6 MVP Complete, M11 CRT Shaders Complete

---

## Overview

Milestone 13 delivers two major features: **HTPC (Home Theater PC) Mode** with controller-first UI and **98% Mapper Coverage** (50 total mappers).

**HTPC Mode** transforms RustyNES into a living room-friendly experience with Cover Flow library browsing, Virtual Shelf visualization, controller-first navigation, and 10-foot UI design optimized for TV viewing.

**Mapper Expansion** ensures broad game compatibility with 50+ mapper implementations, focusing on common mappers and those required for popular games.

---

## Part 1: HTPC Mode

### Core HTPC Features

- [ ] **Cover Flow Library Browser**
  - 3D carousel of ROM box art
  - Smooth scrolling (60 FPS)
  - Auto-download box art (TheGamesDB API)
  - Fallback to title text (missing artwork)
  - Metadata overlay (publisher, year, genre)

- [ ] **Virtual Shelf Visualization**
  - 3D shelf with game spines
  - Cartridge extraction animation
  - Dust/wear effects (optional)
  - Customizable shelf themes (wood, metal, plastic)

- [ ] **Controller-First Navigation**
  - All UI navigable with D-pad
  - No mouse required
  - Consistent button mapping (A = Select, B = Back)
  - On-screen button hints

- [ ] **10-Foot UI Design**
  - Large fonts (readable from 10 feet)
  - High contrast themes
  - Simplified menus (max 5-7 items)
  - Full-screen overlay (settings, ROM browser)

- [ ] **Auto-Start Mode**
  - Launch directly to ROM library
  - Auto-resume last played game
  - Configurable startup behavior

- [ ] **Voice Control Integration (Optional)**
  - "Play Super Mario Bros"
  - "Next game"
  - "Save state"
  - Speech recognition (OS-level)

---

## Part 2: Additional Mappers (98% Coverage)

### Priority Mappers (Phase 3)

#### Common Mappers (20)

- [ ] **Mapper 5 (MMC5)** - ExROM
  - PRG banking (8K/16K/32K)
  - CHR banking (1K/2K/4K/8K)
  - Extended attributes
  - ExRAM
  - IRQ counter

- [ ] **Mapper 7 (AxROM)** - Battletoads
  - 32K PRG switching
  - One-screen mirroring

- [ ] **Mapper 9/10 (MMC2/4)** - Punch-Out!!
  - Latch-based CHR switching
  - 8K PRG switching

- [ ] **Mapper 11 (ColorDreams)**
  - Simple banking
  - No CHR-RAM

- [ ] **Mapper 19 (Namco 163)**
  - Expansion audio
  - IRQ counter
  - Complex banking

- [ ] **Mapper 23/25 (VRC2/4)**
  - 8K PRG banking
  - 1K CHR banking
  - IRQ counter

- [ ] **Mapper 24/26 (VRC6)**
  - Expansion audio (saw wave)
  - 16K PRG banking
  - IRQ counter

- [ ] **Mapper 69 (Sunsoft FME-7)**
  - Expansion audio (AY-3-8910)
  - 8K PRG banking
  - 1K CHR banking

- [ ] Mapper 13 (CPROM)
- [ ] Mapper 15 (100-in-1)
- [ ] Mapper 16 (Bandai FCG)
- [ ] Mapper 18 (Jaleco SS88006)
- [ ] Mapper 21/22 (VRC2/4 variants)
- [ ] Mapper 30 (UNROM 512)
- [ ] Mapper 34 (BNROM/NINA-001)
- [ ] Mapper 66 (GxROM)
- [ ] Mapper 71 (Camerica/Codemasters)
- [ ] Mapper 79 (NINA-03/06)
- [ ] Mapper 87 (Jaleco/Konami)
- [ ] Mapper 94 (UN1ROM)

#### Advanced Mappers (15)

- [ ] Mapper 28 (Action 53)
- [ ] Mapper 32 (Irem G-101)
- [ ] Mapper 33/48 (Taito TC0190)
- [ ] Mapper 37/47 (MMC3 multicart)
- [ ] Mapper 64/158 (Tengen RAMBO-1)
- [ ] Mapper 65 (Irem H-3001)
- [ ] Mapper 67 (Sunsoft-3)
- [ ] Mapper 68 (Sunsoft-4)
- [ ] Mapper 70 (Bandai)
- [ ] Mapper 73 (VRC3)
- [ ] Mapper 75 (VRC1)
- [ ] Mapper 76 (Namco 109)
- [ ] Mapper 80/207 (Taito X1-005)
- [ ] Mapper 82/88 (Taito X1-017)
- [ ] Mapper 85 (VRC7) - FM synthesis

#### Multicart/Bootleg Mappers (15)

- [ ] Mapper 42 (Mario Party bootleg)
- [ ] Mapper 46 (Rumblestation 15-in-1)
- [ ] Mapper 54 (Novel Diamond)
- [ ] Mapper 57/58/59 (Multicarts)
- [ ] Mapper 114 (Sugar Softec)
- [ ] Mapper 118 (TxSROM)
- [ ] Mapper 119 (TQROM)
- [ ] Mapper 154 (Namco 118/340)
- [ ] Mapper 180 (Crazy Climber)
- [ ] Mapper 185 (CNROM with bus conflicts)
- [ ] Mapper 200-218 (Various multicarts)
- [ ] Mapper 225-235 (Multicarts)

---

## HTPC Mode Architecture

### Cover Flow Implementation

```
┌───────────────────────────────────────────────┐
│  Cover Flow View (3D Carousel)                │
│                                               │
│         [Box Art 1]                           │
│      [Box Art 2]   [Box Art 3]                │
│   [Box Art 4] [Box Art 5 (selected)]          │
│      [Box Art 6]   [Box Art 7]                │
│         [Box Art 8]                           │
│                                               │
│  Title: Super Mario Bros                      │
│  Publisher: Nintendo (1985)                   │
│  Genre: Platformer                            │
│                                               │
│  [A] Play  [Y] Info  [X] Favorite  [B] Back   │
└───────────────────────────────────────────────┘
```

### Virtual Shelf Implementation

```
┌──────────────────────────────────────────────┐
│  Virtual Shelf View (3D Shelf)               │
│                                              │
│  ┌───────────────────────────────────────┐   │
│  │ [Spine 1][Spine 2][Spine 3][Spine 4]  │   │
│  │ [Spine 5][Spine 6][Spine 7][Spine 8]  │   │
│  │ [Spine 9][Spine 10][Spine 11][Spin..] │   │
│  └───────────────────────────────────────┘   │
│                                              │
│  Selected: Super Mario Bros 3                │
│                                              │
│  [A] Pull Cartridge  [Y] Change Shelf        │
└──────────────────────────────────────────────┘
```

### wgpu 3D Rendering (Cover Flow)

**File:** `crates/rustynes-desktop/src/htpc/coverflow.rs`

```rust
use wgpu::util::DeviceExt;

pub struct CoverFlowRenderer {
    pipeline: wgpu::RenderPipeline,
    vertex_buffer: wgpu::Buffer,
    index_buffer: wgpu::Buffer,
    uniform_buffer: wgpu::Buffer,
    bind_group: wgpu::BindGroup,

    /// Box art textures
    textures: Vec<wgpu::Texture>,

    /// Current selection
    selected_index: usize,

    /// Carousel scroll position (0.0-1.0)
    scroll_position: f32,
}

impl CoverFlowRenderer {
    pub fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        format: wgpu::TextureFormat,
    ) -> Self {
        // Create 3D carousel pipeline (WGSL shader)
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("CoverFlow Shader"),
            source: wgpu::ShaderSource::Wgsl(include_str!("shaders/coverflow.wgsl").into()),
        });

        // ... pipeline setup ...

        Self {
            pipeline,
            vertex_buffer,
            index_buffer,
            uniform_buffer,
            bind_group,
            textures: Vec::new(),
            selected_index: 0,
            scroll_position: 0.0,
        }
    }

    pub fn update(&mut self, delta_time: f32, input: &ControllerInput) {
        // Scroll carousel with D-pad
        if input.dpad_left {
            self.scroll_position -= 2.0 * delta_time;
        } else if input.dpad_right {
            self.scroll_position += 2.0 * delta_time;
        }

        // Clamp scroll position
        self.scroll_position = self.scroll_position.clamp(
            0.0,
            (self.textures.len() - 1) as f32,
        );

        // Smooth interpolation
        self.selected_index = self.scroll_position.round() as usize;
    }

    pub fn render(&self, encoder: &mut wgpu::CommandEncoder, view: &wgpu::TextureView) {
        let mut render_pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("CoverFlow Pass"),
            color_attachments: &[Some(wgpu::RenderPassColorAttachment {
                view,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::BLACK),
                    store: wgpu::StoreOp::Store,
                },
            })],
            depth_stencil_attachment: None,
            timestamp_writes: None,
            occlusion_query_set: None,
        });

        render_pass.set_pipeline(&self.pipeline);
        render_pass.set_bind_group(0, &self.bind_group, &[]);
        render_pass.set_vertex_buffer(0, self.vertex_buffer.slice(..));
        render_pass.set_index_buffer(self.index_buffer.slice(..), wgpu::IndexFormat::Uint16);

        // Draw each box art with 3D transformation
        for (i, texture) in self.textures.iter().enumerate() {
            let offset = (i as f32) - self.scroll_position;

            // Calculate 3D position (carousel)
            let angle = offset * std::f32::consts::PI / 6.0;
            let radius = 3.0;
            let x = angle.sin() * radius;
            let z = -angle.cos() * radius;

            // Scale selected item larger
            let scale = if i == self.selected_index { 1.2 } else { 1.0 };

            // Update uniform buffer with transformation matrix
            // ... upload matrix to GPU ...

            render_pass.draw_indexed(0..6, 0, 0..1);
        }
    }

    pub fn load_box_art(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, path: &Path) {
        // Load box art from file or download from TheGamesDB
        // ... texture loading ...
    }
}
```

### Box Art Auto-Download

**File:** `crates/rustynes-desktop/src/htpc/boxart.rs`

```rust
use reqwest::blocking::Client;
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct GamesDBResponse {
    data: GamesDBData,
}

#[derive(Debug, Deserialize)]
struct GamesDBData {
    games: Vec<GamesDBGame>,
}

#[derive(Debug, Deserialize)]
struct GamesDBGame {
    id: u32,
    game_title: String,
    boxart: Vec<GamesDBBoxArt>,
}

#[derive(Debug, Deserialize)]
struct GamesDBBoxArt {
    url: String,
    side: String,  // "front", "back", "spine"
}

pub struct BoxArtManager {
    client: Client,
    api_key: String,
    cache_dir: PathBuf,
}

impl BoxArtManager {
    pub fn new(api_key: String, cache_dir: PathBuf) -> Self {
        Self {
            client: Client::new(),
            api_key,
            cache_dir,
        }
    }

    pub fn fetch_box_art(&self, game_title: &str) -> Result<PathBuf, BoxArtError> {
        // Check cache first
        let cache_path = self.cache_dir.join(format!("{}.png", game_title));
        if cache_path.exists() {
            return Ok(cache_path);
        }

        // Search TheGamesDB
        let url = format!(
            "https://api.thegamesdb.net/v1/Games/ByGameName?apikey={}&name={}",
            self.api_key, game_title
        );

        let response: GamesDBResponse = self.client.get(&url).send()?.json()?;

        if let Some(game) = response.data.games.first() {
            if let Some(boxart) = game.boxart.iter().find(|b| b.side == "front") {
                // Download box art
                let image_data = self.client.get(&boxart.url).send()?.bytes()?;

                // Save to cache
                std::fs::write(&cache_path, &image_data)?;

                return Ok(cache_path);
            }
        }

        Err(BoxArtError::NotFound)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum BoxArtError {
    #[error("Box art not found")]
    NotFound,
    #[error("Network error: {0}")]
    Network(#[from] reqwest::Error),
    #[error("IO error: {0}")]
    Io(#[from] std::io::Error),
}
```

---

## HTPC Mode Implementation Plan

### Sprint 1: Cover Flow Renderer

**Duration:** 2 weeks

- [ ] wgpu 3D carousel pipeline
- [ ] Box art texture loading
- [ ] Smooth scrolling (60 FPS)
- [ ] Controller input handling

### Sprint 2: Box Art Integration

**Duration:** 2 weeks

- [ ] TheGamesDB API integration
- [ ] Box art caching
- [ ] Fallback to title text
- [ ] Metadata display (publisher, year, genre)

### Sprint 3: Virtual Shelf

**Duration:** 2 weeks

- [ ] 3D shelf renderer
- [ ] Cartridge extraction animation
- [ ] Dust/wear effects
- [ ] Shelf customization

### Sprint 4: 10-Foot UI

**Duration:** 2 weeks

- [ ] Large font UI theme
- [ ] Controller-first navigation
- [ ] Full-screen overlays
- [ ] Auto-start mode

---

## Mapper Implementation Plan

### Sprint 5-8: Mapper Expansion

**Duration:** 8 weeks total (20 mappers, 2 weeks per 5 mappers)

- [ ] Sprint 5: MMC5, AxROM, MMC2/4, ColorDreams, Namco 163
- [ ] Sprint 6: VRC2/4, VRC6, FME-7, CPROM, 100-in-1
- [ ] Sprint 7: Bandai FCG, Jaleco SS88006, VRC variants, UNROM 512, BNROM
- [ ] Sprint 8: GxROM, Camerica, NINA variants, Action 53, Irem G-101

---

## Acceptance Criteria

### HTPC Mode

- [ ] Cover Flow renders at 60 FPS (3D carousel)
- [ ] Box art auto-downloads from TheGamesDB
- [ ] All UI navigable with controller (no mouse)
- [ ] 10-foot UI readable from 10 feet away
- [ ] Virtual Shelf renders correctly
- [ ] Auto-start mode works (launch to library)

### Mapper Coverage

- [ ] 50 total mappers implemented (98% coverage)
- [ ] All priority mappers functional (MMC5, VRC6, Namco 163, etc.)
- [ ] Mapper-specific test ROMs pass
- [ ] IRQ timing accurate (scanline counters)
- [ ] Expansion audio works (VRC6, FME-7, Namco 163)

---

## Dependencies

### Prerequisites

- **M6 MVP Complete:** Iced GUI, wgpu rendering, gilrs input
- **M11 CRT Shaders:** wgpu shader pipeline established

### Crate Dependencies

```toml
# crates/rustynes-desktop/Cargo.toml

[dependencies.reqwest]
version = "0.11"
features = ["blocking", "json"]  # Box art downloads

[dependencies.image]
version = "0.24"  # Box art decoding

[dependencies.serde]
version = "1.0"
features = ["derive"]  # TheGamesDB JSON parsing

[dependencies.thiserror]
version = "1.0"  # Error handling
```

---

## Related Documentation

- [M6-S3-input-library.md](../../phase-1-mvp/milestone-6-gui/M6-S3-input-library.md) - gilrs controller input
- [M11 CRT Shaders](../milestone-11-webassembly/README.md) - wgpu shader foundation
- [M12 Expansion Audio](../milestone-12-expansion-audio/README.md) - VRC6/FME-7/N163 audio

---

## Success Criteria

1. HTPC Mode functional (Cover Flow, Virtual Shelf, 10-foot UI)
2. Box art auto-downloads from TheGamesDB
3. Controller-first navigation (no mouse required)
4. 50 mappers implemented (98% coverage)
5. All priority mappers pass test ROMs
6. Expansion audio works (VRC6, FME-7, Namco 163)
7. IRQ timing accurate (scanline counters)
8. Zero regressions in existing mappers
9. M13 milestone marked as ✅ COMPLETE

---

**Milestone Status:** ⏳ PLANNED
**Blocked By:** M6 MVP Complete, M11 CRT Shaders Complete
**Next Milestone:** M14 (Plugin Architecture & Mobile Ports)

---

## Design Notes

### HTPC Mode Philosophy

**Living Room Experience:**
- Optimize for TV viewing (10-foot UI, large fonts)
- Controller-first (no mouse/keyboard required)
- Visual library browsing (Cover Flow, Virtual Shelf)
- Auto-start mode (power on → play games)

**Box Art Importance:**
- Visual recognition (easier than text menus)
- Nostalgic experience (authentic box art)
- Metadata context (publisher, year, genre)
- Fallback to generated thumbnails (missing artwork)

### Mapper Priority Rationale

**Common Mappers First:**
- MMC5 (Castlevania III, Laser Invasion)
- AxROM (Battletoads, Arch Rivals)
- MMC2/4 (Punch-Out!!, Fire Emblem)
- VRC6 (Castlevania III Japanese, Akumajou Densetsu)
- Namco 163 (Rolling Thunder, Megami Tensei II)

**Multicart/Bootleg Last:**
- Lower priority (pirate carts)
- Required for 98% coverage
- Complex, undocumented behavior

---

## Future Enhancements (Phase 4)

Advanced features deferred to Phase 4:

1. **Voice Control (M14):**
   - OS-level speech recognition
   - Natural language commands
   - Accessibility feature

2. **Custom Themes (M15):**
   - User-created shelf designs
   - 3D model imports
   - Community theme sharing

3. **Social Features (M14):**
   - Discord Rich Presence (already in Phase 2 M7)
   - Twitch integration
   - Leaderboards

---

**Migration Note:** HTPC Mode features (Cover Flow, Virtual Shelf, 10-foot UI) added from M6 reorganization. Mapper expansion retains original scope.
