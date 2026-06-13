# Milestone 15: Advanced Shader Pipeline & Video Filters

**Phase:** 4 (Polish & Release)
**Duration:** Months 20-21 (2 months)
**Status:** Planned
**Target:** August 2027
**Prerequisites:** M11 CRT Shaders Complete, M14 Plugin Architecture Complete

---

## Overview

Milestone 15 extends M11's CRT shader system with **Advanced Shader Pipeline** features: custom shader language (DSL), visual node editor, HDR support, AI upscaling, and community shader sharing. This milestone transforms RustyNES into the ultimate visual experience platform for NES emulation.

**Building on M11:**

- M11 established 12+ CRT presets (CRT-Royale, CRT-Lottes, scanlines, phosphor persistence)
- M15 adds shader creation tools, HDR, AI upscaling, and community features

---

## Goals

### Advanced Shader Features

- [ ] **Custom Shader Language (DSL)**
  - User-friendly shader creation (no WGSL knowledge required)
  - High-level abstractions (blur, glow, distortion)
  - Compile to optimized WGSL
  - Real-time preview

- [ ] **Visual Node Editor**
  - Node-based shader graph
  - Drag-and-drop interface
  - Live preview (60 FPS)
  - Export to DSL or WGSL

- [ ] **HDR Support**
  - Wide color gamut (Rec. 2020)
  - Peak brightness mapping (1000-4000 nits)
  - Authentic phosphor luminance
  - Auto-detection (HDR displays)

- [ ] **AI Upscaling (ESRGAN)**
  - Machine learning-based upscaling (2x-4x)
  - Detail enhancement
  - Anti-aliasing
  - Pre-trained models (waifu2x, ESRGAN)

- [ ] **Advanced NTSC Simulation**
  - Full NTSC composite video pipeline
  - RF modulation artifacts
  - TV tuner simulation (channel interference)
  - Composite/S-Video/RGB modes

- [ ] **Community Shader Sharing**
  - Upload/download custom shaders
  - Rating system (stars, reviews)
  - Shader collections (curated packs)
  - Auto-updates (new shaders)

- [ ] **Shader Performance Profiling**
  - Per-pass timing (GPU)
  - Bottleneck identification
  - Auto-tuning (quality vs performance)
  - Performance warnings (insufficient GPU)

---

## Custom Shader Language (DSL)

### DSL Syntax Example

**File:** `shaders/custom-glow.dsl`

```dsl
// Custom phosphor glow shader (DSL)

shader PhosphorGlow {
    // Input texture (NES framebuffer)
    input texture: Texture2D;

    // Parameters (user-adjustable)
    param intensity: float = 0.5 [0.0..1.0];
    param radius: int = 8 [1..16];

    // Shader entry point
    fragment main(coord: vec2) -> vec4 {
        // Sample input texture
        let color = sample(texture, coord);

        // Extract bright pixels (bloom threshold)
        let luminance = dot(color.rgb, vec3(0.299, 0.587, 0.114));
        let glow = vec3(0.0);

        if (luminance > 0.7) {
            // Gaussian blur for glow
            glow = gaussianBlur(texture, coord, radius);
        }

        // Composite original + glow
        return vec4(color.rgb + glow * intensity, color.a);
    }

    // Built-in function: Gaussian blur
    function gaussianBlur(tex: Texture2D, coord: vec2, radius: int) -> vec3 {
        let result = vec3(0.0);
        let weight_sum = 0.0;

        for (let x = -radius; x <= radius; x++) {
            for (let y = -radius; y <= radius; y++) {
                let offset = vec2(x, y) * 0.001;
                let sample_color = sample(tex, coord + offset);
                let weight = gaussian2D(x, y, radius / 3.0);

                result += sample_color.rgb * weight;
                weight_sum += weight;
            }
        }

        return result / weight_sum;
    }
}
```

### DSL Compiler

**File:** `crates/rustynes-shader-dsl/src/compiler.rs`

