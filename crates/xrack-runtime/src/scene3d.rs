//! Scene geometry, camera projection, and ray queries shared by 3D hosts.
pub use crate::procedural_sky::SkySettings;
pub use crate::scene_animation::{
    AnimatedMesh, MAX_ANIMATION_KEYS, MAX_ANIMATION_NODES, MAX_MESH_INSTANCES, MeshAnimationBlend,
    MeshAnimationClip, MeshAnimationNode, MeshAnimationProperty, MeshAnimationTrack, MeshAsset,
    MeshCamera, MeshCameraProjection, MeshInstances, MeshInterpolation, MeshLight, MeshLods,
    MeshNodeTransform, MeshPlayback, MorphAnimationTrack,
};
pub use crate::scene_camera::{
    CAMERA_MATRIX_FIELDS, Point3D, ProjectedPoint3D, SceneCamera, SceneCameraSettings,
    hit_all_camera, hit_all_objects, hit_camera, hit_instance, hit_instance_camera, hit_model,
    hit_objects, hit_row, hit_row_camera, hit_subset, hit_subset_camera, pick_camera, pick_in_rect,
    pick_in_rect_camera, pick_model, pick_objects, project, project_camera, unproject,
    unproject_camera,
};
use crate::scene_cast::{MeshContact, ShapeSweep, TriangleContact, cast_shape_triangle};
pub use crate::scene_cast::{
    WalkOptions, WalkResult3D, capsule_cast, capsule_overlap, capsule_slide,
    capsule_trigger_crossings, capsule_trigger_overlap, capsule_walk, capsule_walk_result,
    capsule_walk_with_contacts, sphere_cast, sphere_overlap, sphere_slide, sphere_walk,
};
pub use crate::scene_environment::{
    EnvironmentOrientation, EnvironmentProbe, EnvironmentSource, ReflectionCapture,
    ReflectionProbe, ReflectionRefresh, ReflectionSlicing, ReflectionSource, ViewportEnvironment,
    environment_resource_key, model_reflections, record_reflections,
};
pub use crate::scene_fog::{
    FogNoise, FogVolume3D, FogVolumeDefinition, MAX_FOG_VOLUMES, model_fog_volumes,
    record_fog_volumes, visible_fog_volumes,
};
use crate::scene_hierarchy::{NO_MESH_NODE, SceneTransform};
pub use crate::scene_hierarchy::{
    SceneCollisionShape, SceneGeometry, SceneGeometryInput, SceneModel, SceneNodeDefinition,
    ScenePreloadCursor, ScenePreloadStep, model_cameras, model_nodes, prepare_collision_shapes,
    record_cameras, record_objects,
};
use crate::scene_indices::MIN_INDEXED_VERTICES;
pub use crate::scene_indices::{MeshDrawIndices, MeshIndexElements};
pub use crate::scene_instance_table::{InstanceTable3D, InstanceTableRow};
pub use crate::scene_lighting::{
    DirectionalShadowSpace, LightBakeMode, MAX_SCENE_LIGHTS, PointShadowProjection, SceneLight,
    SceneLightDefinition, SceneLightModel, SceneLightScope, SceneLightSource, ShadowFilter,
    ShadowLight, ShadowMapPrecision, ShadowProjectionSettings, ShadowRadius, ShadowSampling,
    linear_color, model_lights, record_lights, shadow_clip_corners,
};
pub use crate::scene_material::{
    MAX_MESH_TEXTURE_DIMENSION, MaterialChannel, MaterialChannels, MeshAlphaMode, MeshAnisotropy,
    MeshBlendMode, MeshClearcoat, MeshCullMode, MeshDepthCoverage, MeshDepthDraw, MeshDielectric,
    MeshFresnel, MeshImage, MeshMaterial, MeshMaterialSlot, MeshNormalMap, MeshPart, MeshPbr,
    MeshShader, MeshTexture, MeshTransmission, MeshVolume, MeshWorkflow, RefractionChannels,
    SceneItemTexture, SceneMaterial, SceneMaterialBinding, SceneMaterialMaps, SceneTexture,
    TextureChannel, TextureColorSpace, TextureFilter, TextureSampling, TextureTransform,
    TextureWrap, VertexColorMasks, texture_resource_key,
};
use crate::scene_morph::MorphPose;
pub use crate::scene_morph::{
    MAX_MORPH_DELTAS, MAX_MORPH_TARGETS, MorphDelta, MorphGeometry, MorphWeights,
};
pub use crate::scene_particle_affectors::{
    MAX_PARTICLE_AFFECTORS, ParticleAffector, ParticleAffectorDefinition, model_particle_affectors,
    record_particle_affectors,
};
pub use crate::scene_particle_mesh::ParticleMeshSurface;
pub use crate::scene_particle_motion::ParticleMotion;
pub use crate::scene_particles::{
    MAX_PARTICLE_EMITTERS, MAX_PARTICLE_TEXTURE_BYTES, MAX_PARTICLES, ParticleBlend,
    ParticleEmitter, ParticleEmitterDefinition, ParticleSample, ParticleSprite, model_particles,
    record_particles,
};
use crate::scene_pick::{BoundsSegment, MIN_BVH_TRIANGLES, PickTree};
pub use crate::scene_projection::SceneProjectionCache;
pub use crate::scene_random_instances::{InstanceColorModel, SceneRandomInstances};
pub use crate::scene_skin::{
    JointPalette, JointRows, MAX_SKIN_JOINTS, MeshInfluence, MeshSkin, MeshSkinJoint,
    SKIN_INFLUENCES,
};
use crate::scene_skin::{SkinGeometry, SkinPose};
use crate::scene_surface::closest_triangle_point;
pub use crate::scene_surface::{SurfaceSample, corresponding_vertices};
use crate::scene_visibility::Frustum;
pub use crate::scene_visibility::{SceneVisibility, VisibilityIndex};
use crate::{scene_effect::CaptureRenderables, scene_material::SceneAppearance};

/// The default content layer for an authored model, light or camera.
pub use crate::scene_lighting::viewport_lighting;
pub use crate::scene_lightmap::{
    BakeRole, BakedLightmap, LightmapMode, LightmapResolution, LightmapTrace,
};

pub const MAIN_LAYER: u64 = 1;
use crate::{Color, VecModel, scene_bounds::Bounds3D};
use nalgebra::{Matrix3, Vector3};
use std::{
    collections::VecDeque,
    f32::consts::{PI, TAU},
    num::NonZeroU32,
    ops::Range,
    sync::{Arc, Mutex, OnceLock, Weak},
};

pub const MAX_IMPORTED_VERTICES: usize = 900_000;
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
                        && left.instance_blended == right.instance_blended
                        && left.authored == right.authored
                        && left
                            .source_materials
                            .as_ref()
                            .zip(right.source_materials.as_ref())
                            .is_some_and(|(left, right)| Arc::ptr_eq(left, right))
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
enum Primitive {
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
    pick_tree: OnceLock<Arc<PickTree>>,
    primitive: Option<Primitive>,
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
    fn pick_tree(&self) -> &PickTree {
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
    material_slots: Arc<[MeshMaterialSlot]>,
    materials: Arc<[MeshMaterial]>,
    source_materials: Option<Arc<[MeshMaterial]>>,
    uses_base_items: bool,
    item_sources: Box<[String]>,
    capture_sources: Box<[crate::scene_effect::CaptureRead]>,
    capture_positions: bool,
    shader_displacement: f32,
    blended_parts: Vec<(usize, [f32; 3])>,
    transmission_blur: Option<f32>,
    has_opaque_parts: bool,
    instance_blended: bool,
    has_auxiliary_depth: bool,
    has_opaque_depth_prepass: bool,
    has_face_culling: bool,
    depth_prepasses: [bool; 2],
    blended_depth: bool,
    has_depth_writing_transmission: bool,
    has_volume: bool,
    authored: Option<Arc<SceneAppearance>>,
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

    #[doc(hidden)]
    pub fn instance_blend_primitive(&self) -> Option<Mesh3D> {
        if !matches!(&*self.materials,
            [material] if matches!(material.alpha_mode, MeshAlphaMode::InstanceBlend { builtin: true, .. })
        ) {
            return None;
        }
        match self.geometry.primitive? {
            Primitive::Box => Some(Mesh3D::Box),
            Primitive::Sphere => Some(Mesh3D::Sphere),
            _ => None,
        }
    }

    #[doc(hidden)]
    pub fn uses_base_items(&self) -> bool {
        self.uses_base_items
    }

    #[doc(hidden)]
    pub fn uses_item_textures(&self) -> bool {
        !self.item_sources.is_empty()
    }

    #[doc(hidden)]
    pub fn item_sources(&self) -> &[String] {
        &self.item_sources
    }

    #[doc(hidden)]
    pub fn capture_sources(&self) -> &[crate::scene_effect::CaptureRead] {
        &self.capture_sources
    }

    #[doc(hidden)]
    pub fn uses_capture_positions(&self) -> bool {
        self.capture_positions
    }

    fn with_materials(
        geometry: Arc<MeshGeometry>,
        materials: Arc<[MeshMaterial]>,
        authored: Option<Arc<SceneAppearance>>,
    ) -> Self {
        let mut blended_parts = Vec::new();
        let mut transmission_blur = None::<f32>;
        let mut has_opaque_parts = geometry.parts.is_empty();
        let mut has_auxiliary_depth = geometry.parts.is_empty();
        let mut has_opaque_depth_prepass = geometry.parts.is_empty();
        let mut has_face_culling = false;
        let mut depth_prepasses = [false; 2];
        let mut blended_depth = false;
        let mut has_depth_writing_transmission = false;
        let mut has_volume = false;
        for (index, part) in geometry.parts.iter().enumerate() {
            let material = &materials[part.material];
            let blended = material.is_blended();
            let writes_depth = material.depth_coverage().is_some();
            has_face_culling |= material.cull_mode != MeshCullMode::None;
            has_auxiliary_depth |= writes_depth;
            blended_depth |= writes_depth && blended;
            let transmission = material.transmission_blur();
            has_opaque_depth_prepass |= material
                .depth_draw_mode
                .admits_opaque_prepass(blended, transmission.is_some());
            if material.depth_draw_mode == MeshDepthDraw::OpaquePrepass {
                depth_prepasses[usize::from(transmission.is_some())] = true;
            }
            if blended {
                blended_parts.push((index, geometry.part_centers[index]));
            }
            if let Some(blur) = transmission {
                transmission_blur = Some(transmission_blur.map_or(blur, |before| before.max(blur)));
                has_depth_writing_transmission |= !blended;
                has_volume |= material.pbr.as_ref().and_then(MeshPbr::volume).is_some();
            } else if !blended {
                has_opaque_parts = true;
            }
        }
        Self {
            geometry,
            material_slots: Arc::from([]),
            uses_base_items: materials
                .iter()
                .any(|material| material.base_item.is_some()),
            item_sources: materials
                .iter()
                .flat_map(|material| {
                    material
                        .base_item
                        .as_deref()
                        .map(SceneItemTexture::source)
                        .into_iter()
                        .chain(material.shader.iter().flat_map(MeshShader::item_sources))
                })
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .map(str::to_owned)
                .collect(),
            capture_positions: materials
                .iter()
                .filter_map(|material| material.shader.as_ref())
                .any(|shader| {
                    crate::material_shader::MaterialHooks::from_source(&shader.source)
                        .capture_position
                        && shader
                            .capture_sources(crate::material_shader::MAX_TEXTURES)
                            .any(|read| {
                                read.attachment == crate::scene_effect::CaptureAttachment::Depth
                            })
                }),
            capture_sources: materials
                .iter()
                .filter_map(|material| material.shader.as_ref())
                .flat_map(|shader| shader.capture_sources(crate::material_shader::MAX_TEXTURES))
                .collect::<std::collections::BTreeSet<_>>()
                .into_iter()
                .cloned()
                .collect(),
            shader_displacement: materials
                .iter()
                .map(|material| material.vertex_parameters()[3])
                .fold(0.0, f32::max),
            instance_blended: materials
                .iter()
                .any(|material| matches!(material.alpha_mode, MeshAlphaMode::InstanceBlend { .. })),
            materials,
            source_materials: None,
            blended_parts,
            transmission_blur,
            has_opaque_parts,
            has_auxiliary_depth,
            has_opaque_depth_prepass,
            has_face_culling,
            depth_prepasses,
            blended_depth,
            has_depth_writing_transmission,
            has_volume,
            authored,
            derivative_normals: Vec::new(),
        }
    }
}

fn default_mesh_material() -> MeshMaterial {
    MeshMaterial {
        shader: None,
        channels: Default::default(),
        vertex_colors: true,
        vertex_masks: Default::default(),
        cull_mode: MeshCullMode::Back,
        depth_draw_mode: Default::default(),
        blend_mode: Default::default(),
        base_color: [1.0; 4],
        base_texture: None,
        base_item: None,
        alpha_mode: MeshAlphaMode::Opaque,
        pbr: Some(MeshPbr::principled()),
    }
}

#[derive(Debug)]
#[doc(hidden)]
pub struct CaptureMaterial {
    style: SceneMaterial,
    replacement: Arc<[MeshMaterial]>,
    meshes: Mutex<VecDeque<CaptureMesh>>,
}

#[derive(Debug)]
struct CaptureMesh {
    source: Weak<MeshData>,
    filter: CaptureRenderables,
    mesh: Option<Mesh3D>,
}

impl PartialEq for CaptureMaterial {
    fn eq(&self, other: &Self) -> bool {
        self.style == other.style
    }
}

impl CaptureMaterial {
    pub(crate) fn new(style: SceneMaterial) -> Arc<Self> {
        let replacement = Arc::from([style.prepare(&default_mesh_material())]);
        Arc::new(Self {
            style,
            replacement,
            meshes: Mutex::default(),
        })
    }

