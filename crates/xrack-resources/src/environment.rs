//! Decode Radiance panoramas and prefilter them in linear light.
use crate::assets::read_asset;
use flate2::{Compression, write::ZlibEncoder};
use nalgebra::Vector3;
use std::{
    collections::VecDeque,
    f32::consts::{PI, TAU},
    hash::{DefaultHasher, Hash, Hasher},
    io::{BufRead, Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use xrack_runtime::scene3d::{EnvironmentProbe, ViewportEnvironment};

const MAX_PIXELS: usize = 8192 * 1024;
const FILTER_SAMPLES: u32 = 64;
const MAX_CACHED_PROBES: usize = 4;
type ProbeCache = VecDeque<(PathBuf, u64, Arc<EnvironmentProbe>)>;

static CACHE: Mutex<ProbeCache> = Mutex::new(VecDeque::new());

pub(crate) fn load(path: &Path) -> Result<Arc<EnvironmentProbe>, String> {
    if !path
        .extension()
        .is_some_and(|extension| extension.eq_ignore_ascii_case("hdr"))
    {
        return Err("expected a Radiance .hdr panorama".into());
    }
    let bytes = read_asset(path, "environment probe")?;
    let mut fingerprint = DefaultHasher::new();
    bytes.hash(&mut fingerprint);
    let fingerprint = fingerprint.finish();
    {
        let mut cache = CACHE
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        if let Some(index) = cache
            .iter()
            .position(|(key, hash, _)| key == path && *hash == fingerprint)
            && let Some(entry) = cache.remove(index)
        {
            let probe = entry.2.clone();
            cache.push_front(entry);
            return Ok(probe);
        }
    }
    let probe = Arc::new(
        Panorama::decode(&bytes)?
            .prepare()
            .ok_or("invalid prepared environment")?,
    );
    let mut cache = CACHE
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    cache.retain(|(key, _, _)| key != path);
    cache.truncate(MAX_CACHED_PROBES - 1);
    cache.push_front((path.to_owned(), fingerprint, probe.clone()));
    Ok(probe)
}

/// Compress already prepared texels so generated binaries carry the lighting
/// data without having to read or prefilter a panorama at startup.
pub fn pack_environment(probe: &EnvironmentProbe) -> Result<Vec<u8>, String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(probe.texels())
        .map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())
}

struct Panorama {
    size: [usize; 2],
    rgb: Vec<[f32; 3]>,
}

#[derive(Clone, Copy)]
struct Axis {
    horizontal: bool,
    ascending: bool,
    len: usize,
}

impl Axis {
    fn parse(axis: &str, size: &str) -> Result<Self, String> {
        let (horizontal, ascending) = match axis {
            "+X" => (true, true),
            "-X" => (true, false),
            "-Y" => (false, true),
            "+Y" => (false, false),
            _ => return Err("invalid environment scanline orientation".into()),
        };
        let len = size
            .parse::<usize>()
            .map_err(|_| "invalid environment dimensions")?;
        Ok(Self {
            horizontal,
            ascending,
            len,
        })
    }

    fn position(self, index: usize) -> usize {
        if self.ascending {
            index
        } else {
            self.len - 1 - index
        }
    }
}

