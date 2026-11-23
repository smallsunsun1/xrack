//! A lazy mesh-space BVH used by ray queries on immutable geometry.
use crate::{
    scene_bounds::{Bounds3D, BoundsTree},
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

    #[test]
    fn dense_static_grid_only_visits_triangles_near_the_ray() {
        const SIDE: usize = 64;
        let vertices: Vec<_> = (0..SIDE)
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
            .collect();
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
