//! Decode KTX cubemaps and retain authored KTX2 material texture blocks.

use std::{borrow::Cow, io::Read, ops::Range, sync::OnceLock};

use flate2::read::ZlibDecoder;
use xrack_runtime::scene3d::{MeshBlockFormat, MeshImage};

const KTX1: &[u8; 12] = b"\xABKTX 11\xBB\r\n\x1A\n";
const KTX2: &[u8; 12] = b"\xABKTX 20\xBB\r\n\x1A\n";

#[derive(Clone, Copy)]
enum Endian {
    Little,
    Big,
}

impl Endian {
    fn word(self, bytes: &[u8], start: usize) -> Option<u32> {
        let value = bytes.get(start..start.checked_add(4)?)?.try_into().ok()?;
        Some(match self {
            Self::Little => u32::from_le_bytes(value),
            Self::Big => u32::from_be_bytes(value),
        })
    }

    fn half(self, bytes: [u8; 2]) -> u16 {
        match self {
            Self::Little => u16::from_le_bytes(bytes),
            Self::Big => u16::from_be_bytes(bytes),
        }
    }
}

#[derive(Clone, Copy)]
enum Encoding {
    Byte,
    Half,
    Float,
    Rgb9e5,
}

#[derive(Clone, Copy)]
struct ColorFormat {
    encoding: Encoding,
    channels: usize,
    bgr: bool,
    srgb: bool,
}

impl ColorFormat {
    fn scalar_size(self) -> usize {
        match self.encoding {
            Encoding::Byte => 1,
            Encoding::Half => 2,
            Encoding::Float | Encoding::Rgb9e5 => 4,
        }
    }

    fn pixel_size(self) -> usize {
        if matches!(self.encoding, Encoding::Rgb9e5) {
            4
        } else {
            self.scalar_size() * self.channels
        }
    }

    fn decode(self, texel: &[u8], endian: Endian) -> Option<[f32; 4]> {
        let mut rgba = [0.0, 0.0, 0.0, 1.0];
        match self.encoding {
            Encoding::Rgb9e5 => {
                let word = endian.word(texel, 0)?;
                let scale = f32::from_bits(((word >> 27) + 103) << 23);
                for (channel, color) in rgba[..3].iter_mut().enumerate() {
                    *color = ((word >> (channel * 9)) & 511) as f32 * scale;
                }
            }
            encoding => {
                let size = self.scalar_size();
                for (channel, (value, component)) in
                    rgba.iter_mut().zip(texel.chunks_exact(size)).enumerate()
                {
                    *value = match encoding {
                        Encoding::Byte if self.srgb && channel < 3 => {
                            srgb_lut()[usize::from(component[0])]
                        }
                        Encoding::Byte => f32::from(component[0]) / 255.0,
                        Encoding::Half => decode_half(endian.half(component.try_into().ok()?)),
                        Encoding::Float => f32::from_bits(endian.word(component, 0)?),
                        Encoding::Rgb9e5 => return None,
                    };
                }
            }
        }
        if self.bgr {
            rgba.swap(0, 2);
        }
        if !rgba.iter().all(|value| value.is_finite()) {
            return None;
        }
        Some(rgba)
    }
}

fn srgb_lut() -> &'static [f32; 256] {
    static LINEAR: OnceLock<[f32; 256]> = OnceLock::new();
    LINEAR.get_or_init(|| {
        std::array::from_fn(|channel| {
            let value = channel as f32 / 255.0;
            if value <= 0.04045 {
                value / 12.92
            } else {
                ((value + 0.055) / 1.055).powf(2.4)
            }
        })
    })
}

fn decode_half(bits: u16) -> f32 {
    let sign = u32::from(bits & 0x8000) << 16;
    let exponent = u32::from(bits >> 10) & 31;
    let fraction = u32::from(bits & 1023);
    match exponent {
        0 => (fraction as f32 * 2.0_f32.powi(-24)).copysign(f32::from_bits(sign)),
        31 => f32::from_bits(sign | 0x7f80_0000 | fraction << 13),
        _ => f32::from_bits(sign | (exponent + 112) << 23 | fraction << 13),
    }
}

