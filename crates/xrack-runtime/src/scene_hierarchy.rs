//! Local-to-world scene transforms shared by both UI backends and ray queries.
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    ops::{Deref, Range},
    sync::{Arc, Mutex, OnceLock},
};

use nalgebra::{Matrix3, Matrix4, Quaternion, Rotation3, UnitQuaternion, Vector3};
use rapier3d::prelude::Pose as RigidPose;

use crate::{
    Color, Instance3D, MeshNode3D, Node, PhysicsWorld3D, Point2D, Point3D, Quaternion3D,
    RecordList, RecordValue, Value, VecModel, Vector3D,
    mesh_nodes::MeshNodeCatalog,
    scene_animation::{
        AnimatedMesh, MeshAnimationBlend, MeshAnimationLayer, MeshInstanceSpace, MeshPlacement,
        MeshPlayback, MeshPose, MeshSampleOptions, SampledMesh, matrix_orientation,
        prepare_animation_layers,
    },
    scene_authored_mesh::{SceneMeshIndices, SceneMeshInputs, SceneMeshSubset, SceneMeshVertex},
    scene_bounds::Bounds3D,
    scene_camera::{CameraMatrixCache, CameraNode, SceneCamera, SceneCameraSettings},
    scene_effect::CaptureRenderables,
    scene_instance_table::{InstancePlacements, InstanceTable3D},
    scene_lighting::{MAX_SCENE_LIGHTS, SceneLight, SceneLightScope},
    scene_lightmap::{BakeRole, BakedLightmap, LightmapResolution},
    scene_pick::BoundsSegment,
    scene_random_instances::{InstancePlacement, SceneRandomInstances},
    scene_visibility::{Frustum, INDEX_THRESHOLD, SceneVisibility, VisibilityIndex},
    scene3d::{
        CaptureMaterial, InstanceOrder, InstancingLodRange, MAIN_LAYER, Mesh3D, MeshAsset,
        SceneObject, SceneParametricGeometry,
    },
};

const MAX_NODES: usize = 4096;
pub(crate) const NO_MESH_NODE: u32 = u32::MAX;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SceneTransform {
    pub linear: Matrix3<f32>,
    pub origin: Vector3<f32>,
    pub inverse: Matrix3<f32>,
    pub reflected: bool,
}

impl SceneTransform {
    fn identity() -> Self {
        Self {
            linear: Matrix3::identity(),
            origin: Vector3::zeros(),
            inverse: Matrix3::identity(),
            reflected: false,
        }
    }

    pub(crate) fn local(
        position: [f32; 3],
        scale: [f32; 3],
        angles: [f32; 3],
        pivot: [f32; 3],
    ) -> Option<(Self, Rotation3<f32>)> {
        if !angles.into_iter().all(f32::is_finite) {
            return None;
        }
        Self::local_with_rotation(position, scale, pivot, euler_rotation(angles))
    }

    pub(crate) fn local_with_rotation(
        position: [f32; 3],
        scale: [f32; 3],
        pivot: [f32; 3],
        rotation: Rotation3<f32>,
    ) -> Option<(Self, Rotation3<f32>)> {
        if !position
            .into_iter()
            .chain(pivot)
            .all(|value| value.is_finite() && value.abs() <= 200.0)
            || !scale
                .into_iter()
                .all(|value| (0.0001..=200.0).contains(&value.abs()))
        {
            return None;
        }
        let mut linear = *rotation.matrix();
        let mut inverse = linear.transpose();
        let mut reflected = false;
        for (axis, scale) in scale.into_iter().enumerate() {
            linear.column_mut(axis).scale_mut(scale);
            inverse.row_mut(axis).unscale_mut(scale);
            reflected ^= scale < 0.0;
        }
        // The model-space pivot maps to the authored position.
        let origin = Vector3::from(position) - linear * Vector3::from(pivot);
        Some((
            Self {
                linear,
                origin,
                inverse,
                reflected,
            },
            rotation,
        ))
    }

    pub(crate) fn child(self, local: Self) -> Option<Self> {
        Self {
            linear: self.linear * local.linear,
            origin: self.linear * local.origin + self.origin,
            inverse: local.inverse * self.inverse,
            reflected: self.reflected ^ local.reflected,
        }
        .checked()
    }

    pub(crate) fn relative_to(self, root: Self) -> Option<Self> {
        Self {
            linear: root.inverse * self.linear,
            origin: root.inverse * (self.origin - root.origin),
            inverse: self.inverse * root.linear,
            reflected: self.reflected ^ root.reflected,
        }
        .checked()
    }

    pub(crate) fn from_instance_rows(rows: [[f32; 4]; 3]) -> Option<Self> {
        let linear = Matrix3::from_fn(|row, column| rows[row][column]);
        let inverse = linear.try_inverse()?;
        Self {
            linear,
            inverse,
            origin: Vector3::new(rows[0][3], rows[1][3], rows[2][3]),
            reflected: linear.determinant() < 0.0,
        }
        .checked()
    }

    fn matrix(self) -> Matrix4<f32> {
        let mut matrix = Matrix4::identity();
        matrix.fixed_view_mut::<3, 3>(0, 0).copy_from(&self.linear);
        matrix.fixed_view_mut::<3, 1>(0, 3).copy_from(&self.origin);
        matrix
    }

    fn checked(self) -> Option<Self> {
        // A strongly ill-conditioned transform has no reliable f32 inverse
        // for either rasterization or picking.
        self.linear
            .iter()
            .chain(self.origin.iter())
            .chain(self.inverse.iter())
            .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
            .then_some(self)
    }

    fn mesh_node(self, node: &Matrix4<f32>) -> Option<Self> {
        let linear = self.linear * node.fixed_view::<3, 3>(0, 0);
        let inverse = linear.try_inverse()?;
        Self {
            linear,
            origin: self.origin + self.linear * node.fixed_view::<3, 1>(0, 3),
            inverse,
            reflected: linear.determinant() < 0.0,
        }
        .checked()
    }

    pub(crate) fn rows(self) -> [[f32; 4]; 3] {
        std::array::from_fn(|row| {
            [
                self.linear[(row, 0)],
                self.linear[(row, 1)],
                self.linear[(row, 2)],
                self.origin[row],
            ]
        })
    }
}

pub(crate) fn euler_rotation(angles: [f32; 3]) -> Rotation3<f32> {
    let [yaw, pitch, roll] = angles.map(|angle| angle.rem_euclid(360.0).to_radians());
    if yaw == 0.0 && pitch == 0.0 && roll == 0.0 {
        Rotation3::identity()
    } else {
        Rotation3::from_axis_angle(&Vector3::z_axis(), roll)
            * Rotation3::from_axis_angle(&Vector3::y_axis(), yaw)
            * Rotation3::from_axis_angle(&Vector3::x_axis(), pitch)
    }
}

#[doc(hidden)]
pub enum SceneGeometryInput<'a> {
    Parametric(SceneParametricGeometry<'a>),
    Triangles(Option<Mesh3D>),
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct SceneCollisionShape {
    asset: MeshAsset,
    local: SceneTransform,
}

impl SceneCollisionShape {
    pub fn new(
        shape: &str,
        position: Point3D,
        size: Vector3D,
        angles: [f64; 3],
        mesh: &str,
        imported: impl FnOnce(&str) -> Option<MeshAsset>,
    ) -> Option<Self> {
        let extent = [size.x, size.y, size.z];
        let (asset, scale) = match shape {
            "box" => (MeshAsset::Static(Mesh3D::Box), extent),
            "sphere" => (
                MeshAsset::Static(Mesh3D::Sphere),
                extent.map(|value| value * 0.5),
            ),
            "cylinder" => (
                MeshAsset::Static(Mesh3D::builtin("primitive:cylinder")?),
                extent,
            ),
            "cone" => (
                MeshAsset::Static(Mesh3D::builtin("primitive:cone")?),
                extent,
            ),
            "capsule" => (
                MeshAsset::Static(
                    SceneParametricGeometry {
                        shape: "capsule",
                        radius: Some(size.x * 0.5),
                        height: Some(size.y),
                        ..SceneParametricGeometry::default()
                    }
                    .mesh()?,
                ),
                [1.0, 1.0, size.z / size.x],
            ),
            "mesh" if !mesh.is_empty() => (MeshAsset::resolve(mesh, imported)?, extent),
            _ => return None,
        };
        let (local, _) = SceneTransform::local(
            [position.x, position.y, position.z].map(|value| value as f32),
            scale.map(|value| value as f32),
            angles.map(|value| value as f32),
            [0.0; 3],
        )?;
        Some(Self { asset, local })
    }

    fn from_record(
        row: &RecordValue,
        imported: impl FnOnce(&str) -> Option<MeshAsset>,
    ) -> Option<Self> {
        let spec = CollisionShapeSpec::from_record(row)?;
        Self::new(
            spec.shape,
            spec.position,
            spec.size,
            spec.angles,
            spec.mesh,
            imported,
        )
    }
}

pub(crate) struct CollisionShapeSpec<'a> {
    pub shape: &'a str,
    pub position: Point3D,
    pub size: Vector3D,
    pub angles: [f64; 3],
    pub mesh: &'a str,
}

impl<'a> CollisionShapeSpec<'a> {
    pub(crate) fn from_record(row: &'a RecordValue) -> Option<Self> {
        let Value::String(shape) = row.field("shape")? else {
            return None;
        };
        let position = match row.field("position") {
            Some(Value::Point3D(position)) => *position,
            _ => Point3D::default(),
        };
        let size = match row.field("size") {
            Some(Value::Vector3D(size)) => *size,
            _ => Vector3D::new(1.0, 1.0, 1.0),
        };
        let angles = ["yaw", "pitch", "roll"].map(|name| match row.field(name) {
            Some(Value::Float(value)) => *value,
            _ => 0.0,
        });
        let mesh = match row.field("mesh") {
            Some(Value::String(mesh)) => mesh.as_str(),
            _ => "",
        };
        Some(Self {
            shape: shape.as_str(),
            position,
            size,
            angles,
            mesh,
        })
    }
}

#[doc(hidden)]
pub fn prepare_collision_shapes<T: 'static>(
    inputs: &SceneMeshInputs,
    model: &VecModel<T>,
    prepare: impl FnMut(&T) -> Option<SceneCollisionShape>,
) -> Option<Arc<[SceneCollisionShape]>> {
    inputs.remember(model);
    let (count, shapes) = model.inspect_rows(0, MAX_NODES, prepare);
    (count > 0).then(|| shapes.into_iter().flatten().collect::<Vec<_>>().into())
}

/// An authored row's local transform and renderable payload.
#[doc(hidden)]
pub struct SceneNodeDefinition<'a> {
    pub id: String,
    pub parent: String,
    pub parent_node: String,
    pub physics_body: &'a str,
    pub mesh: &'a str,
    pub mesh_asset: Option<&'a crate::MeshAsset3D>,
    pub geometry: Option<SceneGeometryInput<'a>>,
    pub collision_shapes: Option<Arc<[SceneCollisionShape]>>,
    pub material_variant: &'a str,
    pub instance_of: String,
    pub instance_root: String,
    pub instance_data: Option<[f64; 4]>,
    pub instance_template: bool,
    pub random_instances: Option<SceneRandomInstances>,
    pub instance_table: InstanceTable3D,
    pub instance_count_override: i64,
    pub instance_has_transparency: bool,
    pub instance_depth_sorting: bool,
    pub instance_shadow_bounds: Option<[Vector3D; 2]>,
    pub animation: MeshAnimationBlend,
    pub morph_weights: &'a [(&'a str, f64)],
    pub position: [f64; 3],
    pub scale: [f64; 3],
    pub angles: [f64; 3],
    pub pivot: [f64; 3],
    pub orientation: Option<Quaternion3D>,
    pub visible: bool,
    pub camera: Option<SceneCameraSettings>,
    pub look_at: String,
    pub look_at_point: Option<Point3D>,
    pub camera_up: Option<Vector3D>,
    pub color: Color,
    pub depth_bias: f64,
    pub motion_vector_enabled: bool,
    pub motion_vector_scale: f64,
    pub instancing_lod_range: [f64; 2],
    pub instancing_lod_factor: f64,
    pub material: Option<crate::scene3d::SceneMaterial>,
    pub material_slots: Vec<crate::scene3d::SceneMaterialBinding>,
    pub lightmap: Option<BakedLightmap>,
    pub bake_role: &'a str,
    pub lightmap_resolution: LightmapResolution,
    pub casts_shadow: bool,
    pub receives_shadow: bool,
    pub receives_reflections: bool,
    pub casts_reflections: bool,
    pub layers: u64,
    pub pickable: bool,
    pub collidable: bool,
    pub trigger: bool,
    pub lod_of: String,
    pub lod_max_size: f64,
    pub lod_bias: f64,
    pub camera_lod_bias: f64,
}

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct SceneNode {
    id: String,
    parent: String,
    parent_node: Option<Box<str>>,
    local: SceneTransform,
    rotation: Rotation3<f32>,
    physics: Option<Arc<BodyAttachment>>,
    world_locked: bool,
    camera: Option<SceneCameraSettings>,
    look_at: String,
    look_at_point: Option<Point3D>,
    camera_up: Option<Vector3D>,
    visible: bool,
    mesh: Option<MeshAsset>,
    node_catalog: Option<Arc<MeshNodeCatalog>>,
    collision_shapes: Option<Arc<[SceneCollisionShape]>>,
    animation: MeshAnimationBlend,
    morph_weights: Arc<[(String, f64)]>,
    instance_of: String,
    instance_root: String,
    instance_data: Option<[f32; 4]>,
    instance_template: bool,
    placements: Option<InstancePlacements>,
    instance_has_transparency: bool,
    instance_depth_sorting: bool,
    instance_shadow_bounds: Option<Bounds3D>,
    prototype_row: Option<u32>,
    color: Color,
    depth_bias: f64,
    motion_vector_enabled: bool,
    motion_vector_scale: f64,
    instancing_lod: InstancingLodRange,
    instancing_lod_factor: Option<f64>,
    appearance: Option<Arc<crate::scene_material::SceneAppearance>>,
    lightmap: Option<BakedLightmap>,
    bake_role: BakeRole,
    lightmap_resolution: LightmapResolution,
    casts_shadow: bool,
    receives_shadow: bool,
    receives_reflections: bool,
    casts_reflections: bool,
    layers: u64,
    pickable: bool,
    collidable: bool,
    trigger: bool,
    row: u32,
    lighting_index: u16,
    lod_of: String,
    lod_max_size: f64,
    lod_bias: f64,
    camera_lod_bias: f64,
}

