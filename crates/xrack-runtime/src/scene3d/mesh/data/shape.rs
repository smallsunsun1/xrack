//! Mesh variants and their shared draw and spatial queries.

use super::{MeshData, MeshTangent};
use crate::scene3d::{
    mesh::{
        indices::MeshDrawIndices,
        material::{MeshPart, MeshTopology},
        nodes::MeshNodeCatalog,
    },
    pick::{MIN_BVH_TRIANGLES, PickTree},
};
use std::sync::{Arc, OnceLock};

#[derive(Clone, Debug)]
pub enum Mesh3D {
    Box,
    Sphere,
    Imported(Arc<MeshData>),
}

impl PartialEq for Mesh3D {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Box, Self::Box) | (Self::Sphere, Self::Sphere) => true,
            (Self::Imported(left), Self::Imported(right)) => {
                Arc::ptr_eq(left, right)
                    || Arc::ptr_eq(&left.geometry, &right.geometry)
                        && left.model_blend == right.model_blend
                        && left.instance_blended == right.instance_blended
                        && left.authored == right.authored
                        && (left.model_blend.is_some()
                            && Arc::ptr_eq(&left.materials, &right.materials)
                            && left.derivative_normals == right.derivative_normals
                            || left
                                .source_materials
                                .as_ref()
                                .zip(right.source_materials.as_ref())
                                .is_some_and(|(left, right)| Arc::ptr_eq(left, right)))
            }
            _ => false,
        }
    }
}

impl Mesh3D {
    pub fn parts(&self) -> &[MeshPart] {
        match self {
            Self::Imported(mesh) => &mesh.parts,
            _ => &[],
        }
    }

    pub(in crate::scene3d) fn raster_size(&self) -> f32 {
        match self {
            Self::Imported(mesh) => mesh.raster_size,
            _ => 0.0,
        }
    }

    pub(in crate::scene3d) fn raster_tree(
        &self,
    ) -> Option<&crate::scene3d::screen_pick::RasterTree> {
        let Self::Imported(mesh) = self else {
            return None;
        };
        if mesh.raster_size == 0.0 || mesh.vertices.len() < MIN_BVH_TRIANGLES {
            return None;
        }
        Some(mesh.raster_tree.get_or_init(|| {
            Arc::new(crate::scene3d::screen_pick::RasterTree::new(
                &mesh.vertices,
                &mesh.parts,
            ))
        }))
    }

    pub(in crate::scene3d) fn has_triangle_surface(&self) -> bool {
        !matches!(self, Self::Imported(mesh)
            if !mesh.parts.iter().any(|part| part.topology == MeshTopology::Triangles))
    }

    pub(in crate::scene3d) fn triangle_at(&self, vertex: usize) -> &[super::MeshVertex; 3] {
        &self.vertices()[vertex..].as_chunks::<3>().0[0]
    }

    /// Prepare reusable draw indices without changing the triangle data used for picking.
    #[doc(hidden)]
    pub fn draw_indices(&self) -> Option<&MeshDrawIndices> {
        if self.model_blend().is_some() {
            return None;
        }
        static SPHERE: OnceLock<Option<MeshDrawIndices>> = OnceLock::new();
        let slot = match self {
            Self::Sphere => &SPHERE,
            Self::Imported(mesh) => mesh.streams.draw_indices.as_deref()?,
            Self::Box => return None,
        };
        slot.get_or_init(|| {
            MeshDrawIndices::prepare(
                self.vertices(),
                self.tangents(),
                self.binormals(),
                self.lightmap_uvs(),
                self.influences(),
                self.morph_geometry(),
            )
        })
        .as_ref()
    }

    pub fn tangents(&self) -> &[[i16; 4]] {
        match self {
            Self::Imported(mesh) => &mesh.tangents,
            _ => &[],
        }
    }

    /// Independent bitangents for baked geometry with an authored tangent frame.
    #[doc(hidden)]
    pub fn binormals(&self) -> &[MeshTangent] {
        match self {
            Self::Imported(mesh) => &mesh.binormals,
            _ => &[],
        }
    }

    pub(in crate::scene3d) fn pick_tree(&self) -> Option<&PickTree> {
        if self.model_blend().is_some() {
            return None;
        }
        match self {
            Self::Imported(mesh)
                if mesh.vertices.len() / 3 >= MIN_BVH_TRIANGLES && self.has_triangle_surface() =>
            {
                Some(mesh.pick_tree())
            }
            Self::Sphere => {
                static TREE: OnceLock<PickTree> = OnceLock::new();
                Some(TREE.get_or_init(|| {
                    let vertices = self.vertices();
                    PickTree::new(
                        vertices,
                        &[MeshPart {
                            lighting: Default::default(),
                            topology: MeshTopology::Triangles,
                            vertices: 0..vertices.len(),
                            material: 0,
                            volume_inverse: None,
                        }],
                    )
                }))
            }
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn with_node_catalog(mut self, catalog: Arc<MeshNodeCatalog>) -> Self {
        if let Self::Imported(data) = &mut self {
            Arc::make_mut(&mut Arc::make_mut(data).geometry).node_catalog = Some(catalog);
        }
        self
    }

    #[doc(hidden)]
    pub(crate) fn shared_node_catalog(&self) -> Option<Arc<MeshNodeCatalog>> {
        match self {
            Self::Imported(data) => data.node_catalog.clone(),
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn node_catalog(&self) -> Option<&MeshNodeCatalog> {
        match self {
            Self::Imported(data) => data.node_catalog.as_deref(),
            _ => None,
        }
    }

    pub(crate) fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        match self {
            Self::Box => ([-0.5; 3], [0.5; 3]),
            Self::Sphere => ([-1.0; 3], [1.0; 3]),
            Self::Imported(mesh) => {
                let rest = (mesh.min, mesh.max);
                mesh.model_blend
                    .as_deref()
                    .map_or(rest, |pose| pose.bounds(rest))
            }
        }
    }
}
