use crate::brush_paint;
use femtovg::{Canvas, Path, renderer::WGPURenderer};
use xrack_assets::{parse_path, vector_bounds, vector_segments};
use xrack_layout::fitted_rect;
use xrack_layout::{Rect, Size};
use xrack_runtime::{Node, VectorSegment};

pub(crate) fn draw_path(
    canvas: &mut Canvas<WGPURenderer>,
    node: &Node,
    bounds: Rect,
) -> Result<(), String> {
    let Some(commands) = node.text("commands") else {
        return Ok(());
    };
    if let Some((extents, segments)) = node.vector_path_geometry(commands) {
        if segments.is_empty() {
            return Ok(());
        }
        return draw_segments(canvas, node, bounds, segments.iter().copied(), || extents);
    }
    let Some(source) = parse_path(commands).map_err(|error| {
        format!(
            "invalid Path commands on {}: {error}",
            node.element_id().unwrap_or("an anonymous element")
        )
    })?
    else {
        return Ok(());
    };
    draw_segments(canvas, node, bounds, vector_segments(&source), || {
        vector_bounds(&source)
    })
}

fn draw_segments(
    canvas: &mut Canvas<WGPURenderer>,
    node: &Node,
    bounds: Rect,
    segments: impl Iterator<Item = VectorSegment>,
    extents: impl FnOnce() -> [f32; 4],
) -> Result<(), String> {
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
        let [x, y, source_width, source_height] = extents();
        let width = if source_width == 0.0 {
            1.0
        } else {
            source_width
        };
        let height = if source_height == 0.0 {
            1.0
        } else {
            source_height
        };
        (
            // A single horizontal or vertical line still has a useful stroked extent.
            Rect::new(
                x - (width - source_width) * 0.5,
                y - (height - source_height) * 0.5,
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
    let destination = fitted_rect(
        target,
        Size::new(viewbox.width, viewbox.height),
        fit,
        [0.5; 2],
    );
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
    for segment in segments {
        match segment {
            VectorSegment::MoveTo([x, y]) => {
                let (x, y) = map(x, y)?;
                path.move_to(x, y);
            }
            VectorSegment::LineTo([x, y]) => {
                let (x, y) = map(x, y)?;
                path.line_to(x, y);
            }
            VectorSegment::QuadTo([cx, cy], [x, y]) => {
                let (cx, cy) = map(cx, cy)?;
                let (x, y) = map(x, y)?;
                path.quad_to(cx, cy, x, y);
            }
            VectorSegment::CubicTo([c1x, c1y], [c2x, c2y], [x, y]) => {
                let (c1x, c1y) = map(c1x, c1y)?;
                let (c2x, c2y) = map(c2x, c2y)?;
                let (x, y) = map(x, y)?;
                path.bezier_to(c1x, c1y, c2x, c2y, x, y);
            }
            VectorSegment::Close => path.close(),
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
