//! Immutable RGBA8 images prepared from reactive pixel models.

use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{Arc, Weak},
};

use super::material::MeshImage;
use crate::VecModel;

type SnapshotCache<const D: usize, T> = RefCell<VecDeque<(((u64, u64), [u32; D]), Weak<T>)>>;

thread_local! {
    static TEXTURES: SnapshotCache<2, MeshImage> =
        const { RefCell::new(VecDeque::new()) };
    static VOLUMES: SnapshotCache<3, VolumeImage3D> =
        const { RefCell::new(VecDeque::new()) };
}

fn snapshot<const D: usize, T: 'static>(
    cache: &'static std::thread::LocalKey<SnapshotCache<D, T>>,
    size: [u32; D],
    count: usize,
    pixels: &VecModel<i64>,
    image: impl FnOnce(Vec<u8>) -> Option<T>,
) -> Option<Arc<T>> {
    if pixels.len() != count {
        return None;
    }
    let key = (pixels.contents_stamp(), size);
    if let Some(cached) = cache.with_borrow(|textures| {
        textures
            .iter()
            .find(|(id, _)| *id == key)
            .and_then(|(_, image)| image.upgrade())
    }) {
        return Some(cached);
    }
    let mut bytes = Vec::with_capacity(count);
    pixels.visit_rows_untracked(count, |_, _, value| {
        if let Ok(byte) = u8::try_from(*value) {
            bytes.push(byte);
        }
    });
    let image = Arc::new(image(bytes)?);
    cache.with_borrow_mut(|textures| {
        textures.retain(|(id, value)| *id != key && value.strong_count() != 0);
        textures.truncate(31);
        textures.push_front((key, Arc::downgrade(&image)));
    });
    Some(image)
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
        Self(snapshot(
            &TEXTURES,
            size,
            Self::byte_len(size),
            pixels,
            |bytes| MeshImage::new(size, bytes),
        ))
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

/// An immutable stack of RGBA8 slices, with X changing fastest and Z slowest.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextureData3D(Option<Arc<VolumeImage3D>>);

impl TextureData3D {
    pub const MAX_DIMENSION: u32 = 256;
    pub const MAX_BYTES: usize = 16 * 1024 * 1024;

    fn dimensions(width: i64, height: i64, depth: i64) -> Option<([u32; 3], usize)> {
        let size = [
            u32::try_from(width).ok()?,
            u32::try_from(height).ok()?,
            u32::try_from(depth).ok()?,
        ];
        if size
            .iter()
            .any(|side| !(1..=Self::MAX_DIMENSION).contains(side))
        {
            return None;
        }
        let count = size
            .into_iter()
            .map(|side| side as usize)
            .product::<usize>()
            * 4;
        (count <= Self::MAX_BYTES).then_some((size, count))
    }

    pub fn from_rgba8(width: i64, height: i64, depth: i64, voxels: Vec<u8>) -> Self {
        let Some((size, count)) = Self::dimensions(width, height, depth) else {
            return Self::default();
        };
        Self(VolumeImage3D::new(size, count, voxels).map(Arc::new))
    }

    pub fn from_ints(width: i64, height: i64, depth: i64, voxels: &[i64]) -> Self {
        let Some((size, count)) = Self::dimensions(width, height, depth) else {
            return Self::default();
        };
        if voxels.len() != count {
            return Self::default();
        }
        let voxels = voxels
            .iter()
            .copied()
            .map(u8::try_from)
            .collect::<Result<Vec<_>, _>>();
        Self(
            voxels
                .ok()
                .and_then(|bytes| VolumeImage3D::new(size, count, bytes))
                .map(Arc::new),
        )
    }

    pub fn from_model(width: i64, height: i64, depth: i64, voxels: &VecModel<i64>) -> Self {
        let Some((size, count)) = Self::dimensions(width, height, depth) else {
            return Self::default();
        };
        Self(snapshot(&VOLUMES, size, count, voxels, |bytes| {
            VolumeImage3D::new(size, count, bytes)
        }))
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
    pub fn depth(&self) -> i64 {
        self.0
            .as_ref()
            .map_or(0, |image| i64::from(image.size()[2]))
    }
    pub fn image(&self) -> Option<Arc<VolumeImage3D>> {
        self.0.clone()
    }
}

#[doc(hidden)]
pub struct VolumeImage3D {
    size: [u32; 3],
    rgba: Box<[u8]>,
}

impl VolumeImage3D {
    fn new(size: [u32; 3], count: usize, bytes: Vec<u8>) -> Option<Self> {
        (bytes.len() == count).then(|| Self {
            size,
            rgba: bytes.into_boxed_slice(),
        })
    }

    pub fn size(&self) -> [u32; 3] {
        self.size
    }
    pub fn rgba(&self) -> &[u8] {
        &self.rgba
    }
}

impl PartialEq for VolumeImage3D {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}
impl Eq for VolumeImage3D {}
impl std::fmt::Debug for VolumeImage3D {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("VolumeImage3D")
            .field("size", &self.size)
            .finish()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn volume_snapshots_keep_slice_order_and_apply_bounds() {
        let voxels = VecModel::from(vec![17, 34, 68, 85, 102, 119, 136, 153]);
        let before = TextureData3D::from_model(1, 1, 2, &voxels);
        assert_eq!(before.depth(), 2);
        assert_eq!(before, TextureData3D::from_model(1, 1, 2, &voxels));
        assert!(voxels.set_row(4, 255));
        let after = TextureData3D::from_model(1, 1, 2, &voxels);
        assert_eq!(before.image().unwrap().rgba()[4], 102);
        assert_eq!(after.image().unwrap().rgba()[4], 255);
        assert!(!TextureData3D::from_model(1, 1, 0, &voxels).is_valid());
        assert!(!TextureData3D::from_model(256, 256, 256, &voxels).is_valid());
    }

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
