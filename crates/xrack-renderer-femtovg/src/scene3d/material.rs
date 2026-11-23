use std::{
    collections::HashMap,
    sync::{Arc, Weak},
};
use xrack_runtime::scene3d::{
    MeshAlphaMode, MeshDielectric, MeshImage, MeshMaterial, MeshTexture, TextureColorSpace,
    TextureFilter, TextureTransform, TextureWrap,
};

use super::buffer;

pub(super) const MATERIAL_TEXTURES: u32 = 9;
pub(super) const CLEARCOAT_TEXTURES: u32 = 3;
pub(super) const CLEARCOAT_SETTINGS_BINDING: u32 = 1 + 2 * (MATERIAL_TEXTURES + CLEARCOAT_TEXTURES);

#[derive(Clone)]
pub(super) struct MaterialBindings {
    pub base: wgpu::BindGroup,
    pub clearcoat: Option<wgpu::BindGroup>,
}

impl MaterialBindings {
    pub fn color(&self) -> &wgpu::BindGroup {
        self.clearcoat.as_ref().unwrap_or(&self.base)
    }
}

struct ImageViews {
    linear: wgpu::TextureView,
    srgb: wgpu::TextureView,
}

impl ImageViews {
    fn new(texture: &wgpu::Texture, mip_level_count: Option<u32>) -> Self {
        let view = |format| {
            texture.create_view(&wgpu::TextureViewDescriptor {
                format: Some(format),
                mip_level_count,
                ..Default::default()
            })
        };
        Self {
            linear: view(wgpu::TextureFormat::Rgba8Unorm),
            srgb: view(wgpu::TextureFormat::Rgba8UnormSrgb),
        }
    }
}

struct UploadedTexture {
    full: ImageViews,
    base: Option<ImageViews>,
}

struct UploadedImage {
    owner: Weak<MeshImage>,
    // Source-only, linear mips, and sRGB mips. Both kinds of view can share level zero.
    variants: [Option<UploadedTexture>; 3],
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct SamplerKey {
    wrap: [TextureWrap; 2],
    min: TextureFilter,
    mag: TextureFilter,
    mip: Option<TextureFilter>,
}

impl SamplerKey {
    fn from_texture(texture: Option<&MeshTexture>) -> Self {
        let Some(texture) = texture else {
            return Self {
                wrap: [TextureWrap::Clamp; 2],
                min: TextureFilter::Nearest,
                mag: TextureFilter::Nearest,
                mip: None,
            };
        };
        Self {
            wrap: texture.wrap,
            min: texture.min_filter,
            mag: texture.mag_filter,
            mip: texture.mip_filter,
        }
    }
}

pub(super) struct Materials {
    layouts: [wgpu::BindGroupLayout; 2],
    white: ImageViews,
    images: HashMap<usize, UploadedImage>,
    pub(super) mip_levels_uploaded: u64,
    samplers: HashMap<SamplerKey, wgpu::Sampler>,
}

impl Materials {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: [wgpu::BindGroupLayout; 2],
    ) -> Self {
        Self {
            layouts,
            white: upload(device, queue, [1, 1], &[255; 4], std::iter::empty()).full,
            mip_levels_uploaded: 0,
            images: HashMap::new(),
            samplers: HashMap::new(),
        }
    }

    pub(super) fn prune(&mut self) {
        self.images
            .retain(|_, image| image.owner.strong_count() != 0);
    }

