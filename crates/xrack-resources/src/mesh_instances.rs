//! glTF EXT_mesh_gpu_instancing tables share the ordinary mesh and accessor pipeline.
use std::collections::BTreeMap;

use gltf::accessor::{DataType, Dimensions, Item, Iter};
use xrack_runtime::scene3d::{MAX_MESH_INSTANCES, MeshInstances, MeshNodeTransform};
use xrack_syntax::gltf_metadata::GltfMetadata;

use crate::mesh::validate_accessor_with_sparse;

pub(super) fn read(
    document: &gltf::Document,
    metadata: &GltfMetadata,
    buffers: &[Vec<u8>],
) -> Result<BTreeMap<usize, MeshInstances>, String> {
    let mut groups = BTreeMap::new();
    let mut instances = 0;
    for (index, attributes) in metadata.gpu_instance_attributes() {
        let result = (|| {
            if document
                .nodes()
                .nth(index)
                .and_then(|node| node.mesh())
                .is_none()
            {
                return Err("requires a node with a mesh".into());
            }
            let (count, group) = read_group(document, attributes, buffers)?;
            instances += count;
            if instances > MAX_MESH_INSTANCES {
                return Err("exceeds the mesh instance placement limit".into());
            }
            Ok(group)
        })();
        let group = result.map_err(|error: String| {
            format!("glTF EXT_mesh_gpu_instancing on node #{index}: {error}")
        })?;
        groups.insert(index, group);
    }
    if groups.is_empty() && metadata.requires_gpu_instancing() {
        return Err("glTF requires EXT_mesh_gpu_instancing but has no instance groups".into());
    }
    Ok(groups)
}

fn read_group(
    document: &gltf::Document,
    attributes: &BTreeMap<String, usize>,
    buffers: &[Vec<u8>],
) -> Result<(usize, MeshInstances), String> {
    let mut count = None;
    let mut translations = None;
    let mut rotations = None;
    let mut scales = None;
    for (name, &index) in attributes {
        let accessor = document
            .accessors()
            .nth(index)
            .ok_or("invalid instance accessor index")?;
        let next = accessor.count();
        if !(1..=MAX_MESH_INSTANCES).contains(&next)
            || count.replace(next).is_some_and(|old| old != next)
        {
            return Err(
                "all instance attributes must have the same positive, bounded count".into(),
            );
        }
        validate_accessor_with_sparse(&accessor, buffers)?;
        match name.as_str() {
            "TRANSLATION" | "SCALE" => {
                if accessor.dimensions() != Dimensions::Vec3
                    || accessor.data_type() != DataType::F32
                    || accessor.normalized()
                {
                    return Err(format!(
                        "{name} requires an unnormalized VEC3 float accessor"
                    ));
                }
                let values = read_values(accessor, buffers)?;
                if name == "TRANSLATION" {
                    translations = Some(values);
                } else {
                    scales = Some(values);
                }
            }
            "ROTATION" => rotations = Some(read_rotations(accessor, buffers)?),
            custom if custom.starts_with('_') => {}
            _ => return Err(format!("unknown instance attribute {name:?}")),
        }
    }
    let count = count.ok_or("the instance attributes table is empty")?;
    let instances = MeshInstances::from_trs((0..count).map(|i| {
        MeshNodeTransform::Trs {
            translation: translations.as_ref().map_or([0.0; 3], |values| values[i]),
            rotation: rotations
                .as_ref()
                .map_or([0.0, 0.0, 0.0, 1.0], |values| values[i]),
            scale: scales.as_ref().map_or([1.0; 3], |values| values[i]),
        }
    }))
    .ok_or("instance transforms require finite values and nonzero rotation quaternions")?;
    Ok((count, instances))
}

fn read_rotations(
    accessor: gltf::Accessor<'_>,
    buffers: &[Vec<u8>],
) -> Result<Vec<[f32; 4]>, String> {
    if accessor.dimensions() == Dimensions::Vec4 {
        match (accessor.data_type(), accessor.normalized()) {
            (DataType::F32, false) => return read_values(accessor, buffers),
            (DataType::I8, true) => {
                return Ok(read_values::<[i8; 4]>(accessor, buffers)?
                    .into_iter()
                    .map(|values| values.map(|v| (f32::from(v) / 127.0).max(-1.0)))
                    .collect());
            }
            (DataType::I16, true) => {
                return Ok(read_values::<[i16; 4]>(accessor, buffers)?
                    .into_iter()
                    .map(|values| values.map(|v| (f32::from(v) / 32767.0).max(-1.0)))
                    .collect());
            }
            _ => {}
        }
    }
    Err("ROTATION requires a VEC4 float or normalized signed integer accessor".into())
}

fn read_values<T: Item>(
    accessor: gltf::Accessor<'_>,
    buffers: &[Vec<u8>],
) -> Result<Vec<T>, String> {
    let count = accessor.count();
    let values: Vec<_> = Iter::new(accessor, |buffer| {
        buffers.get(buffer.index()).map(Vec::as_slice)
    })
    .ok_or("unable to read instance attribute data")?
    .collect();
    if values.len() != count {
        return Err("instance attribute data is shorter than its declared count".into());
    }
    Ok(values)
}