impl Panorama {
    fn decode(bytes: &[u8]) -> Result<Self, String> {
        let mut source = Cursor::new(bytes);
        let mut line = String::new();
        source
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        if !matches!(line.trim_end(), "#?RADIANCE" | "#?RGBE") {
            return Err("missing Radiance header".into());
        }
        let mut format = false;
        loop {
            line.clear();
            if source
                .read_line(&mut line)
                .map_err(|error| error.to_string())?
                == 0
            {
                return Err("truncated Radiance header".into());
            }
            let header = line.trim_end();
            if header.is_empty() {
                break;
            }
            if let Some(name) = header.strip_prefix("FORMAT=") {
                if name != "32-bit_rle_rgbe" {
                    return Err("expected RGBE Radiance pixels".into());
                }
                format = true;
            }
        }
        if !format {
            return Err("missing Radiance pixel format".into());
        }
        line.clear();
        source
            .read_line(&mut line)
            .map_err(|error| error.to_string())?;
        let tokens: Vec<_> = line.split_ascii_whitespace().collect();
        let [outer_axis, outer_len, inner_axis, inner_len] = tokens.as_slice() else {
            return Err("expected Radiance orientation and dimensions".into());
        };
        let outer = Axis::parse(outer_axis, outer_len)?;
        let inner = Axis::parse(inner_axis, inner_len)?;
        let count = outer
            .len
            .checked_mul(inner.len)
            .ok_or("environment is too large")?;
        if count == 0 || count > MAX_PIXELS || outer.horizontal == inner.horizontal {
            return Err("invalid environment dimensions or orientation (limit 8 Mi pixels)".into());
        }
        let size = if outer.horizontal {
            [outer.len, inner.len]
        } else {
            [inner.len, outer.len]
        };
        let [width, height] = size;
        if height.checked_mul(2) != Some(width) {
            return Err("expected a 2:1 equirectangular panorama".into());
        }
        let mut rgb = vec![[0.0; 3]; count];
        let mut scanline = vec![[0; 4]; inner.len];
        for i in 0..outer.len {
            scan(&mut source, &mut scanline)?;
            let major = outer.position(i);
            for (j, &[r, g, b, e]) in scanline.iter().enumerate() {
                let minor = inner.position(j);
                let [x, y] = if outer.horizontal {
                    [major, minor]
                } else {
                    [minor, major]
                };
                let scale = if e == 0 {
                    0.0
                } else {
                    2.0_f32.powi(i32::from(e) - 136)
                };
                rgb[y * width + x] =
                    [r, g, b].map(|channel| (f32::from(channel) * scale).min(65_408.0));
            }
        }
        Ok(Self { size, rgb })
    }

    fn sample(&self, direction: Vector3<f32>) -> [f32; 3] {
        let [width, height] = self.size;
        let u = (direction.x.atan2(direction.z) / TAU + 0.5) * width as f32 - 0.5;
        let v = (0.5 - direction.y.clamp(-1.0, 1.0).asin() / PI) * height as f32 - 0.5;
        let x = u.floor() as isize;
        let y = v.floor() as isize;
        let at = |x: isize, y: isize| {
            let x = x.rem_euclid(width as isize) as usize;
            let y = y.clamp(0, height as isize - 1) as usize;
            self.rgb[y * width + x]
        };
        let blend = |a: [f32; 3], b: [f32; 3], t| {
            std::array::from_fn(|channel| a[channel] + (b[channel] - a[channel]) * t)
        };
        let tx = u - u.floor();
        blend(
            blend(at(x, y), at(x + 1, y), tx),
            blend(at(x, y + 1), at(x + 1, y + 1), tx),
            v - v.floor(),
        )
    }

