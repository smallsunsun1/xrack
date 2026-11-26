use std::{
    cell::RefCell,
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Weak},
};
use xrack_runtime::material_shader::{MAX_TEXTURES, MaterialHooks};
use xrack_runtime::scene_effect::{
    BufferInput, BufferSampling, CaptureAttachment, ExternalTexture,
};
use xrack_runtime::scene3d::{
    MaterialChannel, MeshAlphaMode, MeshDielectric, MeshFresnel, MeshImage, MeshMaterial,
    MeshShader, MeshTexture, MeshWorkflow, TextureChannel, TextureColorSpace, TextureFilter,
    TextureSampling, TextureTransform, TextureWrap,
};

use super::{
    buffer,
    color_capture::{CaptureViews, SharedProjectionBuffers},
    item_textures::ItemTextureViews,
};

pub(super) const MATERIAL_TEXTURES: u32 = 10;
pub(super) const CLEARCOAT_TEXTURES: u32 = 3;
pub(super) const CLEARCOAT_SETTINGS_BINDING: u32 = 1 + 2 * (MATERIAL_TEXTURES + CLEARCOAT_TEXTURES);
pub(super) const FIRST_SHADER_TEXTURE_BINDING: u32 = CLEARCOAT_SETTINGS_BINDING + 1;
pub(super) const SHADER_CAPTURE_BINDING: u32 = FIRST_SHADER_TEXTURE_BINDING + MAX_TEXTURES as u32;
// Shadow atlas, baked lightmap, and environment cubemap also occupy fragment slots.
const LIGHTING_TEXTURES: u32 = 3;
pub(super) const BASE_MATERIAL_TEXTURES: u32 = MATERIAL_TEXTURES + LIGHTING_TEXTURES;
pub(super) const FULL_MATERIAL_TEXTURES: u32 =
    BASE_MATERIAL_TEXTURES + CLEARCOAT_TEXTURES + MAX_TEXTURES as u32;

pub(crate) fn material_texture_limit(supported: u32) -> u32 {
    supported.min(FULL_MATERIAL_TEXTURES + MAX_TEXTURES as u32)
}

pub(super) fn augment_texture_slots(device: &wgpu::Device, used: u32) -> usize {
    (device
        .limits()
        .max_sampled_textures_per_shader_stage
        .saturating_sub(used) as usize)
        .min(MAX_TEXTURES)
}

pub(super) fn shader_texture_slots(device: &wgpu::Device, coated: bool) -> usize {
    let used = BASE_MATERIAL_TEXTURES + u32::from(coated) * CLEARCOAT_TEXTURES;
    (device
        .limits()
        .max_sampled_textures_per_shader_stage
        .saturating_sub(used) as usize)
        .min(MAX_TEXTURES)
}

pub(super) fn capture_material_texture_count(
    device: &wgpu::Device,
    material: &MeshMaterial,
) -> u32 {
    let coated = material
        .pbr
        .as_ref()
        .and_then(|pbr| pbr.clearcoat.as_ref())
        .is_some_and(|coat| coat.amount > 0.0);
    let uses_inputs = material.shader.as_ref().is_some_and(|shader| {
        !shader.textures.is_empty()
            && MaterialHooks::from_source(&shader.source).uses_fragment(false)
    });
    BASE_MATERIAL_TEXTURES
        + u32::from(coated) * CLEARCOAT_TEXTURES
        + if uses_inputs {
            shader_texture_slots(device, coated) as u32
        } else {
            0
        }
}

fn shader_texture_flags(sampling: BufferSampling, encoding: u32) -> u32 {
    let wrap = |mode| match mode {
        TextureWrap::Clamp => 0,
        TextureWrap::Repeat => 1,
        TextureWrap::Mirror => 2,
    };
    1 | (u32::from(sampling.mag_filter == TextureFilter::Linear) << 1)
        | (wrap(sampling.wrap[0]) << 2)
        | (wrap(sampling.wrap[1]) << 4)
        | (encoding << 6)
        | (u32::from(sampling.mip_filter == Some(TextureFilter::Linear)) << 8)
        | (u32::from(sampling.min_filter == TextureFilter::Linear) << 9)
        | (sampling.anisotropy_bits() << 10)
}

