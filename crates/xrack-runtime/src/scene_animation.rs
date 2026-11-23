//! Prepared glTF node geometry and keyframes shared by both XRack backends.
use crate::{
    scene_camera::{CameraNode, SceneCameraSettings},
    scene_lighting::{MAX_SCENE_LIGHTS, SceneLight, SceneLightSource},
    scene_morph::MorphOverrides,
    scene3d::{JointPalette, MAX_MORPH_TARGETS, Mesh3D, MeshSkin, MorphWeights},
};
use nalgebra::{Matrix3, Matrix4, Quaternion, Rotation3, Translation3, UnitQuaternion, Vector3};
use std::{
    cmp::Ordering,
    collections::BTreeSet,
    sync::{Arc, OnceLock},
};

pub const MAX_ANIMATION_NODES: usize = 4096;
pub const MAX_ANIMATION_KEYS: usize = 1_000_000;

/// Prepared geometry for a static mesh or an animated glTF scene.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub enum MeshAsset {
    Static(Mesh3D),
    Animated(Arc<AnimatedMesh>),
    Lods(Arc<MeshLods>),
}

impl PartialEq for MeshAsset {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Static(a), Self::Static(b)) => a == b,
            (Self::Animated(a), Self::Animated(b)) => Arc::ptr_eq(a, b),
            (Self::Lods(a), Self::Lods(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl MeshAsset {
    pub(crate) fn has_cameras(&self) -> bool {
        match self {
            Self::Animated(rig) => !rig.cameras.is_empty(),
            Self::Lods(lods) => lods.primary().has_cameras(),
            Self::Static(_) => false,
        }
    }

    pub(crate) fn has_lights(&self) -> bool {
        match self {
            Self::Animated(rig) => !rig.lights.is_empty(),
            Self::Lods(lods) => lods.primary().has_lights(),
            Self::Static(_) => false,
        }
    }

    pub fn resolve(key: &str, imported: impl FnOnce(&str) -> Option<Self>) -> Option<Self> {
        match key {
            "primitive:box" => Some(Self::Static(Mesh3D::Box)),
            "primitive:sphere" => Some(Self::Static(Mesh3D::Sphere)),
            _ => imported(key),
        }
    }
}

#[doc(hidden)]
#[derive(Debug)]
pub struct MeshLodLevel {
    pub max_size: f64,
    pub meshes: Box<[Mesh3D]>,
    replacement: OnceLock<MeshAsset>,
}

#[doc(hidden)]
#[derive(Debug)]
pub struct MeshLods {
    primary: MeshAsset,
    levels: Box<[MeshLodLevel]>,
}

impl MeshLods {
    pub fn primary(&self) -> &MeshAsset {
        &self.primary
    }

    pub fn levels(&self) -> &[MeshLodLevel] {
        &self.levels
    }

    pub(crate) fn replacement(&self, level: &MeshLodLevel) -> MeshAsset {
        level
            .replacement
            .get_or_init(|| match &self.primary {
                MeshAsset::Static(_) => MeshAsset::Static(level.meshes[0].clone()),
                MeshAsset::Animated(rig) => {
                    MeshAsset::Animated(Arc::new(rig.with_meshes(level.meshes.to_vec())))
                }
                MeshAsset::Lods(_) => self.primary.clone(),
            })
            .clone()
    }
}

impl MeshAsset {
    /// Meshes in primary asset order, including rigid nodes of imported rigs.
    #[doc(hidden)]
    pub fn geometry_meshes(&self) -> &[Mesh3D] {
        match self {
            Self::Static(mesh) => std::slice::from_ref(mesh),
            Self::Animated(rig) => rig.meshes(),
            Self::Lods(lods) => lods.primary.geometry_meshes(),
        }
    }

    /// Sample authored joint poses for the asset simplifier.
    #[doc(hidden)]
    pub fn lod_joint_palettes(&self) -> Vec<Vec<JointPalette>> {
        let Self::Animated(rig) = self else {
            return Vec::new();
        };
        rig.lod_joint_palettes()
    }

    /// Attach levels produced by the resource pipeline or deserialized by AOT.
    #[doc(hidden)]
    pub fn with_lods(self, levels: Vec<(f64, Vec<Mesh3D>)>) -> Self {
        let primary = match self {
            Self::Lods(lods) => lods.primary.clone(),
            value => value,
        };
        let source = primary.geometry_meshes();
        let mut levels: Vec<_> = levels
            .into_iter()
            .filter_map(|(max_size, meshes)| {
                if !max_size.is_finite() || max_size <= 0.0 || meshes.len() != source.len() {
                    return None;
                }
                if source
                    .iter()
                    .zip(&meshes)
                    .any(|(base, level)| !base.compatible_lod(level))
                {
                    return None;
                }
                Some(MeshLodLevel {
                    max_size,
                    meshes: meshes.into(),
                    replacement: OnceLock::new(),
                })
            })
            .collect();
        levels.sort_by(|a, b| a.max_size.total_cmp(&b.max_size));
        levels.dedup_by(|a, b| a.max_size == b.max_size);
        if levels.is_empty() {
            primary
        } else {
            Self::Lods(Arc::new(MeshLods {
                primary,
                levels: levels.into(),
            }))
        }
    }
}

#[derive(Clone, Copy, Debug)]
pub enum MeshNodeTransform {
    Matrix([[f32; 4]; 4]),
    Trs {
        translation: [f32; 3],
        rotation: [f32; 4],
        scale: [f32; 3],
    },
}

impl MeshNodeTransform {
    fn channel(self, property: MeshAnimationProperty) -> Option<[f32; 4]> {
        let Self::Trs {
            translation,
            rotation,
            scale,
        } = self
        else {
            return None;
        };
        Some(match property {
            MeshAnimationProperty::Translation => {
                let [x, y, z] = translation;
                [x, y, z, 0.0]
            }
            MeshAnimationProperty::Scale => {
                let [x, y, z] = scale;
                [x, y, z, 0.0]
            }
            MeshAnimationProperty::Rotation => rotation,
        })
    }

    fn set_channel(&mut self, property: MeshAnimationProperty, value: [f32; 4]) {
        let Self::Trs {
            translation,
            rotation,
            scale,
        } = self
        else {
            return;
        };
        let [x, y, z, _] = value;
        match property {
            MeshAnimationProperty::Translation => *translation = [x, y, z],
            MeshAnimationProperty::Scale => *scale = [x, y, z],
            MeshAnimationProperty::Rotation => *rotation = value,
        }
    }

    fn matrix(self) -> Option<Matrix4<f32>> {
        Some(match self {
            Self::Matrix(columns) => {
                if columns.iter().flatten().any(|value| !value.is_finite())
                    || columns.map(|column| column[3]) != [0.0, 0.0, 0.0, 1.0]
                {
                    return None;
                }
                Matrix4::from_fn(|row, column| columns[column][row])
            }
            Self::Trs {
                translation,
                rotation,
                scale,
            } => {
                if !translation.into_iter().chain(scale).all(f32::is_finite) {
                    return None;
                }
                Translation3::from(Vector3::from(translation)).to_homogeneous()
                    * quaternion(rotation)?.to_homogeneous()
                    * Matrix4::new_nonuniform_scaling(&Vector3::from(scale))
            }
        })
    }
}

fn matrix_orientation(matrix: Matrix4<f32>) -> Option<UnitQuaternion<f32>> {
    let basis = matrix.fixed_view::<3, 3>(0, 0);
    // glTF TRS decomposition assigns a reflection to the first scale axis.
    let x = basis.column(0).into_owned().try_normalize(1e-12)?
        * if basis.determinant() < 0.0 { -1.0 } else { 1.0 };
    let y = basis.column(1).into_owned().try_normalize(1e-12)?;
    let z = basis.column(2).into_owned().try_normalize(1e-12)?;
    Some(UnitQuaternion::from_rotation_matrix(
        &Rotation3::from_matrix_eps(
            &Matrix3::from_columns(&[x, y, z]),
            1e-6,
            32,
            Rotation3::identity(),
        ),
    ))
}

fn quaternion([x, y, z, w]: [f32; 4]) -> Option<UnitQuaternion<f32>> {
    let value = Quaternion::new(w, x, y, z);
    let length = value.norm();
    if !length.is_finite() || length <= 1e-8 {
        return None;
    }
    Some(UnitQuaternion::new_normalize(value))
}

fn interpolate_rotation(a: [f32; 4], b: [f32; 4], t: f32) -> Option<[f32; 4]> {
    let a = quaternion(a)?;
    let mut b = quaternion(b)?;
    if a.coords.dot(&b.coords) < 0.0 {
        b = UnitQuaternion::new_unchecked(-b.into_inner());
    }
    let q = a.try_slerp(&b, t, 1e-6).unwrap_or_else(|| a.nlerp(&b, t));
    Some([q.i, q.j, q.k, q.w])
}

#[derive(Debug)]
pub struct MeshAnimationNode {
    pub parent: Option<usize>,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
    pub morph_weights: MorphWeights,
    pub transform: MeshNodeTransform,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum MeshAnimationProperty {
    Translation,
    Rotation,
    Scale,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MeshInterpolation {
    Step,
    Linear,
    CubicSpline,
}

#[derive(Debug)]
pub struct MeshAnimationTrack {
    pub node: usize,
    pub property: MeshAnimationProperty,
    pub interpolation: MeshInterpolation,
    pub times: Vec<f32>,
    pub values: Vec<[f32; 4]>,
}

impl MeshAnimationTrack {
    #[doc(hidden)]
    pub fn from_packed(
        node: usize,
        property: MeshAnimationProperty,
        interpolation: MeshInterpolation,
        times: &[u8],
        values: &[u8],
    ) -> Option<Self> {
        if !times.len().is_multiple_of(4)
            || !values.len().is_multiple_of(16)
            || times.len() / 4 > MAX_ANIMATION_KEYS
            || values.len() / 16 > MAX_ANIMATION_KEYS * 3
        {
            return None;
        }
        let times = times
            .as_chunks::<4>()
            .0
            .iter()
            .copied()
            .map(f32::from_le_bytes)
            .collect();
        let values = values
            .as_chunks::<16>()
            .0
            .iter()
            .map(|value| std::array::from_fn(|i| f32::from_le_bytes(value.as_chunks::<4>().0[i])))
            .collect();
        Some(Self {
            node,
            property,
            interpolation,
            times,
            values,
        })
    }

    fn valid(&self, nodes: &[MeshAnimationNode]) -> bool {
        let count = self.times.len();
        let cubic = self.interpolation == MeshInterpolation::CubicSpline;
        let stride = if cubic { 3 } else { 1 };
        matches!(
            nodes.get(self.node).map(|node| node.transform),
            Some(MeshNodeTransform::Trs { .. })
        ) && count > usize::from(cubic)
            && count <= MAX_ANIMATION_KEYS
            && self.values.len() == count * stride
            && self
                .times
                .iter()
                .all(|time| time.is_finite() && *time >= 0.0)
            && self.times.windows(2).all(|pair| pair[0] < pair[1])
            && self.values.iter().flatten().all(|value| value.is_finite())
            && (self.property != MeshAnimationProperty::Rotation
                || (0..count).all(|i| quaternion(self.key(i)).is_some()))
    }

    fn key(&self, index: usize) -> [f32; 4] {
        self.values[if self.interpolation == MeshInterpolation::CubicSpline {
            index * 3 + 1
        } else {
            index
        }]
    }

    fn sample(&self, time: f32) -> Option<[f32; 4]> {
        let after = self.times.partition_point(|key| *key <= time);
        if after == 0 {
            return Some(self.key(0));
        }
        let before = after - 1;
        let a = self.key(before);
        if after == self.times.len()
            || self.times[before] == time
            || self.interpolation == MeshInterpolation::Step
        {
            return Some(a);
        }
        let b = self.key(after);
        let delta = self.times[after] - self.times[before];
        let t = (time - self.times[before]) / delta;
        Some(match self.interpolation {
            MeshInterpolation::Step => a,
            MeshInterpolation::Linear if self.property == MeshAnimationProperty::Rotation => {
                interpolate_rotation(a, b, t)?
            }
            MeshInterpolation::Linear => std::array::from_fn(|i| a[i] * (1.0 - t) + b[i] * t),
            MeshInterpolation::CubicSpline => {
                let out = self.values[before * 3 + 2];
                let into = self.values[after * 3];
                let [wa, wo, wb, wi] = cubic_weights(t, delta);
                let value =
                    std::array::from_fn(|i| wa * a[i] + wo * out[i] + wb * b[i] + wi * into[i]);
                if self.property == MeshAnimationProperty::Rotation {
                    let q = quaternion(value)?;
                    [q.i, q.j, q.k, q.w]
                } else {
                    value
                }
            }
        })
    }
}

fn cubic_weights(t: f32, delta: f32) -> [f32; 4] {
    let t2 = t * t;
    let t3 = t2 * t;
    [
        2.0 * t3 - 3.0 * t2 + 1.0,
        (t3 - 2.0 * t2 + t) * delta,
        -2.0 * t3 + 3.0 * t2,
        (t3 - t2) * delta,
    ]
}

#[derive(Debug)]
pub struct MorphAnimationTrack {
    pub node: usize,
    pub width: usize,
    pub interpolation: MeshInterpolation,
    pub times: Vec<f32>,
    pub values: Vec<f32>,
}

impl MorphAnimationTrack {
    #[doc(hidden)]
    pub fn from_packed(
        node: usize,
        width: usize,
        interpolation: MeshInterpolation,
        times: &[u8],
        values: &[u8],
    ) -> Option<Self> {
        let parse = |bytes: &[u8]| {
            let (values, remainder) = bytes.as_chunks::<4>();
            if !remainder.is_empty() || values.len() > MAX_ANIMATION_KEYS * 3 {
                return None;
            }
            Some(values.iter().copied().map(f32::from_le_bytes).collect())
        };
        Some(Self {
            node,
            width,
            interpolation,
            times: parse(times)?,
            values: parse(values)?,
        })
    }

    fn valid(&self, nodes: &[MeshAnimationNode]) -> bool {
        let cubic = self.interpolation == MeshInterpolation::CubicSpline;
        let slot = if cubic { 3 } else { 1 };
        (1..=MAX_MORPH_TARGETS).contains(&self.width)
            && nodes
                .get(self.node)
                .is_some_and(|node| node.morph_weights.len() == self.width)
            && self.times.len() > usize::from(cubic)
            && self.values.len() == self.times.len() * self.width * slot
            && self
                .times
                .iter()
                .all(|time| time.is_finite() && *time >= 0.0)
            && self.times.windows(2).all(|pair| pair[0] < pair[1])
            && self.values.iter().all(|value| value.is_finite())
    }

    fn key(&self, index: usize) -> &[f32] {
        let start = self.width
            * if self.interpolation == MeshInterpolation::CubicSpline {
                index * 3 + 1
            } else {
                index
            };
        &self.values[start..start + self.width]
    }

    fn sample<'a>(&'a self, time: f32, scratch: &'a mut [f32; MAX_MORPH_TARGETS]) -> &'a [f32] {
        let after = self.times.partition_point(|key| *key <= time);
        let before = after.saturating_sub(1);
        let a = self.key(before);
        if after == 0
            || after == self.times.len()
            || self.times[before] == time
            || self.interpolation == MeshInterpolation::Step
        {
            return a;
        }
        let b = self.key(after);
        let weights = &mut scratch[..self.width];
        let delta = self.times[after] - self.times[before];
        let t = (time - self.times[before]) / delta;
        if self.interpolation == MeshInterpolation::CubicSpline {
            let [wa, wo, wb, wi] = cubic_weights(t, delta);
            let out = (before * 3 + 2) * self.width;
            let into = after * 3 * self.width;
            for (i, weight) in weights.iter_mut().enumerate() {
                *weight =
                    wa * a[i] + wo * self.values[out + i] + wb * b[i] + wi * self.values[into + i];
            }
        } else {
            for (i, weight) in weights.iter_mut().enumerate() {
                *weight = a[i] * (1.0 - t) + b[i] * t;
            }
        }
        weights
    }
}

#[derive(Debug)]
pub struct MeshAnimationClip {
    pub index: usize,
    pub name: Option<String>,
    pub tracks: Vec<MeshAnimationTrack>,
    pub morph_tracks: Vec<MorphAnimationTrack>,
}

impl MeshAnimationClip {
    pub fn duration(&self) -> f32 {
        self.tracks
            .iter()
            .filter_map(|track| track.times.last().copied())
            .chain(
                self.morph_tracks
                    .iter()
                    .filter_map(|track| track.times.last().copied()),
            )
            .fold(0.0, f32::max)
    }
}

/// One independently sampled input to a glTF animation crossfade.
#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshPlayback {
    pub clip: String,
    pub time: f64,
    pub looping: bool,
}

#[doc(hidden)]
#[derive(Clone, Debug, Default, PartialEq)]
pub struct MeshAnimationBlend {
    pub from: MeshPlayback,
    pub to: MeshPlayback,
    pub weight: f64,
}

#[derive(Clone, Copy, Default)]
struct ClipSample<'a> {
    clip: Option<&'a MeshAnimationClip>,
    time: f32,
}

impl<'a> ClipSample<'a> {
    fn same_pose(self, other: Self) -> bool {
        self.clip.map(|clip| clip.index) == other.clip.map(|clip| clip.index)
            && self.time == other.time
    }

    fn tracks(self) -> &'a [MeshAnimationTrack] {
        self.clip.map_or(&[], |clip| &clip.tracks)
    }

    fn morph_tracks(self) -> &'a [MorphAnimationTrack] {
        self.clip.map_or(&[], |clip| &clip.morph_tracks)
    }
}

