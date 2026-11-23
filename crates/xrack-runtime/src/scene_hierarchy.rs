//! Local-to-world scene transforms shared by both UI backends and ray queries.
use std::{
    collections::{BTreeMap, HashMap, VecDeque},
    ops::{Deref, Range},
    sync::{Arc, Mutex, OnceLock},
};

use nalgebra::{Matrix3, Matrix4, Rotation3, UnitQuaternion, Vector3};

use crate::{
    Color, RecordList, Value, VecModel,
    scene_animation::{
        AnimatedMesh, MeshAnimationBlend, MeshPlacement, MeshPlayback, MeshSampleOptions,
    },
    scene_bounds::Bounds3D,
    scene_camera::{CameraNode, SceneCamera, SceneCameraSettings},
    scene_lighting::{MAX_SCENE_LIGHTS, SceneLight},
    scene_lightmap::BakedLightmap,
    scene_pick::BoundsSegment,
    scene_visibility::{SceneVisibility, VisibilityIndex},
    scene3d::{MAIN_LAYER, Mesh3D, MeshAsset, SceneObject},
};

const MAX_NODES: usize = 4096;

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct SceneTransform {
    pub linear: Matrix3<f32>,
    pub origin: Vector3<f32>,
    pub inverse: Matrix3<f32>,
}

impl SceneTransform {
    pub(crate) fn local(
        position: [f32; 3],
        scale: [f32; 3],
        angles: [f32; 3],
        pivot: [f32; 3],
    ) -> Option<(Self, Rotation3<f32>)> {
        if !position
            .into_iter()
            .chain(pivot)
            .all(|value| value.is_finite() && value.abs() <= 200.0)
            || !scale
                .into_iter()
                .all(|value| (0.0001..=200.0).contains(&value))
            || !angles.into_iter().all(f32::is_finite)
        {
            return None;
        }
        let rotation = euler_rotation(angles);
        let mut linear = *rotation.matrix();
        let mut inverse = linear.transpose();
        for (axis, scale) in scale.into_iter().enumerate() {
            linear.column_mut(axis).scale_mut(scale);
            inverse.row_mut(axis).unscale_mut(scale);
        }
        // The model-space pivot maps to the authored position.
        let origin = Vector3::from(position) - linear * Vector3::from(pivot);
        Some((
            Self {
                linear,
                origin,
                inverse,
            },
            rotation,
        ))
    }

    fn child(self, local: Self) -> Option<Self> {
        let transform = Self {
            linear: self.linear * local.linear,
            origin: self.linear * local.origin + self.origin,
            inverse: local.inverse * self.inverse,
        };
        // A strongly ill-conditioned transform has no reliable f32 inverse
        // for either rasterization or picking.
        transform
            .linear
            .iter()
            .chain(transform.origin.iter())
            .chain(transform.inverse.iter())
            .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
            .then_some(transform)
    }

