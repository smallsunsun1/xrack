//! Area-weighted particle sampling from authored model surfaces.
use std::{cell::RefCell, collections::VecDeque, sync::Arc};

use nalgebra::Vector3;

use crate::{
    scene_hierarchy::rest_mesh_objects,
    scene3d::{MAX_IMPORTED_VERTICES, MeshAsset},
};

const MAX_TRIANGLES: usize = MAX_IMPORTED_VERTICES / 3;
const TRIANGLE_BYTES: usize = 64;
const CACHE_BYTES: usize = 40 * 1024 * 1024;
const CACHE_ENTRIES: usize = 8;

/// GPU-ready surface positions, normals and alias table. Geometry stays
/// emitter-local so translating or rotating the emitter reuses the table.
#[doc(hidden)]
#[derive(Debug)]
pub struct ParticleMeshSurface {
    bytes: Vec<u8>,
    pub(crate) radius: f64,
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
}

#[derive(Default)]
pub(crate) struct ParticleSurfaceCache {
    entries: RefCell<VecDeque<Entry>>,
}

struct Entry {
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
        if key.is_empty() {
            return None;
        }
        {
            let mut entries = self.entries.borrow_mut();
            if let Some(index) = entries
                .iter()
                .position(|entry| entry.key == key && entry.scale == scale)
                && let Some(entry) = entries.remove(index)
            {
                let result = entry.surface.clone();
                entries.push_front(entry);
                return result;
            }
        }
        let surface = MeshAsset::resolve(key, imported)
            .and_then(|asset| build_surface(&asset, scale))
            .map(Arc::new);
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
            key: key.into(),
            scale,
            surface: surface.clone(),
        });
        surface
    }
}

struct Triangle {
    points: [[f32; 3]; 3],
    normal: [f32; 3],
}

fn build_surface(asset: &MeshAsset, scale: [f32; 3]) -> Option<ParticleMeshSurface> {
    let objects = rest_mesh_objects(asset);
    let mut triangles = Vec::new();
    let mut weights = Vec::new();
    for points in objects.iter().flat_map(|object| object.posed_triangles()) {
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
    let mut bytes = Vec::with_capacity(triangles.len() * TRIANGLE_BYTES);
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
