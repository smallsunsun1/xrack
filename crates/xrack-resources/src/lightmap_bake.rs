//! Deterministic CPU baking for static meshes with authored lightmap charts.
use std::{
    borrow::Cow,
    cmp::Reverse,
    collections::{BTreeMap, BTreeSet},
    f32::consts::{PI, TAU},
    io::{self, Write},
    thread,
};

use nalgebra::{Vector2, Vector3};
use thiserror::Error;
use xrack_runtime::scene3d::{
    BakeRole, BakedSurface, LightBakeMode, LightmapMode, LightmapResolution, LightmapTrace,
    MeshMaterial, Ray3D, SceneLight, SceneLightSource, SceneObject, ViewportEnvironment,
};

pub use crate::lightmap_samples::replay_lightmap_samples;
use crate::{
    environment::{EnvironmentImportance, EnvironmentRadiance},
    lightmap_samples,
};

pub(crate) const PADDING: u32 = 2;
pub(crate) const MAX_DIMENSION: u32 = xrack_assets::MAX_IMAGE_DIMENSION;
pub(crate) const MIN_RESOLUTION: u32 = 8;
pub(crate) const MAX_RESOLUTION: u32 = 512;

#[derive(Clone, Copy, Debug)]
pub struct BakeOptions {
    pub size: u32,
    pub texels_per_unit: f64,
    pub samples: u32,
    pub bounces: u8,
    pub bias: f32,
    pub mode: LightmapMode,
    pub denoise_passes: u8,
    pub indirect_factor: f32,
}

impl Default for BakeOptions {
    fn default() -> Self {
        Self {
            size: 64,
            texels_per_unit: 0.0,
            samples: 32,
            bounces: 1,
            bias: 0.002,
            mode: LightmapMode::Diffuse,
            denoise_passes: 1,
            indirect_factor: 1.0,
        }
    }
}