    pub fn material(&self) -> &MeshMaterial {
        &self.replacement[0]
    }

    pub fn project(&self, mesh: &Mesh3D, filter: CaptureRenderables) -> Option<Mesh3D> {
        let source = mesh.material_source()?;
        let mut meshes = self
            .meshes
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = meshes.iter().position(|entry| {
            entry.source.as_ptr() == Arc::as_ptr(source) && entry.filter == filter
        }) && let Some(entry) = meshes.remove(index)
        {
            let mesh = entry.mesh.clone();
            meshes.push_front(entry);
            return mesh;
        }
        let mut geometry = source.geometry.as_ref().clone();
        let sections = geometry
            .parts
            .drain(..)
            .zip(geometry.part_centers.drain(..))
            .filter(|(part, _)| filter.includes(source.materials[part.material].is_blended()))
            .map(|(mut part, center)| {
                part.material = 0;
                (part, center)
            });
        let (parts, centers): (Vec<_>, Vec<_>) = sections.unzip();
        let mesh = (!parts.is_empty()).then(|| {
            geometry.parts = parts;
            geometry.part_centers = centers;
            geometry.pick_tree = OnceLock::new();
            let mut data =
                MeshData::with_materials(Arc::new(geometry), self.replacement.clone(), None);
            data.derivative_normals = vec![self.style.overrides_normal_texture()];
            Mesh3D::Imported(Arc::new(data))
        });
        meshes.retain(|entry| entry.source.strong_count() > 0);
        meshes.truncate(63);
        meshes.push_front(CaptureMesh {
            source: Arc::downgrade(source),
            filter,
            mesh: mesh.clone(),
        });
        mesh
    }
}

/// Import-time preparation for a mesh file. Both options default to false.
#[derive(Clone, Copy, Debug, Default)]
pub struct MeshImportOptions {
    pub automatic_lod: bool,
    pub lightmap_uv: bool,
}

impl MeshImportOptions {
    pub fn from_operation(name: &str) -> Option<Self> {
        let [automatic_lod, lightmap_uv] = match name {
            "xr.mesh_url" => [false, false],
            "xr.mesh_lod_url" => [true, false],
            "xr.mesh_lightmap_url" => [false, true],
            "xr.mesh_lod_lightmap_url" => [true, true],
            _ => return None,
        };
        Some(Self {
            automatic_lod,
            lightmap_uv,
        })
    }
}

/// An immutable mesh supplied by a host at runtime. Clones share the prepared
/// geometry and textures. The default is empty.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshAsset3D(Option<MeshAsset>);

impl MeshAsset3D {
    #[doc(hidden)]
    pub fn new(asset: MeshAsset) -> Self {
        Self(Some(asset))
    }

    pub fn is_loaded(&self) -> bool {
        self.0.is_some()
    }

    pub(crate) fn asset(&self) -> Option<&MeshAsset> {
        self.0.as_ref()
    }
}

pub fn mesh_resource_key(path: &str, options: MeshImportOptions) -> String {
    let prefix = match (options.automatic_lod, options.lightmap_uv) {
        (false, false) => "asset",
        (true, false) => "asset-lod",
        (false, true) => "asset-lightmap",
        (true, true) => "asset-lod-lightmap",
    };
    format!("{prefix}:{path}")
}

pub use crate::scene_authored_mesh::{
    SceneMeshIndices, SceneMeshInputs, SceneMeshSubset, SceneMeshVertex,
};
pub use crate::scene_parametric::SceneParametricGeometry;

impl Mesh3D {
    pub fn resolve(key: &str, imported: impl FnOnce(&str) -> Option<Self>) -> Option<Self> {
        Self::builtin(key).or_else(|| imported(key))
    }

    pub(crate) fn builtin(key: &str) -> Option<Self> {
        use crate::scene_primitives;
        static RECTANGLE: OnceLock<Option<Mesh3D>> = OnceLock::new();
        static CYLINDER: OnceLock<Option<Mesh3D>> = OnceLock::new();
        static CONE: OnceLock<Option<Mesh3D>> = OnceLock::new();

        match key {
            "primitive:box" => Some(Self::Box),
            "primitive:sphere" => Some(Self::Sphere),
            "primitive:rectangle" => RECTANGLE
                .get_or_init(|| {
                    Self::prepare_primitive(Primitive::Rectangle, scene_primitives::rectangle())
                })
                .clone(),
            "primitive:cylinder" => CYLINDER
                .get_or_init(|| {
                    Self::prepare_primitive(Primitive::Cylinder, scene_primitives::cylinder())
                })
                .clone(),
            "primitive:cone" => CONE
                .get_or_init(|| Self::prepare_primitive(Primitive::Cone, scene_primitives::cone()))
                .clone(),
            _ => None,
        }
    }

    fn prepare_primitive(
        kind: Primitive,
        parts: crate::scene_primitives::PrimitiveParts,
    ) -> Option<Self> {
        Self::prepare_parts(Some(kind), parts)
    }

    pub(crate) fn prepare_capsule(parts: crate::scene_primitives::PrimitiveParts) -> Option<Self> {
        Self::prepare_primitive(Primitive::Capsule, parts)
    }

    pub(crate) fn prepare_procedural(
        parts: crate::scene_primitives::PrimitiveParts,
    ) -> Option<Self> {
        Self::prepare_parts(None, parts)
    }

    fn prepare_parts(
        kind: Option<Primitive>,
        parts: crate::scene_primitives::PrimitiveParts,
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
        let Self::Imported(mut mesh) = Self::imported(vertices, Vec::new(), ranges, materials)
            .ok()?
            .with_material_slots(slots.into())?
        else {
            return None;
        };
        let geometry = Arc::get_mut(&mut Arc::get_mut(&mut mesh)?.geometry)?;
        geometry.primitive = kind;
        Some(Self::Imported(mesh))
    }

    pub(crate) fn with_procedural_layout(
        &self,
        layout: &[crate::scene_authored_mesh::SceneMeshPart],
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

    pub fn imported(
        vertices: Vec<MeshVertex>,
        tangents: Vec<MeshTangent>,
        parts: Vec<MeshPart>,
        materials: Vec<MeshMaterial>,
    ) -> Result<Self, &'static str> {
        Self::imported_shared(vertices, tangents, parts, materials.into())
    }

    #[doc(hidden)]
    pub fn imported_shared(
        vertices: Vec<MeshVertex>,
        tangents: Vec<MeshTangent>,
        parts: Vec<MeshPart>,
        materials: Arc<[MeshMaterial]>,
    ) -> Result<Self, &'static str> {
        if vertices.is_empty()
            || !vertices.len().is_multiple_of(3)
            || vertices.len() > MAX_IMPORTED_VERTICES
        {
            return Err("mesh must contain complete triangles within the vertex limit");
        }
        let mut end = 0;
        for part in &parts {
            if part.vertices.start != end
                || part.vertices.is_empty()
                || !part.vertices.len().is_multiple_of(3)
                || part.material >= materials.len()
            {
                return Err("mesh parts must cover contiguous triangles with valid materials");
            }
            end = part.vertices.end;
        }
        if end != vertices.len() {
            return Err("mesh parts do not cover the vertex buffer");
        }
        let needs_tangents = parts.iter().any(|part| {
            materials[part.material]
                .pbr
                .as_ref()
                .and_then(MeshPbr::tangent_normal_map)
                .is_some()
        });
        if (needs_tangents || !tangents.is_empty()) && tangents.len() != vertices.len() {
            return Err("mesh tangent data does not match the vertex buffer");
        }
        if materials.iter().any(|material| {
            !material
                .base_color
                .iter()
                .all(|channel| (0.0..=1.0).contains(channel))
                || material
                    .alpha_mode
                    .cutoff()
                    .is_some_and(|alpha| !alpha.is_finite() || alpha < 0.0)
                || material
                    .base_texture
                    .as_ref()
                    .is_some_and(|texture| texture.uv_slot != 0 || !texture.transform.is_valid())
                || material.pbr.as_ref().is_some_and(|pbr| !pbr.is_valid())
        }) {
            return Err("mesh contains an invalid material factor");
        }
        if parts
            .iter()
            .filter_map(|part| part.volume_inverse)
            .flat_map(|inverse| inverse.into_iter().flatten())
            .any(|value| !value.is_finite())
        {
            return Err("invalid mesh volume transform");
        }
        let mut min = [f32::INFINITY; 3];
        let mut max = [f32::NEG_INFINITY; 3];
        let mut part_centers = Vec::with_capacity(parts.len());
        for part in &parts {
            let mut low = [f32::INFINITY; 3];
            let mut high = [f32::NEG_INFINITY; 3];
            for vertex in &vertices[part.vertices.clone()] {
                if !vertex
                    .position
                    .into_iter()
                    .chain(vertex.normal)
                    .chain(vertex.uv)
                    .chain(vertex.uv1)
                    .all(f32::is_finite)
                    || !vertex
                        .color
                        .iter()
                        .all(|channel| (0.0..=1.0).contains(channel))
                    || Vector3::from(vertex.normal).norm_squared() < 1e-8
                {
                    return Err("mesh contains an invalid position or normal");
                }
                for axis in 0..3 {
                    low[axis] = low[axis].min(vertex.position[axis]);
                    high[axis] = high[axis].max(vertex.position[axis]);
                }
            }
            for axis in 0..3 {
                min[axis] = min[axis].min(low[axis]);
                max[axis] = max[axis].max(high[axis]);
            }
            part_centers.push(std::array::from_fn(|axis| {
                f32::midpoint(low[axis], high[axis])
            }));
        }
        let geometry = Arc::new(MeshGeometry {
            streams: Arc::new(MeshVertexStreams {
                draw_indices: (vertices.len() >= MIN_INDEXED_VERTICES).then(Box::default),
                vertices,
                tangents,
                lightmap_uv: Vec::new(),
            }),
            parts,
            part_centers,
            lightmap_min_size: 0,
            min,
            max,
            skin: None,
            morph: None,
            heightfield: None,
            pick_tree: OnceLock::new(),
            primitive: None,
        });
        Ok(Self::Imported(Arc::new(MeshData::with_materials(
            geometry, materials, None,
        ))))
    }

    fn primitive(&self) -> Option<Primitive> {
        match self {
            Self::Box => Some(Primitive::Box),
            Self::Sphere => Some(Primitive::Sphere),
            Self::Imported(mesh) => mesh.primitive,
        }
    }

