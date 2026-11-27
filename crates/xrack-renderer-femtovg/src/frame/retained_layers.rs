use crate::{
    Canvas, Color, LayoutNode, Node, Paint, PaintFrame, Palette, Path, Point, Rect, Renderer,
    WGPURenderer, append_rect, canvas_transform, has_device_scale, image, shadow, vector_masks,
};
use femtovg::{Atlas, FillRule, ImageFlags, ImageId, ImageInfo, PixelFormat, RenderTarget};
use std::{
    cell::{Cell, OnceCell},
    collections::{BTreeMap, HashMap, HashSet},
    rc::{Rc, Weak},
};
use xrack_layout::RoundedClip;
use xrack_layout::item::{ContainerPaint, ItemPaint, paint as item_paint};
use xrack_runtime::{SceneIndex, SceneNodeId};

const MAX_LAYERS: usize = 64;
const MAX_SHADOW_MASKS: usize = 24;
const MAX_NEW_SHADOW_MASKS: usize = 4;
const SHADOW_MASK_LIFETIME: u64 = 60;
const MAX_OPACITY_ATLAS_EDGE: u32 = 1536;
const OPACITY_ATLAS_LIFETIME: u64 = 60;
const MAX_SHARED_OPACITY_TILES: usize = 32;
const MIN_WINDOW_ATLAS_EDGE: u32 = 512;
const MAX_WINDOW_ATLAS_EDGE: u32 = 1024;

#[derive(Clone, Copy, Debug, Default)]
pub struct LayerStats {
    pub rasterized: u64,
    pub reused: u64,
    pub direct: u64,
}

struct Layer {
    page: Weak<(Node, LayoutNode)>,
    raster: Raster,
    bounds: Rect,
    scale: f32,
    palette: Palette,
}

struct OpacityLayer {
    node: Box<Node>,
    captured_layout: Box<LayoutNode>,
    capture_clip: Rect,
    full_coverage: bool,
    image: Rc<OpacityImage>,
    raster_bounds: Rect,
    resample_origin: Option<Point>,
    sampled_clips: Option<Box<[Rect]>>,
    scale: f32,
    palette: Palette,
    in_layout: bool,
    surface_only: bool,
    used: bool,
    last_frame: u64,
    recaptured_at_new_phase: bool,
}

struct OpacityImage {
    image: ImageId,
    bytes: u64,
    cached: Option<Rc<CachedLayerImage>>,
    fonts: Option<[usize; 2]>,
}

pub(crate) struct CachedLayerImage {
    pub(crate) texture: wgpu::Texture,
    pub(crate) binding: OnceCell<wgpu::BindGroup>,
}

struct WindowOpacityAtlas {
    image: Rc<CachedLayerImage>,
    packing: Atlas,
    tiles: HashMap<*const CachedLayerImage, WindowAtlasTile>,
    retained: Vec<RetainedOpacityTile>,
    last_frame: u64,
    grow_next_frame: bool,
    canvas_image: Option<ImageId>,
}

struct RetainedOpacityTile {
    capture: OpacityTile,
    slot: [u32; 2],
    last_frame: u64,
    can_crop: bool,
}

struct WindowAtlasTile {
    source: Weak<CachedLayerImage>,
    origin: [u32; 2],
    slot: [u32; 2],
    last_frame: u64,
}

impl WindowOpacityAtlas {
    fn image(device: &wgpu::Device, edge: u32) -> Rc<CachedLayerImage> {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack compositor opacity atlas"),
            size: wgpu::Extent3d {
                width: edge,
                height: edge,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        Rc::new(CachedLayerImage {
            texture,
            binding: OnceCell::new(),
        })
    }

    fn new(device: &wgpu::Device, edge: u32, frame: u64) -> Self {
        Self {
            image: Self::image(device, edge),
            packing: Atlas::new(edge as usize, edge as usize),
            tiles: HashMap::new(),
            retained: Vec::new(),
            last_frame: frame,
            grow_next_frame: false,
            canvas_image: None,
        }
    }

    fn bytes(edge: u32) -> u64 {
        u64::from(edge) * u64::from(edge) * 4
    }

    fn take_dead_slot(
        &mut self,
        width: u32,
        height: u32,
        frame: u64,
    ) -> Option<([u32; 2], [u32; 2])> {
        let key = self
            .tiles
            .iter()
            .filter(|(_, tile)| {
                tile.last_frame != frame
                    && tile.source.strong_count() == 0
                    && width <= tile.slot[0]
                    && height <= tile.slot[1]
            })
            .min_by_key(|(_, tile)| u64::from(tile.slot[0]) * u64::from(tile.slot[1]))
            .map(|(key, _)| *key)?;
        self.tiles.remove(&key).map(|tile| (tile.origin, tile.slot))
    }

    fn tile(
        &mut self,
        source: &Rc<CachedLayerImage>,
        device: &wgpu::Device,
        encoder: &mut Option<wgpu::CommandEncoder>,
        frame: u64,
    ) -> Option<[u32; 2]> {
        let key = Rc::as_ptr(source);
        if let Some(tile) = self.tiles.get_mut(&key) {
            tile.last_frame = frame;
            return Some(tile.origin);
        }
        let width = source.texture.width();
        let height = source.texture.height();
        let allocation = (self.tiles.len() < MAX_LAYERS)
            .then(|| {
                self.packing
                    .add_rect((width + 2) as usize, (height + 2) as usize)
            })
            .flatten()
            .map(|(x, y)| ([x as u32 + 1, y as u32 + 1], [width, height]));
        let (origin, slot) = allocation
            .or_else(|| self.take_dead_slot(width, height, frame))
            .or_else(|| {
                self.grow_next_frame |= !self.retained.is_empty()
                    && self.tiles.len() < MAX_LAYERS
                    && self.image.texture.width() < MAX_OPACITY_ATLAS_EDGE;
                None
            })?;
        let encoder = encoder.get_or_insert_with(|| {
            device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
                label: Some("XRack compositor atlas copies"),
            })
        });
        // Replicate the edge texels into the gutter so bilinear sampling
        // matches the source texture's clamp-to-edge sampler.
        let spans = |start, extent| {
            [
                (0, start - 1, 1),
                (0, start, extent),
                (extent - 1, start + extent, 1),
            ]
        };
        for (sy, dy, height) in spans(origin[1], height) {
            for (sx, dx, width) in spans(origin[0], width) {
                encoder.copy_texture_to_texture(
                    wgpu::TexelCopyTextureInfo {
                        texture: &source.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: sx, y: sy, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::TexelCopyTextureInfo {
                        texture: &self.image.texture,
                        mip_level: 0,
                        origin: wgpu::Origin3d { x: dx, y: dy, z: 0 },
                        aspect: wgpu::TextureAspect::All,
                    },
                    wgpu::Extent3d {
                        width,
                        height,
                        depth_or_array_layers: 1,
                    },
                );
            }
        }
        self.tiles.insert(
            key,
            WindowAtlasTile {
                source: Rc::downgrade(source),
                origin,
                slot,
                last_frame: frame,
            },
        );
        Some(origin)
    }
}

pub(crate) enum WindowLayerSource {
    Texture(Rc<CachedLayerImage>),
    TintedAlpha {
        image: Rc<CachedLayerImage>,
        tint: u32,
    },
    Solid(Color),
    CoveredSolid {
        tint: u32,
        edges: [f32; 4],
    },
}

pub(crate) struct WindowLayer {
    owner: Option<LayerOwner>,
    pub(crate) source: WindowLayerSource,
    pub(crate) bounds: Rect,
    pub(crate) uv: [f32; 4],
    pub(crate) opacity: f32,
    pub(crate) surface_only: bool,
    pub(crate) mask: Option<LayerClip>,
    pub(crate) clip_samples: [bool; 2],
    pub(crate) edge_clip: Option<Rect>,
}

#[derive(Clone)]
pub(crate) struct LayerClip {
    pub(crate) mask: Rc<WindowClipMask>,
    pub(crate) origin: [i32; 2],
    pub(crate) second: Option<LayerClipSample>,
}

#[derive(Clone)]
pub(crate) struct LayerClipSample {
    pub(crate) mask: Rc<WindowClipMask>,
    pub(crate) origin: [i32; 2],
}

pub(crate) struct WindowClipMask {
    pub(crate) texture: wgpu::Texture,
    pub(crate) binding: OnceCell<wgpu::BindGroup>,
    last_frame: Cell<u64>,
}

struct RetainedClipMask {
    shapes: Rc<[RoundedClip]>,
    raster_only: bool,
    scale: f32,
    physical: Rect,
    mask: Rc<WindowClipMask>,
    bytes: u64,
}

impl RetainedClipMask {
    fn matches(
        &self,
        shapes: &[RoundedClip],
        physical: Rect,
        scale: f32,
        raster_only: bool,
    ) -> bool {
        if self.scale != scale
            || self.raster_only != raster_only
            || self.physical.width != physical.width
            || self.physical.height != physical.height
            || self.shapes.len() != shapes.len()
        {
            return false;
        }
        let dx = physical.x - self.physical.x;
        let dy = physical.y - self.physical.y;
        // Device-pixel translations preserve the mask. Allow for rounding
        // when the layout coordinates were formed in logical pixels.
        let tolerance = 1.0 / 1024.0;
        self.shapes.iter().zip(shapes).all(|(before, after)| {
            before.radii == after.radii
                && before.bounds.width == after.bounds.width
                && before.bounds.height == after.bounds.height
                && ((after.bounds.x - before.bounds.x) * scale - dx).abs() <= tolerance
                && ((after.bounds.y - before.bounds.y) * scale - dy).abs() <= tolerance
        })
    }
}

impl WindowLayer {
    pub(crate) fn tinted_alpha(
        quad: femtovg::Quad,
        image: Rc<CachedLayerImage>,
        color: Color,
    ) -> Self {
        let [r, g, b] = [color.r, color.g, color.b].map(|channel| (channel * 255.0).round() as u32);
        Self {
            owner: None,
            source: WindowLayerSource::TintedAlpha {
                image,
                tint: (r << 16) | (g << 8) | b,
            },
            bounds: Rect::new(quad.x0, quad.y0, quad.x1 - quad.x0, quad.y1 - quad.y0),
            uv: [quad.s0, quad.t0, quad.s1, quad.t1],
            opacity: 1.0,
            surface_only: true,
            mask: None,
            clip_samples: [false; 2],
            edge_clip: None,
        }
    }

    pub(crate) fn solid(bounds: Rect, color: Color) -> Self {
        Self {
            owner: None,
            source: WindowLayerSource::Solid(color),
            bounds,
            uv: [0.0; 4],
            opacity: 1.0,
            surface_only: true,
            mask: None,
            clip_samples: [false; 2],
            edge_clip: None,
        }
    }

    pub(crate) fn covered_solid(bounds: Rect, color: Color, scale: f32) -> Self {
        let [r, g, b] = [color.r, color.g, color.b].map(|value| (value * 255.0).round() as u32);
        let edges = [
            bounds.x,
            bounds.y,
            bounds.x + bounds.width,
            bounds.y + bounds.height,
        ]
        .map(|edge| edge * scale);
        Self {
            source: WindowLayerSource::CoveredSolid {
                tint: (r << 16) | (g << 8) | b,
                edges,
            },
            ..Self::solid(logical_bounds(device_bounds(bounds, scale), scale), color)
        }
    }

    pub(crate) fn crop(&mut self, clip: Rect) {
        let source = self.bounds;
        let visible = source.intersect(clip);
        let relative = [
            (visible.x - source.x) / source.width,
            (visible.y - source.y) / source.height,
            (visible.x + visible.width - source.x) / source.width,
            (visible.y + visible.height - source.y) / source.height,
        ]
        .map(|value| value.clamp(0.0, 1.0));
        let [u, v, right, bottom] = self.uv;
        self.uv = [
            u + relative[0] * (right - u),
            v + relative[1] * (bottom - v),
            u + relative[2] * (right - u),
            v + relative[3] * (bottom - v),
        ];
        self.bounds = visible;
    }
}

impl OpacityLayer {
    fn raster(&self) -> Raster {
        Raster {
            image: self.image.image,
            bytes: self.image.bytes,
            bounds: self.raster_bounds,
        }
    }

    fn release_image(&self, canvas: &mut Canvas<WGPURenderer>) {
        if Rc::strong_count(&self.image) == 1 {
            canvas.delete_image(self.image.image);
        }
    }

    fn matches_paint(&self, node: &Node, style: OpacityTileStyle) -> bool {
        !self.surface_only
            && self.scale == style.scale
            && self.palette == style.palette
            && self.in_layout == style.in_layout
            && self.node.same_paint_except_layer_composition(node)
    }

    fn shares_capture(
        &self,
        node: &Node,
        layout: &LayoutNode,
        style: OpacityTileStyle,
        bounds: Rect,
        origin: Option<Point>,
    ) -> bool {
        // A sampled capture stores its original offset from the layout
        // origin. An exact capture must use the same device-pixel phase.
        if self.resample_origin != origin
            || self.raster_bounds.width != bounds.width
            || self.raster_bounds.height != bounds.height
            || !self.matches_paint(node, style)
        {
            return false;
        }
        let offset = Point {
            x: layout.bounds.x - self.captured_layout.bounds.x,
            y: layout.bounds.y - self.captured_layout.bounds.y,
        };
        let sampled = origin.is_some();
        if !sampled
            && (!self.full_coverage
                || self.capture_clip != self.captured_layout.bounds
                || (offset.x * style.scale).fract() != 0.0
                || (offset.y * style.scale).fract() != 0.0)
        {
            return false;
        }
        let mut motion = LayoutMove::new(self.capture_clip, layout.bounds, offset, 0.0);
        self.captured_layout.matches_translated(
            layout,
            sampled || layout.has_ancestor_clip_chain(),
            |before, after| translated_geometry_matches(before, after, &mut motion),
        )
    }

    fn reuse_at(
        &mut self,
        layout: &LayoutNode,
        clip: Rect,
        full_coverage: bool,
        exact_outer_clip: bool,
    ) -> bool {
        let previous = self.captured_layout.bounds;
        let next = layout.bounds;
        // Exact capture reuse also covers clipped surfaces which cannot move.
        if previous == next && self.capture_clip == clip && *self.captured_layout == *layout {
            let bounds = if self.resample_origin.is_some() {
                clip.inset(-1.0 / self.scale)
            } else {
                clip
            };
            self.raster_bounds = logical_bounds(device_bounds(bounds, self.scale), self.scale);
            return true;
        }
        // A cropped raster cannot reveal content that was outside its old clip.
        // A fully covered subtree raster leaves the ancestor shape on the
        // destination canvas. Exact translation still checks the clip chain of
        // every descendant, including clips introduced inside the subtree.
        if !self.full_coverage
            || !full_coverage
                && (self.resample_origin.is_none() || !can_crop_sampled_layer(layout, self.scale))
            || self.resample_origin.is_none()
                && !exact_outer_clip
                && (layout.has_ancestor_clip_chain()
                    || self.captured_layout.has_ancestor_clip_chain())
        {
            return false;
        }
        let offset = Point {
            x: next.x - previous.x,
            y: next.y - previous.y,
        };
        let (raster_bounds, tolerance) = if let Some(origin) = self.resample_origin {
            let mut raster = self.raster_bounds;
            raster.x = next.x + origin.x;
            raster.y = next.y + origin.y;
            // Addition at two absolute positions can round local coordinates
            // differently. Allow only subpixel floating-point noise here.
            (raster, 1.0 / (1024.0 * self.scale))
        } else {
            // Whole device pixels keep the capture's text and edge phase.
            let dx = offset.x * self.scale;
            let dy = offset.y * self.scale;
            let before = device_bounds(self.capture_clip, self.scale);
            let after = device_bounds(clip, self.scale);
            if dx.fract() != 0.0
                || dy.fract() != 0.0
                || Rect::new(before.x + dx, before.y + dy, before.width, before.height) != after
            {
                return false;
            }
            (logical_bounds(after, self.scale), 0.0)
        };
        let mut motion = LayoutMove::new(previous, next, offset, tolerance);
        if (!full_coverage || self.capture_clip != previous)
            && let Some(clips) = &self.sampled_clips
        {
            motion.sampled_clips = Some(clips.iter());
            motion.viewport = layout.clip;
        }
        if !self.captured_layout.matches_translated(
            layout,
            self.resample_origin.is_some() || exact_outer_clip,
            |before, after| translated_geometry_matches(before, after, &mut motion),
        ) {
            return false;
        }
        self.raster_bounds = raster_bounds;
        true
    }
}

