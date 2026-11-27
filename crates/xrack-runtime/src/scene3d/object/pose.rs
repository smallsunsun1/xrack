//! Posed mesh bounds and triangle deformation shared by scene queries.

use crate::Color;
use nalgebra::{Matrix3, Vector3};
use std::sync::Arc;

use super::SceneObject;
use crate::scene3d::{
    JointPalette, Mesh3D, MeshInfluence, MeshPbr, MeshTangent, MeshVertex, ModelBlendPose,
    MorphGeometry, MorphWeights,
    mesh::{morph::MorphPose, skin::SkinPose},
};

impl SceneObject {
    pub(crate) fn with_morph(mut self, weights: MorphWeights) -> Option<Self> {
        if weights.iter().any(|weight| *weight != 0.0) {
            self.morph = Some(Arc::new(MorphPose::new(
                self.mesh.morph_geometry()?,
                weights,
                self.mesh.bounds(),
            )?));
            self.update_styled_blend_centers(true);
        }
        Some(self)
    }

    pub fn morph_weights(&self) -> Option<&MorphWeights> {
        self.morph.as_ref().map(|pose| &pose.weights)
    }

    pub(crate) fn with_skin(mut self, palette: JointPalette) -> Option<Self> {
        self.skin = Some(Arc::new(SkinPose::new(
            self.mesh.skin_geometry()?,
            palette,
            self.morph.as_deref(),
        )?));
        self.update_styled_blend_centers(true);
        Some(self)
    }

    pub fn skin_palette(&self) -> Option<&JointPalette> {
        self.skin.as_ref().map(|skin| &skin.palette)
    }

    pub(crate) fn with_capture_mesh(mut self, mesh: Mesh3D) -> Self {
        self.mesh = mesh;
        self.color = Color(0xffffffff);
        self.styled_blend_centers = None;
        self.update_styled_blend_centers(false);
        self
    }

    fn update_styled_blend_centers(&mut self, inherit: bool) {
        let Mesh3D::Imported(mesh) = &self.mesh else {
            return;
        };
        if (inherit && mesh.authored.is_none())
            || self.mesh.blended_parts().is_empty()
            || self.skin.is_none() && self.morph.is_none()
        {
            return;
        }
        let inherited = self
            .skin
            .as_ref()
            .map(|skin| skin.blended_parts.as_slice())
            .or_else(|| {
                self.morph
                    .as_ref()
                    .map(|morph| morph.blended_parts.as_slice())
            })
            .unwrap_or_default();
        if inherit
            && inherited.iter().map(|(index, _)| index).eq(self
                .mesh
                .blended_parts()
                .iter()
                .map(|(index, _)| index))
        {
            self.styled_blend_centers = None;
            return;
        }
        let pose = self.pick_pose();
        let triangles = self.mesh.vertices().as_chunks::<3>().0;
        self.styled_blend_centers = Some(
            self.mesh
                .blended_parts()
                .iter()
                .map(|&(index, _)| {
                    if inherit
                        && let Ok(old) = inherited.binary_search_by_key(&index, |(part, _)| *part)
                    {
                        return inherited[old];
                    }
                    let mut min = [f32::INFINITY; 3];
                    let mut max = [f32::NEG_INFINITY; 3];
                    for vertex in self.mesh.parts()[index].vertices.clone().step_by(3) {
                        for vertex in pose.deform::<false>(vertex, triangles[vertex / 3]) {
                            for axis in 0..3 {
                                min[axis] = min[axis].min(vertex.position[axis]);
                                max[axis] = max[axis].max(vertex.position[axis]);
                            }
                        }
                    }
                    (
                        index,
                        std::array::from_fn(|axis| f32::midpoint(min[axis], max[axis])),
                    )
                })
                .collect(),
        );
    }

    pub fn blended_parts(&self) -> &[(usize, [f32; 3])] {
        if let Some(centers) = &self.styled_blend_centers {
            centers
        } else if self.mesh.blended_parts().is_empty() {
            &[]
        } else if let Some(skin) = &self.skin {
            &skin.blended_parts
        } else if let Some(morph) = &self.morph {
            &morph.blended_parts
        } else {
            self.mesh.blended_parts()
        }
    }

