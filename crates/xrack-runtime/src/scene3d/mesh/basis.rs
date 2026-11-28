//! Validated Basis Universal sources. A device can transcode directly to its
//! native blocks, and CPU consumers can request RGBA without a GPU dependency.

use super::{
    blocks::{MeshBlockFormat, MeshTextureBlocks},
    material::{MAX_MESH_TEXTURE_DIMENSION, TextureColorSpace},
};
use basisu::{
    DecodeFlags, TargetFormat, Transcoder,
    ktx2::{BasisFormat, Ktx2Header},
};
use std::{ops::Range, sync::OnceLock};

const IDENTIFIER: &[u8; 12] = b"\xABKTX 20\xBB\r\n\x1A\n";
const MAX_SOURCE_BYTES: usize = 16 * 1024 * 1024;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Channels {
    Rgb,
    Rgba,
    Red,
    RedGreen,
}

#[derive(Debug)]
pub struct MeshBasisTexture {
    bytes: Box<[u8]>,
    size: [u32; 2],
    levels: u32,
    uastc: bool,
    channels: Channels,
    color: TextureColorSpace,
    bt709: bool,
    base_rgba: OnceLock<Box<[u8]>>,
}

impl MeshBasisTexture {
    pub(super) fn is_container(bytes: &[u8]) -> bool {
        bytes.starts_with(IDENTIFIER)
    }

