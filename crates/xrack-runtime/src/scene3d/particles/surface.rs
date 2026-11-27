//! Area-weighted particle sampling from authored model surfaces.
use std::{
    cell::RefCell,
    collections::VecDeque,
    sync::{Arc, Mutex},
};

use nalgebra::Vector3;

use crate::{
    Point3D, Value, VecModel, Vector3D,
    scene3d::hierarchy::rest_mesh_objects,
    scene3d::{
        MAX_IMPORTED_VERTICES, MeshAsset, ParticlePointCloud, SceneModel, random_instances::mix,
    },
};

const MAX_TRIANGLES: usize = MAX_IMPORTED_VERTICES / 3;
const SURFACE_RECORD_BYTES: usize = 64;
const CACHE_BYTES: usize = 40 * 1024 * 1024;
const CACHE_ENTRIES: usize = 8;

/// GPU-ready surface positions, normals and alias table. Geometry stays
/// emitter-local so translating or rotating the emitter reuses the table.
#[doc(hidden)]
#[derive(Debug)]
pub struct ParticleMeshSurface {
    bytes: Vec<u8>,
    pub(crate) radius: f64,
    points: Option<(u32, [f32; 3])>,
}

impl PartialEq for ParticleMeshSurface {
    fn eq(&self, other: &Self) -> bool {
        std::ptr::eq(self, other)
    }
}

impl ParticleMeshSurface {
    pub fn bytes(&self) -> &[u8] {
        &self.bytes
    }

    pub fn record_count(&self) -> u32 {
        (self.bytes.len() / SURFACE_RECORD_BYTES) as u32
    }
    pub fn sample_count(&self) -> u32 {
        self.points
            .map_or_else(|| self.record_count(), |points| points.0)
    }

    pub(crate) fn point_uniform(&self) -> Option<[f32; 4]> {
        self.points
            .map(|(count, [x, y, z])| [x, y, z, count as f32])
    }
}

#[derive(Default)]
pub(crate) struct ParticleSurfaceCache {
    entries: RefCell<VecDeque<Entry>>,
}

#[derive(Clone, Copy, PartialEq)]
enum SurfaceKind {
    Mesh,
    Points { shuffle: Option<u32> },
}

struct Entry {
    kind: SurfaceKind,
    key: String,
    scale: [f32; 3],
    surface: Option<Arc<ParticleMeshSurface>>,
}

impl ParticleSurfaceCache {
    pub const fn new() -> Self {
        Self {
            entries: RefCell::new(VecDeque::new()),
        }
    }

    pub fn surface(
        &self,
        key: &str,
        scale: [f32; 3],
        imported: impl FnOnce(&str) -> Option<MeshAsset>,
    ) -> Option<Arc<ParticleMeshSurface>> {
        self.cached(key, scale, SurfaceKind::Mesh, || {
            MeshAsset::resolve(key, imported).and_then(|asset| build_surface(&asset, scale))
        })
    }

    pub fn points(
        &self,
        key: &str,
        scale: [f32; 3],
        shuffle: Option<u32>,
        imported: impl FnOnce(&str) -> Option<ParticlePointCloud>,
    ) -> Option<Arc<ParticleMeshSurface>> {
        self.cached(key, scale, SurfaceKind::Points { shuffle }, || {
            build_point_table(&imported(key)?, scale, shuffle)
        })
    }

    fn cached(
        &self,
        key: &str,
        scale: [f32; 3],
        kind: SurfaceKind,
        build: impl FnOnce() -> Option<ParticleMeshSurface>,
    ) -> Option<Arc<ParticleMeshSurface>> {
        if key.is_empty() {
            return None;
        }
        {
            let mut entries = self.entries.borrow_mut();
            if let Some(index) = entries
                .iter()
                .position(|entry| entry.kind == kind && entry.key == key && entry.scale == scale)
                && let Some(entry) = entries.remove(index)
            {
                let result = entry.surface.clone();
                entries.push_front(entry);
                return result;
            }
        }
        let surface = build().map(Arc::new);
        let mut bytes = surface.as_ref().map_or(0, |surface| surface.bytes.len());
        let mut entries = self.entries.borrow_mut();
        entries.truncate(CACHE_ENTRIES - 1);
        entries.retain(|entry| {
            bytes += entry
                .surface
                .as_ref()
                .map_or(0, |surface| surface.bytes.len());
            bytes <= CACHE_BYTES
        });
        entries.push_front(Entry {
            kind,
            key: key.into(),
            scale,
            surface: surface.clone(),
        });
        surface
    }
}