#[derive(Clone, Copy, Default)]
struct Metadata {
    orientation: Option<[u8; 2]>,
    swizzle: Option<[u8; 4]>,
    premultiplied: bool,
}

fn metadata(mut bytes: &[u8], endian: Endian, version2: bool) -> Result<Metadata, String> {
    let mut result = Metadata::default();
    while !bytes.is_empty() {
        let length = endian.word(bytes, 0).ok_or("truncated KTX metadata size")? as usize;
        let entry = range(bytes, 4, length).ok_or("truncated KTX metadata value")?;
        let separator = entry
            .iter()
            .position(|&byte| byte == 0)
            .ok_or("missing KTX metadata key terminator")?;
        let (key, value) = (&entry[..separator], &entry[separator + 1..]);
        let value = value.strip_suffix(&[0]).unwrap_or(value);
        match key {
            b"KTXorientation" => {
                let orientation = if version2 {
                    if value != b"rd" {
                        return Err("KTX2 textures require rd orientation".into());
                    }
                    [b'r', b'd']
                } else {
                    match value {
                        [
                            b'S',
                            b'=',
                            x @ (b'r' | b'l'),
                            b',',
                            b'T',
                            b'=',
                            y @ (b'u' | b'd'),
                        ] => [*x, *y],
                        _ => return Err("unsupported KTX cubemap orientation".into()),
                    }
                };
                if result.orientation.replace(orientation).is_some() {
                    return Err("duplicate KTX orientation".into());
                }
            }
            b"KTXswizzle" => {
                let channels: [u8; 4] = value.try_into().map_err(|_| "invalid KTX swizzle")?;
                if !channels.iter().all(|channel| b"rgba01".contains(channel))
                    || result.swizzle.replace(channels).is_some()
                {
                    return Err("invalid or duplicate KTX swizzle".into());
                }
            }
            _ => {}
        }
        let consumed = align(length, 4)
            .and_then(|length| length.checked_add(4))
            .ok_or("invalid KTX metadata size")?;
        bytes = bytes
            .get(consumed..)
            .ok_or("truncated KTX metadata padding")?;
    }
    Ok(result)
}

fn align(size: usize, alignment: usize) -> Option<usize> {
    size.checked_add((alignment - size % alignment) % alignment)
}

fn range(bytes: &[u8], start: usize, len: usize) -> Option<&[u8]> {
    bytes.get(start..start.checked_add(len)?)
}

fn dimensions(
    width: u32,
    height: u32,
    depth: u32,
    layers: u32,
    faces: u32,
    levels: u32,
) -> Result<(usize, usize), String> {
    let size = width as usize;
    if width == 0 || height != width || depth != 0 || layers != 0 || faces != 6 {
        return Err("expected a square six-face cubemap without array layers".into());
    }
    if size
        .checked_mul(size)
        .and_then(|pixels| pixels.checked_mul(6))
        .is_none_or(|pixels| pixels > super::MAX_PIXELS)
    {
        return Err("cubemap exceeds 8 Mi pixels".into());
    }
    if levels > width.ilog2() + 1 {
        return Err("too many KTX mip levels".into());
    }
    Ok((size, levels.max(1) as usize))
}

fn decode_faces(
    pixels: &[u8],
    side: usize,
    stride: usize,
    format: ColorFormat,
    endian: Endian,
    metadata: Metadata,
    output: &mut impl FnMut([f32; 4]),
) -> Result<(), String> {
    let [s, t] = metadata.orientation.unwrap_or(*b"rd");
    let swizzle = metadata.swizzle.unwrap_or(*b"rgba");
    for face in 0..6 {
        for y in 0..side {
            let row = if t == b'u' { side - y - 1 } else { y };
            for x in 0..side {
                let column = if s == b'l' { side - x - 1 } else { x };
                let offset = (face * side + row) * stride + column * format.pixel_size();
                let mut rgba = range(pixels, offset, format.pixel_size())
                    .and_then(|texel| format.decode(texel, endian))
                    .ok_or("invalid cubemap pixel data")?;
                if metadata.premultiplied {
                    let alpha = rgba[3].clamp(0.0, 1.0);
                    for value in &mut rgba[..3] {
                        *value = if alpha > 0.0 { *value / alpha } else { 0.0 };
                    }
                }
                output(swizzle.map(|channel| match channel {
                    b'r' => rgba[0],
                    b'g' => rgba[1],
                    b'b' => rgba[2],
                    b'a' => rgba[3],
                    b'1' => 1.0,
                    _ => 0.0,
                }));
            }
        }
    }
    Ok(())
}