    pub(super) fn parse(bytes: &[u8]) -> Result<Self, String> {
        if !Self::is_container(bytes) || !(80..=MAX_SOURCE_BYTES).contains(&bytes.len()) {
            return Err("expected a bounded Basis Universal KTX2 texture".into());
        }
        let fields = bytes[12..64].as_chunks::<4>().0;
        let [
            vk,
            type_size,
            width,
            height,
            depth,
            layers,
            faces,
            levels,
            scheme,
            dfd_off,
            dfd_len,
            kv_off,
            kv_len,
        ] = <[[u8; 4]; 13]>::try_from(fields)
            .map_err(|_| "invalid Basis KTX2 header")?
            .map(u32::from_le_bytes);
        if (vk, type_size, depth, layers, faces) != (0, 1, 0, 0, 1)
            || [width, height].into_iter().any(|side| {
                !(4..=MAX_MESH_TEXTURE_DIMENSION).contains(&side) || !side.is_multiple_of(4)
            })
            || !(1..=width.max(height).ilog2() + 1).contains(&levels)
        {
            return Err(
                "Basis KTX2 requires a 2D texture with 4-pixel alignment and valid mip levels"
                    .into(),
            );
        }
        let index_end = 80 + levels as usize * 24;
        let mut ranges = vec![0..index_end];
        let mut section = |offset: u64, len: u64| -> Result<&[u8], String> {
            if len == 0 && offset == 0 {
                return Ok(&[]);
            }
            let start = usize::try_from(offset).map_err(|_| "invalid KTX2 section offset")?;
            let end = offset
                .checked_add(len)
                .and_then(|v| usize::try_from(v).ok())
                .filter(|&end| end <= bytes.len())
                .ok_or("truncated Basis KTX2 section")?;
            if len == 0
                || ranges
                    .iter()
                    .any(|range| start < range.end && range.start < end)
            {
                return Err("overlapping or empty Basis KTX2 section".into());
            }
            ranges.push(start..end);
            Ok(&bytes[start..end])
        };
        let dfd = section(dfd_off.into(), dfd_len.into())?;
        if !matches!(dfd.len(), 44 | 60)
            || word(dfd, 0) != Some(dfd_len)
            || dfd[4..8] != [0; 4]
            || usize::from(u16::from_le_bytes([dfd[10], dfd[11]])) + 4 != dfd.len()
            || dfd[16..20] != [3, 3, 0, 0]
            || dfd[15] & 1 != 0
        {
            return Err("invalid Basis KTX2 data format descriptor or premultiplied alpha".into());
        }
        let model = dfd[12];
        let color = match (dfd[13], dfd[14]) {
            (1, 2) => TextureColorSpace::Srgb,
            (0 | 1, 1) => TextureColorSpace::Linear,
            _ => return Err("invalid Basis KTX2 color primaries or transfer function".into()),
        };
        let second = dfd.get(47).map(|channel| channel & 15);
        let (uastc, channels) = match (model, scheme, dfd[31] & 15, second) {
            (163, 1, 0, None) => (false, Channels::Rgb),
            (163, 1, 0, Some(15)) => (false, Channels::Rgba),
            (163, 1, 3, None) => (false, Channels::Red),
            (163, 1, 3, Some(4)) => (false, Channels::RedGreen),
            (166, 0 | 2, 0, None) => (true, Channels::Rgb),
            (166, 0 | 2, 3, None) => (true, Channels::Rgba),
            (166, 0 | 2, 4, None) => (true, Channels::Red),
            (166, 0 | 2, 6, None) => (true, Channels::RedGreen),
            _ => return Err("expected ETC1S/BasisLZ or UASTC LDR with supported channels".into()),
        };
        if matches!(channels, Channels::Red | Channels::RedGreen)
            && color == TextureColorSpace::Srgb
        {
            return Err(
                "Basis KTX2 red and red-green images require a linear transfer function".into(),
            );
        }
        validate_metadata(section(kv_off.into(), kv_len.into())?)?;
        let sgd = section(
            quad(bytes, 64).ok_or("missing KTX2 data offset")?,
            quad(bytes, 72).ok_or("missing KTX2 data length")?,
        )?;
        if uastc {
            if !sgd.is_empty() {
                return Err("unexpected UASTC supercompression global data".into());
            }
        } else {
            let start = 20 + levels as usize * 20;
            let end = (4..20).step_by(4).try_fold(start, |offset, field| {
                offset.checked_add(word(sgd, field)? as usize)
            });
            if end != Some(sgd.len()) || sgd.len() < start {
                return Err("invalid ETC1S global codebook layout".into());
            }
        }
        let header = Ktx2Header::parse(bytes).ok_or("invalid Basis Universal KTX2 header")?;
        debug_assert!(matches!(
            header.format,
            BasisFormat::Etc1s | BasisFormat::Uastc
        ));
        for (level, entry) in header.levels.iter().enumerate() {
            let stored = section(entry.byte_offset, entry.byte_length)?;
            if stored.is_empty() {
                return Err("missing Basis KTX2 mip data".into());
            }
            if uastc {
                let [w, h] = [width, height].map(|side| (side >> level).max(1));
                let expected = u64::from(w.div_ceil(4) * h.div_ceil(4) * 16);
                if entry.uncompressed_byte_length != expected
                    || (scheme == 0
                        && (entry.byte_length != expected || !entry.byte_offset.is_multiple_of(16)))
                {
                    return Err("UASTC mip length does not match its block dimensions".into());
                }
            } else {
                let descriptor = 20 + level * 20;
                let check_slice = |offset| {
                    let start = word(sgd, offset)?;
                    let length = word(sgd, offset + 4)?;
                    (length > 0 && start.checked_add(length)? as usize <= stored.len())
                        .then_some(())
                };
                if check_slice(descriptor + 4).is_none()
                    || (matches!(channels, Channels::Rgba | Channels::RedGreen)
                        && check_slice(descriptor + 12).is_none())
                {
                    return Err("ETC1S image slice is out of bounds".into());
                }
            }
        }
        let transcoder = Transcoder::new(bytes)
            .map_err(|error| format!("invalid Basis Universal texture ({error:?})"))?;
        if transcoder.is_video() {
            return Err("expected a still Basis Universal KTX2 texture".into());
        }
        Ok(Self {
            bytes: bytes.into(),
            size: [width, height],
            levels,
            uastc,
            channels,
            color,
            bt709: dfd[13] == 1,
            base_rgba: OnceLock::new(),
        })
    }

    pub fn size(&self) -> [u32; 2] {
        self.size
    }

    pub fn level_count(&self) -> u32 {
        self.levels
    }

    pub fn is_uastc(&self) -> bool {
        self.uastc
    }

    pub fn matches_gltf_color(&self, color: TextureColorSpace) -> bool {
        self.color == color && self.bt709 == (color == TextureColorSpace::Srgb)
    }

    pub fn linear_data(&self) -> bool {
        self.color == TextureColorSpace::Linear
    }

