//! Screen-space point and line coverage shared by pointer hits and marquee selection.

use nalgebra::{Vector2, Vector3};

use crate::{Length, Point2D};

use super::{
    MeshPart, MeshTopology, MeshVertex, Ray3D, SceneCamera,
    bounds::{Bounds3D, BoundsTree},
    pick,
    visibility::Frustum,
};

#[derive(Clone, Copy)]
pub(super) struct RasterView<'a> {
    camera: &'a SceneCamera,
    scale: Vector2<f64>,
}

impl<'a> RasterView<'a> {
    pub fn new(camera: &'a SceneCamera, scale: [f64; 2]) -> Self {
        Self {
            camera,
            scale: scale.into(),
        }
    }

    fn pixels(self, point: Point2D) -> Vector2<f64> {
        Vector2::new(point.x.0, point.y.0).component_mul(&self.scale)
    }

    fn extent(self) -> Vector2<f64> {
        let [x, y, width, height] = self.camera.content_rect();
        Vector2::new(width + 2.0 * x, height + 2.0 * y).component_mul(&self.scale)
    }

    pub fn point_region(self, point: Point2D, size: f32) -> Option<Frustum> {
        let p = self.pixels(point);
        self.candidate_region(ScreenRect { lo: p, hi: p }, size)
    }

    pub(super) fn candidate_region(self, rect: ScreenRect, size: f32) -> Option<Frustum> {
        let radius = self.radius(size);
        let logical = |p: Vector2<f64>| {
            let p = p.component_div(&self.scale);
            Point2D::new(Length(p.x), Length(p.y))
        };
        Frustum::selection(
            self.camera,
            [logical(rect.lo - radius), logical(rect.hi + radius)],
        )
    }

    pub(super) fn rectangle(self, [from, to]: [Point2D; 2]) -> Option<ScreenRect> {
        let [x, y, width, height] = self.camera.content_rect();
        let viewport_lo = Vector2::new(x, y).component_mul(&self.scale);
        let viewport_hi = Vector2::new(x + width, y + height).component_mul(&self.scale);
        let a = self.pixels(from);
        let b = self.pixels(to);
        if !a.iter().chain(b.iter()).all(|v| v.is_finite()) {
            return None;
        }
        let lo = a.inf(&b).sup(&viewport_lo);
        let hi = a.sup(&b).inf(&viewport_hi);
        (lo.x < hi.x && lo.y < hi.y).then_some(ScreenRect { lo, hi })
    }

    fn radius(self, size: f32) -> Vector2<f64> {
        (self.extent() * 2.0).inf(&Vector2::repeat(f64::from(size) * 0.5))
    }

    fn clip(self, world: [f32; 3]) -> [f64; 4] {
        self.camera.clip_position(world).map(f64::from)
    }

    fn project(self, [x, y, _, w]: [f64; 4]) -> Option<Vector2<f64>> {
        if w <= 0.0 {
            return None;
        }
        let [left, top, width, height] = self.camera.content_rect();
        let p = Vector2::new(
            left + (0.5 + 0.5 * x / w) * width,
            top + (0.5 - 0.5 * y / w) * height,
        )
        .component_mul(&self.scale);
        p.iter().all(|v| v.is_finite()).then_some(p)
    }

    pub fn point(self, world: [f32; 3], size: f32) -> Option<RasterShape> {
        let clip = self.clip(world);
        let [x, y, z, w] = clip;
        if x.abs() > w || y.abs() > w || !(0.0..=w).contains(&z) {
            return None;
        }
        let center = self.project(clip)?;
        let radius = self.radius(size);
        Some(RasterShape::Point(ScreenRect {
            lo: center - radius,
            hi: center + radius,
        }))
    }

