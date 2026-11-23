//! Offline mesh simplification shared by AOT assets and preview resource loading.
use std::collections::HashMap;

use xrack_runtime::scene3d::{JointPalette, Mesh3D, MeshAsset, MeshPart, MeshTangent, MeshVertex};

use crate::mesh_lod_motion::Motion;

const MIN_TRIANGLES: usize = 48;
const ATTRIBUTES: usize = 15;
// Normal, both UV sets, color and tangent. Split vertices remain split.
const WEIGHTS: [f32; ATTRIBUTES] = [
    0.5, 0.5, 0.5, 1.0, 1.0, 1.0, 1.0, 0.6, 0.6, 0.6, 1.0, 0.5, 0.5, 0.5, 1.0,
];
// Desired triangle ratio, maximum normalized error, maximum logical pixels.
const LEVELS: [(f32, f32, f64); 2] = [(0.5, 0.035, 140.0), (0.125, 0.12, 60.0)];
const PIXEL_ERROR: f64 = 2.0;

pub(super) fn generate(asset: MeshAsset) -> MeshAsset {
    let meshes = asset.geometry_meshes();
    let palettes = asset.lod_joint_palettes();
    let prepared: Vec<_> = meshes
        .iter()
        .enumerate()
        .map(|(index, mesh)| {
            PreparedMesh::new(mesh, palettes.get(index).map_or(&[][..], Vec::as_slice))
        })
        .collect();
    let mut previous: Vec<_> = meshes.iter().cloned().map(|mesh| (mesh, 0.0_f32)).collect();
    let mut levels = Vec::new();
    let mut last_size = f64::INFINITY;
    for (ratio, error, size) in LEVELS {
        let mut changed = false;
        for ((base, prepared), (old, old_error)) in meshes.iter().zip(&prepared).zip(&mut previous)
        {
            let Some((mesh, error)) = prepared
                .as_ref()
                .and_then(|prepared| prepared.level(base, ratio, error))
            else {
                continue;
            };
            if mesh.vertices().len() * 8 <= old.vertices().len() * 7 {
                *old = mesh;
                *old_error = error;
                changed = true;
            }
        }
        if changed {
            let error = previous
                .iter()
                .map(|(_, error)| f64::from(*error))
                .fold(0.0, f64::max);
            let max_size = size.min(PIXEL_ERROR / error.max(1e-6)).min(last_size * 0.9);
            levels.push((
                max_size,
                previous.iter().map(|(mesh, _)| mesh.clone()).collect(),
            ));
            last_size = max_size;
        }
    }
    asset.with_lods(levels)
}

struct PreparedMesh<'a> {
    parts: Vec<Option<PreparedPart>>,
    motion: Motion<'a>,
}

impl<'a> PreparedMesh<'a> {
    fn new(mesh: &'a Mesh3D, palettes: &'a [JointPalette]) -> Option<Self> {
        if !matches!(mesh, Mesh3D::Imported(_)) || mesh.vertices().len() < MIN_TRIANGLES * 3 {
            return None;
        }
        let mut min = [f64::INFINITY; 3];
        let mut max = [f64::NEG_INFINITY; 3];
        for vertex in mesh.vertices() {
            for (axis, value) in vertex.position.into_iter().map(f64::from).enumerate() {
                min[axis] = min[axis].min(value);
                max[axis] = max[axis].max(value);
            }
        }
        let scale = min
            .into_iter()
            .zip(max)
            .map(|(a, b)| b - a)
            .fold(0.0, f64::max);
        if scale == 0.0 || !scale.is_finite() {
            return None;
        }
        let motion = Motion::new(mesh, palettes, scale);
        if !motion.can_sample() {
            return None;
        }
        let parts = mesh
            .parts()
            .iter()
            .map(|part| {
                (part.vertices.len() >= MIN_TRIANGLES * 3)
                    .then(|| PreparedPart::new(mesh, part, min, scale, &motion))
            })
            .collect();
        Some(Self { parts, motion })
    }

    fn level(&self, mesh: &Mesh3D, ratio: f32, error: f32) -> Option<(Mesh3D, f32)> {
        let mut vertices = Vec::new();
        let mut parts = Vec::new();
        let mut result_error = 0.0_f32;
        for (original, prepared) in mesh.parts().iter().zip(&self.parts) {
            let start = vertices.len();
            if let Some((indices, error)) = prepared
                .as_ref()
                .and_then(|part| part.level(ratio, error, &self.motion))
            {
                vertices.extend(indices);
                result_error = result_error.max(error);
            } else {
                vertices.extend(original.vertices.clone());
            }
            parts.push(MeshPart {
                vertices: start..vertices.len(),
                material: original.material,
                volume_inverse: original.volume_inverse,
            });
        }
        if vertices.len() == mesh.vertices().len() {
            return None;
        }
        mesh.triangle_subset(&vertices, parts)
            .map(|mesh| (mesh, result_error))
    }
}

