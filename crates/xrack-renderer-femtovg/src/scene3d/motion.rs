//! Capture previous-frame coordinates for the visible 3D surfaces.
use std::{collections::HashMap, num::NonZeroU64, sync::Arc};

use xrack_runtime::scene3d::{Mesh3D, MeshCullMode, MeshTopology, SceneCamera, SceneObject};

use super::{
    Attachment, DEPTH_FORMAT, HDR_FORMAT, MeshCache, MeshColorDraws, append_floats,
    attachment_view, buffer,
    capture_region::CaptureRegion,
    deform::{Deformation, DeformationBinding, DeformationLayouts, SKIN_LAYOUT},
    pipeline::{GEOMETRY_LAYOUTS, INSTANCE_DATA_LAYOUT, mesh_primitive, orient_depth_compare},
    vertex_program::VertexPipelines,
    wireframe::{self, RasterMode},
};

const CAMERA_BYTES: u64 = 176;
const VERTEX_PARAMETERS: u64 = 16;
const ID_FORMAT: wgpu::TextureFormat = wgpu::TextureFormat::R32Uint;
const USAGE: wgpu::TextureUsages =
    wgpu::TextureUsages::RENDER_ATTACHMENT.union(wgpu::TextureUsages::TEXTURE_BINDING);

#[derive(Default)]
pub(super) struct MotionHistory {
    last: Option<MotionFrame>,
    spare: Option<wgpu::TextureView>,
}

struct MotionFrame {
    ids: wgpu::TextureView,
    camera: SceneCamera,
    objects: Arc<[SceneObject]>,
    deformations: Vec<Option<DeformationBinding>>,
    size: [u32; 2],
    raster: RasterMode,
    region: CaptureRegion,
}

pub(super) struct MotionInputs {
    pub vectors: wgpu::TextureView,
    pub ids: wgpu::TextureView,
    pub previous_ids: wgpu::TextureView,
    pub instances: wgpu::Buffer,
}

#[derive(Default)]
pub(super) struct MotionPass {
    pipelines: Option<MotionPipelines>,
    vectors: Option<Attachment>,
    depth: Option<Attachment>,
}