    fn diffuse(&self) -> [[f32; 3]; 9] {
        let [width, height] = self.size;
        let angles: Vec<_> = (0..width)
            .map(|x| (TAU * ((x as f32 + 0.5) / width as f32 - 0.5)).sin_cos())
            .collect();
        let mut coefficients = [[0.0_f64; 3]; 9];
        for y in 0..height {
            let theta = PI * (y as f32 + 0.5) / height as f32;
            let (ring, vertical) = theta.sin_cos();
            let edge = |y| (PI * y as f32 / height as f32).cos();
            let area = f64::from(TAU / width as f32 * (edge(y) - edge(y + 1)));
            for (x, &(azimuth_x, azimuth_z)) in angles.iter().enumerate() {
                let [x_dir, y_dir, z_dir] = [ring * azimuth_x, vertical, ring * azimuth_z];
                let basis = [
                    0.282095,
                    0.488603 * y_dir,
                    0.488603 * z_dir,
                    0.488603 * x_dir,
                    1.092548 * x_dir * y_dir,
                    1.092548 * y_dir * z_dir,
                    0.315392 * (3.0 * z_dir * z_dir - 1.0),
                    1.092548 * x_dir * z_dir,
                    0.546274 * (x_dir * x_dir - y_dir * y_dir),
                ];
                let pixel = &self.rgb[y * width + x];
                for (coefficient, basis) in coefficients.iter_mut().zip(basis) {
                    let weight = f64::from(basis) * area;
                    for (value, channel) in coefficient.iter_mut().zip(pixel) {
                        *value += f64::from(*channel) * weight;
                    }
                }
            }
        }
        // Lambert cosine kernel, normalized by pi, per SH band.
        let factors = [
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
        std::array::from_fn(|i| coefficients[i].map(|channel| (channel * factors[i]) as f32))
    }

    fn prepare(&self) -> Option<EnvironmentProbe> {
        let mut texels = Vec::with_capacity(EnvironmentProbe::BYTE_LEN);
        for level in 0..EnvironmentProbe::LEVELS {
            let size = EnvironmentProbe::FACE_SIZE >> level;
            let roughness = level as f32 / (EnvironmentProbe::LEVELS - 1) as f32;
            let samples = if level == 0 {
                Vec::new()
            } else {
                ggx_samples(roughness)
            };
            for face in 0..6 {
                for row in 0..size {
                    for column in 0..size {
                        let pixel_center = |index| (index as f32 + 0.5) * 2.0 / size as f32 - 1.0;
                        let n = cube_direction(face, pixel_center(column), pixel_center(row));
                        let rgb = if level == 0 {
                            self.sample(n)
                        } else {
                            self.filter(n, &samples)
                        };
                        texels.extend_from_slice(&pack_rgb9e5(rgb).to_le_bytes());
                    }
                }
            }
        }
        EnvironmentProbe::new(self.diffuse(), texels)
    }

    fn filter(&self, n: Vector3<f32>, samples: &[Vector3<f32>]) -> [f32; 3] {
        let up = if n.z.abs() < 0.99 {
            Vector3::z()
        } else {
            Vector3::x()
        };
        let tangent = up.cross(&n).normalize();
        let bitangent = n.cross(&tangent);
        let mut weighted = [0.0; 3];
        let mut total = 0.0;
        for l in samples {
            let pixel = self.sample(tangent * l.x + bitangent * l.y + n * l.z);
            for i in 0..3 {
                weighted[i] += pixel[i] * l.z;
            }
            total += l.z;
        }
        weighted.map(|channel| channel / total)
    }
}

fn ggx_samples(roughness: f32) -> Vec<Vector3<f32>> {
    let alpha_squared = roughness.powi(4);
    (0..FILTER_SAMPLES)
        .filter_map(|i| {
            let t = i.reverse_bits() as f32 / 4_294_967_296.0;
            let cos_theta = ((1.0 - t) / (1.0 + (alpha_squared - 1.0) * t)).sqrt();
            let sin_theta = (1.0 - cos_theta * cos_theta).max(0.0).sqrt();
            let (sin_phi, cos_phi) = (TAU * i as f32 / FILTER_SAMPLES as f32).sin_cos();
            let z = 2.0 * cos_theta * cos_theta - 1.0;
            (z > 0.0).then(|| {
                Vector3::new(
                    2.0 * cos_theta * sin_theta * cos_phi,
                    2.0 * cos_theta * sin_theta * sin_phi,
                    z,
                )
            })
        })
        .collect()
}

fn cube_direction(face: u32, x: f32, y: f32) -> Vector3<f32> {
    let direction = match face {
        0 => [1.0, -y, -x],
        1 => [-1.0, -y, x],
        2 => [x, 1.0, y],
        3 => [x, -1.0, -y],
        4 => [x, -y, 1.0],
        _ => [-x, -y, -1.0],
    };
    Vector3::from(direction).normalize()
}

/// Decoded base-level HDR radiance for repeated CPU ray queries.
pub(crate) struct EnvironmentRadiance {
    faces: Box<[Vector3<f32>]>,
    yaw: (f32, f32),
    pub(crate) importance: Option<EnvironmentImportance>,
}

impl EnvironmentRadiance {
    const SIDE: i32 = EnvironmentProbe::FACE_SIZE as i32;
    const BASE_PIXELS: usize = (Self::SIDE * Self::SIDE * 6) as usize;

    pub(crate) fn new(input: ViewportEnvironment<'_>) -> Self {
        let faces: Box<[_]> = input
            .probe
            .texels()
            .as_chunks::<4>()
            .0
            .iter()
            .take(Self::BASE_PIXELS)
            .map(|bytes| {
                let packed = u32::from_le_bytes(*bytes);
                // RGB9E5 stores three mantissas with a shared, biased exponent.
                let scale = f32::from_bits(((packed >> 27) + (127 - 24)) << 23) * input.intensity;
                Vector3::new(
                    (packed & 511) as f32,
                    ((packed >> 9) & 511) as f32,
                    ((packed >> 18) & 511) as f32,
                ) * scale
            })
            .collect();
        let yaw = input.yaw.sin_cos();
        let importance = EnvironmentImportance::new(&faces, yaw);
        Self {
            faces,
            yaw,
            importance,
        }
    }

