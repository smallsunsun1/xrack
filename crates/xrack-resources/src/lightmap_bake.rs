//! Deterministic CPU baking for static meshes with authored lightmap charts.
use std::{
    collections::BTreeMap,
    f32::consts::{PI, TAU},
    thread,
};

use nalgebra::{Vector2, Vector3};
use thiserror::Error;
use xrack_runtime::scene3d::{
    BakedSurface, LightmapMode, LightmapTrace, MeshAlphaMode, Ray3D, SceneLight, SceneLightSource,
    SceneObject,
};

const PADDING: u32 = 2;
const MAX_DIMENSION: u32 = xrack_assets::MAX_IMAGE_DIMENSION;

#[derive(Clone, Copy, Debug)]
pub struct BakeOptions {
    pub size: u32,
    pub samples: u32,
    pub bounces: u8,
    pub bias: f32,
    pub mode: LightmapMode,
}

impl Default for BakeOptions {
    fn default() -> Self {
        Self {
            size: 64,
            samples: 32,
            bounces: 1,
            bias: 0.002,
            mode: LightmapMode::Diffuse,
        }
    }
}

#[derive(Debug, Error)]
pub enum BakeError {
    #[error("invalid bake settings: {0}")]
    Options(&'static str),
    #[error("no visible static PBR receivers with lightmap UVs")]
    NoReceivers,
    #[error("receiver `{0}` needs visible static PBR geometry with lightmap UVs")]
    MissingReceiver(String),
    #[error("receiver `{0}` contains overlapping lightmap charts at texel {1}, {2}")]
    Overlap(String, u32, u32),
    #[error("receiver `{0}` has no valid covered texels; check the lightmap UVs and bake size")]
    EmptyChart(String),
    #[error("receiver `{0}` needs --bake-size at least {1} for its generated lightmap charts")]
    ChartSize(String, u32),
    #[error("receiver `{0}` has lightmap UVs outside the 0..1 chart")]
    ChartRange(String),
    #[error("the lightmap atlas would exceed {MAX_DIMENSION} pixels per side")]
    AtlasSize,
    #[error("scene bounds or radiance exceed the supported lightmap range")]
    Radiance,
    #[error("lightmap PNG: {0}")]
    Png(#[from] png::EncodingError),
}

pub struct BakedAtlas {
    pub png: Vec<u8>,
    pub intensity: f32,
    pub mode: LightmapMode,
    pub regions: Vec<BakedRegion>,
}

pub struct BakedRegion {
    pub id: String,
    /// [u, v, width, height] inside the atlas, excluding padding.
    pub atlas: [f32; 4],
}

#[derive(Clone, Copy)]
struct Texel<'a> {
    object: usize,
    material: Option<&'a xrack_runtime::scene3d::MeshMaterial>,
    vertex: usize,
    weights: [f32; 3],
    radius: f32,
}

#[derive(Clone, Copy)]
struct ShadedTexel {
    surface: BakedSurface,
    direct: Vector3<f32>,
    indirect: Vector3<f32>,
}

pub fn bake_lightmaps(
    objects: &[SceneObject],
    lights: &[SceneLight],
    ambient: [f32; 3],
    visible_layers: u64,
    requested: &[String],
    options: BakeOptions,
) -> Result<BakedAtlas, BakeError> {
    if !(8..=512).contains(&options.size) {
        return Err(BakeError::Options("size must be between 8 and 512"));
    }
    if !(1..=512).contains(&options.samples) || options.bounces > 4 {
        return Err(BakeError::Options("use 1–512 samples and 0–4 bounces"));
    }
    if !options.bias.is_finite() || options.bias <= 0.0 {
        return Err(BakeError::Options(
            "bias must be a finite positive distance",
        ));
    }

    let mut receivers: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, object) in objects.iter().enumerate() {
        if object.id.is_empty()
            || object.layers & visible_layers == 0
            || object.mesh.influences().is_some()
            || object.mesh.morph_geometry().is_some()
            || object.mesh.lightmap_uvs().is_empty()
            || !requested.is_empty() && !requested.contains(&object.id)
            || !object
                .mesh
                .sections()
                .any(|(_, material)| receiver_material(material))
        {
            continue;
        }
        receivers.entry(&object.id).or_default().push(index);
    }
    for id in requested {
        if !receivers.contains_key(id.as_str()) {
            return Err(BakeError::MissingReceiver(id.clone()));
        }
    }
    if receivers.is_empty() {
        return Err(BakeError::NoReceivers);
    }

