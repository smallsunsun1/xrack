//! Shared sRGB irradiance atlases. The opaque pass can reuse the transmission
//! texture and storage slots because the two inputs are used in separate draws.
use std::collections::{HashMap, VecDeque};

use xrack_runtime::{ImageAsset, ImageIdentity, scene3d::BakedLightmap};

const RESIDENT_ATLASES: usize = 12;
const RESIDENT_BYTES: u64 = 64 * 1024 * 1024;

struct ResidentAtlas {
    identity: ImageIdentity,
    view: wgpu::TextureView,
    bytes: u64,
}

#[derive(Default)]
pub(super) struct LightmapTextures {
    entries: VecDeque<ResidentAtlas>,
    sampler: Option<wgpu::Sampler>,
    pub uploads: u64,
}

impl LightmapTextures {
    pub fn sampler(&mut self, device: &wgpu::Device) -> wgpu::Sampler {
        self.sampler
            .get_or_insert_with(|| {
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("XRack lightmap atlas sampler"),
                    mag_filter: wgpu::FilterMode::Linear,
                    min_filter: wgpu::FilterMode::Linear,
                    ..Default::default()
                })
            })
            .clone()
    }

    pub fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        lightmap: &BakedLightmap,
    ) -> Option<wgpu::TextureView> {
        let id = lightmap.identity();
        if let Some(entry) = self
            .entries
            .iter()
            .position(|entry| entry.identity == id)
            .and_then(|index| self.entries.remove(index))
        {
            let view = entry.view.clone();
            self.entries.push_front(entry);
            return Some(view);
        }
        let image = ImageAsset::from(lightmap.image().clone());
        let pixels = image.pixels()?;
        let [width, height] = pixels.size;
        let limit = device.limits().max_texture_dimension_2d;
        if width > limit || height > limit {
            return None;
        }
        let source = xrack_assets::decode_png(pixels.bytes).ok()?;
        if source.size != pixels.size {
            return None;
        }
        let extent = wgpu::Extent3d {
            width,
            height,
            depth_or_array_layers: 1,
        };
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack baked irradiance atlas"),
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
            &source.rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            extent,
        );
        let view = texture.create_view(&Default::default());
        let bytes = u64::from(width) * u64::from(height) * 4;
        let mut resident: u64 = self.entries.iter().map(|entry| entry.bytes).sum();
        while self.entries.len() >= RESIDENT_ATLASES || resident + bytes > RESIDENT_BYTES {
            let Some(evicted) = self.entries.pop_back() else {
                break;
            };
            resident -= evicted.bytes;
        }
        self.entries.push_front(ResidentAtlas {
            identity: id,
            view: view.clone(),
            bytes,
        });
        self.uploads = self.uploads.saturating_add(1);
        Some(view)
    }
}

pub(super) struct LightmapBindings<'a> {
    pub base: &'a wgpu::BindGroup,
    pub mapped: HashMap<(ImageIdentity, bool), wgpu::BindGroup>,
}