    pub(crate) fn sample(&self, direction: Vector3<f32>) -> Vector3<f32> {
        let rotated = rotate_y(direction, self.yaw);
        if !rotated.iter().all(|value| value.is_finite()) || rotated == Vector3::zeros() {
            return Vector3::zeros();
        }
        let (face, [u, v]) = face_uv(rotated);
        let texel = |coord| (coord + 1.0) * (Self::SIDE as f32 * 0.5) - 0.5;
        let (x, y): (f32, f32) = (texel(u), texel(v));
        let (column, row) = (x.floor() as i32, y.floor() as i32);
        let (fx, fy) = (x - column as f32, y - row as f32);
        let top = self
            .tap(face, column, row)
            .lerp(&self.tap(face, column + 1, row), fx);
        let bottom = self
            .tap(face, column, row + 1)
            .lerp(&self.tap(face, column + 1, row + 1), fx);
        top.lerp(&bottom, fy)
    }

    fn tap(&self, face: u32, column: i32, row: i32) -> Vector3<f32> {
        let (face, column, row) =
            if (0..Self::SIDE).contains(&column) && (0..Self::SIDE).contains(&row) {
                (face, column, row)
            } else {
                // Reproject edge taps onto the neighboring face, as with seamless cubemap filtering.
                let center = |pixel: i32| (pixel as f32 + 0.5) * 2.0 / Self::SIDE as f32 - 1.0;
                let (other, [u, v]) = face_uv(cube_direction(face, center(column), center(row)));
                let index = |value: f32| {
                    (((value + 1.0) * 0.5 * Self::SIDE as f32) as i32).clamp(0, Self::SIDE - 1)
                };
                (other, index(u), index(v))
            };
        self.faces[((face as i32 * Self::SIDE + row) * Self::SIDE + column) as usize]
    }
}

/// A luminance-weighted distribution on the cubemap's texel footprints.
pub(crate) struct EnvironmentImportance {
    cumulative: Box<[f64]>,
    total: f64,
    yaw: (f32, f32),
}

impl EnvironmentImportance {
    const SIDE: usize = EnvironmentProbe::FACE_SIZE as usize;
    const TEXEL: f32 = 2.0 / Self::SIDE as f32;

    fn new(faces: &[Vector3<f32>], yaw: (f32, f32)) -> Option<Self> {
        let luminance = |rgb: &Vector3<f32>| 0.2126 * rgb.x + 0.7152 * rgb.y + 0.0722 * rgb.z;
        let (total, peak) = faces
            .iter()
            .map(luminance)
            .fold((0.0_f64, 0.0_f32), |(total, peak), value| {
                (total + f64::from(value), peak.max(value))
            });
        // A broad, nearly uniform environment is already well sampled by the cosine lobe.
        if !total.is_finite() || f64::from(peak) * faces.len() as f64 <= 4.0 * total {
            return None;
        }
        let mut cumulative = Vec::with_capacity(faces.len() + 1);
        cumulative.push(0.0);
        let mut total = 0.0;
        for (i, rgb) in faces.iter().enumerate() {
            let x = (i % Self::SIDE) as f32;
            let y = (i / Self::SIDE % Self::SIDE) as f32;
            let u = (x + 0.5) * Self::TEXEL - 1.0;
            let v = (y + 0.5) * Self::TEXEL - 1.0;
            total += f64::from(luminance(rgb)) / f64::from(inverse_cube_jacobian(u, v));
            cumulative.push(total);
        }
        Some(Self {
            cumulative: cumulative.into_boxed_slice(),
            total,
            yaw,
        })
    }

