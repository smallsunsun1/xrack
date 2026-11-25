//! Parametric meshes used by scene rows, picking and the renderer.
use std::{
    collections::VecDeque,
    f32::consts::{PI, TAU},
    sync::{Mutex, OnceLock},
};

use crate::{
    RecordValue, Value,
    scene_primitives::{PrimitiveParts, vertex},
    scene3d::{Mesh3D, MeshVertex},
};

const CACHE_ENTRIES: usize = 16;
const CACHE_VERTICES: usize = 250_000;

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SceneParametricGeometry<'a> {
    pub shape: &'a str,
    pub width: Option<f64>,
    pub height: Option<f64>,
    pub depth: Option<f64>,
    pub radius: Option<f64>,
    pub top_radius: Option<f64>,
    pub tube_radius: Option<f64>,
    pub segments: Option<i64>,
    pub rings: Option<i64>,
    pub capped: Option<bool>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Plane {
        width: f32,
        height: f32,
    },
    Box {
        extent: [f32; 3],
    },
    Sphere {
        radius: f32,
    },
    Revolved {
        bottom: f32,
        top: f32,
        height: f32,
        capped: bool,
        cone: bool,
    },
    Capsule {
        radius: f32,
        height: f32,
    },
    Torus {
        radius: f32,
        tube: f32,
    },
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Recipe {
    shape: Shape,
    segments: usize,
    rings: usize,
}

impl<'a> SceneParametricGeometry<'a> {
    pub(crate) fn from_record(record: &'a RecordValue) -> Option<Self> {
        let Value::String(shape) = record.field("shape")? else {
            return None;
        };
        let number = |name| match record.field(name) {
            Some(Value::Float(value)) => Some(*value),
            _ => None,
        };
        let integer = |name| match record.field(name) {
            Some(Value::Int(value)) => Some(*value),
            _ => None,
        };
        Some(Self {
            shape: shape.as_str(),
            width: number("width"),
            height: number("height"),
            depth: number("depth"),
            radius: number("radius"),
            top_radius: number("top-radius"),
            tube_radius: number("tube-radius"),
            segments: integer("segments"),
            rings: integer("rings"),
            capped: match record.field("capped") {
                Some(Value::Bool(value)) => Some(*value),
                _ => None,
            },
        })
    }

    pub(crate) fn is_valid(self) -> bool {
        self.recipe().is_some()
    }

    pub(crate) fn mesh(self) -> Option<Mesh3D> {
        let recipe = self.recipe()?;
        static CACHE: OnceLock<Mutex<GeometryCache>> = OnceLock::new();
        let cache = CACHE.get_or_init(Mutex::default);
        if let Some(mesh) = cache.lock().ok().and_then(|mut cache| cache.get(recipe)) {
            return Some(mesh);
        }
        let parts = recipe.vertices();
        let mesh = if matches!(recipe.shape, Shape::Capsule { .. }) {
            Mesh3D::prepare_capsule(parts)
        } else {
            Mesh3D::prepare_procedural(parts)
        }?;
        if let Ok(mut cache) = cache.lock() {
            // Two windows may prepare the same geometry at the same time.
            if let Some(existing) = cache.get(recipe) {
                return Some(existing);
            }
            cache.insert(recipe, mesh.clone());
        }
        Some(mesh)
    }

    fn recipe(self) -> Option<Recipe> {
        let size = |value: Option<f64>, default: f64| {
            let value = value.unwrap_or(default);
            (0.001..=200.0).contains(&value).then_some(value as f32)
        };
        let count = |value: Option<i64>, default: i64, min: i64| {
            value.unwrap_or(default).clamp(min, 64) as usize
        };
        let (shape, min_segments, min_rings, segments, rings) = match self.shape {
            "plane" => (
                Shape::Plane {
                    width: size(self.width, 1.0)?,
                    height: size(self.height, 1.0)?,
                },
                1,
                1,
                1,
                1,
            ),
            "box" => (
                Shape::Box {
                    extent: [
                        size(self.width, 1.0)?,
                        size(self.height, 1.0)?,
                        size(self.depth, 1.0)?,
                    ],
                },
                1,
                1,
                1,
                1,
            ),
            "sphere" => (
                Shape::Sphere {
                    radius: size(self.radius, 0.5)?,
                },
                3,
                2,
                32,
                16,
            ),
            "cylinder" | "cone" => {
                let cone = self.shape == "cone";
                let bottom = size(self.radius, 0.5)?;
                let top = self
                    .top_radius
                    .unwrap_or(if cone { 0.0 } else { f64::from(bottom) });
                if top != 0.0 && !(0.001..=200.0).contains(&top) {
                    return None;
                }
                (
                    Shape::Revolved {
                        bottom,
                        top: top as f32,
                        height: size(self.height, 1.0)?,
                        capped: self.capped.unwrap_or(true),
                        cone,
                    },
                    3,
                    1,
                    32,
                    1,
                )
            }
            "capsule" => {
                let radius = size(self.radius, 0.5)?;
                let height = size(self.height, 2.0)?;
                if height < 2.0 * radius {
                    return None;
                }
                (Shape::Capsule { radius, height }, 3, 2, 32, 8)
            }
            "torus" => {
                let radius = size(self.radius, 0.75)?;
                let tube = size(self.tube_radius, 0.25)?;
                if tube >= radius {
                    return None;
                }
                (Shape::Torus { radius, tube }, 3, 3, 32, 16)
            }
            _ => return None,
        };
        Some(Recipe {
            shape,
            segments: count(self.segments, segments, min_segments),
            rings: count(self.rings, rings, min_rings),
        })
    }
}

