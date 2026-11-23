//! Offscreen forward rendering for the SceneView3D element.
mod bloom;
mod cascade;
mod deform;
mod depth;
mod depth_of_field;
mod environment;
mod fog;
mod instances;
mod lighting;
mod lightmap;
mod lut;
mod material;
mod motion;
mod occlusion;
mod particle_affectors;
mod particle_meshes;
mod particle_textures;
mod particles;
mod pipeline;
mod postprocess;
mod progressive;
pub(super) mod reflection_capture;
pub(super) mod retained_view;
mod screen_lighting;
mod shadow;
mod supersampling;
mod temporal;
mod transmission;
mod transparency;
mod volumetric_fog;

use crate::{GpuPaintContext, Node};
use deform::{Deformation, DeformationBinding, DeformationBuffers, GeometryBindings};
use depth::DepthPipelines;
use depth_of_field::FocusDepth;
use environment::{EnvironmentGpu, EnvironmentInput, LocalReflections};
use fog::Fog;
use instances::{ColorPlan, InstanceBuffers, InstanceSet};
use lighting::{LightBuffers, LightSet};
use lightmap::{LightmapBindings, LightmapTextures};
use material::{MaterialBindings, Materials};
use motion::{MotionInputs, MotionPass};
use occlusion::{AmbientOcclusion, OcclusionSettings, OcclusionSource};
use particles::{ParticleDraw, ParticlePipelines};
use pipeline::{ColorOutput, ColorPass, Pipelines, Surface, VolumeMode};
use postprocess::{PostProcess, PostSettings};
use reflection_capture::{CapturedCube, Reflections, capture_node};
use retained_view::AaSample;
use screen_lighting::{
    IndirectSettings, ReflectionSettings, ScreenLighting, ScreenSettings, ScreenTarget,
};
use shadow::{ShadowRequest, Shadows};
use std::{
    cell::OnceCell,
    collections::HashMap,
    ops::Range,
    sync::{Arc, Weak},
};
use transmission::{OpaqueDepth, Transmission};
use transparency::Transparency;
use wgpu::util::DeviceExt;
use xrack_runtime::{
    Color, Value,
    scene3d::{
        Mesh3D, MeshAlphaMode, MeshData, MeshVertex, ReflectionSource, SceneCamera, SceneObject,
        SceneVisibility,
    },
};

pub(super) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub(super) use supersampling::texture_extent;

const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const INSTANCE_STRIDE: u64 = 68;

/// Cumulative counters for SceneView3D color passes on this renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneDrawStats {
    /// Updated scene-captured reflection cubemaps.
    pub reflection_cubemap_updates: u64,
    /// Captured cubemap faces before roughness filtering.
    pub reflection_capture_faces: u64,
    /// Full-screen mip face passes used to prefilter scene cubemaps.
    pub reflection_prefilter_passes: u64,
    /// Viewports that recorded their 3D render pass or background clear.
    pub viewport_renders: u64,
    /// Viewports composited from a completed offscreen color texture.
    pub viewport_cache_hits: u64,
    /// Color textures allocated for retained viewports.
    pub viewport_cache_allocations: u64,
    /// Dimensions of the most recently painted viewport's composited texture.
    pub last_texture_size: [u32; 2],
    /// Dimensions of that viewport's color raster before an area resolve.
    pub last_raster_size: [u32; 2],
    /// Cumulative scene color raster area, counted before MSAA sample expansion.
    pub raster_pixels: u64,
    /// Prepared light buffers uploaded for changed lights or shadow assignments.
    pub light_buffer_uploads: u64,
    /// Instances rejected by the camera frustum, excluding shadow queries.
    pub culled_instances: u64,
    /// Instances excluded by the viewport or named camera's content mask.
    pub layer_filtered_instances: u64,
    /// Color draw calls for mesh parts. Skybox and tone mapping are excluded.
    pub color_draws: u64,
    /// Submitted instance count summed over the color draws.
    pub color_instances: u64,
    /// Submitted vertices summed over all instances and color draws.
    pub color_vertices: u64,
    /// Instanced sprite draws, independent of the number of particles.
    pub particle_draws: u64,
    /// Live sprites submitted to the GPU before hardware clipping.
    pub particle_instances: u64,
    /// Decoded sprite sheets uploaded to GPU textures.
    pub particle_texture_uploads: u64,
    /// Model surface sampling tables uploaded for mesh emitters.
    pub particle_mesh_uploads: u64,
    /// Fields uploaded when the visible affector list changes.
    pub particle_affector_uploads: u64,
    /// Submitted intervals in instanced ribbon trails.
    pub particle_trail_segments: u64,
    /// Objects for which a replacement mesh was selected, before camera culling.
    pub lod_objects: u64,
    /// Color-only instance buffer uploads needed to keep fragmented draws batched.
    pub color_compaction_uploads: u64,
    /// Uploaded static irradiance atlas textures.
    pub lightmap_uploads: u64,
    /// Opaque draw calls using a baked irradiance atlas.
    pub lightmapped_draws: u64,
    /// Viewports resolved from a four-sample color attachment.
    pub multisample_resolves: u64,
    /// Area resolves from a larger scene raster to the nominal viewport.
    pub supersample_resolves: u64,
    /// Source pixels consumed by the supersample resolves.
    pub supersample_pixels: u64,
    /// Subpixel frames accumulated for stationary viewports.
    pub progressive_aa_samples: u64,
    /// Subpixel samples rendered using two-frame temporal filtering.
    pub temporal_aa_samples: u64,
    /// Temporal samples blended with the previous raw frame.
    pub temporal_aa_blends: u64,
    /// Geometry submissions that produce visible-surface motion vectors.
    pub motion_vector_draws: u64,
    /// Frames that ran the motion reprojection resolve with scene history.
    pub temporal_reprojections: u64,
    /// Viewports that evaluated screen-space reflections.
    pub screen_reflection_resolves: u64,
    /// Pixels submitted to the reduced-resolution reflection ray search.
    pub screen_reflection_pixels: u64,
    /// Viewports that evaluated screen-space indirect lighting.
    pub screen_indirect_resolves: u64,
    /// Pixels submitted to diffuse hemisphere tracing.
    pub screen_indirect_pixels: u64,
    /// Lower-resolution levels uploaded for material textures.
    pub material_mip_levels_uploaded: u64,
    /// Viewports that retained opaque color for transmission.
    pub transmission_captures: u64,
    /// Downsampling passes used by frosted transmission.
    pub transmission_mip_passes: u64,
    /// Opaque depth captures for visible volume refraction.
    pub volume_depth_captures: u64,
    /// Nearest opaque-depth resolves shared by screen reflections and refraction.
    pub opaque_depth_captures: u64,
    /// Weighted transparency composites recorded for viewports with visible blended geometry.
    pub transparency_composites: u64,
    /// Depth-only mesh draws recorded for ambient occlusion.
    pub occlusion_depth_draws: u64,
    /// Half-resolution ambient occlusion evaluation passes.
    pub occlusion_evaluations: u64,
    /// Pixels of the occlusion maps evaluated this far.
    pub occlusion_pixels: u64,
    /// Scene-linear HDR viewports resolved into the 2D compositor.
    pub hdr_resolves: u64,
    /// Viewports with color adjustments in the final display conversion.
    pub color_adjustment_resolves: u64,
    /// Viewports with a LUT applied in the final display conversion.
    pub lut_resolves: u64,
    /// Number of decoded LUTs uploaded to the GPU.
    pub lut_uploads: u64,
    /// Bloom extraction, reduction and expansion passes.
    pub bloom_passes: u64,
    /// Total pixels filtered below full resolution by the bloom pyramid.
    pub bloom_pixels: u64,
    /// Bloom texture allocations, including new viewport sizes and levels.
    pub bloom_target_allocations: u64,
    /// Depth of field reduction, filtering and composition passes.
    pub depth_of_field_passes: u64,
    /// Pixels filtered by the half-resolution depth of field passes.
    pub depth_of_field_pixels: u64,
}

impl SceneDrawStats {
    fn submit(&mut self, range: &Range<u32>, vertices: &Range<u32>) {
        let instances = u64::from(range.end - range.start);
        let vertices = u64::from(vertices.end - vertices.start);
        self.color_draws = self.color_draws.saturating_add(1);
        self.color_instances = self.color_instances.saturating_add(instances);
        self.color_vertices = self
            .color_vertices
            .saturating_add(instances.saturating_mul(vertices));
    }
}

