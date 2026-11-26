use crate::{
    append_rect, border_path, brush_paint_with, color, pixel_phase,
    retained_layers::{device_bounds, logical_bounds},
};
use femtovg::{
    Canvas, Color, FillRule, ImageFilter, ImageFlags, ImageId, LayerEffects, MorphologyOperator,
    Paint, Path, renderer::WGPURenderer,
};
use xrack_layout::{LayoutNode, Point, Rect, corner_radii, normalize_radii};
use xrack_runtime::{Brush, Color as UiColor, Node, Value};

// Match the Gaussian chain's device-pixel limit and bound morphology to
// eight 24-pixel steps per axis.
const MAX_SIGMA_PX: f32 = 512.0;
const MAX_SPREAD_PX: f32 = 192.0;
// FemtoVG filters a larger sigma through a downsample pyramid.
const MAX_FULL_SIZE_SIGMA_PX: f32 = 8.0;
const MIN_INTERPOLATED_SIGMA_PX: f32 = 10.0;

struct Shadow {
    tint: UiColor,
    offset: [f32; 2],
    spread: f32,
    sigma: f32,
}

impl Shadow {
    fn from_node(node: &Node, scale: f32, inner: bool) -> Option<Self> {
        if node.kind != "Rectangle" {
            return None;
        }
        let [tint, x, y, blur, spread] = if inner {
            [
                "inner-shadow-color",
                "inner-shadow-offset-x",
                "inner-shadow-offset-y",
                "inner-shadow-blur",
                "inner-shadow-spread",
            ]
        } else {
            [
                "drop-shadow-color",
                "drop-shadow-offset-x",
                "drop-shadow-offset-y",
                "drop-shadow-blur",
                "drop-shadow-spread",
            ]
        };
        let tint = node.color(tint)?;
        if tint.0 & 0xff == 0 {
            return None;
        }
        let length = |name| node.length(name).unwrap_or_default();
        Some(Self {
            tint,
            offset: [length(x), length(y)],
            spread: length(spread),
            sigma: (length(blur).max(0.0) * scale * 0.5).min(MAX_SIGMA_PX),
        })
    }

    fn alpha(&self) -> f32 {
        (self.tint.0 & 0xff) as f32 / 255.0
    }

    fn opaque_color(&self) -> Color {
        color(UiColor(self.tint.0 | 0xff))
    }

    fn layer(&self, spread: f32) -> Option<LayerEffects> {
        let mut filters = Vec::new();
        if spread != 0.0 {
            filters.push(ImageFilter::Morphology {
                radius_x: spread.abs(),
                radius_y: spread.abs(),
                operator: if spread > 0.0 {
                    MorphologyOperator::Dilate
                } else {
                    MorphologyOperator::Erode
                },
            });
        }
        if self.sigma > 0.0 {
            filters.push(ImageFilter::gaussian_blur(self.sigma));
        }
        if !filters.is_empty() {
            // Correct RGB after filtering premultiplied coverage. The source
            // already has the shadow hue if the layer's filter budget runs out.
            let tint = self.opaque_color();
            filters.push(ImageFilter::ColorMatrix {
                matrix: [
                    0.0, 0.0, 0.0, 0.0, tint.r, 0.0, 0.0, 0.0, 0.0, tint.g, 0.0, 0.0, 0.0, 0.0,
                    tint.b, 0.0, 0.0, 0.0, 1.0, 0.0,
                ],
            });
        }
        (!filters.is_empty() || self.alpha() < 1.0).then(|| {
            LayerEffects::new()
                .with_opacity(self.alpha())
                .with_filters(&filters)
        })
    }
}

pub(crate) struct OuterShadow {
    shadow: Shadow,
    shape: Rect,
    radii: [f32; 4],
    fill: Option<Paint>,
    border: Option<(f32, Paint)>,
    source_alphas: Option<(u8, u8)>,
    spread_px: f32,
    reach: f32,
}

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct ShadowMaskKey {
    size: [u32; 2],
    pixels: [u32; 2],
    phase: [u8; 2],
    interpolated: bool,
    radii: [u32; 4],
    stroke: u32,
    filter: [u32; 3],
    source_alphas: (u8, u8),
}

#[derive(Clone, Copy)]
pub(super) struct ShadowTile {
    pub(super) key: ShadowMaskKey,
    pub(super) source_pixels: Rect,
    destination: Rect,
    sample: Rect,
}

impl ShadowTile {
    pub(super) fn sampling(self) -> ImageFlags {
        if self.key.interpolated {
            ImageFlags::empty()
        } else {
            ImageFlags::NEAREST
        }
    }
}

#[derive(Clone, Copy)]
pub(super) struct ShadowDraw {
    image: ImageId,
    bounds: Rect,
    sample: Rect,
    tint: Color,
}

