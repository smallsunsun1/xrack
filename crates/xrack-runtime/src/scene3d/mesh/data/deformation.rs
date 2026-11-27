//! Skin weights, morph targets, and particle model blend poses.

use std::sync::Arc;

use super::{Mesh3D, materials::model_blend_primitive_materials};
use crate::scene3d::{
    ModelBlend, ModelBlendPose,
    hierarchy::SceneTransform,
    mesh::{
        morph::{
            MAX_MORPH_DELTAS, MORPH_ATTRIBUTE_WORDS, MORPH_WORDS, MorphAttributes, MorphDelta,
            MorphGeometry, MorphTable,
        },
        skin::{MeshInfluence, SkinGeometry},
    },
};

impl Mesh3D {
    pub(crate) fn with_model_blend(
        self,
        blend: Option<&ModelBlend>,
        transform: SceneTransform,
        row: SceneTransform,
    ) -> (Self, SceneTransform) {
        let Some(blend) =
            blend.filter(|_| self.skin_geometry().is_none() && self.morph_geometry().is_none())
        else {
            return (self, transform);
        };
        let Some(pose) = blend.pose(
            self.bounds(),
            (self.vertices().len() / 3) as u32,
            transform,
            row,
        ) else {
            return (self, transform);
        };
        if let Some(landed) = pose
            .settled_transform()
            .and_then(|end| transform.child(end))
        {
            return (self, landed);
        }
        let Some(source) = self.material_source() else {
            return (self, transform);
        };
        let mut data = source.as_ref().clone();
        if matches!(self, Self::Box | Self::Sphere) {
            data.materials = model_blend_primitive_materials().clone();
            data.has_face_culling = false;
        }
        data.model_blend = Some(Arc::new(pose));
        (Self::Imported(Arc::new(data)), transform)
    }

    #[doc(hidden)]
    pub fn model_blend(&self) -> Option<ModelBlendPose> {
        let Self::Imported(mesh) = self else {
            return None;
        };
        mesh.model_blend.as_deref().copied()
    }

    pub(crate) fn compatible_lod(&self, level: &Self) -> bool {
        let same_skin = match (self.skin_geometry(), level.skin_geometry()) {
            (None, None) => true,
            (Some(a), Some(b)) => b.joint_count <= a.joint_count,
            _ => false,
        };
        let same_morph = match (self.morph_geometry(), level.morph_geometry()) {
            (None, None) => true,
            (Some(a), Some(b)) => {
                a.target_count() == b.target_count() && a.target_names() == b.target_names()
            }
            _ => false,
        };
        same_skin && same_morph
    }

    #[doc(hidden)]
    pub fn with_skin_weights(mut self, weights: Vec<MeshInfluence>) -> Option<Self> {
        let Self::Imported(mesh) = &mut self else {
            return None;
        };
        let mesh = Arc::get_mut(mesh)?;
        let skin = SkinGeometry::new(
            weights,
            &mesh.vertices,
            mesh.blended_parts
                .iter()
                .map(|(part, _)| (*part, mesh.parts[*part].vertices.clone())),
        )?;
        let geometry = Arc::get_mut(&mut mesh.geometry)?;
        Arc::get_mut(&mut geometry.streams)?.invalidate_indices();
        geometry.skin = Some(Arc::new(skin));
        Some(self)
    }

    #[doc(hidden)]
    pub fn with_packed_skin(self, bytes: &[u8]) -> Option<Self> {
        if !bytes.len().is_multiple_of(MeshInfluence::STRIDE) {
            return None;
        }
        let weights = bytes
            .as_chunks::<{ MeshInfluence::STRIDE }>()
            .0
            .iter()
            .copied()
            .map(MeshInfluence::from_packed)
            .collect();
        self.with_skin_weights(weights)
    }

    pub fn influences(&self) -> Option<&[MeshInfluence]> {
        self.skin_geometry()
            .map(|geometry| geometry.weights.as_slice())
    }

