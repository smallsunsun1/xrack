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
mod fxaa;
pub(super) use custom_effect::has_history as has_effect_history;
mod environment;
mod fog;
mod grid;
mod instances;
pub(crate) mod item_textures;
mod lens_flare;
mod lighting;
mod lightmap;
mod linked_transparency;
mod lut;
mod material;
mod material_debug;
mod model_blend;
pub(crate) use material::material_texture_limit;
mod motion;
mod occlusion;
mod particle_affectors;
mod particle_follow;
mod particle_instances;
mod particle_meshes;
mod particle_sort;
mod particle_textures;
mod particle_tracks;
mod particle_trails;
mod particles;
mod physics_debug;
mod pipeline;
mod postprocess;
mod preload;
mod probe_debug;
mod progressive;
pub(super) mod reflection_capture;
pub(super) mod retained_view;
mod screen_depth;
mod screen_lighting;
mod shadow;
mod shadow_compact;
mod shadow_debug;
mod shadow_tiles;
mod sky_cube;
mod supersampling;
mod temporal;
#[cfg(test)]
mod texture_budget_tests;
mod transmission;
mod transparency;
mod vertex_program;
mod viewport_statistics;
pub(crate) use viewport_statistics::RenderSampleTarget;
mod volumetric_fog;
mod wireframe;

use crate::{GpuPaintContext, Node};
use capture_region::CaptureRegion;
use capture_stencil::{StencilBuffers, StencilPass};
use color_capture::{
    CaptureInput, CaptureMask, CapturePlan, CaptureStage, CaptureViews, ColorCaptures,
    SharedProjectionBuffers, capture_scene, cube_revisions, geometry_scene, material_sources,
};
use composite_input::{CompositeInput, CompositeInputs};
use custom_effect::EffectSurface;
use deform::{Deformation, DeformationBinding, DeformationBuffers, GeometryBindings};
use depth::{DepthPass, DepthPipelines};
use depth_of_field::FocusDepth;
use environment::{
    ColorEnvironments, EnvironmentGpu, EnvironmentInput, LocalReflections, MaterialReflections,
    SkyboxInput,
};
use fog::Fog;
use instances::{ColorPlan, InstanceBuffers, InstanceSet};
use item_textures::{FrozenItemTextures, ItemTextureViews};
use lighting::{LightBuffers, LightSet};
use lightmap::{LightmapBindings, LightmapTextures};
use material::{MaterialBindings, Materials};
use material_debug::MaterialDebug;
use motion::{MotionInputs, MotionPass};
use occlusion::{AmbientOcclusion, OcclusionSettings, OcclusionSource};
use particles::{
    ModelParticleBatch, ParticleBatches, ParticleDepth, ParticleDraw, ParticlePipelines,
};
use pipeline::{
    BinormalSource, ColorOutput, ColorPass, GeometryOutput, NAMED_GEOMETRY_ATTACHMENTS, Pipelines,
    Surface, VolumeMode,
};
use postprocess::{PostProcess, PostSettings, PostStats};
use reflection_capture::{CapturedCube, ProbeBudget, Reflections, capture_node};
use retained_view::{AaSample, DisplayFrame};
use screen_lighting::{
    IndirectSettings, ReflectionSettings, ScreenLighting, ScreenSettings, ScreenTarget,
};
use shadow::{ShadowRequest, Shadows};
use std::{
    collections::{BTreeSet, HashMap},
    ops::Range,
    rc::Rc,
    sync::{Arc, Weak},
};
use transmission::{OpaqueDepth, Transmission};
use transparency::Transparency;
use wgpu::util::DeviceExt;
use wireframe::RasterMode;
use xrack_runtime::{
    Color, Value,
    scene_effect::{
        CaptureAttachment, CaptureAugment, CapturePipelineState, CaptureRenderables,
        MATERIAL_DATA_ATTACHMENTS, encode_capture_depth_clear,
    },
    scene3d::{
        CaptureMaterial, Mesh3D, MeshCullMode, MeshData, MeshDepthCoverage, MeshDepthDraw,
        MeshVertex, MeshVertexStreams, ReflectionSource, SceneCamera, SceneGeometry, SceneObject,
        SceneVisibility,
    },
};

pub(super) const COLOR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba8UnormSrgb;
pub(super) use supersampling::texture_extent;

const HDR_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Rgba16Float;
const DEPTH_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::Depth32Float;
const INSTANCE_STRIDE: u64 = 72;

/// Cumulative counters for SceneView3D color passes on this renderer.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct SceneDrawStats {
    /// Updated scene-captured reflection cubemaps.
    pub reflection_cubemap_updates: u64,
    /// Captured cubemap faces before roughness filtering.
    pub reflection_capture_faces: u64,
    /// Full-screen mip face passes used to prefilter scene cubemaps.
    pub reflection_prefilter_passes: u64,
    /// Texels evaluated by reflection prefiltering, including partial mip faces.
    pub reflection_prefilter_texels: u64,
    /// Viewport passes that draw reflection probe guides.
    pub reflection_debug_draws: u64,
    /// Full-screen draws rasterizing the optional world-space ground grid.
    pub ground_grid_draws: u64,
    /// Viewport passes submitting physical collider line geometry.
    pub physics_debug_draws: u64,
    /// Line segments submitted by the physics debug overlay.
    pub physics_debug_lines: u64,
    /// Vertex buffers created for changed physics debug snapshots.
    pub physics_debug_uploads: u64,
    /// Viewport passes submitting shadow guides.
    pub shadow_debug_draws: u64,
    /// Line segments submitted by the shadow guides.
    pub shadow_debug_lines: u64,
    /// Vertex buffer uploads for changed guide snapshots.
    pub shadow_debug_uploads: u64,
    /// Viewports that recorded their 3D render pass or background clear.
    pub viewport_renders: u64,
    /// Viewports rendered directly into the window color target.
    pub viewport_inline_renders: u64,
    /// 3D viewport textures sampled for 2D composition.
    pub viewport_texture_composites: u64,
    /// Floating-point views converted to premultiplied sRGB for 2D.
    pub viewport_display_resolves: u64,
    /// Viewports composited from a completed offscreen color texture.
    pub viewport_cache_hits: u64,
    /// Color textures allocated for retained viewports.
    pub viewport_cache_allocations: u64,
    /// Dimensions of the most recently painted viewport's composited texture.
    pub last_texture_size: [u32; 2],
    /// Dimensions of the viewport's internal color raster, or zero for a custom pipeline.
    pub last_raster_size: [u32; 2],
    /// Samples per pixel in the most recently recorded viewport raster.
    pub last_sample_count: u32,
    /// Cumulative scene color raster area, counted before MSAA sample expansion.
    pub raster_pixels: u64,
    /// Prepared light buffers uploaded for changed lights or shadow assignments.
    pub light_buffer_uploads: u64,
    /// Instances rejected by the camera frustum, excluding shadow queries.
    pub culled_instances: u64,
    /// Instances excluded by the viewport or named camera's content mask.
    pub layer_filtered_instances: u64,
    /// Instances rejected by the camera distance range after layer filtering.
    pub distance_filtered_instances: u64,
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
    /// Preload steps that transferred mesh buffers or static material images.
    pub preloaded_meshes: u64,
    /// Instanced sprite draws, independent of the number of particles.
    pub particle_draws: u64,
    /// Live sprites submitted to the GPU before hardware clipping.
    pub particle_instances: u64,
    /// Tile and merge dispatches for sorted transparent particle batches.
    pub particle_sort_dispatches: u64,
    /// Opaque depth snapshots sampled by particle intersection fading.
    pub particle_depth_captures: u64,
    /// Decoded sprite sheets uploaded to GPU textures.
    pub particle_texture_uploads: u64,
    /// Model surface sampling tables uploaded for mesh emitters.
    pub particle_mesh_uploads: u64,
    pub model_blend_dispatches: u64,
    pub model_blend_allocations: u64,
    pub model_blend_source_uploads: u64,
    /// Immutable world-space emission tracks transferred to GPU storage.
    pub particle_track_uploads: u64,
    /// Fields uploaded when the visible affector list changes.
    pub particle_affector_uploads: u64,
    /// Submitted intervals in instanced ribbon trails.
    pub particle_trail_segments: u64,
    /// Compute dispatches to sample or trim ribbon paths.
    pub particle_trail_dispatches: u64,
    /// GPU storage allocations for prepared ribbon paths.
    pub particle_trail_allocations: u64,
    /// Instanced geometry draws for model particles, including material passes.
    pub particle_model_draws: u64,
    /// Model particle transforms generated by the GPU.
    pub particle_model_instances: u64,
    /// Opaque or alpha-cutout model particle draws into shadow maps.
    pub particle_shadow_draws: u64,
    /// Scene bounds consumed by new shadow fits, with each authored envelope counted once.
    pub shadow_fit_bounds: u64,
    /// Logical depth texture storage for the last rendered viewport, including retained tiles.
    pub shadow_atlas_bytes: u64,
    /// Updated 16-bit shadow maps converted into the mixed-precision sampling buffer.
    pub shadow_packed_passes: u64,
    /// GPU transforms generated for a capture step, shared by its cubemap faces.
    pub particle_reflection_instances: u64,
    /// Sprite and ribbon batches uploaded for probe capture steps.
    pub particle_reflection_sprite_batches: u64,
    /// Sprite and ribbon batches drawn into the faces of reflection probes.
    pub particle_reflection_sprite_draws: u64,
    /// Particle camera uniforms uploaded after checking the view cache.
    pub particle_view_uploads: u64,
    /// Lighting sets uploaded for sprite and ribbon draws.
    pub particle_lighting_uploads: u64,
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
    /// Native anisotropic sampler variants created for material maps.
    pub material_anisotropic_samplers: u64,
    /// GPU variants compiled for authored material shaders.
    pub custom_material_pipeline_builds: u64,
    /// File image GPU allocations shared by materials and postprocessing.
    pub scene_image_uploads: u64,
    /// Procedural environment cubes generated on the GPU, including their roughness mips.
    pub procedural_sky_generations: u64,
    /// File probe cubemaps prefiltered after attenuation of their lower hemisphere.
    pub environment_horizon_prefilters: u64,
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
    /// Viewports that gathered and resolved per-pixel fragment lists.
    pub linked_transparency_composites: u64,
    /// Bytes reserved for the most recent linked transparency target.
    pub linked_transparency_bytes: u64,
    /// Working set allocations for the per-pixel fragment lists.
    pub linked_transparency_allocations: u64,
    /// Depth-only mesh draws recorded for ambient occlusion.
    pub occlusion_depth_draws: u64,
    /// Depth-only mesh draws requested by materials or the viewport prepass.
    pub material_depth_prepass_draws: u64,
    /// Half-resolution ambient occlusion evaluation passes.
    pub occlusion_evaluations: u64,
    /// Pixels of the occlusion maps evaluated this far.
    pub occlusion_pixels: u64,
    /// Scene-linear HDR viewports resolved into the 2D compositor.
    pub hdr_resolves: u64,
    /// Viewports with color adjustments in the final display conversion.
    pub color_adjustment_resolves: u64,
    /// Full-screen resolves shared by tone mapping, grading and final display effects.
    pub display_resolves: u64,
    /// Format and attachment variants compiled for the display resolve.
    pub display_pipeline_builds: u64,
    /// Display resolves that reused the scene and upstream postprocessing inputs.
    pub display_input_cache_hits: u64,
    /// Screen-space FXAA work. The temporary color input is only used after color conversion.
    pub fxaa_passes: u64,
    pub fxaa_pixels: u64,
    pub fxaa_input_allocations: u64,
    /// Full-screen passes using an authored scene effect.
    pub custom_effect_passes: u64,
    /// Draws of named scene color feeds into offscreen textures.
    pub scene_capture_passes: u64,
    /// Named captures that wrote lit color and geometry in one render pass.
    pub scene_capture_mrt_passes: u64,
    /// Derived mip levels written after named color capture renders.
    pub scene_capture_mip_passes: u64,
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
    /// Quarter-resolution lens flare passes and their pixel work.
    pub lens_flare_passes: u64,
    pub lens_flare_pixels: u64,
    /// Highlight passes exclusive to the flare and viewports that shared bloom's highlights.
    pub lens_flare_highlight_passes: u64,
    pub lens_flare_shared_highlights: u64,
    pub lens_flare_image_uploads: u64,
    pub lens_flare_target_allocations: u64,
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
    probe_debug: probe_debug::ProbeDebug,
    grid: Option<grid::GridPass>,
    debug_lines: physics_debug::DebugLineRenderer,
    shadow_cameras: shadow_debug::FrozenCameras,
    color_captures: ColorCaptures,
    composite_inputs: CompositeInputs,
    stencil_buffers: StencilBuffers,
    item_textures: Arc<ItemTextureViews>,
    item_texture_revision: u64,
    preloads: preload::Preloads,
    scene: Option<SceneGpu>,
    native_anisotropy: bool,
    linked_transparency_supported: bool,
    linked_sample_shading: bool,
    depth: [Option<Attachment>; 2],
    inline_depth: [Option<Attachment>; 2],
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
    pub(crate) viewport_collector: viewport_statistics::ViewportCollector,
}

