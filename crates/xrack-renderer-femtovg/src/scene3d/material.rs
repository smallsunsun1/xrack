use std::{
    cell::{OnceCell, RefCell},
    collections::HashMap,
    rc::Rc,
    sync::{Arc, Weak},
};
use xrack_runtime::material_shader::{MAX_TEXTURES, MaterialHooks};
use xrack_runtime::scene_effect::{
    BufferInput, BufferSampling, CaptureAttachment, EffectVolume, ExternalTexture,
    ShaderTextureKinds,
};
use xrack_runtime::scene3d::{
    CubeTexture3D, MaterialChannel, MeshAlphaMode, MeshBlockFormat, MeshDielectric, MeshFresnel,
    MeshImage, MeshMaterial, MeshShader, MeshTexture, MeshWorkflow, TextureChannel,
    TextureColorSpace, TextureFilter, TextureSampling, TextureTransform, TextureWrap,
    VolumeImage3D,
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
pub(super) const SHADER_SPATIAL_SAMPLER_BINDING: u32 = SHADER_CAPTURE_BINDING + 1;
pub(super) const SCALAR_MAP_BINDINGS: [u32; 3] = [
    SHADER_SPATIAL_SAMPLER_BINDING + 1,
    SHADER_SPATIAL_SAMPLER_BINDING + 2,
    SHADER_SPATIAL_SAMPLER_BINDING + 3,
];
pub(super) const SCALAR_MAP_SLOTS: u32 = SCALAR_MAP_BINDINGS.len() as u32;
// Shadow atlas, baked lightmap, and environment cubemap also occupy fragment slots.
const LIGHTING_TEXTURES: u32 = 3;
pub(super) const BASE_MATERIAL_TEXTURES: u32 = MATERIAL_TEXTURES + LIGHTING_TEXTURES;
pub(super) const FULL_MATERIAL_TEXTURES: u32 =
    BASE_MATERIAL_TEXTURES + CLEARCOAT_TEXTURES + MAX_TEXTURES as u32;

pub(crate) fn material_texture_limit(supported: u32) -> u32 {
    supported.min(FULL_MATERIAL_TEXTURES + MAX_TEXTURES as u32)
}

pub(super) fn shader_view_dimension(
    kinds: ShaderTextureKinds,
    slot: usize,
) -> wgpu::TextureViewDimension {
    match kinds.wgsl(slot) {
        "cube" => wgpu::TextureViewDimension::Cube,
        "3d" => wgpu::TextureViewDimension::D3,
        _ => wgpu::TextureViewDimension::D2,
    }
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
    pub height_mapped: bool,
    pub tinted_dielectric: bool,
    pub iridescent: bool,
    pub sheen: bool,
    pub reflection_map: Option<(wgpu::TextureView, wgpu::Sampler)>,
    shader: Option<(Arc<str>, usize)>,
    kinds: ShaderTextureKinds,
    program_layout: Option<wgpu::BindGroupLayout>,
    hooks: MaterialHooks,
    pub vertex: Option<Arc<str>>,
}

#[derive(Clone, Copy)]
pub(super) struct MaterialProgram<'a> {
    pub source: &'a Arc<str>,
    pub slots: usize,
    pub kinds: ShaderTextureKinds,
    pub layout: Option<&'a wgpu::BindGroupLayout>,
}

impl MaterialBindings {
    pub fn program(&self, capture_data: bool) -> Option<MaterialProgram<'_>> {
        let (source, slots) = self.shader.as_ref()?;
        let fragment = self.hooks.uses_fragment(capture_data);
        (self.hooks.vertex || fragment).then_some(MaterialProgram {
            source,
            slots: if fragment { *slots } else { 0 },
            kinds: if fragment {
                self.kinds
            } else {
                ShaderTextureKinds::PLANAR
            },
            layout: self.program_layout.as_ref().filter(|_| fragment),
        })
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
            linear: view(texture.format()),
            srgb: view(texture.format().add_srgb_suffix()),
        }
    }
}

struct UploadedTexture {
    full: ImageViews,
    base: Option<ImageViews>,
}

impl UploadedTexture {
    fn view(&self, color: TextureColorSpace, mipmapped: bool) -> wgpu::TextureView {
        let views = if mipmapped {
            &self.full
        } else {
            self.base.as_ref().unwrap_or(&self.full)
        };
        match color {
            TextureColorSpace::Linear => views.linear.clone(),
            TextureColorSpace::Srgb => views.srgb.clone(),
        }
    }
}

