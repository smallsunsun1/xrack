//! Immutable positions imported from a particle shape asset.
use std::sync::Arc;

#[doc(hidden)]
pub const MAX_PARTICLE_POINT_COUNT: usize = 1_000_000;

#[doc(hidden)]
#[derive(Clone, Debug)]
pub struct ParticlePointCloud(Arc<[[f32; 3]]>);

impl ParticlePointCloud {
    pub fn new(points: Vec<[f32; 3]>) -> Option<Self> {
        (!points.is_empty()
            && points.len() <= MAX_PARTICLE_POINT_COUNT
            && points
                .iter()
                .flatten()
                .all(|value| (-1e8..=1e8).contains(value)))
        .then(|| Self(points.into()))
    }

    pub fn points(&self) -> &[[f32; 3]] {
        &self.0
    }

    /// AOT resources use fixed-width little-endian coordinates.
    pub fn from_packed(bytes: &[u8]) -> Option<Self> {
        let (rows, remainder) = bytes.as_chunks::<12>();
        if rows.is_empty() || !remainder.is_empty() || rows.len() > MAX_PARTICLE_POINT_COUNT {
            return None;
        }
        Self::new(
            rows.iter()
                .map(|row| {
                    let (coordinates, _) = row.as_chunks::<4>();
                    std::array::from_fn(|axis| f32::from_le_bytes(coordinates[axis]))
                })
                .collect(),
        )
    }
}
