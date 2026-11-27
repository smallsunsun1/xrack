pub(crate) mod image_colorize;
pub(crate) mod image_coverage;
pub(crate) mod nine_slice;
pub(crate) mod scroll_images;
pub(crate) use crate::image_colorize::Colorize;
use crate::{
    CanvasTransform,
    image_colorize::{ColorizeGeometry, GradientTexture},
    image_coverage::{Coverage, CoverageAtlas},
};
use femtovg::{
    Atlas as PackingAtlas, Canvas, Color, CompositeOperation, ImageFlags, ImageId, ImageInfo,
    ImageSource, Paint, Path, PixelFormat, Quad,
    renderer::{RenderTarget, WGPURenderer},
};
use rgb::{FromSlice, Gray, RGBA8};
use std::{
    cell::OnceCell,
    collections::{HashMap, hash_map::Entry},
    rc::Rc,
};
use xrack_assets::{MAX_IMAGE_DIMENSION, Raster, decode_png, rasterize_svg_region};
use xrack_layout::{ImagePresentation, ImageRegion, LayoutNode, Point, Rect, Size, Transform2D};
use xrack_runtime::{
    Color as UiColor, ImageAsset, ImageFit, ImageFormat, ImageHorizontalAlignment, ImageIdentity,
    ImagePixels, ImageRendering, ImageVerticalAlignment, MediaFrame, Node, Value,
};

pub(super) struct PlainImage<'a> {
    source: Option<&'a Value>,
    fit: Option<ImageFit>,
    rendering: ImageRendering,
    horizontal: Option<ImageHorizontalAlignment>,
    vertical: Option<ImageVerticalAlignment>,
}

impl<'a> PlainImage<'a> {
    pub(super) fn from_node(node: &'a Node, layout: &LayoutNode) -> Option<Self> {
        if node.kind != "Image"
            || node.opacity() != 1.0
            || !node.children.is_empty()
            || !layout.local_transform().is_identity()
            || layout.table_pane_clip().is_some()
            || layout.allows_overflow()
        {
            return None;
        }
        let mut image = Self {
            source: None,
            fit: None,
            rendering: ImageRendering::Smooth,
            horizontal: None,
            vertical: None,
        };
        // Collect presentation while checking the properties that are safe
        // in an ordinary quad. The full painter handles everything else.
        for (&name, value) in &node.properties {
            match (name, value) {
                ("source", value) => image.source = Some(value),
                ("image-fit", Value::ImageFit(fit)) => image.fit = Some(*fit),
                ("image-rendering", Value::ImageRendering(rendering)) => {
                    image.rendering = *rendering
                }
                ("horizontal-alignment", Value::ImageHorizontalAlignment(alignment)) => {
                    image.horizontal = Some(*alignment);
                }
                ("vertical-alignment", Value::ImageVerticalAlignment(alignment)) => {
                    image.vertical = Some(*alignment);
                }
                (
                    "x"
                    | "y"
                    | "width"
                    | "height"
                    | "visible"
                    | "enabled"
                    | "z"
                    | "opacity"
                    | "image-fit"
                    | "image-rendering"
                    | "horizontal-alignment"
                    | "vertical-alignment"
                    | "preferred-width"
                    | "preferred-height"
                    | "min-width"
                    | "min-height"
                    | "max-width"
                    | "max-height"
                    | "horizontal-stretch"
                    | "vertical-stretch",
                    _,
                ) => {}
                _ => return None,
            }
        }
        Some(image)
    }

    pub(super) fn asset(&self) -> Option<ImageAsset> {
        match self.source? {
            Value::Image(image) => Some(image.clone().into()),
            Value::ImageAsset(asset) => Some(asset.clone()),
            _ => None,
        }
    }

    pub(super) fn presentation(&self, size: [u32; 2], in_layout: bool) -> ImagePresentation {
        let default_fit = if in_layout {
            ImageFit::Contain
        } else {
            ImageFit::Fill
        };
        ImagePresentation::whole_with_style(
            size,
            self.fit.unwrap_or(default_fit),
            self.rendering,
            self.horizontal,
            self.vertical,
        )
    }
}

pub(super) fn supports_quad_transform(transform: CanvasTransform) -> bool {
    let Some([sx, sy]) = transform.axis_scales() else {
        return false;
    };
    let [x, y] = transform.translation();
    sx > 0.0 && sy > 0.0 && [sx, sy, x, y].into_iter().all(f32::is_finite)
}

pub(super) fn quad_edges_are_pixel_aligned(bounds: Rect, transform: CanvasTransform) -> bool {
    let start = transform.map_point(Point {
        x: bounds.x,
        y: bounds.y,
    });
    let end = transform.map_point(Point {
        x: bounds.x + bounds.width,
        y: bounds.y + bounds.height,
    });
    [start.x, start.y, end.x, end.y]
        .into_iter()
        .all(|pixel| pixel.is_finite() && pixel.fract() == 0.0)
}