    #[doc(hidden)]
    pub fn source_bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub(super) fn base_rgba(&self) -> &[u8] {
        self.base_rgba.get_or_init(|| self.rgba(0..1))
    }

    pub(super) fn rgba(&self, levels: Range<u32>) -> Box<[u8]> {
        self.transcode(levels.clone(), TargetFormat::Rgba32)
            .map_or_else(
                |error| {
                    tracing::warn!(?error, "cannot decode Basis Universal texture");
                    let size: usize = levels
                        .map(|level| {
                            let [w, h] = self.size.map(|side| (side >> level).max(1));
                            (w * h * 4) as usize
                        })
                        .sum();
                    vec![0; size].into_boxed_slice()
                },
                |mut bytes| {
                    match self.channels {
                        Channels::Red => {
                            for [_, g, b, a] in bytes.as_chunks_mut::<4>().0 {
                                (*g, *b, *a) = (0, 0, 255);
                            }
                        }
                        Channels::RedGreen => {
                            for [_, g, b, a] in bytes.as_chunks_mut::<4>().0 {
                                (*g, *b, *a) = (if self.uastc { *g } else { *a }, 0, 255);
                            }
                        }
                        _ => {}
                    }
                    bytes.into_boxed_slice()
                },
            )
    }

    pub fn transcode_blocks(&self, format: MeshBlockFormat) -> Option<MeshTextureBlocks> {
        if matches!(self.channels, Channels::Red | Channels::RedGreen) {
            return None;
        }
        let (target, vulkan) = match format {
            MeshBlockFormat::Bc7 => (TargetFormat::Bc7Rgba, 145),
            MeshBlockFormat::Astc([4, 4]) => (TargetFormat::Astc4x4Rgba, 157),
            MeshBlockFormat::Etc2Rgba if !self.uastc || !self.linear_data() => {
                (TargetFormat::Etc2Rgba, 151)
            }
            _ => return None,
        };
        let bytes = self.transcode(0..self.levels, target).ok()?;
        MeshTextureBlocks::new(self.size, vulkan, self.levels, bytes)
    }

    fn transcode(
        &self,
        levels: Range<u32>,
        target: TargetFormat,
    ) -> Result<Vec<u8>, basisu::Error> {
        let transcoder = Transcoder::new(&self.bytes)?;
        let capacity = levels.clone().try_fold(0, |len, level| {
            transcoder.output_size(level, target).map(|size| len + size)
        })?;
        let mut output = Vec::with_capacity(capacity);
        for level in levels {
            output.extend(transcoder.transcode(level, target, DecodeFlags::NONE)?);
        }
        Ok(output)
    }
}

fn validate_metadata(mut data: &[u8]) -> Result<(), String> {
    while !data.is_empty() {
        let len = word(data, 0).ok_or("truncated Basis KTX2 metadata")? as usize;
        let end = len
            .checked_add(4)
            .ok_or("invalid Basis KTX2 metadata length")?;
        let entry = data
            .get(4..end)
            .ok_or("truncated Basis KTX2 metadata entry")?;
        let split = entry
            .iter()
            .position(|&byte| byte == 0)
            .ok_or("invalid Basis KTX2 metadata key")?;
        let value = &entry[split + 1..];
        let value = value.strip_suffix(&[0]).unwrap_or(value);
        match &entry[..split] {
            b"KTXorientation" if value != b"rd" => {
                return Err("Basis KTX2 requires rd orientation".into());
            }
            b"KTXswizzle" if value != b"rgba" => {
                return Err("Basis KTX2 requires RGBA channel order".into());
            }
            _ => {}
        }
        let next = end
            .checked_next_multiple_of(4)
            .ok_or("invalid Basis KTX2 metadata alignment")?;
        data = data
            .get(next..)
            .ok_or("truncated Basis KTX2 metadata padding")?;
    }
    Ok(())
}

fn word(bytes: &[u8], at: usize) -> Option<u32> {
    Some(u32::from_le_bytes(bytes.get(at..at + 4)?.try_into().ok()?))
}

fn quad(bytes: &[u8], at: usize) -> Option<u64> {
    Some(u64::from_le_bytes(bytes.get(at..at + 8)?.try_into().ok()?))
}
