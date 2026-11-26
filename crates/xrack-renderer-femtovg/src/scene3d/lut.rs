//! Display-referred color lookup tables stored as horizontal RGB strips.
use std::collections::VecDeque;

use xrack_assets::{color_lut_size, decode_png};
use xrack_runtime::{ImageAsset, ImageFormat, ImageIdentity, Node};

const MAX_CACHED_LUTS: usize = 4;

pub(super) struct LutSettings {
    asset: ImageAsset,
    pub size: u32,
    pub strength: f32,
}

impl LutSettings {
    pub fn from_node(node: &Node) -> Option<Self> {
        let strength = node
            .float("scene-lut-strength")
            .unwrap_or(1.0)
            .clamp(0.0, 1.0);
        if strength == 0.0 {
            return None;
        }
        let asset = node.image_asset("scene-lut")?;
        let pixels = asset.pixels()?;
        if pixels.format != ImageFormat::Png || pixels.edges.is_some() {
            return None;
        }
        let size = color_lut_size(pixels.size, node.int("scene-lut-size").unwrap_or(0))?;
        Some(Self {
            asset,
            size,
            strength,
        })
    }
}

pub(super) struct LutBinding {
    pub view: wgpu::TextureView,
    pub sampler: wgpu::Sampler,
    pub uploaded: bool,
}

struct CachedLut {
    key: (ImageIdentity, [u32; 2], usize),
    view: Option<wgpu::TextureView>,
}

pub(super) struct LutCache {
    textures: VecDeque<CachedLut>,
    sampler: wgpu::Sampler,
}

impl LutCache {
    pub fn new(device: &wgpu::Device) -> Self {
        Self {
            textures: VecDeque::new(),
            sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("XRack 3D color LUT sampler"),
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                ..Default::default()
            }),
        }
    }

    pub fn bind(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        settings: &LutSettings,
    ) -> Option<LutBinding> {
        self.image(device, queue, &settings.asset)
    }

    pub(super) fn image(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        asset: &ImageAsset,
    ) -> Option<LutBinding> {
        let pixels = asset.pixels()?;
        if pixels.format != ImageFormat::Png || pixels.edges.is_some() {
            return None;
        }
        let key = (pixels.identity, pixels.size, pixels.bytes.len());
        let index = self.textures.iter().position(|cached| cached.key == key);
        let cached = index.and_then(|index| self.textures.remove(index));
        let uploaded = cached.is_none();
        let texture = cached.unwrap_or_else(|| {
            let view = decode_png(pixels.bytes)
                .ok()
                .filter(|raster| raster.size == pixels.size)
                .map(|raster| {
                    let [width, height] = raster.size;
                    let size = wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    };
                    let texture = device.create_texture(&wgpu::TextureDescriptor {
                        label: Some("XRack 3D color LUT"),
                        size,
                        mip_level_count: 1,
                        sample_count: 1,
                        dimension: wgpu::TextureDimension::D2,
                        // Table entries are numeric sRGB signals. The shader owns the
                        // conversion at the display-composition boundary.
                        format: wgpu::TextureFormat::Rgba8Unorm,
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
                        size,
                    );
                    texture.create_view(&wgpu::TextureViewDescriptor::default())
                });
            CachedLut { key, view }
        });
        let binding = texture.view.as_ref().map(|view| LutBinding {
            view: view.clone(),
            sampler: self.sampler.clone(),
            uploaded,
        });
        self.textures.truncate(MAX_CACHED_LUTS - 1);
        self.textures.push_front(texture);
        binding
    }
}
