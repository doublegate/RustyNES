// NES framebuffer rendering shader
// Renders the 256×240 NES texture with nearest-neighbor filtering

struct VertexOutput {
    @builtin(position) position: vec4<f32>,
    @location(0) tex_coords: vec2<f32>,
};

@group(0) @binding(0)
var nes_texture: texture_2d<f32>;

@group(0) @binding(1)
var nes_sampler: sampler;

// Fullscreen triangle vertex shader
// Uses the "fullscreen triangle trick" - 3 vertices cover the entire screen
@vertex
fn vs_main(@builtin(vertex_index) vertex_index: u32) -> VertexOutput {
    var output: VertexOutput;

    // Generate fullscreen triangle positions
    // vertex 0: (-1, -1)
    // vertex 1: (3, -1)
    // vertex 2: (-1, 3)
    let x = f32((vertex_index << 1u) & 2u) - 1.0;
    let y = f32(vertex_index & 2u) - 1.0;

    output.position = vec4<f32>(x, -y, 0.0, 1.0);
    output.tex_coords = vec2<f32>((x + 1.0) * 0.5, (y + 1.0) * 0.5);

    return output;
}

// Fragment shader with nearest-neighbor sampling
@fragment
fn fs_main(input: VertexOutput) -> @location(0) vec4<f32> {
    return textureSample(nes_texture, nes_sampler, input.tex_coords);
}
