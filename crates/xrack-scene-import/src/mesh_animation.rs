//! Validate and lower glTF animation accessors for the shared CPU sampler.
use std::collections::BTreeMap;

use gltf::{
    accessor::{DataType, Dimensions},
    animation::{Interpolation, Property, util::ReadOutputs},
};
use xrack_runtime::scene3d::{
    MAX_ANIMATION_KEYS, MeshAnimationClip, MeshAnimationNode, MeshAnimationProperty,
    MeshAnimationTrack, MeshInterpolation, MorphAnimationTrack,
};

use crate::mesh::validate_accessor_with_sparse;

use xrack_syntax::gltf_metadata::GltfMetadata;

pub(super) fn read_clips(
    document: &gltf::Document,
    metadata: &GltfMetadata,
    buffers: &[Vec<u8>],
    nodes: &BTreeMap<usize, usize>,
    scene: &[MeshAnimationNode],
) -> Result<Vec<MeshAnimationClip>, String> {
    let mut key_count = 0usize;
    document
        .animations()
        .map(|animation| {
            let index = animation.index();
            let mut tracks = Vec::new();
            let mut morph_tracks = Vec::new();
            for channel in animation.channels() {
                let Some(&node) = nodes.get(&channel.target().node().index()) else {
                    continue;
                };
                let target = channel.target().property();
                let (property, dimensions) = match target {
                    Property::Translation => {
                        (Some(MeshAnimationProperty::Translation), Dimensions::Vec3)
                    }
                    Property::Rotation => (Some(MeshAnimationProperty::Rotation), Dimensions::Vec4),
                    Property::Scale => (Some(MeshAnimationProperty::Scale), Dimensions::Vec3),
                    Property::MorphTargetWeights => (None, Dimensions::Scalar),
                };
                let width = if property.is_none() {
                    scene[node].morph_weights.len()
                } else {
                    1
                };
                if width == 0 {
                    return Err("glTF weights channel has no target mesh".into());
                }
                let sampler = channel.sampler();
                let input = sampler.input();
                let output = sampler.output();
                let output_type =
                    if property.is_none() || property == Some(MeshAnimationProperty::Rotation) {
                        match output.data_type() {
                            DataType::F32 => !output.normalized(),
                            DataType::I8 | DataType::U8 | DataType::I16 | DataType::U16 => {
                                output.normalized()
                            }
                            DataType::U32 => false,
                        }
                    } else {
                        output.data_type() == DataType::F32 && !output.normalized()
                    };
                if input.dimensions() != Dimensions::Scalar
                    || input.data_type() != DataType::F32
                    || input.normalized()
                    || output.dimensions() != dimensions
                    || !output_type
                {
                    return Err("invalid glTF animation accessor type".into());
                }
                let interpolation = match sampler.interpolation() {
                    Interpolation::Step => MeshInterpolation::Step,
                    Interpolation::Linear => MeshInterpolation::Linear,
                    Interpolation::CubicSpline => MeshInterpolation::CubicSpline,
                };
                let slots = if interpolation == MeshInterpolation::CubicSpline {
                    3
                } else {
                    1
                };
                key_count = key_count.saturating_add(input.count().saturating_mul(width));
                if key_count > MAX_ANIMATION_KEYS
                    || input
                        .count()
                        .checked_mul(slots)
                        .and_then(|count| count.checked_mul(width))
                        != Some(output.count())
                {
                    return Err("glTF animation accessor counts do not match".into());
                }
                validate_accessor_with_sparse(&input, buffers)?;
                validate_accessor_with_sparse(&output, buffers)?;
                let reader =
                    channel.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
                let times = reader
                    .read_inputs()
                    .ok_or("unable to read glTF animation times")?
                    .collect();
                let values = match reader
                    .read_outputs()
                    .ok_or("unable to read glTF animation values")?
                {
                    ReadOutputs::Translations(values) | ReadOutputs::Scales(values) => {
                        values.map(|[x, y, z]| [x, y, z, 0.0]).collect()
                    }
                    ReadOutputs::Rotations(values) => values.into_f32().collect(),
                    ReadOutputs::MorphTargetWeights(values) => {
                        morph_tracks.push(MorphAnimationTrack {
                            node,
                            width,
                            interpolation,
                            times,
                            values: values.into_f32().collect(),
                        });
                        continue;
                    }
                };
                let property =
                    property.ok_or("glTF animation sampler does not match its channel")?;
                tracks.push(MeshAnimationTrack {
                    node,
                    property,
                    interpolation,
                    times,
                    values,
                });
            }
            let name = metadata
                .animations
                .get(index)
                .and_then(|clip| clip.name.clone());
            Ok(MeshAnimationClip {
                index,
                name,
                tracks,
                morph_tracks,
            })
        })
        .collect()
}
