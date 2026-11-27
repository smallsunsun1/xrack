//! A lazy mesh-space BVH used by ray queries on immutable geometry.
use crate::{
    scene3d::bounds::{Bounds3D, BoundsTree},
    scene3d::visibility::Frustum,
    scene3d::{MeshPart, MeshVertex},
};
use nalgebra::Vector3;

pub(crate) const MIN_BVH_TRIANGLES: usize = 32;
const LEAF_TRIANGLES: usize = 8;

#[derive(Clone, Copy, Debug)]
pub(crate) struct PickTriangle {
    pub vertex: u32,
    pub part: u32,
}

#[derive(Debug)]
pub(crate) struct PickTree(BoundsTree<PickTriangle>);

impl PickTree {
    pub fn new(vertices: &[MeshVertex], parts: &[MeshPart]) -> Self {
        let candidates = parts
            .iter()
            .enumerate()
            .flat_map(|(part, section)| {
                section.vertices.clone().step_by(3).map(move |vertex| {
                    let [a, b, c] = std::array::from_fn(|axis| {
                        Bounds3D::point(vertices[vertex + axis].position)
                    });
                    (
                        a.union(b).union(c),
                        PickTriangle {
                            vertex: vertex as u32,
                            part: part as u32,
                        },
                    )
                })
            })
            .collect();
        Self(BoundsTree::new(candidates, LEAF_TRIANGLES))
    }

    pub(crate) fn visit_expanded(
        &self,
        span: BoundsSegment,
        mut visit: impl FnMut(PickTriangle, f64) -> f64,
    ) {
        span.visit(&self.0, |triangle, closest| visit(*triangle, closest));
    }

    pub(crate) fn visit_nearest(
        &self,
        point: Vector3<f64>,
        rows: [[f32; 4]; 3],
        limit: f64,
        mut visit: impl FnMut(usize, f64) -> f64,
    ) {
        self.0.visit_nearest(
            limit,
            |bounds| bounds.transform(rows).distance_squared(point),
            |triangle, limit| visit(triangle.vertex as usize, limit),
        );
    }

    pub(crate) fn intersects_selection(
        &self,
        region: &Frustum,
        rows: [[f32; 4]; 3],
        planes: u8,
        test_faces: bool,
        mut visit: impl FnMut(PickTriangle, u8) -> bool,
    ) -> bool {
        !self.0.nodes.is_empty()
            && visit_selection(&self.0, 0, region, rows, planes, test_faces, &mut visit)
    }

    /// Supply the closest accepted distance after visiting each triangle.
    pub fn visit(
        &self,
        origin: Vector3<f32>,
        direction: Vector3<f32>,
        min: f32,
        max: f32,
        mut visit: impl FnMut(PickTriangle, f32) -> f32,
    ) {
        visit_bounds(&self.0, origin, direction, min, max, |triangle, closest| {
            visit(*triangle, closest)
        });
    }
}

fn visit_selection(
    tree: &BoundsTree<PickTriangle>,
    index: usize,
    region: &Frustum,
    rows: [[f32; 4]; 3],
    planes: u8,
    test_faces: bool,
    visit: &mut impl FnMut(PickTriangle, u8) -> bool,
) -> bool {
    let node = &tree.nodes[index];
    let Some(planes) = region.intersecting_planes(node.bounds.transform(rows), planes) else {
        return false;
    };
    if planes == 0 && !test_faces {
        return true;
    }
    if let Some(children) = node.children {
        children
            .into_iter()
            .any(|index| visit_selection(tree, index, region, rows, planes, test_faces, visit))
    } else {
        tree.items[node.range.clone()]
            .iter()
            .any(|triangle| visit(*triangle, planes))
    }
}

pub(crate) fn visit_bounds<T>(
    tree: &BoundsTree<T>,
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    min: f32,
    max: f32,
    mut visit: impl FnMut(&T, f32) -> f32,
) {
    BoundsSegment {
        origin: origin.map(f64::from),
        direction: direction.map(f64::from),
        min: f64::from(min),
        max: f64::from(max),
        expansion: Vector3::zeros(),
    }
    .visit(tree, |item, closest| f64::from(visit(item, closest as f32)));
}

