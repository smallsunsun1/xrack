//! Material data shared by 3D resources, rendering, and picking.
use super::{
    basis::MeshBasisTexture, blocks::MeshTextureBlocks, cubemap::TextureAsset3D,
    texture_data::TextureData2D,
};
use crate::{
    Color, RecordValue, Value, Vector3D,
    scene3d::effect::{BufferInput, CaptureRead, ExternalTexture, record_buffer_inputs},
    scene3d::lighting::linear_color,
    scene3d::material_shader::{MAX_TEXTURES, MaterialHooks},
};
use std::{
    io::Read,
    ops::Range,
    sync::{Arc, OnceLock},
};

pub const MAX_MESH_TEXTURE_DIMENSION: u32 = 4096;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureWrap {
    Clamp,
    Repeat,
    Mirror,
}

impl TextureWrap {
    pub(crate) fn parse(mode: &str) -> Option<Self> {
        match mode {
            "clamp" => Some(Self::Clamp),
            "repeat" => Some(Self::Repeat),
            "mirror" => Some(Self::Mirror),
            _ => None,
        }
    }

    fn offset(self, texel: i64, size: i64) -> usize {
        (match self {
            Self::Clamp => texel.clamp(0, size - 1),
            Self::Repeat => texel.rem_euclid(size),
            Self::Mirror => {
                let repeated = texel.rem_euclid(2 * size);
                repeated.min(2 * size - 1 - repeated)
            }
        }) as usize
    }