/// Delayed projection. The scene is resolved after model bindings and
/// physics, so the emitter and the attractor see the same posed geometry.
#[derive(Clone, Debug, PartialEq)]
pub(crate) struct ParticleSceneShape {
    root: Arc<str>,
    excluded: Vec<String>,
    clip: Option<[[f32; 3]; 2]>,
    scale: [f32; 3],
}

impl ParticleSceneShape {
    pub(crate) fn record_exclusions(value: Option<&Value>) -> VecModel<String> {
        match value {
            Some(Value::StringList(names)) => names.clone().into(),
            Some(Value::StringModel(names)) => names.clone(),
            _ => VecModel::default(),
        }
    }

    pub(crate) fn new(
        root: &str,
        excluded: VecModel<String>,
        center: Point3D,
        extents: Vector3D,
        scale: Vector3D,
    ) -> Option<Arc<Self>> {
        let center = [center.x, center.y, center.z];
        let extents = [extents.x, extents.y, extents.z];
        let scale = [scale.x, scale.y, scale.z];
        if !center.into_iter().all(|v| (-1e8..=1e8).contains(&v))
            || !extents
                .into_iter()
                .chain(scale)
                .all(|v| (0.0..=1e8).contains(&v))
        {
            return None;
        }
        let clip = (extents != [0.0; 3]).then(|| {
            [
                std::array::from_fn(|axis| (center[axis] - extents[axis]) as f32),
                std::array::from_fn(|axis| (center[axis] + extents[axis]) as f32),
            ]
        });
        let mut excluded = excluded.into_snapshot();
        excluded.retain(|name| !name.is_empty());
        excluded.sort_unstable();
        excluded.dedup();
        Some(Arc::new(Self {
            root: root.into(),
            excluded,
            clip,
            scale: scale.map(|v| v as f32),
        }))
    }
}

#[derive(Clone, Debug)]
pub(crate) struct SceneSurface {
    pub mesh: Arc<ParticleMeshSurface>,
    pub placement: Option<[[f32; 4]; 3]>,
}

#[derive(Debug, Default)]
pub(crate) struct SceneSurfaceCache(
    Mutex<VecDeque<(Arc<ParticleSceneShape>, Option<SceneSurface>)>>,
);

impl SceneSurfaceCache {
    pub(crate) fn project(
        &self,
        scene: &SceneModel,
        shape: &Arc<ParticleSceneShape>,
    ) -> Option<SceneSurface> {
        let mut cache = self
            .0
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = cache.iter().position(|(key, _)| key == shape)
            && let Some(entry) = cache.remove(index)
        {
            let result = entry.1.clone();
            cache.push_front(entry);
            return result;
        }
        let result = scene_surface(scene, shape);
        let mut bytes = result.as_ref().map_or(0, |source| source.mesh.bytes.len());
        cache.truncate(CACHE_ENTRIES - 1);
        cache.retain(|(_, cached)| {
            bytes += cached.as_ref().map_or(0, |source| source.mesh.bytes.len());
            bytes <= CACHE_BYTES
        });
        cache.push_front((shape.clone(), result.clone()));
        result
    }
}

