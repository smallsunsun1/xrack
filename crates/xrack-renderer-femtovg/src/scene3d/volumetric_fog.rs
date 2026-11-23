//! Pack the visible local fog volumes for the color and background shaders.
use super::append_floats;
use crate::Node;
use xrack_runtime::{
    Value,
    scene3d::{FogVolume3D, MAX_FOG_VOLUMES, SceneCamera, visible_fog_volumes},
};

pub(super) const VOLUME_UNIFORM_BYTES: usize = 48 + 128 * MAX_FOG_VOLUMES;

#[derive(Clone, Copy, PartialEq, Eq, Hash)]
pub(super) enum FogMode {
    Off,
    Unlit,
    Lit,
    Profiled,
    ProfiledLit,
    Noisy,
    NoisyLit,
}

impl FogMode {
    pub(super) const COUNT: usize = Self::NoisyLit as usize + 1;

    pub(super) fn noisy(self) -> bool {
        matches!(self, Self::Noisy | Self::NoisyLit)
    }

    pub(super) fn lit(self) -> bool {
        matches!(self, Self::Lit | Self::ProfiledLit | Self::NoisyLit)
    }

    pub(super) fn profiled(self) -> bool {
        matches!(self, Self::Profiled | Self::ProfiledLit)
    }
}

#[derive(Default)]
pub(super) struct Scattering {
    pub strength: f32,
    asymmetry: f32,
    samples: u32,
}

impl Scattering {
    pub(super) fn for_view(node: &Node, has_volumes: bool, has_light: bool) -> Self {
        if !has_volumes || !has_light {
            return Self::default();
        }
        let finite = |name| {
            node.float(name)
                .filter(|value| value.is_finite())
                .unwrap_or(0.0)
        };
        let strength = finite("fog-volume-scattering").clamp(0.0, 16.0);
        if strength == 0.0 {
            return Self::default();
        }
        Self {
            strength,
            asymmetry: finite("fog-volume-anisotropy").clamp(-0.85, 0.85),
            samples: node.int("fog-volume-samples").unwrap_or(8).clamp(1, 16) as u32,
        }
    }

    pub(super) fn mode(&self, volumes: &[&FogVolume3D]) -> FogMode {
        if volumes.is_empty() {
            return FogMode::Off;
        }
        let lit = self.strength > 0.0;
        if volumes.iter().any(|volume| volume.noise.is_some()) {
            return if lit {
                FogMode::NoisyLit
            } else {
                FogMode::Noisy
            };
        }
        match (volumes.iter().any(|volume| volume.height.is_some()), lit) {
            (false, false) => FogMode::Unlit,
            (false, true) => FogMode::Lit,
            (true, false) => FogMode::Profiled,
            (true, true) => FogMode::ProfiledLit,
        }
    }
}

pub(super) fn receiver_corners(volumes: &[&FogVolume3D]) -> Option<[[f32; 3]; 8]> {
    let [mut min, mut max] = volumes.first()?.world_bounds();
    for volume in &volumes[1..] {
        let [low, high] = volume.world_bounds();
        for axis in 0..3 {
            min[axis] = min[axis].min(low[axis]);
            max[axis] = max[axis].max(high[axis]);
        }
    }
    Some(std::array::from_fn(|corner| {
        std::array::from_fn(|axis| {
            if corner & (1 << axis) == 0 {
                min[axis]
            } else {
                max[axis]
            }
        })
    }))
}

pub(super) fn prepare<'a>(node: &'a Node, camera: Option<&SceneCamera>) -> Vec<&'a FogVolume3D> {
    match (node.properties.get("fog-volumes"), camera) {
        (Some(Value::SceneFogVolumes(volumes)), Some(camera)) if !volumes.is_empty() => {
            visible_fog_volumes(volumes, camera).collect()
        }
        _ => Vec::new(),
    }
}

pub(super) fn write(
    buffer: &mut Vec<u8>,
    volumes: &[&FogVolume3D],
    camera: &SceneCamera,
    background: [f64; 3],
    skybox: bool,
    scattering: &Scattering,
    density_samples: u32,
) {
    let end = buffer.len() + VOLUME_UNIFORM_BYTES;
    let [r, g, b] = background.map(|value| value as f32);
    append_floats(buffer, [r, g, b, f32::from(u8::from(skybox))]);
    let [near, far] = camera.depth_limits();
    append_floats(
        buffer,
        [
            near,
            far.min(f32::MAX),
            volumes.len() as f32,
            density_samples as f32,
        ],
    );
    append_floats(
        buffer,
        [
            scattering.strength,
            scattering.asymmetry,
            scattering.samples as f32,
            0.0,
        ],
    );
    for volume in volumes {
        let [x, y, z] = volume.center;
        append_floats(buffer, [x, y, z, volume.density]);
        let [x, y, z] = volume.half_size;
        append_floats(buffer, [x, y, z, f32::from(u8::from(volume.ellipsoid))]);
        let [r, g, b] = volume.color;
        append_floats(
            buffer,
            [r, g, b, volume.noise.map_or(0.0, |noise| noise.amount)],
        );
        let [clear, dense, inverse] = volume.height.unwrap_or_default();
        append_floats(
            buffer,
            [
                clear,
                dense,
                inverse,
                f32::from(u8::from(volume.height.is_some())),
            ],
        );
        for [x, y, z] in volume.local_from_world {
            append_floats(buffer, [x, y, z, f32::from(u8::from(volume.rotated))]);
        }
        let parameters = volume.noise.map_or([0.0; 4], |noise| {
            let [x, y, z] = noise.phase;
            [x, y, z, noise.frequency]
        });
        append_floats(buffer, parameters);
    }
    buffer.resize(end, 0);
}
