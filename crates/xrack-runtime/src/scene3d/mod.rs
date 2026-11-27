//! Scene geometry, camera projection, and ray queries shared by 3D hosts.

pub(crate) mod bounds;
pub(crate) mod camera;
pub(crate) mod cast;
#[doc(hidden)]
pub mod effect;
pub(crate) mod environment;
pub(crate) mod fog;
pub(crate) mod hierarchy;
pub(crate) mod instance_table;
pub(crate) mod lighting;
pub(crate) mod lightmap;
#[doc(hidden)]
pub mod material_shader;
pub(crate) mod mesh;
mod object;
pub(crate) mod particles;
pub(crate) mod pick;
pub(crate) mod projection;
pub(crate) mod random_instances;
pub(crate) mod rotation;
pub(crate) mod sky;
pub(crate) mod surface;
pub(crate) mod visibility;

pub use crate::scene3d::camera::{
    CAMERA_MATRIX_FIELDS, Point3D, ProjectedPoint3D, SceneCamera, SceneCameraSettings,
    hit_all_camera, hit_all_objects, hit_camera, hit_instance, hit_instance_camera, hit_model,
    hit_objects, hit_row, hit_row_camera, hit_subset, hit_subset_camera, pick_camera, pick_in_rect,
    pick_in_rect_camera, pick_model, pick_objects, project, project_camera, unproject,
    unproject_camera,
};
pub use crate::scene3d::cast::{
    WalkOptions, WalkResult3D, capsule_cast, capsule_overlap, capsule_slide,
    capsule_trigger_crossings, capsule_trigger_overlap, capsule_walk, capsule_walk_result,
    capsule_walk_with_contacts, sphere_cast, sphere_overlap, sphere_slide, sphere_walk,
};
pub use crate::scene3d::environment::{
    EnvironmentOrientation, EnvironmentProbe, EnvironmentSource, ReflectionCapture,
    ReflectionProbe, ReflectionRefresh, ReflectionSlicing, ReflectionSource, ViewportEnvironment,
    environment_resource_key, model_reflections, record_reflections,
};
pub use crate::scene3d::fog::{
    FogNoise, FogVolume3D, FogVolumeDefinition, MAX_FOG_VOLUMES, model_fog_volumes,
    record_fog_volumes, visible_fog_volumes,
};
use crate::scene3d::hierarchy::{NO_MESH_NODE, SceneTransform};
pub use crate::scene3d::hierarchy::{
    SceneCollisionShape, SceneGeometry, SceneGeometryInput, SceneModel, SceneNodeDefinition,
    ScenePreloadCursor, ScenePreloadStep, model_cameras, model_nodes, prepare_collision_shapes,
    record_cameras, record_objects,
};
pub use crate::scene3d::instance_table::{InstanceTable3D, InstanceTableRow};
pub use crate::scene3d::lighting::{
    DirectionalShadowSpace, LightBakeMode, MAX_SCENE_LIGHTS, PointShadowProjection, SceneLight,
    SceneLightDefinition, SceneLightModel, SceneLightScope, SceneLightSource, ShadowFilter,
    ShadowLight, ShadowMapPrecision, ShadowProjectionSettings, ShadowRadius, ShadowSampling,
    linear_color, model_lights, record_lights, shadow_clip_corners, viewport_ambient,
};
pub use crate::scene3d::mesh::animation::{
    AnimatedMesh, MAX_ANIMATION_KEYS, MAX_ANIMATION_NODES, MAX_MESH_INSTANCES, MeshAnimationBlend,
    MeshAnimationClip, MeshAnimationLayer, MeshAnimationNode, MeshAnimationNodeOverride,
    MeshAnimationProperty, MeshAnimationTrack, MeshAsset, MeshCamera, MeshCameraProjection,
    MeshInstances, MeshInterpolation, MeshLight, MeshLods, MeshNodeTransform, MeshPlayback,
    MorphAnimationTrack, prepare_animation_layers, prepare_animation_node_overrides,
};
pub use crate::scene3d::mesh::cubemap::{CubeTexture3D, TextureAsset3D};
pub use crate::scene3d::mesh::indices::{MeshDrawIndices, MeshIndexElements};
pub use crate::scene3d::mesh::material::{
    MAX_MESH_TEXTURE_DIMENSION, MaterialChannel, MaterialChannels, MeshAlphaMode, MeshAnisotropy,
    MeshBlendMode, MeshClearcoat, MeshCullMode, MeshDepthCoverage, MeshDepthDraw, MeshDielectric,
    MeshFresnel, MeshHeightMap, MeshImage, MeshMaterial, MeshMaterialSlot, MeshNormalMap, MeshPart,
    MeshPbr, MeshShader, MeshSpecularReflection, MeshSurfaceMaps, MeshTexture, MeshTransmission,
    MeshVolume, MeshWorkflow, RefractionChannels, SceneItemTexture, SceneMaterial,
    SceneMaterialBinding, SceneMaterialMaps, SceneTexture, TextureChannel, TextureColorSpace,
    TextureFilter, TextureSampling, TextureTransform, TextureWrap, VertexColorMasks,
    texture_resource_key,
};
pub use crate::scene3d::mesh::morph::{
    MAX_MORPH_DELTAS, MAX_MORPH_TARGETS, MorphDelta, MorphGeometry, MorphWeights,
};
pub use crate::scene3d::mesh::nodes::MeshNodeCatalog;
pub use crate::scene3d::mesh::skin::{
    JointPalette, JointRows, MAX_SKIN_JOINTS, MeshInfluence, MeshSkin, MeshSkinJoint,
    SKIN_INFLUENCES, SceneSkinJoint, prepare_scene_skin_joints,
};
pub use crate::scene3d::particles::affectors::{
    MAX_PARTICLE_AFFECTORS, ParticleAffector, ParticleAffectorDefinition, model_particle_affectors,
    record_particle_affectors,
};
pub use crate::scene3d::particles::bursts::{ParticleBurstDefinition, ParticleBurstSchedule};
pub use crate::scene3d::particles::model_blend::{
    ModelBlend, ModelBlendDefinition, ModelBlendPose,
};
pub use crate::scene3d::particles::motion::ParticleMotion;
pub use crate::scene3d::particles::points::{MAX_PARTICLE_POINT_COUNT, ParticlePointCloud};
pub use crate::scene3d::particles::surface::ParticleMeshSurface;
pub use crate::scene3d::particles::track::{
    ParticleEmissionAnimation, ParticleEmissionTrack, ParticleLoopClock,
};
pub use crate::scene3d::particles::{
    MAX_PARTICLE_EMITTERS, MAX_PARTICLE_TEXTURE_BYTES, MAX_PARTICLES, ParticleBlend,
    ParticleEmitter, ParticleEmitterDefinition, ParticleFadeEffect, ParticleFollowPlan,
    ParticleInstanceModel, ParticleModelSource, ParticleSample, ParticleSortMode, ParticleSprite,
    TrailTexcoordMode, model_particles, record_particles,
};
use crate::scene3d::pick::BoundsSegment;
pub use crate::scene3d::projection::SceneProjectionCache;
pub use crate::scene3d::random_instances::{InstanceColorModel, SceneRandomInstances};
pub use crate::scene3d::sky::SkySettings;
pub use crate::scene3d::surface::{SurfaceSample, corresponding_vertices};
pub use crate::scene3d::visibility::{SceneVisibility, VisibilityIndex};

