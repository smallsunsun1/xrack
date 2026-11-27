//! Typed local fog volumes shared by generated and interpreted scene views.
use std::sync::Arc;

use crate::{
    Color, RecordList, Value, VecModel, Vector3D,
    scene3d::MAIN_LAYER,
    scene3d::bounds::Bounds3D,
    scene3d::camera::{Point3D, SceneCamera},
    scene3d::hierarchy::euler_rotation,
    scene3d::lighting::linear_color,
    scene3d::visibility::Frustum,
};

#[doc(hidden)]
pub const MAX_FOG_VOLUMES: usize = 8;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogVolume3D {
    pub center: [f32; 3],
    pub half_size: [f32; 3],
    pub density: f32,
    pub color: [f32; 3],
    pub ellipsoid: bool,
    pub layers: u64,
    pub local_from_world: [[f32; 3]; 3],
    pub rotated: bool,
    pub height: Option<[f32; 3]>,
    pub noise: Option<FogNoise>,
    bounds: Bounds3D,
}

#[doc(hidden)]
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FogNoise {
    pub amount: f32,
    pub frequency: f32,
    pub phase: [f32; 3],
}

impl FogNoise {
    pub fn prepare(amount: f64, size: f64, offset: Vector3D) -> Option<Self> {
        let amount = amount as f32;
        let offset = [offset.x, offset.y, offset.z];
        if !amount.is_finite()
            || amount <= 0.0
            || !size.is_finite()
            || size <= 0.0
            || !offset.into_iter().all(f64::is_finite)
        {
            return None;
        }
        // Global coordinates repeat every 256 lattice cells. Reduce animation
        // offsets in f64 before uploading them to retain sub-cell precision.
        let size = size.clamp(0.05, 1000.0);
        let world_period = size * 256.0;
        Some(Self {
            amount: amount.min(1.0),
            frequency: world_period.recip() as f32,
            phase: offset.map(|value| (value.rem_euclid(world_period) / world_period) as f32),
        })
    }
}

#[doc(hidden)]
pub struct FogVolumeDefinition<'a> {
    pub position: Point3D,
    pub size: Vector3D,
    pub density: f64,
    pub color: Color,
    pub shape: &'a str,
    pub layers: u64,
    pub angles: [f64; 3],
    /// World Y at zero and full density.
    pub height: Option<[f64; 2]>,
    pub noise: Option<FogNoise>,
}

impl FogVolumeDefinition<'_> {
    pub fn prepare(self) -> Option<FogVolume3D> {
        let Self {
            position,
            size,
            density,
            color,
            shape,
            layers,
            angles,
            height,
            noise,
        } = self;
        let ellipsoid = match shape {
            "" | "box" => false,
            "sphere" => true,
            _ => return None,
        };
        let center = [position.x, position.y, position.z].map(|axis| axis as f32);
        let half_size = [size.x, size.y, size.z].map(|axis| (axis * 0.5) as f32);
        let density = density as f32;
        if !density.is_finite()
            || density <= 0.0
            || layers == 0
            || !center.into_iter().zip(half_size).all(|(center, radius)| {
                radius > 0.0
                    && radius.recip().is_finite()
                    && (center - radius).is_finite()
                    && (center + radius).is_finite()
                    && center - radius < center + radius
            })
        {
            return None;
        }
        let angles = angles.map(|value| value as f32);
        if !angles.into_iter().all(f32::is_finite) {
            return None;
        }
        let rotation = euler_rotation(angles);
        let local_from_world =
            std::array::from_fn(|axis| std::array::from_fn(|world| rotation[(world, axis)]));
        let rotated = *rotation.matrix() != nalgebra::Matrix3::identity();
        let radius: [f32; 3] = if rotated {
            std::array::from_fn(|axis| {
                let lengths = std::array::from_fn::<_, 3, _>(|local| {
                    f64::from(rotation[(axis, local)].abs()) * f64::from(half_size[local])
                });
                let radius = if ellipsoid {
                    lengths.into_iter().map(|r| r * r).sum::<f64>().sqrt()
                } else {
                    lengths.into_iter().sum()
                };
                (radius as f32).next_up()
            })
        } else {
            half_size
        };
        let bounds = Bounds3D {
            min: std::array::from_fn(|i| (center[i] - radius[i]).next_down()),
            max: std::array::from_fn(|i| (center[i] + radius[i]).next_up()),
        };
        if !bounds.is_finite() {
            return None;
        }
        let height = height.and_then(|[clear, dense]| {
            let (clear, dense) = (clear as f32, dense as f32);
            let span = dense - clear;
            let inverse = span.recip();
            (clear.is_finite() && dense.is_finite() && span.is_finite() && inverse.is_finite())
                .then_some([clear, dense, inverse])
        });
        Some(FogVolume3D {
            center,
            half_size,
            density: density.min(64.0),
            color: linear_color(color),
            ellipsoid,
            layers,
            local_from_world,
            rotated,
            height,
            noise,
            bounds,
        })
    }
}

impl FogVolume3D {
    pub fn world_bounds(&self) -> [[f32; 3]; 2] {
        [self.bounds.min, self.bounds.max]
    }
}

#[doc(hidden)]
pub fn visible_fog_volumes<'a>(
    volumes: &'a [FogVolume3D],
    camera: &SceneCamera,
) -> impl Iterator<Item = &'a FogVolume3D> + use<'a> {
    let layers = camera.layer_mask();
    let clip = Frustum::new(camera);
    volumes
        .iter()
        .filter(move |volume| volume.layers & layers != 0 && clip.intersects(volume.bounds))
        .take(MAX_FOG_VOLUMES)
}

#[doc(hidden)]
pub fn model_fog_volumes<T: Clone>(
    list: &VecModel<T>,
    project: impl Fn(T) -> Option<FogVolume3D>,
) -> Arc<[FogVolume3D]> {
    (0..list.len())
        .filter_map(|index| project(list.row(index)?))
        .collect()
}

#[doc(hidden)]
pub fn record_fog_volumes(list: &RecordList) -> Arc<[FogVolume3D]> {
    model_fog_volumes(list.model(), |row| {
        if row.field("visible") == Some(&Value::Bool(false)) {
            return None;
        }
        let Value::Point3D(position) = row.field("position")? else {
            return None;
        };
        let Value::Vector3D(size) = row.field("size")? else {
            return None;
        };
        let Value::Float(density) = row.field("density")? else {
            return None;
        };
        let Value::Color(color) = row.field("color")? else {
            return None;
        };
        let shape = match row.field("fog-shape") {
            Some(Value::String(shape)) => shape,
            _ => "box",
        };
        let layers = match row.field("layers") {
            Some(Value::Int(layers)) => *layers as u64,
            _ => MAIN_LAYER,
        };
        let number = |name, default| match row.field(name) {
            Some(Value::Float(value)) => *value,
            _ => default,
        };
        let offset = match row.field("fog-noise-offset") {
            Some(Value::Vector3D(offset)) => *offset,
            _ => Vector3D::default(),
        };
        FogVolumeDefinition {
            position: *position,
            size: *size,
            density: *density,
            color: *color,
            shape,
            layers,
            angles: [
                number("yaw", 0.0),
                number("pitch", 0.0),
                number("roll", 0.0),
            ],
            noise: FogNoise::prepare(
                number("fog-noise-amount", 0.0),
                number("fog-noise-scale", 0.8),
                offset,
            ),
            height: (row.field("fog-height-enabled") == Some(&Value::Bool(true))).then(|| {
                [
                    number("fog-height-start", 10.0),
                    number("fog-height-end", 0.0),
                ]
            }),
        }
        .prepare()
    })
}