    fn coordinate(self, value: f64) -> f64 {
        match self {
            Self::Clamp => value.clamp(0.0, 1.0),
            Self::Repeat => value.rem_euclid(1.0),
            Self::Mirror => value.rem_euclid(2.0),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum TextureFilter {
    Nearest,
    Linear,
}

impl TextureFilter {
    pub(crate) fn parse(mode: &str) -> Self {
        if mode == "nearest" {
            Self::Nearest
        } else {
            Self::Linear
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TextureColorSpace {
    Linear,
    Srgb,
}

impl TextureColorSpace {
    pub const ALL: [Self; 2] = [Self::Linear, Self::Srgb];

    pub const fn slot(self) -> usize {
        self as usize
    }
}

#[derive(Debug)]
pub struct MeshImage {
    size: [u32; 2],
    pixels: MeshPixels,
    mips: [OnceLock<Box<[u8]>>; 2],
}

#[derive(Debug)]
enum MeshPixels {
    Rgba(Box<[u8]>),
    Compressed(Box<CompressedPixels>),
    Basis(Box<MeshBasisTexture>),
}

#[derive(Debug)]
struct CompressedPixels {
    blocks: MeshTextureBlocks,
    rgba: OnceLock<Box<[u8]>>,
}

impl MeshImage {
    pub fn new(size: [u32; 2], rgba: Vec<u8>) -> Option<Self> {
        (Self::byte_len(size)? == rgba.len()).then(|| Self {
            size,
            pixels: MeshPixels::Rgba(rgba.into_boxed_slice()),
            mips: Default::default(),
        })
    }

    #[doc(hidden)]
    pub fn from_blocks(
        size: [u32; 2],
        vulkan_format: u32,
        levels: u32,
        bytes: Vec<u8>,
    ) -> Option<Self> {
        Self::byte_len(size)?;
        let blocks = MeshTextureBlocks::new(size, vulkan_format, levels, bytes)?;
        Some(Self::with_blocks(size, blocks))
    }

    fn with_blocks(size: [u32; 2], blocks: MeshTextureBlocks) -> Self {
        Self {
            size,
            pixels: MeshPixels::Compressed(Box::new(CompressedPixels {
                blocks,
                rgba: OnceLock::new(),
            })),
            mips: Default::default(),
        }
    }

    pub fn blocks(&self) -> Option<&MeshTextureBlocks> {
        match &self.pixels {
            MeshPixels::Compressed(image) => Some(&image.blocks),
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn from_basis_ktx2(bytes: &[u8]) -> Result<Self, String> {
        let texture = MeshBasisTexture::parse(bytes)?;
        Ok(Self {
            size: texture.size(),
            pixels: MeshPixels::Basis(Box::new(texture)),
            mips: Default::default(),
        })
    }

    pub fn basis(&self) -> Option<&MeshBasisTexture> {
        match &self.pixels {
            MeshPixels::Basis(texture) => Some(texture),
            _ => None,
        }
    }

    pub fn authored_level_count(&self) -> Option<u32> {
        match &self.pixels {
            MeshPixels::Rgba(_) => None,
            MeshPixels::Compressed(image) => Some(image.blocks.level_count()),
            MeshPixels::Basis(image) => Some(image.level_count()),
        }
    }

    fn byte_len([width, height]: [u32; 2]) -> Option<usize> {
        if !(1..=MAX_MESH_TEXTURE_DIMENSION).contains(&width)
            || !(1..=MAX_MESH_TEXTURE_DIMENSION).contains(&height)
        {
            return None;
        }
        Some(rgba_len([width, height]))
    }

    /// Decode prepared pixels and mip chains embedded in generated Rust.
    #[doc(hidden)]
    pub fn from_packed(size: [u32; 2], bytes: &[u8]) -> Option<Self> {
        let len = Self::byte_len(size)?;
        if bytes.starts_with(MeshTextureBlocks::PACKED_TAG) {
            return Some(Self::with_blocks(
                size,
                MeshTextureBlocks::unpack(size, bytes)?,
            ));
        }
        if MeshBasisTexture::is_container(bytes) {
            let image = Self::from_basis_ktx2(bytes).ok()?;
            return (image.size() == size).then_some(image);
        }
        let mut decoder = flate2::read::ZlibDecoder::new(bytes);
        let mut flags = [0];
        decoder.read_exact(&mut flags).ok()?;
        let [flags] = flags;
        if flags > 3 {
            return None;
        }
        let rgba = read_pixels(&mut decoder, len)?;
        let image = Self::new(size, rgba)?;
        let mip_len = image.mip_byte_len();
        for color in TextureColorSpace::ALL {
            if flags & (1 << color.slot()) != 0 {
                image.mips[color.slot()]
                    .set(read_pixels(&mut decoder, mip_len)?.into_boxed_slice())
                    .ok()?;
            }
        }
        (decoder.read(&mut [0]).ok()? == 0).then_some(image)
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    /// RGBA bytes in top-to-bottom row order. The texture slot determines RGB's encoding.
    pub fn rgba(&self) -> &[u8] {
        match &self.pixels {
            MeshPixels::Rgba(rgba) => rgba,
            MeshPixels::Compressed(image) => image.rgba.get_or_init(|| {
                image
                    .blocks
                    .rgba_levels(image.blocks.levels().take(1), rgba_len(self.size))
            }),
            MeshPixels::Basis(image) => image.base_rgba(),
        }
    }

    pub fn mip_extents(&self) -> impl Iterator<Item = [u32; 2]> + use<> {
        let count = self
            .authored_level_count()
            .map_or(usize::MAX, |levels| (levels - 1) as usize);
        mip_extents(self.size).take(count)
    }

    pub fn mip_byte_len(&self) -> usize {
        self.mip_extents().map(rgba_len).sum()
    }

    pub fn prepared_mips(&self, color: TextureColorSpace) -> Option<&[u8]> {
        let slot = if self.authored_level_count().is_some() {
            0
        } else {
            color.slot()
        };
        self.mips[slot].get().map(Box::as_ref)
    }

    /// Build each requested interpretation once. Color channels of sRGB maps are
    /// filtered in linear light. Data maps and alpha are averaged as linear values.
    pub fn prepare_mips(&self, color: TextureColorSpace) -> &[u8] {
        if let Some(blocks) = self.blocks() {
            return self.mips[0]
                .get_or_init(|| blocks.rgba_levels(blocks.levels().skip(1), self.mip_byte_len()));
        }
        if let Some(basis) = self.basis() {
            return self.mips[0].get_or_init(|| basis.rgba(1..basis.level_count()));
        }
        self.mips[color.slot()].get_or_init(|| {
            let decode = (color == TextureColorSpace::Srgb).then(|| {
                std::array::from_fn(|index| {
                    let value = index as f32 / 255.0;
                    if value <= 0.04045 {
                        value / 12.92
                    } else {
                        ((value + 0.055) / 1.055).powf(2.4)
                    }
                })
            });
            let mut chain = Vec::with_capacity(self.mip_byte_len());
            let mut previous_size = self.size;
            let mut previous_start = 0;
            for size in self.mip_extents() {
                let start = chain.len();
                chain.resize(start + rgba_len(size), 0);
                let (previous, output) = chain.split_at_mut(start);
                let source = if previous.is_empty() {
                    self.rgba()
                } else {
                    &previous[previous_start..]
                };
                reduce_mip(source, previous_size, output, size, decode.as_ref());
                previous_size = size;
                previous_start = start;
            }
            chain.into_boxed_slice()
        })
    }
}

fn read_pixels(decoder: &mut impl Read, len: usize) -> Option<Vec<u8>> {
    let mut pixels = Vec::with_capacity(len);
    decoder.take(len as u64).read_to_end(&mut pixels).ok()?;
    (pixels.len() == len).then_some(pixels)
}

fn rgba_len([width, height]: [u32; 2]) -> usize {
    width as usize * height as usize * 4
}

fn mip_extents(size: [u32; 2]) -> impl Iterator<Item = [u32; 2]> {
    std::iter::successors(Some(size), |&size| {
        (size != [1, 1]).then(|| size.map(|side| (side / 2).max(1)))
    })
    .skip(1)
}

fn reduce_mip(
    pixels: &[u8],
    [width, height]: [u32; 2],
    output: &mut [u8],
    [next_width, next_height]: [u32; 2],
    decode: Option<&[f32; 256]>,
) {
    let area = (width * height) as f32;
    let (pixels, _) = pixels.as_chunks::<4>();
    let (output, _) = output.as_chunks_mut::<4>();
    for (index, pixel) in output.iter_mut().enumerate() {
        // Integer extents include the last texels of odd-sized levels.
        let x0 = index as u32 % next_width * width;
        let y0 = index as u32 / next_width * height;
        let mut sum = [0.0; 4];
        for y in y0 / next_height..(y0 + height).div_ceil(next_height) {
            let y_weight = (y0 + height).min((y + 1) * next_height) - y0.max(y * next_height);
            for x in x0 / next_width..(x0 + width).div_ceil(next_width) {
                let x_weight = (x0 + width).min((x + 1) * next_width) - x0.max(x * next_width);
                let weight = (x_weight * y_weight) as f32;
                let [r, g, b, a] = pixels[(y * width + x) as usize];
                let rgb = [r, g, b].map(|v| decode.map_or(f32::from(v), |lut| lut[v as usize]));
                for (sum, value) in sum.iter_mut().zip([rgb[0], rgb[1], rgb[2], f32::from(a)]) {
                    *sum += value * weight;
                }
            }
        }
        for (channel, (out, value)) in pixel.iter_mut().zip(sum).enumerate() {
            let value = value / area;
            let encoded = if decode.is_some() && channel < 3 {
                if value <= 0.0031308 {
                    value * 12.92 * 255.0
                } else {
                    (1.055 * value.powf(1.0 / 2.4) - 0.055) * 255.0
                }
            } else {
                value
            };
            *out = encoded.round() as u8;
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TextureTransform {
    pub rows: [[f32; 3]; 2],
}

impl TextureTransform {
    pub const IDENTITY: Self = Self {
        rows: [[1., 0., 0.], [0., 1., 0.]],
    };

    pub fn map_linear(self, [u, v]: [f32; 2]) -> [f32; 2] {
        self.rows.map(|[a, b, _]| a * u + b * v)
    }

    pub fn map(self, uv: [f32; 2]) -> [f32; 2] {
        if self == Self::IDENTITY {
            return uv;
        }
        let [u, v] = self.map_linear(uv);
        [u + self.rows[0][2], v + self.rows[1][2]]
    }

    pub(crate) fn is_valid(self) -> bool {
        self.rows.into_iter().flatten().all(f32::is_finite)
    }
}

#[derive(Clone, Debug)]
pub struct MeshTexture {
    pub image: Arc<MeshImage>,
    pub sampling: TextureSampling,
    /// Slot in the prepared vertex buffer. The base color map always uses slot zero.
    pub uv_slot: u8,
    pub transform: TextureTransform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct TextureSampling {
    pub wrap: [TextureWrap; 2],
    pub min_filter: TextureFilter,
    pub mag_filter: TextureFilter,
    /// None restricts sampling to the base level.
    pub mip_filter: Option<TextureFilter>,
    pub max_anisotropy: u8,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneItemTexture {
    source: Arc<str>,
    pub sampling: TextureSampling,
    pub transform: TextureTransform,
}

impl SceneItemTexture {
    pub fn source(&self) -> &str {
        &self.source
    }

    pub(crate) fn source_key(&self) -> Arc<str> {
        Arc::clone(&self.source)
    }

    pub(crate) fn project(&self, uv: [f32; 2]) -> Option<([f32; 2], [i64; 2])> {
        let mapped = self.transform.map(uv);
        if !mapped.into_iter().all(f32::is_finite) {
            return None;
        }
        let mut tile = [0; 2];
        let uv = std::array::from_fn(|axis| {
            let value = f64::from(mapped[axis]);
            let wrap = self.sampling.wrap[axis];
            if wrap != TextureWrap::Clamp {
                tile[axis] = value.floor() as i64;
            }
            let coordinate = wrap.coordinate(value);
            if wrap == TextureWrap::Mirror {
                coordinate.min(2.0 - coordinate) as f32
            } else {
                coordinate as f32
            }
        });
        Some((uv, tile))
    }
}

impl PartialEq for MeshTexture {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.image, &other.image)
            && self.sampling == other.sampling
            && self.uv_slot == other.uv_slot
            && self.transform == other.transform
    }
}

pub fn texture_resource_key(path: &str) -> String {
    format!("texture:{path}")
}

/// Material sampler and UV parameters in authored coordinates.
#[doc(hidden)]
pub struct SceneTexture<'a> {
    pub source: &'a str,
    pub data: Option<&'a TextureData2D>,
    pub wrap_u: &'a str,
    pub wrap_v: &'a str,
    pub min_filter: &'a str,
    pub mag_filter: &'a str,
    pub mip_filter: &'a str,
    pub max_anisotropy: i64,
    pub uv_set: i64,
    pub uv_scale_u: f64,
    pub uv_scale_v: f64,
    pub uv_pivot_u: f64,
    pub uv_pivot_v: f64,
    pub uv_offset_u: f64,
    pub uv_offset_v: f64,
    pub uv_rotation: f64,
    pub uv_flip_u: bool,
    pub uv_flip_v: bool,
}

impl Default for SceneTexture<'_> {
    fn default() -> Self {
        Self {
            source: "",
            data: None,
            wrap_u: "repeat",
            wrap_v: "repeat",
            min_filter: "linear",
            mag_filter: "linear",
            mip_filter: "linear",
            max_anisotropy: 1,
            uv_set: 0,
            uv_scale_u: 1.0,
            uv_scale_v: 1.0,
            uv_pivot_u: 0.0,
            uv_pivot_v: 0.0,
            uv_offset_u: 0.0,
            uv_offset_v: 0.0,
            uv_rotation: 0.0,
            uv_flip_u: false,
            uv_flip_v: false,
        }
    }
}

impl SceneTexture<'_> {
    pub fn item_defaults() -> Self {
        Self {
            wrap_u: "clamp",
            wrap_v: "clamp",
            ..Self::default()
        }
    }

    pub fn prepare(
        self,
        base_color: bool,
        resolve: impl Fn(&str) -> Option<Arc<MeshImage>>,
    ) -> Option<MeshTexture> {
        Some(MeshTexture {
            image: self
                .data
                .and_then(TextureData2D::image)
                .or_else(|| resolve(self.source))?,
            sampling: self.sampling(),
            uv_slot: u8::from(!base_color && self.uv_set == 1),
            transform: self.transform(),
        })
    }

    pub fn prepare_item(self) -> Option<Arc<SceneItemTexture>> {
        (!self.source.is_empty()).then(|| {
            Arc::new(SceneItemTexture {
                source: Arc::from(self.source),
                sampling: self.sampling(),
                transform: self.transform(),
            })
        })
    }

    fn sampling(&self) -> TextureSampling {
        let wrap = |mode| TextureWrap::parse(mode).unwrap_or(TextureWrap::Repeat);
        TextureSampling {
            wrap: [wrap(self.wrap_u), wrap(self.wrap_v)],
            min_filter: TextureFilter::parse(self.min_filter),
            mag_filter: TextureFilter::parse(self.mag_filter),
            mip_filter: (self.mip_filter != "none").then(|| TextureFilter::parse(self.mip_filter)),
            max_anisotropy: self.max_anisotropy.clamp(1, 16) as u8,
        }
    }

    fn transform(&self) -> TextureTransform {
        let angle = if self.uv_rotation.is_finite() {
            self.uv_rotation.rem_euclid(360.0).to_radians()
        } else {
            0.0
        };
        let (sin, cos) = angle.sin_cos();
        let [u, v] = [self.uv_scale_u, self.uv_scale_v];
        let [x, y] = [self.uv_offset_u, self.uv_offset_v];
        let pivot = [self.uv_pivot_u, self.uv_pivot_v];
        let flip = [self.uv_flip_u, self.uv_flip_v];
        let mut rows = [[cos * u, -sin * v, x], [sin * u, cos * v, y]];
        // Scale and rotate about the pivot, translate, then mirror the sampling axes.
        for (axis, [a, b, offset]) in rows.iter_mut().enumerate() {
            *offset += pivot[axis] - (*a * pivot[0] + *b * pivot[1]);
            if flip[axis] {
                *a = -*a;
                *b = -*b;
                *offset = 1.0 - *offset;
            }
        }
        let transform = TextureTransform {
            rows: rows.map(|row| row.map(|v| v as f32)),
        };
        if transform.is_valid() {
            transform
        } else {
            TextureTransform::IDENTITY
        }
    }

    fn from_value(
        value: &Value,
        base_color: bool,
        resolve: &impl Fn(&str) -> Option<TextureAsset3D>,
    ) -> Option<MeshTexture> {
        SceneTexture::authored(value, SceneTexture::default())?
            .prepare(base_color, |key| resolve(key)?.into_image())
    }

    fn authored<'a>(value: &'a Value, defaults: SceneTexture<'a>) -> Option<SceneTexture<'a>> {
        let record = match value {
            Value::String(source) => return Some(SceneTexture { source, ..defaults }),
            Value::TextureData2D(data) => {
                return Some(SceneTexture {
                    data: Some(data),
                    ..defaults
                });
            }
            Value::Record(record) => record,
            _ => return None,
        };
        let string = |key| match record.field(key) {
            Some(Value::String(value)) => Some(value.as_str()),
            _ => None,
        };
        let number = |key| match record.field(key) {
            Some(Value::Float(value)) => Some(*value),
            _ => None,
        };
        let data = match record.field("data") {
            Some(Value::TextureData2D(data)) => Some(data),
            _ => defaults.data,
        };
        let source = match string("source") {
            Some(source) => source,
            None if data.is_some() => defaults.source,
            None => return None,
        };
        Some(SceneTexture {
            data,
            source,
            wrap_u: string("wrap-u").unwrap_or(defaults.wrap_u),
            wrap_v: string("wrap-v").unwrap_or(defaults.wrap_v),
            min_filter: string("min-filter").unwrap_or(defaults.min_filter),
            mag_filter: string("mag-filter").unwrap_or(defaults.mag_filter),
            mip_filter: string("mip-filter").unwrap_or(defaults.mip_filter),
            max_anisotropy: match record.field("max-anisotropy") {
                Some(Value::Int(maximum)) => *maximum,
                _ => defaults.max_anisotropy,
            },
            uv_set: match record.field("uv-set") {
                Some(Value::Int(i)) => *i,
                _ => 0,
            },
            uv_scale_u: number("uv-scale-u").unwrap_or(1.0),
            uv_scale_v: number("uv-scale-v").unwrap_or(1.0),
            uv_pivot_u: number("uv-pivot-u").unwrap_or(defaults.uv_pivot_u),
            uv_pivot_v: number("uv-pivot-v").unwrap_or(defaults.uv_pivot_v),
            uv_offset_u: number("uv-offset-u").unwrap_or(0.0),
            uv_offset_v: number("uv-offset-v").unwrap_or(0.0),
            uv_rotation: number("uv-rotation").unwrap_or(0.0),
            uv_flip_u: matches!(record.field("uv-flip-u"), Some(Value::Bool(true))),
            uv_flip_v: matches!(record.field("uv-flip-v"), Some(Value::Bool(true))),
        })
    }
}

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneMaterialMaps {
    // None inherits. Some(None) clears the inherited map.
    pub base_color_map: Option<Option<MeshTexture>>,
    pub opacity_map: Option<Option<MeshTexture>>,
    pub height_map: Option<Option<MeshTexture>>,
    pub sheen_color_map: Option<Option<MeshTexture>>,
    pub sheen_roughness_map: Option<Option<MeshTexture>>,
    pub iridescence_map: Option<Option<MeshTexture>>,
    pub iridescence_thickness_map: Option<Option<MeshTexture>>,
    pub metallic_roughness_map: Option<Option<MeshTexture>>,
    pub metalness_map: Option<Option<MeshTexture>>,
    pub roughness_map: Option<Option<MeshTexture>>,
    pub glossiness_map: Option<Option<MeshTexture>>,
    pub emissive_map: Option<Option<MeshTexture>>,
    pub occlusion_map: Option<Option<MeshTexture>>,
    pub normal_map: Option<Option<MeshTexture>>,
    pub clearcoat_map: Option<Option<MeshTexture>>,
    pub clearcoat_roughness_map: Option<Option<MeshTexture>>,
    pub clearcoat_normal_map: Option<Option<MeshTexture>>,
    pub transmission_map: Option<Option<MeshTexture>>,
    pub thickness_map: Option<Option<MeshTexture>>,
    pub anisotropy_map: Option<Option<MeshTexture>>,
    pub specular_color_map: Option<Option<MeshTexture>>,
    pub specular_reflection_map: Option<Option<MeshTexture>>,
    pub specular_map: Option<Option<MeshTexture>>,
}

impl SceneMaterialMaps {
    fn from_record(
        record: &RecordValue,
        resolve: &impl Fn(&str) -> Option<TextureAsset3D>,
    ) -> Option<Arc<Self>> {
        let map = |name| {
            record
                .field(name)
                .map(|value| SceneTexture::from_value(value, name == "base-color-map", resolve))
        };
        let maps = Self {
            base_color_map: map("base-color-map"),
            opacity_map: map("opacity-map"),
            height_map: map("height-map"),
            sheen_color_map: map("sheen-color-map"),
            sheen_roughness_map: map("sheen-roughness-map"),
            iridescence_map: map("iridescence-map"),
            iridescence_thickness_map: map("iridescence-thickness-map"),
            metallic_roughness_map: map("metallic-roughness-map"),
            metalness_map: map("metalness-map"),
            roughness_map: map("roughness-map"),
            glossiness_map: map("glossiness-map"),
            emissive_map: map("emissive-map"),
            occlusion_map: map("occlusion-map"),
            normal_map: map("normal-map"),
            clearcoat_map: map("clearcoat-map"),
            clearcoat_roughness_map: map("clearcoat-roughness-map"),
            clearcoat_normal_map: map("clearcoat-normal-map"),
            transmission_map: map("transmission-map"),
            thickness_map: map("thickness-map"),
            anisotropy_map: map("anisotropy-map"),
            specular_color_map: map("specular-color-map"),
            specular_reflection_map: map("specular-reflection-map"),
            specular_map: map("specular-map"),
        };
        (maps != Self::default()).then(|| Arc::new(maps))
    }
}

impl MeshTexture {
    pub(super) fn is_valid(&self) -> bool {
        self.uv_slot <= 1 && self.transform.is_valid()
    }

    fn alpha_at(&self, uv: [f32; 2]) -> f32 {
        self.sample(uv, |pixel| [f32::from(pixel[3]) / 255.0])[0]
    }

    pub(crate) fn rgb_at(&self, uv: [f32; 2], srgb: bool) -> [f32; 3] {
        self.sample(uv, |pixel| {
            if srgb {
                crate::scene3d::lighting::linear_color(crate::Color(u32::from_be_bytes(pixel)))
            } else {
                [pixel[0], pixel[1], pixel[2]].map(|value| f32::from(value) / 255.0)
            }
        })
    }

    pub(crate) fn channel_color(&self, uv: [f32; 2], channel: TextureChannel) -> [f32; 3] {
        match channel {
            TextureChannel::Rgb => self.rgb_at(uv, true),
            TextureChannel::Alpha => [self.alpha_at(uv); 3],
            TextureChannel::Red | TextureChannel::Green | TextureChannel::Blue => {
                [self.rgb_at(uv, true)[channel as usize]; 3]
            }
        }
    }

    pub(crate) fn scalar_at(&self, uv: [f32; 2], channel: TextureChannel) -> f32 {
        self.sample(uv, |pixel| {
            [f32::from(pixel[(channel as usize).min(3)]) / 255.0]
        })[0]
    }

    fn sample<const N: usize>(
        &self,
        uv: [f32; 2],
        convert: impl Fn([u8; 4]) -> [f32; N],
    ) -> [f32; N] {
        let uv = self.transform.map(uv);
        let [width, height] = self.image.size.map(i64::from);
        let [wrap_u, wrap_v] = self.sampling.wrap;
        let x = wrap_u.coordinate(f64::from(uv[0])) * width as f64;
        let y = wrap_v.coordinate(f64::from(uv[1])) * height as f64;
        let rgba = self.image.rgba();
        let texel = |x, y| {
            let column = wrap_u.offset(x, width);
            let row = wrap_v.offset(y, height);
            let start = (row * width as usize + column) * 4;
            convert(std::array::from_fn(|channel| rgba[start + channel]))
        };
        if self.sampling.mag_filter == TextureFilter::Nearest {
            return texel(x.floor() as i64, y.floor() as i64);
        }
        let [x, y] = [x - 0.5, y - 0.5];
        let [left, top] = [x.floor() as i64, y.floor() as i64];
        let [u, v] = [(x - x.floor()) as f32, (y - y.floor()) as f32];
        let [tl, tr, bl, br] = [
            texel(left, top),
            texel(left + 1, top),
            texel(left, top + 1),
            texel(left + 1, top + 1),
        ];
        std::array::from_fn(|i| {
            let top = tl[i] * (1.0 - u) + tr[i] * u;
            let bottom = bl[i] * (1.0 - u) + br[i] * u;
            top * (1.0 - v) + bottom * v
        })
    }
}

/// Color compositing, independent of a material's alpha coverage.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum MeshBlendMode {
    #[default]
    SourceOver,
    Screen,
    Multiply,
}

impl MeshBlendMode {
    #[doc(hidden)]
    pub fn named(value: &str) -> Option<Self> {
        match value {
            "source-over" => Some(Self::SourceOver),
            "screen" => Some(Self::Screen),
            "multiply" => Some(Self::Multiply),
            _ => None,
        }
    }
}

/// Controls when a material contributes to the scene depth.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum MeshDepthDraw {
    #[default]
    OpaqueOnly,
    Always,
    Never,
    OpaquePrepass,
}

impl MeshDepthDraw {
    #[doc(hidden)]
    pub fn named(value: &str) -> Option<Self> {
        match value {
            "opaque-only" => Some(Self::OpaqueOnly),
            "always" => Some(Self::Always),
            "never" => Some(Self::Never),
            "opaque-prepass" => Some(Self::OpaquePrepass),
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn writes_color_depth(self, blended: bool) -> bool {
        match self {
            Self::OpaqueOnly => !blended,
            Self::Always => true,
            Self::Never | Self::OpaquePrepass => false,
        }
    }

    #[doc(hidden)]
    pub fn admits_opaque_prepass(self, blended: bool, transmission: bool) -> bool {
        !blended && !transmission && self.writes_color_depth(false)
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub enum MeshDepthCoverage {
    Opaque,
    Mask,
    NonzeroAlpha,
    OpaqueAlpha,
}

impl MeshDepthCoverage {
    pub fn samples_alpha(self) -> bool {
        self != Self::Opaque
    }
}

/// Which winding to discard after projecting a triangle.
#[derive(Clone, Copy, Debug, Default, Eq, Hash, PartialEq)]
pub enum MeshCullMode {
    #[default]
    Back,
    Front,
    None,
}

impl MeshCullMode {
    #[doc(hidden)]
    pub fn named(value: &str) -> Option<Self> {
        match value {
            "back" => Some(Self::Back),
            "front" => Some(Self::Front),
            "none" => Some(Self::None),
            _ => None,
        }
    }

    pub(crate) fn accepts(self, front: bool) -> bool {
        match self {
            Self::Back => front,
            Self::Front => !front,
            Self::None => true,
        }
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
#[repr(u32)]
pub enum TextureChannel {
    Red,
    Green,
    Blue,
    Alpha,
    Rgb,
}

impl TextureChannel {
    fn named(name: &str) -> Option<Self> {
        match name {
            "r" => Some(Self::Red),
            "g" => Some(Self::Green),
            "b" => Some(Self::Blue),
            "a" => Some(Self::Alpha),
            "rgb" => Some(Self::Rgb),
            _ => None,
        }
    }
}

/// Texture roles that support a material-level channel override.
#[derive(Clone, Copy, Debug)]
#[repr(usize)]
pub enum MaterialChannel {
    BaseColor,
    Emissive,
    Metalness,
    Roughness,
    Occlusion,
    Specular,
    SpecularColor,
    Transmission,
    Thickness,
    Clearcoat,
    ClearcoatRoughness,
    Glossiness,
    Opacity,
    Height,
    Iridescence,
    IridescenceThickness,
    SheenRoughness,
}

const CHANNEL_FIELDS: [(&str, TextureChannel); 17] = [
    ("base-color-channel", TextureChannel::Rgb),
    ("emissive-channel", TextureChannel::Rgb),
    ("metalness-channel", TextureChannel::Blue),
    ("roughness-channel", TextureChannel::Green),
    ("occlusion-channel", TextureChannel::Red),
    ("specular-channel", TextureChannel::Alpha),
    ("specular-color-channel", TextureChannel::Rgb),
    ("transmission-channel", TextureChannel::Red),
    ("thickness-channel", TextureChannel::Green),
    ("clearcoat-channel", TextureChannel::Red),
    ("clearcoat-roughness-channel", TextureChannel::Green),
    ("glossiness-channel", TextureChannel::Alpha),
    ("opacity-channel", TextureChannel::Alpha),
    ("height-channel", TextureChannel::Red),
    ("iridescence-channel", TextureChannel::Red),
    ("iridescence-thickness-channel", TextureChannel::Green),
    ("sheen-roughness-channel", TextureChannel::Alpha),
];

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct MaterialChannels {
    overrides: [Option<TextureChannel>; CHANNEL_FIELDS.len()],
}

impl MaterialChannels {
    #[doc(hidden)]
    pub fn from_fields<'a>(fields: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut result = Self::default();
        for (name, value) in fields {
            if let Some(index) = CHANNEL_FIELDS.iter().position(|(field, _)| *field == name) {
                result.overrides[index] = TextureChannel::named(value).filter(|channel| {
                    *channel != TextureChannel::Rgb
                        || CHANNEL_FIELDS[index].1 == TextureChannel::Rgb
                });
            }
        }
        result
    }

    pub fn channel(&self, role: MaterialChannel) -> TextureChannel {
        let index = role as usize;
        self.overrides[index].unwrap_or(CHANNEL_FIELDS[index].1)
    }

    fn overlay(&mut self, later: Self) {
        for (value, over) in self.overrides.iter_mut().zip(later.overrides) {
            *value = over.or(*value);
        }
    }

    #[doc(hidden)]
    pub fn authored_fields(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        CHANNEL_FIELDS
            .iter()
            .zip(self.overrides)
            .filter_map(|((name, _), channel)| {
                let value = match channel? {
                    TextureChannel::Red => "r",
                    TextureChannel::Green => "g",
                    TextureChannel::Blue => "b",
                    TextureChannel::Alpha => "a",
                    TextureChannel::Rgb => "rgb",
                };
                Some((*name, value))
            })
    }

    /// Four bits per role, with eight roles per uniform word.
    #[doc(hidden)]
    pub fn gpu_words(&self) -> [u32; 4] {
        let mut words = [0; 4];
        for (index, (_, default)) in CHANNEL_FIELDS.iter().enumerate() {
            let channel = self.overrides[index].unwrap_or(*default);
            words[index / 8] |= (channel as u32) << (4 * (index % 8));
        }
        words
    }

    #[doc(hidden)]
    pub fn has_remapping(&self) -> bool {
        self.overrides
            .iter()
            .zip(CHANNEL_FIELDS)
            // Packed scalar channels are selected during GPU upload.
            .take(MaterialChannel::Opacity as usize)
            .any(|(channel, (_, default))| channel.is_some_and(|value| value != default))
    }
}

// Order matches the vertex-mask role indices in scene.wgsl.
const VERTEX_MASK_FIELDS: [&str; 10] = [
    "vertex-metalness-channel",
    "vertex-roughness-channel",
    "vertex-occlusion-channel",
    "vertex-specular-channel",
    "vertex-normal-channel",
    "vertex-clearcoat-channel",
    "vertex-clearcoat-roughness-channel",
    "vertex-clearcoat-normal-channel",
    "vertex-transmission-channel",
    "vertex-thickness-channel",
];

/// Per-role vertex color multipliers. Three bits encode inherit, none or RGBA.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct VertexColorMasks(u32);

impl VertexColorMasks {
    const OPTIONS: [&'static str; 6] = ["", "none", "r", "g", "b", "a"];

    #[doc(hidden)]
    pub fn from_fields<'a>(fields: impl IntoIterator<Item = (&'a str, &'a str)>) -> Self {
        let mut result = Self::default();
        for (name, channel) in fields {
            if let Some(role) = VERTEX_MASK_FIELDS.iter().position(|field| *field == name)
                && let Some(value) = Self::OPTIONS.iter().position(|option| *option == channel)
            {
                result.set(role, value as u32);
            }
        }
        result
    }

    fn field(&self, role: usize) -> u32 {
        (self.0 >> (3 * role)) & 7
    }

    fn set(&mut self, role: usize, value: u32) {
        let shift = 3 * role;
        self.0 = (self.0 & !(7 << shift)) | (value << shift);
    }

    fn overlay(&mut self, later: Self) {
        if later.0 == 0 {
            return;
        }
        for role in 0..VERTEX_MASK_FIELDS.len() {
            let value = later.field(role);
            if value != 0 {
                self.set(role, value);
            }
        }
    }

    fn masks_metalness(&self) -> bool {
        self.field(0) >= 2
    }

    #[doc(hidden)]
    pub fn masks_roughness(&self) -> bool {
        self.field(1) >= 2
    }

    #[doc(hidden)]
    pub fn masks_clearcoat_roughness(&self) -> bool {
        self.field(6) >= 2
    }

    pub(crate) fn metalness(&self, color: [f32; 4]) -> f32 {
        self.multiplier(0, color)
    }

    pub(crate) fn specular(&self, color: [f32; 4]) -> f32 {
        self.multiplier(3, color)
    }

    fn multiplier(&self, role: usize, color: [f32; 4]) -> f32 {
        match self.field(role) {
            channel @ 2..=5 => color[(channel - 2) as usize].clamp(0.0, 1.0),
            _ => 1.0,
        }
    }

    #[doc(hidden)]
    pub fn authored_fields(&self) -> impl Iterator<Item = (&'static str, &'static str)> + '_ {
        VERTEX_MASK_FIELDS
            .into_iter()
            .enumerate()
            .filter_map(|(role, name)| {
                let value = self.field(role) as usize;
                (value != 0).then(|| (name, Self::OPTIONS[value]))
            })
    }

    #[doc(hidden)]
    pub fn gpu_word(&self, vertex_colors: bool) -> u32 {
        self.0 | (u32::from(!vertex_colors) << 31)
    }

    #[doc(hidden)]
    pub fn uses_vertex_masks(&self) -> bool {
        (0..VERTEX_MASK_FIELDS.len()).any(|role| self.field(role) >= 2)
    }
}

#[derive(Clone, Debug)]
pub struct MeshMaterial {
    /// Diameter of point primitives in render-target pixels.
    pub point_size: f32,
    /// Width of line primitives in render-target pixels.
    pub line_width: f32,
    pub base_color: [f32; 4],
    pub opacity: f32,
    pub opacity_texture: Option<MeshTexture>,
    pub invert_opacity_map_value: bool,
    pub base_texture: Option<MeshTexture>,
    pub base_item: Option<Arc<SceneItemTexture>>,
    pub channels: MaterialChannels,
    pub vertex_colors: bool,
    pub vertex_masks: VertexColorMasks,
    pub alpha_mode: MeshAlphaMode,
    pub blend_mode: MeshBlendMode,
    pub depth_draw_mode: MeshDepthDraw,
    pub cull_mode: MeshCullMode,
    /// Absent for glTF KHR_materials_unlit.
    pub pbr: Option<MeshPbr>,
    pub shader: Option<MeshShader>,
}

#[derive(Clone, Debug)]
pub struct MeshShader {
    pub source: Arc<str>,
    pub parameters: [f32; 3],
    pub displacement: f32,
    pub textures: Arc<[BufferInput]>,
}

impl MeshShader {
    pub fn vertex_program(&self) -> Option<&Arc<str>> {
        MaterialHooks::from_source(&self.source)
            .vertex
            .then_some(&self.source)
    }

    pub fn vertex_parameters(&self) -> [f32; 4] {
        let [x, y, z] = self.parameters;
        [x, y, z, self.displacement]
    }

    pub fn capture_sources(&self, slots: usize) -> impl Iterator<Item = &CaptureRead> {
        self.textures
            .iter()
            .take(if MaterialHooks::from_source(&self.source).fragment() {
                slots.min(MAX_TEXTURES)
            } else {
                0
            })
            .filter_map(|input| match input.external_texture() {
                Some(ExternalTexture::Capture(name)) => Some(name),
                _ => None,
            })
    }

    pub fn item_sources(&self) -> impl Iterator<Item = &str> {
        self.textures
            .iter()
            .take(MAX_TEXTURES)
            .filter_map(|input| match input.external_texture() {
                Some(ExternalTexture::Item(name)) => Some(name),
                _ => None,
            })
    }
}

/// Directional roughness of the base specular lobe.
#[derive(Clone, Debug, Default)]
pub struct MeshAnisotropy {
    pub strength: f32,
    /// Radians in tangent space, counterclockwise from the tangent.
    pub rotation: f32,
    pub texture: Option<MeshTexture>,
}

impl MeshAnisotropy {
    fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.strength)
            && self.rotation.is_finite()
            && self.texture.as_ref().is_none_or(MeshTexture::is_valid)
    }
}

/// Angle-dependent reflectance controls shared by base reflectance and clearcoat.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshFresnel {
    pub power: f32,
    pub scale: f32,
    pub bias: f32,
    pub scale_bias_enabled: bool,
}

impl Default for MeshFresnel {
    fn default() -> Self {
        Self {
            power: 5.0,
            scale: 1.0,
            bias: 0.0,
            scale_bias_enabled: false,
        }
    }
}

impl MeshFresnel {
    const POWER_RANGE: [f64; 2] = [0.125, 64.0];
    const SCALE_RANGE: [f64; 2] = [0.0, 8.0];
    const BIAS_RANGE: [f64; 2] = [-1.0, 1.0];

    fn with_overrides(
        self,
        power: Option<f64>,
        scale: Option<f64>,
        bias: Option<f64>,
        enabled: Option<bool>,
    ) -> Self {
        let [min_power, max_power] = Self::POWER_RANGE;
        let [min_scale, max_scale] = Self::SCALE_RANGE;
        let [min_bias, max_bias] = Self::BIAS_RANGE;
        Self {
            power: factor(power, self.power, min_power, max_power),
            scale: factor(scale, self.scale, min_scale, max_scale),
            bias: factor(bias, self.bias, min_bias, max_bias),
            scale_bias_enabled: enabled.unwrap_or(self.scale_bias_enabled),
        }
    }

    fn is_valid(self) -> bool {
        let in_range = |value: f32, [min, max]: [f64; 2]| (min..=max).contains(&f64::from(value));
        in_range(self.power, Self::POWER_RANGE)
            && in_range(self.scale, Self::SCALE_RANGE)
            && in_range(self.bias, Self::BIAS_RANGE)
    }

    #[doc(hidden)]
    pub fn is_custom(self) -> bool {
        self.power != 5.0 || (self.scale_bias_enabled && (self.scale != 1.0 || self.bias != 0.0))
    }

    #[doc(hidden)]
    pub fn gpu_factors(self) -> [f32; 4] {
        if self.scale_bias_enabled {
            [self.power, self.scale, self.bias, 0.0]
        } else {
            [self.power, 1.0, 0.0, 0.0]
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub enum MeshWorkflow {
    #[default]
    MetallicRoughness,
    SpecularGlossiness,
}

impl MeshWorkflow {
    pub fn named(name: &str) -> Option<Self> {
        match name {
            "metallic-roughness" => Some(Self::MetallicRoughness),
            "specular-glossiness" => Some(Self::SpecularGlossiness),
            _ => None,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MeshPbr {
    pub workflow: MeshWorkflow,
    pub anisotropy: Option<Box<MeshAnisotropy>>,
    pub fresnel: MeshFresnel,
    pub metallic: f32,
    pub roughness: f32,
    /// Linear emission with the glTF emissive strength already applied.
    pub emissive: [f32; 3],
    pub occlusion_strength: f32,
    /// Linear G/B roughness and metallic, or the selected glossiness channel.
    pub surface_texture: Option<MeshTexture>,
    pub height_map: Option<Box<MeshHeightMap>>,
    pub iridescence: Option<Box<MeshIridescence>>,
    pub sheen: Option<Box<MeshSheen>>,
    pub specular_reflection: Option<Box<MeshSpecularReflection>>,
    pub separate_surface_maps: Option<Box<MeshSurfaceMaps>>,
    pub emissive_texture: Option<MeshTexture>,
    pub occlusion_texture: Option<MeshTexture>,
    pub normal_map: Option<MeshNormalMap>,
    pub clearcoat: Option<MeshClearcoat>,
    pub dielectric: Option<Box<MeshDielectric>>,
    pub transmission: Option<Box<MeshTransmission>>,
}

/// A fabric layer with a linear RGB tint and perceptual roughness.
#[derive(Clone, Debug, Default)]
pub struct MeshSheen {
    pub color: [f32; 3],
    pub roughness: f32,
    pub color_texture: Option<MeshTexture>,
    pub roughness_texture: Option<MeshTexture>,
}

impl MeshSheen {
    #[doc(hidden)]
    pub fn is_active(&self) -> bool {
        self.color.into_iter().any(|component| component > 0.0)
    }

    #[doc(hidden)]
    pub fn is_valid(&self) -> bool {
        self.color
            .into_iter()
            .chain([self.roughness])
            .all(|value| (0.0..=1.0).contains(&value))
            && [&self.color_texture, &self.roughness_texture]
                .into_iter()
                .flatten()
                .all(MeshTexture::is_valid)
    }
}

/// Dielectric thin film as specified by KHR_materials_iridescence.
#[derive(Clone, Debug)]
pub struct MeshIridescence {
    pub factor: f32,
    pub ior: f32,
    pub thickness_nm: [f32; 2],
    pub factor_texture: Option<MeshTexture>,
    pub thickness_texture: Option<MeshTexture>,
}

impl Default for MeshIridescence {
    fn default() -> Self {
        Self {
            factor: 0.0,
            ior: 1.3,
            thickness_nm: [100.0, 400.0],
            factor_texture: None,
            thickness_texture: None,
        }
    }
}

impl MeshIridescence {
    #[doc(hidden)]
    pub fn is_active(&self) -> bool {
        self.factor > 0.0
            && (self.thickness_nm[1] > 0.0
                || self.thickness_texture.is_some() && self.thickness_nm[0] > 0.0)
    }

    #[doc(hidden)]
    pub fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.factor)
            && self.ior.is_finite()
            && self.ior >= 1.0
            && self
                .thickness_nm
                .into_iter()
                .all(|value| value.is_finite() && value >= 0.0)
            && [&self.factor_texture, &self.thickness_texture]
                .into_iter()
                .flatten()
                .all(MeshTexture::is_valid)
    }
}

/// Linear height data and the bounded number of steps in its parallax ray.
#[derive(Clone, Debug)]
pub struct MeshHeightMap {
    pub texture: MeshTexture,
    pub amount: f32,
    pub samples: [u8; 2],
}

impl MeshHeightMap {
    const DEFAULT_SAMPLES: [u8; 2] = [8, 32];
    const MAX_SAMPLES: u8 = 64;

    fn is_valid(&self) -> bool {
        self.texture.is_valid()
            && (0.0..=1.0).contains(&self.amount)
            && (1..=Self::MAX_SAMPLES).contains(&self.samples[1])
            && (1..=self.samples[1]).contains(&self.samples[0])
    }
}

/// A material-owned reflection panorama with an optional live UI source.
#[derive(Clone, Debug)]
pub struct MeshSpecularReflection {
    pub image: Option<MeshTexture>,
    pub item: Option<Arc<SceneItemTexture>>,
}

/// Individual metalness and roughness inputs, each with its own sampler.
#[derive(Clone, Debug)]
pub struct MeshSurfaceMaps {
    pub metalness: Option<MeshTexture>,
    pub roughness: Option<MeshTexture>,
}

#[derive(Clone, Debug)]
pub struct MeshTransmission {
    pub factor: f32,
    pub texture: Option<MeshTexture>,
    pub volume: Option<Box<MeshVolume>>,
}

impl MeshTransmission {
    fn is_valid(&self) -> bool {
        (0.0..=1.0).contains(&self.factor)
            && self.texture.as_ref().is_none_or(MeshTexture::is_valid)
            && self.volume.as_deref().is_none_or(MeshVolume::is_valid)
    }
}

#[derive(Clone, Debug)]
pub struct MeshVolume {
    pub thickness: f32,
    /// Authored thickness is in the prepared XRack model, after static glTF node transforms.
    pub thickness_in_prepared_space: bool,
    pub dispersion: f32,
    pub texture: Option<MeshTexture>,
    pub attenuation_distance: Option<f32>,
    pub attenuation_color: [f32; 3],
}

impl MeshVolume {
    fn is_valid(&self) -> bool {
        self.thickness.is_finite()
            && self.thickness >= 0.0
            && self.dispersion.is_finite()
            && self.dispersion >= 0.0
            && self
                .attenuation_distance
                .is_none_or(|v| v.is_finite() && v > 0.0)
            && self
                .attenuation_color
                .into_iter()
                .all(|v| (0.0..=1.0).contains(&v))
            && self.texture.as_ref().is_none_or(MeshTexture::is_valid)
    }
}

#[derive(Clone, Debug)]
pub struct MeshDielectric {
    /// glTF uses zero to represent an infinite index of refraction.
    pub ior: f32,
    pub specular: f32,
    pub tint: f32,
    pub color: [f32; 3],
    pub specular_texture: Option<MeshTexture>,
    pub color_texture: Option<MeshTexture>,
}

impl MeshDielectric {
    pub const DEFAULT_IOR: f32 = 1.5;
    pub const DEFAULT_F0: f32 = 0.04;

    /// Mix white and linear base color for the dielectric Fresnel response.
    pub fn base_color_tint(&self, base: [f32; 3]) -> [f32; 3] {
        let white = 1.0 - self.tint;
        base.map(|value| white + value * self.tint)
    }

    pub fn normal_reflectance(&self) -> f32 {
        if self.ior == 0.0 {
            return 1.0;
        }
        let ior = f64::from(self.ior);
        let ratio = (ior - 1.0) / (ior + 1.0);
        (ratio * ratio) as f32
    }

    pub fn valid_ior(ior: f32) -> bool {
        ior == 0.0 || (ior.is_finite() && ior >= 1.0)
    }

    fn is_valid(&self) -> bool {
        Self::valid_ior(self.ior)
            && (0.0..=1.0).contains(&self.specular)
            && (0.0..=1.0).contains(&self.tint)
            && self
                .color
                .into_iter()
                .all(|value| value.is_finite() && value >= 0.0)
            && [&self.specular_texture, &self.color_texture]
                .into_iter()
                .flatten()
                .all(MeshTexture::is_valid)
    }
}

#[derive(Clone, Debug)]
pub struct MeshClearcoat {
    pub fresnel: MeshFresnel,
    pub amount: f32,
    pub roughness: f32,
    pub amount_texture: Option<MeshTexture>,
    pub roughness_texture: Option<MeshTexture>,
    pub normal_map: Option<MeshNormalMap>,
}

#[derive(Clone, Debug)]
pub struct MeshNormalMap {
    pub texture: MeshTexture,
    pub scale: f32,
}

#[derive(Clone, Copy)]
pub struct RefractionChannels {
    pub inverse_ior: [f32; 3],
    pub blur_scale: [f32; 3],
}

impl Default for RefractionChannels {
    fn default() -> Self {
        Self::new(MeshDielectric::DEFAULT_IOR, 0.0)
    }
}

impl RefractionChannels {
    fn new(ior: f32, dispersion: f32) -> Self {
        if ior == 0.0 {
            // glTF uses zero to represent an infinite index of refraction.
            return Self {
                inverse_ior: [0.0; 3],
                blur_scale: [1.0; 3],
            };
        }
        let ior = f64::from(ior);
        // The glTF extension stores 20/Abbe; use the standardized RGB approximation.
        let half_spread = (ior - 1.0) * f64::from(dispersion) / 40.0;
        let iors = [(ior - half_spread).max(1.0), ior, ior + half_spread];
        Self {
            inverse_ior: iors.map(|ior| ior.recip() as f32),
            blur_scale: iors.map(|ior| (2.0 * (ior - 1.0)).clamp(0.0, 1.0) as f32),
        }
    }

    pub fn dispersive(self) -> bool {
        self.inverse_ior[0] != self.inverse_ior[2]
    }
}

impl MeshPbr {
    pub fn volume(&self) -> Option<&MeshVolume> {
        self.transmission
            .as_ref()?
            .volume
            .as_deref()
            .filter(|volume| volume.thickness > 0.0)
    }

    pub fn index_of_refraction(&self) -> f32 {
        self.dielectric
            .as_ref()
            .map_or(MeshDielectric::DEFAULT_IOR, |term| term.ior)
    }

    pub fn refraction_channels(&self) -> RefractionChannels {
        RefractionChannels::new(
            self.index_of_refraction(),
            self.volume().map_or(0.0, |volume| volume.dispersion),
        )
    }

    pub(in crate::scene3d) fn metalness_texture(&self) -> Option<&MeshTexture> {
        self.separate_surface_maps
            .as_deref()
            .and_then(|maps| maps.metalness.as_ref())
            .or(self.surface_texture.as_ref())
    }

    /// Select the UV set of the mesh's authored or generated tangent frame.
    pub fn tangent_normal_map(&self) -> Option<&MeshNormalMap> {
        self.normal_map
            .as_ref()
            .or_else(|| self.clearcoat.as_ref()?.normal_map.as_ref())
    }

    pub(in crate::scene3d) fn is_valid(&self) -> bool {
        self.fresnel.is_valid()
            && self
                .anisotropy
                .as_deref()
                .is_none_or(MeshAnisotropy::is_valid)
            && [self.metallic, self.roughness, self.occlusion_strength]
                .into_iter()
                .all(|value| (0.0..=1.0).contains(&value))
            && self.emissive.into_iter().all(|v| v.is_finite() && v >= 0.0)
            && [
                &self.surface_texture,
                &self.emissive_texture,
                &self.occlusion_texture,
            ]
            .into_iter()
            .flatten()
            .all(MeshTexture::is_valid)
            && self.separate_surface_maps.as_deref().is_none_or(|maps| {
                self.workflow == MeshWorkflow::MetallicRoughness
                    && [&maps.metalness, &maps.roughness]
                        .into_iter()
                        .flatten()
                        .all(MeshTexture::is_valid)
            })
            && self
                .specular_reflection
                .as_deref()
                .is_none_or(|reflection| {
                    reflection.image.as_ref().is_none_or(MeshTexture::is_valid)
                })
            && self
                .height_map
                .as_deref()
                .is_none_or(MeshHeightMap::is_valid)
            && self.sheen.as_deref().is_none_or(|sheen| {
                self.workflow == MeshWorkflow::MetallicRoughness && sheen.is_valid()
            })
            && self.iridescence.as_deref().is_none_or(|film| {
                self.workflow == MeshWorkflow::MetallicRoughness && film.is_valid()
            })
            && self.normal_map.as_ref().is_none_or(MeshNormalMap::is_valid)
            && self.clearcoat.as_ref().is_none_or(MeshClearcoat::is_valid)
            && self
                .dielectric
                .as_deref()
                .is_none_or(MeshDielectric::is_valid)
            && self
                .transmission
                .as_deref()
                .is_none_or(MeshTransmission::is_valid)
    }
}

impl MeshNormalMap {
    fn is_valid(&self) -> bool {
        self.scale.is_finite() && self.texture.is_valid()
    }
}

impl MeshClearcoat {
    fn is_valid(&self) -> bool {
        self.fresnel.is_valid()
            && [self.amount, self.roughness]
                .into_iter()
                .all(|value| (0.0..=1.0).contains(&value))
            && [&self.amount_texture, &self.roughness_texture]
                .into_iter()
                .flatten()
                .all(MeshTexture::is_valid)
            && self.normal_map.as_ref().is_none_or(MeshNormalMap::is_valid)
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum MeshAlphaMode {
    /// Preserve source alpha when another material or instance setting enables blending.
    Default,
    Opaque,
    Mask(f32),
    Blend,
    #[doc(hidden)]
    InstanceBlend {
        cutoff: Option<f32>,
        builtin: bool,
        source_alpha: bool,
    },
}

impl MeshAlphaMode {
    pub fn cutoff(self) -> Option<f32> {
        match self {
            Self::Mask(cutoff) => Some(cutoff),
            Self::InstanceBlend { cutoff, .. } => cutoff,
            _ => None,
        }
    }
}

/// Original material identity from a source asset, independent of draw order.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MeshMaterialSlot {
    pub source_index: Option<u32>,
    pub name: Option<String>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
enum SlotSelector {
    Default,
    Index(u32),
    Name(String),
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct SceneMaterialBinding {
    target: SlotSelector,
    style: SceneMaterial,
}

impl SceneMaterialBinding {
    pub fn new(slot: &str, style: SceneMaterial) -> Option<Self> {
        let target = match slot {
            "" => return None,
            "#default" => SlotSelector::Default,
            value if value.starts_with("name:") => SlotSelector::Name(value[5..].to_owned()),
            value if value.starts_with('#') => SlotSelector::Index(value[1..].parse().ok()?),
            value => SlotSelector::Name(value.to_owned()),
        };
        Some(Self {
            target,
            style: style.normalized(),
        })
    }

    pub fn for_field(field: &str, slot: Option<&str>, style: SceneMaterial) -> Option<Self> {
        if let Some(slot) = slot.filter(|slot| !slot.is_empty()) {
            return Self::new(slot, style);
        }
        let target = if field == "default-slot" {
            SlotSelector::Default
        } else if let Some(index) = field
            .strip_prefix("slot-")
            .and_then(|text| text.parse().ok())
        {
            SlotSelector::Index(index)
        } else {
            SlotSelector::Name(field.to_owned())
        };
        Some(Self {
            target,
            style: style.normalized(),
        })
    }

    fn matches(&self, slot: Option<&MeshMaterialSlot>, fallback: u32) -> bool {
        match (&self.target, slot) {
            (SlotSelector::Default, Some(slot)) => slot.source_index.is_none(),
            (SlotSelector::Index(index), Some(slot)) => slot.source_index == Some(*index),
            (SlotSelector::Index(index), None) => *index == fallback,
            (SlotSelector::Name(name), Some(slot)) => slot.name.as_ref() == Some(name),
            _ => false,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct SceneAppearance {
    pub material: Option<SceneMaterial>,
    pub bindings: Vec<SceneMaterialBinding>,
}

impl SceneAppearance {
    pub(crate) fn new(
        material: Option<SceneMaterial>,
        bindings: Vec<SceneMaterialBinding>,
    ) -> Option<Arc<Self>> {
        if material.is_none() && bindings.is_empty() {
            None
        } else {
            Some(Arc::new(Self {
                material: material.map(SceneMaterial::normalized),
                bindings,
            }))
        }
    }

    pub(crate) fn styles<'a>(
        &'a self,
        slot: Option<&'a MeshMaterialSlot>,
        index: usize,
    ) -> impl Iterator<Item = &'a SceneMaterial> {
        self.material.iter().chain(
            self.bindings
                .iter()
                .filter(move |binding| binding.matches(slot, index as u32))
                .map(|binding| &binding.style),
        )
    }
}

/// Per-node authored factors. Unspecified factors inherit the source material.
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct SceneMaterial {
    pub point_size: Option<f64>,
    pub line_width: Option<f64>,
    pub base_item: Option<Option<Arc<SceneItemTexture>>>,
    pub specular_reflection_item: Option<Option<Arc<SceneItemTexture>>>,
    pub channels: MaterialChannels,
    pub vertex_colors: Option<bool>,
    pub vertex_masks: VertexColorMasks,
    pub maps: Option<Arc<SceneMaterialMaps>>,
    pub normal_strength: Option<f64>,
    pub clearcoat_normal_strength: Option<f64>,
    pub base_color: Option<Color>,
    pub opacity: Option<f64>,
    pub invert_opacity_map_value: Option<bool>,
    pub height_amount: Option<f64>,
    pub sheen_color: Option<Color>,
    pub sheen_roughness: Option<f64>,
    pub iridescence_factor: Option<f64>,
    pub iridescence_index_of_refraction: Option<f64>,
    pub iridescence_thickness_minimum: Option<f64>,
    pub iridescence_thickness_maximum: Option<f64>,
    pub min_height_map_samples: Option<i64>,
    pub max_height_map_samples: Option<i64>,
    pub workflow: Option<MeshWorkflow>,
    pub glossiness: Option<f64>,
    pub metalness: Option<f64>,
    pub anisotropy_strength: Option<f64>,
    pub anisotropy_rotation: Option<f64>,
    pub roughness: Option<f64>,
    pub emissive_color: Option<Color>,
    pub emissive_factor: Option<Vector3D>,
    pub emissive_strength: Option<f64>,
    pub alpha_mode: Option<MeshAlphaMode>,
    pub blend_mode: Option<MeshBlendMode>,
    pub depth_draw_mode: Option<MeshDepthDraw>,
    pub alpha_cutoff: Option<f64>,
    pub cull_mode: Option<MeshCullMode>,
    pub unlit: Option<bool>,
    pub clearcoat: Option<f64>,
    pub clearcoat_roughness: Option<f64>,
    pub transmission: Option<f64>,
    pub index_of_refraction: Option<f64>,
    pub clearcoat_fresnel_scale_bias_enabled: Option<bool>,
    pub clearcoat_fresnel_bias: Option<f64>,
    pub clearcoat_fresnel_scale: Option<f64>,
    pub clearcoat_fresnel_power: Option<f64>,
    pub fresnel_scale_bias_enabled: Option<bool>,
    pub fresnel_bias: Option<f64>,
    pub fresnel_scale: Option<f64>,
    pub fresnel_power: Option<f64>,
    pub dispersion: Option<f64>,
    pub attenuation_distance: Option<f64>,
    pub attenuation_color: Option<Color>,
    pub thickness: Option<f64>,
    pub specular_color: Option<Color>,
    pub specular_amount: Option<f64>,
    pub specular_tint: Option<f64>,
    pub occlusion_amount: Option<f64>,
    pub material_shader: Option<Option<Arc<str>>>,
    pub shader_parameters: Option<Vector3D>,
    pub shader_displacement: Option<f64>,
    pub shader_textures: Option<Arc<[BufferInput]>>,
}

impl SceneMaterial {
    pub fn alpha(name: &str, cutoff: Option<f64>) -> Option<MeshAlphaMode> {
        match name {
            "default" => Some(MeshAlphaMode::Default),
            "opaque" => Some(MeshAlphaMode::Opaque),
            "mask" => Some(MeshAlphaMode::Mask(factor(cutoff, 0.5, 0.0, 1.0))),
            "blend" => Some(MeshAlphaMode::Blend),
            _ => None,
        }
    }

    pub(crate) fn from_record(
        record: &RecordValue,
        textures: &impl Fn(&str) -> Option<TextureAsset3D>,
        shaders: &impl Fn(&str) -> Option<Arc<str>>,
    ) -> Option<Self> {
        let boolean = |name| match record.field(name) {
            Some(Value::Bool(value)) => Some(*value),
            _ => None,
        };
        if boolean("enabled") == Some(false) {
            return None;
        }
        let number = |name| match record.field(name) {
            Some(Value::Float(value)) => Some(*value),
            _ => None,
        };
        let color = |name| match record.field(name) {
            Some(Value::Color(value)) => Some(*value),
            _ => None,
        };
        let alpha_cutoff = number("alpha-cutoff");
        Some(Self {
            material_shader: match record.field("material-shader") {
                Some(Value::String(key)) => Some(shaders(key)),
                _ => None,
            },
            shader_parameters: match record.field("shader-parameters") {
                Some(Value::Vector3D(parameters)) => Some(*parameters),
                _ => None,
            },
            shader_displacement: number("shader-displacement"),
            shader_textures: match record.field("shader-textures") {
                Some(Value::RecordList(list)) => {
                    Some(record_buffer_inputs(list, MAX_TEXTURES, textures).into())
                }
                _ => None,
            },
            base_item: record
                .field("base-item")
                .and_then(|value| SceneTexture::authored(value, SceneTexture::item_defaults()))
                .map(SceneTexture::prepare_item),
            specular_reflection_item: record
                .field("specular-reflection-item")
                .and_then(|value| SceneTexture::authored(value, SceneTexture::item_defaults()))
                .map(SceneTexture::prepare_item),
            maps: SceneMaterialMaps::from_record(record, textures),
            channels: MaterialChannels::from_fields(CHANNEL_FIELDS.iter().filter_map(
                |(name, _)| match record.field(name)? {
                    Value::String(value) => Some((*name, value.as_str())),
                    _ => None,
                },
            )),
            vertex_colors: boolean("vertex-colors"),
            vertex_masks: VertexColorMasks::from_fields(VERTEX_MASK_FIELDS.iter().filter_map(
                |name| match record.field(name)? {
                    Value::String(value) => Some((*name, value.as_str())),
                    _ => None,
                },
            )),
            point_size: number("point-size"),
            line_width: number("line-width"),
            normal_strength: number("normal-strength"),
            clearcoat_normal_strength: number("clearcoat-normal-strength"),
            base_color: color("base-color"),
            opacity: number("opacity"),
            invert_opacity_map_value: boolean("invert-opacity-map-value"),
            height_amount: number("height-amount"),
            sheen_color: color("sheen-color"),
            sheen_roughness: number("sheen-roughness"),
            iridescence_factor: number("iridescence-factor"),
            iridescence_index_of_refraction: number("iridescence-index-of-refraction"),
            iridescence_thickness_minimum: number("iridescence-thickness-minimum"),
            iridescence_thickness_maximum: number("iridescence-thickness-maximum"),
            min_height_map_samples: match record.field("min-height-map-samples") {
                Some(Value::Int(value)) => Some(*value),
                _ => None,
            },
            max_height_map_samples: match record.field("max-height-map-samples") {
                Some(Value::Int(value)) => Some(*value),
                _ => None,
            },
            workflow: match record.field("material-workflow") {
                Some(Value::String(name)) => MeshWorkflow::named(name),
                _ => None,
            },
            glossiness: number("glossiness"),
            metalness: number("metalness"),
            anisotropy_strength: number("anisotropy-strength"),
            anisotropy_rotation: number("anisotropy-rotation"),
            roughness: number("roughness"),
            emissive_color: color("emissive-color"),
            emissive_factor: match record.field("emissive-factor") {
                Some(Value::Vector3D(factor)) => Some(*factor),
                _ => None,
            },
            emissive_strength: number("emissive-strength"),
            alpha_mode: match record.field("alpha-mode") {
                Some(Value::String(name)) => Self::alpha(name, alpha_cutoff),
                _ => None,
            },
            blend_mode: match record.field("blend-mode") {
                Some(Value::String(name)) => MeshBlendMode::named(name),
                _ => None,
            },
            depth_draw_mode: match record.field("depth-draw-mode") {
                Some(Value::String(name)) => MeshDepthDraw::named(name),
                _ => None,
            },
            alpha_cutoff,
            cull_mode: match record.field("cull-mode") {
                Some(Value::String(name)) => MeshCullMode::named(name),
                _ => None,
            },
            unlit: boolean("unlit"),
            clearcoat: number("clearcoat"),
            clearcoat_roughness: number("clearcoat-roughness"),
            transmission: number("transmission"),
            index_of_refraction: number("index-of-refraction"),
            clearcoat_fresnel_scale_bias_enabled: boolean("clearcoat-fresnel-scale-bias-enabled"),
            clearcoat_fresnel_bias: number("clearcoat-fresnel-bias"),
            clearcoat_fresnel_scale: number("clearcoat-fresnel-scale"),
            clearcoat_fresnel_power: number("clearcoat-fresnel-power"),
            fresnel_scale_bias_enabled: boolean("fresnel-scale-bias-enabled"),
            fresnel_bias: number("fresnel-bias"),
            fresnel_scale: number("fresnel-scale"),
            fresnel_power: number("fresnel-power"),
            dispersion: number("dispersion"),
            attenuation_distance: number("attenuation-distance"),
            attenuation_color: color("attenuation-color"),
            thickness: number("thickness"),
            specular_color: color("specular-color"),
            specular_amount: number("specular-amount"),
            specular_tint: number("specular-tint"),
            occlusion_amount: number("occlusion-amount"),
        })
    }

    pub(crate) fn normalized(mut self) -> Self {
        let clamp = |value: Option<f64>, min: f64, max: f64| {
            value
                .filter(|value| value.is_finite())
                .map(|value| f64::from(value.clamp(min, max) as f32))
        };
        self.metalness = clamp(self.metalness, 0.0, 1.0);
        self.anisotropy_strength = clamp(self.anisotropy_strength, 0.0, 1.0);
        self.anisotropy_rotation = self
            .anisotropy_rotation
            .filter(|rotation| rotation.is_finite())
            .map(|rotation| rotation.rem_euclid(360.0));
        self.roughness = clamp(self.roughness, 0.0, 1.0);
        self.glossiness = clamp(self.glossiness, 0.0, 1.0);
        self.clearcoat = clamp(self.clearcoat, 0.0, 1.0);
        self.clearcoat_roughness = clamp(self.clearcoat_roughness, 0.0, 1.0);
        self.transmission = clamp(self.transmission, 0.0, 1.0);
        self.normal_strength = clamp(self.normal_strength, -8.0, 8.0);
        self.clearcoat_normal_strength = clamp(self.clearcoat_normal_strength, -8.0, 8.0);
        self.alpha_cutoff = clamp(self.alpha_cutoff, 0.0, 1.0);
        self.opacity = clamp(self.opacity, 0.0, 1.0);
        self.shader_displacement = clamp(self.shader_displacement, 0.0, 10_000.0);
        self.emissive_strength = clamp(self.emissive_strength, 0.0, 64.0);
        self.index_of_refraction = clamp(self.index_of_refraction, 1.0, 3.0);
        self.occlusion_amount = clamp(self.occlusion_amount, 0.0, 1.0);
        self.specular_amount = clamp(self.specular_amount, 0.0, 1.0);
        self.specular_tint = clamp(self.specular_tint, 0.0, 1.0);
        let [min_power, max_power] = MeshFresnel::POWER_RANGE;
        let [min_scale, max_scale] = MeshFresnel::SCALE_RANGE;
        let [min_bias, max_bias] = MeshFresnel::BIAS_RANGE;
        self.fresnel_power = clamp(self.fresnel_power, min_power, max_power);
        self.fresnel_scale = clamp(self.fresnel_scale, min_scale, max_scale);
        self.fresnel_bias = clamp(self.fresnel_bias, min_bias, max_bias);
        self.clearcoat_fresnel_power = clamp(self.clearcoat_fresnel_power, min_power, max_power);
        self.clearcoat_fresnel_scale = clamp(self.clearcoat_fresnel_scale, min_scale, max_scale);
        self.clearcoat_fresnel_bias = clamp(self.clearcoat_fresnel_bias, min_bias, max_bias);
        let upper = f64::from(f32::MAX);
        self.emissive_factor = self.emissive_factor.map(|factor| {
            let [x, y, z] = [factor.x, factor.y, factor.z]
                .map(|component| clamp(Some(component), 0.0, upper).unwrap_or(0.0));
            Vector3D::new(x, y, z)
        });
        self.thickness = clamp(self.thickness, 0.0, upper);
        self.attenuation_distance = clamp(self.attenuation_distance, 0.0, upper);
        self.dispersion = clamp(self.dispersion, 0.0, upper);
        self.sheen_roughness = clamp(self.sheen_roughness, 0.0, 1.0);
        self.iridescence_factor = clamp(self.iridescence_factor, 0.0, 1.0);
        self.iridescence_index_of_refraction =
            clamp(self.iridescence_index_of_refraction, 1.0, upper);
        self.iridescence_thickness_minimum = clamp(self.iridescence_thickness_minimum, 0.0, upper);
        self.iridescence_thickness_maximum = clamp(self.iridescence_thickness_maximum, 0.0, upper);
        self
    }

    // Resolve fields before applying them to imported factors. In particular,
    // an emissive-strength override must multiply the final authored color once.
    pub(crate) fn overlay(&mut self, later: &Self) {
        if let Some(item) = &later.base_item {
            self.base_item = Some(item.clone());
        }
        if let Some(item) = &later.specular_reflection_item {
            self.specular_reflection_item = Some(item.clone());
        }
        self.channels.overlay(later.channels);
        self.vertex_masks.overlay(later.vertex_masks);
        if let Some(shader) = &later.material_shader {
            self.material_shader = Some(shader.clone());
        }
        if let Some(textures) = &later.shader_textures {
            self.shader_textures = Some(textures.clone());
        }
        if later.emissive_factor.is_some() || later.emissive_color.is_some() {
            self.emissive_factor = later.emissive_factor;
            self.emissive_color = later.emissive_color;
        }
        macro_rules! overlay_factors {
            ($($field:ident),+ $(,)?) => {
                $(self.$field = later.$field.or(self.$field);)+
            };
        }
        overlay_factors!(
            vertex_colors,
            base_color,
            opacity,
            invert_opacity_map_value,
            height_amount,
            sheen_color,
            sheen_roughness,
            iridescence_factor,
            iridescence_index_of_refraction,
            iridescence_thickness_minimum,
            iridescence_thickness_maximum,
            min_height_map_samples,
            max_height_map_samples,
            workflow,
            glossiness,
            metalness,
            anisotropy_strength,
            anisotropy_rotation,
            roughness,
            emissive_strength,
            alpha_mode,
            blend_mode,
            depth_draw_mode,
            alpha_cutoff,
            cull_mode,
            unlit,
            clearcoat,
            clearcoat_roughness,
            transmission,
            index_of_refraction,
            clearcoat_fresnel_scale_bias_enabled,
            clearcoat_fresnel_bias,
            clearcoat_fresnel_scale,
            clearcoat_fresnel_power,
            fresnel_scale_bias_enabled,
            fresnel_bias,
            fresnel_scale,
            fresnel_power,
            dispersion,
            attenuation_distance,
            attenuation_color,
            thickness,
            specular_color,
            specular_amount,
            specular_tint,
            occlusion_amount,
            normal_strength,
            clearcoat_normal_strength,
            shader_parameters,
            shader_displacement,
        );
        if let Some(maps) = &later.maps {
            let Some(existing) = &mut self.maps else {
                self.maps = Some(maps.clone());
                return;
            };
            let existing = Arc::make_mut(existing);
            macro_rules! overlay_maps {
                ($($field:ident),+ $(,)?) => {
                    $(if maps.$field.is_some() { existing.$field = maps.$field.clone(); })+
                };
            }
            overlay_maps!(
                base_color_map,
                opacity_map,
                height_map,
                sheen_color_map,
                sheen_roughness_map,
                iridescence_map,
                iridescence_thickness_map,
                metallic_roughness_map,
                metalness_map,
                roughness_map,
                glossiness_map,
                emissive_map,
                occlusion_map,
                normal_map,
                clearcoat_map,
                clearcoat_roughness_map,
                clearcoat_normal_map,
                transmission_map,
                thickness_map,
                anisotropy_map,
                specular_color_map,
                specular_reflection_map,
                specular_map,
            );
        }
    }

    pub(crate) fn overrides_normal_texture(&self) -> bool {
        self.maps
            .as_deref()
            .is_some_and(|maps| maps.normal_map.is_some() || maps.clearcoat_normal_map.is_some())
    }

    pub(crate) fn prepare(&self, source: &MeshMaterial) -> MeshMaterial {
        let mut result = source.clone();
        if let Some(source) = &self.material_shader {
            let parameters = result
                .shader
                .as_ref()
                .map_or([0.0; 3], |shader| shader.parameters);
            let displacement = result
                .shader
                .as_ref()
                .map_or(0.0, |shader| shader.displacement);
            let textures = result
                .shader
                .as_ref()
                .map_or_else(Arc::default, |shader| shader.textures.clone());
            result.shader = source.as_ref().map(|source| MeshShader {
                source: source.clone(),
                parameters,
                displacement,
                textures,
            });
        }
        if let (Some(textures), Some(shader)) = (&self.shader_textures, &mut result.shader) {
            shader.textures = textures.clone();
        }
        if let Some(parameters) = self.shader_parameters
            && let Some(shader) = &mut result.shader
        {
            shader.parameters = [parameters.x, parameters.y, parameters.z].map(|value| {
                let value = value as f32;
                if value.is_finite() { value } else { 0.0 }
            });
        }
        if let (Some(displacement), Some(shader)) = (self.shader_displacement, &mut result.shader) {
            shader.displacement = displacement as f32;
        }
        result.point_size = factor(self.point_size, result.point_size, 1.0, f64::from(f32::MAX));
        result.line_width = factor(self.line_width, result.line_width, 1.0, f64::from(f32::MAX));
        result.channels.overlay(self.channels);
        result.vertex_colors = self.vertex_colors.unwrap_or(source.vertex_colors);
        result.vertex_masks.overlay(self.vertex_masks);
        let empty = SceneMaterialMaps::default();
        let maps = self.maps.as_deref().unwrap_or(&empty);
        replace_map(&mut result.base_texture, &maps.base_color_map);
        replace_map(&mut result.opacity_texture, &maps.opacity_map);
        result.invert_opacity_map_value = self
            .invert_opacity_map_value
            .unwrap_or(result.invert_opacity_map_value);
        if let Some(item) = &self.base_item {
            result.base_item = item.clone();
        }
        if let Some(color) = self.base_color {
            let [r, g, b] = linear_color(color);
            result.base_color = [r, g, b, f32::from(color.0 as u8) / 255.0];
        }
        result.cull_mode = self.cull_mode.unwrap_or(result.cull_mode);
        result.alpha_mode = self.alpha_mode.unwrap_or(result.alpha_mode);
        result.opacity = factor(self.opacity, result.opacity, 0.0, 1.0);
        result.blend_mode = self.blend_mode.unwrap_or(result.blend_mode);
        result.depth_draw_mode = self.depth_draw_mode.unwrap_or(result.depth_draw_mode);
        if let MeshAlphaMode::Mask(cutoff) = &mut result.alpha_mode {
            *cutoff = factor(self.alpha_cutoff, *cutoff, 0.0, 1.0);
        }
        if self.unlit == Some(true) {
            result.pbr = None;
        } else if (self.unlit == Some(false) || self.workflow.is_some()) && result.pbr.is_none() {
            result.pbr = Some(MeshPbr::principled());
        }
        if let Some(pbr) = &mut result.pbr {
            if maps.height_map.is_some()
                || self.height_amount.is_some()
                || self.min_height_map_samples.is_some()
                || self.max_height_map_samples.is_some()
            {
                let previous = pbr.height_map.take();
                let amount = factor(
                    self.height_amount,
                    previous.as_deref().map_or(0.0, |map| map.amount),
                    0.0,
                    1.0,
                );
                let [min, max] = previous
                    .as_deref()
                    .map_or(MeshHeightMap::DEFAULT_SAMPLES, |map| map.samples);
                let steps = |value: Option<i64>, fallback| {
                    value.map_or(fallback, |n| {
                        n.clamp(1, i64::from(MeshHeightMap::MAX_SAMPLES)) as u8
                    })
                };
                let min = steps(self.min_height_map_samples, min);
                let max = steps(self.max_height_map_samples, max).max(min);
                let texture = maps
                    .height_map
                    .clone()
                    .unwrap_or_else(|| previous.map(|map| map.texture));
                pbr.height_map = texture.map(|texture| {
                    Box::new(MeshHeightMap {
                        texture,
                        amount,
                        samples: [min, max],
                    })
                });
            }
            if maps.specular_reflection_map.is_some() || self.specular_reflection_item.is_some() {
                let previous = pbr.specular_reflection.as_deref();
                let image = maps
                    .specular_reflection_map
                    .clone()
                    .unwrap_or_else(|| previous.and_then(|reflection| reflection.image.clone()));
                let item = self
                    .specular_reflection_item
                    .clone()
                    .unwrap_or_else(|| previous.and_then(|reflection| reflection.item.clone()));
                pbr.specular_reflection = (image.is_some() || item.is_some())
                    .then(|| Box::new(MeshSpecularReflection { image, item }));
            }
            pbr.fresnel = pbr.fresnel.with_overrides(
                self.fresnel_power,
                self.fresnel_scale,
                self.fresnel_bias,
                self.fresnel_scale_bias_enabled,
            );
            let previous = pbr.workflow;
            pbr.workflow = self.workflow.unwrap_or(previous);
            if pbr.workflow != previous {
                pbr.surface_texture = None;
                pbr.separate_surface_maps = None;
            }
            match pbr.workflow {
                MeshWorkflow::MetallicRoughness => {
                    pbr.metallic = factor(self.metalness, pbr.metallic, 0.0, 1.0);
                    pbr.roughness = factor(self.roughness, pbr.roughness, 0.0, 1.0);
                    replace_map(&mut pbr.surface_texture, &maps.metallic_roughness_map);
                    if maps.metalness_map.is_some() || maps.roughness_map.is_some() {
                        let previous = pbr.separate_surface_maps.as_deref();
                        let metalness = maps
                            .metalness_map
                            .clone()
                            .unwrap_or_else(|| previous.and_then(|maps| maps.metalness.clone()));
                        let roughness = maps
                            .roughness_map
                            .clone()
                            .unwrap_or_else(|| previous.and_then(|maps| maps.roughness.clone()));
                        pbr.separate_surface_maps = (metalness.is_some() || roughness.is_some())
                            .then(|| {
                                Box::new(MeshSurfaceMaps {
                                    metalness,
                                    roughness,
                                })
                            });
                    }
                }
                MeshWorkflow::SpecularGlossiness => {
                    let inherited = if previous == pbr.workflow {
                        1.0 - pbr.roughness
                    } else {
                        1.0
                    };
                    pbr.metallic = 0.0;
                    pbr.roughness = 1.0 - factor(self.glossiness, inherited, 0.0, 1.0);
                    replace_map(&mut pbr.surface_texture, &maps.glossiness_map);
                }
            }
            pbr.occlusion_strength =
                factor(self.occlusion_amount, pbr.occlusion_strength, 0.0, 1.0);
            let strength = factor(self.emissive_strength, 1.0, 0.0, 64.0);
            if let Some(emissive) = self.emissive_factor {
                let [x, y, z] = [emissive.x, emissive.y, emissive.z];
                pbr.emissive =
                    [x, y, z].map(|value| factor(Some(value), 0.0, 0.0, f64::from(f32::MAX)));
            } else if let Some(color) = self.emissive_color {
                pbr.emissive = linear_color(color);
            }
            replace_map(&mut pbr.emissive_texture, &maps.emissive_map);
            replace_map(&mut pbr.occlusion_texture, &maps.occlusion_map);
            replace_normal(&mut pbr.normal_map, &maps.normal_map, self.normal_strength);
            pbr.emissive = pbr
                .emissive
                .map(|channel| (channel * strength).min(f32::MAX));
            if self.clearcoat.is_some()
                || self.clearcoat_roughness.is_some()
                || maps.clearcoat_map.is_some()
                || maps.clearcoat_roughness_map.is_some()
                || maps.clearcoat_normal_map.is_some()
                || self.clearcoat_normal_strength.is_some()
                || self.clearcoat_fresnel_power.is_some()
                || self.clearcoat_fresnel_scale.is_some()
                || self.clearcoat_fresnel_bias.is_some()
                || self.clearcoat_fresnel_scale_bias_enabled.is_some()
            {
                let coat = pbr.clearcoat.get_or_insert(MeshClearcoat {
                    fresnel: MeshFresnel::default(),
                    amount: 0.0,
                    roughness: 0.0,
                    amount_texture: None,
                    roughness_texture: None,
                    normal_map: None,
                });
                coat.fresnel = coat.fresnel.with_overrides(
                    self.clearcoat_fresnel_power,
                    self.clearcoat_fresnel_scale,
                    self.clearcoat_fresnel_bias,
                    self.clearcoat_fresnel_scale_bias_enabled,
                );
                coat.amount = factor(self.clearcoat, coat.amount, 0.0, 1.0);
                coat.roughness = factor(self.clearcoat_roughness, coat.roughness, 0.0, 1.0);
                replace_map(&mut coat.amount_texture, &maps.clearcoat_map);
                replace_map(&mut coat.roughness_texture, &maps.clearcoat_roughness_map);
                replace_normal(
                    &mut coat.normal_map,
                    &maps.clearcoat_normal_map,
                    self.clearcoat_normal_strength,
                );
            }
            self.apply_anisotropy(pbr, maps);
            self.apply_dielectric(pbr, maps);
            self.apply_iridescence(pbr, maps);
            self.apply_sheen(pbr, maps);
            self.apply_transmission(pbr, maps);
        }
        result
    }

    fn apply_anisotropy(&self, pbr: &mut MeshPbr, maps: &SceneMaterialMaps) {
        if self.anisotropy_strength.is_none()
            && self.anisotropy_rotation.is_none()
            && maps.anisotropy_map.is_none()
        {
            return;
        }
        let anisotropy = pbr.anisotropy.get_or_insert_with(Box::default);
        anisotropy.strength = factor(self.anisotropy_strength, anisotropy.strength, 0.0, 1.0);
        if let Some(angle) = self.anisotropy_rotation.filter(|angle| angle.is_finite()) {
            anisotropy.rotation = angle.rem_euclid(360.0).to_radians() as f32;
        }
        replace_map(&mut anisotropy.texture, &maps.anisotropy_map);
    }

    fn apply_dielectric(&self, pbr: &mut MeshPbr, maps: &SceneMaterialMaps) {
        if self.index_of_refraction.is_none()
            && self.specular_amount.is_none()
            && self.specular_color.is_none()
            && self
                .specular_tint
                .is_none_or(|tint| tint == 0.0 && pbr.dielectric.is_none())
            && maps.specular_map.is_none()
            && maps.specular_color_map.is_none()
        {
            return;
        }
        let dielectric = pbr.dielectric.get_or_insert_with(|| {
            Box::new(MeshDielectric {
                ior: MeshDielectric::DEFAULT_IOR,
                specular: 1.0,
                tint: 0.0,
                color: [1.0; 3],
                specular_texture: None,
                color_texture: None,
            })
        });
        dielectric.ior = factor(self.index_of_refraction, dielectric.ior, 1.0, 3.0);
        dielectric.specular = factor(self.specular_amount, dielectric.specular, 0.0, 1.0);
        dielectric.tint = factor(self.specular_tint, dielectric.tint, 0.0, 1.0);
        if let Some(color) = self.specular_color {
            dielectric.color = linear_color(color);
        }
        replace_map(&mut dielectric.specular_texture, &maps.specular_map);
        replace_map(&mut dielectric.color_texture, &maps.specular_color_map);
    }

    fn apply_sheen(&self, pbr: &mut MeshPbr, maps: &SceneMaterialMaps) {
        if pbr.workflow != MeshWorkflow::MetallicRoughness {
            pbr.sheen = None;
            return;
        }
        if self.sheen_color.is_none()
            && self.sheen_roughness.is_none()
            && maps.sheen_color_map.is_none()
            && maps.sheen_roughness_map.is_none()
        {
            return;
        }
        let sheen = pbr.sheen.get_or_insert_with(Default::default);
        if let Some(color) = self.sheen_color {
            sheen.color = crate::scene3d::lighting::linear_color(color);
        }
        sheen.roughness = factor(self.sheen_roughness, sheen.roughness, 0.0, 1.0);
        replace_map(&mut sheen.color_texture, &maps.sheen_color_map);
        replace_map(&mut sheen.roughness_texture, &maps.sheen_roughness_map);
    }

    fn apply_iridescence(&self, pbr: &mut MeshPbr, maps: &SceneMaterialMaps) {
        if pbr.workflow != MeshWorkflow::MetallicRoughness {
            pbr.iridescence = None;
            return;
        }
        if self.iridescence_factor.is_none()
            && self.iridescence_index_of_refraction.is_none()
            && self.iridescence_thickness_minimum.is_none()
            && self.iridescence_thickness_maximum.is_none()
            && maps.iridescence_map.is_none()
            && maps.iridescence_thickness_map.is_none()
        {
            return;
        }
        let film = pbr.iridescence.get_or_insert_with(Default::default);
        let max = f64::from(f32::MAX);
        film.factor = factor(self.iridescence_factor, film.factor, 0.0, 1.0);
        film.ior = factor(self.iridescence_index_of_refraction, film.ior, 1.0, max);
        film.thickness_nm[0] = factor(
            self.iridescence_thickness_minimum,
            film.thickness_nm[0],
            0.0,
            max,
        );
        film.thickness_nm[1] = factor(
            self.iridescence_thickness_maximum,
            film.thickness_nm[1],
            0.0,
            max,
        );
        replace_map(&mut film.factor_texture, &maps.iridescence_map);
        replace_map(&mut film.thickness_texture, &maps.iridescence_thickness_map);
    }

    fn apply_transmission(&self, pbr: &mut MeshPbr, maps: &SceneMaterialMaps) {
        let volume_authored = self.thickness.is_some()
            || self.dispersion.is_some()
            || self.attenuation_color.is_some()
            || self.attenuation_distance.is_some()
            || maps.thickness_map.is_some();
        if !volume_authored && self.transmission.is_none() && maps.transmission_map.is_none() {
            return;
        }
        let term = pbr.transmission.get_or_insert_with(|| {
            Box::new(MeshTransmission {
                factor: 0.0,
                texture: None,
                volume: None,
            })
        });
        term.factor = factor(self.transmission, term.factor, 0.0, 1.0);
        replace_map(&mut term.texture, &maps.transmission_map);
        if !volume_authored {
            return;
        }
        let volume = term.volume.get_or_insert_with(|| {
            Box::new(MeshVolume {
                thickness: 0.0,
                thickness_in_prepared_space: false,
                dispersion: 0.0,
                texture: None,
                attenuation_distance: None,
                attenuation_color: [1.0; 3],
            })
        });
        let upper = f64::from(f32::MAX);
        if self.thickness.is_some_and(f64::is_finite) {
            volume.thickness = factor(self.thickness, volume.thickness, 0.0, upper);
            volume.thickness_in_prepared_space = true;
        }
        volume.dispersion = factor(self.dispersion, volume.dispersion, 0.0, upper);
        if let Some(distance) = self
            .attenuation_distance
            .filter(|distance| distance.is_finite())
        {
            let distance = distance.clamp(0.0, upper) as f32;
            volume.attenuation_distance = (distance > 0.0).then_some(distance);
        }
        if let Some(color) = self.attenuation_color {
            volume.attenuation_color = linear_color(color);
        }
        replace_map(&mut volume.texture, &maps.thickness_map);
    }
}

fn replace_map(target: &mut Option<MeshTexture>, map: &Option<Option<MeshTexture>>) {
    if let Some(map) = map {
        *target = map.clone();
    }
}

fn replace_normal(
    target: &mut Option<MeshNormalMap>,
    map: &Option<Option<MeshTexture>>,
    strength: Option<f64>,
) {
    let scale = factor(
        strength,
        target.as_ref().map_or(1.0, |map| map.scale),
        -8.0,
        8.0,
    );
    if let Some(map) = map {
        *target = map.clone().map(|texture| MeshNormalMap { texture, scale });
    } else if let Some(target) = target {
        target.scale = scale;
    }
}

fn factor(value: Option<f64>, default: f32, min: f64, max: f64) -> f32 {
    value
        .filter(|value| value.is_finite())
        .map_or(default, |value| value.clamp(min, max) as f32)
}

impl MeshPbr {
    pub(crate) fn principled() -> Self {
        Self {
            workflow: MeshWorkflow::MetallicRoughness,
            anisotropy: None,
            fresnel: MeshFresnel::default(),
            metallic: 0.0,
            roughness: 0.5,
            emissive: [0.0; 3],
            occlusion_strength: 1.0,
            surface_texture: None,
            height_map: None,
            iridescence: None,
            sheen: None,
            specular_reflection: None,
            separate_surface_maps: None,
            emissive_texture: None,
            occlusion_texture: None,
            normal_map: None,
            clearcoat: None,
            dielectric: None,
            transmission: None,
        }
    }
}

impl MeshMaterial {
    /// Mesh-space optical basis required by a glTF thickness value.
    #[doc(hidden)]
    pub fn volume_inverse(&self, node_inverse: Option<[[f32; 3]; 3]>) -> Option<[[f32; 3]; 3]> {
        node_inverse.filter(|_| {
            self.pbr
                .as_ref()
                .and_then(MeshPbr::volume)
                .is_some_and(|volume| !volume.thickness_in_prepared_space)
        })
    }

    pub fn is_blended(&self) -> bool {
        self.opacity < 1.0
            || self.opacity_texture.is_some()
            || matches!(
                self.alpha_mode,
                MeshAlphaMode::Blend | MeshAlphaMode::InstanceBlend { .. }
            )
            || self.blend_mode != MeshBlendMode::SourceOver
    }

    /// The coverage mode for this prepared material. Instance fades prepare their
    /// own material variant before classification.
    #[doc(hidden)]
    pub fn effective_alpha_mode(&self) -> MeshAlphaMode {
        match self.alpha_mode {
            MeshAlphaMode::Default if self.is_blended() => MeshAlphaMode::Blend,
            MeshAlphaMode::Default => MeshAlphaMode::Opaque,
            MeshAlphaMode::InstanceBlend {
                source_alpha: true, ..
            } => MeshAlphaMode::Blend,
            mode => mode,
        }
    }

    #[doc(hidden)]
    pub fn is_visible(&self) -> bool {
        self.opacity > 0.0
    }

    #[doc(hidden)]
    pub fn transmission_blur(&self) -> Option<f32> {
        if !self.is_visible() {
            return None;
        }
        let pbr = self.pbr.as_ref()?;
        pbr.transmission.as_ref().filter(|term| term.factor > 0.0)?;
        if pbr.metallic >= 1.0
            && pbr.metalness_texture().is_none()
            && !self.vertex_masks.masks_metalness()
        {
            return None;
        }
        let roughness =
            if pbr.workflow == MeshWorkflow::SpecularGlossiness && pbr.surface_texture.is_some() {
                1.0
            } else {
                pbr.roughness
            };
        Some(roughness * pbr.refraction_channels().blur_scale[2])
    }

    #[doc(hidden)]
    pub fn depth_coverage(&self) -> Option<MeshDepthCoverage> {
        if !self.is_visible()
            || self.depth_draw_mode == MeshDepthDraw::Never
            || self.depth_draw_mode == MeshDepthDraw::OpaqueOnly
                && (self.is_blended() || self.transmission_blur().is_some())
        {
            return None;
        }
        if self.opacity < 1.0 {
            return (self.depth_draw_mode == MeshDepthDraw::Always)
                .then_some(MeshDepthCoverage::NonzeroAlpha);
        }
        if self.opacity_texture.is_some() {
            return Some(match self.depth_draw_mode {
                MeshDepthDraw::OpaquePrepass => MeshDepthCoverage::OpaqueAlpha,
                _ => MeshDepthCoverage::NonzeroAlpha,
            });
        }
        Some(match self.effective_alpha_mode() {
            MeshAlphaMode::Default | MeshAlphaMode::Opaque => MeshDepthCoverage::Opaque,
            MeshAlphaMode::Mask(_) => MeshDepthCoverage::Mask,
            MeshAlphaMode::Blend | MeshAlphaMode::InstanceBlend { .. }
                if self.depth_draw_mode == MeshDepthDraw::OpaquePrepass =>
            {
                MeshDepthCoverage::OpaqueAlpha
            }
            MeshAlphaMode::Blend | MeshAlphaMode::InstanceBlend { .. } => {
                MeshDepthCoverage::NonzeroAlpha
            }
        })
    }

    #[doc(hidden)]
    pub fn vertex_program(&self) -> Option<&Arc<str>> {
        self.shader.as_ref()?.vertex_program()
    }

    #[doc(hidden)]
    pub fn vertex_parameters(&self) -> [f32; 4] {
        self.shader
            .as_ref()
            .filter(|shader| shader.vertex_program().is_some())
            .map_or([0.0; 4], MeshShader::vertex_parameters)
    }

    pub(crate) fn same_shadow_surface(&self, other: &Self) -> bool {
        let displacement = self.vertex_parameters();
        let other_displacement = other.vertex_parameters();
        if (displacement[3] != 0.0 || other_displacement[3] != 0.0)
            && (displacement != other_displacement
                || self.vertex_program() != other.vertex_program())
        {
            return false;
        }
        let coverage = self.depth_coverage();
        if coverage != other.depth_coverage() {
            return false;
        }
        coverage.is_none_or(|coverage| {
            self.cull_mode == other.cull_mode
                && (!coverage.samples_alpha()
                    || self.effective_alpha_mode() == other.effective_alpha_mode()
                        && self.base_color[3] == other.base_color[3]
                        && self.vertex_colors == other.vertex_colors
                        && self.base_item == other.base_item
                        && self.alpha_texture() == other.alpha_texture()
                        && self.opacity_texture == other.opacity_texture
                        && (self.opacity_texture.is_none()
                            || self.invert_opacity_map_value == other.invert_opacity_map_value
                                && self.channels.channel(MaterialChannel::Opacity)
                                    == other.channels.channel(MaterialChannel::Opacity)))
        })
    }

    fn alpha_texture(&self) -> Option<&MeshTexture> {
        if self.channels.channel(MaterialChannel::BaseColor) == TextureChannel::Rgb {
            self.base_texture.as_ref()
        } else {
            None
        }
    }

    fn effective_alpha(&self, uv: [f32; 2], vertex_alpha: f32, tint_alpha: f32) -> f32 {
        let map = if self.base_item.is_some() {
            // Live UI coverage is GPU-only. CPU picking uses the surface factors.
            1.0
        } else {
            self.alpha_texture().map_or(1.0, |tex| tex.alpha_at(uv))
        };
        let alpha = if self.vertex_colors {
            vertex_alpha
        } else {
            1.0
        };
        tint_alpha * (self.base_color[3] * alpha * map)
    }

    fn mapped_opacity(&self, uv: [[f32; 2]; 2]) -> f32 {
        self.opacity
            * self.opacity_texture.as_ref().map_or(1.0, |texture| {
                let value = texture.scalar_at(
                    uv[usize::from(texture.uv_slot)],
                    self.channels.channel(MaterialChannel::Opacity),
                );
                if self.invert_opacity_map_value {
                    1.0 - value
                } else {
                    value
                }
            })
    }

    pub(in crate::scene3d) fn tests_fragment_coverage(&self) -> bool {
        self.effective_alpha_mode() != MeshAlphaMode::Opaque || self.opacity_texture.is_some()
    }

    pub(in crate::scene3d) fn covers(
        &self,
        uv: [[f32; 2]; 2],
        vertex_alpha: f32,
        tint_alpha: f32,
    ) -> bool {
        self.mapped_opacity(uv) > 0.0 && self.alpha_covers(uv[0], vertex_alpha, tint_alpha)
    }

    fn alpha_covers(&self, uv: [f32; 2], vertex_alpha: f32, tint_alpha: f32) -> bool {
        match self.effective_alpha_mode() {
            MeshAlphaMode::Default | MeshAlphaMode::Opaque => true,
            MeshAlphaMode::Mask(cutoff) => {
                self.effective_alpha(uv, vertex_alpha, tint_alpha) >= cutoff
            }
            MeshAlphaMode::Blend => self.effective_alpha(uv, vertex_alpha, tint_alpha) > 0.0,
            MeshAlphaMode::InstanceBlend { cutoff, .. } => {
                tint_alpha > 0.0
                    && cutoff
                        .is_none_or(|cutoff| self.effective_alpha(uv, vertex_alpha, 1.0) >= cutoff)
            }
        }
    }

    pub(in crate::scene3d) fn depth_covers(
        &self,
        uv: [[f32; 2]; 2],
        vertex_alpha: f32,
        tint_alpha: f32,
    ) -> bool {
        match self.depth_coverage() {
            None => false,
            Some(MeshDepthCoverage::Opaque) => true,
            Some(MeshDepthCoverage::OpaqueAlpha) => {
                let opacity = self.mapped_opacity(uv);
                let coverage = match self.effective_alpha_mode() {
                    MeshAlphaMode::InstanceBlend { .. } => tint_alpha,
                    MeshAlphaMode::Blend => self.effective_alpha(uv[0], vertex_alpha, tint_alpha),
                    _ => 1.0,
                };
                opacity * coverage >= 1.0 && self.alpha_covers(uv[0], vertex_alpha, tint_alpha)
            }
            Some(MeshDepthCoverage::Mask | MeshDepthCoverage::NonzeroAlpha) => {
                self.covers(uv, vertex_alpha, tint_alpha)
            }
        }
    }
}

/// Primitive topology for a contiguous mesh section. Strips and loops are expanded on import.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MeshTopology {
    #[default]
    Triangles,
    Lines,
    Points,
}

impl MeshTopology {
    pub const fn vertices_per_primitive(self) -> usize {
        match self {
            Self::Triangles => 3,
            Self::Lines => 2,
            Self::Points => 1,
        }
    }
}

/// Per-part shading imposed by the primitive's available vertex attributes.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Hash)]
pub enum MeshLighting {
    #[default]
    Material,
    VertexNormal,
    BaseAndEmission,
}

#[derive(Clone, Debug)]
pub struct MeshPart {
    pub lighting: MeshLighting,
    pub topology: MeshTopology,
    pub vertices: Range<usize>,
    pub material: usize,
    /// Maps directions from the prepared geometry back into the glTF mesh space.
    /// Only needed when a static node transform was baked into volume geometry.
    pub volume_inverse: Option<[[f32; 3]; 3]>,
}

#[cfg(test)]
mod tests {
    use super::RefractionChannels;

    #[test]
    fn dispersion_scales_rgb_ior_and_clamps_extreme_red_indices() {
        let rgb = RefractionChannels::new(1.6, 16.0);
        let expected = [1.36_f32, 1.6, 1.84].map(f32::recip);
        assert!(rgb.dispersive());
        assert!(
            rgb.inverse_ior
                .into_iter()
                .zip(expected)
                .all(|(a, b)| (a - b).abs() < 1e-6)
        );
        assert!(rgb.blur_scale[0] < rgb.blur_scale[2]);

        for (ior, dispersion) in [(0.0, 10.0), (1.0, 10.0), (1.5, 0.0)] {
            let channels = RefractionChannels::new(ior, dispersion);
            assert!(!channels.dispersive());
            let expected = if ior == 0.0 { 0.0 } else { ior.recip() };
            assert_eq!(channels.inverse_ior, [expected; 3]);
        }
        let extreme = RefractionChannels::new(f32::MAX, f32::MAX);
        assert_eq!(extreme.inverse_ior[0], 1.0);
        assert!(
            extreme
                .inverse_ior
                .into_iter()
                .all(|ior| (0.0..=1.0).contains(&ior))
        );
        assert_eq!(extreme.blur_scale[2], 1.0);
    }
}

#[cfg(test)]
mod material_slot_tests {
    use super::*;

    #[test]
    fn emissive_factors_feed_the_lightmap_and_yield_to_later_color_overrides() {
        use crate::scene3d::{Mesh3D, SceneObject};

        let source = Mesh3D::Box.material_source().unwrap();
        let mesh = SceneObject::cuboid("emitter", [0.0; 3], [1.0; 3], [0.0; 3], Color(0xffffffff))
            .unwrap();
        let mut style = SceneMaterial {
            emissive_factor: Some(Vector3D::new(2.0, 0.25, -4.0)),
            emissive_color: Some(Color(0xff0000ff)),
            emissive_strength: Some(0.5),
            ..SceneMaterial::default()
        }
        .normalized();
        let baked = |material: &SceneMaterial| {
            let prepared = material.prepare(&source.materials[0]);
            mesh.bake_sample(0, [1.0, 0.0, 0.0], Some(&prepared), false)
                .unwrap()
                .emission
        };
        assert_eq!(baked(&style), [1.0, 0.125, 0.0]);
        style.overlay(&SceneMaterial {
            emissive_color: Some(Color(0x00ff00ff)),
            ..SceneMaterial::default()
        });
        assert_eq!(baked(&style), [0.0, 0.5, 0.0]);
    }

    #[test]
    fn texture_pivot_and_flips_compose_in_sampling_space() {
        let image = Arc::new(MeshImage::new([1, 1], vec![255; 4]).unwrap());
        let resolve = |_: &str| Some(Arc::clone(&image));
        let texture = SceneTexture {
            uv_scale_u: 2.0,
            uv_scale_v: 3.0,
            uv_pivot_u: 0.25,
            uv_pivot_v: 0.5,
            uv_offset_u: 0.125,
            uv_offset_v: -0.25,
            uv_rotation: 90.0,
            uv_flip_u: true,
            uv_flip_v: true,
            ..SceneTexture::default()
        }
        .prepare(true, resolve)
        .unwrap();
        for (actual, expected) in [
            (texture.transform.map([0.30, 0.40]), [0.325, 0.65]),
            (texture.transform.map([0.25, 0.50]), [0.625, 0.75]),
            (texture.transform.map_linear([0.1, 0.2]), [0.60, -0.20]),
        ] {
            assert!((actual[0] - expected[0]).abs() < 1e-6, "{actual:?}");
            assert!((actual[1] - expected[1]).abs() < 1e-6, "{actual:?}");
        }
        let unchanged = SceneTexture {
            uv_pivot_u: 0.25,
            uv_pivot_v: 0.50,
            ..SceneTexture::default()
        }
        .prepare(true, resolve)
        .unwrap();
        assert_eq!(unchanged.transform, TextureTransform::IDENTITY);
    }

    #[test]
    fn inverted_opacity_changes_sampled_depth_and_respects_the_cutoff() {
        let image =
            Arc::new(MeshImage::new([2, 1], vec![255, 255, 255, 0, 255, 255, 255, 255]).unwrap());
        let mut plain = crate::scene3d::Mesh3D::Box
            .material_source()
            .unwrap()
            .materials[0]
            .clone();
        plain.opacity_texture = SceneTexture {
            mag_filter: "nearest",
            ..SceneTexture::default()
        }
        .prepare(false, |_| Some(Arc::clone(&image)));
        plain.alpha_mode = MeshAlphaMode::Mask(0.5);
        plain.depth_draw_mode = MeshDepthDraw::OpaquePrepass;
        let mut inverted = plain.clone();
        inverted.invert_opacity_map_value = true;

        let left = [[0.25, 0.5], [0.0; 2]];
        let right = [[0.75, 0.5], [0.0; 2]];
        assert!(!plain.depth_covers(left, 1.0, 1.0));
        assert!(plain.depth_covers(right, 1.0, 1.0));
        assert!(inverted.depth_covers(left, 1.0, 1.0));
        assert!(!inverted.depth_covers(right, 1.0, 1.0));
        assert!(!plain.same_shadow_surface(&inverted));

        inverted.base_color[3] = 0.25;
        assert!(!inverted.covers(left, 1.0, 1.0));
        assert!(!inverted.depth_covers(left, 1.0, 1.0));
        plain.depth_draw_mode = MeshDepthDraw::OpaqueOnly;
        inverted.depth_draw_mode = MeshDepthDraw::OpaqueOnly;
        assert!(plain.same_shadow_surface(&inverted));
    }

    #[test]
    fn anisotropy_invalidates_alpha_sampled_shadow_surfaces() {
        let image = Arc::new(MeshImage::new([1, 1], vec![255; 4]).unwrap());
        let map = |max_anisotropy| {
            SceneTexture {
                max_anisotropy,
                ..SceneTexture::default()
            }
            .prepare(true, |_| Some(Arc::clone(&image)))
        };
        let mut base = MeshMaterial {
            point_size: 1.0,
            line_width: 1.0,
            opacity: 1.0,
            opacity_texture: None,
            invert_opacity_map_value: false,
            shader: None,
            channels: Default::default(),
            vertex_colors: false,
            vertex_masks: Default::default(),
            cull_mode: MeshCullMode::Back,
            depth_draw_mode: Default::default(),
            blend_mode: Default::default(),
            base_color: [1.0; 4],
            base_texture: map(1),
            base_item: None,
            alpha_mode: MeshAlphaMode::Opaque,
            pbr: None,
        };
        let mut filtered = base.clone();
        filtered.base_texture = map(16);
        assert!(base.same_shadow_surface(&filtered));

        base.alpha_mode = MeshAlphaMode::Mask(0.5);
        filtered.alpha_mode = base.alpha_mode;
        assert!(!base.same_shadow_surface(&filtered));
        assert!(filtered.same_shadow_surface(&filtered.clone()));
    }

    #[test]
    fn matching_slot_bindings_compose_factors_before_preparing_the_source() {
        let first = SceneMaterial {
            emissive_strength: Some(5.0),
            roughness: Some(0.25),
            ..SceneMaterial::default()
        };
        let last = SceneMaterial {
            roughness: Some(0.6),
            emissive_color: Some(Color(0xff0000ff)),
            ..SceneMaterial::default()
        };
        let base = MeshMaterial {
            point_size: 1.0,
            line_width: 1.0,
            opacity: 1.0,
            opacity_texture: None,
            invert_opacity_map_value: false,
            shader: None,
            channels: Default::default(),
            vertex_colors: true,
            vertex_masks: Default::default(),
            cull_mode: MeshCullMode::Back,
            depth_draw_mode: Default::default(),
            blend_mode: Default::default(),
            base_color: [1.0; 4],
            base_texture: None,
            base_item: None,
            alpha_mode: MeshAlphaMode::Opaque,
            pbr: Some(MeshPbr::principled()),
        };
        let bindings = vec![
            SceneMaterialBinding::for_field("body", Some("Body finish"), last.clone()).unwrap(),
            SceneMaterialBinding::for_field(
                "slot-0",
                None,
                SceneMaterial {
                    roughness: Some(0.97),
                    ..SceneMaterial::default()
                },
            )
            .unwrap(),
        ];
        let appearance = SceneAppearance::new(Some(first), bindings).unwrap();
        let slot = MeshMaterialSlot {
            source_index: Some(2),
            name: Some("Body finish".into()),
        };
        let combined = appearance
            .styles(Some(&slot), 0)
            .cloned()
            .reduce(|mut before, later| {
                before.overlay(&later);
                before
            })
            .unwrap();
        let pbr = combined.prepare(&base).pbr.unwrap();
        assert_eq!(pbr.roughness, 0.6);
        assert_eq!(pbr.emissive, [5.0, 0.0, 0.0]);
    }
}
