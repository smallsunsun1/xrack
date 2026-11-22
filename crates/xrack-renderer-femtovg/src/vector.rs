use crate::brush_paint;
use femtovg::{Canvas, Path, renderer::WGPURenderer};
use std::{
    collections::{HashMap, VecDeque, hash_map::RandomState},
    hash::BuildHasher,
};
use xrack_assets::{parse_path, vector_bounds, vector_segments};
use xrack_layout::{Rect, Size, fitted_rect};
use xrack_runtime::{ImageFit, Node, VectorSegment};

const MAX_RETAINED_PATHS: usize = 512;
const MAX_RETAINED_BYTES: usize = 4 * 1024 * 1024;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) struct GeometryKey {
    commands: u64,
    dimensions: [u32; 9],
    fit: u8,
}

pub(super) struct Source {
    commands: Box<str>,
    source_geometry: Option<([f32; 4], Box<[VectorSegment]>)>,
}

impl Source {
    pub(super) fn matches(&self, path: &Geometry<'_>) -> bool {
        &*self.commands == path.commands
            && self
                .source_geometry
                .as_ref()
                .map(|(bounds, segments)| (*bounds, &**segments))
                == path.source_geometry
    }
}

pub(super) struct Geometry<'a> {
    pub(super) node: &'a Node,
    commands: &'a str,
    source_geometry: Option<([f32; 4], &'a [VectorSegment])>,
    viewbox: Rect,
    fit: ImageFit,
    pub(super) stroke: f32,
}

impl<'a> Geometry<'a> {
    pub(super) fn for_node(node: &'a Node) -> Option<Self> {
        let commands = node.text("commands")?;
        Some(Self {
            node,
            commands,
            source_geometry: node.vector_path_geometry(commands),
            stroke: node.length("stroke-width").unwrap_or(0.0).max(0.0),
            viewbox: Rect::new(
                node.float("viewbox-x").unwrap_or(0.0),
                node.float("viewbox-y").unwrap_or(0.0),
                node.float("viewbox-width").unwrap_or(0.0),
                node.float("viewbox-height").unwrap_or(0.0),
            ),
            fit: node.image_fit("fit").unwrap_or_default(),
        })
    }

    pub(super) fn key(&self, hash: &RandomState, bounds: Rect) -> GeometryKey {
        GeometryKey {
            commands: hash.hash_one(self.commands),
            dimensions: [
                bounds.x,
                bounds.y,
                bounds.width,
                bounds.height,
                self.viewbox.x,
                self.viewbox.y,
                self.viewbox.width,
                self.viewbox.height,
                self.stroke,
            ]
            .map(f32::to_bits),
            fit: self.fit as u8,
        }
    }

    pub(super) fn source(&self) -> Source {
        Source {
            commands: self.commands.into(),
            source_geometry: self
                .source_geometry
                .map(|(bounds, segments)| (bounds, segments.into())),
        }
    }

    pub(super) fn source_size(&self) -> usize {
        self.commands.len().saturating_add(
            self.source_geometry
                .map_or(0, |(_, segments)| std::mem::size_of_val(segments)),
        )
    }

    pub(super) fn warrants_mask(&self) -> bool {
        self.commands.len() >= 120
            && self.source_size() <= 32 * 1024
            && self
                .source_geometry
                .is_none_or(|(_, segments)| segments.len() >= 16)
    }

    pub(super) fn build(&self, bounds: Rect) -> Result<Option<Path>, String> {
        build_path(
            self.node,
            self.commands,
            self.source_geometry,
            bounds,
            self.fit,
            self.viewbox,
            self.stroke,
        )
    }
}

struct RetainedPath {
    source: Source,
    path: Path,
    bytes: usize,
}

/// Keep FemtoVG's paths (and their flattened contours) across repaints.
#[derive(Default)]
pub(crate) struct Paths {
    hash: RandomState,
    entries: HashMap<GeometryKey, RetainedPath>,
    order: VecDeque<GeometryKey>,
    bytes: usize,
    pub(super) disabled: bool,
}