#[derive(Clone, Copy)]
struct InlineViewport<'a> {
    origin: [u32; 2],
    geometry: &'a Arc<SceneGeometry>,
}

#[derive(Clone, Copy)]
struct ReflectionParticles<'a> {
    draw: &'a ParticleDraw<'a>,
    batches: &'a ParticleBatches,
    view: Option<&'a wgpu::Buffer>,
}

enum DrawTarget<'a> {
    Viewport {
        cubes: &'a [Option<CapturedCube>],
        sample: Option<&'a AaSample>,
        owner: effect_history::Owner<'a>,
        captures: &'a CaptureViews,
        inline: Option<InlineViewport<'a>>,
        retain_display: bool,
        scissor: Option<[f64; 4]>,
    },
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
        augment: Option<&'a CaptureAugment>,
        composite: Option<CompositeInput<'a>>,
    },
    Reflection(
        Option<ReflectionParticles<'a>>,
        &'a CaptureViews,
        Option<&'a FrozenItemTextures>,
    ),
}

#[derive(Clone, Copy)]
struct NamedGeometry<'a> {
    views: [Option<&'a wgpu::TextureView>; NAMED_GEOMETRY_ATTACHMENTS.len()],
    data: [Option<&'a wgpu::TextureView>; MATERIAL_DATA_ATTACHMENTS.len()],
    mask: CaptureMask,
    combined: bool,
}

impl NamedGeometry<'_> {
    fn output(self) -> GeometryOutput {
        let data = self
            .data
            .map(|view| view.map(|view| view.texture().format()));
        if self.combined {
            GeometryOutput::Combined(self.mask, data)
        } else if data.iter().any(Option::is_some) {
            GeometryOutput::Data(data)
        } else {
            GeometryOutput::Named(self.mask)
        }
    }

    fn begin<'a>(
        self,
        encoder: &'a mut wgpu::CommandEncoder,
        depth: Option<&wgpu::TextureView>,
        depth_clear: f32,
        stencil: Option<&StencilPass>,
        color: Option<(&wgpu::TextureView, wgpu::Color)>,
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
        let [normal, base, emissive, captured_depth] = std::array::from_fn(|index| {
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
        let [data0, data1, data2] = self
            .data
            .map(|view| view.map(|view| clear(view, wgpu::Color::TRANSPARENT)));
        let targets = [
            normal,
            base,
            emissive,
            captured_depth,
            color.map(|(view, value)| clear(view, value)),
            data0,
            data1,
            data2,
        ];
        crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
                label: Some(if self.combined {
                    "XRack named scene color and geometry"
                } else {
                    "XRack named scene geometry"
                }),
                color_attachments: {
                    let data_start = NAMED_GEOMETRY_ATTACHMENTS.len() + 1;
                    let data_len = self
                        .data
                        .iter()
                        .rposition(Option::is_some)
                        .map_or(0, |index| index + 1);
                    if self.combined {
                        &targets[..data_start + data_len]
                    } else if data_len != 0 {
                        &targets[data_start..data_start + data_len]
                    } else {
                        &targets[..NAMED_GEOMETRY_ATTACHMENTS.len()]
                    }
                },
                depth_stencil_attachment: depth.map(|view| {
                    wgpu::RenderPassDepthStencilAttachment {
                        view,
                        depth_ops: Some(wgpu::Operations {
                            load: wgpu::LoadOp::Clear(depth_clear),
                            store: wgpu::StoreOp::Discard,
                        }),
                        stencil_ops: stencil.map(|pass| pass.operations(true)),
                    }
                }),
                ..Default::default()
            },
        )
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
    visible: bool,
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
    binormals: Option<wgpu::Buffer>,
    skin_weights: Option<wgpu::Buffer>,
    morph: Option<MorphBuffer>,
    lightmap_uv: std::sync::OnceLock<wgpu::Buffer>,
}

struct MorphBuffer {
    entries: wgpu::Buffer,
    table_layout: [u32; 2],
    binormals: bool,
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
        if let Some(binormals) = &mesh.binormals {
            self.vertex_bytes += binormals.size();
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
        if let Some(binormals) = &self.binormals {
            pass.set_vertex_buffer(5, binormals.slice(..));
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
    model_blends: model_blend::ModelBlendBuffers,
    box_mesh: MeshBuffer,
    sphere_mesh: MeshBuffer,
    wire_sphere: Option<MeshBuffer>,
    materials: Materials,
    item_texture_revision: u64,
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
        item_textures: &Arc<ItemTextureViews>,
        textures: (material::SharedImages, SharedProjectionBuffers, bool),
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
                visible: true,
                mask: false,
                depth_coverage: Some(MeshDepthCoverage::Opaque),
                surface: Surface {
                    blend: false,
                    blend_mode: Default::default(),
                    depth_draw: Default::default(),
                    channel_remapping: false,
                    separate_surface_maps: false,
                    reflection_map: false,
                    height_mapped: false,
                    vertex_colors: true,
                    vertex_masks: false,
                    custom_base_fresnel: false,
                    custom_coat_fresnel: false,
                    transmission: false,
                    volume: VolumeMode::Off,
                    cull: MeshCullMode::None,
                    vertex_tangents: false,
                    binormals: BinormalSource::Derived,
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
                model_blends: Default::default(),
                box_mesh,
                sphere_mesh,
                wire_sphere: None,
                transfers,
                materials,
                item_texture_revision: 0,
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
        self.model_blends.begin_frame(self.frame);
        self.imported
            .retain(|_, entry| entry.source.strong_count() != 0);
        self.captured.retain(|_, entry| {
            self.frame.wrapping_sub(entry.frame) <= 4 && entry.source.strong_count() != 0
        });
        self.geometry
            .retain(|_, (data, _)| data.strong_count() != 0);
    }

    fn get(&mut self, device: &wgpu::Device, queue: &wgpu::Queue, kind: &Mesh3D) -> &MeshBuffer {
        self.get_for_raster(device, queue, kind, RasterMode::Fill)
    }

    fn get_for_raster(
        &mut self,
        device: &wgpu::Device,
        queue: &wgpu::Queue,
        kind: &Mesh3D,
        raster: RasterMode,
    ) -> &MeshBuffer {
        let expand_triangles = raster == RasterMode::ShaderLine;
        match kind {
            Mesh3D::Box => &self.box_mesh,
            Mesh3D::Sphere if expand_triangles && self.sphere_mesh.index.is_some() => {
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
                let expanded = kind.model_blend().is_some()
                    || expand_triangles && kind.draw_indices().is_some();
                let posed_geometry = self.model_blends.get(kind).cloned();
                let posed_builtin = match data.model_blend_primitive() {
                    Some(Mesh3D::Box) => Some(&self.box_mesh.parts),
                    Some(Mesh3D::Sphere) => Some(&self.sphere_mesh.parts),
                    _ => None,
                };
                let box_geometry = &self.box_mesh.geometry;
                let sphere_geometry = &self.sphere_mesh.geometry;
                let materials = &mut self.materials;
                let capture_bound = !data.capture_sources().is_empty();
                let texture_bound = capture_bound || data.uses_item_textures();
                let identity = (Arc::as_ptr(data) as usize, expanded);
                let frame = self.frame;
                let builds = &mut self.capture_binding_builds;
                let uploads = &mut self.uploads;
                let transfers = &mut self.transfers;
                let geometry_cache = &mut self.geometry;
                let key = texture_bound.then(|| {
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
                        .chain(data.item_sources().iter().map(|name| {
                            let allocation = materials
                                .items
                                .get(name)
                                .map_or(0, |image| image.allocation);
                            (allocation, 0)
                        }))
                        .collect();
                    (identity, bindings)
                });
                let create = || {
                    *builds += u64::from(capture_bound);
                    let source = data.vertex_streams();
                    let key = (Arc::as_ptr(source) as usize, expanded);
                    let builtin = data.instance_blend_primitive().and_then(|kind| match kind {
                        Mesh3D::Box if !expanded => Some(box_geometry.clone()),
                        Mesh3D::Sphere if !expanded => Some(sphere_geometry.clone()),
                        _ => None,
                    });
                    let geometry = posed_geometry.clone().or(builtin).unwrap_or_else(|| {
                        geometry_cache
                            .entry(key)
                            .or_insert_with(|| {
                                *uploads = uploads.saturating_add(1);
                                let mesh = mesh_buffer(device, kind, expanded);
                                transfers.upload(&mesh);
                                transfers.wireframe_meshes += u64::from(expanded);
                                (Arc::downgrade(source), Arc::new(mesh))
                            })
                            .1
                            .clone()
                    });
                    if let Some(parts) = posed_builtin {
                        return ImportedMesh {
                            source: Arc::downgrade(data),
                            buffer: MeshBuffer {
                                geometry,
                                parts: parts.clone(),
                            },
                            frame,
                        };
                    }
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
                                    let reflection_map = binding.reflection_map.is_some();
                                    let height_mapped = binding.height_mapped;
                                    let transmission = material.transmission_blur().is_some();
                                    (
                                        binding,
                                        material.alpha_mode.cutoff().is_some(),
                                        Surface {
                                            blend: material.is_blended(),
                                            blend_mode: material.blend_mode,
                                            depth_draw: material.depth_draw_mode,
                                            channel_remapping: material.channels.has_remapping(),
                                            reflection_map,
                                            height_mapped,
                                            separate_surface_maps: pbr.is_some_and(|pbr| {
                                                pbr.separate_surface_maps.is_some()
                                            }),
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
                                                    || !kind.binormals().is_empty()
                                                    || kind.morph_geometry().is_some_and(
                                                        |geometry| geometry.has_binormals(),
                                                    )
                                                    || pbr
                                                        .and_then(|pbr| pbr.tangent_normal_map())
                                                        .is_some()),
                                            binormals: BinormalSource::Derived,
                                            anisotropy,
                                            clearcoat,
                                        },
                                    )
                                });
                            GpuPart {
                                vertices: part.vertices.start as u32..part.vertices.end as u32,
                                material: material.clone(),
                                surface: *surface,
                                visible: kind.materials()[part.material].is_visible(),
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
                if let Some(geometry) = posed_geometry {
                    entry.buffer.geometry = geometry;
                }
                &entry.buffer
            }
        }
    }
}

impl SceneRenderer {
    pub(super) fn new(adapter: &wgpu::Adapter, features: wgpu::Features) -> Self {
        let capabilities = adapter.get_downlevel_capabilities().flags;
        let mut scene = Self {
            native_anisotropy: capabilities.contains(wgpu::DownlevelFlags::ANISOTROPIC_FILTERING),
            linked_transparency_supported: capabilities.contains(
                wgpu::DownlevelFlags::FRAGMENT_WRITABLE_STORAGE
                    | wgpu::DownlevelFlags::READ_ONLY_DEPTH_STENCIL,
            ),
            linked_sample_shading: capabilities
                .contains(wgpu::DownlevelFlags::MULTISAMPLED_SHADING),
            ..Default::default()
        };
        scene.postprocess.effects.formats = custom_effect::OutputFormats::new(adapter, features);
        scene
    }

