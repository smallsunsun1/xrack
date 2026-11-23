//! Material data shared by 3D resources, rendering, and picking.
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
    rgba: Box<[u8]>,
    mips: [OnceLock<Box<[u8]>>; 2],
}

impl MeshImage {
    pub fn new(size: [u32; 2], rgba: Vec<u8>) -> Option<Self> {
        (Self::byte_len(size)? == rgba.len()).then(|| Self {
            size,
            rgba: rgba.into_boxed_slice(),
            mips: Default::default(),
        })
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
        &self.rgba
    }

    pub fn mip_extents(&self) -> impl Iterator<Item = [u32; 2]> + use<> {
        mip_extents(self.size)
    }

    pub fn mip_byte_len(&self) -> usize {
        self.mip_extents().map(rgba_len).sum()
    }

    pub fn prepared_mips(&self, color: TextureColorSpace) -> Option<&[u8]> {
        self.mips[color.slot()].get().map(Box::as_ref)
    }

    /// Build each requested interpretation once. Color channels of sRGB maps are
    /// filtered in linear light. Data maps and alpha are averaged as linear values.
    pub fn prepare_mips(&self, color: TextureColorSpace) -> &[u8] {
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
                    &self.rgba
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

#[derive(Debug)]
pub struct MeshTexture {
    pub image: Arc<MeshImage>,
    pub wrap: [TextureWrap; 2],
    pub min_filter: TextureFilter,
    pub mag_filter: TextureFilter,
    /// None restricts sampling to the source image.
    pub mip_filter: Option<TextureFilter>,
    /// Slot in the prepared vertex buffer. The base color map always uses slot zero.
    pub uv_slot: u8,
    pub transform: TextureTransform,
}

impl MeshTexture {
    fn is_valid(&self) -> bool {
        self.uv_slot <= 1 && self.transform.is_valid()
    }

    fn alpha_at(&self, uv: [f32; 2]) -> f32 {
        self.sample(uv, |pixel| [f32::from(pixel[3]) / 255.0])[0]
    }

    pub(crate) fn rgb_at(&self, uv: [f32; 2], srgb: bool) -> [f32; 3] {
        self.sample(uv, |pixel| {
            if srgb {
                crate::scene_lighting::linear_color(crate::Color(u32::from_be_bytes(pixel)))
            } else {
                [pixel[0], pixel[1], pixel[2]].map(|value| f32::from(value) / 255.0)
            }
        })
    }

    fn sample<const N: usize>(
        &self,
        uv: [f32; 2],
        convert: impl Fn([u8; 4]) -> [f32; N],
    ) -> [f32; N] {
        let uv = self.transform.map(uv);
        let [width, height] = self.image.size.map(i64::from);
        let x = self.wrap[0].coordinate(f64::from(uv[0])) * width as f64;
        let y = self.wrap[1].coordinate(f64::from(uv[1])) * height as f64;
        let texel = |x, y| {
            let column = self.wrap[0].offset(x, width);
            let row = self.wrap[1].offset(y, height);
            let start = (row * width as usize + column) * 4;
            convert(std::array::from_fn(|channel| {
                self.image.rgba[start + channel]
            }))
        };
        if self.mag_filter == TextureFilter::Nearest {
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

#[derive(Debug)]
pub struct MeshMaterial {
    pub base_color: [f32; 4],
    pub base_texture: Option<MeshTexture>,
    pub alpha_mode: MeshAlphaMode,
    pub double_sided: bool,
    /// Absent for glTF KHR_materials_unlit.
    pub pbr: Option<MeshPbr>,
}

#[derive(Debug)]
pub struct MeshPbr {
    pub metallic: f32,
    pub roughness: f32,
    /// Linear emission with the glTF emissive strength already applied.
    pub emissive: [f32; 3],
    pub occlusion_strength: f32,
    pub metallic_roughness_texture: Option<MeshTexture>,
    pub emissive_texture: Option<MeshTexture>,
    pub occlusion_texture: Option<MeshTexture>,
    pub normal_map: Option<MeshNormalMap>,
    pub clearcoat: Option<MeshClearcoat>,
    pub dielectric: Option<Box<MeshDielectric>>,
    pub transmission: Option<Box<MeshTransmission>>,
}

#[derive(Debug)]
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

#[derive(Debug)]
pub struct MeshVolume {
    pub thickness: f32,
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

#[derive(Debug)]
pub struct MeshDielectric {
    /// 0 is the glTF specular-glossiness conversion mode (infinite IOR).
    pub ior: f32,
    pub specular: f32,
    pub color: [f32; 3],
    pub specular_texture: Option<MeshTexture>,
    pub color_texture: Option<MeshTexture>,
}

impl MeshDielectric {
    pub const DEFAULT_IOR: f32 = 1.5;
    pub const DEFAULT_F0: f32 = 0.04;

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

#[derive(Debug)]
pub struct MeshClearcoat {
    pub amount: f32,
    pub roughness: f32,
    pub amount_texture: Option<MeshTexture>,
    pub roughness_texture: Option<MeshTexture>,
    pub normal_map: Option<MeshNormalMap>,
}

#[derive(Debug)]
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
            // The specular-glossiness conversion mode uses an infinite IOR.
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

    pub fn transmission_blur(&self) -> Option<f32> {
        self.transmission
            .as_ref()
            .filter(|term| term.factor > 0.0)?;
        if self.metallic >= 1.0 && self.metallic_roughness_texture.is_none() {
            return None;
        }
        Some(self.roughness * self.refraction_channels().blur_scale[2])
    }

    /// Select the UV set of the mesh's authored or generated tangent frame.
    pub fn tangent_normal_map(&self) -> Option<&MeshNormalMap> {
        self.normal_map
            .as_ref()
            .or_else(|| self.clearcoat.as_ref()?.normal_map.as_ref())
    }

    pub(super) fn is_valid(&self) -> bool {
        [self.metallic, self.roughness, self.occlusion_strength]
            .into_iter()
            .all(|value| (0.0..=1.0).contains(&value))
            && self.emissive.into_iter().all(|v| v.is_finite() && v >= 0.0)
            && [
                &self.metallic_roughness_texture,
                &self.emissive_texture,
                &self.occlusion_texture,
            ]
            .into_iter()
            .flatten()
            .all(MeshTexture::is_valid)
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
        [self.amount, self.roughness]
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
    Opaque,
    Mask(f32),
    Blend,
}

impl MeshMaterial {
    pub fn is_double_sided(&self) -> bool {
        self.double_sided && self.pbr.as_ref().and_then(MeshPbr::volume).is_none()
    }

    pub(super) fn covers(&self, uv: [f32; 2], vertex_alpha: f32, tint_alpha: f32) -> bool {
        if self.alpha_mode == MeshAlphaMode::Opaque {
            return true;
        }
        let texel_alpha = self
            .base_texture
            .as_ref()
            .map_or(1.0, |tex| tex.alpha_at(uv));
        let alpha = tint_alpha * (self.base_color[3] * vertex_alpha * texel_alpha);
        match self.alpha_mode {
            MeshAlphaMode::Opaque => true,
            MeshAlphaMode::Mask(cutoff) => alpha >= cutoff,
            MeshAlphaMode::Blend => alpha > 0.0,
        }
    }
}

#[derive(Clone, Debug)]
pub struct MeshPart {
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
