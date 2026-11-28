use bevy_mikktspace::{Geometry, TangentSpace};
use gltf::{
    Primitive,
    accessor::{DataType, Dimensions},
    mesh::Semantic,
};
use nalgebra::{Matrix4, Vector3};
use xrack_runtime::scene3d::{MeshTangent, MeshVertex, TextureTransform};

use crate::{
    mesh_attributes::{Attribute, AttributeSource},
    mesh_materials::NormalCoordinates,
};

pub(super) fn prepare(
    primitive: &Primitive<'_>,
    attributes: AttributeSource<'_>,
    transform: &Matrix4<f32>,
    coordinates: NormalCoordinates,
    faces: &[[usize; 3]],
    vertices: &[MeshVertex],
    source_vertices: usize,
) -> Result<Vec<MeshTangent>, String> {
    if vertices.is_empty() {
        return Ok(Vec::new());
    }
    let linear = transform.fixed_view::<3, 3>(0, 0);
    let det = linear.determinant();
    if !det.is_finite() || det == 0.0 {
        return Err("glTF normal map requires an invertible node transform".into());
    }
    let mirrored = det < 0.0;
    let normals = primitive.get(&Semantic::Normals);
    let [u, v] = coordinates.transform.rows;
    let same_tangent_basis = u[0] > 0.0 && v[1] > 0.0 && u[1] == 0.0 && v[0] == 0.0;
    let use_authored = !coordinates.overrides_source_set && same_tangent_basis;
    let authored = if normals.is_some() && use_authored {
        read_authored(primitive, attributes, source_vertices)?
    } else {
        None
    };
    let tangents: Vec<_> = if let Some(authored) = authored {
        faces
            .iter()
            .flat_map(|face| {
                let mut order = *face;
                if mirrored {
                    order.swap(1, 2);
                }
                order.map(|index| authored[index])
            })
            .collect()
    } else {
        // The glTF tangent generator operates on the source geometry. The
        // node's non-uniform transform is applied to its output below.
        let positions: Vec<_> = attributes
            .read::<3>(
                primitive
                    .get(&Semantic::Positions)
                    .ok_or("glTF mesh is missing POSITION")?,
                source_vertices,
                Attribute::Position,
            )?
            .collect();
        let normals = primitive
            .get(&Semantic::Normals)
            .map(|accessor| {
                Ok::<_, String>(
                    attributes
                        .read::<3>(accessor, source_vertices, Attribute::Direction)?
                        .map(|n| {
                            n.into_iter()
                                .all(f32::is_finite)
                                .then(|| Vector3::from(n).try_normalize(1e-8))
                                .flatten()
                        })
                        .collect::<Vec<_>>(),
                )
            })
            .transpose()?;
        let flat_normals = faces
            .iter()
            .map(|face| {
                let [a, b, c] = face.map(|index| Vector3::from(positions[index]));
                (b - a)
                    .cross(&(c - a))
                    .try_normalize(1e-12)
                    .unwrap_or_else(Vector3::z)
                    .into()
            })
            .collect();
        let mut geometry = SourceGeometry {
            positions: &positions,
            normals: normals.as_deref().unwrap_or(&[]),
            flat_normals,
            faces,
            baked_vertices: vertices,
            mirrored,
            coordinates,
            tangents: vec![[0.0; 4]; vertices.len()],
        };
        if vertices.iter().any(|vertex| {
            !mapped_uv(vertex, coordinates)
                .into_iter()
                .all(f32::is_finite)
        }) {
            return Err("glTF normal map contains invalid texture coordinates".into());
        }
        bevy_mikktspace::generate_tangents(&mut geometry)
            .map_err(|error| format!("unable to generate glTF tangents: {error}"))?;
        geometry.tangents
    };
    Ok(pack_tangents(transform, tangents, vertices, mirrored, None))
}

pub(super) fn preserve_authored(
    primitive: &Primitive<'_>,
    attributes: AttributeSource<'_>,
    transform: &Matrix4<f32>,
    coordinates: Option<NormalCoordinates>,
    source_count: usize,
    indices: impl Iterator<Item = usize>,
    vertices: &[MeshVertex],
) -> Result<Option<Vec<MeshTangent>>, String> {
    if primitive.get(&Semantic::Normals).is_none()
        || coordinates.is_some_and(|coordinates| coordinates.overrides_source_set)
    {
        return Ok(None);
    }
    let Some(tangents) = read_authored(primitive, attributes, source_count)? else {
        return Ok(None);
    };
    let determinant = transform.fixed_view::<3, 3>(0, 0).determinant();
    if !determinant.is_finite() || determinant == 0.0 {
        return Err("glTF tangent frame requires an invertible node transform".into());
    }
    let uv = coordinates.map(|coordinates| coordinates.transform);
    Ok(Some(pack_tangents(
        transform,
        indices.map(|index| tangents[index]),
        vertices,
        determinant < 0.0,
        uv,
    )))
}

