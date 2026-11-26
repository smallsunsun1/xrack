//! Decode Radiance environment images and prefilter them in linear light.
use crate::assets::read_asset;
use flate2::{Compression, write::ZlibEncoder};
use nalgebra::{Matrix3, RowVector3, Vector3};
use std::{
    collections::VecDeque,
    f32::consts::{PI, TAU},
    hash::{DefaultHasher, Hash, Hasher},
    io::{BufRead, Cursor, Read, Write},
    path::{Path, PathBuf},
    sync::{Arc, Mutex},
};
use xrack_runtime::scene3d::{
    EnvironmentProbe, EnvironmentSource, SkySettings, ViewportEnvironment,
};

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
        return Err("expected a Radiance .hdr environment".into());
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
        RadianceImage::decode(&bytes)?
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
/// data without having to read or prefilter an environment image at startup.
pub fn pack_environment(probe: &EnvironmentProbe) -> Result<Vec<u8>, String> {
    let mut encoder = ZlibEncoder::new(Vec::new(), Compression::fast());
    encoder
        .write_all(probe.texels())
        .map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())
}

struct RadianceImage {
    size: [usize; 2],
    layout: RadianceLayout,
    rgb: Vec<[f32; 3]>,
}

#[derive(Clone, Copy)]
enum RadianceLayout {
    Equirectangular,
    Cross(usize),
    Strip(usize),
}

impl RadianceLayout {
    fn face_size(self) -> Option<usize> {
        match self {
            Self::Cross(side) | Self::Strip(side) => Some(side),
            Self::Equirectangular => None,
        }
    }

    fn face_origin(self, face: u32) -> [usize; 2] {
        // +X, -X, +Y, -Y, +Z, -Z, with +Y above the +Z front of the cross.
        const CROSS: [[usize; 2]; 6] = [[2, 1], [0, 1], [1, 0], [1, 2], [1, 1], [3, 1]];
        match self {
            Self::Cross(side) => CROSS[face as usize].map(|coordinate| coordinate * side),
            Self::Strip(side) => [face as usize * side, 0],
            Self::Equirectangular => [0, 0],
        }
    }
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

impl RadianceImage {
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
        let layout = if 2 * height == width {
            RadianceLayout::Equirectangular
        } else if 6 * height == width {
            RadianceLayout::Strip(height)
        } else if width.is_multiple_of(4) && height == width / 4 * 3 {
            RadianceLayout::Cross(width / 4)
        } else {
            return Err("expected 2:1 HDR panorama, 6:1 cube strip, or 4:3 cube cross".into());
        };
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
        Ok(Self { size, layout, rgb })
    }