    pub(crate) fn sample(&self, selection: f32, vertical: f32) -> (Vector3<f32>, f32) {
        let target = f64::from(selection) * self.total;
        let index = self.cumulative.partition_point(|&bound| bound <= target) - 1;
        let mass = self.cumulative[index + 1] - self.cumulative[index];
        // The position inside the CDF interval supplies a uniform horizontal sub-texel sample.
        let horizontal = ((target - self.cumulative[index]) / mass) as f32;
        let x = (index % Self::SIDE) as f32;
        let y = (index / Self::SIDE % Self::SIDE) as f32;
        let u = (x + horizontal) * Self::TEXEL - 1.0;
        let v = (y + vertical) * Self::TEXEL - 1.0;
        let face = (index / (Self::SIDE * Self::SIDE)) as u32;
        let (sin, cos) = self.yaw;
        let world = rotate_y(cube_direction(face, u, v), (-sin, cos));
        (world, self.solid_angle_pdf(mass, u, v))
    }

    pub(crate) fn pdf(&self, world: Vector3<f32>) -> f32 {
        let (face, [u, v]) = face_uv(rotate_y(world, self.yaw));
        let pixel = |coord| (((coord + 1.0) / Self::TEXEL) as usize).min(Self::SIDE - 1);
        let index = (face as usize * Self::SIDE + pixel(v)) * Self::SIDE + pixel(u);
        let mass = self.cumulative[index + 1] - self.cumulative[index];
        self.solid_angle_pdf(mass, u, v)
    }