#[derive(Clone, Copy)]
pub(super) enum QuadBounds {
    Aligned(Option<Rect>),
    Snapped(Rect),
    Antialiased(Rect, f32),
}

pub(super) struct ImageQuad<'a> {
    pub(super) asset: &'a ImageAsset,
    pub(super) bounds: Rect,
    pub(super) clip: Rect,
    pub(super) coverage: QuadBounds,
    pub(super) presentation: ImagePresentation,
    pub(super) transform: CanvasTransform,
}

pub(super) fn quad_pixel_coverage(
    bounds: Rect,
    transform: CanvasTransform,
    dpi: f32,
) -> Option<Rect> {
    if quad_edges_are_pixel_aligned(bounds, transform) {
        return Some(bounds);
    }
    let [sx, sy] = transform.axis_scales()?;
    if dpi <= 1.0 || bounds.width * sx <= 1.0 || bounds.height * sy <= 1.0 {
        return None;
    }
    let [tx, ty] = transform.translation();
    let device = transform.map_rect(bounds);
    let edges = [
        device.x,
        device.y,
        device.x + device.width,
        device.y + device.height,
    ];
    let snapped = edges.map(f32::round);
    // A rectangular fill and its scissor both reach full coverage half a
    // fringe inside the edge. If no pixel center intersects the fringe, a
    // snapped quad covers exactly the same samples. Keep the original UVs.
    let slack = 0.5 - 0.5 / dpi;
    if !edges
        .iter()
        .zip(snapped)
        .all(|(edge, pixel)| (*edge - pixel).abs() <= slack)
    {
        return None;
    }
    let [left, top, right, bottom] = snapped;
    Some(Rect::new(
        (left - tx) / sx,
        (top - ty) / sy,
        (right - left) / sx,
        (bottom - top) / sy,
    ))
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RasterKey {
    identity: ImageIdentity,
    len: usize,
    format: ImageFormat,
    size: [u32; 2],
    svg_region: Option<[u32; 4]>,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct Key {
    raster: RasterKey,
    region: Option<[u32; 4]>,
    repeat: [bool; 2],
    nearest: bool,
    alpha_mask: bool,
}

impl Key {
    fn sampling(self) -> ImageFlags {
        let mut flags = ImageFlags::empty();
        flags.set(ImageFlags::NEAREST, self.nearest);
        flags.set(ImageFlags::REPEAT_X, self.repeat[0]);
        flags.set(ImageFlags::REPEAT_Y, self.repeat[1]);
        flags
    }

    fn colorize_geometry(self, source: [u32; 2]) -> ColorizeGeometry {
        let [source_x, source_y, source_w, source_h] = self
            .raster
            .svg_region
            .unwrap_or([0, 0, source[0], source[1]]);
        let [raster_w, raster_h] = self.raster.size;
        let [region_x, region_y, _, _] = self.region.unwrap_or([0, 0, raster_w, raster_h]);
        let sx = raster_w as f32 / source_w as f32;
        let sy = raster_h as f32 / source_h as f32;
        ColorizeGeometry {
            source,
            transform: Transform2D::from_coefficients([
                sx,
                0.0,
                0.0,
                sy,
                -(source_x as f32 * sx + region_x as f32),
                -(source_y as f32 * sy + region_y as f32),
            ]),
        }
    }
}

struct Texture {
    id: ImageId,
    size: [u32; 2],
    used: bool,
    gradient: Option<GradientTexture>,
}

struct Atlas {
    raster: Raster,
    last_used: u64,
}

/// Keep source pixels for changing PNG atlas cells. GPU textures still retain
/// only the regions used this frame, and large sources use a draw-local raster.
#[derive(Default)]
struct AtlasCache {
    entries: HashMap<RasterKey, Atlas>,
    bytes: usize,
    clock: u64,
}

impl AtlasCache {
    fn raster<'a>(
        &'a mut self,
        key: RasterKey,
        bytes: &[u8],
        scratch: &'a mut Option<Raster>,
    ) -> Result<&'a Raster, String> {
        const MAX_BYTES: usize = 16 * 1024 * 1024;
        const MAX_ATLASES: usize = 128;
        if let Some(raster) = scratch {
            return Ok(raster);
        }
        self.clock = self.clock.wrapping_add(1);
        if !self.entries.contains_key(&key) {
            let raster = decode_png(bytes)?;
            if raster.rgba.len() > MAX_BYTES {
                return Ok(scratch.insert(raster));
            }
            while self.bytes + raster.rgba.len() > MAX_BYTES || self.entries.len() >= MAX_ATLASES {
                let oldest = *self
                    .entries
                    .iter()
                    .min_by_key(|(_, atlas)| atlas.last_used)
                    .ok_or("image atlas cache accounting is inconsistent")?
                    .0;
                self.bytes -= self
                    .entries
                    .remove(&oldest)
                    .ok_or("missing image atlas")?
                    .raster
                    .rgba
                    .len();
            }
            self.bytes += raster.rgba.len();
            self.entries.insert(
                key,
                Atlas {
                    raster,
                    last_used: self.clock,
                },
            );
        }
        let atlas = self.entries.get_mut(&key).ok_or("missing image atlas")?;
        atlas.last_used = self.clock;
        Ok(&atlas.raster)
    }
}