fn visit_node<T>(
    tree: &BoundsTree<T>,
    index: usize,
    ray: &mut BoundsSegment,
    visit: &mut impl FnMut(&T, f64) -> f64,
) {
    let node = &tree.nodes[index];
    if let Some([a, b]) = node.children {
        let left = ray.entry(tree.nodes[a].bounds);
        let right = ray.entry(tree.nodes[b].bounds);
        match (left, right) {
            (None, None) => {}
            (Some(_), None) => visit_node(tree, a, ray, visit),
            (None, Some(_)) => visit_node(tree, b, ray, visit),
            (Some(left), Some(right)) => {
                let (first, second, entry) = if left <= right {
                    (a, b, right)
                } else {
                    (b, a, left)
                };
                visit_node(tree, first, ray, visit);
                if entry <= ray.padded_max() {
                    visit_node(tree, second, ray, visit);
                }
            }
        }
    } else {
        for item in &tree.items[node.range.clone()] {
            ray.max = visit(item, ray.max);
        }
    }
}

#[derive(Clone, Copy)]
pub(crate) struct BoundsSegment {
    pub origin: Vector3<f64>,
    pub direction: Vector3<f64>,
    pub min: f64,
    pub max: f64,
    pub expansion: Vector3<f64>,
}

impl BoundsSegment {
    pub(crate) fn visit<T>(mut self, tree: &BoundsTree<T>, mut visit: impl FnMut(&T, f64) -> f64) {
        if tree
            .nodes
            .first()
            .is_some_and(|node| self.entry(node.bounds).is_some())
        {
            visit_node(tree, 0, &mut self, &mut visit);
        }
    }

    // Keep coincident surfaces and near-clip hits when f32 triangle arithmetic
    // and the f64 slab test round a boundary by different amounts.
    fn pad(value: f64) -> f64 {
        value.abs().max(1.0) * f64::from(32.0 * f32::EPSILON)
    }

    fn padded_max(&self) -> f64 {
        self.max + Self::pad(self.max)
    }

    pub(crate) fn entry(&self, bounds: Bounds3D) -> Option<f64> {
        let mut near = self.min - Self::pad(self.min);
        let mut far = self.padded_max();
        for axis in 0..3 {
            let low = f64::from(bounds.min[axis]) - self.expansion[axis];
            let high = f64::from(bounds.max[axis]) + self.expansion[axis];
            let origin = self.origin[axis];
            let direction = self.direction[axis];
            let padding = Self::pad(low.abs().max(high.abs()));
            let low = low - padding;
            let high = high + padding;
            if direction == 0.0 {
                if !(low..=high).contains(&origin) {
                    return None;
                }
                continue;
            }
            let a = (low - origin) / direction;
            let b = (high - origin) / direction;
            near = near.max(a.min(b));
            far = far.min(a.max(b));
            if near > far {
                return None;
            }
        }
        Some(near)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Instant;

    const SIDE: usize = 64;

    fn grid_vertices() -> Vec<MeshVertex> {
        (0..SIDE)
            .flat_map(|y| (0..SIDE).map(move |x| (x, y)))
            .flat_map(|(x, y)| {
                [[0.0, 0.0], [0.8, 0.0], [0.0, 0.8]].map(|[dx, dy]| MeshVertex {
                    position: [x as f32 + dx, y as f32 + dy, 0.0],
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0; 2],
                    uv1: [0.0; 2],
                    color: [1.0; 4],
                })
            })
            .collect()
    }