#[derive(Clone, Debug, PartialEq)]
struct BodyAttachment {
    id: String,
    offset: SceneTransform,
}

impl SceneNodeDefinition<'_> {
    pub fn prepare(
        self,
        imported: impl Fn(&str) -> Option<MeshAsset>,
        renderables: bool,
    ) -> Option<SceneNode> {
        let narrow = |values: [f64; 3]| values.map(|value| value as f32);
        let (local, rotation) = if self.lod_of.is_empty() {
            if let Some(orientation) = self.orientation {
                SceneTransform::local_with_rotation(
                    narrow(self.position),
                    narrow(self.scale),
                    narrow(self.pivot),
                    orientation.rotation_f32(),
                )?
            } else {
                SceneTransform::local(
                    narrow(self.position),
                    narrow(self.scale),
                    narrow(self.angles),
                    narrow(self.pivot),
                )?
            }
        } else {
            // A replacement inherits the primary row's transform and appearance.
            (
                SceneTransform {
                    linear: Matrix3::identity(),
                    origin: Vector3::zeros(),
                    inverse: Matrix3::identity(),
                    reflected: false,
                },
                Rotation3::identity(),
            )
        };
        let mesh = if self.instance_of.is_empty() {
            match self.geometry {
                Some(SceneGeometryInput::Triangles(mesh)) => {
                    let mesh = mesh?;
                    renderables.then_some(MeshAsset::Static(mesh))
                }
                Some(SceneGeometryInput::Parametric(geometry)) if !geometry.shape.is_empty() => {
                    if renderables {
                        Some(MeshAsset::Static(geometry.mesh()?))
                    } else if geometry.is_valid() {
                        None
                    } else {
                        return None;
                    }
                }
                _ if let Some(asset) = self.mesh_asset.and_then(crate::MeshAsset3D::asset) => {
                    Some(asset.clone())
                }
                _ if self.mesh.is_empty() => None,
                _ => Some(MeshAsset::resolve(self.mesh, imported)?),
            }
        } else {
            None
        };
        let node_catalog = mesh.as_ref().and_then(MeshAsset::shared_node_catalog);
        let mesh = mesh.map(|asset| asset.select_material_variant(self.material_variant));
        let owns_mesh = self.instance_of.is_empty();
        let appearance = if owns_mesh {
            crate::scene_material::SceneAppearance::new(self.material, self.material_slots)
        } else {
            None
        };
        let morph_weights = if owns_mesh {
            self.morph_weights
                .iter()
                .map(|&(name, weight)| (name.to_owned(), weight))
                .collect()
        } else {
            Arc::from([])
        };
        let physics = if renderables && !self.physics_body.is_empty() && self.lod_of.is_empty() {
            let (offset, _) =
                SceneTransform::local([0.0; 3], narrow(self.scale), [0.0; 3], narrow(self.pivot))?;
            Some(Arc::new(BodyAttachment {
                id: self.physics_body.into(),
                offset,
            }))
        } else {
            None
        };
        Some(SceneNode {
            id: self.id,
            parent: self.parent,
            parent_node: (!self.parent_node.is_empty()).then(|| self.parent_node.into_boxed_str()),
            mesh,
            node_catalog,
            collision_shapes: self.collision_shapes,
            instance_of: self.instance_of,
            instance_root: self.instance_root,
            instance_data: self.instance_data.map(|data| {
                data.map(|value| {
                    let value = value as f32;
                    if value.is_finite() { value } else { 0.0 }
                })
            }),
            instance_template: self.instance_template,
            placements: InstancePlacements::new(
                self.instance_table,
                self.random_instances,
                self.instance_count_override,
                MAX_NODES,
            ),
            instance_has_transparency: self.instance_has_transparency,
            instance_depth_sorting: self.instance_depth_sorting,
            instance_shadow_bounds: self.instance_shadow_bounds.and_then(|bounds| {
                let [min, max] = bounds.map(|v| narrow([v.x, v.y, v.z]));
                let bounds = Bounds3D { min, max };
                (bounds.is_finite() && (0..3).all(|axis| min[axis] < max[axis])).then_some(bounds)
            }),
            prototype_row: None,
            animation: self.animation,
            morph_weights,
            visible: self.visible,
            local,
            rotation,
            physics,
            world_locked: false,
            camera: self.camera,
            look_at: self.look_at,
            look_at_point: self.look_at_point,
            camera_up: self.camera_up,
            color: self.color,
            depth_bias: if self.depth_bias.is_finite() {
                self.depth_bias
            } else {
                0.0
            },
            motion_vector_enabled: self.motion_vector_enabled,
            motion_vector_scale: self.motion_vector_scale,
            instancing_lod: InstancingLodRange::from_limits(self.instancing_lod_range),
            instancing_lod_factor: (self.instancing_lod_factor.is_finite()
                && self.instancing_lod_factor >= 0.0)
                .then(|| self.instancing_lod_factor.min(1.0)),
            appearance,
            lightmap: self.lightmap,
            bake_role: BakeRole::from_name(self.bake_role),
            lightmap_resolution: self.lightmap_resolution,
            casts_shadow: self.casts_shadow,
            receives_shadow: self.receives_shadow,
            receives_reflections: self.receives_reflections,
            casts_reflections: self.casts_reflections,
            layers: self.layers,
            pickable: self.pickable,
            collidable: self.collidable,
            trigger: self.trigger,
            row: 0,
            lighting_index: 0,
            lod_of: self.lod_of,
            lod_max_size: self.lod_max_size,
            lod_bias: lod_bias(self.lod_bias),
            camera_lod_bias: lod_bias(self.camera_lod_bias),
        })
    }
}

fn lod_bias(value: f64) -> f64 {
    if value.is_finite() && value >= 0.0 {
        value
    } else {
        1.0
    }
}

#[derive(Debug, Default)]
struct SceneMeshCache {
    styles: HashMap<(usize, usize, bool), Mesh3D>,
    poses: HashMap<(u32, usize), SampledMesh>,
    // Cache local poses for the glTF assets referenced by child rows.
    socket_poses: HashMap<(u32, usize), Arc<MeshPose>>,
}

fn styled_mesh(
    styles: &mut HashMap<(usize, usize, bool), Mesh3D>,
    mesh: Mesh3D,
    appearance: Option<&Arc<crate::scene_material::SceneAppearance>>,
    instance_blending: bool,
) -> Mesh3D {
    if appearance.is_none() && !instance_blending {
        return mesh;
    }
    let source = match &mesh {
        Mesh3D::Box => 0,
        Mesh3D::Sphere => 1,
        Mesh3D::Imported(mesh) => Arc::as_ptr(mesh) as usize,
    };
    let style = appearance.map_or(0, |appearance| Arc::as_ptr(appearance) as usize);
    styles
        .entry((source, style, instance_blending))
        .or_insert_with(|| {
            let mesh = mesh.styled(appearance);
            if instance_blending {
                mesh.with_instance_blending()
            } else {
                mesh
            }
        })
        .clone()
}

impl SceneNode {
    fn shadow_bounds_in(&self, mount: MeshMount) -> Option<Arc<[[f32; 3]; 2]>> {
        let root = mount.root.unwrap_or(mount.world);
        let bounds = self.instance_shadow_bounds?.transform(root.rows());
        bounds
            .is_finite()
            .then(|| Arc::new([bounds.min, bounds.max]))
    }
    fn transform_parent(&self) -> &str {
        if self.parent.is_empty() && !self.instance_of.is_empty() {
            &self.instance_root
        } else {
            &self.parent
        }
    }

    fn mounts_asset_attachments(&self) -> bool {
        !self.instance_template && self.instance_of.is_empty()
    }

    fn automatic_levels(&self) -> Vec<LodLevel> {
        match self.mesh.as_ref() {
            Some(MeshAsset::Lods(lods)) => lods
                .levels()
                .iter()
                .map(|level| LodLevel {
                    max_size: level.max_size,
                    mesh: lods.replacement(level),
                    posed: OnceLock::new(),
                })
                .collect(),
            _ => Vec::new(),
        }
    }

    fn collision_row(&mut self, mount: MeshMount) -> Option<CollisionRow> {
        if self.instance_template {
            return None;
        }
        let shapes = self.collision_shapes.take()?;
        Some(CollisionRow {
            row: self.row,
            id: self.id.clone(),
            world: mount.world,
            root: mount.root,
            placements: self.placements.clone(),
            shapes,
            layers: self.layers,
            collidable: self.collidable,
            trigger: self.trigger,
        })
    }

    fn append_scene(
        mut self,
        (world, mount): (WorldPose, Option<MeshMount>),
        objects: Option<&mut Vec<SceneObject>>,
        lights: Option<&mut Option<Vec<SceneLight>>>,
        cameras: Option<&mut ImportedCameras>,
        node_frame: Option<&mut Option<Box<AssetNodeFrame>>>,
        cache: &mut SceneMeshCache,
    ) {
        if self.instance_template {
            return;
        }
        let mounts_attachments = self.mounts_asset_attachments();
        if let Some(asset) = self.mesh.take() {
            let nodes = node_frame.and_then(|frame| {
                let catalog = self.node_catalog.take()?;
                let transform = mount?.world;
                *frame = Some(Box::new(AssetNodeFrame {
                    catalog,
                    transform,
                    pose: AssetNodePose::Rest,
                }));
                frame.as_deref_mut()
            });
            let attachments = ImportAttachments {
                nodes,
                rotation: world.rotation,
                lights: lights
                    .filter(|_| {
                        mounts_attachments && asset.has_lights() && world.rotation.is_some()
                    })
                    .map(|lights| lights.get_or_insert_with(Vec::new)),
                cameras: cameras.filter(|_| {
                    mounts_attachments && asset.has_cameras() && world.rotation.is_some()
                }),
            };
            let objects = objects.filter(|_| mount.is_some());
            let mount = mount.unwrap_or_else(|| MeshMount::at(world.transform));
            self.append_mesh(asset, mount, objects, attachments, cache);
        }
    }

    fn append_mesh(
        self,
        asset: MeshAsset,
        mount: MeshMount,
        objects: Option<&mut Vec<SceneObject>>,
        attachments: ImportAttachments<'_>,
        cache: &mut SceneMeshCache,
    ) {
        let Some(table) = self.placements.clone() else {
            return self.append_mesh_base(asset, mount, objects, attachments, cache);
        };
        let row = self.row;
        let depth_sorting = self.instance_depth_sorting;
        let shadow_bounds = self.shadow_bounds_in(mount);
        let mut template = Vec::new();
        let sample = table.len() != 0
            && objects
                .as_ref()
                .is_some_and(|objects| objects.len() < MAX_NODES);
        self.append_mesh_base(
            asset,
            mount,
            sample.then_some(&mut template),
            attachments,
            cache,
        );
        let root = mount.root.unwrap_or(mount.world);
        let Some(objects) = objects.filter(|_| !template.is_empty()) else {
            return;
        };
        let parts: Vec<_> = template
            .into_iter()
            .filter_map(|object| object.relative_to(root))
            .collect();
        if parts.is_empty() {
            return;
        }
        let count = table.len().min((MAX_NODES - objects.len()) / parts.len());
        let samples = (0..count).filter_map(|index| {
            let mut sample = table.sample(index)?;
            sample.transform = root.child(sample.transform)?;
            Some((index as u32, sample))
        });
        let place = |part: &SceneObject, (index, sample): (u32, InstancePlacement)| {
            let mut object = part.clone().with_parent_transform(sample.transform)?;
            let (node, inner) = part.imported_placement().unwrap_or((NO_MESH_NODE, None));
            let ordinal = inner.map_or(index, |inner| index * MAX_NODES as u32 + inner);
            object.imported_placement = Some((node, Some(ordinal)));
            object.instance_order = Some(InstanceOrder {
                group: [row, NO_MESH_NODE],
                ordinal: index,
                group_origin: mount.world.origin.into(),
                origin: sample.transform.origin.into(),
                depth_sorting,
            });
            object.instance_shadow_bounds.clone_from(&shadow_bounds);
            object.color = sample.tint.apply(object.color);
            object.instance_data = sample.data.or(object.instance_data);
            Some(object)
        };
        if let [part] = parts.as_slice() {
            objects.extend(samples.filter_map(|sample| place(part, sample)));
        } else {
            // Keep each part's copies contiguous so the GPU can draw them together.
            let samples: Vec<_> = samples.collect();
            for part in &parts {
                objects.extend(
                    samples
                        .iter()
                        .copied()
                        .filter_map(|sample| place(part, sample)),
                );
            }
        }
    }

