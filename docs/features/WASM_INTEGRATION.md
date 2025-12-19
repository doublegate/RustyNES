# RustyNES WebAssembly Integration

Complete guide for building and deploying RustyNES as a WebAssembly application for browser-based emulation.

## Overview

The WebAssembly (WASM) build enables RustyNES to run entirely in web browsers, providing cross-platform compatibility without installation. This document covers the complete architecture for the `rustynes-web` crate.

## Architecture

### Module Structure

```
crates/rustynes-web/
├── Cargo.toml
├── src/
│   ├── lib.rs              # WASM entry point, exported API
│   ├── emulator.rs         # Emulator wrapper for JS
│   ├── audio.rs            # Web Audio API integration
│   ├── video.rs            # Canvas/WebGL rendering
│   ├── input.rs            # Keyboard/gamepad handling
│   ├── storage.rs          # IndexedDB persistence
│   └── worker.rs           # Web Worker for audio
├── www/
│   ├── index.html          # Host page
│   ├── index.js            # JavaScript bootstrap
│   ├── style.css           # UI styles
│   └── worker.js           # Audio worker script
└── tests/
    └── web.rs              # WASM-specific tests
```

### Build Configuration

```toml
# Cargo.toml
[package]
name = "rustynes-web"
version = "0.1.0"
edition = "2021"
description = "WebAssembly build of RustyNES NES emulator"

[lib]
crate-type = ["cdylib", "rlib"]

[features]
default = ["console_error_panic_hook"]
# Enable WebGL rendering (vs Canvas 2D)
webgl = ["web-sys/WebGl2RenderingContext", "web-sys/WebGlProgram", "web-sys/WebGlShader"]

[dependencies]
rustynes-core = { path = "../rustynes-core", default-features = false }
rustynes-cpu = { path = "../rustynes-cpu" }
rustynes-ppu = { path = "../rustynes-ppu" }
rustynes-apu = { path = "../rustynes-apu" }
rustynes-mappers = { path = "../rustynes-mappers" }

wasm-bindgen = "0.2"
wasm-bindgen-futures = "0.4"
js-sys = "0.3"
console_error_panic_hook = { version = "0.1", optional = true }

[dependencies.web-sys]
version = "0.3"
features = [
    "console",
    "Window",
    "Document",
    "Element",
    "HtmlCanvasElement",
    "CanvasRenderingContext2d",
    "ImageData",
    "KeyboardEvent",
    "GamepadEvent",
    "Gamepad",
    "GamepadButton",
    "AudioContext",
    "AudioContextState",
    "AudioBuffer",
    "AudioBufferSourceNode",
    "AudioDestinationNode",
    "GainNode",
    "ScriptProcessorNode",
    "AudioProcessingEvent",
    "Worker",
    "MessageEvent",
    "Blob",
    "BlobPropertyBag",
    "Url",
    "IdbFactory",
    "IdbDatabase",
    "IdbObjectStore",
    "IdbRequest",
    "IdbTransaction",
    "IdbTransactionMode",
    "Performance",
    "PerformanceTiming",
    "Storage",
    "File",
    "FileReader",
    "DragEvent",
    "DataTransfer",
]

[dev-dependencies]
wasm-bindgen-test = "0.3"

[profile.release]
opt-level = "z"          # Optimize for size
lto = true               # Link-time optimization
codegen-units = 1        # Single codegen unit for better optimization
panic = "abort"          # No unwinding in WASM
```

## WASM Entry Point

```rust
// src/lib.rs
use wasm_bindgen::prelude::*;
use web_sys::console;

mod emulator;
mod audio;
mod video;
mod input;
mod storage;

pub use emulator::WasmEmulator;
pub use audio::WebAudioPlayer;
pub use video::CanvasRenderer;
pub use input::InputHandler;
pub use storage::IndexedDbStorage;

/// Initialize panic hook for better error messages
#[wasm_bindgen(start)]
pub fn init() {
    #[cfg(feature = "console_error_panic_hook")]
    console_error_panic_hook::set_once();

    console::log_1(&"RustyNES WASM initialized".into());
}

/// Create a new emulator instance
#[wasm_bindgen]
pub fn create_emulator() -> WasmEmulator {
    WasmEmulator::new()
}

/// Get version string
#[wasm_bindgen]
pub fn version() -> String {
    env!("CARGO_PKG_VERSION").to_string()
}

/// Check if WebGL is available
#[wasm_bindgen]
pub fn webgl_available() -> bool {
    let window = web_sys::window().expect("no global window");
    let document = window.document().expect("no document");
    let canvas = document
        .create_element("canvas")
        .expect("failed to create canvas")
        .dyn_into::<web_sys::HtmlCanvasElement>()
        .expect("not a canvas");

    canvas.get_context("webgl2").ok().flatten().is_some()
        || canvas.get_context("webgl").ok().flatten().is_some()
}
```

## Emulator Wrapper