struct VideoTexture {
    id: ImageId,
    frame: MediaFrame,
    used: bool,
}

const BATCH_ATLAS_SIZE: usize = 1024;
const BATCH_TILE_LIMIT: u32 = 64;

/// A bounded, straight-RGBA atlas for linear-sampled PNGs and solid fills.
struct BatchAtlas {
    image: ImageId,
    texture: wgpu::Texture,
    compositor_binding: OnceCell<wgpu::BindGroup>,
    packing: PackingAtlas,
    tiles: HashMap<BatchTile, usize>,
    slots: Vec<BatchSlot>,
    retired: Option<HashMap<[u32; 2], RetiredSlots>>,
    uploads: Vec<BatchUpload>,
    frame: u64,
    used: bool,
    key: Rc<()>,
    pinned: bool,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum BatchTile {
    Raster(RasterKey),
    Solid(UiColor),
}

impl BatchTile {
    fn size(self) -> [u32; 2] {
        match self {
            Self::Raster(key) => key.size,
            Self::Solid(_) => [1, 1],
        }
    }
}

struct BatchSlot {
    key: BatchTile,
    position: [usize; 2],
    last_frame: u64,
}

#[derive(Default)]
struct RetiredSlots {
    stale: Vec<usize>,
    previous_frame: Vec<usize>,
}

struct BatchUpload {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    pixels: Vec<RGBA8>,
}

impl BatchAtlas {
    fn lookup(&mut self, key: BatchTile) -> Option<[usize; 2]> {
        let tile = &mut self.slots[*self.tiles.get(&key)?];
        tile.last_frame = self.frame;
        self.used = true;
        Some(tile.position)
    }

    fn store(
        &mut self,
        key: BatchTile,
        position: [usize; 2],
        replaced: Option<usize>,
        pixels: Vec<RGBA8>,
    ) {
        let [x, y] = position;
        let [width, height] = key.size().map(|n| n as usize);
        self.uploads.push(BatchUpload {
            x: x - 1,
            y: y - 1,
            width: width + 2,
            height: height + 2,
            pixels,
        });
        let slot = BatchSlot {
            key,
            position,
            last_frame: self.frame,
        };
        let index = if let Some(index) = replaced {
            self.key = Rc::new(());
            let old = std::mem::replace(&mut self.slots[index], slot);
            self.tiles.remove(&old.key);
            index
        } else {
            let index = self.slots.len();
            self.slots.push(slot);
            index
        };
        self.tiles.insert(key, index);
        self.used = true;
    }

    fn reserve(&mut self, size: [u32; 2]) -> Option<([usize; 2], Option<usize>)> {
        let [width, height] = size.map(|n| n as usize);
        if let Some((x, y)) = self.packing.add_rect(width + 2, height + 2) {
            return Some(([x + 1, y + 1], None));
        }
        if self.pinned {
            return None;
        }

        // Build the list only when fresh space runs out. Prefer slots that
        // were already absent last frame over slots that could still be visible.
        // Within each age group, the original packing order keeps neighbors
        // together for uploads when the tile sizes are uniform.
        let retired = self.retired.get_or_insert_with(|| {
            let mut by_size = HashMap::<_, RetiredSlots>::new();
            for (index, tile) in self.slots.iter().enumerate().rev() {
                if tile.last_frame == self.frame {
                    continue;
                }
                let slots = by_size.entry(tile.key.size()).or_default();
                if tile.last_frame == self.frame.wrapping_sub(1) {
                    slots.previous_frame.push(index);
                } else {
                    slots.stale.push(index);
                }
            }
            by_size
        });
        let slots = retired.get_mut(&size)?;
        while let Some(index) = slots.stale.pop().or_else(|| slots.previous_frame.pop()) {
            // A quad can claim a tile after the list was built. Keep it intact
            // until the canvas has submitted all draws for this frame.
            let tile = &self.slots[index];
            if tile.last_frame != self.frame {
                return Some((tile.position, Some(index)));
            }
        }
        None
    }
}

#[derive(Default)]
pub(crate) struct Images {
    textures: HashMap<Key, Texture>,
    atlases: AtlasCache,
    video: HashMap<usize, VideoTexture>,
    batch_atlas: Option<BatchAtlas>,
    coverage_atlas: Option<CoverageAtlas>,
}

impl Images {
    pub(super) fn source_atlas_key(&self, image: ImageId) -> Option<Rc<()>> {
        let atlas = self
            .batch_atlas
            .as_ref()
            .filter(|atlas| atlas.image == image)?;
        Some(atlas.key.clone())
    }

    pub(super) fn retain_source_atlas(&mut self, key: &Rc<()>) -> Option<ImageId> {
        let atlas = self
            .batch_atlas
            .as_mut()
            .filter(|atlas| Rc::ptr_eq(&atlas.key, key))?;
        atlas.used = true;
        // The cached instance buffer refers to tiles the collector did not
        // look up on this frame. Keep them intact through overlay painting.
        atlas.pinned = true;
        Some(atlas.image)
    }

