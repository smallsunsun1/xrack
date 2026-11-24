//! Offscreen forward rendering for the SceneView3D element.
mod bloom;
mod capture_region;
mod capture_stencil;
mod cascade;
mod color_capture;
mod composite_input;
mod custom_effect;
mod deform;
mod depth;
mod depth_of_field;
pub(super) mod effect_history;
pub(super) use custom_effect::has_history as has_effect_history;
mod environment;
mod fog;
mod instances;
pub(crate) mod item_textures;
mod lighting;
mod lightmap;
mod lut;
mod material;
pub(crate) use material::material_texture_limit;
mod motion;
mod occlusion;
mod particle_affectors;
mod particle_instances;
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
mod vertex_program;
mod volumetric_fog;
mod wireframe;

use crate::{GpuPaintContext, Node};
use capture_region::CaptureRegion;
use capture_stencil::{StencilBuffers, StencilPass};
use color_capture::{
    CaptureInput, CapturePlan, CaptureStage, CaptureViews, ColorCaptures, SharedProjectionBuffers,
    capture_scene, cube_revisions, geometry_scene, material_sources,
};
use composite_input::{CompositeInput, CompositeInputs};
use custom_effect::EffectSurface;
use deform::{Deformation, DeformationBinding, DeformationBuffers, GeometryBindings};
use depth::{DepthPass, DepthPipelines};
use depth_of_field::FocusDepth;
use environment::{EnvironmentGpu, EnvironmentInput, LocalReflections};
use fog::Fog;
use instances::{ColorPlan, InstanceBuffers, InstanceSet};
use item_textures::ItemTextureView;
use lighting::{LightBuffers, LightSet};
use lightmap::{LightmapBindings, LightmapTextures};
use material::{MaterialBindings, Materials};
use motion::{MotionInputs, MotionPass};
use occlusion::{AmbientOcclusion, OcclusionSettings, OcclusionSource};
use particles::{ModelParticleBatch, ParticleBatches, ParticleDraw, ParticlePipelines};
use pipeline::{
    ColorOutput, ColorPass, GeometryOutput, NAMED_GEOMETRY_ATTACHMENTS, Pipelines, Surface,
    VolumeMode,
};
use postprocess::{PostProcess, PostSettings};
use reflection_capture::{CapturedCube, Reflections, capture_node};
use retained_view::AaSample;
use screen_lighting::{
    IndirectSettings, ReflectionSettings, ScreenLighting, ScreenSettings, ScreenTarget,
};
use shadow::{ShadowRequest, Shadows};
use std::{
    collections::{BTreeMap, BTreeSet, HashMap},
    ops::Range,
    sync::{Arc, Weak},
};
use transmission::{OpaqueDepth, Transmission};
use transparency::Transparency;
use wgpu::util::DeviceExt;
use wireframe::RasterMode;
use xrack_runtime::{
    Color, Value,
    scene_effect::{
        CaptureAttachment, CapturePipelineState, CaptureRenderables, encode_capture_depth_clear,
    },
    scene3d::{
        CaptureMaterial, Mesh3D, MeshAlphaMode, MeshCullMode, MeshData, MeshDepthCoverage,
        MeshDepthDraw, MeshVertex, MeshVertexStreams, ReflectionSource, SceneCamera, SceneObject,
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
    /// Submitted vertex references summed over all instances and color draws.
    pub color_vertices: u64,
    /// Bytes uploaded for mesh position, normal, UV, color and tangent buffers.
    pub mesh_vertex_bytes_uploaded: u64,
    /// Bytes uploaded for mesh index buffers.
    pub mesh_index_bytes_uploaded: u64,
    /// Expanded vertex streams created for the portable wireframe path.
    pub mesh_wireframe_uploads: u64,
    /// Bytes uploaded for per-vertex skin weights and morph displacement tables.
    pub mesh_deformation_bytes_uploaded: u64,
    /// Mesh uploads that used compact index buffers.
    pub indexed_mesh_uploads: u64,
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
    /// Instanced geometry draws for model particles, including material passes.
    pub particle_model_draws: u64,
    /// Model particle transforms generated by the GPU.
    pub particle_model_instances: u64,
    /// Opaque or alpha-cutout model particle draws into shadow maps.
    pub particle_shadow_draws: u64,
    /// GPU transforms generated for a capture step, shared by its cubemap faces.
    pub particle_reflection_instances: u64,
    /// Objects for which a replacement mesh was selected, before camera culling.
    pub lod_objects: u64,
    /// Color-only instance buffer uploads needed to keep fragmented draws batched.
    pub color_compaction_uploads: u64,
    /// Host bytes sent to direct or compacted vertex instance buffers.
    pub instance_upload_bytes: u64,
    /// Existing instance buffers updated with a bounded subset of rows.
    pub partial_instance_uploads: u64,
    /// New vertex instance buffers initialized with all their rows.
    pub instance_buffer_allocations: u64,
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
    /// GPU variants compiled for authored material shaders.
    pub custom_material_pipeline_builds: u64,
    /// File image GPU allocations shared by materials and postprocessing.
    pub scene_image_uploads: u64,
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
    /// Depth-only mesh draws requested by material depth modes.
    pub material_depth_prepass_draws: u64,
    /// Half-resolution ambient occlusion evaluation passes.
    pub occlusion_evaluations: u64,
    /// Pixels of the occlusion maps evaluated this far.
    pub occlusion_pixels: u64,
    /// Scene-linear HDR viewports resolved into the 2D compositor.
    pub hdr_resolves: u64,
    /// Viewports with color adjustments in the final display conversion.
    pub color_adjustment_resolves: u64,
    /// Full-screen passes using an authored scene effect.
    pub custom_effect_passes: u64,
    /// Draws of named scene color feeds into offscreen textures.
    pub scene_capture_passes: u64,
    /// Named color and depth imported by a fullscreen draw in the opaque pass.
    pub scene_capture_composite_draws: u64,
    /// Stencil aspects copied between named captures.
    pub scene_capture_stencil_copies: u64,
    /// Geometry draws for retained depth and normal-roughness attachments.
    pub scene_capture_geometry_draws: u64,
    /// Named feeds served by a completed or shared capture.
    pub scene_capture_reuses: u64,
    /// Bytes allocated for named scene color textures.
    pub scene_capture_bytes_allocated: u64,
    /// Mesh binding variants created for capture-backed shader textures.
    pub scene_capture_material_bindings: u64,
    /// Immutable camera projection tables uploaded for named depth inputs.
    pub scene_capture_projection_uploads: u64,
    /// Capture reads omitted to break a render feedback loop.
    pub scene_capture_feedback_edges: u64,
    /// Opaque scene captures built for post-effect surface sampling.
    pub custom_effect_surface_passes: u64,
    /// Geometry draws submitted to the post-effect surface capture.
    pub custom_effect_surface_draws: u64,
    /// Scene-effect pipeline variants prepared for a shader and depth mode.
    pub custom_effect_pipelines: u64,
    /// Pixels shaded by authored passes at their chosen render scales.
    pub custom_effect_pixels: u64,
    /// Scratch textures allocated by authored passes.
    pub custom_effect_target_allocations: u64,
    /// Viewport-local history textures allocated by authored effect passes.
    pub custom_effect_history_allocations: u64,
    /// Cumulative payload bytes of effect output and history textures.
    pub custom_effect_bytes_allocated: u64,
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
    /// Viewport resolves that prepared premultiplied display color for 2D.
    pub alpha_composite_resolves: u64,
    /// Captured 2D material sources repainted into GPU textures.
    pub item_texture_captures: u64,
    /// GPU textures allocated for visible 2D material sources.
    pub item_texture_allocations: u64,
    /// Material sources reused without a 2D repaint.
    pub item_texture_reuses: u64,
    /// Stored GPU mip chains for 2D material sources.
    pub item_texture_mip_allocations: u64,
    /// GPU passes that decode or downsample a source's mip chain.
    pub item_texture_mip_passes: u64,
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
    color_captures: ColorCaptures,
    composite_inputs: CompositeInputs,
    stencil_buffers: StencilBuffers,
    item_textures: BTreeMap<String, ItemTextureView>,
    item_texture_revision: u64,
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
    Viewport(
        &'a [Option<CapturedCube>],
        Option<&'a AaSample>,
        effect_history::Owner<'a>,
        &'a CaptureViews,
    ),
    ColorCapture {
        cubes: &'a [Option<CapturedCube>],
        inputs: &'a CaptureViews,
        geometry: Option<NamedGeometry<'a>>,
        renderables: CaptureRenderables,
        pipeline_state: CapturePipelineState,
        region: CaptureRegion,
        depth_clear: f32,
        stencil: Option<&'a StencilPass>,
        override_material: Option<&'a Arc<CaptureMaterial>>,
        composite: Option<CompositeInput<'a>>,
    },
    Reflection(
        Option<(&'a ParticleDraw<'a>, &'a ParticleBatches)>,
        &'a CaptureViews,
    ),
}

#[derive(Clone, Copy)]
struct NamedGeometry<'a> {
    views: [Option<&'a wgpu::TextureView>; NAMED_GEOMETRY_ATTACHMENTS.len()],
    mask: u8,
}

impl NamedGeometry<'_> {
    fn output(self) -> GeometryOutput {
        GeometryOutput::Named(self.mask)
    }

    fn begin<'a>(
        self,
        encoder: &'a mut wgpu::CommandEncoder,
        depth: Option<&wgpu::TextureView>,
        depth_clear: f32,
        stencil: Option<&StencilPass>,
    ) -> wgpu::RenderPass<'a> {
        fn clear(
            view: &wgpu::TextureView,
            color: wgpu::Color,
        ) -> wgpu::RenderPassColorAttachment<'_> {
            wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(color),
                    store: wgpu::StoreOp::Store,
                },
            }
        }
        let targets = std::array::from_fn::<_, { NAMED_GEOMETRY_ATTACHMENTS.len() }, _>(|index| {
            let color = if NAMED_GEOMETRY_ATTACHMENTS[index] == CaptureAttachment::Depth {
                wgpu::Color {
                    r: encode_capture_depth_clear(depth_clear),
                    ..wgpu::Color::TRANSPARENT
                }
            } else {
                wgpu::Color::TRANSPARENT
            };
            self.views[index].map(|view| clear(view, color))
        });
        encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack named scene geometry"),
            color_attachments: &targets,
            depth_stencil_attachment: depth.map(|view| wgpu::RenderPassDepthStencilAttachment {
                view,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(depth_clear),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: stencil.map(|pass| pass.operations(true)),
            }),
            ..Default::default()
        })
    }
}