#[derive(Default)]
pub(super) struct SceneRenderer {
    reflections: Reflections,
    scene: Option<SceneGpu>,
    depth: [Option<Attachment>; 2],
    sampled_depth: [Option<Attachment>; 2],
    multisampled_color: [Option<Attachment>; 2],
    continued_color: [Option<Attachment>; 2],
    transmission: Transmission,
    opaque_depth: OpaqueDepth,
    transparency: Transparency,
    particles: ParticlePipelines,
    postprocess: PostProcess,
    motion: MotionPass,
    screen_lighting: ScreenLighting,
    pub stats: SceneDrawStats,
}

enum DrawTarget<'a> {
    Viewport(&'a [Option<CapturedCube>], Option<&'a AaSample>),
    Reflection,
}

pub(super) fn has_scene_reflections(node: &Node) -> bool {
    matches!(node.properties.get("reflection-probes"), Some(Value::SceneReflections(probes))
        if probes.iter().any(|probe| matches!(probe.source, ReflectionSource::Scene(_))))
}

struct GpuPart {
    vertices: Range<u32>,
    material: MaterialBindings,
    surface: Surface,
    mask: bool,
}

struct MeshBuffer {
    vertices: wgpu::Buffer,
    tangents: Option<wgpu::Buffer>,
    skin_weights: Option<wgpu::Buffer>,
    morph: Option<wgpu::Buffer>,
    lightmap_uv: OnceCell<wgpu::Buffer>,
    parts: Vec<GpuPart>,
}

impl MeshBuffer {
    fn bind(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        if let Some(tangents) = &self.tangents {
            pass.set_vertex_buffer(2, tangents.slice(..));
        }
        self.bind_skin(pass);
    }

    fn bind_lightmap_uv(
        &self,
        device: &wgpu::Device,
        pass: &mut wgpu::RenderPass<'_>,
        mesh: &Mesh3D,
    ) {
        let uv = self.lightmap_uv.get_or_init(|| {
            let bytes: Vec<_> = mesh
                .lightmap_uvs()
                .iter()
                .flatten()
                .copied()
                .flat_map(f32::to_ne_bytes)
                .collect();
            buffer(
                device,
                "XRack lightmap UV chart",
                &bytes,
                wgpu::BufferUsages::VERTEX,
            )
        });
        pass.set_vertex_buffer(3, uv.slice(..));
    }

    fn bind_skin(&self, pass: &mut wgpu::RenderPass<'_>) {
        if let Some(weights) = &self.skin_weights {
            pass.set_vertex_buffer(3, weights.slice(..));
        }
    }
}

struct MeshCache {
    box_mesh: MeshBuffer,
    sphere_mesh: MeshBuffer,
    materials: Materials,
    imported: HashMap<usize, (Weak<MeshData>, MeshBuffer)>,
    uploads: u64,
}

struct SceneGpu {
    pipelines: Pipelines,
    meshes: MeshCache,
    lightmaps: LightmapTextures,
    lights: LightBuffers,
    shadows: Shadows,
    environment: EnvironmentGpu,
    background: wgpu::TextureView,
    background_sampler: wgpu::Sampler,
    background_depth: wgpu::Buffer,
    instances: InstanceBuffers,
    deformations: DeformationBuffers,
    occlusion: AmbientOcclusion,
}

impl SceneGpu {
    fn new(device: &wgpu::Device, queue: &wgpu::Queue) -> Self {
        let pipelines = Pipelines::new(device);
        let mut materials = Materials::new(device, queue, pipelines.materials.clone());
        let default = materials.bind(device, queue, None, None);
        let builtin = |mesh: &Mesh3D| {
            mesh_buffer(
                device,
                mesh,
                vec![GpuPart {
                    vertices: 0..mesh.vertices().len() as u32,
                    material: default.clone(),
                    mask: false,
                    surface: Surface {
                        blend: false,
                        transmission: false,
                        volume: VolumeMode::Off,
                        double_sided: true,
                        normal_mapped: false,
                        clearcoat: false,
                    },
                }],
            )
        };
        Self {
            shadows: Shadows::new(
                device,
                pipelines.materials[0].clone(),
                pipelines.deformations.clone(),
            ),
            background: attachment_view(
                device,
                [1, 1],
                COLOR_FORMAT,
                1,
                wgpu::TextureUsages::TEXTURE_BINDING,
                "XRack 3D fallback transmission background",
            ),
            background_depth: buffer(
                device,
                "XRack 3D no opaque depth snapshot",
                &1.0f32.to_ne_bytes(),
                wgpu::BufferUsages::STORAGE,
            ),
            background_sampler: device.create_sampler(&wgpu::SamplerDescriptor {
                label: Some("XRack 3D transmission sampler"),
                min_filter: wgpu::FilterMode::Linear,
                mag_filter: wgpu::FilterMode::Linear,
                mipmap_filter: wgpu::MipmapFilterMode::Linear,
                ..Default::default()
            }),
            environment: EnvironmentGpu::new(device, pipelines.environment.clone()),
            instances: InstanceBuffers::new(device),
            deformations: DeformationBuffers::new(pipelines.deformations.clone()),
            occlusion: AmbientOcclusion::new(device),
            pipelines,
            lights: LightBuffers::default(),
            lightmaps: LightmapTextures::default(),
            meshes: MeshCache {
                box_mesh: builtin(&Mesh3D::Box),
                sphere_mesh: builtin(&Mesh3D::Sphere),
                materials,
                imported: HashMap::new(),
                uploads: 0,
            },
        }
    }
}

impl MeshCache {
    fn prune(&mut self) {
        self.imported
            .retain(|_, (data, _)| data.strong_count() != 0);
        self.materials.prune();
    }

    fn get(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, kind: &Mesh3D) -> &MeshBuffer {
        match kind {
            Mesh3D::Box => &self.box_mesh,
            Mesh3D::Sphere => &self.sphere_mesh,
            Mesh3D::Imported(data) => {
                let key = Arc::as_ptr(data) as usize;
                let materials = &mut self.materials;
                let uploads = &mut self.uploads;
                &self
                    .imported
                    .entry(key)
                    .or_insert_with(|| {
                        *uploads = uploads.saturating_add(1);
                        let mut bindings = HashMap::new();
                        let parts = kind
                            .parts()
                            .iter()
                            .map(|part| {
                                let key = (
                                    part.material,
                                    part.volume_inverse
                                        .map(|rows| rows.map(|row| row.map(f32::to_bits))),
                                );
                                let (material, mask, surface) =
                                    bindings.entry(key).or_insert_with(|| {
                                        let material = &kind.materials()[part.material];
                                        let binding = materials.bind(
                                            device,
                                            queue,
                                            Some(material),
                                            part.volume_inverse,
                                        );
                                        let pbr = material.pbr.as_ref();
                                        let clearcoat = binding.clearcoat.is_some();
                                        (
                                            binding,
                                            matches!(material.alpha_mode, MeshAlphaMode::Mask(_)),
                                            Surface {
                                                blend: material.alpha_mode == MeshAlphaMode::Blend,
                                                transmission: pbr
                                                    .and_then(|pbr| pbr.transmission_blur())
                                                    .is_some(),
                                                volume: pbr.map_or(VolumeMode::Off, |pbr| {
                                                    if pbr.volume().is_none() {
                                                        VolumeMode::Off
                                                    } else if pbr.refraction_channels().dispersive()
                                                    {
                                                        VolumeMode::Dispersion
                                                    } else {
                                                        VolumeMode::Volume
                                                    }
                                                }),
                                                double_sided: material.is_double_sided(),
                                                normal_mapped: pbr
                                                    .and_then(|pbr| pbr.tangent_normal_map())
                                                    .is_some(),
                                                clearcoat,
                                            },
                                        )
                                    });
                                GpuPart {
                                    vertices: part.vertices.start as u32..part.vertices.end as u32,
                                    material: material.clone(),
                                    surface: *surface,
                                    mask: *mask,
                                }
                            })
                            .collect();
                        (Arc::downgrade(data), mesh_buffer(device, kind, parts))
                    })
                    .1
            }
        }
    }
}

impl SceneRenderer {
    pub(super) fn begin_frame(&mut self) {
        self.reflections.begin_frame();
    }