```rust
// src/emulator.rs
use wasm_bindgen::prelude::*;
use js_sys::{Array, Uint8Array, Uint8ClampedArray};
use rustynes_core::{Emulator, EmulatorConfig};
use crate::audio::WebAudioPlayer;
use crate::video::CanvasRenderer;
use crate::input::InputHandler;
use crate::storage::IndexedDbStorage;

/// WebAssembly-compatible emulator wrapper
#[wasm_bindgen]
pub struct WasmEmulator {
    /// Core emulator instance
    emulator: Option<Emulator>,
    /// Canvas renderer
    renderer: Option<CanvasRenderer>,
    /// Audio player
    audio: Option<WebAudioPlayer>,
    /// Input handler
    input: InputHandler,
    /// Frame counter
    frame_count: u64,
    /// Running state
    running: bool,
    /// Performance timing
    last_frame_time: f64,
    frame_times: Vec<f64>,
}

#[wasm_bindgen]
impl WasmEmulator {
    /// Create new emulator instance
    #[wasm_bindgen(constructor)]
    pub fn new() -> Self {
        Self {
            emulator: None,
            renderer: None,
            audio: None,
            input: InputHandler::new(),
            frame_count: 0,
            running: false,
            last_frame_time: 0.0,
            frame_times: Vec::with_capacity(60),
        }
    }

    /// Initialize with canvas element
    #[wasm_bindgen]
    pub fn init(&mut self, canvas_id: &str) -> Result<(), JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let document = window.document().ok_or("no document")?;
        let canvas = document
            .get_element_by_id(canvas_id)
            .ok_or("canvas not found")?
            .dyn_into::<web_sys::HtmlCanvasElement>()?;

        // Initialize renderer
        self.renderer = Some(CanvasRenderer::new(canvas)?);

        // Initialize audio
        self.audio = Some(WebAudioPlayer::new()?);

        // Set up input handlers
        self.input.setup_keyboard_handlers(&document)?;
        self.input.setup_gamepad_handlers(&window)?;

        Ok(())
    }

    /// Load ROM from Uint8Array
    #[wasm_bindgen]
    pub fn load_rom(&mut self, data: &Uint8Array) -> Result<String, JsValue> {
        let rom_data: Vec<u8> = data.to_vec();

        let config = EmulatorConfig::default();
        let emulator = Emulator::from_rom_data(&rom_data, config)
            .map_err(|e| JsValue::from_str(&e.to_string()))?;

        let rom_name = emulator.rom_name().to_string();
        self.emulator = Some(emulator);
        self.frame_count = 0;

        Ok(rom_name)
    }

    /// Load ROM from URL
    #[wasm_bindgen]
    pub async fn load_rom_url(&mut self, url: &str) -> Result<String, JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let response = wasm_bindgen_futures::JsFuture::from(window.fetch_with_str(url)).await?;
        let response: web_sys::Response = response.dyn_into()?;

        if !response.ok() {
            return Err(JsValue::from_str(&format!(
                "Failed to fetch ROM: {}",
                response.status()
            )));
        }

        let array_buffer = wasm_bindgen_futures::JsFuture::from(response.array_buffer()?).await?;
        let uint8_array = Uint8Array::new(&array_buffer);

        self.load_rom(&uint8_array)
    }

    /// Run single frame
    #[wasm_bindgen]
    pub fn run_frame(&mut self) -> Result<(), JsValue> {
        let emulator = self.emulator.as_mut().ok_or("no ROM loaded")?;

        // Get current input state
        let input = self.input.get_state();
        emulator.set_controller_state(0, input.0);
        emulator.set_controller_state(1, input.1);

        // Run one frame
        emulator.run_frame();
        self.frame_count += 1;

        // Render frame
        if let Some(renderer) = &mut self.renderer {
            let framebuffer = emulator.framebuffer();
            renderer.render(framebuffer)?;
        }

        // Queue audio samples
        if let Some(audio) = &mut self.audio {
            let samples = emulator.audio_samples();
            audio.queue_samples(samples)?;
        }

        Ok(())
    }

    /// Start emulation loop
    #[wasm_bindgen]
    pub fn start(&mut self) -> Result<(), JsValue> {
        if self.emulator.is_none() {
            return Err(JsValue::from_str("no ROM loaded"));
        }

        self.running = true;

        // Resume audio context (required after user interaction)
        if let Some(audio) = &self.audio {
            audio.resume()?;
        }

        Ok(())
    }

    /// Pause emulation
    #[wasm_bindgen]
    pub fn pause(&mut self) {
        self.running = false;
        if let Some(audio) = &self.audio {
            let _ = audio.suspend();
        }
    }

    /// Reset emulator
    #[wasm_bindgen]
    pub fn reset(&mut self) {
        if let Some(emulator) = &mut self.emulator {
            emulator.reset();
            self.frame_count = 0;
        }
    }

    /// Check if running
    #[wasm_bindgen]
    pub fn is_running(&self) -> bool {
        self.running && self.emulator.is_some()
    }

    /// Get current frame count
    #[wasm_bindgen]
    pub fn frame_count(&self) -> u64 {
        self.frame_count
    }

    /// Get framebuffer as ImageData-compatible array
    #[wasm_bindgen]
    pub fn get_framebuffer(&self) -> Option<Uint8ClampedArray> {
        let emulator = self.emulator.as_ref()?;
        let fb = emulator.framebuffer();

        // Convert to RGBA format
        let mut rgba = Vec::with_capacity(256 * 240 * 4);
        for pixel in fb {
            rgba.push((pixel >> 16) as u8); // R
            rgba.push((pixel >> 8) as u8);  // G
            rgba.push(*pixel as u8);        // B
            rgba.push(255);                 // A
        }

        Some(Uint8ClampedArray::from(&rgba[..]))
    }

    /// Create save state
    #[wasm_bindgen]
    pub fn save_state(&self) -> Option<Uint8Array> {
        let emulator = self.emulator.as_ref()?;
        let state = emulator.save_state().ok()?;
        Some(Uint8Array::from(&state[..]))
    }

    /// Load save state
    #[wasm_bindgen]
    pub fn load_state(&mut self, data: &Uint8Array) -> Result<(), JsValue> {
        let emulator = self.emulator.as_mut().ok_or("no ROM loaded")?;
        let state_data = data.to_vec();
        emulator
            .load_state(&state_data)
            .map_err(|e| JsValue::from_str(&e.to_string()))
    }

    /// Get SRAM data for battery saves
    #[wasm_bindgen]
    pub fn get_sram(&self) -> Option<Uint8Array> {
        let emulator = self.emulator.as_ref()?;
        let sram = emulator.sram()?;
        Some(Uint8Array::from(sram))
    }

    /// Load SRAM data
    #[wasm_bindgen]
    pub fn set_sram(&mut self, data: &Uint8Array) -> Result<(), JsValue> {
        let emulator = self.emulator.as_mut().ok_or("no ROM loaded")?;
        let sram_data = data.to_vec();
        emulator.set_sram(&sram_data);
        Ok(())
    }

    /// Set audio volume (0.0 - 1.0)
    #[wasm_bindgen]
    pub fn set_volume(&mut self, volume: f32) {
        if let Some(audio) = &mut self.audio {
            audio.set_volume(volume);
        }
    }

    /// Enable/disable specific audio channel
    #[wasm_bindgen]
    pub fn set_channel_enabled(&mut self, channel: u8, enabled: bool) {
        if let Some(emulator) = &mut self.emulator {
            emulator.set_channel_enabled(channel, enabled);
        }
    }

    /// Update performance timing
    #[wasm_bindgen]
    pub fn update_timing(&mut self, timestamp: f64) {
        if self.last_frame_time > 0.0 {
            let delta = timestamp - self.last_frame_time;
            self.frame_times.push(delta);
            if self.frame_times.len() > 60 {
                self.frame_times.remove(0);
            }
        }
        self.last_frame_time = timestamp;
    }

    /// Get average FPS
    #[wasm_bindgen]
    pub fn fps(&self) -> f64 {
        if self.frame_times.is_empty() {
            return 0.0;
        }
        let avg_ms: f64 = self.frame_times.iter().sum::<f64>() / self.frame_times.len() as f64;
        if avg_ms > 0.0 {
            1000.0 / avg_ms
        } else {
            0.0
        }
    }

    /// Handle file drop
    #[wasm_bindgen]
    pub async fn handle_file_drop(&mut self, file: web_sys::File) -> Result<String, JsValue> {
        let file_name = file.name();

        // Read file contents
        let array_buffer = wasm_bindgen_futures::JsFuture::from(file.array_buffer()).await?;
        let uint8_array = Uint8Array::new(&array_buffer);

        // Check file extension
        if file_name.ends_with(".nes") || file_name.ends_with(".NES") {
            self.load_rom(&uint8_array)
        } else if file_name.ends_with(".state") || file_name.ends_with(".sav") {
            self.load_state(&uint8_array)?;
            Ok(format!("Loaded state: {}", file_name))
        } else {
            Err(JsValue::from_str("Unsupported file type"))
        }
    }
}

impl Default for WasmEmulator {
    fn default() -> Self {
        Self::new()
    }
}
```

## Canvas Renderer

