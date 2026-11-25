//! Sparse per-vertex glTF displacements and conservative posed bounds.
use crate::{scene_skin::Bounds, scene3d::MeshVertex};
use std::{ops::Range, sync::Arc};

pub const MAX_MORPH_TARGETS: usize = 32;
pub const MAX_MORPH_DELTAS: usize = 1_500_000;
pub type MorphWeights = Arc<[f32]>;

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MorphDelta {
    pub target: u32,
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub tangent: [f32; 3],
}

impl MorphDelta {
    pub(crate) fn words(&self) -> [u32; 10] {
        let [px, py, pz] = self.position.map(f32::to_bits);
        let [nx, ny, nz] = self.normal.map(f32::to_bits);
        let [tx, ty, tz] = self.tangent.map(f32::to_bits);
        [self.target, px, py, pz, nx, ny, nz, tx, ty, tz]
    }
}

type TargetBounds = Vec<Bounds>;
pub(crate) enum MorphOverrides {
    Partial(Box<[(usize, f32)]>),
    Complete(MorphWeights),
}

/// The first V+1 packed words are offsets into ten-word delta records.
#[derive(Debug)]
pub struct MorphGeometry {
    target_count: usize,
    names: Arc<[String]>,
    offsets: Vec<u32>,
    deltas: Vec<MorphDelta>,
    bounds: TargetBounds,
    blended: Vec<(usize, Bounds, TargetBounds)>,
}

impl MorphGeometry {
    pub(crate) fn new(
        target_count: usize,
        names: Arc<[String]>,
        offsets: Vec<u32>,
        deltas: Vec<MorphDelta>,
        vertices: &[MeshVertex],
        blended_parts: impl Iterator<Item = (usize, Range<usize>)>,
    ) -> Option<Self> {
        if !(1..=MAX_MORPH_TARGETS).contains(&target_count)
            || (!names.is_empty() && names.len() != target_count)
            || offsets.len() != vertices.len() + 1
            || deltas.len() > MAX_MORPH_DELTAS
            || offsets.first().copied() != Some(0)
            || offsets.last().copied() != Some(deltas.len() as u32)
            || offsets.windows(2).any(|pair| pair[0] > pair[1])
            || deltas.iter().any(|delta| {
                delta.target as usize >= target_count
                    || !delta
                        .position
                        .into_iter()
                        .chain(delta.normal)
                        .chain(delta.tangent)
                        .all(f32::is_finite)
            })
        {
            return None;
        }
        let bounds = target_bounds(&deltas, target_count);
        let blended = blended_parts
            .map(|(index, range)| {
                let mut min = [f32::INFINITY; 3];
                let mut max = [f32::NEG_INFINITY; 3];
                for vertex in &vertices[range.clone()] {
                    for (i, value) in vertex.position.into_iter().enumerate() {
                        min[i] = min[i].min(value);
                        max[i] = max[i].max(value);
                    }
                }
                let slice = &deltas[offsets[range.start] as usize..offsets[range.end] as usize];
                (index, (min, max), target_bounds(slice, target_count))
            })
            .collect();
        Some(Self {
            target_count,
            names,
            offsets,
            deltas,
            bounds,
            blended,
        })
    }

    #[doc(hidden)]
    pub fn delta_count(&self) -> usize {
        self.deltas.len()
    }

    pub fn target_count(&self) -> usize {
        self.target_count
    }

    pub fn target_names(&self) -> &Arc<[String]> {
        &self.names
    }

    pub(crate) fn overrides(&self, fields: &[(&str, f64)]) -> Option<MorphOverrides> {
        let ordinal = |name: &str| {
            let digits = name.strip_prefix("target-")?;
            if (digits.len() > 1 && digits.starts_with('0'))
                || !digits.bytes().all(|byte| byte.is_ascii_digit())
            {
                return None;
            }
            digits.parse::<usize>().ok()
        };
        let mut resolved = [None; MAX_MORPH_TARGETS];
        let mut authored = [None; MAX_MORPH_TARGETS];
        for &(name, value) in fields {
            let value = value as f32;
            if !value.is_finite() {
                continue;
            }
            if let Some(index) = ordinal(name) {
                if index < self.target_count {
                    resolved[index] = Some(value);
                }
            } else if let Some(index) = self.names.iter().position(|target| target == name) {
                authored[index] = Some(value);
            }
        }
        let targets = authored
            .into_iter()
            .zip(resolved)
            .take(self.target_count)
            .map(|(named, ordinal)| named.or(ordinal));
        let count = targets.clone().flatten().count();
        match count {
            0 => None,
            count if count == self.target_count => {
                Some(MorphOverrides::Complete(targets.flatten().collect()))
            }
            _ => Some(MorphOverrides::Partial(
                targets
                    .enumerate()
                    .filter_map(|(index, value)| Some((index, value?)))
                    .collect(),
            )),
        }
    }

