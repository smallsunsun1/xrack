//! Resident sprite sheets and color tables shared by emitters and viewports.
use std::collections::VecDeque;

use xrack_runtime::{
    Color, Image, ImageAsset, ImageIdentity, ImagePixels,
    scene3d::{MAX_PARTICLE_EMITTERS, MAX_PARTICLE_TEXTURE_BYTES},
};

use super::linear_rgb;

const CACHE_BYTES: u64 = 2 * MAX_PARTICLE_TEXTURE_BYTES;

#[derive(Clone)]
pub(super) struct SpriteBinding {
    pub identity: ImageIdentity,
    pub size: [u32; 2],
    pub view: wgpu::TextureView,
}

struct CachedSprite {
    identity: ImageIdentity,
    binding: Option<SpriteBinding>,
    bytes: u64,
}

#[derive(Default)]
pub(super) struct SpriteTextures {
    entries: VecDeque<CachedSprite>,
    bytes: u64,
    pub uploads: u64,
}

impl SpriteTextures {
    pub fn get(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Image,
    ) -> Option<SpriteBinding> {
        let asset = ImageAsset::from(image.clone());
        let pixels = asset.pixels()?;
        let index = self
            .entries
            .iter()
            .position(|entry| entry.identity == pixels.identity);
        if let Some(entry) = index.and_then(|index| self.entries.remove(index)) {
            let binding = entry.binding.clone();
            self.entries.push_front(entry);
            return binding;
        }
        let binding = upload(device, queue, &pixels);
        let bytes = if binding.is_some() {
            self.uploads = self.uploads.saturating_add(1);
            pixels.size.into_iter().map(u64::from).product::<u64>() * 4
        } else {
            0
        };
        while self.bytes + bytes > CACHE_BYTES || self.entries.len() >= MAX_PARTICLE_EMITTERS {
            if let Some(entry) = self.entries.pop_back() {
                self.bytes -= entry.bytes;
            } else {
                break;
            }
        }
        self.bytes += bytes;
        self.entries.push_front(CachedSprite {
            identity: pixels.identity,
            binding: binding.clone(),
            bytes,
        });
        binding
    }
}

fn upload(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    pixels: &ImagePixels<'_>,
) -> Option<SpriteBinding> {
    let [width, height] = pixels.size;
    let limit = device.limits().max_texture_dimension_2d;
    if width > limit || height > limit {
        return None;
    }
    let mut raster = xrack_assets::decode_png(pixels.bytes).ok()?;
    if raster.size != pixels.size {
        return None;
    }
    // Blend and filter premultiplied linear color so transparent texels
    // contribute neither dark edges nor stray RGB to neighboring frames.
    for pixel in raster.rgba.as_chunks_mut::<4>().0 {
        match pixel[3] {
            0 => pixel[..3].fill(0),
            255 => {}
            opacity => {
                let a = f64::from(opacity) / 255.0;
                let rgb = linear_rgb(Color(u32::from_be_bytes(*pixel)));
                for (out, value) in pixel[..3].iter_mut().zip(rgb) {
                    let value = value * a;
                    let encoded = if value <= 0.003_130_8 {
                        12.92 * value
                    } else {
                        1.055 * value.powf(1.0 / 2.4) - 0.055
                    };
                    *out = (255.0 * encoded).round() as u8;
                }
            }
        }
    }
    let extent = wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("XRack particle sprite sheet"),
        size: extent,
        mip_level_count: 1,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8UnormSrgb,
        usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
        view_formats: &[],
    });
    queue.write_texture(
        texture.as_image_copy(),
        &raster.rgba,
        wgpu::TexelCopyBufferLayout {
            offset: 0,
            bytes_per_row: Some(width * 4),
            rows_per_image: Some(height),
        },
        extent,
    );
    Some(SpriteBinding {
        identity: pixels.identity,
        size: pixels.size,
        view: texture.create_view(&wgpu::TextureViewDescriptor::default()),
    })
}