```rust
// src/video.rs
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{CanvasRenderingContext2d, HtmlCanvasElement, ImageData};

/// Canvas-based renderer for NES output
pub struct CanvasRenderer {
    canvas: HtmlCanvasElement,
    context: CanvasRenderingContext2d,
    /// Pixel buffer for ImageData
    pixel_buffer: Vec<u8>,
    /// Scaling factor
    scale: u32,
    /// Rendering mode
    mode: RenderMode,
}

#[derive(Clone, Copy)]
pub enum RenderMode {
    Canvas2d,
    #[cfg(feature = "webgl")]
    WebGL,
}

impl CanvasRenderer {
    /// Create new canvas renderer
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        let context = canvas
            .get_context("2d")?
            .ok_or("failed to get 2d context")?
            .dyn_into::<CanvasRenderingContext2d>()?;

        // Disable image smoothing for crisp pixels
        context.set_image_smoothing_enabled(false);

        // Calculate scale based on canvas size
        let width = canvas.width();
        let scale = width / 256;

        Ok(Self {
            canvas,
            context,
            pixel_buffer: vec![0; 256 * 240 * 4],
            scale: scale.max(1),
            mode: RenderMode::Canvas2d,
        })
    }

    /// Render framebuffer to canvas
    pub fn render(&mut self, framebuffer: &[u32]) -> Result<(), JsValue> {
        // Convert RGB to RGBA
        for (i, &pixel) in framebuffer.iter().enumerate() {
            let base = i * 4;
            self.pixel_buffer[base] = ((pixel >> 16) & 0xFF) as u8;     // R
            self.pixel_buffer[base + 1] = ((pixel >> 8) & 0xFF) as u8;  // G
            self.pixel_buffer[base + 2] = (pixel & 0xFF) as u8;         // B
            self.pixel_buffer[base + 3] = 255;                          // A
        }

        // Create ImageData
        let image_data = ImageData::new_with_u8_clamped_array_and_sh(
            wasm_bindgen::Clamped(&self.pixel_buffer),
            256,
            240,
        )?;

        // Draw to canvas
        if self.scale == 1 {
            self.context.put_image_data(&image_data, 0.0, 0.0)?;
        } else {
            // For scaled rendering, use temporary canvas
            let temp_canvas = web_sys::window()
                .ok_or("no window")?
                .document()
                .ok_or("no document")?
                .create_element("canvas")?
                .dyn_into::<HtmlCanvasElement>()?;

            temp_canvas.set_width(256);
            temp_canvas.set_height(240);

            let temp_ctx = temp_canvas
                .get_context("2d")?
                .ok_or("no context")?
                .dyn_into::<CanvasRenderingContext2d>()?;

            temp_ctx.put_image_data(&image_data, 0.0, 0.0)?;

            // Scale draw to main canvas
            self.context.draw_image_with_html_canvas_element_and_dw_and_dh(
                &temp_canvas,
                0.0,
                0.0,
                (256 * self.scale) as f64,
                (240 * self.scale) as f64,
            )?;
        }

        Ok(())
    }

    /// Set canvas scale
    pub fn set_scale(&mut self, scale: u32) {
        self.scale = scale.max(1);
        self.canvas.set_width(256 * self.scale);
        self.canvas.set_height(240 * self.scale);
        self.context.set_image_smoothing_enabled(false);
    }

    /// Apply CRT filter effect (simple scanlines)
    pub fn apply_scanlines(&self, intensity: f64) -> Result<(), JsValue> {
        let width = self.canvas.width() as f64;
        let height = self.canvas.height() as f64;

        self.context.set_fill_style_str(&format!("rgba(0, 0, 0, {})", intensity));

        let scanline_height = (self.scale as f64).max(1.0);
        let mut y = scanline_height;
        while y < height {
            self.context.fill_rect(0.0, y, width, scanline_height / 2.0);
            y += scanline_height * 2.0;
        }

        Ok(())
    }

    /// Take screenshot as data URL
    pub fn screenshot(&self) -> Result<String, JsValue> {
        self.canvas
            .to_data_url_with_type("image/png")
            .map_err(|e| e.into())
    }
}

#[cfg(feature = "webgl")]
pub struct WebGLRenderer {
    canvas: HtmlCanvasElement,
    gl: web_sys::WebGl2RenderingContext,
    program: web_sys::WebGlProgram,
    texture: web_sys::WebGlTexture,
    vao: web_sys::WebGlVertexArrayObject,
}

#[cfg(feature = "webgl")]
impl WebGLRenderer {
    pub fn new(canvas: HtmlCanvasElement) -> Result<Self, JsValue> {
        let gl = canvas
            .get_context("webgl2")?
            .ok_or("WebGL2 not supported")?
            .dyn_into::<web_sys::WebGl2RenderingContext>()?;

        // Compile shaders
        let vert_shader = Self::compile_shader(
            &gl,
            web_sys::WebGl2RenderingContext::VERTEX_SHADER,
            VERTEX_SHADER_SOURCE,
        )?;

        let frag_shader = Self::compile_shader(
            &gl,
            web_sys::WebGl2RenderingContext::FRAGMENT_SHADER,
            FRAGMENT_SHADER_SOURCE,
        )?;

        // Link program
        let program = gl.create_program().ok_or("failed to create program")?;
        gl.attach_shader(&program, &vert_shader);
        gl.attach_shader(&program, &frag_shader);
        gl.link_program(&program);

        if !gl
            .get_program_parameter(&program, web_sys::WebGl2RenderingContext::LINK_STATUS)
            .as_bool()
            .unwrap_or(false)
        {
            let info = gl.get_program_info_log(&program).unwrap_or_default();
            return Err(JsValue::from_str(&format!("Program link error: {}", info)));
        }

        // Create texture
        let texture = gl.create_texture().ok_or("failed to create texture")?;
        gl.bind_texture(web_sys::WebGl2RenderingContext::TEXTURE_2D, Some(&texture));

        // Set texture parameters for nearest-neighbor scaling
        gl.tex_parameteri(
            web_sys::WebGl2RenderingContext::TEXTURE_2D,
            web_sys::WebGl2RenderingContext::TEXTURE_MIN_FILTER,
            web_sys::WebGl2RenderingContext::NEAREST as i32,
        );
        gl.tex_parameteri(
            web_sys::WebGl2RenderingContext::TEXTURE_2D,
            web_sys::WebGl2RenderingContext::TEXTURE_MAG_FILTER,
            web_sys::WebGl2RenderingContext::NEAREST as i32,
        );

        // Create VAO with fullscreen quad
        let vao = gl.create_vertex_array().ok_or("failed to create VAO")?;
        gl.bind_vertex_array(Some(&vao));

        // Quad vertices
        let vertices: [f32; 16] = [
            // positions    // texcoords
            -1.0, -1.0, 0.0, 1.0, // bottom-left
            1.0, -1.0, 1.0, 1.0,  // bottom-right
            -1.0, 1.0, 0.0, 0.0,  // top-left
            1.0, 1.0, 1.0, 0.0,   // top-right
        ];

        let buffer = gl.create_buffer().ok_or("failed to create buffer")?;
        gl.bind_buffer(web_sys::WebGl2RenderingContext::ARRAY_BUFFER, Some(&buffer));

        unsafe {
            let vertex_array = js_sys::Float32Array::view(&vertices);
            gl.buffer_data_with_array_buffer_view(
                web_sys::WebGl2RenderingContext::ARRAY_BUFFER,
                &vertex_array,
                web_sys::WebGl2RenderingContext::STATIC_DRAW,
            );
        }

        // Position attribute
        gl.vertex_attrib_pointer_with_i32(0, 2, web_sys::WebGl2RenderingContext::FLOAT, false, 16, 0);
        gl.enable_vertex_attrib_array(0);

        // Texcoord attribute
        gl.vertex_attrib_pointer_with_i32(1, 2, web_sys::WebGl2RenderingContext::FLOAT, false, 16, 8);
        gl.enable_vertex_attrib_array(1);

        Ok(Self {
            canvas,
            gl,
            program,
            texture,
            vao,
        })
    }

    fn compile_shader(
        gl: &web_sys::WebGl2RenderingContext,
        shader_type: u32,
        source: &str,
    ) -> Result<web_sys::WebGlShader, JsValue> {
        let shader = gl.create_shader(shader_type).ok_or("failed to create shader")?;
        gl.shader_source(&shader, source);
        gl.compile_shader(&shader);

        if !gl
            .get_shader_parameter(&shader, web_sys::WebGl2RenderingContext::COMPILE_STATUS)
            .as_bool()
            .unwrap_or(false)
        {
            let info = gl.get_shader_info_log(&shader).unwrap_or_default();
            return Err(JsValue::from_str(&format!("Shader compile error: {}", info)));
        }

        Ok(shader)
    }

    pub fn render(&self, framebuffer: &[u32]) -> Result<(), JsValue> {
        let gl = &self.gl;

        // Convert to RGB bytes
        let mut rgb_data = Vec::with_capacity(256 * 240 * 3);
        for &pixel in framebuffer {
            rgb_data.push(((pixel >> 16) & 0xFF) as u8);
            rgb_data.push(((pixel >> 8) & 0xFF) as u8);
            rgb_data.push((pixel & 0xFF) as u8);
        }

        // Update texture
        gl.bind_texture(web_sys::WebGl2RenderingContext::TEXTURE_2D, Some(&self.texture));
        gl.tex_image_2d_with_i32_and_i32_and_i32_and_format_and_type_and_opt_u8_array(
            web_sys::WebGl2RenderingContext::TEXTURE_2D,
            0,
            web_sys::WebGl2RenderingContext::RGB as i32,
            256,
            240,
            0,
            web_sys::WebGl2RenderingContext::RGB,
            web_sys::WebGl2RenderingContext::UNSIGNED_BYTE,
            Some(&rgb_data),
        )?;

        // Render
        gl.viewport(0, 0, self.canvas.width() as i32, self.canvas.height() as i32);
        gl.use_program(Some(&self.program));
        gl.bind_vertex_array(Some(&self.vao));
        gl.draw_arrays(web_sys::WebGl2RenderingContext::TRIANGLE_STRIP, 0, 4);

        Ok(())
    }
}

#[cfg(feature = "webgl")]
const VERTEX_SHADER_SOURCE: &str = r#"#version 300 es
layout(location = 0) in vec2 a_position;
layout(location = 1) in vec2 a_texcoord;
out vec2 v_texcoord;
void main() {
    gl_Position = vec4(a_position, 0.0, 1.0);
    v_texcoord = a_texcoord;
}
"#;

#[cfg(feature = "webgl")]
const FRAGMENT_SHADER_SOURCE: &str = r#"#version 300 es
precision mediump float;
in vec2 v_texcoord;
out vec4 fragColor;
uniform sampler2D u_texture;
void main() {
    fragColor = texture(u_texture, v_texcoord);
}
"#;
```