struct UploadedImage {
    owner: Weak<MeshImage>,
    // Source-only, linear mips, and sRGB mips. Both kinds of view can share level zero.
    variants: [Option<UploadedTexture>; 3],
    compressed: OnceCell<Option<UploadedTexture>>,
}

type PackedInput<'a> = (&'a MeshTexture, TextureChannel);
type PackedMapKey = (usize, u8, bool);
type PackedBindingKey = ([Option<PackedMapKey>; 3], bool);

struct UploadedMaps {
    owners: [Option<Weak<MeshImage>>; 3],
    buffer: wgpu::Buffer,
}

#[derive(Default)]
pub(super) struct Images {
    images: HashMap<usize, UploadedImage>,
    packed_maps: HashMap<PackedBindingKey, UploadedMaps>,
    cubemaps: HashMap<usize, (Weak<CubeTexture3D>, wgpu::TextureView)>,
    volumes: HashMap<usize, (Weak<VolumeImage3D>, ImageViews)>,
    pub mip_levels_uploaded: u64,
    pub uploads: u64,
    pub block_bytes_uploaded: u64,
}

pub(super) type SharedImages = Rc<RefCell<Images>>;

impl Images {
    pub fn prune(&mut self) {
        self.packed_maps.retain(|_, maps| {
            maps.owners
                .iter()
                .flatten()
                .all(|owner| owner.strong_count() != 0)
        });
        self.images
            .retain(|_, image| image.owner.strong_count() != 0);
        self.cubemaps
            .retain(|_, (owner, _)| owner.strong_count() != 0);
        self.volumes
            .retain(|_, (owner, _)| owner.strong_count() != 0);
    }

