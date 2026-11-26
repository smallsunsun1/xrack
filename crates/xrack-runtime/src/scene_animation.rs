//! Prepared glTF node geometry and keyframes shared by both XRack backends.
use crate::{
    Point3D, Quaternion3D, RecordValue, Value, VecModel,
    scene_authored_mesh::SceneMeshInputs,
    scene_camera::{CameraNode, SceneCameraSettings},
    scene_lighting::{MAX_SCENE_LIGHTS, SceneLight, SceneLightSource},
    scene_morph::MorphOverrides,
    scene_particle_track::{
        MAX_EMISSION_KEYS, MAX_EMISSION_TIME, ParticleEmissionAnimation, ParticleEmissionTrack,
    },
    scene3d::{JointPalette, MAX_MORPH_TARGETS, Mesh3D, MeshSkin, MorphWeights},
};
use nalgebra::{Matrix3, Matrix4, Quaternion, Rotation3, Translation3, UnitQuaternion, Vector3};
use std::{
    borrow::Cow,
    cmp::Ordering,
    collections::BTreeSet,
    ops::Range,
    sync::{Arc, OnceLock},
};

pub const MAX_ANIMATION_NODES: usize = 4096;
pub const MAX_ANIMATION_KEYS: usize = 1_000_000;
pub const MAX_MESH_INSTANCES: usize = 4096;
const MAX_ANIMATION_LAYERS: usize = 16;

/// Prepared geometry for a static mesh or an animated glTF scene.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub enum MeshAsset {
    Static(Mesh3D),
    Animated(Arc<AnimatedMesh>),
    Lods(Arc<MeshLods>),
    Variants(Arc<MeshVariants>),
}

impl PartialEq for MeshAsset {
    fn eq(&self, other: &Self) -> bool {
        match (self, other) {
            (Self::Static(a), Self::Static(b)) => a == b,
            (Self::Animated(a), Self::Animated(b)) => Arc::ptr_eq(a, b),
            (Self::Lods(a), Self::Lods(b)) => Arc::ptr_eq(a, b),
            (Self::Variants(a), Self::Variants(b)) => Arc::ptr_eq(a, b),
            _ => false,
        }
    }
}

impl MeshAsset {
    pub(crate) fn has_cameras(&self) -> bool {
        match self {
            Self::Animated(rig) => !rig.cameras.is_empty(),
            Self::Lods(lods) => lods.primary().has_cameras(),
            Self::Variants(variants) => variants.primary.has_cameras(),
            Self::Static(_) => false,
        }
    }

    pub(crate) fn has_lights(&self) -> bool {
        match self {
            Self::Animated(rig) => !rig.lights.is_empty(),
            Self::Lods(lods) => lods.primary().has_lights(),
            Self::Variants(variants) => variants.primary.has_lights(),
            Self::Static(_) => false,
        }
    }

    pub fn node(&self, selector: &str) -> crate::MeshNode3D {
        self.node_catalog()
            .map_or_else(crate::MeshNode3D::default, |catalog| catalog.node(selector))
    }

    pub fn node_at(&self, source: i64) -> crate::MeshNode3D {
        self.node_catalog()
            .map_or_else(crate::MeshNode3D::default, |catalog| {
                catalog.node_at(source)
            })
    }

    pub fn node_indices(&self, kind: &str) -> crate::VecModel<i64> {
        self.node_catalog()
            .map_or_else(crate::VecModel::default, |catalog| catalog.indices(kind))
    }

    pub fn mesh_nodes(&self, kind: &str) -> Vec<crate::MeshNode3D> {
        self.node_catalog()
            .map_or_else(Vec::new, |catalog| catalog.nodes(kind).cloned().collect())
    }

    pub(crate) fn shared_node_catalog(&self) -> Option<Arc<crate::mesh_nodes::MeshNodeCatalog>> {
        match self {
            Self::Static(mesh) => mesh.shared_node_catalog(),
            Self::Animated(rig) => rig.node_catalog.clone(),
            Self::Lods(lods) => lods.primary().shared_node_catalog(),
            Self::Variants(variants) => variants.primary().shared_node_catalog(),
        }
    }

    fn node_catalog(&self) -> Option<&crate::mesh_nodes::MeshNodeCatalog> {
        match self {
            Self::Static(mesh) => mesh.node_catalog(),
            Self::Animated(rig) => rig.node_catalog(),
            Self::Lods(lods) => lods.primary().node_catalog(),
            Self::Variants(variants) => variants.primary().node_catalog(),
        }
    }