/// The default content layer for an authored model, light or camera.
pub use crate::scene3d::lighting::viewport_lighting;
pub use crate::scene3d::lightmap::{
    BakeRole, BakedLightmap, LightmapMode, LightmapResolution, LightmapTrace,
};

pub use crate::scene3d::mesh::authored::{
    SceneMeshIndices, SceneMeshInputs, SceneMeshSubset, SceneMeshVertex, SceneMorphDelta,
    SceneSkinWeight,
};
pub use crate::scene3d::mesh::parametric::SceneParametricGeometry;

pub use mesh::data::{
    CaptureMaterial, MAX_IMPORTED_VERTICES, Mesh3D, MeshAsset3D, MeshData, MeshGeometry,
    MeshHeightField, MeshImportOptions, MeshTangent, MeshVertex, MeshVertexStreams,
    mesh_resource_key,
};
pub use mesh::text::{PreparedText, SceneTextFonts};
pub use mesh::texture_data::{TextureData2D, TextureData3D, VolumeImage3D};
pub(crate) use object::InstancingLodRange;
pub use object::{InstanceOrder, SceneObject};

pub const MAIN_LAYER: u64 = 1;
use crate::{Color, VecModel};
use nalgebra::{Matrix2, Vector2, Vector3};
use std::{collections::BTreeMap, sync::Arc};

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
            item: None,
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
    pub item: Option<Box<Hit3DItem>>,
    pub u: f64,
    pub v: f64,
}

#[doc(hidden)]
#[derive(Debug, PartialEq)]
pub struct Hit3DItem {
    source: Arc<str>,
    position: crate::Point2D,
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
        Self::from_surface(hit, None)
    }
}

impl Hit3D {
    pub fn item_source(&self) -> &str {
        self.item.as_ref().map_or("", |item| item.source.as_ref())
    }