    let tiles = receivers.len() as u32;
    let columns = (tiles as f32).sqrt().ceil() as u32;
    let rows = tiles.div_ceil(columns);
    let side = options.size + 2 * PADDING;
    let [width, height] = [columns * side, rows * side];
    if width > MAX_DIMENSION || height > MAX_DIMENSION {
        return Err(BakeError::AtlasSize);
    }
    let distance = scene_extent(objects, visible_layers).ok_or(BakeError::Radiance)?;
    let trace = LightmapTrace::new(objects);
    let lighting = BakeLighting {
        trace: &trace,
        lights,
        ambient: Vector3::from(ambient) * 0.22,
        distance,
        visible_layers,
        options,
    };
    let mut atlas = vec![[0.0; 3]; (width * height) as usize];
    let mut regions = Vec::with_capacity(receivers.len());
    for (tile, (id, instances)) in receivers.into_iter().enumerate() {
        let min_size = instances
            .iter()
            .map(|&index| objects[index].mesh.lightmap_min_size())
            .max()
            .unwrap_or(0);
        if options.size < min_size {
            return Err(BakeError::ChartSize(id.into(), min_size));
        }
        let x = tile as u32 % columns * side;
        let y = tile as u32 / columns * side;
        let texels = rasterize(objects, id, &instances, side, options.size)?;
        let mut shaded = vec![None; texels.len()];
        shade_tile(objects, &lighting, &texels, &mut shaded, tile as u32, side);
        if shaded.iter().all(Option::is_none) {
            return Err(BakeError::EmptyChart(id.into()));
        }
        let mut filtered = filter_indirect(&texels, &shaded, side);
        pad_chart(&mut filtered, side);
        for (row, pixels) in filtered.chunks_exact(side as usize).enumerate() {
            let start = ((y + row as u32) * width + x) as usize;
            for (out, pixel) in atlas[start..start + side as usize].iter_mut().zip(pixels) {
                *out = pixel.unwrap_or([0.0; 3]);
            }
        }
        regions.push(BakedRegion {
            id: id.into(),
            atlas: [
                (x + PADDING) as f32 / width as f32,
                (y + PADDING) as f32 / height as f32,
                options.size as f32 / width as f32,
                options.size as f32 / height as f32,
            ],
        });
    }
    let mut intensity = 1.0_f32;
    for &channel in atlas.iter().flatten() {
        if !channel.is_finite() || channel > 1024.0 {
            return Err(BakeError::Radiance);
        }
        intensity = intensity.max(channel);
    }
    intensity = intensity.ceil();
    let bytes: Vec<_> = atlas
        .iter()
        .flat_map(|pixel| pixel.map(|channel| encode_srgb(channel / intensity)))
        .collect();
    let mut png = Vec::new();
    let mut encoder = png::Encoder::new(&mut png, width, height);
    encoder.set_color(png::ColorType::Rgb);
    encoder.set_depth(png::BitDepth::Eight);
    encoder.write_header()?.write_image_data(&bytes)?;
    Ok(BakedAtlas {
        png,
        intensity,
        mode: options.mode,
        regions,
    })
}

fn receiver_material(material: Option<&xrack_runtime::scene3d::MeshMaterial>) -> bool {
    material.is_some_and(|material| {
        material.alpha_mode != MeshAlphaMode::Blend
            && material
                .pbr
                .as_ref()
                .is_some_and(|pbr| pbr.transmission_blur().is_none())
    })
}

fn scene_extent(objects: &[SceneObject], layers: u64) -> Option<f32> {
    let (min, max) = objects
        .iter()
        .filter(|object| object.layers & layers != 0)
        .flat_map(SceneObject::bounds_corners)
        .fold(
            (
                Vector3::repeat(f32::INFINITY),
                Vector3::repeat(f32::NEG_INFINITY),
            ),
            |(min, max), point| {
                let point = Vector3::from(point);
                (min.inf(&point), max.sup(&point))
            },
        );
    let distance = (max - min).norm() * 1.01;
    (distance > 0.0 && distance.is_finite()).then_some(distance)
}