fn version1(
    bytes: &[u8],
    all_levels: bool,
    output: &mut impl FnMut([f32; 4]),
) -> Result<(usize, usize), String> {
    let marker = bytes.get(12..16).ok_or("truncated KTX endianness marker")?;
    let endian = match marker {
        [1, 2, 3, 4] => Endian::Little,
        [4, 3, 2, 1] => Endian::Big,
        _ => return Err("invalid KTX endianness marker".into()),
    };
    let field = |offset| endian.word(bytes, offset).ok_or("truncated KTX header");
    let (gl_type, size, gl_format, internal) = (field(16)?, field(20)?, field(24)?, field(28)?);
    let (channels, bgr) = match gl_format {
        0x1907 => (3, false),
        0x1908 => (4, false),
        0x80e0 => (3, true),
        0x80e1 => (4, true),
        _ => return Err("unsupported KTX cubemap channel format".into()),
    };
    let base = field(32)?;
    if base != if channels == 3 { 0x1907 } else { 0x1908 } && base != gl_format {
        return Err("invalid KTX base channel format".into());
    }
    let (encoding, srgb) = match (gl_type, internal, channels) {
        (0x1401, 0x8051 | 0x1907, 3) | (0x1401, 0x8058 | 0x1908, 4) => (Encoding::Byte, false),
        (0x1401, 0x8c41, 3) | (0x1401, 0x8c43, 4) => (Encoding::Byte, true),
        (0x140b | 0x8d61, 0x881b, 3) | (0x140b | 0x8d61, 0x881a, 4) => (Encoding::Half, false),
        (0x1406, 0x8815 | 0x881b, 3) | (0x1406, 0x8814 | 0x881a, 4) => (Encoding::Float, false),
        (0x8c3e, 0x8c3d, 3) if !bgr => (Encoding::Rgb9e5, false),
        _ => return Err("unsupported KTX cubemap pixel format".into()),
    };
    let format = ColorFormat {
        encoding,
        channels,
        bgr,
        srgb,
    };
    if size as usize != format.scalar_size() {
        return Err("invalid KTX component size".into());
    }
    let (side, levels) = dimensions(
        field(36)?,
        field(40)?,
        field(44)?,
        field(48)?,
        field(52)?,
        field(56)?,
    )?;
    let metadata_size = field(60)? as usize;
    let metadata = metadata(
        range(bytes, 64, metadata_size).ok_or("truncated KTX metadata")?,
        endian,
        false,
    )?;
    let mut cursor = 64 + metadata_size;
    for level in 0..levels {
        let width = (side >> level).max(1);
        let stride = align(width * format.pixel_size(), 4).ok_or("invalid KTX face stride")?;
        let face_size = endian
            .word(bytes, cursor)
            .ok_or("truncated KTX level size")? as usize;
        if face_size != stride * width {
            return Err("KTX cube face does not match its dimensions".into());
        }
        cursor += 4;
        let contents = range(bytes, cursor, face_size * 6).ok_or("truncated KTX cubemap level")?;
        if level == 0 || all_levels {
            decode_faces(contents, width, stride, format, endian, metadata, output)?;
        }
        cursor += contents.len();
    }
    Ok((side, levels))
}

fn vulkan_format(format: u32) -> Option<ColorFormat> {
    let (encoding, channels, bgr, srgb) = match format {
        23 | 29 => (Encoding::Byte, 3, false, format == 29),
        30 | 36 => (Encoding::Byte, 3, true, format == 36),
        37 | 43 => (Encoding::Byte, 4, false, format == 43),
        44 | 50 => (Encoding::Byte, 4, true, format == 50),
        90 | 97 => (
            Encoding::Half,
            if format == 90 { 3 } else { 4 },
            false,
            false,
        ),
        106 | 109 => (
            Encoding::Float,
            if format == 106 { 3 } else { 4 },
            false,
            false,
        ),
        123 => (Encoding::Rgb9e5, 3, false, false),
        _ => return None,
    };
    Some(ColorFormat {
        encoding,
        channels,
        bgr,
        srgb,
    })
}