    pub(super) fn source_atlas(
        &self,
        image: ImageId,
    ) -> Option<(&wgpu::Texture, &OnceCell<wgpu::BindGroup>)> {
        let atlas = self
            .batch_atlas
            .as_ref()
            .filter(|atlas| atlas.image == image)?;
        Some((&atlas.texture, &atlas.compositor_binding))
    }

    pub(crate) fn draw_icon(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        image: &ImageAsset,
        bounds: Rect,
        scale: f32,
        enabled: bool,
        tint: Option<femtovg::Color>,
    ) -> Result<(), String> {
        canvas.save();
        if !enabled && tint.is_none() {
            canvas.set_global_alpha(0.45);
        }
        let result = self.draw(
            canvas,
            image,
            bounds,
            scale,
            ImagePresentation::whole(image.size(), ImageFit::Contain),
            tint.map_or(Colorize::Original, Colorize::Solid),
        );
        canvas.restore();
        result
    }

    pub(crate) fn begin_frame(&mut self) {
        if let Some(atlas) = &mut self.coverage_atlas {
            atlas.begin_frame();
        }
        if let Some(atlas) = &mut self.batch_atlas {
            atlas.frame = atlas.frame.wrapping_add(1);
            atlas.retired = None;
            atlas.used = false;
            atlas.pinned = false;
        }
        for texture in self.video.values_mut() {
            texture.used = false;
        }
        for texture in self.textures.values_mut() {
            texture.used = false;
            if let Some(gradient) = &mut texture.gradient {
                gradient.used = false;
            }
        }
    }

    pub(crate) fn end_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        if let Some(atlas) = self.coverage_atlas.take_if(CoverageAtlas::idle) {
            canvas.delete_image(atlas.image);
        }
        // A failed display-list recording can leave pending writes in the page.
        if let Some(atlas) = self
            .batch_atlas
            .take_if(|atlas| !atlas.used || !atlas.uploads.is_empty())
        {
            canvas.delete_image(atlas.image);
        }
        self.video.retain(|_, texture| {
            if !texture.used {
                canvas.delete_image(texture.id);
            }
            texture.used
        });
        self.textures.retain(|_, texture| {
            if let Some(gradient) = texture.gradient.take_if(|gradient| !gradient.used) {
                canvas.delete_image(gradient.id);
            }
            if !texture.used {
                canvas.delete_image(texture.id);
            }
            texture.used
        });
    }

    pub(crate) fn draw_video(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        key: usize,
        frame: &MediaFrame,
        bounds: Rect,
        presentation: ImagePresentation,
    ) -> Result<(), String> {
        let placement = presentation.placement(bounds);
        if placement.visible.width <= 0.0 || placement.visible.height <= 0.0 {
            return Ok(());
        }
        let [width, height] = frame.size();
        let pixels = ImageSource::Rgba(imgref::Img::new(
            frame.rgba().as_rgba(),
            width as usize,
            height as usize,
        ));
        if self
            .video
            .get(&key)
            .is_some_and(|texture| texture.frame.size() != frame.size())
            && let Some(texture) = self.video.remove(&key)
        {
            canvas.delete_image(texture.id);
        }
        let texture = match self.video.entry(key) {
            Entry::Occupied(entry) => {
                let texture = entry.into_mut();
                if &texture.frame != frame {
                    canvas
                        .update_image(texture.id, pixels, 0, 0)
                        .map_err(|error| error.to_string())?;
                    texture.frame = frame.clone();
                }
                texture
            }
            Entry::Vacant(entry) => entry.insert(VideoTexture {
                id: canvas
                    .create_image(pixels, ImageFlags::empty())
                    .map_err(|error| error.to_string())?,
                frame: frame.clone(),
                used: true,
            }),
        };
        texture.used = true;
        let paint = Colorize::Original.paint(texture.id, placement.pattern);
        fill(canvas, placement.visible, &paint);
        Ok(())
    }