fn rasterize<'a>(
    objects: &'a [SceneObject],
    id: &str,
    instances: &[usize],
    side: u32,
    inner: u32,
) -> Result<Vec<Option<Texel<'a>>>, BakeError> {
    let mut output: Vec<Option<Texel<'_>>> = vec![None; (side * side) as usize];
    for &index in instances {
        let mesh = &objects[index].mesh;
        let chart = mesh.lightmap_uvs();
        for (range, material) in mesh
            .sections()
            .filter(|(_, material)| receiver_material(*material))
        {
            for vertex in range.step_by(3) {
                let uv = [chart[vertex], chart[vertex + 1], chart[vertex + 2]];
                if uv == [[-1.0; 2]; 3] {
                    continue;
                }
                if !uv.iter().flatten().all(|value| (0.0..=1.0).contains(value)) {
                    return Err(BakeError::ChartRange(id.into()));
                }
                let [a, b, c] =
                    uv.map(|uv| Vector2::from(uv) * inner as f32 + Vector2::repeat(PADDING as f32));
                let ab = b - a;
                let ac = c - a;
                let area = ab.perp(&ac);
                if area.abs() <= 1e-8 {
                    continue;
                }
                let transform = objects[index].transform_rows();
                let world = |vertex: usize| {
                    let [x, y, z] = mesh.vertices()[vertex].position;
                    Vector3::from(transform.map(|[a, b, c, d]| a * x + b * y + c * z + d))
                };
                let world_a = world(vertex);
                let edge_b = world(vertex + 1) - world_a;
                let edge_c = world(vertex + 2) - world_a;
                let dx = (edge_b * ac.y - edge_c * ab.y) / area;
                let dy = (edge_c * ab.x - edge_b * ac.x) / area;
                let radius = 2.5 * dx.norm().max(dy.norm()).max(1e-5);
                let low = a.inf(&b).inf(&c).map(|value| value.floor() as u32);
                let high = a
                    .sup(&b)
                    .sup(&c)
                    .map(|value| (value.ceil() as u32).min(side));
                for y in low.y..high.y {
                    for x in low.x..high.x {
                        let position = Vector2::new(x as f32 + 0.5, y as f32 + 0.5) - a;
                        let u = position.perp(&ac) / area;
                        let v = ab.perp(&position) / area;
                        let w = 1.0 - u - v;
                        if u.min(v).min(w) < -1e-6 {
                            continue;
                        }
                        let texel = Texel {
                            object: index,
                            material,
                            vertex,
                            weights: [w, u, v],
                            radius,
                        };
                        let entry = &mut output[(y * side + x) as usize];
                        if let Some(old) = entry {
                            let interior = |texel: Texel<'_>| {
                                texel.weights.into_iter().all(|weight| weight > 1e-5)
                            };
                            if interior(*old) && interior(texel) {
                                return Err(BakeError::Overlap(
                                    id.into(),
                                    x - PADDING,
                                    y - PADDING,
                                ));
                            }
                            if interior(*old) {
                                continue;
                            }
                        }
                        *entry = Some(texel);
                    }
                }
            }
        }
    }
    Ok(output)
}

fn filter_indirect(
    texels: &[Option<Texel<'_>>],
    shaded: &[Option<ShadedTexel>],
    side: u32,
) -> Vec<Option<[f32; 3]>> {
    let side = side as usize;
    shaded
        .iter()
        .zip(texels)
        .enumerate()
        .map(|(index, (value, texel))| {
            let (value, texel) = value.as_ref().zip(texel.as_ref())?;
            let (x, y) = (index % side, index / side);
            let origin = Vector3::from(value.surface.position);
            let normal = Vector3::from(value.surface.normal);
            let mut total = Vector3::zeros();
            let mut weights = 0.0;
            for row in y.saturating_sub(1)..=(y + 1).min(side - 1) {
                for column in x.saturating_sub(1)..=(x + 1).min(side - 1) {
                    let other = row * side + column;
                    let Some((other, neighbor)) =
                        shaded[other].as_ref().zip(texels[other].as_ref())
                    else {
                        continue;
                    };
                    let facing = normal.dot(&Vector3::from(other.surface.normal));
                    if neighbor.object != texel.object || facing < 0.85 {
                        continue;
                    }
                    let distance = (Vector3::from(other.surface.position) - origin).norm();
                    let similarity = (1.0 - distance / texel.radius).max(0.0);
                    let kernel = ((2 - x.abs_diff(column)) * (2 - y.abs_diff(row))) as f32;
                    let weight = kernel * similarity * facing.max(0.0);
                    total += other.indirect * weight;
                    weights += weight;
                }
            }
            Some((value.direct + total / weights.max(1e-6)).into())
        })
        .collect()
}

fn pad_chart(pixels: &mut [Option<[f32; 3]>], side: u32) {
    let side = side as usize;
    for _ in 0..PADDING {
        let source = pixels.to_vec();
        for (index, pixel) in pixels.iter_mut().enumerate() {
            if pixel.is_some() {
                continue;
            }
            let (x, y) = (index % side, index / side);
            let neighbors = [
                (x > 0).then(|| index - 1),
                (x + 1 < side).then(|| index + 1),
                (y > 0).then(|| index - side),
                (y + 1 < side).then(|| index + side),
            ];
            *pixel = neighbors
                .into_iter()
                .flatten()
                .find_map(|index| source[index]);
        }
    }
}

struct BakeLighting<'a> {
    trace: &'a LightmapTrace<'a>,
    lights: &'a [SceneLight],
    ambient: Vector3<f32>,
    distance: f32,
    visible_layers: u64,
    options: BakeOptions,
}