#[derive(Default)]
pub(super) struct PreparedCaptures {
    plan: Option<CapturePlan>,
    ready: CaptureViews,
}

#[derive(Default)]
pub(super) struct ReflectionFrame {
    pub cubes: Vec<Option<CapturedCube>>,
    pub needs_redraw: bool,
    pub captures: PreparedCaptures,
}

struct CaptureContext<'a> {
    device: &'a wgpu::Device,
    queue: &'a wgpu::Queue,
    encoder: &'a mut wgpu::CommandEncoder,
    owner: effect_history::Owner<'a>,
    paint: crate::PaintContext<'a>,
}

pub(super) fn has_scene_reflections(node: &Node) -> bool {
    matches!(node.properties.get("reflection-probes"), Some(Value::SceneReflections(probes))
        if probes.iter().any(|probe| matches!(probe.source, ReflectionSource::Scene(_))))
}

#[derive(Clone)]
struct GpuPart {
    vertices: Range<u32>,
    material: MaterialBindings,
    surface: Surface,
    mask: bool,
    depth_coverage: Option<MeshDepthCoverage>,
}

struct MeshBuffer {
    geometry: Arc<MeshBuffers>,
    parts: Vec<GpuPart>,
}

impl std::ops::Deref for MeshBuffer {
    type Target = MeshBuffers;

    fn deref(&self) -> &Self::Target {
        &self.geometry
    }
}

struct MeshBuffers {
    vertices: wgpu::Buffer,
    index: Option<(wgpu::Buffer, wgpu::IndexFormat)>,
    tangents: Option<wgpu::Buffer>,
    skin_weights: Option<wgpu::Buffer>,
    morph: Option<MorphBuffer>,
    lightmap_uv: std::sync::OnceLock<wgpu::Buffer>,
}

struct MorphBuffer {
    entries: wgpu::Buffer,
    record_offset: u32,
}

#[derive(Default)]
struct MeshTransfers {
    vertex_bytes: u64,
    index_bytes: u64,
    deformation_bytes: u64,
    indexed: u64,
    wireframe_meshes: u64,
}

impl MeshTransfers {
    fn upload(&mut self, mesh: &MeshBuffers) {
        self.vertex_bytes += mesh.vertices.size();
        if let Some(tangents) = &mesh.tangents {
            self.vertex_bytes += tangents.size();
        }
        if let Some(weights) = &mesh.skin_weights {
            self.deformation_bytes += weights.size();
        }
        if let Some(morph) = &mesh.morph {
            self.deformation_bytes += morph.entries.size();
        }
        if let Some((index, _)) = &mesh.index {
            self.index_bytes += index.size();
            self.indexed += 1;
        }
    }
}

impl MeshBuffer {
    fn bind_geometry(&self, pass: &mut wgpu::RenderPass<'_>) {
        pass.set_vertex_buffer(0, self.vertices.slice(..));
        if let Some((buffer, format)) = &self.index {
            pass.set_index_buffer(buffer.slice(..), *format);
        }
    }

    fn draw_range(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        elements: Range<u32>,
        instances: Range<u32>,
    ) {
        if self.index.is_some() {
            pass.draw_indexed(elements, 0, instances);
        } else {
            pass.draw(elements, instances);
        }
    }

    fn bind(&self, pass: &mut wgpu::RenderPass<'_>) {
        self.bind_geometry(pass);
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
            let compact = self.index.as_ref().and_then(|_| mesh.draw_indices());
            let bytes: Vec<_> = mesh_rows(mesh.lightmap_uvs(), compact)
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
    wire_sphere: Option<MeshBuffer>,
    expand_triangles: bool,
    materials: Materials,
    imported: HashMap<(usize, bool), ImportedMesh>,
    captured: HashMap<((usize, bool), Vec<(u64, u64)>), ImportedMesh>,
    frame: u64,
    capture_binding_builds: u64,
    geometry: HashMap<(usize, bool), (Weak<MeshVertexStreams>, Arc<MeshBuffers>)>,
    transfers: MeshTransfers,
    uploads: u64,
}

struct ImportedMesh {
    source: Weak<MeshData>,
    buffer: MeshBuffer,
    frame: u64,
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
    material_depth: Option<DepthPipelines>,
}

impl SceneGpu {
    fn new(
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        item_textures: &BTreeMap<String, ItemTextureView>,
        textures: (material::SharedImages, SharedProjectionBuffers),
    ) -> Self {
        let pipelines = Pipelines::new(device);
        let mut materials = Materials::new(
            device,
            queue,
            pipelines.materials.clone(),
            pipelines.shader_materials.clone(),
            textures,
        );
        materials.items.clone_from(item_textures);
        let default = materials.bind(device, queue, None, None);
        let builtin = |mesh: &Mesh3D| MeshBuffer {
            geometry: Arc::new(mesh_buffer(device, mesh, false)),
            parts: vec![GpuPart {
                vertices: 0..mesh.vertices().len() as u32,
                material: default.clone(),
                mask: false,
                depth_coverage: Some(MeshDepthCoverage::Opaque),
                surface: Surface {
                    blend: false,
                    blend_mode: Default::default(),
                    depth_draw: Default::default(),
                    channel_remapping: false,
                    vertex_colors: true,
                    vertex_masks: false,
                    custom_base_fresnel: false,
                    custom_coat_fresnel: false,
                    transmission: false,
                    volume: VolumeMode::Off,
                    cull: MeshCullMode::None,
                    vertex_tangents: false,
                    anisotropy: false,
                    clearcoat: false,
                },
            }],
        };
        let box_mesh = builtin(&Mesh3D::Box);
        let sphere_mesh = builtin(&Mesh3D::Sphere);
        let mut transfers = MeshTransfers::default();
        transfers.upload(&box_mesh);
        transfers.upload(&sphere_mesh);
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
            material_depth: None,
            pipelines,
            lights: LightBuffers::default(),
            lightmaps: LightmapTextures::default(),
            meshes: MeshCache {
                box_mesh,
                sphere_mesh,
                wire_sphere: None,
                expand_triangles: false,
                transfers,
                materials,
                imported: HashMap::new(),
                captured: HashMap::new(),
                frame: 0,
                capture_binding_builds: 0,
                geometry: HashMap::new(),
                uploads: 0,
            },
        }
    }
}

impl MeshCache {
    fn begin_frame(&mut self) {
        self.frame = self.frame.wrapping_add(1);
        self.captured
            .retain(|_, entry| self.frame.wrapping_sub(entry.frame) <= 4);
    }

    fn prune(&mut self) {
        self.imported
            .retain(|_, entry| entry.source.strong_count() != 0);
        self.captured
            .retain(|_, entry| entry.source.strong_count() != 0);
        self.geometry
            .retain(|_, (data, _)| data.strong_count() != 0);
    }

