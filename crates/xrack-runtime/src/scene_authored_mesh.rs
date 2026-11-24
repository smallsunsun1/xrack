//! Turn application vertex and index models into shared triangle meshes.
use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    ops::Range,
    sync::{Mutex, OnceLock},
};

use nalgebra::Vector3;

use crate::{
    Color, Point3D, RecordValue, Value, VecModel, Vector3D,
    scene_lighting::linear_color,
    scene3d::{Mesh3D, MeshVertex, SceneGeometryInput},
};

const MAX_VERTICES: usize = 60_000;
const MAX_INDICES: usize = 180_000;
const MAX_SUBSETS: usize = 256;
const CACHED_LAYOUTS: usize = 4;
const CACHE_ENTRIES: usize = 12;
const CACHE_VERTICES: usize = 300_000;

static CACHE: OnceLock<Mutex<MeshCache>> = OnceLock::new();

type Stamp = (u64, u64);
type Read = Box<dyn Fn() -> bool>;

/// Tracks the nested model inputs to one 3D projection without retaining model handles.
#[doc(hidden)]
#[derive(Default)]
pub struct SceneMeshInputs {
    reads: RefCell<BTreeMap<Stamp, Read>>,
}

impl SceneMeshInputs {
    pub(crate) fn current(&self) -> bool {
        self.reads.borrow().values().all(|read| read())
    }

    fn remember<T: 'static>(&self, model: &VecModel<T>) -> Stamp {
        let stamp = model.contents_stamp();
        self.reads
            .borrow_mut()
            .entry(stamp)
            .or_insert_with(|| model.remember_revision(stamp.1));
        stamp
    }

    pub fn subsets<T: 'static>(
        &self,
        model: &VecModel<T>,
        convert: impl FnMut(&T) -> Option<SceneMeshSubset>,
    ) -> Option<Vec<SceneMeshSubset>> {
        self.remember(model);
        let (count, rows) = model.inspect_rows(0, MAX_SUBSETS, convert);
        if count > MAX_SUBSETS {
            return None;
        }
        rows.into_iter().collect()
    }

    pub fn geometry<T: 'static>(
        &self,
        vertices: &VecModel<T>,
        indices: SceneMeshIndices<'_>,
        subsets: Option<Vec<SceneMeshSubset>>,
        vertex: impl FnMut(&T) -> Option<SceneMeshVertex>,
    ) -> Option<SceneGeometryInput<'static>> {
        let vertices_stamp = self.remember(vertices);
        let key = match &indices {
            SceneMeshIndices::Sequential => Some((vertices_stamp, None)),
            SceneMeshIndices::Model(indices) => {
                Some((vertices_stamp, Some(self.remember(indices))))
            }
            SceneMeshIndices::Slice(_) => None,
        };
        let Some(subsets) = subsets else {
            return Some(SceneGeometryInput::Triangles(None));
        };
        if vertices.is_empty() && indices.is_empty() && subsets.is_empty() {
            return None;
        }
        let mesh =
            Self::mesh(vertices, indices, vertex, key).and_then(|base| with_subsets(base, subsets));
        Some(SceneGeometryInput::Triangles(mesh))
    }

    fn mesh<T>(
        vertices: &VecModel<T>,
        indices: SceneMeshIndices<'_>,
        vertex: impl FnMut(&T) -> Option<SceneMeshVertex>,
        key: Option<(Stamp, Option<Stamp>)>,
    ) -> Option<Mesh3D> {
        let cache = CACHE.get_or_init(Mutex::default);

        if let Some(mesh) =
            key.and_then(|key| cache.lock().ok()?.get(|entry| entry.key == Some(key)))
        {
            return Some(mesh);
        }
        let (count, rows) = vertices.inspect_rows(0, MAX_VERTICES, vertex);
        if !(3..=MAX_VERTICES).contains(&count) {
            return None;
        }
        let vertices: Vec<_> = rows
            .into_iter()
            .map(|row| row.and_then(SceneMeshVertex::prepare))
            .collect::<Option<_>>()?;
        let indices = indices.prepare(count)?;
        let mut cache_guard = cache.lock().ok();
        if let Some(mesh) = cache_guard.as_mut().and_then(|cache| {
            cache.get(|entry| entry.vertices == vertices && entry.indices == indices)
        }) {
            if let Some(cache) = &mut cache_guard {
                if let Some(entry) = cache.entries.front_mut() {
                    entry.key = key;
                }
            }
            return Some(mesh);
        }
        drop(cache_guard);

        let mesh = Mesh3D::prepare_procedural(vec![("surface", triangles(&vertices, &indices)?)])?;
        if let Ok(mut cache) = cache.lock() {
            if let Some(existing) =
                cache.get(|entry| entry.vertices == vertices && entry.indices == indices)
            {
                return Some(existing);
            }
            cache.insert(CacheEntry {
                key,
                vertices,
                indices,
                mesh: mesh.clone(),
                layouts: VecDeque::new(),
            });
        }
        Some(mesh)
    }
}

