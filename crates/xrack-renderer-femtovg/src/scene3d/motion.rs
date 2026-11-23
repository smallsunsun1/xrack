//! Capture previous-frame coordinates for the visible 3D surfaces.
use std::{collections::HashMap, sync::Arc};

use xrack_runtime::scene3d::{Mesh3D, SceneCamera, SceneObject};

use super::{
    Attachment, DEPTH_FORMAT, HDR_FORMAT, MeshCache, MeshColorDraws, append_floats,
    attachment_view, buffer,
    deform::{Deformation, DeformationBinding, DeformationLayouts, SKIN_LAYOUT, shader_source},
    pipeline::GEOMETRY_LAYOUTS,
};

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
        (draws, objects, camera): (&MeshColorDraws<'_>, &Arc<[SceneObject]>, SceneCamera),
        history: &mut MotionHistory,
    ) -> (MotionInputs, u64) {
        let device = ctx.device;
        // A supersampled color image resolves to this grid before temporal filtering.
        let size = ctx.size;
        let previous = history.last.take().filter(|frame| frame.size == size);
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
        let mut uniforms = Vec::with_capacity(176);
        for projection in [camera, previous_camera] {
            append_floats(&mut uniforms, projection.clip_rows().into_iter().flatten());
            append_floats(&mut uniforms, projection.depth_row());
        }
        append_floats(&mut uniforms, [size[0] as f32, size[1] as f32, 0.0, 0.0]);
        let uniform = buffer(
            device,
            "XRack motion cameras",
            &uniforms,
            wgpu::BufferUsages::UNIFORM,
        );
        let mut transforms = Vec::with_capacity(objects.len() * 64);
        for (object, index) in objects.iter().zip(&correspondences) {
            let old = index.and_then(|index| previous.as_ref()?.objects.get(index));
            append_floats(
                &mut transforms,
                old.unwrap_or(object).transform_rows().into_iter().flatten(),
            );
            transforms.extend(
                [index.map_or(0, |index| index as u32 + 1), 0, 0, 0]
                    .into_iter()
                    .flat_map(u32::to_ne_bytes),
            );
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
                    resource: uniform.as_entire_binding(),
                },
                wgpu::BindGroupEntry {
                    binding: 1,
                    resource: instances.as_entire_binding(),
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
        let mut encoder = ctx.encoder.begin_render_pass(&wgpu::RenderPassDescriptor {
            label: Some("XRack 3D motion vectors"),
            color_attachments: &colors,
            depth_stencil_attachment: Some(wgpu::RenderPassDepthStencilAttachment {
                view: depth,
                depth_ops: Some(wgpu::Operations {
                    load: wgpu::LoadOp::Clear(1.0),
                    store: wgpu::StoreOp::Discard,
                }),
                stencil_ops: None,
            }),
            ..Default::default()
        });
        encoder.set_bind_group(0, &globals, &[]);
        encoder.set_vertex_buffer(1, draws.set.buffer.slice(..));
        let mut active = None;
        let mut count = 0;
        for phase in [Phase::Opaque, Phase::Refraction, Phase::Translucent] {
            for batch in &draws.set.draws {
                let source = batch.start;
                let kind = &objects[source as usize].mesh;
                let draws_phase = match phase {
                    Phase::Opaque => kind.has_opaque_parts(),
                    Phase::Refraction => kind.has_depth_writing_transmission(),
                    Phase::Translucent => !kind.blended_parts().is_empty(),
                };
                if !draws_phase || draws.visibility.runs(batch.clone()).next().is_none() {
                    continue;
                }
                let mesh = meshes.get(device, queue, kind);
                let parts = mesh.parts.iter().filter(|part| {
                    let surface = part.surface;
                    match phase {
                        Phase::Opaque => !surface.blend && !surface.transmission,
                        Phase::Refraction => !surface.blend && surface.transmission,
                        Phase::Translucent => surface.blend,
                    }
                });
                let current = draws.geometry.deformation(source);
                let mode = current.map_or(Deformation::Rigid, |binding| binding.mode);
                encoder.set_vertex_buffer(0, mesh.vertices.slice(..));
                if mode.skin() {
                    mesh.bind_skin(&mut encoder);
                }
                if let Some(current) = current {
                    let old = correspondences[source as usize]
                        .and_then(|index| previous.as_ref()?.deformations.get(index)?.as_ref())
                        .filter(|binding| binding.mode == mode)
                        .unwrap_or(current);
                    encoder.set_bind_group(2, &old.group, &[]);
                    encoder.set_bind_group(3, &current.group, &[]);
                }
                for part in parts {
                    let key = Variant {
                        phase,
                        mode,
                        coverage: part.mask || part.surface.blend,
                        double_sided: part.surface.double_sided,
                    };
                    if active != Some(key) {
                        encoder.set_pipeline(pipelines.get(device, key));
                        active = Some(key);
                    }
                    encoder.set_bind_group(1, &part.material.base, &[]);
                    for range in draws.visibility.runs(batch.clone()) {
                        encoder.draw(part.vertices.clone(), range);
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
            deformations: draws.geometry.deformations.to_vec(),
            size,
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
    fn key(object: &SceneObject) -> (&str, usize) {
        let asset = match &object.mesh {
            Mesh3D::Box => 0,
            Mesh3D::Sphere => 1,
            Mesh3D::Imported(mesh) => Arc::as_ptr(mesh) as usize,
        };
        (&object.id, asset)
    }
    fn unique(objects: &[SceneObject]) -> HashMap<(&str, usize), Option<usize>> {
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
                && object.morph_weights().is_some() == other.morph_weights().is_some())
            .then_some(index)
        })
        .collect()
}

#[derive(Clone, Copy, PartialEq)]
enum Phase {
    Opaque,
    Refraction,
    Translucent,
}

#[derive(Clone, Copy, PartialEq)]
struct Variant {
    phase: Phase,
    mode: Deformation,
    coverage: bool,
    double_sided: bool,
}

struct MotionPipelines {
    globals: wgpu::BindGroupLayout,
    layouts: [wgpu::PipelineLayout; 4],
    shader: wgpu::ShaderModule,
    variants: Vec<(Variant, wgpu::RenderPipeline)>,
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
            ],
        });
        let layouts = Deformation::ALL.map(|mode| {
            let deformation = deformations.get(mode);
            let groups = [Some(&globals), Some(materials), deformation, deformation];
            device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
                label: Some("XRack motion pipeline layout"),
                bind_group_layouts: &groups[..if deformation.is_some() { 4 } else { 2 }],
                immediate_size: 0,
            })
        });
        let shader = device.create_shader_module(wgpu::ShaderModuleDescriptor {
            label: Some("XRack motion shader"),
            source: shader_source(include_str!("motion.wgsl")),
        });
        Self {
            globals,
            layouts,
            shader,
            variants: Vec::new(),
        }
    }

    fn get(&mut self, device: &wgpu::Device, variant: Variant) -> &wgpu::RenderPipeline {
        let index = self
            .variants
            .iter()
            .position(|(key, _)| *key == variant)
            .unwrap_or_else(|| {
                let buffers = [
                    GEOMETRY_LAYOUTS[0].clone(),
                    GEOMETRY_LAYOUTS[1].clone(),
                    None,
                    SKIN_LAYOUT,
                ];
                let targets = [HDR_FORMAT, ID_FORMAT].map(|format| {
                    Some(wgpu::ColorTargetState {
                        format,
                        blend: None,
                        write_mask: wgpu::ColorWrites::ALL,
                    })
                });
                let pipeline = device.create_render_pipeline(&wgpu::RenderPipelineDescriptor {
                    label: Some("XRack motion vector pipeline"),
                    layout: Some(&self.layouts[variant.mode as usize]),
                    vertex: wgpu::VertexState {
                        module: &self.shader,
                        entry_point: Some(match variant.mode {
                            Deformation::Rigid => "rigid",
                            Deformation::Skin => "skinned",
                            Deformation::Morph => "morphed",
                            Deformation::SkinMorph => "skinned_morphed",
                        }),
                        compilation_options: Default::default(),
                        buffers: if variant.mode.skin() {
                            &buffers
                        } else {
                            &GEOMETRY_LAYOUTS
                        },
                    },
                    fragment: Some(wgpu::FragmentState {
                        module: &self.shader,
                        entry_point: Some("fragment"),
                        compilation_options: wgpu::PipelineCompilationOptions {
                            constants: &[
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
                    primitive: wgpu::PrimitiveState {
                        cull_mode: (!variant.double_sided).then_some(wgpu::Face::Back),
                        ..Default::default()
                    },
                    depth_stencil: Some(wgpu::DepthStencilState {
                        format: DEPTH_FORMAT,
                        depth_write_enabled: Some(variant.phase != Phase::Translucent),
                        depth_compare: Some(wgpu::CompareFunction::Less),
                        stencil: Default::default(),
                        bias: Default::default(),
                    }),
                    multisample: Default::default(),
                    multiview_mask: None,
                    cache: None,
                });
                let index = self.variants.len();
                self.variants.push((variant, pipeline));
                index
            });
        &self.variants[index].1
    }
}