    fn material_source(&self) -> Option<&Arc<MeshData>> {
        let builtin = |mesh: &Mesh3D| {
            let vertices = mesh.vertices().to_vec();
            let parts = vec![MeshPart {
                vertices: 0..vertices.len(),
                material: 0,
                volume_inverse: None,
            }];
            let materials = vec![default_mesh_material()];
            let Self::Imported(mut source) =
                Self::imported(vertices, Vec::new(), parts, materials).ok()?
            else {
                return None;
            };
            Arc::get_mut(&mut Arc::get_mut(&mut source)?.geometry)?.primitive = mesh.primitive();
            Some(source)
        };
        static BOX: OnceLock<Option<Arc<MeshData>>> = OnceLock::new();
        static SPHERE: OnceLock<Option<Arc<MeshData>>> = OnceLock::new();
        match self {
            Self::Box => BOX.get_or_init(|| builtin(self)).as_ref(),
            Self::Sphere => SPHERE.get_or_init(|| builtin(self)).as_ref(),
            Self::Imported(data) => Some(data),
        }
    }

    pub(crate) fn styled(self, style: Option<&Arc<SceneAppearance>>) -> Self {
        let Some(style) = style else {
            return self;
        };
        let Some(source) = self.material_source() else {
            return self;
        };
        let mut changed = false;
        let mut derivative_normals = Vec::with_capacity(source.materials.len());
        let materials = source
            .materials
            .iter()
            .enumerate()
            .map(|(index, material)| {
                let style = style
                    .styles(source.material_slots.get(index), index)
                    .cloned()
                    .reduce(|mut before, later| {
                        before.overlay(&later);
                        before
                    });
                let Some(resolved) = style else {
                    derivative_normals.push(false);
                    return material.clone();
                };
                changed = true;
                derivative_normals.push(resolved.overrides_normal_texture());
                resolved.prepare(material)
            })
            .collect();
        if !changed {
            return self;
        }
        let mut result =
            MeshData::with_materials(source.geometry.clone(), materials, Some(style.clone()));
        result.source_materials = Some(source.materials.clone());
        result.derivative_normals = derivative_normals;
        result.material_slots = source.material_slots.clone();
        Self::Imported(Arc::new(result))
    }

    pub(crate) fn with_instance_blending(self) -> Self {
        let builtin = matches!(self, Self::Box | Self::Sphere);
        let Some(source) = self.material_source() else {
            return self;
        };
        if source.materials.iter().all(|material| {
            matches!(
                material.alpha_mode,
                MeshAlphaMode::Blend | MeshAlphaMode::InstanceBlend { .. }
            )
        }) {
            return self;
        }
        let materials = source
            .materials
            .iter()
            .cloned()
            .map(|mut material| {
                material.alpha_mode = match material.alpha_mode {
                    MeshAlphaMode::Opaque => MeshAlphaMode::InstanceBlend {
                        cutoff: None,
                        builtin,
                    },
                    MeshAlphaMode::Mask(cutoff) => MeshAlphaMode::InstanceBlend {
                        cutoff: Some(cutoff),
                        builtin,
                    },
                    mode => mode,
                };
                if builtin {
                    material.pbr = None;
                    material.cull_mode = MeshCullMode::None;
                }
                material
            })
            .collect();
        let mut mesh =
            MeshData::with_materials(source.geometry.clone(), materials, source.authored.clone());
        mesh.derivative_normals
            .clone_from(&source.derivative_normals);
        mesh.material_slots = source.material_slots.clone();
        mesh.source_materials = Some(
            source
                .source_materials
                .as_ref()
                .unwrap_or(&source.materials)
                .clone(),
        );
        Self::Imported(Arc::new(mesh))
    }

    #[doc(hidden)]
    pub fn material_slots(&self) -> &[MeshMaterialSlot] {
        match self {
            Self::Imported(data) => &data.material_slots,
            _ => &[],
        }
    }

    #[doc(hidden)]
    pub fn with_material_slots(mut self, slots: Arc<[MeshMaterialSlot]>) -> Option<Self> {
        if slots.is_empty() {
            return Some(self);
        }
        let Self::Imported(mesh) = &mut self else {
            return None;
        };
        let data = Arc::get_mut(mesh)?;
        if slots.len() != data.materials.len() {
            return None;
        }
        data.material_slots = slots;
        Some(self)
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
            let (offsets, deltas) = morph.subset(vertices)?;
            mesh = mesh.with_morph(
                morph.target_count(),
                morph.target_names().clone(),
                offsets,
                deltas,
            )?;
        }
        Some(mesh)
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

    /// Authored or generated lightmap coordinates, independent of material UV slots.
    pub fn lightmap_uvs(&self) -> &[[f32; 2]] {
        match self {
            Self::Imported(mesh) => &mesh.lightmap_uv,
            _ => &[],
        }
    }

    pub fn lightmap_min_size(&self) -> u32 {
        match self {
            Self::Imported(mesh) => mesh.lightmap_min_size,
            _ => 0,
        }
    }

    #[doc(hidden)]
    pub fn with_lightmap_uv(self, uv: Vec<[f32; 2]>) -> Option<Self> {
        self.attach_lightmap_uv(uv, 0)
    }

    #[doc(hidden)]
    pub fn heightfield(&self) -> Option<&MeshHeightField> {
        let Self::Imported(data) = self else {
            return None;
        };
        data.heightfield.as_deref()
    }

    #[doc(hidden)]
    pub fn with_heightfield(self, dimensions: [usize; 2], samples: Vec<f32>) -> Option<Self> {
        let [width, height] = dimensions;
        if width < 2
            || height < 2
            || width.checked_mul(height)? != samples.len()
            || samples.iter().any(|height| !(0.0..=1.0).contains(height))
        {
            return None;
        }
        self.attach_heightfield(Arc::new(MeshHeightField {
            dimensions,
            samples: samples.into_boxed_slice(),
        }))
    }

    #[doc(hidden)]
    pub fn with_packed_heightfield(self, dimensions: [usize; 2], bytes: &[u8]) -> Option<Self> {
        let (words, remainder) = bytes.as_chunks::<4>();
        if !remainder.is_empty() {
            return None;
        }
        self.with_heightfield(
            dimensions,
            words.iter().copied().map(f32::from_le_bytes).collect(),
        )
    }

    fn attach_heightfield(mut self, heightfield: Arc<MeshHeightField>) -> Option<Self> {
        let Self::Imported(data) = &mut self else {
            return None;
        };
        Arc::make_mut(&mut Arc::make_mut(data).geometry).heightfield = Some(heightfield);
        Some(self)
    }

    #[doc(hidden)]
    pub fn with_generated_lightmap_uv(mut self, uv: Vec<[f32; 2]>, min_size: u32) -> Option<Self> {
        let Self::Imported(source) = &mut self else {
            return None;
        };
        if source.skin.is_some() || source.morph.is_some() {
            return None;
        }
        if Arc::get_mut(source).is_some() {
            return self.attach_lightmap_uv(uv, min_size);
        }
        let mut mesh = Self::imported_shared(
            source.vertices.clone(),
            source.tangents.clone(),
            source.parts.clone(),
            source.materials.clone(),
        )
        .ok()?
        .with_material_slots(source.material_slots.clone())?;
        if let Some(heightfield) = &source.heightfield {
            mesh = mesh.attach_heightfield(heightfield.clone())?;
        }
        mesh.attach_lightmap_uv(uv, min_size)
    }

    fn attach_lightmap_uv(mut self, uv: Vec<[f32; 2]>, min_size: u32) -> Option<Self> {
        if uv.is_empty() {
            return Some(self);
        }
        let Self::Imported(mesh) = &mut self else {
            return None;
        };
        let data = Arc::get_mut(&mut Arc::get_mut(mesh)?.geometry)?;
        if uv.len() != data.vertices.len() || !uv.iter().flatten().all(|value| value.is_finite()) {
            return None;
        }
        let streams = Arc::get_mut(&mut data.streams)?;
        streams.lightmap_uv = uv;
        streams.invalidate_indices();
        data.lightmap_min_size = min_size;
        Some(self)
    }

    #[doc(hidden)]
    pub fn with_packed_lightmap_uv(self, bytes: &[u8], min_size: u32) -> Option<Self> {
        let (coordinates, rest) = bytes.as_chunks::<8>();
        if !rest.is_empty() || coordinates.len() > MAX_IMPORTED_VERTICES {
            return None;
        }
        self.attach_lightmap_uv(
            coordinates
                .iter()
                .map(|uv| {
                    let [x, y] = uv.as_chunks::<4>().0 else {
                        unreachable!()
                    };
                    [*x, *y].map(f32::from_le_bytes)
                })
                .collect(),
            min_size,
        )
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
        mut self,
        target_count: usize,
        names: Arc<[String]>,
        offsets: Vec<u32>,
        deltas: Vec<MorphDelta>,
    ) -> Option<Self> {
        let Self::Imported(mesh) = &mut self else {
            return None;
        };
        let mesh = Arc::get_mut(mesh)?;
        let morph = MorphGeometry::new(
            target_count,
            names,
            offsets,
            deltas,
            &mesh.vertices,
            mesh.blended_parts
                .iter()
                .map(|(part, _)| (*part, mesh.parts[*part].vertices.clone())),
        )?;
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
        let offsets = words
            .get(..count)?
            .iter()
            .copied()
            .map(u32::from_le_bytes)
            .collect();
        let (records, remainder) = words[count..].as_chunks::<10>();
        if !remainder.is_empty() || records.len() > MAX_MORPH_DELTAS {
            return None;
        }
        let deltas = records
            .iter()
            .map(|record| {
                let vector =
                    |offset| std::array::from_fn(|axis| f32::from_le_bytes(record[offset + axis]));
                MorphDelta {
                    target: u32::from_le_bytes(record[0]),
                    position: vector(1),
                    normal: vector(4),
                    tangent: vector(7),
                }
            })
            .collect();
        self.with_morph(
            target_count,
            names.iter().map(|name| (*name).to_owned()).collect(),
            offsets,
            deltas,
        )
    }

    pub fn morph_geometry(&self) -> Option<&MorphGeometry> {
        match self {
            Self::Imported(mesh) => mesh.morph.as_deref(),
            _ => None,
        }
    }

    /// Read the compact triangle data emitted by the AOT resource compiler.
    #[doc(hidden)]
    pub fn from_packed(
        bytes: &[u8],
        tangent_bytes: &[u8],
        parts: Vec<MeshPart>,
        materials: Vec<MeshMaterial>,
    ) -> Option<Self> {
        if !bytes.len().is_multiple_of(MeshVertex::STRIDE)
            || bytes.len() / MeshVertex::STRIDE > MAX_IMPORTED_VERTICES
            || !tangent_bytes.len().is_multiple_of(8)
            || tangent_bytes.len() / 8 > MAX_IMPORTED_VERTICES
        {
            return None;
        }
        let vertices = bytes
            .as_chunks::<{ MeshVertex::STRIDE }>()
            .0
            .iter()
            .map(|row| {
                let [x, y, z, nx, ny, nz, u, v, r, g, b, a, u1, v1] = row.as_chunks::<4>().0 else {
                    unreachable!()
                };
                MeshVertex {
                    position: [*x, *y, *z].map(f32::from_le_bytes),
                    normal: [*nx, *ny, *nz].map(f32::from_le_bytes),
                    uv: [*u, *v].map(f32::from_le_bytes),
                    uv1: [*u1, *v1].map(f32::from_le_bytes),
                    color: [*r, *g, *b, *a].map(f32::from_le_bytes),
                }
            })
            .collect();
        let tangents = tangent_bytes
            .as_chunks::<8>()
            .0
            .iter()
            .map(|bytes| {
                let [x, y, z, w] = bytes.as_chunks::<2>().0 else {
                    unreachable!()
                };
                [*x, *y, *z, *w].map(i16::from_le_bytes)
            })
            .collect();
        Self::imported(vertices, tangents, parts, materials).ok()
    }

    pub fn parts(&self) -> &[MeshPart] {
        match self {
            Self::Imported(mesh) => &mesh.parts,
            _ => &[],
        }
    }

    #[doc(hidden)]
    pub fn uses_base_items(&self) -> bool {
        matches!(self, Self::Imported(mesh) if mesh.uses_base_items())
    }

    pub fn materials(&self) -> &[MeshMaterial] {
        match self {
            Self::Imported(mesh) => &mesh.materials,
            _ => &[],
        }
    }

    /// Authored normal maps use the exact transformed UV derivatives.
    #[doc(hidden)]
    pub fn uses_vertex_tangents(&self, material: usize) -> bool {
        matches!(self, Self::Imported(mesh) if !mesh.tangents.is_empty()
            && !mesh.derivative_normals.get(material).copied().unwrap_or(false))
    }