struct PreparedPart {
    indices: Vec<u32>,
    positions: Vec<[f32; 3]>,
    attributes: Vec<f32>,
    weights: Vec<f32>,
    source_vertices: Vec<usize>,
}

impl PreparedPart {
    fn new(mesh: &Mesh3D, part: &MeshPart, min: [f64; 3], scale: f64, motion: &Motion<'_>) -> Self {
        let lightmapped = !mesh.lightmap_uvs().is_empty();
        let lightmap_channels = if lightmapped { &[0.6, 0.6][..] } else { &[] };
        let channels = motion.channels(
            part.vertices.clone(),
            32 - ATTRIBUTES - lightmap_channels.len(),
        );
        let weights: Vec<_> = WEIGHTS
            .into_iter()
            .chain(lightmap_channels.iter().copied())
            .chain(channels.weights())
            .collect();
        let mut indices = Vec::with_capacity(part.vertices.len());
        let mut positions = Vec::new();
        let mut attributes = Vec::new();
        let mut source_vertices = Vec::new();
        let mut unique = HashMap::new();
        for index in part.vertices.clone() {
            let vertex = &mesh.vertices()[index];
            let tangent = mesh.tangents().get(index).copied().unwrap_or([0; 4]);
            let chart = mesh.lightmap_uvs().get(index).copied();
            let key = (
                vertex.position.map(bits),
                vertex.normal.map(bits),
                vertex.uv.map(bits),
                vertex.uv1.map(bits),
                vertex.color.map(bits),
                tangent,
                chart.map(|uv| uv.map(bits)),
                motion.key(index),
            );
            let slot = *unique.entry(key).or_insert_with(|| {
                let slot = source_vertices.len() as u32;
                source_vertices.push(index);
                positions.push(std::array::from_fn(|axis| {
                    ((f64::from(vertex.position[axis]) - min[axis]) / scale) as f32
                }));
                attributes.extend(attributes_for(vertex, tangent));
                attributes.extend(chart.into_iter().flatten());
                motion.attributes(index, &channels, &mut attributes);
                slot
            });
            indices.push(slot);
        }
        Self {
            indices,
            positions,
            attributes,
            weights,
            source_vertices,
        }
    }

    fn level(&self, ratio: f32, error: f32, motion: &Motion<'_>) -> Option<(Vec<usize>, f32)> {
        let mut output = vec![0; self.indices.len()];
        let target = ((self.indices.len() / 3) as f32 * ratio).max(1.0) as usize * 3;
        let mut measured = 0.0;
        let count = rmeshopt::simplify_with_attributes(
            &mut output,
            &self.indices,
            self.positions.as_flattened(),
            self.source_vertices.len(),
            3 * size_of::<f32>(),
            &self.attributes,
            self.weights.len() * size_of::<f32>(),
            &self.weights,
            None,
            target,
            error,
            rmeshopt::SIMPLIFY_LOCK_BORDER | rmeshopt::SIMPLIFY_ERROR_ABSOLUTE,
            Some(&mut measured),
        );
        if count == 0
            || count >= self.indices.len()
            || !count.is_multiple_of(3)
            || !measured.is_finite()
            || measured > error
        {
            return None;
        }
        let retained = output.get(..count)?;
        let posed = motion.error(&self.source_vertices, &self.positions, retained, error)?;
        let points = retained
            .iter()
            .map(|&index| self.source_vertices.get(index as usize).copied())
            .collect::<Option<Vec<_>>>()?;
        Some((points, measured.max(posed)))
    }
}

fn attributes_for(vertex: &MeshVertex, tangent: MeshTangent) -> [f32; ATTRIBUTES] {
    let [nx, ny, nz] = vertex.normal;
    let [u, v] = vertex.uv;
    let [u1, v1] = vertex.uv1;
    let [r, g, b, a] = vertex.color;
    let [tx, ty, tz, handedness] = tangent.map(|value| f32::from(value) / 32767.0);
    [nx, ny, nz, u, v, u1, v1, r, g, b, a, tx, ty, tz, handedness]
}

fn bits(value: f32) -> u32 {
    if value == 0.0 { 0 } else { value.to_bits() }
}
