use std::collections::BTreeMap;

use lyon_tessellation::{
    FillOptions, FillRule, FillTessellator, VertexBuffers,
    geometry_builder::simple_builder,
    math::{Point, point},
    path::{Path, PathEvent, iterator::PathIterator, path::Builder},
};
use ttf_parser::OutlineBuilder;

use crate::scene3d::mesh::primitives::{PrimitiveParts, vertex};

const MAX_OUTLINE_EDGES: usize = 24_000;

pub(super) type FilledOutline = VertexBuffers<Point, u16>;

pub(super) struct Outlines {
    builder: Builder,
    edge_count: usize,
}

impl Outlines {
    pub(super) fn new() -> Self {
        Self {
            builder: Path::builder(),
            edge_count: 0,
        }
    }

    pub(super) fn add_glyph(
        &mut self,
        face: &ttf_parser::Face<'_>,
        glyph_id: ttf_parser::GlyphId,
        origin: [f32; 2],
        scale: f32,
        tolerance: f32,
    ) -> Option<()> {
        let mut glyph = GlyphPath {
            builder: Path::builder(),
            origin,
            scale,
            open: false,
        };
        if face.outline_glyph(glyph_id, &mut glyph).is_none() {
            return Some(());
        }
        glyph.close();
        let glyph = glyph.builder.build();
        let mut contours: Vec<Vec<Point>> = Vec::new();
        let mut area = 0.0;
        for event in glyph.iter().flattened(tolerance) {
            match event {
                PathEvent::Begin { at } => contours.push(vec![at]),
                PathEvent::Line { from, to } => {
                    let contour = contours.last_mut()?;
                    if from.distance_to(to) > 1e-7 {
                        self.edge_count += 1;
                        if self.edge_count > MAX_OUTLINE_EDGES {
                            return None;
                        }
                        area += from.x * to.y - from.y * to.x;
                        contour.push(to);
                    }
                }
                PathEvent::End { last, first, .. } => {
                    area += last.x * first.y - last.y * first.x;
                }
                _ => {}
            }
        }
        // TrueType and CFF outlines may use opposite exterior winding. Keep
        // glyph counters while giving overlaps between glyphs the same fill sign.
        for mut contour in contours.into_iter().filter(|points| points.len() >= 3) {
            if area < 0.0 {
                contour.reverse();
            }
            self.builder.begin(contour[0]);
            for point in contour[1..].iter().copied() {
                self.builder.line_to(point);
            }
            self.builder.end(true);
        }
        Some(())
    }

    pub(super) fn fill(self) -> Option<FilledOutline> {
        let mut tessellated: VertexBuffers<Point, u16> = VertexBuffers::new();
        FillTessellator::new()
            .tessellate_path(
                &self.builder.build(),
                &FillOptions::default().with_fill_rule(FillRule::NonZero),
                &mut simple_builder(&mut tessellated),
            )
            .ok()?;
        (!tessellated.indices.is_empty() && tessellated.indices.len() <= super::MAX_TEXT_VERTICES)
            .then_some(tessellated)
    }
}

struct GlyphPath {
    builder: Builder,
    origin: [f32; 2],
    scale: f32,
    open: bool,
}

impl GlyphPath {
    fn point(&self, x: f32, y: f32) -> Point {
        point(
            self.origin[0] + self.scale * x,
            self.origin[1] + self.scale * y,
        )
    }
}

impl OutlineBuilder for GlyphPath {
    fn move_to(&mut self, x: f32, y: f32) {
        self.close();
        self.builder.begin(self.point(x, y));
        self.open = true;
    }

    fn line_to(&mut self, x: f32, y: f32) {
        if self.open {
            self.builder.line_to(self.point(x, y));
        }
    }

    fn quad_to(&mut self, x1: f32, y1: f32, x: f32, y: f32) {
        if self.open {
            self.builder
                .quadratic_bezier_to(self.point(x1, y1), self.point(x, y));
        }
    }

    fn curve_to(&mut self, x1: f32, y1: f32, x2: f32, y2: f32, x: f32, y: f32) {
        if self.open {
            self.builder
                .cubic_bezier_to(self.point(x1, y1), self.point(x2, y2), self.point(x, y));
        }
    }

    fn close(&mut self) {
        if self.open {
            self.builder.end(true);
            self.open = false;
        }
    }
}

pub(super) fn extrude_fill(fill: &FilledOutline, scale: f32, depth: f32) -> Option<PrimitiveParts> {
    if fill.indices.is_empty() || fill.indices.len() > super::MAX_TEXT_VERTICES {
        return None;
    }
    let bounds = fill.vertices.iter().fold(
        ([f32::INFINITY; 2], [f32::NEG_INFINITY; 2]),
        |(min, max), point| {
            (
                [min[0].min(point.x), min[1].min(point.y)],
                [max[0].max(point.x), max[1].max(point.y)],
            )
        },
    );
    let uv = |point: Point| {
        [
            (point.x - bounds.0[0]) / (bounds.1[0] - bounds.0[0]).max(1e-6),
            1.0 - (point.y - bounds.0[1]) / (bounds.1[1] - bounds.0[1]).max(1e-6),
        ]
    };
    let face = |index: u16, z, normal| {
        let p = fill.vertices[usize::from(index)];
        vertex([p.x * scale, p.y * scale, z], normal, uv(p))
    };
    let mut front = Vec::with_capacity(fill.indices.len());
    let mut back = Vec::new();
    let mut edges = BTreeMap::new();
    for &[a, b, c] in fill.indices.as_chunks::<3>().0 {
        let [a_pt, b_pt, c_pt] = [a, b, c].map(|index| fill.vertices[usize::from(index)]);
        let winding = (b_pt - a_pt).cross(c_pt - a_pt);
        if winding.abs() <= 1e-12 {
            continue;
        }
        let [a, b, c] = if winding > 0.0 { [a, b, c] } else { [a, c, b] };
        front.extend([a, b, c].map(|index| face(index, depth, [0.0, 0.0, 1.0])));
        if depth > 0.0 {
            back.extend([a, c, b].map(|index| face(index, 0.0, [0.0, 0.0, -1.0])));
            for (from, to) in [(a, b), (b, c), (c, a)] {
                let key = (from.min(to), from.max(to));
                if edges.remove(&key).is_none() {
                    edges.insert(key, (from, to));
                }
            }
        }
    }
    if front.is_empty() || front.len() + back.len() + edges.len() * 6 > super::MAX_TEXT_VERTICES {
        return None;
    }
    let mut parts = vec![("front", front)];
    if !back.is_empty() {
        parts.push(("back", back));
    }
    let mut sides = Vec::with_capacity(edges.len() * 6);
    for (a, b) in edges.into_values() {
        let p = fill.vertices[usize::from(a)];
        let q = fill.vertices[usize::from(b)];
        let d = q - p;
        let len = d.length();
        if len <= 1e-7 {
            continue;
        }
        let normal = [d.y / len, -d.x / len, 0.0];
        let axis = usize::from(d.y.abs() > d.x.abs());
        let side = |point: Point, z, v| {
            vertex(
                [point.x * scale, point.y * scale, z],
                normal,
                [uv(point)[axis], v],
            )
        };
        let [a0, b0, a1, b1] = [
            side(p, 0.0, 1.0),
            side(q, 0.0, 1.0),
            side(p, depth, 0.0),
            side(q, depth, 0.0),
        ];
        sides.extend([a0, b0, b1, a0, b1, a1]);
    }
    if !sides.is_empty() {
        parts.push(("side", sides));
    }
    Some(parts)
}
