use crate::{
    Canvas, Color, LayoutNode, Node, Paint, Path, Rect, Renderer, WGPURenderer, append_rect,
    canvas_transform, color, pixel_phase,
    retained_layers::{CachedLayerImage, WindowLayer, device_bounds, logical_bounds},
    transform::has_pixel_aligned_scale,
};
use femtovg::rgb::RGBA8;
use femtovg::{
    Atlas, BlendFactor, DrawCommand, GlyphDrawCommands, ImageFlags, ImageId, ImageInfo,
    LayerEffects, PixelFormat, Quad, imgref::Img,
};
use std::{
    cell::OnceCell,
    collections::{HashMap, HashSet},
    rc::Rc,
};
use xrack_runtime::{Brush, Value};

const ATLAS_WIDTH: usize = 128;
const ATLAS_HEIGHT: usize = 128;
const MIN_BATCH_SIZE: usize = 8;
const ROUND_ATLAS_SIZE: usize = 512;
const MAX_ROUND_TILE: f32 = 128.0;
const MAX_NEW_ROUND_TILES: usize = 16;

pub(super) struct RoundedPanel {
    bounds: Rect,
    template: Rect,
    margins: [f32; 4],
    radii: [f32; 4],
    tint: Color,
}

impl RoundedPanel {
    pub(super) fn from_node(node: &Node, layout: &LayoutNode, scale: f32) -> Option<Self> {
        let bounds = layout.bounds;
        if node.children.is_empty()
            || !plain_surface(node, layout)
            || node.opacity() != 1.0
            || !layout.local_transform().is_identity()
            || layout.table_pane_clip().is_some()
            || scale < 1.0
            || !crate::pixel_aligned(bounds, scale)
        {
            return None;
        }
        let tint = color(node.color("background")?);
        let radii = xrack_layout::corner_radii(node, bounds);
        if tint.a != 1.0 || radii == [0.0; 4] {
            return None;
        }
        let [tl, tr, br, bl] = radii;
        // Leave a device pixel beyond each arc. The straight middle spans
        // can then hold one texel each without sampling a curved edge.
        let margins = [tl.max(bl), tl.max(tr), tr.max(br), bl.max(br)]
            .map(|radius| (radius * scale).ceil() + 1.0);
        let [left, top, right, bottom] = margins;
        let width = left + right + 1.0;
        let height = top + bottom + 1.0;
        let template = Rect::new(0.0, 0.0, width / scale, height / scale);
        let limit = MAX_ROUND_TILE + 3.0;
        (width <= limit
            && height <= limit
            && template.width < bounds.width
            && template.height < bounds.height
            && crate::pixel_aligned(template, scale))
        .then_some(Self {
            bounds,
            template,
            margins,
            radii,
            tint,
        })
    }
}

/// A leaf whose only paint operation can be its background fill.
pub(super) fn single_fill(node: &Node, layout: &LayoutNode) -> bool {
    node.children.is_empty() && plain_surface(node, layout)
}

pub(super) fn plain_surface(node: &Node, layout: &LayoutNode) -> bool {
    surface_node(node, layout) && node.properties.keys().all(|name| surface_property(name))
}

fn surface_node(node: &Node, layout: &LayoutNode) -> bool {
    node.kind == "Rectangle"
        && node.list.is_none()
        && node.scroll_target.is_none()
        && layout.table.is_none()
        && !layout.allows_overflow()
}

fn surface_property(name: &str) -> bool {
    matches!(
        name,
        "x" | "y"
            | "width"
            | "height"
            | "z"
            | "opacity"
            | "visible"
            | "enabled"
            | "background"
            | "clip"
            | "border-radius"
            | "border-top-left-radius"
            | "border-top-right-radius"
            | "border-bottom-left-radius"
            | "border-bottom-right-radius"
            | "min-width"
            | "max-width"
            | "min-height"
            | "max-height"
            | "preferred-width"
            | "preferred-height"
            | "horizontal-stretch"
            | "vertical-stretch"
    )
}

/// A square fill covering whole device pixels can skip the coverage fringe.
pub(super) fn fill_surface(
    canvas: &mut Canvas<WGPURenderer>,
    bounds: Rect,
    radii: [f32; 4],
    mut paint: Paint,
) {
    let mut path = Path::new();
    if radii == [0.0; 4] && bounds.width > 0.0 && bounds.height > 0.0 {
        let transform = canvas_transform(canvas);
        if crate::image::supports_quad_transform(transform)
            && crate::image::quad_edges_are_pixel_aligned(bounds, transform)
        {
            paint.set_anti_alias(false);
        }
        path.rect(bounds.x, bounds.y, bounds.width, bounds.height);
    } else {
        append_rect(&mut path, bounds, radii);
    }
    canvas.fill_path(&path, &paint);
}

#[derive(Clone, Copy)]
pub(super) struct Solid {
    bounds: Rect,
    shape: Rect,
    radii: [f32; 4],
    color: Color,
    opacity: f32,
    fractional: bool,
}