    fn append_mesh_base(
        self,
        asset: MeshAsset,
        mount: MeshMount,
        objects: Option<&mut Vec<SceneObject>>,
        attachments: ImportAttachments<'_>,
        cache: &mut SceneMeshCache,
    ) {
        let renderables = objects
            .as_ref()
            .is_some_and(|objects| objects.len() < MAX_NODES);
        if !renderables
            && attachments.lights.is_none()
            && attachments.cameras.is_none()
            && attachments.nodes.is_none()
        {
            return;
        }
        let world = mount.world;
        let blend_table = self.instance_has_transparency && self.placements.is_some();
        let object = |id, mesh: crate::scene3d::Mesh3D, transform| {
            let lightmap = self.lightmap.as_ref().filter(|_| {
                !mesh.lightmap_uvs().is_empty()
                    && mesh.skin_geometry().is_none()
                    && mesh.morph_geometry().is_none()
            });
            let mut object = SceneObject::from_transform(id, mesh, transform, self.color)
                .with_depth_bias(self.depth_bias)
                .with_motion_vectors(self.motion_vector_enabled, self.motion_vector_scale)
                .with_shadows(self.casts_shadow, self.receives_shadow)
                .with_pickable(self.pickable)
                .with_collidable(self.collidable)
                .with_trigger(self.trigger)
                .with_source_row(self.row);
            object.receives_reflections = self.receives_reflections;
            object.casts_reflections = self.casts_reflections;
            object.layers = self.layers;
            object.lighting_index = self.lighting_index;
            object.instance_data = self.instance_data;
            object.instancing_lod = self.instancing_lod;
            object.lightmap = lightmap.cloned();
            object.bake_role = self.bake_role;
            object.lightmap_resolution = self.lightmap_resolution;
            object
        };
        match asset {
            MeshAsset::Lods(lods) => {
                self.append_mesh_base(lods.primary().clone(), mount, objects, attachments, cache)
            }
            MeshAsset::Variants(variants) => self.append_mesh_base(
                variants.primary().clone(),
                mount,
                objects,
                attachments,
                cache,
            ),
            MeshAsset::Static(mesh) => {
                if let Some(objects) = objects.filter(|_| renderables) {
                    objects.push(object(
                        self.id,
                        styled_mesh(
                            &mut cache.styles,
                            mesh,
                            self.appearance.as_ref(),
                            blend_table,
                        ),
                        world,
                    ));
                }
            }
            MeshAsset::Animated(rig) => {
                let shadow_bounds = if self.placements.is_none() {
                    self.shadow_bounds_in(mount)
                } else {
                    None
                };
                let retain_pose = attachments.nodes.is_some()
                    || (self.prototype_row.is_some() && self.node_catalog.is_some());
                let key = (
                    self.prototype_row.unwrap_or(self.row),
                    Arc::as_ptr(&rig) as usize,
                );
                let pose = cache.socket_poses.get(&key);
                let sample = |lights, cameras| {
                    let weights: Vec<_> = self
                        .morph_weights
                        .iter()
                        .filter(|_| renderables)
                        .map(|(name, weight)| (name.as_str(), *weight))
                        .collect();
                    let overrides = rig.morph_overrides(&weights);
                    rig.sample(
                        &self.animation,
                        MeshSampleOptions {
                            renderables,
                            retain_pose,
                            pose,
                            overrides: &overrides,
                            lights,
                            cameras,
                            instance_space: mount.instance_space.as_ref(),
                        },
                    )
                };
                let mut positioned_lights = Vec::new();
                let mut positioned_cameras = Vec::new();
                let collect_lights = attachments
                    .lights
                    .as_ref()
                    .is_some_and(|lights| lights.len() < MAX_SCENE_LIGHTS);
                let local_sample;
                let sampled: &SampledMesh = if self.prototype_row.is_some() {
                    cache.poses.entry(key).or_insert_with(|| sample(None, None))
                } else {
                    local_sample = sample(
                        collect_lights.then_some(&mut positioned_lights),
                        attachments
                            .cameras
                            .is_some()
                            .then_some(&mut positioned_cameras),
                    );
                    &local_sample
                };
                if let Some((frame, matrices)) = attachments.nodes.zip(sampled.pose.as_ref()) {
                    frame.pose = AssetNodePose::Animated(matrices.clone());
                }
                if let Some((lights, rotation)) = attachments.lights.zip(attachments.rotation) {
                    let remaining = MAX_SCENE_LIGHTS.saturating_sub(lights.len());
                    lights.extend(
                        positioned_lights
                            .into_iter()
                            .filter_map(|mut light| {
                                light.layers = self.layers;
                                light.mounted_on(world.origin, world.linear, rotation)
                            })
                            .take(remaining),
                    );
                }
                if let Some((cameras, rotation)) = attachments.cameras.zip(attachments.rotation) {
                    for camera in positioned_cameras {
                        let mut pose = camera.pose;
                        pose.eye = world.origin + world.linear * pose.eye;
                        if !pose.eye.iter().all(|value| value.is_finite()) {
                            continue;
                        }
                        pose.rotation = rotation * pose.rotation;
                        pose.rotation.renormalize();
                        pose.lod_bias = self.camera_lod_bias;
                        pose.layers = self.layers;
                        let keys = std::iter::once(format!("{}/#{}", self.id, camera.source_node))
                            .chain(camera.name.map(|name| format!("{}/{name}", self.id)));
                        for key in keys {
                            cameras
                                .entry(key)
                                .and_modify(|old| *old = None)
                                .or_insert(Some(pose));
                        }
                    }
                }
                let Some(objects) = objects else {
                    return;
                };
                for placement in &sampled.placements {
                    if objects.len() == MAX_NODES {
                        break;
                    }
                    if let Some(mut placed) =
                        placed_mesh(&rig, placement, world, |mesh, transform| {
                            let blend = blend_table
                                || self.instance_has_transparency && placement.source.1.is_some();
                            let mesh = styled_mesh(
                                &mut cache.styles,
                                mesh,
                                self.appearance.as_ref(),
                                blend,
                            );
                            object(self.id.clone(), mesh, transform)
                        })
                    {
                        if let ((node, Some(ordinal)), Some(anchor)) =
                            (placement.source, placement.instance_anchor)
                        {
                            placed.instance_order = Some(InstanceOrder {
                                group: [self.row, node],
                                ordinal,
                                group_origin: (world.origin + world.linear * anchor).into(),
                                origin: placed.transform_rows().map(|row| row[3]),
                                depth_sorting: self.instance_depth_sorting,
                            });
                            placed.instance_shadow_bounds.clone_from(&shadow_bounds);
                        }
                        objects.push(placed);
                    }
                    if objects.len() == MAX_NODES {
                        break;
                    }
                }
            }
        }
    }
}

fn placed_mesh(
    rig: &AnimatedMesh,
    placement: &MeshPlacement,
    world: SceneTransform,
    make_object: impl FnOnce(Mesh3D, SceneTransform) -> SceneObject,
) -> Option<SceneObject> {
    let transform = world.mesh_node(&placement.transform)?;
    let mesh = rig.meshes().get(placement.mesh)?.clone();
    let mut object = make_object(mesh, transform);
    object.imported_placement = Some(placement.source);
    if let Some(tint) = placement.tint {
        object.color = multiply_instance_tint(object.color, tint);
    }
    object.instance_data = object.instance_data.or(placement.instance_data);
    if let Some(weights) = &placement.morph {
        object = object.with_morph(weights.clone())?;
    }
    if let Some(palette) = &placement.skin {
        object = object.with_skin(palette.clone())?;
    }
    Some(object)
}

pub(crate) fn multiply_instance_tint(color: Color, tint: [f32; 4]) -> Color {
    if tint == [1.0; 4] {
        return color;
    }
    let linear = crate::scene_lighting::linear_color(color);
    let mut rgba = color.0.to_be_bytes();
    for channel in 0..3 {
        let value = linear[channel] * tint[channel];
        let encoded = if value <= 0.0031308 {
            value * 12.92
        } else {
            1.055 * value.powf(1.0 / 2.4) - 0.055
        };
        rgba[channel] = (encoded * 255.0).round() as u8;
    }
    rgba[3] = (f32::from(rgba[3]) * tint[3]).round() as u8;
    Color(u32::from_be_bytes(rgba))
}

pub(crate) fn rest_mesh_bounds(asset: &MeshAsset) -> crate::Bounds3D {
    match asset {
        MeshAsset::Lods(lods) => rest_mesh_bounds(lods.primary()),
        MeshAsset::Variants(variants) => rest_mesh_bounds(variants.primary()),
        MeshAsset::Static(mesh) => {
            let (min, max) = mesh.bounds();
            crate::Bounds3D::from_spatial(Some(Bounds3D { min, max }))
        }
        MeshAsset::Animated(_) => crate::Bounds3D::from_spatial(
            rest_mesh_objects(asset)
                .iter()
                .map(SceneObject::world_bounds)
                .filter(|bounds| bounds.is_finite())
                .reduce(Bounds3D::union),
        ),
    }
}

/// Primary geometry in the authored glTF rest pose, using the same node and
/// deformation transforms as a model drawn without an animation clip.
pub(crate) fn rest_mesh_objects(asset: &MeshAsset) -> Vec<SceneObject> {
    let identity = SceneTransform::identity();
    let object = |mesh, transform| {
        SceneObject::from_transform(String::new(), mesh, transform, Color(0xffffffff))
    };
    match asset {
        MeshAsset::Lods(lods) => rest_mesh_objects(lods.primary()),
        MeshAsset::Variants(variants) => rest_mesh_objects(variants.primary()),
        MeshAsset::Static(mesh) => vec![object(mesh.clone(), identity)],
        MeshAsset::Animated(rig) => rig
            .sample(
                &MeshAnimationBlend::default(),
                MeshSampleOptions {
                    renderables: true,
                    retain_pose: false,
                    pose: None,
                    overrides: &[],
                    lights: None,
                    cameras: None,
                    instance_space: None,
                },
            )
            .placements
            .into_iter()
            .filter_map(|placement| placed_mesh(rig, &placement, identity, object))
            .collect(),
    }
}

type ImportedCameras = BTreeMap<String, Option<CameraNode>>;

#[derive(Default)]
struct ImportAttachments<'a> {
    nodes: Option<&'a mut AssetNodeFrame>,
    rotation: Option<UnitQuaternion<f32>>,
    lights: Option<&'a mut Vec<SceneLight>>,
    cameras: Option<&'a mut ImportedCameras>,
}

/// A reusable selection of renderable surfaces for a camera.
#[doc(hidden)]
#[derive(Debug)]
pub struct SceneGeometry {
    objects: Arc<[SceneObject]>,
    visibility: OnceLock<Option<VisibilityIndex>>,
    bounds: Mutex<VecDeque<(u64, crate::Bounds3D)>>,
    instance_addresses: OnceLock<HashMap<[i64; 3], usize>>,
    shared_layers: u64,
    trigger_layers: OnceLock<u64>,
    distance_lods: Vec<usize>,
    distance_selections: Mutex<DistanceSelections>,
    layer_selections: Mutex<VecDeque<(u64, Arc<SceneVisibility>)>>,
    reflection_casters: OnceLock<Option<Arc<SceneVisibility>>>,
    capture_materials: Mutex<VecDeque<CaptureGeometry>>,
    pub lod_objects: usize,
    pub lod_fade: bool,
}

#[derive(Debug)]
struct CaptureGeometry {
    layers: u64,
    filter: CaptureRenderables,
    material: Arc<CaptureMaterial>,
    geometry: Arc<SceneGeometry>,
}

type DistanceSelections = VecDeque<([f32; 3], Arc<SceneVisibility>)>;

impl SceneGeometry {
    fn new(objects: Vec<SceneObject>, lod_objects: usize) -> Arc<Self> {
        let mut shared_layers = u64::MAX;
        let mut distance_lods = Vec::new();
        let mut lod_fade = false;
        for (index, object) in objects.iter().enumerate() {
            shared_layers &= object.layers;
            lod_fade |= object.lod_coverage != 0;
            if object.has_instancing_lod_range() {
                distance_lods.push(index);
            }
        }
        Arc::new(Self {
            objects: objects.into(),
            visibility: OnceLock::new(),
            bounds: Mutex::default(),
            instance_addresses: OnceLock::new(),
            shared_layers,
            trigger_layers: OnceLock::new(),
            distance_lods,
            distance_selections: Mutex::default(),
            layer_selections: Mutex::default(),
            reflection_casters: OnceLock::new(),
            capture_materials: Mutex::default(),
            lod_objects,
            lod_fade,
        })
    }

    #[doc(hidden)]
    pub fn with_capture_material(
        &self,
        layers: u64,
        filter: CaptureRenderables,
        material: &Arc<CaptureMaterial>,
    ) -> Arc<Self> {
        let mut captures = self
            .capture_materials
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = captures.iter().position(|entry| {
            entry.layers == layers && entry.filter == filter && entry.material == *material
        }) && let Some(entry) = captures.remove(index)
        {
            let geometry = entry.geometry.clone();
            captures.push_front(entry);
            return geometry;
        }
        let objects = self
            .objects
            .iter()
            .filter(|object| object.layers & layers != 0)
            .filter_map(|object| {
                let mesh = material.project(&object.mesh, filter)?;
                Some(object.clone().with_capture_mesh(mesh))
            })
            .collect();
        let geometry = Self::new(objects, self.lod_objects);
        captures.truncate(3);
        captures.push_front(CaptureGeometry {
            layers,
            filter,
            material: material.clone(),
            geometry: geometry.clone(),
        });
        geometry
    }

    pub fn objects(&self) -> &Arc<[SceneObject]> {
        &self.objects
    }

    pub(crate) fn instance_index(&self, address: [i64; 3]) -> Option<usize> {
        self.instance_addresses
            .get_or_init(|| {
                self.objects
                    .iter()
                    .enumerate()
                    .filter_map(|(index, object)| Some((object.instance_address()?, index)))
                    .collect()
            })
            .get(&address)
            .copied()
    }

    pub(crate) fn instance(&self, address: [i64; 3]) -> Instance3D {
        self.instance_index(address)
            .map_or_else(Instance3D::default, |index| {
                Instance3D::from_object(&self.objects[index], address)
            })
    }

    pub fn visibility(&self, camera: &SceneCamera) -> SceneVisibility {
        self.visibility
            .get_or_init(|| VisibilityIndex::new(&self.objects))
            .as_ref()
            .map_or_else(
                || SceneVisibility::all(self.objects.len()),
                |index| index.visibility(camera),
            )
    }

