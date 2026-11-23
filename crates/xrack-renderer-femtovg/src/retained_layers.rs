use super::{
    Canvas, Color, LayoutNode, Node, Paint, PaintFrame, Palette, Path, Point, Rect, Renderer,
    WGPURenderer, append_rect, canvas_transform, has_device_scale, image, shadow, vector_masks,
};
use femtovg::{Atlas, FillRule, ImageFlags, ImageId, ImageInfo, PixelFormat};
use std::{
    cell::{Cell, OnceCell},
    collections::{BTreeMap, HashMap, HashSet},
    rc::{Rc, Weak},
};
use xrack_layout::RoundedClip;
use xrack_runtime::{SceneIndex, SceneNodeId};

const MAX_LAYERS: usize = 64;
const MAX_SHADOW_MASKS: usize = 24;
const MAX_NEW_SHADOW_MASKS: usize = 4;
const SHADOW_MASK_LIFETIME: u64 = 60;
const MAX_OPACITY_ATLAS_EDGE: u32 = 1536;
const OPACITY_ATLAS_LIFETIME: u64 = 60;
const MAX_SHARED_OPACITY_TILES: usize = 32;
const WINDOW_ATLAS_EDGE: u32 = 512;
const WINDOW_ATLAS_BYTES: u64 = WINDOW_ATLAS_EDGE as u64 * WINDOW_ATLAS_EDGE as u64 * 4;

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
}

pub(super) struct CachedLayerImage {
    pub(super) texture: wgpu::Texture,
    pub(super) binding: OnceCell<wgpu::BindGroup>,
}

struct WindowOpacityAtlas {
    image: Rc<CachedLayerImage>,
    packing: Atlas,
    tiles: HashMap<*const CachedLayerImage, WindowAtlasTile>,
    last_frame: u64,
}

struct WindowAtlasTile {
    source: Weak<CachedLayerImage>,
    origin: [u32; 2],
}

impl WindowOpacityAtlas {
    fn new(device: &wgpu::Device, frame: u64) -> Self {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack compositor opacity atlas"),
            size: wgpu::Extent3d {
                width: WINDOW_ATLAS_EDGE,
                height: WINDOW_ATLAS_EDGE,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::TEXTURE_BINDING | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        Self {
            image: Rc::new(CachedLayerImage {
                texture,
                binding: OnceCell::new(),
            }),
            packing: Atlas::new(WINDOW_ATLAS_EDGE as usize, WINDOW_ATLAS_EDGE as usize),
            tiles: HashMap::new(),
            last_frame: frame,
        }
    }

    fn tile(
        &mut self,
        source: &Rc<CachedLayerImage>,
        device: &wgpu::Device,
        encoder: &mut Option<wgpu::CommandEncoder>,
    ) -> Option<[u32; 2]> {
        let key = Rc::as_ptr(source);
        if let Some(tile) = self.tiles.get(&key) {
            return Some(tile.origin);
        }
        if self.tiles.len() >= MAX_LAYERS {
            return None;
        }
        let width = source.texture.width();
        let height = source.texture.height();
        let (x, y) = self
            .packing
            .add_rect((width + 2) as usize, (height + 2) as usize)?;
        let origin = [x as u32 + 1, y as u32 + 1];
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
            },
        );
        Some(origin)
    }
}

pub(super) enum WindowLayerSource {
    Texture(Rc<CachedLayerImage>),
    TintedAlpha {
        image: Rc<CachedLayerImage>,
        tint: u32,
    },
    Solid(Color),
}

pub(super) struct WindowLayer {
    owner: Option<LayerOwner>,
    pub(super) source: WindowLayerSource,
    pub(super) bounds: Rect,
    pub(super) uv: [f32; 4],
    pub(super) opacity: f32,
    pub(super) surface_only: bool,
    pub(super) mask: Option<LayerClip>,
    pub(super) sample_mask: bool,
}

#[derive(Clone)]
pub(super) struct LayerClip {
    pub(super) mask: Rc<WindowClipMask>,
    pub(super) origin: [i32; 2],
}

pub(super) struct WindowClipMask {
    pub(super) texture: wgpu::Texture,
    pub(super) binding: OnceCell<wgpu::BindGroup>,
    last_frame: Cell<u64>,
}

struct RetainedClipMask {
    shapes: Rc<[RoundedClip]>,
    scale: f32,
    physical: Rect,
    mask: Rc<WindowClipMask>,
    bytes: u64,
}