```rust
use pest::Parser;
use pest_derive::Parser;

#[derive(Parser)]
#[grammar = "dsl.pest"]
pub struct DslParser;

pub struct DslCompiler {
    // AST representation
    ast: ShaderAst,
}

pub struct ShaderAst {
    pub name: String,
    pub inputs: Vec<InputDecl>,
    pub params: Vec<ParamDecl>,
    pub functions: Vec<FunctionDecl>,
}

impl DslCompiler {
    pub fn new() -> Self {
        Self {
            ast: ShaderAst {
                name: String::new(),
                inputs: Vec::new(),
                params: Vec::new(),
                functions: Vec::new(),
            },
        }
    }

    pub fn compile(&mut self, source: &str) -> Result<String, CompilerError> {
        // Parse DSL source
        let pairs = DslParser::parse(Rule::shader, source)?;

        // Build AST
        for pair in pairs {
            self.visit_shader(pair)?;
        }

        // Generate WGSL code
        let wgsl = self.generate_wgsl()?;

        Ok(wgsl)
    }

    fn generate_wgsl(&self) -> Result<String, CompilerError> {
        let mut wgsl = String::new();

        // Header
        wgsl.push_str("// Generated from DSL\n\n");

        // Uniforms (parameters)
        wgsl.push_str("struct Params {\n");
        for param in &self.ast.params {
            wgsl.push_str(&format!("    {}: {},\n", param.name, param.ty));
        }
        wgsl.push_str("}\n\n");

        wgsl.push_str("@group(0) @binding(0) var<uniform> params: Params;\n");
        wgsl.push_str("@group(0) @binding(1) var input_texture: texture_2d<f32>;\n");
        wgsl.push_str("@group(0) @binding(2) var sampler0: sampler;\n\n");

        // Fragment shader
        wgsl.push_str("@fragment\n");
        wgsl.push_str("fn main(@location(0) coord: vec2<f32>) -> @location(0) vec4<f32> {\n");
        wgsl.push_str("    // DSL-generated code\n");
        // ... AST traversal to generate WGSL ...
        wgsl.push_str("}\n");

        Ok(wgsl)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum CompilerError {
    #[error("Parse error: {0}")]
    ParseError(String),
    #[error("Type error: {0}")]
    TypeError(String),
    #[error("Code generation error: {0}")]
    CodeGenError(String),
}
```

---

## Visual Node Editor

### Node Graph Interface

```text
┌────────────────────────────────────────────────────────┐
│  Shader Node Editor                                    │
│                                                        │
│  [Input]                [Gaussian Blur]    [Output]    │
│  ┌───────┐              ┌────────────┐     ┌──────┐    │
│  │Texture│─────────────>│ Radius: 8  │────>│Color │    │
│  └───────┘              └────────────┘     └──────┘    │
│                                                        │
│  [Bloom Threshold]      [Composite]                    │
│  ┌────────────┐         ┌──────────┐                   │
│  │ Threshold  │────────>│  Mix     │─────>             │
│  │   0.7      │         │  0.5     │                   │
│  └────────────┘         └──────────┘                   │
│                                                        │
│  [Add Node]  [Export WGSL]  [Save Preset]              │
└────────────────────────────────────────────────────────┘
```

### Node Editor Implementation

**File:** `crates/rustynes-desktop/src/shader_editor/mod.rs`