fn scene_surface(scene: &SceneModel, shape: &ParticleSceneShape) -> Option<SceneSurface> {
    let (scope, root) = if shape.root.is_empty() {
        (None, None)
    } else {
        let (scope, root) = scene.particle_surface_frame(&shape.root)?;
        (Some(scope), Some(root))
    };
    let excluded: Vec<_> = shape
        .excluded
        .iter()
        .filter_map(|node| scene.light_scope(node))
        .collect();
    let included = |index: u16| {
        scope.is_none_or(|scope| scope.contains(index))
            && !excluded.iter().any(|scope| scope.contains(index))
    };
    let posed = scene
        .objects()
        .iter()
        .filter(|object| included(object.lighting_index()))
        .flat_map(|object| object.posed_triangles())
        .map(|mut triangle| {
            if let Some(root) = root {
                if root.reflected {
                    triangle.swap(1, 2);
                }
                triangle = triangle
                    .map(|point| (root.inverse * (Vector3::from(point) - root.origin)).into());
            }
            triangle
        });
    let surface = if let Some(clip) = shape.clip {
        build_surface_table(
            posed.flat_map(|triangle| clip_triangle(triangle, clip)),
            shape.scale,
        )
    } else {
        build_surface_table(posed, shape.scale)
    }?;
    Some(SceneSurface {
        mesh: Arc::new(surface),
        placement: root.map(|root| root.rows()),
    })
}

/// Closing a convex triangle against six planes creates at most nine
/// corners. Emit the clipped triangles directly into the area table.
fn clip_triangle(
    triangle: [[f32; 3]; 3],
    [min, max]: [[f32; 3]; 2],
) -> impl Iterator<Item = [[f32; 3]; 3]> {
    let mut polygon = [[0.0; 3]; 9];
    let mut clipped = [[0.0; 3]; 9];
    polygon[..3].copy_from_slice(&triangle);
    let mut count = 3;
    for axis in 0..3 {
        for (limit, sign) in [(min[axis], 1.0), (max[axis], -1.0)] {
            if count < 3 {
                break;
            }
            let distance = |point: [f32; 3]| sign * (f64::from(point[axis]) - f64::from(limit));
            let mut written = 0;
            let mut previous = polygon[count - 1];
            let mut before = distance(previous);
            for &point in &polygon[..count] {
                let after = distance(point);
                if (before < 0.0 && after > 0.0) || (before > 0.0 && after < 0.0) {
                    let t = before / (before - after);
                    if let Some(slot) = clipped.get_mut(written) {
                        *slot = std::array::from_fn(|column| {
                            let a = f64::from(previous[column]);
                            let b = f64::from(point[column]);
                            (a + t * (b - a)) as f32
                        });
                        slot[axis] = limit;
                        written += 1;
                    }
                }
                if after >= 0.0
                    && let Some(slot) = clipped.get_mut(written)
                {
                    *slot = point;
                    written += 1;
                }
                previous = point;
                before = after;
            }
            count = written;
            std::mem::swap(&mut polygon, &mut clipped);
        }
    }
    let result: [[[f32; 3]; 3]; 7] =
        std::array::from_fn(|index| [polygon[0], polygon[index + 1], polygon[index + 2]]);
    result.into_iter().take(count.saturating_sub(2))
}

struct Triangle {
    points: [[f32; 3]; 3],
    normal: [f32; 3],
}

fn build_surface(asset: &MeshAsset, scale: [f32; 3]) -> Option<ParticleMeshSurface> {
    let objects = rest_mesh_objects(asset);
    build_surface_table(
        objects.iter().flat_map(|object| object.posed_triangles()),
        scale,
    )
}