impl RetainedClipMask {
    fn matches(&self, shapes: &[RoundedClip], physical: Rect, scale: f32) -> bool {
        if self.scale != scale
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
    pub(super) fn tinted_alpha(
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
            sample_mask: false,
        }
    }

    pub(super) fn solid(bounds: Rect, color: Color) -> Self {
        Self {
            owner: None,
            source: WindowLayerSource::Solid(color),
            bounds,
            uv: [0.0; 4],
            opacity: 1.0,
            surface_only: true,
            mask: None,
            sample_mask: false,
        }
    }

    pub(super) fn crop(&mut self, clip: Rect) {
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

    fn shares_hint(
        &self,
        node: &Node,
        layout: &LayoutNode,
        style: OpacityTileStyle,
        bounds: Rect,
    ) -> bool {
        // The source may have moved since capture. Its original sampling
        // phase is stored relative to its own layout origin.
        let origin = Point {
            x: bounds.x - layout.bounds.x,
            y: bounds.y - layout.bounds.y,
        };
        if self.resample_origin != Some(origin)
            || self.scale != style.scale
            || self.palette != style.palette
            || self.in_layout != style.in_layout
            || self.raster_bounds.width != bounds.width
            || self.raster_bounds.height != bounds.height
            || !self.node.same_paint_except_layer_composition(node)
        {
            return false;
        }
        let offset = Point {
            x: layout.bounds.x - self.captured_layout.bounds.x,
            y: layout.bounds.y - self.captured_layout.bounds.y,
        };
        let mut motion = LayoutMove::new(self.capture_clip, layout.bounds, offset, 0.0);
        self.captured_layout
            .matches_translated(layout, true, |before, after| {
                translated_geometry_matches(before, after, &mut motion)
            })
    }

    fn reuse_at(&mut self, layout: &LayoutNode, clip: Rect, full_coverage: bool) -> bool {
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
        if !self.full_coverage
            || !full_coverage
                && (self.resample_origin.is_none() || !can_crop_sampled_layer(layout, self.scale))
            || self.resample_origin.is_none()
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
            self.resample_origin.is_some(),
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

pub(super) fn device_bounds(bounds: Rect, scale: f32) -> Rect {
    let left = (bounds.x * scale).floor();
    let top = (bounds.y * scale).floor();
    Rect::new(
        left,
        top,
        ((bounds.x + bounds.width) * scale).ceil() - left,
        ((bounds.y + bounds.height) * scale).ceil() - top,
    )
}

pub(super) fn logical_bounds(bounds: Rect, scale: f32) -> Rect {
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
        translated(previous.clip.intersect(motion.previous), motion.offset)
    };
    let next_clip = next.clip.intersect(motion.next);
    equal(translated(previous.bounds, motion.offset), next.bounds)
        && (equal(clip, next_clip)
            || motion.sampled_clips.is_some()
                && !shadow::nonempty(clip)
                && !shadow::nonempty(next_clip))
}

#[derive(Clone, Copy)]
pub(super) struct Raster {
    pub(super) image: ImageId,
    bounds: Rect,
    bytes: u64,
}

struct CapturedTexture {
    texture: wgpu::Texture,
    bounds: Rect,
    bytes: u64,
}

impl CapturedTexture {
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

struct OpacityAtlas {
    image: ImageId,
    texture: wgpu::Texture,
    slots: Atlas,
    tiles: Vec<OpacityTile>,
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
}

impl OpacityTile {
    fn matches(
        &self,
        node: &Node,
        layout: &LayoutNode,
        clip: Rect,
        device: Rect,
        style: OpacityTileStyle,
    ) -> bool {
        if self.style != style
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
        if offset.x * style.scale != device.x - self.device.x
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
            .matches_translated(layout, false, |before, after| {
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
}

struct ChangingGroup {
    node: Box<Node>,
    last_frame: u64,
    moving_phase: Option<LayerPlacement>,
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
        // Both placements must be eligible for capture, with device-aligned bounds.
        if self.scale != other.scale {
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

pub(super) enum OpacitySource<'a> {
    Transient(Option<&'a (shadow::OuterShadow, Rect)>),
    MovingLayer,
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

pub(super) struct RetainedLayers {
    layers: Vec<Layer>,
    opacity: BTreeMap<LayerOwner, OpacityLayer>,
    opacity_stats: LayerStats,
    pub(super) vectors: vector_masks::Masks,
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
    pub(super) fn reset_canvas(&mut self) {
        self.layers.clear();
        self.opacity.clear();
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

    pub(super) fn begin_frame(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.frame_number = self.frame_number.wrapping_add(1);
        if self.window_opacity_atlas.as_ref().is_some_and(|atlas| {
            self.frame_number.wrapping_sub(atlas.last_frame) > OPACITY_ATLAS_LIFETIME
        }) {
            self.window_opacity_atlas = None;
        }
        self.window_clip_masks
            .retain(|entry| self.frame_number.wrapping_sub(entry.mask.last_frame.get()) <= 2);
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

    pub(super) fn finish_frame(&mut self, canvas: &mut Canvas<WGPURenderer>, scene: &SceneIndex) {
        self.vectors.finish_frame(canvas);
        self.changing_groups
            .retain(|_, group| group.last_frame == self.frame_number);
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
                || self.frame_number.wrapping_sub(layer.last_frame) <= 120
                    && matches!(owner, LayerOwner::SceneNode(id) if scene.is_pooled(*id))
            {
                return true;
            }
            layer.release_image(canvas);
            false
        });
    }

    pub(super) fn clear_images(&mut self, canvas: &mut Canvas<WGPURenderer>) {
        self.window_clip_masks.clear();
        self.clear_opacity_atlas(canvas);
        self.window_opacity_atlas = None;
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
        for layer in std::mem::take(&mut self.opacity).into_values() {
            layer.release_image(canvas);
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
        let window = self
            .window_opacity_atlas
            .as_ref()
            .map_or(0, |_| WINDOW_ATLAS_BYTES);
        atlas
            + window
            + self
                .layers
                .iter()
                .map(|layer| layer.raster.bytes)
                .chain(
                    self.opacity
                        .values()
                        .filter(|layer| {
                            Rc::strong_count(&layer.image) == 1 || charged.insert(layer.image.image)
                        })
                        .map(|layer| layer.image.bytes),
                )
                .chain(self.vectors.rasters().map(|raster| raster.bytes))
                .chain(self.shadow_masks.values().map(|mask| mask.main.bytes))
                .chain(self.window_clip_masks.iter().map(|mask| mask.bytes))
                .sum::<u64>()
    }

    pub(super) fn sync_font_defaults(
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

    fn admit_stable_group(
        &mut self,
        owner: &LayerOwner,
        node: &Node,
        placement: LayerPlacement,
        repeated_phase_change: bool,
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
            group.last_frame = self.frame_number;
            return false;
        }
        if !repeated_phase_change
            && self
                .opacity
                .get(owner)
                .is_none_or(|layer| layer.node.same_paint_except_layer_composition(node))
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
                },
            );
        }
        false
    }
}

impl Renderer {
    pub(super) fn pack_composited_opacity_layers(&mut self, layers: &mut [WindowLayer]) {
        fn source(layer: &WindowLayer) -> Option<&Rc<CachedLayerImage>> {
            let WindowLayerSource::Texture(image) = &layer.source else {
                return None;
            };
            let texture = &image.texture;
            (!layer.surface_only
                && texture.format() == wgpu::TextureFormat::Rgba8Unorm
                && texture.usage().contains(wgpu::TextureUsages::COPY_SRC)
                && texture.width() + 2 <= WINDOW_ATLAS_EDGE
                && texture.height() + 2 <= WINDOW_ATLAS_EDGE)
                .then_some(image)
        }

        let cache = &mut self.retained_layers;
        if cache.opacity_atlas_disabled {
            return;
        }
        if let Some(atlas) = &mut cache.window_opacity_atlas
            && !atlas.tiles.is_empty()
            && atlas
                .tiles
                .values()
                .all(|tile| tile.source.strong_count() == 0)
        {
            atlas.tiles.clear();
            atlas
                .packing
                .reset(WINDOW_ATLAS_EDGE as usize, WINDOW_ATLAS_EDGE as usize);
        }
        if cache.window_opacity_atlas.is_none() {
            let mut candidates = layers.iter().filter_map(source);
            let Some(first) = candidates.next() else {
                return;
            };
            if !candidates.any(|image| !Rc::ptr_eq(first, image))
                || WINDOW_ATLAS_EDGE > self.device.limits().max_texture_dimension_2d
                || WINDOW_ATLAS_BYTES > cache.budget.saturating_sub(cache.resident_bytes())
            {
                return;
            }
            cache.window_opacity_atlas =
                Some(WindowOpacityAtlas::new(&self.device, cache.frame_number));
        }
        let Some(atlas) = &mut cache.window_opacity_atlas else {
            return;
        };
        let edge = WINDOW_ATLAS_EDGE as f32;
        for layer in layers {
            let Some(image) = source(layer) else { continue };
            let Some([x, y]) = atlas.tile(image, &self.device, &mut self.custom_encoder) else {
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

    #[doc(hidden)]
    pub fn opacity_atlas_stats(&self) -> OpacityAtlasStats {
        self.retained_layers.opacity_atlas_stats
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

    pub(super) fn prepare_shadow_mask(
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
        let Some(main) = texture.register(&mut self.canvas, tile.sampling()) else {
            return Ok(None);
        };
        let Some(capture) = texture.register(offscreen, tile.sampling()) else {
            self.canvas.delete_image(main.image);
            return Ok(None);
        };
        let image = if for_capture {
            capture.image
        } else {
            main.image
        };
        self.retained_layers.shadow_masks.insert(
            tile.key,
            ShadowMask {
                main,
                capture: capture.image,
                last_frame,
            },
        );
        self.retained_layers.new_shadow_masks += 1;
        self.retained_layers.shadow_mask_stats.rasterized += 1;
        Ok(Some(shadow.masked_draw(image, tile)))
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

    pub(super) fn can_retain_layer(&self, scale: f32) -> bool {
        self.can_cache_on_main_canvas() && has_device_scale(canvas_transform(&self.canvas), scale)
    }

    pub(super) fn opacity_layer_count(&self) -> usize {
        self.retained_layers.opacity.len()
    }

    fn opacity_owner(&self, path: &[usize]) -> LayerOwner {
        self.geometry_cache
            .enabled
            .then(|| self.render_scene.node_at(path))
            .flatten()
            .map_or_else(|| LayerOwner::Path(path.to_vec()), LayerOwner::SceneNode)
    }

    pub(super) fn cached_window_layer(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &[usize],
        in_layout: bool,
        frame: PaintFrame<'_>,
    ) -> Option<WindowLayer> {
        let bounds = layout.bounds;
        let opacity = node.opacity();
        let full_coverage = layout.clip.x <= bounds.x
            && layout.clip.y <= bounds.y
            && layout.clip.x + layout.clip.width >= bounds.x + bounds.width
            && layout.clip.y + layout.clip.height >= bounds.y + bounds.height;
        let hinted = node.boolean("cache-rendering-hint", false);
        if !(hinted || Self::can_cache_window_surface(node, layout, frame.scale))
            || !opacity.is_finite()
            || layout.allows_overflow()
            || !layout.local_transform().is_identity()
            || layout.table_pane_clip().is_some()
            || !full_coverage && !can_crop_sampled_layer(layout, frame.scale)
        {
            return None;
        }
        let owner = self.opacity_owner(path);
        let layer = self.retained_layers.opacity.get_mut(&owner)?;
        if layer.scale != frame.scale
            || layer.palette != frame.theme
            || layer.in_layout != in_layout
            || !full_coverage && layer.resample_origin.is_none()
        {
            return None;
        }
        let image = layer.image.cached.as_ref()?.clone();
        let raster_bounds = if layer.surface_only && node.kind == "Rectangle" {
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
            if hinted
                || !captured.preserves_surface_raster(current)
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
            if !layer.reuse_at(layout, clip, full_coverage) {
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
            sample_mask: false,
        })
    }

    pub(super) fn can_cache_window_surface(node: &Node, layout: &LayoutNode, scale: f32) -> bool {
        let plain = match node.kind {
            "Rectangle" => {
                node.opacity() == 1.0
                    && node.color("background").is_some()
                    && crate::rectangle::plain_surface(node, layout)
            }
            "Label" | "Text" => {
                Self::is_plain_label(node, layout)
                    && matches!(
                        node.properties.get("text"),
                        Some(xrack_runtime::Value::String(_))
                    )
            }
            "Image" => {
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
            && crate::pixel_aligned(layout.bounds, scale)
            && shadow::nonempty(layout.bounds)
    }

    pub(super) fn admit_window_surface(
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
            let stable = previous.last_frame == previous_frame
                && if node.kind == "Rectangle" {
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
            return stable;
        }
        if candidates.len() < MAX_LAYERS {
            candidates.insert(
                owner,
                ChangingGroup {
                    node: Box::new(node.clone()),
                    last_frame: self.retained_layers.frame_number,
                    moving_phase: Some(placement),
                },
            );
        }
        false
    }

    pub(super) fn capture_window_surface(
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
            layer.release_image(&mut self.canvas);
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
                        renderer.paint_surface(node, layout, &mut path.to_vec(), frame, in_layout)
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
            sample_mask: false,
        })
    }

    pub(super) fn capture_window_viewport(
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
            sample_mask: false,
        })
    }

    pub(super) fn window_clip_mask(
        &mut self,
        shapes: &Rc<[RoundedClip]>,
        scale: f32,
    ) -> Option<LayerClip> {
        let bounds = shapes
            .iter()
            .map(|shape| shape.bounds)
            .reduce(Rect::intersect)?;
        let physical = device_bounds(bounds, scale);
        let origin = [physical.x as i32, physical.y as i32];
        if let Some(entry) = self
            .retained_layers
            .window_clip_masks
            .iter()
            .find(|entry| entry.matches(shapes, physical, scale))
        {
            return Some(LayerClip {
                mask: entry.mask.clone(),
                origin,
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
                shapes: shapes.clone(),
                scale,
                physical,
                mask: mask.clone(),
                bytes: captured.bytes,
            });
        Some(LayerClip { mask, origin })
    }

    pub(super) fn retain_composited_layers(&mut self, layers: &[WindowLayer]) {
        for item in layers {
            if let Some(clip) = &item.mask {
                clip.mask.last_frame.set(self.retained_layers.frame_number);
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

    pub(super) fn can_cache_on_main_canvas(&self) -> bool {
        !self.retained_layers.capturing && self.retained_layers.budget > 0
    }

    pub(super) fn using_capture_canvas(&self) -> bool {
        self.retained_layers.capturing
    }

    pub(super) fn paint_stack_layer(
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

    pub(super) fn paint_retained_opacity(
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
        let mut phase_changed = false;
        let mut repeated_phase_change = false;
        let mut cropped_sample = false;
        let mut discard_sample = false;
        let cached = self
            .retained_layers
            .opacity
            .get_mut(&owner)
            .and_then(|layer| {
                if layer.scale != frame.scale
                    || layer.palette != frame.theme
                    || layer.in_layout != in_layout
                    || layer.surface_only
                    || !layer.node.same_paint_except_layer_composition(node)
                {
                    return None;
                }
                let sampled = layer.resample_origin.is_some();
                let crop =
                    sampled && hint && !resample && can_crop_sampled_layer(layout, frame.scale);
                if sampled != resample && !crop {
                    return None;
                }
                let previous = layer.captured_layout.bounds;
                if layer.reuse_at(layout, clip, full_coverage) {
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
        let raster = if let Some(raster) = cached {
            self.retained_layers.changing_groups.remove(&owner);
            self.retained_layers.opacity_stats.reused += 1;
            raster
        } else {
            let defer_unstable = resample
                || shadow.is_some()
                || !cfg!(any(target_os = "ios", target_os = "android"));
            if !passive_content(node) {
                return Ok(false);
            }
            if defer_unstable
                && !self.retained_layers.admit_stable_group(
                    &owner,
                    node,
                    placement,
                    repeated_phase_change,
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
                if self.paint_transient_opacity(
                    node,
                    layout,
                    path,
                    frame,
                    in_layout,
                    OpacitySource::MovingLayer,
                )? {
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
            if let Some(layer) = self.retained_layers.opacity.remove(&owner) {
                layer.release_image(&mut self.canvas);
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
            let style = OpacityTileStyle {
                scale: frame.scale,
                palette: frame.theme,
                in_layout,
            };
            let shared = (resample
                && self.retained_layers.hinted_layer_sharing
                && self.retained_layers.opacity.len() < MAX_LAYERS)
                .then(|| {
                    self.retained_layers
                        .opacity
                        .values()
                        .find(|layer| layer.shares_hint(node, layout, style, raster_bounds))
                        .map(|layer| layer.image.clone())
                })
                .flatten();
            let image = if let Some(image) = shared {
                self.retained_layers.opacity_stats.reused += 1;
                image
            } else {
                let Some(captured) = self.capture_texture(bounds, frame.scale, |renderer| {
                    renderer.paint_group(clip, 1.0, |renderer| {
                        renderer
                            .paint_element_with_shadow(node, layout, path, frame, in_layout, paint)
                    })
                })?
                else {
                    self.retained_layers.opacity_stats.direct += 1;
                    return Ok(false);
                };
                let Some(raster) = captured.register(&mut self.canvas, flags) else {
                    self.retained_layers.opacity_stats.direct += 1;
                    return Ok(false);
                };
                self.retained_layers.opacity_stats.rasterized += 1;
                Rc::new(OpacityImage {
                    image: raster.image,
                    bytes: raster.bytes,
                    cached: resample.then(|| {
                        Rc::new(CachedLayerImage {
                            texture: captured.texture,
                            binding: OnceCell::new(),
                        })
                    }),
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
                    resample_origin: resample.then_some(Point {
                        x: raster.bounds.x - layout.bounds.x,
                        y: raster.bounds.y - layout.bounds.y,
                    }),
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

    pub(super) fn prepare_rectangle_opacity_atlas(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        scale: f32,
    ) {
        if cfg!(any(target_os = "ios", target_os = "android"))
            || self.retained_layers.opacity_atlas_disabled
            || self.retained_layers.opacity_atlas.is_some()
            || self.retained_layers.previous_opacity_demand.groups >= 2
            || layout.children.len() < 2
        {
            return;
        }
        let limit = self.opacity_atlas_limit();
        let mut demand = OpacityDemand::default();
        for (_, child, item) in layout.children_in_paint_order(node) {
            let opacity = child.opacity();
            if !item.visible
                || !(0.0 < opacity && opacity < 1.0)
                || !item.local_transform().is_identity()
                || item.has_ancestor_clip_chain()
                || item.table_pane_clip().is_some()
                || !crate::rectangle::single_fill(child, item)
            {
                continue;
            }
            let clip = item.clip.intersect(item.bounds);
            if !shadow::nonempty(clip)
                || (crate::pixel_aligned(clip, scale) && crate::pixel_aligned(item.bounds, scale))
            {
                continue;
            }
            let device = device_bounds(clip, scale);
            let width = device.width as u32;
            let height = device.height as u32;
            if opacity_tile_fits(width, height, limit) {
                demand.record(width, height);
            }
        }
        if demand.groups >= 2 {
            // Size the cold atlas from the imminent sibling fills.
            self.create_opacity_atlas(demand.atlas_edge(0, 0, limit));
        }
    }

    pub(super) fn paint_transient_opacity(
        &mut self,
        node: &Node,
        layout: &LayoutNode,
        path: &mut Vec<usize>,
        frame: PaintFrame<'_>,
        in_layout: bool,
        source: OpacitySource<'_>,
    ) -> Result<bool, String> {
        let (shadow, moving_layer) = match source {
            OpacitySource::Transient(shadow) => (shadow, false),
            OpacitySource::MovingLayer => (None, true),
        };
        if cfg!(any(target_os = "ios", target_os = "android"))
            || self.retained_layers.opacity_atlas_disabled
            || !self.can_retain_layer(frame.scale)
            || self.retained_layers.vectors.opacity_passthrough
            || layout.has_ancestor_clip_chain()
            || node.children.is_empty() && !crate::rectangle::single_fill(node, layout)
            || !passive_content(node)
        {
            return Ok(false);
        }
        let content = layout.clip.intersect(layout.bounds);
        let clip = shadow.map_or(content, |(_, effect)| shadow::union(content, *effect));
        if !shadow::nonempty(clip) {
            return Ok(false);
        }
        let device = device_bounds(clip, frame.scale);
        let raster = logical_bounds(device, frame.scale);
        let limit = self.opacity_atlas_limit();
        let [width, height] = [device.width as u32, device.height as u32];
        if !opacity_tile_fits(width, height, limit) {
            return Ok(false);
        }
        if shadow.is_some_and(|(shadow, _)| shadow.mask_tile(frame.scale, layout.clip).is_none()) {
            return Ok(false);
        }
        self.retained_layers.opacity_demand.record(width, height);
        let demand = self.retained_layers.previous_opacity_demand;
        if demand.groups < 2
            && !(node.children.is_empty() && self.retained_layers.opacity_atlas.is_some())
        {
            return Ok(false);
        }
        let mask = self.prepare_shadow_mask(shadow, layout, frame.scale, true)?;
        if shadow.is_some() && mask.is_none() {
            return Ok(false);
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
        let share_tiles =
            moving_layer && self.retained_layers.opacity_tile_sharing && frame.scale >= 1.0;
        let style = OpacityTileStyle {
            scale: frame.scale,
            palette: frame.theme,
            in_layout,
        };
        let Some(atlas) = self.retained_layers.opacity_atlas.as_mut() else {
            return Ok(false);
        };
        let edge = atlas.texture.width();
        let image = atlas.image;
        let shared = if share_tiles {
            atlas
                .tiles
                .iter()
                .find(|tile| tile.matches(node, layout, clip, device, style))
                .map(|tile| tile.position)
        } else {
            None
        };
        let position = if let Some(position) = shared {
            self.retained_layers.opacity_atlas_stats.reused_tiles += 1;
            position
        } else {
            let Some((x, y)) = atlas.slots.add_rect(width as usize, height as usize) else {
                return Ok(false);
            };
            [x as u32, y as u32]
        };
        atlas.last_frame = self.retained_layers.frame_number;
        let offset = Point {
            x: position[0] as f32 / frame.scale,
            y: position[1] as f32 / frame.scale,
        };
        if shared.is_none() {
            let was_pending = atlas.pending;
            let clear = !atlas.cleared;
            atlas.pending = true;
            atlas.cleared = true;
            let paint = shadow.map(|(source, clip)| shadow::ShadowPaint {
                source,
                clip: *clip,
                mask,
            });
            let recorded = self.with_capture_canvas(|renderer| {
                if !was_pending {
                    renderer.canvas.set_size(edge, edge, frame.scale);
                    renderer.images.begin_frame();
                }
                renderer.canvas.reset();
                if clear {
                    renderer
                        .canvas
                        .clear_rect(0, 0, edge, edge, Color::rgba(0, 0, 0, 0));
                }
                renderer.canvas.scale(frame.scale, frame.scale);
                renderer
                    .canvas
                    .translate(offset.x - raster.x, offset.y - raster.y);
                let capture_clip = if moving_layer { clip } else { raster };
                renderer.paint_group(capture_clip, 1.0, |renderer| {
                    renderer.paint_element_with_shadow(node, layout, path, frame, in_layout, paint)
                })
            });
            match recorded {
                Err(error) => {
                    self.submit_opacity_atlas();
                    return Err(error);
                }
                Ok(None) => {
                    if let Some(atlas) = &mut self.retained_layers.opacity_atlas {
                        atlas.pending = was_pending;
                        atlas.cleared = !clear;
                    }
                    return Ok(false);
                }
                Ok(Some(())) => {}
            }
            if share_tiles
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
                });
            }
        }
        let mut path = Path::new();
        path.rect(raster.x, raster.y, raster.width, raster.height);
        let extent = edge as f32 / frame.scale;
        let mut paint = Paint::image(
            image,
            raster.x - offset.x,
            raster.y - offset.y,
            extent,
            extent,
            0.0,
            node.opacity(),
        );
        paint.set_anti_alias(moving_layer && !crate::pixel_aligned(clip, frame.scale));
        if raster == clip {
            self.canvas.fill_path(&path, &paint);
        } else {
            self.canvas.save();
            self.canvas
                .intersect_scissor(clip.x, clip.y, clip.width, clip.height);
            self.canvas.fill_path(&path, &paint);
            self.canvas.restore();
        }
        self.retained_layers.opacity_atlas_stats.groups += 1;
        Ok(true)
    }

    fn create_opacity_atlas(&mut self, edge: u32) {
        let bytes = u64::from(edge) * u64::from(edge) * 4;
        let previous = self
            .retained_layers
            .opacity_atlas
            .as_ref()
            .map_or(0, |atlas| {
                u64::from(atlas.texture.width()) * u64::from(atlas.texture.height()) * 4
            });
        let used = self.retained_layers.resident_bytes() - previous;
        if bytes > self.retained_layers.budget.saturating_sub(used) {
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
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT | wgpu::TextureUsages::TEXTURE_BINDING,
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
            pending: false,
            cleared: false,
            last_frame: self.retained_layers.frame_number,
        });
    }

    pub(super) fn submit_opacity_atlas(&mut self) {
        if let Some(opacity) = self.take_opacity_atlas_commands() {
            xrack_runtime::update_profile::submit_gpu();
            let rounded = self.rectangle_batches.take_rounded_commands();
            self.queue.submit(rounded.into_iter().chain([opacity]));
        }
    }

    pub(super) fn take_opacity_atlas_commands(&mut self) -> Option<wgpu::CommandBuffer> {
        let atlas = self.retained_layers.opacity_atlas.as_mut()?;
        if !atlas.pending {
            return None;
        }
        let (canvas, images) = self.retained_layers.capture.as_mut()?;
        atlas.pending = false;
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
        let Some(raster) =
            self.capture_layer(bounds, frame.scale, ImageFlags::empty(), |renderer| {
                renderer.paint(&page.0, &page.1, &mut Vec::new(), frame, false)
            })?
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

    pub(super) fn capture_layer(
        &mut self,
        bounds: Rect,
        scale: f32,
        flags: ImageFlags,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Option<Raster>, String> {
        Ok(self
            .capture_texture(bounds, scale, draw)?
            .and_then(|texture| texture.register(&mut self.canvas, flags)))
    }

    fn capture_texture(
        &mut self,
        bounds: Rect,
        scale: f32,
        draw: impl FnOnce(&mut Self) -> Result<(), String>,
    ) -> Result<Option<CapturedTexture>, String> {
        if bounds.width <= 0.0 || bounds.height <= 0.0 {
            return Ok(None);
        }
        self.capture_device_texture(device_bounds(bounds, scale), scale, draw)
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
        if self.retained_layers.layers.len()
            + self.retained_layers.opacity.len()
            + self.retained_layers.vectors.rasters().count()
            + self.retained_layers.shadow_masks.len()
            + self.retained_layers.window_clip_masks.len()
            >= MAX_LAYERS
        {
            return Ok(None);
        }
        let Rect { width, height, .. } = physical;
        let limit = self.device.limits().max_texture_dimension_2d as f32;
        if !(width > 0.0 && width <= limit && height > 0.0 && height <= limit) {
            return Ok(None);
        }
        let bounds = logical_bounds(physical, scale);
        let width = width as u32;
        let height = height as u32;
        let texel_bytes = match format {
            wgpu::TextureFormat::Rgba16Float => 8,
            wgpu::TextureFormat::R16Float => 2,
            _ => 4,
        };
        let bytes = u64::from(width) * u64::from(height) * texel_bytes;
        let used = self.retained_layers.resident_bytes();
        if bytes > self.retained_layers.budget.saturating_sub(used) {
            return Ok(None);
        }
        self.submit_opacity_atlas();
        let texture = self.device.create_texture(&wgpu::TextureDescriptor {
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
        let rendered = self.with_capture_canvas(|renderer| {
            renderer.canvas.set_size(width, height, scale);
            renderer.canvas.reset();
            renderer
                .canvas
                .clear_rect(0, 0, width, height, Color::rgba(0, 0, 0, 0));
            renderer.canvas.scale(scale, scale);
            renderer.canvas.translate(-bounds.x, -bounds.y);
            renderer.images.begin_frame();
            let result = draw(renderer);
            renderer.submit_canvas(&texture);
            renderer.images.end_frame(&mut renderer.canvas);
            result
        })?;
        Ok(rendered.map(|()| CapturedTexture {
            texture,
            bounds,
            bytes,
        }))
    }

    fn with_capture_canvas<T>(
        &mut self,
        draw: impl FnOnce(&mut Self) -> Result<T, String>,
    ) -> Result<Option<T>, String> {
        if self.retained_layers.capturing {
            return Ok(None);
        }
        if self.retained_layers.capture.is_none() {
            let Ok(canvas) = Canvas::new_with_text_context(
                WGPURenderer::new(self.device.clone(), self.queue.clone()),
                self.text.context().clone(),
            ) else {
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
        let result = draw(self);
        std::mem::swap(&mut self.canvas, &mut capture.0);
        std::mem::swap(&mut self.images, &mut capture.1);
        self.retained_layers.capture = Some(capture);
        self.retained_layers.capturing = false;
        result.map(Some)
    }
}

fn opacity_tile_fits(width: u32, height: u32, limit: u32) -> bool {
    width <= limit
        && height <= limit
        && u64::from(width) * u64::from(height) <= u64::from(limit) * u64::from(limit) / 4
}

pub(super) fn passive_content(node: &Node) -> bool {
    matches!(
        node.kind,
        "Rectangle"
            | "Text"
            | "Label"
            | "Path"
            | "Image"
            | "HorizontalLayout"
            | "VerticalLayout"
            | "Row"
            | "Column"
            | "GridLayout"
    ) && node.children.iter().all(passive_content)
}
