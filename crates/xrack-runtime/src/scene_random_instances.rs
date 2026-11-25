//! Seeded instance placements shared by AOT and the interpreter.

use crate::{
    Color, Vector3D,
    scene_hierarchy::{SceneTransform, multiply_instance_tint},
    scene_lighting::linear_srgb_channel,
};

/// Bindings projected from a model row's random-instances record.
#[doc(hidden)]
#[derive(Default)]
pub struct SceneRandomInstances {
    pub count: i64,
    pub enabled: Option<bool>,
    pub seed: i64,
    pub position: [Option<Vector3D>; 2],
    pub rotation: [Option<Vector3D>; 2],
    pub scale: [Option<Vector3D>; 2],
    pub proportional_scale: bool,
    pub color: [Option<Color>; 2],
    pub color_model: InstanceColorModel,
    pub proportional_color: bool,
    pub hue: [Option<f64>; 2],
    pub data: [Option<[f64; 4]>; 2],
    pub grid_spacing: Vector3D,
}

#[derive(Clone, Debug, PartialEq)]
pub(crate) struct RandomInstances {
    count: usize,
    seed: u64,
    position: Bounds<3>,
    rotation: Bounds<3>,
    scale: Bounds<3>,
    proportional_scale: bool,
    color: RandomColor,
    data: Option<Bounds<4>>,
    grid: Option<Grid>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) struct InstancePlacement {
    pub transform: SceneTransform,
    pub tint: InstanceTint,
    pub data: Option<[f32; 4]>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub(crate) enum InstanceTint {
    Linear([f32; 4]),
    Srgb([f32; 4]),
}

impl InstanceTint {
    pub fn linear(self) -> [f32; 4] {
        match self {
            Self::Linear(rgba) => rgba,
            Self::Srgb([r, g, b, a]) => [
                linear_srgb_channel(r),
                linear_srgb_channel(g),
                linear_srgb_channel(b),
                a,
            ],
        }
    }

    pub fn apply(self, base: Color) -> Color {
        if let Self::Srgb(mut rgba) = self
            && base.0 >> 8 == 0xff_ff_ff
        {
            // A white model needs no color multiplication. Keep the sampled
            // sRGB values through quantization instead of decoding and re-encoding.
            rgba[3] *= f32::from(base.0 as u8) / 255.0;
            return Color(u32::from_be_bytes(
                rgba.map(|channel| (channel * 255.0).round() as u8),
            ));
        }
        multiply_instance_tint(base, self.linear())
    }
}

/// Color coordinates used for component-wise sampling of a seeded range.
#[doc(hidden)]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum InstanceColorModel {
    #[default]
    Rgb,
    Hsv,
    Hsl,
}

impl InstanceColorModel {
    pub fn from_name(name: &str) -> Self {
        match name {
            "hsv" => Self::Hsv,
            "hsl" => Self::Hsl,
            _ => Self::Rgb,
        }
    }

    fn coordinates(self, color: Color) -> [f64; 4] {
        let [r, g, b, a] = color.normalized_rgba();
        if self == Self::Rgb {
            return [r, g, b, a];
        }
        let high = r.max(g).max(b);
        let low = r.min(g).min(b);
        let chroma = high - low;
        let component = if self == Self::Hsv {
            high
        } else {
            f64::midpoint(high, low)
        };
        if chroma == 0.0 {
            // A grey endpoint has no hue. Resolve it after reading both ends.
            return [f64::NAN, 0.0, component, a];
        }
        let hue = Color::hue_sector([r, g, b], high, chroma) / 6.0;
        let saturation = if self == Self::Hsv {
            chroma / high
        } else {
            chroma / (1.0 - (2.0 * component - 1.0).abs())
        };
        [hue, saturation, component, a]
    }

    fn srgb_tint(self, [x, y, z, a]: [f64; 4]) -> InstanceTint {
        let rgb = match self {
            Self::Rgb => [x, y, z],
            Self::Hsv => hue_rgb(x, z * y, z - z * y),
            Self::Hsl => {
                let chroma = (1.0 - (2.0 * z - 1.0).abs()) * y;
                hue_rgb(x, chroma, z - chroma / 2.0)
            }
        };
        let [r, g, b] = rgb.map(|channel| channel.clamp(0.0, 1.0) as f32);
        InstanceTint::Srgb([r, g, b, a as f32])
    }
}