    fn get(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, kind: &Mesh3D) -> &MeshBuffer {
        match kind {
            Mesh3D::Box => &self.box_mesh,
            Mesh3D::Sphere if self.expand_triangles && self.sphere_mesh.index.is_some() => {
                self.wire_sphere.get_or_insert_with(|| {
                    let mesh = mesh_buffer(device, kind, true);
                    self.transfers.upload(&mesh);
                    self.transfers.wireframe_meshes += 1;
                    MeshBuffer {
                        geometry: Arc::new(mesh),
                        parts: self.sphere_mesh.parts.clone(),
                    }
                })
            }
            Mesh3D::Sphere => &self.sphere_mesh,
            Mesh3D::Imported(data) => {
                let materials = &mut self.materials;
                let capture_bound = !data.capture_sources().is_empty();
                let expanded = self.expand_triangles && kind.draw_indices().is_some();
                let identity = (Arc::as_ptr(data) as usize, expanded);
                let frame = self.frame;
                let builds = &mut self.capture_binding_builds;
                let uploads = &mut self.uploads;
                let transfers = &mut self.transfers;
                let geometry_cache = &mut self.geometry;
                let key = capture_bound.then(|| {
                    let positions = data.uses_capture_positions();
                    let bindings = data
                        .capture_sources()
                        .iter()
                        .map(|read| {
                            materials.captures.get(read).map_or((0, 0), |capture| {
                                let revision = if positions {
                                    capture
                                        .projection
                                        .as_ref()
                                        .map_or(0, |projection| projection.revision)
                                } else {
                                    0
                                };
                                (capture.allocation, revision)
                            })
                        })
                        .collect();
                    (identity, bindings)
                });
                let create = || {
                    *builds += u64::from(capture_bound);
                    let source = data.vertex_streams();
                    let key = (Arc::as_ptr(source) as usize, expanded);
                    let geometry = geometry_cache
                        .entry(key)
                        .or_insert_with(|| {
                            *uploads = uploads.saturating_add(1);
                            let mesh = mesh_buffer(device, kind, expanded);
                            transfers.upload(&mesh);
                            transfers.wireframe_meshes += u64::from(expanded);
                            (Arc::downgrade(source), Arc::new(mesh))
                        })
                        .1
                        .clone();
                    let mut bindings = HashMap::new();
                    let parts = kind
                        .parts()
                        .iter()
                        .map(|part| {
                            let material = &kind.materials()[part.material];
                            let volume_inverse = material.volume_inverse(part.volume_inverse);
                            let key = (
                                part.material,
                                volume_inverse.map(|rows| rows.map(|row| row.map(f32::to_bits))),
                            );
                            let (material, mask, surface) =
                                bindings.entry(key).or_insert_with(|| {
                                    let binding = materials.bind(
                                        device,
                                        queue,
                                        Some(material),
                                        volume_inverse,
                                    );
                                    let pbr = material.pbr.as_ref();
                                    let anisotropy = pbr
                                        .and_then(|pbr| pbr.anisotropy.as_deref())
                                        .is_some_and(|term| term.strength > 0.0);
                                    let clearcoat = binding.coated;
                                    let transmission = material.transmission_blur().is_some();
                                    (
                                        binding,
                                        matches!(material.alpha_mode, MeshAlphaMode::Mask(_)),
                                        Surface {
                                            blend: material.is_blended(),
                                            blend_mode: material.blend_mode,
                                            depth_draw: material.depth_draw_mode,
                                            channel_remapping: material.channels.has_remapping(),
                                            vertex_colors: material.vertex_colors,
                                            vertex_masks: pbr.is_some()
                                                && material.vertex_masks.uses_vertex_masks(),
                                            custom_base_fresnel: pbr
                                                .is_some_and(|pbr| pbr.fresnel.is_custom()),
                                            custom_coat_fresnel: clearcoat
                                                && pbr
                                                    .and_then(|pbr| pbr.clearcoat.as_ref())
                                                    .is_some_and(|coat| coat.fresnel.is_custom()),
                                            transmission,
                                            volume: pbr.filter(|_| transmission).map_or(
                                                VolumeMode::Off,
                                                |pbr| {
                                                    if pbr.volume().is_none() {
                                                        VolumeMode::Off
                                                    } else if pbr.refraction_channels().dispersive()
                                                    {
                                                        VolumeMode::Dispersion
                                                    } else {
                                                        VolumeMode::Volume
                                                    }
                                                },
                                            ),
                                            cull: material.cull_mode,
                                            vertex_tangents: kind
                                                .uses_vertex_tangents(part.material)
                                                && (anisotropy
                                                    || pbr
                                                        .and_then(|pbr| pbr.tangent_normal_map())
                                                        .is_some()),
                                            anisotropy,
                                            clearcoat,
                                        },
                                    )
                                });
                            GpuPart {
                                vertices: part.vertices.start as u32..part.vertices.end as u32,
                                material: material.clone(),
                                surface: *surface,
                                mask: *mask,
                                depth_coverage: kind.materials()[part.material].depth_coverage(),
                            }
                        })
                        .collect();
                    ImportedMesh {
                        source: Arc::downgrade(data),
                        buffer: MeshBuffer { geometry, parts },
                        frame,
                    }
                };
                let entry = if let Some(key) = key {
                    self.captured.entry(key).or_insert_with(create)
                } else {
                    self.imported.entry(identity).or_insert_with(create)
                };
                entry.frame = frame;
                &entry.buffer
            }
        }
    }
}

impl SceneRenderer {
    fn set_item_textures(&mut self, revision: u64, items: BTreeMap<String, ItemTextureView>) {
        if let Some(scene) = &mut self.scene {
            scene.shadows.invalidate_item_textures();
            scene.occlusion.invalidate_item_textures();
            if self.item_textures != items {
                scene.meshes.materials.items.clone_from(&items);
                let uses_current_items = |entry: &ImportedMesh| {
                    entry
                        .source
                        .upgrade()
                        .is_some_and(|mesh| !mesh.uses_item_textures())
                };
                scene
                    .meshes
                    .imported
                    .retain(|_, entry| uses_current_items(entry));
                scene
                    .meshes
                    .captured
                    .retain(|_, entry| uses_current_items(entry));
            }
        }
        self.item_textures = items;
        self.item_texture_revision = revision;
    }

    pub(super) fn item_texture_revision(&self, node: &Node) -> u64 {
        if self.item_texture_revision == 0 {
            return 0;
        }
        if Self::uses_material_items(node)
            || matches!(node.properties.get("scene-effects"), Some(Value::SceneEffects(effects))
                if effects.item_sources().next().is_some())
        {
            self.item_texture_revision
        } else {
            0
        }
    }

    fn uses_material_items(node: &Node) -> bool {
        matches!(node.properties.get("model"), Some(Value::SceneObjects(model))
            if model.iter().any(|object| matches!(&object.mesh, Mesh3D::Imported(mesh) if mesh.uses_item_textures())))
            || matches!(node.properties.get("scene-captures"), Some(Value::SceneCaptures(captures))
            if captures.values().filter_map(|capture| capture.override_material.as_ref()).any(|style| {
                let material = style.material();
                material.base_item.is_some()
                    || material.shader.as_ref().is_some_and(|shader| shader.item_sources().next().is_some())
            }))
    }

    pub(super) fn begin_frame(&mut self) {
        self.reflections.begin_frame();
        self.color_captures.begin_frame();
        self.postprocess.effects.history.begin_frame();
        self.postprocess.effects.images.borrow_mut().prune();
        if let Some(scene) = &mut self.scene {
            scene.instances.begin_frame();
            scene.meshes.begin_frame();
        }
    }