impl BakeOptions {
    pub(crate) fn validate(self) -> Result<(), BakeError> {
        if !(MIN_RESOLUTION..=MAX_RESOLUTION).contains(&self.size) {
            return Err(BakeError::Options("size must be between 8 and 512"));
        }
        if !self.texels_per_unit.is_finite() || self.texels_per_unit < 0.0 {
            return Err(BakeError::Options(
                "texels per unit must be finite and nonnegative",
            ));
        }
        if !(1..=512).contains(&self.samples) || self.bounces > 4 {
            return Err(BakeError::Options("use 1–512 samples and 0–4 bounces"));
        }
        if !self.bias.is_finite() || self.bias <= 0.0 {
            return Err(BakeError::Options(
                "bias must be a finite positive distance",
            ));
        }
        if self.denoise_passes > 4 {
            return Err(BakeError::Options("denoise passes must be between 0 and 4"));
        }
        if !self.indirect_factor.is_finite() || self.indirect_factor < 0.0 {
            return Err(BakeError::Options(
                "indirect factor must be finite and nonnegative",
            ));
        }
        Ok(())
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
    #[error("a model with bake-role `receiver` needs an ID")]
    UnnamedReceiver,
    #[error("model `{id}` uses bake-role `{role}` and cannot receive an atlas region")]
    ReceiverRole { id: String, role: &'static str },
    #[error("receiver `{0}` contains overlapping lightmap charts at texel {1}, {2}")]
    Overlap(String, u32, u32),
    #[error("receiver `{0}` has no valid covered texels; check the lightmap UVs and bake size")]
    EmptyChart(String),
    #[error("receiver `{0}` needs at least {1} texels per side for its generated lightmap charts")]
    ChartSize(String, u32),
    #[error("receiver `{0}` needs lightmap-size between 8 and 512, or 0 to inherit")]
    ReceiverSize(String),
    #[error("receiver `{0}` needs finite, positive lightmap-texels-per-unit, or 0 to inherit")]
    ReceiverDensity(String),
    #[error("receiver `{0}` has conflicting resolutions across its mesh parts")]
    ConflictingResolution(String),
    #[error("receiver `{0}` has lightmap UVs outside the 0..1 chart")]
    ChartRange(String),
    #[error("the lightmap atlas would exceed {MAX_DIMENSION} pixels per side")]
    AtlasSize,
    #[error("encoded lightmap atlas exceeds the image asset size limit")]
    AtlasBytes,
    #[error("scene bounds or radiance exceed the supported lightmap range")]
    Radiance,
    #[error("lightmap PNG: {0}")]
    Png(#[from] png::EncodingError),
    #[error("lightmap samples: {0}")]
    SamplesIo(#[from] io::Error),
    #[error("invalid lightmap samples: {0}")]
    InvalidSamples(&'static str),
    #[error("these samples were baked with zero indirect light; rebake to enable indirect light")]
    MissingIndirectSamples,
}

pub struct BakedAtlas {
    pub png: Vec<u8>,
    pub size: [u32; 2],
    pub intensity: f32,
    pub mode: LightmapMode,
    pub regions: Vec<BakedRegion>,
}

pub struct BakedRegion {
    pub id: String,
    /// [u, v, width, height] inside the atlas, excluding padding.
    pub atlas: [f32; 4],
    pub resolution: u32,
}

#[derive(Clone, Copy)]
pub struct BakeLightInputs<'a> {
    pub lights: &'a [SceneLight],
    pub ambient: [f32; 3],
    pub environment: Option<ViewportEnvironment<'a>>,
}

#[derive(Clone, Copy)]
struct Texel<'a> {
    object: usize,
    material: Option<&'a MeshMaterial>,
    vertex: usize,
    weights: [f32; 3],
    radius: f32,
}

#[derive(Clone, Copy)]
pub(crate) struct ShadedTexel {
    pub position: [f32; 3],
    pub normal: [f32; 3],
    pub object: usize,
    pub radius: f32,
    pub direct: Vector3<f32>,
    pub indirect: Vector3<f32>,
}

pub(crate) struct BakeTile {
    pub id: String,
    pub origin: [u32; 2],
    pub resolution: u32,
}

pub fn bake_lightmaps(
    objects: &[SceneObject],
    lights: &[SceneLight],
    ambient: [f32; 3],
    visible_layers: u64,
    requested: &[String],
    options: BakeOptions,
) -> Result<BakedAtlas, BakeError> {
    bake_lightmaps_recording(
        objects,
        BakeLightInputs {
            lights,
            ambient,
            environment: None,
        },
        visible_layers,
        requested,
        options,
        None,
    )
}

/// Bake an atlas and optionally stream its unfiltered samples into a replay file.
pub fn bake_lightmaps_recording(
    objects: &[SceneObject],
    inputs: BakeLightInputs<'_>,
    visible_layers: u64,
    requested: &[String],
    options: BakeOptions,
    mut samples: Option<&mut dyn Write>,
) -> Result<BakedAtlas, BakeError> {
    options.validate()?;
    let BakeLightInputs {
        lights,
        ambient,
        environment,
    } = inputs;

    let requested: BTreeSet<_> = requested.iter().map(String::as_str).collect();
    let mut required = requested.clone();
    let mut receivers: BTreeMap<&str, Vec<usize>> = BTreeMap::new();
    for (index, object) in objects.iter().enumerate() {
        let id = object.id.as_str();
        if object.layers & visible_layers == 0 || !requested.is_empty() && !requested.contains(id) {
            continue;
        }
        if object.bake_role == BakeRole::Receiver {
            if id.is_empty() {
                return Err(BakeError::UnnamedReceiver);
            }
            required.insert(id);
        }
        if id.is_empty()
            || !object.bake_role.receives_atlas()
            || object.mesh.influences().is_some()
            || object.mesh.morph_geometry().is_some()
            || object.mesh.lightmap_uvs().is_empty()
            || !object
                .mesh
                .sections()
                .any(|(_, material)| receiver_material(material))
        {
            continue;
        }
        receivers.entry(id).or_default().push(index);
    }
    for id in required {
        if !receivers.contains_key(id) {
            if let Some(object) = objects.iter().find(|object| {
                object.id == id
                    && object.layers & visible_layers != 0
                    && !object.bake_role.receives_atlas()
            }) {
                return Err(BakeError::ReceiverRole {
                    id: id.into(),
                    role: object.bake_role.name(),
                });
            }
            return Err(BakeError::MissingReceiver(id.into()));
        }
    }
    if receivers.is_empty() {
        return Err(BakeError::NoReceivers);
    }

    let receivers = receivers
        .into_iter()
        .map(|(id, instances)| {
            let resolution = receiver_resolution(objects, id, &instances, options)?;
            Ok((id, instances, resolution))
        })
        .collect::<Result<Vec<_>, BakeError>>()?;
    let sides: Vec<_> = receivers
        .iter()
        .map(|(_, _, resolution)| resolution + 2 * PADDING)
        .collect();
    let (size, origins) = pack_atlas(&sides).ok_or(BakeError::AtlasSize)?;
    let trace = LightmapTrace::new(objects, visible_layers);
    let distance = trace.max_distance().ok_or(BakeError::Radiance)?;
    let bounce_lights = if options.mode == LightmapMode::Mixed {
        Cow::Owned(
            lights
                .iter()
                .copied()
                .filter(|light| light.bake_mode != LightBakeMode::Disabled)
                .collect(),
        )
    } else {
        Cow::Borrowed(lights)
    };
    let direct_lights = match options.mode {
        LightmapMode::Indirect => Cow::Borrowed(&[][..]),
        LightmapMode::Diffuse => Cow::Borrowed(lights),
        LightmapMode::Mixed => Cow::Owned(
            bounce_lights
                .iter()
                .copied()
                .filter(|light| light.bake_mode == LightBakeMode::All)
                .collect(),
        ),
    };
    let lighting = BakeLighting {
        trace: &trace,
        bounce_lights: &bounce_lights,
        direct_lights: &direct_lights,
        ambient: Vector3::from(ambient) * 0.22,
        environment: environment
            .filter(|environment| options.indirect_factor > 0.0 && environment.contributes_light())
            .map(EnvironmentRadiance::new),
        distance,
        visible_layers,
        options,
    };
    if let Some(writer) = samples.as_deref_mut() {
        lightmap_samples::write_header(writer, size, receivers.len(), options)?;
    }
    let mut atlas = AtlasBuilder::new(size, receivers.len());
    for (index, ((id, instances, resolution), origin)) in
        receivers.into_iter().zip(origins).enumerate()
    {
        let side = sides[index];
        let texels = rasterize(objects, id, &instances, side, resolution)?;
        let mut shaded = vec![None; texels.len()];
        shade_tile(objects, &lighting, &texels, &mut shaded, index as u32, side);
        drop(texels);
        let tile = BakeTile {
            id: id.into(),
            origin,
            resolution,
        };
        if let Some(writer) = samples.as_deref_mut() {
            lightmap_samples::write_tile(writer, &tile, &shaded)?;
        }
        atlas.place(tile, &shaded, options)?;
    }
    atlas.finish(options.mode)
}

pub(crate) struct AtlasBuilder {
    size: [u32; 2],
    pixels: Vec<[f32; 3]>,
    regions: Vec<BakedRegion>,
}

impl AtlasBuilder {
    pub(crate) fn new(size: [u32; 2], count: usize) -> Self {
        Self {
            size,
            pixels: vec![[0.0; 3]; (size[0] * size[1]) as usize],
            regions: Vec::with_capacity(count),
        }
    }

    pub(crate) fn place(
        &mut self,
        tile: BakeTile,
        shaded: &[Option<ShadedTexel>],
        options: BakeOptions,
    ) -> Result<(), BakeError> {
        let BakeTile {
            id,
            origin: [x, y],
            resolution,
        } = tile;
        if shaded.iter().all(Option::is_none) {
            return Err(BakeError::EmptyChart(id));
        }
        let [width, height] = self.size;
        let side = resolution + 2 * PADDING;
        let mut filtered = filter_indirect(shaded, side, options);
        pad_chart(&mut filtered, side);
        for (row, pixels) in filtered.chunks_exact(side as usize).enumerate() {
            let start = ((y + row as u32) * width + x) as usize;
            for (out, pixel) in self.pixels[start..start + side as usize]
                .iter_mut()
                .zip(pixels)
            {
                *out = pixel.unwrap_or([0.0; 3]);
            }
        }
        self.regions.push(BakedRegion {
            id,
            atlas: [
                (x + PADDING) as f32 / width as f32,
                (y + PADDING) as f32 / height as f32,
                resolution as f32 / width as f32,
                resolution as f32 / height as f32,
            ],
            resolution,
        });
        Ok(())
    }

    pub(crate) fn finish(self, mode: LightmapMode) -> Result<BakedAtlas, BakeError> {
        let mut intensity = 1.0_f32;
        for &channel in self.pixels.iter().flatten() {
            if !channel.is_finite() || channel > 1024.0 {
                return Err(BakeError::Radiance);
            }
            intensity = intensity.max(channel);
        }
        intensity = intensity.ceil();
        let bytes: Vec<_> = self
            .pixels
            .iter()
            .flat_map(|pixel| pixel.map(|channel| encode_srgb(channel / intensity)))
            .collect();
        let mut png = Vec::new();
        let [width, height] = self.size;
        let mut encoder = png::Encoder::new(&mut png, width, height);
        encoder.set_color(png::ColorType::Rgb);
        encoder.set_depth(png::BitDepth::Eight);
        encoder.write_header()?.write_image_data(&bytes)?;
        if png.len() as u64 > xrack_assets::MAX_ASSET_BYTES {
            return Err(BakeError::AtlasBytes);
        }
        Ok(BakedAtlas {
            png,
            size: self.size,
            intensity,
            mode,
            regions: self.regions,
        })
    }
}

fn receiver_material(material: Option<&MeshMaterial>) -> bool {
    material.is_some_and(|material| {
        material.depth_coverage().is_some()
            && material.pbr.is_some()
            && material.transmission_blur().is_none()
    })
}

fn receiver_resolution(
    objects: &[SceneObject],
    id: &str,
    instances: &[usize],
    options: BakeOptions,
) -> Result<u32, BakeError> {
    let requested = instances
        .first()
        .map(|&index| objects[index].lightmap_resolution)
        .unwrap_or_default();
    let minimum = instances
        .iter()
        .map(|&index| objects[index].mesh.lightmap_min_size())
        .max()
        .unwrap_or(0);
    let resolution = match requested {
        LightmapResolution::Size(size) => u32::try_from(size)
            .ok()
            .filter(|size| (MIN_RESOLUTION..=MAX_RESOLUTION).contains(size))
            .ok_or_else(|| BakeError::ReceiverSize(id.into()))?,
        LightmapResolution::Inherit if options.texels_per_unit == 0.0 => options.size,
        request => {
            let density = match request {
                LightmapResolution::TexelsPerUnit(value) => value,
                _ => options.texels_per_unit,
            };
            if !density.is_finite() || density <= 0.0 {
                return Err(BakeError::ReceiverDensity(id.into()));
            }
            density_resolution(objects, id, instances, density)?.max(minimum.min(MAX_RESOLUTION))
        }
    };
    if instances
        .iter()
        .any(|&index| objects[index].lightmap_resolution != requested)
    {
        return Err(BakeError::ConflictingResolution(id.into()));
    }
    if resolution < minimum {
        return Err(BakeError::ChartSize(id.into(), minimum));
    }
    Ok(resolution)
}

fn density_resolution(
    objects: &[SceneObject],
    id: &str,
    instances: &[usize],
    density: f64,
) -> Result<u32, BakeError> {
    let mut world_area = 0.0;
    let mut uv_area = 0.0;
    for &index in instances {
        let object = &objects[index];
        let transform = object.transform_rows();
        let vertices = object.mesh.vertices();
        for (range, material) in object.mesh.sections() {
            if !receiver_material(material) {
                continue;
            }
            for vertex in range.step_by(3) {
                let Some(uv) = chart_triangle(object, id, vertex)? else {
                    continue;
                };
                let [a, b, c] = uv.map(|uv| Vector2::from(uv.map(f64::from)));
                let area = (b - a).perp(&(c - a)).abs();
                if area == 0.0 {
                    continue;
                }
                let origin = Vector3::from(vertices[vertex].position.map(f64::from));
                let edge = |offset: usize| {
                    let delta =
                        Vector3::from(vertices[vertex + offset].position.map(f64::from)) - origin;
                    Vector3::from(transform.map(|[x, y, z, _]| {
                        f64::from(x) * delta.x + f64::from(y) * delta.y + f64::from(z) * delta.z
                    }))
                };
                world_area += edge(1).cross(&edge(2)).norm();
                uv_area += area;
            }
        }
    }
    if uv_area == 0.0 {
        return Err(BakeError::EmptyChart(id.into()));
    }
    if !world_area.is_finite() {
        return Err(BakeError::Radiance);
    }
    let side = (density * (world_area / uv_area).sqrt()).ceil();
    Ok(side.clamp(f64::from(MIN_RESOLUTION), f64::from(MAX_RESOLUTION)) as u32)
}

fn pack_atlas(sides: &[u32]) -> Option<([u32; 2], Vec<[u32; 2]>)> {
    let largest = sides.iter().copied().max()?;
    let area: u64 = sides.iter().map(|&side| u64::from(side).pow(2)).sum();
    if largest > MAX_DIMENSION || area > u64::from(MAX_DIMENSION).pow(2) {
        return None;
    }
    let mut order: Vec<_> = (0..sides.len()).collect();
    order.sort_unstable_by_key(|&index| (Reverse(sides[index]), index));
    let minimum = largest.max(area.div_ceil(u64::from(MAX_DIMENSION)) as u32);
    let ideal = (area as f64).sqrt().ceil() as u32;
    let mut widths = [
        minimum,
        ideal,
        ideal.next_multiple_of(largest),
        ideal.next_power_of_two(),
        (ideal + largest).min(MAX_DIMENSION),
        MAX_DIMENSION,
    ];
    widths
        .iter_mut()
        .for_each(|w| *w = (*w).clamp(minimum, MAX_DIMENSION));
    widths.sort_unstable();
    widths
        .into_iter()
        .enumerate()
        .filter(|&(index, width)| index == 0 || widths[index - 1] != width)
        .filter_map(|(_, width)| pack_shelves(sides, &order, width))
        .min_by_key(|&([width, height], _)| (width * height, width.max(height), Reverse(width)))
}

fn pack_shelves(sides: &[u32], order: &[usize], width: u32) -> Option<([u32; 2], Vec<[u32; 2]>)> {
    let mut origins = vec![[0, 0]; sides.len()];
    let mut shelves: Vec<(u32, u32)> = Vec::new(); // (top, occupied width)
    let (mut right, mut bottom) = (0, 0);
    for &index in order {
        let side = sides[index];
        // Each shelf was started with a square at least this tall.
        let (top, used) =
            if let Some(shelf) = shelves.iter_mut().rev().find(|s| s.1 + side <= width) {
                shelf
            } else {
                let top = bottom;
                bottom += side;
                if bottom > MAX_DIMENSION {
                    return None;
                }
                shelves.push((top, 0));
                shelves.last_mut()?
            };
        origins[index] = [*used, *top];
        *used += side;
        right = right.max(*used);
    }
    Some(([right, bottom], origins))
}

fn chart_triangle(
    object: &SceneObject,
    id: &str,
    vertex: usize,
) -> Result<Option<[[f32; 2]; 3]>, BakeError> {
    let chart = object.mesh.lightmap_uvs();
    let uv = [chart[vertex], chart[vertex + 1], chart[vertex + 2]];
    if uv == [[-1.0; 2]; 3] {
        return Ok(None);
    }
    if !uv.iter().flatten().all(|value| (0.0..=1.0).contains(value)) {
        return Err(BakeError::ChartRange(id.into()));
    }
    Ok(Some(uv))
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
        for (range, material) in mesh
            .sections()
            .filter(|(_, material)| receiver_material(*material))
        {
            for vertex in range.step_by(3) {
                let Some(uv) = chart_triangle(&objects[index], id, vertex)? else {
                    continue;
                };
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
    shaded: &[Option<ShadedTexel>],
    side: u32,
    options: BakeOptions,
) -> Vec<Option<[f32; 3]>> {
    if options.indirect_factor == 0.0 || options.denoise_passes == 0 {
        return shaded
            .iter()
            .map(|value| value.map(|v| (v.direct + v.indirect * options.indirect_factor).into()))
            .collect();
    }
    let mut source: Vec<_> = shaded
        .iter()
        .map(|value| value.map_or(Vector3::zeros(), |value| value.indirect))
        .collect();
    let mut target = vec![Vector3::zeros(); source.len()];
    for pass in 0..options.denoise_passes {
        denoise_pass(shaded, &source, &mut target, side as usize, 1 << pass);
        std::mem::swap(&mut source, &mut target);
    }
    shaded
        .iter()
        .zip(source)
        .map(|(value, indirect)| {
            value.map(|v| (v.direct + indirect * options.indirect_factor).into())
        })
        .collect()
}

fn denoise_pass(
    shaded: &[Option<ShadedTexel>],
    source: &[Vector3<f32>],
    target: &mut [Vector3<f32>],
    side: usize,
    stride: isize,
) {
    let chunk = tile_chunk(side);
    thread::scope(|scope| {
        for (row, target) in target.chunks_mut(chunk).enumerate() {
            scope.spawn(move || {
                for (offset, output) in target.iter_mut().enumerate() {
                    let index = row * chunk + offset;
                    let Some(texel) = shaded[index].as_ref() else {
                        continue;
                    };
                    let origin = Vector3::from(texel.position);
                    let normal = Vector3::from(texel.normal);
                    let (x, y) = (index % side, index / side);
                    let mut value = source[index] * 4.0;
                    let mut weights = 4.0;
                    let sample_weight = |neighbor: usize, distance: isize| {
                        let Some(other) = shaded[neighbor].as_ref() else {
                            return 0.0;
                        };
                        let facing = normal.dot(&Vector3::from(other.normal));
                        if other.object != texel.object || facing < 0.85 {
                            return 0.0;
                        }
                        let separation = (Vector3::from(other.position) - origin).norm();
                        facing * (1.0 - separation / (texel.radius * distance as f32)).max(0.0)
                    };
                    for dy in -1_isize..=1 {
                        for dx in -1_isize..=1 {
                            if dx == 0 && dy == 0 {
                                continue;
                            }
                            let Some(column) =
                                x.checked_add_signed(dx * stride).filter(|&v| v < side)
                            else {
                                continue;
                            };
                            let Some(row) = y.checked_add_signed(dy * stride).filter(|&v| v < side)
                            else {
                                continue;
                            };
                            let neighbor = row * side + column;
                            let weight = sample_weight(neighbor, stride);
                            if weight == 0.0 {
                                continue;
                            }
                            // Every intervening texel must be on the same local surface.
                            // Wider taps cannot jump across an empty UV seam.
                            let delta = dy * side as isize + dx;
                            if (1..stride).any(|step| {
                                sample_weight((index as isize + delta * step) as usize, step) == 0.0
                            }) {
                                continue;
                            }
                            let kernel = ((2 - dx.unsigned_abs()) * (2 - dy.unsigned_abs())) as f32;
                            value += source[neighbor] * (kernel * weight);
                            weights += kernel * weight;
                        }
                    }
                    *output = value / weights;
                }
            });
        }
    });
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
    bounce_lights: &'a [SceneLight],
    direct_lights: &'a [SceneLight],
    ambient: Vector3<f32>,
    environment: Option<EnvironmentRadiance>,
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
    let chunk = tile_chunk(side as usize);
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
                    let back = texel.material.is_some_and(|material| {
                        material.cull_mode == xrack_runtime::scene3d::MeshCullMode::Front
                    });
                    *out = object
                        .bake_sample(texel.vertex, texel.weights, texel.material, back)
                        .map(|surface| {
                            let (direct, indirect) = lighting.irradiance(surface, seed);
                            ShadedTexel {
                                position: surface.position,
                                normal: surface.normal,
                                object: texel.object,
                                radius: texel.radius,
                                direct,
                                indirect,
                            }
                        });
                }
            });
        }
    });
}