    pub(crate) fn draw(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        image: &ImageAsset,
        bounds: Rect,
        scale: f32,
        presentation: ImagePresentation,
        colorize: Colorize<'_>,
    ) -> Result<(), String> {
        let image = image.frame(0);
        let Some(ImagePixels {
            identity,
            format,
            bytes,
            size: source,
            edges,
        }) = image.pixels()
        else {
            return Ok(());
        };
        if edges.is_some_and(|edges| !edges.fits(source)) {
            return Err(xrack_runtime::InvalidNineSlice.to_string());
        }
        let clip = presentation.region;
        if clip.width == 0 || clip.height == 0 || bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Ok(());
        }
        let placement = presentation.placement(bounds);
        if edges.is_none() && (placement.visible.width <= 0.0 || placement.visible.height <= 0.0) {
            return Ok(());
        }
        let region = (clip != ImageRegion::full(source)).then_some([
            clip.x,
            clip.y,
            clip.width,
            clip.height,
        ]);
        let nearest = presentation.rendering == ImageRendering::Pixelated;
        let raster_size = match format {
            ImageFormat::Png => source,
            ImageFormat::Svg => {
                // Nine-slice SVGs keep their source density across resizes. The
                // rest rasterize just the selected region at the pattern scale.
                let (size, scale) = if nearest {
                    (clip.logical_size(), 1.0)
                } else if edges.is_some() {
                    (clip.logical_size(), scale.max(1.0))
                } else {
                    (
                        Size::new(placement.pattern.width, placement.pattern.height),
                        scale,
                    )
                };
                let raster_scale =
                    scale.min(MAX_IMAGE_DIMENSION as f32 / size.width.max(size.height));
                [size.width, size.height].map(|n| {
                    (n * raster_scale)
                        .ceil()
                        .clamp(1.0, MAX_IMAGE_DIMENSION as f32) as u32
                })
            }
        };
        let is_png = format == ImageFormat::Png;
        let key = Key {
            raster: RasterKey {
                identity,
                len: bytes.len(),
                format,
                size: raster_size,
                svg_region: if is_png { None } else { region },
            },
            region: if is_png { region } else { None },
            repeat: placement.repeat,
            nearest,
            alpha_mask: colorize.uses_alpha(),
        };
        let mut scratch = None;
        if let Some(edges) = edges {
            let edges = clip.clipped_edges(source, edges);
            let grid_size = if is_png { clip.size() } else { raster_size };
            for patch in
                nine_slice::patches(bounds, clip.size(), grid_size, edges, presentation.modes)
            {
                let [x, y, width, height] = patch.source;
                let region = if is_png {
                    [clip.x + x, clip.y + y, width, height]
                } else {
                    patch.source
                };
                let key = Key {
                    region: Some(region),
                    repeat: patch.repeat,
                    ..key
                };
                let texture = self.texture(canvas, key, bytes, &mut scratch, source, colorize)?;
                let mut paint = colorize.paint(
                    texture,
                    Rect::new(
                        patch.destination.x,
                        patch.destination.y,
                        patch.tile.width,
                        patch.tile.height,
                    ),
                );
                // Independent anti-aliased edges would blend at shared cuts.
                paint.set_anti_alias(false);
                fill(canvas, patch.destination, &paint);
            }
        } else {
            let texture = self.texture(canvas, key, bytes, &mut scratch, source, colorize)?;
            let paint = colorize.paint(texture, placement.pattern);
            fill(canvas, placement.visible, &paint);
        }
        Ok(())
    }

    /// Prepare a straight-RGBA image for a quad batch. Small linear-sampled
    /// PNGs can share a texture even when they come from different sources.
    pub(super) fn batch_quad(
        &mut self,
        device: &wgpu::Device,
        canvas: &mut Canvas<WGPURenderer>,
        image: ImageQuad<'_>,
    ) -> Result<Option<(ImageId, Quad)>, String> {
        let ImageQuad {
            asset,
            bounds,
            clip,
            coverage,
            presentation,
            transform,
        } = image;
        let image = asset.frame(0);
        let Some(ImagePixels {
            identity,
            format: ImageFormat::Png,
            bytes,
            size,
            edges: None,
        }) = image.pixels()
        else {
            return Ok(None);
        };
        let placement = presentation.placement(bounds);
        let pattern = placement.pattern;
        if presentation.region != ImageRegion::full(size)
            || placement.repeat != [false; 2]
            || pattern.width <= 0.0
            || pattern.height <= 0.0
        {
            return Ok(None);
        }
        let key = Key {
            raster: RasterKey {
                identity,
                len: bytes.len(),
                format: ImageFormat::Png,
                size,
                svg_region: None,
            },
            region: None,
            repeat: [false; 2],
            nearest: presentation.rendering == ImageRendering::Pixelated,
            alpha_mask: false,
        };
        let mut decoded = None;
        let rect = match coverage {
            QuadBounds::Aligned(clip) => {
                let visible = placement.visible;
                let rect = clip.map_or(visible, |clip| visible.intersect(clip));
                if !quad_edges_are_pixel_aligned(rect, transform) {
                    return Ok(None);
                }
                rect
            }
            QuadBounds::Snapped(rect) => rect,
            QuadBounds::Antialiased(leaf_clip, dpi) => {
                let Some(coverage) =
                    Coverage::new(transform, placement.visible, leaf_clip, pattern, clip, dpi)
                else {
                    return Ok(None);
                };
                let Some((_, source)) =
                    self.batch_atlas_tile(device, canvas, key, bytes, &mut decoded)?
                else {
                    return Ok(None);
                };
                if self.coverage_atlas.is_none() {
                    self.coverage_atlas = CoverageAtlas::new(device, canvas);
                }
                return Ok(self.coverage_atlas.as_mut().and_then(|atlas| {
                    atlas
                        .push(canvas, coverage, source, size, BATCH_ATLAS_SIZE)
                        .map(|quad| (atlas.image, quad))
                }));
            }
        };
        // Keep sampling in the uncut pattern after adjusting the quad's
        // coverage for clipping and antialiasing.
        if rect != rect.intersect(clip) || rect.width <= 0.0 || rect.height <= 0.0 {
            return Ok(None);
        }

        let mut quad = Quad {
            x0: rect.x,
            y0: rect.y,
            x1: rect.x + rect.width,
            y1: rect.y + rect.height,
            s0: (rect.x - pattern.x) / pattern.width,
            t0: (rect.y - pattern.y) / pattern.height,
            s1: (rect.x + rect.width - pattern.x) / pattern.width,
            t1: (rect.y + rect.height - pattern.y) / pattern.height,
        };
        if let Some((image, [x, y])) =
            self.batch_atlas_tile(device, canvas, key, bytes, &mut decoded)?
        {
            let horizontal = |u: f32| (x as f32 + u * size[0] as f32) / BATCH_ATLAS_SIZE as f32;
            let vertical = |v: f32| (y as f32 + v * size[1] as f32) / BATCH_ATLAS_SIZE as f32;
            quad.s0 = horizontal(quad.s0);
            quad.s1 = horizontal(quad.s1);
            quad.t0 = vertical(quad.t0);
            quad.t1 = vertical(quad.t1);
            return Ok(Some((image, quad)));
        }
        let texture = self.texture(canvas, key, bytes, &mut decoded, size, Colorize::Original)?;
        Ok(Some((texture, quad)))
    }

    fn batch_atlas_tile(
        &mut self,
        device: &wgpu::Device,
        canvas: &mut Canvas<WGPURenderer>,
        key: Key,
        bytes: &[u8],
        decoded: &mut Option<Raster>,
    ) -> Result<Option<(ImageId, [usize; 2])>, String> {
        let size = key.raster.size;
        if key.nearest
            || !size
                .into_iter()
                .all(|n| (1..=BATCH_TILE_LIMIT).contains(&n))
        {
            return Ok(None);
        }
        let key = BatchTile::Raster(key.raster);
        let atlas = self.batch_atlas(device, canvas)?;
        if let Some(tile) = atlas.lookup(key) {
            return Ok(Some((atlas.image, tile)));
        }
        let Some((tile, replaced)) = atlas.reserve(size) else {
            return Ok(None);
        };
        let raster = decoded.insert(decode_png(bytes)?);
        if raster.size != size {
            return Ok(None);
        }
        let [width, height] = size.map(|n| n as usize);
        // A one-texel gutter reproduces the edge clamp of the original texture.
        let pixels = raster.rgba.as_rgba();
        let mut padded = Vec::with_capacity((width + 2) * (height + 2));
        for row in 0..height + 2 {
            let from = row.saturating_sub(1).min(height - 1) * width;
            padded.push(pixels[from]);
            padded.extend_from_slice(&pixels[from..from + width]);
            padded.push(pixels[from + width - 1]);
        }
        atlas.store(key, tile, replaced, padded);
        Ok(Some((atlas.image, tile)))
    }

    pub(super) fn batch_solid_quad(
        &mut self,
        device: &wgpu::Device,
        canvas: &mut Canvas<WGPURenderer>,
        bounds: Rect,
        tint: UiColor,
    ) -> Result<Option<(ImageId, Quad)>, String> {
        let atlas = self.batch_atlas(device, canvas)?;
        let key = BatchTile::Solid(tint);
        let [x, y] = if let Some(tile) = atlas.lookup(key) {
            tile
        } else {
            let Some((tile, replaced)) = atlas.reserve(key.size()) else {
                return Ok(None);
            };
            let [r, g, b, a] = tint.0.to_be_bytes();
            atlas.store(key, tile, replaced, vec![RGBA8::new(r, g, b, a); 9]);
            tile
        };
        // Sample the same texel at all four corners, with the usual atlas
        // gutters isolating its color from the neighbouring PNGs.
        let s = (x as f32 + 0.5) / BATCH_ATLAS_SIZE as f32;
        let t = (y as f32 + 0.5) / BATCH_ATLAS_SIZE as f32;
        Ok(Some((
            atlas.image,
            Quad {
                x0: bounds.x,
                y0: bounds.y,
                x1: bounds.x + bounds.width,
                y1: bounds.y + bounds.height,
                s0: s,
                t0: t,
                s1: s,
                t1: t,
            },
        )))
    }

    fn batch_atlas(
        &mut self,
        device: &wgpu::Device,
        canvas: &mut Canvas<WGPURenderer>,
    ) -> Result<&mut BatchAtlas, String> {
        Ok(match &mut self.batch_atlas {
            Some(atlas) => atlas,
            slot => {
                // The coverage pass runs before the canvas command buffer.
                // A native texture's updates are queued before both passes.
                let texture = device.create_texture(&wgpu::TextureDescriptor {
                    label: Some("XRack source image atlas"),
                    size: wgpu::Extent3d {
                        width: BATCH_ATLAS_SIZE as u32,
                        height: BATCH_ATLAS_SIZE as u32,
                        depth_or_array_layers: 1,
                    },
                    mip_level_count: 1,
                    sample_count: 1,
                    dimension: wgpu::TextureDimension::D2,
                    format: wgpu::TextureFormat::Rgba8Unorm,
                    usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
                    view_formats: &[],
                });
                let image = canvas
                    .create_image_from_native_texture(
                        texture.clone(),
                        ImageInfo::new(
                            ImageFlags::empty(),
                            BATCH_ATLAS_SIZE,
                            BATCH_ATLAS_SIZE,
                            PixelFormat::Rgba8,
                        ),
                    )
                    .map_err(|error| format!("image atlas allocation failed: {error}"))?;
                slot.insert(BatchAtlas {
                    image,
                    texture,
                    compositor_binding: OnceCell::new(),
                    packing: PackingAtlas::new(BATCH_ATLAS_SIZE, BATCH_ATLAS_SIZE),
                    tiles: HashMap::new(),
                    slots: Vec::new(),
                    retired: None,
                    uploads: Vec::new(),
                    frame: 0,
                    used: false,
                    key: Rc::new(()),
                    pinned: false,
                })
            }
        })
    }

    /// Upload adjacent tiles as one strip before submitting their first draw.
    pub(super) fn flush_batch_uploads(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
    ) -> Result<(), String> {
        let Some(atlas) = &mut self.batch_atlas else {
            return Ok(());
        };
        let mut remaining = atlas.uploads.as_slice();
        let mut packed = Vec::new();
        while let Some((first, tail)) = remaining.split_first() {
            let mut count = 1;
            let mut width = first.width;
            for next in tail {
                if next.y != first.y || next.height != first.height || next.x != first.x + width {
                    break;
                }
                width += next.width;
                count += 1;
            }
            let (run, rest) = remaining.split_at(count);
            let pixels = if count == 1 {
                first.pixels.as_slice()
            } else {
                packed.clear();
                packed.reserve(width * first.height);
                for row in 0..first.height {
                    for tile in run {
                        let start = row * tile.width;
                        packed.extend_from_slice(&tile.pixels[start..start + tile.width]);
                    }
                }
                packed.as_slice()
            };
            canvas
                .update_image(
                    atlas.image,
                    ImageSource::Rgba(imgref::Img::new(pixels, width, first.height)),
                    first.x,
                    first.y,
                )
                .map_err(|error| format!("image atlas upload failed: {error}"))?;
            xrack_runtime::update_profile::texture_upload(pixels.len() * 4);
            remaining = rest;
        }
        atlas.uploads.clear();
        Ok(())
    }

    pub(super) fn encode_coverage(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
    ) -> Option<wgpu::CommandBuffer> {
        let source = self.batch_atlas.as_ref()?;
        self.coverage_atlas
            .as_mut()?
            .encode(device, queue, &source.texture, &source.key)
    }

    fn texture(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        key: Key,
        bytes: &[u8],
        scratch: &mut Option<Raster>,
        source: [u32; 2],
        colorize: Colorize<'_>,
    ) -> Result<ImageId, String> {
        let texture = match self.textures.entry(key) {
            Entry::Occupied(entry) => entry.into_mut(),
            Entry::Vacant(entry) => {
                let RasterKey {
                    format,
                    size,
                    svg_region,
                    ..
                } = key.raster;
                let raster = if format == ImageFormat::Png && key.region.is_some() {
                    self.atlases.raster(key.raster, bytes, scratch)?
                } else {
                    match scratch {
                        Some(raster) => raster,
                        empty => empty.insert(match format {
                            ImageFormat::Png => decode_png(bytes)?,
                            ImageFormat::Svg => rasterize_svg_region(bytes, size, svg_region)?,
                        }),
                    }
                };
                let [width, height] = raster.size;
                let mut pixels =
                    imgref::Img::new(raster.rgba.as_rgba(), width as usize, height as usize);
                if let Some([x, y, w, h]) = key.region {
                    if x.saturating_add(w) > width || y.saturating_add(h) > height {
                        return Err("image clip is outside the decoded raster".into());
                    }
                    pixels = pixels.sub_image(x as usize, y as usize, w as usize, h as usize);
                }
                let size = [pixels.width() as u32, pixels.height() as u32];
                let mut flags = key.sampling();
                let alpha;
                let packed;
                let image = if key.alpha_mask {
                    // Gray8 samples as (a, a, a, a) in FemtoVG's image shader.
                    alpha = pixels
                        .pixels()
                        .map(|pixel| Gray(pixel.a))
                        .collect::<Vec<_>>();
                    ImageSource::Gray(imgref::Img::new(
                        alpha.as_slice(),
                        pixels.width(),
                        pixels.height(),
                    ))
                } else {
                    flags.set(ImageFlags::PREMULTIPLIED, format == ImageFormat::Svg);
                    if pixels.stride() != pixels.width() {
                        // WGPU's upload reads tightly packed rows.
                        packed = pixels.pixels().collect::<Vec<_>>();
                        pixels =
                            imgref::Img::new(packed.as_slice(), pixels.width(), pixels.height());
                    }
                    ImageSource::Rgba(pixels)
                };
                let channels = if key.alpha_mask { 1 } else { 4 };
                xrack_runtime::update_profile::texture_upload(
                    size[0] as usize * size[1] as usize * channels,
                );
                let id = canvas
                    .create_image(image, flags)
                    .map_err(|error| format!("image texture upload failed: {error}"))?;
                entry.insert(Texture {
                    id,
                    size,
                    used: true,
                    gradient: None,
                })
            }
        };
        texture.used = true;
        if let Colorize::Gradient(brush) = colorize {
            let gradient = match &mut texture.gradient {
                Some(gradient) => gradient,
                slot => slot.insert(GradientTexture::new(canvas, texture.size, key.sampling())?),
            };
            return Ok(gradient.paint(
                canvas,
                texture.id,
                texture.size,
                key.colorize_geometry(source),
                brush,
            ));
        }
        Ok(texture.id)
    }
}