/// Merge the two sorted sets of channels without allocating a second node pose.
fn channel_pairs<'a, T, K: Ord>(
    mut from: &'a [T],
    mut to: &'a [T],
    key: impl Fn(&T) -> K,
) -> impl Iterator<Item = (Option<&'a T>, Option<&'a T>)> {
    std::iter::from_fn(move || match (from.split_first(), to.split_first()) {
        (None, None) => None,
        (Some((a, rest)), None) => {
            from = rest;
            Some((Some(a), None))
        }
        (None, Some((b, rest))) => {
            to = rest;
            Some((None, Some(b)))
        }
        (Some((a, more_from)), Some((b, more_to))) => match key(a).cmp(&key(b)) {
            Ordering::Less => {
                from = more_from;
                Some((Some(a), None))
            }
            Ordering::Greater => {
                to = more_to;
                Some((None, Some(b)))
            }
            Ordering::Equal => {
                from = more_from;
                to = more_to;
                Some((Some(a), Some(b)))
            }
        },
    })
}

#[derive(Clone, Copy, Debug)]
pub struct MeshLight {
    pub node: usize,
    pub light: SceneLight,
}

#[derive(Clone, Debug)]
pub struct MeshCamera {
    pub node: usize,
    pub source_node: usize,
    pub name: Option<String>,
    pub lens: MeshCameraProjection,
}