## Web Audio Integration

```rust
// src/audio.rs
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{AudioContext, AudioBuffer, GainNode};
use std::collections::VecDeque;

/// Web Audio API-based audio player
pub struct WebAudioPlayer {
    context: AudioContext,
    gain_node: GainNode,
    /// Sample rate (typically 44100)
    sample_rate: u32,
    /// Buffer of pending samples
    sample_buffer: VecDeque<f32>,
    /// Samples per buffer
    buffer_size: usize,
    /// Current volume
    volume: f32,
    /// Audio is muted
    muted: bool,
    /// Target latency in samples
    target_latency: usize,
}

impl WebAudioPlayer {
    /// Create new Web Audio player
    pub fn new() -> Result<Self, JsValue> {
        let context = AudioContext::new()?;
        let sample_rate = context.sample_rate() as u32;

        // Create gain node for volume control
        let gain_node = context.create_gain()?;
        gain_node.connect_with_audio_node(&context.destination())?;
        gain_node.gain().set_value(1.0);

        // Buffer size tuned for latency vs. stability
        // ~3 frames worth of audio at 60fps
        let buffer_size = (sample_rate as usize / 60) * 3;

        Ok(Self {
            context,
            gain_node,
            sample_rate,
            sample_buffer: VecDeque::with_capacity(buffer_size * 2),
            buffer_size,
            volume: 1.0,
            muted: false,
            target_latency: buffer_size,
        })
    }

    /// Resume audio context (required after user interaction)
    pub fn resume(&self) -> Result<(), JsValue> {
        if self.context.state() == web_sys::AudioContextState::Suspended {
            let _ = self.context.resume()?;
        }
        Ok(())
    }

    /// Suspend audio context
    pub fn suspend(&self) -> Result<(), JsValue> {
        let _ = self.context.suspend()?;
        Ok(())
    }

    /// Queue audio samples from emulator
    pub fn queue_samples(&mut self, samples: &[f32]) -> Result<(), JsValue> {
        // Add to buffer
        self.sample_buffer.extend(samples.iter().cloned());

        // If we have enough samples, create and play audio buffer
        while self.sample_buffer.len() >= self.buffer_size {
            self.flush_buffer()?;
        }

        Ok(())
    }

    /// Flush buffered samples to audio output
    fn flush_buffer(&mut self) -> Result<(), JsValue> {
        if self.sample_buffer.len() < self.buffer_size {
            return Ok(());
        }

        // Create audio buffer
        let buffer = self.context.create_buffer(
            1, // mono
            self.buffer_size as u32,
            self.sample_rate as f32,
        )?;

        // Fill buffer
        let mut channel_data = buffer.get_channel_data(0)?;
        for i in 0..self.buffer_size {
            if let Some(sample) = self.sample_buffer.pop_front() {
                channel_data[i] = sample;
            }
        }

        // Create source and play
        let source = self.context.create_buffer_source()?;
        source.set_buffer(Some(&buffer));
        source.connect_with_audio_node(&self.gain_node)?;

        // Schedule playback
        let current_time = self.context.current_time();
        source.start_with_when(current_time)?;

        Ok(())
    }

    /// Set volume (0.0 - 1.0)
    pub fn set_volume(&mut self, volume: f32) {
        self.volume = volume.clamp(0.0, 1.0);
        if !self.muted {
            self.gain_node.gain().set_value(self.volume);
        }
    }

    /// Get current volume
    pub fn volume(&self) -> f32 {
        self.volume
    }

    /// Mute/unmute audio
    pub fn set_muted(&mut self, muted: bool) {
        self.muted = muted;
        if muted {
            self.gain_node.gain().set_value(0.0);
        } else {
            self.gain_node.gain().set_value(self.volume);
        }
    }

    /// Check if muted
    pub fn is_muted(&self) -> bool {
        self.muted
    }

    /// Get buffer fill level (for debugging)
    pub fn buffer_fill(&self) -> usize {
        self.sample_buffer.len()
    }

    /// Get latency estimate in milliseconds
    pub fn latency_ms(&self) -> f64 {
        let samples = self.sample_buffer.len();
        (samples as f64 / self.sample_rate as f64) * 1000.0
    }

    /// Clear audio buffer
    pub fn clear(&mut self) {
        self.sample_buffer.clear();
    }
}

/// Audio worklet-based player for lower latency (Chrome)
pub struct AudioWorkletPlayer {
    context: AudioContext,
    // Worklet node would be stored here
    sample_buffer: VecDeque<f32>,
}

impl AudioWorkletPlayer {
    /// Create new worklet-based player
    pub async fn new() -> Result<Self, JsValue> {
        let context = AudioContext::new()?;

        // Register audio worklet
        // Note: This requires serving the worklet script
        // await context.audioWorklet.addModule('worklet.js');

        Ok(Self {
            context,
            sample_buffer: VecDeque::new(),
        })
    }
}
```

## Input Handling