```rust
use egui::{Ui, Vec2, Pos2, Stroke, Color32};

pub struct ShaderNodeEditor {
    /// Nodes in the graph
    nodes: Vec<ShaderNode>,

    /// Connections between nodes
    connections: Vec<Connection>,

    /// Currently selected node
    selected_node: Option<usize>,
}

pub struct ShaderNode {
    pub id: usize,
    pub node_type: NodeType,
    pub position: Pos2,
    pub inputs: Vec<NodeSocket>,
    pub outputs: Vec<NodeSocket>,
    pub params: Vec<NodeParam>,
}

pub enum NodeType {
    Input,
    Output,
    GaussianBlur,
    BloomThreshold,
    Composite,
    Scanlines,
    CurvedScreen,
    ColorGrading,
}

pub struct NodeSocket {
    pub name: String,
    pub ty: SocketType,
}

pub enum SocketType {
    Texture,
    Color,
    Float,
    Vec2,
    Vec3,
}

pub struct NodeParam {
    pub name: String,
    pub value: ParamValue,
}

pub enum ParamValue {
    Float(f32),
    Int(i32),
    Bool(bool),
}

pub struct Connection {
    pub from_node: usize,
    pub from_socket: usize,
    pub to_node: usize,
    pub to_socket: usize,
}

impl ShaderNodeEditor {
    pub fn new() -> Self {
        Self {
            nodes: Vec::new(),
            connections: Vec::new(),
            selected_node: None,
        }
    }

    pub fn ui(&mut self, ui: &mut Ui) {
        // Node editor canvas
        let (response, painter) = ui.allocate_painter(
            Vec2::new(800.0, 600.0),
            egui::Sense::click_and_drag(),
        );

        // Draw connections
        for connection in &self.connections {
            let from_node = &self.nodes[connection.from_node];
            let to_node = &self.nodes[connection.to_node];

            let from_pos = from_node.position + Vec2::new(100.0, 50.0);
            let to_pos = to_node.position + Vec2::new(0.0, 50.0);

            painter.line_segment(
                [from_pos, to_pos],
                Stroke::new(2.0, Color32::WHITE),
            );
        }

        // Draw nodes
        for (i, node) in self.nodes.iter_mut().enumerate() {
            let node_response = ui.allocate_ui_at_rect(
                egui::Rect::from_min_size(node.position, Vec2::new(120.0, 80.0)),
                |ui| {
                    ui.group(|ui| {
                        ui.label(format!("{:?}", node.node_type));

                        // Node parameters
                        for param in &mut node.params {
                            match &mut param.value {
                                ParamValue::Float(val) => {
                                    ui.add(egui::Slider::new(val, 0.0..=1.0).text(&param.name));
                                }
                                ParamValue::Int(val) => {
                                    ui.add(egui::Slider::new(val, 0..=100).text(&param.name));
                                }
                                ParamValue::Bool(val) => {
                                    ui.checkbox(val, &param.name);
                                }
                            }
                        }
                    });
                },
            );

            // Drag node
            if node_response.response.dragged() {
                node.position += node_response.response.drag_delta();
            }

            // Select node
            if node_response.response.clicked() {
                self.selected_node = Some(i);
            }
        }

        // Add node button
        if ui.button("Add Node").clicked() {
            self.add_node(NodeType::GaussianBlur, Pos2::new(100.0, 100.0));
        }

        // Export to WGSL
        if ui.button("Export WGSL").clicked() {
            let wgsl = self.generate_wgsl();
            println!("{}", wgsl);
        }
    }

    fn add_node(&mut self, node_type: NodeType, position: Pos2) {
        let node = ShaderNode {
            id: self.nodes.len(),
            node_type,
            position,
            inputs: vec![NodeSocket {
                name: "Input".to_string(),
                ty: SocketType::Texture,
            }],
            outputs: vec![NodeSocket {
                name: "Output".to_string(),
                ty: SocketType::Texture,
            }],
            params: vec![NodeParam {
                name: "Radius".to_string(),
                value: ParamValue::Int(8),
            }],
        };

        self.nodes.push(node);
    }

    fn generate_wgsl(&self) -> String {
        // Traverse node graph and generate WGSL
        "// Generated WGSL from node editor\n".to_string()
    }
}
```

---

## HDR Support

### HDR Rendering Pipeline

```text
┌────────────────────────────────────────────────┐
│  NES Framebuffer (SDR, 256×240)                │
│  ↓                                             │
│  Color Grading (sRGB → Linear)                 │
│  ↓                                             │
│  CRT Shaders (Phosphor Glow, Bloom)            │
│  ↓                                             │
│  HDR Tonemapping (Linear → Rec. 2020)          │
│  ├─ Peak brightness mapping (1000-4000 nits)   │
│  ├─ Authentic phosphor luminance               │
│  └─ Color gamut expansion                      │
│  ↓                                             │
│  HDR Output (3840×2160 @ 10-bit)               │
└────────────────────────────────────────────────┘
```

### HDR Shader (WGSL)