impl Solid {
    pub(super) fn from_node(node: &Node, layout: &LayoutNode, scale: f32) -> Option<Self> {
        if !node.children.is_empty()
            || !surface_node(node, layout)
            || !layout.local_transform().is_identity()
            || layout.has_ancestor_clip_chain()
            || layout.table_pane_clip().is_some()
        {
            return None;
        }
        let mut background = None;
        let mut opacity = 1.0;
        let mut has_radius = false;
        // Validate and read the paint properties in one walk. Flat fills need
        // neither the corner lookups nor the size-dependent radius calculation.
        for (&name, value) in &node.properties {
            match name {
                "background" => match value {
                    Value::Color(value) | Value::Brush(Brush::Solid(value)) => {
                        background = Some(*value);
                    }
                    _ => return None,
                },
                "opacity" => opacity = node.opacity(),
                "border-radius"
                | "border-top-left-radius"
                | "border-top-right-radius"
                | "border-bottom-left-radius"
                | "border-bottom-right-radius" => {
                    has_radius |= matches!(value, Value::Length(radius) if radius.0.is_finite() && radius.0 > 0.0);
                }
                name if surface_property(name) => (),
                _ => return None,
            }
        }
        let tint = color(background?);
        let bounds = layout.bounds.intersect(layout.clip);
        let shape = layout.bounds;
        let radii = if has_radius {
            xrack_layout::corner_radii(node, shape)
        } else {
            [0.0; 4]
        };
        if !layout.visible || tint.a * opacity == 0.0 || bounds.width <= 0.0 || bounds.height <= 0.0
        {
            return Some(Self {
                bounds: Rect::default(),
                shape,
                radii,
                color: tint,
                opacity,
                fractional: false,
            });
        }
        let aligned = crate::pixel_aligned(shape, scale) && crate::pixel_aligned(bounds, scale);
        let rounded = radii != [0.0; 4];
        if rounded
            && (shape.width * scale > MAX_ROUND_TILE || shape.height * scale > MAX_ROUND_TILE)
        {
            return None;
        }
        if rounded && !aligned {
            // A rounded tile bakes the shape's clip. An additional
            // ancestor edge near the raster still needs the path painter.
            let fringe = 1.0 / scale;
            if !(layout.clip.x <= shape.x - fringe
                && layout.clip.y <= shape.y - fringe
                && layout.clip.x + layout.clip.width >= shape.x + shape.width + fringe
                && layout.clip.y + layout.clip.height >= shape.y + shape.height + fringe)
            {
                return None;
            }
        }
        if !rounded && !aligned && scale < 1.0 {
            return None;
        }
        Some(Self {
            bounds,
            shape,
            radii,
            color: tint,
            opacity,
            fractional: !aligned,
        })
    }

    fn tint(self) -> Color {
        let mut tint = self.color;
        tint.a *= self.opacity;
        tint
    }

    pub(super) fn from_rect(
        bounds: Rect,
        clip: Rect,
        tint: xrack_runtime::Color,
        scale: f32,
    ) -> Option<Self> {
        let clipped = bounds.intersect(clip);
        if !crate::pixel_aligned(bounds, scale)
            || !crate::pixel_aligned(clipped, scale)
            || clipped.width <= 0.0
            || clipped.height <= 0.0
        {
            return None;
        }
        Some(Self {
            bounds: clipped,
            shape: bounds,
            radii: [0.0; 4],
            color: color(tint),
            opacity: 1.0,
            fractional: false,
        })
    }

    pub(super) fn opaque_rect(&self) -> Option<(Rect, u32)> {
        let tint = self.tint();
        (!self.uses_mask() && self.visible() && tint.a == 1.0).then_some((
            self.bounds,
            u32::from_be_bytes([tint.r, tint.g, tint.b, tint.a].map(|v| (v * 255.0).round() as u8)),
        ))
    }

    pub(super) fn square_bounds(&self) -> Option<Rect> {
        (!self.rounded()).then_some(self.bounds)
    }

    pub(super) fn plain_layer(self) -> Option<(Rect, Color)> {
        (!self.uses_mask()).then_some((self.bounds, self.tint()))
    }

    pub(super) fn window_opaque_layer(self, clip: Rect, scale: f32) -> Option<WindowLayer> {
        if self.rounded() || self.opacity != 1.0 || self.color.a != 1.0 {
            return None;
        }
        if !self.fractional {
            return Some(WindowLayer::solid(self.bounds, self.color));
        }
        self.square_batchable(clip, scale)
            .then(|| WindowLayer::covered_solid(self.bounds, self.color, scale))
    }

    pub(super) fn collect_square(&self, clip: Rect, scale: f32) -> bool {
        !self.rounded() && (self.opacity == 1.0 || self.square_batchable(clip, scale))
    }

    fn square_batchable(&self, clip: Rect, scale: f32) -> bool {
        if !self.fractional {
            return true;
        }
        let raster = device_bounds(self.bounds, scale);
        // The canvas applies the parent scissor to the quads. Its soft
        // edge must be outside the baked coverage.
        let interior = clip.scaled(scale).inset(0.5 / scale + 1.0 / 64.0);
        scale >= 1.0
            && self.bounds.width * scale > 1.0
            && self.bounds.height * scale > 1.0
            && raster == raster.intersect(interior)
    }

    pub(super) fn rounded(&self) -> bool {
        self.radii != [0.0; 4]
    }

    pub(super) fn uses_mask(&self) -> bool {
        self.fractional || self.rounded()
    }

    pub(super) fn visible(&self) -> bool {
        self.bounds.width > 0.0
    }

    pub(super) fn draw(self, canvas: &mut Canvas<WGPURenderer>) {
        let mut path = Path::new();
        let Rect {
            x,
            y,
            width,
            height,
        } = self.bounds;
        let mut paint = Paint::color(self.tint());
        if self.uses_mask() {
            canvas.save();
            canvas.intersect_scissor(x, y, width, height);
            append_rect(&mut path, self.shape, self.radii);
            fill_rounded(canvas, &path, self.color, self.opacity, self.fractional);
            canvas.restore();
        } else {
            path.rect(x, y, width, height);
            paint.set_anti_alias(false);
            canvas.fill_path(&path, &paint);
        }
    }
}