#[derive(Clone)]
pub(super) struct MaterialBindings {
    pub base: wgpu::BindGroup,
    color: Option<wgpu::BindGroup>,
    unprogrammed_coat: Option<wgpu::BindGroup>,
    pub coated: bool,
    shader: Option<(Arc<str>, usize)>,
    hooks: MaterialHooks,
    pub vertex: Option<Arc<str>>,
}

impl MaterialBindings {
    pub fn program(&self, capture_data: bool) -> Option<(&Arc<str>, usize)> {
        let (source, slots) = self.shader.as_ref()?;
        let fragment = self.hooks.uses_fragment(capture_data);
        (self.hooks.vertex || fragment).then_some((source, if fragment { *slots } else { 0 }))
    }

    pub fn color(&self, slots: usize) -> &wgpu::BindGroup {
        if slots == 0 && self.shader.as_ref().is_some_and(|(_, slots)| *slots > 0) {
            return self.unprogrammed_coat.as_ref().unwrap_or(&self.base);
        }
        self.color.as_ref().unwrap_or(&self.base)
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

#[derive(Default)]
pub(super) struct Images {
    images: HashMap<usize, UploadedImage>,
    pub mip_levels_uploaded: u64,
    pub uploads: u64,
}

pub(super) type SharedImages = Rc<RefCell<Images>>;

impl Images {
    pub fn prune(&mut self) {
        self.images
            .retain(|_, image| image.owner.strong_count() != 0);
    }

    pub fn sampled_input_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Arc<MeshImage>,
        color: TextureColorSpace,
        sampling: BufferSampling,
    ) -> wgpu::TextureView {
        self.view(
            device,
            queue,
            image,
            color,
            (
                sampling.mip_filter.is_some() || sampling.max_anisotropy > 1,
                false,
            ),
        )
    }

    fn view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        image: &Arc<MeshImage>,
        color: TextureColorSpace,
        (mipmapped, prepared): (bool, bool),
    ) -> wgpu::TextureView {
        let mipmapped = mipmapped && image.size() != [1, 1];
        let entry = self
            .images
            .entry(Arc::as_ptr(image) as usize)
            .or_insert_with(|| UploadedImage {
                owner: Arc::downgrade(image),
                variants: Default::default(),
            });
        // The two encodings share level zero. For a base-level sampler use any
        // resident copy before allocating another texture.
        let colors = [color, TextureColorSpace::ALL[1 - color.slot()]];
        let chain_color = if mipmapped {
            Some(color)
        } else if entry.variants[0].is_some() {
            None
        } else {
            colors
                .into_iter()
                .find(|color| entry.variants[1 + color.slot()].is_some())
                .or_else(|| {
                    prepared
                        .then(|| {
                            colors
                                .into_iter()
                                .find(|color| image.prepared_mips(*color).is_some())
                        })
                        .flatten()
                })
        };
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
            self.uploads += 1;
            upload(device, queue, image.size(), image.rgba(), levels)
        });
        let views = if mipmapped {
            &uploaded.full
        } else {
            uploaded.base.as_ref().unwrap_or(&uploaded.full)
        };
        match color {
            TextureColorSpace::Linear => views.linear.clone(),
            TextureColorSpace::Srgb => views.srgb.clone(),
        }
    }
}

const PLACEHOLDER_SAMPLING: TextureSampling = TextureSampling {
    wrap: [TextureWrap::Clamp; 2],
    min_filter: TextureFilter::Nearest,
    mag_filter: TextureFilter::Nearest,
    mip_filter: None,
    max_anisotropy: 1,
};

fn native_anisotropy_limit(sampling: TextureSampling, supported: bool) -> u8 {
    if supported
        && sampling.min_filter == TextureFilter::Linear
        && sampling.mag_filter == TextureFilter::Linear
        && sampling.mip_filter == Some(TextureFilter::Linear)
    {
        sampling.max_anisotropy.clamp(1, 16)
    } else {
        1
    }
}