    fn packed_map(
        &mut self,
        device: &wgpu::Device,
        inputs: [Option<PackedInput<'_>>; 3],
        height_range: bool,
    ) -> Option<wgpu::Buffer> {
        let key = inputs.map(|input| {
            input.map(|(map, channel)| {
                (
                    Arc::as_ptr(&map.image) as usize,
                    channel as u8,
                    map.sampling.mip_filter.is_some() && map.image.size() != [1, 1],
                )
            })
        });
        if key.iter().all(Option::is_none) {
            return None;
        }
        Some(
            self.packed_maps
                .entry((key, height_range))
                .or_insert_with(|| {
                    let mut bytes = Vec::new();
                    for (slot, input) in inputs.into_iter().enumerate() {
                        let Some(input) = input else { continue };
                        if !bytes.is_empty() {
                            // The base header stores companion offsets relative to its first texel.
                            // The opacity/height binding uses the last word for the height range.
                            let offset = ((bytes.len() - PACKED_HEADER_BYTES) / 4) as u32;
                            let header_word = 4 + 4 * slot;
                            bytes[header_word..header_word + 4]
                                .copy_from_slice(&offset.to_le_bytes());
                        }
                        self.mip_levels_uploaded +=
                            append_packed_map(&mut bytes, input, height_range && slot == 1);
                    }
                    self.uploads += 1;
                    UploadedMaps {
                        owners: inputs
                            .map(|input| input.map(|(map, _)| Arc::downgrade(&map.image))),
                        buffer: buffer(
                            device,
                            "XRack 3D packed material maps",
                            &bytes,
                            wgpu::BufferUsages::STORAGE,
                        ),
                    }
                })
                .buffer
                .clone(),
        )
    }

    pub fn volume_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        volume: &EffectVolume,
    ) -> Option<wgpu::TextureView> {
        let [width, height, depth_or_array_layers] = volume.image.size();
        if [width, height, depth_or_array_layers].into_iter().max()?
            > device.limits().max_texture_dimension_3d
        {
            return None;
        }
        let (_, views) = self
            .volumes
            .entry(Arc::as_ptr(&volume.image) as usize)
            .or_insert_with(|| {
                let size = wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers,
                };
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("XRack shader volume"),
                    size,
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D3,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[wgpu::TextureFormat::Rgba8UnormSrgb],
                });
                queue.write_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d::ZERO,
                        aspect: wgpu::TextureAspect::All,
                    },
                    volume.image.rgba(),
                    wgpu::TexelCopyBufferLayout {
                        offset: 0,
                        bytes_per_row: Some(width * 4),
                        rows_per_image: Some(height),
                    },
                    size,
                );
                self.uploads += 1;
                (
                    Arc::downgrade(&volume.image),
                    ImageViews::new(&texture, None),
                )
            });
        Some(match volume.color {
            TextureColorSpace::Linear => views.linear.clone(),
            TextureColorSpace::Srgb => views.srgb.clone(),
        })
    }

    pub fn cubemap_view(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        cube: &Arc<CubeTexture3D>,
    ) -> Option<wgpu::TextureView> {
        if cube.side() > device.limits().max_texture_dimension_2d
            || device.limits().max_texture_array_layers < 6
        {
            return None;
        }
        let view = &self
            .cubemaps
            .entry(Arc::as_ptr(cube) as usize)
            .or_insert_with(|| {
                let extent = |side| wgpu::Extent3d {
                    width: side,
                    height: side,
                    depth_or_array_layers: 6,
                };
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("XRack shader cubemap"),
                    size: extent(cube.side()),
                    mip_level_count: cube.levels(),
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba16Float,
                    usage: wgpu::TextureUsages::COPY_DST | wgpu::TextureUsages::TEXTURE_BINDING,
                    view_formats: &[],
                });
                for (level, (side, texels)) in cube.mip_data().enumerate() {
                    queue.write_texture(
                        wgpu::TexelCopyTextureInfo {
                            texture: &texture,
                            mip_level: level as u32,
                            origin: wgpu::Origin3d::ZERO,
                            aspect: wgpu::TextureAspect::All,
                        },
                        texels,
                        wgpu::TexelCopyBufferLayout {
                            offset: 0,
                            bytes_per_row: Some(side * 8),
                            rows_per_image: Some(side),
                        },
                        extent(side),
                    );
                }
                self.uploads += 1;
                self.mip_levels_uploaded += u64::from(cube.levels() - 1);
                let view = texture.create_view(&wgpu::TextureViewDescriptor {
                    dimension: Some(wgpu::TextureViewDimension::Cube),
                    ..Default::default()
                });
                (Arc::downgrade(cube), view)
            })
            .1;
        Some(view.clone())
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
        let blocks = image.blocks();
        let authored_levels = image.authored_level_count();
        let mipmapped =
            mipmapped && authored_levels.map_or(image.size() != [1, 1], |levels| levels > 1);
        let entry = self
            .images
            .entry(Arc::as_ptr(image) as usize)
            .or_insert_with(|| UploadedImage {
                owner: Arc::downgrade(image),
                variants: Default::default(),
                compressed: OnceCell::new(),
            });
        let mut upload_blocks = |blocks: &xrack_runtime::scene3d::MeshTextureBlocks, format| {
            self.uploads += 1;
            self.mip_levels_uploaded += u64::from(blocks.level_count() - 1);
            let levels = blocks.levels().inspect(|(_, bytes)| {
                self.block_bytes_uploaded += bytes.len() as u64;
            });
            upload(device, queue, image.size(), format, levels)
        };
        let compressed = if let Some(basis) = image.basis() {
            entry
                .compressed
                .get_or_init(|| {
                    let astc = MeshBlockFormat::Astc([4, 4]);
                    let bc7 = MeshBlockFormat::Bc7;
                    let etc2 = MeshBlockFormat::Etc2Rgba;
                    // UASTC's block representation is closest to ASTC. ETC1S
                    // can retain its ETC color blocks on ETC2-only devices.
                    let candidates = if basis.is_uastc() {
                        [astc, bc7, etc2]
                    } else {
                        [bc7, etc2, astc]
                    };
                    let features = device.features();
                    candidates.into_iter().find_map(|candidate| {
                        let format = block_format(candidate)?;
                        if !features.contains(format.required_features()) {
                            return None;
                        }
                        let blocks = basis.transcode_blocks(candidate)?;
                        Some(upload_blocks(&blocks, format))
                    })
                })
                .as_ref()
        } else if let Some(blocks) = blocks
            && let Some(format) = block_format(blocks.format())
            && (color == TextureColorSpace::Linear || format.add_srgb_suffix() != format)
            && device.features().contains(format.required_features())
            && blocks.native_equivalent()
            && image
                .size()
                .into_iter()
                .zip(blocks.format().block_size())
                .all(|(size, block)| size.is_multiple_of(block))
        {
            entry
                .compressed
                .get_or_init(|| Some(upload_blocks(blocks, format)))
                .as_ref()
        } else {
            None
        };
        if let Some(uploaded) = compressed {
            return uploaded.view(color, mipmapped);
        }
        // The two encodings share level zero. For a base-level sampler use any
        // resident copy before allocating another texture.
        let colors = [color, TextureColorSpace::ALL[1 - color.slot()]];
        let chain_color = if let Some(levels) = authored_levels {
            // Authored levels have the same bytes for both texture views.
            (levels > 1 && (mipmapped || entry.variants[1].is_some()))
                .then_some(TextureColorSpace::Linear)
        } else if mipmapped {
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
            upload(
                device,
                queue,
                image.size(),
                wgpu::TextureFormat::Rgba8Unorm,
                std::iter::once((image.size(), image.rgba())).chain(levels),
            )
        });
        uploaded.view(color, mipmapped)
    }
}