    fn mesh_node(self, node: &Matrix4<f32>) -> Option<(Self, bool)> {
        let mut linear = self.linear * node.fixed_view::<3, 3>(0, 0);
        let origin = self.origin + self.linear * node.fixed_view::<3, 1>(0, 3);
        let mirrored = linear.determinant() < 0.0;
        if mirrored {
            // Use a resident reflected geometry variant instead of changing
            // the front-face winding of the whole viewport.
            linear.column_mut(0).scale_mut(-1.0);
        }
        let inverse = linear.try_inverse()?;
        linear
            .iter()
            .chain(origin.iter())
            .chain(inverse.iter())
            .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
            .then_some((
                Self {
                    linear,
                    origin: origin.into_owned(),
                    inverse,
                },
                mirrored,
            ))
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

/// An authored row's local transform and renderable payload.
#[doc(hidden)]
pub struct SceneNodeDefinition<'a> {
    pub id: String,
    pub parent: String,
    pub mesh: &'a str,
    pub animation: MeshAnimationBlend,
    pub morph_weights: &'a [(&'a str, f64)],
    pub position: [f64; 3],
    pub scale: [f64; 3],
    pub angles: [f64; 3],
    pub pivot: [f64; 3],
    pub visible: bool,
    pub camera: Option<SceneCameraSettings>,
    pub look_at: String,
    pub color: Color,
    pub lightmap: Option<BakedLightmap>,
    pub casts_shadow: bool,
    pub receives_shadow: bool,
    pub receives_reflections: bool,
    pub casts_reflections: bool,
    pub layers: u64,
    pub pickable: bool,
    pub collidable: bool,
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
    local: SceneTransform,
    rotation: Rotation3<f32>,
    camera: Option<SceneCameraSettings>,
    look_at: String,
    visible: bool,
    mesh: Option<MeshAsset>,
    animation: MeshAnimationBlend,
    morph_weights: Vec<(String, f64)>,
    color: Color,
    lightmap: Option<BakedLightmap>,
    casts_shadow: bool,
    receives_shadow: bool,
    receives_reflections: bool,
    casts_reflections: bool,
    layers: u64,
    pickable: bool,
    collidable: bool,
    row: u32,
    lod_of: String,
    lod_max_size: f64,
    lod_bias: f64,
    camera_lod_bias: f64,
}