fn hue_rgb(hue: f64, chroma: f64, offset: f64) -> [f64; 3] {
    let sector = hue.rem_euclid(1.0) * 6.0;
    Color::chromatic_rgb(sector, chroma).map(|v| v + offset)
}

#[derive(Clone, Debug, PartialEq)]
enum RandomColor {
    Fixed(InstanceTint),
    Range {
        model: InstanceColorModel,
        from: [f64; 4],
        to: [f64; 4],
        proportional: bool,
    },
}

impl RandomColor {
    fn new(settings: &SceneRandomInstances) -> Self {
        let model = settings.color_model;
        let [a, b] = settings.color;
        let from = a.or(b).unwrap_or(Color(0xffffffff));
        let [mut from, mut to] = [from, b.unwrap_or(from)].map(|color| model.coordinates(color));
        if model != InstanceColorModel::Rgb {
            for (end, hue) in [&mut from, &mut to].into_iter().zip(settings.hue) {
                if let Some(hue) = hue.filter(|value| value.is_finite()) {
                    end[0] = hue.clamp(0.0, 1.0);
                }
            }
            if !from[0].is_finite() {
                from[0] = if to[0].is_finite() { to[0] } else { 0.0 };
            }
            if !to[0].is_finite() {
                to[0] = from[0];
            }
        }
        if from == to {
            Self::Fixed(InstanceTint::Linear(model.srgb_tint(from).linear()))
        } else {
            Self::Range {
                model,
                from,
                to,
                proportional: settings.proportional_color,
            }
        }
    }

