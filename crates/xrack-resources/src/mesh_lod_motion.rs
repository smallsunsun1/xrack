//! Deformation attributes and pose error checks for automatic LOD meshes.
use std::{
    hash::{Hash, Hasher},
    ops::Range,
};

use nalgebra::Vector3;
use xrack_runtime::scene3d::{
    JointPalette, JointRows, MAX_MORPH_TARGETS, MAX_SKIN_JOINTS, Mesh3D, MeshInfluence, MorphDelta,
    corresponding_vertices,
};

pub(super) struct Motion<'a> {
    mesh: &'a Mesh3D,
    palettes: &'a [JointPalette],
    scale: f64,
}

impl<'a> Motion<'a> {
    pub fn new(mesh: &'a Mesh3D, palettes: &'a [JointPalette], scale: f64) -> Self {
        Self {
            mesh,
            palettes,
            scale,
        }
    }

    pub fn can_sample(&self) -> bool {
        self.mesh.influences().is_none() || !self.palettes.is_empty()
    }

    pub fn key(&self, index: usize) -> MotionKey<'a> {
        MotionKey {
            skin: self
                .mesh
                .influences()
                .map(|weights| weights[index].packed()),
            morph: self.morph(index),
        }
    }

    fn morph(&self, index: usize) -> &'a [MorphDelta] {
        self.mesh
            .morph_geometry()
            .and_then(|morph| morph.vertex_deltas(index))
            .unwrap_or_default()
    }

    pub fn channels(&self, part: Range<usize>, budget: usize) -> MotionChannels {
        let mut totals = [0.0_f64; MAX_SKIN_JOINTS];
        if let Some(influences) = self.mesh.influences() {
            for influence in &influences[part.clone()] {
                for (joint, weight) in influence.joints.into_iter().zip(influence.weights) {
                    if weight > 0.0 {
                        totals[usize::from(joint)] += f64::from(weight);
                    }
                }
            }
        }
        let joints = strongest(&totals, budget.min(8));
        let mut deltas = [0.0_f64; MAX_MORPH_TARGETS];
        for index in part {
            for delta in self.morph(index) {
                deltas[delta.target as usize] += delta
                    .position
                    .into_iter()
                    .map(|value| f64::from(value).powi(2))
                    .sum::<f64>();
            }
        }
        let targets = strongest(&deltas, (budget - joints.len()) / 3);
        MotionChannels { joints, targets }
    }

    pub fn attributes(&self, index: usize, channels: &MotionChannels, output: &mut Vec<f32>) {
        if let Some(influences) = self.mesh.influences() {
            let influence = influences[index];
            for &joint in &channels.joints {
                output.push(
                    influence
                        .joints
                        .into_iter()
                        .zip(influence.weights)
                        .filter(|(index, _)| usize::from(*index) == joint)
                        .map(|(_, weight)| weight)
                        .sum(),
                );
            }
        }
        for &target in &channels.targets {
            let delta = self
                .morph(index)
                .iter()
                .find(|delta| delta.target as usize == target);
            output.extend(delta.map_or([0.0; 3], |delta| {
                delta
                    .position
                    .map(|value| (f64::from(value) / self.scale).clamp(-10_000.0, 10_000.0) as f32)
            }));
        }
    }

    pub fn error(
        &self,
        original: &[usize],
        positions: &[[f32; 3]],
        retained: &[u32],
        budget: f32,
    ) -> Option<f32> {
        if self.mesh.influences().is_none() && self.mesh.morph_geometry().is_none() {
            return Some(0.0);
        }
        let nearest = corresponding_vertices(positions, retained)?;
        let mut max_error = 0.0_f64;
        for palette in std::iter::once(None).chain(self.palettes.iter().map(Some)) {
            let rows = self.poses(original, palette)?;
            let positions: Vec<_> = original
                .iter()
                .enumerate()
                .map(|(index, &source)| {
                    let input = self.mesh.vertices()[source].position;
                    point(input, rows.as_ref().map(|rows| &rows[index]))
                })
                .collect();
            for (index, sample) in nearest.iter().enumerate() {
                if sample.vertices[0] == index && sample.weights[0] == 1.0 {
                    continue;
                }
                let approx = sample
                    .vertices
                    .into_iter()
                    .zip(sample.weights)
                    .map(|(vertex, weight)| positions[vertex] * weight)
                    .sum::<Vector3<f64>>();
                let mut error = (positions[index] - approx).norm();
                if self.mesh.morph_geometry().is_some() {
                    // The sum bounds the contribution of independently blended
                    // targets for weights with absolute value at most one.
                    let mut deltas = [Vector3::<f64>::zeros(); MAX_MORPH_TARGETS];
                    for (vertex, weight) in std::iter::once((index, 1.0)).chain(
                        sample
                            .vertices
                            .into_iter()
                            .zip(sample.weights.map(|weight| -weight)),
                    ) {
                        let matrix = rows.as_ref().map(|rows| &rows[vertex]);
                        for delta in self.morph(original[vertex]) {
                            deltas[delta.target as usize] +=
                                weight * displacement(delta.position, matrix);
                        }
                    }
                    error += deltas.into_iter().map(|delta| delta.norm()).sum::<f64>();
                }
                let normalized = error / self.scale;
                if !normalized.is_finite() || normalized > f64::from(budget) {
                    return None;
                }
                max_error = max_error.max(normalized);
            }
        }
        Some(max_error as f32)
    }

    fn poses(
        &self,
        original: &[usize],
        palette: Option<&JointPalette>,
    ) -> Option<Option<Vec<JointRows>>> {
        match self.mesh.influences().zip(palette) {
            Some((influences, palette)) => original
                .iter()
                .map(|&vertex| influences[vertex].deformation_rows(palette))
                .collect::<Option<Vec<_>>>()
                .map(Some),
            None => Some(None),
        }
    }
}