    fn solid_angle_pdf(&self, mass: f64, u: f32, v: f32) -> f32 {
        // Our conditional samples are uniform on the cube plane. Convert their density to sr^-1.
        (mass / self.total) as f32 * inverse_cube_jacobian(u, v) / (Self::TEXEL * Self::TEXEL)
    }
}

fn inverse_cube_jacobian(u: f32, v: f32) -> f32 {
    let length_squared = 1.0 + u * u + v * v;
    length_squared * length_squared.sqrt()
}

fn rotate_y(direction: Vector3<f32>, (sin, cos): (f32, f32)) -> Vector3<f32> {
    Vector3::new(
        cos * direction.x - sin * direction.z,
        direction.y,
        sin * direction.x + cos * direction.z,
    )
}

fn face_uv(direction: Vector3<f32>) -> (u32, [f32; 2]) {
    let [x, y, z]: [f32; 3] = direction.into();
    let [ax, ay, az] = [x.abs(), y.abs(), z.abs()];
    if ax >= ay.max(az) {
        let face = u32::from(x < 0.0);
        (face, [if face == 0 { -z / ax } else { z / ax }, -y / ax])
    } else if ay >= az {
        let face = 2 + u32::from(y < 0.0);
        (face, [x / ay, if face == 2 { z / ay } else { -z / ay }])
    } else {
        let face = 4 + u32::from(z < 0.0);
        (face, [if face == 4 { x / az } else { -x / az }, -y / az])
    }
}

#[cfg(test)]
pub(crate) fn test_probe(color: impl Fn(Vector3<f32>) -> [f32; 3]) -> Option<EnvironmentProbe> {
    let mut texels = Vec::with_capacity(EnvironmentProbe::BYTE_LEN);
    let side = EnvironmentProbe::FACE_SIZE;
    let center = |index: u32| (index as f32 + 0.5) * 2.0 / side as f32 - 1.0;
    for face in 0..6 {
        for row in 0..side {
            for column in 0..side {
                let rgb = color(cube_direction(face, center(column), center(row)));
                texels.extend_from_slice(&pack_rgb9e5(rgb).to_le_bytes());
            }
        }
    }
    texels.resize(EnvironmentProbe::BYTE_LEN, 0);
    EnvironmentProbe::new([[0.0; 3]; 9], texels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cpu_environment_samples_all_faces_and_filters_across_the_seam() -> Result<(), String> {
        let probe = Arc::new(
            test_probe(|direction| ((direction + Vector3::repeat(1.0)) * 0.5).into())
                .ok_or("invalid synthetic probe")?,
        );
        let view = ViewportEnvironment {
            probe: &probe,
            intensity: 0.5,
            yaw: 0.0,
            skybox: false,
        };
        let environment = EnvironmentRadiance::new(view);
        assert!(
            environment.importance.is_none(),
            "a low-contrast probe uses cosine sampling"
        );
        for axis in [
            Vector3::x(),
            -Vector3::x(),
            Vector3::y(),
            -Vector3::y(),
            Vector3::z(),
            -Vector3::z(),
        ] {
            let expected = (axis + Vector3::repeat(1.0)) * 0.25;
            assert!(
                (environment.sample(axis) - expected).norm() < 0.006,
                "{axis:?}"
            );
        }
        let a = environment.sample(Vector3::new(1.005, 0.1, 1.0));
        let b = environment.sample(Vector3::new(1.0, 0.1, 1.005));
        assert!((a - b).norm() < 0.006, "{a:?}, {b:?}");
        let rotated = EnvironmentRadiance::new(ViewportEnvironment {
            yaw: std::f32::consts::FRAC_PI_2,
            ..view
        });
        assert!((rotated.sample(Vector3::z()) - environment.sample(-Vector3::x())).norm() < 0.006);
        Ok(())
    }

    #[test]
    fn environment_pdf_matches_samples_after_rotation() -> Result<(), String> {
        let probe = Arc::new(
            test_probe(|ray| if ray.z > 0.99 { [32.0; 3] } else { [0.0; 3] })
                .ok_or("invalid synthetic probe")?,
        );
        let environment = EnvironmentRadiance::new(ViewportEnvironment {
            probe: &probe,
            intensity: 1.0,
            yaw: std::f32::consts::FRAC_PI_2,
            skybox: false,
        });
        let importance = environment
            .importance
            .as_ref()
            .ok_or("missing HDR distribution")?;
        assert_eq!(importance.pdf(-Vector3::x()), 0.0);
        for i in 0..256 {
            let (direction, density) = importance.sample(
                (i as f32 + 0.37) / 256.0,
                (((i * 17) % 256) as f32 + 0.61) / 256.0,
            );
            assert!(direction.x > 0.98);
            assert!(density > 0.0);
            assert!((density - importance.pdf(direction)).abs() < density * 0.0001);
        }
        Ok(())
    }
}

fn pack_rgb9e5(rgb: [f32; 3]) -> u32 {
    let maximum = rgb.into_iter().fold(2.0_f32.powi(-16), f32::max);
    let mut exponent = (maximum.log2().floor() as i32 + 16).max(0);
    let mut scale = 2.0_f32.powi(24 - exponent);
    if (maximum * scale).round() == 512.0 {
        exponent += 1;
        scale *= 0.5;
    }
    let [r, g, b] = rgb.map(|channel| (channel * scale).round() as u32);
    r | (g << 9) | (b << 18) | ((exponent as u32) << 27)
}

fn read<const N: usize>(source: &mut impl Read) -> Result<[u8; N], String> {
    let mut bytes = [0; N];
    source
        .read_exact(&mut bytes)
        .map_err(|_| "truncated Radiance pixels")?;
    Ok(bytes)
}

fn scan(source: &mut impl Read, output: &mut [[u8; 4]]) -> Result<(), String> {
    let first = read::<4>(source)?;
    if (8..=32767).contains(&output.len()) && first[0..2] == [2, 2] && first[2] & 128 == 0 {
        if usize::from(u16::from_be_bytes([first[2], first[3]])) != output.len() {
            return Err("incorrect Radiance scanline length".into());
        }
        for channel in 0..4 {
            let mut offset = 0;
            while offset < output.len() {
                let [code] = read(source)?;
                let run = code > 128;
                let count = usize::from(if run { code - 128 } else { code });
                if count == 0 || count > output.len() - offset {
                    return Err("invalid Radiance run length".into());
                }
                if run {
                    let [value] = read(source)?;
                    for pixel in &mut output[offset..offset + count] {
                        pixel[channel] = value;
                    }
                } else {
                    for pixel in &mut output[offset..offset + count] {
                        pixel[channel] = read::<1>(source)?[0];
                    }
                }
                offset += count;
            }
        }
        return Ok(());
    }

    let mut pixel = first;
    let mut offset = 0;
    let mut shift = 0;
    while offset < output.len() {
        if pixel[0..3] == [1, 1, 1] {
            if offset == 0 || shift > 24 {
                return Err("invalid legacy Radiance repeat".into());
            }
            let repeat = usize::from(pixel[3]) << shift;
            if repeat > output.len() - offset {
                return Err("legacy Radiance repeat exceeds scanline".into());
            }
            let previous = output[offset - 1];
            output[offset..offset + repeat].fill(previous);
            offset += repeat;
            shift += 8;
        } else {
            output[offset] = pixel;
            offset += 1;
            shift = 0;
        }
        if offset < output.len() {
            pixel = read(source)?;
        }
    }
    Ok(())
}
