//! KTX2 GPU blocks and on-demand CPU pixels for unsupported devices and samplers.

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshBlockFormat {
    Bc1Rgb,
    Bc1Rgba,
    Bc2,
    Bc3,
    Bc4,
    Bc5,
    Bc7,
    Etc2Rgb,
    Etc2RgbA1,
    Etc2Rgba,
    Astc([u8; 2]),
}

impl MeshBlockFormat {
    const ASTC_FOOTPRINTS: [[u8; 2]; 14] = [
        [4, 4],
        [5, 4],
        [5, 5],
        [6, 5],
        [6, 6],
        [8, 5],
        [8, 6],
        [8, 8],
        [10, 5],
        [10, 6],
        [10, 8],
        [10, 10],
        [12, 10],
        [12, 12],
    ];

    pub fn from_vulkan(format: u32) -> Option<Self> {
        Some(match format {
            131 | 132 => Self::Bc1Rgb,
            133 | 134 => Self::Bc1Rgba,
            135 | 136 => Self::Bc2,
            137 | 138 => Self::Bc3,
            139 => Self::Bc4,
            141 => Self::Bc5,
            145 | 146 => Self::Bc7,
            147 | 148 => Self::Etc2Rgb,
            149 | 150 => Self::Etc2RgbA1,
            151 | 152 => Self::Etc2Rgba,
            157..=184 => Self::Astc(Self::ASTC_FOOTPRINTS[((format - 157) / 2) as usize]),
            _ => return None,
        })
    }

    pub fn block_size(self) -> [u32; 2] {
        match self {
            Self::Astc(size) => size.map(u32::from),
            _ => [4, 4],
        }
    }

    pub fn block_bytes(self) -> usize {
        match self {
            Self::Bc1Rgb | Self::Bc1Rgba | Self::Bc4 | Self::Etc2Rgb | Self::Etc2RgbA1 => 8,
            _ => 16,
        }
    }

    pub fn level_bytes(self, [width, height]: [u32; 2]) -> usize {
        let [cols, rows] = self.block_grid([width, height]);
        cols as usize * rows as usize * self.block_bytes()
    }

    fn block_grid(self, [width, height]: [u32; 2]) -> [u32; 2] {
        let [bw, bh] = self.block_size();
        [width.div_ceil(bw), height.div_ceil(bh)]
    }

    fn decode(self, blocks: &[u8], [width, height]: [u32; 2], out: &mut [u8]) {
        let [bw, bh] = self.block_size();
        let [cols, _] = self.block_grid([width, height]);
        let mut pixels = [0; 12 * 12 * 4];
        for (block, source) in blocks.chunks_exact(self.block_bytes()).enumerate() {
            self.decode_block(source, &mut pixels);
            let x = block as u32 % cols * bw;
            let y = block as u32 / cols * bh;
            for row in 0..bh.min(height - y) {
                let start = ((y + row) * width + x) as usize * 4;
                let decoded_start = (row * bw) as usize * 4;
                let bytes = bw.min(width - x) as usize * 4;
                out[start..start + bytes]
                    .copy_from_slice(&pixels[decoded_start..decoded_start + bytes]);
            }
        }
    }

    fn decode_block(self, source: &[u8], rgba: &mut [u8]) {
        match self {
            Self::Bc1Rgb | Self::Bc1Rgba => {
                bcdec_rs::bc1(source, rgba, 16);
                if self == Self::Bc1Rgb {
                    for pixel in rgba[..64].chunks_exact_mut(4) {
                        pixel[3] = 255;
                    }
                }
            }
            Self::Bc2 => bcdec_rs::bc2(source, rgba, 16),
            Self::Bc3 => bcdec_rs::bc3(source, rgba, 16),
            Self::Bc4 => {
                let mut red = [0; 16];
                bcdec_rs::bc4(source, &mut red, 4, false);
                for (pixel, red) in rgba.chunks_exact_mut(4).zip(red) {
                    pixel.copy_from_slice(&[red, 0, 0, 255]);
                }
            }
            Self::Bc5 => {
                let mut rg = [[0; 2]; 16];
                bcdec_rs::bc5(source, rg.as_flattened_mut(), 8, false);
                for (pixel, [r, g]) in rgba.chunks_exact_mut(4).zip(rg) {
                    pixel.copy_from_slice(&[r, g, 0, 255]);
                }
            }
            Self::Bc7 => bcdec_rs::bc7(source, rgba, 16),
            Self::Etc2Rgb | Self::Etc2RgbA1 | Self::Etc2Rgba => {
                let mut pixels = [0; 16];
                match self {
                    Self::Etc2Rgb => texture2ddecoder::decode_etc2_rgb_block(source, &mut pixels),
                    Self::Etc2RgbA1 => {
                        texture2ddecoder::decode_etc2_rgba1_block(source, &mut pixels)
                    }
                    _ => texture2ddecoder::decode_etc2_rgba8_block(source, &mut pixels),
                }
                for (pixel, bgra) in rgba.chunks_exact_mut(4).zip(pixels) {
                    let [b, g, r, a] = bgra.to_le_bytes();
                    pixel.copy_from_slice(&[r, g, b, a]);
                }
            }
            Self::Astc([bw, bh]) => {
                let Ok(source) = <&[u8; 16]>::try_from(source) else {
                    return;
                };
                let footprint = astc_decode::Footprint::new(u32::from(bw), u32::from(bh));
                astc_decode::astc_decode_block(source, footprint, |x, y, pixel| {
                    let start = (y * u32::from(bw) + x) as usize * 4;
                    rgba[start..start + 4].copy_from_slice(&pixel);
                });
            }
        }
    }
}