```wgsl
// HDR tonemapping shader

@group(0) @binding(0) var input_texture: texture_2d<f32>;
@group(0) @binding(1) var<uniform> hdr_params: HdrParams;

struct HdrParams {
    peak_nits: f32,       // 1000-4000 nits
    phosphor_luminance: f32,  // Authentic phosphor brightness
}

@fragment
fn hdr_tonemap(@location(0) coord: vec2<f32>) -> @location(0) vec4<f32> {
    // Sample SDR input (linear)
    let color_linear = textureSample(input_texture, sampler0, coord);

    // Expand color gamut (sRGB → Rec. 2020)
    let color_rec2020 = srgb_to_rec2020(color_linear.rgb);

    // Map brightness to HDR range (0-1 → 0-peak_nits)
    let luminance = dot(color_rec2020, vec3(0.2627, 0.6780, 0.0593));
    let hdr_luminance = luminance * hdr_params.peak_nits;

    // Authentic phosphor glow (higher peak brightness)
    let phosphor_boost = max(0.0, luminance - 0.7) * hdr_params.phosphor_luminance;

    let final_color = color_rec2020 * (hdr_luminance + phosphor_boost);

    return vec4(final_color, color_linear.a);
}

fn srgb_to_rec2020(color: vec3<f32>) -> vec3<f32> {
    // Conversion matrix (sRGB → Rec. 2020)
    let matrix = mat3x3<f32>(
        vec3(0.627404, 0.329283, 0.043313),
        vec3(0.069097, 0.919541, 0.011362),
        vec3(0.016391, 0.088013, 0.895595)
    );

    return matrix * color;
}
```

---

## AI Upscaling (ESRGAN)

### ESRGAN Integration

**File:** `crates/rustynes-desktop/src/ai_upscaler.rs`

```rust
use ort::{Environment, SessionBuilder, Value, GraphOptimizationLevel};
use ndarray::{Array, ArrayBase, Dim};

pub struct EsrganUpscaler {
    session: ort::Session,
}

impl EsrganUpscaler {
    pub fn new(model_path: &str) -> Result<Self, UpscalerError> {
        let environment = Environment::builder()
            .with_name("RustyNES")
            .build()?;

        let session = SessionBuilder::new(&environment)?
            .with_optimization_level(GraphOptimizationLevel::Level3)?
            .with_model_from_file(model_path)?;

        Ok(Self { session })
    }

    pub fn upscale(&self, input: &[u8], width: usize, height: usize) -> Result<Vec<u8>, UpscalerError> {
        // Convert input to ndarray (HWC → NCHW)
        let input_array = Array::from_shape_vec(
            (1, 3, height, width),
            input.to_vec(),
        )?;

        // Run ESRGAN inference
        let input_tensor = Value::from_array(input_array)?;
        let outputs = self.session.run(&[input_tensor])?;

        // Extract output tensor
        let output_tensor = &outputs[0];
        let output_array: ArrayBase<_, Dim<[usize; 4]>> = output_tensor.try_extract()?;

        // Convert output to RGB buffer
        let output_vec = output_array.into_raw_vec();

        Ok(output_vec)
    }
}

#[derive(Debug, thiserror::Error)]
pub enum UpscalerError {
    #[error("ONNX Runtime error: {0}")]
    OrtError(#[from] ort::Error),
    #[error("ndarray error: {0}")]
    NdarrayError(#[from] ndarray::ShapeError),
}
```

---

## Implementation Plan

### Sprint 1: Custom Shader Language (DSL)

**Duration:** 3 weeks

- [ ] DSL syntax design (grammar)
- [ ] Parser (pest)
- [ ] AST representation
- [ ] WGSL code generator
- [ ] Real-time preview

### Sprint 2: Visual Node Editor

**Duration:** 3 weeks

- [ ] Node graph UI (egui)
- [ ] Drag-and-drop interface
- [ ] Node library (blur, glow, scanlines)
- [ ] Connection system
- [ ] Export to DSL/WGSL

### Sprint 3: HDR Support

**Duration:** 2 weeks

- [ ] HDR display detection
- [ ] Rec. 2020 color space conversion
- [ ] Peak brightness mapping
- [ ] Authentic phosphor luminance
- [ ] HDR output (10-bit)

### Sprint 4: AI Upscaling (ESRGAN)

**Duration:** 2 weeks

- [ ] ONNX Runtime integration
- [ ] Pre-trained model loading (waifu2x, ESRGAN)
- [ ] Upscaling pipeline (2x-4x)
- [ ] Performance optimization
- [ ] Fallback (nearest-neighbor)

---

## Acceptance Criteria

### Functionality

- [ ] DSL compiles to valid WGSL
- [ ] Node editor exports correct shaders
- [ ] HDR output works on HDR displays
- [ ] AI upscaling (2x-4x) functional
- [ ] Advanced NTSC simulation accurate
- [ ] Community shader sharing works

