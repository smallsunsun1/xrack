//! Imported skin weights, joint palettes, and posed bounds.
use std::{
    collections::VecDeque,
    ops::Range,
    sync::{Arc, Mutex, OnceLock},
};

use crate::{
    RecordValue, Value, VecModel,
    scene3d::MeshVertex,
    scene3d::mesh::authored::SceneMeshInputs,
    scene3d::mesh::morph::{MorphPose, expanded},
};
use nalgebra::{Matrix4, Vector3};

pub const MAX_SKIN_JOINTS: usize = 128;
pub const SKIN_INFLUENCES: usize = 8;
pub type JointRows = [[f32; 4]; 3];
pub type JointPalette = Arc<[JointRows]>;
pub(crate) type Bounds = ([f32; 3], [f32; 3]);
type JointBounds = Vec<(usize, Bounds)>;
type JointBindings = Arc<[SceneSkinJoint]>;
type JointBindingEntry = ((u64, u64), JointBindings);

const JOINT_BINDING_CACHE_SIZE: usize = 16;
static JOINT_BINDINGS: OnceLock<Mutex<VecDeque<JointBindingEntry>>> = OnceLock::new();

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct SceneSkinJoint {
    pub(crate) node: Box<str>,
    pub(crate) inverse_bind: Matrix4<f32>,
}

impl SceneSkinJoint {
    pub fn new(node: &str, inverse_bind: Option<[[f64; 4]; 4]>) -> Option<Self> {
        if node.is_empty() {
            return None;
        }
        let matrix = inverse_bind.map_or_else(Matrix4::identity, |rows| {
            Matrix4::from_fn(|row, col| rows[row][col] as f32)
        });
        if std::array::from_fn::<_, 4, _>(|col| matrix[(3, col)]) != [0.0, 0.0, 0.0, 1.0]
            || !matrix.iter().all(|value| value.is_finite())
            || matrix
                .try_inverse()
                .is_none_or(|inverse| !inverse.iter().all(|value| value.is_finite()))
        {
            return None;
        }
        Some(Self {
            node: node.into(),
            inverse_bind: matrix,
        })
    }

    pub(crate) fn from_record(row: &RecordValue) -> Option<Self> {
        let Value::String(node) = row.field("node")? else {
            return None;
        };
        let inverse_bind = match row.field("inverse-bind") {
            Some(Value::Record(matrix)) => {
                let mut rows = [[0.0; 4]; 4];
                for (row, fields) in rows.iter_mut().zip(crate::scene3d::CAMERA_MATRIX_FIELDS) {
                    for (value, field) in row.iter_mut().zip(fields) {
                        let Value::Float(number) = matrix.field(field)? else {
                            return None;
                        };
                        *value = *number;
                    }
                }
                Some(rows)
            }
            _ => None,
        };
        Self::new(node, inverse_bind)
    }
}

#[doc(hidden)]
pub fn prepare_scene_skin_joints<T: 'static>(
    inputs: &SceneMeshInputs,
    model: &VecModel<T>,
    convert: impl FnMut(&T) -> Option<SceneSkinJoint>,
) -> Option<JointBindings> {
    let stamp = inputs.remember(model);
    if model.is_empty() {
        return None;
    }
    let cache = JOINT_BINDINGS.get_or_init(Mutex::default);
    if let Ok(mut cache) = cache.lock()
        && let Some(index) = cache.iter().position(|(key, _)| *key == stamp)
        && let Some(entry) = cache.remove(index)
    {
        let bindings = entry.1.clone();
        cache.push_front(entry);
        return Some(bindings);
    }
    let (count, rows) = model.inspect_rows(0, MAX_SKIN_JOINTS, convert);
    let bindings = if count > MAX_SKIN_JOINTS {
        Arc::default()
    } else {
        rows.into_iter()
            .collect::<Option<JointBindings>>()
            .unwrap_or_default()
    };
    if let Ok(mut cache) = cache.lock() {
        cache.truncate(JOINT_BINDING_CACHE_SIZE - 1);
        cache.push_front((stamp, bindings.clone()));
    }
    Some(bindings)
}

#[cfg(test)]
mod tests {
    use super::{Arc, SceneMeshInputs, SceneSkinJoint, VecModel, prepare_scene_skin_joints};

