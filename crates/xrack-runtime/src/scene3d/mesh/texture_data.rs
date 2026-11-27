//! Immutable RGBA8 images prepared from reactive pixel models.

use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{Arc, Weak},
};

use super::material::MeshImage;
use crate::VecModel;

type TextureKey = ((u64, u64), [u32; 2]);

thread_local! {
    static TEXTURES: RefCell<VecDeque<(TextureKey, Weak<MeshImage>)>> =
        const { RefCell::new(VecDeque::new()) };
}

/// A snapshot of top-to-bottom RGBA8 pixels with straight alpha.
#[derive(Clone, Default)]
pub struct TextureData2D(Option<Arc<MeshImage>>);

impl TextureData2D {
    // Bound raw data to 16 MiB before allocating or reading a pixel model.
    pub const MAX_DIMENSION: u32 = 2048;

    pub fn from_rgba8(width: i64, height: i64, pixels: Vec<u8>) -> Self {
        Self(
            Self::dimensions(width, height)
                .and_then(|size| MeshImage::new(size, pixels))
                .map(Arc::new),
        )
    }

    pub fn from_ints(width: i64, height: i64, pixels: &[i64]) -> Self {
        let Some(size) = Self::dimensions(width, height) else {
            return Self::default();
        };
        if pixels.len() != Self::byte_len(size) {
            return Self::default();
        }
        let pixels = pixels
            .iter()
            .copied()
            .map(u8::try_from)
            .collect::<Result<Vec<_>, _>>();
        Self(
            pixels
                .ok()
                .and_then(|pixels| MeshImage::new(size, pixels))
                .map(Arc::new),
        )
    }

    pub fn from_model(width: i64, height: i64, pixels: &VecModel<i64>) -> Self {
        let Some(size) = Self::dimensions(width, height) else {
            return Self::default();
        };
        let count = Self::byte_len(size);
        if pixels.len() != count {
            return Self::default();
        }
        let key = (pixels.contents_stamp(), size);
        let cached = TEXTURES.with_borrow(|textures| {
            textures
                .iter()
                .find(|(id, _)| *id == key)
                .and_then(|(_, image)| image.upgrade())
        });
        if cached.is_some() {
            return Self(cached);
        }
        let mut bytes = Vec::with_capacity(count);
        pixels.visit_rows_untracked(count, |_, _, value| {
            if let Ok(byte) = u8::try_from(*value) {
                bytes.push(byte);
            }
        });
        let image = MeshImage::new(size, bytes).map(Arc::new);
        if let Some(image) = &image {
            TEXTURES.with_borrow_mut(|textures| {
                textures.retain(|(id, value)| *id != key && value.strong_count() != 0);
                textures.truncate(31);
                textures.push_front((key, Arc::downgrade(image)));
            });
        }
        Self(image)
    }

    fn dimensions(width: i64, height: i64) -> Option<[u32; 2]> {
        let size = [u32::try_from(width).ok()?, u32::try_from(height).ok()?];
        size.iter()
            .all(|side| (1..=Self::MAX_DIMENSION).contains(side))
            .then_some(size)
    }

    fn byte_len([width, height]: [u32; 2]) -> usize {
        width as usize * height as usize * 4
    }

    pub fn is_valid(&self) -> bool {
        self.0.is_some()
    }

    pub fn width(&self) -> i64 {
        self.0
            .as_ref()
            .map_or(0, |image| i64::from(image.size()[0]))
    }

    pub fn height(&self) -> i64 {
        self.0
            .as_ref()
            .map_or(0, |image| i64::from(image.size()[1]))
    }

    pub fn image(&self) -> Option<Arc<MeshImage>> {
        self.0.clone()
    }
}

impl PartialEq for TextureData2D {
    fn eq(&self, other: &Self) -> bool {
        match (&self.0, &other.0) {
            (Some(left), Some(right)) => Arc::ptr_eq(left, right),
            (None, None) => true,
            _ => false,
        }
    }
}

impl std::fmt::Debug for TextureData2D {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("TextureData2D")
            .field("width", &self.width())
            .field("height", &self.height())
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixel_snapshots_follow_model_revisions_and_validate_all_channels() {
        let pixels = VecModel::from(vec![18, 42, 70, 255]);
        let before = TextureData2D::from_model(1, 1, &pixels);
        assert!(before.is_valid());
        assert_eq!(TextureData2D::from_model(1, 1, &pixels), before);
        assert!(pixels.set_row(0, 128));
        let after = TextureData2D::from_model(1, 1, &pixels);
        assert_ne!(before, after);
        assert_eq!(before.image().unwrap().rgba(), &[18, 42, 70, 255]);
        assert_eq!(after.image().unwrap().rgba(), &[128, 42, 70, 255]);
        assert!(pixels.set_row(3, 256));
        assert!(!TextureData2D::from_model(1, 1, &pixels).is_valid());
        assert!(!TextureData2D::from_model(0, 1, &pixels).is_valid());
        assert!(!TextureData2D::from_model(2, 1, &pixels).is_valid());
    }
}