    fn set_item_textures(&mut self, revision: u64, items: ItemTextureViews) {
        self.item_textures = Arc::new(items);
        self.item_texture_revision = revision;
    }

    pub(super) fn resolve_viewport_display(
        &mut self,
        device: &wgpu::Device,
        encoder: &mut wgpu::CommandEncoder,
        views: (&wgpu::TextureView, &wgpu::TextureView),
    ) {
        self.postprocess
            .area_filter(device)
            .display_viewport(device, encoder, views);
        self.stats.viewport_display_resolves += 1;
    }

    pub(super) fn item_color_texture(&self, name: &str) -> Option<wgpu::Texture> {
        Some(self.item_textures.get(name)?.color_texture.clone())
    }

    pub(super) fn item_texture_revision(&self, node: &Node) -> u64 {
        if self.item_texture_revision == 0 {
            return 0;
        }
        if Self::uses_material_items(node)
            || matches!(node.properties.get("scene-captures"), Some(Value::SceneCaptures(captures))
                if captures.values().filter_map(|capture| capture.augment.as_ref())
                    .any(|augment| augment.item_sources(augment.textures.len()).next().is_some()))
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

    pub(super) fn begin_frame(&mut self, gpu_supported: bool) {
        self.viewport_collector.begin_frame(gpu_supported);
        self.reflections.begin_frame();
        self.shadow_cameras.begin_frame();
        self.color_captures.begin_frame();
        self.postprocess.effects.history.begin_frame();
        self.postprocess.effects.images.borrow_mut().prune();
        if let Some(scene) = &mut self.scene {
            scene.instances.begin_frame();
            scene.meshes.begin_frame();
        }
    }

    pub(super) fn finish_frame(&mut self, submitted: bool) {
        if !submitted {
            self.viewport_collector.clear();
        }
        self.reflections.finish_frame(submitted);
        self.shadow_cameras.finish_frame(submitted);
        self.color_captures.finish_frame(submitted);
        self.postprocess.effects.history.finish_frame(submitted);
        let images = self.postprocess.effects.images.borrow();
        self.stats.material_mip_levels_uploaded = images.mip_levels_uploaded;
        self.stats.scene_image_uploads = images.uploads;
        self.stats.scene_capture_projection_uploads =
            self.postprocess.effects.projections.borrow().uploads;
        if let Some(scene) = &mut self.scene {
            self.stats.material_anisotropic_samplers = scene.meshes.materials.anisotropic_samplers;
            scene.environment.finish_frame(submitted);
            self.stats.procedural_sky_generations = scene.environment.sky_generations;
            self.stats.environment_horizon_prefilters = scene.environment.masked_generations;
            self.stats.custom_material_pipeline_builds = scene.pipelines.material_shader_builds();
            self.stats.scene_capture_material_bindings = scene.meshes.capture_binding_builds;
            self.stats.mesh_vertex_bytes_uploaded = scene.meshes.transfers.vertex_bytes;
            self.stats.mesh_index_bytes_uploaded = scene.meshes.transfers.index_bytes;
            self.stats.mesh_wireframe_uploads = scene.meshes.transfers.wireframe_meshes;
            self.stats.mesh_deformation_bytes_uploaded = scene.meshes.transfers.deformation_bytes;
            self.stats.indexed_mesh_uploads = scene.meshes.transfers.indexed;
            scene
                .environment
                .retain_captured_bindings(|view| self.reflections.contains_view(view));
        }
    }

    pub(super) fn discard_recorded_passes(&mut self) {
        if let Some(scene) = &mut self.scene {
            scene.shadows.discard_recorded_layers();
            scene.occlusion.discard_recorded_maps();
            scene.environment.finish_frame(false);
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

    pub(super) fn visit_viewport(
        &mut self,
        owner: effect_history::Owner<'_>,
        node: &Node,
        feedback: bool,
    ) {
        self.postprocess.effects.history.visit(owner, feedback);
        self.shadow_cameras
            .visit(owner, node.boolean("shadow-freeze-camera", false));
    }

    pub(super) fn draw(
        &mut self,
        node: &Node,
        mut ctx: GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        owner: effect_history::Owner<'_>,
        camera: Option<SceneCamera>,
        (cubes, sample, mut captures): (
            &[Option<CapturedCube>],
            Option<&AaSample>,
            PreparedCaptures,
        ),
        (retain_display, scissor): (bool, Option<[f64; 4]>),
    ) -> Option<Rc<DisplayFrame>> {
        self.stats.viewport_renders = self.stats.viewport_renders.saturating_add(1);
        let plan = captures.plan.take().unwrap_or_else(|| {
            CapturePlan::for_view(
                node,
                (ctx.device, self.postprocess.effects.formats),
                ctx.size,
                camera,
                &BTreeSet::new(),
            )
        });
        self.stats.scene_capture_feedback_edges += plan.feedback_edges;
        let internal_passes = !node.scene_internal_passes_disabled();
        self.draw_color_captures(
            node,
            CaptureContext {
                device: ctx.device,
                queue,
                encoder: &mut *ctx.encoder,
                owner,
                paint: ctx.paint,
            },
            internal_passes.then_some(cubes),
            plan.stages,
            &mut captures.ready,
        );
        if !internal_passes {
            self.draw_custom_pipeline(node, &mut ctx, queue, owner, &captures.ready);
            return None;
        }
        let prepared = self.draw_target(
            node,
            GpuPaintContext {
                encoder: &mut *ctx.encoder,
                ..ctx
            },
            queue,
            owner.path,
            camera,
            DrawTarget::Viewport {
                cubes,
                sample,
                owner,
                captures: &captures.ready,
                inline: None,
                retain_display: retain_display
                    && !node.properties.contains_key("__xr-physics-debug-lines"),
                scissor,
            },
        );
        let region = CaptureRegion::scissor(scissor, ctx.size);
        if let (Some(camera), Some(Value::PhysicsDebugLines(lines))) =
            (camera, node.properties.get("__xr-physics-debug-lines"))
            && let Some((lines, uploaded)) =
                self.debug_lines
                    .draw(&mut ctx, camera, lines.vertices(), region)
        {
            self.stats.physics_debug_draws += 1;
            self.stats.physics_debug_lines += u64::from(lines);
            self.stats.physics_debug_uploads += u64::from(uploaded);
        }
        prepared
    }

    fn draw_custom_pipeline(
        &mut self,
        node: &Node,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        owner: effect_history::Owner<'_>,
        captures: &CaptureViews,
    ) {
        self.stats.last_texture_size = ctx.size;
        self.stats.last_raster_size = [0; 2];
        self.stats.last_sample_count = 0;
        let settings = PostSettings::custom(node);
        let source = composite_input::capture_reads(node)
            .color
            .and_then(|read| captures.get(&read));
        let source = match source {
            Some(capture) => capture.view.clone(),
            None if settings.has_effects() => {
                self.postprocess.transparent_source(ctx.device).clone()
            }
            None => {
                drop(viewport_pass(
                    ctx.encoder,
                    ctx.target,
                    None,
                    None,
                    false,
                    (
                        Some(wgpu::Color::TRANSPARENT),
                        wgpu::LoadOp::Clear(1.0),
                        None,
                    ),
                    false,
                ));
                return;
            }
        };
        self.resolve_postprocessing(
            ctx,
            queue,
            (&source, None),
            (None, None, Some(captures)),
            (settings, None, None, Some(owner), false),
        );
    }

    pub(super) fn draw_inline(
        &mut self,
        node: &Node,
        ctx: GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        owner: effect_history::Owner<'_>,
        camera: SceneCamera,
        (origin, geometry, scissor): ([u32; 2], &Arc<SceneGeometry>, Option<[f64; 4]>),
    ) {
        self.stats.viewport_renders = self.stats.viewport_renders.saturating_add(1);
        self.stats.viewport_inline_renders = self.stats.viewport_inline_renders.saturating_add(1);
        self.draw_target(
            node,
            ctx,
            queue,
            owner.path,
            Some(camera),
            DrawTarget::Viewport {
                cubes: &[],
                sample: None,
                owner,
                captures: &CaptureViews::default(),
                inline: Some(InlineViewport { origin, geometry }),
                retain_display: false,
                scissor,
            },
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
                    mipmaps: target.mipmaps,
                    material_data_formats: std::array::from_fn(|index| {
                        if attachments & (1 << MATERIAL_DATA_ATTACHMENTS[index].index()) != 0 {
                            target.material_data_formats[index]
                        } else {
                            HDR_FORMAT
                        }
                    }),
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
                    augment: stage.augment.clone(),
                    augment_items: stage.augment.as_ref().map_or_else(Vec::new, |augment| {
                        augment
                            .item_sources(augment.textures.len())
                            .map(|name| {
                                self.item_textures
                                    .get(name)
                                    .map(|view| [view.allocation, view.revision])
                            })
                            .collect()
                    }),
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
                    primary.render_view().clone()
                } else {
                    work.views[CaptureAttachment::Color.index()]
                        .as_ref()
                        .map_or_else(
                            || self.stencil_buffers.color(device, target.size),
                            |target| target.render_view().clone(),
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
                let geometry = stage.has_geometry().then(|| NamedGeometry {
                    views: NAMED_GEOMETRY_ATTACHMENTS.map(|attachment| {
                        work.views[attachment.index()]
                            .as_ref()
                            .map(|capture| &capture.view)
                    }),
                    data: MATERIAL_DATA_ATTACHMENTS.map(|attachment| {
                        work.views[attachment.index()]
                            .as_ref()
                            .map(|capture| &capture.view)
                    }),
                    mask: attachments,
                    combined: !target.geometry,
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
                        augment: stage.augment.as_ref(),
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
                if let Some(levels) = work.views[CaptureAttachment::Color.index()]
                    .as_ref()
                    .and_then(|color| color.mip_views.as_deref())
                {
                    self.postprocess.area_filter(device).resolve_mips(
                        device,
                        encoder,
                        levels,
                        target.size,
                        color_format,
                    );
                    self.stats.scene_capture_mip_passes += (levels.len() - 1) as u64;
                }
                self.stats.scene_capture_passes += 1;
                self.stats.scene_capture_mrt_passes +=
                    u64::from(!target.geometry && geometry.is_some());
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
        let (Some(Value::SceneObjects(model)), Some(Value::SceneReflections(probes))) = (
            node.properties.get("model"),
            node.properties.get("reflection-probes"),
        ) else {
            return ReflectionFrame::default();
        };
        let geometry = model.render_geometry(
            &camera,
            f64::from(node.length("lod-fade-width").unwrap_or_default()),
        );
        let scene = self.scene.get_or_insert_with(|| {
            SceneGpu::new(
                device,
                queue,
                &self.item_textures,
                (
                    self.postprocess.effects.images.clone(),
                    self.postprocess.effects.projections.clone(),
                    self.native_anisotropy,
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
        let visible = geometry.distance_visibility(camera.eye_position());
        let used = slots
            .iter()
            .enumerate()
            .filter(|&(index, _)| {
                visible
                    .as_ref()
                    .is_none_or(|visible| visible.contains(index as u32))
            })
            .fold(0u16, |mask, (_, &slot)| mask | (1 << slot));
        let mut feed_names = vec![BTreeSet::new(); probes.len()];
        let mut item_inputs = vec![ItemTextureViews::new(); probes.len()];
        let has_feeds = matches!(node.properties.get("scene-captures"), Some(Value::SceneCaptures(feeds)) if !feeds.is_empty());
        let has_items = !self.item_textures.is_empty();
        if has_feeds || has_items {
            for (index, probe) in probes.iter().enumerate() {
                let ReflectionSource::Scene(settings) = probe.source else {
                    continue;
                };
                let layers = probe.layers & camera.layer_mask();
                if used & (1 << (index + 1)) == 0
                    || self
                        .reflections
                        .first_frame_ready((owner, index, layers), probe, settings)
                {
                    continue;
                }
                // All cubemap faces use the same layers and unmodified mesh LOD.
                if let Some(face) = SceneCamera::reflection_face(probe.position, 0, layers) {
                    let source = capture_node(node, layers);
                    let mut item_names = BTreeSet::new();
                    let feeds = material_sources(
                        &source,
                        Some(model),
                        device,
                        face,
                        (true, CaptureRenderables::All, false),
                        None,
                        has_items.then_some(&mut item_names),
                    );
                    if has_feeds {
                        feed_names[index] = feeds;
                    }
                    item_inputs[index].extend(item_names.into_iter().filter_map(|name| {
                        let view = self.item_textures.get(&name)?.clone();
                        Some((name, view))
                    }));
                }
            }
        }
        let roots: BTreeSet<_> = feed_names.iter().flatten().cloned().collect();
        let mut captures = PreparedCaptures::default();
        if !roots.is_empty() {
            let mut plan = CapturePlan::for_view(
                node,
                (device, self.postprocess.effects.formats),
                texture_size,
                Some(camera),
                &roots,
            );
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
        let (budget_slot, mut budget) =
            reflections.frame_budget(owner, ProbeBudget::from_node(node));
        let mut order = Vec::new();
        if budget_slot.is_some() {
            order.extend(0..probes.len());
            order.sort_by_key(|&index| {
                let layers = probes[index].layers & camera.layer_mask();
                (reflections.last_work((owner, index, layers)), index)
            });
        }
        let mut needs_redraw = false;
        let mut cubes = vec![None; probes.len()];
        for index in (0..probes.len()).map(|index| order.get(index).copied().unwrap_or(index)) {
            let probe = &probes[index];
            let ReflectionSource::Scene(capture) = probe.source else {
                continue;
            };
            if used & (1 << (index + 1)) == 0 {
                continue;
            }
            let layers = probe.layers & camera.layer_mask();
            let source = capture_node(node, layers);
            let feeds: CaptureViews = feed_names[index]
                .iter()
                .filter_map(|name| Some((name.clone(), captures.ready.get(name)?.clone())))
                .collect();
            let prepared = reflections.prepare(
                device,
                reflection_capture::ProbeInputs {
                    scene: &source,
                    feeds: &feeds,
                    items: &item_inputs[index],
                    item_revision: self.item_texture_revision,
                },
                (owner, index, layers),
                probe,
                capture,
                &mut budget,
            );
            needs_redraw |= prepared.needs_redraw;
            if let Some(step) = prepared.step {
                let particle_camera = (!step.faces.is_empty())
                    .then(|| SceneCamera::reflection_face(step.cube.position, 0, layers))
                    .flatten();
                let particle_draw = particle_camera
                    .as_ref()
                    .and_then(|camera| ParticleDraw::sample(&step.scene, camera));
                let mut particle_batches =
                    particle_draw
                        .as_ref()
                        .zip(particle_camera)
                        .map(|(draw, camera)| {
                            let weighted = draw.has_model_transparency()
                                || step.scene.boolean("order-independent-transparency", false);
                            let batches = self.particles.prepare(
                                draw,
                                (device, queue, encoder),
                                (camera.layer_mask(), None),
                                (weighted, step.scene.boolean("shadows", false)),
                            );
                            self.stats.particle_reflection_instances += batches
                                .models
                                .iter()
                                .map(|batch| u64::from(batch.count))
                                .sum::<u64>();
                            self.stats.particle_reflection_sprite_batches +=
                                (batches.direct.len() + batches.weighted.len()) as u64;
                            (
                                self.stats.particle_trail_dispatches,
                                self.stats.particle_trail_allocations,
                            ) = self.particles.trail_work();
                            batches
                        });
                let empty = CaptureViews::new();
                let face_inputs = if let Some(frozen) = &step.frozen_inputs {
                    &frozen.feeds.views
                } else if step.use_feeds {
                    &feeds
                } else {
                    &empty
                };
                for face in step.faces.clone() {
                    let face_view = step.cube.face(face);
                    let face_camera =
                        SceneCamera::reflection_face(step.cube.position, face as usize, layers);
                    let view =
                        face_camera
                            .zip(particle_batches.as_mut())
                            .and_then(|(camera, batches)| {
                                let weight = if batches.weighted.is_empty() {
                                    0.0
                                } else {
                                    transparency_weight_scale(&camera)
                                };
                                self.particles.reflection_view(
                                    (device, encoder),
                                    batches,
                                    camera,
                                    weight,
                                )
                            });
                    if view.is_some()
                        && let Some(batches) = &particle_batches
                    {
                        self.stats.particle_reflection_sprite_draws +=
                            (batches.direct.len() + batches.weighted.len()) as u64;
                    }
                    self.stats.particle_sort_dispatches = self.particles.sort_dispatches();
                    self.stats.particle_view_uploads = self.particles.view_uploads();
                    self.stats.particle_lighting_uploads = self.particles.lighting_uploads();
                    let face_particles = particle_draw.as_ref().zip(particle_batches.as_ref()).map(
                        |(draw, batches)| ReflectionParticles {
                            draw,
                            batches,
                            view: view.as_ref(),
                        },
                    );
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
                            face_particles,
                            face_inputs,
                            step.frozen_inputs.as_ref().map(|frozen| &frozen.items),
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
                self.stats.reflection_prefilter_texels = self
                    .stats
                    .reflection_prefilter_texels
                    .saturating_add(step.filter_texels);
            }
            cubes[index] = prepared.output;
        }
        reflections.store_budget(budget_slot, budget);
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
    ) -> Option<Rc<DisplayFrame>> {
        let reflection = matches!(kind, DrawTarget::Reflection(..));
        let (inline, scissor) = match &kind {
            DrawTarget::Viewport {
                inline, scissor, ..
            } => (*inline, *scissor),
            _ => (None, None),
        };
        let inline_origin = inline.map(|viewport| viewport.origin);
        let (
            named_geometry,
            source_filter,
            override_material,
            mut pipeline_state,
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
        let custom_clear = depth_clear != 1.0;
        let reverse_depth = camera.is_some_and(|camera| camera.reverse_depth());
        let depth_clear = if reverse_depth {
            1.0 - depth_clear
        } else {
            depth_clear
        };
        let offscreen = !matches!(kind, DrawTarget::Viewport { .. });
        let requested_raster = if !offscreen && camera.is_some() {
            supersampling::extent(
                ctx.device,
                ctx.size,
                node.float("supersampling").unwrap_or(1.0),
            )
        } else {
            ctx.size
        };
        let region = if offscreen {
            region
        } else {
            CaptureRegion::scissor(scissor, requested_raster)
                .translated(inline_origin.unwrap_or([0, 0]))
        };
        let display_region = CaptureRegion::scissor(scissor, ctx.size);
        if !offscreen {
            pipeline_state.wireframe = node.boolean("debug-wireframe", false);
        }
        let diagnostic = if offscreen {
            MaterialDebug::None
        } else {
            MaterialDebug::from_node(node)
        };
        let debug_material = diagnostic.active();
        let ground_grid = if offscreen || debug_material {
            None
        } else {
            grid::GridSettings::from_node(node)
        };
        let shadow_debug = if offscreen {
            Default::default()
        } else {
            shadow_debug::Settings::from_node(node)
        };
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
        let (scene_cubes, sample, captured_particles, owner, captures, composite, retain_display) =
            match kind {
                DrawTarget::Viewport {
                    cubes,
                    sample,
                    owner,
                    captures,
                    retain_display,
                    ..
                } => {
                    let composite = camera
                        .filter(|_| !region.is_empty())
                        .and_then(|camera| CompositeInput::from_node(node, camera, captures));
                    (
                        cubes,
                        sample,
                        None,
                        Some(owner),
                        Some(captures),
                        composite,
                        retain_display,
                    )
                }
                DrawTarget::ColorCapture {
                    cubes,
                    inputs,
                    composite,
                    ..
                } => (cubes, None, None, None, Some(inputs), composite, false),
                DrawTarget::Reflection(particles, captures, _) => {
                    (&[][..], None, particles, None, Some(captures), None, false)
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
        let geometry = if region.is_empty() {
            None
        } else if let Some(viewport) = inline {
            Some(viewport.geometry.clone())
        } else {
            model
                .filter(|_| source_filter != CaptureRenderables::None)
                .zip(camera.as_ref())
                .map(|(model, camera)| {
                    let geometry = model.render_geometry(
                        camera,
                        f64::from(node.length("lod-fade-width").unwrap_or_default()),
                    );
                    match override_material {
                        Some(material) => geometry.with_capture_material(
                            camera.layer_mask(),
                            source_filter,
                            material,
                        ),
                        None => geometry,
                    }
                })
        };
        let lod_fade = geometry.as_ref().is_some_and(|geometry| geometry.lod_fade);
        let objects = geometry
            .as_ref()
            .map(|geometry| geometry.objects())
            .filter(|objects| !objects.is_empty());
        let object_count = objects.map_or(0, |objects| objects.len());
        let (layer_count, eligibility) = geometry.as_ref().zip(camera.as_ref()).map_or(
            (object_count, None),
            |(geometry, camera)| {
                let mut layers = geometry.layer_visibility(camera.layer_mask());
                if reflection && let Some(casters) = geometry.reflection_visibility() {
                    layers = Some(layers.as_ref().map_or_else(
                        || casters.clone(),
                        |layers| SceneVisibility::intersect(layers, &casters),
                    ));
                }
                let layer_count = layers
                    .as_ref()
                    .map_or(object_count, |mask| mask.visible_count());
                let distance = geometry.distance_visibility(camera.eye_position());
                let eligible = match (layers, distance) {
                    (Some(layers), Some(distance)) => {
                        Some(SceneVisibility::intersect(&layers, &distance))
                    }
                    (layers, distance) => layers.or(distance),
                };
                (layer_count, eligible)
            },
        );
        let eligible_count = eligibility
            .as_ref()
            .map_or(object_count, |mask| mask.visible_count());
        self.stats.layer_filtered_instances = self
            .stats
            .layer_filtered_instances
            .saturating_add((object_count - layer_count) as u64);
        self.stats.distance_filtered_instances = self
            .stats
            .distance_filtered_instances
            .saturating_add((layer_count - eligible_count) as u64);
        let has_geometry = eligible_count != 0;
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
            .map(|particles| particles.draw)
            .or(particle_draw.as_ref());
        let has_particles = particles.is_some();
        let has_model_particles =
            particles.is_some_and(|draw| draw.model_meshes().next().is_some());
        let has_drawables =
            has_geometry || has_particles || composite.is_some() || ground_grid.is_some();
        self.stats.lod_objects = self.stats.lod_objects.saturating_add(
            geometry
                .as_ref()
                .map_or(0, |geometry| geometry.lod_objects as u64),
        );
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
        let sky = (!debug_material)
            .then(|| SkyboxInput::from_node(node, environment.as_ref()))
            .flatten();
        let skybox = sky.is_some();
        let fog_volumes = if debug_material {
            Vec::new()
        } else {
            volumetric_fog::prepare(node, camera.as_ref())
        };
        let fog_background = !fog_volumes.is_empty();

        let probes = match node.properties.get("reflection-probes") {
            Some(Value::SceneReflections(probes)) if !probes.is_empty() => Some(probes),
            _ => None,
        };
        let debug_probes = !offscreen
            && camera.is_some_and(|camera| {
                probes.is_some_and(|probes| {
                    probes
                        .iter()
                        .any(|probe| probe_debug::visible(probe, camera.layer_mask()))
                })
            });
        let retain_display = retain_display && !debug_probes && !shadow_debug.active();
        let composite_alpha = alpha < 1.0 && (camera.is_none() || !skybox);
        let mut post = if debug_material {
            PostSettings::unprocessed(composite_alpha)
        } else {
            PostSettings::from_node(
                node,
                camera.is_some()
                    && (environment.is_some() || skybox || probes.is_some() && has_geometry),
                has_geometry || has_model_particles,
                composite_alpha,
            )
        };
        if !offscreen && matches!(ctx.format, HDR_FORMAT | wgpu::TextureFormat::Rgba32Float) {
            post.require_linear_intermediate();
        }
        let retain_depth =
            !offscreen && (debug_probes || post.focus.is_some() || post.needs_effect_depth());
        let raster_size =
            if (has_drawables || skybox || fog_background) && (offscreen || !region.is_empty()) {
                requested_raster
            } else {
                ctx.size
            };
        if !offscreen {
            self.stats.last_texture_size = ctx.size;
            self.stats.last_raster_size = raster_size;
            self.stats.last_sample_count = 1;
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
        let Some(camera) = camera.filter(|_| {
            (offscreen || !region.is_empty())
                && (debug_probes
                    || has_drawables
                    || environment.is_some()
                    || skybox
                    || fog_background)
        }) else {
            if inline_origin.is_some() {
                return None;
            }
            if let Some(named) = named_geometry {
                drop(named.begin(
                    ctx.encoder,
                    stencil.map(|pass| &pass.view),
                    depth_clear,
                    stencil,
                    named.combined.then_some((ctx.target, clear)),
                ));
                return None;
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
                return self.resolve_postprocessing(
                    &mut ctx,
                    queue,
                    (source, Some(raster_size)),
                    (None, None, captures),
                    (post, sample, None, owner, retain_display),
                );
            }
            return retain_display.then(|| self.postprocess.pin_direct(ctx.target));
        };
        let clip_offset = sample.map_or([0.0; 2], |sample| sample.clip_offset(raster_size));
        let culling_camera = sample.map_or(camera, |_| camera.with_clip_offset(clip_offset));
        let raster_camera = region.camera(culling_camera);
        let multisampling = !supersampled
            && (has_drawables || fog_background)
            && node.boolean("multisampling", false);
        if !offscreen && multisampling {
            self.stats.last_sample_count = 4;
        }
        let scene = self.scene.get_or_insert_with(|| {
            SceneGpu::new(
                ctx.device,
                queue,
                &self.item_textures,
                (
                    self.postprocess.effects.images.clone(),
                    self.postprocess.effects.projections.clone(),
                    self.native_anisotropy,
                ),
            )
        });
        let augment = match kind {
            DrawTarget::ColorCapture { augment, .. } => augment,
            _ => None,
        };
        scene.pipelines.capture_augment = augment.map(|augment| augment.source.clone());
        if let DrawTarget::Reflection(_, _, Some(items)) = kind {
            scene.meshes.materials.items.clone_from(&items.views);
            scene.meshes.item_texture_revision = items.revision;
        } else {
            scene.meshes.materials.items.clone_from(&self.item_textures);
            scene.meshes.item_texture_revision = self.item_texture_revision;
        }
        if let Some(captures) = captures {
            scene.meshes.materials.captures.clone_from(captures);
        } else {
            scene.meshes.materials.captures.clear();
        }
        let augment_slots = augment.map_or(0, |augment| augment.textures.len());
        scene
            .pipelines
            .prepare_augment_layout(ctx.device, augment_slots);
        scene.pipelines.capture_augment_slots = augment_slots;
        let augment_textures = augment.filter(|_| augment_slots != 0).map(|augment| {
            let materials = &scene.meshes.materials;
            let (flags, views, _) = materials.external_textures(
                ctx.device,
                queue,
                &augment.textures,
                augment_slots,
                false,
            );
            let projections =
                materials.external_projections(ctx.device, &augment.textures, augment_slots);
            (flags, views, projections)
        });
        let raster = RasterMode::select(ctx.device, pipeline_state.wireframe);
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
                    .visibility(model, set, &culling_camera, eligibility.as_ref())
            })
            .or_else(|| eligibility.clone());
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
        let oit_method = (!offscreen).then(|| node.text("oit-method")).flatten();
        let force_direct = oit_method == Some("sorted")
            || pipeline_state.blend.is_some()
            || pipeline_state.depth_write == Some(true)
            || pipeline_state.color_write_mask != 15;
        let model_transparency = renderables.includes(true)
            && particles.is_some_and(ParticleDraw::has_model_transparency);
        let wants_weighted = oit_method.map_or_else(
            || node.boolean("order-independent-transparency", false),
            |mode| matches!(mode, "weighted" | "linked-list"),
        );
        let weighted = !force_direct
            && (model_transparency
                || (wants_weighted
                    && (!ordered_material && visible_transparency
                        || (reflection || !ordered_material)
                            && particles.is_some_and(ParticleDraw::has_source_over))));
        let direct_models = force_direct && model_transparency;
        let weighted_meshes = weighted && !ordered_material;
        let sorted_meshes = visible_transparency && !weighted_meshes;
        let (prepasses, scene_depth_prepass) = instances
            .as_ref()
            .filter(|_| stencil.is_none() && pipeline::uses_material_prepass(pipeline_state))
            .zip(objects)
            .map_or(([false; 2], false), |(set, objects)| {
                (
                    set.visible_prepasses(visibility, objects, renderables),
                    !offscreen
                        && node.boolean("depth-prepass", false)
                        && set.has_visible_scene_depth(visibility, objects),
                )
            });
        let opaque_prepass = prepasses[0] || scene_depth_prepass;
        let mut effects = ScreenSettings::default();
        if !offscreen && !debug_material {
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
        let particle_depth = particles.and_then(|draw| {
            let direct = draw.depth_fade(weighted)?;
            if named_geometry.is_some() {
                return None;
            }
            let opaque_geometry = || {
                objects.is_some_and(|objects| {
                    objects.iter().enumerate().any(|(index, object)| {
                        visibility.contains(index as u32) && object.mesh.has_opaque_parts()
                    })
                }) || draw
                    .model_meshes_with_fading()
                    .any(|(mesh, fading)| !fading && mesh.has_opaque_parts())
            };
            (composite.is_some_and(CompositeInput::has_depth)
                || renderables.includes(false) && opaque_geometry())
            .then(|| ParticleDepth::parameters(raster_camera, raster_size, depth_clear))
            .flatten()
            .map(|parameters| (direct, parameters))
        });
        let depth_snapshot =
            (particle_depth.is_some() || needs_volume_depth || multisampling && effects.active())
                .then(|| self.opaque_depth.prepare(ctx.device, raster_size))
                .flatten();
        let (soft_direct, soft_depth) = match particle_depth.zip(depth_snapshot.as_ref()) {
            Some(((direct, parameters), pixels)) => (
                direct,
                Some(ParticleDepth::new(ctx.device, pixels.clone(), parameters)),
            ),
            None => (false, None),
        };
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
            format_override: (inline_origin.is_some()
                || offscreen
                    && named_geometry.is_none_or(|geometry| geometry.combined)
                    && ctx.format != HDR_FORMAT)
                .then_some(ctx.format),
            encode_srgb: inline_origin.is_some() && !ctx.format.is_srgb(),
            reverse_winding: reflection ^ raster_camera.reverses_winding(),
            reverse_depth,
            lod_fade,
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
            linked_transparency: false,
            debug_material,
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
        let defer_direct_sprites = soft_direct && !capturing && surfaces.is_none();
        let continue_opaque = visible_transmission.is_some_and(|(_, writes_depth, _)| {
            !weighted_meshes || writes_depth || has_custom_blend || prepasses[1]
        });
        let captured_depth = depth_snapshot.as_ref().filter(|_| needs_volume_depth);
        let linked = weighted
            && oit_method == Some("linked-list")
            && self.linked_transparency_supported
            && (!multisampling
                || (self.linked_sample_shading
                    && ctx.device.limits().max_inter_stage_shader_variables
                        >= 15 + u32::from(raster == RasterMode::ShaderLine)))
            && !ordered_material
            && !has_custom_blend
            && linked_texture_budget(ctx.device, objects, visibility, particles);
        let transparency = weighted.then(|| {
            self.transparency.prepare(
                ctx.device,
                raster_size,
                output,
                (
                    retain_depth || depth_snapshot.is_some() || surfaces.is_some(),
                    if linked {
                        region.pixels(raster_size)
                    } else {
                        None
                    },
                ),
                stencil.map(|pass| &pass.view),
            )
        });
        let linked_frame = transparency
            .as_ref()
            .and_then(|frame| frame.linked.as_ref());
        if let Some(frame) = linked_frame {
            scene
                .pipelines
                .prepare_linked_layout(ctx.device, multisampling);
            frame.clear(ctx.encoder);
            self.stats.linked_transparency_bytes = frame.bytes();
            self.stats.linked_transparency_allocations = frame.allocations;
        }
        let (depth, multisampled) = if inline_origin.is_some() {
            let target = ctx.target.texture();
            (
                Attachment::get(
                    &mut self.inline_depth[usize::from(opaque_prepass)],
                    ctx.device,
                    [target.width(), target.height()],
                    DEPTH_FORMAT,
                    1,
                    opaque_prepass,
                ),
                None,
            )
        } else if let Some(frame) = &transparency {
            (frame.depth, frame.multisampled_color)
        } else {
            let preserve_depth = retain_depth
                || capturing
                || surfaces.is_some()
                || opaque_prepass
                || depth_snapshot.is_some();
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
                let persistent = capturing || surfaces.is_some() || defer_direct_sprites;
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
            .saturating_add((eligible_count - visibility.visible_count()) as u64);
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
            (diagnostic == MaterialDebug::AmbientOcclusion
                || lights.has_ambient()
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
        let mut uniforms = Vec::with_capacity(336 + volumetric_fog::VOLUME_UNIFORM_BYTES);
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
        let weight_scale = if weighted {
            transparency_weight_scale(&camera)
        } else {
            0.0
        };
        let viewport_particle_view = particle_draw
            .as_ref()
            .filter(|draw| draw.needs_view(weighted))
            .map(|_| self.particles.view(ctx.device, raster_camera, weight_scale));
        self.stats.particle_view_uploads = self.particles.view_uploads();
        let viewport_particles;
        let (particles, particle_view) = if let Some(particles) = captured_particles {
            (particles.batches, particles.view)
        } else {
            viewport_particles = if let Some(draw) = particle_draw.as_ref() {
                let batches = self.particles.prepare(
                    draw,
                    (ctx.device, queue, ctx.encoder),
                    (raster_camera.layer_mask(), viewport_particle_view.as_ref()),
                    (weighted, node.boolean("shadows", false)),
                );
                self.stats.particle_draws += (batches.direct.len() + batches.weighted.len()) as u64;
                self.stats.particle_instances += batches
                    .direct
                    .iter()
                    .chain(&batches.weighted)
                    .map(|batch| u64::from(batch.instances))
                    .sum::<u64>();
                self.stats.particle_sort_dispatches = self.particles.sort_dispatches();
                self.stats.particle_texture_uploads = self.particles.texture_uploads();
                self.stats.particle_lighting_uploads = self.particles.lighting_uploads();
                (
                    self.stats.particle_trail_dispatches,
                    self.stats.particle_trail_allocations,
                ) = self.particles.trail_work();
                self.stats.particle_mesh_uploads = self.particles.mesh_uploads();
                self.stats.particle_track_uploads = self.particles.track_uploads();
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
            (&viewport_particles, viewport_particle_view.as_ref())
        };
        let mut draw_particles = |pass: &mut wgpu::RenderPass<'_>, accumulation, scene_depth| {
            let Some(view) = particle_view else {
                return;
            };
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
                (
                    accumulation,
                    linked_frame
                        .filter(|_| accumulation)
                        .map(|frame| (frame, depth)),
                ),
                (view, particles.affectors.as_ref(), scene_depth),
            );
        };
        let specular_aa = f32::from(u8::from(node.boolean("specular-aa", false)));
        append_floats(
            &mut uniforms,
            [
                weight_scale,
                specular_aa,
                alpha as f32,
                if raster_camera.has_custom_projection() {
                    2.0
                } else {
                    f32::from(u8::from(raster_camera.has_oblique_depth()))
                },
            ],
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
        let [[rx, ry, rz], [ux, uy, uz]] = raster_camera.lateral_axes();
        append_floats(
            &mut uniforms,
            [rx, ry, rz, f32::from(reverse_depth), ux, uy, uz, 0.0],
        );
        append_floats(
            &mut uniforms,
            raster_camera.custom_world_viewer().unwrap_or([0.0; 4]),
        );
        let inverse = if raster_camera.has_custom_projection() {
            raster_camera.inverse_clip_rows()
        } else {
            None
        };
        append_floats(
            &mut uniforms,
            inverse.unwrap_or([[0.0; 4]; 4]).into_iter().flatten(),
        );
        let [x, y, z] = augment.map_or([0.0; 3], |augment| augment.parameters);
        append_floats(&mut uniforms, [x, y, z]);
        uniforms.extend_from_slice(&(diagnostic as u32).to_ne_bytes());
        let augment_flags = augment_textures
            .as_ref()
            .map_or([0; 4], |(flags, _, _)| *flags);
        uniforms.extend(augment_flags.into_iter().flat_map(u32::to_ne_bytes));
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
        let cascade_camera = owner.map_or(camera, |owner| {
            self.shadow_cameras.reference(
                owner,
                node.boolean("shadow-freeze-camera", false),
                camera,
            )
        });
        let request = ShadowRequest::for_view(
            node,
            (objects.filter(|_| has_geometry), &particles.models),
            lights.lights(),
            (camera, cascade_camera),
            (path, shadow_debug),
            eligibility.as_ref(),
            if scattering.strength > 0.0 {
                volumetric_fog::receiver_corners(&fog_volumes)
            } else {
                None
            },
        );
        let has_shadows = request.is_some();
        if let Some(draw_geometry) = geometry.as_ref() {
            let mesh_frame = scene.meshes.frame;
            let blends = &mut scene.meshes.model_blends;
            for &index in draw_geometry.model_blends() {
                let object = &draw_geometry.objects()[index as usize];
                if visibility.contains(index)
                    || has_shadows
                        && object.casts_shadow
                        && eligibility
                            .as_ref()
                            .is_none_or(|filter| filter.contains(index))
                {
                    blends.prepare(ctx.device, ctx.encoder, mesh_frame, &object.mesh);
                }
            }
            self.stats.model_blend_dispatches = blends.dispatches;
            self.stats.model_blend_allocations = blends.allocations;
            self.stats.model_blend_source_uploads = blends.source_uploads;
        }
        let deformations = scene.deformations.prepare(
            ctx.device,
            queue,
            (&mut scene.meshes, RasterMode::Fill),
            objects.map_or(&[][..], Arc::as_ref),
            |index, object| {
                visibility.contains(index)
                    || has_shadows
                        && object.casts_shadow
                        && eligibility
                            .as_ref()
                            .is_none_or(|filter| filter.contains(index))
            },
        );
        // Morph indices are in the draw stream's vertex order. Filled auxiliary
        // passes keep compact buffers while the portable line shader uses triangle corners.
        let wire_deformations = objects
            .filter(|objects| {
                raster == RasterMode::ShaderLine
                    && objects.iter().enumerate().any(|(index, object)| {
                        visibility.contains(index as u32)
                            && object.morph_weights().is_some()
                            && object.mesh.draw_indices().is_some()
                    })
            })
            .map(|objects| {
                scene.deformations.prepare(
                    ctx.device,
                    queue,
                    (&mut scene.meshes, raster),
                    objects,
                    |index, _| visibility.contains(index),
                )
            });
        let geometry = GeometryBindings {
            instances: instance_buffer,
            instance_data: instances
                .as_ref()
                .and_then(|set| set.data.as_ref())
                .map(|data| &data.buffer),
            offsets: instances.as_ref().map_or(&[], |set| set.buffer_offsets()),
            zero_data: &scene.instances.zero_data,
            deformations: &deformations,
            wire_deformations: wire_deformations.as_deref(),
            lod_fade,
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
                            item_revision: scene.meshes.item_texture_revision,
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
                inline_rect: inline_origin.map(|origin| (origin, ctx.size)),
            });
        let sampler = scene.shadows.sampler.clone();
        let shadow = scene
            .shadows
            .prepare(&mut ctx, queue, &mut scene.meshes, geometry, request);
        let lights = scene.lights.get(ctx.device, lights, shadow.atlas.slots);
        self.stats.light_buffer_uploads = scene.lights.uploads;
        self.stats.particle_shadow_draws = shadow.particle_draws;
        self.stats.shadow_fit_bounds = shadow.fit_bounds;
        self.stats.shadow_atlas_bytes = shadow.atlas.bytes;
        self.stats.shadow_packed_passes = shadow.packed_passes;
        let guides = shadow.guides;
        let shadow = shadow.atlas;
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
                    (
                        DepthPass::Auxiliary {
                            renderables,
                            reverse_winding: raster_camera.reverses_winding(),
                            reverse_depth,
                            lod_fade,
                        },
                        wgpu::LoadOp::Clear(raster_camera.depth_clear()),
                    ),
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
                               captured_depth: &wgpu::Buffer,
                               linked: bool| {
            let entries = [
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
                wgpu::BindGroupEntry {
                    binding: pipeline::PACKED_SHADOW_BINDING,
                    resource: shadow.packed.as_entire_binding(),
                },
            ];
            let extended;
            let entries =
                if let Some(frame) = linked_frame.filter(|_| linked) {
                    extended = entries
                        .into_iter()
                        .chain(frame.capture_entries(depth))
                        .collect::<Vec<_>>();
                    extended.as_slice()
                } else if let Some((_, views, projections)) = &augment_textures {
                    extended =
                        entries
                            .into_iter()
                            .chain(views.iter().enumerate().map(|(slot, view)| {
                                wgpu::BindGroupEntry {
                                    binding: pipeline::AUGMENT_TEXTURE_BINDING + slot as u32,
                                    resource: wgpu::BindingResource::TextureView(view),
                                }
                            }))
                            .chain(std::iter::once(wgpu::BindGroupEntry {
                                binding: pipeline::AUGMENT_CAPTURE_BINDING,
                                resource: projections.as_entire_binding(),
                            }))
                            .collect::<Vec<_>>();
                    extended.as_slice()
                } else {
                    &entries
                };
            ctx.device.create_bind_group(&wgpu::BindGroupDescriptor {
                label: Some("XRack 3D globals"),
                layout: scene
                    .pipelines
                    .globals_layout(augment_slots, linked, multisampling),
                entries,
            })
        };

        let bind_group = bind_background(
            &uniform_buffer,
            &scene.background,
            &scene.background_sampler,
            &scene.background_depth,
            false,
        );
        let sky_bind_group = sky_uniform.as_ref().map(|uniform| {
            bind_background(
                uniform,
                &scene.background,
                &scene.background_sampler,
                &scene.background_depth,
                false,
            )
        });
        let transmitted_group = capture.as_ref().map(|(frame, _, _)| {
            bind_background(
                &uniform_buffer,
                &frame.sampled,
                &scene.background_sampler,
                captured_depth.unwrap_or(&scene.background_depth),
                false,
            )
        });
        let resumed_group = transmitted_group.as_ref().unwrap_or(&bind_group);
        let linked_group = linked_frame.map(|_| {
            let (image, depth) = capture.as_ref().map_or(
                (&scene.background, &scene.background_depth),
                |(frame, _, _)| {
                    (
                        &frame.sampled,
                        captured_depth.unwrap_or(&scene.background_depth),
                    )
                },
            );
            bind_background(
                &uniform_buffer,
                image,
                &scene.background_sampler,
                depth,
                true,
            )
        });

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
                        entry.insert(bind_background(
                            &uniform_buffer,
                            &texture,
                            &sampler,
                            buffer,
                            false,
                        ));
                    }
                }
            }
            (!mapped.is_empty()).then_some(LightmapBindings {
                base: &bind_group,
                mapped,
            })
        });
        self.stats.lightmap_uploads = scene.lightmaps.uploads;

        let environment_bindings =
            scene
                .environment
                .bind(ctx.device, queue, ctx.encoder, environment.as_ref());
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
        let panoramas = MaterialReflections::new(
            ctx.device,
            scene.pipelines.panorama.clone(),
            environment.as_ref(),
        );
        let reflections = ColorEnvironments {
            fallback: &environment_bindings,
            local: local.as_ref(),
            material: &panoramas,
        };
        let global_reflections = ColorEnvironments {
            local: None,
            ..reflections
        };
        if let Some(named) = named_geometry {
            let mut pass = named.begin(
                ctx.encoder,
                Some(depth),
                depth_clear,
                stencil,
                named.combined.then_some((ctx.target, clear)),
            );
            let capture = ColorOutput {
                capture_geometry: Some(named.output()),
                named_geometry: true,
                ..output
            };
            if let Some(composite) = composite.filter(|_| named.combined) {
                self.composite_inputs
                    .draw(ctx.device, &mut pass, composite, capture, clip_offset);
                self.stats.scene_capture_composite_draws += 1;
            }
            prepare_pass(&mut pass);
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(2, &environment_bindings, &[]);
            draw_model_particles(
                global_reflections,
                (&particles.models, geometry.zero_data),
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
                    (
                        if named.combined {
                            reflections
                        } else {
                            global_reflections
                        },
                        lightmaps.as_ref().filter(|_| named.combined),
                    ),
                );
            }
            return None;
        }
        let skybox_bindings =
            scene
                .environment
                .bind_skybox(ctx.device, queue, ctx.encoder, sky.as_ref());
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
                    wgpu::LoadOp::Clear(raster_camera.depth_clear()),
                    None,
                ),
                false,
            );
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(2, &environment_bindings, &[]);
            prepare_pass(&mut pass);
            let capture = ColorOutput {
                hdr: true,
                multisampling: false,
                capture_geometry: Some(GeometryOutput::Surface),
                ..output
            };
            self.stats.custom_effect_surface_passes += 1;
            draw_model_particles(
                global_reflections,
                (&particles.models, geometry.zero_data),
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
                    (global_reflections, None),
                );
            }
        }
        let draw_direct_models = |pass: &mut wgpu::RenderPass<'_>,
                                  meshes: (&mut MeshCache, &mut Pipelines),
                                  globals: &wgpu::BindGroup,
                                  stats: &mut SceneDrawStats| {
            if direct_models {
                pass.set_bind_group(0, globals, &[]);
                pass.set_bind_group(2, &environment_bindings, &[]);
                draw_model_particles(
                    global_reflections,
                    (&particles.models, geometry.zero_data),
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
        let mut material_depth = (opaque_prepass || prepasses[1]).then(|| {
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
            scene_opaque: scene_depth_prepass && !transmission,
            explicit: prepasses[usize::from(transmission)],
            cull_override: pipeline_state.cull,
            renderables,
            multisampling: output.multisampling,
            reverse_winding: output.reverse_winding,
            reverse_depth,
            lod_fade,
        };
        if opaque_prepass
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
        let depth_load = if opaque_prepass {
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
                        .filter(|_| capturing || !weighted && !defer_direct_sprites)
                        .map(|_| opaque),
                    Some(depth),
                    weighted || retain_depth || capturing || depth_snapshot.is_some(),
                    (
                        inline_origin.is_none().then_some(clear),
                        depth_load,
                        stencil.map(|pass| pass.operations(true)),
                    ),
                    capturing,
                ),
                output,
            )
        };
        if let Some(origin) = inline_origin {
            set_inline_rect(&mut pass, origin, ctx.size);
        }
        pass.set_bind_group(0, &bind_group, &[]);
        pass.set_bind_group(2, &environment_bindings, &[]);
        if !offscreen {
            prepare_pass(&mut pass);
        }
        let background_first =
            custom_clear || composite.is_some() || offscreen && region != CaptureRegion::Full;
        if skybox && !fog_background || fog_background && background_first {
            if let Some(group) = &sky_bind_group {
                pass.set_bind_group(0, group, &[]);
            }
            if let Some(group) = &skybox_bindings {
                pass.set_bind_group(2, group, &[]);
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
            if skybox_bindings.is_some() {
                pass.set_bind_group(2, &environment_bindings, &[]);
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
        if offscreen {
            prepare_pass(&mut pass);
        }
        draw_model_particles(
            global_reflections,
            (&particles.models, geometry.zero_data),
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
            pass.set_bind_group(
                2,
                skybox_bindings.as_ref().unwrap_or(&environment_bindings),
                &[],
            );
            pass.draw(0..3, 0..1);
            if skybox_bindings.is_some() {
                pass.set_bind_group(2, &environment_bindings, &[]);
            }
        }
        if let Some(style) = ground_grid
            && self
                .grid
                .get_or_insert_with(|| grid::GridPass::new(ctx.device))
                .draw(ctx.device, &mut pass, raster_camera, opaque_output, style)
        {
            self.stats.ground_grid_draws += 1;
            pass.set_bind_group(0, &bind_group, &[]);
            pass.set_bind_group(2, &environment_bindings, &[]);
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
            if !defer_direct_sprites {
                draw_particles(&mut pass, false, None);
            }
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
                (multisampling, reverse_depth),
            );
            self.stats.opaque_depth_captures += 1;
            self.stats.volume_depth_captures += u64::from(needs_volume_depth);
            self.stats.particle_depth_captures += u64::from(soft_depth.is_some());
        }
        if defer_direct_sprites {
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
            draw_particles(&mut pass, false, soft_depth.as_ref());
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
                draw_particles(&mut pass, false, soft_depth.as_ref());
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
                    global_reflections,
                    (&particles.models, geometry.zero_data),
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
                draw_particles(&mut pass, false, soft_depth.as_ref());
            }
        }
        if let Some(transparency) = &transparency {
            let mut pass = transparency.accumulate(
                ctx.encoder,
                retain_depth,
                stencil.map(|pass| pass.operations(false)),
            );
            prepare_pass(&mut pass);
            let transparency_group = linked_group.as_ref().unwrap_or(resumed_group);
            if !particles.models.is_empty() {
                pass.set_bind_group(0, transparency_group, &[]);
                pass.set_bind_group(2, &environment_bindings, &[]);
                draw_model_particles(
                    global_reflections,
                    (&particles.models, geometry.zero_data),
                    (&mut scene.meshes, &mut scene.pipelines),
                    (ctx.device, queue),
                    &mut pass,
                    &mut self.stats,
                    ColorOutput {
                        pass: ColorPass::Weighted,
                        linked_transparency: linked_frame.is_some(),
                        ..output
                    },
                );
            }
            if weighted_meshes && let Some(draws) = &draws {
                pass.set_bind_group(0, transparency_group, &[]);
                pass.set_bind_group(2, &environment_bindings, &[]);
                draws.batches(
                    (&mut scene.meshes, &mut scene.pipelines),
                    (ctx.device, queue),
                    &mut pass,
                    &mut self.stats,
                    ColorOutput {
                        pass: ColorPass::Weighted,
                        linked_transparency: linked_frame.is_some(),
                        ..output
                    },
                    (reflections, None),
                );
            }
            draw_particles(&mut pass, true, soft_depth.as_ref());
            drop(pass);
            transparency.composite(ctx.encoder, target);
            self.stats.linked_transparency_composites += u64::from(linked_frame.is_some());
            self.stats.transparency_composites =
                self.stats.transparency_composites.saturating_add(1);
        }
        let motion = motion_history.and_then(|history| {
            let mut history = history.borrow_mut();
            let Some((draws, objects)) = draws
                .as_ref()
                .zip(objects)
                .filter(|_| !display_region.is_empty())
            else {
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
                (draws, objects, raster_camera, raster, display_region),
                &mut history,
            );
            self.stats.motion_vector_draws += count;
            Some(inputs)
        });
        let prepared = if let Some(source) = &post_target {
            self.resolve_postprocessing(
                &mut ctx,
                queue,
                (source, Some(raster_size)),
                (focus_depth.as_ref(), effect_surface.as_ref(), captures),
                (post, sample, motion.as_ref(), owner, retain_display),
            )
        } else {
            retain_display.then(|| self.postprocess.pin_direct(ctx.target))
        };
        if debug_probes
            && !display_region.is_empty()
            && let (Some(probes), Some(depth)) = (probes, focus_depth.as_ref())
        {
            self.probe_debug
                .draw(&mut ctx, probes, camera, depth, clip_offset, display_region);
            self.stats.reflection_debug_draws += 1;
        }
        if let Some(guides) = guides
            && let Some((lines, uploaded)) =
                self.debug_lines
                    .draw(&mut ctx, camera, &guides, display_region)
        {
            self.stats.shadow_debug_draws += 1;
            self.stats.shadow_debug_lines += u64::from(lines);
            self.stats.shadow_debug_uploads += u64::from(uploaded);
        }
        prepared
    }

    fn resolve_postprocessing(
        &mut self,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        source: (&wgpu::TextureView, Option<[u32; 2]>),
        (depth, surface, captures): (
            Option<&FocusDepth>,
            Option<&EffectSurface>,
            Option<&CaptureViews>,
        ),
        (settings, sample, motion, owner, retain_display): (
            PostSettings<'_>,
            Option<&AaSample>,
            Option<&MotionInputs>,
            Option<effect_history::Owner<'_>>,
            bool,
        ),
    ) -> Option<Rc<DisplayFrame>> {
        let (work, prepared) = self.postprocess.resolve(
            ctx,
            (queue, &self.item_textures, captures),
            source,
            (depth, surface),
            (&settings, sample, motion, owner, retain_display),
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
        if let Some(raster_size) = source.1.filter(|&size| size != ctx.size) {
            self.stats.supersample_resolves = self.stats.supersample_resolves.saturating_add(1);
            self.stats.supersample_pixels = self
                .stats
                .supersample_pixels
                .saturating_add(raster_size.into_iter().map(u64::from).product());
        }
        self.record_display_work(&settings, &work);
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
        self.stats.bloom_passes = self.stats.bloom_passes.saturating_add(work.bloom_passes);
        self.stats.bloom_pixels = self.stats.bloom_pixels.saturating_add(work.bloom_pixels);
        self.stats.bloom_target_allocations = self
            .stats
            .bloom_target_allocations
            .saturating_add(work.bloom_target_allocations);
        self.stats.lens_flare_passes += work.lens_flare_passes;
        self.stats.lens_flare_pixels += work.lens_flare_pixels;
        self.stats.lens_flare_highlight_passes += work.lens_flare_highlight_passes;
        self.stats.lens_flare_shared_highlights += work.lens_flare_shared_highlights;
        self.stats.lens_flare_image_uploads += work.lens_flare_image_uploads;
        self.stats.lens_flare_target_allocations += work.lens_flare_target_allocations;
        if work.focus_filtered_pixels != 0 {
            self.stats.depth_of_field_passes = self.stats.depth_of_field_passes.saturating_add(3);
            self.stats.depth_of_field_pixels = self
                .stats
                .depth_of_field_pixels
                .saturating_add(work.focus_filtered_pixels);
        }
        prepared
    }

    pub(super) fn reprocess_display(
        &mut self,
        node: &Node,
        ctx: &mut GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        frame: &DisplayFrame,
    ) {
        let settings = PostSettings::for_retained(node, frame);
        let work = self
            .postprocess
            .render_display(ctx, queue, frame, &settings);
        self.record_display_work(&settings, &work);
        self.stats.display_input_cache_hits += 1;
    }

    fn record_display_work(&mut self, settings: &PostSettings<'_>, work: &PostStats) {
        self.stats.fxaa_passes += work.fxaa_passes;
        self.stats.fxaa_pixels += work.fxaa_pixels;
        self.stats.fxaa_input_allocations += work.fxaa_input_allocations;
        self.stats.hdr_resolves = self
            .stats
            .hdr_resolves
            .saturating_add(u64::from(settings.hdr));
        self.stats.color_adjustment_resolves = self
            .stats
            .color_adjustment_resolves
            .saturating_add(u64::from(settings.has_adjustments()));
        self.stats.display_resolves = self
            .stats
            .display_resolves
            .saturating_add(work.display_resolves);
        self.stats.display_pipeline_builds = self
            .stats
            .display_pipeline_builds
            .saturating_add(work.display_pipeline_builds);
        self.stats.alpha_composite_resolves = self
            .stats
            .alpha_composite_resolves
            .saturating_add(u64::from(settings.composites_alpha()));
        self.stats.lut_resolves = self.stats.lut_resolves.saturating_add(work.lut_resolves);
        self.stats.lut_uploads = self.stats.lut_uploads.saturating_add(work.lut_uploads);
    }
}

struct MeshColorDraws<'a> {
    set: &'a InstanceSet,
    objects: &'a [SceneObject],
    visibility: &'a SceneVisibility,
    geometry: GeometryBindings<'a>,
    plan: Option<&'a ColorPlan>,
    region: CaptureRegion,
    inline_rect: Option<([u32; 2], [u32; 2])>,
}

impl MeshColorDraws<'_> {
    fn bind_instances(
        &self,
        pass: &mut wgpu::RenderPass<'_>,
        active: &mut Option<(bool, u32)>,
        draw: &instances::ColorDraw,
    ) {
        let offset = if draw.compacted {
            0
        } else {
            self.geometry.offset(draw.instances.start)
        };
        let state = (draw.compacted, offset);
        if *active == Some(state) {
            return;
        }
        if draw.compacted
            && let Some(plan) = self.plan
            && let Some(instances) = &plan.compact
        {
            let data = plan
                .compact_data
                .as_ref()
                .map_or(self.geometry.zero_data, |data| &data.buffer);
            deform::bind_instance_streams(pass, instances, data);
        } else {
            self.geometry.bind_instances(pass, offset);
        }
        *active = Some(state);
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
        let mut pass = crate::viewport_gpu_timing::render_pass(
            encoder,
            &wgpu::RenderPassDescriptor {
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
            },
        );
        if let Some((origin, size)) = self.inline_rect {
            set_inline_rect(&mut pass, origin, size);
        }
        if !self.region.clip(&mut pass) {
            return 0;
        }
        pass.set_bind_group(0, &globals, &[]);
        let mut total = 0;
        let mut bound_instances = None;
        let mut active = None;
        // Color compaction may omit wholly translucent batches. Depth-writing
        // translucent materials need the original visible ranges for this pass.
        let raw = match stage {
            DepthPass::Material { explicit, .. } => explicit,
            _ => self.set.has_visible_blended_depth(self.visibility),
        };
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
            let object = &self.objects[batch.source as usize];
            let kind = &object.mesh;
            let eligible = match stage {
                DepthPass::Material {
                    transmission,
                    scene_opaque,
                    explicit,
                    renderables,
                    ..
                } => {
                    scene_opaque && kind.has_opaque_depth_prepass()
                        || explicit
                            && instances::mesh_material_prepass(
                                kind,
                                renderables,
                                usize::from(transmission),
                            )
                }
                _ => kind.has_auxiliary_depth(),
            };
            if !eligible {
                continue;
            }
            self.bind_instances(&mut pass, &mut bound_instances, &batch);
            total += pipelines.draw(
                device,
                &mut pass,
                &mut active,
                meshes.get(device, queue, kind),
                (
                    batch.instances,
                    self.geometry.deformation(batch.source),
                    object.is_reflected(),
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
        (reflections, lightmaps): (ColorEnvironments<'_>, Option<&LightmapBindings<'_>>),
    ) {
        if !output.draws_geometry() {
            return;
        }
        let mut bound_instances = None;
        let mut active = None;
        let mut active_environment = reflections.fallback.clone();
        let mut active_lightmap = None;
        for draw in self.set.color_draws(self.plan, self.visibility) {
            let object = &self.objects[draw.source as usize];
            let kind = &object.mesh;
            let has_parts = match output.pass {
                ColorPass::Opaque => kind.has_opaque_parts(),
                ColorPass::Transmission => kind.has_depth_writing_transmission(),
                ColorPass::Transparent | ColorPass::Weighted => !kind.blended_parts().is_empty(),
            };
            if !has_parts {
                continue;
            }
            self.bind_instances(pass, &mut bound_instances, &draw);
            let mesh = meshes.get_for_raster(device, queue, kind, output.raster);
            mesh.bind(pass);
            let mut lightmapped = false;
            if let Some(bindings) = lightmaps {
                let map = object
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
            for part in mesh.parts.iter().filter(|part| {
                part.visible
                    && match output.pass {
                        ColorPass::Opaque => !part.surface.blend && !part.surface.transmission,
                        ColorPass::Transmission => !part.surface.blend && part.surface.transmission,
                        ColorPass::Transparent | ColorPass::Weighted => part.surface.blend,
                    }
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
                }
                if output.capture_geometry.is_none_or(GeometryOutput::combined) {
                    stats.submit(&draw.instances, &part.vertices);
                }
                if lightmapped {
                    stats.lightmapped_draws += 1;
                }
                reflections.bind(
                    pass,
                    &mut active_environment,
                    Some(draw.source),
                    part.material.reflection_map.as_ref(),
                );
                draw_part(
                    device,
                    pipelines,
                    pass,
                    &mut active,
                    (mesh, part),
                    (
                        draw.instances.clone(),
                        self.geometry.deformation_for(draw.source, output.raster),
                    ),
                    ColorOutput {
                        lightmapped,
                        reverse_winding: output.reverse_winding ^ object.is_reflected(),
                        ..output
                    },
                );
            }
        }
        reflections.restore(pass, &active_environment);
    }

    fn sorted_transparency(
        &self,
        (meshes, pipelines): (&mut MeshCache, &mut Pipelines),
        (device, queue): (&wgpu::Device, &wgpu::Queue),
        pass: &mut wgpu::RenderPass<'_>,
        stats: &mut SceneDrawStats,
        output: ColorOutput,
        camera: &SceneCamera,
        reflections: ColorEnvironments<'_>,
    ) {
        let mut draws = self
            .set
            .transparent_draws(camera, self.objects, self.visibility)
            .peekable();
        if draws.peek().is_none() {
            return;
        }
        let mut bound_instances = None;
        let mut active_mesh = None;
        let mut active = None;
        let mut active_environment = reflections.fallback.clone();
        while let Some(draw) = draws.next() {
            self.geometry
                .bind_instance_range(pass, &mut bound_instances, draw.instance);
            let object = &self.objects[draw.instance as usize];
            let kind = &object.mesh;
            let mesh = meshes.get_for_raster(device, queue, kind, output.raster);
            if active_mesh != Some(kind) {
                mesh.bind(pass);
                active_mesh = Some(kind);
            }
            let part = &mesh.parts[draw.part];
            let mut range = draw.instance..draw.instance + 1;
            let deformation = self.geometry.deformation_for(draw.instance, output.raster);
            if deformation.is_none()
                && let Some(order) = object.instance_order()
            {
                let offset = self.geometry.offset(draw.instance);
                while draws.peek().is_some_and(|next| {
                    if next.instance != range.end || next.part != draw.part {
                        return false;
                    }
                    let next_object = &self.objects[next.instance as usize];
                    next_object.mesh == object.mesh
                        && next_object.is_reflected() == object.is_reflected()
                        && next_object
                            .instance_order()
                            .is_some_and(|next| next.group == order.group)
                        && self.geometry.deformation(next.instance).is_none()
                        && self.geometry.offset(next.instance) == offset
                        && reflections.local.is_none_or(|local| {
                            local.slot(next.instance) == local.slot(draw.instance)
                        })
                }) {
                    range.end += 1;
                    draws.next();
                }
            }
            stats.submit(&range, &part.vertices);
            reflections.bind(
                pass,
                &mut active_environment,
                Some(draw.instance),
                part.material.reflection_map.as_ref(),
            );
            draw_part(
                device,
                pipelines,
                pass,
                &mut active,
                (mesh, part),
                (range, deformation),
                ColorOutput {
                    reverse_winding: output.reverse_winding ^ object.is_reflected(),
                    ..output
                },
            );
        }
        reflections.restore(pass, &active_environment);
    }
}

fn transparency_weight_scale(camera: &SceneCamera) -> f32 {
    let [near, far] = camera.depth_limits();
    // Weighting remains bounded when the projection reaches infinity.
    let far = if far.is_finite() {
        far
    } else {
        (near + 100.0).max(100.0)
    };
    (near.max(0.001).sqrt() * far.sqrt()).recip()
}

fn linked_texture_budget(
    device: &wgpu::Device,
    objects: Option<&Arc<[SceneObject]>>,
    visibility: &SceneVisibility,
    particles: Option<&ParticleDraw<'_>>,
) -> bool {
    let limits = device.limits();
    if limits.max_storage_buffers_per_shader_stage < 5 + material::SCALAR_MAP_SLOTS {
        return false;
    }
    let slots = limits.max_sampled_textures_per_shader_stage;
    if slots > material::FULL_MATERIAL_TEXTURES {
        return true;
    }
    let fits = |material| material::capture_material_texture_count(device, material) < slots;
    objects.is_none_or(|objects| {
        objects.iter().enumerate().all(|(index, object)| {
            !visibility.contains(index as u32)
                || object
                    .mesh
                    .materials()
                    .iter()
                    .filter(|material| material.is_blended())
                    .all(fits)
        })
    }) && particles.is_none_or(|draw| {
        draw.model_meshes()
            .all(|mesh| mesh.materials().iter().all(fits))
    })
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

fn set_inline_rect(pass: &mut wgpu::RenderPass<'_>, [x, y]: [u32; 2], [w, h]: [u32; 2]) {
    pass.set_viewport(x as f32, y as f32, w as f32, h as f32, 0.0, 1.0);
    pass.set_scissor_rect(x, y, w, h);
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
    crate::viewport_gpu_timing::render_pass(
        encoder,
        &wgpu::RenderPassDescriptor {
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
        },
    )
}

fn draw_model_particles(
    reflections: ColorEnvironments<'_>,
    (batches, zero_data): (&[ModelParticleBatch], &wgpu::Buffer),
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
        lod_fade: false,
        ..output
    };
    let mut active = None;
    let mut active_environment = reflections.fallback.clone();
    for batch in batches {
        deform::bind_instance_streams(pass, &batch.instances, zero_data);
        for prototype in batch.meshes.iter() {
            let mesh = meshes.get_for_raster(device, queue, prototype, output.raster);
            mesh.bind(pass);
            for part in mesh.parts.iter().filter(|part| part.visible) {
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
                }
                if output.capture_geometry.is_none_or(GeometryOutput::combined) {
                    stats.submit(&range, &part.vertices);
                    stats.particle_model_draws += 1;
                }
                reflections.bind(
                    pass,
                    &mut active_environment,
                    None,
                    part.material.reflection_map.as_ref(),
                );
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
    reflections.restore(pass, &active_environment);
}

#[derive(Clone, Copy, PartialEq)]
struct MeshPipelineKey {
    surface: Surface,
    deformation: Deformation,
    lightmapped: bool,
    reverse_winding: bool,
    shader: Option<(
        usize,
        usize,
        xrack_runtime::scene_effect::ShaderTextureKinds,
    )>,
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
    if !part.visible {
        return;
    }
    let mode = deformation.map_or(Deformation::Rigid, |binding| binding.mode);
    let binormals = if !part.surface.vertex_tangents {
        BinormalSource::Derived
    } else if mode.morph() && mesh.morph.as_ref().is_some_and(|morph| morph.binormals) {
        BinormalSource::Morph
    } else if mode == Deformation::Rigid && mesh.binormals.is_some() {
        BinormalSource::Vertex
    } else {
        BinormalSource::Derived
    };
    let surface = Surface {
        blend: part.surface.blend || output.model_particles && output.transparent(),
        binormals,
        ..part.surface
    };
    let shader = part
        .material
        .program(output.writes_material_data() && pipelines.capture_augment.is_none());
    let selection = MeshPipelineKey {
        surface,
        deformation: mode,
        lightmapped: output.lightmapped,
        reverse_winding: output.reverse_winding,
        shader: shader.map(|program| {
            (
                Arc::as_ptr(program.source) as *const () as usize,
                program.slots,
                program.kinds,
            )
        }),
    };
    if *active != Some(selection) {
        pass.set_pipeline(pipelines.get(device, surface, output, selection.deformation, shader));
        *active = Some(selection);
    }
    if let Some(binding) = deformation {
        pass.set_bind_group(3, &binding.group, &[]);
    }
    pass.set_bind_group(
        1,
        part.material
            .color(shader.map_or(0, |program| program.slots)),
        &[],
    );
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
    let vertex_usage = if mesh.model_blend().is_some() {
        wgpu::BufferUsages::VERTEX | wgpu::BufferUsages::STORAGE
    } else {
        wgpu::BufferUsages::VERTEX
    };
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
        vertices: buffer(device, "XRack 3D unit mesh", &bytes, vertex_usage),
        skin_weights: mesh.influences().map(|weights| {
            let packed: Vec<_> = mesh_rows(weights, compact)
                .copied()
                .flat_map(|weight| weight.packed())
                .collect();
            buffer(
                device,
                "XRack joint weights",
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
                    "XRack morph deltas",
                    &bytes,
                    wgpu::BufferUsages::STORAGE,
                ),
                table_layout: [count as u32 + 1, geometry.record_words() as u32],
                binormals: geometry.has_binormals(),
            }
        }),
        tangents: (!mesh.tangents().is_empty()).then(|| {
            let bytes: Vec<_> = mesh_rows(mesh.tangents(), compact)
                .flatten()
                .copied()
                .flat_map(i16::to_ne_bytes)
                .collect();
            buffer(device, "XRack 3D mesh tangents", &bytes, vertex_usage)
        }),
        binormals: (!mesh.binormals().is_empty()).then(|| {
            let bytes: Vec<_> = mesh_rows(mesh.binormals(), compact)
                .flatten()
                .copied()
                .flat_map(i16::to_ne_bytes)
                .collect();
            buffer(device, "XRack 3D mesh bitangents", &bytes, vertex_usage)
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

pub(super) fn prepare_inline_geometry(
    node: &Node,
    camera: &SceneCamera,
    format: wgpu::TextureFormat,
) -> Option<Arc<SceneGeometry>> {
    if !matches!(
        format,
        wgpu::TextureFormat::Rgba8Unorm
            | wgpu::TextureFormat::Bgra8Unorm
            | wgpu::TextureFormat::Rgba8UnormSrgb
            | wgpu::TextureFormat::Bgra8UnormSrgb
    ) || background(node).0 as u8 != 255
        || [
            "multisampling",
            "cache-rendering",
            "ambient-occlusion",
            "screen-space-reflections",
            "screen-space-global-illumination",
            "order-independent-transparency",
        ]
        .into_iter()
        .any(|name| node.boolean(name, false))
        || matches!(node.text("oit-method"), Some("weighted" | "linked-list"))
        || retained_view::Sampling::from_node(node).is_active()
        || node.float("supersampling").is_some_and(|scale| scale > 1.0)
        || node.properties.contains_key("__xr-physics-debug-lines")
        || shadow_debug::Settings::from_node(node).active()
        || matches!(node.properties.get("scene-captures"), Some(Value::SceneCaptures(captures)) if !captures.is_empty())
        || matches!(node.properties.get("particle-emitters"), Some(Value::SceneParticleEmitters(emitters)) if !emitters.is_empty())
        || matches!(node.properties.get("fog-volumes"), Some(Value::SceneFogVolumes(volumes)) if !volumes.is_empty())
    {
        return None;
    }
    let environment = EnvironmentInput::from_node(node);
    let hdr_environment = environment.is_some()
        || SkyboxInput::from_node(node, environment.as_ref()).is_some()
        || matches!(node.properties.get("reflection-probes"), Some(Value::SceneReflections(probes)) if !probes.is_empty());
    if PostSettings::from_node(node, hdr_environment, true, false).needs_target() {
        return None;
    }
    let Some(Value::SceneObjects(model)) = node.properties.get("model") else {
        return None;
    };
    let geometry = model.render_geometry(
        camera,
        f64::from(node.length("lod-fade-width").unwrap_or_default()),
    );
    (!geometry.lod_fade
        && geometry.objects().iter().all(|object| {
            let mesh = &object.mesh;
            mesh.transmission_blur().is_none()
                && mesh.depth_participation().0 == [false; 2]
                && (format.is_srgb() || mesh.blended_parts().is_empty())
        }))
    .then_some(geometry)
}

pub(super) fn background(node: &Node) -> Color {
    if node.scene_internal_passes_disabled() {
        Color(0)
    } else {
        node.color("scene-background").unwrap_or(Color(0x202a38ff))
    }
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