fn shade_tile(
    objects: &[SceneObject],
    lighting: &BakeLighting<'_>,
    texels: &[Option<Texel<'_>>],
    shaded: &mut [Option<ShadedTexel>],
    tile: u32,
    side: u32,
) {
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(side as usize);
    let chunk = side as usize * (side as usize).div_ceil(workers);
    thread::scope(|scope| {
        for (row, (shaded, texels)) in shaded
            .chunks_mut(chunk)
            .zip(texels.chunks(chunk))
            .enumerate()
        {
            scope.spawn(move || {
                for (index, (out, texel)) in shaded.iter_mut().zip(texels).enumerate() {
                    let Some(texel) = texel else {
                        continue;
                    };
                    let object = &objects[texel.object];
                    let seed = hash(tile.wrapping_mul(0xc2b2ae35) ^ (row * chunk + index) as u32);
                    *out = object
                        .bake_sample(texel.vertex, texel.weights, texel.material, false)
                        .map(|surface| lighting.irradiance(surface, seed));
                }
            });
        }
    });
}

impl BakeLighting<'_> {
    fn irradiance(&self, surface: BakedSurface, seed: u32) -> ShadedTexel {
        let mut indirect = Vector3::zeros();
        let layers = surface.layers & self.visible_layers;
        for sample in 0..self.options.samples {
            let mut at = surface;
            let mut throughput: Vector3<f32> = Vector3::repeat(1.0);
            let mut seed = hash(seed ^ sample.wrapping_mul(0x9e3779b9));
            for bounce in 0..=self.options.bounces {
                let normal = Vector3::from(at.normal);
                let direction = hemisphere(normal, &mut seed);
                let origin = Vector3::from(at.position) + normal * self.options.bias;
                let hit = Ray3D::new(origin.into(), direction.into())
                    .and_then(|ray| self.trace.trace(ray, self.distance, layers));
                let Some(hit) = hit else {
                    indirect += throughput.component_mul(&self.ambient);
                    break;
                };
                if bounce == self.options.bounces {
                    break;
                }
                let diffuse = Vector3::from(hit.diffuse);
                let radiance =
                    Vector3::from(hit.emission) + diffuse.component_mul(&self.direct(hit));
                indirect += radiance.component_mul(&throughput);
                throughput.component_mul_assign(&diffuse);
                if throughput.max() < 0.0001 {
                    break;
                }
                at = hit;
            }
        }
        let direct = if self.options.mode == LightmapMode::Diffuse {
            self.direct(surface)
        } else {
            Vector3::zeros()
        };
        ShadedTexel {
            surface,
            direct,
            indirect: indirect / self.options.samples as f32,
        }
    }

    fn direct(&self, surface: BakedSurface) -> Vector3<f32> {
        let position = Vector3::from(surface.position);
        let normal = Vector3::from(surface.normal);
        let mut result = Vector3::zeros();
        for light in self.lights {
            let layers = surface.layers & light.layers & self.visible_layers;
            if layers == 0 {
                continue;
            }
            let Some((direction, distance, intensity)) =
                incident(light.source, position, self.distance)
            else {
                continue;
            };
            let cosine = normal.dot(&direction).max(0.0);
            if cosine == 0.0 {
                continue;
            }
            let origin = position + normal * self.options.bias;
            if light.casts_shadow
                && surface.receives_shadow
                && Ray3D::new(origin.into(), direction.into()).is_some_and(|ray| {
                    self.trace
                        .occluded(ray, (distance - self.options.bias * 2.0).max(0.0), layers)
                })
            {
                continue;
            }
            let radiance =
                Vector3::from(light.radiance).map(|channel| (channel * intensity).min(1e6));
            result += radiance * (cosine / PI);
        }
        result
    }
}

