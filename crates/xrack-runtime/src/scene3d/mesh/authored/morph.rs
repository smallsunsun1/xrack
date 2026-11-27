//! Convert deltas addressed by source vertex to the shared sparse morph format.

use std::sync::Arc;

use crate::{RecordValue, Value, VecModel, Vector3D};

use super::{
    super::morph::{
        MAX_MORPH_DELTAS, MAX_MORPH_TARGETS, MorphAttributes, MorphDelta, MorphTable, morph_ordinal,
    },
    Vertex,
};

/// One target's additive displacement of a source vertex.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SceneMorphDelta<'a> {
    pub target: &'a str,
    pub vertex: i64,
    pub position: Vector3D,
    pub normal: Vector3D,
    pub tangent: Vector3D,
    pub binormal: Vector3D,
    pub u: f64,
    pub v: f64,
    pub u1: f64,
    pub v1: f64,
    pub color_r: f64,
    pub color_g: f64,
    pub color_b: f64,
    pub color_a: f64,
}

pub(in crate::scene3d) fn record_morph_delta(row: &RecordValue) -> Option<SceneMorphDelta<'_>> {
    let (Some(Value::String(target)), Some(Value::Int(vertex))) =
        (row.field("target"), row.field("vertex"))
    else {
        return None;
    };
    let channel = |name| match row.field(name) {
        Some(Value::Vector3D(value)) => Some(*value),
        None => Some(Vector3D::default()),
        _ => None,
    };
    let scalar = |name| match row.field(name) {
        Some(Value::Float(value)) => Some(*value),
        None => Some(0.0),
        _ => None,
    };
    Some(SceneMorphDelta {
        target: target.as_str(),
        vertex: *vertex,
        position: channel("position")?,
        normal: channel("normal")?,
        tangent: channel("tangent")?,
        binormal: channel("binormal")?,
        u: scalar("u")?,
        v: scalar("v")?,
        u1: scalar("u1")?,
        v1: scalar("v1")?,
        color_r: scalar("color-r")?,
        color_g: scalar("color-g")?,
        color_b: scalar("color-b")?,
        color_a: scalar("color-a")?,
    })
}

#[derive(PartialEq)]
pub(super) struct AuthoredMorph {
    pub names: Arc<[String]>,
    table: MorphTable,
}

impl AuthoredMorph {
    pub(super) fn prepare<T>(
        vertices: &[Vertex],
        model: &VecModel<T>,
        mut read: impl for<'a> FnMut(&'a T) -> Option<SceneMorphDelta<'a>>,
    ) -> Option<Self> {
        let limit = MAX_MORPH_DELTAS.min(vertices.len() * MAX_MORPH_TARGETS);
        let mut names = Vec::new();
        let mut extra = Vec::new();
        let (count, rows) = model.inspect_rows(0, limit, |row| {
            let row = read(row)?;
            let vertex = usize::try_from(row.vertex).ok()?;
            let source = vertices.get(vertex)?;
            let tangent = finite([row.tangent.x, row.tangent.y, row.tangent.z])?;
            let binormal = finite([row.binormal.x, row.binormal.y, row.binormal.z])?;
            if row.target.is_empty()
                || morph_ordinal(row.target).is_some()
                || (tangent != [0.0; 3] || binormal != [0.0; 3]) && source.tangent.is_none()
            {
                return None;
            }
            let target = if let Some(index) = names.iter().position(|name| name == row.target) {
                index
            } else {
                let index = names.len();
                if index == MAX_MORPH_TARGETS {
                    return None;
                }
                names.push(row.target.to_owned());
                index
            };
            let attributes = MorphAttributes {
                uv: finite([row.u, row.v])?,
                uv1: finite([row.u1, row.v1])?,
                color: finite([row.color_r, row.color_g, row.color_b, row.color_a])?,
                binormal,
            };
            let index = if attributes == MorphAttributes::default() {
                None
            } else {
                let index = extra.len();
                extra.push(attributes);
                Some(index)
            };
            Some((
                vertex,
                MorphDelta {
                    target: target as u32,
                    position: finite([row.position.x, row.position.y, row.position.z])?,
                    normal: finite([row.normal.x, row.normal.y, row.normal.z])?,
                    tangent,
                },
                index,
            ))
        });
        if count > limit {
            return None;
        }
        let mut entries: Vec<_> = rows.into_iter().collect::<Option<_>>()?;
        entries.sort_unstable_by_key(|(vertex, delta, _)| (*vertex, delta.target));
        if entries
            .windows(2)
            .any(|pair| pair[0].0 == pair[1].0 && pair[0].1.target == pair[1].1.target)
        {
            return None;
        }
        let mut table = MorphTable {
            offsets: vec![0; vertices.len() + 1],
            deltas: Vec::with_capacity(count),
            attributes: if extra.is_empty() {
                Vec::new()
            } else {
                Vec::with_capacity(count)
            },
        };
        for (vertex, delta, index) in entries {
            table.offsets[vertex + 1] += 1;
            table.deltas.push(delta);
            if !extra.is_empty() {
                table
                    .attributes
                    .push(index.map_or_else(MorphAttributes::default, |index| extra[index]));
            }
        }
        for vertex in 0..vertices.len() {
            table.offsets[vertex + 1] += table.offsets[vertex];
        }
        Some(Self {
            names: names.into(),
            table,
        })
    }

    /// Expand source indices in the same order as the prepared triangles.
    pub(super) fn expand(&self, indices: &[u32]) -> Option<MorphTable> {
        let mut table = MorphTable {
            offsets: Vec::with_capacity(indices.len() + 1),
            ..Default::default()
        };
        table.offsets.push(0);
        let mut count = 0;
        for &vertex in indices {
            count += self.for_vertex(vertex).len();
            if count > MAX_MORPH_DELTAS {
                return None;
            }
            table.offsets.push(count as u32);
        }
        table.deltas.reserve_exact(count);
        if !self.table.attributes.is_empty() {
            table.attributes.reserve_exact(count);
        }
        for &vertex in indices {
            let range = self.for_vertex(vertex);
            table
                .deltas
                .extend_from_slice(&self.table.deltas[range.clone()]);
            if !self.table.attributes.is_empty() {
                table
                    .attributes
                    .extend_from_slice(&self.table.attributes[range]);
            }
        }
        Some(table)
    }

    fn for_vertex(&self, vertex: u32) -> std::ops::Range<usize> {
        let vertex = vertex as usize;
        self.table.offsets[vertex] as usize..self.table.offsets[vertex + 1] as usize
    }

    pub(super) fn delta_count(&self) -> usize {
        self.table.deltas.len()
    }
}

fn finite<const N: usize>(values: [f64; N]) -> Option<[f32; N]> {
    let packed = values.map(|value| value as f32);
    packed.into_iter().all(f32::is_finite).then_some(packed)
}