// Four header words, sixteen level offsets, then packed texels.
const PACKED_HEADER_BYTES: usize = 80;

fn append_packed_map(
    bytes: &mut Vec<u8>,
    (map, channel): PackedInput<'_>,
    record_height_range: bool,
) -> u64 {
    let image = &map.image;
    let [width, height] = image.size();
    let start = bytes.len();
    let end_header = start + PACKED_HEADER_BYTES;
    bytes.resize(end_header, 0);
    bytes[start..start + 4].copy_from_slice(&width.to_le_bytes());
    bytes[start + 4..start + 8].copy_from_slice(&height.to_le_bytes());
    let colors = if channel == TextureChannel::Rgb {
        TextureColorSpace::Srgb
    } else {
        TextureColorSpace::Linear
    };
    let mips = map.sampling.mip_filter.map(|_| image.prepare_mips(colors));
    let mut offset = 0;
    let mut uploaded = 0;
    let levels = std::iter::once(image.rgba()).chain(image.mip_extents().filter_map(|[w, h]| {
        let pixels = mips?;
        let start = offset;
        offset += (w * h * 4) as usize;
        Some(&pixels[start..offset])
    }));
    for (level, rgba) in levels.enumerate() {
        let offset = ((bytes.len() - end_header) / 4) as u32;
        let header = start + 16 + level * 4;
        bytes[header..header + 4].copy_from_slice(&offset.to_le_bytes());
        let pixels = bytes.len();
        if channel == TextureChannel::Rgb {
            // RGB8 keeps a fully mipmapped 4K color map and two 4K scalar maps
            // below the portable 128 MiB storage-buffer binding limit.
            bytes.extend(
                rgba.chunks_exact(4)
                    .flat_map(|pixel| [pixel[0], pixel[1], pixel[2]]),
            );
        } else {
            bytes.extend(rgba.chunks_exact(4).map(|pixel| pixel[channel as usize]));
        }
        if record_height_range && level == 0 {
            // Linear filters and averaged mip levels stay within this range.
            let [min, max] = bytes[pixels..]
                .iter()
                .fold([u8::MAX, 0], |[min, max], &next| {
                    [min.min(next), max.max(next)]
                });
            bytes[start + 12] = min;
            bytes[start + 13] = max;
        }
        bytes.resize(bytes.len().next_multiple_of(4), 0);
        uploaded += u64::from(level != 0);
    }
    bytes.resize(bytes.len().max(start + 96), 0);
    uploaded
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
    spatial_layouts: HashMap<(bool, bool, ShaderTextureKinds), wgpu::BindGroupLayout>,
    spatial_sampler: OnceCell<wgpu::Sampler>,
    white: ImageViews,
    empty_scalar_map: wgpu::Buffer,
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
            spatial_layouts: HashMap::new(),
            spatial_sampler: OnceCell::new(),
            white: upload(
                device,
                queue,
                [1, 1],
                wgpu::TextureFormat::Rgba8Unorm,
                std::iter::once(([1, 1], &[255; 4][..])),
            )
            .full,
            empty_scalar_map: buffer(
                device,
                "XRack 3D empty scalar map",
                &[0; 96],
                wgpu::BufferUsages::STORAGE,
            ),
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
        let separate = pbr.and_then(|pbr| pbr.separate_surface_maps.as_deref());
        let metalness = separate.and_then(|maps| maps.metalness.as_ref());
        let roughness = separate.and_then(|maps| maps.roughness.as_ref());
        // No pass needs the combined map once both channels are overridden.
        let mr = pbr
            .and_then(|pbr| pbr.surface_texture.as_ref())
            .filter(|_| metalness.is_none() || roughness.is_none());
        let glossy = pbr.is_some_and(|pbr| pbr.workflow == MeshWorkflow::SpecularGlossiness);
        let height_map = pbr
            .and_then(|pbr| pbr.height_map.as_deref())
            .filter(|map| map.amount > 0.0);
        let sheen = pbr
            .and_then(|pbr| pbr.sheen.as_deref())
            .filter(|sheen| sheen.is_active());
        let sheen_color_map = sheen.and_then(|sheen| sheen.color_texture.as_ref());
        let sheen_roughness_map = sheen
            .filter(|sheen| sheen.roughness > 0.0)
            .and_then(|sheen| sheen.roughness_texture.as_ref());
        let film = pbr
            .and_then(|pbr| pbr.iridescence.as_deref())
            .filter(|film| film.is_active());
        let film_factor = film.and_then(|film| film.factor_texture.as_ref());
        let film_thickness = film
            .filter(|film| film.thickness_nm[0] != film.thickness_nm[1])
            .and_then(|film| film.thickness_texture.as_ref());
        let scalar_maps = [
            (
                material.and_then(|material| material.opacity_texture.as_ref()),
                MaterialChannel::Opacity,
            ),
            (metalness, MaterialChannel::Metalness),
            (roughness, MaterialChannel::Roughness),
        ];
        let emissive_texture = pbr.and_then(|pbr| pbr.emissive_texture.as_ref());
        let occlusion = pbr.and_then(|pbr| pbr.occlusion_texture.as_ref());
        let normal = pbr.and_then(|pbr| pbr.normal_map.as_ref());
        let normal_texture = normal.map(|normal| &normal.texture);
        let reflection = pbr.and_then(|pbr| pbr.specular_reflection.as_deref());
        let live_reflection = reflection
            .and_then(|reflection| reflection.item.as_deref())
            .and_then(|item| Some((item, self.items.get(item.source())?.clone())));
        let mut reflection_options = [0.0; 4];
        let mut reflection_uv = transform_words(TextureTransform::IDENTITY, 1);
        let reflection_map = if let Some((item, view)) = live_reflection {
            let mut sampling = item.sampling;
            if view.base_view.is_none() {
                sampling.mip_filter = None;
            }
            let texture = if sampling.mip_filter.is_some() {
                view.view
            } else {
                view.base_view.unwrap_or(view.view)
            };
            reflection_options[0] = if view.linear_premultiplied { 1.0 } else { 2.0 };
            reflection_uv = transform_words(item.transform, 1);
            Some((texture, self.sampler(device, sampling)))
        } else {
            reflection
                .and_then(|reflection| reflection.image.as_ref())
                .map(|map| {
                    reflection_uv = transform_words(map.transform, 1);
                    self.texture(device, queue, Some(map), true)
                })
        };
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
        let mode = material.map_or(MeshAlphaMode::Opaque, MeshMaterial::effective_alpha_mode);
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
        let [r_base, g_base, b_base, _] = rgba;
        let [sr, sg, sb] = dielectric.map_or([1.0; 3], |material| material.color);
        let tinted = dielectric.filter(|material| !glossy && material.tint > 0.0);
        let [tint_r, tint_g, tint_b] = tinted.map_or([1.0; 3], |material| {
            material.base_color_tint([r_base, g_base, b_base])
        });
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
        // The sign bit marks the independent map. The scalar is always nonnegative.
        channels[3] = material.map_or(1.0, |material| material.opacity).to_bits() & 0x7fff_ffff;
        channels[3] |= u32::from(scalar_maps[0].0.is_some()) << 31;
        let companion_maps = [
            (height_map.map(|map| &map.texture), MaterialChannel::Height),
            (film_factor, MaterialChannel::Iridescence),
            (film_thickness, MaterialChannel::IridescenceThickness),
        ];
        let [primary_maps, companion_maps] = [scalar_maps, companion_maps].map(|maps| {
            maps.map(|(map, role)| {
                map.zip(material)
                    .map(|(map, style)| (map, style.channels.channel(role)))
            })
        });
        let height = companion_maps[0];
        let sheen_maps = [
            None,
            sheen_color_map.map(|map| (map, TextureChannel::Rgb)),
            sheen_roughness_map
                .zip(material)
                .map(|(map, style)| (map, style.channels.channel(MaterialChannel::SheenRoughness))),
        ];
        let scalar_buffers = std::array::from_fn::<_, 3, _>(|slot| {
            self.images.borrow_mut().packed_map(
                device,
                [primary_maps[slot], companion_maps[slot], sheen_maps[slot]],
                slot == 0 && height.is_some(),
            )
        });
        let (sheen_color_uv, sheen_color_sampling) = scalar_sampler(sheen_color_map);
        let (sheen_roughness_uv, sheen_roughness_sampling) = scalar_sampler(sheen_roughness_map);
        let sheen_factors = sheen.map_or([0.0; 4], |sheen| {
            let [r, g, b] = sheen.color;
            [r, g, b, sheen.roughness]
        });
        let (film_factor_uv, film_factor_sampling) = scalar_sampler(film_factor);
        let (film_thickness_uv, film_thickness_sampling) = scalar_sampler(film_thickness);
        let film_parameters = film.map_or([0.0; 4], |film| {
            [
                film.factor,
                film.ior,
                film.thickness_nm[0],
                film.thickness_nm[1],
            ]
        });
        let (height_transform, height_sampling) = scalar_sampler(height.map(|(map, _)| map));
        let height_parameters = height_map.map_or([0.0; 4], |map| {
            [
                map.amount,
                f32::from(map.samples[0]),
                f32::from(map.samples[1]),
                0.0,
            ]
        });
        let empty_scalar_map = self.empty_scalar_map.clone();
        let [
            (opacity_transform, mut opacity_sampling),
            (metalness_transform, metalness_sampling),
            (roughness_transform, roughness_sampling),
        ] = scalar_maps.map(|(map, _)| scalar_sampler(map));
        // Apply inversion after filtering, using the same cached pixels and sampler.
        if scalar_maps[0].0.is_some()
            && material.is_some_and(|material| material.invert_opacity_map_value)
        {
            opacity_sampling[3] |= 2;
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
        let (texture_flags, shader_views, kinds) = self.external_textures(
            device,
            queue,
            shader.map_or(&[][..], |shader| &shader.textures),
            slots,
            true,
        );
        let projections = shader
            .filter(|_| slots != 0 && hooks.capture_position)
            .map(|shader| self.external_projections(device, &shader.textures, slots));
        let coated = coat.is_some();
        let positions = projections.is_some();
        let program_layout = if kinds.nonplanar() == 0 {
            self.shader_layouts[usize::from(coated)][usize::from(positions)]
                .as_ref()
                .filter(|_| slots != 0)
                .cloned()
        } else {
            Some(
                self.spatial_layouts
                    .entry((coated, positions, kinds))
                    .or_insert_with(|| {
                        super::pipeline::material_layout(device, coated, slots, positions, kinds)
                    })
                    .clone(),
            )
        };
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
            .chain(opacity_transform)
            .chain(opacity_sampling.map(f32::from_bits))
            .chain([
                material.map_or(1.0, |value| value.point_size),
                material.map_or(1.0, |value| value.line_width),
                0.0,
                0.0,
            ])
            .chain(factors)
            .chain([r, g, b, normal_scale])
            .chain([
                slot(mr),
                slot(emissive_texture),
                slot(occlusion),
                slot(normal_texture),
            ])
            .chain([sr, sg, sb, specular])
            .chain([f0, slot(specular_texture), slot(specular_color), tint_r])
            .chain([
                transmission.map_or(0.0, |term| term.factor),
                slot(transmission_texture),
                lod_g,
                tint_g,
            ])
            .chain([
                volume.map_or(0.0, |volume| volume.thickness),
                slot(thickness_texture),
                inv_g,
                tint_b,
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
            .chain(metalness_transform)
            .chain(roughness_transform)
            .chain(metalness_sampling.map(f32::from_bits))
            .chain(roughness_sampling.map(f32::from_bits))
            .chain(reflection_uv)
            .chain(reflection_options)
            .chain(height_transform)
            .chain(height_sampling.map(f32::from_bits))
            .chain(height_parameters)
            .chain(film_parameters)
            .chain(film_factor_uv)
            .chain(film_thickness_uv)
            .chain(film_factor_sampling.map(f32::from_bits))
            .chain(film_thickness_sampling.map(f32::from_bits))
            .chain(sheen_factors)
            .chain(sheen_color_uv)
            .chain(sheen_roughness_uv)
            .chain(sheen_color_sampling.map(f32::from_bits))
            .chain(sheen_roughness_sampling.map(f32::from_bits))
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
        entries.extend(SCALAR_MAP_BINDINGS.into_iter().zip(&scalar_buffers).map(
            |(binding, buffer)| {
                wgpu::BindGroupEntry {
                    binding,
                    resource: buffer
                        .as_ref()
                        .unwrap_or(&empty_scalar_map)
                        .as_entire_binding(),
                }
            },
        ));
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
                    Some(self.color_binding(device, true, entries.clone(), &[], None, None, false));
            }
            Some(self.color_binding(
                device,
                true,
                entries,
                &shader_views,
                projections.as_ref(),
                program_layout.as_ref(),
                kinds.nonplanar() != 0,
            ))
        } else if slots > 0 {
            Some(self.color_binding(
                device,
                false,
                entries,
                &shader_views,
                projections.as_ref(),
                program_layout.as_ref(),
                kinds.nonplanar() != 0,
            ))
        } else {
            None
        };
        MaterialBindings {
            base,
            color,
            unprogrammed_coat,
            coated: coat.is_some(),
            height_mapped: height.is_some(),
            tinted_dielectric: tinted.is_some(),
            iridescent: film.is_some(),
            sheen: sheen.is_some(),
            reflection_map,
            shader: shader.map(|shader| (shader.source.clone(), slots)),
            kinds,
            program_layout,
            hooks,
            vertex: shader.and_then(MeshShader::vertex_program).cloned(),
        }
    }

    fn color_binding<'a>(
        &'a self,
        device: &wgpu::Device,
        coated: bool,
        mut entries: Vec<wgpu::BindGroupEntry<'a>>,
        views: &'a [wgpu::TextureView],
        projections: Option<&'a wgpu::Buffer>,
        program_layout: Option<&wgpu::BindGroupLayout>,
        nonplanar: bool,
    ) -> wgpu::BindGroup {
        let layout =
            if let Some(layout) = program_layout {
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
                if nonplanar {
                    let sampler = self.spatial_sampler.get_or_init(|| {
                        device.create_sampler(&wgpu::SamplerDescriptor {
                            label: Some("XRack material spatial sampler"),
                            mag_filter: wgpu::FilterMode::Linear,
                            min_filter: wgpu::FilterMode::Linear,
                            mipmap_filter: wgpu::MipmapFilterMode::Linear,
                            ..Default::default()
                        })
                    });
                    entries.push(wgpu::BindGroupEntry {
                        binding: SHADER_SPATIAL_SAMPLER_BINDING,
                        resource: wgpu::BindingResource::Sampler(sampler),
                    });
                }
                layout
            } else {
                &self.layouts[usize::from(coated)]
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
        allow_nonplanar: bool,
    ) -> (
        [u32; MAX_TEXTURES],
        Vec<wgpu::TextureView>,
        ShaderTextureKinds,
    ) {
        let mut flags = [0; MAX_TEXTURES];
        let mut kinds = ShaderTextureKinds::PLANAR;
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
                    Some(ExternalTexture::Cubemap(cube)) if allow_nonplanar => {
                        let Some(view) = self.images.borrow_mut().cubemap_view(device, queue, cube)
                        else {
                            return self.white.linear.clone();
                        };
                        kinds.cubemaps |= 1 << index;
                        (view, 0)
                    }
                    Some(ExternalTexture::Volume(volume)) if allow_nonplanar => {
                        let Some(view) =
                            self.images.borrow_mut().volume_view(device, queue, volume)
                        else {
                            return self.white.linear.clone();
                        };
                        kinds.volumes |= 1 << index;
                        (view, 0)
                    }
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
        (flags, views, kinds)
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

fn scalar_sampler(map: Option<&MeshTexture>) -> ([f32; 8], [u32; 4]) {
    let Some(map) = map else {
        return (transform_words(TextureTransform::IDENTITY, 1), [0; 4]);
    };
    let sampling = map.sampling;
    let wrap = |value| match value {
        TextureWrap::Clamp => 0u32,
        TextureWrap::Repeat => 1,
        TextureWrap::Mirror => 2,
    };
    let flags = u32::from(sampling.min_filter == TextureFilter::Linear)
        | (u32::from(sampling.mag_filter == TextureFilter::Linear) << 1)
        | (u32::from(sampling.mip_filter == Some(TextureFilter::Linear)) << 2)
        | (wrap(sampling.wrap[0]) << 4)
        | (wrap(sampling.wrap[1]) << 6);
    let max_level = sampling
        .mip_filter
        .map_or(0, |_| map.image.mip_extents().count() as u32);
    (
        transform_words(map.transform, sampling.max_anisotropy.clamp(1, 16)),
        [u32::from(map.uv_slot), flags, max_level, 1],
    )
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
    format: wgpu::TextureFormat,
    levels: impl Iterator<Item = ([u32; 2], &'a [u8])>,
) -> UploadedTexture {
    let levels: Vec<_> = levels.collect();
    let mip_level_count = levels.len() as u32;
    let extent = |width, height| wgpu::Extent3d {
        width,
        height,
        depth_or_array_layers: 1,
    };
    let srgb = format.add_srgb_suffix();
    let texture = device.create_texture(&wgpu::TextureDescriptor {
        label: Some("XRack 3D material texture"),
        size: extent(width, height),
        mip_level_count,
        sample_count: 1,
        dimension: wgpu::TextureDimension::D2,
        format,
        usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
        view_formats: if format == srgb {
            &[]
        } else {
            std::slice::from_ref(&srgb)
        },
    });
    let (block_width, block_height) = format.block_dimensions();
    let block_bytes = format.block_copy_size(None).unwrap_or(4);
    for (mip_level, ([width, height], bytes)) in levels.into_iter().enumerate() {
        let physical = extent(width, height).physical_size(format);
        queue.write_texture(
            wgpu::TexelCopyTextureInfo {
                mip_level: mip_level as u32,
                ..texture.as_image_copy()
            },
            bytes,
            wgpu::TexelCopyBufferLayout {
                offset: 0,
                bytes_per_row: Some(physical.width / block_width * block_bytes),
                rows_per_image: Some(physical.height / block_height),
            },
            physical,
        );
    }
    UploadedTexture {
        full: ImageViews::new(&texture, None),
        base: (mip_level_count > 1).then(|| ImageViews::new(&texture, Some(1))),
    }
}

fn block_format(format: MeshBlockFormat) -> Option<wgpu::TextureFormat> {
    use wgpu::{AstcBlock, AstcChannel, TextureFormat as F};
    Some(match format {
        MeshBlockFormat::Bc1Rgb | MeshBlockFormat::Bc1Rgba => F::Bc1RgbaUnorm,
        MeshBlockFormat::Bc2 => F::Bc2RgbaUnorm,
        MeshBlockFormat::Bc3 => F::Bc3RgbaUnorm,
        MeshBlockFormat::Bc4 => F::Bc4RUnorm,
        MeshBlockFormat::Bc5 => F::Bc5RgUnorm,
        MeshBlockFormat::Bc7 => F::Bc7RgbaUnorm,
        MeshBlockFormat::Etc2Rgb => F::Etc2Rgb8Unorm,
        MeshBlockFormat::Etc2RgbA1 => F::Etc2Rgb8A1Unorm,
        MeshBlockFormat::Etc2Rgba => F::Etc2Rgba8Unorm,
        MeshBlockFormat::Astc(size) => {
            let block = match size {
                [4, 4] => AstcBlock::B4x4,
                [5, 4] => AstcBlock::B5x4,
                [5, 5] => AstcBlock::B5x5,
                [6, 5] => AstcBlock::B6x5,
                [6, 6] => AstcBlock::B6x6,
                [8, 5] => AstcBlock::B8x5,
                [8, 6] => AstcBlock::B8x6,
                [8, 8] => AstcBlock::B8x8,
                [10, 5] => AstcBlock::B10x5,
                [10, 6] => AstcBlock::B10x6,
                [10, 8] => AstcBlock::B10x8,
                [10, 10] => AstcBlock::B10x10,
                [12, 10] => AstcBlock::B12x10,
                [12, 12] => AstcBlock::B12x12,
                _ => return None,
            };
            F::Astc {
                block,
                channel: AstcChannel::Unorm,
            }
        }
    })
}