    pub(super) fn finish_frame(&mut self, submitted: bool) {
        self.reflections.finish_frame(submitted);
        if let Some(scene) = &mut self.scene {
            scene
                .environment
                .retain_captured_bindings(|view| self.reflections.contains_view(view));
        }
    }

    pub(super) fn discard_recorded_passes(&mut self) {
        if let Some(scene) = &mut self.scene {
            scene.shadows.discard_recorded_layers();
            scene.occlusion.discard_recorded_maps();
        }
    }

    pub(super) fn shadow_passes(&self) -> u64 {
        self.scene.as_ref().map_or(0, |scene| scene.shadows.passes)
    }

    pub(super) fn environment_uploads(&self) -> u64 {
        self.scene
            .as_ref()
            .map_or(0, |scene| scene.environment.uploads)
    }

    pub(super) fn mesh_uploads(&self) -> u64 {
        self.scene.as_ref().map_or(0, |scene| scene.meshes.uploads)
    }

    pub(super) fn skin_uploads(&self) -> u64 {
        self.scene
            .as_ref()
            .map_or(0, |scene| scene.deformations.skin_uploads)
    }

    pub(super) fn morph_uploads(&self) -> u64 {
        self.scene
            .as_ref()
            .map_or(0, |scene| scene.deformations.morph_uploads)
    }

    pub(super) fn instance_uploads(&self) -> u64 {
        self.scene
            .as_ref()
            .map_or(0, |scene| scene.instances.uploads)
    }

    pub(super) fn camera_for_view(node: &Node, viewport: [f64; 2]) -> Option<SceneCamera> {
        let camera_id = node.text("camera").unwrap_or_default();
        let camera = if camera_id.is_empty() {
            SceneCamera::new(
                node.number("camera-yaw").unwrap_or(25.0),
                node.number("camera-pitch").unwrap_or(24.0),
                node.number("camera-distance").unwrap_or(10.0),
                viewport,
            )
        } else {
            match node.properties.get("model") {
                Some(Value::SceneObjects(model)) => model.camera(camera_id, viewport),
                _ => None,
            }
        };
        camera.map(|camera| camera.with_layer_mask(node.int("layer-mask")))
    }

    pub(super) fn draw(
        &mut self,
        node: &Node,
        ctx: GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        path: &[usize],
        camera: Option<SceneCamera>,
        (cubes, sample): (&[Option<CapturedCube>], Option<&AaSample>),
    ) {
        self.stats.viewport_renders = self.stats.viewport_renders.saturating_add(1);
        self.draw_target(
            node,
            ctx,
            queue,
            path,
            camera,
            DrawTarget::Viewport(cubes, sample),
        );
    }

    pub(super) fn capture_reflections(
        &mut self,
        node: &Node,
        (device, queue, encoder): (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder),
        path: &[usize],
        camera: SceneCamera,
        generation: u64,
        paint: crate::PaintContext<'_>,
    ) -> (Vec<Option<CapturedCube>>, bool) {
        let (Some(Value::SceneObjects(model)), Some(Value::SceneReflections(probes))) = (
            node.properties.get("model"),
            node.properties.get("reflection-probes"),
        ) else {
            return (Vec::new(), false);
        };
        let geometry = model.geometry(&camera);
        let scene = self
            .scene
            .get_or_insert_with(|| SceneGpu::new(device, queue));
        let Some(slots) =
            scene
                .environment
                .assignments(probes, geometry.objects(), camera.layer_mask())
        else {
            return (Vec::new(), false);
        };
        let used = slots.iter().fold(0u16, |mask, &slot| mask | (1 << slot));
        let source = capture_node(node);
        let mut reflections = std::mem::take(&mut self.reflections);
        let mut needs_redraw = false;
        let cubes = probes
            .iter()
            .enumerate()
            .map(|(index, probe)| {
                let ReflectionSource::Scene(capture) = probe.source else {
                    return None;
                };
                if used & (1 << (index + 1)) == 0 {
                    return None;
                }
                let layers = probe.layers & camera.layer_mask();
                let prepared = reflections.prepare(
                    device,
                    &source,
                    (path, index, generation),
                    probe,
                    capture,
                    layers,
                );
                needs_redraw |= prepared.needs_redraw;
                if let Some(step) = prepared.step {
                    for face in step.faces.clone() {
                        let face_view = step.cube.face(face);
                        let face_camera =
                            SceneCamera::reflection_face(step.cube.position, face as usize, layers);
                        let size = [step.cube.size(); 2];
                        self.draw_target(
                            &step.scene,
                            GpuPaintContext {
                                paint,
                                device,
                                encoder: &mut *encoder,
                                target: face_view,
                                size,
                                format: HDR_FORMAT,
                            },
                            queue,
                            path,
                            face_camera,
                            DrawTarget::Reflection,
                        );
                    }
                    let passes = reflections.prefilter(device, encoder, &step);
                    self.stats.reflection_cubemap_updates = self
                        .stats
                        .reflection_cubemap_updates
                        .saturating_add(u64::from(step.complete));
                    self.stats.reflection_capture_faces = self
                        .stats
                        .reflection_capture_faces
                        .saturating_add(step.faces.len() as u64);
                    self.stats.reflection_prefilter_passes = self
                        .stats
                        .reflection_prefilter_passes
                        .saturating_add(passes);
                }
                prepared.output
            })
            .collect();
        self.reflections = reflections;
        (cubes, needs_redraw)
    }