fn validate_dfd(bytes: &[u8]) -> Result<(), String> {
    let size = Endian::Little
        .word(bytes, 0)
        .ok_or("missing KTX2 data format descriptor")? as usize;
    if size != bytes.len() || size < 28 || &bytes[4..8] != [0; 4] {
        return Err("invalid KTX2 data format descriptor".into());
    }
    let block = u16::from_le_bytes([bytes[10], bytes[11]]) as usize;
    if !(24..=size - 4).contains(&block) {
        return Err("invalid KTX2 descriptor block length".into());
    }
    if !matches!(bytes[13], 0 | 1) {
        return Err("KTX2 texture color primaries must be sRGB/BT.709".into());
    }
    Ok(())
}

fn dfd_premultiplied(bytes: &[u8], format: ColorFormat) -> Result<bool, String> {
    if bytes[12] != 1 || bytes[16..20] != [0; 4] || bytes[20] != format.pixel_size() as u8 {
        return Err("KTX2 descriptor does not match its uncompressed pixel format".into());
    }
    if format.srgb && bytes[14] != 2 || !format.srgb && !matches!(bytes[14], 0 | 1) {
        return Err("KTX2 transfer function does not match its pixel format".into());
    }
    Ok(bytes[15] & 1 != 0)
}

struct Ktx2<'a> {
    bytes: &'a [u8],
    descriptor: &'a [u8],
    meta: Metadata,
    data_start: usize,
    scheme: u32,
}

impl<'a> Ktx2<'a> {
    fn field(bytes: &[u8], offset: usize) -> Result<u32, String> {
        Endian::Little
            .word(bytes, offset)
            .ok_or_else(|| "truncated KTX2 header".into())
    }

    fn quad(bytes: &[u8], offset: usize) -> Result<usize, String> {
        range(bytes, offset, 8)
            .and_then(|v| v.try_into().ok())
            .map(u64::from_le_bytes)
            .and_then(|value| usize::try_from(value).ok())
            .ok_or_else(|| "invalid KTX2 byte range".into())
    }

    fn open(bytes: &'a [u8], levels: usize) -> Result<Self, String> {
        let field = |offset| Self::field(bytes, offset);
        let scheme = field(44)?;
        if !matches!(scheme, 0 | 3) {
            return Err("KTX2 textures require uncompressed or ZLIB level data".into());
        }
        if Self::quad(bytes, 64)? != 0 || Self::quad(bytes, 72)? != 0 {
            return Err("unexpected KTX2 supercompression global data".into());
        }
        let index_end = 80 + levels * 24;
        let (dfd_offset, dfd_size) = (field(48)? as usize, field(52)? as usize);
        if dfd_offset < index_end {
            return Err("KTX2 descriptor overlaps the level index".into());
        }
        let descriptor =
            range(bytes, dfd_offset, dfd_size).ok_or("truncated KTX2 data format descriptor")?;
        validate_dfd(descriptor)?;
        let (kv_offset, kv_size) = (field(56)? as usize, field(60)? as usize);
        let mut data_start = dfd_offset + dfd_size;
        let kv = if kv_size == 0 {
            &[][..]
        } else {
            if kv_offset < data_start {
                return Err("KTX2 key/value data overlaps the descriptor".into());
            }
            let value = range(bytes, kv_offset, kv_size).ok_or("truncated KTX2 key/value data")?;
            data_start = kv_offset + kv_size;
            value
        };
        let meta = metadata(kv, Endian::Little, true)?;
        Ok(Self {
            bytes,
            descriptor,
            meta,
            data_start,
            scheme,
        })
    }