fn build_surface_table(
    source: impl Iterator<Item = [[f32; 3]; 3]>,
    scale: [f32; 3],
) -> Option<ParticleMeshSurface> {
    let mut triangles = Vec::new();
    let mut weights = Vec::new();
    for points in source {
        let points: [[f32; 3]; 3] =
            points.map(|p| std::array::from_fn(|axis| p[axis] * scale[axis]));
        let [a, b, c] = points.map(|p| Vector3::from(p).cast::<f64>());
        let cross = (b - a).cross(&(c - a));
        let area = cross.norm();
        if area == 0.0 || !area.is_finite() {
            continue;
        }
        if triangles.len() == MAX_TRIANGLES {
            return None;
        }
        triangles.push(Triangle {
            points,
            normal: (cross / area).cast::<f32>().into(),
        });
        weights.push(area);
    }
    if triangles.is_empty() {
        return None;
    }
    let alias = alias_table(&mut weights);
    let mut bytes = Vec::with_capacity(triangles.len() * SURFACE_RECORD_BYTES);
    let mut radius_squared = 0.0_f64;
    for (triangle, (probability, alias)) in triangles.into_iter().zip(alias) {
        let [a, b, c] = triangle.points;
        for point in triangle.points {
            radius_squared = radius_squared.max(Vector3::from(point).cast::<f64>().norm_squared());
        }
        for value in a.into_iter().chain([probability]).chain(b) {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
        bytes.extend_from_slice(&alias.to_ne_bytes());
        for value in c
            .into_iter()
            .chain([0.0])
            .chain(triangle.normal)
            .chain([0.0])
        {
            bytes.extend_from_slice(&value.to_ne_bytes());
        }
    }
    Some(ParticleMeshSurface {
        bytes,
        radius: radius_squared.sqrt(),
        points: None,
    })
}

fn build_point_table(
    cloud: &ParticlePointCloud,
    scale: [f32; 3],
    shuffle: Option<u32>,
) -> Option<ParticleMeshSurface> {
    if !scale.iter().all(|value| (0.0..=1e8).contains(value)) {
        return None;
    }
    let count = cloud.points().len();
    let mut points: Vec<_> = cloud
        .points()
        .iter()
        .map(|p| std::array::from_fn(|axis| p[axis] * scale[axis]))
        .collect::<Vec<[f32; 3]>>();
    if let Some(seed) = shuffle {
        let mut state = u64::from(seed);
        for last in (1..count).rev() {
            state = state.wrapping_add(0x9e37_79b9_7f4a_7c15);
            points.swap(last, (mix(state) % (last as u64 + 1)) as usize);
        }
    }
    let mut sum = Vector3::<f64>::zeros();
    let mut radius_squared = 0.0_f64;
    let mut bytes = Vec::with_capacity(count.div_ceil(4) * SURFACE_RECORD_BYTES);
    for point in points {
        let value = Vector3::from(point).cast::<f64>();
        sum += value;
        radius_squared = radius_squared.max(value.norm_squared());
        // Four XYZ lanes share one GPU surface record. The fourth word in
        // each lane is unused, so point tables do not expand to triangles.
        for component in point.into_iter().chain([0.0]) {
            bytes.extend_from_slice(&component.to_ne_bytes());
        }
    }
    bytes.resize(bytes.len().next_multiple_of(SURFACE_RECORD_BYTES), 0);
    Some(ParticleMeshSurface {
        bytes,
        radius: radius_squared.sqrt(),
        points: Some((count as u32, (sum / count as f64).cast::<f32>().into())),
    })
}

fn alias_table(weights: &mut [f64]) -> Vec<(f32, u32)> {
    let scale = weights.len() as f64 / weights.iter().sum::<f64>();
    let mut small = Vec::new();
    let mut large = Vec::new();
    let mut result = Vec::with_capacity(weights.len());
    for (index, weight) in weights.iter_mut().enumerate() {
        *weight *= scale;
        if *weight < 1.0 {
            small.push(index);
        } else {
            large.push(index);
        }
        result.push((1.0, index as u32));
    }
    while let (Some(low), Some(high)) = (small.pop(), large.pop()) {
        result[low] = (weights[low].clamp(0.0, 1.0) as f32, high as u32);
        weights[high] += weights[low] - 1.0;
        if weights[high] < 1.0 {
            small.push(high);
        } else {
            large.push(high);
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn aliases_reproduce_triangles_with_unequal_areas() {
        let mut weights = [0.001, 1.0, 0.15, 11.0, 5.0, 0.000001];
        let expected = weights.map(|weight| weight / weights.iter().sum::<f64>());
        let table = alias_table(&mut weights);
        let mut observed = [0.0; 6];
        for (index, (probability, alias)) in table.into_iter().enumerate() {
            observed[index] += f64::from(probability) / 6.0;
            observed[alias as usize] += f64::from(1.0 - probability) / 6.0;
        }
        for (actual, expected) in observed.into_iter().zip(expected) {
            assert!((actual - expected).abs() < 1e-7);
        }
    }
}