    #[test]
    fn authored_bindings_are_cached_until_the_joint_model_changes() {
        let joints = VecModel::from(vec!["hinge".to_owned()]);
        let prepare = |node: &String| SceneSkinJoint::new(node, None);
        let first =
            prepare_scene_skin_joints(&SceneMeshInputs::default(), &joints, prepare).unwrap();
        let reused = prepare_scene_skin_joints(&SceneMeshInputs::default(), &joints, |_| {
            panic!("the joint bindings should already be cached")
        })
        .unwrap();
        assert!(Arc::ptr_eq(&first, &reused));
        assert!(joints.set_row(0, "root-bone".to_owned()));
        let edited =
            prepare_scene_skin_joints(&SceneMeshInputs::default(), &joints, prepare).unwrap();
        assert!(!Arc::ptr_eq(&first, &edited));
        assert_eq!(edited[0].node.as_ref(), "root-bone");
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MeshInfluence {
    pub joints: [u16; SKIN_INFLUENCES],
    pub weights: [f32; SKIN_INFLUENCES],
}

impl MeshInfluence {
    pub const STRIDE: usize = SKIN_INFLUENCES * 6;

    #[doc(hidden)]
    pub fn normalized(
        mut joints: [u16; SKIN_INFLUENCES],
        mut weights: [f32; SKIN_INFLUENCES],
    ) -> Option<Self> {
        let sum = weights.iter().sum::<f32>();
        if !(sum.is_finite() && sum > 0.0 && weights.iter().all(|value| *value >= 0.0)) {
            return None;
        }
        for (joint, weight) in joints.iter_mut().zip(&mut weights) {
            *weight /= sum;
            if *weight == 0.0 {
                *joint = 0;
            }
        }
        Some(Self { joints, weights })
    }

    /// Weighted local-to-pose matrix used for both raycasts and asset error checks.
    #[doc(hidden)]
    pub fn deformation_rows(self, palette: &JointPalette) -> Option<JointRows> {
        let mut rows = [[0.0; 4]; 3];
        for (joint, weight) in self.joints.into_iter().zip(self.weights) {
            if weight == 0.0 {
                continue;
            }
            let transform = palette.get(usize::from(joint))?;
            for (a, b) in rows.iter_mut().flatten().zip(transform.iter().flatten()) {
                *a += weight * b;
            }
        }
        Some(rows)
    }

    pub fn packed(self) -> [u8; Self::STRIDE] {
        let mut bytes = [0; Self::STRIDE];
        for (i, value) in self.joints.into_iter().enumerate() {
            bytes[i * 2..i * 2 + 2].copy_from_slice(&value.to_le_bytes());
        }
        for (i, value) in self.weights.into_iter().enumerate() {
            let offset = SKIN_INFLUENCES * 2 + i * 4;
            bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
        }
        bytes
    }

    pub(crate) fn from_packed(bytes: [u8; Self::STRIDE]) -> Self {
        let joints = std::array::from_fn(|i| u16::from_le_bytes([bytes[i * 2], bytes[i * 2 + 1]]));
        let floats = bytes[SKIN_INFLUENCES * 2..].as_chunks::<4>().0;
        let weights = std::array::from_fn(|i| f32::from_le_bytes(floats[i]));
        Self { joints, weights }
    }
}

#[derive(Debug)]
pub struct MeshSkinJoint {
    pub node: usize,
    pub inverse_bind: [[f32; 4]; 4],
}

#[derive(Debug)]
pub struct MeshSkin {
    pub joints: Vec<MeshSkinJoint>,
}

impl MeshSkin {
    pub(crate) fn valid(&self, node_count: usize) -> bool {
        if self.joints.is_empty() || self.joints.len() > MAX_SKIN_JOINTS {
            return false;
        }
        let mut seen = std::collections::BTreeSet::new();
        self.joints.iter().all(|joint| {
            joint.node < node_count
                && seen.insert(joint.node)
                && joint.inverse_bind.map(|column| column[3]) == [0.0, 0.0, 0.0, 1.0]
                && joint
                    .inverse_bind
                    .iter()
                    .flatten()
                    .all(|value| value.is_finite())
        })
    }

    pub(crate) fn palette(&self, world: &[Option<Matrix4<f32>>]) -> Option<JointPalette> {
        self.joints
            .iter()
            .map(|joint| {
                let world = world[joint.node]?;
                let bind = Matrix4::from_fn(|row, col| joint.inverse_bind[col][row]);
                joint_rows(world * bind)
            })
            .collect::<Option<Vec<_>>>()
            .map(Into::into)
    }
}

pub(crate) fn joint_rows(pose: Matrix4<f32>) -> Option<JointRows> {
    let rows: JointRows = std::array::from_fn(|row| std::array::from_fn(|col| pose[(row, col)]));
    rows.iter()
        .flatten()
        .all(|value| value.is_finite() && value.abs() <= 1_000_000.0)
        .then_some(rows)
}

#[derive(Debug)]
pub(crate) struct SkinGeometry {
    pub weights: Vec<MeshInfluence>,
    pub joint_count: usize,
    bounds: JointBounds,
    blended_bounds: Vec<(usize, JointBounds)>,
}

impl SkinGeometry {
    pub(crate) fn new(
        weights: Vec<MeshInfluence>,
        vertices: &[MeshVertex],
        blended_parts: impl Iterator<Item = (usize, Range<usize>)>,
    ) -> Option<Self> {
        if vertices.len() != weights.len() {
            return None;
        }
        let mut joint_count = 0;
        for influence in &weights {
            if (influence.weights.iter().sum::<f32>() - 1.0).abs() > 1e-5 {
                return None;
            }
            for (joint, weight) in influence.joints.into_iter().zip(influence.weights) {
                if usize::from(joint) >= MAX_SKIN_JOINTS || !(0.0..=1.0).contains(&weight) {
                    return None;
                }
                joint_count = joint_count.max(usize::from(joint) + 1);
            }
        }
        let bounds = joint_bounds(vertices, &weights, joint_count);
        let blended_bounds = blended_parts
            .map(|(part, range)| {
                (
                    part,
                    joint_bounds(&vertices[range.clone()], &weights[range], joint_count),
                )
            })
            .collect();
        Some(Self {
            weights,
            joint_count,
            bounds,
            blended_bounds,
        })
    }
}

fn joint_bounds(
    vertices: &[MeshVertex],
    weights: &[MeshInfluence],
    joint_count: usize,
) -> JointBounds {
    let mut boxes = vec![([f32::INFINITY; 3], [f32::NEG_INFINITY; 3]); joint_count];
    for (vertex, influence) in vertices.iter().zip(weights) {
        for (joint, weight) in influence.joints.into_iter().zip(influence.weights) {
            if weight > 0.0 {
                let (min, max) = &mut boxes[usize::from(joint)];
                for axis in 0..3 {
                    min[axis] = min[axis].min(vertex.position[axis]);
                    max[axis] = max[axis].max(vertex.position[axis]);
                }
            }
        }
    }
    boxes
        .into_iter()
        .enumerate()
        .filter(|(_, (min, _))| min[0].is_finite())
        .collect()
}

fn posed_bounds(bounds: &JointBounds, palette: &[JointRows], displacement: Bounds) -> Bounds {
    let mut min = [f32::INFINITY; 3];
    let mut max = [f32::NEG_INFINITY; 3];
    // Each skinned vertex is a convex sum of its joint-transformed positions.
    // The union of the transformed joint boxes encloses those sums.
    for &(joint, bind) in bounds {
        let (low, high) = expanded(bind, displacement);
        for (axis, [x, y, z, offset]) in palette[joint].into_iter().enumerate() {
            let mut a = offset;
            let mut b = offset;
            for (i, scale) in [x, y, z].into_iter().enumerate() {
                a += (scale * low[i]).min(scale * high[i]);
                b += (scale * low[i]).max(scale * high[i]);
            }
            min[axis] = min[axis].min(a);
            max[axis] = max[axis].max(b);
        }
    }
    for axis in 0..3 {
        let pad = min[axis].abs().max(max[axis].abs()).max(1.0) * 2e-6;
        min[axis] -= pad;
        max[axis] += pad;
    }
    (min, max)
}

#[derive(Debug, PartialEq)]
pub(crate) struct SkinPose {
    pub palette: JointPalette,
    pub bounds: Bounds,
    pub blended_parts: Vec<(usize, [f32; 3])>,
}

impl SkinPose {
    pub(crate) fn new(
        geometry: &SkinGeometry,
        palette: JointPalette,
        morph: Option<&MorphPose>,
    ) -> Option<Self> {
        if geometry.joint_count > palette.len() {
            return None;
        }
        let bounds = posed_bounds(
            &geometry.bounds,
            &palette,
            morph.map_or(([0.0; 3], [0.0; 3]), |pose| pose.displacement),
        );
        if !bounds.0.into_iter().chain(bounds.1).all(f32::is_finite) {
            return None;
        }
        let blended_parts = geometry
            .blended_bounds
            .iter()
            .map(|(index, bounds)| {
                let delta =
                    morph.map_or(([0.0; 3], [0.0; 3]), |pose| pose.part_displacement(*index));
                let (min, max) = posed_bounds(bounds, &palette, delta);
                (
                    *index,
                    std::array::from_fn(|axis| f32::midpoint(min[axis], max[axis])),
                )
            })
            .collect();
        Some(Self {
            palette,
            bounds,
            blended_parts,
        })
    }

    pub(crate) fn deform<const NORMALS: bool>(
        &self,
        influence: MeshInfluence,
        mut vertex: MeshVertex,
    ) -> MeshVertex {
        let Some(rows) = influence.deformation_rows(&self.palette) else {
            return vertex;
        };
        let [x, y, z] = vertex.position;
        vertex.position = rows.map(|[a, b, c, offset]| a * x + b * y + c * z + offset);
        if NORMALS {
            let [x, y, z] =
                std::array::from_fn(|i| Vector3::new(rows[0][i], rows[1][i], rows[2][i]));
            let yz = y.cross(&z);
            let [a, b, c] = vertex.normal;
            let normal = a * yz + b * z.cross(&x) + c * x.cross(&y);
            let normal = if x.dot(&yz) < 0.0 { -normal } else { normal };
            if let Some(normal) = normal.try_normalize(1e-12) {
                vertex.normal = normal.into();
            }
        }
        vertex
    }
}
