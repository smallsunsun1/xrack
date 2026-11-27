//! Mesh geometry and shared vertex storage.

mod deformation;
mod import;
mod materials;

pub use import::{MAX_IMPORTED_VERTICES, MeshAsset3D, MeshImportOptions, mesh_resource_key};
pub use materials::CaptureMaterial;

use crate::scene3d::{
    ModelBlendPose,
    pick::{MIN_BVH_TRIANGLES, PickTree},
};
use nalgebra::Vector3;
use std::{
    f32::consts::{PI, TAU},
    sync::{Arc, OnceLock},
};

use super::{
    indices::MeshDrawIndices,
    material::{
        MeshAlphaMode, MeshCullMode, MeshMaterial, MeshMaterialSlot, MeshPart, MeshPbr,
        SceneAppearance,
    },
    morph::MorphGeometry,
    nodes::MeshNodeCatalog,
    primitives::{self, PrimitiveParts},
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

    fn solid(position: [f32; 3], normal: [f32; 3]) -> Self {
        Self {
            position,
            normal,
            uv: [0.0; 2],
            uv1: [0.0; 2],
            color: [1.0; 4],
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(in crate::scene3d) enum Primitive {
    Box,
    Sphere,
    Rectangle,
    Cylinder,
    Cone,
    Capsule,
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
    pub fn resolve(key: &str, imported: impl FnOnce(&str) -> Option<Self>) -> Option<Self> {
        Self::builtin(key).or_else(|| imported(key))
    }

    pub(crate) fn builtin(key: &str) -> Option<Self> {
        static RECTANGLE: OnceLock<Option<Mesh3D>> = OnceLock::new();
        static CYLINDER: OnceLock<Option<Mesh3D>> = OnceLock::new();
        static CONE: OnceLock<Option<Mesh3D>> = OnceLock::new();

        match key {
            "primitive:box" => Some(Self::Box),
            "primitive:sphere" => Some(Self::Sphere),
            "primitive:rectangle" => RECTANGLE
                .get_or_init(|| {
                    Self::prepare_primitive(Primitive::Rectangle, primitives::rectangle())
                })
                .clone(),
            "primitive:cylinder" => CYLINDER
                .get_or_init(|| {
                    Self::prepare_primitive(Primitive::Cylinder, primitives::cylinder())
                })
                .clone(),
            "primitive:cone" => CONE
                .get_or_init(|| Self::prepare_primitive(Primitive::Cone, primitives::cone()))
                .clone(),
            _ => None,
        }
    }

    fn prepare_primitive(kind: Primitive, parts: PrimitiveParts) -> Option<Self> {
        Self::prepare_parts(Some(kind), parts, Vec::new())
    }

    pub(crate) fn prepare_capsule(parts: PrimitiveParts) -> Option<Self> {
        Self::prepare_primitive(Primitive::Capsule, parts)
    }

    pub(crate) fn prepare_procedural(parts: PrimitiveParts) -> Option<Self> {
        Self::prepare_parts(None, parts, Vec::new())
    }

    pub(crate) fn prepare_authored_triangles(
        vertices: Vec<MeshVertex>,
        tangents: Vec<MeshTangent>,
    ) -> Option<Self> {
        Self::prepare_parts(None, vec![("surface", vertices)], tangents)
    }

    fn prepare_parts(
        kind: Option<Primitive>,
        parts: PrimitiveParts,
        tangents: Vec<MeshTangent>,
    ) -> Option<Self> {
        let mut vertices = Vec::new();
        let mut ranges = Vec::new();
        let mut slots = Vec::new();
        let mut materials = Vec::new();
        for (index, (name, part)) in parts.into_iter().enumerate() {
            let start = vertices.len();
            vertices.extend(part);
            ranges.push(MeshPart {
                vertices: start..vertices.len(),
                material: index,
                volume_inverse: None,
            });
            slots.push(MeshMaterialSlot {
                source_index: Some(index as u32),
                name: Some(name.into()),
            });
            materials.push(MeshMaterial {
                shader: None,
                channels: Default::default(),
                vertex_colors: true,
                vertex_masks: Default::default(),
                cull_mode: MeshCullMode::None,
                depth_draw_mode: Default::default(),
                blend_mode: Default::default(),
                base_color: [1.0; 4],
                base_texture: None,
                base_item: None,
                alpha_mode: MeshAlphaMode::Opaque,
                pbr: Some(MeshPbr::principled()),
            });
        }
        let Self::Imported(mut mesh) = Self::imported(vertices, tangents, ranges, materials)
            .ok()?
            .with_material_slots(slots.into())?
        else {
            return None;
        };
        let geometry = Arc::get_mut(&mut Arc::get_mut(&mut mesh)?.geometry)?;
        geometry.primitive = kind;
        geometry.authored_tangents = !geometry.tangents.is_empty();
        Some(Self::Imported(mesh))
    }

    pub(crate) fn with_procedural_layout(
        &self,
        layout: &[crate::scene3d::mesh::authored::SceneMeshPart],
    ) -> Option<Self> {
        let Self::Imported(base) = self else {
            return None;
        };
        let material = base.materials.first()?;
        let mut part_centers = Vec::with_capacity(layout.len());
        let mut parts = Vec::with_capacity(layout.len());
        let mut slots = Vec::new();
        let mut indices = std::collections::BTreeMap::<&str, usize>::new();
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

    pub(in crate::scene3d) fn primitive(&self) -> Option<Primitive> {
        match self {
            Self::Box => Some(Primitive::Box),
            Self::Sphere => Some(Primitive::Sphere),
            Self::Imported(mesh) if mesh.model_blend.is_none() => mesh.primitive,
            Self::Imported(_) => None,
        }
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

    /// Triangles in model space, shared by rasterization and ray queries.
    pub fn vertices(&self) -> &[MeshVertex] {
        static BOX: OnceLock<Vec<MeshVertex>> = OnceLock::new();
        static SPHERE: OnceLock<Vec<MeshVertex>> = OnceLock::new();
        match self {
            Self::Box => BOX.get_or_init(box_vertices),
            Self::Sphere => SPHERE.get_or_init(sphere_vertices),
            Self::Imported(mesh) => &mesh.vertices,
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

fn box_vertices() -> Vec<MeshVertex> {
    let corners = [
        [-0.5, -0.5, -0.5],
        [0.5, -0.5, -0.5],
        [0.5, 0.5, -0.5],
        [-0.5, 0.5, -0.5],
        [-0.5, -0.5, 0.5],
        [0.5, -0.5, 0.5],
        [0.5, 0.5, 0.5],
        [-0.5, 0.5, 0.5],
    ];
    let faces = [
        ([3, 2, 1, 0], [0.0, 0.0, -1.0]),
        ([4, 5, 6, 7], [0.0, 0.0, 1.0]),
        ([4, 7, 3, 0], [-1.0, 0.0, 0.0]),
        ([1, 2, 6, 5], [1.0, 0.0, 0.0]),
        ([0, 1, 5, 4], [0.0, -1.0, 0.0]),
        ([3, 7, 6, 2], [0.0, 1.0, 0.0]),
    ];
    faces
        .into_iter()
        .flat_map(|(indices, normal)| {
            let up = match normal[1] {
                1.0 => -Vector3::z(),
                -1.0 => Vector3::z(),
                _ => Vector3::y(),
            };
            let right = up.cross(&Vector3::from(normal));
            [0, 1, 2, 0, 2, 3].map(|index| {
                let position = corners[indices[index]];
                let p = Vector3::from(position);
                MeshVertex {
                    uv: [0.5 + p.dot(&right), 0.5 - p.dot(&up)],
                    ..MeshVertex::solid(position, normal)
                }
            })
        })
        .collect()
}

fn sphere_vertices() -> Vec<MeshVertex> {
    const SLICES: usize = 24;
    const STACKS: usize = 12;
    let point = |row: usize, column: usize| {
        let polar = PI * row as f32 / STACKS as f32;
        let azimuth = TAU * (column % SLICES) as f32 / SLICES as f32;
        let (radius, y) = polar.sin_cos();
        let position = match row {
            0 => [0.0, 1.0, 0.0],
            STACKS => [0.0, -1.0, 0.0],
            _ => [radius * azimuth.cos(), y, radius * azimuth.sin()],
        };
        MeshVertex {
            uv: [column as f32 / SLICES as f32, row as f32 / STACKS as f32],
            ..MeshVertex::solid(position, position)
        }
    };
    let mut vertices = Vec::with_capacity(SLICES * (STACKS - 1) * 6);
    for row in 0..STACKS {
        for column in 0..SLICES {
            let a = point(row, column);
            let b = point(row, column + 1);
            let c = point(row + 1, column);
            let d = point(row + 1, column + 1);
            if row > 0 {
                vertices.extend([a, b, c]);
            }
            if row + 1 < STACKS {
                vertices.extend([b, d, c]);
            }
        }
    }
    vertices
}