    #[test]
    fn nearest_surface_prunes_dense_geometry_after_a_sheared_world_transform() {
        use crate::scene3d::surface::closest_triangle_point;

        let vertices = grid_vertices();
        let tree = PickTree::new(
            &vertices,
            &[MeshPart {
                vertices: 0..vertices.len(),
                material: 0,
                volume_inverse: None,
            }],
        );
        let rows = [
            [2.0, 0.35, 0.0, 4.0],
            [0.0, 0.65, 0.0, -6.0],
            [0.0, 0.0, 1.0, 3.0],
        ];
        let point = Vector3::new(2.0 * 32.15 + 0.35 * 18.21 + 4.0, 0.65 * 18.21 - 6.0, 3.2);
        let corners = vertices.as_chunks::<3>().0;
        let world_distance = |vertex: usize| {
            let triangle = corners[vertex / 3].map(|vertex| {
                let [x, y, z] = vertex.position.map(f64::from);
                Vector3::from(rows.map(|[a, b, c, d]| {
                    f64::from(a) * x + f64::from(b) * y + f64::from(c) * z + f64::from(d)
                }))
            });
            closest_triangle_point(point, triangle).1
        };
        let mut visited = 0;
        let mut best = None;
        tree.visit_nearest(point, rows, 2.0_f64.powi(2), |vertex, limit| {
            visited += 1;
            let distance = world_distance(vertex);
            if distance < limit {
                best = Some((vertex, distance));
                distance
            } else {
                limit
            }
        });
        let brute = (0..vertices.len())
            .step_by(3)
            .map(|vertex| (vertex, world_distance(vertex)))
            .min_by(|(_, a), (_, b)| a.total_cmp(b));
        assert_eq!(best, brute);
        assert!(
            visited <= 32,
            "searched {visited} of {} triangles",
            corners.len()
        );
        eprintln!(
            "closest-point traversal tested {visited} of {} triangles",
            corners.len()
        );
    }

    #[test]
    fn selection_in_dense_mesh_only_tests_nearby_triangles() {
        use crate::{Length, Point2D, scene3d::SceneCamera};

        let vertices = grid_vertices();
        let tree = PickTree::new(
            &vertices,
            &[MeshPart {
                vertices: 0..vertices.len(),
                material: 0,
                volume_inverse: None,
            }],
        );
        let camera = SceneCamera::new(0.0, 0.0, 8.0, [512.0; 2]).expect("test camera");
        let p = |x, y| Point2D::new(Length(x), Length(y));
        let region = Frustum::selection(&camera, [p(255.5, 255.5), p(256.5, 256.5)])
            .expect("nonempty selection");
        let rows = [
            [1.0, 0.0, 0.0, -32.15],
            [0.0, 1.0, 0.0, -22.15],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let triangles = vertices.as_chunks::<3>().0;
        let mut tested = 0;
        let found =
            tree.intersects_selection(&region, rows, 0b11_1111, false, |triangle, planes| {
                tested += 1;
                let world = triangles[triangle.vertex as usize / 3].map(|vertex| {
                    let [x, y, z] = vertex.position;
                    [x + rows[0][3], y + rows[1][3], z]
                });
                region.triangle_intersects(world, planes)
            });
        assert!(found);
        assert!((1..=16).contains(&tested), "tested {tested} triangles");
        eprintln!("rectangle tested {tested} of {} triangles", triangles.len());
    }

    #[test]
    fn dense_static_grid_only_visits_triangles_near_the_ray() {
        let vertices = grid_vertices();
        let begin = Instant::now();
        let tree = PickTree::new(
            &vertices,
            &[MeshPart {
                vertices: 0..vertices.len(),
                material: 0,
                volume_inverse: None,
            }],
        );
        let build = begin.elapsed();
        let mut visited = Vec::new();
        tree.visit(
            Vector3::new(31.125, 22.125, 2.0),
            -Vector3::z(),
            0.0,
            8.0,
            |triangle, max| {
                visited.push(triangle.vertex as usize);
                max
            },
        );
        assert!(visited.contains(&((22 * SIDE + 31) * 3)));
        assert!(visited.len() <= 2 * LEAF_TRIANGLES, "{visited:?}");
        eprintln!(
            "{} of {} triangles visited; tree build {build:?}",
            visited.len(),
            SIDE * SIDE
        );
    }
}