    pub fn item_position(&self) -> crate::Point2D {
        self.item
            .as_ref()
            .map_or_else(crate::Point2D::default, |item| item.position)
    }

    fn from_surface(hit: RayHit<'_>, item: Option<Box<Hit3DItem>>) -> Self {
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
            item,
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

pub(crate) fn world_query(
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
    if layers == 0 {
        return Hit3D::default();
    }
    nearest_point_in(model.query_geometry(), origin, radius, |object| {
        object.layers & layers != 0 && object_id.is_none_or(|id| id == object.id)
    })
}

pub(crate) fn nearest_point_in(
    geometry: &SceneGeometry,
    origin: Point3D,
    radius: f64,
    includes: impl Fn(&SceneObject) -> bool,
) -> Hit3D {
    if radius < 0.0 || !(radius as f32).is_finite() || origin.narrow().is_none() {
        return Hit3D::default();
    }
    let point = Vector3::new(origin.x, origin.y, origin.z);
    let mut best: Option<(usize, ClosestSurface)> = None;
    geometry.visit_nearest(point, radius * radius, |index, limit| {
        let object = &geometry[index];
        if object.is_pickable()
            && includes(object)
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
        geometry[index]
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

/// The mapping of a captured texture to the source's untransformed layout pixels.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SceneTextureMapping {
    pub bounds: [f32; 4],
    pub inverse: [f32; 6],
}

impl SceneTextureMapping {
    pub fn layout_point(self, uv: [f32; 2]) -> [f32; 2] {
        let [x, y, width, height] = self.bounds;
        let [u, v] = uv.map(|value| value.min(1.0 - f32::EPSILON));
        let [a, b, c, d, tx, ty] = self.inverse;
        let captured = Vector2::new(x + u * width, y + v * height);
        (Matrix2::new(a, c, b, d) * captured + Vector2::new(tx, ty)).into()
    }

    fn item_point(self, uv: [f32; 2]) -> Option<crate::Point2D> {
        let [x, y] = self.layout_point(uv);
        let [left, top, width, height] = self.bounds;
        let local = [x - left, y - top];
        ((0.0..width).contains(&local[0]) && (0.0..height).contains(&local[1])).then(|| {
            let [x, y] = local.map(f64::from);
            crate::Point2D::new(crate::Length(x), crate::Length(y))
        })
    }
}

#[doc(hidden)]
pub type SceneTextureSources = BTreeMap<Arc<str>, Option<SceneTextureMapping>>;

struct ItemSurfaceHit<'a> {
    object: &'a SceneObject,
    hit: SurfaceHit,
    item: &'a SceneItemTexture,
    uv: [f32; 2],
    tile: [i64; 2],
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
        self.item_surface(geometry).map(|surface| {
            surface.map(|surface| SceneTextureHit {
                source: surface.item.source_key(),
                row: surface.object.scene_row(),
                id: surface.object.id.clone(),
                placement: surface.object.imported_placement,
                uv: surface.uv,
                tile: surface.tile,
            })
        })
    }

    pub(crate) fn hit_item(
        self,
        geometry: &SceneGeometry,
        sources: &SceneTextureSources,
    ) -> Option<Hit3D> {
        let surface = self.item_surface(geometry)??;
        let source = surface.item.source();
        let mapping = sources.get(source)?.as_ref()?;
        Some(Hit3D::from_surface(
            surface.object.describe_hit(self.ray, surface.hit),
            Some(Box::new(Hit3DItem {
                source: surface.item.source_key(),
                position: mapping.item_point(surface.uv)?,
            })),
        ))
    }

    fn item_surface(self, geometry: &SceneGeometry) -> Option<Option<ItemSurfaceHit<'_>>> {
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
            let item = material
                .base_item
                .as_ref()
                .filter(|_| object.is_pickable())?;
            let tri = &object.mesh.vertices()[hit.vertex..hit.vertex + 3];
            let uv = std::array::from_fn(|axis| {
                tri[0].uv[axis] * hit.weights[0]
                    + tri[1].uv[axis] * hit.weights[1]
                    + tri[2].uv[axis] * hit.weights[2]
            });
            let (uv, tile) = item.project(uv)?;
            Some(ItemSurfaceHit {
                object,
                hit,
                item,
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

pub fn direction(yaw: f64, elevation: f64) -> [f32; 3] {
    let yaw = yaw.rem_euclid(360.0).to_radians() as f32;
    let elevation = elevation.clamp(-90.0, 90.0).to_radians() as f32;
    [
        yaw.sin() * elevation.cos(),
        elevation.sin(),
        yaw.cos() * elevation.cos(),
    ]
}
