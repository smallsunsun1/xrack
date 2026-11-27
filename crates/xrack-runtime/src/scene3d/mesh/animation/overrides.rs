//! Overrides applied to local glTF node poses after clip blending.

use std::sync::Arc;

use nalgebra::{Matrix3, Rotation3, UnitQuaternion};

use crate::{Quaternion3D, RecordValue, Value, VecModel, Vector3D};

use super::{MAX_ANIMATION_NODES, MeshNodeTransform, SceneMeshInputs, interpolate_rotation};

#[doc(hidden)]
#[derive(Clone, Debug, PartialEq)]
pub struct MeshAnimationNodeOverride {
    pub(super) selector: Box<str>,
    translation: Option<[f32; 3]>,
    rotation: Option<[f32; 4]>,
    scale: Option<[f32; 3]>,
    weight: f32,
}

impl MeshAnimationNodeOverride {
    pub fn new(
        selector: &str,
        translation: Option<Vector3D>,
        rotation: Option<Quaternion3D>,
        scale: Option<Vector3D>,
        weight: f64,
    ) -> Option<Self> {
        if selector.is_empty() || !weight.is_finite() || weight <= 0.0 {
            return None;
        }
        let translation = translation.and_then(narrow);
        let rotation = rotation.map(|rotation| rotation.components().map(|value| value as f32));
        let scale = scale.and_then(narrow).filter(|scale| usable_scale(*scale));
        (translation.is_some() || rotation.is_some() || scale.is_some()).then(|| Self {
            selector: selector.into(),
            translation,
            rotation,
            scale,
            weight: weight.min(1.0) as f32,
        })
    }

    pub(crate) fn from_record(row: &RecordValue) -> Option<Self> {
        let Value::String(selector) = row.field("node")? else {
            return None;
        };
        let vector = |field| match row.field(field) {
            Some(Value::Vector3D(vector)) => Some(*vector),
            _ => None,
        };
        let rotation = match row.field("rotation") {
            Some(Value::Quaternion3D(rotation)) => Some(*rotation),
            _ => None,
        };
        let weight = match row.field("weight") {
            Some(Value::Float(weight)) => *weight,
            _ => 1.0,
        };
        Self::new(
            selector,
            vector("translation"),
            rotation,
            vector("scale"),
            weight,
        )
    }

    pub(super) fn apply(&self, transform: &mut MeshNodeTransform) {
        if let MeshNodeTransform::Matrix(columns) = transform {
            if (self.rotation.is_some() || self.scale.is_some())
                && let Some(trs) = decompose(*columns)
            {
                *transform = trs;
            } else {
                if let Some(position) = self.translation {
                    for (value, target) in columns[3][..3].iter_mut().zip(position) {
                        *value = *value * (1.0 - self.weight) + target * self.weight;
                    }
                }
                return;
            }
        }
        let MeshNodeTransform::Trs {
            translation,
            rotation,
            scale,
        } = transform
        else {
            return;
        };
        if let Some(target) = self.translation {
            *translation = mix(*translation, target, self.weight);
        }
        if let Some(target) = self.rotation
            && let Some(blended) = interpolate_rotation(*rotation, target, self.weight)
        {
            *rotation = blended;
        }
        if let Some(target) = self.scale {
            let blended = mix(*scale, target, self.weight);
            if usable_scale(blended) {
                *scale = blended;
            }
        }
    }
}

fn narrow(value: Vector3D) -> Option<[f32; 3]> {
    let value = [value.x, value.y, value.z].map(|value| value as f32);
    value.into_iter().all(f32::is_finite).then_some(value)
}

fn usable_scale(scale: [f32; 3]) -> bool {
    scale
        .into_iter()
        .all(|value| value.is_finite() && value.abs() > 1e-8)
}

fn mix(current: [f32; 3], target: [f32; 3], weight: f32) -> [f32; 3] {
    std::array::from_fn(|axis| current[axis] * (1.0 - weight) + target[axis] * weight)
}

fn decompose(columns: [[f32; 4]; 4]) -> Option<MeshNodeTransform> {
    let basis = Matrix3::from_fn(|row, column| columns[column][row]);
    // glTF uses a signed first scale axis when decomposing a reflection.
    let mut scale = std::array::from_fn(|axis| basis.column(axis).norm());
    if !usable_scale(scale) {
        return None;
    }
    if basis.determinant() < 0.0 {
        scale[0] = -scale[0];
    }
    let normalized = Matrix3::from_columns(&std::array::from_fn::<_, 3, _>(|axis| {
        basis.column(axis).into_owned() / scale[axis]
    }));
    // Preserve shear instead of discarding it while converting to TRS.
    if (normalized.transpose() * normalized - Matrix3::identity()).amax() > 1e-4 {
        return None;
    }
    let rotation =
        UnitQuaternion::from_rotation_matrix(&Rotation3::from_matrix_unchecked(normalized));
    let [x, y, z, _] = columns[3];
    Some(MeshNodeTransform::Trs {
        translation: [x, y, z],
        rotation: [rotation.i, rotation.j, rotation.k, rotation.w],
        scale,
    })
}

#[doc(hidden)]
pub fn prepare_animation_node_overrides<T: 'static>(
    inputs: &SceneMeshInputs,
    model: &VecModel<T>,
    prepare: impl FnMut(&T) -> Option<MeshAnimationNodeOverride>,
) -> Arc<[MeshAnimationNodeOverride]> {
    inputs.remember(model);
    let (_, overrides) = model.inspect_rows(0, MAX_ANIMATION_NODES, prepare);
    overrides.into_iter().flatten().collect()
}

#[cfg(test)]
mod tests {
    use super::{MeshAnimationNodeOverride, MeshNodeTransform, Quaternion3D, Vector3D};

    #[test]
    fn matrix_pose_override_preserves_reflection_and_sheared_translation() {
        let orientation = Quaternion3D::from_axis_angle(Vector3D::new(0.0, 0.0, 1.0), 90.0);
        let edit = MeshAnimationNodeOverride::new(
            "#0",
            Some(Vector3D::new(4.0, 8.0, 12.0)),
            Some(orientation),
            None,
            1.0,
        )
        .unwrap();
        let columns = [
            [-2.0, 0.0, 0.0, 0.0],
            [0.0, 3.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        let mut pose = MeshNodeTransform::Matrix(columns);
        edit.apply(&mut pose);
        let MeshNodeTransform::Trs {
            translation, scale, ..
        } = pose
        else {
            panic!("orthogonal matrix was not decomposed");
        };
        assert_eq!(scale, [-2.0, 3.0, 1.0]);
        assert_eq!(translation, [4.0, 8.0, 12.0]);
        assert!((pose.matrix().unwrap()[(1, 0)] + 2.0).abs() < 1e-4);

        let mut shear = columns;
        shear[1][0] = 0.5;
        let mut pose = MeshNodeTransform::Matrix(shear);
        edit.apply(&mut pose);
        let MeshNodeTransform::Matrix(edits) = pose else {
            panic!("shear must remain a matrix");
        };
        assert_eq!(edits[..3], shear[..3]);
        assert_eq!(edits[3], [4.0, 8.0, 12.0, 1.0]);
    }
}