fn incident(
    source: SceneLightSource,
    target: Vector3<f32>,
    extent: f32,
) -> Option<(Vector3<f32>, f32, f32)> {
    match source {
        SceneLightSource::Directional { toward_light } => Some((toward_light.into(), extent, 1.0)),
        SceneLightSource::Point {
            position,
            inverse_range_squared,
        } => punctual(position, target, inverse_range_squared),
        SceneLightSource::Spot {
            position,
            inverse_range_squared,
            toward_light,
            cone_scale,
            cone_offset,
            ..
        } => {
            let (direction, distance, intensity) =
                punctual(position, target, inverse_range_squared)?;
            let cone = (direction.dot(&Vector3::from(toward_light)) * cone_scale + cone_offset)
                .clamp(0.0, 1.0);
            (cone > 0.0).then_some((direction, distance, intensity * cone * cone))
        }
    }
}

fn punctual(
    position: [f32; 3],
    target: Vector3<f32>,
    inverse_range: f32,
) -> Option<(Vector3<f32>, f32, f32)> {
    let delta = Vector3::from(position) - target;
    let length_squared = delta.norm_squared();
    let fraction = length_squared * inverse_range;
    if length_squared <= 1e-6 || fraction >= 1.0 || !length_squared.is_finite() {
        return None;
    }
    let length = length_squared.sqrt();
    Some((
        delta / length,
        length,
        (1.0 - fraction * fraction) / length_squared.max(0.0001),
    ))
}

fn hemisphere(normal: Vector3<f32>, seed: &mut u32) -> Vector3<f32> {
    let anchor = if normal.z.abs() > 0.9 {
        Vector3::y()
    } else {
        Vector3::z()
    };
    let tangent = anchor.cross(&normal).normalize();
    let bitangent = normal.cross(&tangent);
    let radius_squared = random(seed);
    let (s, c) = (TAU * random(seed)).sin_cos();
    let radius = radius_squared.sqrt();
    (tangent * c + bitangent * s) * radius + normal * (1.0 - radius_squared).sqrt()
}

fn hash(mut value: u32) -> u32 {
    value = (value ^ (value >> 16)).wrapping_mul(0x7feb352d);
    value = (value ^ (value >> 15)).wrapping_mul(0x846ca68b);
    value ^ (value >> 16)
}

fn random(seed: &mut u32) -> f32 {
    *seed = hash(seed.wrapping_add(0x9e3779b9));
    (*seed >> 8) as f32 / 16_777_216.0
}

fn encode_srgb(value: f32) -> u8 {
    let linear = value.clamp(0.0, 1.0);
    let srgb = if linear <= 0.0031308 {
        linear * 12.92
    } else {
        1.055 * linear.powf(1.0 / 2.4) - 0.055
    };
    (srgb * 255.0).round() as u8
}

#[cfg(test)]
mod tests {
    use super::*;
    use xrack_runtime::{
        Color,
        scene3d::{Mesh3D, MeshMaterial, MeshPart, MeshPbr, MeshVertex},
    };

    type TestResult = Result<(), Box<dyn std::error::Error>>;