    fn sample(&self, seed: u64, index: usize) -> InstanceTint {
        match self {
            Self::Fixed(tint) => *tint,
            Self::Range {
                model,
                from,
                to,
                proportional,
            } => model.srgb_tint(std::array::from_fn(|channel| {
                let lane = if *proportional {
                    12
                } else {
                    12 + channel as u64
                };
                from[channel] + (to[channel] - from[channel]) * fraction(seed, index, lane)
            })),
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Bounds<const N: usize> {
    min: [f64; N],
    max: [f64; N],
}

impl<const N: usize> Bounds<N> {
    fn new([a, b]: [Option<[f64; N]>; 2], fallback: [f64; N], [low, high]: [f64; 2]) -> Self {
        let clean = |values: [f64; N]| {
            std::array::from_fn(|axis| {
                let value = values[axis];
                if value.is_finite() {
                    value.clamp(low, high)
                } else {
                    fallback[axis]
                }
            })
        };
        let a = clean(a.or(b).unwrap_or(fallback));
        let b = clean(b.unwrap_or(a));
        Self {
            min: std::array::from_fn(|axis| a[axis].min(b[axis])),
            max: std::array::from_fn(|axis| a[axis].max(b[axis])),
        }
    }

    fn sample(&self, seed: u64, index: usize, lane: u64, proportional: bool) -> [f64; N] {
        std::array::from_fn(|axis| {
            let fraction = fraction(
                seed,
                index,
                lane + if proportional { 0 } else { axis as u64 },
            );
            self.min[axis] + (self.max[axis] - self.min[axis]) * fraction
        })
    }
}

impl SceneRandomInstances {
    pub fn from_record(record: &crate::RecordValue) -> Self {
        use crate::Value;
        let vector = |name| match record.field(name) {
            Some(Value::Vector3D(value)) => Some(*value),
            _ => None,
        };
        let color = |name| match record.field(name) {
            Some(Value::Color(value)) => Some(*value),
            _ => None,
        };
        let data = |name| match record.field(name) {
            Some(Value::Record(value)) => {
                Some(
                    ["x", "y", "z", "w"].map(|channel| match value.field(channel) {
                        Some(Value::Float(value)) => *value,
                        _ => 0.0,
                    }),
                )
            }
            _ => None,
        };
        let integer = |name| match record.field(name) {
            Some(Value::Int(value)) => *value,
            _ => 0,
        };
        Self {
            count: integer("count"),
            enabled: match record.field("enabled") {
                Some(Value::Bool(value)) => Some(*value),
                _ => None,
            },
            seed: integer("seed"),
            position: ["position-min", "position-max"].map(vector),
            rotation: ["rotation-min", "rotation-max"].map(vector),
            scale: ["scale-min", "scale-max"].map(vector),
            proportional_scale: record.field("proportional-scale") == Some(&Value::Bool(true)),
            color: ["color-min", "color-max"].map(color),
            color_model: match record.field("color-model") {
                Some(Value::String(name)) => InstanceColorModel::from_name(name),
                _ => InstanceColorModel::default(),
            },
            proportional_color: record.field("proportional-color") == Some(&Value::Bool(true)),
            hue: ["hue-min", "hue-max"].map(|name| match record.field(name) {
                Some(Value::Float(value)) => Some(*value),
                _ => None,
            }),
            data: ["data-min", "data-max"].map(data),
            grid_spacing: vector("grid-spacing").unwrap_or_default(),
        }
    }

    pub(crate) fn prepare(self, limit: usize) -> RandomInstances {
        let coords = |v: Vector3D| [v.x, v.y, v.z];
        let position = Bounds::new(
            self.position.map(|v| v.map(coords)),
            [0.0; 3],
            [-200.0, 200.0],
        );
        let rotation = Bounds::new(
            self.rotation.map(|v| v.map(coords)),
            [0.0; 3],
            [-1.0e9, 1.0e9],
        );
        let scale = Bounds::new(self.scale.map(|v| v.map(coords)), [1.0; 3], [0.0001, 200.0]);
        let color = RandomColor::new(&self);
        let data = self.data.iter().any(Option::is_some).then(|| {
            Bounds::new(
                self.data,
                [0.0; 4],
                [-f64::from(f32::MAX), f64::from(f32::MAX)],
            )
        });
        let seed = self.seed as u64;
        let mut count = self.count.clamp(0, limit as i64) as usize;
        let grid = Grid::new(&position, coords(self.grid_spacing), seed);
        if let Some(grid) = &grid {
            count = (count as u128).min(grid.capacity) as usize;
        }
        RandomInstances {
            count,
            seed,
            position,
            rotation,
            scale,
            proportional_scale: self.proportional_scale,
            color,
            data,
            grid,
        }
    }
}

impl RandomInstances {
    pub fn len(&self) -> usize {
        self.count
    }

    pub fn transform(&self, index: usize) -> Option<SceneTransform> {
        let mut position = self.position.sample(self.seed, index, 0, false);
        if let Some(grid) = &self.grid {
            grid.place(index, self.position.min, &mut position);
        }
        let [pitch, yaw, roll] = self
            .rotation
            .sample(self.seed, index, 4, false)
            .map(|v| v as f32);
        let scale = self
            .scale
            .sample(self.seed, index, 8, self.proportional_scale)
            .map(|v| v as f32);
        SceneTransform::local(
            position.map(|v| v as f32),
            scale,
            [yaw, pitch, roll],
            [0.0; 3],
        )
        .map(|(transform, _)| transform)
    }

    pub fn sample(&self, index: usize) -> Option<InstancePlacement> {
        let transform = self.transform(index)?;
        let tint = self.color.sample(self.seed, index);
        let data = self
            .data
            .as_ref()
            .map(|bounds| bounds.sample(self.seed, index, 16, false).map(|v| v as f32));
        Some(InstancePlacement {
            transform,
            tint,
            data,
        })
    }
}

#[derive(Clone, Debug, PartialEq)]
struct Grid {
    steps: [f64; 3],
    axes: [u32; 3],
    capacity: u128,
    first: u128,
    stride: u128,
}

impl Grid {
    fn new(bounds: &Bounds<3>, mut steps: [f64; 3], seed: u64) -> Option<Self> {
        let axes = std::array::from_fn(|axis| {
            let span = bounds.max[axis] - bounds.min[axis];
            let step = &mut steps[axis];
            if !step.is_finite() || *step <= 0.0 {
                *step = 0.0;
                return 1;
            }
            // Preserve distinct coordinates when the sampled grid becomes an f32 transform.
            let radius = bounds.min[axis].abs().max(bounds.max[axis].abs()) as f32;
            let representable = f64::from(radius.next_up() - radius) * 2.0;
            *step = step.max(representable).max(span / f64::from(u32::MAX - 1));
            ((span / *step).floor() as u32).saturating_add(1)
        });
        if steps == [0.0; 3] {
            return None;
        }
        let capacity = axes.into_iter().map(u128::from).product::<u128>();
        let grid_bits = |salt| {
            let lane = seed ^ salt;
            (u128::from(mix(lane)) << 64) | u128::from(mix(lane ^ 0x8b5a6df3))
        };
        let first = grid_bits(0) % capacity;
        let mut stride = (grid_bits(0xf9d07ea4) % capacity).max(1);
        while gcd(capacity, stride) != 1 {
            stride += 1;
        }
        Some(Self {
            steps,
            axes,
            capacity,
            first,
            stride,
        })
    }

    fn place(&self, index: usize, min: [f64; 3], position: &mut [f64; 3]) {
        // A coprime step visits each grid cell exactly once, without a table or retry loop.
        let mut cell = (self.first + index as u128 * self.stride) % self.capacity;
        for axis in 0..3 {
            let count = u128::from(self.axes[axis]);
            if self.steps[axis] > 0.0 {
                position[axis] = min[axis] + (cell % count) as f64 * self.steps[axis];
            }
            cell /= count;
        }
    }
}

fn gcd(mut a: u128, mut b: u128) -> u128 {
    while b != 0 {
        (a, b) = (b, a % b);
    }
    a
}

fn mix(mut bits: u64) -> u64 {
    bits = (bits ^ (bits >> 30)).wrapping_mul(0xbf58_476d_1ce4_e5b9);
    bits = (bits ^ (bits >> 27)).wrapping_mul(0x94d0_49bb_1331_11eb);
    bits ^ (bits >> 31)
}

fn fraction(seed: u64, index: usize, lane: u64) -> f64 {
    let stream = ((index as u64) << 32) | lane;
    (mix(seed
        .wrapping_add(stream)
        .wrapping_add(0x9e37_79b9_7f4a_7c15))
        >> 40) as f64
        / 16_777_216.0
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn color_models_keep_fixed_tints_and_sample_in_srgb_space() {
        let ink = Color(0xb04d8a88);
        for color_model in [
            InstanceColorModel::Rgb,
            InstanceColorModel::Hsv,
            InstanceColorModel::Hsl,
        ] {
            let table = crate::InstanceTable3D::from_random(SceneRandomInstances {
                color: [None, Some(ink)],
                color_model,
                count: 8,
                ..Default::default()
            });
            for row in 0..8 {
                assert_eq!(table.instance(row).color, ink);
            }
        }
        assert_eq!(
            InstanceColorModel::Hsv
                .srgb_tint([1.0 / 3.0, 1.0, 0.5, 0.25])
                .linear(),
            [0.0, linear_srgb_channel(0.5), 0.0, 0.25]
        );
        assert_eq!(
            InstanceColorModel::Hsl
                .srgb_tint([1.0 / 3.0, 1.0, 0.25, 0.25])
                .linear(),
            [0.0, linear_srgb_channel(0.5), 0.0, 0.25]
        );

        let fixed = RandomColor::new(&SceneRandomInstances::default());
        assert_eq!(fixed, RandomColor::Fixed(InstanceTint::Linear([1.0; 4])));
        let gradient = crate::InstanceTable3D::from_random(SceneRandomInstances {
            count: 64,
            color: [Some(Color(0xff000000)), Some(Color(0x00ff00ff))],
            proportional_color: true,
            ..Default::default()
        });
        for row in 0..gradient.count() {
            let [red, green, blue, alpha] = gradient.instance(row).color.0.to_be_bytes();
            assert!((254..=256).contains(&(u16::from(red) + u16::from(green))));
            assert_eq!((blue, green), (0, alpha));
        }
    }

    #[test]
    fn full_hue_ranges_and_grey_endpoints_preserve_geometry_and_data() {
        let positions = settings(10, 7).prepare(4096);
        for color_model in [InstanceColorModel::Hsv, InstanceColorModel::Hsl] {
            let spectrum = RandomColor::new(&SceneRandomInstances {
                color_model,
                color: [Some(Color(0xff000080)); 2],
                hue: [Some(0.0), Some(1.0)],
                ..Default::default()
            });
            let tints: Vec<_> = (0..64)
                .map(|row| spectrum.sample(7, row).linear())
                .collect();
            for channel in 0..3 {
                assert!(tints.iter().any(|tint| tint[channel] > 0.95));
            }
            assert!(tints.iter().all(|tint| tint[3] == 128.0_f32 / 255.0));

            let settings = SceneRandomInstances {
                color_model,
                color: [Some(Color(0x808080ff)), Some(Color(0x0000ffff))],
                ..settings(10, 7)
            }
            .prepare(4096);
            for row in 0..settings.len() {
                let before = positions.sample(row).expect("source instance");
                let after = settings.sample(row).expect("colored instance");
                assert_eq!(before.transform, after.transform);
                assert_eq!(before.data, after.data);
                let [r, g, b, _] = after.tint.linear();
                assert_eq!(r, g);
                assert!(b >= r);
            }
        }
    }

    fn settings(count: i64, seed: i64) -> SceneRandomInstances {
        SceneRandomInstances {
            count,
            seed,
            position: [
                Some(Vector3D::new(-4.0, 0.0, 0.0)),
                Some(Vector3D::new(4.0, 0.0, 4.0)),
            ],
            grid_spacing: Vector3D::new(2.0, 0.0, 2.0),
            scale: [
                Some(Vector3D::new(0.4, 0.4, 0.4)),
                Some(Vector3D::new(1.2, 1.2, 1.2)),
            ],
            proportional_scale: true,
            data: [Some([1.0, 0.0, -1.0, 0.5]), Some([5.0, 2.0, 0.0, 1.0])],
            ..Default::default()
        }
    }

    #[test]
    fn seeded_grid_keeps_the_prefix_without_repeated_cells() {
        let short = settings(5, 7).prepare(4096);
        let full = settings(80, 7).prepare(4096);
        let other = settings(80, 8).prepare(4096);
        assert_eq!(short.len(), 5);
        assert_eq!(full.len(), 15);
        let mut cells = std::collections::HashSet::new();
        for index in 0..full.len() {
            let sample = full.sample(index).expect("finite grid sample");
            let [x, y, z]: [f32; 3] = sample.transform.origin.into();
            assert!((-4.0..=4.0).contains(&x) && (0.0..=4.0).contains(&z));
            assert_eq!(y, 0.0);
            assert!(cells.insert([x.to_bits(), z.to_bits()]));
            let scale = sample.transform.linear.diagonal();
            assert_eq!(scale.x, scale.y);
            assert_eq!(scale.y, scale.z);
            if index < short.len() {
                let prefix = short.sample(index).expect("unchanged prefix");
                assert_eq!(sample.transform, prefix.transform);
                assert_eq!(sample.data, prefix.data);
            }
        }
        let a = full.sample(0).and_then(|sample| sample.data);
        let b = other.sample(0).and_then(|sample| sample.data);
        assert_ne!(a, b);
    }

    #[test]
    fn grid_preserves_distinct_f32_coordinates_at_tiny_spacing() {
        let instances = SceneRandomInstances {
            count: 4096,
            position: [
                Some(Vector3D::new(120.0, 0.0, 0.0)),
                Some(Vector3D::new(120.001, 0.0, 0.0)),
            ],
            grid_spacing: Vector3D::new(1.0e-12, 0.0, 0.0),
            ..Default::default()
        }
        .prepare(4096);
        let xs: std::collections::HashSet<_> = (0..instances.len())
            .map(|index| {
                instances
                    .transform(index)
                    .expect("representable grid")
                    .origin
                    .x
                    .to_bits()
            })
            .collect();
        assert_eq!(xs.len(), instances.len());
    }
}