#[derive(Clone, Copy, Debug)]
pub enum MeshCameraProjection {
    Perspective {
        yfov: f32,
        aspect: Option<f32>,
        near: f32,
        far: Option<f32>,
    },
    Orthographic {
        xmag: f32,
        ymag: f32,
        near: f32,
        far: f32,
    },
}

impl MeshCameraProjection {
    pub fn settings(self) -> Option<SceneCameraSettings> {
        match self {
            Self::Perspective {
                yfov,
                aspect,
                near,
                far,
            } => SceneCameraSettings::perspective(yfov, aspect, near, far),
            Self::Orthographic {
                xmag,
                ymag,
                near,
                far,
            } => SceneCameraSettings::orthographic(xmag, ymag, near, far),
        }
    }
}

pub(crate) struct CameraPlacement {
    pub source_node: usize,
    pub name: Option<String>,
    pub pose: CameraNode,
}

pub(crate) struct MeshSampleOptions<'a> {
    pub renderables: bool,
    pub overrides: &'a [Option<MorphOverrides>],
    pub lights: Option<&'a mut Vec<SceneLight>>,
    pub cameras: Option<&'a mut Vec<CameraPlacement>>,
}

#[derive(Debug)]
struct AttachmentFrames {
    needed: Vec<bool>,
    fixed_rotations: Vec<Option<UnitQuaternion<f32>>>,
}