    pub(crate) fn visit_ray(&self, span: BoundsSegment, mut visit: impl FnMut(usize, f64) -> f64) {
        let index = (self.len() >= INDEX_THRESHOLD)
            .then(|| {
                self.visibility
                    .get_or_init(|| VisibilityIndex::new(&self.objects))
            })
            .and_then(Option::as_ref);
        if let Some(index) = index {
            index.visit_sweep(span, visit);
        } else {
            let mut max = span.max;
            for index in 0..self.len() {
                max = visit(index, max);
            }
        }
    }

    pub(crate) fn visit_nearest(
        &self,
        point: Vector3<f64>,
        mut limit: f64,
        mut visit: impl FnMut(usize, f64) -> f64,
    ) {
        if self.is_empty() {
            return;
        }
        if let Some(bounds) = self
            .visibility
            .get_or_init(|| VisibilityIndex::new(&self.objects))
        {
            bounds.visit_nearest(point, limit, visit);
        } else {
            for index in 0..self.len() {
                limit = visit(index, limit);
            }
        }
    }

    pub(crate) fn visit_selection(&self, frustum: &Frustum, mut visit: impl FnMut(usize, u8)) {
        if let Some(index) = self
            .visibility
            .get_or_init(|| VisibilityIndex::new(&self.objects))
        {
            index.visit_selection(frustum, visit);
        } else {
            for (index, object) in self.objects.iter().enumerate() {
                if let Some(planes) = frustum.bounds_planes(object.world_bounds()) {
                    visit(index, planes);
                }
            }
        }
    }

    /// Restrict eligible instances without reallocating the shared instance set.
    #[doc(hidden)]
    pub fn distance_visibility(&self, eye: [f32; 3]) -> Option<Arc<SceneVisibility>> {
        if self.distance_lods.is_empty() {
            return None;
        }
        let mut entries = self
            .distance_selections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = entries.iter().position(|(previous, _)| *previous == eye);
        let entry = index
            .and_then(|index| entries.remove(index))
            .unwrap_or_else(|| {
                let excluded = self
                    .distance_lods
                    .iter()
                    .copied()
                    .filter(|&index| !self.objects[index].visible_at_distance(eye));
                let selection = SceneVisibility::excluding(self.len(), excluded);
                let selection = entries
                    .iter()
                    .find(|(_, saved)| **saved == selection)
                    .map_or_else(|| Arc::new(selection), |(_, saved)| saved.clone());
                (eye, selection)
            });
        let visible = (entry.1.visible_count() != self.len()).then(|| entry.1.clone());
        entries.truncate(3);
        entries.push_front(entry);
        visible
    }

    /// A viewport layer filter shares the original instance indices and payloads.
    /// `None` is the fast path when every object is on a matching layer.
    pub fn layer_visibility(&self, layers: u64) -> Option<Arc<SceneVisibility>> {
        if self.is_empty() || self.shared_layers & layers != 0 {
            return None;
        }
        let mut entries = self
            .layer_selections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        let index = entries.iter().position(|(mask, _)| *mask == layers);
        let entry = index
            .and_then(|index| entries.remove(index))
            .unwrap_or_else(|| {
                (
                    layers,
                    Arc::new(SceneVisibility::from_layers(&self.objects, layers)),
                )
            });
        let visible = (entry.1.visible_count() != self.len()).then(|| entry.1.clone());
        entries.truncate(3);
        entries.push_front(entry);
        visible
    }

    #[doc(hidden)]
    pub fn reflection_visibility(&self) -> Option<Arc<SceneVisibility>> {
        self.reflection_casters
            .get_or_init(|| {
                let visibility =
                    SceneVisibility::matching(&self.objects, |object| object.casts_reflections);
                (visibility.visible_count() != self.objects.len()).then(|| Arc::new(visibility))
            })
            .clone()
    }
}

impl Deref for SceneGeometry {
    type Target = [SceneObject];

    fn deref(&self) -> &Self::Target {
        &self.objects
    }
}

#[derive(Debug)]
struct LodLevel {
    max_size: f64,
    mesh: MeshAsset,
    posed: OnceLock<Arc<[SceneObject]>>,
}

impl PartialEq for LodLevel {
    fn eq(&self, other: &Self) -> bool {
        self.max_size == other.max_size && self.mesh == other.mesh
    }
}

#[derive(Debug, PartialEq)]
struct LodGroup {
    range: Range<usize>,
    source: SceneNode,
    mount: MeshMount,
    levels: Vec<LodLevel>,
    fixed_level: Option<usize>,
    morph_error_scale: f64,
    // Derived bounds of the primary posed renderables, independent of the replacement.
    bounds: Bounds3D,
}

impl LodGroup {
    fn select(&self, camera: &SceneCamera, fade: f64) -> LodChoice {
        use LodChoice::Single;
        if let Some(level) = self.fixed_level {
            return Single(level);
        }
        let bias = self.source.lod_bias * camera.lod_bias;
        if bias <= 0.0 {
            return Single(0);
        }
        let size = camera.lod_size(self.bounds) * self.morph_error_scale;
        if !size.is_finite() {
            return Single(0);
        }
        for (index, level) in self.levels.iter().enumerate() {
            let threshold = level.max_size * bias;
            let coarse = index + 1;
            if fade > 0.0 && threshold.is_finite() {
                let previous = index
                    .checked_sub(1)
                    .map_or(0.0, |index| self.levels[index].max_size * bias);
                let next = self.levels.get(coarse);
                let gap = next.map_or(f64::INFINITY, |next| next.max_size * bias - threshold);
                let half = 0.5 * fade.min(threshold - previous).min(gap);
                if half > 0.0 && (size - threshold).abs() < half {
                    let coarse_steps =
                        ((0.5 + (threshold - size) / (2.0 * half)) * 64.0).round() as u8;
                    let fine = next.map_or(0, |_| coarse + 1);
                    return match coarse_steps {
                        0 => Single(fine),
                        64 => Single(coarse),
                        _ => LodChoice::Blend {
                            fine,
                            coarse,
                            coarse_steps,
                        },
                    };
                }
            }
            if size <= threshold {
                return Single(coarse);
            }
        }
        Single(0)
    }

    fn replacement(&self, selection: usize) -> Option<&[SceneObject]> {
        let level = self.levels.get(selection.checked_sub(1)?)?;
        let posed = level.posed.get_or_init(|| {
            let mut objects = Vec::new();
            self.source.clone().append_mesh(
                level.mesh.clone(),
                self.mount,
                Some(&mut objects),
                ImportAttachments::default(),
                &mut SceneMeshCache::default(),
            );
            objects.into()
        });
        (!posed.is_empty()).then_some(posed)
    }
}

/// Tracks the mesh resources of one prepared scene across preload frames.
#[doc(hidden)]
#[derive(Default)]
pub struct ScenePreloadCursor {
    primary: usize,
    group: usize,
    level: usize,
    row: usize,
}

#[doc(hidden)]
pub enum ScenePreloadStep<'a> {
    Mesh(&'a Mesh3D),
    Skipped,
    Deferred,
}

impl ScenePreloadCursor {
    pub fn pending(&self, model: &SceneModel) -> bool {
        self.primary < model.objects().len() || self.group < model.lod_groups.len()
    }

    pub fn advance<'a>(
        &mut self,
        model: &'a SceneModel,
        new_lod_poses: &mut usize,
    ) -> Option<ScenePreloadStep<'a>> {
        if let Some(object) = model.objects().get(self.primary) {
            self.primary += 1;
            return Some(ScenePreloadStep::Mesh(&object.mesh));
        }

        let group = model.lod_groups.get(self.group)?;
        let selection = group.fixed_level.unwrap_or(self.level + 1);
        let Some(level) = selection
            .checked_sub(1)
            .and_then(|index| group.levels.get(index))
        else {
            self.group += 1;
            return Some(ScenePreloadStep::Skipped);
        };
        if level.posed.get().is_none() {
            if *new_lod_poses == 0 {
                return Some(ScenePreloadStep::Deferred);
            }
            *new_lod_poses -= 1;
        }

        // Use the same posed objects as drawing so the replacement's
        // deformation and per-slot material bindings share their GPU keys.
        let objects = group.replacement(selection).unwrap_or_default();
        let mesh = objects.get(self.row).map(|object| &object.mesh);
        self.row += 1;
        if self.row >= objects.len() {
            self.row = 0;
            self.level += 1;
            if group.fixed_level.is_some() || self.level == group.levels.len() {
                self.group += 1;
                self.level = 0;
            }
        }
        Some(mesh.map_or(ScenePreloadStep::Skipped, ScenePreloadStep::Mesh))
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum LodChoice {
    Single(usize),
    Blend {
        fine: usize,
        coarse: usize,
        coarse_steps: u8,
    },
}

impl LodChoice {
    fn dominant(self) -> usize {
        match self {
            Self::Single(level) => level,
            Self::Blend {
                fine,
                coarse,
                coarse_steps,
            } => {
                if coarse_steps >= 32 {
                    coarse
                } else {
                    fine
                }
            }
        }
    }
}

type ViewSelections = VecDeque<(Vec<LodChoice>, Arc<SceneGeometry>)>;

#[derive(Debug)]
enum SceneLodSelection {
    Primary,
    Fixed(OnceLock<Arc<SceneGeometry>>),
    Adaptive(Mutex<ViewSelections>),
}

impl SceneLodSelection {
    fn for_groups(groups: &[LodGroup]) -> Self {
        if groups.iter().any(|group| group.fixed_level.is_none()) {
            Self::Adaptive(Mutex::default())
        } else if groups.iter().any(|group| group.fixed_level != Some(0)) {
            Self::Fixed(OnceLock::new())
        } else {
            Self::Primary
        }
    }
}

#[derive(Debug, PartialEq)]
struct CollisionRow {
    row: u32,
    id: String,
    world: SceneTransform,
    root: Option<SceneTransform>,
    placements: Option<InstancePlacements>,
    shapes: Arc<[SceneCollisionShape]>,
    layers: u64,
    collidable: bool,
    trigger: bool,
}

impl CollisionRow {
    fn append(&self, colliders: &mut Vec<SceneObject>) {
        if !self.collidable && !self.trigger {
            return;
        }
        let Some(table) = &self.placements else {
            return self.append_shapes(self.world, None, colliders);
        };
        let root = self.root.unwrap_or(self.world);
        let Some(relative) = self.world.relative_to(root) else {
            return;
        };
        for index in 0..table.len() {
            if colliders.len() >= MAX_NODES {
                break;
            }
            if let Some(world) = table
                .transform(index)
                .and_then(|placement| root.child(placement))
                .and_then(|placement| placement.child(relative))
            {
                self.append_shapes(world, Some(index as u32), colliders);
            }
        }
    }

    fn append_shapes(
        &self,
        mounted: SceneTransform,
        index: Option<u32>,
        colliders: &mut Vec<SceneObject>,
    ) {
        for shape in self.shapes.iter() {
            if colliders.len() >= MAX_NODES {
                break;
            }
            let Some(world) = mounted.child(shape.local) else {
                continue;
            };
            if let MeshAsset::Static(mesh) = &shape.asset {
                colliders.extend(self.collision_object(
                    SceneObject::from_transform(
                        String::new(),
                        mesh.clone(),
                        world,
                        Color(0xffffffff),
                    ),
                    index,
                ));
            } else {
                let remaining = MAX_NODES - colliders.len();
                colliders.extend(
                    rest_mesh_objects(&shape.asset)
                        .into_iter()
                        .filter_map(|mesh| {
                            self.collision_object(mesh.with_parent_transform(world)?, index)
                        })
                        .take(remaining),
                );
            }
        }
    }

    fn collision_object(&self, object: SceneObject, index: Option<u32>) -> Option<SceneObject> {
        let mut object = object
            .with_pickable(false)
            .with_collidable(self.collidable)
            .with_trigger(self.trigger)
            .with_source_row(self.row);
        // Open triangle surfaces cannot describe the interior of a trigger.
        if self.trigger && !object.is_trigger() {
            return None;
        }
        object.id.clone_from(&self.id);
        object.layers = self.layers;
        if let Some(index) = index {
            object.imported_placement = Some((NO_MESH_NODE, Some(index)));
        }
        Some(object)
    }
}

/// Renderables and named camera poses resolved from the same authored rows.
#[derive(Debug)]
pub struct SceneModel {
    geometry: Arc<SceneGeometry>,
    collision_rows: Vec<CollisionRow>,
    collision_geometry: OnceLock<Arc<SceneGeometry>>,
    cameras: BTreeMap<String, CameraNode>,
    imported_lights: Option<Arc<[SceneLight]>>,
    frames: HashMap<String, NodeFrame>,
    lod_groups: Vec<LodGroup>,
    lod_selection: SceneLodSelection,
    physics: Option<Box<PhysicsScene>>,
}

#[derive(Debug)]
struct PhysicsScene {
    nodes: Vec<SceneNode>,
    bindings: Vec<usize>,
    cache: Mutex<PhysicsSceneCache>,
}

#[derive(Debug, Default)]
struct PhysicsSceneCache {
    projection: Option<PhysicsProjection>,
    meshes: SceneMeshCache,
}

#[derive(Debug)]
struct PhysicsProjection {
    poses: Vec<Option<RigidPose>>,
    model: Arc<SceneModel>,
}

impl PartialEq for SceneModel {
    fn eq(&self, other: &Self) -> bool {
        self.objects() == other.objects()
            && self.collision_rows == other.collision_rows
            && self.cameras == other.cameras
            && self.imported_lights == other.imported_lights
            && self.frames == other.frames
            && self.lod_groups == other.lod_groups
            && self.physics.as_ref().map(|scene| &scene.nodes)
                == other.physics.as_ref().map(|scene| &scene.nodes)
    }
}

#[derive(Debug)]
enum AssetNodePose {
    Rest,
    Animated(Arc<MeshPose>),
}