    pub(super) fn bind(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material: Option<&MeshMaterial>,
        volume_inverse: Option<[[f32; 3]; 3]>,
    ) -> MaterialBindings {
        let base_texture = material.and_then(|material| material.base_texture.as_ref());
        let pbr = material.and_then(|material| material.pbr.as_ref());
        let mr = pbr.and_then(|pbr| pbr.metallic_roughness_texture.as_ref());
        let emissive_texture = pbr.and_then(|pbr| pbr.emissive_texture.as_ref());
        let occlusion = pbr.and_then(|pbr| pbr.occlusion_texture.as_ref());
        let normal = pbr.and_then(|pbr| pbr.normal_map.as_ref());
        let normal_texture = normal.map(|normal| &normal.texture);
        let dielectric = pbr.and_then(|pbr| pbr.dielectric.as_deref());
        let specular_texture = dielectric.and_then(|material| material.specular_texture.as_ref());
        let specular_color = dielectric.and_then(|material| material.color_texture.as_ref());
        let transmission = pbr.and_then(|pbr| pbr.transmission.as_deref());
        let transmission_texture = transmission.and_then(|term| term.texture.as_ref());
        let volume = pbr.and_then(|pbr| pbr.volume());
        let thickness_texture = volume.and_then(|volume| volume.texture.as_ref());
        let bindings = [
            (base_texture, true),
            (mr, false),
            (emissive_texture, true),
            (occlusion, false),
            (normal_texture, false),
            (specular_texture, false),
            (specular_color, true),
            (transmission_texture, false),
            (thickness_texture, false),
        ]
        .map(|(texture, srgb)| self.texture(device, queue, texture, srgb));

        let rgba = material.map_or([1.0; 4], |material| material.base_color);
        let mode = material.map_or(MeshAlphaMode::Opaque, |material| material.alpha_mode);
        let cutoff = match mode {
            MeshAlphaMode::Mask(cutoff) => cutoff,
            _ => -1.0,
        };
        let flag = |value| if value { 1.0 } else { 0.0 };
        let flags = [
            cutoff,
            flag(material.is_some()),
            flag(base_texture.is_some()),
            flag(mode == MeshAlphaMode::Blend),
        ];
        let factors = pbr.map_or([0.0; 4], |pbr| {
            [pbr.metallic, pbr.roughness, pbr.occlusion_strength, 1.0]
        });
        let [r, g, b] = pbr.map_or([0.0; 3], |pbr| pbr.emissive);
        let normal_scale = normal.map_or(1.0, |normal| normal.scale);
        let slot =
            |texture: Option<&MeshTexture>| texture.map_or(-1.0, |tex| f32::from(tex.uv_slot));
        let [sr, sg, sb] = dielectric.map_or([1.0; 3], |material| material.color);
        let specular = dielectric.map_or(1.0, |material| material.specular);
        let f0 = dielectric.map_or(
            MeshDielectric::DEFAULT_F0,
            MeshDielectric::normal_reflectance,
        );
        let refraction = pbr.map(|pbr| pbr.refraction_channels()).unwrap_or_default();
        let [inv_r, inv_g, inv_b] = refraction.inverse_ior;
        let [lod_r, lod_g, lod_b] = refraction.blur_scale;
        let [ar, ag, ab] = volume.map_or([1.0; 3], |volume| volume.attenuation_color);
        let distance = volume
            .and_then(|volume| volume.attenuation_distance)
            .unwrap_or(0.0);
        let volume_inverse = volume_inverse.unwrap_or([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
        let bytes: Vec<_> = rgba
            .into_iter()
            .chain(flags)
            .chain(transform_words(base_texture))
            .chain(factors)
            .chain([r, g, b, normal_scale])
            .chain([
                slot(mr),
                slot(emissive_texture),
                slot(occlusion),
                slot(normal_texture),
            ])
            .chain([sr, sg, sb, specular])
            .chain([f0, slot(specular_texture), slot(specular_color), 0.0])
            .chain([
                transmission.map_or(0.0, |term| term.factor),
                slot(transmission_texture),
                lod_g,
                0.0,
            ])
            .chain([
                volume.map_or(0.0, |volume| volume.thickness),
                slot(thickness_texture),
                inv_g,
                0.0,
            ])
            .chain([inv_r, inv_b, lod_r, lod_b])
            .chain([ar, ag, ab, distance])
            .chain(
                volume_inverse
                    .into_iter()
                    .flat_map(|[a, b, c]| [a, b, c, 0.0]),
            )
            .chain(
                [
                    mr,
                    emissive_texture,
                    occlusion,
                    normal_texture,
                    specular_texture,
                    specular_color,
                    transmission_texture,
                    thickness_texture,
                ]
                .into_iter()
                .flat_map(transform_words),
            )
            .flat_map(f32::to_ne_bytes)
            .collect();
        let uniform = buffer(
            device,
            "XRack 3D material factors",
            &bytes,
            wgpu::BufferUsages::UNIFORM,
        );
        let mut entries = vec![wgpu::BindGroupEntry {
            binding: 0,
            resource: uniform.as_entire_binding(),
        }];
        texture_entries(&mut entries, 0, &bindings);
        let base = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D material"),
            layout: &self.layouts[0],
            entries: &entries,
        });
        let clearcoat = pbr
            .and_then(|pbr| pbr.clearcoat.as_ref())
            .filter(|coat| coat.amount > 0.0)
            .map(|coat| {
                let normal = coat.normal_map.as_ref();
                let textures = [
                    coat.amount_texture.as_ref(),
                    coat.roughness_texture.as_ref(),
                    normal.map(|normal| &normal.texture),
                ];
                let bytes: Vec<_> = [
                    coat.amount,
                    coat.roughness,
                    normal.map_or(1.0, |normal| normal.scale),
                    0.0,
                ]
                .into_iter()
                .chain(textures.map(slot))
                .chain([0.0])
                .chain(textures.into_iter().flat_map(transform_words))
                .flat_map(f32::to_ne_bytes)
                .collect();
                let uniform = buffer(
                    device,
                    "XRack 3D clearcoat factors",
                    &bytes,
                    wgpu::BufferUsages::UNIFORM,
                );
                let bindings = textures.map(|texture| self.texture(device, queue, texture, false));
                // The shadow pass uses the base binding and its existing alpha test.
                let mut entries = entries;
                texture_entries(&mut entries, MATERIAL_TEXTURES, &bindings);
                entries.push(wgpu::BindGroupEntry {
                    binding: CLEARCOAT_SETTINGS_BINDING,
                    resource: uniform.as_entire_binding(),
                });
                device.create_bind_group(&wgpu::BindGroupDescriptor {
                    label: Some("XRack 3D clearcoat material"),
                    layout: &self.layouts[1],
                    entries: &entries,
                })
            });
        MaterialBindings { base, clearcoat }
    }

    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: Option<&MeshTexture>,
        srgb: bool,
    ) -> (wgpu::TextureView, wgpu::Sampler) {
        let views = if let Some(texture) = texture {
            let image = &texture.image;
            let color = if srgb {
                TextureColorSpace::Srgb
            } else {
                TextureColorSpace::Linear
            };
            let mipmapped = texture.mip_filter.is_some() && image.size() != [1, 1];
            let chain_color = if mipmapped {
                Some(color)
            } else {
                // An unmipped sampler can share the source level of either prepared chain.
                [color, TextureColorSpace::ALL[1 - color.slot()]]
                    .into_iter()
                    .find(|color| image.prepared_mips(*color).is_some())
            };
            let entry = self
                .images
                .entry(Arc::as_ptr(image) as usize)
                .or_insert_with(|| UploadedImage {
                    owner: Arc::downgrade(image),
                    variants: Default::default(),
                });
            let slot = chain_color.map_or(0, |color| 1 + color.slot());
            let uploaded = entry.variants[slot].get_or_insert_with(|| {
                let chain = chain_color.map(|color| image.prepare_mips(color));
                let mut offset = 0;
                let levels = image.mip_extents().filter_map(|[width, height]| {
                    let chain = chain?;
                    let len = width as usize * height as usize * 4;
                    let start = offset;
                    offset += len;
                    Some(([width, height], &chain[start..offset]))
                });
                if chain.is_some() {
                    self.mip_levels_uploaded += image.mip_extents().count() as u64;
                }
                upload(device, queue, image.size(), image.rgba(), levels)
            });
            if mipmapped {
                &uploaded.full
            } else {
                uploaded.base.as_ref().unwrap_or(&uploaded.full)
            }
        } else {
            &self.white
        };
        let view = if srgb {
            views.srgb.clone()
        } else {
            views.linear.clone()
        };
        let key = SamplerKey::from_texture(texture);
        let sampler = self.samplers.entry(key).or_insert_with(|| {
            device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("XRack 3D material sampler"),
                address_mode_u: address(key.wrap[0]),
                address_mode_v: address(key.wrap[1]),
                min_filter: filter(key.min),
                mag_filter: filter(key.mag),
                mipmap_filter: match key.mip {
                    Some(TextureFilter::Linear) => wgpu::MipmapFilterMode::Linear,
                    _ => wgpu::MipmapFilterMode::Nearest,
                },
                ..Default::default()
            })
        });
        (view, sampler.clone())
    }
}