fn fill(canvas: &mut Canvas<WGPURenderer>, bounds: Rect, paint: &Paint) {
    let mut path = Path::new();
    path.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    canvas.fill_path(&path, paint);
}

pub(super) fn draw_live_texture(
    canvas: &mut Canvas<WGPURenderer>,
    texture: wgpu::Texture,
    bounds: Rect,
    presentation: ImagePresentation,
    colorize: Colorize<'_>,
) -> Result<(), String> {
    let size = [texture.width(), texture.height()];
    let region = presentation.region;
    if region.width == 0 || region.height == 0 {
        return Ok(());
    }
    let placement = presentation.placement(bounds);
    if placement.visible.width <= 0.0 || placement.visible.height <= 0.0 {
        return Ok(());
    }
    let cropped = region != ImageRegion::full(size);
    // Crop edges must clamp to the selected texels. Colorize also needs a
    // separate GPU target for the source alpha mask.
    let render_region = colorize.uses_alpha() || cropped;
    let mut flags = ImageFlags::PREMULTIPLIED;
    if presentation.rendering == ImageRendering::Pixelated {
        flags |= ImageFlags::NEAREST;
    }
    let mut repeat = ImageFlags::empty();
    repeat.set(ImageFlags::REPEAT_X, placement.repeat[0]);
    repeat.set(ImageFlags::REPEAT_Y, placement.repeat[1]);
    let source = canvas
        .create_image_from_native_texture(
            texture,
            ImageInfo::new(
                if render_region { flags } else { flags | repeat },
                size[0] as usize,
                size[1] as usize,
                PixelFormat::Rgba8,
            ),
        )
        .map_err(|error| error.to_string())?;
    let result = if render_region {
        raster_live_region(canvas, source, size, region, flags | repeat, colorize).map(|image| {
            let paint = Colorize::Original.paint(image, placement.pattern);
            fill(canvas, placement.visible, &paint);
            canvas.delete_image(image);
        })
    } else {
        let paint = Colorize::Original.paint(source, placement.pattern);
        fill(canvas, placement.visible, &paint);
        Ok(())
    };
    canvas.delete_image(source);
    result
}

