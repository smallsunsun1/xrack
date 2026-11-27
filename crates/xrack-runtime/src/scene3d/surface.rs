//! Shared nearest-point geometry for scene queries and mesh simplification.
use crate::scene3d::bounds::{Bounds3D, BoundsTree};
use nalgebra::Vector3;

#[doc(hidden)]
#[derive(Clone, Copy)]
pub struct SurfaceSample {
    pub vertices: [usize; 3],
    pub weights: [f64; 3],
}

/// Match each point to the closest location on a retained triangle.
/// Points are in a compact, normalized mesh surface.
#[doc(hidden)]
pub fn corresponding_vertices(
    positions: &[[f32; 3]],
    indices: &[u32],
) -> Option<Vec<SurfaceSample>> {
    let (triangles, trailing) = indices.as_chunks::<3>();
    if triangles.is_empty()
        || !trailing.is_empty()
        || !positions.iter().flatten().all(|v| v.is_finite())
    {
        return None;
    }
    let candidates = triangles
        .iter()
        .map(|&triangle| {
            let vertices = triangle.map(|index| index as usize);
            let [a, b, c] = vertices;
            let bounds = Bounds3D::point(*positions.get(a)?)
                .union(Bounds3D::point(*positions.get(b)?))
                .union(Bounds3D::point(*positions.get(c)?));
            Some((bounds, vertices))
        })
        .collect::<Option<Vec<_>>>()?;
    let mut retained = vec![false; positions.len()];
    for &index in indices {
        retained[index as usize] = true;
    }
    let tree = BoundsTree::new(candidates, 8);
    positions
        .iter()
        .enumerate()
        .map(|(index, &point)| {
            if retained[index] {
                return Some(SurfaceSample {
                    vertices: [index; 3],
                    weights: [1.0, 0.0, 0.0],
                });
            }
            let point = Vector3::from(point.map(f64::from));
            let mut best: Option<SurfaceSample> = None;
            tree.visit_nearest(
                f64::INFINITY,
                |bounds| bounds.distance_squared(point),
                |&vertices, limit| {
                    let triangle =
                        vertices.map(|index| Vector3::from(positions[index].map(f64::from)));
                    let (weights, distance) = closest_triangle_point(point, triangle);
                    if distance < limit
                        || distance == limit
                            && best.is_none_or(|previous| vertices < previous.vertices)
                    {
                        best = Some(SurfaceSample { vertices, weights });
                        distance
                    } else {
                        limit
                    }
                },
            );
            best
        })
        .collect()
}

pub(crate) fn closest_triangle_point(
    point: Vector3<f64>,
    [a, b, c]: [Vector3<f64>; 3],
) -> ([f64; 3], f64) {
    let ab = b - a;
    let ac = c - a;
    let ap = point - a;
    let normal = ab.cross(&ac);
    let denominator = normal.norm_squared();
    if denominator > 0.0 {
        let u = ap.cross(&ac).dot(&normal) / denominator;
        let v = ab.cross(&ap).dot(&normal) / denominator;
        if u >= 0.0 && v >= 0.0 && u + v <= 1.0 {
            return ([1.0 - u - v, u, v], (ap - u * ab - v * ac).norm_squared());
        }
    }
    let edge = |from: Vector3<f64>, to: Vector3<f64>| {
        let direction = to - from;
        let length = direction.norm_squared();
        let t = if length == 0.0 {
            0.0
        } else {
            ((point - from).dot(&direction) / length).clamp(0.0, 1.0)
        };
        (t, (point - from - direction * t).norm_squared())
    };
    let (ab, ab_distance) = edge(a, b);
    let (bc, bc_distance) = edge(b, c);
    let (ca, ca_distance) = edge(c, a);
    if ab_distance <= bc_distance && ab_distance <= ca_distance {
        ([1.0 - ab, ab, 0.0], ab_distance)
    } else if bc_distance <= ca_distance {
        ([0.0, 1.0 - bc, bc], bc_distance)
    } else {
        ([ca, 0.0, 1.0 - ca], ca_distance)
    }
}