#[derive(Debug)]
struct AssetNodeFrame {
    catalog: Arc<MeshNodeCatalog>,
    transform: SceneTransform,
    pose: AssetNodePose,
}

impl PartialEq for AssetNodeFrame {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.catalog, &other.catalog)
            && self.transform == other.transform
            && match (&self.pose, &other.pose) {
                (AssetNodePose::Rest, AssetNodePose::Rest) => true,
                (AssetNodePose::Animated(a), AssetNodePose::Animated(b)) => {
                    Arc::ptr_eq(a, b) || a.matrices == b.matrices
                }
                _ => false,
            }
    }
}

impl AssetNodeFrame {
    fn node_matrix(&self, ordinal: usize) -> Option<Matrix4<f32>> {
        match &self.pose {
            AssetNodePose::Rest => Some(self.catalog.entries().get(ordinal)?.rest_matrix()),
            // Import uses the same traversal for the catalog and the animation rig.
            AssetNodePose::Animated(pose) => pose.matrices.get(ordinal).copied().flatten(),
        }
    }

    fn world_axes(&self, ordinal: usize) -> Option<(Vector3<f32>, Matrix3<f32>)> {
        let matrix = self.node_matrix(ordinal)?;
        let origin =
            self.transform.origin + self.transform.linear * matrix.fixed_view::<3, 1>(0, 3);
        let linear = self.transform.linear * matrix.fixed_view::<3, 3>(0, 0);
        origin
            .iter()
            .chain(linear.iter())
            .all(|value| value.is_finite())
            .then_some((origin, linear))
    }

    fn pose_node(&self, mut node: MeshNode3D) -> Option<MeshNode3D> {
        let ordinal = self.catalog.ordinal(node.source_index)?;
        let (origin, linear) = self.world_axes(ordinal)?;
        let axis = |i| {
            let column: Vector3<f32> = linear.column(i).into_owned();
            Vector3D::new(
                f64::from(column.x),
                f64::from(column.y),
                f64::from(column.z),
            )
        };
        node.origin = Point3D::new(
            f64::from(origin.x),
            f64::from(origin.y),
            f64::from(origin.z),
        );
        node.axis_x = axis(0);
        node.axis_y = axis(1);
        node.axis_z = axis(2);
        Some(node)
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct NodeFrame {
    pub origin: Vector3<f32>,
    pub linear: Matrix3<f32>,
    inverse: Matrix3<f32>,
    rotation: Rotation3<f32>,
    rotation_parent: String,
    visible: bool,
    subtree: [u16; 2],
    asset: Option<Box<AssetNodeFrame>>,
}

impl Default for SceneModel {
    fn default() -> Self {
        prepare_nodes(Vec::new(), true, &mut SceneMeshCache::default())
    }
}

impl SceneModel {
    /// Resolve current poses into a scene snapshot without changing the authored source.
    pub fn resolve_physics(self: &Arc<Self>, world: &PhysicsWorld3D) -> Arc<Self> {
        self.with_physics(world).unwrap_or_else(|| self.clone())
    }

    fn with_physics(&self, world: &PhysicsWorld3D) -> Option<Arc<Self>> {
        let source = self.physics.as_ref()?;
        let ids = source.bindings.iter().map(|&index| {
            source.nodes[index]
                .physics
                .as_ref()
                .map_or("", |link| link.id.as_str())
        });
        let poses = world.scene_poses(ids)?;
        let mut cache = source
            .cache
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(previous) = cache
            .projection
            .as_ref()
            .filter(|previous| previous.poses == poses)
        {
            return Some(previous.model.clone());
        }

        let mut nodes = source.nodes.clone();
        for (&index, pose) in source.bindings.iter().zip(&poses) {
            let node = &mut nodes[index];
            let Some((link, pose)) = node.physics.take().zip(*pose) else {
                continue;
            };
            let q = pose.rotation;
            let rotation = UnitQuaternion::from_quaternion(Quaternion::new(q.w, q.x, q.y, q.z))
                .to_rotation_matrix();
            let linear = *rotation.matrix();
            let rigid = SceneTransform {
                linear,
                origin: Vector3::from(pose.translation.to_array()),
                inverse: linear.transpose(),
                reflected: false,
            };
            if let Some(transform) = rigid.child(link.offset) {
                node.local = transform;
                node.rotation = rotation;
                node.world_locked = true;
            }
        }
        let model = Arc::new(prepare_nodes(nodes, true, &mut cache.meshes));
        cache.projection = Some(PhysicsProjection {
            poses,
            model: model.clone(),
        });
        Some(model)
    }

    pub(crate) fn collision_geometry(&self) -> &SceneGeometry {
        if self.collision_rows.is_empty() {
            return &self.geometry;
        }
        self.collision_geometry.get_or_init(|| {
            // Replace every rendered part of a row with its authored shapes.
            // Preserve row order for equal-distance hits and model indices.
            let mut renderables = self.objects().iter().peekable();
            let mut colliders = Vec::new();
            for row in &self.collision_rows {
                while colliders.len() < MAX_NODES
                    && renderables
                        .peek()
                        .is_some_and(|object| object.scene_row() < Some(row.row))
                {
                    colliders.extend(renderables.next().cloned());
                }
                while renderables
                    .peek()
                    .is_some_and(|object| object.scene_row() == Some(row.row))
                {
                    renderables.next();
                }
                row.append(&mut colliders);
                if colliders.len() >= MAX_NODES {
                    break;
                }
            }
            colliders.extend(
                renderables
                    .cloned()
                    .take(MAX_NODES.saturating_sub(colliders.len())),
            );
            SceneGeometry::new(colliders, 0)
        })
    }

    pub(crate) fn has_collision_shapes(&self) -> bool {
        !self.collision_rows.is_empty()
    }

    pub(crate) fn has_triggers(&self, layers: u64) -> bool {
        layers != 0
            && layers
                & self.collision_geometry().trigger_layers.get_or_init(|| {
                    self.collision_geometry()
                        .objects()
                        .iter()
                        .filter(|object| object.is_trigger())
                        .fold(0, |mask, object| mask | object.layers)
                })
                != 0
    }

    pub(crate) fn visit_sweep(
        &self,
        mut span: BoundsSegment,
        mut visit: impl FnMut(usize, f64) -> f64,
    ) {
        let geometry = self.collision_geometry();
        if geometry.is_empty() {
            return;
        }
        if let Some(bounds) = geometry
            .visibility
            .get_or_init(|| VisibilityIndex::new(&geometry.objects))
        {
            bounds.visit_sweep(span, visit);
        } else {
            for index in 0..geometry.len() {
                span.max = visit(index, span.max);
            }
        }
    }

    pub(crate) fn query_geometry(&self) -> &SceneGeometry {
        &self.geometry
    }

    pub fn objects(&self) -> &Arc<[SceneObject]> {
        self.geometry.objects()
    }

    /// Bounds of the prepared scene pose, before camera-dependent LOD selection.
    pub fn bounds(&self, layer_mask: Option<i64>) -> crate::Bounds3D {
        let layers = layer_mask.map_or(super::scene3d::MAIN_LAYER, |mask| mask as u64);
        let mut cached = self
            .geometry
            .bounds
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some((_, bounds)) = cached.iter().find(|(mask, _)| *mask == layers) {
            return *bounds;
        }
        let result = crate::Bounds3D::from_spatial(
            self.objects()
                .iter()
                .filter(|object| object.layers & layers != 0)
                .map(SceneObject::world_bounds)
                .filter(|bounds| bounds.is_finite())
                .reduce(Bounds3D::union),
        );
        cached.truncate(3);
        cached.push_front((layers, result));
        result
    }

    pub fn imported_lights(&self) -> Option<Arc<[SceneLight]>> {
        self.imported_lights.clone()
    }

    pub fn camera(&self, id: &str, viewport: [f64; 2]) -> Option<SceneCamera> {
        SceneCamera::from_node(self.cameras.get(id)?, viewport)
    }

    #[doc(hidden)]
    pub fn geometry(&self, camera: &SceneCamera) -> Arc<SceneGeometry> {
        self.render_geometry(camera, 0.0)
    }

    /// Choose the camera's draw geometry, with screen-space LOD coverage when requested.
    #[doc(hidden)]
    pub fn render_geometry(&self, camera: &SceneCamera, fade_width: f64) -> Arc<SceneGeometry> {
        if camera.lod_bias == 0.0 {
            return self.geometry.clone();
        }
        match &self.lod_selection {
            SceneLodSelection::Primary => self.geometry.clone(),
            SceneLodSelection::Fixed(geometry) => geometry
                .get_or_init(|| {
                    let levels: Vec<_> = self
                        .lod_groups
                        .iter()
                        .map(|group| LodChoice::Single(group.fixed_level.unwrap_or(0)))
                        .collect();
                    self.geometry_with_lods(&levels)
                })
                .clone(),
            SceneLodSelection::Adaptive(selections) => {
                let fade = if fade_width.is_finite() {
                    fade_width.clamp(0.0, 4096.0)
                } else {
                    0.0
                };
                self.adaptive_geometry(camera, fade, selections)
            }
        }
    }

    fn adaptive_geometry(
        &self,
        camera: &SceneCamera,
        fade: f64,
        selections: &Mutex<ViewSelections>,
    ) -> Arc<SceneGeometry> {
        let key: Vec<_> = self
            .lod_groups
            .iter()
            .map(|group| group.select(camera, fade))
            .collect();
        if key.iter().all(|&level| level == LodChoice::Single(0)) {
            return self.geometry.clone();
        }
        let mut selections = selections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = selections.iter().position(|(old, _)| *old == key)
            && let Some(entry) = selections.remove(index)
        {
            let result = entry.1.clone();
            selections.push_front(entry);
            return result;
        }

        let result = self.geometry_with_lods(&key);
        selections.truncate(3);
        selections.push_front((key, result.clone()));
        result
    }

    fn geometry_with_lods(&self, levels: &[LodChoice]) -> Arc<SceneGeometry> {
        let mut objects = Vec::with_capacity(self.len());
        let mut lod_objects = 0;
        let mut cursor = 0;
        let groups = || self.lod_groups.iter().zip(levels.iter().copied());
        let mut spare = 0;
        if levels
            .iter()
            .any(|level| matches!(level, LodChoice::Blend { .. }))
        {
            let baseline = groups().fold(self.len(), |count, (group, level)| {
                count - group.range.len() + self.lod_slice(group, level.dominant()).len()
            });
            spare = MAX_NODES.saturating_sub(baseline);
        }
        let mut append = |slice: &[SceneObject], coverage| {
            let count = slice.len().min(MAX_NODES - objects.len());
            let start = objects.len();
            objects.extend_from_slice(&slice[..count]);
            if coverage != 0 {
                for object in &mut objects[start..] {
                    object.lod_coverage = coverage;
                }
            }
        };
        for (group, level) in groups() {
            append(&self[cursor..group.range.start], 0);
            cursor = group.range.end;
            let dominant = level.dominant();
            if let LodChoice::Blend {
                fine,
                coarse,
                coarse_steps,
            } = level
            {
                let fine_objects = self.lod_slice(group, fine);
                let coarse_objects = self.lod_slice(group, coarse);
                let extra = if dominant == fine {
                    coarse_objects.len()
                } else {
                    fine_objects.len()
                };
                if extra <= spare && !std::ptr::eq(fine_objects, coarse_objects) {
                    append(fine_objects, coarse_steps | 128);
                    append(coarse_objects, coarse_steps);
                    spare -= extra;
                    lod_objects += 1;
                    continue;
                }
            }
            if let Some(replacement) = group.replacement(dominant) {
                lod_objects += 1;
                append(replacement, 0);
            } else {
                append(&self[group.range.clone()], 0);
            }
        }
        append(&self[cursor..], 0);
        SceneGeometry::new(objects, lod_objects)
    }

    fn lod_slice<'a>(&'a self, group: &'a LodGroup, level: usize) -> &'a [SceneObject] {
        group
            .replacement(level)
            .unwrap_or(&self[group.range.clone()])
    }

    /// Read a primary instance by its source row, mesh node, and table index.
    pub fn instance(&self, address: [i64; 3]) -> Instance3D {
        self.geometry.instance(address)
    }

    /// Read the placement selected for a named camera.
    pub fn instance_camera(
        &self,
        camera: &str,
        viewport: Point2D,
        address: [i64; 3],
    ) -> Instance3D {
        self.camera(camera, [viewport.x.0, viewport.y.0])
            .map_or_else(Instance3D::default, |camera| {
                self.geometry(&camera).instance(address)
            })
    }

    /// Read the placement selected for an orbit camera.
    pub fn instance_orbit(
        &self,
        [yaw, pitch, distance]: [f64; 3],
        viewport: Point2D,
        address: [i64; 3],
    ) -> Instance3D {
        SceneCamera::new(yaw, pitch, distance, [viewport.x.0, viewport.y.0])
            .map_or_else(Instance3D::default, |camera| {
                self.geometry(&camera).instance(address)
            })
    }

    /// Read a glTF node in the current world pose of the named scene row.
    pub fn scene_node(&self, object: &str, selector: &str) -> MeshNode3D {
        self.asset_node(object, |catalog| catalog.node(selector))
    }

    /// Read a glTF node by its original source index.
    pub fn scene_node_at(&self, object: &str, source: i64) -> MeshNode3D {
        self.asset_node(object, |catalog| catalog.node_at(source))
    }

    fn asset_node(
        &self,
        object: &str,
        select: impl FnOnce(&MeshNodeCatalog) -> MeshNode3D,
    ) -> MeshNode3D {
        self.frames
            .get(object)
            .and_then(|frame| frame.asset.as_deref())
            .and_then(|frame| frame.pose_node(select(&frame.catalog)))
            .unwrap_or_default()
    }

    /// Convert a point between authored nodes. An empty endpoint means world space.
    pub fn map_position(&self, from: &str, to: &str, point: Point3D) -> Option<Point3D> {
        let (source, destination) = self.mapping_endpoints(from, to)?;
        let mut point = Vector3::new(point.x, point.y, point.z);
        if from != to {
            if let Some(source) = source {
                point = source.linear.cast::<f64>() * point + source.origin.cast::<f64>();
            }
            if let Some(destination) = destination {
                point = destination.inverse.cast::<f64>() * (point - destination.origin.cast());
            }
        }
        finite_coordinates(point).map(|[x, y, z]| Point3D::new(x, y, z))
    }

    /// Convert a direction using inherited rotations, preserving its magnitude.
    pub fn map_direction(&self, from: &str, to: &str, direction: Vector3D) -> Option<Vector3D> {
        let (source, destination) = self.mapping_endpoints(from, to)?;
        let mut direction = Vector3::new(direction.x, direction.y, direction.z);
        if from != to {
            if let Some(source) = source {
                direction = self.node_rotation(source) * direction;
            }
            if let Some(destination) = destination {
                direction = self
                    .node_rotation(destination)
                    .inverse_transform_vector(&direction);
            }
        }
        finite_coordinates(direction).map(|[x, y, z]| Vector3D::new(x, y, z))
    }

    fn mapping_endpoints(
        &self,
        from: &str,
        to: &str,
    ) -> Option<(Option<&NodeFrame>, Option<&NodeFrame>)> {
        let resolve = |id: &str| {
            if id.is_empty() {
                Some(None)
            } else {
                self.frames.get(id).map(Some)
            }
        };
        Some((resolve(from)?, resolve(to)?))
    }

    fn node_rotation<'a>(&'a self, mut frame: &'a NodeFrame) -> UnitQuaternion<f64> {
        let mut rotation = UnitQuaternion::identity();
        loop {
            rotation =
                UnitQuaternion::from_rotation_matrix(&frame.rotation.cast::<f64>()) * rotation;
            let Some(parent) = self.frames.get(&frame.rotation_parent) else {
                break;
            };
            frame = parent;
        }
        rotation.renormalize();
        rotation
    }

    pub(crate) fn light_scope(&self, id: &str) -> Option<SceneLightScope> {
        let [first, last] = self.node_frame(id)?.subtree;
        Some(SceneLightScope::subtree(first, last))
    }

    pub(crate) fn node_frame(&self, id: &str) -> Option<&NodeFrame> {
        self.frames.get(id).filter(|frame| frame.visible)
    }

    pub(crate) fn particle_parent_rows(
        &self,
        parent: &str,
        selector: &str,
    ) -> Option<[[f32; 4]; 3]> {
        let frame = self.node_frame(parent)?;
        let (origin, linear) = if selector.is_empty() {
            (frame.origin, frame.linear)
        } else {
            let asset = frame.asset.as_deref()?;
            let ordinal = asset.catalog.selector_ordinal(selector)?;
            asset.world_axes(ordinal)?
        };
        Some(std::array::from_fn(|row| {
            [
                linear[(row, 0)],
                linear[(row, 1)],
                linear[(row, 2)],
                origin[row],
            ]
        }))
    }

    pub(crate) fn rotate_node_direction<'a>(
        &'a self,
        mut frame: &'a NodeFrame,
        mut direction: Vector3<f32>,
    ) -> Vector3<f32> {
        loop {
            direction = frame.rotation * direction;
            let Some(parent) = self.frames.get(&frame.rotation_parent) else {
                break;
            };
            frame = parent;
        }
        direction.normalize()
    }
}