impl MotionPass {
    pub fn record(
        &mut self,
        ctx: &mut crate::GpuPaintContext<'_>,
        queue: &wgpu::Queue,
        (meshes, materials, layouts): (&mut MeshCache, &wgpu::BindGroupLayout, &DeformationLayouts),
        (draws, objects, camera, raster, region): (
            &MeshColorDraws<'_>,
            &Arc<[SceneObject]>,
            SceneCamera,
            RasterMode,
            CaptureRegion,
        ),
        history: &mut MotionHistory,
    ) -> (MotionInputs, u64) {
        let device = ctx.device;
        let reverses_winding = camera.reverses_winding();
        let reverse_depth = camera.reverse_depth();
        // A supersampled color image resolves to this grid before temporal filtering.
        let size = ctx.size;
        let previous = history
            .last
            .take()
            .filter(|frame| frame.size == size && frame.raster == raster && frame.region == region);
        let ids = history.spare.take().unwrap_or_else(|| {
            attachment_view(
                device,
                size,
                ID_FORMAT,
                1,
                USAGE,
                "XRack motion surface IDs",
            )
        });
        history.spare = previous.as_ref().map(|frame| frame.ids.clone());
        let correspondences =
            correspondences(objects, previous.as_ref().map(|frame| &frame.objects));
        let previous_camera = previous.as_ref().map_or(camera, |frame| frame.camera);
        let mut uniforms = Vec::with_capacity(CAMERA_BYTES as usize);
        for projection in [camera, previous_camera] {
            append_floats(&mut uniforms, projection.clip_rows().into_iter().flatten());
            append_floats(&mut uniforms, projection.depth_row());
        }
        append_floats(&mut uniforms, [size[0] as f32, size[1] as f32, 0.0, 0.0]);
        let alignment = device.limits().min_uniform_buffer_offset_alignment as usize;
        let parameters_start = uniforms.len().next_multiple_of(alignment);
        uniforms.resize(parameters_start + VERTEX_PARAMETERS as usize, 0);
        let parameter_offsets = append_previous_parameters(
            &mut uniforms,
            alignment,
            parameters_start,
            previous.as_ref().map(|frame| frame.objects.as_ref()),
        );
        let uniform = buffer(
            device,
            "XRack motion cameras",
            &uniforms,
            wgpu::BufferUsages::UNIFORM,
        );
        let mut transforms = Vec::with_capacity(objects.len() * 80);
        for (object, index) in objects.iter().zip(&correspondences) {
            let old = index
                .and_then(|index| previous.as_ref()?.objects.get(index))
                .unwrap_or(object);
            append_floats(&mut transforms, old.transform_rows().into_iter().flatten());
            transforms.extend(
                [
                    index.map_or(0, |index| index as u32 + 1),
                    (old.shader_instance_index() << 1) | u32::from(object.motion_vector_enabled()),
                    object.motion_vector_scale().to_bits(),
                    old.color.0,
                ]
                .into_iter()
                .flat_map(u32::to_ne_bytes),
            );
            append_floats(&mut transforms, old.instance_data());
        }
        let instances = buffer(
            device,
            "XRack previous 3D transforms",
            &transforms,
            wgpu::BufferUsages::STORAGE,
        );
        let pipelines = self
            .pipelines
            .get_or_insert_with(|| MotionPipelines::new(device, materials, layouts));
        let globals = device.create_bind_group(&wgpu::BindGroupDescriptor {
            label: Some("XRack motion cameras and transforms"),
            layout: &pipelines.globals,
            entries: &[
                wgpu::BindGroupEntry {
                    binding: 0,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniform,
                        offset: 0,
                        size: NonZeroU64::new(CAMERA_BYTES),
                    }),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: instances.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 2,
                    resource: wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                        buffer: &uniform,
                        offset: parameters_start as u64,
                        size: NonZeroU64::new(VERTEX_PARAMETERS),
                    }),
                },
            ],
        });
        let vectors = Attachment::get(&mut self.vectors, device, size, HDR_FORMAT, 1, true);
        let depth = Attachment::get(&mut self.depth, device, size, DEPTH_FORMAT, 1, false);
        let colors = [vectors, &ids].map(|view| {
            Some(wgpu::RenderPassColorAttachment {
                view,
                depth_slice: None,
                resolve_target: None,
                ops: wgpu::Operations {
                    load: wgpu::LoadOp::Clear(wgpu::Color::TRANSPARENT),
                    store: wgpu::StoreOp::Store,
                },
            })
        });
        let mut encoder = crate::viewport_gpu_timing::render_pass(
            ctx.encoder,
            &wgpu::RenderPassDescriptor {
                label: Some("XRack 3D motion vectors"),
                color_attachments: &colors,
                depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                    view: depth,
                    depth_ops: Some(wgpu::Operations {
                        load: wgpu::LoadOp::Clear(camera.depth_clear()),
                        store: wgpu::StoreOp::Discard,
                    }),
                    stencil_ops: None,
                }),
                ..Default::default()
            },
        );
        region.clip(&mut encoder);
        encoder.set_bind_group(0, &globals, &[0]);
        let mut active_parameters = 0;
        let mut bound_instances = None;
        let mut active = None;
        let mut count = 0;
        for phase in [Phase::Opaque, Phase::Refraction, Phase::Translucent] {
            for batch in &draws.set.draws {
                let source = batch.start;
                let object = &objects[source as usize];
                let kind = &object.mesh;
                let draws_phase = match phase {
                    Phase::Opaque => kind.has_opaque_parts(),
                    Phase::Refraction => kind.has_depth_writing_transmission(),
                    Phase::Translucent => !kind.blended_parts().is_empty(),
                };
                if !draws_phase || draws.visibility.runs(batch.clone()).next().is_none() {
                    continue;
                }
                draws
                    .geometry
                    .bind_instance_range(&mut encoder, &mut bound_instances, source);
                let mesh = meshes.get_for_raster(device, queue, kind, raster);
                let parts = mesh.parts.iter().enumerate().filter(|(_, part)| {
                    let surface = part.surface;
                    part.visible
                        && match phase {
                            Phase::Opaque => !surface.blend && !surface.transmission,
                            Phase::Refraction => !surface.blend && surface.transmission,
                            Phase::Translucent => surface.blend,
                        }
                });
                let current = draws.geometry.deformation_for(source, raster);
                let mode = current.map_or(Deformation::Rigid, |binding| binding.mode);
                mesh.bind_geometry(&mut encoder);
                if mode.skin() {
                    mesh.bind_skin(&mut encoder);
                }
                if let Some(current) = current {
                    let old = correspondences[source as usize]
                        .and_then(|index| previous.as_ref()?.deformations.get(index)?.as_ref())
                        .filter(|binding| binding.mode == mode)
                        .unwrap_or(current);
                    encoder.set_bind_group(2, &old.group, &[]);
                }
                let mut bound_wide = None;
                for (index, part) in parts {
                    let key = Variant {
                        wide_points: part.surface.wide_points,
                        wide_lines: part.surface.wide_lines,
                        topology: part.surface.topology,
                        phase,
                        mode,
                        coverage: part.mask || part.surface.blend,
                        cull: part.surface.cull,
                        reflected: object.is_reflected() ^ reverses_winding,
                        reverse_depth,
                        lod_fade: draws.geometry.lod_fade,
                        raster: raster.for_topology(part.surface.topology),
                    };
                    let program = part.material.vertex.as_ref();
                    let selection = (
                        key,
                        program.map(|source| Arc::as_ptr(source) as *const () as usize),
                    );
                    if active != Some(selection) {
                        encoder.set_pipeline(pipelines.get(device, key, program));
                        active = Some(selection);
                    }
                    if bound_wide != Some(key.wide_lines) {
                        mesh.bind_deformation(&mut encoder, current, key.wide_lines);
                        bound_wide = Some(key.wide_lines);
                    }
                    let offset = program
                        .and_then(|_| {
                            let prior = correspondences[source as usize]?;
                            let mesh = &previous.as_ref()?.objects.get(prior)?.mesh;
                            let material = mesh.parts().get(index)?.material;
                            let parameters = mesh.materials().get(material)?.vertex_parameters();
                            parameter_offsets
                                .get(&parameters.map(f32::to_bits))
                                .copied()
                        })
                        .unwrap_or(0);
                    if active_parameters != offset {
                        encoder.set_bind_group(0, &globals, &[offset]);
                        active_parameters = offset;
                    }
                    encoder.set_bind_group(1, &part.material.base, &[]);
                    for range in draws.visibility.runs(batch.clone()) {
                        mesh.draw_range(&mut encoder, part.vertices.clone(), range);
                        count += 1;
                    }
                }
            }
        }
        drop(encoder);
        let previous_ids = previous.map_or_else(|| ids.clone(), |frame| frame.ids);
        history.last = Some(MotionFrame {
            ids: ids.clone(),
            camera,
            objects: objects.clone(),
            deformations: draws.geometry.deformations_for(raster).to_vec(),
            size,
            raster,
            region,
        });
        (
            MotionInputs {
                vectors: vectors.clone(),
                ids,
                previous_ids,
                instances,
            },
            count,
        )
    }
}