    fn sample(&self, direction: Vector3<f32>) -> [f32; 3] {
        if let Some(side) = self.layout.face_size() {
            return sample_cube(direction, side as i32, |face, x, y| {
                Vector3::from(self.face_texel(face, x as usize, y as usize))
            })
            .into();
        }
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

    fn face_texel(&self, face: u32, x: usize, y: usize) -> [f32; 3] {
        let [left, top] = self.layout.face_origin(face);
        self.rgb[(top + y) * self.size[0] + left + x]
    }

    fn diffuse(&self) -> ([[f32; 3]; 9], [[f32; 3]; 9]) {
        let mut hemispheres = [[[0.0_f64; 3]; 9]; 2];
        if let Some(side) = self.layout.face_size() {
            let coordinate = |i: usize| 2.0 * i as f32 / side as f32 - 1.0;
            let integral = |u: f32, v: f32| {
                let (u, v) = (f64::from(u), f64::from(v));
                (u * v).atan2((1.0 + u * u + v * v).sqrt())
            };
            for y in 0..side {
                let (v0, v1) = (coordinate(y), coordinate(y + 1));
                for x in 0..side {
                    let (u0, u1) = (coordinate(x), coordinate(x + 1));
                    let area =
                        integral(u1, v1) - integral(u0, v1) - integral(u1, v0) + integral(u0, v0);
                    let (u, v) = (0.5 * (u0 + u1), 0.5 * (v0 + v1));
                    for face in 0..6 {
                        let upper = match face {
                            2 => area,
                            3 => 0.0,
                            _ if v1 <= 0.0 => area,
                            _ if v0 >= 0.0 => 0.0,
                            _ => 0.5 * area,
                        };
                        accumulate_hemisphere(
                            &mut hemispheres,
                            cube_direction(face, u, v).into(),
                            self.face_texel(face, x, y),
                            [upper, area - upper],
                        );
                    }
                }
            }
            return diffuse_coefficients(hemispheres);
        }
        let [width, height] = self.size;
        let angles: Vec<_> = (0..width)
            .map(|x| (TAU * ((x as f32 + 0.5) / width as f32 - 0.5)).sin_cos())
            .collect();
        for y in 0..height {
            let theta = PI * (y as f32 + 0.5) / height as f32;
            let (ring, vertical) = theta.sin_cos();
            let edge = |y| (PI * y as f32 / height as f32).cos();
            let (top, bottom) = (edge(y), edge(y + 1));
            let area = f64::from(TAU / width as f32 * (top - bottom));
            let upper = f64::from(TAU / width as f32 * (top.max(0.0) - bottom.max(0.0)));
            for (x, &(azimuth_x, azimuth_z)) in angles.iter().enumerate() {
                let [x_dir, y_dir, z_dir] = [ring * azimuth_x, vertical, ring * azimuth_z];
                accumulate_hemisphere(
                    &mut hemispheres,
                    [x_dir, y_dir, z_dir],
                    self.rgb[y * width + x],
                    [upper, area - upper],
                );
            }
        }
        diffuse_coefficients(hemispheres)
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
        let (full, upper) = self.diffuse();
        EnvironmentProbe::new(full, upper, texels)
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

fn accumulate_hemisphere(
    hemispheres: &mut [[[f64; 3]; 9]; 2],
    [x, y, z]: [f32; 3],
    rgb: [f32; 3],
    areas: [f64; 2],
) {
    let basis = [
        0.282095,
        0.488603 * y,
        0.488603 * z,
        0.488603 * x,
        1.092548 * x * y,
        1.092548 * y * z,
        0.315392 * (3.0 * z * z - 1.0),
        1.092548 * x * z,
        0.546274 * (x * x - y * y),
    ];
    for (coefficients, area) in hemispheres.iter_mut().zip(areas) {
        if area > 0.0 {
            for (coefficient, basis) in coefficients.iter_mut().zip(basis) {
                let weight = f64::from(basis) * area;
                for (value, channel) in coefficient.iter_mut().zip(rgb) {
                    *value += f64::from(channel) * weight;
                }
            }
        }
    }
}

fn diffuse_coefficients([upper, lower]: [[[f64; 3]; 9]; 2]) -> ([[f32; 3]; 9], [[f32; 3]; 9]) {
    let full = std::array::from_fn(|i| {
        std::array::from_fn(|channel| upper[i][channel] + lower[i][channel])
    });
    (lambert_bands(full), lambert_bands(upper))
}

fn lambert_bands(coefficients: [[f64; 3]; 9]) -> [[f32; 3]; 9] {
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
    source: RadianceSource,
    orientation: Matrix3<f32>,
    horizon: f32,
    pub(crate) importance: Option<EnvironmentImportance>,
}

enum RadianceSource {
    Cubemap(Box<[Vector3<f32>]>),
    Sky(SkySettings, f32),
}

impl EnvironmentRadiance {
    const SIDE: i32 = EnvironmentProbe::FACE_SIZE as i32;
    const BASE_PIXELS: usize = (Self::SIDE * Self::SIDE * 6) as usize;

    pub(crate) fn new(input: ViewportEnvironment<'_>) -> Self {
        let orientation = Matrix3::from_rows(&input.orientation.rows().map(RowVector3::from));
        let (source, importance) = match input.source {
            EnvironmentSource::Panorama(probe) => {
                let faces: Box<[_]> = probe
                    .texels()
                    .as_chunks::<4>()
                    .0
                    .iter()
                    .take(Self::BASE_PIXELS)
                    .map(|bytes| {
                        let packed = u32::from_le_bytes(*bytes);
                        // RGB9E5 stores three mantissas with a shared, biased exponent.
                        let scale =
                            f32::from_bits(((packed >> 27) + (127 - 24)) << 23) * input.intensity;
                        Vector3::new(
                            (packed & 511) as f32,
                            ((packed >> 9) & 511) as f32,
                            ((packed >> 18) & 511) as f32,
                        ) * scale
                    })
                    .collect();
                let importance = EnvironmentImportance::new(&faces, orientation, input.horizon);
                (RadianceSource::Cubemap(faces), importance)
            }
            EnvironmentSource::Sky(sky) => {
                let mut faces = Vec::with_capacity(Self::BASE_PIXELS);
                let center = |pixel| (pixel as f32 + 0.5) * 2.0 / Self::SIDE as f32 - 1.0;
                for face in 0..6 {
                    for y in 0..Self::SIDE {
                        for x in 0..Self::SIDE {
                            let point = cube_direction(face, center(x), center(y));
                            faces.push(Vector3::from(sky.radiance(point.into())) * input.intensity);
                        }
                    }
                }
                (
                    RadianceSource::Sky(sky, input.intensity),
                    EnvironmentImportance::new(&faces, orientation, input.horizon),
                )
            }
        };
        Self {
            source,
            orientation,
            horizon: input.horizon,
            importance,
        }
    }

    pub(crate) fn sample(&self, direction: Vector3<f32>) -> Vector3<f32> {
        let rotated = self.orientation * direction;
        if !rotated.iter().all(|value| value.is_finite()) || rotated == Vector3::zeros() {
            return Vector3::zeros();
        }
        let weight = if rotated.y < 0.0 {
            1.0 - self.horizon
        } else {
            1.0
        };
        match &self.source {
            RadianceSource::Cubemap(faces) => {
                sample_cube(rotated, Self::SIDE, |face, column, row| {
                    faces[((face as i32 * Self::SIDE + row) * Self::SIDE + column) as usize]
                }) * weight
            }
            RadianceSource::Sky(sky, intensity) => {
                Vector3::from(sky.radiance(rotated.normalize().into())) * (*intensity * weight)
            }
        }
    }
}

fn sample_cube(
    direction: Vector3<f32>,
    side: i32,
    at: impl Fn(u32, i32, i32) -> Vector3<f32>,
) -> Vector3<f32> {
    let (face, [u, v]) = face_uv(direction);
    let texel = |coord| (coord + 1.0) * (side as f32 * 0.5) - 0.5;
    let (x, y): (f32, f32) = (texel(u), texel(v));
    let (column, row) = (x.floor() as i32, y.floor() as i32);
    let (fx, fy) = (x - column as f32, y - row as f32);
    let tap = |column, row| {
        if (0..side).contains(&column) && (0..side).contains(&row) {
            return at(face, column, row);
        }
        // Reproject edge taps onto the adjacent face.
        let center = |pixel: i32| (pixel as f32 + 0.5) * 2.0 / side as f32 - 1.0;
        let (other, [u, v]) = face_uv(cube_direction(face, center(column), center(row)));
        let index = |value: f32| (((value + 1.0) * 0.5 * side as f32) as i32).clamp(0, side - 1);
        at(other, index(u), index(v))
    };
    let top = tap(column, row).lerp(&tap(column + 1, row), fx);
    let bottom = tap(column, row + 1).lerp(&tap(column + 1, row + 1), fx);
    top.lerp(&bottom, fy)
}

/// A luminance-weighted distribution on the cubemap's texel footprints.
pub(crate) struct EnvironmentImportance {
    cumulative: Box<[f64]>,
    total: f64,
    orientation: Matrix3<f32>,
}

impl EnvironmentImportance {
    const SIDE: usize = EnvironmentProbe::FACE_SIZE as usize;
    const TEXEL: f32 = 2.0 / Self::SIDE as f32;

    fn new(faces: &[Vector3<f32>], orientation: Matrix3<f32>, horizon: f32) -> Option<Self> {
        let luminance = |index: usize, rgb: &Vector3<f32>| {
            let value = 0.2126 * rgb.x + 0.7152 * rgb.y + 0.0722 * rgb.z;
            if horizon == 0.0 {
                return value;
            }
            let row = index / Self::SIDE;
            let face = row / Self::SIDE;
            let below = match face {
                2 => false,
                3 => true,
                _ => row % Self::SIDE >= Self::SIDE / 2,
            };
            if below {
                value * (1.0 - horizon)
            } else {
                value
            }
        };
        let (total, peak) = faces
            .iter()
            .enumerate()
            .map(|(index, rgb)| luminance(index, rgb))
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
            total += f64::from(luminance(i, rgb)) / f64::from(inverse_cube_jacobian(u, v));
            cumulative.push(total);
        }
        Some(Self {
            cumulative: cumulative.into_boxed_slice(),
            total,
            orientation,
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
        let world = self.orientation.transpose() * cube_direction(face, u, v);
        (world, self.solid_angle_pdf(mass, u, v))
    }

    pub(crate) fn pdf(&self, world: Vector3<f32>) -> f32 {
        let (face, [u, v]) = face_uv(self.orientation * world);
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
    EnvironmentProbe::new([[0.0; 3]; 9], [[0.0; 3]; 9], texels)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn baking_uses_probe_orientation_and_masks_probe_local_ground() -> Result<(), String> {
        use xrack_runtime::{Node, RecordValue, Value};
        let mut node = Node::new("SceneView3D", None);
        node.properties.insert(
            "environment-sky",
            Value::Record(RecordValue::new(
                "Sky",
                [("sun-energy".into(), Value::Float(0.0))].into(),
            )),
        );
        node.properties
            .insert("environment-pitch", Value::Float(90.0));
        let full = ViewportEnvironment::from_node(&node).ok_or("missing procedural probe")?;
        let original = EnvironmentRadiance::new(full);
        let masked = EnvironmentRadiance::new(ViewportEnvironment {
            horizon: 1.0,
            ..full
        });
        assert!(original.sample(-Vector3::z()).norm() > 0.001);
        assert_eq!(masked.sample(-Vector3::z()), Vector3::zeros());
        assert_eq!(masked.sample(Vector3::z()), original.sample(Vector3::z()));
        assert!(masked.sample(Vector3::z()).norm() > 0.001);
        Ok(())
    }

    #[test]
    fn radiance_cube_atlases_sample_faces_and_integrate_solid_angle() -> Result<(), String> {
        let side = 16;
        for (layout, width, height) in [
            (RadianceLayout::Cross(side), 4 * side, 3 * side),
            (RadianceLayout::Strip(side), 6 * side, side),
        ] {
            let mut pixels = vec![[0_u8; 4]; width * height];
            let uv = |i| (i as f32 + 0.5) * 2.0 / side as f32 - 1.0;
            for face in 0..6 {
                let [left, top] = layout.face_origin(face);
                for y in 0..side {
                    for x in 0..side {
                        let rgb = cube_direction(face, uv(x), uv(y))
                            .map(|value| ((0.5 + 0.35 * value) * 256.0) as u8);
                        pixels[(top + y) * width + left + x] = [rgb.x, rgb.y, rgb.z, 128];
                    }
                }
            }
            let mut bytes =
                format!("#?RADIANCE\nFORMAT=32-bit_rle_rgbe\n\n-Y {height} +X {width}\n")
                    .into_bytes();
            bytes.extend(pixels.into_iter().flatten());
            let image = RadianceImage::decode(&bytes)?;
            for axis in [
                Vector3::x(),
                -Vector3::x(),
                Vector3::y(),
                -Vector3::y(),
                Vector3::z(),
                -Vector3::z(),
            ] {
                let actual = Vector3::from(image.sample(axis));
                let expected = Vector3::repeat(0.5) + axis * 0.35;
                assert!((actual - expected).norm() < 0.014, "{axis:?}: {actual:?}");
            }
            let left = image.sample(Vector3::new(1.0001, 0.12, 1.0).normalize());
            let right = image.sample(Vector3::new(1.0, 0.12, 1.0001).normalize());
            assert!((Vector3::from(left) - Vector3::from(right)).norm() < 0.006);
            let dc = image.diffuse().0[0];
            assert!(
                dc.into_iter()
                    .all(|value| (value * 0.282095 - 0.5).abs() < 0.004)
            );
        }
        Ok(())
    }

    #[test]
    fn procedural_sun_supplies_cpu_baking_and_importance_queries() -> Result<(), String> {
        use xrack_runtime::{Color, Node, RecordValue, Value};
        let mut node = Node::new("SceneView3D", None);
        node.properties.insert(
            "environment-sky",
            Value::Record(RecordValue::new(
                "Sky",
                [
                    ("sky-energy".into(), Value::Float(0.0)),
                    ("ground-energy".into(), Value::Float(0.0)),
                    ("sun-latitude".into(), Value::Float(0.0)),
                    ("sun-color".into(), Value::Color(Color(0xffffffff))),
                    ("sun-energy".into(), Value::Float(10.0)),
                ]
                .into(),
            )),
        );
        node.properties
            .insert("environment-yaw", Value::Float(90.0));
        node.properties
            .insert("environment-intensity", Value::Float(0.5));
        let input = ViewportEnvironment::from_node(&node).ok_or("missing declared sky")?;
        let radiance = EnvironmentRadiance::new(input);
        assert_eq!(radiance.sample(Vector3::x()), Vector3::repeat(5.0));
        assert_eq!(radiance.sample(Vector3::z()), Vector3::zeros());
        let importance = radiance
            .importance
            .as_ref()
            .ok_or("missing solar distribution")?;
        let (sampled, pdf) = importance.sample(0.27, 0.51);
        assert!(sampled.x > 0.99 && pdf > 0.0);
        assert!((importance.pdf(sampled) - pdf).abs() < pdf * 1e-4);
        Ok(())
    }

    #[test]
    fn cpu_environment_samples_all_faces_and_filters_across_the_seam() -> Result<(), String> {
        let probe = Arc::new(
            test_probe(|direction| ((direction + Vector3::repeat(1.0)) * 0.5).into())
                .ok_or("invalid synthetic probe")?,
        );
        let view = ViewportEnvironment {
            source: EnvironmentSource::Panorama(&probe),
            intensity: 0.5,
            orientation: Default::default(),
            horizon: 0.0,
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
            orientation: xrack_runtime::scene3d::EnvironmentOrientation::degrees([90.0, 0.0, 0.0]),
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
            source: EnvironmentSource::Panorama(&probe),
            intensity: 1.0,
            orientation: xrack_runtime::scene3d::EnvironmentOrientation::degrees([90.0, 0.0, 0.0]),
            horizon: 0.0,
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