fn finite_coordinates(value: Vector3<f64>) -> Option<[f64; 3]> {
    let point = [value.x, value.y, value.z];
    point.into_iter().all(f64::is_finite).then_some(point)
}

impl Deref for SceneModel {
    type Target = [SceneObject];

    fn deref(&self) -> &Self::Target {
        &self.geometry
    }
}

#[derive(Clone, Copy)]
struct WorldPose {
    transform: SceneTransform,
    rotation: Option<UnitQuaternion<f32>>,
    visible: bool,
}

impl WorldPose {
    fn root(node: &SceneNode, track_cameras: bool) -> Self {
        Self {
            transform: node.local,
            visible: node.visible,
            rotation: track_cameras.then(|| UnitQuaternion::from_rotation_matrix(&node.rotation)),
        }
    }

    fn child(self, node: &SceneNode, camera_ancestor: bool) -> Option<Self> {
        if node.world_locked {
            let mut pose = Self::root(node, camera_ancestor);
            pose.visible &= self.visible;
            return Some(pose);
        }
        let transform = self.transform.child(node.local)?;
        let rotation = self
            .rotation
            .filter(|_| camera_ancestor)
            .map(|parent| parent * UnitQuaternion::from_rotation_matrix(&node.rotation));
        Some(Self {
            transform,
            rotation,
            visible: self.visible && node.visible,
        })
    }
}

#[derive(Clone, Copy)]
enum Parent {
    Root,
    Node(usize),
    Invalid,
}

#[derive(Clone, Copy)]
enum Pose {
    Pending,
    Resolving,
    Unavailable,
    Ready(WorldPose),
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct MeshMount {
    world: SceneTransform,
    root: Option<SceneTransform>,
    instance_space: Option<MeshInstanceSpace>,
}

impl MeshMount {
    fn at(world: SceneTransform) -> Self {
        Self {
            world,
            root: None,
            instance_space: None,
        }
    }

    fn resolve(
        world: SceneTransform,
        root: Parent,
        prototype: Option<usize>,
        poses: &[Pose],
    ) -> Option<Self> {
        let root = match root {
            Parent::Root => return Some(Self::at(world)),
            Parent::Invalid => return None,
            Parent::Node(index) => match poses[index] {
                Pose::Ready(root) if root.visible => root.transform,
                _ => return None,
            },
        };
        if let Some(index) = prototype {
            let Pose::Ready(prototype) = poses[index] else {
                return None;
            };
            let model = prototype.transform.relative_to(root)?;
            Some(Self {
                root: Some(root),
                ..Self::at(world.child(model)?)
            })
        } else {
            Some(Self {
                world,
                root: Some(root),
                instance_space: Some(MeshInstanceSpace {
                    to_model: root.relative_to(world)?.matrix(),
                    to_root: world.relative_to(root)?.matrix(),
                }),
            })
        }
    }
}

fn scene_reference(name: &str, ids: &HashMap<&str, Option<usize>>) -> Parent {
    if name.is_empty() {
        Parent::Root
    } else {
        ids.get(name)
            .copied()
            .flatten()
            .map_or(Parent::Invalid, Parent::Node)
    }
}

fn node_indices(nodes: &[SceneNode]) -> HashMap<&str, Option<usize>> {
    let mut ids = HashMap::new();
    for (index, node) in nodes
        .iter()
        .enumerate()
        .filter(|(_, node)| !node.id.is_empty())
    {
        ids.entry(node.id.as_str())
            .and_modify(|entry| *entry = None)
            .or_insert(Some(index));
    }
    ids
}

fn resolve_prototypes(nodes: &mut [SceneNode], renderables: bool) -> Vec<Option<usize>> {
    if !nodes.iter().any(|node| !node.instance_of.is_empty()) {
        return Vec::new();
    }
    let ids = node_indices(nodes);
    let sources: Vec<_> = nodes
        .iter()
        .map(|node| {
            if node.instance_of.is_empty() || node.instance_template {
                return None;
            }
            let index = ids.get(node.instance_of.as_str()).copied().flatten()?;
            let source = &nodes[index];
            (source.instance_of.is_empty()
                && (!renderables || source.mesh.is_some() || source.collision_shapes.is_some()))
            .then_some(index)
        })
        .collect();
    for (index, &source) in sources.iter().enumerate() {
        let Some(source) = source else { continue };
        let Ok([instance, prototype]) = nodes.get_disjoint_mut([index, source]) else {
            continue;
        };
        if instance.instance_root.is_empty() {
            instance.instance_root.clone_from(&prototype.instance_root);
        }
        if !renderables {
            continue;
        }
        instance.mesh = prototype.mesh.clone();
        if instance.collision_shapes.is_none() {
            instance.collision_shapes = prototype.collision_shapes.clone();
        }
        instance.appearance = prototype.appearance.clone();
        instance.animation = prototype.animation.clone();
        instance.node_catalog = prototype.node_catalog.clone();
        instance.morph_weights = prototype.morph_weights.clone();
        instance.prototype_row = Some(prototype.row);
    }
    sources
}

fn rotation_ancestry(nodes: &[SceneNode], parents: &[Parent], renderables: bool) -> Vec<bool> {
    // Only view and imported light chains need a separate, scale-free orientation.
    let mut needed = vec![false; nodes.len()];
    for (mut index, _) in nodes.iter().enumerate().filter(|(_, node)| {
        node.camera.is_some()
            || node.mounts_asset_attachments()
                && ((!node.id.is_empty() && node.mesh.as_ref().is_some_and(MeshAsset::has_cameras))
                    || renderables && node.mesh.as_ref().is_some_and(MeshAsset::has_lights))
    }) {
        while !needed[index] {
            needed[index] = true;
            let Parent::Node(parent) = parents[index] else {
                break;
            };
            index = parent;
        }
    }
    needed
}

#[derive(Clone, Copy)]
struct SceneSocket {
    transform: SceneTransform,
    rotation: UnitQuaternion<f32>,
}

fn resolve_sockets(
    nodes: &[SceneNode],
    parents: &[Parent],
    prototypes: &[Option<usize>],
    cache: &mut SceneMeshCache,
) -> HashMap<usize, Option<SceneSocket>> {
    let mut sockets = HashMap::new();
    let mut resolved = HashMap::new();
    for (index, node) in nodes.iter().enumerate() {
        let Some(selector) = node.parent_node.as_deref() else {
            continue;
        };
        let socket = (|| {
            let Parent::Node(parent) = parents[index] else {
                return None;
            };
            let source = prototypes.get(parent).copied().flatten().unwrap_or(parent);
            let host = &nodes[source];
            let catalog = host.node_catalog.as_ref()?;
            let ordinal = catalog.selector_ordinal(selector)?;
            *resolved.entry((source, ordinal)).or_insert_with(|| {
                let asset = host.mesh.as_ref()?;
                let matrix = if let Some(rig) = asset.animated_primary() {
                    let key = (host.row, Arc::as_ptr(rig) as usize);
                    let pose = cache.socket_poses.entry(key).or_insert_with(|| {
                        Arc::new(rig.sample_pose(&host.animation, host.mounts_asset_attachments()))
                    });
                    *pose.matrices.get(ordinal)?.as_ref()?
                } else {
                    catalog.entries().get(ordinal)?.rest_matrix()
                };
                let transform = SceneTransform::identity().mesh_node(&matrix)?;
                let rotation = matrix_orientation(matrix)?;
                Some(SceneSocket {
                    transform,
                    rotation,
                })
            })
        })();
        sockets.insert(index, socket);
    }
    sockets
}

fn world_poses(
    nodes: &[SceneNode],
    parents: &[Parent],
    sockets: &HashMap<usize, Option<SceneSocket>>,
    track_rotation: bool,
    renderables: bool,
) -> Vec<Pose> {
    let rotation_nodes = if track_rotation {
        rotation_ancestry(nodes, parents, renderables)
    } else {
        Vec::new()
    };
    let tracks_rotation = |index| rotation_nodes.get(index).copied().unwrap_or(false);
    let mut poses = vec![Pose::Pending; nodes.len()];
    let mut trail = Vec::new();
    for index in 0..nodes.len() {
        if !matches!(poses[index], Pose::Pending) {
            continue;
        }
        let mut current = index;
        let mut inherited = loop {
            match poses[current] {
                Pose::Ready(world) => break Some(world),
                Pose::Unavailable | Pose::Resolving => break None,
                Pose::Pending => {}
            }
            let node = &nodes[current];
            match parents[current] {
                Parent::Root => {
                    let root = WorldPose::root(node, tracks_rotation(current));
                    poses[current] = Pose::Ready(root);
                    break Some(root);
                }
                Parent::Invalid => {
                    poses[current] = Pose::Unavailable;
                    break None;
                }
                Parent::Node(parent) => {
                    poses[current] = Pose::Resolving;
                    trail.push(current);
                    current = parent;
                }
            }
        };
        for child in trail.drain(..).rev() {
            inherited = inherited.and_then(|mut world| {
                if let Some(socket) = sockets.get(&child) {
                    let socket = (*socket)?;
                    world.transform = world.transform.child(socket.transform)?;
                    world.rotation = world.rotation.map(|rotation| rotation * socket.rotation);
                }
                world.child(&nodes[child], tracks_rotation(child))
            });
            poses[child] = inherited.map_or(Pose::Unavailable, Pose::Ready);
        }
    }
    poses
}

/// Number each subtree once, independently of draw order and camera LOD.
fn light_subtrees(parents: &[Parent]) -> Vec<[u16; 2]> {
    let root = parents.len();
    let mut children = vec![None; root + 1];
    let mut siblings = vec![None; root];
    for (index, parent) in parents.iter().enumerate().rev() {
        let parent = match parent {
            Parent::Root => root,
            Parent::Node(parent) => *parent,
            Parent::Invalid => continue,
        };
        siblings[index] = children[parent].replace(index);
    }

    let mut ranges = vec![[0; 2]; root];
    let mut stack = Vec::new();
    let mut child = children[root];
    let mut number = 0;
    while child.is_some() || !stack.is_empty() {
        if let Some(node) = child {
            number += 1;
            ranges[node][0] = number;
            stack.push(node);
            child = children[node];
        } else if let Some(node) = stack.pop() {
            ranges[node][1] = number;
            child = siblings[node];
        }
    }
    ranges
}

#[doc(hidden)]
pub fn model_nodes<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<SceneNode>,
) -> Arc<SceneModel> {
    prepare_model(model, project, true, true)
}

