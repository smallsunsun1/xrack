//! Posed mesh bounds and triangle deformation shared by scene queries.

use crate::Color;
use nalgebra::{Matrix3, Vector3};
use std::sync::Arc;

use super::SceneObject;
use crate::scene3d::{
    JointPalette, Mesh3D, MeshInfluence, MeshPbr, MeshTangent, MeshTopology, MeshVertex,
    ModelBlendPose, MorphGeometry, MorphWeights, SceneTransform,
    bounds::Bounds3D,
    mesh::{morph::MorphPose, skin::SkinPose},
};

impl SceneObject {
    pub(crate) fn relative_to(mut self, root: SceneTransform) -> Option<Self> {
        self.transform = self.transform.relative_to(root)?;
        Some(self)
    }

    pub(crate) fn with_parent_transform(mut self, parent: SceneTransform) -> Option<Self> {
        self.transform = parent.child(self.transform)?;
        Some(self)
    }

    fn render_bounds(&self) -> ([f32; 3], [f32; 3]) {
        let (min, max) = self.bounds();
        let expansion = self.mesh.shader_displacement();
        (
            min.map(|point| point - expansion),
            max.map(|point| point + expansion),
        )
    }

    pub(crate) fn world_bounds(&self) -> Bounds3D {
        let (min, max) = self.render_bounds();
        Bounds3D { min, max }.transform(self.transform_rows())
    }

    /// Corners of the transformed mesh bounds.
    pub fn bounds_corners(&self) -> [[f32; 3]; 8] {
        let (min, max) = self.render_bounds();
        let transform = self.transform_rows();
        std::array::from_fn(|corner| {
            let [x, y, z] = std::array::from_fn(|axis| {
                if corner & (1 << axis) == 0 {
                    min[axis]
                } else {
                    max[axis]
                }
            });
            transform.map(|[a, b, c, d]| a * x + b * y + c * z + d)
        })
    }

    /// Rows of the model-to-world affine transform, including translation.
    pub fn transform_rows(&self) -> [[f32; 4]; 3] {
        self.transform.rows()
    }

    /// Whether this object's model-to-world transform reverses orientation.
    pub fn is_reflected(&self) -> bool {
        self.transform.reflected
    }

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
                    let part = &self.mesh.parts()[index];
                    let mut include = |vertex: MeshVertex| {
                        for axis in 0..3 {
                            min[axis] = min[axis].min(vertex.position[axis]);
                            max[axis] = max[axis].max(vertex.position[axis]);
                        }
                    };
                    if part.topology == MeshTopology::Triangles {
                        for vertex in part.vertices.clone().step_by(3) {
                            for vertex in
                                pose.deform::<false>(vertex, *self.mesh.triangle_at(vertex))
                            {
                                include(vertex);
                            }
                        }
                    } else {
                        for vertex in part.vertices.clone() {
                            include(
                                pose.deform_vertices::<false, 1>(
                                    vertex,
                                    [self.mesh.vertices()[vertex]],
                                )[0],
                            );
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
            .triangle_sections()
            .flat_map(|(range, _)| range.step_by(3))
            .map(move |vertex| {
                let mut points = deformation
                    .deform::<false>(vertex, *self.mesh.triangle_at(vertex))
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
        let mut parts = Mesh3D::Imported(mesh.clone()).parts().to_vec();
        let mut push = |mut vertex: MeshVertex| {
            vertex.position = (self.transform.origin
                + self.transform.linear * Vector3::from(vertex.position))
            .into();
            let normal = normal_transform * Vector3::from(vertex.normal);
            vertex.normal = normal.try_normalize(1e-12).unwrap_or(Vector3::y()).into();
            vertices.push(vertex);
        };
        for part in &parts {
            if part.topology != MeshTopology::Triangles {
                for vertex in part.vertices.clone() {
                    let [posed] =
                        pose.deform_vertices::<true, 1>(vertex, [self.mesh.vertices()[vertex]]);
                    push(posed);
                }
                continue;
            }
            for vertex in part.vertices.clone().step_by(3) {
                let triangle = *self.mesh.triangle_at(vertex);
                let vertices_in_pose = if let Some(blend) = pose.blend
                    && !self.mesh.tangents().is_empty()
                {
                    let (deformed, basis) =
                        blend.deform_with_basis::<true, true>((vertex / 3) as u32, triangle);
                    triangle_bases.push(basis);
                    deformed
                } else {
                    pose.deform::<true>(vertex, triangle)
                };
                vertices_in_pose.into_iter().for_each(&mut push);
            }
        }
        let (mut tangents, mut binormals) =
            self.bake_particle_tangent_frame(&pose, &triangle_bases);
        if self.is_reflected() {
            for range in parts
                .iter()
                .filter(|part| part.topology == MeshTopology::Triangles)
                .map(|part| part.vertices.clone())
            {
                for triangle in vertices[range.clone()].as_chunks_mut::<3>().0 {
                    triangle.swap(1, 2);
                }
                if let Some(tangents) = tangents.get_mut(range.clone()) {
                    for triangle in tangents.as_chunks_mut::<3>().0 {
                        triangle.swap(1, 2);
                    }
                }
                if let Some(binormals) = binormals.get_mut(range) {
                    for triangle in binormals.as_chunks_mut::<3>().0 {
                        triangle.swap(1, 2);
                    }
                }
            }
        }
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

    pub(super) fn material_vertices<const N: usize>(
        &self,
        vertex: usize,
        mut vertices: [MeshVertex; N],
    ) -> [MeshVertex; N] {
        if let Some((pose, geometry)) = self.morph {
            for (i, point) in vertices.iter_mut().enumerate() {
                *point = geometry.deform_surface(vertex + i, &pose.weights, *point);
            }
        }
        vertices
    }

    pub(super) fn deform<const NORMALS: bool>(
        &self,
        vertex: usize,
        triangle: [MeshVertex; 3],
    ) -> [MeshVertex; 3] {
        let triangle = self.deform_vertices::<NORMALS, 3>(vertex, triangle);
        self.blend.map_or(triangle, |blend| {
            blend.deform::<NORMALS>((vertex / 3) as u32, triangle)
        })
    }

    pub(super) fn deform_vertices<const NORMALS: bool, const N: usize>(
        &self,
        vertex: usize,
        mut vertices: [MeshVertex; N],
    ) -> [MeshVertex; N] {
        if let Some((pose, geometry)) = self.morph
            && (NORMALS || pose.moves_positions())
        {
            for (i, point) in vertices.iter_mut().enumerate() {
                *point = geometry.deform::<NORMALS, NORMALS>(vertex + i, &pose.weights, *point);
            }
        }
        if let Some((pose, weights)) = self.skin {
            for (i, point) in vertices.iter_mut().enumerate() {
                *point = pose.deform::<NORMALS>(weights[vertex + i], *point);
            }
        }
        vertices
    }
}