impl Paths {
    pub(crate) fn set_enabled(&mut self, enabled: bool) {
        if enabled == self.disabled {
            *self = Self {
                disabled: !enabled,
                ..Self::default()
            };
        }
    }

    pub(super) fn draw(
        &mut self,
        canvas: &mut Canvas<WGPURenderer>,
        geometry: &Geometry<'_>,
        bounds: Rect,
    ) -> Result<(), String> {
        if self.disabled {
            if let Some(path) = geometry.build(bounds)? {
                paint_path(canvas, geometry.node, bounds, geometry.stroke, &path);
            }
            return Ok(());
        }
        let key = geometry.key(&self.hash, bounds);
        if let Some(retained) = self.entries.get(&key)
            && retained.source.matches(geometry)
        {
            paint_path(
                canvas,
                geometry.node,
                bounds,
                geometry.stroke,
                &retained.path,
            );
            return Ok(());
        }
        if let Some(path) = geometry.build(bounds)? {
            paint_path(canvas, geometry.node, bounds, geometry.stroke, &path);
            self.retain(key, geometry, path);
        }
        Ok(())
    }

    fn retain(&mut self, key: GeometryKey, geometry: &Geometry<'_>, path: Path) {
        let bytes = path.size().saturating_add(geometry.source_size());
        if bytes > MAX_RETAINED_BYTES / 8 {
            return;
        }
        if let Some(replaced) = self.entries.remove(&key) {
            self.bytes -= replaced.bytes;
            self.order.retain(|entry| *entry != key);
        }
        while self.entries.len() >= MAX_RETAINED_PATHS || self.bytes + bytes > MAX_RETAINED_BYTES {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            if let Some(entry) = self.entries.remove(&oldest) {
                self.bytes -= entry.bytes;
            }
        }
        self.bytes += bytes;
        self.order.push_back(key);
        self.entries.insert(
            key,
            RetainedPath {
                source: geometry.source(),
                path,
                bytes,
            },
        );
    }
}

fn build_path(
    node: &Node,
    commands: &str,
    source_geometry: Option<([f32; 4], &[VectorSegment])>,
    bounds: Rect,
    fit: ImageFit,
    viewbox: Rect,
    stroke: f32,
) -> Result<Option<Path>, String> {
    if let Some((extents, segments)) = source_geometry {
        if segments.is_empty() {
            return Ok(None);
        }
        return map_segments(
            bounds,
            fit,
            viewbox,
            stroke,
            segments.iter().copied(),
            || extents,
        );
    }
    let Some(source) = parse_path(commands).map_err(|error| {
        format!(
            "invalid Path commands on {}: {error}",
            node.element_id().unwrap_or("an anonymous element")
        )
    })?
    else {
        return Ok(None);
    };
    map_segments(
        bounds,
        fit,
        viewbox,
        stroke,
        vector_segments(&source),
        || vector_bounds(&source),
    )
}

fn map_segments(
    bounds: Rect,
    fit: ImageFit,
    explicit_viewbox: Rect,
    stroke: f32,
    segments: impl Iterator<Item = VectorSegment>,
    extents: impl FnOnce() -> [f32; 4],
) -> Result<Option<Path>, String> {
    let (viewbox, target) = if explicit_viewbox.width > 0.0 && explicit_viewbox.height > 0.0 {
        (explicit_viewbox, bounds)
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
        return Ok(None);
    }
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
    Ok(Some(path))
}

fn paint_path(
    canvas: &mut Canvas<WGPURenderer>,
    node: &Node,
    bounds: Rect,
    stroke: f32,
    path: &Path,
) {
    if let Some(fill) = brush_paint(node, "fill", bounds, 0.0) {
        canvas.fill_path(path, &fill);
    }
    if stroke > 0.0
        && let Some(mut paint) = brush_paint(node, "stroke", bounds, 0.0)
    {
        paint.set_line_width(stroke);
        canvas.stroke_path(path, &paint);
    }
}