/// A material assignment to a contiguous range of triangle elements.
#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct SceneMeshSubset {
    pub offset: i64,
    pub count: i64,
    pub name: String,
}

impl SceneMeshSubset {
    pub(crate) fn from_record(row: &RecordValue) -> Option<Self> {
        let [Value::Int(offset), Value::Int(count), Value::String(name)] = [
            row.field("offset")?,
            row.field("count")?,
            row.field("name")?,
        ] else {
            return None;
        };
        Some(Self {
            offset: *offset,
            count: *count,
            name: name.to_string(),
        })
    }

    fn prepare(self, limit: usize) -> Option<SceneMeshPart> {
        let start = usize::try_from(self.offset).ok()?;
        let count = usize::try_from(self.count).ok()?;
        let end = start.checked_add(count)?;
        if count == 0
            || end > limit
            || !start.is_multiple_of(3)
            || !count.is_multiple_of(3)
            || self.name.is_empty()
            || self.name.len() > 256
        {
            return None;
        }
        Some(SceneMeshPart {
            name: self.name,
            range: start..end,
        })
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub(crate) struct SceneMeshPart {
    pub name: String,
    pub range: Range<usize>,
}

fn mesh_layout(subsets: Vec<SceneMeshSubset>, limit: usize) -> Option<Vec<SceneMeshPart>> {
    let mut subsets: Vec<_> = subsets
        .into_iter()
        .map(|subset| subset.prepare(limit))
        .collect::<Option<_>>()?;
    subsets.sort_unstable_by_key(|part| part.range.start);
    let mut layout = Vec::<SceneMeshPart>::with_capacity(subsets.len());
    let mut add = |part: SceneMeshPart| {
        if let Some(previous) = layout.last_mut()
            && previous.name == part.name
            && previous.range.end == part.range.start
        {
            previous.range.end = part.range.end;
        } else {
            layout.push(part);
        }
    };
    let mut start = 0;
    for part in subsets {
        if part.range.start < start {
            return None;
        }
        if start < part.range.start {
            add(SceneMeshPart {
                name: "surface".into(),
                range: start..part.range.start,
            });
        }
        start = part.range.end;
        add(part);
    }
    if start < limit {
        add(SceneMeshPart {
            name: "surface".into(),
            range: start..limit,
        });
    }
    Some(layout)
}

fn with_subsets(base: Mesh3D, subsets: Vec<SceneMeshSubset>) -> Option<Mesh3D> {
    if subsets.is_empty() {
        return Some(base);
    }
    let layout = mesh_layout(subsets, base.vertices().len())?;
    if layout.len() == 1 && layout[0].name == "surface" {
        return Some(base);
    }
    let cache = CACHE.get_or_init(Mutex::default);
    if let Some(mesh) = cache.lock().ok().and_then(|mut cache| {
        cache
            .layout_entry(&base)
            .and_then(|entry| entry.layout(&layout))
    }) {
        return Some(mesh);
    }

    let mesh = base.with_procedural_layout(&layout)?;
    if let Ok(mut cache) = cache.lock()
        && let Some(entry) = cache.layout_entry(&base)
    {
        if let Some(existing) = entry.layout(&layout) {
            return Some(existing);
        }
        entry.layouts.truncate(CACHED_LAYOUTS - 1);
        entry.layouts.push_front((layout, mesh.clone()));
    }
    Some(mesh)
}

#[doc(hidden)]
pub enum SceneMeshIndices<'a> {
    Sequential,
    Model(&'a VecModel<i64>),
    Slice(&'a [i64]),
}

impl SceneMeshIndices<'_> {
    fn is_empty(&self) -> bool {
        match self {
            Self::Sequential => true,
            Self::Model(indices) => indices.is_empty(),
            Self::Slice(indices) => indices.is_empty(),
        }
    }

    fn prepare(self, vertices: usize) -> Option<Vec<u32>> {
        let values = match self {
            Self::Sequential => Vec::new(),
            Self::Slice(indices) => {
                if indices.len() > MAX_INDICES {
                    return None;
                }
                indices.to_vec()
            }
            Self::Model(indices) => {
                let (count, values) = indices.inspect_rows(0, MAX_INDICES, |index| *index);
                if count > MAX_INDICES {
                    return None;
                }
                values
            }
        };
        if values.is_empty() {
            return vertices
                .is_multiple_of(3)
                .then(|| (0..vertices as u32).collect());
        }
        if !values.len().is_multiple_of(3) {
            return None;
        }
        values
            .into_iter()
            .map(|index| {
                u32::try_from(index)
                    .ok()
                    .filter(|index| (*index as usize) < vertices)
            })
            .collect()
    }
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default)]
pub struct SceneMeshVertex {
    pub position: Point3D,
    pub normal: Option<Vector3D>,
    pub u: f64,
    pub v: f64,
    pub u1: Option<f64>,
    pub v1: Option<f64>,
    pub color: Option<Color>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct Vertex {
    position: [f32; 3],
    normal: Option<[f32; 3]>,
    uv: [f32; 2],
    uv1: [f32; 2],
    color: [f32; 4],
}

impl SceneMeshVertex {
    fn prepare(self) -> Option<Vertex> {
        let position = [self.position.x, self.position.y, self.position.z];
        let uv = [self.u, self.v];
        let uv1 = [self.u1.unwrap_or(self.u), self.v1.unwrap_or(self.v)];
        if !position
            .into_iter()
            .chain(uv)
            .chain(uv1)
            .all(|value| value.abs() <= 10_000.0)
        {
            return None;
        }
        let normal = if let Some(normal) = self.normal {
            let values = [normal.x, normal.y, normal.z];
            if !values.into_iter().all(f64::is_finite) {
                return None;
            }
            let scale = values.into_iter().map(f64::abs).fold(0.0, f64::max);
            (scale > 0.0).then(|| {
                Vector3::from(values.map(|value| (value / scale) as f32))
                    .normalize()
                    .into()
            })
        } else {
            None
        };
        let color = self.color.map_or([1.0; 4], |color| {
            let [r, g, b] = linear_color(color);
            [r, g, b, f32::from(color.0 as u8) / 255.0]
        });
        Some(Vertex {
            position: position.map(|v| v as f32),
            normal,
            uv: uv.map(|v| v as f32),
            uv1: uv1.map(|v| v as f32),
            color,
        })
    }