impl SceneNodeDefinition<'_> {
    pub fn prepare(self, imported: impl Fn(&str) -> Option<MeshAsset>) -> Option<SceneNode> {
        let mesh = if self.mesh.is_empty() {
            None
        } else {
            Some(MeshAsset::resolve(self.mesh, imported)?)
        };
        let narrow = |values: [f64; 3]| values.map(|value| value as f32);
        let (local, rotation) = if self.lod_of.is_empty() {
            SceneTransform::local(
                narrow(self.position),
                narrow(self.scale),
                narrow(self.angles),
                narrow(self.pivot),
            )?
        } else {
            // A replacement inherits the primary row's transform and appearance.
            (
                SceneTransform {
                    linear: Matrix3::identity(),
                    origin: Vector3::zeros(),
                    inverse: Matrix3::identity(),
                },
                Rotation3::identity(),
            )
        };
        Some(SceneNode {
            id: self.id,
            parent: self.parent,
            mesh,
            animation: self.animation,
            morph_weights: self
                .morph_weights
                .iter()
                .map(|&(name, weight)| (name.to_owned(), weight))
                .collect(),
            visible: self.visible,
            local,
            rotation,
            camera: self.camera,
            look_at: self.look_at,
            color: self.color,
            lightmap: self.lightmap,
            casts_shadow: self.casts_shadow,
            receives_shadow: self.receives_shadow,
            receives_reflections: self.receives_reflections,
            casts_reflections: self.casts_reflections,
            layers: self.layers,
            pickable: self.pickable,
            collidable: self.collidable,
            row: 0,
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

impl SceneNode {
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

    fn append_scene(
        mut self,
        world: WorldPose,
        objects: Option<&mut Vec<SceneObject>>,
        lights: Option<&mut Option<Vec<SceneLight>>>,
        cameras: Option<&mut ImportedCameras>,
    ) {
        if let Some(asset) = self.mesh.take() {
            let attachments = ImportAttachments {
                rotation: world.rotation,
                lights: lights
                    .filter(|_| asset.has_lights() && world.rotation.is_some())
                    .map(|lights| lights.get_or_insert_with(Vec::new)),
                cameras: cameras.filter(|_| asset.has_cameras() && world.rotation.is_some()),
            };
            self.append_mesh(asset, world.transform, objects, attachments);
        }
    }

    fn append_mesh(
        self,
        asset: MeshAsset,
        world: SceneTransform,
        objects: Option<&mut Vec<SceneObject>>,
        attachments: ImportAttachments<'_>,
    ) {
        let renderables = objects
            .as_ref()
            .is_some_and(|objects| objects.len() < MAX_NODES);
        if !renderables && attachments.lights.is_none() && attachments.cameras.is_none() {
            return;
        }
        let object = |id, mesh: crate::scene3d::Mesh3D, transform| {
            let lightmap = self.lightmap.as_ref().filter(|_| {
                !mesh.lightmap_uvs().is_empty()
                    && mesh.skin_geometry().is_none()
                    && mesh.morph_geometry().is_none()
            });
            let mut object = SceneObject::from_transform(id, mesh, transform, self.color)
                .with_shadows(self.casts_shadow, self.receives_shadow)
                .with_pickable(self.pickable)
                .with_collidable(self.collidable)
                .with_source_row(self.row);
            object.receives_reflections = self.receives_reflections;
            object.casts_reflections = self.casts_reflections;
            object.layers = self.layers;
            object.lightmap = lightmap.cloned();
            object
        };
        match asset {
            MeshAsset::Lods(lods) => {
                self.append_mesh(lods.primary().clone(), world, objects, attachments)
            }
            MeshAsset::Static(mesh) => {
                if let Some(objects) = objects.filter(|_| renderables) {
                    objects.push(object(self.id, mesh, world));
                }
            }
            MeshAsset::Animated(rig) => {
                let weights: Vec<_> = self
                    .morph_weights
                    .iter()
                    .filter(|_| renderables)
                    .map(|(name, weight)| (name.as_str(), *weight))
                    .collect();
                let overrides = rig.morph_overrides(&weights);
                let mut positioned_lights = Vec::new();
                let mut positioned_cameras = Vec::new();
                let collect_lights = attachments
                    .lights
                    .as_ref()
                    .is_some_and(|lights| lights.len() < MAX_SCENE_LIGHTS);
                let placements = rig.sample(
                    &self.animation,
                    MeshSampleOptions {
                        renderables,
                        overrides: &overrides,
                        lights: collect_lights.then_some(&mut positioned_lights),
                        cameras: attachments
                            .cameras
                            .is_some()
                            .then_some(&mut positioned_cameras),
                    },
                );
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
                for placement in placements {
                    if objects.len() == MAX_NODES {
                        break;
                    }
                    if let Some(placed) = placed_mesh(&rig, placement, world, |mesh, transform| {
                        object(self.id.clone(), mesh, transform)
                    }) {
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
    placement: MeshPlacement,
    world: SceneTransform,
    make_object: impl FnOnce(Mesh3D, SceneTransform) -> SceneObject,
) -> Option<SceneObject> {
    let (transform, mirrored) = world.mesh_node(&placement.transform)?;
    let mesh = rig.geometry(placement.mesh, mirrored)?;
    let mut object = make_object(mesh, transform);
    if let Some(weights) = placement.morph {
        object = object.with_morph(weights)?;
    }
    if let Some(palette) = placement.skin {
        object = object.with_skin(palette)?;
    }
    Some(object)
}

/// Primary geometry in the authored glTF rest pose, using the same node and
/// deformation transforms as a model drawn without an animation clip.
pub(crate) fn rest_mesh_objects(asset: &MeshAsset) -> Vec<SceneObject> {
    let identity = SceneTransform {
        linear: Matrix3::identity(),
        origin: Vector3::zeros(),
        inverse: Matrix3::identity(),
    };
    let object = |mesh, transform| {
        SceneObject::from_transform(String::new(), mesh, transform, Color(0xffffffff))
    };
    match asset {
        MeshAsset::Lods(lods) => rest_mesh_objects(lods.primary()),
        MeshAsset::Static(mesh) => vec![object(mesh.clone(), identity)],
        MeshAsset::Animated(rig) => rig
            .sample(
                &MeshAnimationBlend::default(),
                MeshSampleOptions {
                    renderables: true,
                    overrides: &[],
                    lights: None,
                    cameras: None,
                },
            )
            .into_iter()
            .filter_map(|placement| placed_mesh(rig, placement, identity, object))
            .collect(),
    }
}

type ImportedCameras = BTreeMap<String, Option<CameraNode>>;

#[derive(Default)]
struct ImportAttachments<'a> {
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
    shared_layers: u64,
    layer_selections: Mutex<VecDeque<(u64, Arc<SceneVisibility>)>>,
    reflection_casters: OnceLock<Option<Arc<SceneVisibility>>>,
    pub lod_objects: usize,
}

impl SceneGeometry {
    fn new(objects: Vec<SceneObject>, lod_objects: usize) -> Arc<Self> {
        let shared_layers = objects
            .iter()
            .fold(u64::MAX, |shared, object| shared & object.layers);
        Arc::new(Self {
            objects: objects.into(),
            visibility: OnceLock::new(),
            shared_layers,
            layer_selections: Mutex::default(),
            reflection_casters: OnceLock::new(),
            lod_objects,
        })
    }

    pub fn objects(&self) -> &Arc<[SceneObject]> {
        &self.objects
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
    world: SceneTransform,
    levels: Vec<LodLevel>,
    morph_error_scale: f64,
    // Derived bounds of the primary posed renderables, independent of the replacement.
    bounds: Bounds3D,
}

impl LodGroup {
    fn select(&self, camera: &SceneCamera) -> usize {
        let bias = self.source.lod_bias * camera.lod_bias;
        if bias <= 0.0 {
            return 0;
        }
        let size = camera.lod_size(self.bounds) * self.morph_error_scale;
        if !size.is_finite() {
            return 0;
        }
        self.levels
            .iter()
            .position(|level| size <= level.max_size * bias)
            .map_or(0, |index| index + 1)
    }

    fn replacement(&self, selection: usize) -> Option<&[SceneObject]> {
        let level = self.levels.get(selection.checked_sub(1)?)?;
        let posed = level.posed.get_or_init(|| {
            let mut objects = Vec::new();
            self.source.clone().append_mesh(
                level.mesh.clone(),
                self.world,
                Some(&mut objects),
                ImportAttachments::default(),
            );
            objects.into()
        });
        (!posed.is_empty()).then_some(posed)
    }
}

type ViewSelections = VecDeque<(Vec<usize>, Arc<SceneGeometry>)>;

/// Renderables and named camera poses resolved from the same authored rows.
#[derive(Debug)]
pub struct SceneModel {
    geometry: Arc<SceneGeometry>,
    cameras: BTreeMap<String, CameraNode>,
    imported_lights: Option<Arc<[SceneLight]>>,
    frames: HashMap<String, NodeFrame>,
    lod_groups: Vec<LodGroup>,
    selections: Mutex<ViewSelections>,
}

impl PartialEq for SceneModel {
    fn eq(&self, other: &Self) -> bool {
        self.objects() == other.objects()
            && self.cameras == other.cameras
            && self.imported_lights == other.imported_lights
            && self.frames == other.frames
            && self.lod_groups == other.lod_groups
    }
}

#[derive(Debug, PartialEq)]
pub(crate) struct NodeFrame {
    pub origin: Vector3<f32>,
    pub linear: Matrix3<f32>,
    rotation: Rotation3<f32>,
    parent: String,
}

impl SceneModel {
    pub(crate) fn visit_sweep(
        &self,
        mut span: BoundsSegment,
        mut visit: impl FnMut(usize, f64) -> f64,
    ) {
        if self.is_empty() {
            return;
        }
        if let Some(bounds) = self
            .geometry
            .visibility
            .get_or_init(|| VisibilityIndex::new(&self.geometry.objects))
        {
            bounds.visit_sweep(span, visit);
        } else {
            for index in 0..self.len() {
                span.max = visit(index, span.max);
            }
        }
    }

    pub fn objects(&self) -> &Arc<[SceneObject]> {
        self.geometry.objects()
    }

    pub fn imported_lights(&self) -> Option<Arc<[SceneLight]>> {
        self.imported_lights.clone()
    }

    pub fn camera(&self, id: &str, viewport: [f64; 2]) -> Option<SceneCamera> {
        SceneCamera::from_node(self.cameras.get(id)?, viewport)
    }

    #[doc(hidden)]
    pub fn geometry(&self, camera: &SceneCamera) -> Arc<SceneGeometry> {
        if self.lod_groups.is_empty() || camera.lod_bias == 0.0 {
            return self.geometry.clone();
        }
        let key: Vec<_> = self
            .lod_groups
            .iter()
            .map(|group| group.select(camera))
            .collect();
        if key.iter().all(|&level| level == 0) {
            return self.geometry.clone();
        }
        let mut selections = self
            .selections
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = selections.iter().position(|(old, _)| *old == key)
            && let Some(entry) = selections.remove(index)
        {
            let result = entry.1.clone();
            selections.push_front(entry);
            return result;
        }

        let mut objects = Vec::with_capacity(self.len());
        let mut lod_objects = 0;
        let mut cursor = 0;
        let mut append = |slice: &[SceneObject]| {
            let count = slice.len().min(MAX_NODES - objects.len());
            objects.extend_from_slice(&slice[..count]);
        };
        for (group, &level) in self.lod_groups.iter().zip(&key) {
            append(&self[cursor..group.range.start]);
            if let Some(replacement) = group.replacement(level) {
                lod_objects += 1;
                append(replacement);
            } else {
                append(&self[group.range.clone()]);
            }
            cursor = group.range.end;
        }
        append(&self[cursor..]);
        let result = SceneGeometry::new(objects, lod_objects);
        selections.truncate(3);
        selections.push_front((key, result.clone()));
        result
    }

    pub(crate) fn node_frame(&self, id: &str) -> Option<&NodeFrame> {
        self.frames.get(id)
    }

    pub(crate) fn rotate_node_direction<'a>(
        &'a self,
        mut frame: &'a NodeFrame,
        mut direction: Vector3<f32>,
    ) -> Vector3<f32> {
        loop {
            direction = frame.rotation * direction;
            let Some(parent) = self.frames.get(&frame.parent) else {
                break;
            };
            frame = parent;
        }
        direction.normalize()
    }
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
}

impl WorldPose {
    fn root(node: &SceneNode, track_cameras: bool) -> Self {
        Self {
            transform: node.local,
            rotation: track_cameras.then(|| UnitQuaternion::from_rotation_matrix(&node.rotation)),
        }
    }

    fn child(self, node: &SceneNode, camera_ancestor: bool) -> Option<Self> {
        let transform = self.transform.child(node.local)?;
        let rotation = self
            .rotation
            .filter(|_| camera_ancestor)
            .map(|parent| parent * UnitQuaternion::from_rotation_matrix(&node.rotation));
        Some(Self {
            transform,
            rotation,
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
    Hidden,
    Ready(WorldPose),
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

fn rotation_ancestry(nodes: &[SceneNode], parents: &[Parent], renderables: bool) -> Vec<bool> {
    // Only view and imported light chains need a separate, scale-free orientation.
    let mut needed = vec![false; nodes.len()];
    for (mut index, _) in nodes.iter().enumerate().filter(|(_, node)| {
        node.camera.is_some()
            || (!node.id.is_empty() && node.mesh.as_ref().is_some_and(MeshAsset::has_cameras))
            || renderables && node.mesh.as_ref().is_some_and(MeshAsset::has_lights)
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

fn world_poses(
    nodes: &[SceneNode],
    ids: &HashMap<&str, Option<usize>>,
    track_rotation: bool,
    renderables: bool,
) -> Vec<Pose> {
    let parents: Vec<_> = nodes
        .iter()
        .map(|node| {
            if node.parent.is_empty() {
                Parent::Root
            } else {
                ids.get(node.parent.as_str())
                    .copied()
                    .flatten()
                    .map_or(Parent::Invalid, Parent::Node)
            }
        })
        .collect();
    let rotation_nodes = if track_rotation {
        rotation_ancestry(nodes, &parents, renderables)
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
                Pose::Hidden | Pose::Resolving => break None,
                Pose::Pending => {}
            }
            let node = &nodes[current];
            if !node.visible {
                poses[current] = Pose::Hidden;
                break None;
            }
            match parents[current] {
                Parent::Root => {
                    let root = WorldPose::root(node, tracks_rotation(current));
                    poses[current] = Pose::Ready(root);
                    break Some(root);
                }
                Parent::Invalid => {
                    poses[current] = Pose::Hidden;
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
            inherited =
                inherited.and_then(|world| world.child(&nodes[child], tracks_rotation(child)));
            poses[child] = inherited.map_or(Pose::Hidden, Pose::Ready);
        }
    }
    poses
}

#[doc(hidden)]
pub fn model_nodes<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<SceneNode>,
) -> Arc<SceneModel> {
    prepare_model(model, project, true)
}

#[doc(hidden)]
pub fn model_cameras<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<SceneNode>,
) -> Arc<SceneModel> {
    prepare_model(model, project, false)
}

fn prepare_model<T: Clone>(
    model: &VecModel<T>,
    project: impl Fn(T) -> Option<SceneNode>,
    renderables: bool,
) -> Arc<SceneModel> {
    // Observe rows as well as list structure so both render paths update when
    // a parent moves, disappears, or changes visibility.
    let mut nodes: Vec<_> = (0..model.len().min(MAX_NODES))
        .filter_map(|index| {
            let mut node = project(model.row(index)?)?;
            node.row = index as u32;
            Some(node)
        })
        .collect();
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
    let track_cameras = nodes
        .iter()
        .any(|node| node.camera.is_some() && !node.id.is_empty());
    let track_imported = renderables
        && nodes
            .iter()
            .any(|node| node.mesh.as_ref().is_some_and(MeshAsset::has_lights));
    let track_imported_cameras = nodes
        .iter()
        .any(|node| !node.id.is_empty() && node.mesh.as_ref().is_some_and(MeshAsset::has_cameras));
    let mut imported_cameras = ImportedCameras::new();
    let mut imported_lights = None;
    if nodes.iter().all(|row| {
        row.id.is_empty() && row.parent.is_empty() && !matches!(row.mesh, Some(MeshAsset::Lods(_)))
    }) {
        let mut objects = Vec::new();
        if renderables {
            for node in nodes.into_iter().filter(|node| node.visible) {
                let world = WorldPose::root(&node, track_imported);
                node.append_scene(world, Some(&mut objects), Some(&mut imported_lights), None);
            }
        }
        return Arc::new(SceneModel {
            geometry: SceneGeometry::new(objects, 0),
            cameras: BTreeMap::new(),
            imported_lights: imported_lights.map(Arc::from),
            frames: HashMap::new(),
            lod_groups: Vec::new(),
            selections: Mutex::default(),
        });
    }
    let ids = node_indices(&nodes);
    let poses = world_poses(
        &nodes,
        &ids,
        track_cameras || track_imported || track_imported_cameras,
        renderables,
    );
    let camera_owners = if track_imported_cameras {
        nodes
            .iter()
            .enumerate()
            .map(|(index, node)| {
                ids.get(node.id.as_str()).copied().flatten() == Some(index)
                    && node.mesh.as_ref().is_some_and(MeshAsset::has_cameras)
            })
            .collect()
    } else {
        Vec::new()
    };
    let frames: HashMap<_, _> = ids
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
                    rotation: node.rotation,
                    parent: node.parent.clone(),
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
                if ids.get(node.id.as_str()).copied().flatten() != Some(index) {
                    return None;
                }
                let settings = node.camera?;
                let mut rotation = world.rotation?;
                rotation.renormalize();
                Some((
                    node.id.clone(),
                    CameraNode {
                        eye: world.transform.origin,
                        rotation,
                        target: frames.get(&node.look_at).map(|frame| frame.origin),
                        settings,
                        lod_bias: node.camera_lod_bias,
                        layers: node.layers,
                    },
                ))
            })
            .collect()
    } else {
        BTreeMap::new()
    };
    let mut by_index: HashMap<_, _> = alternatives
        .into_iter()
        .filter_map(|(id, levels)| Some((ids.get(&*id).copied().flatten()?, levels)))
        .collect();
    // Preserve primary row order, including each replacement's position in it.
    let mut objects = Vec::new();
    let mut lod_groups = Vec::new();
    if renderables || track_imported_cameras {
        for (index, (node, pose)) in nodes.into_iter().zip(poses).enumerate() {
            if let Pose::Ready(world) = pose {
                let (levels, automatic) = if renderables {
                    by_index
                        .remove(&index)
                        .map_or_else(|| (node.automatic_levels(), true), |levels| (levels, false))
                } else {
                    (Vec::new(), false)
                };
                let candidate = (!levels.is_empty()).then(|| (node.clone(), levels, automatic));
                let start = objects.len();
                let owns_cameras = camera_owners.get(index).copied().unwrap_or(false);
                node.append_scene(
                    world,
                    renderables.then_some(&mut objects),
                    renderables.then_some(&mut imported_lights),
                    owns_cameras.then_some(&mut imported_cameras),
                );
                let end = objects.len();
                if let Some((source, mut levels, automatic)) = candidate
                    && let Some(bounds) = objects[start..end]
                        .iter()
                        .map(SceneObject::world_bounds)
                        .reduce(Bounds3D::union)
                {
                    levels.sort_by(|a, b| a.max_size.total_cmp(&b.max_size));
                    levels.dedup_by(|a, b| a.max_size == b.max_size);
                    let morph_error_scale = if automatic {
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
                        world: world.transform,
                        levels,
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
    Arc::new(SceneModel {
        geometry: SceneGeometry::new(objects, 0),
        cameras,
        imported_lights: imported_lights.map(Arc::from),
        frames,
        lod_groups,
        selections: Mutex::default(),
    })
}

#[doc(hidden)]
pub fn record_objects(
    list: &RecordList,
    imported: impl Fn(&str) -> Option<MeshAsset>,
) -> Arc<SceneModel> {
    prepare_record_scene(list, imported, true)
}

#[doc(hidden)]
pub fn record_cameras(
    list: &RecordList,
    imported: impl Fn(&str) -> Option<MeshAsset>,
) -> Arc<SceneModel> {
    prepare_record_scene(list, imported, false)
}

fn prepare_record_scene(
    list: &RecordList,
    imported: impl Fn(&str) -> Option<MeshAsset>,
    renderables: bool,
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
            SceneNodeDefinition {
                id: string("id")?.to_owned(),
                parent: string("parent").unwrap_or_default().to_owned(),
                mesh: string("mesh")?,
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
                },
                morph_weights: &morph_weights,
                position: [number("x")?, number("y")?, number("z")?],
                scale: [number("scale-x")?, number("scale-y")?, number("scale-z")?],
                angles: [number("yaw")?, number("pitch")?, number("roll")?],
                pivot: ["pivot-x", "pivot-y", "pivot-z"].map(|name| number(name).unwrap_or(0.0)),
                visible: enabled("visible"),
                look_at: string("camera-look-at").unwrap_or_default().to_owned(),
                camera: string("camera-kind")
                    .filter(|kind| !kind.is_empty())
                    .and_then(|kind| {
                        SceneCameraSettings::from_record(
                            kind,
                            number("camera-fov").unwrap_or(45.0),
                            number("camera-height").unwrap_or(10.0),
                            number("camera-near").unwrap_or(0.1),
                            number("camera-far").unwrap_or(100.0),
                            ["camera-left", "camera-right", "camera-bottom", "camera-top"]
                                .map(number),
                        )
                    }),
                color: *color,
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
                lod_of: string("lod-of").unwrap_or_default().to_owned(),
                lod_max_size: match row.field("lod-max-size") {
                    Some(Value::Length(length)) => length.0,
                    _ => 0.0,
                },
                lod_bias: number("lod-bias").unwrap_or(1.0),
                camera_lod_bias: number("camera-lod-bias").unwrap_or(1.0),
            }
            .prepare(&imported)
        },
        renderables,
    )
}