#[derive(Default)]
struct GeometryCache {
    entries: VecDeque<(Recipe, Mesh3D)>,
    vertex_count: usize,
}

impl GeometryCache {
    fn get(&mut self, recipe: Recipe) -> Option<Mesh3D> {
        let index = self.entries.iter().position(|(key, _)| *key == recipe)?;
        let entry = self.entries.remove(index)?;
        let mesh = entry.1.clone();
        self.entries.push_front(entry);
        Some(mesh)
    }

    fn insert(&mut self, recipe: Recipe, mesh: Mesh3D) {
        let vertices = mesh.vertices().len();
        while self.entries.len() >= CACHE_ENTRIES || self.vertex_count + vertices > CACHE_VERTICES {
            let Some((_, removed)) = self.entries.pop_back() else {
                return;
            };
            self.vertex_count -= removed.vertices().len();
        }
        self.vertex_count += vertices;
        self.entries.push_front((recipe, mesh));
    }
}

impl Recipe {
    fn vertices(self) -> PrimitiveParts {
        let (segments, rings) = (self.segments, self.rings);
        match self.shape {
            Shape::Plane { width, height } => vec![(
                "surface",
                panel(
                    [-width * 0.5, -height * 0.5, 0.0],
                    [width, 0.0, 0.0],
                    [0.0, height, 0.0],
                    segments,
                    rings,
                ),
            )],
            Shape::Box { extent: [w, h, d] } => {
                let [x, y, z] = [0.5 * w, 0.5 * h, 0.5 * d];
                [
                    ("front", [-x, -y, z], [w, 0.0, 0.0], [0.0, h, 0.0]),
                    ("back", [x, -y, -z], [-w, 0.0, 0.0], [0.0, h, 0.0]),
                    ("right", [x, -y, z], [0.0, 0.0, -d], [0.0, h, 0.0]),
                    ("left", [-x, -y, -z], [0.0, 0.0, d], [0.0, h, 0.0]),
                    ("top", [-x, y, z], [w, 0.0, 0.0], [0.0, 0.0, -d]),
                    ("bottom", [-x, -y, -z], [w, 0.0, 0.0], [0.0, 0.0, d]),
                ]
                .into_iter()
                .map(|(name, o, u, v)| (name, panel(o, u, v, segments, rings)))
                .collect()
            }
            Shape::Sphere { radius } => {
                let profile = (0..=rings)
                    .map(|i| rounded(i, rings, radius, 0.0))
                    .collect::<Vec<_>>();
                vec![("surface", lathe(&profile, segments))]
            }
            Shape::Capsule { radius, height } => {
                let offset = height * 0.5 - radius;
                let mut profile = Vec::with_capacity(2 * rings + 2);
                for i in 0..=rings {
                    profile.push(rounded(i, rings * 2, radius, offset));
                }
                for i in (rings + usize::from(offset == 0.0))..=rings * 2 {
                    profile.push(rounded(i, rings * 2, radius, -offset));
                }
                vec![("surface", lathe(&profile, segments))]
            }
            Shape::Revolved {
                bottom,
                top,
                height,
                capped,
                cone,
            } => {
                let origin = if cone { 0.0 } else { -height * 0.5 };
                let slope = [height, bottom - top];
                let norm = f32::hypot(slope[0], slope[1]);
                let profile = (0..=rings)
                    .map(|i| {
                        let t = fraction(i, rings);
                        Profile {
                            height: origin + height * (1.0 - t),
                            radius: (top * (1.0 - t)) + bottom * t,
                            normal: slope.map(|component| component / norm),
                        }
                    })
                    .collect::<Vec<_>>();
                let mut parts = vec![("side", lathe(&profile, segments))];
                if capped && top > 0.0 {
                    parts.push(("top", cap(origin + height, top, true, segments)));
                }
                if capped {
                    parts.push((
                        if cone { "base" } else { "bottom" },
                        cap(origin, bottom, false, segments),
                    ));
                }
                parts
            }
            Shape::Torus { radius, tube } => vec![(
                "surface",
                tessellate(segments, rings, |x, y| {
                    let (s, c) = turn(x, segments);
                    let (sz, cr) = turn(y, rings);
                    let radial = radius + cr * tube;
                    vertex(
                        [c * radial, s * radial, sz * tube],
                        [c * cr, s * cr, sz],
                        [fraction(x, segments), fraction(y, rings)],
                    )
                }),
            )],
        }
    }
}

fn fraction(value: usize, total: usize) -> f32 {
    value as f32 / total as f32
}