#[doc(hidden)]
pub fn model_cameras<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<SceneNode>,
) -> Arc<SceneModel> {
    prepare_model(model, project, false, true)
}

pub(crate) fn prepare_model<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<SceneNode>,
    renderables: bool,
    observe_rows: bool,
) -> Arc<SceneModel> {
    let project_row = |index, row| {
        let mut node = project(row)?;
        node.row = index as u32;
        Some(node)
    };
    let nodes: Vec<_> = if observe_rows {
        // A live model can change through another handle after this read.
        (0..model.len().min(MAX_NODES))
            .filter_map(|index| project_row(index, model.row(index)?))
            .collect()
    } else {
        model
            .inspect_rows(0, MAX_NODES, T::clone)
            .1
            .into_iter()
            .enumerate()
            .filter_map(|(index, row)| project_row(index, row))
            .collect()
    };
    let bindings: Vec<_> = nodes
        .iter()
        .enumerate()
        .filter_map(|(index, node)| node.physics.as_ref().map(|_| index))
        .collect();
    let original = (!bindings.is_empty()).then(|| nodes.clone());
    let mut meshes = SceneMeshCache::default();
    let mut scene = prepare_nodes(nodes, renderables, &mut meshes);
    if let Some(nodes) = original {
        scene.physics = Some(Box::new(PhysicsScene {
            nodes,
            bindings,
            cache: Mutex::new(PhysicsSceneCache {
                meshes,
                projection: None,
            }),
        }));
    }
    Arc::new(scene)
}