    fn levels(
        &self,
        count: usize,
        alignment: usize,
        length: impl Fn(usize) -> usize,
    ) -> Result<Vec<(Range<usize>, usize)>, String> {
        let mut levels: Vec<(Range<usize>, usize)> = Vec::with_capacity(count);
        for level in 0..count {
            let index = 80 + level * 24;
            let offset = Self::quad(self.bytes, index)?;
            let stored = Self::quad(self.bytes, index + 8)?;
            let decoded = Self::quad(self.bytes, index + 16)?;
            let expected = length(level);
            if offset < self.data_start
                || stored == 0
                || decoded != expected
                || self.scheme == 0 && (stored != expected || !offset.is_multiple_of(alignment))
            {
                return Err("invalid KTX2 level layout".into());
            }
            let end = offset
                .checked_add(stored)
                .filter(|end| *end <= self.bytes.len())
                .ok_or("truncated KTX2 level")?;
            if levels
                .iter()
                .any(|(range, _)| offset < range.end && range.start < end)
            {
                return Err("overlapping KTX2 levels".into());
            }
            levels.push((offset..end, expected));
        }
        Ok(levels)
    }

    fn level(
        &self,
        index: usize,
        (range, expected): (Range<usize>, usize),
    ) -> Result<Cow<'a, [u8]>, String> {
        let bytes = &self.bytes[range];
        if self.scheme == 0 {
            return Ok(Cow::Borrowed(bytes));
        }
        let mut expanded = Vec::with_capacity(expected);
        ZlibDecoder::new(bytes)
            .take(expected as u64 + 1)
            .read_to_end(&mut expanded)
            .map_err(|error| format!("cannot inflate KTX2 level {index}: {error}"))?;
        if expanded.len() != expected {
            return Err("inflated KTX2 level has the wrong size".into());
        }
        Ok(Cow::Owned(expanded))
    }
}

fn version2(
    bytes: &[u8],
    all_levels: bool,
    output: &mut impl FnMut([f32; 4]),
) -> Result<(usize, usize), String> {
    let field = |offset| Ktx2::field(bytes, offset);
    let format = vulkan_format(field(12)?).ok_or("unsupported KTX2 cubemap pixel format")?;
    if field(16)? as usize != format.scalar_size() {
        return Err("invalid KTX2 component size".into());
    }
    let (side, levels) = dimensions(
        field(20)?,
        field(24)?,
        field(28)?,
        field(32)?,
        field(36)?,
        field(40)?,
    )?;
    let data = Ktx2::open(bytes, levels)?;
    let mut meta = data.meta;
    meta.premultiplied = dfd_premultiplied(data.descriptor, format)?;
    let pixel_size = format.pixel_size();
    // LCM(pixel size, 4) for the supported tightly packed color formats.
    let alignment = if pixel_size.is_multiple_of(4) {
        pixel_size
    } else if pixel_size.is_multiple_of(2) {
        pixel_size * 2
    } else {
        pixel_size * 4
    };
    let ranges = data.levels(levels, alignment, |level| {
        let width = (side >> level).max(1);
        width * width * 6 * pixel_size
    })?;
    for (index, level) in ranges
        .into_iter()
        .take(if all_levels { levels } else { 1 })
        .enumerate()
    {
        let side = side >> index;
        let bytes = data.level(index, level)?;
        decode_faces(
            &bytes,
            side,
            side * pixel_size,
            format,
            Endian::Little,
            meta,
            output,
        )?;
    }
    Ok((side, levels))
}

pub(crate) fn decode_ktx2_image(bytes: &[u8]) -> Result<Option<MeshImage>, String> {
    if !bytes.starts_with(KTX2) {
        return Ok(None);
    }
    let field = |offset| Ktx2::field(bytes, offset);
    let faces = field(36)?;
    if faces == 6 {
        return Ok(None);
    }
    let vk_format = field(12)?;
    if vk_format == 0 {
        return MeshImage::from_basis_ktx2(bytes).map(Some);
    }
    let format =
        MeshBlockFormat::from_vulkan(vk_format).ok_or("unsupported KTX2 2D block format")?;
    if field(16)? != 1 || field(28)? != 0 || field(32)? != 0 || faces != 1 {
        return Err("expected a non-array KTX2 2D block texture".into());
    }
    let size = xrack_assets::validate_image_size([field(20)?, field(24)?])?;
    let count = field(40)?;
    if !(1..=size[0].max(size[1]).ilog2() + 1).contains(&count) {
        return Err("invalid number of KTX2 block texture mip levels".into());
    }
    let data = Ktx2::open(bytes, count as usize)?;
    let descriptor = data.descriptor;
    let [width, height] = format.block_size();
    if descriptor[16..20] != [width as u8 - 1, height as u8 - 1, 0, 0]
        || descriptor[20] != format.block_bytes() as u8
        || (vk_format.is_multiple_of(2) && descriptor[14] != 2)
        || (!vk_format.is_multiple_of(2) && !matches!(descriptor[14], 0 | 1))
    {
        return Err("KTX2 descriptor does not match its compressed pixel format".into());
    }
    if descriptor[15] & 1 != 0 || data.meta.swizzle.is_some_and(|swizzle| swizzle != *b"rgba") {
        return Err("KTX2 2D block textures require straight alpha and RGBA channel order".into());
    }
    let ranges = data.levels(count as usize, format.block_bytes(), |level| {
        format.level_bytes(size.map(|side| (side >> level).max(1)))
    })?;
    let mut bytes = Vec::with_capacity(ranges.iter().map(|(_, size)| size).sum());
    for (index, level) in ranges.into_iter().enumerate() {
        bytes.extend_from_slice(&data.level(index, level)?);
    }
    MeshImage::from_blocks(size, vk_format, count, bytes)
        .map(Some)
        .ok_or_else(|| "invalid KTX2 2D block texture".into())
}

