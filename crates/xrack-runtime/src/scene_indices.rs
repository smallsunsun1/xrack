//! Compact GPU draw data for triangle meshes.
use std::{
    collections::HashMap,
    hash::{Hash, Hasher},
};

use crate::scene3d::{MeshInfluence, MeshTangent, MeshVertex, MorphDelta, MorphGeometry};

pub(crate) const MIN_INDEXED_VERTICES: usize = 64;
const MIN_SAVED_BYTES: usize = 256;

/// Source rows for compact attribute buffers and elements in original triangle order.
#[doc(hidden)]
#[derive(Debug)]
pub struct MeshDrawIndices {
    pub rows: Box<[u32]>,
    pub elements: MeshIndexElements,
}

#[doc(hidden)]
#[derive(Debug)]
pub enum MeshIndexElements {
    U16(Box<[u16]>),
    U32(Box<[u32]>),
}

struct VertexKey<'a> {
    vertex: &'a MeshVertex,
    tangent: Option<&'a MeshTangent>,
    lightmap_uv: Option<&'a [f32; 2]>,
    influence: Option<&'a MeshInfluence>,
    deltas: &'a [MorphDelta],
}

impl VertexKey<'_> {
    fn attribute_words(&self) -> impl Iterator<Item = u32> {
        let vertex = self.vertex;
        vertex
            .position
            .into_iter()
            .chain(vertex.normal)
            .chain(vertex.uv)
            .chain(vertex.uv1)
            .chain(vertex.color)
            .chain(self.lightmap_uv.into_iter().flatten().copied())
            .map(f32::to_bits)
    }

    fn influence_bytes(&self) -> Option<[u8; MeshInfluence::STRIDE]> {
        self.influence.copied().map(MeshInfluence::packed)
    }
}

impl Hash for VertexKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.attribute_words().for_each(|word| word.hash(state));
        self.tangent.hash(state);
        self.influence_bytes().hash(state);
        self.deltas.len().hash(state);
        self.deltas
            .iter()
            .for_each(|delta| delta.words().hash(state));
    }
}

impl PartialEq for VertexKey<'_> {
    fn eq(&self, other: &Self) -> bool {
        self.attribute_words().eq(other.attribute_words())
            && self.tangent == other.tangent
            && self.influence_bytes() == other.influence_bytes()
            && self
                .deltas
                .iter()
                .map(MorphDelta::words)
                .eq(other.deltas.iter().map(MorphDelta::words))
    }
}

impl Eq for VertexKey<'_> {}