    fn draw_target(
        &mut self,
        node: &Node,
        mut ctx: GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        path: &[usize],
        camera: Option<SceneCamera>,
        kind: DrawTarget<'_>,
    ) {
        let reflection = matches!(kind, DrawTarget::Reflection);
        let (scene_cubes, sample) = match kind {
            DrawTarget::Viewport(cubes, sample) => (cubes, sample),
            DrawTarget::Reflection => (&[][..], None),
        };
        let model = match node.properties.get("model") {
            Some(Value::SceneObjects(model)) => Some(model),
            _ => None,
        };
        let geometry = model
            .zip(camera.as_ref())
            .map(|(model, camera)| model.geometry(camera));
        let objects = geometry
            .as_ref()
            .map(|geometry| geometry.objects())
            .filter(|objects| !objects.is_empty());
        let object_count = objects.map_or(0, |objects| objects.len());
        let layer_filter = geometry
            .as_ref()
            .zip(camera.as_ref())
            .and_then(|(geometry, camera)| {
                let layers = geometry.layer_visibility(camera.layer_mask());
                if reflection && let Some(casters) = geometry.reflection_visibility() {
                    return Some(layers.as_ref().map_or_else(
                        || casters.clone(),
                        |layers| SceneVisibility::intersect(layers, &casters),
                    ));
                }
                layers
            });
        let layer_count = layer_filter
            .as_ref()
            .map_or(object_count, |layers| layers.visible_count());
        self.stats.layer_filtered_instances = self
            .stats
            .layer_filtered_instances
            .saturating_add((object_count - layer_count) as u64);
        let has_geometry = layer_count != 0;
        let particles = camera
            .as_ref()
            .filter(|_| !reflection)
            .and_then(|camera| ParticleDraw::sample(node, camera));
        let has_particles = particles.is_some();
        let has_drawables = has_geometry || has_particles;
        self.stats.lod_objects = self.stats.lod_objects.saturating_add(
            geometry
                .as_ref()
                .map_or(0, |geometry| geometry.lod_objects as u64),
        );
        if !reflection && let Some(scene) = &mut self.scene {
            scene.meshes.prune();
        }
        let background = background(node);
        let [r, g, b] = linear_rgb(background);
        let clear = wgpu::Color { r, g, b, a: 1.0 };
        let environment = EnvironmentInput::from_node(node);
        let skybox = environment.as_ref().is_some_and(|input| input.skybox);
        let fog_volumes = volumetric_fog::prepare(node, camera.as_ref());
        let fog_background = !fog_volumes.is_empty();

        let probes = match node.properties.get("reflection-probes") {
            Some(Value::SceneReflections(probes)) if !probes.is_empty() => Some(probes),
            _ => None,
        };
        let mut post = PostSettings::from_node(
            node,
            camera.is_some() && (environment.is_some() || probes.is_some() && has_geometry),
            has_geometry,
        );
        let retain_depth = post.focus.is_some();
        let raster_size = if camera.is_some() && (has_drawables || skybox || fog_background) {
            supersampling::extent(
                ctx.device,
                ctx.size,
                node.float("supersampling").unwrap_or(1.0),
            )
        } else {
            ctx.size
        };
        if !reflection {
            self.stats.last_texture_size = ctx.size;
            self.stats.last_raster_size = raster_size;
            self.stats.raster_pixels = self
                .stats
                .raster_pixels
                .saturating_add(u64::from(raster_size[0]) * u64::from(raster_size[1]));
        }
        let supersampled = raster_size != ctx.size;
        let motion_history = match sample {
            Some(AaSample::Temporal(sample)) => sample.motion.as_ref(),
            _ => None,
        };
        let Some(camera) =
            camera.filter(|_| has_drawables || environment.is_some() || fog_background)
        else {
            if let Some(history) = motion_history {
                *history.borrow_mut() = Default::default();
            }
            let post_target = (!reflection
                && (supersampled || post.needs_target() || sample.is_some()))
            .then(|| {
                self.postprocess
                    .target(ctx.device, raster_size, post.hdr, supersampled)
            });
            let target = post_target.as_ref().unwrap_or(ctx.target);
            viewport_pass(ctx.encoder, target, None, None, false, Some(clear), false);
            if let Some(source) = &post_target {
                self.resolve_postprocessing(
                    &mut ctx,
                    queue,
                    (source, raster_size),
                    None,
                    (post, sample, None),
                );
            }
            return;
        };
        let raster_camera = sample.map_or(camera, |sample| {
            camera.with_clip_offset(sample.clip_offset(raster_size))
        });
        let multisampling = !supersampled
            && (has_drawables || fog_background)
            && node.boolean("multisampling", false);
        let scene = self
            .scene
            .get_or_insert_with(|| SceneGpu::new(ctx.device, queue));
        let selected = node.text("selected-object").unwrap_or_default();
        let instances = objects
            .filter(|_| has_geometry)
            .map(|objects| scene.instances.get(ctx.device, objects, selected));
        let culled = geometry
            .as_ref()
            .zip(instances.as_ref())
            .filter(|_| node.properties.get("frustum-culling") != Some(&Value::Bool(false)))
            .map(|(model, set)| {
                scene
                    .instances
                    .visibility(model, set, &raster_camera, layer_filter.as_ref())
            })
            .or_else(|| layer_filter.clone());
        let all = SceneVisibility::all(object_count);
        let visibility = culled.as_deref().unwrap_or(&all);
        let has_custom_blend = particles
            .as_ref()
            .is_some_and(ParticleDraw::has_custom_blend);
        let weighted = node.boolean("order-independent-transparency", false)
            && (particles
                .as_ref()
                .is_some_and(ParticleDraw::has_source_over)
                || instances
                    .as_ref()
                    .is_some_and(|set| set.has_visible_transparency(visibility)));
        let mut effects = ScreenSettings::default();
        if !reflection {
            let visible_material = |receives: &dyn Fn(&Mesh3D) -> bool| {
                objects.is_some_and(|objects| {
                    objects.iter().enumerate().any(|(index, object)| {
                        visibility.contains(index as u32) && receives(&object.mesh)
                    })
                })
            };
            effects.reflection = ReflectionSettings::from_node(node)
                .filter(|settings| visible_material(&|mesh| settings.receives(mesh)));
            effects.indirect =
                IndirectSettings::from_node(node).filter(|_| visible_material(&receives_occlusion));
        }
        let visible_transmission = instances
            .as_ref()
            .and_then(|set| set.visible_transmission(visibility));
        let needs_volume_depth = visible_transmission.is_some_and(|(_, _, volume)| volume);
        let depth_snapshot = (needs_volume_depth || multisampling && effects.active())
            .then(|| self.opaque_depth.prepare(ctx.device, raster_size))
            .flatten();
        if multisampling && effects.active() {
            if depth_snapshot.is_some() {
                // Signed reflection deltas are blended into the original MSAA samples.
                // Keep radiance linear without changing the viewport's tone curve.
                post.require_linear_intermediate();
            } else {
                effects = ScreenSettings::default();
            }
        }
        let surfaces = effects.active().then(|| {
            self.screen_lighting.surfaces(
                ctx.device,
                raster_size,
                multisampling,
                effects.indirect.is_some(),
            )
        });
        let lights = LightSet::from_node(node, model.map(Arc::as_ref), camera.layer_mask());
        let scattering = volumetric_fog::Scattering::for_view(
            node,
            fog_background,
            fog_background
                && lights
                    .lights()
                    .iter()
                    .any(|light| light.radiance.iter().any(|&channel| channel > 0.0)),
        );
        let output = ColorOutput {
            hdr: reflection || post.uses_linear_intermediate(),
            reverse_winding: reflection,
            multisampling,
            pass: ColorPass::Opaque,
            screen_reflections: false,
            screen_indirect_light: false,
            lightmapped: false,
            fog: scattering.mode(&fog_volumes),
        };
        let post_target =
            (!reflection && (supersampled || post.needs_target() || sample.is_some())).then(|| {
                self.postprocess.target(
                    ctx.device,
                    raster_size,
                    post.uses_linear_intermediate(),
                    supersampled,
                )
            });
        let target = post_target.as_ref().unwrap_or(ctx.target);
        let capture = visible_transmission.map(|(blur, _, _)| {
            self.transmission
                .prepare(ctx.device, raster_size, output, blur)
        });
        let capturing = capture.is_some();
        let continue_opaque = visible_transmission
            .is_some_and(|(_, writes_depth, _)| !weighted || writes_depth || has_custom_blend);
        let captured_depth = depth_snapshot.as_ref().filter(|_| needs_volume_depth);
        let transparency = weighted.then(|| {
            self.transparency.prepare(
                ctx.device,
                raster_size,
                output,
                retain_depth || captured_depth.is_some() || surfaces.is_some(),
            )
        });
        let (depth, multisampled) = if let Some(frame) = &transparency {
            (frame.depth, frame.multisampled_color)
        } else {
            let preserve_depth = retain_depth || capturing || surfaces.is_some();
            let pool = if preserve_depth {
                &mut self.sampled_depth
            } else {
                &mut self.depth
            };
            let depth = Attachment::get(
                &mut pool[usize::from(output.multisampling)],
                ctx.device,
                raster_size,
                DEPTH_FORMAT,
                output.samples(),
                preserve_depth,
            );
            let multisampled = output.multisampling.then(|| {
                let persistent = capturing || surfaces.is_some();
                let colors = if persistent {
                    &mut self.continued_color
                } else {
                    &mut self.multisampled_color
                };
                Attachment::get(
                    &mut colors[usize::from(output.hdr)],
                    ctx.device,
                    raster_size,
                    output.format(),
                    output.samples(),
                    persistent,
                )
            });
            (depth, multisampled)
        };
        let focus_depth = retain_depth.then(|| FocusDepth {
            view: depth.clone(),
            camera,
            multisampled: output.multisampling,
        });
        self.stats.culled_instances = self
            .stats
            .culled_instances
            .saturating_add((layer_count - visibility.visible_count()) as u64);
        let probe_slots =
            probes
                .zip(objects)
                .filter(|_| has_geometry)
                .and_then(|(probes, objects)| {
                    scene
                        .environment
                        .assignments(probes, objects, camera.layer_mask())
                });
        let has_lightmaps = instances
            .as_ref()
            .is_some_and(|set| set.lightmap_parameters.is_some());
        let full_visibility = (culled.is_none() && (probe_slots.is_some() || has_lightmaps))
            .then(|| Arc::new(SceneVisibility::all(object_count)));
        let mut color_plan_for = |slots, lightmaps| {
            instances
                .as_ref()
                .zip(culled.as_ref().or(full_visibility.as_ref()))
                .zip(objects)
                .and_then(|((set, visibility), objects)| {
                    scene.instances.color_plan(
                        ctx.device,
                        set,
                        visibility,
                        objects,
                        weighted,
                        (slots, lightmaps),
                    )
                })
        };
        let color_plan = color_plan_for(probe_slots.clone(), true);
        let ao = OcclusionSettings::from_node(node).filter(|_| {
            if effects
                .indirect
                .is_some_and(|settings| settings.occlusion > 0.0)
            {
                return false;
            }
            (lights.has_ambient()
                || environment
                    .as_ref()
                    .is_some_and(EnvironmentInput::contributes_light)
                || probe_slots.is_some())
                && instances
                    .as_ref()
                    .zip(objects)
                    .is_some_and(|(set, objects)| {
                        set.color_draws(color_plan.as_deref(), visibility)
                            .any(|draw| receives_occlusion(&objects[draw.source as usize].mesh))
                    })
        });
        let depth_plan = if ao.is_some() && (probe_slots.is_some() || has_lightmaps) {
            color_plan_for(None, false)
        } else {
            color_plan.clone()
        };
        self.stats.color_compaction_uploads = scene.instances.color_uploads;
        let instance_buffer = instances
            .as_ref()
            .map_or(&scene.instances.empty, |set| &set.buffer);
        let mut uniforms = Vec::with_capacity(192 + volumetric_fog::VOLUME_UNIFORM_BYTES);
        append_floats(
            &mut uniforms,
            raster_camera.clip_rows().into_iter().flatten(),
        );
        let [x, y, z] = camera.eye_position();
        append_floats(
            &mut uniforms,
            [x, y, z, if camera.is_orthographic() { 1.0 } else { 0.0 }],
        );
        append_floats(&mut uniforms, camera.depth_row());
        let fog = Fog::from_node(node, [r, g, b]);
        append_floats(&mut uniforms, fog.color_density);
        append_floats(&mut uniforms, fog.depth);
        append_floats(&mut uniforms, fog.height);
        let [clip_near, clip_far] = camera.depth_limits();
        let weight_scale = if weighted {
            // Weighting is a heuristic, independent of the actual projection far plane.
            let far = if clip_far.is_finite() {
                clip_far
            } else {
                (clip_near + 100.0).max(100.0)
            };
            let near = (clip_near).max(0.001);
            (near.sqrt() * far.sqrt()).recip()
        } else {
            0.0
        };
        let particles = if let Some(draw) = particles {
            let batches = self.particles.prepare(
                draw,
                (ctx.device, queue),
                raster_camera,
                weight_scale,
                weighted,
            );
            self.stats.particle_draws += (batches.direct.len() + batches.weighted.len()) as u64;
            self.stats.particle_instances += batches
                .direct
                .iter()
                .chain(&batches.weighted)
                .map(|batch| u64::from(batch.instances))
                .sum::<u64>();
            self.stats.particle_texture_uploads = self.particles.texture_uploads();
            self.stats.particle_mesh_uploads = self.particles.mesh_uploads();
            self.stats.particle_affector_uploads = self.particles.affector_uploads();
            self.stats.particle_trail_segments += batches
                .direct
                .iter()
                .chain(&batches.weighted)
                .map(|batch| batch.trail_segments())
                .sum::<u64>();
            batches
        } else {
            Default::default()
        };
        let mut draw_particles = |pass: &mut wgpu::RenderPass<'_>, accumulation| {
            let batches = if accumulation {
                &particles.weighted
            } else {
                &particles.direct
            };
            self.particles.draw(
                ctx.device,
                pass,
                batches,
                output,
                accumulation,
                particles.affectors.as_ref(),
            );
        };
        let specular_aa = f32::from(u8::from(node.boolean("specular-aa", false)));
        append_floats(&mut uniforms, [weight_scale, specular_aa, 0.0, 0.0]);
        let [width, height] = raster_size.map(|v| v as f32);
        let [strength, radius] = ao.map_or([0.0, 0.0], |parameters| {
            [parameters.strength, parameters.radius]
        });
        append_floats(&mut uniforms, [strength, radius, width, height]);
        append_floats(&mut uniforms, raster_camera.clip_lens());
        volumetric_fog::write(
            &mut uniforms,
            &fog_volumes,
            &camera,
            [r, g, b],
            skybox,
            &scattering,
            if output.fog.noisy() {
                node.int("fog-volume-density-samples")
                    .unwrap_or(32)
                    .clamp(8, 64) as u32
            } else {
                0
            },
        );
        let uniform_buffer = buffer(
            ctx.device,
            "XRack 3D camera",
            &uniforms,
            wgpu::BufferUsages::UNIFORM,
        );
        let request = objects.filter(|_| has_geometry).and_then(|objects| {
            ShadowRequest::for_view(
                node,
                objects,
                lights.lights(),
                camera,
                path,
                layer_filter.as_ref(),
                if scattering.strength > 0.0 {
                    volumetric_fog::receiver_corners(&fog_volumes)
                } else {
                    None
                },
            )
        });
        let has_shadows = request.is_some();
        let deformations = scene.deformations.prepare(
            ctx.device,
            queue,
            &mut scene.meshes,
            objects.map_or(&[][..], Arc::as_ref),
            |index, object| {
                visibility.contains(index)
                    || has_shadows
                        && object.casts_shadow
                        && layer_filter
                            .as_ref()
                            .is_none_or(|filter| filter.contains(index))
            },
        );
        let geometry = GeometryBindings {
            instances: instance_buffer,
            deformations: &deformations,
        };
        let ao_frame =
            ao.zip(instances.as_ref())
                .zip(objects)
                .map(|((settings, instances), objects)| {
                    scene.occlusion.prepare(
                        ctx.device,
                        raster_size,
                        OcclusionSource {
                            instances,
                            visibility: culled.as_ref(),
                            objects,
                            deformation: &deformations,
                        },
                        raster_camera,
                        settings,
                    )
                });
        let draws = instances
            .as_deref()
            .zip(objects)
            .map(|(set, objects)| MeshColorDraws {
                set,
                objects,
                visibility,
                geometry,
                plan: color_plan.as_deref(),
            });
        let sampler = scene.shadows.sampler.clone();
        let shadow = scene
            .shadows
            .prepare(&mut ctx, queue, &mut scene.meshes, geometry, request);
        let lights = scene.lights.get(ctx.device, lights, shadow.slots);
        self.stats.light_buffer_uploads = scene.lights.uploads;
        if let Some((frame, settings)) = ao_frame.as_ref().zip(ao) {
            if frame.depth_dirty
                && let Some(draws) = &draws
            {
                let depth = scene.occlusion.depth_pipeline.get_or_insert_with(|| {
                    DepthPipelines::new(
                        ctx.device,
                        &scene.pipelines.materials[0],
                        &scene.pipelines.deformations,
                        wgpu::DepthBiasState::default(),
                    )
                });
                let depth_draws = MeshColorDraws {
                    plan: depth_plan.as_deref(),
                    ..*draws
                };
                let commands = depth_draws.depth_prepass(
                    (&mut scene.meshes, depth),
                    (ctx.device, queue),
                    ctx.encoder,
                    &frame.depth,
                    &uniform_buffer,
                );
                self.stats.occlusion_depth_draws =
                    self.stats.occlusion_depth_draws.saturating_add(commands);
            }
            if frame.occlusion_dirty {
                scene
                    .occlusion
                    .evaluate(ctx.device, ctx.encoder, frame, raster_camera, settings);
                self.stats.occlusion_evaluations =
                    self.stats.occlusion_evaluations.saturating_add(1);
                self.stats.occlusion_pixels = self
                    .stats
                    .occlusion_pixels
                    .saturating_add(frame.size.into_iter().map(u64::from).product());
            }
        }
        let bind_background = |background: &wgpu::TextureView,
                               texture_sampler: &wgpu::Sampler,
                               captured_depth: &wgpu::Buffer| {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("XRack 3D globals"),
                layout: &scene.pipelines.globals,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniform_buffer.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 1,
                        resource: lights.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 2,
                        resource: shadow.matrices.as_entire_binding(),
                    },
                    wgpu::BindGroupEntry {
                        binding: 3,
                        resource: wgpu::BindingResource::TextureView(&shadow.depth),
                    },
                    wgpu::BindGroupEntry {
                        binding: 4,
                        resource: wgpu::BindingResource::Sampler(&sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 5,
                        resource: wgpu::BindingResource::TextureView(
                            ao_frame
                                .as_ref()
                                .map_or(&scene.occlusion.fallback, |frame| &frame.map),
                        ),
                    },
                    wgpu::BindGroupEntry {
                        binding: 6,
                        resource: wgpu::BindingResource::TextureView(background),
                    },
                    wgpu::BindGroupEntry {
                        binding: 7,
                        resource: wgpu::BindingResource::Sampler(texture_sampler),
                    },
                    wgpu::BindGroupEntry {
                        binding: 8,
                        resource: captured_depth.as_entire_binding(),
                    },
                ],
            })
        };

        let bind_group = bind_background(
            &scene.background,
            &scene.background_sampler,
            &scene.background_depth,
        );
        let transmitted_group = capture.as_ref().map(|(frame, _, _)| {
            bind_background(
                &frame.sampled,
                &scene.background_sampler,
                captured_depth.unwrap_or(&scene.background_depth),
            )
        });
        let resumed_group = transmitted_group.as_ref().unwrap_or(&bind_group);

        let lightmaps = draws.as_ref().and_then(|draws| {
            let params = draws.set.lightmap_parameters.as_ref()?;
            let sampler = scene.lightmaps.sampler(ctx.device);
            let mut mapped = HashMap::new();
            for draw in draws.set.color_draws(draws.plan, draws.visibility) {
                let Some(map) = draws.objects[draw.source as usize].lightmap.as_ref() else {
                    continue;
                };
                let key = (map.identity(), draw.compacted);
                if let std::collections::hash_map::Entry::Vacant(entry) = mapped.entry(key) {
                    let buffer = if draw.compacted {
                        draws.plan.and_then(|plan| plan.compact_lightmaps.as_ref())
                    } else {
                        Some(params)
                    };
                    if let Some((buffer, texture)) =
                        buffer.zip(scene.lightmaps.texture(ctx.device, queue, map))
                    {
                        entry.insert(bind_background(&texture, &sampler, buffer));
                    }
                }
            }
            (!mapped.is_empty()).then_some(LightmapBindings {
                base: &bind_group,
                mapped,
            })
        });
        self.stats.lightmap_uploads = scene.lightmaps.uploads;

        let environment_bindings = scene
            .environment
            .bind(ctx.device, queue, environment.as_ref());
        let local_bindings = probe_slots.as_ref().zip(probes).map(|(slots, probes)| {
            scene.environment.local_bindings(
                (ctx.device, queue),
                environment.as_ref(),
                probes,
                slots,
                visibility,
                scene_cubes,
            )
        });
        let local = probe_slots
            .as_deref()
            .zip(local_bindings.as_deref())
            .map(|(slots, bindings)| LocalReflections { slots, bindings });
        let reflections = local.as_ref().map(|local| (local, &environment_bindings));
        if multisampled.is_some() {
            self.stats.multisample_resolves = self.stats.multisample_resolves.saturating_add(1);
        }
        let opaque = capture
            .as_ref()
            .map_or(target, |(frame, _, _)| &frame.levels[0]);
        let (mut pass, opaque_output) = if let Some(surfaces) = &surfaces {
            (
                surfaces.begin(ctx.encoder, depth, clear, multisampled),
                ColorOutput {
                    hdr: true,
                    screen_reflections: effects.reflection.is_some(),
                    screen_indirect_light: effects.indirect.is_some(),
                    ..output
                },
            )
        } else {
            (
                viewport_pass(
                    ctx.encoder,
                    multisampled.unwrap_or(opaque),
                    multisampled
                        .filter(|_| capturing || !weighted)
                        .map(|_| opaque),
                    Some(depth),
                    weighted || retain_depth || capturing,
                    Some(clear),
                    capturing,
                ),
                output,
            )
        };
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_bind_group(2, &environment_bindings, &[]);
        if skybox && !fog_background {
            pass.set_pipeline(scene.pipelines.skybox(ctx.device, opaque_output));
            pass.draw(0..3, 0..1);
        }
        if let Some(draws) = &draws {
            draws.batches(
                (&mut scene.meshes, &mut scene.pipelines),
                (ctx.device, queue),
                &mut pass,
                &mut self.stats,
                opaque_output,
                (reflections, lightmaps.as_ref()),
            );
        }
        if fog_background {
            pass.set_pipeline(scene.pipelines.skybox(ctx.device, opaque_output));
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(2, &environment_bindings, &[]);
            pass.draw(0..3, 0..1);
        }
        if !capturing && surfaces.is_none() {
            if !weighted && let Some(draws) = &draws {
                if lightmaps.is_some() {
                    pass.set_bind_group(0, &bind_group, &[]);
                }
                draws.sorted_transparency(
                    (&mut scene.meshes, &mut scene.pipelines),
                    (ctx.device, queue),
                    &mut pass,
                    &mut self.stats,
                    output,
                    &camera,
                    reflections,
                );
            }
            draw_particles(&mut pass, false);
        }
        drop(pass);
        if let Some(snapshot) = &depth_snapshot {
            self.opaque_depth.capture(
                ctx.device,
                ctx.encoder,
                (depth, snapshot),
                raster_size,
                multisampling,
            );
            self.stats.opaque_depth_captures += 1;
            self.stats.volume_depth_captures += u64::from(needs_volume_depth);
        }
        if let Some(surfaces) = &surfaces {
            let resume_blending = !capturing
                && if weighted {
                    has_custom_blend
                } else {
                    has_particles
                        || instances
                            .as_ref()
                            .is_some_and(|set| set.has_visible_transparency(visibility))
                };
            let destination =
                if let Some((color, depth_snapshot)) = multisampled.zip(depth_snapshot.as_ref()) {
                    ScreenTarget::Multisampled {
                        color,
                        resolve: (capturing || !weighted).then_some(opaque),
                        keep_samples: resume_blending || continue_opaque || weighted,
                        depth_snapshot,
                    }
                } else {
                    ScreenTarget::Single {
                        color: opaque,
                        hdr: output.hdr,
                    }
                };
            let [reflected, indirect] = self.screen_lighting.resolve(
                (ctx.device, ctx.encoder),
                surfaces,
                (destination, depth),
                raster_size,
                (raster_camera, effects),
            );
            self.stats.screen_reflection_resolves += u64::from(reflected > 0);
            self.stats.screen_reflection_pixels += reflected;
            self.stats.screen_indirect_resolves += u64::from(indirect > 0);
            self.stats.screen_indirect_pixels += indirect;
            if resume_blending {
                let mut pass = viewport_pass(
                    ctx.encoder,
                    multisampled.unwrap_or(target),
                    multisampled.filter(|_| !weighted).map(|_| target),
                    Some(depth),
                    weighted || retain_depth,
                    None,
                    false,
                );
                if !weighted && let Some(draws) = &draws {
                    pass.set_bind_group(0, &bind_group, &[]);
                    pass.set_bind_group(2, &environment_bindings, &[]);
                    draws.sorted_transparency(
                        (&mut scene.meshes, &mut scene.pipelines),
                        (ctx.device, queue),
                        &mut pass,
                        &mut self.stats,
                        output,
                        &camera,
                        reflections,
                    );
                }
                draw_particles(&mut pass, false);
            }
        }
        if let Some((frame, filter, levels)) = capture {
            frame.filter(
                ctx.device,
                ctx.encoder,
                filter,
                levels,
                multisampled.is_none().then_some(target),
            );
            self.stats.transmission_captures += 1;
            self.stats.transmission_mip_passes += (levels - 1) as u64;
            if continue_opaque {
                let mut pass = viewport_pass(
                    ctx.encoder,
                    multisampled.unwrap_or(target),
                    multisampled.filter(|_| !weighted).map(|_| target),
                    Some(depth),
                    weighted || retain_depth,
                    None,
                    false,
                );
                pass.set_bind_group(0, resumed_group, &[]);
                pass.set_bind_group(2, &environment_bindings, &[]);
                if let Some(draws) = &draws {
                    draws.batches(
                        (&mut scene.meshes, &mut scene.pipelines),
                        (ctx.device, queue),
                        &mut pass,
                        &mut self.stats,
                        ColorOutput {
                            pass: ColorPass::Transmission,
                            ..output
                        },
                        (reflections, None),
                    );
                    if !weighted {
                        draws.sorted_transparency(
                            (&mut scene.meshes, &mut scene.pipelines),
                            (ctx.device, queue),
                            &mut pass,
                            &mut self.stats,
                            output,
                            &camera,
                            reflections,
                        );
                    }
                }
                draw_particles(&mut pass, false);
            }
        }
        if let Some(transparency) = &transparency {
            let mut pass = transparency.accumulate(ctx.encoder, retain_depth);
            if let Some(draws) = &draws {
                pass.set_bind_group(0, resumed_group, &[]);
                pass.set_bind_group(2, &environment_bindings, &[]);
                draws.batches(
                    (&mut scene.meshes, &mut scene.pipelines),
                    (ctx.device, queue),
                    &mut pass,
                    &mut self.stats,
                    ColorOutput {
                        pass: ColorPass::Weighted,
                        ..output
                    },
                    (reflections, None),
                );
            }
            draw_particles(&mut pass, true);
            drop(pass);
            transparency.composite(ctx.encoder, target);
            self.stats.transparency_composites =
                self.stats.transparency_composites.saturating_add(1);
        }
        let motion = motion_history.and_then(|history| {
            let mut history = history.borrow_mut();
            let Some((draws, objects)) = draws.as_ref().zip(objects) else {
                *history = Default::default();
                return None;
            };
            let (inputs, count) = self.motion.record(
                &mut ctx,
                queue,
                (
                    &mut scene.meshes,
                    &scene.pipelines.materials[0],
                    &scene.pipelines.deformations,
                ),
                (draws, objects, raster_camera),
                &mut history,
            );
            self.stats.motion_vector_draws += count;
            Some(inputs)
        });
        self.stats.material_mip_levels_uploaded = scene.meshes.materials.mip_levels_uploaded;
        if let Some(source) = &post_target {
            self.resolve_postprocessing(
                &mut ctx,
                queue,
                (source, raster_size),
                focus_depth.as_ref(),
                (post, sample, motion.as_ref()),
            );
        }
    }

    fn resolve_postprocessing(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        source: (&wgpu::TextureView, [u32; 2]),
        depth: Option<&FocusDepth>,
        (settings, sample, motion): (PostSettings, Option<&AaSample>, Option<&MotionInputs>),
    ) {
        let work = self
            .postprocess
            .resolve(ctx, queue, source, depth, (&settings, sample, motion));
        self.stats.progressive_aa_samples = self
            .stats
            .progressive_aa_samples
            .saturating_add(u64::from(matches!(sample, Some(AaSample::Progressive(_)))));
        self.stats.temporal_aa_samples = self
            .stats
            .temporal_aa_samples
            .saturating_add(u64::from(matches!(sample, Some(AaSample::Temporal(_)))));
        if let Some(AaSample::Temporal(sample)) = sample {
            self.stats.temporal_aa_blends = self.stats.temporal_aa_blends.saturating_add(
                u64::from(sample.motion.is_none() && sample.previous.is_some()),
            );
            self.stats.temporal_reprojections = self
                .stats
                .temporal_reprojections
                .saturating_add(u64::from(motion.is_some() && sample.previous.is_some()));
        }
        if source.1 != ctx.size {
            self.stats.supersample_resolves = self.stats.supersample_resolves.saturating_add(1);
            self.stats.supersample_pixels = self
                .stats
                .supersample_pixels
                .saturating_add(source.1.into_iter().map(u64::from).product());
        }
        self.stats.hdr_resolves = self
            .stats
            .hdr_resolves
            .saturating_add(u64::from(settings.hdr));
        self.stats.color_adjustment_resolves = self
            .stats
            .color_adjustment_resolves
            .saturating_add(u64::from(settings.has_adjustments()));
        self.stats.lut_resolves = self.stats.lut_resolves.saturating_add(work.lut_resolves);
        self.stats.lut_uploads = self.stats.lut_uploads.saturating_add(work.lut_uploads);
        self.stats.bloom_passes = self.stats.bloom_passes.saturating_add(work.bloom_passes);
        self.stats.bloom_pixels = self.stats.bloom_pixels.saturating_add(work.bloom_pixels);
        self.stats.bloom_target_allocations = self
            .stats
            .bloom_target_allocations
            .saturating_add(work.bloom_target_allocations);
        if work.focus_filtered_pixels != 0 {
            self.stats.depth_of_field_passes = self.stats.depth_of_field_passes.saturating_add(3);
            self.stats.depth_of_field_pixels = self
                .stats
                .depth_of_field_pixels
                .saturating_add(work.focus_filtered_pixels);
        }
    }
}