fn read_authored(
    primitive: &Primitive<'_>,
    attributes: AttributeSource<'_>,
    source_vertices: usize,
) -> Result<Option<Vec<[f32; 4]>>, String> {
    let Some(accessor) = primitive.get(&Semantic::Tangents) else {
        return Ok(None);
    };
    let authored: Vec<_> = attributes
        .read::<4>(accessor, source_vertices, Attribute::Direction)?
        .collect();
    if authored.len() != source_vertices
        || authored
            .iter()
            .any(|values| !values.iter().copied().all(f32::is_finite) || values[3].abs() != 1.0)
    {
        return Err("glTF tangents require finite directions and a sign of +1 or -1".into());
    }
    Ok(Some(authored))
}

fn pack_tangents(
    transform: &Matrix4<f32>,
    tangents: impl IntoIterator<Item = [f32; 4]>,
    vertices: &[MeshVertex],
    mirrored: bool,
    uv: Option<TextureTransform>,
) -> Vec<MeshTangent> {
    let linear = transform.fixed_view::<3, 3>(0, 0);
    let uv_basis = uv.map(|uv| {
        let [[ux, uy, _], [vx, vy, _]] = uv.rows;
        let det = ux * vy - uy * vx;
        (
            vy,
            -vx,
            if det == 0.0 || !det.is_finite() {
                0.0
            } else {
                det.signum()
            },
        )
    });
    tangents
        .into_iter()
        .zip(vertices)
        .map(|([x, y, z, w], vertex)| {
            if w == 0.0 {
                return [0; 4];
            }
            let n = Vector3::from(vertex.normal);
            let t = linear * Vector3::new(x, y, z);
            let Some(t) = (t - n * n.dot(&t)).try_normalize(1e-8) else {
                return [0; 4];
            };
            if !t.iter().copied().all(f32::is_finite) {
                return [0; 4];
            }
            let w = if mirrored { -w } else { w };
            let (t, w) = if let Some((x, y, sign)) = uv_basis {
                // Invert the linear UV transform in the authored tangent basis.
                let bitangent = n.cross(&t) * w;
                let Some(rotated) = ((t * x + bitangent * y) * sign).try_normalize(1e-8) else {
                    return [0; 4];
                };
                (rotated, w * sign)
            } else {
                (t, w)
            };
            [t.x, t.y, t.z, w].map(|value| (value.clamp(-1.0, 1.0) * 32767.0).round() as i16)
        })
        .collect()
}

fn mapped_uv(vertex: &MeshVertex, coordinates: NormalCoordinates) -> [f32; 2] {
    let uv = if coordinates.slot == 0 {
        vertex.uv
    } else {
        vertex.uv1
    };
    if coordinates.transform == TextureTransform::IDENTITY {
        uv
    } else {
        coordinates.transform.map_linear(uv)
    }
}

struct SourceGeometry<'a> {
    positions: &'a [[f32; 3]],
    normals: &'a [Option<Vector3<f32>>],
    flat_normals: Vec<[f32; 3]>,
    faces: &'a [[usize; 3]],
    baked_vertices: &'a [MeshVertex],
    mirrored: bool,
    coordinates: NormalCoordinates,
    tangents: Vec<[f32; 4]>,
}

impl SourceGeometry<'_> {
    fn baked_corner(&self, face: usize, vertex: usize) -> usize {
        3 * face
            + if self.mirrored && vertex != 0 {
                3 - vertex
            } else {
                vertex
            }
    }
}

impl Geometry for SourceGeometry<'_> {
    fn num_faces(&self) -> usize {
        self.faces.len()
    }

    fn num_vertices_of_face(&self, _face: usize) -> usize {
        3
    }

    fn position(&self, face: usize, vertex: usize) -> [f32; 3] {
        self.positions[self.faces[face][vertex]]
    }

    fn normal(&self, face: usize, vertex: usize) -> [f32; 3] {
        self.normals
            .get(self.faces[face][vertex])
            .copied()
            .flatten()
            .map_or(self.flat_normals[face], Into::into)
    }

    fn tex_coord(&self, face: usize, vertex: usize) -> [f32; 2] {
        mapped_uv(
            &self.baked_vertices[self.baked_corner(face, vertex)],
            self.coordinates,
        )
    }

    fn set_tangent(&mut self, tangent: Option<TangentSpace>, face: usize, vertex: usize) {
        let index = self.baked_corner(face, vertex);
        if let Some(tangent) = tangent {
            self.tangents[index] = tangent.tangent_encoded();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn point_line_frames_follow_rotated_uvs_and_mirrored_handedness() {
        let vertices = [MeshVertex {
            position: [0.0; 3],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0; 2],
            uv1: [0.0; 2],
            color: [1.0; 4],
        }];
        let prepare = |rows| {
            pack_tangents(
                &Matrix4::identity(),
                [[1.0, 0.0, 0.0, 1.0]],
                &vertices,
                false,
                Some(TextureTransform { rows }),
            )
        };
        assert_eq!(
            prepare([[0.0, -2.0, 0.25], [3.0, 0.0, 0.0]]),
            [[0, -32767, 0, 32767]],
        );
        assert_eq!(
            prepare([[-2.0, 0.0, 0.0], [0.0, 3.0, 0.0]]),
            [[-32767, 0, 0, -32767]],
        );
        assert_eq!(prepare([[0.0; 3]; 2]), [[0; 4]]);
    }
}