fn can_crop_sampled_layer(layout: &LayoutNode, scale: f32) -> bool {
    scale >= 1.0 && crate::pixel_aligned(layout.clip, scale)
}

fn sampled_clip_regions(layout: &LayoutNode) -> Box<[Rect]> {
    fn visit(layout: &LayoutNode, bounds: Rect, clips: &mut Vec<Rect>) {
        let mut clip = layout.clip.intersect(bounds);
        clip.x -= bounds.x;
        clip.y -= bounds.y;
        clips.push(clip);
        for child in &layout.children {
            visit(child, bounds, clips);
        }
    }
    let mut clips = Vec::new();
    visit(layout, layout.bounds, &mut clips);
    clips.into_boxed_slice()
}

pub(crate) fn device_bounds(bounds: Rect, scale: f32) -> Rect {
    let left = (bounds.x * scale).floor();
    let top = (bounds.y * scale).floor();
    Rect::new(
        left,
        top,
        ((bounds.x + bounds.width) * scale).ceil() - left,
        ((bounds.y + bounds.height) * scale).ceil() - top,
    )
}

pub(crate) fn logical_bounds(bounds: Rect, scale: f32) -> Rect {
    Rect::new(
        bounds.x / scale,
        bounds.y / scale,
        bounds.width / scale,
        bounds.height / scale,
    )
}

struct LayoutMove<'a> {
    previous: Rect,
    next: Rect,
    offset: Point,
    tolerance: f32,
    sampled_clips: Option<std::slice::Iter<'a, Rect>>,
    viewport: Rect,
    exact_crop: Option<Rect>,
}

impl LayoutMove<'_> {
    fn new(previous: Rect, next: Rect, offset: Point, tolerance: f32) -> Self {
        Self {
            previous,
            next,
            offset,
            tolerance,
            sampled_clips: None,
            viewport: next,
            exact_crop: None,
        }
    }
}

fn translated_geometry_matches(
    previous: &LayoutNode,
    next: &LayoutNode,
    motion: &mut LayoutMove<'_>,
) -> bool {
    let translated = |rect: Rect, offset: Point| {
        Rect::new(
            rect.x + offset.x,
            rect.y + offset.y,
            rect.width,
            rect.height,
        )
    };
    let equal = |a: Rect, b: Rect| {
        a == b
            || motion.tolerance > 0.0
                && (a.x - b.x).abs() <= motion.tolerance
                && (a.y - b.y).abs() <= motion.tolerance
                && (a.width - b.width).abs() <= motion.tolerance
                && (a.height - b.height).abs() <= motion.tolerance
    };
    let clip = if let Some(regions) = &mut motion.sampled_clips {
        let Some(region) = regions.next() else {
            return false;
        };
        translated(
            *region,
            Point {
                x: motion.next.x,
                y: motion.next.y,
            },
        )
        .intersect(motion.viewport)
    } else {
        let clip = translated(previous.clip.intersect(motion.previous), motion.offset);
        if let Some(viewport) = motion.exact_crop {
            if !next.local_transform().is_identity() || next.table_pane_clip().is_some() {
                return false;
            }
            clip.intersect(viewport)
        } else {
            clip
        }
    };
    let next_clip = next.clip.intersect(motion.next);
    equal(translated(previous.bounds, motion.offset), next.bounds)
        && (equal(clip, next_clip)
            || (motion.sampled_clips.is_some() || motion.exact_crop.is_some())
                && !shadow::nonempty(clip)
                && !shadow::nonempty(next_clip))
}

#[derive(Clone, Copy)]
pub(crate) struct Raster {
    pub(crate) image: ImageId,
    bounds: Rect,
    pub(crate) bytes: u64,
}

struct CapturedTexture {
    texture: wgpu::Texture,
    bounds: Rect,
    bytes: u64,
}

impl CapturedTexture {
    fn allocate(
        device: &wgpu::Device,
        physical: Rect,
        scale: f32,
        format: wgpu::TextureFormat,
        budget: u64,
    ) -> Option<Self> {
        let Rect { width, height, .. } = physical;
        let limit = device.limits().max_texture_dimension_2d as f32;
        if !(width > 0.0 && width <= limit && height > 0.0 && height <= limit) {
            return None;
        }
        let (width, height) = (width as u32, height as u32);
        let texel_bytes = match format {
            wgpu::TextureFormat::Rgba16Float => 8,
            wgpu::TextureFormat::R16Float => 2,
            _ => 4,
        };
        let bytes = u64::from(width) * u64::from(height) * texel_bytes;
        if bytes > budget {
            return None;
        }
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack retained layer"),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        Some(Self {
            texture,
            bounds: logical_bounds(physical, scale),
            bytes,
        })
    }

    fn register(&self, canvas: &mut Canvas<WGPURenderer>, flags: ImageFlags) -> Option<Raster> {
        let info = ImageInfo::new(
            ImageFlags::PREMULTIPLIED | flags,
            self.texture.width() as usize,
            self.texture.height() as usize,
            PixelFormat::Rgba8,
        );
        let image = canvas
            .create_image_from_native_texture(self.texture.clone(), info)
            .ok()?;
        Some(Raster {
            image,
            bounds: self.bounds,
            bytes: self.bytes,
        })
    }
}

struct ShadowMask {
    main: Raster,
    capture: ImageId,
    last_frame: u64,
}

impl ShadowMask {
    fn delete(&self, main: &mut Canvas<WGPURenderer>, capture: Option<&mut Canvas<WGPURenderer>>) {
        main.delete_image(self.main.image);
        if let Some(capture) = capture {
            capture.delete_image(self.capture);
        }
    }
}

#[derive(Clone, Copy)]
struct OpacityTilePlacement {
    image: ImageId,
    edge: u32,
    scale: f32,
    raster: Rect,
    offset: Point,
}

impl OpacityTilePlacement {
    fn new(image: ImageId, edge: u32, scale: f32, raster: Rect, position: [u32; 2]) -> Self {
        Self {
            image,
            edge,
            scale,
            raster,
            offset: Point {
                x: position[0] as f32 / scale,
                y: position[1] as f32 / scale,
            },
        }
    }
}

struct OpacityRectangle {
    tile: OpacityTilePlacement,
    rectangle: crate::rectangle::Solid,
    clip: Rect,
    opacity: f32,
}

struct OpacityAtlas {
    image: ImageId,
    texture: wgpu::Texture,
    slots: Atlas,
    tiles: Vec<OpacityTile>,
    previous_tiles: Vec<OpacityTile>,
    composited: OnceCell<Rc<CachedLayerImage>>,
    pending: bool,
    cleared: bool,
    last_frame: u64,
}

#[derive(Clone, Copy, PartialEq)]
struct OpacityTileStyle {
    scale: f32,
    palette: Palette,
    in_layout: bool,
}

struct OpacityTile {
    node: Node,
    layout: Box<LayoutNode>,
    clip: Rect,
    device: Rect,
    position: [u32; 2],
    style: OpacityTileStyle,
    capture: OpacityTileCapture,
}

#[derive(Clone, Copy)]
struct OpacityTileCapture {
    moving: bool,
    mask: Option<shadow::ShadowDraw>,
    fonts: [usize; 2],
}

impl OpacityTile {
    fn same_content(
        &self,
        node: &Node,
        layout: &LayoutNode,
        style: OpacityTileStyle,
        fonts: [usize; 2],
    ) -> bool {
        self.capture.moving
            && self.style == style
            && self.capture.fonts == fonts
            && self.layout.bounds.width == layout.bounds.width
            && self.layout.bounds.height == layout.bounds.height
            && self.node.same_paint_except_layer_composition(node)
    }

    fn reuse_retained(
        &self,
        node: &Node,
        layout: &LayoutNode,
        clip: Rect,
        device: Rect,
        style: OpacityTileStyle,
        capture: OpacityTileCapture,
        can_crop: bool,
    ) -> Option<[u32; 2]> {
        if self.matches(node, layout, clip, device, style, capture) {
            return Some(self.position);
        }
        let clipped_edges = |clip: Rect, bounds: Rect| {
            [
                clip.x > bounds.x,
                clip.y > bounds.y,
                clip.x + clip.width < bounds.x + bounds.width,
                clip.y + clip.height < bounds.y + bounds.height,
            ]
        };
        let previous = self.layout.bounds;
        let next = layout.bounds;
        // Both captures must already be clipped on the same sides.
        // Switching from a full capture can change soft edge coverage on
        // the opposite side, even when the new cut is pixel-aligned.
        if !can_crop
            || clip == next
            || self.clip == previous
            || self.style != style
            || self.capture.fonts != capture.fonts
            || previous.width != next.width
            || previous.height != next.height
            || clipped_edges(self.clip, previous) != clipped_edges(clip, next)
            || !can_crop_sampled_layer(layout, style.scale)
            || !can_crop_sampled_layer(&self.layout, style.scale)
        {
            return None;
        }
        let offset = Point {
            x: next.x - previous.x,
            y: next.y - previous.y,
        };
        let [dx, dy] = [offset.x * style.scale, offset.y * style.scale];
        let [x, y] = [
            device.x - (self.device.x + dx),
            device.y - (self.device.y + dy),
        ];
        if dx.fract() != 0.0
            || dy.fract() != 0.0
            || x < 0.0
            || y < 0.0
            || x + device.width > self.device.width
            || y + device.height > self.device.height
            || !self.node.same_paint_except_layer_composition(node)
        {
            return None;
        }
        let mut motion = LayoutMove::new(self.clip, clip, offset, 0.0);
        motion.exact_crop = Some(layout.clip);
        self.layout
            .matches_translated(layout, true, |before, after| {
                // Clipping a child can change its own antialiasing. A smaller
                // crop may only reduce the root's visible area.
                (std::ptr::eq(after, layout)
                    || (!before.allows_overflow()
                        && !after.allows_overflow()
                        && before.clip.intersect(before.bounds) == before.bounds
                        && after.clip.intersect(after.bounds) == after.bounds))
                    && translated_geometry_matches(before, after, &mut motion)
            })
            .then(|| [self.position[0] + x as u32, self.position[1] + y as u32])
    }

    fn matches(
        &self,
        node: &Node,
        layout: &LayoutNode,
        clip: Rect,
        device: Rect,
        style: OpacityTileStyle,
        capture: OpacityTileCapture,
    ) -> bool {
        if self.style != style
            || self.capture.moving != capture.moving
            || self.capture.fonts != capture.fonts
            || self.device.width != device.width
            || self.device.height != device.height
            || self.layout.bounds.width != layout.bounds.width
            || self.layout.bounds.height != layout.bounds.height
        {
            return false;
        }
        // Only a whole-device-pixel translation can reuse the sampled
        // text and antialiased edges. Retain the full layout comparison.
        let offset = Point {
            x: layout.bounds.x - self.layout.bounds.x,
            y: layout.bounds.y - self.layout.bounds.y,
        };
        let shadow_matches = match (self.capture.mask, capture.mask) {
            (Some(previous), Some(next)) => previous.matches_translation(next, offset),
            (None, None) => true,
            _ => false,
        };
        if !shadow_matches
            || offset.x * style.scale != device.x - self.device.x
            || offset.y * style.scale != device.y - self.device.y
            || Rect::new(
                self.clip.x + offset.x,
                self.clip.y + offset.y,
                self.clip.width,
                self.clip.height,
            ) != clip
            || !self.node.same_paint_except_layer_composition(node)
        {
            return false;
        }
        let mut motion = LayoutMove::new(self.clip, clip, offset, 0.0);
        self.layout
            .matches_translated(layout, capture.moving, |before, after| {
                translated_geometry_matches(before, after, &mut motion)
            })
    }
}

#[derive(Clone, Copy, Default)]
struct OpacityDemand {
    groups: u64,
    pixels: u64,
    edge: u32,
}

pub(crate) struct RectangleOpacityAtlasPlan {
    edge: u32,
    pub(crate) covers_batch: bool,
}

impl OpacityDemand {
    fn record(&mut self, width: u32, height: u32) {
        self.groups += 1;
        self.pixels = self
            .pixels
            .saturating_add(u64::from(width) * u64::from(height));
        self.edge = self.edge.max(width).max(height);
    }

    fn atlas_edge(self, width: u32, height: u32, limit: u32) -> u32 {
        let area = self.pixels.min(u64::from(limit) * u64::from(limit));
        // Reserve some slack for rectangular slots that do not pack tightly.
        let estimated = (1.5 * area as f64).sqrt().ceil() as u32;
        estimated
            .max(self.edge)
            .max(width)
            .max(height)
            .max(256)
            .next_multiple_of(256)
            .min(limit)
    }
}

#[derive(Clone, Copy, Debug, Default)]
pub struct OpacityAtlasStats {
    pub groups: u64,
    pub submissions: u64,
    pub reused_tiles: u64,
    pub retained_hits: u64,
}

struct ChangingGroup {
    node: Box<Node>,
    last_frame: u64,
    moving_phase: Option<LayerPlacement>,
    window_surface: bool,
}

#[derive(Clone, Copy, PartialEq)]
struct LayerPlacement {
    bounds: Rect,
    scale: f32,
}

impl LayerPlacement {
    fn same_phase(self, other: Self) -> bool {
        self.scale == other.scale
            && ((self.bounds.x - other.bounds.x) * self.scale).fract() == 0.0
            && ((self.bounds.y - other.bounds.y) * self.scale).fract() == 0.0
    }

    fn preserves_surface_raster(self, other: Self) -> bool {
        // Both placements must be eligible for capture at the same device-pixel phase.
        if !self.same_phase(other) {
            return false;
        }
        let before = device_bounds(self.bounds, self.scale);
        let after = device_bounds(other.bounds, other.scale);
        self.bounds.width == other.bounds.width
            && self.bounds.height == other.bounds.height
            && before.width == after.width
            && before.height == after.height
    }
}

pub(crate) enum OpacitySource<'a> {
    Transient(Option<&'a (shadow::OuterShadow, Rect)>),
    MovingLayer,
    Composited,
}

#[derive(Default)]
struct FontDefaults {
    family: String,
    weight: u16,
    italic: bool,
    size: Option<f32>,
}

#[derive(Clone, Eq, Ord, PartialEq, PartialOrd)]
enum LayerOwner {
    Path(Vec<usize>),
    SceneNode(SceneNodeId),
}

pub(crate) struct RetainedLayers {
    layers: Vec<Layer>,
    opacity: BTreeMap<LayerOwner, OpacityLayer>,
    alternate_opacity: BTreeMap<LayerOwner, OpacityLayer>,
    opacity_stats: LayerStats,
    pub(crate) vectors: vector_masks::Masks,
    shadow_masks: HashMap<shadow::ShadowMaskKey, ShadowMask>,
    window_clip_masks: Vec<RetainedClipMask>,
    changing_groups: BTreeMap<LayerOwner, ChangingGroup>,
    shadow_mask_stats: LayerStats,
    frame_number: u64,
    new_shadow_masks: usize,
    opacity_atlas: Option<OpacityAtlas>,
    window_opacity_atlas: Option<WindowOpacityAtlas>,
    opacity_atlas_stats: OpacityAtlasStats,
    opacity_atlas_disabled: bool,
    #[cfg(xrack_femtovg_glyph_samples)]
    direct_rounded_fills_disabled: bool,
    opacity_tile_sharing: bool,
    hinted_layer_sharing: bool,
    opacity_demand: OpacityDemand,
    previous_opacity_demand: OpacityDemand,
    capturing: bool,
    capture: Option<(Canvas<WGPURenderer>, image::Images)>,
    budget: u64,
    stats: LayerStats,
    font: FontDefaults,
}