// With DPI >= 1 only the first and last pixel centers can hit a fringe.
// An opacity layer rasterizes the original shape and clips the composite.
// An ordinary upright fill rasterizes its intersection with the clip.
fn coverage_spans(
    [start, end]: [f32; 2],
    layer_shape: Option<[f32; 2]>,
    dpi: f32,
) -> [(f32, f32, f32, f32); 3] {
    let [fill_start, fill_end] = layer_shape.unwrap_or([start, end]);
    let left = start.floor();
    let right = end.ceil();
    let cover = |distance: f32| (0.5 + distance * dpi).clamp(0.0, 1.0);
    let first = cover(left + 0.5 - fill_start);
    let last = cover(fill_end - (right - 0.5));
    let [first_clip, last_clip] = if layer_shape.is_some() && dpi != 1.0 {
        [cover(left + 0.5 - start), cover(end - (right - 0.5))]
    } else {
        [1.0; 2]
    };
    let core_left = if first == 1.0 && first_clip == 1.0 {
        left
    } else {
        left + 1.0
    };
    let core_right = if last == 1.0 && last_clip == 1.0 {
        right
    } else {
        right - 1.0
    };
    let spans = [
        (left, core_left, first, first_clip),
        (core_left, core_right, 1.0, 1.0),
        (core_right, right, last, last_clip),
    ];
    if layer_shape.is_some() && dpi == 1.0 {
        // The original blit clips geometry at 1x. Let rasterization apply
        // its edge rule even when the boundary hits a pixel center.
        spans.map(|(a, b, fill, clip)| (a.max(start), b.min(end), fill, clip))
    } else {
        spans
    }
}

fn captured_tint(color: Color, coverage: f32, opacity: f32) -> Color {
    let alpha = color.a * coverage * 255.0;
    let saved_alpha = alpha.round_ties_even();
    if saved_alpha == 0.0 {
        return Color::rgbaf(0.0, 0.0, 0.0, 0.0);
    }
    // The layer normally stores premultiplied RGBA8. Recover a straight
    // tint for the color batch after that intermediate rounding.
    let channel = |value: f32| (value * alpha).round_ties_even() / saved_alpha;
    Color::rgbaf(
        channel(color.r),
        channel(color.g),
        channel(color.b),
        saved_alpha * opacity / 255.0,
    )
}