    /// Prepare reusable draw indices without changing the triangle data used for picking.
    #[doc(hidden)]
    pub fn draw_indices(&self) -> Option<&MeshDrawIndices> {
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

    pub fn transmission_blur(&self) -> Option<f32> {
        match self {
            Self::Imported(mesh) => mesh.transmission_blur,
            _ => None,
        }
    }

    pub fn has_volume(&self) -> bool {
        matches!(self, Self::Imported(mesh) if mesh.has_volume)
    }

    pub fn has_depth_writing_transmission(&self) -> bool {
        matches!(self, Self::Imported(mesh) if mesh.has_depth_writing_transmission)
    }

    #[doc(hidden)]
    pub fn shader_displacement(&self) -> f32 {
        match self {
            Self::Imported(mesh) => mesh.shader_displacement,
            _ => 0.0,
        }
    }

    /// Geometry and material state used by the scene's depth-only passes.
    #[doc(hidden)]
    pub fn same_shadow_mesh(&self, other: &Self) -> bool {
        self == other
            || match (self, other) {
                (Self::Imported(a), Self::Imported(b)) => {
                    Arc::ptr_eq(a.vertex_streams(), b.vertex_streams())
                        && a.parts.len() == b.parts.len()
                        && a.parts.iter().zip(&b.parts).all(|(left, right)| {
                            left.vertices == right.vertices
                                && a.materials[left.material]
                                    .same_shadow_surface(&b.materials[right.material])
                        })
                }
                _ => false,
            }
    }

    #[doc(hidden)]
    pub fn has_auxiliary_depth(&self) -> bool {
        match self {
            Self::Imported(mesh) => mesh.has_auxiliary_depth,
            _ => true,
        }
    }

    #[doc(hidden)]
    pub fn has_opaque_depth_prepass(&self) -> bool {
        match self {
            Self::Imported(mesh) => mesh.has_opaque_depth_prepass,
            _ => true,
        }
    }

    #[doc(hidden)]
    pub fn depth_participation(&self) -> ([bool; 2], bool) {
        match self {
            Self::Imported(mesh) => (mesh.depth_prepasses, mesh.blended_depth),
            _ => ([false; 2], false),
        }
    }

    pub fn has_opaque_parts(&self) -> bool {
        match self {
            Self::Imported(mesh) => mesh.has_opaque_parts,
            _ => true,
        }
    }

    /// Model-space bounds centers of the blend parts, in source order.
    pub fn blended_parts(&self) -> &[(usize, [f32; 3])] {
        match self {
            Self::Imported(mesh) => &mesh.blended_parts,
            _ => &[],
        }
    }

    pub fn sections(&self) -> impl Iterator<Item = (Range<usize>, Option<&MeshMaterial>)> {
        let builtin = self
            .parts()
            .is_empty()
            .then(|| (0..self.vertices().len(), None));
        builtin.into_iter().chain(self.parts().iter().map(|part| {
            (
                part.vertices.clone(),
                Some(&self.materials()[part.material]),
            )
        }))
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

    fn pick_tree(&self) -> Option<&PickTree> {
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

    pub(crate) fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        match self {
            Self::Box => ([-0.5; 3], [0.5; 3]),
            Self::Sphere => ([-1.0; 3], [1.0; 3]),
            Self::Imported(mesh) => (mesh.min, mesh.max),
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InstancingLodRange([f64; 2]);

impl InstancingLodRange {
    const ALL: Self = Self([0.0, f64::INFINITY]);

    pub(crate) fn from_limits([near, far]: [f64; 2]) -> Self {
        let near = if near.is_finite() { near.max(0.0) } else { 0.0 };
        let far = if far.is_finite() && far >= 0.0 {
            far
        } else {
            f64::INFINITY
        };
        Self([near * near, far * far])
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct SceneObject {
    pub id: String,
    pub mesh: Mesh3D,
    pub color: Color,
    /// World-unit offset used only by depth-sorted transparency.
    depth_bias: f64,
    pub(crate) lod_coverage: u8,
    motion_vector_enabled: bool,
    motion_vector_scale: f32,
    pub(crate) instancing_lod: InstancingLodRange,
    pub(crate) instance_data: Option<[f32; 4]>,
    pub casts_shadow: bool,
    pub receives_shadow: bool,
    pub receives_reflections: bool,
    pub casts_reflections: bool,
    pub layers: u64,
    pub lightmap: Option<BakedLightmap>,
    pub bake_role: BakeRole,
    pub lightmap_resolution: LightmapResolution,
    source_row: Option<NonZeroU32>,
    pub(crate) lighting_index: u16,
    pub(crate) imported_placement: Option<(u32, Option<u32>)>,
    pub(crate) instance_order: Option<InstanceOrder>,
    pub(crate) instance_shadow_bounds: Option<Arc<[[f32; 3]; 2]>>,
    pickable: bool,
    collidable: bool,
    trigger: bool,
    transform: SceneTransform,
    skin: Option<Arc<SkinPose>>,
    morph: Option<Arc<MorphPose>>,
    styled_blend_centers: Option<Arc<[(usize, [f32; 3])]>>,
}

/// Transparent ordering information for a row of an instancing table.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InstanceOrder {
    pub group: [u32; 2],
    pub ordinal: u32,
    pub group_origin: [f32; 3],
    pub origin: [f32; 3],
    pub depth_sorting: bool,
}

impl SceneObject {
    #[doc(hidden)]
    pub fn instance_order(&self) -> Option<InstanceOrder> {
        self.instance_order
    }

    /// The shared world-space envelope for fitting a table's shadow projections.
    #[doc(hidden)]
    pub fn instance_shadow_bounds(&self) -> Option<&Arc<[[f32; 3]; 2]>> {
        self.instance_shadow_bounds.as_ref()
    }
    pub(crate) fn instance_address(&self) -> Option<[i64; 3]> {
        let row = i64::from(self.scene_row()?);
        let Some((node, instance)) = self.imported_placement() else {
            return Some([row, -1, -1]);
        };
        Some([row, mesh_node_index(node), instance.map_or(-1, i64::from)])
    }

    #[doc(hidden)]
    pub fn lighting_index(&self) -> u16 {
        self.lighting_index
    }

    #[doc(hidden)]
    pub fn imported_placement(&self) -> Option<(u32, Option<u32>)> {
        self.imported_placement
    }

    /// Resolve a mesh key separately from the object's stable identity.
    /// Built-in scene keys and imported mesh keys share this entry point.
    pub fn from_mesh(
        id: String,
        mesh: Mesh3D,
        center: [f64; 3],
        scale: [f64; 3],
        angles: [f64; 3],
        color: Color,
    ) -> Option<Self> {
        Self::new(
            id,
            mesh,
            center.map(|v| v as f32),
            scale.map(|v| v as f32),
            angles.map(|v| v as f32),
            color,
        )
    }

    /// Construct a box at its center using world dimensions and yaw, pitch,
    /// and roll in degrees (applied in pitch, yaw, roll order).
    pub fn cuboid(
        id: impl Into<String>,
        center: [f32; 3],
        dimensions: [f32; 3],
        angles: [f32; 3],
        color: Color,
    ) -> Option<Self> {
        Self::new(id.into(), Mesh3D::Box, center, dimensions, angles, color)
    }

    /// Construct a sphere from its center and world-space radius.
    pub fn sphere(
        id: impl Into<String>,
        center: [f32; 3],
        radius: f32,
        color: Color,
    ) -> Option<Self> {
        Self::new(
            id.into(),
            Mesh3D::Sphere,
            center,
            [radius; 3],
            [0.0; 3],
            color,
        )
    }

    fn new(
        id: String,
        mesh: Mesh3D,
        center: [f32; 3],
        scale: [f32; 3],
        angles: [f32; 3],
        color: Color,
    ) -> Option<Self> {
        let (transform, _) = SceneTransform::local(center, scale, angles, [0.0; 3])?;
        Some(Self::from_transform(id, mesh, transform, color))
    }

    pub(crate) fn from_transform(
        id: String,
        mesh: Mesh3D,
        transform: SceneTransform,
        color: Color,
    ) -> Self {
        Self {
            id,
            mesh,
            color,
            depth_bias: 0.0,
            lod_coverage: 0,
            motion_vector_enabled: true,
            motion_vector_scale: 1.0,
            instancing_lod: InstancingLodRange::ALL,
            instance_data: None,
            casts_shadow: true,
            receives_shadow: true,
            receives_reflections: true,
            casts_reflections: true,
            layers: MAIN_LAYER,
            lightmap: None,
            bake_role: BakeRole::Auto,
            lightmap_resolution: LightmapResolution::Inherit,
            source_row: None,
            lighting_index: 0,
            imported_placement: None,
            instance_order: None,
            instance_shadow_bounds: None,
            pickable: true,
            collidable: true,
            trigger: false,
            transform,
            skin: None,
            morph: None,
            styled_blend_centers: None,
        }
    }

    /// Set the depth-sort offset. Positive values move the sort key farther away.
    pub fn with_depth_bias(mut self, bias: f64) -> Self {
        self.depth_bias = if bias.is_finite() { bias } else { 0.0 };
        self
    }

    pub fn depth_bias(&self) -> f64 {
        self.depth_bias
    }

    #[doc(hidden)]
    pub fn lod_coverage(&self) -> u8 {
        self.lod_coverage
    }

    /// Configure this surface's participation and displacement in temporal reprojection.
    pub fn with_motion_vectors(mut self, enabled: bool, scale: f64) -> Self {
        self.motion_vector_enabled = enabled;
        let scale = scale as f32;
        self.motion_vector_scale = if scale.is_finite() { scale } else { 1.0 };
        self
    }

    pub fn motion_vector_enabled(&self) -> bool {
        self.motion_vector_enabled
    }

    pub fn motion_vector_scale(&self) -> f32 {
        self.motion_vector_scale
    }

    /// Four per-instance channels shared by programmable vertex and fragment stages.
    pub fn instance_data(&self) -> [f32; 4] {
        self.instance_data.unwrap_or([0.0; 4])
    }

    /// Choose the camera distances, in world units, at which this instance is visible.
    pub fn with_instancing_lod_range(mut self, range: [f64; 2]) -> Self {
        self.instancing_lod = InstancingLodRange::from_limits(range);
        self
    }

    pub(crate) fn has_instancing_lod_range(&self) -> bool {
        self.instancing_lod != InstancingLodRange::ALL
    }

    #[doc(hidden)]
    pub fn visible_at_distance(&self, eye: [f32; 3]) -> bool {
        if !self.has_instancing_lod_range() {
            return true;
        }
        let distance =
            (self.transform.origin.cast::<f64>() - Vector3::from(eye).cast::<f64>()).norm_squared();
        let InstancingLodRange([near, far]) = self.instancing_lod;
        near <= distance && distance < far
    }

    #[doc(hidden)]
    pub fn with_shadows(mut self, casts: bool, receives: bool) -> Self {
        self.casts_shadow = casts;
        self.receives_shadow = receives;
        self
    }

    #[doc(hidden)]
    pub fn is_pickable(&self) -> bool {
        self.pickable
    }

    pub fn with_pickable(mut self, enabled: bool) -> Self {
        self.pickable = enabled;
        self
    }

    pub fn with_collidable(mut self, enabled: bool) -> Self {
        self.collidable = enabled;
        self
    }

    /// Source index for a material program, independent of GPU culling and draw order.
    pub fn shader_instance_index(&self) -> u32 {
        self.imported_placement
            .and_then(|(_, instance)| instance)
            .or_else(|| self.scene_row())
            .unwrap_or(0)
    }

    #[doc(hidden)]
    pub fn same_vertex_program_instance(&self, other: &Self) -> bool {
        self.mesh.shader_displacement() == 0.0
            || self.color == other.color
                && self.instance_data() == other.instance_data()
                && self.shader_instance_index() == other.shader_instance_index()
    }

    pub(crate) fn scene_row(&self) -> Option<u32> {
        self.source_row.map(|row| row.get() - 1)
    }

    #[doc(hidden)]
    pub fn with_trigger(mut self, enabled: bool) -> Self {
        self.trigger = enabled
            && matches!(
                self.mesh.primitive(),
                Some(
                    Primitive::Box
                        | Primitive::Sphere
                        | Primitive::Cylinder
                        | Primitive::Cone
                        | Primitive::Capsule
                )
            );
        self
    }

    pub(crate) fn is_trigger(&self) -> bool {
        self.trigger
    }

    pub(crate) fn relative_to(mut self, root: SceneTransform) -> Option<Self> {
        self.transform = self.transform.relative_to(root)?;
        Some(self)
    }

    pub(crate) fn with_parent_transform(mut self, parent: SceneTransform) -> Option<Self> {
        self.transform = parent.child(self.transform)?;
        Some(self)
    }

    pub(crate) fn sweep_contact(&self, sweep: ShapeSweep, max: f64) -> Option<MeshContact> {
        if !self.collidable || self.trigger {
            return None;
        }
        self.shape_contact(sweep, max)
    }

    pub(crate) fn trigger_contact(&self, sweep: ShapeSweep, max: f64) -> Option<MeshContact> {
        if !self.trigger {
            return None;
        }
        self.shape_contact(sweep, max)
    }

    fn shape_contact(&self, sweep: ShapeSweep, max: f64) -> Option<MeshContact> {
        if sweep.span(max).entry(self.world_bounds()).is_none() {
            return None;
        }
        let linear = self.transform.linear.cast::<f64>();
        let origin = self.transform.origin.cast::<f64>();
        let inverse = linear.try_inverse();
        let local_center = inverse.map(|inverse| inverse * (sweep.origin - origin));
        let triangles = self.mesh.vertices().as_chunks::<3>().0;
        let inside_faces = |point: Vector3<f64>| {
            triangles.iter().all(|triangle| {
                let [a, b, c] =
                    triangle.map(|vertex| Vector3::from(vertex.position.map(f64::from)));
                let normal = (b - a).cross(&(c - a));
                normal == Vector3::zeros() || normal.dot(&(point - a)) < 0.0
            })
        };
        // Generated primitives are convex solids. Imported geometry contributes
        // its posed surfaces, which may describe open or disconnected meshes.
        let inside = local_center.is_some_and(|point| match self.mesh.primitive() {
            Some(Primitive::Box) => point.abs().max() < 0.5,
            Some(Primitive::Sphere) if point.norm_squared() < 1.0 => {
                triangles.iter().all(|triangle| {
                    let [a, b, c] =
                        triangle.map(|vertex| Vector3::from(vertex.position.map(f64::from)));
                    let normal = (b - a).cross(&(c - a));
                    let plane = normal.dot(&a);
                    plane == 0.0 || normal.dot(&(point - a)) * plane.signum() < 0.0
                })
            }
            Some(Primitive::Cylinder)
                if point.y.abs() < 0.5 && point.x * point.x + point.z * point.z < 0.25 =>
            {
                inside_faces(point)
            }
            Some(Primitive::Cone)
                if point.y > 0.0
                    && point.y < 1.0
                    && point.x * point.x + point.z * point.z < 0.25 * (1.0 - point.y).powi(2) =>
            {
                inside_faces(point)
            }
            Some(Primitive::Capsule) => {
                let (min, max) = self.mesh.bounds();
                (0..3).all(|axis| {
                    point[axis] > f64::from(min[axis]) && point[axis] < f64::from(max[axis])
                }) && inside_faces(point)
            }
            _ => false,
        });
        let pose = self.pick_pose();
        let mut best: Option<(usize, TriangleContact, [MeshVertex; 3], bool)> = None;
        let mut inspect = |vertex: usize, closest: f64| {
            let triangle = pose.deform::<false>(vertex, triangles[vertex / 3]);
            let world =
                triangle.map(|p| linear * Vector3::from(p.position.map(f64::from)) + origin);
            if let Some((contact, intersected)) = cast_shape_triangle(sweep, world, inside, closest)
                && best
                    .as_ref()
                    .is_none_or(|(previous_vertex, previous, _, previous_inside)| {
                        contact
                            .distance
                            .total_cmp(&previous.distance)
                            .then_with(|| previous_inside.cmp(&intersected))
                            .then(contact.initial_gap.total_cmp(&previous.initial_gap))
                            .then(vertex.cmp(previous_vertex))
                            .is_lt()
                    })
            {
                let distance = contact.distance;
                best = Some((vertex, contact, triangle, intersected));
                distance
            } else {
                closest
            }
        };
        let tree = (!inside && !pose.is_deformed())
            .then(|| self.mesh.pick_tree())
            .flatten();
        if let (Some(tree), Some(inverse), Some(local_center)) = (tree, inverse, local_center) {
            // Inverse-transforming a world sphere gives an ellipsoid. Its
            // coordinate radii conservatively expand every mesh-space BVH node.
            let span = BoundsSegment {
                origin: local_center,
                direction: inverse * sweep.direction,
                min: 0.0,
                max,
                expansion: Vector3::from_fn(|axis, _| {
                    inverse.row(axis).norm() * sweep.radius
                        + inverse[(axis, 1)].abs() * sweep.half_segment
                }),
            };
            tree.visit_expanded(span, |triangle, closest| {
                inspect(triangle.vertex as usize, closest)
            });
        } else {
            let mut closest = max;
            for vertex in (0..triangles.len() * 3).step_by(3) {
                closest = inspect(vertex, closest);
            }
        }
        let (_, contact, triangle, inside) = best?;
        let local_position = triangle
            .iter()
            .zip(contact.weights)
            .fold(Vector3::zeros(), |sum, (point, weight)| {
                sum + Vector3::from(point.position.map(f64::from)) * weight
            });
        let uv = std::array::from_fn(|axis| {
            triangle
                .iter()
                .zip(contact.weights)
                .map(|(point, weight)| f64::from(point.uv[axis]) * weight)
                .sum()
        });
        let local_normal = (linear.transpose() * contact.normal)
            .try_normalize(0.0)
            .unwrap_or(contact.normal);
        Some(MeshContact {
            contact,
            local_position,
            local_normal,
            uv,
            inside,
        })
    }

    pub(crate) fn with_source_row(mut self, row: u32) -> Self {
        self.source_row = NonZeroU32::new(row + 1);
        self
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
            || mesh.blended_parts.is_empty()
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
            && inherited
                .iter()
                .map(|(index, _)| index)
                .eq(mesh.blended_parts.iter().map(|(index, _)| index))
        {
            self.styled_blend_centers = None;
            return;
        }
        let pose = self.pick_pose();
        let triangles = mesh.vertices.as_chunks::<3>().0;
        self.styled_blend_centers = Some(
            mesh.blended_parts
                .iter()
                .map(|&(index, _)| {
                    if inherit
                        && let Ok(old) = inherited.binary_search_by_key(&index, |(part, _)| *part)
                    {
                        return inherited[old];
                    }
                    let mut min = [f32::INFINITY; 3];
                    let mut max = [f32::NEG_INFINITY; 3];
                    for vertex in mesh.parts[index].vertices.clone().step_by(3) {
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

    fn bounds(&self) -> ([f32; 3], [f32; 3]) {
        if let Some(skin) = &self.skin {
            skin.bounds
        } else if let Some(morph) = &self.morph {
            morph.bounds
        } else {
            self.mesh.bounds()
        }
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
            return Some(self.mesh.clone());
        }
        let Mesh3D::Imported(mesh) = &self.mesh else {
            return None;
        };
        let mut vertices = Vec::with_capacity(mesh.vertices.len());
        let normal_transform = self.transform.inverse.transpose();
        for (index, triangle) in mesh.vertices.as_chunks::<3>().0.iter().enumerate() {
            vertices.extend(pose.deform::<true>(index * 3, *triangle).map(|mut vertex| {
                vertex.position = (self.transform.origin
                    + self.transform.linear * Vector3::from(vertex.position))
                .into();
                let normal = normal_transform * Vector3::from(vertex.normal);
                vertex.normal = normal.try_normalize(1e-12).unwrap_or(Vector3::y()).into();
                vertex
            }));
        }
        let mut tangents: Vec<_> = mesh
            .tangents
            .iter()
            .enumerate()
            .map(|(index, &[x, y, z, sign])| {
                let mut tangent = Vector3::new(f32::from(x), f32::from(y), f32::from(z));
                if let Some((skin, influences)) = pose.skin
                    && let Some(rows) = influences[index].deformation_rows(&skin.palette)
                {
                    tangent = Vector3::from(
                        rows.map(|[a, b, c, _]| a * tangent.x + b * tangent.y + c * tangent.z),
                    );
                }
                let Some(tangent) = (self.transform.linear * tangent).try_normalize(1e-12) else {
                    return [0; 4];
                };
                std::array::from_fn(|axis| {
                    if axis == 3 {
                        sign
                    } else {
                        (tangent[axis] * 32767.0).round() as i16
                    }
                })
            })
            .collect();
        if self.is_reflected() {
            for triangle in vertices.chunks_exact_mut(3) {
                triangle.swap(1, 2);
            }
            for triangle in tangents.chunks_exact_mut(3) {
                triangle.swap(1, 2);
                for tangent in triangle {
                    tangent[3] = tangent[3].saturating_neg();
                }
            }
        }
        let mut parts = mesh.parts.clone();
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
            .with_material_slots(mesh.material_slots.clone())
    }

    pub(crate) fn intersects_selection(
        &self,
        camera: &SceneCamera,
        region: &Frustum,
        planes: u8,
    ) -> bool {
        if !self.pickable
            || self.color.0 as u8 == 0
                && matches!(&self.mesh, Mesh3D::Imported(mesh) if mesh.instance_blended)
        {
            return false;
        }
        let test_faces = matches!(&self.mesh, Mesh3D::Imported(mesh) if mesh.has_face_culling);
        if planes == 0 && !test_faces {
            return true;
        }
        let pose = self.pick_pose();
        let triangles = self.mesh.vertices().as_chunks::<3>().0;
        let intersects = |vertex, material: Option<&MeshMaterial>, planes| {
            let vertices = pose.deform::<false>(vertex, triangles[vertex / 3]);
            let points = vertices.map(|vertex| {
                self.transform.linear * Vector3::from(vertex.position) + self.transform.origin
            });
            let cull = material.map_or(MeshCullMode::None, |material| material.cull_mode);
            if cull != MeshCullMode::None {
                let [a, b, c] = points;
                let facing = (b - a).cross(&(c - a)).dot(&camera.toward_viewer(a));
                if facing == 0.0 || !cull.accepts((facing > 0.0) ^ self.is_reflected()) {
                    return false;
                }
            }
            region.triangle_intersects(points.map(Into::into), planes)
        };
        if !pose.is_deformed()
            && let Some(tree) = self.mesh.pick_tree()
        {
            tree.intersects_selection(
                region,
                self.transform_rows(),
                planes,
                test_faces,
                |triangle, planes| {
                    let material = self
                        .mesh
                        .parts()
                        .get(triangle.part as usize)
                        .map(|part| &self.mesh.materials()[part.material]);
                    intersects(triangle.vertex as usize, material, planes)
                },
            )
        } else {
            self.mesh.sections().any(|(range, material)| {
                range
                    .step_by(3)
                    .any(|vertex| intersects(vertex, material, planes))
            })
        }
    }

    fn closest_surface(&self, point: Vector3<f64>, limit: f64) -> Option<ClosestSurface> {
        let pose = self.pick_pose();
        let origin = self.transform.origin.cast::<f64>();
        let linear = self.transform.linear.cast::<f64>();
        let triangles = self.mesh.vertices().as_chunks::<3>().0;
        let mut best: Option<ClosestSurface> = None;
        let mut inspect = |vertex: usize, limit: f64| {
            let triangle = pose.deform::<false>(vertex, triangles[vertex / 3]);
            let [a, b, c] =
                triangle.map(|v| linear * Vector3::from(v.position).cast::<f64>() + origin);
            if (b - a).cross(&(c - a)).norm_squared() == 0.0 {
                return limit;
            }
            let (weights, distance_squared) = closest_triangle_point(point, [a, b, c]);
            if distance_squared <= limit
                && (distance_squared < limit || best.is_none_or(|old| vertex < old.vertex))
            {
                let [u, v, w] = weights;
                let position = (a * u + b * v + c * w).cast::<f32>();
                if position.iter().copied().all(f32::is_finite) {
                    best = Some(ClosestSurface {
                        vertex,
                        weights,
                        distance_squared,
                        position,
                    });
                    return distance_squared;
                }
            }
            limit
        };
        if !pose.is_deformed()
            && let Some(tree) = self.mesh.pick_tree()
        {
            tree.visit_nearest(point, self.transform_rows(), limit, inspect);
        } else {
            let mut limit = limit;
            for vertex in (0..triangles.len() * 3).step_by(3) {
                limit = inspect(vertex, limit);
            }
        }
        best
    }

    fn hit(&self, ray: Ray3D, min: f32, max: f32) -> Option<SurfaceHit> {
        self.pickable
            .then(|| self.surface_hit(ray, min, max, false))
            .flatten()
    }

    fn surface_hit(&self, ray: Ray3D, min: f32, max: f32, bake: bool) -> Option<SurfaceHit> {
        // Keep the ray parameter in world units under non-uniform scaling.
        let local_origin = self.transform.inverse * (ray.origin - self.transform.origin);
        let world_step = self.transform.inverse * ray.direction;
        if !overlaps_bounds(local_origin, world_step, self.bounds(), min, max) {
            return None;
        }
        let ray_scale = world_step.norm();
        let local_direction = world_step / ray_scale;
        let triangles = self.mesh.vertices().as_chunks::<3>().0;
        let pose = self.pick_pose();
        let tint_alpha = f32::from(self.color.0 as u8) / 255.0;
        let mut best = None;
        let mut test = |vertex: usize, material: Option<&MeshMaterial>, closest| {
            if bake && material.is_some_and(|material| !bake_opaque(material)) {
                return closest;
            }
            let base = &triangles[vertex / 3];
            let posed;
            let [a, b, c] = if pose.is_deformed() {
                posed = pose.deform::<false>(vertex, *base);
                posed.each_ref()
            } else {
                base.each_ref()
            };
            let Some((distance, u, v, front)) =
                intersect_triangle(local_origin, local_direction, [a, b, c])
            else {
                return closest;
            };
            let distance = distance / ray_scale;
            if !(min..=closest).contains(&distance) {
                return closest;
            }
            let w = 1.0 - u - v;
            if let Some(material) = material {
                if !material.cull_mode.accepts(front) {
                    return closest;
                }
                if material.alpha_mode != MeshAlphaMode::Opaque {
                    let uv = std::array::from_fn(|axis| {
                        a.uv[axis] * w + b.uv[axis] * u + c.uv[axis] * v
                    });
                    let vertex_alpha = a.color[3] * w + b.color[3] * u + c.color[3] * v;
                    let covered = if bake {
                        material.depth_covers(uv, vertex_alpha, tint_alpha)
                    } else {
                        material.covers(uv, vertex_alpha, tint_alpha)
                    };
                    if !covered {
                        return closest;
                    }
                }
            }
            let hit = SurfaceHit {
                distance,
                vertex,
                weights: [w, u, v],
                back_face: material.is_some() && !front,
                blended: material.is_some_and(|material| material.is_blended()),
            };
            if hit.precedes_triangle(best) {
                best = Some(hit);
                distance
            } else {
                closest
            }
        };
        if !pose.is_deformed()
            && let Mesh3D::Imported(mesh) = &self.mesh
            && triangles.len() >= MIN_BVH_TRIANGLES
        {
            let tree = mesh.pick_tree();
            tree.visit(local_origin, world_step, min, max, |triangle, closest| {
                let part = &mesh.parts[triangle.part as usize];
                test(
                    triangle.vertex as usize,
                    Some(&mesh.materials[part.material]),
                    closest,
                )
            });
        } else {
            let mut closest = max;
            for (range, material) in self.mesh.sections() {
                for vertex in range.step_by(3) {
                    closest = test(vertex, material, closest);
                }
            }
        }
        best
    }

    pub(super) fn bake_occludes(&self, ray: Ray3D, max: f32) -> bool {
        self.surface_hit(ray, 0.0, max, true).is_some()
    }

    pub(super) fn bake_hit(&self, ray: Ray3D, min: f32, max: f32) -> Option<(f32, BakedSurface)> {
        let hit = self.surface_hit(ray, min, max, true)?;
        let parts = self.mesh.parts();
        let part = parts.partition_point(|part| part.vertices.end <= hit.vertex);
        let material = parts
            .get(part)
            .and_then(|part| self.mesh.materials().get(part.material));
        self.bake_sample(hit.vertex, hit.weights, material, hit.back_face)
            .map(|sample| (hit.distance, sample))
    }

    /// Evaluate the same material UVs and world pose used by the opaque draw.
    #[doc(hidden)]
    pub fn bake_sample(
        &self,
        vertex: usize,
        weights: [f32; 3],
        material: Option<&MeshMaterial>,
        back_face: bool,
    ) -> Option<BakedSurface> {
        let triangle = *self.mesh.vertices().as_chunks::<3>().0.get(vertex / 3)?;
        let vertices = self.pick_pose().deform::<true>(vertex, triangle);
        let interpolate = |value: fn(&MeshVertex) -> [f32; 3]| {
            vertices
                .iter()
                .zip(weights)
                .fold(Vector3::zeros(), |sum, (vertex, weight)| {
                    sum + Vector3::from(value(vertex)) * weight
                })
        };
        let local = interpolate(|vertex| vertex.position);
        let position = self.transform.linear * local + self.transform.origin;
        let normal = (self.transform.inverse.transpose() * interpolate(|vertex| vertex.normal))
            .try_normalize(1e-12)?
            * if back_face { -1.0 } else { 1.0 };
        let interpolate_uv = |slot| {
            std::array::from_fn(|axis| {
                vertices
                    .iter()
                    .zip(weights)
                    .map(|(vertex, weight)| {
                        let uv = if slot == 0 { vertex.uv } else { vertex.uv1 };
                        uv[axis] * weight
                    })
                    .sum()
            })
        };
        let uv = interpolate_uv(0);
        let rgba = std::array::from_fn::<_, 4, _>(|channel| {
            vertices
                .iter()
                .zip(weights)
                .map(|(vertex, weight)| vertex.color[channel] * weight)
                .sum()
        });
        let [r, g, b, alpha] = rgba;
        let color = Vector3::new(r, g, b);
        let tint = Vector3::from(crate::scene_lighting::linear_color(self.color));
        let mut diffuse = color.component_mul(&tint);
        let mut emission = Vector3::zeros();
        if let Some(material) = material {
            if !material.depth_covers(uv, alpha, f32::from(self.color.0 as u8) / 255.0) {
                return None;
            }
            if !material.vertex_colors {
                diffuse = tint;
            }
            let [r, g, b, _] = material.base_color;
            diffuse.component_mul_assign(&Vector3::new(r, g, b));
            if let Some(texture) = &material.base_texture {
                diffuse.component_mul_assign(&Vector3::from(
                    texture
                        .channel_color(uv, material.channels.channel(MaterialChannel::BaseColor)),
                ));
            }
            if let Some(pbr) = &material.pbr {
                if pbr.workflow == MeshWorkflow::SpecularGlossiness {
                    let mut specular = [1.0; 3];
                    let mut weight = material.vertex_masks.specular(rgba);
                    if let Some(dielectric) = &pbr.dielectric {
                        specular = dielectric.color;
                        weight *= dielectric.specular;
                        if let Some(map) = &dielectric.color_texture {
                            let tint = map.channel_color(
                                interpolate_uv(map.uv_slot),
                                material.channels.channel(MaterialChannel::SpecularColor),
                            );
                            specular = std::array::from_fn(|index| specular[index] * tint[index]);
                        }
                        if let Some(map) = &dielectric.specular_texture {
                            weight *= map.scalar_at(
                                interpolate_uv(map.uv_slot),
                                material.channels.channel(MaterialChannel::Specular),
                            );
                        }
                    }
                    diffuse *=
                        1.0 - specular.into_iter().fold(0.0, f32::max).clamp(0.0, 1.0) * weight;
                } else {
                    let metallic = pbr.metallic
                        * material.vertex_masks.metalness(rgba)
                        * pbr.surface_texture.as_ref().map_or(1.0, |map| {
                            map.scalar_at(
                                interpolate_uv(map.uv_slot),
                                material.channels.channel(MaterialChannel::Metalness),
                            )
                        });
                    diffuse *= 1.0 - metallic.clamp(0.0, 1.0);
                }
                emission = Vector3::from(pbr.emissive);
                if let Some(map) = &pbr.emissive_texture {
                    emission.component_mul_assign(&Vector3::from(map.channel_color(
                        interpolate_uv(map.uv_slot),
                        material.channels.channel(MaterialChannel::Emissive),
                    )));
                }
            } else {
                emission = diffuse;
                diffuse = Vector3::zeros();
            }
        }
        Some(BakedSurface {
            position: position.into(),
            normal: normal.into(),
            diffuse: diffuse.map(|channel| channel.clamp(0.0, 1.0)).into(),
            emission: emission.into(),
            layers: self.layers,
            receives_shadow: self.receives_shadow,
            lighting_index: self.lighting_index,
        })
    }

    fn pick_pose(&self) -> PickPose<'_> {
        PickPose {
            skin: self.skin.as_deref().zip(self.mesh.influences()),
            morph: self.morph.as_deref().zip(self.mesh.morph_geometry()),
        }
    }

    fn describe_hit(&self, ray: Ray3D, surface: SurfaceHit) -> RayHit<'_> {
        let position = ray.origin + surface.distance * ray.direction;
        self.describe_surface(position, surface)
    }

    fn describe_surface(&self, position: Vector3<f32>, surface: SurfaceHit) -> RayHit<'_> {
        let base = self.mesh.vertices().as_chunks::<3>().0[surface.vertex / 3];
        let [a, b, c] = self.pick_pose().deform::<true>(surface.vertex, base);
        let [w, u, v] = surface.weights;
        let local_position = Vector3::from(a.position) * w
            + Vector3::from(b.position) * u
            + Vector3::from(c.position) * v;
        let interpolated =
            Vector3::from(a.normal) * w + Vector3::from(b.normal) * u + Vector3::from(c.normal) * v;
        // Mesh normals can cancel at a seam. Use the face's oriented normal in
        // that case so a surface hit always has a usable unit direction.
        let local_normal = interpolated.try_normalize(1e-12).unwrap_or_else(|| {
            let a = Vector3::from(a.position);
            (Vector3::from(b.position) - a)
                .cross(&(Vector3::from(c.position) - a))
                .normalize()
        });
        let sign = if surface.back_face { -1.0 } else { 1.0 };
        let world_normal = (self.transform.inverse.transpose() * local_normal).normalize();
        RayHit {
            object_id: &self.id,
            row_index: self.source_row.map(|row| row.get() - 1),
            imported_placement: self.imported_placement,
            distance: surface.distance,
            position: position.into(),
            normal: (sign * world_normal).into(),
            local_position: local_position.into(),
            local_normal: (sign * local_normal).into(),
            uv: std::array::from_fn(|axis| a.uv[axis] * w + b.uv[axis] * u + c.uv[axis] * v),
        }
    }
}

struct PickPose<'a> {
    skin: Option<(&'a SkinPose, &'a [MeshInfluence])>,
    morph: Option<(&'a MorphPose, &'a MorphGeometry)>,
}

impl PickPose<'_> {
    fn is_deformed(&self) -> bool {
        self.skin.is_some() || self.morph.is_some()
    }

    fn deform<const NORMALS: bool>(
        &self,
        vertex: usize,
        mut triangle: [MeshVertex; 3],
    ) -> [MeshVertex; 3] {
        if let Some((pose, geometry)) = self.morph {
            for (i, point) in triangle.iter_mut().enumerate() {
                *point = geometry.deform::<NORMALS>(vertex + i, &pose.weights, *point);
            }
        }
        if let Some((pose, weights)) = self.skin {
            for (i, point) in triangle.iter_mut().enumerate() {
                *point = pose.deform::<NORMALS>(weights[vertex + i], *point);
            }
        }
        triangle
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

/// A displacement or direction in 3D scene coordinates.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vector3D {
    pub x: f64,
    pub y: f64,
    pub z: f64,
}

impl Vector3D {
    pub const fn new(x: f64, y: f64, z: f64) -> Self {
        Self { x, y, z }
    }

    fn ray_direction(self) -> Option<[f32; 3]> {
        let direction = [self.x, self.y, self.z];
        if !direction.into_iter().all(f64::is_finite) {
            return None;
        }
        let scale = direction.into_iter().map(f64::abs).fold(0.0, f64::max);
        (scale > 0.0).then(|| direction.map(|value| (value / scale) as f32))
    }
}

/// A frozen description of one instance's resolved placement and material inputs.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Instance3D(Option<Arc<Instance3DData>>);

impl Instance3D {
    pub(crate) fn from_object(object: &SceneObject, address: [i64; 3]) -> Self {
        Self::from_placement(
            object.id.clone(),
            address,
            object.color,
            object.transform,
            object.instance_data(),
        )
    }

    pub(crate) fn from_placement(
        object_id: String,
        [row_index, mesh_node_index, instance_index]: [i64; 3],
        color: Color,
        transform: SceneTransform,
        data: [f32; 4],
    ) -> Self {
        let [x, y, z] = transform.origin.map(f64::from).into();
        let axis = |column: usize| {
            let column = transform.linear.column(column);
            Vector3D::new(
                f64::from(column.x),
                f64::from(column.y),
                f64::from(column.z),
            )
        };
        let [data_x, data_y, data_z, data_w] = data.map(f64::from);
        Self(Some(Arc::new(Instance3DData {
            valid: true,
            object_id,
            row_index,
            mesh_node_index,
            instance_index,
            color,
            origin: Point3D::new(x, y, z),
            axis_x: axis(0),
            axis_y: axis(1),
            axis_z: axis(2),
            data_x,
            data_y,
            data_z,
            data_w,
        })))
    }
}

impl std::ops::Deref for Instance3D {
    type Target = Instance3DData;

    fn deref(&self) -> &Self::Target {
        static MISSING: Instance3DData = Instance3DData {
            valid: false,
            object_id: String::new(),
            row_index: -1,
            mesh_node_index: -1,
            instance_index: -1,
            color: Color(0),
            origin: Point3D::new(0.0, 0.0, 0.0),
            axis_x: Vector3D::new(0.0, 0.0, 0.0),
            axis_y: Vector3D::new(0.0, 0.0, 0.0),
            axis_z: Vector3D::new(0.0, 0.0, 0.0),
            data_x: 0.0,
            data_y: 0.0,
            data_z: 0.0,
            data_w: 0.0,
        };
        self.0.as_deref().unwrap_or(&MISSING)
    }
}

#[doc(hidden)]
#[derive(Debug, PartialEq)]
pub struct Instance3DData {
    pub valid: bool,
    pub object_id: String,
    pub row_index: i64,
    pub mesh_node_index: i64,
    pub instance_index: i64,
    pub color: Color,
    pub origin: Point3D,
    pub axis_x: Vector3D,
    pub axis_y: Vector3D,
    pub axis_z: Vector3D,
    pub data_x: f64,
    pub data_y: f64,
    pub data_z: f64,
    pub data_w: f64,
}

/// The result of a scene surface query. A miss has zero coordinates.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Hit3D(Option<Arc<Hit3DData>>);

impl Hit3D {
    #[doc(hidden)]
    pub fn from_data(data: Hit3DData) -> Self {
        Self(data.valid.then(|| Arc::new(data)))
    }
}

impl std::ops::Deref for Hit3D {
    type Target = Hit3DData;

    fn deref(&self) -> &Self::Target {
        static MISS: Hit3DData = Hit3DData {
            valid: false,
            entering: false,
            object_id: String::new(),
            row_index: -1,
            mesh_node_index: -1,
            instance_index: -1,
            distance: 0.0,
            position: Point3D::new(0.0, 0.0, 0.0),
            normal: Vector3D::new(0.0, 0.0, 0.0),
            local_position: Point3D::new(0.0, 0.0, 0.0),
            local_normal: Vector3D::new(0.0, 0.0, 0.0),
            u: 0.0,
            v: 0.0,
        };
        self.0.as_deref().unwrap_or(&MISS)
    }
}

#[doc(hidden)]
#[derive(Debug, PartialEq)]
pub struct Hit3DData {
    pub valid: bool,
    pub entering: bool,
    pub object_id: String,
    pub row_index: i64,
    pub mesh_node_index: i64,
    pub instance_index: i64,
    pub distance: f64,
    pub position: Point3D,
    pub normal: Vector3D,
    pub local_position: Point3D,
    pub local_normal: Vector3D,
    pub u: f64,
    pub v: f64,
}

fn mesh_node_index(node: u32) -> i64 {
    if node == NO_MESH_NODE {
        -1
    } else {
        i64::from(node)
    }
}

impl From<RayHit<'_>> for Hit3D {
    fn from(hit: RayHit<'_>) -> Self {
        let [x, y, z] = hit.position.map(f64::from);
        let [nx, ny, nz] = hit.normal.map(f64::from);
        let [lx, ly, lz] = hit.local_position.map(f64::from);
        let [lnx, lny, lnz] = hit.local_normal.map(f64::from);
        Self::from_data(Hit3DData {
            valid: true,
            entering: false,
            object_id: hit.object_id.to_owned(),
            row_index: hit.row_index.map_or(-1, i64::from),
            mesh_node_index: hit
                .imported_placement
                .map_or(-1, |(node, _)| mesh_node_index(node)),
            instance_index: hit
                .imported_placement
                .and_then(|(_, instance)| instance)
                .map_or(-1, i64::from),
            distance: f64::from(hit.distance),
            position: Point3D::new(x, y, z),
            normal: Vector3D::new(nx, ny, nz),
            local_position: Point3D::new(lx, ly, lz),
            local_normal: Vector3D::new(lnx, lny, lnz),
            u: f64::from(hit.uv[0]),
            v: f64::from(hit.uv[1]),
        })
    }
}

#[doc(hidden)]
pub fn ray_hit(
    objects: &[SceneObject],
    origin: Point3D,
    direction: Vector3D,
    max_distance: f64,
    layers: Option<i64>,
) -> Hit3D {
    world_query(origin, direction, max_distance, layers)
        .and_then(|query| trace_surfaces(objects, query))
        .map_or_else(Hit3D::default, Hit3D::from)
}

#[doc(hidden)]
pub fn ray_hit_model(
    model: &SceneModel,
    origin: Point3D,
    direction: Vector3D,
    max_distance: f64,
    layers: Option<i64>,
) -> Hit3D {
    world_query(origin, direction, max_distance, layers).map_or_else(Hit3D::default, |query| {
        query.first(model.query_geometry(), &RayRows::All)
    })
}

#[doc(hidden)]
pub fn ray_hit_all(
    model: &SceneModel,
    origin: Point3D,
    direction: Vector3D,
    max_distance: f64,
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    world_query(origin, direction, max_distance, layers).map_or_else(VecModel::default, |query| {
        query.all(model.query_geometry(), &RayRows::All)
    })
}

#[doc(hidden)]
pub fn ray_hit_row(
    model: &SceneModel,
    origin: Point3D,
    direction: Vector3D,
    max_distance: f64,
    row: i64,
    layers: Option<i64>,
) -> Hit3D {
    world_query(origin, direction, max_distance, layers).map_or_else(Hit3D::default, |query| {
        query.first(model.query_geometry(), &RayRows::Row(row))
    })
}

/// Trace a ray against one placement in an imported glTF instance table.
#[doc(hidden)]
pub fn ray_hit_instance(
    model: &SceneModel,
    origin: Point3D,
    direction: Vector3D,
    max_distance: f64,
    address: [i64; 3],
    layers: Option<i64>,
) -> Hit3D {
    let Some(rows) = RayRows::instance(address) else {
        return Hit3D::default();
    };
    world_query(origin, direction, max_distance, layers).map_or_else(Hit3D::default, |query| {
        query.first(model.query_geometry(), &rows)
    })
}

#[doc(hidden)]
pub fn ray_hit_subset(
    model: &SceneModel,
    origin: Point3D,
    direction: Vector3D,
    max_distance: f64,
    rows: &[i64],
    layers: Option<i64>,
) -> VecModel<Hit3D> {
    world_query(origin, direction, max_distance, layers).map_or_else(VecModel::default, |query| {
        query.all(model.query_geometry(), &RayRows::subset(rows))
    })
}

fn world_query(
    origin: Point3D,
    direction: Vector3D,
    max_distance: f64,
    layers: Option<i64>,
) -> Option<SceneRay> {
    let (ray, max) = world_ray(origin, direction, max_distance)?;
    Some(SceneRay::new(
        ray,
        0.0,
        max,
        layers.map_or(MAIN_LAYER, |mask| mask as u64),
    ))
}

/// Find the nearest posed triangle surface within a radius in world units.
#[doc(hidden)]
pub fn closest_point(
    model: &SceneModel,
    origin: Point3D,
    radius: f64,
    layers: Option<i64>,
) -> Hit3D {
    nearest_point(model, origin, radius, layers, None)
}

/// Find the nearest surface on the scene rows with the selected object ID.
#[doc(hidden)]
pub fn closest_point_on(
    model: &SceneModel,
    object_id: &str,
    origin: Point3D,
    radius: f64,
    layers: Option<i64>,
) -> Hit3D {
    nearest_point(model, origin, radius, layers, Some(object_id))
}

fn nearest_point(
    model: &SceneModel,
    origin: Point3D,
    radius: f64,
    layers: Option<i64>,
    object_id: Option<&str>,
) -> Hit3D {
    let layers = layers.map_or(MAIN_LAYER, |mask| mask as u64);
    if layers == 0 || radius < 0.0 || !(radius as f32).is_finite() || origin.narrow().is_none() {
        return Hit3D::default();
    }
    let point = Vector3::new(origin.x, origin.y, origin.z);
    let mut best: Option<(usize, ClosestSurface)> = None;
    model.visit_nearest(point, radius * radius, |index, limit| {
        let object = &model[index];
        if object.pickable
            && object.layers & layers != 0
            && object_id.is_none_or(|id| id == object.id)
            && let Some(found) = object.closest_surface(point, limit)
            && (found.distance_squared < limit || best.is_none_or(|(previous, _)| index < previous))
        {
            best = Some((index, found));
            found.distance_squared
        } else {
            limit
        }
    });
    best.map_or_else(Hit3D::default, |(index, hit)| {
        model[index]
            .describe_surface(
                hit.position,
                SurfaceHit {
                    distance: hit.distance_squared.sqrt() as f32,
                    vertex: hit.vertex,
                    weights: hit.weights.map(|weight| weight as f32),
                    back_face: false,
                    blended: false,
                },
            )
            .into()
    })
}

#[derive(Clone, Copy)]
struct ClosestSurface {
    vertex: usize,
    weights: [f64; 3],
    distance_squared: f64,
    position: Vector3<f32>,
}

fn world_ray(origin: Point3D, direction: Vector3D, max_distance: f64) -> Option<(Ray3D, f32)> {
    let max = max_distance as f32;
    if !max.is_finite() || max_distance < 0.0 {
        return None;
    }
    let ray = Ray3D::new(origin.narrow()?, direction.ray_direction()?)?;
    Some((ray, max))
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct BakedSurface {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub diffuse: [f32; 3],
    pub emission: [f32; 3],
    pub layers: u64,
    pub receives_shadow: bool,
    pub lighting_index: u16,
}

pub(crate) fn bake_opaque(material: &MeshMaterial) -> bool {
    material.depth_coverage().is_some() && material.transmission_blur().is_none()
}

/// A world-space ray with a unit-length direction.
#[derive(Clone, Copy, Debug)]
pub struct Ray3D {
    origin: Vector3<f32>,
    pub(crate) direction: Vector3<f32>,
}

impl Ray3D {
    pub(crate) fn origin(self) -> Vector3<f32> {
        self.origin
    }

    pub fn new(origin: [f32; 3], direction: [f32; 3]) -> Option<Self> {
        let length = Vector3::from(direction).norm();
        if !origin.iter().all(|value| value.is_finite()) || !length.is_finite() || length == 0.0 {
            return None;
        }
        Some(Self {
            origin: origin.into(),
            direction: Vector3::from(direction) / length,
        })
    }
}

#[derive(Debug)]
pub struct RayHit<'a> {
    pub object_id: &'a str,
    pub row_index: Option<u32>,
    pub imported_placement: Option<(u32, Option<u32>)>,
    pub distance: f32,
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub local_position: [f32; 3],
    pub local_normal: [f32; 3],
    pub uv: [f32; 2],
}

#[derive(Clone, Copy)]
struct SurfaceHit {
    distance: f32,
    vertex: usize,
    weights: [f32; 3],
    back_face: bool,
    blended: bool,
}

impl SurfaceHit {
    fn precedes(self, previous: Option<Self>) -> bool {
        // Opaque depth wins a tie. Coplanar blend parts compose in source order.
        previous.is_none_or(|previous| {
            self.distance < previous.distance
                || (self.distance == previous.distance && previous.blended)
        })
    }

    fn precedes_triangle(self, previous: Option<Self>) -> bool {
        previous.is_none_or(|previous| {
            if self.distance != previous.distance {
                self.distance < previous.distance
            } else if self.blended != previous.blended {
                !self.blended
            } else if self.blended {
                self.vertex > previous.vertex
            } else {
                self.vertex < previous.vertex
            }
        })
    }
}

/// Find the nearest visible surface along a world-space ray.
/// Empty IDs can still occlude other surfaces.
pub fn raycast(objects: &[SceneObject], ray: Ray3D, max_distance: f32) -> Option<RayHit<'_>> {
    if !max_distance.is_finite() || max_distance < 0.0 {
        return None;
    }
    trace_surfaces(objects, SceneRay::new(ray, 0.0, max_distance, u64::MAX))
}

/// Return the nearest surface of each intersected object, sorted from the ray origin.
pub fn raycast_all(objects: &[SceneObject], ray: Ray3D, max_distance: f32) -> Vec<RayHit<'_>> {
    if !max_distance.is_finite() || max_distance < 0.0 {
        return Vec::new();
    }
    sorted_surfaces(objects, ray, max_distance)
        .into_iter()
        .map(|(index, surface)| objects[index].describe_hit(ray, surface))
        .collect()
}

#[derive(Clone, Copy)]
pub(crate) struct SceneRay {
    ray: Ray3D,
    min: f32,
    max: f32,
    layers: u64,
    eye: Option<[f32; 3]>,
}

pub(crate) enum RayRows {
    All,
    Row(i64),
    Subset(Vec<u32>),
    Instance([i64; 3]),
}

impl RayRows {
    pub(crate) fn subset(rows: &[i64]) -> Self {
        let mut rows: Vec<_> = rows
            .iter()
            .copied()
            .filter_map(|row| u32::try_from(row).ok())
            .collect();
        rows.sort_unstable();
        rows.dedup();
        Self::Subset(rows)
    }

    pub(crate) fn instance([row, node, index]: [i64; 3]) -> Option<Self> {
        u32::try_from(row).ok()?;
        if node != -1 {
            u32::try_from(node).ok()?;
        }
        u32::try_from(index).ok()?;
        Some(Self::Instance([row, node, index]))
    }

    fn is_empty(&self) -> bool {
        match self {
            Self::All | Self::Instance(_) => false,
            Self::Row(row) => u32::try_from(*row).is_err(),
            Self::Subset(rows) => rows.is_empty(),
        }
    }

    fn contains(&self, object: &SceneObject) -> bool {
        let row = object.scene_row();
        match self {
            Self::All => true,
            Self::Row(selected) => row.is_some_and(|row| i64::from(row) == *selected),
            Self::Subset(rows) => row.is_some_and(|row| rows.binary_search(&row).is_ok()),
            Self::Instance(address) => object.instance_address() == Some(*address),
        }
    }
}

/// The rendered face and texture coordinates used for forwarding 2D input.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct SceneTextureHit {
    pub source: Arc<str>,
    pub row: Option<u32>,
    pub id: String,
    pub placement: Option<(u32, Option<u32>)>,
    pub uv: [f32; 2],
    pub tile: [i64; 2],
}

impl SceneRay {
    pub(crate) fn new(ray: Ray3D, min: f32, max: f32, layers: u64) -> Self {
        Self {
            ray,
            min,
            max,
            layers,
            eye: None,
        }
    }

    pub(crate) fn with_camera(mut self, eye: [f32; 3]) -> Self {
        self.eye = Some(eye);
        self
    }

    fn includes(self, object: &SceneObject) -> bool {
        object.layers & self.layers != 0
            && self.eye.is_none_or(|eye| object.visible_at_distance(eye))
    }

    fn visit(
        self,
        geometry: &SceneGeometry,
        rows: &RayRows,
        intersect: impl Fn(&SceneObject, f32) -> Option<SurfaceHit>,
        mut visit: impl FnMut((usize, SurfaceHit), f64) -> f64,
    ) {
        if self.layers == 0 || rows.is_empty() {
            return;
        }
        let span = BoundsSegment {
            origin: self.ray.origin.cast(),
            direction: self.ray.direction.cast(),
            min: f64::from(self.min),
            max: f64::from(self.max),
            expansion: Vector3::zeros(),
        };
        geometry.visit_ray(span, |index, limit| {
            let object = &geometry[index];
            if self.includes(object)
                && rows.contains(object)
                && let Some(hit) = intersect(object, limit as f32)
            {
                visit((index, hit), limit)
            } else {
                limit
            }
        });
    }

    fn nearest(self, geometry: &SceneGeometry, rows: &RayRows) -> Option<(usize, SurfaceHit)> {
        if let RayRows::Instance(address) = rows {
            if self.layers == 0 {
                return None;
            }
            let index = geometry.instance_index(*address)?;
            let object = &geometry[index];
            return self
                .includes(object)
                .then(|| object.hit(self.ray, self.min, self.max))
                .flatten()
                .map(|hit| (index, hit));
        }
        self.nearest_matching(geometry, rows, |object, max| {
            object.hit(self.ray, self.min, max)
        })
    }

    fn nearest_matching(
        self,
        geometry: &SceneGeometry,
        rows: &RayRows,
        intersect: impl Fn(&SceneObject, f32) -> Option<SurfaceHit>,
    ) -> Option<(usize, SurfaceHit)> {
        let mut nearest = None;
        self.visit(geometry, rows, intersect, |hit, limit| {
            if nearest.is_none_or(|prior| surface_order(&hit, &prior).is_lt()) {
                nearest = Some(hit);
                f64::from(hit.1.distance)
            } else {
                limit
            }
        });
        nearest
    }

    pub(crate) fn item_texture(self, geometry: &SceneGeometry) -> Option<Option<SceneTextureHit>> {
        let (index, hit) = self.nearest_matching(geometry, &RayRows::All, |object, max| {
            object.surface_hit(self.ray, self.min, max, false)
        })?;
        let object = &geometry[index];
        let parts = object.mesh.parts();
        let index = parts.partition_point(|part| part.vertices.end <= hit.vertex);
        let material = parts
            .get(index)
            .and_then(|part| object.mesh.materials().get(part.material));
        Some(material.and_then(|material| {
            let item = material.base_item.as_ref().filter(|_| object.pickable)?;
            let tri = &object.mesh.vertices()[hit.vertex..hit.vertex + 3];
            let uv = std::array::from_fn(|axis| {
                tri[0].uv[axis] * hit.weights[0]
                    + tri[1].uv[axis] * hit.weights[1]
                    + tri[2].uv[axis] * hit.weights[2]
            });
            let (uv, tile) = item.project(uv)?;
            Some(SceneTextureHit {
                source: item.source_key(),
                row: object.scene_row(),
                id: object.id.clone(),
                placement: object.imported_placement,
                uv,
                tile,
            })
        }))
    }

    pub(crate) fn id(self, geometry: &SceneGeometry) -> String {
        self.nearest(geometry, &RayRows::All)
            .map_or_else(String::new, |(index, _)| geometry[index].id.clone())
    }

    pub(crate) fn first(self, geometry: &SceneGeometry, rows: &RayRows) -> Hit3D {
        self.nearest(geometry, rows)
            .map_or_else(Hit3D::default, |(index, hit)| {
                geometry[index].describe_hit(self.ray, hit).into()
            })
    }

    pub(crate) fn all(self, geometry: &SceneGeometry, rows: &RayRows) -> VecModel<Hit3D> {
        let mut hits = Vec::new();
        self.visit(
            geometry,
            rows,
            |object, max| object.hit(self.ray, self.min, max),
            |hit, limit| {
                hits.push(hit);
                limit
            },
        );
        hits.sort_unstable_by(surface_order);
        // Ray traversal order is independent of authored order. By reducing the
        // sorted hits, keep one ordinary surface per row and one per
        // glTF instance address. The source node disambiguates instance tables.
        let mut seen = std::collections::HashSet::new();
        let hits: Vec<_> = hits
            .into_iter()
            .filter_map(|(index, hit)| {
                let object = &geometry[index];
                object
                    .scene_row()
                    .is_none_or(|row| {
                        let placement = object
                            .imported_placement()
                            .and_then(|(node, instance)| instance.map(|index| (node, index)));
                        seen.insert((row, placement))
                    })
                    .then(|| Hit3D::from(object.describe_hit(self.ray, hit)))
            })
            .collect();
        hits.into()
    }
}

fn surface_order(a: &(usize, SurfaceHit), b: &(usize, SurfaceHit)) -> std::cmp::Ordering {
    let (a_index, a) = a;
    let (b_index, b) = b;
    if a.distance != b.distance {
        a.distance.total_cmp(&b.distance)
    } else {
        a.blended.cmp(&b.blended).then_with(|| {
            if a.blended {
                b_index.cmp(a_index)
            } else {
                a_index.cmp(b_index)
            }
        })
    }
}

fn sorted_surfaces(objects: &[SceneObject], ray: Ray3D, max: f32) -> Vec<(usize, SurfaceHit)> {
    let mut hits: Vec<_> = objects
        .iter()
        .enumerate()
        .filter_map(|(index, object)| {
            // Keep layer-zero objects out of the public slice queries.
            (object.layers != 0)
                .then(|| object.hit(ray, 0.0, max))
                .flatten()
                .map(|hit| (index, hit))
        })
        .collect();
    hits.sort_unstable_by(surface_order);
    hits
}

pub(crate) fn trace_surfaces(objects: &[SceneObject], query: SceneRay) -> Option<RayHit<'_>> {
    let (object, surface) = nearest_surface(objects, query)?;
    Some(object.describe_hit(query.ray, surface))
}

pub(crate) fn trace_id(objects: &[SceneObject], query: SceneRay) -> Option<&str> {
    nearest_surface(objects, query).map(|(object, _)| object.id.as_str())
}

fn nearest_surface(objects: &[SceneObject], query: SceneRay) -> Option<(&SceneObject, SurfaceHit)> {
    let mut best: Option<(&SceneObject, SurfaceHit)> = None;
    let mut closest = query.max;
    for item in objects {
        if query.includes(item)
            && let Some(hit) = item.hit(query.ray, query.min, closest)
            && hit.precedes(best.map(|(_, surface)| surface))
        {
            closest = hit.distance;
            best = Some((item, hit));
        }
    }
    best
}

fn overlaps_bounds(
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    (min, max): ([f32; 3], [f32; 3]),
    mut near: f32,
    mut far: f32,
) -> bool {
    for axis in 0..3 {
        if direction[axis].abs() < 1e-10 {
            if origin[axis] < min[axis] || origin[axis] > max[axis] {
                return false;
            }
            continue;
        }
        let a = (min[axis] - origin[axis]) / direction[axis];
        let b = (max[axis] - origin[axis]) / direction[axis];
        near = near.max(a.min(b));
        far = far.min(a.max(b));
        if near > far {
            return false;
        }
    }
    true
}

fn intersect_triangle(
    origin: Vector3<f32>,
    direction: Vector3<f32>,
    [a, b, c]: [&MeshVertex; 3],
) -> Option<(f32, f32, f32, bool)> {
    let a = Vector3::from(a.position);
    let ab = Vector3::from(b.position) - a;
    let ac = Vector3::from(c.position) - a;
    let p = direction.cross(&ac);
    let det = ab.dot(&p);
    if det.abs() < 1e-8 {
        return None;
    }
    let delta = origin - a;
    let u = delta.dot(&p) / det;
    if !(0.0..=1.0).contains(&u) {
        return None;
    }
    let q = delta.cross(&ab);
    let v = direction.dot(&q) / det;
    if v < 0.0 || u + v > 1.0 {
        return None;
    }
    Some((ac.dot(&q) / det, u, v, det > 0.0))
}

pub fn direction(yaw: f64, elevation: f64) -> [f32; 3] {
    let yaw = yaw.rem_euclid(360.0).to_radians() as f32;
    let elevation = elevation.clamp(-90.0, 90.0).to_radians() as f32;
    [
        yaw.sin() * elevation.cos(),
        elevation.sin(),
        yaw.cos() * elevation.cos(),
    ]
}
