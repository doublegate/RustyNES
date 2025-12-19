//! NES framebuffer texture management.
//!
//! Handles the GPU texture for the NES framebuffer (256×240 resolution)
//! with efficient RGB to RGBA conversion and texture upload.

// Re-export wgpu types from iced to ensure version compatibility
use iced::widget::shader::wgpu;
use wgpu::{Device, Queue, Sampler, Texture, TextureFormat, TextureView};

/// NES framebuffer texture (256×240 RGB888 → RGBA8 GPU)
#[allow(dead_code)] // Will be used for custom wgpu rendering pipeline
pub struct NesTexture {
    texture: Texture,
    view: TextureView,
    width: u32,
    height: u32,
    /// Pre-allocated RGBA buffer (avoid allocations in hot path)
    rgba_buffer: Vec<u8>,
}

#[allow(dead_code)] // Will be used for custom wgpu rendering pipeline
impl NesTexture {
    /// NES native resolution width
    pub const NES_WIDTH: u32 = 256;
    /// NES native resolution height
    pub const NES_HEIGHT: u32 = 240;

    /// Create a new NES texture
    pub fn new(device: &Device) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("NES Framebuffer"),
            size: wgpu::Extent3d {
                width: Self::NES_WIDTH,
                height: Self::NES_HEIGHT,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: TextureFormat::Rgba8UnormSrgb,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });

        let view = texture.create_view(&wgpu::TextureViewDescriptor::default());

        // Pre-allocate RGBA buffer to avoid allocations during texture updates
        let rgba_buffer = vec![0u8; (Self::NES_WIDTH * Self::NES_HEIGHT * 4) as usize];

        Self {
            texture,
            view,
            width: Self::NES_WIDTH,
            height: Self::NES_HEIGHT,
            rgba_buffer,
        }
    }

    /// Upload NES framebuffer (RGB888) to GPU texture
    ///
    /// # Performance
    /// - Converts RGB to RGBA in-place using pre-allocated buffer
    /// - Target: <1ms upload time
    ///
    /// # Panics
    /// Panics if framebuffer size is not 256×240×3 bytes (184,320 bytes)
    pub fn update(&mut self, queue: &Queue, framebuffer: &[u8]) {
        assert_eq!(
            framebuffer.len(),
            (Self::NES_WIDTH * Self::NES_HEIGHT * 3) as usize,
            "Invalid framebuffer size: expected {} bytes, got {}",
            Self::NES_WIDTH * Self::NES_HEIGHT * 3,
            framebuffer.len()
        );

        // Convert RGB888 → RGBA8888 in-place (no allocations)
        for (i, chunk) in framebuffer.chunks_exact(3).enumerate() {
            let offset = i * 4;
            self.rgba_buffer[offset] = chunk[0]; // R
            self.rgba_buffer[offset + 1] = chunk[1]; // G
            self.rgba_buffer[offset + 2] = chunk[2]; // B
            self.rgba_buffer[offset + 3] = 255; // A (opaque)
        }

        queue.write_texture(
            wgpu::ImageCopyTexture {
                texture: &self.texture,
                mip_level: 0,
                origin: wgpu::Origin3d::ZERO,
                aspect: wgpu::TextureAspect::All,
            },
            &self.rgba_buffer,
            wgpu::ImageDataLayout {
                offset: 0,
                bytes_per_row: Some(Self::NES_WIDTH * 4),
                rows_per_image: Some(Self::NES_HEIGHT),
            },
            wgpu::Extent3d {
                width: Self::NES_WIDTH,
                height: Self::NES_HEIGHT,
                depth_or_array_layers: 1,
            },
        );
    }

    /// Get texture view for binding
    pub fn view(&self) -> &TextureView {
        &self.view
    }

    /// Get texture dimensions
    pub fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }
}

/// Nearest-neighbor sampler for pixel-perfect NES rendering
#[allow(dead_code)] // Will be used for custom wgpu rendering pipeline
pub struct NesSampler {
    sampler: Sampler,
}

#[allow(dead_code)] // Will be used for custom wgpu rendering pipeline
impl NesSampler {
    /// Create a new nearest-neighbor sampler
    pub fn new(device: &Device) -> Self {
        let sampler = device.create_sampler(&wgpu::SamplerDescriptor {
            label: Some("NES Sampler (Nearest)"),
            address_mode_u: wgpu::AddressMode::ClampToEdge,
            address_mode_v: wgpu::AddressMode::ClampToEdge,
            address_mode_w: wgpu::AddressMode::ClampToEdge,
            mag_filter: wgpu::FilterMode::Nearest, // Pixel-perfect scaling
            min_filter: wgpu::FilterMode::Nearest,
            mipmap_filter: wgpu::FilterMode::Nearest,
            ..Default::default()
        });

        Self { sampler }
    }

    /// Get sampler for binding
    pub fn sampler(&self) -> &Sampler {
        &self.sampler
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nes_dimensions() {
        assert_eq!(NesTexture::NES_WIDTH, 256);
        assert_eq!(NesTexture::NES_HEIGHT, 240);
    }

    #[test]
    fn test_framebuffer_size() {
        let expected_size = (NesTexture::NES_WIDTH * NesTexture::NES_HEIGHT * 3) as usize;
        assert_eq!(expected_size, 184_320);
    }
}