    pub(crate) fn animated_primary(&self) -> Option<&Arc<AnimatedMesh>> {
        match self {
            Self::Static(_) => None,
            Self::Animated(rig) => Some(rig),
            Self::Lods(lods) => lods.primary().animated_primary(),
            Self::Variants(variants) => variants.primary().animated_primary(),
        }
    }

    pub fn resolve(key: &str, imported: impl FnOnce(&str) -> Option<Self>) -> Option<Self> {
        Mesh3D::builtin(key)
            .map(Self::Static)
            .or_else(|| imported(key))
    }
}

/// A single imported model with alternative prepared materials and vertex layouts.
#[doc(hidden)]
#[derive(Debug)]
pub struct MeshVariants {
    primary: MeshAsset,
    alternatives: Box<[(String, Option<MeshAsset>)]>,
}

impl MeshVariants {
    pub fn primary(&self) -> &MeshAsset {
        &self.primary
    }

    pub fn alternatives(&self) -> &[(String, Option<MeshAsset>)] {
        &self.alternatives
    }

    fn select(&self, selector: &str) -> &MeshAsset {
        let selected = if let Some(index) = selector.strip_prefix('#') {
            index
                .parse::<usize>()
                .ok()
                .and_then(|index| self.alternatives.get(index))
        } else {
            let mut matches = self
                .alternatives
                .iter()
                .filter(|(name, _)| name == selector);
            matches.next().filter(|_| matches.next().is_none())
        };
        selected
            .and_then(|(_, alternative)| alternative.as_ref())
            .unwrap_or(&self.primary)
    }
}

impl MeshAsset {
    #[doc(hidden)]
    pub fn with_material_variants(self, alternatives: Vec<(String, Option<Self>)>) -> Self {
        if alternatives.is_empty() {
            return self;
        }
        let alternatives = alternatives
            .into_iter()
            .map(|(name, asset)| (name, asset.map(|asset| asset.share_variant_streams(&self))))
            .collect();
        Self::Variants(Arc::new(MeshVariants {
            primary: self,
            alternatives,
        }))
    }

    pub(crate) fn select_material_variant(self, selector: &str) -> Self {
        match self {
            Self::Variants(variants) => variants.select(selector).clone(),
            asset => asset,
        }
    }

    /// Apply offline mesh processing independently to each material layout.
    #[doc(hidden)]
    pub fn map_material_variants<E>(
        self,
        mut prepare: impl FnMut(Self) -> Result<Self, E>,
    ) -> Result<Self, E> {
        let Self::Variants(variants) = self else {
            return prepare(self);
        };
        let primary = prepare(variants.primary.clone())?;
        let alternatives = variants
            .alternatives
            .iter()
            .map(|(name, asset)| Ok((name.clone(), asset.clone().map(&mut prepare).transpose()?)))
            .collect::<Result<Vec<_>, E>>()?;
        Ok(primary.with_material_variants(alternatives))
    }