fn append_previous_parameters(
    bytes: &mut Vec<u8>,
    align: usize,
    start: usize,
    objects: Option<&[SceneObject]>,
) -> HashMap<[u32; 4], u32> {
    let mut offsets = HashMap::new();
    for object in objects.into_iter().flatten() {
        if object.mesh.shader_displacement() == 0.0 {
            continue;
        }
        for material in object.mesh.materials() {
            let parameters = material.vertex_parameters();
            if parameters[3] == 0.0 {
                continue;
            }
            offsets
                .entry(parameters.map(f32::to_bits))
                .or_insert_with(|| {
                    let offset = bytes.len().next_multiple_of(align);
                    bytes.resize(offset, 0);
                    append_floats(bytes, parameters);
                    (offset - start) as u32
                });
        }
    }
    offsets
}

fn same_vertex_programs(current: &Mesh3D, previous: &Mesh3D) -> bool {
    if let (Mesh3D::Imported(current_data), Mesh3D::Imported(previous_data)) = (current, previous)
        && (current_data.wide_points().is_some()
            || previous_data.wide_points().is_some()
            || current_data.wide_lines().is_some()
            || previous_data.wide_lines().is_some())
        && (current_data.wide_points() != previous_data.wide_points()
            || current_data.wide_lines() != previous_data.wide_lines()
            || current
                .materials()
                .iter()
                .map(|m| (m.point_size, m.line_width))
                .ne(previous
                    .materials()
                    .iter()
                    .map(|m| (m.point_size, m.line_width))))
    {
        return false;
    }
    if current.shader_displacement() == 0.0 && previous.shader_displacement() == 0.0 {
        return true;
    }
    current.parts().len() == previous.parts().len()
        && current.sections().zip(previous.sections()).all(
            |((current, material), (previous, old))| {
                current == previous
                    && material.and_then(|material| material.vertex_program())
                        == old.and_then(|material| material.vertex_program())
            },
        )
}

