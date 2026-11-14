use crate::{Image, ImageFormat, InvalidNineSlice, NineSlice};
use std::{
    fmt,
    rc::Rc,
    sync::{
        Arc,
        atomic::{AtomicU64, Ordering},
    },
};

/// The image value accepted by layout, rendering, and playback.
#[derive(Clone, Debug)]
pub enum ImageAsset {
    Embedded(Image),
    Loaded(LoadedImage),
    Animated(Rc<LoadedAnimation>),
}

impl PartialEq for ImageAsset {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Embedded(left), Self::Embedded(right)) => left == right,
            (Self::Loaded(left), Self::Loaded(right)) => left == right,
            (Self::Animated(left), Self::Animated(right)) => {
                Rc::ptr_eq(left, right) || left == right
            }
            (Self::Embedded(Image::Animated(embedded)), Self::Animated(loaded))
            | (Self::Animated(loaded), Self::Embedded(Image::Animated(embedded))) => {
                embedded.matches_loaded(loaded)
            }
            (Self::Loaded(_), Self::Embedded(_)) | (Self::Embedded(_), Self::Loaded(_)) => {
                let (Some(left), Some(right)) = (self.pixels(), other.pixels()) else {
                    return false;
                };
                left.format == right.format
                    && left.size == right.size
                    && left.edges == right.edges
                    && left.bytes == right.bytes
            }
            _ => false,
        }
    }
}

impl Eq for ImageAsset {}

impl Default for ImageAsset {
    fn default() -> Self {
        Self::Embedded(Image::Empty)
    }
}

impl From<Image> for ImageAsset {
    fn from(image: Image) -> Self {
        match image {
            Image::Loaded(image) => Self::Loaded(image),
            embedded => Self::Embedded(embedded),
        }
    }
}

#[derive(Clone)]
pub struct LoadedImage {
    source: Arc<LoadedImageSource>,
    edges: Option<NineSlice>,
}

struct LoadedImageSource {
    id: u64,
    format: ImageFormat,
    bytes: Vec<u8>,
    size: [u32; 2],
}

impl LoadedImage {
    pub fn new(format: ImageFormat, bytes: Vec<u8>, size: [u32; 2]) -> Self {
        static NEXT_IMAGE: AtomicU64 = AtomicU64::new(1);
        let id = NEXT_IMAGE
            .try_update(Ordering::Relaxed, Ordering::Relaxed, |id| id.checked_add(1))
            .expect("loaded image identity exhausted");
        Self {
            source: Arc::new(LoadedImageSource {
                id,
                format,
                bytes,
                size,
            }),
            edges: None,
        }
    }

    pub fn size(&self) -> [u32; 2] {
        self.source.size
    }

    pub(crate) fn with_nine_slice(mut self, edges: NineSlice) -> Result<Self, InvalidNineSlice> {
        if !edges.fits(self.source.size) {
            return Err(InvalidNineSlice);
        }
        self.edges = Some(edges);
        Ok(self)
    }

    pub(crate) fn bytes(&self) -> &[u8] {
        &self.source.bytes
    }
}

impl PartialEq for LoadedImage {
    fn eq(&self, other: &Self) -> bool {
        self.edges == other.edges
            && (Arc::ptr_eq(&self.source, &other.source)
                || (self.source.size == other.source.size
                    && self.source.format == other.source.format
                    && self.source.bytes == other.source.bytes))
    }
}

impl Eq for LoadedImage {}

impl fmt::Debug for LoadedImage {
    fn fmt(&self, output: &mut fmt::Formatter<'_>) -> fmt::Result {
        output
            .debug_struct("LoadedImage")
            .field("format", &self.source.format)
            .field("size", &self.source.size)
            .field("edges", &self.edges)
            .finish()
    }
}

#[derive(Debug, PartialEq, Eq)]
pub struct LoadedAnimation {
    pub(crate) size: [u32; 2],
    pub(crate) frames: Vec<(u32, LoadedImage)>,
    pub(crate) plays: u32,
}

impl LoadedAnimation {
    pub fn new(size: [u32; 2], frames: Vec<(u32, Vec<u8>)>, plays: u32) -> Self {
        Self {
            size,
            frames: frames
                .into_iter()
                .map(|(end, bytes)| (end, LoadedImage::new(ImageFormat::Png, bytes, size)))
                .collect(),
            plays,
        }
    }

    #[doc(hidden)]
    pub fn frames(&self) -> impl ExactSizeIterator<Item = (u32, &[u8])> {
        self.frames.iter().map(|(end, frame)| (*end, frame.bytes()))
    }
}

/// Identity for a cache entry. Loaded assets can outlive their original scene
/// without making a later allocation at the same address hit an old texture.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum ImageIdentity {
    Embedded(usize),
    Loaded(u64),
}

pub struct ImagePixels<'a> {
    pub identity: ImageIdentity,
    pub format: ImageFormat,
    pub bytes: &'a [u8],
    pub size: [u32; 2],
    pub edges: Option<NineSlice>,
}

impl ImageAsset {
    pub fn size(&self) -> [u32; 2] {
        match self {
            Self::Embedded(image) => image.size(),
            Self::Loaded(image) => image.source.size,
            Self::Animated(animation) => animation.size,
        }
    }

    pub fn frame_count(&self) -> usize {
        match self {
            Self::Embedded(image) => image.frame_count(),
            Self::Loaded(_) => 1,
            Self::Animated(animation) => animation.frames.len(),
        }
    }

    pub fn frame(&self, index: usize) -> Self {
        match self {
            Self::Embedded(image) => image.clone().frame(index).into(),
            Self::Loaded(_) => self.clone(),
            Self::Animated(animation) => animation
                .frames
                .get(index)
                .or_else(|| animation.frames.first())
                .map_or_else(Self::default, |(_, image)| Self::Loaded(image.clone())),
        }
    }

    pub fn with_nine_slice(self, edges: NineSlice) -> Result<Self, InvalidNineSlice> {
        match self {
            Self::Embedded(image) => image.with_nine_slice(edges).map(Self::Embedded),
            Self::Loaded(image) => image.with_nine_slice(edges).map(Self::Loaded),
            _ => Err(InvalidNineSlice),
        }
    }

    pub fn pixels(&self) -> Option<ImagePixels<'_>> {
        let (identity, format, bytes, size, edges) = match self {
            Self::Embedded(Image::Embedded {
                format,
                bytes,
                size,
            }) => (
                ImageIdentity::Embedded(bytes.as_ptr() as usize),
                *format,
                *bytes,
                *size,
                None,
            ),
            Self::Embedded(Image::NineSliced {
                format,
                bytes,
                size,
                edges,
            }) => (
                ImageIdentity::Embedded(bytes.as_ptr() as usize),
                *format,
                *bytes,
                *size,
                Some(*edges),
            ),
            Self::Loaded(image) => (
                ImageIdentity::Loaded(image.source.id),
                image.source.format,
                image.bytes(),
                image.source.size,
                image.edges,
            ),
            _ => return None,
        };
        Some(ImagePixels {
            identity,
            format,
            bytes,
            size,
            edges,
        })
    }
}
