use gltf::{
    Accessor, Primitive,
    accessor::{DataType, Dimensions},
};
use nalgebra::Vector3;
use xrack_runtime::scene3d::{MAX_MORPH_DELTAS, MAX_MORPH_TARGETS, MorphDelta};

use crate::mesh::validate_accessor_with_sparse;

pub(super) struct MorphData {
    pub count: usize,
    pub offsets: Vec<u32>,
    pub deltas: Vec<MorphDelta>,
}

impl MorphData {
    pub fn new(count: usize) -> Self {
        Self {
            count,
            offsets: vec![0],
            deltas: Vec::new(),
        }
    }

    fn vertex(&self, index: usize) -> &[MorphDelta] {
        &self.deltas[self.offsets[index] as usize..self.offsets[index + 1] as usize]
    }

    fn append_vertex(
        &mut self,
        deltas: impl IntoIterator<Item = MorphDelta>,
    ) -> Result<(), String> {
        for delta in deltas {
            if delta.position == [0.0; 3] && delta.normal == [0.0; 3] && delta.tangent == [0.0; 3] {
                continue;
            }
            if self.deltas.len() >= MAX_MORPH_DELTAS {
                return Err("glTF morph displacements exceed the limit".into());
            }
            self.deltas.push(delta);
        }
        self.offsets.push(self.deltas.len() as u32);
        Ok(())
    }

    pub fn append_triangle(
        &mut self,
        source: &MorphData,
        indices: [usize; 3],
        positions: [Vector3<f32>; 3],
        flat_normal: Option<Vector3<f32>>,
    ) -> Result<(), String> {
        if let Some(base) = flat_normal {
            // glTF requires flat normals for each authored target when the
            // primitive omitted NORMAL in its base geometry.
            let mut corner = [[MorphDelta::default(); MAX_MORPH_TARGETS]; 3];
            for (deltas, index) in corner.iter_mut().zip(indices) {
                for delta in source.vertex(index) {
                    deltas[delta.target as usize].position = delta.position;
                }
            }
            for target in 0..source.count {
                let [a, b, c] = std::array::from_fn(|i| {
                    positions[i] + Vector3::from(corner[i][target].position)
                });
                let normal = (b - a).cross(&(c - a)).try_normalize(1e-12).unwrap_or(base);
                let normal: [f32; 3] = (normal - base).into();
                for deltas in &mut corner {
                    deltas[target].target = target as u32;
                    deltas[target].normal = normal;
                }
            }
            for deltas in corner {
                self.append_vertex(deltas.into_iter().take(source.count))?;
            }
        } else {
            for index in indices {
                self.append_vertex(source.vertex(index).iter().copied())?;
            }
        }
        Ok(())
    }
}

pub(super) fn read_morph(
    primitive: &Primitive<'_>,
    buffers: &[Vec<u8>],
    count: usize,
    vertices: usize,
) -> Result<MorphData, String> {
    use gltf::mesh::Semantic;
    if primitive.morph_targets().len() != count || count > MAX_MORPH_TARGETS {
        return Err("inconsistent or oversized glTF morph target list".into());
    }
    let mut channels = Vec::new();
    for target in primitive.morph_targets() {
        let accessors = [target.positions(), target.normals(), target.tangents()];
        for (accessor, base) in
            accessors
                .iter()
                .zip([Semantic::Positions, Semantic::Normals, Semantic::Tangents])
        {
            if let Some(accessor) = accessor {
                if primitive.get(&base).is_none() {
                    return Err("glTF morph target has no corresponding base attribute".into());
                }
                validate_morph_accessor(accessor, buffers, vertices)?;
            }
        }
        channels.push(accessors.map(|accessor| accessor.is_some()));
    }
    let reader = primitive.reader(|buffer| buffers.get(buffer.index()).map(Vec::as_slice));
    let mut records = Vec::new();
    for (target, ((mut position, mut normal, mut tangent), present)) in
        reader.read_morph_targets().zip(channels).enumerate()
    {
        if [position.is_some(), normal.is_some(), tangent.is_some()] != present {
            return Err("unable to read glTF morph target accessor".into());
        }
        for vertex in 0..vertices {
            let read = |channel: &mut Option<gltf::accessor::Iter<'_, [f32; 3]>>| {
                channel
                    .as_mut()
                    .map_or(Some([0.0; 3]), Iterator::next)
                    .ok_or("short glTF morph accessor")
            };
            let position = read(&mut position)?;
            let normal = read(&mut normal)?;
            let tangent = read(&mut tangent)?;
            if !position
                .into_iter()
                .chain(normal)
                .chain(tangent)
                .all(f32::is_finite)
            {
                return Err("invalid glTF morph displacement".into());
            }
            if position != [0.0; 3] || normal != [0.0; 3] || tangent != [0.0; 3] {
                if records.len() == MAX_MORPH_DELTAS {
                    return Err("glTF morph displacements exceed the limit".into());
                }
                records.push((
                    vertex,
                    MorphDelta {
                        target: target as u32,
                        position,
                        normal,
                        tangent,
                    },
                ));
            }
        }
    }
    records.sort_unstable_by_key(|(vertex, delta)| (*vertex, delta.target));
    let mut records = records.into_iter().peekable();
    let mut result = MorphData::new(count);
    for vertex in 0..vertices {
        while records.peek().is_some_and(|(index, _)| *index == vertex) {
            let Some((_, delta)) = records.next() else {
                break;
            };
            result.deltas.push(delta);
        }
        result.offsets.push(result.deltas.len() as u32);
    }
    Ok(result)
}

fn validate_morph_accessor(
    accessor: &Accessor<'_>,
    buffers: &[Vec<u8>],
    vertices: usize,
) -> Result<(), String> {
    if accessor.count() != vertices
        || accessor.dimensions() != Dimensions::Vec3
        || accessor.data_type() != DataType::F32
        || accessor.normalized()
    {
        return Err("invalid glTF morph target accessor type".into());
    }
    validate_accessor_with_sparse(accessor, buffers)
}