    pub(super) fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if let Some(skin) = &self.skin {
            skin.bounds
        } else if let Some(morph) = &self.morph {
            morph.bounds
        } else {
            self.mesh.bounds()
        }
    }

    pub(crate) fn posed_triangles(&self) -> impl Iterator<Item = [[f32; 3]; 3]> + '_ {
        let deformation = self.pick_pose();
        self.mesh
            .vertices()
            .as_chunks::<3>()
            .0
            .iter()
            .enumerate()
            .map(move |(index, triangle)| {
                let mut points = deformation
                    .deform::<false>(index * 3, *triangle)
                    .map(|vertex| {
                        (self.transform.origin
                            + self.transform.linear * Vector3::from(vertex.position))
                        .into()
                    });
                if self.is_reflected() {
                    points.swap(1, 2);
                }
                points
            })
    }

    pub(crate) fn particle_prototype(&self) -> Option<Mesh3D> {
        let pose = self.pick_pose();
        if self.transform.linear == Matrix3::identity()
            && self.transform.origin == Vector3::zeros()
            && !pose.is_deformed()
        {
            return self.mesh.clone().with_particle_tint(self.color);
        }
        let mesh = self.mesh.material_source()?;
        let mut vertices = Vec::with_capacity(self.mesh.vertices().len());
        let normal_transform = self.transform.inverse.transpose();
        let mut triangle_bases = Vec::new();
        for (index, triangle) in self.mesh.vertices().as_chunks::<3>().0.iter().enumerate() {
            let vertices_in_pose = if let Some(blend) = pose.blend
                && !self.mesh.tangents().is_empty()
            {
                let (deformed, basis) =
                    blend.deform_with_basis::<true, true>(index as u32, *triangle);
                triangle_bases.push(basis);
                deformed
            } else {
                pose.deform::<true>(index * 3, *triangle)
            };
            vertices.extend(vertices_in_pose.map(|mut vertex| {
                vertex.position = (self.transform.origin
                    + self.transform.linear * Vector3::from(vertex.position))
                .into();
                let normal = normal_transform * Vector3::from(vertex.normal);
                vertex.normal = normal.try_normalize(1e-12).unwrap_or(Vector3::y()).into();
                vertex
            }));
        }
        let (mut tangents, mut binormals) =
            self.bake_particle_tangent_frame(&pose, &triangle_bases);
        if self.is_reflected() {
            for triangle in vertices.as_chunks_mut::<3>().0 {
                triangle.swap(1, 2);
            }
            for triangle in tangents.as_chunks_mut::<3>().0 {
                triangle.swap(1, 2);
            }
            for triangle in binormals.as_chunks_mut::<3>().0 {
                triangle.swap(1, 2);
            }
        }
        let mut parts = Mesh3D::Imported(mesh.clone()).parts().to_vec();
        for part in &mut parts {
            let material = &mesh.materials[part.material];
            if material.pbr.as_ref().and_then(MeshPbr::volume).is_some() {
                let previous = material
                    .volume_inverse(part.volume_inverse)
                    .map_or_else(Matrix3::identity, |rows| {
                        Matrix3::from_fn(|r, c| rows[r][c])
                    });
                let inverse = previous * self.transform.inverse;
                part.volume_inverse = Some(std::array::from_fn(|r| {
                    std::array::from_fn(|c| inverse[(r, c)])
                }));
            }
        }
        let mut materials = mesh.materials.clone();
        if materials.iter().any(|material| {
            material
                .pbr
                .as_ref()
                .and_then(MeshPbr::volume)
                .is_some_and(|volume| volume.thickness_in_prepared_space)
        }) {
            // The prepared model was baked again as a particle prototype.
            // Its parts now carry the resolved optical basis.
            for material in Arc::make_mut(&mut materials) {
                if let Some(volume) = material
                    .pbr
                    .as_mut()
                    .and_then(|pbr| pbr.transmission.as_deref_mut())
                    .and_then(|term| term.volume.as_deref_mut())
                {
                    volume.thickness_in_prepared_space = false;
                }
            }
        }
        Mesh3D::imported_shared(vertices, tangents, parts, materials)
            .ok()?
            .with_material_slots(mesh.material_slots.clone())?
            .with_particle_basis(binormals, mesh)?
            .with_particle_tint(self.color)
    }

    fn bake_particle_tangent_frame(
        &self,
        pose: &PickPose<'_>,
        triangle_bases: &[Option<(Matrix3<f32>, bool)>],
    ) -> (Vec<MeshTangent>, Vec<MeshTangent>) {
        let tangents = self.mesh.tangents();
        let binormals = self.mesh.binormals();
        let explicit = !binormals.is_empty()
            || pose
                .morph
                .is_some_and(|(_, geometry)| geometry.has_binormals());
        let mut baked_tangents = Vec::with_capacity(tangents.len());
        let mut baked_binormals = Vec::with_capacity(if explicit { tangents.len() } else { 0 });
        for destination in 0..tangents.len() {
            let basis = triangle_bases.get(destination / 3).copied().flatten();
            let reflected = basis.is_some_and(|(_, reflected)| reflected);
            let corner = destination % 3;
            let index = if reflected && corner != 0 {
                destination - corner + 3 - corner
            } else {
                destination
            };
            let [x, y, z, original_sign] = tangents[index];
            let tangent = Vector3::new(f32::from(x), f32::from(y), f32::from(z)) / 32767.0;
            let (tangent_delta, binormal_delta) = pose
                .morph
                .map(|(pose, geometry)| geometry.frame_displacement(index, &pose.weights))
                .unwrap_or_default();
            let skin = pose
                .skin
                .and_then(|(pose, influences)| influences[index].deformation_rows(&pose.palette))
                .map(|rows| Matrix3::from_fn(|row, col| rows[row][col]));
            let mirrored = self.is_reflected()
                ^ reflected
                ^ skin.is_some_and(|linear| linear.determinant() < 0.);
            let local = basis.map(|(linear, _)| linear).or(skin);
            let transform = local.map_or(self.transform.linear, |linear| {
                self.transform.linear * linear
            });
            let sign = if mirrored {
                original_sign.saturating_neg()
            } else {
                original_sign
            };
            baked_tangents.push(pack_direction(transform * (tangent + tangent_delta), sign));
            if explicit {
                let rest = if let Some([x, y, z, _]) = binormals.get(index).copied() {
                    Vector3::new(f32::from(x), f32::from(y), f32::from(z)) / 32767.0
                } else {
                    let normal = Vector3::from(self.mesh.vertices()[index].normal);
                    (normal.cross(&tangent))
                        .try_normalize(1e-8)
                        .unwrap_or_default()
                        * (f32::from(original_sign) / 32767.0)
                };
                baked_binormals.push(pack_direction(transform * (rest + binormal_delta), 0));
            }
        }
        (baked_tangents, baked_binormals)
    }

    pub(super) fn pick_pose(&self) -> PickPose<'_> {
        PickPose {
            skin: self.skin.as_deref().zip(self.mesh.influences()),
            morph: self.morph.as_deref().zip(self.mesh.morph_geometry()),
            blend: self.mesh.model_blend(),
        }
    }
}

