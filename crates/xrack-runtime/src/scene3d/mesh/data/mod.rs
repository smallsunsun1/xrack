//! Mesh geometry and shared vertex storage.

mod deformation;
mod generated;
mod import;
mod materials;
mod topology;

pub(in crate::scene3d) use generated::Primitive;
pub use import::{MAX_IMPORTED_VERTICES, MeshAsset3D, MeshImportOptions, mesh_resource_key};
pub use materials::CaptureMaterial;

use crate::scene3d::{
    ModelBlendPose,
    pick::{MIN_BVH_TRIANGLES, PickTree},
};
use std::sync::{Arc, OnceLock};

use super::{
    indices::MeshDrawIndices,
    material::{MeshMaterial, MeshMaterialSlot, MeshPart, SceneAppearance},
    morph::MorphGeometry,
    nodes::MeshNodeCatalog,
    skin::SkinGeometry,
};

/// Signed normalized tangent XYZ and orientation W. Zero W disables normal mapping.
pub type MeshTangent = [i16; 4];

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

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshVertex {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub uv: [f32; 2],
    pub uv1: [f32; 2],
    pub color: [f32; 4],
}

impl MeshVertex {
    pub const STRIDE: usize = 56;
}

#[doc(hidden)]
#[derive(Debug)]
pub struct MeshVertexStreams {
    vertices: Vec<MeshVertex>,
    tangents: Vec<MeshTangent>,
    binormals: Vec<MeshTangent>,
    lightmap_uv: Vec<[f32; 2]>,
    draw_indices: Option<Box<OnceLock<Option<MeshDrawIndices>>>>,
}

impl MeshVertexStreams {
    fn invalidate_indices(&mut self) {
        if let Some(indices) = &mut self.draw_indices {
            indices.take();
        }
    }
}

#[derive(Clone, Debug)]
#[doc(hidden)]
pub struct MeshGeometry {
    streams: Arc<MeshVertexStreams>,
    parts: Vec<MeshPart>,
    part_centers: Vec<[f32; 3]>,
    lightmap_min_size: u32,
    min: [f32; 3],
    max: [f32; 3],
    skin: Option<Arc<SkinGeometry>>,
    morph: Option<Arc<MorphGeometry>>,
    heightfield: Option<Arc<MeshHeightField>>,
    node_catalog: Option<Arc<MeshNodeCatalog>>,
    pick_tree: OnceLock<Arc<PickTree>>,
    primitive: Option<Primitive>,
    // Authored tangents use the untransformed primary UV set.
    authored_tangents: bool,
}

/// The normalized terrain samples in the original PNG's row order.
#[doc(hidden)]
#[derive(Debug)]
pub struct MeshHeightField {
    dimensions: [usize; 2],
    samples: Box<[f32]>,
}

impl MeshHeightField {
    pub fn dimensions(&self) -> [usize; 2] {
        self.dimensions
    }

    pub fn samples(&self) -> &[f32] {
        &self.samples
    }
}

impl MeshGeometry {
    pub(in crate::scene3d) fn pick_tree(&self) -> &PickTree {
        self.pick_tree
            .get_or_init(|| Arc::new(PickTree::new(&self.vertices, &self.parts)))
    }
}

impl std::ops::Deref for MeshGeometry {
    type Target = MeshVertexStreams;

    fn deref(&self) -> &Self::Target {
        &self.streams
    }
}

#[derive(Clone, Debug)]
pub struct MeshData {
    geometry: Arc<MeshGeometry>,
    model_blend: Option<Arc<ModelBlendPose>>,
    pub(in crate::scene3d) material_slots: Arc<[MeshMaterialSlot]>,
    slot_selectors: OnceLock<Arc<[String]>>,
    pub(in crate::scene3d) materials: Arc<[MeshMaterial]>,
    source_materials: Option<Arc<[MeshMaterial]>>,
    uses_base_items: bool,
    item_sources: Box<[String]>,
    capture_sources: Box<[crate::scene3d::effect::CaptureRead]>,
    capture_positions: bool,
    shader_displacement: f32,
    blended_parts: Vec<(usize, [f32; 3])>,
    transmission_blur: Option<f32>,
    has_opaque_parts: bool,
    pub(in crate::scene3d) instance_blended: bool,
    has_auxiliary_depth: bool,
    has_opaque_depth_prepass: bool,
    pub(in crate::scene3d) has_face_culling: bool,
    depth_prepasses: [bool; 2],
    blended_depth: bool,
    has_depth_writing_transmission: bool,
    has_volume: bool,
    pub(in crate::scene3d) authored: Option<Arc<SceneAppearance>>,
    derivative_normals: Vec<bool>,
}

impl std::ops::Deref for MeshData {
    type Target = MeshGeometry;

    fn deref(&self) -> &Self::Target {
        &self.geometry
    }
}

impl MeshData {
    #[doc(hidden)]
    pub fn geometry(&self) -> &Arc<MeshGeometry> {
        &self.geometry
    }

    /// Identity for GPU vertex attributes. Draw ranges and materials are cached separately.
    #[doc(hidden)]
    pub fn vertex_streams(&self) -> &Arc<MeshVertexStreams> {
        &self.geometry.streams
    }
}

impl Mesh3D {
    pub fn parts(&self) -> &[MeshPart] {
        match self {
            Self::Imported(mesh) => &mesh.parts,
            _ => &[],
        }
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
            Self::Imported(mesh) if mesh.vertices.len() / 3 >= MIN_BVH_TRIANGLES => {
                Some(mesh.pick_tree())
            }
            Self::Sphere => {
                static TREE: OnceLock<PickTree> = OnceLock::new();
                Some(TREE.get_or_init(|| {
                    let vertices = self.vertices();
                    PickTree::new(
                        vertices,
                        &[MeshPart {
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
