//! Prepared texture data shared by generated and interpreted scenes.

use std::{io::Read, sync::Arc};

use super::material::MeshImage;

#[derive(Clone, Debug)]
pub enum TextureAsset3D {
    Image(Arc<MeshImage>),
    Cubemap(Arc<CubeTexture3D>),
}

impl TextureAsset3D {
    pub fn into_image(self) -> Option<Arc<MeshImage>> {
        match self {
            Self::Image(image) => Some(image),
            Self::Cubemap(_) => None,
        }
    }
}

/// Linear RGBA16F texels in mip-major order. Each mip stores six faces in
/// +X, -X, +Y, -Y, +Z, -Z order with rows running from top to bottom.
pub struct CubeTexture3D {
    side: u32,
    levels: u32,
    texels: Box<[u8]>,
}

impl CubeTexture3D {
    const MAX_FACE_PIXELS: u32 = (8 * 1024 * 1024) / 6;

    fn byte_len(side: u32, levels: u32) -> Option<usize> {
        if side.checked_mul(side)? > Self::MAX_FACE_PIXELS
            || !(1..=side.checked_ilog2()? + 1).contains(&levels)
        {
            return None;
        }
        Some(
            (0..levels)
                .map(|level| Self::level_len(side >> level))
                .sum(),
        )
    }

    fn level_len(side: u32) -> usize {
        side as usize * side as usize * 6 * 8
    }

    #[doc(hidden)]
    pub fn new(side: u32, levels: u32, texels: Vec<u8>) -> Option<Self> {
        (Self::byte_len(side, levels)? == texels.len()).then(|| Self {
            side,
            levels,
            texels: texels.into_boxed_slice(),
        })
    }

    #[doc(hidden)]
    pub fn from_packed(side: u32, levels: u32, compressed: &[u8]) -> Option<Self> {
        let len = Self::byte_len(side, levels)?;
        let mut texels = Vec::with_capacity(len);
        flate2::read::ZlibDecoder::new(compressed)
            .take(len as u64 + 1)
            .read_to_end(&mut texels)
            .ok()?;
        Self::new(side, levels, texels)
    }

    pub fn side(&self) -> u32 {
        self.side
    }

    pub fn levels(&self) -> u32 {
        self.levels
    }

    pub fn texels(&self) -> &[u8] {
        &self.texels
    }

    pub fn mip_data(&self) -> impl Iterator<Item = (u32, &[u8])> {
        let mut offset = 0;
        (0..self.levels).map(move |level| {
            let side = self.side >> level;
            let start = offset;
            offset += Self::level_len(side);
            (side, &self.texels[start..offset])
        })
    }
}

impl std::fmt::Debug for CubeTexture3D {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CubeTexture3D")
            .field("side", &self.side)
            .field("levels", &self.levels)
            .finish()
    }
}

impl PartialEq for CubeTexture3D {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl Eq for CubeTexture3D {}