struct MeshColorDraws<'a> {
    set: &'a InstanceSet,
    objects: &'a [SceneObject],
    visibility: &'a SceneVisibility,
    geometry: GeometryBindings<'a>,
    plan: Option<&'a ColorPlan>,
}

impl MeshColorDraws<'_> {
    fn instances(&self, compacted: bool) -> &wgpu::Buffer {
        if compacted {
            self.plan
                .and_then(|plan| plan.compact.as_ref())
                .unwrap_or(&self.set.buffer)
        } else {
            &self.set.buffer
        }
    }

    fn depth_prepass(
        &self,
        (meshes, pipelines): (&mut MeshCache, &mut DepthPipelines),
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        encoder: &mut wgpu::CommandEncoder,
        target: &wgpu::TextureView,
        camera: &wgpu::Buffer,
    ) -> u64 {
        let globals = pipelines.camera_group(device, camera);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack ambient occlusion depth"),
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: target,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        pass.set_bind_group(0, &globals, &[]);
        let mut total = 0;
        let mut compacted = None;
        let mut active = None;
        for batch in self.set.color_draws(self.plan, self.visibility) {
            let kind = &self.objects[batch.source as usize].mesh;
            if !kind.has_opaque_parts() {
                continue;
            }
            if compacted != Some(batch.compacted) {
                pass.set_vertex_buffer(1, self.instances(batch.compacted).slice(..));
                compacted = Some(batch.compacted);
            }
            total += pipelines.draw(
                device,
                &mut pass,
                &mut active,
                meshes.get(device, queue, kind),
                (batch.instances, self.geometry.deformation(batch.source)),
            );
        }
        total
    }

    fn batches(
        &self,
        (meshes, pipelines): (&mut MeshCache, &mut Pipelines),
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        pass: &mut wgpu::RenderPass<'_>,
        stats: &mut SceneDrawStats,
        output: ColorOutput,
        (reflections, lightmaps): (
            Option<(&LocalReflections<'_>, &wgpu::BindGroup)>,
            Option<&LightmapBindings<'_>>,
        ),
    ) {
        let mut bound_compacted = None;
        let mut active = None;
        let mut active_probe = None;
        let mut active_lightmap = None;
        for draw in self.set.color_draws(self.plan, self.visibility) {
            let kind = &self.objects[draw.source as usize].mesh;
            let has_parts = match output.pass {
                ColorPass::Opaque => kind.has_opaque_parts(),
                ColorPass::Transmission => kind.has_depth_writing_transmission(),
                ColorPass::Weighted => !kind.blended_parts().is_empty(),
            };
            if !has_parts {
                continue;
            }
            if let Some((local, fallback)) = reflections {
                let slot = local.slot(draw.source);
                if active_probe != Some(slot) {
                    local.bind(pass, slot, fallback);
                    active_probe = Some(slot);
                }
            }
            if bound_compacted != Some(draw.compacted) {
                pass.set_vertex_buffer(1, self.instances(draw.compacted).slice(..));
                bound_compacted = Some(draw.compacted);
            }
            let mesh = meshes.get(device, queue, kind);
            mesh.bind(pass);
            let mut lightmapped = false;
            if let Some(bindings) = lightmaps {
                let map = self.objects[draw.source as usize]
                    .lightmap
                    .as_ref()
                    .map(|map| (map.identity(), draw.compacted));
                let mapped = map.and_then(|key| bindings.mapped.get(&key));
                lightmapped = mapped.is_some();
                if active_lightmap != map {
                    pass.set_bind_group(0, mapped.unwrap_or(bindings.base), &[]);
                    active_lightmap = map;
                }
                if lightmapped {
                    mesh.bind_lightmap_uv(device, pass, kind);
                }
            }
            for part in mesh.parts.iter().filter(|part| match output.pass {
                ColorPass::Opaque => !part.surface.blend && !part.surface.transmission,
                ColorPass::Transmission => !part.surface.blend && part.surface.transmission,
                ColorPass::Weighted => part.surface.blend,
            }) {
                stats.submit(&draw.instances, &part.vertices);
                if lightmapped {
                    stats.lightmapped_draws += 1;
                }
                draw_part(
                    device,
                    pipelines,
                    pass,
                    &mut active,
                    part,
                    (
                        draw.instances.clone(),
                        self.geometry.deformation(draw.source),
                    ),
                    ColorOutput {
                        lightmapped,
                        ..output
                    },
                );
            }
        }
    }

    fn sorted_transparency(
        &self,
        (meshes, pipelines): (&mut MeshCache, &mut Pipelines),
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        pass: &mut wgpu::RenderPass<'_>,
        stats: &mut SceneDrawStats,
        output: ColorOutput,
        camera: &SceneCamera,
        reflections: Option<(&LocalReflections<'_>, &wgpu::BindGroup)>,
    ) {
        let mut draws = self
            .set
            .transparent_draws(camera, self.objects, self.visibility)
            .peekable();
        if draws.peek().is_none() {
            return;
        }
        pass.set_vertex_buffer(1, self.set.buffer.slice(..));
        let mut active_mesh = None;
        let mut active = None;
        let mut active_probe = None;
        for draw in draws {
            if let Some((local, fallback)) = reflections {
                let slot = local.slot(draw.instance);
                if active_probe != Some(slot) {
                    local.bind(pass, slot, fallback);
                    active_probe = Some(slot);
                }
            }
            let kind = &self.objects[draw.instance as usize].mesh;
            let mesh = meshes.get(device, queue, kind);
            if active_mesh != Some(kind) {
                mesh.bind(pass);
                active_mesh = Some(kind);
            }
            let part = &mesh.parts[draw.part];
            let range = draw.instance..draw.instance + 1;
            stats.submit(&range, &part.vertices);
            draw_part(
                device,
                pipelines,
                pass,
                &mut active,
                part,
                (range, self.geometry.deformation(draw.instance)),
                output,
            );
        }
    }
}

fn receives_occlusion(mesh: &Mesh3D) -> bool {
    mesh.parts().is_empty()
        || mesh.sections().any(|(_, material)| {
            material.is_some_and(|material| {
                material.alpha_mode != MeshAlphaMode::Blend
                    && material
                        .pbr
                        .as_ref()
                        .is_some_and(|pbr| pbr.transmission_blur().is_none())
            })
        })
}

fn viewport_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    resolve: Option<&wgpu::TextureView>,
    depth: Option<&wgpu::TextureView>,
    keep_depth: bool,
    clear: Option<wgpu::Color>,
    keep_multisampled: bool,
) -> wgpu::RenderPass<'a> {
    encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
        label: Some("XRack 3D viewport"),
        color_attachments: &[Some(wgpu::RenderPassColorAttachment {
            view: target,
            depth_slice: None,
            resolve_target: resolve,
            ops: wgpu::Operations {
                load: clear.map_or(wgpu::LoadOp::Load, wgpu::LoadOp::Clear),
                store: if resolve.is_some() && !keep_multisampled {
                    wgpu::StoreOp::Discard
                } else {
                    wgpu::StoreOp::Store
                },
            },
        })],
        depth_stencil_attachment: depth.map(|depth| wgpu::RenderPassDepthStencilAttachment {
            view: depth,
            depth_ops: Some(wgpu::Operations {
                load: if clear.is_some() {
                    wgpu::LoadOp::Clear(1.0)
                } else {
                    wgpu::LoadOp::Load
                },
                store: if keep_depth {
                    wgpu::StoreOp::Store
                } else {
                    wgpu::StoreOp::Discard
                },
            }),
            stencil_ops: None,
        }),
        ..Default::default()
    })
}

