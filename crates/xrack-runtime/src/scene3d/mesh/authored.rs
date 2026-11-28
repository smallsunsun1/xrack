//! Turn application vertex and index models into shared meshes.
mod morph;
mod topology;

use morph::AuthoredMorph;
use topology::{Expansion, Mode};

use super::{
    morph::MAX_MORPH_DELTAS,
    parametric::SceneParametricGeometry,
    text::{GeometryStatus, PreparedText, TextRequest},
};
pub use morph::SceneMorphDelta;
pub(in crate::scene3d) use morph::record_morph_delta;

use std::{
    cell::RefCell,
    collections::{BTreeMap, VecDeque},
    ops::Range,
    rc::Rc,
    sync::{Mutex, OnceLock},
};

use nalgebra::Vector3;

use crate::{
    Color, Point3D, RecordValue, Value, VecModel, Vector3D,
    scene3d::lighting::linear_color,
    scene3d::{
        MAX_SKIN_JOINTS, Mesh3D, MeshInfluence, MeshTangent, MeshTopology, MeshVertex,
        SKIN_INFLUENCES, SceneGeometryInput,
    },
};

const MAX_VERTICES: usize = 60_000;
const MAX_INDICES: usize = 180_000;
const MAX_SUBSETS: usize = 256;
const CACHED_LAYOUTS: usize = 4;
const CACHE_ENTRIES: usize = 12;
const CACHE_VERTICES: usize = 300_000;
// Bound both source and expanded copies while admitting a full-size morph mesh.
const CACHE_MORPH_DELTAS: usize = 2 * MAX_MORPH_DELTAS;

static CACHE: OnceLock<Mutex<MeshCache>> = OnceLock::new();

type Stamp = (u64, u64);
type DeformationStamps = (Option<Stamp>, Option<Stamp>);
type GeometryKey = (Stamp, Option<Stamp>, DeformationStamps);
type Read = Box<dyn Fn() -> bool>;

/// Tracks the nested model inputs to one 3D projection without retaining model handles.
#[doc(hidden)]
#[derive(Default)]
pub struct SceneMeshInputs {
    reads: RefCell<BTreeMap<Stamp, Read>>,
    text: RefCell<Vec<Rc<TextRequest>>>,
}

impl SceneMeshInputs {
    pub(crate) fn current(&self) -> bool {
        self.reads.borrow().values().all(|read| read())
            && self
                .text
                .borrow()
                .iter()
                .all(|job| job.read().status == GeometryStatus::Loading)
    }

    pub(super) fn observe_text(&self, job: Rc<TextRequest>) -> PreparedText {
        let result = job.read();
        if result.status == GeometryStatus::Loading {
            let mut jobs = self.text.borrow_mut();
            if !jobs.iter().any(|old| Rc::ptr_eq(old, &job)) {
                jobs.push(job);
            }
        }
        result
    }

