use std::f32::consts::TAU;

use nalgebra::Vector3;

use crate::{Color, RecordValue, Value, scene3d::lighting::linear_color};

/// The scene-linear environment shared by the GPU and the lightmap baker.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SkySettings {
    gradients: [[[f32; 3]; 2]; 2],
    curves: [f32; 2],
    sun_direction: [f32; 3],
    sun_radiance: [f32; 3],
    sun_cosines: [f32; 2],
    sun_curve: f32,
    resolution: u32,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn horizon_mask_clips_a_sun_straddling_the_equator() {
        let sky = SkySettings::from_record(&RecordValue::new(
            "Sky",
            [
                ("sky-energy".into(), Value::Float(0.0)),
                ("ground-energy".into(), Value::Float(0.0)),
                ("sun-latitude".into(), Value::Float(0.0)),
            ]
            .into(),
        ))
        .unwrap();
        let full = sky.diffuse(0.0);
        let upper = sky.diffuse(1.0);
        assert!((upper[0][0] / full[0][0] - 0.5).abs() < 0.01);
        assert!(full[1][0].abs() < 0.00001 && upper[1][0] > 0.0);
    }

    #[test]
    fn constant_sky_projects_only_the_dc_diffuse_band() {
        let mut fields = std::collections::BTreeMap::new();
        for field in [
            "sky-horizon-color",
            "sky-top-color",
            "ground-horizon-color",
            "ground-bottom-color",
        ] {
            fields.insert(field.into(), Value::Color(Color(0xffffffff)));
        }
        fields.insert("ground-energy".into(), Value::Float(1.0));
        fields.insert("sun-energy".into(), Value::Float(0.0));
        let sky = SkySettings::from_record(&RecordValue::new("Sky", fields)).unwrap();
        let sh = sky.diffuse(0.0);
        assert!((sh[0][0] * 0.282095 - 1.0).abs() < 0.00001);
        assert!(sh[1..].iter().flatten().all(|value| value.abs() < 0.00001));
        assert_eq!(sky.radiance([0.0, 0.0, 1.0]), [1.0; 3]);
    }
}

impl SkySettings {
    pub(crate) fn from_record(record: &RecordValue) -> Option<Self> {
        if matches!(record.field("enabled"), Some(Value::Bool(false))) {
            return None;
        }
        let number = |field, default: f64| match record.field(field) {
            Some(Value::Float(value)) if value.is_finite() => *value,
            _ => default,
        };
        let rgb = |field, default, energy: f32| {
            let color = match record.field(field) {
                Some(Value::Color(color)) => *color,
                _ => Color(default),
            };
            linear_color(color).map(|channel| channel * energy)
        };
        let sky = number("sky-energy", 1.0).clamp(0.0, 64.0) as f32;
        let ground = number("ground-energy", 0.6).clamp(0.0, 64.0) as f32;
        let latitude = number("sun-latitude", 25.0).clamp(-90.0, 90.0).to_radians() as f32;
        let longitude = number("sun-longitude", 0.0).rem_euclid(360.0).to_radians() as f32;
        let (elevation, ring) = latitude.sin_cos();
        let (x, z) = longitude.sin_cos();
        let outer = number("sun-angle-max", 6.0).clamp(0.1, 75.0);
        let inner = number("sun-angle-min", 2.0).clamp(0.0, outer);
        let resolution = match record.field("resolution") {
            Some(Value::Int(value)) => (*value).clamp(32, 256) as u32,
            _ => 128,
        };
        Some(Self {
            gradients: [
                [
                    rgb("sky-horizon-color", 0xc2d5eaff, sky),
                    rgb("sky-top-color", 0x38629eff, sky),
                ],
                [
                    rgb("ground-horizon-color", 0xa38e78ff, ground),
                    rgb("ground-bottom-color", 0x322827ff, ground),
                ],
            ],
            curves: [
                number("sky-curve", 0.8).clamp(0.05, 16.0) as f32,
                number("ground-curve", 0.9).clamp(0.05, 16.0) as f32,
            ],
            sun_direction: [x * ring, elevation, z * ring],
            sun_radiance: rgb(
                "sun-color",
                0xffd1a0ff,
                number("sun-energy", 18.0).clamp(0.0, 32_000.0) as f32,
            ),
            sun_cosines: [
                outer.to_radians().cos() as f32,
                inner.to_radians().cos() as f32,
            ],
            sun_curve: number("sun-curve", 1.0).clamp(0.05, 16.0) as f32,
            resolution: resolution.next_power_of_two(),
        })
    }

    pub fn resolution(self) -> u32 {
        self.resolution
    }

    #[doc(hidden)]
    pub fn uniforms(self) -> [[f32; 4]; 7] {
        let [
            [[sh_r, sh_g, sh_b], [st_r, st_g, st_b]],
            [[gh_r, gh_g, gh_b], [gb_r, gb_g, gb_b]],
        ] = self.gradients;
        let [sun_x, sun_y, sun_z] = self.sun_direction;
        let [sun_r, sun_g, sun_b] = self.sun_radiance;
        [
            [sh_r, sh_g, sh_b, self.curves[0]],
            [st_r, st_g, st_b, 0.0],
            [gh_r, gh_g, gh_b, self.curves[1]],
            [gb_r, gb_g, gb_b, 0.0],
            [sun_x, sun_y, sun_z, self.sun_cosines[0]],
            [sun_r, sun_g, sun_b, self.sun_cosines[1]],
            [self.sun_curve, 0.0, 0.0, 0.0],
        ]
    }

