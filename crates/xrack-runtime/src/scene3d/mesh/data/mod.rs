//! Mesh geometry and shared vertex storage.

mod deformation;
mod generated;
mod import;
mod materials;
mod shape;
mod topology;

pub(in crate::scene3d) use generated::Primitive;
pub use import::{MAX_IMPORTED_VERTICES, MeshAsset3D, MeshImportOptions, mesh_resource_key};
pub use materials::CaptureMaterial;
pub use shape::Mesh3D;

use crate::scene3d::{ModelBlendPose, pick::PickTree, screen_pick::RasterTree};
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
    raster_tree: OnceLock<Arc<RasterTree>>,
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
    raster_size: f32,
    wide_points: Option<Arc<[std::ops::Range<usize>]>>,
    wide_lines: Option<Arc<[std::ops::Range<usize>]>>,
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