pub(super) struct Materials {
    layouts: [wgpu::BindGroupLayout; 2],
    shader_layouts: [[Option<wgpu::BindGroupLayout>; 2]; 2],
    white: ImageViews,
    images: SharedImages,
    samplers: HashMap<TextureSampling, wgpu::Sampler>,
    native_anisotropy: bool,
    pub anisotropic_samplers: u64,
    pub(super) items: Arc<ItemTextureViews>,
    pub(super) captures: CaptureViews,
    projections: SharedProjectionBuffers,
}

impl Materials {
    pub(super) fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        layouts: [wgpu::BindGroupLayout; 2],
        shader_layouts: [[Option<wgpu::BindGroupLayout>; 2]; 2],
        (images, projections, native_anisotropy): (SharedImages, SharedProjectionBuffers, bool),
    ) -> Self {
        Self {
            layouts,
            shader_layouts,
            white: upload(device, queue, [1, 1], &[255; 4], std::iter::empty()).full,
            images,
            samplers: HashMap::new(),
            native_anisotropy,
            anisotropic_samplers: 0,
            items: Arc::default(),
            captures: CaptureViews::new(),
            projections,
        }
    }

    pub(super) fn bind(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        material: Option<&MeshMaterial>,
        volume_inverse: Option<[[f32; 3]; 3]>,
    ) -> MaterialBindings {
        let item = material
            .and_then(|material| material.base_item.as_deref())
            .and_then(|item| Some((item, self.items.get(item.source())?.clone())));
        let base_texture = material.and_then(|material| material.base_texture.as_ref());
        let pbr = material.and_then(|material| material.pbr.as_ref());
        let mr = pbr.and_then(|pbr| pbr.surface_texture.as_ref());
        let glossy = pbr.is_some_and(|pbr| pbr.workflow == MeshWorkflow::SpecularGlossiness);
        let emissive_texture = pbr.and_then(|pbr| pbr.emissive_texture.as_ref());
        let occlusion = pbr.and_then(|pbr| pbr.occlusion_texture.as_ref());
        let normal = pbr.and_then(|pbr| pbr.normal_map.as_ref());
        let normal_texture = normal.map(|normal| &normal.texture);
        let dielectric = pbr.and_then(|pbr| pbr.dielectric.as_deref());
        let specular_texture = dielectric.and_then(|material| material.specular_texture.as_ref());
        let specular_color = dielectric.and_then(|material| material.color_texture.as_ref());
        let transmission = pbr
            .and_then(|pbr| pbr.transmission.as_deref())
            .filter(|term| term.factor > 0.0);
        let transmission_texture = transmission.and_then(|term| term.texture.as_ref());
        let volume = transmission.and(pbr).and_then(|pbr| pbr.volume());
        let thickness_texture = volume.and_then(|volume| volume.texture.as_ref());
        let anisotropy = pbr
            .and_then(|pbr| pbr.anisotropy.as_deref())
            .filter(|term| term.strength > 0.0);
        let anisotropy_map = anisotropy.and_then(|term| term.texture.as_ref());
        let shared_gloss_alpha = glossy
            && material.is_some_and(|material| {
                material.channels.channel(MaterialChannel::Glossiness) == TextureChannel::Alpha
            })
            && mr
                .zip(specular_color)
                .is_some_and(|(a, b)| Arc::ptr_eq(&a.image, &b.image));
        let mut bindings = [
            (base_texture.filter(|_| item.is_none()), true),
            // glTF's packed specular/glossiness map shares its sRGB image views.
            (mr, shared_gloss_alpha),
            (emissive_texture, true),
            (occlusion, false),
            (normal_texture, false),
            (specular_texture, false),
            (specular_color, true),
            (transmission_texture, false),
            (thickness_texture, false),
            (anisotropy_map, false),
        ]
        .map(|(texture, srgb)| self.texture(device, queue, texture, srgb));
        let item_kind = item
            .as_ref()
            .map(|(_, view)| if view.linear_premultiplied { 3.0 } else { 2.0 });
        let live_transform = item.map(|(item, view)| {
            let mut sampling = item.sampling;
            if view.base_view.is_none() {
                sampling.mip_filter = None;
            }
            let texture = if sampling.mip_filter.is_some() {
                view.view
            } else {
                view.base_view.unwrap_or(view.view)
            };
            let portable = portable_anisotropy_limit(sampling, self.native_anisotropy);
            bindings[0] = (texture, self.sampler(device, sampling));
            transform_words(item.transform, portable)
        });

        let rgba = material.map_or([1.0; 4], |material| material.base_color);
        let mode = material.map_or(MeshAlphaMode::Opaque, |material| material.alpha_mode);
        let cutoff = mode.cutoff().unwrap_or(-1.0);
        let flag = |value| if value { 1.0 } else { 0.0 };
        let flags = [
            cutoff,
            flag(
                material.is_some()
                    && !matches!(mode, MeshAlphaMode::InstanceBlend { builtin: true, .. }),
            ),
            item_kind.unwrap_or_else(|| flag(base_texture.is_some())),
            match mode {
                MeshAlphaMode::InstanceBlend { .. } => 2.0,
                MeshAlphaMode::Blend => 1.0,
                _ => 0.0,
            },
        ];
        let factors = pbr.map_or([0.0; 4], |pbr| {
            [
                pbr.metallic,
                pbr.roughness,
                pbr.occlusion_strength,
                if glossy { 2.0 } else { 1.0 },
            ]
        });
        let [r, g, b] = pbr.map_or([0.0; 3], |pbr| pbr.emissive);
        let normal_scale = normal.map_or(1.0, |normal| normal.scale);
        let slot =
            |texture: Option<&MeshTexture>| texture.map_or(-1.0, |tex| f32::from(tex.uv_slot));
        let [sr, sg, sb] = dielectric.map_or([1.0; 3], |material| material.color);
        let specular = dielectric.map_or(1.0, |material| material.specular);
        let f0 = if glossy {
            1.0
        } else {
            dielectric.map_or(
                MeshDielectric::DEFAULT_F0,
                MeshDielectric::normal_reflectance,
            )
        };
        let refraction = pbr.map(|pbr| pbr.refraction_channels()).unwrap_or_default();
        let [inv_r, inv_g, inv_b] = refraction.inverse_ior;
        let [lod_r, lod_g, lod_b] = refraction.blur_scale;
        let [ar, ag, ab] = volume.map_or([1.0; 3], |volume| volume.attenuation_color);
        let distance = volume
            .and_then(|volume| volume.attenuation_distance)
            .unwrap_or(0.0);
        let volume_inverse = volume_inverse.unwrap_or([[1., 0., 0.], [0., 1., 0.], [0., 0., 1.]]);
        let fresnel = pbr.map_or_else(MeshFresnel::default, |pbr| pbr.fresnel);
        let mut channels = material
            .map_or_else(Default::default, |material| material.channels)
            .gpu_words();
        if let Some(material) = material {
            channels[2] = material.vertex_masks.gpu_word(material.vertex_colors);
        }
        let shader = material.and_then(|material| material.shader.as_ref());
        let hooks = shader.map_or_else(MaterialHooks::default, |shader| {
            MaterialHooks::from_source(&shader.source)
        });
        let coat = pbr
            .and_then(|pbr| pbr.clearcoat.as_ref())
            .filter(|coat| coat.amount > 0.0);
        let slots = shader
            .filter(|shader| !shader.textures.is_empty() && hooks.fragment())
            .map_or(0, |_| shader_texture_slots(device, coat.is_some()));
        let (texture_flags, shader_views) = self.external_textures(
            device,
            queue,
            shader.map_or(&[][..], |shader| &shader.textures),
            slots,
        );
        let projections = shader
            .filter(|_| slots != 0 && hooks.capture_position)
            .map(|shader| self.external_projections(device, &shader.textures, slots));
        let parameters = shader.map_or([0.0; 4], MeshShader::vertex_parameters);
        let native_anisotropy = self.native_anisotropy;
        let map_transform = |texture: Option<&MeshTexture>| {
            texture.map_or_else(
                || transform_words(TextureTransform::IDENTITY, 1),
                |texture| {
                    transform_words(
                        texture.transform,
                        portable_anisotropy_limit(texture.sampling, native_anisotropy),
                    )
                },
            )
        };
        let base_transform = live_transform.unwrap_or_else(|| map_transform(base_texture));
        let bytes: Vec<_> = rgba
            .into_iter()
            .chain(flags)
            .chain(base_transform)
            // The shader reads this uniform word as u32, preserving its packed bits.
            .chain(channels.map(f32::from_bits))
            .chain(parameters)
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
                .flat_map(map_transform),
            )
            .chain(fresnel.gpu_factors())
            .chain(anisotropy.map_or([0.0, 1.0, 0.0, -1.0], |term| {
                let (sine, cosine) = term.rotation.sin_cos();
                [term.strength, cosine, sine, slot(anisotropy_map)]
            }))
            .chain(map_transform(anisotropy_map))
            .chain(texture_flags.map(f32::from_bits))
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
        let mut unprogrammed_coat = None;
        let color = if let Some(coat) = coat {
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
            .chain(textures.into_iter().flat_map(map_transform))
            .chain(coat.fresnel.gpu_factors())
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
            if slots > 0 && !hooks.uses_fragment(false) {
                unprogrammed_coat =
                    Some(self.color_binding(device, true, entries.clone(), &[], None));
            }
            Some(self.color_binding(device, true, entries, &shader_views, projections.as_ref()))
        } else if slots > 0 {
            Some(self.color_binding(device, false, entries, &shader_views, projections.as_ref()))
        } else {
            None
        };
        MaterialBindings {
            base,
            color,
            unprogrammed_coat,
            coated: coat.is_some(),
            shader: shader.map(|shader| (shader.source.clone(), slots)),
            hooks,
            vertex: shader.and_then(MeshShader::vertex_program).cloned(),
        }
    }

    fn color_binding<'a>(
        &self,
        device: &wgpu::Device,
        coated: bool,
        mut entries: Vec<wgpu::BindGroupEntry<'a>>,
        views: &'a [wgpu::TextureView],
        projections: Option<&'a wgpu::Buffer>,
    ) -> wgpu::BindGroup {
        let index = usize::from(coated);
        let layout =
            if let Some(layout) =
                self.shader_layouts[index][usize::from(projections.is_some())].as_ref()
                && !views.is_empty()
            {
                entries.extend(views.iter().enumerate().map(|(index, view)| {
                    wgpu::BindGroupEntry {
                        binding: FIRST_SHADER_TEXTURE_BINDING + index as u32,
                        resource: wgpu::BindingResource::TextureView(view),
                    }
                }));
                if let Some(projections) = projections {
                    entries.push(wgpu::BindGroupEntry {
                        binding: SHADER_CAPTURE_BINDING,
                        resource: projections.as_entire_binding(),
                    });
                }
                layout
            } else {
                &self.layouts[index]
            };
        device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack 3D color material"),
            layout,
            entries: &entries,
        })
    }

    pub(super) fn external_textures(
        &self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        inputs: &[BufferInput],
        slots: usize,
    ) -> ([u32; MAX_TEXTURES], Vec<wgpu::TextureView>) {
        let mut flags = [0; MAX_TEXTURES];
        let views = flags[..slots]
            .iter_mut()
            .enumerate()
            .map(|(index, flag)| {
                let Some(input) = inputs.get(index) else {
                    return self.white.linear.clone();
                };
                let mut sampling = input.sampling;
                let (view, encoding) = match input.external_texture() {
                    Some(ExternalTexture::Image(image)) => (
                        self.images.borrow_mut().sampled_input_view(
                            device,
                            queue,
                            &image.image,
                            image.color,
                            sampling,
                        ),
                        0,
                    ),
                    Some(ExternalTexture::Item(name)) if let Some(item) = self.items.get(name) => (
                        item.view.clone(),
                        if item.linear_premultiplied { 1 } else { 2 },
                    ),
                    Some(ExternalTexture::Capture(read))
                        if let Some(capture) = self.captures.get(read) =>
                    {
                        if read.attachment == CaptureAttachment::Depth {
                            sampling.min_filter = TextureFilter::Nearest;
                            sampling.mag_filter = TextureFilter::Nearest;
                            sampling.max_anisotropy = 1;
                        }
                        (
                            capture.view.clone(),
                            match read.attachment {
                                CaptureAttachment::Color => 1,
                                CaptureAttachment::Depth => 3,
                                _ => 0,
                            },
                        )
                    }
                    _ => return self.white.linear.clone(),
                };
                *flag = shader_texture_flags(sampling, encoding);
                view
            })
            .collect();
        (flags, views)
    }

    pub(super) fn external_projections(
        &self,
        device: &wgpu::Device,
        inputs: &[BufferInput],
        slots: usize,
    ) -> wgpu::Buffer {
        self.projections.borrow_mut().get(
            device,
            slots,
            inputs.iter().map(|texture| {
                let ExternalTexture::Capture(read) = texture.external_texture()? else {
                    return None;
                };
                self.captures.get(read)?.projection.as_deref()
            }),
        )
    }

    fn texture(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        texture: Option<&MeshTexture>,
        srgb: bool,
    ) -> (wgpu::TextureView, wgpu::Sampler) {
        let color = if srgb {
            TextureColorSpace::Srgb
        } else {
            TextureColorSpace::Linear
        };
        let view = if let Some(texture) = texture {
            self.images.borrow_mut().view(
                device,
                queue,
                &texture.image,
                color,
                (texture.sampling.mip_filter.is_some(), true),
            )
        } else if srgb {
            self.white.srgb.clone()
        } else {
            self.white.linear.clone()
        };
        let sampling = texture.map_or(PLACEHOLDER_SAMPLING, |texture| texture.sampling);
        (view, self.sampler(device, sampling))
    }

    fn sampler(&mut self, device: &wgpu::Device, sampling: TextureSampling) -> wgpu::Sampler {
        let key = TextureSampling {
            max_anisotropy: native_anisotropy_limit(sampling, self.native_anisotropy),
            ..sampling
        };
        self.samplers
            .entry(key)
            .or_insert_with(|| {
                self.anisotropic_samplers += u64::from(key.max_anisotropy > 1);
                device.create_sampler(&wgpu::SamplerDescriptor {
                    label: Some("XRack 3D material sampler"),
                    address_mode_u: address(key.wrap[0]),
                    address_mode_v: address(key.wrap[1]),
                    min_filter: filter(key.min_filter),
                    mag_filter: filter(key.mag_filter),
                    anisotropy_clamp: u16::from(key.max_anisotropy),
                    mipmap_filter: match key.mip_filter {
                        Some(TextureFilter::Linear) => wgpu::MipmapFilterMode::Linear,
                        _ => wgpu::MipmapFilterMode::Nearest,
                    },
                    ..Default::default()
                })
            })
            .clone()
    }
}

fn portable_anisotropy_limit(sampling: TextureSampling, supported: bool) -> u8 {
    if native_anisotropy_limit(sampling, supported) > 1 {
        1
    } else {
        sampling.max_anisotropy.clamp(1, 16)
    }
}

fn transform_words(transform: TextureTransform, portable_anisotropy: u8) -> [f32; 8] {
    let [[ux, uy, u], [vx, vy, v]] = transform.rows;
    let active = f32::from(u8::from(transform != TextureTransform::IDENTITY));
    let linear = f32::from(u8::from([ux, uy, vx, vy] != [1.0, 0.0, 0.0, 1.0]));
    let gradient_flags = linear + 2.0 * f32::from(portable_anisotropy.saturating_sub(1));
    [ux, uy, u, active, vx, vy, v, gradient_flags]
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

pub(super) fn address(wrap: TextureWrap) -> wgpu::AddressMode {
    match wrap {
        TextureWrap::Clamp => wgpu::AddressMode::ClampToEdge,
        TextureWrap::Repeat => wgpu::AddressMode::Repeat,
        TextureWrap::Mirror => wgpu::AddressMode::MirrorRepeat,
    }
}

pub(super) fn filter(filter: TextureFilter) -> wgpu::FilterMode {
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