fn tile_chunk(side: usize) -> usize {
    let workers = thread::available_parallelism()
        .map_or(1, usize::from)
        .min(side);
    side * side.div_ceil(workers)
}

impl BakeLighting<'_> {
    fn irradiance(&self, surface: BakedSurface, seed: u32) -> (Vector3<f32>, Vector3<f32>) {
        let direct = self.direct(surface, self.direct_lights);
        let indirect = if self.options.indirect_factor > 0.0 {
            self.indirect(surface, seed)
        } else {
            Vector3::zeros()
        };
        (direct, indirect)
    }

    fn indirect(&self, surface: BakedSurface, seed: u32) -> Vector3<f32> {
        let mut indirect = Vector3::zeros();
        let layers = surface.layers & self.visible_layers;
        let importance = self
            .environment
            .as_ref()
            .and_then(|env| env.importance.as_ref());
        for sample in 0..self.options.samples {
            let mut at = surface;
            let mut throughput: Vector3<f32> = Vector3::repeat(1.0);
            let mut path_weight = 1.0;
            let mut seed = hash(seed ^ sample.wrapping_mul(0x9e3779b9));
            for bounce in 0..=self.options.bounces {
                let normal = Vector3::from(at.normal);
                let direction = if let Some(importance) = importance {
                    let (direction, weight) = mixed_direction(normal, &mut seed, importance);
                    if weight == 0.0 {
                        break;
                    }
                    path_weight *= weight;
                    direction
                } else {
                    hemisphere(normal, &mut seed)
                };
                let origin = Vector3::from(at.position) + normal * self.options.bias;
                let hit = Ray3D::new(origin.into(), direction.into())
                    .and_then(|ray| self.trace.trace(ray, self.distance, layers));
                let Some(hit) = hit else {
                    let radiance = self
                        .environment
                        .as_ref()
                        .map_or(self.ambient, |env| self.ambient + env.sample(direction));
                    indirect += throughput.component_mul(&radiance) * path_weight;
                    break;
                };
                if bounce == self.options.bounces {
                    break;
                }
                let diffuse = Vector3::from(hit.diffuse);
                let radiance = Vector3::from(hit.emission)
                    + diffuse.component_mul(&self.direct(hit, self.bounce_lights));
                indirect += radiance.component_mul(&throughput) * path_weight;
                throughput.component_mul_assign(&diffuse);
                if throughput.max() < 0.0001 {
                    break;
                }
                at = hit;
            }
        }
        indirect / self.options.samples as f32
    }

    fn direct(&self, surface: BakedSurface, lights: &[SceneLight]) -> Vector3<f32> {
        let position = Vector3::from(surface.position);
        let normal = Vector3::from(surface.normal);
        let mut result = Vector3::zeros();
        for light in lights {
            let layers = surface.layers & light.layers & self.visible_layers;
            if layers == 0 || !light.scope.contains(surface.lighting_index) {
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
                    self.trace.occluded(
                        ray,
                        (distance - self.options.bias * 2.0).max(0.0),
                        layers,
                        light.scope,
                    )
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

fn mixed_direction(
    normal: Vector3<f32>,
    seed: &mut u32,
    importance: &EnvironmentImportance,
) -> (Vector3<f32>, f32) {
    let (direction, environment_pdf) = if random(seed) < 0.5 {
        importance.sample(random(seed), random(seed))
    } else {
        let direction = hemisphere(normal, seed);
        (direction, importance.pdf(direction))
    };
    let cosine_pdf = normal.dot(&direction).max(0.0) / PI;
    let weight = if cosine_pdf > 0.0 {
        // One-sample balance heuristic for a 50/50 mixture of the two distributions.
        2.0 * cosine_pdf / (cosine_pdf + environment_pdf)
    } else {
        0.0
    };
    (direction, weight)
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
        scene3d::{Mesh3D, MeshAlphaMode, MeshMaterial, MeshPart, MeshPbr, MeshVertex},
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
            opacity: 1.0,
            opacity_texture: None,
            invert_opacity_map_value: false,
            shader: None,
            base_item: None,
            channels: Default::default(),
            vertex_colors: true,
            vertex_masks: Default::default(),
            cull_mode: Default::default(),
            depth_draw_mode: Default::default(),
            blend_mode: Default::default(),
            base_color: if emitter {
                [1.0, 0.0, 0.0, 1.0]
            } else {
                [1.0; 4]
            },
            base_texture: None,
            alpha_mode: MeshAlphaMode::Opaque,
            pbr: (!emitter).then_some(MeshPbr {
                anisotropy: None,
                fresnel: Default::default(),
                metallic: 0.0,
                roughness: 1.0,
                emissive: [0.0; 3],
                occlusion_strength: 1.0,
                surface_texture: None,
                separate_surface_maps: None,
                specular_reflection: None,
                height_map: None,
                workflow: Default::default(),
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
    fn scoped_bake_filters_receivers_and_occluders() -> TestResult {
        use xrack_runtime::{
            RecordList, RecordValue, Value, VecModel,
            scene3d::{MeshAsset, model_lights, record_objects},
        };

        let plate = plate("receiver", 0.0, false)?.mesh;
        let row = |id: &str, parent: &str, mesh: &str, x, z, size: [f64; 3]| {
            let named = [("id", id), ("parent", parent), ("mesh", mesh)]
                .into_iter()
                .map(|(name, value)| (name.to_owned(), Value::String(value.into())));
            let numeric = [
                "x", "y", "z", "scale-x", "scale-y", "scale-z", "yaw", "pitch", "roll",
            ]
            .into_iter()
            .zip([x, 0.0, z, size[0], size[1], size[2], 0.0, 0.0, 0.0])
            .map(|(name, value)| (name.to_owned(), Value::Float(value)));
            RecordValue::new(
                "Actor",
                named
                    .chain(numeric)
                    .chain([("color".into(), Value::Color(Color(0xffffffff)))])
                    .collect(),
            )
        };
        let light = SceneLight {
            source: SceneLightSource::Directional {
                toward_light: [0.0, 0.0, 1.0],
            },
            radiance: [PI, 0.0, 0.0],
            casts_shadow: true,
            shadow: Default::default(),
            shadow_projection: Default::default(),
            bake_mode: LightBakeMode::All,
            layers: 1,
            scope: Default::default(),
        };
        let authored = model_lights(&VecModel::from(vec![light]), |light| {
            Some((light, String::new(), "group".into()))
        });
        let bake = |parent| -> Result<[u8; 2], Box<dyn std::error::Error>> {
            let scene = record_objects(
                &RecordList::new(
                    "Actor",
                    vec![
                        row("receiver", "group", "plate", 0.0, 0.0, [1.0; 3]),
                        row("neighbor", "", "plate", 2.0, 0.0, [1.0; 3]),
                        row(
                            "caster",
                            parent,
                            "primitive:box",
                            0.0,
                            0.4,
                            [0.45, 0.45, 0.1],
                        ),
                        row("group", "", "", 0.0, 0.0, [1.0; 3]),
                    ],
                ),
                |name| (name == "plate").then(|| MeshAsset::Static(plate.clone())),
                |_| None,
            );
            let atlas = bake_lightmaps(
                scene.objects(),
                &authored.resolve(Some(&scene)),
                [0.0; 3],
                1,
                &[],
                BakeOptions {
                    size: 16,
                    mode: LightmapMode::Mixed,
                    indirect_factor: 0.0,
                    ..Default::default()
                },
            )?;
            let png = xrack_assets::decode_png(&atlas.png)?;
            let center = |id| -> Result<u8, &'static str> {
                let region = atlas
                    .regions
                    .iter()
                    .find(|region| region.id == id)
                    .ok_or("missing region")?;
                let [x, y, w, h] = region.atlas;
                let row = ((y + h * 0.5) * atlas.size[1] as f32) as usize;
                let column = ((x + w * 0.5) * atlas.size[0] as f32) as usize;
                Ok(png.rgba[4 * (row * atlas.size[0] as usize + column)])
            };
            Ok([center("receiver")?, center("neighbor")?])
        };
        assert_eq!(bake("")?, [255, 0]);
        assert_eq!(bake("group")?, [0, 0]);
        Ok(())
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
            shadow: Default::default(),
            shadow_projection: Default::default(),
            bake_mode: LightBakeMode::Disabled,
            layers: 1,
            scope: Default::default(),
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
    fn model_bake_roles_select_receivers_and_exclude_moving_occluders() -> TestResult {
        let mut receiver = plate("receiver", 0.0, false)?;
        receiver.bake_role = BakeRole::Receiver;
        let mut static_prop = plate("prop", -0.2, false)?;
        static_prop.bake_role = BakeRole::Contributor;
        let mut moving = SceneObject::cuboid(
            "moving",
            [0.0, 0.0, 0.4],
            [0.45, 0.45, 0.1],
            [0.0; 3],
            Color(0xffffffff),
        )
        .ok_or("invalid moving object")?;
        moving.bake_role = BakeRole::Excluded;
        let light = SceneLight {
            source: SceneLightSource::Directional {
                toward_light: [0.0, 0.0, 1.0],
            },
            radiance: [PI; 3],
            casts_shadow: true,
            shadow: Default::default(),
            shadow_projection: Default::default(),
            bake_mode: LightBakeMode::All,
            layers: 1,
            scope: Default::default(),
        };
        let options = BakeOptions {
            size: 16,
            samples: 2,
            bounces: 0,
            mode: LightmapMode::Mixed,
            ..BakeOptions::default()
        };
        let bake = |objects: &[SceneObject], requested: &[String]| {
            bake_lightmaps(objects, &[light], [0.0; 3], 1, requested, options)
        };
        let mut objects = [receiver, static_prop, moving];
        let excluded = bake(&objects, &[])?;
        assert_eq!(excluded.regions.len(), 1);
        assert_eq!(excluded.regions[0].id, "receiver");
        assert_eq!(excluded.png, bake(&objects[..2], &[])?.png);
        assert!(matches!(
            bake(&objects, &["prop".into()]),
            Err(BakeError::ReceiverRole {
                role: "contributor",
                ..
            })
        ));
        assert!(matches!(
            bake(&objects, &["moving".into()]),
            Err(BakeError::ReceiverRole {
                role: "excluded",
                ..
            })
        ));

        objects[2].bake_role = BakeRole::Contributor;
        let shadowed = bake(&objects, &[])?;
        let center = (10 * 20 + 10) * 4;
        assert!(xrack_assets::decode_png(&excluded.png)?.rgba[center] > 240);
        assert!(xrack_assets::decode_png(&shadowed.png)?.rgba[center] < 10);
        objects[2].bake_role = BakeRole::Receiver;
        assert!(matches!(
            bake(&objects, &[]),
            Err(BakeError::MissingReceiver(id)) if id == "moving"
        ));
        Ok(())
    }

    #[test]
    fn receiver_sizes_and_world_density_share_a_compact_deterministic_atlas() -> TestResult {
        let mut fixed = plate("fixed", 0.0, false)?;
        fixed.lightmap_resolution = LightmapResolution::from_settings(64, 200.0);
        let mut dense = SceneObject::from_mesh(
            "dense".into(),
            fixed.mesh.clone(),
            [3.0, 0.0, 0.0],
            [1.0, 1.0, 1.0],
            [0.0; 3],
            Color(0xffffffff),
        )
        .ok_or("invalid density fixture")?;
        dense.lightmap_resolution = LightmapResolution::TexelsPerUnit(16.0);
        let scaled = SceneObject::from_mesh(
            "scaled".into(),
            fixed.mesh.clone(),
            [7.0, 0.0, 0.0],
            [2.0, 2.0, 1.0],
            [0.0; 3],
            Color(0xffffffff),
        )
        .ok_or("invalid scaled fixture")?;
        let settings = BakeOptions {
            samples: 1,
            bounces: 0,
            texels_per_unit: 16.0,
            ..BakeOptions::default()
        };
        let bake =
            |objects: &[SceneObject]| bake_lightmaps(objects, &[], [1.0; 3], 1, &[], settings);
        let mut objects = [fixed, dense, scaled];
        let atlas = bake(&objects)?;
        let [width, height] = atlas.size;
        assert_eq!(
            atlas
                .regions
                .iter()
                .map(|r| (r.id.as_str(), r.resolution))
                .collect::<Vec<_>>(),
            [("dense", 16), ("fixed", 64), ("scaled", 32)]
        );
        assert!(width * height < 3 * (64 + 2 * PADDING).pow(2));
        let image = xrack_assets::decode_png(&atlas.png)?;
        assert_eq!(image.size, atlas.size);
        let bounds: Vec<_> = atlas
            .regions
            .iter()
            .map(|region| {
                let [x, y, w, h] = region.atlas;
                let x = (x * width as f32).round() as u32;
                let y = (y * height as f32).round() as u32;
                assert_eq!((w * width as f32).round() as u32, region.resolution);
                assert_eq!((h * height as f32).round() as u32, region.resolution);
                let right = x + region.resolution;
                let bottom = y + region.resolution;
                assert!(
                    x >= PADDING
                        && y >= PADDING
                        && right + PADDING <= width
                        && bottom + PADDING <= height
                );
                let center =
                    ((y + region.resolution / 2) * width + x + region.resolution / 2) as usize;
                assert!(image.rgba[center * 4] > 100);
                [x - PADDING, y - PADDING, right + PADDING, bottom + PADDING]
            })
            .collect();
        for (index, [left, top, right, bottom]) in bounds.iter().copied().enumerate() {
            for &[x, y, end_x, end_y] in &bounds[..index] {
                assert!(right <= x || end_x <= left || bottom <= y || end_y <= top);
            }
        }
        objects.reverse();
        assert_eq!(atlas.png, bake(&objects)?.png);

        let dense = &mut objects[1];
        let uv = dense.mesh.lightmap_uvs().to_vec();
        dense.mesh = dense
            .mesh
            .clone()
            .with_generated_lightmap_uv(uv, 32)
            .ok_or("invalid generated chart fixture")?;
        assert_eq!(bake(&objects)?.regions[0].resolution, 32);
        objects[1].lightmap_resolution = LightmapResolution::Size(16);
        assert!(matches!(bake(&objects), Err(BakeError::ChartSize(id, 32)) if id == "dense"));
        objects[1].lightmap_resolution = LightmapResolution::Size(-1);
        assert!(matches!(bake(&objects), Err(BakeError::ReceiverSize(id)) if id == "dense"));
        objects[1].lightmap_resolution = LightmapResolution::TexelsPerUnit(f64::NAN);
        assert!(matches!(bake(&objects), Err(BakeError::ReceiverDensity(id)) if id == "dense"));
        Ok(())
    }

    #[test]
    fn multiscale_denoise_reduces_variation_and_protects_uv_gaps_and_direct_shadows() -> TestResult
    {
        const SIDE: usize = 12;
        let mut shaded = Vec::new();
        for y in 0..SIDE {
            for x in 0..SIDE {
                if x == 5 {
                    shaded.push(None);
                    continue;
                }
                let direct = Vector3::new(0.0, if y < 6 { 0.31 } else { 0.0 }, 0.0);
                let indirect = if x < 5 {
                    let noise = if (x + y) % 2 == 0 { -0.18 } else { 0.18 };
                    Vector3::new(0.25 + noise, 0.0, 0.0)
                } else {
                    Vector3::new(0.0, 0.0, if y < 6 { 0.75 } else { 0.25 })
                };
                shaded.push(Some(ShadedTexel {
                    direct,
                    indirect,
                    position: [x as f32, y as f32, 0.0],
                    normal: if x > 5 && y >= 6 {
                        [0.0, 1.0, 0.0]
                    } else {
                        [0.0, 0.0, 1.0]
                    },
                    object: 0,
                    radius: 2.5,
                }));
            }
        }
        let filter = |denoise_passes, indirect_factor| {
            filter_indirect(
                &shaded,
                SIDE as u32,
                BakeOptions {
                    denoise_passes,
                    indirect_factor,
                    ..BakeOptions::default()
                },
            )
        };
        let raw = filter(0, 1.0);
        let denoised = filter(4, 1.0);
        let scaled = filter(4, 2.0);
        let direct_only = filter(4, 0.0);
        let mut raw_error = 0.0;
        let mut filtered_error = 0.0;
        for index in 0..SIDE * SIDE {
            let Some(input) = shaded[index] else {
                assert!(denoised[index].is_none());
                continue;
            };
            let filtered = denoised[index].ok_or("missing denoised texel")?;
            let doubled = scaled[index].ok_or("missing scaled texel")?;
            let raw = raw[index].ok_or("missing raw texel")?;
            assert_eq!(filtered[1], input.direct.y);
            assert_eq!(direct_only[index], Some(input.direct.into()));
            assert_eq!(doubled[0], 2.0 * filtered[0]);
            assert_eq!(doubled[2], 2.0 * filtered[2]);
            if index % SIDE < 5 {
                assert_eq!(filtered[2], 0.0, "an empty UV seam blocks the wider taps");
                raw_error += (raw[0] - 0.25).powi(2);
                filtered_error += (filtered[0] - 0.25).powi(2);
            } else {
                assert_eq!(filtered[0], 0.0);
                let expected = if index / SIDE < 6 { 0.75 } else { 0.25 };
                assert!((filtered[2] - expected).abs() < 1e-6);
            }
        }
        assert!(
            filtered_error < raw_error * 0.10,
            "{filtered_error}, {raw_error}"
        );
        Ok(())
    }

    #[test]
    fn saved_lightmap_samples_recreate_the_original_and_reprocess_indirect_light() -> TestResult {
        let panel = plate("receiver", 0.0, false)?;
        let emitter = plate("emitter", 0.8, true)?;
        let objects = [panel, emitter];
        let options = BakeOptions {
            size: 16,
            samples: 16,
            bounces: 1,
            denoise_passes: 2,
            ..BakeOptions::default()
        };
        let mut samples = Vec::new();
        let inputs = BakeLightInputs {
            lights: &[],
            ambient: [0.1; 3],
            environment: None,
        };
        let atlas =
            bake_lightmaps_recording(&objects, inputs, 1, &[], options, Some(&mut samples))?;
        let (replayed, recovered) = replay_lightmap_samples(&mut samples.as_slice(), None, None)?;
        assert_eq!(atlas.png, replayed.png);
        assert_eq!(atlas.size, replayed.size);
        assert_eq!(atlas.intensity, replayed.intensity);
        assert_eq!(atlas.regions[0].atlas, replayed.regions[0].atlas);
        assert_eq!(recovered.samples, options.samples);
        assert_eq!(recovered.bounces, options.bounces);
        assert_eq!(recovered.denoise_passes, options.denoise_passes);
        let (edited, _) = replay_lightmap_samples(&mut samples.as_slice(), Some(0), Some(2.0))?;
        let fresh = bake_lightmaps(
            &objects,
            &[],
            [0.1; 3],
            1,
            &[],
            BakeOptions {
                denoise_passes: 0,
                indirect_factor: 2.0,
                ..options
            },
        )?;
        assert_eq!(edited.png, fresh.png);
        assert_ne!(edited.png, atlas.png);
        assert!(matches!(
            replay_lightmap_samples(&mut &samples[..samples.len() - 1], None, None),
            Err(BakeError::SamplesIo(error)) if error.kind() == io::ErrorKind::UnexpectedEof
        ));
        samples.push(0);
        assert!(matches!(
            replay_lightmap_samples(&mut samples.as_slice(), None, None),
            Err(BakeError::InvalidSamples("trailing data"))
        ));

        let mut direct_only = Vec::new();
        let direct = bake_lightmaps_recording(
            &objects,
            inputs,
            1,
            &[],
            BakeOptions {
                indirect_factor: 0.0,
                ..options
            },
            Some(&mut direct_only),
        )?;
        let (replayed, _) = replay_lightmap_samples(&mut direct_only.as_slice(), None, None)?;
        assert_eq!(direct.png, replayed.png);
        assert!(matches!(
            replay_lightmap_samples(&mut direct_only.as_slice(), None, Some(1.0)),
            Err(BakeError::MissingIndirectSamples)
        ));
        Ok(())
    }

    #[test]
    fn hdr_environment_bakes_color_and_yaw_with_visibility() -> TestResult {
        use crate::environment::test_probe;
        use std::sync::Arc;

        let probe = Arc::new(
            test_probe(|ray| {
                if ray.z > 0.72 {
                    [0.5, 0.0, 0.0]
                } else if ray.z < -0.72 {
                    [0.0, 0.0, 0.5]
                } else {
                    [0.0; 3]
                }
            })
            .ok_or("invalid HDR fixture")?,
        );
        let receiver = plate("receiver", 0.0, false)?;
        let screen = SceneObject::cuboid(
            "screen",
            [0.0, 0.0, 0.3],
            [100.0, 100.0, 0.1],
            [0.0; 3],
            Color(0xffffffff),
        )
        .ok_or("invalid screen fixture")?;
        let bake = |objects: &[SceneObject],
                    yaw,
                    intensity,
                    indirect_factor|
         -> Result<[u8; 3], Box<dyn std::error::Error>> {
            let atlas = bake_lightmaps_recording(
                objects,
                BakeLightInputs {
                    lights: &[],
                    ambient: [0.0; 3],
                    environment: Some(ViewportEnvironment {
                        source: xrack_runtime::scene3d::EnvironmentSource::Panorama(&probe),
                        orientation: xrack_runtime::scene3d::EnvironmentOrientation::from_radians(
                            [yaw, 0.0, 0.0],
                        ),
                        horizon: 0.0,
                        intensity,
                    }),
                },
                1,
                &[],
                BakeOptions {
                    size: 16,
                    samples: 96,
                    bounces: 0,
                    indirect_factor,
                    ..BakeOptions::default()
                },
                None,
            )?;
            let image = xrack_assets::decode_png(&atlas.png)?;
            let offset = (10 * 20 + 10) * 4;
            Ok(std::array::from_fn(|i| image.rgba[offset + i]))
        };
        let visible = std::slice::from_ref(&receiver);
        let red = bake(visible, 0.0, 1.0, 1.0)?;
        let blue = bake(visible, PI, 1.0, 1.0)?;
        assert!(red[0] > 100 && red[2] == 0, "{red:?}");
        assert!(blue[2] > 100 && blue[0] == 0, "{blue:?}");
        let dim = bake(visible, 0.0, 0.25, 1.0)?;
        assert!(
            dim[0] > 25 && dim[0].saturating_add(20) < red[0],
            "{dim:?}, {red:?}"
        );
        assert_eq!(bake(visible, 0.0, 0.0, 1.0)?, [0; 3]);
        assert_eq!(bake(visible, 0.0, 1.0, 0.0)?, [0; 3]);
        assert_eq!(bake(&[receiver, screen], 0.0, 1.0, 1.0)?, [0; 3]);
        Ok(())
    }

    #[test]
    fn importance_sampling_reduces_hotspot_noise_at_the_same_ray_budget() -> TestResult {
        use crate::environment::test_probe;
        use std::sync::Arc;

        let probe = Arc::new(
            test_probe(|ray| if ray.z > 0.996 { [64.0; 3] } else { [0.0; 3] })
                .ok_or("invalid HDR hotspot")?,
        );
        let environment = EnvironmentRadiance::new(ViewportEnvironment {
            source: xrack_runtime::scene3d::EnvironmentSource::Panorama(&probe),
            orientation: Default::default(),
            horizon: 0.0,
            intensity: 1.0,
        });
        let importance = environment
            .importance
            .as_ref()
            .ok_or("missing HDR distribution")?;
        // The emitter lies entirely on +Z. Midpoint quadrature integrates the same
        // bilinearly filtered radiance, including the cubemap's solid-angle Jacobian.
        const SIDE: u32 = 256;
        let coordinate = |i: u32| (i as f32 + 0.5) * 2.0 / SIDE as f32 - 1.0;
        let mut reference = 0.0_f64;
        for row in 0..SIDE {
            for column in 0..SIDE {
                let dir = Vector3::new(coordinate(column), -coordinate(row), 1.0).normalize();
                reference += f64::from(environment.sample(dir).x * dir.z.powi(4));
            }
        }
        let reference = (reference * 4.0 / f64::from(SIDE * SIDE) / f64::from(PI)) as f32;
        assert!(reference > 0.3, "{reference}");

        const PIXELS: u32 = 256;
        const SAMPLES: u32 = 16;
        let mut cosine_error = 0.0;
        let mut mixed_error = 0.0;
        let mut mixed_mean = 0.0;
        for pixel in 0..PIXELS {
            let mut cosine = 0.0;
            let mut mixed = 0.0;
            for sample in 0..SAMPLES {
                let seed = hash(pixel ^ sample.wrapping_mul(0x9e3779b9));
                let mut cosine_seed = seed;
                let mut mixed_seed = seed;
                let dir = hemisphere(Vector3::z(), &mut cosine_seed);
                cosine += environment.sample(dir).x;
                let (dir, weight) = mixed_direction(Vector3::z(), &mut mixed_seed, importance);
                mixed += environment.sample(dir).x * weight;
            }
            cosine /= SAMPLES as f32;
            mixed /= SAMPLES as f32;
            cosine_error += (cosine - reference).powi(2);
            mixed_error += (mixed - reference).powi(2);
            mixed_mean += mixed;
        }
        assert!(
            mixed_error < 0.15 * cosine_error,
            "{mixed_error}, {cosine_error}"
        );
        assert!((mixed_mean / PIXELS as f32 - reference).abs() < 0.05 * reference);
        Ok(())
    }

    #[test]
    fn indirect_bake_samples_emitting_surfaces_and_rejects_overlapping_receivers() -> TestResult {
        let panel = plate("receiver", 0.0, false)?;
        let mut emitter = plate("red", 0.8, true)?;
        emitter.bake_role = BakeRole::Contributor;
        let options = BakeOptions {
            size: 16,
            samples: 24,
            bounces: 0,
            mode: LightmapMode::Indirect,
            ..BakeOptions::default()
        };
        let mut objects = [panel.clone(), emitter];
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
        objects[1].bake_role = BakeRole::Excluded;
        let excluded = bake_lightmaps(
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
        assert_eq!(rgb(&excluded)?, [0; 3]);
        assert!(matches!(
            bake_lightmaps(&[panel.clone(), panel], &[], [0.0; 3], 1, &[], options),
            Err(BakeError::Overlap(_, _, _))
        ));
        Ok(())
    }

    #[test]
    fn mixed_bake_partitions_direct_and_bounced_light_by_lamp() -> TestResult {
        let receiver = plate("receiver", 0.0, false)?;
        let bounce = SceneObject::from_mesh(
            String::new(),
            receiver.mesh.clone(),
            [0.0, 0.0, 0.8],
            [3.0, 3.0, 1.0],
            [180.0, 0.0, 0.0],
            Color(0xffffffff),
        )
        .ok_or("invalid bounce surface")?;
        let objects = [receiver, bounce];
        let modes = [
            LightBakeMode::All,
            LightBakeMode::Indirect,
            LightBakeMode::Disabled,
        ];
        let lights: Vec<_> = modes
            .into_iter()
            .enumerate()
            .map(|(channel, bake_mode)| {
                let mut radiance = [0.0; 3];
                radiance[channel] = 0.2;
                SceneLight {
                    source: SceneLightSource::Point {
                        position: [0.0, 0.0, 0.4],
                        inverse_range_squared: 0.0,
                    },
                    radiance,
                    bake_mode,
                    casts_shadow: true,
                    shadow: Default::default(),
                    shadow_projection: Default::default(),
                    layers: 1,
                    scope: Default::default(),
                }
            })
            .collect();
        let at_center = |bounces| -> Result<[u8; 3], Box<dyn std::error::Error>> {
            let atlas = bake_lightmaps(
                &objects,
                &lights,
                [0.0; 3],
                1,
                &[],
                BakeOptions {
                    size: 16,
                    samples: 48,
                    bounces,
                    mode: LightmapMode::Mixed,
                    ..BakeOptions::default()
                },
            )?;
            let image = xrack_assets::decode_png(&atlas.png)?;
            let offset = (10 * 20 + 10) * 4;
            Ok(std::array::from_fn(|i| image.rgba[offset + i]))
        };
        let direct = at_center(0)?;
        assert!(direct[0] > 120 && direct[1..] == [0, 0], "{direct:?}");
        let bounced = at_center(1)?;
        assert!(
            bounced[0] > direct[0] && bounced[1] > 45 && bounced[2] == 0,
            "{bounced:?}"
        );
        Ok(())
    }
}