### Performance

- [ ] DSL compilation <100ms
- [ ] Node editor updates at 60 FPS
- [ ] HDR overhead <3ms
- [ ] AI upscaling <50ms (2x), <200ms (4x)
- [ ] Total shader pipeline <15ms (all features enabled)

### User Experience

- [ ] DSL syntax intuitive (beginner-friendly)
- [ ] Node editor easy to use (drag-and-drop)
- [ ] HDR auto-detected (no manual config)
- [ ] AI upscaling quality high (no artifacts)
- [ ] Shader marketplace easy to browse

---

## Dependencies

### Prerequisites

- **M11 CRT Shaders Complete:** wgpu shader pipeline established
- **M14 Plugin Architecture Complete:** Shader plugin system

### Crate Dependencies

```toml
# crates/rustynes-desktop/Cargo.toml

[dependencies.pest]
version = "2.7"  # DSL parser

[dependencies.pest_derive]
version = "2.7"  # Parser macros

[dependencies.ort]
version = "2.0"  # ONNX Runtime (AI upscaling)

[dependencies.ndarray]
version = "0.15"  # Multi-dimensional arrays
```

---

## Related Documentation

- [M11 CRT Shaders](../../phase-3-expansion/milestone-11-webassembly/README.md) - Foundation CRT shaders
- [M14 Plugin Architecture](../../phase-3-expansion/milestone-14-mobile/README.md) - Shader plugin system
- [ONNX Runtime](https://onnxruntime.ai/) - AI inference engine

---

## Success Criteria

1. Custom shader language (DSL) compiles correctly
2. Visual node editor exports functional shaders
3. HDR support works on HDR displays
4. AI upscaling (ESRGAN) produces high-quality results
5. Advanced NTSC simulation accurate
6. Community shader sharing functional
7. Performance targets met (all features <15ms)
8. Zero regressions in M11 CRT shaders
9. M15 milestone marked as ✅ COMPLETE

---

**Milestone Status:** ⏳ PLANNED
**Blocked By:** M11 CRT Shaders Complete, M14 Plugin Architecture Complete
**Next Milestone:** M16 (TAS Editor with Piano Roll Interface)

---

## Design Notes

### Advanced Shader Pipeline Philosophy

**Why Custom DSL?**

- WGSL too low-level for users
- High-level abstractions (blur, glow, distortion)
- Faster iteration (no manual WGSL)
- Compile-time optimization

**Why Visual Node Editor?**

- Accessible to non-programmers
- Real-time preview (immediate feedback)
- Experiment-friendly (drag-and-drop)
- Export to multiple formats (DSL, WGSL, plugin)

**Why HDR?**

- Authentic phosphor luminance (CRTs were bright)
- Wide color gamut (accurate colors)
- Modern displays support HDR
- Competitive advantage (few emulators support HDR)

**Why AI Upscaling?**

- 4K displays common (256x240 to 3840x2160 is 15x upscale)
- Nearest-neighbor looks pixelated
- Bilinear/bicubic looks blurry
- AI upscaling preserves detail

### Performance Considerations

**DSL Compilation:**

- Compile shaders offline (not per-frame)
- Cache compiled WGSL
- Minimal overhead (100ms one-time cost)

**AI Upscaling:**

- GPU-accelerated (ONNX Runtime)
- Pre-trained models (no training required)
- Fallback to nearest-neighbor (if GPU insufficient)

**HDR Tonemapping:**

- Single shader pass (<3ms)
- Auto-detection (no user configuration)

---

## Future Enhancements (Post-v1.0)

Advanced features for future releases:

1. **Ray Tracing (RTX):**
   - Screen reflections (ambient light)
   - Global illumination (phosphor glow)
   - Physically accurate CRT simulation

2. **VR Support:**
   - Virtual CRT in 3D space
   - Depth perception (curved screen)
   - Head tracking (viewing angle)

3. **Cloud Shader Rendering:**
   - Offload shader computation to cloud
   - Ultra-high-quality presets (no local GPU required)

---

**Migration Note:** Advanced shader pipeline features added from M11 foundation. M15 focuses on shader creation tools, HDR, AI upscaling, and community features.