fn turn(value: usize, total: usize) -> (f32, f32) {
    (fraction(value % total, total) * TAU).sin_cos()
}

fn tessellate(
    width: usize,
    height: usize,
    point: impl Fn(usize, usize) -> MeshVertex,
) -> Vec<MeshVertex> {
    let stride = width + 1;
    let mut grid = Vec::with_capacity(stride * (height + 1));
    for y in 0..=height {
        for x in 0..=width {
            grid.push(point(x, y));
        }
    }
    let mut vertices = Vec::with_capacity(width * height * 6);
    for y in 0..height {
        for x in 0..width {
            let top = stride * y + x;
            let bottom = top + stride;
            let [a, b, c, d] = [grid[top], grid[top + 1], grid[bottom], grid[bottom + 1]];
            vertices.extend([a, b, d, a, d, c]);
        }
    }
    vertices
}

fn panel(
    origin: [f32; 3],
    across: [f32; 3],
    up: [f32; 3],
    segments: usize,
    rings: usize,
) -> Vec<MeshVertex> {
    use nalgebra::Vector3;

    let normal = Vector3::from(across)
        .cross(&Vector3::from(up))
        .normalize()
        .into();
    tessellate(segments, rings, |x, y| {
        let [u, v] = [fraction(x, segments), fraction(y, rings)];
        vertex(
            std::array::from_fn(|axis| origin[axis] + across[axis] * u + up[axis] * v),
            normal,
            [u, 1.0 - v],
        )
    })
}

struct Profile {
    height: f32,
    radius: f32,
    normal: [f32; 2],
}

fn rounded(row: usize, rows: usize, radius: f32, offset: f32) -> Profile {
    let (r, y) = match row {
        0 => (0.0, 1.0),
        row if row == rows => (0.0, -1.0),
        row if row * 2 == rows => (1.0, 0.0),
        _ => (fraction(row, rows) * PI).sin_cos(),
    };
    Profile {
        height: offset + y * radius,
        radius: r * radius,
        normal: [r, y],
    }
}

fn lathe(profile: &[Profile], segments: usize) -> Vec<MeshVertex> {
    let rings = profile.len() - 1;
    let directions = (0..=segments)
        .map(|segment| turn(segment, segments))
        .collect::<Vec<_>>();
    let point = |segment: usize, row: usize| {
        let (s, c) = directions[segment];
        let Profile {
            height,
            radius,
            normal: [r, y],
        } = profile[row];
        vertex(
            [c * radius, height, s * radius],
            [c * r, y, s * r],
            [fraction(segment, segments), fraction(row, rings)],
        )
    };
    let mut vertices = Vec::with_capacity(segments * rings * 6);
    for row in 0..rings {
        for segment in 0..segments {
            let [a, b, c, d] = [
                point(segment, row),
                point(segment + 1, row),
                point(segment, row + 1),
                point(segment + 1, row + 1),
            ];
            if profile[row].radius > 0.0 {
                vertices.extend([a, b, d]);
            }
            if profile[row + 1].radius > 0.0 {
                vertices.extend([a, d, c]);
            }
        }
    }
    vertices
}

fn cap(height: f32, radius: f32, top: bool, segments: usize) -> Vec<MeshVertex> {
    let normal = [0.0, if top { 1.0 } else { -1.0 }, 0.0];
    let center = vertex([0.0, height, 0.0], normal, [0.5, 0.5]);
    let point = |i| {
        let (s, c) = turn(i, segments);
        vertex(
            [c * radius, height, s * radius],
            normal,
            [0.5 + c * 0.5, 0.5 + s * if top { 0.5 } else { -0.5 }],
        )
    };
    (0..segments)
        .flat_map(|i| {
            let [a, b] = [point(i), point(i + 1)];
            if top { [center, b, a] } else { [center, a, b] }
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use nalgebra::Vector3;

    #[test]
    fn all_shapes_have_outward_winding_finite_normals_and_uvs() {
        for shape in [
            "plane", "box", "sphere", "cylinder", "cone", "capsule", "torus",
        ] {
            let geometry = SceneParametricGeometry {
                shape,
                segments: Some(11),
                rings: Some(7),
                ..Default::default()
            };
            let parts = geometry.recipe().unwrap().vertices();
            for (slot, vertices) in parts {
                assert!(!vertices.is_empty());
                for [a, b, c] in vertices.as_chunks::<3>().0 {
                    let normal = (Vector3::from(b.position) - Vector3::from(a.position))
                        .cross(&(Vector3::from(c.position) - Vector3::from(a.position)));
                    let face_normal =
                        Vector3::from(a.normal) + Vector3::from(b.normal) + Vector3::from(c.normal);
                    assert!(
                        normal.dot(&face_normal) > 0.0,
                        "{shape} / {slot}: {normal:?}, {face_normal:?}"
                    );
                    for vertex in [a, b, c] {
                        assert!((Vector3::from(vertex.normal).norm() - 1.0).abs() < 1e-5);
                        assert!(vertex.uv.iter().all(|uv| (0.0..=1.0).contains(uv)));
                    }
                }
            }
        }
    }
}