pub(super) fn decode(bytes: &[u8]) -> Result<(usize, Vec<[f32; 3]>), String> {
    let mut rgb = Vec::new();
    let (side, _) = decode_levels(bytes, false, &mut |[r, g, b, _]| {
        rgb.push([r, g, b].map(|value| value.clamp(0.0, 65_408.0)));
    })?;
    Ok((side, rgb))
}

pub(super) fn decode_texture(bytes: &[u8]) -> Result<(usize, usize, Vec<u8>), String> {
    let mut texels = Vec::new();
    let (side, levels) = decode_levels(bytes, true, &mut |[r, g, b, a]| {
        for value in [r, g, b, a.clamp(0.0, 1.0)] {
            texels.extend_from_slice(&encode_half(value).to_le_bytes());
        }
    })?;
    Ok((side, levels, texels))
}

fn encode_half(value: f32) -> u16 {
    let magnitude = value.abs().min(65_504.0);
    let sign = ((value.to_bits() >> 16) & 0x8000) as u16;
    let bits = magnitude.to_bits();
    if bits < 113 << 23 {
        // Subnormal binary16 values are integer multiples of 2^-24.
        return sign | (magnitude * 16_777_216.0).round_ties_even() as u16;
    }
    let significand = bits - (112 << 23);
    let round = 0xfff + ((significand >> 13) & 1);
    sign | ((significand + round) >> 13) as u16
}