    pub fn line(self, [a, b]: [[f32; 3]; 2], width: f32) -> Option<RasterShape> {
        let a = self.clip(a);
        let b = self.clip(b);
        if !a.into_iter().chain(b).all(f64::is_finite) {
            return None;
        }
        let plane = |[x, y, z, w]: [f64; 4]| [w + x, w - x, w + y, w - y, z, w - z];
        let [fa, fb] = [a, b].map(plane);
        // The shader rejects a shared outside clip plane before widening.
        if fa.into_iter().zip(fb).any(|(a, b)| a < 0.0 && b < 0.0) {
            return None;
        }
        // Lateral clipping clips the expanded quad, so retain the projected
        // centerline there. Trim only the near/far planes before dividing by W.
        let [mut start, mut end] = [0.0_f64, 1.0];
        for i in 4..6 {
            if fa[i] < 0.0 {
                start = start.max(fa[i] / (fa[i] - fb[i]));
            } else if fb[i] < 0.0 {
                end = end.min(fa[i] / (fa[i] - fb[i]));
            }
        }
        if start >= end {
            return None;
        }
        let clip = |t| std::array::from_fn(|i| a[i] + (b[i] - a[i]) * t);
        let [a, b] = [start, end].map(clip);
        let left = self.project(a)?;
        let right = self.project(b)?;
        let along = right - left;
        let perpendicular = Vector2::new(-along.y, along.x).try_normalize(1e-10)?;
        let across = perpendicular.component_mul(&self.radius(width));
        Some(RasterShape::Line {
            from: left,
            along,
            across,
            interval: [start, end],
            inverse_w: [a[3].recip(), b[3].recip()],
        })
    }

    /// Convert device depth to distance along the pick ray. This compares
    /// the sampled primitive and triangles at the same screen pixel even
    /// with custom or oblique projections.
    pub fn distance(self, ray: Ray3D, world: [f32; 3]) -> Option<f32> {
        let sample = self.clip(world);
        let [depth, w] = [sample[2], sample[3]];
        if w <= 0.0 {
            return None;
        }
        let [_, _, z, h] = self.camera.clip_rows();
        let coefficient = |row: [f32; 4]| Vector3::new(row[0], row[1], row[2]).cast::<f64>();
        let origin = ray.origin.cast::<f64>();
        let direction = ray.direction.cast::<f64>();
        let z0 = coefficient(z).dot(&origin) + f64::from(z[3]);
        let w0 = coefficient(h).dot(&origin) + f64::from(h[3]);
        let dz = coefficient(z).dot(&direction);
        let dw = coefficient(h).dot(&direction);
        let ndc = depth / w;
        let distance = ((ndc * w0 - z0) / (dz - ndc * dw)) as f32;
        distance.is_finite().then_some(distance)
    }

    pub fn parameter(self, shape: RasterShape, point: Point2D) -> Option<f64> {
        shape.parameter(self.pixels(point))
    }
}

#[derive(Clone, Copy)]
pub(super) struct ScreenRect {
    lo: Vector2<f64>,
    hi: Vector2<f64>,
}

#[derive(Clone, Copy)]
pub(super) enum RasterShape {
    Point(ScreenRect),
    Line {
        from: Vector2<f64>,
        along: Vector2<f64>,
        across: Vector2<f64>,
        interval: [f64; 2],
        inverse_w: [f64; 2],
    },
}

impl RasterShape {
    fn parameter(self, cursor: Vector2<f64>) -> Option<f64> {
        match self {
            Self::Point(rect) => ((rect.lo.x..=rect.hi.x).contains(&cursor.x)
                && (rect.lo.y..=rect.hi.y).contains(&cursor.y))
            .then_some(0.0),
            Self::Line {
                from,
                along,
                across,
                interval: [a, b],
                inverse_w: [wa, wb],
            } => {
                let offset = cursor - from;
                let determinant = along.perp(&across);
                let t = offset.perp(&across) / determinant;
                let side = along.perp(&offset) / determinant;
                if !(0.0..=1.0).contains(&t) || side.abs() > 1.0 {
                    return None;
                }
                let local = (t * wb) / ((1.0 - t) * wa + t * wb);
                Some(a + (b - a) * local)
            }
        }
    }