    pub fn packed_words(&self) -> impl Iterator<Item = u32> + '_ {
        self.offsets
            .iter()
            .copied()
            .chain(self.deltas.iter().flat_map(MorphDelta::words))
    }

    /// Pack the sparse displacement table in the same row order as GPU vertex buffers.
    #[doc(hidden)]
    pub fn packed_draw_rows<'a>(&'a self, rows: &'a [u32]) -> impl Iterator<Item = u32> + 'a {
        let deltas = rows.iter().map(|&row| self.vertex(row as usize));
        let offsets = deltas.clone().scan(0, |offset, row| {
            *offset += row.len() as u32;
            Some(*offset)
        });
        std::iter::once(0)
            .chain(offsets)
            .chain(deltas.flatten().flat_map(MorphDelta::words))
    }

    #[doc(hidden)]
    pub fn vertex_deltas(&self, vertex: usize) -> Option<&[MorphDelta]> {
        let offsets = self.offsets.get(vertex..vertex.checked_add(2)?)?;
        self.deltas.get(offsets[0] as usize..offsets[1] as usize)
    }

    pub(crate) fn subset(&self, vertices: &[usize]) -> Option<(Vec<u32>, Vec<MorphDelta>)> {
        let mut offsets = Vec::with_capacity(vertices.len() + 1);
        let mut deltas = Vec::new();
        offsets.push(0);
        for &source in vertices {
            let changes = self.vertex_deltas(source)?;
            if deltas.len().checked_add(changes.len())? > MAX_MORPH_DELTAS {
                return None;
            }
            deltas.extend_from_slice(changes);
            offsets.push(deltas.len() as u32);
        }
        Some((offsets, deltas))
    }

    pub(crate) fn vertex(&self, vertex: usize) -> &[MorphDelta] {
        &self.deltas[self.offsets[vertex] as usize..self.offsets[vertex + 1] as usize]
    }

    pub(crate) fn deform<const NORMALS: bool>(
        &self,
        vertex: usize,
        weights: &[f32],
        mut base: MeshVertex,
    ) -> MeshVertex {
        let mut position = [0.0; 3];
        let mut normal = [0.0; 3];
        for delta in self.vertex(vertex) {
            let weight = weights[delta.target as usize];
            for axis in 0..3 {
                position[axis] += delta.position[axis] * weight;
                if NORMALS {
                    normal[axis] += delta.normal[axis] * weight;
                }
            }
        }
        for (axis, delta) in position.into_iter().enumerate() {
            base.position[axis] += delta;
        }
        if NORMALS {
            let normal = nalgebra::Vector3::from(base.normal) + nalgebra::Vector3::from(normal);
            if let Some(normal) = normal.try_normalize(1e-12) {
                base.normal = normal.into();
            }
        }
        base
    }
}

fn target_bounds(deltas: &[MorphDelta], count: usize) -> TargetBounds {
    // Include zero for vertices with no entry for a given target.
    let mut bounds = vec![([0.0_f32; 3], [0.0_f32; 3]); count];
    for delta in deltas {
        let (min, max) = &mut bounds[delta.target as usize];
        for (i, value) in delta.position.into_iter().enumerate() {
            min[i] = min[i].min(value);
            max[i] = max[i].max(value);
        }
    }
    bounds
}

fn displacement(bounds: &TargetBounds, weights: &[f32]) -> Bounds {
    let mut min = [0.0; 3];
    let mut max = [0.0; 3];
    for (&(a, b), &weight) in bounds.iter().zip(weights) {
        for i in 0..3 {
            min[i] += (a[i] * weight).min(b[i] * weight);
            max[i] += (a[i] * weight).max(b[i] * weight);
        }
    }
    (min, max)
}

pub(crate) fn expanded((min, max): Bounds, (low, high): Bounds) -> Bounds {
    (
        std::array::from_fn(|i| min[i] + low[i]),
        std::array::from_fn(|i| max[i] + high[i]),
    )
}

#[derive(Debug, PartialEq)]
pub(crate) struct MorphPose {
    pub weights: MorphWeights,
    pub bounds: Bounds,
    pub displacement: Bounds,
    pub blended_parts: Vec<(usize, [f32; 3])>,
    part_displacements: Vec<(usize, Bounds)>,
}

impl MorphPose {
    pub(crate) fn new(
        geometry: &MorphGeometry,
        weights: MorphWeights,
        base: Bounds,
    ) -> Option<Self> {
        if weights.len() != geometry.target_count
            || !weights.iter().all(|weight| weight.is_finite())
        {
            return None;
        }
        let delta = displacement(&geometry.bounds, &weights);
        let bounds = expanded(base, delta);
        if !bounds.0.into_iter().chain(bounds.1).all(f32::is_finite) {
            return None;
        }
        let mut blended_parts = Vec::with_capacity(geometry.blended.len());
        let mut part_displacements = Vec::with_capacity(geometry.blended.len());
        for (part, base, targets) in &geometry.blended {
            let local = displacement(targets, &weights);
            let (min, max) = expanded(*base, local);
            blended_parts.push((
                *part,
                std::array::from_fn(|i| f32::midpoint(min[i], max[i])),
            ));
            part_displacements.push((*part, local));
        }
        Some(Self {
            weights,
            bounds,
            displacement: delta,
            blended_parts,
            part_displacements,
        })
    }

    pub(crate) fn part_displacement(&self, part: usize) -> Bounds {
        self.part_displacements
            .iter()
            .find(|(index, _)| *index == part)
            .map_or(self.displacement, |(_, bounds)| *bounds)
    }
}
