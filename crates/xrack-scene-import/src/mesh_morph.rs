use gltf::{Primitive, mesh::Semantic};
use nalgebra::Vector3;
use xrack_runtime::scene3d::{MAX_MORPH_DELTAS, MAX_MORPH_TARGETS, MorphDelta};

use crate::{mesh::ImportData, mesh_attributes::Attribute};

type TextureDeltas = [[f32; 2]; 2];
type Delta = (MorphDelta, TextureDeltas);

pub(super) struct MorphData {
    pub count: usize,
    pub offsets: Vec<u32>,
    pub deltas: Vec<MorphDelta>,
    pub uvs: Vec<TextureDeltas>,
}

impl MorphData {
    pub fn new(count: usize) -> Self {
        Self {
            count,
            offsets: vec![0],
            deltas: Vec::new(),
            uvs: Vec::new(),
        }
    }

    fn vertex(&self, index: usize) -> impl Iterator<Item = Delta> + '_ {
        let start = self.offsets[index] as usize;
        let end = self.offsets[index + 1] as usize;
        (start..end).map(|record| {
            (
                self.deltas[record],
                self.uvs.get(record).copied().unwrap_or_default(),
            )
        })
    }

    fn push_delta(&mut self, (delta, uv): Delta) {
        if uv != [[0.0; 2]; 2] {
            self.uvs.resize(self.deltas.len(), [[0.0; 2]; 2]);
            self.uvs.push(uv);
        } else if !self.uvs.is_empty() {
            self.uvs.push(uv);
        }
        self.deltas.push(delta);
    }

    fn append_vertex(&mut self, deltas: impl IntoIterator<Item = Delta>) -> Result<(), String> {
        for delta in deltas.into_iter().filter(|delta| nonzero(*delta)) {
            if self.deltas.len() >= MAX_MORPH_DELTAS {
                return Err("glTF morph displacements exceed the limit".into());
            }
            self.push_delta(delta);
        }
        self.offsets.push(self.deltas.len() as u32);
        Ok(())
    }

    pub fn append_source_vertex(&mut self, source: &Self, index: usize) -> Result<(), String> {
        self.append_vertex(source.vertex(index))
    }

    pub fn append_triangle(
        &mut self,
        source: &MorphData,
        indices: [usize; 3],
        positions: [Vector3<f32>; 3],
        flat_normal: Option<Vector3<f32>>,
    ) -> Result<(), String> {
        if let Some(base) = flat_normal {
            // glTF derives flat normals per target when the base omitted NORMAL.
            // UV deltas stay attached to their original vertex when it is expanded.
            let mut corner = [[(MorphDelta::default(), [[0.0; 2]; 2]); MAX_MORPH_TARGETS]; 3];
            for (deltas, index) in corner.iter_mut().zip(indices) {
                for (delta, uv) in source.vertex(index) {
                    let record = &mut deltas[delta.target as usize];
                    record.0.position = delta.position;
                    record.1 = uv;
                }
            }
            for target in 0..source.count {
                let [a, b, c] = std::array::from_fn(|i| {
                    positions[i] + Vector3::from(corner[i][target].0.position)
                });
                let normal = (b - a).cross(&(c - a)).try_normalize(1e-12).unwrap_or(base);
                let normal: [f32; 3] = (normal - base).into();
                for deltas in &mut corner {
                    deltas[target].0.target = target as u32;
                    deltas[target].0.normal = normal;
                }
            }
            for deltas in corner {
                self.append_vertex(deltas.into_iter().take(source.count))?;
            }
        } else {
            for index in indices {
                self.append_source_vertex(source, index)?;
            }
        }
        Ok(())
    }
}

fn nonzero((delta, uv): Delta) -> bool {
    delta.position != [0.0; 3]
        || delta.normal != [0.0; 3]
        || delta.tangent != [0.0; 3]
        || uv != [[0.0; 2]; 2]
}

fn next<const N: usize>(
    stream: &mut Option<Box<dyn ExactSizeIterator<Item = [f32; N]> + '_>>,
) -> Result<[f32; N], String> {
    let vector = stream
        .as_mut()
        .map_or(Some([0.0; N]), Iterator::next)
        .ok_or("short glTF morph accessor")?;
    if !vector.into_iter().all(f32::is_finite) {
        return Err("invalid glTF morph displacement".into());
    }
    Ok(vector)
}

pub(super) fn read_morph(
    source: ImportData<'_>,
    mesh: usize,
    primitive: &Primitive<'_>,
    count: usize,
    vertices: usize,
    uv_sets: [Option<u32>; 2],
) -> Result<MorphData, String> {
    if primitive.morph_targets().len() != count || count > MAX_MORPH_TARGETS {
        return Err("inconsistent or oversized glTF morph target list".into());
    }
    let mut records = Vec::new();
    for (target, target_data) in primitive.morph_targets().enumerate() {
        let read = |accessor: Option<gltf::Accessor<'_>>, semantic, attribute| {
            accessor
                .map(|accessor| {
                    if primitive.get(&semantic).is_none() {
                        return Err("glTF morph target has no corresponding base attribute".into());
                    }
                    source.attributes.read::<3>(accessor, vertices, attribute)
                })
                .transpose()
        };
        let mut position = read(
            target_data.positions(),
            Semantic::Positions,
            Attribute::MorphPosition,
        )?;
        let mut normal = read(
            target_data.normals(),
            Semantic::Normals,
            Attribute::Direction,
        )?;
        let mut tangent = read(
            target_data.tangents(),
            Semantic::Tangents,
            Attribute::Direction,
        )?;
        let mut uvs = [None, None];
        for (set, accessor) in
            source
                .metadata
                .morph_texcoord_accessors(mesh, primitive.index(), target)
        {
            if primitive.get(&Semantic::TexCoords(set)).is_none() {
                return Err("glTF morph target has no corresponding base UV set".into());
            }
            let accessor = source
                .document
                .accessors()
                .nth(accessor)
                .ok_or("glTF morph UV accessor index is out of bounds")?;
            let values =
                source
                    .attributes
                    .read::<2>(accessor, vertices, Attribute::MorphTexcoord)?;
            if let Some(slot) = uv_sets.iter().position(|value| *value == Some(set)) {
                uvs[slot] = Some(values);
            }
        }
        let [mut uv, mut uv1] = uvs;
        for vertex in 0..vertices {
            let delta = (
                MorphDelta {
                    target: target as u32,
                    position: next(&mut position)?,
                    normal: next(&mut normal)?,
                    tangent: next(&mut tangent)?,
                },
                [next(&mut uv)?, next(&mut uv1)?],
            );
            if nonzero(delta) {
                if records.len() == MAX_MORPH_DELTAS {
                    return Err("glTF morph displacements exceed the limit".into());
                }
                records.push((vertex, delta));
            }
        }
    }
    records.sort_unstable_by_key(|(vertex, delta)| (*vertex, delta.0.target));
    let mut records = records.into_iter().peekable();
    let mut result = MorphData::new(count);
    for vertex in 0..vertices {
        while records.peek().is_some_and(|(index, _)| *index == vertex) {
            let Some((_, delta)) = records.next() else {
                break;
            };
            result.push_delta(delta);
        }
        result.offsets.push(result.deltas.len() as u32);
    }
    Ok(result)
}