fn draw_part(
    device: &wgpu::Device,
    pipelines: &mut Pipelines,
    pass: &mut wgpu::RenderPass<'_>,
    active: &mut Option<(Surface, Deformation, bool)>,
    part: &GpuPart,
    (instances, deformation): (Range<u32>, Option<&DeformationBinding>),
    output: ColorOutput,
) {
    let selection = (
        part.surface,
        deformation.map_or(Deformation::Rigid, |binding| binding.mode),
        output.lightmapped,
    );
    if *active != Some(selection) {
        pass.set_pipeline(pipelines.get(device, part.surface, output, selection.1));
        *active = Some(selection);
    }
    if let Some(binding) = deformation {
        pass.set_bind_group(3, &binding.group, &[]);
    }
    pass.set_bind_group(1, part.material.color(), &[]);
    pass.draw(part.vertices.clone(), instances);
}

fn append_floats(bytes: &mut Vec<u8>, values: impl IntoIterator<Item = f32>) {
    for value in values {
        bytes.extend_from_slice(&value.to_ne_bytes());
    }
}

fn buffer(
    device: &wgpu::Device,
    label: &str,
    contents: &[u8],
    usage: wgpu::BufferUsages,
) -> wgpu::Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents,
        usage,
    })
}

fn mesh_buffer(device: &wgpu::Device, mesh: &Mesh3D, parts: Vec<GpuPart>) -> MeshBuffer {
    let vertices = mesh.vertices();
    let mut bytes = Vec::with_capacity(vertices.len() * MeshVertex::STRIDE);
    for vertex in vertices {
        append_floats(
            &mut bytes,
            vertex
                .position
                .into_iter()
                .chain(vertex.normal)
                .chain(vertex.uv)
                .chain(vertex.color)
                .chain(vertex.uv1),
        );
    }
    MeshBuffer {
        lightmap_uv: OnceCell::new(),
        vertices: buffer(
            device,
            "XRack 3D unit mesh",
            &bytes,
            wgpu::BufferUsages::VERTEX,
        ),
        skin_weights: mesh.influences().map(|weights| {
            let packed: Vec<_> = weights
                .iter()
                .copied()
                .flat_map(|weight| weight.packed())
                .collect();
            buffer(
                device,
                "XRack glTF joint weights",
                &packed,
                wgpu::BufferUsages::VERTEX,
            )
        }),
        morph: mesh.morph_geometry().map(|geometry| {
            let bytes: Vec<_> = geometry.packed_words().flat_map(u32::to_ne_bytes).collect();
            buffer(
                device,
                "XRack glTF morph deltas",
                &bytes,
                wgpu::BufferUsages::STORAGE,
            )
        }),
        tangents: (!mesh.tangents().is_empty()).then(|| {
            let bytes: Vec<_> = mesh
                .tangents()
                .iter()
                .flatten()
                .copied()
                .flat_map(i16::to_ne_bytes)
                .collect();
            buffer(
                device,
                "XRack 3D mesh tangents",
                &bytes,
                wgpu::BufferUsages::VERTEX,
            )
        }),
        parts,
    }
}