```rust
// src/input.rs
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{Document, KeyboardEvent, Window, Gamepad};
use std::cell::RefCell;
use std::rc::Rc;

/// NES controller button flags
pub mod buttons {
    pub const A: u8 = 0x01;
    pub const B: u8 = 0x02;
    pub const SELECT: u8 = 0x04;
    pub const START: u8 = 0x08;
    pub const UP: u8 = 0x10;
    pub const DOWN: u8 = 0x20;
    pub const LEFT: u8 = 0x40;
    pub const RIGHT: u8 = 0x80;
}

/// Input state for both controllers
#[derive(Default, Clone, Copy)]
pub struct InputState(pub u8, pub u8);

/// Keyboard and gamepad input handler
pub struct InputHandler {
    /// Shared state updated by event handlers
    state: Rc<RefCell<InputState>>,
    /// Key bindings for player 1
    p1_bindings: KeyBindings,
    /// Key bindings for player 2
    p2_bindings: KeyBindings,
}

/// Key bindings configuration
#[derive(Clone)]
pub struct KeyBindings {
    pub up: String,
    pub down: String,
    pub left: String,
    pub right: String,
    pub a: String,
    pub b: String,
    pub start: String,
    pub select: String,
}

impl Default for KeyBindings {
    fn default() -> Self {
        Self {
            up: "ArrowUp".to_string(),
            down: "ArrowDown".to_string(),
            left: "ArrowLeft".to_string(),
            right: "ArrowRight".to_string(),
            a: "KeyX".to_string(),
            b: "KeyZ".to_string(),
            start: "Enter".to_string(),
            select: "ShiftRight".to_string(),
        }
    }
}

impl KeyBindings {
    /// Alternative WASD bindings for player 2
    pub fn wasd() -> Self {
        Self {
            up: "KeyW".to_string(),
            down: "KeyS".to_string(),
            left: "KeyA".to_string(),
            right: "KeyD".to_string(),
            a: "KeyG".to_string(),
            b: "KeyF".to_string(),
            start: "KeyT".to_string(),
            select: "KeyR".to_string(),
        }
    }
}

impl InputHandler {
    /// Create new input handler
    pub fn new() -> Self {
        Self {
            state: Rc::new(RefCell::new(InputState::default())),
            p1_bindings: KeyBindings::default(),
            p2_bindings: KeyBindings::wasd(),
        }
    }

    /// Get current input state
    pub fn get_state(&self) -> InputState {
        *self.state.borrow()
    }

    /// Set custom key bindings
    pub fn set_bindings(&mut self, player: u8, bindings: KeyBindings) {
        match player {
            0 => self.p1_bindings = bindings,
            1 => self.p2_bindings = bindings,
            _ => {}
        }
    }

    /// Set up keyboard event handlers
    pub fn setup_keyboard_handlers(&self, document: &Document) -> Result<(), JsValue> {
        let state = self.state.clone();
        let p1_bindings = self.p1_bindings.clone();
        let p2_bindings = self.p2_bindings.clone();

        // Key down handler
        let state_down = state.clone();
        let p1_down = p1_bindings.clone();
        let p2_down = p2_bindings.clone();
        let keydown_callback = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            let code = event.code();
            let mut s = state_down.borrow_mut();

            // Player 1
            if code == p1_down.up {
                s.0 |= buttons::UP;
            } else if code == p1_down.down {
                s.0 |= buttons::DOWN;
            } else if code == p1_down.left {
                s.0 |= buttons::LEFT;
            } else if code == p1_down.right {
                s.0 |= buttons::RIGHT;
            } else if code == p1_down.a {
                s.0 |= buttons::A;
            } else if code == p1_down.b {
                s.0 |= buttons::B;
            } else if code == p1_down.start {
                s.0 |= buttons::START;
            } else if code == p1_down.select {
                s.0 |= buttons::SELECT;
            }

            // Player 2
            if code == p2_down.up {
                s.1 |= buttons::UP;
            } else if code == p2_down.down {
                s.1 |= buttons::DOWN;
            } else if code == p2_down.left {
                s.1 |= buttons::LEFT;
            } else if code == p2_down.right {
                s.1 |= buttons::RIGHT;
            } else if code == p2_down.a {
                s.1 |= buttons::A;
            } else if code == p2_down.b {
                s.1 |= buttons::B;
            } else if code == p2_down.start {
                s.1 |= buttons::START;
            } else if code == p2_down.select {
                s.1 |= buttons::SELECT;
            }

            // Prevent default for game keys
            if Self::is_game_key(&code, &p1_down, &p2_down) {
                event.prevent_default();
            }
        }) as Box<dyn FnMut(KeyboardEvent)>);

        document.add_event_listener_with_callback(
            "keydown",
            keydown_callback.as_ref().unchecked_ref(),
        )?;
        keydown_callback.forget();

        // Key up handler
        let state_up = state;
        let p1_up = p1_bindings;
        let p2_up = p2_bindings;
        let keyup_callback = Closure::wrap(Box::new(move |event: KeyboardEvent| {
            let code = event.code();
            let mut s = state_up.borrow_mut();

            // Player 1
            if code == p1_up.up {
                s.0 &= !buttons::UP;
            } else if code == p1_up.down {
                s.0 &= !buttons::DOWN;
            } else if code == p1_up.left {
                s.0 &= !buttons::LEFT;
            } else if code == p1_up.right {
                s.0 &= !buttons::RIGHT;
            } else if code == p1_up.a {
                s.0 &= !buttons::A;
            } else if code == p1_up.b {
                s.0 &= !buttons::B;
            } else if code == p1_up.start {
                s.0 &= !buttons::START;
            } else if code == p1_up.select {
                s.0 &= !buttons::SELECT;
            }

            // Player 2
            if code == p2_up.up {
                s.1 &= !buttons::UP;
            } else if code == p2_up.down {
                s.1 &= !buttons::DOWN;
            } else if code == p2_up.left {
                s.1 &= !buttons::LEFT;
            } else if code == p2_up.right {
                s.1 &= !buttons::RIGHT;
            } else if code == p2_up.a {
                s.1 &= !buttons::A;
            } else if code == p2_up.b {
                s.1 &= !buttons::B;
            } else if code == p2_up.start {
                s.1 &= !buttons::START;
            } else if code == p2_up.select {
                s.1 &= !buttons::SELECT;
            }
        }) as Box<dyn FnMut(KeyboardEvent)>);

        document.add_event_listener_with_callback("keyup", keyup_callback.as_ref().unchecked_ref())?;
        keyup_callback.forget();

        Ok(())
    }

    fn is_game_key(code: &str, p1: &KeyBindings, p2: &KeyBindings) -> bool {
        code == p1.up
            || code == p1.down
            || code == p1.left
            || code == p1.right
            || code == p1.a
            || code == p1.b
            || code == p1.start
            || code == p1.select
            || code == p2.up
            || code == p2.down
            || code == p2.left
            || code == p2.right
            || code == p2.a
            || code == p2.b
            || code == p2.start
            || code == p2.select
    }

    /// Set up gamepad handlers
    pub fn setup_gamepad_handlers(&self, window: &Window) -> Result<(), JsValue> {
        // Gamepad connected handler
        let connected_callback = Closure::wrap(Box::new(move |event: web_sys::GamepadEvent| {
            if let Some(gamepad) = event.gamepad() {
                web_sys::console::log_1(
                    &format!("Gamepad connected: {} ({})", gamepad.id(), gamepad.index()).into(),
                );
            }
        }) as Box<dyn FnMut(web_sys::GamepadEvent)>);

        window.add_event_listener_with_callback(
            "gamepadconnected",
            connected_callback.as_ref().unchecked_ref(),
        )?;
        connected_callback.forget();

        Ok(())
    }

    /// Poll gamepads and update state
    pub fn poll_gamepads(&self) {
        let window = match web_sys::window() {
            Some(w) => w,
            None => return,
        };

        let gamepads = match window.navigator().get_gamepads() {
            Ok(g) => g,
            Err(_) => return,
        };

        let mut state = self.state.borrow_mut();

        for i in 0..gamepads.length() {
            if let Some(gamepad_val) = gamepads.get(i) {
                if gamepad_val.is_null() {
                    continue;
                }
                if let Ok(gamepad) = gamepad_val.dyn_into::<Gamepad>() {
                    let player = (gamepad.index() as u8).min(1);
                    let buttons = self.read_gamepad_buttons(&gamepad);

                    match player {
                        0 => state.0 = buttons,
                        1 => state.1 = buttons,
                        _ => {}
                    }
                }
            }
        }
    }

    fn read_gamepad_buttons(&self, gamepad: &Gamepad) -> u8 {
        let buttons_array = gamepad.buttons();
        let axes = gamepad.axes();

        let mut result = 0u8;

        // Standard gamepad button mapping
        if let Some(btn) = buttons_array.get(0) {
            if let Ok(b) = btn.dyn_into::<web_sys::GamepadButton>() {
                if b.pressed() {
                    result |= buttons::B;
                }
            }
        }
        if let Some(btn) = buttons_array.get(1) {
            if let Ok(b) = btn.dyn_into::<web_sys::GamepadButton>() {
                if b.pressed() {
                    result |= buttons::A;
                }
            }
        }
        if let Some(btn) = buttons_array.get(8) {
            if let Ok(b) = btn.dyn_into::<web_sys::GamepadButton>() {
                if b.pressed() {
                    result |= buttons::SELECT;
                }
            }
        }
        if let Some(btn) = buttons_array.get(9) {
            if let Ok(b) = btn.dyn_into::<web_sys::GamepadButton>() {
                if b.pressed() {
                    result |= buttons::START;
                }
            }
        }

        // D-pad buttons (12-15)
        if let Some(btn) = buttons_array.get(12) {
            if let Ok(b) = btn.dyn_into::<web_sys::GamepadButton>() {
                if b.pressed() {
                    result |= buttons::UP;
                }
            }
        }
        if let Some(btn) = buttons_array.get(13) {
            if let Ok(b) = btn.dyn_into::<web_sys::GamepadButton>() {
                if b.pressed() {
                    result |= buttons::DOWN;
                }
            }
        }
        if let Some(btn) = buttons_array.get(14) {
            if let Ok(b) = btn.dyn_into::<web_sys::GamepadButton>() {
                if b.pressed() {
                    result |= buttons::LEFT;
                }
            }
        }
        if let Some(btn) = buttons_array.get(15) {
            if let Ok(b) = btn.dyn_into::<web_sys::GamepadButton>() {
                if b.pressed() {
                    result |= buttons::RIGHT;
                }
            }
        }

        // Analog stick (axes 0,1) as d-pad fallback
        if axes.length() >= 2 {
            let x = axes.get(0).as_f64().unwrap_or(0.0);
            let y = axes.get(1).as_f64().unwrap_or(0.0);

            if x < -0.5 {
                result |= buttons::LEFT;
            }
            if x > 0.5 {
                result |= buttons::RIGHT;
            }
            if y < -0.5 {
                result |= buttons::UP;
            }
            if y > 0.5 {
                result |= buttons::DOWN;
            }
        }

        result
    }
}

impl Default for InputHandler {
    fn default() -> Self {
        Self::new()
    }
}
```