impl AttachmentFrames {
    fn new(
        nodes: &[MeshAnimationNode],
        lights: &[MeshLight],
        cameras: &[MeshCamera],
    ) -> Option<Self> {
        if lights.is_empty() && cameras.is_empty() {
            return None;
        }
        let mut needed = vec![false; nodes.len()];
        for mut node in lights
            .iter()
            .map(|light| light.node)
            .chain(cameras.iter().map(|camera| camera.node))
        {
            while !needed[node] {
                needed[node] = true;
                let Some(parent) = nodes[node].parent else {
                    break;
                };
                node = parent;
            }
        }
        let fixed_rotations = nodes
            .iter()
            .zip(&needed)
            .map(|(node, &needed)| match node.transform {
                MeshNodeTransform::Matrix(_) if needed => {
                    matrix_orientation(node.transform.matrix()?)
                }
                _ => None,
            })
            .collect();
        Some(Self {
            needed,
            fixed_rotations,
        })
    }
}

#[derive(Debug)]
pub struct AnimatedMesh {
    meshes: Vec<Mesh3D>,
    nodes: Arc<[MeshAnimationNode]>,
    clips: Arc<[MeshAnimationClip]>,
    clip_durations: Arc<[f32]>,
    skins: Arc<[MeshSkin]>,
    lights: Arc<[MeshLight]>,
    cameras: Arc<[MeshCamera]>,
    camera_settings: Arc<[SceneCameraSettings]>,
    attachment_frames: Option<Arc<AttachmentFrames>>,
    mirrored: Vec<OnceLock<Option<Mesh3D>>>,
}