impl Default for RetainedLayers {
    fn default() -> Self {
        Self {
            layers: Vec::new(),
            opacity: BTreeMap::new(),
            alternate_opacity: BTreeMap::new(),
            opacity_stats: LayerStats::default(),
            vectors: Default::default(),
            shadow_masks: HashMap::new(),
            window_clip_masks: Vec::new(),
            changing_groups: BTreeMap::new(),
            shadow_mask_stats: LayerStats::default(),
            frame_number: 0,
            new_shadow_masks: 0,
            opacity_atlas: None,
            window_opacity_atlas: None,
            opacity_atlas_stats: OpacityAtlasStats::default(),
            opacity_atlas_disabled: false,
            #[cfg(xrack_femtovg_glyph_samples)]
            direct_rounded_fills_disabled: false,
            opacity_tile_sharing: true,
            hinted_layer_sharing: true,
            opacity_demand: Default::default(),
            previous_opacity_demand: Default::default(),
            capturing: false,
            capture: None,
            budget: 32 * 1024 * 1024,
            stats: LayerStats::default(),
            font: FontDefaults::default(),
        }
    }
}

impl RetainedLayers {
    pub(crate) fn invalidate_live_image_layers(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        let has_live_image = |node: &Node| {
            node.contains_scene_feature(xrack_runtime::SceneFeature::SceneImageTexture)
        };
        for cache in [&mut self.opacity, &mut self.alternate_opacity] {
            cache.retain(|_, layer| {
                if has_live_image(&layer.node) {
                    layer.release_image(canvas);
                    false
                } else {
                    true
                }
            });
        }
        self.layers.retain(|layer| {
            if layer
                .page
                .upgrade()
                .is_some_and(|page| has_live_image(&page.0))
            {
                canvas.delete_image(layer.raster.image);
                false
            } else {
                true
            }
        });
    }

    pub(crate) fn reset_canvas(&mut self) {
        self.layers.clear();
        self.opacity.clear();
        self.alternate_opacity.clear();
        self.vectors.reset_canvas();
        self.shadow_masks.clear();
        self.window_clip_masks.clear();
        self.changing_groups.clear();
        self.opacity_atlas = None;
        self.window_opacity_atlas = None;
        self.opacity_demand = Default::default();
        self.previous_opacity_demand = Default::default();
        self.capture = None;
    }

    pub(crate) fn begin_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.frame_number = self.frame_number.wrapping_add(1);
        if self.window_opacity_atlas.as_ref().is_some_and(|atlas| {
            self.frame_number.wrapping_sub(atlas.last_frame) > OPACITY_ATLAS_LIFETIME
        }) {
            self.clear_window_opacity_atlas(canvas);
        }
        self.window_clip_masks.retain(|entry| {
            let idle = if entry.shapes.len() == 1
                && crate::display_list::fractional_window_composition()
            {
                4
            } else {
                2
            };
            self.frame_number.wrapping_sub(entry.mask.last_frame.get()) <= idle
        });
        self.new_shadow_masks = 0;
        self.previous_opacity_demand = std::mem::take(&mut self.opacity_demand);
        if self.opacity_atlas.as_ref().is_some_and(|atlas| {
            self.frame_number.wrapping_sub(atlas.last_frame) > OPACITY_ATLAS_LIFETIME
        }) {
            self.clear_opacity_atlas(canvas);
        }
        if let Some(atlas) = &mut self.opacity_atlas {
            let edge = atlas.texture.width() as usize;
            atlas.slots.reset(edge, edge);
            std::mem::swap(&mut atlas.tiles, &mut atlas.previous_tiles);
            atlas.tiles.clear();
            atlas.cleared = false;
        }
        self.vectors.begin_frame();
        self.layers.retain(|layer| {
            if layer.page.strong_count() != 0 {
                return true;
            }
            canvas.delete_image(layer.raster.image);
            false
        });
        for layer in self.opacity.values_mut() {
            layer.used = false;
        }
    }

    pub(crate) fn finish_frame(&mut self, canvas: &mut Canvas<WGPURenderer>, scene: &SceneIndex) {
        self.vectors.finish_frame(canvas);
        self.changing_groups.retain(|_, group| {
            group.last_frame == self.frame_number
                || crate::display_list::fractional_window_composition()
                    && group.window_surface
                    && self.frame_number.wrapping_sub(group.last_frame) <= 2
        });
        let capture = &mut self.capture;
        self.shadow_masks.retain(|_, mask| {
            if self.frame_number.wrapping_sub(mask.last_frame) <= SHADOW_MASK_LIFETIME {
                return true;
            }
            mask.delete(canvas, capture.as_mut().map(|(canvas, _)| canvas));
            false
        });
        self.opacity.retain(|owner, layer| {
            if layer.used
                || crate::display_list::fractional_window_composition()
                    && layer.surface_only
                    && self.frame_number.wrapping_sub(layer.last_frame) <= 2
                || self.frame_number.wrapping_sub(layer.last_frame) <= 120
                    && matches!(owner, LayerOwner::SceneNode(id) if scene.is_pooled(*id))
            {
                return true;
            }
            layer.release_image(canvas);
            false
        });
        self.alternate_opacity.retain(|owner, layer| {
            if self.opacity.contains_key(owner)
                && self.frame_number.wrapping_sub(layer.last_frame) <= OPACITY_ATLAS_LIFETIME
            {
                return true;
            }
            layer.release_image(canvas);
            false
        });
    }

    pub(crate) fn clear_images(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.window_clip_masks.clear();
        self.clear_opacity_atlas(canvas);
        self.clear_window_opacity_atlas(canvas);
        self.opacity_demand = Default::default();
        self.previous_opacity_demand = Default::default();
        self.vectors.clear(canvas);
        self.changing_groups.clear();
        for (_, mask) in self.shadow_masks.drain() {
            mask.delete(canvas, self.capture.as_mut().map(|(canvas, _)| canvas));
        }
        for layer in self.layers.drain(..) {
            canvas.delete_image(layer.raster.image);
        }
        for layer in std::mem::take(&mut self.opacity)
            .into_values()
            .chain(std::mem::take(&mut self.alternate_opacity).into_values())
        {
            layer.release_image(canvas);
        }
    }

    fn clear_window_opacity_atlas(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        if let Some(image) = self
            .window_opacity_atlas
            .take()
            .and_then(|atlas| atlas.canvas_image)
        {
            canvas.delete_image(image);
        }
    }

    fn clear_opacity_atlas(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        if let Some(atlas) = self.opacity_atlas.take() {
            canvas.delete_image(atlas.image);
        }
    }

    fn resident_bytes(&self) -> u64 {
        let atlas = self.opacity_atlas.as_ref().map_or(0, |atlas| {
            u64::from(atlas.texture.width()) * u64::from(atlas.texture.height()) * 4
        });
        let mut charged = HashSet::new();
        let window = self.window_opacity_atlas.as_ref().map_or(0, |atlas| {
            WindowOpacityAtlas::bytes(atlas.image.texture.width())
        });
        atlas
            + window
            + self.vectors.resident_bytes()
            + self
                .layers
                .iter()
                .map(|layer| layer.raster.bytes)
                .chain(
                    self.opacity
                        .values()
                        .chain(self.alternate_opacity.values())
                        .filter(|layer| {
                            Rc::strong_count(&layer.image) == 1 || charged.insert(layer.image.image)
                        })
                        .map(|layer| layer.image.bytes),
                )
                .chain(self.shadow_masks.values().map(|mask| mask.main.bytes))
                .chain(self.window_clip_masks.iter().map(|mask| mask.bytes))
                .sum::<u64>()
    }

    pub(crate) fn sync_font_defaults(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        defaults: (&str, u16, bool, Option<f32>),
    ) {
        let (family, weight, italic, size) = defaults;
        let last = &mut self.font;
        if last.family == family
            && last.weight == weight
            && last.italic == italic
            && last.size == size
        {
            return;
        }
        last.family.clear();
        last.family.push_str(family);
        last.weight = weight;
        last.italic = italic;
        last.size = size;
        self.clear_images(canvas);
    }

    fn has_retained_opacity_paint(
        &self,
        node: &Node,
        layout: &LayoutNode,
        style: OpacityTileStyle,
        fonts: [usize; 2],
    ) -> bool {
        self.window_opacity_atlas.as_ref().is_some_and(|atlas| {
            atlas
                .retained
                .iter()
                .any(|tile| tile.capture.same_content(node, layout, style, fonts))
        })
    }

    fn admit_stable_group(
        &mut self,
        owner: &LayerOwner,
        node: &Node,
        placement: LayerPlacement,
        repeated_phase_change: bool,
        defer_new_capture: bool,
    ) -> bool {
        if let Some(group) = self.changing_groups.get_mut(owner) {
            let same_paint = group.node.same_paint_except_layer_composition(node);
            if same_paint
                && group
                    .moving_phase
                    .is_none_or(|phase| phase.same_phase(placement))
            {
                self.changing_groups.remove(owner);
                return true;
            }
            if !same_paint {
                group.node.as_mut().clone_from(node);
            }
            if group.moving_phase.is_some() {
                group.moving_phase = Some(placement);
            }
            group.window_surface = false;
            group.last_frame = self.frame_number;
            return false;
        }
        if !repeated_phase_change
            && self.opacity.get(owner).map_or(!defer_new_capture, |layer| {
                layer.node.same_paint_except_layer_composition(node)
            })
        {
            return true;
        }
        // Wait for a stable snapshot before allocating another persistent
        // texture. A changing group can use the temporary opacity atlas.
        if self.changing_groups.len() < MAX_LAYERS {
            self.changing_groups.insert(
                owner.clone(),
                ChangingGroup {
                    node: Box::new(node.clone()),
                    last_frame: self.frame_number,
                    moving_phase: repeated_phase_change.then_some(placement),
                    window_surface: false,
                },
            );
        }
        false
    }
}