    pub fn parametric_geometry<'a>(
        &self,
        geometry: SceneParametricGeometry<'a>,
        renderables: bool,
    ) -> SceneGeometryInput<'a> {
        if geometry.shape == "text" && renderables {
            SceneGeometryInput::Text(geometry.prepare_text(self))
        } else {
            SceneGeometryInput::Parametric(geometry)
        }
    }

    pub(crate) fn remember<T: 'static>(&self, model: &VecModel<T>) -> Stamp {
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
        draw: (&str, Option<Vec<SceneMeshSubset>>),
        vertex: impl FnMut(&T) -> Option<SceneMeshVertex>,
    ) -> Option<SceneGeometryInput<'static>> {
        self.geometry_cached(vertices, indices, draw, vertex, (None, None), |_| {
            Some(Deformations::default())
        })
    }

    pub fn deformed_geometry<T: 'static, W: 'static, M: 'static>(
        &self,
        vertices: &VecModel<T>,
        indices: SceneMeshIndices<'_>,
        draw: (&str, Option<Vec<SceneMeshSubset>>),
        vertex: impl FnMut(&T) -> Option<SceneMeshVertex>,
        (weights, weight): (
            Option<&VecModel<W>>,
            impl FnMut(&W) -> Option<SceneSkinWeight>,
        ),
        (deltas, delta): (
            Option<&VecModel<M>>,
            impl for<'a> FnMut(&'a M) -> Option<SceneMorphDelta<'a>>,
        ),
    ) -> Option<SceneGeometryInput<'static>> {
        let skin_stamp = weights
            .map(|rows| self.remember(rows))
            .filter(|_| weights.is_some_and(|rows| !rows.is_empty()));
        let morph_stamp = deltas
            .map(|rows| self.remember(rows))
            .filter(|_| deltas.is_some_and(|rows| !rows.is_empty()));
        self.geometry_cached(
            vertices,
            indices,
            draw,
            vertex,
            (skin_stamp, morph_stamp),
            |vertices| {
                let skin = match weights.filter(|_| skin_stamp.is_some()) {
                    Some(rows) => Some(prepare_weights(vertices.len(), rows, weight)?),
                    None => None,
                };
                let morph = match deltas.filter(|_| morph_stamp.is_some()) {
                    Some(rows) => Some(AuthoredMorph::prepare(vertices, rows, delta)?),
                    None => None,
                };
                Some(Deformations { skin, morph })
            },
        )
    }

    fn geometry_cached<T: 'static>(
        &self,
        vertices: &VecModel<T>,
        indices: SceneMeshIndices<'_>,
        draw: (&str, Option<Vec<SceneMeshSubset>>),
        vertex: impl FnMut(&T) -> Option<SceneMeshVertex>,
        stamps: DeformationStamps,
        deformations: impl FnOnce(&[Vertex]) -> Option<Deformations>,
    ) -> Option<SceneGeometryInput<'static>> {
        let vertices_stamp = self.remember(vertices);
        let key = match &indices {
            SceneMeshIndices::Sequential => Some((vertices_stamp, None, stamps)),
            SceneMeshIndices::Model(indices) => {
                Some((vertices_stamp, Some(self.remember(indices)), stamps))
            }
            SceneMeshIndices::Slice(_) => None,
        };
        let (mode, subsets) = draw;
        let Some(subsets) = subsets else {
            return Some(SceneGeometryInput::Authored(None));
        };
        let index_count = indices.len();
        let vertex_count = vertices.len();
        if stamps == (None, None) && vertex_count == 0 && index_count == 0 && subsets.is_empty() {
            return None;
        }
        let source_count = if index_count == 0 {
            vertex_count
        } else {
            index_count
        };
        let mesh = Mode::named(mode)
            .and_then(|mode| Expansion::prepare(mode, subsets, source_count))
            .and_then(|(expansion, layout)| {
                Self::mesh(vertices, indices, vertex, key, expansion, deformations)
                    .and_then(|base| with_layout(base, layout))
            });
        Some(SceneGeometryInput::Authored(mesh))
    }

    fn mesh<T>(
        vertices: &VecModel<T>,
        indices: SceneMeshIndices<'_>,
        vertex: impl FnMut(&T) -> Option<SceneMeshVertex>,
        key: Option<GeometryKey>,
        expansion: Expansion,
        deformations: impl FnOnce(&[Vertex]) -> Option<Deformations>,
    ) -> Option<Mesh3D> {
        let cache = CACHE.get_or_init(Mutex::default);

        if let Some(mesh) = key.and_then(|key| {
            cache
                .lock()
                .ok()?
                .get(|entry| entry.key == Some(key) && entry.expansion == expansion)
        }) {
            return Some(mesh);
        }
        let (count, rows) = vertices.inspect_rows(0, MAX_VERTICES, vertex);
        if !(1..=MAX_VERTICES).contains(&count) {
            return None;
        }
        let vertices: Vec<_> = rows
            .into_iter()
            .map(|row| row.and_then(SceneMeshVertex::prepare))
            .collect::<Option<_>>()?;
        let indices = expansion.indices(indices.prepare(count)?)?;
        let deformations = deformations(&vertices)?;
        let matches = |entry: &CacheEntry| {
            entry.expansion == expansion
                && entry.vertices == vertices
                && entry.indices == indices
                && entry.deformations == deformations
        };
        let mut cache_guard = cache.lock().ok();
        if let Some(mesh) = cache_guard.as_mut().and_then(|cache| cache.get(matches)) {
            if let Some(cache) = &mut cache_guard {
                if let Some(entry) = cache.entries.front_mut() {
                    entry.key = key;
                }
            }
            return Some(mesh);
        }
        drop(cache_guard);

        let tangents = if vertices.iter().any(|vertex| vertex.tangent.is_some()) {
            indices
                .iter()
                .map(|&index| vertices[index as usize].tangent.unwrap_or([0; 4]))
                .collect()
        } else {
            Vec::new()
        };
        let topology = expansion.mode.topology();
        let prepared = if topology == MeshTopology::Triangles {
            triangles(&vertices, &indices)?
        } else {
            indices
                .iter()
                .map(|&index| vertices[index as usize].with_normal([0.0, 0.0, 1.0]))
                .collect()
        };
        let mesh = Mesh3D::prepare_authored_mesh(topology, prepared, tangents)?;
        let mesh = if let Some(skin) = &deformations.skin {
            mesh.with_skin_weights(indices.iter().map(|&index| skin[index as usize]).collect())?
        } else {
            mesh
        };
        let mesh = if let Some(morph) = &deformations.morph {
            mesh.with_morph_table(
                morph.names.len(),
                morph.names.clone(),
                morph.expand(&indices)?,
            )?
        } else {
            mesh
        };
        if let Ok(mut cache) = cache.lock() {
            if let Some(existing) = cache.get(matches) {
                return Some(existing);
            }
            cache.insert(CacheEntry {
                key,
                expansion,
                vertices,
                indices,
                deformations,
                mesh: mesh.clone(),
                layouts: VecDeque::new(),
            });
        }
        Some(mesh)
    }
}