    pub(crate) fn from_record(row: &RecordValue) -> Option<Self> {
        let Value::Point3D(position) = row.field("position")? else {
            return None;
        };
        let number = |name| match row.field(name) {
            Some(Value::Float(value)) => Some(*value),
            _ => None,
        };
        Some(Self {
            position: *position,
            normal: match row.field("normal") {
                Some(Value::Vector3D(normal)) => Some(*normal),
                _ => None,
            },
            u: number("u").unwrap_or(0.0),
            v: number("v").unwrap_or(0.0),
            u1: number("u1"),
            v1: number("v1"),
            color: match row.field("color") {
                Some(Value::Color(color)) => Some(*color),
                _ => None,
            },
        })
    }
}

fn triangles(vertices: &[Vertex], indices: &[u32]) -> Option<Vec<MeshVertex>> {
    let mut normals = vec![Vector3::zeros(); vertices.len()];
    let mut faces = Vec::with_capacity(indices.len() / 3);
    for &face in indices.as_chunks::<3>().0 {
        let ids = face.map(|index| index as usize);
        let [a, b, c] = ids.map(|index| Vector3::from(vertices[index].position));
        let normal = (b - a).cross(&(c - a));
        let unit = normal.try_normalize(1e-12)?;
        for id in ids {
            normals[id] += normal;
        }
        faces.push((ids, unit));
    }
    let normals: Vec<_> = normals
        .into_iter()
        .map(|normal| normal.try_normalize(1e-12))
        .collect();
    Some(
        faces
            .into_iter()
            .flat_map(|(ids, face)| {
                ids.map(|index| {
                    let v = vertices[index];
                    MeshVertex {
                        position: v.position,
                        normal: v
                            .normal
                            .unwrap_or_else(|| normals[index].unwrap_or(face).into()),
                        uv: v.uv,
                        uv1: v.uv1,
                        color: v.color,
                    }
                })
            })
            .collect(),
    )
}

struct CacheEntry {
    key: Option<(Stamp, Option<Stamp>)>,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    mesh: Mesh3D,
    layouts: VecDeque<(Vec<SceneMeshPart>, Mesh3D)>,
}

impl CacheEntry {
    fn layout(&mut self, layout: &[SceneMeshPart]) -> Option<Mesh3D> {
        let index = self.layouts.iter().position(|(key, _)| key == layout)?;
        let entry = self.layouts.remove(index)?;
        let mesh = entry.1.clone();
        self.layouts.push_front(entry);
        Some(mesh)
    }