impl ShadowDraw {
    pub(super) fn matches_translation(self, next: Self, offset: Point) -> bool {
        let translate = |rect: Rect| {
            Rect::new(
                rect.x + offset.x,
                rect.y + offset.y,
                rect.width,
                rect.height,
            )
        };
        self.image == next.image
            && self.tint == next.tint
            && translate(self.bounds) == next.bounds
            && translate(self.sample) == next.sample
    }

    pub(super) fn draw(self, canvas: &mut Canvas<WGPURenderer>) {
        let Rect {
            x,
            y,
            width,
            height,
        } = self.bounds;
        let mut path = Path::new();
        path.rect(x, y, width, height);
        let sample = self.sample;
        let mut paint = Paint::image_tint(
            self.image,
            sample.x,
            sample.y,
            sample.width,
            sample.height,
            0.0,
            self.tint,
        );
        paint.set_anti_alias(false);
        canvas.fill_path(&path, &paint);
    }
}

pub(super) struct ShadowPaint<'a> {
    pub(super) source: &'a OuterShadow,
    pub(super) clip: Rect,
    pub(super) mask: Option<ShadowDraw>,
}

impl ShadowPaint<'_> {
    pub(super) fn draw(&self, canvas: &mut Canvas<WGPURenderer>) {
        if let Some(mask) = self.mask {
            mask.draw(canvas);
        } else {
            self.source.draw(canvas, self.clip);
        }
    }
}

impl OuterShadow {
    pub(super) fn clipped_for_node(
        node: &Node,
        layout: &LayoutNode,
        scale: f32,
        fallback_border: UiColor,
    ) -> Option<(Self, Rect)> {
        let shadow = Self::for_node(
            node,
            layout.bounds,
            scale * layout.world_transform().max_scale(),
            fallback_border,
        )?;
        let clip = layout.clip.intersect(shadow.paint_bounds());
        nonempty(clip).then_some((shadow, clip))
    }

    pub(crate) fn for_node(
        node: &Node,
        bounds: Rect,
        scale: f32,
        fallback_border: UiColor,
    ) -> Option<Self> {
        if !nonempty(bounds) {
            return None;
        }
        let shadow = Shadow::from_node(node, scale, false)?;
        let tint = shadow.tint.0 & !0xff;
        let mask_color = |source: UiColor| color(UiColor(tint | (source.0 & 0xff)));
        let fill = has_coverage(node.properties.get("background"))
            .then(|| brush_paint_with(node, "background", bounds, mask_color))
            .flatten();
        let border_width = node
            .length("border-width")
            .unwrap_or_default()
            .clamp(0.0, bounds.width.min(bounds.height));
        let border = if border_width > 0.0 {
            let explicit = node.properties.get("border-color");
            if explicit.is_none() || has_coverage(explicit) {
                let paint = brush_paint_with(node, "border-color", bounds, mask_color)
                    .unwrap_or_else(|| Paint::color(mask_color(fallback_border)));
                Some((border_width, paint))
            } else {
                None
            }
        } else {
            None
        };
        if fill.is_none() && border.is_none() {
            return None;
        }
        let spread_px = (shadow.spread * scale)
            .clamp(-MAX_SPREAD_PX, MAX_SPREAD_PX)
            .round();
        let reach = ((3.0 * shadow.sigma + spread_px.max(0.0)).ceil() + 2.0) / scale;
        let fill_alpha = if fill.is_some() {
            uniform_alpha(node.properties.get("background"))
        } else {
            Some(0)
        };
        let border_alpha = if border.is_some() {
            node.properties.get("border-color").map_or_else(
                || Some(fallback_border.0 as u8),
                |value| uniform_alpha(Some(value)),
            )
        } else {
            Some(0)
        };
        Some(Self {
            shadow,
            shape: bounds,
            radii: corner_radii(node, bounds),
            fill,
            border,
            source_alphas: fill_alpha.zip(border_alpha),
            spread_px,
            reach,
        })
    }

    pub(crate) fn paint_bounds(&self) -> Rect {
        shifted(self.shape.inset(-self.reach), self.shadow.offset)
    }

    pub(crate) fn is_filtered(&self) -> bool {
        self.spread_px != 0.0 || self.shadow.sigma > 0.0
    }

    pub(super) fn initial_batch_tile(&self, scale: f32, clip: Rect) -> Option<ShadowTile> {
        // Each chunk shares the canvas's filter budget. Keep the work per
        // tile bounded to one full-size separable blur with no morphology.
        if self.spread_px != 0.0 || self.shadow.sigma > MAX_FULL_SIZE_SIGMA_PX {
            return None;
        }
        self.mask_tile(scale, clip)
    }