    fn plate(id: &str, z: f64, emitter: bool) -> Result<SceneObject, String> {
        let positions = [
            [-0.5, -0.5, 0.0],
            [0.5, -0.5, 0.0],
            [0.5, 0.5, 0.0],
            [-0.5, 0.5, 0.0],
        ];
        let indices = [0, 1, 2, 0, 2, 3];
        let vertices = indices
            .iter()
            .map(|&index| MeshVertex {
                position: positions[index],
                normal: [0.0, 0.0, 1.0],
                uv: [0.0; 2],
                uv1: [0.0; 2],
                color: [1.0; 4],
            })
            .collect();
        let chart = [[0.0, 1.0], [1.0, 1.0], [1.0, 0.0], [0.0, 0.0]];
        let material = MeshMaterial {
            base_color: if emitter {
                [1.0, 0.0, 0.0, 1.0]
            } else {
                [1.0; 4]
            },
            base_texture: None,
            alpha_mode: MeshAlphaMode::Opaque,
            double_sided: false,
            pbr: (!emitter).then_some(MeshPbr {
                metallic: 0.0,
                roughness: 1.0,
                emissive: [0.0; 3],
                occlusion_strength: 1.0,
                metallic_roughness_texture: None,
                emissive_texture: None,
                occlusion_texture: None,
                normal_map: None,
                clearcoat: None,
                dielectric: None,
                transmission: None,
            }),
        };
        let mesh = Mesh3D::imported(
            vertices,
            Vec::new(),
            vec![MeshPart {
                vertices: 0..6,
                material: 0,
                volume_inverse: None,
            }],
            vec![material],
        )?
        .with_lightmap_uv(indices.map(|index| chart[index]).to_vec())
        .ok_or("invalid chart")?;
        let angles = [if emitter { 180.0 } else { 0.0 }, 0.0, 0.0];
        let scale = if emitter { [4.0, 4.0, 1.0] } else { [1.0; 3] };
        SceneObject::from_mesh(
            id.into(),
            mesh,
            [0.0, 0.0, z],
            scale,
            angles,
            Color(0xffffffff),
        )
        .ok_or_else(|| "invalid receiver".into())
    }

    #[test]
    fn direct_bake_keeps_shadows_from_unpickable_geometry_and_is_deterministic() -> TestResult {
        let panel = plate("receiver", 0.0, false)?;
        let occluder = SceneObject::cuboid(
            "",
            [0.0, 0.0, 0.4],
            [0.45, 0.45, 0.1],
            [0.0; 3],
            Color(0xffffffff),
        )
        .ok_or("invalid occluder")?
        .with_pickable(false);
        let light = SceneLight {
            source: SceneLightSource::Directional {
                toward_light: [0.0, 0.0, 1.0],
            },
            radiance: [PI; 3],
            casts_shadow: true,
            layers: 1,
        };
        let objects = [panel, occluder];
        let settings = BakeOptions {
            size: 16,
            samples: 2,
            bounces: 0,
            ..BakeOptions::default()
        };
        let atlas = bake_lightmaps(&objects, &[light], [0.0; 3], 1, &[], settings)?;
        let repeated = bake_lightmaps(&objects, &[light], [0.0; 3], 1, &[], settings)?;
        assert_eq!(atlas.png, repeated.png);
        assert_eq!(atlas.regions[0].id, "receiver");
        let image = xrack_assets::decode_png(&atlas.png)?;
        assert_eq!(image.size, [20, 20]);
        let red = |x: usize, y: usize| image.rgba[(y * 20 + x) * 4];
        assert!(red(4, 4) > 240 && red(10, 10) < 10);
        assert_eq!(red(1, 4), red(2, 4), "the atlas pads the chart border");
        Ok(())
    }

    #[test]
    fn indirect_bake_samples_emitting_surfaces_and_rejects_overlapping_receivers() -> TestResult {
        let panel = plate("receiver", 0.0, false)?;
        let emitter = plate("red", 0.8, true)?;
        let options = BakeOptions {
            size: 16,
            samples: 24,
            bounces: 0,
            mode: LightmapMode::Indirect,
            ..BakeOptions::default()
        };
        let objects = [panel.clone(), emitter];
        let first = bake_lightmaps(&objects, &[], [0.0; 3], 1, &[], options)?;
        let bounce = bake_lightmaps(
            &objects,
            &[],
            [0.0; 3],
            1,
            &[],
            BakeOptions {
                bounces: 1,
                ..options
            },
        )?;
        let rgb = |atlas: &BakedAtlas| -> Result<[u8; 3], String> {
            let image = xrack_assets::decode_png(&atlas.png)?;
            let offset = (10 * 20 + 10) * 4;
            Ok(std::array::from_fn(|i| image.rgba[offset + i]))
        };
        assert_eq!(rgb(&first)?, [0; 3]);
        let red = rgb(&bounce)?;
        assert!(red[0] > 100 && red[1..] == [0, 0], "{red:?}");
        assert!(matches!(
            bake_lightmaps(&[panel.clone(), panel], &[], [0.0; 3], 1, &[], options),
            Err(BakeError::Overlap(_, _, _))
        ));
        Ok(())
    }
}