impl Renderer {
    pub(crate) fn grow_window_opacity_atlas(&mut self) {
        let cache = &mut self.retained_layers;
        if cache.opacity_atlas_disabled
            || !cache
                .window_opacity_atlas
                .as_ref()
                .is_some_and(|atlas| atlas.grow_next_frame)
        {
            return;
        }
        let budget = cache.budget.saturating_sub(cache.resident_bytes());
        let Some(atlas) = &mut cache.window_opacity_atlas else {
            return;
        };
        atlas.grow_next_frame = false;
        let old = &atlas.image.texture;
        let limit = MAX_OPACITY_ATLAS_EDGE.min(self.device.limits().max_texture_dimension_2d);
        let Some(edge) = [MAX_WINDOW_ATLAS_EDGE, limit].into_iter().find(|&edge| {
            old.width() < edge && edge <= limit && WindowOpacityAtlas::bytes(edge) <= budget
        }) else {
            return;
        };
        let image = WindowOpacityAtlas::image(&self.device, edge);
        let encoder = self.custom_encoder.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("XRack compositor atlas growth"),
                })
        });
        // Grow before collecting the frame's layers so all UVs use the new
        // texture size. Existing allocations keep their pixel coordinates.
        encoder.copy_texture_to_texture(
            old.as_image_copy(),
            image.texture.as_image_copy(),
            old.size(),
        );
        atlas.packing.expand(edge as usize, edge as usize);
        if let Some(previous) = atlas.canvas_image.take() {
            self.canvas.delete_image(previous);
        }
        atlas.image = image;
    }

    pub(crate) fn pack_composited_opacity_layers(&mut self, layers: &mut [WindowLayer]) {
        fn source<'a>(
            layer: &'a WindowLayer,
            transient: Option<&Rc<CachedLayerImage>>,
        ) -> Option<&'a Rc<CachedLayerImage>> {
            let WindowLayerSource::Texture(image) = &layer.source else {
                return None;
            };
            let texture = &image.texture;
            (!layer.surface_only
                && transient.is_none_or(|transient| !Rc::ptr_eq(transient, image))
                && texture.format() == wgpu::TextureFormat::Rgba8Unorm
                && texture.usage().contains(wgpu::TextureUsages::COPY_SRC)
                && texture.width() + 2 <= MIN_WINDOW_ATLAS_EDGE
                && texture.height() + 2 <= MIN_WINDOW_ATLAS_EDGE)
                .then_some(image)
        }

        let cache = &mut self.retained_layers;
        if cache.opacity_atlas_disabled {
            return;
        }
        let transient = cache
            .opacity_atlas
            .as_ref()
            .and_then(|atlas| atlas.composited.get());
        if let Some(atlas) = &mut cache.window_opacity_atlas
            && atlas.retained.is_empty()
            && !atlas.tiles.is_empty()
            && atlas
                .tiles
                .values()
                .all(|tile| tile.source.strong_count() == 0)
        {
            atlas.tiles.clear();
            let edge = atlas.image.texture.width() as usize;
            atlas.packing.reset(edge, edge);
        }
        if cache.window_opacity_atlas.is_none() {
            let mut demand = OpacityDemand::default();
            let mut sources = HashSet::new();
            for image in layers.iter().filter_map(|layer| source(layer, transient)) {
                if sources.insert(Rc::as_ptr(image)) {
                    demand.record(image.texture.width() + 2, image.texture.height() + 2);
                }
            }
            if demand.groups < 2 {
                return;
            }
            let limit = self
                .device
                .limits()
                .max_texture_dimension_2d
                .min(MAX_WINDOW_ATLAS_EDGE);
            let desired = demand.atlas_edge(0, 0, limit).max(MIN_WINDOW_ATLAS_EDGE);
            let available = cache.budget.saturating_sub(cache.resident_bytes());
            let Some(edge) = [desired, MIN_WINDOW_ATLAS_EDGE]
                .into_iter()
                .find(|&edge| edge <= limit && WindowOpacityAtlas::bytes(edge) <= available)
            else {
                return;
            };
            cache.window_opacity_atlas = Some(WindowOpacityAtlas::new(
                &self.device,
                edge,
                cache.frame_number,
            ));
        }
        let Some(atlas) = &mut cache.window_opacity_atlas else {
            return;
        };
        let edge = atlas.image.texture.width() as f32;
        for layer in layers {
            let Some(image) = source(layer, transient) else {
                continue;
            };
            let Some([x, y]) = atlas.tile(
                image,
                &self.device,
                &mut self.custom_encoder,
                cache.frame_number,
            ) else {
                continue;
            };
            let [u, v, right, bottom] = layer.uv;
            let width = image.texture.width() as f32;
            let height = image.texture.height() as f32;
            layer.uv = [
                (x as f32 + u * width) / edge,
                (y as f32 + v * height) / edge,
                (x as f32 + right * width) / edge,
                (y as f32 + bottom * height) / edge,
            ];
            layer.source = WindowLayerSource::Texture(atlas.image.clone());
            atlas.last_frame = cache.frame_number;
        }
    }

    #[doc(hidden)]
    pub fn stack_layer_stats(&self) -> LayerStats {
        self.retained_layers.stats
    }

    #[doc(hidden)]
    pub fn set_layer_budget(&mut self, bytes: u64) {
        self.submit_opacity_atlas();
        self.retained_layers.budget = bytes;
        self.retained_layers.clear_images(&mut self.canvas);
        if bytes == 0 {
            self.retained_layers.capture = None;
            self.rectangle_batches.reset_capture_canvas();
        }
    }

    #[doc(hidden)]
    pub fn opacity_layer_stats(&self) -> LayerStats {
        self.retained_layers.opacity_stats
    }

    #[doc(hidden)]
    pub fn set_opacity_atlas_enabled(&mut self, enabled: bool) {
        self.retained_layers.opacity_atlas_disabled = !enabled;
    }

    /// Compares direct fractional rounded fills with the retained painter.
    #[doc(hidden)]
    pub fn set_direct_rounded_fills_enabled(&mut self, enabled: bool) {
        #[cfg(xrack_femtovg_glyph_samples)]
        {
            self.retained_layers.direct_rounded_fills_disabled = !enabled;
        }
        #[cfg(not(xrack_femtovg_glyph_samples))]
        let _ = enabled;
    }

    pub(crate) fn direct_rounded_fills_enabled(&self) -> bool {
        #[cfg(xrack_femtovg_glyph_samples)]
        {
            !self.retained_layers.direct_rounded_fills_disabled
        }
        #[cfg(not(xrack_femtovg_glyph_samples))]
        false
    }

    #[doc(hidden)]
    pub fn opacity_atlas_stats(&self) -> OpacityAtlasStats {
        self.retained_layers.opacity_atlas_stats
    }

    #[doc(hidden)]
    pub fn opacity_atlas_extent(&self) -> Option<u32> {
        self.retained_layers
            .opacity_atlas
            .as_ref()
            .map(|atlas| atlas.texture.width())
    }

    #[doc(hidden)]
    pub fn set_opacity_tile_sharing_enabled(&mut self, enabled: bool) {
        self.retained_layers.opacity_tile_sharing = enabled;
    }

    #[doc(hidden)]
    pub fn set_hinted_layer_sharing_enabled(&mut self, enabled: bool) {
        self.retained_layers.hinted_layer_sharing = enabled;
    }

    #[doc(hidden)]
    pub fn shadow_mask_stats(&self) -> LayerStats {
        self.retained_layers.shadow_mask_stats
    }

    pub(crate) fn prepare_shadow_mask(
        &mut self,
        shadow: Option<&(shadow::OuterShadow, Rect)>,
        layout: &LayoutNode,
        scale: f32,
        for_capture: bool,
    ) -> Result<Option<shadow::ShadowDraw>, String> {
        let Some((shadow, _)) = shadow else {
            return Ok(None);
        };
        if !self.can_retain_layer(scale) || layout.has_ancestor_clip_chain() {
            return Ok(None);
        }
        let Some(tile) = shadow.mask_tile(scale, layout.clip) else {
            return Ok(None);
        };
        let last_frame = self.retained_layers.frame_number;
        if let Some(mask) = self.retained_layers.shadow_masks.get_mut(&tile.key) {
            mask.last_frame = last_frame;
            self.retained_layers.shadow_mask_stats.reused += 1;
            let image = if for_capture {
                mask.capture
            } else {
                mask.main.image
            };
            return Ok(Some(shadow.masked_draw(image, tile)));
        }
        if self.retained_layers.new_shadow_masks >= MAX_NEW_SHADOW_MASKS
            || !self.make_shadow_mask_room()
        {
            return Ok(None);
        }
        let Some(texture) = self.capture_device_texture(tile.source_pixels, scale, |renderer| {
            shadow.draw_mask(&mut renderer.canvas, tile, scale);
            Ok(())
        })?
        else {
            return Ok(None);
        };
        let Some((offscreen, _)) = self.retained_layers.capture.as_mut() else {
            return Ok(None);
        };
        let flags = tile.sampling();
        let Some(capture) = texture.register(offscreen, flags) else {
            return Ok(None);
        };
        let Some(main) = self.cache_shadow_mask(tile.key, &texture, capture.image, flags) else {
            return Ok(None);
        };
        let image = if for_capture { capture.image } else { main };
        Ok(Some(shadow.masked_draw(image, tile)))
    }

    fn cache_shadow_mask(
        &mut self,
        key: shadow::ShadowMaskKey,
        texture: &CapturedTexture,
        capture: ImageId,
        flags: ImageFlags,
    ) -> Option<ImageId> {
        let Some(main) = texture.register(&mut self.canvas, flags) else {
            if let Some((canvas, _)) = self.retained_layers.capture.as_mut() {
                canvas.delete_image(capture);
            }
            return None;
        };
        self.retained_layers.shadow_masks.insert(
            key,
            ShadowMask {
                main,
                capture,
                last_frame: self.retained_layers.frame_number,
            },
        );
        self.retained_layers.new_shadow_masks += 1;
        self.retained_layers.shadow_mask_stats.rasterized += 1;
        Some(main.image)
    }

    fn make_shadow_mask_room(&mut self) -> bool {
        if self.retained_layers.shadow_masks.len() < MAX_SHADOW_MASKS {
            return true;
        }
        let frame = self.retained_layers.frame_number;
        let oldest = self
            .retained_layers
            .shadow_masks
            .iter()
            .filter(|(_, mask)| mask.last_frame != frame)
            .max_by_key(|(_, mask)| frame.wrapping_sub(mask.last_frame))
            .map(|(key, _)| *key);
        let Some(mask) = oldest.and_then(|key| self.retained_layers.shadow_masks.remove(&key))
        else {
            return false;
        };
        mask.delete(
            &mut self.canvas,
            self.retained_layers
                .capture
                .as_mut()
                .map(|(canvas, _)| canvas),
        );
        true
    }

    pub(crate) fn can_retain_layer(&self, scale: f32) -> bool {
        self.can_cache_on_main_canvas() && has_device_scale(canvas_transform(&self.canvas), scale)
    }

    pub(crate) fn opacity_layer_count(&self) -> usize {
        self.retained_layers.opacity.len()
    }

    fn opacity_owner(&self, path: &[usize]) -> LayerOwner {
        self.geometry_cache
            .enabled
            .then(|| self.render_scene.node_at(path))
            .flatten()
            .map_or_else(|| LayerOwner::Path(path.to_vec()), LayerOwner::SceneNode)
    }

    pub(crate) fn exact_masked_group(
        node: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
    ) -> bool {
        Self::can_capture_clipped_group(node, layout, frame.scale)
            && !node.boolean("cache-rendering-hint", false)
            && shadow::OuterShadow::clipped_for_node(
                node,
                layout,
                frame.scale,
                frame.theme.role(crate::Role::Border),
            )
            .is_none()
    }

    fn can_capture_clipped_group(node: &Node, layout: &LayoutNode, scale: f32) -> bool {
        !cfg!(any(target_os = "ios", target_os = "android"))
            && 0.0 < node.opacity()
            && node.opacity() < 1.0
            && scale >= 1.0
            && !node.children.is_empty()
            && layout.has_ancestor_clip_chain()
            && !layout.allows_overflow()
            && layout.local_transform().is_identity()
            && layout.table_pane_clip().is_none()
    }

    fn cached_masked_group(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &[usize],
        in_layout: bool,
        frame: PaintFrame<'_>,
    ) -> Option<WindowLayer> {
        let clip = layout.bounds.intersect(layout.clip);
        if clip != layout.bounds || !shadow::nonempty(clip) {
            return None;
        }
        let owner = self.opacity_owner(path);
        let style = OpacityTileStyle {
            scale: frame.scale,
            palette: frame.theme,
            in_layout,
        };
        // Each texture retains the exact device-pixel phase of its capture.
        // The compositor applies the ancestor clip in the new placement.
        for cache in [
            &mut self.retained_layers.opacity,
            &mut self.retained_layers.alternate_opacity,
        ] {
            let Some(layer) = cache.get_mut(&owner) else {
                continue;
            };
            if layer.resample_origin.is_some()
                || !layer.matches_paint(node, style)
                || !layer.reuse_at(layout, clip, true, true)
            {
                continue;
            }
            let Some(image) = layer.image.cached.clone() else {
                continue;
            };
            layer.last_frame = self.retained_layers.frame_number;
            return Some(WindowLayer {
                owner: Some(owner),
                source: WindowLayerSource::Texture(image),
                bounds: layer.raster_bounds,
                uv: [0.0, 0.0, 1.0, 1.0],
                opacity: node.opacity(),
                surface_only: false,
                mask: None,
                clip_samples: [false; 2],
                edge_clip: (!crate::pixel_aligned(clip, frame.scale)).then_some(clip),
            });
        }
        None
    }

    pub(crate) fn cached_window_layer(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &[usize],
        in_layout: bool,
        frame: PaintFrame<'_>,
    ) -> Option<WindowLayer> {
        if Self::exact_masked_group(node, layout, frame) {
            return self.cached_masked_group(node, layout, path, in_layout, frame);
        }
        let bounds = layout.bounds;
        let opacity = node.opacity();
        let full_coverage = layout.clip.x <= bounds.x
            && layout.clip.y <= bounds.y
            && layout.clip.x + layout.clip.width >= bounds.x + bounds.width
            && layout.clip.y + layout.clip.height >= bounds.y + bounds.height;
        let hinted = node.boolean("cache-rendering-hint", false);
        let whole_group =
            hinted || Self::can_composite_exact_opacity(node, layout, frame.scale, opacity);
        if !(whole_group || Self::can_cache_window_surface(node, layout, frame.scale))
            || !opacity.is_finite()
            || layout.allows_overflow()
            || !layout.local_transform().is_identity()
            || layout.table_pane_clip().is_some()
            || !full_coverage && !can_crop_sampled_layer(layout, frame.scale)
        {
            return None;
        }
        let owner = self.opacity_owner(path);
        if fractional_window_surface(layout) && !whole_group {
            let placement = LayerPlacement {
                bounds,
                scale: frame.scale,
            };
            let matches_phase = |layer: &OpacityLayer| {
                layer.surface_only
                    && (LayerPlacement {
                        bounds: layer.captured_layout.bounds,
                        scale: layer.scale,
                    })
                    .preserves_surface_raster(placement)
                    && layer.node.same_own_paint_except_position(node)
                    && layer.palette == frame.theme
                    && layer.in_layout == in_layout
                    && same_window_surface_layout(&layer.captured_layout, layout)
            };
            if !self
                .retained_layers
                .opacity
                .get(&owner)
                .is_some_and(&matches_phase)
                && self
                    .retained_layers
                    .alternate_opacity
                    .get(&owner)
                    .is_some_and(&matches_phase)
            {
                if let Some(mut layer) = self.retained_layers.alternate_opacity.remove(&owner) {
                    layer.last_frame = self.retained_layers.frame_number;
                    if let Some(mut previous) =
                        self.retained_layers.opacity.insert(owner.clone(), layer)
                    {
                        previous.last_frame = self.retained_layers.frame_number;
                        self.retained_layers
                            .alternate_opacity
                            .insert(owner.clone(), previous);
                    }
                }
            }
        }
        let layer = self.retained_layers.opacity.get_mut(&owner)?;
        // Group opacity must also apply to the children of the surface.
        if layer.surface_only == whole_group
            || layer.scale != frame.scale
            || layer.palette != frame.theme
            || layer.in_layout != in_layout
            || !full_coverage && layer.resample_origin.is_none()
        {
            return None;
        }
        let image = layer.image.cached.as_ref()?.clone();
        let raster_bounds = if layer.surface_only
            && (matches!(item_paint(node), ItemPaint::Rectangle)
                || fractional_window_surface(layout))
        {
            let captured = LayerPlacement {
                bounds: layer.captured_layout.bounds,
                scale: layer.scale,
            };
            let current = LayerPlacement {
                bounds,
                scale: frame.scale,
            };
            // The background raster covers only the panel surface. Descendant
            // layout and the current ancestor clip are handled by the compositor.
            if !captured.preserves_surface_raster(current)
                || !same_window_surface_layout(&layer.captured_layout, layout)
                || !layer.node.same_own_paint_except_position(node)
            {
                return None;
            }
            logical_bounds(device_bounds(bounds, frame.scale), frame.scale)
        } else {
            if !layer.node.same_paint_except_layer_composition(node) {
                return None;
            }
            let clip = if full_coverage {
                bounds
            } else {
                bounds.intersect(layout.clip)
            };
            if !layer.reuse_at(layout, clip, full_coverage, false) {
                if let Some(layer) = self.retained_layers.opacity.remove(&owner) {
                    layer.release_image(&mut self.canvas);
                }
                return None;
            }
            layer.raster_bounds
        };
        Some(WindowLayer {
            owner: Some(owner),
            source: WindowLayerSource::Texture(image),
            bounds: raster_bounds,
            uv: [0.0, 0.0, 1.0, 1.0],
            opacity,
            surface_only: layer.surface_only,
            mask: None,
            clip_samples: [false; 2],
            edge_clip: None,
        })
    }

    fn can_composite_exact_opacity(
        node: &Node,
        layout: &LayoutNode,
        scale: f32,
        opacity: f32,
    ) -> bool {
        0.0 < opacity
            && opacity < 1.0
            && !node.children.is_empty()
            && scale >= 1.0
            && !layout.has_ancestor_clip_chain()
            && layout.clip.intersect(layout.bounds) == layout.bounds
            && crate::pixel_aligned(layout.bounds, scale)
    }

    pub(crate) fn can_cache_window_surface(node: &Node, layout: &LayoutNode, scale: f32) -> bool {
        let plain = match item_paint(node) {
            ItemPaint::Rectangle => {
                node.opacity() == 1.0
                    && node.color("background").is_some()
                    && crate::rectangle::plain_surface(node, layout)
            }
            ItemPaint::Label | ItemPaint::Text => {
                Self::is_plain_label(node, layout)
                    && matches!(
                        node.properties.get("text"),
                        Some(xrack_runtime::Value::String(_))
                    )
            }
            ItemPaint::Image { animated: false } => {
                Self::is_plain_image(node, layout)
                    && node
                        .image_asset("source")
                        .is_some_and(|asset| asset.pixels().is_some())
            }
            _ => false,
        };
        plain
            && layout.local_transform().is_identity()
            && layout.table_pane_clip().is_none()
            && layout.clip.intersect(layout.bounds) == layout.bounds
            && (fractional_window_surface(layout) || crate::pixel_aligned(layout.bounds, scale))
            && shadow::nonempty(layout.bounds)
    }

    pub(crate) fn admit_window_surface(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &[usize],
        scale: f32,
    ) -> bool {
        let owner = self.opacity_owner(path);
        let previous_frame = self.retained_layers.frame_number.wrapping_sub(1);
        let placement = LayerPlacement {
            bounds: layout.bounds,
            scale,
        };
        let candidates = &mut self.retained_layers.changing_groups;
        if let Some(previous) = candidates.get_mut(&owner) {
            let stable = (previous.last_frame == previous_frame
                || fractional_window_surface(layout)
                    && previous.window_surface
                    && previous_frame.wrapping_sub(previous.last_frame) <= 2)
                && if fractional_window_surface(layout) {
                    previous.moving_phase.is_some_and(|old| {
                        old.scale == placement.scale
                            && old.bounds.width == placement.bounds.width
                            && old.bounds.height == placement.bounds.height
                    }) && previous.node.same_own_paint_except_position(node)
                } else if matches!(item_paint(node), ItemPaint::Rectangle) {
                    previous
                        .moving_phase
                        .is_some_and(|old| old.preserves_surface_raster(placement))
                        && previous.node.same_own_paint_except_position(node)
                } else {
                    previous.moving_phase == Some(placement)
                        && previous.node.same_own_paint_inputs(node)
                };
            if !stable {
                previous.node.as_mut().clone_from(node);
            }
            previous.last_frame = self.retained_layers.frame_number;
            previous.moving_phase = Some(placement);
            previous.window_surface = true;
            return stable;
        }
        if candidates.len() < MAX_LAYERS {
            candidates.insert(
                owner,
                ChangingGroup {
                    node: Box::new(node.clone()),
                    last_frame: self.retained_layers.frame_number,
                    moving_phase: Some(placement),
                    window_surface: true,
                },
            );
        }
        false
    }

    pub(crate) fn capture_window_surface(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &[usize],
        in_layout: bool,
        frame: PaintFrame<'_>,
    ) -> Option<WindowLayer> {
        let bounds = layout.bounds;
        let owner = self.opacity_owner(path);
        if let Some(layer) = self.retained_layers.opacity.remove(&owner) {
            if fractional_window_surface(layout)
                && layer.surface_only
                && layer.scale == frame.scale
                && layer.palette == frame.theme
                && layer.in_layout == in_layout
                && layer.node.same_own_paint_except_position(node)
                && same_window_surface_layout(&layer.captured_layout, layout)
                && !(LayerPlacement {
                    bounds: layer.captured_layout.bounds,
                    scale: layer.scale,
                })
                .same_phase(LayerPlacement {
                    bounds,
                    scale: frame.scale,
                })
                && layer.captured_layout.bounds.width == bounds.width
                && layer.captured_layout.bounds.height == bounds.height
            {
                if let Some(old) = self
                    .retained_layers
                    .alternate_opacity
                    .insert(owner.clone(), layer)
                {
                    old.release_image(&mut self.canvas);
                }
            } else {
                layer.release_image(&mut self.canvas);
            }
        }
        // Ordinary paints blend coverage directly into the window.
        // The float image keeps that coverage until the final blend.
        let captured = self
            .capture_device_texture_with_format(
                device_bounds(bounds, frame.scale),
                frame.scale,
                wgpu::TextureFormat::Rgba16Float,
                |renderer| {
                    renderer.paint_group(bounds, 1.0, |renderer| {
                        let item = xrack_layout::item::ItemRef { node, layout };
                        renderer.paint_surface(
                            item,
                            item.paint(),
                            &mut path.to_vec(),
                            frame,
                            in_layout,
                        )
                    })
                },
            )
            .ok()??;
        let raster = captured.register(&mut self.canvas, ImageFlags::NEAREST)?;
        let image = Rc::new(CachedLayerImage {
            texture: captured.texture,
            binding: OnceCell::new(),
        });
        self.retained_layers.opacity.insert(
            owner.clone(),
            OpacityLayer {
                node: Box::new(node.clone()),
                captured_layout: Box::new(layout.clone()),
                capture_clip: bounds,
                full_coverage: true,
                image: Rc::new(OpacityImage {
                    image: raster.image,
                    bytes: raster.bytes,
                    cached: Some(image.clone()),
                    fonts: None,
                }),
                raster_bounds: raster.bounds,
                resample_origin: None,
                sampled_clips: None,
                scale: frame.scale,
                palette: frame.theme,
                in_layout,
                surface_only: true,
                used: true,
                last_frame: self.retained_layers.frame_number,
                recaptured_at_new_phase: false,
            },
        );
        self.retained_layers.opacity_stats.rasterized += 1;
        Some(WindowLayer {
            owner: Some(owner),
            source: WindowLayerSource::Texture(image),
            bounds: raster.bounds,
            uv: [0.0, 0.0, 1.0, 1.0],
            opacity: 1.0,
            surface_only: true,
            mask: None,
            clip_samples: [false; 2],
            edge_clip: None,
        })
    }

    pub(crate) fn capture_window_viewport(
        &mut self,
        clip: Rect,
        scale: f32,
        reserved: &mut u64,
        paint: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Option<WindowLayer> {
        let physical = device_bounds(clip, scale);
        let bytes = (physical.width as u64)
            .saturating_mul(physical.height as u64)
            .saturating_mul(8);
        let available = self
            .retained_layers
            .budget
            .saturating_sub(self.retained_layers.resident_bytes())
            .saturating_sub(*reserved);
        if bytes > available {
            return None;
        }
        // Clip-aligned RGBA16Float preserves the pane's blended coverage.
        // These surfaces are owned by the current composition only.
        let captured = self
            .capture_device_texture_with_format(
                physical,
                scale,
                wgpu::TextureFormat::Rgba16Float,
                paint,
            )
            .ok()??;
        *reserved += captured.bytes;
        Some(WindowLayer {
            owner: None,
            source: WindowLayerSource::Texture(Rc::new(CachedLayerImage {
                texture: captured.texture,
                binding: OnceCell::new(),
            })),
            bounds: captured.bounds,
            uv: [0.0, 0.0, 1.0, 1.0],
            opacity: 1.0,
            surface_only: false,
            mask: None,
            clip_samples: [false; 2],
            edge_clip: None,
        })
    }

    pub(crate) fn window_clip_masks(
        &mut self,
        shapes: &[RoundedClip],
        scale: f32,
    ) -> Option<LayerClip> {
        #[cfg(xrack_femtovg_glyph_samples)]
        if let [outer, inner] = shapes {
            let mut outer_path = Path::new();
            let mut inner_path = Path::new();
            append_rect(&mut outer_path, outer.bounds, outer.radii);
            append_rect(&mut inner_path, inner.bounds, inner.radii);
            let intersection = outer.bounds.intersect(inner.bounds);
            let bounds = logical_bounds(device_bounds(intersection, scale), scale);
            let mut transform = femtovg::Transform2D::translation(-bounds.x, -bounds.y);
            transform.premultiply(&femtovg::Transform2D::scaling(scale, scale));
            match Canvas::<WGPURenderer>::rounded_clip_composition(
                &outer_path,
                &inner_path,
                transform,
                scale,
            ) {
                femtovg::RoundedClipComposition::Outer if intersection == outer.bounds => {
                    return self.window_clip_mask(std::slice::from_ref(outer), scale, false);
                }
                femtovg::RoundedClipComposition::Inner if intersection == inner.bounds => {
                    return self.window_clip_mask(std::slice::from_ref(inner), scale, false);
                }
                femtovg::RoundedClipComposition::Separate => {
                    let mut first =
                        self.window_clip_mask(std::slice::from_ref(outer), scale, false)?;
                    if let Some(second) =
                        self.window_clip_mask(std::slice::from_ref(inner), scale, true)
                    {
                        first.second = Some(LayerClipSample {
                            mask: second.mask,
                            origin: second.origin,
                        });
                        return Some(first);
                    }
                }
                _ => {}
            }
        }
        self.window_clip_mask(shapes, scale, false)
    }

    fn window_clip_mask(
        &mut self,
        shapes: &[RoundedClip],
        scale: f32,
        raster_only: bool,
    ) -> Option<LayerClip> {
        let bounds = shapes
            .iter()
            .map(|shape| shape.bounds)
            .reduce(Rect::intersect)?;
        let physical = device_bounds(bounds, scale);
        let origin = [physical.x as i32, physical.y as i32];
        // Moving a nested clip can alternate between analytic and raster
        // coverage. Keep the other representation of this exact pixel phase.
        if shapes.len() == 1
            && let Some(alternate) = self
                .retained_layers
                .window_clip_masks
                .iter()
                .find(|entry| entry.matches(shapes, physical, scale, !raster_only))
        {
            alternate
                .mask
                .last_frame
                .set(self.retained_layers.frame_number);
        }
        if let Some(entry) = self
            .retained_layers
            .window_clip_masks
            .iter()
            .find(|entry| entry.matches(shapes, physical, scale, raster_only))
        {
            return Some(LayerClip {
                mask: entry.mask.clone(),
                origin,
                second: None,
            });
        }
        let captured = self
            .capture_device_texture_with_format(
                physical,
                scale,
                wgpu::TextureFormat::R16Float,
                |renderer| {
                    for shape in shapes.iter() {
                        let mut path = Path::new();
                        append_rect(&mut path, shape.bounds, shape.radii);
                        #[cfg(xrack_femtovg_glyph_samples)]
                        if raster_only {
                            if !renderer.canvas.clip_path_as_mask(&path, FillRule::NonZero) {
                                return Err("raster clip exceeded the canvas mask budget".into());
                            }
                            continue;
                        }
                        renderer.canvas.clip_path(&path, FillRule::NonZero);
                    }
                    let mut fill = Path::new();
                    let extent = bounds.inset(-1.0 / scale);
                    fill.rect(extent.x, extent.y, extent.width, extent.height);
                    let mut paint = Paint::color(Color::white());
                    paint.set_anti_alias(false);
                    renderer.canvas.fill_path(&fill, &paint);
                    Ok(())
                },
            )
            .ok()??;
        let mask = Rc::new(WindowClipMask {
            texture: captured.texture,
            binding: OnceCell::new(),
            last_frame: Cell::new(self.retained_layers.frame_number),
        });
        self.retained_layers
            .window_clip_masks
            .push(RetainedClipMask {
                shapes: Rc::from(shapes),
                raster_only,
                scale,
                physical,
                mask: mask.clone(),
                bytes: captured.bytes,
            });
        Some(LayerClip {
            mask,
            origin,
            second: None,
        })
    }

    pub(crate) fn retain_composited_layers(&mut self, layers: &[WindowLayer]) {
        let atlas = self
            .retained_layers
            .opacity_atlas
            .as_ref()
            .and_then(|atlas| atlas.composited.get());
        let retained = self.retained_layers.window_opacity_atlas.as_ref();
        for item in layers {
            if let Some(atlas) = retained
                && item.owner.is_none()
                && matches!(&item.source, WindowLayerSource::Texture(source) if Rc::ptr_eq(source, &atlas.image))
            {
                self.retained_layers.opacity_atlas_stats.retained_hits += 1;
            }
            if let Some(image) = atlas
                && matches!(&item.source, WindowLayerSource::Texture(source) if Rc::ptr_eq(source, image))
            {
                self.retained_layers.opacity_atlas_stats.groups += 1;
            }
            if let Some(clip) = &item.mask {
                clip.mask.last_frame.set(self.retained_layers.frame_number);
                if let Some(second) = &clip.second {
                    second
                        .mask
                        .last_frame
                        .set(self.retained_layers.frame_number);
                }
            }
            let Some(owner) = &item.owner else {
                continue;
            };
            if let Some(layer) = self.retained_layers.opacity.get_mut(owner) {
                layer.used = true;
                layer.last_frame = self.retained_layers.frame_number;
            }
            self.retained_layers.changing_groups.remove(owner);
            self.retained_layers.opacity_stats.reused += 1;
        }
    }

    pub(crate) fn can_cache_on_main_canvas(&self) -> bool {
        !self.retained_layers.capturing && self.retained_layers.budget > 0
    }

    pub(crate) fn using_capture_canvas(&self) -> bool {
        self.retained_layers.capturing
    }

    pub(crate) fn paint_stack_layer(
        &mut self,
        page: &Rc<(Node, LayoutNode)>,
        bounds: Rect,
        offset: Point,
        frame: PaintFrame<'_>,
        cacheable: bool,
    ) -> Result<(), String> {
        let layer = if cacheable && self.can_retain_layer(frame.scale) {
            self.stack_layer_image(page, bounds, frame)?
        } else {
            None
        };
        self.canvas.translate(offset.x, offset.y);
        if let Some(raster) = layer {
            self.composite_layer(raster, 1.0, true);
            Ok(())
        } else {
            self.retained_layers.stats.direct += 1;
            self.paint(&page.0, &page.1, &mut Vec::new(), frame, false)
        }
    }

    pub(crate) fn paint_retained_opacity(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
        shadow: Option<&(shadow::OuterShadow, Rect)>,
    ) -> Result<bool, String> {
        if !self.can_retain_layer(frame.scale) || node.children.is_empty() && shadow.is_none() {
            return Ok(false);
        }
        let content = layout.clip.intersect(layout.bounds);
        let full = shadow.map_or(layout.bounds, |(shadow, _)| {
            shadow::union(layout.bounds, shadow.paint_bounds())
        });
        let hint = node.boolean("cache-rendering-hint", false)
            && shadow.is_none()
            && shadow::nonempty(full);
        let resample = hint
            && layout.clip.x <= full.x
            && layout.clip.y <= full.y
            && layout.clip.x + layout.clip.width >= full.x + full.width
            && layout.clip.y + layout.clip.height >= full.y + full.height;
        let clip = if resample {
            full
        } else {
            shadow.map_or(content, |(_, effect)| shadow::union(content, *effect))
        };
        let full_coverage = clip == full;
        let pixel_aligned = crate::pixel_aligned(clip, frame.scale);
        if shadow.is_some() && (!pixel_aligned || layout.has_ancestor_clip_chain()) {
            return Ok(false);
        }
        let owner = self.opacity_owner(path);
        let placement = LayerPlacement {
            bounds: layout.bounds,
            scale: frame.scale,
        };
        let track_phase = !resample
            && shadow.is_none()
            && node.opacity() < 1.0
            && !self.retained_layers.opacity_atlas_disabled
            && !cfg!(any(target_os = "ios", target_os = "android"));
        let exact_placement = track_phase
            && frame.scale >= 1.0
            && layout.has_ancestor_clip_chain()
            && layout.local_transform().is_identity()
            && layout.table_pane_clip().is_none();
        let exact_outer_clip = exact_placement && full_coverage;
        let style = OpacityTileStyle {
            scale: frame.scale,
            palette: frame.theme,
            in_layout,
        };
        let mut phase_changed = false;
        let mut repeated_phase_change = false;
        let mut cropped_sample = false;
        let mut discard_sample = false;
        let cached = self
            .retained_layers
            .opacity
            .get_mut(&owner)
            .and_then(|layer| {
                if !layer.matches_paint(node, style) {
                    return None;
                }
                let sampled = layer.resample_origin.is_some();
                let crop =
                    sampled && hint && !resample && can_crop_sampled_layer(layout, frame.scale);
                if sampled != resample && !crop {
                    return None;
                }
                let previous = layer.captured_layout.bounds;
                if layer.reuse_at(layout, clip, full_coverage, exact_outer_clip) {
                    layer.used = true;
                    layer.last_frame = self.retained_layers.frame_number;
                    layer.recaptured_at_new_phase = false;
                    cropped_sample = crop;
                    return Some(layer.raster());
                }
                discard_sample = sampled;
                phase_changed = track_phase
                    && previous.width == placement.bounds.width
                    && previous.height == placement.bounds.height
                    && !LayerPlacement {
                        bounds: previous,
                        scale: layer.scale,
                    }
                    .same_phase(placement);
                repeated_phase_change = phase_changed && layer.recaptured_at_new_phase;
                None
            });
        if discard_sample && let Some(layer) = self.retained_layers.opacity.remove(&owner) {
            layer.release_image(&mut self.canvas);
        }
        let cached = cached.or_else(|| {
            if !exact_outer_clip {
                return None;
            }
            let primary = self.retained_layers.opacity.get_mut(&owner)?;
            let layer = self.retained_layers.alternate_opacity.get_mut(&owner)?;
            if !layer.matches_paint(node, style)
                || !layer.reuse_at(layout, clip, full_coverage, true)
            {
                return None;
            }
            layer.used = true;
            layer.last_frame = self.retained_layers.frame_number;
            primary.used = true;
            primary.last_frame = self.retained_layers.frame_number;
            Some(layer.raster())
        });
        let raster = if let Some(raster) = cached {
            self.retained_layers.changing_groups.remove(&owner);
            self.retained_layers.opacity_stats.reused += 1;
            raster
        } else {
            let reuse_cropped = exact_placement && !full_coverage;
            if reuse_cropped && self.paint_retained_opacity_tile(node, layout, frame, in_layout) {
                self.retained_layers.changing_groups.remove(&owner);
                return Ok(true);
            }
            let defer_unstable = resample
                || shadow.is_some()
                || !cfg!(any(target_os = "ios", target_os = "android"));
            if !passive_content(node) {
                return Ok(false);
            }
            // A cold atlas can handle shadowed siblings in one pass. Admit
            // a persistent capture only if the group's paint stays stable.
            let defer_new_capture = shadow.is_some()
                && node.opacity() < 1.0
                && self.retained_layers.previous_opacity_demand.groups < 2
                && self.retained_layers.opacity_atlas.is_some()
                && !self.retained_layers.vectors.opacity_passthrough
                && transient_opacity_bounds(
                    node,
                    layout,
                    frame.scale,
                    self.opacity_atlas_limit(),
                    shadow,
                    false,
                )
                .is_some();
            if defer_unstable
                && !self.retained_layers.admit_stable_group(
                    &owner,
                    node,
                    placement,
                    repeated_phase_change,
                    defer_new_capture,
                )
            {
                if resample && let Some(layer) = self.retained_layers.opacity.remove(&owner) {
                    layer.release_image(&mut self.canvas);
                }
                let moving = track_phase
                    && self
                        .retained_layers
                        .changing_groups
                        .get(&owner)
                        .is_some_and(|group| group.moving_phase.is_some());
                if !moving {
                    return Ok(false);
                }
                if (!reuse_cropped
                    && self.paint_retained_opacity_tile(node, layout, frame, in_layout))
                    || self.paint_transient_opacity(
                        node,
                        layout,
                        path,
                        frame,
                        in_layout,
                        OpacitySource::MovingLayer,
                    )?
                {
                    return Ok(true);
                }
                // The atlas can be warming up or full. Preserve the
                // isolated layer's sampling in that frame as well.
            }
            if phase_changed && !repeated_phase_change {
                // A first phase change still gets a retained capture.
                // Its extent lets the shared atlas size the next frame
                // if several siblings keep changing phase.
                let device = device_bounds(clip, frame.scale);
                let [width, height] = [device.width as u32, device.height as u32];
                if opacity_tile_fits(width, height, self.opacity_atlas_limit()) {
                    self.retained_layers.opacity_demand.record(width, height);
                }
            }
            if let Some(old) = self.retained_layers.alternate_opacity.remove(&owner) {
                old.release_image(&mut self.canvas);
            }
            let keep_previous_phase =
                exact_outer_clip && phase_changed && self.retained_capture_slots() > 0 && {
                    let device = device_bounds(clip, frame.scale);
                    let bytes = device.width as u64 * device.height as u64 * 4;
                    self.retained_layers
                        .budget
                        .saturating_sub(self.retained_layers.resident_bytes())
                        >= bytes
                };
            if let Some(layer) = self.retained_layers.opacity.remove(&owner) {
                if keep_previous_phase
                    && layer.full_coverage
                    && layer.capture_clip == layer.captured_layout.bounds
                    && layer.captured_layout.has_ancestor_clip_chain()
                {
                    // The first phase change already needs a new capture.
                    // Keep the previous one for a later whole-pixel translation.
                    self.retained_layers
                        .alternate_opacity
                        .insert(owner.clone(), layer);
                } else {
                    layer.release_image(&mut self.canvas);
                }
            }
            let flags = if pixel_aligned && !resample {
                ImageFlags::NEAREST
            } else {
                ImageFlags::empty()
            };
            let mask = self.prepare_shadow_mask(shadow, layout, frame.scale, true)?;
            let paint = shadow.map(|(source, clip)| shadow::ShadowPaint {
                source,
                clip: *clip,
                mask,
            });
            // Include a clear border for bilinear sampling at fractional
            // positions and keep the original group clip inside the texture.
            let bounds = if resample {
                clip.inset(-1.0 / frame.scale)
            } else {
                clip
            };
            let raster_bounds = logical_bounds(device_bounds(bounds, frame.scale), frame.scale);
            let exact = !resample
                && shadow.is_none()
                && Self::can_composite_exact_opacity(node, layout, frame.scale, node.opacity());
            let origin = resample.then_some(Point {
                x: raster_bounds.x - layout.bounds.x,
                y: raster_bounds.y - layout.bounds.y,
            });
            // The ancestor shape clips the composed quad. Complete captures
            // of the same paint can share across whole-pixel translations.
            let share_exact = exact
                || exact_outer_clip
                    && full_coverage
                    && !node.contains_scene_feature(xrack_runtime::SceneFeature::SceneImageTexture);
            let share = if resample {
                self.retained_layers.hinted_layer_sharing
            } else {
                share_exact && self.retained_layers.opacity_tile_sharing
            };
            let fonts = self.text.registered_font_counts();
            let shared = (share && self.retained_layers.opacity.len() < MAX_LAYERS)
                .then(|| {
                    self.retained_layers
                        .opacity
                        .values()
                        .chain(self.retained_layers.alternate_opacity.values())
                        .find(|layer| {
                            (resample || layer.image.fonts == Some(fonts))
                                && layer.shares_capture(node, layout, style, raster_bounds, origin)
                        })
                        .map(|layer| layer.image.clone())
                })
                .flatten();
            let image = if let Some(image) = shared {
                self.retained_layers.opacity_stats.reused += 1;
                image
            } else {
                let Some(captured) = self.capture_texture(
                    bounds,
                    frame.scale,
                    wgpu::TextureFormat::Rgba8Unorm,
                    |renderer| {
                        renderer.paint_group(clip, 1.0, |renderer| {
                            renderer.paint_element_with_shadow(
                                node, layout, path, frame, in_layout, paint,
                            )
                        })
                    },
                )?
                else {
                    self.retained_layers.opacity_stats.direct += 1;
                    return Ok(false);
                };
                let Some(raster) = captured.register(&mut self.canvas, flags) else {
                    self.retained_layers.opacity_stats.direct += 1;
                    return Ok(false);
                };
                self.retained_layers.opacity_stats.rasterized += 1;
                // An isolated capture can fill a gap left by the temporary
                // atlas. Keep its exact crop when this paint already recurs.
                if reuse_cropped
                    && self.can_reuse_opacity_tiles(node, frame)
                    && fonts == self.text.registered_font_counts()
                    && self
                        .retained_layers
                        .has_retained_opacity_paint(node, layout, style, fonts)
                {
                    self.copy_retained_opacity(
                        &captured.texture,
                        [0, 0],
                        OpacityTile {
                            node: node.clone(),
                            layout: Box::new(layout.clone()),
                            clip,
                            device: device_bounds(clip, frame.scale),
                            position: [0, 0],
                            style,
                            capture: OpacityTileCapture {
                                moving: true,
                                mask: None,
                                fonts,
                            },
                        },
                    );
                }
                Rc::new(OpacityImage {
                    image: raster.image,
                    bytes: raster.bytes,
                    // At texel centers the compositor's linear sampler
                    // gives the same pixels as the exact retained layer.
                    cached: (resample || exact || exact_outer_clip).then(|| {
                        Rc::new(CachedLayerImage {
                            texture: captured.texture,
                            binding: OnceCell::new(),
                        })
                    }),
                    fonts: (share_exact && fonts == self.text.registered_font_counts())
                        .then_some(fonts),
                })
            };
            let raster = Raster {
                image: image.image,
                bytes: image.bytes,
                bounds: raster_bounds,
            };
            self.retained_layers.opacity.insert(
                owner,
                OpacityLayer {
                    node: Box::new(node.clone()),
                    captured_layout: Box::new(layout.clone()),
                    capture_clip: clip,
                    full_coverage,
                    image,
                    raster_bounds,
                    resample_origin: origin,
                    sampled_clips: resample.then(|| sampled_clip_regions(layout)),
                    scale: frame.scale,
                    palette: frame.theme,
                    in_layout,
                    surface_only: false,
                    used: true,
                    last_frame: self.retained_layers.frame_number,
                    recaptured_at_new_phase: phase_changed,
                },
            );
            raster
        };
        if cropped_sample {
            self.paint_group(layout.clip, 1.0, |renderer| {
                renderer.composite_layer(raster, node.opacity(), false);
                Ok(())
            })?;
        } else if resample || clip == raster.bounds && pixel_aligned {
            self.composite_layer(raster, node.opacity(), false);
        } else {
            self.paint_group(clip, 1.0, |renderer| {
                renderer.composite_layer(raster, node.opacity(), true);
                Ok(())
            })?;
        }
        Ok(true)
    }

    fn composite_layer(&mut self, raster: Raster, opacity: f32, antialias: bool) {
        let bounds = raster.bounds;
        let mut shape = Path::new();
        shape.rect(bounds.x, bounds.y, bounds.width, bounds.height);
        let mut paint = Paint::image(
            raster.image,
            bounds.x,
            bounds.y,
            bounds.width,
            bounds.height,
            0.0,
            opacity,
        );
        paint.set_anti_alias(antialias);
        self.canvas.fill_path(&shape, &paint);
    }

    fn opacity_atlas_limit(&self) -> u32 {
        let limit = self
            .device
            .limits()
            .max_texture_dimension_2d
            .min(MAX_OPACITY_ATLAS_EDGE);
        let extent = self.canvas.width().max(self.canvas.height()).min(limit);
        extent.next_power_of_two().max(256).min(limit)
    }

    pub(crate) fn prepare_opacity_atlas_for_children(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
    ) -> Result<(), String> {
        if cfg!(any(target_os = "ios", target_os = "android"))
            || self.retained_layers.opacity_atlas_disabled
            || self.retained_layers.opacity_atlas.is_some()
            || self.retained_layers.previous_opacity_demand.groups >= 2
            || layout.children.len() < 2
        {
            return Ok(());
        }
        let limit = self.opacity_atlas_limit();
        let mut demand = OpacityDemand::default();
        let mask_slots =
            if self.retained_layers.shadow_masks.is_empty() && self.can_retain_layer(frame.scale) {
                MAX_SHADOW_MASKS.min(self.retained_capture_slots())
            } else {
                0
            };
        let mut mask_keys = HashSet::new();
        let mut masks = Vec::new();
        for (_, child, item) in layout.children_in_paint_order(node) {
            let opacity = child.opacity();
            if !item.visible
                || !(0.0 < opacity && opacity < 1.0)
                || !item.local_transform().is_identity()
                || item.table_pane_clip().is_some()
                || frame.damage.is_some_and(|damage| {
                    !crate::window_damage::intersects_damage(child, item, frame, damage)
                })
            {
                continue;
            }
            let shadow = shadow::OuterShadow::clipped_for_node(
                child,
                item,
                frame.scale,
                frame.theme.role(crate::Role::Border),
            );
            let Some((clip, device)) =
                transient_opacity_bounds(child, item, frame.scale, limit, shadow.as_ref(), false)
            else {
                continue;
            };
            if shadow.is_none()
                && !(child.children.is_empty()
                    && (!crate::pixel_aligned(clip, frame.scale)
                        || !crate::pixel_aligned(item.bounds, frame.scale)))
            {
                continue;
            }
            demand.record(device.width as u32, device.height as u32);
            if masks.len() < mask_slots
                && let Some((source, _)) = shadow
                && let Some(tile) = source.initial_batch_tile(frame.scale, item.clip)
                && mask_keys.insert(tile.key)
            {
                masks.push((source, tile));
            }
        }
        if demand.groups >= 2 {
            // Allocate for these siblings before recording the first capture.
            self.create_opacity_atlas(demand.atlas_edge(0, 0, limit));
            if masks.len() > MAX_NEW_SHADOW_MASKS {
                self.capture_initial_shadow_masks(masks, frame.scale)?;
            }
        }
        Ok(())
    }

    fn capture_initial_shadow_masks(
        &mut self,
        masks: Vec<(shadow::OuterShadow, shadow::ShadowTile)>,
        scale: f32,
    ) -> Result<(), String> {
        let Some(atlas) = &self.retained_layers.opacity_atlas else {
            return Ok(());
        };
        let output = atlas.texture.clone();
        let mut budget = self
            .retained_layers
            .budget
            .saturating_sub(self.retained_layers.resident_bytes());
        let mut captures = Vec::with_capacity(masks.len());
        for (source, tile) in masks {
            let Some(texture) = CapturedTexture::allocate(
                &self.device,
                tile.source_pixels,
                scale,
                wgpu::TextureFormat::Rgba8Unorm,
                budget,
            ) else {
                continue;
            };
            budget -= texture.bytes;
            captures.push((source, tile, texture));
        }
        if captures.is_empty() {
            return Ok(());
        }
        let Some(registered) = self.with_capture_canvas(|renderer| {
            let canvas = &mut renderer.canvas;
            canvas.set_size(output.width(), output.height(), scale);
            let mut registered = Vec::with_capacity(captures.len());
            // Submit small chunks before the opacity atlas starts to sample
            // them. The GPU filters a chunk while the next one is recorded.
            for chunk in captures.chunks(MAX_NEW_SHADOW_MASKS) {
                let start = registered.len();
                for (source, tile, texture) in chunk {
                    // A FemtoVG image target stores rows bottom-up.
                    let flags = tile.sampling() | ImageFlags::FLIP_Y;
                    let Some(raster) = texture.register(canvas, flags) else {
                        continue;
                    };
                    canvas.set_render_target(RenderTarget::Image(raster.image));
                    canvas.reset();
                    canvas.clear_rect(
                        0,
                        0,
                        texture.texture.width(),
                        texture.texture.height(),
                        Color::rgba(0, 0, 0, 0),
                    );
                    canvas.scale(scale, scale);
                    source.draw_mask_into_image(canvas, *tile, scale, raster.image);
                    registered.push((tile.key, texture, raster.image, flags));
                }
                if registered.len() != start {
                    // The screen attachment receives no draws. Reuse the
                    // upcoming atlas as the output for this flush.
                    canvas.set_render_target(RenderTarget::Screen);
                    xrack_runtime::update_profile::submit_gpu();
                    renderer.queue.submit(canvas.flush_to_output(&output));
                }
            }
            Ok(registered)
        })?
        else {
            return Ok(());
        };
        for (key, texture, image, flags) in registered {
            let _ = self.cache_shadow_mask(key, texture, image, flags);
        }
        Ok(())
    }

    pub(crate) fn paint_transient_opacity(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
        source: OpacitySource<'_>,
    ) -> Result<bool, String> {
        if !has_device_scale(canvas_transform(&self.canvas), frame.scale) {
            return Ok(false);
        }
        let moving_layer = matches!(source, OpacitySource::MovingLayer);
        let Some((tile, clip)) =
            self.capture_opacity_tile(node, layout, path, frame, in_layout, source)?
        else {
            return Ok(false);
        };
        self.draw_opacity_tile(
            tile,
            clip,
            node.opacity(),
            moving_layer && !crate::pixel_aligned(clip, frame.scale),
        );
        Ok(true)
    }

    fn paint_retained_opacity_tile(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> bool {
        // The normal painter can replay the same retained pixels under
        // its live clip, even when the window cannot batch every draw.
        if let Some((raster, clip, position)) =
            self.lookup_retained_opacity(node, layout, frame, in_layout)
            && let Some(atlas) = self.retained_layers.window_opacity_atlas.as_mut()
        {
            let edge = atlas.image.texture.width();
            if atlas.canvas_image.is_none() {
                let info = ImageInfo::new(
                    ImageFlags::PREMULTIPLIED | ImageFlags::NEAREST,
                    edge as usize,
                    edge as usize,
                    PixelFormat::Rgba8,
                );
                atlas.canvas_image = self
                    .canvas
                    .create_image_from_native_texture(atlas.image.texture.clone(), info)
                    .ok();
            }
            if let Some(image) = atlas.canvas_image {
                let tile = OpacityTilePlacement::new(image, edge, frame.scale, raster, position);
                self.draw_opacity_tile(
                    tile,
                    clip,
                    node.opacity(),
                    !crate::pixel_aligned(clip, frame.scale),
                );
                self.retained_layers.opacity_atlas_stats.retained_hits += 1;
                return true;
            }
        }
        false
    }

    fn can_reuse_opacity_tiles(&self, node: &Node, frame: PaintFrame<'_>) -> bool {
        self.cached_layer_compositing
            && self.retained_layers.opacity_tile_sharing
            && !self.retained_layers.opacity_atlas_disabled
            && frame.damage.is_none()
            && !node.contains_scene_feature(xrack_runtime::SceneFeature::SceneImageTexture)
    }

    fn lookup_retained_opacity(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Option<(Rect, Rect, [u32; 2])> {
        let clip = layout.bounds.intersect(layout.clip);
        if !self.can_reuse_opacity_tiles(node, frame) || !shadow::nonempty(clip) {
            return None;
        }
        let device = device_bounds(clip, frame.scale);
        let style = OpacityTileStyle {
            scale: frame.scale,
            palette: frame.theme,
            in_layout,
        };
        let capture = OpacityTileCapture {
            moving: true,
            mask: None,
            fonts: self.text.registered_font_counts(),
        };
        if let Some(atlas) = self.retained_layers.window_opacity_atlas.as_mut()
            && let Some(position) = atlas.retained.iter_mut().find_map(|tile| {
                let position = tile.capture.reuse_retained(
                    node,
                    layout,
                    clip,
                    device,
                    style,
                    capture,
                    tile.can_crop,
                )?;
                tile.last_frame = self.retained_layers.frame_number;
                Some(position)
            })
        {
            atlas.last_frame = self.retained_layers.frame_number;
            return Some((logical_bounds(device, frame.scale), clip, position));
        }
        None
    }

    pub(crate) fn capture_composited_opacity(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
    ) -> Result<Option<WindowLayer>, String> {
        if let Some((raster, clip, position)) =
            self.lookup_retained_opacity(node, layout, frame, in_layout)
        {
            let Some(atlas) = &self.retained_layers.window_opacity_atlas else {
                return Ok(None);
            };
            return Ok(Some(Self::composited_opacity_layer(
                atlas.image.clone(),
                raster,
                clip,
                position,
                frame.scale,
                node.opacity(),
            )));
        }
        let Some((tile, clip)) = self.capture_opacity_tile(
            node,
            layout,
            path,
            frame,
            in_layout,
            OpacitySource::Composited,
        )?
        else {
            return Ok(None);
        };
        let Some(atlas) = self.retained_layers.opacity_atlas.as_ref() else {
            return Ok(None);
        };
        let image = atlas
            .composited
            .get_or_init(|| {
                Rc::new(CachedLayerImage {
                    texture: atlas.texture.clone(),
                    binding: OnceCell::new(),
                })
            })
            .clone();
        let position = [
            (tile.offset.x * tile.scale).round() as u32,
            (tile.offset.y * tile.scale).round() as u32,
        ];
        Ok(Some(Self::composited_opacity_layer(
            image,
            tile.raster,
            clip,
            position,
            frame.scale,
            node.opacity(),
        )))
    }

    fn composited_opacity_layer(
        image: Rc<CachedLayerImage>,
        bounds: Rect,
        clip: Rect,
        position: [u32; 2],
        scale: f32,
        opacity: f32,
    ) -> WindowLayer {
        let extent = image.texture.width() as f32;
        WindowLayer {
            owner: None,
            source: WindowLayerSource::Texture(image),
            bounds,
            uv: [
                position[0] as f32 / extent,
                position[1] as f32 / extent,
                (position[0] as f32 + bounds.width * scale) / extent,
                (position[1] as f32 + bounds.height * scale) / extent,
            ],
            opacity,
            surface_only: false,
            mask: None,
            clip_samples: [false; 2],
            edge_clip: (!crate::pixel_aligned(clip, scale)).then_some(clip),
        }
    }

    fn copy_retained_opacity(
        &mut self,
        source: &wgpu::Texture,
        origin: [u32; 2],
        mut tile: OpacityTile,
    ) {
        let cache = &mut self.retained_layers;
        let [width, height] = [tile.device.width as u32, tile.device.height as u32];
        if cache.window_opacity_atlas.is_none() {
            let limit = MAX_WINDOW_ATLAS_EDGE.min(self.device.limits().max_texture_dimension_2d);
            let budget = cache.budget.saturating_sub(cache.resident_bytes());
            let Some(edge) = [limit, MIN_WINDOW_ATLAS_EDGE].into_iter().find(|edge| {
                *edge <= limit
                    && opacity_tile_fits(width, height, *edge)
                    && WindowOpacityAtlas::bytes(*edge) <= budget
            }) else {
                return;
            };
            cache.window_opacity_atlas = Some(WindowOpacityAtlas::new(
                &self.device,
                edge,
                cache.frame_number,
            ));
        }
        let Some(atlas) = cache.window_opacity_atlas.as_mut() else {
            return;
        };
        let allocation = (atlas.retained.len() < MAX_LAYERS)
            .then(|| {
                atlas
                    .packing
                    .add_rect(width as usize, height as usize)
                    .map(|(x, y)| ([x as u32, y as u32], [width, height]))
                    .or_else(|| atlas.take_dead_slot(width, height, cache.frame_number))
            })
            .flatten();
        let (index, slot) = if let Some((origin, slot)) = allocation {
            tile.position = origin;
            (atlas.retained.len(), slot)
        } else {
            atlas.grow_next_frame |= atlas.retained.len() < MAX_LAYERS
                && atlas.image.texture.width() < MAX_OPACITY_ATLAS_EDGE
                && opacity_tile_fits(width, height, MAX_OPACITY_ATLAS_EDGE);
            // An earlier draw in this frame may still sample its tile.
            // Reuse only an older slot whose allocation contains the new tile.
            let Some((index, old)) = atlas
                .retained
                .iter()
                .enumerate()
                .filter(|(_, old)| {
                    old.last_frame != cache.frame_number
                        && old.slot[0] >= width
                        && old.slot[1] >= height
                })
                .max_by_key(|(_, old)| cache.frame_number.wrapping_sub(old.last_frame))
            else {
                return;
            };
            tile.position = old.capture.position;
            (index, old.slot)
        };
        let encoder = self.custom_encoder.get_or_insert_with(|| {
            self.device
                .create_command_encoder(&wgpu::CommandEncoderDescriptor {
                    label: Some("XRack opacity tile copies"),
                })
        });
        encoder.copy_texture_to_texture(
            wgpu::TexelCopyTextureInfo {
                texture: source,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: origin[0],
                    y: origin[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::TexelCopyTextureInfo {
                texture: &atlas.image.texture,
                mip_level: 0,
                origin: wgpu::Origin3d {
                    x: tile.position[0],
                    y: tile.position[1],
                    z: 0,
                },
                aspect: wgpu::TextureAspect::All,
            },
            wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
        );
        let can_crop = Self::crop_retained_opacity(&tile.node);
        let entry = RetainedOpacityTile {
            capture: tile,
            slot,
            last_frame: cache.frame_number,
            can_crop,
        };
        atlas.last_frame = cache.frame_number;
        if index == atlas.retained.len() {
            atlas.retained.push(entry);
        } else {
            atlas.retained[index] = entry;
        }
    }

    fn crop_retained_opacity(node: &Node) -> bool {
        matches!(
            item_paint(node),
            ItemPaint::Rectangle | ItemPaint::Text | ItemPaint::Label
        ) && node.color("drop-shadow-color").is_none()
            && node.color("inner-shadow-color").is_none()
            && node.children.iter().all(Self::crop_retained_opacity)
    }

    fn capture_opacity_tile(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
        source: OpacitySource<'_>,
    ) -> Result<Option<(OpacityTilePlacement, Rect)>, String> {
        let (shadow, moving_layer) = match source {
            OpacitySource::Transient(shadow) => (shadow, false),
            OpacitySource::MovingLayer | OpacitySource::Composited => (None, true),
        };
        if cfg!(any(target_os = "ios", target_os = "android"))
            || self.retained_layers.opacity_atlas_disabled
            || !self.can_cache_on_main_canvas()
            || self.retained_layers.vectors.opacity_passthrough
        {
            return Ok(None);
        }
        let limit = self.opacity_atlas_limit();
        // The retained painter handles the cache hint before this fallback.
        // Capture live content at its current phase and apply the ancestor's
        // shaped clip on the main canvas during replay.
        let clipped_transient = matches!(source, OpacitySource::Transient(None))
            && Self::can_capture_clipped_group(node, layout, frame.scale);
        let Some((clip, device)) = transient_opacity_bounds(
            node,
            layout,
            frame.scale,
            limit,
            shadow,
            moving_layer || clipped_transient,
        ) else {
            return Ok(None);
        };
        let raster = logical_bounds(device, frame.scale);
        let [width, height] = [device.width as u32, device.height as u32];
        self.retained_layers.opacity_demand.record(width, height);
        let demand = self.retained_layers.previous_opacity_demand;
        if demand.groups < 2
            && !(self.retained_layers.opacity_atlas.is_some()
                && (node.children.is_empty()
                    || shadow.is_some()
                    || clipped_transient
                    || matches!(source, OpacitySource::Composited)))
        {
            return Ok(None);
        }
        let mask = self.prepare_shadow_mask(shadow, layout, frame.scale, true)?;
        if shadow.is_some() && mask.is_none() {
            return Ok(None);
        }
        let desired_edge = demand.atlas_edge(width, height, limit);
        if self
            .retained_layers
            .opacity_atlas
            .as_ref()
            .is_none_or(|atlas| !atlas.cleared && atlas.texture.width() < desired_edge)
        {
            self.create_opacity_atlas(desired_edge);
        }
        let share_tiles = self.retained_layers.opacity_tile_sharing && frame.scale >= 1.0;
        let style = OpacityTileStyle {
            scale: frame.scale,
            palette: frame.theme,
            in_layout,
        };
        let capture = OpacityTileCapture {
            moving: moving_layer,
            mask,
            fonts: self.text.registered_font_counts(),
        };
        let Some(atlas) = self.retained_layers.opacity_atlas.as_mut() else {
            return Ok(None);
        };
        let edge = atlas.texture.width();
        let image = atlas.image;
        let shared = if share_tiles {
            atlas
                .tiles
                .iter()
                .find(|tile| tile.matches(node, layout, clip, device, style, capture))
                .map(|tile| tile.position)
        } else {
            None
        };
        let position = if let Some(position) = shared {
            self.retained_layers.opacity_atlas_stats.reused_tiles += 1;
            position
        } else {
            let Some((x, y)) = atlas.slots.add_rect(width as usize, height as usize) else {
                return Ok(None);
            };
            [x as u32, y as u32]
        };
        atlas.last_frame = self.retained_layers.frame_number;
        let tile = OpacityTilePlacement::new(image, edge, frame.scale, raster, position);
        if shared.is_none() {
            let paint = shadow.map(|(source, clip)| shadow::ShadowPaint {
                source,
                clip: *clip,
                mask,
            });
            let capture_clip = if moving_layer { clip } else { raster };
            if !self.record_opacity_tile(tile, capture_clip, |renderer| {
                renderer.paint_element_with_shadow(node, layout, path, frame, in_layout, paint)
            })? {
                return Ok(None);
            }
            if share_tiles
                && capture.fonts == self.text.registered_font_counts()
                && let Some(atlas) = &mut self.retained_layers.opacity_atlas
                && atlas.image == image
                && atlas.tiles.len() < MAX_SHARED_OPACITY_TILES
            {
                atlas.tiles.push(OpacityTile {
                    node: node.clone(),
                    layout: Box::new(layout.clone()),
                    clip,
                    device,
                    position,
                    style,
                    capture,
                });
            }
        }
        if moving_layer
            && share_tiles
            && self.can_reuse_opacity_tiles(node, frame)
            && capture.fonts == self.text.registered_font_counts()
            && let Some(atlas) = self.retained_layers.opacity_atlas.as_ref()
            && (atlas
                .previous_tiles
                .iter()
                .any(|tile| tile.same_content(node, layout, style, capture.fonts))
                || self.retained_layers.has_retained_opacity_paint(
                    node,
                    layout,
                    style,
                    capture.fonts,
                ))
        {
            let source = atlas.texture.clone();
            self.copy_retained_opacity(
                &source,
                position,
                OpacityTile {
                    node: node.clone(),
                    layout: Box::new(layout.clone()),
                    clip,
                    device,
                    position,
                    style,
                    capture,
                },
            );
        }
        Ok(Some((tile, clip)))
    }

    pub(crate) fn plan_opacity_atlas_for_rectangles(
        &self,
        bounds: impl Iterator<Item = Rect>,
        scale: f32,
    ) -> Option<RectangleOpacityAtlasPlan> {
        let limit = self.opacity_atlas_limit();
        let mut demand = OpacityDemand::default();
        for bounds in bounds {
            let raster = device_bounds(bounds, scale);
            let (width, height) = (raster.width as u32, raster.height as u32);
            if opacity_tile_fits(width, height, limit) {
                demand.record(width, height);
            }
        }
        if demand.groups < 2 {
            return None;
        }
        let edge = demand.atlas_edge(0, 0, limit);
        let pixels = u64::from(edge) * u64::from(edge);
        let atlas = self.retained_layers.opacity_atlas.as_ref();
        let available = atlas.is_none_or(|atlas| !atlas.cleared);
        Some(RectangleOpacityAtlasPlan {
            edge,
            covers_batch: demand.pixels <= pixels
                && available
                && (atlas.is_some_and(|atlas| atlas.texture.width() >= edge)
                    || pixels * 4 <= self.opacity_atlas_replacement_budget()),
        })
    }

    pub(crate) fn can_capture_transient_rectangles(&self, scale: f32) -> bool {
        scale >= 1.0
            && !cfg!(any(target_os = "ios", target_os = "android"))
            && !self.retained_layers.opacity_atlas_disabled
            && self.can_retain_layer(scale)
            && !self.retained_layers.vectors.opacity_passthrough
    }

    fn reserve_transient_rectangle(
        &mut self,
        rectangle: crate::rectangle::Solid,
        scale: f32,
    ) -> Option<OpacityRectangle> {
        let (clip, opacity) = rectangle.opacity_capture();
        let device = device_bounds(clip, scale);
        let [width, height] = [device.width as u32, device.height as u32];
        let Some(atlas) = self.retained_layers.opacity_atlas.as_mut() else {
            return None;
        };
        let edge = atlas.texture.width();
        if !opacity_tile_fits(width, height, edge) {
            return None;
        }
        self.retained_layers.opacity_demand.record(width, height);
        let Some((x, y)) = atlas.slots.add_rect(width as usize, height as usize) else {
            return None;
        };
        let raster = logical_bounds(device, scale);
        let tile =
            OpacityTilePlacement::new(atlas.image, edge, scale, raster, [x as u32, y as u32]);
        atlas.last_frame = self.retained_layers.frame_number;
        Some(OpacityRectangle {
            tile,
            rectangle,
            clip,
            opacity,
        })
    }

    pub(crate) fn draw_transient_rectangles(
        &mut self,
        rectangles: &mut Vec<crate::rectangle::Solid>,
        scale: f32,
        plan: Option<&RectangleOpacityAtlasPlan>,
    ) -> Result<(), String> {
        if rectangles.is_empty() {
            return Ok(());
        }
        #[cfg(xrack_femtovg_glyph_samples)]
        if self.direct_rounded_fills_enabled() && rectangles.len() > 1 {
            let as_array = |bounds: Rect| [bounds.x, bounds.y, bounds.width, bounds.height];
            let paths = rectangles.iter().map(|&rectangle| {
                let (path, color) = rectangle.unfaded_path();
                let (clip, opacity) = rectangle.opacity_capture();
                let raster = logical_bounds(device_bounds(clip, scale), scale);
                femtovg::renderer::DirectOpacityPath {
                    path,
                    color,
                    clip: as_array(clip),
                    raster: as_array(raster),
                    opacity,
                }
            });
            if let Some(uploaded) = self.canvas.draw_direct_opacity_paths(paths) {
                xrack_runtime::update_profile::geometry_upload(uploaded);
                rectangles.clear();
                return Ok(());
            }
        }
        if let Some(plan) = plan
            && self
                .retained_layers
                .opacity_atlas
                .as_ref()
                .is_none_or(|atlas| !atlas.cleared && atlas.texture.width() < plan.edge)
        {
            self.create_opacity_atlas(plan.edge);
        }
        let mut tiles = Vec::new();
        for rectangle in rectangles.drain(..) {
            if let Some(tile) = self.reserve_transient_rectangle(rectangle, scale) {
                tiles.push(tile);
                continue;
            }
            self.draw_transient_rectangle_tiles(&mut tiles)?;
            rectangle.draw(&mut self.canvas);
        }
        self.draw_transient_rectangle_tiles(&mut tiles)
    }

    fn draw_transient_rectangle_tiles(
        &mut self,
        rectangles: &mut Vec<OpacityRectangle>,
    ) -> Result<(), String> {
        let Some(first) = rectangles.first().map(|rectangle| rectangle.tile) else {
            return Ok(());
        };
        if !self.record_opacity_tiles(first, |renderer| {
            for rectangle in rectangles.iter() {
                let tile = rectangle.tile;
                renderer.position_capture_tile(tile);
                let clip = tile.raster;
                renderer
                    .canvas
                    .scissor(clip.x, clip.y, clip.width, clip.height);
                rectangle.rectangle.draw_unfaded(&mut renderer.canvas);
            }
            Ok(())
        })? {
            for rectangle in rectangles.drain(..) {
                rectangle.rectangle.draw(&mut self.canvas);
            }
            return Ok(());
        }
        #[cfg(xrack_femtovg_glyph_samples)]
        if rectangles.len() > 1 {
            let image = rectangles[0].tile.image;
            if rectangles
                .iter()
                .all(|rectangle| rectangle.tile.image == image)
            {
                let rect = |bounds: Rect| [bounds.x, bounds.y, bounds.width, bounds.height];
                let tiles: Vec<_> = rectangles
                    .iter()
                    .map(|rectangle| {
                        let tile = rectangle.tile;
                        let extent = tile.edge as f32 / tile.scale;
                        femtovg::renderer::OpacityTile {
                            bounds: rect(tile.raster),
                            clip: rect(rectangle.clip),
                            mapping: [
                                tile.raster.x - tile.offset.x,
                                tile.raster.y - tile.offset.y,
                                extent,
                                extent,
                            ],
                            opacity: rectangle.opacity,
                        }
                    })
                    .collect();
                if let Some(upload) = self.canvas.draw_opacity_tiles(image, &tiles) {
                    if upload != 0 {
                        xrack_runtime::update_profile::quad_batch_upload(upload);
                    }
                    self.retained_layers.opacity_atlas_stats.groups += tiles.len() as u64;
                    rectangles.clear();
                    return Ok(());
                }
            }
        }
        for rectangle in rectangles.drain(..) {
            self.draw_opacity_tile(rectangle.tile, rectangle.clip, rectangle.opacity, false);
        }
        Ok(())
    }

    fn record_opacity_tile(
        &mut self,
        tile: OpacityTilePlacement,
        capture_clip: Rect,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<bool, String> {
        self.record_opacity_tiles(tile, |renderer| {
            renderer.position_capture_tile(tile);
            renderer.paint_group(capture_clip, 1.0, draw)
        })
    }

    fn position_capture_tile(&mut self, tile: OpacityTilePlacement) {
        self.canvas.reset();
        self.canvas.scale(tile.scale, tile.scale);
        self.canvas
            .translate(tile.offset.x - tile.raster.x, tile.offset.y - tile.raster.y);
    }

    fn record_opacity_tiles(
        &mut self,
        tile: OpacityTilePlacement,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<bool, String> {
        let Some(atlas) = self.retained_layers.opacity_atlas.as_mut() else {
            return Ok(false);
        };
        let was_pending = atlas.pending;
        let clear = !atlas.cleared;
        atlas.pending = true;
        atlas.cleared = true;
        let recorded = self.with_capture_canvas(|renderer| {
            if !was_pending {
                renderer.canvas.set_size(tile.edge, tile.edge, tile.scale);
                renderer.images.begin_frame();
            }
            if clear {
                renderer.canvas.reset();
                renderer
                    .canvas
                    .clear_rect(0, 0, tile.edge, tile.edge, Color::rgba(0, 0, 0, 0));
            }
            draw(renderer)
        });
        match recorded {
            Err(error) => {
                self.submit_opacity_atlas();
                Err(error)
            }
            Ok(None) => {
                if let Some(atlas) = &mut self.retained_layers.opacity_atlas {
                    atlas.pending = was_pending;
                    atlas.cleared = !clear;
                }
                Ok(false)
            }
            Ok(Some(())) => Ok(true),
        }
    }

    fn draw_opacity_tile(
        &mut self,
        tile: OpacityTilePlacement,
        clip: Rect,
        opacity: f32,
        antialias: bool,
    ) {
        let raster = tile.raster;
        let mut path = Path::new();
        path.rect(raster.x, raster.y, raster.width, raster.height);
        let extent = tile.edge as f32 / tile.scale;
        let mut paint = Paint::image(
            tile.image,
            raster.x - tile.offset.x,
            raster.y - tile.offset.y,
            extent,
            extent,
            0.0,
            opacity,
        );
        paint.set_anti_alias(antialias);
        if raster == clip {
            self.canvas.fill_path(&path, &paint);
        } else {
            self.canvas.save();
            self.canvas
                .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
            self.canvas.fill_path(&path, &paint);
            self.canvas.restore();
        }
        if self
            .retained_layers
            .opacity_atlas
            .as_ref()
            .is_some_and(|atlas| atlas.image == tile.image)
        {
            self.retained_layers.opacity_atlas_stats.groups += 1;
        }
    }

    fn opacity_atlas_replacement_budget(&self) -> u64 {
        let previous = self
            .retained_layers
            .opacity_atlas
            .as_ref()
            .map_or(0, |atlas| {
                u64::from(atlas.texture.width()) * u64::from(atlas.texture.height()) * 4
            });
        let used = self.retained_layers.resident_bytes() - previous;
        self.retained_layers.budget.saturating_sub(used)
    }

    fn create_opacity_atlas(&mut self, edge: u32) {
        let bytes = u64::from(edge) * u64::from(edge) * 4;
        if bytes > self.opacity_atlas_replacement_budget() {
            return;
        }
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack transient opacity atlas"),
            size: wgpu::Extent3d {
                width: edge,
                height: edge,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_SRC,
            view_formats: &[],
        });
        let edge = edge as usize;
        let info = ImageInfo::new(
            ImageFlags::PREMULTIPLIED | ImageFlags::NEAREST,
            edge,
            edge,
            PixelFormat::Rgba8,
        );
        let Ok(image) = self
            .canvas
            .create_image_from_native_texture(texture.clone(), info)
        else {
            return;
        };
        self.retained_layers.clear_opacity_atlas(&mut self.canvas);
        self.retained_layers.opacity_atlas = Some(OpacityAtlas {
            image,
            texture,
            slots: Atlas::new(edge, edge),
            tiles: Vec::new(),
            previous_tiles: Vec::new(),
            composited: OnceCell::new(),
            pending: false,
            cleared: false,
            last_frame: self.retained_layers.frame_number,
        });
    }

    pub(crate) fn submit_opacity_atlas(&mut self) {
        if let Some(opacity) = self.take_opacity_atlas_commands() {
            xrack_runtime::update_profile::submit_gpu();
            let rounded = self.rectangle_batches.take_rounded_commands();
            self.queue.submit(rounded.into_iter().chain([opacity]));
        }
    }

    pub(crate) fn take_opacity_atlas_commands(&mut self) -> Option<wgpu::CommandBuffer> {
        let atlas = self.retained_layers.opacity_atlas.as_mut()?;
        if !atlas.pending {
            return None;
        }
        let (canvas, images) = self.retained_layers.capture.as_mut()?;
        atlas.pending = false;
        #[cfg(xrack_femtovg_glyph_samples)]
        canvas.batch_opacity_atlas_fills();
        let commands = canvas.flush_to_output(&atlas.texture);
        images.end_frame(canvas);
        self.retained_layers.opacity_atlas_stats.submissions += 1;
        commands
    }

    fn stack_layer_image(
        &mut self,
        page: &Rc<(Node, LayoutNode)>,
        bounds: Rect,
        frame: PaintFrame<'_>,
    ) -> Result<Option<Raster>, String> {
        let page_ref = Rc::downgrade(page);
        if let Some(layer) = self.retained_layers.layers.iter().find(|layer| {
            layer.page.ptr_eq(&page_ref)
                && layer.bounds == bounds
                && layer.scale == frame.scale
                && layer.palette == frame.theme
        }) {
            self.retained_layers.stats.reused += 1;
            return Ok(Some(layer.raster));
        }
        let Some(raster) = self.capture_layer(
            bounds,
            frame.scale,
            wgpu::TextureFormat::Rgba8Unorm,
            ImageFlags::empty(),
            |renderer| renderer.paint(&page.0, &page.1, &mut Vec::new(), frame, false),
        )?
        else {
            return Ok(None);
        };
        self.retained_layers.layers.push(Layer {
            page: page_ref,
            raster,
            bounds,
            scale: frame.scale,
            palette: frame.theme,
        });
        self.retained_layers.stats.rasterized += 1;
        Ok(Some(raster))
    }

    pub(crate) fn capture_layer(
        &mut self,
        bounds: Rect,
        scale: f32,
        format: wgpu::TextureFormat,
        flags: ImageFlags,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Option<Raster>, String> {
        Ok(self
            .capture_texture(bounds, scale, format, draw)?
            .and_then(|texture| texture.register(&mut self.canvas, flags)))
    }

    fn capture_texture(
        &mut self,
        bounds: Rect,
        scale: f32,
        format: wgpu::TextureFormat,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Option<CapturedTexture>, String> {
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Ok(None);
        }
        self.capture_device_texture_with_format(device_bounds(bounds, scale), scale, format, draw)
    }

    fn capture_device_texture(
        &mut self,
        physical: Rect,
        scale: f32,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Option<CapturedTexture>, String> {
        self.capture_device_texture_with_format(
            physical,
            scale,
            wgpu::TextureFormat::Rgba8Unorm,
            draw,
        )
    }

    fn capture_device_texture_with_format(
        &mut self,
        physical: Rect,
        scale: f32,
        format: wgpu::TextureFormat,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Option<CapturedTexture>, String> {
        if self.retained_capture_slots() == 0 {
            return Ok(None);
        }
        let budget = self
            .retained_layers
            .budget
            .saturating_sub(self.retained_layers.resident_bytes());
        let Some(texture) =
            CapturedTexture::allocate(&self.device, physical, scale, format, budget)
        else {
            return Ok(None);
        };
        self.submit_opacity_atlas();
        let rendered = self.paint_capture_target(&texture.texture, texture.bounds, scale, draw)?;
        Ok(rendered.map(|()| texture))
    }

    fn retained_capture_slots(&self) -> usize {
        MAX_LAYERS.saturating_sub(
            self.retained_layers.layers.len()
                + self.retained_layers.opacity.len()
                + self.retained_layers.alternate_opacity.len()
                + self.retained_layers.shadow_masks.len()
                + self.retained_layers.window_clip_masks.len(),
        )
    }

    pub(crate) fn capture_scene_item(
        &mut self,
        bounds: Rect,
        scale: f32,
        previous: Option<&wgpu::Texture>,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Option<wgpu::Texture>, String> {
        let physical = device_bounds(bounds, scale);
        if let Some(texture) = previous.filter(|texture| {
            texture.width() == physical.width as u32 && texture.height() == physical.height as u32
        }) {
            return Ok(self
                .paint_capture_target(texture, logical_bounds(physical, scale), scale, draw)?
                .map(|()| texture.clone()));
        }
        Ok(self
            .capture_device_texture(physical, scale, draw)?
            .map(|captured| captured.texture))
    }

    fn paint_capture_target(
        &mut self,
        texture: &wgpu::Texture,
        bounds: Rect,
        scale: f32,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Option<()>, String> {
        let (width, height) = (texture.width(), texture.height());
        self.with_capture_canvas(|renderer| {
            renderer.canvas.set_size(width, height, scale);
            renderer.canvas.reset();
            renderer
                .canvas
                .clear_rect(0, 0, width, height, Color::rgba(0, 0, 0, 0));
            renderer.canvas.scale(scale, scale);
            renderer.canvas.translate(-bounds.x, -bounds.y);
            renderer.images.begin_frame();
            let result = draw(renderer);
            renderer.submit_canvas(texture);
            renderer.images.end_frame(&mut renderer.canvas);
            result
        })
    }

    fn with_capture_canvas<T>(
        &mut self,
        draw: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        if self.retained_layers.capturing {
            return Ok(None);
        }
        if self.retained_layers.capture.is_none() {
            let Ok(canvas) = self.create_auxiliary_canvas() else {
                return Ok(None);
            };
            self.retained_layers.capture = Some((canvas, Default::default()));
        }
        let Some(mut capture) = self.retained_layers.capture.take() else {
            return Ok(None);
        };
        self.retained_layers.capturing = true;
        std::mem::swap(&mut self.canvas, &mut capture.0);
        std::mem::swap(&mut self.images, &mut capture.1);
        let inline_target = self.inline_target.take();
        let result = draw(self);
        self.inline_target = inline_target;
        std::mem::swap(&mut self.canvas, &mut capture.0);
        std::mem::swap(&mut self.images, &mut capture.1);
        self.retained_layers.capture = Some(capture);
        self.retained_layers.capturing = false;
        result.map(Some)
    }
}

fn fractional_window_surface(layout: &LayoutNode) -> bool {
    crate::display_list::fractional_window_composition() && layout.has_ancestor_clip_chain()
}

fn same_window_surface_layout(before: &LayoutNode, after: &LayoutNode) -> bool {
    before.enabled == after.enabled
        && before.layout_direction_override() == after.layout_direction_override()
        && before.world_transform().max_scale() == after.world_transform().max_scale()
}

fn transient_opacity_bounds(
    node: &Node,
    layout: &LayoutNode,
    scale: f32,
    limit: u32,
    shadow: Option<&(shadow::OuterShadow, Rect)>,
    composite_under_ancestor_clip: bool,
) -> Option<(Rect, Rect)> {
    // A clipped group can capture the rectangular source and composite on
    // the main canvas, where the ancestor's shaped clip is still active.
    if layout.allows_overflow()
        || (!composite_under_ancestor_clip && layout.has_ancestor_clip_chain())
        || node.children.is_empty() && !crate::rectangle::single_fill(node, layout)
        || !passive_content(node)
    {
        return None;
    }
    let content = layout.clip.intersect(layout.bounds);
    let clip = shadow.map_or(content, |(_, effect)| shadow::union(content, *effect));
    if !shadow::nonempty(clip) {
        return None;
    }
    let device = device_bounds(clip, scale);
    (opacity_tile_fits(device.width as u32, device.height as u32, limit)
        && shadow.is_none_or(|(shadow, _)| shadow.mask_tile(scale, layout.clip).is_some()))
    .then_some((clip, device))
}

fn opacity_tile_fits(width: u32, height: u32, limit: u32) -> bool {
    width <= limit
        && height <= limit
        && u64::from(width) * u64::from(height) <= u64::from(limit) * u64::from(limit) / 4
}

pub(crate) fn passive_content(node: &Node) -> bool {
    matches!(
        item_paint(node),
        ItemPaint::Rectangle
            | ItemPaint::Text
            | ItemPaint::Label
            | ItemPaint::Vector
            | ItemPaint::Image { animated: false }
            | ItemPaint::Container(
                ContainerPaint::Horizontal
                    | ContainerPaint::Vertical
                    | ContainerPaint::Grid
                    | ContainerPaint::Row
                    | ContainerPaint::Column
            )
    ) && node.children.iter().all(passive_content)
}