fn strongest(values: &[f64], count: usize) -> Vec<usize> {
    let mut weights: Vec<_> = values
        .iter()
        .enumerate()
        .filter(|(_, value)| **value > 0.0)
        .collect();
    weights.sort_by(|(a, x), (b, y)| y.total_cmp(x).then_with(|| a.cmp(b)));
    weights
        .into_iter()
        .take(count)
        .map(|(index, _)| index)
        .collect()
}

fn point(input: [f32; 3], rows: Option<&JointRows>) -> Vector3<f64> {
    let origin = displacement(input, rows);
    origin
        + rows.map_or_else(Vector3::zeros, |rows| {
            Vector3::from(rows.map(|row| f64::from(row[3])))
        })
}

fn displacement(input: [f32; 3], rows: Option<&JointRows>) -> Vector3<f64> {
    let [x, y, z] = input.map(f64::from);
    rows.map_or_else(
        || Vector3::new(x, y, z),
        |rows| {
            Vector3::from(
                rows.map(|[a, b, c, _]| f64::from(a) * x + f64::from(b) * y + f64::from(c) * z),
            )
        },
    )
}

pub(super) struct MotionChannels {
    joints: Vec<usize>,
    targets: Vec<usize>,
}

impl MotionChannels {
    pub fn weights(&self) -> impl Iterator<Item = f32> {
        std::iter::repeat_n(0.35, self.joints.len())
            .chain(std::iter::repeat_n(0.9, self.targets.len() * 3))
    }
}

#[derive(PartialEq)]
pub(super) struct MotionKey<'a> {
    skin: Option<[u8; MeshInfluence::STRIDE]>,
    // The glTF loader validates finite values. Both signed zeros hash alike.
    morph: &'a [MorphDelta],
}

impl Eq for MotionKey<'_> {}

impl Hash for MotionKey<'_> {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.skin.hash(state);
        self.morph.len().hash(state);
        for delta in self.morph {
            delta.target.hash(state);
            for value in delta
                .position
                .into_iter()
                .chain(delta.normal)
                .chain(delta.tangent)
            {
                (if value == 0.0 { 0 } else { value.to_bits() }).hash(state);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_runtime::scene3d::{MeshMaterial, MeshPart, MeshVertex};

    const CORNERS: [[f32; 3]; 4] = [
        [0.0, 0.0, 0.0],
        [1.0, 0.0, 0.0],
        [0.0, 1.0, 0.0],
        [0.25, 0.25, 0.0],
    ];
    const ORIGINAL: [usize; 4] = [0, 1, 4, 2];
    const RETAINED: [u32; 3] = [0, 1, 2];

    fn surface() -> Mesh3D {
        let vertices = [0, 1, 3, 1, 2, 3, 2, 0, 3].map(|index| MeshVertex {
            position: CORNERS[index],
            normal: [0.0, 0.0, 1.0],
            uv: [0.0; 2],
            uv1: [0.0; 2],
            color: [1.0; 4],
        });
        Mesh3D::imported(
            vertices.into(),
            Vec::new(),
            vec![MeshPart {
                vertices: 0..9,
                material: 0,
                volume_inverse: None,
            }],
            vec![MeshMaterial {
                base_color: [1.0; 4],
                alpha_mode: xrack_runtime::scene3d::MeshAlphaMode::Opaque,
                double_sided: false,
                base_texture: None,
                pbr: None,
            }],
        )
        .expect("valid test surface")
    }

    #[test]
    fn a_flat_bind_pose_cannot_hide_a_removed_morph_peak() {
        let delta = MorphDelta {
            position: [0.0, 0.0, 0.5],
            ..Default::default()
        };
        let mesh = surface()
            .with_morph(
                1,
                Vec::new().into(),
                vec![0, 0, 0, 1, 1, 1, 2, 2, 2, 3],
                vec![delta; 3],
            )
            .expect("valid morph");
        let motion = Motion::new(&mesh, &[], 1.0);
        assert!(motion.error(&ORIGINAL, &CORNERS, &RETAINED, 0.1).is_none());
        assert_eq!(motion.error(&ORIGINAL, &CORNERS, &RETAINED, 1.0), Some(0.5));
    }

    #[test]
    fn joint_samples_reject_a_collapse_that_only_fits_the_bind_pose() {
        let weights = (0..9)
            .map(|index| {
                let mut joints = [0; xrack_runtime::scene3d::SKIN_INFLUENCES];
                joints[0] = u16::from(index % 3 == 2);
                let mut weights = [0.0; xrack_runtime::scene3d::SKIN_INFLUENCES];
                weights[0] = 1.0;
                MeshInfluence { joints, weights }
            })
            .collect();
        let mesh = surface().with_skin_weights(weights).expect("valid weights");
        let rest = [
            [1.0, 0.0, 0.0, 0.0],
            [0.0, 1.0, 0.0, 0.0],
            [0.0, 0.0, 1.0, 0.0],
        ];
        let held: JointPalette = [rest, rest].into();
        let mut lifted = rest;
        lifted[2][3] = 0.5;
        let raised: JointPalette = [rest, lifted].into();
        assert_eq!(
            Motion::new(&mesh, &[held], 1.0).error(&ORIGINAL, &CORNERS, &RETAINED, 0.1),
            Some(0.0)
        );
        assert!(
            Motion::new(&mesh, &[raised], 1.0)
                .error(&ORIGINAL, &CORNERS, &RETAINED, 0.1)
                .is_none()
        );
    }
}