fn prepare_nodes(
    mut nodes: Vec<SceneNode>,
    renderables: bool,
    cache: &mut SceneMeshCache,
) -> SceneModel {
    let mut alternatives: HashMap<String, Vec<LodLevel>> = HashMap::new();
    nodes.retain_mut(|node| {
        if node.lod_of.is_empty() {
            return true;
        }
        if renderables
            && node.visible
            && node.lod_max_size.is_finite()
            && node.lod_max_size > 0.0
            && let Some(mesh) = node.mesh.take()
        {
            alternatives
                .entry(std::mem::take(&mut node.lod_of))
                .or_default()
                .push(LodLevel {
                    max_size: node.lod_max_size,
                    mesh,
                    posed: OnceLock::new(),
                });
        }
        false
    });
    let has_instance_roots = nodes.iter().any(|node| !node.instance_root.is_empty());
    let has_sockets = nodes.iter().any(|node| node.parent_node.is_some());
    let prototypes = if renderables || has_instance_roots || has_sockets {
        resolve_prototypes(&mut nodes, renderables)
    } else {
        Vec::new()
    };
    let track_cameras = nodes
        .iter()
        .any(|node| node.camera.is_some() && !node.id.is_empty());
    let track_imported = renderables
        && nodes.iter().any(|node| {
            node.mounts_asset_attachments() && node.mesh.as_ref().is_some_and(MeshAsset::has_lights)
        });
    let track_imported_cameras = nodes.iter().any(|node| {
        !node.id.is_empty()
            && node.mounts_asset_attachments()
            && node.mesh.as_ref().is_some_and(MeshAsset::has_cameras)
    });
    let mut imported_cameras = ImportedCameras::new();
    let mut imported_lights = None;
    let mut collision_rows = Vec::new();
    if nodes.iter().all(|row| {
        row.id.is_empty()
            && row.parent.is_empty()
            && row.parent_node.is_none()
            && row.instance_root.is_empty()
            && !matches!(row.mesh, Some(MeshAsset::Lods(_)))
    }) {
        let mut objects = Vec::new();
        if renderables {
            for mut node in nodes.into_iter().filter(|node| node.visible) {
                let world = WorldPose::root(&node, track_imported);
                collision_rows.extend(node.collision_row(MeshMount::at(world.transform)));
                node.append_scene(
                    (world, Some(MeshMount::at(world.transform))),
                    Some(&mut objects),
                    Some(&mut imported_lights),
                    None,
                    None,
                    cache,
                );
            }
        }
        return SceneModel {
            geometry: SceneGeometry::new(objects, 0),
            collision_rows,
            collision_geometry: OnceLock::new(),
            cameras: BTreeMap::new(),
            imported_lights: imported_lights.map(Arc::from),
            frames: HashMap::new(),
            lod_groups: Vec::new(),
            lod_selection: SceneLodSelection::Primary,
            physics: None,
        };
    }
    let ids = node_indices(&nodes);
    let parents: Vec<_> = nodes
        .iter()
        .map(|node| {
            if node.parent_node.is_some() && node.parent.is_empty() {
                Parent::Invalid
            } else {
                scene_reference(node.transform_parent(), &ids)
            }
        })
        .collect();
    let sockets = if has_sockets {
        resolve_sockets(&nodes, &parents, &prototypes, cache)
    } else {
        HashMap::new()
    };
    let subtrees = light_subtrees(&parents);
    let poses = world_poses(
        &nodes,
        &parents,
        &sockets,
        track_cameras || track_imported || track_imported_cameras,
        renderables,
    );
    let camera_owners = if track_imported_cameras {
        nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                ids.get(node.id.as_str()).copied().flatten() == Some(index)
                    && node.mounts_asset_attachments()
                    && node.mesh.as_ref().is_some_and(MeshAsset::has_cameras)
            })
            .collect()
    } else {
        Vec::new()
    };
    let mut frames: HashMap<_, _> = ids
        .iter()
        .filter_map(|(&id, index)| {
            let index = (*index)?;
            let Pose::Ready(world) = poses[index] else {
                return None;
            };
            let node = &nodes[index];
            Some((
                id.to_owned(),
                NodeFrame {
                    origin: world.transform.origin,
                    linear: world.transform.linear,
                    inverse: world.transform.inverse,
                    rotation: sockets
                        .get(&index)
                        .copied()
                        .flatten()
                        .filter(|_| !node.world_locked)
                        .map_or(node.rotation, |socket| {
                            socket.rotation.to_rotation_matrix() * node.rotation
                        }),
                    rotation_parent: if node.world_locked {
                        String::new()
                    } else {
                        node.transform_parent().to_owned()
                    },
                    visible: world.visible,
                    subtree: subtrees[index],
                    asset: None,
                },
            ))
        })
        .collect();
    let mut cameras = if track_cameras {
        nodes
            .iter()
            .zip(&poses)
            .enumerate()
            .filter_map(|(index, (node, pose))| {
                let Pose::Ready(world) = pose else {
                    return None;
                };
                if !world.visible || ids.get(node.id.as_str()).copied().flatten() != Some(index) {
                    return None;
                }
                let settings = node.camera?;
                let mut rotation = world.rotation?;
                rotation.renormalize();
                let mut camera = CameraNode {
                    eye: world.transform.origin,
                    rotation,
                    settings,
                    lod_bias: node.camera_lod_bias,
                    layers: node.layers,
                };
                let target = frames
                    .get(&node.look_at)
                    .filter(|frame| frame.visible)
                    .map(|frame| {
                        let [x, y, z] = <[f32; 3]>::from(frame.origin).map(f64::from);
                        Point3D::new(x, y, z)
                    })
                    .or(node.look_at_point);
                if let Some(target) = target {
                    camera.aim_at(target, node.camera_up);
                }
                Some((node.id.clone(), camera))
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    let by_index: HashMap<_, _> = alternatives
        .into_iter()
        .filter_map(|(id, levels)| Some((ids.get(&*id).copied().flatten()?, levels)))
        .collect();
    let instance_roots: Vec<_> = if has_instance_roots && renderables {
        nodes
            .iter()
            .map(|node| scene_reference(&node.instance_root, &ids))
            .collect()
    } else {
        Vec::new()
    };
    // Preserve primary row order, including each replacement's position in it.
    let mut objects = Vec::new();
    let mut lod_groups = Vec::new();
    if renderables || track_imported_cameras {
        for (index, (mut node, pose)) in nodes.into_iter().zip(poses.iter().copied()).enumerate() {
            if let Pose::Ready(world) = pose
                && world.visible
            {
                node.lighting_index = subtrees[index][0];
                let prototype = prototypes.get(index).copied().flatten();
                let root = instance_roots.get(index).copied().unwrap_or(Parent::Root);
                let mount = MeshMount::resolve(world.transform, root, prototype, &poses);
                let (levels, automatic) = if renderables && mount.is_some() {
                    let primary = prototype.unwrap_or(index);
                    by_index.get(&primary).map_or_else(
                        || (node.automatic_levels(), true),
                        |levels| {
                            let levels = levels
                                .iter()
                                .map(|level| LodLevel {
                                    max_size: level.max_size,
                                    mesh: level.mesh.clone(),
                                    posed: OnceLock::new(),
                                })
                                .collect();
                            (levels, false)
                        },
                    )
                } else {
                    (Vec::new(), false)
                };
                if renderables && let Some(mount) = mount {
                    collision_rows.extend(node.collision_row(mount));
                }
                let candidate = mount
                    .filter(|_| !levels.is_empty())
                    .map(|mount| (node.clone(), mount, levels, automatic));
                let start = objects.len();
                let owns_cameras = camera_owners.get(index).copied().unwrap_or(false);
                let node_frame = if node.node_catalog.is_some() {
                    frames.get_mut(&node.id).map(|frame| &mut frame.asset)
                } else {
                    None
                };
                node.append_scene(
                    (world, mount),
                    renderables.then_some(&mut objects),
                    renderables.then_some(&mut imported_lights),
                    owns_cameras.then_some(&mut imported_cameras),
                    node_frame,
                    cache,
                );
                let end = objects.len();
                if let Some((mut source, mount, mut levels, automatic)) = candidate
                    && let Some(bounds) = objects[start..end]
                        .iter()
                        .map(SceneObject::world_bounds)
                        .reduce(Bounds3D::union)
                {
                    levels.sort_by(|a, b| a.max_size.total_cmp(&b.max_size));
                    levels.dedup_by(|a, b| a.max_size == b.max_size);
                    // Keep only the selected level in the render key. Equivalent factors
                    // leave the posed geometry and cached viewport eligible for reuse.
                    let factor = source.instancing_lod_factor.take();
                    let fixed_level = if source.lod_bias == 0.0 {
                        Some(0)
                    } else {
                        factor.map(|factor| {
                            let step = (factor * levels.len() as f64).round() as usize;
                            // Levels are stored from coarsest to finest, preceded by the primary.
                            if step == 0 {
                                0
                            } else {
                                levels.len() + 1 - step
                            }
                        })
                    };
                    let morph_error_scale = if automatic && fixed_level.is_none() {
                        objects[start..end]
                            .iter()
                            .filter_map(SceneObject::morph_weights)
                            .flat_map(|weights| weights.iter().copied())
                            .map(|weight| f64::from(weight.abs()))
                            .fold(1.0, f64::max)
                    } else {
                        1.0
                    };
                    lod_groups.push(LodGroup {
                        range: start..end,
                        source,
                        mount,
                        levels,
                        fixed_level,
                        morph_error_scale,
                        bounds,
                    });
                }
            }
        }
    }
    for (id, camera) in imported_cameras {
        if let Some(camera) = camera {
            cameras.entry(id).or_insert(camera);
        }
    }
    SceneModel {
        geometry: SceneGeometry::new(objects, 0),
        collision_rows,
        collision_geometry: OnceLock::new(),
        cameras,
        imported_lights: imported_lights.map(Arc::from),
        frames,
        lod_selection: SceneLodSelection::for_groups(&lod_groups),
        lod_groups,
        physics: None,
    }
}

impl Node {
    /// Resolve rigid-body poses and place particle emitters in the same scene snapshot.
    #[doc(hidden)]
    pub fn prepare_scene_view(&mut self, follow_authored_model: bool) {
        if follow_authored_model
            && let (Some(Value::SceneObjects(model)), Some(Value::PhysicsWorld3D(world))) = (
                self.properties.get("model"),
                self.properties.get("physics-world"),
            )
            && let Some(model) = model.with_physics(world)
        {
            self.properties.insert("model", Value::SceneObjects(model));
        }
        self.prepare_particle_attachments();
        self.prepare_affector_attachments();
        self.prepare_physics_debug();
    }
}

#[doc(hidden)]
pub fn record_objects(
    list: &RecordList,
    imported: impl Fn(&str) -> Option<MeshAsset>,
    textures: impl Fn(&str) -> Option<Arc<crate::scene3d::MeshImage>>,
) -> Arc<SceneModel> {
    prepare_record_scene(
        list,
        imported,
        (textures, |_| None),
        (true, true),
        &SceneMeshInputs::default(),
        &CameraMatrixCache::new(),
    )
}

#[doc(hidden)]
pub fn record_cameras(
    list: &RecordList,
    imported: impl Fn(&str) -> Option<MeshAsset>,
) -> Arc<SceneModel> {
    prepare_record_scene(
        list,
        imported,
        (|_| None, |_| None),
        (false, true),
        &SceneMeshInputs::default(),
        &CameraMatrixCache::new(),
    )
}

pub(crate) fn prepare_record_scene(
    list: &RecordList,
    imported: impl Fn(&str) -> Option<MeshAsset>,
    (textures, shaders): (
        impl Fn(&str) -> Option<Arc<crate::scene3d::MeshImage>>,
        impl Fn(&str) -> Option<Arc<str>>,
    ),
    (renderables, observe_rows): (bool, bool),
    mesh_inputs: &SceneMeshInputs,
    camera_matrices: &CameraMatrixCache,
) -> Arc<SceneModel> {
    prepare_model(
        list.model(),
        |row| {
            let number = |name| match row.field(name)? {
                Value::Float(value) => Some(*value),
                _ => None,
            };
            let string = |name| match row.field(name)? {
                Value::String(value) => Some(value.as_str()),
                _ => None,
            };
            let enabled = |name| row.field(name) != Some(&Value::Bool(false));
            let Value::Color(color) = row.field("color")? else {
                return None;
            };
            let morph_weights: Vec<_> = match (renderables, row.field("morph-weights")) {
                (true, Some(Value::Record(weights))) => weights
                    .fields()
                    .filter_map(|(name, value)| match value {
                        Value::Float(value) => Some((name, *value)),
                        _ => None,
                    })
                    .collect(),
                _ => Vec::new(),
            };
            let instance_of = string("instance-of").unwrap_or_default();
            let geometry = if instance_of.is_empty() {
                row.field("geometry").and_then(|value| {
                    let Value::Record(geometry) = value else {
                        return None;
                    };
                    if let Some(Value::RecordList(vertices)) = geometry.field("vertices") {
                        let indices = match geometry.field("indices") {
                            Some(Value::IntModel(indices)) => SceneMeshIndices::Model(indices),
                            Some(Value::IntList(indices)) => SceneMeshIndices::Slice(indices),
                            _ => SceneMeshIndices::Sequential,
                        };
                        let subsets = match geometry.field("subsets") {
                            Some(Value::RecordList(subsets)) => {
                                mesh_inputs.subsets(subsets.model(), SceneMeshSubset::from_record)
                            }
                            _ => Some(Vec::new()),
                        };
                        mesh_inputs.geometry(
                            vertices.model(),
                            indices,
                            subsets,
                            SceneMeshVertex::from_record,
                        )
                    } else {
                        SceneParametricGeometry::from_record(geometry)
                            .map(SceneGeometryInput::Parametric)
                    }
                })
            } else {
                None
            };
            SceneNodeDefinition {
                id: string("id")?.to_owned(),
                parent: string("parent").unwrap_or_default().to_owned(),
                parent_node: string("parent-node").unwrap_or_default().to_owned(),
                physics_body: string("physics-body").unwrap_or_default(),
                mesh: string("mesh").unwrap_or_default(),
                mesh_asset: match row.field("mesh-asset") {
                    Some(Value::MeshAsset3D(asset)) => Some(asset),
                    _ => None,
                },
                geometry,
                collision_shapes: match (renderables, row.field("collision-shapes")) {
                    (true, Some(Value::RecordList(shapes))) => {
                        prepare_collision_shapes(mesh_inputs, shapes.model(), |shape| {
                            SceneCollisionShape::from_record(shape, &imported)
                        })
                    }
                    _ => None,
                },
                material_variant: string("material-variant").unwrap_or_default(),
                instance_of: instance_of.to_owned(),
                instance_root: string("instance-root").unwrap_or_default().to_owned(),
                instance_data: match row.field("instance-data") {
                    Some(Value::Record(data)) => {
                        Some(["x", "y", "z", "w"].map(|name| match data.field(name) {
                            Some(Value::Float(value)) => *value,
                            _ => 0.0,
                        }))
                    }
                    _ => None,
                },
                instance_template: row.field("instance-template") == Some(&Value::Bool(true)),
                instance_table: match row.field("instance-table") {
                    Some(Value::InstanceTable3D(table)) if renderables => table.clone(),
                    _ => InstanceTable3D::default(),
                },
                instance_count_override: match row.field("instance-count-override") {
                    Some(Value::Int(count)) => *count,
                    _ => -1,
                },
                instance_has_transparency: row.field("instance-has-transparency")
                    == Some(&Value::Bool(true)),
                instance_depth_sorting: row.field("instance-depth-sorting")
                    == Some(&Value::Bool(true)),
                instance_shadow_bounds: match (
                    row.field("instance-shadow-bounds-min"),
                    row.field("instance-shadow-bounds-max"),
                ) {
                    (Some(Value::Vector3D(min)), Some(Value::Vector3D(max))) => Some([*min, *max]),
                    _ => None,
                },
                random_instances: match (renderables, row.field("random-instances")) {
                    (true, Some(Value::Record(settings))) => {
                        Some(SceneRandomInstances::from_record(settings))
                    }
                    _ => None,
                },
                animation: MeshAnimationBlend {
                    from: MeshPlayback {
                        clip: string("animation").unwrap_or_default().to_owned(),
                        time: number("animation-time").unwrap_or(0.0),
                        looping: row.field("animation-loop") == Some(&Value::Bool(true)),
                    },
                    to: MeshPlayback {
                        clip: string("animation-to").unwrap_or_default().to_owned(),
                        time: number("animation-to-time").unwrap_or(0.0),
                        looping: row.field("animation-to-loop") == Some(&Value::Bool(true)),
                    },
                    weight: number("animation-blend").unwrap_or(0.0),
                    layers: match row.field("animation-layers") {
                        Some(Value::RecordList(layers)) => prepare_animation_layers(
                            mesh_inputs,
                            layers.model(),
                            MeshAnimationLayer::from_record,
                        ),
                        _ => Arc::from([]),
                    },
                },
                morph_weights: &morph_weights,
                position: [number("x")?, number("y")?, number("z")?],
                scale: [number("scale-x")?, number("scale-y")?, number("scale-z")?],
                angles: [number("yaw")?, number("pitch")?, number("roll")?],
                orientation: match row.field("orientation") {
                    Some(Value::Quaternion3D(value)) => Some(*value),
                    _ => None,
                },
                pivot: ["pivot-x", "pivot-y", "pivot-z"].map(|name| number(name).unwrap_or(0.0)),
                visible: enabled("visible"),
                look_at: string("camera-look-at").unwrap_or_default().to_owned(),
                look_at_point: match row.field("camera-look-at-point") {
                    Some(Value::Point3D(point)) => Some(*point),
                    _ => None,
                },
                camera_up: match row.field("camera-up") {
                    Some(Value::Vector3D(up)) => Some(*up),
                    _ => None,
                },
                camera: string("camera-kind")
                    .filter(|kind| !kind.is_empty())
                    .and_then(|kind| {
                        if kind == "custom" {
                            let Value::Record(matrix) = row.field("camera-projection")? else {
                                return None;
                            };
                            return SceneCameraSettings::custom_from_record(
                                matrix,
                                string("camera-clip-space").unwrap_or_default(),
                                camera_matrices,
                            );
                        }
                        SceneCameraSettings::from_record(
                            kind,
                            number("camera-fov").unwrap_or(45.0),
                            number("camera-height").unwrap_or(10.0),
                            number("camera-near").unwrap_or(0.1),
                            number("camera-far").unwrap_or(100.0),
                            ["camera-left", "camera-right", "camera-bottom", "camera-top"]
                                .map(number),
                            number("camera-width"),
                        )
                        .and_then(|settings| {
                            let normal = match row.field("camera-clip-normal") {
                                Some(Value::Vector3D(normal)) => Some(*normal),
                                _ => None,
                            };
                            settings.with_clip_normal(normal)
                        })
                    })
                    .map(|settings| {
                        settings.with_reverse_depth(
                            row.field("camera-reverse-depth") == Some(&Value::Bool(true)),
                        )
                    }),
                color: *color,
                depth_bias: number("depth-bias").unwrap_or_default(),
                motion_vector_enabled: enabled("motion-vector-enabled"),
                motion_vector_scale: number("motion-vector-scale").unwrap_or(1.0),
                instancing_lod_range: [
                    number("instancing-lod-min").unwrap_or(0.0),
                    number("instancing-lod-max").unwrap_or(-1.0),
                ],
                instancing_lod_factor: number("instancing-lod-factor").unwrap_or(-1.0),
                material: match (renderables, row.field("material")) {
                    (true, Some(Value::Record(style))) => {
                        crate::scene3d::SceneMaterial::from_record(style, &textures, &shaders)
                    }
                    _ => None,
                },
                material_slots: match (renderables, row.field("material-slots")) {
                    (true, Some(Value::Record(palette))) => palette
                        .fields()
                        .filter_map(|(name, value)| {
                            let Value::Record(record) = value else {
                                return None;
                            };
                            let style = crate::scene3d::SceneMaterial::from_record(
                                record, &textures, &shaders,
                            )?;
                            let slot = match record.field("slot") {
                                Some(Value::String(slot)) => Some(slot.as_str()),
                                _ => None,
                            };
                            crate::scene3d::SceneMaterialBinding::for_field(name, slot, style)
                        })
                        .collect(),
                    _ => Vec::new(),
                },
                lightmap: row.field("lightmap").and_then(|image| {
                    BakedLightmap::from_value(
                        image,
                        string("lightmap-mode").unwrap_or_default(),
                        number("lightmap-intensity").unwrap_or(1.0),
                        [
                            number("lightmap-u").unwrap_or(0.0),
                            number("lightmap-v").unwrap_or(0.0),
                            number("lightmap-width").unwrap_or(1.0),
                            number("lightmap-height").unwrap_or(1.0),
                        ],
                    )
                }),
                bake_role: string("bake-role").unwrap_or_default(),
                lightmap_resolution: LightmapResolution::from_settings(
                    match row.field("lightmap-size") {
                        Some(Value::Int(size)) => *size,
                        _ => 0,
                    },
                    number("lightmap-texels-per-unit").unwrap_or(0.0),
                ),
                casts_shadow: enabled("casts-shadow"),
                receives_shadow: enabled("receives-shadow"),
                receives_reflections: enabled("receives-reflections"),
                casts_reflections: enabled("casts-reflections"),
                layers: match row.field("layers") {
                    Some(Value::Int(layers)) => *layers as u64,
                    _ => MAIN_LAYER,
                },
                pickable: enabled("pickable"),
                collidable: enabled("collidable"),
                trigger: row.field("trigger") == Some(&Value::Bool(true)),
                lod_of: string("lod-of").unwrap_or_default().to_owned(),
                lod_max_size: match row.field("lod-max-size") {
                    Some(Value::Length(length)) => length.0,
                    _ => 0.0,
                },
                lod_bias: number("lod-bias").unwrap_or(1.0),
                camera_lod_bias: number("camera-lod-bias").unwrap_or(1.0),
            }
            .prepare(&imported, renderables)
        },
        renderables,
        observe_rows,
    )
}

#[cfg(test)]
mod indexed_ray_tests {
    use super::*;
    use crate::scene3d::{Ray3D, RayRows, SceneRay, raycast};
    use std::{hint::black_box, time::Instant};

    #[test]
    fn distance_masks_keep_instance_order_at_the_interval_boundary() {
        let box_at_origin =
            SceneObject::cuboid("box", [0.0; 3], [1.0; 3], [0.0; 3], Color(0xffffffff))
                .expect("test box");
        let mut objects = vec![box_at_origin.clone(); 65];
        objects[0] = box_at_origin.clone().with_instancing_lod_range([0.0, 10.0]);
        objects[64] = box_at_origin.with_instancing_lod_range([10.0, -1.0]);
        let geometry = SceneGeometry::new(objects, 0);
        let near = geometry
            .distance_visibility([0.0, 0.0, 9.0])
            .expect("near mask");
        let far = geometry
            .distance_visibility([0.0, 0.0, 10.0])
            .expect("far mask");
        let farther = geometry
            .distance_visibility([0.0, 0.0, 11.0])
            .expect("reused mask");
        assert_eq!(near.runs(0..65).collect::<Vec<_>>(), [0..64]);
        assert_eq!(far.runs(0..65).collect::<Vec<_>>(), [1..65]);
        assert!(Arc::ptr_eq(&far, &farther));
        let shared = SceneVisibility::intersect(&near, &far);
        assert_eq!(shared.visible_count(), 63);
        assert_eq!(shared.runs(0..65).collect::<Vec<_>>(), [1..64]);
    }

    #[test]
    fn repeated_scene_rays_prune_large_scenes_and_keep_coplanar_order() {
        const SIDE: usize = 64;
        let cube = |id: String, center: [f32; 3]| {
            SceneObject::cuboid(id, center, [0.8; 3], [0.0; 3], Color(0xffffffff))
                .expect("test box has a valid transform")
        };
        let mut objects = vec![
            cube("first".into(), [0.0; 3]).with_source_row(0),
            cube("second".into(), [0.0; 3]).with_source_row(1),
            cube("behind".into(), [0.0, 0.0, -2.0]).with_source_row(2),
        ];
        for y in 0..SIDE {
            for x in 0..SIDE {
                let center = [(x as f32 - 31.0) * 3.0, (y as f32 - 31.0) * 3.0, 0.0];
                if center != [0.0; 3] {
                    objects.push(cube(format!("{x},{y}"), center));
                }
            }
        }
        let geometry = SceneGeometry::new(objects, 0);
        let ray = Ray3D::new([0.0, 0.0, 5.0], [0.0, 0.0, -1.0]).expect("test ray");
        let query = SceneRay::new(ray, 0.0, 12.0, u64::MAX);
        let first = query.first(&geometry, &RayRows::All);
        let linear = raycast(&geometry, ray, 12.0).expect("ray reaches the center");
        assert_eq!(first.object_id, "first");
        assert_eq!(first, linear.into());
        let hits = query.all(&geometry, &RayRows::All).snapshot();
        assert_eq!(
            hits.iter()
                .map(|hit| hit.object_id.as_str())
                .collect::<Vec<_>>(),
            ["first", "second", "behind"]
        );
        let span = BoundsSegment {
            origin: Vector3::new(0.0, 0.0, 5.0),
            direction: -Vector3::z(),
            min: 0.0,
            max: 12.0,
            expansion: Vector3::zeros(),
        };
        let mut candidates = 0;
        geometry.visit_ray(span, |_, limit| {
            candidates += 1;
            limit
        });
        assert!(candidates <= 24, "{candidates} candidates");

        let start = Instant::now();
        for _ in 0..512 {
            black_box(query.first(black_box(&geometry), &RayRows::All));
        }
        let indexed = start.elapsed();
        let start = Instant::now();
        for _ in 0..512 {
            black_box(raycast(black_box(&geometry), ray, 12.0));
        }
        let scan = start.elapsed();
        eprintln!(
            "{}/{} object candidates; 512 warm rays: indexed {indexed:?}, linear {scan:?}",
            candidates,
            geometry.len()
        );
    }
}