fn fill_rounded(
    canvas: &mut Canvas<WGPURenderer>,
    path: &Path,
    mut tint: Color,
    opacity: f32,
    fractional: bool,
) {
    if fractional && opacity < 1.0 {
        let _isolated = canvas.begin_layer(&LayerEffects::new().with_opacity(opacity));
        canvas.fill_path(path, &Paint::color(tint));
        canvas.end_layer();
    } else {
        tint.a *= opacity;
        canvas.fill_path(path, &Paint::color(tint));
    }
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
enum RoundedTileClip {
    Shape,
    Coverage,
    Vertical { y: u32, height: u32 },
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
struct RoundedKey {
    size: [u32; 2],
    pixels: [u32; 2],
    phase: [u8; 2],
    radii: [u32; 4],
    color: [u32; 4],
    opacity: u32,
    scale: u32,
    clip: RoundedTileClip,
}

impl RoundedKey {
    fn same_capsule(self, other: Self) -> bool {
        matches!(self.clip, RoundedTileClip::Vertical { .. })
            && matches!(other.clip, RoundedTileClip::Vertical { .. })
            && self.size == other.size
            && self.radii == other.radii
            && self.color == other.color
            && self.opacity == other.opacity
            && self.scale == other.scale
    }
}

struct FractionalSlot {
    origin: [u32; 2],
    capacity: [u32; 2],
    last_frame: u64,
}

struct RoundedAtlas {
    main_image: Option<ImageId>,
    capture_image: Option<ImageId>,
    texture: wgpu::Texture,
    window_texture: Option<Rc<CachedLayerImage>>,
    producer: Canvas<WGPURenderer>,
    packing: Atlas,
    reused_aligned_area: bool,
    slots: HashMap<RoundedKey, [u32; 2]>,
    fractional_packing: Atlas,
    reused_fractional_area: bool,
    fractional_slots: HashMap<RoundedKey, FractionalSlot>,
    frame: u64,
    reset_fractional: bool,
    new_tiles: usize,
    warm_requests: Vec<(RoundedKey, Solid)>,
    pending_scale: Option<f32>,
    full: bool,
    used: bool,
}

impl RoundedAtlas {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Option<Self> {
        let texture = device.create_texture(&wgpu::TextureDescriptor {
            label: Some("XRack rounded rectangle atlas"),
            size: wgpu::Extent3d {
                width: ROUND_ATLAS_SIZE as u32,
                height: ROUND_ATLAS_SIZE as u32,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count: 1,
            dimension: wgpu::TextureDimension::D2,
            format: wgpu::TextureFormat::Rgba8Unorm,
            usage: wgpu::TextureUsages::RENDER_ATTACHMENT
                | wgpu::TextureUsages::TEXTURE_BINDING
                | wgpu::TextureUsages::COPY_DST,
            view_formats: &[],
        });
        let producer = Canvas::new(WGPURenderer::new(device.clone(), queue.clone())).ok()?;
        Some(Self {
            main_image: None,
            capture_image: None,
            texture,
            window_texture: None,
            producer,
            packing: Atlas::new(ROUND_ATLAS_SIZE, ROUND_ATLAS_SIZE / 2),
            reused_aligned_area: false,
            slots: HashMap::new(),
            fractional_packing: Atlas::new(ROUND_ATLAS_SIZE, ROUND_ATLAS_SIZE / 2),
            reused_fractional_area: false,
            fractional_slots: HashMap::new(),
            frame: 0,
            reset_fractional: false,
            new_tiles: 0,
            warm_requests: Vec::new(),
            pending_scale: None,
            full: false,
            used: false,
        })
    }

    fn image(&mut self, canvas: &mut Canvas<WGPURenderer>, capturing: bool) -> Option<ImageId> {
        let slot = if capturing {
            &mut self.capture_image
        } else {
            &mut self.main_image
        };
        if let Some(id) = slot {
            return Some(*id);
        }
        let info = ImageInfo::new(
            ImageFlags::NEAREST,
            ROUND_ATLAS_SIZE,
            ROUND_ATLAS_SIZE,
            PixelFormat::Rgba8,
        );
        let image = canvas
            .create_image_from_native_texture(self.texture.clone(), info)
            .ok()?;
        *slot = Some(image);
        Some(image)
    }

    fn window_source(&mut self) -> Rc<CachedLayerImage> {
        self.window_texture
            .get_or_insert_with(|| {
                Rc::new(CachedLayerImage {
                    texture: self.texture.clone(),
                    binding: OnceCell::new(),
                })
            })
            .clone()
    }

    fn begin_frame(&mut self, retain_fractional: bool) {
        self.frame = self.frame.wrapping_add(1);
        self.new_tiles = 0;
        self.warm_requests.clear();
        if !retain_fractional || std::mem::take(&mut self.reset_fractional) {
            self.reused_fractional_area |= !self.fractional_slots.is_empty();
            self.fractional_packing
                .reset(ROUND_ATLAS_SIZE, ROUND_ATLAS_SIZE / 2);
            self.fractional_slots.clear();
        }
        if self.full && !self.used {
            // Keep hot tiles in a full atlas. Reclaim slots between frames once
            // the current scene no longer uses any of them.
            self.packing.reset(ROUND_ATLAS_SIZE, ROUND_ATLAS_SIZE / 2);
            self.reused_aligned_area = true;
            self.slots.clear();
            self.full = false;
        }
        self.used = false;
    }

    fn key(rectangle: Solid, device: Rect, scale: f32, clip: RoundedTileClip) -> RoundedKey {
        let color = if clip == RoundedTileClip::Shape {
            rectangle.color
        } else {
            Color::rgbaf(1.0, 1.0, 1.0, rectangle.color.a)
        };
        RoundedKey {
            size: [rectangle.shape.width, rectangle.shape.height].map(f32::to_bits),
            pixels: [device.width as u32, device.height as u32],
            phase: [
                pixel_phase(rectangle.shape.x * scale),
                pixel_phase(rectangle.shape.y * scale),
            ],
            radii: rectangle.radii.map(f32::to_bits),
            color: [color.r, color.g, color.b, color.a].map(f32::to_bits),
            opacity: rectangle.opacity.to_bits(),
            scale: scale.to_bits(),
            clip,
        }
    }

    fn exceeds_fractional_capacity(rectangles: &[Solid], scale: f32) -> bool {
        let mut keys = HashSet::new();
        let mut pixels = 0usize;
        for &rect in rectangles
            .iter()
            .filter(|rect| rect.fractional && rect.opacity < 1.0)
        {
            let key = Self::key(
                rect,
                device_bounds(rect.shape, scale),
                scale,
                RoundedTileClip::Shape,
            );
            if keys.insert(key) {
                pixels += key.pixels[0] as usize * key.pixels[1] as usize;
                if pixels > ROUND_ATLAS_SIZE * (ROUND_ATLAS_SIZE / 2) {
                    return true;
                }
            }
        }
        false
    }

    fn quad(
        &mut self,
        queue: &wgpu::Queue,
        rectangle: Solid,
        scale: f32,
        clip: RoundedTileClip,
    ) -> Option<Quad> {
        self.quad_with_admission(queue, rectangle, scale, clip, true)
    }

    fn quad_with_admission(
        &mut self,
        queue: &wgpu::Queue,
        rectangle: Solid,
        scale: f32,
        clip: RoundedTileClip,
        admit_new: bool,
    ) -> Option<Quad> {
        let device = device_bounds(rectangle.shape, scale);
        let key = Self::key(rectangle, device, scale, clip);
        let cached = if rectangle.fractional {
            self.fractional_slots.get_mut(&key).map(|slot| {
                slot.last_frame = self.frame;
                slot.origin
            })
        } else {
            self.slots.get(&key).copied()
        };
        let [left, top] = match cached {
            Some(origin) => origin,
            None if !admit_new => return None,
            None => {
                let warm = self.should_warm_capsule(key);
                let origin = self.rasterize(queue, rectangle, scale, key, true)?;
                if warm {
                    self.warm_requests.push((key, rectangle));
                }
                origin
            }
        };
        self.used |= !rectangle.fractional;
        let raster = logical_bounds(device, scale);
        let Rect {
            x,
            y,
            width,
            height,
        } = if rectangle.fractional {
            raster
        } else {
            rectangle.bounds
        };
        let s = left as f32 + (x - raster.x) * scale;
        let t = top as f32 + (y - raster.y) * scale;
        let stride = ROUND_ATLAS_SIZE as f32;
        Some(Quad {
            x0: x,
            y0: y,
            x1: x + width,
            y1: y + height,
            s0: s / stride,
            t0: t / stride,
            s1: (s + width * scale) / stride,
            t1: (t + height * scale) / stride,
        })
    }

    fn should_warm_capsule(&self, key: RoundedKey) -> bool {
        if key.phase[1] == 0
            || !matches!(key.clip, RoundedTileClip::Vertical { .. })
            || self.warm_requests.iter().any(|&(pending, _)| {
                pending.phase[1] == key.phase[1]
                    && pending.clip == key.clip
                    && key.same_capsule(pending)
            })
        {
            return false;
        }
        let mut seen_fractional_pose = false;
        for &old in self.fractional_slots.keys() {
            if old.phase[1] != 0 && key.same_capsule(old) {
                if old.phase[1] == key.phase[1] {
                    return true;
                }
                seen_fractional_pose = true;
            }
        }
        // Seed the first fractional scroll pose. Later distinct poses must
        // recur before borrowing horizontal phases from the cache.
        !seen_fractional_pose
    }

    fn warm_capsule_phases(
        &mut self,
        queue: &wgpu::Queue,
        rectangle: Solid,
        scale: f32,
        key: RoundedKey,
    ) {
        // Rasterize previously seen horizontal cap phases together with the
        // required tiles in the same pending atlas pass.
        let remaining = MAX_NEW_ROUND_TILES.saturating_sub(self.new_tiles);
        let mut hints = Vec::new();
        for &old in self.slots.keys().chain(self.fractional_slots.keys()) {
            if hints.len() == remaining {
                break;
            }
            if key.same_capsule(old) {
                let hint = RoundedKey {
                    phase: [old.phase[0], key.phase[1]],
                    pixels: [old.pixels[0], key.pixels[1]],
                    ..key
                };
                if !self.fractional_slots.contains_key(&hint) && !hints.contains(&hint) {
                    hints.push(hint);
                }
            }
        }
        for hint in hints {
            if self
                .rasterize(queue, rectangle, scale, hint, false)
                .is_none()
            {
                break;
            }
        }
    }

    fn allocate_fractional(&mut self, key: RoundedKey, recycle: bool) -> Option<([u32; 2], bool)> {
        let [width, height] = key.pixels;
        let (slot, needs_clear) = if let Some((x, y)) = self
            .fractional_packing
            .add_rect(width as usize, height as usize)
        {
            (
                FractionalSlot {
                    origin: [x as u32, (y + ROUND_ATLAS_SIZE / 2) as u32],
                    capacity: key.pixels,
                    last_frame: self.frame,
                },
                self.reused_fractional_area,
            )
        } else if recycle {
            // A slot referenced by this frame may still be sampled by an
            // unsubmitted canvas. Only overwrite slots from earlier frames.
            let oldest = self
                .fractional_slots
                .iter()
                .filter(|(_, slot)| {
                    slot.last_frame != self.frame
                        && slot.capacity[0] >= width
                        && slot.capacity[1] >= height
                })
                .max_by_key(|(_, slot)| (self.frame.wrapping_sub(slot.last_frame), slot.origin))
                .map(|(&key, _)| key);
            let Some(mut slot) = oldest.and_then(|key| self.fractional_slots.remove(&key)) else {
                // Try a fresh packing next frame when slot sizes change.
                self.reset_fractional = true;
                return None;
            };
            slot.last_frame = self.frame;
            (slot, true)
        } else {
            return None;
        };
        let origin = slot.origin;
        self.fractional_slots.insert(key, slot);
        Some((origin, needs_clear))
    }

    fn rasterize(
        &mut self,
        queue: &wgpu::Queue,
        rectangle: Solid,
        scale: f32,
        key: RoundedKey,
        recycle: bool,
    ) -> Option<[u32; 2]> {
        if self.full && !rectangle.fractional || self.new_tiles >= MAX_NEW_ROUND_TILES {
            return None;
        }
        let [width, height] = key.pixels;
        let ([x, y], needs_clear) = if rectangle.fractional {
            self.allocate_fractional(key, recycle)?
        } else {
            let Some((x, y)) = self.packing.add_rect(width as usize, height as usize) else {
                self.full = true;
                return None;
            };
            self.slots.insert(key, [x as u32, y as u32]);
            ([x as u32, y as u32], self.reused_aligned_area)
        };

        let coverage_only = key.clip != RoundedTileClip::Shape;
        if !coverage_only {
            // The color-quad shader expects straight RGBA. Initialize RGB with the
            // unassociated tint, then let FemtoVG's regular path fill update alpha.
            let [r, g, b] = [rectangle.color.r, rectangle.color.g, rectangle.color.b]
                .map(|channel| (channel * 255.0).round() as u8);
            let pixels = [r, g, b, 0].repeat(width as usize * height as usize);
            queue.write_texture(
                wgpu::TexelCopyTextureInfo {
                    texture: &self.texture,
                    mip_level: 0,
                    origin: wgpu::Origin3d { x, y, z: 0 },
                    aspect: wgpu::TextureAspect::All,
                },
                &pixels,
                wgpu::TexelCopyBufferLayout {
                    offset: 0,
                    bytes_per_row: Some(width * 4),
                    rows_per_image: Some(height),
                },
                wgpu::Extent3d {
                    width,
                    height,
                    depth_or_array_layers: 1,
                },
            );
        }
        if self.pending_scale.is_none() {
            self.producer
                .set_size(ROUND_ATLAS_SIZE as u32, ROUND_ATLAS_SIZE as u32, scale);
        }
        self.producer.reset();
        if coverage_only && needs_clear {
            // Initialize in the raster pass. The window compositor samples
            // alpha and applies its tint independently of the mask. Unused
            // regions of the original allocation are already zero in wgpu.
            self.producer
                .clear_rect(x, y, width, height, Color::rgbaf(0.0, 0.0, 0.0, 0.0));
        }
        self.producer.scale(scale, scale);
        self.producer.translate(x as f32 / scale, y as f32 / scale);
        let bounds = Rect::new(
            f32::from(key.phase[0]) / (256.0 * scale),
            f32::from(key.phase[1]) / (256.0 * scale),
            rectangle.shape.width,
            rectangle.shape.height,
        );
        match key.clip {
            RoundedTileClip::Shape | RoundedTileClip::Coverage => {
                self.producer
                    .scissor(bounds.x, bounds.y, bounds.width, bounds.height);
            }
            RoundedTileClip::Vertical { y, height } => {
                self.producer.scissor(
                    0.0,
                    f32::from_bits(y),
                    width as f32 / scale,
                    f32::from_bits(height),
                );
            }
        }
        if !coverage_only {
            self.producer.global_composite_blend_func_separate(
                BlendFactor::Zero,
                BlendFactor::One,
                BlendFactor::One,
                BlendFactor::OneMinusSrcAlpha,
            );
        }
        let mut path = Path::new();
        append_rect(&mut path, bounds, rectangle.radii);
        fill_rounded(
            &mut self.producer,
            &path,
            Color::rgbaf(1.0, 1.0, 1.0, rectangle.color.a),
            rectangle.opacity,
            rectangle.fractional,
        );
        self.pending_scale = Some(scale);
        self.new_tiles += 1;
        Some([x, y])
    }

    fn take_commands(&mut self) -> Option<wgpu::CommandBuffer> {
        self.warm_requests.clear();
        self.pending_scale.take()?;
        self.producer.flush_to_output(&self.texture)
    }

    fn submit(&mut self, queue: &wgpu::Queue) {
        if let Some(commands) = self.take_commands() {
            xrack_runtime::update_profile::submit_gpu();
            queue.submit([commands]);
        }
    }
}

#[derive(Default)]
pub(super) struct RectangleBatches {
    image: Option<ImageId>,
    colors: Vec<RGBA8>,
    uploaded_colors: Vec<RGBA8>,
    rounded: Option<RoundedAtlas>,
    pub(super) disabled: bool,
    pub(super) eager_atlas_submission: bool,
    pub(super) reset_fractional_tiles_each_frame: bool,
    pub(super) capsule_warmup_disabled: bool,
    pub(super) draws: u64,
    pub(super) palette_uploads: u64,
    pub(super) rounded_draws: u64,
}

impl RectangleBatches {
    fn window_atlas(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        scale: f32,
    ) -> Option<&mut RoundedAtlas> {
        if self.rounded.is_none() {
            self.rounded = RoundedAtlas::new(device, queue);
        }
        let atlas = self.rounded.as_mut()?;
        if atlas.pending_scale.is_some_and(|pending| pending != scale) {
            atlas.submit(queue);
        }
        Some(atlas)
    }

    pub(super) fn window_panel(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        panel: RoundedPanel,
        scale: f32,
    ) -> Option<[WindowLayer; 9]> {
        let atlas = self.window_atlas(device, queue, scale)?;
        let shape = panel.template;
        let rectangle = Solid {
            bounds: shape,
            shape,
            radii: panel.radii,
            color: Color::white(),
            opacity: 1.0,
            fractional: false,
        };
        let tile = atlas.quad(queue, rectangle, scale, RoundedTileClip::Coverage)?;
        let image = atlas.window_source();
        let spans = |start, extent, [near, far]: [f32; 2], [uv_start, uv_end]: [f32; 2]| {
            let end = start + extent;
            let near_edge = uv_start + near / ROUND_ATLAS_SIZE as f32;
            let far_edge = uv_end - far / ROUND_ATLAS_SIZE as f32;
            let middle = (near_edge + far_edge) * 0.5;
            [
                (start, start + near / scale, uv_start, near_edge),
                (start + near / scale, end - far / scale, middle, middle),
                (end - far / scale, end, far_edge, uv_end),
            ]
        };
        let [left, top, right, bottom] = panel.margins;
        let bounds = panel.bounds;
        let columns = spans(bounds.x, bounds.width, [left, right], [tile.s0, tile.s1]);
        let rows = spans(bounds.y, bounds.height, [top, bottom], [tile.t0, tile.t1]);
        Some(std::array::from_fn(|index| {
            let (x0, x1, s0, s1) = columns[index % 3];
            let (y0, y1, t0, t1) = rows[index / 3];
            WindowLayer::tinted_alpha(
                Quad {
                    x0,
                    y0,
                    x1,
                    y1,
                    s0,
                    t0,
                    s1,
                    t1,
                },
                image.clone(),
                panel.tint,
            )
        }))
    }

    pub(super) fn warm_capsules(&mut self, queue: &wgpu::Queue) {
        if self.capsule_warmup_disabled {
            return;
        }
        let Some(atlas) = &mut self.rounded else {
            return;
        };
        let Some(scale) = atlas.pending_scale else {
            return;
        };
        // Use only the budget left after all visible draws acquired tiles.
        // Warming joins the pending pass, with no eviction or separate submit.
        let mut requests = std::mem::take(&mut atlas.warm_requests);
        for (key, rectangle) in requests.drain(..) {
            atlas.warm_capsule_phases(queue, rectangle, scale, key);
        }
        atlas.warm_requests = requests;
    }

    pub(super) fn capsule_template_width(bounds: Rect, scale: f32) -> Option<f32> {
        let width = bounds.height + 2.0 / scale;
        (bounds.width >= width && width * scale <= MAX_ROUND_TILE && bounds.height * scale >= 2.0)
            .then_some(width)
    }

    pub(super) fn window_capsule(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        bounds: Rect,
        tint: Color,
        scale: f32,
        clip_y: [f32; 2],
    ) -> Option<[WindowLayer; 3]> {
        let radius = bounds.height * 0.5;
        let template_width = Self::capsule_template_width(bounds, scale)?;
        let left = ((bounds.x + radius) * scale).ceil() / scale;
        let right = ((bounds.x + bounds.width - radius) * scale).floor() / scale;
        let raster = logical_bounds(device_bounds(bounds, scale), scale);
        let [clip_top, clip_height] = clip_y;
        // Bake the actual intersection. A clipped capsule may need the
        // scissor on an edge that the full path could render on its own.
        let clip = RoundedTileClip::Vertical {
            y: (clip_top - raster.y).to_bits(),
            height: clip_height.to_bits(),
        };
        let middle = Rect::new(left, raster.y, right - left, raster.height);
        // Expanding or contracting a capsule only moves the caps. Keep the
        // cap's device-pixel phase and join each one inside the straight body.
        let atlas = self.window_atlas(device, queue, scale)?;
        let texture = atlas.window_source();
        let mut cap = |x| {
            let shape = Rect::new(x, bounds.y, template_width, bounds.height);
            let rectangle = Solid {
                bounds: shape,
                shape,
                radii: [radius; 4],
                color: tint,
                opacity: 1.0,
                fractional: !crate::pixel_aligned(shape, scale),
            };
            atlas.quad(queue, rectangle, scale, clip)
        };
        let left_clip = Rect::new(raster.x, raster.y, left - raster.x, raster.height);
        let right_clip = Rect::new(
            right,
            raster.y,
            raster.x + raster.width - right,
            raster.height,
        );
        let left_quad = cap(bounds.x)?;
        let right_quad = cap(bounds.x + bounds.width - template_width)?;
        let mut left_cap = WindowLayer::tinted_alpha(left_quad, texture.clone(), tint);
        left_cap.crop(left_clip);
        let mut right_cap = WindowLayer::tinted_alpha(right_quad, texture.clone(), tint);
        right_cap.crop(right_clip);

        // Sample the first complete column of the template's straight body.
        // All three parts can then share the same draw and sampling state.
        let column = left_quad.s0 + ((left - left_quad.x0) * scale + 0.5) / ROUND_ATLAS_SIZE as f32;
        let mut body = WindowLayer::tinted_alpha(left_quad, texture, tint);
        body.bounds = middle;
        body.uv = [column, left_quad.t0, column, left_quad.t1];
        Some([left_cap, body, right_cap])
    }

    pub(super) fn rounded_tile_rasterizations(&self) -> usize {
        self.rounded.as_ref().map_or(0, |atlas| atlas.new_tiles)
    }

    pub(super) fn take_rounded_commands(&mut self) -> Option<wgpu::CommandBuffer> {
        self.rounded.as_mut()?.take_commands()
    }

    pub(super) fn reset_capture_canvas(&mut self) {
        if let Some(atlas) = &mut self.rounded {
            atlas.capture_image = None;
        }
    }

    pub(super) fn begin_frame(&mut self, canvas_changed: bool) {
        if canvas_changed {
            self.image = None;
            self.uploaded_colors.clear();
            self.rounded = None;
        }
        if let Some(atlas) = &mut self.rounded {
            atlas.begin_frame(!self.reset_fractional_tiles_each_frame);
        }
        self.colors.clear();
    }

    pub(super) fn draw_squares(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        scale: f32,
        clip: Rect,
        rectangles: &mut Vec<Solid>,
    ) {
        let fractional = rectangles.iter().any(|rect| rect.fractional);
        debug_assert!(!fractional || has_pixel_aligned_scale(canvas_transform(canvas), scale));
        if (!fractional && rectangles.len() < MIN_BATCH_SIZE)
            || self.colors.len() + rectangles.len() > ATLAS_WIDTH * ATLAS_HEIGHT
        {
            for rectangle in rectangles.drain(..) {
                rectangle.draw(canvas);
            }
            return;
        }
        let image = self.image.or_else(|| {
            canvas
                .create_image_empty(
                    ATLAS_WIDTH,
                    ATLAS_HEIGHT,
                    PixelFormat::Rgba8,
                    ImageFlags::NEAREST,
                )
                .ok()
        });
        let Some(image) = image else {
            for rectangle in rectangles.drain(..) {
                rectangle.draw(canvas);
            }
            return;
        };
        self.image = Some(image);
        let mut quads = Vec::with_capacity(rectangles.len());
        for rectangle in rectangles.drain(..) {
            let needed = if rectangle.fractional { 9 } else { 1 };
            if self.colors.len() + needed <= ATLAS_WIDTH * ATLAS_HEIGHT
                && rectangle.square_batchable(clip, scale)
            {
                if !rectangle.fractional {
                    self.push_quad(&mut quads, rectangle.bounds, rectangle.tint());
                    continue;
                }
                let bounds = rectangle.bounds;
                let shape = rectangle.shape;
                let faded = rectangle.opacity < 1.0;
                let horizontal = coverage_spans(
                    [bounds.x * scale, (bounds.x + bounds.width) * scale],
                    faded.then_some([shape.x * scale, (shape.x + shape.width) * scale]),
                    scale,
                );
                let vertical = coverage_spans(
                    [bounds.y * scale, (bounds.y + bounds.height) * scale],
                    faded.then_some([shape.y * scale, (shape.y + shape.height) * scale]),
                    scale,
                );
                for (y, bottom, cy, clip_y) in vertical {
                    for (x, right, cx, clip_x) in horizontal {
                        // The convex fill's miter uses the nearer edge at
                        // the corners. A layer's clip has its own coverage.
                        let coverage = cx.min(cy);
                        if x >= right || y >= bottom || coverage == 0.0 {
                            continue;
                        }
                        let color = if faded {
                            captured_tint(
                                rectangle.color,
                                coverage,
                                rectangle.opacity * clip_x * clip_y,
                            )
                        } else {
                            let mut tint = rectangle.color;
                            tint.a *= coverage;
                            tint
                        };
                        if color.a > 0.0 {
                            let pixels = Rect::new(x, y, right - x, bottom - y);
                            self.push_quad(&mut quads, logical_bounds(pixels, scale), color);
                        }
                    }
                }
                continue;
            }
            // A draw outside the palette cannot move across its neighbors.
            self.draws += draw_quads(canvas, image, std::mem::take(&mut quads));
            rectangle.draw(canvas);
        }
        // FemtoVG's color-atlas draw preserves vertex order and samples
        // straight RGBA, premultiplying in the shader before blending.
        self.draws += draw_quads(canvas, image, quads);
    }

    fn push_quad(&mut self, quads: &mut Vec<Quad>, bounds: Rect, color: Color) {
        let slot = self.colors.len();
        let channel = |value: f32| (value * 255.0).round() as u8;
        let Color { r, g, b, a } = color;
        self.colors
            .push(RGBA8::new(channel(r), channel(g), channel(b), channel(a)));
        let s = ((slot % ATLAS_WIDTH) as f32 + 0.5) / ATLAS_WIDTH as f32;
        let t = ((slot / ATLAS_WIDTH) as f32 + 0.5) / ATLAS_HEIGHT as f32;
        quads.push(Quad {
            x0: bounds.x,
            y0: bounds.y,
            x1: bounds.x + bounds.width,
            y1: bounds.y + bounds.height,
            s0: s,
            t0: t,
            s1: s,
            t1: t,
        });
    }

    pub(super) fn upload(&mut self, canvas: &mut Canvas<WGPURenderer>) -> Result<(), String> {
        if let Some(image) = self.image
            && !self.uploaded_colors.starts_with(&self.colors)
        {
            let color_count = self.colors.len();
            let rows = color_count.div_ceil(ATLAS_WIDTH);
            self.colors
                .resize(rows * ATLAS_WIDTH, RGBA8::new(0, 0, 0, 0));
            xrack_runtime::update_profile::texture_upload(self.colors.len() * 4);
            let result = canvas.update_image(
                image,
                Img::new(self.colors.as_slice(), ATLAS_WIDTH, rows),
                0,
                0,
            );
            self.colors.truncate(color_count);
            result.map_err(|error| error.to_string())?;
            std::mem::swap(&mut self.colors, &mut self.uploaded_colors);
            self.palette_uploads += 1;
        }
        Ok(())
    }
}

impl Renderer {
    pub(super) fn draw_rounded_batch(
        &mut self,
        rectangles: &mut Vec<Solid>,
        scale: f32,
    ) -> Result<(), String> {
        if rectangles.len() < MIN_BATCH_SIZE {
            for rectangle in rectangles.drain(..) {
                rectangle.draw(&mut self.canvas);
            }
            return Ok(());
        }
        let capturing = self.using_capture_canvas();
        if self.rectangle_batches.rounded.is_none() {
            self.rectangle_batches.rounded = RoundedAtlas::new(&self.device, &self.queue);
        }
        let image = self.rectangle_batches.rounded.as_mut().and_then(|atlas| {
            let image = atlas.image(&mut self.canvas, capturing)?;
            if atlas.pending_scale.is_some_and(|pending| pending != scale) {
                atlas.submit(&self.queue);
            }
            Some(image)
        });
        let Some(image) = image else {
            for rectangle in rectangles.drain(..) {
                rectangle.draw(&mut self.canvas);
            }
            return Ok(());
        };

        // When the whole set cannot fit in the retained fractional atlas,
        // take its hits and send new captures to the existing transient atlas.
        let prefer_transient = self.can_capture_transient_rectangles(scale)
            && RoundedAtlas::exceeds_fractional_capacity(rectangles, scale)
            && self.prepare_opacity_atlas_for_rectangles(
                rectangles
                    .iter()
                    .filter(|rect| rect.fractional && rect.opacity < 1.0)
                    .map(|rect| rect.bounds),
                scale,
            );
        let mut quads = Vec::with_capacity(rectangles.len());
        let mut opacity_tiles = Vec::new();
        let mut rectangles = rectangles.drain(..);
        let mut prepared_opacity_atlas = prefer_transient;
        while let Some(rectangle) = rectangles.next() {
            let quad = self.rectangle_batches.rounded.as_mut().and_then(|atlas| {
                let admit = !(prefer_transient && rectangle.fractional && rectangle.opacity < 1.0);
                atlas.quad_with_admission(
                    &self.queue,
                    rectangle,
                    scale,
                    RoundedTileClip::Shape,
                    admit,
                )
            });
            if let Some(quad) = quad {
                self.draw_transient_rectangles(&mut opacity_tiles);
                quads.push(quad);
                continue;
            }
            // Keep the cached quads and the isolated draws in paint order.
            self.rectangle_batches.rounded_draws +=
                draw_quads(&mut self.canvas, image, std::mem::take(&mut quads));
            if rectangle.fractional && rectangle.opacity < 1.0 {
                if !prepared_opacity_atlas {
                    let bounds = std::iter::once(rectangle)
                        .chain(rectangles.as_slice().iter().copied())
                        .filter(|rect| rect.fractional && rect.opacity < 1.0)
                        .map(|rect| rect.bounds);
                    self.prepare_opacity_atlas_for_rectangles(bounds, scale);
                    prepared_opacity_atlas = true;
                }
                // Capture the unfaded fill. Opacity and the element scissor
                // apply to the premultiplied pixels at composition time.
                if let Some(tile) = self.capture_transient_rectangle(
                    rectangle.bounds,
                    scale,
                    rectangle.opacity,
                    |canvas| {
                        let mut path = Path::new();
                        append_rect(&mut path, rectangle.shape, rectangle.radii);
                        canvas.fill_path(&path, &Paint::color(rectangle.color));
                    },
                )? {
                    opacity_tiles.push(tile);
                    continue;
                }
            }
            self.draw_transient_rectangles(&mut opacity_tiles);
            rectangle.draw(&mut self.canvas);
        }
        self.draw_transient_rectangles(&mut opacity_tiles);
        self.rectangle_batches.rounded_draws += draw_quads(&mut self.canvas, image, quads);
        if self.rectangle_batches.eager_atlas_submission
            && let Some(atlas) = self.rectangle_batches.rounded.as_mut()
        {
            atlas.submit(&self.queue);
        }
        Ok(())
    }
}

fn draw_quads(canvas: &mut Canvas<WGPURenderer>, image_id: ImageId, quads: Vec<Quad>) -> u64 {
    if quads.is_empty() {
        return 0;
    }
    xrack_runtime::update_profile::quad_batch(quads.len());
    canvas.draw_glyph_commands(
        GlyphDrawCommands {
            color_glyphs: vec![DrawCommand { image_id, quads }],
            ..Default::default()
        },
        &Paint::color(Color::white()),
    );
    1
}