    /// Radiance along a unit direction in the unrotated environment.
    pub fn radiance(self, direction: [f32; 3]) -> [f32; 3] {
        let hemisphere = usize::from(direction[1] < 0.0);
        let [horizon, pole] = self.gradients[hemisphere];
        let factor = direction[1].abs().min(1.0).powf(self.curves[hemisphere]);
        let dot = direction
            .into_iter()
            .zip(self.sun_direction)
            .map(|(a, b)| a * b)
            .sum();
        let sunlight = self.sun_profile(dot);
        std::array::from_fn(|channel| {
            horizon[channel]
                + (pole[channel] - horizon[channel]) * factor
                + sunlight * self.sun_radiance[channel]
        })
    }

    fn sun_profile(self, cosine: f32) -> f32 {
        let [outer, inner] = self.sun_cosines;
        let t = ((cosine - outer) / (inner - outer).max(1e-7)).clamp(0.0, 1.0);
        (t * t * (3.0 - 2.0 * t)).powf(self.sun_curve)
    }

    /// Spherical harmonics convolved with the normalized Lambert kernel.
    pub fn diffuse(self, horizon_mask: f32) -> [[f32; 3]; 9] {
        let moment = |band: f32, channel: usize, hemisphere: usize| {
            let [horizon, pole] = self.gradients[hemisphere].map(|rgb| rgb[channel]);
            horizon / (band + 1.0) + (pole - horizon) / (band + 1.0 + self.curves[hemisphere])
        };
        let mut result = [[0.0; 3]; 9];
        for channel in 0..3 {
            let ground = 1.0 - horizon_mask;
            let m0 = moment(0.0, channel, 0) + ground * moment(0.0, channel, 1);
            let m1 = moment(1.0, channel, 0) - ground * moment(1.0, channel, 1);
            let m2 = moment(2.0, channel, 0) + ground * moment(2.0, channel, 1);
            let zonal = TAU * 0.25 * (0.5 * m0 - 1.5 * m2);
            result[0][channel] = TAU * 0.282095 * m0;
            result[1][channel] = TAU * (2.0 / 3.0) * 0.488603 * m1;
            result[6][channel] = 0.315392 * zonal;
            result[8][channel] = 0.546274 * zonal;
        }
        if self.sun_radiance != [0.0; 3] {
            let sun = self.sun_coefficients(horizon_mask);
            for (coefficient, weight) in result.iter_mut().zip(sun) {
                for (value, radiance) in coefficient.iter_mut().zip(self.sun_radiance) {
                    *value += radiance * weight;
                }
            }
        }
        result
    }

    fn sun_coefficients(self, horizon_mask: f32) -> [f32; 9] {
        const LAMBERT: [f32; 9] = [
            1.0,
            2.0 / 3.0,
            2.0 / 3.0,
            2.0 / 3.0,
            0.25,
            0.25,
            0.25,
            0.25,
            0.25,
        ];
        let edge = (1.0 - self.sun_cosines[0] * self.sun_cosines[0])
            .max(0.0)
            .sqrt();
        let y = self.sun_direction[1];
        if horizon_mask > 0.0 && y.abs() < edge {
            // A disk crossing the horizon loses axial symmetry. Integrate
            // in local disk coordinates only for this case.
            let axis = Vector3::from(self.sun_direction);
            let up = if axis.z.abs() > 0.99 {
                Vector3::x()
            } else {
                Vector3::z()
            };
            let x = up.cross(&axis).normalize();
            let y = axis.cross(&x);
            let width = (1.0 - self.sun_cosines[0]) / 32.0;
            let area = width * TAU / 32.0;
            let angles = std::array::from_fn::<_, 32, _>(|index| {
                (TAU * (index as f32 + 0.5) / 32.0).sin_cos()
            });
            let mut sum = [0.0; 9];
            for index in 0..32 {
                let t = self.sun_cosines[0] + (index as f32 + 0.5) * width;
                let radius = (1.0 - t * t).max(0.0).sqrt();
                let solar = area * self.sun_profile(t);
                for (sin, cos) in angles {
                    let direction = axis * t + x * (radius * cos) + y * (radius * sin);
                    let weight = if direction.y < 0.0 {
                        1.0 - horizon_mask
                    } else {
                        1.0
                    } * solar;
                    for (value, basis) in sum.iter_mut().zip(sh_basis(direction.into())) {
                        *value += weight * basis;
                    }
                }
            }
            return std::array::from_fn(|index| LAMBERT[index] * sum[index]);
        }
        let outer = self.sun_cosines[0];
        let width = (1.0 - outer) / 32.0;
        let mut integrals = [0.0; 3];
        for index in 0..32 {
            let t = outer + (index as f32 + 0.5) * width;
            let weight = self.sun_profile(t);
            for (integral, basis) in integrals
                .iter_mut()
                .zip([1.0, t, 0.5 * (3.0 * t * t - 1.0)])
            {
                *integral += weight * basis;
            }
        }
        let basis = sh_basis(self.sun_direction);
        let bands = [0, 1, 1, 1, 2, 2, 2, 2, 2];
        let width = if y < 0.0 {
            width * (1.0 - horizon_mask)
        } else {
            width
        };
        std::array::from_fn(|i| TAU * width * integrals[bands[i]] * LAMBERT[i] * basis[i])
    }
}

fn sh_basis([x, y, z]: [f32; 3]) -> [f32; 9] {
    [
        0.282095,
        0.488603 * y,
        0.488603 * z,
        0.488603 * x,
        1.092548 * x * y,
        1.092548 * y * z,
        0.315392 * (3.0 * z * z - 1.0),
        1.092548 * x * z,
        0.546274 * (x * x - y * y),
    ]
}