#[derive(Debug)]
pub struct MeshTextureBlocks {
    size: [u32; 2],
    format: MeshBlockFormat,
    vulkan: u32,
    level_count: u32,
    bytes: Box<[u8]>,
    native_equivalent: bool,
}

impl MeshTextureBlocks {
    pub(super) const PACKED_TAG: &[u8] = b"XRBT";

    pub(super) fn new(
        size: [u32; 2],
        vulkan: u32,
        level_count: u32,
        bytes: Vec<u8>,
    ) -> Option<Self> {
        let format = MeshBlockFormat::from_vulkan(vulkan)?;
        if !(1..=size.into_iter().max()?.checked_ilog2()? + 1).contains(&level_count) {
            return None;
        }
        let expected: usize = (0..level_count)
            .map(|level| format.level_bytes(size.map(|side| (side >> level).max(1))))
            .sum();
        if bytes.len() != expected {
            return None;
        }
        // WebGPU only exposes the RGBA form of BC1. If RGB uses the fourth
        // selector in a three-color block, it requires an opaque black texel.
        let native_equivalent = format != MeshBlockFormat::Bc1Rgb
            || bytes.as_chunks::<8>().0.iter().all(|block| {
                let a = u16::from_le_bytes([block[0], block[1]]);
                let b = u16::from_le_bytes([block[2], block[3]]);
                let selectors = u32::from_le_bytes([block[4], block[5], block[6], block[7]]);
                a > b || selectors & (selectors >> 1) & 0x5555_5555 == 0
            });
        Some(Self {
            size,
            format,
            vulkan,
            level_count,
            bytes: bytes.into_boxed_slice(),
            native_equivalent,
        })
    }

    pub fn format(&self) -> MeshBlockFormat {
        self.format
    }

    pub fn level_count(&self) -> u32 {
        self.level_count
    }

    pub fn native_equivalent(&self) -> bool {
        self.native_equivalent
    }

    pub fn levels(&self) -> impl Iterator<Item = ([u32; 2], &[u8])> {
        (0..self.level_count).scan(self.bytes.as_ref(), |bytes, level| {
            let size = self.size.map(|side| (side >> level).max(1));
            let (level, rest) = bytes.split_at(self.format.level_bytes(size));
            *bytes = rest;
            Some((size, level))
        })
    }

    pub(super) fn rgba_levels<'a>(
        &self,
        levels: impl Iterator<Item = ([u32; 2], &'a [u8])>,
        len: usize,
    ) -> Box<[u8]> {
        let mut pixels = vec![0; len].into_boxed_slice();
        let mut out = &mut pixels[..];
        for ([width, height], input) in levels {
            let (level, rest) = out.split_at_mut(width as usize * height as usize * 4);
            self.format.decode(input, [width, height], level);
            out = rest;
        }
        pixels
    }

    #[doc(hidden)]
    pub fn pack(&self) -> Vec<u8> {
        let mut bytes = Vec::with_capacity(12 + self.bytes.len());
        bytes.extend_from_slice(Self::PACKED_TAG);
        bytes.extend(self.vulkan.to_le_bytes());
        bytes.extend(self.level_count.to_le_bytes());
        bytes.extend_from_slice(&self.bytes);
        bytes
    }

    pub(super) fn unpack(size: [u32; 2], bytes: &[u8]) -> Option<Self> {
        let [vulkan, levels] = bytes.get(4..12)?.as_chunks::<4>().0.try_into().ok()?;
        Self::new(
            size,
            u32::from_le_bytes(vulkan),
            u32::from_le_bytes(levels),
            bytes.get(12..)?.to_vec(),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene3d::{MeshImage, TextureColorSpace};

    #[test]
    fn truncated_levels_and_unknown_formats_are_rejected() {
        assert!(MeshImage::from_blocks([4, 4], 138, 2, vec![0; 31]).is_none());
        assert!(MeshImage::from_blocks([4, 4], 138, 4, vec![0; 64]).is_none());
        assert!(MeshImage::from_blocks([4, 4], 137, 0, Vec::new()).is_none());
        assert!(MeshImage::from_blocks([4, 4], 143, 1, vec![0; 16]).is_none());
        assert!(MeshImage::from_packed([4, 4], b"XRBT").is_none());
    }

    #[test]
    fn bc1_preserves_distinct_alpha_semantics_and_authored_levels() -> Result<(), String> {
        let black = [0, 0, 0xff, 0xff, 0xff, 0xff, 0xff, 0xff];
        let green = [0xe0, 0x07, 0, 0, 0, 0, 0, 0];
        let data = [black, green].concat();
        for (format, alpha) in [(132, 255), (134, 0)] {
            let image =
                MeshImage::from_blocks([3, 4], format, 2, data.clone()).ok_or("invalid fixture")?;
            let blocks = image.blocks().ok_or("expected compressed pixels")?;
            assert_eq!(
                blocks.format() == MeshBlockFormat::Bc1Rgba,
                blocks.native_equivalent()
            );
            let packed = blocks.pack();
            let packed = MeshImage::from_packed([3, 4], &packed).ok_or("cannot unpack image")?;
            assert_eq!(packed.rgba(), [0, 0, 0, alpha].repeat(12));
            assert_eq!(packed.mip_extents().collect::<Vec<_>>(), [[1, 2]]);
            let linear = packed.prepare_mips(TextureColorSpace::Linear);
            assert_eq!(linear, [0, 255, 0, 255].repeat(2));
            assert!(std::ptr::eq(
                linear,
                packed.prepare_mips(TextureColorSpace::Srgb)
            ));
        }
        Ok(())
    }
}