impl MeshDrawIndices {
    pub(crate) fn prepare(
        vertices: &[MeshVertex],
        tangents: &[MeshTangent],
        lightmap_uv: &[[f32; 2]],
        influences: Option<&[MeshInfluence]>,
        morph: Option<&MorphGeometry>,
    ) -> Option<Self> {
        if vertices.len() < MIN_INDEXED_VERTICES {
            return None;
        }
        let mut unique = HashMap::new();
        let mut rows = Vec::new();
        let mut elements = Vec::with_capacity(vertices.len());
        for (row, vertex) in vertices.iter().enumerate() {
            let key = VertexKey {
                vertex,
                tangent: tangents.get(row),
                lightmap_uv: lightmap_uv.get(row),
                influence: influences.map(|weights| &weights[row]),
                deltas: morph.map_or(&[], |morph| morph.vertex(row)),
            };
            let index = *unique.entry(key).or_insert_with(|| {
                let index = rows.len() as u32;
                rows.push(row as u32);
                index
            });
            elements.push(index);
        }
        let compact_short = rows.len() <= usize::from(u16::MAX) + 1;
        // Lightmap coordinates are uploaded only when a draw needs them.
        let vertex_stride = MeshVertex::STRIDE
            + if tangents.is_empty() {
                0
            } else {
                size_of::<MeshTangent>()
            }
            + influences.map_or(0, |_| MeshInfluence::STRIDE);
        let removed_morph_bytes = morph.map_or(0, |morph| {
            let retained_deltas: usize = rows
                .iter()
                .map(|&row| morph.vertex(row as usize).len())
                .sum();
            (morph.delta_count() - retained_deltas) * size_of::<[u32; 10]>()
                + (vertices.len() - rows.len()) * size_of::<u32>()
        });
        let removed = (vertices.len() - rows.len()) * vertex_stride + removed_morph_bytes;
        let index_bytes = elements.len()
            * if compact_short {
                size_of::<u16>()
            } else {
                size_of::<u32>()
            };
        if removed < index_bytes + MIN_SAVED_BYTES {
            return None;
        }
        let elements = if compact_short {
            MeshIndexElements::U16(elements.into_iter().map(|index| index as u16).collect())
        } else {
            MeshIndexElements::U32(elements.into_boxed_slice())
        };
        Some(Self {
            rows: rows.into_boxed_slice(),
            elements,
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::scene_primitives::vertex;

    #[test]
    fn indices_preserve_all_vertex_attributes_and_triangle_order() {
        let point = vertex([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0; 2]);
        let variant = MeshVertex {
            uv1: [0.4, 0.8],
            ..point
        };
        let vertices = [point, variant].repeat(60);
        let mut tangents = vec![[0, 0, 0, 32767]; vertices.len()];
        tangents[0][3] = -32767;
        let mut lightmap_uv = vec![[0.5, 0.5]; vertices.len()];
        lightmap_uv[1] = [1.0, 0.5];
        let mesh =
            MeshDrawIndices::prepare(&vertices, &tangents, &lightmap_uv, None, None).unwrap();
        let MeshIndexElements::U16(elements) = mesh.elements else {
            panic!("four compact rows should use 16-bit indices");
        };
        assert_eq!(mesh.rows.len(), 4);
        assert_eq!(elements.len(), vertices.len());
        for (row, index) in elements.iter().copied().enumerate() {
            let index = mesh.rows[usize::from(index)] as usize;
            assert_eq!(vertices[row], vertices[index]);
            assert_eq!(tangents[row], tangents[index]);
            assert_eq!(lightmap_uv[row], lightmap_uv[index]);
        }
    }

    #[test]
    fn deformation_distinguishes_equal_vertices_and_packs_sparse_rows() {
        let point = vertex([1.0, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0; 2]);
        let vertices = vec![point; 96];
        let unchanged = MeshInfluence {
            joints: [0; 8],
            weights: [1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
        };
        let different_joint = MeshInfluence {
            joints: [1, 0, 0, 0, 0, 0, 0, 0],
            ..unchanged
        };
        let influences = [unchanged, unchanged, different_joint, unchanged].repeat(24);
        let mut offsets = vec![0];
        let mut deltas = Vec::new();
        for row in 0..vertices.len() {
            match row % 4 {
                0 => {}
                other => deltas.push(MorphDelta {
                    target: u32::from(other == 3),
                    position: [0.25, 0.0, 0.0],
                    ..Default::default()
                }),
            }
            offsets.push(deltas.len() as u32);
        }
        let morph = MorphGeometry::new(
            2,
            Default::default(),
            offsets,
            deltas,
            &vertices,
            std::iter::empty(),
        )
        .unwrap();
        let mesh =
            MeshDrawIndices::prepare(&vertices, &[], &[], Some(&influences), Some(&morph)).unwrap();
        assert_eq!(&*mesh.rows, &[0, 1, 2, 3]);
        let MeshIndexElements::U16(elements) = mesh.elements else {
            panic!("four compact rows should use 16-bit indices");
        };
        assert_eq!(&*elements, &[0, 1, 2, 3].repeat(24));
        let packed = morph.packed_draw_rows(&mesh.rows).collect::<Vec<_>>();
        assert_eq!(&packed[..5], &[0, 0, 1, 2, 3]);
        assert_eq!(packed.len(), 35);
        assert_eq!(packed[5], 0);
        assert_eq!(packed[25], 1);
        assert_eq!(packed[26], 0.25f32.to_bits());
    }

    #[test]
    fn large_compact_streams_preserve_indices_above_u16_max() {
        let source: Vec<_> = (0..65_538)
            .map(|index| vertex([index as f32, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0; 2]))
            .collect();
        let mesh = MeshDrawIndices::prepare(&source.repeat(2), &[], &[], None, None).unwrap();
        let MeshIndexElements::U32(elements) = mesh.elements else {
            panic!("large compact buffers need 32-bit indices");
        };
        assert_eq!(mesh.rows.len(), source.len());
        assert_eq!(elements[source.len() - 1], 65_537);
        assert_eq!(elements[source.len()], 0);
    }

    #[test]
    fn unique_geometry_retains_its_direct_vertex_stream() {
        let vertices: Vec<_> = (0..128)
            .map(|i| vertex([i as f32, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0; 2]))
            .collect();
        assert!(MeshDrawIndices::prepare(&vertices, &[], &[], None, None).is_none());
    }

    #[test]
    fn savings_do_not_depend_on_uploading_optional_lightmap_uvs() {
        let mut vertices: Vec<_> = (0..89)
            .map(|i| vertex([i as f32, 0.0, 0.0], [0.0, 0.0, 1.0], [0.0; 2]))
            .collect();
        vertices.resize(96, vertices[0]);
        let lightmap_uv = [[0.5; 2]; 96];
        assert!(MeshDrawIndices::prepare(&vertices, &[], &lightmap_uv, None, None).is_none());
    }
}