/// One joint contribution to a source mesh vertex.
#[doc(hidden)]
#[derive(Clone, Copy, Debug)]
pub struct SceneSkinWeight {
    pub vertex: i64,
    pub joint: i64,
    pub weight: f64,
}

impl SceneSkinWeight {
    pub(crate) fn from_record(row: &RecordValue) -> Option<Self> {
        let (Some(Value::Int(vertex)), Some(Value::Int(joint)), Some(Value::Float(weight))) =
            (row.field("vertex"), row.field("joint"), row.field("weight"))
        else {
            return None;
        };
        Some(Self {
            vertex: *vertex,
            joint: *joint,
            weight: *weight,
        })
    }
}

fn prepare_weights<W>(
    vertices: usize,
    model: &VecModel<W>,
    read: impl FnMut(&W) -> Option<SceneSkinWeight>,
) -> Option<Vec<MeshInfluence>> {
    let limit = vertices * SKIN_INFLUENCES;
    let (count, rows) = model.inspect_rows(0, limit, read);
    if count > limit {
        return None;
    }
    let mut weights = vec![
        MeshInfluence {
            joints: [0; SKIN_INFLUENCES],
            weights: [0.0; SKIN_INFLUENCES],
        };
        vertices
    ];
    for row in rows {
        let row = row?;
        let entry = weights.get_mut(usize::try_from(row.vertex).ok()?)?;
        let joint = u16::try_from(row.joint).ok()?;
        let weight = row.weight as f32;
        if usize::from(joint) >= MAX_SKIN_JOINTS || !weight.is_finite() || row.weight < 0.0 {
            return None;
        }
        if weight == 0.0 {
            continue;
        }
        let slot = entry.weights.iter().position(|&weight| weight == 0.0)?;
        if entry.joints[..slot].contains(&joint) {
            return None;
        }
        entry.joints[slot] = joint;
        entry.weights[slot] = weight;
    }
    weights
        .into_iter()
        .map(|entry| MeshInfluence::normalized(entry.joints, entry.weights))
        .collect()
}