    pub(super) fn mask_tile(&self, scale: f32, clip: Rect) -> Option<ShadowTile> {
        let source_alphas = self.source_alphas?;
        let bounds = self.paint_bounds();
        // A cropped filter may use a different clipped input or blur pyramid.
        if !self.is_filtered()
            || clip.x > bounds.x
            || clip.y > bounds.y
            || clip.x + clip.width < bounds.x + bounds.width
            || clip.y + clip.height < bounds.y + bounds.height
        {
            return None;
        }
        let device = device_bounds(bounds, scale);
        // A low-alpha shadow from an opaque fill can reuse a full-size
        // Gaussian mask across fractional translations. Above an 8-pixel
        // sigma the filter downsamples the source, which needs the wider
        // threshold for phase-independent sampling.
        let alpha = self.shadow.alpha();
        let full_size = (7.25..=MAX_FULL_SIZE_SIGMA_PX).contains(&self.shadow.sigma)
            && alpha <= 0.4
            && (1.0..=2.0).contains(&scale)
            && source_alphas == (255, 0);
        let wide = self.shadow.sigma >= MIN_INTERPOLATED_SIGMA_PX && alpha <= 0.5;
        let interpolated = self.spread_px == 0.0 && (full_size || wide);
        let [width, height] = if interpolated {
            [self.shape.width, self.shape.height]
                .map(|size| ((size + 2.0 * self.reach) * scale).ceil() + 2.0)
        } else {
            [device.width, device.height]
        };
        if !(width > 0.0 && width <= 1024.0 && height > 0.0 && height <= 1024.0) {
            return None;
        }
        let destination = logical_bounds(device, scale);
        let source_pixels = Rect::new(0.0, 0.0, width, height);
        let source = logical_bounds(source_pixels, scale);
        let (phase, sample) = if interpolated {
            let guard = 1.0 / scale;
            (
                [0; 2],
                Rect::new(
                    bounds.x - guard,
                    bounds.y - guard,
                    source.width,
                    source.height,
                ),
            )
        } else {
            (
                [pixel_phase(bounds.x * scale), pixel_phase(bounds.y * scale)],
                destination,
            )
        };
        Some(ShadowTile {
            key: ShadowMaskKey {
                size: [self.shape.width, self.shape.height].map(f32::to_bits),
                pixels: [width as u32, height as u32],
                phase,
                interpolated,
                radii: self.radii.map(f32::to_bits),
                stroke: self
                    .border
                    .as_ref()
                    .map_or(0.0, |(width, _)| *width)
                    .to_bits(),
                filter: [self.spread_px, self.shadow.sigma, scale].map(f32::to_bits),
                source_alphas,
            },
            source_pixels,
            destination,
            sample,
        })
    }

    pub(super) fn draw_mask(
        &self,
        canvas: &mut Canvas<WGPURenderer>,
        tile: ShadowTile,
        scale: f32,
    ) {
        if let Some(mask) = self.mask_source(tile, scale) {
            mask.draw(canvas, mask.paint_bounds());
        }
    }

    // Initial batches bind each tile as an image render target. Filtering
    // premultiplied white coverage in that image keeps RGB equal to alpha
    // throughout the blur and avoids an extra layer capture and composite.
    pub(super) fn draw_mask_into_image(
        &self,
        canvas: &mut Canvas<WGPURenderer>,
        tile: ShadowTile,
        scale: f32,
        image: ImageId,
    ) {
        let Some(mask) = self.mask_source(tile, scale) else {
            return;
        };
        mask.paint_shape(canvas);
        if canvas
            .filter_image_chain(
                image,
                &[ImageFilter::gaussian_blur(self.shadow.sigma)],
                image,
            )
            .is_err()
        {
            // The rejected filter leaves the unblurred source where it was.
            // Discard it before retrying via the general layer painter.
            canvas.clear_rect(
                0,
                0,
                canvas.width(),
                canvas.height(),
                Color::rgba(0, 0, 0, 0),
            );
            mask.draw(canvas, mask.paint_bounds());
        }
    }

    fn mask_source(&self, tile: ShadowTile, scale: f32) -> Option<Self> {
        let (fill, border) = self.source_alphas?;
        let [x, y] = if tile.key.interpolated {
            [1.0 / scale; 2]
        } else {
            tile.key
                .phase
                .map(|phase| f32::from(phase) / (256.0 * scale))
        };
        let white = |alpha| Paint::color(Color::rgba(255, 255, 255, alpha));
        Some(Self {
            shadow: Shadow {
                tint: UiColor(0xffff_ffff),
                offset: [0.0; 2],
                spread: self.shadow.spread,
                sigma: self.shadow.sigma,
            },
            shape: Rect::new(
                self.reach + x,
                self.reach + y,
                self.shape.width,
                self.shape.height,
            ),
            radii: self.radii,
            fill: (fill != 0).then(|| white(fill)),
            border: self
                .border
                .as_ref()
                .map(|(width, _)| (*width, white(border))),
            source_alphas: self.source_alphas,
            spread_px: self.spread_px,
            reach: self.reach,
        })
    }