    pub(crate) fn skin_geometry(&self) -> Option<&SkinGeometry> {
        match self {
            Self::Imported(mesh) => mesh.skin.as_deref(),
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn with_morph(
        self,
        target_count: usize,
        names: Arc<[String]>,
        offsets: Vec<u32>,
        deltas: Vec<MorphDelta>,
    ) -> Option<Self> {
        self.with_morph_table(
            target_count,
            names,
            MorphTable {
                offsets,
                deltas,
                attributes: Vec::new(),
            },
        )
    }

    pub(in crate::scene3d::mesh) fn with_morph_table(
        mut self,
        target_count: usize,
        names: Arc<[String]>,
        table: MorphTable,
    ) -> Option<Self> {
        let Self::Imported(mesh) = &mut self else {
            return None;
        };
        let mesh = Arc::get_mut(mesh)?;
        let morph = MorphGeometry::new(
            target_count,
            names,
            table,
            &mesh.vertices,
            mesh.blended_parts
                .iter()
                .map(|(part, _)| (*part, mesh.parts[*part].vertices.clone())),
        )?;
        if morph.has_binormals() && mesh.tangents.is_empty() {
            return None;
        }
        let geometry = Arc::get_mut(&mut mesh.geometry)?;
        Arc::get_mut(&mut geometry.streams)?.invalidate_indices();
        geometry.morph = Some(Arc::new(morph));
        Some(self)
    }

    #[doc(hidden)]
    pub fn with_packed_morph(
        self,
        target_count: usize,
        names: &[&str],
        bytes: &[u8],
    ) -> Option<Self> {
        let count = self.vertices().len() + 1;
        let (words, remainder) = bytes.as_chunks::<4>();
        if !remainder.is_empty() {
            return None;
        }
        let offsets: Vec<_> = words
            .get(..count)?
            .iter()
            .copied()
            .map(u32::from_le_bytes)
            .collect();
        let record_count = *offsets.last()? as usize;
        if record_count > MAX_MORPH_DELTAS {
            return None;
        }
        let payload = &words[count..];
        let stride = if payload.len() == record_count * MORPH_WORDS {
            MORPH_WORDS
        } else if payload.len() == record_count * (MORPH_WORDS + MORPH_ATTRIBUTE_WORDS) {
            MORPH_WORDS + MORPH_ATTRIBUTE_WORDS
        } else {
            return None;
        };
        let mut deltas = Vec::with_capacity(record_count);
        let mut attributes = Vec::new();
        for record in payload.chunks_exact(stride) {
            let float = |offset: usize| f32::from_le_bytes(record[offset]);
            let vector = |offset| std::array::from_fn(|axis| float(offset + axis));
            deltas.push(MorphDelta {
                target: u32::from_le_bytes(record[0]),
                position: vector(1),
                normal: vector(4),
                tangent: vector(7),
            });
            if stride > MORPH_WORDS {
                attributes.push(MorphAttributes {
                    uv: [float(10), float(11)],
                    uv1: [float(12), float(13)],
                    color: std::array::from_fn(|axis| float(14 + axis)),
                    binormal: vector(18),
                });
            }
        }
        self.with_morph_table(
            target_count,
            names.iter().map(|name| (*name).to_owned()).collect(),
            MorphTable {
                offsets,
                deltas,
                attributes,
            },
        )
    }

    pub fn morph_geometry(&self) -> Option<&MorphGeometry> {
        match self {
            Self::Imported(mesh) => mesh.morph.as_deref(),
            _ => None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene3d::MeshVertex;

    #[test]
    fn material_only_morph_keeps_the_base_pick_tree() {
        use crate::{
            Color,
            scene3d::{Ray3D, SceneObject, pick::MIN_BVH_TRIANGLES},
        };

        let vertices: Vec<_> = (0..MIN_BVH_TRIANGLES)
            .flat_map(|index| {
                let x = index as f32 * 2.0;
                [[x, 0., 0.], [x + 1., 0., 0.], [x, 1., 0.]].map(|position| MeshVertex {
                    position,
                    normal: [0.0, 0.0, 1.0],
                    uv: [0.0; 2],
                    uv1: [0.0; 2],
                    color: [1.0; 4],
                })
            })
            .collect();
        let mut offsets = vec![1; vertices.len() + 1];
        offsets[0] = 0;
        let mesh = Mesh3D::prepare_procedural(vec![("surface", vertices)])
            .unwrap()
            .with_morph_table(
                1,
                Arc::from(["pan".to_owned()]),
                MorphTable {
                    offsets,
                    deltas: vec![MorphDelta::default()],
                    attributes: vec![MorphAttributes {
                        uv: [0.5, 0.0],
                        ..Default::default()
                    }],
                },
            )
            .unwrap();
        let Mesh3D::Imported(data) = &mesh else {
            panic!("expected prepared mesh");
        };
        assert!(data.pick_tree.get().is_none());
        let object = SceneObject::from_mesh(
            "panel".into(),
            mesh.clone(),
            [0.0; 3],
            [1.0; 3],
            [0.0; 3],
            Color(0xffff_ffff),
        )
        .unwrap()
        .with_morph(Arc::from([1.0]))
        .unwrap();
        let ray = Ray3D::new([0.25, 0.25, 1.0], [0.0, 0.0, -1.0]).unwrap();
        let surface = object.surface_hit(ray, 0.0, 2.0, false).unwrap();
        assert!(data.pick_tree.get().is_some());
        assert_eq!(object.describe_hit(ray, surface).uv, [0.25, 0.0]);
    }

    #[test]
    fn packed_surface_morphs_preserve_attributes_through_decode_and_remapping() {
        let triangle = [[0., 0., 0.], [1., 0., 0.], [0., 1., 0.]].map(|position| MeshVertex {
            position,
            normal: [0.0, 0.0, 1.0],
            uv: [0.0; 2],
            uv1: [0.0; 2],
            color: [1.0; 4],
        });
        let mesh = || {
            Mesh3D::prepare_authored_triangles(triangle.to_vec(), vec![[32767, 0, 0, 32767]; 3])
                .unwrap()
        };
        let attributes = MorphAttributes {
            uv: [0.5, -0.25],
            uv1: [0.0, 0.75],
            color: [-0.6, 0.0, 0.0, -1.0],
            binormal: [1.0, 0.0, 0.0],
        };
        let source = mesh()
            .with_morph_table(
                1,
                Arc::from(["paint".to_owned()]),
                MorphTable {
                    offsets: vec![0, 0, 1, 1],
                    deltas: vec![MorphDelta::default()],
                    attributes: vec![attributes],
                },
            )
            .unwrap();
        let source_geometry = source.morph_geometry().unwrap();
        assert_eq!(
            source_geometry.record_words(),
            MORPH_WORDS + MORPH_ATTRIBUTE_WORDS
        );
        let payload: Vec<_> = source_geometry
            .packed_words()
            .flat_map(u32::to_le_bytes)
            .collect();
        let decoded = mesh().with_packed_morph(1, &["paint"], &payload).unwrap();
        assert!(
            source_geometry
                .packed_words()
                .eq(decoded.morph_geometry().unwrap().packed_words())
        );
        let subset = decoded
            .triangle_subset(&[1, 2, 0], decoded.parts().to_vec())
            .unwrap();
        let geometry = subset.morph_geometry().unwrap();
        assert_eq!(geometry.vertex_attributes(0), &[attributes]);
        assert!(geometry.vertex_attributes(1).is_empty());
        let painted = geometry.deform::<false, true>(0, &[1.0], subset.vertices()[0]);
        assert_eq!(painted.uv, attributes.uv);
        assert_eq!(painted.uv1, attributes.uv1);
        assert_eq!(painted.color[3], 0.0);
        let position_only = geometry.deform::<false, false>(0, &[1.0], subset.vertices()[0]);
        assert_eq!(position_only.color, [1.0; 4]);
    }
}