/// A material assignment to a contiguous range of authored elements.
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
        if count == 0 || end > limit || self.name.is_empty() || self.name.len() > 256 {
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

fn with_layout(base: Mesh3D, layout: Vec<SceneMeshPart>) -> Option<Mesh3D> {
    if layout.is_empty() {
        return Some(base);
    }
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
    fn len(&self) -> usize {
        match self {
            Self::Sequential => 0,
            Self::Model(indices) => indices.len(),
            Self::Slice(indices) => indices.len(),
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
            return Some((0..vertices as u32).collect());
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
    pub tangent: Option<Vector3D>,
    pub tangent_sign: Option<f64>,
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
    tangent: Option<MeshTangent>,
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
        let normal = match self.normal {
            Some(normal) => prepared_direction(normal)?,
            None => None,
        };
        let tangent = match (self.tangent, self.tangent_sign) {
            (Some(tangent), sign) => {
                let sign = sign.unwrap_or(1.0);
                if sign.abs() != 1.0 {
                    return None;
                }
                let [x, y, z] = prepared_direction(tangent)??;
                Some([x, y, z, sign as f32].map(|value| (value * 32767.0).round() as i16))
            }
            (None, Some(_)) => return None,
            (None, None) => None,
        };
        let color = self.color.map_or([1.0; 4], |color| {
            let [r, g, b] = linear_color(color);
            [r, g, b, f32::from(color.0 as u8) / 255.0]
        });
        Some(Vertex {
            position: position.map(|v| v as f32),
            normal,
            tangent,
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
        let vector = |name| match row.field(name) {
            Some(Value::Vector3D(value)) => Some(*value),
            _ => None,
        };
        Some(Self {
            position: *position,
            normal: vector("normal"),
            tangent: vector("tangent"),
            tangent_sign: number("tangent-sign"),
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

fn prepared_direction(direction: Vector3D) -> Option<Option<[f32; 3]>> {
    let values = [direction.x, direction.y, direction.z];
    if !values.into_iter().all(f64::is_finite) {
        return None;
    }
    let scale = values.into_iter().map(f64::abs).fold(0.0, f64::max);
    Some((scale > 0.0).then(|| {
        Vector3::from(values.map(|value| (value / scale) as f32))
            .normalize()
            .into()
    }))
}

impl Vertex {
    fn with_normal(self, fallback: [f32; 3]) -> MeshVertex {
        MeshVertex {
            position: self.position,
            normal: self.normal.unwrap_or(fallback),
            uv: self.uv,
            uv1: self.uv1,
            color: self.color,
        }
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
                ids.map(|index| vertices[index].with_normal(normals[index].unwrap_or(face).into()))
            })
            .collect(),
    )
}

#[derive(Default, PartialEq)]
struct Deformations {
    skin: Option<Vec<MeshInfluence>>,
    morph: Option<AuthoredMorph>,
}

struct CacheEntry {
    key: Option<GeometryKey>,
    expansion: Expansion,
    vertices: Vec<Vertex>,
    indices: Vec<u32>,
    deformations: Deformations,
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

    fn vertex_cost(&self) -> usize {
        self.vertices.len()
            + self.indices.len()
            + self.deformations.skin.as_ref().map_or(0, Vec::len)
    }

    fn morph_cost(&self) -> usize {
        self.deformations
            .morph
            .as_ref()
            .map_or(0, AuthoredMorph::delta_count)
            + self
                .mesh
                .morph_geometry()
                .map_or(0, |geometry| geometry.delta_count())
    }
}

#[derive(Default)]
struct MeshCache {
    entries: VecDeque<CacheEntry>,
    vertices: usize,
    morph_deltas: usize,
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
        let vertices = entry.vertex_cost();
        let morph_deltas = entry.morph_cost();
        while self.entries.len() >= CACHE_ENTRIES
            || self.vertices + vertices > CACHE_VERTICES
            || self.morph_deltas + morph_deltas > CACHE_MORPH_DELTAS
        {
            let Some(removed) = self.entries.pop_back() else {
                return;
            };
            self.vertices -= removed.vertex_cost();
            self.morph_deltas -= removed.morph_cost();
        }
        self.vertices += vertices;
        self.morph_deltas += morph_deltas;
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
        let (_, layout) = Expansion::prepare(
            Mode::Triangles,
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
            assert!(Expansion::prepare(Mode::Triangles, subsets, 12).is_none());
        }
    }
}
