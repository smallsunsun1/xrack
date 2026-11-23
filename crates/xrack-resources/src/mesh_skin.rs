use gltf::{
    Primitive, Skin,
    accessor::{DataType, Dimensions},
    mesh::Semantic,
};
use std::collections::BTreeMap;
use xrack_runtime::scene3d::{MAX_SKIN_JOINTS, MeshInfluence, MeshSkin, MeshSkinJoint};

use crate::mesh::validate_accessor;

pub(super) fn read_skin(
    skin: Skin<'_>,
    nodes: &BTreeMap<usize, usize>,
    buffers: &[Vec<u8>],
) -> Result<MeshSkin, String> {
    let joints: Vec<_> = skin
        .joints()
        .map(|node| {
            nodes
                .get(&node.index())
                .copied()
                .ok_or("glTF skin joint is outside the selected scene")
        })
        .collect::<Result<_, _>>()?;
    if joints.is_empty() || joints.len() > MAX_SKIN_JOINTS {
        return Err(format!("glTF skins support 1 to {MAX_SKIN_JOINTS} joints"));
    }
    let binds = if let Some(bind) = skin.inverse_bind_matrices() {
        if bind.dimensions() != Dimensions::Mat4
            || bind.data_type() != DataType::F32
            || bind.normalized()
            || bind.count() < joints.len()
        {
            return Err("invalid glTF inverse bind matrices".into());
        }
        validate_accessor(&bind)?;
        skin.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice))
            .read_inverse_bind_matrices()
            .ok_or("unable to read inverse bind matrices")?
            .take(joints.len())
            .collect::<Vec<_>>()
    } else {
        let identity = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
            [0.0, 0.0, 0.0, 1.0],
        ];
        vec![identity; joints.len()]
    };
    if binds.len() != joints.len() {
        return Err("glTF inverse bind matrices do not cover the skin".into());
    }
    Ok(MeshSkin {
        joints: joints
            .into_iter()
            .zip(binds)
            .map(|(node, inverse_bind)| MeshSkinJoint { node, inverse_bind })
            .collect(),
    })
}

pub(super) fn read_influences(
    primitive: &Primitive<'_>,
    buffers: &[Vec<u8>],
    count: usize,
) -> Result<Vec<MeshInfluence>, String> {
    if primitive.attributes().any(|(semantic, _)| {
        matches!(semantic, Semantic::Joints(set) | Semantic::Weights(set) if set > 1)
    }) {
        return Err("glTF supports up to two joint/weight sets per vertex".into());
    }
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
    let read = |set| {
        let joints = primitive.get(&Semantic::Joints(set));
        let weights = primitive.get(&Semantic::Weights(set));
        let (joints, weights) = match (joints, weights) {
            (Some(joints), Some(weights)) => (joints, weights),
            (None, None) if set == 1 => return Ok(None),
            _ => {
                return Err(
                    "skinned glTF primitive requires paired JOINTS and WEIGHTS attributes".into(),
                );
            }
        };
        if joints.dimensions() != Dimensions::Vec4
            || joints.count() != count
            || joints.normalized()
            || !matches!(joints.data_type(), DataType::U8 | DataType::U16)
            || weights.dimensions() != Dimensions::Vec4
            || weights.count() != count
            || !match weights.data_type() {
                DataType::F32 => !weights.normalized(),
                DataType::U8 | DataType::U16 => weights.normalized(),
                _ => false,
            }
        {
            return Err("invalid glTF skin weight or joint accessor type".into());
        }
        validate_accessor(&joints)?;
        validate_accessor(&weights)?;
        let joints = reader
            .read_joints(set)
            .ok_or("unable to read glTF joints")?
            .into_u16();
        let weights = reader
            .read_weights(set)
            .ok_or("unable to read glTF weights")?
            .into_f32();
        Ok::<_, String>(Some(joints.zip(weights).collect::<Vec<_>>()))
    };
    let first = read(0)?.ok_or("missing glTF joint attributes")?;
    let second = read(1)?;
    if first.len() != count || second.as_ref().is_some_and(|values| values.len() != count) {
        return Err("glTF skin attributes do not match the vertex count".into());
    }
    first
        .into_iter()
        .enumerate()
        .map(|(index, ([a, b, c, d], [wa, wb, wc, wd]))| {
            let ([e, f, g, h], [we, wf, wg, wh]) = second
                .as_ref()
                .map_or(([0; 4], [0.0; 4]), |values| values[index]);
            MeshInfluence::normalized([a, b, c, d, e, f, g, h], [wa, wb, wc, wd, we, wf, wg, wh])
                .ok_or_else(|| "invalid glTF skin weights".to_owned())
        })
        .collect()
}