    pub(super) fn finish_frame(&mut self, submitted: bool) {
        self.reflections.finish_frame(submitted);
        self.color_captures.finish_frame(submitted);
        self.postprocess.effects.history.finish_frame(submitted);
        let images = self.postprocess.effects.images.borrow();
        self.stats.material_mip_levels_uploaded = images.mip_levels_uploaded;
        self.stats.scene_image_uploads = images.uploads;
        self.stats.scene_capture_projection_uploads =
            self.postprocess.effects.projections.borrow().uploads;
        if let Some(scene) = &mut self.scene {
            self.stats.custom_material_pipeline_builds = scene.pipelines.material_shader_builds();
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

    pub(super) fn visit_effect_history(&mut self, owner: effect_history::Owner<'_>, active: bool) {
        self.postprocess.effects.history.visit(owner, active);
    }

    pub(super) fn draw(
        &mut self,
        node: &Node,
        ctx: GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        owner: effect_history::Owner<'_>,
        camera: Option<SceneCamera>,
        (cubes, sample, mut captures): (
            &[Option<CapturedCube>],
            Option<&AaSample>,
            PreparedCaptures,
        ),
    ) {
        self.stats.viewport_renders = self.stats.viewport_renders.saturating_add(1);
        let plan = captures.plan.take().unwrap_or_else(|| {
            CapturePlan::for_view(node, ctx.device, ctx.size, camera, &BTreeSet::new())
        });
        self.stats.scene_capture_feedback_edges += plan.feedback_edges;
        self.draw_color_captures(
            node,
            CaptureContext {
                device: ctx.device,
                queue,
                encoder: &mut *ctx.encoder,
                owner,
                paint: ctx.paint,
            },
            Some(cubes),
            plan.stages,
            &mut captures.ready,
        );
        self.draw_target(
            node,
            ctx,
            queue,
            owner.path,
            camera,
            DrawTarget::Viewport(cubes, sample, owner, &captures.ready),
        );
    }

    fn draw_color_captures(
        &mut self,
        node: &Node,
        context: CaptureContext<'_>,
        cubes: Option<&[Option<CapturedCube>]>,
        stages: Vec<CaptureStage>,
        views: &mut CaptureViews,
    ) {
        if stages.is_empty() {
            return;
        }
        let CaptureContext {
            device,
            queue,
            encoder,
            owner,
            paint,
        } = context;
        let revision = if Self::uses_material_items(node) {
            self.item_texture_revision
        } else {
            0
        };
        let static_probes = cubes.is_none();
        let cubes = cubes.unwrap_or_default();
        let cubes_stamp = cube_revisions(cubes);
        for stage in stages {
            let target = stage.target;
            let inputs = stage.inputs(views);
            let scene = if target.geometry {
                if target.renderables == CaptureRenderables::None || target.region.is_empty() {
                    Node::new("SceneView3D", None)
                } else {
                    geometry_scene(node, revision)
                }
            } else {
                capture_scene(node, target.background, revision, static_probes)
            };
            let Some(primary) = stage.outputs.first() else {
                continue;
            };
            let attachments = stage.attachments();
            let color_format = if attachments & (1 << CaptureAttachment::Color.index()) == 0 {
                HDR_FORMAT
            } else {
                target.color_format
            };
            let work = self.color_captures.prepare(
                device,
                owner,
                primary.name.as_ref(),
                CaptureInput {
                    camera: target.raster_camera(),
                    scene: scene.clone(),
                    size: target.size,
                    color_format,
                    cubes: if target.geometry {
                        Vec::new()
                    } else {
                        cubes_stamp.clone()
                    },
                    dependencies: inputs
                        .iter()
                        .map(|(read, capture)| (read.clone(), capture.revision))
                        .collect(),
                    attachments,
                    composition: stage.composition.clone(),
                    renderables: target.renderables,
                    override_material: stage.override_material.clone(),
                    pipeline_state: target.pipeline_state,
                    region: target.region,
                    depth_clear: target.depth_clear,
                    stencil_clear: target.stencil_clear,
                },
            );
            self.stats.scene_capture_bytes_allocated += work.allocated_bytes;
            if work.render {
                let target_view = if target.geometry {
                    let Some(primary) = &work.views[primary.attachment.index()] else {
                        continue;
                    };
                    primary.view.clone()
                } else {
                    work.views[CaptureAttachment::Color.index()]
                        .as_ref()
                        .map_or_else(
                            || self.stencil_buffers.color(device, target.size),
                            |target| target.view.clone(),
                        )
                };
                let stencil_output = work.views[CaptureAttachment::Stencil.index()].as_ref();
                let stencil_input = stage
                    .composition
                    .stencil
                    .as_ref()
                    .and_then(|read| inputs.get(read));
                let stencil = (stencil_output.is_some()
                    || stencil_input.is_some()
                    || target.pipeline_state.stencil.is_some())
                .then(|| {
                    let (pass, copied) = self.stencil_buffers.prepare(
                        (device, encoder),
                        target.size,
                        stencil_output.map(|output| &output.view),
                        stencil_input.map(|input| &input.view),
                        (
                            target.stencil_clear,
                            target
                                .pipeline_state
                                .stencil
                                .map_or(0, |state| state.reference),
                        ),
                    );
                    self.stats.scene_capture_stencil_copies += u64::from(copied);
                    pass
                });
                let mut path = owner.path.to_vec();
                path.extend([usize::MAX, work.slot]);
                let geometry = target.geometry.then(|| NamedGeometry {
                    views: NAMED_GEOMETRY_ATTACHMENTS.map(|attachment| {
                        work.views[attachment.index()]
                            .as_ref()
                            .map(|capture| &capture.view)
                    }),
                    mask: attachments,
                });
                self.draw_target(
                    &scene,
                    GpuPaintContext {
                        paint,
                        device,
                        encoder: &mut *encoder,
                        target: &target_view,
                        size: target.size,
                        format: target_view.texture().format(),
                    },
                    queue,
                    &path,
                    Some(target.camera),
                    DrawTarget::ColorCapture {
                        cubes,
                        inputs: &inputs,
                        geometry,
                        renderables: target.renderables,
                        override_material: stage.override_material.as_ref(),
                        pipeline_state: target.pipeline_state,
                        region: target.region,
                        depth_clear: target.depth_clear,
                        stencil: stencil.as_ref(),
                        composite: CompositeInput::from_reads(
                            &stage.composition,
                            target.raster_camera(),
                            &inputs,
                        ),
                    },
                );
                self.stats.scene_capture_passes += 1;
            }
            let outputs = work.views.iter().flatten().count();
            self.stats.scene_capture_reuses +=
                (stage.outputs.len() - outputs * usize::from(work.render)) as u64;
            for output in stage.outputs {
                if let Some(view) = &work.views[output.attachment.index()] {
                    views.insert(output, view.clone());
                }
            }
        }
    }

    pub(super) fn capture_reflections(
        &mut self,
        node: &Node,
        (device, queue, encoder): (&wgpu::Device, &wgpu::Queue, &mut wgpu::CommandEncoder),
        (owner, texture_size): (effect_history::Owner<'_>, [u32; 2]),
        camera: SceneCamera,
        paint: crate::PaintContext<'_>,
    ) -> ReflectionFrame {
        let path = owner.path;
        let generation = owner.generation;
        let (Some(Value::SceneObjects(model)), Some(Value::SceneReflections(probes))) = (
            node.properties.get("model"),
            node.properties.get("reflection-probes"),
        ) else {
            return ReflectionFrame::default();
        };
        let geometry = model.geometry(&camera);
        let item_revision = if Self::uses_material_items(node) {
            self.item_texture_revision
        } else {
            0
        };
        let scene = self.scene.get_or_insert_with(|| {
            SceneGpu::new(
                device,
                queue,
                &self.item_textures,
                (
                    self.postprocess.effects.images.clone(),
                    self.postprocess.effects.projections.clone(),
                ),
            )
        });
        let Some(slots) =
            scene
                .environment
                .assignments(probes, geometry.objects(), camera.layer_mask())
        else {
            return ReflectionFrame::default();
        };
        let used = slots.iter().fold(0u16, |mask, &slot| mask | (1 << slot));
        let mut feed_names = vec![BTreeSet::new(); probes.len()];
        if matches!(node.properties.get("scene-captures"), Some(Value::SceneCaptures(feeds)) if !feeds.is_empty())
        {
            for (index, probe) in probes.iter().enumerate() {
                let ReflectionSource::Scene(settings) = probe.source else {
                    continue;
                };
                let layers = probe.layers & camera.layer_mask();
                if used & (1 << (index + 1)) == 0
                    || self.reflections.first_frame_ready(
                        (path, index, generation),
                        probe,
                        settings,
                        layers,
                    )
                {
                    continue;
                }
                // All cubemap faces use the same layers and unmodified mesh LOD.
                if let Some(face) = SceneCamera::reflection_face(probe.position, 0, layers) {
                    let source = capture_node(node, layers);
                    feed_names[index] = material_sources(
                        &source,
                        Some(model),
                        device,
                        face,
                        (true, CaptureRenderables::All),
                        None,
                    );
                }
            }
        }
        let roots: BTreeSet<_> = feed_names.iter().flatten().cloned().collect();
        let mut captures = PreparedCaptures::default();
        if !roots.is_empty() {
            let mut plan = CapturePlan::for_view(node, device, texture_size, Some(camera), &roots);
            let upstream = plan.take_dependencies(&roots);
            // Probe inputs precede scene-derived cubemaps. Static environment
            // inputs remain available without recursively reading the probes.
            self.draw_color_captures(
                node,
                CaptureContext {
                    device,
                    queue,
                    encoder: &mut *encoder,
                    owner,
                    paint,
                },
                None,
                upstream,
                &mut captures.ready,
            );
            captures.plan = Some(plan);
        }
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
                let mut source = capture_node(node, layers);
                if item_revision != 0 {
                    source
                        .properties
                        .insert("item-texture-revision", Value::Int(item_revision as i64));
                }
                let feeds: CaptureViews = feed_names[index]
                    .iter()
                    .filter_map(|name| Some((name.clone(), captures.ready.get(name)?.clone())))
                    .collect();
                let stamp: Vec<_> = feeds
                    .iter()
                    .map(|(name, capture)| (name.clone(), capture.revision))
                    .collect();
                let prepared = reflections.prepare(
                    device,
                    reflection_capture::ProbeInputs {
                        scene: &source,
                        feeds: &stamp,
                        live_items: item_revision != 0,
                    },
                    (path, index, generation),
                    probe,
                    capture,
                    layers,
                );
                needs_redraw |= prepared.needs_redraw;
                if let Some(step) = prepared.step {
                    let particle_camera = (!step.faces.is_empty())
                        .then(|| SceneCamera::reflection_face(step.cube.position, 0, layers))
                        .flatten();
                    let particle_draw = particle_camera
                        .as_ref()
                        .and_then(|camera| ParticleDraw::sample(&step.scene, camera));
                    let particle_batches =
                        particle_draw
                            .as_ref()
                            .zip(particle_camera)
                            .map(|(draw, camera)| {
                                let batches = self.particles.prepare(
                                    draw,
                                    (device, queue, encoder),
                                    camera,
                                    0.0,
                                    (true, step.scene.boolean("shadows", false)),
                                );
                                self.stats.particle_reflection_instances += batches
                                    .models
                                    .iter()
                                    .map(|batch| u64::from(batch.count))
                                    .sum::<u64>();
                                batches
                            });
                    let empty = CaptureViews::new();
                    let face_inputs = if step.use_feeds { &feeds } else { &empty };
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
                            DrawTarget::Reflection(
                                particle_draw.as_ref().zip(particle_batches.as_ref()),
                                face_inputs,
                            ),
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
        ReflectionFrame {
            cubes,
            needs_redraw,
            captures,
        }
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
        let reflection = matches!(kind, DrawTarget::Reflection(..));
        let (
            named_geometry,
            source_filter,
            override_material,
            pipeline_state,
            region,
            depth_clear,
            stencil,
        ) = match kind {
            DrawTarget::ColorCapture {
                geometry,
                renderables,
                override_material,
                pipeline_state,
                region,
                depth_clear,
                stencil,
                ..
            } => (
                geometry,
                renderables,
                override_material,
                pipeline_state,
                region,
                depth_clear,
                stencil,
            ),
            _ => (
                None,
                CaptureRenderables::All,
                None,
                CapturePipelineState::default(),
                CaptureRegion::Full,
                1.0,
                None,
            ),
        };
        let offscreen = !matches!(kind, DrawTarget::Viewport(..));
        let blend_constant = pipeline_state.blend_constant.map(|color| {
            let [r, g, b] = linear_rgb(color);
            let a = f64::from(color.0 as u8) / 255.0;
            wgpu::Color { r, g, b, a }
        });
        let prepare_pass = |pass: &mut wgpu::RenderPass<'_>| {
            if let Some(stencil) = stencil {
                stencil.set_reference(pass);
            }
            if let Some(constant) = blend_constant {
                pass.set_blend_constant(constant);
            }
            region.clip(pass);
        };
        let (scene_cubes, sample, captured_particles, owner, captures, composite) = match kind {
            DrawTarget::Viewport(cubes, sample, owner, captures) => {
                let composite =
                    camera.and_then(|camera| CompositeInput::from_node(node, camera, captures));
                (cubes, sample, None, Some(owner), Some(captures), composite)
            }
            DrawTarget::ColorCapture {
                cubes,
                inputs,
                composite,
                ..
            } => (cubes, None, None, None, Some(inputs), composite),
            DrawTarget::Reflection(particles, captures) => {
                (&[][..], None, particles, None, Some(captures), None)
            }
        };
        let model = match node.properties.get("model") {
            Some(Value::SceneObjects(model)) => Some(model),
            _ => None,
        };
        let renderables = if override_material.is_none() {
            source_filter
        } else if named_geometry.is_some() {
            CaptureRenderables::Opaque
        } else {
            CaptureRenderables::All
        };
        let geometry = model
            .filter(|_| !region.is_empty() && source_filter != CaptureRenderables::None)
            .zip(camera.as_ref())
            .map(|(model, camera)| {
                let geometry = model.geometry(camera);
                match override_material {
                    Some(material) => {
                        geometry.with_capture_material(camera.layer_mask(), source_filter, material)
                    }
                    None => geometry,
                }
            });
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
        let particle_draw = camera
            .as_ref()
            .filter(|_| !reflection && !region.is_empty())
            .and_then(|camera| {
                ParticleDraw::sample_with_capture_material(
                    node,
                    camera,
                    source_filter,
                    override_material,
                )
            });
        let particles = captured_particles
            .map(|(draw, _)| draw)
            .or(particle_draw.as_ref());
        let has_particles = particles.is_some();
        let has_model_particles =
            particles.is_some_and(|draw| draw.model_meshes().next().is_some());
        let has_drawables = has_geometry || has_particles || composite.is_some();
        self.stats.lod_objects = self.stats.lod_objects.saturating_add(
            geometry
                .as_ref()
                .map_or(0, |geometry| geometry.lod_objects as u64),
        );
        if !offscreen && let Some(scene) = &mut self.scene {
            scene.meshes.prune();
        }
        let background = background(node);
        let [r, g, b] = linear_rgb(background);
        let alpha = if reflection {
            1.0
        } else {
            f64::from(background.0.to_be_bytes()[3]) / 255.0
        };
        let clear = wgpu::Color {
            r: r * alpha,
            g: g * alpha,
            b: b * alpha,
            a: alpha,
        };
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
            has_geometry || has_model_particles,
            alpha < 1.0 && (camera.is_none() || !skybox),
        );
        let retain_depth = !offscreen && (post.focus.is_some() || post.needs_effect_depth());
        let raster_size =
            if !offscreen && camera.is_some() && (has_drawables || skybox || fog_background) {
                supersampling::extent(
                    ctx.device,
                    ctx.size,
                    node.float("supersampling").unwrap_or(1.0),
                )
            } else {
                ctx.size
            };
        if !offscreen {
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
            if let Some(named) = named_geometry {
                drop(named.begin(
                    ctx.encoder,
                    stencil.map(|pass| &pass.view),
                    depth_clear,
                    stencil,
                ));
                return;
            }
            if let Some(history) = motion_history {
                *history.borrow_mut() = Default::default();
            }
            let post_target = (!offscreen
                && (supersampled || post.needs_target() || sample.is_some()))
            .then(|| {
                self.postprocess.target(
                    ctx.device,
                    raster_size,
                    post.uses_linear_intermediate(),
                    supersampled,
                )
            });
            let target = post_target.as_ref().unwrap_or(ctx.target);
            viewport_pass(
                ctx.encoder,
                target,
                None,
                stencil.map(|pass| &pass.view),
                false,
                (
                    Some(clear),
                    wgpu::LoadOp::Clear(depth_clear),
                    stencil.map(|pass| pass.operations(true)),
                ),
                false,
            );
            if let Some(source) = &post_target {
                self.resolve_postprocessing(
                    &mut ctx,
                    queue,
                    (source, raster_size),
                    (None, None, captures),
                    (post, sample, None, owner),
                );
            }
            return;
        };
        let clip_offset = sample.map_or([0.0; 2], |sample| sample.clip_offset(raster_size));
        let culling_camera = sample.map_or(camera, |_| camera.with_clip_offset(clip_offset));
        let raster_camera = region.camera(culling_camera);
        let multisampling = !supersampled
            && (has_drawables || fog_background)
            && node.boolean("multisampling", false);
        let scene = self.scene.get_or_insert_with(|| {
            SceneGpu::new(
                ctx.device,
                queue,
                &self.item_textures,
                (
                    self.postprocess.effects.images.clone(),
                    self.postprocess.effects.projections.clone(),
                ),
            )
        });
        if let Some(captures) = captures {
            scene.meshes.materials.captures.clone_from(captures);
        } else {
            scene.meshes.materials.captures.clear();
        }
        let raster = RasterMode::select(ctx.device, pipeline_state.wireframe);
        scene.meshes.expand_triangles = raster == RasterMode::ShaderLine;
        let selected = node.text("selected-object").unwrap_or_default();
        let instances = objects
            .filter(|_| has_geometry)
            .map(|objects| scene.instances.get(ctx.device, queue, objects, selected));
        let culled = geometry
            .as_ref()
            .zip(instances.as_ref())
            .filter(|_| node.properties.get("frustum-culling") != Some(&Value::Bool(false)))
            .map(|(model, set)| {
                scene
                    .instances
                    .visibility(model, set, &culling_camera, layer_filter.as_ref())
            })
            .or_else(|| layer_filter.clone());
        let all = SceneVisibility::all(object_count);
        let visibility = culled.as_deref().unwrap_or(&all);
        let has_custom_blend = particles.is_some_and(ParticleDraw::has_custom_blend);
        let visible_transparency = renderables.includes(true)
            && instances
                .as_ref()
                .is_some_and(|set| set.has_visible_transparency(visibility));
        let ordered_material = renderables.includes(true)
            && instances
                .as_ref()
                .is_some_and(|set| set.requires_sorted_transparency(visibility));
        let force_direct = pipeline_state.blend.is_some()
            || pipeline_state.depth_write == Some(true)
            || pipeline_state.color_write_mask != 15;
        let model_transparency = renderables.includes(true)
            && particles.is_some_and(ParticleDraw::has_model_transparency);
        let weighted = !force_direct
            && (model_transparency
                || (node.boolean("order-independent-transparency", false)
                    && !ordered_material
                    && (particles.is_some_and(ParticleDraw::has_source_over)
                        || visible_transparency)));
        let direct_models = force_direct && model_transparency;
        let weighted_meshes = weighted && !ordered_material;
        let sorted_meshes = visible_transparency && !weighted_meshes;
        let prepasses = instances
            .as_ref()
            .filter(|_| stencil.is_none() && pipeline::uses_material_prepass(pipeline_state))
            .zip(objects)
            .map_or([false; 2], |(set, objects)| {
                set.visible_prepasses(visibility, objects, renderables)
            });
        let mut effects = ScreenSettings::default();
        if !offscreen {
            let visible_material = |receives: &dyn Fn(&Mesh3D) -> bool| {
                objects.is_some_and(|objects| {
                    objects.iter().enumerate().any(|(index, object)| {
                        visibility.contains(index as u32) && receives(&object.mesh)
                    })
                }) || particles.is_some_and(|draw| draw.model_meshes().any(receives))
            };
            effects.reflection = ReflectionSettings::from_node(node)
                .filter(|settings| visible_material(&|mesh| settings.receives(mesh)));
            effects.indirect =
                IndirectSettings::from_node(node).filter(|_| visible_material(&receives_occlusion));
        }
        let visible_transmission = instances
            .as_ref()
            .zip(objects)
            .and_then(|(set, objects)| set.visible_transmission(visibility, objects, renderables))
            .into_iter()
            .chain(particles.and_then(|draw| draw.model_transmission(renderables)))
            .reduce(|a, b| (a.0.max(b.0), a.1 || b.1, a.2 || b.2));
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
            hdr: offscreen || post.uses_linear_intermediate(),
            format_override: (offscreen && named_geometry.is_none() && ctx.format != HDR_FORMAT)
                .then_some(ctx.format),
            reverse_winding: reflection,
            multisampling,
            pass: ColorPass::Opaque,
            renderables,
            pipeline_state: {
                let mut baked = pipeline_state;
                if let Some(stencil) = &mut baked.stencil {
                    stencil.reference = 0;
                }
                baked
            },
            stenciled: stencil.is_some(),
            raster,
            screen_reflections: false,
            screen_indirect_light: false,
            capture_geometry: None,
            named_geometry: false,
            lightmapped: false,
            model_particles: false,
            fog: scattering.mode(&fog_volumes),
        };
        let post_target = (!offscreen && (supersampled || post.needs_target() || sample.is_some()))
            .then(|| {
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
        let continue_opaque = visible_transmission.is_some_and(|(_, writes_depth, _)| {
            !weighted_meshes || writes_depth || has_custom_blend || prepasses[1]
        });
        let captured_depth = depth_snapshot.as_ref().filter(|_| needs_volume_depth);
        let transparency = weighted.then(|| {
            self.transparency.prepare(
                ctx.device,
                raster_size,
                output,
                retain_depth || captured_depth.is_some() || surfaces.is_some(),
                stencil.map(|pass| &pass.view),
            )
        });
        let (depth, multisampled) = if let Some(frame) = &transparency {
            (frame.depth, frame.multisampled_color)
        } else {
            let preserve_depth = retain_depth || capturing || surfaces.is_some() || prepasses[0];
            let pool = if preserve_depth {
                &mut self.sampled_depth
            } else {
                &mut self.depth
            };
            let depth = stencil.map_or_else(
                || {
                    Attachment::get(
                        &mut pool[usize::from(output.multisampling)],
                        ctx.device,
                        raster_size,
                        DEPTH_FORMAT,
                        output.samples(),
                        preserve_depth,
                    )
                },
                |pass| &pass.view,
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
                        (ctx.device, queue),
                        set,
                        visibility,
                        objects,
                        weighted_meshes,
                        (slots, lightmaps),
                    )
                })
        };
        let color_plan = color_plan_for(probe_slots.clone(), true);
        let ao = OcclusionSettings::from_node(node).filter(|_| {
            if !renderables.includes(false)
                || effects
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
        scene.instances.record_transfer_stats(&mut self.stats);
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
        let viewport_particles;
        let particles = if let Some((_, batches)) = captured_particles {
            batches
        } else {
            viewport_particles = if let Some(draw) = particle_draw.as_ref() {
                let batches = self.particles.prepare(
                    draw,
                    (ctx.device, queue, ctx.encoder),
                    raster_camera,
                    weight_scale,
                    (weighted, node.boolean("shadows", false)),
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
                self.stats.particle_model_instances += batches
                    .models
                    .iter()
                    .map(|batch| u64::from(batch.count))
                    .sum::<u64>();
                batches
            } else {
                Default::default()
            };
            &viewport_particles
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
        append_floats(
            &mut uniforms,
            [weight_scale, specular_aa, alpha as f32, 0.0],
        );
        let [width, height] = raster_size.map(|v| v as f32);
        let [strength, radius] = ao.map_or([0.0, 0.0], |parameters| {
            [parameters.strength, parameters.radius]
        });
        append_floats(&mut uniforms, [strength, radius, width, height]);
        let lens_start = uniforms.len();
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
        let sky_uniform =
            (region.maps_viewport() && named_geometry.is_none() && (skybox || fog_background))
                .then(|| {
                    let write = |bytes: &mut [u8], values: &[f32]| {
                        for (bytes, value) in bytes.chunks_exact_mut(size_of::<f32>()).zip(values) {
                            bytes.copy_from_slice(&value.to_ne_bytes());
                        }
                    };
                    write(
                        &mut uniforms[..size_of::<[[f32; 4]; 4]>()],
                        culling_camera.clip_rows().as_flattened(),
                    );
                    write(
                        &mut uniforms[lens_start..lens_start + size_of::<[f32; 4]>()],
                        &culling_camera.clip_lens(),
                    );
                    buffer(
                        ctx.device,
                        "XRack 3D background camera",
                        &uniforms,
                        wgpu::BufferUsages::UNIFORM,
                    )
                });
        let request = ShadowRequest::for_view(
            node,
            (objects.filter(|_| has_geometry), &particles.models),
            lights.lights(),
            camera,
            path,
            layer_filter.as_ref(),
            if scattering.strength > 0.0 {
                volumetric_fog::receiver_corners(&fog_volumes)
            } else {
                None
            },
        );
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
                region,
            });
        let sampler = scene.shadows.sampler.clone();
        let (shadow, particle_shadow_draws) =
            scene
                .shadows
                .prepare(&mut ctx, queue, &mut scene.meshes, geometry, request);
        let lights = scene.lights.get(ctx.device, lights, shadow.slots);
        self.stats.light_buffer_uploads = scene.lights.uploads;
        self.stats.particle_shadow_draws = particle_shadow_draws;
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
                    region: CaptureRegion::Full,
                    ..*draws
                };
                let commands = depth_draws.depth_prepass(
                    (&mut scene.meshes, depth),
                    (ctx.device, queue),
                    ctx.encoder,
                    &frame.depth,
                    &uniform_buffer,
                    (DepthPass::Auxiliary(renderables), wgpu::LoadOp::Clear(1.0)),
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
        let bind_background = |uniforms: &wgpu::Buffer,
                               background: &wgpu::TextureView,
                               texture_sampler: &wgpu::Sampler,
                               captured_depth: &wgpu::Buffer| {
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("XRack 3D globals"),
                layout: &scene.pipelines.globals,
                entries: &[
                    wgpu::BindGroupEntry {
                        binding: 0,
                        resource: uniforms.as_entire_binding(),
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
                        resource: ao_frame
                            .as_ref()
                            .map_or(&scene.occlusion.fallback, |frame| &frame.map)
                            .as_entire_binding(),
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
            &uniform_buffer,
            &scene.background,
            &scene.background_sampler,
            &scene.background_depth,
        );
        let sky_bind_group = sky_uniform.as_ref().map(|uniform| {
            bind_background(
                uniform,
                &scene.background,
                &scene.background_sampler,
                &scene.background_depth,
            )
        });
        let transmitted_group = capture.as_ref().map(|(frame, _, _)| {
            bind_background(
                &uniform_buffer,
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
                        entry.insert(bind_background(&uniform_buffer, &texture, &sampler, buffer));
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
        if let Some(named) = named_geometry {
            let mut pass = named.begin(ctx.encoder, Some(depth), depth_clear, stencil);
            prepare_pass(&mut pass);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(2, &environment_bindings, &[]);
            let capture = ColorOutput {
                capture_geometry: Some(named.output()),
                named_geometry: true,
                ..output
            };
            draw_model_particles(
                &particles.models,
                (&mut scene.meshes, &mut scene.pipelines),
                (ctx.device, queue),
                &mut pass,
                &mut self.stats,
                capture,
            );
            if let Some(draws) = &draws {
                draws.batches(
                    (&mut scene.meshes, &mut scene.pipelines),
                    (ctx.device, queue),
                    &mut pass,
                    &mut self.stats,
                    capture,
                    (None, None),
                );
            }
            self.stats.scene_capture_material_bindings = scene.meshes.capture_binding_builds;
            self.stats.mesh_index_bytes_uploaded = scene.meshes.transfers.index_bytes;
            self.stats.mesh_wireframe_uploads = scene.meshes.transfers.wireframe_meshes;
            self.stats.mesh_deformation_bytes_uploaded = scene.meshes.transfers.deformation_bytes;
            self.stats.indexed_mesh_uploads = scene.meshes.transfers.indexed;
            self.stats.mesh_vertex_bytes_uploaded = scene.meshes.transfers.vertex_bytes;
            return;
        }
        let effect_surface = (!offscreen && post.needs_effect_surface())
            .then(|| {
                self.postprocess
                    .effects
                    .prepare_surface(ctx.device, raster_size, raster_camera)
            })
            .flatten();
        if let Some(surface) = &effect_surface {
            let mut pass = viewport_pass(
                ctx.encoder,
                &surface.view,
                None,
                Some(&surface.depth.view),
                true,
                (
                    Some(wgpu::Color::TRANSPARENT),
                    wgpu::LoadOp::Clear(1.0),
                    None,
                ),
                false,
            );
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(2, &environment_bindings, &[]);
            let capture = ColorOutput {
                hdr: true,
                multisampling: false,
                capture_geometry: Some(GeometryOutput::Surface),
                ..output
            };
            self.stats.custom_effect_surface_passes += 1;
            draw_model_particles(
                &particles.models,
                (&mut scene.meshes, &mut scene.pipelines),
                (ctx.device, queue),
                &mut pass,
                &mut self.stats,
                capture,
            );
            if let Some(draws) = &draws {
                draws.batches(
                    (&mut scene.meshes, &mut scene.pipelines),
                    (ctx.device, queue),
                    &mut pass,
                    &mut self.stats,
                    capture,
                    (None, None),
                );
            }
        }
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
        let draw_direct_models = |pass: &mut wgpu::RenderPass<'_>,
                                  meshes: (&mut MeshCache, &mut Pipelines),
                                  globals: &wgpu::BindGroup,
                                  stats: &mut SceneDrawStats| {
            if direct_models {
                pass.set_bind_group(0, globals, &[]);
                pass.set_bind_group(2, &environment_bindings, &[]);
                draw_model_particles(
                    &particles.models,
                    meshes,
                    (ctx.device, queue),
                    pass,
                    stats,
                    ColorOutput {
                        pass: ColorPass::Transparent,
                        ..output
                    },
                );
            }
        };
        if multisampled.is_some() {
            self.stats.multisample_resolves = self.stats.multisample_resolves.saturating_add(1);
        }
        let opaque = capture
            .as_ref()
            .map_or(target, |(frame, _, _)| &frame.levels[0]);
        let mut material_depth = prepasses.into_iter().any(|enabled| enabled).then(|| {
            scene.material_depth.get_or_insert_with(|| {
                DepthPipelines::new(
                    ctx.device,
                    &scene.pipelines.materials[0],
                    &scene.pipelines.deformations,
                    wgpu::DepthBiasState::default(),
                )
            })
        });
        let material_stage = |transmission| DepthPass::Material {
            transmission,
            cull_override: pipeline_state.cull,
            renderables,
            multisampling: output.multisampling,
            reverse_winding: output.reverse_winding,
        };
        if prepasses[0]
            && let Some((pipeline, draws)) = material_depth.as_deref_mut().zip(draws.as_ref())
        {
            let count = draws.depth_prepass(
                (&mut scene.meshes, pipeline),
                (ctx.device, queue),
                ctx.encoder,
                depth,
                &uniform_buffer,
                (material_stage(false), wgpu::LoadOp::Clear(depth_clear)),
            );
            self.stats.material_depth_prepass_draws += count;
        }
        let depth_load = if prepasses[0] {
            wgpu::LoadOp::Load
        } else {
            wgpu::LoadOp::Clear(depth_clear)
        };
        let (mut pass, opaque_output) = if let Some(surfaces) = &surfaces {
            (
                surfaces.begin(ctx.encoder, depth, clear, multisampled, depth_load),
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
                    (
                        Some(clear),
                        depth_load,
                        stencil.map(|pass| pass.operations(true)),
                    ),
                    capturing,
                ),
                output,
            )
        };
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_bind_group(2, &environment_bindings, &[]);
        let custom_clear = depth_clear != 1.0;
        let background_first = custom_clear || composite.is_some() || region != CaptureRegion::Full;
        if skybox && !fog_background || fog_background && background_first {
            if let Some(group) = &sky_bind_group {
                pass.set_bind_group(0, group, &[]);
            }
            pass.set_pipeline(
                scene
                    .pipelines
                    .skybox(ctx.device, opaque_output, custom_clear),
            );
            pass.draw(0..3, 0..1);
            if sky_bind_group.is_some() {
                pass.set_bind_group(0, &bind_group, &[]);
            }
        }
        if let Some(composite) = composite {
            self.composite_inputs.draw(
                ctx.device,
                &mut pass,
                composite,
                opaque_output,
                clip_offset,
            );
            self.stats.scene_capture_composite_draws += 1;
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(2, &environment_bindings, &[]);
        }
        prepare_pass(&mut pass);
        draw_model_particles(
            &particles.models,
            (&mut scene.meshes, &mut scene.pipelines),
            (ctx.device, queue),
            &mut pass,
            &mut self.stats,
            opaque_output,
        );
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
        if fog_background && !background_first {
            pass.set_pipeline(
                scene
                    .pipelines
                    .skybox(ctx.device, opaque_output, custom_clear),
            );
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(2, &environment_bindings, &[]);
            pass.draw(0..3, 0..1);
        }
        if !capturing && surfaces.is_none() {
            if sorted_meshes && let Some(draws) = &draws {
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
            draw_direct_models(
                &mut pass,
                (&mut scene.meshes, &mut scene.pipelines),
                &bind_group,
                &mut self.stats,
            );
            draw_particles(&mut pass, false);
        }
        drop(pass);
        if let Some(snapshot) = &depth_snapshot {
            let sampled_view = stencil.map(|_| {
                depth.texture().create_view(&wgpu::TextureViewDescriptor {
                    label: Some("XRack capture depth aspect"),
                    aspect: wgpu::TextureAspect::DepthOnly,
                    ..Default::default()
                })
            });
            self.opaque_depth.capture(
                ctx.device,
                ctx.encoder,
                (sampled_view.as_ref().unwrap_or(depth), snapshot),
                raster_size,
                multisampling,
            );
            self.stats.opaque_depth_captures += 1;
            self.stats.volume_depth_captures += u64::from(needs_volume_depth);
        }
        if let Some(surfaces) = &surfaces {
            let resume_blending = !capturing
                && (sorted_meshes
                    || if weighted {
                        has_custom_blend
                    } else {
                        has_particles
                    });
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
                    (
                        None,
                        wgpu::LoadOp::Load,
                        stencil.map(|pass| pass.operations(false)),
                    ),
                    false,
                );
                prepare_pass(&mut pass);
                if sorted_meshes && let Some(draws) = &draws {
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
                draw_direct_models(
                    &mut pass,
                    (&mut scene.meshes, &mut scene.pipelines),
                    &bind_group,
                    &mut self.stats,
                );
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
            if prepasses[1]
                && let Some((pipeline, draws)) = material_depth.zip(draws.as_ref())
            {
                let count = draws.depth_prepass(
                    (&mut scene.meshes, pipeline),
                    (ctx.device, queue),
                    ctx.encoder,
                    depth,
                    &uniform_buffer,
                    (material_stage(true), wgpu::LoadOp::Load),
                );
                self.stats.material_depth_prepass_draws += count;
            }
            if continue_opaque {
                let mut pass = viewport_pass(
                    ctx.encoder,
                    multisampled.unwrap_or(target),
                    multisampled.filter(|_| !weighted).map(|_| target),
                    Some(depth),
                    weighted || retain_depth,
                    (
                        None,
                        wgpu::LoadOp::Load,
                        stencil.map(|pass| pass.operations(false)),
                    ),
                    false,
                );
                prepare_pass(&mut pass);
                pass.set_bind_group(0, resumed_group, &[]);
                pass.set_bind_group(2, &environment_bindings, &[]);
                draw_model_particles(
                    &particles.models,
                    (&mut scene.meshes, &mut scene.pipelines),
                    (ctx.device, queue),
                    &mut pass,
                    &mut self.stats,
                    ColorOutput {
                        pass: ColorPass::Transmission,
                        ..output
                    },
                );
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
                    if sorted_meshes {
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
                draw_direct_models(
                    &mut pass,
                    (&mut scene.meshes, &mut scene.pipelines),
                    resumed_group,
                    &mut self.stats,
                );
                draw_particles(&mut pass, false);
            }
        }
        if let Some(transparency) = &transparency {
            let mut pass = transparency.accumulate(
                ctx.encoder,
                retain_depth,
                stencil.map(|pass| pass.operations(false)),
            );
            prepare_pass(&mut pass);
            if !particles.models.is_empty() {
                pass.set_bind_group(0, resumed_group, &[]);
                pass.set_bind_group(2, &environment_bindings, &[]);
                draw_model_particles(
                    &particles.models,
                    (&mut scene.meshes, &mut scene.pipelines),
                    (ctx.device, queue),
                    &mut pass,
                    &mut self.stats,
                    ColorOutput {
                        pass: ColorPass::Weighted,
                        ..output
                    },
                );
            }
            if weighted_meshes && let Some(draws) = &draws {
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
        self.stats.scene_capture_material_bindings = scene.meshes.capture_binding_builds;
        self.stats.mesh_vertex_bytes_uploaded = scene.meshes.transfers.vertex_bytes;
        self.stats.mesh_index_bytes_uploaded = scene.meshes.transfers.index_bytes;
        self.stats.mesh_wireframe_uploads = scene.meshes.transfers.wireframe_meshes;
        self.stats.mesh_deformation_bytes_uploaded = scene.meshes.transfers.deformation_bytes;
        self.stats.indexed_mesh_uploads = scene.meshes.transfers.indexed;
        if let Some(source) = &post_target {
            self.resolve_postprocessing(
                &mut ctx,
                queue,
                (source, raster_size),
                (focus_depth.as_ref(), effect_surface.as_ref(), captures),
                (post, sample, motion.as_ref(), owner),
            );
        }
    }

    fn resolve_postprocessing(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        source: (&wgpu::TextureView, [u32; 2]),
        (depth, surface, captures): (
            Option<&FocusDepth>,
            Option<&EffectSurface>,
            Option<&CaptureViews>,
        ),
        (settings, sample, motion, owner): (
            PostSettings<'_>,
            Option<&AaSample>,
            Option<&MotionInputs>,
            Option<effect_history::Owner<'_>>,
        ),
    ) {
        let work = self.postprocess.resolve(
            ctx,
            (queue, &self.item_textures, captures),
            source,
            (depth, surface),
            (&settings, sample, motion, owner),
        );
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
        self.stats.custom_effect_passes = self
            .stats
            .custom_effect_passes
            .saturating_add(work.custom_effect_passes);
        self.stats.custom_effect_pipelines = self
            .stats
            .custom_effect_pipelines
            .saturating_add(work.custom_effect_pipelines);
        self.stats.custom_effect_pixels = self
            .stats
            .custom_effect_pixels
            .saturating_add(work.custom_effect_pixels);
        self.stats.custom_effect_target_allocations = self
            .stats
            .custom_effect_target_allocations
            .saturating_add(work.custom_effect_target_allocations);
        self.stats.custom_effect_history_allocations = self
            .stats
            .custom_effect_history_allocations
            .saturating_add(work.custom_effect_history_allocations);
        self.stats.custom_effect_bytes_allocated = self
            .stats
            .custom_effect_bytes_allocated
            .saturating_add(work.custom_effect_bytes_allocated);
        self.stats.alpha_composite_resolves = self
            .stats
            .alpha_composite_resolves
            .saturating_add(u64::from(settings.composites_alpha()));
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
    region: CaptureRegion,
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
        (stage, load): (DepthPass, wgpu::LoadOp<f32>),
    ) -> u64 {
        let globals = pipelines.camera_group(device, camera);
        let mut pass = encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack 3D depth prepass"),
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: target,
                depth_ops: Some(wgpu::Operations {
                    load,
                    store: wgpu::StoreOp::Store,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        if !self.region.clip(&mut pass) {
            return 0;
        }
        pass.set_bind_group(0, &globals, &[]);
        let mut total = 0;
        let mut compacted = None;
        let mut active = None;
        // Color compaction may omit wholly translucent batches. Depth-writing
        // translucent materials need the original visible ranges for this pass.
        let raw = matches!(stage, DepthPass::Material { .. })
            || self.set.has_visible_blended_depth(self.visibility);
        let color_batches = (!raw)
            .then(|| self.set.color_draws(self.plan, self.visibility))
            .into_iter()
            .flatten();
        let raw_batches = raw
            .then_some(self.set.draws.iter())
            .into_iter()
            .flatten()
            .flat_map(|range| self.visibility.runs(range.clone()))
            .map(|range| instances::ColorDraw {
                source: range.start,
                instances: range,
                compacted: false,
            });
        for batch in color_batches.chain(raw_batches) {
            let kind = &self.objects[batch.source as usize].mesh;
            let eligible = match stage {
                DepthPass::Material {
                    transmission,
                    renderables,
                    ..
                } => instances::mesh_material_prepass(kind, renderables, usize::from(transmission)),
                _ => kind.has_auxiliary_depth(),
            };
            if !eligible {
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
                (
                    batch.instances,
                    self.geometry.deformation(batch.source),
                    stage,
                ),
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
        if !output.draws_geometry() {
            return;
        }
        let mut bound_compacted = None;
        let mut active = None;
        let mut active_probe = None;
        let mut active_lightmap = None;
        for draw in self.set.color_draws(self.plan, self.visibility) {
            let kind = &self.objects[draw.source as usize].mesh;
            let has_parts = match output.pass {
                ColorPass::Opaque => kind.has_opaque_parts(),
                ColorPass::Transmission => kind.has_depth_writing_transmission(),
                ColorPass::Transparent | ColorPass::Weighted => !kind.blended_parts().is_empty(),
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
                ColorPass::Transparent | ColorPass::Weighted => part.surface.blend,
            }) {
                if output.capture_geometry.is_some() {
                    if part.depth_coverage.is_none()
                        && output.pipeline_state.depth_write != Some(true)
                    {
                        continue;
                    }
                    if output.named_geometry {
                        stats.scene_capture_geometry_draws += 1;
                    } else {
                        stats.custom_effect_surface_draws += 1;
                    }
                } else {
                    stats.submit(&draw.instances, &part.vertices);
                }
                if lightmapped {
                    stats.lightmapped_draws += 1;
                }
                draw_part(
                    device,
                    pipelines,
                    pass,
                    &mut active,
                    (mesh, part),
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
                (mesh, part),
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
                !material.is_blended()
                    && material.depth_draw_mode != MeshDepthDraw::Never
                    && material.pbr.is_some()
                    && material.transmission_blur().is_none()
            })
        })
}

fn viewport_pass<'a>(
    encoder: &'a mut wgpu::CommandEncoder,
    target: &wgpu::TextureView,
    resolve: Option<&wgpu::TextureView>,
    depth: Option<&wgpu::TextureView>,
    keep_depth: bool,
    (clear, depth_load, stencil_ops): (
        Option<wgpu::Color>,
        wgpu::LoadOp<f32>,
        Option<wgpu::Operations<u32>>,
    ),
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
                load: depth_load,
                store: if keep_depth {
                    wgpu::StoreOp::Store
                } else {
                    wgpu::StoreOp::Discard
                },
            }),
            stencil_ops,
        }),
        ..Default::default()
    })
}

fn draw_model_particles(
    batches: &[ModelParticleBatch],
    (meshes, pipelines): (&mut MeshCache, &mut Pipelines),
    (device, queue): (&wgpu::Device, &wgpu::Queue),
    pass: &mut wgpu::RenderPass<'_>,
    stats: &mut SceneDrawStats,
    output: ColorOutput,
) {
    if !output.draws_geometry() {
        return;
    }
    let output = ColorOutput {
        model_particles: true,
        ..output
    };
    let mut active = None;
    for batch in batches {
        pass.set_vertex_buffer(1, batch.instances.slice(..));
        for prototype in batch.meshes.iter() {
            let mesh = meshes.get(device, queue, prototype);
            mesh.bind(pass);
            for part in &mesh.parts {
                let translucent = batch.fading || part.surface.blend;
                let draw = match output.pass {
                    ColorPass::Opaque => !translucent && !part.surface.transmission,
                    ColorPass::Transmission => !translucent && part.surface.transmission,
                    ColorPass::Transparent | ColorPass::Weighted => translucent,
                };
                if !draw {
                    continue;
                }
                let range = 0..batch.count;
                if output.capture_geometry.is_some() {
                    if part.depth_coverage.is_none()
                        && output.pipeline_state.depth_write != Some(true)
                    {
                        continue;
                    }
                    if output.named_geometry {
                        stats.scene_capture_geometry_draws += 1;
                    } else {
                        stats.custom_effect_surface_draws += 1;
                    }
                } else {
                    stats.submit(&range, &part.vertices);
                    stats.particle_model_draws += 1;
                }
                draw_part(
                    device,
                    pipelines,
                    pass,
                    &mut active,
                    (mesh, part),
                    (range, None),
                    output,
                );
            }
        }
    }
}

#[derive(Clone, Copy, PartialEq)]
struct MeshPipelineKey {
    surface: Surface,
    deformation: Deformation,
    lightmapped: bool,
    shader: Option<(usize, usize)>,
}

fn draw_part(
    device: &wgpu::Device,
    pipelines: &mut Pipelines,
    pass: &mut wgpu::RenderPass<'_>,
    active: &mut Option<MeshPipelineKey>,
    (mesh, part): (&MeshBuffer, &GpuPart),
    (instances, deformation): (Range<u32>, Option<&DeformationBinding>),
    output: ColorOutput,
) {
    let surface = if output.model_particles && output.transparent() {
        Surface {
            blend: true,
            ..part.surface
        }
    } else {
        part.surface
    };
    let shader = part.material.shader.as_ref();
    let selection = MeshPipelineKey {
        surface,
        deformation: deformation.map_or(Deformation::Rigid, |binding| binding.mode),
        lightmapped: output.lightmapped,
        shader: shader.map(|(source, slots)| (Arc::as_ptr(source) as *const () as usize, *slots)),
    };
    if *active != Some(selection) {
        pass.set_pipeline(pipelines.get(
            device,
            surface,
            output,
            selection.deformation,
            shader.map(|(source, slots)| (source, *slots)),
        ));
        *active = Some(selection);
    }
    if let Some(binding) = deformation {
        pass.set_bind_group(3, &binding.group, &[]);
    }
    pass.set_bind_group(1, part.material.color(), &[]);
    mesh.draw_range(pass, part.vertices.clone(), instances);
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

fn mesh_rows<'a, T>(
    rows: &'a [T],
    compact: Option<&'a xrack_runtime::scene3d::MeshDrawIndices>,
) -> impl Iterator<Item = &'a T> {
    let len = compact.map_or(rows.len(), |data| data.rows.len());
    (0..len).map(move |index| {
        let source = compact.map_or(index, |data| data.rows[index] as usize);
        &rows[source]
    })
}

fn mesh_buffer(device: &wgpu::Device, mesh: &Mesh3D, expanded: bool) -> MeshBuffers {
    use xrack_runtime::scene3d::MeshIndexElements;

    let compact = if expanded { None } else { mesh.draw_indices() };
    let vertices = mesh.vertices();
    let count = compact.map_or(vertices.len(), |data| data.rows.len());
    let mut bytes = Vec::with_capacity(count * MeshVertex::STRIDE);
    for vertex in mesh_rows(vertices, compact) {
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
    let index = compact.map(|data| {
        let (bytes, format) = match &data.elements {
            MeshIndexElements::U16(elements) => (
                elements
                    .iter()
                    .copied()
                    .flat_map(u16::to_ne_bytes)
                    .collect::<Vec<_>>(),
                wgpu::IndexFormat::Uint16,
            ),
            MeshIndexElements::U32(elements) => (
                elements
                    .iter()
                    .copied()
                    .flat_map(u32::to_ne_bytes)
                    .collect(),
                wgpu::IndexFormat::Uint32,
            ),
        };
        (
            buffer(
                device,
                "XRack 3D mesh indices",
                &bytes,
                wgpu::BufferUsages::INDEX,
            ),
            format,
        )
    });
    MeshBuffers {
        lightmap_uv: std::sync::OnceLock::new(),
        index,
        vertices: buffer(
            device,
            "XRack 3D unit mesh",
            &bytes,
            wgpu::BufferUsages::VERTEX,
        ),
        skin_weights: mesh.influences().map(|weights| {
            let packed: Vec<_> = mesh_rows(weights, compact)
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
            let bytes: Vec<_> = match compact {
                Some(data) => geometry
                    .packed_draw_rows(&data.rows)
                    .flat_map(u32::to_ne_bytes)
                    .collect(),
                None => geometry.packed_words().flat_map(u32::to_ne_bytes).collect(),
            };
            MorphBuffer {
                entries: buffer(
                    device,
                    "XRack glTF morph deltas",
                    &bytes,
                    wgpu::BufferUsages::STORAGE,
                ),
                record_offset: count as u32 + 1,
            }
        }),
        tangents: (!mesh.tangents().is_empty()).then(|| {
            let bytes: Vec<_> = mesh_rows(mesh.tangents(), compact)
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
    }
}

fn mesh_samples_item_alpha(mesh: &Mesh3D) -> bool {
    mesh.uses_base_items()
        && mesh.materials().iter().any(|material| {
            material.base_item.is_some()
                && material
                    .depth_coverage()
                    .is_some_and(|coverage| coverage.samples_alpha())
        })
}

pub(super) fn background(node: &Node) -> Color {
    node.color("scene-background").unwrap_or(Color(0x202a38ff))
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

#[derive(Clone)]
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