    pub(super) fn masked_draw(&self, image: ImageId, tile: ShadowTile) -> ShadowDraw {
        ShadowDraw {
            image,
            bounds: tile.destination,
            sample: tile.sample,
            tint: color(self.shadow.tint),
        }
    }

    pub(crate) fn draw(&self, canvas: &mut Canvas<WGPURenderer>, clip: Rect) {
        canvas.save();
        canvas.intersect_scissor(clip.x, clip.y, clip.width, clip.height);
        filtered(canvas, self.shadow.layer(self.spread_px), |canvas| {
            canvas.translate(self.shadow.offset[0], self.shadow.offset[1]);
            self.paint_shape(canvas);
        });
        canvas.restore();
    }

    fn paint_shape(&self, canvas: &mut Canvas<WGPURenderer>) {
        if let Some(fill) = &self.fill {
            let mut path = Path::new();
            append_rect(&mut path, self.shape, self.radii);
            canvas.fill_path(&path, fill);
        }
        if let Some((width, paint)) = &self.border {
            canvas.fill_path(&border_path(self.shape, self.radii, *width), paint);
        }
    }
}

pub(crate) fn draw_inner(
    canvas: &mut Canvas<WGPURenderer>,
    node: &Node,
    bounds: Rect,
    radii: [f32; 4],
    scale: f32,
) {
    let Some(shadow) = Shadow::from_node(node, scale, true) else {
        return;
    };
    if shadow.sigma == 0.0 && shadow.spread == 0.0 && shadow.offset == [0.0, 0.0] {
        return;
    }
    let hole = shifted(bounds.inset(shadow.spread), shadow.offset);
    // Cover all pixels that the Gaussian can sample inside the rounded box.
    let pad = ((shadow.sigma * 3.0).ceil() + 2.0) / scale;
    let guard = union(bounds.inset(-pad), hole);
    let mut inverse = Path::new();
    inverse.rect(guard.x, guard.y, guard.width, guard.height);
    if nonempty(hole) {
        let hole_radii =
            normalize_radii(hole, radii.map(|radius| (radius - shadow.spread).max(0.0)));
        append_rect(&mut inverse, hole, hole_radii);
    }
    let mut paint = Paint::color(shadow.opaque_color());
    paint.set_fill_rule(FillRule::EvenOdd);

    let mut clip = Path::new();
    append_rect(&mut clip, bounds, radii);
    canvas.save();
    canvas.clip_path(&clip, FillRule::NonZero);
    filtered(canvas, shadow.layer(0.0), |canvas| {
        canvas.fill_path(&inverse, &paint)
    });
    canvas.restore();
}

fn filtered(
    canvas: &mut Canvas<WGPURenderer>,
    layer: Option<LayerEffects>,
    draw: impl FnOnce(&mut Canvas<WGPURenderer>),
) {
    if let Some(effects) = layer {
        let _captured = canvas.begin_layer(&effects);
        draw(canvas);
        canvas.end_layer();
    } else {
        draw(canvas);
    }
}

fn has_coverage(value: Option<&Value>) -> bool {
    match value {
        Some(Value::Color(color) | Value::Brush(Brush::Solid(color))) => color.0 & 0xff != 0,
        Some(Value::Brush(Brush::Linear { stops, .. } | Brush::Radial { stops })) => {
            stops.iter().any(|stop| stop.color.0 & 0xff != 0)
        }
        _ => false,
    }
}

fn uniform_alpha(value: Option<&Value>) -> Option<u8> {
    match value {
        Some(Value::Color(color) | Value::Brush(Brush::Solid(color))) => Some(color.0 as u8),
        Some(Value::Brush(Brush::Linear { stops, .. } | Brush::Radial { stops })) => {
            let alpha = stops.first()?.color.0 as u8;
            stops
                .iter()
                .all(|stop| stop.color.0 as u8 == alpha)
                .then_some(alpha)
        }
        _ => Some(0),
    }
}

pub(crate) fn nonempty(rect: Rect) -> bool {
    rect.width > 0.0 && rect.height > 0.0
}

pub(crate) fn union(left: Rect, right: Rect) -> Rect {
    if !nonempty(left) {
        return right;
    }
    if !nonempty(right) {
        return left;
    }
    let x = left.x.min(right.x);
    let y = left.y.min(right.y);
    Rect::new(
        x,
        y,
        (left.x + left.width).max(right.x + right.width) - x,
        (left.y + left.height).max(right.y + right.height) - y,
    )
}

fn shifted(bounds: Rect, [dx, dy]: [f32; 2]) -> Rect {
    Rect::new(bounds.x + dx, bounds.y + dy, bounds.width, bounds.height)
}