    fn cost(&self) -> usize {
        self.vertices.len() + self.indices.len()
    }
}

#[derive(Default)]
struct MeshCache {
    entries: VecDeque<CacheEntry>,
    cost: usize,
}

impl MeshCache {
    fn layout_entry(&mut self, base: &Mesh3D) -> Option<&mut CacheEntry> {
        self.entries.iter_mut().find(|entry| entry.mesh == *base)
    }

    fn get(&mut self, matches: impl Fn(&CacheEntry) -> bool) -> Option<Mesh3D> {
        let index = self.entries.iter().position(matches)?;
        let entry = self.entries.remove(index)?;
        let mesh = entry.mesh.clone();
        self.entries.push_front(entry);
        Some(mesh)
    }

    fn insert(&mut self, entry: CacheEntry) {
        let cost = entry.cost();
        while self.entries.len() >= CACHE_ENTRIES || self.cost + cost > CACHE_VERTICES {
            let Some(removed) = self.entries.pop_back() else {
                return;
            };
            self.cost -= removed.cost();
        }
        self.cost += cost;
        self.entries.push_front(entry);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn subset(name: &str, offset: i64, count: i64) -> SceneMeshSubset {
        SceneMeshSubset {
            name: name.into(),
            offset,
            count,
        }
    }

    #[test]
    fn layouts_merge_adjacent_ranges_and_cover_gaps() {
        let layout = mesh_layout(
            vec![
                subset("badge", 9, 3),
                subset("badge", 3, 6),
                subset("detail", 15, 3),
            ],
            21,
        )
        .unwrap();
        let ranges: Vec<_> = layout
            .iter()
            .map(|part| (part.name.as_str(), part.range.clone()))
            .collect();
        assert_eq!(
            ranges,
            [
                ("surface", 0..3),
                ("badge", 3..12),
                ("surface", 12..15),
                ("detail", 15..18),
                ("surface", 18..21),
            ]
        );
    }

    #[test]
    fn layouts_reject_partial_triangles_overlap_and_out_of_range_offsets() {
        for subsets in [
            vec![subset("detail", 1, 3)],
            vec![subset("detail", 0, 1)],
            vec![subset("detail", 9, 6)],
            vec![subset("detail", -3, 3)],
            vec![subset("detail", 0, 6), subset("badge", 3, 3)],
            vec![subset("", 0, 3)],
        ] {
            assert!(mesh_layout(subsets, 12).is_none());
        }
    }
}