pub(super) fn background(node: &Node) -> Color {
    let color = node.color("scene-background").unwrap_or(Color(0x202a38ff));
    Color(color.0 | 0xff)
}

fn linear_rgb(color: Color) -> [f64; 3] {
    let [r, g, b, _] = color.0.to_be_bytes();
    [r, g, b].map(|channel| {
        let encoded = f64::from(channel) / 255.0;
        if encoded <= 0.04045 {
            encoded / 12.92
        } else {
            ((encoded + 0.055) / 1.055).powf(2.4)
        }
    })
}

struct Attachment {
    size: [u32; 2],
    view: wgpu::TextureView,
}

impl Attachment {
    /// Each slot fixes the format and sample count. Only size can change.
    fn get<'a>(
        slot: &'a mut Option<Self>,
        device: &wgpu::Device,
        size: [u32; 2],
        format: wgpu::TextureFormat,
        sample_count: u32,
        readable: bool,
    ) -> &'a wgpu::TextureView {
        if slot
            .as_ref()
            .is_some_and(|attachment| attachment.size != size)
        {
            *slot = None;
        }
        &slot
            .get_or_insert_with(|| Self {
                size,
                view: attachment_view(
                    device,
                    size,
                    format,
                    sample_count,
                    wgpu::TextureUsages::RENDER_ATTACHMENT
                        | if readable {
                            wgpu::TextureUsages::TEXTURE_BINDING
                        } else {
                            wgpu::TextureUsages::TRANSIENT_ATTACHMENT
                        },
                    "XRack 3D viewport attachment",
                ),
            })
            .view
    }
}

fn attachment_view(
    device: &wgpu::Device,
    [width, height]: [u32; 2],
    format: wgpu::TextureFormat,
    sample_count: u32,
    usage: wgpu::TextureUsages,
    label: &str,
) -> wgpu::TextureView {
    device
        .create_texture(&wgpu::TextureDescriptor {
            label: Some(label),
            size: wgpu::Extent3d {
                width,
                height,
                depth_or_array_layers: 1,
            },
            mip_level_count: 1,
            sample_count,
            dimension: wgpu::TextureDimension::D2,
            format,
            usage,
            view_formats: &[],
        })
        .create_view(&Default::default())
}
