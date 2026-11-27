//! Unit geometry for the procedural scene models.

use crate::scene3d::MeshVertex;
use nalgebra::Vector3;
use std::f32::consts::{PI, TAU};

pub(crate) type PrimitiveParts = Vec<(&'static str, Vec<MeshVertex>)>;

const SEGMENTS: usize = 32;
const RADIUS: f32 = 0.5;

pub(crate) fn vertex(position: [f32; 3], normal: [f32; 3], uv: [f32; 2]) -> MeshVertex {
    MeshVertex {
        position,
        normal,
        uv,
        uv1: [0.0; 2],
        color: [1.0; 4],
    }
}

pub(crate) fn rectangle() -> PrimitiveParts {
    let normal = [0.0, 0.0, 1.0];
    let corners = [
        vertex([-0.5, -0.5, 0.0], normal, [0.0, 1.0]),
        vertex([0.5, -0.5, 0.0], normal, [1.0, 1.0]),
        vertex([0.5, 0.5, 0.0], normal, [1.0, 0.0]),
        vertex([-0.5, 0.5, 0.0], normal, [0.0, 0.0]),
    ];
    vec![("surface", [0, 1, 2, 0, 2, 3].map(|i| corners[i]).into())]
}

fn radial(segment: usize) -> [f32; 2] {
    let (sin, cos) = (TAU * (segment % SEGMENTS) as f32 / SEGMENTS as f32).sin_cos();
    [cos, sin]
}

fn cap(y: f32, top: bool) -> Vec<MeshVertex> {
    let normal = [0.0, if top { 1.0 } else { -1.0 }, 0.0];
    let center = vertex([0.0, y, 0.0], normal, [0.5, 0.5]);
    let edge = |segment| {
        let [x, z] = radial(segment).map(|value| value * RADIUS);
        vertex([x, y, z], normal, [0.5 + x, 0.5 + if top { z } else { -z }])
    };
    (0..SEGMENTS)
        .flat_map(|i| {
            let (a, b) = (edge(i), edge(i + 1));
            if top { [center, b, a] } else { [center, a, b] }
        })
        .collect()
}

pub(crate) fn cylinder() -> PrimitiveParts {
    let side = |segment: usize, y, v| {
        let [x, z] = radial(segment);
        vertex(
            [RADIUS * x, y, RADIUS * z],
            [x, 0.0, z],
            [segment as f32 / SEGMENTS as f32, v],
        )
    };
    let sides = (0..SEGMENTS)
        .flat_map(|i| {
            let a = side(i, -0.5, 1.0);
            let b = side(i + 1, -0.5, 1.0);
            let c = side(i, 0.5, 0.0);
            let d = side(i + 1, 0.5, 0.0);
            [a, c, d, a, d, b]
        })
        .collect();
    vec![
        ("side", sides),
        ("top", cap(0.5, true)),
        ("bottom", cap(-0.5, false)),
    ]
}

pub(crate) fn cone() -> PrimitiveParts {
    let normal = |[x, z]: [f32; 2]| {
        let inv_len = (1.0 + RADIUS * RADIUS).sqrt().recip();
        [x * inv_len, RADIUS * inv_len, z * inv_len]
    };
    let edge = |segment: usize| {
        let direction = radial(segment);
        vertex(
            [RADIUS * direction[0], 0.0, RADIUS * direction[1]],
            normal(direction),
            [segment as f32 / SEGMENTS as f32, 1.0],
        )
    };
    let sides = (0..SEGMENTS)
        .flat_map(|i| {
            let u = (i as f32 + 0.5) / SEGMENTS as f32;
            let (sin, cos) = (TAU * u).sin_cos();
            [
                edge(i),
                vertex([0.0, 1.0, 0.0], normal([cos, sin]), [u, 0.0]),
                edge(i + 1),
            ]
        })
        .collect();
    vec![("side", sides), ("base", cap(0.0, false))]
}

pub(super) fn box_vertices() -> Vec<MeshVertex> {
    let corners = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];
    let faces = [
        ([3, 2, 1, 0], [0.0, 0.0, -1.0]),
        ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
        ([4, 7, 3, 0], [-1.0, 0.0, 0.0]),
        ([1, 2, 6, 5], [1.0, 0.0, 0.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([3, 7, 6, 2], [0.0, 1.0, 0.0]),
    ];
    faces
        .into_iter()
        .flat_map(|(indices, normal)| {
            let up = match normal[1] {
                1.0 => -Vector3::z(),
                -1.0 => Vector3::z(),
                _ => Vector3::y(),
            };
            let right = up.cross(&Vector3::from(normal));
            [0, 1, 2, 0, 2, 3].map(|index| {
                let position = corners[indices[index]];
                let p = Vector3::from(position);
                vertex(position, normal, [0.5 + p.dot(&right), 0.5 - p.dot(&up)])
            })
        })
        .collect()
}

pub(super) fn sphere_vertices() -> Vec<MeshVertex> {
    const SLICES: usize = 24;
    const STACKS: usize = 12;
    let point = |row: usize, column: usize| {
        let polar = PI * row as f32 / STACKS as f32;
        let azimuth = TAU * (column % SLICES) as f32 / SLICES as f32;
        let (radius, y) = polar.sin_cos();
        let position = match row {
            0 => [0.0, 1.0, 0.0],
            STACKS => [0.0, -1.0, 0.0],
            _ => [radius * azimuth.cos(), y, radius * azimuth.sin()],
        };
        vertex(
            position,
            position,
            [column as f32 / SLICES as f32, row as f32 / STACKS as f32],
        )
    };
    let mut vertices = Vec::with_capacity(SLICES * (STACKS - 1) * 6);
    for row in 0..STACKS {
        for column in 0..SLICES {
            let a = point(row, column);
            let b = point(row, column + 1);
            let c = point(row + 1, column);
            let d = point(row + 1, column + 1);
            if row > 0 {
                vertices.extend([a, b, c]);
            }
            if row + 1 < STACKS {
                vertices.extend([b, d, c]);
            }
        }
    }
    vertices
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Vector3;

    #[test]
    fn procedural_parts_have_outward_winding_and_bounded_uvs() {
        for (parts, triangles_per_part) in [
            (rectangle(), &[2][..]),
            (cylinder(), &[64, 32, 32][..]),
            (cone(), &[32, 32][..]),
        ] {
            assert_eq!(parts.len(), triangles_per_part.len());
            for ((name, vertices), count) in parts.into_iter().zip(triangles_per_part) {
                let (triangles, remainder) = vertices.as_chunks::<3>();
                assert!(remainder.is_empty());
                assert_eq!(triangles.len(), *count, "{name}");
                for face in triangles {
                    let [a, b, c] = face.map(|vertex| Vector3::from(vertex.position));
                    let normal = (b - a).cross(&(c - a));
                    for point in face {
                        assert!(normal.dot(&Vector3::from(point.normal)) > 0.0, "{name}");
                        assert!(point.uv.iter().all(|value| (0.0..=1.0).contains(value)));
                    }
                }
            }
        }
    }
}