fn transform_words(texture: Option<&MeshTexture>) -> [f32; 8] {
    let transform = texture.map_or(TextureTransform::IDENTITY, |texture| texture.transform);
    let [[ux, uy, u], [vx, vy, v]] = transform.rows;
    let active = f32::from(u8::from(transform != TextureTransform::IDENTITY));
    let linear = f32::from(u8::from([ux, uy, vx, vy] != [1.0, 0.0, 0.0, 1.0]));
    [ux, uy, u, active, vx, vy, v, linear]
}

fn texture_entries<'a>(
    entries: &mut Vec<wgpu::BindGroupEntry<'a>>,
    first: u32,
    bindings: &'a [(wgpu::TextureView, wgpu::Sampler)],
) {
    for (offset, (view, sampler)) in bindings.iter().enumerate() {
        let index = first + offset as u32;
        entries.extend([
            wgpu::BindGroupEntry {
                binding: 1 + 2 * index,
                resource: wgpu::BindingResource::TextureView(view),
            },
            wgpu::BindGroupEntry {
                binding: 2 + 2 * index,
                resource: wgpu::BindingResource::Sampler(sampler),
            },
        ]);
    }
}

fn address(wrap: TextureWrap) -> wgpu::AddressMode {
    match wrap {
        TextureWrap::Clamp => wgpu::AddressMode::ClampToEdge,
        TextureWrap::Repeat => wgpu::AddressMode::Repeat,
        TextureWrap::Mirror => wgpu::AddressMode::MirrorRepeat,
    }
}