    pub fn intersects(self, rect: ScreenRect) -> bool {
        match self {
            Self::Point(point) => {
                point.hi.x >= rect.lo.x
                    && point.lo.x <= rect.hi.x
                    && point.hi.y >= rect.lo.y
                    && point.lo.y <= rect.hi.y
            }
            Self::Line {
                from,
                along,
                across,
                ..
            } => {
                // Separating axes of a rectangle and the projected line quad.
                let line_half = along * 0.5;
                let box_half = (rect.hi - rect.lo) * 0.5;
                let delta = rect.lo + box_half - from - line_half;
                [
                    Vector2::x(),
                    Vector2::y(),
                    Vector2::new(-along.y, along.x),
                    Vector2::new(-across.y, across.x),
                ]
                .into_iter()
                .all(|axis| {
                    let extent = axis.dot(&line_half).abs()
                        + axis.dot(&across).abs()
                        + axis.abs().dot(&box_half);
                    axis.dot(&delta).abs() <= extent
                })
            }
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub(super) struct RasterPrimitive {
    pub vertex: u32,
    pub part: u32,
}

#[derive(Debug)]
pub(super) struct RasterTree(BoundsTree<RasterPrimitive>);

impl RasterTree {
    pub fn new(vertices: &[MeshVertex], parts: &[MeshPart]) -> Self {
        let candidates = parts
            .iter()
            .enumerate()
            .filter(|(_, part)| part.topology != MeshTopology::Triangles)
            .flat_map(|(part_index, part)| {
                let arity = part.topology.vertices_per_primitive();
                part.vertices.clone().step_by(arity).map(move |vertex| {
                    let first = Bounds3D::point(vertices[vertex].position);
                    let last = Bounds3D::point(vertices[vertex + arity - 1].position);
                    (
                        first.union(last),
                        RasterPrimitive {
                            vertex: vertex as u32,
                            part: part_index as u32,
                        },
                    )
                })
            })
            .collect();
        Self(BoundsTree::new(candidates, 8))
    }

    pub fn visit(
        &self,
        region: &Frustum,
        rows: [[f32; 4]; 3],
        mut visit: impl FnMut(RasterPrimitive) -> bool,
    ) -> bool {
        let Some(root) = self.0.nodes.first() else {
            return false;
        };
        region
            .bounds_planes(root.bounds.transform(rows))
            .is_some_and(|planes| {
                pick::visit_selection(
                    &self.0,
                    0,
                    region,
                    rows,
                    planes,
                    true,
                    &mut |primitive, _| visit(primitive),
                )
            })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn perspective_line_interpolates_the_original_segment_parameter() {
        let camera = SceneCamera::new(0.0, 0.0, 5.0, [160.0, 120.0]).unwrap();
        let view = RasterView::new(&camera, [1.0; 2]);
        let endpoints = [[-0.8, -0.2, 0.0], [0.9, 0.2, 2.0]];
        let clip = endpoints.map(|position| view.clip(position));
        let [a, b] = clip.map(|point| view.project(point).unwrap());
        let shape = view.line(endpoints, 8.0).unwrap();

        let center = (a + b) * 0.5;
        let expected = clip[0][3] / (clip[0][3] + clip[1][3]);
        assert!((shape.parameter(center).unwrap() - expected).abs() < 1e-8);

        let beyond = b + (b - a).normalize() * 2.0;
        assert!(shape.parameter(beyond).is_none(), "line uses a flat cap");
        let outside = Vector2::new(a.x.min(b.x), a.y.min(b.y));
        let outside = ScreenRect {
            lo: outside,
            hi: outside + Vector2::repeat(1.0),
        };
        assert!(!shape.intersects(outside));
    }

    #[test]
    fn dense_point_cloud_uses_the_bounds_tree_for_a_pointer_region() {
        let vertices: Vec<_> = (0..64)
            .flat_map(|y| {
                (0..64).map(move |x| MeshVertex {
                    position: [(x as f32 - 32.0) * 0.05, (y as f32 - 32.0) * 0.05, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0; 2],
                    uv1: [0.0; 2],
                    color: [1.0; 4],
                })
            })
            .collect();
        let tree = RasterTree::new(
            &vertices,
            &[MeshPart {
                topology: MeshTopology::Points,
                lighting: Default::default(),
                vertices: 0..vertices.len(),
                material: 0,
                volume_inverse: None,
            }],
        );
        let camera = SceneCamera::new(0.0, 0.0, 4.0, [160.0, 120.0]).unwrap();
        let view = RasterView::new(&camera, [1.0; 2]);
        let region = view
            .point_region(Point2D::new(Length(80.0), Length(60.0)), 1.0)
            .unwrap();
        let mut visits = 0;
        tree.visit(
            &region,
            [
                [1.0, 0.0, 0.0, 0.0],
                [0.0, 1.0, 0.0, 0.0],
                [0.0, 0.0, 1.0, 0.0],
            ],
            |_| {
                visits += 1;
                false
            },
        );
        assert!((1..=32).contains(&visits), "{visits} candidate points");
    }
}