fn pack_direction(direction: Vector3<f32>, fourth: i16) -> MeshTangent {
    let Some(direction) = direction.try_normalize(1e-12) else {
        return [0; 4];
    };
    let [x, y, z] =
        [direction.x, direction.y, direction.z].map(|axis| (axis * 32767.0).round() as i16);
    [x, y, z, fourth]
}

pub(super) struct PickPose<'a> {
    skin: Option<(&'a SkinPose, &'a [MeshInfluence])>,
    morph: Option<(&'a MorphPose, &'a MorphGeometry)>,
    blend: Option<ModelBlendPose>,
}

impl PickPose<'_> {
    pub(super) fn is_deformed(&self) -> bool {
        self.skin.is_some() || self.morph.is_some() || self.blend.is_some()
    }

    pub(super) fn moves_positions(&self) -> bool {
        self.skin.is_some()
            || self.blend.is_some()
            || self.morph.is_some_and(|(pose, _)| pose.moves_positions())
    }

    pub(super) fn material_triangle(
        &self,
        vertex: usize,
        mut triangle: [MeshVertex; 3],
    ) -> [MeshVertex; 3] {
        if let Some((pose, geometry)) = self.morph {
            for (i, point) in triangle.iter_mut().enumerate() {
                *point = geometry.deform_surface(vertex + i, &pose.weights, *point);
            }
        }
        triangle
    }

    pub(super) fn deform<const NORMALS: bool>(
        &self,
        vertex: usize,
        mut triangle: [MeshVertex; 3],
    ) -> [MeshVertex; 3] {
        if let Some((pose, geometry)) = self.morph
            && (NORMALS || pose.moves_positions())
        {
            for (i, point) in triangle.iter_mut().enumerate() {
                *point = geometry.deform::<NORMALS, NORMALS>(vertex + i, &pose.weights, *point);
            }
        }
        if let Some((pose, weights)) = self.skin {
            for (i, point) in triangle.iter_mut().enumerate() {
                *point = pose.deform::<NORMALS>(weights[vertex + i], *point);
            }
        }
        self.blend.map_or(triangle, |blend| {
            blend.deform::<NORMALS>((vertex / 3) as u32, triangle)
        })
    }
}