fn decode_levels(
    bytes: &[u8],
    all_levels: bool,
    output: &mut impl FnMut([f32; 4]),
) -> Result<(usize, usize), String> {
    if bytes.starts_with(KTX1) {
        version1(bytes, all_levels, output)
    } else if bytes.starts_with(KTX2) {
        version2(bytes, all_levels, output)
    } else {
        Err("missing KTX or KTX2 signature".into())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ktx_reads_big_endian_half_float_faces_and_padded_rows() {
        let mut file = KTX1.to_vec();
        let header: [u32; 13] = [
            0x04030201, 0x140b, 2, 0x1907, 0x881b, 0x1907, 1, 1, 0, 0, 6, 1, 0,
        ];
        file.extend(header.into_iter().flat_map(u32::to_be_bytes));
        file.extend(8u32.to_be_bytes());
        for face in 0..6u16 {
            let channels = [0x3c00 + (face << 10), 0x3800, 0x3400, 0];
            file.extend(channels.into_iter().flat_map(u16::to_be_bytes));
        }
        let (side, pixels) = decode(&file).unwrap();
        assert_eq!(side, 1);
        assert_eq!(
            pixels,
            [1.0, 2.0, 4.0, 8.0, 16.0, 32.0].map(|red| [red, 0.5, 0.25])
        );
        assert!(decode(&file[..file.len() - 1]).is_err());
    }

    fn two_level_ktx2() -> Vec<u8> {
        let descriptor_offset = 128;
        let descriptor_size = 92;
        let small_offset = descriptor_offset + descriptor_size;
        let full_offset = small_offset + 24;
        let mut file = KTX2.to_vec();
        let fields: [u32; 13] = [
            43,
            1,
            2,
            2,
            0,
            0,
            6,
            2,
            0,
            descriptor_offset,
            descriptor_size,
            0,
            0,
        ];
        file.extend(fields.into_iter().flat_map(u32::to_le_bytes));
        file.extend([0; 16]); // No supercompression global data.
        file.extend(
            [
                u64::from(full_offset),
                96,
                96,
                u64::from(small_offset),
                24,
                24,
            ]
            .into_iter()
            .flat_map(u64::to_le_bytes),
        );
        file.extend(descriptor_size.to_le_bytes());
        file.extend([0; 4]); // Khronos basic descriptor.
        file.extend(2u16.to_le_bytes());
        file.extend(88u16.to_le_bytes());
        file.extend([1, 1, 2, 0, 0, 0, 0, 0, 4, 0, 0, 0, 0, 0, 0, 0]);
        for (index, channel) in [0, 1, 2, 31].into_iter().enumerate() {
            file.extend(((index * 8) as u16).to_le_bytes());
            file.extend([7, channel, 0, 0, 0, 0]);
            file.extend(0u32.to_le_bytes());
            file.extend(255u32.to_le_bytes());
        }
        file.extend([255; 24]);
        for face in 0..6 {
            for pixel in 0..4 {
                file.extend([32 * face, 32 * pixel, 128, 255]);
            }
        }
        file
    }

    #[test]
    fn ktx2_uses_the_indexed_base_level_and_linearizes_srgb() {
        let file = two_level_ktx2();
        let (side, pixels) = decode(&file).unwrap();
        assert_eq!(side, 2);
        assert_eq!(pixels.len(), 24);
        for face in 0..6 {
            for pixel in 0..4 {
                assert_eq!(
                    pixels[face * 4 + pixel],
                    [
                        srgb_lut()[32 * face],
                        srgb_lut()[32 * pixel],
                        srgb_lut()[128]
                    ]
                );
            }
        }
        assert!((pixels[0][2] - 0.21586).abs() < 0.0001);
    }

    #[test]
    fn shader_cube_decodes_authored_mips_and_straight_alpha() {
        let mut file = two_level_ktx2();
        let first_base = file.len() - 96;
        file[first_base + 3] = 128;
        let (side, levels, texels) = decode_texture(&file).unwrap();
        assert_eq!((side, levels, texels.len()), (2, 2, (24 + 6) * 8));
        let half_at = |offset| {
            decode_half(u16::from_le_bytes(
                texels[offset..offset + 2].try_into().unwrap(),
            ))
        };
        assert!((half_at(4) - srgb_lut()[128]).abs() < 0.001);
        assert!((half_at(6) - 128.0 / 255.0).abs() < 0.001);
        assert_eq!(half_at(24 * 8), 1.0);
        assert_eq!(half_at(24 * 8 + 6), 1.0);
        assert_eq!(encode_half(-1.0), 0xbc00);
        assert_eq!(encode_half(2.0_f32.powi(-24)), 1);
        assert_eq!(encode_half(f32::MAX), 0x7bff);

        let path = std::path::Path::new("authored-cube.ktx2");
        let original = super::super::decode_cubemap(path, &file).unwrap();
        let reloaded = super::super::decode_cubemap(path, &file).unwrap();
        assert!(std::sync::Arc::ptr_eq(&original, &reloaded));
        file[first_base + 3] = 64;
        let changed = super::super::decode_cubemap(path, &file).unwrap();
        assert!(!std::sync::Arc::ptr_eq(&original, &changed));
        assert_ne!(original.texels(), changed.texels());
    }

    #[test]
    fn ktx2_reports_invalid_layout_and_unsupported_encoding() {
        let mut file = two_level_ktx2();
        file[44..48].copy_from_slice(&1u32.to_le_bytes());
        assert!(decode(&file).unwrap_err().contains("uncompressed or ZLIB"));
        file[44..48].copy_from_slice(&0u32.to_le_bytes());
        file[80..88].copy_from_slice(&220u64.to_le_bytes());
        assert!(decode(&file).unwrap_err().contains("overlapping"));
        assert!(decode(&file[..70]).is_err());
    }
}