## IndexedDB Storage

```rust
// src/storage.rs
use wasm_bindgen::prelude::*;
use wasm_bindgen::JsCast;
use web_sys::{IdbDatabase, IdbRequest, IdbTransaction};
use wasm_bindgen_futures::JsFuture;
use js_sys::{Uint8Array, Promise};

const DB_NAME: &str = "rustynes";
const DB_VERSION: u32 = 1;
const SAVES_STORE: &str = "saves";
const STATES_STORE: &str = "states";
const SETTINGS_STORE: &str = "settings";

/// IndexedDB-based persistent storage
pub struct IndexedDbStorage {
    db: Option<IdbDatabase>,
}

impl IndexedDbStorage {
    /// Create new storage instance
    pub fn new() -> Self {
        Self { db: None }
    }

    /// Initialize database connection
    pub async fn init(&mut self) -> Result<(), JsValue> {
        let window = web_sys::window().ok_or("no window")?;
        let idb = window
            .indexed_db()?
            .ok_or("IndexedDB not available")?;

        let request = idb.open_with_u32(DB_NAME, DB_VERSION)?;

        // Set up upgrade handler for schema creation
        let upgrade_callback = Closure::once(Box::new(move |event: web_sys::IdbVersionChangeEvent| {
            let request: IdbRequest = event.target().unwrap().dyn_into().unwrap();
            let db: IdbDatabase = request.result().unwrap().dyn_into().unwrap();

            // Create object stores if they don't exist
            if !db.object_store_names().contains(&SAVES_STORE.into()) {
                db.create_object_store(SAVES_STORE).unwrap();
            }
            if !db.object_store_names().contains(&STATES_STORE.into()) {
                db.create_object_store(STATES_STORE).unwrap();
            }
            if !db.object_store_names().contains(&SETTINGS_STORE.into()) {
                db.create_object_store(SETTINGS_STORE).unwrap();
            }
        }) as Box<dyn FnOnce(web_sys::IdbVersionChangeEvent)>);

        request.set_onupgradeneeded(Some(upgrade_callback.as_ref().unchecked_ref()));
        upgrade_callback.forget();

        // Wait for database to open
        let promise = Promise::new(&mut |resolve, reject| {
            let resolve = resolve.clone();
            let reject = reject.clone();

            let success_callback = Closure::once(Box::new(move |_: web_sys::Event| {
                resolve.call0(&JsValue::NULL).unwrap();
            }) as Box<dyn FnOnce(web_sys::Event)>);

            let error_callback = Closure::once(Box::new(move |event: web_sys::Event| {
                reject.call1(&JsValue::NULL, &event).unwrap();
            }) as Box<dyn FnOnce(web_sys::Event)>);

            request.set_onsuccess(Some(success_callback.as_ref().unchecked_ref()));
            request.set_onerror(Some(error_callback.as_ref().unchecked_ref()));

            success_callback.forget();
            error_callback.forget();
        });

        JsFuture::from(promise).await?;

        self.db = Some(request.result()?.dyn_into()?);
        Ok(())
    }

    /// Save SRAM data for a ROM
    pub async fn save_sram(&self, rom_hash: &str, data: &[u8]) -> Result<(), JsValue> {
        self.put(SAVES_STORE, rom_hash, data).await
    }

    /// Load SRAM data for a ROM
    pub async fn load_sram(&self, rom_hash: &str) -> Result<Option<Vec<u8>>, JsValue> {
        self.get(SAVES_STORE, rom_hash).await
    }

    /// Save state with slot number
    pub async fn save_state(&self, rom_hash: &str, slot: u8, data: &[u8]) -> Result<(), JsValue> {
        let key = format!("{}_{}", rom_hash, slot);
        self.put(STATES_STORE, &key, data).await
    }

    /// Load state from slot
    pub async fn load_state(&self, rom_hash: &str, slot: u8) -> Result<Option<Vec<u8>>, JsValue> {
        let key = format!("{}_{}", rom_hash, slot);
        self.get(STATES_STORE, &key).await
    }

    /// List saved state slots for a ROM
    pub async fn list_states(&self, rom_hash: &str) -> Result<Vec<u8>, JsValue> {
        let db = self.db.as_ref().ok_or("database not initialized")?;
        let tx = db.transaction_with_str_and_mode(
            STATES_STORE,
            web_sys::IdbTransactionMode::Readonly,
        )?;
        let store = tx.object_store(STATES_STORE)?;

        let mut slots = Vec::new();
        for slot in 0..10 {
            let key = format!("{}_{}", rom_hash, slot);
            let request = store.get(&JsValue::from_str(&key))?;

            let promise = Self::request_to_promise(&request);
            let result = JsFuture::from(promise).await?;

            if !result.is_undefined() && !result.is_null() {
                slots.push(slot);
            }
        }

        Ok(slots)
    }

    /// Save setting
    pub async fn save_setting(&self, key: &str, value: &str) -> Result<(), JsValue> {
        let db = self.db.as_ref().ok_or("database not initialized")?;
        let tx = db.transaction_with_str_and_mode(
            SETTINGS_STORE,
            web_sys::IdbTransactionMode::Readwrite,
        )?;
        let store = tx.object_store(SETTINGS_STORE)?;

        store.put_with_key(&JsValue::from_str(value), &JsValue::from_str(key))?;

        let promise = Self::transaction_to_promise(&tx);
        JsFuture::from(promise).await?;

        Ok(())
    }

    /// Load setting
    pub async fn load_setting(&self, key: &str) -> Result<Option<String>, JsValue> {
        let db = self.db.as_ref().ok_or("database not initialized")?;
        let tx = db.transaction_with_str_and_mode(
            SETTINGS_STORE,
            web_sys::IdbTransactionMode::Readonly,
        )?;
        let store = tx.object_store(SETTINGS_STORE)?;
        let request = store.get(&JsValue::from_str(key))?;

        let promise = Self::request_to_promise(&request);
        let result = JsFuture::from(promise).await?;

        if result.is_undefined() || result.is_null() {
            Ok(None)
        } else {
            Ok(result.as_string())
        }
    }

    /// Delete saved data for a ROM
    pub async fn delete_rom_data(&self, rom_hash: &str) -> Result<(), JsValue> {
        let db = self.db.as_ref().ok_or("database not initialized")?;

        // Delete SRAM
        let tx = db.transaction_with_str_and_mode(
            SAVES_STORE,
            web_sys::IdbTransactionMode::Readwrite,
        )?;
        let store = tx.object_store(SAVES_STORE)?;
        store.delete(&JsValue::from_str(rom_hash))?;

        // Delete all save states
        let tx = db.transaction_with_str_and_mode(
            STATES_STORE,
            web_sys::IdbTransactionMode::Readwrite,
        )?;
        let store = tx.object_store(STATES_STORE)?;
        for slot in 0..10 {
            let key = format!("{}_{}", rom_hash, slot);
            store.delete(&JsValue::from_str(&key))?;
        }

        Ok(())
    }

    // Helper: put binary data
    async fn put(&self, store_name: &str, key: &str, data: &[u8]) -> Result<(), JsValue> {
        let db = self.db.as_ref().ok_or("database not initialized")?;
        let tx = db.transaction_with_str_and_mode(
            store_name,
            web_sys::IdbTransactionMode::Readwrite,
        )?;
        let store = tx.object_store(store_name)?;

        let array = Uint8Array::from(data);
        store.put_with_key(&array, &JsValue::from_str(key))?;

        let promise = Self::transaction_to_promise(&tx);
        JsFuture::from(promise).await?;

        Ok(())
    }

    // Helper: get binary data
    async fn get(&self, store_name: &str, key: &str) -> Result<Option<Vec<u8>>, JsValue> {
        let db = self.db.as_ref().ok_or("database not initialized")?;
        let tx = db.transaction_with_str_and_mode(
            store_name,
            web_sys::IdbTransactionMode::Readonly,
        )?;
        let store = tx.object_store(store_name)?;
        let request = store.get(&JsValue::from_str(key))?;

        let promise = Self::request_to_promise(&request);
        let result = JsFuture::from(promise).await?;

        if result.is_undefined() || result.is_null() {
            Ok(None)
        } else {
            let array = Uint8Array::new(&result);
            Ok(Some(array.to_vec()))
        }
    }

    fn request_to_promise(request: &IdbRequest) -> Promise {
        Promise::new(&mut |resolve, reject| {
            let resolve_clone = resolve.clone();
            let reject_clone = reject.clone();

            let success = Closure::once(Box::new(move |_: web_sys::Event| {
                let result = request.result().unwrap_or(JsValue::UNDEFINED);
                resolve_clone.call1(&JsValue::NULL, &result).unwrap();
            }) as Box<dyn FnOnce(web_sys::Event)>);

            let error = Closure::once(Box::new(move |event: web_sys::Event| {
                reject_clone.call1(&JsValue::NULL, &event).unwrap();
            }) as Box<dyn FnOnce(web_sys::Event)>);

            request.set_onsuccess(Some(success.as_ref().unchecked_ref()));
            request.set_onerror(Some(error.as_ref().unchecked_ref()));

            success.forget();
            error.forget();
        })
    }

    fn transaction_to_promise(tx: &IdbTransaction) -> Promise {
        Promise::new(&mut |resolve, reject| {
            let resolve_clone = resolve.clone();
            let reject_clone = reject.clone();

            let complete = Closure::once(Box::new(move |_: web_sys::Event| {
                resolve_clone.call0(&JsValue::NULL).unwrap();
            }) as Box<dyn FnOnce(web_sys::Event)>);

            let error = Closure::once(Box::new(move |event: web_sys::Event| {
                reject_clone.call1(&JsValue::NULL, &event).unwrap();
            }) as Box<dyn FnOnce(web_sys::Event)>);

            tx.set_oncomplete(Some(complete.as_ref().unchecked_ref()));
            tx.set_onerror(Some(error.as_ref().unchecked_ref()));

            complete.forget();
            error.forget();
        })
    }
}

impl Default for IndexedDbStorage {
    fn default() -> Self {
        Self::new()
    }
}
```

