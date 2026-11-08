use crate::{brush_paint, image::fitted_rect};
use femtovg::{Canvas, Path, renderer::WGPURenderer};
use xrack_assets::{PathSegment, parse_path};
use xrack_layout::{Rect, Size};
use xrack_runtime::Node;

pub(crate) fn draw_path(
    canvas: &mut Canvas<WGPURenderer>,
    node: &Node,
    bounds: Rect,
) -> Result<(), String> {
    let Some(commands) = node.text("commands") else {
        return Ok(());
    };
    let Some(source) = parse_path(commands).map_err(|error| {
        format!(
            "invalid Path commands on {}: {error}",
            node.id.unwrap_or("an anonymous element")
        )
    })?
    else {
        return Ok(());
    };

    let stroke = node.length("stroke-width").unwrap_or(0.0).max(0.0);
    let explicit_width = node.float("viewbox-width").unwrap_or(0.0);
    let explicit_height = node.float("viewbox-height").unwrap_or(0.0);
    let (viewbox, target) = if explicit_width > 0.0 && explicit_height > 0.0 {
        (
            Rect::new(
                node.float("viewbox-x").unwrap_or(0.0),
                node.float("viewbox-y").unwrap_or(0.0),
                explicit_width,
                explicit_height,
            ),
            bounds,
        )
    } else {
        let extents = source
            .compute_tight_bounds()
            .unwrap_or_else(|| source.bounds());
        let width = if extents.width() == 0.0 {
            1.0
        } else {
            extents.width()
        };
        let height = if extents.height() == 0.0 {
            1.0
        } else {
            extents.height()
        };
        (
            // A single horizontal or vertical line still has a useful stroked extent.
            Rect::new(
                extents.x() - (width - extents.width()) * 0.5,
                extents.y() - (height - extents.height()) * 0.5,
                width,
                height,
            ),
            bounds.inset(stroke * 0.5),
        )
    };
    if target.width <= 0.0 || target.height <= 0.0 {
        return Ok(());
    }
    let fit = node.image_fit("fit").unwrap_or_default();
    let destination = fitted_rect(target, Size::new(viewbox.width, viewbox.height), fit);
    let sx = destination.width / viewbox.width;
    let sy = destination.height / viewbox.height;
    let map = |x: f32, y: f32| {
        let x = destination.x + (x - viewbox.x) * sx;
        let y = destination.y + (y - viewbox.y) * sy;
        if x.abs() <= 16_000_000.0 && y.abs() <= 16_000_000.0 {
            Ok((x, y))
        } else {
            Err("Path coordinates exceed the drawable range after applying the viewbox")
        }
    };

    let mut path = Path::new();
    for segment in source.segments() {
        match segment {
            PathSegment::MoveTo(p) => {
                let (x, y) = map(p.x, p.y)?;
                path.move_to(x, y);
            }
            PathSegment::LineTo(p) => {
                let (x, y) = map(p.x, p.y)?;
                path.line_to(x, y);
            }
            PathSegment::QuadTo(c, p) => {
                let (cx, cy) = map(c.x, c.y)?;
                let (x, y) = map(p.x, p.y)?;
                path.quad_to(cx, cy, x, y);
            }
            PathSegment::CubicTo(c1, c2, p) => {
                let (c1x, c1y) = map(c1.x, c1.y)?;
                let (c2x, c2y) = map(c2.x, c2.y)?;
                let (x, y) = map(p.x, p.y)?;
                path.bezier_to(c1x, c1y, c2x, c2y, x, y);
            }
            PathSegment::Close => path.close(),
        }
    }
    if let Some(fill) = brush_paint(node, "fill", bounds, 0.0) {
        canvas.fill_path(&path, &fill);
    }
    if stroke > 0.0
        && let Some(mut paint) = brush_paint(node, "stroke", bounds, 0.0)
    {
        paint.set_line_width(stroke);
        canvas.stroke_path(&path, &paint);
    }
    Ok(())
}
