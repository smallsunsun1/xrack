//! glTF EXT_mesh_gpu_instancing tables share the ordinary mesh and accessor pipeline.
use std::collections::BTreeMap;

use gltf::accessor::{DataType, Dimensions, Item, Iter};
use xrack_runtime::scene3d::{MAX_MESH_INSTANCES, MeshInstances, MeshNodeTransform};
use xrack_syntax::gltf_metadata::{
    GltfMetadata, INSTANCE_COLOR_ALIAS, INSTANCE_COLOR_ATTRIBUTE, INSTANCE_DATA_ATTRIBUTE,
};

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
    let mut colors = None;
    let mut data = None;
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
            INSTANCE_COLOR_ALIAS if attributes.contains_key(INSTANCE_COLOR_ATTRIBUTE) => {}
            INSTANCE_COLOR_ALIAS | INSTANCE_COLOR_ATTRIBUTE => {
                colors = Some(read_colors(accessor, buffers, name)?);
            }
            INSTANCE_DATA_ATTRIBUTE => {
                if accessor.dimensions() != Dimensions::Vec4 {
                    return Err(format!("{name} requires a VEC4 accessor"));
                }
                let values = read_channels::<4>(accessor, buffers, true, name)?;
                if values.iter().flatten().any(|value| !value.is_finite()) {
                    return Err(format!("{name} requires finite channels"));
                }
                data = Some(values);
            }
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
    .ok_or("instance transforms require finite values and nonzero rotation quaternions")?
    .with_attributes(colors, data)
    .ok_or("invalid optional instance channels")?;
    Ok((count, instances))
}

fn read_colors(
    accessor: gltf::Accessor<'_>,
    buffers: &[Vec<u8>],
    name: &str,
) -> Result<Vec<[f32; 4]>, String> {
    let colors = match accessor.dimensions() {
        Dimensions::Vec3 => read_channels::<3>(accessor, buffers, false, name)?
            .into_iter()
            .map(|[r, g, b]| [r, g, b, 1.0])
            .collect(),
        Dimensions::Vec4 => read_channels::<4>(accessor, buffers, false, name)?,
        _ => return Err(format!("{name} requires a VEC3 or VEC4 accessor")),
    };
    if colors
        .iter()
        .flatten()
        .any(|value| !(0.0..=1.0).contains(value))
    {
        return Err(format!(
            "{name} requires RGBA channels between zero and one"
        ));
    }
    Ok(colors)
}

fn read_channels<const N: usize>(
    accessor: gltf::Accessor<'_>,
    buffers: &[Vec<u8>],
    signed: bool,
    name: &str,
) -> Result<Vec<[f32; N]>, String>
where
    [f32; N]: Item,
    [u8; N]: Item,
    [u16; N]: Item,
    [i8; N]: Item,
    [i16; N]: Item,
{
    match (accessor.data_type(), accessor.normalized()) {
        (DataType::F32, false) => read_values(accessor, buffers),
        (DataType::U8, true) => Ok(read_values::<[u8; N]>(accessor, buffers)?
            .into_iter()
            .map(|row| row.map(|channel| f32::from(channel) / 255.0))
            .collect()),
        (DataType::U16, true) => Ok(read_values::<[u16; N]>(accessor, buffers)?
            .into_iter()
            .map(|row| row.map(|channel| f32::from(channel) / 65535.0))
            .collect()),
        (DataType::I8, true) if signed => Ok(read_values::<[i8; N]>(accessor, buffers)?
            .into_iter()
            .map(|row| row.map(|channel| (f32::from(channel) / 127.0).max(-1.0)))
            .collect()),
        (DataType::I16, true) if signed => Ok(read_values::<[i16; N]>(accessor, buffers)?
            .into_iter()
            .map(|row| row.map(|channel| (f32::from(channel) / 32767.0).max(-1.0)))
            .collect()),
        _ => Err(format!(
            "{name} requires float or normalized {}integer components",
            if signed { "" } else { "unsigned " }
        )),
    }
}

fn read_rotations(
    accessor: gltf::Accessor<'_>,
    buffers: &[Vec<u8>],
) -> Result<Vec<[f32; 4]>, String> {
    let format = (accessor.data_type(), accessor.normalized());
    if accessor.dimensions() != Dimensions::Vec4
        || !matches!(
            format,
            (DataType::F32, false) | (DataType::I8 | DataType::I16, true)
        )
    {
        return Err("ROTATION requires a VEC4 float or normalized signed integer accessor".into());
    }
    read_channels::<4>(accessor, buffers, true, "ROTATION")
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn decodes_normalized_channels_and_rejects_invalid_stream_layouts() {
        let source = br#"{
            "asset": {"version":"2.0"},
            "buffers": [{"byteLength":20}],
            "bufferViews": [
                {"buffer":0,"byteOffset":0,"byteLength":12},
                {"buffer":0,"byteOffset":12,"byteLength":8}
            ],
            "accessors": [
                {"bufferView":0,"count":2,"componentType":5123,"type":"VEC3","normalized":true},
                {"bufferView":1,"count":2,"componentType":5120,"type":"VEC4","normalized":true}
            ]
        }"#;
        let gltf = gltf::Gltf::from_slice(source).expect("valid normalized accessors");
        let mut bytes: Vec<_> = [65535u16, 0, 32768, 0, 65535, 0]
            .into_iter()
            .flat_map(u16::to_le_bytes)
            .collect();
        bytes.extend([128, 0, 127, 0, 127, 0, 128, 127]);
        let buffers = [bytes];
        let attributes = BTreeMap::from([
            (INSTANCE_COLOR_ALIAS.to_owned(), 0),
            (INSTANCE_DATA_ATTRIBUTE.to_owned(), 1),
        ]);
        let (count, instances) =
            read_group(&gltf.document, &attributes, &buffers).expect("two valid instances");
        assert_eq!(count, 2);
        assert_eq!(
            instances.colors(),
            &[[1.0, 0.0, 32768.0 / 65535.0, 1.0], [0.0, 1.0, 0.0, 1.0]]
        );
        assert_eq!(
            instances.data(),
            &[[-1.0, 0.0, 1.0, 0.0], [1.0, 0.0, -1.0, 1.0]]
        );

        let transforms: Vec<_> = instances
            .packed_words()
            .flat_map(f32::to_le_bytes)
            .collect();
        let channels: Vec<_> = instances
            .data()
            .iter()
            .flatten()
            .copied()
            .flat_map(f32::to_le_bytes)
            .collect();
        assert!(MeshInstances::from_packed(&transforms, &[], &channels[..16]).is_none());
        let mut invalid = channels;
        invalid[..4].copy_from_slice(&f32::NAN.to_le_bytes());
        assert!(MeshInstances::from_packed(&transforms, &[], &invalid).is_none());

        let bad_channels = BTreeMap::from([(INSTANCE_DATA_ATTRIBUTE.to_owned(), 0)]);
        let error = read_group(&gltf.document, &bad_channels, &buffers)
            .expect_err("VEC3 is not instance data");
        assert!(error.contains("_INSTANCE_DATA requires a VEC4 accessor"));
        let bad_color = BTreeMap::from([(INSTANCE_COLOR_ATTRIBUTE.to_owned(), 1)]);
        let error = read_group(&gltf.document, &bad_color, &buffers).expect_err("signed colors");
        assert!(
            error.contains("_COLOR_0 requires float or normalized unsigned integer components")
        );
    }
}
