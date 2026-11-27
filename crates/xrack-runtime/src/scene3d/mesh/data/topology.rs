//! Mesh part layouts, triangle subsets, and sharing of unchanged vertex streams.

use std::{
    collections::BTreeMap,
    sync::{Arc, OnceLock},
};

use super::{Mesh3D, MeshData, MeshGeometry};
use crate::scene3d::mesh::{
    authored::SceneMeshPart,
    material::{MeshMaterialSlot, MeshPart},
};

impl Mesh3D {
    pub(crate) fn with_procedural_layout(&self, layout: &[SceneMeshPart]) -> Option<Self> {
        let Self::Imported(base) = self else {
            return None;
        };
        let material = base.materials.first()?;
        let mut part_centers = Vec::with_capacity(layout.len());
        let mut parts = Vec::with_capacity(layout.len());
        let mut slots = Vec::new();
        let mut indices = BTreeMap::<&str, usize>::new();
        for part in layout {
            let slot = *indices.entry(&part.name).or_insert_with(|| {
                let index = slots.len();
                slots.push(MeshMaterialSlot {
                    source_index: Some(index as u32),
                    name: Some(part.name.clone()),
                });
                index
            });
            let mut low = [f32::INFINITY; 3];
            let mut high = [f32::NEG_INFINITY; 3];
            for vertex in base.vertices.get(part.range.clone())? {
                for axis in 0..3 {
                    low[axis] = low[axis].min(vertex.position[axis]);
                    high[axis] = high[axis].max(vertex.position[axis]);
                }
            }
            part_centers.push(std::array::from_fn(|axis| {
                f32::midpoint(low[axis], high[axis])
            }));
            parts.push(MeshPart {
                vertices: part.range.clone(),
                material: slot,
                volume_inverse: None,
            });
        }
        let geometry = Arc::new(MeshGeometry {
            parts,
            part_centers,
            pick_tree: OnceLock::new(),
            ..(*base.geometry).clone()
        });
        let materials = vec![material.clone(); slots.len()].into();
        let mut mesh = MeshData::with_materials(geometry, materials, None);
        mesh.material_slots = slots.into();
        Some(Self::Imported(Arc::new(mesh)))
    }

    /// Share immutable buffers between preparations of the same imported mesh.
    #[doc(hidden)]
    pub fn share_variant_streams(&mut self, reference: &Self) {
        let (Self::Imported(mesh), Self::Imported(reference)) = (&mut *self, reference) else {
            return;
        };
        if Arc::ptr_eq(mesh.vertex_streams(), reference.vertex_streams())
            || mesh.vertices != reference.vertices
            || mesh.tangents != reference.tangents
            || mesh.binormals != reference.binormals
            || mesh.lightmap_uv != reference.lightmap_uv
            || mesh.skin.as_ref().map(|skin| &skin.weights)
                != reference.skin.as_ref().map(|skin| &skin.weights)
            || match (&mesh.morph, &reference.morph) {
                (None, None) => false,
                (Some(a), Some(b)) => !a.packed_words().eq(b.packed_words()),
                _ => true,
            }
        {
            return;
        }
        Arc::make_mut(&mut Arc::make_mut(mesh).geometry).streams =
            reference.vertex_streams().clone();
    }

    /// Rebuild surviving triangles with the original materials and vertex deformation data.
    #[doc(hidden)]
    pub fn triangle_subset(&self, vertices: &[usize], parts: Vec<MeshPart>) -> Option<Self> {
        let Self::Imported(source) = self else {
            return None;
        };
        let geometry = vertices
            .iter()
            .map(|&index| source.vertices.get(index).copied())
            .collect::<Option<_>>()?;
        let tangents = if source.tangents.is_empty() {
            Vec::new()
        } else {
            vertices
                .iter()
                .map(|&index| source.tangents.get(index).copied())
                .collect::<Option<_>>()?
        };
        let mut mesh = Self::imported_shared(geometry, tangents, parts, source.materials.clone())
            .ok()?
            .with_material_slots(source.material_slots.clone())?;
        if !source.binormals.is_empty() {
            let reordered = vertices
                .iter()
                .map(|&index| source.binormals.get(index).copied())
                .collect::<Option<_>>()?;
            mesh = mesh.with_particle_basis(reordered, source)?;
        }
        if source.authored_tangents {
            let Self::Imported(data) = &mut mesh else {
                return None;
            };
            Arc::get_mut(&mut Arc::get_mut(data)?.geometry)?.authored_tangents = true;
        }
        if !source.lightmap_uv.is_empty() {
            let uv = vertices
                .iter()
                .map(|&index| source.lightmap_uv[index])
                .collect();
            mesh = mesh.attach_lightmap_uv(uv, source.lightmap_min_size)?;
        }
        if let Some(skin) = &source.skin {
            let weights = vertices
                .iter()
                .map(|&index| skin.weights.get(index).copied())
                .collect::<Option<_>>()?;
            mesh = mesh.with_skin_weights(weights)?;
        }
        if let Some(morph) = &source.morph {
            mesh = mesh.with_morph_table(
                morph.target_count(),
                morph.target_names().clone(),
                morph.subset(vertices)?,
            )?;
        }
        Some(mesh)
    }
}