// Imported scenes may place the same mesh more than once. Preserve identity
// through reordering when the authored name and mesh identify a unique object.
fn correspondences(
    current: &Arc<[SceneObject]>,
    previous: Option<&Arc<[SceneObject]>>,
) -> Vec<Option<usize>> {
    let Some(previous) = previous else {
        return vec![None; current.len()];
    };
    if Arc::ptr_eq(current, previous) {
        return (0..current.len()).map(Some).collect();
    }
    type Identity<'a> = (&'a str, usize, Option<(u32, Option<u32>)>);
    fn key(object: &SceneObject) -> Identity<'_> {
        let asset = match &object.mesh {
            Mesh3D::Box => 0,
            Mesh3D::Sphere => 1,
            Mesh3D::Imported(mesh) => Arc::as_ptr(mesh.geometry()) as usize,
        };
        (&object.id, asset, object.imported_placement())
    }
    fn unique(objects: &[SceneObject]) -> HashMap<Identity<'_>, Option<usize>> {
        let mut indices = HashMap::with_capacity(objects.len());
        for (index, object) in objects
            .iter()
            .enumerate()
            .filter(|(_, object)| !object.id.is_empty())
        {
            indices
                .entry(key(object))
                .and_modify(|slot| *slot = None)
                .or_insert(Some(index));
        }
        indices
    }
    let old = unique(previous);
    let now = unique(current);
    current
        .iter()
        .map(|object| {
            let identity = key(object);
            now.get(&identity).copied().flatten()?;
            let index = old.get(&identity).copied().flatten()?;
            let other = &previous[index];
            (object.skin_palette().is_some() == other.skin_palette().is_some()
                && object.morph_weights().is_some() == other.morph_weights().is_some()
                && same_vertex_programs(&object.mesh, &other.mesh))
            .then_some(index)
        })
        .collect()
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
enum Phase {
    Opaque,
    Refraction,
    Translucent,
}

#[derive(Clone, Copy, Eq, Hash, PartialEq)]
struct Variant {
    wide_points: bool,
    wide_lines: bool,
    topology: MeshTopology,
    phase: Phase,
    mode: Deformation,
    coverage: bool,
    cull: MeshCullMode,
    reflected: bool,
    reverse_depth: bool,
    lod_fade: bool,
    raster: RasterMode,
}

struct MotionPipelines {
    globals: wgpu::BindGroupLayout,
    layouts: [wgpu::PipelineLayout; 8],
    wide_pipelines: [Option<VertexPipelines<Variant>>; 4],
    pipelines: VertexPipelines<Variant>,
    wire_pipelines: Option<VertexPipelines<Variant>>,
}

