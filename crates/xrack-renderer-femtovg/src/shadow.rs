use crate::{append_rect, border_path, brush_paint_with, color};
use femtovg::{
    Canvas, Color, FillRule, ImageFilter, LayerEffects, MorphologyOperator, Paint, Path,
    renderer::WGPURenderer,
};
use xrack_layout::{Rect, corner_radii, normalize_radii};
use xrack_runtime::{Brush, Color as UiColor, Node, Value};

// Match the Gaussian chain's device-pixel limit and bound morphology to
// eight 24-pixel steps per axis.
const MAX_SIGMA_PX: f32 = 512.0;
const MAX_SPREAD_PX: f32 = 192.0;

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
    spread_px: f32,
    reach: f32,
}

impl OuterShadow {
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
        Some(Self {
            shadow,
            shape: bounds,
            radii: corner_radii(node, bounds),
            fill,
            border,
            spread_px,
            reach,
        })
    }

    pub(crate) fn paint_bounds(&self) -> Rect {
        shifted(self.shape.inset(-self.reach), self.shadow.offset)
    }

    pub(crate) fn draw(&self, canvas: &mut Canvas<WGPURenderer>, clip: Rect) {
        canvas.save();
        canvas.intersect_scissor(clip.x, clip.y, clip.width, clip.height);
        filtered(canvas, self.shadow.layer(self.spread_px), |canvas| {
            canvas.translate(self.shadow.offset[0], self.shadow.offset[1]);
            if let Some(fill) = &self.fill {
                let mut path = Path::new();
                append_rect(&mut path, self.shape, self.radii);
                canvas.fill_path(&path, fill);
            }
            if let Some((width, paint)) = &self.border {
                canvas.fill_path(&border_path(self.shape, self.radii, *width), paint);
            }
        });
        canvas.restore();
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