## JavaScript Integration

```html
<!-- www/index.html -->
<!DOCTYPE html>
<html lang="en">
<head>
    <meta charset="UTF-8">
    <meta name="viewport" content="width=device-width, initial-scale=1.0">
    <title>RustyNES</title>
    <link rel="stylesheet" href="style.css">
</head>
<body>
    <div id="app">
        <header>
            <h1>RustyNES</h1>
            <div id="controls">
                <input type="file" id="rom-input" accept=".nes,.NES" style="display: none;">
                <button id="load-btn">Load ROM</button>
                <button id="start-btn" disabled>Start</button>
                <button id="pause-btn" disabled>Pause</button>
                <button id="reset-btn" disabled>Reset</button>
            </div>
        </header>

        <main>
            <div id="canvas-container">
                <canvas id="nes-canvas" width="512" height="480"></canvas>
                <div id="drop-zone">Drop ROM here</div>
            </div>

            <aside id="info-panel">
                <div id="rom-info">No ROM loaded</div>
                <div id="fps-display">FPS: --</div>
                <div id="frame-count">Frame: 0</div>
            </aside>
        </main>

        <footer>
            <div id="status">Ready</div>
        </footer>
    </div>

    <script type="module" src="index.js"></script>
</body>
</html>
```

```javascript
// www/index.js
import init, { create_emulator, version, webgl_available } from './pkg/rustynes_web.js';

let emulator = null;
let animationId = null;
let storage = null;
let currentRomHash = null;

async function main() {
    // Initialize WASM module
    await init();
    console.log(`RustyNES v${version()} initialized`);
    console.log(`WebGL available: ${webgl_available()}`);

    // Create emulator instance
    emulator = create_emulator();
    await emulator.init('nes-canvas');

    // Initialize storage
    storage = new IndexedDbStorage();
    await storage.init();

    setupUI();
    setupDragDrop();
}

function setupUI() {
    const loadBtn = document.getElementById('load-btn');
    const romInput = document.getElementById('rom-input');
    const startBtn = document.getElementById('start-btn');
    const pauseBtn = document.getElementById('pause-btn');
    const resetBtn = document.getElementById('reset-btn');

    loadBtn.addEventListener('click', () => romInput.click());

    romInput.addEventListener('change', async (e) => {
        const file = e.target.files[0];
        if (file) {
            await loadRomFile(file);
        }
    });

    startBtn.addEventListener('click', () => {
        emulator.start();
        startEmulationLoop();
        startBtn.disabled = true;
        pauseBtn.disabled = false;
    });

    pauseBtn.addEventListener('click', () => {
        emulator.pause();
        stopEmulationLoop();
        startBtn.disabled = false;
        pauseBtn.disabled = true;
    });

    resetBtn.addEventListener('click', () => {
        emulator.reset();
        document.getElementById('frame-count').textContent = 'Frame: 0';
    });

    // Keyboard shortcuts
    document.addEventListener('keydown', (e) => {
        if (e.key === 'F1') {
            e.preventDefault();
            saveState(1);
        } else if (e.key === 'F4') {
            e.preventDefault();
            loadState(1);
        } else if (e.key === 'Escape') {
            if (emulator.is_running()) {
                emulator.pause();
                stopEmulationLoop();
                startBtn.disabled = false;
                pauseBtn.disabled = true;
            }
        }
    });
}

function setupDragDrop() {
    const dropZone = document.getElementById('drop-zone');
    const container = document.getElementById('canvas-container');

    container.addEventListener('dragover', (e) => {
        e.preventDefault();
        dropZone.style.display = 'flex';
    });

    container.addEventListener('dragleave', () => {
        dropZone.style.display = 'none';
    });

    container.addEventListener('drop', async (e) => {
        e.preventDefault();
        dropZone.style.display = 'none';

        const file = e.dataTransfer.files[0];
        if (file && (file.name.endsWith('.nes') || file.name.endsWith('.NES'))) {
            await loadRomFile(file);
        }
    });
}

async function loadRomFile(file) {
    try {
        setStatus('Loading ROM...');

        const arrayBuffer = await file.arrayBuffer();
        const data = new Uint8Array(arrayBuffer);

        const romName = emulator.load_rom(data);
        currentRomHash = await hashRom(data);

        // Load SRAM if available
        const sram = await storage.load_sram(currentRomHash);
        if (sram) {
            emulator.set_sram(sram);
            console.log('Loaded SRAM');
        }

        document.getElementById('rom-info').textContent = romName;
        document.getElementById('start-btn').disabled = false;
        document.getElementById('reset-btn').disabled = false;

        setStatus(`Loaded: ${romName}`);
    } catch (error) {
        setStatus(`Error: ${error}`);
        console.error(error);
    }
}

function startEmulationLoop() {
    let lastTime = 0;

    function frame(timestamp) {
        if (!emulator.is_running()) {
            return;
        }

        // Run frame
        emulator.run_frame();
        emulator.update_timing(timestamp);

        // Update UI
        document.getElementById('fps-display').textContent =
            `FPS: ${emulator.fps().toFixed(1)}`;
        document.getElementById('frame-count').textContent =
            `Frame: ${emulator.frame_count()}`;

        // Poll gamepads
        emulator.poll_gamepads?.();

        animationId = requestAnimationFrame(frame);
    }

    animationId = requestAnimationFrame(frame);
}

function stopEmulationLoop() {
    if (animationId) {
        cancelAnimationFrame(animationId);
        animationId = null;
    }
}

async function saveState(slot) {
    if (!currentRomHash) return;

    const state = emulator.save_state();
    if (state) {
        await storage.save_state(currentRomHash, slot, state);
        setStatus(`Saved state to slot ${slot}`);
    }
}

async function loadState(slot) {
    if (!currentRomHash) return;

    const state = await storage.load_state(currentRomHash, slot);
    if (state) {
        emulator.load_state(state);
        setStatus(`Loaded state from slot ${slot}`);
    } else {
        setStatus(`No state in slot ${slot}`);
    }
}

async function hashRom(data) {
    const hashBuffer = await crypto.subtle.digest('SHA-256', data);
    const hashArray = Array.from(new Uint8Array(hashBuffer));
    return hashArray.map(b => b.toString(16).padStart(2, '0')).join('');
}

function setStatus(message) {
    document.getElementById('status').textContent = message;
}

// Auto-save SRAM periodically
setInterval(async () => {
    if (currentRomHash && emulator) {
        const sram = emulator.get_sram();
        if (sram) {
            await storage.save_sram(currentRomHash, sram);
        }
    }
}, 60000); // Every 60 seconds

main().catch(console.error);
```