impl MotionPipelines {
    fn new(
        device: &wgpu::Device,
        materials: &wgpu::BindGroupLayout,
        deformations: &DeformationLayouts,
    ) -> Self {
        let layout_buffer = |binding, ty, visibility| wgpu::BindGroupLayoutEntry {
            binding,
            visibility,
            ty: wgpu::BindingType::Buffer {
                ty,
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        };
        let globals = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
            label: Some("XRack motion layout"),
            entries: &[
                layout_buffer(
                    0,
                    wgpu::BufferBindingType::Uniform,
                    wgpu::ShaderStages::VERTEX_FRAGMENT,
                ),
                layout_buffer(
                    1,
                    wgpu::BufferBindingType::Storage { read_only: true },
                    wgpu::ShaderStages::VERTEX,
                ),
                wgpu::BindGroupLayoutEntry {
                    binding: 2,
                    visibility: wgpu::ShaderStages::VERTEX,
                    ty: wgpu::BindingType::Buffer {
                        ty: wgpu::BufferBindingType::Uniform,
                        has_dynamic_offset: true,
                        min_binding_size: NonZeroU64::new(VERTEX_PARAMETERS),
                    },
                    count: None,
                },
            ],
        });
        let layouts = std::array::from_fn(|index| {
            let (mode, wide) = Deformation::at_pipeline(index);
            let previous = deformations.get(mode);
            let current = deformations.get_for_line(mode, wide);
            let groups = [Some(&globals), Some(materials), previous, current];
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack motion pipeline layout"),
                bind_group_layouts: &groups[..if current.is_some() { 4 } else { 2 }],
                immediate_size: 0,
            })
        });
        Self {
            globals,
            layouts,
            pipelines: VertexPipelines::new(device, include_str!("motion.wgsl")),
            wire_pipelines: None,
            wide_pipelines: Default::default(),
        }
    }

    fn get(
        &mut self,
        device: &wgpu::Device,
        variant: Variant,
        program: Option<&Arc<str>>,
    ) -> &wgpu::RenderPipeline {
        let pipelines = if variant.wide_lines {
            self.wide_pipelines[variant.mode as usize].get_or_insert_with(|| {
                VertexPipelines::new(
                    device,
                    super::primitive_raster::line_shader(include_str!("motion.wgsl"), variant.mode),
                )
            })
        } else if variant.raster == RasterMode::ShaderLine {
            self.wire_pipelines.get_or_insert_with(|| {
                VertexPipelines::new(device, wireframe::mesh(include_str!("motion.wgsl")))
            })
        } else {
            &mut self.pipelines
        };
        pipelines.get(device, program, variant, |module| {
            let buffers = [
                GEOMETRY_LAYOUTS[0].clone(),
                GEOMETRY_LAYOUTS[1].clone(),
                None,
                SKIN_LAYOUT.filter(|_| variant.mode.skin()),
                INSTANCE_DATA_LAYOUT,
            ];
            let targets = [HDR_FORMAT, ID_FORMAT].map(|format| {
                Some(wgpu::ColorTargetState {
                    format,
                    blend: None,
                    write_mask: wgpu::ColorWrites::ALL,
                })
            });
            device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                label: Some("XRack motion vector pipeline"),
                layout: Some(&self.layouts[variant.mode.pipeline_index(variant.wide_lines)]),
                vertex: wgpu::VertexState {
                    module,
                    entry_point: Some(match variant.mode {
                        Deformation::Rigid => "rigid",
                        Deformation::Skin => "skinned",
                        Deformation::Morph => "morphed",
                        Deformation::SkinMorph => "skinned_morphed",
                    }),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[("WIDE_POINTS", f64::from(variant.wide_points))],
                        ..Default::default()
                    },
                    buffers: &buffers,
                },
                fragment: Some(wgpu::FragmentState {
                    module,
                    entry_point: Some("fragment"),
                    compilation_options: wgpu::PipelineCompilationOptions {
                        constants: &[
                            ("LOD_FADE", f64::from(variant.lod_fade)),
                            ("TEST_COVERAGE", if variant.coverage { 1.0 } else { 0.0 }),
                            (
                                "TRACK_SURFACE",
                                if variant.phase == Phase::Opaque {
                                    1.0
                                } else {
                                    0.0
                                },
                            ),
                        ],
                        ..Default::default()
                    },
                    targets: &targets,
                }),
                primitive: mesh_primitive(
                    variant.topology,
                    variant.wide_points || variant.wide_lines,
                    variant.raster,
                    variant.reflected,
                    variant.cull,
                ),
                depth_stencil: Some(wgpu::DepthStencilState {
                    format: DEPTH_FORMAT,
                    depth_write_enabled: Some(variant.phase != Phase::Translucent),
                    depth_compare: Some(orient_depth_compare(
                        wgpu::CompareFunction::Less,
                        variant.reverse_depth,
                    )),
                    stencil: Default::default(),
                    bias: Default::default(),
                }),
                multisample: Default::default(),
                multiview_mask: None,
                cache: None,
            })
        })
    }
}