pub(crate) struct MeshPlacement {
    pub mesh: usize,
    pub transform: Matrix4<f32>,
    pub skin: Option<JointPalette>,
    pub morph: Option<MorphWeights>,
}

impl AnimatedMesh {
    pub fn new(
        meshes: Vec<Mesh3D>,
        nodes: Vec<MeshAnimationNode>,
        mut clips: Vec<MeshAnimationClip>,
        skins: Vec<MeshSkin>,
        lights: Vec<MeshLight>,
        cameras: Vec<MeshCamera>,
    ) -> Result<Self, &'static str> {
        if nodes.len() > MAX_ANIMATION_NODES
            || (lights.is_empty()
                && cameras.is_empty()
                && !nodes.iter().any(|node| node.mesh.is_some()))
            || cameras.len() > MAX_ANIMATION_NODES
            || cameras.iter().any(|camera| camera.node >= nodes.len())
            || lights.len() > MAX_ANIMATION_NODES
            || lights
                .iter()
                .any(|mount| mount.node >= nodes.len() || !mount.light.is_valid())
            || nodes.iter().enumerate().any(|(index, node)| {
                node.parent.is_some_and(|parent| parent >= index)
                    || node.mesh.is_some_and(|mesh| mesh >= meshes.len())
                    || node.transform.matrix().is_none()
            })
        {
            return Err("invalid animated glTF node graph");
        }
        if skins.iter().any(|skin| !skin.valid(nodes.len()))
            || nodes.iter().any(|node| match (node.mesh, node.skin) {
                (Some(mesh), Some(skin)) => {
                    let Some(skin) = skins.get(skin) else {
                        return true;
                    };
                    meshes[mesh]
                        .skin_geometry()
                        .is_none_or(|geometry| geometry.joint_count > skin.joints.len())
                }
                (Some(mesh), None) => meshes[mesh].skin_geometry().is_some(),
                (None, Some(_)) => true,
                (None, None) => false,
            })
        {
            return Err("invalid glTF skin joint or mesh references");
        }
        if nodes.iter().any(|node| {
            let count = node
                .mesh
                .and_then(|mesh| meshes[mesh].morph_geometry())
                .map_or(0, |morph| morph.target_count());
            node.morph_weights.len() != count
                || !node.morph_weights.iter().all(|value| value.is_finite())
        }) {
            return Err("invalid default glTF morph weights");
        }
        let mut keys = 0usize;
        let mut clip_indices = BTreeSet::new();
        for clip in &mut clips {
            let mut channels = BTreeSet::new();
            let mut weight_nodes = BTreeSet::new();
            if !clip_indices.insert(clip.index)
                || clip.tracks.iter().any(|track| {
                    keys = keys.saturating_add(track.times.len());
                    keys > MAX_ANIMATION_KEYS
                        || !channels.insert((track.node, track.property))
                        || !track.valid(&nodes)
                })
                || clip.morph_tracks.iter().any(|track| {
                    keys = keys.saturating_add(track.times.len().saturating_mul(track.width));
                    keys > MAX_ANIMATION_KEYS
                        || !weight_nodes.insert(track.node)
                        || !track.valid(&nodes)
                })
            {
                return Err("invalid glTF animation channels or keyframes");
            }
            clip.tracks
                .sort_unstable_by_key(|track| (track.node, track.property));
            clip.morph_tracks.sort_unstable_by_key(|track| track.node);
        }
        let clip_durations = clips.iter().map(MeshAnimationClip::duration).collect();
        let mirrored = (0..meshes.len()).map(|_| OnceLock::new()).collect();
        let camera_settings = cameras
            .iter()
            .map(|camera| camera.lens.settings())
            .collect::<Option<Vec<_>>>()
            .ok_or("invalid glTF camera projection")?;
        let attachment_frames = AttachmentFrames::new(&nodes, &lights, &cameras).map(Arc::new);
        Ok(Self {
            meshes,
            nodes: nodes.into(),
            clips: clips.into(),
            clip_durations,
            skins: skins.into(),
            lights: lights.into(),
            cameras: cameras.into(),
            camera_settings: camera_settings.into(),
            attachment_frames,
            mirrored,
        })
    }

    fn with_meshes(&self, meshes: Vec<Mesh3D>) -> Self {
        let mirrored = (0..meshes.len()).map(|_| OnceLock::new()).collect();
        Self {
            meshes,
            nodes: self.nodes.clone(),
            clips: self.clips.clone(),
            clip_durations: self.clip_durations.clone(),
            skins: self.skins.clone(),
            lights: self.lights.clone(),
            cameras: self.cameras.clone(),
            camera_settings: self.camera_settings.clone(),
            attachment_frames: self.attachment_frames.clone(),
            mirrored,
        }
    }

    pub fn meshes(&self) -> &[Mesh3D] {
        &self.meshes
    }
    pub fn nodes(&self) -> &[MeshAnimationNode] {
        &self.nodes
    }
    pub fn clips(&self) -> &[MeshAnimationClip] {
        &self.clips
    }
    pub fn skins(&self) -> &[MeshSkin] {
        &self.skins
    }

    pub fn lights(&self) -> &[MeshLight] {
        &self.lights
    }

    pub fn cameras(&self) -> &[MeshCamera] {
        &self.cameras
    }

    fn lod_joint_palettes(&self) -> Vec<Vec<JointPalette>> {
        if self.skins.is_empty() {
            return Vec::new();
        }
        const POSES_PER_MESH: usize = 9;
        let mut samples = vec![Vec::new(); self.meshes.len()];
        let mut record = |placements: Vec<MeshPlacement>| {
            for placement in placements {
                if let Some(palette) = placement.skin {
                    let poses = &mut samples[placement.mesh];
                    if poses.len() < POSES_PER_MESH && !poses.contains(&palette) {
                        poses.push(palette);
                    }
                }
            }
        };
        let motion_options = || MeshSampleOptions {
            renderables: true,
            overrides: &[],
            lights: None,
            cameras: None,
        };
        let mut motion = MeshAnimationBlend::default();
        record(self.sample(&motion, motion_options()));
        for clip in self.clips.iter().take(8) {
            motion.from.clip = format!("#{}", clip.index);
            let duration = f64::from(clip.duration());
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                motion.from.time = fraction * duration;
                record(self.sample(&motion, motion_options()));
            }
        }
        samples
    }

    fn clip(&self, name: &str) -> Option<usize> {
        if name.is_empty() {
            return None;
        }
        if let Some(index) = name
            .strip_prefix('#')
            .and_then(|index| index.parse::<usize>().ok())
        {
            return self.clips.iter().position(|clip| clip.index == index);
        }
        let mut matches = self
            .clips
            .iter()
            .enumerate()
            .filter(|(_, clip)| clip.name.as_deref() == Some(name));
        let (first, _) = matches.next()?;
        matches.next().is_none().then_some(first)
    }

    fn clip_sample(&self, playback: &MeshPlayback) -> ClipSample<'_> {
        let Some(index) = self.clip(&playback.clip) else {
            return ClipSample::default();
        };
        let duration = f64::from(self.clip_durations[index]);
        let time = if playback.time.is_finite() {
            playback.time.max(0.0)
        } else {
            0.0
        };
        let time = if playback.looping && duration > 0.0 {
            time % duration
        } else {
            time.min(duration)
        } as f32;
        ClipSample {
            clip: Some(&self.clips[index]),
            time,
        }
    }

    fn motion_samples(
        &self,
        motion: &MeshAnimationBlend,
    ) -> (ClipSample<'_>, Option<(ClipSample<'_>, f32)>) {
        let weight = if motion.weight.is_finite() {
            motion.weight.clamp(0.0, 1.0) as f32
        } else {
            0.0
        };
        if weight == 1.0 {
            return (self.clip_sample(&motion.to), None);
        }
        let from = self.clip_sample(&motion.from);
        let blend = (weight > 0.0)
            .then(|| self.clip_sample(&motion.to))
            .filter(|to| !from.same_pose(*to))
            .map(|to| (to, weight));
        (from, blend)
    }

    pub(crate) fn geometry(&self, mesh: usize, mirrored: bool) -> Option<Mesh3D> {
        if mirrored {
            self.mirrored[mesh]
                .get_or_init(|| self.meshes[mesh].mirrored_x())
                .clone()
        } else {
            Some(self.meshes[mesh].clone())
        }
    }

    pub(crate) fn morph_overrides(&self, fields: &[(&str, f64)]) -> Vec<Option<MorphOverrides>> {
        if fields.is_empty() {
            return Vec::new();
        }
        self.meshes
            .iter()
            .map(|mesh| {
                mesh.morph_geometry()
                    .and_then(|geometry| geometry.overrides(fields))
            })
            .collect()
    }

    pub(crate) fn sample(
        &self,
        motion: &MeshAnimationBlend,
        options: MeshSampleOptions<'_>,
    ) -> Vec<MeshPlacement> {
        let MeshSampleOptions {
            renderables,
            overrides: morph_overrides,
            lights,
            cameras,
        } = options;
        let mut transforms: Vec<_> = self.nodes.iter().map(|node| node.transform).collect();
        let (from, blend) = self.motion_samples(motion);
        if let Some((to, weight)) = blend {
            for (a, b) in channel_pairs(from.tracks(), to.tracks(), |track| {
                (track.node, track.property)
            }) {
                let Some(track) = a.or(b) else { continue };
                let rest = self.nodes[track.node].transform.channel(track.property);
                let a = a.and_then(|track| track.sample(from.time)).or(rest);
                let b = b.and_then(|track| track.sample(to.time)).or(rest);
                if let Some((a, b)) = a.zip(b) {
                    let value = if track.property == MeshAnimationProperty::Rotation {
                        interpolate_rotation(a, b, weight)
                    } else {
                        Some(std::array::from_fn(|i| {
                            a[i] * (1.0 - weight) + b[i] * weight
                        }))
                    };
                    if let Some(value) = value {
                        transforms[track.node].set_channel(track.property, value);
                    }
                }
            }
        } else {
            for track in from.tracks() {
                if let Some(value) = track.sample(from.time) {
                    transforms[track.node].set_channel(track.property, value);
                }
            }
        }
        let mut world: Vec<Option<Matrix4<f32>>> = Vec::with_capacity(self.nodes.len());
        let frames = self
            .attachment_frames
            .as_deref()
            .filter(|_| lights.is_some() || cameras.is_some());
        let mut orientations =
            frames.map(|_| Vec::<Option<UnitQuaternion<f32>>>::with_capacity(self.nodes.len()));
        for (index, (node, local)) in self.nodes.iter().zip(transforms).enumerate() {
            let matrix = local
                .matrix()
                .and_then(|local| {
                    Some(match node.parent {
                        Some(parent) => world[parent]? * local,
                        None => local,
                    })
                })
                .filter(|matrix| matrix.iter().all(|value| value.is_finite()));
            world.push(matrix);
            if let Some((frames, orientations)) = frames.zip(orientations.as_mut()) {
                let rotation = frames.needed[index]
                    .then(|| {
                        let local = match local {
                            MeshNodeTransform::Trs { rotation, .. } => quaternion(rotation),
                            MeshNodeTransform::Matrix(_) => frames.fixed_rotations[index],
                        }?;
                        Some(match node.parent {
                            Some(parent) => orientations[parent]? * local,
                            None => local,
                        })
                    })
                    .flatten();
                orientations.push(rotation);
            }
        }
        if let Some((lights, orientations)) = lights.zip(orientations.as_ref()) {
            for mount in self.lights.iter() {
                if lights.len() >= MAX_SCENE_LIGHTS {
                    break;
                }
                if mount.light.radiance == [0.0; 3] {
                    continue;
                }
                let Some(matrix) = world[mount.node] else {
                    continue;
                };
                let rotation = if let Some(rotation) = orientations[mount.node] {
                    rotation
                } else if matches!(mount.light.source, SceneLightSource::Point { .. }) {
                    UnitQuaternion::identity()
                } else {
                    continue;
                };
                if let Some(light) = mount
                    .light
                    .placed(matrix.fixed_view::<3, 1>(0, 3).into_owned(), rotation)
                {
                    lights.push(light);
                }
            }
        }
        if let Some((cameras, orientations)) = cameras.zip(orientations.as_ref()) {
            for (camera, &settings) in self.cameras.iter().zip(self.camera_settings.iter()) {
                let Some((matrix, rotation)) = world[camera.node].zip(orientations[camera.node])
                else {
                    continue;
                };
                cameras.push(CameraPlacement {
                    source_node: camera.source_node,
                    name: camera.name.clone(),
                    pose: CameraNode {
                        eye: matrix.fixed_view::<3, 1>(0, 3).into_owned(),
                        rotation,
                        target: None,
                        settings,
                        lod_bias: 1.0,
                        layers: crate::scene3d::MAIN_LAYER,
                    },
                });
            }
        }
        if !renderables {
            return Vec::new();
        }
        let (to, weight) = blend.unwrap_or_default();
        let mut morph_channels =
            channel_pairs(from.morph_tracks(), to.morph_tracks(), |track| track.node).peekable();
        let palettes: Vec<_> = self.skins.iter().map(|skin| skin.palette(&world)).collect();
        self.nodes
            .iter()
            .zip(world)
            .enumerate()
            .filter_map(|(index, (node, matrix))| {
                let mesh = node.mesh?;
                let channels = morph_channels
                    .next_if(|(a, b)| a.or(*b).is_some_and(|track| track.node == index));
                let overrides = morph_overrides.get(mesh).and_then(Option::as_ref);
                let morph = match overrides {
                    Some(MorphOverrides::Complete(weights)) => Some(weights.clone()),
                    overrides => {
                        let mut weights = channels
                            .map(|(a, b)| {
                                let mut a_buffer = [0.0; MAX_MORPH_TARGETS];
                                let a = a.map_or(&*node.morph_weights, |track| {
                                    track.sample(from.time, &mut a_buffer)
                                });
                                if blend.is_none() {
                                    return Arc::from(a);
                                }
                                let mut b_buffer = [0.0; MAX_MORPH_TARGETS];
                                let b = b.map_or(&*node.morph_weights, |track| {
                                    track.sample(to.time, &mut b_buffer)
                                });
                                let mut weights = [0.0; MAX_MORPH_TARGETS];
                                for ((out, &a), &b) in weights.iter_mut().zip(a).zip(b) {
                                    *out = a * (1.0 - weight) + b * weight;
                                }
                                Arc::from(&weights[..a.len()])
                            })
                            .or_else(|| {
                                (!node.morph_weights.is_empty()).then(|| node.morph_weights.clone())
                            });
                        if let Some((weights, MorphOverrides::Partial(overrides))) =
                            weights.as_mut().zip(overrides)
                            && overrides
                                .iter()
                                .any(|&(index, value)| weights[index] != value)
                        {
                            let weights = Arc::make_mut(weights);
                            for &(index, value) in overrides {
                                weights[index] = value;
                            }
                        }
                        weights
                    }
                };
                if let Some(skin) = node.skin {
                    // glTF applies the animated joint transforms in scene space.
                    // The node bearing the mesh has no additional local transform.
                    Some(MeshPlacement {
                        mesh,
                        transform: Matrix4::identity(),
                        skin: Some(palettes[skin].as_ref()?.clone()),
                        morph,
                    })
                } else {
                    Some(MeshPlacement {
                        mesh,
                        transform: matrix?,
                        skin: None,
                        morph,
                    })
                }
            })
            .collect()
    }
}