## Build Process

```bash
#!/bin/bash
# build.sh - Build script for WASM target

set -e

# Install wasm-pack if not present
if ! command -v wasm-pack &> /dev/null; then
    echo "Installing wasm-pack..."
    cargo install wasm-pack
fi

# Build WASM package
echo "Building WASM..."
wasm-pack build crates/rustynes-web --target web --release

# Copy to www directory
cp -r crates/rustynes-web/pkg crates/rustynes-web/www/

# Optimize WASM binary
if command -v wasm-opt &> /dev/null; then
    echo "Optimizing WASM..."
    wasm-opt -O3 \
        crates/rustynes-web/www/pkg/rustynes_web_bg.wasm \
        -o crates/rustynes-web/www/pkg/rustynes_web_bg.wasm
fi

echo "Build complete! Serve from crates/rustynes-web/www/"
```

## Development Server

```javascript
// serve.js - Simple development server
const http = require('http');
const fs = require('fs');
const path = require('path');

const PORT = 8080;
const WWW_DIR = path.join(__dirname, 'www');

const MIME_TYPES = {
    '.html': 'text/html',
    '.js': 'application/javascript',
    '.wasm': 'application/wasm',
    '.css': 'text/css',
    '.json': 'application/json',
    '.png': 'image/png',
};

const server = http.createServer((req, res) => {
    let filePath = path.join(WWW_DIR, req.url === '/' ? 'index.html' : req.url);
    const ext = path.extname(filePath);

    fs.readFile(filePath, (err, content) => {
        if (err) {
            res.writeHead(404);
            res.end('Not found');
            return;
        }

        // Required headers for WASM
        res.setHeader('Cross-Origin-Opener-Policy', 'same-origin');
        res.setHeader('Cross-Origin-Embedder-Policy', 'require-corp');

        const contentType = MIME_TYPES[ext] || 'application/octet-stream';
        res.writeHead(200, { 'Content-Type': contentType });
        res.end(content);
    });
});

server.listen(PORT, () => {
    console.log(`Server running at http://localhost:${PORT}/`);
});
```

## Performance Optimization

### WASM Size Reduction

```toml
# Cargo.toml - Release profile for minimal size
[profile.release]
opt-level = "z"       # Optimize for size
lto = true            # Link-time optimization
codegen-units = 1     # Single codegen unit
panic = "abort"       # No unwinding
strip = true          # Strip symbols

[profile.release.package."*"]
opt-level = "z"
```

### Frame Timing

```rust
/// Accurate frame timing using requestAnimationFrame
pub struct FrameTimer {
    target_fps: f64,
    frame_duration: f64,
    last_frame: f64,
    accumulated: f64,
}

impl FrameTimer {
    pub fn new(target_fps: f64) -> Self {
        Self {
            target_fps,
            frame_duration: 1000.0 / target_fps,
            last_frame: 0.0,
            accumulated: 0.0,
        }
    }

    /// Returns number of frames to run this tick
    pub fn tick(&mut self, timestamp: f64) -> u32 {
        if self.last_frame == 0.0 {
            self.last_frame = timestamp;
            return 1;
        }

        let delta = timestamp - self.last_frame;
        self.last_frame = timestamp;
        self.accumulated += delta;

        let frames = (self.accumulated / self.frame_duration) as u32;
        self.accumulated -= frames as f64 * self.frame_duration;

        // Cap to prevent spiral of death
        frames.min(4)
    }
}
```

## Implementation Checklist

### Core
- [ ] WASM entry point and init
- [ ] Emulator wrapper with JS API
- [ ] ROM loading (Uint8Array, URL, file drop)
- [ ] Frame running loop

### Video
- [ ] Canvas 2D rendering
- [ ] WebGL rendering (optional)
- [ ] Integer scaling
- [ ] Scanline filter

### Audio
- [ ] Web Audio API integration
- [ ] Sample buffering
- [ ] Volume control
- [ ] Audio resume on interaction

### Input
- [ ] Keyboard handlers
- [ ] Gamepad API support
- [ ] Configurable key bindings
- [ ] Two-player support

### Storage
- [ ] IndexedDB initialization
- [ ] SRAM persistence
- [ ] Save states
- [ ] Settings storage

### UI
- [ ] Load ROM button
- [ ] Start/Pause/Reset controls
- [ ] FPS display
- [ ] Drag-and-drop support
- [ ] Keyboard shortcuts

### Build
- [ ] wasm-pack build script
- [ ] wasm-opt optimization
- [ ] Development server
- [ ] Production deployment

## Browser Compatibility

| Feature | Chrome | Firefox | Safari | Edge |
|---------|--------|---------|--------|------|
| WebAssembly | 57+ | 52+ | 11+ | 16+ |
| Web Audio | Yes | Yes | Yes | Yes |
| Gamepad API | Yes | Yes | Yes | Yes |
| IndexedDB | Yes | Yes | Yes | Yes |
| WebGL 2.0 | 56+ | 51+ | 15+ | 79+ |
| SharedArrayBuffer* | 68+ | 79+ | 15.2+ | 79+ |

*Required for multithreaded WASM (optional)

## References

- [wasm-bindgen documentation](https://rustwasm.github.io/docs/wasm-bindgen/)
- [wasm-pack documentation](https://rustwasm.github.io/docs/wasm-pack/)
- [Web Audio API](https://developer.mozilla.org/en-US/docs/Web/API/Web_Audio_API)
- [Gamepad API](https://developer.mozilla.org/en-US/docs/Web/API/Gamepad_API)
- [IndexedDB API](https://developer.mozilla.org/en-US/docs/Web/API/IndexedDB_API)
- [WebGL 2.0 specification](https://www.khronos.org/registry/webgl/specs/latest/2.0/)