fn raster_live_region(
    canvas: &mut Canvas<WGPURenderer>,
    source: ImageId,
    size: [u32; 2],
    region: ImageRegion,
    flags: ImageFlags,
    colorize: Colorize<'_>,
) -> Result<ImageId, String> {
    let target = canvas
        .create_image_empty(
            region.width as usize,
            region.height as usize,
            PixelFormat::Rgba8,
            flags | ImageFlags::FLIP_Y,
        )
        .map_err(|error| error.to_string())?;
    let source_bounds = Rect::new(
        -(region.x as f32),
        -(region.y as f32),
        size[0] as f32,
        size[1] as f32,
    );
    let bounds = Rect::new(0.0, 0.0, region.width as f32, region.height as f32);
    let mut copy = Colorize::Original.paint(source, source_bounds);
    copy.set_anti_alias(false);
    let tint = match colorize {
        Colorize::Original => None,
        Colorize::Solid(color) => Some(Paint::color(color)),
        Colorize::Gradient(brush) => Some(crate::paint_brush(brush, source_bounds, 0.0)),
    };
    canvas.save();
    canvas.reset_transform();
    canvas.reset_scissor();
    canvas.set_global_alpha(1.0);
    canvas.with_render_target(RenderTarget::Image(target), |canvas| {
        canvas.clear_rect(0, 0, region.width, region.height, Color::rgba(0, 0, 0, 0));
        canvas.global_composite_operation(CompositeOperation::SourceOver);
        fill(canvas, bounds, &copy);
        if let Some(mut tint) = tint {
            tint.set_anti_alias(false);
            canvas.global_composite_operation(CompositeOperation::SourceIn);
            fill(canvas, bounds, &tint);
        }
    });
    canvas.restore();
    Ok(target)
}