    fn share_variant_streams(self, primary: &Self) -> Self {
        let share_meshes = |meshes: &mut [Mesh3D], source: &[Mesh3D]| {
            for (mesh, reference) in meshes.iter_mut().zip(source) {
                mesh.share_variant_streams(reference);
            }
        };
        match (self, primary) {
            (Self::Static(mut mesh), Self::Static(reference)) => {
                mesh.share_variant_streams(reference);
                Self::Static(mesh)
            }
            (Self::Animated(rig), Self::Animated(reference)) => {
                let mut meshes = rig.meshes.clone();
                share_meshes(&mut meshes, reference.meshes());
                Self::Animated(Arc::new(rig.with_meshes(meshes)))
            }
            (Self::Lods(lods), Self::Lods(reference)) => {
                let primary = lods
                    .primary
                    .clone()
                    .share_variant_streams(&reference.primary);
                let levels = lods
                    .levels
                    .iter()
                    .map(|level| {
                        let mut meshes = level.meshes.to_vec();
                        if let Some(reference) = reference
                            .levels
                            .iter()
                            .find(|other| other.max_size == level.max_size)
                        {
                            share_meshes(&mut meshes, &reference.meshes);
                        }
                        (level.max_size, meshes)
                    })
                    .collect();
                primary.with_lods(levels)
            }
            (asset, Self::Lods(reference)) => asset.share_variant_streams(&reference.primary),
            (asset, _) => asset,
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
                MeshAsset::Lods(_) | MeshAsset::Variants(_) => self.primary.clone(),
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
            Self::Variants(variants) => variants.primary.geometry_meshes(),
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

pub(crate) fn matrix_orientation(matrix: Matrix4<f32>) -> Option<UnitQuaternion<f32>> {
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

/// Affine transforms prepared once per imported glTF instance group.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct MeshInstances {
    transforms: Arc<[Matrix4<f32>]>,
    colors: Option<Arc<[[f32; 4]]>>,
    data: Option<Arc<[[f32; 4]]>>,
}

impl MeshInstances {
    pub fn from_trs(transforms: impl ExactSizeIterator<Item = MeshNodeTransform>) -> Option<Self> {
        if !(1..=MAX_MESH_INSTANCES).contains(&transforms.len()) {
            return None;
        }
        Some(Self {
            transforms: transforms
                .map(|transform| {
                    transform
                        .matrix()
                        .filter(|matrix| matrix.iter().all(|value| value.is_finite()))
                })
                .collect::<Option<Vec<_>>>()?
                .into(),
            colors: None,
            data: None,
        })
    }

    /// Attach validated, optional channels indexed in transform order.
    pub fn with_attributes(
        mut self,
        colors: Option<Vec<[f32; 4]>>,
        data: Option<Vec<[f32; 4]>>,
    ) -> Option<Self> {
        let matches_count = |rows: &[[f32; 4]]| rows.len() == self.transforms.len();
        let colors_valid = colors.as_deref().is_none_or(|rows| {
            matches_count(rows)
                && rows
                    .iter()
                    .flatten()
                    .all(|value| (0.0..=1.0).contains(value))
        });
        let data_valid = data.as_deref().is_none_or(|rows| {
            matches_count(rows) && rows.iter().flatten().all(|value| value.is_finite())
        });
        if !colors_valid || !data_valid {
            return None;
        }
        self.colors = colors.map(Arc::from);
        self.data = data.map(Arc::from);
        Some(self)
    }

    /// Optional linear RGBA tint values.
    pub fn colors(&self) -> &[[f32; 4]] {
        self.colors.as_deref().unwrap_or_default()
    }

    /// Optional four-channel material input values.
    pub fn data(&self) -> &[[f32; 4]] {
        self.data.as_deref().unwrap_or_default()
    }

    pub fn packed_words(&self) -> impl Iterator<Item = f32> + '_ {
        self.transforms.iter().flat_map(|matrix| {
            matrix
                .as_slice()
                .chunks_exact(4)
                .flat_map(|column| column[..3].iter().copied())
        })
    }

    pub fn from_packed(bytes: &[u8], colors: &[u8], data: &[u8]) -> Option<Self> {
        let (instances, remainder) = bytes.as_chunks::<48>();
        if !remainder.is_empty() || !(1..=MAX_MESH_INSTANCES).contains(&instances.len()) {
            return None;
        }
        let transforms = instances.iter().map(|instance| {
            let (axes, _) = instance.as_chunks::<4>();
            let values: [f32; 12] = std::array::from_fn(|index| f32::from_le_bytes(axes[index]));
            values.iter().all(|value| value.is_finite()).then(|| {
                Matrix4::from_fn(|row, col| {
                    if row == 3 {
                        if col == 3 { 1.0 } else { 0.0 }
                    } else {
                        values[col * 3 + row]
                    }
                })
            })
        });
        let decode = |bytes: &[u8]| {
            let (rows, remainder) = bytes.as_chunks::<16>();
            if !remainder.is_empty() {
                return None;
            }
            Some((!rows.is_empty()).then(|| {
                rows.iter()
                    .map(|row| {
                        let (channels, _) = row.as_chunks::<4>();
                        std::array::from_fn(|index| f32::from_le_bytes(channels[index]))
                    })
                    .collect()
            }))
        };
        Self {
            transforms: transforms.collect::<Option<Vec<_>>>()?.into(),
            colors: None,
            data: None,
        }
        .with_attributes(decode(colors)?, decode(data)?)
    }
}

#[derive(Debug)]
pub struct MeshAnimationNode {
    pub source_node: u32,
    pub parent: Option<usize>,
    pub mesh: Option<usize>,
    pub skin: Option<usize>,
    pub morph_weights: MorphWeights,
    pub transform: MeshNodeTransform,
    pub instances: Option<MeshInstances>,
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
    pub layers: Arc<[MeshAnimationLayer]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
enum AnimationLayerMode {
    Override,
    Additive,
}

/// One weighted glTF clip layered over the base pose.
#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct MeshAnimationLayer {
    playback: MeshPlayback,
    root: String,
    weight: f32,
    mode: AnimationLayerMode,
}

impl MeshAnimationLayer {
    pub fn new(playback: MeshPlayback, root: String, weight: f64, mode: &str) -> Option<Self> {
        if !weight.is_finite() || weight <= 0.0 || playback.clip.is_empty() {
            return None;
        }
        let mode = match mode {
            "override" => AnimationLayerMode::Override,
            "additive" => AnimationLayerMode::Additive,
            _ => return None,
        };
        Some(Self {
            playback,
            root,
            weight: weight.min(1.0) as f32,
            mode,
        })
    }

    pub(crate) fn from_record(row: &RecordValue) -> Option<Self> {
        let string = |name| match row.field(name) {
            Some(Value::String(value)) => Some(value.as_str()),
            _ => None,
        };
        let number = |name| match row.field(name) {
            Some(Value::Float(value)) => Some(*value),
            _ => None,
        };
        Self::new(
            MeshPlayback {
                clip: string("clip")?.to_owned(),
                time: number("time").unwrap_or(0.0),
                looping: row.field("looping") == Some(&Value::Bool(true)),
            },
            string("root-node").unwrap_or_default().to_owned(),
            number("weight").unwrap_or(1.0),
            string("mode").unwrap_or("override"),
        )
    }
}

#[doc(hidden)]
pub fn prepare_animation_layers<T: 'static>(
    inputs: &SceneMeshInputs,
    model: &VecModel<T>,
    prepare: impl FnMut(&T) -> Option<MeshAnimationLayer>,
) -> Arc<[MeshAnimationLayer]> {
    inputs.remember(model);
    let (_, layers) = model.inspect_rows(0, MAX_ANIMATION_LAYERS, prepare);
    layers.into_iter().flatten().collect()
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

struct LayerSample<'a> {
    clip: ClipSample<'a>,
    mask: Option<(&'a [Range<u16>], Range<u16>)>,
    weight: f32,
    mode: AnimationLayerMode,
}

impl LayerSample<'_> {
    fn includes(&self, node: usize) -> bool {
        self.mask
            .as_ref()
            .is_none_or(|(ranges, mask)| mask.contains(&ranges[node].start))
    }

    fn scalar(&self, current: f32, target: f32, rest: f32) -> f32 {
        match self.mode {
            AnimationLayerMode::Override => current * (1.0 - self.weight) + target * self.weight,
            AnimationLayerMode::Additive => current + (target - rest) * self.weight,
        }
    }

    fn transform_channel(
        &self,
        property: MeshAnimationProperty,
        current: [f32; 4],
        target: [f32; 4],
        rest: [f32; 4],
    ) -> Option<[f32; 4]> {
        let result = if property != MeshAnimationProperty::Rotation {
            std::array::from_fn(|i| self.scalar(current[i], target[i], rest[i]))
        } else if self.mode == AnimationLayerMode::Override {
            interpolate_rotation(current, target, self.weight)?
        } else {
            let delta = quaternion(rest)?.inverse() * quaternion(target)?;
            let delta = interpolate_rotation(
                [0.0, 0.0, 0.0, 1.0],
                [delta.i, delta.j, delta.k, delta.w],
                self.weight,
            )?;
            let rotation = quaternion(current)? * quaternion(delta)?;
            [rotation.i, rotation.j, rotation.k, rotation.w]
        };
        result
            .iter()
            .all(|value| value.is_finite())
            .then_some(result)
    }
}

// The input is parent-first. Reserve a contiguous interval for each subtree
// even when the source does not store its children consecutively.
fn animation_subtrees(nodes: &[MeshAnimationNode]) -> Vec<Range<u16>> {
    let mut sizes = vec![1_u16; nodes.len()];
    for (index, node) in nodes.iter().enumerate().rev() {
        if let Some(parent) = node.parent {
            sizes[parent] += sizes[index];
        }
    }
    let mut offsets = vec![0_u16; nodes.len()];
    let mut root_offset = 0;
    let mut ranges = Vec::with_capacity(nodes.len());
    for (index, node) in nodes.iter().enumerate() {
        let offset = node
            .parent
            .map_or(&mut root_offset, |parent| &mut offsets[parent]);
        let range = *offset..*offset + sizes[index];
        *offset = range.end;
        offsets[index] = range.start + 1;
        ranges.push(range);
    }
    ranges
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

/// Converts an instance table's declared frame to and from glTF scene space.
#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct MeshInstanceSpace {
    pub to_model: Matrix4<f32>,
    pub to_root: Matrix4<f32>,
}

#[derive(Clone, Debug)]
pub(crate) struct MeshPose {
    pub matrices: Vec<Option<Matrix4<f32>>>,
    orientations: Option<Vec<Option<UnitQuaternion<f32>>>>,
}

#[derive(Debug)]
pub(crate) struct SampledMesh {
    pub placements: Vec<MeshPlacement>,
    pub pose: Option<Arc<MeshPose>>,
}

pub(crate) struct MeshSampleOptions<'a> {
    pub renderables: bool,
    pub retain_pose: bool,
    pub pose: Option<&'a Arc<MeshPose>>,
    pub overrides: &'a [Option<MorphOverrides>],
    pub lights: Option<&'a mut Vec<SceneLight>>,
    pub cameras: Option<&'a mut Vec<CameraPlacement>>,
    pub instance_space: Option<&'a MeshInstanceSpace>,
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

#[derive(Clone, Debug)]
pub struct AnimatedMesh {
    meshes: Vec<Mesh3D>,
    placements: usize,
    nodes: Arc<[MeshAnimationNode]>,
    clips: Arc<[MeshAnimationClip]>,
    clip_durations: Arc<[f32]>,
    subtrees: Arc<OnceLock<Vec<Range<u16>>>>,
    skins: Arc<[MeshSkin]>,
    lights: Arc<[MeshLight]>,
    cameras: Arc<[MeshCamera]>,
    camera_settings: Arc<[SceneCameraSettings]>,
    attachment_frames: Option<Arc<AttachmentFrames>>,
    node_catalog: Option<Arc<crate::mesh_nodes::MeshNodeCatalog>>,
}

#[derive(Debug)]
pub(crate) struct MeshPlacement {
    pub mesh: usize,
    pub source: (u32, Option<u32>),
    pub transform: Matrix4<f32>,
    pub instance_anchor: Option<Vector3<f32>>,
    pub tint: Option<[f32; 4]>,
    pub instance_data: Option<[f32; 4]>,
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
                    || node.instances.is_some() && node.mesh.is_none()
                    || node.transform.matrix().is_none()
            })
        {
            return Err("invalid animated glTF node graph");
        }
        let placements: usize = nodes
            .iter()
            .filter(|node| node.mesh.is_some())
            .map(|node| {
                node.instances
                    .as_ref()
                    .map_or(1, |group| group.transforms.len())
            })
            .sum();
        if placements > MAX_MESH_INSTANCES {
            return Err("glTF exceeds the mesh instance placement limit");
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
        let camera_settings = cameras
            .iter()
            .map(|camera| camera.lens.settings())
            .collect::<Option<Vec<_>>>()
            .ok_or("invalid glTF camera projection")?;
        let attachment_frames = AttachmentFrames::new(&nodes, &lights, &cameras).map(Arc::new);
        Ok(Self {
            meshes,
            placements,
            nodes: nodes.into(),
            clips: clips.into(),
            clip_durations,
            subtrees: Arc::new(OnceLock::new()),
            skins: skins.into(),
            lights: lights.into(),
            cameras: cameras.into(),
            camera_settings: camera_settings.into(),
            attachment_frames,
            node_catalog: None,
        })
    }

    #[doc(hidden)]
    pub fn with_meshes(&self, meshes: Vec<Mesh3D>) -> Self {
        Self {
            meshes,
            placements: self.placements,
            nodes: self.nodes.clone(),
            clips: self.clips.clone(),
            clip_durations: self.clip_durations.clone(),
            subtrees: self.subtrees.clone(),
            skins: self.skins.clone(),
            lights: self.lights.clone(),
            cameras: self.cameras.clone(),
            camera_settings: self.camera_settings.clone(),
            attachment_frames: self.attachment_frames.clone(),
            node_catalog: self.node_catalog.clone(),
        }
    }

    #[doc(hidden)]
    pub fn with_node_catalog(mut self, catalog: Arc<crate::mesh_nodes::MeshNodeCatalog>) -> Self {
        self.node_catalog = Some(catalog);
        self
    }

    #[doc(hidden)]
    pub fn node_catalog(&self) -> Option<&crate::mesh_nodes::MeshNodeCatalog> {
        self.node_catalog.as_deref()
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
            retain_pose: false,
            pose: None,
            overrides: &[],
            lights: None,
            cameras: None,
            instance_space: None,
        };
        let mut motion = MeshAnimationBlend::default();
        record(self.sample(&motion, motion_options()).placements);
        for clip in self.clips.iter().take(8) {
            motion.from.clip = format!("#{}", clip.index);
            let duration = f64::from(clip.duration());
            for fraction in [0.0, 0.25, 0.5, 0.75, 1.0] {
                motion.from.time = fraction * duration;
                record(self.sample(&motion, motion_options()).placements);
            }
        }
        samples
    }

    /// Sample the selected ancestor chain without evaluating skinning or unrelated nodes.
    pub(crate) fn emission_track(
        &self,
        settings: &ParticleEmissionAnimation,
    ) -> Option<ParticleEmissionTrack> {
        let ordinal = self.node_catalog()?.selector_ordinal(&settings.node)?;
        let clip_index = self.clip(&settings.clip)?;
        let clip = self.clips.get(clip_index)?;
        let duration = self.clip_durations[clip_index];
        if !(1.0..=240.0).contains(&settings.sample_rate)
            || !(0.0..=MAX_EMISSION_TIME).contains(&f64::from(duration))
        {
            return None;
        }
        let scale = [settings.scale.x, settings.scale.y, settings.scale.z];
        let position = [
            settings.position.x,
            settings.position.y,
            settings.position.z,
        ];
        if !scale
            .into_iter()
            .chain(position)
            .all(|value| value.abs() <= 1.0e8)
        {
            return None;
        }
        let [qx, qy, qz, qw] = settings.orientation.components().map(|value| value as f32);
        let orientation = quaternion([qx, qy, qz, qw])?;
        let placement = Translation3::from(Vector3::from(position.map(|value| value as f32)))
            .to_homogeneous()
            * orientation.to_homogeneous()
            * Matrix4::new_nonuniform_scaling(&Vector3::from(scale.map(|value| value as f32)));

        let mut chain = Vec::new();
        let mut next = Some(ordinal);
        while let Some(index) = next {
            let node = self.nodes.get(index)?;
            let start = clip.tracks.partition_point(|track| track.node < index);
            let end = clip.tracks.partition_point(|track| track.node <= index);
            let rest_orientation = match node.transform {
                MeshNodeTransform::Matrix(_) => Some(matrix_orientation(node.transform.matrix()?)?),
                MeshNodeTransform::Trs { .. } => None,
            };
            chain.push((node.transform, rest_orientation, &clip.tracks[start..end]));
            next = node.parent;
        }

        // Nonnegative f32 bit patterns sort in time order. Retain both sides
        // of each step even when regular samples fall on neither side.
        let mut times = BTreeSet::from([0]);
        if chain.iter().any(|(_, _, tracks)| !tracks.is_empty()) {
            times.insert(duration.to_bits());
            for (_, _, tracks) in &chain {
                for track in *tracks {
                    for (index, &time) in track.times.iter().enumerate() {
                        times.insert(if time == 0.0 { 0 } else { time.to_bits() });
                        if index > 0 && time > 0.0 && track.interpolation == MeshInterpolation::Step
                        {
                            times.insert(time.next_down().to_bits());
                        }
                        if times.len() > MAX_EMISSION_KEYS {
                            return None;
                        }
                    }
                }
            }
            let intervals = ((f64::from(duration) * settings.sample_rate).ceil() as usize)
                .min(MAX_EMISSION_KEYS - times.len() + 1);
            for index in 1..intervals {
                let time = (f64::from(duration) * index as f64 / intervals as f64) as f32;
                times.insert(time.to_bits());
            }
        }

        ParticleEmissionTrack::from_frames(times.into_iter().map(|bits| {
            let time = f32::from_bits(bits);
            let mut world = placement;
            let mut rotation = orientation;
            for &(mut local, rest_orientation, tracks) in chain.iter().rev() {
                for track in tracks {
                    local.set_channel(track.property, track.sample(time)?);
                }
                world *= local.matrix()?;
                let axis = match local {
                    MeshNodeTransform::Trs { rotation, .. } => quaternion(rotation)?,
                    MeshNodeTransform::Matrix(_) => rest_orientation?,
                };
                rotation *= axis;
            }
            let point = Point3D::new(
                f64::from(world[(0, 3)]),
                f64::from(world[(1, 3)]),
                f64::from(world[(2, 3)]),
            );
            Some((
                f64::from(time),
                point,
                Quaternion3D::new(
                    f64::from(rotation.i),
                    f64::from(rotation.j),
                    f64::from(rotation.k),
                    f64::from(rotation.w),
                ),
            ))
        }))
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

    fn layer_samples<'a>(&'a self, motion: &MeshAnimationBlend) -> Vec<LayerSample<'a>> {
        motion
            .layers
            .iter()
            .filter_map(|layer| {
                let clip = self.clip_sample(&layer.playback);
                clip.clip?;
                let mask = if layer.root.is_empty() {
                    None
                } else {
                    let index = self.node_catalog()?.selector_ordinal(&layer.root)?;
                    let ranges = self
                        .subtrees
                        .get_or_init(|| animation_subtrees(&self.nodes));
                    Some((ranges.as_slice(), ranges.get(index)?.clone()))
                };
                Some(LayerSample {
                    clip,
                    mask,
                    weight: layer.weight,
                    mode: layer.mode,
                })
            })
            .collect()
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

    pub(crate) fn sample_pose(
        &self,
        motion: &MeshAnimationBlend,
        track_orientation: bool,
    ) -> MeshPose {
        self.pose_from_samples(
            self.motion_samples(motion),
            &self.layer_samples(motion),
            track_orientation,
        )
    }

    fn pose_from_samples(
        &self,
        (from, blend): (ClipSample<'_>, Option<(ClipSample<'_>, f32)>),
        layers: &[LayerSample<'_>],
        track_orientation: bool,
    ) -> MeshPose {
        let mut transforms: Vec<_> = self.nodes.iter().map(|node| node.transform).collect();
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
        for layer in layers {
            for track in layer
                .clip
                .tracks()
                .iter()
                .filter(|track| layer.includes(track.node))
            {
                let current = &mut transforms[track.node];
                let rest = self.nodes[track.node].transform;
                if let Some(((current_value, target), rest)) = current
                    .channel(track.property)
                    .zip(track.sample(layer.clip.time))
                    .zip(rest.channel(track.property))
                    && let Some(result) =
                        layer.transform_channel(track.property, current_value, target, rest)
                {
                    current.set_channel(track.property, result);
                }
            }
        }
        let mut world: Vec<Option<Matrix4<f32>>> = Vec::with_capacity(self.nodes.len());
        let frames = self
            .attachment_frames
            .as_deref()
            .filter(|_| track_orientation);
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
        MeshPose {
            matrices: world,
            orientations,
        }
    }

    pub(crate) fn sample(
        &self,
        motion: &MeshAnimationBlend,
        options: MeshSampleOptions<'_>,
    ) -> SampledMesh {
        let MeshSampleOptions {
            renderables,
            retain_pose,
            pose,
            overrides: morph_overrides,
            lights,
            cameras,
            instance_space,
        } = options;
        let samples = self.motion_samples(motion);
        let layers = if renderables || pose.is_none() {
            self.layer_samples(motion)
        } else {
            Vec::new()
        };
        let evaluated = pose.map_or_else(
            || {
                Cow::Owned(self.pose_from_samples(
                    samples,
                    &layers,
                    lights.is_some() || cameras.is_some(),
                ))
            },
            |pose| Cow::Borrowed(pose.as_ref()),
        );
        let world = &evaluated.matrices;
        let orientations = evaluated.orientations.as_ref();
        let retain = |evaluated: Cow<'_, MeshPose>| {
            retain_pose.then(|| {
                pose.cloned()
                    .unwrap_or_else(|| Arc::new(evaluated.into_owned()))
            })
        };
        if let Some((lights, orientations)) = lights.zip(orientations) {
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
        if let Some((cameras, orientations)) = cameras.zip(orientations) {
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
                        settings,
                        lod_bias: 1.0,
                        layers: crate::scene3d::MAIN_LAYER,
                    },
                });
            }
        }
        if !renderables {
            return SampledMesh {
                placements: Vec::new(),
                pose: retain(evaluated),
            };
        }
        let (from, blend) = samples;
        let (to, weight) = blend.unwrap_or_default();
        let mut morph_channels =
            channel_pairs(from.morph_tracks(), to.morph_tracks(), |track| track.node).peekable();
        let palettes: Vec<_> = self.skins.iter().map(|skin| skin.palette(world)).collect();
        let mut placements = Vec::with_capacity(self.placements);
        for (index, (node, matrix)) in self.nodes.iter().zip(world.iter().copied()).enumerate() {
            let Some(mesh) = node.mesh else {
                continue;
            };
            let channels =
                morph_channels.next_if(|(a, b)| a.or(*b).is_some_and(|track| track.node == index));
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
                    if let Some(weights) = weights.as_mut() {
                        for layer in layers.iter().filter(|layer| layer.includes(index)) {
                            let tracks = layer.clip.morph_tracks();
                            let Ok(track) = tracks.binary_search_by_key(&index, |track| track.node)
                            else {
                                continue;
                            };
                            let mut sample = [0.0; MAX_MORPH_TARGETS];
                            let target = tracks[track].sample(layer.clip.time, &mut sample);
                            let rest = &node.morph_weights;
                            let mut blended = [0.0; MAX_MORPH_TARGETS];
                            let blended = &mut blended[..weights.len()];
                            for (((value, &current), &target), &rest) in blended
                                .iter_mut()
                                .zip(weights.iter())
                                .zip(target)
                                .zip(rest.iter())
                            {
                                let next = layer.scalar(current, target, rest);
                                *value = if next.is_finite() { next } else { current };
                            }
                            if *weights.as_ref() != *blended {
                                Arc::make_mut(weights).copy_from_slice(blended);
                            }
                        }
                    }
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
            let push = |placements: &mut Vec<MeshPlacement>, instance, transform, skin| {
                let attributes = node.instances.as_ref().zip(instance);
                placements.push(MeshPlacement {
                    mesh,
                    source: (node.source_node, instance),
                    transform,
                    instance_anchor: instance
                        .and(matrix.as_ref())
                        .map(|matrix| matrix.fixed_view::<3, 1>(0, 3).into_owned()),
                    tint: attributes
                        .and_then(|(group, index)| group.colors().get(index as usize).copied()),
                    instance_data: attributes
                        .and_then(|(group, index)| group.data().get(index as usize).copied()),
                    skin,
                    morph: morph.clone(),
                });
            };
            let palette = if let Some(skin) = node.skin {
                let Some(palette) = &palettes[skin] else {
                    continue;
                };
                Some(palette)
            } else {
                None
            };
            if let Some(instances) = &node.instances {
                let Some(matrix) = matrix else {
                    continue;
                };
                if let Some(space) = instance_space {
                    // Skinned vertices use the joint palette in scene space.
                    let local = if palette.is_some() {
                        space.to_root
                    } else {
                        space.to_root * matrix
                    };
                    for (index, instance) in instances.transforms.iter().enumerate() {
                        push(
                            &mut placements,
                            Some(index as u32),
                            space.to_model * instance * local,
                            palette.cloned(),
                        );
                    }
                } else if let Some(palette) = palette {
                    // The joint palette is in glTF scene space. Convert the instance
                    // transform into that space before applying it to skinned vertices.
                    let Some(inverse) = matrix.try_inverse() else {
                        continue;
                    };
                    for (i, instance) in instances.transforms.iter().enumerate() {
                        push(
                            &mut placements,
                            Some(i as u32),
                            matrix * instance * inverse,
                            Some(palette.clone()),
                        );
                    }
                } else {
                    for (i, instance) in instances.transforms.iter().enumerate() {
                        push(&mut placements, Some(i as u32), matrix * instance, None);
                    }
                }
            } else if let Some(palette) = palette {
                push(
                    &mut placements,
                    None,
                    Matrix4::identity(),
                    Some(palette.clone()),
                );
            } else if let Some(matrix) = matrix {
                push(&mut placements, None, matrix, None);
            }
        }
        SampledMesh {
            placements,
            pose: retain(evaluated),
        }
    }
}