fn filter(filter: TextureFilter) -> wgpu::FilterMode {
    match filter {
        TextureFilter::Linear => wgpu::FilterMode::Linear,
        TextureFilter::Nearest => wgpu::FilterMode::Nearest,
    }
}

fn upload<'a>(
    device: &wgpu::Device,
    queue: &wgpu::Queue,
    [width, height]: [u32; 2],
    rgba: &'a [u8],
    levels: impl Iterator<Item = ([u32; 2], &'a [u8])>,
) -> UploadedTexture {
    let levels: Vec<_> = std::iter::once(([width, height], rgba))
        .chain(levels)
        .collect();
    let mip_level_count = levels.len() as u32;
    let extent = |width, height| wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("XRack 3D material texture"),
        size: extent(width, height),
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format: wgpu::TextureFormat::Rgba8Unorm,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
    });
    for (mip_level, ([width, height], rgba)) in levels.into_iter().enumerate() {
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                mip_level: mip_level as u32,
                ..texture.as_image_copy()
            },
            rgba,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(width * 4),
                rows_per_image: Some(height),
            },
            extent(width, height),
        );
    }
    UploadedTexture {
        full: ImageViews::new(&texture, None),
        base: (mip_level_count > 1).then(|| ImageViews::new(&texture, Some(1))),
    }
}
